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
            HStack(spacing: .wd.s4) {
                ForEach(ResultQuantity.allCases, id: \.self) { q in
                    WDChip(q.title, isOn: ctx.results.quantity == q) { ctx.results.quantity = q; ctx.rebuildScenes() }
                }
            }
            .padding(.horizontal, .wd.s12)
            .padding(.bottom, .wd.s2)
            .help("Level, or a reverberation time of the band (EDT, T20, T30) in the wave solver's range")
            if ctx.results.quantity != .spl {
                let bands = ctx.cache.decayBands(fraction: ctx.results.band == .octave ? 1 : 3)
                Text(bands.isEmpty
                     ? "This run has no reverberation times: they need the wave solver (FDTD) on an evenly spaced frequency grid."
                     : "Reverberation time in the band nearest the frequency, from the wave solver's range (\(bands.map { String(format: "%g", $0) }.joined(separator: ", ")) Hz). Needs a step fine enough to hold the decay: a coarse step shows no value.")
                    .font(.wd.detail).foregroundStyle(Color.wd.textTertiary)
                    .padding(.horizontal, .wd.s12).padding(.bottom, .wd.s4)
            }
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
            let draped = p.drapeHeight != nil
            let n = draped ? (ctx.audiencePoints(p).count / 3, 1) : cellCount(p)
            InspectorBody {
                InspectorHeader(icon: draped ? .receiverGrid : .cutPlane, name: planes.count == 1 ? "\(p.code) · \(p.name)" : "\(planes.count) cut planes",
                                subtitle: "\(draped ? "Audience area" : "Cut plane") · \(n.0 * n.1) samples\(solved ? "" : " · needs solve")",
                                onRename: planes.count == 1 ? { v in update { $0.name = v.replacingOccurrences(of: "\(p.code) · ", with: "") } } : nil)
                FrequencySection(ctx: ctx)
                if draped {
                    audienceSection(p, samples: n.0)
                } else {
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

    /// Audience area: ear height above the floor, spacing and which floors.
    @ViewBuilder private func audienceSection(_ p: CutPlane, samples: Int) -> some View {
        InspectorSection("Audience area") {
            WDPropertyRow("Listeners") {
                WDSegmented([(1.2, "Seated 1.2"), (1.7, "Standing 1.7")], selection: p.drapeHeight) { v in
                    update(name: "Set Ear Height") { $0.drapeHeight = v; $0.offset = v }
                }
            }
            WDPropertyRow("Ear height") {
                HStack(spacing: .wd.s6) {
                    WDNumberField(shared(planes) { $0.drapeHeight ?? 1.2 }, UnitFormatter(.length), range: 0.1...5) { v in
                        update(name: "Set Ear Height", coalesce: "ear") { $0.drapeHeight = v; $0.offset = v }
                    }
                    Text("m above the floor").font(.wd.detail).foregroundStyle(Color.wd.textTertiary).fixedSize()
                }
            }
            WDPropertyRow("Resolution") {
                HStack(spacing: .wd.s6) {
                    WDNumberField(shared(planes) { $0.spacing }, UnitFormatter(.length), range: 0.1...10) { v in update(name: "Set Resolution") { $0.spacing = v } }
                    Text("m · \(samples) points").font(.wd.detail)
                        .foregroundStyle(samples > 250_000 ? Color.wd.statusOutdated : Color.wd.textTertiary).fixedSize()
                }
            }
            WDPropertyRow("Floors") {
                if p.drapeSurfaces.isEmpty {
                    WDValueText("All floors")
                } else {
                    HStack(spacing: .wd.s6) {
                        Text(p.drapeSurfaces.map { ctx.store.surfacesByKey[$0]?.code ?? $0 }.joined(separator: ", "))
                            .font(.wd.body).foregroundStyle(Color.wd.textSecondary).lineLimit(2)
                        Spacer(minLength: 0)
                        WDTextButton("All") { update(name: "Audience on All Floors") { $0.drapeSurfaces = [] } }
                    }
                }
            }
            Text("Audience area tool: ⇧-click floors to add them to the selected area.")
                .font(.wd.detail).foregroundStyle(Color.wd.textTertiary).fixedSize(horizontal: false, vertical: true)
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

/// Microphone / point receiver: in Results its level and room acoustics
/// first, then where it is and how it is shown.
struct MicrophoneInspector: View {
    @Bindable var ctx: WindowContext
    let ids: [UInt64]

    var body: some View {
        let mics = ids.compactMap { ctx.store.micsById[$0] }
        if let m = mics.first {
            InspectorBody {
                InspectorHeader(icon: .microphone, name: mics.count == 1 ? "\(m.code) · \(m.name)" : "\(mics.count) microphones",
                                subtitle: m.includeInSolve ? "Receiver · solved point" : "Microphone · probed from a cut plane",
                                onRename: mics.count == 1 ? { n in update { $0.name = n.replacingOccurrences(of: "\(m.code) · ", with: "") } } : nil)
                if ctx.workspace == .results, mics.count == 1 {
                    MicLevelSection(ctx: ctx, mic: m)
                    if let mt = (try? ctx.store.doc.micMetrics(micId: m.id)) ?? nil { MicAcousticsSection(metrics: mt) }
                    if let sd = try? ctx.store.doc.micDecay(micId: m.id, octaveFraction: ctx.results.band == .octave ? 1 : 3), !sd.bandsHz.isEmpty {
                        MicDecaySection(decay: sd)
                    }
                }
                InspectorSection("Position", note: "m") {
                    WDPropertyRow("X · Y · Z") {
                        WDVec3Field([shared(mics) { $0.position.x }, shared(mics) { $0.position.y }, shared(mics) { $0.position.z }], UnitFormatter(.length)) { i, v in
                            update(name: "Move Receiver", coalesce: "mpos\(i)") {
                                if i == 0 { $0.position.x = v } else if i == 1 { $0.position.y = v } else { $0.position.z = v }
                            }
                        }
                    }
                    WDPropertyRow("Solve") {
                        WDCheckbox("Include in the solve", isOn: shared(mics) { $0.includeInSolve }) { v in update(name: "Include in Solve") { $0.includeInSolve = v } }
                    }
                }
                InspectorSection("Display") {
                    WDPropertyRow("Colour") {
                        HStack(spacing: .wd.s6) {
                            ForEach(Color.wd.curveHex, id: \.self) { hex in
                                Circle().fill(Color(hex: hex)!).frame(width: 12, height: 12)
                                    .padding(2)
                                    .overlay(Circle().strokeBorder(hex == m.color ? Color.wd.textPrimary : .clear, lineWidth: 1.2))
                                    .contentShape(Circle())
                                    .onTapGesture { update(name: "Set Colour") { $0.color = hex } }
                            }
                        }
                    }
                    WDPropertyRow("Visible") {
                        WDCheckbox("In the view and plots", isOn: shared(mics) { $0.visible }) { v in update(name: "Toggle Microphone") { $0.visible = v } }
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

/// Level at the current frequency, a small response plot (click to move
/// the frequency) and the full response window.
struct MicLevelSection: View {
    @Bindable var ctx: WindowContext
    let mic: Microphone

    var body: some View {
        let f = ctx.results.frequency
        InspectorSection("Level", note: f >= 1000 ? String(format: "at %.2f kHz", f / 1000) : String(format: "at %.1f Hz", f)) {
            if let r = ctx.micResponse(mic.id) {
                WDPropertyRow("SPL") { WDValueText(ctx.micLevel(mic.id).map { String(format: "%.1f dB", $0) } ?? "—") }
                let idx = ctx.store.microphones.firstIndex { $0.id == mic.id } ?? 0
                let color = mic.color.isEmpty ? Color.wd.curve(idx) : (Color(hex: mic.color) ?? Color.wd.curve(idx))
                let lo = max(r.freqsHz.first ?? 20, 1), hi = max(r.freqsHz.last ?? 20000, lo * 2)
                LinePlot(series: [PlotSeries(id: "m", name: mic.name, color: color, x: r.freqsHz, y: r.splDb)],
                         yAxis: .decibels, xRange: lo...hi, cursor: f, showsLegend: false, onCursor: { ctx.setFrequency($0) })
                    .frame(height: 130)
                    .padding(.horizontal, .wd.s8)
                HStack {
                    Spacer()
                    Button("Open responses…") { ctx.openMicResponse(mic.id) }.buttonStyle(.wd)
                        .help("Magnitude, phase and impulse response (or double-click the microphone)")
                }
                .padding(.horizontal, .wd.s12).padding(.top, .wd.s4)
            } else {
                Text(ctx.store.resultInfo == nil ? "No results yet — solve first." : mic.includeInSolve ? "Not in the active run — solve again." : "Place it on a solved cut plane to probe it.")
                    .font(.wd.detail).foregroundStyle(Color.wd.textTertiary)
                    .padding(.horizontal, .wd.s12).fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

/// Reverberation and clarity in one octave band (or the 500 Hz–1 kHz mid
/// average), picked with chips.
struct MicAcousticsSection: View {
    let metrics: MetricsRec
    @State private var band = -1

    var body: some View {
        let bands = metrics.bandsHz
        let options: [(Int, String)] = [(-1, "Mid")] + bands.enumerated().filter { $0.element >= 125 && $0.element <= 4000 }.map { i, b in
            (i, b >= 1000 ? "\(Int(b / 1000))k" : "\(Int(b))")
        }
        InspectorSection("Room acoustics", note: band < 0 ? "500 Hz–1 kHz" : "octave band") {
            WDChipGroup(options, selection: Binding(get: { band }, set: { band = $0 }))
                .padding(.horizontal, .wd.s12).padding(.bottom, .wd.s4)
            row("T30", metrics.t30S, "%.2f s")
            row("T20", metrics.t20S, "%.2f s")
            row("EDT", metrics.edtS, "%.2f s")
            row("C80", metrics.c80Db, "%+.1f dB")
            row("D50", metrics.d50.map { $0 * 100 }, "%.0f %%")
            // Statistical estimates from the room's volume and absorption.
            row("Sabine", metrics.sabineS, "%.2f s", dim: true)
            row("Eyring", metrics.eyringS, "%.2f s", dim: true)
        }
    }

    private func value(_ v: [Double]) -> Double? {
        if band >= 0 { return v.indices.contains(band) && v[band].isFinite ? v[band] : nil }
        let mid = metrics.bandsHz.enumerated().filter { $0.element == 500 || $0.element == 1000 }.map(\.offset).compactMap { v.indices.contains($0) && v[$0].isFinite ? v[$0] : nil }
        return mid.isEmpty ? nil : mid.reduce(0, +) / Double(mid.count)
    }

    private func row(_ title: String, _ v: [Double], _ fmt: String, dim: Bool = false) -> some View {
        WDPropertyRow(title) { WDValueText(value(v).map { String(format: fmt, $0) } ?? "—", color: dim ? Color.wd.textTertiary : Color.wd.textSecondary) }
    }
}

/// Reverberation times at a mic from the wave solver's response, with the
/// sources' filters and delays in it.
struct MicDecaySection: View {
    let decay: SpectralDecayRec

    var body: some View {
        InspectorSection("Low-frequency decay", note: String(format: "%.1f s window", decay.windowS)) {
            ForEach(Array(decay.bandsHz.enumerated()), id: \.offset) { i, f in
                WDPropertyRow(String(format: "%g Hz", f)) {
                    WDValueText(text(i), color: Color.wd.textSecondary)
                }
            }
        }
    }

    private func text(_ i: Int) -> String {
        func v(_ x: [Double]) -> String { x[i].isFinite ? String(format: "%.2f", x[i]) : "—" }
        return "T30 \(v(decay.t30S))  T20 \(v(decay.t20S))  EDT \(v(decay.edtS)) s"
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
        var out = "x,y,z,\(results.quantity == .spl ? "spl_db" : results.quantity.rawValue + "_s")\n"
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
