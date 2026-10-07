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
//!                 "x": 22.5, "y": 10.3, "bottom": 1.44, "yaw": 180, "gain_db": 0,
//!                 "filters": [{"type": "high_pass", "freq_hz": 80, "slope": "linkwitz_riley24"}]}],
//!   "mics": [{"name": "FOH", "x": 8, "y": 12, "z": 1.6}],
//!   "cut_planes": [{"name": "Ear height", "axis": "horizontal", "offset": 1.6, "spacing": 0.25}]
//!   // an audience area over all floors instead: {"name": "Seats", "drape_height": 1.2, "spacing": 0.5}
//! }
//! ```
//! A speaker with `bottom` stands on the floor (its lowest point that high);
//! otherwise `z` is its centre height.
//!
//! Instead of `outline`/`ceiling_height`/`interior`, a room can be an OBJ
//! mesh with one material per `usemtl` group, and materials can be read from
//! CSV files (`freq;…` / `abs;…` / `sct;…` rows, as in the BRAS database):
//!
//! ```json
//! { "mesh": {"obj": "room.obj", "scale": 1.0, "groups": {"mat_wall": "concrete"}},
//!   "material_csv": [{"id": "concrete", "file": "mat_concrete.csv"}], … }
//! ```

use std::path::Path;

use glam::{DVec2, DVec3};
use serde::Deserialize;
use wavey_project::Document;
use wavey_solver::{CutPlane, Microphone, PlaneAxis};

#[derive(Deserialize)]
struct Spec {
    #[serde(default)]
    ceiling_height: f64,
    #[serde(default)]
    outline: Vec<[f64; 2]>,
    #[serde(default)]
    solids: Vec<Vec<[f64; 2]>>,
    #[serde(default)]
    interior: [f64; 2],
    #[serde(default)]
    mesh: Option<MeshSpec>,
    #[serde(default)]
    material_csv: Vec<CsvMaterial>,
    #[serde(default)]
    speaker_models: Vec<String>,
    #[serde(default)]
    speakers: Vec<SpeakerSpec>,
    #[serde(default)]
    mics: Vec<MicSpec>,
    #[serde(default)]
    cut_planes: Vec<PlaneSpec>,
    /// Extra materials added to the project library.
    #[serde(default)]
    materials: Vec<wavey_materials::Material>,
    /// Materials of the main interior: walls (all walls and solids), floor, ceiling.
    #[serde(default)]
    surfaces: SurfaceSpec,
    /// Further interior zones (e.g. a dance floor) with their own floor and
    /// ceiling materials; their edges are open, not walls.
    #[serde(default)]
    zones: Vec<ZoneSpec>,
}

#[derive(Deserialize)]
struct MeshSpec {
    obj: String,
    #[serde(default = "one")]
    scale: f64,
    /// Material id per OBJ material group.
    #[serde(default)]
    groups: std::collections::BTreeMap<String, String>,
}

fn one() -> f64 {
    1.0
}

#[derive(Deserialize)]
struct CsvMaterial {
    id: String,
    file: String,
    #[serde(default)]
    name: Option<String>,
    /// Multiplies the absorption (capped at 0.99): keeps a material's
    /// absorption area S·α when the model's surface area differs from the
    /// one the coefficients were fitted for.
    #[serde(default)]
    absorption_scale: Option<f64>,
}

/// Material from a `freq;…` / `abs;…` / `sct;…` CSV (semicolon or comma separated).
fn csv_material(id: &str, name: &str, text: &str) -> Result<wavey_materials::Material, String> {
    let mut rows: std::collections::BTreeMap<String, Vec<f64>> = Default::default();
    for line in text.lines() {
        let cells: Vec<&str> = line.split([';', ',']).map(str::trim).collect();
        if cells.len() < 2 {
            continue;
        }
        let vals: Vec<f64> = cells[1..].iter().filter_map(|c| c.parse().ok()).collect();
        rows.insert(cells[0].to_lowercase(), vals);
    }
    let f = rows.get("freq").ok_or("no freq row")?.clone();
    let get = |k: &str| rows.get(k).cloned().filter(|v| v.len() == f.len());
    let abs = get("abs").ok_or("no abs row matching freq")?;
    let sct = get("sct").unwrap_or_else(|| vec![0.05; f.len()]);
    Ok(wavey_materials::Material {
        id: id.into(),
        name: name.into(),
        absorption: wavey_materials::Spectrum::Bands { freqs_hz: f.clone(), values: abs },
        scattering: wavey_materials::Spectrum::Bands { freqs_hz: f, values: sct },
        impedance: None,
        angle_dependent: false,
        color: "#9aa4ae".into(),
        builtin: false,
        source: Some("CSV import".into()),
    })
}

#[derive(Deserialize, Default)]
struct SurfaceSpec {
    walls: Option<String>,
    floor: Option<String>,
    ceiling: Option<String>,
}

#[derive(Deserialize)]
struct ZoneSpec {
    #[allow(dead_code)]
    name: String,
    polygon: Vec<[f64; 2]>,
    floor: Option<String>,
    ceiling: Option<String>,
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
    #[serde(default)]
    filters: Vec<wavey_speaker::Filter>,
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
    /// Audience area: this far above the floors (axis and offset are then ignored).
    #[serde(default)]
    drape_height: Option<f64>,
}

