use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use wavey_geom::sketch::EditResult;
use wavey_geom::{SegmentId, Sketch, SurfaceKey, VertexId};
use wavey_materials::{Material, Medium, builtin_materials};
use wavey_solver::{CutPlane, Microphone, SimSettings, SimulationInput, SourceInput};
use wavey_speaker::{SpeakerInstance, SpeakerModel};

use crate::meta::{AimTarget, LAYER_WALLS, ProjectMeta};

pub const SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_MATERIAL: &str = "concrete-painted";

/// A closed room mesh with named material groups.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshModel {
    pub vertices: Vec<[f64; 3]>,
    pub faces: Vec<MeshFace>,
    pub groups: Vec<MeshGroup>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshFace {
    /// Vertex indices of one planar polygon.
    pub v: Vec<u32>,
    pub group: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshGroup {
    pub name: String,
    /// Material of every surface in the group (unless assigned per surface).
    pub material: String,
}

impl MeshModel {
    pub fn build(&self) -> Result<wavey_geom::Room, wavey_geom::GeomError> {
        let v: Vec<DVec3> = self.vertices.iter().map(|p| DVec3::from_array(*p)).collect();
        let f: Vec<(Vec<u32>, u32)> = self.faces.iter().map(|f| (f.v.clone(), f.group)).collect();
        wavey_geom::room_from_mesh(&v, &f)
    }

    /// Reads a Wavefront OBJ: `v` lines, `f` polygons, `usemtl` groups
    /// (else `g`/`o` names), as stored (no repair).
    pub fn from_obj(text: &str, scale: f64) -> Result<Self, String> {
        crate::import::obj_mesh(text, scale)
    }
}

/// Everything the user edits. Results are stored separately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub schema_version: u32,
    pub sketch: Sketch,
    pub ceiling_height: f64,
    /// Points marking which sketch regions are simulated interiors.
    pub interior_seeds: BTreeMap<u64, DVec2>,
    /// Material library by id (built-ins are copied in so projects are self-contained).
    #[serde(default = "default_materials")]
    pub materials: BTreeMap<String, Material>,
    /// Material id per surface, keyed by the surface key's string form.
    #[serde(default)]
    pub assignments: BTreeMap<String, String>,
    #[serde(default = "default_material_id")]
    pub default_material: String,
    #[serde(default)]
    pub medium: Medium,
    /// Speaker models used by this project (embedded so projects are portable).
    #[serde(default)]
    pub speaker_models: BTreeMap<String, Arc<SpeakerModel>>,
    #[serde(default)]
    pub speakers: BTreeMap<u64, SpeakerInstance>,
    #[serde(default)]
    pub microphones: BTreeMap<u64, Microphone>,
    #[serde(default)]
    pub cut_planes: BTreeMap<u64, CutPlane>,
    #[serde(default)]
    pub sim_settings: SimSettings,
    /// Layers, levels, groups, codes and display settings.
    #[serde(default)]
    pub meta: ProjectMeta,
    /// Imported room geometry. When present it replaces the sketch:
    /// any closed polygon mesh (sloped ceilings, stepped floors, recesses).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<MeshModel>,
    next_id: u64,
}

fn default_materials() -> BTreeMap<String, Material> {
    builtin_materials().into_iter().map(|m| (m.id.clone(), m)).collect()
}

fn default_material_id() -> String {
    DEFAULT_MATERIAL.into()
}

impl Default for Project {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            sketch: Sketch::new(),
            ceiling_height: 3.0,
            interior_seeds: BTreeMap::new(),
            materials: default_materials(),
            assignments: BTreeMap::new(),
            default_material: default_material_id(),
            medium: Medium::default(),
            speaker_models: BTreeMap::new(),
            speakers: BTreeMap::new(),
            microphones: BTreeMap::new(),
            cut_planes: BTreeMap::new(),
            sim_settings: SimSettings::default(),
            meta: ProjectMeta::default(),
            mesh: None,
            next_id: 0,
        }
    }
}

