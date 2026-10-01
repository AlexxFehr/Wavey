//! Headless Wavey: inspect and simulate `.wavey` projects without the app.

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
    /// Write the built-in sample speakers as .waveyspk files.
    ExportSpeakers { dir: PathBuf },
    /// Print a summary of a project.
    Info { path: PathBuf },
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
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
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
        Command::Info { path } => {
            let mut doc = Document::open(&path)?;
            let p = doc.project();
            println!("segments: {}", p.sketch.segments().len());
            println!("ceiling height: {} m", p.ceiling_height);
            match doc.room() {
                Ok(room) => {
                    println!("volume: {:.1} m³", room.volume());
                    println!("surfaces: {}", room.surfaces.len());
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
            println!("backend: {}  time: {:.2} s  paths: {}", st.backend, st.seconds, st.paths);
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
            CutPlane { id, name: "Ear height".into(), axis: PlaneAxis::Horizontal, offset: 1.2, spacing: 0.2 },
        );
    });
    doc
}
