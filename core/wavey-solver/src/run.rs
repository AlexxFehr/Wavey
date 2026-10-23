//! Top-level simulation driver.

use std::f64::consts::PI;
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

/// Adds the diffracted field on the chosen backend, falling back to the CPU.
fn accumulate_diffraction(
    paths: &[Vec<crate::diffraction::DiffPath>],
    edges: &[crate::diffraction::Edge],
    job: &AccumulateJob,
    choice: crate::scene::BackendChoice,
    out: &mut [Complex32],
    stats: &mut RunStats,
) {
    if paths.iter().all(Vec::is_empty) {
        return;
    }
    #[cfg(feature = "gpu")]
    if choice != crate::scene::BackendChoice::Cpu {
        match crate::gpu::gpu().ok_or_else(|| "no GPU adapter".to_string()).and_then(|g| g.accumulate_diffraction(paths, edges, job, out)) {
            Ok(()) => return,
            Err(e) => stats.notes.push(format!("diffraction: GPU unavailable ({e}); used CPU")),
        }
    }
    #[cfg(not(feature = "gpu"))]
    let _ = (choice, &stats);
    crate::diffraction::accumulate(paths, edges, job, out);
}

/// Exact early energy-time histograms at the mics: direct sound and every
/// valid specular path up to the image-source order, with per-band
/// absorption, the scattered part removed (it is in the rays), and air
/// absorption. Same units as the ray-traced echograms.
fn ism_echograms(
    room: &wavey_geom::Room,
    images: &[crate::ism::Image],
    surfaces: &[BandSurface],
    medium: &wavey_materials::Medium,
    model: &wavey_speaker::SpeakerModel,
    to_local: glam::DMat3,
    mics: &[DVec3],
) -> Vec<raytrace::Echogram> {
    use rayon::prelude::*;
    let c = medium.speed_of_sound();
    let air: [f64; NB] = raytrace::BANDS.map(|f| medium.air_intensity_coefficient(f));
    let r_ref = model.reference.distance_m;
    let dims = raytrace::panel_dims(room);
    let smooth = raytrace::smooth_clusters(room, surfaces, &dims, c);
    let source = images[0].pos;
    mics.par_iter()
        .map(|m| {
            let mut bands = vec![Vec::<f32>::new(); NB];
            for i in 0..images.len() {
                let Some(v) = crate::ism::validate(room, images, i, *m) else { continue };
                let bin = (v.r / c / raytrace::BIN) as usize;
                // Legs around each reflection (receiver → … → source) and the panel hit.
                let chain: Vec<DVec3> = std::iter::once(*m).chain(v.points.iter().copied()).chain(std::iter::once(source)).collect();
                let refl: Vec<(usize, f64, f64, [f64; 2], usize)> = v
                    .refl
                    .iter()
                    .enumerate()
                    .map(|(k, (s, cos))| {
                        let (d_out, d_in) = ((chain[k + 1] - chain[k]).length(), (chain[k + 2] - chain[k + 1]).length());
                        let a_star = 2.0 * d_in * d_out / (d_in + d_out).max(1e-9);
                        let tri = room.surfaces[*s as usize].triangles[0] as usize;
                        let hit = room.bvh.intersect(chain[k], (chain[k + 1] - chain[k]).normalize(), 1e-6, f64::INFINITY).map_or(tri, |h| h.triangle as usize);
                        (*s as usize, *cos as f64, a_star, dims[hit], hit)
                    })
                    .collect();
                for b in 0..NB {
                    let mut e = raytrace::band_power(model, to_local * v.departure, raytrace::BANDS[b]) * r_ref * r_ref / (v.r * v.r);
                    for (s, cos, a_star, pd, hit) in &refl {
                        let bs = &surfaces[*s];
                        // Facets of a surface that is smooth at this band
                        // reflect off its mean plane: in the rays.
                        let spec = if smooth[*hit][b].is_some() { 0.0 } else { raytrace::specular_fraction(raytrace::BANDS[b], c, *a_star, *cos, *pd) };
                        e *= (1.0 - bs.absorption[b]) * (1.0 - bs.scattering[b]) * spec;
                    }
                    e *= (-air[b] * v.r).exp();
                    if bands[b].len() <= bin {
                        bands[b].resize(bin + 1, 0.0);
                    }
                    bands[b][bin] += e as f32;
                }
            }
            raytrace::Echogram { bands }
        })
        .collect()
}