impl Project {
    /// Allocates an id for project-level objects (seeds, speakers, mics, …).
    pub fn alloc_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    // ---- sketch edits that keep per-segment data consistent ----

    pub fn add_segment(&mut self, a: DVec2, b: DVec2) -> Vec<SegmentId> {
        let r = self.sketch.add_segment(a, b);
        self.carry_over(&r);
        self.put_on_current_layer(&r.created);
        r.created
    }

    pub fn add_polygon(&mut self, points: &[DVec2]) -> Vec<SegmentId> {
        let r = self.sketch.add_polygon(points);
        self.carry_over(&r);
        self.put_on_current_layer(&r.created);
        r.created
    }

    fn put_on_current_layer(&mut self, segments: &[SegmentId]) {
        let layer = self.meta.current_layer.clone();
        for s in segments {
            if layer == LAYER_WALLS {
                self.meta.segment_layers.remove(s);
            } else {
                self.meta.segment_layers.insert(*s, layer.clone());
            }
        }
    }

    pub fn remove_segment(&mut self, id: SegmentId) {
        if self.sketch.remove_segment(id) {
            for reversed in [false, true] {
                let key = SurfaceKey::Wall { segment: id, reversed }.to_string();
                self.assignments.remove(&key);
                self.meta.surface_names.remove(&key);
                self.meta.forget(&key);
            }
            self.meta.segment_layers.remove(&id);
        }
    }

    pub fn move_vertex(&mut self, id: VertexId, to: DVec2) {
        self.sketch.move_vertex(id, to);
    }

    /// Split walls keep the material, layer and name of the wall they came
    /// from; the first piece also keeps its code.
    fn carry_over(&mut self, r: &EditResult) {
        for split in &r.splits {
            if let Some(layer) = self.meta.segment_layers.remove(&split.old) {
                for new in &split.new {
                    self.meta.segment_layers.insert(*new, layer.clone());
                }
            }
            for reversed in [false, true] {
                let old = SurfaceKey::Wall { segment: split.old, reversed }.to_string();
                let new_key = |n: &SegmentId| SurfaceKey::Wall { segment: *n, reversed }.to_string();
                if let Some(mat) = self.assignments.remove(&old) {
                    for new in &split.new {
                        self.assignments.insert(new_key(new), mat.clone());
                    }
                }
                if let Some(name) = self.meta.surface_names.remove(&old) {
                    for new in &split.new {
                        self.meta.surface_names.insert(new_key(new), name.clone());
                    }
                }
                if let (Some(code), Some(first)) = (self.meta.codes.remove(&old), split.new.first()) {
                    self.meta.codes.insert(new_key(first), code);
                }
            }
        }
    }

    /// Gives sources, mics, planes and groups without a code (projects from
    /// before codes existed) the next free one, in id order.
    pub fn ensure_entity_codes(&mut self) {
        let keys: Vec<String> = self
            .speakers
            .keys()
            .map(|id| format!("spk:{id}"))
            .chain(self.microphones.keys().map(|id| format!("mic:{id}")))
            .chain(self.cut_planes.keys().map(|id| format!("plane:{id}")))
            .chain(self.meta.groups.keys().map(|id| format!("group:{id}")))
            .collect();
        for k in keys {
            self.meta.ensure_code(&k);
        }
    }

    /// Gives every surface of `room` a code (new surfaces get the next free one).
    pub fn ensure_surface_codes(&mut self, room: &wavey_geom::Room) {
        let mut keys: Vec<SurfaceKey> = room.surfaces.iter().map(|s| s.key).collect();
        keys.sort();
        for k in keys {
            self.meta.ensure_code(&k.to_string());
        }
    }

