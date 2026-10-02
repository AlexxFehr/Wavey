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
    let spec = run_cpu(&g, src, &signal, &[rcv], &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
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
    let spec = run_cpu(&g, src, &signal, &[rcv], &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
    let mag: Vec<f64> = spec.iter().map(|v| v.norm()).collect();
    let peaks: Vec<f64> = (1..mag.len() - 1).filter(|&i| mag[i] > mag[i - 1] && mag[i] > mag[i + 1]).map(|i| freqs[i]).collect();
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
    for m in modes {
        let nearest = peaks.iter().copied().min_by(|a, b| (a - m).abs().total_cmp(&(b - m).abs())).unwrap();
        eprintln!("mode {m:.2} Hz → peak {nearest:.2} Hz ({:+.2}%)", (nearest / m - 1.0) * 100.0);
        assert!((nearest / m - 1.0).abs() < 0.005);
    }
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
    let cpu = run_cpu(&g, src, &signal, &rcv, &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
    let t1 = std::time::Instant::now();
    let gpu_out = gpu.run_fdtd(&g, src, &signal, &rcv, &freqs, &AtomicBool::new(false), &|_| {}).unwrap().unwrap();
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
    let fs = FdtdSettings { crossover_hz: 250.0, duration_s: 0.4, points_per_wavelength: 15.0 };
    let dx = c / (fs.f_max() * fs.points_per_wavelength);
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
        let spec_all = run_cpu(&g, src, &signal, &[rcv], &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
        let long = shoebox(len * 2.0 + cells(15.0), w, w);
        let lmats = vec![material(0.0); long.surfaces.len()];
        let gl = FdtdGrid::build(&long, &fit_surfaces(&lmats, (20.0, f_max)), c, f_max, ppw).unwrap();
        let shift = cells(30.0);
        let src_l = gl.nearest_air(glam::DVec3::new(15.0 + shift, w / 2.0, w / 2.0)).unwrap();
        let rcv_l = gl.nearest_air(glam::DVec3::new(xr + shift, w / 2.0, w / 2.0)).unwrap();
        let spec_inc = run_cpu(&gl, src_l, &signal, &[rcv_l], &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
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
    let run_to = |n: usize| run_cpu(&g, src, &signal[..n], &[rcv], &f, &AtomicBool::new(false), &|_| {}).unwrap();
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
    let cpu = run_cpu(&g, src, &signal, &rcv, &freqs, &AtomicBool::new(false), &|_| {}).unwrap();
    let gpu_out = gpu.run_fdtd(&g, src, &signal, &rcv, &freqs, &AtomicBool::new(false), &|_| {}).unwrap().unwrap();
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
    let fs = FdtdSettings { crossover_hz: 500.0, duration_s: 0.4, points_per_wavelength: 15.0 };
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
