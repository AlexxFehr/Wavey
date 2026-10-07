// Metal shaders, compiled at runtime (the Command Line Tools have no
// offline Metal compiler).

let shaderSource = #"""
#include <metal_stdlib>
using namespace metal;

struct Frame {
    float4x4 viewProj;
    float4x4 inverse;
    float2 viewport;      // points
    float scale;          // pixels per point
    float gridZ;
    float3 eye;
    float isPlan;
    float gridStep;
    float metresPerPoint;
    float showGrid;
    float ortho;
    float4 bgCentre;
    float4 bgEdge;
    float4 gridMinor;
    float4 gridMajor;
    float4 gridAxis;
};

// ---- background ----

struct BgOut { float4 pos [[position]]; float2 uv; };

vertex BgOut bg_vs(uint vid [[vertex_id]]) {
    float2 p = float2((vid << 1) & 2, vid & 2);
    BgOut o;
    o.pos = float4(p * 2.0 - 1.0, 1.0, 1.0);
    o.uv = p;
    return o;
}

fragment float4 bg_fs(BgOut in [[stage_in]], constant Frame& F [[buffer(0)]]) {
    if (F.isPlan > 0.5) return F.bgCentre;
    float2 d = (in.uv - 0.5) * float2(F.viewport.x / max(F.viewport.y, 1.0), 1.0);
    float t = clamp(length(d) / 0.85, 0.0, 1.0);
    return mix(F.bgCentre, F.bgEdge, t);
}

// ---- procedural grid on z = gridZ ----

struct GridOut { float4 pos [[position]]; float2 ndc; };

vertex GridOut grid_vs(uint vid [[vertex_id]]) {
    float2 p = float2((vid << 1) & 2, vid & 2) * 2.0 - 1.0;
    GridOut o;
    o.pos = float4(p, 0.0, 1.0);
    o.ndc = p;
    return o;
}

struct GridFrag { float4 color [[color(0)]]; float depth [[depth(any)]]; };

static float gridLine(float2 w, float step, float px) {
    float2 g = abs(fract(w / step - 0.5) - 0.5) * step;
    float2 fw = fwidth(w) * px;
    float2 a = 1.0 - smoothstep(float2(0.0), fw, g);
    return max(a.x, a.y);
}

fragment GridFrag grid_fs(GridOut in [[stage_in]], constant Frame& F [[buffer(0)]]) {
    float4 a = F.inverse * float4(in.ndc, 0.0, 1.0);
    float4 b = F.inverse * float4(in.ndc, 1.0, 1.0);
    float3 pa = a.xyz / a.w, pb = b.xyz / b.w;
    float3 d = pb - pa;
    if (F.showGrid < 0.5 || abs(d.z) < 1e-9) discard_fragment();
    float t = (F.gridZ - pa.z) / d.z;
    if (t < 0.0 || t > 1.0) discard_fragment();
    float3 w = pa + d * t;
    float4 clip = F.viewProj * float4(w, 1.0);
    float minor = gridLine(w.xy, F.gridStep, 1.0);
    float major = gridLine(w.xy, F.gridStep * 5.0, 1.2);
    float2 ax = abs(w.xy) / max(fwidth(w.xy), 1e-6);
    float axis = 1.0 - smoothstep(0.0, 1.2, min(ax.x, ax.y));
    float4 c = float4(0.0);
    c = mix(c, F.gridMinor, minor);
    c = mix(c, F.gridMajor, major);
    c = mix(c, F.gridAxis, axis);
    if (F.isPlan < 0.5 && F.ortho < 0.5) {
        float dist = length(w - F.eye);
        c.a *= clamp(1.0 - dist / 120.0, 0.0, 1.0);
    }
    if (c.a < 0.01) discard_fragment();
    GridFrag o;
    o.color = c;
    o.depth = clamp(clip.z / clip.w, 0.0, 1.0);
    return o;
}

// ---- surfaces and meshes ----

struct MeshIn {
    packed_float3 pos;
    packed_float3 nrm;
    packed_float4 col;
};

struct Draw {
    float4 color;
    uint id;
    float lit;
    float useVertexColor;
    float pad;
};

