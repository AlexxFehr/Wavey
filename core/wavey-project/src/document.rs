use std::path::Path;
use std::sync::Arc;

use glam::DVec2;
use wavey_geom::regions::find_regions;
use wavey_geom::{GeomError, Room, build_room};
use wavey_solver::SimulationResult;

use crate::project::{Project, SCHEMA_VERSION};

const UNDO_LIMIT: usize = 200;
const PROJECT_FILE: &str = "project.json";
const RESULTS_DIR: &str = "results";

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid project file: {0}")]
    Json(#[from] serde_json::Error),
    #[error("project was saved by a newer version of Wavey (schema {0})")]
    NewerSchema(u32),
    #[error(transparent)]
    Geom(#[from] GeomError),
}

/// A project plus edit history and cached derived data.
///
/// Undo is snapshot-based: the project is small compared to results, so
/// cloning it per edit is cheap and makes every edit trivially undoable.
#[derive(Debug, Default)]
pub struct Document {
    project: Project,
    undo: Vec<Project>,
    redo: Vec<Project>,
    room: Option<Result<Room, String>>,
    dirty: bool,
    /// Latest simulation results (not part of undo history).
    results: Option<Arc<SimulationResult>>,
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Applies an edit as one undoable step.
    pub fn edit<R>(&mut self, f: impl FnOnce(&mut Project) -> R) -> R {
        self.undo.push(self.project.clone());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.room = None;
        self.dirty = true;
        f(&mut self.project)
    }

    /// Applies a fallible edit; on error the project is left unchanged and
    /// no undo step is recorded.
    pub fn try_edit<R, E>(&mut self, f: impl FnOnce(&mut Project) -> Result<R, E>) -> Result<R, E> {
        let mut next = self.project.clone();
        let r = f(&mut next)?;
        let prev = std::mem::replace(&mut self.project, next);
        self.undo.push(prev);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.room = None;
        self.dirty = true;
        Ok(r)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.project, prev));
        self.room = None;
        self.dirty = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(&mut self.project, next));
        self.room = None;
        self.dirty = true;
        true
    }

    /// The extruded room, rebuilt lazily after edits.
    pub fn room(&mut self) -> Result<&Room, String> {
        if self.room.is_none() {
            let p = &self.project;
            self.room = Some(
                build_room(&p.sketch, p.ceiling_height, &p.interior_seeds).map_err(|e| e.to_string()),
            );
        }
        self.room.as_ref().unwrap().as_ref().map_err(Clone::clone)
    }

    pub fn results(&self) -> Option<&Arc<SimulationResult>> {
        self.results.as_ref()
    }

    pub fn set_results(&mut self, r: SimulationResult) {
        self.results = Some(Arc::new(r));
        self.dirty = true;
    }

    pub fn clear_results(&mut self) {
        self.results = None;
        self.dirty = true;
    }

    /// Whether the results no longer match the project.
    pub fn results_stale(&self) -> bool {
        self.results.as_ref().is_some_and(|r| r.fingerprint != self.project.simulation_fingerprint())
    }

    /// Marks the region at `p` as interior, or unmarks it if it already is.
    /// Returns false if there is no closed region at `p`.
    pub fn toggle_interior_at(&mut self, p: DVec2) -> bool {
        let regions = find_regions(&self.project.sketch);
        let Some(r) = regions.region_at(p) else { return false };
        let existing: Vec<u64> = self
            .project
            .interior_seeds
            .iter()
            .filter(|(_, s)| regions.region_at(**s) == Some(r))
            .map(|(id, _)| *id)
            .collect();
        self.edit(|proj| {
            if existing.is_empty() {
                let id = proj.alloc_id();
                proj.interior_seeds.insert(id, p);
            } else {
                for id in existing {
                    proj.interior_seeds.remove(&id);
                }
            }
        });
        true
    }

    /// Saves to a `.wavey` package directory.
    pub fn save(&mut self, path: &Path) -> Result<(), ProjectError> {
        std::fs::create_dir_all(path)?;
        let json = serde_json::to_vec_pretty(&self.project)?;
        let tmp = path.join(format!("{PROJECT_FILE}.tmp"));
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path.join(PROJECT_FILE))?;
        let results_dir = path.join(RESULTS_DIR);
        match &self.results {
            Some(r) => r.save(&results_dir)?,
            None if results_dir.exists() => std::fs::remove_dir_all(&results_dir)?,
            None => {}
        }
        self.dirty = false;
        Ok(())
    }

    pub fn open(path: &Path) -> Result<Self, ProjectError> {
        let bytes = std::fs::read(path.join(PROJECT_FILE))?;
        let project: Project = serde_json::from_slice(&bytes)?;
        if project.schema_version > SCHEMA_VERSION {
            return Err(ProjectError::NewerSchema(project.schema_version));
        }
        // Results are optional; a damaged results folder should not block opening.
        let results = SimulationResult::load(&path.join(RESULTS_DIR)).ok().map(Arc::new);
        Ok(Self { project, results, ..Self::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(doc: &mut Document) {
        doc.edit(|p| {
            p.sketch.add_polygon(&[
                DVec2::new(0.0, 0.0),
                DVec2::new(6.0, 0.0),
                DVec2::new(6.0, 4.0),
                DVec2::new(0.0, 4.0),
            ])
        });
    }

    #[test]
    fn undo_redo() {
        let mut doc = Document::new();
        rect(&mut doc);
        assert_eq!(doc.project().sketch.segments().len(), 4);
        assert!(doc.undo());
        assert!(doc.project().sketch.is_empty());
        assert!(doc.redo());
        assert_eq!(doc.project().sketch.segments().len(), 4);
    }

    #[test]
    fn interior_toggle_builds_room() {
        let mut doc = Document::new();
        rect(&mut doc);
        assert!(doc.room().is_err());
        assert!(doc.toggle_interior_at(DVec2::new(1.0, 1.0)));
        assert!((doc.room().unwrap().volume() - 72.0).abs() < 1e-9);
        assert!(doc.toggle_interior_at(DVec2::new(2.0, 2.0)));
        assert!(doc.room().is_err());
        assert!(!doc.toggle_interior_at(DVec2::new(10.0, 10.0)));
    }

    #[test]
    fn save_and_open() {
        let dir = std::env::temp_dir().join(format!("wavey-test-{}.wavey", std::process::id()));
        let mut doc = Document::new();
        rect(&mut doc);
        doc.toggle_interior_at(DVec2::new(1.0, 1.0));
        doc.save(&dir).unwrap();
        let mut back = Document::open(&dir).unwrap();
        assert_eq!(back.project().sketch.segments().len(), 4);
        assert!(back.room().is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
