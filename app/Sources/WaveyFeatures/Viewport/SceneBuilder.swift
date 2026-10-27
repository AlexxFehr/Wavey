import Foundation
import simd
import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel
import WaveyViewport

/// Direct-manipulation handles drawn in the viewport.
enum Handle: Hashable {
    /// Move gizmo axis (0 x, 1 y, 2 z).
    case axis(Int)
    /// Move gizmo horizontal-plane handle.
    case planeXY
    /// Rotate tool ring (turns about the vertical axis).
    case rotateZ
    /// Cut-plane normal arrow.
    case planeOffset(UInt64)
}

/// Stable pick ids for entities within one scene build.
@MainActor
final class PickTable {
    private(set) var byID: [UInt32: EntityRef] = [:]
    private var byRef: [EntityRef: UInt32] = [:]
    private var handles: [UInt32: Handle] = [:]
    private var handleIDs: [Handle: UInt32] = [:]

    func reset() {
        byID = [:]
        byRef = [:]
        handles = [:]
        handleIDs = [:]
    }

    func id(_ h: Handle) -> UInt32 {
        if let v = handleIDs[h] { return v }
        let v = 0x8000_0000 | UInt32(handles.count + 1)
        handles[v] = h
        handleIDs[h] = v
        return v
    }

    func handle(_ id: UInt32?) -> Handle? { id.flatMap { handles[$0] } }

    func id(_ e: EntityRef) -> UInt32 {
        if let v = byRef[e] { return v }
        let v = UInt32(byID.count + 1)
        byID[v] = e
        byRef[e] = v
        return v
    }

    func entity(_ id: UInt32?) -> EntityRef? { id.flatMap { byID[$0] } }
}

/// Derived geometry reused across scene builds: surface outlines and bounds.
@MainActor
final class GeometryCache {
    private var revision = -1
    private(set) var outlines: [String: [Float]] = [:]
    private(set) var centres: [String: SIMD3<Float>] = [:]

    func sync(_ store: DocumentStore) {
        guard store.geometryRevision != revision else { return }
        revision = store.geometryRevision
        outlines = [:]
        centres = [:]
        for s in store.room?.surfaces ?? [] {
            outlines[s.key] = outlineEdges(s.positions)
            let n = max(s.positions.count / 3, 1)
            var c = SIMD3<Float>(0, 0, 0)
            for i in 0..<n { c += SIMD3(s.positions[i * 3], s.positions[i * 3 + 1], s.positions[i * 3 + 2]) }
            centres[s.key] = c / Float(n)
        }
    }

}

/// Edges used by exactly one triangle (the polygon outline), as segment pairs.
func outlineEdges(_ p: [Float]) -> [Float] {
        struct K: Hashable { let a: SIMD3<Int32>, b: SIMD3<Int32> }
        func q(_ i: Int) -> SIMD3<Int32> { SIMD3(Int32((p[i * 3] * 1e4).rounded()), Int32((p[i * 3 + 1] * 1e4).rounded()), Int32((p[i * 3 + 2] * 1e4).rounded())) }
        var count: [K: (Int, Int, Int)] = [:]
        for t in 0..<(p.count / 9) {
            for k in 0..<3 {
                let i = t * 3 + k, j = t * 3 + (k + 1) % 3
                let (a, b) = (q(i), q(j))
                let key = (a.x, a.y, a.z) < (b.x, b.y, b.z) ? K(a: a, b: b) : K(a: b, b: a)
                let c = count[key]
                count[key] = (c?.0 ?? 0) + 1 == 1 ? (1, i, j) : (2, i, j)
            }
        }
        // Triangles and the surface normal, to drop edges with surface on
        // both sides: the bridges a polygon with holes (a ceiling around
        // pillars) is cut along, when the triangles either side split them
        // differently.
        let v = (0..<(p.count / 3)).map { SIMD3<Float>(p[$0 * 3], p[$0 * 3 + 1], p[$0 * 3 + 2]) }
        var n = SIMD3<Float>(0, 0, 0)
        for t in 0..<(v.count / 3) { n += simd_cross(v[t * 3 + 1] - v[t * 3], v[t * 3 + 2] - v[t * 3]) }
        let unit = simd_length(n) > 0 ? simd_normalize(n) : SIMD3<Float>(0, 0, 1)
        func covered(_ x: SIMD3<Float>) -> Bool {
            for t in 0..<(v.count / 3) {
                let a = v[t * 3], b = v[t * 3 + 1], c = v[t * 3 + 2]
                let d1 = simd_dot(simd_cross(b - a, x - a), unit), d2 = simd_dot(simd_cross(c - b, x - b), unit), d3 = simd_dot(simd_cross(a - c, x - c), unit)
                if (d1 >= 0 && d2 >= 0 && d3 >= 0) || (d1 <= 0 && d2 <= 0 && d3 <= 0) {
                    if simd_length(simd_cross(b - a, c - a)) > 1e-9 { return true }
                }
            }
            return false
        }
        // Each once-used edge is split at the surface's vertices on it (a
        // long edge can be outline in part and a bridge in part), and
        // each piece is kept only where the surface is on one side.
        let verts = Array(Set(v.map { SIMD3<Int32>(Int32(($0.x * 1e4).rounded()), Int32(($0.y * 1e4).rounded()), Int32(($0.z * 1e4).rounded())) }))
            .map { SIMD3<Float>(Float($0.x), Float($0.y), Float($0.z)) / 1e4 }
        var out: [Float] = []
        for (_, e) in count where e.0 == 1 {
            let a = v[e.1], b = v[e.2]
            let d = b - a
            let len = simd_length(d)
            guard len > 1e-6 else { continue }
            let dir = d / len
            var cuts: [Float] = [0, 1]
            for q in verts {
                let t = simd_dot(q - a, dir)
                if t > 1e-4, t < len - 1e-4, simd_length(q - (a + dir * t)) < 1e-4 { cuts.append(t / len) }
            }
            cuts.sort()
            let side = simd_normalize(simd_cross(unit, d)) * 0.002
            for k in 0..<(cuts.count - 1) where cuts[k + 1] - cuts[k] > 1e-6 {
                let p0 = a + d * cuts[k], p1 = a + d * cuts[k + 1]
                let m = (p0 + p1) / 2
                if covered(m + side) && covered(m - side) { continue }
                out += [p0.x, p0.y, p0.z, p1.x, p1.y, p1.z]
            }
        }
        return out
    }

/// Builds the immutable viewport scenes from the model, selection, hover,
/// results view state and the active tool's overlays.
@MainActor
struct SceneBuilder {
    let ctx: WindowContext
    var store: DocumentStore { ctx.store }

    init(context: WindowContext) {
        ctx = context
        ctx.geometryCache.sync(context.store)
    }

    // MARK: Colours

    static let outline = rgba(0xC9D1D9)
    static let accent = rgba(0x4DA3FF)
    static let accentText = rgba(0x6DB3FF)
    static let source = rgba(0xF29D38)
    static let sourceFill = rgba(0x2A1E10)
    static let receiver = rgba(0xE4E7EA)
    static let tertiary = rgba(0x8A949E)
    static let secondary = rgba(0xD4D9DF)
    static let ghostEdge = rgba(0x3E4650)
    static let floorTint = rgba(0x18222C)
    static let ghostWall = rgba(0x1F2B37)
    static let planBG = rgba(0x121417)

