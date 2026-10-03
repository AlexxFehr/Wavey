import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel

private let blankSettings = ElementSettingsRec(gainDb: 0, delayMs: 0, polarityInverted: false, muted: false, filters: [])

/// Devices to pick: the project's, then stock-library ones not in it yet
/// (id `lib:<path>`, added to the project when picked).
@MainActor private func modelItems(_ store: DocumentStore, rigged: ((String) -> Bool)? = nil) -> [WDDropdownItem<String>] {
    let inProject = Set(store.speakerModels.map(\.id))
    // Line arrays: only modules with rigging data.
    let project = store.speakerModels.filter { m in rigged.map { $0(m.id) } ?? true }.map { m in
        WDDropdownItem(m.id, m.name, detail: m.manufacturer)
    }
    let library = SpeakerLibrary.shared.entries.filter { !inProject.contains($0.id) && (rigged == nil || $0.hasRigging) }.map { e in
        WDDropdownItem("lib:\(e.path)", e.name, detail: "library · \(e.manufacturer)")
    }
    return project + library
}

/// Resolves a picked item to a project model id (adding library devices).
@MainActor private func resolve(_ id: String, _ model: AssemblyEditorModel) -> String? {
    id.hasPrefix("lib:") ? model.ctx.useLibraryFile(String(id.dropFirst(4))) : id
}

// MARK: - Stack

struct StackParameters: View {
    @Bindable var model: AssemblyEditorModel

