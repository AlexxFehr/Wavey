import AppKit
import Foundation
import Observation
import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel
import WaveyViewport

/// Frequency display of cut planes.
public enum BandMode: String, Codable, Sendable, CaseIterable {
    case single, third, octave, peak
    public var title: String {
        switch self {
        case .single: "Single f"
        case .third: "1/3 oct"
        case .octave: "1/1 oct"
        case .peak: "Peak hold"
        }
    }
}

/// Results view state (window-level, not undoable).
@MainActor @Observable
public final class ResultsViewState {
    public var frequency = 63.0
    public var band: BandMode = .single
    public var smoothing = 0
    public var cells = false
    public var removeDelay = true
    public var hiddenMics: Set<UInt64> = []
    public var sweeping = false
    public var plotMode = "frequency"
    public init() {}
}

/// Transient status-bar message.
public struct StatusMessage: Equatable {
    public let text: String
    public let kind: DocumentStore.MessageKind
    public let at: Date
}

/// Everything one window needs. Views read the parts they render.
@MainActor @Observable
public final class WindowContext {
    public let store: DocumentStore
    public let selection = SelectionState()
    public let runs: RunCoordinator
    public let cache: ResultsCache
    public let results = ResultsViewState()

    public var workspace: Workspace = .geometry {
        didSet { if workspace != oldValue { workspaceChanged(from: oldValue) } }
    }
    public var ui: [Workspace: WorkspaceUIState] = Dictionary(uniqueKeysWithValues: Workspace.allCases.map { ($0, .defaults($0)) })
    public var leftVisible = true
    public var inspectorVisible = true
    public var activeTool: ToolID = .select
    public var palettePresented = false
    public var solveSettingsPresented = false
    public var runPopoverPresented = false
    public var issuesPresented = false
    public var materialLibraryPresented = false
    public var message: StatusMessage?
    /// Cursor world position (status bar).
    public var cursor: Vec3?
    /// Text typed into the command line (Geometry dock).
    public var commandHistory: [String] = []
    public var commandOutput: [(String, Int)] = []
    /// Current balloon frequency (Sources).
    public var balloonFrequency = 1000.0
    /// Grid step for plan snapping (m).
    public var gridStep = 0.5
    /// Plan level shown in Geometry.
    public var planLevel = "floor"

    /// Receiver-grid popover anchor (viewport points), when open.
    public var receiverGridPopover: CGPoint?
    /// Microphone line / grid settings, after its two clicks.
    public var micArrayPopover: MicArrayRequest?
    /// The library device being dragged (for the drop ghost) and where it would land.
    public var dragDevice: DragDevice?
    public var dropGhost: SIMD3<Double>?
    /// The variant last picked on each rail button.
    public var railChoice: [String: ToolID] = [:]
    /// Probe line (Results), not persisted.
    public var probeLine: (Vec3, Vec3)? {
        get { probe.map { ($0.a, $0.b) } }
        set { probe = newValue.map { ProbeSegment(a: $0.0, b: $0.1) } }
    }
    var probe: ProbeSegment?
    /// Marquee rectangle in viewport points while dragging.
    public var marquee: CGRect?
    public var marqueeCrossing = false
    /// Text being typed into the command line, and a request to focus it.
    public var commandDraft = ""
    public var focusCommandLine = false
    /// Device chosen in the library (placement tools use it).
    public var libraryDevice: String?
    /// Entity whose context menu is open, and where.
    public var contextTarget: ContextTarget?

    /// Opens a document in a new window (set by the app).
    @ObservationIgnored public var openRequested: ((URL) -> Void)?
    @ObservationIgnored private var insetSelection: [EntityRef] = []
    @ObservationIgnored let picks = PickTable()
    @ObservationIgnored let geometryCache = GeometryCache()
    @ObservationIgnored var balloonCache: [String: BalloonMeshRec] = [:]
    @ObservationIgnored var polarCache: [String: PolarRec] = [:]
    @ObservationIgnored var fieldMemo: [String: FieldValues] = [:]

    /// One viewport (camera) per workspace, plus the inset preview.
    public let viewports: [Workspace: ViewportController]
    public let inset: ViewportController
    private(set) var tool: ToolSession!

    public init(store: DocumentStore) {
        self.store = store
        runs = RunCoordinator(store: store)
        cache = ResultsCache(store: store)
        let pal = ViewportPalette.wavey
        var v: [Workspace: ViewportController] = [:]
        for ws in Workspace.allCases {
            let c = ViewportController(palette: pal)
            c.mode = WorkspaceUIState.defaults(ws).viewMode
            v[ws] = c
        }
        viewports = v
        inset = ViewportController(palette: pal)
        inset.passive = true
        tool = ToolSession(context: self)
        store.onMessage = { [weak self] text, kind in self?.post(text, kind) }
        for (ws, c) in viewports {
            c.onEvent = { [weak self] e in self?.tool.handle(e, in: ws) }
        }
        if let f = store.resultInfo?.freqsHz, !f.isEmpty {
            results.frequency = f.min(by: { abs(log($0 / 63)) < abs(log($1 / 63)) }) ?? 63
        }
        rebuildScenes()
    }