/// Runs the wave solver on the chosen backend, falling back to the CPU.
#[allow(clippy::too_many_arguments)]
fn run_fdtd(
    grid: &crate::fdtd::FdtdGrid,
    source: &[(usize, f64)],
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

/// Cabinet calibrations of this process, by `calibration_key`.
static CALIBRATIONS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<u64, crate::fdtd::CabinetCalibration>>> =
    std::sync::LazyLock::new(Default::default);

/// Bump when the calibration changes, so stored ones are not reused.
const CALIBRATION_VERSION: u32 = 1;

/// Where calibrations are kept between runs (none with WAVEY_NO_DISK_CACHE).
fn calibration_dir() -> Option<std::path::PathBuf> {
    if std::env::var_os("WAVEY_NO_DISK_CACHE").is_some() {
        return None;
    }
    if let Some(d) = std::env::var_os("WAVEY_CACHE_DIR") {
        return Some(std::path::PathBuf::from(d).join("calibrations"));
    }
    let home = std::path::PathBuf::from(std::env::var_os("HOME")?);
    Some(if cfg!(target_os = "macos") { home.join("Library/Caches/app.wavey/calibrations") } else { home.join(".cache/wavey/calibrations") })
}

fn load_calibration(key: u64) -> Option<crate::fdtd::CabinetCalibration> {
    let bytes = std::fs::read(calibration_dir()?.join(format!("{key:016x}.json"))).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Best effort: a calibration that cannot be stored is just recomputed next time.
fn store_calibration(key: u64, cal: &crate::fdtd::CabinetCalibration) {
    let Some(dir) = calibration_dir() else { return };
    if std::fs::create_dir_all(&dir).is_ok()
        && let Ok(json) = serde_json::to_vec(cal)
    {
        let tmp = dir.join(format!("{key:016x}.tmp"));
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, dir.join(format!("{key:016x}.json")));
        }
    }
}

/// Everything a cabinet calibration depends on, hashed.
#[allow(clippy::too_many_arguments)]
fn calibration_key(v: &[DVec3], t: &[[u32; 3]], ac: DVec3, c: f64, f_max: f64, ppw: f64, freqs: &[f64], backend: crate::scene::BackendChoice) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in v {
        p.to_array().map(f64::to_bits).hash(&mut h);
    }
    t.hash(&mut h);
    ac.to_array().map(f64::to_bits).hash(&mut h);
    [c, f_max, ppw].map(f64::to_bits).hash(&mut h);
    freqs.iter().map(|f| f.to_bits()).collect::<Vec<_>>().hash(&mut h);
    format!("{backend:?}").hash(&mut h);
    CALIBRATION_VERSION.hash(&mut h);
    h.finish()
}

