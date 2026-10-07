//! Boolean operations on closed polyhedra (BSP trees, after Evan Wallace's
//! csg.js). Every polygon carries a tag (the room surface it came from), so
//! the pieces of a face cut by another solid keep their identity and
//! material.
//!
//! Solids are lists of convex planar polygons whose normals point out of
//! the solid. Room models are solids of air: walls face out of the air.

use glam::DVec3;

/// Points within this of a plane count as on it (m).
const EPS: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Plane {
    n: DVec3,
    w: f64,
}

impl Plane {
    fn from_points(a: DVec3, b: DVec3, c: DVec3) -> Option<Plane> {
        let n = (b - a).cross(c - a);
        let l = n.length();
        if l < 1e-14 {
            return None;
        }
        let n = n / l;
        Some(Plane { n, w: n.dot(a) })
    }

    fn flip(&mut self) {
        self.n = -self.n;
        self.w = -self.w;
    }
}

/// A convex planar polygon with a tag.
#[derive(Debug, Clone)]
pub struct Polygon {
    pub verts: Vec<DVec3>,
    pub tag: u32,
    plane: Plane,
}

impl Polygon {
    /// None for degenerate polygons.
    pub fn new(verts: Vec<DVec3>, tag: u32) -> Option<Polygon> {
        if verts.len() < 3 {
            return None;
        }
        // Newell normal: robust for polygons with collinear corners.
        let mut n = DVec3::ZERO;
        for i in 0..verts.len() {
            let (a, b) = (verts[i], verts[(i + 1) % verts.len()]);
            n += DVec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
        }
        let l = n.length();
        if l < 1e-12 {
            return Plane::from_points(verts[0], verts[1], verts[2]).map(|plane| Polygon { verts, tag, plane });
        }
        let n = n / l;
        let c = verts.iter().sum::<DVec3>() / verts.len() as f64;
        Some(Polygon { plane: Plane { n, w: n.dot(c) }, verts, tag })
    }

    pub fn normal(&self) -> DVec3 {
        self.plane.n
    }

    fn flip(&mut self) {
        self.verts.reverse();
        self.plane.flip();
    }
}

const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;
const SPANNING: u8 = 3;

/// Splits `p` by `plane` into the four lists.
fn split(plane: &Plane, p: &Polygon, cof: &mut Vec<Polygon>, cob: &mut Vec<Polygon>, front: &mut Vec<Polygon>, back: &mut Vec<Polygon>) {
    let mut kind = 0u8;
    let types: Vec<u8> = p
        .verts
        .iter()
        .map(|v| {
            let t = plane.n.dot(*v) - plane.w;
            let k = if t < -EPS { BACK } else if t > EPS { FRONT } else { COPLANAR };
            kind |= k;
            k
        })
        .collect();
    match kind {
        COPLANAR => {
            if plane.n.dot(p.plane.n) > 0.0 { cof.push(p.clone()) } else { cob.push(p.clone()) }
        }
        FRONT => front.push(p.clone()),
        BACK => back.push(p.clone()),
        _ => {
            let (mut f, mut b) = (Vec::new(), Vec::new());
            let n = p.verts.len();
            for i in 0..n {
                let j = (i + 1) % n;
                let (ti, tj) = (types[i], types[j]);
                let (vi, vj) = (p.verts[i], p.verts[j]);
                if ti != BACK {
                    f.push(vi);
                }
                if ti != FRONT {
                    b.push(vi);
                }
                if (ti | tj) == SPANNING {
                    let t = (plane.w - plane.n.dot(vi)) / plane.n.dot(vj - vi);
                    let v = vi + (vj - vi) * t;
                    f.push(v);
                    b.push(v);
                }
            }
            if f.len() >= 3 {
                front.push(Polygon { verts: f, tag: p.tag, plane: p.plane });
            }
            if b.len() >= 3 {
                back.push(Polygon { verts: b, tag: p.tag, plane: p.plane });
            }
        }
    }
}

