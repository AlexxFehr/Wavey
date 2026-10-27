import SwiftUI
import WaveyDesign
import WaveyModel
import WaveyViewport

/// Viewport header, the Metal viewport with its overlays, and the dock.
struct ViewportRegion: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        VStack(spacing: 0) {
            ViewportHeader(ctx: ctx)
            ZStack {
                ViewportView(ctx.viewport)
                    .id(ctx.workspace)
                    .onDrop(of: [.plainText, .utf8PlainText, .text], delegate: DeviceDropDelegate(ctx: ctx))
                ViewportOverlays(ctx: ctx)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

/// Library drags over the viewport: a ghost of the device follows the
/// floor point under the cursor; the drop places it.
struct DeviceDropDelegate: DropDelegate {
    let ctx: WindowContext

    func dropEntered(info: DropInfo) { dropUpdated(info: info).map { _ in } }

    func dropUpdated(info: DropInfo) -> DropProposal? {
        if let hit = ctx.viewport.projection.onPlane(info.location, z: 0) {
            var p = hit
            if ctx.state.flags.contains("snap.grid"), ctx.gridStep > 0 {
                p = SIMD3((hit.x / ctx.gridStep).rounded() * ctx.gridStep, (hit.y / ctx.gridStep).rounded() * ctx.gridStep, 0)
            }
            ctx.dropGhost = p
            ctx.rebuildScenes()
        }
        return DropProposal(operation: .copy)
    }

    func dropExited(info: DropInfo) {
        ctx.dropGhost = nil
        ctx.rebuildScenes()
    }

    func performDrop(info: DropInfo) -> Bool {
        let location = info.location
        let providers = info.itemProviders(for: [.plainText, .utf8PlainText, .text])
        ctx.dropGhost = nil
        guard let p = providers.first else { return false }
        _ = p.loadObject(ofClass: NSString.self) { obj, _ in
            guard let s = obj as? NSString else { return }
            let text = String(s)
            Task { @MainActor in
                _ = ctx.dropDevices([text], at: location)
                ctx.dragDevice = nil
                ctx.rebuildScenes()
            }
        }
        return true
    }
}

// MARK: Header

struct ViewportHeader: View {
    @Bindable var ctx: WindowContext
    @State private var levelPicker = false
    @State private var gridEdit = false

    var body: some View {
        RegionHeader {
            switch ctx.workspace {
            case .geometry: geometry
            case .sources: sources
            case .results: results
            }
        } trailing: {
            HStack(spacing: .wd.s6) {
                switch ctx.workspace {
                case .geometry:
                    Button { gridEdit.toggle() } label: {
                        Text(String(format: "Grid %.2f m", ctx.gridStep)).font(.wd.chip).foregroundStyle(Color.wd.textTertiary)
                    }
                    .buttonStyle(.plain)
                    .popover(isPresented: $gridEdit) {
                        WDPropertyRow("Grid") {
                            WDNumberField(ctx.gridStep, UnitFormatter(.length, decimals: 2), range: 0.01...10) { ctx.gridStep = $0; ctx.rebuildScenes() }
                        }
                        .frame(width: 200).padding(.vertical, .wd.s6)
                    }
                case .sources:
                    EmptyView()
                case .results:
                    WDIconButton(.image, size: .small, help: "Export image of view") { ctx.exportViewImage() }
                }
                WDIconButton(.fit, size: .small, help: "Zoom to fit (Home)") { ctx.viewport.zoomToFit() }
            }
        }
    }

    private func modeChip(_ title: String, _ icon: WDIconName?, _ mode: ViewMode) -> some View {
        WDChip(title, icon: icon, isOn: ctx.viewport.mode == mode) {
            ctx.viewport.mode = mode
            ctx.state.viewMode = mode
            ctx.rebuildScenes()
            if mode == .plan { ctx.viewport.zoomToFit() }
        }
    }

    private func flagChip(_ title: String, _ flag: String) -> some View {
        WDChip(title, isOn: ctx.flag(flag)) { ctx.toggleFlag(flag) }
    }

    @ViewBuilder private var geometry: some View {
        let level = ctx.store.levels.first { $0.id == ctx.planLevel }
        WDChip("Plan · \(level?.name ?? "L0")", icon: .topView, isOn: ctx.viewport.mode == .plan) {
            if ctx.viewport.mode == .plan { levelPicker.toggle() } else {
                ctx.viewport.mode = .plan
                ctx.state.viewMode = .plan
                ctx.rebuildScenes()
                ctx.viewport.zoomToFit()
            }
        }
        .popover(isPresented: $levelPicker) {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(ctx.store.levels, id: \.id) { l in
                    Button {
                        ctx.planLevel = l.id
                        levelPicker = false
                        ctx.rebuildScenes()
                    } label: {
                        HStack {
                            Text(l.name).font(.wd.body).foregroundStyle(l.id == ctx.planLevel ? Color.wd.accentText : Color.wd.textSecondary)
                            Spacer()
                            Text(UnitFormatter(.length).string(l.z)).font(.wd.meta).foregroundStyle(Color.wd.textTertiary)
                        }
                        .frame(height: .wd.row).contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.wd.s8).frame(width: 200)
        }
        modeChip("3D", .cube, .perspective)
        WDHairline().padding(.horizontal, .wd.s4)
        HeaderLabel("Snap")
        flagChip("END", "snap.end")
        flagChip("MID", "snap.mid")
        flagChip("INT", "snap.int")
        flagChip("GRID", "snap.grid")
        flagChip("PERP", "snap.perp")
        WDHairline().padding(.horizontal, .wd.s4)
        flagChip("ORTHO", "ortho")
        flagChip("POLAR 15°", "polar")
    }

    @ViewBuilder private var sources: some View {
        modeChip("Persp", .cube, .perspective)
        modeChip("Top", nil, .top)
        modeChip("Front", nil, .front)
        modeChip("Side", nil, .side)
        WDHairline().padding(.horizontal, .wd.s4)
        HeaderLabel("Show")
        flagChip("Directivity", "show.directivity")
        flagChip("Aim lines", "show.aim")
        flagChip("Receivers", "show.receivers")
        // Shown unless hidden, so earlier documents keep their labels.
        WDChip("Labels", isOn: !ctx.flag("hide.labels")) { ctx.toggleFlag("hide.labels") }
        flagChip("Surfaces α", "show.surfaces")
    }

    @ViewBuilder private var results: some View {
        modeChip("Persp", .cube, .perspective)
        modeChip("Top", nil, .top)
        modeChip("Front", nil, .front)
        WDHairline().padding(.horizontal, .wd.s4)
        HeaderLabel("Quantity")
        WDChip("SPL", isOn: true) {}
        WDHairline().padding(.horizontal, .wd.s4)
        flagChip("Wire", "show.wire")
        flagChip("Surfaces", "show.surfaces")
        flagChip("Sources", "show.sources")
        flagChip("Cells", "cells")
            .onChange(of: ctx.flag("cells")) { _, v in ctx.results.cells = v; ctx.rebuildScenes() }
    }
}

// MARK: Overlays

struct ViewportOverlays: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        ZStack {
            // Top-left
            VStack(alignment: .leading, spacing: .wd.s8) {
                if ctx.workspace == .results { ReadoutCard(ctx: ctx) }
                Spacer()
            }
            .padding(.wd.s10)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .allowsHitTesting(false)

            // Top-right
            VStack(alignment: .trailing, spacing: .wd.s8) {
                if ctx.workspace == .sources, !ctx.selection.speakers.isEmpty || ctx.selection.ids.contains(where: { $0.groupID != nil }) {
                    BalloonCard(ctx: ctx)
                }
                Group {
                    if ctx.workspace == .sources { OverlayKey(ctx: ctx) }
                    if ctx.workspace == .results { ColorBarOverlay(ctx: ctx) }
                }
                .allowsHitTesting(false)
                Spacer()
            }
            .padding(.wd.s10)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)

            // Bottom-left: axis gizmo (and scale bar in plan).
            HStack(alignment: .bottom, spacing: .wd.s12) {
                AxisGizmo(ctx: ctx)
                if ctx.viewport.mode == .plan { ScaleBar(ctx: ctx) }
            }
            .padding(.leading, .wd.s12)
            .padding(.bottom, .wd.s12)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)

            // Bottom-centre: typed input (lengths, coordinates) while a tool runs.
            if ctx.typedInputVisible {
                TypedInputBox(ctx: ctx)
                    .padding(.bottom, .wd.s12)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
            }

            // Bottom-right: inset preview (Geometry).
            if ctx.workspace == .geometry, ctx.flag("inset") {
                InsetPreview(ctx: ctx)
                    .padding(.wd.s10)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
            }

            // Empty states.
            if ctx.workspace == .results, ctx.store.runs.isEmpty {
                WDEmptyState("No results yet", detail: "Solve to compute the field; cut planes and microphones can be added now.",
                             actionTitle: "Solve ⌘R") { ctx.runs.solve() }
                    .wdOverlayCard()
            } else if ctx.store.room == nil, ctx.store.sketch.segments.isEmpty, ctx.workspace == .geometry {
                WDEmptyState("Empty model", detail: "Draw an outline with Rectangle (R) or Line (L), then Extrude (E) it.")
                    .allowsHitTesting(false)
            }

            // Marquee.
            if let r = ctx.marquee {
                Rectangle()
                    .fill(Color.wd.accent.opacity(0.06))
                    .overlay(Rectangle().strokeBorder(Color.wd.accent, style: StrokeStyle(lineWidth: 1, dash: ctx.marqueeCrossing ? [4, 3] : [])))
                    .frame(width: r.width, height: r.height)
                    .position(x: r.midX, y: r.midY)
                    .allowsHitTesting(false)
            }

            // Microphone line / grid popover.
            if let r = ctx.micArrayPopover {
                MicArrayPopover(ctx: ctx, request: r).id(r.at.x + r.at.y)
                    .position(x: r.at.x + 140, y: r.at.y + 70)
            }

            // Receiver grid popover.
            if let pt = ctx.receiverGridPopover {
                ReceiverGridPopover(ctx: ctx)
                    .position(x: pt.x + 140, y: pt.y + 70)
            }

            // Transient hint.
            Text(ctx.tool.prompt)
                .font(.wd.status)
                .foregroundStyle(Color.wd.textSecondary)
                .padding(.horizontal, .wd.s8)
                .padding(.vertical, .wd.s4)
                .wdOverlayCard()
                .padding(.bottom, .wd.s10)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
                .opacity(ctx.activeTool == .select ? 0 : 1)
                .allowsHitTesting(false)
        }
    }
}

