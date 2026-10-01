import Charts
import SwiftUI
import WaveyCore

extension AppModel {
    func currentPlaneMaps() -> [PlaneMapRec] {
        guard let info = resultInfo else { return [] }
        let mode = planeMode()
        return info.planeIds.compactMap { try? doc.planeMap(planeId: $0, mode: mode) }
    }

    func displayRange(_ maps: [PlaneMapRec]) -> ClosedRange<Double> {
        if !autoRange { return rangeMin...max(rangeMax, rangeMin + 1) }
        // Top of the scale at the 99th percentile, so near-field hot spots
        // right at a speaker do not wash out the rest of the map; a 30 dB
        // window below it shows coverage best.
        var values = maps.flatMap { $0.valuesDb.lazy.filter(\.isFinite) }
        guard !values.isEmpty else { return 70...100 }
        values.sort()
        let hi = Double(values[min(values.count - 1, Int(Double(values.count) * 0.99))])
        return (hi - 30).rounded(.down)...hi.rounded(.up)
    }
}

extension AppModel {
    /// With `removeDelay`, unwraps the phase after removing the direct-path
    /// delay from the nearest speaker, which makes the phase readable.
    func displayPhase(_ r: MicResponseRec) -> MicResponseRec {
        guard removeDelay, let mic = microphones.first(where: { $0.id == r.micId }) else { return r }
        let c = medium?.speedOfSound ?? 343
        let d = speakers.map { hypot(hypot($0.position.x - mic.position.x, $0.position.y - mic.position.y), $0.position.z - mic.position.z) }.min() ?? 0
        var out = r
        out.phaseDeg = zip(r.freqsHz, r.phaseDeg).map { f, ph in
            let v = ph + 360 * f * d / c
            return v - 360 * ((v + 180) / 360).rounded(.down)
        }
        return out
    }
}

struct ResultsSections: View {
    @Bindable var model: AppModel

    var body: some View {
        if let info = model.resultInfo {
            Section("Results") {
                if info.stale {
                    Label("The project changed since this run. Simulate again to update.", systemImage: "exclamationmark.triangle")
                        .foregroundStyle(.orange)
                }
                LabeledContent("Computed on", value: info.backend)
                LabeledContent("Time", value: String(format: "%.2f s", info.seconds))
                LabeledContent("Paths", value: "\(info.paths)")
                ForEach(info.notes, id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary) }
                Text("Speaker gain, delay and polarity changes apply instantly.").font(.caption).foregroundStyle(.secondary)
            }
            Section("Cut planes") {
                LabeledContent("Frequency") { FrequencySlider(value: $model.resultFrequency) }
                Picker("Show", selection: $model.resultBand) {
                    ForEach(ResultBand.allCases) { Text($0.rawValue).tag($0) }
                }
                Toggle("Automatic range", isOn: $model.autoRange)
                if !model.autoRange {
                    LabeledContent("Range") {
                        TextField("", value: $model.rangeMin, format: .number).frame(width: 50)
                        Text("–")
                        TextField("", value: $model.rangeMax, format: .number).frame(width: 50)
                        Text("dB").foregroundStyle(.secondary)
                    }
                }
                let maps = model.currentPlaneMaps()
                ForEach(maps, id: \.planeId) { m in
                    let name = model.cutPlanes.first { $0.id == m.planeId }?.name ?? "Plane"
                    LabeledContent(name, value: String(format: "%.1f – %.1f dB", m.minDb, m.maxDb))
                }
            }
            Section("Microphones") {
                let ids = info.micIds.filter { model.selectedMics.isEmpty || model.selectedMics.contains($0) }
                let responses = ids.compactMap { try? model.doc.micResponse(micId: $0, soloSpeaker: nil) }
                if responses.isEmpty {
                    Text("No microphones in this run.").foregroundStyle(.secondary)
                } else {
                    MicChart(model: model, responses: responses, phase: false).frame(height: 180)
                    MicChart(model: model, responses: responses.map { model.displayPhase($0) }, phase: true).frame(height: 120)
                    Toggle("Remove propagation delay", isOn: $model.removeDelay)
                    ForEach(responses, id: \.micId) { r in
                        let name = model.microphones.first { $0.id == r.micId }?.name ?? "Mic"
                        let i = r.freqsHz.firstIndex { $0 >= model.resultFrequency } ?? 0
                        LabeledContent(name, value: String(format: "%.1f dB @ %.0f Hz", r.splDb[i], r.freqsHz[i]))
                    }
                    Button("Export CSV…") { export(responses) }
                }
            }
            if info.hasLate, let id = (model.selectedMics.first ?? info.micIds.first),
               let m = try? model.doc.micMetrics(micId: id) {
                Section("Room acoustics – \(model.microphones.first { $0.id == id }?.name ?? "Mic")") {
                    MetricsTable(metrics: m)
                }
            }
        } else {
            Section("Results") {
                Text("No results yet. Set up receivers in Simulate mode and run.").foregroundStyle(.secondary)
            }
        }
    }

    private func export(_ responses: [MicResponseRec]) {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(model.title)-mics.csv"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        var text = "frequency_hz"
        for r in responses {
            let name = model.microphones.first { $0.id == r.micId }?.name ?? "mic\(r.micId)"
            text += ",\(name) spl_db,\(name) phase_deg"
        }
        text += "\n"
        for i in responses[0].freqsHz.indices {
            text += String(format: "%.3f", responses[0].freqsHz[i])
            for r in responses { text += String(format: ",%.3f,%.2f", r.splDb[i], r.phaseDeg[i]) }
            text += "\n"
        }
        do { try text.write(to: url, atomically: true, encoding: .utf8) } catch { model.errorMessage = error.localizedDescription }
    }
}

