import AppKit
import SwiftUI
import WaveyCore

/// SwiftUI wrapper around the AppKit sketch editor.
struct SketchCanvas: NSViewRepresentable {
    var model: AppModel

    func makeNSView(context: Context) -> SketchCanvasView {
        let view = SketchCanvasView()
        view.model = model
        return view
    }

    func updateNSView(_ view: SketchCanvasView, context: Context) {
        view.model = model
        // Touch observed properties so SwiftUI re-runs this on changes.
        _ = (model.revision, model.tool, model.snapToGrid, model.selectedSegments)
        if view.activeTool != model.tool {
            view.cancelDrawing()
            view.activeTool = model.tool
        }
        view.fitOnceIfNeeded()
        view.needsDisplay = true
    }
}

/// 2D CAD canvas. World units are meters, y up (same as the view's default).
final class SketchCanvasView: NSView {
    weak var model: AppModel?
    var activeTool: SketchTool = .line

    // View transform: view = (world - center) * scale + bounds.mid
    private var center = CGPoint(x: 5, y: 4)
    private var scale: CGFloat = 50  // points per meter

    // Drawing state (world coordinates).
    private var chain: [CGPoint] = []           // line tool: placed points
    private var anchor: CGPoint?                 // rectangle corner / circle center
    private var hover: CGPoint?                  // snapped cursor
    private var hoverSnap: SnapKind = .none
    private var typed = ""                       // numeric length entry
    private var dragVertex: (id: UInt64, at: CGPoint)?
    private var panStart: (mouse: CGPoint, center: CGPoint)?
    private var shiftDown = false

    enum SnapKind { case none, grid, vertex, midpoint, edge }

