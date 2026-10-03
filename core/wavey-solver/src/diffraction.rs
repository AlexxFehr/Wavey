//! Edge diffraction around the vertical edges of walls and pillars (uniform
//! theory of diffraction, Kouyoumjian & Pathak 1974), added to the
//! image-source field.
//!
//! All obstacles are full-height prisms, so every diffracting edge is a
//! vertical line from floor to ceiling and all edges are parallel. A path
//! S → edge → R (and S → edge → edge → R) then unfolds to a straight line in
//! (horizontal arc length, z), which fixes the diffraction points exactly
//! and keeps the edge angle β the same along the whole path. The UTD
//! coefficient makes the total field continuous across the shadow and
//! reflection boundaries where the image-source field switches paths on and
//! off; second order fills the deep shadow behind thick pillars, which is
//! reached only around two corners.

use std::f64::consts::{PI, TAU};
use std::sync::atomic::{AtomicBool, Ordering};

use glam::{DMat3, DVec2, DVec3};
use num_complex::{Complex32, Complex64};
use rayon::prelude::*;
use wavey_geom::{Room, SurfaceKind};

use crate::accumulate::AccumulateJob;
use crate::directivity::{Cell, Directivity};
use crate::ism::{Image, validate};

/// Distance the diffraction point is moved off the edge into the air, so
/// visibility tests do not graze the edge's own faces.
const NUDGE: f64 = 1e-4;

/// A diffracting vertical edge: an air wedge wider than π.
#[derive(Debug, Clone)]
pub struct Edge {
    pub p: DVec2,
    pub height: f64,
    /// Direction (rad) of the o-face, from which edge angles are measured
    /// counter-clockwise through the air.
    pub theta0: f64,
    /// Exterior (air) wedge angle nπ.
    pub wedge: f64,
    /// Surfaces of the o-face and the n-face.
    pub faces: [u32; 2],
}

impl Edge {
    pub fn n(&self) -> f64 {
        self.wedge / PI
    }

    /// Angle of `q` around the edge from the o-face; None inside the solid.
    pub fn angle(&self, q: DVec2) -> Option<f64> {
        let d = q - self.p;
        if d.length_squared() < 1e-18 {
            return None;
        }
        let a = (d.y.atan2(d.x) - self.theta0).rem_euclid(TAU);
        if a <= self.wedge + 1e-9 {
            Some(a.min(self.wedge))
        } else if a >= TAU - 1e-9 {
            Some(0.0)
        } else {
            None
        }
    }

    fn bisector(&self) -> DVec2 {
        let a = self.theta0 + 0.5 * self.wedge;
        DVec2::new(a.cos(), a.sin())
    }

    /// The diffraction point at height `z`, just off the edge in the air.
    pub fn point(&self, z: f64) -> DVec3 {
        let b = self.p + self.bisector() * NUDGE;
        DVec3::new(b.x, b.y, z)
    }
}

/// Smallest absolute difference of two angles.
fn angle_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(TAU);
    d.min(TAU - d)
}

