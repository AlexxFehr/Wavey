import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel

/// Run list, progress log and changes since the active run.
struct RunPopover: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        VStack(alignment: .leading, spacing: .wd.s6) {
            Text("Runs").font(.wd.title).foregroundStyle(Color.wd.textPrimary)
            if ctx.runs.solving {
                WDStatusItem(nil, "\(Int(ctx.runs.fraction * 100))% · \(ctx.runs.stage)", dot: Color.wd.accent)
                if ctx.runs.editedDuringSolve {
                    Text("Edits since solve started — the finished run will be outdated").font(.wd.detail).foregroundStyle(Color.wd.statusOutdated)
                }
                ScrollView {
                    VStack(alignment: .leading, spacing: 1) {
                        ForEach(Array(ctx.runs.log.enumerated()), id: \.offset) { _, l in Text(l).font(.wd.status).foregroundStyle(Color.wd.textTertiary) }
                    }
                    .textSelection(.enabled)
                }
                .frame(maxHeight: 120)
                Button("Cancel") { ctx.runs.cancel() }.buttonStyle(.wd)
            }
            if case .failed(let e) = ctx.runs.status {
                Text(e).font(.wd.body).foregroundStyle(Color.wd.statusError).textSelection(.enabled)
                WDTextButton("Dismiss") { ctx.runs.clearError() }
            }
            if ctx.store.runs.isEmpty && !ctx.runs.solving {
                Text("No runs yet").font(.wd.body).foregroundStyle(Color.wd.textTertiary)
            }
            ForEach(ctx.store.runs.reversed(), id: \.id) { r in
                HStack(spacing: .wd.s6) {
                    Circle().fill(r.stale ? Color.wd.statusOutdated : Color.wd.statusCurrent).frame(width: 6, height: 6)
                    Text(r.name).font(.wd.body).foregroundStyle(r.active ? Color.wd.accentText : Color.wd.textSecondary)
                    Text("\(Int(r.fMin))–\(Int(r.fMax)) Hz").font(.wd.meta).foregroundStyle(Color.wd.textTertiary)
                    Spacer()
                    Text(formatDuration(r.seconds)).font(.wd.meta).foregroundStyle(Color.wd.textTertiary)
                }
                .frame(height: .wd.row)
                .contentShape(Rectangle())
                .onTapGesture {
                    _ = ctx.store.doc.setActiveRun(id: r.id)
                    ctx.store.refresh()
                    ctx.afterChange()
                }
            }
            if !ctx.store.changesSinceRun.isEmpty {
                Text("Changed since the active run: \(ctx.store.changesSinceRun.joined(separator: ", "))")
                    .font(.wd.detail).foregroundStyle(Color.wd.statusOutdated)
            }
            HStack {
                WDTextButton("Settings…") { ctx.runPopoverPresented = false; ctx.solveSettingsPresented = true }
                Spacer()
            }
        }
        .padding(.wd.s12)
        .frame(width: 340)
        .background(Color.wd.surfacePanel)
    }
}

/// How much work a solve puts into the methods it uses. Sets the detailed
/// numbers (reflection order, rays, wave-solver crossover, accuracy and
/// points per wavelength) so they need no setting of their own.
enum SolveQuality: String, CaseIterable {
    case draft, medium, high

    var title: String { rawValue.capitalized }

    var order: UInt32 { switch self { case .draft: 2; case .medium: 3; case .high: 4 } }
    var rays: UInt32 { switch self { case .draft: 5_000; case .medium: 20_000; case .high: 100_000 } }
    var crossoverHz: Double { switch self { case .draft: 80; case .medium: 120; case .high: 160 } }
    var accuracy: WaveAccuracyRec { switch self { case .draft: .draft; case .medium: .standard; case .high: .high } }
    var pointsPerWavelength: Double { switch self { case .draft: 10; case .medium: 12; case .high: 15 } }

