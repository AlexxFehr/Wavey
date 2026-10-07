//! Edge diffraction against the wave solver.

mod common;

use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;

use common::*;
use glam::{DMat3, DVec2, DVec3};
use num_complex::{Complex32, Complex64};
use wavey_geom::{Sketch, build_room};
use wavey_materials::Medium;
use wavey_solver::accumulate::{AccumulateJob, accumulate_cpu};
use wavey_solver::diffraction::{DiffractionSettings, EdgeGraph, NONE, trace_diffraction};
use wavey_solver::directivity::Directivity;
use wavey_solver::fdtd::*;
use wavey_solver::ism::{generate_images, trace_paths};
use wavey_solver::scene::{Spectra, SurfaceAcoustics};

/// A rigid 80 cm square pillar between source and a line of receivers that
/// crosses its shadow. The wave solver runs only until just before the first
/// reflection off the room's walls, floor or ceiling arrives, so both
/// methods see the pillar alone. Rigid surfaces make the image-source part
/// exact; the remaining difference is the diffraction model.
#[test]
fn pillar_shadow_matches_wave_solver() {
    let (lx, ly, h) = (13.0, 11.0, 7.0);
    let pc = DVec2::new(8.0, 5.5);
    let half = 0.4;
    let mut sk = Sketch::new();
    sk.add_polygon(&[DVec2::new(0.0, 0.0), DVec2::new(lx, 0.0), DVec2::new(lx, ly), DVec2::new(0.0, ly)]);
    sk.add_polygon(&[pc + DVec2::new(-half, -half), pc + DVec2::new(half, -half), pc + DVec2::new(half, half), pc + DVec2::new(-half, half)]);
    let room = build_room(&sk, h, &BTreeMap::from([(1, DVec2::new(1.0, 1.0))])).unwrap();

    let c = Medium::default().speed_of_sound();
    let mats = vec![material(0.0); room.surfaces.len()];
    let f_max = 800.0;
    let g = FdtdGrid::build(&room, &fit_surfaces(&mats, (20.0, f_max)), c, f_max, 10.0).unwrap();
    let pos = |i: usize| {
        let (z, r) = (i / (g.n[0] * g.n[1]), i % (g.n[0] * g.n[1]));
        g.center(r % g.n[0], r / g.n[0], z)
    };
    let src = g.nearest_air(DVec3::new(6.0, 5.65, 3.5)).unwrap();
    let rx: Vec<usize> = (0..25).map(|i| g.nearest_air(DVec3::new(10.0, 3.7 + 0.15 * i as f64, 3.5)).unwrap()).collect();
    // Earliest unwanted arrival: the floor/ceiling reflection (≥ 8 m path).
    let steps = (7.3 / c / g.dt) as usize;
    let signal = source_signal(f_max, g.dt, steps);
    let freqs = vec![250.0, 400.0, 600.0];
    let spec = run_cpu(&g, &[(src, 1.0)], &signal, &rx, &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
    let wave = to_transfer(&spec, &signal, &g, &freqs, c, 1.0, &vec![Complex64::new(1.0, 0.0); freqs.len()]);

    // Image sources: direct sound and reflections off the pillar only.
    let sp = pos(src);
    let all = generate_images(&room, sp, 1);
    let pillar = |s: u32| s != u32::MAX && room.surfaces[s as usize].signed_distance(pc.extend(h / 2.0)).abs() < half + 1e-6;
    let mut images = vec![all[0]];
    images.extend(all.iter().filter(|im| im.order == 1 && pillar(im.surface)).copied());
    let medium = Medium::default();
    let spectra = Spectra::new(freqs.clone(), &medium);
    let model = wavey_speaker::samples::omni();
    let directivity = Directivity::from_model(&model, &freqs);
    let surfaces: Vec<SurfaceAcoustics> = mats.iter().map(|m| SurfaceAcoustics::new(m, &freqs)).collect();
    let receivers: Vec<DVec3> = rx.iter().map(|&i| pos(i)).collect();
    let paths = trace_paths(&room, &images, DMat3::IDENTITY, &directivity, &receivers, &AtomicBool::new(false), &|_| {}).unwrap();
    let h0 = vec![Complex64::new(1.0, 0.0); freqs.len()];
    let job = AccumulateJob { paths: &paths, spectra: &spectra, h0: &h0, r_ref: 1.0, directivity: &directivity, surfaces: &surfaces };
    let geometric = accumulate_cpu(&job);

    let graph = EdgeGraph::build(&room);
    assert_eq!(graph.edges.len(), 4, "the four pillar corners");
    let with = |settings: DiffractionSettings| {
        let dpaths = trace_diffraction(&room, &graph, &images, DMat3::IDENTITY, &directivity, &receivers, settings, &AtomicBool::new(false)).unwrap();
        let mut total = geometric.clone();
        wavey_solver::diffraction::accumulate(&dpaths, &graph.edges, &job, &mut total);
        total
    };
    let single = with(DiffractionSettings { single_image_order: 1, chain_image_order: NONE, max_edges: 1, prune: None });
    let double = with(DiffractionSettings { single_image_order: 1, chain_image_order: 0, max_edges: 2, prune: None });
    let total = with(DiffractionSettings::default());
    // Pruning at the lowest frequency (−50 dB) must not change the result.
    let pruned = with(DiffractionSettings { prune: Some((2.0 * std::f64::consts::PI * 200.0 / c, 50.0)), ..Default::default() });
    let dev = total.iter().zip(&pruned).map(|(a, b)| ((a - b).norm() / a.norm().max(1e-12)) as f64).fold(0.0, f64::max);
    eprintln!("pruning: max relative change {dev:.2e}");
    assert!(dev < 0.01, "pruning changed the field by {dev}");

    let db = |a: Complex32, b: Complex64| 20.0 * (a.norm() as f64 / b.norm()).log10();
    let nf = freqs.len();
    let mut errs = Vec::new();
    let mut errs_geo = Vec::new();
    for (fi, f) in freqs.iter().enumerate() {
        eprintln!("{f} Hz: y, wave dB, minus wave: geometric, 1 edge, ≤2 edges, ≤3 edges");
        for (ri, r) in receivers.iter().enumerate() {
            let w = wave[ri * nf + fi];
            let e = |v: &[Complex32]| db(v[ri * nf + fi], w);
            let ed = e(&total);
            eprintln!("  {:5.2} {:7.1} {:+6.1} {:+6.2} {:+6.2} {:+6.2}", r.y, 20.0 * w.norm().log10(), e(&geometric), e(&single), e(&double), ed);
            errs.push(ed.abs());
            errs_geo.push(e(&geometric).abs());
        }
    }
    let stats = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        (v[v.len() / 2], v[v.len() * 9 / 10], v[v.len() - 1])
    };
    let (gm, g90, gmax) = stats(&mut errs_geo);
    let (dm, d90, dmax) = stats(&mut errs);
    eprintln!("geometric only:   median {gm:.2} dB, 90 % {g90:.2}, max {gmax:.2}");
    eprintln!("with diffraction: median {dm:.2} dB, 90 % {d90:.2}, max {dmax:.2}");
    // Measured: median 0.43 dB, 90 % 1.08 dB, max 1.95 dB (in a −22 dB
    // interference null). UTD is asymptotic; receivers here are only about
    // one wavelength behind the pillar at 250 Hz.
    assert!(dm < 0.6 && d90 < 1.3 && dmax < 2.5, "median {dm}, 90 % {d90}, max {dmax}");
}

