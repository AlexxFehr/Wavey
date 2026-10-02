import AppKit
import SwiftUI
import UniformTypeIdentifiers
import WaveyCore
import WaveyDesign
import WaveyModel
import WaveyPlots
import WaveyViewport

/// Frequency section shared by the cut-plane inspector and the results default.
struct FrequencySection: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        let freqs = ctx.store.resultInfo?.freqsHz ?? []
        let lo = freqs.first ?? 20, hi = max(freqs.last ?? 500, lo * 1.01)
        InspectorSection("Frequency", note: "linked to plots", noteAccent: true) {
            HStack(spacing: .wd.s8) {
                WDSlider(value: ctx.results.frequency, range: lo...hi, logarithmic: hi / lo > 10) { v, _ in ctx.setFrequency(v) }
                WDNumberField(ctx.results.frequency, UnitFormatter(.frequency), range: lo...hi) { ctx.setFrequency($0) }
                    .frame(width: 84)
            }
            .padding(.horizontal, .wd.s12)
            .frame(minHeight: .wd.propertyRow)
            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: .wd.s4), count: 5), spacing: .wd.s4) {
                ForEach(bandChips(lo, hi), id: \.self) { b in
                    WDChip(label(b), isOn: abs(nearest(b, freqs) - ctx.results.frequency) < 0.01) { ctx.setFrequency(b) }
                        .frame(maxWidth: .infinity)
                }
            }
            .padding(.horizontal, .wd.s12)
            .padding(.vertical, .wd.s4)
            HStack(spacing: .wd.s4) {
                ForEach(BandMode.allCases, id: \.self) { m in
                    WDChip(m.title, isOn: ctx.results.band == m) { ctx.results.band = m; ctx.rebuildScenes() }
                }
            }
            .padding(.horizontal, .wd.s12)
            .padding(.top, .wd.s2)
        }
    }

    private func label(_ b: Double) -> String {
        if b >= 1000 { return b.truncatingRemainder(dividingBy: 1000) == 0 ? "\(Int(b / 1000))k" : String(format: "%.1fk", b / 1000) }
        return b == b.rounded() ? "\(Int(b))" : String(format: "%.1f", b)
    }

    /// Preferred 1/3-octave centres inside the run range (10, centred on the current frequency).
    private func bandChips(_ lo: Double, _ hi: Double) -> [Double] {
        let all: [Double] = [20, 25, 31.5, 40, 50, 63, 80, 100, 125, 160, 200, 250, 315, 400, 500, 630, 800, 1000, 1250, 1600, 2000, 2500, 3150, 4000, 5000, 6300, 8000, 10000, 12500, 16000]
        let inside = all.filter { $0 >= lo * 0.98 && $0 <= hi * 1.02 }
        guard inside.count > 10 else { return inside }
        let i = inside.indices.min { abs(log(inside[$0] / ctx.results.frequency)) < abs(log(inside[$1] / ctx.results.frequency)) } ?? 0
        let start = max(0, min(inside.count - 10, i - 4))
        return Array(inside[start..<(start + 10)])
    }

    private func nearest(_ f: Double, _ freqs: [Double]) -> Double {
        freqs.min { abs(log($0 / f)) < abs(log($1 / f)) } ?? f
    }
}

struct ResultsFrequencyOnly: View {
    @Bindable var ctx: WindowContext
    var body: some View {
        InspectorBody {
            InspectorHeader(icon: .chart, name: ctx.store.activeRun?.name ?? "No results", subtitle: "Results · nothing selected")
            FrequencySection(ctx: ctx)
            if ctx.store.cutPlanes.isEmpty {
                WDEmptyState("No cut planes", detail: "Add one with the Cut plane tool (C).").frame(maxWidth: .infinity)
            }
        }
    }
}

/// Cut plane (also a receiver grid): frequency, plane, colour map.
struct CutPlaneInspector: View {
    @Bindable var ctx: WindowContext
    let planes: [CutPlane]

