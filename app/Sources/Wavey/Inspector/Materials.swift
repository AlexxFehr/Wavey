import Charts
import SwiftUI
import WaveyCore

/// Standard band centers offered in the material editor.
let editorBands: [Double] = [63, 125, 250, 500, 1000, 2000, 4000, 8000]

extension SpectrumRec {
    var isScalar: Bool { freqsHz.isEmpty }

    func value(at f: Double) -> Double {
        guard !freqsHz.isEmpty else { return values.first ?? 0 }
        if f <= freqsHz[0] { return values[0] }
        if f >= freqsHz[freqsHz.count - 1] { return values[values.count - 1] }
        let i = freqsHz.lastIndex(where: { $0 <= f })!
        let t = (log(f) - log(freqsHz[i])) / (log(freqsHz[i + 1]) - log(freqsHz[i]))
        return values[i] + t * (values[i + 1] - values[i])
    }

    var summary: String {
        isScalar ? String(format: "%.2f", values.first ?? 0)
            : "\(freqsHz.count) bands, \(String(format: "%.2f", values.min() ?? 0))–\(String(format: "%.2f", values.max() ?? 0))"
    }
}

struct SurfaceSection: View {
    var model: AppModel

    var body: some View {
        Section("Surfaces") {
            let selected = model.room?.surfaces.filter { model.selectedSurfaces.contains($0.key) } ?? []
            HStack {
                Button("Walls") { select(.wall) }
                Button("Floor") { select(.floor) }
                Button("Ceiling") { select(.ceiling) }
                Button("All") { model.selectedSurfaces = Set(model.room?.surfaces.map(\.key) ?? []) }
            }
            .buttonStyle(.borderless)
            if selected.isEmpty {
                Text("Click surfaces in the view (Shift-click to add)").foregroundStyle(.secondary)
            } else {
                LabeledContent("Selected", value: "\(selected.count)")
                LabeledContent("Area", value: String(format: "%.2f m²", selected.reduce(0) { $0 + $1.area }))
                let ids = Set(selected.map(\.materialId))
                Picker("Material", selection: Binding(
                    get: { ids.count == 1 ? ids.first! : "" },
                    set: { id in
                        let keys = selected.map(\.key)
                        model.edit { try $0.assignMaterial(surfaceKeys: keys, materialId: id) }
                    })
                ) {
                    if ids.count != 1 { Text("Mixed").tag("") }
                    ForEach(model.materials, id: \.id) { m in
                        Text(m.name).tag(m.id)
                    }
                }
                if ids.count == 1, let m = model.materialsById[ids.first!] {
                    AbsorptionChart(material: m).frame(height: 140)
                }
            }
        }
    }

    private func select(_ kind: SurfaceKindRec) {
        model.selectedSurfaces = Set(model.room?.surfaces.filter { $0.kind == kind }.map(\.key) ?? [])
    }
}

struct AbsorptionChart: View {
    var material: MaterialRec

    var body: some View {
        let freqs = (0...60).map { 50 * pow(10, Double($0) / 60 * log10(200)) }  // 50 Hz – 10 kHz
        Chart {
            ForEach(freqs, id: \.self) { f in
                LineMark(x: .value("Hz", f), y: .value("α", material.absorption.value(at: f)), series: .value("", "α"))
                    .foregroundStyle(.blue)
                LineMark(x: .value("Hz", f), y: .value("s", material.scattering.value(at: f)), series: .value("", "s"))
                    .foregroundStyle(.orange)
                    .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 3]))
            }
        }
        .chartXScale(type: .log)
        .chartYScale(domain: 0...1)
        .chartXAxis {
            AxisMarks(values: [63, 125, 250, 500, 1000, 2000, 4000, 8000]) { v in
                AxisGridLine()
                AxisValueLabel { if let f = v.as(Double.self) { Text(f >= 1000 ? "\(Int(f / 1000))k" : "\(Int(f))") } }
            }
        }
        .chartForegroundStyleScale(["absorption α": Color.blue, "scattering s": Color.orange])
    }
}

struct MaterialLibrarySection: View {
    var model: AppModel
    @State private var editing: MaterialRec?

