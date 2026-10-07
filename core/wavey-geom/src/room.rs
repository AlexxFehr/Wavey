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
    /// One planar surface of an imported mesh: material group and a hash of
    /// its plane (so the key survives re-imports and reordered faces).
    Mesh { group: u32, index: u32 },
    /// A face of a modelling feature; curved faces are split into facets
    /// (one plane each) that share the face's material.
    Feature { feature: u64, face: u32, facet: u32 },
}

impl SurfaceKey {
    /// The key materials are assigned by: a curved face's facets share the
    /// face's (facet 0).
    pub fn material_key(&self) -> SurfaceKey {
        match *self {
            SurfaceKey::Feature { feature, face, .. } => SurfaceKey::Feature { feature, face, facet: 0 },
            k => k,
        }
    }
}

impl fmt::Display for SurfaceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SurfaceKey::Wall { segment, reversed } => {
                write!(f, "wall:{segment}:{}", if *reversed { "r" } else { "f" })
            }
            SurfaceKey::Floor { seed } => write!(f, "floor:{seed}"),
            SurfaceKey::Ceiling { seed } => write!(f, "ceiling:{seed}"),
            SurfaceKey::Mesh { group, index } => write!(f, "mesh:{group}:{index}"),
            SurfaceKey::Feature { feature, face, facet: 0 } => write!(f, "feat:{feature}:{face}"),
            SurfaceKey::Feature { feature, face, facet } => write!(f, "feat:{feature}:{face}:{facet}"),
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
            Some("mesh") => Ok(SurfaceKey::Mesh { group: num(1)? as u32, index: num(2)? as u32 }),
            Some("feat") => Ok(SurfaceKey::Feature {
                feature: num(1)?,
                face: num(2)? as u32,
                facet: if parts.len() > 3 { num(3)? as u32 } else { 0 },
            }),
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
    /// One side of a thin panel (a reflector, canopy or free-standing wall).
    pub panel: bool,
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
    #[error("{0}")]
    Feature(String),
}

#[derive(Debug, Clone)]
pub struct Room {
    /// Extruded from a sketch: vertical walls, flat floor at 0 and ceiling
    /// at `height`. False for imported meshes (`height` is then the extent in z).
    pub prism: bool,
    pub height: f64,
    pub regions: Vec<Region>,
    pub positions: Vec<DVec3>,
    pub triangles: Vec<[u32; 3]>,
    /// Surface index of each triangle.
    pub tri_surface: Vec<u32>,
    pub surfaces: Vec<Surface>,
    pub bvh: Bvh,
    /// Mesh rooms: each surface's key in the old numbering (by discovery
    /// order) and its current key, for migrating saved assignments.
    pub legacy_keys: Vec<(SurfaceKey, SurfaceKey)>,
}

impl Room {
    pub fn contains(&self, p: DVec3) -> bool {
        if self.prism {
            p.z > 0.0 && p.z < self.height && self.regions.iter().any(|r| r.contains(p.truncate()))
        } else {
            parity_inside(&self.bvh, p)
        }
    }

    pub fn floor_area(&self) -> f64 {
        if self.prism {
            self.regions.iter().map(Region::area).sum()
        } else {
            self.surfaces.iter().filter(|s| s.kind == SurfaceKind::Floor).map(|s| s.area).sum()
        }
    }

    pub fn volume(&self) -> f64 {
        if self.prism {
            return self.floor_area() * self.height;
        }
        // Divergence theorem with inward normals: V = −⅓ Σ (x·n) A.
        -self
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| self.positions[i as usize]);
                a.dot((b - a).cross(c - a)) / 6.0
            })
            .sum::<f64>()
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
            panel: false,
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
        prism: true,
        height,
        regions,
        positions: b.positions,
        triangles: b.triangles,
        tri_surface: b.tri_surface,
        surfaces: b.surfaces,
        bvh,
        legacy_keys: Vec::new(),
    })
}