/// The total field (geometric + diffracted) must be continuous where a
/// receiver crosses the line of a pillar face, where a single-diffracted
/// path is cut off and the double-diffracted one takes over.
#[test]
fn pillar_field_is_continuous_across_face_lines() {
    let (lx, ly, h) = (13.0, 11.0, 7.0);
    let pc = DVec2::new(8.0, 5.5);
    let half = 0.4;
    let mut sk = Sketch::new();
    sk.add_polygon(&[DVec2::new(0.0, 0.0), DVec2::new(lx, 0.0), DVec2::new(lx, ly), DVec2::new(0.0, ly)]);
    sk.add_polygon(&[pc + DVec2::new(-half, -half), pc + DVec2::new(half, -half), pc + DVec2::new(half, half), pc + DVec2::new(-half, half)]);
    let room = build_room(&sk, h, &BTreeMap::from([(1, DVec2::new(1.0, 1.0))])).unwrap();
    let sp = DVec3::new(6.0, 5.65, 3.5);
    let all = generate_images(&room, sp, 1);
    let pillar = |s: u32| s != u32::MAX && room.surfaces[s as usize].signed_distance(pc.extend(h / 2.0)).abs() < half + 1e-6;
    let mut images = vec![all[0]];
    images.extend(all.iter().filter(|im| im.order == 1 && pillar(im.surface)).copied());
    let freqs = vec![250.0, 600.0];
    let spectra = Spectra::new(freqs.clone(), &Medium::default());
    let model = wavey_speaker::samples::omni();
    let directivity = Directivity::from_model(&model, &freqs);
    let mats = vec![material(0.0); room.surfaces.len()];
    let surfaces: Vec<SurfaceAcoustics> = mats.iter().map(|m| SurfaceAcoustics::new(m, &freqs)).collect();
    let graph = EdgeGraph::build(&room);
    let h0 = vec![Complex64::new(1.0, 0.0); freqs.len()];
    let eps = 1e-4;
    // Face lines y = 5.1 and y = 5.9 behind the pillar, the direct shadow
    // boundaries, and a reflection boundary off the front face.
    for (x, y) in [(10.0, 5.1), (10.0, 5.9), (11.0, 4.6895), (9.0, 5.1)] {
        let receivers = vec![DVec3::new(x, y - eps, 3.6), DVec3::new(x, y + eps, 3.6)];
        let paths = trace_paths(&room, &images, DMat3::IDENTITY, &directivity, &receivers, &AtomicBool::new(false), &|_| {}).unwrap();
        let job = AccumulateJob { paths: &paths, spectra: &spectra, h0: &h0, r_ref: 1.0, directivity: &directivity, surfaces: &surfaces };
        let mut total = accumulate_cpu(&job);
        let settings = DiffractionSettings::default();
        let dpaths = trace_diffraction(&room, &graph, &images, DMat3::IDENTITY, &directivity, &receivers, settings, &AtomicBool::new(false)).unwrap();
        wavey_solver::diffraction::accumulate(&dpaths, &graph.edges, &job, &mut total);
        for fi in 0..freqs.len() {
            let (a, b) = (total[fi], total[freqs.len() + fi]);
            let rel = (a - b).norm() / a.norm();
            eprintln!("({x}, {y}) {} Hz: {:.4} vs {:.4} (relative jump {rel:.4}); paths {} / {}", freqs[fi], a.norm(), b.norm(), dpaths[0].len(), dpaths[1].len());
            assert!(rel < 0.005, "({x}, {y}) {} Hz: jump {rel}", freqs[fi]);
        }
    }
}