    var body: some View {
        Section("Material library") {
            ForEach(model.materials, id: \.id) { m in
                HStack {
                    Circle().fill(Color(nsColor: NSColor(hex: m.color) ?? .gray)).frame(width: 12, height: 12)
                    VStack(alignment: .leading) {
                        Text(m.name)
                        Text("α " + m.absorption.summary).font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    if m.builtin {
                        Image(systemName: "lock").foregroundStyle(.tertiary).help("Built-in")
                    }
                }
                .contentShape(Rectangle())
                .contextMenu {
                    Button("Duplicate") { duplicate(m) }
                    if !m.builtin {
                        Button("Edit…") { editing = m }
                        Button("Delete", role: .destructive) {
                            model.edit { try $0.deleteMaterial(id: m.id) }
                        }
                    }
                }
                .onTapGesture(count: 2) { if m.builtin { duplicate(m) } else { editing = m } }
            }
            Button("New material…") {
                editing = MaterialRec(
                    id: "material-\(Int(Date().timeIntervalSince1970))", name: "New material",
                    absorption: SpectrumRec(freqsHz: [125, 250, 500, 1000, 2000, 4000], values: [0.1, 0.1, 0.1, 0.1, 0.1, 0.1]),
                    scattering: SpectrumRec(freqsHz: [], values: [0.1]),
                    angleDependent: false, color: "#8899aa", builtin: false, source: nil)
            }
        }
        .sheet(item: $editing) { m in
            MaterialEditor(material: m) { saved in
                model.edit { try $0.upsertMaterial(material: saved) }
            }
        }
    }

    private func duplicate(_ m: MaterialRec) {
        var newId: String?
        model.edit { newId = try $0.duplicateMaterial(id: m.id) }
        if let id = newId, let copy = model.materialsById[id] { editing = copy }
    }
}

extension MaterialRec: Identifiable {}

struct MaterialEditor: View {
    @State var material: MaterialRec
    var onSave: (MaterialRec) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Material").font(.headline)
            Form {
                TextField("Name", text: $material.name)
                ColorPicker("Display color", selection: Binding(
                    get: { Color(nsColor: NSColor(hex: material.color) ?? .gray) },
                    set: { material.color = NSColor($0).hexString }))
                SpectrumEditor(title: "Absorption α", spectrum: $material.absorption)
                SpectrumEditor(title: "Scattering s", spectrum: $material.scattering)
                Toggle("Angle-dependent reflection (fit impedance to α)", isOn: $material.angleDependent)
                TextField("Source", text: Binding(get: { material.source ?? "" }, set: { material.source = $0.isEmpty ? nil : $0 }))
            }
            AbsorptionChart(material: material).frame(height: 160)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Save") { onSave(material); dismiss() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 560)
    }
}

/// Edits a spectrum either as one scalar or as an octave-band table.
struct SpectrumEditor: View {
    var title: String
    @Binding var spectrum: SpectrumRec

    var body: some View {
        VStack(alignment: .leading) {
            Picker(title, selection: Binding(
                get: { spectrum.isScalar },
                set: { scalar in
                    if scalar {
                        let mean = spectrum.values.reduce(0, +) / Double(max(spectrum.values.count, 1))
                        spectrum = SpectrumRec(freqsHz: [], values: [mean])
                    } else {
                        let v = spectrum.values.first ?? 0
                        spectrum = SpectrumRec(freqsHz: editorBands, values: editorBands.map { _ in v })
                    }
                })
            ) {
                Text("Single value").tag(true)
                Text("Per octave band").tag(false)
            }
            .pickerStyle(.segmented)
            if spectrum.isScalar {
                TextField("Value", value: Binding(get: { spectrum.values.first ?? 0 }, set: { spectrum.values = [min(max($0, 0), 1)] }),
                          format: .number.precision(.fractionLength(2)))
            } else {
                Grid(horizontalSpacing: 6) {
                    GridRow {
                        ForEach(spectrum.freqsHz.indices, id: \.self) { i in
                            let f = spectrum.freqsHz[i]
                            Text(f >= 1000 ? "\(Int(f / 1000))k" : "\(Int(f))").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    GridRow {
                        ForEach(spectrum.values.indices, id: \.self) { i in
                            TextField("", value: Binding(get: { spectrum.values[i] }, set: { spectrum.values[i] = min(max($0, 0), 1) }),
                                      format: .number.precision(.fractionLength(2)))
                                .multilineTextAlignment(.center)
                                .frame(width: 48)
                        }
                    }
                }
            }
        }
    }
}