    var body: some View {
        if let (preset, elements) = model.stackRecipe {
            let len = UnitFormatter(.length)
            InspectorSection("Preset", note: "c = \(String(format: "%.1f", model.store.medium.speedOfSound)) m/s") {
                WDPropertyRow("Type") {
                    WDDropdown([WDDropdownItem("cardioid", "Cardioid stack", detail: "rear box reversed, inverted, delayed"),
                                WDDropdownItem("endfire", "Endfire", detail: "in line, delayed toward the front"),
                                WDDropdownItem("gradient", "Gradient pair", detail: "front and rear, rear inverted"),
                                WDDropdownItem("broadside", "Broadside row", detail: "side by side, optional delay arc"),
                                WDDropdownItem("custom", "Custom", detail: "elements as edited")],
                               selection: kind(preset)) { k in model.applyPreset(make(k, from: preset)) }
                }
                switch preset {
                case .cardioid(let m, let fwd, let rev):
                    modelRow(m) { model.applyPreset(.cardioid(modelId: $0, forward: fwd, reversed: rev)) }
                    WDPropertyRow("Forward") { count(fwd, 1...8) { model.applyPreset(.cardioid(modelId: m, forward: $0, reversed: rev)) } }
                    WDPropertyRow("Reversed") { count(rev, 0...8) { model.applyPreset(.cardioid(modelId: m, forward: fwd, reversed: $0)) } }
                case .endfire(let m, let n, let d):
                    modelRow(m) { model.applyPreset(.endfire(modelId: $0, count: n, spacingM: d)) }
                    WDPropertyRow("Count") { count(n, 2...16) { model.applyPreset(.endfire(modelId: m, count: $0, spacingM: d)) } }
                    WDPropertyRow("Spacing") { WDNumberField(d, len, range: 0.1...10) { model.applyPreset(.endfire(modelId: m, count: n, spacingM: $0)) } }
                    WDPropertyRow("Tuned for") { WDValueText(String(format: "%.0f Hz (¼ λ = spacing)", model.store.medium.speedOfSound / (4 * d))) }
                case .gradient(let m, let d):
                    modelRow(m) { model.applyPreset(.gradient(modelId: $0, spacingM: d)) }
                    WDPropertyRow("Spacing") { WDNumberField(d, len, range: 0.1...10) { model.applyPreset(.gradient(modelId: m, spacingM: $0)) } }
                case .broadside(let m, let n, let d, let arc):
                    modelRow(m) { model.applyPreset(.broadside(modelId: $0, count: n, spacingM: d, arcDeg: arc)) }
                    WDPropertyRow("Count") { count(n, 2...32) { model.applyPreset(.broadside(modelId: m, count: $0, spacingM: d, arcDeg: arc)) } }
                    WDPropertyRow("Spacing") { WDNumberField(d, len, range: 0.1...10) { model.applyPreset(.broadside(modelId: m, count: n, spacingM: $0, arcDeg: arc)) } }
                    WDPropertyRow("Delay arc") { WDNumberField(arc, UnitFormatter(.angle), range: 0...170) { model.applyPreset(.broadside(modelId: m, count: n, spacingM: d, arcDeg: $0)) } }
                case .custom:
                    EmptyView()
                }
            }
            if elements.contains(where: { $0.settings.polarityInverted }) {
                InspectorSection("Rear rejection", note: model.tuneResult.map { String(format: "%+.1f dB", $0) }) {
                    WDPropertyRow("Band") {
                        HStack(spacing: .wd.s4) {
                            WDNumberField(model.tuneLo, UnitFormatter(.frequency, decimals: 0), range: 10...500, showUnit: false) { model.tuneLo = $0 }
                            WDNumberField(model.tuneHi, UnitFormatter(.frequency, decimals: 0), range: 10...500, unit: "Hz") { model.tuneHi = $0 }
                        }
                    }
                    HStack(spacing: .wd.s6) {
                        Button("Tune delay & gain") { model.tuneRear() }.buttonStyle(WDButtonStyle(primary: true))
                        Spacer()
                    }
                    .padding(.horizontal, .wd.s12).padding(.vertical, .wd.s4)
                    Text("Sets the inverted elements' delay and gain for the least sound behind the stack (free field, measured directivity).")
                        .font(.wd.detail).foregroundStyle(Color.wd.textTertiary).padding(.horizontal, .wd.s12).fixedSize(horizontal: false, vertical: true)
                }
            }
            InspectorSection("Elements", note: "\(elements.count)") {
                HStack(spacing: .wd.s6) {
                    Button("Add") {
                        model.editStack { p, e in
                            var n = e.last ?? StackElementRec(modelId: model.store.speakerModels.first?.id ?? "", offset: Point3(x: 0, y: 0, z: 0.3), yawDeg: 0, pitchDeg: 0, rollDeg: 0, settings: blankSettings)
                            let h = model.store.modelsById[n.modelId]?.heightM ?? 0.5
                            n.offset.z += e.isEmpty ? 0 : h
                            e.append(n)
                            p = .custom
                        }
                    }.buttonStyle(.wd)
                    Button("Remove") {
                        model.editStack { p, e in
                            e = e.enumerated().filter { !model.selectedRows.contains($0.offset) }.map(\.element)
                            p = .custom
                        }
                        model.selectedRows = []
                    }.buttonStyle(.wd).disabled(model.selectedRows.isEmpty)
                    Spacer()
                }
                .padding(.horizontal, .wd.s12).padding(.vertical, .wd.s4)
                Text("Offsets are from the stack base (footprint centre on the floor), before the stack's facing.")
                    .font(.wd.detail).foregroundStyle(Color.wd.textTertiary).padding(.horizontal, .wd.s12).fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func modelRow(_ id: String, _ set: @escaping (String) -> Void) -> some View {
        WDPropertyRow("Model") { WDDropdown(modelItems(model.store), selection: id) { if let r = resolve($0, model) { set(r) } } }
    }

    private func count(_ n: UInt32, _ r: ClosedRange<Double>, _ set: @escaping (UInt32) -> Void) -> some View {
        WDNumberField(Double(n), UnitFormatter(.count), range: r, step: 1) { set(UInt32($0.rounded())) }
    }

    private func kind(_ p: StackPresetRec) -> String {
        switch p {
        case .cardioid: "cardioid"
        case .endfire: "endfire"
        case .gradient: "gradient"
        case .broadside: "broadside"
        case .custom: "custom"
        }
    }

    private func make(_ k: String, from p: StackPresetRec) -> StackPresetRec {
        let m: String = switch p {
        case .cardioid(let id, _, _), .endfire(let id, _, _), .gradient(let id, _), .broadside(let id, _, _, _): id
        case .custom: model.stackRecipe?.1.first?.modelId ?? model.store.speakerModels.first?.id ?? ""
        }
        return switch k {
        case "cardioid": .cardioid(modelId: m, forward: 2, reversed: 1)
        case "endfire": .endfire(modelId: m, count: 4, spacingM: 1.0)
        case "gradient": .gradient(modelId: m, spacingM: 1.0)
        case "broadside": .broadside(modelId: m, count: 6, spacingM: 1.2, arcDeg: 0)
        default: .custom
        }
    }
}

private struct StackRow: Identifiable {
    let id: Int
    let e: StackElementRec
}

struct StackTable: View {
    @Bindable var model: AssemblyEditorModel
    @Environment(\.wdStaticTables) private var staticTables

    var body: some View {
        let rows = (model.stackRecipe?.1 ?? []).enumerated().map { StackRow(id: $0.offset, e: $0.element) }
        let len = UnitFormatter(.length), ang = UnitFormatter(.angle), db = UnitFormatter(.level), ms = UnitFormatter(.delay)
        let columns: [WDTableColumn<StackRow>] = [
            WDTableColumn("n", "#", width: 30, numeric: true) { "\($0.id + 1)" },
            WDTableColumn("model", "Model", width: 170, editor: .menu({ _ in modelItems(model.store).map { ($0.id, $0.title) } }) { rows, id in
                if let r = resolve(id, model) { edit(rows) { $0.modelId = r } }
            }) { model.store.modelsById[$0.e.modelId]?.name ?? $0.e.modelId },
            num("x", "X", len, \.offset.x),
            num("y", "Y", len, \.offset.y),
            num("z", "Z", len, \.offset.z),
            num("yaw", "Facing", ang, \.yawDeg),
            num("gain", "Gain", db, \.settings.gainDb),
            num("delay", "Delay", ms, \.settings.delayMs),
            WDTableColumn("inv", "Inv", width: 40, editor: .checkbox({ $0.e.settings.polarityInverted }) { rows, v in edit(rows) { $0.settings.polarityInverted = v } }) {
                $0.e.settings.polarityInverted ? "✓" : ""
            },
            WDTableColumn("mute", "Mute", width: 44, editor: .checkbox({ $0.e.settings.muted }) { rows, v in edit(rows) { $0.settings.muted = v } }) {
                $0.e.settings.muted ? "✓" : ""
            },
        ]
        if staticTables {
            WDStaticTable(rows, columns: columns, selection: model.selectedRows)
        } else {
            WDTable(rows, columns: columns, selection: model.selectedRows, emptyText: "No elements") { ids, _ in model.selectedRows = Set(ids) }
        }
    }

    private func num(_ id: String, _ title: String, _ f: UnitFormatter, _ kp: WritableKeyPath<StackElementRec, Double>) -> WDTableColumn<StackRow> {
        WDTableColumn(id, title, width: 74, numeric: true, editor: .number(f) { rows, v in edit(rows) { $0[keyPath: kp] = v } }) { f.string($0.e[keyPath: kp]) }
    }

    private func edit(_ rows: [StackRow], _ f: @escaping (inout StackElementRec) -> Void) {
        let ids = Set(rows.map(\.id))
        model.editStack { _, e in
            for i in e.indices where ids.contains(i) { f(&e[i]) }
        }
    }
}

// MARK: - Line array

struct LineArrayParameters: View {
    @Bindable var model: AssemblyEditorModel

    var body: some View {
        if let r = model.lineRecipe {
            let len = UnitFormatter(.length), ang = UnitFormatter(.angle)
            let rig = model.rigging
            InspectorSection("Module") {
                WDPropertyRow("Model") {
                    WDDropdown(modelItems(model.store) { model.store.doc.speakerRigging(modelId: $0) != nil }, selection: r.modelId,
                               placeholder: "No line-array modules") { picked in
                        guard let id = resolve(picked, model), let rg = model.store.doc.speakerRigging(modelId: id) else { return }
                        // Keep the mount (flown / ground) if the new module has it.
                        let flown = model.currentFrame?.flown ?? true
                        model.editLine {
                            $0.modelId = id
                            $0.frame = rg.frames.first { $0.flown == flown }?.key ?? rg.frames.first?.key
                            $0.frameSplayDeg = 0
                            $0.tilt = .manual(deg: 0)
                        }
                    }
                }
                if rig == nil {
                    Text("Line arrays need a module with rigging data (pivots, splays): add its GLL conversion to the library (scripts/build-library.sh).")
                        .font(.wd.detail).foregroundStyle(Color.wd.statusOutdated).padding(.horizontal, .wd.s12).fixedSize(horizontal: false, vertical: true)
                }
                let flown = model.currentFrame?.flown ?? true
                let canFly = rig?.frames.contains { $0.flown } ?? false, canStack = rig?.frames.contains { !$0.flown } ?? false
                WDPropertyRow("Mount") {
                    WDSegmented([(true, "Flown"), (false, "Ground stack")], selection: flown) { fly in
                        guard let f = rig?.frames.first(where: { $0.flown == fly }) else {
                            model.ctx.post(fly ? "This module has no fly frame" : "This module cannot be ground-stacked (no ground frame in its data)", .info)
                            return
                        }
                        model.setMount(frame: f)
                    }
                    .disabled(!(canFly && canStack))
                    .help(canStack ? (canFly ? "Hang from a fly frame or stand on a ground-stack frame" : "Ground stack only") : "This module has no ground-stack frame")
                }
                WDPropertyRow("Frame") {
                    WDDropdown((rig?.frames ?? []).filter { $0.flown == flown }.map {
                        WDDropdownItem($0.key, $0.label, detail: $0.maxBoxes.map { "max \($0) boxes" })
                    } + (flown ? [WDDropdownItem("", "None", detail: "top box hangs at the origin")] : []), selection: r.frame ?? "") { k in
                        model.editLine { $0.frame = k.isEmpty ? nil : k; $0.frameSplayDeg = 0; if k.isEmpty { $0.tilt = .manual(deg: 0) } }
                    }
                }
                WDPropertyRow("Boxes") {
                    HStack(spacing: .wd.s6) {
                        WDNumberField(Double(r.elements.count), UnitFormatter(.count), range: 1...64, step: 1) { model.setBoxCount(Int($0.rounded())) }
                        if let mx = model.currentFrame?.maxBoxes { Text("max \(mx)").font(.wd.detail).foregroundStyle(Color.wd.textTertiary).fixedSize() }
                    }
                }
                if let f = model.currentFrame {
                    WDPropertyRow(f.flown ? "Frame angle" : "Base angle") {
                        WDDropdown(f.splaysDeg.map { WDDropdownItem(angleKey($0), String(format: "%+g°", $0)) }, selection: angleKey(r.frameSplayDeg)) { k in
                            model.editLine { $0.frameSplayDeg = Double(k) ?? 0 }
                        }
                    }
                }
            }
            if model.currentFrame?.flown ?? true {
                InspectorSection("Hang") {
                    WDPropertyRow("Tilt by") {
                        WDSegmented([("manual", "Angle"), ("pick", "Pick point")], selection: isPick(r) ? "pick" : "manual") { v in
                            model.editLine { $0.tilt = v == "pick" ? .pick(index: UInt32((model.currentFrame?.pickPoints.count ?? 1) / 2)) : .manual(deg: model.layout?.frameTiltDeg ?? 0) }
                        }
                        .disabled(model.currentFrame?.pickPoints.isEmpty ?? true)
                    }
                    switch r.tilt {
                    case .manual(let deg):
                        WDPropertyRow("Frame tilt") { WDNumberField(deg, ang, range: -60...60, unit: "° down") { v in model.editLine { $0.tilt = .manual(deg: v) } } }
                    case .pick(let i):
                        WDPropertyRow("Pick point") {
                            WDDropdown((model.currentFrame?.pickPoints ?? []).enumerated().map { k, p in WDDropdownItem("\(k)", "Pin \(p.label)") }, selection: "\(i)") { k in
                                model.editLine { $0.tilt = .pick(index: UInt32(k) ?? 0) }
                            }
                        }
                        WDPropertyRow("Frame tilt") { WDValueText(String(format: "%+.1f° (by gravity)", model.layout?.frameTiltDeg ?? 0)) }
                    }
                }
            }
            InspectorSection("Audience · auto-splay") {
                WDPropertyRow("First · last") {
                    HStack(spacing: .wd.s4) {
                        WDNumberField(model.audienceFirst, len, range: 0.5...300, showUnit: false) { model.audienceFirst = $0; model.refreshGuides() }
                        WDNumberField(model.audienceLast, len, range: 1...400, showUnit: false) { model.audienceLast = $0; model.refreshGuides() }
                    }
                }
                WDPropertyRow("Ear height") { WDNumberField(model.listenerZ, len, range: -10...50) { model.listenerZ = $0; model.refreshGuides() } }
                WDPropertyRow("Rake") { WDNumberField(model.rakeDeg, ang, range: -30...45) { model.rakeDeg = $0; model.refreshGuides() } }
                HStack(spacing: .wd.s6) {
                    Button("Auto-splay") { model.autoSplay() }.buttonStyle(WDButtonStyle(primary: true)).disabled(rig == nil)
                    if let presets = rig?.presets, !presets.isEmpty {
                        Menu("Presets") {
                            ForEach(presets, id: \.label) { p in
                                Button(p.label) { model.setBoxCount(p.splaysDeg.count + 1); model.editLine { $0.splaysDeg = p.splaysDeg } }
                            }
                        }
                        .menuStyle(.borderlessButton).fixedSize()
                    }
                    Spacer()
                    Menu("All splays") {
                        ForEach(rig?.splaysDeg ?? [], id: \.self) { s in
                            Button(String(format: "%g°", s)) { model.editLine { $0.splaysDeg = $0.splaysDeg.map { _ in s } } }
                        }
                    }
                    .menuStyle(.borderlessButton).fixedSize()
                }
                .padding(.horizontal, .wd.s12).padding(.vertical, .wd.s4)
                Text("Each box aims at its share of the audience line, splays snapped to the module's settings.")
                    .font(.wd.detail).foregroundStyle(Color.wd.textTertiary).padding(.horizontal, .wd.s12).fixedSize(horizontal: false, vertical: true)
            }
            if let l = model.layout, let top = l.elements.first, let bottom = l.elements.last {
                InspectorSection("Summary") {
                    WDPropertyRow("Total splay") { WDValueText(String(format: "%.1f°", r.splaysDeg.reduce(0, +))) }
                    WDPropertyRow("Top · bottom") { WDValueText(String(format: "%.2f m · %.2f m", top.position.z, bottom.position.z)) }
                    WDPropertyRow("Aim") { WDValueText(String(format: "%+.1f° … %+.1f°", 0 - (l.boxTiltDeg.first ?? 0), 0 - (l.boxTiltDeg.last ?? 0))) }
                }
            }
        }
    }

    private func isPick(_ r: LineArrayRec) -> Bool { if case .pick = r.tilt { true } else { false } }
}

func angleKey(_ v: Double) -> String { String(format: "%g", v) }

private struct BoxRow: Identifiable {
    let id: Int
    let splay: Double?
    let aim: Double
    let z: Double
    let s: ElementSettingsRec
}

struct LineArrayTable: View {
    @Bindable var model: AssemblyEditorModel
    @Environment(\.wdStaticTables) private var staticTables

    var body: some View {
        let rows = boxRows()
        let allowed = model.rigging?.splaysDeg ?? []
        let db = UnitFormatter(.level), ms = UnitFormatter(.delay)
        let columns: [WDTableColumn<BoxRow>] = [
            WDTableColumn("n", "Box", width: 40, numeric: true) { "\($0.id + 1)" },
            WDTableColumn("splay", "Splay to next", width: 100, numeric: true, editor: .menu({ row in row.splay == nil ? [] : allowed.map { (angleKey($0), String(format: "%g°", $0)) } }) { rows, k in
                guard let v = Double(k) else { return }
                let ids = Set(rows.map(\.id))
                model.editLine { r in for i in r.splaysDeg.indices where ids.contains(i) { r.splaysDeg[i] = v } }
            }) { $0.splay.map { String(format: "%g°", $0) } ?? "—" },
            WDTableColumn("aim", "Aim", width: 70, numeric: true) { String(format: "%+.1f°", $0.aim) },
            WDTableColumn("z", "Height", width: 70, numeric: true) { String(format: "%.2f m", $0.z) },
            WDTableColumn("gain", "Gain", width: 70, numeric: true, editor: .number(db) { rows, v in edit(rows) { $0.gainDb = v } }) { db.string($0.s.gainDb) },
            WDTableColumn("delay", "Delay", width: 70, numeric: true, editor: .number(ms) { rows, v in edit(rows) { $0.delayMs = v } }) { ms.string($0.s.delayMs) },
            WDTableColumn("inv", "Inv", width: 40, editor: .checkbox({ $0.s.polarityInverted }) { rows, v in edit(rows) { $0.polarityInverted = v } }) { $0.s.polarityInverted ? "✓" : "" },
            WDTableColumn("mute", "Mute", width: 44, editor: .checkbox({ $0.s.muted }) { rows, v in edit(rows) { $0.muted = v } }) { $0.s.muted ? "✓" : "" },
        ]
        if staticTables {
            WDStaticTable(rows, columns: columns, selection: model.selectedRows)
        } else {
            WDTable(rows, columns: columns, selection: model.selectedRows, emptyText: "No boxes") { ids, _ in model.selectedRows = Set(ids) }
        }
    }

    private func boxRows() -> [BoxRow] {
        guard let r = model.lineRecipe else { return [] }
        let tilts: [Double] = model.layout?.boxTiltDeg ?? []
        let poses: [ElementPoseRec] = model.layout?.elements ?? []
        var out: [BoxRow] = []
        for (i, s) in r.elements.enumerated() {
            let splay: Double? = i < r.splaysDeg.count ? r.splaysDeg[i] : nil
            let aim: Double = i < tilts.count ? 0 - tilts[i] : 0
            let z: Double = i < poses.count ? poses[i].position.z : 0
            out.append(BoxRow(id: i, splay: splay, aim: aim, z: z, s: s))
        }
        return out
    }

    private func edit(_ rows: [BoxRow], _ f: @escaping (inout ElementSettingsRec) -> Void) {
        let ids = Set(rows.map(\.id))
        model.editLine { r in for i in r.elements.indices where ids.contains(i) { f(&r.elements[i]) } }
    }
}

extension Array {
    subscript(safe i: Int) -> Element? { indices.contains(i) ? self[i] : nil }
}
