import AppKit
import SwiftUI
import WaveyFeatures
import WaveyViewport

/// Renders the new shell offscreen to a PNG and quits (visual checks; never captures the screen).
@MainActor
enum ShellSnapshot {
    private static var window: NSWindow?

    static func schedule(ctx: WindowContext, to path: String, size: CGSize = CGSize(width: 1440, height: 900), root: AnyView? = nil) {
        let host = NSHostingView(rootView: (root ?? AnyView(ShellView(context: ctx))).frame(width: size.width, height: size.height))
        host.frame = CGRect(origin: .zero, size: size)
        let w = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
        w.appearance = NSAppearance(named: .darkAqua)
        w.contentView = host
        window = w
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
            ctx.fitAll()
            ctx.rebuildScenes()
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + (root == nil ? 3.0 : 6.0)) {
            capture(ctx: ctx, host: host, to: path, deadline: Date() + 60)
        }
    }

    /// Writes the PNG once result fields have arrived (or after `deadline`).
    private static func capture(ctx: WindowContext, host: NSView, to path: String, deadline: Date) {
        if !ctx.cache.loading.isEmpty && Date() < deadline {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { capture(ctx: ctx, host: host, to: path, deadline: deadline) }
            return
        }
        if deadline != .distantPast && Date() < deadline {
            // Give the views a moment to draw what just arrived.
            ctx.rebuildScenes()
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) { capture(ctx: ctx, host: host, to: path, deadline: .distantPast) }
            return
        }
        host.layoutSubtreeIfNeeded()
        exit(write(view: host, to: URL(fileURLWithPath: path)) ? 0 : 1)
    }

    private static func write(view: NSView, to url: URL) -> Bool {
        guard let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return false }
        view.cacheDisplay(in: view.bounds, to: rep)
        if let g = NSGraphicsContext(bitmapImageRep: rep) {
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = g
            for (rect, img) in viewportImages(in: view) {
                var r = rect
                if view.isFlipped { r.origin.y = view.bounds.height - r.maxY }
                NSImage(cgImage: img, size: r.size).draw(in: r)
            }
            NSGraphicsContext.restoreGraphicsState()
            // Labels and overlays drawn above the Metal views: repaint the SwiftUI layer on top.
        }
        guard let png = rep.representation(using: .png, properties: [:]) else { return false }
        return (try? png.write(to: url)) != nil
    }
}