struct MicChart: View {
    var model: AppModel
    var responses: [MicResponseRec]
    var phase: Bool

    var body: some View {
        Chart {
            ForEach(responses, id: \.micId) { r in
                let name = model.microphones.first { $0.id == r.micId }?.name ?? "Mic"
                ForEach(r.freqsHz.indices, id: \.self) { i in
                    LineMark(x: .value("Hz", r.freqsHz[i]), y: .value(phase ? "°" : "dB", phase ? r.phaseDeg[i] : r.splDb[i]))
                        .foregroundStyle(by: .value("Mic", name))
                        .lineStyle(StrokeStyle(lineWidth: phase ? 1 : 1.5))
                }
            }
            RuleMark(x: .value("f", model.resultFrequency)).foregroundStyle(.secondary.opacity(0.5))
        }
        .chartXScale(domain: (responses[0].freqsHz.first ?? 20)...(responses[0].freqsHz.last ?? 20000), type: .log)
        .chartYScale(domain: phase ? -180...180 : yRange)
        .chartYAxisLabel(phase ? "phase (°)" : "SPL (dB)")
        .chartLegend(phase ? .hidden : .visible)
        .chartXAxis {
            AxisMarks(values: [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000]) { v in
                AxisGridLine()
                AxisValueLabel { if let f = v.as(Double.self) { Text(f >= 1000 ? "\(Int(f / 1000))k" : "\(Int(f))") } }
            }
        }
    }

    private var yRange: ClosedRange<Double> {
        let all = responses.flatMap(\.splDb).filter(\.isFinite)
        let hi = (all.max() ?? 100) + 3
        return max((all.min() ?? 40) - 3, hi - 60)...hi
    }
}

/// Colour scale legend for SPL maps.
struct ColorLegend: View {
    var range: ClosedRange<Double>

    var body: some View {
        VStack(alignment: .trailing, spacing: 4) {
            Text(String(format: "%.0f dB", range.upperBound)).font(.caption2).monospacedDigit()
            LinearGradient(colors: (0...10).reversed().map { i in
                let c = Colormap.color(Double(i) / 10)
                return Color(red: c.r, green: c.g, blue: c.b)
            }, startPoint: .top, endPoint: .bottom)
            .frame(width: 14, height: 160)
            .clipShape(RoundedRectangle(cornerRadius: 3))
            Text(String(format: "%.0f dB", range.lowerBound)).font(.caption2).monospacedDigit()
        }
        .padding(8)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 8))
    }
}

struct MetricsTable: View {
    var metrics: MetricsRec

    var body: some View {
        Grid(alignment: .trailing, horizontalSpacing: 8, verticalSpacing: 3) {
            GridRow {
                Text("Hz").foregroundStyle(.secondary)
                ForEach(metrics.bandsHz.indices, id: \.self) { i in
                    let f = metrics.bandsHz[i]
                    Text(f >= 1000 ? "\(Int(f / 1000))k" : "\(Int(f))").foregroundStyle(.secondary)
                }
            }
            row("T30 s", metrics.t30S, "%.2f")
            row("T20 s", metrics.t20S, "%.2f")
            row("EDT s", metrics.edtS, "%.2f")
            row("C80 dB", metrics.c80Db, "%.1f")
            row("D50 %", metrics.d50.map { $0 * 100 }, "%.0f")
            row("Eyring s", metrics.eyringS, "%.2f")
            row("Sabine s", metrics.sabineS, "%.2f")
        }
        .font(.caption.monospacedDigit())
    }

    private func row(_ label: String, _ values: [Double], _ fmt: String) -> some View {
        GridRow {
            Text(label).gridColumnAlignment(.leading)
            ForEach(values.indices, id: \.self) { i in
                Text(values[i].isFinite ? String(format: fmt, values[i]) : "–")
            }
        }
    }
}