/// Whether `p` is inside a closed mesh: an odd number of crossings along a
/// ray, by majority over three rays (robust to a ray grazing an edge or
/// slipping through a small gap). Coincident triangles (both sides of a
/// thin panel) each count, so panels do not change the answer.
fn parity_inside(bvh: &Bvh, p: DVec3) -> bool {
    // Irrational-ish directions so rays do not run along edges.
    const DIRS: [[f64; 3]; 3] = [
        [0.577_215_664_9, 0.618_033_988_7, 0.533_831_9],
        [-0.693_147_180_6, 0.414_213_562_4, -0.301_029_995_7],
        [0.236_067_977_5, -0.732_050_807_6, 0.645_751_311_1],
    ];
    let odd = DIRS
        .iter()
        .filter(|d| bvh.all_hits(p, DVec3::from_array(**d).normalize(), 1e-9, f64::INFINITY).len() % 2 == 1)
        .count();
    odd >= 2
}

/// Triangles of a planar polygon (convex or not): earcut in its plane
/// (Newell normal), keeping the polygon's winding.
pub fn triangulate(poly: &[DVec3]) -> Vec<[DVec3; 3]> {
    match poly.len() {
        0..=2 => return vec![],
        3 => return vec![[poly[0], poly[1], poly[2]]],
        _ => {}
    }
    let mut n = DVec3::ZERO;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        n += DVec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
    }
    let n = n.normalize_or_zero();
    if n == DVec3::ZERO {
        return vec![];
    }
    let u = n.any_orthonormal_vector();
    let v = n.cross(u);
    let flat: Vec<f64> = poly.iter().flat_map(|p| [p.dot(u), p.dot(v)]).collect();
    let idx = earcutr::earcut(&flat, &[], 2).unwrap_or_default();
    idx.chunks_exact(3)
        .map(|c| {
            let t = [poly[c[0]], poly[c[1]], poly[c[2]]];
            // Earcut may return either orientation; match the polygon's.
            if (t[1] - t[0]).cross(t[2] - t[0]).dot(n) < 0.0 { [t[0], t[2], t[1]] } else { t }
        })
        .collect()
}

/// Builds a room from a closed polygon mesh (any orientation). `faces` are
/// vertex-index polygons with a material group each. Coplanar faces of a
/// group form one surface; normals are made to point into the room; floors,
/// ceilings and walls are told apart by their normals.
pub fn room_from_mesh(vertices: &[DVec3], faces: &[(Vec<u32>, u32)]) -> Result<Room, GeomError> {
    let mut tris: Vec<([DVec3; 3], u32)> = Vec::new();
    for (poly, g) in faces {
        for t in triangulate(&poly.iter().map(|&i| vertices[i as usize]).collect::<Vec<_>>()) {
            if (t[1] - t[0]).cross(t[2] - t[0]).length() > 1e-12 {
                tris.push((t, *g));
            }
        }
    }
    if tris.is_empty() {
        return Err(GeomError::NoInterior);
    }
    let flat: Vec<DVec3> = tris.iter().flat_map(|(t, _)| *t).collect();
    let idx: Vec<[u32; 3]> = (0..tris.len() as u32).map(|i| [3 * i, 3 * i + 1, 3 * i + 2]).collect();
    let probe = Bvh::build(&flat, &idx);
    let (lo, hi) = flat.iter().fold((DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)), |(a, b), p| (a.min(*p), b.max(*p)));
    let eps = (hi - lo).length() * 1e-5;
    let mut b = Builder { positions: Vec::new(), triangles: Vec::new(), tri_surface: Vec::new(), surfaces: Vec::new() };
    // Surface per (group, plane).
    let mut planes: Vec<(u32, DVec3, f64, usize)> = Vec::new();
    let mut counts: BTreeMap<u32, u32> = BTreeMap::new();
    let mut legacy_keys = Vec::new();
    for (t, g) in &tris {
        let c = (t[0] + t[1] + t[2]) / 3.0;
        let mut n = (t[1] - t[0]).cross(t[2] - t[0]).normalize();
        if !parity_inside(&probe, c + n * eps) {
            n = -n;
        }
        let d = n.dot(c);
        let s = match planes.iter().find(|(pg, pn, pd, _)| pg == g && pn.dot(n) > 1.0 - 1e-6 && (pd - d).abs() < 1e-4) {
            Some(x) => x.3,
            None => {
                let index = counts.entry(*g).or_insert(0);
                let kind = if n.z > 0.7 { SurfaceKind::Floor } else if n.z < -0.7 { SurfaceKind::Ceiling } else { SurfaceKind::Wall };
                let mut key = SurfaceKey::Mesh { group: *g, index: plane_hash(n, d) };
                while b.surfaces.iter().any(|s| s.key == key) {
                    if let SurfaceKey::Mesh { index, .. } = &mut key {
                        *index = index.wrapping_add(1);
                    }
                }
                legacy_keys.push((SurfaceKey::Mesh { group: *g, index: *index }, key));
                let s = b.begin(key, kind, n, c);
                *index += 1;
                planes.push((*g, n, d, s));
                s
            }
        };
        b.tri(s, t[0], t[1], t[2]);
    }
    let bvh = Bvh::build(&b.positions, &b.triangles);
    Ok(Room {
        prism: false,
        height: hi.z - lo.z,
        regions: Vec::new(),
        positions: b.positions,
        triangles: b.triangles,
        tri_surface: b.tri_surface,
        surfaces: b.surfaces,
        bvh,
        legacy_keys,
    })
}

