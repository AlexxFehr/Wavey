//! Low-frequency wave solver: 3D FDTD on a cubic grid.
//!
//! Scheme: the standard rectilinear leapfrog for pressure,
//!   p⁺ = (2 − Kλ²) p + λ² Σ p_nb − p⁻   (K = 6 in air),
//! with locally reacting boundaries (Kowalczyk & van Walstijn; Bilbao): a
//! node with K air neighbours and normalized admittances βᵢ on its missing
//! faces updates as
//!   (1 + λB/2) p⁺ = (2 − Kλ²) p + λ² Σ_air p_nb − (1 − λB/2) p⁻,   B = Σβᵢ.
//! This is energy-stable for β ≥ 0 and λ ≤ 1/√3.
//!
//! The source is a soft point source s(t) (a differentiated Gaussian).
//! Injecting sⁿ while computing pⁿ⁺¹ is the centred discretization of
//! p_tt = c²∇²p + q(t)δ(x) with q = s·Δx³/Δt², whose free-field solution is
//! q(t − r/c)/(4πc²r). Receiver spectra divided by the source spectrum
//! therefore give the Green's function in the same units as the
//! image-source method.
//!
//! Boundaries are locally reacting with frequency-dependent admittance
//! β(ω) = β₀ + Σ_b 1/(r_b + jωm_b + k_b/(jω)), fitted to each material's
//! α(f) (see `wavey_materials::impedance_fit`). Each branch carries a
//! normalized face velocity u_b at half steps and its integral g_b, updated
//! with the trapezoidal branch equation centred at step n,
//! m(u⁺ − u⁻)/Δt + r(u⁺ + u⁻)/2 + k(g⁺ + g⁻)/2 = (p⁺ + 2p + p⁻)/4,
//! g⁺ = g⁻ + Δt(u⁺ + u⁻)/2. Branches without mass use r·u⁺ + … = (p⁺ + p)/2
//! instead (centred at n + ½), which keeps a pure conductance exact and has
//! no undamped alternating mode. The pressure update takes the face flux
//! −λ Σ(u⁺ − u⁻); because u⁺ is linear in p⁺ the coupled system is solved
//! per node in closed form. A pure conductance (m = k = 0) reduces exactly
//! to the classic scheme (1 + λβ/2)p⁺ = … − (1 − λβ/2)p⁻, and passive
//! branches (r, m, k ≥ 0) only remove energy.

use std::f64::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering};

use glam::DVec3;
use num_complex::Complex64;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use wavey_geom::Room;
use wavey_materials::{AdmittanceFit, Material, fit_admittance};

/// How finely the wave solver resolves the top of the requested band: the
/// grid is built for the highest frequency the solve needs times this
/// headroom (never above the crossover band's top). More headroom costs
/// roughly its fourth power in time and cuts numerical dispersion and the
/// staircase error of walls and cabinets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveAccuracy {
    /// Headroom 1: the grid just resolves the top frequency.
    Draft,
    /// Headroom 1.5.
    Standard,
    /// Headroom 2: well resolved (the default, and what projects without the
    /// setting use).
    #[default]
    High,
}

impl WaveAccuracy {
    pub fn headroom(self) -> f64 {
        match self {
            WaveAccuracy::Draft => 1.0,
            WaveAccuracy::Standard => 1.5,
            WaveAccuracy::High => 2.0,
        }
    }

