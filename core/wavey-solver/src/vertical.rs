//! Vertical coherence of the late field between flat floor and ceiling.
//!
//! The ray-traced late field adds energy incoherently. Between a flat floor
//! and ceiling, though, every late path belongs to a vertical family of
//! images (z = 2nH ± z_s) whose members interfere: the field is a sum of
//! floor-to-ceiling modes, and a source or receiver height couples to each
//! mode as cos²(nπz/H). A source at mid-height, for example, drives only
//! every other mode. Averaged over a band and over positions in the plane,
//! the coherent level then differs from the incoherent one by a factor that
//! depends on frequency and both heights.
//!
//! [`coherence_gain`] computes that factor from the image lattice. Images
//! factor into horizontal images (from the walls), each carrying a full
//! vertical column (from floor and ceiling). Columns of different
//! horizontal images add incoherently; within a column the terms interfere.
//! Horizontal images lie at distance ρ with density ∝ ρ dρ and lose energy
//! to wall absorption over the horizontal free path, so
//!
//!   G = ∫ ρ e^{−ρ/λ} |Σ column|² dρ / ∫ ρ e^{−ρ/λ} Σ |column terms|² dρ,
//!
//! The numerator is the wave physics: specular amplitudes with the surface
//! impedance and spherical-wave reflection, plus the scattered energy as
//! incoherent. The denominator is the ray tracer's own model, (1 − α) per
//! hit at every angle, so G converts the ray-traced late energy rather than
//! an idealized one. Averaged over ±1/6 octave.
//! Validated against the wave solver in shoebox rooms (see tests/vertical.rs).
//!
//! Rooms that are not extrusions use the local column at each receiver: the
//! flat floor below and flat ceiling above it (stepped floors, a balcony
//! overhang). Where either is sloped, or the source stands under a different
//! column, the vertical images do not line up and the gain is 1.

use std::f64::consts::PI;

use glam::DVec3;
use num_complex::Complex64;
use wavey_geom::{Room, SurfaceKind};

use crate::scene::SurfaceAcoustics;

/// Gauss–Laguerre nodes and weights (5 points) for ∫₀^∞ e^{−x} f(x) dx.
const LAGUERRE: [(f64, f64); 5] = [
    (0.263_560_319_718_141, 0.521_755_610_582_809),
    (1.413_403_059_106_517, 0.398_666_811_083_176),
    (3.596_425_771_040_722, 0.075_942_449_681_708),
    (7.085_810_005_858_837, 0.003_611_758_679_922),
    (12.640_800_844_275_783, 0.000_023_369_972_386),
];

/// Floor and ceiling surfaces with their areas.
pub struct Horizontal {
    floor: Vec<(usize, f64)>,
    ceiling: Vec<(usize, f64)>,
    height: f64,
}

impl Horizontal {
    /// All floors and ceilings of an extruded room (flat, at one height);
    /// other rooms use [`Horizontal::local`].
    pub fn new(room: &Room) -> Self {
        let pick = |kind| {
            if !room.prism {
                return vec![];
            }
            room.surfaces.iter().enumerate().filter(|(_, s)| s.kind == kind).map(|(i, s)| (i, s.area)).collect()
        };
        Self { floor: pick(SurfaceKind::Floor), ceiling: pick(SurfaceKind::Ceiling), height: room.height }
    }

    /// One flat floor and ceiling: the local column at a receiver.
    pub fn local(col: &Column) -> Self {
        Self { floor: vec![(col.floor, 1.0)], ceiling: vec![(col.ceiling, 1.0)], height: col.height }
    }

    /// Area-weighted specular reflection factor (wave physics: impedance and
    /// spherical-wave reflection) of one family at grid frequency `fi`,
    /// wavenumber `k`, incidence `cos` and path length `r`.
    fn reflect(family: &[(usize, f64)], surfaces: &[SurfaceAcoustics], fi: usize, cos: f64, k: f64, r: f64) -> Complex64 {
        let area: f64 = family.iter().map(|f| f.1).sum();
        family.iter().map(|&(s, a)| surfaces[s].reflection_spherical(fi, cos, k, r) * a).sum::<Complex64>() / area
    }

    /// Area-weighted reflected energy per hit in the ray tracer's model:
    /// total (1 − α) and its specular part (1 − α)(1 − s).
    fn energy(family: &[(usize, f64)], surfaces: &[SurfaceAcoustics], fi: usize) -> (f64, f64) {
        let area: f64 = family.iter().map(|f| f.1).sum();
        let (mut total, mut spec) = (0.0, 0.0);
        for &(s, a) in family {
            let sa = &surfaces[s];
            let e = (1.0 - sa.absorption[fi]).max(0.0);
            total += e * a;
            spec += e * (1.0 - sa.scattering[fi]) * a;
        }
        (total / area, spec / area)
    }
}

/// The flat floor below and flat ceiling above a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Column {
    pub floor: usize,
    pub ceiling: usize,
    /// Height of the floor, and floor to ceiling (both rounded to 1 mm so
    /// equal columns compare equal).
    pub z_floor: f64,
    pub height: f64,
}

/// Surfaces within this of horizontal count as flat (about 2°).
const FLAT: f64 = 0.9994;

/// The column at `p`: None if the floor below or the ceiling above is not
/// flat (or `p` is outside the room).
pub fn column(room: &Room, p: DVec3) -> Option<Column> {
    let down = room.bvh.intersect(p, DVec3::NEG_Z, 1e-9, f64::INFINITY)?;
    let up = room.bvh.intersect(p, DVec3::Z, 1e-9, f64::INFINITY)?;
    let (floor, ceiling) = (room.tri_surface[down.triangle as usize] as usize, room.tri_surface[up.triangle as usize] as usize);
    if room.surfaces[floor].normal.z < FLAT || room.surfaces[ceiling].normal.z > -FLAT {
        return None;
    }
    let mm = |v: f64| (v * 1000.0).round() / 1000.0;
    Some(Column { floor, ceiling, z_floor: mm(p.z - down.t), height: mm(down.t + up.t) })
}

