//! Simulation results, mixing and storage.

use std::io::{Read, Write};
use std::path::Path;

use num_complex::{Complex32, Complex64};
use serde::{Deserialize, Serialize};

use crate::freq::band_edges;
use crate::raytrace::{Echogram, Metrics, NB, band_interp, metrics};
use crate::receivers::{CutPlane, Microphone, PlaneGrid};

/// Transfer functions are stored per speaker without the user drive
/// (gain/delay/polarity), so speaker settings can be changed after a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub freqs: Vec<f64>,
    pub speaker_ids: Vec<u64>,
    pub mics: Vec<MicResult>,
    pub planes: Vec<PlaneResult>,
    pub stats: RunStats,
    /// Fingerprint of the inputs, set by the caller to detect stale results.
    #[serde(default)]
    pub fingerprint: u64,
    /// Statistical reverberation times per octave band (Sabine, Eyring).
    #[serde(default)]
    pub rt_sabine: [f64; NB],
    #[serde(default)]
    pub rt_eyring: [f64; NB],
    /// Whether late (ray-traced) energy is included.
    #[serde(default)]
    pub has_late: bool,
    /// Wave-solver crossover, if the FDTD stage ran. Below it, results are
    /// wave-based and the late (geometrical) energy is faded out.
    #[serde(default)]
    pub fdtd: Option<crate::fdtd::FdtdSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicResult {
    pub mic: Microphone,
    /// `[speaker][freq]`.
    #[serde(skip)]
    pub transfer: Vec<Vec<Complex32>>,
    /// Late mean-square pressure per speaker and octave band (Pa², unit drive).
    #[serde(skip)]
    pub late: Vec<[f32; NB]>,
    /// Vertical-coherence gain of the late energy, `[speaker][freq]`
    /// (empty: 1).
    #[serde(default)]
    pub late_gain: Vec<Vec<f32>>,
    /// Full energy-time histogram per speaker (unit drive).
    #[serde(default)]
    pub echograms: Vec<Echogram>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaneResult {
    pub plane: CutPlane,
    pub grid: PlaneGrid,
    /// `[speaker][inside point * nf + freq]`.
    #[serde(skip)]
    pub transfer: Vec<Vec<Complex32>>,
    /// `[speaker][inside point]` late energy per band.
    #[serde(skip)]
    pub late: Vec<Vec<[f32; NB]>>,
    /// Height group of each inside point, indexing `late_gain`.
    #[serde(default)]
    pub late_group: Vec<u32>,
    /// Vertical-coherence gain of the late energy, `[speaker][height
    /// group][freq]` (empty: 1). Groups are shared by all planes of a run.
    #[serde(default)]
    pub late_gain: Vec<Vec<Vec<f32>>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunStats {
    pub backend: String,
    pub seconds: f64,
    pub images: usize,
    pub paths: usize,
    #[serde(default)]
    pub diffracted_paths: usize,
    pub receivers: usize,
    #[serde(default)]
    pub notes: Vec<String>,
}

/// How a cut plane value is derived from the spectrum.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaneMode {
    /// Coherent value at the grid frequency nearest to `f`.
    Single { f: f64 },
    /// Mean-square over the 1/`fraction`-octave band centred on `f`.
    Band { f: f64, fraction: f64 },
    /// Mean-square over all frequencies (flat spectrum).
    Broadband,
}

/// Which decay time a map shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecayQuantity {
    Edt,
    T20,
    T30,
}

pub const P_REF: f64 = 20e-6;

/// Coherent pressure with incoherent late energy added to its magnitude.
fn with_late(p: Complex64, late: f64) -> Complex64 {
    if late <= 0.0 {
        return p;
    }
    let m2 = p.norm_sqr();
    if m2 < 1e-30 {
        return Complex64::new(late.sqrt(), 0.0);
    }
    p * ((m2 + late) / m2).sqrt()
}

pub fn spl(p_rms_sq: f64) -> f64 {
    10.0 * (p_rms_sq.max(1e-30) / (P_REF * P_REF)).log10()
}

