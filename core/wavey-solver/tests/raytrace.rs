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
