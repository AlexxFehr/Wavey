//! Checking and repairing imported room meshes: welding near vertices,
//! dropping degenerate and duplicate faces, closing T-junctions and small
//! planar holes, and reporting the gaps that remain.
//!
//! Face winding is left alone: [`crate::room_from_mesh`] orients every face
//! towards the air by itself.

use std::collections::HashMap;

use glam::DVec3;

/// What [`repair`] changed and what is still wrong.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RepairReport {
    pub vertices_welded: usize,
    pub degenerate_removed: usize,
    pub duplicates_removed: usize,
    pub t_junctions_fixed: usize,
    pub holes_filled: usize,
    /// Edges used by one face only: gaps the sound leaks through.
    pub open_edges: Vec<[DVec3; 2]>,
    /// Edges shared by more than two faces (not counting both sides of a
    /// thin panel).
    pub non_manifold_edges: usize,
}

impl RepairReport {
    pub fn watertight(&self) -> bool {
        self.open_edges.is_empty()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RepairOptions {
    /// Vertices closer than this are merged (m).
    pub weld: f64,
    /// Holes with at most this many edges are closed (0: none).
    pub max_hole_edges: usize,
}

impl Default for RepairOptions {
    fn default() -> Self {
        Self { weld: 1e-3, max_hole_edges: 12 }
    }
}

/// Polygon faces with a group each, as in [`crate::room_from_mesh`].
pub type Faces = Vec<(Vec<u32>, u32)>;

/// Repairs a polygon mesh; returns the new vertices and faces and a report.
pub fn repair(vertices: &[DVec3], faces: &[(Vec<u32>, u32)], opts: RepairOptions) -> (Vec<DVec3>, Faces, RepairReport) {
    let mut report = RepairReport::default();
    let (verts, map) = weld(vertices, opts.weld);
    report.vertices_welded = vertices.len() - verts.len();

    // Remap; drop repeated corners, then faces with no area.
    let mut out: Faces = Vec::with_capacity(faces.len());
    for (poly, g) in faces {
        let mut p: Vec<u32> = Vec::with_capacity(poly.len());
        for &i in poly {
            let v = map[i as usize];
            if p.last() != Some(&v) {
                p.push(v);
            }
        }
        while p.len() > 1 && p.first() == p.last() {
            p.pop();
        }
        if p.len() < 3 || newell(&verts, &p).length() < opts.weld * opts.weld {
            report.degenerate_removed += 1;
            continue;
        }
        out.push((p, *g));
    }

    // Same corners in the same order: a duplicate (the reverse order is the
    // other side of a thin panel and stays).
    let mut seen: HashMap<Vec<u32>, ()> = HashMap::new();
    out.retain(|(p, _)| {
        let start = (0..p.len()).min_by_key(|&i| p[i]).unwrap();
        let key: Vec<u32> = (0..p.len()).map(|k| p[(start + k) % p.len()]).collect();
        if seen.insert(key, ()).is_some() {
            report.duplicates_removed += 1;
            false
        } else {
            true
        }
    });

    report.t_junctions_fixed = fix_t_junctions(&verts, &mut out, opts.weld);
    if opts.max_hole_edges >= 3 {
        report.holes_filled = fill_holes(&verts, &mut out, opts);
    }

    let uses = edge_uses(&out);
    for (&(a, b), faces) in &uses {
        if faces.len() == 1 {
            report.open_edges.push([verts[a as usize], verts[b as usize]]);
        } else if faces.len() > 2 && !(faces.len() == 4 && panel_pairs(&out, faces)) {
            report.non_manifold_edges += 1;
        }
    }
    report.open_edges.sort_by(|x, y| x[0].x.total_cmp(&y[0].x).then(x[0].y.total_cmp(&y[0].y)).then(x[0].z.total_cmp(&y[0].z)));
    (verts, out, report)
}

/// Newell normal (length: twice the area).
fn newell(v: &[DVec3], p: &[u32]) -> DVec3 {
    let mut n = DVec3::ZERO;
    for i in 0..p.len() {
        let (a, b) = (v[p[i] as usize], v[p[(i + 1) % p.len()] as usize]);
        n += DVec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
    }
    n
}

/// Merges vertices within `tol` (first one wins); returns the new list and
/// the old → new index map.
fn weld(v: &[DVec3], tol: f64) -> (Vec<DVec3>, Vec<u32>) {
    let cell = |p: DVec3| [(p.x / tol).floor() as i64, (p.y / tol).floor() as i64, (p.z / tol).floor() as i64];
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    let mut out: Vec<DVec3> = Vec::new();
    let mut map = Vec::with_capacity(v.len());
    for &p in v {
        let c = cell(p);
        let mut found = None;
        'search: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) {
                        if let Some(&i) = list.iter().find(|&&i| out[i as usize].distance(p) <= tol) {
                            found = Some(i);
                            break 'search;
                        }
                    }
                }
            }
        }
        let i = found.unwrap_or_else(|| {
            out.push(p);
            let i = (out.len() - 1) as u32;
            grid.entry(c).or_default().push(i);
            i
        });
        map.push(i);
    }
    (out, map)
}