    var body: some View {
        if let p = planes.first {
            let solved = ctx.store.resultInfo?.planeIds.contains(p.id) ?? false
            let n = cellCount(p)
            InspectorBody {
                InspectorHeader(icon: .cutPlane, name: planes.count == 1 ? "\(p.code) · \(p.name)" : "\(planes.count) cut planes",
                                subtitle: "Cut plane · \(n.0 * n.1) samples\(solved ? "" : " · needs solve")",
                                onRename: planes.count == 1 ? { v in update { $0.name = v.replacingOccurrences(of: "\(p.code) · ", with: "") } } : nil)
                FrequencySection(ctx: ctx)
                InspectorSection("Plane") {
                    WDPropertyRow("Orientation") {
                        WDSegmented([(PlaneAxisRec.horizontal, "XY"), (.constY, "XZ"), (.constX, "YZ")], selection: shared(planes) { $0.axis }) { a in
                            update(name: "Change Orientation") { $0.axis = a; $0.offset = defaultOffset(a) }
                        }
                    }
                    WDPropertyRow("Offset \(p.axis == .horizontal ? "Z" : p.axis == .constX ? "X" : "Y")") {
                        HStack(spacing: .wd.s6) {
                            WDNumberField(shared(planes) { $0.offset }, UnitFormatter(.length)) { v in update(name: "Move Plane", coalesce: "offset") { $0.offset = v } }
                            Text(p.axis == .horizontal ? "m · ear height" : "m").font(.wd.detail).foregroundStyle(Color.wd.textTertiary).fixedSize()
                        }
                    }
                    WDPropertyRow("Resolution") {
                        HStack(spacing: .wd.s6) {
                            WDNumberField(shared(planes) { $0.spacing }, UnitFormatter(.length), range: 0.025...10) { v in update(name: "Set Resolution") { $0.spacing = v } }
                            Text("m · \(n.0) × \(n.1)").font(.wd.detail)
                                .foregroundStyle(n.0 * n.1 > 250_000 ? Color.wd.statusOutdated : Color.wd.textTertiary).fixedSize()
                        }
                    }
                    WDPropertyRow("Clip") { WDValueText("To room volume") }
                }
                InspectorSection("Colour map") {
                    WDPropertyRow("Map") {
                        WDDropdown(ColorMap.allCases.map { WDDropdownItem($0.rawValue, $0.title) }, selection: shared(planes) { $0.style.colorMap }) { v in
                            update(name: "Set Colour Map") { $0.style.colorMap = v }
                        }
                    }
                    WDPropertyRow("Range") {
                        WDSegmented([(RangeModeRec.auto, "Auto · \(Int(p.style.autoSpan)) dB"), (.fixed, "Fixed"), (.symmetric, "Symmetric")],
                                    selection: shared(planes) { $0.style.range }) { v in update(name: "Set Range") { $0.style.range = v } }
                    }
                    if p.style.range == .fixed {
                        WDPropertyRow("Min · Max") {
                            HStack(spacing: .wd.s4) {
                                WDNumberField(p.style.rangeMin, UnitFormatter(.level)) { v in update(name: "Set Range", coalesce: "rmin") { $0.style.rangeMin = v } }
                                WDNumberField(p.style.rangeMax, UnitFormatter(.level)) { v in update(name: "Set Range", coalesce: "rmax") { $0.style.rangeMax = v } }
                            }
                        }
                    }
                    WDPropertyRow("Contours") {
                        HStack(spacing: .wd.s8) {
                            WDNumberField(p.style.contourStep, UnitFormatter(.level), range: 0.5...20) { v in update(name: "Set Contours", coalesce: "contour") { $0.style.contourStep = v } }
                            WDCheckbox("Show", isOn: shared(planes) { $0.style.showContours }) { v in update(name: "Show Contours") { $0.style.showContours = v } }
                        }
                    }
                    WDPropertyRow("Opacity") {
                        HStack(spacing: .wd.s8) {
                            WDSlider(value: p.style.opacity * 100, range: 20...100, step: 1) { v, _ in
                                update(name: "Set Opacity", coalesce: "opacity") { $0.style.opacity = v / 100 }
                            }
                            Text("\(Int((p.style.opacity * 100).rounded()))%").font(.wd.numeric).foregroundStyle(Color.wd.textSecondary).frame(width: 36, alignment: .trailing)
                        }
                    }
                }
            } footer: {
                Button(ctx.results.sweeping ? "Stop" : "Animate sweep") { ctx.toggleSweep() }.buttonStyle(.wd).frame(maxWidth: .infinity)
                Button("Export CSV") { ctx.exportPlaneCSV(p) }.buttonStyle(.wd).frame(maxWidth: .infinity).disabled(!solved)
            }
        }
    }

    private func defaultOffset(_ a: PlaneAxisRec) -> Double {
        guard let r = ctx.store.room else { return 1.2 }
        switch a {
        case .horizontal: return 1.2
        case .constX: return (r.boundsMin.x + r.boundsMax.x) / 2
        case .constY: return (r.boundsMin.y + r.boundsMax.y) / 2
        }
    }

    private func cellCount(_ p: CutPlane) -> (Int, Int) {
        guard let r = ctx.store.room else { return (0, 0) }
        let (w, d): (Double, Double) = switch p.axis {
        case .horizontal: (r.boundsMax.x - r.boundsMin.x, r.boundsMax.y - r.boundsMin.y)
        case .constX: (r.boundsMax.y - r.boundsMin.y, r.boundsMax.z - r.boundsMin.z)
        case .constY: (r.boundsMax.x - r.boundsMin.x, r.boundsMax.z - r.boundsMin.z)
        }
        return (Int((w / p.spacing).rounded(.up)), Int((d / p.spacing).rounded(.up)))
    }

