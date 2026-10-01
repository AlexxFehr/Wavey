import SwiftUI
import WaveyCore

struct SimulateSections: View {
    @Bindable var model: AppModel

    var body: some View {
        if let s = model.simSettings {
            SettingsSection(model: model, settings: s).id(model.revision)
        }
        if let m = model.medium {
            AirSection(model: model, medium: m).id(model.revision)
        }
        MicrophonesSection(model: model)
        CutPlanesSection(model: model)
        RunSection(model: model)
    }
}

private let fractions: [(String, UInt32)] = [("1/1 octave", 1), ("1/3 octave", 3), ("1/6 octave", 6),
                                             ("1/12 octave", 12), ("1/24 octave", 24), ("1/48 octave", 48), ("Linear", 0)]

private struct SettingsSection: View {
    var model: AppModel
    @State var settings: SimSettingsRec

    var body: some View {
        Section("Frequency range") {
            LabeledContent("From") { hz(\.fMin) }
            LabeledContent("To") { hz(\.fMax) }
            Picker("Resolution", selection: $settings.octaveFraction) {
                ForEach(fractions, id: \.1) { Text($0.0).tag($0.1) }
            }
            .onChange(of: settings.octaveFraction) { commit() }
            if settings.octaveFraction == 0 {
                LabeledContent("Step") { hz(\.linearStepHz) }
            }
        }
        Section("Method") {
            Stepper("Reflection order: \(settings.maxOrder)", value: $settings.maxOrder, in: 0...12)
                .onChange(of: settings.maxOrder) { commit() }
            Toggle("Use GPU", isOn: $settings.useGpu).onChange(of: settings.useGpu) { commit() }
            Toggle("Late reverberation (ray tracing)", isOn: $settings.lateReverb).onChange(of: settings.lateReverb) { commit() }
            if settings.lateReverb {
                LabeledContent("Rays per source") {
                    TextField("", value: $settings.raysPerSource, format: .number).frame(width: 90)
                        .multilineTextAlignment(.trailing).onSubmit(commit)
                }
            }
            Toggle("Low-frequency wave solver (FDTD)", isOn: $settings.fdtdEnabled).onChange(of: settings.fdtdEnabled) { commit() }
            if settings.fdtdEnabled {
                LabeledContent("Crossover") { hz(\.fdtdCrossoverHz) }
                LabeledContent("Duration") {
                    TextField("", value: $settings.fdtdDurationS, format: .number.precision(.fractionLength(0...2)))
                        .frame(width: 70).multilineTextAlignment(.trailing).onSubmit(commit)
                    Text("s").foregroundStyle(.secondary)
                }
            }
        }
    }

    private func hz(_ path: WritableKeyPath<SimSettingsRec, Double>) -> some View {
        HStack {
            TextField("", value: Binding(get: { settings[keyPath: path] }, set: { settings[keyPath: path] = $0 }),
                      format: .number.precision(.fractionLength(0...1)))
                .frame(width: 80).multilineTextAlignment(.trailing).onSubmit(commit)
            Text("Hz").foregroundStyle(.secondary)
        }
    }

    private func commit() {
        let s = settings
        model.edit { try $0.setSimSettings(settings: s) }
    }
}

private struct AirSection: View {
    var model: AppModel
    @State var medium: MediumRec

    var body: some View {
        Section("Air") {
            field("Temperature", \.temperatureC, "°C")
            field("Humidity", \.relativeHumidityPct, "%")
            field("Pressure", \.pressureKpa, "kPa")
            LabeledContent("Speed of sound", value: String(format: "%.1f m/s", medium.speedOfSound))
        }
    }

    private func field(_ label: String, _ path: WritableKeyPath<MediumRec, Double>, _ unit: String) -> some View {
        LabeledContent(label) {
            TextField("", value: Binding(get: { medium[keyPath: path] }, set: { medium[keyPath: path] = $0 }),
                      format: .number.precision(.fractionLength(0...2)))
                .frame(width: 70).multilineTextAlignment(.trailing)
                .onSubmit {
                    let m = medium
                    model.edit { try $0.setMedium(medium: m) }
                }
            Text(unit).foregroundStyle(.secondary)
        }
    }
}

private struct MicrophonesSection: View {
    @Bindable var model: AppModel

