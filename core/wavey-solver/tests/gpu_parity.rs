//! GPU accumulation must match the CPU reference.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::*;
use glam::{DVec2, DVec3};
use wavey_solver::*;

#[test]
fn gpu_matches_cpu() {
    let Some(gpu) = wavey_solver::gpu::gpu() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    eprintln!("GPU: {}", gpu.name);

    let mut s = wavey_geom::Sketch::new();
    s.add_polygon(&[DVec2::new(0.0, 0.0), DVec2::new(30.0, 0.0), DVec2::new(30.0, 20.0), DVec2::new(0.0, 20.0)]);
    s.add_polygon(&[DVec2::new(14.0, 9.0), DVec2::new(15.0, 9.0), DVec2::new(15.0, 10.0), DVec2::new(14.0, 10.0)]);
    let room = wavey_geom::build_room(&s, 8.0, &BTreeMap::from([(1, DVec2::new(1.0, 1.0))])).unwrap();

    let mut src = omni_at(DVec3::new(2.0, 10.0, 4.0));
    src.model = Arc::new(wavey_speaker::samples::two_way());
    src.instance.yaw_deg = 10.0;
    src.instance.pitch_deg = -8.0;
    let mics: Vec<DVec3> = (0..40)
        .map(|i| DVec3::new(4.0 + (i % 8) as f64 * 3.2, 2.0 + (i / 8) as f64 * 3.7, 1.2 + (i % 3) as f64))
        .collect();
    let mut inp = input(room, 0.15, src, &mics, 3, (40.0, 20000.0));
    inp.settings.resolution = Resolution::FractionalOctave { n: 6 };
    // Angle-dependent reflection on half the surfaces.
    for (i, m) in inp.surface_materials.iter_mut().enumerate() {
        if i % 2 == 0 {
            m.angle_dependent = true;
            m.absorption = wavey_materials::Spectrum::bands(&[125.0, 4000.0], &[0.05, 0.6]);
            m.scattering = wavey_materials::Spectrum::Scalar(0.2);
        }
    }

    inp.settings.backend = BackendChoice::Cpu;
    let cpu = run(&inp);
    inp.settings.backend = BackendChoice::Gpu;
    let gpu_res = run(&inp);
    assert!(gpu_res.stats.backend.starts_with("gpu"), "{:?}", gpu_res.stats);
    eprintln!("paths: {}, cpu {:.3}s gpu {:.3}s", cpu.stats.paths, cpu.stats.seconds, gpu_res.stats.seconds);

    let d = unit_drives(&cpu);
    let mut worst = 0.0f64;
    let mut worst_phase = 0.0f64;
    for m in 0..mics.len() {
        let a = cpu.mic_pressure(m, &d);
        let b = gpu_res.mic_pressure(m, &d);
        let scale = a.iter().map(|p| p.norm()).fold(0.0, f64::max);
        for (x, y) in a.iter().zip(&b) {
            worst = worst.max((x - y).norm() / scale);
            if x.norm() > 0.1 * scale {
                worst_phase = worst_phase.max((x / y).arg().abs());
            }
        }
    }
    eprintln!("worst relative error {worst:.2e}, worst phase error {worst_phase:.2e} rad");
    assert!(worst < 2e-4);
    assert!(worst_phase < 1e-3);
}

