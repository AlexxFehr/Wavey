"""Classroom.skp (Fratoni/Cingolani pilot round robin) -> OBJ in metres,
ear-clipped triangles grouped by layer (usemtl = layer name)."""
import sys, collections, numpy as np, openskp
from pathlib import Path
m = openskp.SkpFile(Path(sys.argv[1])).parse()
r = m.root
I = 0.0254
names = {28: "plaster", 29: "seats", 30: "wood", 31: "windows"}
V = {k: np.array([v.x, v.y, v.z]) * I for k, v in r.vertices.items()}
E = r.edges
use = collections.Counter()
def ring(face):
    out = []
    for eid, d in face.loops[0]:
        e = E[eid]
        a, b = (e.v1_id, e.v2_id) if d == 1 else (e.v2_id, e.v1_id)
        out.append(a)
        use[frozenset((e.v1_id, e.v2_id))] += 1
    return out
def earclip(idx, n):
    P = np.array([V[i] for i in idx])
    u = np.cross(n, [1, 0, 0] if abs(n[0]) < 0.9 else [0, 1, 0]); u /= np.linalg.norm(u)
    w = np.cross(n, u)
    p2 = [(p @ u, p @ w) for p in P]
    area = sum(p2[i][0]*p2[(i+1)%len(p2)][1]-p2[(i+1)%len(p2)][0]*p2[i][1] for i in range(len(p2)))
    order = list(range(len(idx)))
    if area < 0: order.reverse()
    tris = []
    def cross(o, a, b): return (a[0]-o[0])*(b[1]-o[1])-(a[1]-o[1])*(b[0]-o[0])
    guard = 0
    while len(order) > 3 and guard < 10000:
        guard += 1
        for k in range(len(order)):
            i0, i1, i2 = order[k-1], order[k], order[(k+1) % len(order)]
            a, b, c = p2[i0], p2[i1], p2[i2]
            if cross(a, b, c) <= 1e-12: continue
            if any(cross(a, b, p2[j]) >= 0 and cross(b, c, p2[j]) >= 0 and cross(c, a, p2[j]) >= 0 for j in order if j not in (i0, i1, i2)):
                continue
            tris.append((i0, i1, i2)); order.pop(k); break
        else:
            break
    if len(order) == 3: tris.append(tuple(order))
    # back to vertex ids, winding along n
    res = []
    for t in tris:
        a, b, c = (idx[j] for j in t)
        if np.cross(V[b]-V[a], V[c]-V[a]) @ n < 0: b, c = c, b
        res.append((a, b, c))
    return res
groups = collections.defaultdict(list)
area = collections.Counter()
for f in r.faces.values():
    if f.layer not in names: continue
    n = np.array(f.normal); n /= np.linalg.norm(n)
    idx = ring(f)
    for t in earclip(idx, n):
        groups[names[f.layer]].append(t)
        area[names[f.layer]] += 0.5*np.linalg.norm(np.cross(V[t[1]]-V[t[0]], V[t[2]]-V[t[0]]))
ids = sorted({i for g in groups.values() for t in g for i in t})
num = {v: k+1 for k, v in enumerate(ids)}
with open(sys.argv[2], "w") as o:
    o.write("# Classroom (pilot round robin, Fratoni et al.), metres, from Classroom.skp\n")
    for v in ids: o.write("v %.5f %.5f %.5f\n" % tuple(V[v]))
    for g, ts in groups.items():
        o.write(f"g {g}\nusemtl {g}\n")
        for t in ts: o.write("f %d %d %d\n" % tuple(num[i] for i in t))
open_edges = sum(1 for k, c in use.items() if c == 1)
print("areas m2", {k: round(v, 1) for k, v in area.items()}, "total", round(sum(area.values()), 1))
print("tris", sum(len(g) for g in groups.values()), "edges used once", open_edges, "more than twice", sum(1 for c in use.values() if c > 2))

