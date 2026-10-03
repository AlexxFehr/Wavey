import simd
import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel

/// Side or top drawing of an assembly: cabinet wireframes at their laid-out
/// poses, the rigging frame and pick point, and (line arrays, side view)
/// each box's aim ray down to the audience line. Coordinates are relative
/// to the assembly origin with its yaw removed (x forward, z up).
struct AssemblyPreview: View {
    let model: AssemblyEditorModel
    let top: Bool

    var body: some View {
        GeometryReader { geo in
            Canvas { g, size in draw(&g, size) }
                .contentShape(Rectangle())
                .gesture(SpatialTapGesture().onEnded { tap($0.location, geo.size) })
        }
        .background(LinearGradient(colors: [Color.wd.viewportCentre, Color.wd.viewportEdge], startPoint: .top, endPoint: .bottom))
        .overlay(alignment: .topLeading) { legend }
    }

    /// Aim rays (line arrays, side view) and the audience line, in local coordinates.
    private func guides() -> (rays: [(SIMD2<Double>, SIMD2<Double>, Int)], audience: [SIMD2<Double>]) {
        guard model.kind == .lineArray, !top else { return ([], []) }
        let zLocal = model.listenerZ - model.assembly.origin.z
        var rays: [(SIMD2<Double>, SIMD2<Double>, Int)] = []
        for (i, (c, a)) in axes().enumerated() {
            let lc = local(c), la = local(c + a) - lc
            guard la.y < -1e-3 else { continue }
            let t = (zLocal - lc.y) / la.y
            rays.append((lc, lc + la * min(t, 80), i))
        }
        let audience = [SIMD2(model.audienceFirst, zLocal),
                        SIMD2(model.audienceLast, zLocal + (model.audienceLast - model.audienceFirst) * tan(model.rakeDeg * .pi / 180))]
        return (rays, audience)
    }

    // MARK: Geometry

    /// Assembly-local 2D coordinates of a room point.
    private func local(_ p: SIMD3<Double>) -> SIMD2<Double> {
        let o = model.assembly.origin
        let d = p - SIMD3(o.x, o.y, o.z)
        let t = -model.assembly.yawDeg * .pi / 180
        let x = d.x * cos(t) - d.y * sin(t), y = d.x * sin(t) + d.y * cos(t)
        return top ? SIMD2(x, y) : SIMD2(x, d.z)
    }

    private func rotation(_ e: ElementPoseRec) -> simd_double3x3 {
        let (y, p, r) = (e.yawDeg * .pi / 180, -e.pitchDeg * .pi / 180, e.rollDeg * .pi / 180)
        let rz = simd_double3x3(rows: [SIMD3(cos(y), -sin(y), 0), SIMD3(sin(y), cos(y), 0), SIMD3(0, 0, 1)])
        let ry = simd_double3x3(rows: [SIMD3(cos(p), 0, sin(p)), SIMD3(0, 1, 0), SIMD3(-sin(p), 0, cos(p))])
        let rx = simd_double3x3(rows: [SIMD3(1, 0, 0), SIMD3(0, cos(r), -sin(r)), SIMD3(0, sin(r), cos(r))])
        return rz * ry * rx
    }

    /// Edge segments (room coordinates) of every element, by element index.
    private func elementEdges() -> [(Int, [(SIMD3<Double>, SIMD3<Double>)])] {
        guard let lay = model.layout else { return [] }
        return lay.elements.enumerated().map { i, e in
            let pos = SIMD3(e.position.x, e.position.y, e.position.z), rot = rotation(e)
            let ep = model.store.cabinetMeshes[e.modelId]?.edgePositions ?? []
            var segs: [(SIMD3<Double>, SIMD3<Double>)] = []
            var k = 0
            while k + 5 < ep.count {
                let a = SIMD3(Double(ep[k]), Double(ep[k + 1]), Double(ep[k + 2])), b = SIMD3(Double(ep[k + 3]), Double(ep[k + 4]), Double(ep[k + 5]))
                segs.append((pos + rot * a, pos + rot * b))
                k += 6
            }
            return (i, segs)
        }
    }