pub fn build(spec_path: &Path, out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let spec: Spec = serde_json::from_slice(&std::fs::read(spec_path)?)?;
    let base = spec_path.parent().unwrap_or(Path::new("."));
    let pts = |v: &[[f64; 2]]| v.iter().map(|p| DVec2::new(p[0], p[1])).collect::<Vec<_>>();

    let mut doc = Document::new();
    if let Some(ms) = &spec.mesh {
        let text = std::fs::read_to_string(base.join(&ms.obj))?;
        let mut mesh = wavey_project::MeshModel::from_obj(&text, ms.scale)?;
        let mut mats = Vec::new();
        for c in &spec.material_csv {
            let text = std::fs::read_to_string(base.join(&c.file))?;
            let mut m = csv_material(&c.id, c.name.as_deref().unwrap_or(&c.id), &text)?;
            if let (Some(k), wavey_materials::Spectrum::Bands { values, .. }) = (c.absorption_scale, &mut m.absorption) {
                values.iter_mut().for_each(|v| *v = (*v * k).min(0.99));
            }
            mats.push(m);
        }
        for g in &mut mesh.groups {
            if let Some(m) = ms.groups.get(&g.name) {
                g.material = m.clone();
            }
        }
        doc.try_edit(|p| -> Result<(), String> {
            for m in mats.iter().chain(&spec.materials) {
                p.upsert_material(m.clone())?;
            }
            p.mesh = Some(mesh);
            Ok(())
        })?;
        doc.room().map_err(|e| format!("room: {e}"))?;
        return finish(doc, &spec, base, out, &[]);
    }
    doc.edit(|p| {
        p.ceiling_height = spec.ceiling_height;
        p.add_polygon(&pts(&spec.outline));
        for s in &spec.solids {
            p.add_polygon(&pts(s));
        }
        for z in &spec.zones {
            p.add_polygon(&pts(&z.polygon));
        }
    });
    if !doc.toggle_interior_at(DVec2::new(spec.interior[0], spec.interior[1])) {
        return Err("the interior point is not inside a closed region".into());
    }
    // A zone is marked through a point inside it (its centroid, nudged if it falls in a hole).
    let mut zone_seeds = Vec::new();
    for z in &spec.zones {
        let poly = pts(&z.polygon);
        let c = poly.iter().copied().sum::<DVec2>() / poly.len() as f64;
        let regions = wavey_geom::regions::find_regions(&doc.project().sketch);
        let probe = [c, c + DVec2::new(0.37, 0.21), c - DVec2::new(0.41, 0.33), c + DVec2::new(1.13, -0.71)]
            .into_iter()
            .find(|p| regions.region_at(*p).is_some_and(|r| wavey_geom::regions::point_in_polygon(*p, &regions.regions[r].outer.points)
                && regions.regions[r].holes.iter().all(|h| !wavey_geom::regions::point_in_polygon(*p, &h.points))))
            .ok_or(format!("zone `{}`: no point inside it", z.name))?;
        if !doc.toggle_interior_at(probe) {
            return Err(format!("zone `{}` is not a closed region", z.name).into());
        }
        let seed = *doc.project().interior_seeds.iter().last().unwrap().0;
        zone_seeds.push(seed);
    }
    doc.room().map_err(|e| format!("room: {e}"))?;
    finish(doc, &spec, base, out, &zone_seeds)
}

/// Speakers, mics, planes, surface materials; then saves.
fn finish(mut doc: Document, spec: &Spec, base: &Path, out: &Path, zone_seeds: &[u64]) -> Result<(), Box<dyn std::error::Error>> {

    doc.try_edit(|p| -> Result<(), String> {
        for m in &spec.materials {
            p.upsert_material(m.clone())?;
        }
        for path in &spec.speaker_models {
            // `sample:<id>` is a built-in sample (e.g. `sample:sample-omni`).
            let m = if let Some(id) = path.strip_prefix("sample:") {
                wavey_speaker::samples::all().into_iter().find(|m| m.id == id).ok_or(format!("no sample `{id}`"))?
            } else {
                let full = base.join(path);
                wavey_speaker::SpeakerModel::load(&full).map_err(|e| format!("{}: {e}", full.display()))?
            };
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
            inst.filters = s.filters.clone();
            inst.bottom_height_m = s.bottom.unwrap_or(0.0);
            p.update_speaker(inst)?;
        }
        for m in &spec.mics {
            let id = p.alloc_id();
            p.microphones.insert(id, Microphone { id, name: m.name.clone(), position: DVec3::new(m.x, m.y, m.z) });
        }
        for c in &spec.cut_planes {
            let id = p.alloc_id();
            p.cut_planes.insert(id, CutPlane { id, name: c.name.clone(), axis: c.axis, offset: c.offset, spacing: c.spacing, drape: c.drape_height.map(|height| wavey_solver::Drape { height, surfaces: vec![] }) });
        }
        Ok(())
    })?;

    // Surface materials.
    let room = doc.room().map_err(|e| format!("room: {e}"))?.clone();
    doc.try_edit(|p| -> Result<(), String> {
        for s in &room.surfaces {
            let (kind_mat, zone) = match s.key {
                wavey_geom::SurfaceKey::Wall { .. } => (spec.surfaces.walls.clone(), None),
                wavey_geom::SurfaceKey::Floor { seed } => (spec.surfaces.floor.clone(), zone_seeds.iter().position(|z| *z == seed).map(|i| (i, true))),
                wavey_geom::SurfaceKey::Ceiling { seed } => (spec.surfaces.ceiling.clone(), zone_seeds.iter().position(|z| *z == seed).map(|i| (i, false))),
                // Mesh surfaces take their group's material.
                wavey_geom::SurfaceKey::Mesh { .. } | wavey_geom::SurfaceKey::Feature { .. } => continue,
            };
            let mat = match zone {
                Some((i, true)) => spec.zones[i].floor.clone().or(kind_mat),
                Some((i, false)) => spec.zones[i].ceiling.clone().or(kind_mat),
                None => kind_mat,
            };
            if let Some(m) = mat {
                p.assign(&[s.key], &m)?;
            }
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
