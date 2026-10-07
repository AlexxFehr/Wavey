//! Importing room models (OBJ, glTF/GLB, STL, DXF) and DXF underlays,
//! checking the room mesh.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use wavey_geom::{DVec3, RepairOptions, RepairReport};
use wavey_project::import::{self, ImportOptions, Origin, RawModel, UpAxis};
use wavey_project::meta::Underlay;

use crate::{Point3, WaveyDocument, WaveyError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ImportOrigin {
    Keep,
    Center,
    FloorAtZero,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ImportOptionsRec {
    /// Metres per file unit.
    pub scale: f64,
    /// The file is Y-up (else Z-up).
    pub y_up: bool,
    pub origin: ImportOrigin,
}

impl From<&ImportOptionsRec> for ImportOptions {
    fn from(o: &ImportOptionsRec) -> Self {
        ImportOptions {
            scale: o.scale,
            up: if o.y_up { UpAxis::Y } else { UpAxis::Z },
            origin: match o.origin {
                ImportOrigin::Keep => Origin::Keep,
                ImportOrigin::Center => Origin::Center,
                ImportOrigin::FloorAtZero => Origin::FloorAtZero,
            },
        }
    }
}

impl From<ImportOptions> for ImportOptionsRec {
    fn from(o: ImportOptions) -> Self {
        ImportOptionsRec {
            scale: o.scale,
            y_up: o.up == UpAxis::Y,
            origin: match o.origin {
                Origin::Keep => ImportOrigin::Keep,
                Origin::Center => ImportOrigin::Center,
                Origin::FloorAtZero => ImportOrigin::FloorAtZero,
            },
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ImportGroupRec {
    pub name: String,
    pub faces: u32,
    /// Material to use (on input) or the guess from the name (on output).
    pub material_id: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RepairReportRec {
    pub vertices_welded: u32,
    pub degenerate_removed: u32,
    pub duplicates_removed: u32,
    pub t_junctions_fixed: u32,
    pub holes_filled: u32,
    pub open_edges: u32,
    pub non_manifold_edges: u32,
    /// Open edges as xyz pairs (metres), for drawing.
    pub open_edge_positions: Vec<f32>,
    pub watertight: bool,
}

impl From<&RepairReport> for RepairReportRec {
    fn from(r: &RepairReport) -> Self {
        RepairReportRec {
            vertices_welded: r.vertices_welded as u32,
            degenerate_removed: r.degenerate_removed as u32,
            duplicates_removed: r.duplicates_removed as u32,
            t_junctions_fixed: r.t_junctions_fixed as u32,
            holes_filled: r.holes_filled as u32,
            open_edges: r.open_edges.len() as u32,
            non_manifold_edges: r.non_manifold_edges as u32,
            open_edge_positions: r.open_edges.iter().flatten().flat_map(|p| [p.x as f32, p.y as f32, p.z as f32]).collect(),
            watertight: r.watertight(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ImportPreviewRec {
    /// The options passed in, or suggested ones.
    pub options: ImportOptionsRec,
    pub groups: Vec<ImportGroupRec>,
    pub faces: u32,
    pub lines: u32,
    /// Size of the model with these options (metres).
    pub size: Point3,
    /// Volume of the repaired room (0 if it is not closed).
    pub volume: f64,
    pub report: RepairReportRec,
    /// The file gave its units.
    pub units_from_file: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct UnderlayRec {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    /// xyz pairs (metres).
    pub positions: Vec<f32>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RoomModelRec {
    pub groups: Vec<ImportGroupRec>,
    pub faces: u32,
}

/// The last file read, so the import sheet's previews do not reparse it.
static LAST: Mutex<Option<(PathBuf, std::time::SystemTime, RawModel)>> = Mutex::new(None);

fn raw(path: &Path) -> Result<RawModel, WaveyError> {
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).map_err(WaveyError::new)?;
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, t, r)) = &*last {
        if p == path && *t == mtime {
            return Ok(r.clone());
        }
    }
    let r = import::read(path).map_err(WaveyError::new)?;
    *last = Some((path.to_path_buf(), mtime, r.clone()));
    Ok(r)
}

fn face_counts(raw: &RawModel) -> Vec<u32> {
    let mut n = vec![0u32; raw.groups.len()];
    for (_, g) in &raw.faces {
        n[*g as usize] += 1;
    }
    n
}

#[uniffi::export]
impl WaveyDocument {
    /// Reads a model file and previews the import: groups with guessed
    /// materials, size and the repair report. `options` None suggests them.
    pub fn preview_import(&self, path: String, options: Option<ImportOptionsRec>) -> Result<ImportPreviewRec, WaveyError> {
        let raw = raw(Path::new(&path))?;
        let opts: ImportOptions = options.as_ref().map(Into::into).unwrap_or_else(|| import::suggest(&raw));
        let library = self.lock().project().materials.clone();
        let counts = face_counts(&raw);
        let groups = raw
            .groups
            .iter()
            .zip(&counts)
            .filter(|(_, n)| **n > 0)
            .map(|(name, n)| ImportGroupRec { name: name.clone(), faces: *n, material_id: import::guess_material(name, &library) })
            .collect();
        let tf = import::transform(&raw, &opts);
        let pts: Vec<DVec3> = raw.vertices.iter().chain(raw.lines.iter().flatten()).map(|p| tf(*p)).collect();
        let (lo, hi) = pts.iter().fold((DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)), |(a, b), p| (a.min(*p), b.max(*p)));
        let size = if lo.x.is_finite() { hi - lo } else { DVec3::ZERO };
        let (report, volume) = if raw.faces.is_empty() {
            (RepairReport::default(), 0.0)
        } else {
            let verts: Vec<DVec3> = raw.vertices.iter().map(|p| tf(*p)).collect();
            let (v, f, report) = wavey_geom::repair(&verts, &raw.faces, RepairOptions::default());
            let volume = if report.watertight() { wavey_geom::room_from_mesh(&v, &f).map(|r| r.volume().abs()).unwrap_or(0.0) } else { 0.0 };
            (report, volume)
        };
        Ok(ImportPreviewRec {
            options: opts.into(),
            groups,
            faces: raw.faces.len() as u32,
            lines: raw.lines.len() as u32,
            size: size.into(),
            volume,
            report: (&report).into(),
            units_from_file: raw.unit_scale.is_some(),
        })
    }

    /// Replaces the room with the model in `path` (repaired), with the
    /// given material per group name. Undoable.
    pub fn import_room(&self, path: String, options: ImportOptionsRec, groups: Vec<ImportGroupRec>) -> Result<RepairReportRec, WaveyError> {
        let raw = raw(Path::new(&path))?;
        let mats: BTreeMap<String, String> = groups.into_iter().map(|g| (g.name, g.material_id)).collect();
        let mut doc = self.lock();
        let library = doc.project().materials.clone();
        let (mesh, report) = import::to_mesh(&raw, &(&options).into(), &mats, &library).map_err(WaveyError::new)?;
        mesh.build().map_err(WaveyError::new)?;
        doc.edit(|p| p.mesh = Some(mesh));
        doc.room().map_err(WaveyError::new)?;
        Ok((&report).into())
    }

    /// Adds the lines of a DXF (or the edges of any model) as underlay.
    pub fn import_underlay(&self, path: String, options: ImportOptionsRec) -> Result<u64, WaveyError> {
        let raw = raw(Path::new(&path))?;
        let mut lines = import::to_lines(&raw, &(&options).into());
        if lines.is_empty() {
            // A mesh file: its polygon edges.
            let tf = import::transform(&raw, &(&options).into());
            for (poly, _) in &raw.faces {
                for i in 0..poly.len() {
                    lines.push([tf(raw.vertices[poly[i] as usize]), tf(raw.vertices[poly[(i + 1) % poly.len()] as usize])]);
                }
            }
        }
        if lines.is_empty() {
            return Err(WaveyError::new("the file has no lines"));
        }
        let name = Path::new(&path).file_stem().and_then(|s| s.to_str()).unwrap_or("Underlay").to_string();
        let mut doc = self.lock();
        Ok(doc.edit(|p| {
            let id = p.alloc_id();
            p.meta.underlays.push(Underlay { id, name, visible: true, lines: lines.iter().map(|l| [l[0].to_array(), l[1].to_array()]).collect() });
            id
        }))
    }

    pub fn underlays(&self) -> Vec<UnderlayRec> {
        self.lock()
            .project()
            .meta
            .underlays
            .iter()
            .map(|u| UnderlayRec {
                id: u.id,
                name: u.name.clone(),
                visible: u.visible,
                positions: u.lines.iter().flatten().flatten().map(|v| *v as f32).collect(),
            })
            .collect()
    }

    pub fn set_underlay_visible(&self, id: u64, visible: bool) {
        self.lock().edit(|p| {
            if let Some(u) = p.meta.underlays.iter_mut().find(|u| u.id == id) {
                u.visible = visible;
            }
        });
    }

    pub fn remove_underlay(&self, id: u64) {
        self.lock().edit(|p| p.meta.underlays.retain(|u| u.id != id));
    }

    /// The imported room model, if the room is one (else the sketch is used).
    pub fn room_model(&self) -> Option<RoomModelRec> {
        let doc = self.lock();
        let m = doc.project().mesh.as_ref()?;
        let mut counts = vec![0u32; m.groups.len()];
        m.faces.iter().for_each(|f| counts[f.group as usize] += 1);
        Some(RoomModelRec {
            groups: m.groups.iter().zip(counts).map(|(g, n)| ImportGroupRec { name: g.name.clone(), faces: n, material_id: g.material.clone() }).collect(),
            faces: m.faces.len() as u32,
        })
    }

    /// Sets an imported model group's material (surfaces without their own
    /// assignment follow it).
    pub fn set_room_model_group_material(&self, group: String, material_id: String) -> Result<(), WaveyError> {
        self.lock()
            .try_edit(|p| {
                if !p.materials.contains_key(&material_id) {
                    return Err(format!("no material `{material_id}`"));
                }
                let m = p.mesh.as_mut().ok_or("the room is not an imported model")?;
                let g = m.groups.iter_mut().find(|g| g.name == group).ok_or(format!("no group `{group}`"))?;
                g.material = material_id;
                Ok(())
            })
            .map_err(WaveyError::new)
    }

    /// Goes back to the drawn (sketched) room. Undoable.
    pub fn remove_room_model(&self) {
        self.lock().edit(|p| p.mesh = None);
    }

    /// Checks the imported room model for gaps (without changing it).
    pub fn check_room_model(&self) -> Option<RepairReportRec> {
        let doc = self.lock();
        let m = doc.project().mesh.as_ref()?;
        let v: Vec<DVec3> = m.vertices.iter().map(|p| DVec3::from_array(*p)).collect();
        let f: Vec<(Vec<u32>, u32)> = m.faces.iter().map(|f| (f.v.clone(), f.group)).collect();
        let (_, _, report) = wavey_geom::repair(&v, &f, RepairOptions { max_hole_edges: 0, weld: 1e-6 });
        Some((&report).into())
    }
}
