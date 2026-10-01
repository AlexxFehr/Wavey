//! Speaker directivity resampled onto the simulation frequency grid.
//!
//! The angular lookup (cell + weights) depends only on direction, so it is
//! computed once per path and reused for every frequency. The same layout
//! and interpolation are used by the GPU kernel.

use glam::DVec3;
use num_complex::Complex64;
use wavey_speaker::SpeakerModel;

/// Four grid corners and bilinear weights for one direction.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Cell {
    pub idx: [u32; 4],
    pub w: [f32; 4],
}

#[derive(Debug, Clone)]
pub struct Directivity {
    /// None: omnidirectional.
    pub grid: Option<DirectivityGrid>,
}

#[derive(Debug, Clone)]
pub struct DirectivityGrid {
    pub theta_deg: Vec<f64>,
    pub phi_deg: Vec<f64>,
    /// `[freq][theta][phi]`, dB and degrees, already interpolated in frequency.
    pub mag_db: Vec<f32>,
    pub phase_deg: Vec<f32>,
}

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

pub fn wrap180(d: f64) -> f64 {
    (d + 180.0).rem_euclid(360.0) - 180.0
}

impl Directivity {
    pub fn from_model(model: &SpeakerModel, freqs: &[f64]) -> Self {
        let Some(b) = &model.balloon else { return Directivity { grid: None } };
        let (nt, np) = (b.theta_deg.len(), b.phi_deg.len());
        let mut mag_db = Vec::with_capacity(freqs.len() * nt * np);
        let mut phase_deg = Vec::with_capacity(mag_db.capacity());
        for &f in freqs {
            for t in &b.theta_deg {
                for p in &b.phi_deg {
                    // Evaluate exactly at grid angles: the model only interpolates in frequency.
                    let (t, p) = (t.to_radians(), p.to_radians());
                    let dir = DVec3::new(t.cos(), t.sin() * p.cos(), t.sin() * p.sin());
                    let d = model.directivity(f, dir);
                    mag_db.push((20.0 * d.norm().max(1e-6).log10()) as f32);
                    phase_deg.push(d.arg().to_degrees() as f32);
                }
            }
        }
        Directivity {
            grid: Some(DirectivityGrid { theta_deg: b.theta_deg.clone(), phi_deg: b.phi_deg.clone(), mag_db, phase_deg }),
        }
    }

    pub fn cell(&self, dir_local: DVec3) -> Cell {
        let Some(g) = &self.grid else { return Cell { idx: [0; 4], w: [1.0, 0.0, 0.0, 0.0] } };
        let theta = dir_local.x.clamp(-1.0, 1.0).acos().to_degrees();
        let phi = dir_local.z.atan2(dir_local.y).to_degrees().rem_euclid(360.0);
        let (ti, tj, tt) = bracket(&g.theta_deg, theta);
        let np = g.phi_deg.len();
        let (pi, pj, pt) = if np == 1 {
            (0, 0, 0.0)
        } else if phi >= g.phi_deg[np - 1] || phi < g.phi_deg[0] {
            let last = g.phi_deg[np - 1];
            let span = g.phi_deg[0] + 360.0 - last;
            (np - 1, 0, (phi - last).rem_euclid(360.0) / span)
        } else {
            bracket(&g.phi_deg, phi)
        };
        let at = |t: usize, p: usize| (t * np + p) as u32;
        Cell {
            idx: [at(ti, pi), at(ti, pj), at(tj, pi), at(tj, pj)],
            w: [
                ((1.0 - tt) * (1.0 - pt)) as f32,
                ((1.0 - tt) * pt) as f32,
                (tt * (1.0 - pt)) as f32,
                (tt * pt) as f32,
            ],
        }
    }

    pub fn cells_per_freq(&self) -> usize {
        self.grid.as_ref().map_or(1, |g| g.theta_deg.len() * g.phi_deg.len())
    }

    /// Directivity at frequency index `fi` for a precomputed cell.
    pub fn eval(&self, fi: usize, cell: &Cell) -> Complex64 {
        let Some(g) = &self.grid else { return Complex64::new(1.0, 0.0) };
        let base = fi * self.cells_per_freq();
        let p0 = g.phase_deg[base + cell.idx[0] as usize] as f64;
        let (mut db, mut ph) = (0.0, 0.0);
        for k in 0..4 {
            let w = cell.w[k] as f64;
            let i = base + cell.idx[k] as usize;
            db += w * g.mag_db[i] as f64;
            ph += w * (p0 + wrap180(g.phase_deg[i] as f64 - p0));
        }
        Complex64::from_polar(10f64.powf(db / 20.0), ph.to_radians())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wavey_speaker::samples;

    #[test]
    fn matches_model_at_grid_frequencies() {
        let m = samples::two_way();
        let freqs = vec![m.frequencies_hz[30], m.frequencies_hz[45]];
        let d = Directivity::from_model(&m, &freqs);
        for dir in [DVec3::new(0.8, 0.5, 0.33).normalize(), DVec3::new(-0.2, -0.9, 0.4).normalize()] {
            for (fi, f) in freqs.iter().enumerate() {
                let a = d.eval(fi, &d.cell(dir));
                let b = m.directivity(*f, dir);
                assert!((a - b).norm() < 1e-4, "{a} vs {b}");
            }
        }
    }
}
