//! Low-frequency wave solver (FDTD). Settings only for now.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FdtdSettings {
    /// Center of the crossover band between FDTD and geometrical results (Hz).
    pub crossover_hz: f64,
    /// Simulated duration (s).
    pub duration_s: f64,
}

impl Default for FdtdSettings {
    fn default() -> Self {
        Self { crossover_hz: 250.0, duration_s: 1.0 }
    }
}
