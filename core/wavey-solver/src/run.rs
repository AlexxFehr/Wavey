//! Top-level simulation driver.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use glam::DVec3;
use num_complex::Complex32;

use crate::accumulate::{AccumulateJob, accumulate_cpu};
use crate::directivity::Directivity;
use crate::ism::{generate_images, trace_paths};
use crate::raytrace::{self, BandSurface, NB, RaySettings};
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

/// Runs the wave solver on the chosen backend, falling back to the CPU.
#[allow(clippy::too_many_arguments)]
fn run_fdtd(
    grid: &crate::fdtd::FdtdGrid,
    source: usize,
    signal: &[f64],
    receivers: &[usize],
    freqs: &[f64],
    choice: crate::scene::BackendChoice,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f64) + Sync),
    stats: &mut RunStats,
) -> Option<crate::fdtd::Spectra> {
    #[cfg(feature = "gpu")]
    if choice != crate::scene::BackendChoice::Cpu {
        if let Some(g) = crate::gpu::gpu() {
            match g.run_fdtd(grid, source, signal, receivers, freqs, cancel, progress) {
                Ok(r) => return r,
                Err(e) => stats.notes.push(format!("wave solver on GPU failed ({e}); used CPU")),
            }
        }
    }
    let _ = (&choice, &stats);
    crate::fdtd::run_cpu(grid, source, signal, receivers, freqs, cancel, progress)
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
    let band_surfaces: Vec<BandSurface> = input.surface_materials.iter().map(BandSurface::new).collect();
    let (rt_sabine, rt_eyring) = raytrace::statistical_rt(&input.room, &band_surfaces, &input.medium);
    // Per source: late energy at every receiver, and mic echograms.
    let mut lates: Vec<Vec<[f32; NB]>> = Vec::new();
    let mut echos: Vec<Vec<raytrace::Echogram>> = Vec::new();
    let mic_points: Vec<DVec3> = input.microphones.iter().map(|m| m.position).collect();
    let late = s.late_reverb && s.rays_per_source > 0;

    // Wave solver: one grid for all sources.
    let fdtd = match &s.fdtd {
        Some(fs) => {
            progress(0.0, "wave solver: building grid");
            let c = input.medium.speed_of_sound();
            let grid = crate::fdtd::FdtdGrid::build(&input.room, &input.surface_materials, c, fs.f_max(), fs.points_per_wavelength, fs.crossover_hz)
                .map_err(SolverError::Invalid)?;
            let fis: Vec<usize> = (0..nf).filter(|&i| fs.weight(freqs[i]) > 0.0).collect();
            let nodes: Vec<Option<usize>> = receivers.iter().map(|p| if input.room.contains(*p) { grid.nearest_air(*p) } else { None }).collect();
            let duration = if fs.duration_s > 0.0 { fs.duration_s } else { crate::fdtd::auto_duration(&rt_eyring) };
            let steps = (duration / grid.dt).ceil() as usize;
            let (lo, hi) = input.room.bounds();
            let err = crate::fdtd::FdtdSettings::phase_error_deg_per_m(fs.crossover_hz, c, grid.dx) * (hi - lo).length();
            stats.notes.push(format!(
                "wave solver: {:.1} M air cells (Δx = {:.1} cm), {steps} steps over {duration:.2} s; worst-case dispersion at the crossover across the room ≈ {err:.0}°",
                grid.air_cells() as f64 / 1e6,
                grid.dx * 100.0
            ));
            stats.notes.push("wave solver: omnidirectional source with on-axis response; frequency-independent boundary admittance fitted at the crossover".into());
            Some((fs.clone(), grid, fis, nodes, steps))
        }
        None => None,
    };
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

        if let Some((fs, grid, fis, nodes, steps)) = &fdtd {
            let name = &src.instance.name;
            progress(base + 0.86 / ns, &format!("{name}: wave solver"));
            let c = input.medium.speed_of_sound();
            let src_node = grid.nearest_air(center).ok_or_else(|| SolverError::Invalid(format!("speaker `{name}` is not inside the wave-solver grid")))?;
            let signal = crate::fdtd::source_signal(fs.f_max(), grid.dt, *steps);
            let rx: Vec<usize> = nodes.iter().flatten().copied().collect();
            let sub_freqs: Vec<f64> = fis.iter().map(|&i| freqs[i]).collect();
            let fprog = |f: f64| progress(base + (0.86 + 0.04 * f) / ns, &format!("{name}: wave solver"));
            let spectra = run_fdtd(grid, src_node, &signal, &rx, &sub_freqs, input.settings.backend, cancel, &fprog, &mut stats)
                .ok_or(SolverError::Cancelled)?;
            let h0: Vec<_> = fis.iter().map(|&i| source_spectrum(&src.model, &[freqs[i]])[0]).collect();
            let t = crate::fdtd::to_transfer(&spectra, &signal, grid, &sub_freqs, c, src.model.reference.distance_m, &h0);
            let out = transfers.last_mut().unwrap();
            let mut k = 0;
            for (ri, node) in nodes.iter().enumerate() {
                if node.is_none() {
                    continue;
                }
                for (j, &fi) in fis.iter().enumerate() {
                    let w = fs.weight(freqs[fi]);
                    let ism = out[ri * nf + fi];
                    let wave = t[k * fis.len() + j];
                    let v = num_complex::Complex64::new(ism.re as f64, ism.im as f64) * (1.0 - w) + wave * w;
                    out[ri * nf + fi] = Complex32::new(v.re as f32, v.im as f32);
                }
                k += 1;
            }
        }

        if late {
            progress(base + 0.9 / ns, &format!("{}: ray tracing", src.instance.name));
            let max_time = (rt_eyring.iter().copied().filter(|t| t.is_finite()).fold(0.0, f64::max) * 1.2).clamp(0.3, 8.0);
            let settings = RaySettings { rays: s.rays_per_source, ism_order: s.max_order, max_time, seed: 0x5EED + si as u64 };
            let rprog = |f: f64| progress(base + (0.9 + 0.1 * f) / ns, &format!("{}: ray tracing", src.instance.name));
            let out = raytrace::trace(&input.room, &band_surfaces, &input.medium, center, to_local, &src.model, &mic_points, settings, cancel, &rprog)
                .ok_or(SolverError::Cancelled)?;
            lates.push(receivers.iter().map(|p| out.late.at(*p)).collect());
            echos.push(out.echograms);
        }
    }

    // Split the receiver-major transfers into mics and planes.
    let take = |t: &Vec<Complex32>, first: usize, count: usize| t[first * nf..(first + count) * nf].to_vec();
    let mics = input
        .microphones
        .iter()
        .enumerate()
        .map(|(i, m)| MicResult {
            mic: m.clone(),
            transfer: transfers.iter().map(|t| take(t, i, 1)).collect(),
            late: lates.iter().map(|l| l[i]).collect(),
            echograms: echos.iter().map(|e| e[i].clone()).collect(),
        })
        .collect();
    let mut first = input.microphones.len();
    let mut planes = Vec::new();
    for (p, g) in input.planes.iter().zip(grids) {
        let n = g.inside_count();
        planes.push(PlaneResult {
            plane: p.clone(),
            transfer: transfers.iter().map(|t| take(t, first, n)).collect(),
            late: lates.iter().map(|l| l[first..first + n].to_vec()).collect(),
            grid: g,
        });
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
        rt_sabine,
        rt_eyring,
        has_late: late,
        fdtd: s.fdtd.clone(),
    })
}
