import Foundation
import Observation
@preconcurrency import WaveyCore

public typealias Speaker = SpeakerRec
public typealias Microphone = MicrophoneRec
public typealias CutPlane = CutPlaneRec

/// The document model for the UI: owns the Rust document, mirrors its state
/// as plain values for views, and is the single path for edits (each one a
/// named, possibly coalesced, undo step).
@MainActor @Observable
public final class DocumentStore {
    public private(set) var doc: WaveyDocument
    public private(set) var fileURL: URL?
    public private(set) var savedAt: Date?
    public private(set) var saving = false

    // Mirrored state
    public private(set) var sketch = SketchSnapshot(vertices: [], segments: [], regions: [])
    public private(set) var room: RoomSnapshot?
    public private(set) var roomError: String?
    public private(set) var layers: [LayerRec] = []
    public private(set) var currentLayer = "walls"
    public private(set) var levels: [LevelRec] = []
    public private(set) var materials: [MaterialRec] = []
    public private(set) var speakerModels: [SpeakerModelRec] = []
    public private(set) var speakers: [Speaker] = []
    public private(set) var microphones: [Microphone] = []
    public private(set) var cutPlanes: [CutPlane] = []
    public private(set) var groups: [GroupRec] = []
    public private(set) var runs: [RunRec] = []
    public private(set) var resultInfo: ResultInfo?
    public private(set) var simSettings: SimSettingsRec
    public private(set) var medium: MediumRec
    public private(set) var issues: [IssueRec] = []
    public private(set) var changesSinceRun: [String] = []
    public private(set) var undoName: String?
    public private(set) var redoName: String?
    public private(set) var dirty = false
    /// Cabinet meshes by model id (cached; models are immutable once added).
    public private(set) var cabinetMeshes: [String: CabinetMeshRec] = [:]

    /// Bumps on every refresh; per-area counters bump only when that area changed.
    public private(set) var revision = 0
    public private(set) var geometryRevision = 0
    public private(set) var sourcesRevision = 0
    public private(set) var receiversRevision = 0
    public private(set) var resultsRevision = 0

    // Lookups
    public private(set) var materialsById: [String: MaterialRec] = [:]
    public private(set) var modelsById: [String: SpeakerModelRec] = [:]
    public private(set) var speakersById: [UInt64: Speaker] = [:]
    public private(set) var micsById: [UInt64: Microphone] = [:]
    public private(set) var planesById: [UInt64: CutPlane] = [:]
    public private(set) var surfacesByKey: [String: SurfaceMesh] = [:]
    public private(set) var segmentsById: [UInt64: SketchSegment] = [:]
    public private(set) var layersById: [String: LayerRec] = [:]
    public private(set) var groupsById: [UInt64: GroupRec] = [:]

    /// Called with user-facing messages (errors, confirmations) for the status bar.
    public var onMessage: ((String, MessageKind) -> Void)?

    public enum MessageKind: Sendable { case info, warning, error }

    public init() {
        let d = WaveyDocument()
        doc = d
        simSettings = d.simSettings()
        medium = d.medium()
        refresh()
    }

    public init(opening url: URL) throws {
        let d = try WaveyDocument.open(path: url.path)
        doc = d
        fileURL = url
        simSettings = d.simSettings()
        medium = d.medium()
        refresh()
    }

    public var title: String { fileURL?.deletingPathExtension().lastPathComponent ?? "Untitled" }

    // MARK: Refresh

    /// Re-reads the document's state. Cheap enough to call after every edit.
    public func refresh() {
        let newSketch = doc.sketch()
        var newRoom: RoomSnapshot?
        var newRoomError: String?
        do { newRoom = try doc.room() } catch { newRoomError = describe(error) }
        let geomChanged = newSketch != sketch || newRoom != room || newRoomError != roomError
        sketch = newSketch
        room = newRoom
        roomError = newRoomError
        layers = doc.layers()
        currentLayer = doc.currentLayer()
        levels = doc.levels()
        let newMaterials = doc.materials()
        if newMaterials != materials { geometryChangedMaterials(newMaterials) }

        let newModels = doc.speakerModels()
        if newModels != speakerModels {
            speakerModels = newModels
            modelsById = Dictionary(uniqueKeysWithValues: newModels.map { ($0.id, $0) })
            for m in newModels where cabinetMeshes[m.id] == nil {
                cabinetMeshes[m.id] = try? doc.cabinetMesh(modelId: m.id)
            }
        }
        let newSpeakers = doc.speakers()
        let newGroups = doc.groups()
        let sourcesChanged = newSpeakers != speakers || newGroups != groups
        speakers = newSpeakers
        groups = newGroups
        let newMics = doc.microphones()
        let newPlanes = doc.cutPlanes()
        let receiversChanged = newMics != microphones || newPlanes != cutPlanes
        microphones = newMics
        cutPlanes = newPlanes
        let newRuns = doc.runs()
        let newInfo = doc.resultInfo()
        let resultsChanged = newRuns != runs || newInfo != resultInfo
        runs = newRuns
        resultInfo = newInfo
        simSettings = doc.simSettings()
        medium = doc.medium()
        issues = doc.issues()
        changesSinceRun = doc.changesSinceRun()
        undoName = doc.undoName()
        redoName = doc.redoName()
        dirty = doc.isDirty()

        speakersById = Dictionary(uniqueKeysWithValues: speakers.map { ($0.id, $0) })
        micsById = Dictionary(uniqueKeysWithValues: microphones.map { ($0.id, $0) })
        planesById = Dictionary(uniqueKeysWithValues: cutPlanes.map { ($0.id, $0) })
        surfacesByKey = Dictionary(uniqueKeysWithValues: (room?.surfaces ?? []).map { ($0.key, $0) })
        segmentsById = Dictionary(uniqueKeysWithValues: sketch.segments.map { ($0.id, $0) })
        layersById = Dictionary(uniqueKeysWithValues: layers.map { ($0.id, $0) })
        groupsById = Dictionary(uniqueKeysWithValues: groups.map { ($0.id, $0) })

        revision += 1
        if geomChanged { geometryRevision += 1 }
        if sourcesChanged { sourcesRevision += 1 }
        if receiversChanged { receiversRevision += 1 }
        if resultsChanged || sourcesChanged { resultsRevision += 1 }
    }