struct MeshOut { float4 pos [[position]]; float4 col; float3 nrm; float3 world; };

vertex MeshOut mesh_vs(uint vid [[vertex_id]], const device MeshIn* v [[buffer(0)]],
                       constant Frame& F [[buffer(1)]], constant Draw& D [[buffer(2)]]) {
    MeshIn m = v[vid];
    MeshOut o;
    o.world = float3(m.pos);
    o.pos = F.viewProj * float4(o.world, 1.0);
    o.nrm = float3(m.nrm);
    o.col = D.useVertexColor > 0.5 ? float4(m.col) : D.color;
    return o;
}

fragment float4 mesh_fs(MeshOut in [[stage_in]], constant Frame& F [[buffer(0)]], constant Draw& D [[buffer(1)]],
                        bool front [[front_facing]]) {
    float4 c = in.col;
    if (D.lit > 0.5) {
        float3 n = normalize(in.nrm);
        float3 l = normalize(float3(0.35, -0.55, 0.75));
        float diff = abs(dot(n, l));
        c.rgb *= 0.55 + 0.45 * diff;
    }
    if (!front && D.lit > 0.5) c.rgb *= 0.8;
    return c;
}

fragment uint mesh_id_fs(MeshOut in [[stage_in]], constant Draw& D [[buffer(1)]]) {
    return D.id;
}

// ---- screen-space lines (instanced quads) ----

struct LineIn { packed_float3 a; packed_float3 b; };

struct LineDraw {
    float4 color;
    float width;
    float dashed;
    uint id;
    float pad;
};

struct LineOut { float4 pos [[position]]; float across; float along; float halfWidth; };

vertex LineOut line_vs(uint vid [[vertex_id]], uint iid [[instance_id]], const device LineIn* L [[buffer(0)]],
                       constant Frame& F [[buffer(1)]], constant LineDraw& D [[buffer(2)]]) {
    LineIn s = L[iid];
    float4 ca = F.viewProj * float4(float3(s.a), 1.0);
    float4 cb = F.viewProj * float4(float3(s.b), 1.0);
    // Clip against the near plane so segments behind the eye do not explode.
    if (ca.w < 1e-4 && cb.w < 1e-4) { LineOut o; o.pos = float4(0, 0, 2, 1); return o; }
    if (ca.w < 1e-4) { float t = (1e-4 - ca.w) / (cb.w - ca.w); ca = mix(ca, cb, t); }
    if (cb.w < 1e-4) { float t = (1e-4 - cb.w) / (ca.w - cb.w); cb = mix(cb, ca, t); }
    float2 sa = ca.xy / ca.w * 0.5 * F.viewport, sb = cb.xy / cb.w * 0.5 * F.viewport;
    float2 dir = sb - sa;
    float len = length(dir);
    dir = len > 1e-5 ? dir / len : float2(1, 0);
    float2 nrm = float2(-dir.y, dir.x);
    float hw = D.width * 0.5 + 0.75;
    uint corner = vid % 6;
    float side = (corner == 0 || corner == 2 || corner == 4) ? -1.0 : 1.0;
    bool end = (corner == 2 || corner == 4 || corner == 5);
    float4 c = end ? cb : ca;
    float2 off = nrm * side * hw + dir * (end ? hw : -hw) * 0.0;
    float2 ndcOff = off / (0.5 * F.viewport);
    LineOut o;
    o.pos = float4(c.xy + ndcOff * c.w, c.z, c.w);
    o.across = side * hw;
    o.along = end ? len : 0.0;
    o.halfWidth = D.width * 0.5;
    return o;
}

fragment float4 line_fs(LineOut in [[stage_in]], constant LineDraw& D [[buffer(1)]]) {
    if (D.dashed > 0.5 && fmod(in.along, 9.0) > 5.0) discard_fragment();
    float a = 1.0 - smoothstep(in.halfWidth - 0.5, in.halfWidth + 0.5, abs(in.across));
    float4 c = D.color;
    c.a *= a;
    if (c.a < 0.004) discard_fragment();
    return c;
}

// ---- screen-constant markers ----

