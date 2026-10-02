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
        case .geometry: [("command", "Command line")]
        case .sources: [("sources", "Sources"), ("receivers", "Receivers"), ("groups", "Groups")]
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
        case .sources:
            Button { ctx.tool.activate(ctx.state.dockTab == "receivers" ? .receiver : .loudspeaker) } label: {
                HStack(spacing: 4) { WDIcon(.plus, size: 12); Text(ctx.state.dockTab == "receivers" ? "Add receiver" : "Add source") }
            }
            .buttonStyle(.wd)
        case .results:
            HeaderLabel("Smoothing")
            WDChipGroup([(0, "None"), (6, "1/6"), (3, "1/3")], selection: Binding(get: { ctx.results.smoothing }, set: { ctx.results.smoothing = $0 }))
        default:
            EmptyView()
        }
    }

    @ViewBuilder private var content: some View {
        switch (ctx.workspace, ctx.state.dockTab) {
        case (.geometry, _): CommandLineDock(ctx: ctx)
        case (.sources, "receivers"): ReceiversTable(ctx: ctx)
        case (.sources, "groups"): GroupsTable(ctx: ctx)
        case (.sources, _): SourcesTable(ctx: ctx)
        case (.results, "phase"): ResponsePlotDock(ctx: ctx, phase: true)
        case (.results, "impulse"): ImpulseDock(ctx: ctx)
        case (.results, "modes"): ModalTable(ctx: ctx, nearestOnly: false)
        case (.results, _): ResponsePlotDock(ctx: ctx, phase: false)
        }
    }
}

// MARK: Command line

struct CommandLineDock: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(Array(ctx.commandOutput.suffix(3).enumerated()), id: \.offset) { _, line in
                Text(line.0)
                    .font(.wd.numeric)
                    .foregroundStyle(line.1 == 2 ? Color.wd.statusError : line.1 == 1 ? Color.wd.textSecondary : Color.wd.textTertiary)
                    .lineLimit(1)
                    .textSelection(.enabled)
            }
            Spacer(minLength: 0)
            HStack(spacing: .wd.s8) {
                Text(ctx.activeTool == .select ? "Command" : ctx.activeTool.aliases.first ?? ctx.activeTool.title.uppercased())
                    .font(.wd.numeric).foregroundStyle(Color.wd.accentText)
                Text(ctx.tool.prompt).font(.wd.numeric).foregroundStyle(Color.wd.accentText).lineLimit(1).layoutPriority(-1)
                CommandField(ctx: ctx)
                Text("Enter ↵ · Esc").font(.wd.meta).foregroundStyle(Color.wd.textTertiary).fixedSize()
            }
            .padding(.horizontal, .wd.s8)
            .frame(height: .wd.button)
            .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(Color.wd.surfaceBar))
            .overlay(RoundedRectangle(cornerRadius: .wd.radiusField).strokeBorder(Color.wd.hairline, lineWidth: 1))
        }
        .padding(.horizontal, .wd.s10)
        .padding(.vertical, .wd.s6)
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
                return true
            case #selector(NSResponder.cancelOperation(_:)):
                f.stringValue = ""
                ctx.tool.escape()
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

// MARK: Tables

