import SwiftUI
import WaveyCore

extension SlopeRec: CaseIterable, Identifiable {
    public static var allCases: [SlopeRec] { [.butterworth12, .butterworth24, .linkwitzRiley24, .linkwitzRiley48] }
    public var id: Self { self }
    var label: String {
        switch self {
        case .butterworth12: "BW 12 dB/oct"
        case .butterworth24: "BW 24 dB/oct"
        case .linkwitzRiley24: "LR 24 dB/oct"
        case .linkwitzRiley48: "LR 48 dB/oct"
        }
    }
}

extension FilterRec {
    var title: String {
        switch self {
        case .highPass: "High-pass"
        case .lowPass: "Low-pass"
        case .lowShelf: "Low shelf"
        case .highShelf: "High shelf"
        case .peak: "Peak EQ"
        }
    }

    var freq: Double {
        get {
            switch self {
            case let .highPass(f, _), let .lowPass(f, _), let .lowShelf(f, _), let .highShelf(f, _), let .peak(f, _, _): f
            }
        }
        set {
            switch self {
            case let .highPass(_, s): self = .highPass(freqHz: newValue, slope: s)
            case let .lowPass(_, s): self = .lowPass(freqHz: newValue, slope: s)
            case let .lowShelf(_, g): self = .lowShelf(freqHz: newValue, gainDb: g)
            case let .highShelf(_, g): self = .highShelf(freqHz: newValue, gainDb: g)
            case let .peak(_, g, q): self = .peak(freqHz: newValue, gainDb: g, q: q)
            }
        }
    }
}

/// Edits a speaker's filter list; `commit` is called after every change.
struct FilterList: View {
    @Binding var filters: [FilterRec]
    var commit: () -> Void

    var body: some View {
        ForEach(filters.indices, id: \.self) { i in
            FilterRow(filter: Binding(get: { filters[i] }, set: { filters[i] = $0 }), commit: commit) {
                filters.remove(at: i)
                commit()
            }
        }
        Menu("Add filter") {
            Button("High-pass (LR 24, 80 Hz)") { add(.highPass(freqHz: 80, slope: .linkwitzRiley24)) }
            Button("Low-pass (LR 24, 80 Hz)") { add(.lowPass(freqHz: 80, slope: .linkwitzRiley24)) }
            Button("Low shelf") { add(.lowShelf(freqHz: 60, gainDb: 6)) }
            Button("High shelf") { add(.highShelf(freqHz: 8000, gainDb: -3)) }
            Button("Peak EQ") { add(.peak(freqHz: 1000, gainDb: -3, q: 2)) }
        }
    }

    private func add(_ f: FilterRec) {
        filters.append(f)
        commit()
    }
}

private struct FilterRow: View {
    @Binding var filter: FilterRec
    var commit: () -> Void
    var remove: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(filter.title).fontWeight(.medium)
                Spacer()
                Button(role: .destructive, action: remove) { Image(systemName: "minus.circle") }.buttonStyle(.borderless)
            }
            HStack(spacing: 6) {
                field("Hz", Binding(get: { filter.freq }, set: { filter.freq = $0 }), width: 64)
                switch filter {
                case let .highPass(f, slope), let .lowPass(f, slope):
                    Picker("", selection: Binding(get: { slope }, set: { s in
                        if case .highPass = filter { filter = .highPass(freqHz: f, slope: s) } else { filter = .lowPass(freqHz: f, slope: s) }
                        commit()
                    })) {
                        ForEach(SlopeRec.allCases) { Text($0.label).tag($0) }
                    }
                    .labelsHidden()
                case let .lowShelf(f, g):
                    field("dB", Binding(get: { g }, set: { filter = .lowShelf(freqHz: f, gainDb: $0) }), width: 50)
                case let .highShelf(f, g):
                    field("dB", Binding(get: { g }, set: { filter = .highShelf(freqHz: f, gainDb: $0) }), width: 50)
                case let .peak(f, g, q):
                    field("dB", Binding(get: { g }, set: { filter = .peak(freqHz: f, gainDb: $0, q: q) }), width: 50)
                    field("Q", Binding(get: { q }, set: { filter = .peak(freqHz: f, gainDb: g, q: $0) }), width: 44)
                }
            }
        }
    }

    private func field(_ unit: String, _ value: Binding<Double>, width: CGFloat) -> some View {
        HStack(spacing: 2) {
            TextField("", value: value, format: .number.precision(.fractionLength(0...2)))
                .labelsHidden().frame(width: width).multilineTextAlignment(.trailing).onSubmit(commit)
            Text(unit).foregroundStyle(.secondary)
        }
    }
}