    /// Renames surface keys saved under the old mesh numbering (by discovery
    /// order) to the room's current keys: material assignments, codes and
    /// names. Keys that are current are left alone.
    pub fn migrate_surface_keys(&mut self, room: &wavey_geom::Room) {
        let current: std::collections::BTreeSet<String> = room.surfaces.iter().map(|s| s.key.to_string()).collect();
        for (old, new) in &room.legacy_keys {
            let (old, new) = (old.to_string(), new.to_string());
            if old == new || current.contains(&old) {
                continue;
            }
            for map in [&mut self.assignments, &mut self.meta.codes, &mut self.meta.surface_names] {
                if let Some(v) = map.remove(&old) {
                    map.entry(new.clone()).or_insert(v);
                }
            }
        }
    }

    // ---- materials ----

    pub fn material_for(&self, key: &SurfaceKey) -> &Material {
        let group_material = match (key, &self.mesh) {
            (SurfaceKey::Mesh { group, .. }, Some(m)) => m.groups.get(*group as usize).and_then(|g| self.materials.get(&g.material)),
            _ => None,
        };
        self.assignments
            .get(&key.to_string())
            .and_then(|id| self.materials.get(id))
            .or(group_material)
            .or_else(|| self.materials.get(&self.default_material))
            .or_else(|| self.materials.values().next())
            .expect("project has at least one material")
    }

    pub fn upsert_material(&mut self, mut m: Material) -> Result<(), String> {
        m.validate()?;
        if let Some(existing) = self.materials.get(&m.id)
            && existing.builtin
        {
            return Err("built-in materials cannot be changed; duplicate it first".into());
        }
        m.builtin = false;
        self.materials.insert(m.id.clone(), m);
        Ok(())
    }

    pub fn delete_material(&mut self, id: &str) -> Result<(), String> {
        match self.materials.get(id) {
            None => return Err(format!("no material `{id}`")),
            Some(m) if m.builtin => return Err("built-in materials cannot be deleted".into()),
            _ => {}
        }
        if self.default_material == id {
            return Err("the default material cannot be deleted".into());
        }
        self.materials.remove(id);
        self.assignments.retain(|_, m| m != id);
        Ok(())
    }

    pub fn assign(&mut self, keys: &[SurfaceKey], material: &str) -> Result<(), String> {
        if !self.materials.contains_key(material) {
            return Err(format!("no material `{material}`"));
        }
        for k in keys {
            self.assignments.insert(k.to_string(), material.to_string());
        }
        Ok(())
    }
}

impl Project {
    // ---- speakers ----

    /// Adds a model, renaming its id if another model already uses it.
    pub fn add_speaker_model(&mut self, mut model: SpeakerModel) -> Result<String, String> {
        model.validate().map_err(|e| e.to_string())?;
        if let Some(existing) = self.speaker_models.get(&model.id) {
            if **existing == model {
                return Ok(model.id);
            }
            let base = model.id.clone();
            let mut n = 2;
            while self.speaker_models.contains_key(&format!("{base}-{n}")) {
                n += 1;
            }
            model.id = format!("{base}-{n}");
        }
        let id = model.id.clone();
        self.speaker_models.insert(id.clone(), Arc::new(model));
        Ok(id)
    }

    pub fn remove_speaker_model(&mut self, id: &str) -> Result<(), String> {
        if self.speakers.values().any(|s| s.model_id == id) {
            return Err("model is used by placed speakers".into());
        }
        self.speaker_models.remove(id).map(|_| ()).ok_or(format!("no speaker model `{id}`"))
    }

    pub fn add_speaker(&mut self, model_id: &str, position: glam::DVec3) -> Result<u64, String> {
        let model = self.speaker_models.get(model_id).ok_or(format!("no speaker model `{model_id}`"))?;
        let name = format!("{} {}", model.name, self.speakers.len() + 1);
        let id = self.alloc_id();
        self.meta.ensure_code(&format!("spk:{id}"));
        self.speakers.insert(
            id,
            SpeakerInstance {
                id,
                name,
                model_id: model_id.into(),
                position,
                yaw_deg: 0.0,
                pitch_deg: 0.0,
                roll_deg: 0.0,
                gain_db: 0.0,
                delay_ms: 0.0,
                polarity_inverted: false,
                muted: false,
                on_floor: false,
                bottom_height_m: 0.0,
                filters: vec![],
            },
        );
        Ok(id)
    }

