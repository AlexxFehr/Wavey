import Foundation
import WaveyDesign
import WaveyViewport

/// The three peer workspaces. Nothing is gated behind another.
public enum Workspace: String, Codable, CaseIterable, Sendable {
    case geometry, sources, results

    public var title: String {
        switch self {
        case .geometry: "Geometry"
        case .sources: "Sources & receivers"
        case .results: "Results"
        }
    }

    public var icon: WDIconName {
        switch self {
        case .geometry: .surface
        case .sources: .speaker
        case .results: .chart
        }
    }
}

/// Tool identity is global so shared tools keep their state across workspaces.
public enum ToolID: String, Codable, Sendable, CaseIterable {
    case select, move
    case line, polyline, rectangle, arc, offset
    case extrude, surfaceFromLoop, split
    case measure
    case aim, omni, loudspeaker, subwoofer, receiver, receiverGrid
    case cutPlane, probeLine, microphone

    public var title: String {
        switch self {
        case .select: "Select"
        case .move: "Move"
        case .line: "Line"
        case .polyline: "Polyline"
        case .rectangle: "Rectangle"
        case .arc: "Arc"
        case .offset: "Offset"
        case .extrude: "Extrude"
        case .surfaceFromLoop: "Surface from loop"
        case .split: "Split surface"
        case .measure: "Measure"
        case .aim: "Aim at target"
        case .omni: "Omni point source"
        case .loudspeaker: "Loudspeaker"
        case .subwoofer: "Subwoofer"
        case .receiver: "Receiver"
        case .receiverGrid: "Receiver grid"
        case .cutPlane: "Cut plane"
        case .probeLine: "Probe line"
        case .microphone: "Microphone"
        }
    }

    public var icon: WDIconName {
        switch self {
        case .select: .select
        case .move: .move
        case .line: .line
        case .polyline: .polyline
        case .rectangle: .rectangle
        case .arc: .arc
        case .offset: .offset
        case .extrude: .extrude
        case .surfaceFromLoop: .surface
        case .split: .split
        case .measure: .measure
        case .aim: .aim
        case .omni: .pointSource
        case .loudspeaker: .speaker
        case .subwoofer: .subwoofer
        case .receiver: .microphone
        case .receiverGrid: .receiverGrid
        case .cutPlane: .cutPlane
        case .probeLine: .probeLine
        case .microphone: .microphone
        }
    }

    /// Single-key shortcut, scoped to the workspace whose rail owns the tool.
    public func key(in ws: Workspace) -> String? {
        switch (ws, self) {
        case (_, .select): "V"
        case (_, .move): "M"
        case (.geometry, .line): "L"
        case (.geometry, .polyline): "P"
        case (.geometry, .rectangle): "R"
        case (.geometry, .arc): "A"
        case (.geometry, .offset): "O"
        case (.geometry, .extrude): "E"
        case (.geometry, .surfaceFromLoop): "S"
        case (.geometry, .split): "X"
        case (.geometry, .measure), (.sources, .measure), (.results, .measure): "D"
        case (.sources, .aim): "A"
        case (.sources, .omni): "O"
        case (.sources, .loudspeaker): "L"
        case (.sources, .subwoofer): "B"
        case (.sources, .receiver): "R"
        case (.sources, .receiverGrid): "G"
        case (.results, .cutPlane): "C"
        case (.results, .probeLine): "P"
        case (.results, .microphone): "I"
        default: nil
        }
    }

    /// Command-line aliases.
    public var aliases: [String] {
        switch self {
        case .select: ["SELECT"]
        case .move: ["M", "MOVE"]
        case .line: ["L", "LINE"]
        case .polyline: ["PL", "PLINE", "POLYLINE"]
        case .rectangle: ["REC", "RECT", "RECTANGLE"]
        case .arc: ["A", "ARC"]
        case .offset: ["O", "OFFSET"]
        case .extrude: ["E", "EXT", "EXTRUDE"]
        case .surfaceFromLoop: ["S", "SURF", "SURFACE"]
        case .split: ["X", "SPLIT"]
        case .measure: ["D", "DIST", "MEASURE"]
        default: []
        }
    }
}

/// Rail groups per workspace (separators between groups).
public func railTools(_ ws: Workspace) -> [[ToolID]] {
    switch ws {
    case .geometry: [[.select, .move], [.line, .polyline, .rectangle, .arc, .offset], [.extrude, .surfaceFromLoop, .split], [.measure]]
    case .sources: [[.select, .move, .aim], [.omni, .loudspeaker, .subwoofer], [.receiver, .receiverGrid], [.measure]]
    case .results: [[.select, .move], [.cutPlane, .probeLine, .microphone], [.measure]]
    }
}

/// Per-workspace layout and view state, persisted in the document.
public struct WorkspaceUIState: Codable, Equatable, Sendable {
    public var leftWidth: Double = 240
    public var inspectorWidth: Double = 300
    public var dockHeight: Double
    public var dockVisible = true
    public var dockTab: String
    public var collapsed: Set<String> = []
    public var viewMode: ViewMode
    public var orbit = OrbitCamera()
    public var plan = PlanCamera()
    public var flags: Set<String>

    public static func defaults(_ ws: Workspace) -> WorkspaceUIState {
        switch ws {
        case .geometry:
            WorkspaceUIState(dockHeight: 96, dockTab: "command", collapsed: ["sources", "receivers", "groups", "library", "results"],
                             viewMode: .plan, flags: ["snap.end", "snap.mid", "snap.grid", "ortho", "inset", "dims"])
        case .sources:
            WorkspaceUIState(dockHeight: 190, dockTab: "sources", collapsed: ["layers", "surfaces", "levels", "results"],
                             viewMode: .perspective, flags: ["show.directivity", "show.aim", "show.receivers"])
        case .results:
            WorkspaceUIState(dockHeight: 260, dockTab: "magnitude", collapsed: ["layers", "surfaces", "levels", "library", "groups"],
                             viewMode: .perspective, flags: ["show.wire", "show.sources"])
        }
    }
}
