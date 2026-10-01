import SwiftUI
import WaveyCore

struct Inspector: View {
    @Bindable var model: AppModel

    var body: some View {
        Form {
            RoomSection(model: model)
            if model.mode == .sketch {
                SelectionSection(model: model)
            }
            if model.mode != .sketch, let key = model.selectedSurface,
               let s = model.room?.surfaces.first(where: { $0.key == key }) {
                Section("Surface") {
                    LabeledContent("Kind", value: "\(s.kind)".capitalized)
                    LabeledContent("Area", value: String(format: "%.2f m²", s.area))
                    LabeledContent("Key", value: s.key)
                }
            }
        }
        .formStyle(.grouped)
    }
}

struct RoomSection: View {
    @Bindable var model: AppModel
    @State private var height = 3.0

    var body: some View {
        Section("Room") {
            LabeledContent("Ceiling height") {
                TextField("", value: $height, format: .number.precision(.fractionLength(2)))
                    .multilineTextAlignment(.trailing)
                    .frame(width: 70)
                    .onSubmit {
                        model.edit { try $0.setCeilingHeight(height: height) }
                    }
                Text("m")
            }
            if let room = model.room {
                LabeledContent("Floor area", value: String(format: "%.1f m²", room.floorArea))
                LabeledContent("Volume", value: String(format: "%.1f m³", room.volume))
                LabeledContent("Surface area", value: String(format: "%.1f m²", room.totalSurfaceArea))
                LabeledContent("Surfaces", value: "\(room.surfaces.count)")
            } else if let err = model.roomError {
                Text(err).foregroundStyle(.secondary)
            }
        }
        .onAppear { height = model.doc.ceilingHeight() }
        .onChange(of: model.revision) { height = model.doc.ceilingHeight() }
    }
}

struct SelectionSection: View {
    var model: AppModel

    var body: some View {
        Section("Selection") {
            let segs = model.sketch.segments.filter { model.selectedSegments.contains($0.id) }
            if segs.isEmpty {
                Text("Nothing selected").foregroundStyle(.secondary)
            } else {
                let total = segs.reduce(0.0) { $0 + hypot($1.pb.x - $1.pa.x, $1.pb.y - $1.pa.y) }
                LabeledContent("Segments", value: "\(segs.count)")
                LabeledContent("Total length", value: String(format: "%.3f m", total))
                Button("Delete", role: .destructive) {
                    let ids = Array(model.selectedSegments)
                    model.edit { $0.removeSegments(ids: ids) }
                }
            }
        }
    }
}
