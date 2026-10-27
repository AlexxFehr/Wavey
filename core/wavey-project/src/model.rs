//! Modelling features: the room as an ordered list of solids added to or
//! cut from the volume of air, on top of the drawn (or imported) room.
//!
//! A hall with raked seating, a balcony and a vaulted ceiling is the drawn
//! floor plan plus a Rake (subtracted: solid seating), a Balcony slab
//! (subtracted) and a Vault (added: more air above the walls). Every face a
//! feature contributes has a stable key ([`SurfaceKey::Feature`]) and takes
//! the feature's material unless one is assigned to it.
//!
//! Panels (reflectors, canopies, free-standing walls) are thin two-sided
//! surfaces added after the solids; imprints mark part of a surface (an
//! absorber on a wall, a carpet aisle) with its own key and material.

use std::f64::consts::TAU;

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use wavey_geom::csg::{Polygon, Solid};
use wavey_geom::{GeomError, Room, SurfaceKey, SurfaceKind};

use crate::project::MeshModel;

/// A plane to sketch on: points (x, y) map to `origin + x·u + y·v`; the
/// normal (extrusion direction) is u × v.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorkPlane {
    pub origin: DVec3,
    pub u: DVec3,
    pub v: DVec3,
}

impl WorkPlane {
    /// Plan at height `z` (normal up).
    pub fn horizontal(z: f64) -> Self {
        WorkPlane { origin: DVec3::new(0.0, 0.0, z), u: DVec3::X, v: DVec3::Y }
    }
    /// Section across x at `x` (y right, z up; normal +x).
    pub fn section_x(x: f64) -> Self {
        WorkPlane { origin: DVec3::new(x, 0.0, 0.0), u: DVec3::Y, v: DVec3::Z }
    }
    /// Section across y at `y` (x right, z up; normal −y).
    pub fn section_y(y: f64) -> Self {
        WorkPlane { origin: DVec3::new(0.0, y, 0.0), u: DVec3::X, v: DVec3::Z }
    }
    pub fn normal(&self) -> DVec3 {
        self.u.cross(self.v).normalize()
    }
    pub fn at(&self, p: DVec2) -> DVec3 {
        self.origin + self.u * p.x + self.v * p.y
    }
    /// Plane coordinates of a world point (projected onto the plane).
    pub fn local(&self, p: DVec3) -> DVec2 {
        let d = p - self.origin;
        DVec2::new(d.dot(self.u), d.dot(self.v))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// More air (an alcove, a fly tower, a vault).
    Add,
    /// Solid taken from the air (a column, a stage, seating, a slab).
    Subtract,
}

/// Axis of a plan rectangle's length (vaults run along it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanAxis {
    X,
    Y,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeatureKind {
    /// A profile on a work plane swept along its normal from `from` to `to`;
    /// `slope` tilts the far end: rise per metre along `slope_dir` (plane
    /// coordinates), e.g. a pitched or raked ceiling.
    Extrude {
        plane: WorkPlane,
        outline: Vec<DVec2>,
        #[serde(default)]
        holes: Vec<Vec<DVec2>>,
        from: f64,
        to: f64,
        #[serde(default)]
        slope: f64,
        #[serde(default)]
        slope_dir: DVec2,
    },
    /// A profile on a work plane turned about an axis in that plane
    /// (domes, apses, round columns).
    Revolve {
        plane: WorkPlane,
        outline: Vec<DVec2>,
        axis: [DVec2; 2],
        angle_deg: f64,
        segments: u32,
    },
    /// An imported mesh as a solid.
    Body { mesh: MeshModel },
    /// Tiered seating over a plan rectangle: `rows` steps (or a smooth
    /// slope) rising `riser` each from `base` towards +axis (or −axis with
    /// `reverse`).
    Rake { min: DVec2, max: DVec2, axis: PlanAxis, #[serde(default)] reverse: bool, base: f64, rows: u32, riser: f64, #[serde(default = "yes")] stepped: bool },
    /// A slab over a plan outline with its underside at `underside`, and a
    /// parapet along the outline's first edge (the front).
    Balcony { outline: Vec<DVec2>, underside: f64, thickness: f64, #[serde(default)] parapet: f64 },
    /// A barrel vault over a plan rectangle, springing at `spring` and
    /// rising `rise`, running along `axis`.
    Vault { min: DVec2, max: DVec2, axis: PlanAxis, spring: f64, rise: f64, segments: u32 },
    /// A dome on a circle, springing at `spring`.
    Dome { centre: DVec2, radius: f64, spring: f64, rise: f64, segments: u32 },
    /// A column from `bottom` to `top`: round (`size.x` is the diameter) or
    /// rectangular.
    Column { centre: DVec2, size: DVec2, round: bool, bottom: f64, top: f64 },
    /// A beam from `a` to `b` in plan, `width` wide, its underside at
    /// `bottom`, `depth` deep.
    Beam { a: DVec2, b: DVec2, width: f64, bottom: f64, depth: f64 },
    /// A thin two-sided panel: a polygon on a work plane.
    Panel { plane: WorkPlane, outline: Vec<DVec2> },
    /// Part of a room surface (by key) with its own key and material: a
    /// polygon on that surface's plane (plan coordinates of the surface's
    /// own frame: [`WorkPlane`] given).
    Imprint { target: SurfaceKey, plane: WorkPlane, outline: Vec<DVec2> },
    /// A standing crowd of `density` people per m² over a plan outline on
    /// the floor (z = 0): the floor there absorbs and scatters like people.
    /// Its material follows the density (see [`wavey_materials::crowd_material`]).
    Crowd { outline: Vec<DVec2>, density: f64 },
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub id: u64,
    pub name: String,
    pub op: Op,
    #[serde(default)]
    pub suppressed: bool,
    /// Material of the feature's faces (unless assigned per face).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    pub kind: FeatureKind,
}

impl FeatureKind {
    /// The usual operation: seating, slabs, columns, beams and stages take
    /// air away; vaults, domes and extrusions add it.
    pub fn default_op(&self) -> Op {
        match self {
            FeatureKind::Rake { .. } | FeatureKind::Balcony { .. } | FeatureKind::Column { .. } | FeatureKind::Beam { .. } => Op::Subtract,
            _ => Op::Add,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            FeatureKind::Extrude { .. } => "Extrusion",
            FeatureKind::Revolve { .. } => "Revolve",
            FeatureKind::Body { .. } => "Body",
            FeatureKind::Rake { .. } => "Rake",
            FeatureKind::Balcony { .. } => "Balcony",
            FeatureKind::Vault { .. } => "Vault",
            FeatureKind::Dome { .. } => "Dome",
            FeatureKind::Column { .. } => "Column",
            FeatureKind::Beam { .. } => "Beam",
            FeatureKind::Panel { .. } => "Panel",
            FeatureKind::Imprint { .. } => "Imprint",
            FeatureKind::Crowd { .. } => "Crowd",
        }
    }
}

/// Polygons (outward) with a face and facet number each.
type Faces = Vec<(Vec<DVec3>, u32, u32)>;

/// Counter-clockwise (positive area) copy.
fn ccw(p: &[DVec2]) -> Vec<DVec2> {
    let a: f64 = (0..p.len()).map(|i| p[i].perp_dot(p[(i + 1) % p.len()])).sum();
    if a < 0.0 { p.iter().rev().copied().collect() } else { p.to_vec() }
}

/// Triangles of a plane polygon with holes (2D, earcut).
fn tris2d(outer: &[DVec2], holes: &[Vec<DVec2>]) -> Vec<[DVec2; 3]> {
    let mut pts: Vec<DVec2> = outer.to_vec();
    let mut idx = Vec::new();
    for h in holes {
        idx.push(pts.len());
        pts.extend(h);
    }
    let flat: Vec<f64> = pts.iter().flat_map(|p| [p.x, p.y]).collect();
    earcutr::earcut(&flat, &idx, 2).unwrap_or_default().chunks_exact(3).map(|c| [pts[c[0]], pts[c[1]], pts[c[2]]]).collect()
}

/// A prism: `outline` (and holes) on `plane`, from offset `from` along the
/// normal to `top(p)` (plane point → offset). Faces: 0 bottom, 1 top, then
/// one per outline edge (holes' edges after).
fn prism(plane: &WorkPlane, outline: &[DVec2], holes: &[Vec<DVec2>], from: f64, top: &dyn Fn(DVec2) -> f64) -> Faces {
    let n = plane.normal();
    let outer = ccw(outline);
    let holes: Vec<Vec<DVec2>> = holes.iter().map(|h| ccw(h).into_iter().rev().collect()).collect();
    let lo = |p: DVec2| plane.at(p) + n * from;
    let hi = |p: DVec2| plane.at(p) + n * top(p);
    let mut out: Faces = Vec::new();
    for t in tris2d(&outer, &holes) {
        out.push((vec![lo(t[0]), lo(t[2]), lo(t[1])], 0, 0));
        out.push((vec![hi(t[0]), hi(t[1]), hi(t[2])], 1, 0));
    }
    let mut face = 2;
    for ring in std::iter::once(&outer).chain(holes.iter()) {
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            out.push((vec![lo(a), lo(b), hi(b), hi(a)], face, 0));
            face += 1;
        }
    }
    out
}

/// `outline` (plane coordinates) turned about the axis through `axis[0]`,
/// `axis[1]` by `angle` in `segments` steps. Faces: one per outline edge
/// (facets: the steps), then the two end caps of a partial turn.
fn revolve(plane: &WorkPlane, outline: &[DVec2], axis: [DVec2; 2], angle: f64, segments: u32) -> Faces {
    let a0 = plane.at(axis[0]);
    let dir = (plane.at(axis[1]) - a0).normalize();
    let rot = |p: DVec3, t: f64| {
        let r = p - a0;
        let along = dir * r.dot(dir);
        let perp = r - along;
        a0 + along + perp * t.cos() + dir.cross(perp) * t.sin()
    };
    let segs = segments.max(3);
    let pts: Vec<DVec3> = outline.iter().map(|p| plane.at(*p)).collect();
    let mut out: Faces = Vec::new();
    for k in 0..segs {
        let (t0, t1) = (angle * k as f64 / segs as f64, angle * (k + 1) as f64 / segs as f64);
        for i in 0..pts.len() {
            let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
            let mut quad = vec![rot(p, t0), rot(q, t0), rot(q, t1), rot(p, t1)];
            quad.dedup_by(|a, b| a.distance(*b) < 1e-9);
            if quad.len() >= 3 && quad[0].distance(*quad.last().unwrap()) > 1e-9 {
                out.push((quad, i as u32, k));
            } else if quad.len() > 3 {
                quad.pop();
                out.push((quad, i as u32, k));
            }
        }
    }
    if angle < TAU - 1e-9 {
        let n = outline.len() as u32;
        for t in tris2d(&ccw(outline), &[]) {
            let c: Vec<DVec3> = t.iter().map(|p| plane.at(*p)).collect();
            out.push((c.clone(), n, 0));
            out.push((c.iter().map(|p| rot(*p, angle)).collect(), n + 1, 0));
        }
    }
    out
}

/// A plan rectangle's frame: origin corner, the axis direction and length,
/// the cross direction and width.
fn rect_frame(min: DVec2, max: DVec2, axis: PlanAxis) -> (DVec2, DVec2, f64, DVec2, f64) {
    let s = max - min;
    match axis {
        PlanAxis::X => (min, DVec2::X, s.x, DVec2::Y, s.y),
        PlanAxis::Y => (min, DVec2::Y, s.y, DVec2::X, s.x),
    }
}

impl Feature {
    /// The feature's solids, as polygons with face and facet numbers
    /// (outward normals). Empty for panels and imprints.
    pub fn solids(&self) -> Result<Vec<Faces>, String> {
        let bad = |m: &str| Err(format!("{}: {m}", self.name));
        Ok(match &self.kind {
            FeatureKind::Extrude { plane, outline, holes, from, to, slope, slope_dir } => {
                if outline.len() < 3 || (to - from).abs() < 1e-6 {
                    return bad("needs a closed outline and a depth");
                }
                let d = slope_dir.normalize_or_zero();
                let (from, to) = (from.min(*to), from.max(*to));
                vec![prism(plane, outline, holes, from, &|p| to + slope * p.dot(d))]
            }
            FeatureKind::Revolve { plane, outline, axis, angle_deg, segments } => {
                if outline.len() < 3 || axis[0].distance(axis[1]) < 1e-9 {
                    return bad("needs a closed outline and an axis");
                }
                vec![revolve(plane, outline, *axis, angle_deg.to_radians().clamp(1e-3, TAU), *segments)]
            }
            FeatureKind::Body { mesh } => {
                let mut f: Faces = Vec::new();
                for face in &mesh.faces {
                    let pts: Vec<DVec3> = face.v.iter().map(|&i| DVec3::from_array(mesh.vertices[i as usize])).collect();
                    for t in wavey_geom::triangulate(&pts) {
                        f.push((t.to_vec(), face.group, 0));
                    }
                }
                vec![f]
            }
            FeatureKind::Rake { min, max, axis, reverse, base, rows, riser, stepped } => {
                let (o, along, len, across, width) = rect_frame(*min, *max, *axis);
                if *rows == 0 || len <= 0.0 || width <= 0.0 {
                    return bad("needs rows and a plan area");
                }
                // Side profile in (s along the rake, z), swept across; a little below the base so it meets the floor.
                let n = *rows as usize;
                let t = len / n as f64;
                let bottom = base - 0.05;
                let mut prof = vec![DVec2::new(0.0, bottom), DVec2::new(len, bottom)];
                if *stepped {
                    for k in (0..n).rev() {
                        let h = base + (k as f64 + 1.0) * riser;
                        prof.push(DVec2::new((k + 1) as f64 * t, h));
                        prof.push(DVec2::new(k as f64 * t, h));
                    }
                } else {
                    prof.push(DVec2::new(len, base + n as f64 * riser));
                    prof.push(DVec2::new(0.0, *base));
                }
                // Rows rise from the `start` edge towards the far edge.
                let (start, dir) = if *reverse { (o + along * len, -along) } else { (o, along) };
                let plane = WorkPlane { origin: DVec3::new(start.x, start.y, 0.0), u: DVec3::new(dir.x, dir.y, 0.0), v: DVec3::Z };
                // Swept along the plane normal, onto the side the rectangle lies.
                let (f0, f1) = if plane.normal().truncate().dot(across) > 0.0 { (0.0, width) } else { (-width, 0.0) };
                vec![prism(&plane, &prof, &[], f0, &|_| f1)]
            }
            FeatureKind::Balcony { outline, underside, thickness, parapet } => {
                if outline.len() < 3 || *thickness <= 0.0 {
                    return bad("needs an outline and a thickness");
                }
                let plan = WorkPlane::horizontal(0.0);
                let top = underside + thickness;
                let mut parts = vec![prism(&plan, outline, &[], *underside, &|_| top)];
                if *parapet > 0.0 {
                    // Along the first edge, 0.1 m thick, on the outline's inside.
                    let (a, b) = (outline[0], outline[1]);
                    let c = outline.iter().sum::<DVec2>() / outline.len() as f64;
                    let mut inward = (b - a).normalize().perp() * 0.1;
                    if inward.dot(c - a) < 0.0 {
                        inward = -inward;
                    }
                    parts.push(prism(&plan, &[a, b, b + inward, a + inward], &[], top - 0.01, &|_| top + parapet));
                }
                parts
            }
            FeatureKind::Vault { min, max, axis, spring, rise, segments } => {
                let (o, along, len, across, width) = rect_frame(*min, *max, *axis);
                if *rise <= 0.0 || width <= 0.0 || len <= 0.0 {
                    return bad("needs a rise and a plan area");
                }
                // Circular segment over the width: radius from chord and rise.
                let r = (width * width / 4.0 + rise * rise) / (2.0 * rise);
                let cz = spring + rise - r;
                let half = (width / 2.0 / r).clamp(-1.0, 1.0).asin();
                let segs = (*segments).max(4) as usize;
                let mut prof = vec![DVec2::new(0.0, spring - 0.05), DVec2::new(width, spring - 0.05)];
                for k in 0..=segs {
                    let a = half - 2.0 * half * k as f64 / segs as f64;
                    prof.push(DVec2::new(width / 2.0 + r * a.sin(), cz + r * a.cos()));
                }
                let plane = WorkPlane { origin: DVec3::new(o.x, o.y, 0.0), u: DVec3::new(across.x, across.y, 0.0), v: DVec3::Z };
                let (f0, f1) = if plane.normal().truncate().dot(along) > 0.0 { (0.0, len) } else { (-len, 0.0) };
                vec![prism(&plane, &prof, &[], f0, &|_| f1)]
            }
            FeatureKind::Dome { centre, radius, spring, rise, segments } => {
                if *rise <= 0.0 || *radius <= 0.0 {
                    return bad("needs a radius and a rise");
                }
                let r = (radius * radius + rise * rise) / (2.0 * rise);
                let cz = spring + rise - r;
                let half = (radius / r).clamp(-1.0, 1.0).asin();
                let steps = ((*segments).max(8) / 2) as usize;
                let mut prof = vec![DVec2::new(0.0, spring - 0.05), DVec2::new(*radius, spring - 0.05)];
                for k in 0..steps {
                    let a = half - half * k as f64 / steps as f64;
                    prof.push(DVec2::new(r * a.sin(), cz + r * a.cos()));
                }
                prof.push(DVec2::new(0.0, spring + rise));
                let plane = WorkPlane { origin: DVec3::new(centre.x, centre.y, 0.0), u: DVec3::X, v: DVec3::Z };
                vec![revolve(&plane, &prof, [DVec2::ZERO, DVec2::Y], TAU, (*segments).max(8))]
            }
            FeatureKind::Column { centre, size, round, bottom, top } => {
                if top <= bottom || size.x <= 0.0 {
                    return bad("needs a size and a height");
                }
                let outline: Vec<DVec2> = if *round {
                    let n = 24;
                    (0..n).map(|k| *centre + DVec2::from_angle(TAU * k as f64 / n as f64) * (size.x / 2.0)).collect()
                } else {
                    let h = *size / 2.0;
                    vec![*centre - h, *centre + DVec2::new(h.x, -h.y), *centre + h, *centre + DVec2::new(-h.x, h.y)]
                };
                vec![prism(&WorkPlane::horizontal(0.0), &outline, &[], *bottom, &|_| *top)]
            }
            FeatureKind::Beam { a, b, width, bottom, depth } => {
                if a.distance(*b) < 1e-6 || *width <= 0.0 || *depth <= 0.0 {
                    return bad("needs a length, width and depth");
                }
                let side = (*b - *a).normalize().perp() * (width / 2.0);
                let top = bottom + depth;
                vec![prism(&WorkPlane::horizontal(0.0), &[*a - side, *b - side, *b + side, *a + side], &[], *bottom, &|_| top)]
            }
            FeatureKind::Panel { .. } | FeatureKind::Imprint { .. } | FeatureKind::Crowd { .. } => vec![],
        })
    }
}

/// Builds the room: `base` (the drawn or imported room) with `features`
/// applied in order. Without active features `base` is returned unchanged.
pub fn evaluate(base: Room, features: &[Feature]) -> Result<Room, GeomError> {
    let active: Vec<&Feature> = features.iter().filter(|f| !f.suppressed).collect();
    if active.is_empty() {
        return Ok(base);
    }
    // Keys by tag.
    let mut keys: Vec<SurfaceKey> = Vec::new();
    let mut tag = |k: SurfaceKey| -> u32 {
        match keys.iter().position(|x| *x == k) {
            Some(i) => i as u32,
            None => {
                keys.push(k);
                (keys.len() - 1) as u32
            }
        }
    };

    // The base room as a solid of air (faces turned out of the air), minus
    // its thin two-sided walls, which stay panels.
    let thin = thin_surfaces(&base);
    let mut panels: Vec<([DVec3; 3], SurfaceKey, DVec3)> = Vec::new();
    let mut air: Vec<Polygon> = Vec::new();
    for (si, s) in base.surfaces.iter().enumerate() {
        for &t in &s.triangles {
            let [a, b, c] = base.triangles[t as usize].map(|i| base.positions[i as usize]);
            if thin[si] {
                panels.push(([a, b, c], s.key, s.normal));
                continue;
            }
            let into_air = (b - a).cross(c - a).dot(s.normal) > 0.0;
            let verts = if into_air { vec![a, c, b] } else { vec![a, b, c] };
            if let Some(p) = Polygon::new(verts, tag(s.key)) {
                air.push(p);
            }
        }
    }
    let mut solid = Solid::new(air);
    let mut imprints: Vec<&Feature> = Vec::new();
    for f in &active {
        match &f.kind {
            FeatureKind::Panel { plane, outline } => {
                let n = plane.normal();
                for t in tris2d(&ccw(outline), &[]) {
                    let w = t.map(|p| plane.at(p));
                    panels.push((w, SurfaceKey::Feature { feature: f.id, face: 0, facet: 0 }, n));
                    panels.push((w, SurfaceKey::Feature { feature: f.id, face: 1, facet: 0 }, -n));
                }
                continue;
            }
            FeatureKind::Imprint { .. } | FeatureKind::Crowd { .. } => {
                imprints.push(f);
                continue;
            }
            _ => {}
        }
        let parts = f.solids().map_err(GeomError::Feature)?;
        for part in parts {
            let mut polys: Vec<Polygon> = part
                .into_iter()
                .filter_map(|(v, face, facet)| Polygon::new(v, tag(SurfaceKey::Feature { feature: f.id, face, facet })))
                .collect();
            // Outward normals whatever the outline's winding.
            if Solid::new(polys.clone()).volume() < 0.0 {
                polys.iter_mut().for_each(|p| *p = Polygon::new(p.verts.iter().rev().copied().collect(), p.tag).unwrap());
            }
            let s = Solid::new(polys);
            solid = match f.op {
                Op::Add => solid.union(&s),
                Op::Subtract => solid.subtract(&s),
            };
        }
    }
    if solid.polygons.is_empty() || solid.volume() <= 1e-6 {
        return Err(GeomError::Feature("the features leave no air".into()));
    }
    let mut polys: Vec<(Vec<DVec3>, SurfaceKey)> = solid.polygons.iter().map(|p| (p.verts.clone(), keys[p.tag as usize])).collect();
    // Imprints: the parts of the target's polygons inside the imprint's
    // outline (a thin prism through the surface) take the imprint's key.
    for f in imprints {
        // An imprint takes a surface; a crowd takes whatever lies in the floor plane.
        let ground = WorkPlane::horizontal(0.0);
        let (target, plane, outline) = match &f.kind {
            FeatureKind::Imprint { target, plane, outline } => (Some(target), plane, outline),
            FeatureKind::Crowd { outline, .. } => (None, &ground, outline),
            _ => continue,
        };
        if outline.len() < 3 {
            continue;
        }
        let mut cutter: Vec<Polygon> = prism(plane, outline, &[], -0.01, &|_| 0.01)
            .into_iter()
            .filter_map(|(v, _, _)| Polygon::new(v, 0))
            .collect();
        if Solid::new(cutter.clone()).volume() < 0.0 {
            cutter.iter_mut().for_each(|p| *p = Polygon::new(p.verts.iter().rev().copied().collect(), 0).unwrap());
        }
        let key = SurfaceKey::Feature { feature: f.id, face: 0, facet: 0 };
        let mut next = Vec::new();
        for (v, k) in polys {
            let hit = match target {
                Some(t) => k.material_key() == t.material_key(),
                None => v.iter().all(|p| p.z.abs() < 1e-6),
            };
            if !hit {
                next.push((v, k));
                continue;
            }
            let (inside, outside) = wavey_geom::csg::split_by(Polygon::new(v.clone(), 0).unwrap(), &Solid::new(cutter.clone()));
            next.extend(inside.into_iter().map(|p| (p.verts, key)));
            next.extend(outside.into_iter().map(|p| (p.verts, k)));
        }
        polys = next;
    }
    wavey_geom::room_from_polygons(&polys, &panels)
}

/// Surfaces that are one side of a thin wall: another surface lies on the
/// same plane facing the other way and covers it.
fn thin_surfaces(room: &Room) -> Vec<bool> {
    let n = room.surfaces.len();
    let mut thin = vec![false; n];
    for i in 0..n {
        let s = &room.surfaces[i];
        if s.kind != SurfaceKind::Wall && room.prism {
            continue;
        }
        for j in 0..n {
            let o = &room.surfaces[j];
            if i == j || s.normal.dot(o.normal) > -1.0 + 1e-9 || (s.d + o.d).abs() > 1e-6 {
                continue;
            }
            let covered = s.triangles.iter().all(|&t| {
                let c = room.triangles[t as usize].iter().map(|&k| room.positions[k as usize]).sum::<DVec3>() / 3.0;
                room.surface_contains(j, c)
            });
            if covered {
                thin[i] = true;
                break;
            }
        }
    }
    thin
}
