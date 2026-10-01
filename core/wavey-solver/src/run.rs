//! Top-level simulation driver.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use glam::DVec3;
use num_complex::Complex32;

use crate::accumulate::{AccumulateJob, accumulate_cpu};
use crate::directivity::Directivity;
use crate::ism::{generate_images, trace_paths};
use crate::result::{MicResult, PlaneResult, RunStats, SimulationResult};
use crate::scene::{SimulationInput, Spectra, SurfaceAcoustics, source_spectrum};

#[derive(Debug, thiserror::Error)]
pub enum SolverError {
    #[error("{0}")]
    Invalid(String),
    #[error("simulation cancelled")]
    Cancelled,
}

/// Rough size of a run, for the UI before starting.
#[derive(Debug, Clone, Copy, Default)]
pub struct Estimate {
    pub frequencies: usize,
    pub receivers: usize,
    pub images_per_source: usize,
    pub result_bytes: usize,
}

pub fn estimate(input: &SimulationInput) -> Result<Estimate, SolverError> {
    input.settings.validate().map_err(SolverError::Invalid)?;
    let nf = input.settings.frequencies().len();
    let mut receivers = input.microphones.len();
    for p in &input.planes {
        receivers += p.grid(&input.room).map_err(SolverError::Invalid)?.inside_count();
    }
    let images = input
        .sources
        .first()
        .map(|s| generate_images(&input.room, s.instance.acoustic_center(&s.model), input.settings.max_order).len())
        .unwrap_or(0);
    Ok(Estimate {
        frequencies: nf,
        receivers,
        images_per_source: images,
        result_bytes: receivers * nf * input.sources.len() * 8,
    })
}

/// Runs the accumulation on the chosen backend, falling back to the CPU.
fn accumulate(job: &AccumulateJob, choice: crate::scene::BackendChoice, stats: &mut RunStats) -> Vec<Complex32> {
    use crate::scene::BackendChoice;
    #[cfg(feature = "gpu")]
    if choice != BackendChoice::Cpu {
        match crate::gpu::gpu().ok_or_else(|| "no GPU adapter".to_string()).and_then(|g| {
            let r = g.accumulate(job);
            stats.backend = format!("gpu: {}", g.name);
            r
        }) {
            Ok(out) => return out,
            Err(e) => {
                stats.backend = "cpu".into();
                stats.notes.push(format!("GPU unavailable ({e}); used CPU"));
            }
        }
    }
    #[cfg(not(feature = "gpu"))]
    if choice == BackendChoice::Gpu {
        stats.notes.push("built without GPU support; used CPU".into());
    }
    accumulate_cpu(job)
}

/// Largest result kept in memory (bytes).
pub const MAX_RESULT_BYTES: usize = 4 << 30;

/// Runs the simulation. `progress` receives (fraction 0…1, stage text).
pub fn simulate(
    input: &SimulationInput,
    progress: &(dyn Fn(f64, &str) + Sync),
    cancel: &AtomicBool,
) -> Result<SimulationResult, SolverError> {
    let start = Instant::now();
    let s = &input.settings;
    s.validate().map_err(SolverError::Invalid)?;
    if input.sources.is_empty() {
        return Err(SolverError::Invalid("place at least one speaker".into()));
    }
    if input.microphones.is_empty() && input.planes.is_empty() {
        return Err(SolverError::Invalid("add a microphone or a cut plane".into()));
    }
    if input.surface_materials.len() != input.room.surfaces.len() {
        return Err(SolverError::Invalid("every surface needs a material".into()));
    }
    for src in &input.sources {
        let c = src.instance.acoustic_center(&src.model);
        if !input.room.contains(c) {
            return Err(SolverError::Invalid(format!("speaker `{}` is outside the room", src.instance.name)));
        }
    }

    let freqs = s.frequencies();
    let nf = freqs.len();
    let spectra = Spectra::new(freqs.clone(), &input.medium);
    let surfaces: Vec<SurfaceAcoustics> =
        input.surface_materials.iter().map(|m| SurfaceAcoustics::new(m, &freqs)).collect();

    // Receivers: microphones first, then each plane's inside points.
    let mut receivers: Vec<DVec3> = input.microphones.iter().map(|m| m.position).collect();
    let mut grids = Vec::new();
    for p in &input.planes {
        let g = p.grid(&input.room).map_err(SolverError::Invalid)?;
        receivers.extend(g.inside_points());
        grids.push(g);
    }
    let bytes = receivers.len() * nf * input.sources.len() * 8;
    if bytes > MAX_RESULT_BYTES {
        return Err(SolverError::Invalid(format!(
            "result would need {:.1} GB; use fewer frequencies or coarser cut planes",
            bytes as f64 / 1e9
        )));
    }

    let mut stats = RunStats { backend: "cpu".into(), receivers: receivers.len(), ..Default::default() };
    let mut transfers: Vec<Vec<Complex32>> = Vec::new();
    let ns = input.sources.len() as f64;
    for (si, src) in input.sources.iter().enumerate() {
        let base = si as f64 / ns;
        let center = src.instance.acoustic_center(&src.model);
        progress(base, &format!("{}: image sources", src.instance.name));
        let images = generate_images(&input.room, center, s.max_order);
        stats.images += images.len();

        let directivity = Directivity::from_model(&src.model, &freqs);
        let done = AtomicUsize::new(0);
        let total = receivers.len().max(1);
        let tick = |n: usize| {
            let d = done.fetch_add(n, Ordering::Relaxed) + n;
            if d % 256 == 0 || d == total {
                progress(base + 0.8 / ns * d as f64 / total as f64, &format!("{}: tracing paths", src.instance.name));
            }
        };
        let to_local = src.instance.rotation().transpose();
        let paths = trace_paths(&input.room, &images, to_local, &directivity, &receivers, cancel, &tick)
            .ok_or(SolverError::Cancelled)?;
        stats.paths += paths.paths.len();

        progress(base + 0.85 / ns, &format!("{}: summing", src.instance.name));
        let h0 = source_spectrum(&src.model, &freqs);
        let job = AccumulateJob {
            paths: &paths,
            spectra: &spectra,
            h0: &h0,
            r_ref: src.model.reference.distance_m,
            directivity: &directivity,
            surfaces: &surfaces,
        };
        transfers.push(accumulate(&job, input.settings.backend, &mut stats));
        if cancel.load(Ordering::Relaxed) {
            return Err(SolverError::Cancelled);
        }
    }

    // Split the receiver-major transfers into mics and planes.
    let take = |t: &Vec<Complex32>, first: usize, count: usize| t[first * nf..(first + count) * nf].to_vec();
    let mics = input
        .microphones
        .iter()
        .enumerate()
        .map(|(i, m)| MicResult { mic: m.clone(), transfer: transfers.iter().map(|t| take(t, i, 1)).collect() })
        .collect();
    let mut first = input.microphones.len();
    let mut planes = Vec::new();
    for (p, g) in input.planes.iter().zip(grids) {
        let n = g.inside_count();
        planes.push(PlaneResult { plane: p.clone(), transfer: transfers.iter().map(|t| take(t, first, n)).collect(), grid: g });
        first += n;
    }

    stats.seconds = start.elapsed().as_secs_f64();
    progress(1.0, "done");
    Ok(SimulationResult {
        freqs,
        speaker_ids: input.sources.iter().map(|s| s.instance.id).collect(),
        mics,
        planes,
        stats,
        fingerprint: 0,
    })
}
