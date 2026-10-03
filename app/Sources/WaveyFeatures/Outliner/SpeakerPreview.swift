import simd
import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel
import WaveyPlots

/// Previews of library devices, loaded in the background and kept per file.
@MainActor @Observable
final class PreviewCache {
    static let shared = PreviewCache()
    private(set) var previews: [String: SpeakerPreviewRec] = [:]
    private var loading: Set<String> = []

    func forget(_ key: String) { previews[key] = nil }

    /// The preview of a library file or a project model (`model:<id>`), loading it if needed.
    func preview(path: String?, modelID: String?, doc: WaveyDocument) -> SpeakerPreviewRec? {
        let key = path ?? "model:\(modelID ?? "")"
        if let p = previews[key] { return p }
        guard !loading.contains(key) else { return nil }
        loading.insert(key)
        Task.detached(priority: .userInitiated) {
            let p: SpeakerPreviewRec? = if let path { try? libraryPreview(path: path) } else { modelID.flatMap { try? doc.modelPreview(modelId: $0) } }
            await MainActor.run {
                self.loading.remove(key)
                if let p { self.previews[key] = p }
            }
        }
        return nil
    }
}

/// A speaker at a glance: cabinet, figures, response and coverage.
struct SpeakerPreviewCard: View {
    let ctx: WindowContext
    let path: String?
    let modelID: String?
    @State private var turn = 35.0

