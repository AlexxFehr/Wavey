import simd
import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel
import WaveyPlots

/// Sources: device, placement and aim, signal, directivity.
struct SourceInspector: View {
    @Bindable var ctx: WindowContext
    let ids: [UInt64]
    @State private var plane = "h"
    @State private var filterOpen = false

    var body: some View {
        let s = ctx.store
        let spk = ids.compactMap { s.speakersById[$0] }
        if let first = spk.first {
            let model = s.modelsById[first.modelId]
            let len = UnitFormatter(.length), ang = UnitFormatter(.angle)
            let group = s.groups.first { $0.members.contains(first.id) }
            let sub = model.map { ($0.nominalMaxHz ?? $0.maxHz) <= 250 } ?? false
            let kind = !(model?.hasBalloon ?? true) ? "point source · omni" : sub ? "subwoofer" : "loudspeaker · directional"
            InspectorBody {
                InspectorHeader(icon: sub ? .subwoofer : .speaker, tileFill: Color.wd.sourceFill, tileStroke: Color.wd.source, iconColor: Color.wd.source,
                                name: spk.count == 1 ? "\(first.code) · \(first.name)" : "\(spk.count) sources",
                                subtitle: spk.count == 1 ? "\(kind.capitalizedFirst)\(group.map { " · group \($0.name)" } ?? "")" : "Sources · \(spk.count) selected",
                                onRename: spk.count == 1 ? { n in update { $0.name = n.replacingOccurrences(of: "\(first.code) · ", with: "") } } : nil) {
                    HStack(spacing: 2) {
                        LetterToggle(letter: "S", isOn: shared(spk) { $0.solo } ?? false, help: "Solo") {
                            let v = !(shared(spk) { $0.solo } ?? false)
                            update(name: v ? "Solo" : "Unsolo") { $0.solo = v }
                        }
                        LetterToggle(letter: "M", isOn: shared(spk) { $0.muted } ?? false, help: "Mute") {
                            let v = !(shared(spk) { $0.muted } ?? false)
                            update(name: v ? "Mute" : "Unmute") { $0.muted = v }
                        }
                    }
                }
                InspectorSection("Device") {
                    WDPropertyRow("Model") {
                        WDDropdown(s.speakerModels.map { WDDropdownItem($0.id, $0.name, detail: $0.manufacturer) }, selection: shared(spk) { $0.modelId }) { id in
                            update(name: "Change Device") { $0.modelId = id }
                        }
                    }
                    WDPropertyRow("Directivity") {
                        WDValueText(model.map { m in m.hasBalloon ? "\(m.id) · balloon\(m.phaseEstimated ? " · est. phase" : "")" : "omnidirectional" } ?? "—")
                    }
                }
                InspectorSection("Placement", note: "m · deg") {
                    WDPropertyRow("X · Y · Z") {
                        WDVec3Field([shared(spk) { $0.position.x }, shared(spk) { $0.position.y }, shared(spk) { $0.position.z }], len) { i, v in
                            update(name: "Move Source", coalesce: "pos\(i)") {
                                if i == 0 { $0.position.x = v } else if i == 1 { $0.position.y = v } else { $0.position.z = v; $0.onFloor = false }
                            }
                        }
                    }
                    WDPropertyRow("Az · El · Roll") {
                        WDVec3Field([shared(spk) { $0.yawDeg }, shared(spk) { $0.pitchDeg }, shared(spk) { $0.rollDeg }], ang) { i, v in
                            update(name: "Aim Source", coalesce: "aim\(i)") {
                                if i == 0 { $0.yawDeg = v } else if i == 1 { $0.pitchDeg = v } else { $0.rollDeg = v }
                                if i < 2 { $0.aim = nil }
                            }
                        }
                    }
                    WDPropertyRow("On floor") {
                        HStack(spacing: .wd.s6) {
                            WDCheckbox(isOn: shared(spk) { $0.onFloor }) { v in update(name: "Stand on Floor") { $0.onFloor = v } }
                            if shared(spk, { $0.onFloor }) == true {
                                WDNumberField(shared(spk) { $0.bottomHeightM }, len, range: 0...50, unit: "m bottom") { v in
                                    update(name: "Set Bottom Height", coalesce: "bottom") { $0.bottomHeightM = v }
                                }
                            }
                        }
                    }
                    WDPropertyRow("Aim target") {
                        HStack(spacing: .wd.s4) {
                            WDDropdown([WDDropdownItem("none", "None")] + s.microphones.map { WDDropdownItem("mic:\($0.id)", "\($0.code) · \($0.name)", icon: .microphone) },
                                       selection: aimKey(spk)) { key in
                                update(name: "Aim Source") { r in
                                    if key == "none" { r.aim = nil } else if let id = UInt64(key.dropFirst(4)) { r.aim = .microphone(id: id) }
                                }
                            }
                            WDIconButton(.aim, size: .small, isOn: ctx.activeTool == .aim, help: "Pick aim target") { ctx.tool.activate(.aim) }
                        }
                    }
                    if spk.count == 1, let t = aimPoint(first) {
                        let ac = SceneBuilder.acousticCentre(first, mesh: s.cabinetMeshes[first.modelId])
                        let d = simd_distance(SIMD3<Double>(ac), t)
                        WDPropertyRow("To target") {
                            WDValueText(String(format: "%.3f m · %.2f ms", d, d / s.medium.speedOfSound * 1000))
                        }
                    }
                }
                InspectorSection("Signal") {
                    WDPropertyRow("Level") {
                        HStack(spacing: .wd.s6) {
                            WDNumberField(shared(spk) { level($0) }, UnitFormatter(.level), showUnit: false) { v in
                                update(name: "Set Level", coalesce: "level") { $0.gainDb = v - (s.modelsById[$0.modelId]?.sensitivityDb ?? 0) }
                            }
                            Text("dB SPL @ 1 m").font(.wd.detail).foregroundStyle(Color.wd.textTertiary).fixedSize()
                        }
                    }
                    WDPropertyRow("Delay · Gain") {
                        HStack(spacing: .wd.s4) {
                            WDNumberField(shared(spk) { $0.delayMs }, UnitFormatter(.delay), range: 0...2000) { v in update(name: "Set Delay", coalesce: "delay") { $0.delayMs = v } }
                            WDNumberField(shared(spk) { $0.gainDb }, UnitFormatter(.level), range: -120...60) { v in update(name: "Set Gain", coalesce: "gain") { $0.gainDb = v } }
                        }
                    }
                    WDPropertyRow("Polarity") {
                        WDSegmented([(false, "+ Normal"), (true, "− Inverted")], selection: shared(spk) { $0.polarityInverted }) { v in
                            update(name: "Set Polarity") { $0.polarityInverted = v }
                        }
                    }
                    WDPropertyRow("Filter") {
                        Button { filterOpen.toggle() } label: {
                            HStack {
                                Text(spk.count == 1 ? filterSummary(first.filters) : (shared(spk) { filterSummary($0.filters) } ?? "—"))
                                    .font(.wd.numeric).foregroundStyle(Color.wd.textSecondary).lineLimit(1)
                                Spacer()
                                WDIcon(.chevronDown, size: 12).foregroundStyle(Color.wd.textTertiary)
                            }
                            .padding(.horizontal, 7).frame(height: .wd.control)
                            .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(Color.wd.controlFill))
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .disabled(spk.count != 1)
                        .popover(isPresented: $filterOpen, arrowEdge: .leading) { FilterPopover(ctx: ctx, speaker: first) }
                    }
                    if spk.count == 1, !first.filters.isEmpty || first.gainDb != 0 {
                        let r = s.doc.speakerProcessing(speaker: first)
                        LinePlot(series: [PlotSeries(id: "p", name: "Processing", color: Color.wd.source, x: r.freqsHz, y: r.magnitudeDb)],
                                 yAxis: .decibels, xRange: 20...20000)
                            .frame(height: 110)
                            .padding(.horizontal, .wd.s8)
                    }
                }
                if spk.count == 1 || shared(spk, { $0.modelId }) != nil, let m = model, m.hasBalloon {
                    InspectorSection("Directivity · \(plane == "h" ? "horizontal" : "vertical")", note: "dB") {
                        WDSegmented([("h", "Horizontal"), ("v", "Vertical")], selection: plane) { plane = $0 }
                            .padding(.horizontal, .wd.s12).padding(.bottom, .wd.s6)
                        PolarPlot(curves: polarCurves(m.id))
                            .frame(height: 210)
                            .padding(.horizontal, .wd.s12)
                    }
                }
            }
        }
    }