struct AxisGizmo: View {
    @Bindable var ctx: WindowContext

    /// Click a letter: look along that axis (X → side, Y → front, Z → top).
    private func snapView(_ axis: String) {
        let mode: ViewMode = axis == "X" ? .side : axis == "Y" ? .front : .top
        ctx.viewport.mode = mode
        ctx.state.viewMode = mode
        ctx.rebuildScenes()
    }

    var body: some View {
        let p = ctx.viewport.projection
        let o = SIMD3<Double>(0, 0, 0)
        func dir(_ a: SIMD3<Double>) -> CGVector {
            guard let s0 = p.project(ctx.viewport.orbit.target + o), let s1 = p.project(ctx.viewport.orbit.target + a * ctx.viewport.orbit.distance * 0.05) else {
                return .zero
            }
            let v = CGVector(dx: s1.x - s0.x, dy: s1.y - s0.y)
            let l = max(hypot(v.dx, v.dy), 1e-6)
            return CGVector(dx: v.dx / l * 28, dy: v.dy / l * 28)
        }
        let plan = ctx.viewport.mode == .plan
        let axes: [(String, CGVector, Color)] = plan
            ? [("X", CGVector(dx: 28, dy: 0), Color.wd.axisX), ("Y", CGVector(dx: 0, dy: -28), Color.wd.axisY)]
            : [("X", dir([1, 0, 0]), Color.wd.axisX), ("Y", dir([0, 1, 0]), Color.wd.axisY), ("Z", dir([0, 0, 1]), Color.wd.axisZ)]
        return Canvas { g, size in
            let c = CGPoint(x: 20, y: size.height - 20)
            for (name, v, color) in axes {
                var path = Path()
                path.move(to: c)
                path.addLine(to: CGPoint(x: c.x + v.dx, y: c.y + v.dy))
                g.stroke(path, with: .color(color), lineWidth: 1.4)
                g.draw(Text(name).font(.wd.meta).foregroundColor(color), at: CGPoint(x: c.x + v.dx * 1.3, y: c.y + v.dy * 1.3))
            }
            g.fill(Path(ellipseIn: CGRect(x: c.x - 2, y: c.y - 2, width: 4, height: 4)), with: .color(Color.wd.textTertiary))
        }
        .frame(width: 64, height: 64)
        .overlay {
            if !plan {
                ForEach(axes, id: \.0) { name, v, _ in
                    Color.clear.frame(width: 14, height: 14).contentShape(Rectangle())
                        .position(x: 20 + v.dx * 1.3, y: 44 + v.dy * 1.3)
                        .onTapGesture { snapView(name) }
                        .help("View along \(name)")
                }
            }
        }
    }
}

