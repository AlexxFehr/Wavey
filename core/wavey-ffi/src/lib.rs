//! Swift-facing API of the Wavey core, exported with UniFFI.
//!
//! The app holds one [`WaveyDocument`] per window. All edits go through it,
//! and the app pulls plain-data snapshots back for drawing.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use wavey_geom::DVec2;
use wavey_geom::regions::find_regions;
use wavey_project::Document;

mod types;
pub use types::*;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum WaveyError {
    #[error("{message}")]
    Failed { message: String },
}

impl WaveyError {
    fn new(e: impl ToString) -> Self {
        WaveyError::Failed { message: e.to_string() }
    }
}

#[uniffi::export]
pub fn core_version() -> String {
    wavey_geom::CORE_VERSION.to_string()
}

#[derive(uniffi::Object)]
pub struct WaveyDocument {
    doc: Mutex<Document>,
}

impl WaveyDocument {
    fn lock(&self) -> MutexGuard<'_, Document> {
        self.doc.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[uniffi::export]
impl WaveyDocument {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self { doc: Mutex::new(Document::new()) })
    }

    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Self>, WaveyError> {
        let doc = Document::open(&PathBuf::from(path)).map_err(WaveyError::new)?;
        Ok(Arc::new(Self { doc: Mutex::new(doc) }))
    }

    pub fn save(&self, path: String) -> Result<(), WaveyError> {
        self.lock().save(&PathBuf::from(path)).map_err(WaveyError::new)
    }

    pub fn is_dirty(&self) -> bool {
        self.lock().is_dirty()
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

    // ---- sketch ----

    pub fn add_segment(&self, a: Point2, b: Point2) -> Vec<u64> {
        self.lock().edit(|p| p.sketch.add_segment(a.into(), b.into()).created)
    }

    pub fn add_polygon(&self, points: Vec<Point2>) -> Vec<u64> {
        let pts: Vec<DVec2> = points.into_iter().map(Into::into).collect();
        self.lock().edit(|p| p.sketch.add_polygon(&pts).created)
    }

    pub fn remove_segments(&self, ids: Vec<u64>) {
        self.lock().edit(|p| {
            for id in ids {
                p.sketch.remove_segment(id);
            }
        });
    }

    pub fn move_vertex(&self, id: u64, to: Point2) {
        self.lock().edit(|p| p.sketch.move_vertex(id, to.into()));
    }

    /// Toggles whether the closed region at `p` is simulated. False if `p` is
    /// not inside a closed region.
    pub fn toggle_interior_at(&self, p: Point2) -> bool {
        self.lock().toggle_interior_at(p.into())
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
        let interior: Vec<usize> =
            p.interior_seeds.values().filter_map(|s| regions.region_at(*s)).collect();
        SketchSnapshot {
            vertices: sketch
                .vertices()
                .iter()
                .map(|(id, v)| SketchVertex { id: *id, p: (*v).into() })
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
                })
                .collect(),
            regions: regions
                .regions
                .iter()
                .enumerate()
                .map(|(i, r)| SketchRegion {
                    outer: r.outer.points.iter().map(|p| (*p).into()).collect(),
                    holes: r.holes.iter().map(|h| h.points.iter().map(|p| (*p).into()).collect()).collect(),
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
        let room = doc.room().map_err(WaveyError::new)?;
        Ok(RoomSnapshot::from_room(room))
    }
}
