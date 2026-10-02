import AppKit
import SwiftUI
import WaveyDesign
import WaveyModel

/// Status bar (22 pt): cursor, workspace context, validity, hints, units.
struct StatusBar: View {
    @Bindable var ctx: WindowContext
    @State private var flags: NSEvent.ModifierFlags = []

    var body: some View {
        HStack(spacing: .wd.s12) {
            cursor
            sep
            context
            sep
            validity
            if let m = ctx.message {
                sep
                WDStatusItem(nil, m.text, dot: m.kind == .error ? Color.wd.statusError : m.kind == .warning ? Color.wd.statusOutdated : Color.wd.statusCurrent)
                    .lineLimit(1)
                    .layoutPriority(-1)
            } else if ctx.runs.solving {
                sep
                Text(ctx.runs.stage).font(.wd.status).foregroundStyle(Color.wd.textTertiary).lineLimit(1)
            }
            Spacer(minLength: .wd.s8)
            Text(hints).font(.wd.status).foregroundStyle(Color.wd.textTertiary).lineLimit(1).layoutPriority(-2)
            WDStatusItem(nil, units)
        }
        .padding(.horizontal, .wd.s12)
        .frame(height: .wd.statusBar)
        .background(Color.wd.surfaceChrome)
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didUpdateNotification)) { _ in
            let f = NSEvent.modifierFlags.intersection([.shift, .command, .option])
            if f != flags { flags = f }
        }
    }

    private var sep: some View { WDHairline(height: 12) }

    private var cursor: some View {
        let c = ctx.cursor
        let f = UnitFormatter(.length)
        return HStack(spacing: .wd.s8) {
            WDStatusItem("X", c.map { f.string($0.x) } ?? "—")
            WDStatusItem("Y", c.map { f.string($0.y) } ?? "—")
            WDStatusItem("Z", c.map { f.string($0.z) } ?? "—")
        }
    }

    @ViewBuilder private var context: some View {
        switch ctx.workspace {
        case .geometry:
            HStack(spacing: .wd.s8) {
                WDStatusItem("Layer", ctx.store.layersById[ctx.store.currentLayer]?.name ?? "—")
                WDStatusItem("Level", ctx.store.levels.first { $0.id == ctx.planLevel }?.name ?? "—")
            }
        case .sources:
            if let p = ctx.selection.primary {
                WDStatusItem("Sel", ctx.store.code(p))
            } else {
                WDStatusItem("Sel", "—")
            }
        case .results:
            HStack(spacing: .wd.s8) {
                WDStatusItem("Run", ctx.store.activeRun.map { String(format: "%03d", $0.id) } ?? "—")
                WDStatusItem("f", String(format: "%.1f Hz", ctx.results.frequency))
            }
        }
    }

    private var validity: some View {
        let w = ctx.store.warnings.count
        let e = ctx.store.blockingIssue
        let text: String
        let dot: Color
        if let e {
            text = e.message
            dot = Color.wd.statusError
        } else if w > 0 {
            text = "\(w) warning\(w == 1 ? "" : "s")"
            dot = Color.wd.statusOutdated
        } else if let r = ctx.store.room {
            text = "Closed · \(r.surfaces.count) surfaces · \(String(format: "%.1f", r.volume)) m³"
            dot = Color.wd.statusCurrent
        } else {
            text = "No volume"
            dot = Color.wd.textTertiary
        }
        return Button { ctx.issuesPresented.toggle() } label: { WDStatusItem(nil, text, dot: dot).lineLimit(1) }
            .buttonStyle(.plain)
            .popover(isPresented: $ctx.issuesPresented, arrowEdge: .top) { IssuesPopover(ctx: ctx) }
    }

    private var hints: String {
        let tool = ctx.activeTool.title
        if flags.contains(.shift) { return "\(tool) · ⇧ axis lock" }
        if flags.contains(.command) { return "\(tool) · ⌘ snap 0.05 m" }
        if flags.contains(.option) { return "\(tool) · ⌥ snaps off" }
        switch ctx.activeTool {
        case .select: return "Select · ⇧ add · ⌘ toggle"
        case .move: return "Move · ⇧ axis lock · ⌘ snap 0.05 m"
        case .line, .polyline: return "\(tool) · C close · U undo point · ⌥ snaps off"
        default: return "\(tool) · Esc cancel"
        }
    }

    private var units: String {
        let m = ctx.store.medium
        return String(format: "m · %.0f °C · %.1f m/s", m.temperatureC, m.speedOfSound)
    }
}

/// Validation issues with Zoom to.
struct IssuesPopover: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        VStack(alignment: .leading, spacing: .wd.s4) {
            Text("Model check").font(.wd.title).foregroundStyle(Color.wd.textPrimary)
            if ctx.store.issues.isEmpty {
                Text("No issues").font(.wd.body).foregroundStyle(Color.wd.textTertiary)
            }
            ForEach(Array(ctx.store.issues.enumerated()), id: \.offset) { _, i in
                HStack(spacing: .wd.s6) {
                    Circle().fill(i.severity == .error ? Color.wd.statusError : Color.wd.statusOutdated).frame(width: 6, height: 6)
                    Text(i.message).font(.wd.body).foregroundStyle(Color.wd.textSecondary)
                    Spacer()
                    if let key = i.entity, let e = EntityRef(engineKey: key) {
                        WDTextButton("Zoom to") {
                            ctx.issuesPresented = false
                            ctx.reveal(e, in: e.kind == .surface ? .geometry : nil)
                        }
                    }
                }
                .frame(minHeight: .wd.row)
            }
        }
        .padding(.wd.s12)
        .frame(width: 360)
        .background(Color.wd.surfacePanel)
    }
}
