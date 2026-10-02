//! Convex hull of a small point set (cabinet outlines), as triangles.

use glam::DVec3;

/// Triangulated convex hull. Small sets use a brute force over point
/// triples (exact for tens of points); larger ones an incremental hull.
/// Detailed outlines (manufacturer wireframes have thousands of vertices)
/// are first reduced to their extreme points in a dense set of directions.
/// Coplanar faces are merged into one polygon and fan-triangulated, so quads
/// do not produce overlapping triangles. Triangles are wound
/// counter-clockwise seen from outside.
pub fn convex_hull(points: &[DVec3]) -> (Vec<DVec3>, Vec<[u32; 3]>) {
    // Deduplicate.
    let mut pts: Vec<DVec3> = Vec::new();
    for p in extreme_points(points) {
        if !pts.iter().any(|q| q.distance(p) < 1e-6) {
            pts.push(p);
        }
    }
    let n = pts.len();
    let scale = pts.iter().map(|p| p.length()).fold(1e-3, f64::max);
    let eps = 1e-7 * scale;
    let planes = if n > 40 {
        incremental_planes(&pts, eps).unwrap_or_else(|| brute_force_planes(&pts[..n.min(40)], eps, scale))
    } else {
        brute_force_planes(&pts, eps, scale)
    };
    let mut tris = Vec::new();
    for (nrm, d) in planes {
        let mut face: Vec<usize> = (0..n).filter(|&i| (nrm.dot(pts[i]) - d).abs() <= eps * 10.0).collect();
        if face.len() < 3 {
            continue;
        }
        let c = face.iter().map(|&i| pts[i]).sum::<DVec3>() / face.len() as f64;
        let (u, v) = nrm.any_orthonormal_pair();
        face.sort_by(|&a, &b| {
            let pa = pts[a] - c;
            let pb = pts[b] - c;
            pa.dot(v).atan2(pa.dot(u)).total_cmp(&pb.dot(v).atan2(pb.dot(u)))
        });
        for w in 1..face.len() - 1 {
            let (a, b, cc) = (face[0], face[w], face[w + 1]);
            let tri_n = (pts[b] - pts[a]).cross(pts[cc] - pts[a]);
            if tri_n.dot(nrm) >= 0.0 {
                tris.push([a as u32, b as u32, cc as u32]);
            } else {
                tris.push([a as u32, cc as u32, b as u32]);
            }
        }
    }
    (pts, tris)
}

/// Adds a plane unless an equal one is already listed.
fn push_plane(planes: &mut Vec<(DVec3, f64)>, nrm: DVec3, d: f64, eps: f64) {
    if !planes.iter().any(|(m, e)| m.dot(nrm) > 1.0 - 1e-9 && (e - d).abs() < eps * 10.0) {
        planes.push((nrm, d));
    }
}

/// Outward face planes `(normal, offset)` by testing every point triple.
fn brute_force_planes(pts: &[DVec3], eps: f64, scale: f64) -> Vec<(DVec3, f64)> {
    let n = pts.len();
    let mut planes: Vec<(DVec3, f64)> = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let nrm = (pts[j] - pts[i]).cross(pts[k] - pts[i]);
                if nrm.length() < 1e-12 * scale * scale {
                    continue;
                }
                let nrm = nrm.normalize();
                let d = nrm.dot(pts[i]);
                let (mut above, mut below) = (false, false);
                for p in pts {
                    let s = nrm.dot(*p) - d;
                    above |= s > eps;
                    below |= s < -eps;
                }
                if above && below {
                    continue;
                }
                let (nrm, d) = if above { (-nrm, -d) } else { (nrm, d) };
                push_plane(&mut planes, nrm, d, eps);
            }
        }
    }
    planes
}

