//! Swift-facing API of the Wavey core, exported with UniFFI.
//!
//! The app holds one [`WaveyDocument`] per window. All edits go through it,
//! and the app pulls plain-data snapshots back for drawing.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use wavey_geom::regions::find_regions;
use wavey_geom::{DVec2, SurfaceKey};
use wavey_project::Document;

mod assembly;
mod import;
mod library;
mod model;
mod organize;
mod simulation;
mod fir;
mod impulses;
mod speakers;
mod types;
pub use assembly::*;
pub use fir::*;
pub use impulses::*;
pub use import::*;
pub use library::*;
pub use model::*;
pub use organize::*;
pub use simulation::*;
pub use speakers::*;
pub use types::*;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum WaveyError {
    #[error("{message}")]
    Failed { message: String },
}

impl WaveyError {
    fn new(e: impl ToString) -> Self {
        WaveyError::Failed {
            message: e.to_string(),
        }
    }
}

#[uniffi::export]
pub fn core_version() -> String {
    wavey_geom::CORE_VERSION.to_string()
}

/// Which parts of the document an edit changed; the app re-reads only those.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ChangedAreas {
    /// The room, modelling features, materials and surface names.
    pub geometry: bool,
    /// Speaker models, speakers and groups.
    pub sources: bool,
    /// Microphones, cut planes and audience areas.
    pub receivers: bool,
    /// Solved runs, the active run and staleness.
    pub results: bool,
    /// Layers, levels, the medium and the simulation settings.
    pub meta: bool,
}

impl From<wavey_project::areas::Areas> for ChangedAreas {
    fn from(a: wavey_project::areas::Areas) -> Self {
        ChangedAreas { geometry: a.geometry, sources: a.sources, receivers: a.receivers, results: a.results, meta: a.meta }
    }
}

#[derive(uniffi::Object)]
pub struct WaveyDocument {
    pub(crate) doc: Arc<Mutex<Document>>,
}