    private func frameEdges() -> [(SIMD3<Double>, SIMD3<Double>)] {
        guard let f = model.layout?.frame, let rf = model.rigging?.frames.first(where: { $0.key == f.key }) else { return [] }
        let o = SIMD3(f.reference.x, f.reference.y, f.reference.z)
        let ax = SIMD3(f.axisX.x, f.axisX.y, f.axisX.z), ay = SIMD3(f.axisY.x, f.axisY.y, f.axisY.z), az = SIMD3(f.axisZ.x, f.axisZ.y, f.axisZ.z)
        let w = { (i: Int) in o + ax * Double(rf.edgePositions[i]) + ay * Double(rf.edgePositions[i + 1]) + az * Double(rf.edgePositions[i + 2]) }
        var segs: [(SIMD3<Double>, SIMD3<Double>)] = []
        var k = 0
        while k + 5 < rf.edgePositions.count { segs.append((w(k), w(k + 3))); k += 6 }
        return segs
    }

    /// Acoustic centre and aim axis of each element (room coordinates).
    private func axes() -> [(SIMD3<Double>, SIMD3<Double>)] {
        guard let lay = model.layout else { return [] }
        return lay.elements.map { e in
            let rot = rotation(e)
            let ac = model.store.cabinetMeshes[e.modelId]?.acousticCenter
            let c = SIMD3(e.position.x, e.position.y, e.position.z) + rot * SIMD3(ac?.x ?? 0, ac?.y ?? 0, ac?.z ?? 0)
            return (c, rot * SIMD3(1, 0, 0))
        }
    }

    private struct Fit {
        let scale: Double
        let offset: CGPoint
        let size: CGSize
        func pt(_ p: SIMD2<Double>) -> CGPoint { CGPoint(x: offset.x + p.x * scale, y: offset.y - p.y * scale) }
        func world(_ q: CGPoint) -> SIMD2<Double> { SIMD2((q.x - offset.x) / scale, (offset.y - q.y) / scale) }
    }

    private func fit(_ size: CGSize, include extra: [SIMD2<Double>]) -> Fit {
        var pts: [SIMD2<Double>] = extra
        pts.append(.zero)
        for (_, segs) in elementEdges() {
            for (a, b) in segs { pts.append(local(a)); pts.append(local(b)) }
        }
        for (a, b) in frameEdges() { pts.append(local(a)); pts.append(local(b)) }
        var lo = SIMD2<Double>(repeating: .infinity), hi = SIMD2<Double>(repeating: -.infinity)
        for p in pts { lo = pointwiseMin(lo, p); hi = pointwiseMax(hi, p) }
        let span: SIMD2<Double> = pointwiseMax(hi - lo, SIMD2(0.5, 0.5))
        let pad = 28.0
        let w = Double(size.width), h = Double(size.height)
        let s: Double = min((w - 2 * pad) / span.x, (h - 2 * pad) / span.y)
        let mid: SIMD2<Double> = (lo + hi) / 2
        return Fit(scale: s, offset: CGPoint(x: w / 2 - mid.x * s, y: h / 2 + mid.y * s), size: size)
    }

    // MARK: Drawing