struct ScaleBar: View {
    @Bindable var ctx: WindowContext
    var body: some View {
        let ppm = ctx.viewport.plan.pointsPerMetre
        let target = 60 / ppm
        let nice = [0.1, 0.2, 0.5, 1, 2, 5, 10, 20, 50, 100].first { $0 >= target } ?? 100
        let w = nice * ppm
        return HStack(spacing: .wd.s6) {
            Canvas { g, size in
                var p = Path()
                p.move(to: CGPoint(x: 0, y: size.height / 2)); p.addLine(to: CGPoint(x: size.width, y: size.height / 2))
                p.move(to: CGPoint(x: 0, y: 2)); p.addLine(to: CGPoint(x: 0, y: size.height - 2))
                p.move(to: CGPoint(x: size.width, y: 2)); p.addLine(to: CGPoint(x: size.width, y: size.height - 2))
                p.move(to: CGPoint(x: size.width / 2, y: 4)); p.addLine(to: CGPoint(x: size.width / 2, y: size.height - 4))
                g.stroke(p, with: .color(Color.wd.textTertiary), lineWidth: 1)
            }
            .frame(width: CGFloat(w), height: 10)
            Text(nice >= 1 ? "\(Int(nice)) m" : String(format: "%.1f m", nice)).font(.wd.meta).foregroundStyle(Color.wd.textTertiary)
        }
        .padding(.bottom, 8)
        .allowsHitTesting(false)
    }
}