    /// Places a speaker; with `on_floor` it stands on the floor (its
    /// lowest point at z = 0) instead of being centred at `position.z`.
    pub fn place_speaker(&mut self, model_id: &str, position: glam::DVec3, on_floor: bool) -> Result<u64, String> {
        let id = self.add_speaker(model_id, position)?;
        if on_floor {
            let model = self.speaker_models[model_id].clone();
            let s = self.speakers.get_mut(&id).unwrap();
            s.on_floor = true;
            s.bottom_height_m = 0.0;
            s.apply_floor(&model, 0.0);
        }
        Ok(id)
    }

    pub fn update_speaker(&mut self, mut s: SpeakerInstance) -> Result<(), String> {
        let Some(model) = self.speaker_models.get(&s.model_id) else {
            return Err(format!("no speaker model `{}`", s.model_id));
        };
        s.apply_floor(model, 0.0);
        for f in &s.filters {
            f.validate()?;
        }
        match self.speakers.get_mut(&s.id) {
            Some(slot) => {
                *slot = s;
                Ok(())
            }
            None => Err(format!("no speaker {}", s.id)),
        }
    }
}

impl Project {
    // ---- simulation ----

    /// Solver input for the current project and its extruded room.
    pub fn simulation_input(&self, room: &wavey_geom::Room) -> SimulationInput {
        SimulationInput {
            room: room.clone(),
            surface_materials: room.surfaces.iter().map(|s| self.material_for(&s.key).clone()).collect(),
            sources: self
                .speakers
                .values()
                .filter(|s| !s.muted)
                .filter_map(|s| {
                    let model = self.speaker_models.get(&s.model_id)?.clone();
                    Some(SourceInput { instance: s.clone(), model })
                })
                .collect(),
            medium: self.medium,
            microphones: self.solved_microphones().cloned().collect(),
            planes: self.cut_planes.values().cloned().collect(),
            settings: self.sim_settings.clone(),
        }
    }

    /// Hash of everything that affects simulation results. Speaker gain,
    /// delay, polarity and names are excluded: they are applied when mixing.
    pub fn simulation_fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let speakers: Vec<_> = self
            .speakers
            .values()
            .filter(|s| !s.muted)
            .map(|s| (s.id, &s.model_id, s.position, s.yaw_deg, s.pitch_deg, s.roll_deg))
            .collect();
        let materials: BTreeMap<String, &Material> =
            self.assignments.values().filter_map(|id| self.materials.get(id).map(|m| (id.clone(), m))).collect();
        let mut key = serde_json::json!({
            "sketch": self.sketch,
            "height": self.ceiling_height,
            "seeds": self.interior_seeds,
            "assignments": self.assignments,
            "default": self.materials.get(&self.default_material),
            "materials": materials,
            "medium": self.medium,
            "speakers": speakers,
            "models": self.speaker_models.keys().collect::<Vec<_>>(),
            "mics": self.solved_microphones().map(|m| (m.id.to_string(), m)).collect::<BTreeMap<_, _>>(),
            "planes": self.cut_planes,
            "settings": self.sim_settings,
        });
        if let Some(m) = &self.mesh {
            key["mesh"] = serde_json::json!(m);
            let used: BTreeMap<&String, &Material> =
                m.groups.iter().filter_map(|g| self.materials.get(&g.material).map(|x| (&g.material, x))).collect();
            key["mesh_materials"] = serde_json::json!(used);
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key.to_string().hash(&mut h);
        h.finish()
    }

