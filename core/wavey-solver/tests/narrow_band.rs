//! Narrow low bands on Draft and Standard: the grid follows the band, small
//! cabinets become directional sources, the wave solver works alone and the
//! run lasts as long as the frequency step needs. High stays as it was.

mod common;

use std::sync::Arc;

use common::*;
use glam::DVec3;
use wavey_solver::fdtd::{FdtdSettings, WaveAccuracy};
use wavey_solver::*;

/// An 18-inch sub (0.46 m box, a balloon) in a 6 × 5 × 3 m room, a mic 2 m away.
fn sub_room(band: (f64, f64), accuracy: WaveAccuracy) -> SimulationInput {
    unsafe { std::env::set_var("WAVEY_NO_DISK_CACHE", "1") };
    let mut model = wavey_speaker::samples::piston("sub18", "18-inch sub", 0.23);
    model.acoustic_center_m = [model.enclosure.depth_m / 2.0, 0.0, 0.0];
    let mut src = omni_at(DVec3::new(2.0, 2.5, 0.5));
    src.instance.model_id = "sub18".into();
    src.model = Arc::new(model);
    let mut inp = input(shoebox(6.0, 5.0, 3.0), 0.2, src, &[DVec3::new(4.0, 2.5, 1.2), DVec3::new(5.0, 4.0, 1.6)], 2, band);
    inp.settings.resolution = Resolution::Linear { step_hz: 1.0 };
    inp.settings.fdtd = Some(FdtdSettings { crossover_hz: 80.0, duration_s: 0.0, points_per_wavelength: 12.0, source_order: 1, cabinets: true, accuracy });
    inp
}

fn has_note(r: &SimulationResult, text: &str) -> bool {
    r.stats.notes.iter().any(|n| n.contains(text))
}

fn cell_cm(r: &SimulationResult) -> f64 {
    let n = r.stats.notes.iter().find(|n| n.contains("air cells")).unwrap();
    n.split("Δx = ").nth(1).unwrap().split(" cm").next().unwrap().parse().unwrap()
}

fn seconds(r: &SimulationResult) -> f64 {
    let n = r.stats.notes.iter().find(|n| n.contains("air cells")).unwrap();
    n.split(" steps over ").nth(1).unwrap().split(" s").next().unwrap().parse().unwrap()
}

fn level_db(r: &SimulationResult) -> Vec<Vec<f64>> {
    let d = unit_drives(r);
    (0..r.mics.len()).map(|m| r.mic_pressure(m, &d).iter().map(|p| 20.0 * p.norm().max(1e-12).log10()).collect()).collect()
}

#[test]
fn standard_leaves_a_small_cabinet_out_and_sizes_the_grid_by_the_band() {
    let standard = run(&sub_room((20.0, 60.0), WaveAccuracy::Standard));
    let high = run(&sub_room((20.0, 60.0), WaveAccuracy::High));
    assert!(has_note(&standard, "left out"), "{:?}", standard.stats.notes);
    assert!(!has_note(&standard, "rigid box"), "no calibration without a cabinet");
    // High keeps the cabinet, so its grid is the cabinet's (8 cells across 0.46 m).
    assert!(has_note(&high, "rigid box") && !has_note(&high, "left out"));
    assert!(cell_cm(&standard) > 4.0 * cell_cm(&high), "{} vs {} cm", cell_cm(&standard), cell_cm(&high));
}

#[test]
fn a_low_band_on_standard_is_the_wave_solver_alone() {
    let r = run(&sub_room((20.0, 60.0), WaveAccuracy::Standard));
    assert!(has_note(&r, "wave solver only"), "{:?}", r.stats.notes);
    assert_eq!(r.stats.paths, 0, "no image sources");
    // High keeps the crossover the user set.
    let h = run(&sub_room((20.0, 60.0), WaveAccuracy::High));
    assert_eq!(h.fdtd.as_ref().unwrap().crossover_hz, 80.0);
}