    func build(for ws: Workspace, inset: Bool = false) -> SceneSnapshot {
        var s = SceneSnapshot()
        s.revision = Int.random(in: 0..<Int.max)
        if let r = store.room {
            s.boundsMin = Vec3(r.boundsMin.x, r.boundsMin.y, r.boundsMin.z)
            s.boundsMax = Vec3(r.boundsMax.x, r.boundsMax.y, r.boundsMax.z)
        } else if let b = sketchBounds() {
            s.boundsMin = Vec3(b.0.x, b.0.y, 0)
            s.boundsMax = Vec3(b.1.x, b.1.y, store.levels.first { $0.kind == .ceiling }?.z ?? 3)
        }
        let plan = !inset && ctx.viewports[ws]?.mode == .plan
        s.gridZ = plan ? planZ : 0
        s.gridStep = ws == .geometry ? ctx.gridStep : 0
        s.showGrid = !inset
        switch (ws, plan) {
        case (.geometry, true): buildPlan(&s)
        case (.geometry, false): buildGeometry3D(&s, inset: inset)
        case (.sources, _): buildSources(&s, plan: plan)
        case (.results, _): buildResults(&s, plan: plan)
        }
        // Handles: Move shows the axis arrows, Rotate the ring (Select shows
        // none in Setup and Results; Geometry keeps the arrows on Select).
        if !inset, ws != .geometry || !plan {
            if ctx.activeTool == .move || (ws == .geometry && ctx.activeTool == .select) { addGizmo(&s, ws: ws) }
            if ctx.activeTool == .rotate { addRotateRing(&s, ws: ws) }
        }
        if !inset, let g = ctx.dropGhost, let d = ctx.dragDevice {
            addDropGhost(&s, at: g, device: d)
        }
        if !inset {
            s.lines += ctx.tool.overlayLines(for: ws)
            s.markers += ctx.tool.overlayMarkers(for: ws)
            s.labels += ctx.tool.overlayLabels(for: ws)
        }
        return s
    }

    var planZ: Double { store.levels.first { $0.id == ctx.planLevel }?.z ?? 0 }

