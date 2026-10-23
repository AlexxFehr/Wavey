//! Which areas of a document an edit changed, so the app re-reads only
//! those.
//!
//! The areas follow what the app mirrors:
//! - `geometry`: the drawn or imported room, modelling features, materials
//!   and their assignments, surface names and codes, segment layers, underlays;
//! - `sources`: speaker models, speakers, groups and per-source state;
//! - `receivers`: microphones, cut planes and audience areas, and their state;
//! - `results`: solved runs, which run is active, and whether a run is stale;
//! - `meta`: layers, levels, the medium and the simulation settings.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::project::Project;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Areas {
    pub geometry: bool,
    pub sources: bool,
    pub receivers: bool,
    pub results: bool,
    pub meta: bool,
}

impl Areas {
    pub const ALL: Areas = Areas { geometry: true, sources: true, receivers: true, results: true, meta: true };

    pub fn any(self) -> bool {
        self.geometry || self.sources || self.receivers || self.results || self.meta
    }

    pub fn union(self, o: Areas) -> Areas {
        Areas {
            geometry: self.geometry || o.geometry,
            sources: self.sources || o.sources,
            receivers: self.receivers || o.receivers,
            results: self.results || o.results,
            meta: self.meta || o.meta,
        }
    }

    /// The areas whose data differs between two states of a project. Run
    /// state is not part of a project: [`Document`](crate::Document) adds
    /// `results`.
    pub fn between(a: &Project, b: &Project) -> Areas {
        let mut out = Areas::default();
        out.geometry = a.sketch != b.sketch
            || a.ceiling_height != b.ceiling_height
            || a.interior_seeds != b.interior_seeds
            || a.materials != b.materials
            || a.assignments != b.assignments
            || a.default_material != b.default_material
            || a.mesh != b.mesh
            || a.features != b.features
            || a.meta.underlays != b.meta.underlays
            || a.meta.segment_layers != b.meta.segment_layers
            || a.meta.surface_names != b.meta.surface_names;
        out.sources = !same_models(a, b) || a.speakers != b.speakers || a.meta.sources != b.meta.sources || a.meta.groups != b.meta.groups;
        out.receivers = a.microphones != b.microphones || a.cut_planes != b.cut_planes || a.meta.mics != b.meta.mics || a.meta.planes != b.meta.planes;
        out.meta = a.meta.layers != b.meta.layers
            || a.meta.current_layer != b.meta.current_layer
            || a.meta.levels != b.meta.levels
            || a.medium != b.medium
            || a.sim_settings != b.sim_settings;
        // Codes belong to the area of what they label.
        for key in changed_keys(&a.meta.codes, &b.meta.codes) {
            if key.starts_with("spk:") || key.starts_with("group:") {
                out.sources = true;
            } else if key.starts_with("mic:") || key.starts_with("plane:") {
                out.receivers = true;
            } else {
                out.geometry = true;
            }
        }
        out
    }
}

/// Models are immutable once added: a replaced model is a new `Arc`.
fn same_models(a: &Project, b: &Project) -> bool {
    a.speaker_models.len() == b.speaker_models.len()
        && a.speaker_models.iter().zip(&b.speaker_models).all(|((ka, va), (kb, vb))| ka == kb && Arc::ptr_eq(va, vb))
}

fn changed_keys<'a>(a: &'a BTreeMap<String, String>, b: &'a BTreeMap<String, String>) -> impl Iterator<Item = &'a String> {
    let changed_in_a = a.iter().filter(|(k, v)| b.get(*k) != Some(v)).map(|(k, _)| k);
    let new_in_b = b.keys().filter(|k| !a.contains_key(*k));
    changed_in_a.chain(new_in_b)
}
