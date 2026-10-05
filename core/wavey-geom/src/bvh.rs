//! Bounding volume hierarchy over room triangles, for ray casts and
//! visibility checks. The node layout is flat so it can be uploaded to the GPU.

use glam::DVec3;

#[derive(Debug, Clone, Copy)]
pub struct BvhNode {
    pub min: DVec3,
    pub max: DVec3,
    /// Leaf: index of first triangle in `Bvh::order`. Inner: index of right child
    /// (the left child is always the next node).
    pub offset: u32,
    /// Number of triangles for a leaf, 0 for inner nodes.
    pub count: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Bvh {
    pub nodes: Vec<BvhNode>,
    /// Triangle indices in leaf order.
    pub order: Vec<u32>,
    /// Triangle vertex positions, indexed by triangle id.
    pub tris: Vec<[DVec3; 3]>,
    /// In leaf order: (triangle id, [v0, v1 − v0, v2 − v0]) — the edges the
    /// intersection test needs, computed once (same arithmetic).
    leaf: Vec<(u32, [DVec3; 3])>,
}

#[derive(Debug, Clone, Copy)]
pub struct Hit {
    pub t: f64,
    pub triangle: u32,
}

const LEAF_SIZE: usize = 4;

enum Visit {
    /// Stop the traversal.
    Stop,
    /// Keep searching, but only for hits closer than this one.
    Accept,
    /// Keep searching as if this hit did not exist.
    Ignore,
}

impl Bvh {
    pub fn build(positions: &[DVec3], triangles: &[[u32; 3]]) -> Self {
        let tris: Vec<[DVec3; 3]> =
            triangles.iter().map(|t| t.map(|i| positions[i as usize])).collect();
        let centroids: Vec<DVec3> = tris.iter().map(|t| (t[0] + t[1] + t[2]) / 3.0).collect();
        let mut order: Vec<u32> = (0..tris.len() as u32).collect();
        let mut nodes = Vec::with_capacity(tris.len() * 2);
        if !tris.is_empty() {
            build_rec(&tris, &centroids, &mut order, 0, tris.len(), &mut nodes);
        }
        let leaf = order.iter().map(|&i| {
            let t = &tris[i as usize];
            (i, [t[0], t[1] - t[0], t[2] - t[0]])
        }).collect();
        Bvh { nodes, order, tris, leaf }
    }

    /// Closest hit along `origin + t·dir` with `t_min < t < t_max`.
    pub fn intersect(&self, origin: DVec3, dir: DVec3, t_min: f64, t_max: f64) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        let mut limit = t_max;
        self.walk(origin, dir, t_min, &mut limit, &mut |tri, t| {
            best = Some(Hit { t, triangle: tri });
            Visit::Accept
        });
        best
    }

    /// Whether anything blocks the open segment from `a` to `b`, ignoring
    /// triangles for which `skip` returns true.
    pub fn occluded(&self, a: DVec3, b: DVec3, skip: impl Fn(u32) -> bool) -> bool {
        const EPS: f64 = 1e-7;
        let dir = b - a;
        let mut limit = 1.0 - EPS;
        let mut blocked = false;
        self.walk(a, dir, EPS, &mut limit, &mut |tri, _| {
            if skip(tri) {
                return Visit::Ignore;
            }
            blocked = true;
            Visit::Stop
        });
        blocked
    }

    /// Visits triangle hits within `(t_min, t_max)` in no particular order.
    fn walk(
        &self,
        origin: DVec3,
        dir: DVec3,
        t_min: f64,
        t_max: &mut f64,
        on_hit: &mut dyn FnMut(u32, f64) -> Visit,
    ) {
        if self.nodes.is_empty() {
            return;
        }
        let inv = dir.recip();
        let mut stack = [0u32; 64];
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            if !slab(node.min, node.max, origin, inv, t_min, *t_max) {
                continue;
            }
            if node.count > 0 {
                for k in 0..node.count {
                    let (tri, pre) = &self.leaf[(node.offset + k) as usize];
                    let tri = *tri;
                    if let Some(t) = ray_triangle_edges(origin, dir, pre)
                        && t > t_min
                        && t < *t_max
                    {
                        match on_hit(tri, t) {
                            Visit::Stop => return,
                            Visit::Accept => *t_max = t,
                            Visit::Ignore => {}
                        }
                    }
                }
            } else {
                let idx = stack[sp];
                stack[sp] = idx + 1;
                stack[sp + 1] = node.offset;
                sp += 2;
            }
        }
    }
}

