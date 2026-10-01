import AppKit
import SceneKit
import SwiftUI

/// Renders the main view offscreen to a PNG and quits. Used by
/// `scripts/screenshot.sh` for visual checks; it never captures the screen.
@MainActor
enum Snapshot {
    private static var window: NSWindow?

    static func schedule(model: AppModel, to path: String, size: CGSize = CGSize(width: 1400, height: 860)) {
        let host = NSHostingView(rootView: ContentView(model: model))
        host.frame = CGRect(origin: .zero, size: size)
        let w = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
        w.contentView = host
        window = w
        DispatchQueue.main.asyncAfter(deadline: .now() + 2.0) {
            host.layoutSubtreeIfNeeded()
            let ok = write(view: host, to: URL(fileURLWithPath: path))
            exit(ok ? 0 : 1)
        }
    }

    private static func write(view: NSView, to url: URL) -> Bool {
        guard let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return false }
        view.cacheDisplay(in: view.bounds, to: rep)

        // Metal-backed SceneKit content is missing from cacheDisplay; paint it in.
        if let ctx = NSGraphicsContext(bitmapImageRep: rep) {
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = ctx
            for scn in subviews(of: view).compactMap({ $0 as? SCNView }) {
                var r = scn.convert(scn.bounds, to: view)
                if view.isFlipped { r.origin.y = view.bounds.height - r.maxY }
                scn.snapshot().draw(in: r)
            }
            NSGraphicsContext.restoreGraphicsState()
        }
        guard let png = rep.representation(using: .png, properties: [:]) else { return false }
        return (try? png.write(to: url)) != nil
    }

    private static func subviews(of view: NSView) -> [NSView] {
        view.subviews + view.subviews.flatMap { subviews(of: $0) }
    }
}
