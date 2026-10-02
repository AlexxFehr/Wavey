import AppKit
import Foundation
import simd
import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel
import WaveyViewport

/// Runs the active tool: turns viewport events into selection changes,
/// previews and edits. Tools never mutate the model directly; every change
/// goes through `WindowContext.edit` as one named undo step.
@MainActor
final class ToolSession {
    unowned let ctx: WindowContext

    // Pointer state
    private(set) var snap: SnapResult?
    private var hoverWorld: Vec3?
    private var screen: CGPoint?
    // Drawing state
    private(set) var points: [SIMD2<Double>] = []
    private var chainStartVertex: SIMD2<Double>?
    private var editsInChain = 0
    // Drag state
    private enum Drag {
        case marquee(start: CGPoint, now: CGPoint)
        case vertex(id: UInt64, start: SIMD2<Double>)
        case entities(start: Vec3, items: [(EntityRef, Point3)], key: String)
        case planeOffset(id: UInt64, start: CGPoint, offset: Double)
        /// Gizmo: along `axis` through `origin` (or in the horizontal plane), from parameter `s0`.
        case gizmo(axis: Vec3?, origin: Vec3, s0: Vec3, items: [(EntityRef, Point3)], key: String)
        case offsetHandle(plane: UInt64, normal: Vec3, origin: Vec3, s0: Double, offset0: Double, key: String)
    }
    private var drag: Drag?
    private var pendingPick: EntityRef?
    private var dragKeyCounter = 0
    // Tool-specific
    private var aimSource: UInt64?
    private var offsetSegment: UInt64?
    private var measure: [Vec3] = []
    private var lastSourcePlaced: UInt64?
    private var probe: [Vec3] = []
    /// Command-line prompt for the active tool.
    var prompt: String {
        switch ctx.activeTool {
        case .select: "Select objects (drag for a window, right-to-left for crossing)"
        case .move: points.isEmpty ? "Specify base point:" : "Specify second point or [Copy]:"
        case .line, .polyline: points.isEmpty ? "Specify first point:" : "Specify next point or [Close/Undo]:"
        case .rectangle: points.isEmpty ? "Specify first corner point or [Elevation]:" : "Specify other corner or [Dimensions]:"
        case .arc: points.count == 0 ? "Specify start point of arc:" : points.count == 1 ? "Specify second point of arc:" : "Specify end point of arc:"
        case .offset: offsetSegment == nil ? "Select edge to offset:" : "Specify offset distance or click the side:"
        case .extrude: "Select closed loop to extrude, or type height:"
        case .surfaceFromLoop: "Select closed loop:"
        case .split: "Select surface to split:"
        case .measure: measure.isEmpty ? "Specify first point:" : "Specify next point:"
        case .aim: aimSource == nil ? "Select source to aim:" : "Select target (receiver, point):"
        case .omni, .loudspeaker, .subwoofer: lastSourcePlaced == nil ? "Click to place · Esc to finish" : "Click to place another · ⌥-click to aim the last one · Esc to finish"
        case .receiver: "Click to place receiver · Esc to finish"
        case .receiverGrid: "Click to add a receiver grid"
        case .cutPlane: "Click a surface for an aligned plane, or empty space for a horizontal plane at 1.200 m"
        case .probeLine: probe.isEmpty ? "Specify start of probe line:" : "Specify end of probe line:"
        case .microphone: "Click on a plane or the floor to place a microphone"
        }
    }

    init(context: WindowContext) {
        ctx = context
    }

    var store: DocumentStore { ctx.store }

    // MARK: Activation

    func activate(_ t: ToolID) {
        cancel()
        ctx.activeTool = t
        if t == .split {
            ctx.post("Split surface needs general planar surfaces, which the geometry engine does not support yet", .warning)
        }
        ctx.rebuildScenes()
    }

    /// Esc: cancel the operation, then clear selection, then return to Select.
    func escape() {
        if drag != nil || !points.isEmpty || aimSource != nil || offsetSegment != nil || !measure.isEmpty || !probe.isEmpty {
            cancel()
        } else if lastSourcePlaced != nil {
            lastSourcePlaced = nil
        } else if !ctx.selection.isEmpty {
            ctx.selection.clear()
        } else {
            ctx.activeTool = .select
        }
        ctx.rebuildScenes()
    }

    func cancel() {
        drag = nil
        points = []
        chainStartVertex = nil
        editsInChain = 0
        aimSource = nil
        offsetSegment = nil
        measure = []
        probe = []
        lastSourcePlaced = nil
    }

    /// ⌘Z while drawing removes the last point first. True if consumed.
    func undoPoint() -> Bool {
        guard [.line, .polyline, .arc, .rectangle, .measure].contains(ctx.activeTool), !points.isEmpty || !measure.isEmpty else { return false }
        if !measure.isEmpty { measure.removeLast(); return true }
        if (ctx.activeTool == .line || ctx.activeTool == .polyline), editsInChain > 0 {
            store.undo()
            editsInChain -= 1
        }
        points.removeLast()
        return true
    }

    // MARK: Events