    public var viewport: ViewportController { viewports[workspace]! }

    public var state: WorkspaceUIState {
        get { ui[workspace]! }
        set { ui[workspace] = newValue }
    }

    public func flag(_ name: String) -> Bool { state.flags.contains(name) }
    public func toggleFlag(_ name: String) {
        if state.flags.contains(name) { state.flags.remove(name) } else { state.flags.insert(name) }
        rebuildScenes()
    }

    public func post(_ text: String, _ kind: DocumentStore.MessageKind = .info) {
        message = StatusMessage(text: text, kind: kind, at: Date())
        let at = message?.at
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(4))
            if self?.message?.at == at { self?.message = nil }
        }
    }

    private func workspaceChanged(from old: Workspace) {
        tool.cancel()
        saveUIState()
        // Tools that do not belong to the new workspace fall back to Select.
        if !railTools(workspace).flatMap({ $0 }).contains(activeTool) { activeTool = .select }
        rebuildScenes()
    }

    // MARK: Edits

    /// Runs a named edit, then prunes the selection and redraws.
    @discardableResult
    public func edit(_ name: String, coalesce: String? = nil, _ body: (WaveyDocument) throws -> Void) -> Bool {
        let ok = store.edit(name, coalesce: coalesce, body)
        afterChange()
        return ok
    }

    public func afterChange() {
        selection.prune { store.exists($0) }
        rebuildScenes()
    }

    public func undo() {
        if let tv = NSApp.keyWindow?.firstResponder as? NSTextView, tv.undoManager?.canUndo == true {
            tv.undoManager?.undo()
            return
        }
        guard tool.undoPoint() == false else { rebuildScenes(); return }
        let name = store.undoName
        store.undo()
        afterChange()
        if let name { post("Undo \(name)") }
    }

    public func redo() {
        if let tv = NSApp.keyWindow?.firstResponder as? NSTextView, tv.undoManager?.canRedo == true {
            tv.undoManager?.redo()
            return
        }
        let name = store.redoName
        store.redo()
        afterChange()
        if let name { post("Redo \(name)") }
    }

    // MARK: Selection and navigation

    public func select(_ e: [EntityRef], primary: EntityRef? = nil) {
        selection.select(e, primary: primary)
        rebuildScenes()
    }

    /// The inset preview frames the selection (or the whole model).
    func followInset() {
        if let (lo, hi) = SceneBuilder.bounds(of: selection.ids, store: store) {
            let pad = Vec3(repeating: 1.5)
            inset.frame(min: lo - pad, max: hi + pad)
        } else {
            inset.zoomToFit()
        }
    }

    /// Selects an entity, switching workspace if asked, and frames it.
    public func reveal(_ e: EntityRef, in ws: Workspace? = nil, frame: Bool = true) {
        if let ws, ws != workspace {
            workspace = ws
            post("Switched to \(ws.title)")
        }
        select([e], primary: e)
        if frame { frameSelection() }
    }

    public func frameSelection() {
        guard let (lo, hi) = SceneBuilder.bounds(of: selection.ids, store: store) else { return viewport.zoomToFit() }
        let pad = Vec3(repeating: 0.5)
        viewport.frame(min: lo - pad, max: hi + pad)
    }

    // MARK: Scenes

    /// Rebuilds the viewport scenes (model, selection, hover, tool overlays).
    public func rebuildScenes() {
        cache.sync()
        picks.reset()
        let builder = SceneBuilder(context: self)
        for ws in Workspace.allCases {
            viewports[ws]!.scene = builder.build(for: ws)
        }
        inset.scene = builder.build(for: .geometry, inset: true)
        inset.mode = .perspective
        if selection.ids != insetSelection {
            insetSelection = selection.ids
            followInset()
        }
    }

    /// Re-frames every viewport (after opening a document).
    public func fitAll() {
        for (ws, c) in viewports {
            if ws == .geometry, c.mode == .plan { c.zoomToFit() } else { c.zoomToFit() }
        }
        inset.zoomToFit()
    }
}

public extension ViewportPalette {
    static let wavey = ViewportPalette(
        planBackground: rgba(0x121417), centre: rgba(0x171A1E), edge: rgba(0x0F1012),
        gridMinor: rgba(0x1C1F23), gridMajor: rgba(0x24282D), gridMinor3D: rgba(0x20252B), gridAxis: rgba(0x2C3238))
}

/// sRGB hex to RGBA floats.
public func rgba(_ hex: UInt32, _ a: Float = 1) -> SIMD4<Float> {
    SIMD4(Float((hex >> 16) & 0xff) / 255, Float((hex >> 8) & 0xff) / 255, Float(hex & 0xff) / 255, a)
}

public func rgba(hex: String, _ a: Float = 1) -> SIMD4<Float> {
    var s = hex.trimmingCharacters(in: .whitespaces)
    if s.hasPrefix("#") { s.removeFirst() }
    guard s.count == 6, let v = UInt32(s, radix: 16) else { return rgba(0x9AA4AE, a) }
    return rgba(v, a)
}

public struct ContextTarget: Equatable {
    public var entity: EntityRef?
    public var point: CGPoint
}
