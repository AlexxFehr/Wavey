import AppKit
import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel
import WaveyPlots

/// The stack / line-array editor: parameters on the left, the drawing and
/// the element table in the middle, the free-field prediction on the right
/// (toggle), Insert or Apply at the bottom.
struct AssemblyEditorView: View {
    @Bindable var model: AssemblyEditorModel
    let close: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            header
            Rectangle().fill(Color.wd.hairline).frame(height: 1)
            HStack(spacing: 0) {
                ScrollView {
                    VStack(alignment: .leading, spacing: 0) {
                        if model.kind == .stack { StackParameters(model: model) } else { LineArrayParameters(model: model) }
                        PlacementSection(model: model)
                    }
                    .padding(.bottom, .wd.s12)
                }
                .scrollIndicators(.never)
                .frame(width: 290)
                .background(Color.wd.surfacePanel)
                Rectangle().fill(Color.wd.hairline).frame(width: 1)
                VStack(spacing: 0) {
                    HStack(spacing: .wd.s8) {
                        WDSegmented([("side", "Side"), ("top", "Top")], selection: model.view) { model.view = $0 }.frame(width: 140)
                        if model.kind == .lineArray && model.view == "side" {
                            WDSegmented([(false, "Array"), (true, "Coverage")], selection: model.showCoverage) { model.showCoverage = $0 }.frame(width: 170)
                        }
                        Spacer()
                        if let l = model.layout, l.weightKg > 0 {
                            Text(String(format: "%.0f kg", l.weightKg)).font(.wd.numeric).foregroundStyle(Color.wd.textTertiary)
                        }
                        Text("\(model.elementCount) \(model.elementCount == 1 ? "speaker" : "speakers")").font(.wd.numeric).foregroundStyle(Color.wd.textTertiary)
                    }
                    .padding(.horizontal, .wd.s12).frame(height: .wd.viewportHeader)
                    AssemblyPreview(model: model, top: model.view == "top")
                    Rectangle().fill(Color.wd.hairline).frame(height: 1)
                    Group {
                        if model.kind == .stack { StackTable(model: model) } else { LineArrayTable(model: model) }
                    }
                    .frame(height: 210)
                    .clipped()
                }
                if model.predict {
                    Rectangle().fill(Color.wd.hairline).frame(width: 1)
                    PredictionPanel(model: model).frame(width: 330).background(Color.wd.surfacePanel)
                }
            }
            Rectangle().fill(Color.wd.hairline).frame(height: 1)
            footer
        }
        .background(Color.wd.surfaceChrome)
    }

    private var header: some View {
        HStack(spacing: .wd.s8) {
            Color.clear.frame(width: .wd.trafficLights)
            WDIcon(model.kind == .stack ? .stack : .lineArray).foregroundStyle(Color.wd.source)
            WDTextField(model.name, placeholder: "Name", chrome: false, font: .wd.title) { model.name = $0 }
                .frame(width: 220)
            Text(model.groupID == nil ? (model.kind == .stack ? "New speaker stack" : "New line array") : (model.kind == .stack ? "Editing stack" : "Editing line array"))
                .font(.wd.detail).foregroundStyle(Color.wd.textTertiary)
            Spacer()
            WDCheckbox("Prediction", isOn: model.predict) { model.predict = $0 }
                .help("Free-field prediction of the assembly (no room)")
        }
        .padding(.trailing, .wd.s12)
        .frame(height: .wd.topBar)
        .background(Color.wd.surfaceBar)
    }

    private var footer: some View {
        HStack(spacing: .wd.s8) {
            if let e = model.error {
                WDIcon(.warning, size: 12).foregroundStyle(Color.wd.statusError)
                Text(e).font(.wd.detail).foregroundStyle(Color.wd.statusError).lineLimit(1)
            } else if let w = model.layout?.warnings, !w.isEmpty {
                WDIcon(.warning, size: 12).foregroundStyle(Color.wd.statusOutdated)
                Text(w.joined(separator: " · ")).font(.wd.detail).foregroundStyle(Color.wd.statusOutdated).lineLimit(1)
            } else {
                Text(model.groupID == nil ? "Inserted as a group; edit it again from the group's inspector." : "Apply re-lays out the group's speakers.")
                    .font(.wd.detail).foregroundStyle(Color.wd.textTertiary)
            }
            Spacer()
            Button("Cancel") { close() }.buttonStyle(.wd).keyboardShortcut(.cancelAction)
            Button(model.groupID == nil ? "Insert into Project" : "Apply") {
                if model.commit() { close() }
            }
            .buttonStyle(WDButtonStyle(primary: true))
            .keyboardShortcut(.defaultAction)
            .disabled(model.layout == nil || model.elementCount == 0)
        }
        .padding(.horizontal, .wd.s12)
        .frame(height: 44)
        .background(Color.wd.surfaceBar)
    }
}

