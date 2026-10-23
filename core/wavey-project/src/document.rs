use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use glam::DVec2;
use serde::{Deserialize, Serialize};
use wavey_geom::regions::find_regions;
use wavey_geom::{GeomError, Room};
use wavey_solver::result::RunStats;
use wavey_solver::{SimSettings, SimulationResult};

use crate::areas::Areas;
use crate::project::{Project, SCHEMA_VERSION};

const UNDO_LIMIT: usize = 200;
const PROJECT_FILE: &str = "project.json";
const RESULTS_DIR: &str = "results";
const RUNS_FILE: &str = "runs.json";
/// Consecutive edits with the same coalescing key within this window are one undo step.
const COALESCE_WINDOW: Duration = Duration::from_millis(800);

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

/// One undo step: the project as it was before the edit.
#[derive(Debug)]
struct Step {
    project: Project,
    name: String,
    key: Option<String>,
    at: Instant,
}

/// What a solved run was computed from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMeta {
    pub id: u32,
    pub name: String,
    /// Seconds since the Unix epoch.
    pub created: u64,
    pub settings: SimSettings,
    /// [`Project::fingerprint_parts`] at solve time.
    #[serde(default)]
    pub fingerprint_parts: BTreeMap<String, u64>,
    /// Codes of the sources that took part.
    #[serde(default)]
    pub sources: Vec<String>,
    /// [`Project::simulation_fingerprint`] the run was solved for.
    #[serde(default)]
    pub fingerprint: u64,
    #[serde(default)]
    pub stats: RunStats,
    /// Number of solved frequencies.
    #[serde(default)]
    pub frequencies: usize,
}

/// A solved run. Its data is loaded from disk on first use.
#[derive(Debug)]
pub struct Run {
    pub meta: RunMeta,
    data: OnceLock<Option<Arc<SimulationResult>>>,
    /// Where the data is stored, if it has been saved.
    dir: Option<PathBuf>,
}

