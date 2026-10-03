//! Directivity diagnostics: how well low-order source models fit a
//! speaker's balloon, with and without moving the acoustic centre.
//! `cargo run --example dirdiag -- <file.waveyspk>`
use glam::DVec3;
use num_complex::Complex64;
use wavey_speaker::SpeakerModel;

fn dirs(n: usize) -> Vec<DVec3> {
    let g = std::f64::consts::PI * (3.0 - 5f64.sqrt());
    (0..n).map(|i| {
        let z = 1.0 - 2.0 * (i as f64 + 0.5) / n as f64;
        let r = (1.0 - z * z).sqrt();
        DVec3::new(r * (g * i as f64).cos(), r * (g * i as f64).sin(), z)
    }).collect()
}

fn basis(u: DVec3, order: usize) -> Vec<f64> {
    let mut b = vec![1.0];
    if order >= 1 { b.extend([u.x, u.y, u.z]); }
    if order >= 2 { b.extend([u.x * u.y, u.x * u.z, u.y * u.z, u.x * u.x - u.y * u.y, 3.0 * u.z * u.z - 1.0]); }
    b
}

/// Least-squares residual (relative rms) of fitting `d` with the basis.
fn residual(us: &[DVec3], d: &[Complex64], order: usize) -> f64 {
    let n = basis(DVec3::X, order).len();
    let mut m = vec![vec![0.0; n]; n];
    let mut rhs = vec![Complex64::new(0.0, 0.0); n];
    for (u, v) in us.iter().zip(d) {
        let b = basis(*u, order);
        for i in 0..n { for j in 0..n { m[i][j] += b[i] * b[j]; } rhs[i] += v * b[i]; }
    }
    let mut x = rhs.clone();
    for c in 0..n {
        let p = (c..n).max_by(|a, b| m[*a][c].abs().total_cmp(&m[*b][c].abs())).unwrap();
        m.swap(c, p); x.swap(c, p);
        for r in 0..n { if r != c { let k = m[r][c] / m[c][c]; for cc in 0..n { m[r][cc] -= k * m[c][cc]; } x[r] = x[r] - x[c] * k; } }
    }
    let a: Vec<Complex64> = (0..n).map(|i| x[i] / m[i][i]).collect();
    let (mut e, mut t) = (0.0, 0.0);
    for (u, v) in us.iter().zip(d) {
        let fit: Complex64 = basis(*u, order).iter().zip(&a).map(|(b, a)| a * *b).sum();
        e += (fit - v).norm_sqr(); t += v.norm_sqr();
    }
    (e / t).sqrt()
}

fn main() {
    let path = std::env::args().nth(1).expect("waveyspk path");
    let m = SpeakerModel::load(std::path::Path::new(&path)).unwrap();
    println!("{} ac {:?}", m.name, m.acoustic_center_m);
    let us = dirs(600);
    let c = 343.0;
    for f in [40.0, 63.0, 100.0, 160.0, 250.0, 400.0] {
        let k = 2.0 * std::f64::consts::PI * f / c;
        let d: Vec<Complex64> = us.iter().map(|u| m.directivity(f, *u)).collect();
        // Best extra centre shift along x (forward axis) in ±1 m.
        let mut best = (0.0, f64::INFINITY);
        for i in -100..=100 {
            let s = i as f64 * 0.01;
            let dd: Vec<Complex64> = us.iter().zip(&d).map(|(u, v)| v * Complex64::from_polar(1.0, -k * s * u.x)).collect();
            let r = residual(&us, &dd, 1);
            if r < best.1 { best = (s, r); }
        }
        println!("{f:>5} Hz  order1 {:>5.1}%  order2 {:>5.1}%  order1+shift {:+.2} m → {:>5.1}%",
            residual(&us, &d, 1) * 100.0, residual(&us, &d, 2) * 100.0, best.0, best.1 * 100.0);
    }
}
