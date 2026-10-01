use std::collections::BTreeMap;

use glam::DVec2;
use serde::{Deserialize, Serialize};
use wavey_geom::sketch::EditResult;
use wavey_geom::{SegmentId, Sketch, SurfaceKey, VertexId};
use wavey_materials::{Material, Medium, builtin_materials};

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