/// All diffracting vertical edges of the room: wall corners whose air wedge
/// exceeds π (pillar corners, wall ends, free-standing walls).
pub fn find_edges(room: &Room) -> Vec<Edge> {
    // Wall base segments, as (vertex, direction away from it, surface).
    let mut ends: Vec<(DVec2, f64, u32)> = Vec::new();
    for (si, s) in room.surfaces.iter().enumerate() {
        if s.kind != SurfaceKind::Wall {
            continue;
        }
        let mut base: Vec<DVec2> = Vec::new();
        for &t in &s.triangles {
            for &vi in &room.triangles[t as usize] {
                let p = room.positions[vi as usize];
                if p.z.abs() < 1e-9 && !base.iter().any(|b| b.distance(p.truncate()) < 1e-9) {
                    base.push(p.truncate());
                }
            }
        }
        if base.len() != 2 {
            continue;
        }
        let d = base[1] - base[0];
        ends.push((base[0], d.y.atan2(d.x), si as u32));
        ends.push((base[1], (-d).y.atan2(-d.x), si as u32));
    }
    let mut groups: Vec<(DVec2, Vec<(f64, u32)>)> = Vec::new();
    for (p, a, s) in ends {
        match groups.iter_mut().find(|g| g.0.distance(p) < 1e-6) {
            Some(g) => g.1.push((a.rem_euclid(TAU), s)),
            None => groups.push((p, vec![(a.rem_euclid(TAU), s)])),
        }
    }

    let zmid = room.height * 0.5;
    let mut edges = Vec::new();
    for (p, mut walls) in groups {
        walls.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut dirs: Vec<f64> = Vec::new();
        for (a, _) in &walls {
            if dirs.last().is_none_or(|l| angle_diff(*a, *l) > 1e-9) {
                dirs.push(*a);
            }
        }
        if dirs.len() > 1 && angle_diff(dirs[0], *dirs.last().unwrap()) < 1e-9 {
            dirs.pop();
        }
        // The face along `ang` whose normal points to the given side
        // (+1: counter-clockwise, -1: clockwise).
        let face = |ang: f64, side: f64| {
            let dir = DVec2::new(ang.cos(), ang.sin());
            walls
                .iter()
                .filter(|(a, _)| angle_diff(*a, ang) < 1e-9)
                .map(|(_, s)| *s)
                .find(|&s| room.surfaces[s as usize].normal.truncate().dot(dir.perp()) * side > 0.0)
        };
        for i in 0..dirs.len() {
            let start = dirs[i];
            let end = if i + 1 < dirs.len() { dirs[i + 1] } else { dirs[0] + TAU };
            let width = end - start;
            if width < PI + 1e-3 {
                continue;
            }
            let mid = start + 0.5 * width;
            let probe = p + DVec2::new(mid.cos(), mid.sin()) * 1e-3;
            if !room.contains(DVec3::new(probe.x, probe.y, zmid)) {
                continue;
            }
            if let (Some(f0), Some(f1)) = (face(start, 1.0), face(end, -1.0)) {
                edges.push(Edge { p, height: room.height, theta0: start, wedge: width, faces: [f0, f1] });
            }
        }
    }
    edges
}

/// Fresnel integrals C(x) = ∫₀ˣ cos(πt²/2) dt and S(x) (Numerical Recipes).
pub fn fresnel(x: f64) -> (f64, f64) {
    const EPS: f64 = 1e-15;
    const MAXIT: usize = 200;
    let ax = x.abs();
    let (c, s) = if ax < 1e-150 {
        (ax, 0.0)
    } else if ax <= 1.5 {
        let fact = PI / 2.0 * ax * ax;
        let (mut sum, mut sums, mut sumc) = (0.0, 0.0, ax);
        let (mut sign, mut odd, mut term, mut n) = (1.0, true, ax, 3.0);
        for k in 1..=MAXIT {
            term *= fact / k as f64;
            sum += sign * term / n;
            let test = sum.abs() * EPS;
            if odd {
                sign = -sign;
                sums = sum;
                sum = sumc;
            } else {
                sumc = sum;
                sum = sums;
            }
            if term < test {
                break;
            }
            odd = !odd;
            n += 2.0;
        }
        (sumc, sums)
    } else {
        // Continued fraction for the complementary error function (Lentz).
        let pix2 = PI * ax * ax;
        let mut b = Complex64::new(1.0, -pix2);
        let mut cc = Complex64::new(1e300, 0.0);
        let mut d = 1.0 / b;
        let mut h = d;
        let mut n = -1.0;
        for _ in 2..=MAXIT {
            n += 2.0;
            let a = -(n * (n + 1.0));
            b += 4.0;
            d = 1.0 / (a * d + b);
            cc = b + a / cc;
            let del = cc * d;
            h *= del;
            if (del.re - 1.0).abs() + del.im.abs() < EPS {
                break;
            }
        }
        h *= Complex64::new(ax, -ax);
        let cs = Complex64::new(0.5, 0.5) * (1.0 - Complex64::from_polar(1.0, 0.5 * pix2) * h);
        (cs.re, cs.im)
    };
    if x < 0.0 { (-c, -s) } else { (c, s) }
}

/// UTD transition function F(X) = 2j√X e^{jX} ∫_{√X}^∞ e^{-jτ²} dτ
/// (e^{jωt} convention): 0 at X = 0, → 1 for large X.
pub fn transition(x: f64) -> Complex64 {
    if x <= 0.0 {
        return Complex64::new(0.0, 0.0);
    }
    if x > 60.0 {
        // Asymptotic series Σ jᵐ (2m−1)!! / (2X)ᵐ.
        let i = 1.0 / x;
        return Complex64::new(1.0 - 0.75 * i * i + 105.0 / 16.0 * i.powi(4), 0.5 * i - 15.0 / 8.0 * i.powi(3) + 945.0 / 32.0 * i.powi(5));
    }
    let u = x.sqrt();
    let (c, s) = fresnel(u * (2.0 / PI).sqrt());
    let integral = (PI / 2.0).sqrt() * Complex64::new(0.5 - c, -(0.5 - s));
    Complex64::new(0.0, 2.0 * u) * Complex64::from_polar(1.0, x) * integral
}

