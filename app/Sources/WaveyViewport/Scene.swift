import CoreGraphics
import Foundation
import simd

public typealias Vec3 = SIMD3<Double>

/// How the viewport projects the scene.
public enum ViewMode: String, Codable, Sendable, CaseIterable {
    case plan, perspective, top, front, side
    public var is3D: Bool { self != .plan }
    public var isOrtho: Bool { self != .perspective }
}

/// Orbit camera for 3D views (z up). Angles in degrees.
public struct OrbitCamera: Codable, Equatable, Sendable {
    public var target: Vec3
    public var yaw: Double
    public var pitch: Double
    public var distance: Double
    public var fovDeg: Double
    public var orthographic: Bool

    public init(target: Vec3 = .zero, yaw: Double = 35, pitch: Double = 28, distance: Double = 20, fovDeg: Double = 35, orthographic: Bool = false) {
        self.target = target
        self.yaw = yaw
        self.pitch = pitch
        self.distance = distance
        self.fovDeg = fovDeg
        self.orthographic = orthographic
    }

    /// Unit vector from target to eye. Yaw 0 looks from −y towards +y;
    /// positive yaw swings the eye towards +x.
    public var back: Vec3 {
        let y = yaw * .pi / 180, p = pitch * .pi / 180
        return Vec3(cos(p) * sin(y), -cos(p) * cos(y), sin(p))
    }

    public var eye: Vec3 { target + back * distance }

    /// Frames a box with 10 % margin.
    public mutating func frame(min lo: Vec3, max hi: Vec3) {
        target = (lo + hi) / 2
        let r = simd_length(hi - lo) / 2
        distance = max(0.5, r * 1.1 / sin(fovDeg * .pi / 360))
    }
}

/// 2D plan camera (top-down, per level).
public struct PlanCamera: Codable, Equatable, Sendable {
    public var center: SIMD2<Double>
    public var pointsPerMetre: Double
    public init(center: SIMD2<Double> = .zero, pointsPerMetre: Double = 50) {
        self.center = center
        self.pointsPerMetre = pointsPerMetre
    }

    public mutating func frame(min lo: SIMD2<Double>, max hi: SIMD2<Double>, in size: CGSize) {
        center = (lo + hi) / 2
        let w = max(hi.x - lo.x, 0.5), h = max(hi.y - lo.y, 0.5)
        pointsPerMetre = min(max(min(Double(size.width) / w, Double(size.height) / h) * 0.8, 2), 4000)
    }
}

/// Visual role of a scene element (picks colours and widths in the renderer).
public enum DrawStyle: UInt8, Sendable {
    case normal, ghost, selected, hovered, dimmed, locked
}

public struct SurfaceDrawable: Sendable {
    public var id: UInt32
    /// Triangles, xyz.
    public var positions: [Float]
    /// RGBA 0–1.
    public var color: SIMD4<Float>
    public var style: DrawStyle
    public var normal: SIMD3<Float>
    public init(id: UInt32, positions: [Float], color: SIMD4<Float>, style: DrawStyle, normal: SIMD3<Float>) {
        self.id = id
        self.positions = positions
        self.color = color
        self.style = style
        self.normal = normal
    }
}

/// A batch of screen-space-width lines (pairs of xyz endpoints).
public struct LineBatch: Sendable {
    public var id: UInt32
    public var segments: [Float]
    public var color: SIMD4<Float>
    /// Points.
    public var width: Float
    public var dashed: Bool
    /// Drawn on top regardless of depth.
    public var overlay: Bool
    public init(id: UInt32 = 0, segments: [Float], color: SIMD4<Float>, width: Float = 1, dashed: Bool = false, overlay: Bool = false) {
        self.id = id
        self.segments = segments
        self.color = color
        self.width = width
        self.dashed = dashed
        self.overlay = overlay
    }
}

/// Triangle mesh in world space (cabinets, balloons, markers).
public struct MeshDrawable: Sendable {
    public var id: UInt32
    public var positions: [Float]
    /// Per-vertex RGBA, or empty to use `color`.
    public var colors: [Float]
    public var color: SIMD4<Float>
    public var lit: Bool
    public var depthWrite: Bool
    public init(id: UInt32, positions: [Float], colors: [Float] = [], color: SIMD4<Float>, lit: Bool = true, depthWrite: Bool = true) {
        self.id = id
        self.positions = positions
        self.colors = colors
        self.color = color
        self.lit = lit
        self.depthWrite = depthWrite
    }
}

/// Screen-constant marker (receivers, handles, snap markers).
public enum MarkerShape: UInt8, Sendable { case ring, dot, square, hollowSquare, triangle, cross, diamond }

