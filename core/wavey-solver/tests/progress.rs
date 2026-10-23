//! What a solve reports while it runs: a bar that only moves forward and
//! ends at 1, with and without the wave solver.

mod common;

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use common::*;
use glam::DVec3;
use wavey_solver::fdtd::FdtdSettings;
use wavey_solver::*;

/// Every (fraction, stage) a solve reports.
fn reports(inp: &SimulationInput) -> Vec<(f64, String)> {
    let seen = Mutex::new(Vec::new());
    simulate(inp, &|f, s| seen.lock().unwrap().push((f, s.to_string())), &AtomicBool::new(false)).unwrap();
    seen.into_inner().unwrap()
}

fn room_input(freqs: (f64, f64), fdtd: Option<FdtdSettings>) -> SimulationInput {
    let mut inp = input(shoebox(4.0, 3.0, 2.5), 0.3, omni_at(DVec3::new(1.0, 1.0, 1.2)), &[DVec3::new(3.0, 2.0, 1.2)], 2, freqs);
    inp.settings.fdtd = fdtd;
    inp
}

fn assert_forward_to_done(r: &[(f64, String)]) {
    assert!(r.windows(2).all(|w| w[1].0 >= w[0].0), "the bar went back: {:?}", r.iter().map(|x| x.0).collect::<Vec<_>>());
    assert_eq!(r.last().unwrap(), &(1.0, "done".to_string()));
    assert!(r.iter().all(|(f, _)| (0.0..=1.0).contains(f)));
}

#[test]
fn geometrical_run_reports_a_forward_bar() {
    let r = reports(&room_input((100.0, 400.0), None));
    assert_forward_to_done(&r);
}

#[test]
fn run_with_the_wave_solver_reports_a_forward_bar() {
    let fs = FdtdSettings { crossover_hz: 150.0, duration_s: 0.15, points_per_wavelength: 12.0, source_order: 0, cabinets: false, accuracy: Default::default() };
    let r = reports(&room_input((50.0, 200.0), Some(fs)));
    assert_forward_to_done(&r);
}

#[test]
fn wave_solver_only_run_reports_progress_while_it_runs() {
    let fs = FdtdSettings { crossover_hz: 300.0, duration_s: 0.15, points_per_wavelength: 12.0, source_order: 0, cabinets: false, accuracy: Default::default() };
    let r = reports(&room_input((50.0, 120.0), Some(fs)));
    assert_forward_to_done(&r);
    // The wave solver is the whole run, so its progress is on the bar.
    assert!(r.iter().any(|(f, s)| *f > 0.05 && *f < 0.95 && s.contains("wave solver")), "{r:?}");
}
