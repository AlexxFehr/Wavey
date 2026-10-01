import AppKit
import Observation
import UniformTypeIdentifiers
import WaveyCore

enum Mode: String, CaseIterable, Identifiable {
    case sketch = "Sketch"
    case model = "Model"
    case materials = "Materials"
    case sources = "Sources"
    case simulate = "Simulate"
    case results = "Results"

    var id: String { rawValue }

    var systemImage: String {
        switch self {
        case .sketch: "pencil.and.ruler"
        case .model: "cube"
        case .materials: "square.stack.3d.down.right"
        case .sources: "hifispeaker"
        case .simulate: "waveform.path"
        case .results: "chart.xyaxis.line"
        }
    }
}

enum SketchTool: String, CaseIterable, Identifiable {
    case select = "Select"
    case line = "Line"
    case rectangle = "Rectangle"
    case circle = "Circle"
    case region = "Interior"
    case erase = "Erase"

    var id: String { rawValue }

    var systemImage: String {
        switch self {
        case .select: "cursorarrow"
        case .line: "line.diagonal"
        case .rectangle: "rectangle"
        case .circle: "circle"
        case .region: "square.dashed.inset.filled"
        case .erase: "eraser"
        }
    }

    var help: String {
        switch self {
        case .select: "Select segments, drag vertices, drag empty space to pan"
        case .line: "Click points to draw walls. Type a length and press Return. Esc ends."
        case .rectangle: "Click two corners"
        case .circle: "Click center, then radius (pillars)"
        case .region: "Click inside a closed area to toggle whether it is simulated"
        case .erase: "Click a segment to delete it"
        }
    }
}

/// Owns the Rust document for one window and mirrors its state for SwiftUI.
@MainActor
@Observable
final class AppModel {
    private(set) var doc: WaveyDocument
    var fileURL: URL?
    var mode: Mode = .sketch
    var tool: SketchTool = .line
    var snapToGrid = true

    private(set) var sketch: SketchSnapshot
    private(set) var room: RoomSnapshot?
    private(set) var roomError: String?
    /// Bumped on every document change; views use it to rebuild caches.
    private(set) var revision = 0

    var selectedSegments: Set<UInt64> = []
    var selectedSurface: String?
    var errorMessage: String?

    init() {
        doc = WaveyDocument()
        sketch = SketchSnapshot(vertices: [], segments: [], regions: [])
        refresh()
        applyLaunchArguments()
    }

    /// `Wavey [project.wavey] [--mode <mode>]`, used for scripted launches.
    private func applyLaunchArguments() {
        let args = Array(CommandLine.arguments.dropFirst())
        if let path = args.first(where: { $0.hasSuffix(".wavey") }) {
            open(URL(fileURLWithPath: path))
        }
        if let i = args.firstIndex(of: "--mode"), i + 1 < args.count,
           let m = Mode.allCases.first(where: { $0.rawValue.lowercased() == args[i + 1].lowercased() }) {
            mode = m
        }
        if let i = args.firstIndex(of: "--snapshot"), i + 1 < args.count {
            Snapshot.schedule(model: self, to: args[i + 1])
        }
    }

    var title: String {
        fileURL?.deletingPathExtension().lastPathComponent ?? "Untitled"
    }

    func refresh() {
        sketch = doc.sketch()
        do {
            room = try doc.room()
            roomError = nil
        } catch {
            room = nil
            roomError = describe(error)
        }
        selectedSegments = selectedSegments.filter { id in sketch.segments.contains { $0.id == id } }
        if let key = selectedSurface, room?.surfaces.contains(where: { $0.key == key }) != true {
            selectedSurface = nil
        }
        revision += 1
    }

    /// Runs an edit against the document and refreshes the mirrored state.
    func edit(_ body: (WaveyDocument) throws -> Void) {
        do {
            try body(doc)
        } catch {
            errorMessage = describe(error)
        }
        refresh()
    }

    func undo() { if doc.undo() { refresh() } }
    func redo() { if doc.redo() { refresh() } }

    // MARK: Files

    func newDocument() {
        doc = WaveyDocument()
        fileURL = nil
        selectedSegments = []
        selectedSurface = nil
        refresh()
    }

    func openWithPanel() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.treatsFilePackagesAsDirectories = false
        panel.message = "Choose a .wavey project"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        open(url)
    }

    func open(_ url: URL) {
        do {
            doc = try WaveyDocument.open(path: url.path)
            fileURL = url
            selectedSegments = []
            selectedSurface = nil
            refresh()
        } catch {
            errorMessage = describe(error)
        }
    }

    func save() {
        guard let url = fileURL else { return saveAs() }
        write(to: url)
    }

    func saveAs() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(title).wavey"
        panel.canCreateDirectories = true
        guard panel.runModal() == .OK, var url = panel.url else { return }
        if url.pathExtension != "wavey" { url.appendPathExtension("wavey") }
        write(to: url)
    }

    private func write(to url: URL) {
        do {
            try doc.save(path: url.path)
            fileURL = url
        } catch {
            errorMessage = describe(error)
        }
    }
}

func describe(_ error: Error) -> String {
    if case let WaveyError.Failed(message) = error { return message }
    return error.localizedDescription
}

extension AppModel {
    /// Display color of a room surface.
    func surfaceColor(_ key: String) -> NSColor {
        if key.hasPrefix("floor") { return NSColor(white: 0.55, alpha: 1) }
        if key.hasPrefix("ceiling") { return NSColor(white: 0.85, alpha: 1) }
        return NSColor(white: 0.72, alpha: 1)
    }
}