fn build_rec(
    tris: &[[DVec3; 3]],
    centroids: &[DVec3],
    order: &mut [u32],
    start: usize,
    end: usize,
    nodes: &mut Vec<BvhNode>,
) -> usize {
    let (mut min, mut max) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
    let (mut cmin, mut cmax) = (min, max);
    for &i in &order[start..end] {
        for p in tris[i as usize] {
            min = min.min(p);
            max = max.max(p);
        }
        cmin = cmin.min(centroids[i as usize]);
        cmax = cmax.max(centroids[i as usize]);
    }
    let me = nodes.len();
    nodes.push(BvhNode { min, max, offset: start as u32, count: (end - start) as u32 });
    if end - start <= LEAF_SIZE {
        return me;
    }
    let ext = cmax - cmin;
    let axis = if ext.x >= ext.y && ext.x >= ext.z { 0 } else if ext.y >= ext.z { 1 } else { 2 };
    if ext[axis] <= 0.0 {
        return me;
    }
    let mid = (start + end) / 2;
    order[start..end].select_nth_unstable_by(mid - start, |a, b| {
        centroids[*a as usize][axis].total_cmp(&centroids[*b as usize][axis])
    });
    build_rec(tris, centroids, order, start, mid, nodes);
    let right = build_rec(tris, centroids, order, mid, end, nodes);
    nodes[me].offset = right as u32;
    nodes[me].count = 0;
    me
}

fn slab(min: DVec3, max: DVec3, o: DVec3, inv: DVec3, t_min: f64, t_max: f64) -> bool {
    let t0 = (min - o) * inv;
    let t1 = (max - o) * inv;
    // NaN (0·∞) comparisons fall through to the conservative side.
    let tn = t0.min(t1);
    let tf = t0.max(t1);
    let enter = tn.x.max(tn.y).max(tn.z).max(t_min);
    let exit = tf.x.min(tf.y).min(tf.z).min(t_max);
    enter <= exit * (1.0 + 1e-12) + 1e-12
}

/// Möller–Trumbore, two-sided. Returns the ray parameter of the hit.
pub fn ray_triangle(o: DVec3, d: DVec3, tri: &[DVec3; 3]) -> Option<f64> {
    ray_triangle_edges(o, d, &[tri[0], tri[1] - tri[0], tri[2] - tri[0]])
}

/// `ray_triangle` with the edges precomputed: `[v0, v1 − v0, v2 − v0]`.
#[inline]
pub fn ray_triangle_edges(o: DVec3, d: DVec3, t: &[DVec3; 3]) -> Option<f64> {
    let (tri0, e1, e2) = (t[0], t[1], t[2]);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - tri0;
    let u = s.dot(p) * inv;
    if !(-1e-10..=1.0 + 1e-10).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < -1e-10 || u + v > 1.0 + 1e-10 {
        return None;
    }
    Some(e2.dot(q) * inv)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(n: usize) -> (Vec<DVec3>, Vec<[u32; 3]>) {
        // n×n unit squares at z = 0 plus a big quad at z = 5.
        let mut pos = Vec::new();
        let mut tri = Vec::new();
        for i in 0..n {
            for j in 0..n {
                let b = pos.len() as u32;
                let (x, y) = (i as f64, j as f64);
                pos.extend([
                    DVec3::new(x, y, 0.0),
                    DVec3::new(x + 1.0, y, 0.0),
                    DVec3::new(x + 1.0, y + 1.0, 0.0),
                    DVec3::new(x, y + 1.0, 0.0),
                ]);
                tri.push([b, b + 1, b + 2]);
                tri.push([b, b + 2, b + 3]);
            }
        }
        let b = pos.len() as u32;
        let m = n as f64;
        pos.extend([
            DVec3::new(0.0, 0.0, 5.0),
            DVec3::new(m, 0.0, 5.0),
            DVec3::new(m, m, 5.0),
            DVec3::new(0.0, m, 5.0),
        ]);
        tri.push([b, b + 1, b + 2]);
        tri.push([b, b + 2, b + 3]);
        (pos, tri)
    }

    #[test]
    fn closest_hit_matches_brute_force() {
        let (pos, tri) = grid(8);
        let bvh = Bvh::build(&pos, &tri);
        for k in 0..50 {
            let o = DVec3::new(0.3 + k as f64 * 0.15, 0.7 + k as f64 * 0.11, 2.0);
            for d in [DVec3::NEG_Z, DVec3::Z, DVec3::new(0.3, -0.2, -1.0).normalize()] {
                let brute = bvh
                    .tris
                    .iter()
                    .filter_map(|t| ray_triangle(o, d, t))
                    .filter(|t| *t > 1e-9)
                    .fold(f64::INFINITY, f64::min);
                let got = bvh.intersect(o, d, 1e-9, f64::INFINITY).map_or(f64::INFINITY, |h| h.t);
                assert!((brute - got).abs() < 1e-9 || (brute.is_infinite() && got.is_infinite()));
            }
        }
    }

    #[test]
    fn occlusion() {
        let (pos, tri) = grid(4);
        let bvh = Bvh::build(&pos, &tri);
        let a = DVec3::new(1.5, 1.5, 2.0);
        assert!(bvh.occluded(a, DVec3::new(1.5, 1.5, -1.0), |_| false));
        assert!(!bvh.occluded(a, DVec3::new(2.5, 1.5, 1.0), |_| false));
        // Ending exactly on the floor is not occluded by the floor itself.
        assert!(!bvh.occluded(a, DVec3::new(1.5, 1.5, 0.0), |_| false));
        // Skipped triangles do not block.
        assert!(!bvh.occluded(a, DVec3::new(1.5, 1.5, 6.0), |t| t >= 32));
    }
}