// MARK: - Shared sections

/// Origin and orientation of the assembly in the room.
struct PlacementSection: View {
    @Bindable var model: AssemblyEditorModel

    var body: some View {
        let len = UnitFormatter(.length), ang = UnitFormatter(.angle)
        let o = model.assembly.origin
        InspectorSection("Placement", note: model.kind == .stack || model.currentFrame?.flown == false ? "stack base" : (isPick ? "pick point" : "frame")) {
            WDPropertyRow("X · Y · Z") {
                WDVec3Field([o.x, o.y, o.z], len) { i, v in
                    var p = o
                    if i == 0 { p.x = v } else if i == 1 { p.y = v } else { p.z = v }
                    model.setOrigin(p)
                }
            }
            WDPropertyRow("Facing") {
                HStack(spacing: .wd.s4) {
                    WDNumberField(model.assembly.yawDeg, ang, range: -360...360) { model.setYaw($0) }
                    ForEach([(0.0, "+X"), (90.0, "+Y"), (180.0, "−X"), (270.0, "−Y")], id: \.0) { deg, label in
                        WDChip(label, isOn: abs(model.assembly.yawDeg - deg) < 0.01) { model.setYaw(deg) }
                    }
                }
            }
        }
    }

    private var isPick: Bool {
        if case .lineArray(let r) = model.assembly.kind, case .pick = r.tilt { return true }
        return false
    }
}

/// Frequencies to predict, as chips (up to three).
struct FrequencyChips: View {
    @Bindable var model: AssemblyEditorModel

    var body: some View {
        let options: [Double] = model.kind == .stack ? [25, 31.5, 40, 50, 63, 80, 100, 125] : [125, 250, 500, 1000, 2000, 4000, 8000, 16000]
        HStack(spacing: .wd.s4) {
            ForEach(options, id: \.self) { f in
                let on = model.predictFrequencies.contains(f)
                WDChip(f >= 1000 ? String(format: "%gk", f / 1000) : String(format: "%g", f), isOn: on) {
                    var s = model.predictFrequencies
                    if on { s.removeAll { $0 == f } } else { s.append(f); if s.count > 3 { s.removeFirst() } }
                    model.predictFrequencies = s.sorted()
                }
            }
        }
    }
}

