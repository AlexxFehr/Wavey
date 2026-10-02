//! Frequency-dependent surface impedance from absorption coefficients.
//!
//! Wave-based solvers need a locally reacting surface impedance, while
//! material data are random-incidence absorption coefficients α(f). Following
//! the constrained-fit approach of Mondet et al. / Cardoso Soares et al.
//! (Appl. Acoust. 188, 2022), the normalized admittance β = ρc/Z is modelled
//! as a pure conductance plus parallel mass–spring–damper branches,
//!
//!   β(ω) = β₀ + Σ_b 1 / (r_b + jω m_b + k_b / (jω)),
//!
//! with all parameters ≥ 0. Such an admittance is passive by construction
//! (its real part is never negative), so a time-domain boundary built from
//! it cannot add energy — the stability condition stressed by Melander et
//! al. (IJHPCA 2023) for multipole boundary models. Parameters are found by
//! minimising the misfit between the model's statistical absorption (Paris
//! formula) and α(f) over the band of interest.

use std::f64::consts::PI;

use num_complex::Complex64;
use serde::{Deserialize, Serialize};

use crate::material::Material;

/// One series mass–spring–damper branch in normalized units: impedance
/// ζ_b(ω) = r + jω·m + k/(jω), with m in seconds and k in 1/s.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Branch {
    pub r: f64,
    pub m: f64,
    pub k: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdmittanceFit {
    /// Frequency-independent conductance.
    pub beta0: f64,
    pub branches: Vec<Branch>,
    /// Band the fit was made for (Hz).
    pub band: (f64, f64),
    /// RMS and maximum absolute error in α over the band.
    pub rms_error: f64,
    pub max_error: f64,
}

impl AdmittanceFit {
    /// Normalized admittance β(f).
    pub fn admittance(&self, f: f64) -> Complex64 {
        let jw = Complex64::new(0.0, 2.0 * PI * f.max(1e-6));
        let mut y = Complex64::new(self.beta0, 0.0);
        for b in &self.branches {
            y += 1.0 / (b.r + jw * b.m + b.k / jw);
        }
        y
    }

    /// Normalized impedance ζ(f) = 1/β(f) (capped for rigid surfaces).
    pub fn impedance(&self, f: f64) -> Complex64 {
        let y = self.admittance(f);
        if y.norm() < 1e-9 { Complex64::new(1e9, 0.0) } else { 1.0 / y }
    }

    pub fn absorption(&self, f: f64) -> f64 {
        paris(self.admittance(f), 256)
    }

    pub fn is_rigid(&self) -> bool {
        self.beta0 < 1e-12 && self.branches.is_empty()
    }
}

/// Statistical absorption of a locally reacting surface with admittance β.
fn paris(beta: Complex64, n: usize) -> f64 {
    let h = PI / 2.0 / n as f64;
    let mut a = 0.0;
    for i in 0..n {
        let th = (i as f64 + 0.5) * h;
        let c = th.cos();
        // R = (ζ cosθ − 1)/(ζ cosθ + 1) = (cosθ − β)/(cosθ + β)
        let r = (c - beta) / (c + beta);
        a += (1.0 - r.norm_sqr()) * (2.0 * th).sin() * h;
    }
    a
}

