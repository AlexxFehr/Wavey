import AppKit
import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel

/// A microphone's responses: magnitude, phase and impulse response.
struct MicResponseView: View {
    @Bindable var ctx: WindowContext
    let mic: UInt64
    @State private var tab = "magnitude"

    var body: some View {
        let m = ctx.store.micsById[mic]
        VStack(spacing: 0) {
            HStack(spacing: .wd.s8) {
                Color.clear.frame(width: .wd.trafficLights)
                WDIcon(.microphone).foregroundStyle(Color.wd.receiver)
                Text(m.map { "\($0.code) · \($0.name)" } ?? "Microphone").font(.wd.title).foregroundStyle(Color.wd.textPrimary)
                    .lineLimit(1).fixedSize()
                Spacer()
                WDSegmented([("magnitude", "Magnitude"), ("phase", "Phase"), ("impulse", "Impulse response")], selection: tab) { tab = $0 }
                    .frame(width: 360)
                if tab == "magnitude" {
                    HeaderLabel("Smoothing")
                    WDChipGroup([(0, "None"), (6, "1/6"), (3, "1/3")], selection: Binding(get: { ctx.results.smoothing }, set: { ctx.results.smoothing = $0 }))
                }
            }
            .padding(.trailing, .wd.s12)
            .frame(height: .wd.topBar)
            .background(Color.wd.surfaceBar)
            Rectangle().fill(Color.wd.hairline).frame(height: 1)
            Group {
                switch tab {
                case "phase": ResponsePlotDock(ctx: ctx, phase: true, only: mic)
                case "impulse": ImpulseDock(ctx: ctx, only: mic)
                default: ResponsePlotDock(ctx: ctx, phase: false, only: mic)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color.wd.surfacePanel)
        }
        .background(Color.wd.surfaceChrome)
        .environment(\.colorScheme, .dark)
    }
}

/// SPL along the probe line.
struct ProbeLineView: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: .wd.s8) {
                Color.clear.frame(width: .wd.trafficLights)
                WDIcon(.probeLine).foregroundStyle(Color.wd.accent)
                Text("Probe line").font(.wd.title).foregroundStyle(Color.wd.textPrimary)
                Text(String(format: "%.1f Hz", ctx.results.frequency)).font(.wd.numeric).foregroundStyle(Color.wd.textTertiary)
                Spacer()
            }
            .padding(.trailing, .wd.s12)
            .frame(height: .wd.topBar)
            .background(Color.wd.surfaceBar)
            Rectangle().fill(Color.wd.hairline).frame(height: 1)
            SpatialPlot(ctx: ctx)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Color.wd.surfacePanel)
        }
        .background(Color.wd.surfaceChrome)
        .environment(\.colorScheme, .dark)
    }
}

/// One plot window per key (a microphone, the probe line).
@MainActor
final class PlotWindow: NSWindowController, NSWindowDelegate {
    private static var open: [String: PlotWindow] = [:]
    let key: String

    static func show(_ key: String, title: String, size: NSSize = NSSize(width: 820, height: 460), content: () -> AnyView) {
        if let w = open[key] { w.window?.makeKeyAndOrderFront(nil); return }
        let w = PlotWindow(key: key, title: title, size: size, content: content())
        open[key] = w
        w.window?.center()
        w.showWindow(nil)
    }

    init(key: String, title: String, size: NSSize, content: AnyView) {
        self.key = key
        let win = NSWindow(contentRect: NSRect(origin: .zero, size: size),
                           styleMask: [.titled, .closable, .resizable, .miniaturizable, .fullSizeContentView], backing: .buffered, defer: false)
        win.titlebarAppearsTransparent = true
        win.titleVisibility = .hidden
        win.appearance = NSAppearance(named: .darkAqua)
        win.backgroundColor = NSColor(Color.wd.surfaceChrome)
        win.minSize = NSSize(width: 520, height: 300)
        win.title = title
        win.isReleasedWhenClosed = false
        super.init(window: win)
        win.delegate = self
        win.contentView = NSHostingView(rootView: content)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func windowWillClose(_ notification: Notification) { Self.open[key] = nil }
}

extension WindowContext {
    /// Double-click on an item: show it in the inspector (reopening it),
    /// and for a microphone with results, its response window.
    func openDetails(_ e: EntityRef) {
        if !selection.contains(e) { select([e]) }
        inspectorVisible = true
        if case .microphone(let id) = e, store.resultInfo != nil { openMicResponse(id) }
    }

    func openMicResponse(_ id: UInt64) {
        let name = store.micsById[id].map { "\($0.code) · \($0.name)" } ?? "Microphone"
        PlotWindow.show("mic-\(id)", title: name) { AnyView(MicResponseView(ctx: self, mic: id)) }
    }

    func openProbeLine() {
        PlotWindow.show("probe", title: "Probe line", size: NSSize(width: 720, height: 380)) { AnyView(ProbeLineView(ctx: self)) }
    }

    /// The response window of the first microphone (offscreen snapshots).
    public func micResponseSnapshotView() -> AnyView? {
        guard let m = store.microphones.first(where: { store.resultInfo?.micIds.contains($0.id) == true }) ?? store.microphones.first else { return nil }
        return AnyView(MicResponseView(ctx: self, mic: m.id))
    }
}
