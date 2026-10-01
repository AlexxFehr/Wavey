//! Extrusion of the selected sketch regions into a closed 3D room.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

use crate::bvh::Bvh;
use crate::regions::{HalfEdge, Region, find_regions};
use crate::sketch::{SegmentId, Sketch};

/// Stable identity of a room surface; material assignments are keyed by it.
///
/// Walls are keyed by segment and side, floors and ceilings by the interior
/// seed that selected their region, so keys survive unrelated edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SurfaceKey {
    /// `reversed` is false for the side to the left of the segment's a→b direction.
    Wall { segment: SegmentId, reversed: bool },
    Floor { seed: u64 },
    Ceiling { seed: u64 },
}

impl fmt::Display for SurfaceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SurfaceKey::Wall { segment, reversed } => {
                write!(f, "wall:{segment}:{}", if *reversed { "r" } else { "f" })
            }
            SurfaceKey::Floor { seed } => write!(f, "floor:{seed}"),
            SurfaceKey::Ceiling { seed } => write!(f, "ceiling:{seed}"),
        }
    }
}

impl FromStr for SurfaceKey {
    type Err = GeomError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || GeomError::BadSurfaceKey(s.to_string());
        let parts: Vec<&str> = s.split(':').collect();
        let num = |i: usize| parts.get(i).and_then(|p| p.parse::<u64>().ok()).ok_or_else(bad);
        match parts.first().copied() {
            Some("wall") => Ok(SurfaceKey::Wall {
                segment: num(1)?,
                reversed: match parts.get(2) {
                    Some(&"r") => true,
                    Some(&"f") => false,
                    _ => return Err(bad()),
                },
            }),
            Some("floor") => Ok(SurfaceKey::Floor { seed: num(1)? }),
            Some("ceiling") => Ok(SurfaceKey::Ceiling { seed: num(1)? }),
            _ => Err(bad()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    Wall,
    Floor,
    Ceiling,
}

#[derive(Debug, Clone)]
pub struct Surface {
    pub key: SurfaceKey,
    pub kind: SurfaceKind,
    /// Unit normal pointing into the room.
    pub normal: DVec3,
    /// Plane offset: points x on the surface satisfy `normal · x = d`.
    pub d: f64,
    /// Indices into [`Room::triangles`].
    pub triangles: Vec<u32>,
    pub area: f64,
}

impl Surface {
    pub fn mirror(&self, p: DVec3) -> DVec3 {
        p - 2.0 * (self.normal.dot(p) - self.d) * self.normal
    }

    pub fn signed_distance(&self, p: DVec3) -> f64 {
        self.normal.dot(p) - self.d
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GeomError {
    #[error("ceiling height must be positive")]
    InvalidHeight,
    #[error("no interior region selected")]
    NoInterior,
    #[error("invalid surface key `{0}`")]
    BadSurfaceKey(String),
}

#[derive(Debug, Clone)]
pub struct Room {
    pub height: f64,
    pub regions: Vec<Region>,
    pub positions: Vec<DVec3>,
    pub triangles: Vec<[u32; 3]>,
    /// Surface index of each triangle.
    pub tri_surface: Vec<u32>,
    pub surfaces: Vec<Surface>,
    pub bvh: Bvh,
}

impl Room {
    pub fn contains(&self, p: DVec3) -> bool {
        p.z > 0.0 && p.z < self.height && self.regions.iter().any(|r| r.contains(p.truncate()))
    }

    pub fn floor_area(&self) -> f64 {
        self.regions.iter().map(Region::area).sum()
    }

    pub fn volume(&self) -> f64 {
        self.floor_area() * self.height
    }

    pub fn total_surface_area(&self) -> f64 {
        self.surfaces.iter().map(|s| s.area).sum()
    }

    pub fn bounds(&self) -> (DVec3, DVec3) {
        self.positions.iter().fold(
            (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        )
    }

    pub fn surface_index(&self, key: SurfaceKey) -> Option<usize> {
        self.surfaces.iter().position(|s| s.key == key)
    }

    /// Whether `p` (assumed on the plane of `surface`) lies within its triangles.
    pub fn surface_contains(&self, surface: usize, p: DVec3) -> bool {
        let s = &self.surfaces[surface];
        s.triangles.iter().any(|&t| {
            let [a, b, c] = self.triangles[t as usize].map(|i| self.positions[i as usize]);
            point_in_triangle(p, a, b, c, s.normal)
        })
    }
}

fn point_in_triangle(p: DVec3, a: DVec3, b: DVec3, c: DVec3, n: DVec3) -> bool {
    const EPS: f64 = 1e-9;
    let e = |u: DVec3, v: DVec3| (v - u).cross(p - u).dot(n);
    let (d0, d1, d2) = (e(a, b), e(b, c), e(c, a));
    (d0 >= -EPS && d1 >= -EPS && d2 >= -EPS) || (d0 <= EPS && d1 <= EPS && d2 <= EPS)
}

struct Builder {
    positions: Vec<DVec3>,
    triangles: Vec<[u32; 3]>,
    tri_surface: Vec<u32>,
    surfaces: Vec<Surface>,
}

impl Builder {
    fn begin(&mut self, key: SurfaceKey, kind: SurfaceKind, normal: DVec3, point: DVec3) -> usize {
        self.surfaces.push(Surface {
            key,
            kind,
            normal,
            d: normal.dot(point),
            triangles: Vec::new(),
            area: 0.0,
        });
        self.surfaces.len() - 1
    }

    /// Adds a triangle, flipping its winding so its normal matches the surface.
    fn tri(&mut self, surface: usize, a: DVec3, b: DVec3, c: DVec3) {
        let n = (b - a).cross(c - a);
        let area = n.length() * 0.5;
        if area < 1e-12 {
            return;
        }
        let (b, c) = if n.dot(self.surfaces[surface].normal) < 0.0 { (c, b) } else { (b, c) };
        let base = self.positions.len() as u32;
        self.positions.extend([a, b, c]);
        self.surfaces[surface].triangles.push(self.triangles.len() as u32);
        self.surfaces[surface].area += area;
        self.triangles.push([base, base + 1, base + 2]);
        self.tri_surface.push(surface as u32);
    }

    fn wall(&mut self, key: SurfaceKey, u: DVec2, v: DVec2, height: f64) {
        let d = v - u;
        let normal = DVec3::new(-d.y, d.x, 0.0).normalize();
        let s = self.begin(key, SurfaceKind::Wall, normal, u.extend(0.0));
        let (p0, p1) = (u.extend(0.0), v.extend(0.0));
        let (p2, p3) = (v.extend(height), u.extend(height));
        self.tri(s, p0, p1, p2);
        self.tri(s, p0, p2, p3);
    }

    fn horizontal(&mut self, key: SurfaceKey, kind: SurfaceKind, region: &Region, z: f64) {
        let normal = if kind == SurfaceKind::Floor { DVec3::Z } else { DVec3::NEG_Z };
        let s = self.begin(key, kind, normal, DVec3::new(0.0, 0.0, z));
        let mut pts: Vec<DVec2> = region.outer.points.clone();
        let mut flat: Vec<f64> = pts.iter().flat_map(|p| [p.x, p.y]).collect();
        let mut hole_idx = Vec::new();
        for h in &region.holes {
            hole_idx.push(pts.len());
            pts.extend(&h.points);
            flat.extend(h.points.iter().flat_map(|p| [p.x, p.y]));
        }
        let idx = earcutr::earcut(&flat, &hole_idx, 2).unwrap_or_default();
        for t in idx.chunks_exact(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| pts[i].extend(z));
            self.tri(s, a, b, c);
        }
    }
}

/// Builds the room from the regions that contain an interior seed point.
pub fn build_room(sketch: &Sketch, height: f64, seeds: &BTreeMap<u64, DVec2>) -> Result<Room, GeomError> {
    if !(height > 0.0) {
        return Err(GeomError::InvalidHeight);
    }
    let rs = find_regions(sketch);

    // Region index → lowest seed id inside it.
    let mut interior: BTreeMap<usize, u64> = BTreeMap::new();
    for (seed, p) in seeds {
        if let Some(r) = rs.region_at(*p) {
            interior.entry(r).or_insert(*seed);
        }
    }
    if interior.is_empty() {
        return Err(GeomError::NoInterior);
    }

    let mut b = Builder {
        positions: Vec::new(),
        triangles: Vec::new(),
        tri_surface: Vec::new(),
        surfaces: Vec::new(),
    };
    let endpoints = |h: HalfEdge| {
        let (a, bb) = sketch.segment_points(h.segment).unwrap();
        if h.forward { (a, bb) } else { (bb, a) }
    };

    for (&ri, &seed) in &interior {
        let region = &rs.regions[ri];
        b.horizontal(SurfaceKey::Floor { seed }, SurfaceKind::Floor, region, 0.0);
        b.horizontal(SurfaceKey::Ceiling { seed }, SurfaceKind::Ceiling, region, height);
        for h in region.boundary() {
            let twin = HalfEdge { segment: h.segment, forward: !h.forward };
            let other = rs.left_region.get(&twin).copied();
            // Shared edges between two interior regions are open; bridges
            // (same region on both sides) are thin walls.
            let open = other.is_some_and(|o| o != ri && interior.contains_key(&o));
            if !open {
                let (u, v) = endpoints(h);
                b.wall(SurfaceKey::Wall { segment: h.segment, reversed: !h.forward }, u, v, height);
            }
        }
    }

    // Free-standing segments inside the room become two-sided walls.
    for &seg in &rs.filaments {
        let (a, c) = sketch.segment_points(seg).unwrap();
        let inside = rs.region_at((a + c) * 0.5).is_some_and(|r| interior.contains_key(&r));
        if inside {
            b.wall(SurfaceKey::Wall { segment: seg, reversed: false }, a, c, height);
            b.wall(SurfaceKey::Wall { segment: seg, reversed: true }, c, a, height);
        }
    }

    let bvh = Bvh::build(&b.positions, &b.triangles);
    let regions = interior.keys().map(|&i| rs.regions[i].clone()).collect();
    Ok(Room {
        height,
        regions,
        positions: b.positions,
        triangles: b.triangles,
        tri_surface: b.tri_surface,
        surfaces: b.surfaces,
        bvh,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn room_with_pillar() -> Room {
        let mut s = Sketch::new();
        s.add_polygon(&[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 8.0), v(0.0, 8.0)]);
        s.add_polygon(&[v(4.0, 3.0), v(5.0, 3.0), v(5.0, 4.0), v(4.0, 4.0)]);
        let seeds = BTreeMap::from([(1, v(1.0, 1.0))]);
        build_room(&s, 3.0, &seeds).unwrap()
    }

    #[test]
    fn pillar_room_surfaces() {
        let room = room_with_pillar();
        let walls = room.surfaces.iter().filter(|s| s.kind == SurfaceKind::Wall).count();
        assert_eq!(walls, 8);
        assert!((room.volume() - 79.0 * 3.0).abs() < 1e-9);
        let floor = room.surfaces.iter().find(|s| s.kind == SurfaceKind::Floor).unwrap();
        assert!((floor.area - 79.0).abs() < 1e-9);
        // 36 m outer perimeter + 4 m pillar perimeter, 3 m high.
        let wall_area: f64 = room.surfaces.iter().filter(|s| s.kind == SurfaceKind::Wall).map(|s| s.area).sum();
        assert!((wall_area - 40.0 * 3.0).abs() < 1e-9);
    }

    #[test]
    fn normals_point_into_room() {
        let room = room_with_pillar();
        for s in &room.surfaces {
            for &t in &s.triangles {
                let [a, b, c] = room.triangles[t as usize].map(|i| room.positions[i as usize]);
                let centroid = (a + b + c) / 3.0;
                let probe = centroid + s.normal * 1e-3;
                assert!(room.contains(probe), "{} normal points out", s.key);
                let geo = (b - a).cross(c - a).normalize();
                assert!(geo.dot(s.normal) > 0.999);
            }
        }
    }

    #[test]
    fn mesh_is_closed() {
        // In a closed, consistently wound mesh every directed edge is matched
        // by the same edge in the opposite direction.
        let room = room_with_pillar();
        let key = |p: DVec3| ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64, (p.z * 1e6).round() as i64);
        let mut count: std::collections::HashMap<_, i32> = Default::default();
        for t in &room.triangles {
            let p = t.map(|i| key(room.positions[i as usize]));
            for (a, b) in [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])] {
                *count.entry((a, b)).or_default() += 1;
                *count.entry((b, a)).or_default() -= 1;
            }
        }
        assert!(count.values().all(|c| *c == 0), "mesh has open edges");
        let area: DVec3 = room.triangles.iter().map(|t| {
            let [a, b, c] = t.map(|i| room.positions[i as usize]);
            (b - a).cross(c - a)
        }).sum();
        assert!(area.length() < 1e-9, "vector area of a closed surface is zero");
    }

    #[test]
    fn surface_key_round_trip() {
        for k in [
            SurfaceKey::Wall { segment: 12, reversed: true },
            SurfaceKey::Floor { seed: 3 },
            SurfaceKey::Ceiling { seed: 4 },
        ] {
            assert_eq!(k.to_string().parse::<SurfaceKey>().unwrap(), k);
        }
    }

    #[test]
    fn two_interior_regions_share_open_edge() {
        let mut s = Sketch::new();
        s.add_polygon(&[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 5.0), v(0.0, 5.0)]);
        s.add_segment(v(5.0, 0.0), v(5.0, 5.0));
        let one = build_room(&s, 3.0, &BTreeMap::from([(1, v(1.0, 1.0))])).unwrap();
        let both = build_room(&s, 3.0, &BTreeMap::from([(1, v(1.0, 1.0)), (2, v(9.0, 1.0))])).unwrap();
        let walls = |r: &Room| r.surfaces.iter().filter(|s| s.kind == SurfaceKind::Wall).count();
        assert_eq!(walls(&one), 4);
        assert_eq!(walls(&both), 6);
        assert!((both.volume() - 150.0).abs() < 1e-9);
    }
}