/// The GPU diffraction sum must match `diffraction::accumulate`: a pillar
/// and a free-standing wall, a directional aimed source, first-order
/// image sources and edge chains, angle-dependent faces, 40 Hz–20 kHz.
#[test]
fn gpu_diffraction_matches_cpu() {
    use std::sync::atomic::AtomicBool;

    use glam::DMat3;
    use num_complex::{Complex32, Complex64};
    use wavey_solver::accumulate::AccumulateJob;
    use wavey_solver::diffraction::{DiffractionSettings, EdgeGraph, trace_diffraction};
    use wavey_solver::directivity::Directivity;
    use wavey_solver::ism::generate_images;
    use wavey_solver::scene::{Spectra, SurfaceAcoustics};

    let Some(gpu) = wavey_solver::gpu::gpu() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let mut s = wavey_geom::Sketch::new();
    s.add_polygon(&[DVec2::new(0.0, 0.0), DVec2::new(24.0, 0.0), DVec2::new(24.0, 16.0), DVec2::new(0.0, 16.0)]);
    s.add_polygon(&[DVec2::new(10.0, 7.0), DVec2::new(10.8, 7.0), DVec2::new(10.8, 7.8), DVec2::new(10.0, 7.8)]);
    s.add_polygon(&[DVec2::new(15.0, 2.0), DVec2::new(15.2, 2.0), DVec2::new(15.2, 9.0), DVec2::new(15.0, 9.0)]);
    let room = wavey_geom::build_room(&s, 4.0, &BTreeMap::from([(1, DVec2::new(1.0, 1.0))])).unwrap();

    let freqs: Vec<f64> = (0..55).map(|i| 40.0 * 2f64.powf(i as f64 / 6.0)).take_while(|f| *f <= 20000.0).collect();
    let mut mats = vec![material(0.1); room.surfaces.len()];
    for (i, m) in mats.iter_mut().enumerate() {
        if i % 2 == 0 {
            m.angle_dependent = true;
            m.absorption = wavey_materials::Spectrum::bands(&[125.0, 4000.0], &[0.05, 0.6]);
            m.scattering = wavey_materials::Spectrum::Scalar(0.2);
        }
    }
    let surfaces: Vec<SurfaceAcoustics> = mats.iter().map(|m| SurfaceAcoustics::new(m, &freqs)).collect();
    assert!(surfaces.iter().any(|s| s.zeta.iter().any(Option::is_some)), "some faces need an impedance");
    let spectra = Spectra::new(freqs.clone(), &wavey_materials::Medium::default());
    let model = wavey_speaker::samples::two_way();
    let directivity = Directivity::from_model(&model, &freqs);
    let to_local = DMat3::from_rotation_z(0.4) * DMat3::from_rotation_y(0.1);
    let images = generate_images(&room, DVec3::new(4.0, 8.0, 1.7), 1);
    let receivers: Vec<DVec3> = (0..60).map(|i| DVec3::new(11.5 + (i % 10) as f64 * 1.2, 1.0 + (i / 10) as f64 * 2.4, 1.2 + (i % 3) as f64 * 0.4)).collect();
    let graph = EdgeGraph::build(&room);
    let dpaths = trace_diffraction(&room, &graph, &images, to_local, &directivity, &receivers, DiffractionSettings::default(), &AtomicBool::new(false)).unwrap();
    let n: usize = dpaths.iter().map(Vec::len).sum();
    assert!(dpaths.iter().flatten().any(|p| p.hits.len() >= 2) && dpaths.iter().flatten().any(|p| !p.reflections.is_empty()));

    let empty = wavey_solver::ism::PathSet::default();
    let h0: Vec<Complex64> = freqs.iter().map(|f| Complex64::from_polar(1.0, -f / 3000.0)).collect();
    let job = AccumulateJob { paths: &empty, spectra: &spectra, h0: &h0, r_ref: 1.0, directivity: &directivity, surfaces: &surfaces };
    let zero = vec![Complex32::new(0.0, 0.0); receivers.len() * freqs.len()];
    let mut cpu = zero.clone();
    let t = std::time::Instant::now();
    wavey_solver::diffraction::accumulate(&dpaths, &graph.edges, &job, &mut cpu);
    let tc = t.elapsed();
    let mut out = zero;
    let t = std::time::Instant::now();
    gpu.accumulate_diffraction(&dpaths, &graph.edges, &job, &mut out).unwrap();
    eprintln!("{n} diffracted paths: cpu {tc:?}, gpu {:?}", t.elapsed());

    // Relative to each receiver's level at each frequency band.
    let nf = freqs.len();
    let mut worst = 0.0f64;
    for (a, b) in cpu.chunks(nf).zip(out.chunks(nf)) {
        let scale = a.iter().map(|p| p.norm()).fold(0.0f32, f32::max) as f64;
        for (x, y) in a.iter().zip(b) {
            worst = worst.max((x - y).norm() as f64 / scale);
        }
    }
    eprintln!("worst relative error {worst:.2e}");
    assert!(worst < 1e-3);
}