    fn is_high(&self) -> bool {
        *self == WaveAccuracy::High
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FdtdSettings {
    /// Center of the one-octave crossover band to the geometrical results (Hz).
    pub crossover_hz: f64,
    /// Simulated duration (s); 0 or less chooses one from the reverberation time.
    pub duration_s: f64,
    /// Grid points per wavelength at the top of the crossover band. Higher
    /// values reduce numerical dispersion (phase error ∝ 1/ppw²) at a cost
    /// that grows with ppw⁴.
    #[serde(default = "default_ppw")]
    pub points_per_wavelength: f64,
    /// Directivity order of sources in the wave solver: 0 = omnidirectional
    /// (one run per speaker), 1 = monopole + dipoles fitted to the measured
    /// balloon (four runs per speaker).
    #[serde(default = "default_source_order")]
    pub source_order: u32,
    /// Speaker cabinets are rigid obstacles in the grid and each speaker is
    /// driven by a monopole at its acoustic centre on the baffle, so
    /// directivity, rear diffraction and cabinets shadowing each other come
    /// from the simulation (`source_order` is then unused).
    #[serde(default = "default_cabinets")]
    pub cabinets: bool,
    /// Headroom of the grid above the highest frequency the solve needs.
    /// Not stored when High, so older runs keep their fingerprints.
    #[serde(default, skip_serializing_if = "WaveAccuracy::is_high")]
    pub accuracy: WaveAccuracy,
}

fn default_source_order() -> u32 {
    1
}

fn default_cabinets() -> bool {
    true
}

fn default_ppw() -> f64 {
    15.0
}

impl Default for FdtdSettings {
    fn default() -> Self {
        Self {
            crossover_hz: 250.0,
            duration_s: 0.0,
            points_per_wavelength: default_ppw(),
            source_order: default_source_order(),
            cabinets: default_cabinets(),
            accuracy: WaveAccuracy::default(),
        }
    }
}
/// Courant number (the 3D stability limit).
pub const COURANT: f64 = 0.577_350_269_189_625_7;
/// Largest grid accepted.
pub const MAX_CELLS: usize = 600_000_000;

impl FdtdSettings {
    /// Highest frequency the FDTD result is used for (top of the crossover).
    pub fn f_max(&self) -> f64 {
        self.crossover_hz * 2f64.sqrt()
    }

    /// Highest frequency the grid resolves for a solve at `freqs`: the
    /// highest one the wave solver contributes to, times the accuracy's
    /// headroom, up to the top of the crossover band.
    pub fn grid_fmax(&self, freqs: &[f64]) -> f64 {
        let top = freqs.iter().copied().filter(|f| self.weight(*f) > 0.0).fold(0.0, f64::max);
        if top > 0.0 { (top * self.accuracy.headroom()).min(self.f_max()) } else { self.f_max() }
    }

    /// Worst-case (grid-axis) phase error in degrees per metre at `f`.
    pub fn phase_error_deg_per_m(f: f64, c: f64, dx: f64) -> f64 {
        let dt = COURANT * dx / c;
        let w = 2.0 * PI * f;
        let k_num = 2.0 / dx * ((w * dt / 2.0).sin() / COURANT).asin();
        (k_num - w / c).abs().to_degrees()
    }

    /// Weight of the FDTD result at `f`: 1 below the crossover band, 0 above,
    /// raised cosine over one octave centred on the crossover.
    pub fn weight(&self, f: f64) -> f64 {
        let x = (f / self.crossover_hz).log2(); // −½ … ½ across the band
        if x <= -0.5 {
            1.0
        } else if x >= 0.5 {
            0.0
        } else {
            0.5 * (1.0 + (PI * (x + 0.5)).cos())
        }
    }
}

/// One admittance branch at a boundary node, with time-step constants so
/// that u⁺ = (½ − w)/D·p⁺ + [cv·u⁻ − k·g⁻ + ½p + w·p⁻]/D.
#[derive(Debug, Clone, Copy)]
pub struct Slot {
    pub inv_d: f64,
    pub cv: f64,
    pub k: f64,
    /// ¼ for trapezoidal (mass) branches, 0 for resistive ones.
    pub w: f64,
}

/// A boundary node with frequency-dependent faces.
#[derive(Debug, Clone)]
pub struct BoundaryNode {
    pub cell: u32,
    pub slots: Vec<Slot>,
}

/// Voxelized room with boundary data.
pub struct FdtdGrid {
    pub origin: DVec3,
    pub dx: f64,
    pub dt: f64,
    pub n: [usize; 3],
    /// Bit 6: air. Bits 0–5: neighbour (−x, +x, −y, +y, −z, +z) is air.
    pub flags: Vec<u8>,
    /// Index into `boundary` per cell, or `NO_BOUNDARY`.
    pub bidx: Vec<u32>,
    pub boundary: Vec<BoundaryNode>,
}

pub const NO_BOUNDARY: u32 = u32::MAX;

/// Fits each distinct material over the wave solver's band, returning one
/// fit per surface (same order as `materials`).
pub fn fit_surfaces(materials: &[Material], band: (f64, f64)) -> Vec<AdmittanceFit> {
    let mut cache: std::collections::HashMap<String, AdmittanceFit> = Default::default();
    let unique: Vec<&Material> = {
        let mut seen = std::collections::HashSet::new();
        materials.iter().filter(|m| seen.insert(m.id.clone())).collect()
    };
    let fits: Vec<(String, AdmittanceFit)> = unique.par_iter().map(|m| (m.id.clone(), fit_admittance(m, band, 2))).collect();
    cache.extend(fits);
    materials.iter().map(|m| cache[&m.id].clone()).collect()
}

pub const AIR: u8 = 1 << 6;

impl FdtdGrid {
    pub fn index(&self, x: usize, y: usize, z: usize) -> usize {
        (z * self.n[1] + y) * self.n[0] + x
    }

    pub fn cells(&self) -> usize {
        self.n[0] * self.n[1] * self.n[2]
    }

    pub fn air_cells(&self) -> usize {
        self.flags.iter().filter(|f| **f & AIR != 0).count()
    }

    pub fn center(&self, x: usize, y: usize, z: usize) -> DVec3 {
        self.origin + DVec3::new(x as f64, y as f64, z as f64) * self.dx
    }

    /// Nearest air node to `p` (searching up to 16 cells out, so points
    /// inside a cabinet still find the air around it).
    pub fn nearest_air(&self, p: DVec3) -> Option<usize> {
        let q = (p - self.origin) / self.dx;
        let c = [q.x.round() as i64, q.y.round() as i64, q.z.round() as i64];
        for r in 0..17i64 {
            let mut best: Option<(f64, usize)> = None;
            for dz in -r..=r {
                for dy in -r..=r {
                    for dx in -r..=r {
                        let (x, y, z) = (c[0] + dx, c[1] + dy, c[2] + dz);
                        if x < 0 || y < 0 || z < 0 || x >= self.n[0] as i64 || y >= self.n[1] as i64 || z >= self.n[2] as i64 {
                            continue;
                        }
                        let i = self.index(x as usize, y as usize, z as usize);
                        if self.flags[i] & AIR != 0 {
                            let d = (self.center(x as usize, y as usize, z as usize) - p).length_squared();
                            if best.is_none_or(|b| d < b.0) {
                                best = Some((d, i));
                            }
                        }
                    }
                }
            }
            if let Some((_, i)) = best {
                return Some(i);
            }
        }
        None
    }

    /// Builds the grid for frequencies up to `f_max`; `fits` holds one
    /// admittance fit per room surface.
    pub fn build(room: &Room, fits: &[AdmittanceFit], c: f64, f_max: f64, ppw: f64) -> Result<Self, String> {
        Self::build_with(room, &[], fits, c, f_max, ppw)
    }

    /// Like [`build`](Self::build), with rigid convex obstacles (speaker
    /// cabinets) removed from the air.
    pub fn build_with(room: &Room, solids: &[Solid], fits: &[AdmittanceFit], c: f64, f_max: f64, ppw: f64) -> Result<Self, String> {
        // Fine enough for the wavelength and to resolve every cabinet (8 cells across its smallest side).
        let cabinet = solids.iter().map(|s| (s.hi - s.lo).min_element() / 8.0).fold(f64::INFINITY, f64::min);
        Self::build_dx(room, solids, fits, c, (c / (f_max * ppw)).min(cabinet))
    }

    /// Builds the grid with spacing `dx`.
    pub fn build_dx(room: &Room, solids: &[Solid], fits: &[AdmittanceFit], c: f64, dx: f64) -> Result<Self, String> {
        let dt = COURANT * dx / c;
        let (lo, hi) = room.bounds();
        // One solid layer of padding around the bounds.
        let origin = lo - DVec3::splat(dx * 0.5);
        let size = hi - lo;
        let n = [
            (size.x / dx).ceil() as usize + 2,
            (size.y / dx).ceil() as usize + 2,
            (size.z / dx).ceil() as usize + 2,
        ];
        let cells = n[0] * n[1] * n[2];
        if cells > MAX_CELLS {
            return Err(format!(
                "the wave solver grid would have {:.0} M cells; lower the crossover frequency",
                cells as f64 / 1e6
            ));
        }
        let mut g = FdtdGrid { origin, dx, dt, n, flags: vec![0; cells], bidx: vec![NO_BOUNDARY; cells], boundary: vec![] };
        let solid: Vec<bool> = (0..cells)
            .into_par_iter()
            .map(|i| {
                if solids.is_empty() {
                    return false;
                }
                let (z, r) = (i / (n[0] * n[1]), i % (n[0] * n[1]));
                let (y, x) = (r / n[0], r % n[0]);
                let p = g.center(x, y, z);
                solids.iter().any(|s| s.contains(p))
            })
            .collect();
        // Inside the room: per cell for extruded rooms (a cheap plan test),
        // by vertical column for others (one ray per column).
        let inside = if room.prism { Vec::new() } else { column_fill(room, &g) };
        let air: Vec<bool> = (0..cells)
            .into_par_iter()
            .map(|i| {
                let (z, r) = (i / (n[0] * n[1]), i % (n[0] * n[1]));
                let (y, x) = (r / n[0], r % n[0]);
                !solid[i]
                    && x > 0
                    && y > 0
                    && z > 0
                    && x < n[0] - 1
                    && y < n[1] - 1
                    && z < n[2] - 1
                    && if room.prism { room.contains(g.center(x, y, z)) } else { inside[i] }
            })
            .collect();
        let dirs: [(i64, i64, i64); 6] = [(-1, 0, 0), (1, 0, 0), (0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1)];
        // Per node: flags and the surfaces behind its missing faces.
        let data: Vec<(u8, Vec<usize>)> = (0..cells)
            .into_par_iter()
            .map(|i| {
                if !air[i] {
                    return (0, vec![]);
                }
                let (z, r) = (i / (n[0] * n[1]), i % (n[0] * n[1]));
                let (y, x) = (r / n[0], r % n[0]);
                let p = g.center(x, y, z);
                let mut flags = AIR;
                let mut faces = Vec::new();
                for (k, (ddx, ddy, ddz)) in dirs.iter().enumerate() {
                    let j = g.index((x as i64 + ddx) as usize, (y as i64 + ddy) as usize, (z as i64 + ddz) as usize);
                    if air[j] {
                        flags |= 1 << k;
                    } else if solid[j] {
                        // Cabinet face: rigid, no boundary branch.
                    } else {
                        // The surface between this node and the solid neighbour.
                        let dir = DVec3::new(*ddx as f64, *ddy as f64, *ddz as f64);
                        if let Some(h) = room.bvh.intersect(p, dir, 0.0, dx * 1.5) {
                            faces.push(room.tri_surface[h.triangle as usize] as usize);
                        }
                    }
                }
                (flags, faces)
            })
            .collect();
        for (i, (f, faces)) in data.into_iter().enumerate() {
            g.flags[i] = f;
            let slots = boundary_slots(&faces, fits, dt);
            if !slots.is_empty() {
                g.bidx[i] = g.boundary.len() as u32;
                g.boundary.push(BoundaryNode { cell: i as u32, slots });
            }
        }
        Ok(g)
    }
}

/// Inside flags of every cell: one upward ray per (x, y) column collects
/// the surface crossings, and a cell is inside after an odd number of them.
/// The ray is moved off the cell centres by a tiny irrational offset so it
/// does not run through mesh edges; hits of one surface side at the same
/// height (a shared edge) count once, both sides of a thin panel count twice.
fn column_fill(room: &Room, g: &FdtdGrid) -> Vec<bool> {
    let n = g.n;
    let off = DVec3::new(0.000_137_2 * g.dx, 0.000_291_8 * g.dx, 0.0);
    let start = g.origin.z - g.dx;
    let cols: Vec<Vec<bool>> = (0..n[0] * n[1])
        .into_par_iter()
        .map(|c| {
            let (x, y) = (c % n[0], c / n[0]);
            let o = DVec3::new(g.center(x, y, 0).x, g.center(x, y, 0).y, start) + off;
            let mut crossings: Vec<(f64, bool)> = Vec::new();
            for h in room.bvh.all_hits(o, DVec3::Z, 0.0, f64::INFINITY) {
                let [a, b, cc] = room.triangles[h.triangle as usize].map(|i| room.positions[i as usize]);
                let up = (b - a).cross(cc - a).z > 0.0;
                if crossings.last().is_some_and(|&(t, u)| h.t - t < 1e-9 && u == up) {
                    continue;
                }
                crossings.push((h.t, up));
            }
            let mut out = vec![false; n[2]];
            let mut k = 0;
            for (z, cell) in out.iter_mut().enumerate() {
                let tz = g.center(x, y, z).z - start;
                while k < crossings.len() && crossings[k].0 < tz {
                    k += 1;
                }
                *cell = k % 2 == 1;
            }
            out
        })
        .collect();
    let mut inside = vec![false; n[0] * n[1] * n[2]];
    for (c, col) in cols.iter().enumerate() {
        for (z, &v) in col.iter().enumerate() {
            inside[z * n[0] * n[1] + c] = v;
        }
    }
    inside
}

/// A rigid convex obstacle: the points with n·p ≤ d for every plane.
#[derive(Debug, Clone)]
pub struct Solid {
    pub planes: Vec<(DVec3, f64)>,
    pub lo: DVec3,
    pub hi: DVec3,
}

impl Solid {
    /// From a convex triangle mesh (outward winding) in the local frame,
    /// rotated by `rot` and moved to `pos`.
    pub fn from_mesh(verts: &[DVec3], tris: &[[u32; 3]], rot: glam::DMat3, pos: DVec3) -> Option<Solid> {
        if tris.is_empty() {
            return None;
        }
        let w: Vec<DVec3> = verts.iter().map(|v| rot * *v + pos).collect();
        let mut planes: Vec<(DVec3, f64)> = Vec::new();
        for t in tris {
            let [a, b, c] = t.map(|i| w[i as usize]);
            let n = (b - a).cross(c - a);
            if n.length() < 1e-12 {
                continue;
            }
            let n = n.normalize();
            let d = n.dot(a);
            if !planes.iter().any(|(m, e)| m.dot(n) > 1.0 - 1e-9 && (e - d).abs() < 1e-9) {
                planes.push((n, d));
            }
        }
        let lo = w.iter().fold(DVec3::splat(f64::INFINITY), |m, v| m.min(*v));
        let hi = w.iter().fold(DVec3::splat(f64::NEG_INFINITY), |m, v| m.max(*v));
        Some(Solid { planes, lo, hi })
    }

    pub fn contains(&self, p: DVec3) -> bool {
        p.cmpge(self.lo).all() && p.cmple(self.hi).all() && self.planes.iter().all(|(n, d)| n.dot(p) <= *d)
    }
}

impl FdtdGrid {
    /// Point source at `p` spread trilinearly over the surrounding air nodes
    /// (solid ones dropped, weights renormalized), so its position is not
    /// rounded to the grid.
    pub fn taps_at(&self, p: DVec3) -> Option<Vec<(usize, f64)>> {
        let q = (p - self.origin) / self.dx;
        let (x0, y0, z0) = (q.x.floor(), q.y.floor(), q.z.floor());
        let (fx, fy, fz) = (q.x - x0, q.y - y0, q.z - z0);
        let mut taps = Vec::with_capacity(8);
        for (dx, wx) in [(0, 1.0 - fx), (1, fx)] {
            for (dy, wy) in [(0, 1.0 - fy), (1, fy)] {
                for (dz, wz) in [(0, 1.0 - fz), (1, fz)] {
                    let (x, y, z) = (x0 as i64 + dx, y0 as i64 + dy, z0 as i64 + dz);
                    if x < 0 || y < 0 || z < 0 || x >= self.n[0] as i64 || y >= self.n[1] as i64 || z >= self.n[2] as i64 {
                        continue;
                    }
                    let i = self.index(x as usize, y as usize, z as usize);
                    let w = wx * wy * wz;
                    if self.flags[i] & AIR != 0 && w > 1e-9 {
                        taps.push((i, w));
                    }
                }
            }
        }
        let sum: f64 = taps.iter().map(|t| t.1).sum();
        if sum < 0.25 {
            return self.nearest_air(p).map(|i| vec![(i, 1.0)]);
        }
        Some(taps.into_iter().map(|(i, w)| (i, w / sum)).collect())
    }

    /// The driver point of a cabinet: on its baffle, on the axis through
    /// the acoustic centre `p` (the first point along the axis that is not
    /// inside a cabinet cell, so the source sits on the radiating face).
    pub fn baffle_point(&self, p: DVec3, axis: DVec3) -> DVec3 {
        let step = self.dx * 0.05;
        for k in 0..((2.0 / step) as usize) {
            let q = p + axis * (k as f64 * step);
            let r = ((q - self.origin) / self.dx).round();
            if r.cmplt(DVec3::ZERO).any() {
                continue;
            }
            let (x, y, z) = (r.x as usize, r.y as usize, r.z as usize);
            if x < self.n[0] && y < self.n[1] && z < self.n[2] && self.flags[self.index(x, y, z)] & AIR != 0 {
                // Nearest node is air: halfway between it and the solid cell is the face.
                return q - axis * (self.dx * 0.5).min(k as f64 * step);
            }
        }
        p
    }

    /// First air node along `axis` from `p` (within 2 m): where a driver on
    /// the cabinet's baffle radiates into the air.
    pub fn air_along(&self, p: DVec3, axis: DVec3) -> Option<usize> {
        let step = self.dx * 0.5;
        for k in 0..((2.0 / step) as usize) {
            let q = p + axis * (k as f64 * step);
            let r = (q - self.origin) / self.dx;
            let (x, y, z) = (r.x.round(), r.y.round(), r.z.round());
            if x < 0.0 || y < 0.0 || z < 0.0 {
                continue;
            }
            let (x, y, z) = (x as usize, y as usize, z as usize);
            if x < self.n[0] && y < self.n[1] && z < self.n[2] {
                let i = self.index(x, y, z);
                if self.flags[i] & AIR != 0 {
                    return Some(i);
                }
            }
        }
        self.nearest_air(p)
    }
}

/// Branch slots of a node whose missing faces lie on `faces` (surface
/// indices). Faces with the same fit share slots, with admittance × count.
fn boundary_slots(faces: &[usize], fits: &[AdmittanceFit], dt: f64) -> Vec<Slot> {
    let mut counts: Vec<(usize, usize)> = Vec::new();
    for &s in faces {
        match counts.iter_mut().find(|(t, _)| fits[*t] == fits[s]) {
            Some(c) => c.1 += 1,
            None => counts.push((s, 1)),
        }
    }
    let mut slots = Vec::new();
    for (s, count) in counts {
        let fit = &fits[s];
        let n = count as f64;
        let mut add = |r: f64, m: f64, k: f64| {
            // n identical faces in parallel: impedance divided by n.
            let (r, m, k) = (r / n, m / n, k / n);
            let (d, cv, w) = if m > 0.0 {
                // Trapezoidal (second order), centred at step n.
                (m / dt + r / 2.0 + k * dt / 4.0, m / dt - r / 2.0 - k * dt / 4.0, 0.25)
            } else {
                // Centred at n + ½: exact classic form for a pure conductance.
                (r + k * dt / 2.0, -k * dt / 2.0, 0.0)
            };
            slots.push(Slot { inv_d: 1.0 / d, cv, k, w });
        };
        if fit.beta0 > 0.0 {
            add(1.0 / fit.beta0, 0.0, 0.0);
        }
        for b in &fit.branches {
            add(b.r, b.m, b.k);
        }
    }
    slots
}

/// Branch states (normalized face velocity u at n−½ and its integral g).
#[derive(Debug, Clone, Copy, Default)]
pub struct SlotState {
    pub u: f64,
    pub g: f64,
}

/// Boundary update for one node: returns pⁿ⁺¹ and advances the branch states.
#[inline]
pub fn boundary_update(slots: &[Slot], states: &mut [SlotState], p: f64, p_prev: f64, lap: f64, lam: f64, dt: f64) -> f64 {
    // Each branch: u⁺ = a·p⁺ + e. The flux −λΣ(u⁺ − u⁻) makes p⁺ solvable.
    let (mut s1, mut s2) = (0.0, 0.0);
    for (sl, st) in slots.iter().zip(states.iter()) {
        let e = (sl.cv * st.u - sl.k * st.g + 0.5 * p + sl.w * p_prev) * sl.inv_d;
        s1 += (0.5 - sl.w) * sl.inv_d;
        s2 += e - st.u;
    }
    let next = (2.0 * p - p_prev + lap - lam * s2) / (1.0 + lam * s1);
    for (sl, st) in slots.iter().zip(states.iter_mut()) {
        let e = (sl.cv * st.u - sl.k * st.g + 0.5 * p + sl.w * p_prev) * sl.inv_d;
        let u = (0.5 - sl.w) * sl.inv_d * next + e;
        st.g += 0.5 * dt * (u + st.u);
        st.u = u;
    }
    next
}

/// Differentiated Gaussian with little energy above `f_max`.
pub fn source_signal(f_max: f64, dt: f64, steps: usize) -> Vec<f64> {
    let sigma = 0.342 / f_max;
    let t0 = 6.0 * sigma;
    (0..steps)
        .map(|n| {
            let t = n as f64 * dt - t0;
            -t / (sigma * sigma) * (-(t * t) / (2.0 * sigma * sigma)).exp() * dt
        })
        .collect()
}

/// Accumulated DFT at the receivers: `[receiver][freq]` of Σ pⁿ e^{−jωnΔt}.
pub type Spectra = Vec<Complex64>;

/// CPU reference implementation. Returns per-receiver spectra (receiver-major).
pub fn run_cpu(
    g: &FdtdGrid,
    source: &[(usize, f64)],
    signal: &[f64],
    receivers: &[usize],
    freqs: &[f64],
    cancel: &AtomicBool,
    progress: &(dyn Fn(f64) + Sync),
) -> Option<Spectra> {
    run_cpu_with(g, source, signal, receivers, freqs, cancel, progress, None)
}

/// Pressure time series at the receivers (`[receiver][step]`), the
/// response to `signal`: for time-domain room parameters.
pub fn run_cpu_ir(
    g: &FdtdGrid,
    source: &[(usize, f64)],
    signal: &[f64],
    receivers: &[usize],
    cancel: &AtomicBool,
    progress: &(dyn Fn(f64) + Sync),
) -> Option<Vec<Vec<f64>>> {
    let mut rec = vec![Vec::with_capacity(signal.len()); receivers.len()];
    run_cpu_with(g, source, signal, receivers, &[], cancel, progress, Some(&mut rec))?;
    Some(rec)
}

#[allow(clippy::too_many_arguments)]
fn run_cpu_with(
    g: &FdtdGrid,
    source: &[(usize, f64)],
    signal: &[f64],
    receivers: &[usize],
    freqs: &[f64],
    cancel: &AtomicBool,
    progress: &(dyn Fn(f64) + Sync),
    mut record: Option<&mut Vec<Vec<f64>>>,
) -> Option<Spectra> {
    let cells = g.cells();
    let l2 = COURANT * COURANT;
    let lam = COURANT;
    let (nx, nxy) = (g.n[0], g.n[0] * g.n[1]);
    let offs: [isize; 6] = [-1, 1, -(nx as isize), nx as isize, -(nxy as isize), nxy as isize];
    let mut p = vec![0f64; cells];
    let mut prev = vec![0f64; cells];
    let mut states: Vec<Vec<SlotState>> = g.boundary.iter().map(|b| vec![SlotState::default(); b.slots.len()]).collect();
    let mut bnext = vec![0f64; g.boundary.len()];
    let nf = freqs.len();
    let mut acc = vec![Complex64::new(0.0, 0.0); receivers.len() * nf];
    let steps = signal.len();
    for n in 0..steps {
        if n % 64 == 0 {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            progress(n as f64 / steps as f64);
        }
        // Record pⁿ.
        for (ri, &r) in receivers.iter().enumerate() {
            let v = p[r];
            if let Some(rec) = record.as_deref_mut() {
                rec[ri].push(v);
            }
            for (fi, f) in freqs.iter().enumerate() {
                let ph = -2.0 * PI * (f * g.dt * n as f64).fract();
                acc[ri * nf + fi] += Complex64::from_polar(v, ph);
            }
        }
        // prev ← pⁿ⁺¹ (in place: each node needs only its own pⁿ⁻¹).
        let lap = |i: usize, p_ref: &[f64]| -> f64 {
            let f = g.flags[i];
            let mut sum = 0.0;
            let mut kair = 0.0;
            for (d, o) in offs.iter().enumerate() {
                if f & (1 << d) != 0 {
                    sum += p_ref[(i as isize + o) as usize];
                    kair += 1.0;
                }
            }
            l2 * (sum - kair * p_ref[i])
        };
        {
            let p_ref = &p;
            prev.par_chunks_mut(nxy).enumerate().for_each(|(z, slab)| {
                let base = z * nxy;
                for (k, out) in slab.iter_mut().enumerate() {
                    let i = base + k;
                    if g.flags[i] & AIR == 0 || g.bidx[i] != NO_BOUNDARY {
                        continue;
                    }
                    *out = 2.0 * p_ref[i] - *out + lap(i, p_ref);
                }
            });
            let prev_ref = &prev;
            bnext.par_iter_mut().zip(states.par_iter_mut()).zip(g.boundary.par_iter()).for_each(|((out, st), b)| {
                let i = b.cell as usize;
                *out = boundary_update(&b.slots, st, p_ref[i], prev_ref[i], lap(i, p_ref), lam, g.dt);
            });
            for (b, v) in g.boundary.iter().zip(&bnext) {
                prev[b.cell as usize] = *v;
            }
        }
        for &(cell, w) in source {
            prev[cell] += w * signal[n];
        }
        std::mem::swap(&mut p, &mut prev);
    }
    // pⁿ for the final step.
    Some(acc)
}

/// Injection taps of the directional source components at node `c`: the
/// monopole, then (for order 1) dipoles along +x, +y, +z formed by ±1 at the
/// neighbouring nodes. None if a neighbour is not air.
pub fn source_components(g: &FdtdGrid, c: usize, order: u32) -> Option<Vec<Vec<(usize, f64)>>> {
    let mut comps = vec![vec![(c, 1.0)]];
    if order >= 1 {
        let (nx, nxy) = (g.n[0], g.n[0] * g.n[1]);
        for step in [1usize, nx, nxy] {
            let (plus, minus) = (c + step, c.checked_sub(step)?);
            if g.flags.get(plus).is_none_or(|f| f & AIR == 0) || g.flags[minus] & AIR == 0 {
                return None;
            }
            comps.push(vec![(plus, 1.0), (minus, -1.0)]);
        }
    }
    Some(comps)
}

/// First-order fit D(d) ≈ a₀ + a·d of a speaker's complex directivity at `f`,
/// with directions in room coordinates (`to_room` rotates local → room).
/// Returns (a₀, a, relative RMS residual over the sphere).
pub fn fit_first_order(model: &wavey_speaker::SpeakerModel, to_room: glam::DMat3, f: f64) -> (Complex64, [Complex64; 3], f64) {
    const N: usize = 400;
    let golden = PI * (3.0 - 5f64.sqrt());
    let mut m = [[0.0f64; 4]; 4];
    let mut rhs = [Complex64::new(0.0, 0.0); 4];
    let mut samples = Vec::with_capacity(N);
    for i in 0..N {
        let z = 1.0 - 2.0 * (i as f64 + 0.5) / N as f64;
        let r = (1.0 - z * z).sqrt();
        let local = DVec3::new(r * (golden * i as f64).cos(), r * (golden * i as f64).sin(), z);
        let d = model.directivity(f, local);
        let room = to_room * local;
        let b = [1.0, room.x, room.y, room.z];
        for a in 0..4 {
            for c in 0..4 {
                m[a][c] += b[a] * b[c];
            }
            rhs[a] += d * b[a];
        }
        samples.push((b, d));
    }
    // Solve the 4×4 normal equations (Gaussian elimination, real matrix).
    let mut x = rhs;
    for col in 0..4 {
        let piv = (col..4).max_by(|a, b| m[*a][col].abs().total_cmp(&m[*b][col].abs())).unwrap();
        m.swap(col, piv);
        x.swap(col, piv);
        for row in 0..4 {
            if row != col {
                let k = m[row][col] / m[col][col];
                for c in 0..4 {
                    m[row][c] -= k * m[col][c];
                }
                x[row] = x[row] - x[col] * k;
            }
        }
    }
    let a: Vec<Complex64> = (0..4).map(|i| x[i] / m[i][i]).collect();
    let (mut err, mut tot) = (0.0, 0.0);
    for (b, d) in &samples {
        let fit = a[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
        err += (fit - d).norm_sqr();
        tot += d.norm_sqr();
    }
    (a[0], [a[1], a[2], a[3]], (err / tot.max(1e-30)).sqrt())
}

/// Weight of each source component (as returned by `source_components`) to
/// reproduce the fitted directivity: the ±Δx dipole radiates
/// 2j·sin(kΔx·cos θ) ≈ 2j·sin(kΔx)·cos θ times the monopole field.
pub fn component_weights(a0: Complex64, a: [Complex64; 3], k: f64, dx: f64) -> Vec<Complex64> {
    let dip = Complex64::new(0.0, 2.0 * (k * dx).sin());
    vec![a0, a[0] / dip, a[1] / dip, a[2] / dip]
}

/// Converts receiver spectra into transfer functions in the solver's units:
/// pressure for a source with on-axis response `h0` at `r_ref`.
pub fn to_transfer(spectra: &Spectra, signal: &[f64], g: &FdtdGrid, freqs: &[f64], c: f64, r_ref: f64, h0: &[Complex64]) -> Vec<Complex64> {
    let nf = freqs.len();
    let s: Vec<Complex64> = freqs
        .iter()
        .map(|f| {
            signal
                .iter()
                .enumerate()
                .map(|(n, v)| Complex64::from_polar(*v, -2.0 * PI * (f * g.dt * n as f64).fract()))
                .sum()
        })
        .collect();
    let scale = g.dt * g.dt / g.dx.powi(3) * 4.0 * PI * c * c * r_ref;
    spectra
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let fi = i % nf;
            v / s[fi] * scale * h0[fi]
        })
        .collect()
}

/// Power-complementary crossfade of the image-source and wave fields with
/// wave weight `w`: the phase follows the complex blend, the energy is
/// (1 − w)|a|² + w|b|². A plain complex blend loses up to 3 dB mid-band
/// wherever the two methods are not phase-aligned point by point (the
/// cross term averages out), which shows as a dip at the crossover.
pub fn blend(ism: Complex64, wave: Complex64, w: f64) -> Complex64 {
    if w <= 0.0 {
        return ism;
    }
    if w >= 1.0 {
        return wave;
    }
    let target = (1.0 - w) * ism.norm_sqr() + w * wave.norm_sqr();
    let p = ism * (1.0 - w) + wave * w;
    let m2 = p.norm_sqr();
    if m2 < 1e-6 * target {
        // Near-total cancellation: keep the energy, take the larger field's phase.
        let dir = if wave.norm_sqr() * w >= ism.norm_sqr() * (1.0 - w) { wave } else { ism };
        return dir / dir.norm().max(1e-300) * target.sqrt();
    }
    p * (target / m2).sqrt()
}

/// Simulated duration from the low-band reverberation time.
pub fn auto_duration(rt_eyring: &[f64]) -> f64 {
    let t = rt_eyring.iter().take(3).copied().filter(|t| t.is_finite()).fold(0.0, f64::max);
    (1.2 * t).clamp(0.3, 4.0)
}

/// Free-field behaviour of a cabinet with its driver, measured in the wave
/// solver itself: two short runs in an absorbing box, one with the cabinet
/// and one with the bare driver monopole, give the cabinet's effect.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CabinetCalibration {
    /// On-axis gain of cabinet + driver relative to the bare monopole, per frequency.
    pub on_axis: Vec<Complex64>,
    /// Horizontal angles (local frame, 0 = on axis, +90 = left).
    pub angles_deg: Vec<f64>,
    /// Directivity re on-axis, `[angle][freq]`.
    pub directivity: Vec<Vec<Complex64>>,
}