/// Nelder–Mead minimisation (small, dependency-free).
fn nelder_mead(f: &dyn Fn(&[f64]) -> f64, x0: &[f64], step: f64, iters: usize) -> (Vec<f64>, f64) {
    let n = x0.len();
    let mut pts: Vec<Vec<f64>> = vec![x0.to_vec()];
    for i in 0..n {
        let mut p = x0.to_vec();
        p[i] += step;
        pts.push(p);
    }
    let mut vals: Vec<f64> = pts.iter().map(|p| f(p)).collect();
    for _ in 0..iters {
        let mut idx: Vec<usize> = (0..=n).collect();
        idx.sort_by(|a, b| vals[*a].total_cmp(&vals[*b]));
        pts = idx.iter().map(|i| pts[*i].clone()).collect();
        vals = idx.iter().map(|i| vals[*i]).collect();
        if (vals[n] - vals[0]).abs() < 1e-14 {
            break;
        }
        let centroid: Vec<f64> = (0..n).map(|d| pts[..n].iter().map(|p| p[d]).sum::<f64>() / n as f64).collect();
        let along = |t: f64| -> Vec<f64> { (0..n).map(|d| centroid[d] + t * (pts[n][d] - centroid[d])).collect() };
        let xr = along(-1.0);
        let fr = f(&xr);
        if fr < vals[0] {
            let xe = along(-2.0);
            let fe = f(&xe);
            if fe < fr { pts[n] = xe; vals[n] = fe } else { pts[n] = xr; vals[n] = fr }
        } else if fr < vals[n - 1] {
            pts[n] = xr;
            vals[n] = fr;
        } else {
            let xc = along(if fr < vals[n] { -0.5 } else { 0.5 });
            let fc = f(&xc);
            if fc < vals[n].min(fr) {
                pts[n] = xc;
                vals[n] = fc;
            } else {
                for i in 1..=n {
                    pts[i] = (0..n).map(|d| pts[0][d] + 0.5 * (pts[i][d] - pts[0][d])).collect();
                    vals[i] = f(&pts[i]);
                }
            }
        }
    }
    let best = (0..=n).min_by(|a, b| vals[*a].total_cmp(&vals[*b])).unwrap();
    (pts[best].clone(), vals[best])
}

/// Highest branch quality factor allowed. Measured α(f) curves of real
/// absorbers are smooth; sharp resonances in a fit are artefacts that also
/// need very fine time steps to reproduce.
pub const MAX_Q: f64 = 2.0;

/// Parameters: ln β₀, then per branch (ln m, ln k, logit(Q / MAX_Q)); the
/// resistance follows as r = √(mk)/Q, so Q ≤ MAX_Q by construction.
fn decode(x: &[f64], nb: usize) -> (f64, Vec<Branch>) {
    let beta0 = x[0].exp();
    let branches = (0..nb)
        .map(|b| {
            let (m, k) = (x[1 + 3 * b].exp(), x[2 + 3 * b].exp());
            let q = MAX_Q / (1.0 + (-x[3 + 3 * b]).exp());
            Branch { r: (m * k).sqrt() / q.max(1e-9), m, k }
        })
        .collect();
    (beta0, branches)
}

fn encode_branch(r: f64, m: f64, k: f64) -> [f64; 3] {
    let q = ((m * k).sqrt() / r / MAX_Q).clamp(1e-6, 1.0 - 1e-6);
    [m.ln(), k.ln(), (q / (1.0 - q)).ln()]
}

