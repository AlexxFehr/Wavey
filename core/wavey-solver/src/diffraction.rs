//! Edge diffraction around the convex edges of a room (uniform theory of
//! diffraction, Kouyoumjian & Pathak 1974), added to the image-source field.
//!
//! Every diffracting edge is a straight segment where two room faces meet
//! with an air wedge wider than π: pillar corners, wall ends, beam and
//! balcony edges, stage lips. Each edge has a frame (e1, e2, t) with t along
//! the edge; parallel edges share it, so their positions and angles are
//! comparable. A path S → edge → R (and through chains of parallel edges)
//! unfolds to a straight line in (arc length across the edges, axial
//! position), which fixes the diffraction points exactly and keeps the edge
//! angle β the same along the whole path. Chains of non-parallel edges find
//! their diffraction points iteratively (the path length is convex in them).
//! The UTD coefficient makes the total field continuous across the shadow
//! and reflection boundaries where the image-source field switches paths on
//! and off; second order fills the deep shadow behind thick pillars, which is
//! reached only around two corners.
//!
//! In extruded rooms all edges are vertical and full height, so visibility
//! does not depend on height and is precomputed; in other rooms it is tested
//! at each path's actual diffraction points.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};
use std::sync::atomic::{AtomicBool, Ordering};

use glam::{DMat3, DVec2, DVec3};
use num_complex::{Complex32, Complex64};
use rayon::prelude::*;
use wavey_geom::Room;

use crate::accumulate::AccumulateJob;
use crate::directivity::{Cell, Directivity};
use crate::ism::{Image, validate};

/// Distance the diffraction point is moved off the edge into the air, so
/// visibility tests do not graze the edge's own faces.
const NUDGE: f64 = 1e-4;

/// A diffracting edge: an air wedge wider than π along a straight segment.
#[derive(Debug, Clone)]
pub struct Edge {
    /// Frame: `e1`, `e2` span the plane across the edge, `t` runs along it
    /// (right-handed; shared by all parallel edges).
    pub e1: DVec3,
    pub e2: DVec3,
    pub t: DVec3,
    /// Position of the edge line in (e1, e2).
    pub p: DVec2,
    /// Extent along `t`.
    pub lo: f64,
    pub hi: f64,
    /// Direction (rad, in the e1–e2 plane) of the o-face, from which edge
    /// angles are measured counter-clockwise through the air.
    pub theta0: f64,
    /// Exterior (air) wedge angle nπ.
    pub wedge: f64,
    /// Surfaces of the o-face and the n-face.
    pub faces: [u32; 2],
}

/// The edge frame for direction `t` (either sign): t made canonical, then
/// e1 = ẑ × t (x̂ for vertical edges) and e2 = t × e1.
fn frame(t: DVec3) -> (DVec3, DVec3, DVec3) {
    let flip = if t.z.abs() > 1e-12 { t.z < 0.0 } else if t.y.abs() > 1e-12 { t.y < 0.0 } else { t.x < 0.0 };
    let t = if flip { -t } else { t };
    if t.x.abs() < 1e-12 && t.y.abs() < 1e-12 {
        return (DVec3::X, DVec3::Y, DVec3::Z);
    }
    let e1 = DVec3::Z.cross(t).normalize();
    (e1, t.cross(e1), t)
}

impl Edge {
    pub fn n(&self) -> f64 {
        self.wedge / PI
    }

    /// `q` in the edge frame: position across the edge and along it.
    pub fn local(&self, q: DVec3) -> (DVec2, f64) {
        (DVec2::new(q.dot(self.e1), q.dot(self.e2)), q.dot(self.t))
    }

    /// Whether another edge is parallel (shares the frame).
    pub fn parallel(&self, o: &Edge) -> bool {
        self.t == o.t
    }

    /// Angle of `q` (in the edge frame) around the edge from the o-face;
    /// None inside the solid.
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

    /// The diffraction point at axial position `z`, just off the edge in the air.
    pub fn point(&self, z: f64) -> DVec3 {
        let b = self.p + self.bisector() * NUDGE;
        self.e1 * b.x + self.e2 * b.y + self.t * z
    }

    /// Whether axial position `z` lies strictly within the edge.
    fn within(&self, z: f64) -> bool {
        z > self.lo && z < self.hi
    }
}

/// Smallest absolute difference of two angles.
fn angle_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(TAU);
    d.min(TAU - d)
}

