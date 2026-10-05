import AppKit
import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel
import WaveyPlots

/// Bottom dock: tabs per workspace.
struct Dock: View {
    @Bindable var ctx: WindowContext

    var tabs: [(String, String)] {
        switch ctx.workspace {
        case .geometry, .sources: []
        case .results: [("magnitude", "Magnitude response"), ("phase", "Phase"), ("impulse", "Impulse response"), ("modes", "Modal table")]
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: .wd.s4) {
                ForEach(tabs, id: \.0) { t in
                    Button { ctx.state.dockTab = t.0 } label: {
                        HStack(spacing: .wd.s4) {
                            Text(t.1).font(.wd.body)
                                .foregroundStyle(ctx.state.dockTab == t.0 ? Color.wd.textPrimary : Color.wd.textTertiary)
                            if let n = count(t.0) { Text("\(n)").font(.wd.meta).foregroundStyle(Color.wd.textTertiary) }
                        }
                        .padding(.horizontal, .wd.s8)
                        .frame(height: .wd.dockTabBar)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
                Spacer()
                trailing
            }
            .padding(.horizontal, .wd.s6)
            .frame(height: .wd.dockTabBar)
            .background(Color.wd.surfaceBar)
            content
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Color.wd.surfacePanel)
        }
    }

    private func count(_ tab: String) -> Int? {
        switch tab {
        case "sources": ctx.store.speakers.count
        case "receivers": ctx.store.microphones.filter(\.includeInSolve).count + ctx.store.cutPlanes.count
        case "groups": ctx.store.groups.count
        default: nil
        }
    }

    @ViewBuilder private var trailing: some View {
        switch ctx.workspace {
        case .results:
            HeaderLabel("Smoothing")
            WDChipGroup([(0, "None"), (6, "1/6"), (3, "1/3")], selection: Binding(get: { ctx.results.smoothing }, set: { ctx.results.smoothing = $0 }))
        default:
            EmptyView()
        }
    }

    @ViewBuilder private var content: some View {
        switch (ctx.workspace, ctx.state.dockTab) {
        case (.geometry, _), (.sources, _): EmptyView()
        case (.results, "phase"): ResponsePlotDock(ctx: ctx, phase: true)
        case (.results, "impulse"): ImpulseDock(ctx: ctx)
        case (.results, "modes"): ModalTable(ctx: ctx, nearestOnly: false)
        case (.results, _): ResponsePlotDock(ctx: ctx, phase: false)
        }
    }
}

/// The command-line input: aliases start commands, points and numbers go to
/// the active tool, ↑/↓ browse history, Esc cancels.
struct CommandField: NSViewRepresentable {
    @Bindable var ctx: WindowContext

    func makeCoordinator() -> Coordinator { Coordinator(ctx) }

    func makeNSView(context: Context) -> NSTextField {
        let f = NSTextField()
        f.isBordered = false
        f.drawsBackground = false
        f.focusRingType = .none
        f.font = WDFonts.nsMono(11)
        f.textColor = NSColor(Color.wd.textPrimary)
        f.placeholderString = "Type a command (LINE, REC, EXTRUDE…) or coordinates"
        f.delegate = context.coordinator
        f.cell?.isScrollable = true
        return f
    }

    func updateNSView(_ f: NSTextField, context: Context) {
        context.coordinator.ctx = ctx
        if ctx.focusCommandLine {
            DispatchQueue.main.async {
                f.window?.makeFirstResponder(f)
                f.stringValue = ctx.commandDraft
                f.currentEditor()?.selectedRange = NSRange(location: f.stringValue.count, length: 0)
                ctx.focusCommandLine = false
            }
        }
    }

    @MainActor final class Coordinator: NSObject, NSTextFieldDelegate {
        var ctx: WindowContext
        var historyIndex: Int?
        init(_ c: WindowContext) { ctx = c }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy sel: Selector) -> Bool {
            guard let f = control as? NSTextField else { return false }
            switch sel {
            case #selector(NSResponder.insertNewline(_:)):
                let text = f.stringValue
                f.stringValue = ""
                historyIndex = nil
                ctx.runCommand(text)
                ctx.typedInputVisible = false
                f.window?.makeFirstResponder(nil)
                return true
            case #selector(NSResponder.cancelOperation(_:)):
                f.stringValue = ""
                ctx.tool.escape()
                ctx.typedInputVisible = false
                f.window?.makeFirstResponder(nil)
                return true
            case #selector(NSResponder.moveUp(_:)):
                guard !ctx.commandHistory.isEmpty else { return true }
                let i = max(0, (historyIndex ?? ctx.commandHistory.count) - 1)
                historyIndex = i
                f.stringValue = ctx.commandHistory[i]
                return true
            case #selector(NSResponder.moveDown(_:)):
                guard let i = historyIndex else { return true }
                if i + 1 < ctx.commandHistory.count {
                    historyIndex = i + 1
                    f.stringValue = ctx.commandHistory[i + 1]
                } else {
                    historyIndex = nil
                    f.stringValue = ""
                }
                return true
            default:
                return false
            }
        }
    }
}

extension WindowContext {
    /// Command-line input: an alias (or unambiguous prefix) starts a tool;
    /// anything else is handed to the active tool (points, distances, options).
    func runCommand(_ raw: String) {
        let text = raw.trimmingCharacters(in: .whitespaces)
        if text.isEmpty {
            tool.finishChain()
            return
        }
        commandHistory.append(text)
        commandEcho("Command: \(text)", level: 1)
        let upper = text.uppercased()
        if !upper.contains(where: { $0.isNumber || $0 == "@" || $0 == "#" || $0 == "," }) {
            let tools = railTools(workspace).flatMap { $0 }
            if let exact = tools.first(where: { $0.aliases.contains(upper) }) {
                tool.activate(exact)
                commandEcho(tool.prompt)
                return
            }
            let prefix = tools.filter { $0.aliases.contains { $0.hasPrefix(upper) } }
            if prefix.count == 1 {
                tool.activate(prefix[0])
                commandEcho(tool.prompt)
                return
            }
            if let cmd = commands.first(where: { $0.alias.contains(upper) }) {
                cmd.perform(self)
                return
            }
        }
        if !tool.textEntered(text) {
            commandEcho("Unknown command or input “\(text)”", level: 2)
        } else {
            commandEcho(tool.prompt)
        }
    }
}

func filterSummary(_ filters: [FilterRec]) -> String {
    guard !filters.isEmpty else { return "—" }
    return filters.map { f -> String in
        switch f {
        case .highPass(let hz, let slope): "HP \(Int(hz)) \(slopeName(slope))"
        case .lowPass(let hz, let slope): "LP \(Int(hz)) \(slopeName(slope))"
        case .lowShelf(let hz, let g, _): "LS \(Int(hz)) \(String(format: "%+.1f", g))"
        case .highShelf(let hz, let g, _): "HS \(Int(hz)) \(String(format: "%+.1f", g))"
        case .peak(let hz, let g, _): "PK \(Int(hz)) \(String(format: "%+.1f", g))"
        }
    }.joined(separator: " · ")
}

func slopeName(_ s: SlopeRec) -> String {
    switch s {
    case .butterworth12: "BW12"
    case .butterworth24: "BW24"
    case .linkwitzRiley24: "LR24"
    case .linkwitzRiley48: "LR48"
    }
}