#[derive(Default)]
struct Node {
    plane: Option<Plane>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
    polygons: Vec<Polygon>,
}

impl Node {
    fn new(polys: Vec<Polygon>) -> Node {
        let mut n = Node::default();
        n.build(polys);
        n
    }

    fn invert(&mut self) {
        for p in &mut self.polygons {
            p.flip();
        }
        if let Some(pl) = &mut self.plane {
            pl.flip();
        }
        if let Some(f) = &mut self.front {
            f.invert();
        }
        if let Some(b) = &mut self.back {
            b.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }

    /// The parts of `polys` outside this solid.
    fn clip_polygons(&self, polys: Vec<Polygon>) -> Vec<Polygon> {
        let Some(plane) = &self.plane else { return polys };
        let (mut front, mut back) = (Vec::new(), Vec::new());
        for p in &polys {
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            split(plane, p, &mut cf, &mut cb, &mut front, &mut back);
            front.append(&mut cf);
            back.append(&mut cb);
        }
        let front = match &self.front {
            Some(f) => f.clip_polygons(front),
            None => front,
        };
        let back = match &self.back {
            Some(b) => b.clip_polygons(back),
            None => Vec::new(),
        };
        let mut out = front;
        out.extend(back);
        out
    }

    /// Removes the parts of this tree's polygons inside `bsp`.
    fn clip_to(&mut self, bsp: &Node) {
        self.polygons = bsp.clip_polygons(std::mem::take(&mut self.polygons));
        if let Some(f) = &mut self.front {
            f.clip_to(bsp);
        }
        if let Some(b) = &mut self.back {
            b.clip_to(bsp);
        }
    }

    fn all_polygons(&self) -> Vec<Polygon> {
        let mut out = self.polygons.clone();
        if let Some(f) = &self.front {
            out.extend(f.all_polygons());
        }
        if let Some(b) = &self.back {
            out.extend(b.all_polygons());
        }
        out
    }

    fn build(&mut self, polys: Vec<Polygon>) {
        if polys.is_empty() {
            return;
        }
        let plane = *self.plane.get_or_insert(polys[0].plane);
        let (mut front, mut back) = (Vec::new(), Vec::new());
        for p in &polys {
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            split(&plane, p, &mut cf, &mut cb, &mut front, &mut back);
            self.polygons.append(&mut cf);
            self.polygons.append(&mut cb);
        }
        if !front.is_empty() {
            self.front.get_or_insert_with(Default::default).build(front);
        }
        if !back.is_empty() {
            self.back.get_or_insert_with(Default::default).build(back);
        }
    }
}

/// A closed solid as convex polygons (normals pointing out).
#[derive(Debug, Clone, Default)]
pub struct Solid {
    pub polygons: Vec<Polygon>,
}

impl Solid {
    pub fn new(polygons: Vec<Polygon>) -> Solid {
        Solid { polygons }
    }

    /// Everything in either solid.
    pub fn union(&self, other: &Solid) -> Solid {
        if self.polygons.is_empty() {
            return other.clone();
        }
        let mut a = Node::new(self.polygons.clone());
        let mut b = Node::new(other.polygons.clone());
        a.clip_to(&b);
        b.clip_to(&a);
        b.invert();
        b.clip_to(&a);
        b.invert();
        a.build(b.all_polygons());
        Solid::new(a.all_polygons())
    }

    /// This solid minus `other`.
    pub fn subtract(&self, other: &Solid) -> Solid {
        let mut a = Node::new(self.polygons.clone());
        let mut b = Node::new(other.polygons.clone());
        a.invert();
        a.clip_to(&b);
        b.clip_to(&a);
        b.invert();
        b.clip_to(&a);
        b.invert();
        a.build(b.all_polygons());
        a.invert();
        Solid::new(a.all_polygons())
    }

