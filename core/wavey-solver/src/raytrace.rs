//! Stochastic ray tracing for the late, diffuse part of the field.
//!
//! Rays carry per-octave-band energy. At each wall hit, energy is reduced by
//! (1 − α), and the ray continues specularly with probability (1 − s) or in
//! a Lambert direction otherwise. Energy is counted as *late* once a ray has
//! scattered or exceeded the image-source order: its earlier, purely
//! specular history is exactly what the ISM already computes coherently, and
//! ISM amplitudes carry the matching √(1 − s) factor per reflection.
//!
//! Late energy is deposited along ray segments into a voxel grid (a volume
//! detector), giving a smooth mean-square pressure field that every receiver
//! samples. Separately, sphere detectors at microphones record full
//! energy-time histograms for room-acoustic metrics.
//!
//! Units: a ray's energy `e` is in Pa²·m² such that a detector of volume V
//! crossed over a chord ℓ receives a mean-square pressure of e·ℓ / V.

use std::f64::consts::PI;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use glam::{DMat3, DVec3};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use wavey_geom::Room;
use wavey_materials::{Material, Medium};
use wavey_speaker::SpeakerModel;

/// Octave band centers used by the ray tracer (Hz).
pub const BANDS: [f64; 8] = [63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0];
pub const NB: usize = BANDS.len();

/// Echogram bin width (s).
pub const BIN: f64 = 0.001;

#[derive(Debug, Clone, Copy)]
pub struct RaySettings {
    pub rays: u32,
    /// Image-source order: specular paths up to this order are not "late".
    pub ism_order: u32,
    /// Stop rays after this time (s).
    pub max_time: f64,
    pub seed: u64,
}

/// Small, fast PRNG (PCG-XSH-RR 64/32).
#[derive(Clone)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        let mut r = Rng(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407));
        r.next_u32();
        r
    }

    fn next_u32(&mut self) -> u32 {
        let old = self.0;
        self.0 = old.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    fn uniform(&mut self) -> f64 {
        (self.next_u32() as f64 + 0.5) / 4294967296.0
    }
}

/// Evenly spread unit vectors (Fibonacci sphere).
fn fibonacci(i: u32, n: u32) -> DVec3 {
    let golden = PI * (3.0 - 5f64.sqrt());
    let z = 1.0 - 2.0 * (i as f64 + 0.5) / n as f64;
    let r = (1.0 - z * z).sqrt();
    let phi = golden * i as f64;
    DVec3::new(r * phi.cos(), r * phi.sin(), z)
}

/// Cosine-weighted direction in the hemisphere around `n`.
fn lambert(n: DVec3, rng: &mut Rng) -> DVec3 {
    let (u1, u2) = (rng.uniform(), rng.uniform());
    let r = u1.sqrt();
    let phi = 2.0 * PI * u2;
    let (t, b) = n.any_orthonormal_pair();
    (t * (r * phi.cos()) + b * (r * phi.sin()) + n * (1.0 - u1).max(0.0).sqrt()).normalize()
}

/// Per-band surface data.
#[derive(Debug, Clone)]
pub struct BandSurface {
    pub absorption: [f64; NB],
    pub scattering: [f64; NB],
}

impl BandSurface {
    pub fn new(m: &Material) -> Self {
        BandSurface {
            absorption: BANDS.map(|f| m.absorption_at(f)),
            scattering: BANDS.map(|f| m.scattering_at(f)),
        }
    }
}

/// Uniform voxel grid over the room bounds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoxelGrid {
    pub origin: DVec3,
    pub h: f64,
    pub n: [usize; 3],
}

impl VoxelGrid {
    /// About `target` voxels, at least 0.2 m.
    pub fn for_room(room: &Room, target: usize) -> Self {
        let (lo, hi) = room.bounds();
        let size = hi - lo;
        let h = (size.x * size.y * size.z / target as f64).cbrt().max(0.2);
        let n = [
            (size.x / h).ceil().max(1.0) as usize,
            (size.y / h).ceil().max(1.0) as usize,
            (size.z / h).ceil().max(1.0) as usize,
        ];
        VoxelGrid { origin: lo, h, n }
    }

