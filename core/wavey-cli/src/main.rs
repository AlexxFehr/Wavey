//! Headless Wavey: inspect and simulate `.wavey` projects without the app.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use wavey_geom::{DVec2, DVec3, SurfaceKey, SurfaceKind};
use wavey_project::Document;

#[derive(Parser)]
#[command(name = "wavey", version = wavey_geom::CORE_VERSION, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write a demo project: a 20 × 14 m hall with two pillars.
    Demo { path: PathBuf },
    /// Print a summary of a project.
    Info { path: PathBuf },
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
        Command::Demo { path } => {
            let mut doc = demo_document();
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
    }
    Ok(())
}

pub fn demo_document() -> Document {
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
    doc
}