    /// Where a dragged library speaker would land: its box at the drop
    /// height (subwoofers on the floor) and a line down to the floor point.
    private func addDropGhost(_ s: inout SceneSnapshot, at p: SIMD3<Double>, device d: DragDevice) {
        let zc = d.sub ? d.height / 2 : 1.35
        let (hx, hy, hz) = (Float(d.depth / 2), Float(d.width / 2), Float(d.height / 2))
        let c = SIMD3<Float>(Float(p.x), Float(p.y), Float(zc))
        let corners = (0..<8).map { i -> SIMD3<Float> in
            c + SIMD3(i & 1 == 0 ? -hx : hx, i & 2 == 0 ? -hy : hy, i & 4 == 0 ? -hz : hz)
        }
        var seg: [Float] = []
        for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
            seg += [corners[a].x, corners[a].y, corners[a].z, corners[b].x, corners[b].y, corners[b].z]
        }
        // Front face (+x) marked by a cross.
        seg += [corners[1].x, corners[1].y, corners[1].z, corners[7].x, corners[7].y, corners[7].z]
        seg += [corners[3].x, corners[3].y, corners[3].z, corners[5].x, corners[5].y, corners[5].z]
        s.lines.append(LineBatch(segments: seg, color: Self.accent, width: 2, overlay: true))
        s.lines.append(LineBatch(segments: [c.x, c.y, c.z - hz, c.x, c.y, 0], color: Self.accent, width: 1, dashed: true, overlay: true))
        s.labels.append(contentsOf: [])
    }

    private func style(_ e: EntityRef) -> DrawStyle {
        if ctx.selection.contains(e) || inGroup(e, ctx.selection.ids) || ofFeature(e, ctx.selection.ids) { return .selected }
        if ctx.selection.hover == e || ctx.selection.hover.map({ inGroup(e, [$0]) || ofFeature(e, [$0]) }) == true { return .hovered }
        return .normal
    }

    /// A speaker or microphone that belongs to one of these groups (a
    /// selected or hovered group lights up all its members).
    private func inGroup(_ e: EntityRef, _ refs: [EntityRef]) -> Bool {
        let groups = refs.compactMap(\.groupID)
        guard !groups.isEmpty else { return false }
        return groups.contains { g in
            guard let r = store.groupsById[g] else { return false }
            switch e {
            case .speaker(let id): return r.members.contains(id)
            case .microphone(let id): return r.receivers.contains(id)
            default: return false
            }
        }
    }

    /// A surface made by one of these features (a selected feature lights up its faces).
    private func ofFeature(_ e: EntityRef, _ refs: [EntityRef]) -> Bool {
        guard case .surface(let k) = e, k.hasPrefix("feat:") else { return false }
        return refs.contains { r in r.featureID.map { k.hasPrefix("feat:\($0):") } ?? false }
    }

    /// Feature footprints on the plan: dashed, pickable.
    private func addFeatureFootprints(_ s: inout SceneSnapshot, z: Float) {
        for f in store.features {
            let seg = ctx.featureFootprint(f, z: z)
            guard !seg.isEmpty else { continue }
            let st = style(.feature(f.id))
            let c: SIMD4<Float> = st == .selected ? Self.accent : st == .hovered ? mix(rgba(0xC9D1D9, 0.6), Self.accent, 0.5) : rgba(0xC9D1D9, f.suppressed ? 0.2 : 0.55)
            s.lines.append(LineBatch(id: ctx.picks.id(.feature(f.id)), segments: seg, color: c, width: st == .selected ? 2 : 1.2, dashed: true, overlay: st != .normal))
            if let x = seg.first, let y = seg.dropFirst().first {
                s.labels.append(Label(id: "feat-\(f.id)", anchor: SIMD3(x, y, z), text: f.name, color: st == .selected ? Self.accentText : Self.tertiary,
                                      priority: st == .selected ? 9 : 1, offset: CGSize(width: 0, height: -10)))
            }
        }
    }

    private func sketchBounds() -> (SIMD2<Double>, SIMD2<Double>)? {
        let v = store.sketch.vertices
        guard !v.isEmpty else { return nil }
        let xs = v.map(\.p.x), ys = v.map(\.p.y)
        return (SIMD2(xs.min()!, ys.min()!), SIMD2(xs.max()!, ys.max()!))
    }

    private func layerVisible(_ id: String) -> Bool { store.layersById[id]?.visible ?? true }

    // MARK: Geometry: plan

    /// Imported underlay linework (greyed; flattened onto the plan level in
    /// plan) and the gaps of an imported room model (red, on top).
    private func addImportLines(_ s: inout SceneSnapshot, flattenTo z: Float?) {
        var u: [Float] = []
        for ul in store.underlays where ul.visible { u += ul.positions }
        if let z { for i in stride(from: 2, to: u.count, by: 3) { u[i] = z } }
        if !u.isEmpty { s.lines.append(LineBatch(segments: u, color: rgba(0x7C8794, 0.55), width: 1)) }
        if !ctx.openEdges.isEmpty, store.roomModel != nil {
            s.lines.append(LineBatch(segments: ctx.openEdges, color: rgba(0xFF6B6B, 0.95), width: 3, overlay: true))
        }
    }

    private func buildPlan(_ s: inout SceneSnapshot) {
        let z = Float(planZ)
        addImportLines(&s, flattenTo: z - 0.0005)
        addFeatureFootprints(&s, z: z + 0.001)
        // Simulated floor regions as a flat tint.
        for surf in store.room?.surfaces ?? [] where surf.kind == .floor {
            var p = surf.positions
            for i in stride(from: 2, to: p.count, by: 3) { p[i] = z - 0.001 }
            s.surfaces.append(SurfaceDrawable(id: ctx.picks.id(.surface(surf.key)), positions: p, color: Self.floorTint,
                                              style: style(.surface(surf.key)), normal: SIMD3(0, 0, 1)))
        }
        // Non-simulated closed regions: faint fill.
        var normal: [Float] = [], selected: [Float] = [], hovered: [Float] = [], hidden = Set<UInt64>()
        for seg in store.sketch.segments {
            guard layerVisible(seg.layerId) else { hidden.insert(seg.id); continue }
            let a = seg.pa, b = seg.pb
            let pts: [Float] = [Float(a.x), Float(a.y), z, Float(b.x), Float(b.y), z]
            let e = EntityRef.segment(seg.id)
            let walls = [EntityRef.surface("wall:\(seg.id):f"), EntityRef.surface("wall:\(seg.id):r")]
            if ctx.selection.contains(e) || walls.contains(where: ctx.selection.contains) {
                selected += pts
            } else if ctx.selection.hover == e || walls.contains(where: { ctx.selection.hover == $0 }) {
                hovered += pts
            } else {
                normal += pts
            }
            // Wall codes along the segment.
            if let code = store.surfacesByKey["wall:\(seg.id):f"]?.code ?? store.surfacesByKey["wall:\(seg.id):r"]?.code {
                let mid = SIMD3<Float>(Float((a.x + b.x) / 2), Float((a.y + b.y) / 2), z)
                let sel = selected.count > 0 && (ctx.selection.contains(e) || walls.contains(where: ctx.selection.contains))
                s.labels.append(Label(id: "code-\(seg.id)", anchor: mid, text: code, color: sel ? Self.accentText : Self.tertiary,
                                      priority: sel ? 10 : 1, plate: true))
            }
        }
        let segPick = store.sketch.segments.filter { !hidden.contains($0.id) }
        // Invisible pick quads are not needed: segments are picked on the CPU (8 pt).
        _ = segPick
        if !normal.isEmpty { s.lines.append(LineBatch(segments: normal, color: Self.outline, width: 2)) }
        if !hovered.isEmpty { s.lines.append(LineBatch(segments: hovered, color: Self.accent, width: 1.5, overlay: true)) }
        if !selected.isEmpty {
            s.lines.append(LineBatch(segments: selected, color: rgba(0x4DA3FF, 0.18), width: 10, overlay: true))
            s.lines.append(LineBatch(segments: selected, color: Self.accent, width: 3, overlay: true))
            // Vertex handles of selected segments.
            for seg in store.sketch.segments where ctx.selection.contains(.segment(seg.id))
                || ctx.selection.contains(.surface("wall:\(seg.id):f")) || ctx.selection.contains(.surface("wall:\(seg.id):r")) {
                for p in [seg.pa, seg.pb] {
                    s.markers.append(Marker(position: SIMD3(Float(p.x), Float(p.y), z), shape: .hollowSquare, size: 8,
                                            color: Self.accent, fill: Self.planBG))
                }
                let m = SIMD3<Float>(Float((seg.pa.x + seg.pb.x) / 2), Float((seg.pa.y + seg.pb.y) / 2), z)
                s.markers.append(Marker(position: m, shape: .square, size: 6, color: Self.accent, fill: Self.accent))
            }
        }
        // Sources dimmed in plan.
        for spk in store.speakers {
            let p = SIMD3<Float>(Float(spk.position.x), Float(spk.position.y), z)
            s.markers.append(Marker(id: ctx.picks.id(.speaker(spk.id)), position: p, shape: .square, size: 7,
                                    color: rgba(0xF29D38, 0.5), fill: rgba(0x2A1E10, 0.5)))
        }
        if ctx.flag("dims"), let (lo, hi) = sketchBounds() {
            dimension(&s, from: SIMD3(Float(lo.x), Float(hi.y), z), to: SIMD3(Float(hi.x), Float(hi.y), z), offset: SIMD3(0, 1, 0), id: "w")
            dimension(&s, from: SIMD3(Float(lo.x), Float(lo.y), z), to: SIMD3(Float(hi.x), Float(lo.y), z), offset: SIMD3(0, -1, 0), id: "w2")
            dimension(&s, from: SIMD3(Float(lo.x), Float(lo.y), z), to: SIMD3(Float(lo.x), Float(hi.y), z), offset: SIMD3(-1, 0, 0), id: "h")
        }
    }

    /// Aligned dimension with extension lines, ticks and a value plate.
    private func dimension(_ s: inout SceneSnapshot, from a: SIMD3<Float>, to b: SIMD3<Float>, offset dir: SIMD3<Float>, id: String) {
        let mpp = Float(ctx.viewports[.geometry]?.projection.metresPerPoint ?? 0.02)
        let off = dir * mpp * 30
        let (pa, pb) = (a + off, b + off)
        var l: [Float] = []
        l += [a.x + dir.x * mpp * 6, a.y + dir.y * mpp * 6, a.z, pa.x + dir.x * mpp * 6, pa.y + dir.y * mpp * 6, pa.z]
        l += [b.x + dir.x * mpp * 6, b.y + dir.y * mpp * 6, b.z, pb.x + dir.x * mpp * 6, pb.y + dir.y * mpp * 6, pb.z]
        l += [pa.x, pa.y, pa.z, pb.x, pb.y, pb.z]
        let along = simd_normalize(pb - pa)
        let tick = (along + dir) * mpp * 5
        for p in [pa, pb] { l += [p.x - tick.x, p.y - tick.y, p.z, p.x + tick.x, p.y + tick.y, p.z] }
        s.lines.append(LineBatch(segments: l, color: Self.tertiary, width: 1.2, overlay: true))
        let len = simd_length(b - a)
        s.labels.append(Label(id: "dim-\(id)", anchor: (pa + pb) / 2, text: String(format: "%.3f", len), color: Self.secondary, priority: 3))
    }

    // MARK: Geometry: 3D

    /// 1: the camera is above the ceiling (cut it away), -1: below the
    /// floor (cut that away), 0: neither (or not a 3D view).
    static func cutawaySide(eyeZ: Double?, room: RoomSnapshot?) -> Int {
        guard let z = eyeZ, let r = room else { return 0 }
        return z > r.boundsMax.z ? 1 : z < r.boundsMin.z ? -1 : 0
    }

    private func buildGeometry3D(_ s: inout SceneSnapshot, inset: Bool) {
        var edges: [Float] = [], selEdges: [Float] = []
        let vp = ctx.viewports[.geometry]
        let side = inset ? 0 : Self.cutawaySide(eyeZ: vp.map { $0.mode.is3D ? $0.projection.eye.z : nil } ?? nil, room: store.room)
        ctx.cutawaySide = side
        for surf in store.room?.surfaces ?? [] {
            guard layerVisible(surf.layerId) else { continue }
            let e = EntityRef.surface(surf.key)
            let st = style(e)
            // Seen from above, the ceiling would cover (and catch the clicks
            // for) every wall: draw just its outline unless it is selected.
            if st == .normal, (side == 1 && surf.kind == .ceiling) || (side == -1 && surf.kind == .floor) {
                edges += ctx.geometryCache.outlines[surf.key] ?? []
                continue
            }
            var c = rgba(hex: store.materialsById[surf.materialId]?.color ?? "#9aa4ae")
            if inset {
                c = surf.kind == .floor ? rgba(0x18222C, 0.85) : rgba(0x1F2B37, 0.35)
            } else {
                c.w = surf.kind == .wall ? 0.55 : 0.75
            }
            if st == .selected { c = rgba(0x4DA3FF, 0.35) } else if st == .hovered { c = mix(c, Self.accent, 0.15) }
            s.surfaces.append(SurfaceDrawable(id: ctx.picks.id(e), positions: surf.positions, color: c, style: inset ? .ghost : st,
                                              normal: SIMD3(Float(surf.normal.x), Float(surf.normal.y), Float(surf.normal.z))))
            if st == .selected { selEdges += ctx.geometryCache.outlines[surf.key] ?? [] } else { edges += ctx.geometryCache.outlines[surf.key] ?? [] }
        }
        s.lines.append(LineBatch(segments: edges, color: inset ? Self.ghostEdge : rgba(0xC9D1D9, 0.55), width: 1))
        if !selEdges.isEmpty {
            s.lines.append(LineBatch(segments: selEdges, color: Self.accent, width: inset ? 2 : 3, overlay: inset))
        }
        if !inset {
            addImportLines(&s, flattenTo: nil)
            addSources(&s, dimmed: true, labels: false)
        }
    }

    private func mix(_ a: SIMD4<Float>, _ b: SIMD4<Float>, _ t: Float) -> SIMD4<Float> { a + (b - a) * t }

    // MARK: Sources & receivers

    /// Ghosted room: floor tint, faint walls, edges.
    private func ghostRoom(_ s: inout SceneSnapshot, pickable: Bool, wire: Bool = false) {
        var edges: [Float] = []
        for surf in store.room?.surfaces ?? [] {
            guard layerVisible(surf.layerId) else { continue }
            let id = pickable ? ctx.picks.id(.surface(surf.key)) : 0
            if !wire || surf.kind == .floor {
                let c = surf.kind == .floor ? rgba(0x18222C, wire ? 0.5 : 0.85) : rgba(0x1F2B37, 0.18)
                s.surfaces.append(SurfaceDrawable(id: id, positions: surf.positions, color: c, style: .ghost,
                                                  normal: SIMD3(Float(surf.normal.x), Float(surf.normal.y), Float(surf.normal.z))))
            }
            edges += ctx.geometryCache.outlines[surf.key] ?? []
        }
        s.lines.append(LineBatch(segments: edges, color: Self.ghostEdge, width: 1))
    }

    private func buildSources(_ s: inout SceneSnapshot, plan: Bool) {
        // Surfaces are pickable only while a tool places something on them.
        ghostRoom(&s, pickable: ctx.activeTool == .cutPlane)
        if ctx.flag("show.surfaces") {
            for surf in store.room?.surfaces ?? [] where surf.kind != .floor {
                let a = Float(store.materialsById[surf.materialId].map { absorptionAt500($0) } ?? 0)
                let c = SIMD4<Float>(0.85, 0.64, 0.36, 0.08 + 0.5 * a)
                s.surfaces.append(SurfaceDrawable(id: 0, positions: surf.positions, color: c, style: .ghost, normal: SIMD3(0, 0, 1)))
            }
        }
        addSources(&s, dimmed: false, labels: true)
        if ctx.flag("show.receivers") { addReceivers(&s, results: false) }
    }

    private func absorptionAt500(_ m: MaterialRec) -> Double {
        let a = m.absorption
        if a.freqsHz.isEmpty { return a.values.first ?? 0 }
        let i = a.freqsHz.enumerated().min { abs($0.element - 500) < abs($1.element - 500) }?.offset ?? 0
        return a.values[i]
    }

    static func rotation(_ spk: Speaker) -> simd_float3x3 {
        let yaw = Float(spk.yawDeg * .pi / 180), pitch = Float(spk.pitchDeg * .pi / 180), roll = Float(spk.rollDeg * .pi / 180)
        let q = simd_quatf(angle: yaw, axis: [0, 0, 1]) * simd_quatf(angle: -pitch, axis: [0, 1, 0]) * simd_quatf(angle: roll, axis: [1, 0, 0])
        return simd_float3x3(q)
    }

    static func acousticCentre(_ spk: Speaker, mesh: CabinetMeshRec?) -> SIMD3<Float> {
        let p = SIMD3<Float>(Float(spk.position.x), Float(spk.position.y), Float(spk.position.z))
        guard let ac = mesh?.acousticCenter else { return p }
        return p + rotation(spk) * SIMD3(Float(ac.x), Float(ac.y), Float(ac.z))
    }

    private func addSources(_ s: inout SceneSnapshot, dimmed: Bool, labels: Bool) {
        var drop: [Float] = [], aim: [Float] = []
        for spk in store.speakers {
            let e = EntityRef.speaker(spk.id)
            let st = style(e)
            let mesh = store.cabinetMeshes[spk.modelId]
            let r = Self.rotation(spk)
            let p = SIMD3<Float>(Float(spk.position.x), Float(spk.position.y), Float(spk.position.z))
            let alpha: Float = spk.muted ? 0.4 : dimmed ? 0.5 : 1
            let id = ctx.picks.id(e)
            if let mesh {
                func place(_ v: [Float], scale: Float = 1) -> [Float] {
                    var out: [Float] = []
                    out.reserveCapacity(v.count)
                    for k in 0..<(v.count / 3) {
                        let w = r * (SIMD3(v[k * 3], v[k * 3 + 1], v[k * 3 + 2]) * scale) + p
                        out += [w.x, w.y, w.z]
                    }
                    return out
                }
                var hull: [Float] = []
                hull.reserveCapacity(mesh.indices.count * 3)
                for i in mesh.indices {
                    hull += [mesh.positions[Int(i) * 3], mesh.positions[Int(i) * 3 + 1], mesh.positions[Int(i) * 3 + 2]]
                }
                // A model with a wireframe is drawn as the clean solids
                // recovered from it (satin charcoal, drivers and ports
                // darker) with their edges; otherwise as its hull.
                let solid = !mesh.surfacePositions.isEmpty
                if solid {
                    s.meshes.append(MeshDrawable(id: id, positions: place(mesh.surfacePositions), color: rgba(0x34373D, alpha), lit: true))
                    if !mesh.insetPositions.isEmpty {
                        s.meshes.append(MeshDrawable(id: id, positions: place(mesh.insetPositions), color: rgba(0x17181B, alpha), lit: true))
                    }
                } else {
                    s.meshes.append(MeshDrawable(id: id, positions: place(hull), color: rgba(0x2C2E33, alpha), lit: true))
                }
                let edges = place(solid ? mesh.detailPositions : mesh.edgePositions)
                if st == .selected {
                    s.lines.append(LineBatch(segments: edges, color: rgba(0xF29D38, solid ? 0.18 : 0.25), width: solid ? 4 : 6))
                }
                // Orange marks a source when selected or hovered; at rest the
                // edges are quiet so the cabinet reads as an object.
                let rest = solid ? rgba(0x9AA0A8, alpha * 0.5) : rgba(0xF29D38, alpha * 0.8)
                s.lines.append(LineBatch(segments: edges, color: st == .normal ? rest : rgba(0xF29D38, alpha),
                                         width: st == .selected ? (solid ? 1.8 : 2.5) : st == .hovered ? 1.6 : 1))
            } else {
                s.markers.append(Marker(id: id, position: p, shape: .square, size: 10, color: rgba(0xF29D38, alpha), fill: rgba(0x2A1E10, alpha)))
            }
            let ac = Self.acousticCentre(spk, mesh: mesh)
            s.markers.append(Marker(id: id, position: ac, shape: .dot, size: 4, color: rgba(0xF29D38, alpha)))
            if spk.solo { s.markers.append(Marker(position: ac, shape: .ring, size: 22, color: rgba(0xF29D38, 0.5))) }
            if dimmed { continue }
            // Drop line and floor ellipse.
            var lowest: Float = 0
            if let m = mesh {
                for k in 0..<(m.positions.count / 3) {
                    let v: SIMD3<Float> = r * SIMD3<Float>(m.positions[k * 3], m.positions[k * 3 + 1], m.positions[k * 3 + 2])
                    lowest = min(lowest, v.z)
                }
            }
            if p.z + lowest > 0.02 {
                drop += [p.x, p.y, p.z + lowest, p.x, p.y, 0]
            }
            s.markers.append(Marker(position: SIMD3(p.x, p.y, 0.001), shape: .ring, size: 7, color: rgba(0xF29D38, 0.45)))
            // Aim line or axis.
            if ctx.flag("show.aim") {
                var target: SIMD3<Float>?
                switch spk.aim {
                case .microphone(let mid): if let m = store.micsById[mid] { target = SIMD3(Float(m.position.x), Float(m.position.y), Float(m.position.z)) }
                case .point(let q): target = SIMD3(Float(q.x), Float(q.y), Float(q.z))
                case nil: break
                }
                if let t = target {
                    aim += [ac.x, ac.y, ac.z, t.x, t.y, t.z]
                    let d = simd_length(t - ac)
                    s.labels.append(Label(id: "aim-\(spk.id)", anchor: (ac + t) / 2, text: String(format: "%.3f m", d),
                                          color: Self.source, priority: st == .selected ? 8 : 2))
                } else {
                    let axis = r * SIMD3<Float>(1, 0, 0)
                    let t = ac + axis * 1.2
                    aim += [ac.x, ac.y, ac.z, t.x, t.y, t.z]
                }
            }
            if labels, !ctx.flag("hide.labels") || st == .selected {
                s.labels.append(Label(id: "spk-\(spk.id)", anchor: p + SIMD3(0, 0, 0), text: spk.code, secondary: spk.name,
                                      color: st == .selected ? Self.accentText : Self.source,
                                      priority: st == .selected ? 10 : st == .hovered ? 9 : 4, offset: CGSize(width: 0, height: -22)))
            }
            // Directivity balloon of the selected source.
            if ctx.flag("show.directivity"), st == .selected, ctx.selection.speakers.count <= 4,
               let balloon = ctx.balloon(model: spk.modelId, frequency: ctx.balloonFrequency(model: spk.modelId)) {
                let scale: Float = 1.2
                var tris: [Float] = []
                tris.reserveCapacity(balloon.indices.count * 3)
                for i in balloon.indices {
                    let v = r * (SIMD3(balloon.positions[Int(i) * 3], balloon.positions[Int(i) * 3 + 1], balloon.positions[Int(i) * 3 + 2]) * scale) + ac
                    tris += [v.x, v.y, v.z]
                }
                s.meshes.append(MeshDrawable(id: 0, positions: tris, color: rgba(0xF29D38, 0.10), lit: false, depthWrite: false))
                // −6 dB contour (horizontal plane), dashed.
                if let polar = ctx.polar(model: spk.modelId, frequency: ctx.balloonFrequency(model: spk.modelId)) {
                    var ring: [Float] = []
                    let n = polar.anglesDeg.count
                    func pt(_ k: Int) -> SIMD3<Float> {
                        let a = Float(polar.anglesDeg[k] * .pi / 180)
                        let rad = scale * Float(max(0, 1 - max(polar.horizontalDb[k], -30) / -30))
                        return r * SIMD3(cos(a) * rad, sin(a) * rad, 0) + ac
                    }
                    for k in 0..<n {
                        let a = pt(k), b = pt((k + 1) % n)
                        ring += [a.x, a.y, a.z, b.x, b.y, b.z]
                    }
                    s.lines.append(LineBatch(segments: ring, color: rgba(0xF29D38, 0.55), width: 1, dashed: false))
                    // −6 dB circle in the balloon's radial scale.
                    var six: [Float] = []
                    let rad6 = scale * (1 - 6.0 / 30.0)
                    for k in 0..<48 {
                        let a0 = Float(k) / 48 * 2 * .pi, a1 = Float(k + 1) / 48 * 2 * .pi
                        let p0 = r * SIMD3(cos(a0) * rad6, sin(a0) * rad6, 0) + ac
                        let p1 = r * SIMD3(cos(a1) * rad6, sin(a1) * rad6, 0) + ac
                        six += [p0.x, p0.y, p0.z, p1.x, p1.y, p1.z]
                    }
                    s.lines.append(LineBatch(segments: six, color: rgba(0xF29D38, 0.7), width: 1, dashed: true))
                }
            }
        }
        if !drop.isEmpty { s.lines.append(LineBatch(segments: drop, color: rgba(0xF29D38, 0.45), width: 1, dashed: true)) }
        if !aim.isEmpty { s.lines.append(LineBatch(segments: aim, color: Self.source, width: 1, dashed: true)) }
    }

    private func addReceivers(_ s: inout SceneSnapshot, results: Bool) {
        var drop: [Float] = []
        for (i, m) in store.microphones.enumerated() where m.visible {
            let e = EntityRef.microphone(m.id)
            let st = style(e)
            let p = SIMD3<Float>(Float(m.position.x), Float(m.position.y), Float(m.position.z))
            let c = results ? micColor(m, index: i) : Self.receiver
            let id = ctx.picks.id(e)
            if results {
                // Stem up from the position, ring and anchor dot.
                let top = p + SIMD3(0, 0, 0.55)
                s.lines.append(LineBatch(id: id, segments: [p.x, p.y, p.z, top.x, top.y, top.z], color: c, width: 1.5))
                s.markers.append(Marker(id: id, position: top, shape: .ring, size: st == .selected ? 12 : 10, color: c, fill: rgba(0x15171A)))
                s.markers.append(Marker(id: id, position: p, shape: .dot, size: 4, color: c))
                var value: String?
                if let db = ctx.micLevel(m.id) { value = String(format: "%.1f", db) }
                s.labels.append(Label(id: "mic-\(m.id)", anchor: top, text: m.code, secondary: value, color: c,
                                      priority: st == .selected ? 10 : 6, offset: CGSize(width: 0, height: -16)))
            } else {
                s.markers.append(Marker(id: id, position: p, shape: .ring, size: st == .selected ? 9 : 6,
                                        color: st == .selected ? Self.accent : st == .hovered ? Self.accent : Self.receiver,
                                        fill: rgba(0x15171A, 0.8)))
                drop += [p.x, p.y, p.z, p.x, p.y, 0]
                if !ctx.flag("hide.labels") || st == .selected { s.labels.append(Label(id: "mic-\(m.id)", anchor: p, text: "\(m.code) · \(m.name)",
                                      color: st == .selected ? Self.accentText : Self.secondary, priority: st == .selected ? 10 : 3,
                                      offset: CGSize(width: 0, height: 14))) }
            }
        }
        if !drop.isEmpty { s.lines.append(LineBatch(segments: drop, color: rgba(0xE4E7EA, 0.35), width: 1, dashed: true)) }
        if !results {
            // Receiver grids (solved cut planes): outline and points.
            guard let r = store.room else { return }
            for plane in store.cutPlanes {
                let e = EntityRef.plane(plane.id)
                let st = style(e)
                if plane.drapeHeight != nil {
                    // Audience area: the floors' outlines at ear height and its points.
                    let c = st == .selected ? Self.accent : rgba(0xE4E7EA, 0.5)
                    s.lines.append(LineBatch(id: ctx.picks.id(e), segments: drapeOutline(plane), color: c, width: st == .selected ? 2 : 1, dashed: true))
                    let pts = ctx.audiencePoints(plane)
                    let n = pts.count / 3
                    if n <= 5000 {
                        for k in 0..<n {
                            s.markers.append(Marker(id: ctx.picks.id(e), position: SIMD3(pts[k * 3], pts[k * 3 + 1], pts[k * 3 + 2]),
                                                    shape: .ring, size: 3, color: rgba(0xE4E7EA, st == .selected ? 0.9 : 0.45)))
                        }
                    }
                    continue
                }
                let rect = planeRect(plane, room: r)
                let c = st == .selected ? Self.accent : rgba(0xE4E7EA, 0.5)
                s.lines.append(LineBatch(id: ctx.picks.id(e), segments: rect.outline, color: c, width: st == .selected ? 2 : 1, dashed: true))
                let n = rect.points.count / 3
                if n <= 5000 {
                    for k in 0..<n {
                        s.markers.append(Marker(id: ctx.picks.id(e), position: SIMD3(rect.points[k * 3], rect.points[k * 3 + 1], rect.points[k * 3 + 2]),
                                                shape: .ring, size: 3, color: rgba(0xE4E7EA, st == .selected ? 0.9 : 0.45)))
                    }
                }
            }
        }
    }

    /// Outlines of an audience area's floors (all floors if none chosen),
    /// lifted to its ear height.
    func drapeOutline(_ plane: CutPlane) -> [Float] {
        let h = Float(plane.drapeHeight ?? 0)
        let keys = plane.drapeSurfaces.isEmpty ? (store.room?.surfaces ?? []).filter { $0.kind == .floor }.map(\.key) : plane.drapeSurfaces
        var out: [Float] = []
        for k in keys {
            var o = ctx.geometryCache.outlines[k] ?? []
            for i in stride(from: 2, to: o.count, by: 3) { o[i] += h }
            out += o
        }
        return out
    }

    func micColor(_ m: Microphone, index: Int) -> SIMD4<Float> {
        if !m.color.isEmpty { return rgba(hex: m.color) }
        return rgba(hex: Color.wd.curveHex[index % Color.wd.curveHex.count])
    }

    /// Rectangle of a cut plane over the room bounds, plus its grid points (inside the bounds).
    func planeRect(_ plane: CutPlane, room r: RoomSnapshot) -> (outline: [Float], points: [Float], corners: [SIMD3<Float>]) {
        let lo = SIMD3<Float>(Float(r.boundsMin.x), Float(r.boundsMin.y), Float(r.boundsMin.z))
        let hi = SIMD3<Float>(Float(r.boundsMax.x), Float(r.boundsMax.y), Float(r.boundsMax.z))
        let o = Float(plane.offset)
        let corners: [SIMD3<Float>]
        switch plane.axis {
        case .horizontal: corners = [SIMD3(lo.x, lo.y, o), SIMD3(hi.x, lo.y, o), SIMD3(hi.x, hi.y, o), SIMD3(lo.x, hi.y, o)]
        case .constX: corners = [SIMD3(o, lo.y, lo.z), SIMD3(o, hi.y, lo.z), SIMD3(o, hi.y, hi.z), SIMD3(o, lo.y, hi.z)]
        case .constY: corners = [SIMD3(lo.x, o, lo.z), SIMD3(hi.x, o, lo.z), SIMD3(hi.x, o, hi.z), SIMD3(lo.x, o, hi.z)]
        }
        var outline: [Float] = []
        for k in 0..<4 {
            let a = corners[k], b = corners[(k + 1) % 4]
            outline += [a.x, a.y, a.z, b.x, b.y, b.z]
        }
        var points: [Float] = []
        let sp = Float(max(plane.spacing, 0.05))
        let u = corners[1] - corners[0], v = corners[3] - corners[0]
        let nu = max(1, Int((simd_length(u) / sp).rounded(.up))), nv = max(1, Int((simd_length(v) / sp).rounded(.up)))
        if nu * nv <= 5000 {
            for j in 0..<nv {
                for i in 0..<nu {
                    let q = corners[0] + u * ((Float(i) + 0.5) / Float(nu)) + v * ((Float(j) + 0.5) / Float(nv))
                    points += [q.x, q.y, q.z]
                }
            }
        }
        return (outline, points, corners)
    }

    // MARK: Results

    private func buildResults(_ s: inout SceneSnapshot, plan: Bool) {
        sectionRoom(&s)
        if ctx.flag("show.sources") { addSources(&s, dimmed: true, labels: false) }
        for plane in store.cutPlanes where plane.style.visible {
            addField(&s, plane)
        }
        addReceivers(&s, results: true)
    }

    /// The room as a section cut at the highest visible horizontal plane:
    /// walls solid up to the cut and nothing above it, so the field reads
    /// clearly on top with no translucent layers darkening it; the cut
    /// line is drawn brighter. With only vertical planes (or none), the
    /// walls are cut at the floor and only their outline shows.
    private func sectionRoom(_ s: inout SceneSnapshot) {
        guard let room = store.room else { return }
        let horizontal = store.cutPlanes.filter { $0.style.visible && $0.axis == .horizontal }.map(\.offset)
        let zc = Float(horizontal.max() ?? room.boundsMax.z)
        let solid = !horizontal.isEmpty && ctx.flag("show.surfaces")
        var walls: [Float] = [], floor: [Float] = [], edges: [Float] = [], cut: [Float] = []
        for surf in room.surfaces where layerVisible(surf.layerId) {
            switch surf.kind {
            case .ceiling: continue
            case .floor:
                floor += surf.positions
                edges += ctx.geometryCache.outlines[surf.key] ?? []
            default:
                let clipped = Self.clipBelow(surf.positions, z: zc)
                guard !clipped.isEmpty else { continue }
                if solid { walls += clipped }
                let o = outlineEdges(clipped)
                for k in 0..<(o.count / 6) {
                    let seg = Array(o[k * 6..<(k * 6 + 6)])
                    if abs(seg[2] - zc) < 1e-3 && abs(seg[5] - zc) < 1e-3 {
                        cut += [seg[0], seg[1], seg[2] + 0.01, seg[3], seg[4], seg[5] + 0.01]
                    } else {
                        edges += seg
                    }
                }
            }
        }
        if !floor.isEmpty { s.meshes.append(MeshDrawable(id: 0, positions: floor, color: rgba(0x15181C), lit: false)) }
        if !walls.isEmpty { s.meshes.append(MeshDrawable(id: 0, positions: walls, color: rgba(0x4A515B), lit: true)) }
        if ctx.flag("show.wire") || !solid {
            s.lines.append(LineBatch(segments: edges, color: rgba(0x8A939E, solid ? 0.35 : 0.55), width: 1))
        }
        s.lines.append(LineBatch(segments: cut, color: rgba(0xC9D1DA, 0.9), width: 1.4))
    }

    /// The parts of triangles at or below height `z`.
    static func clipBelow(_ p: [Float], z: Float) -> [Float] {
        var out: [Float] = []
        for t in 0..<(p.count / 9) {
            let v = (0..<3).map { SIMD3<Float>(p[t * 9 + $0 * 3], p[t * 9 + $0 * 3 + 1], p[t * 9 + $0 * 3 + 2]) }
            var poly: [SIMD3<Float>] = []
            for i in 0..<3 {
                let a = v[i], b = v[(i + 1) % 3]
                let ina = a.z <= z + 1e-5, inb = b.z <= z + 1e-5
                if ina { poly.append(a) }
                if ina != inb {
                    let u = (z - a.z) / (b.z - a.z)
                    poly.append(a + (b - a) * u)
                }
            }
            guard poly.count >= 3 else { continue }
            for i in 1..<(poly.count - 1) {
                for q in [poly[0], poly[i], poly[i + 1]] { out += [q.x, q.y, q.z] }
            }
        }
        return out
    }

    private func addField(_ s: inout SceneSnapshot, _ plane: CutPlane) {
        let e = EntityRef.plane(plane.id)
        let st = style(e)
        guard let room = store.room else { return }
        let draped = plane.drapeHeight != nil
        let rect = planeRect(plane, room: room)
        if let field = ctx.fieldValues(plane) {
            let stack = field.stack
            let du = Float(stack.du), dv = Float(stack.dv), off = draped ? 0 : Float(stack.offset)
            let (u0, v0) = (Float(stack.u0) - du / 2, Float(stack.v0) - dv / 2)
            let (uLen, vLen) = (du * Float(stack.nu), dv * Float(stack.nv))
            let origin: SIMD3<Float>, ua: SIMD3<Float>, va: SIMD3<Float>
            switch stack.axis {
            case .horizontal: origin = SIMD3(u0, v0, off); ua = SIMD3(uLen, 0, 0); va = SIMD3(0, vLen, 0)
            case .constX: origin = SIMD3(off, u0, v0); ua = SIMD3(0, uLen, 0); va = SIMD3(0, 0, vLen)
            case .constY: origin = SIMD3(u0, off, v0); ua = SIMD3(uLen, 0, 0); va = SIMD3(0, 0, vLen)
            }
            s.fields.append(FieldQuad(id: ctx.picks.id(e), origin: origin, uAxis: ua, vAxis: va, nu: Int(stack.nu), nv: Int(stack.nv),
                                      values: field.values, range: field.range, colorMap: ColorMap(rawValue: plane.style.colorMap) ?? .inferno,
                                      opacity: Float(plane.style.opacity), contourStep: plane.style.showContours ? Float(ctx.results.quantity == .spl ? plane.style.contourStep : ctx.results.decayContour) : 0,
                                      cells: ctx.results.cells, version: field.version, heights: draped ? stack.heights : []))
        }
        if draped {
            // Audience area: its floors' outlines at ear height (the height is set in the inspector).
            let c = st == .selected ? Self.accent : rgba(0x4DA3FF, 0.45)
            s.lines.append(LineBatch(id: ctx.picks.id(e), segments: drapeOutline(plane), color: SIMD4(c.x, c.y, c.z, 0.7), width: 1, dashed: true))
            return
        }
        // Chrome: dashed bounds, corner handles, normal handle and label.
        let c = st == .selected ? Self.accent : rgba(0x4DA3FF, 0.45)
        s.lines.append(LineBatch(id: ctx.picks.id(e), segments: rect.outline, color: SIMD4(c.x, c.y, c.z, 0.7), width: 1, dashed: true, overlay: false))
        if st == .selected {
            for q in rect.corners {
                s.markers.append(Marker(position: q, shape: .hollowSquare, size: 7, color: Self.accent, fill: rgba(0x121417)))
            }
            let n: SIMD3<Float> = plane.axis == .horizontal ? SIMD3(0, 0, 1) : plane.axis == .constX ? SIMD3(1, 0, 0) : SIMD3(0, 1, 0)
            let base = (rect.corners[0] + rect.corners[1] + rect.corners[2] + rect.corners[3]) / 4
            let mpp = Float(ctx.viewports[.results]?.projection.metresPerPoint ?? 0.02)
            let tip = base + n * max(mpp * 34, 0.3)
            s.lines.append(LineBatch(segments: [base.x, base.y, base.z, tip.x, tip.y, tip.z], color: Self.accent, width: 1.5, overlay: true))
            s.markers.append(Marker(id: ctx.picks.id(.planeOffset(plane.id)), position: tip, shape: .triangle, size: 11, color: Self.accent, fill: Self.accent))
            for t: Float in [0.35, 0.7] {
                s.markers.append(Marker(id: ctx.picks.id(.planeOffset(plane.id)), position: base + (tip - base) * t, shape: .dot, size: 9, color: SIMD4(0, 0, 0, 0)))
            }
            s.markers.append(Marker(position: base, shape: .hollowSquare, size: 7, color: Self.accent, fill: rgba(0x121417)))
        }
        let axisName = plane.axis == .horizontal ? "z" : plane.axis == .constX ? "x" : "y"
        let label = "\(plane.code) · \(axisName) \(String(format: "%.3f", plane.offset)) m"
        let top = rect.corners.max { $0.z < $1.z } ?? rect.corners[0]
        s.labels.append(Label(id: "plane-\(plane.id)", anchor: top, text: label, color: st == .selected ? Self.accentText : Self.tertiary,
                              priority: st == .selected ? 9 : 2, offset: CGSize(width: 0, height: -12)))
    }

    // MARK: Move gizmo

    /// Centre of the selected movable entities (sources, receivers).
    static func gizmoCentre(_ ctx: WindowContext) -> SIMD3<Double>? {
        var pts: [SIMD3<Double>] = []
        for e in ctx.selection.ids {
            if case .speaker(let id) = e, let s = ctx.store.speakersById[id] { pts.append(SIMD3(s.position.x, s.position.y, s.position.z)) }
            if case .microphone(let id) = e, let m = ctx.store.micsById[id] { pts.append(SIMD3(m.position.x, m.position.y, m.position.z)) }
            if case .group(let id) = e, let g = ctx.store.groupsById[id] {
                for m in g.members { if let s = ctx.store.speakersById[m] { pts.append(SIMD3(s.position.x, s.position.y, s.position.z)) } }
            }
        }
        guard !pts.isEmpty else { return nil }
        return pts.reduce(.zero, +) / Double(pts.count)
    }

    /// World length of `points` screen points at `p`.
    static func worldLength(_ points: Double, at p: SIMD3<Double>, _ vp: ViewportController) -> Double {
        let pr = vp.projection
        if pr.ortho { return pr.metresPerPoint * points }
        let d = simd_distance(pr.eye, p)
        return d * 2 * tan(vp.orbit.fovDeg * .pi / 360) / max(Double(pr.size.height), 1) * points
    }

    /// Three axis arrows (X red, Y green, Z blue), a plane handle and a centre dot.
    private func addGizmo(_ s: inout SceneSnapshot, ws: Workspace) {
        guard let c = Self.gizmoCentre(ctx), let vp = ctx.viewports[ws] else { return }
        let len = Self.worldLength(40, at: c, vp)
        let cf = SIMD3<Float>(c)
        let axes: [(SIMD3<Float>, SIMD4<Float>)] = [(SIMD3(1, 0, 0), rgba(0xE06C5A)), (SIMD3(0, 1, 0), rgba(0x6CC07A)), (SIMD3(0, 0, 1), rgba(0x4DA3FF))]
        for (i, (a, col)) in axes.enumerated() {
            if vp.mode == .plan && i == 2 { continue }
            let tip = cf + a * Float(len)
            s.lines.append(LineBatch(segments: [cf.x, cf.y, cf.z, tip.x, tip.y, tip.z], color: col, width: 2, overlay: true))
            let id = ctx.picks.id(.axis(i))
            s.markers.append(Marker(id: id, position: tip, shape: .dot, size: 7, color: col))
            for t: Float in [0.3, 0.55, 0.8] {
                s.markers.append(Marker(id: id, position: cf + a * Float(len) * t, shape: .dot, size: 8, color: SIMD4(0, 0, 0, 0)))
            }
        }
        let corner = cf + SIMD3<Float>(1, 1, 0) * Float(len * 0.3)
        s.markers.append(Marker(id: ctx.picks.id(.planeXY), position: corner, shape: .hollowSquare, size: 8, color: rgba(0x4DA3FF), fill: rgba(0x4DA3FF, 0.25)))
        s.markers.append(Marker(position: cf, shape: .dot, size: 5, color: rgba(0xF1F4F7)))
    }

    /// Ring of the Rotate tool around the selection, with a grab knob and,
    /// while turning, the angle.
    private func addRotateRing(_ s: inout SceneSnapshot, ws: Workspace) {
        guard let c = Self.gizmoCentre(ctx), let vp = ctx.viewports[ws] else { return }
        let r = Float(Self.worldLength(56, at: c, vp))
        let cf = SIMD3<Float>(c)
        let col = rgba(0xF29D38)
        var seg: [Float] = []
        let n = 72
        let id = ctx.picks.id(.rotateZ)
        for k in 0..<n {
            let a0 = Float(k) / Float(n) * 2 * .pi, a1 = Float(k + 1) / Float(n) * 2 * .pi
            let p = cf + SIMD3(cos(a0), sin(a0), 0) * r, q = cf + SIMD3(cos(a1), sin(a1), 0) * r
            seg += [p.x, p.y, p.z, q.x, q.y, q.z]
            if k % 3 == 0 { s.markers.append(Marker(id: id, position: p, shape: .dot, size: 12, color: SIMD4(0, 0, 0, 0))) }
        }
        s.lines.append(LineBatch(id: id, segments: seg, color: col, width: 2, overlay: true))
        // Knob along the facing of the first selected speaker (or +x).
        let yaw = ctx.selection.speakers.first.flatMap { ctx.store.speakersById[$0]?.yawDeg }
            ?? ctx.selectedGroups.first.flatMap { store.doc.groupAssembly(group: $0)?.yawDeg } ?? 0
        let a = Float(yaw * .pi / 180)
        let knob = cf + SIMD3(cos(a), sin(a), 0) * r
        s.lines.append(LineBatch(segments: [cf.x, cf.y, cf.z, knob.x, knob.y, knob.z], color: SIMD4(col.x, col.y, col.z, 0.5), width: 1, dashed: true, overlay: true))
        s.markers.append(Marker(id: id, position: knob, shape: .dot, size: 9, color: col))
        s.markers.append(Marker(position: cf, shape: .dot, size: 5, color: rgba(0xF1F4F7)))
        if let turn = ctx.tool.turnLabel {
            s.labels.append(Label(id: "rotate", anchor: knob, text: turn, color: col, priority: 10, offset: CGSize(width: 0, height: -14)))
        }
    }

    // MARK: Bounds

    static func bounds(of ids: [EntityRef], store: DocumentStore) -> (Vec3, Vec3)? {
        var lo = Vec3(repeating: .infinity), hi = Vec3(repeating: -.infinity)
        func add(_ p: Vec3) { lo = simd_min(lo, p); hi = simd_max(hi, p) }
        for e in ids {
            switch e {
            case .surface(let k):
                guard let s = store.surfacesByKey[k] else { continue }
                for i in 0..<(s.positions.count / 3) { add(Vec3(Double(s.positions[i * 3]), Double(s.positions[i * 3 + 1]), Double(s.positions[i * 3 + 2]))) }
            case .speaker(let id):
                if let s = store.speakersById[id] { add(Vec3(s.position.x, s.position.y, s.position.z)) }
            case .microphone(let id):
                if let m = store.micsById[id] { add(Vec3(m.position.x, m.position.y, m.position.z)) }
            case .segment(let id):
                if let s = store.segmentsById[id] { add(Vec3(s.pa.x, s.pa.y, 0)); add(Vec3(s.pb.x, s.pb.y, 0)) }
            case .plane(let id):
                if let p = store.planesById[id], let r = store.room {
                    let lo3 = Vec3(r.boundsMin.x, r.boundsMin.y, r.boundsMin.z), hi3 = Vec3(r.boundsMax.x, r.boundsMax.y, r.boundsMax.z)
                    switch p.axis {
                    case .horizontal: add(Vec3(lo3.x, lo3.y, p.offset)); add(Vec3(hi3.x, hi3.y, p.offset))
                    case .constX: add(Vec3(p.offset, lo3.y, lo3.z)); add(Vec3(p.offset, hi3.y, hi3.z))
                    case .constY: add(Vec3(lo3.x, p.offset, lo3.z)); add(Vec3(hi3.x, p.offset, hi3.z))
                    }
                }
            default: continue
            }
        }
        return lo.x.isFinite ? (lo, hi) : nil
    }
}
