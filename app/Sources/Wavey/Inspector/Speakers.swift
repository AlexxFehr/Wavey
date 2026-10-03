import Charts
import SwiftUI
import UniformTypeIdentifiers
import WaveyCore

/// Editor for the selected speaker(s).
struct SpeakerSection: View {
    @Bindable var model: AppModel

    var body: some View {
        Section("Speaker") {
            let selected = model.speakers.filter { model.selectedSpeakers.contains($0.id) }
            if selected.count == 1, let s = selected.first {
                SpeakerEditor(model: model, speaker: s)
                    .id(s.id)
            } else if selected.count > 1 {
                LabeledContent("Selected", value: "\(selected.count) speakers")
                Button("Delete", role: .destructive) {
                    let ids = Array(model.selectedSpeakers)
                    model.edit { $0.removeSpeakers(ids: ids) }
                }
            } else if let id = model.placingModelId, let m = model.speakerModelsById[id] {
                Text("Click a surface to place “\(m.name)”.").foregroundStyle(.secondary)
                Toggle("Stand on floor", isOn: $model.placeOnFloor)
                if !model.placeOnFloor {
                    LabeledContent("Centre height") {
                        TextField("", value: $model.placementHeight, format: .number.precision(.fractionLength(2)))
                            .frame(width: 60).multilineTextAlignment(.trailing)
                        Text("m")
                    }
                }
                Button("Done placing") { model.placingModelId = nil }
            } else {
                Text("Choose a model below and click Place, or click a speaker to edit it.")
                    .foregroundStyle(.secondary)
            }
        }
    }
}

private struct SpeakerEditor: View {
    var model: AppModel
    @State var speaker: SpeakerRec

    var body: some View {
        TextField("Name", text: $speaker.name).onSubmit(commit)
        Picker("Model", selection: $speaker.modelId) {
            ForEach(model.speakerModels, id: \.id) { Text($0.name).tag($0.id) }
        }
        .onChange(of: speaker.modelId) { commit() }
        Toggle("Stand on floor", isOn: $speaker.onFloor).onChange(of: speaker.onFloor) { commit() }
        if speaker.onFloor {
            vector("Position x / y", [("x", \.position.x), ("y", \.position.y)], unit: "m")
            number("Bottom above floor", \.bottomHeightM, unit: "m")
            LabeledContent("Centre height", value: String(format: "%.3f m", speaker.position.z))
        } else {
            vector("Position x / y / z", [("x", \.position.x), ("y", \.position.y), ("z", \.position.z)], unit: "m")
        }
        vector("Yaw / pitch / roll", [("yaw", \.yawDeg), ("pitch", \.pitchDeg), ("roll", \.rollDeg)], unit: "°")
        number("Gain", \.gainDb, unit: "dB")
        number("Delay", \.delayMs, unit: "ms")
        Toggle("Invert polarity", isOn: $speaker.polarityInverted).onChange(of: speaker.polarityInverted) { commit() }
        FilterList(filters: $speaker.filters, commit: commit)
        if !speaker.filters.isEmpty || speaker.gainDb != 0 {
            let r = model.doc.speakerProcessing(speaker: speaker)
            ResponseChart(freqs: r.freqsHz, values: r.magnitudeDb, unit: "processing dB").frame(height: 100)
        }
        Toggle("Mute", isOn: $speaker.muted).onChange(of: speaker.muted) { commit() }
        HStack {
            Button("Duplicate") {
                let copy = speaker
                model.edit { doc in
                    var s = try doc.addSpeaker(modelId: copy.modelId,
                                               position: Point3(x: copy.position.x + 1, y: copy.position.y, z: copy.position.z),
                                               onFloor: copy.onFloor)
                    let id = s.id
                    s = copy
                    s.id = id
                    s.name = copy.name + " copy"
                    s.position.x += 1
                    try doc.updateSpeaker(speaker: s)
                }
            }
            Spacer()
            Button("Delete", role: .destructive) {
                let id = speaker.id
                model.edit { $0.removeSpeakers(ids: [id]) }
            }
        }
        if let m = model.speakerModelsById[speaker.modelId] {
            SpeakerModelDetails(model: model, info: m)
        }
    }

    private func commit() {
        model.updateSpeaker(speaker)
    }

