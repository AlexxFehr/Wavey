use glam::DVec3;
use num_complex::Complex64;
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "waveyspk";
pub const VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum SpeakerError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid speaker file: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid speaker data: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    pub distance_m: f64,
    pub input_voltage_rms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OnAxis {
    pub magnitude_db_spl: Vec<f64>,
    pub phase_deg: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Enclosure {
    pub width_m: f64,
    pub height_m: f64,
    pub depth_m: f64,
}

impl Default for Enclosure {
    fn default() -> Self {
        Self { width_m: 0.3, height_m: 0.5, depth_m: 0.3 }
    }
}

/// Complex directivity on a θ/φ grid, flattened as `[freq][theta][phi]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Balloon {
    pub theta_deg: Vec<f64>,
    pub phi_deg: Vec<f64>,
    pub magnitude_db: Vec<f32>,
    pub phase_deg: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeakerModel {
    pub format: String,
    pub version: u32,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    pub frequencies_hz: Vec<f64>,
    pub reference: Reference,
    pub on_axis: OnAxis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_input_voltage_rms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_spl_db: Option<f64>,
    #[serde(default)]
    pub latency_s: f64,
    /// Acoustic center in the local frame, relative to the enclosure center.
    #[serde(default)]
    pub acoustic_center_m: [f64; 3],
    #[serde(default)]
    pub enclosure: Enclosure,
    /// None means omnidirectional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balloon: Option<Balloon>,
    /// True when phase was derived rather than measured.
    #[serde(default)]
    pub phase_estimated: bool,
}

const P_REF: f64 = 20e-6;

fn db_phase(db: f64, deg: f64) -> Complex64 {
    Complex64::from_polar(10f64.powf(db / 20.0), deg.to_radians())
}

/// Wraps an angle difference in degrees to (-180, 180].
fn wrap180(d: f64) -> f64 {
    let r = (d + 180.0).rem_euclid(360.0) - 180.0;
    if r == -180.0 { 180.0 } else { r }
}

/// Interpolates (dB, phase) pairs with weights, unwrapping phases around the first.
fn blend(items: &[(f64, f64, f64)]) -> (f64, f64) {
    let base = items[0].2;
    let mut db = 0.0;
    let mut ph = 0.0;
    for (w, d, p) in items {
        db += w * d;
        ph += w * (base + wrap180(p - base));
    }
    (db, ph)
}

/// Bracketing indices and weight for `x` in a sorted grid (clamped).
fn bracket(grid: &[f64], x: f64) -> (usize, usize, f64) {
    let n = grid.len();
    if n == 1 || x <= grid[0] {
        return (0, 0, 0.0);
    }
    if x >= grid[n - 1] {
        return (n - 1, n - 1, 0.0);
    }
    let i = grid.partition_point(|g| *g <= x) - 1;
    (i, i + 1, (x - grid[i]) / (grid[i + 1] - grid[i]))
}

impl SpeakerModel {
    pub fn load(path: &std::path::Path) -> Result<Self, SpeakerError> {
        let m: SpeakerModel = serde_json::from_slice(&std::fs::read(path)?)?;
        m.validate()?;
        Ok(m)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), SpeakerError> {
        std::fs::write(path, serde_json::to_vec(self)?)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), SpeakerError> {
        let bad = |s: &str| Err(SpeakerError::Invalid(s.into()));
        if self.format != FORMAT {
            return bad("format must be \"waveyspk\"");
        }
        if self.version > VERSION {
            return bad("speaker file is from a newer version of Wavey");
        }
        let n = self.frequencies_hz.len();
        if n == 0 || self.frequencies_hz[0] <= 0.0 || self.frequencies_hz.windows(2).any(|w| w[1] <= w[0]) {
            return bad("frequencies must be positive and increasing");
        }
        if self.on_axis.magnitude_db_spl.len() != n || self.on_axis.phase_deg.len() != n {
            return bad("on-axis arrays must match the frequency list");
        }
        if !(self.reference.distance_m > 0.0) || !(self.reference.input_voltage_rms > 0.0) {
            return bad("reference distance and input must be positive");
        }
        if let Some(b) = &self.balloon {
            let incr = |g: &[f64]| g.windows(2).all(|w| w[1] > w[0]);
            if b.theta_deg.is_empty() || !incr(&b.theta_deg) || b.theta_deg[0] < 0.0 || *b.theta_deg.last().unwrap() > 180.0 {
                return bad("balloon θ grid must increase within 0…180°");
            }
            if b.phi_deg.is_empty() || !incr(&b.phi_deg) || b.phi_deg[0] < 0.0 || *b.phi_deg.last().unwrap() >= 360.0 {
                return bad("balloon φ grid must increase within 0…360°");
            }
            let len = n * b.theta_deg.len() * b.phi_deg.len();
            if b.magnitude_db.len() != len || b.phase_deg.len() != len {
                return bad("balloon arrays must have frequencies × θ × φ entries");
            }
        }
        Ok(())
    }

    fn freq_bracket(&self, f: f64) -> (usize, usize, f64) {
        let (i, j, _) = bracket(&self.frequencies_hz, f);
        if i == j {
            return (i, j, 0.0);
        }
        let (a, b) = (self.frequencies_hz[i].ln(), self.frequencies_hz[j].ln());
        (i, j, (f.ln() - a) / (b - a))
    }

    /// Complex on-axis pressure (Pa RMS) at the reference distance and input.
    pub fn on_axis(&self, f: f64) -> Complex64 {
        let (i, j, t) = self.freq_bracket(f);
        let m = &self.on_axis;
        let (db, ph) = blend(&[
            (1.0 - t, m.magnitude_db_spl[i], m.phase_deg[i]),
            (t, m.magnitude_db_spl[j], m.phase_deg[j]),
        ]);
        db_phase(db, ph) * P_REF
    }

    /// On-axis sensitivity in dB SPL at the reference distance and input.
    pub fn sensitivity_db(&self, f: f64) -> f64 {
        20.0 * (self.on_axis(f).norm() / P_REF).log10()
    }

    /// Directivity for direction `dir` (unit vector in the local frame).
    pub fn directivity(&self, f: f64, dir: DVec3) -> Complex64 {
        let Some(b) = &self.balloon else { return Complex64::new(1.0, 0.0) };
        let theta = dir.x.clamp(-1.0, 1.0).acos().to_degrees();
        let phi = dir.z.atan2(dir.y).to_degrees().rem_euclid(360.0);
        let (fi, fj, ft) = self.freq_bracket(f);
        let (ti, tj, tt) = bracket(&b.theta_deg, theta);
        // φ wraps around: the cell after the last grid point closes back to the first.
        let np = b.phi_deg.len();
        let (pi, pj, pt) = if np == 1 {
            (0, 0, 0.0)
        } else if phi >= b.phi_deg[np - 1] || phi < b.phi_deg[0] {
            let last = b.phi_deg[np - 1];
            let span = b.phi_deg[0] + 360.0 - last;
            let d = (phi - last).rem_euclid(360.0);
            (np - 1, 0, d / span)
        } else {
            bracket(&b.phi_deg, phi)
        };
        let nt = b.theta_deg.len();
        let idx = |f: usize, t: usize, p: usize| (f * nt + t) * np + p;
        let mut items = Vec::with_capacity(8);
        for (f, wf) in [(fi, 1.0 - ft), (fj, ft)] {
            for (t, wt) in [(ti, 1.0 - tt), (tj, tt)] {
                for (p, wp) in [(pi, 1.0 - pt), (pj, pt)] {
                    let w = wf * wt * wp;
                    if w > 0.0 || items.is_empty() {
                        let k = idx(f, t, p);
                        items.push((w, b.magnitude_db[k] as f64, b.phase_deg[k] as f64));
                    }
                }
            }
        }
        let (db, ph) = blend(&items);
        db_phase(db, ph)
    }

    /// Lowest and highest frequency with data.
    pub fn frequency_range(&self) -> (f64, f64) {
        (self.frequencies_hz[0], *self.frequencies_hz.last().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::samples;

    #[test]
    fn samples_validate_and_round_trip() {
        for m in samples::all() {
            m.validate().unwrap();
            let json = serde_json::to_vec(&m).unwrap();
            let back: SpeakerModel = serde_json::from_slice(&json).unwrap();
            assert_eq!(back.id, m.id);
            assert!((back.sensitivity_db(1000.0) - m.sensitivity_db(1000.0)).abs() < 1e-6);
        }
    }

    #[test]
    fn on_axis_directivity_is_unity() {
        for m in samples::all() {
            for f in [50.0, 500.0, 5000.0, 15000.0] {
                let d = m.directivity(f, DVec3::X);
                assert!((d.norm() - 1.0).abs() < 1e-3, "{} at {f}: {d}", m.id);
            }
        }
    }

    #[test]
    fn piston_matches_analytic_off_axis() {
        // Away from directivity nulls, where dB interpolation of 1/6-octave
        // data is inherently coarse.
        let a = 0.1;
        let m = samples::piston("p", "p", a);
        let c = 343.0;
        let grid = samples::sixth_octave_grid();
        for f in [grid[34], grid[40], 1500.0] {
            for deg in [20.0f64, 37.5, 60.0] {
                let th = deg.to_radians();
                let dir = DVec3::new(th.cos(), th.sin(), 0.0);
                let got = m.directivity(f, dir).norm();
                let want = samples::piston_directivity(2.0 * std::f64::consts::PI * f / c * a, th).norm();
                let err_db = 20.0 * (got / want).log10();
                assert!(err_db.abs() < 1.0, "{f} Hz {deg}°: {err_db} dB");
            }
        }
    }

    #[test]
    fn phi_wraps_smoothly() {
        let m = samples::two_way();
        let f = 4000.0;
        let near = |phi: f64| {
            let th = 30f64.to_radians();
            let phi = phi.to_radians();
            m.directivity(f, DVec3::new(th.cos(), th.sin() * phi.cos(), th.sin() * phi.sin()))
        };
        // Values just either side of φ = 0 must agree (the field itself rotates
        // ~4.4° of phase per degree of φ here, from the tweeter's offset).
        assert!((near(359.95) - near(0.05)).norm() < 0.01);
    }
}

