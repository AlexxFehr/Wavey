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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunStats {
    pub backend: String,
    pub seconds: f64,
    pub images: usize,
    pub paths: usize,
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

    /// Late mean-square pressure at frequency index `fi`, mixed incoherently.
    fn late_at(&self, late: &[[f32; NB]], drives: &[Vec<Complex64>], fi: usize) -> f64 {
        if !self.has_late {
            return 0.0;
        }
        let f = self.freqs[fi];
        let w = self.fdtd.as_ref().map_or(0.0, |s| s.weight(f));
        (1.0 - w) * late.iter().zip(drives).map(|(l, d)| d[fi].norm_sqr() * band_interp(l, f)).sum::<f64>()
    }

    /// Mixed pressure at a mic. `drives[speaker][freq]`. The phase is that of
    /// the coherent (ISM) part; the magnitude includes the late energy.
    pub fn mic_pressure(&self, mic: usize, drives: &[Vec<Complex64>]) -> Vec<Complex64> {
        let m = &self.mics[mic];
        (0..self.freqs.len())
            .map(|fi| {
                let p: Complex64 =
                    m.transfer.iter().zip(drives).map(|(t, d)| d[fi] * Complex64::new(t[fi].re as f64, t[fi].im as f64)).sum();
                let late = if m.late.is_empty() { 0.0 } else { self.late_at(&m.late, drives, fi) };
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
            for &fi in &fis {
                if !late.is_empty() {
                    acc += self.late_at(&late, drives, fi);
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
