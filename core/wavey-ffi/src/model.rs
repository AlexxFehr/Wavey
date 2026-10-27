//! Modelling features (rake, balcony, vault, dome, column, beam, extrude,
//! revolve, panel, imprint): list, add, edit, reorder.

use wavey_geom::{DVec2, DVec3, SurfaceKey};
use wavey_project::model::{Feature, FeatureKind, Op, PlanAxis, WorkPlane};

use crate::{Point2, Point3, WaveyDocument, WaveyError};

#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct WorkPlaneRec {
    pub origin: Point3,
    pub u: Point3,
    pub v: Point3,
}

impl From<WorkPlane> for WorkPlaneRec {
    fn from(w: WorkPlane) -> Self {
        WorkPlaneRec { origin: w.origin.into(), u: w.u.into(), v: w.v.into() }
    }
}

impl From<WorkPlaneRec> for WorkPlane {
    fn from(w: WorkPlaneRec) -> Self {
        WorkPlane { origin: w.origin.into(), u: DVec3::from(w.u).normalize_or(DVec3::X), v: DVec3::from(w.v).normalize_or(DVec3::Y) }
    }
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum FeatureKindRec {
    Extrude { plane: WorkPlaneRec, outline: Vec<Point2>, holes: Vec<Vec<Point2>>, from: f64, to: f64, slope: f64, slope_dir: Point2 },
    Revolve { plane: WorkPlaneRec, outline: Vec<Point2>, axis_a: Point2, axis_b: Point2, angle_deg: f64, segments: u32 },
    /// An imported mesh as a solid (its faces are kept on update).
    Body { faces: u32 },
    Rake { min: Point2, max: Point2, along_x: bool, reverse: bool, base: f64, rows: u32, riser: f64, stepped: bool },
    Balcony { outline: Vec<Point2>, underside: f64, thickness: f64, parapet: f64 },
    Vault { min: Point2, max: Point2, along_x: bool, spring: f64, rise: f64, segments: u32 },
    Dome { centre: Point2, radius: f64, spring: f64, rise: f64, segments: u32 },
    Column { centre: Point2, size: Point2, round: bool, bottom: f64, top: f64 },
    Beam { a: Point2, b: Point2, width: f64, bottom: f64, depth: f64 },
    Panel { plane: WorkPlaneRec, outline: Vec<Point2> },
    Imprint { target: String, plane: WorkPlaneRec, outline: Vec<Point2> },
    /// A standing crowd over a floor outline, in people per m².
    Crowd { outline: Vec<Point2>, density: f64 },
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FeatureRec {
    pub id: u64,
    pub name: String,
    /// Takes air away (else adds it).
    pub subtract: bool,
    pub suppressed: bool,
    /// The feature's material (None: the default material).
    pub material_id: Option<String>,
    pub kind: FeatureKindRec,
    /// "Rake", "Balcony", …; ignored on input.
    pub label: String,
    /// Why the feature cannot be built; ignored on input.
    pub error: Option<String>,
}

fn pts(v: &[DVec2]) -> Vec<Point2> {
    v.iter().map(|p| (*p).into()).collect()
}

fn dpts(v: &[Point2]) -> Vec<DVec2> {
    v.iter().map(|p| (*p).into()).collect()
}

fn axis(x: bool) -> PlanAxis {
    if x { PlanAxis::X } else { PlanAxis::Y }
}

impl From<&FeatureKind> for FeatureKindRec {
    fn from(k: &FeatureKind) -> Self {
        match k {
            FeatureKind::Extrude { plane, outline, holes, from, to, slope, slope_dir } => FeatureKindRec::Extrude {
                plane: (*plane).into(),
                outline: pts(outline),
                holes: holes.iter().map(|h| pts(h)).collect(),
                from: *from,
                to: *to,
                slope: *slope,
                slope_dir: (*slope_dir).into(),
            },
            FeatureKind::Revolve { plane, outline, axis, angle_deg, segments } => FeatureKindRec::Revolve {
                plane: (*plane).into(),
                outline: pts(outline),
                axis_a: axis[0].into(),
                axis_b: axis[1].into(),
                angle_deg: *angle_deg,
                segments: *segments,
            },
            FeatureKind::Body { mesh } => FeatureKindRec::Body { faces: mesh.faces.len() as u32 },
            FeatureKind::Rake { min, max, axis, reverse, base, rows, riser, stepped } => FeatureKindRec::Rake {
                min: (*min).into(),
                max: (*max).into(),
                along_x: *axis == PlanAxis::X,
                reverse: *reverse,
                base: *base,
                rows: *rows,
                riser: *riser,
                stepped: *stepped,
            },
            FeatureKind::Balcony { outline, underside, thickness, parapet } => {
                FeatureKindRec::Balcony { outline: pts(outline), underside: *underside, thickness: *thickness, parapet: *parapet }
            }
            FeatureKind::Vault { min, max, axis, spring, rise, segments } => FeatureKindRec::Vault {
                min: (*min).into(),
                max: (*max).into(),
                along_x: *axis == PlanAxis::X,
                spring: *spring,
                rise: *rise,
                segments: *segments,
            },
            FeatureKind::Dome { centre, radius, spring, rise, segments } => {
                FeatureKindRec::Dome { centre: (*centre).into(), radius: *radius, spring: *spring, rise: *rise, segments: *segments }
            }
            FeatureKind::Column { centre, size, round, bottom, top } => {
                FeatureKindRec::Column { centre: (*centre).into(), size: (*size).into(), round: *round, bottom: *bottom, top: *top }
            }
            FeatureKind::Beam { a, b, width, bottom, depth } => {
                FeatureKindRec::Beam { a: (*a).into(), b: (*b).into(), width: *width, bottom: *bottom, depth: *depth }
            }
            FeatureKind::Panel { plane, outline } => FeatureKindRec::Panel { plane: (*plane).into(), outline: pts(outline) },
            FeatureKind::Imprint { target, plane, outline } => {
                FeatureKindRec::Imprint { target: target.to_string(), plane: (*plane).into(), outline: pts(outline) }
            }
            FeatureKind::Crowd { outline, density } => FeatureKindRec::Crowd { outline: pts(outline), density: *density },
        }
    }
}

/// The core kind from a record; a body keeps `old`'s mesh.
fn kind_from(r: FeatureKindRec, old: Option<&FeatureKind>) -> Result<FeatureKind, String> {
    Ok(match r {
        FeatureKindRec::Extrude { plane, outline, holes, from, to, slope, slope_dir } => FeatureKind::Extrude {
            plane: plane.into(),
            outline: dpts(&outline),
            holes: holes.iter().map(|h| dpts(h)).collect(),
            from,
            to,
            slope,
            slope_dir: slope_dir.into(),
        },
        FeatureKindRec::Revolve { plane, outline, axis_a, axis_b, angle_deg, segments } => {
            FeatureKind::Revolve { plane: plane.into(), outline: dpts(&outline), axis: [axis_a.into(), axis_b.into()], angle_deg, segments }
        }
        FeatureKindRec::Body { .. } => match old {
            Some(k @ FeatureKind::Body { .. }) => k.clone(),
            _ => return Err("a body comes from an imported model".into()),
        },
        FeatureKindRec::Rake { min, max, along_x, reverse, base, rows, riser, stepped } => {
            FeatureKind::Rake { min: min.into(), max: max.into(), axis: axis(along_x), reverse, base, rows, riser, stepped }
        }
        FeatureKindRec::Balcony { outline, underside, thickness, parapet } => FeatureKind::Balcony { outline: dpts(&outline), underside, thickness, parapet },
        FeatureKindRec::Vault { min, max, along_x, spring, rise, segments } => {
            FeatureKind::Vault { min: min.into(), max: max.into(), axis: axis(along_x), spring, rise, segments }
        }
        FeatureKindRec::Dome { centre, radius, spring, rise, segments } => FeatureKind::Dome { centre: centre.into(), radius, spring, rise, segments },
        FeatureKindRec::Column { centre, size, round, bottom, top } => FeatureKind::Column { centre: centre.into(), size: size.into(), round, bottom, top },
        FeatureKindRec::Beam { a, b, width, bottom, depth } => FeatureKind::Beam { a: a.into(), b: b.into(), width, bottom, depth },
        FeatureKindRec::Panel { plane, outline } => FeatureKind::Panel { plane: plane.into(), outline: dpts(&outline) },
        FeatureKindRec::Imprint { target, plane, outline } => FeatureKind::Imprint {
            target: target.parse::<SurfaceKey>().map_err(|e| e.to_string())?,
            plane: plane.into(),
            outline: dpts(&outline),
        },
        FeatureKindRec::Crowd { outline, density } => FeatureKind::Crowd { outline: dpts(&outline), density: density.clamp(0.1, 8.0) },
    })
}

fn rec(f: &Feature) -> FeatureRec {
    FeatureRec {
        id: f.id,
        name: f.name.clone(),
        subtract: f.op == Op::Subtract,
        suppressed: f.suppressed,
        material_id: f.material.clone(),
        kind: (&f.kind).into(),
        label: f.kind.label().into(),
        error: f.solids().err(),
    }
}

#[uniffi::export]
impl WaveyDocument {
    /// Modelling features in evaluation order.
    pub fn features(&self) -> Vec<FeatureRec> {
        self.lock().project().features.iter().map(rec).collect()
    }

    /// Adds a feature (adding or taking air as usual for its kind) and returns its id.
    pub fn add_feature(&self, kind: FeatureKindRec, name: Option<String>) -> Result<u64, WaveyError> {
        let k = kind_from(kind, None).map_err(WaveyError::new)?;
        Ok(self.lock().edit(|p| p.add_feature(k, name)))
    }

    pub fn update_feature(&self, feature: FeatureRec) -> Result<(), WaveyError> {
        self.lock()
            .try_edit(|p| {
                let old = p.features.iter().find(|f| f.id == feature.id).ok_or("no such feature")?;
                if let Some(m) = &feature.material_id {
                    if !p.materials.contains_key(m) {
                        return Err(format!("no material `{m}`"));
                    }
                }
                let kind = kind_from(feature.kind, Some(&old.kind))?;
                p.update_feature(Feature {
                    id: feature.id,
                    name: feature.name,
                    op: if feature.subtract { Op::Subtract } else { Op::Add },
                    suppressed: feature.suppressed,
                    material: feature.material_id,
                    kind,
                })
            })
            .map_err(WaveyError::new)
    }

    pub fn remove_features(&self, ids: Vec<u64>) {
        self.lock().edit(|p| ids.iter().for_each(|id| p.remove_feature(*id)));
    }

    /// Moves a feature to `index` in the evaluation order.
    pub fn move_feature(&self, id: u64, index: u32) {
        self.lock().edit(|p| p.move_feature(id, index as usize));
    }

    /// Adds the imported room model in `path` as a body feature (added air,
    /// e.g. a stage house; or subtracted, e.g. seating) instead of
    /// replacing the room.
    pub fn import_body(&self, path: String, options: crate::ImportOptionsRec, subtract: bool) -> Result<u64, WaveyError> {
        let raw = wavey_project::import::read(std::path::Path::new(&path)).map_err(WaveyError::new)?;
        let mut doc = self.lock();
        let library = doc.project().materials.clone();
        let (mesh, _) = wavey_project::import::to_mesh(&raw, &(&options).into(), &Default::default(), &library).map_err(WaveyError::new)?;
        let name = std::path::Path::new(&path).file_stem().and_then(|s| s.to_str()).map(str::to_string);
        Ok(doc.edit(|p| {
            let id = p.add_feature(FeatureKind::Body { mesh }, name);
            if let Some(f) = p.features.iter_mut().find(|f| f.id == id) {
                f.op = if subtract { Op::Subtract } else { Op::Add };
            }
            id
        }))
    }

    /// The feature a room surface comes from (`feat:` keys).
    pub fn feature_of_surface(&self, surface_key: String) -> Option<u64> {
        match surface_key.parse::<SurfaceKey>().ok()? {
            SurfaceKey::Feature { feature, .. } => Some(feature),
            _ => None,
        }
    }
}
