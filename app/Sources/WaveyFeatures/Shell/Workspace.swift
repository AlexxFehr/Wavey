import Foundation
import WaveyDesign
import WaveyViewport

/// The three peer workspaces. Nothing is gated behind another.
public enum Workspace: String, Codable, CaseIterable, Sendable {
    case geometry, sources, results

    public var title: String {
        switch self {
        case .geometry: "Geometry"
        case .sources: "Setup"
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
    case select, move, rotate
    case line, polyline, rectangle, arc, offset
    case extrude, surfaceFromLoop, split
    case measure
    case aim
    case microphone, micLine, micGrid
    case cutPlane, planeH, planeX, planeY, audienceArea, receiverGrid
    case probeLine
    case stackTool, lineArrayTool

    public var title: String {
        switch self {
        case .select: "Select"
        case .move: "Move"
        case .rotate: "Rotate"
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
        case .microphone: "Microphone"
        case .micLine: "Microphone line"
        case .micGrid: "Microphone grid"
        case .cutPlane: "Cut plane (on surface)"
        case .planeH: "Horizontal cut plane"
        case .planeX: "Vertical cut plane (X)"
        case .planeY: "Vertical cut plane (Y)"
        case .audienceArea: "Audience area (on floors)"
        case .receiverGrid: "Receiver grid…"
        case .probeLine: "Probe line"
        case .stackTool: "Speaker stack"
        case .lineArrayTool: "Line array"
        }
    }

    public var icon: WDIconName {
        switch self {
        case .select: .select
        case .move: .move
        case .rotate: .rotate
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
        case .microphone, .micLine: .microphone
        case .micGrid, .receiverGrid, .audienceArea: .receiverGrid
        case .cutPlane, .planeH, .planeX, .planeY: .cutPlane
        case .probeLine: .probeLine
        case .stackTool: .stack
        case .lineArrayTool: .lineArray
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
        case (.sources, .microphone), (.results, .microphone): "R"
        case (.sources, .cutPlane), (.results, .cutPlane): "C"
        case (.sources, .micGrid), (.results, .micGrid): "G"
        case (.results, .probeLine): "P"
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

/// A rail button: its tools (the first is the default; right-click or
/// press and hold for the others, the last picked stays on the button).
public struct RailItem: Hashable, Sendable {
    public let variants: [ToolID]
    public init(_ variants: ToolID...) { self.variants = variants }
    public var key: String { variants[0].rawValue }
}

/// Rail buttons per workspace (separators between groups). Speakers are
/// placed by dragging them from the Library; the rail holds the tools.
public func railItems(_ ws: Workspace) -> [[RailItem]] {
    let receivers = [RailItem(.microphone, .micLine, .micGrid), RailItem(.cutPlane, .planeH, .planeX, .planeY, .audienceArea, .receiverGrid)]
    switch ws {
    case .geometry:
        return [[RailItem(.select), RailItem(.move)], [.line, .polyline, .rectangle, .arc, .offset].map { RailItem($0) },
                [.extrude, .surfaceFromLoop, .split].map { RailItem($0) }, [RailItem(.measure)]]
    case .sources:
        return [[RailItem(.select), RailItem(.move), RailItem(.rotate), RailItem(.aim)], [RailItem(.stackTool), RailItem(.lineArrayTool)], receivers, [RailItem(.measure)]]
    case .results:
        return [[RailItem(.select), RailItem(.move), RailItem(.rotate)], receivers + [RailItem(.probeLine)], [RailItem(.measure)]]
    }
}

/// Every tool on a workspace's rail (variants included).
public func railTools(_ ws: Workspace) -> [[ToolID]] {
    railItems(ws).map { $0.flatMap(\.variants) }
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
