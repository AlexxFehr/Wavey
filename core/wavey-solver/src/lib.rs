//! Wavey acoustic solver.
//!
//! Stage 1 is a coherent image-source method: every valid specular path
//! from each speaker to each receiver is summed with its complex
//! directivity, reflection factors, spreading, air absorption and phase.
//! See PLAN.md §8 for the full hybrid design.

pub mod accumulate;
pub mod directivity;
pub mod fdtd;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod freq;
pub mod ism;
pub mod receivers;
pub mod result;
pub mod run;
pub mod scene;

pub use freq::{Resolution, frequency_grid};
pub use receivers::{CutPlane, Microphone, PlaneAxis, PlaneGrid};
pub use result::{MicResult, PlaneMode, PlaneResult, RunStats, SimulationResult};
pub use run::{SolverError, estimate, simulate};
pub use scene::{BackendChoice, SimSettings, SimulationInput, SourceInput, user_drive};
