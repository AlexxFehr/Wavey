import AppKit
import SwiftUI
import WaveyModel
import WaveyViewport

extension WindowContext {
    /// `--bench`: times scene rebuilds, renders, picks and the main-thread
    /// cost of a camera step and a hover change with the full shell hosted.
    public func runBench() {
        func time(_ label: String, _ n: Int, _ f: () -> Void) {
            let t0 = Date()
            for _ in 0..<n { f() }
            print(String(format: "%@: %.2f ms", label, Date().timeIntervalSince(t0) * 1000 / Double(n)))
        }
        let host = NSHostingView(rootView: ShellView(context: self).frame(width: 1440, height: 900))
        host.frame = CGRect(x: 0, y: 0, width: 1440, height: 900)
        let w = NSWindow(contentRect: host.frame, styleMask: [.borderless], backing: .buffered, defer: false)
        w.contentView = host
        w.orderBack(nil)
        func settle() {
            host.layoutSubtreeIfNeeded()
            CATransaction.flush()
            RunLoop.current.run(until: Date().addingTimeInterval(0.0005))
        }
        if CommandLine.arguments.contains("--table-check") {
            workspace = .sources
            for _ in 0..<20 { settle() }
            func all(_ v: NSView) -> [NSView] { v.subviews + v.subviews.flatMap(all) }
            for t in all(host).compactMap({ $0 as? NSTableView }) {
                print("table rows \(t.numberOfRows) cols \(t.numberOfColumns) frame \(t.frame) visible \(t.visibleRect) rowsInRect \(t.rows(in: t.visibleRect)) superview \(String(describing: t.superview?.frame))")
            }
            exit(0)
        }
        for ws in Workspace.allCases {
            workspace = ws
            settle()
            let vp = viewport
            time("\(ws) rebuildScenes", 10) { rebuildScenes() }
            time("\(ws) render", 10) { _ = vp.image(scale: 2) }
            time("\(ws) pick", 10) { _ = vp.pick(at: CGPoint(x: 400, y: 300), radius: 3) }
            time("\(ws) camera step + UI update", 30) { var o = vp.orbit; o.yaw += 1; vp.orbit = o; settle() }
            let ids = store.speakers.map { EntityRef.speaker($0.id) } + (store.room?.surfaces.prefix(10).map { EntityRef.surface($0.key) } ?? [])
            var i = 0
            time("\(ws) hover change + rebuild + UI", 20) { selection.hover = ids[i % max(ids.count, 1)]; i += 1; rebuildScenes(); settle() }
        }
        exit(0)
    }
}