/// 240 × 168 inset 3D preview with a title strip.
struct InsetPreview: View {
    @Bindable var ctx: WindowContext
    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("3D · ISO").font(.wd.meta).foregroundStyle(Color.wd.textTertiary)
                Spacer()
                Text(ctx.selection.primary.map { ctx.store.code($0) } ?? "").font(.wd.meta).foregroundStyle(Color.wd.accentText)
            }
            .padding(.horizontal, .wd.s8)
            .frame(height: 22)
            ViewportView(ctx.inset)
        }
        .frame(width: 240, height: 168)
        .wdOverlayCard()
        .clipShape(RoundedRectangle(cornerRadius: .wd.radiusSegment))
    }
}

/// Sources: what the overlay draws.
struct OverlayKey: View {
    @Bindable var ctx: WindowContext
    var body: some View {
        VStack(alignment: .leading, spacing: .wd.s4) {
            Text("Overlay").font(.wd.meta).foregroundStyle(Color.wd.textTertiary)
            HStack(spacing: .wd.s6) {
                RoundedRectangle(cornerRadius: 2).fill(Color.wd.source.opacity(0.6)).frame(width: 12, height: 8)
                Text("−6 dB contour @ \(ctx.balloonFrequency >= 1000 ? "\(Int(ctx.balloonFrequency / 1000)) kHz" : "\(Int(ctx.balloonFrequency)) Hz")")
            }
            HStack(spacing: .wd.s6) {
                Rectangle().fill(Color.wd.source).frame(width: 12, height: 1)
                Text("Aim axis")
            }
            HStack(spacing: .wd.s6) {
                Circle().strokeBorder(Color.wd.receiver, lineWidth: 1.2).frame(width: 8, height: 8).padding(.horizontal, 2)
                Text("Receivers")
            }
        }
        .font(.wd.meta)
        .foregroundStyle(Color.wd.textSecondary)
        .padding(.wd.s10)
        .wdOverlayCard()
    }
}

/// Balloon frequency of the selected speakers (a slider snapping to the
/// 1/3-octave centres; balloons are cached per frequency).
struct BalloonCard: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        HStack(spacing: .wd.s8) {
            Text("Balloon").font(.wd.meta).foregroundStyle(Color.wd.textTertiary)
            WDSlider(value: ctx.balloonFrequency, range: 31.5...16000, logarithmic: true) { v, _ in
                let f = thirdOctaveCentres.min { abs(log($0 / v)) < abs(log($1 / v)) } ?? v
                guard f != ctx.balloonFrequency else { return }
                ctx.balloonFrequency = f
                ctx.rebuildScenes()
            }
            .frame(width: 130)
            Text(ctx.balloonFrequency >= 1000 ? String(format: "%g kHz", ctx.balloonFrequency / 1000) : String(format: "%g Hz", ctx.balloonFrequency))
                .font(.wd.numeric).foregroundStyle(Color.wd.textSecondary)
                .frame(width: 56, alignment: .leading)
        }
        .padding(.horizontal, .wd.s10)
        .frame(height: .wd.button)
        .wdOverlayCard()
        .help("Frequency of the balloons and coverage contours")
    }
}

