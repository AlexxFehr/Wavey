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
                    row("Sensitivity", String(format: "%.1f dB @ 1 kHz", p.sensitivity1kDb) + (p.maxSplDb.map { String(format: " · max %.0f dB", $0) } ?? ""))
                    if p.hasBalloon {
                        row("Coverage", String(format: "1k %.0f° × %.0f° · 4k %.0f° × %.0f°", p.coverage1k[0], p.coverage1k[1], p.coverage4k[0], p.coverage4k[1]))
                    }
                    if !p.rigging.isEmpty { row("Rigging", p.rigging) }
                }
                LinePlot(series: [PlotSeries(id: "r", name: "On axis", color: Color.wd.source, x: p.freqsHz, y: p.onAxisDb)],
                         yAxis: .decibels, xRange: max(p.freqsHz.first ?? 20, 10)...max(p.freqsHz.last ?? 20000, 100))
                    .frame(height: 96)
                if p.hasBalloon {
                    PolarPlot(curves: [
                        PolarPlot.Curve(id: "1kh", label: "1 kHz H", color: Color.wd.source, anglesDeg: p.polarAnglesDeg, db: p.polar1kH),
                        PolarPlot.Curve(id: "1kv", label: "1 kHz V", color: Color.wd.curve250, anglesDeg: p.polarAnglesDeg, db: p.polar1kV),
                        PolarPlot.Curve(id: "4kh", label: "4 kHz H", color: Color.wd.curve4k, anglesDeg: p.polarAnglesDeg, db: p.polar4kH),
                    ])
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

    private func row(_ k: String, _ v: String) -> some View {
        GridRow {
            Text(k).font(.wd.detail).foregroundStyle(Color.wd.textTertiary)
            Text(v).font(.wd.numeric).foregroundStyle(Color.wd.textSecondary).lineLimit(2).fixedSize(horizontal: false, vertical: true)
        }
    }

    /// The cabinet outline in an isometric view, turned by `turn` degrees.
    private func cabinet(_ g: inout GraphicsContext, _ size: CGSize, _ p: SpeakerPreviewRec) {
        let e = p.edgePositions
        guard e.count >= 6 else { return }
        let a = turn * .pi / 180, el = 25.0 * .pi / 180
        func proj(_ i: Int) -> SIMD2<Double> {
            let x = Double(e[i]), y = Double(e[i + 1]), z = Double(e[i + 2])
            let xr = x * cos(a) - y * sin(a), yr = x * sin(a) + y * cos(a)
            return SIMD2(yr, z * cos(el) - xr * sin(el))
        }
        var pts: [SIMD2<Double>] = []
        var i = 0
        while i + 2 < e.count { pts.append(proj(i)); i += 3 }
        let lo = pts.reduce(SIMD2(repeating: Double.infinity)) { pointwiseMin($0, $1) }
        let hi = pts.reduce(SIMD2(repeating: -Double.infinity)) { pointwiseMax($0, $1) }
        let span = max(hi.x - lo.x, hi.y - lo.y, 1e-3)
        let s = (min(size.width, size.height) - 20) / span
        let mid = (lo + hi) / 2
        var path = Path()
        for k in stride(from: 0, to: pts.count - 1, by: 2) {
            let a = pts[k], b = pts[k + 1]
            path.move(to: CGPoint(x: size.width / 2 + (a.x - mid.x) * s, y: size.height / 2 - (a.y - mid.y) * s))
            path.addLine(to: CGPoint(x: size.width / 2 + (b.x - mid.x) * s, y: size.height / 2 - (b.y - mid.y) * s))
        }
        g.stroke(path, with: .color(Color.wd.source), lineWidth: 1)
    }
}

extension WindowContext {
    /// The library preview card of a file (for snapshots).
    public func libraryPreviewView(path: String) -> AnyView {
        _ = PreviewCache.shared.preview(path: path, modelID: nil, doc: store.doc)
        return AnyView(SpeakerPreviewCard(ctx: self, path: path, modelID: nil).environment(\.wdStaticTables, true))
    }
}
