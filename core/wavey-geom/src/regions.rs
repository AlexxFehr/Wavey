//! Closed regions of a sketch.
//!
//! Faces of the planar graph are traced with half-edges so that each face lies
//! to the left of its half-edges: bounded faces come out counter-clockwise
//! (positive area), and the outer boundary of every connected component comes
//! out clockwise. A clockwise boundary that lies inside a bounded face of
//! another component is a hole in that face (e.g. a pillar).
//!
//! Dangling segments (filaments) do not bound anything; they are reported
//! separately so the extruder can turn them into free-standing walls.

use std::collections::{BTreeMap, HashMap};

use glam::DVec2;

use crate::sketch::{SegmentId, Sketch, VertexId};

/// A segment traversed in one direction. `forward` means from `a` to `b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HalfEdge {
    pub segment: SegmentId,
    pub forward: bool,
}

#[derive(Debug, Clone)]
pub struct Cycle {
    pub edges: Vec<HalfEdge>,
    pub points: Vec<DVec2>,
    pub signed_area: f64,
}

#[derive(Debug, Clone)]
pub struct Region {
    pub outer: Cycle,
    /// Clockwise boundaries of components nested directly inside this region.
    pub holes: Vec<Cycle>,
}

impl Region {
    pub fn area(&self) -> f64 {
        self.outer.signed_area + self.holes.iter().map(|h| h.signed_area).sum::<f64>()
    }

    pub fn contains(&self, p: DVec2) -> bool {
        point_in_polygon(p, &self.outer.points)
            && !self.holes.iter().any(|h| point_in_polygon(p, &h.points))
    }

    /// All half-edges bounding this region; the region lies to their left.
    pub fn boundary(&self) -> impl Iterator<Item = HalfEdge> + '_ {
        self.outer
            .edges
            .iter()
            .chain(self.holes.iter().flat_map(|h| h.edges.iter()))
            .copied()
    }
}

#[derive(Debug, Clone, Default)]
pub struct RegionSet {
    pub regions: Vec<Region>,
    /// Segments that do not bound any face (dangling or free-standing).
    pub filaments: Vec<SegmentId>,
    /// Region to the left of each bounding half-edge (absent = unbounded side).
    pub left_region: HashMap<HalfEdge, usize>,
}

impl RegionSet {
    /// The innermost region containing `p`.
    pub fn region_at(&self, p: DVec2) -> Option<usize> {
        self.regions
            .iter()
            .enumerate()
            .filter(|(_, r)| r.contains(p))
            .min_by(|a, b| a.1.area().total_cmp(&b.1.area()))
            .map(|(i, _)| i)
    }
}

pub fn find_regions(sketch: &Sketch) -> RegionSet {
    let segs = sketch.segments();
    let pos = |v: VertexId| sketch.vertex(v).unwrap();

    // Prune filaments: repeatedly drop segments with a degree-1 endpoint.
    let mut alive: BTreeMap<SegmentId, (VertexId, VertexId)> =
        segs.iter().map(|(id, s)| (*id, (s.a, s.b))).collect();
    let mut filaments = Vec::new();
    loop {
        let mut degree: HashMap<VertexId, usize> = HashMap::new();
        for (a, b) in alive.values() {
            *degree.entry(*a).or_default() += 1;
            *degree.entry(*b).or_default() += 1;
        }
        let dead: Vec<SegmentId> = alive
            .iter()
            .filter(|(_, (a, b))| degree[a] < 2 || degree[b] < 2)
            .map(|(id, _)| *id)
            .collect();
        if dead.is_empty() {
            break;
        }
        for id in dead {
            alive.remove(&id);
            filaments.push(id);
        }
    }
    filaments.sort_unstable();

    // Outgoing half-edges at each vertex, sorted counter-clockwise by angle.
    let mut outgoing: HashMap<VertexId, Vec<(f64, HalfEdge, VertexId)>> = HashMap::new();
    for (id, (a, b)) in &alive {
        let (pa, pb) = (pos(*a), pos(*b));
        let d = pb - pa;
        outgoing.entry(*a).or_default().push((
            d.y.atan2(d.x),
            HalfEdge { segment: *id, forward: true },
            *b,
        ));
        outgoing.entry(*b).or_default().push((
            (-d.y).atan2(-d.x),
            HalfEdge { segment: *id, forward: false },
            *a,
        ));
    }
    for list in outgoing.values_mut() {
        list.sort_by(|x, y| x.0.total_cmp(&y.0));
    }
    let origin = |h: HalfEdge| {
        let (a, b) = alive[&h.segment];
        if h.forward { a } else { b }
    };
    let target = |h: HalfEdge| {
        let (a, b) = alive[&h.segment];
        if h.forward { b } else { a }
    };
    let next = |h: HalfEdge| -> HalfEdge {
        let v = target(h);
        let list = &outgoing[&v];
        let twin = HalfEdge { segment: h.segment, forward: !h.forward };
        let i = list.iter().position(|e| e.1 == twin).unwrap();
        // Next clockwise from the twin keeps the face on the left.
        list[(i + list.len() - 1) % list.len()].1
    };

    // Trace all cycles.
    let mut visited: HashMap<HalfEdge, bool> = HashMap::new();
    let mut cycles: Vec<Cycle> = Vec::new();
    for id in alive.keys() {
        for forward in [true, false] {
            let start = HalfEdge { segment: *id, forward };
            if visited.contains_key(&start) {
                continue;
            }
            let mut edges = Vec::new();
            let mut h = start;
            loop {
                visited.insert(h, true);
                edges.push(h);
                h = next(h);
                if h == start {
                    break;
                }
            }
            let points: Vec<DVec2> = edges.iter().map(|e| pos(origin(*e))).collect();
            let signed_area = polygon_signed_area(&points);
            cycles.push(Cycle { edges, points, signed_area });
        }
    }

    // Connected components, to tell nesting apart from a component's own faces.
    let mut comp: HashMap<VertexId, usize> = HashMap::new();
    {
        let mut parent: HashMap<VertexId, VertexId> = HashMap::new();
        fn find(p: &mut HashMap<VertexId, VertexId>, x: VertexId) -> VertexId {
            let px = *p.entry(x).or_insert(x);
            if px == x {
                return x;
            }
            let r = find(p, px);
            p.insert(x, r);
            r
        }
        for (a, b) in alive.values() {
            let (ra, rb) = (find(&mut parent, *a), find(&mut parent, *b));
            if ra != rb {
                parent.insert(ra, rb);
            }
        }
        let keys: Vec<VertexId> = parent.keys().copied().collect();
        let mut ids: HashMap<VertexId, usize> = HashMap::new();
        for v in keys {
            let r = find(&mut parent, v);
            let n = ids.len();
            let c = *ids.entry(r).or_insert(n);
            comp.insert(v, c);
        }
    }
    let cycle_comp = |c: &Cycle| comp[&origin(c.edges[0])];

    let (bounded, outer): (Vec<Cycle>, Vec<Cycle>) =
        cycles.into_iter().partition(|c| c.signed_area > 1e-12);

    let mut regions: Vec<Region> =
        bounded.into_iter().map(|outer| Region { outer, holes: Vec::new() }).collect();

    // The smallest containing face of another component is the innermost host.
    for hole in outer {
        let hc = cycle_comp(&hole);
        let probe = hole.points[0];
        let host = regions
            .iter()
            .enumerate()
            .filter(|(_, r)| cycle_comp(&r.outer) != hc && point_in_polygon(probe, &r.outer.points))
            .min_by(|a, b| a.1.outer.signed_area.total_cmp(&b.1.outer.signed_area))
            .map(|(i, _)| i);
        if let Some(i) = host {
            regions[i].holes.push(hole);
        }
    }

    let mut left_region = HashMap::new();
    for (i, r) in regions.iter().enumerate() {
        for h in r.boundary() {
            left_region.insert(h, i);
        }
    }

    RegionSet { regions, filaments, left_region }
}

