//! A cardioid subwoofer stack simulated in the wave solver. Cabinets are
//! rigid obstacles and each driver radiates from its baffle, so the rear
//! cancellation comes from the simulation: positions, cabinets (which shift
//! the effective source positions and shadow each other), polarity and delay.
//! Delay and gain are applied when mixing, so one solve per orientation
//! lets the test search them the way a cardioid preset is tuned.
//!
//! `RAYON_NUM_THREADS=2 cargo test --release --test cardioid -- --nocapture`

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use common::*;
use glam::DVec3;
use num_complex::Complex64;
use wavey_solver::fdtd::FdtdSettings;
use wavey_solver::*;
use wavey_speaker::samples;

fn sub(id: u64, pos: DVec3, yaw: f64) -> SourceInput {
    let mut m = samples::piston("sub18", "18-inch sub", 0.23);
    // Driver on the front baffle.
    m.acoustic_center_m = [m.enclosure.depth_m / 2.0, 0.0, 0.0];
    let mut s = omni_at(pos);
    s.instance.id = id;
    s.instance.model_id = "sub18".into();
    s.instance.yaw_deg = yaw;
    s.model = Arc::new(m);
    s
}

/// A two-sub vertical stack (bottom facing −x, top at `top_yaw`) at 38–42 Hz,
/// with a mic 4 m in front and 4 m behind.
fn solve(alpha: f64, top_yaw: f64) -> SimulationResult {
    let room = shoebox(14.0, 10.0, 5.0);
    let mut inp = input(
        room,
        alpha,
        sub(1, DVec3::new(7.0, 5.0, 0.4), 180.0),
        &[DVec3::new(3.0, 5.0, 1.0), DVec3::new(11.0, 5.0, 1.0)],
        0,
        (38.0, 42.0),
    );
    inp.sources.push(sub(2, DVec3::new(7.0, 5.0, 1.05), top_yaw));
    inp.settings.resolution = Resolution::Linear { step_hz: 1.0 };
    inp.settings.backend = BackendChoice::Cpu;
    inp.settings.fdtd = Some(FdtdSettings { crossover_hz: 80.0, duration_s: 0.0, points_per_wavelength: 15.0, source_order: 0, cabinets: true });
    let r = simulate(&inp, &|_, _| {}, &AtomicBool::new(false)).unwrap();
    for n in &r.stats.notes {
        eprintln!("  note: {n}");
    }
    r
}

/// Mean SPL (dB) at (front, rear) with the top sub's polarity, delay and gain.
fn mix(r: &SimulationResult, invert: bool, delay_ms: f64, gain_db: f64) -> (f64, f64) {
    let mut drives = vec![vec![Complex64::new(1.0, 0.0); r.freqs.len()]; 2];
    for (fi, f) in r.freqs.iter().enumerate() {
        let w = 2.0 * std::f64::consts::PI * f * delay_ms / 1000.0;
        let g = 10f64.powf(gain_db / 20.0) * if invert { -1.0 } else { 1.0 };
        drives[1][fi] = Complex64::from_polar(g, -w);
    }
    let level = |mic: usize| {
        let p = r.mic_pressure(mic, &drives);
        result::spl(p.iter().map(|v| v.norm_sqr()).sum::<f64>() / p.len() as f64)
    };
    (level(0), level(1))
}

/// Delay and gain of the top sub that minimise the level behind.
fn tune(r: &SimulationResult) -> (f64, f64, f64, f64) {
    let mut best = (0.0, 0.0, f64::INFINITY, 0.0);
    for d in 0..=120 {
        let delay = d as f64 * 0.05;
        for g in -24..=24 {
            let gain = g as f64 * 0.25;
            let (front, rear) = mix(r, true, delay, gain);
            if rear - front < best.2 {
                best = (delay, gain, rear - front, front);
            }
        }
    }
    best
}

#[test]
fn cardioid_stack_cancels_behind() {
    let fwd = solve(0.98, 180.0);
    let (f0, b0) = mix(&fwd, false, 0.0, 0.0);
    eprintln!("both forward: front {f0:.1} dB, rear {b0:.1} dB (rear − front {:+.1} dB)", b0 - f0);
    // Nearly omnidirectional: the rear mic is 0.5 m further from the drivers
    // (≈ 1 dB) and the two stacked boxes shadow it a little.
    assert!((b0 - f0).abs() < 6.0, "two forward subs at 40 Hz are nearly omnidirectional");

    let card = solve(0.98, 0.0);
    let depth = 2.2 * 0.23;
    let naive = depth / 343.0 * 1000.0;
    let (fn_, bn) = mix(&card, true, naive, 0.0);
    eprintln!("cardioid, delay = depth/c ({naive:.2} ms): rear − front {:+.1} dB", bn - fn_);
    let (delay, gain, rej, front) = tune(&card);
    eprintln!("cardioid, tuned: delay {delay:.2} ms, top gain {gain:+.2} dB → rear − front {rej:+.1} dB, front {:+.1} dB vs both forward", front - f0);
    assert!(rej < -15.0, "tuned cardioid rejection {rej:.1} dB");
    assert!(delay > naive, "the cabinets push the effective source positions apart ({delay:.2} ms vs {naive:.2} ms)");

    // In a reflective room the reflected front sound also arrives behind.
    let room = solve(0.6, 0.0);
    let (fr, br) = mix(&room, true, delay, gain);
    eprintln!("same tuning in an α 0.6 room: rear − front {:+.1} dB", br - fr);
}