/// A room from tagged polygons whose normals point out of the air (as
/// from [`crate::csg`]): polygons with the same key form one surface (a key
/// found on a second plane becomes another facet). `panels` are thin
/// surfaces given as is: triangles with their key and normal into the air.
/// T-junctions between the pieces are closed first.
pub fn room_from_polygons(polys: &[(Vec<DVec3>, SurfaceKey)], panels: &[([DVec3; 3], SurfaceKey, DVec3)]) -> Result<Room, GeomError> {
    if polys.is_empty() {
        return Err(GeomError::NoInterior);
    }
    // Weld and split T-junctions (groups index `keys`).
    let mut keys: Vec<SurfaceKey> = Vec::new();
    let mut verts: Vec<DVec3> = Vec::new();
    let mut faces: Vec<(Vec<u32>, u32)> = Vec::new();
    for (p, k) in polys {
        let g = match keys.iter().position(|x| x == k) {
            Some(i) => i,
            None => {
                keys.push(*k);
                keys.len() - 1
            }
        } as u32;
        let base = verts.len() as u32;
        verts.extend_from_slice(p);
        faces.push(((0..p.len() as u32).map(|i| base + i).collect(), g));
    }
    let (v, f, _) = crate::repair::repair(&verts, &faces, crate::repair::RepairOptions { weld: 1e-5, max_hole_edges: 0 });

    let mut b = Builder { positions: Vec::new(), triangles: Vec::new(), tri_surface: Vec::new(), surfaces: Vec::new() };
    // (key, normal into the air, d) → surface.
    let mut planes: Vec<(SurfaceKey, DVec3, f64, usize)> = Vec::new();
    for (poly, g) in &f {
        let pts: Vec<DVec3> = poly.iter().map(|&i| v[i as usize]).collect();
        let mut n = DVec3::ZERO;
        for i in 0..pts.len() {
            let (a, c) = (pts[i], pts[(i + 1) % pts.len()]);
            n += DVec3::new((a.y - c.y) * (a.z + c.z), (a.z - c.z) * (a.x + c.x), (a.x - c.x) * (a.y + c.y));
        }
        if n.length() < 1e-12 {
            continue;
        }
        let n = -n.normalize();
        let c = pts.iter().sum::<DVec3>() / pts.len() as f64;
        let d = n.dot(c);
        let key = keys[*g as usize];
        let s = match planes.iter().find(|(k, pn, pd, _)| *k == key && pn.dot(n) > 1.0 - 1e-6 && (pd - d).abs() < 1e-4) {
            Some(x) => x.3,
            None => {
                // The same key on another plane: the next free facet.
                let mut k = key;
                while planes.iter().any(|x| x.0 == k) {
                    match &mut k {
                        SurfaceKey::Feature { facet, .. } => *facet += 1,
                        _ => break,
                    }
                }
                let kind = if n.z > 0.7 { SurfaceKind::Floor } else if n.z < -0.7 { SurfaceKind::Ceiling } else { SurfaceKind::Wall };
                let s = b.begin(k, kind, n, c);
                planes.push((k, n, d, s));
                s
            }
        };
        for t in triangulate(&pts) {
            b.tri(s, t[0], t[1], t[2]);
        }
    }
    for (t, key, n) in panels {
        let s = match b.surfaces.iter().position(|s| s.key == *key) {
            Some(s) => s,
            None => {
                let kind = if n.z > 0.7 { SurfaceKind::Floor } else if n.z < -0.7 { SurfaceKind::Ceiling } else { SurfaceKind::Wall };
                let s = b.begin(*key, kind, *n, t[0]);
                b.surfaces[s].panel = true;
                s
            }
        };
        b.tri(s, t[0], t[1], t[2]);
    }
    let (lo, hi) = b.positions.iter().fold((DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)), |(a, c), p| (a.min(*p), c.max(*p)));
    let bvh = Bvh::build(&b.positions, &b.triangles);
    Ok(Room {
        prism: false,
        height: hi.z - lo.z,
        regions: Vec::new(),
        positions: b.positions,
        triangles: b.triangles,
        tri_surface: b.tri_surface,
        surfaces: b.surfaces,
        bvh,
        legacy_keys: Vec::new(),
    })
}