pub fn polygon_signed_area(points: &[DVec2]) -> f64 {
    let n = points.len();
    (0..n).map(|i| points[i].perp_dot(points[(i + 1) % n])).sum::<f64>() * 0.5
}

/// Even-odd point-in-polygon test.
pub fn point_in_polygon(p: DVec2, poly: &[DVec2]) -> bool {
    let n = poly.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn rect(s: &mut Sketch, x0: f64, y0: f64, x1: f64, y1: f64) {
        s.add_polygon(&[v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)]);
    }

    #[test]
    fn single_room() {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 4.0, 3.0);
        let rs = find_regions(&s);
        assert_eq!(rs.regions.len(), 1);
        assert!((rs.regions[0].area() - 12.0).abs() < 1e-9);
    }

    #[test]
    fn room_with_pillar() {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 10.0, 8.0);
        rect(&mut s, 4.0, 3.0, 5.0, 4.0);
        let rs = find_regions(&s);
        assert_eq!(rs.regions.len(), 2);
        let room = rs.region_at(v(1.0, 1.0)).unwrap();
        let pillar = rs.region_at(v(4.5, 3.5)).unwrap();
        assert_ne!(room, pillar);
        assert_eq!(rs.regions[room].holes.len(), 1);
        assert!((rs.regions[room].area() - 79.0).abs() < 1e-9);
        assert!((rs.regions[pillar].area() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn divided_room_has_two_regions() {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 10.0, 5.0);
        s.add_segment(v(5.0, 0.0), v(5.0, 5.0));
        let rs = find_regions(&s);
        assert_eq!(rs.regions.len(), 2);
        assert!(rs.regions.iter().all(|r| (r.area() - 25.0).abs() < 1e-9));
    }

    #[test]
    fn dangling_segment_is_filament() {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 10.0, 5.0);
        s.add_segment(v(2.0, 2.0), v(4.0, 2.0)); // free-standing panel
        s.add_segment(v(10.0, 2.0), v(8.0, 2.0)); // stub off a wall
        let rs = find_regions(&s);
        assert_eq!(rs.regions.len(), 1);
        assert_eq!(rs.filaments.len(), 2);
    }

    #[test]
    fn nested_hole_goes_to_innermost_region() {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 20.0, 20.0);
        rect(&mut s, 5.0, 5.0, 15.0, 15.0);
        rect(&mut s, 9.0, 9.0, 11.0, 11.0);
        let rs = find_regions(&s);
        assert_eq!(rs.regions.len(), 3);
        let outer = rs.region_at(v(1.0, 1.0)).unwrap();
        let middle = rs.region_at(v(6.0, 6.0)).unwrap();
        assert_eq!(rs.regions[outer].holes.len(), 1);
        assert_eq!(rs.regions[middle].holes.len(), 1);
        assert!((rs.regions[middle].area() - 96.0).abs() < 1e-9);
    }
}