/// Where to evaluate the gain: frequency, index of the nearest grid
/// frequency (for surface data) and the horizontal energy e-folding
/// length λ (wall absorption and air) there.
#[derive(Debug, Clone, Copy)]
pub struct EvalPoint {
    pub f: f64,
    pub fi: usize,
    pub decay_len: f64,
}

/// Late-field gain at each evaluation point for a source at height `zs` and
/// a receiver at `zr`.
pub fn coherence_gain(geo: &Horizontal, surfaces: &[SurfaceAcoustics], points: &[EvalPoint], c: f64, zs: f64, zr: f64) -> Vec<f64> {
    use rayon::prelude::*;
    let h = geo.height;
    if geo.floor.is_empty() || geo.ceiling.is_empty() || !(zs > 0.0 && zs < h && zr > 0.0 && zr < h) {
        return vec![1.0; points.len()];
    }
    // Vertical images: (z, floor hits, ceiling hits).
    let images = |nmax: i64| {
        let mut v = Vec::new();
        for n in -nmax..=nmax {
            v.push((2.0 * n as f64 * h + zs, n.unsigned_abs() as i32, n.unsigned_abs() as i32));
            let total = (2 * n - 1).unsigned_abs() as i32;
            let low = if n <= 0 { (total + 1) / 2 } else { total / 2 };
            v.push((2.0 * n as f64 * h - zs, low, total - low));
        }
        v
    };
    points
        .par_iter()
        .map(|&EvalPoint { f, fi, decay_len }| {
            let (ef, ef_spec) = Horizontal::energy(&geo.floor, surfaces, fi);
            let (ec, ec_spec) = Horizontal::energy(&geo.ceiling, surfaces, fi);
            let (mut num, mut den) = (0.0, 0.0);
            // ρ = λx with weight ρ e^{−ρ/λ}: Gauss–Laguerre on x·g(λx).
            for (x, wx) in LAGUERRE {
                let rho = (decay_len * x).max(0.25 * h);
                let weight = wx * x;
                // Enough images that the farthest are well past grazing.
                let nmax = ((4.0 * rho / (2.0 * h)).ceil() as i64).clamp(4, 400);
                let imgs = images(nmax);
                const SUB: usize = 5;
                for j in 0..SUB {
                    let fs = f * 2f64.powf((j as f64 + 0.5) / SUB as f64 / 3.0 - 1.0 / 6.0);
                    let k = 2.0 * PI * fs / c;
                    let (mut coh, mut inc_a, mut inc_b) = (Complex64::new(0.0, 0.0), 0.0, 0.0);
                    for &(z, nf, nc) in &imgs {
                        let dz = z - zr;
                        let r = (rho * rho + dz * dz).sqrt();
                        let cos = dz.abs() / r;
                        let qf = Horizontal::reflect(&geo.floor, surfaces, fi, cos, k, r);
                        let qc = Horizontal::reflect(&geo.ceiling, surfaces, fi, cos, k, r);
                        let a = qf.powi(nf) * qc.powi(nc) / r;
                        coh += a * Complex64::from_polar(1.0, -k * r);
                        let r2 = r * r;
                        inc_a += ef_spec.powi(nf) * ec_spec.powi(nc) / r2;
                        inc_b += ef.powi(nf) * ec.powi(nc) / r2;
                    }
                    // Coherent specular column plus the scattered energy
                    // (incoherent), over the ray tracer's incoherent energy.
                    num += weight * (coh.norm_sqr() + (inc_b - inc_a).max(0.0));
                    den += weight * inc_b;
                }
            }
            num / den
        })
        .collect()
}

/// Evaluation points: the grid itself if it is coarse, otherwise a
/// 1/24-octave grid (the gain is smooth at that scale after the ±1/6-octave
/// average).
pub fn eval_points(freqs: &[f64], decay_len: impl Fn(f64) -> f64) -> Vec<EvalPoint> {
    let nearest = |f: f64| {
        (0..freqs.len()).min_by(|&a, &b| (freqs[a].ln() - f.ln()).abs().total_cmp(&(freqs[b].ln() - f.ln()).abs())).unwrap_or(0)
    };
    let (lo, hi) = (freqs[0], *freqs.last().unwrap());
    let coarse: Vec<f64> = if freqs.len() <= 2 + (24.0 * (hi / lo).log2()).ceil() as usize {
        freqs.to_vec()
    } else {
        let n = (24.0 * (hi / lo).log2()).ceil() as usize;
        (0..=n).map(|i| lo * (hi / lo).powf(i as f64 / n as f64)).collect()
    };
    coarse.into_iter().map(|f| EvalPoint { f, fi: nearest(f), decay_len: decay_len(f) }).collect()
}

/// Interpolates gains at `points` onto the grid `freqs` (log-frequency).
pub fn to_grid(points: &[EvalPoint], gains: &[f64], freqs: &[f64]) -> Vec<f32> {
    freqs
        .iter()
        .map(|&f| {
            let j = points.partition_point(|p| p.f < f);
            let g = if j == 0 {
                gains[0]
            } else if j >= points.len() {
                gains[points.len() - 1]
            } else {
                let (a, b) = (&points[j - 1], &points[j]);
                let t = (f.ln() - a.f.ln()) / (b.f.ln() - a.f.ln()).max(1e-12);
                gains[j - 1] + t * (gains[j] - gains[j - 1])
            };
            g as f32
        })
        .collect()
}
