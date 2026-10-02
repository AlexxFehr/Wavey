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
    let mut surfaces: Vec<SurfaceAcoustics> =
        input.surface_materials.iter().map(|m| SurfaceAcoustics::new(m, &freqs)).collect();
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
    stats.notes.extend(impedance_note);
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
            let fits = fits.as_ref().unwrap();
            let grid = crate::fdtd::FdtdGrid::build(&input.room, fits, c, fs.f_max(), fs.points_per_wavelength)
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
            Some((fs.clone(), grid, fis, nodes, steps))
        }
        None => None,
    };
    let edge_graph = s.diffraction.then(|| crate::diffraction::EdgeGraph::build(&input.room));
    if let Some(g) = &edge_graph {
        stats.notes.push(format!(
            "diffraction: {} wall/pillar edges (UTD; direct sound and first reflections once, direct sound edge to edge)",
            g.edges.len()
        ));
    }
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
        if let Some(graph) = &edge_graph {
            progress(base + 0.82 / ns, &format!("{}: diffraction", src.instance.name));
            let dpaths = crate::diffraction::trace_diffraction(
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
            )
            .ok_or(SolverError::Cancelled)?;
            stats.diffracted_paths += dpaths.iter().map(Vec::len).sum::<usize>();
            crate::diffraction::accumulate(&dpaths, &graph.edges, &job, transfers.last_mut().unwrap());
        }
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
            let h0: Vec<_> = fis.iter().map(|&i| source_spectrum(&src.model, &[freqs[i]])[0]).collect();
            // Directional source: monopole + dipoles fitted to the balloon.
            let order = if src.model.balloon.is_some() { fs.source_order.min(1) } else { 0 };
            let comps = crate::fdtd::source_components(grid, src_node, order).unwrap_or_else(|| {
                stats.notes.push(format!("wave solver: `{name}` is too close to a surface for a directional source; used omnidirectional"));
                vec![vec![(src_node, 1.0)]]
            });
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
                    fs.f_max()
                ));
                w
            };
            let mut t = vec![num_complex::Complex64::new(0.0, 0.0); rx.len() * sub_freqs.len()];
            for (ci, taps) in comps.iter().enumerate() {
                let fprog = |f: f64| {
                    progress(base + (0.86 + 0.04 * (ci as f64 + f) / comps.len() as f64) / ns, &format!("{name}: wave solver {}/{}", ci + 1, comps.len()))
                };
                let spectra = run_fdtd(grid, taps, &signal, &rx, &sub_freqs, input.settings.backend, cancel, &fprog, &mut stats)
                    .ok_or(SolverError::Cancelled)?;
                let tc = crate::fdtd::to_transfer(&spectra, &signal, grid, &sub_freqs, c, src.model.reference.distance_m, &h0);
                for (k, v) in tc.iter().enumerate() {
                    t[k] += weights[k % sub_freqs.len()][ci] * v;
                }
            }
            let out = transfers.last_mut().unwrap();
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

    // Vertical coherence of the late field, per source and receiver height.
    let mut groups: Vec<f64> = Vec::new();
    let group_of: Vec<u32> = receivers
        .iter()
        .map(|p| {
            let z = (p.z * 1000.0).round() / 1000.0;
            match groups.iter().position(|g| *g == z) {
                Some(i) => i as u32,
                None => {
                    groups.push(z);
                    (groups.len() - 1) as u32
                }
            }
        })
        .collect();
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
        let mfp_2d = PI * input.room.floor_area() / (wall_area / input.room.height).max(1e-9);
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
                let zs = src.instance.acoustic_center(&src.model).z;
                groups
                    .iter()
                    .map(|&zr| {
                        let g = crate::vertical::coherence_gain(&geo, &vsurfaces, &points, c, zs, zr);
                        crate::vertical::to_grid(&points, &g, &freqs)
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
