//! Ray tracer against statistical room acoustics.

mod common;

use std::f64::consts::PI;
use std::sync::atomic::AtomicBool;

use common::*;
use glam::{DMat3, DVec3};
use wavey_materials::{Medium, Spectrum};
use wavey_solver::raytrace::*;

fn diffuse_box(alpha: f64) -> (wavey_geom::Room, Vec<BandSurface>) {
    let room = shoebox(12.0, 9.0, 5.0);
    let mut m = material(alpha);
    m.scattering = Spectrum::Scalar(0.9);
    let surfaces = vec![BandSurface::new(&m); room.surfaces.len()];
    (room, surfaces)
}

#[test]
fn reverberation_time_matches_eyring() {
    let (room, surfaces) = diffuse_box(0.15);
    let medium = Medium::default();
    let model = wavey_speaker::samples::omni();
    let src = DVec3::new(3.0, 3.0, 1.6);
    let mic = DVec3::new(8.0, 6.0, 1.4);
    let settings = RaySettings { rays: 20_000, ism_order: 0, max_time: 2.5, seed: 7 };
    let out = trace(&room, &surfaces, &medium, src, DMat3::IDENTITY, &model, &[mic], settings, &AtomicBool::new(false), &|_| {}).unwrap();
    let m = metrics(&out.echograms[0]);
    let (sabine, eyring) = statistical_rt(&room, &surfaces, &medium);
    for b in [2, 3, 4, 5] {
        eprintln!("{} Hz: T30 {:.3} s, Eyring {:.3} s, Sabine {:.3} s", BANDS[b], m.t30_s[b], eyring[b], sabine[b]);
        assert!((m.t30_s[b] / eyring[b] - 1.0).abs() < 0.1, "band {}", BANDS[b]);
    }
}

#[test]
fn steady_state_level_matches_diffuse_theory() {
    let alpha = 0.1;
    let (room, surfaces) = diffuse_box(alpha);
    let medium = Medium::default();
    let model = wavey_speaker::samples::omni();
    let src = DVec3::new(3.0, 3.0, 1.6);
    let settings = RaySettings { rays: 20_000, ism_order: 0, max_time: 4.0, seed: 3 };
    let out = trace(&room, &surfaces, &medium, src, DMat3::IDENTITY, &model, &[], settings, &AtomicBool::new(false), &|_| {}).unwrap();
    // Average late energy over the room's central region, 500 Hz band.
    let b = 3;
    let mut sum = 0.0;
    let mut n = 0;
    for x in 0..8 {
        for y in 0..6 {
            let p = DVec3::new(2.5 + x as f64, 2.0 + y as f64, 2.5);
            sum += out.late.at(p)[b] as f64;
            n += 1;
        }
    }
    let got = sum / n as f64;
    // Reverberant field of a source with on-axis p_ref at r_ref:
    // p² = 16π r_ref² p_ref² (1 − α) / A, with Eyring's absorption area A.
    let p_ref2 = model.on_axis(500.0).norm_sqr();
    let s = room.total_surface_area();
    let v = room.volume();
    let a_eyring = -s * (1.0f64 - alpha).ln() + 4.0 * medium.air_intensity_coefficient(500.0) * v;
    let want = 16.0 * PI * p_ref2 * (1.0 - alpha) / a_eyring;
    let diff_db = 10.0 * (got / want).log10();
    eprintln!("late level {:.2} dB vs theory {:.2} dB ({diff_db:+.2} dB)", 10.0 * (got / 4e-10).log10(), 10.0 * (want / 4e-10).log10());
    assert!(diff_db.abs() < 1.0);
}

/// Stepped seating (0.6 m treads, 0.3 m risers) is a smooth 1:2 slope at
/// 125 Hz (λ = 2.7 m): its facets reflect off the mean plane. At 2 kHz the
/// treads are larger than half a wavelength and reflect themselves.
#[test]
fn stepped_floor_reflects_off_its_slope_at_low_frequencies() {
    let (run, rise, steps, depth, top) = (0.6, 0.3, 10usize, 4.0, 6.0);
    let mut v: Vec<DVec3> = Vec::new();
    let mut faces: Vec<(Vec<u32>, u32)> = Vec::new();
    let mut quad = |v: &mut Vec<DVec3>, p: [DVec3; 4]| {
        let i = v.len() as u32;
        v.extend(p);
        faces.push((vec![i, i + 1, i + 2, i + 3], 0));
    };
    let y = DVec3::new(0.0, depth, 0.0);
    for k in 0..steps {
        let (x0, x1, z) = (k as f64 * run, (k + 1) as f64 * run, k as f64 * rise);
        let (a, b) = (DVec3::new(x0, 0.0, z), DVec3::new(x1, 0.0, z));
        quad(&mut v, [a, b, b + y, a + y]); // tread
        if k + 1 < steps {
            let c = DVec3::new(x1, 0.0, z + rise);
            quad(&mut v, [b, c, c + y, b + y]); // riser
        }
        // side walls, one column per step (keeps every face convex)
        let (t0, t1) = (DVec3::new(x0, 0.0, top), DVec3::new(x1, 0.0, top));
        quad(&mut v, [a, b, t1, t0]);
        quad(&mut v, [a + y, b + y, t1 + y, t0 + y]);
    }
    let len = steps as f64 * run;
    let zl = (steps - 1) as f64 * rise;
    let p = |x: f64, yy: f64, z: f64| DVec3::new(x, yy, z);
    quad(&mut v, [p(0.0, 0.0, 0.0), p(0.0, depth, 0.0), p(0.0, depth, top), p(0.0, 0.0, top)]); // front wall
    quad(&mut v, [p(len, 0.0, zl), p(len, depth, zl), p(len, depth, top), p(len, 0.0, top)]); // back wall
    quad(&mut v, [p(0.0, 0.0, top), p(len, 0.0, top), p(len, depth, top), p(0.0, depth, top)]); // ceiling
    let room = wavey_geom::room_from_mesh(&v, &faces).unwrap();
    let surfaces = vec![BandSurface::new(&material(0.1)); room.surfaces.len()];
    let c = Medium::default().speed_of_sound();
    let clusters = smooth_clusters(&room, &surfaces, &panel_dims(&room), c);
    let slope = DVec3::new(-rise, 0.0, run).normalize();
    // A tread triangle: horizontal, below the ceiling.
    let tread = (0..room.triangles.len())
        .find(|&t| {
            let n = room.surfaces[room.tri_surface[t] as usize].normal;
            let z = room.positions[room.triangles[t][0] as usize].z;
            n.z > 0.99 && z > 0.5 && z < 2.0
        })
        .unwrap();
    let b125 = BANDS.iter().position(|f| *f == 125.0).unwrap();
    let b2k = BANDS.iter().position(|f| *f == 2000.0).unwrap();
    let (n, dims) = clusters[tread][b125].expect("steps act as one slope at 125 Hz");
    assert!(n.dot(slope) > 0.999, "mean normal {n} vs slope {slope}");
    // The cluster spans the seating: about the slope length × depth.
    let slope_len = ((len - run).powi(2) + zl.powi(2)).sqrt();
    assert!((dims[0] - depth).abs() < 1.0 && (dims[1] - slope_len).abs() < 1.5, "dims {dims:?}");
    assert!(clusters[tread][b2k].is_none());
}
