//! Multi-source systems against exact free-field sums: an endfire subwoofer
//! array and a straight line array. Each source is simulated separately and
//! mixed with its own delay and gain, so the array response must equal the
//! analytic coherent sum Σ gᵢ e^{−jω τᵢ} e^{−jk rᵢ − m rᵢ/2}/rᵢ (relative to
//! one source, with air absorption m), over the audible range.

mod common;

use common::*;
use glam::DVec3;
use num_complex::Complex64;
use wavey_solver::*;

const C: f64 = 343.0;

/// Free-field room (direct sound only): sources around the centre of a
/// large box, mics at `mics`.
fn solve(sources: &[DVec3], mics: &[DVec3], freqs: (f64, f64)) -> SimulationResult {
    let room = shoebox(400.0, 400.0, 200.0);
    let mut inp = input(room, 1.0, omni_at(sources[0]), mics, 0, freqs);
    for (i, p) in sources.iter().enumerate().skip(1) {
        let mut s = omni_at(*p);
        s.instance.id = i as u64 + 1;
        inp.sources.push(s);
    }
    inp.settings.resolution = Resolution::FractionalOctave { n: 12 };
    let c = inp.medium.speed_of_sound();
    assert!((c - C).abs() < 1.0);
    run(&inp)
}

/// Array level re one source (dB): simulated and analytic, at mic `m`.
fn compare(r: &SimulationResult, src: &[DVec3], mic: DVec3, m: usize, delays_s: &[f64], gains: &[f64]) -> Vec<(f64, f64, f64)> {
    let c = wavey_materials::Medium::default().speed_of_sound();
    let mut drives = vec![vec![Complex64::new(0.0, 0.0); r.freqs.len()]; src.len()];
    for (i, d) in drives.iter_mut().enumerate() {
        for (k, f) in r.freqs.iter().enumerate() {
            d[k] = Complex64::from_polar(gains[i], -2.0 * std::f64::consts::PI * f * delays_s[i]);
        }
    }
    let p = r.mic_pressure(m, &drives);
    let mut one = vec![vec![Complex64::new(0.0, 0.0); r.freqs.len()]; src.len()];
    one[0] = vec![Complex64::new(1.0, 0.0); r.freqs.len()];
    let p1 = r.mic_pressure(m, &one);
    r.freqs
        .iter()
        .enumerate()
        .map(|(k, f)| {
            let kk = 2.0 * std::f64::consts::PI * f / c;
            // Air absorption (intensity coefficient m → pressure e^{−m r/2}).
            let air = wavey_materials::Medium::default().air_intensity_coefficient(*f) / 2.0;
            let field = |i: usize| {
                let ri = (mic - src[i]).length();
                Complex64::from_polar(gains[i] * (-air * ri).exp() / ri, -kk * ri - 2.0 * std::f64::consts::PI * f * delays_s[i])
            };
            let a: Complex64 = (0..src.len()).map(field).sum();
            let r0 = (mic - src[0]).length();
            let a1 = Complex64::from_polar((-air * r0).exp() / r0, -kk * r0);
            (*f, 20.0 * (p[k].norm() / p1[k].norm()).log10(), 20.0 * (a.norm() / a1.norm()).log10())
        })
        .collect()
}

#[test]
fn endfire_sub_array_matches_the_coherent_sum() {
    // Four subs 1.0 m apart along +x, firing towards −x: the ones in front
    // wait for the ones behind (delay = distance from the rearmost / c).
    let o = DVec3::new(200.0, 200.0, 100.0);
    let src: Vec<DVec3> = (0..4).map(|i| o + DVec3::new(i as f64, 0.0, 0.0)).collect();
    let mics = [o + DVec3::new(-30.0, 0.0, 0.0), o + DVec3::new(30.0, 0.0, 0.0), o + DVec3::new(0.0, 30.0, 0.0)];
    let r = solve(&src, &mics, (20.0, 200.0));
    let c = wavey_materials::Medium::default().speed_of_sound();
    let delays: Vec<f64> = (0..4).map(|i| (3 - i) as f64 / c).collect();
    for (m, name) in ["front", "rear", "side"].iter().enumerate() {
        let rows = compare(&r, &src, mics[m], m, &delays, &[1.0; 4]);
        let worst = rows.iter().map(|(_, s, a)| (s - a).abs()).fold(0.0, f64::max);
        let mid = rows[rows.len() / 2];
        eprintln!("endfire {name}: {:.0} Hz sim {:+.2} dB, exact {:+.2} dB (worst |Δ| {worst:.3} dB)", mid.0, mid.1, mid.2);
        assert!(worst < 0.02, "endfire {name}: worst deviation {worst} dB");
        if m == 0 {
            // In phase in front: four sources, slightly further away than the front one (≈ +11.6 dB).
            assert!(rows.iter().all(|(_, s, _)| *s > 11.0 && *s < 12.1), "endfire sums in phase in front");
        }
    }
}

#[test]
fn line_array_vertical_pattern_matches_the_coherent_sum() {
    // 16 elements 0.3 m apart (4.5 m tall), straight, no shading.
    let o = DVec3::new(200.0, 200.0, 80.0);
    let src: Vec<DVec3> = (0..16).map(|i| o + DVec3::new(0.0, 0.0, i as f64 * 0.3)).collect();
    let centre = o + DVec3::new(0.0, 0.0, 2.25);
    let angles = [0.0f64, 5.0, 10.0, 20.0, 40.0];
    let mics: Vec<DVec3> = angles.iter().map(|a| centre + DVec3::new(a.to_radians().cos(), 0.0, a.to_radians().sin()) * 40.0).collect();
    let r = solve(&src, &mics, (100.0, 16000.0));
    let zero = vec![0.0; 16];
    for (m, a) in angles.iter().enumerate() {
        let rows = compare(&r, &src, mics[m], m, &zero, &[1.0; 16]);
        // In the deep nulls a dB difference is meaningless: there the
        // complex error relative to the on-axis level (+24 dB re one source) counts.
        let on_axis = 10f64.powf(23.9 / 20.0);
        let worst = rows
            .iter()
            .map(|(_, s, x)| {
                if *x > 23.9 - 15.0 { (s - x).abs() } else { (10f64.powf(s / 20.0) - 10f64.powf(x / 20.0)).abs() / on_axis * 100.0 * 0.05 }
            })
            .fold(0.0, f64::max);
        eprintln!("line array {a:>4}°: 500 Hz sim {:+.2} dB, exact {:+.2} dB (worst |Δ| {worst:.3} dB over 100 Hz–16 kHz)",
            rows.iter().min_by(|p, q| (p.0 - 500.0).abs().total_cmp(&(q.0 - 500.0).abs())).unwrap().1,
            rows.iter().min_by(|p, q| (p.0 - 500.0).abs().total_cmp(&(q.0 - 500.0).abs())).unwrap().2);
        assert!(worst < 0.05, "line array {a}°: worst deviation {worst} dB");
    }
}