    pub fn len(&self) -> usize {
        self.n[0] * self.n[1] * self.n[2]
    }

    pub fn index(&self, i: [usize; 3]) -> usize {
        (i[2] * self.n[1] + i[1]) * self.n[0] + i[0]
    }

    pub fn cell(&self, p: DVec3) -> [usize; 3] {
        let q = (p - self.origin) / self.h;
        [
            (q.x.floor().max(0.0) as usize).min(self.n[0] - 1),
            (q.y.floor().max(0.0) as usize).min(self.n[1] - 1),
            (q.z.floor().max(0.0) as usize).min(self.n[2] - 1),
        ]
    }

    pub fn center(&self, i: [usize; 3]) -> DVec3 {
        self.origin + (DVec3::new(i[0] as f64, i[1] as f64, i[2] as f64) + 0.5) * self.h
    }

    /// Calls `f(voxel, length)` for each voxel crossed by segment a→b (3D DDA).
    pub fn traverse(&self, a: DVec3, b: DVec3, mut f: impl FnMut(usize, f64, f64)) {
        let d = b - a;
        let len = d.length();
        if len <= 0.0 {
            return;
        }
        let dir = d / len;
        let mut cell = self.cell(a).map(|c| c as i64);
        let step = [dir.x, dir.y, dir.z].map(|v| if v > 0.0 { 1i64 } else { -1 });
        let mut t_max = [0.0f64; 3];
        let mut t_delta = [f64::INFINITY; 3];
        for k in 0..3 {
            let dk = dir[k];
            if dk.abs() > 1e-15 {
                let boundary = self.origin[k] + (cell[k] + if dk > 0.0 { 1 } else { 0 }) as f64 * self.h;
                t_max[k] = (boundary - a[k]) / dk;
                t_delta[k] = self.h / dk.abs();
            } else {
                t_max[k] = f64::INFINITY;
            }
        }
        let mut t = 0.0;
        loop {
            let k = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] { 0 } else if t_max[1] <= t_max[2] { 1 } else { 2 };
            let t_next = t_max[k].min(len);
            if (0..3).all(|j| cell[j] >= 0 && (cell[j] as usize) < self.n[j]) {
                let idx = self.index(cell.map(|c| c as usize));
                f(idx, t_next - t, (t + t_next) * 0.5);
            }
            if t_max[k] >= len {
                break;
            }
            t = t_max[k];
            cell[k] += step[k];
            t_max[k] += t_delta[k];
        }
    }
}

/// Late mean-square pressure per voxel and band (Pa²).
#[derive(Debug, Clone)]
pub struct LateField {
    pub grid: VoxelGrid,
    pub energy: Vec<[f32; NB]>,
}

impl LateField {
    pub fn at(&self, p: DVec3) -> [f32; NB] {
        self.energy[self.grid.index(self.grid.cell(p))]
    }
}

/// Energy-time histogram at a microphone: `[band][bin]`, the mean-square
/// pressure (Pa²) contributed by arrivals within each bin.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Echogram {
    pub bands: Vec<Vec<f32>>,
}

pub struct TraceOutput {
    pub late: LateField,
    pub echograms: Vec<Echogram>,
}

/// Mean |H0·D|² over an octave band for a local direction (five samples).
fn band_power(model: &SpeakerModel, dir_local: DVec3, band: f64) -> f64 {
    (0..5)
        .map(|i| {
            let f = band * 2f64.powf((i as f64 - 2.0) / 5.0);
            (model.on_axis(f) * model.directivity(f, dir_local)).norm_sqr()
        })
        .sum::<f64>()
        / 5.0
}