/// Free-field polar and level-over-distance plots.
struct PredictionPanel: View {
    @Bindable var model: AssemblyEditorModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                InspectorSection("Prediction", note: model.predicting ? "computing…" : "free field") {
                    FrequencyChips(model: model).padding(.horizontal, .wd.s12).padding(.bottom, .wd.s6)
                }
                InspectorSection(model.polarVertical ? "Vertical polar · 30 m" : "Horizontal polar · 30 m", note: "dB re max") {
                    WDSegmented([(false, "Horizontal"), (true, "Vertical")], selection: model.polarVertical) { model.polarVertical = $0 }
                        .padding(.horizontal, .wd.s12).padding(.bottom, .wd.s6)
                    PolarPlot(curves: polarCurves).frame(height: 250).padding(.horizontal, .wd.s12)
                }
                InspectorSection(model.kind == .stack ? "Front and rear · 1.2 m" : "Level over the audience", note: "dB SPL") {
                    if let d = model.distance, let lo = d.x.first, let hi = d.x.last, hi > lo {
                        LinePlot(series: d.freqs.enumerated().map { i, f in
                            PlotSeries(id: "\(f)", name: label(f), color: Color.wd.curve(i), x: d.x, y: d.splDb[i])
                        } + (model.distanceRear.map { r in r.freqs.enumerated().map { i, f in
                            PlotSeries(id: "r\(f)", name: "\(label(f)) rear", color: Color.wd.curve(i), x: r.x, y: r.splDb[i], dashed: true)
                        } } ?? []), xAxis: .linear(unit: "m"), yAxis: .decibels, xRange: lo...hi)
                        .frame(height: 170).padding(.horizontal, .wd.s8)
                        if model.kind == .stack, let r = model.distanceRear, let k = d.x.firstIndex(where: { $0 >= 10 }) {
                            ForEach(Array(d.freqs.enumerated()), id: \.offset) { i, f in
                                WDPropertyRow(label(f)) {
                                    WDValueText(String(format: "rear %+.1f dB", r.splDb[i][k] - d.splDb[i][k]),
                                                color: r.splDb[i][k] - d.splDb[i][k] < -10 ? Color.wd.accentText : Color.wd.textSecondary)
                                }
                            }
                            Text("Rear re front at 10 m; dashed: behind the stack").font(.wd.detail).foregroundStyle(Color.wd.textTertiary).padding(.horizontal, .wd.s12)
                        }
                    } else {
                        WDEmptyState("No prediction", detail: model.error ?? "Choose frequencies above")
                    }
                }
            }
        }
        .scrollIndicators(.never)
    }

    private func label(_ f: Double) -> String { f >= 1000 ? String(format: "%g kHz", f / 1000) : String(format: "%g Hz", f) }

    private var polarCurves: [PolarPlot.Curve] {
        guard let p = model.polar else { return [] }
        return p.freqs.enumerated().map { i, f in
            let mx = p.splDb[i].max() ?? 0
            return PolarPlot.Curve(id: "\(f)", label: label(f), color: Color.wd.curve(i), anglesDeg: p.x, db: p.splDb[i].map { $0 - mx })
        }
    }
}

// MARK: - Window

/// One editor window per edited group (or per new assembly).
@MainActor
final class AssemblyEditorWindow: NSWindowController, NSWindowDelegate {
    private static var open: [ObjectIdentifier: AssemblyEditorWindow] = [:]
    let model: AssemblyEditorModel

    static func show(ctx: WindowContext, kind: AssemblyKindTag, group: UInt64? = nil) {
        if let g = group, let w = open.values.first(where: { $0.model.groupID == g }) {
            w.window?.makeKeyAndOrderFront(nil)
            return
        }
        let m = AssemblyEditorModel(ctx: ctx, kind: kind, group: group)
        let w = AssemblyEditorWindow(model: m)
        open[ObjectIdentifier(w)] = w
        w.window?.center()
        w.showWindow(nil)
    }

    init(model: AssemblyEditorModel) {
        self.model = model
        let win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1240, height: 760),
                           styleMask: [.titled, .closable, .resizable, .miniaturizable, .fullSizeContentView], backing: .buffered, defer: false)
        win.titlebarAppearsTransparent = true
        win.titleVisibility = .hidden
        win.appearance = NSAppearance(named: .darkAqua)
        win.backgroundColor = NSColor(Color.wd.surfaceChrome)
        win.minSize = NSSize(width: 1040, height: 680)
        win.title = model.name
        win.isReleasedWhenClosed = false
        super.init(window: win)
        win.delegate = self
        win.contentView = NSHostingView(rootView: AssemblyEditorView(model: model) { [weak self] in self?.close() })
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func windowWillClose(_ notification: Notification) {
        Self.open[ObjectIdentifier(self)] = nil
    }

    /// A view of an editor for offscreen snapshots (`--snapshot`); the
    /// model's prediction runs as usual.
    static func snapshotView(ctx: WindowContext, kind: AssemblyKindTag, group: UInt64?) -> AnyView {
        let m = AssemblyEditorModel(ctx: ctx, kind: kind, group: group)
        if ProcessInfo.processInfo.environment["WAVEY_SNAPSHOT_AUTOSPLAY"] != nil, kind == .lineArray {
            m.setBoxCount(12)
            m.autoSplay()
            m.showCoverage = true
        }
        return AnyView(AssemblyEditorView(model: m) {}.environment(\.wdStaticTables, true))
    }
}