    var body: some View {
        Section("Microphones") {
            ForEach(model.microphones, id: \.id) { m in
                MicRow(model: model, mic: m).id("\(m.id)-\(model.revision)")
            }
            HStack {
                Button(model.placingMic ? "Click a surface…" : "Place microphone") { model.placingMic.toggle() }
                Spacer()
                if !model.selectedMics.isEmpty {
                    Button("Delete selected", role: .destructive) {
                        let ids = Array(model.selectedMics)
                        model.edit { $0.removeMicrophones(ids: ids) }
                    }
                }
            }
            .buttonStyle(.borderless)
        }
    }
}

private struct MicRow: View {
    var model: AppModel
    @State var mic: MicrophoneRec

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            TextField("Name", text: $mic.name).onSubmit(commit)
                .fontWeight(model.selectedMics.contains(mic.id) ? .semibold : .regular)
            HStack(spacing: 4) {
                ForEach([("x", \MicrophoneRec.position.x), ("y", \.position.y), ("z", \.position.z)], id: \.0) { name, path in
                    TextField(name, value: Binding(get: { mic[keyPath: path] }, set: { mic[keyPath: path] = $0 }),
                              format: .number.precision(.fractionLength(0...2)))
                        .labelsHidden().frame(width: 56).multilineTextAlignment(.trailing).onSubmit(commit)
                }
                Text("m").foregroundStyle(.secondary)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture { model.selectedMics = [mic.id] }
    }

    private func commit() {
        let m = mic
        model.edit { try $0.updateMicrophone(mic: m) }
    }
}

private struct CutPlanesSection: View {
    var model: AppModel

    var body: some View {
        Section("Cut planes") {
            ForEach(model.cutPlanes, id: \.id) { p in
                PlaneRow(model: model, plane: p).id("\(p.id)-\(model.revision)")
            }
            Menu("Add cut plane") {
                Button("Horizontal (ear height)") { model.edit { _ = $0.addCutPlane(axis: .horizontal, offset: 1.2, spacing: 0.2) } }
                Button("Vertical, constant x") { add(.constX) }
                Button("Vertical, constant y") { add(.constY) }
            }
        }
    }

    private func add(_ axis: PlaneAxisRec) {
        guard let r = model.room else { return }
        let mid = axis == .constX ? (r.boundsMin.x + r.boundsMax.x) / 2 : (r.boundsMin.y + r.boundsMax.y) / 2
        model.edit { _ = $0.addCutPlane(axis: axis, offset: mid, spacing: 0.2) }
    }
}

private struct PlaneRow: View {
    var model: AppModel
    @State var plane: CutPlaneRec

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                TextField("Name", text: $plane.name).onSubmit(commit)
                Button(role: .destructive) {
                    let id = plane.id
                    model.edit { $0.removeCutPlanes(ids: [id]) }
                } label: { Image(systemName: "trash") }
                .buttonStyle(.borderless)
            }
            HStack {
                Text(plane.axis == .horizontal ? "z =" : plane.axis == .constX ? "x =" : "y =").foregroundStyle(.secondary)
                TextField("", value: $plane.offset, format: .number.precision(.fractionLength(0...2)))
                    .labelsHidden().frame(width: 56).multilineTextAlignment(.trailing).onSubmit(commit)
                Text("spacing").foregroundStyle(.secondary)
                TextField("", value: $plane.spacing, format: .number.precision(.fractionLength(0...3)))
                    .labelsHidden().frame(width: 56).multilineTextAlignment(.trailing).onSubmit(commit)
                Text("m").foregroundStyle(.secondary)
            }
        }
    }

    private func commit() {
        let p = plane
        model.edit { try $0.updateCutPlane(plane: p) }
    }
}

private struct RunSection: View {
    var model: AppModel

    var body: some View {
        Section("Run") {
            if let e = try? model.doc.estimate() {
                LabeledContent("Sources × receivers", value: "\(e.sources) × \(e.receivers)")
                LabeledContent("Frequencies", value: "\(e.frequencies)")
                LabeledContent("Images per source", value: "\(e.imagesPerSource)")
                LabeledContent("Result size", value: String(format: "%.0f MB", e.resultMegabytes))
            }
            if let p = model.jobProgress, model.isSimulating {
                ProgressView(value: p.fraction) { Text(p.stage).font(.caption) }
                Button("Cancel", role: .cancel) { model.cancelSimulation() }
            } else {
                Button {
                    model.startSimulation()
                } label: {
                    Label("Simulate", systemImage: "play.fill").frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .disabled(model.room == nil || model.speakers.isEmpty)
            }
        }
    }
}