    private func level(_ r: Speaker) -> Double { (ctx.store.modelsById[r.modelId]?.sensitivityDb ?? 0) + r.gainDb }

    private func aimKey(_ spk: [Speaker]) -> String? {
        shared(spk) { r -> String in
            switch r.aim {
            case .microphone(let id): "mic:\(id)"
            case .point: "point"
            case nil: "none"
            }
        }
    }

    private func aimPoint(_ r: Speaker) -> SIMD3<Double>? {
        switch r.aim {
        case .microphone(let id): ctx.store.micsById[id].map { SIMD3($0.position.x, $0.position.y, $0.position.z) }
        case .point(let p): SIMD3(p.x, p.y, p.z)
        case nil: nil
        }
    }

    private func polarCurves(_ model: String) -> [PolarPlot.Curve] {
        let colors = [Color.wd.curve250, Color.wd.source, Color.wd.curve4k]
        let specs: [(Double, String, Color)] = ctx.polarFrequencies(model: model).enumerated().map { i, f in
            (f, f >= 1000 ? String(format: "%g kHz", f / 1000) : String(format: "%g Hz", f), colors[i])
        }
        return specs.compactMap { f, label, c in
            guard let p = ctx.polar(model: model, frequency: f) else { return nil }
            return PolarPlot.Curve(id: label, label: label, color: c, anglesDeg: p.anglesDeg, db: plane == "h" ? p.horizontalDb : p.verticalDb)
        }
    }