    private func geometryChangedMaterials(_ m: [MaterialRec]) {
        materials = m
        materialsById = Dictionary(uniqueKeysWithValues: m.map { ($0.id, $0) })
        geometryRevision += 1
    }

    // MARK: Edits

    /// Runs one edit as a named undo step. Consecutive edits with the same
    /// `coalesce` key within 800 ms merge (scrubbing, dragging, typing).
    /// Returns false (and reports) on failure.
    @discardableResult
    public func edit(_ name: String, coalesce: String? = nil, _ body: (WaveyDocument) throws -> Void) -> Bool {
        doc.labelNextEdit(name: name, coalesceKey: coalesce)
        defer { refresh() }
        do {
            try body(doc)
            return true
        } catch {
            onMessage?(describe(error), .error)
            return false
        }
    }

    public var canUndo: Bool { undoName != nil }
    public var canRedo: Bool { redoName != nil }

    public func undo() {
        if doc.undo() { refresh() }
    }

    public func redo() {
        if doc.redo() { refresh() }
    }

    // MARK: Files

    public func save(to url: URL? = nil) {
        guard let target = url ?? fileURL else { return }
        saving = true
        defer { saving = false }
        do {
            try doc.save(path: target.path)
            fileURL = target
            savedAt = Date()
            refresh()
        } catch {
            onMessage?(describe(error), .error)
        }
    }

    // MARK: Queries

    public func exists(_ e: EntityRef) -> Bool {
        switch e {
        case .surface(let k): surfacesByKey[k] != nil
        case .segment(let id): segmentsById[id] != nil
        case .speaker(let id): speakersById[id] != nil
        case .microphone(let id): micsById[id] != nil
        case .plane(let id): planesById[id] != nil
        case .group(let id): groupsById[id] != nil
        case .layer(let id): layersById[id] != nil
        case .level(let id): levels.contains { $0.id == id }
        case .material(let id): materialsById[id] != nil
        case .device(let id): modelsById[id] != nil
        case .run(let id): runs.contains { $0.id == id }
        }
    }

    /// Short code ("W-07", "S1", "M2", "CP-1") or a fallback label.
    public func code(_ e: EntityRef) -> String {
        switch e {
        case .surface(let k): surfacesByKey[k]?.code ?? k
        case .segment(let id): "E\(id)"
        case .speaker(let id): speakersById[id]?.code ?? "S?"
        case .microphone(let id): micsById[id]?.code ?? "M?"
        case .plane(let id): planesById[id]?.code ?? "CP?"
        case .group(let id): groupsById[id]?.code ?? "G?"
        case .layer(let id): layersById[id]?.name ?? id
        case .level(let id): id
        case .material(let id): materialsById[id]?.name ?? id
        case .device(let id): modelsById[id]?.name ?? id
        case .run(let id): String(format: "Run %03d", id)
        }
    }

    /// Display name of an entity.
    public func name(_ e: EntityRef) -> String {
        if case .surface(let k) = e {
            guard let s = surfacesByKey[k] else { return k }
            return s.name.isEmpty ? defaultSurfaceName(s) : s.name
        }
        return switch e {
        case .surface(let k): k
        case .segment(let id): "Edge \(id)"
        case .speaker(let id): speakersById[id]?.name ?? ""
        case .microphone(let id): micsById[id]?.name ?? ""
        case .plane(let id): planesById[id]?.name ?? ""
        case .group(let id): groupsById[id]?.name ?? ""
        case .layer(let id): layersById[id]?.name ?? id
        case .level(let id): levels.first { $0.id == id }?.name ?? id
        case .material(let id): materialsById[id]?.name ?? id
        case .device(let id): modelsById[id]?.name ?? id
        case .run(let id): runs.first { $0.id == id }?.name ?? code(e)
        }
    }

    public func defaultSurfaceName(_ s: SurfaceMesh) -> String {
        switch s.kind {
        case .wall: "Wall"
        case .floor: "Floor"
        case .ceiling: "Ceiling"
        }
    }

    public var activeRun: RunRec? { runs.first { $0.active } }

    /// The first engine error that makes a solve impossible.
    public var blockingIssue: IssueRec? { issues.first { $0.severity == .error } }
    public var warnings: [IssueRec] { issues.filter { $0.severity == .warning } }

    public func material(for surface: SurfaceMesh) -> MaterialRec? { materialsById[surface.materialId] }
}

public func describe(_ error: Error) -> String {
    if case let WaveyError.Failed(message) = error { return message }
    return error.localizedDescription
}
