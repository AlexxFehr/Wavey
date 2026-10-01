//! Plain data records passed across the FFI boundary.

use wavey_geom::{DVec2, Room, SurfaceKind};

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
    pub kind: SurfaceKindRec,
    pub area: f64,
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
    pub fn from_room(room: &Room) -> Self {
        let p3 = |v: wavey_geom::DVec3| Point3 { x: v.x, y: v.y, z: v.z };
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
                SurfaceMesh {
                    key: s.key.to_string(),
                    kind: s.kind.into(),
                    area: s.area,
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
