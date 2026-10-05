// Shared helpers for the accumulation kernels: complex arithmetic, the
// Faddeeva function and spherical-wave reflection (constants WL, WC are
// prepended by gpu.rs).

const TAU: f32 = 6.283185307179586;

fn cmul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}

fn cdiv(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    let d = dot(b, b);
    return vec2<f32>(a.x * b.x + a.y * b.y, a.y * b.x - a.x * b.y) / d;
}

// Faddeeva function (Weideman series), as reflection::faddeeva. For
// |z| ≥ 5 the Laplace continued fraction (8 terms) is used instead: it
// agrees with the series to ~2e-9 there (far below f32 rounding) at a
// fraction of the cost.
fn faddeeva_upper(z: vec2<f32>) -> vec2<f32> {
    if (dot(z, z) >= 25.0) {
        var d = z;
        for (var k = 8u; k >= 1u; k = k - 1u) {
            d = z - cdiv(vec2<f32>(0.5 * f32(k), 0.0), d);
        }
        return cdiv(vec2<f32>(0.0, 0.5641895835477563), d);
    }
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