/// Outward face planes from an incremental hull (O(n · faces)). None if
/// the points are (nearly) coplanar.
fn incremental_planes(pts: &[DVec3], eps: f64) -> Option<Vec<(DVec3, f64)>> {
    struct Face {
        v: [usize; 3],
        n: DVec3,
        d: f64,
        alive: bool,
    }
    let make = |v: [usize; 3]| {
        let n = (pts[v[1]] - pts[v[0]]).cross(pts[v[2]] - pts[v[0]]).normalize_or_zero();
        Face { v, n, d: n.dot(pts[v[0]]), alive: true }
    };
    // Initial tetrahedron from extreme points.
    let (i0, i1) = {
        let lo = (0..pts.len()).min_by(|&a, &b| pts[a].x.total_cmp(&pts[b].x))?;
        let hi = (0..pts.len()).max_by(|&a, &b| pts[a].x.total_cmp(&pts[b].x))?;
        (lo, hi)
    };
    let axis = (pts[i1] - pts[i0]).normalize_or_zero();
    let line_dist = |p: DVec3| (p - pts[i0] - axis * (p - pts[i0]).dot(axis)).length();
    let i2 = (0..pts.len()).max_by(|&a, &b| line_dist(pts[a]).total_cmp(&line_dist(pts[b])))?;
    let base = (pts[i1] - pts[i0]).cross(pts[i2] - pts[i0]).normalize_or_zero();
    let plane_dist = |p: DVec3| (p - pts[i0]).dot(base);
    let i3 = (0..pts.len()).max_by(|&a, &b| plane_dist(pts[a]).abs().total_cmp(&plane_dist(pts[b]).abs()))?;
    if axis == DVec3::ZERO || line_dist(pts[i2]) < eps * 10.0 || plane_dist(pts[i3]).abs() < eps * 10.0 {
        return None;
    }
    let centre = (pts[i0] + pts[i1] + pts[i2] + pts[i3]) / 4.0;
    let mut faces: Vec<Face> = Vec::new();
    for v in [[i0, i1, i2], [i0, i1, i3], [i0, i2, i3], [i1, i2, i3]] {
        let mut f = make(v);
        if f.n.dot(centre) - f.d > 0.0 {
            f = make([v[0], v[2], v[1]]);
        }
        faces.push(f);
    }
    for (pi, p) in pts.iter().enumerate() {
        if [i0, i1, i2, i3].contains(&pi) {
            continue;
        }
        let visible: Vec<usize> =
            (0..faces.len()).filter(|&f| faces[f].alive && faces[f].n.dot(*p) - faces[f].d > eps).collect();
        if visible.is_empty() {
            continue;
        }
        // Horizon: directed edges of visible faces whose reverse is not on a visible face.
        let mut edges = std::collections::HashSet::new();
        for &f in &visible {
            let v = faces[f].v;
            for k in 0..3 {
                edges.insert((v[k], v[(k + 1) % 3]));
            }
        }
        let horizon: Vec<(usize, usize)> = edges.iter().filter(|(a, b)| !edges.contains(&(*b, *a))).copied().collect();
        for &f in &visible {
            faces[f].alive = false;
        }
        for (a, b) in horizon {
            faces.push(make([a, b, pi]));
        }
        if faces.len() > 8 * pts.len() + 64 {
            // Compact dead faces now and then.
            faces.retain(|f| f.alive);
        }
    }
    let mut planes = Vec::new();
    for f in faces.iter().filter(|f| f.alive && f.n != DVec3::ZERO) {
        push_plane(&mut planes, f.n, f.d, eps);
    }
    Some(planes)
}