/// The 1/3-octave band centres (nominal), 31.5 Hz – 16 kHz.
let thirdOctaveCentres: [Double] = [31.5, 40, 50, 63, 80, 100, 125, 160, 200, 250, 315, 400, 500, 630, 800, 1000, 1250, 1600, 2000, 2500, 3150, 4000, 5000, 6300, 8000, 10000, 12500, 16000]

/// A small value box for typed input: opened by typing a number in the
/// view, Enter applies it to the active tool, Esc closes.
struct TypedInputBox: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        HStack(spacing: .wd.s8) {
            Text(ctx.tool.prompt).font(.wd.numeric).foregroundStyle(Color.wd.accentText).lineLimit(1)
            CommandField(ctx: ctx).frame(width: 160)
            Text("Enter ↵ · Esc").font(.wd.meta).foregroundStyle(Color.wd.textTertiary).fixedSize()
        }
        .padding(.horizontal, .wd.s10)
        .frame(height: .wd.button)
        .wdOverlayCard()
        .frame(maxWidth: 520)
    }
}

/// Results: f · λ · max · min of the visible field.
struct ReadoutCard: View {
    @Bindable var ctx: WindowContext
    var body: some View {
        let f = ctx.results.frequency
        let c = ctx.store.medium.speedOfSound
        let (lo, hi) = visibleRange
        return HStack(spacing: 14) {
            HStack(spacing: .wd.s4) {
                Text("f").font(.wd.meta).foregroundStyle(Color.wd.textTertiary)
                Text(String(format: "%.1f", f)).font(.wd.readout).foregroundStyle(Color.wd.textPrimary)
                Text("Hz").font(.wd.meta).foregroundStyle(Color.wd.textTertiary)
            }
            WDStatusItem("λ", String(format: "%.2f m", c / f))
            if let lo, let hi {
                let unit = ctx.results.quantity == .spl ? "dB" : "s"
                WDStatusItem("max", String(format: "%.1f", hi))
                WDStatusItem("min", String(format: "%.1f %@", lo, unit))
            }
        }
        .padding(.horizontal, .wd.s10)
        .padding(.vertical, .wd.s6)
        .wdOverlayCard()
    }

    private var visibleRange: (Float?, Float?) {
        var lo: Float?, hi: Float?
        for p in ctx.store.cutPlanes where p.style.visible {
            guard let v = ctx.fieldValues(p) else { continue }
            for x in v.values where x.isFinite {
                lo = min(lo ?? x, x)
                hi = max(hi ?? x, x)
            }
        }
        return (lo, hi)
    }
}

/// Colour bar of the selected (or first visible) plane's style.
struct ColorBarOverlay: View {
    @Bindable var ctx: WindowContext
    var body: some View {
        let plane = ctx.selection.planes.first.flatMap { ctx.store.planesById[$0] } ?? ctx.store.cutPlanes.first { $0.style.visible }
        if let plane, let field = ctx.fieldValues(plane) {
            let map = ColorMap(rawValue: plane.style.colorMap) ?? .inferno
            VStack(alignment: .trailing, spacing: .wd.s6) {
                Text(field.unit == "dB" ? "SPL dB" : "\(ctx.results.quantity.title) s").font(.wd.ui(10, .semibold)).foregroundStyle(Color.wd.textTertiary)
                HStack(alignment: .top, spacing: .wd.s6) {
                    VStack(alignment: .trailing) {
                        ForEach(0..<4, id: \.self) { i in
                            let v = field.range.upperBound - (field.range.upperBound - field.range.lowerBound) * Float(i) / 3
                            Text(String(format: field.decimals == 0 ? "%.0f" : "%.1f", v)).font(.wd.meta).foregroundStyle(Color.wd.textSecondary)
                            if i < 3 { Spacer() }
                        }
                    }
                    if let img = map.image(vertical: true) {
                        Image(decorative: img, scale: 1)
                            .resizable()
                            .frame(width: 12, height: 260)
                            .overlay(Rectangle().strokeBorder(Color.wd.hairline, lineWidth: 1))
                    }
                }
                .frame(height: 260)
            }
            .padding(.wd.s8)
        }
    }
}
