// FDTD leapfrog update and receiver DFT; see fdtd.rs for the scheme.

struct Grid {
    cells: u32,
    nx: u32,
    nxy: u32,
    source: u32,
    l2: f32,
    lam: f32,
    n_recv: u32,
    nf: u32,
}

struct Step {
    n: u32,
    s: f32,
}

@group(0) @binding(0) var<uniform> G: Grid;
@group(0) @binding(1) var<uniform> S: Step;
@group(0) @binding(2) var<storage, read> cell_info: array<u32>;
@group(0) @binding(3) var<storage, read> p: array<f32>;
@group(0) @binding(4) var<storage, read_write> prev: array<f32>;
@group(0) @binding(5) var<storage, read> recv: array<u32>;
@group(0) @binding(6) var<storage, read> q: array<vec2<f32>>;      // per freq: (q_hi, q_lo) of f·Δt
@group(0) @binding(7) var<storage, read_write> acc: array<vec2<f32>>; // [receiver][freq]

const AIR: u32 = 64u;
const BSCALE: f32 = 4.76837158203125e-7; // 2^-21

fn flat(gid: vec3<u32>, nwg: vec3<u32>) -> u32 {
    return gid.x + gid.y * nwg.x * 64u;
}

@compute @workgroup_size(64)
fn update(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let i = flat(gid, nwg);
    if (i >= G.cells) {
        return;
    }
    let m = cell_info[i];
    if ((m & AIR) == 0u) {
        return;
    }
    var sum = 0.0;
    var k = 0.0;
    if ((m & 1u) != 0u) { sum = sum + p[i - 1u]; k = k + 1.0; }
    if ((m & 2u) != 0u) { sum = sum + p[i + 1u]; k = k + 1.0; }
    if ((m & 4u) != 0u) { sum = sum + p[i - G.nx]; k = k + 1.0; }
    if ((m & 8u) != 0u) { sum = sum + p[i + G.nx]; k = k + 1.0; }
    if ((m & 16u) != 0u) { sum = sum + p[i - G.nxy]; k = k + 1.0; }
    if ((m & 32u) != 0u) { sum = sum + p[i + G.nxy]; k = k + 1.0; }
    let hb = 0.5 * G.lam * f32(m >> 8u) * BSCALE;
    var next = ((2.0 - k * G.l2) * p[i] + G.l2 * sum - (1.0 - hb) * prev[i]) / (1.0 + hb);
    if (i == G.source) {
        next = next + S.s;
    }
    prev[i] = next;
}

@compute @workgroup_size(64)
fn record(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let i = flat(gid, nwg);
    if (i >= G.n_recv * G.nf) {
        return;
    }
    let r = i / G.nf;
    let f = i % G.nf;
    let v = p[recv[r]];
    // Phase n·fΔt in cycles; n·q_hi is exact in f32 (see gpu.rs).
    let nn = f32(S.n);
    let cyc = fract(fract(nn * q[f].x) + fract(nn * q[f].y));
    let a = -6.283185307179586 * cyc;
    acc[i] = acc[i] + v * vec2<f32>(cos(a), sin(a));
}
