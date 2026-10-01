//! Simulation inputs and their preparation into per-frequency tables.

use std::sync::Arc;

use num_complex::Complex64;
use serde::{Deserialize, Serialize};
use wavey_geom::Room;
use wavey_materials::{Material, Medium, fit_real_impedance};
use wavey_speaker::{SpeakerInstance, SpeakerModel};

use crate::freq::{Resolution, frequency_grid};
use crate::receivers::{CutPlane, Microphone};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BackendChoice {
    /// GPU if available, else CPU.
    #[default]
    Auto,
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimSettings {
    pub f_min: f64,
    pub f_max: f64,
    pub resolution: Resolution,
    /// Maximum reflection order of the image-source method.
    pub max_order: u32,
    #[serde(default)]
    pub backend: BackendChoice,
    /// Add the late diffuse field from ray tracing.
    #[serde(default = "yes")]
    pub late_reverb: bool,
    /// Rays per source for the late field.
    #[serde(default = "default_rays")]
    pub rays_per_source: u32,
    /// Low-frequency wave solver settings (None: disabled).
    #[serde(default)]
    pub fdtd: Option<crate::fdtd::FdtdSettings>,
}

fn yes() -> bool {
    true
}

fn default_rays() -> u32 {
    20_000
}

impl Default for SimSettings {
    fn default() -> Self {
        Self {
            f_min: 20.0,
            f_max: 20000.0,
            resolution: Resolution::FractionalOctave { n: 12 },
            max_order: 3,
            backend: BackendChoice::Auto,
            late_reverb: true,
            rays_per_source: default_rays(),
            fdtd: None,
        }
    }
}

impl SimSettings {
    pub fn frequencies(&self) -> Vec<f64> {
        frequency_grid(self.f_min, self.f_max, self.resolution)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !(self.f_min > 0.0 && self.f_max >= self.f_min) {
            return Err("frequency range must satisfy 0 < f_min ≤ f_max".into());
        }
        if self.frequencies().is_empty() {
            return Err("the frequency range and resolution give no frequencies".into());
        }
        if self.max_order > 12 {
            return Err("reflection order above 12 is not supported".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SourceInput {
    pub instance: SpeakerInstance,
    pub model: Arc<SpeakerModel>,
}

/// Everything the solver needs, independent of the project format.
#[derive(Debug, Clone)]
pub struct SimulationInput {
    pub room: Room,
    /// Material of each room surface (same order as `room.surfaces`).
    pub surface_materials: Vec<Material>,
    pub sources: Vec<SourceInput>,
    pub medium: Medium,
    pub microphones: Vec<Microphone>,
    pub planes: Vec<CutPlane>,
    pub settings: SimSettings,
}

/// Reflection data of one surface on the frequency grid.
#[derive(Debug, Clone)]
pub struct SurfaceAcoustics {
    /// Angle-independent reflection coefficient (used when `zeta` is None).
    pub r_fixed: Vec<Complex64>,
    /// Normalized impedance for angle-dependent reflection.
    pub zeta: Option<Vec<Complex64>>,
    /// √(1 − s): specular share of reflected pressure.
    pub specular: Vec<f64>,
    pub absorption: Vec<f64>,
    pub scattering: Vec<f64>,
}

impl SurfaceAcoustics {
    pub fn new(m: &Material, freqs: &[f64]) -> Self {
        let zeta = if let Some((re, im)) = &m.impedance {
            Some(freqs.iter().map(|f| Complex64::new(re.eval(*f), im.eval(*f))).collect())
        } else if m.angle_dependent {
            Some(freqs.iter().map(|f| Complex64::new(fit_real_impedance(m.absorption.eval(*f).clamp(0.0, 1.0)), 0.0)).collect())
        } else {
            None
        };
        SurfaceAcoustics {
            r_fixed: freqs.iter().map(|f| m.reflection(*f, 1.0)).collect(),
            zeta,
            specular: freqs.iter().map(|f| (1.0 - m.scattering_at(*f)).sqrt()).collect(),
            absorption: freqs.iter().map(|f| m.absorption_at(*f)).collect(),
            scattering: freqs.iter().map(|f| m.scattering_at(*f)).collect(),
        }
    }

    /// Specular pressure reflection factor (including the scattering split).
    #[inline]
    pub fn reflection(&self, fi: usize, cos: f64) -> Complex64 {
        let r = match &self.zeta {
            Some(z) => {
                let zc = z[fi] * cos.max(1e-6);
                (zc - 1.0) / (zc + 1.0)
            }
            None => self.r_fixed[fi],
        };
        r * self.specular[fi]
    }
}

/// Frequency tables shared by all paths.
#[derive(Debug, Clone)]
pub struct Spectra {
    pub freqs: Vec<f64>,
    /// Wavenumber 2πf/c.
    pub k: Vec<f64>,
    /// Air attenuation of pressure amplitude in nepers per meter.
    pub air_np: Vec<f64>,
}

impl Spectra {
    pub fn new(freqs: Vec<f64>, medium: &Medium) -> Self {
        let c = medium.speed_of_sound();
        let k = freqs.iter().map(|f| 2.0 * std::f64::consts::PI * f / c).collect();
        let air_np = freqs.iter().map(|f| medium.air_attenuation_db_per_m(*f) * std::f64::consts::LN_10 / 20.0).collect();
        Spectra { freqs, k, air_np }
    }
}

/// Source transfer spectrum: on-axis pressure at the reference distance with
/// model latency, excluding user gain/delay/polarity (applied when mixing).
pub fn source_spectrum(model: &SpeakerModel, freqs: &[f64]) -> Vec<Complex64> {
    freqs
        .iter()
        .map(|f| {
            model.on_axis(*f) * Complex64::from_polar(1.0, -2.0 * std::f64::consts::PI * f * model.latency_s)
        })
        .collect()
}

/// User drive of a placed speaker: gain, polarity, delay and filters.
pub fn user_drive(s: &SpeakerInstance, f: f64) -> Complex64 {
    s.processing(f)
}
