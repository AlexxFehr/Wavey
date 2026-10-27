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
        if CommandLine.arguments.contains("--move-check") {
            // The Move gizmo end to end: the handles must be pickable where they are drawn and drag the selection.
            workspace = CommandLine.arguments.contains("--results") ? .results : .sources
            for _ in 0..<10 { settle() }
            guard let spk = store.speakers.first else { print("FAIL no speaker in the project"); exit(1) }
            if CommandLine.arguments.contains("--group"), let g = store.groups.first { select([.group(g.id)]) } else { select([.speaker(spk.id)]) }
            useTool(.move)
            rebuildScenesNow()
            settle()
            let vp = viewport
            guard let c = SceneBuilder.gizmoCentre(self) else { print("FAIL no gizmo centre"); exit(1) }
            let len = SceneBuilder.worldLength(40, at: c, vp)
            var failures = 0
            let startCentre = c
            print("scene before any drag: \(vp.scene.markers.count) markers, \(vp.scene.lines.count) lines, \(vp.scene.surfaces.count) surfaces, \(vp.scene.meshes.count) meshes")
            for (name, off, want) in [("plane handle", SIMD3(1, 1, 0) * len * 0.3, Handle.planeXY), ("x arrow tip", SIMD3(len, 0, 0), .axis(0)), ("x arrow middle", SIMD3(len * 0.55, 0, 0), .axis(0)), ("y arrow tip", SIMD3(0, len, 0), .axis(1))] {
                guard let pt = vp.projection.project(c + off) else { print("FAIL \(name) is off screen"); failures += 1; continue }
                let ids = vp.pick(at: pt, radius: 3)
                let got = ids.first.flatMap { picks.handle($0) }
                let ok = got == want
                if !ok { failures += 1 }
                print("\(ok ? "PASS" : "FAIL") \(name) at \(Int(pt.x)),\(Int(pt.y)): picked \(ids.map { String($0, radix: 16) }) -> \(got.map { "\($0)" } ?? "nothing")")
            }
            if !CommandLine.arguments.contains("--mouse-only"), let pt = vp.projection.project(c + SIMD3(1, 1, 0) * len * 0.3), let id = vp.pick(at: pt, radius: 3).first {
                let before = store.speakers.first!.position
                tool.handle(.down(world: vp.projection.onPlane(pt, z: c.z), pick: id, screen: pt, modifiers: [], clickCount: 1), in: workspace)
                let to = CGPoint(x: pt.x + 60, y: pt.y + 30)
                tool.handle(.drag(world: vp.projection.onPlane(to, z: c.z), screen: to, start: pt, modifiers: []), in: workspace)
                tool.handle(.up(world: vp.projection.onPlane(to, z: c.z), pick: nil, screen: to, modifiers: []), in: workspace)
                let after = store.speakers.first!.position
                let moved = abs(after.x - before.x) + abs(after.y - before.y) > 0.05
                if !moved { failures += 1 }
                print("\(moved ? "PASS" : "FAIL") dragging the plane handle moves the source: \(before.x),\(before.y) -> \(after.x),\(after.y)")
            }
            // Then with real mouse events through the view: hover first (which rebuilds the scene), then press, drag, release.
            func views(_ v: NSView) -> [NSView] { v.subviews + v.subviews.flatMap(views) }
            let candidates = views(host).filter { String(describing: type(of: $0)).contains("ViewportMTKView") && !$0.isHidden && $0.frame.width > 300 }
            if let view = candidates.max(by: { $0.frame.width * $0.frame.height < $1.frame.width * $1.frame.height }) {
                for _ in 0..<5 { settle() }
                // The view is not the whole window: place the pointer where the handle is drawn, in the view's own points.
                let vp = viewport
                guard let marker = vp.scene.markers.first(where: { $0.id == picks.id(.planeXY) }), let pt = vp.projection.project(marker.position) else { print("FAIL the scene has no plane handle"); exit(1) }
                let c = SceneBuilder.gizmoCentre(self)!
                func event(_ type: NSEvent.EventType, _ p: CGPoint) -> NSEvent {
                    NSEvent.mouseEvent(with: type, location: view.convert(p, to: nil), modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                                       windowNumber: w.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!
                }
                let before = store.speakers.first!.position
                let hit = host.hitTest(host.superview?.convert(view.convert(pt, to: nil), from: nil) ?? view.convert(pt, to: nil))
                print("the view under the pointer at the handle: \(hit.map { String(describing: type(of: $0)) } ?? "none")")
                // Wander over the gizmo like a hand does, then land on the handle.
                for k in 0..<6 { view.mouseMoved(with: event(.mouseMoved, CGPoint(x: pt.x - 30 + Double(k) * 6, y: pt.y - 12 + Double(k) * 2))); settle() }
                view.mouseMoved(with: event(.mouseMoved, pt)); settle()
                var hoverIds = vp.pick(at: pt, radius: 3)
                if hoverIds.isEmpty {
                    print("nothing at the handle; centre \(c) -> \(pt); wider pick: \(vp.pick(at: pt, radius: 25).map { String($0, radix: 16) }); rebuilding now")
                    print("pick at the old place: \(vp.pick(at: vp.projection.project(startCentre + SIMD3(1, 1, 0) * len * 0.3) ?? .zero, radius: 3).map { String($0, radix: 16) }); scene markers \(vp.scene.markers.count), ids \(vp.scene.markers.compactMap { $0.id }.filter { $0 & 0x8000_0000 != 0 }.count)")
                    rebuildScenesNow()
                    hoverIds = vp.pick(at: pt, radius: 3)
                    print("after a rebuild: \(hoverIds.map { String($0, radix: 16) }); handle markers at \(vp.scene.markers.filter { $0.id & 0x8000_0000 != 0 }.map { $0.position })")
                }
                print("after hovering: picked \(hoverIds.map { String($0, radix: 16) }) -> \(hoverIds.first.flatMap { picks.handle($0) }.map { "\($0)" } ?? "no handle"); tool \(activeTool); workspace \(workspace); selection \(selection.ids.count)")
                view.mouseDown(with: event(.leftMouseDown, pt)); settle()
                for k in 1...10 { view.mouseDragged(with: event(.leftMouseDragged, CGPoint(x: pt.x + Double(k) * 6, y: pt.y + Double(k) * 3))); settle() }
                view.mouseUp(with: event(.leftMouseUp, CGPoint(x: pt.x + 60, y: pt.y + 30))); settle()
                let after = store.speakers.first!.position
                let moved = abs(after.x - before.x) + abs(after.y - before.y) > 0.05
                if !moved { failures += 1 }
                print("\(moved ? "PASS" : "FAIL") a real mouse press and drag on the handle moves the source: \(before.x),\(before.y) -> \(after.x),\(after.y)")
            } else {
                print("FAIL no viewport view found to send mouse events to")
                failures += 1
            }
            exit(failures == 0 ? 0 : 1)
        }
        if CommandLine.arguments.contains("--select-bench") {
            workspace = .sources
            for _ in 0..<5 { settle() }
            let ids = store.speakers.map { EntityRef.speaker($0.id) } + store.groups.map { EntityRef.group($0.id) }
            var i = 0
            time("scene build (now)", 10) { rebuildScenesNow() }
            time("select + UI (no scene)", Int(ProcessInfo.processInfo.environment["WAVEY_BENCH_N"] ?? "20") ?? 20) { selection.select([ids[i % ids.count]]); i += 1; host.layoutSubtreeIfNeeded(); CATransaction.flush() }
            time("select + scene + UI", 20) { select([ids[i % ids.count]]); i += 1; rebuildScenesNow(); settle() }
            // Each region alone.
            for (name, view) in [("left", AnyView(LeftPanel(ctx: self).frame(width: 240, height: 800))),
                                 ("inspector", AnyView(InspectorHost(ctx: self).frame(width: 300, height: 800))),
                                 ("library", AnyView(ScrollView { LazyVStack(alignment: .leading, spacing: 0) { LibrarySection(ctx: self, collapsed: false, part: .content) {} } }.frame(width: 240, height: 500))),
                                 ("sources", AnyView(ScrollView { LazyVStack(alignment: .leading, spacing: 0) { SourcesSection(ctx: self, collapsed: false) {} } }.frame(width: 240, height: 500))),
                                 ("topbar", AnyView(TopBar(ctx: self).frame(width: 1440))),
                                 ("status", AnyView(StatusBar(ctx: self).frame(width: 1440))),
                                 ("viewport", AnyView(ViewportRegion(ctx: self).frame(width: 900, height: 800)))] {
                let h = NSHostingView(rootView: view)
                h.frame = CGRect(x: 0, y: 0, width: 1440, height: 900)
                w.contentView = h
                for _ in 0..<3 { h.layoutSubtreeIfNeeded() }
                time("  \(name)", 20) { selection.select([ids[i % ids.count]]); i += 1; h.layoutSubtreeIfNeeded(); CATransaction.flush() }
            }
            let h = NSHostingView(rootView: InspectorHost(ctx: self).frame(width: 300, height: 800))
            h.frame = CGRect(x: 0, y: 0, width: 300, height: 800)
            w.contentView = h
            let spk = store.speakers.map { EntityRef.speaker($0.id) }
            time("  inspector speakers only", 20) { selection.select([spk[i % spk.count]]); i += 1; h.layoutSubtreeIfNeeded(); CATransaction.flush() }
            if let g = store.groups.first {
                time("  inspector speaker <-> group", Int(ProcessInfo.processInfo.environment["WAVEY_BENCH_SWITCH"] ?? "20") ?? 20) { selection.select([i % 2 == 0 ? .group(g.id) : spk[0]]); i += 1; h.layoutSubtreeIfNeeded(); CATransaction.flush() }
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
