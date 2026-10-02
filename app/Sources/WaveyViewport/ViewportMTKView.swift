import AppKit
import MetalKit

/// MTKView that renders on demand and turns pointer, scroll and key input
/// into navigation or `ViewportEvent`s.
final class ViewportMTKView: MTKView, MTKViewDelegate {
    var controller: ViewportController
    private var tracking: NSTrackingArea?
    private var dragStart: CGPoint?
    private var dragging = false
    private var navDrag: (kind: NavKind, last: CGPoint)?
    private var spaceDown = false
    private var rightDownAt: CGPoint?

    enum NavKind { case orbit, pan }

    init(controller: ViewportController) {
        self.controller = controller
        super.init(frame: .zero, device: controller.renderer?.device)
        delegate = self
        colorPixelFormat = .bgra8Unorm
        depthStencilPixelFormat = .depth32Float
        sampleCount = controller.renderer?.sampleCount ?? 1
        clearColor = MTLClearColor(red: 0.07, green: 0.08, blue: 0.09, alpha: 1)
        clearDepth = 1
        enableSetNeedsDisplay = true
        isPaused = true
        preferredFramesPerSecond = 120
        layer?.isOpaque = true
    }

    @available(*, unavailable)
    required init(coder: NSCoder) { fatalError() }

    override var acceptsFirstResponder: Bool { true }
    override var isFlipped: Bool { true }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let t = NSTrackingArea(rect: bounds, options: [.mouseMoved, .mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect], owner: self)
        addTrackingArea(t)
        tracking = t
    }

    // MARK: Drawing

    func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {
        needsDisplay = true
    }

    func draw(in view: MTKView) {
        controller.setSize(bounds.size)
        guard let r = controller.renderer, let rp = currentRenderPassDescriptor, let drawable = currentDrawable,
              let cb = r.queue.makeCommandBuffer(), let enc = cb.makeRenderCommandEncoder(descriptor: rp) else { return }
        r.upload(controller.scene)
        var frame = r.frameUniforms(controller.projection, mode: controller.mode, scene: controller.scene, scale: Float(window?.backingScaleFactor ?? 2))
        r.encode(enc, frame: &frame)
        enc.endEncoding()
        cb.present(drawable)
        cb.commit()
    }

    /// Current frame as an image (snapshots: Metal content is missing from cacheDisplay).
    func snapshotImage() -> CGImage? {
        controller.setSize(bounds.size)
        return controller.image(scale: window?.backingScaleFactor ?? 2)
    }

    // MARK: Helpers

    private func point(_ e: NSEvent) -> CGPoint { convert(e.locationInWindow, from: nil) }

    private func mods(_ e: NSEvent) -> ModifierSet {
        var m = ModifierSet()
        if e.modifierFlags.contains(.shift) { m.insert(.shift) }
        if e.modifierFlags.contains(.command) { m.insert(.command) }
        if e.modifierFlags.contains(.option) { m.insert(.option) }
        if e.modifierFlags.contains(.control) { m.insert(.control) }
        return m
    }

    private func world(_ pt: CGPoint) -> Vec3? {
        controller.projection.onPlane(pt, z: controller.scene.gridZ)
    }

    private func pickID(_ pt: CGPoint) -> UInt32? {
        controller.pick(at: pt, radius: 3).first
    }

    // MARK: Pointer

    override func mouseMoved(with e: NSEvent) {
        let pt = point(e)
        controller.setPointer(pt)
        guard !controller.passive else { return }
        controller.onEvent?(.hover(world: world(pt), pick: pickID(pt), screen: pt))
    }

    override func mouseExited(with event: NSEvent) {
        controller.setPointer(nil)
        controller.onEvent?(.exited)
    }

    override func mouseDown(with e: NSEvent) {
        window?.makeFirstResponder(self)
        let pt = point(e)
        if spaceDown {
            navDrag = (controller.mode == .plan ? .pan : .orbit, pt)
            NSCursor.closedHand.push()
            return
        }
        dragStart = pt
        dragging = false
        controller.onEvent?(.down(world: world(pt), pick: pickID(pt), screen: pt, modifiers: mods(e), clickCount: e.clickCount))
    }

    override func mouseDragged(with e: NSEvent) {
        let pt = point(e)
        controller.setPointer(pt)
        if let nav = navDrag {
            let dx = pt.x - nav.last.x, dy = pt.y - nav.last.y
            nav.kind == .pan ? controller.pan(dx: dx, dy: dy) : controller.orbitBy(dx: dx, dy: dy)
            navDrag = (nav.kind, pt)
            return
        }
        guard let start = dragStart else { return }
        if !dragging, hypot(pt.x - start.x, pt.y - start.y) < 4 { return }
        dragging = true
        controller.onEvent?(.drag(world: world(pt), screen: pt, start: start, modifiers: mods(e)))
    }

    override func mouseUp(with e: NSEvent) {
        let pt = point(e)
        if navDrag != nil {
            navDrag = nil
            NSCursor.pop()
            return
        }
        controller.onEvent?(.up(world: world(pt), pick: dragging ? nil : pickID(pt), screen: pt, modifiers: mods(e)))
        dragStart = nil
        dragging = false
    }

    override func rightMouseDown(with e: NSEvent) {
        rightDownAt = point(e)
        navDrag = nil
    }

    override func rightMouseDragged(with e: NSEvent) {
        let pt = point(e)
        guard controller.mode != .plan, let start = rightDownAt else { return }
        if navDrag == nil {
            guard hypot(pt.x - start.x, pt.y - start.y) >= 4 else { return }
            navDrag = (.orbit, pt)
        }
        if let nav = navDrag {
            controller.orbitBy(dx: pt.x - nav.last.x, dy: pt.y - nav.last.y)
            navDrag = (.orbit, pt)
        }
    }

    override func rightMouseUp(with e: NSEvent) {
        let pt = point(e)
        if navDrag == nil {
            controller.onEvent?(.context(pick: pickID(pt), screen: pt))
        }
        navDrag = nil
        rightDownAt = nil
    }

    override func otherMouseDown(with e: NSEvent) {
        let pt = point(e)
        if e.clickCount == 2 { controller.zoomToFit(); return }
        let pan = controller.mode == .plan || e.modifierFlags.contains(.shift)
        navDrag = (pan ? .pan : .orbit, pt)
    }

    override func otherMouseDragged(with e: NSEvent) {
        let pt = point(e)
        guard let nav = navDrag else { return }
        nav.kind == .pan ? controller.pan(dx: pt.x - nav.last.x, dy: pt.y - nav.last.y) : controller.orbitBy(dx: pt.x - nav.last.x, dy: pt.y - nav.last.y)
        navDrag = (nav.kind, pt)
    }

    override func otherMouseUp(with e: NSEvent) { navDrag = nil }

    override func scrollWheel(with e: NSEvent) {
        let pt = point(e)
        let trackpad = e.hasPreciseScrollingDeltas
        if !trackpad || e.modifierFlags.contains(.command) {
            // Mouse wheel or ⌘-scroll: zoom to cursor.
            let dy = trackpad ? e.scrollingDeltaY : e.scrollingDeltaY * 8
            controller.zoom(pow(1.0025, Double(dy)), at: pt)
            return
        }
        if controller.mode == .plan || e.modifierFlags.contains(.shift) {
            controller.pan(dx: e.scrollingDeltaX, dy: e.scrollingDeltaY)
        } else {
            controller.orbitBy(dx: -e.scrollingDeltaX, dy: -e.scrollingDeltaY)
        }
    }

    override func magnify(with e: NSEvent) {
        controller.zoom(1 + Double(e.magnification), at: point(e))
    }

    // MARK: Keys

    override func keyDown(with e: NSEvent) {
        if e.keyCode == 49 { // space
            spaceDown = true
            return
        }
        let chars = e.charactersIgnoringModifiers ?? ""
        if e.keyCode == 115 { controller.zoomToFit(); return } // Home
        if chars == "5", e.modifierFlags.intersection([.command, .option, .control]).isEmpty {
            controller.toggleOrtho()
            return
        }
        controller.onEvent?(.key(chars, keyCode: e.keyCode, modifiers: mods(e)))
    }

    override func keyUp(with e: NSEvent) {
        if e.keyCode == 49 { spaceDown = false }
    }

    override func flagsChanged(with e: NSEvent) {
        if let pt = controller.pointer, !controller.passive {
            controller.onEvent?(.hover(world: world(pt), pick: nil, screen: pt))
        }
    }
}

/// Offscreen images of every viewport under `root`, with their frames in
/// `root`'s coordinates (Metal content is missing from `cacheDisplay`).
@MainActor
public func viewportImages(in root: NSView) -> [(CGRect, CGImage)] {
    func all(_ v: NSView) -> [NSView] { v.subviews + v.subviews.flatMap(all) }
    return all(root).compactMap { v in
        guard let m = v as? ViewportMTKView, let img = m.snapshotImage() else { return nil }
        return (m.convert(m.bounds, to: root), img)
    }
}
