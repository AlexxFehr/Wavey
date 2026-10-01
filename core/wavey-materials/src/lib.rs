//! Acoustic materials, reflection coefficients and the propagation medium.

mod library;
mod material;
mod medium;
mod spectrum;

pub use library::builtin_materials;
pub use material::{Material, statistical_absorption, fit_real_impedance};
pub use medium::Medium;
pub use spectrum::Spectrum;

pub use num_complex::Complex64;

/// Standard octave band centers used for material tables (Hz).
pub const OCTAVE_BANDS: [f64; 8] = [63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0];