/// Frequency-independent parts of one term cot((π ± β)/2n)·F(kL a±(β)):
/// the term is c·F(kL·a) + e^{jπ/4}(s1·√(2πkL) − 2kL·s2·e^{jπ/4}), where
/// the second part is the finite limit on the boundary where the cotangent
/// is singular (then c = 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TermParts {
    pub c: f64,
    pub a: f64,
    pub s1: f64,
    pub s2: f64,
}

fn term_parts(n: f64, beta: f64, sign: f64) -> TermParts {
    let big_n = ((beta + sign * PI) / (TAU * n)).round();
    let eps = PI + sign * beta - TAU * n * big_n;
    if eps.abs() < 1e-6 {
        return TermParts { c: 0.0, a: 0.0, s1: n * eps.signum(), s2: n * eps };
    }
    let a = 2.0 * ((TAU * n * big_n - beta) / 2.0).cos().powi(2);
    let c = 1.0 / ((PI + sign * beta) / (2.0 * n)).tan();
    TermParts { c, a, s1: 0.0, s2: 0.0 }
}

fn eval_term(t: &TermParts, kl: f64) -> Complex64 {
    let e4 = Complex64::from_polar(1.0, PI / 4.0);
    let mut v = t.c * transition(kl * t.a);
    if t.s1 != 0.0 {
        v += ((TAU * kl).sqrt() * t.s1 - 2.0 * kl * t.s2 * e4) * e4;
    }
    v
}

/// The four terms of [`coefficient`] in its order: (φ−φ')±, then (φ+φ')
/// with the o-face (×R0) and the n-face (×Rn).
pub fn coefficient_terms(n: f64, phi: f64, phi_in: f64) -> [TermParts; 4] {
    let (dm, dp) = (phi - phi_in, phi + phi_in);
    [term_parts(n, dm, 1.0), term_parts(n, dm, -1.0), term_parts(n, dp, -1.0), term_parts(n, dp, 1.0)]
}

/// Frequency-independent prefactor of [`coefficient`] without 1/√k:
/// −e^{−jπ/4} / (2n√(2π) sin β).
pub fn coefficient_prefactor(n: f64, sin_beta: f64) -> Complex64 {
    -Complex64::from_polar(1.0, -PI / 4.0) / (2.0 * n * TAU.sqrt() * sin_beta)
}

/// Kouyoumjian–Pathak diffraction coefficient (units √m) for wedge index
/// n, wavenumber k, distance parameter L, observation and incidence
/// angles φ, φ' and edge angle β. `r0`/`rn` are the reflection factors of
/// the o- and n-face (1 = rigid).
#[allow(clippy::too_many_arguments)]
pub fn coefficient(n: f64, k: f64, l: f64, phi: f64, phi_in: f64, sin_beta: f64, r0: Complex64, rn: Complex64) -> Complex64 {
    let kl = k * l;
    let t = coefficient_terms(n, phi, phi_in);
    coefficient_prefactor(n, sin_beta) / k.sqrt()
        * (eval_term(&t[0], kl) + eval_term(&t[1], kl) + r0 * eval_term(&t[2], kl) + rn * eval_term(&t[3], kl))
}

/// One edge interaction along a diffracted path.
#[derive(Debug, Clone, Copy)]
pub struct EdgeHit {
    pub edge: u32,
    pub phi_in: f64,
    pub phi_out: f64,
    /// UTD distance parameter (m, including sin²β).
    pub l: f64,
    /// 1/2 for grazing incidence along a face (avoids counting the face
    /// reflection twice), else 1.
    pub factor: f64,
}

/// A diffracted path from a source (or one of its images) to a receiver.
#[derive(Debug, Clone)]
pub struct DiffPath {
    /// Unfolded path length (m).
    pub r: f64,
    /// Geometric spreading 1/√(s₁…s_m · r) (1/m).
    pub spread: f64,
    pub sin_beta: f64,
    pub cell: Cell,
    pub reflections: Vec<(u32, f32)>,
    pub hits: Vec<EdgeHit>,
}