    var body: some View {
        let p = PreviewCache.shared.preview(path: path, modelID: modelID, doc: ctx.store.doc)
        VStack(alignment: .leading, spacing: .wd.s6) {
            if let p {
                VStack(alignment: .leading, spacing: 2) {
                    Text(p.name).font(.wd.title).foregroundStyle(Color.wd.textPrimary)
                    Text(p.manufacturer.isEmpty ? "—" : p.manufacturer).font(.wd.detail).foregroundStyle(Color.wd.textTertiary)
                }
                Canvas { g, size in cabinet(&g, size, p) }
                    .frame(height: 130)
                    .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(Color.wd.viewportEdge))
                    .gesture(DragGesture().onChanged { v in turn = 35 + Double(v.translation.width) })
                    .help("Drag to turn")
                Grid(alignment: .leading, horizontalSpacing: .wd.s8, verticalSpacing: 3) {
                    row("Box", String(format: "%.2f × %.2f × %.2f m", p.widthM, p.heightM, p.depthM))
                    if let w = p.weightKg { row("Weight", String(format: "%.1f kg", w)) }
                    row("Range", "\(Int(p.freqsHz.first ?? 0))–\(Int(p.freqsHz.last ?? 0)) Hz"
                        + (p.nominalMinHz.map { lo in p.nominalMaxHz.map { " · nominal \(Int(lo))–\(Int($0))" } ?? "" } ?? ""))
                    row("Sensitivity", String(format: "%.1f dB @ ", p.sensitivityDb) + label(p.sensitivityHz) + (p.maxSplDb.map { String(format: " · max %.0f dB", $0) } ?? ""))
                    if p.hasBalloon {
                        row("Coverage", p.coverageH.allSatisfy { $0 >= 355 } && p.coverageV.allSatisfy { $0 >= 355 }
                            ? "omni at " + p.polarFreqsHz.map(label).joined(separator: " / ")
                            : p.polarFreqsHz.indices.map { i in
                                String(format: "%@ %.0f° × %.0f°", label(p.polarFreqsHz[i]), p.coverageH[i], p.coverageV[i])
                            }.joined(separator: "\n"))
                    }
                    if !p.rigging.isEmpty { row("Rigging", p.rigging) }
                }
                let band = p.plotMinHz...max(p.plotMaxHz, p.plotMinHz * 2), (x, y) = clip(p.freqsHz, p.onAxisDb, to: band)
                LinePlot(series: [PlotSeries(id: "r", name: "On axis", color: Color.wd.source, x: x, y: y)],
                         yAxis: .decibels, xRange: band)
                    .frame(height: 96)
                if p.hasBalloon {
                    PolarPlot(curves: zip(p.polarFreqsHz.indices, [Color.wd.curve250, Color.wd.source, Color.wd.curve4k]).map { i, c in
                        PolarPlot.Curve(id: "\(i)", label: "\(label(p.polarFreqsHz[i])) H", color: c, anglesDeg: p.polarAnglesDeg, db: p.polarH[i])
                    })
                    .frame(height: 170)
                }
            } else {
                HStack(spacing: .wd.s8) {
                    ProgressView().controlSize(.small)
                    Text("Loading…").font(.wd.detail).foregroundStyle(Color.wd.textTertiary)
                }
                .frame(width: 300, height: 80)
            }
        }
        .padding(.wd.s12)
        .frame(width: 330)
        .background(Color.wd.surfacePanel)
    }

    private func label(_ f: Double) -> String {
        f >= 1000 ? String(format: "%g kHz", (f / 100).rounded() / 10) : String(format: "%.0f Hz", f)
    }

    private func row(_ k: String, _ v: String) -> some View {
        GridRow {
            Text(k).font(.wd.detail).foregroundStyle(Color.wd.textTertiary)
            Text(v).font(.wd.numeric).foregroundStyle(Color.wd.textSecondary).lineLimit(3).fixedSize(horizontal: false, vertical: true)
        }
    }

    /// The cabinet outline in an isometric view, turned by `turn` degrees.
    private func cabinet(_ g: inout GraphicsContext, _ size: CGSize, _ p: SpeakerPreviewRec) {
        let e = p.edgePositions
        guard e.count >= 6 else { return }
        let a = turn * .pi / 180, el = 25.0 * .pi / 180
        func proj(_ e: [Float], _ i: Int) -> SIMD2<Double> {
            let x = Double(e[i]), y = Double(e[i + 1]), z = Double(e[i + 2])
            let xr = x * cos(a) - y * sin(a), yr = x * sin(a) + y * cos(a)
            return SIMD2(yr, z * cos(el) - xr * sin(el))
        }
        let pts = stride(from: 0, to: e.count - 2, by: 3).map { proj(e, $0) }
        let detail = stride(from: 0, to: p.detailPositions.count - 2, by: 3).map { proj(p.detailPositions, $0) }
        let all = pts + detail
        let lo = all.reduce(SIMD2(repeating: Double.infinity)) { pointwiseMin($0, $1) }
        let hi = all.reduce(SIMD2(repeating: -Double.infinity)) { pointwiseMax($0, $1) }
        let span = max(hi.x - lo.x, hi.y - lo.y, 1e-3)
        let s = (min(size.width, size.height) - 20) / span
        let mid = (lo + hi) / 2
        func path(_ pts: [SIMD2<Double>]) -> Path {
            var path = Path()
            for k in stride(from: 0, to: pts.count - 1, by: 2) {
                let a = pts[k], b = pts[k + 1]
                path.move(to: CGPoint(x: size.width / 2 + (a.x - mid.x) * s, y: size.height / 2 - (a.y - mid.y) * s))
                path.addLine(to: CGPoint(x: size.width / 2 + (b.x - mid.x) * s, y: size.height / 2 - (b.y - mid.y) * s))
            }
            return path
        }
        func screen(_ q: SIMD2<Double>) -> CGPoint {
            CGPoint(x: size.width / 2 + (q.x - mid.x) * s, y: size.height / 2 - (q.y - mid.y) * s)
        }
        // Solid models: faces and their edges painted back to front, faces
        // shaded by a key light. Others: the outline.
        let solid = !p.surfacePositions.isEmpty
        if solid {
            enum Item { case face([CGPoint], Double, Bool), edge(CGPoint, CGPoint) }
            var items: [(Double, Item)] = []
            func depth(_ c: SIMD3<Double>) -> Double {
                (c.x * cos(a) - c.y * sin(a)) * cos(el) + c.z * sin(el)
            }
            func world(_ buf: [Float], _ i: Int) -> SIMD3<Double> { SIMD3(Double(buf[i]), Double(buf[i + 1]), Double(buf[i + 2])) }
            let light = simd_normalize(SIMD3<Double>(0.35, -0.55, 0.75))
            for (buf, inset) in [(p.surfacePositions, false), (p.insetPositions, true)] {
                for t in stride(from: 0, to: buf.count - 8, by: 9) {
                    let w = (0..<3).map { world(buf, t + $0 * 3) }
                    let n = simd_normalize(simd_cross(w[1] - w[0], w[2] - w[0]))
                    guard n.x.isFinite else { continue }
                    let nr = SIMD3(n.x * cos(a) - n.y * sin(a), n.x * sin(a) + n.y * cos(a), n.z)
                    items.append((depth((w[0] + w[1] + w[2]) / 3) + (inset ? 1e-3 : 0),
                                  .face((0..<3).map { screen(proj(buf, t + $0 * 3)) }, abs(simd_dot(nr, light)), inset)))
                }
            }
            let d = p.detailPositions
            for k in stride(from: 0, to: d.count - 5, by: 6) {
                // A little in front of the faces it bounds.
                items.append((depth((world(d, k) + world(d, k + 3)) / 2) + 2e-3, .edge(screen(proj(d, k)), screen(proj(d, k + 3)))))
            }
            for (_, item) in items.sorted(by: { $0.0 < $1.0 }) {
                switch item {
                case let .face(pts, shade, inset):
                    var path = Path()
                    path.addLines(pts)
                    path.closeSubpath()
                    let v = (inset ? 0.12 : 0.36) * (0.55 + 0.45 * shade)
                    let col = Color(red: v, green: v * 1.03, blue: v * 1.1)
                    g.fill(path, with: .color(col))
                    g.stroke(path, with: .color(col), lineWidth: 0.6)
                case let .edge(p0, p1):
                    var path = Path()
                    path.move(to: p0)
                    path.addLine(to: p1)
                    g.stroke(path, with: .color(Color.white.opacity(0.3)), lineWidth: 0.6)
                }
            }
        }
        if !solid { g.stroke(path(pts), with: .color(Color.wd.source), lineWidth: 1) }
    }
}

extension WindowContext {
    /// The library preview card of a file (for snapshots).
    public func libraryPreviewView(path: String) -> AnyView {
        _ = PreviewCache.shared.preview(path: path, modelID: nil, doc: store.doc)
        return AnyView(SpeakerPreviewCard(ctx: self, path: path, modelID: nil).environment(\.wdStaticTables, true))
    }
}
