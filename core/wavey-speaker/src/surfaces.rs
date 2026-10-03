//! Surfaces of a cabinet wireframe. Manufacturer files (GLL) describe a
//! cabinet by its edges only; the faces are recovered here so the cabinet
//! can be drawn solid with its real shape (curved fronts, horns, rigging)
//! instead of its convex hull.
//!
//! Every plane spanned by two edges at a vertex is a candidate. The edges
//! lying in a plane form a planar graph whose bounded faces are traced by
//! angle (the face to the left of each half-edge). Faces inside other faces
//! of the same plane (a driver ring on a baffle) are nested: they are kept
//! with their depth and lifted slightly off their parent so both show.

use std::collections::{HashMap, HashSet};

use glam::{DVec2, DVec3};

/// One recovered face, triangulated.
#[derive(Debug, Clone)]
pub struct SurfaceFace {
    pub triangles: Vec<[DVec3; 3]>,
    /// Unit normal, pointing away from the cabinet's centre.
    pub normal: DVec3,
    /// 0 for an outer face, 1 for a face inside another one in its plane, …
    pub depth: u32,
    /// The face's boundary as welded vertex indices (shared between faces).
    pub outline: Vec<usize>,
    /// Offset of a nested face off its parent (zero for outer faces).
    pub lift: DVec3,
}

/// Faces of a wireframe (vertex positions and index pairs).
pub fn faces(vertices: &[[f64; 3]], edges: &[[u32; 2]]) -> Vec<SurfaceFace> {
    recover(vertices, edges).1
}

/// A clean solid version of a wireframe: what can be drawn without holes.
#[derive(Debug, Clone, Default)]
pub struct Solids {
    pub triangles: Vec<[DVec3; 3]>,
    /// Faces nested in others (drivers, ports), slightly lifted.
    pub insets: Vec<[DVec3; 3]>,
    /// The solids' edges: face outlines and hull creases.
    pub edges: Vec<[DVec3; 2]>,
}

