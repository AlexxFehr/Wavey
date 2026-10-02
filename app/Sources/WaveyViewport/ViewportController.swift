import AppKit
import MetalKit
import Observation
import SwiftUI

/// Camera, scene and input state of one viewport. The host sets `scene`
/// and receives `ViewportEvent`s; navigation is handled here.
@MainActor @Observable
public final class ViewportController {
    public var mode: ViewMode = .perspective { didSet { if mode != oldValue { redraw() } } }
    public var orbit = OrbitCamera() { didSet { redraw() } }
    public var plan = PlanCamera() { didSet { redraw() } }
    public var scene = SceneSnapshot() { didSet { redraw() } }
    public private(set) var size: CGSize = CGSize(width: 800, height: 600)
    /// Pointer position (points, top-left origin) while over the view.
    public private(set) var pointer: CGPoint?

    @ObservationIgnored public var onEvent: ((ViewportEvent) -> Void)?
    /// Fired when navigation changes the camera (for persistence).
    @ObservationIgnored public var onCamera: (() -> Void)?
    /// When true, the view never reports a hover pick (inset previews).
    @ObservationIgnored public var passive = false

    @ObservationIgnored weak var view: ViewportMTKView?
    @ObservationIgnored let renderer: Renderer?
    @ObservationIgnored private var fitted = false

    public init(palette: ViewportPalette) {
        renderer = Renderer(palette: palette)
    }

    public var available: Bool { renderer != nil }

    public var projection: Projection {
        Projection(mode: mode, orbit: orbit, plan: plan, gridZ: scene.gridZ, size: size)
    }

    func setSize(_ s: CGSize) {
        guard s != size, s.width > 0, s.height > 0 else { return }
        size = s
        if !fitted { zoomToFit(); fitted = true }
    }

    func setPointer(_ p: CGPoint?) { pointer = p }

    public func redraw() { view?.needsDisplay = true }

    // MARK: Camera commands

    public func zoomToFit() {
        frame(min: scene.boundsMin, max: scene.boundsMax)
    }

    public func frame(min lo: Vec3, max hi: Vec3) {
        if mode == .plan {
            plan.frame(min: SIMD2(lo.x, lo.y), max: SIMD2(hi.x, hi.y), in: size)
        } else {
            var o = orbit
            o.frame(min: lo, max: hi)
            if mode != .perspective || o.orthographic {
                // Ortho half-height = distance · tan(fov/2): fit the box.
                let r = simd_length(hi - lo) / 2
                o.distance = Swift.max(0.5, r * 1.15 / tan(o.fovDeg * .pi / 360))
            }
            orbit = o
        }
        onCamera?()
    }

    public func toggleOrtho() {
        orbit.orthographic.toggle()
        onCamera?()
    }

    /// Picks the entity id at a screen point (with an optional search radius in points).
    public func pick(at pt: CGPoint, radius: Int = 0) -> [UInt32] {
        guard let renderer else { return [] }
        renderer.upload(scene)
        return renderer.pick(at: pt, projection: projection, mode: mode, scene: scene, scale: 1, radius: radius)
    }

    /// Offscreen render of the current view (for snapshots).
    public func image(scale: CGFloat = 2) -> CGImage? {
        renderer?.image(projection: projection, mode: mode, scene: scene, scale: scale)
    }

    // MARK: Navigation

    func pan(dx: CGFloat, dy: CGFloat) {
        if mode == .plan {
            plan.center.x -= Double(dx) / plan.pointsPerMetre
            plan.center.y += Double(dy) / plan.pointsPerMetre
        } else {
            let p = projection
            let right = simd_normalize(simd_cross(orbit.target - p.eye, Vec3(0, 0, 1)))
            let up = simd_normalize(simd_cross(right, orbit.target - p.eye))
            let mpp = p.ortho ? p.metresPerPoint : 2 * orbit.distance * tan(orbit.fovDeg * .pi / 360) / Double(size.height)
            orbit.target -= right * Double(dx) * mpp
            orbit.target += up * Double(dy) * mpp
        }
        onCamera?()
    }

    func orbitBy(dx: CGFloat, dy: CGFloat) {
        guard mode != .plan else { return pan(dx: dx, dy: dy) }
        if mode != .perspective { mode = .perspective }
        orbit.yaw -= Double(dx) * 0.4
        orbit.pitch = Swift.min(89, Swift.max(-89, orbit.pitch + Double(dy) * 0.4))
        onCamera?()
    }

