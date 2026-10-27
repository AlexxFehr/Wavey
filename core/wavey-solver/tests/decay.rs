//! Decay times from a stored result: bands, mics, planes, and the effect of
//! the source mix.

use glam::DVec3;
use num_complex::{Complex32, Complex64};
use wavey_solver::fdtd::FdtdSettings;
use wavey_solver::receivers::{CutPlane, Microphone, PlaneAxis, PlaneGrid};
use wavey_solver::result::{MicResult, PlaneResult};
use wavey_solver::{DecayQuantity, SimulationResult};

const STEP: f64 = 0.25;
const N: usize = 160;

fn grid() -> Vec<f64> {
    (0..N).map(|i| 40.0 + i as f64 * STEP).collect()
}

/// Transfer function of a noise-like response decaying over `rt`, the
/// noise given by `seed`.
fn transfer(rt: f64, seed: u64) -> Vec<Complex32> {
    let freqs = grid();
    let fs = 16.0 * N as f64 * STEP;
    let len = (fs / STEP) as usize;
    let mut s = seed;
    let ir: Vec<f64> = (0..len)
        .map(|i| {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let noise = ((s >> 33) as f64 / (1u64 << 31) as f64) - 0.5;
            let t = i as f64 / fs;
            if t < 0.02 { 0.0 } else { noise * (-(t - 0.02) * 6.9078 / rt).exp() }
        })
        .collect();
    freqs
        .iter()
        .map(|f| {
            let h: Complex64 = ir.iter().enumerate().map(|(i, x)| Complex64::from_polar(*x, -2.0 * std::f64::consts::PI * f * i as f64 / fs)).sum();
            Complex32::new(h.re as f32, h.im as f32)
        })
        .collect()
}

fn result(fdtd: bool, sources: Vec<Vec<Complex32>>) -> SimulationResult {
    let ns = sources.len();
    // A 2 × 1 plane with one point outside the room.
    let plane_grid = PlaneGrid { axis: PlaneAxis::Horizontal, offset: 1.5, u0: 0.0, v0: 0.0, du: 1.0, dv: 1.0, nu: 2, nv: 1, inside: vec![false, true], z: vec![] };
    SimulationResult {
        freqs: grid(),
        speaker_ids: (1..=ns as u64).collect(),
        mics: vec![MicResult {
            mic: Microphone { id: 1, name: "m".into(), position: DVec3::new(1.0, 1.0, 1.5) },
            transfer: sources.clone(),
            late: vec![],
            late_gain: vec![],
            echograms: vec![],
        }],
        planes: vec![PlaneResult {
            plane: CutPlane { id: 2, name: "p".into(), axis: PlaneAxis::Horizontal, offset: 1.5, spacing: 1.0, drape: None },
            grid: plane_grid,
            transfer: sources,
            late: vec![],
            late_group: vec![],
            late_gain: vec![],
        }],
        stats: Default::default(),
        fingerprint: 0,
        rt_sabine: Default::default(),
        rt_eyring: Default::default(),
        has_late: false,
        fdtd: fdtd.then(|| FdtdSettings { crossover_hz: 100.0, ..Default::default() }),
    }
}

fn unit(ns: usize) -> Vec<Vec<Complex64>> {
    vec![vec![Complex64::new(1.0, 0.0); N]; ns]
}

#[test]
fn bands_are_those_the_wave_solver_covers() {
    let r = result(true, vec![transfer(1.0, 1)]);
    assert_eq!(r.decay_bands(1.0), vec![63.0]);
    assert!(r.decay_bands(3.0).contains(&50.0) && r.decay_bands(3.0).contains(&63.0));
    // No wave solver: no decay from the spectrum.
    assert!(result(false, vec![transfer(1.0, 1)]).decay_bands(1.0).is_empty());
}

#[test]
fn a_mic_gives_the_decay_of_its_response() {
    let r = result(true, vec![transfer(1.0, 1)]);
    let d = r.mic_decay(0, &unit(1), 63.0, 1.0);
    assert!((d.t30 - 1.0).abs() < 0.3, "T30 {}", d.t30);
    // A band the run does not cover is nothing.
    assert!(r.mic_decay(0, &unit(1), 500.0, 1.0).t30.is_nan());
}

#[test]
fn a_plane_gives_the_same_decay_inside_and_nothing_outside() {
    let r = result(true, vec![transfer(1.0, 1)]);
    let map = r.plane_decay(0, &unit(1), 63.0, 1.0, DecayQuantity::T30);
    assert!(map[0].is_nan());
    let mic = r.mic_decay(0, &unit(1), 63.0, 1.0).t30;
    assert!((map[1] as f64 - mic).abs() < 1e-4, "{} vs {mic}", map[1]);
}

#[test]
fn the_phase_of_the_mix_decides_the_decay() {
    // Two sources with different responses. Mixed in phase they decay as a
    // blend; with the second delayed by half a second the sum has a second
    // arrival half a second in, which holds up the early decay (EDT). The
    // late slope (T30) is the same 0.6 s decay either way.
    let r = result(true, vec![transfer(0.6, 1), transfer(0.6, 2)]);
    let flat = r.mic_decay(0, &unit(2), 63.0, 1.0);
    let freqs = grid();
    let mut delayed = unit(2);
    for (fi, d) in delayed[1].iter_mut().enumerate() {
        *d = Complex64::from_polar(1.0, -2.0 * std::f64::consts::PI * freqs[fi] * 0.5);
    }
    let with_delay = r.mic_decay(0, &delayed, 63.0, 1.0);
    assert!(flat.edt.is_finite() && with_delay.edt.is_finite());
    assert!(with_delay.edt > flat.edt * 1.3, "EDT {} vs {}", flat.edt, with_delay.edt);
    assert!((flat.t30 - with_delay.t30).abs() < 0.15, "T30 {} vs {}", flat.t30, with_delay.t30);
}

#[test]
fn sources_that_cancel_leave_nothing_to_measure() {
    let t = transfer(1.0, 1);
    let r = result(true, vec![t.clone(), t]);
    let mut drives = unit(2);
    drives[1].iter_mut().for_each(|d| *d = Complex64::new(-1.0, 0.0));
    assert!(r.mic_decay(0, &drives, 63.0, 1.0).t30.is_nan());
}

#[test]
fn a_step_too_coarse_for_the_room_gives_no_t20_or_t30_and_says_what_it_needs() {
    let mut r = result(true, vec![transfer(1.0, 1)]);
    // The room's statistical decay is much longer than the 4 s window holds for T30.
    r.rt_eyring = [6.0; wavey_solver::raytrace::NB];
    let d = r.mic_decay(0, &unit(1), 63.0, 1.0);
    assert!(d.t30.is_nan() && d.t20.is_nan(), "{d:?}");
    assert!(d.edt.is_finite(), "EDT needs only the first 10 dB");
    let need = r.decay_step_needed(63.0, DecayQuantity::T30).unwrap();
    assert!((need - 0.6 * 60.0 / (6.0 * 35.0)).abs() < 1e-12);
    assert!(need < STEP);
    // The plane follows the same rule.
    assert!(r.plane_decay(0, &unit(1), 63.0, 1.0, DecayQuantity::T30)[1].is_nan());
}