    /// The settings with this quality applied to the methods that are on.
    func applied(to s: SimSettingsRec) -> SimSettingsRec {
        var s = s
        s.maxOrder = order
        if s.lateReverb { s.raysPerSource = rays }
        if s.fdtdEnabled {
            s.fdtdCrossoverHz = crossoverHz
            s.fdtdAccuracy = accuracy
            s.fdtdPointsPerWavelength = pointsPerWavelength
        }
        return s
    }

    /// The quality the settings amount to, if any.
    static func of(_ s: SimSettingsRec) -> SolveQuality? {
        allCases.first { $0.applied(to: s) == s }
    }
}

/// Solve settings: the kind of solve, range and resolution, methods,
/// hardware and quality; estimate; Solve.
struct SolveSettingsPopover: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        let s = ctx.store.simSettings
        let hz = UnitFormatter(.frequency, decimals: 0)
        VStack(alignment: .leading, spacing: 0) {
            Text("Solve settings").font(.wd.title).foregroundStyle(Color.wd.textPrimary).padding(.wd.s12)
            WDPropertyRow("Sound") {
                HStack(spacing: .wd.s12) {
                    WDRadio("Anechoic", isOn: s.freeField) { set { $0.freeField = true } }
                        .help("Direct sound only: nothing reflects, blocks or diffracts, not even the ground")
                    WDRadio("Echoic", isOn: !s.freeField) { set { $0.freeField = false } }
                        .help("Reflections, diffraction and the reverberant field")
                }
            }
            if !s.freeField {
                if !ctx.store.isOpenAir {
                    WDPropertyRow("Roof") {
                        WDSegmented([(false, "Closed"), (true, "Open sky")], selection: ctx.store.isOpenRoof) { v in
                            ctx.edit("Set Roof") { try $0.setOpenRoof(open: v) }
                        }
                    }
                } else {
                    WDPropertyRow("Ground") {
                        WDDropdown(ctx.store.groundChoices().map { WDDropdownItem($0.id, $0.name) }, selection: ctx.store.groundMaterial) { id in
                            ctx.edit("Set Ground") { try $0.setGroundMaterial(id: id) }
                        }
                    }
                }
            }
            WDPropertyRow("Range") {
                HStack(spacing: .wd.s4) {
                    WDNumberField(s.fMin, hz, range: 1...20000) { v in set { $0.fMin = v } }
                    WDNumberField(s.fMax, hz, range: 2...40000) { v in set { $0.fMax = v } }
                }
            }
            WDPropertyRow("Resolution") {
                WDDropdown([(1, "1/1 oct"), (3, "1/3 oct"), (6, "1/6 oct"), (12, "1/12 oct"), (24, "1/24 oct"), (48, "1/48 oct"), (0, "Linear")]
                    .map { WDDropdownItem(UInt32($0.0), $0.1) }, selection: s.octaveFraction) { v in set { $0.octaveFraction = v } }
            }
            if s.octaveFraction == 0 {
                WDPropertyRow("Step") { WDNumberField(s.linearStepHz, UnitFormatter(.frequency, decimals: 2), range: 0.01...1000) { v in set { $0.linearStepHz = v } } }
            }
            if !s.freeField {
                WDPropertyRow("Methods") {
                    VStack(alignment: .leading, spacing: 4) {
                        WDCheckbox("Diffraction", isOn: s.diffraction) { v in set { $0.diffraction = v } }
                        WDCheckbox("Angle-dependent reflection", isOn: s.impedanceReflection) { v in set { $0.impedanceReflection = v } }
                        WDCheckbox("Late reverb (rays)", isOn: s.lateReverb) { v in set { $0.lateReverb = v } }
                        WDCheckbox("Wave solver (FDTD)", isOn: s.fdtdEnabled) { v in set { $0.fdtdEnabled = v } }
                    }
                    .padding(.vertical, 4)
                }
            }
            WDPropertyRow("Run on") {
                HStack(spacing: .wd.s12) {
                    WDRadio("GPU", isOn: s.useGpu) { set { $0.useGpu = true } }
                    WDRadio("CPU", isOn: !s.useGpu) { set { $0.useGpu = false } }
                }
            }
            if !s.freeField {
                let quality = SolveQuality.of(s)
                WDPropertyRow("Quality") {
                    WDDropdown((quality == nil ? [WDDropdownItem("custom", "Custom")] : []) + SolveQuality.allCases.map { WDDropdownItem($0.rawValue, $0.title) },
                               selection: quality?.rawValue ?? "custom") { id in
                        if let q = SolveQuality(rawValue: id) { set { $0 = q.applied(to: $0) } }
                    }
                }
                Text(qualityNote(s, quality))
                    .font(.wd.detail).foregroundStyle(Color.wd.textTertiary).fixedSize(horizontal: false, vertical: true)
                    .padding(.horizontal, .wd.s12).padding(.bottom, .wd.s4)
            }
            if let e = try? ctx.store.doc.estimate() {
                Text("~\(e.sources) sources × \(e.receivers) receivers · \(e.frequencies) f · \(String(format: "%.0f", e.resultMegabytes)) MB")
                    .font(.wd.status).foregroundStyle(Color.wd.textTertiary).padding(.horizontal, .wd.s12).padding(.top, .wd.s6)
            }
            HStack {
                Spacer()
                Button("Solve") { ctx.solveSettingsPresented = false; ctx.runs.solve() }
                    .buttonStyle(.wdPrimary).disabled(ctx.runs.solving || ctx.store.blockingIssue != nil)
            }
            .padding(.wd.s12)
        }
        .frame(width: 340)
        .background(Color.wd.surfacePanel)
    }

    /// What the quality sets for the methods that are on.
    private func qualityNote(_ s: SimSettingsRec, _ q: SolveQuality?) -> String {
        guard let q else { return "Reflection order, rays and wave-solver numbers were set by hand (an older project, or the file). Pick a quality to set them together." }
        var parts = ["reflection order \(q.order)"]
        if s.lateReverb { parts.append("\(q.rays / 1000)k rays") }
        if s.fdtdEnabled { parts.append("wave solver to \(Int(q.crossoverHz)) Hz, \(Int(q.pointsPerWavelength)) points per wavelength") }
        return "Sets " + parts.joined(separator: ", ") + "."
    }

    private func set(_ f: (inout SimSettingsRec) -> Void) {
        var s = ctx.store.simSettings
        f(&s)
        ctx.edit("Change Solve Settings") { try $0.setSimSettings(settings: s) }
    }
}