    /// Current user drive per result speaker (`[speaker][freq]`). Speakers
    /// removed since the run are silent; when any speaker is soloed, only
    /// the soloed ones play.
    pub fn drives(&self, speaker_ids: &[u64], freqs: &[f64]) -> Vec<Vec<num_complex::Complex64>> {
        let any_solo = self.speakers.keys().any(|id| self.is_soloed(*id));
        speaker_ids
            .iter()
            .map(|id| match self.speakers.get(id) {
                Some(s) if !any_solo || self.is_soloed(*id) => {
                    // Processing of every group the speaker is in, on top of its own.
                    let groups: Vec<&crate::meta::Group> = self.meta.groups.values().filter(|g| g.members.contains(id)).collect();
                    freqs.iter().map(|f| wavey_solver::user_drive(s, *f) * groups.iter().map(|g| g.response(*f)).product::<num_complex::Complex64>()).collect()
                }
                _ => vec![num_complex::Complex64::new(0.0, 0.0); freqs.len()],
            })
            .collect()
    }

    pub fn is_soloed(&self, speaker: u64) -> bool {
        self.meta.sources.get(&speaker).is_some_and(|m| m.solo)
    }

    /// Microphones that take part in the next solve.
    pub fn solved_microphones(&self) -> impl Iterator<Item = &Microphone> {
        self.microphones.values().filter(|m| self.meta.mics.get(&m.id).is_none_or(|x| x.include_in_solve))
    }

    /// Per-category hashes of the result-relevant data, to tell what
    /// changed since a run.
    pub fn fingerprint_parts(&self) -> BTreeMap<String, u64> {
        use std::hash::{Hash, Hasher};
        let hash = |v: serde_json::Value| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            v.to_string().hash(&mut h);
            h.finish()
        };
        let speakers: Vec<_> = self
            .speakers
            .values()
            .filter(|s| !s.muted)
            .map(|s| (s.id, &s.model_id, s.position, s.yaw_deg, s.pitch_deg, s.roll_deg))
            .collect();
        let materials: BTreeMap<String, &Material> =
            self.assignments.values().filter_map(|id| self.materials.get(id).map(|m| (id.clone(), m))).collect();
        let mics: Vec<_> = self.solved_microphones().collect();
        let mut parts = BTreeMap::new();
        parts.insert("geometry".into(), hash(serde_json::json!([self.sketch, self.ceiling_height, self.interior_seeds, self.mesh])));
        parts.insert(
            "materials".into(),
            hash(serde_json::json!([self.assignments, self.materials.get(&self.default_material), materials])),
        );
        parts.insert("sources".into(), hash(serde_json::json!([speakers, self.speaker_models.keys().collect::<Vec<_>>()])));
        parts.insert("receivers".into(), hash(serde_json::json!([mics, self.cut_planes])));
        parts.insert("environment".into(), hash(serde_json::json!(self.medium)));
        parts.insert("settings".into(), hash(serde_json::json!(self.sim_settings)));
        parts
    }
}

impl Project {
    // ---- receivers ----

    pub fn add_microphone(&mut self, position: glam::DVec3) -> u64 {
        let id = self.alloc_id();
        let name = format!("Mic {}", self.microphones.len() + 1);
        self.microphones.insert(id, Microphone { id, name, position });
        self.meta.ensure_code(&format!("mic:{id}"));
        id
    }

    pub fn remove_microphone(&mut self, id: u64) {
        if self.microphones.remove(&id).is_some() {
            self.meta.forget(&format!("mic:{id}"));
            for m in self.meta.sources.values_mut() {
                if m.aim == Some(AimTarget::Microphone { id }) {
                    m.aim = None;
                }
            }
        }
    }

    pub fn add_cut_plane(&mut self, axis: wavey_solver::PlaneAxis, offset: f64, spacing: f64) -> u64 {
        let id = self.alloc_id();
        let name = format!("Plane {}", self.cut_planes.len() + 1);
        self.cut_planes.insert(id, CutPlane { id, name, axis, offset, spacing });
        self.meta.ensure_code(&format!("plane:{id}"));
        id
    }