/// Hull vertices of a large point set: the farthest point in each of
/// `DIRS` directions (axes, box diagonals and a Fibonacci sphere), so the
/// box corners and faces of a cabinet are kept exactly and rounded parts
/// to within the direction spacing (~4°). Small sets are returned as they
/// are.
fn extreme_points(points: &[DVec3]) -> Vec<DVec3> {
    const DIRS: usize = 2000;
    if points.len() <= 40 {
        return points.to_vec();
    }
    let mut dirs: Vec<DVec3> = Vec::with_capacity(DIRS + 26);
    for x in [-1.0, 0.0, 1.0] {
        for y in [-1.0, 0.0, 1.0] {
            for z in [-1.0, 0.0, 1.0] {
                if (x, y, z) != (0.0, 0.0, 0.0) {
                    dirs.push(DVec3::new(x, y, z).normalize());
                }
            }
        }
    }
    let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
    for i in 0..DIRS {
        let z = 1.0 - 2.0 * (i as f64 + 0.5) / DIRS as f64;
        let r = (1.0 - z * z).sqrt();
        let a = golden * i as f64;
        dirs.push(DVec3::new(r * a.cos(), r * a.sin(), z));
    }
    let mut keep = vec![false; points.len()];
    for d in dirs {
        let best = (0..points.len()).max_by(|&a, &b| points[a].dot(d).total_cmp(&points[b].dot(d))).unwrap();
        keep[best] = true;
    }
    points.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// Edges of a triangulated hull where the surface bends by more than
/// `min_angle_deg` (corners and creases; not the diagonals of flat faces or
/// the facets of curved parts), as vertex index pairs.
pub fn feature_edges(verts: &[DVec3], tris: &[[u32; 3]], min_angle_deg: f64) -> Vec<[u32; 2]> {
    use std::collections::BTreeMap;
    let normal = |t: &[u32; 3]| {
        let [a, b, c] = t.map(|i| verts[i as usize]);
        (b - a).cross(c - a).normalize_or_zero()
    };
    let mut faces: BTreeMap<[u32; 2], Vec<DVec3>> = BTreeMap::new();
    for t in tris {
        let n = normal(t);
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            faces.entry([a.min(b), a.max(b)]).or_default().push(n);
        }
    }
    let cos_min = min_angle_deg.to_radians().cos();
    faces.into_iter().filter(|(_, ns)| ns.len() != 2 || ns[0].dot(ns[1]) < cos_min).map(|(e, _)| e).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_hull() {
        let mut pts = Vec::new();
        for x in [-1.0, 1.0] {
            for y in [-1.0, 1.0] {
                for z in [-1.0, 1.0] {
                    pts.push(DVec3::new(x, y, z));
                }
            }
        }
        pts.push(DVec3::ZERO); // interior point is ignored
        let (v, t) = convex_hull(&pts);
        assert_eq!(t.len(), 12);
        // Outward winding and closed surface: total signed volume = 8.
        let vol: f64 = t.iter().map(|f| v[f[0] as usize].dot(v[f[1] as usize].cross(v[f[2] as usize])) / 6.0).sum();
        assert!((vol - 8.0).abs() < 1e-9, "{vol}");
    }

    #[test]
    fn trapezoid_prism() {
        // Evo 2-like plan: wide front, narrow back.
        let mut pts = Vec::new();
        for z in [0.0, 0.78] {
            for (x, y) in [(0.0, 0.264), (0.0, -0.264), (-0.45, 0.1), (-0.45, -0.1)] {
                pts.push(DVec3::new(x, y, z));
            }
        }
        let (v, t) = convex_hull(&pts);
        let vol: f64 = t.iter().map(|f| v[f[0] as usize].dot(v[f[1] as usize].cross(v[f[2] as usize])) / 6.0).sum();
        let want = 0.5 * (0.528 + 0.2) * 0.45 * 0.78;
        assert!((vol - want).abs() < 1e-9);
    }

    #[test]
    fn dense_outline_is_fast_and_encloses_every_point() {
        // A manufacturer-style wireframe: 4000 points along the edges of a
        // box with rounded vertical edges.
        let mut pts = Vec::new();
        for i in 0..1000 {
            let t = i as f64 / 1000.0 * std::f64::consts::TAU;
            for z in [-0.6, 0.6] {
                let (c, s) = (t.cos(), t.sin());
                pts.push(DVec3::new(0.4 * c.signum() + 0.05 * c, 0.45 * s.signum() + 0.05 * s, z));
                pts.push(DVec3::new(0.45 * c, 0.5 * s, z * (i as f64 / 1000.0)));
            }
        }
        let t = std::time::Instant::now();
        let (v, tris) = convex_hull(&pts);
        assert!(t.elapsed().as_secs_f64() < 2.0, "{:?}", t.elapsed());
        // At most one point per sampled direction survives the reduction.
        assert!(v.len() <= 2026 && !tris.is_empty(), "{} hull points", v.len());
        // Every input point is inside or within 1 mm of the hull.
        for p in &pts {
            for t in &tris {
                let n = (v[t[1] as usize] - v[t[0] as usize]).cross(v[t[2] as usize] - v[t[0] as usize]).normalize();
                assert!(n.dot(*p - v[t[0] as usize]) < 1e-3, "{p} outside");
            }
        }
    }
}