/// Adds a finished background diffraction sum to its source's transfer.
fn finish_diffraction(i: usize, h: std::thread::ScopedJoinHandle<'_, (Vec<Complex32>, RunStats)>, transfers: &mut [Vec<Complex32>], stats: &mut RunStats) {
    let (d, st) = h.join().expect("diffraction thread");
    for (o, v) in transfers[i].iter_mut().zip(d) {
        *o += v;
    }
    stats.notes.extend(st.notes);
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
    // Free field and open air have no room: direct sound, and the ground's
    // reflection over open air. Nothing else is modelled.
    let free_field = s.mode == crate::scene::SimMode::FreeField;
    let open_field = free_field || input.ground.is_some();
    let direct_only;
    let s = if open_field {
        direct_only = crate::scene::SimSettings { max_order: 0, late_reverb: false, fdtd: None, diffraction: false, impedance_reflection: false, ..s.clone() };
        &direct_only
    } else {
        s
    };
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
        if !open_field && !input.room.contains(c) {
            return Err(SolverError::Invalid(format!("speaker `{}` is outside the room", src.instance.name)));
        }
    }

    let freqs = s.frequencies();
    let nf = freqs.len();
    let spectra = Spectra::new(freqs.clone(), &input.medium);
    let mut surfaces: Vec<SurfaceAcoustics> =
        input.surface_materials.iter().map(|m| SurfaceAcoustics::new(m, &freqs)).collect();
    // Over open air the only surface is the ground, with the reflection it
    // has at the angle (grazing incidence over soft ground cancels).
    let ground = input.ground.as_ref().filter(|_| !free_field);
    if let Some(g) = ground {
        let mut sa = SurfaceAcoustics::new(g, &freqs);
        let fit = crate::fdtd::fit_surfaces(std::slice::from_ref(g), (20f64.min(s.f_min), s.f_max.max(40.0)));
        for (fi, f) in freqs.iter().enumerate() {
            if sa.zeta[fi].is_none() {
                sa.zeta[fi] = Some(fit[0].impedance(*f));
            }
        }
        surfaces = vec![sa];
    }
    // With the wave solver, both methods use the same fitted surface
    // impedance in its band, so they agree across the crossover.
    let fits = s.fdtd.as_ref().map(|fs| {
        progress(0.0, "wave solver: fitting surface impedances");
        crate::fdtd::fit_surfaces(&input.surface_materials, (20f64.min(s.f_min), fs.f_max()))
    });
    if let (Some(fs), Some(fits)) = (&s.fdtd, &fits) {
        for (sa, fit) in surfaces.iter_mut().zip(fits) {
            for (fi, f) in freqs.iter().enumerate() {
                if *f <= fs.f_max() {
                    sa.zeta[fi] = Some(fit.impedance(*f));
                }
            }
        }
    }
    // Above the wave band (or without it): impedances fitted over the whole
    // simulated band, unless a material gives its own.
    let mut impedance_note = None;
    if s.impedance_reflection {
        progress(0.0, "fitting surface impedances");
        let full = crate::fdtd::fit_surfaces(&input.surface_materials, (20f64.min(s.f_min), s.f_max.max(40.0)));
        let worst = full.iter().map(|f| f.max_error).fold(0.0, f64::max);
        for (sa, fit) in surfaces.iter_mut().zip(&full) {
            for (fi, f) in freqs.iter().enumerate() {
                if sa.zeta[fi].is_none() {
                    sa.zeta[fi] = Some(fit.impedance(*f));
                }
            }
        }
        impedance_note = Some(format!(
            "reflection: angle-dependent from fitted surface impedances at all frequencies (worst α fit error {worst:.3}), spherical-wave factor"
        ));
    }

    // Receivers: microphones first, then each plane's inside points.
    let mut receivers: Vec<DVec3> = input.microphones.iter().map(|m| m.position).collect();
    // Height step for grouping the late-field coherence: 1 mm, but 5 cm on
    // audience areas, whose points follow the floor at many heights.
    let mut height_step: Vec<f64> = vec![0.001; receivers.len()];
    let mut grids = Vec::new();
    for p in &input.planes {
        let g = p.grid(&input.room).map_err(SolverError::Invalid)?;
        let pts = g.inside_points();
        height_step.extend(std::iter::repeat_n(if p.drape.is_some() { 0.05 } else { 0.001 }, pts.len()));
        receivers.extend(pts);
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
    stats.notes.extend(impedance_note);
    let mut transfers: Vec<Vec<Complex32>> = Vec::new();
    let band_surfaces: Vec<BandSurface> = input.surface_materials.iter().map(BandSurface::new).collect();
    let (rt_sabine, rt_eyring) = raytrace::statistical_rt(&input.room, &band_surfaces, &input.medium);
    // Per source: late energy at every receiver, and mic echograms.
    let mut lates: Vec<Vec<[f32; NB]>> = Vec::new();
    let mut echos: Vec<Vec<raytrace::Echogram>> = Vec::new();
    let mic_points: Vec<DVec3> = input.microphones.iter().map(|m| m.position).collect();
    // Below the crossover band the result is the wave solver's alone.
    let all_wave = s.fdtd.as_ref().is_some_and(|fs| freqs.iter().all(|f| fs.weight(*f) >= 1.0));
    let late = s.late_reverb && s.rays_per_source > 0 && !all_wave;
    if all_wave {
        stats.notes.push("all frequencies are below the crossover band: wave solver only (no image sources, diffraction or rays)".into());
    }

    // Wave solver: one grid for all sources.
    let fdtd = match &s.fdtd {
        Some(fs) => {
            progress(0.0, "wave solver: building grid");
            let c = input.medium.speed_of_sound();
            let fits = fits.as_ref().unwrap();
            // Resolve only what is used: up to the highest requested frequency
            // the wave solver contributes to, with the accuracy's headroom
            // (High keeps twice the points per wavelength there, which holds
            // the phase error where it matters, e.g. for tuning arrays).
            let grid_fmax = fs.grid_fmax(&freqs);
            // Cabinets as rigid obstacles (every playing speaker's, in every run).
            let cabinets: Vec<Option<crate::fdtd::Solid>> = input
                .sources
                .iter()
                .map(|src| {
                    if !fs.cabinets || src.model.balloon.is_none() {
                        return None;
                    }
                    let (v, t) = src.model.cabinet_mesh();
                    crate::fdtd::Solid::from_mesh(&v, &t, src.instance.rotation(), src.instance.position)
                })
                .collect();
            let solids: Vec<crate::fdtd::Solid> = cabinets.iter().flatten().cloned().collect();
            let grid = crate::prof::time("wave grid", || crate::fdtd::FdtdGrid::build_with(&input.room, &solids, fits, c, grid_fmax, fs.points_per_wavelength))
                .map_err(SolverError::Invalid)?;
            let worst = fits.iter().map(|f| f.max_error).fold(0.0, f64::max);
            stats.notes.push(format!(
                "wave solver: frequency-dependent surface impedances fitted to α(f) up to {:.0} Hz (worst α error {:.3}), shared with the image sources",
                fs.f_max(),
                worst
            ));
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
            Some((fs.clone(), grid, fis, nodes, steps, grid_fmax, cabinets.iter().map(Option::is_some).collect::<Vec<bool>>()))
        }
        None => None,
    };
    let edge_graph = s.diffraction.then(|| crate::diffraction::EdgeGraph::build(&input.room));
    if let Some(g) = &edge_graph {
        stats.notes.push(format!(
            "diffraction: {} edges (UTD; direct sound and first reflections once, direct sound edge to edge)",
            g.edges.len()
        ));
    }
    let ns = input.sources.len() as f64;
    let max_time = (rt_eyring.iter().copied().filter(|t| t.is_finite()).fold(0.0, f64::max) * 2.0).clamp(0.3, 12.0);
    // The wave solver (GPU) runs in the background from the start, while
    // the geometrical stages run here; its result is blended in at the end.
    let bg_cancel = AtomicBool::new(false);
    // The geometrical stages and the wave solver report separately; the bar
    // follows the slower one.
    let tracker = std::sync::Mutex::new(crate::progress::Tracker::new(fdtd.is_some()));
    let geo = |fraction: f64, stage: &str| {
        let (bar, text) = tracker.lock().unwrap().geometry(fraction, stage);
        progress(bar, &text);
    };
    let wav = |fraction: f64, stage: &str| {
        let (bar, text) = tracker.lock().unwrap().wave(fraction, stage);
        progress(bar, &text);
    };
    if all_wave {
        geo(1.0, "wave solver");
    }
    std::thread::scope(|outer| -> Result<(), SolverError> {
    let wave_job = fdtd.as_ref().map(|(fs, grid, fis, nodes, steps, grid_fmax, has_cab)| {
        let (bg_cancel, wav, freqs) = (&bg_cancel, &wav, &freqs);
        let gfm = *grid_fmax;
        outer.spawn(move || -> Result<(Vec<Vec<num_complex::Complex64>>, RunStats), SolverError> {
            let cancel = bg_cancel;
            let mut stats = RunStats::default();
            let progress = |f: f64, m: &str| wav(f, m);
            let c = input.medium.speed_of_sound();
            // Each cabinet model's free-field on-axis gain and directivity,
            // measured in the wave solver (absorbing box, with and without it).
            let mut calib: std::collections::HashMap<String, crate::fdtd::CabinetCalibration> = std::collections::HashMap::new();
            let sub_freqs: Vec<f64> = fis.iter().map(|&i| freqs[i]).collect();
            for (src, cab) in input.sources.iter().zip(has_cab.iter()) {
                if !*cab || calib.contains_key(&src.model.id) {
                    continue;
                }
                progress(0.0, &format!("wave solver: calibrating the {} cabinet", src.model.name));
                let (v, t) = src.model.cabinet_mesh();
                let ac = DVec3::from_array(src.model.acoustic_center_m);
                let runner = |g: &crate::fdtd::FdtdGrid, taps: &[(usize, f64)], sig: &[f64], rx: &[usize], f: &[f64]| {
                    run_fdtd(g, taps, sig, rx, f, input.settings.backend, cancel, &|_| {}, &mut RunStats::default())
                };
                // Depends only on the cabinet, the air and the grid: kept for
                // later solves in this process (re-solving after an edit).
                let key = calibration_key(&v, &t, ac, c, gfm, fs.points_per_wavelength, &sub_freqs, input.settings.backend);
                let cached = CALIBRATIONS.lock().unwrap().get(&key).cloned().or_else(|| load_calibration(key));
                let cal = match cached {
                    Some(cal) => cal,
                    None => {
                        let cal = crate::prof::time("cabinet calibration", || crate::fdtd::calibrate_cabinet(&v, &t, ac, c, gfm, fs.points_per_wavelength, &sub_freqs, &runner))
                            .ok_or(SolverError::Cancelled)?;
                        store_calibration(key, &cal);
                        cal
                    }
                };
                CALIBRATIONS.lock().unwrap().insert(key, cal.clone());
                // Compare the simulated rear level with the measured balloon near the top of the band.
                if let Some(j) = (0..sub_freqs.len()).rev().find(|&j| sub_freqs[j] <= gfm / 1.5).or(Some(0))
                    && !sub_freqs.is_empty()
                {
                    let f = sub_freqs[j];
                    let rear = cal.angles_deg.iter().position(|a| *a == 180.0).unwrap_or(6);
                    let sim = 20.0 * cal.directivity[rear][j].norm().max(1e-9).log10();
                    let meas = 20.0 * src.model.directivity(f, DVec3::NEG_X).norm().max(1e-9).log10();
                    stats.notes.push(format!(
                        "wave solver: {} cabinet simulated as a rigid box (on-axis gain {:+.1} dB at {:.0} Hz; rear {:+.1} dB simulated vs {:+.1} dB measured)",
                        src.model.name,
                        20.0 * cal.on_axis[j].norm().max(1e-9).log10(),
                        f,
                        sim,
                        meas
                    ));
                }
                calib.insert(src.model.id.clone(), cal);
            }
            let mut ts = Vec::with_capacity(input.sources.len());
            for (si, src) in input.sources.iter().enumerate() {
            let base = si as f64 / ns;
            let center = src.instance.acoustic_center(&src.model);
            let name = &src.instance.name;
            progress(base, &format!("{name}: wave solver"));
            let c = input.medium.speed_of_sound();
            let cal = calib.get(&src.model.id);
            // With a cabinet the driver radiates from the baffle in front of
            // the acoustic centre; spread over the nearest nodes, not rounded.
            // An acoustic centre deep inside its own cabinet (e.g. a model
            // without geometry: the box centre) radiates from the baffle.
            let src_node = (if cal.is_some() { grid.air_along(center, src.instance.axis()) } else { None })
                .or_else(|| grid.nearest_air(center))
                .or_else(|| grid.air_along(center, src.instance.axis()))
                .ok_or_else(|| SolverError::Invalid(format!("speaker `{name}` is not inside the wave-solver grid")))?;
            // Without a cabinet: a point source exactly at the acoustic centre.
            let driver = Some(if cal.is_some() { grid.baffle_point(center, src.instance.axis()) } else { center });
            let signal = crate::fdtd::source_signal(*grid_fmax, grid.dt, *steps);
            let rx: Vec<usize> = nodes.iter().flatten().copied().collect();
            let sub_freqs: Vec<f64> = fis.iter().map(|&i| freqs[i]).collect();
            let h0: Vec<_> = fis.iter().map(|&i| source_spectrum(&src.model, &[freqs[i]])[0]).collect();
            // Directional source: monopole + dipoles fitted to the balloon.
            let order = if src.model.balloon.is_some() && cal.is_none() { fs.source_order.min(1) } else { 0 };
            let comps = if let Some(p) = driver.filter(|_| cal.is_some() || order == 0) {
                vec![grid.taps_at(p).unwrap_or_else(|| vec![(src_node, 1.0)])]
            } else { crate::fdtd::source_components(grid, src_node, order).unwrap_or_else(|| {
                stats.notes.push(format!("wave solver: `{name}` is too close to a surface for a directional source; used omnidirectional"));
                vec![vec![(src_node, 1.0)]]
            }) };
            let to_room = src.instance.rotation();
            let weights: Vec<Vec<num_complex::Complex64>> = if comps.len() == 1 {
                vec![vec![num_complex::Complex64::new(1.0, 0.0)]; sub_freqs.len()]
            } else {
                let mut worst = 0.0f64;
                let w = sub_freqs
                    .iter()
                    .map(|f| {
                        let (a0, a, res) = crate::fdtd::fit_first_order(&src.model, to_room, *f);
                        worst = worst.max(res);
                        crate::fdtd::component_weights(a0, a, 2.0 * std::f64::consts::PI * f / c, grid.dx)
                    })
                    .collect();
                stats.notes.push(format!(
                    "wave solver: `{name}` directivity fitted to first order (worst residual {:.0} % of the balloon up to {:.0} Hz)",
                    worst * 100.0,
                    grid_fmax
                ));
                w
            };
            let mut t = vec![num_complex::Complex64::new(0.0, 0.0); rx.len() * sub_freqs.len()];
            for (ci, taps) in comps.iter().enumerate() {
                let fprog = |f: f64| {
                    progress(base + (ci as f64 + f) / comps.len() as f64 / ns, &format!("{name}: wave solver {}/{}", ci + 1, comps.len()))
                };
                let spectra = crate::prof::time("wave solver run", || run_fdtd(grid, taps, &signal, &rx, &sub_freqs, input.settings.backend, cancel, &fprog, &mut stats))
                    .ok_or(SolverError::Cancelled)?;
                let mut tc = crate::fdtd::to_transfer(&spectra, &signal, grid, &sub_freqs, c, src.model.reference.distance_m, &h0);
                if let Some(cal) = cal {
                    // Calibrated so the cabinet's free-field on-axis response is the measured one.
                    for (k, v) in tc.iter_mut().enumerate() {
                        *v /= cal.on_axis[k % sub_freqs.len()];
                    }
                }
                for (k, v) in tc.iter().enumerate() {
                    t[k] += weights[k % sub_freqs.len()][ci] * v;
                }
            }
            ts.push(t);
            }
            progress(1.0, "wave solver");
            Ok((ts, stats))
        })
    });
    let geometry = (|| -> Result<(), SolverError> {
    let mut pending: Option<(usize, std::thread::ScopedJoinHandle<'_, (Vec<Complex32>, RunStats)>)> = None;
    for (si, src) in input.sources.iter().enumerate() {
      if cancel.load(Ordering::Relaxed) { bg_cancel.store(true, Ordering::Relaxed); }
      std::thread::scope(|sc| -> Result<(), SolverError> {
        let base = si as f64 / ns;
        let center = src.instance.acoustic_center(&src.model);
        let to_local = src.instance.rotation().transpose();
        let mut early_echo: Option<Vec<raytrace::Echogram>> = None;
        // The late field (CPU) runs alongside the paths and the GPU work.
        let band_surfaces = &band_surfaces;
        let mic_points = &mic_points;
        let ray_job = late.then(|| {
            sc.spawn(move || {
                // Long enough for a 35 dB decay even where rays decay slower than Eyring.
                let settings = RaySettings { rays: s.rays_per_source, ism_order: s.max_order, max_time, seed: 0x5EED + si as u64 };
                crate::prof::time("ray tracing", || raytrace::trace(&input.room, band_surfaces, &input.medium, center, to_local, &src.model, mic_points, settings, cancel, &|_| {}))
            })
        });
        if all_wave {
            // Every frequency comes from the wave solver: no geometrical paths needed.
            transfers.push(vec![Complex32::new(0.0, 0.0); receivers.len() * nf]);
        } else {
            geo(base, &format!("{}: image sources", src.instance.name));
            let images = crate::prof::time("image sources", || generate_images(&input.room, center, s.max_order));
            if late {
                early_echo = Some(crate::prof::time("early echograms", || ism_echograms(&input.room, &images, &band_surfaces, &input.medium, &src.model, to_local, &mic_points)));
            }
            stats.images += images.len();

            let directivity = Directivity::from_model(&src.model, &freqs);
            let done = AtomicUsize::new(0);
            let total = receivers.len().max(1);
            let tick = |n: usize| {
                let d = done.fetch_add(n, Ordering::Relaxed) + n;
                if d % 256 == 0 || d == total {
                    geo(base + 0.8 / ns * d as f64 / total as f64, &format!("{}: tracing paths", src.instance.name));
                }
            };

            let paths = if open_field {
                crate::ism::open_field_paths(center, ground.is_some(), to_local, &directivity, &receivers)
            } else {
                crate::prof::time("trace paths", || trace_paths(&input.room, &images, to_local, &directivity, &receivers, cancel, &tick))
                    .ok_or(SolverError::Cancelled)?
            };
            stats.paths += paths.paths.len();

            geo(base + 0.85 / ns, &format!("{}: summing", src.instance.name));
            let h0 = source_spectrum(&src.model, &freqs);
            let job = AccumulateJob {
                paths: &paths,
                spectra: &spectra,
                h0: &h0,
                r_ref: src.model.reference.distance_m,
                directivity: &directivity,
                surfaces: &surfaces,
            };
            // The GPU sums the paths while the CPU traces the diffracted ones.
            let (transfer, dpaths) = std::thread::scope(|sc2| {
                let acc = sc2.spawn(|| {
                    let mut st = RunStats::default();
                    let t = crate::prof::time("accumulate", || accumulate(&job, input.settings.backend, &mut st));
                    (t, st)
                });
                let dpaths = edge_graph.as_ref().map(|graph| {
                geo(base + 0.82 / ns, &format!("{}: diffraction", src.instance.name));
                crate::prof::time("trace diffraction", || crate::diffraction::trace_diffraction(
                    &input.room,
                    graph,
                    &images,
                    to_local,
                    &directivity,
                    &receivers,
                    crate::diffraction::DiffractionSettings {
                        prune: Some((2.0 * PI * s.f_min.max(20.0) / input.medium.speed_of_sound(), 50.0)),
                        ..Default::default()
                    },
                    cancel,
                ))
                });
                let (t, st) = acc.join().expect("accumulate thread");
                if !st.backend.is_empty() { stats.backend = st.backend; }
                stats.notes.extend(st.notes);
                (t, dpaths)
            });
            transfers.push(transfer);
            if let (Some(graph), Some(dpaths)) = (&edge_graph, dpaths) {
                let dpaths = dpaths.ok_or(SolverError::Cancelled)?;
                stats.diffracted_paths += dpaths.iter().map(Vec::len).sum::<usize>();
                // The GPU sums the diffracted paths while the CPU goes on
                // with the next source; one sum in flight at a time.
                if let Some((i, h)) = pending.take() {
                    finish_diffraction(i, h, &mut transfers, &mut stats);
                }
                let (spectra, surfaces, backend) = (&spectra, &surfaces, input.settings.backend);
                let (r_ref, len) = (src.model.reference.distance_m, transfers[si].len());
                pending = Some((si, outer.spawn(move || {
                    let job = AccumulateJob { paths: &paths, spectra, h0: &h0, r_ref, directivity: &directivity, surfaces };
                    let mut out = vec![Complex32::new(0.0, 0.0); len];
                    let mut st = RunStats::default();
                    crate::prof::time("accumulate diffraction", || accumulate_diffraction(&dpaths, &graph.edges, &job, backend, &mut out, &mut st));
                    (out, st)
                })));
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(SolverError::Cancelled);
        }

        if let Some(job) = ray_job {
            geo(base + 0.9 / ns, &format!("{}: ray tracing", src.instance.name));
            let out = job.join().expect("ray tracing thread").ok_or(SolverError::Cancelled)?;
            lates.push(receivers.iter().map(|p| out.late.at(*p)).collect());
            let mut echo = out.echograms;
            if let Some(early) = early_echo {
                for (e, x) in echo.iter_mut().zip(early) {
                    for (band, xb) in e.bands.iter_mut().zip(x.bands) {
                        for (v, w) in band.iter_mut().zip(xb) {
                            *v += w;
                        }
                    }
                }
            }
            echos.push(echo);
        }
        Ok(())
      })?;
    }
    if let Some((i, h)) = pending.take() {
        finish_diffraction(i, h, &mut transfers, &mut stats);
    }
    Ok(())
    })();
    if geometry.is_err() {
        bg_cancel.store(true, Ordering::Relaxed);
    }
    geometry?;
    geo(1.0, "waiting for the wave solver");
    if let Some(job) = wave_job {
        while !job.is_finished() {
            if cancel.load(Ordering::Relaxed) {
                bg_cancel.store(true, Ordering::Relaxed);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let (ts, st) = job.join().expect("wave solver thread")?;
        stats.notes.extend(st.notes);
        if let Some((fs, _grid, fis, nodes, ..)) = &fdtd {
            for (si, t) in ts.iter().enumerate() {
            let out = &mut transfers[si];
            let mut k = 0;
            for (ri, node) in nodes.iter().enumerate() {
                if node.is_none() {
                    continue;
                }
                for (j, &fi) in fis.iter().enumerate() {
                    let w = fs.weight(freqs[fi]);
                    let ism = out[ri * nf + fi];
                    let ism = num_complex::Complex64::new(ism.re as f64, ism.im as f64);
                    let wave = t[k * fis.len() + j];
                    let v = crate::fdtd::blend(ism, wave, w);
                    out[ri * nf + fi] = Complex32::new(v.re as f32, v.im as f32);
                }
                k += 1;
            }
            }
        }
    }
    Ok(())
    })?;

    // Vertical coherence of the late field, per source and receiver height
    // (and, outside extruded rooms, the receiver's floor–ceiling column).
    let columns: Vec<Option<crate::vertical::Column>> =
        receivers.iter().map(|p| if input.room.prism { None } else { crate::vertical::column(&input.room, *p) }).collect();
    let mut groups: Vec<(Option<crate::vertical::Column>, f64)> = Vec::new();
    let group_of: Vec<u32> = receivers
        .iter()
        .zip(&columns)
        .zip(&height_step)
        .map(|((p, col), step)| {
            let key = (*col, if *step == 0.001 { (p.z * 1000.0).round() / 1000.0 } else { (p.z / step).round() * step });
            match groups.iter().position(|g| *g == key) {
                Some(i) => i as u32,
                None => {
                    groups.push(key);
                    (groups.len() - 1) as u32
                }
            }
        })
        .collect();
    let t_vert = std::time::Instant::now();
    let late_gain: Vec<Vec<Vec<f32>>> = if late && !lates.is_empty() {
        progress(0.99, "late field: vertical coherence");
        let geo = crate::vertical::Horizontal::new(&input.room);
        let c = input.medium.speed_of_sound();
        // Horizontal energy e-folding length: 2D mean free path πA/P over
        // the wall absorption exponent, plus air absorption.
        let walls: Vec<(usize, f64)> = input
            .room
            .surfaces
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind == wavey_geom::SurfaceKind::Wall)
            .map(|(i, s)| (i, s.area))
            .collect();
        let wall_area: f64 = walls.iter().map(|w| w.1).sum();
        let mfp_2d = if input.room.prism {
            PI * input.room.floor_area() / (wall_area / input.room.height).max(1e-9)
        } else {
            // Mean height V / A for the perimeter.
            PI * input.room.volume() / wall_area.max(1e-9)
        };
        let decay = |f: f64| {
            let fi = (0..nf).min_by(|&a, &b| (freqs[a] - f).abs().total_cmp(&(freqs[b] - f).abs())).unwrap_or(0);
            let alpha = walls.iter().map(|&(i, a)| surfaces[i].absorption[fi] * a).sum::<f64>() / wall_area.max(1e-9);
            let air = 2.0 * spectra.air_np[fi];
            1.0 / ((-(1.0 - alpha.min(0.999)).ln()) / mfp_2d + air).max(1e-6)
        };
        let points = crate::vertical::eval_points(&freqs, decay);
        // Floor and ceiling as impedances over the whole band: the vertical
        // interference (including the grazing-incidence cancellation over
        // absorptive floors) is wave physics at every frequency.
        let horizontal: Vec<usize> = input
            .room
            .surfaces
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind != wavey_geom::SurfaceKind::Wall)
            .map(|(i, _)| i)
            .collect();
        let mut vsurfaces = surfaces.clone();
        if !s.impedance_reflection {
            let band = (20f64.min(s.f_min), s.f_max.max(40.0));
            let hfits = crate::fdtd::fit_surfaces(&horizontal.iter().map(|&i| input.surface_materials[i].clone()).collect::<Vec<_>>(), band);
            for (&i, fit) in horizontal.iter().zip(&hfits) {
                for (fi, f) in freqs.iter().enumerate() {
                    if vsurfaces[i].zeta[fi].is_none() {
                        vsurfaces[i].zeta[fi] = Some(fit.impedance(*f));
                    }
                }
            }
        }
        let out: Vec<Vec<Vec<f32>>> = input
            .sources
            .iter()
            .map(|src| {
                let ac = src.instance.acoustic_center(&src.model);
                let zs = ac.z;
                let src_col = if input.room.prism { None } else { crate::vertical::column(&input.room, ac) };
                groups
                    .iter()
                    .map(|&(col, zr)| {
                        if input.room.prism {
                            let g = crate::vertical::coherence_gain(&geo, &vsurfaces, &points, c, zs, zr);
                            return crate::vertical::to_grid(&points, &g, &freqs);
                        }
                        // The source must stand in the same column.
                        match col.filter(|c| src_col == Some(*c)) {
                            Some(col) => {
                                let local = crate::vertical::Horizontal::local(&col);
                                let g = crate::vertical::coherence_gain(&local, &vsurfaces, &points, c, zs - col.z_floor, zr - col.z_floor);
                                crate::vertical::to_grid(&points, &g, &freqs)
                            }
                            None => vec![1.0; freqs.len()],
                        }
                    })
                    .collect()
            })
            .collect();
        let (lo, hi) = out.iter().flatten().flatten().fold((f32::MAX, f32::MIN), |(a, b), g| (a.min(*g), b.max(*g)));
        stats.notes.push(format!(
            "late field: vertical floor–ceiling coherence applied ({:+.1} to {:+.1} dB across sources, heights and frequencies)",
            10.0 * lo.max(1e-6).log10(),
            10.0 * hi.max(1e-6).log10()
        ));
        out
    } else {
        Vec::new()
    };

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
            late_gain: late_gain.iter().map(|g| g[group_of[i] as usize].clone()).collect(),
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
            late_group: group_of[first..first + n].to_vec(),
            late_gain: late_gain.clone(),
            grid: g,
        });
        first += n;
    }

    stats.seconds = start.elapsed().as_secs_f64();
    progress(1.0, "done");
    crate::prof::add("vertical coherence and assembly", t_vert.elapsed().as_secs_f64());
    crate::prof::report();
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