/// Image order meaning "none" in [`DiffractionSettings`].
pub const NONE: i32 = -1;

/// Which diffracted paths to search for (image orders; [`NONE`] disables).
#[derive(Debug, Clone, Copy)]
pub struct DiffractionSettings {
    /// Images up to this reflection order are diffracted once.
    pub single_image_order: i32,
    /// Images up to this order are diffracted along chains of edges.
    pub chain_image_order: i32,
    /// Longest chain. Beyond two edges, chains only continue along faces
    /// (around the same pillar or wall end), where each order takes over at
    /// the shadow boundary of the previous one.
    pub max_edges: usize,
    /// Paths are dropped if, at wavenumber `prune_k` with rigid faces, they
    /// stay this many dB below the free-field level at the receiver's
    /// distance from the source (diffraction is strongest at low k, so pass
    /// the lowest simulated frequency). None keeps every path.
    pub prune: Option<(f64, f64)>,
}

impl Default for DiffractionSettings {
    fn default() -> Self {
        Self { single_image_order: 1, chain_image_order: 0, max_edges: 3, prune: None }
    }
}

/// Amplitude of a diffracted path at wavenumber `k` with rigid faces.
fn estimate(p: &DiffPath, edges: &[Edge], k: f64) -> f64 {
    let one = Complex64::new(1.0, 0.0);
    p.hits.iter().fold(p.spread, |a, h| {
        let e = &edges[h.edge as usize];
        a * h.factor * coefficient(e.n(), k, h.l, h.phi_out, h.phi_in, p.sin_beta, one, one).norm()
    })
}

/// Precomputed edge-to-edge visibility. Obstacles are full-height prisms,
/// so visibility between vertical edges does not depend on height.
pub struct EdgeGraph {
    pub edges: Vec<Edge>,
    /// Edges visible from each edge, with the outgoing and incoming angles.
    pub pairs: Vec<Vec<(u32, f64, f64)>>,
}

impl EdgeGraph {
    pub fn build(room: &Room) -> Self {
        // Vertical wall edges of extruded rooms only.
        let edges = if room.prism { find_edges(room) } else { vec![] };
        let z = room.height * 0.5;
        let pairs = (0..edges.len())
            .into_par_iter()
            .map(|i| {
                let a = &edges[i];
                (0..edges.len())
                    .filter(|&j| j != i)
                    .filter_map(|j| {
                        let b = &edges[j];
                        let out = a.angle(b.p)?;
                        let inc = b.angle(a.p)?;
                        // Paths straight along a face into the next edge are
                        // fine, but the edge must not lie on a's face line
                        // beyond the solid.
                        (!room.bvh.occluded(a.point(z), b.point(z), |_| false)).then_some((j as u32, out, inc))
                    })
                    .collect()
            })
            .collect();
        Self { edges, pairs }
    }
}

fn near_face(e: &Edge, phi: f64) -> bool {
    phi < 1e-6 || (e.wedge - phi) < 1e-6
}

