// Coherent image-source accumulation: one invocation per (receiver, frequency).
// Mirrors accumulate::accumulate_cpu; see gpu.rs for the data layout.

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

struct Path {
    r_hi: f32,
    r_lo: f32,
    refl_start: u32,
    order: u32,
    idx: vec4<u32>,
    w: vec4<f32>,
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
@group(0) @binding(2) var<storage, read> paths: array<Path>;
@group(0) @binding(3) var<storage, read> refl: array<vec2<u32>>;
@group(0) @binding(4) var<storage, read> freqs: array<Freq>;
@group(0) @binding(5) var<storage, read> dir: array<vec2<f32>>;
@group(0) @binding(6) var<storage, read> surf: array<Surf>;
@group(0) @binding(7) var<storage, read_write> out: array<vec2<f32>>;

const TAU: f32 = 6.283185307179586;

fn cmul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}

fn cdiv(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    let d = dot(b, b);
    return vec2<f32>(a.x * b.x + a.y * b.y, a.y * b.x - a.x * b.y) / d;
}

// Faddeeva function (Weideman series; constants WL, WC are prepended by
// gpu.rs), as reflection::faddeeva.
fn faddeeva_upper(z: vec2<f32>) -> vec2<f32> {
    let iz = vec2<f32>(-z.y, z.x);
    let den = vec2<f32>(WL, 0.0) - iz;
    let zz = cdiv(vec2<f32>(WL, 0.0) + iz, den);
    var p = vec2<f32>(0.0, 0.0);
    for (var j = 0u; j < 32u; j = j + 1u) {
        p = cmul(p, zz) + vec2<f32>(WC[j], 0.0);
    }
    return 2.0 * cdiv(p, cmul(den, den)) + cdiv(vec2<f32>(0.5641895835477563, 0.0), den);
}

fn faddeeva(z: vec2<f32>) -> vec2<f32> {
    if (z.y >= 0.0) {
        return faddeeva_upper(z);
    }
    // w(z) = 2 e^{−z²} − w(−z)
    let e = exp(min(z.y * z.y - z.x * z.x, 80.0));
    let ph = -2.0 * z.x * z.y;
    return 2.0 * e * vec2<f32>(cos(ph), sin(ph)) - faddeeva_upper(-z);
}

// Spherical-wave reflection factor, as reflection::spherical.
fn spherical(zeta: vec2<f32>, cosv: f32, k: f32, r: f32) -> vec2<f32> {
    let c = clamp(cosv, 0.0, 1.0);
    let beta = cdiv(vec2<f32>(1.0, 0.0), vec2<f32>(zeta.x, -zeta.y));
    let one = vec2<f32>(1.0, 0.0);
    let rp = cdiv(vec2<f32>(c, 0.0) - beta, vec2<f32>(c, 0.0) + beta);
    let w = cmul(vec2<f32>(0.5, 0.5), (vec2<f32>(c, 0.0) + beta) * sqrt(k * r));
    let ww = cmul(w, faddeeva(w));
    let f = one + 1.7724538509055159 * vec2<f32>(-ww.y, ww.x);
    let q = rp + cmul(one - rp, f);
    return vec2<f32>(q.x, -q.y);
}

fn wrap180(d: f32) -> f32 {
    return d - 360.0 * floor((d + 180.0) / 360.0);
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
    var acc = vec2<f32>(0.0, 0.0);
    for (var k = offsets[ri]; k < offsets[ri + 1u]; k = k + 1u) {
        let p = paths[k];
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
        let kw = TAU * (F.q_hi + F.q_lo);
        for (var j = 0u; j < p.order; j = j + 1u) {
            let rf = refl[p.refl_start + j];
            let s = surf[rf.x * P.nf + fi];
            var R = s.r;
            if (s.has_zeta != 0u) {
                R = spherical(s.zeta, bitcast<f32>(rf.y), kw, r);
            }
            a = cmul(a, R * s.spec);
        }
        let mag = P.r_ref / r * exp(-F.air * r);
        // Phase in cycles, split so the large product r_hi·q_hi is exact in f32.
        let c1 = fract(p.r_hi * F.q_hi);
        let rest = p.r_hi * F.q_lo + p.r_lo * F.q_hi + p.r_lo * F.q_lo;
        let ang = -TAU * fract(c1 + fract(rest));
        acc = acc + cmul(a, vec2<f32>(mag * cos(ang), mag * sin(ang)));
    }
    out[i] = acc;
}