    /// Applies a change to every selected source (and to linked group members).
    private func update(name: String = "Change Source", coalesce: String? = nil, _ f: (inout Speaker) -> Void) {
        var targets = Set(ids)
        for g in ctx.store.groups where g.linked && !targets.isDisjoint(with: g.members) { targets.formUnion(g.members) }
        let recs = targets.compactMap { ctx.store.speakersById[$0] }.map { r -> Speaker in var r = r; f(&r); return r }
        ctx.edit(name, coalesce: coalesce.map { "\($0)-\(ids)" }) { try $0.updateSpeakers(speakers: recs) }
    }
}

extension String {
    var capitalizedFirst: String { prefix(1).uppercased() + dropFirst() }
}

/// Filter stages of one source.
struct FilterPopover: View {
    @Bindable var ctx: WindowContext
    let speaker: Speaker

    static let slopes: [(SlopeRec, String)] = [(.butterworth12, "BW12"), (.butterworth24, "BW24"), (.linkwitzRiley24, "LR24"), (.linkwitzRiley48, "LR48")]

    var body: some View {
        let s = ctx.store.speakersById[speaker.id] ?? speaker
        VStack(alignment: .leading, spacing: .wd.s6) {
            Text("Filters · \(s.code)").font(.wd.title).foregroundStyle(Color.wd.textPrimary)
            if s.filters.isEmpty { Text("No filters").font(.wd.body).foregroundStyle(Color.wd.textTertiary) }
            ForEach(Array(s.filters.enumerated()), id: \.offset) { i, f in
                row(s, i, f)
            }
            HStack {
                Menu("Add stage") {
                    Button("High-pass · LR24 · 80 Hz") { add(s, .highPass(freqHz: 80, slope: .linkwitzRiley24)) }
                    Button("Low-pass · LR24 · 80 Hz") { add(s, .lowPass(freqHz: 80, slope: .linkwitzRiley24)) }
                    Button("Low shelf") { add(s, .lowShelf(freqHz: 60, gainDb: 6)) }
                    Button("High shelf") { add(s, .highShelf(freqHz: 8000, gainDb: -3)) }
                    Button("Peak EQ") { add(s, .peak(freqHz: 1000, gainDb: -3, q: 2)) }
                }
                .menuStyle(.borderlessButton).fixedSize()
                Spacer()
            }
        }
        .padding(.wd.s12)
        .frame(width: 340)
        .background(Color.wd.surfacePanel)
    }