struct SourcesTable: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        let len = UnitFormatter(.length), ang = UnitFormatter(.angle), lvl = UnitFormatter(.level), del = UnitFormatter(.delay)
        let store = ctx.store
        WDTable(store.speakers, columns: columns(store, len, ang, lvl, del), selection: Set(ctx.selection.speakers), emptyText: "No sources") { ids, primary in
            ctx.select(ids.map { .speaker($0) }, primary: primary.map { .speaker($0) })
        }
    }

    private func columns(_ store: DocumentStore, _ len: UnitFormatter, _ ang: UnitFormatter, _ lvl: UnitFormatter, _ del: UnitFormatter) -> [WDTableColumn<Speaker>] {
        var c: [WDTableColumn<Speaker>] = []
        c.append(WDTableColumn<Speaker>("id", "ID", width: 44, numeric: false, color: { _ in Color.wd.source }) { $0.code })
        c.append(WDTableColumn("name", "Name", width: 120, editor: .text { rows, v in
                ctx.edit("Rename Source") { try $0.updateSpeakers(speakers: rows.map { var r = $0; r.name = v; return r }) }
            }) { $0.name })
        c.append(WDTableColumn("device", "Device", width: 150, editor: .menu({ _ in store.speakerModels.map { ($0.id, $0.name) } }) { rows, id in
                ctx.edit("Change Device") { try $0.updateSpeakers(speakers: rows.map { var r = $0; r.modelId = id; return r }) }
            }) { store.modelsById[$0.modelId]?.name ?? $0.modelId })
        c.append(numberColumn("x", "X", len, \.position.x))
        c.append(numberColumn("y", "Y", len, \.position.y))
        c.append(numberColumn("z", "Z", len, \.position.z))
        c.append(numberColumn("az", "Az", ang, \.yawDeg))
        c.append(numberColumn("el", "El", ang, \.pitchDeg))
        c.append(WDTableColumn("level", "Level", width: 56, numeric: true, editor: .number(lvl) { rows, v in
                ctx.edit("Set Level") { try $0.updateSpeakers(speakers: rows.map { var r = $0; r.gainDb = v - (store.modelsById[r.modelId]?.sensitivityDb ?? 0); return r }) }
            }) { s in lvl.string((store.modelsById[s.modelId]?.sensitivityDb ?? 0) + s.gainDb) })
        c.append(numberColumn("delay", "Delay", del, \.delayMs))
        c.append(WDTableColumn("pol", "Pol", width: 36, editor: .menu({ _ in [("+", "+ Normal"), ("-", "− Inverted")] }) { rows, id in
                ctx.edit("Set Polarity") { try $0.updateSpeakers(speakers: rows.map { var r = $0; r.polarityInverted = id == "-"; return r }) }
            }) { $0.polarityInverted ? "−" : "+" })
        c.append(WDTableColumn("filter", "Filter", width: 96) { filterSummary($0.filters) })
        c.append(WDTableColumn("group", "Group", width: 60) { s in store.groups.first { $0.members.contains(s.id) }?.name ?? "—" })
        c.append(WDTableColumn("m", "M", width: 30, editor: .checkbox({ $0.muted }) { rows, v in
                ctx.edit(v ? "Mute" : "Unmute") { try $0.updateSpeakers(speakers: rows.map { var r = $0; r.muted = v; return r }) }
            }) { $0.muted ? "✓" : "" })
        c.append(WDTableColumn("s", "S", width: 30, editor: .checkbox({ $0.solo }) { rows, v in
                ctx.edit(v ? "Solo" : "Unsolo") { try $0.updateSpeakers(speakers: rows.map { var r = $0; r.solo = v; return r }) }
            }) { $0.solo ? "✓" : "" })
        return c
    }

    private func numberColumn(_ id: String, _ title: String, _ f: UnitFormatter, _ kp: WritableKeyPath<Speaker, Double>) -> WDTableColumn<Speaker> {
        WDTableColumn(id, title, width: 58, numeric: true, editor: .number(f) { rows, v in
            ctx.edit("Set \(title)") { try $0.updateSpeakers(speakers: rows.map { var r = $0; r[keyPath: kp] = v; if id == "az" || id == "el" { r.aim = nil }; return r }) }
        }) { f.string($0[keyPath: kp]) }
    }
}

