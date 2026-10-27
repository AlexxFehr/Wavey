//! FDTD wave solver against analytic solutions.

mod common;

use std::f64::consts::PI;
use std::sync::atomic::AtomicBool;

use common::*;
use num_complex::Complex64;
use wavey_materials::Medium;
use wavey_solver::fdtd::*;

#[test]
fn direct_sound_is_calibrated() {
    // Large room, receiver close to the source; the run stops before the
    // first reflection arrives, so the result is the free-field response.
    let room = shoebox(12.0, 10.0, 8.0);
    let mats = vec![material(0.2); room.surfaces.len()];
    let c = Medium::default().speed_of_sound();
    let f_max = 600.0;
    let g = FdtdGrid::build(&room, &fit_surfaces(&mats, (20.0, f_max)), c, f_max, 10.0).unwrap();
    let src = g.nearest_air(glam::DVec3::new(6.0, 5.0, 4.0)).unwrap();
    let rcv = g.nearest_air(glam::DVec3::new(7.5, 5.0, 4.0)).unwrap();
    let pos = |i: usize| {
        let (z, r) = (i / (g.n[0] * g.n[1]), i % (g.n[0] * g.n[1]));
        g.center(r % g.n[0], r / g.n[0], z)
    };
    let r = (pos(rcv) - pos(src)).length();
    let steps = (0.016 / g.dt) as usize;
    let signal = source_signal(f_max, g.dt, steps);
    let freqs = [150.0, 250.0, 350.0, 450.0];
    let spec = run_cpu(&g, &[(src, 1.0)], &signal, &[rcv], &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
    let h0 = vec![Complex64::new(1.0, 0.0); freqs.len()];
    let t = to_transfer(&spec, &signal, &g, &freqs, c, 1.0, &h0);
    for (fi, f) in freqs.iter().enumerate() {
        let k = 2.0 * PI * f / c;
        let want = Complex64::from_polar(1.0 / r, -k * r);
        let db = 20.0 * (t[fi].norm() / want.norm()).log10();
        let deg = (t[fi] / want).arg().to_degrees();
        // The receiver lies on a grid axis, the scheme's worst direction:
        // the lag must equal the analytic axial dispersion.
        let predicted = -FdtdSettings::phase_error_deg_per_m(*f, c, g.dx) * r;
        eprintln!("{f} Hz: {db:+.3} dB, {deg:+.2}° (predicted dispersion {predicted:+.2}°)");
        assert!(db.abs() < 0.1, "{f} Hz: {db} dB");
        assert!((deg - predicted).abs() < 0.3, "{f} Hz: {deg}° vs {predicted}°");
    }
}

#[test]
fn rigid_box_eigenfrequencies() {
    let (lx, ly, lz) = (3.0, 2.5, 2.0);
    let room = shoebox(lx, ly, lz);
    let mats = vec![material(0.0); room.surfaces.len()];
    let c = Medium::default().speed_of_sound();
    // dx = c / (10 f_max) = 0.1 m puts the effective walls exactly on the room.
    let f_max = c;
    let g = FdtdGrid::build(&room, &fit_surfaces(&mats, (20.0, f_max)), c, f_max, 10.0).unwrap();
    assert!((g.dx - 0.1).abs() < 1e-12);
    let src = g.nearest_air(glam::DVec3::new(0.05, 0.05, 0.05)).unwrap();
    let rcv = g.nearest_air(glam::DVec3::new(lx - 0.05, ly - 0.05, lz - 0.05)).unwrap();
    let steps = (2.0 / g.dt) as usize;
    let signal = source_signal(400.0, g.dt, steps);
    let freqs: Vec<f64> = (0..1000).map(|i| 40.0 + i as f64 * 0.08).collect();
    let spec = run_cpu(&g, &[(src, 1.0)], &signal, &[rcv], &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
    let mag: Vec<f64> = spec.iter().map(|v| v.norm()).collect();
    let mut modes = vec![];
    for nx in 0..4 {
        for ny in 0..4 {
            for nz in 0..4 {
                let f = c / 2.0 * ((nx as f64 / lx).powi(2) + (ny as f64 / ly).powi(2) + (nz as f64 / lz).powi(2)).sqrt();
                if f > 45.0 && f < 115.0 {
                    modes.push(f);
                }
            }
        }
    }
    // Strongest response within ±1.5 % of each well-separated mode (the
    // nearest local maximum would be a sidelobe ripple).
    let mut checked = 0;
    for &m in &modes {
        if modes.iter().any(|o| (o / m - 1.0).abs() > 1e-9 && (o / m - 1.0).abs() < 0.03) {
            continue;
        }
        let (k, _) = freqs.iter().enumerate().filter(|(_, f)| (*f / m - 1.0).abs() < 0.015).max_by(|a, b| mag[a.0].total_cmp(&mag[b.0])).unwrap();
        eprintln!("mode {m:.2} Hz → peak {:.2} Hz ({:+.2}%)", freqs[k], (freqs[k] / m - 1.0) * 100.0);
        assert!((freqs[k] / m - 1.0).abs() < 0.005);
        checked += 1;
    }
    assert!(checked >= 4);
}

/// The 1 m rigid cube of Li et al. 2022 (Appl. Acoust. 191, Fig. 5), source
/// (0.2, 0.2, 0.2), receiver (0.9, 0.9, 0.9), 100–800 Hz, on the default
/// 15 points per wavelength at 800 Hz: every analytic mode within 0.6 %
/// (the scheme's worst case, axial modes, is −0.37 % at 686 Hz).
#[test]
fn rigid_cube_modes_to_800_hz() {
    let room = shoebox(1.0, 1.0, 1.0);
    let mats = vec![material(0.0); room.surfaces.len()];
    let c = Medium::default().speed_of_sound();
    // A whole number of cells across the cube (35), ≈ 15 per wavelength at 800 Hz.
    let n = (1.0 / (c / (800.0 * 15.0))).ceil();
    let f_max = c * n / 15.0;
    let g = FdtdGrid::build(&room, &fit_surfaces(&mats, (20.0, f_max)), c, f_max, 15.0).unwrap();
    let src = g.nearest_air(glam::DVec3::new(0.2, 0.2, 0.2)).unwrap();
    let rcv = g.nearest_air(glam::DVec3::new(0.9, 0.9, 0.9)).unwrap();
    let steps = (4.0 / g.dt) as usize;
    let signal = source_signal(1600.0, g.dt, steps);
    let freqs: Vec<f64> = (0..7400).map(|i| 90.0 + i as f64 * 0.1).collect();
    let spec = run_cpu(&g, &[(src, 1.0)], &signal, &[rcv], &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
    let mag: Vec<f64> = spec.iter().map(|v| v.norm()).collect();
    let mut modes: Vec<(f64, [u32; 3])> = vec![];
    for nx in 0..6u32 {
        for ny in 0..=nx {
            for nz in 0..=ny {
                let m = c / 2.0 * ((nx * nx + ny * ny + nz * nz) as f64).sqrt();
                if (100.0..=800.0).contains(&m) {
                    modes.push((m, [nx, ny, nz]));
                }
            }
        }
    }
    // The strongest response within ±1.5 % of each mode that has no other
    // mode within 3 % (the 4 s window's sidelobes ripple every 0.25 Hz, so
    // the nearest local maximum would always look exact).
    let mut worst: f64 = 0.0;
    let mut checked = 0;
    for &(m, idx) in &modes {
        if modes.iter().any(|&(o, _)| (o / m - 1.0).abs() > 1e-9 && (o / m - 1.0).abs() < 0.03) {
            continue;
        }
        let (k, _) = freqs
            .iter()
            .enumerate()
            .filter(|(_, f)| (*f / m - 1.0).abs() < 0.015)
            .max_by(|a, b| mag[a.0].total_cmp(&mag[b.0]))
            .unwrap();
        let e = freqs[k] / m - 1.0;
        eprintln!("mode {idx:?} {m:.1} Hz → {:.1} Hz ({:+.2} %)", freqs[k], e * 100.0);
        worst = worst.max(e.abs());
        checked += 1;
    }
    assert!(checked >= 5);
    assert!(worst < 0.006, "worst mode shift {:.2} %", worst * 100.0);
}

#[test]
fn gpu_matches_cpu_with_a_source_spread_over_many_cells() {
    let Some(gpu) = wavey_solver::gpu::gpu() else {
        eprintln!("no GPU; skipping");
        return;
    };
    let room = shoebox(6.0, 4.0, 3.0);
    let mats: Vec<_> = (0..room.surfaces.len()).map(|_| material(0.2)).collect();
    let c = Medium::default().speed_of_sound();
    let g = FdtdGrid::build(&room, &fit_surfaces(&mats, (20.0, 200.0)), c, 200.0, 12.0).unwrap();
    // A point between nodes: eight trilinear taps (a cabinet's baffle point, or a directional source).
    let taps = g.taps_at(glam::DVec3::new(1.53, 1.27, 1.41)).unwrap();
    assert_eq!(taps.len(), 8);
    let rcv: Vec<usize> = [(4.5, 3.0, 1.2), (5.0, 1.0, 2.0)].iter().map(|p| g.nearest_air(glam::DVec3::new(p.0, p.1, p.2)).unwrap()).collect();
    let steps = (0.4 / g.dt) as usize;
    let signal = source_signal(200.0, g.dt, steps);
    let freqs: Vec<f64> = (0..20).map(|i| 30.0 + i as f64 * 8.0).collect();
    let cpu = run_cpu(&g, &taps, &signal, &rcv, &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
    let gpu_out = gpu.run_fdtd(&g, &taps, &signal, &rcv, &freqs, &AtomicBool::new(false), &|_| {}).expect("the GPU takes any number of taps").unwrap();
    let scale = cpu.iter().map(|v| v.norm()).fold(0.0, f64::max);
    let worst = cpu.iter().zip(&gpu_out).map(|(a, b)| (a - b).norm() / scale).fold(0.0, f64::max);
    assert!(worst < 1e-3, "worst relative difference {worst:.2e}");
}

#[test]
fn gpu_matches_cpu() {
    let Some(gpu) = wavey_solver::gpu::gpu() else {
        eprintln!("no GPU; skipping");
        return;
    };
    use glam::DVec2;
    use std::collections::BTreeMap;
    let mut s = wavey_geom::Sketch::new();
    s.add_polygon(&[DVec2::new(0.0, 0.0), DVec2::new(8.0, 0.0), DVec2::new(8.0, 6.0), DVec2::new(0.0, 6.0)]);
    s.add_polygon(&[DVec2::new(3.5, 2.5), DVec2::new(4.5, 2.5), DVec2::new(4.5, 3.5), DVec2::new(3.5, 3.5)]);
    let room = wavey_geom::build_room(&s, 3.0, &BTreeMap::from([(1, DVec2::new(1.0, 1.0))])).unwrap();
    let mats: Vec<_> = (0..room.surfaces.len()).map(|i| material(if i % 2 == 0 { 0.1 } else { 0.4 })).collect();
    let c = Medium::default().speed_of_sound();
    let g = FdtdGrid::build(&room, &fit_surfaces(&mats, (20.0, 300.0)), c, 300.0, 12.0).unwrap();
    let src = g.nearest_air(glam::DVec3::new(1.5, 3.0, 1.5)).unwrap();
    let rcv: Vec<usize> = [(6.5, 3.0, 1.2), (6.0, 1.0, 2.0), (2.0, 5.0, 0.5)]
        .iter()
        .map(|p| g.nearest_air(glam::DVec3::new(p.0, p.1, p.2)).unwrap())
        .collect();
    let steps = (0.8 / g.dt) as usize;
    let signal = source_signal(300.0, g.dt, steps);
    let freqs: Vec<f64> = (0..40).map(|i| 30.0 + i as f64 * 6.0).collect();
    let t0 = std::time::Instant::now();
    let cpu = run_cpu(&g, &[(src, 1.0)], &signal, &rcv, &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
    let t1 = std::time::Instant::now();
    let gpu_out = gpu.run_fdtd(&g, &[(src, 1.0)], &signal, &rcv, &freqs, &AtomicBool::new(false), &|_| {}).unwrap().unwrap();
    let t2 = std::time::Instant::now();
    eprintln!("{} cells, {steps} steps: cpu {:.2?}, gpu {:.2?}", g.cells(), t1 - t0, t2 - t1);
    let scale = cpu.iter().map(|v| v.norm()).fold(0.0, f64::max);
    let worst = cpu.iter().zip(&gpu_out).map(|(a, b)| (a - b).norm() / scale).fold(0.0, f64::max);
    eprintln!("worst relative difference {worst:.2e}");
    assert!(worst < 1e-3);
}

#[test]
fn fdtd_agrees_with_ism_in_crossover_band() {
    use wavey_solver::*;
    let c = Medium::default().speed_of_sound();
    let fs = FdtdSettings { crossover_hz: 250.0, duration_s: 0.4, points_per_wavelength: 15.0, source_order: 1, cabinets: false, accuracy: Default::default() };
    // The wave-only run below (every frequency under the crossover band)
    // sizes its grid for the requested band: 2 × 350 Hz.
    let dx = c / ((350.0 * 2.0f64).min(FdtdSettings { crossover_hz: 500.0, ..fs.clone() }.f_max()) * fs.points_per_wavelength);
    // Room dimensions on whole cells, so the staircase walls sit exactly on the surfaces.
    let l = [7.0, 5.5, 3.5].map(|v: f64| (v / dx).round() * dx);
    let src = glam::DVec3::new(2.0, 2.0, 1.7);
    let mic = glam::DVec3::new(4.6, 3.3, 1.5);
    let mut inp = input(shoebox(l[0], l[1], l[2]), 0.5, omni_at(src), &[mic], 8, (180.0, 350.0));
    inp.settings.resolution = Resolution::FractionalOctave { n: 12 };
    for m in &mut inp.surface_materials {
        m.angle_dependent = true;
    }
    inp.settings.backend = BackendChoice::Cpu;
    let ism = run(&inp);
    inp.settings.fdtd = Some(fs.clone());
    inp.settings.f_min = 180.0;
    inp.settings.f_max = 350.0;
    // Crossover weight 1 everywhere: compare pure FDTD by moving the crossover up.
    inp.settings.fdtd = Some(FdtdSettings { crossover_hz: 500.0, ..fs });
    let wave = run(&inp);
    eprintln!("{:?}", wave.stats.notes);
    let d = unit_drives(&ism);
    let (a, b) = (ism.mic_pressure(0, &d), wave.mic_pressure(0, &d));
    // The two methods model reflection differently (plane-wave coefficients
    // vs. a locally reacting boundary), so compare the complex difference to
    // the band's RMS level rather than dB at interference notches.
    let rms = (a.iter().map(|v| v.norm_sqr()).sum::<f64>() / a.len() as f64).sqrt();
    let mut dbs = Vec::new();
    let mut worst = 0.0f64;
    for (fi, f) in ism.freqs.iter().enumerate() {
        let db = 20.0 * (b[fi].norm() / a[fi].norm()).log10();
        let deg = (b[fi] / a[fi]).arg().to_degrees();
        let rel = (a[fi] - b[fi]).norm() / rms;
        eprintln!("{f:.1} Hz: ISM {:.2} dB, FDTD {:.2} dB, Δ {db:+.2} dB {deg:+.1}°, error {:.1} dB re band level",
            20.0 * (a[fi].norm() / 2e-5).log10(), 20.0 * (b[fi].norm() / 2e-5).log10(), 20.0 * rel.log10());
        dbs.push(db.abs());
        worst = worst.max(rel);
    }
    dbs.sort_by(f64::total_cmp);
    let median = dbs[dbs.len() / 2];
    eprintln!("median |Δ| {median:.2} dB, worst complex error {:.1} dB re band level", 20.0 * worst.log10());
    assert!(median < 1.0);
    assert!(worst < 0.25, "complex error {:.1} dB re band level", 20.0 * worst.log10());
}

fn library(id: &str) -> wavey_materials::Material {
    wavey_materials::builtin_materials().into_iter().find(|m| m.id == id).unwrap()
}

/// Plane wave in a rigid tube hitting a frequency-dependent end wall: the
/// measured reflection coefficient must match (ζ − 1)/(ζ + 1) of the fit.
#[test]
fn tube_reflection_matches_fitted_impedance() {
    let c = Medium::default().speed_of_sound();
    let (f_max, ppw) = (500.0, 15.0);
    let dx = c / (f_max * ppw);
    let cells = |v: f64| (v / dx).round() * dx;
    let (len, w) = (cells(30.0), cells(0.3));
    for id in ["audience-standing", "mineral-wool-50", "carpet-heavy"] {
        let room = shoebox(len, w, w);
        let mut mats = vec![material(0.0); room.surfaces.len()];
        let end = room.surfaces.iter().position(|s| s.normal.x > 0.99).unwrap(); // wall at x = 0
        mats[end] = library(id);
        let fits = fit_surfaces(&mats, (20.0, f_max));
        let g = FdtdGrid::build(&room, &fits, c, f_max, ppw).unwrap();
        let src = g.nearest_air(glam::DVec3::new(15.0, w / 2.0, w / 2.0)).unwrap();
        let d = 2.0;
        let rcv = g.nearest_air(glam::DVec3::new(d, w / 2.0, w / 2.0)).unwrap();
        let xr = {
            let (z, r) = (rcv / (g.n[0] * g.n[1]), rcv % (g.n[0] * g.n[1]));
            g.center(r % g.n[0], r / g.n[0], z).x
        };
        let steps = (0.075 / g.dt) as usize;
        let signal = source_signal(f_max, g.dt, steps);
        // Record the full time series at the receiver (one "frequency" trick:
        // run with no DFT and read pressure via a tiny custom loop instead).
        let freqs: Vec<f64> = (0..15).map(|i| 40.0 + i as f64 * 22.0).collect();
        // Separate incident and reflected pulses by running twice: once with
        // the real end wall and once with an anechoic-length tube (doubled),
        // whose window contains only the incident pulse.
        let t_inc = (15.0 - xr) / c;
        let t_ref = (15.0 + xr) / c;
        assert!(t_ref - t_inc > 0.01);
        let spec_all = run_cpu(&g, &[(src, 1.0)], &signal, &[rcv], &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
        let long = shoebox(len * 2.0 + cells(15.0), w, w);
        let lmats = vec![material(0.0); long.surfaces.len()];
        let gl = FdtdGrid::build(&long, &fit_surfaces(&lmats, (20.0, f_max)), c, f_max, ppw).unwrap();
        let shift = cells(30.0);
        let src_l = gl.nearest_air(glam::DVec3::new(15.0 + shift, w / 2.0, w / 2.0)).unwrap();
        let rcv_l = gl.nearest_air(glam::DVec3::new(xr + shift, w / 2.0, w / 2.0)).unwrap();
        let spec_inc = run_cpu(&gl, &[(src_l, 1.0)], &signal, &[rcv_l], &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
        // Reflected = total − incident; remove the extra path 2·x_r with the
        // scheme's own (axial) wavenumber.
        let mut worst = 0.0f64;
        for (fi, f) in freqs.iter().enumerate() {
            let w_ = 2.0 * PI * f;
            let k_num = 2.0 / g.dx * ((w_ * g.dt / 2.0).sin() / COURANT).asin();
            let r_meas = (spec_all[fi] - spec_inc[fi]) / spec_inc[fi] * Complex64::from_polar(1.0, k_num * 2.0 * xr);
            let z = fits[end].impedance(*f);
            let r_true = (z - 1.0) / (z + 1.0);
            let err = (r_meas - r_true).norm();
            worst = worst.max(err);
            if fi % 4 == 0 {
                eprintln!("{id:<18} {f:>5.0} Hz: |R| {:.3} (analytic {:.3}), ∠ {:+6.1}° ({:+6.1}°)",
                    r_meas.norm(), r_true.norm(), r_meas.arg().to_degrees(), r_true.arg().to_degrees());
            }
        }
        eprintln!("{id:<18} worst |ΔR| {worst:.4}");
        assert!(worst < 0.03, "{id}: worst |ΔR| {worst}");
    }
}

/// Strongly absorbing multi-branch boundaries over a long run must only
/// lose energy (no growth, no late blow-up).
#[test]
fn branch_boundaries_are_stable() {
    let c = Medium::default().speed_of_sound();
    let room = shoebox(3.0, 2.4, 2.1);
    let mats: Vec<_> = (0..room.surfaces.len())
        .map(|i| library(["mineral-wool-100", "audience-standing", "carpet-heavy", "curtain-heavy", "gypsum-board", "wood-panel"][i % 6]))
        .collect();
    let g = FdtdGrid::build(&room, &fit_surfaces(&mats, (20.0, 300.0)), c, 300.0, 12.0).unwrap();
    assert!(g.boundary.iter().any(|b| b.slots.len() >= 3));
    let src = g.nearest_air(glam::DVec3::new(0.7, 0.6, 0.5)).unwrap();
    let rcv = g.nearest_air(glam::DVec3::new(2.3, 1.7, 1.6)).unwrap();
    let steps = 200_000;
    let signal = source_signal(300.0, g.dt, steps);
    // Contributions to the spectrum from successive thirds of the run must
    // shrink (decay) and never grow. Near 0 Hz spring-type walls absorb
    // little, so that part decays slowly — but it must still decay.
    let f = [1.0, 80.0, 200.0];
    let run_to = |n: usize| run_cpu(&g, &[(src, 1.0)], &signal[..n], &[rcv], &f, &AtomicBool::new(false), &|_| {}).unwrap();
    let (a, b, c3) = (run_to(steps / 3), run_to(2 * steps / 3), run_to(steps));
    for (i, f) in f.iter().enumerate() {
        let (first, mid, last) = (a[i].norm(), (b[i] - a[i]).norm(), (c3[i] - b[i]).norm());
        eprintln!("{f} Hz: contributions by third {first:.2e}, {mid:.2e}, {last:.2e}");
        assert!(c3[i].norm().is_finite());
        assert!(mid < 0.1 * first && last < 0.1 * mid, "{f} Hz: not decaying");
    }
}

#[test]
fn gpu_matches_cpu_with_branches() {
    let Some(gpu) = wavey_solver::gpu::gpu() else {
        eprintln!("no GPU; skipping");
        return;
    };
    let c = Medium::default().speed_of_sound();
    let room = shoebox(5.0, 4.0, 3.0);
    let mats: Vec<_> = (0..room.surfaces.len())
        .map(|i| library(["audience-standing", "mineral-wool-50", "plaster-rough", "carpet-heavy", "wood-panel", "ceiling-industrial"][i % 6]))
        .collect();
    let g = FdtdGrid::build(&room, &fit_surfaces(&mats, (20.0, 300.0)), c, 300.0, 12.0).unwrap();
    let src = g.nearest_air(glam::DVec3::new(1.2, 1.1, 1.3)).unwrap();
    let rcv: Vec<usize> = [(4.1, 3.2, 1.5), (2.5, 0.5, 2.6)].iter().map(|p| g.nearest_air(glam::DVec3::new(p.0, p.1, p.2)).unwrap()).collect();
    let steps = (0.6 / g.dt) as usize;
    let signal = source_signal(300.0, g.dt, steps);
    let freqs: Vec<f64> = (0..30).map(|i| 30.0 + i as f64 * 9.0).collect();
    let cpu = run_cpu(&g, &[(src, 1.0)], &signal, &rcv, &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
    let gpu_out = gpu.run_fdtd(&g, &[(src, 1.0)], &signal, &rcv, &freqs, &AtomicBool::new(false), &|_| {}).unwrap().unwrap();
    let scale = cpu.iter().map(|v| v.norm()).fold(0.0, f64::max);
    let worst = cpu.iter().zip(&gpu_out).map(|(a, b)| (a - b).norm() / scale).fold(0.0, f64::max);
    eprintln!("{} boundary nodes; worst relative difference {worst:.2e}", g.boundary.len());
    assert!(worst < 1e-3);
}

/// The point of shared impedances: with frequency-dependent materials the
/// image sources and the wave solver must still agree in the crossover band.
#[test]
fn fdtd_agrees_with_ism_with_frequency_dependent_materials() {
    use wavey_solver::*;
    let c = Medium::default().speed_of_sound();
    let fs = FdtdSettings { crossover_hz: 500.0, duration_s: 0.4, points_per_wavelength: 15.0, source_order: 1, cabinets: false, accuracy: Default::default() };
    let dx = c / (fs.f_max() * fs.points_per_wavelength);
    let l = [7.0, 5.5, 3.5].map(|v: f64| (v / dx).round() * dx);
    let src = glam::DVec3::new(2.0, 2.0, 1.7);
    let mic = glam::DVec3::new(4.6, 3.3, 1.5);
    // 180–350 Hz: pure wave solver (weight 1); 360–707 Hz: the blend band.
    let mut inp = input(shoebox(l[0], l[1], l[2]), 0.5, omni_at(src), &[mic], 8, (180.0, 700.0));
    inp.settings.resolution = Resolution::FractionalOctave { n: 12 };
    for (i, s) in inp.room.surfaces.iter().enumerate() {
        inp.surface_materials[i] = match s.kind {
            wavey_geom::SurfaceKind::Floor => library("audience-standing"),
            wavey_geom::SurfaceKind::Ceiling => library("mineral-wool-50"),
            wavey_geom::SurfaceKind::Wall => library("curtain-heavy"),
        };
    }
    inp.settings.backend = BackendChoice::Gpu;
    // ISM-only reference using exactly the impedances the wave solver gets:
    // tabulate each fit as a material impedance.
    let band = (20.0, fs.f_max());
    let mut ref_inp = inp.clone();
    for m in ref_inp.surface_materials.iter_mut() {
        let fit = wavey_materials::fit_admittance(m, band, 2);
        let fq: Vec<f64> = (0..200).map(|i| band.0 * (band.1 / band.0).powf(i as f64 / 199.0)).collect();
        let z: Vec<Complex64> = fq.iter().map(|f| fit.impedance(*f)).collect();
        m.impedance = Some((
            wavey_materials::Spectrum::bands(&fq, &z.iter().map(|v| v.re).collect::<Vec<_>>()),
            wavey_materials::Spectrum::bands(&fq, &z.iter().map(|v| v.im).collect::<Vec<_>>()),
        ));
        m.scattering = wavey_materials::Spectrum::Scalar(0.0);
    }
    for m in inp.surface_materials.iter_mut() {
        m.scattering = wavey_materials::Spectrum::Scalar(0.0);
    }
    let ism = run(&ref_inp);
    inp.settings.fdtd = Some(fs);
    let wave = run(&inp);
    let d = unit_drives(&ism);
    let (a, b) = (ism.mic_pressure(0, &d), wave.mic_pressure(0, &d));
    let stats = |lo: f64, hi: f64| {
        let idx: Vec<usize> = (0..a.len()).filter(|&i| ism.freqs[i] >= lo && ism.freqs[i] <= hi).collect();
        let rms = (idx.iter().map(|&i| a[i].norm_sqr()).sum::<f64>() / idx.len() as f64).sqrt();
        let mut dbs: Vec<f64> = idx.iter().map(|&i| (20.0 * (b[i].norm() / a[i].norm()).log10()).abs()).collect();
        dbs.sort_by(f64::total_cmp);
        let worst = idx.iter().map(|&i| (a[i] - b[i]).norm() / rms).fold(0.0, f64::max);
        (dbs[dbs.len() / 2], worst)
    };
    for (fi, f) in ism.freqs.iter().enumerate() {
        eprintln!("{f:.1} Hz: ISM {:.2} dB, hybrid {:.2} dB, Δ {:+.2} dB {:+.1}°",
            20.0 * (a[fi].norm() / 2e-5).log10(), 20.0 * (b[fi].norm() / 2e-5).log10(),
            20.0 * (b[fi].norm() / a[fi].norm()).log10(), (b[fi] / a[fi]).arg().to_degrees());
    }
    let (med_pure, worst_pure) = stats(180.0, 350.0);
    let (med_blend, worst_blend) = stats(360.0, 707.2);
    eprintln!("pure wave band: median {med_pure:.2} dB, worst {:.1} dB re band level", 20.0 * worst_pure.log10());
    eprintln!("blend band:     median {med_blend:.2} dB, worst {:.1} dB re band level", 20.0 * worst_blend.log10());
    // Image sources with plane-wave reflection are approximate on soft
    // surfaces at low frequencies, so only the median is checked there.
    assert!(med_pure < 1.0);
    // Across the crossover the hybrid must join the image sources smoothly.
    assert!(med_blend < 0.5 && worst_blend < 0.25);
}


/// Directional sources: in free field (the run stops before reflections),
/// the response with all components, relative to the monopole alone, must
/// reproduce the speaker's directivity in the speaker's aimed frame.
#[test]
fn directional_source_reproduces_directivity() {
    use wavey_speaker::SpeakerInstance;
    let c = Medium::default().speed_of_sound();
    let room = shoebox(14.0, 12.0, 10.0);
    let mats = vec![material(0.2); room.surfaces.len()];
    let f_max = 500.0;
    let g = FdtdGrid::build(&room, &fit_surfaces(&mats, (20.0, f_max)), c, f_max, 12.0).unwrap();
    let centre = glam::DVec3::new(7.0, 6.0, 5.0);
    let src = g.nearest_air(centre).unwrap();
    let comps = source_components(&g, src, 1).unwrap();
    let r = 1.6;
    let angles = [0.0f64, 60.0, 90.0, 135.0, 180.0, 270.0];
    let rx: Vec<usize> = angles
        .iter()
        .map(|a| {
            let t = a.to_radians();
            g.nearest_air(centre + glam::DVec3::new(t.cos(), t.sin(), 0.0) * r).unwrap()
        })
        .chain([g.nearest_air(centre + glam::DVec3::new(0.0, 0.0, r)).unwrap()])
        .collect();
    let pos = |i: usize| {
        let (z, rr) = (i / (g.n[0] * g.n[1]), i % (g.n[0] * g.n[1]));
        g.center(rr % g.n[0], rr / g.n[0], z)
    };
    let steps = (0.018 / g.dt) as usize; // before the first reflection (≥ 5 m away)
    let signal = source_signal(f_max, g.dt, steps);
    let freqs = [120.0, 200.0, 300.0];
    let spectra: Vec<_> = comps
        .iter()
        .map(|taps| {
            let s = run_cpu(&g, taps, &signal, &rx, &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
            to_transfer(&s, &signal, &g, &freqs, c, 1.0, &[Complex64::new(1.0, 0.0); 3])
        })
        .collect();
    let models = [
        ("cardioid yaw 90°", wavey_speaker::samples::with_directivity("card", "Cardioid", |_, d: glam::DVec3| Complex64::new(0.5 + 0.5 * d.x, 0.0)), 90.0),
        ("2-way yaw 30°", wavey_speaker::samples::two_way(), 30.0),
    ];
    for (label, model, yaw) in models {
        let inst = SpeakerInstance { yaw_deg: yaw, ..Default::default() };
        let rot = inst.rotation();
        let mut worst = 0.0f64;
        for (fi, f) in freqs.iter().enumerate() {
            let (a0, a, res) = fit_first_order(&model, rot, *f);
            let w = component_weights(a0, a, 2.0 * PI * f / c, g.dx);
            for (ri, &cell) in rx.iter().enumerate() {
                let total: Complex64 = (0..comps.len()).map(|ci| w[ci] * spectra[ci][ri * freqs.len() + fi]).sum();
                let measured = total / spectra[0][ri * freqs.len() + fi];
                let dir_room = (pos(cell) - pos(src)).normalize();
                let want = model.directivity(*f, inst.to_local(dir_room));
                // First-order fit including the dipoles' near-field term
                // (1 + 1/(jkr)), which is physical at this distance.
                let k = 2.0 * PI * f / c;
                let dist = (pos(cell) - pos(src)).length();
                let near = Complex64::new(1.0, 0.0) + 1.0 / Complex64::new(0.0, k * dist);
                let fit = a0 + (a[0] * dir_room.x + a[1] * dir_room.y + a[2] * dir_room.z) * near;
                // Against that fit: the discretization error.
                let err_fit = (measured - fit).norm();
                worst = worst.max(err_fit);
                if fi == 1 {
                    eprintln!("{label:<18} {f:>4.0} Hz rx{ri}: |D| meas {:.3} fit {:.3} model {:.3}  (fit residual {:.0} %)",
                        measured.norm(), fit.norm(), want.norm(), res * 100.0);
                }
            }
        }
        eprintln!("{label}: worst |measured − first-order fit| {worst:.3}");
        assert!(worst < 0.05, "{label}: {worst}");
    }
}

/// Oblique incidence on the impedance boundary: a source and receivers
/// 1.2 m above the floor, incidence 30°–75°, gated before any other
/// reflection. The floor-reflected part is isolated with a second run over
/// a rigid floor: Q = (p_imp − p_dir) / (p_rigid − p_dir), with the direct
/// sound along a grid axis known analytically (including its dispersion).
/// It must match the exact point-source reflection off a locally reacting
/// plane (Weyl–van der Pol) for the fitted impedance.
#[test]
fn oblique_reflection_matches_spherical_wave_theory() {
    use wavey_solver::reflection::{plane, spherical};
    let c = Medium::default().speed_of_sound();
    let room = shoebox(20.0, 14.0, 8.0);
    let f_max = 600.0;
    let floor = room.surfaces.iter().position(|s| s.kind == wavey_geom::SurfaceKind::Floor).unwrap();
    let freqs = [200.0, 300.0, 450.0];
    let run = |alpha: f64| {
        let mut mats = vec![material(0.0); room.surfaces.len()];
        mats[floor] = material(alpha);
        let fits = fit_surfaces(&mats, (20.0, f_max));
        let g = FdtdGrid::build(&room, &fits, c, f_max, 10.0).unwrap();
        (g, fits)
    };
    let alpha = 0.3;
    let (g_imp, fits) = run(alpha);
    let (g_rig, _) = run(0.0);
    let pos = |g: &FdtdGrid, i: usize| {
        let (z, r) = (i / (g.n[0] * g.n[1]), i % (g.n[0] * g.n[1]));
        g.center(r % g.n[0], r / g.n[0], z)
    };
    let src = g_imp.nearest_air(glam::DVec3::new(8.0, 7.0, 1.2)).unwrap();
    let sp = pos(&g_imp, src);
    let h = sp.z; // the floor is at z = 0
    let angles = [30.0f64, 45.0, 60.0, 70.0, 75.0];
    let rx: Vec<usize> = angles.iter().map(|a| g_imp.nearest_air(sp + glam::DVec3::new(2.0 * h * a.to_radians().tan(), 0.0, 0.0)).unwrap()).collect();
    let steps = (12.0 / c / g_imp.dt) as usize;
    let signal = source_signal(f_max, g_imp.dt, steps);
    let h0 = vec![Complex64::new(1.0, 0.0); freqs.len()];
    let spectra = |g: &FdtdGrid| {
        let s = match wavey_solver::gpu::gpu() {
            Some(gpu) => gpu.run_fdtd(g, &[(src, 1.0)], &signal, &rx, &freqs, &AtomicBool::new(false), &|_| {}).unwrap().unwrap(),
            None => run_cpu(g, &[(src, 1.0)], &signal, &rx, &freqs, &AtomicBool::new(false), &|_| {}).unwrap(),
        };
        to_transfer(&s, &signal, g, &freqs, c, 1.0, &h0)
    };
    let (p_imp, p_rig) = (spectra(&g_imp), spectra(&g_rig));
    let nf = freqs.len();
    let mut worst = 0.0f64;
    for (ri, &cell) in rx.iter().enumerate() {
        let xp = pos(&g_imp, cell);
        let d = (xp - sp).length();
        let r2 = (d * d + 4.0 * h * h).sqrt();
        let cos = 2.0 * h / r2;
        for (fi, f) in freqs.iter().enumerate() {
            let k = 2.0 * PI * f / c;
            let lag = FdtdSettings::phase_error_deg_per_m(*f, c, g_imp.dx).to_radians() * d;
            let dir = Complex64::from_polar(1.0 / d, -k * d - lag);
            let q = (p_imp[ri * nf + fi] - dir) / (p_rig[ri * nf + fi] - dir);
            let zeta = fits[floor].impedance(*f);
            let (qs, qp) = (spherical(zeta, cos, k, r2), plane(zeta, cos));
            let err = (q - qs).norm();
            worst = worst.max(err);
            eprintln!(
                "θ {:4.1}° {f:>3} Hz: |Q| wave {:.3}  spherical {:.3}  plane {:.3}   |Δ| {err:.3}",
                cos.acos().to_degrees(),
                q.norm(),
                qs.norm(),
                qp.norm()
            );
        }
    }
    assert!(worst < 0.05, "worst |ΔQ| {worst}");
}

/// The crossover blend keeps energy: identical fields pass unchanged, and
/// fields with unrelated phases keep the weighted mean energy (a plain
/// complex blend would lose up to 3 dB mid-band).
#[test]
fn crossover_blend_is_power_complementary() {
    let a = Complex64::from_polar(2.0, 0.3);
    for w in [0.0, 0.25, 0.5, 0.75, 1.0] {
        assert!((blend(a, a, w) - a).norm() < 1e-12);
    }
    // Average over random relative phases.
    let (mut e_blend, mut e_plain) = (0.0, 0.0);
    let n = 360;
    for i in 0..n {
        let b = Complex64::from_polar(1.0, 2.0 * PI * i as f64 / n as f64);
        e_blend += blend(a, b, 0.5).norm_sqr();
        e_plain += (a * 0.5 + b * 0.5).norm_sqr();
    }
    let want = 0.5 * 4.0 + 0.5 * 1.0;
    assert!((e_blend / n as f64 - want).abs() < 1e-9, "{}", e_blend / n as f64);
    assert!(e_plain / (n as f64) < 0.6 * want);
    // Exact cancellation keeps the energy.
    let v = blend(a, -a, 0.5);
    assert!((v.norm_sqr() - 4.0).abs() < 1e-9);
}
