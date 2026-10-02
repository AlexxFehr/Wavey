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
}

fn default_ppw() -> f64 {
    15.0
}

impl Default for FdtdSettings {
    fn default() -> Self {
        Self { crossover_hz: 250.0, duration_s: 0.0, points_per_wavelength: default_ppw() }
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

    /// Nearest air node to `p`.
    pub fn nearest_air(&self, p: DVec3) -> Option<usize> {
        let q = (p - self.origin) / self.dx;
        let c = [q.x.round() as i64, q.y.round() as i64, q.z.round() as i64];
        for r in 0..4i64 {
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
        let dx = c / (f_max * ppw);
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
        let air: Vec<bool> = (0..cells)
            .into_par_iter()
            .map(|i| {
                let (z, r) = (i / (n[0] * n[1]), i % (n[0] * n[1]));
                let (y, x) = (r / n[0], r % n[0]);
                x > 0 && y > 0 && z > 0 && x < n[0] - 1 && y < n[1] - 1 && z < n[2] - 1 && room.contains(g.center(x, y, z))
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
    source: usize,
    signal: &[f64],
    receivers: &[usize],
    freqs: &[f64],
    cancel: &AtomicBool,
    progress: &(dyn Fn(f64) + Sync),
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
        prev[source] += signal[n];
        std::mem::swap(&mut p, &mut prev);
    }
    // pⁿ for the final step.
    Some(acc)
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

/// Simulated duration from the low-band reverberation time.
pub fn auto_duration(rt_eyring: &[f64]) -> f64 {
    let t = rt_eyring.iter().take(3).copied().filter(|t| t.is_finite()).fold(0.0, f64::max);
    (1.2 * t).clamp(0.3, 4.0)
}
