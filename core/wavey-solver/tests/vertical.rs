//! The hybrid above the crossover against a pure wave-solver reference in a
//! low, flat room, where floor–ceiling interference makes the incoherent late
//! field wrong by 1–2 dB without the vertical-coherence correction.
//!
//! Needs a GPU and about a minute: `cargo test --release --features gpu
//! --test vertical -- --ignored --nocapture`.

mod common;

use std::sync::atomic::AtomicBool;

use common::*;
use glam::DVec3;
use num_complex::Complex64;
use wavey_solver::fdtd::FdtdSettings;
use wavey_solver::*;

fn plane_energy_db(r: &SimulationResult, f: f64) -> f64 {
    let drives: Vec<Vec<Complex64>> = vec![vec![Complex64::new(1.0, 0.0); r.freqs.len()]];
    let v = r.plane_spl(0, &drives, PlaneMode::Band { f, fraction: 3.0 });
    let e: Vec<f64> = v.iter().filter(|x| x.is_finite()).map(|x| 10f64.powf(*x as f64 / 10.0)).collect();
    10.0 * (e.iter().sum::<f64>() / e.len() as f64).log10()
}

#[test]
#[ignore]
fn hybrid_matches_wave_reference_in_a_low_room() {
    // 9 × 7 m, 3 m high; source near mid-height, ear-height plane.
    let room = shoebox(9.0, 7.0, 3.0);
    let mut base = input(room, 0.15, omni_at(DVec3::new(7.6, 3.2, 1.45)), &[], 3, (200.0, 560.0));
    for m in &mut base.surface_materials {
        m.scattering = wavey_materials::Spectrum::Scalar(0.1);
    }
    base.planes = vec![CutPlane { id: 1, name: "ear".into(), axis: PlaneAxis::Horizontal, offset: 1.6, spacing: 0.4, drape: None }];
    base.settings.resolution = Resolution::FractionalOctave { n: 48 };
    base.settings.backend = BackendChoice::Gpu;
    base.settings.late_reverb = true;
    base.settings.rays_per_source = 20_000;
    base.settings.diffraction = true;
    base.settings.impedance_reflection = true;

    let mut hybrid = base.clone();
    hybrid.settings.fdtd = Some(FdtdSettings { crossover_hz: 150.0, duration_s: 0.0, points_per_wavelength: 15.0, source_order: 0, cabinets: false });
    let mut wave = base.clone();
    wave.settings.fdtd = Some(FdtdSettings { crossover_hz: 707.1, duration_s: 0.0, points_per_wavelength: 8.0, source_order: 0, cabinets: false });

    let run = |i: &SimulationInput| simulate(i, &|_, _| {}, &AtomicBool::new(false)).unwrap();
    let (h, w) = (run(&hybrid), run(&wave));
    let mut worst = 0.0f64;
    let mut sum = 0.0;
    for f in [250.0, 315.0, 400.0, 500.0] {
        let d = plane_energy_db(&h, f) - plane_energy_db(&w, f);
        eprintln!("{f} Hz: hybrid − wave {d:+.2} dB");
        worst = worst.max(d.abs());
        sum += d;
    }
    eprintln!("mean {:+.2} dB, worst {worst:.2} dB", sum / 4.0);
    assert!(worst < 1.2 && (sum / 4.0).abs() < 0.6, "worst {worst}, mean {}", sum / 4.0);
}