impl WaveyDocument {
    pub(crate) fn lock(&self) -> MutexGuard<'_, Document> {
        self.doc.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[uniffi::export]
impl WaveyDocument {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            doc: Arc::new(Mutex::new(Document::new())),
        })
    }

    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Self>, WaveyError> {
        let doc = Document::open(&PathBuf::from(path)).map_err(WaveyError::new)?;
        Ok(Arc::new(Self {
            doc: Arc::new(Mutex::new(doc)),
        }))
    }

    pub fn save(&self, path: String) -> Result<(), WaveyError> {
        self.lock()
            .save(&PathBuf::from(path))
            .map_err(WaveyError::new)
    }

    pub fn is_dirty(&self) -> bool {
        self.lock().is_dirty()
    }

    /// The areas changed since the last call (everything for a new or
    /// opened document), and resets them.
    pub fn take_changed_areas(&self) -> ChangedAreas {
        self.lock().take_changed().into()
    }

    // ---- history ----

    pub fn undo(&self) -> bool {
        self.lock().undo()
    }

    pub fn redo(&self) -> bool {
        self.lock().redo()
    }

    pub fn can_undo(&self) -> bool {
        self.lock().can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.lock().can_redo()
    }

    /// Names the next edit for the undo menu; consecutive edits with the
    /// same `coalesce_key` within 800 ms become one undo step.
    pub fn label_next_edit(&self, name: String, coalesce_key: Option<String>) {
        self.lock().label_next_edit(&name, coalesce_key.as_deref());
    }

    pub fn undo_name(&self) -> Option<String> {
        self.lock().undo_name().map(str::to_string)
    }

    pub fn redo_name(&self) -> Option<String> {
        self.lock().redo_name().map(str::to_string)
    }

    /// Grows by one per recorded (non-coalesced) undo step.
    pub fn undo_depth(&self) -> u64 {
        self.lock().undo_depth() as u64
    }

    // ---- sketch ----

    pub fn add_segment(&self, a: Point2, b: Point2) -> Vec<u64> {
        self.lock().edit(|p| p.add_segment(a.into(), b.into()))
    }

    pub fn add_polygon(&self, points: Vec<Point2>) -> Vec<u64> {
        let pts: Vec<DVec2> = points.into_iter().map(Into::into).collect();
        self.lock().edit(|p| p.add_polygon(&pts))
    }

    pub fn remove_segments(&self, ids: Vec<u64>) {
        self.lock().edit(|p| {
            for id in ids {
                p.remove_segment(id);
            }
        });
    }

    pub fn move_vertex(&self, id: u64, to: Point2) {
        self.lock().edit(|p| p.move_vertex(id, to.into()));
    }

    /// Toggles whether the closed region at `p` is simulated. False if `p` is
    /// not inside a closed region.
    pub fn toggle_interior_at(&self, p: Point2) -> bool {
        self.lock().toggle_interior_at(p.into())
    }

    /// Makes the closed region at `p` simulated (or not). False if `p` is
    /// not inside a closed region.
    pub fn set_interior_at(&self, p: Point2, interior: bool) -> bool {
        self.lock().set_interior_at(p.into(), interior)
    }

    pub fn ceiling_height(&self) -> f64 {
        self.lock().project().ceiling_height
    }

    pub fn set_ceiling_height(&self, height: f64) -> Result<(), WaveyError> {
        if !(height > 0.0) {
            return Err(WaveyError::new("ceiling height must be positive"));
        }
        self.lock().edit(|p| p.ceiling_height = height);
        Ok(())
    }

    pub fn sketch(&self) -> SketchSnapshot {
        let doc = self.lock();
        let p = doc.project();
        let sketch = &p.sketch;
        let regions = find_regions(sketch);
        let interior: Vec<usize> = p
            .interior_seeds
            .values()
            .filter_map(|s| regions.region_at(*s))
            .collect();
        SketchSnapshot {
            vertices: sketch
                .vertices()
                .iter()
                .map(|(id, v)| SketchVertex {
                    id: *id,
                    p: (*v).into(),
                })
                .collect(),
            segments: sketch
                .segments()
                .iter()
                .map(|(id, s)| SketchSegment {
                    id: *id,
                    a: s.a,
                    b: s.b,
                    pa: sketch.vertex(s.a).unwrap().into(),
                    pb: sketch.vertex(s.b).unwrap().into(),
                    layer_id: p.meta.segment_layer(*id).to_string(),
                })
                .collect(),
            regions: regions
                .regions
                .iter()
                .enumerate()
                .map(|(i, r)| SketchRegion {
                    outer: r.outer.points.iter().map(|p| (*p).into()).collect(),
                    holes: r
                        .holes
                        .iter()
                        .map(|h| h.points.iter().map(|p| (*p).into()).collect())
                        .collect(),
                    area: r.area(),
                    interior: interior.contains(&i),
                })
                .collect(),
        }
    }

    // ---- 3D ----

    /// Extruded room, one mesh per surface. Fails if no interior is selected.
    pub fn room(&self) -> Result<RoomSnapshot, WaveyError> {
        let mut doc = self.lock();
        let room = doc.room().map_err(WaveyError::new)?.clone();
        let project = doc.project();
        Ok(RoomSnapshot::from_room(&room, project))
    }

    // ---- materials ----

    pub fn materials(&self) -> Vec<MaterialRec> {
        let doc = self.lock();
        let p = doc.project();
        let mut list: Vec<MaterialRec> = p.materials.values().map(MaterialRec::from).collect();
        list.sort_by(|a, b| b.builtin.cmp(&a.builtin).then(a.name.cmp(&b.name)));
        list
    }

    pub fn default_material(&self) -> String {
        self.lock().project().default_material.clone()
    }

    /// Adds or replaces a user material.
    pub fn upsert_material(&self, material: MaterialRec) -> Result<(), WaveyError> {
        let m = material.into_material();
        self.lock()
            .try_edit(|p| p.upsert_material(m))
            .map_err(WaveyError::new)
    }

    /// Copies a material under a new id and returns that id.
    pub fn duplicate_material(&self, id: String) -> Result<String, WaveyError> {
        self.lock()
            .try_edit(|p| {
                let mut m = p
                    .materials
                    .get(&id)
                    .cloned()
                    .ok_or(format!("no material `{id}`"))?;
                let mut n = 2;
                while p.materials.contains_key(&format!("{id}-{n}")) {
                    n += 1;
                }
                m.id = format!("{id}-{n}");
                m.name = format!("{} (copy)", m.name);
                let new_id = m.id.clone();
                p.upsert_material(m)?;
                Ok::<_, String>(new_id)
            })
            .map_err(WaveyError::new)
    }

    pub fn delete_material(&self, id: String) -> Result<(), WaveyError> {
        self.lock()
            .try_edit(|p| p.delete_material(&id))
            .map_err(WaveyError::new)
    }

    pub fn assign_material(
        &self,
        surface_keys: Vec<String>,
        material_id: String,
    ) -> Result<(), WaveyError> {
        let keys = surface_keys
            .iter()
            .map(|k| k.parse::<SurfaceKey>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(WaveyError::new)?;
        self.lock()
            .try_edit(|p| p.assign(&keys, &material_id))
            .map_err(WaveyError::new)
    }

    // ---- medium ----

    pub fn medium(&self) -> MediumRec {
        self.lock().project().medium.into()
    }

    pub fn set_medium(&self, medium: MediumRec) -> Result<(), WaveyError> {
        if !(medium.pressure_kpa > 0.0)
            || !(0.0..=100.0).contains(&medium.relative_humidity_pct)
            || !(-50.0..=60.0).contains(&medium.temperature_c)
        {
            return Err(WaveyError::new("air conditions out of range"));
        }
        self.lock().edit(|p| p.medium = medium.into());
        Ok(())
    }
}
