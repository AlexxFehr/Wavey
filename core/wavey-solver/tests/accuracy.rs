//! The wave solver's accuracy level: how finely the grid resolves the top
//! of the requested band.

mod common;

use common::*;
use glam::DVec3;
use wavey_solver::fdtd::{FdtdSettings, WaveAccuracy};
use wavey_solver::*;

fn settings(crossover_hz: f64, accuracy: WaveAccuracy) -> FdtdSettings {
    FdtdSettings { crossover_hz, duration_s: 0.1, points_per_wavelength: 12.0, source_order: 0, cabinets: false, accuracy }
}

/// What the grid's top frequency was before accuracy levels existed.
fn before_levels(fs: &FdtdSettings, freqs: &[f64]) -> f64 {
    let all_wave = freqs.iter().all(|f| fs.weight(*f) >= 1.0);
    if all_wave { (freqs.last().copied().unwrap() * 2.0).min(fs.f_max()) } else { fs.f_max() }
}

fn log_freqs(lo: f64, hi: f64) -> Vec<f64> {
    (0..25).map(|i| lo * (hi / lo).powf(i as f64 / 24.0)).collect()
}

#[test]
fn headroom_scales_the_grid_to_the_top_of_a_narrow_band() {
    let freqs = log_freqs(25.0, 80.0);
    let top = |a| settings(120.0, a).grid_fmax(&freqs);
    assert!((top(WaveAccuracy::Draft) - 80.0).abs() < 1e-9);
    assert!((top(WaveAccuracy::Standard) - 120.0).abs() < 1e-9);
    // 2 × 80 Hz, under the crossover band's top (120 √2).
    assert!((top(WaveAccuracy::High) - 160.0).abs() < 1e-9);
}

#[test]
fn headroom_never_goes_above_the_crossover_band() {
    let freqs = log_freqs(20.0, 120.0);
    let fs = settings(120.0, WaveAccuracy::High);
    assert!((fs.grid_fmax(&freqs) - fs.f_max()).abs() < 1e-9);
    // A band reaching into the blend: Draft resolves what is blended.
    assert!((settings(120.0, WaveAccuracy::Draft).grid_fmax(&freqs) - 120.0).abs() < 1e-9);
    // Nothing the wave solver contributes to: the whole band's top.
    let above = log_freqs(400.0, 800.0);
    assert!((settings(120.0, WaveAccuracy::Draft).grid_fmax(&above) - settings(120.0, WaveAccuracy::Draft).f_max()).abs() < 1e-9);
}

#[test]
fn high_is_the_grid_from_before_accuracy_levels() {
    for crossover in [80.0, 120.0, 250.0, 500.0] {
        for (lo, hi) in [(20.0, 60.0), (25.0, 80.0), (20.0, 120.0), (50.0, 200.0), (100.0, 700.0), (300.0, 900.0)] {
            let freqs = log_freqs(lo, hi);
            let fs = settings(crossover, WaveAccuracy::High);
            assert!((fs.grid_fmax(&freqs) - before_levels(&fs, &freqs)).abs() < 1e-9, "crossover {crossover}, {lo}–{hi} Hz");
        }
    }
}

#[test]
fn high_is_the_default_and_is_not_stored() {
    assert_eq!(WaveAccuracy::default(), WaveAccuracy::High);
    assert!(!serde_json::to_string(&settings(120.0, WaveAccuracy::High)).unwrap().contains("accuracy"));
    assert!(serde_json::to_string(&settings(120.0, WaveAccuracy::Draft)).unwrap().contains("\"accuracy\":\"draft\""));
    // Settings saved before the level existed load as High.
    let old = r#"{"crossover_hz":120.0,"duration_s":0.1,"points_per_wavelength":12.0,"source_order":0,"cabinets":false}"#;
    assert_eq!(serde_json::from_str::<FdtdSettings>(old).unwrap().accuracy, WaveAccuracy::High);
}

/// Cell size in metres from the run's notes.
fn cell_size(r: &SimulationResult) -> f64 {
    let note = r.stats.notes.iter().find(|n| n.contains("air cells")).expect("a wave solver note");
    let after = note.split("Δx = ").nth(1).unwrap();
    after.split(" cm").next().unwrap().parse::<f64>().unwrap() / 100.0
}

#[test]
fn a_lower_level_builds_a_coarser_grid_for_the_same_band() {
    let solve = |a| {
        let mut inp = input(shoebox(4.0, 3.0, 2.5), 0.3, omni_at(DVec3::new(1.0, 1.0, 1.2)), &[DVec3::new(3.0, 2.0, 1.2)], 0, (30.0, 60.0));
        inp.settings.fdtd = Some(settings(120.0, a));
        run(&inp)
    };
    let (draft, standard, high) = (cell_size(&solve(WaveAccuracy::Draft)), cell_size(&solve(WaveAccuracy::Standard)), cell_size(&solve(WaveAccuracy::High)));
    assert!(draft > standard && standard > high, "{draft} {standard} {high}");
    // Cell size follows the headroom: 1 : 1.5 : 2 (to the grid's rounding).
    assert!((draft / high - 2.0).abs() < 0.3, "{draft} vs {high}");
}