/// Finds the diffracted paths from `images` to every receiver. Returns None
/// if cancelled.
#[allow(clippy::too_many_arguments)]
pub fn trace_diffraction(
    room: &Room,
    graph: &EdgeGraph,
    images: &[Image],
    to_local: DMat3,
    directivity: &Directivity,
    receivers: &[DVec3],
    settings: DiffractionSettings,
    cancel: &AtomicBool,
) -> Option<Vec<Vec<DiffPath>>> {
    let edges = &graph.edges;
    if edges.is_empty() {
        return Some(vec![Vec::new(); receivers.len()]);
    }
    let h = room.height;
    let max_order = settings.single_image_order.max(settings.chain_image_order);
    let sources: Vec<usize> = (0..images.len()).filter(|&i| images[i].order as i32 <= max_order).collect();
    let src_pos = images[0].pos;
    // Direct-source visibility of each edge (height independent).
    let direct_vis: Vec<bool> = edges.iter().map(|e| !room.bvh.occluded(src_pos, e.point(src_pos.z), |_| false)).collect();

    receivers
        .par_iter()
        .map(|&x| {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let rx_vis: Vec<bool> = edges.iter().map(|e| !room.bvh.occluded(e.point(x.z), x, |_| false)).collect();
            let mut out = Vec::new();
            let direct = (x - src_pos).length().max(0.1);
            let keep = |p: &DiffPath| match settings.prune {
                Some((k, db)) => estimate(p, edges, k) * direct >= 10f64.powf(-db / 20.0),
                None => true,
            };
            for &img in &sources {
                let q = images[img].pos;
                let order = images[img].order as i32;
                // Image → edge validation (reflections and occlusion).
                let reach = |e: usize, z: f64| {
                    if img == 0 {
                        direct_vis[e].then(|| crate::ism::Validated {
                            r: (edges[e].point(z) - q).length(),
                            departure: (edges[e].point(z) - q).normalize_or(DVec3::X),
                            refl: Vec::new(),
                            points: Vec::new(),
                        })
                    } else {
                        validate(room, images, img, edges[e].point(z))
                    }
                };
                for (ei, e) in edges.iter().enumerate() {
                    let Some(phi_in) = e.angle(q.truncate()) else { continue };
                    // Single diffraction.
                    if order <= settings.single_image_order && rx_vis[ei] {
                        if let Some(phi) = e.angle(x.truncate()) {
                            let (rs, rr) = (q.truncate().distance(e.p), x.truncate().distance(e.p));
                            let zd = q.z + (x.z - q.z) * rs / (rs + rr);
                            if zd > 0.0 && zd < h {
                                if let Some(v) = reach(ei, zd) {
                                    let (s, sr) = ((rs * rs + (zd - q.z).powi(2)).sqrt(), (rr * rr + (x.z - zd).powi(2)).sqrt());
                                    let sin_beta = rs / s;
                                    let p = DiffPath {
                                        r: s + sr,
                                        spread: 1.0 / (s * sr * (s + sr)).sqrt(),
                                        sin_beta,
                                        cell: directivity.cell(to_local * v.departure),
                                        reflections: v.refl,
                                        hits: vec![EdgeHit {
                                            edge: ei as u32,
                                            phi_in,
                                            phi_out: phi,
                                            l: s * sr / (s + sr) * sin_beta * sin_beta,
                                            factor: if near_face(e, phi_in) { 0.5 } else { 1.0 },
                                        }],
                                    };
                                    if keep(&p) {
                                        out.push(p);
                                    }
                                }
                            }
                        }
                    }
                    // Multiple diffraction along chains of edges.
                    if order <= settings.chain_image_order {
                        let mut chain = vec![(ei, phi_in, 0.0)];
                        extend_chains(graph, &rx_vis, x, settings.max_edges, &mut chain, &mut |chain| {
                            if let Some(p) = chain_path(edges, q, x, h, chain, &reach, directivity, to_local) {
                                if keep(&p) {
                                    out.push(p);
                                }
                            }
                        });
                    }
                }
            }
            Some(out)
        })
        .collect()
}

/// Depth-first search over edge chains starting with `chain[0]`; calls
/// `found` for every chain of two or more edges whose last edge sees the
/// receiver. Chain entries are (edge, incoming angle, outgoing angle of the
/// previous hop).
fn extend_chains(
    graph: &EdgeGraph,
    rx_vis: &[bool],
    x: DVec3,
    max_edges: usize,
    chain: &mut Vec<(usize, f64, f64)>,
    found: &mut dyn FnMut(&[(usize, f64, f64)]),
) {
    let last = chain.last().unwrap().0;
    for &(next, out, inc) in &graph.pairs[last] {
        let next = next as usize;
        if chain.iter().any(|c| c.0 == next) {
            continue;
        }
        // Third and later edges only along faces.
        if chain.len() >= 2 && !near_face(&graph.edges[next], inc) {
            continue;
        }
        chain.push((next, inc, out));
        if rx_vis[next] && graph.edges[next].angle(x.truncate()).is_some() {
            found(chain);
        }
        if chain.len() < max_edges {
            extend_chains(graph, rx_vis, x, max_edges, chain, found);
        }
        chain.pop();
    }
}