    private func update(name: String = "Change Cut Plane", coalesce: String? = nil, _ f: (inout CutPlane) -> Void) {
        let recs = planes.compactMap { ctx.store.planesById[$0.id] }.map { p -> CutPlane in var p = p; f(&p); return p }
        ctx.edit(name, coalesce: coalesce.map { "\($0)-\(planes.map(\.id))" }) { doc in for r in recs { try doc.updateCutPlane(plane: r) } }
    }
}

/// Microphone / point receiver.
struct MicrophoneInspector: View {
    @Bindable var ctx: WindowContext
    let ids: [UInt64]

    var body: some View {
        let mics = ids.compactMap { ctx.store.micsById[$0] }
        if let m = mics.first {
            let idx = ctx.store.microphones.firstIndex { $0.id == m.id } ?? 0
            let color = m.color.isEmpty ? Color.wd.curve(idx) : (Color(hex: m.color) ?? Color.wd.curve(idx))
            InspectorBody {
                InspectorHeader(icon: .microphone, name: mics.count == 1 ? "\(m.code) · \(m.name)" : "\(mics.count) microphones",
                                subtitle: m.includeInSolve ? "Receiver · solved point" : "Microphone · probed from a cut plane",
                                onRename: mics.count == 1 ? { n in update { $0.name = n.replacingOccurrences(of: "\(m.code) · ", with: "") } } : nil)
                InspectorSection("Position", note: "m") {
                    WDPropertyRow("X · Y · Z") {
                        WDVec3Field([shared(mics) { $0.position.x }, shared(mics) { $0.position.y }, shared(mics) { $0.position.z }], UnitFormatter(.length)) { i, v in
                            update(name: "Move Receiver", coalesce: "mpos\(i)") {
                                if i == 0 { $0.position.x = v } else if i == 1 { $0.position.y = v } else { $0.position.z = v }
                            }
                        }
                    }
                    WDPropertyRow("In solve") {
                        WDCheckbox("Solve this point", isOn: shared(mics) { $0.includeInSolve }) { v in update(name: "Include in Solve") { $0.includeInSolve = v } }
                    }
                }
                InspectorSection("Display") {
                    WDPropertyRow("Colour") {
                        HStack(spacing: 4) {
                            ForEach(Color.wd.curveHex, id: \.self) { hex in
                                Circle().fill(Color(hex: hex)!).frame(width: 14, height: 14)
                                    .overlay(Circle().strokeBorder(hex == m.color ? Color.wd.textPrimary : .clear, lineWidth: 1.5))
                                    .onTapGesture { update(name: "Set Colour") { $0.color = hex } }
                            }
                        }
                    }
                    WDPropertyRow("Show") {
                        WDCheckbox("In viewport and plots", isOn: shared(mics) { $0.visible }) { v in update(name: "Toggle Microphone") { $0.visible = v } }
                    }
                }
                if mics.count == 1 {
                    InspectorSection("Response") {
                        if let r = ctx.micResponse(m.id) {
                            Text(String(format: "SPL @ %.1f Hz: %.1f dB", ctx.results.frequency, ctx.micLevel(m.id) ?? .nan))
                                .font(.wd.readout).foregroundStyle(Color.wd.textPrimary)
                                .padding(.horizontal, .wd.s12)
                            Sparkline(x: r.freqsHz, y: r.splDb, color: color, cursor: ctx.results.frequency)
                                .frame(height: 40)
                                .padding(.horizontal, .wd.s12)
                        } else {
                            Text(ctx.store.resultInfo == nil ? "No results yet" : m.includeInSolve ? "Not in the active run — solve again" : "Place it on a solved cut plane to probe it")
                                .font(.wd.body).foregroundStyle(Color.wd.textTertiary).padding(.horizontal, .wd.s12)
                        }
                    }
                    if let mt = (try? ctx.store.doc.micMetrics(micId: m.id)) ?? nil {
                        InspectorSection("Room acoustics", note: "octave bands") { MetricsGrid(m: mt) }
                    }
                }
            }
        }
    }

    private func update(name: String = "Change Microphone", coalesce: String? = nil, _ f: (inout Microphone) -> Void) {
        let recs = ids.compactMap { ctx.store.micsById[$0] }.map { m -> Microphone in var m = m; f(&m); return m }
        ctx.edit(name, coalesce: coalesce.map { "\($0)-\(ids)" }) { try $0.updateMicrophones(mics: recs) }
    }
}