    private func row(_ s: Speaker, _ i: Int, _ f: FilterRec) -> some View {
        let hz = UnitFormatter(.frequency, decimals: 0), db = UnitFormatter(.level)
        return HStack(spacing: .wd.s4) {
            Text(title(f)).font(.wd.body).foregroundStyle(Color.wd.textSecondary).frame(width: 64, alignment: .leading)
            switch f {
            case .highPass(let fr, let sl), .lowPass(let fr, let sl):
                WDNumberField(fr, hz, range: 5...40000) { v in set(s, i, isHP(f) ? .highPass(freqHz: v, slope: sl) : .lowPass(freqHz: v, slope: sl)) }
                WDDropdown(Self.slopes.map { WDDropdownItem("\($0.1)", $0.1) }, selection: slopeName(sl)) { name in
                    let n = Self.slopes.first { $0.1 == name }!.0
                    set(s, i, isHP(f) ? .highPass(freqHz: fr, slope: n) : .lowPass(freqHz: fr, slope: n))
                }
            case .lowShelf(let fr, let g):
                WDNumberField(fr, hz, range: 5...40000) { v in set(s, i, .lowShelf(freqHz: v, gainDb: g)) }
                WDNumberField(g, db, range: -40...40) { v in set(s, i, .lowShelf(freqHz: fr, gainDb: v)) }
            case .highShelf(let fr, let g):
                WDNumberField(fr, hz, range: 5...40000) { v in set(s, i, .highShelf(freqHz: v, gainDb: g)) }
                WDNumberField(g, db, range: -40...40) { v in set(s, i, .highShelf(freqHz: fr, gainDb: v)) }
            case .peak(let fr, let g, let q):
                WDNumberField(fr, hz, range: 5...40000) { v in set(s, i, .peak(freqHz: v, gainDb: g, q: q)) }
                WDNumberField(g, db, range: -40...40) { v in set(s, i, .peak(freqHz: fr, gainDb: v, q: q)) }
                WDNumberField(q, UnitFormatter(.plain), range: 0.1...30, unit: "Q") { v in set(s, i, .peak(freqHz: fr, gainDb: g, q: v)) }
            }
            WDIconButton(.minus, size: .small, help: "Remove stage") {
                var r = s
                r.filters.remove(at: i)
                ctx.edit("Remove Filter") { try $0.updateSpeaker(speaker: r) }
            }
        }
    }

    private func isHP(_ f: FilterRec) -> Bool { if case .highPass = f { true } else { false } }

    private func title(_ f: FilterRec) -> String {
        switch f {
        case .highPass: "High-pass"
        case .lowPass: "Low-pass"
        case .lowShelf: "Low shelf"
        case .highShelf: "High shelf"
        case .peak: "Peak"
        }
    }

    private func set(_ s: Speaker, _ i: Int, _ f: FilterRec) {
        var r = s
        r.filters[i] = f
        ctx.edit("Change Filter", coalesce: "filter-\(s.id)-\(i)") { try $0.updateSpeaker(speaker: r) }
    }

    private func add(_ s: Speaker, _ f: FilterRec) {
        var r = s
        r.filters.append(f)
        ctx.edit("Add Filter") { try $0.updateSpeaker(speaker: r) }
    }
}

/// Group: members, link, relative level/delay/polarity.
struct GroupInspector: View {
    @Bindable var ctx: WindowContext
    let id: UInt64