/// The diffracted path for an edge chain, if its diffraction points lie
/// within the edges and the image reaches the first one.
#[allow(clippy::too_many_arguments)]
fn chain_path(
    edges: &[Edge],
    q: DVec3,
    x: DVec3,
    h: f64,
    chain: &[(usize, f64, f64)],
    reach: &dyn Fn(usize, f64) -> Option<crate::ism::Validated>,
    directivity: &Directivity,
    to_local: DMat3,
) -> Option<DiffPath> {
    let m = chain.len();
    // Horizontal segment lengths: image → e1 → … → em → receiver.
    let mut hs = Vec::with_capacity(m + 1);
    hs.push(q.truncate().distance(edges[chain[0].0].p));
    for w in chain.windows(2) {
        hs.push(edges[w[0].0].p.distance(edges[w[1].0].p));
    }
    hs.push(edges[chain[m - 1].0].p.distance(x.truncate()));
    if hs.iter().any(|&v| v < 1e-9) {
        return None;
    }
    let hsum: f64 = hs.iter().sum();
    // The unfolded path is straight in (arc length, z).
    let mut acc = 0.0;
    let mut zs = Vec::with_capacity(m);
    for &hv in &hs[..m] {
        acc += hv;
        let z = q.z + (x.z - q.z) * acc / hsum;
        if !(z > 0.0 && z < h) {
            return None;
        }
        zs.push(z);
    }
    let v = reach(chain[0].0, zs[0])?;
    let stretch = (hsum * hsum + (x.z - q.z).powi(2)).sqrt() / hsum;
    let seg: Vec<f64> = hs.iter().map(|v| v * stretch).collect();
    let total: f64 = seg.iter().sum();
    let sin_beta = 1.0 / stretch;
    let sb2 = sin_beta * sin_beta;
    let phi_last = edges[chain[m - 1].0].angle(x.truncate())?;
    let mut hits = Vec::with_capacity(m);
    let mut remaining = total - seg[0];
    for i in 0..m {
        let e = &edges[chain[i].0];
        let phi_in = chain[i].1;
        let phi_out = if i + 1 < m { chain[i + 1].2 } else { phi_last };
        // UTD distance parameter for the wavefront arriving at this edge:
        // caustic at the previous edge (or the source) and the remaining
        // path to the receiver. Consistent hand-over between orders at
        // every shadow boundary.
        let s_in = seg[i];
        hits.push(EdgeHit {
            edge: chain[i].0 as u32,
            phi_in,
            phi_out,
            l: s_in * remaining / (s_in + remaining) * sb2,
            factor: if near_face(e, phi_in) { 0.5 } else { 1.0 },
        });
        remaining -= seg[i + 1];
    }
    Some(DiffPath {
        r: total,
        spread: 1.0 / (seg.iter().product::<f64>() * total).sqrt(),
        sin_beta,
        cell: directivity.cell(to_local * v.departure),
        reflections: v.refl,
        hits,
    })
}

