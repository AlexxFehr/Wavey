//! Headless Wavey: inspect and simulate `.wavey` projects without the app.

mod build;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use wavey_geom::{DVec2, DVec3, SurfaceKey, SurfaceKind};
use std::sync::atomic::AtomicBool;

use wavey_project::Document;
use wavey_solver::{BackendChoice, CutPlane, Microphone, PlaneAxis};

#[derive(Parser)]
#[command(name = "wavey", version = wavey_geom::CORE_VERSION, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Quiet mode: limit the GPU to this busy fraction (e.g. 0.25) and use
    /// few CPU threads, to keep the machine cool. Combine with
    /// `taskpolicy -c background wavey …` to stay on the efficiency cores.
    #[arg(long, global = true)]
    duty: Option<f64>,
    /// Number of CPU worker threads.
    #[arg(long, global = true)]
    threads: Option<usize>,
}

#[derive(Subcommand)]
enum Command {
    /// Write a demo project: a 20 × 14 m hall with two pillars.
    Demo {
        path: PathBuf,
        /// Stereo PA: this top (.waveyspk) stacked on `--sub` at each side of
        /// the stage, instead of two small 2-way speakers.
        #[arg(long, requires = "sub")]
        top: Option<PathBuf>,
        /// Subwoofer model (.waveyspk) for the PA.
        #[arg(long, requires = "top")]
        sub: Option<PathBuf>,
        /// Further models to include in the project (not placed).
        #[arg(long)]
        extra: Vec<PathBuf>,
    },
    /// Create a project from a JSON room description (see build.rs).
    Build { spec: PathBuf, out: PathBuf },
    /// Band levels at a microphone from the stored results, with current
    /// speaker settings: total, and per speaker group.
    Levels {
        path: PathBuf,
        /// Microphone name.
        #[arg(long)]
        mic: String,
        /// Frequency bands as LO:HI in Hz (energy average over the band).
        #[arg(long = "band", num_args = 1.., default_values_t = ["20:60".to_string(), "60:120".to_string(), "1000:10000".to_string()])]
        bands: Vec<String>,
        /// Speaker groups by name substring (e.g. "top", "sub").
        #[arg(long = "group")]
        groups: Vec<String>,
    },
    /// Diagnostic for convex rooms: incoherent image-source energy on the
    /// first cut plane to high order (all images are valid in a convex
    /// room), with (a) the wave solver's fitted impedances (angle-dependent)
    /// and (b) angle-independent random-incidence absorption, per 1/3 octave.
    DiagEnergy {
        path: PathBuf,
        #[arg(long, default_value_t = 40)]
        order: u32,
        #[arg(long, default_value_t = 250.0)]
        from: f64,
        #[arg(long, default_value_t = 500.0)]
        to: f64,
    },
    /// Statistics of the first cut plane per 1/N-octave band (dB over all
    /// points): mean, standard deviation and 10th/90th percentiles.
    PlaneStats {
        path: PathBuf,
        #[arg(long, default_value_t = 3.0)]
        fraction: f64,
        #[arg(long, default_value_t = 50.0)]
        from: f64,
        #[arg(long, default_value_t = 1000.0)]
        to: f64,
        /// Use single frequencies instead of band averages.
        #[arg(long)]
        single: bool,
    },
    /// Compare two results of the same room and receivers (e.g. hybrid vs a
    /// pure wave-solver reference): per 1/N-octave band, plane means and the
    /// point-by-point difference, plus microphone differences.
    Compare {
        test: PathBuf,
        reference: PathBuf,
        #[arg(long, default_value_t = 3.0)]
        fraction: f64,
        #[arg(long, default_value_t = 100.0)]
        from: f64,
        #[arg(long, default_value_t = 500.0)]
        to: f64,
    },
    /// Change speakers whose name contains MATCH; results are kept (speaker
    /// processing is applied when mixing).
    Set {
        path: PathBuf,
        #[arg(long = "match")]
        pattern: String,
        #[arg(long)]
        gain_db: Option<f64>,
        #[arg(long)]
        delay_ms: Option<f64>,
        /// Replace the filter list with this JSON array.
        #[arg(long)]
        filters: Option<String>,
    },
    /// Write the built-in sample speakers as .waveyspk files.
    ExportSpeakers { dir: PathBuf },
    /// Print a summary of a project.
    Info { path: PathBuf },
    /// List the GPUs the solver can use (any wgpu backend). Choose one with
    /// WAVEY_GPU=<name part>, a backend with WAVEY_GPU_BACKEND=metal|vulkan|dx12|gl.
    Gpus,
    /// Run the simulation and store results in the project.
    Simulate {
        path: PathBuf,
        /// Compute backend.
        #[arg(long, value_enum, default_value_t = Backend::Auto)]
        backend: Backend,
        /// Override the reflection order.
        #[arg(long)]
        order: Option<u32>,
        /// Run without writing results back.
        #[arg(long)]
        dry_run: bool,
        /// Enable the wave solver with this crossover frequency (Hz).
        #[arg(long)]
        fdtd: Option<f64>,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Backend {
    Auto,
    Cpu,
    Gpu,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Some(d) = cli.duty {
        wavey_solver::set_duty_cycle(d);
    }
    let threads = cli.threads.or(cli.duty.map(|_| 2));
    if let Some(n) = threads {
        let _ = rayon::ThreadPoolBuilder::new().num_threads(n.max(1)).build_global();
    }
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Build { spec, out } => build::build(&spec, &out)?,
        Command::Levels { path, mic, bands, groups } => {
            let doc = Document::open(&path)?;
            let r = doc.results().ok_or("project has no results; run `wavey simulate` first")?;
            if doc.results_stale() {
                println!("note: results are out of date with the project");
            }
            let mi = r.mics.iter().position(|m| m.mic.name == mic).ok_or(format!("no microphone `{mic}` in the results"))?;
            let p = doc.project();
            let names: Vec<String> = r.speaker_ids.iter().map(|id| p.speakers.get(id).map_or(String::new(), |s| s.name.clone())).collect();
            let all = p.drives(&r.speaker_ids, &r.freqs);
            let mut sets: Vec<(String, Vec<Vec<num_complex::Complex64>>)> = vec![("all".into(), all.clone())];
            for g in &groups {
                let d = all
                    .iter()
                    .zip(&names)
                    .map(|(d, n)| if n.contains(g.as_str()) { d.clone() } else { vec![num_complex::Complex64::new(0.0, 0.0); d.len()] })
                    .collect();
                sets.push((format!("\"{g}\""), d));
            }
            print!("{:<14}", "band (Hz)");
            for (n, _) in &sets {
                print!("{n:>12}");
            }
            println!();
            for b in &bands {
                let (lo, hi) = b.split_once(':').ok_or("bands look like LO:HI")?;
                let (lo, hi): (f64, f64) = (lo.parse()?, hi.parse()?);
                print!("{b:<14}");
                for (_, d) in &sets {
                    let pr = r.mic_pressure(mi, d);
                    let sel: Vec<f64> = r.freqs.iter().zip(&pr).filter(|(f, _)| **f >= lo && **f <= hi).map(|(_, v)| v.norm_sqr()).collect();
                    let db = wavey_solver::result::spl(sel.iter().sum::<f64>() / sel.len().max(1) as f64);
                    print!("{db:>9.1} dB");
                }
                println!();
            }
        }
        Command::Compare { test, reference, fraction, from, to } => {
            let (da, db) = (Document::open(&test)?, Document::open(&reference)?);
            let (ra, rb) = (da.results().ok_or("test has no results")?, db.results().ok_or("reference has no results")?);
            let (pa, pb) = (&ra.planes[0], &rb.planes[0]);
            if pa.grid.inside != pb.grid.inside {
                return Err("the two cut planes differ".into());
            }
            let (dra, drb) = (da.project().drives(&ra.speaker_ids, &ra.freqs), db.project().drives(&rb.speaker_ids, &rb.freqs));
            println!("{:>7} | {:>7} {:>7} | {:>9} {:>9} {:>6} | mics (test − ref)", "f", "test", "ref", "mean diff", "rms diff", "|d|>3");
            let n = (fraction * (to / from).log2()).round() as i32;
            for i in 0..=n {
                let f = 1000.0 * 2f64.powf(((fraction * (from / 1000.0).log2()).round() + i as f64) / fraction);
                if f > to * 1.01 {
                    break;
                }
                let mode = wavey_solver::PlaneMode::Band { f, fraction };
                let (a, b) = (ra.plane_spl(0, &dra, mode), rb.plane_spl(0, &drb, mode));
                let d: Vec<(f64, f64, f64)> = a
                    .iter()
                    .zip(&b)
                    .filter(|(x, y)| x.is_finite() && y.is_finite())
                    .map(|(x, y)| (*x as f64, *y as f64, (*x - *y) as f64))
                    .collect();
                let nd = d.len() as f64;
                let (ma, mb) = (d.iter().map(|v| v.0).sum::<f64>() / nd, d.iter().map(|v| v.1).sum::<f64>() / nd);
                let md = d.iter().map(|v| v.2).sum::<f64>() / nd;
                let rms = (d.iter().map(|v| v.2 * v.2).sum::<f64>() / nd).sqrt();
                let big = d.iter().filter(|v| v.2.abs() > 3.0).count() as f64 / nd * 100.0;
                let mut mics = String::new();
                for (mi, m) in ra.mics.iter().enumerate() {
                    let band = |r: &wavey_solver::SimulationResult, dr: &[Vec<num_complex::Complex64>], idx: usize| {
                        let (lo, hi) = wavey_solver::freq::band_edges(f, fraction);
                        let p = r.mic_pressure(idx, dr);
                        let sel: Vec<f64> = r.freqs.iter().zip(&p).filter(|(q, _)| **q >= lo && **q < hi).map(|(_, v)| v.norm_sqr()).collect();
                        wavey_solver::result::spl(sel.iter().sum::<f64>() / sel.len().max(1) as f64)
                    };
                    let mj = rb.mics.iter().position(|x| x.mic.name == m.mic.name).unwrap_or(mi);
                    mics += &format!(" {:+5.1}", band(ra, &dra, mi) - band(rb, &drb, mj));
                }
                let emean = |i: usize| 10.0 * (d.iter().map(|v| 10f64.powf([v.0, v.1][i] / 10.0)).sum::<f64>() / nd).log10();
                println!("{f:>7.0} | {ma:>7.1} {mb:>7.1} | {md:>+9.2} {rms:>9.2} {big:>5.0}% |{mics}   energy means {:.2} / {:.2} ({:+.2})", emean(0), emean(1), emean(0) - emean(1));
            }
        }
        Command::DiagEnergy { path, order, from, to } => {
            let mut doc = Document::open(&path)?;
            let room = doc.room().map_err(|e| format!("room: {e}"))?.clone();
            let input = doc.project().simulation_input(&room);
            let results = doc.results().cloned();
            let src = &input.sources[0];
            let centre = src.instance.acoustic_center(&src.model);
            // Shoebox image lattice: per axis, image coordinate 2nL ± s with
            // reflection counts on the low and high wall.
            let (lo, hi) = room.bounds();
            if room.surfaces.len() != 6 {
                return Err("diag-energy needs a shoebox room".into());
            }
            let wall = |axis: usize, low: bool| {
                room.surfaces
                    .iter()
                    .position(|s| s.normal[axis] > 0.99 && low || s.normal[axis] < -0.99 && !low)
                    .unwrap()
            };
            let axis_images = |axis: usize| {
                let (l, s0) = (hi[axis] - lo[axis], centre[axis] - lo[axis]);
                let mut v = Vec::new();
                let nmax = order as i64 / 2 + 1;
                for n in -nmax..=nmax {
                    for sign in [1.0, -1.0] {
                        let pos = lo[axis] + 2.0 * n as f64 * l + sign * s0;
                        let total = if sign > 0.0 { (2 * n).unsigned_abs() } else { (2 * n - 1).unsigned_abs() };
                        let low_hits = if sign > 0.0 { n.unsigned_abs() } else if n <= 0 { total.div_ceil(2) } else { total / 2 };
                        v.push((pos, low_hits as u32, (total - low_hits) as u32));
                    }
                }
                v
            };
            let (ax, ay, az) = (axis_images(0), axis_images(1), axis_images(2));
            let mut images: Vec<(glam::DVec3, [u32; 6])> = Vec::new();
            let idx = [wall(0, true), wall(0, false), wall(1, true), wall(1, false), wall(2, true), wall(2, false)];
            for x in &ax {
                for y in &ay {
                    for z in &az {
                        let hits = [x.1, x.2, y.1, y.2, z.1, z.2];
                        if hits.iter().sum::<u32>() <= order {
                            let mut c = [0u32; 6];
                            for k in 0..6 {
                                c[idx[k]] += hits[k];
                            }
                            images.push((glam::DVec3::new(x.0, y.0, z.0), c));
                        }
                    }
                }
            }
            let all_pts = input.planes[0].grid(&room)?.inside_points();
            let pts: Vec<_> = all_pts.iter().step_by(5).copied().collect();
            let fs_max = input.settings.fdtd.as_ref().map_or(1000.0, |f| f.f_max()).max(to * 1.2);
            let fits = wavey_solver::fdtd::fit_surfaces(&input.surface_materials, (20.0, fs_max));
            println!("{} images (order ≤ {order}), {} plane points", images.len(), pts.len());
            let _ = &all_pts;
            println!("{:>7} | {:>12} {:>12} {:>12} {:>12} | energy mean of plane, dB", "f", "impedance", "α only", "coherent", "stored");
            let n = (3.0 * (to / from).log2()).round() as i32;
            for i in 0..=n {
                let fc = 1000.0 * 2f64.powf(((3.0 * (from / 1000.0).log2()).round() + i as f64) / 3.0);
                let (lo, hi) = wavey_solver::freq::band_edges(fc, 3.0);
                // The stored result's frequencies in the band, if any.
                let stored: Vec<f64> = results.as_ref().map_or(Vec::new(), |r| r.freqs.iter().copied().filter(|f| *f >= lo && *f < hi).collect());
                let freqs: Vec<f64> = if stored.is_empty() { (0..12).map(|k| lo * (hi / lo).powf((k as f64 + 0.5) / 12.0)).collect() } else { stored };
                let spectra = wavey_solver::scene::Spectra::new(freqs.clone(), &input.medium);
                let h0 = wavey_solver::scene::source_spectrum(&src.model, &freqs);
                let r_ref = src.model.reference.distance_m;
                let ns = room.surfaces.len();
                // Per surface and frequency: impedance and α.
                let zeta: Vec<Vec<num_complex::Complex64>> = (0..ns).map(|s| freqs.iter().map(|f| fits[s].impedance(*f)).collect()).collect();
                let alpha: Vec<Vec<f64>> = (0..ns).map(|s| freqs.iter().map(|f| input.surface_materials[s].absorption_at(*f)).collect()).collect();
                use rayon::prelude::*;
                // Per point and frequency: incoherent sums (impedance, α) and
                // the coherent sum with spherical-wave reflection factors.
                let (e_imp, e_alpha, e_coh) = pts
                    .par_iter()
                    .map(|x| {
                        let (mut ei, mut ea) = (0.0f64, 0.0f64);
                        let mut coh = vec![num_complex::Complex64::new(0.0, 0.0); freqs.len()];
                        for (ipos, cnt) in &images {
                            let d = *x - *ipos;
                            let r = d.length().max(0.05);
                            for fi in 0..freqs.len() {
                                let k = spectra.k[fi];
                                let amp = h0[fi].norm() * r_ref / r * (-spectra.air_np[fi] * r).exp();
                                let (mut gi, mut ga) = (1.0, 1.0);
                                let mut q = num_complex::Complex64::new(1.0, 0.0);
                                for s in 0..ns {
                                    if cnt[s] == 0 {
                                        continue;
                                    }
                                    let cos = (d.dot(room.surfaces[s].normal) / r).abs();
                                    gi *= wavey_solver::reflection::plane(zeta[s][fi], cos).norm_sqr().powi(cnt[s] as i32);
                                    ga *= (1.0 - alpha[s][fi]).powi(cnt[s] as i32);
                                    q *= wavey_solver::reflection::spherical(zeta[s][fi], cos, k, r).powi(cnt[s] as i32);
                                }
                                ei += amp * amp * gi;
                                ea += amp * amp * ga;
                                coh[fi] += q * num_complex::Complex64::from_polar(amp, -k * r);
                            }
                        }
                        (ei, ea, coh.iter().map(|v| v.norm_sqr()).sum::<f64>())
                    })
                    .reduce(|| (0.0, 0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2));
                let norm = (pts.len() * freqs.len()) as f64;
                let db = |e: f64| 10.0 * (e / norm / 4e-10).log10();
                // The stored (wave-solver) result at the same points and frequencies.
                let stored_db = results.as_ref().map(|r| {
                    let drives = doc.project().drives(&r.speaker_ids, &r.freqs);
                    let mut e = 0.0;
                    for f in &freqs {
                        let v = r.plane_spl(0, &drives, wavey_solver::PlaneMode::Single { f: *f });
                        e += (0..pts.len()).map(|i| 10f64.powf(v[i * 5] as f64 / 10.0) * 4e-10).sum::<f64>();
                    }
                    db(e)
                });
                println!("{fc:>7.0} | {:>12.2} {:>12.2} {:>12.2} {:>12.2}", db(e_imp), db(e_alpha), db(e_coh), stored_db.unwrap_or(f64::NAN));
            }
        }
        Command::PlaneStats { path, fraction, from, to, single } => {
            let doc = Document::open(&path)?;
            let r = doc.results().ok_or("project has no results")?;
            let drives = doc.project().drives(&r.speaker_ids, &r.freqs);
            if let Some(fd) = &r.fdtd {
                println!("wave solver below {:.0} Hz (blend {:.0}–{:.0} Hz)", fd.crossover_hz, fd.crossover_hz / 2f64.sqrt(), fd.f_max());
            }
            println!("{:>8}  {:>7} {:>6} {:>7} {:>7}", "f (Hz)", "mean", "std", "p10", "p90");
            let n = (fraction * (to / from).log2()).round() as i32;
            for i in 0..=n {
                let f = 1000.0 * 2f64.powf(((fraction * (from / 1000.0).log2()).round() + i as f64) / fraction);
                if f > to * 1.01 {
                    break;
                }
                let mode = if single {
                    wavey_solver::PlaneMode::Single { f }
                } else {
                    wavey_solver::PlaneMode::Band { f, fraction }
                };
                let mut v: Vec<f64> = r.plane_spl(0, &drives, mode).into_iter().filter(|x| x.is_finite()).map(|x| x as f64).collect();
                v.sort_by(f64::total_cmp);
                let mean = v.iter().sum::<f64>() / v.len() as f64;
                let std = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
                let pct = |q: f64| v[((v.len() - 1) as f64 * q) as usize];
                let w = r.fdtd.as_ref().map_or(0.0, |fd| fd.weight(f));
                println!("{f:>8.0}  {mean:>7.1} {std:>6.1} {:>7.1} {:>7.1}   wave weight {w:.2}", pct(0.1), pct(0.9));
            }
        }
        Command::Set { path, pattern, gain_db, delay_ms, filters } => {
            let mut doc = Document::open(&path)?;
            let filters: Option<Vec<wavey_speaker::Filter>> = filters.map(|f| serde_json::from_str(&f)).transpose()?;
            let n = doc.try_edit(|p| -> Result<usize, String> {
                let ids: Vec<u64> = p.speakers.values().filter(|s| s.name.contains(pattern.as_str())).map(|s| s.id).collect();
                for id in &ids {
                    let mut s = p.speakers[id].clone();
                    if let Some(g) = gain_db {
                        s.gain_db = g;
                    }
                    if let Some(d) = delay_ms {
                        s.delay_ms = d;
                    }
                    if let Some(f) = &filters {
                        s.filters = f.clone();
                    }
                    p.update_speaker(s)?;
                }
                Ok(ids.len())
            })?;
            doc.save(&path)?;
            println!("updated {n} speakers");
        }
        Command::ExportSpeakers { dir } => {
            std::fs::create_dir_all(&dir)?;
            for m in wavey_speaker::samples::all() {
                let file = dir.join(format!("{}.waveyspk", m.id));
                m.save(&file)?;
                println!("wrote {}", file.display());
            }
        }
        Command::Demo { path, top, sub, extra } => {
            let load = |p: &PathBuf| wavey_speaker::SpeakerModel::load(p).map_err(|e| format!("{}: {e}", p.display()));
            let pa = match (top, sub) {
                (Some(t), Some(s)) => Some((load(&t)?, load(&s)?)),
                _ => None,
            };
            let extra = extra.iter().map(load).collect::<Result<Vec<_>, _>>()?;
            let mut doc = demo_document(pa, extra);
            doc.save(&path)?;
            println!("wrote {}", path.display());
        }
        Command::Gpus => {
            let list = wavey_solver::gpu::adapters();
            if list.is_empty() {
                println!("no GPU adapters found");
            }
            for a in list {
                println!("{a}");
            }
            match wavey_solver::gpu::gpu() {
                Some(g) => println!("selected: {}", g.name),
                None => println!("selected: none (CPU)"),
            }
        }
        Command::Info { path } => {
            let mut doc = Document::open(&path)?;
            let p = doc.project();
            println!("segments: {}", p.sketch.segments().len());
            println!("ceiling height: {} m", p.ceiling_height);
            match doc.room() {
                Ok(room) => {
                    println!("volume: {:.1} m³", room.volume());
                    println!("surfaces: {}", room.surfaces.len());
                    let g = wavey_solver::diffraction::EdgeGraph::build(room);
                    let pairs: usize = g.pairs.iter().map(Vec::len).sum();
                    println!("diffracting edges: {} ({} visible edge pairs)", g.edges.len(), pairs);
                }
                Err(e) => println!("room: {e}"),
            }
        }
        Command::Simulate { path, backend, order, dry_run, fdtd } => {
            let mut doc = Document::open(&path)?;
            doc.edit(|p| {
                p.sim_settings.backend = match backend {
                    Backend::Auto => BackendChoice::Auto,
                    Backend::Cpu => BackendChoice::Cpu,
                    Backend::Gpu => BackendChoice::Gpu,
                };
                if let Some(o) = order {
                    p.sim_settings.max_order = o;
                }
                if let Some(fc) = fdtd {
                    p.sim_settings.fdtd = Some(wavey_solver::fdtd::FdtdSettings { crossover_hz: fc, ..Default::default() });
                }
            });
            let room = doc.room()?.clone();
            let input = doc.project().simulation_input(&room);
            let est = wavey_solver::estimate(&input)?;
            println!(
                "{} sources, {} receivers, {} frequencies, {} images/source",
                input.sources.len(),
                est.receivers,
                est.frequencies,
                est.images_per_source
            );
            let last = std::sync::Mutex::new(String::new());
            let progress = |f: f64, stage: &str| {
                let mut l = last.lock().unwrap();
                if *l != stage {
                    eprintln!("[{:5.1}%] {stage}", f * 100.0);
                    *l = stage.to_string();
                }
            };
            let mut result = wavey_solver::simulate(&input, &progress, &AtomicBool::new(false))?;
            result.fingerprint = doc.project().simulation_fingerprint();
            let st = &result.stats;
            println!("backend: {}  time: {:.2} s  paths: {}  diffracted: {}", st.backend, st.seconds, st.paths, st.diffracted_paths);
            for n in &st.notes {
                println!("note: {n}");
            }
            let drives = doc.project().drives(&result.speaker_ids, &result.freqs);
            for (i, m) in result.mics.iter().enumerate() {
                let p = result.mic_pressure(i, &drives);
                let mean = p.iter().map(|v| v.norm_sqr()).sum::<f64>() / p.len() as f64;
                println!("{}: {:.1} dB SPL (mean over band)", m.mic.name, wavey_solver::result::spl(mean));
            }
            if !dry_run {
                doc.set_results(result);
                doc.save(&path)?;
                println!("results saved to {}", path.display());
            }
        }
    }
    Ok(())
}

pub fn demo_document(pa: Option<(wavey_speaker::SpeakerModel, wavey_speaker::SpeakerModel)>, extra: Vec<wavey_speaker::SpeakerModel>) -> Document {
    let v = DVec2::new;
    let mut doc = Document::new();
    doc.edit(|p| {
        p.ceiling_height = 6.0;
        p.sketch.add_polygon(&[v(0.0, 0.0), v(20.0, 0.0), v(20.0, 14.0), v(0.0, 14.0)]);
        for cx in [7.0, 13.0] {
            p.sketch.add_polygon(&[v(cx - 0.3, 6.7), v(cx + 0.3, 6.7), v(cx + 0.3, 7.3), v(cx - 0.3, 7.3)]);
        }
    });
    doc.toggle_interior_at(v(1.0, 1.0));
    // Floor, ceiling and the back wall get distinct materials.
    let keys: Vec<(SurfaceKey, &str)> = {
        let room = doc.room().expect("demo room");
        room.surfaces
            .iter()
            .filter_map(|s| {
                let mat = match s.kind {
                    SurfaceKind::Floor => "wood-floor",
                    SurfaceKind::Ceiling => "acoustic-tile",
                    SurfaceKind::Wall if s.normal.y < -0.9 && s.d < -13.0 => "curtain-heavy",
                    _ => return None,
                };
                Some((s.key, mat))
            })
            .collect()
    };
    for (key, mat) in keys {
        doc.try_edit(|p| p.assign(&[key], mat)).expect("builtin material");
    }
    if let Some((top, sub)) = pa {
        // Stereo PA: a sub standing on the floor at each side of the stage,
        // with the top standing on it, angled slightly toward the centre.
        // The sub's level is matched to the top's at 100 Hz.
        doc.try_edit(|p| {
            let sub_height = {
                let s = wavey_speaker::SpeakerInstance { yaw_deg: 0.0, ..Default::default() };
                2.0 * s.depth_below_center(&sub)
            };
            let sub_gain = top.sensitivity_db(100.0) - sub.sensitivity_db(100.0);
            let top_id = p.add_speaker_model(top)?;
            let sub_id = p.add_speaker_model(sub)?;
            for m in extra {
                p.add_speaker_model(m)?;
            }
            for (y, yaw, side) in [(2.0, 8.0, "Right"), (12.0, -8.0, "Left")] {
                let id = p.place_speaker(&sub_id, DVec3::new(1.0, y, 0.0), true)?;
                let mut s = p.speakers[&id].clone();
                s.name = format!("{side} sub");
                s.gain_db = (sub_gain * 10.0).round() / 10.0;
                p.update_speaker(s)?;
                let id = p.place_speaker(&top_id, DVec3::new(1.0, y, 0.0), true)?;
                let mut s = p.speakers[&id].clone();
                s.name = format!("{side} top");
                s.yaw_deg = yaw;
                s.bottom_height_m = sub_height;
                p.update_speaker(s)?;
            }
            Ok::<_, String>(())
        })
        .expect("demo PA");
    } else {
        // A stereo pair of 2-way speakers on the short wall, angled in and down.
        doc.try_edit(|p| {
            let model = p.add_speaker_model(wavey_speaker::samples::two_way())?;
            for (y, yaw) in [(3.5, 12.0), (10.5, -12.0)] {
                let id = p.add_speaker(&model, DVec3::new(1.0, y, 3.5))?;
                let s = p.speakers.get_mut(&id).unwrap();
                s.yaw_deg = yaw;
                s.pitch_deg = -10.0;
                s.name = if y < 7.0 { "Right".into() } else { "Left".into() };
            }
            Ok::<_, String>(())
        })
        .expect("demo speakers");
    }
    doc.edit(|p| {
        for (name, x, y) in [("Front row", 6.0, 7.0), ("Middle", 11.0, 5.0), ("Back", 17.5, 9.0)] {
            let id = p.alloc_id();
            p.microphones.insert(id, Microphone { id, name: name.into(), position: DVec3::new(x, y, 1.2) });
        }
        let id = p.alloc_id();
        p.cut_planes.insert(
            id,
            CutPlane { id, name: "Ear height".into(), axis: PlaneAxis::Horizontal, offset: 1.2, spacing: 0.2, drape: None },
        );
    });
    doc
}
