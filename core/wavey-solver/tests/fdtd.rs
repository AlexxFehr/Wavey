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
    let g = FdtdGrid::build(&room, &mats, c, f_max, 10.0, 400.0).unwrap();
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
    let g = FdtdGrid::build(&room, &mats, c, f_max, 10.0, 100.0).unwrap();
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
    let g = FdtdGrid::build(&room, &mats, c, 300.0, 12.0, 200.0).unwrap();
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