/// Adds the diffracted field to `out` (`[receiver][freq]`, as from the
/// image-source accumulation).
pub fn accumulate(paths: &[Vec<DiffPath>], edges: &[Edge], job: &AccumulateJob, out: &mut [Complex32]) {
    let nf = job.spectra.freqs.len();
    out.par_chunks_mut(nf).zip(paths).for_each(|(row, rp)| {
        for p in rp {
            for (fi, cell) in row.iter_mut().enumerate() {
                let k = job.spectra.k[fi];
                let mut a = job.h0[fi] * job.directivity.eval(fi, &p.cell) * (job.r_ref * p.spread * (-job.spectra.air_np[fi] * p.r).exp());
                for &(s, cos) in &p.reflections {
                    a *= job.surfaces[s as usize].reflection_spherical(fi, cos as f64, k, p.r);
                }
                for hit in &p.hits {
                    let e = &edges[hit.edge as usize];
                    // Face reflection factors at the incidence angle on each face.
                    let cos0 = (hit.phi_in.sin() * p.sin_beta).abs();
                    let cosn = ((e.wedge - hit.phi_in).sin() * p.sin_beta).abs();
                    let r0 = job.surfaces[e.faces[0] as usize].reflection_spherical(fi, cos0, k, p.r);
                    let rn = job.surfaces[e.faces[1] as usize].reflection_spherical(fi, cosn, k, p.r);
                    a *= hit.factor * coefficient(e.n(), k, hit.l, hit.phi_out, hit.phi_in, p.sin_beta, r0, rn);
                }
                let v = a * Complex64::from_polar(1.0, -k * p.r);
                *cell += Complex32::new(v.re as f32, v.im as f32);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresnel_values() {
        // Reference values (numerical quadrature).
        for (x, c, s) in [(0.5, 0.4923442259, 0.0647324329), (1.0, 0.7798934004, 0.4382591474), (2.0, 0.4882534061, 0.3434156784), (5.0, 0.5636311887, 0.4991913819)] {
            let (fc, fs) = fresnel(x);
            assert!((fc - c).abs() < 1e-9 && (fs - s).abs() < 1e-9, "{x}: {fc} {fs}");
        }
    }

    #[test]
    fn transition_via_faddeeva() {
        // F(X) = √(πX) e^{jπ/4} w(√X e^{j3π/4}), the form the GPU uses.
        for x in [1e-4f64, 0.01, 0.3, 1.0, 4.0, 15.0, 40.0, 59.0] {
            let z = Complex64::from_polar(x.sqrt(), 0.75 * PI);
            let f = (PI * x).sqrt() * Complex64::from_polar(1.0, PI / 4.0) * crate::reflection::faddeeva(z);
            assert!((f - transition(x)).norm() < 1e-10, "{x}: {f} vs {}", transition(x));
        }
    }

    #[test]
    fn transition_limits_and_continuity() {
        assert!(transition(1e-8).norm() < 1e-3);
        assert!((transition(200.0) - 1.0).norm() < 1e-2);
        let (a, b) = (transition(60.0 - 1e-9), transition(60.0 + 1e-9));
        assert!((a - b).norm() < 1e-7, "{a} {b}");
        // Small-argument form (√(πX) − 2X e^{jπ/4}) e^{j(π/4 + X)}.
        let x = 1e-4;
        let e4 = Complex64::from_polar(1.0, PI / 4.0);
        let want = ((PI * x).sqrt() - 2.0 * x * e4) * Complex64::from_polar(1.0, PI / 4.0 + x);
        assert!((transition(x) - want).norm() < 1e-7, "{} vs {want}", transition(x));
    }

    /// Half-plane (n = 2), rigid: the total field (incident + reflected +
    /// diffracted) must be continuous across the shadow boundary, where the
    /// geometric incident field switches off.
    #[test]
    fn total_field_is_continuous_at_the_shadow_boundary() {
        let (n, k, rho_s, rho) = (2.0, 2.0 * PI * 300.0 / 343.0, 3.0, 2.0);
        let phi_in = 0.7;
        let field = |phi: f64| {
            let l = rho_s * rho / (rho_s + rho);
            let d = coefficient(n, k, l, phi, phi_in, 1.0, Complex64::new(1.0, 0.0), Complex64::new(1.0, 0.0));
            // Line geometry in the plane (β = 90°): diffracted wave.
            let diff = Complex64::from_polar(1.0, -k * rho_s) / rho_s * d * (rho_s / (rho * (rho_s + rho))).sqrt() * Complex64::from_polar(1.0, -k * rho);
            // Geometric incident field, present on the lit side φ < π + φ'.
            let dist = |a: f64| (rho_s * rho_s + rho * rho - 2.0 * rho_s * rho * (a).cos()).sqrt();
            let inc = if phi < PI + phi_in { Complex64::from_polar(1.0 / dist(phi - phi_in), -k * dist(phi - phi_in)) } else { Complex64::new(0.0, 0.0) };
            inc + diff
        };
        let sb = PI + phi_in;
        let (a, b) = (field(sb - 1e-7), field(sb + 1e-7));
        assert!((a - b).norm() < 1e-4 * a.norm(), "{a} vs {b}");
        // Reflection boundary of the o-face at φ = π − φ'.
        let field_r = |phi: f64| {
            let l = rho_s * rho / (rho_s + rho);
            let d = coefficient(n, k, l, phi, phi_in, 1.0, Complex64::new(1.0, 0.0), Complex64::new(1.0, 0.0));
            let diff = Complex64::from_polar(1.0, -k * rho_s) / rho_s * d * (rho_s / (rho * (rho_s + rho))).sqrt() * Complex64::from_polar(1.0, -k * rho);
            let dist = |a: f64| (rho_s * rho_s + rho * rho - 2.0 * rho_s * rho * (a).cos()).sqrt();
            let refl = if phi < PI - phi_in { Complex64::from_polar(1.0 / dist(phi + phi_in), -k * dist(phi + phi_in)) } else { Complex64::new(0.0, 0.0) };
            refl + diff
        };
        let rb = PI - phi_in;
        let (a, b) = (field_r(rb - 1e-7), field_r(rb + 1e-7));
        assert!((a - b).norm() < 1e-4 * a.norm(), "{a} vs {b}");
    }
}
