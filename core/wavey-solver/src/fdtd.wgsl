// FDTD leapfrog update and receiver DFT; see fdtd.rs for the scheme.

struct Grid {
    cells: u32,
    nx: u32,
    nxy: u32,
    n_src: u32,
    l2: f32,
    lam: f32,
    n_recv: u32,
    nf: u32,
    dt: f32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
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
@group(0) @binding(8) var<storage, read> bstart: array<u32>;           // slot range per boundary node
@group(0) @binding(9) var<storage, read> slots: array<vec4<f32>>;      // (1/D, cv, k, w), see fdtd.rs
@group(0) @binding(10) var<storage, read_write> sstate: array<vec2<f32>>; // (u, g) per slot
@group(0) @binding(11) var<storage, read> interior: array<u32>;        // one bit per cell: plain interior air
@group(0) @binding(12) var<storage, read> taps: array<vec2<f32>>;      // source taps: (cell as bits, weight)

// cell_info: bits 0–5 air neighbours, bit 6 air, bits 7–31 boundary index + 1.
const AIR: u32 = 64u;

fn flat(gid: vec3<u32>, nwg: vec3<u32>) -> u32 {
    return gid.x + gid.y * nwg.x * 64u;
}

@compute @workgroup_size(64)
fn update(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let i = flat(gid, nwg);
    if (i >= G.cells) {
        return;
    }
    // Plain interior air: no cell info to read. Same arithmetic, in the
    // same order, as the general path below.
    if (((interior[i >> 5u] >> (i & 31u)) & 1u) != 0u) {
        var s = 0.0;
        s = s + p[i - 1u];
        s = s + p[i + 1u];
        s = s + p[i - G.nx];
        s = s + p[i + G.nx];
        s = s + p[i - G.nxy];
        s = s + p[i + G.nxy];
        let c = p[i];
        prev[i] = 2.0 * c - prev[i] + G.l2 * (s - 6.0 * c);
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
    let pc = p[i];
    let lap = G.l2 * (sum - k * pc);
    let b = m >> 7u;
    var next: f32;
    if (b == 0u) {
        next = 2.0 * pc - prev[i] + lap;
    } else {
        // Frequency-dependent boundary: solve jointly with the branch states (see fdtd.rs).
        let j0 = bstart[b - 1u];
        let j1 = bstart[b];
        let pp = prev[i];
        var s1 = 0.0;
        var s2 = 0.0;
        for (var j = j0; j < j1; j = j + 1u) {
            let sl = slots[j];
            let st = sstate[j];
            let e = (sl.y * st.x - sl.z * st.y + 0.5 * pc + sl.w * pp) * sl.x;
            s1 = s1 + (0.5 - sl.w) * sl.x;
            s2 = s2 + e - st.x;
        }
        next = (2.0 * pc - pp + lap - G.lam * s2) / (1.0 + G.lam * s1);
        for (var j = j0; j < j1; j = j + 1u) {
            let sl = slots[j];
            let st = sstate[j];
            let e = (sl.y * st.x - sl.z * st.y + 0.5 * pc + sl.w * pp) * sl.x;
            let u = (0.5 - sl.w) * sl.x * next + e;
            sstate[j] = vec2<f32>(u, st.y + 0.5 * G.dt * (u + st.x));
        }
    }
    prev[i] = next;
}

// After `update`: the source signal into its taps (distinct cells, so no two
// threads write the same one).
@compute @workgroup_size(64)
fn inject(@builtin(global_invocation_id) gid: vec3<u32>) {
    let t = gid.x;
    if (t >= G.n_src) {
        return;
    }
    let tap = taps[t];
    let cell = bitcast<u32>(tap.x);
    prev[cell] = prev[cell] + S.s * tap.y;
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