    var body: some View {
        if let g = ctx.store.groupsById[id] {
            InspectorBody {
                InspectorHeader(icon: g.kind == "stack" ? .stack : g.kind == "line_array" ? .lineArray : .link,
                                tileFill: g.kind.isEmpty ? Color.wd.accentFill : Color.wd.sourceFill, tileStroke: g.kind.isEmpty ? nil : Color.wd.source,
                                iconColor: g.kind.isEmpty ? Color.wd.accentText : Color.wd.source,
                                name: g.name, subtitle: "\(g.kind == "stack" ? "Speaker stack" : g.kind == "line_array" ? "Line array" : "Group") · \(g.members.count) sources", onRename: { n in
                    var r = g; r.name = n; ctx.edit("Rename Group") { try $0.updateGroup(group: r) }
                })
                if !g.kind.isEmpty, let a = ctx.store.doc.groupAssembly(group: g.id) {
                    AssemblySection(ctx: ctx, group: g, assembly: a)
                }
                InspectorSection("Group") {
                    WDPropertyRow("Link pair") {
                        WDCheckbox("Edits apply to all members", isOn: g.linked) { v in var r = g; r.linked = v; ctx.edit("Link Group") { try $0.updateGroup(group: r) } }
                    }
                    WDPropertyRow("Level") {
                        WDNumberField(0, UnitFormatter(.level), range: -60...60, unit: "dB rel.") { v in relative { $0.gainDb += v } }
                    }
                    WDPropertyRow("Delay") {
                        WDNumberField(0, UnitFormatter(.delay), range: -1000...1000, unit: "ms rel.") { v in relative { $0.delayMs = max(0, $0.delayMs + v) } }
                    }
                    WDPropertyRow("Polarity") {
                        WDTextButton("Invert all") { relative { $0.polarityInverted.toggle() } }
                    }
                }
                InspectorSection("Members", note: "\(g.members.count)") {
                    ForEach(g.members, id: \.self) { m in
                        if let s = ctx.store.speakersById[m] {
                            HStack(spacing: .wd.s6) {
                                Text(s.code).font(.wd.meta).foregroundStyle(Color.wd.source)
                                Text(s.name).font(.wd.body).foregroundStyle(Color.wd.textSecondary)
                                Spacer()
                                WDIconButton(.close, size: .small, help: "Remove from group") {
                                    var r = g; r.members.removeAll { $0 == m }
                                    ctx.edit("Remove from Group") { try $0.updateGroup(group: r) }
                                }
                            }
                            .padding(.horizontal, .wd.s12).frame(height: .wd.row)
                        }
                    }
                    WDTextButton("Add selected sources") {}
                        .padding(.horizontal, .wd.s12).hidden()
                }
            }
        }
    }

    private func relative(_ f: (inout Speaker) -> Void) {
        guard let g = ctx.store.groupsById[id] else { return }
        let recs = g.members.compactMap { ctx.store.speakersById[$0] }.map { r -> Speaker in var r = r; f(&r); return r }
        ctx.edit("Change Group") { try $0.updateSpeakers(speakers: recs) }
    }
}

/// Stack / line-array recipe of a group: summary, placement, Edit….
struct AssemblySection: View {
    @Bindable var ctx: WindowContext
    let group: GroupRec
    let assembly: AssemblyRec