/// Hash of a plane (normal and offset to 0.1 mm), FNV-1a.
fn plane_hash(n: DVec3, d: f64) -> u32 {
    let q = |v: f64| ((v * 1e4).round() as i64).to_le_bytes();
    let mut h: u32 = 0x811c_9dc5;
    for b in [q(n.x), q(n.y), q(n.z), q(d)].iter().flatten() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
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

    #[test]
    fn mesh_box_matches_extruded_box() {
        // A 4 × 3 × 2.5 m box as an OBJ-style quad mesh with mixed winding.
        let v: Vec<DVec3> = [(0., 0., 0.), (4., 0., 0.), (4., 3., 0.), (0., 3., 0.), (0., 0., 2.5), (4., 0., 2.5), (4., 3., 2.5), (0., 3., 2.5)]
            .iter()
            .map(|p| DVec3::new(p.0, p.1, p.2))
            .collect();
        let faces = vec![
            (vec![0, 1, 2, 3], 0),
            (vec![4, 5, 6, 7], 1),
            (vec![0, 4, 5, 1], 2),
            (vec![1, 2, 6, 5], 2),
            (vec![2, 3, 7, 6], 2),
            (vec![3, 0, 4, 7], 2),
        ];
        let r = room_from_mesh(&v, &faces).unwrap();
        assert!((r.volume() - 30.0).abs() < 1e-9, "volume {}", r.volume());
        assert_eq!(r.surfaces.len(), 6);
        assert!(r.contains(DVec3::new(1.0, 1.0, 1.0)) && !r.contains(DVec3::new(5.0, 1.0, 1.0)));
        for s in &r.surfaces {
            // Inward normals: a point just inside along the normal is inside.
            let t = r.triangles[s.triangles[0] as usize].map(|i| r.positions[i as usize]);
            let c = (t[0] + t[1] + t[2]) / 3.0;
            assert!(r.contains(c + s.normal * 0.01));
        }
        assert_eq!(r.surfaces.iter().filter(|s| s.kind == SurfaceKind::Floor).count(), 1);
        assert!((r.floor_area() - 12.0).abs() < 1e-9);
    }
}
