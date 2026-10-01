//! Simulation results, mixing and storage.

use std::io::{Read, Write};
use std::path::Path;

use num_complex::{Complex32, Complex64};
use serde::{Deserialize, Serialize};

use crate::freq::band_edges;
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicResult {
    pub mic: Microphone,
    /// `[speaker][freq]`.
    #[serde(skip)]
    pub transfer: Vec<Vec<Complex32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaneResult {
    pub plane: CutPlane,
    pub grid: PlaneGrid,
    /// `[speaker][inside point * nf + freq]`.
    #[serde(skip)]
    pub transfer: Vec<Vec<Complex32>>,
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

    /// Mixed complex pressure at a mic. `drives[speaker][freq]`.
    pub fn mic_pressure(&self, mic: usize, drives: &[Vec<Complex64>]) -> Vec<Complex64> {
        let m = &self.mics[mic];
        (0..self.freqs.len())
            .map(|fi| {
                m.transfer.iter().zip(drives).map(|(t, d)| d[fi] * Complex64::new(t[fi].re as f64, t[fi].im as f64)).sum()
            })
            .collect()
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
            for &fi in &fis {
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
        Ok(r)
    }
}