/// Undirected edge (low, high) → faces using it.
fn edge_uses(faces: &Faces) -> HashMap<(u32, u32), Vec<usize>> {
    let mut uses: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (fi, (p, _)) in faces.iter().enumerate() {
        for i in 0..p.len() {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            uses.entry((a.min(b), a.max(b))).or_default().push(fi);
        }
    }
    uses
}

/// Four faces on one edge that are two thin panels' front and back (each
/// face has a reversed twin among them): a panel edge, not a defect.
fn panel_pairs(faces: &Faces, on_edge: &[usize]) -> bool {
    let rev = |a: &[u32], b: &[u32]| {
        let mut r = b.to_vec();
        r.reverse();
        a.len() == r.len() && (0..r.len()).any(|s| (0..r.len()).all(|k| a[k] == r[(s + k) % r.len()]))
    };
    on_edge.iter().all(|&i| on_edge.iter().any(|&j| j != i && rev(&faces[i].0, &faces[j].0)))
}

/// Splits open edges at vertices lying on them (a face edge running past
/// the corner of a neighbour). Returns the number of vertices inserted.
fn fix_t_junctions(v: &[DVec3], faces: &mut Faces, tol: f64) -> usize {
    let mut fixed = 0;
    for _ in 0..3 {
        let uses = edge_uses(faces);
        let open: Vec<(u32, u32)> = uses.iter().filter(|(_, f)| f.len() == 1).map(|(e, _)| *e).collect();
        if open.is_empty() {
            break;
        }
        // Vertices used by faces, bucketed on a coarse grid.
        let (lo, hi) = v.iter().fold((DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)), |(a, b), p| (a.min(*p), b.max(*p)));
        let size = ((hi - lo).max_element() / 64.0).max(tol * 4.0);
        let key = |p: DVec3| {
            let c = ((p - lo) / size).floor();
            [c.x as i64, c.y as i64, c.z as i64]
        };
        let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
        let mut used = vec![false; v.len()];
        faces.iter().flat_map(|f| f.0.iter()).for_each(|&i| used[i as usize] = true);
        for (i, p) in v.iter().enumerate() {
            if used[i] {
                grid.entry(key(*p)).or_default().push(i as u32);
            }
        }
        let mut changed = false;
        for (a, b) in open {
            let (pa, pb) = (v[a as usize], v[b as usize]);
            let d = pb - pa;
            let len2 = d.length_squared();
            let (ka, kb) = (key(pa.min(pb) - DVec3::splat(tol)), key(pa.max(pb) + DVec3::splat(tol)));
            let mut on: Vec<(f64, u32)> = Vec::new();
            for x in ka[0]..=kb[0] {
                for y in ka[1]..=kb[1] {
                    for z in ka[2]..=kb[2] {
                        for &i in grid.get(&[x, y, z]).map(Vec::as_slice).unwrap_or(&[]) {
                            if i == a || i == b {
                                continue;
                            }
                            let s = (v[i as usize] - pa).dot(d) / len2;
                            if s > 1e-9 && s < 1.0 - 1e-9 && (pa + d * s).distance(v[i as usize]) <= tol {
                                on.push((s, i));
                            }
                        }
                    }
                }
            }
            if on.is_empty() {
                continue;
            }
            on.sort_by(|x, y| x.0.total_cmp(&y.0));
            on.dedup_by_key(|x| x.1);
            let fi = uses[&(a, b)][0];
            let p = &mut faces[fi].0;
            let n = p.len();
            let Some(k) = (0..n).find(|&k| (p[k] == a && p[(k + 1) % n] == b) || (p[k] == b && p[(k + 1) % n] == a)) else { continue };
            let mut ins: Vec<u32> = on.iter().map(|x| x.1).collect();
            if p[k] == b {
                ins.reverse();
            }
            fixed += ins.len();
            p.splice(k + 1..k + 1, ins);
            changed = true;
        }
        if !changed {
            break;
        }
    }
    fixed
}