#[allow(clippy::too_many_arguments)]
pub fn trace(
    room: &Room,
    surfaces: &[BandSurface],
    medium: &Medium,
    source: DVec3,
    to_local: DMat3,
    model: &SpeakerModel,
    mics: &[DVec3],
    settings: RaySettings,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f64) + Sync),
) -> Option<TraceOutput> {
    let grid = VoxelGrid::for_room(room, 200_000);
    let c = medium.speed_of_sound();
    let air: [f64; NB] = BANDS.map(|f| medium.air_intensity_coefficient(f));
    let n = settings.rays.max(1);
    let r_ref = model.reference.distance_m;
    let solid = 4.0 * PI / n as f64;
    let max_dist = c * settings.max_time;
    let bins = (settings.max_time / BIN).ceil() as usize + 1;
    let det_r = ((room.volume() / (PI * n as f64)).sqrt()).max(0.3);
    let det_v = 4.0 / 3.0 * PI * det_r.powi(3);

    struct Acc {
        vox: Vec<[f64; NB]>,
        echo: Vec<Vec<[f64; NB]>>,
    }
    let new_acc = || Acc { vox: vec![[0.0; NB]; grid.len()], echo: vec![vec![[0.0; NB]; bins]; mics.len()] };
    let done = AtomicUsize::new(0);
    let chunk = 256u32;
    let chunks: Vec<u32> = (0..n.div_ceil(chunk)).collect();

    let acc = chunks
        .par_iter()
        .map(|&ci| {
            let mut acc = new_acc();
            if cancel.load(Ordering::Relaxed) {
                return acc;
            }
            let mut rng = Rng::new(settings.seed ^ (ci as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            for i in ci * chunk..((ci + 1) * chunk).min(n) {
                let dir0 = fibonacci(i, n);
                let local = to_local * dir0;
                let mut e: [f64; NB] = BANDS.map(|b| band_power(model, local, b) * r_ref * r_ref * solid);
                let e_floor = e.iter().copied().fold(0.0, f64::max) * 1e-7;
                let (mut pos, mut dir) = (source, dir0);
                let (mut order, mut diffuse, mut travelled) = (0u32, false, 0.0);
                while travelled < max_dist {
                    let Some(hit) = room.bvh.intersect(pos, dir, 1e-9, f64::INFINITY) else { break };
                    let seg = hit.t;
                    let end = pos + dir * seg;
                    if diffuse || order > settings.ism_order {
                        grid.traverse(pos, end, |v, len, mid| {
                            let cell = &mut acc.vox[v];
                            for b in 0..NB {
                                cell[b] += e[b] * (-air[b] * mid).exp() * len;
                            }
                        });
                    }
                    for (mi, m) in mics.iter().enumerate() {
                        let t = (*m - pos).dot(dir).clamp(0.0, seg);
                        let d2 = (pos + dir * t - *m).length_squared();
                        if d2 < det_r * det_r {
                            let chord = 2.0 * (det_r * det_r - d2).sqrt();
                            let bin = ((travelled + t) / c / BIN) as usize;
                            if bin < bins {
                                for b in 0..NB {
                                    acc.echo[mi][bin][b] += e[b] * (-air[b] * t).exp() * chord / det_v;
                                }
                            }
                        }
                    }
                    for b in 0..NB {
                        e[b] *= (-air[b] * seg).exp();
                    }
                    travelled += seg;
                    let s = &surfaces[room.tri_surface[hit.triangle as usize] as usize];
                    let normal = room.surfaces[room.tri_surface[hit.triangle as usize] as usize].normal;
                    // Two-sided free-standing walls: face the ray.
                    let n_in = if normal.dot(dir) < 0.0 { normal } else { -normal };
                    for b in 0..NB {
                        e[b] *= 1.0 - s.absorption[b];
                    }
                    if e.iter().all(|v| *v < e_floor) {
                        break;
                    }
                    // Choose specular or diffuse with the band-mean scattering,
                    // reweighting bands so each band's split stays exact.
                    let p_diff = s.scattering.iter().sum::<f64>() / NB as f64;
                    let go_diffuse = p_diff >= 1.0 || (p_diff > 0.0 && rng.uniform() < p_diff);
                    for b in 0..NB {
                        let w = if go_diffuse { s.scattering[b] / p_diff.max(1e-12) } else { (1.0 - s.scattering[b]) / (1.0 - p_diff).max(1e-12) };
                        e[b] *= w;
                    }
                    dir = if go_diffuse { lambert(n_in, &mut rng) } else { dir - 2.0 * dir.dot(n_in) * n_in };
                    diffuse |= go_diffuse;
                    order += 1;
                    pos = end + n_in * 1e-7;
                }
            }
            let d = done.fetch_add(1, Ordering::Relaxed) + 1;
            progress(d as f64 / chunks.len() as f64);
            acc
        })
        .reduce(new_acc, |mut a, b| {
            for (x, y) in a.vox.iter_mut().zip(&b.vox) {
                for k in 0..NB {
                    x[k] += y[k];
                }
            }
            for (ea, eb) in a.echo.iter_mut().zip(&b.echo) {
                for (x, y) in ea.iter_mut().zip(eb) {
                    for k in 0..NB {
                        x[k] += y[k];
                    }
                }
            }
            a
        });
    if cancel.load(Ordering::Relaxed) {
        return None;
    }

    // Normalize by the inside volume of each voxel, then smooth over inside neighbours.
    let frac: Vec<f64> = (0..grid.len())
        .into_par_iter()
        .map(|v| {
            let (iz, rest) = (v / (grid.n[0] * grid.n[1]), v % (grid.n[0] * grid.n[1]));
            let (iy, ix) = (rest / grid.n[0], rest % grid.n[0]);
            let base = grid.origin + DVec3::new(ix as f64, iy as f64, iz as f64) * grid.h;
            let mut inside = 0;
            for a in 0..3 {
                for b in 0..3 {
                    for cc in 0..3 {
                        let p = base + DVec3::new(a as f64 + 0.5, b as f64 + 0.5, cc as f64 + 0.5) * (grid.h / 3.0);
                        inside += room.contains(p) as u32;
                    }
                }
            }
            inside as f64 / 27.0
        })
        .collect();
    let vol = grid.h.powi(3);
    let raw: Vec<[f64; NB]> = acc
        .vox
        .iter()
        .zip(&frac)
        .map(|(e, f)| if *f > 0.0 { e.map(|x| x / (vol * f)) } else { [0.0; NB] })
        .collect();
    let [nx, ny, nz] = grid.n;
    let energy: Vec<[f32; NB]> = (0..grid.len())
        .into_par_iter()
        .map(|v| {
            let (iz, rest) = (v / (nx * ny), v % (nx * ny));
            let (iy, ix) = (rest / nx, rest % nx);
            let mut sum = [0.0; NB];
            let mut w = 0.0;
            for dz in -1i64..=1 {
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        let (x, y, z) = (ix as i64 + dx, iy as i64 + dy, iz as i64 + dz);
                        if x < 0 || y < 0 || z < 0 || x >= nx as i64 || y >= ny as i64 || z >= nz as i64 {
                            continue;
                        }
                        let u = grid.index([x as usize, y as usize, z as usize]);
                        if frac[u] > 0.0 {
                            for k in 0..NB {
                                sum[k] += raw[u][k] * frac[u];
                            }
                            w += frac[u];
                        }
                    }
                }
            }
            if w > 0.0 { sum.map(|s| (s / w) as f32) } else { [0.0; NB] }
        })
        .collect();

    let echograms = acc
        .echo
        .into_iter()
        .map(|bins| Echogram { bands: (0..NB).map(|b| bins.iter().map(|x| x[b] as f32).collect()).collect() })
        .collect();
    Some(TraceOutput { late: LateField { grid, energy }, echograms })
}