    /// Zoom by `factor` (> 1 = in) keeping the point under `at` fixed.
    func zoom(_ factor: Double, at pt: CGPoint) {
        if mode == .plan {
            let before = projection.onPlane(pt, z: scene.gridZ)
            plan.pointsPerMetre = Swift.min(4000, Swift.max(2, plan.pointsPerMetre * factor))
            if let b = before, let a = projection.onPlane(pt, z: scene.gridZ) {
                plan.center.x += b.x - a.x
                plan.center.y += b.y - a.y
            }
        } else {
            let diag = simd_length(scene.boundsMax - scene.boundsMin)
            let newDist = Swift.min(Swift.max(orbit.distance / factor, 0.2), Swift.max(10 * diag, 20))
            // Dolly towards the point under the cursor.
            if let hit = projection.onPlane(pt, z: scene.gridZ), !projection.ortho || true {
                let t = 1 - newDist / orbit.distance
                orbit.target += (hit - orbit.target) * Swift.max(-1, Swift.min(1, t)) * 0.5
            }
            orbit.distance = newDist
        }
        onCamera?()
    }
}

// MARK: SwiftUI host

/// The Metal viewport plus its label overlay.
public struct ViewportView: View {
    let controller: ViewportController

    public init(_ controller: ViewportController) {
        self.controller = controller
    }

    public var body: some View {
        ZStack(alignment: .topLeading) {
            ViewportRepresentable(controller: controller)
            LabelLayer(controller: controller).allowsHitTesting(false)
        }
        .clipped()
    }
}

struct ViewportRepresentable: NSViewRepresentable {
    let controller: ViewportController

    func makeNSView(context: Context) -> ViewportMTKView {
        let v = ViewportMTKView(controller: controller)
        controller.view = v
        return v
    }

    func updateNSView(_ v: ViewportMTKView, context: Context) {
        if v.controller !== controller {
            v.controller = controller
            controller.view = v
        }
        v.needsDisplay = true
    }
}

/// Text labels over the Metal view, placed from projected anchors. One
/// Canvas, so a camera move redraws text without relaying out views.
struct LabelLayer: View {
    let controller: ViewportController

    var body: some View {
        let p = controller.projection
        let labels = controller.scene.labels
        Canvas { g, _ in
            var placed: [CGRect] = []
            for l in labels.sorted(by: { $0.priority > $1.priority }) {
                guard placed.count < 250, let s = p.project(l.anchor) else { continue }
                let pt = CGPoint(x: s.x + l.offset.width, y: s.y + l.offset.height)
                guard pt.x > -40, pt.y > -20, pt.x < p.size.width + 40, pt.y < p.size.height + 20 else { continue }
                let primary = g.resolve(Text(l.text).font(Self.font).foregroundColor(color(l.color)))
                let secondary = l.secondary.map { g.resolve(Text($0).font(Self.font).foregroundColor(Self.white)) }
                let s1 = primary.measure(in: CGSize(width: 400, height: 40))
                let s2 = secondary?.measure(in: CGSize(width: 400, height: 40)) ?? .zero
                let w = s1.width + (secondary == nil ? 0 : s2.width + 6)
                let pad: CGFloat = l.plate ? 5 : 0
                let r = CGRect(x: pt.x - w / 2 - pad, y: pt.y - s1.height / 2 - (l.plate ? 2 : 0), width: w + 2 * pad, height: s1.height + (l.plate ? 4 : 0))
                if placed.contains(where: { $0.intersects(r) }) { continue }
                placed.append(r)
                if l.plate { g.fill(Path(roundedRect: r, cornerRadius: 3), with: .color(Self.plate)) }
                g.draw(primary, at: CGPoint(x: r.minX + pad, y: r.midY), anchor: .leading)
                if let secondary { g.draw(secondary, at: CGPoint(x: r.minX + pad + s1.width + 6, y: r.midY), anchor: .leading) }
            }
        }
    }

    static let font = Font.custom("GeistMono-Regular", fixedSize: 10)
    static let white = Color(.sRGB, red: 0.945, green: 0.957, blue: 0.969, opacity: 1)
    static let plate = Color(.sRGB, red: 0.082, green: 0.09, blue: 0.102, opacity: 0.92)

    func color(_ c: SIMD4<Float>) -> Color {
        Color(.sRGB, red: Double(c.x), green: Double(c.y), blue: Double(c.z), opacity: Double(c.w))
    }
}