struct MarkerIn {
    packed_float3 pos;
    float size;
    packed_float4 color;
    packed_float4 fill;
    uint shape;
    uint id;
    float pad0;
    float pad1;
};

struct MarkerOut { float4 pos [[position]]; float2 local; float size; float4 color; float4 fill; uint shape [[flat]]; uint id [[flat]]; };

vertex MarkerOut marker_vs(uint vid [[vertex_id]], uint iid [[instance_id]], const device MarkerIn* M [[buffer(0)]],
                           constant Frame& F [[buffer(1)]]) {
    MarkerIn m = M[iid];
    float4 c = F.viewProj * float4(float3(m.pos), 1.0);
    float2 corners[6] = { float2(-1,-1), float2(1,-1), float2(-1,1), float2(-1,1), float2(1,-1), float2(1,1) };
    float2 q = corners[vid % 6];
    float hs = m.size * 0.5 + 1.5;
    MarkerOut o;
    o.pos = float4(c.xy + q * hs / (0.5 * F.viewport) * c.w, c.z * 0.999, c.w);
    o.local = q * hs;
    o.size = m.size;
    o.color = float4(m.color);
    o.fill = float4(m.fill);
    o.shape = m.shape;
    o.id = m.id;
    return o;
}

// Signed distance to the marker outline (points); negative inside.
static float markerSDF(uint shape, float2 p, float r) {
    switch (shape) {
        case 0: case 1: return length(p) - r;                         // ring, dot
        case 2: case 3: { float2 d = abs(p) - r; return max(d.x, d.y); } // square
        case 4: { float k = sqrt(3.0); p.x = abs(p.x); p.y += r * 0.3;
                  return max(p.x * k * 0.5 + p.y * 0.5, -p.y) - r * 0.5; } // triangle
        case 6: { float2 d = abs(p); return (d.x + d.y) - r; }           // diamond
        default: return 1e9;
    }
}

fragment float4 marker_fs(MarkerOut in [[stage_in]]) {
    float r = in.size * 0.5;
    float stroke = 1.4;
    if (in.shape == 5) { // cross
        float2 d = abs(in.local);
        float l = min(max(d.x - r, d.y - 0.7), max(d.y - r, d.x - 0.7));
        float a = 1.0 - smoothstep(-0.5, 0.5, l);
        if (a < 0.01) discard_fragment();
        return float4(in.color.rgb, in.color.a * a);
    }
    float sd = markerSDF(in.shape, in.local, r);
    if (in.shape == 1) { // dot: filled
        float a = 1.0 - smoothstep(-0.5, 0.5, sd);
        if (a < 0.01) discard_fragment();
        return float4(in.color.rgb, in.color.a * a);
    }
    float edge = 1.0 - smoothstep(stroke * 0.5 - 0.5, stroke * 0.5 + 0.5, abs(sd));
    float inside = 1.0 - smoothstep(-0.5, 0.5, sd);
    float4 c = in.fill * inside * (1.0 - edge);
    c = c + float4(in.color.rgb * edge * in.color.a, edge * in.color.a);
    if (c.a < 0.01) discard_fragment();
    return float4(c.rgb / max(c.a, 1e-4), c.a);
}

fragment uint marker_id_fs(MarkerOut in [[stage_in]]) {
    float sd = markerSDF(in.shape == 5 ? 0u : in.shape, in.local, in.size * 0.5 + 3.0);
    if (sd > 0.0) discard_fragment();
    return in.id;
}

// ---- field quads ----

struct FieldDraw {
    float3 origin; float pad0;
    float3 uAxis; float pad1;
    float3 vAxis; float pad2;
    float lo;
    float hi;
    float opacity;
    float contour;
    float cells;
    uint id;
    float draped;
    float pad4;
};

struct FieldOut { float4 pos [[position]]; float2 uv; };

