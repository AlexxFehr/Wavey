use std::collections::BTreeMap;

use glam::DVec2;
use serde::{Deserialize, Serialize};
use wavey_geom::Sketch;

pub const SCHEMA_VERSION: u32 = 1;

/// Everything the user edits. Results are stored separately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub schema_version: u32,
    pub sketch: Sketch,
    pub ceiling_height: f64,
    /// Points marking which sketch regions are simulated interiors.
    pub interior_seeds: BTreeMap<u64, DVec2>,
    next_id: u64,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            sketch: Sketch::new(),
            ceiling_height: 3.0,
            interior_seeds: BTreeMap::new(),
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
}