    pub fn remove_cut_plane(&mut self, id: u64) {
        if self.cut_planes.remove(&id).is_some() {
            self.meta.forget(&format!("plane:{id}"));
        }
    }

    pub fn remove_speaker(&mut self, id: u64) {
        if self.speakers.remove(&id).is_some() {
            self.meta.forget(&format!("spk:{id}"));
        }
    }

    // ---- groups ----

    pub fn add_group(&mut self, name: &str, members: Vec<u64>) -> u64 {
        let id = self.alloc_id();
        self.meta.groups.insert(id, crate::meta::Group { id, name: name.into(), members, receivers: vec![], filters: vec![], gain_db: 0.0, delay_ms: 0.0, polarity_inverted: false, linked: false, assembly: None });
        self.meta.ensure_code(&format!("group:{id}"));
        id
    }

    // ---- aiming ----

    /// Points every source with an aim target at it (yaw and pitch of its
    /// acoustic axis from its acoustic centre).
    pub fn update_aims(&mut self) {
        let targets: Vec<(u64, glam::DVec3)> = self
            .meta
            .sources
            .iter()
            .filter_map(|(id, m)| {
                let p = match m.aim? {
                    AimTarget::Point { p } => p,
                    AimTarget::Microphone { id } => self.microphones.get(&id)?.position,
                };
                Some((*id, p))
            })
            .collect();
        for (id, target) in targets {
            let Some(s) = self.speakers.get(&id) else { continue };
            let Some(model) = self.speaker_models.get(&s.model_id).cloned() else { continue };
            let from = s.acoustic_center(&model);
            let d = target - from;
            if d.length() < 1e-6 {
                continue;
            }
            let yaw = d.y.atan2(d.x).to_degrees();
            let pitch = d.z.atan2(d.truncate().length()).to_degrees();
            let s = self.speakers.get_mut(&id).unwrap();
            if (s.yaw_deg - yaw).abs() > 1e-9 || (s.pitch_deg - pitch).abs() > 1e-9 {
                s.yaw_deg = yaw;
                s.pitch_deg = pitch;
                s.apply_floor(&model, 0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUBE: &str = "v 0 0 0\nv 4 0 0\nv 4 3 0\nv 0 3 0\nv 0 0 2.5\nv 4 0 2.5\nv 4 3 2.5\nv 0 3 2.5\nusemtl walls\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\nusemtl floor\nf 1 4 3 2\nusemtl ceiling\nf 5 6 7 8\n";

    #[test]
    fn mesh_keys_survive_reordered_faces_and_old_keys_migrate() {
        let keys = |text: &str| {
            let room = MeshModel::from_obj(text, 1.0).unwrap().build().unwrap();
            let mut k: Vec<String> = room.surfaces.iter().map(|s| s.key.to_string()).collect();
            k.sort();
            k
        };
        // The walls in another order: same keys.
        let shuffled = CUBE.replace("f 1 2 6 5\nf 2 3 7 6\n", "f 2 3 7 6\nf 1 2 6 5\n");
        assert_eq!(keys(CUBE), keys(&shuffled));
        assert_eq!(keys(CUBE).len(), 6);

        // A project saved with the old numbering: wall index 1 (the second wall found) had glass.
        let mut doc = crate::Document::new();
        doc.edit(|p| {
            p.mesh = Some(MeshModel::from_obj(CUBE, 1.0).unwrap());
            p.assignments.insert("mesh:0:1".into(), "glass".into());
        });
        let room = doc.room().unwrap().clone();
        let second = room.legacy_keys.iter().find(|(o, _)| o.to_string() == "mesh:0:1").unwrap().1;
        assert_eq!(doc.project().assignments.get(&second.to_string()).map(String::as_str), Some("glass"));
        assert!(!doc.project().assignments.contains_key("mesh:0:1"));
    }

    #[test]
    fn split_walls_keep_material() {
        let mut p = Project::default();
        let ids = p.add_segment(DVec2::new(0.0, 0.0), DVec2::new(4.0, 0.0));
        let key = SurfaceKey::Wall { segment: ids[0], reversed: false };
        p.assign(&[key], "brick").unwrap();
        p.add_segment(DVec2::new(2.0, -1.0), DVec2::new(2.0, 1.0));
        let walls: Vec<_> = p.assignments.iter().filter(|(_, m)| *m == "brick").collect();
        assert_eq!(walls.len(), 2);
        assert!(!p.assignments.contains_key(&key.to_string()));
    }

    #[test]
    fn speaker_models_dedupe_and_protect() {
        let mut p = Project::default();
        let a = p.add_speaker_model(wavey_speaker::samples::omni()).unwrap();
        assert_eq!(p.add_speaker_model(wavey_speaker::samples::omni()).unwrap(), a);
        let mut other = wavey_speaker::samples::omni();
        other.name = "different".into();
        assert_ne!(p.add_speaker_model(other).unwrap(), a);
        let s = p.add_speaker(&a, glam::DVec3::new(1.0, 1.0, 1.5)).unwrap();
        assert!(p.remove_speaker_model(&a).is_err());
        p.speakers.remove(&s);
        p.remove_speaker_model(&a).unwrap();
    }

    #[test]
    fn builtins_are_protected() {
        let mut p = Project::default();
        let mut m = p.materials["brick"].clone();
        m.name = "changed".into();
        assert!(p.upsert_material(m.clone()).is_err());
        m.id = "my-brick".into();
        p.upsert_material(m).unwrap();
        assert!(p.delete_material("brick").is_err());
        p.delete_material("my-brick").unwrap();
    }

    #[test]
    fn aim_target_tracks_and_ui_meta_keeps_fingerprint() {
        let mut p = Project::default();
        let m = p.add_speaker_model(wavey_speaker::samples::omni()).unwrap();
        let s = p.add_speaker(&m, glam::DVec3::new(0.0, 0.0, 1.0)).unwrap();
        let mic = p.add_microphone(glam::DVec3::new(0.0, 5.0, 1.0));
        p.meta.sources.entry(s).or_default().aim = Some(AimTarget::Microphone { id: mic });
        p.update_aims();
        assert!((p.speakers[&s].yaw_deg - 90.0).abs() < 1e-9);
        p.microphones.get_mut(&mic).unwrap().position = glam::DVec3::new(5.0, 0.0, 6.0);
        p.update_aims();
        assert!(p.speakers[&s].yaw_deg.abs() < 1e-9 && (p.speakers[&s].pitch_deg - 45.0).abs() < 1e-9);

        let before = p.simulation_fingerprint();
        p.meta.surface_names.insert("wall:1:f".into(), "Front".into());
        p.meta.planes.entry(1).or_default().opacity = 0.5;
        p.meta.sources.entry(s).or_default().solo = true;
        assert_eq!(p.simulation_fingerprint(), before);
        p.meta.mics.entry(mic).or_default().include_in_solve = false;
        assert_ne!(p.simulation_fingerprint(), before);
        assert_eq!(p.meta.code(&format!("spk:{s}")), Some("S1"));
        assert_eq!(p.meta.code(&format!("mic:{mic}")), Some("M1"));
    }

    #[test]
    fn solo_silences_others() {
        let mut p = Project::default();
        let m = p.add_speaker_model(wavey_speaker::samples::omni()).unwrap();
        let a = p.add_speaker(&m, glam::DVec3::new(0.0, 0.0, 1.0)).unwrap();
        let b = p.add_speaker(&m, glam::DVec3::new(1.0, 0.0, 1.0)).unwrap();
        p.meta.sources.entry(b).or_default().solo = true;
        let d = p.drives(&[a, b], &[100.0]);
        assert_eq!(d[0][0].norm(), 0.0);
        assert!(d[1][0].norm() > 0.0);
    }
}