func filterSummary(_ filters: [FilterRec]) -> String {
    guard !filters.isEmpty else { return "—" }
    return filters.map { f -> String in
        switch f {
        case .highPass(let hz, let slope): "HP \(Int(hz)) \(slopeName(slope))"
        case .lowPass(let hz, let slope): "LP \(Int(hz)) \(slopeName(slope))"
        case .lowShelf(let hz, let g): "LS \(Int(hz)) \(String(format: "%+.1f", g))"
        case .highShelf(let hz, let g): "HS \(Int(hz)) \(String(format: "%+.1f", g))"
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

/// Receivers table: point receivers and grids.
struct ReceiverRow: Identifiable {
    let id: EntityRef
    let code: String
    let name: String
    let type: String
    let x: Double?, y: Double?, z: Double?
    let spacing: Double?
    let points: Int
    let inSolve: Bool
}

struct ReceiversTable: View {
    @Bindable var ctx: WindowContext

    var rows: [ReceiverRow] {
        let r = ctx.store.room
        var out: [ReceiverRow] = ctx.store.microphones.filter(\.includeInSolve).map {
            ReceiverRow(id: .microphone($0.id), code: $0.code, name: $0.name, type: "Point", x: $0.position.x, y: $0.position.y, z: $0.position.z,
                        spacing: nil, points: 1, inSolve: $0.includeInSolve)
        }
        for p in ctx.store.cutPlanes {
            var n = 0
            if let r {
                let (w, d): (Double, Double) = switch p.axis {
                case .horizontal: (r.boundsMax.x - r.boundsMin.x, r.boundsMax.y - r.boundsMin.y)
                case .constX: (r.boundsMax.y - r.boundsMin.y, r.boundsMax.z - r.boundsMin.z)
                case .constY: (r.boundsMax.x - r.boundsMin.x, r.boundsMax.z - r.boundsMin.z)
                }
                n = Int((w / p.spacing).rounded(.up)) * Int((d / p.spacing).rounded(.up))
            }
            let axis = p.axis == .horizontal ? "Grid XY" : p.axis == .constX ? "Grid YZ" : "Grid XZ"
            out.append(ReceiverRow(id: .plane(p.id), code: p.code, name: p.name, type: axis,
                                   x: p.axis == .constX ? p.offset : nil, y: p.axis == .constY ? p.offset : nil, z: p.axis == .horizontal ? p.offset : nil,
                                   spacing: p.spacing, points: n, inSolve: true))
        }
        return out
    }

    var body: some View {
        let len = UnitFormatter(.length)
        WDTable(rows, columns: [
            WDTableColumn<ReceiverRow>("id", "ID", width: 50) { $0.code },
            WDTableColumn("name", "Name", width: 140, editor: .text { rows, v in rename(rows, v) }) { $0.name },
            WDTableColumn("type", "Type", width: 70) { $0.type },
            WDTableColumn("x", "X", width: 60, numeric: true, editor: .number(len) { rows, v in setCoord(rows, 0, v) }) { $0.x.map(len.string) ?? "—" },
            WDTableColumn("y", "Y", width: 60, numeric: true, editor: .number(len) { rows, v in setCoord(rows, 1, v) }) { $0.y.map(len.string) ?? "—" },
            WDTableColumn("z", "Z", width: 60, numeric: true, editor: .number(len) { rows, v in setCoord(rows, 2, v) }) { $0.z.map(len.string) ?? "—" },
            WDTableColumn("sp", "Spacing", width: 64, numeric: true) { $0.spacing.map { String(format: "%.2f", $0) } ?? "—" },
            WDTableColumn("pts", "Points", width: 56, numeric: true) { "\($0.points)" },
            WDTableColumn("solve", "In solve", width: 60, editor: .checkbox({ $0.inSolve }) { rows, v in
                let mics = rows.compactMap { $0.id.microphoneID.flatMap { ctx.store.micsById[$0] } }.map { m -> MicrophoneRec in var m = m; m.includeInSolve = v; return m }
                if !mics.isEmpty { ctx.edit("Include in Solve") { try $0.updateMicrophones(mics: mics) } }
            }) { $0.inSolve ? "✓" : "" },
        ], selection: Set(ctx.selection.ids.filter { $0.kind == .microphone || $0.kind == .plane }), emptyText: "No receivers") { ids, p in
            ctx.select(ids, primary: p)
        }
    }

    private func rename(_ rows: [ReceiverRow], _ v: String) {
        ctx.edit("Rename") { doc in
            for r in rows {
                if let id = r.id.microphoneID, var m = ctx.store.micsById[id] { m.name = v; try doc.updateMicrophone(mic: m) }
                if let id = r.id.planeID, var p = ctx.store.planesById[id] { p.name = v; try doc.updateCutPlane(plane: p) }
            }
        }
    }

    private func setCoord(_ rows: [ReceiverRow], _ axis: Int, _ v: Double) {
        ctx.edit("Move Receiver") { doc in
            for r in rows {
                if let id = r.id.microphoneID, var m = ctx.store.micsById[id] {
                    if axis == 0 { m.position.x = v } else if axis == 1 { m.position.y = v } else { m.position.z = v }
                    try doc.updateMicrophone(mic: m)
                }
                if let id = r.id.planeID, var p = ctx.store.planesById[id] {
                    let matches = (axis == 0 && p.axis == .constX) || (axis == 1 && p.axis == .constY) || (axis == 2 && p.axis == .horizontal)
                    if matches { p.offset = v; try doc.updateCutPlane(plane: p) }
                }
            }
        }
    }
}

struct GroupsTable: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        let store = ctx.store
        WDTable(store.groups, columns: [
            WDTableColumn<GroupRec>("id", "ID", width: 44) { $0.code },
            WDTableColumn("name", "Name", width: 160, editor: .text { rows, v in
                ctx.edit("Rename Group") { doc in for g in rows { var r = g; r.name = v; try doc.updateGroup(group: r) } }
            }) { $0.name },
            WDTableColumn("members", "Members", width: 200) { g in g.members.compactMap { store.speakersById[$0]?.code }.joined(separator: ", ") },
            WDTableColumn("linked", "Linked", width: 56, editor: .checkbox({ $0.linked }) { rows, v in
                ctx.edit("Link Group") { doc in for g in rows { var r = g; r.linked = v; try doc.updateGroup(group: r) } }
            }) { $0.linked ? "✓" : "" },
        ], selection: Set(ctx.selection.ids.compactMap(\.groupID)), emptyText: "No groups — select sources and press ⌘G") { ids, p in
            ctx.select(ids.map { .group($0) }, primary: p.map { .group($0) })
        }
    }
}