    var body: some View {
        let len = UnitFormatter(.length), ang = UnitFormatter(.angle)
        InspectorSection(group.kind == "stack" ? "Stack" : "Line array", note: group.inSync ? nil : "edited outside the editor", noteAccent: false) {
            switch assembly.kind {
            case .stack(let preset, let elements):
                WDPropertyRow("Layout") { WDValueText(presetName(preset) + " · \(elements.count) boxes") }
            case .lineArray(let r):
                WDPropertyRow("Module") { WDValueText(ctx.store.modelsById[r.modelId]?.name ?? r.modelId) }
                WDPropertyRow("Boxes") { WDValueText(String(format: "%d · total splay %.0f°", r.elements.count, r.splaysDeg.reduce(0, +))) }
            }
            WDPropertyRow("Origin") {
                WDVec3Field([assembly.origin.x, assembly.origin.y, assembly.origin.z], len) { i, v in
                    var o = assembly.origin
                    if i == 0 { o.x = v } else if i == 1 { o.y = v } else { o.z = v }
                    ctx.edit("Move Array", coalesce: "asm-origin-\(group.id)-\(i)") { try $0.transformAssembly(group: group.id, origin: o, yawDeg: assembly.yawDeg) }
                }
            }
            WDPropertyRow("Facing") {
                WDNumberField(assembly.yawDeg, ang, range: -360...360) { v in
                    ctx.edit("Turn Array", coalesce: "asm-yaw-\(group.id)") { try $0.transformAssembly(group: group.id, origin: assembly.origin, yawDeg: v) }
                }
            }
            if !group.inSync {
                Text("Members were moved individually; Edit… or a move re-applies the recipe.")
                    .font(.wd.detail).foregroundStyle(Color.wd.statusOutdated).padding(.horizontal, .wd.s12).fixedSize(horizontal: false, vertical: true)
            }
            HStack(spacing: .wd.s6) {
                Button(group.kind == "stack" ? "Edit Stack…" : "Edit Array…") { ctx.editAssembly(group.id) }.buttonStyle(WDButtonStyle(primary: true))
                Button("Duplicate") { ctx.duplicateGroups([group.id]) }.buttonStyle(.wd)
                Button("Ungroup") { ctx.detachAssembly(group.id) }.buttonStyle(.wd).help("Keep the speakers as a plain group without the recipe")
            }
            .padding(.horizontal, .wd.s12).padding(.vertical, .wd.s4)
        }
    }

    private func presetName(_ p: StackPresetRec) -> String {
        switch p {
        case .cardioid(_, let f, let r): "Cardioid \(f)+\(r)"
        case .endfire(_, let n, let d): String(format: "Endfire %d × %.2f m", n, d)
        case .gradient(_, let d): String(format: "Gradient %.2f m", d)
        case .broadside(_, let n, let d, let a): String(format: "Broadside %d × %.2f m, arc %.0f°", n, d, a)
        case .custom: "Custom"
        }
    }
}

/// A library device: response, sensitivity, directivity.
struct DeviceInspector: View {
    @Bindable var ctx: WindowContext
    let id: String

    var body: some View {
        if let m = ctx.store.modelsById[id] {
            InspectorBody {
                InspectorHeader(icon: m.hasBalloon ? .speaker : .pointSource, tileFill: Color.wd.sourceFill, tileStroke: Color.wd.source, iconColor: Color.wd.source,
                                name: m.name, subtitle: "Device\(m.manufacturer.map { " · \($0)" } ?? "")")
                InspectorSection("Device") {
                    WDPropertyRow("Sensitivity") { WDValueText(String(format: "%.1f dB @ %.0f Hz, %.0f m, %.2f V", m.sensitivityDb, m.referenceHz, m.referenceDistanceM, m.referenceVoltageRms)) }
                    WDPropertyRow("Range") { WDValueText("\(Int(m.minHz))–\(Int(m.maxHz)) Hz") }
                    if let lo = m.nominalMinHz, let hi = m.nominalMaxHz { WDPropertyRow("Nominal") { WDValueText("\(Int(lo))–\(Int(hi)) Hz") } }
                    WDPropertyRow("Box") { WDValueText(String(format: "%.2f × %.2f × %.2f m", m.widthM, m.heightM, m.depthM)) }
                    if m.phaseEstimated { WDPropertyRow("Phase") { WDValueText("estimated (minimum phase)", color: Color.wd.statusOutdated) } }
                }
                if let r = try? ctx.store.doc.speakerResponse(modelId: id) {
                    InspectorSection("On-axis response", note: "dB SPL") {
                        LinePlot(series: [PlotSeries(id: "r", name: m.name, color: Color.wd.source, x: r.freqsHz, y: r.magnitudeDb)],
                                 yAxis: .decibels, xRange: max(r.freqsHz.first ?? 20, 10)...max(r.freqsHz.last ?? 20000, 100))
                            .frame(height: 130).padding(.horizontal, .wd.s8)
                    }
                }
                if m.hasBalloon {
                    InspectorSection("Directivity · horizontal", note: "dB") {
                        PolarPlot(curves: zip(ctx.polarFrequencies(model: id), [Color.wd.curve250, Color.wd.source, Color.wd.curve4k]).map { f, c in
                            (f, f >= 1000 ? String(format: "%g kHz", f / 1000) : String(format: "%g Hz", f), c)
                        }.compactMap { f, l, c in
                            ctx.polar(model: id, frequency: f).map { PolarPlot.Curve(id: l, label: l, color: c, anglesDeg: $0.anglesDeg, db: $0.horizontalDb) }
                        })
                        .frame(height: 210).padding(.horizontal, .wd.s12)
                    }
                }
            } footer: {
                Button("Place at centre") { ctx.placeDeviceAtCentre(id) }.buttonStyle(.wd).frame(maxWidth: .infinity)
                Button("Export…") { ctx.exportDevice(id) }.buttonStyle(.wd).frame(maxWidth: .infinity)
            }
        }
    }
}

