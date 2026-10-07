//! Plain data records passed across the FFI boundary.

use wavey_geom::{DVec2, Room, SurfaceKey, SurfaceKind};
use wavey_materials::{Material, Medium, Spectrum};

#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct Point2 {
    pub x: f64,
    pub y: f64,
}

impl From<Point2> for DVec2 {
    fn from(p: Point2) -> Self {
        DVec2::new(p.x, p.y)
    }
}

impl From<DVec2> for Point2 {
    fn from(p: DVec2) -> Self {
        Point2 { x: p.x, y: p.y }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct Point3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl From<Point3> for wavey_geom::DVec3 {
    fn from(p: Point3) -> Self {
        wavey_geom::DVec3::new(p.x, p.y, p.z)
    }
}

impl From<wavey_geom::DVec3> for Point3 {
    fn from(p: wavey_geom::DVec3) -> Self {
        Point3 {
            x: p.x,
            y: p.y,
            z: p.z,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SketchVertex {
    pub id: u64,
    pub p: Point2,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SketchSegment {
    pub id: u64,
    pub a: u64,
    pub b: u64,
    pub pa: Point2,
    pub pb: Point2,
    pub layer_id: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SketchRegion {
    pub outer: Vec<Point2>,
    pub holes: Vec<Vec<Point2>>,
    pub area: f64,
    pub interior: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SketchSnapshot {
    pub vertices: Vec<SketchVertex>,
    pub segments: Vec<SketchSegment>,
    pub regions: Vec<SketchRegion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SurfaceKindRec {
    Wall,
    Floor,
    Ceiling,
}

impl From<SurfaceKind> for SurfaceKindRec {
    fn from(k: SurfaceKind) -> Self {
        match k {
            SurfaceKind::Wall => Self::Wall,
            SurfaceKind::Floor => Self::Floor,
            SurfaceKind::Ceiling => Self::Ceiling,
        }
    }
}

/// One planar room surface as a standalone triangle mesh (meters, z up).
#[derive(Debug, Clone, uniffi::Record)]
pub struct SurfaceMesh {
    pub key: String,
    /// Display code ("W-07"), stable across edits.
    pub code: String,
    /// User name; empty if unnamed.
    pub name: String,
    pub layer_id: String,
    /// Sketch segment of a wall.
    pub segment_id: Option<u64>,
    pub kind: SurfaceKindRec,
    pub area: f64,
    pub material_id: String,
    pub normal: Point3,
    /// xyz triples.
    pub positions: Vec<f32>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RoomSnapshot {
    pub surfaces: Vec<SurfaceMesh>,
    pub height: f64,
    pub volume: f64,
    pub floor_area: f64,
    pub total_surface_area: f64,
    pub bounds_min: Point3,
    pub bounds_max: Point3,
}

impl RoomSnapshot {
    pub fn from_room(room: &Room, project: &wavey_project::Project) -> Self {
        use wavey_project::meta::{LAYER_CEILING, LAYER_FLOOR};
        let p3 = |v: wavey_geom::DVec3| Point3 {
            x: v.x,
            y: v.y,
            z: v.z,
        };
        let surfaces = room
            .surfaces
            .iter()
            .map(|s| {
                let mut positions = Vec::with_capacity(s.triangles.len() * 9);
                for &t in &s.triangles {
                    for i in room.triangles[t as usize] {
                        let p = room.positions[i as usize];
                        positions.extend([p.x as f32, p.y as f32, p.z as f32]);
                    }
                }
                let indices = (0..(s.triangles.len() * 3) as u32).collect();
                let key = s.key.to_string();
                let (layer_id, segment_id) = match s.key {
                    SurfaceKey::Wall { segment, .. } => (
                        project.meta.segment_layer(segment).to_string(),
                        Some(segment),
                    ),
                    SurfaceKey::Floor { .. } => (LAYER_FLOOR.to_string(), None),
                    SurfaceKey::Ceiling { .. } => (LAYER_CEILING.to_string(), None),
                    SurfaceKey::Mesh { .. } | SurfaceKey::Feature { .. } => (
                        match s.kind {
                            SurfaceKind::Floor => LAYER_FLOOR,
                            SurfaceKind::Ceiling => LAYER_CEILING,
                            SurfaceKind::Wall => wavey_project::meta::LAYER_WALLS,
                        }
                        .to_string(),
                        None,
                    ),
                };
                SurfaceMesh {
                    code: project.meta.code(&key).unwrap_or_default().to_string(),
                    name: project
                        .meta
                        .surface_names
                        .get(&key)
                        .cloned()
                        .unwrap_or_default(),
                    layer_id,
                    segment_id,
                    key,
                    kind: s.kind.into(),
                    area: s.area,
                    material_id: project.material_for(&s.key).id.clone(),
                    normal: p3(s.normal),
                    positions,
                    indices,
                }
            })
            .collect();
        let (lo, hi) = room.bounds();
        RoomSnapshot {
            surfaces,
            height: room.height,
            volume: room.volume(),
            floor_area: room.floor_area(),
            total_surface_area: room.total_surface_area(),
            bounds_min: p3(lo),
            bounds_max: p3(hi),
        }
    }
}

/// A frequency-dependent value. An empty `freqs_hz` means a single scalar `values[0]`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SpectrumRec {
    pub freqs_hz: Vec<f64>,
    pub values: Vec<f64>,
}

impl From<&Spectrum> for SpectrumRec {
    fn from(s: &Spectrum) -> Self {
        match s {
            Spectrum::Scalar(v) => SpectrumRec {
                freqs_hz: vec![],
                values: vec![*v],
            },
            Spectrum::Bands { freqs_hz, values } => SpectrumRec {
                freqs_hz: freqs_hz.clone(),
                values: values.clone(),
            },
        }
    }
}

impl From<SpectrumRec> for Spectrum {
    fn from(s: SpectrumRec) -> Self {
        if s.freqs_hz.is_empty() {
            Spectrum::Scalar(s.values.first().copied().unwrap_or(0.0))
        } else {
            Spectrum::Bands {
                freqs_hz: s.freqs_hz,
                values: s.values,
            }
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MaterialRec {
    pub id: String,
    pub name: String,
    pub absorption: SpectrumRec,
    pub scattering: SpectrumRec,
    pub angle_dependent: bool,
    pub color: String,
    pub builtin: bool,
    pub source: Option<String>,
}

impl From<&Material> for MaterialRec {
    fn from(m: &Material) -> Self {
        MaterialRec {
            id: m.id.clone(),
            name: m.name.clone(),
            absorption: (&m.absorption).into(),
            scattering: (&m.scattering).into(),
            angle_dependent: m.angle_dependent,
            color: m.color.clone(),
            builtin: m.builtin,
            source: m.source.clone(),
        }
    }
}

impl MaterialRec {
    pub fn into_material(self) -> Material {
        Material {
            id: self.id,
            name: self.name,
            absorption: self.absorption.into(),
            scattering: self.scattering.into(),
            impedance: None,
            angle_dependent: self.angle_dependent,
            color: self.color,
            builtin: false,
            source: self.source,
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct MediumRec {
    pub temperature_c: f64,
    pub relative_humidity_pct: f64,
    pub pressure_kpa: f64,
    /// Derived, ignored on input.
    pub speed_of_sound: f64,
}

impl From<Medium> for MediumRec {
    fn from(m: Medium) -> Self {
        MediumRec {
            temperature_c: m.temperature_c,
            relative_humidity_pct: m.relative_humidity_pct,
            pressure_kpa: m.pressure_kpa,
            speed_of_sound: m.speed_of_sound(),
        }
    }
}

impl From<MediumRec> for Medium {
    fn from(m: MediumRec) -> Self {
        Medium {
            temperature_c: m.temperature_c,
            relative_humidity_pct: m.relative_humidity_pct,
            pressure_kpa: m.pressure_kpa,
        }
    }
}