impl SimulationResult {
    pub fn freq_index(&self, f: f64) -> usize {
        self.freqs
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1.ln() - f.ln()).abs().total_cmp(&(b.1.ln() - f.ln()).abs()))
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    fn freq_indices(&self, mode: PlaneMode) -> Vec<usize> {
        match mode {
            PlaneMode::Single { f } => vec![self.freq_index(f)],
            PlaneMode::Band { f, fraction } => {
                let (lo, hi) = band_edges(f, fraction);
                let v: Vec<usize> = (0..self.freqs.len()).filter(|&i| self.freqs[i] >= lo && self.freqs[i] < hi).collect();
                if v.is_empty() { vec![self.freq_index(f)] } else { v }
            }
            PlaneMode::Broadband => (0..self.freqs.len()).collect(),
        }
    }

    /// Late mean-square pressure at frequency index `fi`, mixed
    /// incoherently. `gain(speaker)` is the vertical-coherence gain row.
    fn late_at<'a>(&self, late: &[[f32; NB]], gain: impl Fn(usize) -> Option<&'a [f32]>, drives: &[Vec<Complex64>], fi: usize) -> f64 {
        if !self.has_late {
            return 0.0;
        }
        let f = self.freqs[fi];
        let w = self.fdtd.as_ref().map_or(0.0, |s| s.weight(f));
        (1.0 - w)
            * late
                .iter()
                .zip(drives)
                .enumerate()
                .map(|(si, (l, d))| d[fi].norm_sqr() * band_interp(l, f) * gain(si).map_or(1.0, |g| g[fi] as f64))
                .sum::<f64>()
    }

    /// Mixed pressure at a mic. `drives[speaker][freq]`. The phase is that of
    /// the coherent (ISM) part; the magnitude includes the late energy.
    pub fn mic_pressure(&self, mic: usize, drives: &[Vec<Complex64>]) -> Vec<Complex64> {
        let m = &self.mics[mic];
        (0..self.freqs.len())
            .map(|fi| {
                let p: Complex64 =
                    m.transfer.iter().zip(drives).map(|(t, d)| d[fi] * Complex64::new(t[fi].re as f64, t[fi].im as f64)).sum();
                let late = if m.late.is_empty() { 0.0 } else { self.late_at(&m.late, |si| m.late_gain.get(si).map(|g| g.as_slice()), drives, fi) };
                with_late(p, late)
            })
            .collect()
    }

    /// The coherent (ISM-only) part at a mic.
    pub fn mic_coherent(&self, mic: usize, drives: &[Vec<Complex64>]) -> Vec<Complex64> {
        let m = &self.mics[mic];
        (0..self.freqs.len())
            .map(|fi| m.transfer.iter().zip(drives).map(|(t, d)| d[fi] * Complex64::new(t[fi].re as f64, t[fi].im as f64)).sum())
            .collect()
    }

    /// Room-acoustic metrics at a mic with the speakers mixed at their current levels.
    pub fn mic_metrics(&self, mic: usize, drives: &[Vec<Complex64>]) -> Option<Metrics> {
        let m = &self.mics[mic];
        if m.echograms.is_empty() {
            return None;
        }
        let bins = m.echograms[0].bands[0].len();
        let mut mixed = Echogram { bands: vec![vec![0.0; bins]; NB] };
        for (echo, d) in m.echograms.iter().zip(drives) {
            for b in 0..NB {
                let g = d[self.freq_index(crate::raytrace::BANDS[b])].norm_sqr() as f32;
                for (x, y) in mixed.bands[b].iter_mut().zip(&echo.bands[b]) {
                    *x += g * y;
                }
            }
        }
        Some(metrics(&mixed))
    }

    /// Centres of the `1/fraction`-octave bands (1 or 3) this run can give a
    /// decay time for: inside the wave solver's range, on a uniform
    /// frequency grid fine enough to hold a band. Whether the grid also
    /// holds the decay shows in the values (NaN where it does not).
    pub fn decay_bands(&self, fraction: f64) -> Vec<f64> {
        let Some(wave) = &self.fdtd else { return vec![] };
        // Nominal centres (the exact ones are powers of two apart).
        const OCTAVES: [f64; 11] = [16.0, 31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
        const THIRDS: [f64; 31] = [
            16.0, 20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0,
            2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0, 10000.0, 12500.0, 16000.0,
        ];
        let centres: &[f64] = if fraction >= 3.0 { &THIRDS } else { &OCTAVES };
        centres
            .iter()
            .copied()
            .filter(|c| *c <= wave.crossover_hz && crate::decay::band_points(&self.freqs, *c, fraction).is_some())
            .collect()
    }

    /// The room's statistical (Eyring) reverberation time in the octave
    /// nearest `centre` (the lowest band below it), NaN if unknown.
    pub fn expected_rt(&self, centre: f64) -> f64 {
        let i = crate::raytrace::BANDS.iter().enumerate().min_by(|a, b| (a.1 / centre).ln().abs().total_cmp(&(b.1 / centre).ln().abs())).map_or(0, |x| x.0);
        let t = self.rt_eyring[i];
        if t.is_finite() && t > 0.0 { t } else { f64::NAN }
    }

    /// Largest frequency step (Hz) that gives `quantity` in the band at
    /// `centre`, from the room's statistical reverberation time; None if unknown.
    pub fn decay_step_needed(&self, centre: f64, quantity: DecayQuantity) -> Option<f64> {
        let rt = self.expected_rt(centre);
        let down = match quantity {
            DecayQuantity::Edt => 10.0,
            DecayQuantity::T20 => 25.0,
            DecayQuantity::T30 => 35.0,
        };
        rt.is_finite().then(|| crate::decay::step_needed(rt, down))
    }

    /// The point's mixed transfer function over the grid points `range`,
    /// coherent only: the late energy has no phase.
    fn mixed_band(&self, drives: &[Vec<Complex64>], range: &std::ops::Range<usize>, transfer: impl Fn(usize, usize) -> Complex32) -> Vec<Complex64> {
        range
            .clone()
            .map(|fi| {
                drives
                    .iter()
                    .enumerate()
                    .map(|(si, d)| {
                        let v = transfer(si, fi);
                        d[fi] * Complex64::new(v.re as f64, v.im as f64)
                    })
                    .sum()
            })
            .collect()
    }

    /// Decay times at a mic in the `1/fraction`-octave band at `centre`,
    /// from the phase-aware mix of all sources (so source filters and
    /// delays change it). NaN where the run cannot say.
    pub fn mic_decay(&self, mic: usize, drives: &[Vec<Complex64>], centre: f64, fraction: f64) -> crate::decay::Decay {
        use crate::decay::{Decay, band_points};
        if !self.decay_bands(fraction).iter().any(|c| (c / centre - 1.0).abs() < 1e-6) {
            return Decay::NONE;
        }
        let Some(range) = band_points(&self.freqs, centre, fraction) else { return Decay::NONE };
        let m = &self.mics[mic];
        let h = self.mixed_band(drives, &range, |si, fi| m.transfer[si][fi]);
        crate::decay::decay_with(&self.freqs[range], &h, centre, fraction, self.expected_rt(centre))
    }

    /// Map of a decay time (s) over a plane, NaN outside the room and where
    /// the run cannot say. Row-major (v outer), like
    /// [`plane_spl`](Self::plane_spl).
    pub fn plane_decay(&self, plane: usize, drives: &[Vec<Complex64>], centre: f64, fraction: f64, quantity: DecayQuantity) -> Vec<f32> {
        use crate::decay::band_points;
        use rayon::prelude::*;
        let pr = &self.planes[plane];
        let mut out = vec![f32::NAN; pr.grid.nu * pr.grid.nv];
        if !self.decay_bands(fraction).iter().any(|c| (c / centre - 1.0).abs() < 1e-6) {
            return out;
        }
        let Some(range) = band_points(&self.freqs, centre, fraction) else { return out };
        let nf = self.freqs.len();
        let expected = self.expected_rt(centre);
        let cells: Vec<usize> = pr.grid.inside.iter().enumerate().filter(|(_, i)| **i).map(|(c, _)| c).collect();
        let values: Vec<f32> = (0..cells.len())
            .into_par_iter()
            .map(|k| {
                let h = self.mixed_band(drives, &range, |si, fi| pr.transfer[si][k * nf + fi]);
                let d = crate::decay::decay_with(&self.freqs[range.clone()], &h, centre, fraction, expected);
                (match quantity {
                    DecayQuantity::Edt => d.edt,
                    DecayQuantity::T20 => d.t20,
                    DecayQuantity::T30 => d.t30,
                }) as f32
            })
            .collect();
        for (c, v) in cells.into_iter().zip(values) {
            out[c] = v;
        }
        out
    }

    /// SPL map of a plane (dB), NaN outside the room. Row-major (v outer).
    pub fn plane_spl(&self, plane: usize, drives: &[Vec<Complex64>], mode: PlaneMode) -> Vec<f32> {
        let pr = &self.planes[plane];
        let nf = self.freqs.len();
        let fis = self.freq_indices(mode);
        let mut out = vec![f32::NAN; pr.grid.nu * pr.grid.nv];
        let mut k = 0;
        for (cell, inside) in pr.grid.inside.iter().enumerate() {
            if !inside {
                continue;
            }
            let mut acc = 0.0;
            let late: Vec<[f32; NB]> = pr.late.iter().map(|l| l[k]).collect();
            let group = pr.late_group.get(k).copied().unwrap_or(0) as usize;
            let gain = |si: usize| pr.late_gain.get(si).and_then(|g| g.get(group)).map(|g| g.as_slice());
            for &fi in &fis {
                if !late.is_empty() {
                    acc += self.late_at(&late, gain, drives, fi);
                }
                let p: Complex64 = pr
                    .transfer
                    .iter()
                    .zip(drives)
                    .map(|(t, d)| {
                        let v = t[k * nf + fi];
                        d[fi] * Complex64::new(v.re as f64, v.im as f64)
                    })
                    .sum();
                acc += p.norm_sqr();
            }
            out[cell] = spl(acc / fis.len() as f64) as f32;
            k += 1;
        }
        out
    }

    /// Coherent pressure at one inside point `k` of a plane and frequency
    /// `fi`, plus its late mean-square energy.
    fn plane_point(&self, pr: &PlaneResult, k: usize, drives: &[Vec<Complex64>], fi: usize) -> (Complex64, f64) {
        let nf = self.freqs.len();
        let p: Complex64 = pr
            .transfer
            .iter()
            .zip(drives)
            .map(|(t, d)| {
                let v = t[k * nf + fi];
                d[fi] * Complex64::new(v.re as f64, v.im as f64)
            })
            .sum();
        if pr.late.is_empty() {
            return (p, 0.0);
        }
        let late: Vec<[f32; NB]> = pr.late.iter().map(|l| l[k]).collect();
        let group = pr.late_group.get(k).copied().unwrap_or(0) as usize;
        let gain = |si: usize| pr.late_gain.get(si).and_then(|g| g.get(group)).map(|g| g.as_slice());
        (p, self.late_at(&late, gain, drives, fi))
    }

    /// SPL of a plane at every grid frequency (dB), `[freq][v][u]`, NaN
    /// outside the room. Same values as [`plane_spl`](Self::plane_spl) in
    /// single-frequency mode, for all frequencies in one pass.
    pub fn plane_spl_stack(&self, plane: usize, drives: &[Vec<Complex64>]) -> Vec<f32> {
        use rayon::prelude::*;
        let pr = &self.planes[plane];
        let nf = self.freqs.len();
        let cells = pr.grid.nu * pr.grid.nv;
        let inside: Vec<usize> = (0..cells).filter(|&c| pr.grid.inside[c]).collect();
        let columns: Vec<Vec<f32>> = inside
            .par_iter()
            .enumerate()
            .map(|(k, _)| {
                (0..nf)
                    .map(|fi| {
                        let (p, late) = self.plane_point(pr, k, drives, fi);
                        spl(p.norm_sqr() + late) as f32
                    })
                    .collect()
            })
            .collect();
        let mut out = vec![f32::NAN; nf * cells];
        for (col, &cell) in columns.iter().zip(&inside) {
            for (fi, v) in col.iter().enumerate() {
                out[fi * cells + cell] = *v;
            }
        }
        out
    }

    /// Pressure at an arbitrary point of a plane, interpolated bilinearly
    /// from the surrounding inside grid points (coherent part complex, late
    /// energy separately, then combined as at a mic). None if no inside
    /// grid point is adjacent.
    pub fn plane_probe(&self, plane: usize, u: f64, v: f64, drives: &[Vec<Complex64>]) -> Option<Vec<Complex64>> {
        let pr = &self.planes[plane];
        let g = &pr.grid;
        let fu = ((u - g.u0) / g.du).clamp(0.0, (g.nu - 1) as f64);
        let fv = ((v - g.v0) / g.dv).clamp(0.0, (g.nv - 1) as f64);
        let (iu, iv) = (fu.floor() as usize, fv.floor() as usize);
        let (tu, tv) = (fu - iu as f64, fv - iv as f64);
        // Inside index of each cell (position among inside cells).
        let index_of = |cell: usize| -> Option<usize> {
            g.inside[cell].then(|| g.inside[..cell].iter().filter(|b| **b).count())
        };
        let mut corners = Vec::new();
        for (du, dv, w) in [(0, 0, (1.0 - tu) * (1.0 - tv)), (1, 0, tu * (1.0 - tv)), (0, 1, (1.0 - tu) * tv), (1, 1, tu * tv)] {
            let (cu, cv) = ((iu + du).min(g.nu - 1), (iv + dv).min(g.nv - 1));
            if let Some(k) = index_of(cv * g.nu + cu)
                && w > 0.0
            {
                corners.push((k, w));
            }
        }
        if corners.is_empty() {
            return None;
        }
        let wsum: f64 = corners.iter().map(|c| c.1).sum();
        Some(
            (0..self.freqs.len())
                .map(|fi| {
                    let (mut p, mut late) = (Complex64::new(0.0, 0.0), 0.0);
                    for &(k, w) in &corners {
                        let (pk, lk) = self.plane_point(pr, k, drives, fi);
                        p += pk * (w / wsum);
                        late += lk * (w / wsum);
                    }
                    with_late(p, late)
                })
                .collect(),
        )
    }

    // ---- storage: results.json (metadata) + results.bin (f32 data) ----

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("results.json"), serde_json::to_vec(self)?)?;
        let mut f = std::io::BufWriter::new(std::fs::File::create(dir.join("results.bin"))?);
        let all = self.mics.iter().flat_map(|m| &m.transfer).chain(self.planes.iter().flat_map(|p| &p.transfer));
        for t in all {
            f.write_all(bytemuck::cast_slice(t))?;
        }
        for m in &self.mics {
            f.write_all(bytemuck::cast_slice(&m.late))?;
        }
        for p in &self.planes {
            for l in &p.late {
                f.write_all(bytemuck::cast_slice(l))?;
            }
        }
        f.flush()
    }

    pub fn load(dir: &Path) -> std::io::Result<Self> {
        let mut r: SimulationResult = serde_json::from_slice(&std::fs::read(dir.join("results.json"))?)?;
        let mut f = std::io::BufReader::new(std::fs::File::open(dir.join("results.bin"))?);
        let (ns, nf) = (r.speaker_ids.len(), r.freqs.len());
        let mut read = |n: usize| -> std::io::Result<Vec<Complex32>> {
            let mut v = vec![Complex32::new(0.0, 0.0); n];
            f.read_exact(bytemuck::cast_slice_mut(&mut v))?;
            Ok(v)
        };
        for m in &mut r.mics {
            m.transfer = (0..ns).map(|_| read(nf)).collect::<Result<_, _>>()?;
        }
        for p in &mut r.planes {
            let n = p.grid.inside_count() * nf;
            p.transfer = (0..ns).map(|_| read(n)).collect::<Result<_, _>>()?;
        }
        if r.has_late {
            let mut read_late = |n: usize| -> std::io::Result<Vec<[f32; NB]>> {
                let mut v = vec![[0f32; NB]; n];
                f.read_exact(bytemuck::cast_slice_mut(&mut v))?;
                Ok(v)
            };
            for m in &mut r.mics {
                m.late = read_late(ns)?;
            }
            for p in &mut r.planes {
                let n = p.grid.inside_count();
                p.late = (0..ns).map(|_| read_late(n)).collect::<Result<_, _>>()?;
            }
        }
        Ok(r)
    }
}