    private func number(_ label: String, _ path: WritableKeyPath<SpeakerRec, Double>, unit: String) -> some View {
        LabeledContent(label) {
            TextField("", value: Binding(get: { speaker[keyPath: path] }, set: { speaker[keyPath: path] = $0 }),
                      format: .number.precision(.fractionLength(0...3)))
                .frame(width: 70).multilineTextAlignment(.trailing)
                .onSubmit(commit)
            Text(unit).foregroundStyle(.secondary)
        }
    }

    private func vector(_ label: String, _ parts: [(String, WritableKeyPath<SpeakerRec, Double>)], unit: String) -> some View {
        LabeledContent(label) {
            HStack(spacing: 4) {
                ForEach(parts, id: \.0) { name, path in
                    TextField(name, value: Binding(get: { speaker[keyPath: path] }, set: { speaker[keyPath: path] = $0 }),
                              format: .number.precision(.fractionLength(0...3)))
                        .frame(width: 56).multilineTextAlignment(.trailing)
                        .onSubmit(commit)
                        .labelsHidden()
                        .help(name)
                }
                Text(unit).foregroundStyle(.secondary)
            }
        }
    }
}

/// Response, polar plot and balloon controls for a speaker model.
struct SpeakerModelDetails: View {
    @Bindable var model: AppModel
    var info: SpeakerModelRec

    var body: some View {
        LabeledContent("Sensitivity", value: String(format: "%.1f dB @ %@, %.0f m, %.2f V", info.sensitivityDb,
                                                    hz(info.referenceHz), info.referenceDistanceM, info.referenceVoltageRms))
        if let lo = info.nominalMinHz, let hi = info.nominalMaxHz {
            LabeledContent("Nominal band", value: "\(hz(lo)) – \(hz(hi))")
        }
        LabeledContent("Range", value: String(format: "%.0f Hz – %.1f kHz", info.minHz, info.maxHz / 1000))
        if info.phaseEstimated {
            Label("Phase was not measured", systemImage: "exclamationmark.triangle").foregroundStyle(.orange)
        }
        if let r = try? model.doc.speakerResponse(modelId: info.id) {
            ResponseChart(freqs: r.freqsHz, values: r.magnitudeDb, unit: "dB SPL").frame(height: 120)
        }
        LabeledContent("Frequency") {
            FrequencySlider(value: $model.balloonFrequency)
        }
        let f = model.balloonFrequency(for: info)
        if f != model.balloonFrequency {
            Text("Outside this model's band; showing \(hz(f)).").font(.caption).foregroundStyle(.secondary)
        }
        if info.hasBalloon, let polar = try? model.doc.speakerPolar(modelId: info.id, freqHz: f) {
            PolarPlot(polar: polar).frame(height: 200)
        } else if !info.hasBalloon {
            Text("Omnidirectional").foregroundStyle(.secondary)
        }
        Toggle("Show balloon in view", isOn: $model.showBalloon)
    }
}

/// Log-scaled frequency slider (20 Hz – 20 kHz).
struct FrequencySlider: View {
    @Binding var value: Double

    var body: some View {
        HStack {
            Slider(value: Binding(get: { log10(value) }, set: { value = (pow(10, $0) * 10).rounded() / 10 }),
                   in: log10(20)...log10(20000))
            Text(value >= 1000 ? String(format: "%.1f kHz", value / 1000) : String(format: "%.0f Hz", value))
                .monospacedDigit().frame(width: 64, alignment: .trailing)
        }
    }
}

struct ResponseChart: View {
    var freqs: [Double]
    var values: [Double]
    var unit: String

    var body: some View {
        Chart {
            ForEach(Array(zip(freqs, values).enumerated()), id: \.offset) { _, p in
                LineMark(x: .value("Hz", p.0), y: .value(unit, p.1))
            }
        }
        .chartXScale(domain: 20...20000, type: .log)
        .chartYScale(domain: (values.max() ?? 0) - 40 ... (values.max() ?? 0) + 5)
        .chartXAxis {
            AxisMarks(values: [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000]) { v in
                AxisGridLine()
                AxisValueLabel { if let f = v.as(Double.self) { Text(f >= 1000 ? "\(Int(f / 1000))k" : "\(Int(f))") } }
            }
        }
        .chartYAxisLabel(unit)
    }
}

/// Horizontal (blue) and vertical (orange) polar response, 0 dB at the rim, −30 dB at the center.
struct PolarPlot: View {
    var polar: PolarRec
    let floorDb = -30.0

