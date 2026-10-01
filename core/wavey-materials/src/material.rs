use num_complex::Complex64;
use serde::{Deserialize, Serialize};

use crate::spectrum::Spectrum;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub id: String,
    pub name: String,
    /// Random-incidence (statistical) absorption coefficient α, 0…1.
    pub absorption: Spectrum,
    /// Scattering coefficient s, 0…1: fraction of reflected energy that is diffuse.
    #[serde(default)]
    pub scattering: Spectrum,
    /// Optional normalized surface impedance ζ = Z/ρc (real and imaginary
    /// parts). When present it defines phase and angle dependence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impedance: Option<(Spectrum, Spectrum)>,
    /// Derive angle dependence from α via an equivalent real impedance.
    #[serde(default)]
    pub angle_dependent: bool,
    /// Display color as `#rrggbb`.
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default)]
    pub builtin: bool,
    /// Where the data comes from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

fn default_color() -> String {
    "#b8b8b8".into()
}

impl Material {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() || self.name.trim().is_empty() {
            return Err("material needs an id and a name".into());
        }
        self.absorption.validate(0.0..=1.0).map_err(|e| format!("absorption: {e}"))?;
        self.scattering.validate(0.0..=1.0).map_err(|e| format!("scattering: {e}"))?;
        if let Some((re, im)) = &self.impedance {
            re.validate(1e-6..=1e9).map_err(|e| format!("impedance (real): {e}"))?;
            im.validate(-1e9..=1e9).map_err(|e| format!("impedance (imag): {e}"))?;
        }
        Ok(())
    }

    /// Specular pressure reflection coefficient at frequency `f` for a plane
    /// wave arriving at angle θ from the normal (`cos_theta` = cos θ).
    pub fn reflection(&self, f: f64, cos_theta: f64) -> Complex64 {
        let c = cos_theta.abs().max(1e-6);
        if let Some((re, im)) = &self.impedance {
            let zeta = Complex64::new(re.eval(f), im.eval(f));
            return (zeta * c - 1.0) / (zeta * c + 1.0);
        }
        let alpha = self.absorption.eval(f).clamp(0.0, 1.0);
        if self.angle_dependent {
            let zeta = fit_real_impedance(alpha);
            return Complex64::new((zeta * c - 1.0) / (zeta * c + 1.0), 0.0);
        }
        Complex64::new((1.0 - alpha).sqrt(), 0.0)
    }

    pub fn scattering_at(&self, f: f64) -> f64 {
        self.scattering.eval(f).clamp(0.0, 1.0)
    }

    pub fn absorption_at(&self, f: f64) -> f64 {
        if let Some((re, im)) = &self.impedance {
            let zeta = Complex64::new(re.eval(f), im.eval(f));
            return statistical_absorption(|c| (zeta * c - 1.0) / (zeta * c + 1.0));
        }
        self.absorption.eval(f).clamp(0.0, 1.0)
    }
}

/// Paris formula: α_st = ∫₀^{π/2} (1 − |R(θ)|²) sin 2θ dθ, given R as a
/// function of cos θ.
pub fn statistical_absorption(r: impl Fn(f64) -> Complex64) -> f64 {
    const N: usize = 256;
    let h = std::f64::consts::FRAC_PI_2 / N as f64;
    // Midpoint rule; the integrand is smooth.
    (0..N)
        .map(|i| {
            let th = (i as f64 + 0.5) * h;
            (1.0 - r(th.cos()).norm_sqr()) * (2.0 * th).sin() * h
        })
        .sum()
}

/// The real impedance ζ ≥ 1.5 (the high-impedance branch) whose statistical
/// absorption equals `alpha`. Values above the maximum attainable (≈0.95)
/// are clamped to it.
pub fn fit_real_impedance(alpha: f64) -> f64 {
    let a = |z: f64| statistical_absorption(|c| Complex64::new((z * c - 1.0) / (z * c + 1.0), 0.0));
    // α_st(ζ) peaks near ζ ≈ 1.57 and decreases monotonically above it.
    let (mut lo, mut hi) = (1.567, 1e7);
    if alpha >= a(lo) {
        return lo;
    }
    if alpha <= 0.0 {
        return hi;
    }
    for _ in 0..80 {
        let mid = (lo * hi).sqrt();
        if a(mid) > alpha { lo = mid } else { hi = mid }
    }
    (lo * hi).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mat(alpha: f64) -> Material {
        Material {
            id: "m".into(),
            name: "m".into(),
            absorption: Spectrum::Scalar(alpha),
            scattering: Spectrum::Scalar(0.1),
            impedance: None,
            angle_dependent: false,
            color: default_color(),
            builtin: false,
            source: None,
        }
    }

    #[test]
    fn energy_conservation_without_impedance() {
        let m = mat(0.36);
        assert!((1.0 - m.reflection(500.0, 0.7).norm_sqr() - 0.36).abs() < 1e-12);
    }

    #[test]
    fn rigid_and_matched_impedance() {
        let mut m = mat(0.0);
        m.impedance = Some((Spectrum::Scalar(1e9), Spectrum::Scalar(0.0)));
        assert!((m.reflection(100.0, 1.0) - Complex64::new(1.0, 0.0)).norm() < 1e-6);
        m.impedance = Some((Spectrum::Scalar(1.0), Spectrum::Scalar(0.0)));
        assert!(m.reflection(100.0, 1.0).norm() < 1e-9);
    }

    #[test]
    fn impedance_fit_reproduces_absorption() {
        for alpha in [0.02, 0.1, 0.3, 0.6, 0.9] {
            let z = fit_real_impedance(alpha);
            let got = statistical_absorption(|c| Complex64::new((z * c - 1.0) / (z * c + 1.0), 0.0));
            assert!((got - alpha).abs() < 1e-6, "α {alpha}: got {got}");
            let mut m = mat(alpha);
            m.angle_dependent = true;
            let st = statistical_absorption(|c| m.reflection(1000.0, c));
            assert!((st - alpha).abs() < 1e-6);
        }
    }
}
