//! `wavey build`: create a project from a JSON room description.
//!
//! ```json
//! {
//!   "ceiling_height": 5.0,
//!   "outline": [[0, 0], [25.6, 0], …],          // closed polygon, metres
//!   "solids": [[[x, y], …], …],                  // walls/pillars, floor to ceiling
//!   "interior": [12.0, 12.0],                    // a point inside the room
//!   "speaker_models": ["speakers/a.waveyspk"],
//!   "speakers": [{"name": "L top", "model": "funktion-one-evo2",
//!                 "x": 22.5, "y": 10.3, "bottom": 1.44, "yaw": 180, "gain_db": 0}],
//!   "mics": [{"name": "FOH", "x": 8, "y": 12, "z": 1.6}],
//!   "cut_planes": [{"name": "Ear height", "axis": "horizontal", "offset": 1.6, "spacing": 0.25}]
//! }
//! ```
//! A speaker with `bottom` stands on the floor (its lowest point that high);
//! otherwise `z` is its centre height.

use std::path::Path;

use glam::{DVec2, DVec3};
use serde::Deserialize;
use wavey_project::Document;
use wavey_solver::{CutPlane, Microphone, PlaneAxis};

#[derive(Deserialize)]
struct Spec {
    ceiling_height: f64,
    outline: Vec<[f64; 2]>,
    #[serde(default)]
    solids: Vec<Vec<[f64; 2]>>,
    interior: [f64; 2],
    #[serde(default)]
    speaker_models: Vec<String>,
    #[serde(default)]
    speakers: Vec<SpeakerSpec>,
    #[serde(default)]
    mics: Vec<MicSpec>,
    #[serde(default)]
    cut_planes: Vec<PlaneSpec>,
}

#[derive(Deserialize)]
struct SpeakerSpec {
    name: String,
    model: String,
    x: f64,
    y: f64,
    #[serde(default)]
    z: Option<f64>,
    #[serde(default)]
    bottom: Option<f64>,
    #[serde(default)]
    yaw: f64,
    #[serde(default)]
    pitch: f64,
    #[serde(default)]
    gain_db: f64,
    #[serde(default)]
    delay_ms: f64,
}

#[derive(Deserialize)]
struct MicSpec {
    name: String,
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Deserialize)]
struct PlaneSpec {
    name: String,
    axis: PlaneAxis,
    offset: f64,
    spacing: f64,
}

pub fn build(spec_path: &Path, out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let spec: Spec = serde_json::from_slice(&std::fs::read(spec_path)?)?;
    let base = spec_path.parent().unwrap_or(Path::new("."));
    let pts = |v: &[[f64; 2]]| v.iter().map(|p| DVec2::new(p[0], p[1])).collect::<Vec<_>>();

    let mut doc = Document::new();
    doc.edit(|p| {
        p.ceiling_height = spec.ceiling_height;
        p.add_polygon(&pts(&spec.outline));
        for s in &spec.solids {
            p.add_polygon(&pts(s));
        }
    });
    if !doc.toggle_interior_at(DVec2::new(spec.interior[0], spec.interior[1])) {
        return Err("the interior point is not inside a closed region".into());
    }
    doc.room().map_err(|e| format!("room: {e}"))?;

    doc.try_edit(|p| -> Result<(), String> {
        for path in &spec.speaker_models {
            let full = base.join(path);
            let m = wavey_speaker::SpeakerModel::load(&full).map_err(|e| format!("{}: {e}", full.display()))?;
            p.add_speaker_model(m)?;
        }
        for s in &spec.speakers {
            let id = p.place_speaker(&s.model, DVec3::new(s.x, s.y, s.z.unwrap_or(1.5)), s.bottom.is_some())?;
            let mut inst = p.speakers[&id].clone();
            inst.name = s.name.clone();
            inst.yaw_deg = s.yaw;
            inst.pitch_deg = s.pitch;
            inst.gain_db = s.gain_db;
            inst.delay_ms = s.delay_ms;
            inst.bottom_height_m = s.bottom.unwrap_or(0.0);
            p.update_speaker(inst)?;
        }
        for m in &spec.mics {
            let id = p.alloc_id();
            p.microphones.insert(id, Microphone { id, name: m.name.clone(), position: DVec3::new(m.x, m.y, m.z) });
        }
        for c in &spec.cut_planes {
            let id = p.alloc_id();
            p.cut_planes.insert(id, CutPlane { id, name: c.name.clone(), axis: c.axis, offset: c.offset, spacing: c.spacing });
        }
        Ok(())
    })?;

    // Every speaker must be inside the room.
    let room = doc.room().map_err(|e| format!("room: {e}"))?.clone();
    for s in doc.project().speakers.values() {
        let model = &doc.project().speaker_models[&s.model_id];
        if !room.contains(s.acoustic_center(model)) {
            return Err(format!("speaker `{}` is outside the room", s.name).into());
        }
    }
    doc.save(out)?;
    let r = doc.room().unwrap();
    println!(
        "wrote {}: {:.1} m² floor, {:.0} m³, {} surfaces, {} speakers",
        out.display(),
        r.floor_area(),
        r.volume(),
        r.surfaces.len(),
        doc.project().speakers.len()
    );
    Ok(())
}