    override var acceptsFirstResponder: Bool { true }
    override var isFlipped: Bool { false }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(
            rect: bounds,
            options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect, .mouseEnteredAndExited],
            owner: self))
    }

    override func viewDidMoveToWindow() {
        window?.makeFirstResponder(self)
    }

    func cancelDrawing() {
        chain = []
        anchor = nil
        typed = ""
        dragVertex = nil
        needsDisplay = true
    }

    // MARK: Transform

    private func toView(_ p: CGPoint) -> CGPoint {
        CGPoint(x: (p.x - center.x) * scale + bounds.midX, y: (p.y - center.y) * scale + bounds.midY)
    }

    private func toWorld(_ p: CGPoint) -> CGPoint {
        CGPoint(x: (p.x - bounds.midX) / scale + center.x, y: (p.y - bounds.midY) / scale + center.y)
    }

    private func zoom(by factor: CGFloat, around viewPoint: CGPoint) {
        let before = toWorld(viewPoint)
        scale = min(max(scale * factor, 2), 5000)
        let after = toWorld(viewPoint)
        center.x += before.x - after.x
        center.y += before.y - after.y
        needsDisplay = true
    }

    private var didFit = false

    /// Fits the first non-empty sketch once the view has a size.
    func fitOnceIfNeeded() {
        guard !didFit, bounds.width > 0, model?.sketch.vertices.isEmpty == false else { return }
        didFit = true
        zoomToFit()
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        fitOnceIfNeeded()
    }

    /// Fits the view to the sketch extents.
    func zoomToFit() {
        guard let sketch = model?.sketch, !sketch.vertices.isEmpty else { return }
        let xs = sketch.vertices.map(\.p.x), ys = sketch.vertices.map(\.p.y)
        let (x0, x1, y0, y1) = (xs.min()!, xs.max()!, ys.min()!, ys.max()!)
        center = CGPoint(x: (x0 + x1) / 2, y: (y0 + y1) / 2)
        let w = max(x1 - x0, 1), h = max(y1 - y0, 1)
        scale = min(bounds.width / w, bounds.height / h) * 0.8
        needsDisplay = true
    }

    /// Grid spacing in meters such that minor lines are at least ~14 points apart.
    private var gridStep: CGFloat {
        let steps: [CGFloat] = [0.01, 0.02, 0.05, 0.1, 0.25, 0.5, 1, 2, 5, 10, 20, 50, 100]
        return steps.first { $0 * scale >= 14 } ?? 100
    }

    // MARK: Snapping

    private func snap(_ viewPoint: CGPoint) -> (CGPoint, SnapKind) {
        let w = toWorld(viewPoint)
        let radius = 10 / scale
        guard let sketch = model?.sketch else { return (w, .none) }

        if let v = sketch.vertices.min(by: { dist($0.p, w) < dist($1.p, w) }), dist(v.p, w) <= radius {
            return (CGPoint(x: v.p.x, y: v.p.y), .vertex)
        }
        for s in sketch.segments {
            let m = CGPoint(x: (s.pa.x + s.pb.x) / 2, y: (s.pa.y + s.pb.y) / 2)
            if hypot(m.x - w.x, m.y - w.y) <= radius { return (m, .midpoint) }
        }
        var p = w
        var kind = SnapKind.none
        // Angle constraint relative to the drawing anchor (Shift: 45°, else 0/90° within 3°).
        if activeTool == .line, let a = chain.last {
            let d = CGPoint(x: w.x - a.x, y: w.y - a.y)
            let len = hypot(d.x, d.y)
            let ang = atan2(d.y, d.x)
            let stepAngle = shiftDown ? CGFloat.pi / 4 : CGFloat.pi / 2
            let snapped = (ang / stepAngle).rounded() * stepAngle
            if shiftDown || abs(snapped - ang) < 3 * .pi / 180 {
                p = CGPoint(x: a.x + cos(snapped) * len, y: a.y + sin(snapped) * len)
                if model?.snapToGrid == true {
                    let l = (len / gridStep).rounded() * gridStep
                    p = CGPoint(x: a.x + cos(snapped) * l, y: a.y + sin(snapped) * l)
                    kind = .grid
                }
                return (p, kind)
            }
        }
        // Landing on an existing edge (splits it); grid snapping takes precedence when on.
        if let s = model?.sketch.segments.first(where: { segDist(w, $0) <= 6 / scale }), model?.snapToGrid != true {
            return (project(w, s), .edge)
        }
        if model?.snapToGrid == true {
            let g = gridStep
            p = CGPoint(x: (w.x / g).rounded() * g, y: (w.y / g).rounded() * g)
            kind = .grid
        }
        return (p, kind)
    }

    private func dist(_ a: Point2, _ b: CGPoint) -> CGFloat { hypot(a.x - b.x, a.y - b.y) }

    private func segDist(_ p: CGPoint, _ s: SketchSegment) -> CGFloat {
        let q = project(p, s)
        return hypot(q.x - p.x, q.y - p.y)
    }

    private func project(_ p: CGPoint, _ s: SketchSegment) -> CGPoint {
        let (ax, ay, bx, by) = (s.pa.x, s.pa.y, s.pb.x, s.pb.y)
        let dx = bx - ax, dy = by - ay
        let l2 = dx * dx + dy * dy
        let t = l2 == 0 ? 0 : max(0, min(1, ((p.x - ax) * dx + (p.y - ay) * dy) / l2))
        return CGPoint(x: ax + t * dx, y: ay + t * dy)
    }

    /// The next point to place, honoring a typed length if present.
    private var placement: CGPoint? {
        guard let h = hover else { return nil }
        guard let a = chain.last, let len = Double(typed), len > 0 else { return h }
        let d = CGPoint(x: h.x - a.x, y: h.y - a.y)
        let l = hypot(d.x, d.y)
        guard l > 0 else { return h }
        return CGPoint(x: a.x + d.x / l * len, y: a.y + d.y / l * len)
    }

    // MARK: Mouse

    override func mouseMoved(with event: NSEvent) {
        shiftDown = event.modifierFlags.contains(.shift)
        (hover, hoverSnap) = snap(convert(event.locationInWindow, from: nil))
        needsDisplay = true
    }

    override func mouseExited(with event: NSEvent) {
        hover = nil
        needsDisplay = true
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        guard let model else { return }
        let vp = convert(event.locationInWindow, from: nil)
        let (p, _) = snap(vp)
        let world = toWorld(vp)

        if event.modifierFlags.contains(.option) {
            panStart = (vp, center)
            return
        }

        switch activeTool {
        case .select:
            if let v = model.sketch.vertices.first(where: { dist($0.p, world) <= 8 / scale }) {
                dragVertex = (v.id, CGPoint(x: v.p.x, y: v.p.y))
            } else if let s = model.sketch.segments
                .filter({ segDist(world, $0) <= 6 / scale })
                .min(by: { segDist(world, $0) < segDist(world, $1) }) {
                if event.modifierFlags.contains(.shift) {
                    model.selectedSegments.formSymmetricDifference([s.id])
                } else {
                    model.selectedSegments = [s.id]
                }
            } else {
                model.selectedSegments = []
                panStart = (vp, center)
            }
        case .line:
            guard let q = placement else { return }
            if let a = chain.last {
                model.edit { _ = $0.addSegment(a: Point2(x: a.x, y: a.y), b: Point2(x: q.x, y: q.y)) }
                // Closing the loop on the chain's start ends the chain.
                if let first = chain.first, hypot(first.x - q.x, first.y - q.y) < 1e-9 {
                    chain = []
                } else {
                    chain.append(q)
                }
            } else {
                chain = [q]
            }
            if event.clickCount == 2 { chain = [] }
            typed = ""
        case .rectangle:
            if let a = anchor {
                if a.x != p.x && a.y != p.y {
                    let pts = [a, CGPoint(x: p.x, y: a.y), p, CGPoint(x: a.x, y: p.y)]
                    model.edit { _ = $0.addPolygon(points: pts.map { Point2(x: $0.x, y: $0.y) }) }
                }
                anchor = nil
            } else {
                anchor = p
            }
        case .circle:
            if let c = anchor {
                let r = hypot(p.x - c.x, p.y - c.y)
                if r > 1e-6 {
                    let n = 24
                    let pts = (0..<n).map { i -> Point2 in
                        let t = Double(i) / Double(n) * 2 * .pi
                        return Point2(x: c.x + r * cos(t), y: c.y + r * sin(t))
                    }
                    model.edit { _ = $0.addPolygon(points: pts) }
                }
                anchor = nil
            } else {
                anchor = p
            }
        case .region:
            if !model.doc.toggleInteriorAt(p: Point2(x: world.x, y: world.y)) {
                NSSound.beep()
            }
            model.refresh()
        case .erase:
            if let s = model.sketch.segments
                .filter({ segDist(world, $0) <= 8 / scale })
                .min(by: { segDist(world, $0) < segDist(world, $1) }) {
                model.edit { $0.removeSegments(ids: [s.id]) }
            }
        }
        needsDisplay = true
    }

    override func mouseDragged(with event: NSEvent) {
        let vp = convert(event.locationInWindow, from: nil)
        if let pan = panStart {
            center = CGPoint(x: pan.center.x - (vp.x - pan.mouse.x) / scale,
                             y: pan.center.y - (vp.y - pan.mouse.y) / scale)
        } else if let d = dragVertex {
            let (p, _) = snap(vp)
            dragVertex = (d.id, p)
        }
        needsDisplay = true
    }

    override func mouseUp(with event: NSEvent) {
        if let d = dragVertex, let model {
            model.edit { $0.moveVertex(id: d.id, to: Point2(x: d.at.x, y: d.at.y)) }
        }
        dragVertex = nil
        panStart = nil
        needsDisplay = true
    }

    override func rightMouseDown(with event: NSEvent) {
        panStart = (convert(event.locationInWindow, from: nil), center)
    }

    override func rightMouseDragged(with event: NSEvent) {
        mouseDragged(with: event)
    }

    override func rightMouseUp(with event: NSEvent) {
        panStart = nil
    }

    override func scrollWheel(with event: NSEvent) {
        if event.hasPreciseScrollingDeltas && !event.modifierFlags.contains(.command) {
            // Trackpad: two-finger pan.
            center.x -= event.scrollingDeltaX / scale
            center.y += event.scrollingDeltaY / scale
            needsDisplay = true
        } else {
            let factor = pow(1.0015, event.hasPreciseScrollingDeltas ? event.scrollingDeltaY : event.scrollingDeltaY * 20)
            zoom(by: factor, around: convert(event.locationInWindow, from: nil))
        }
    }

    override func magnify(with event: NSEvent) {
        zoom(by: 1 + event.magnification, around: convert(event.locationInWindow, from: nil))
    }

    override func flagsChanged(with event: NSEvent) {
        shiftDown = event.modifierFlags.contains(.shift)
    }

    // MARK: Keyboard

    override func keyDown(with event: NSEvent) {
        guard let chars = event.charactersIgnoringModifiers else { return }
        switch event.keyCode {
        case 53: // Esc
            cancelDrawing()
            model?.selectedSegments = []
        case 51, 117: // Delete, Forward delete
            if !typed.isEmpty {
                typed.removeLast()
            } else if let model, !model.selectedSegments.isEmpty {
                let ids = Array(model.selectedSegments)
                model.edit { $0.removeSegments(ids: ids) }
            }
        case 36, 76: // Return, Enter
            if activeTool == .line, !typed.isEmpty, let q = placement, let a = chain.last, let model {
                model.edit { _ = $0.addSegment(a: Point2(x: a.x, y: a.y), b: Point2(x: q.x, y: q.y)) }
                chain.append(q)
                typed = ""
            } else {
                chain = []
            }
        default:
            if chars.allSatisfy({ "0123456789.".contains($0) }), activeTool == .line, !chain.isEmpty {
                typed += chars
            } else if chars == "f" {
                zoomToFit()
            } else {
                super.keyDown(with: event)
            }
        }
        needsDisplay = true
    }

    // MARK: Drawing

    override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext, let model else { return }
        NSColor.textBackgroundColor.setFill()
        bounds.fill()
        drawGrid(ctx)

        // Regions: simulated interiors in blue, other closed areas faint.
        for r in model.sketch.regions {
            let path = CGMutablePath()
            addPolygon(path, r.outer)
            r.holes.forEach { addPolygon(path, $0) }
            ctx.addPath(path)
            ctx.setFillColor(r.interior
                ? NSColor.systemBlue.withAlphaComponent(0.18).cgColor
                : NSColor.secondaryLabelColor.withAlphaComponent(0.05).cgColor)
            ctx.fillPath(using: .evenOdd)
        }

        // Segments.
        ctx.setLineCap(.round)
        for s in model.sketch.segments {
            let selected = model.selectedSegments.contains(s.id)
            ctx.setStrokeColor((selected ? NSColor.systemOrange : NSColor.labelColor).cgColor)
            ctx.setLineWidth(selected ? 3 : 2)
            var a = s.pa, b = s.pb
            if let d = dragVertex {
                if s.a == d.id { a = Point2(x: d.at.x, y: d.at.y) }
                if s.b == d.id { b = Point2(x: d.at.x, y: d.at.y) }
            }
            ctx.move(to: toView(CGPoint(x: a.x, y: a.y)))
            ctx.addLine(to: toView(CGPoint(x: b.x, y: b.y)))
            ctx.strokePath()
        }
        ctx.setFillColor(NSColor.labelColor.cgColor)
        for v in model.sketch.vertices {
            let p = toView(CGPoint(x: v.p.x, y: v.p.y))
            ctx.fillEllipse(in: CGRect(x: p.x - 2.5, y: p.y - 2.5, width: 5, height: 5))
        }

        drawPreview(ctx)
        drawHUD(model)
    }

    private func addPolygon(_ path: CGMutablePath, _ pts: [Point2]) {
        guard let first = pts.first else { return }
        path.move(to: toView(CGPoint(x: first.x, y: first.y)))
        pts.dropFirst().forEach { path.addLine(to: toView(CGPoint(x: $0.x, y: $0.y))) }
        path.closeSubpath()
    }

    private func drawGrid(_ ctx: CGContext) {
        let g = gridStep
        let lo = toWorld(.zero), hi = toWorld(CGPoint(x: bounds.maxX, y: bounds.maxY))
        ctx.setLineWidth(1)
        var x = (lo.x / g).rounded(.down) * g
        while x <= hi.x {
            let major = abs((x / (g * 5)).rounded() * g * 5 - x) < g * 0.01
            ctx.setStrokeColor(NSColor.separatorColor.withAlphaComponent(major ? 0.6 : 0.25).cgColor)
            let vx = toView(CGPoint(x: x, y: 0)).x
            ctx.move(to: CGPoint(x: vx, y: 0)); ctx.addLine(to: CGPoint(x: vx, y: bounds.maxY)); ctx.strokePath()
            x += g
        }
        var y = (lo.y / g).rounded(.down) * g
        while y <= hi.y {
            let major = abs((y / (g * 5)).rounded() * g * 5 - y) < g * 0.01
            ctx.setStrokeColor(NSColor.separatorColor.withAlphaComponent(major ? 0.6 : 0.25).cgColor)
            let vy = toView(CGPoint(x: 0, y: y)).y
            ctx.move(to: CGPoint(x: 0, y: vy)); ctx.addLine(to: CGPoint(x: bounds.maxX, y: vy)); ctx.strokePath()
            y += g
        }
        // Origin axes.
        let o = toView(.zero)
        ctx.setLineWidth(1.5)
        ctx.setStrokeColor(NSColor.systemRed.withAlphaComponent(0.6).cgColor)
        ctx.move(to: CGPoint(x: 0, y: o.y)); ctx.addLine(to: CGPoint(x: bounds.maxX, y: o.y)); ctx.strokePath()
        ctx.setStrokeColor(NSColor.systemGreen.withAlphaComponent(0.6).cgColor)
        ctx.move(to: CGPoint(x: o.x, y: 0)); ctx.addLine(to: CGPoint(x: o.x, y: bounds.maxY)); ctx.strokePath()
    }

    private func drawPreview(_ ctx: CGContext) {
        guard let h = hover else { return }
        ctx.setStrokeColor(NSColor.systemBlue.cgColor)
        ctx.setLineWidth(1.5)
        ctx.setLineDash(phase: 0, lengths: [6, 4])
        let q = placement ?? h
        switch activeTool {
        case .line:
            if let a = chain.last {
                ctx.move(to: toView(a)); ctx.addLine(to: toView(q)); ctx.strokePath()
                label(String(format: "%.3f m", hypot(q.x - a.x, q.y - a.y)) + (typed.isEmpty ? "" : "  [\(typed)]"),
                      at: toView(CGPoint(x: (a.x + q.x) / 2, y: (a.y + q.y) / 2)))
            }
        case .rectangle:
            if let a = anchor {
                ctx.stroke(CGRect(x: min(toView(a).x, toView(h).x), y: min(toView(a).y, toView(h).y),
                                  width: abs(toView(h).x - toView(a).x), height: abs(toView(h).y - toView(a).y)))
                label(String(format: "%.2f × %.2f m", abs(h.x - a.x), abs(h.y - a.y)), at: toView(h))
            }
        case .circle:
            if let c = anchor {
                let r = hypot(h.x - c.x, h.y - c.y) * scale
                let vc = toView(c)
                ctx.strokeEllipse(in: CGRect(x: vc.x - r, y: vc.y - r, width: 2 * r, height: 2 * r))
                label(String(format: "r = %.3f m", r / scale), at: toView(h))
            }
        default: break
        }
        ctx.setLineDash(phase: 0, lengths: [])

        // Snap marker.
        let v = toView(q)
        ctx.setStrokeColor((hoverSnap == .vertex || hoverSnap == .midpoint ? NSColor.systemOrange : NSColor.systemBlue).cgColor)
        ctx.setLineWidth(1.5)
        switch hoverSnap {
        case .vertex: ctx.stroke(CGRect(x: v.x - 5, y: v.y - 5, width: 10, height: 10))
        case .midpoint:
            ctx.move(to: CGPoint(x: v.x, y: v.y + 6)); ctx.addLine(to: CGPoint(x: v.x - 6, y: v.y - 4))
            ctx.addLine(to: CGPoint(x: v.x + 6, y: v.y - 4)); ctx.closePath(); ctx.strokePath()
        default: ctx.strokeEllipse(in: CGRect(x: v.x - 3, y: v.y - 3, width: 6, height: 6))
        }
    }

    private func label(_ text: String, at p: CGPoint) {
        let attrs: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .medium),
            .foregroundColor: NSColor.labelColor,
            .backgroundColor: NSColor.textBackgroundColor.withAlphaComponent(0.85),
        ]
        (text as NSString).draw(at: CGPoint(x: p.x + 8, y: p.y + 8), withAttributes: attrs)
    }

    private func drawHUD(_ model: AppModel) {
        var text = activeTool.help
        if let h = hover { text = String(format: "x %.3f  y %.3f   ", h.x, h.y) + text }
        let attrs: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 11),
            .foregroundColor: NSColor.secondaryLabelColor,
        ]
        (text as NSString).draw(at: CGPoint(x: 10, y: 8), withAttributes: attrs)
    }
}