/// Room-acoustic metrics from an echogram (per band).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub edt_s: [f64; NB],
    pub t20_s: [f64; NB],
    pub t30_s: [f64; NB],
    pub c80_db: [f64; NB],
    pub d50: [f64; NB],
}

/// Decay time from the Schroeder curve between `from_db` and `to_db`,
/// extrapolated to 60 dB. NaN if the range is not reached.
fn decay_time(edc_db: &[f64], from_db: f64, to_db: f64) -> f64 {
    let pts: Vec<(f64, f64)> = edc_db
        .iter()
        .enumerate()
        .filter(|(_, v)| **v <= from_db && **v >= to_db)
        .map(|(i, v)| (i as f64 * BIN, *v))
        .collect();
    if pts.len() < 3 || edc_db.iter().all(|v| *v > to_db) {
        return f64::NAN;
    }
    let n = pts.len() as f64;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (mx, my) = (sx / n, sy / n);
    let (num, den) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + (p.0 - mx) * (p.1 - my), a.1 + (p.0 - mx).powi(2)));
    let slope = num / den;
    if slope < 0.0 { -60.0 / slope } else { f64::NAN }
}

pub fn metrics(echo: &Echogram) -> Metrics {
    let mut m = Metrics::default();
    for b in 0..NB {
        let e: Vec<f64> = echo.bands[b].iter().map(|v| *v as f64).collect();
        let total: f64 = e.iter().sum();
        if total <= 0.0 {
            m.edt_s[b] = f64::NAN;
            m.t20_s[b] = f64::NAN;
            m.t30_s[b] = f64::NAN;
            m.c80_db[b] = f64::NAN;
            m.d50[b] = f64::NAN;
            continue;
        }
        let mut edc = vec![0.0; e.len()];
        let mut acc = 0.0;
        for i in (0..e.len()).rev() {
            acc += e[i];
            edc[i] = 10.0 * (acc / total).max(1e-30).log10();
        }
        m.edt_s[b] = decay_time(&edc, 0.0, -10.0);
        m.t20_s[b] = decay_time(&edc, -5.0, -25.0);
        m.t30_s[b] = decay_time(&edc, -5.0, -35.0);
        let first = e.iter().position(|v| *v > 0.0).unwrap_or(0);
        let sum = |a: usize, z: usize| e[a.min(e.len())..z.min(e.len())].iter().sum::<f64>();
        let i50 = first + (0.050 / BIN) as usize;
        let i80 = first + (0.080 / BIN) as usize;
        m.c80_db[b] = 10.0 * (sum(0, i80) / sum(i80, e.len()).max(1e-30)).log10();
        m.d50[b] = sum(0, i50) / total;
    }
    m
}