    func handle(_ e: ViewportEvent, in ws: Workspace) {
        guard ws == ctx.workspace else { return }
        let vp = ctx.viewports[ws]!
        switch e {
        case .hover(let world, let pick, let pt):
            screen = pt
            hoverWorld = world
            updateSnap(world, vp: vp)
            ctx.cursor = snapped3D(world)
            let hovered = pick.flatMap(ctx.picks.entity) ?? segmentNear(pt, vp: vp).map { EntityRef.segment($0) }
            if hovered != ctx.selection.hover || needsPreview { ctx.selection.hover = hovered; ctx.rebuildScenes() }
        case .exited:
            screen = nil
            snap = nil
            if ctx.selection.hover != nil { ctx.selection.hover = nil; ctx.rebuildScenes() }
        case .down(let world, let pick, let pt, let mods, let clicks):
            if let h = ctx.picks.handle(pick) {
                beginHandle(h, at: pt, vp: vp)
                return
            }
            down(world: world, pick: pick.flatMap(ctx.picks.entity), pt: pt, mods: mods, clicks: clicks, vp: vp)
        case .drag(let world, let pt, let start, let mods):
            dragged(world: world, pt: pt, start: start, mods: mods, vp: vp)
        case .up(let world, let pick, let pt, let mods):
            up(world: world, pick: pick.flatMap(ctx.picks.entity), pt: pt, mods: mods, vp: vp)
        case .marquee:
            break
        case .key(let chars, let code, let mods):
            key(chars, code: code, mods: mods)
        case .context(let pick, let pt):
            ctx.contextMenu(for: pick.flatMap(ctx.picks.entity), at: pt)
        }
    }

    private var needsPreview: Bool {
        !points.isEmpty || !measure.isEmpty || !probe.isEmpty || offsetSegment != nil || aimSource != nil || isPointTool
    }

    private var isPointTool: Bool {
        [.line, .polyline, .rectangle, .arc, .move, .measure, .offset, .probeLine].contains(ctx.activeTool)
    }

    private func updateSnap(_ world: Vec3?, vp: ViewportController) {
        guard let w = world, isPointTool else { snap = nil; return }
        let flags = NSEvent.modifierFlags
        let engine = SnapEngine(store: store, enabled: ctx.snapKinds, ortho: ctx.flag("ortho") != flags.contains(.shift) && ctx.flag("ortho"),
                                polarDeg: ctx.flag("polar") ? 15 : nil, grid: ctx.gridStep, mpp: vp.projection.metresPerPoint)
        snap = engine.snap(SIMD2(w.x, w.y), from: points.last, fine: flags.contains(.command), off: flags.contains(.option))
    }

    private func snapped3D(_ world: Vec3?) -> Vec3? {
        if let s = snap { return Vec3(s.point.x, s.point.y, world?.z ?? 0) }
        return world
    }

    /// Nearest sketch segment within 8 pt (CPU pick for plan lines).
    func segmentNear(_ pt: CGPoint, vp: ViewportController) -> UInt64? {
        guard vp.mode == .plan else { return nil }
        let p = vp.projection
        var best: (UInt64, CGFloat)?
        for s in store.sketch.segments {
            guard store.layersById[s.layerId]?.visible ?? true, !(store.layersById[s.layerId]?.locked ?? false) else { continue }
            guard let a = p.project(SIMD3<Double>(s.pa.x, s.pa.y, ctx.planZ)), let b = p.project(SIMD3<Double>(s.pb.x, s.pb.y, ctx.planZ)) else { continue }
            let d = distance(pt, a, b)
            if d <= 8, d < (best?.1 ?? .infinity) { best = (s.id, d) }
        }
        return best?.0
    }

    private func vertexNear(_ pt: CGPoint, vp: ViewportController) -> (UInt64, SIMD2<Double>)? {
        guard vp.mode == .plan else { return nil }
        let p = vp.projection
        for v in store.sketch.vertices {
            guard let s = p.project(SIMD3<Double>(v.p.x, v.p.y, ctx.planZ)) else { continue }
            if hypot(s.x - pt.x, s.y - pt.y) <= 8 { return (v.id, SIMD2(v.p.x, v.p.y)) }
        }
        return nil
    }

