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
    var selectedSurfaces: Set<String> = []
    private(set) var speakers: [SpeakerRec] = []
    private(set) var speakerModels: [SpeakerModelRec] = []
    private(set) var speakerModelsById: [String: SpeakerModelRec] = [:]
    var selectedSpeakers: Set<UInt64> = []
    /// When set, clicking the floor in Sources mode places this model.
    var placingModelId: String?
    var placementHeight = 1.5
    var showBalloon = true
    var balloonFrequency = 1000.0
    private(set) var materials: [MaterialRec] = []
    private(set) var materialsById: [String: MaterialRec] = [:]

    // Simulation
    private(set) var microphones: [MicrophoneRec] = []
    private(set) var cutPlanes: [CutPlaneRec] = []
    private(set) var simSettings: SimSettingsRec?
    private(set) var medium: MediumRec?
    private(set) var resultInfo: ResultInfo?
    private(set) var job: SimulationJob?
    private(set) var jobProgress: SimProgress?
    private var pollTimer: Timer?
    var selectedMics: Set<UInt64> = []
    /// When true, clicking a surface in Simulate mode adds a microphone.
    var placingMic = false

    // Results view
    var resultFrequency = 1000.0
    var resultBand: ResultBand = .thirdOctave
    var autoRange = true
    /// Show mic phase relative to the direct-sound arrival from the nearest speaker.
    var removeDelay = true
    var rangeMin = 60.0
    var rangeMax = 100.0
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
        if args.contains("--select-first-speaker"), let s = speakers.first {
            selectedSpeakers = [s.id]
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
        let keys = Set(room?.surfaces.map(\.key) ?? [])
        selectedSurfaces = selectedSurfaces.intersection(keys)
        speakers = doc.speakers()
        speakerModels = doc.speakerModels()
        speakerModelsById = Dictionary(uniqueKeysWithValues: speakerModels.map { ($0.id, $0) })
        selectedSpeakers = selectedSpeakers.filter { id in speakers.contains { $0.id == id } }
        microphones = doc.microphones()
        cutPlanes = doc.cutPlanes()
        simSettings = doc.simSettings()
        medium = doc.medium()
        resultInfo = doc.resultInfo()
        selectedMics = selectedMics.filter { id in microphones.contains { $0.id == id } }
        materials = doc.materials()
        materialsById = Dictionary(uniqueKeysWithValues: materials.map { ($0.id, $0) })
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
        selectedSurfaces = []
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
            selectedSurfaces = []
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
    /// Display color of a room surface: its material's color.
    func surfaceColor(_ key: String) -> NSColor {
        guard let id = room?.surfaces.first(where: { $0.key == key })?.materialId,
              let hex = materialsById[id]?.color else { return NSColor(white: 0.72, alpha: 1) }
        return NSColor(hex: hex) ?? NSColor(white: 0.72, alpha: 1)
    }

    /// Routes a 3D click according to the current mode.
    func handlePick(_ pick: Pick, extend: Bool) {
        switch (mode, pick) {
        case (.sources, .surface(_, let p)) where placingModelId != nil:
            let id = placingModelId!
            edit { doc in
                let s = try doc.addSpeaker(modelId: id, position: Point3(x: p.x, y: p.y, z: placementHeight))
                selectedSpeakers = [s.id]
            }
        case (.sources, .speaker(let id)):
            if extend { selectedSpeakers.formSymmetricDifference([id]) } else { selectedSpeakers = [id] }
        case (.sources, _):
            if !extend { selectedSpeakers = [] }
        case (.simulate, .surface(_, let p)) where placingMic:
            placingMic = false
            edit { doc in
                let id = doc.addMicrophone(position: Point3(x: p.x, y: p.y, z: 1.2))
                selectedMics = [id]
            }
        case (.simulate, .microphone(let id)), (.results, .microphone(let id)):
            if extend { selectedMics.formSymmetricDifference([id]) } else { selectedMics = [id] }
        case (.simulate, _), (.results, _):
            if !extend { selectedMics = [] }
        case (.model, .surface(let key, _)), (.materials, .surface(let key, _)):
            pickSurface(key, extend: extend)
        case (.model, _), (.materials, _):
            pickSurface(nil, extend: extend)
        default:
            break
        }
    }

    // MARK: Simulation

    func startSimulation() {
        do {
            job = try doc.startSimulation()
            jobProgress = job?.progress()
            pollTimer?.invalidate()
            pollTimer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated { self?.pollJob() }
            }
        } catch {
            errorMessage = describe(error)
        }
    }

    private func pollJob() {
        guard let job else { return }
        let p = job.progress()
        jobProgress = p
        if p.finished {
            pollTimer?.invalidate()
            pollTimer = nil
            self.job = nil
            if let e = p.error, e != "simulation cancelled" { errorMessage = e }
            refresh()
            if p.error == nil { mode = .results }
        }
    }

    func cancelSimulation() { job?.cancel() }

    var isSimulating: Bool { job != nil }

    func planeMode() -> PlaneModeRec {
        switch resultBand {
        case .single: .single(freqHz: resultFrequency)
        case .thirdOctave: .band(freqHz: resultFrequency, octaveFraction: 3)
        case .octave: .band(freqHz: resultFrequency, octaveFraction: 1)
        case .broadband: .broadband
        }
    }

    func updateSpeaker(_ s: SpeakerRec) {
        edit { try $0.updateSpeaker(speaker: s) }
    }

    func pickSurface(_ key: String?, extend: Bool) {
        guard let key else {
            if !extend { selectedSurfaces = [] }
            return
        }
        if extend {
            selectedSurfaces.formSymmetricDifference([key])
        } else {
            selectedSurfaces = [key]
        }
    }
}

enum ResultBand: String, CaseIterable, Identifiable {
    case single = "Single frequency"
    case thirdOctave = "1/3 octave"
    case octave = "Octave"
    case broadband = "Broadband"
    var id: String { rawValue }
}

extension NSColor {
    convenience init?(hex: String) {
        var s = hex.trimmingCharacters(in: .whitespaces)
        if s.hasPrefix("#") { s.removeFirst() }
        guard s.count == 6, let v = UInt32(s, radix: 16) else { return nil }
        self.init(srgbRed: CGFloat((v >> 16) & 0xff) / 255, green: CGFloat((v >> 8) & 0xff) / 255,
                  blue: CGFloat(v & 0xff) / 255, alpha: 1)
    }

    var hexString: String {
        let c = usingColorSpace(.sRGB) ?? self
        return String(format: "#%02x%02x%02x", Int(c.redComponent * 255), Int(c.greenComponent * 255), Int(c.blueComponent * 255))
    }
}