    private func draw(_ g: inout GraphicsContext, _ size: CGSize) {
        let isLine = model.kind == .lineArray
        let (rays, audience) = guides()
        let f = fit(size, include: model.showCoverage ? audience + rays.map { $0.1 } : [])
        drawGrid(&g, f)
        // Floor (stacks, side view) and audience line.
        if !top && !isLine {
            let y = f.pt(SIMD2(0, -model.assembly.origin.z)).y
            g.stroke(Path { $0.move(to: CGPoint(x: 0, y: y)); $0.addLine(to: CGPoint(x: size.width, y: y)) }, with: .color(Color.wd.planOutline.opacity(0.5)), lineWidth: 1)
        }
        if audience.count == 2 {
            let p = Path { $0.move(to: f.pt(audience[0])); $0.addLine(to: f.pt(audience[1])) }
            g.stroke(p, with: .color(Color.wd.receiver.opacity(0.8)), lineWidth: 2)
        }
        for (a, b, i) in rays {
            let sel = model.selectedRows.contains(i)
            g.stroke(Path { $0.move(to: f.pt(a)); $0.addLine(to: f.pt(b)) },
                     with: .color((sel ? Color.wd.accent : Color.wd.source).opacity(sel ? 0.9 : 0.35)), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
        }
        for (a, b) in frameEdges() {
            g.stroke(Path { $0.move(to: f.pt(local(a))); $0.addLine(to: f.pt(local(b))) }, with: .color(Color.wd.textTertiary), lineWidth: 1)
        }
        for (i, segs) in elementEdges() {
            let sel = model.selectedRows.contains(i)
            var p = Path()
            for (a, b) in segs { p.move(to: f.pt(local(a))); p.addLine(to: f.pt(local(b))) }
            g.stroke(p, with: .color(sel ? Color.wd.accent : Color.wd.source), lineWidth: sel ? 1.6 : 1)
        }
        // Aim axes (short), acoustic centres and element numbers.
        for (i, (c, a)) in axes().enumerated() {
            let lc = f.pt(local(c)), tip = f.pt(local(c + a * 0.35))
            g.stroke(Path { $0.move(to: lc); $0.addLine(to: tip) }, with: .color(Color.wd.source.opacity(0.9)), lineWidth: 1.2)
            g.fill(Path(ellipseIn: CGRect(x: lc.x - 2, y: lc.y - 2, width: 4, height: 4)), with: .color(Color.wd.source))
            if model.elementCount <= 32 {
                g.draw(Text("\(i + 1)").font(.wd.chip).foregroundColor(Color.wd.textTertiary), at: CGPoint(x: lc.x - 10, y: lc.y), anchor: .trailing)
            }
        }
        // Origin (pick point / stack origin).
        let o = f.pt(.zero)
        g.stroke(Path { $0.move(to: CGPoint(x: o.x - 6, y: o.y)); $0.addLine(to: CGPoint(x: o.x + 6, y: o.y)); $0.move(to: CGPoint(x: o.x, y: o.y - 6)); $0.addLine(to: CGPoint(x: o.x, y: o.y + 6)) },
                 with: .color(Color.wd.accentText), lineWidth: 1.2)
    }

    private func drawGrid(_ g: inout GraphicsContext, _ f: Fit) {
        let lo = f.world(CGPoint(x: 0, y: f.size.height)), hi = f.world(CGPoint(x: f.size.width, y: 0))
        let span = max(hi.x - lo.x, hi.y - lo.y)
        let step = [0.1, 0.25, 0.5, 1, 2, 5, 10, 20].first { span / $0 <= 14 } ?? 50
        var p = Path()
        var x = (lo.x / step).rounded(.down) * step
        while x <= hi.x { p.move(to: f.pt(SIMD2(x, lo.y))); p.addLine(to: f.pt(SIMD2(x, hi.y))); x += step }
        var y = (lo.y / step).rounded(.down) * step
        while y <= hi.y { p.move(to: f.pt(SIMD2(lo.x, y))); p.addLine(to: f.pt(SIMD2(hi.x, y))); y += step }
        g.stroke(p, with: .color(Color.wd.gridMajor), lineWidth: 1)
        g.draw(Text(step >= 1 ? "grid \(Int(step)) m" : String(format: "grid %g m", step)).font(.wd.chip).foregroundColor(Color.wd.textTertiary),
               at: CGPoint(x: f.size.width - 10, y: f.size.height - 10), anchor: .bottomTrailing)
    }

    private var legend: some View {
        HStack(spacing: .wd.s8) {
            Text(top ? "TOP" : "SIDE").font(.wd.chip).foregroundStyle(Color.wd.textTertiary)
            if let l = model.layout, model.kind == .lineArray {
                Text(String(format: "frame %+.1f° · top box %+.1f° · bottom %+.1f°", l.frameTiltDeg + 0, 0 - (l.boxTiltDeg.first ?? 0), 0 - (l.boxTiltDeg.last ?? 0)))
                    .font(.wd.chip).foregroundStyle(Color.wd.textSecondary)
            }
        }
        .padding(.wd.s8)
    }

    private func tap(_ q: CGPoint, _ size: CGSize) {
        // Select the element whose acoustic centre is nearest the tap.
        let (rays, audience) = guides()
        let f = fit(size, include: model.showCoverage ? audience + rays.map { $0.1 } : [])
        let pts = axes().map { f.pt(local($0.0)) }
        if let (i, d) = pts.enumerated().map({ ($0.offset, hypot($0.element.x - q.x, $0.element.y - q.y)) }).min(by: { $0.1 < $1.1 }), d < 30 {
            model.selectedRows = [i]
        } else {
            model.selectedRows = []
        }
    }
}
