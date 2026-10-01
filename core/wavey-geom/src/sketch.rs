//! 2D sketch: a planar graph of vertices and straight segments.
//!
//! Every edit keeps the graph planar: new segments are split where they cross
//! existing ones, and existing segments are split where new endpoints land on
//! them. Region detection (see [`crate::regions`]) relies on this invariant.

use std::collections::BTreeMap;

use glam::DVec2;
use serde::{Deserialize, Serialize};

pub type VertexId = u64;
pub type SegmentId = u64;

/// Points closer than this (meters) are merged into one vertex.
pub const MERGE_TOL: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub a: VertexId,
    pub b: VertexId,
}

/// Records that `old` was replaced by `new` (in order from `old.a` to `old.b`).
/// Callers use it to carry per-segment data (e.g. materials) over to the pieces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentSplit {
    pub old: SegmentId,
    pub new: Vec<SegmentId>,
}

#[derive(Debug, Clone, Default)]
pub struct EditResult {
    pub created: Vec<SegmentId>,
    pub splits: Vec<SegmentSplit>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sketch {
    next_id: u64,
    vertices: BTreeMap<VertexId, DVec2>,
    segments: BTreeMap<SegmentId, Segment>,
}

impl Sketch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn vertices(&self) -> &BTreeMap<VertexId, DVec2> {
        &self.vertices
    }

    pub fn segments(&self) -> &BTreeMap<SegmentId, Segment> {
        &self.segments
    }

    pub fn vertex(&self, id: VertexId) -> Option<DVec2> {
        self.vertices.get(&id).copied()
    }

