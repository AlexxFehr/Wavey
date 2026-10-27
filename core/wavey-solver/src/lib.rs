//! Wavey acoustic solver.
//!
//! Stage 1 is a coherent image-source method: every valid specular path
//! from each speaker to each receiver is summed with its complex
//! directivity, reflection factors, spreading, air absorption and phase.
//! See PLAN.md §8 for the full hybrid design.

pub mod accumulate;
pub mod analysis;
pub mod decay;
pub mod diffraction;
pub mod directivity;
pub mod fdtd;
#[cfg(feature = "gpu")]
pub mod gpu;
#[cfg(feature = "gpu")]
mod gpu_fdtd;
pub mod freefield;
pub mod freq;
pub mod prof;
pub mod progress;
pub mod ism;
pub mod raytrace;
pub mod reflection;
pub mod receivers;
pub mod result;
pub mod run;
pub mod scene;
pub mod vertical;

pub use freq::{Resolution, frequency_grid};
pub use receivers::{CutPlane, Drape, Microphone, PlaneAxis, PlaneGrid};
pub use result::{DecayQuantity, MicResult, PlaneMode, PlaneResult, RunStats, SimulationResult};
pub use run::{SolverError, estimate, simulate};
pub use scene::{BackendChoice, SimMode, SimSettings, SimulationInput, SourceInput, user_drive};

/// Fraction of wall-clock time the GPU may be busy (1 = no limit). Lower
/// values rest the GPU between batches, trading speed for heat and noise.
static DUTY_CYCLE_MILLI: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1000);

pub fn set_duty_cycle(duty: f64) {
    let d = (duty.clamp(0.02, 1.0) * 1000.0).round() as u32;
    DUTY_CYCLE_MILLI.store(d, std::sync::atomic::Ordering::Relaxed);
}

pub fn duty_cycle() -> f64 {
    DUTY_CYCLE_MILLI.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1000.0
}

/// After `busy` of GPU work, sleep so the busy fraction matches the duty cycle.
pub(crate) fn rest_after(busy: std::time::Duration) {
    let d = duty_cycle();
    if d < 0.999 {
        std::thread::sleep(busy.mul_f64((1.0 - d) / d));
    }
}