/// Runs a grid: (grid, source taps, signal, receiver nodes, freqs) → spectra.
pub type GridRunner<'a> = dyn Fn(&FdtdGrid, &[(usize, f64)], &[f64], &[usize], &[f64]) -> Option<Spectra> + 'a;

/// Calibrates one cabinet (`verts`/`tris` relative to the enclosure centre,
/// acoustic centre `ac` in the same frame, +x on axis) on a grid of spacing
/// `dx`. None if cancelled.
pub fn calibrate_cabinet(
    verts: &[DVec3],
    tris: &[[u32; 3]],
    ac: DVec3,
    c: f64,
    f_max: f64,
    ppw: f64,
    freqs: &[f64],
    run: &GridRunner,
) -> Option<CabinetCalibration> {
    use std::collections::BTreeMap;
    let reach = verts.iter().map(|v| (*v - ac).length()).fold(0.1, f64::max);
    // The cabinet only matters from about ka = 0.5; size the absorbing box
    // to half a wavelength there (capped by cell count), so its walls do
    // not dominate the measurement.
    let f_cab = 0.5 * c / (2.0 * PI * reach);
    // Same spacing rule as the room grid: the wavelength, and 8 cells across the cabinet.
    let lo = verts.iter().fold(DVec3::splat(f64::INFINITY), |m, v| m.min(*v));
    let hi = verts.iter().fold(DVec3::splat(f64::NEG_INFINITY), |m, v| m.max(*v));
    let dx_est = (c / (f_max * ppw)).min((hi - lo).min_element() / 8.0);
    let max_half = 0.5 * dx_est * 160.0; // ≤ 160³ cells
    // Measure at 3 m or more: closer, the detour around the box still costs
    // level behind it that a far-field (manufacturer) balloon does not show.
    let r_m = (reach + 1.0).max(3.0);
    let half = (0.5 * c / f_cab).min(max_half).max(r_m + 1.5);
    let mut sk = wavey_geom::Sketch::new();
    let side = 2.0 * half;
    sk.add_polygon(&[glam::DVec2::ZERO, glam::DVec2::new(side, 0.0), glam::DVec2::new(side, side), glam::DVec2::new(0.0, side)]);
    let room = wavey_geom::build_room(&sk, side, &BTreeMap::from([(1, glam::DVec2::splat(half))])).ok()?;
    // Matched (ρc) walls: a first-order absorbing boundary.
    let absorber = AdmittanceFit { beta0: 1.0, branches: vec![], band: (1.0, f_max), rms_error: 0.0, max_error: 0.0 };
    let fits = vec![absorber; room.surfaces.len()];
    let p0 = DVec3::splat(half);
    let cab = Solid::from_mesh(verts, tris, glam::DMat3::IDENTITY, p0 - ac)?;
    // Both runs on the same grid spacing, so only the cabinet differs.
    let with = FdtdGrid::build_dx(&room, std::slice::from_ref(&cab), &fits, c, dx_est).ok()?;
    let bare = FdtdGrid::build_dx(&room, &[], &fits, c, dx_est).ok()?;
    let src_pos = with.baffle_point(p0, DVec3::X);
    let taps_with = with.taps_at(src_pos)?;
    let taps_bare = bare.taps_at(src_pos)?;
    let angles_deg: Vec<f64> = (0..12).map(|k| k as f64 * 30.0).collect();
    let points: Vec<DVec3> = angles_deg.iter().map(|a| {
        let r = a.to_radians();
        p0 + DVec3::new(r.cos(), r.sin(), 0.0) * r_m
    }).collect();
    let rx_with: Vec<usize> = points.iter().map(|p| with.nearest_air(*p)).collect::<Option<_>>()?;
    let rx_bare: Vec<usize> = points.iter().map(|p| bare.nearest_air(*p)).collect::<Option<_>>()?;
    let duration = side * 3f64.sqrt() * 2.0 / c + 12.0 * 0.342 / f_max;
    let steps = (duration / with.dt).ceil() as usize;
    let signal = source_signal(f_max, with.dt, steps);
    let s_with = run(&with, &taps_with, &signal, &rx_with, freqs)?;
    let s_bare = run(&bare, &taps_bare, &signal, &rx_bare, freqs)?;
    let nf = freqs.len();
    // Below f_cab the box is acoustically small: its effect tends to 1 as (ka)²;
    // fade the measured ratio towards that limit where the box is too small to measure it.
    let fade = |f: usize| (freqs[f] / f_cab).powi(2).min(1.0);
    let ratio = |r: usize, f: usize| {
        let m = s_with[r * nf + f] / s_bare[r * nf + f];
        let w = fade(f);
        Complex64::from_polar(m.norm().powf(w), m.arg() * w)
    };
    let on_axis: Vec<Complex64> = (0..nf).map(|f| ratio(0, f)).collect();
    let directivity = (0..angles_deg.len()).map(|r| (0..nf).map(|f| ratio(r, f) / on_axis[f]).collect()).collect();
    Some(CabinetCalibration { on_axis, angles_deg, directivity })
}