    var body: some View {
        Canvas { ctx, size in
            let c = CGPoint(x: size.width / 2, y: size.height / 2)
            let r = min(size.width, size.height) / 2 - 12
            for db in stride(from: 0.0, through: floorDb, by: -10) {
                let rr = r * (1 - db / floorDb)
                ctx.stroke(Path(ellipseIn: CGRect(x: c.x - rr, y: c.y - rr, width: 2 * rr, height: 2 * rr)),
                           with: .color(.secondary.opacity(0.3)))
            }
            for a in stride(from: 0.0, to: 360, by: 30) {
                var p = Path()
                p.move(to: c)
                p.addLine(to: CGPoint(x: c.x + r * sin(a * .pi / 180), y: c.y - r * cos(a * .pi / 180)))
                ctx.stroke(p, with: .color(.secondary.opacity(0.2)))
            }
            for (values, color) in [(polar.horizontalDb, Color.blue), (polar.verticalDb, Color.orange)] {
                var p = Path()
                for (i, a) in polar.anglesDeg.enumerated() {
                    let rr = r * max(0, 1 - max(values[i], floorDb) / floorDb)
                    // On-axis points up.
                    let pt = CGPoint(x: c.x + rr * sin(a * .pi / 180), y: c.y - rr * cos(a * .pi / 180))
                    if i == 0 { p.move(to: pt) } else { p.addLine(to: pt) }
                }
                p.closeSubpath()
                ctx.stroke(p, with: .color(color), lineWidth: 1.5)
            }
            ctx.draw(Text("0 dB").font(.caption2).foregroundStyle(.secondary), at: CGPoint(x: c.x + 14, y: c.y - r - 6))
        }
        .overlay(alignment: .bottomLeading) {
            HStack(spacing: 8) {
                Label("Horizontal", systemImage: "circle.fill").foregroundStyle(.blue)
                Label("Vertical", systemImage: "circle.fill").foregroundStyle(.orange)
            }
            .font(.caption2)
            .labelStyle(.titleAndIcon)
        }
    }
}

struct SpeakerLibrarySection: View {
    @Bindable var model: AppModel

    var body: some View {
        Section("Speaker models") {
            if model.speakerModels.isEmpty {
                Text("No speaker models in this project yet.").foregroundStyle(.secondary)
            }
            ForEach(model.speakerModels, id: \.id) { m in
                HStack {
                    VStack(alignment: .leading) {
                        Text(m.name)
                        Text(String(format: "%.0f dB @ %@ · %@", m.sensitivityDb, hz(m.referenceHz), m.hasBalloon ? "balloon" : "omni"))
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button(model.placingModelId == m.id ? "Placing…" : "Place") {
                        model.placingModelId = model.placingModelId == m.id ? nil : m.id
                        model.selectedSpeakers = []
                    }
                    .buttonStyle(.bordered)
                }
                .contextMenu {
                    Button("Export…") { export(m) }
                    Button("Remove", role: .destructive) {
                        model.edit { try $0.removeSpeakerModel(id: m.id) }
                    }
                }
            }
            HStack {
                Button("Import .waveyspk…") { importFile() }
                Button("Import table…") { importTable() }
            }
            .buttonStyle(.borderless)
        }
    }

    private func importFile() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "waveyspk") ?? .json, .json]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        model.edit { _ = try $0.importSpeakerFile(path: url.path) }
    }

    private func importTable() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.commaSeparatedText, .tabSeparatedText, .plainText]
        panel.message = "Columns: frequency_hz, theta_deg, phi_deg, magnitude_db_spl[, phase_deg] (1 m, 2.83 V reference)"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let name = url.deletingPathExtension().lastPathComponent
        model.edit { _ = try $0.importSpeakerTable(path: url.path, name: name, referenceDistanceM: 1, referenceVoltageRms: 2.83) }
    }

    private func export(_ m: SpeakerModelRec) {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(m.id).waveyspk"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        model.edit { try $0.exportSpeakerModel(id: m.id, path: url.path) }
    }
}

func hz(_ f: Double) -> String {
    f >= 1000 ? String(format: "%.1f kHz", f / 1000) : String(format: "%.0f Hz", f)
}

extension AppModel {
    /// The balloon/polar frequency for a model: the chosen frequency, or the
    /// centre of the model's nominal band if the choice lies outside it.
    func balloonFrequency(for m: SpeakerModelRec) -> Double {
        guard let lo = m.nominalMinHz, let hi = m.nominalMaxHz, !(lo...hi).contains(balloonFrequency) else {
            return balloonFrequency
        }
        return (lo * hi).squareRoot()
    }
}
