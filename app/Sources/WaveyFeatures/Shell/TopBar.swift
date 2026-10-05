import AppKit
import SwiftUI
import WaveyDesign
import WaveyModel

/// Top bar (40 pt): app name, workspace switcher, document and run status, Solve.
struct TopBar: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        ZStack {
            HStack(spacing: .wd.s8) {
                Color.clear.frame(width: .wd.trafficLights - .wd.s8)
                WDIcon(.waveform).foregroundStyle(Color.wd.accent)
                Text("Wavey").font(.wd.appName).foregroundStyle(Color.wd.textPrimary)
                Spacer()
                trailing
            }
            .padding(.trailing, .wd.s8)
            WDWorkspaceSwitcher(Workspace.allCases.map { ($0, $0.title, $0.icon) }, selection: ctx.workspace) { ctx.workspace = $0 }
        }
        .frame(height: .wd.topBar)
        .background(WindowDragArea())
        .background(Color.wd.surfaceChrome)
    }

    @ViewBuilder private var trailing: some View {
        HStack(spacing: .wd.s6) {
            Text(ctx.store.title).font(.wd.body).foregroundStyle(Color.wd.textSecondary).lineLimit(1)
            Text(saveState).font(.wd.status).foregroundStyle(Color.wd.textTertiary)
            WDIconButton(.undo, size: .small, help: ctx.store.undoName.map { "Undo \($0)" } ?? "Undo") { ctx.undo() }
                .disabled(!ctx.store.canUndo)
            WDIconButton(.redo, size: .small, help: ctx.store.redoName.map { "Redo \($0)" } ?? "Redo") { ctx.redo() }
                .disabled(!ctx.store.canRedo)
            WDHairline().padding(.horizontal, .wd.s4)
            RunStatusView(ctx: ctx)
            SolveButton(ctx: ctx)
        }
    }

    private var saveState: String {
        if ctx.store.saving { return "saving…" }
        if ctx.store.dirty || ctx.store.fileURL == nil { return "edited" }
        if let t = ctx.store.savedAt { return "saved \(t.formatted(date: .omitted, time: .shortened))" }
        return "saved"
    }
}

/// Lets the custom top bar move the window; double-click zooms.
struct WindowDragArea: NSViewRepresentable {
    final class DragView: NSView {
        override var mouseDownCanMoveWindow: Bool { true }
        override func mouseDown(with e: NSEvent) {
            if e.clickCount == 2 { window?.zoom(nil) } else { window?.performDrag(with: e) }
        }
    }
    func makeNSView(context: Context) -> NSView { DragView() }
    func updateNSView(_ nsView: NSView, context: Context) {}
}

/// Run status: none / current / outdated / solving / failed. Clicking opens the run popover.
struct RunStatusView: View {
    @Bindable var ctx: WindowContext
    @State private var hover = false

    var body: some View {
        Button { ctx.runPopoverPresented.toggle() } label: {
            content
                .padding(.horizontal, .wd.s6)
                .frame(height: .wd.button)
                .background(RoundedRectangle(cornerRadius: .wd.radiusButton).fill(hover ? Color.wd.controlFill : .clear))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
        .popover(isPresented: $ctx.runPopoverPresented, arrowEdge: .bottom) { RunPopover(ctx: ctx) }
        .accessibilityLabel("Run status")
    }

    @ViewBuilder private var content: some View {
        switch ctx.runs.status {
        case .none:
            WDStatusItem(nil, "No results", dot: Color.wd.textTertiary, hollowDot: true)
        case .current(let r):
            WDStatusItem(nil, "\(r.name) · current", dot: Color.wd.statusCurrent)
        case .outdated(let r):
            WDStatusItem(nil, "\(r.name) · outdated", dot: Color.wd.statusOutdated)
        case .solving(let f, let eta, _):
            HStack(spacing: .wd.s6) {
                ZStack(alignment: .leading) {
                    Capsule().fill(Color.wd.controlFill)
                    Capsule().fill(Color.wd.accent).frame(width: 80 * f)
                }
                .frame(width: 80, height: 4)
                Text("\(Int(f * 100))%" + (eta.map { " · \(formatDuration($0)) left" } ?? ""))
                    .font(.wd.status).foregroundStyle(Color.wd.textSecondary).fixedSize()
            }
        case .failed:
            WDStatusItem(nil, "Run failed", dot: Color.wd.statusError)
        }
    }
}

struct SolveButton: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        HStack(spacing: 1) {
            Button {
                if NSEvent.modifierFlags.contains(.option) { ctx.solveSettingsPresented = true; return }
                ctx.runs.solving ? ctx.runs.cancel() : ctx.runs.solve()
            } label: {
                HStack(spacing: .wd.s6) {
                    WDIcon(ctx.runs.solving ? .stop : .play, size: 12)
                    Text(ctx.runs.solving ? "Stop" : "Solve")
                }
            }
            .buttonStyle(WDButtonStyle(primary: !ctx.runs.solving))
            .disabled(!ctx.runs.solving && ctx.store.blockingIssue != nil)
            .help(ctx.store.blockingIssue?.message ?? (ctx.runs.solving ? "Stop the solve (⌘.)" : "Solve (⌘R)"))
            Button { ctx.solveSettingsPresented.toggle() } label: { WDIcon(.chevronDown, size: 12) }
                .buttonStyle(WDButtonStyle(primary: !ctx.runs.solving))
                .help("Solve settings (⌘⇧R)")
                .popover(isPresented: $ctx.solveSettingsPresented, arrowEdge: .bottom) { SolveSettingsPopover(ctx: ctx) }
        }
    }
}
