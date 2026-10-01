import SwiftUI
import WaveyCore

struct ContentView: View {
    @Bindable var model: AppModel

    var body: some View {
        HSplitView {
            mainArea
                .frame(minWidth: 600, maxWidth: .infinity, maxHeight: .infinity)
            Inspector(model: model)
                .frame(minWidth: 260, idealWidth: 300, maxWidth: 420)
        }
        .toolbar {
            ToolbarItem(placement: .principal) {
                Picker("Mode", selection: $model.mode) {
                    ForEach(Mode.allCases) { m in
                        Label(m.rawValue, systemImage: m.systemImage).tag(m)
                    }
                }
                .pickerStyle(.segmented)
                .labelStyle(.titleAndIcon)
            }
        }
        .alert("Wavey", isPresented: Binding(
            get: { model.errorMessage != nil },
            set: { if !$0 { model.errorMessage = nil } })
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(model.errorMessage ?? "")
        }
    }

    @ViewBuilder private var mainArea: some View {
        switch model.mode {
        case .sketch:
            VStack(spacing: 0) {
                SketchToolbar(model: model)
                Divider()
                SketchCanvas(model: model)
            }
        default:
            ZStack {
                RoomViewport(model: model, onPickSurface: { key in
                    model.selectedSurface = key
                })
                if model.room == nil {
                    ContentUnavailableView(
                        "No room yet",
                        systemImage: "cube.transparent",
                        description: Text("Draw closed walls in Sketch mode, then use the Interior tool to select the area to simulate."))
                }
            }
        }
    }
}

struct SketchToolbar: View {
    @Bindable var model: AppModel

    var body: some View {
        HStack(spacing: 12) {
            Picker("Tool", selection: $model.tool) {
                ForEach(SketchTool.allCases) { t in
                    Label(t.rawValue, systemImage: t.systemImage).tag(t)
                }
            }
            .pickerStyle(.segmented)
            .labelStyle(.titleAndIcon)
            .fixedSize()
            Toggle("Snap to grid", isOn: $model.snapToGrid)
            Spacer()
            Text("Pan: two-finger scroll / right-drag   Zoom: pinch / ⌘-scroll   F: fit")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }
}