/// Fits `material` over `band` (Hz) with up to `max_branches` resonant branches.
pub fn fit_admittance(material: &Material, band: (f64, f64), max_branches: usize) -> AdmittanceFit {
    let (lo, hi) = (band.0.max(1.0), band.1.max(band.0 + 1.0));
    let nf = 32;
    let freqs: Vec<f64> = (0..nf).map(|i| lo * (hi / lo).powf(i as f64 / (nf - 1) as f64)).collect();
    // A locally reacting surface cannot exceed α ≈ 0.951 (Paris formula).
    let target: Vec<f64> = freqs.iter().map(|f| material.absorption_at(*f).clamp(0.0, 0.95)).collect();
    let rigid = AdmittanceFit { beta0: 0.0, branches: vec![], band: (lo, hi), rms_error: 0.0, max_error: 0.0 };
    if target.iter().all(|a| *a < 1e-6) {
        let max = target.iter().copied().fold(0.0, f64::max);
        return AdmittanceFit { rms_error: max, max_error: max, ..rigid };
    }

    let errors = |beta0: f64, branches: &[Branch]| -> (f64, f64) {
        let fit = AdmittanceFit { beta0, branches: branches.to_vec(), ..rigid.clone() };
        let e: Vec<f64> = freqs.iter().zip(&target).map(|(f, t)| paris(fit.admittance(*f), 96) - t).collect();
        let rms = (e.iter().map(|v| v * v).sum::<f64>() / e.len() as f64).sqrt();
        (rms, e.iter().map(|v| v.abs()).fold(0.0, f64::max))
    };

    // Conductance-only start: the real admittance matching the mean absorption.
    let mean = target.iter().sum::<f64>() / target.len() as f64;
    let beta_mean = 1.0 / crate::material::fit_real_impedance(mean.max(1e-4));
    let mut best: Option<(f64, f64, Vec<Branch>)> = None;
    for nb in 0..=max_branches {
        let cost = |x: &[f64]| -> f64 {
            let (b0, br) = decode(x, nb);
            let mut s = 0.0;
            for (f, t) in freqs.iter().zip(&target) {
                let fit = AdmittanceFit { beta0: b0, branches: br.clone(), ..rigid.clone() };
                let e = paris(fit.admittance(*f), 48) - t;
                s += e * e;
            }
            s
        };
        // Deterministic restarts: corner frequencies spread over the band.
        let starts: Vec<Vec<f64>> = if nb == 0 {
            vec![vec![beta_mean.ln()]]
        } else {
            let mut v = Vec::new();
            for &corner in &[0.3, 1.0, 3.0] {
                for &level in &[0.3, 1.0, 3.0] {
                    let mut x = vec![(beta_mean * 0.3).max(1e-6).ln()];
                    for b in 0..nb {
                        let fc = (lo * hi).sqrt() * corner * 3f64.powi(b as i32);
                        let wc = 2.0 * PI * fc;
                        let r = 1.0 / (beta_mean * level).max(1e-6);
                        // Spring-dominated below fc, mass-dominated far above.
                        x.extend(encode_branch(r, r / (wc * 3.0), r * wc));
                    }
                    v.push(x);
                }
            }
            v
        };
        for x0 in starts {
            let (x, _) = nelder_mead(&cost, &x0, 1.0, 1500 * (1 + nb));
            let (x, _) = nelder_mead(&cost, &x, 0.2, 1000 * (1 + nb));
            let (b0, br) = decode(&x, nb);
            let (rms, _) = errors(b0, &br);
            if best.as_ref().is_none_or(|b| rms < b.0 - 1e-6) {
                best = Some((rms, b0, br));
            }
        }
        // Stop adding branches once the fit is good.
        if best.as_ref().is_some_and(|b| b.0 < 0.005) {
            break;
        }
    }
    let (_, beta0, branches) = best.unwrap();
    // Drop negligible branches (they only cost time in the solver).
    let branches: Vec<Branch> = branches
        .into_iter()
        .filter(|b| freqs.iter().any(|f| (1.0 / (b.r + Complex64::new(0.0, 2.0 * PI * f) * b.m + b.k / Complex64::new(0.0, 2.0 * PI * f))).norm() > 1e-5))
        .collect();
    let beta0 = if beta0 < 1e-7 { 0.0 } else { beta0 };
    let (rms_error, max_error) = errors(beta0, &branches);
    AdmittanceFit { beta0, branches, band: (lo, hi), rms_error, max_error }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_materials;

    #[test]
    fn library_materials_fit_over_low_band() {
        for m in builtin_materials() {
            let fit = fit_admittance(&m, (20.0, 424.0), 2);
            eprintln!(
                "{:<26} β0 {:.4} branches {} rms {:.4} max {:.4}",
                m.id, fit.beta0, fit.branches.len(), fit.rms_error, fit.max_error
            );
            assert!(fit.max_error < 0.05, "{}: max error {}", m.id, fit.max_error);
            assert!(fit.rms_error < 0.02, "{}: rms error {}", m.id, fit.rms_error);
        }
    }

    #[test]
    fn branches_are_broad() {
        for m in builtin_materials() {
            for b in fit_admittance(&m, (20.0, 707.0), 2).branches {
                assert!((b.m * b.k).sqrt() / b.r <= MAX_Q + 1e-9, "{}", m.id);
            }
        }
    }

    #[test]
    fn passive_admittance() {
        for m in builtin_materials() {
            let fit = fit_admittance(&m, (20.0, 424.0), 2);
            for i in 0..200 {
                let f = 1.0 + i as f64 * 25.0;
                assert!(fit.admittance(f).re >= 0.0);
            }
        }
    }

    #[test]
    fn audience_is_nearly_transparent_at_low_frequency() {
        let lib = builtin_materials();
        let crowd = lib.iter().find(|m| m.id == "audience-standing").unwrap();
        let fit = fit_admittance(crowd, (20.0, 424.0), 2);
        assert!(fit.absorption(40.0) < 0.08, "{}", fit.absorption(40.0));
        assert!(fit.absorption(300.0) > 0.3, "{}", fit.absorption(300.0));
    }
}