/// Material shown when selected from the library sheet.
struct MaterialInspectorView: View {
    @Bindable var ctx: WindowContext
    let id: String
    var body: some View {
        if let m = ctx.store.materialsById[id] {
            InspectorBody {
                InspectorHeader(icon: .layers, name: m.name, subtitle: m.builtin ? "Material · built-in" : "Material")
                InspectorSection("Absorption and scattering") { MaterialBands(material: m).padding(.horizontal, .wd.s12) }
            } footer: {
                Button("Edit in library…") { ctx.materialLibraryPresented = true }.buttonStyle(.wd).frame(maxWidth: .infinity)
            }
        }
    }
}

/// Material library: list on the left, editor on the right.
struct MaterialLibrarySheet: View {
    @Bindable var ctx: WindowContext
    @Environment(\.dismiss) private var dismiss
    @State private var selected: String?
    @State private var query = ""

    var body: some View {
        let mats = ctx.store.materials.filter { query.isEmpty || $0.name.localizedCaseInsensitiveContains(query) }
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                VStack(alignment: .leading, spacing: .wd.s4) {
                    TextField("Search", text: $query)
                        .textFieldStyle(.plain).font(.wd.body)
                        .padding(.horizontal, 7).frame(height: .wd.control)
                        .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(Color.wd.controlFill))
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            ForEach(mats, id: \.id) { m in
                                HStack(spacing: .wd.s6) {
                                    Circle().fill(Color(hex: m.color) ?? .gray).frame(width: 8, height: 8)
                                    Text(m.name).font(.wd.body).foregroundStyle(m.id == selected ? Color.wd.accentTextOnFill : Color.wd.textSecondary).lineLimit(1)
                                    Spacer()
                                    if m.builtin { WDIcon(.lock, size: 12).foregroundStyle(Color.wd.textTertiary) }
                                }
                                .padding(.horizontal, .wd.s6)
                                .frame(height: .wd.row)
                                .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(m.id == selected ? Color.wd.selectionRow : .clear))
                                .contentShape(Rectangle())
                                .onTapGesture { selected = m.id }
                            }
                        }
                    }
                }
                .padding(.wd.s12)
                .frame(width: 240)
                Rectangle().fill(Color.wd.hairline).frame(width: 1)
                editor.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }
            HStack {
                Button("Duplicate") {
                    guard let id = selected else { return }
                    var new: String?
                    ctx.edit("Duplicate Material") { new = try $0.duplicateMaterial(id: id) }
                    selected = new
                }
                .buttonStyle(.wd).disabled(selected == nil)
                Button("Delete") {
                    guard let id = selected else { return }
                    ctx.edit("Delete Material") { try $0.deleteMaterial(id: id) }
                    selected = nil
                }
                .buttonStyle(.wd).disabled(selected.flatMap { ctx.store.materialsById[$0] }?.builtin ?? true)
                Spacer()
                Button("Done") { dismiss() }.buttonStyle(.wdPrimary).keyboardShortcut(.defaultAction)
            }
            .padding(.wd.s12)
        }
        .frame(width: 720, height: 480)
        .background(Color.wd.surfacePanel)
        .environment(\.colorScheme, .dark)
    }

    @ViewBuilder private var editor: some View {
        if let id = selected, let m = ctx.store.materialsById[id] {
            let bands = MaterialBands.bands
            let a = MaterialBands(material: m).values(m.absorption), s = MaterialBands(material: m).values(m.scattering)
            VStack(alignment: .leading, spacing: 0) {
                InspectorHeader(icon: .layers, name: m.name, subtitle: m.builtin ? "Built-in · duplicate to edit" : "User material",
                                onRename: m.builtin ? nil : { n in save(m) { $0.name = n } })
                MaterialBands(material: m).padding(.horizontal, .wd.s12)
                if !m.builtin {
                    Grid(horizontalSpacing: 4, verticalSpacing: 4) {
                        GridRow {
                            Text("").gridColumnAlignment(.leading)
                            ForEach(bands, id: \.self) { b in Text(b >= 1000 ? "\(Int(b / 1000))k" : "\(Int(b))").font(.wd.meta).foregroundStyle(Color.wd.textTertiary) }
                        }
                        GridRow {
                            Text("α").font(.wd.meta).foregroundStyle(Color.wd.absorption)
                            ForEach(0..<8, id: \.self) { i in
                                WDNumberField(a[i], UnitFormatter(.ratio), range: 0...1, showUnit: false) { v in
                                    var n = a; n[i] = v
                                    save(m) { $0.absorption = SpectrumRec(freqsHz: bands, values: n) }
                                }
                            }
                        }
                        GridRow {
                            Text("s").font(.wd.meta).foregroundStyle(Color.wd.scattering)
                            ForEach(0..<8, id: \.self) { i in
                                WDNumberField(s[i], UnitFormatter(.ratio), range: 0...1, showUnit: false) { v in
                                    var n = s; n[i] = v
                                    save(m) { $0.scattering = SpectrumRec(freqsHz: bands, values: n) }
                                }
                            }
                        }
                    }
                    .padding(.wd.s12)
                }
            }
        } else {
            WDEmptyState("Select a material").frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func save(_ m: MaterialRec, _ f: (inout MaterialRec) -> Void) {
        var r = m
        f(&r)
        ctx.edit("Edit Material") { try $0.upsertMaterial(material: r) }
    }
}