    /// Volume by the divergence theorem (normals out).
    pub fn volume(&self) -> f64 {
        self.polygons
            .iter()
            .map(|p| (1..p.verts.len() - 1).map(|k| p.verts[0].dot((p.verts[k] - p.verts[0]).cross(p.verts[k + 1] - p.verts[0]))).sum::<f64>())
            .sum::<f64>()
            / 6.0
    }
}

/// The parts of `p` inside and outside `solid`.
pub fn split_by(p: Polygon, solid: &Solid) -> (Vec<Polygon>, Vec<Polygon>) {
    let node = Node::new(solid.polygons.clone());
    let outside = node.clip_polygons(vec![p.clone()]);
    let mut inv = Node::new(solid.polygons.clone());
    inv.invert();
    let inside = inv.clip_polygons(vec![p]);
    (inside, outside)
}

/// An axis-aligned box (for tests and simple features).
pub fn cuboid(lo: DVec3, hi: DVec3, tag: impl Fn(usize) -> u32) -> Solid {
    let c = |i: usize| DVec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
    // Faces −x, +x, −y, +y, −z, +z with outward winding.
    let faces: [[usize; 4]; 6] = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
    Solid::new(faces.iter().enumerate().filter_map(|(k, f)| Polygon::new(f.iter().map(|&i| c(i)).collect(), tag(k))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(lo: [f64; 3], hi: [f64; 3], tag: u32) -> Solid {
        cuboid(DVec3::from_array(lo), DVec3::from_array(hi), |k| tag * 10 + k as u32)
    }

    #[test]
    fn box_volume_and_outward_normals() {
        let a = bx([0.0; 3], [2.0, 3.0, 4.0], 1);
        assert!((a.volume() - 24.0).abs() < 1e-9);
        for p in &a.polygons {
            let c = p.verts.iter().sum::<DVec3>() / p.verts.len() as f64;
            assert!(p.normal().dot(c - DVec3::new(1.0, 1.5, 2.0)) > 0.0);
        }
    }

    #[test]
    fn union_and_subtract_volumes() {
        let a = bx([0.0; 3], [4.0, 4.0, 3.0], 1);
        let b = bx([3.0, 1.0, 0.0], [6.0, 3.0, 2.0], 2);
        assert!((a.union(&b).volume() - (48.0 + 12.0 - 4.0)).abs() < 1e-9);
        let pillar = bx([1.0, 1.0, -1.0], [2.0, 2.0, 4.0], 3);
        let s = a.subtract(&pillar);
        assert!((s.volume() - 45.0).abs() < 1e-9);
        // The pillar's sides are now room surfaces, facing out of the air (into the pillar).
        let sides: Vec<_> = s.polygons.iter().filter(|p| p.tag / 10 == 3).collect();
        assert!(!sides.is_empty());
        for p in sides {
            let c = p.verts.iter().sum::<DVec3>() / p.verts.len() as f64;
            assert!(p.normal().dot(DVec3::new(1.5, 1.5, c.z) - c) > 0.0);
        }
    }

    #[test]
    fn balcony_slab_and_tags() {
        // Hall minus a slab spanning the back wall: a balcony.
        let hall = bx([0.0; 3], [10.0, 8.0, 6.0], 1);
        let slab = bx([7.0, -1.0, 3.0], [11.0, 9.0, 3.4], 2);
        let s = hall.subtract(&slab);
        assert!((s.volume() - (480.0 - 3.0 * 8.0 * 0.4)).abs() < 1e-9);
        // The back wall (tag 11: +x face of the hall) is split above and below the slab.
        let back: f64 = s.polygons.iter().filter(|p| p.tag == 11).map(|p| {
            (1..p.verts.len() - 1).map(|k| (p.verts[k] - p.verts[0]).cross(p.verts[k + 1] - p.verts[0]).length() / 2.0).sum::<f64>()
        }).sum();
        assert!((back - (48.0 - 8.0 * 0.4)).abs() < 1e-9);
    }
}