    pub fn segment_points(&self, id: SegmentId) -> Option<(DVec2, DVec2)> {
        let s = self.segments.get(&id)?;
        Some((self.vertices[&s.a], self.vertices[&s.b]))
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    fn alloc(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn find_vertex(&self, p: DVec2) -> Option<VertexId> {
        self.vertices
            .iter()
            .find(|(_, v)| v.distance(p) <= MERGE_TOL)
            .map(|(id, _)| *id)
    }

    fn find_segment(&self, a: VertexId, b: VertexId) -> Option<SegmentId> {
        self.segments
            .iter()
            .find(|(_, s)| (s.a == a && s.b == b) || (s.a == b && s.b == a))
            .map(|(id, _)| *id)
    }

    /// Returns the vertex at `p`, creating it (and splitting any segment it lies on) if needed.
    fn insert_point(&mut self, p: DVec2, result: &mut EditResult) -> VertexId {
        if let Some(id) = self.find_vertex(p) {
            return id;
        }
        let id = self.alloc();
        self.vertices.insert(id, p);
        let on: Vec<SegmentId> = self
            .segments
            .iter()
            .filter(|(_, s)| {
                let (a, b) = (self.vertices[&s.a], self.vertices[&s.b]);
                point_segment_distance(p, a, b) <= MERGE_TOL
            })
            .map(|(sid, _)| *sid)
            .collect();
        for sid in on {
            self.split_segment(sid, id, result);
        }
        id
    }

    fn split_segment(&mut self, sid: SegmentId, v: VertexId, result: &mut EditResult) {
        let Some(seg) = self.segments.remove(&sid) else { return };
        let s1 = self.alloc();
        let s2 = self.alloc();
        self.segments.insert(s1, Segment { a: seg.a, b: v });
        self.segments.insert(s2, Segment { a: v, b: seg.b });
        // If the split segment was itself created by this edit, replace it in place.
        if let Some(pos) = result.created.iter().position(|c| *c == sid) {
            result.created.splice(pos..=pos, [s1, s2]);
            return;
        }
        // If it came from an earlier split in this edit, refine that record.
        for split in &mut result.splits {
            if let Some(pos) = split.new.iter().position(|c| *c == sid) {
                split.new.splice(pos..=pos, [s1, s2]);
                return;
            }
        }
        result.splits.push(SegmentSplit { old: sid, new: vec![s1, s2] });
    }

    /// Adds a straight segment from `p` to `q`, splitting at every crossing.
    pub fn add_segment(&mut self, p: DVec2, q: DVec2) -> EditResult {
        let mut result = EditResult::default();
        if p.distance(q) <= MERGE_TOL {
            return result;
        }
        let va = self.insert_point(p, &mut result);
        let vb = self.insert_point(q, &mut result);

        // Proper crossings with existing segments become new vertices.
        let crossings: Vec<(SegmentId, DVec2)> = self
            .segments
            .iter()
            .filter_map(|(sid, s)| {
                let (a, b) = (self.vertices[&s.a], self.vertices[&s.b]);
                segment_intersection(p, q, a, b).map(|x| (*sid, x))
            })
            .collect();
        for (_, x) in crossings {
            self.insert_point(x, &mut result);
        }

        // Every vertex on pq (endpoints, crossings, existing collinear vertices) cuts it.
        let dir = q - p;
        let len2 = dir.length_squared();
        let mut cuts: Vec<(f64, VertexId)> = self
            .vertices
            .iter()
            .filter(|(_, v)| point_segment_distance(**v, p, q) <= MERGE_TOL)
            .map(|(id, v)| ((*v - p).dot(dir) / len2, *id))
            .collect();
        cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
        cuts.dedup_by_key(|c| c.1);
        debug_assert!(cuts.first().map(|c| c.1) == Some(va));
        debug_assert!(cuts.last().map(|c| c.1) == Some(vb));

        for w in cuts.windows(2) {
            let (a, b) = (w[0].1, w[1].1);
            if a == b || self.find_segment(a, b).is_some() {
                continue; // overlaps an existing segment
            }
            let sid = self.alloc();
            self.segments.insert(sid, Segment { a, b });
            result.created.push(sid);
        }
        result
    }

    /// Adds a closed polygon (e.g. a rectangle or a pillar outline).
    pub fn add_polygon(&mut self, points: &[DVec2]) -> EditResult {
        let mut all = EditResult::default();
        for i in 0..points.len() {
            let r = self.add_segment(points[i], points[(i + 1) % points.len()]);
            merge_results(&mut all, r);
        }
        all
    }

    pub fn remove_segment(&mut self, id: SegmentId) -> bool {
        let Some(seg) = self.segments.remove(&id) else { return false };
        for v in [seg.a, seg.b] {
            if !self.segments.values().any(|s| s.a == v || s.b == v) {
                self.vertices.remove(&v);
            }
        }
        true
    }

    /// Moves a vertex. Does not re-resolve intersections; callers should avoid
    /// moves that create crossings (the UI re-adds segments instead).
    pub fn move_vertex(&mut self, id: VertexId, p: DVec2) -> bool {
        match self.vertices.get_mut(&id) {
            Some(v) => {
                *v = p;
                true
            }
            None => false,
        }
    }

    /// Nearest vertex within `radius`.
    pub fn nearest_vertex(&self, p: DVec2, radius: f64) -> Option<(VertexId, DVec2)> {
        self.vertices
            .iter()
            .map(|(id, v)| (*id, *v, v.distance(p)))
            .filter(|(_, _, d)| *d <= radius)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(id, v, _)| (id, v))
    }

    /// Nearest segment within `radius`.
    pub fn nearest_segment(&self, p: DVec2, radius: f64) -> Option<SegmentId> {
        self.segments
            .iter()
            .map(|(id, s)| {
                (*id, point_segment_distance(p, self.vertices[&s.a], self.vertices[&s.b]))
            })
            .filter(|(_, d)| *d <= radius)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }
}

fn merge_results(into: &mut EditResult, r: EditResult) {
    // Later edits may split segments created by earlier ones in the same batch.
    for split in r.splits {
        if let Some(pos) = into.created.iter().position(|c| *c == split.old) {
            into.created.splice(pos..=pos, split.new);
        } else {
            into.splits.push(split);
        }
    }
    into.created.extend(r.created);
}

pub fn point_segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 == 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Proper intersection of open segments pq and ab (not at endpoints, not collinear).
pub fn segment_intersection(p: DVec2, q: DVec2, a: DVec2, b: DVec2) -> Option<DVec2> {
    let r = q - p;
    let s = b - a;
    let denom = r.perp_dot(s);
    if denom.abs() < 1e-12 {
        return None;
    }
    let t = (a - p).perp_dot(s) / denom;
    let u = (a - p).perp_dot(r) / denom;
    let eps_t = MERGE_TOL / r.length();
    let eps_u = MERGE_TOL / s.length();
    if t > eps_t && t < 1.0 - eps_t && u > eps_u && u < 1.0 - eps_u {
        Some(p + r * t)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn crossing_segments_split_each_other() {
        let mut s = Sketch::new();
        s.add_segment(v(0.0, 0.0), v(2.0, 0.0));
        let r = s.add_segment(v(1.0, -1.0), v(1.0, 1.0));
        assert_eq!(s.segments().len(), 4);
        assert_eq!(r.created.len(), 2);
        assert_eq!(r.splits.len(), 1);
        assert_eq!(r.splits[0].new.len(), 2);
        assert_eq!(s.vertices().len(), 5);
    }

    #[test]
    fn endpoint_on_segment_splits_it() {
        let mut s = Sketch::new();
        s.add_segment(v(0.0, 0.0), v(2.0, 0.0));
        s.add_segment(v(1.0, 0.0), v(1.0, 1.0));
        assert_eq!(s.segments().len(), 3);
    }

    #[test]
    fn collinear_overlap_is_not_duplicated() {
        let mut s = Sketch::new();
        s.add_segment(v(0.0, 0.0), v(2.0, 0.0));
        s.add_segment(v(1.0, 0.0), v(3.0, 0.0));
        // 0-1, 1-2, 2-3
        assert_eq!(s.segments().len(), 3);
        assert_eq!(s.vertices().len(), 4);
    }

    #[test]
    fn polygon_closes() {
        let mut s = Sketch::new();
        let r = s.add_polygon(&[v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0), v(0.0, 1.0)]);
        assert_eq!(r.created.len(), 4);
        assert_eq!(s.vertices().len(), 4);
    }

    #[test]
    fn remove_drops_orphan_vertices() {
        let mut s = Sketch::new();
        let r = s.add_segment(v(0.0, 0.0), v(1.0, 0.0));
        assert!(s.remove_segment(r.created[0]));
        assert!(s.vertices().is_empty());
    }
}