/// A stock-library device not yet in the project: what it is, and using it.
struct LibraryItemInspector: View {
    @Bindable var ctx: WindowContext
    let path: String

    var body: some View {
        if let e = SpeakerLibrary.shared.entriesByPath[path] {
            let sub = (e.nominalMaxHz ?? e.maxHz) <= 250
            InspectorBody {
                InspectorHeader(icon: e.hasRigging ? .lineArray : sub ? .subwoofer : .speaker, tileFill: Color.wd.sourceFill, tileStroke: Color.wd.source,
                                iconColor: Color.wd.source, name: e.name, subtitle: "Library\(e.manufacturer.isEmpty ? "" : " · \(e.manufacturer)")")
                InspectorSection("Device") {
                    WDPropertyRow("Range") { WDValueText("\(Int(e.minHz))–\(Int(e.maxHz)) Hz") }
                    if let lo = e.nominalMinHz, let hi = e.nominalMaxHz { WDPropertyRow("Nominal") { WDValueText("\(Int(lo))–\(Int(hi)) Hz") } }
                    WDPropertyRow("Box") { WDValueText(String(format: "%.2f × %.2f × %.2f m", e.widthM, e.heightM, e.depthM)) }
                    WDPropertyRow("Directivity") { WDValueText(e.hasBalloon ? "balloon" : "omnidirectional") }
                    if e.hasRigging { WDPropertyRow("Rigging") { WDValueText("line-array module (frames, splays)") } }
                    WDPropertyRow("File") { WDValueText((e.path as NSString).lastPathComponent).help(e.path) }
                }
                Text("Not in this project yet: placing it, dropping it in the view or picking it in an editor adds it.")
                    .font(.wd.detail).foregroundStyle(Color.wd.textTertiary).padding(.horizontal, .wd.s12).fixedSize(horizontal: false, vertical: true)
            } footer: {
                Button("Place at centre") { if let id = ctx.useLibraryFile(path) { ctx.placeDeviceAtCentre(id) } }.buttonStyle(.wd).frame(maxWidth: .infinity)
                Button("Add to project") {
                    if let id = ctx.useLibraryFile(path) { ctx.libraryDevice = id; ctx.select([.device(id)]) }
                }.buttonStyle(.wd).frame(maxWidth: .infinity)
            }
        }
    }
}

/// Small lettered toggle (S / M) for solo and mute.
struct LetterToggle: View {
    let letter: String
    let isOn: Bool
    let help: String
    let action: () -> Void
    @State private var hover = false

    var body: some View {
        Button(action: action) {
            Text(letter)
                .font(.wd.ui(11, .semibold))
                .foregroundStyle(isOn ? Color.wd.accentTextOnFill : hover ? Color.wd.textPrimary : Color.wd.textTertiary)
                .frame(width: .wd.iconSmall, height: .wd.iconSmall)
                .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(isOn ? Color.wd.accentFill : hover ? Color.wd.controlFill : .clear))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
        .help(help)
        .accessibilityLabel(help)
        .accessibilityValue(isOn ? "on" : "off")
    }
}