vertex FieldOut field_vs(uint vid [[vertex_id]], constant Frame& F [[buffer(1)]], constant FieldDraw& D [[buffer(2)]],
                         texture2d<float, access::read> H [[texture(0)]]) {
    float2 corners[6] = { float2(0,0), float2(1,0), float2(0,1), float2(0,1), float2(1,0), float2(1,1) };
    float2 uv = corners[vid % 6];
    float lift = 0.0;
    if (D.draped > 0.5) {
        // One quad per cell; each corner at the mean height of the cells
        // around it (outside cells, NaN, left out). Cells more than 0.35 m
        // from this one are across a step (stage edge, balcony front): left
        // out too, so the map breaks there instead of hanging a curtain.
        int2 size = int2(H.get_width(), H.get_height());
        uint cell = vid / 6;
        int2 c = int2(int(cell % uint(size.x)), int(cell / uint(size.x)));
        float own = H.read(uint2(c)).r;
        int2 k = c + int2(uv);
        float acc = 0.0, n = 0.0;
        for (int j = -1; j <= 0; j++) for (int i = -1; i <= 0; i++) {
            int2 q = k + int2(i, j);
            if (q.x < 0 || q.y < 0 || q.x >= size.x || q.y >= size.y) continue;
            float h = H.read(uint2(q)).r;
            if (!isnan(h) && (isnan(own) || abs(h - own) < 0.35)) { acc += h; n += 1.0; }
        }
        lift = n > 0.0 ? acc / n : 0.0;
        uv = float2(k) / float2(size);
    }
    float3 w = D.origin + D.uAxis * uv.x + D.vAxis * uv.y + float3(0, 0, lift);
    FieldOut o;
    o.pos = F.viewProj * float4(w, 1.0);
    o.uv = uv;
    return o;
}

static float fieldValue(texture2d<float, access::read> V, float2 uv, float cells) {
    uint2 size = uint2(V.get_width(), V.get_height());
    float2 p = uv * float2(size) - 0.5;
    if (cells > 0.5) {
        uint2 q = uint2(clamp(floor(p + 0.5), float2(0), float2(size) - 1.0));
        return V.read(q).r;
    }
    float2 f = fract(p);
    int2 b = int2(floor(p));
    float acc = 0.0, wsum = 0.0;
    for (int j = 0; j < 2; j++) for (int i = 0; i < 2; i++) {
        int2 q = clamp(b + int2(i, j), int2(0), int2(size) - 1);
        float v = V.read(uint2(q)).r;
        float w = (i == 0 ? 1.0 - f.x : f.x) * (j == 0 ? 1.0 - f.y : f.y);
        if (!isnan(v) && w > 0.0) { acc += v * w; wsum += w; }
    }
    // Outside the room (NaN) when the nearest texel is NaN.
    uint2 n = uint2(clamp(floor(p + 0.5), float2(0), float2(size) - 1.0));
    if (isnan(V.read(n).r) || wsum < 1e-6) return NAN;
    return acc / wsum;
}

fragment float4 field_fs(FieldOut in [[stage_in]], constant FieldDraw& D [[buffer(1)]],
                         texture2d<float, access::read> V [[texture(0)]], texture2d<float> lut [[texture(1)]],
                         sampler s [[sampler(0)]]) {
    float v = fieldValue(V, in.uv, D.cells);
    if (isnan(v)) discard_fragment();
    float t = clamp((v - D.lo) / max(D.hi - D.lo, 1e-6), 0.0, 1.0);
    float4 c = lut.sample(s, float2(t, 0.5));
    if (D.contour > 0.0) {
        float k = v / D.contour;
        float fw = fwidth(k);
        float line = 1.0 - smoothstep(0.0, fw * 1.0, abs(fract(k - 0.5) - 0.5));
        float lum = dot(c.rgb, float3(0.299, 0.587, 0.114));
        float3 ink = lum > 0.5 ? float3(0.0) : float3(1.0);
        c.rgb = mix(c.rgb, ink, line * 0.35);
    }
    c.a = D.opacity;
    return c;
}

fragment uint field_id_fs(FieldOut in [[stage_in]], constant FieldDraw& D [[buffer(1)]],
                          texture2d<float, access::read> V [[texture(0)]]) {
    float v = fieldValue(V, in.uv, 1.0);
    if (isnan(v)) discard_fragment();
    return D.id;
}
"""#