/// The wireframe's faces grouped into shells (faces joined along edges).
/// A closed shell is kept as recovered; an open one (a horn wall drawn
/// without its mouth, a frame drawn as profiles) is replaced by its convex
/// hull, a clean closed solid of the same extent. Flat shells (a panel, a
/// driver ring) are kept as faces.
pub fn solids(vertices: &[[f64; 3]], edges: &[[u32; 2]]) -> Solids {
    let (pts, faces) = recover(vertices, edges);
    let mut out = Solids::default();
    if faces.is_empty() {
        return out;
    }
    let size = {
        let lo = pts.iter().fold(DVec3::splat(f64::INFINITY), |a, p| a.min(*p));
        let hi = pts.iter().fold(DVec3::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
        (hi - lo).length().max(1e-3)
    };
    let mut seen_edges: HashSet<[usize; 2]> = HashSet::new();
    for (group, open) in shells(&faces) {
        let mut ids: Vec<usize> = group.iter().flat_map(|&k| faces[k].outline.iter().copied()).collect();
        ids.sort_unstable();
        ids.dedup();
        let f0 = &faces[group[0]];
        let p0 = pts[f0.outline[0]];
        let flat = ids.iter().all(|&i| f0.normal.dot(pts[i] - p0).abs() < 4e-3 * size);
        // A lone sliver (one strip of a curved wall whose neighbours did
        // not close) adds only stray lines: skip it.
        if group.len() == 1 && f0.depth == 0 {
            let area: f64 = f0.triangles.iter().map(|t| (t[1] - t[0]).cross(t[2] - t[0]).length() / 2.0).sum();
            let lo = ids.iter().fold(DVec3::splat(f64::INFINITY), |a, &i| a.min(pts[i]));
            let hi = ids.iter().fold(DVec3::splat(f64::NEG_INFINITY), |a, &i| a.max(pts[i]));
            if area < 0.05 * (hi - lo).length_squared() {
                continue;
            }
        }
        // A closed or flat shell is drawn as its faces; an open one as its
        // convex hull.
        let hull = if open == 0 || flat {
            for &k in &group {
                let f = &faces[k];
                if f.depth == 0 { &mut out.triangles } else { &mut out.insets }.extend(f.triangles.iter().copied());
            }
            None
        } else {
            let shell: Vec<DVec3> = ids.iter().map(|&i| pts[i]).collect();
            let (hv, ht) = crate::hull::convex_hull(&shell);
            out.triangles.extend(ht.iter().map(|t| t.map(|i| hv[i as usize])));
            Some((hv, ht))
        };
        // Lines: the faces' edges where they bound one face or two meet at
        // a crease (not between the strips a curved wall is drawn with).
        // On a hull, only those lying on its surface (rims, end rings).
        let mut by_edge: HashMap<[usize; 2], Vec<usize>> = HashMap::new();
        for &k in &group {
            let f = &faces[k];
            let n = f.outline.len();
            for i in 0..n {
                let (a, b) = (f.outline[i], f.outline[(i + 1) % n]);
                by_edge.entry([a.min(b), a.max(b)]).or_default().push(k);
            }
        }
        let on_hull = |p: DVec3| {
            hull.as_ref().is_none_or(|(hv, ht)| {
                ht.iter().any(|t| {
                    let [a, b, c] = t.map(|i| hv[i as usize]);
                    let n = (b - a).cross(c - a).normalize_or_zero();
                    n != DVec3::ZERO && n.dot(p - a).abs() < 3e-3 * size && {
                        // Inside the triangle (barycentric, with a margin).
                        let (v0, v1, v2) = (b - a, c - a, p - a);
                        let (d00, d01, d11, d20, d21) = (v0.dot(v0), v0.dot(v1), v1.dot(v1), v2.dot(v0), v2.dot(v1));
                        let den = d00 * d11 - d01 * d01;
                        let v = (d11 * d20 - d01 * d21) / den;
                        let w = (d00 * d21 - d01 * d20) / den;
                        v > -0.02 && w > -0.02 && v + w < 1.02
                    }
                })
            })
        };
        let crease = 35f64.to_radians().cos();
        let mut keys: Vec<_> = by_edge.into_iter().collect();
        keys.sort_unstable_by_key(|(e, _)| *e);
        for (e, fs) in keys {
            let smooth = fs.len() == 2 && faces[fs[0]].normal.dot(faces[fs[1]].normal).abs() > crease;
            let lift = faces[fs[0]].lift;
            let (p, q) = (pts[e[0]] + lift, pts[e[1]] + lift);
            if !smooth && on_hull((p + q) / 2.0) && seen_edges.insert(e) {
                out.edges.push([p, q]);
            }
        }
    }
    out
}

fn recover(vertices: &[[f64; 3]], edges: &[[u32; 2]]) -> (Vec<DVec3>, Vec<SurfaceFace>) {
    let (pts, edges) = weld(vertices, edges);
    if edges.len() < 3 {
        return (pts, vec![]);
    }
    let lo = pts.iter().fold(DVec3::splat(f64::INFINITY), |a, p| a.min(*p));
    let hi = pts.iter().fold(DVec3::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
    let size = (hi - lo).length().max(1e-3);
    let centre = (lo + hi) / 2.0;
    let tol = 1.5e-3 * size;

    // Outlines left open in the file (a line-array box has no edge across
    // its grille) are closed first, so their faces and the grille appear.
    let mut edges = edges;
    let closing = closures(&pts, &edges, &planes_of(&pts, &edges, tol), tol);
    edges.extend(closing);
    let planes = planes_of(&pts, &edges, tol);

    let mut seen: HashSet<Vec<usize>> = HashSet::new();
    let mut out = vec![];
    for (n, d) in planes {
        let on = |p: usize| (n.dot(pts[p]) - d).abs() <= tol;
        let in_plane: Vec<[usize; 2]> = edges.iter().copied().filter(|&[a, b]| on(a) && on(b)).collect();
        if in_plane.len() < 3 {
            continue;
        }
        let (u, w) = n.any_orthonormal_pair();
        let to2 = |p: usize| DVec2::new(pts[p].dot(u), pts[p].dot(w));
        let loops = plane_faces(&in_plane, &to2);
        let polys: Vec<Vec<DVec2>> = loops.iter().map(|l| l.iter().map(|&i| to2(i)).collect()).collect();
        for (k, l) in loops.iter().enumerate() {
            let mut key = l.clone();
            key.sort_unstable();
            if !seen.insert(key) {
                continue;
            }
            // Depth: how many other faces of the plane contain this one.
            let probe = interior_point(&polys[k]);
            let depth = polys
                .iter()
                .enumerate()
                .filter(|(j, p)| *j != k && area(p).abs() > area(&polys[k]).abs() && contains(p, probe))
                .count() as u32;
            let c3 = l.iter().map(|&i| pts[i]).sum::<DVec3>() / l.len() as f64;
            let outward = if n.dot(c3 - centre) >= 0.0 { n } else { -n };
            let lift = outward * (depth as f64 * 1e-3 * size.min(1.0));
            let tris: Vec<[DVec3; 3]> = triangulate(&polys[k])
                .into_iter()
                .map(|t| t.map(|i| pts[l[i]] + lift))
                .collect();
            if !tris.is_empty() {
                out.push(SurfaceFace { triangles: tris, normal: outward, depth, outline: l.clone(), lift });
            }
        }
    }
    (pts, out)
}

/// Faces joined along shared edges into shells. A shell is closed when
/// every edge of it borders at least two of its faces.
pub fn shells(faces: &[SurfaceFace]) -> Vec<(Vec<usize>, usize)> {
    let mut by_edge: HashMap<[usize; 2], Vec<usize>> = HashMap::new();
    for (k, f) in faces.iter().enumerate() {
        let n = f.outline.len();
        for i in 0..n {
            let (a, b) = (f.outline[i], f.outline[(i + 1) % n]);
            by_edge.entry([a.min(b), a.max(b)]).or_default().push(k);
        }
    }
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    fn root(p: &mut Vec<usize>, x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        p[x] = r;
        r
    }
    for fs in by_edge.values() {
        for w in fs.windows(2) {
            let (a, b) = (root(&mut parent, w[0]), root(&mut parent, w[1]));
            parent[a] = b;
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for k in 0..faces.len() {
        let r = root(&mut parent, k);
        groups.entry(r).or_default().push(k);
    }
    let mut out: Vec<(Vec<usize>, usize)> = groups
        .into_values()
        .map(|g| {
            let open = by_edge.values().filter(|fs| fs.len() == 1 && g.contains(&fs[0])).count();
            (g, open)
        })
        .collect();
    out.sort_by_key(|(g, _)| std::cmp::Reverse(g.len()));
    out
}

/// Candidate planes: one per distinct (normal, offset) spanned by two
/// edges at a vertex.
fn planes_of(pts: &[DVec3], edges: &[[usize; 2]], tol: f64) -> Vec<(DVec3, f64)> {
    let mut adj: Vec<Vec<usize>> = vec![vec![]; pts.len()];
    for &[a, b] in edges {
        adj[a].push(b);
        adj[b].push(a);
    }
    let mut planes: HashMap<(i64, i64, i64, i64), (DVec3, f64)> = HashMap::new();
    for (v, nb) in adj.iter().enumerate() {
        for i in 0..nb.len() {
            for j in i + 1..nb.len() {
                let (a, b) = ((pts[nb[i]] - pts[v]).normalize(), (pts[nb[j]] - pts[v]).normalize());
                let c = a.cross(b);
                if c.length() < 0.05 {
                    continue;
                }
                let mut n = c.normalize();
                let first = if n.x.abs() > 1e-6 { n.x } else if n.y.abs() > 1e-6 { n.y } else { n.z };
                if first < 0.0 {
                    n = -n;
                }
                let d = n.dot(pts[v]);
                let q = |x: f64, s: f64| (x / s).round() as i64;
                planes.entry((q(n.x, 0.02), q(n.y, 0.02), q(n.z, 0.02), q(d, tol * 2.0))).or_insert((n, d));
            }
        }
    }
    let mut out: Vec<(DVec3, f64)> = planes.into_values().collect();
    out.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.x.total_cmp(&b.0.x)).then(a.0.y.total_cmp(&b.0.y)));
    out
}

/// Edges that close open outlines: in a plane, a chain of at least three
/// edges whose ends are free (no other edge of the plane continues them)
/// is closed by joining its ends, if that segment crosses no edge of the
/// plane and is no longer than the chain's extent.
fn closures(pts: &[DVec3], edges: &[[usize; 2]], planes: &[(DVec3, f64)], tol: f64) -> Vec<[usize; 2]> {
    let mut out: HashSet<[usize; 2]> = HashSet::new();
    for &(n, d) in planes {
        let on = |p: usize| (n.dot(pts[p]) - d).abs() <= tol;
        let in_plane: Vec<[usize; 2]> = edges.iter().copied().filter(|&[a, b]| on(a) && on(b)).collect();
        if in_plane.len() < 3 {
            continue;
        }
        let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
        for &[a, b] in &in_plane {
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
        let (u, w) = n.any_orthonormal_pair();
        let to2 = |p: usize| DVec2::new(pts[p].dot(u), pts[p].dot(w));
        let mut visited: HashSet<usize> = HashSet::new();
        for (&start, nb) in &adj {
            if nb.len() != 1 || visited.contains(&start) {
                continue;
            }
            // Walk the chain from this free end while it stays unbranched.
            let mut chain = vec![start];
            let (mut prev, mut cur) = (start, nb[0]);
            loop {
                chain.push(cur);
                let next: Vec<usize> = adj[&cur].iter().copied().filter(|&x| x != prev).collect();
                if next.len() != 1 || chain.len() > 4096 {
                    break;
                }
                (prev, cur) = (cur, next[0]);
            }
            visited.extend(chain.iter().copied());
            let end = *chain.last().unwrap();
            if adj[&end].len() != 1 || chain.len() < 4 {
                continue;
            }
            let (a, b) = (to2(start), to2(end));
            let lo = chain.iter().fold(DVec2::splat(f64::INFINITY), |m, &i| m.min(to2(i)));
            let hi = chain.iter().fold(DVec2::splat(f64::NEG_INFINITY), |m, &i| m.max(to2(i)));
            if a.distance(b) > (hi - lo).max_element() * 1.01 {
                continue;
            }
            let crosses = in_plane.iter().any(|&[p, q]| {
                if [p, q].contains(&start) || [p, q].contains(&end) {
                    return false;
                }
                let (c, e) = (to2(p), to2(q));
                let s1 = (b - a).perp_dot(c - a) * (b - a).perp_dot(e - a);
                let s2 = (e - c).perp_dot(a - c) * (e - c).perp_dot(b - c);
                s1 < 0.0 && s2 < 0.0
            });
            if !crosses {
                out.insert([start.min(end), start.max(end)]);
            }
        }
    }
    let mut v: Vec<[usize; 2]> = out.into_iter().collect();
    v.sort_unstable();
    v
}

/// Merges coincident vertices (0.5 mm) and drops repeated or zero-length edges.
fn weld(vertices: &[[f64; 3]], edges: &[[u32; 2]]) -> (Vec<DVec3>, Vec<[usize; 2]>) {
    let mut ids: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut pts = vec![];
    let mut id = |p: [f64; 3]| {
        let k = (p.map(|c| (c * 2000.0).round() as i64)).into();
        *ids.entry(k).or_insert_with(|| {
            pts.push(DVec3::from_array(p));
            pts.len() - 1
        })
    };
    let mut set = HashSet::new();
    for &[a, b] in edges {
        let (Some(pa), Some(pb)) = (vertices.get(a as usize), vertices.get(b as usize)) else { continue };
        let (a, b) = (id(*pa), id(*pb));
        if a != b {
            set.insert([a.min(b), a.max(b)]);
        }
    }
    let mut e: Vec<[usize; 2]> = set.into_iter().collect();
    e.sort_unstable();
    (pts, e)
}

/// Bounded faces of a planar graph (vertex loops, counter-clockwise).
/// Dangling edges are pruned first so faces have no spikes.
fn plane_faces(edges: &[[usize; 2]], to2: &dyn Fn(usize) -> DVec2) -> Vec<Vec<usize>> {
    let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
    for &[a, b] in edges {
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    }
    loop {
        let leaves: Vec<usize> = adj.iter().filter(|(_, n)| n.len() < 2).map(|(v, _)| *v).collect();
        if leaves.is_empty() {
            break;
        }
        for v in leaves {
            if let Some(nb) = adj.remove(&v) {
                for x in nb {
                    if let Some(l) = adj.get_mut(&x) {
                        l.retain(|&y| y != v);
                    }
                }
            }
        }
    }
    // Neighbours counter-clockwise by angle.
    for (v, nb) in adj.iter_mut() {
        let p = to2(*v);
        nb.sort_by(|a, b| {
            let (da, db) = (to2(*a) - p, to2(*b) - p);
            da.y.atan2(da.x).total_cmp(&db.y.atan2(db.x))
        });
    }
    let mut used: HashSet<(usize, usize)> = HashSet::new();
    let mut faces = vec![];
    let mut starts: Vec<(usize, usize)> = adj.iter().flat_map(|(a, nb)| nb.iter().map(move |b| (*a, *b))).collect();
    starts.sort_unstable();
    for (a0, b0) in starts {
        if used.contains(&(a0, b0)) {
            continue;
        }
        let mut lp = vec![];
        let (mut a, mut b) = (a0, b0);
        loop {
            if !used.insert((a, b)) || lp.len() > 4096 {
                break;
            }
            lp.push(a);
            // Next: the neighbour of b just clockwise of a (face on the left).
            let nb = &adj[&b];
            let i = nb.iter().position(|&x| x == a).unwrap();
            let next = nb[(i + nb.len() - 1) % nb.len()];
            (a, b) = (b, next);
            if (a, b) == (a0, b0) {
                break;
            }
        }
        if lp.len() >= 3 {
            let poly: Vec<DVec2> = lp.iter().map(|&i| to2(i)).collect();
            let unique: HashSet<usize> = lp.iter().copied().collect();
            if area(&poly) > 1e-9 && unique.len() == lp.len() {
                faces.push(lp);
            }
        }
    }
    faces
}

fn area(p: &[DVec2]) -> f64 {
    (0..p.len()).map(|i| p[i].perp_dot(p[(i + 1) % p.len()])).sum::<f64>() / 2.0
}

fn contains(p: &[DVec2], q: DVec2) -> bool {
    let mut inside = false;
    for i in 0..p.len() {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// A point inside a polygon: the centroid of its first ear.
fn interior_point(p: &[DVec2]) -> DVec2 {
    triangulate(p).first().map_or(p[0], |t| (p[t[0]] + p[t[1]] + p[t[2]]) / 3.0)
}

/// Ear-clipping triangulation of a simple counter-clockwise polygon.
fn triangulate(p: &[DVec2]) -> Vec<[usize; 3]> {
    let mut idx: Vec<usize> = (0..p.len()).collect();
    let mut out = vec![];
    let mut guard = 0;
    while idx.len() > 3 && guard < p.len() * p.len() {
        guard += 1;
        let n = idx.len();
        let mut clipped = false;
        for k in 0..n {
            let (i0, i1, i2) = (idx[(k + n - 1) % n], idx[k], idx[(k + 1) % n]);
            let (a, b, c) = (p[i0], p[i1], p[i2]);
            if (b - a).perp_dot(c - b) <= 1e-12 {
                continue;
            }
            let inside = idx.iter().any(|&j| {
                j != i0 && j != i1 && j != i2 && {
                    let q = p[j];
                    (b - a).perp_dot(q - a) >= 0.0 && (c - b).perp_dot(q - b) >= 0.0 && (a - c).perp_dot(q - c) >= 0.0
                }
            });
            if !inside {
                out.push([i0, i1, i2]);
                idx.remove(k);
                clipped = true;
                break;
            }
        }
        if !clipped {
            // Degenerate rest (collinear points): drop the flattest vertex.
            let k = (0..n)
                .min_by(|&x, &y| {
                    let t = |k: usize| (p[idx[k]] - p[idx[(k + n - 1) % n]]).perp_dot(p[idx[(k + 1) % n]] - p[idx[k]]).abs();
                    t(x).total_cmp(&t(y))
                })
                .unwrap();
            idx.remove(k);
        }
    }
    if idx.len() == 3 && (p[idx[1]] - p[idx[0]]).perp_dot(p[idx[2]] - p[idx[1]]) > 1e-12 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube_with_ring() -> (Vec<[f64; 3]>, Vec<[u32; 2]>) {
        let mut v = vec![];
        for z in [0.0, 1.0] {
            for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
                v.push([x, y, z]);
            }
        }
        let mut e = vec![];
        for k in 0..4u32 {
            e.push([k, (k + 1) % 4]);
            e.push([4 + k, 4 + (k + 1) % 4]);
            e.push([k, k + 4]);
        }
        // A driver ring on the front face (x = 1).
        let base = v.len() as u32;
        for i in 0..16 {
            let a = i as f64 / 16.0 * std::f64::consts::TAU;
            v.push([1.0, 0.5 + 0.3 * a.cos(), 0.5 + 0.3 * a.sin()]);
        }
        for i in 0..16 {
            e.push([base + i, base + (i + 1) % 16]);
        }
        (v, e)
    }

    #[test]
    fn recovers_box_faces_and_a_nested_ring() {
        let (v, e) = cube_with_ring();
        let f = faces(&v, &e);
        assert_eq!(f.iter().filter(|f| f.depth == 0).count(), 6);
        let ring: Vec<_> = f.iter().filter(|f| f.depth == 1).collect();
        assert_eq!(ring.len(), 1);
        assert!((ring[0].normal - DVec3::X).length() < 1e-9);
        // The ring sits just in front of the face.
        assert!(ring[0].triangles.iter().flatten().all(|p| p.x > 1.0));
        // Faces are fully covered: total area of the cube faces is 6.
        let a: f64 = f
            .iter()
            .filter(|f| f.depth == 0)
            .flat_map(|f| &f.triangles)
            .map(|t| (t[1] - t[0]).cross(t[2] - t[0]).length() / 2.0)
            .sum();
        assert!((a - 6.0).abs() < 1e-9, "{a}");
    }

    #[test]
    fn a_split_panel_gives_two_faces_not_its_outline() {
        let v = vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 1.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]];
        let e = vec![[0, 4], [4, 1], [1, 2], [2, 5], [5, 3], [3, 0], [4, 5]];
        let f = faces(&v, &e);
        assert_eq!(f.len(), 2);
        for face in &f {
            let a: f64 = face.triangles.iter().map(|t| (t[1] - t[0]).cross(t[2] - t[0]).length() / 2.0).sum();
            assert!((a - 1.0).abs() < 1e-9, "{a}");
        }
    }

    #[test]
    fn an_open_front_is_closed() {
        // A box whose front (x = 1) has no top or bottom edge: the top and
        // bottom outlines are open chains, and closing them gives the front.
        let v = vec![
            [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0],
        ];
        let e = vec![[1, 0], [0, 3], [3, 2], [5, 4], [4, 7], [7, 6], [0, 4], [1, 5], [2, 6], [3, 7]];
        let f = faces(&v, &e);
        assert_eq!(f.len(), 6, "{:?}", f.iter().map(|f| f.normal).collect::<Vec<_>>());
    }

    #[test]
    fn an_open_shell_becomes_its_hull() {
        // A box missing its top face: open, so drawn as its (closed) hull.
        let v = vec![
            [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0],
        ];
        // Top ring of edges drawn as two diagonals only: no top face.
        let e = vec![[0, 1], [1, 2], [2, 3], [3, 0], [0, 4], [1, 5], [2, 6], [3, 7], [4, 5], [6, 7]];
        let s = solids(&v, &e);
        let area: f64 = s.triangles.iter().map(|t| (t[1] - t[0]).cross(t[2] - t[0]).length() / 2.0).sum();
        assert!((area - 6.0).abs() < 1e-9, "{area}");
        // The drawn edges: the four walls' outlines (no top edges exist).
        assert!(s.edges.len() >= 8, "{}", s.edges.len());
    }

    #[test]
    fn concave_outline_is_triangulated_inside() {
        // An L-shaped prism: its top face is concave.
        let l = [(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)];
        let mut v = vec![];
        for z in [0.0, 1.0] {
            v.extend(l.iter().map(|&(x, y)| [x, y, z]));
        }
        let mut e = vec![];
        for k in 0..6u32 {
            e.push([k, (k + 1) % 6]);
            e.push([6 + k, 6 + (k + 1) % 6]);
            e.push([k, k + 6]);
        }
        let f = faces(&v, &e);
        let top: f64 = f
            .iter()
            .filter(|f| f.normal.z > 0.9)
            .flat_map(|f| &f.triangles)
            .map(|t| (t[1] - t[0]).cross(t[2] - t[0]).length() / 2.0)
            .sum();
        assert!((top - 3.0).abs() < 1e-9, "{top}");
        assert_eq!(f.len(), 8);
    }
}