/// Closes holes bounded by open edges when the boundary is short and flat.
fn fill_holes(v: &[DVec3], faces: &mut Faces, opts: RepairOptions) -> usize {
    let uses = edge_uses(faces);
    // Directed boundary: a face edge a → b with no partner means the hole
    // runs b → a.
    let mut next: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
    for (fi, (p, g)) in faces.iter().enumerate() {
        for i in 0..p.len() {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            if uses[&(a.min(b), a.max(b))] == [fi] {
                next.entry(b).or_default().push((a, *g));
            }
        }
    }
    let mut filled = 0;
    let mut done: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let starts: Vec<u32> = {
        let mut s: Vec<u32> = next.keys().copied().collect();
        s.sort();
        s
    };
    for start in starts {
        if done.contains(&start) {
            continue;
        }
        let mut lp = vec![start];
        let mut group = None;
        let mut cur = start;
        let closed = loop {
            let Some(cands) = next.get(&cur) else { break false };
            if cands.len() != 1 {
                break false;
            }
            let (n, g) = cands[0];
            group.get_or_insert(g);
            if n == start {
                break true;
            }
            if lp.contains(&n) || lp.len() > opts.max_hole_edges {
                break false;
            }
            lp.push(n);
            cur = n;
        };
        lp.iter().for_each(|i| {
            done.insert(*i);
        });
        if !closed || lp.len() < 3 || lp.len() > opts.max_hole_edges {
            continue;
        }
        // Flat enough: every corner within 5 mm (or 1 % of the size) of the plane.
        let n = newell(v, &lp);
        if n.length() < 1e-12 {
            continue;
        }
        let n = n.normalize();
        let c = lp.iter().map(|&i| v[i as usize]).sum::<DVec3>() / lp.len() as f64;
        let size = lp.iter().map(|&i| v[i as usize].distance(c)).fold(0.0, f64::max);
        if lp.iter().any(|&i| (v[i as usize] - c).dot(n).abs() > (0.005f64).max(0.01 * size)) {
            continue;
        }
        faces.push((lp, group.unwrap_or(0)));
        filled += 1;
    }
    filled
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unit cube, quads, one group per face.
    fn cube() -> (Vec<DVec3>, Faces) {
        let v: Vec<DVec3> = (0..8).map(|i| DVec3::new((i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64)).collect();
        let f = vec![
            (vec![0, 2, 3, 1], 0),
            (vec![4, 5, 7, 6], 1),
            (vec![0, 1, 5, 4], 2),
            (vec![2, 6, 7, 3], 3),
            (vec![0, 4, 6, 2], 4),
            (vec![1, 3, 7, 5], 5),
        ];
        (v, f)
    }

    #[test]
    fn closed_cube_is_watertight() {
        let (v, f) = cube();
        let (_, out, r) = repair(&v, &f, RepairOptions::default());
        assert!(r.watertight() && r.non_manifold_edges == 0);
        assert_eq!(out.len(), 6);
    }

    #[test]
    fn unwelded_triangles_are_welded() {
        // Every face's corners as separate (slightly noisy) vertices, as STL files have.
        let (v, f) = cube();
        let mut verts = Vec::new();
        let mut faces = Vec::new();
        for (k, (p, g)) in f.iter().enumerate() {
            let base = verts.len() as u32;
            verts.extend(p.iter().map(|&i| v[i as usize] + DVec3::splat(1e-5 * k as f64)));
            faces.push(((0..p.len() as u32).map(|i| base + i).collect(), *g));
        }
        let (vw, _, r) = repair(&verts, &faces, RepairOptions::default());
        assert_eq!(vw.len(), 8);
        assert!(r.watertight(), "{:?}", r.open_edges);
    }

    #[test]
    fn missing_face_is_filled_and_duplicates_dropped() {
        let (v, mut f) = cube();
        f.remove(1);
        f.push(f[0].clone());
        let (_, out, r) = repair(&v, &f, RepairOptions::default());
        assert_eq!((r.duplicates_removed, r.holes_filled), (1, 1));
        assert!(r.watertight());
        assert_eq!(out.len(), 6);
    }

    #[test]
    fn t_junction_is_split() {
        // The top face in two halves; one side face spans the full edge.
        let (mut v, mut f) = cube();
        v.push(DVec3::new(0.5, 0.0, 1.0)); // 8
        v.push(DVec3::new(0.5, 1.0, 1.0)); // 9
        f[1] = (vec![4, 8, 9, 6], 1);
        f.push((vec![8, 5, 7, 9], 1));
        let (_, _, r) = repair(&v, &f, RepairOptions { max_hole_edges: 0, ..Default::default() });
        assert_eq!(r.t_junctions_fixed, 2);
        assert!(r.watertight(), "{:?}", r.open_edges);
    }

    #[test]
    fn large_gap_is_reported() {
        let (v, mut f) = cube();
        f.remove(1);
        let (_, _, r) = repair(&v, &f, RepairOptions { max_hole_edges: 0, ..Default::default() });
        assert_eq!(r.open_edges.len(), 4);
    }
}