/// T30, T20, EDT, C80, D50 and statistical RT per octave band.
struct MetricsGrid: View {
    let m: MetricsRec
    var body: some View {
        let rows: [(String, [Double], String)] = [("T30", m.t30S, "%.2f"), ("T20", m.t20S, "%.2f"), ("EDT", m.edtS, "%.2f"),
                                                    ("C80", m.c80Db, "%.1f"), ("D50", m.d50.map { $0 * 100 }, "%.0f"),
                                                    ("Eyring", m.eyringS, "%.2f"), ("Sabine", m.sabineS, "%.2f")]
        Grid(alignment: .trailing, horizontalSpacing: 4, verticalSpacing: 3) {
            GridRow {
                Text("").gridColumnAlignment(.leading)
                ForEach(m.bandsHz, id: \.self) { b in Text(b >= 1000 ? "\(Int(b / 1000))k" : "\(Int(b))").foregroundStyle(Color.wd.textTertiary) }
            }
            ForEach(rows, id: \.0) { r in
                GridRow {
                    Text(r.0).foregroundStyle(Color.wd.textTertiary)
                    ForEach(Array(r.1.enumerated()), id: \.offset) { _, v in
                        Text(v.isFinite ? String(format: r.2, v) : "—").foregroundStyle(Color.wd.textSecondary)
                    }
                }
            }
        }
        .font(.wd.meta)
        .padding(.horizontal, .wd.s12)
    }
}

extension WindowContext {
    /// Sweeps the frequency across the run range (8 s, looping) until stopped.
    func toggleSweep() {
        results.sweeping.toggle()
        guard results.sweeping, let freqs = store.resultInfo?.freqsHz, freqs.count > 1 else { results.sweeping = false; return }
        Task { @MainActor [weak self] in
            var i = 0
            let dt = max(8.0 / Double(freqs.count), 1.0 / 60)
            while let self, self.results.sweeping {
                self.results.frequency = freqs[i % freqs.count]
                self.rebuildScenes()
                i += 1
                try? await Task.sleep(for: .seconds(dt))
            }
        }
    }

    /// Field values at the current frequency/band as CSV (x, y, z, dB).
    func exportPlaneCSV(_ p: CutPlane) {
        guard let f = fieldValues(p) else { return }
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(p.code) \(Int(results.frequency)) Hz.csv"
        panel.allowedContentTypes = [.commaSeparatedText]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let s = f.stack
        var out = "x,y,z,spl_db\n"
        for iv in 0..<Int(s.nv) {
            for iu in 0..<Int(s.nu) {
                let v = f.values[iv * Int(s.nu) + iu]
                guard v.isFinite else { continue }
                let u = s.u0 + Double(iu) * s.du, w = s.v0 + Double(iv) * s.dv
                let (x, y, z): (Double, Double, Double) = switch s.axis {
                case .horizontal: (u, w, s.offset)
                case .constX: (s.offset, u, w)
                case .constY: (u, s.offset, w)
                }
                out += String(format: "%.3f,%.3f,%.3f,%.2f\n", x, y, z, v)
            }
        }
        try? out.write(to: url, atomically: true, encoding: .utf8)
    }
}

/// Receiver grid popover: height and spacing; the grid covers the room.
struct ReceiverGridPopover: View {
    @Bindable var ctx: WindowContext
    @State private var height = 1.2
    @State private var spacing = 0.6

    var body: some View {
        let r = ctx.store.room
        let count = r.map { Int((($0.boundsMax.x - $0.boundsMin.x) / spacing).rounded(.up)) * Int((($0.boundsMax.y - $0.boundsMin.y) / spacing).rounded(.up)) } ?? 0
        VStack(alignment: .leading, spacing: 0) {
            Text("Receiver grid").font(.wd.title).foregroundStyle(Color.wd.textPrimary).padding(.horizontal, .wd.s12).padding(.top, .wd.s8)
            WDPropertyRow("Height z") { WDNumberField(height, UnitFormatter(.length), range: 0.05...100) { height = $0 } }
            WDPropertyRow("Spacing") { WDNumberField(spacing, UnitFormatter(.length), range: 0.05...10) { spacing = $0 } }
            WDPropertyRow("Size") { WDValueText("Fit to volume") }
            WDPropertyRow("Points") { WDValueText("\(count) (outside skipped)") }
            HStack {
                Button("Cancel") { ctx.receiverGridPopover = nil }.buttonStyle(.wd)
                Spacer()
                Button("Create") {
                    var id: UInt64 = 0
                    ctx.edit("Add Receiver Grid") { id = $0.addCutPlane(axis: .horizontal, offset: height, spacing: spacing) }
                    ctx.receiverGridPopover = nil
                    if id != 0 { ctx.select([.plane(id)]) }
                }
                .buttonStyle(.wdPrimary)
            }
            .padding(.wd.s12)
        }
        .frame(width: 280)
        .wdPopoverChrome()
    }
}
