use std::collections::BTreeMap;
use std::sync::Arc;

use glam::DVec2;
use serde::{Deserialize, Serialize};
use wavey_geom::sketch::EditResult;
use wavey_geom::{SegmentId, Sketch, SurfaceKey, VertexId};
use wavey_materials::{Material, Medium, builtin_materials};
use wavey_solver::{CutPlane, Microphone, SimSettings, SimulationInput, SourceInput};
use wavey_speaker::{SpeakerInstance, SpeakerModel};

pub const SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_MATERIAL: &str = "concrete-painted";

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
        r.created
    }

    pub fn add_polygon(&mut self, points: &[DVec2]) -> Vec<SegmentId> {
        let r = self.sketch.add_polygon(points);
        self.carry_over(&r);
        r.created
    }

    pub fn remove_segment(&mut self, id: SegmentId) {
        if self.sketch.remove_segment(id) {
            for reversed in [false, true] {
                self.assignments.remove(&SurfaceKey::Wall { segment: id, reversed }.to_string());
            }
        }
    }

    pub fn move_vertex(&mut self, id: VertexId, to: DVec2) {
        self.sketch.move_vertex(id, to);
    }

    /// Split walls keep the material of the wall they came from.
    fn carry_over(&mut self, r: &EditResult) {
        for split in &r.splits {
            for reversed in [false, true] {
                let old = SurfaceKey::Wall { segment: split.old, reversed }.to_string();
                if let Some(mat) = self.assignments.remove(&old) {
                    for new in &split.new {
                        self.assignments
                            .insert(SurfaceKey::Wall { segment: *new, reversed }.to_string(), mat.clone());
                    }
                }
            }
        }
    }

    // ---- materials ----

    pub fn material_for(&self, key: &SurfaceKey) -> &Material {
        self.assignments
            .get(&key.to_string())
            .and_then(|id| self.materials.get(id))
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
            },
        );
        Ok(id)
    }

    pub fn update_speaker(&mut self, s: SpeakerInstance) -> Result<(), String> {
        if !self.speaker_models.contains_key(&s.model_id) {
            return Err(format!("no speaker model `{}`", s.model_id));
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
            microphones: self.microphones.values().cloned().collect(),
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
        let key = serde_json::json!({
            "sketch": self.sketch,
            "height": self.ceiling_height,
            "seeds": self.interior_seeds,
            "assignments": self.assignments,
            "default": self.materials.get(&self.default_material),
            "materials": materials,
            "medium": self.medium,
            "speakers": speakers,
            "models": self.speaker_models.keys().collect::<Vec<_>>(),
            "mics": self.microphones,
            "planes": self.cut_planes,
            "settings": self.sim_settings,
        });
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key.to_string().hash(&mut h);
        h.finish()
    }

    /// Current user drive per result speaker (`[speaker][freq]`). Speakers
    /// removed since the run are silent.
    pub fn drives(&self, speaker_ids: &[u64], freqs: &[f64]) -> Vec<Vec<num_complex::Complex64>> {
        speaker_ids
            .iter()
            .map(|id| match self.speakers.get(id) {
                Some(s) => freqs.iter().map(|f| wavey_solver::user_drive(s, *f)).collect(),
                None => vec![num_complex::Complex64::new(0.0, 0.0); freqs.len()],
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
