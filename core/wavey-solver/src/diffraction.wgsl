// Coherent sum of diffracted paths: one invocation per (receiver, frequency).
// Mirrors diffraction::accumulate; complex.wgsl is prepended and gpu.rs
// documents the data layout. The frequency-independent parts of every UTD
// term (cotangent, a±, singular limit, prefactor) come precomputed in f64.

struct Params {
    nf: u32,
    n_recv: u32,
    cells: u32,
    omni: u32,
    r_ref: f32,
    _p0: u32,
    _p1: u32,
    _p2: u32,
}

struct DPath {
    r_hi: f32,
    r_lo: f32,
    spread: f32,
    refl_start: u32,
    n_refl: u32,
    hit_start: u32,
    n_hits: u32,
    _p: u32,
    idx: vec4<u32>,
    w: vec4<f32>,
}

// One edge: distance parameter, incidence cosines on the two faces, their
// surfaces, the prefactor (incl. the grazing factor, without 1/√k) and the
// four terms as (c, a, s1, s2).
struct Hit {
    l: f32,
    cos0: f32,
    cosn: f32,
    f0: u32,
    f1: u32,
    _p: u32,
    pre: vec2<f32>,
    t: array<vec4<f32>, 4>,
}

struct Freq {
    h0: vec2<f32>,
    q_hi: f32,
    q_lo: f32,
    air: f32,
    _p0: f32,
    _p1: f32,
    _p2: f32,
}

struct Surf {
    r: vec2<f32>,
    zeta: vec2<f32>,
    spec: f32,
    has_zeta: u32,
    _p0: u32,
    _p1: u32,
}

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var<storage, read> offsets: array<u32>;
@group(0) @binding(2) var<storage, read> paths: array<DPath>;
@group(0) @binding(3) var<storage, read> refl: array<vec2<u32>>;
@group(0) @binding(4) var<storage, read> freqs: array<Freq>;
@group(0) @binding(5) var<storage, read> dir: array<vec2<f32>>;
@group(0) @binding(6) var<storage, read> surf: array<Surf>;
@group(0) @binding(7) var<storage, read_write> out: array<vec2<f32>>;
@group(0) @binding(8) var<storage, read> hits: array<Hit>;

const E4 = vec2<f32>(0.7071067811865476, 0.7071067811865476);

// UTD transition function, as diffraction::transition, through the
// Faddeeva function: F(X) = √(πX) e^{jπ/4} w(√X e^{j3π/4}).
fn transition(x: f32) -> vec2<f32> {
    if (x <= 0.0) {
        return vec2<f32>(0.0, 0.0);
    }
    if (x > 60.0) {
        let i = 1.0 / x;
        let i2 = i * i;
        return vec2<f32>(1.0 - 0.75 * i2 + 6.5625 * i2 * i2, 0.5 * i - 1.875 * i2 * i + 29.53125 * i2 * i2 * i);
    }
    let u = sqrt(x);
    let w = faddeeva_upper(vec2<f32>(-0.7071067811865476 * u, 0.7071067811865476 * u));
    return sqrt(3.141592653589793 * x) * cmul(E4, w);
}

fn term(t: vec4<f32>, kl: f32) -> vec2<f32> {
    var v = t.x * transition(kl * t.y);
    if (t.z != 0.0) {
        v = v + cmul(sqrt(TAU * kl) * t.z * vec2<f32>(1.0, 0.0) - 2.0 * kl * t.w * E4, E4);
    }
    return v;
}

fn face(s: u32, fi: u32, cosv: f32, k: f32, r: f32) -> vec2<f32> {
    let S = surf[s * P.nf + fi];
    var R = S.r;
    if (S.has_zeta != 0u) {
        R = spherical(S.zeta, cosv, k, r);
    }
    return R * S.spec;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let i = gid.x + gid.y * nwg.x * 64u;
    if (i >= P.n_recv * P.nf) {
        return;
    }
    let ri = i / P.nf;
    let fi = i % P.nf;
    let F = freqs[fi];
    let k = TAU * (F.q_hi + F.q_lo);
    let rk = inverseSqrt(k);
    var acc = vec2<f32>(0.0, 0.0);
    for (var pi = offsets[ri]; pi < offsets[ri + 1u]; pi = pi + 1u) {
        let p = paths[pi];
        var a = F.h0;
        if (P.omni == 0u) {
            let base = fi * P.cells;
            let v0 = dir[base + p.idx.x];
            let v1 = dir[base + p.idx.y];
            let v2 = dir[base + p.idx.z];
            let v3 = dir[base + p.idx.w];
            let db = p.w.x * v0.x + p.w.y * v1.x + p.w.z * v2.x + p.w.w * v3.x;
            let p0 = v0.y;
            let ph = p.w.x * p0 + p.w.y * (p0 + wrap180(v1.y - p0))
                   + p.w.z * (p0 + wrap180(v2.y - p0)) + p.w.w * (p0 + wrap180(v3.y - p0));
            let m = pow(10.0, db / 20.0);
            let rad = radians(ph);
            a = cmul(a, vec2<f32>(m * cos(rad), m * sin(rad)));
        }
        let r = p.r_hi + p.r_lo;
        for (var j = 0u; j < p.n_refl; j = j + 1u) {
            let rf = refl[p.refl_start + j];
            a = cmul(a, face(rf.x, fi, bitcast<f32>(rf.y), k, r));
        }
        for (var j = 0u; j < p.n_hits; j = j + 1u) {
            let h = hits[p.hit_start + j];
            let kl = k * h.l;
            let r0 = face(h.f0, fi, h.cos0, k, r);
            let rn = face(h.f1, fi, h.cosn, k, r);
            let sum = term(h.t[0], kl) + term(h.t[1], kl) + cmul(r0, term(h.t[2], kl)) + cmul(rn, term(h.t[3], kl));
            a = cmul(a, cmul(h.pre * rk, sum));
        }
        let mag = P.r_ref * p.spread * exp(-F.air * r);
        // Phase in cycles, split so the large product r_hi·q_hi is exact in f32.
        let c1 = fract(p.r_hi * F.q_hi);
        let rest = p.r_hi * F.q_lo + p.r_lo * F.q_hi + p.r_lo * F.q_lo;
        let ang = -TAU * fract(c1 + fract(rest));
        acc = acc + cmul(a, vec2<f32>(mag * cos(ang), mag * sin(ang)));
    }
    out[i] = acc;
}