/// All diffracting edges of the room: mesh edges where faces meet with an
/// air wedge wider than π (pillar corners, wall ends, free-standing walls,
/// beam and balcony edges). Faces of one surface (and coplanar ones) do not
/// diffract; collinear pieces with the same faces are merged.
pub fn find_edges(room: &Room) -> Vec<Edge> {
    // Outside extruded rooms, curved surfaces come as facets meeting at a
    // few degrees: smooth, not edges. Only corners turning more than 17°
    // diffract there.
    let min_excess = if room.prism { 1e-3 } else { 0.3 };
    // Welded vertices: positions equal to within 1 µm share an id.
    let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
    let mut weld = |p: DVec3| {
        let k = [(p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64, (p.z * 1e6).round() as i64];
        let n = ids.len() as u32;
        *ids.entry(k).or_insert(n)
    };
    // Mesh edge (welded ids, low first) → its two endpoints and every
    // incident triangle as (surface, third vertex).
    let mut incident: HashMap<(u32, u32), (DVec3, DVec3, Vec<(u32, DVec3)>)> = HashMap::new();
    let mut order: Vec<(u32, u32)> = Vec::new();
    for (ti, tri) in room.triangles.iter().enumerate() {
        let v = tri.map(|i| room.positions[i as usize]);
        let w = v.map(&mut weld);
        for k in 0..3 {
            let (i, j, o) = (k, (k + 1) % 3, (k + 2) % 3);
            let (key, a, b) = if w[i] < w[j] { ((w[i], w[j]), v[i], v[j]) } else { ((w[j], w[i]), v[j], v[i]) };
            if key.0 == key.1 {
                continue;
            }
            let e = incident.entry(key).or_insert_with(|| {
                order.push(key);
                (a, b, Vec::new())
            });
            e.2.push((room.tri_surface[ti], v[o]));
        }
    }

    let mut edges: Vec<Edge> = Vec::new();
    for key in order {
        let (a, b, tris) = &incident[&key];
        if tris.len() < 2 {
            continue;
        }
        let (e1, e2, t) = frame((*b - *a).normalize());
        let (za, zb) = (a.dot(t), b.dot(t));
        let (lo, hi) = (za.min(zb), za.max(zb));
        let p = DVec2::new(a.dot(e1), a.dot(e2));
        // Direction of each face away from the edge, in the e1–e2 plane.
        let mut faces: Vec<(f64, u32)> = tris
            .iter()
            .map(|(s, o)| {
                let d = DVec2::new(o.dot(e1), o.dot(e2)) - p;
                (d.y.atan2(d.x).rem_euclid(TAU), *s)
            })
            .collect();
        faces.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut dirs: Vec<f64> = Vec::new();
        for (ang, _) in &faces {
            if dirs.last().is_none_or(|l| angle_diff(*ang, *l) > 1e-9) {
                dirs.push(*ang);
            }
        }
        if dirs.len() > 1 && angle_diff(dirs[0], *dirs.last().unwrap()) < 1e-9 {
            dirs.pop();
        }
        // The face along `ang` whose normal points to the given side
        // (+1: counter-clockwise, -1: clockwise).
        let face = |ang: f64, side: f64| {
            let dir = DVec2::new(ang.cos(), ang.sin());
            faces
                .iter()
                .filter(|(f, _)| angle_diff(*f, ang) < 1e-9)
                .map(|(_, s)| *s)
                .find(|&s| {
                    let n = room.surfaces[s as usize].normal;
                    DVec2::new(n.dot(e1), n.dot(e2)).dot(dir.perp()) * side > 0.0
                })
        };
        let zmid = 0.5 * (lo + hi);
        for i in 0..dirs.len() {
            let start = dirs[i];
            let end = if i + 1 < dirs.len() { dirs[i + 1] } else { dirs[0] + TAU };
            let width = end - start;
            if width < PI + min_excess {
                continue;
            }
            let mid = start + 0.5 * width;
            let probe = p + DVec2::new(mid.cos(), mid.sin()) * 1e-3;
            if !room.contains(e1 * probe.x + e2 * probe.y + t * zmid) {
                continue;
            }
            if let (Some(f0), Some(f1)) = (face(start, 1.0), face(end, -1.0)) {
                edges.push(Edge { e1, e2, t, p, lo, hi, theta0: start, wedge: width, faces: [f0, f1] });
            }
        }
    }
    merge_collinear(edges)
}

/// Joins pieces of one edge (same line, faces and wedge) that touch.
fn merge_collinear(mut edges: Vec<Edge>) -> Vec<Edge> {
    let same = |a: &Edge, b: &Edge| {
        a.parallel(b) && a.faces == b.faces && a.p.distance(b.p) < 1e-6 && angle_diff(a.theta0, b.theta0) < 1e-9 && (a.wedge - b.wedge).abs() < 1e-9
    };
    let mut out: Vec<Edge> = Vec::new();
    while let Some(mut e) = edges.pop() {
        loop {
            let Some(i) = edges.iter().position(|o| same(&e, o) && o.lo <= e.hi + 1e-6 && o.hi >= e.lo - 1e-6) else { break };
            let o = edges.swap_remove(i);
            e.lo = e.lo.min(o.lo);
            e.hi = e.hi.max(o.hi);
        }
        out.push(e);
    }
    out.reverse();
    out
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

/// The UTD transition function tabulated for the GPU: F at u = √X on a
/// uniform grid of `TRANSITION_N` intervals over 0 ≤ X ≤ 60 (above, the
/// asymptotic series is used). Read with 4-point cubic interpolation in u,
/// whose error is far below f32 rounding (see the test).
pub const TRANSITION_N: usize = 4096;
pub const TRANSITION_X_MAX: f64 = 60.0;

pub fn transition_table() -> Vec<[f32; 2]> {
    let h = TRANSITION_X_MAX.sqrt() / TRANSITION_N as f64;
    (0..=TRANSITION_N)
        .map(|j| {
            let u = j as f64 * h;
            let f = transition(u * u);
            [f.re as f32, f.im as f32]
        })
        .collect()
}

/// The GPU's interpolation of `transition_table` (same formula), for tests.
pub fn transition_interp(table: &[[f32; 2]], x: f64) -> Complex64 {
    let h = TRANSITION_X_MAX.sqrt() / TRANSITION_N as f64;
    let t = x.sqrt() / h;
    let j = (t.floor() as usize).clamp(1, TRANSITION_N - 2);
    let s = t - j as f64;
    let w = [-s * (s - 1.0) * (s - 2.0) / 6.0, (s + 1.0) * (s - 1.0) * (s - 2.0) / 2.0, -(s + 1.0) * s * (s - 2.0) / 2.0, (s + 1.0) * s * (s - 1.0) / 6.0];
    (0..4).map(|k| Complex64::new(table[j - 1 + k][0] as f64, table[j - 1 + k][1] as f64) * w[k]).sum()
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
    /// Sine of the angle between the ray and the edge (the same on both
    /// sides of the edge: Keller's cone).
    pub sin_beta: f64,
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
        a * h.factor * coefficient(e.n(), k, h.l, h.phi_out, h.phi_in, h.sin_beta, one, one).norm()
    })
}