impl Run {
    pub fn result(&self) -> Option<&Arc<SimulationResult>> {
        self.data
            .get_or_init(|| self.dir.as_ref().and_then(|d| SimulationResult::load(d).ok()).map(Arc::new))
            .as_ref()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RunsFile {
    active: Option<u32>,
    runs: Vec<RunMeta>,
}

/// A project plus edit history, derived data and solved runs.
///
/// Undo is snapshot-based: the project is small compared to results, so
/// cloning it per edit is cheap and makes every edit trivially undoable.
#[derive(Debug, Default)]
pub struct Document {
    project: Project,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Name and coalescing key for the next edit (set by the app).
    next_label: Option<(String, Option<String>)>,
    room: Option<Result<Room, String>>,
    dirty: bool,
    /// Solved runs, oldest first (not part of undo history).
    runs: Vec<Run>,
    active_run: Option<u32>,
    /// Areas changed since the app last asked ([`take_changed`](Self::take_changed)).
    changed: Areas,
}

impl Document {
    pub fn new() -> Self {
        Self { changed: Areas::ALL, ..Self::default() }
    }

    /// The areas changed since the last call, and resets them. A new or
    /// opened document reports everything.
    pub fn take_changed(&mut self) -> Areas {
        std::mem::take(&mut self.changed)
    }

    /// Notes what differs between `other` (the project as it was) and the
    /// current project, including whether any run's staleness flipped.
    fn note_changes_from(&mut self, other: &Project) {
        let mut areas = Areas::between(other, &self.project);
        if !self.runs.is_empty() && areas.any() {
            let (was, now) = (other.simulation_fingerprint(), self.project.simulation_fingerprint());
            if self.runs.iter().any(|r| (r.meta.fingerprint != was) != (r.meta.fingerprint != now)) {
                areas.results = true;
            }
        }
        self.changed = self.changed.union(areas);
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    // ---- edits and history ----

    /// Names the next edit for the undo menu. Consecutive edits with the
    /// same `coalesce` key within 800 ms merge into one undo step.
    pub fn label_next_edit(&mut self, name: &str, coalesce: Option<&str>) {
        self.next_label = Some((name.to_string(), coalesce.map(str::to_string)));
    }

    fn record(&mut self, before: Project) {
        self.note_changes_from(&before);
        let (name, key) = self.next_label.take().unwrap_or_else(|| ("Edit".into(), None));
        let now = Instant::now();
        let merge = key.is_some()
            && self.redo.is_empty()
            && self.undo.last().is_some_and(|s| s.key == key && now.duration_since(s.at) < COALESCE_WINDOW);
        if merge {
            self.undo.last_mut().unwrap().at = now;
        } else {
            self.undo.push(Step { project: before, name, key, at: now });
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.room = None;
        self.dirty = true;
    }

    /// Applies an edit as one undoable step.
    pub fn edit<R>(&mut self, f: impl FnOnce(&mut Project) -> R) -> R {
        let before = self.project.clone();
        let r = f(&mut self.project);
        self.project.update_aims();
        self.record(before);
        r
    }

    /// Applies a fallible edit; on error the project is left unchanged and
    /// no undo step is recorded.
    pub fn try_edit<R, E>(&mut self, f: impl FnOnce(&mut Project) -> Result<R, E>) -> Result<R, E> {
        let mut next = self.project.clone();
        let r = match f(&mut next) {
            Ok(r) => r,
            Err(e) => {
                self.next_label = None;
                return Err(e);
            }
        };
        next.update_aims();
        let before = std::mem::replace(&mut self.project, next);
        self.record(before);
        Ok(r)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Name of the step `undo` would revert.
    pub fn undo_name(&self) -> Option<&str> {
        self.undo.last().map(|s| s.name.as_str())
    }

    pub fn redo_name(&self) -> Option<&str> {
        self.redo.last().map(|s| s.name.as_str())
    }

    /// Number of undo steps (grows by one per non-coalesced edit).
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    pub fn undo(&mut self) -> bool {
        let Some(mut step) = self.undo.pop() else { return false };
        std::mem::swap(&mut self.project, &mut step.project);
        self.note_changes_from(&step.project);
        step.key = None;
        self.redo.push(step);
        self.room = None;
        self.dirty = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(mut step) = self.redo.pop() else { return false };
        std::mem::swap(&mut self.project, &mut step.project);
        self.note_changes_from(&step.project);
        self.undo.push(step);
        self.room = None;
        self.dirty = true;
        true
    }

    /// What a solve runs on: the room, or open air with a ground when the
    /// project has no walls.
    pub fn solver_input(&mut self) -> Result<wavey_solver::SimulationInput, String> {
        if self.project.open_air() {
            return self.project.open_air_input();
        }
        let room = self.room()?.clone();
        Ok(self.project.simulation_input(&room))
    }

    /// The extruded room, rebuilt lazily after edits. New surfaces get codes.
    pub fn room(&mut self) -> Result<&Room, String> {
        if self.room.is_none() {
            let p = &self.project;
            let built = p.build_room().map_err(|e| e.to_string());
            if let Ok(room) = &built {
                self.project.migrate_surface_keys(room);
                self.project.ensure_surface_codes(room);
            }
            self.room = Some(built);
        }
        self.room.as_ref().unwrap().as_ref().map_err(Clone::clone)
    }

    // ---- runs ----

    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    pub fn active_run(&self) -> Option<&Run> {
        self.active_run.and_then(|id| self.runs.iter().find(|r| r.meta.id == id))
    }

    pub fn set_active_run(&mut self, id: u32) -> bool {
        if self.runs.iter().any(|r| r.meta.id == id) {
            self.active_run = Some(id);
            self.dirty = true;
            self.changed.results = true;
            true
        } else {
            false
        }
    }

    /// Results of the active run.
    pub fn results(&self) -> Option<&Arc<SimulationResult>> {
        self.active_run()?.result()
    }

    /// Stores a finished solve as a new run and makes it active. Returns its id.
    pub fn add_run(&mut self, r: SimulationResult) -> u32 {
        let id = self.runs.iter().map(|r| r.meta.id).max().unwrap_or(0) + 1;
        let p = &self.project;
        let sources = r.speaker_ids.iter().filter_map(|s| p.meta.code(&format!("spk:{s}")).map(str::to_string)).collect();
        let meta = RunMeta {
            id,
            name: format!("Run {id:03}"),
            created: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
            settings: p.sim_settings.clone(),
            fingerprint_parts: p.fingerprint_parts(),
            sources,
            fingerprint: r.fingerprint,
            stats: r.stats.clone(),
            frequencies: r.freqs.len(),
        };
        let data = OnceLock::new();
        let _ = data.set(Some(Arc::new(r)));
        self.runs.push(Run { meta, data, dir: None });
        self.active_run = Some(id);
        self.dirty = true;
        self.changed.results = true;
        id
    }

    /// Same as [`add_run`](Self::add_run); kept for callers that think of one result.
    pub fn set_results(&mut self, r: SimulationResult) {
        self.add_run(r);
    }

    pub fn rename_run(&mut self, id: u32, name: &str) -> bool {
        let Some(r) = self.runs.iter_mut().find(|r| r.meta.id == id) else { return false };
        r.meta.name = name.to_string();
        self.dirty = true;
        self.changed.results = true;
        true
    }

    /// Deletes a run; the newest remaining run becomes active if it was.
    pub fn delete_run(&mut self, id: u32) -> bool {
        let before = self.runs.len();
        self.runs.retain(|r| r.meta.id != id);
        if self.runs.len() == before {
            return false;
        }
        if self.active_run == Some(id) {
            self.active_run = self.runs.last().map(|r| r.meta.id);
        }
        self.dirty = true;
        self.changed.results = true;
        true
    }

    pub fn clear_results(&mut self) {
        self.runs.clear();
        self.active_run = None;
        self.dirty = true;
        self.changed.results = true;
    }

    /// Whether the active run no longer matches the project.
    pub fn results_stale(&self) -> bool {
        self.active_run().is_some_and(|r| self.run_stale(r))
    }

    pub fn run_stale(&self, run: &Run) -> bool {
        run.meta.fingerprint != self.project.simulation_fingerprint()
    }

    /// Result-relevant categories that changed since the active run.
    pub fn changes_since_run(&self) -> Vec<String> {
        let Some(run) = self.active_run() else { return vec![] };
        let now = self.project.fingerprint_parts();
        if run.meta.fingerprint_parts.is_empty() {
            return if self.results_stale() { vec!["unknown".into()] } else { vec![] };
        }
        now.iter().filter(|(k, v)| run.meta.fingerprint_parts.get(*k) != Some(v)).map(|(k, _)| k.clone()).collect()
    }

    // ---- sketch helpers ----

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

    /// Makes the region at `p` interior (no-op if it already is). False if
    /// there is no closed region at `p`.
    pub fn set_interior_at(&mut self, p: DVec2, interior: bool) -> bool {
        let regions = find_regions(&self.project.sketch);
        let Some(r) = regions.region_at(p) else { return false };
        let is = self.project.interior_seeds.values().any(|s| regions.region_at(*s) == Some(r));
        if is != interior {
            self.toggle_interior_at(p);
        }
        true
    }

    // ---- files ----

    /// Saves to a `.wavey` package directory.
    pub fn save(&mut self, path: &Path) -> Result<(), ProjectError> {
        std::fs::create_dir_all(path)?;
        let json = serde_json::to_vec_pretty(&self.project)?;
        let tmp = path.join(format!("{PROJECT_FILE}.tmp"));
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path.join(PROJECT_FILE))?;

        let results_dir = path.join(RESULTS_DIR);
        if self.runs.is_empty() {
            if results_dir.exists() {
                std::fs::remove_dir_all(&results_dir)?;
            }
        } else {
            std::fs::create_dir_all(&results_dir)?;
            // Load everything first: a run may live in a folder this save replaces.
            for run in &self.runs {
                run.result();
            }
            let mut keep = vec![std::ffi::OsString::from(RUNS_FILE)];
            for run in &mut self.runs {
                let name = format!("run-{:03}", run.meta.id);
                let target = results_dir.join(&name);
                if run.dir.as_deref() != Some(target.as_path()) {
                    if let Some(r) = run.result() {
                        if target.exists() {
                            std::fs::remove_dir_all(&target)?;
                        }
                        r.save(&target)?;
                    }
                    run.dir = Some(target);
                }
                keep.push(name.into());
            }
            for entry in std::fs::read_dir(&results_dir)? {
                let entry = entry?;
                if !keep.contains(&entry.file_name()) {
                    let p = entry.path();
                    if p.is_dir() { std::fs::remove_dir_all(p)? } else { std::fs::remove_file(p)? }
                }
            }
            let file = RunsFile { active: self.active_run, runs: self.runs.iter().map(|r| r.meta.clone()).collect() };
            std::fs::write(results_dir.join(RUNS_FILE), serde_json::to_vec_pretty(&file)?)?;
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
        let results_dir = path.join(RESULTS_DIR);
        let mut doc = Self { project, changed: Areas::ALL, ..Self::default() };
        doc.project.ensure_entity_codes();
        // Results are optional; a damaged results folder should not block opening.
        match std::fs::read(results_dir.join(RUNS_FILE)).ok().and_then(|b| serde_json::from_slice::<RunsFile>(&b).ok()) {
            Some(file) => {
                doc.runs = file
                    .runs
                    .into_iter()
                    .map(|meta| {
                        let dir = Some(results_dir.join(format!("run-{:03}", meta.id)));
                        Run { meta, data: OnceLock::new(), dir }
                    })
                    .collect();
                doc.active_run = file.active.filter(|id| doc.runs.iter().any(|r| r.meta.id == *id));
            }
            None => {
                // Single result from before runs existed: becomes run 1 (moved on the next save).
                if let Ok(r) = SimulationResult::load(&results_dir) {
                    let created = std::fs::metadata(results_dir.join("results.json"))
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_secs());
                    let meta = RunMeta {
                        id: 1,
                        name: "Run 001".into(),
                        created,
                        settings: doc.project.sim_settings.clone(),
                        fingerprint_parts: BTreeMap::new(),
                        sources: vec![],
                        fingerprint: r.fingerprint,
                        stats: r.stats.clone(),
                        frequencies: r.freqs.len(),
                    };
                    let data = OnceLock::new();
                    let _ = data.set(Some(Arc::new(r)));
                    doc.runs.push(Run { meta, data, dir: Some(results_dir.clone()) });
                    doc.active_run = Some(1);
                }
            }
        }
        Ok(doc)
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
    fn labelled_and_coalesced_undo() {
        let mut doc = Document::new();
        doc.label_next_edit("Set Height", Some("height"));
        doc.edit(|p| p.ceiling_height = 3.1);
        doc.label_next_edit("Set Height", Some("height"));
        doc.edit(|p| p.ceiling_height = 3.2);
        assert_eq!(doc.undo_depth(), 1);
        assert_eq!(doc.undo_name(), Some("Set Height"));
        doc.label_next_edit("Set Height", Some("other"));
        doc.edit(|p| p.ceiling_height = 3.3);
        assert_eq!(doc.undo_depth(), 2);
        doc.edit(|p| p.ceiling_height = 3.4);
        assert_eq!(doc.undo_name(), Some("Edit"));
        assert!(doc.undo() && doc.undo() && doc.undo());
        assert_eq!(doc.project().ceiling_height, 3.0);
        assert_eq!(doc.redo_name(), Some("Set Height"));
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
    fn surfaces_get_codes() {
        let mut doc = Document::new();
        rect(&mut doc);
        doc.toggle_interior_at(DVec2::new(1.0, 1.0));
        let n = doc.room().unwrap().surfaces.len();
        let codes = &doc.project().meta.codes;
        assert_eq!(codes.len(), n);
        assert!(codes.values().any(|c| c == "W-01") && codes.values().any(|c| c == "F-01"));
    }

    fn tiny_result(doc: &Document) -> SimulationResult {
        SimulationResult {
            freqs: vec![100.0],
            speaker_ids: vec![],
            mics: vec![],
            planes: vec![],
            stats: Default::default(),
            fingerprint: doc.project().simulation_fingerprint(),
            rt_sabine: Default::default(),
            rt_eyring: Default::default(),
            has_late: false,
            fdtd: None,
        }
    }

    #[test]
    fn runs_save_reopen_and_migrate() {
        let dir = std::env::temp_dir().join(format!("wavey-runs-{}.wavey", std::process::id()));
        let mut doc = Document::new();
        rect(&mut doc);
        let r = tiny_result(&doc);
        // Old layout: a single result directly in results/.
        doc.save(&dir).unwrap();
        r.save(&dir.join(RESULTS_DIR)).unwrap();

        let mut back = Document::open(&dir).unwrap();
        assert_eq!(back.runs().len(), 1);
        assert!(back.results().is_some());
        assert!(!back.results_stale());
        let second = back.add_run(tiny_result(&back));
        assert_eq!(second, 2);
        back.save(&dir).unwrap();
        assert!(dir.join("results/run-001/results.json").exists());
        assert!(!dir.join("results/results.json").exists());

        let mut again = Document::open(&dir).unwrap();
        assert_eq!(again.runs().len(), 2);
        assert_eq!(again.active_run().unwrap().meta.id, 2);
        assert!(again.runs()[0].result().is_some());
        assert!(again.delete_run(2));
        assert_eq!(again.active_run().unwrap().meta.id, 1);
        again.save(&dir).unwrap();
        assert!(!dir.join("results/run-002").exists());
        std::fs::remove_dir_all(&dir).unwrap();
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
