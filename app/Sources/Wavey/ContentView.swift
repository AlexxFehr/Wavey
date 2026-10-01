import SceneKit
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
                RoomViewport(
                    model: model,
                    onPick: { pick, extend in model.handlePick(pick, extend: extend) },
                    decorate: { root in buildDecorations(root) },
                    decorationRevision: decorationKey)
                if model.mode == .results, model.resultInfo != nil, !model.cutPlanes.isEmpty {
                    ColorLegend(range: model.displayRange(model.currentPlaneMaps()))
                        .padding(12)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
                }
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

extension ContentView {
    /// Changes whenever decorations need rebuilding (beyond document edits).
    var decorationKey: Int {
        var h = Hasher()
        h.combine(model.mode)
        h.combine(model.selectedSpeakers)
        h.combine(model.showBalloon)
        h.combine(model.balloonFrequency)
        h.combine(model.selectedMics)
        h.combine(model.resultFrequency)
        h.combine(model.resultBand)
        h.combine(model.autoRange)
        h.combine(model.rangeMin)
        h.combine(model.rangeMax)
        return h.finalize()
    }

    func buildDecorations(_ root: SCNNode) {
        var balloon: (UInt64, BalloonMeshRec)?
        if model.mode == .sources, model.showBalloon, model.selectedSpeakers.count == 1,
           let s = model.speakers.first(where: { $0.id == model.selectedSpeakers.first }),
           let mesh = try? model.doc.speakerBalloon(modelId: s.modelId, freqHz: model.balloonFrequency, floorDb: -30) {
            balloon = (s.id, mesh)
        }
        Decorations.speakers(model.speakers, models: model.speakerModelsById,
                             selected: model.mode == .sources ? model.selectedSpeakers : [],
                             balloon: balloon, into: root)
        if model.mode == .simulate || model.mode == .results {
            Decorations.microphones(model.microphones, selected: model.selectedMics, into: root)
        }
        if model.mode == .simulate, let room = model.room {
            model.cutPlanes.forEach { Decorations.planeOutline($0, room: room, into: root) }
        }
        if model.mode == .results {
            let maps = model.currentPlaneMaps()
            let range = model.displayRange(maps)
            maps.forEach { Decorations.heatmap($0, range: range, into: root) }
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