/// Precomputed edge-to-edge visibility. In extruded rooms all edges are
/// full-height verticals, so it does not depend on height; elsewhere it is
/// a prefilter and each path is tested at its own diffraction points.
pub struct EdgeGraph {
    pub edges: Vec<Edge>,
    /// Edges visible from each edge, with the outgoing and incoming angles.
    pub pairs: Vec<Vec<(u32, f64, f64)>>,
    /// Visibility depends on the position along the edges: tested per path.
    pub exact: bool,
}

impl EdgeGraph {
    pub fn build(room: &Room) -> Self {
        let edges = find_edges(room);
        let exact = !room.prism;
        let visible = |a: DVec3, b: DVec3| !room.bvh.occluded(a, b, |_| false);
        let pairs = (0..edges.len())
            .into_par_iter()
            .map(|i| {
                let a = &edges[i];
                (0..edges.len())
                    .filter(|&j| j != i)
                    .filter_map(|j| {
                        let b = &edges[j];
                        let (out, inc) = if a.parallel(b) {
                            (a.angle(b.p)?, b.angle(a.p)?)
                        } else {
                            let (ma, mb) = (a.point(0.5 * (a.lo + a.hi)), b.point(0.5 * (b.lo + b.hi)));
                            (a.angle(a.local(mb).0)?, b.angle(b.local(ma).0)?)
                        };
                        // Paths straight along a face into the next edge are
                        // fine, but the edge must not lie on a's face line
                        // beyond the solid.
                        let seen = if !exact {
                            let z = 0.5 * (a.lo + a.hi);
                            visible(a.point(z), b.point(z))
                        } else {
                            // Parallel and overlapping: at common positions; else spread along both.
                            let (lo, hi) = (a.lo.max(b.lo), a.hi.min(b.hi));
                            [0.25, 0.5, 0.75].iter().any(|&f| {
                                if a.parallel(b) && hi > lo {
                                    let z = lo + f * (hi - lo);
                                    visible(a.point(z), b.point(z))
                                } else {
                                    visible(a.point(a.lo + f * (a.hi - a.lo)), b.point(b.lo + f * (b.hi - b.lo)))
                                }
                            })
                        };
                        seen.then_some((j as u32, out, inc))
                    })
                    .collect()
            })
            .collect();
        Self { edges, pairs, exact }
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
    let exact = graph.exact;
    let visible = |a: DVec3, b: DVec3| !room.bvh.occluded(a, b, |_| false);
    let max_order = settings.single_image_order.max(settings.chain_image_order);
    let sources: Vec<usize> = (0..images.len()).filter(|&i| images[i].order as i32 <= max_order).collect();
    let src_pos = images[0].pos;
    // Direct-source visibility of each edge (height independent in extruded
    // rooms; elsewhere tested per path).
    let direct_vis: Vec<bool> = if exact {
        vec![true; edges.len()]
    } else {
        edges.iter().map(|e| visible(src_pos, e.point(e.local(src_pos).1))).collect()
    };

    receivers
        .par_iter()
        .map(|&x| {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let xl: Vec<(DVec2, f64)> = edges.iter().map(|e| e.local(x)).collect();
            let rx_vis: Vec<bool> = if exact {
                vec![true; edges.len()]
            } else {
                edges.iter().zip(&xl).map(|(e, l)| visible(e.point(l.1), x)).collect()
            };
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
                        let pt = edges[e].point(z);
                        (direct_vis[e] && (!exact || visible(q, pt))).then(|| crate::ism::Validated {
                            r: (pt - q).length(),
                            departure: (pt - q).normalize_or(DVec3::X),
                            refl: Vec::new(),
                            points: Vec::new(),
                        })
                    } else {
                        validate(room, images, img, edges[e].point(z))
                    }
                };
                for (ei, e) in edges.iter().enumerate() {
                    let (q2, qz) = e.local(q);
                    let Some(phi_in) = e.angle(q2) else { continue };
                    // Single diffraction.
                    if order <= settings.single_image_order && rx_vis[ei] {
                        let (x2, xz) = xl[ei];
                        if let Some(phi) = e.angle(x2) {
                            let (rs, rr) = (q2.distance(e.p), x2.distance(e.p));
                            let zd = qz + (xz - qz) * rs / (rs + rr);
                            if e.within(zd) && (!exact || visible(e.point(zd), x)) {
                                if let Some(v) = reach(ei, zd) {
                                    let (s, sr) = ((rs * rs + (zd - qz).powi(2)).sqrt(), (rr * rr + (xz - zd).powi(2)).sqrt());
                                    let sin_beta = rs / s;
                                    let p = DiffPath {
                                        r: s + sr,
                                        spread: 1.0 / (s * sr * (s + sr)).sqrt(),
                                        cell: directivity.cell(to_local * v.departure),
                                        reflections: v.refl,
                                        hits: vec![EdgeHit {
                                            edge: ei as u32,
                                            phi_in,
                                            phi_out: phi,
                                            l: s * sr / (s + sr) * sin_beta * sin_beta,
                                            sin_beta,
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
                        extend_chains(graph, &rx_vis, &xl, settings.max_edges, &mut chain, &mut |chain| {
                            let found = if chain.iter().all(|c| edges[c.0].parallel(e)) {
                                chain_path(edges, exact, &visible, q, x, chain, &reach, directivity, to_local)
                            } else {
                                skew_chain_path(edges, exact, &visible, q, x, chain, &reach, directivity, to_local)
                            };
                            if let Some(p) = found {
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
/// receiver (`xl`: the receiver in each edge's frame). Chain entries are
/// (edge, incoming angle, outgoing angle of the previous hop).
fn extend_chains(
    graph: &EdgeGraph,
    rx_vis: &[bool],
    xl: &[(DVec2, f64)],
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
        if rx_vis[next] && graph.edges[next].angle(xl[next].0).is_some() {
            found(chain);
        }
        if chain.len() < max_edges {
            extend_chains(graph, rx_vis, xl, max_edges, chain, found);
        }
        chain.pop();
    }
}

type Reach<'a> = dyn Fn(usize, f64) -> Option<crate::ism::Validated> + 'a;

/// Whether consecutive diffraction points (and the last one and the
/// receiver) see each other.
fn chain_visible(edges: &[Edge], chain: &[(usize, f64, f64)], zs: &[f64], x: DVec3, visible: &dyn Fn(DVec3, DVec3) -> bool) -> bool {
    let pts: Vec<DVec3> = chain.iter().zip(zs).map(|(c, &z)| edges[c.0].point(z)).collect();
    pts.windows(2).all(|w| visible(w[0], w[1])) && visible(*pts.last().unwrap(), x)
}

/// The diffracted path for a chain of parallel edges, if its diffraction
/// points lie within the edges and the image reaches the first one.
#[allow(clippy::too_many_arguments)]
fn chain_path(
    edges: &[Edge],
    exact: bool,
    visible: &dyn Fn(DVec3, DVec3) -> bool,
    q: DVec3,
    x: DVec3,
    chain: &[(usize, f64, f64)],
    reach: &Reach,
    directivity: &Directivity,
    to_local: DMat3,
) -> Option<DiffPath> {
    let m = chain.len();
    let first = &edges[chain[0].0];
    let ((q2, qz), (x2, xz)) = (first.local(q), first.local(x));
    // Segment lengths across the edges: image → e1 → … → em → receiver.
    let mut hs = Vec::with_capacity(m + 1);
    hs.push(q2.distance(first.p));
    for w in chain.windows(2) {
        hs.push(edges[w[0].0].p.distance(edges[w[1].0].p));
    }
    hs.push(edges[chain[m - 1].0].p.distance(x2));
    if hs.iter().any(|&v| v < 1e-9) {
        return None;
    }
    let hsum: f64 = hs.iter().sum();
    // The unfolded path is straight in (arc length, axial position).
    let mut acc = 0.0;
    let mut zs = Vec::with_capacity(m);
    for (i, &hv) in hs[..m].iter().enumerate() {
        acc += hv;
        let z = qz + (xz - qz) * acc / hsum;
        if !edges[chain[i].0].within(z) {
            return None;
        }
        zs.push(z);
    }
    if exact && !chain_visible(edges, chain, &zs, x, visible) {
        return None;
    }
    let v = reach(chain[0].0, zs[0])?;
    let stretch = (hsum * hsum + (xz - qz).powi(2)).sqrt() / hsum;
    let seg: Vec<f64> = hs.iter().map(|v| v * stretch).collect();
    let total: f64 = seg.iter().sum();
    let sin_beta = 1.0 / stretch;
    let sb2 = sin_beta * sin_beta;
    let phi_last = edges[chain[m - 1].0].angle(x2)?;
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
            sin_beta,
            factor: if near_face(e, phi_in) { 0.5 } else { 1.0 },
        });
        remaining -= seg[i + 1];
    }
    Some(DiffPath {
        r: total,
        spread: 1.0 / (seg.iter().product::<f64>() * total).sqrt(),
        cell: directivity.cell(to_local * v.departure),
        reflections: v.refl,
        hits,
    })
}

/// The diffracted path for a chain with non-parallel edges. The path length
/// is convex in the positions along the edges, so updating each in turn to
/// the single-edge stationary point (given its neighbours) converges to the
/// diffracted path.
#[allow(clippy::too_many_arguments)]
fn skew_chain_path(
    edges: &[Edge],
    exact: bool,
    visible: &dyn Fn(DVec3, DVec3) -> bool,
    q: DVec3,
    x: DVec3,
    chain: &[(usize, f64, f64)],
    reach: &Reach,
    directivity: &Directivity,
    to_local: DMat3,
) -> Option<DiffPath> {
    let m = chain.len();
    let es: Vec<&Edge> = chain.iter().map(|c| &edges[c.0]).collect();
    let on = |e: &Edge, z: f64| e.e1 * e.p.x + e.e2 * e.p.y + e.t * z;
    let mut zs: Vec<f64> = es.iter().map(|e| 0.5 * (e.lo + e.hi)).collect();
    for _ in 0..50 {
        let mut moved = 0.0f64;
        for i in 0..m {
            let a = if i == 0 { q } else { on(es[i - 1], zs[i - 1]) };
            let b = if i + 1 == m { x } else { on(es[i + 1], zs[i + 1]) };
            let ((a2, az), (b2, bz)) = (es[i].local(a), es[i].local(b));
            let (ra, rb) = (a2.distance(es[i].p), b2.distance(es[i].p));
            if ra + rb < 1e-12 {
                return None;
            }
            let z = az + (bz - az) * ra / (ra + rb);
            moved = moved.max((z - zs[i]).abs());
            zs[i] = z;
        }
        if moved < 1e-9 {
            break;
        }
    }
    if (0..m).any(|i| !es[i].within(zs[i])) {
        return None;
    }
    if exact && !chain_visible(edges, chain, &zs, x, visible) {
        return None;
    }
    // Path vertices: image, the diffraction points, receiver.
    let mut verts = vec![q];
    verts.extend((0..m).map(|i| on(es[i], zs[i])));
    verts.push(x);
    let seg: Vec<f64> = verts.windows(2).map(|w| w[0].distance(w[1])).collect();
    if seg.iter().any(|&s| s < 1e-9) {
        return None;
    }
    let v = reach(chain[0].0, zs[0])?;
    let total: f64 = seg.iter().sum();
    let mut hits = Vec::with_capacity(m);
    let mut remaining = total - seg[0];
    for i in 0..m {
        let e = es[i];
        let (a2, b2) = (e.local(verts[i]).0, e.local(verts[i + 2]).0);
        let (phi_in, phi_out) = (e.angle(a2)?, e.angle(b2)?);
        let sin_beta = (a2.distance(e.p) / seg[i]).min(1.0);
        let s_in = seg[i];
        hits.push(EdgeHit {
            edge: chain[i].0 as u32,
            phi_in,
            phi_out,
            l: s_in * remaining / (s_in + remaining) * sin_beta * sin_beta,
            sin_beta,
            factor: if near_face(e, phi_in) { 0.5 } else { 1.0 },
        });
        remaining -= seg[i + 1];
    }
    Some(DiffPath {
        r: total,
        spread: 1.0 / (seg.iter().product::<f64>() * total).sqrt(),
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
                    let cos0 = (hit.phi_in.sin() * hit.sin_beta).abs();
                    let cosn = ((e.wedge - hit.phi_in).sin() * hit.sin_beta).abs();
                    let r0 = job.surfaces[e.faces[0] as usize].reflection_spherical(fi, cos0, k, p.r);
                    let rn = job.surfaces[e.faces[1] as usize].reflection_spherical(fi, cosn, k, p.r);
                    a *= hit.factor * coefficient(e.n(), k, hit.l, hit.phi_out, hit.phi_in, hit.sin_beta, r0, rn);
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

    /// The vertical-only edge finder of extruded rooms (before general edges),
    /// as an oracle: (position, θ0, wedge, faces).
    fn old_find_edges(room: &Room) -> Vec<(DVec2, f64, f64, [u32; 2])> {
        use wavey_geom::SurfaceKind;
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
                    edges.push((p, start, width, [f0, f1]));
                }
            }
        }
        edges
    }

    fn pillar_room() -> Room {
        use std::collections::BTreeMap;
        let v = DVec2::new;
        let mut s = wavey_geom::Sketch::new();
        s.add_polygon(&[v(0.0, 0.0), v(12.0, 0.0), v(12.0, 9.0), v(6.0, 9.0), v(6.0, 6.0), v(0.0, 6.0)]);
        s.add_polygon(&[v(4.0, 2.0), v(5.0, 2.0), v(5.0, 3.0), v(4.0, 3.0)]);
        s.add_segment(v(8.0, 1.0), v(8.0, 4.0));
        wavey_geom::build_room(&s, 4.0, &BTreeMap::from([(1, v(1.0, 1.0))])).unwrap()
    }

    #[test]
    fn general_edges_match_the_vertical_finder_in_extruded_rooms() {
        let room = pillar_room();
        let old = old_find_edges(&room);
        let new = find_edges(&room);
        // Pillar corners (4), free-standing wall ends (2), re-entrant corner (1).
        assert_eq!(old.len(), 7);
        assert_eq!(new.len(), old.len());
        for (p, theta0, wedge, faces) in &old {
            let e = new.iter().find(|e| e.p.distance(*p) < 1e-9 && angle_diff(e.theta0, *theta0) < 1e-9).expect("edge");
            assert_eq!(e.t, DVec3::Z);
            assert_eq!((e.lo, e.hi), (0.0, 4.0));
            assert!((e.wedge - wedge).abs() < 1e-12 && e.faces == *faces);
        }
    }

    #[test]
    fn horizontal_edges_of_a_rotated_room() {
        // The same room as a mesh turned onto its side: the vertical edges
        // become horizontal ones along y.
        let room = pillar_room();
        let rot = DMat3::from_rotation_x(std::f64::consts::FRAC_PI_2);
        let verts: Vec<DVec3> = room.positions.iter().map(|p| rot * *p).collect();
        let faces: Vec<(Vec<u32>, u32)> = room.triangles.iter().zip(&room.tri_surface).map(|(t, s)| (t.to_vec(), *s)).collect();
        let turned = wavey_geom::room_from_mesh(&verts, &faces).unwrap();
        let edges = find_edges(&turned);
        assert_eq!(edges.len(), 7);
        for e in &edges {
            assert!(e.t.y.abs() > 1.0 - 1e-9, "{:?}", e.t);
            assert!((e.hi - e.lo - 4.0).abs() < 1e-9);
        }
    }

    #[test]
    fn skew_chain_finds_the_shortest_path() {
        // Two perpendicular edges: x-directed at (y 0, z 0) and z-directed at (x 2, y 1).
        let mk = |a: DVec3, b: DVec3| {
            let (e1, e2, t) = frame((b - a).normalize());
            let (za, zb) = (a.dot(t), b.dot(t));
            Edge { e1, e2, t, p: DVec2::new(a.dot(e1), a.dot(e2)), lo: za.min(zb), hi: za.max(zb), theta0: 0.0, wedge: 1.5 * PI, faces: [0, 0] }
        };
        let edges = vec![mk(DVec3::new(-5.0, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0)), mk(DVec3::new(2.0, 1.0, -5.0), DVec3::new(2.0, 1.0, 5.0))];
        let (q, x) = (DVec3::new(-1.0, -2.0, -1.5), DVec3::new(3.5, 3.0, 2.0));
        let on = |e: &Edge, z: f64| e.e1 * e.p.x + e.e2 * e.p.y + e.t * z;
        let len = |a: f64, b: f64| {
            let (p, r) = (on(&edges[0], a), on(&edges[1], b));
            q.distance(p) + p.distance(r) + r.distance(x)
        };
        // Brute force.
        let mut best = (f64::INFINITY, 0.0, 0.0);
        for i in 0..=400 {
            for j in 0..=400 {
                let (a, b) = (-5.0 + 0.025 * i as f64, -5.0 + 0.025 * j as f64);
                if len(a, b) < best.0 {
                    best = (len(a, b), a, b);
                }
            }
        }
        let reach = |_: usize, _: f64| Some(crate::ism::Validated { r: 0.0, departure: DVec3::X, refl: vec![], points: vec![] });
        let model = wavey_speaker::samples::omni();
        let dir = Directivity::from_model(&model, &[100.0]);
        let chain = [(0usize, 0.0, 0.0), (1usize, 0.0, 0.0)];
        // Angles may fall in the solid for these synthetic wedges; only the path length matters.
        let mut wide = edges.clone();
        wide.iter_mut().for_each(|e| e.wedge = TAU - 1e-6);
        let p = skew_chain_path(&wide, false, &|_, _| true, q, x, &chain, &reach, &dir, DMat3::IDENTITY).expect("path");
        assert!((p.r - best.0).abs() < 1e-4, "{} vs brute force {}", p.r, best.0);
        assert!(p.r <= best.0 + 1e-12);
    }


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
    fn transition_table_matches_the_function() {
        let table = transition_table();
        let mut worst = 0.0f64;
        for k in 1..20000 {
            let x = 60.0 * (k as f64 / 20000.0).powi(2);
            let e = (transition_interp(&table, x) - transition(x)).norm() / transition(x).norm().max(1e-3);
            worst = worst.max(e);
        }
        assert!(worst < 2e-6, "worst relative error {worst:e}");
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
