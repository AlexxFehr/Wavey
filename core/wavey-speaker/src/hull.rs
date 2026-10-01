//! Convex hull of a small point set (cabinet outlines), as triangles.

use glam::DVec3;

/// Triangulated convex hull. Brute force over point triples, which is fine
/// for cabinet outlines (tens of points). Coplanar faces are merged into one
/// polygon and fan-triangulated, so quads do not produce overlapping
/// triangles. Triangles are wound counter-clockwise seen from outside.
pub fn convex_hull(points: &[DVec3]) -> (Vec<DVec3>, Vec<[u32; 3]>) {
    // Deduplicate.
    let mut pts: Vec<DVec3> = Vec::new();
    for p in points {
        if !pts.iter().any(|q| q.distance(*p) < 1e-6) {
            pts.push(*p);
        }
    }
    let n = pts.len();
    let scale = pts.iter().map(|p| p.length()).fold(1e-3, f64::max);
    let eps = 1e-7 * scale;
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
                for p in &pts {
                    let s = nrm.dot(*p) - d;
                    above |= s > eps;
                    below |= s < -eps;
                }
                if above && below {
                    continue;
                }
                let (nrm, d) = if above { (-nrm, -d) } else { (nrm, d) };
                if !planes.iter().any(|(m, e)| m.dot(nrm) > 1.0 - 1e-9 && (e - d).abs() < eps * 10.0) {
                    planes.push((nrm, d));
                }
            }
        }
    }
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
}