    private func distance(_ p: CGPoint, _ a: CGPoint, _ b: CGPoint) -> CGFloat {
        let dx = b.x - a.x, dy = b.y - a.y
        let l2 = dx * dx + dy * dy
        guard l2 > 1e-9 else { return hypot(p.x - a.x, p.y - a.y) }
        let t = max(0, min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / l2))
        return hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy))
    }

    // MARK: Pointer down/drag/up

    private func down(world: Vec3?, pick: EntityRef?, pt: CGPoint, mods: ModifierSet, clicks: Int, vp: ViewportController) {
        let target = pick ?? segmentNear(pt, vp: vp).map { EntityRef.segment($0) }
        let p2 = snap?.point ?? world.map { SIMD2($0.x, $0.y) }
        switch ctx.activeTool {
        case .select, .move where points.isEmpty && ctx.activeTool == .select:
            if clicks == 2, target == nil { vp.zoomToFit(); return }
            if ctx.activeTool == .select, let (vid, at) = vertexNear(pt, vp: vp), !mods.contains(.shift) {
                drag = .vertex(id: vid, start: at)
                return
            }
            if let t = target, ctx.selection.contains(t), !mods.contains(.shift), !mods.contains(.command), let w = world {
                let items = movable()
                if !items.isEmpty {
                    dragKeyCounter += 1
                    drag = .entities(start: w, items: items, key: "drag-\(dragKeyCounter)")
                    return
                }
            }
            pendingPick = target
            drag = .marquee(start: pt, now: pt)
        case .move:
            guard let p = p2 else { return }
            if points.isEmpty {
                if ctx.selection.isEmpty, let t = target { ctx.select([t]) }
                points = [p]
            } else {
                moveSelection(by: p - points[0], copy: mods.contains(.option))
                points = []
                ctx.activeTool = .select
            }
        case .line, .polyline:
            guard let p = p2 else { return }
            addChainPoint(p)
        case .rectangle:
            guard let p = p2 else { return }
            if points.isEmpty { points = [p] } else { finishRectangle(points[0], p) }
        case .arc:
            guard let p = p2 else { return }
            points.append(p)
            if points.count == 3 { finishArc() }
        case .offset:
            if let s = offsetSegment, let p = p2 {
                guard let seg = store.segmentsById[s] else { offsetSegment = nil; return }
                let a = SIMD2(seg.pa.x, seg.pa.y), b = SIMD2(seg.pb.x, seg.pb.y)
                let foot = SnapEngine(store: store, enabled: [], ortho: false, polarDeg: nil, grid: 1, mpp: 1).project(p, onto: (a, b), clamp: false)
                offset(s, by: simd_distance(p, foot), towards: p)
            } else if case .segment(let id) = target {
                offsetSegment = id
            } else if case .surface(let k) = target, let seg = store.surfacesByKey[k]?.segmentId {
                offsetSegment = seg
            }
        case .extrude, .surfaceFromLoop:
            guard let w = world else { return }
            extrude(at: SIMD2(w.x, w.y), height: nil)
        case .split:
            ctx.post("Split surface needs general planar surfaces (a later engine phase); nothing was changed", .warning)
        case .measure:
            guard let w = world else { return }
            let p = snap.map { Vec3($0.point.x, $0.point.y, w.z) } ?? w
            if mods.contains(.shift) || measure.count < 2 { measure.append(p) } else { measure = [p] }
            reportMeasure()
        case .aim:
            if aimSource == nil {
                if case .speaker(let id) = target { aimSource = id; ctx.select([.speaker(id)]) } else { ctx.post("Click a source first", .info) }
            } else if let src = aimSource {
                aim(src, at: target, world: world)
                aimSource = nil
            }
        case .omni, .loudspeaker, .subwoofer:
            guard let w = world else { return }
            // ⌥-click after placing sets the new source's aim point.
            if let last = lastSourcePlaced, mods.contains(.option) {
                aim(last, at: target, world: w)
                lastSourcePlaced = nil
                return
            }
            placeSource(at: w)
        case .receiver:
            guard let w = world else { return }
            placeReceiver(at: w, solved: true)
        case .receiverGrid:
            ctx.receiverGridPopover = pt
        case .cutPlane:
            addCutPlane(on: target, world: world)
        case .microphone:
            guard let w = world else { return }
            placeResultsMic(at: w, on: target)
        case .probeLine:
            guard let w = world else { return }
            probe.append(w)
            if probe.count == 2 {
                ctx.probeLine = (probe[0], probe[1])
                ctx.post(String(format: "Probe line %.2f m — spatial plot in the Magnitude tab", simd_distance(probe[0], probe[1])))
                probe = []
                ctx.results.plotMode = "spatial"
            }
        }
        ctx.rebuildScenes()
    }

    // MARK: Handles

    /// Parameter along the line `origin + axis·s` closest to the pointer ray.
    private func lineParam(_ pt: CGPoint, origin: Vec3, axis: Vec3, vp: ViewportController) -> Double? {
        let (o, d) = vp.projection.ray(pt)
        let b = simd_dot(axis, d)
        guard abs(1 - b * b) > 1e-6 else { return nil }
        let w0 = origin - o
        return (b * simd_dot(d, w0) - simd_dot(axis, w0)) / (1 - b * b)
    }

    private func beginHandle(_ h: Handle, at pt: CGPoint, vp: ViewportController) {
        dragKeyCounter += 1
        let key = "handle-\(dragKeyCounter)"
        switch h {
        case .axis(let i):
            guard let c = SceneBuilder.gizmoCentre(ctx) else { return }
            let a: Vec3 = i == 0 ? Vec3(1, 0, 0) : i == 1 ? Vec3(0, 1, 0) : Vec3(0, 0, 1)
            guard let s0 = lineParam(pt, origin: c, axis: a, vp: vp) else { return }
            drag = .gizmo(axis: a, origin: c, s0: Vec3(s0, 0, 0), items: movable(), key: key)
        case .planeXY:
            guard let c = SceneBuilder.gizmoCentre(ctx), let hit = vp.projection.onPlane(pt, z: c.z) else { return }
            drag = .gizmo(axis: nil, origin: c, s0: hit, items: movable(), key: key)
        case .planeOffset(let id):
            guard let p = store.planesById[id], let r = store.room else { return }
            let n: Vec3 = p.axis == .horizontal ? Vec3(0, 0, 1) : p.axis == .constX ? Vec3(1, 0, 0) : Vec3(0, 1, 0)
            let c = Vec3((r.boundsMin.x + r.boundsMax.x) / 2, (r.boundsMin.y + r.boundsMax.y) / 2, (r.boundsMin.z + r.boundsMax.z) / 2)
            let base = c + n * (p.offset - simd_dot(c, n))
            guard let s0 = lineParam(pt, origin: base, axis: n, vp: vp) else { return }
            drag = .offsetHandle(plane: id, normal: n, origin: base, s0: s0, offset0: p.offset, key: key)
        }
    }

    private func dragged(world: Vec3?, pt: CGPoint, start: CGPoint, mods: ModifierSet, vp: ViewportController) {
        let fine = mods.contains(.command)
        func snapStep(_ v: Double, _ step: Double) -> Double { (v / step).rounded(.toNearestOrAwayFromZero) * step }
        switch drag {
        case .gizmo(let axis, let origin, let s0, let items, let key):
            var d: Vec3
            if let a = axis {
                guard let s = lineParam(pt, origin: origin, axis: a, vp: vp) else { return }
                d = a * snapStep(s - s0.x, fine ? 0.01 : 0.05)
            } else {
                guard let hit = vp.projection.onPlane(pt, z: origin.z) else { return }
                d = hit - s0
                d = Vec3(snapStep(d.x, fine ? 0.01 : 0.05), snapStep(d.y, fine ? 0.01 : 0.05), 0)
            }
            translate(items, by: d, key: key, keepFloor: axis?.z != 1)
            return
        case .offsetHandle(let id, let n, let origin, let s0, let off0, let key):
            guard let s = lineParam(pt, origin: origin, axis: n, vp: vp), var p = store.planesById[id] else { return }
            p.offset = snapStep(off0 + s - s0, fine ? 0.01 : 0.05)
            ctx.edit("Move Plane", coalesce: key) { try $0.updateCutPlane(plane: p) }
            return
        default:
            break
        }
        switch drag {
        case .marquee(let s, _):
            drag = .marquee(start: s, now: pt)
            ctx.marquee = CGRect(x: min(s.x, pt.x), y: min(s.y, pt.y), width: abs(pt.x - s.x), height: abs(pt.y - s.y))
            ctx.marqueeCrossing = pt.x < s.x
        case .vertex(let id, _):
            updateSnap(world, vp: vp)
            if let p = snap?.point ?? world.map({ SIMD2($0.x, $0.y) }) {
                ctx.edit("Move Vertex", coalesce: "vertex-\(id)") { try? $0.moveVertex(id: id, to: Point2(x: p.x, y: p.y)) }
            }
            return
        case .entities(let start, let items, let key):
            guard let w = world else { return }
            var d = w - start
            if mods.contains(.shift) { if abs(d.x) > abs(d.y) { d.y = 0 } else { d.x = 0 } }
            if !mods.contains(.command) { d = (d / 0.05).rounded(.toNearestOrAwayFromZero) * 0.05 }
            translate(items, by: d, key: key)
            return
        default:
            break
        }
        ctx.rebuildScenes()
    }

    private func up(world: Vec3?, pick: EntityRef?, pt: CGPoint, mods: ModifierSet, vp: ViewportController) {
        defer {
            ctx.marquee = nil
            ctx.rebuildScenes()
        }
        switch drag {
        case .marquee(let s, let now):
            drag = nil
            let rect = CGRect(x: min(s.x, now.x), y: min(s.y, now.y), width: abs(now.x - s.x), height: abs(now.y - s.y))
            if rect.width < 4 && rect.height < 4 {
                click(pendingPick ?? pick ?? segmentNear(pt, vp: vp).map { .segment($0) }, mods: mods)
            } else {
                let hits = marqueeHits(rect, crossing: now.x < s.x, vp: vp)
                if mods.contains(.option) { ctx.selection.subtract(hits) } else if mods.contains(.shift) { ctx.selection.extend(hits) } else { ctx.selection.select(hits) }
            }
            pendingPick = nil
        case .vertex, .entities, .gizmo, .offsetHandle:
            drag = nil
        default:
            break
        }
    }

    private func click(_ target: EntityRef?, mods: ModifierSet) {
        guard var t = target else {
            if !mods.contains(.shift) { ctx.selection.clear() }
            return
        }
        // Clicking a wall in plan selects its surfaces' segment as the wall surface (front side).
        if case .segment(let id) = t, ctx.workspace == .geometry {
            if let key = ["wall:\(id):f", "wall:\(id):r"].first(where: { store.surfacesByKey[$0] != nil }) { t = .surface(key) }
        }
        if mods.contains(.command) { ctx.selection.toggle(t) } else if mods.contains(.shift) { ctx.selection.extend([t]) } else { ctx.selection.select([t]) }
    }

    /// Entities whose anchors fall in the rectangle (window: all, crossing: any).
    private func marqueeHits(_ rect: CGRect, crossing: Bool, vp: ViewportController) -> [EntityRef] {
        let p = vp.projection
        var out: [EntityRef] = []
        func test(_ e: EntityRef, _ pts: [SIMD3<Double>]) {
            let proj = pts.compactMap { p.project($0) }
            guard !proj.isEmpty else { return }
            let inside = proj.map { rect.contains($0) }
            if crossing ? inside.contains(true) : !inside.contains(false) { out.append(e) }
        }
        let z = ctx.planZ
        if vp.mode == .plan {
            for s in store.sketch.segments where store.layersById[s.layerId]?.visible ?? true {
                let key = ["wall:\(s.id):f", "wall:\(s.id):r"].first { store.surfacesByKey[$0] != nil }
                test(key.map { .surface($0) } ?? .segment(s.id), [SIMD3(s.pa.x, s.pa.y, z), SIMD3(s.pb.x, s.pb.y, z)])
            }
        } else {
            for s in store.room?.surfaces ?? [] where ctx.workspace == .geometry {
                var pts: [SIMD3<Double>] = []
                for i in 0..<(s.positions.count / 3) { pts.append(SIMD3(Double(s.positions[i * 3]), Double(s.positions[i * 3 + 1]), Double(s.positions[i * 3 + 2]))) }
                test(.surface(s.key), pts)
            }
        }
        if ctx.workspace != .geometry || vp.mode != .plan {
            for s in store.speakers { test(.speaker(s.id), [SIMD3(s.position.x, s.position.y, s.position.z)]) }
            for m in store.microphones { test(.microphone(m.id), [SIMD3(m.position.x, m.position.y, m.position.z)]) }
        }
        return out
    }

    // MARK: Keys and text

    private func key(_ chars: String, code: UInt16, mods: ModifierSet) {
        switch code {
        case 53: escape(); return // Esc
        case 36, 76: finishChain(); return // Return
        case 51, 117: // Delete
            ctx.deleteSelection()
            return
        default: break
        }
        guard mods.isEmpty || mods == .shift else { return }
        let c = chars.uppercased()
        if [.line, .polyline].contains(ctx.activeTool), !points.isEmpty {
            if c == "C" { closeChain(); return }
            if c == "U" { _ = undoPoint(); ctx.rebuildScenes(); return }
        }
        if c == "F" { ctx.frameSelection(); return }
        if ctx.workspace == .results {
            let big = mods.contains(.shift) ? 6 : 1
            if chars == "," || chars == "<" { ctx.stepFrequency(-big); return }
            if chars == "." || chars == ">" { ctx.stepFrequency(big); return }
        }
        if c == "H", mods.isEmpty { ctx.hideSelection(); return }
        // Digits start typed input into the command line.
        if let f = c.first, f.isNumber || f == "@" || f == "#" || f == "-" || f == "." {
            ctx.commandDraft = chars
            ctx.focusCommandLine = true
            return
        }
        // Tool shortcuts scoped to the workspace.
        for t in railTools(ctx.workspace).flatMap({ $0 }) where t.key(in: ctx.workspace) == c {
            activate(t)
            return
        }
    }

    /// Text from the command line or dynamic input. True if consumed.
    func textEntered(_ text: String) -> Bool {
        let t = text.trimmingCharacters(in: .whitespaces)
        let lengthParse: (String) -> Double? = { UnitFormatter(.length).parse($0) }
        switch ctx.activeTool {
        case .line, .polyline, .rectangle, .arc, .move, .measure:
            if t.uppercased() == "C", !points.isEmpty { closeChain(); return true }
            if t.uppercased() == "U" { _ = undoPoint(); ctx.rebuildScenes(); return true }
            guard let input = PointInput.parse(t, unit: lengthParse) else { return false }
            let p: SIMD2<Double>
            switch input {
            case .absolute(let a): p = a
            case .relative(let d): p = (points.last ?? .zero) + d
            case .distance(let d):
                guard let last = points.last else { return false }
                let cur = snap?.point ?? hoverWorld.map { SIMD2($0.x, $0.y) } ?? (last + SIMD2(1, 0))
                let dir = simd_length(cur - last) > 1e-9 ? simd_normalize(cur - last) : SIMD2(1, 0)
                p = last + dir * d
            }
            let w = Vec3(p.x, p.y, 0)
            switch ctx.activeTool {
            case .line, .polyline: addChainPoint(p)
            case .rectangle: if points.isEmpty { points = [p] } else { finishRectangle(points[0], p) }
            case .arc: points.append(p); if points.count == 3 { finishArc() }
            case .move:
                if points.isEmpty { points = [p] } else { moveSelection(by: p - points[0], copy: false); points = [] }
            case .measure: measure.append(w); reportMeasure()
            default: break
            }
            ctx.rebuildScenes()
            return true
        case .extrude:
            guard let h = lengthParse(t), h > 0 else { return false }
            ctx.edit("Set Height") { try $0.upsertLevel(level: LevelRec(id: "ceiling", name: "Ceiling", z: h, kind: .ceiling)) }
            ctx.post(String(format: "Extrusion height %.3f m", h))
            return true
        case .offset:
            guard let s = offsetSegment, let d = lengthParse(t), let w = hoverWorld else { return false }
            offset(s, by: d, towards: SIMD2(w.x, w.y))
            return true
        default:
            return false
        }
    }

    // MARK: Geometry operations

    private func addChainPoint(_ p: SIMD2<Double>) {
        guard let last = points.last else {
            points = [p]
            chainStartVertex = p
            editsInChain = 0
            return
        }
        guard simd_distance(last, p) > 1e-6 else { return }
        let name = ctx.activeTool == .polyline ? "Polyline" : "Line"
        ctx.edit("Add \(name)") { _ = $0.addSegment(a: Point2(x: last.x, y: last.y), b: Point2(x: p.x, y: p.y)) }
        editsInChain += 1
        if let s = chainStartVertex, simd_distance(s, p) < 1e-6, points.count > 1 {
            finishChain()
            return
        }
        points.append(p)
    }

    private func closeChain() {
        guard let s = chainStartVertex, points.count >= 2 else { return }
        addChainPoint(s)
        finishChain()
    }

    func finishChain() {
        if [.line, .polyline].contains(ctx.activeTool) {
            points = []
            chainStartVertex = nil
            editsInChain = 0
        }
        ctx.rebuildScenes()
    }

    private func finishRectangle(_ a: SIMD2<Double>, _ b: SIMD2<Double>) {
        guard abs(a.x - b.x) > 1e-6, abs(a.y - b.y) > 1e-6 else { return }
        let pts = [Point2(x: a.x, y: a.y), Point2(x: b.x, y: a.y), Point2(x: b.x, y: b.y), Point2(x: a.x, y: b.y)]
        ctx.edit("Add Rectangle") { _ = $0.addPolygon(points: pts) }
        points = []
        ctx.post(String(format: "Rectangle %.3f × %.3f m", abs(b.x - a.x), abs(b.y - a.y)))
    }

    private func finishArc() {
        let (a, m, b) = (points[0], points[1], points[2])
        points = []
        guard let pts = arcPoints(a, m, b) else { ctx.post("The three points are collinear", .warning); return }
        dragKeyCounter += 1
        let key = "arc-\(dragKeyCounter)"
        for i in 0..<(pts.count - 1) {
            let (p, q) = (pts[i], pts[i + 1])
            ctx.edit("Add Arc", coalesce: key) { _ = $0.addSegment(a: Point2(x: p.x, y: p.y), b: Point2(x: q.x, y: q.y)) }
        }
    }

    /// Polyline through a 3-point arc (≈ 10° per segment).
    func arcPoints(_ a: SIMD2<Double>, _ m: SIMD2<Double>, _ b: SIMD2<Double>) -> [SIMD2<Double>]? {
        let d = 2 * (a.x * (m.y - b.y) + m.x * (b.y - a.y) + b.x * (a.y - m.y))
        guard abs(d) > 1e-9 else { return nil }
        let ux = (simd_length_squared(a) * (m.y - b.y) + simd_length_squared(m) * (b.y - a.y) + simd_length_squared(b) * (a.y - m.y)) / d
        let uy = (simd_length_squared(a) * (b.x - m.x) + simd_length_squared(m) * (a.x - b.x) + simd_length_squared(b) * (m.x - a.x)) / d
        let c = SIMD2(ux, uy), r = simd_distance(a, c)
        func ang(_ p: SIMD2<Double>) -> Double { atan2(p.y - c.y, p.x - c.x) }
        let a0 = ang(a)
        var am = ang(m), a1 = ang(b)
        // Go from a to b through m.
        func norm(_ x: Double) -> Double { var y = x; while y < a0 { y += 2 * .pi }; return y }
        am = norm(am); a1 = norm(a1)
        if am > a1 { a1 -= 2 * .pi }
        let n = max(2, Int(abs(a1 - a0) / (10 * .pi / 180)))
        return (0...n).map { i in
            let t = a0 + (a1 - a0) * Double(i) / Double(n)
            return c + SIMD2(cos(t), sin(t)) * r
        }
    }

    private func offset(_ seg: UInt64, by d: Double, towards p: SIMD2<Double>) {
        guard let s = store.segmentsById[seg], d > 1e-6 else { return }
        let a = SIMD2(s.pa.x, s.pa.y), b = SIMD2(s.pb.x, s.pb.y)
        var n = simd_normalize(SIMD2(-(b - a).y, (b - a).x))
        if simd_dot(p - a, n) < 0 { n = -n }
        let (a2, b2) = (a + n * d, b + n * d)
        ctx.edit("Offset") { _ = $0.addSegment(a: Point2(x: a2.x, y: a2.y), b: Point2(x: b2.x, y: b2.y)) }
        offsetSegment = nil
    }

    private func extrude(at p: SIMD2<Double>, height: Double?) {
        let name = ctx.activeTool == .extrude ? "Extrude" : "Surface from Loop"
        var found = true
        ctx.edit(name) { found = $0.setInteriorAt(p: Point2(x: p.x, y: p.y), interior: true) }
        if !found {
            ctx.post("No closed loop here: draw a closed outline first (Rectangle, or Line with C to close)", .warning)
        } else if let h = store.levels.first(where: { $0.kind == .ceiling })?.z {
            ctx.post(String(format: "%@ to %.3f m — type a height to change it", name, h))
        }
    }

    // MARK: Moving

    private func movable() -> [(EntityRef, Point3)] {
        ctx.selection.ids.compactMap { e in
            switch e {
            case .speaker(let id): store.speakersById[id].map { (e, $0.position) }
            case .microphone(let id): store.micsById[id].map { (e, $0.position) }
            default: nil
            }
        }
    }

    private func translate(_ items: [(EntityRef, Point3)], by d: Vec3, key: String, keepFloor: Bool = true) {
        let spk: [SpeakerRec] = items.compactMap { e, p0 in
            guard case .speaker(let id) = e, var s = store.speakersById[id] else { return nil }
            s.position = Point3(x: p0.x + d.x, y: p0.y + d.y, z: p0.z + d.z)
            if !keepFloor, d.z != 0 { s.onFloor = false }
            return s
        }
        let mics: [MicrophoneRec] = items.compactMap { e, p0 in
            guard case .microphone(let id) = e, var m = store.micsById[id] else { return nil }
            m.position = Point3(x: p0.x + d.x, y: p0.y + d.y, z: p0.z + d.z)
            return m
        }
        let name = items.count == 1 ? (spk.isEmpty ? "Move Receiver" : "Move Source") : "Move"
        ctx.edit(name, coalesce: key) { doc in
            if !spk.isEmpty { try doc.updateSpeakers(speakers: spk) }
            if !mics.isEmpty { try doc.updateMicrophones(mics: mics) }
        }
    }

    private func moveSelection(by d: SIMD2<Double>, copy: Bool) {
        let items = movable()
        if !items.isEmpty {
            dragKeyCounter += 1
            translate(items, by: Vec3(d.x, d.y, 0), key: "move-\(dragKeyCounter)")
        }
        // Sketch: move the vertices of selected segments/walls.
        var verts = Set<UInt64>()
        for e in ctx.selection.ids {
            var seg: UInt64?
            if case .segment(let id) = e { seg = id }
            if case .surface(let k) = e { seg = store.surfacesByKey[k]?.segmentId }
            if let s = seg.flatMap({ store.segmentsById[$0] }) { verts.insert(s.a); verts.insert(s.b) }
        }
        if !verts.isEmpty {
            let pos = Dictionary(uniqueKeysWithValues: store.sketch.vertices.map { ($0.id, $0.p) })
            dragKeyCounter += 1
            let key = "movev-\(dragKeyCounter)"
            for v in verts {
                guard let p = pos[v] else { continue }
                ctx.edit("Move", coalesce: key) { $0.moveVertex(id: v, to: Point2(x: p.x + d.x, y: p.y + d.y)) }
            }
        }
        if items.isEmpty && verts.isEmpty { ctx.post("Nothing selected to move", .info) }
        _ = copy
    }

    // MARK: Sources & receivers

    private func aim(_ src: UInt64, at target: EntityRef?, world: Vec3?) {
        guard var s = store.speakersById[src] else { return }
        if case .microphone(let id) = target {
            s.aim = .microphone(id: id)
        } else if let w = world {
            s.aim = .point(p: Point3(x: w.x, y: w.y, z: 1.2))
        } else { return }
        ctx.edit("Aim Source") { try $0.updateSpeaker(speaker: s) }
    }

    /// Device for a placement tool: the library selection if it fits, else the first of a matching kind.
    private func device(for tool: ToolID) -> SpeakerModelRec? {
        let models = store.speakerModels
        func isSub(_ m: SpeakerModelRec) -> Bool { (m.nominalMaxHz ?? m.maxHz) <= 250 || m.name.lowercased().contains("sub") }
        func isOmni(_ m: SpeakerModelRec) -> Bool { !m.hasBalloon || m.name.lowercased().contains("omni") }
        if let id = ctx.libraryDevice, let m = store.modelsById[id] {
            switch tool {
            case .subwoofer where isSub(m), .omni where isOmni(m), .loudspeaker: return m
            default: break
            }
        }
        switch tool {
        case .subwoofer: return models.first(where: isSub)
        case .omni: return models.first(where: isOmni)
        default: return models.first { !isSub($0) && !isOmni($0) } ?? models.first
        }
    }

    private func placeSource(at w: Vec3) {
        let tool = ctx.activeTool
        if device(for: tool) == nil {
            ctx.edit("Add Sample Devices") { try $0.addSampleSpeakerModels() }
        }
        guard let m = device(for: tool) else { return }
        let sub = tool == .subwoofer
        var placed: SpeakerRec?
        let p = Point3(x: w.x, y: w.y, z: sub ? 0.2 : 1.35)
        ctx.edit("Add Source") { placed = try $0.addSpeaker(modelId: m.id, position: p, onFloor: sub) }
        if let s = placed {
            lastSourcePlaced = s.id
            ctx.select([.speaker(s.id)])
        }
    }

    private func placeReceiver(at w: Vec3, solved: Bool) {
        var id: UInt64 = 0
        ctx.edit("Add Receiver") { doc in
            id = doc.addMicrophone(position: Point3(x: w.x, y: w.y, z: 1.2))
            if !solved, var m = doc.microphones().first(where: { $0.id == id }) {
                m.includeInSolve = false
                try doc.updateMicrophone(mic: m)
            }
        }
        if id != 0 { ctx.select([.microphone(id)]) }
    }

    /// Results: a probe microphone (not solved) on a plane or at ear height.
    private func placeResultsMic(at w: Vec3, on target: EntityRef?) {
        var p = Vec3(w.x, w.y, 1.2)
        if case .plane(let pid) = target, let plane = store.planesById[pid], plane.axis == .horizontal { p.z = plane.offset }
        var id: UInt64 = 0
        ctx.edit("Add Microphone") { doc in
            id = doc.addMicrophone(position: Point3(x: p.x, y: p.y, z: p.z))
            if var m = doc.microphones().first(where: { $0.id == id }) {
                m.includeInSolve = false
                try doc.updateMicrophone(mic: m)
            }
        }
        if id != 0 { ctx.select([.microphone(id)]) }
    }

    private func addCutPlane(on target: EntityRef?, world: Vec3?) {
        var axis = PlaneAxisRec.horizontal
        var offset = 1.2
        if case .surface(let k) = target, let s = store.surfacesByKey[k], let w = world {
            let n = s.normal
            if abs(n.x) > 0.9 { axis = .constX; offset = w.x } else if abs(n.y) > 0.9 { axis = .constY; offset = w.y } else { offset = 1.2 }
        }
        var id: UInt64 = 0
        let flags = NSEvent.modifierFlags
        if flags.contains(.shift), let w = world { axis = .constX; offset = w.x }
        ctx.edit("Add Cut Plane") { id = $0.addCutPlane(axis: axis, offset: offset, spacing: 0.2) }
        if id != 0 {
            ctx.select([.plane(id)])
            ctx.post("Cut plane added — solve to compute its field", .info)
        }
    }

    // MARK: Measure

    private func reportMeasure() {
        guard measure.count >= 2 else { return }
        let a = measure[measure.count - 2], b = measure[measure.count - 1]
        let d = b - a
        var total = 0.0
        for i in 1..<measure.count { total += simd_distance(measure[i - 1], measure[i]) }
        let ang = atan2(d.y, d.x) * 180 / .pi
        let text = String(format: "Distance %.3f m · Δx %.3f Δy %.3f Δz %.3f · ∠ %.1f°", simd_length(d), d.x, d.y, d.z, ang)
            + (measure.count > 2 ? String(format: " · path %.3f m", total) : "")
        ctx.commandEcho(text)
        ctx.post(text)
    }

    // MARK: Overlays

    func overlayLines(for ws: Workspace) -> [LineBatch] {
        guard ws == ctx.workspace else { return [] }
        var out: [LineBatch] = []
        let z = Float(ctx.planZ)
        let cursor = snap?.point ?? hoverWorld.map { SIMD2($0.x, $0.y) }
        let accent = rgba(0x4DA3FF)
        switch ctx.activeTool {
        case .line, .polyline:
            if let last = points.last, let c = cursor {
                out.append(LineBatch(segments: [Float(last.x), Float(last.y), z, Float(c.x), Float(c.y), z], color: accent, width: 1.5, dashed: true, overlay: true))
            }
        case .rectangle:
            if let a = points.first, let b = cursor {
                let pts: [SIMD2<Double>] = [a, SIMD2(b.x, a.y), b, SIMD2(a.x, b.y)]
                var seg: [Float] = []
                for i in 0..<4 { let p = pts[i], q = pts[(i + 1) % 4]; seg += [Float(p.x), Float(p.y), z, Float(q.x), Float(q.y), z] }
                out.append(LineBatch(segments: seg, color: accent, width: 1.5, dashed: true, overlay: true))
            }
        case .arc:
            if points.count == 2, let c = cursor, let pts = arcPoints(points[0], points[1], c) {
                var seg: [Float] = []
                for i in 0..<(pts.count - 1) { seg += [Float(pts[i].x), Float(pts[i].y), z, Float(pts[i + 1].x), Float(pts[i + 1].y), z] }
                out.append(LineBatch(segments: seg, color: accent, width: 1.5, dashed: true, overlay: true))
            } else if points.count == 1, let c = cursor {
                out.append(LineBatch(segments: [Float(points[0].x), Float(points[0].y), z, Float(c.x), Float(c.y), z], color: accent, width: 1, dashed: true, overlay: true))
            }
        case .move:
            if let a = points.first, let c = cursor {
                out.append(LineBatch(segments: [Float(a.x), Float(a.y), z, Float(c.x), Float(c.y), z], color: accent, width: 1, dashed: true, overlay: true))
            }
        case .measure:
            var seg: [Float] = []
            let pts = measure + (cursor.map { [Vec3($0.x, $0.y, measure.last?.z ?? 0)] } ?? [])
            if pts.count >= 2 { for i in 1..<pts.count { seg += [Float(pts[i - 1].x), Float(pts[i - 1].y), Float(pts[i - 1].z), Float(pts[i].x), Float(pts[i].y), Float(pts[i].z)] } }
            if !seg.isEmpty, !measure.isEmpty { out.append(LineBatch(segments: seg, color: rgba(0xF29D38), width: 1.2, overlay: true)) }
        case .offset:
            if let s = offsetSegment.flatMap({ store.segmentsById[$0] }), let c = cursor {
                let a = SIMD2(s.pa.x, s.pa.y), b = SIMD2(s.pb.x, s.pb.y)
                var n = simd_normalize(SIMD2(-(b - a).y, (b - a).x))
                if simd_dot(c - a, n) < 0 { n = -n }
                let d = abs(simd_dot(c - a, n))
                let (a2, b2) = (a + n * d, b + n * d)
                out.append(LineBatch(segments: [Float(a2.x), Float(a2.y), z, Float(b2.x), Float(b2.y), z], color: accent, width: 1.5, dashed: true, overlay: true))
            }
        case .aim:
            if let src = aimSource.flatMap({ store.speakersById[$0] }), let w = hoverWorld {
                let p = SIMD3<Float>(Float(src.position.x), Float(src.position.y), Float(src.position.z))
                out.append(LineBatch(segments: [p.x, p.y, p.z, Float(w.x), Float(w.y), 1.2], color: rgba(0xF29D38), width: 1, dashed: true, overlay: true))
            }
        case .probeLine:
            if let a = probe.first, let w = hoverWorld {
                out.append(LineBatch(segments: [Float(a.x), Float(a.y), Float(a.z), Float(w.x), Float(w.y), Float(w.z)], color: accent, width: 1.5, dashed: true, overlay: true))
            }
        default:
            break
        }
        if let (a, b) = ctx.probeLine, ws == .results {
            out.append(LineBatch(segments: [Float(a.x), Float(a.y), Float(a.z), Float(b.x), Float(b.y), Float(b.z)], color: accent, width: 1.5, overlay: true))
        }
        return out
    }

    func overlayMarkers(for ws: Workspace) -> [Marker] {
        guard ws == ctx.workspace, let s = snap, isPointTool else { return [] }
        let z = Float(ctx.planZ)
        let shape: MarkerShape = switch s.kind {
        case .endpoint: .hollowSquare
        case .midpoint: .triangle
        case .intersection, .grid: .cross
        case .perpendicular: .diamond
        case .nearest, .centre: .ring
        case nil: .cross
        }
        let color = s.kind == .grid ? rgba(0xF29D38) : rgba(0x4DA3FF)
        var m = [Marker(position: SIMD3(Float(s.point.x), Float(s.point.y), z), shape: shape, size: 8, color: color, fill: rgba(0x121417))]
        for p in points { m.append(Marker(position: SIMD3(Float(p.x), Float(p.y), z), shape: .dot, size: 5, color: rgba(0x4DA3FF))) }
        return m
    }

    func overlayLabels(for ws: Workspace) -> [WaveyViewport.Label] {
        guard ws == ctx.workspace, let s = snap, isPointTool else { return [] }
        let z = Float(ctx.planZ)
        var text = "\(s.kind?.rawValue ?? "PT") \(String(format: "%.3f, %.3f", s.point.x, s.point.y))"
        var secondary: String?
        if let last = points.last {
            let d = s.point - last
            if ctx.activeTool == .rectangle {
                secondary = String(format: "Δ %.3f × %.3f m", abs(d.x), abs(d.y))
            } else {
                secondary = String(format: "%.3f m ∠ %.1f°", simd_length(d), atan2(d.y, d.x) * 180 / .pi)
            }
        }
        if ctx.activeTool == .measure, let a = measure.last {
            secondary = String(format: "%.3f m", simd_distance(Vec3(s.point.x, s.point.y, a.z), a))
        }
        text = text.replacingOccurrences(of: "-", with: "\u{2212}")
        return [WaveyViewport.Label(id: "snap", anchor: SIMD3(Float(s.point.x), Float(s.point.y), z), text: text, secondary: secondary,
                                    color: s.kind == .grid ? rgba(0xF29D38) : rgba(0x6DB3FF), priority: 100, offset: CGSize(width: 70, height: 22))]
    }
}