/// Sabine and Eyring reverberation times per band for reference.
pub fn statistical_rt(room: &Room, surfaces: &[BandSurface], medium: &Medium) -> ([f64; NB], [f64; NB]) {
    let v = room.volume();
    let s_total = room.total_surface_area();
    let mut sabine = [0.0; NB];
    let mut eyring = [0.0; NB];
    for b in 0..NB {
        let a: f64 = room.surfaces.iter().zip(surfaces).map(|(s, m)| s.area * m.absorption[b]).sum();
        let m4v = 4.0 * medium.air_intensity_coefficient(BANDS[b]) * v;
        let mean = (a / s_total).min(0.999_999);
        sabine[b] = 0.161 * v / (a + m4v);
        eyring[b] = 0.161 * v / (-s_total * (1.0 - mean).ln() + m4v);
    }
    (sabine, eyring)
}

/// Late energy at frequency `f`, interpolated in log-energy between bands.
pub fn band_interp(e: &[f32; NB], f: f64) -> f64 {
    let x = (f / BANDS[0]).log2().clamp(0.0, (NB - 1) as f64);
    let i = (x.floor() as usize).min(NB - 2);
    let t = x - i as f64;
    let (a, b) = (e[i].max(1e-30) as f64, e[i + 1].max(1e-30) as f64);
    if e[i] <= 0.0 && e[i + 1] <= 0.0 {
        return 0.0;
    }
    (a.ln() * (1.0 - t) + b.ln() * t).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dda_lengths_sum_to_segment() {
        let g = VoxelGrid { origin: DVec3::ZERO, h: 0.5, n: [10, 10, 10] };
        let (a, b) = (DVec3::new(0.1, 0.2, 0.3), DVec3::new(4.7, 3.1, 2.2));
        let mut total = 0.0;
        let mut count = 0;
        g.traverse(a, b, |_, len, _| {
            total += len;
            count += 1;
        });
        assert!((total - (b - a).length()).abs() < 1e-9);
        assert!(count >= 9);
    }

    #[test]
    fn decay_time_of_exponential() {
        // 2 s reverberation: energy ∝ 10^(−6t/T).
        let bins = 3000;
        let e: Vec<f32> = (0..bins).map(|i| 10f64.powf(-6.0 * i as f64 * BIN / 2.0) as f32).collect();
        let m = metrics(&Echogram { bands: vec![e; NB] });
        assert!((m.t30_s[3] - 2.0).abs() < 0.02, "{}", m.t30_s[3]);
        assert!((m.edt_s[3] - 2.0).abs() < 0.02);
    }
}