public struct Marker: Sendable {
    public var id: UInt32
    public var position: SIMD3<Float>
    public var shape: MarkerShape
    /// Points.
    public var size: Float
    public var color: SIMD4<Float>
    public var fill: SIMD4<Float>
    public init(id: UInt32 = 0, position: SIMD3<Float>, shape: MarkerShape, size: Float, color: SIMD4<Float>, fill: SIMD4<Float> = .zero) {
        self.id = id
        self.position = position
        self.shape = shape
        self.size = size
        self.color = color
        self.fill = fill
    }
}

/// A scalar field on a rectangle in 3D, coloured through a colour map.
public struct FieldQuad: Sendable {
    public var id: UInt32
    /// Corner of cell (0,0) centre minus half a cell, and the u/v edge vectors.
    public var origin: SIMD3<Float>
    public var uAxis: SIMD3<Float>
    public var vAxis: SIMD3<Float>
    public var nu: Int
    public var nv: Int
    /// Row-major (v outer), NaN = transparent.
    public var values: [Float]
    public var range: ClosedRange<Float>
    public var colorMap: ColorMap
    public var opacity: Float
    public var contourStep: Float
    public var cells: Bool
    /// Identity of `values` (re-upload only when this changes).
    public var version: Int
    public init(id: UInt32, origin: SIMD3<Float>, uAxis: SIMD3<Float>, vAxis: SIMD3<Float>, nu: Int, nv: Int, values: [Float],
                range: ClosedRange<Float>, colorMap: ColorMap, opacity: Float, contourStep: Float, cells: Bool, version: Int) {
        self.id = id
        self.origin = origin
        self.uAxis = uAxis
        self.vAxis = vAxis
        self.nu = nu
        self.nv = nv
        self.values = values
        self.range = range
        self.colorMap = colorMap
        self.opacity = opacity
        self.contourStep = contourStep
        self.cells = cells
        self.version = version
    }
}

/// Text placed at a world anchor (drawn by SwiftUI, not Metal).
public struct Label: Sendable, Identifiable {
    public var id: String
    public var anchor: SIMD3<Float>
    public var text: String
    public var secondary: String?
    public var color: SIMD4<Float>
    /// Higher wins when labels overlap: selected > hovered > readouts > dimensions > codes.
    public var priority: Int
    public var plate: Bool
    /// Points from the anchor.
    public var offset: CGSize
    public init(id: String, anchor: SIMD3<Float>, text: String, secondary: String? = nil, color: SIMD4<Float>, priority: Int = 0,
                plate: Bool = true, offset: CGSize = .zero) {
        self.id = id
        self.anchor = anchor
        self.text = text
        self.secondary = secondary
        self.color = color
        self.priority = priority
        self.plate = plate
        self.offset = offset
    }
}

/// Everything the renderer draws: an immutable value built on the main
/// actor when the model, selection, hover or tool state changes.
public struct SceneSnapshot: Sendable {
    public var revision: Int = 0
    public var surfaces: [SurfaceDrawable] = []
    public var lines: [LineBatch] = []
    public var meshes: [MeshDrawable] = []
    public var markers: [Marker] = []
    public var fields: [FieldQuad] = []
    public var labels: [Label] = []
    /// Bounds for fitting (world).
    public var boundsMin: Vec3 = .init(-5, -5, 0)
    public var boundsMax: Vec3 = .init(5, 5, 3)
    /// z of the plan level and of the grid in 3D.
    public var gridZ: Double = 0
    /// Grid spacing (m); 0 = automatic.
    public var gridStep: Double = 0
    public var showGrid = true
    public init() {}
}

/// Pointer and camera events the viewport reports to its host.
public enum ViewportEvent: Sendable {
    case hover(world: Vec3?, pick: UInt32?, screen: CGPoint)
    case down(world: Vec3?, pick: UInt32?, screen: CGPoint, modifiers: ModifierSet, clickCount: Int)
    case drag(world: Vec3?, screen: CGPoint, start: CGPoint, modifiers: ModifierSet)
    case up(world: Vec3?, pick: UInt32?, screen: CGPoint, modifiers: ModifierSet)
    case marquee(rect: CGRect, crossing: Bool, picks: [UInt32], modifiers: ModifierSet)
    case key(String, keyCode: UInt16, modifiers: ModifierSet)
    case context(pick: UInt32?, screen: CGPoint)
    case exited
}

public struct ModifierSet: OptionSet, Sendable, Hashable {
    public let rawValue: Int
    public init(rawValue: Int) { self.rawValue = rawValue }
    public static let shift = ModifierSet(rawValue: 1)
    public static let command = ModifierSet(rawValue: 2)
    public static let option = ModifierSet(rawValue: 4)
    public static let control = ModifierSet(rawValue: 8)
}