#[test]
fn the_run_lasts_as_long_as_the_frequency_step_needs() {
    let mut fine = sub_room((30.0, 50.0), WaveAccuracy::Draft);
    fine.settings.resolution = Resolution::Linear { step_hz: 0.25 };
    assert!(seconds(&run(&fine)) >= 4.0 - 1e-6);
    // High keeps the duration from the room's decay.
    let mut high = sub_room((30.0, 50.0), WaveAccuracy::High);
    high.settings.resolution = Resolution::Linear { step_hz: 0.25 };
    assert!(seconds(&run(&high)) < 4.0);
}

/// What leaving the cabinet out costs, against High with it: in this small
/// room (the box is large for it) the median level difference was 0.5 and
/// 2.8 dB at the two mics. Guards against it getting worse.
#[test]
fn standard_without_the_cabinet_stays_within_a_few_db_of_high_with_it() {
    let standard = level_db(&run(&sub_room((25.0, 60.0), WaveAccuracy::Standard)));
    let high = level_db(&run(&sub_room((25.0, 60.0), WaveAccuracy::High)));
    for (m, (s, h)) in standard.iter().zip(&high).enumerate() {
        let mut d: Vec<f64> = s.iter().zip(h).map(|(a, b)| (a - b).abs()).collect();
        d.sort_by(f64::total_cmp);
        let median = d[d.len() / 2];
        eprintln!("mic {m}: median {median:.2} dB, worst {:.2} dB", d.last().unwrap());
        assert!(median < 3.0, "mic {m}: median difference {median:.2} dB");
    }
}

/// Diagnostic (run with --ignored): Standard and High without a cabinet
/// against High with it, to see what the cabinet and the grid each cost.
#[test]
#[ignore]
fn diag_where_standard_differs() {
    // With a real measured sub when the venue project is there (WAVEY_DIAG_MODEL=path/to/project.json).
    let model: Option<Arc<wavey_speaker::SpeakerModel>> = std::env::var("WAVEY_DIAG_MODEL").ok().map(|p| {
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
        let (_, m) = v["speaker_models"].as_object().unwrap().iter().find(|(_, m)| m["name"].as_str().unwrap_or("").contains("DXS18")).unwrap();
        Arc::new(serde_json::from_value(m.clone()).unwrap())
    });
    let sub_room = |band, a| {
        let mut inp = sub_room(band, a);
        if let Some(m) = &model {
            inp.sources[0].model = m.clone();
            inp.sources[0].instance.model_id = m.id.clone();
        }
        inp
    };
    let reference = level_db(&run(&sub_room((25.0, 60.0), WaveAccuracy::High)));
    let report = |name: &str, inp: SimulationInput| {
        let r = run(&inp);
        let l = level_db(&r);
        for (m, (s, h)) in l.iter().zip(&reference).enumerate() {
            let mut d: Vec<f64> = s.iter().zip(h).map(|(a, b)| (a - b).abs()).collect();
            d.sort_by(f64::total_cmp);
            eprintln!("{name:50} mic {m}: median {:.2} dB, 90% {:.2}, worst {:.2}  (dx {:.1} cm)", d[d.len() / 2], d[d.len() * 9 / 10], d.last().unwrap(), cell_cm(&r));
        }
    };
    report("standard (new)", sub_room((25.0, 60.0), WaveAccuracy::Standard));
    let mut no_cab_fine = sub_room((25.0, 60.0), WaveAccuracy::High);
    if let Some(f) = no_cab_fine.settings.fdtd.as_mut() { f.cabinets = false; f.points_per_wavelength = 50.0; }
    report("high, no cabinet, fine grid", no_cab_fine);
    let mut no_cab = sub_room((25.0, 60.0), WaveAccuracy::High);
    if let Some(f) = no_cab.settings.fdtd.as_mut() { f.cabinets = false; }
    report("high, no cabinet, band grid (2x headroom)", no_cab);
    let mut std_fine = sub_room((25.0, 60.0), WaveAccuracy::Standard);
    if let Some(f) = std_fine.settings.fdtd.as_mut() { f.points_per_wavelength = 40.0; }
    report("standard, finer grid (40 ppw)", std_fine);
}