/// The same room as a mesh turned onto its side (vertical pillar edges
/// become horizontal), with source and receivers turned along: image
/// sources and diffraction must give the same field. Exercises general
/// edges and position-dependent visibility.
#[test]
fn rotated_room_gives_the_same_field() {
    let (lx, ly, h) = (13.0, 11.0, 7.0);
    let pc = DVec2::new(8.0, 5.5);
    let half = 0.4;
    let mut sk = Sketch::new();
    sk.add_polygon(&[DVec2::new(0.0, 0.0), DVec2::new(lx, 0.0), DVec2::new(lx, ly), DVec2::new(0.0, ly)]);
    sk.add_polygon(&[pc + DVec2::new(-half, -half), pc + DVec2::new(half, -half), pc + DVec2::new(half, half), pc + DVec2::new(-half, half)]);
    sk.add_segment(DVec2::new(3.0, 2.0), DVec2::new(3.0, 6.0));
    let room = build_room(&sk, h, &BTreeMap::from([(1, DVec2::new(1.0, 1.0))])).unwrap();
    let rot = DMat3::from_rotation_x(std::f64::consts::FRAC_PI_2);
    let verts: Vec<DVec3> = room.positions.iter().map(|p| rot * *p).collect();
    let faces: Vec<(Vec<u32>, u32)> = room.triangles.iter().zip(&room.tri_surface).map(|(t, s)| (t.to_vec(), *s)).collect();
    let turned = wavey_geom::room_from_mesh(&verts, &faces).unwrap();
    assert!(!turned.prism);
    assert_eq!(turned.surfaces.len(), room.surfaces.len());

    let freqs = vec![125.0, 250.0, 600.0];
    let spectra = Spectra::new(freqs.clone(), &Medium::default());
    let model = wavey_speaker::samples::omni();
    let directivity = Directivity::from_model(&model, &freqs);
    let h0 = vec![Complex64::new(1.0, 0.0); freqs.len()];
    let field = |room: &wavey_geom::Room, sp: DVec3, receivers: &[DVec3]| {
        let mats = vec![material(0.1); room.surfaces.len()];
        let surfaces: Vec<SurfaceAcoustics> = mats.iter().map(|m| SurfaceAcoustics::new(m, &freqs)).collect();
        let images = generate_images(room, sp, 1);
        let paths = trace_paths(room, &images, DMat3::IDENTITY, &directivity, receivers, &AtomicBool::new(false), &|_| {}).unwrap();
        let job = AccumulateJob { paths: &paths, spectra: &spectra, h0: &h0, r_ref: 1.0, directivity: &directivity, surfaces: &surfaces };
        let mut total = accumulate_cpu(&job);
        let graph = EdgeGraph::build(room);
        let dpaths = trace_diffraction(room, &graph, &images, DMat3::IDENTITY, &directivity, receivers, DiffractionSettings::default(), &AtomicBool::new(false)).unwrap();
        wavey_solver::diffraction::accumulate(&dpaths, &graph.edges, &job, &mut total);
        (total, graph.edges.len(), dpaths.iter().map(Vec::len).sum::<usize>())
    };
    let sp = DVec3::new(6.0, 5.65, 3.2);
    let receivers: Vec<DVec3> = (0..30).map(|i| DVec3::new(1.5 + 0.35 * i as f64, 4.0 + 0.11 * i as f64, 1.2 + 0.15 * (i % 7) as f64)).collect();
    let (a, na, pa) = field(&room, sp, &receivers);
    let turned_rx: Vec<DVec3> = receivers.iter().map(|r| rot * *r).collect();
    let (b, nb, pb) = field(&turned, rot * sp, &turned_rx);
    eprintln!("edges {na} / {nb}, diffracted paths {pa} / {pb}");
    assert_eq!(na, nb);
    assert_eq!(pa, pb);
    let worst = a.iter().zip(&b).map(|(x, y)| (20.0 * (y.norm() / x.norm()).log10()).abs()).fold(0.0f32, f32::max);
    eprintln!("worst level difference {worst:.4} dB");
    assert!(worst < 0.05, "{worst} dB");
}
