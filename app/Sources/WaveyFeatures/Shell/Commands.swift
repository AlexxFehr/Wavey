import AppKit
import SwiftUI
import WaveyDesign
import WaveyModel

/// One user action. Menus, the palette, the command line and shortcuts all
/// resolve to these.
public struct AppCommand: Identifiable {
    public let id: String
    public let title: String
    public var keywords: [String] = []
    /// Shortcut as shown in menus and the palette ("⌘R").
    public var shortcut: String?
    public var key: KeyEquivalent?
    public var modifiers: EventModifiers = .command
    /// Command-line aliases.
    public var alias: [String] = []
    public var icon: WDIconName?
    public var workspace: Workspace?
    public var menu: String
    public var isEnabled: @MainActor (WindowContext) -> Bool = { _ in true }
    /// Reason shown when disabled.
    public var disabledReason: String?
    public var perform: @MainActor (WindowContext) -> Void
}

extension WindowContext {
    /// The command registry.
    public var commands: [AppCommand] {
        var c: [AppCommand] = []
        // File
        c.append(AppCommand(id: "file.open", title: "Open…", shortcut: "⌘O", key: "o", menu: "File") { $0.openPanel() })
        c.append(AppCommand(id: "file.save", title: "Save", shortcut: "⌘S", key: "s", menu: "File") { $0.save() })
        c.append(AppCommand(id: "file.saveAs", title: "Save As…", shortcut: "⇧⌘S", key: "s", modifiers: [.command, .shift], menu: "File") { $0.saveAs() })
        c.append(AppCommand(id: "file.exportImage", title: "Export Image of View…", menu: "File") { $0.exportViewImage() })
        c.append(AppCommand(id: "file.importDevice", title: "Import Directivity (.waveyspk)…", keywords: ["speaker", "device"], menu: "File") { $0.importDevice() })
        // Edit
        c.append(AppCommand(id: "edit.delete", title: "Delete", shortcut: "⌫", menu: "Edit", isEnabled: { !$0.selection.isEmpty }) { $0.deleteSelection() })
        // Copy/paste of sources, stacks and arrays: the Edit menu's own
        // Copy/Paste items (⌘C/⌘V) route here when no text is being edited.
        c.append(AppCommand(id: "edit.copySources", title: "Copy Sources", shortcut: "⌘C", menu: "Palette",
                            isEnabled: { !$0.selection.speakers.isEmpty || !$0.selectedGroups.isEmpty }) { $0.copySelection() })
        c.append(AppCommand(id: "edit.pasteSources", title: "Paste Sources", shortcut: "⌘V", menu: "Palette") { $0.paste() })
        c.append(AppCommand(id: "edit.duplicate", title: "Duplicate", shortcut: "⌘D", key: "d", menu: "Edit",
                            isEnabled: { !$0.selection.speakers.isEmpty || !$0.selection.microphones.isEmpty || !$0.selectedGroups.isEmpty }) { $0.duplicateSelection() })
        c.append(AppCommand(id: "edit.selectAll", title: "Select All", shortcut: "⌘A", key: "a", menu: "Edit") { $0.selectAll() })
        c.append(AppCommand(id: "edit.deselect", title: "Deselect", shortcut: "⇧⌘A", key: "a", modifiers: [.command, .shift], menu: "Edit") { $0.select([]) })
        c.append(AppCommand(id: "edit.group", title: "Group", shortcut: "⌘G", key: "g", menu: "Edit",
                            isEnabled: { !$0.selection.speakers.isEmpty }) { $0.groupSelection() })
        c.append(AppCommand(id: "edit.mirror", title: "Mirror About Centre Line", keywords: ["L/R", "pair"], menu: "Edit",
                            isEnabled: { !$0.selection.speakers.isEmpty }) { $0.mirrorSelection() })
        c.append(AppCommand(id: "edit.hide", title: "Hide Selection", shortcut: "H", menu: "Edit") { $0.hideSelection() })
        // View
        for (i, ws) in Workspace.allCases.enumerated() {
            c.append(AppCommand(id: "view.workspace.\(ws.rawValue)", title: ws.title, keywords: ["workspace"], shortcut: "⌘\(i + 1)",
                                key: KeyEquivalent(Character("\(i + 1)")), menu: "View") { $0.workspace = ws })
        }
        c.append(AppCommand(id: "view.left", title: "Show/Hide Left Panel", shortcut: "⌥⌘1", key: "1", modifiers: [.command, .option], menu: "View") { $0.leftVisible.toggle() })
        c.append(AppCommand(id: "view.inspector", title: "Show/Hide Inspector", shortcut: "⌥⌘2", key: "2", modifiers: [.command, .option], menu: "View") { $0.inspectorVisible.toggle() })
        c.append(AppCommand(id: "view.dock", title: "Show/Hide Dock", shortcut: "⌥⌘3", key: "3", modifiers: [.command, .option], menu: "View") { $0.state.dockVisible.toggle() })
        c.append(AppCommand(id: "view.fit", title: "Zoom to Fit", shortcut: "Home", menu: "View") { $0.viewport.zoomToFit() })
        c.append(AppCommand(id: "view.frame", title: "Frame Selection", shortcut: "F", menu: "View") { $0.frameSelection() })
        c.append(AppCommand(id: "view.inset", title: "Show/Hide Inset Preview", menu: "View") { $0.toggleFlag("inset") })
        c.append(AppCommand(id: "view.dims", title: "Show/Hide Dimensions", menu: "View") { $0.toggleFlag("dims") })
        c.append(AppCommand(id: "view.palette", title: "Command Palette", shortcut: "⌘K", key: "k", menu: "View") { $0.palettePresented.toggle() })
        // Model
        c.append(AppCommand(id: "model.materials", title: "Materials Library…", shortcut: "⇧⌘M", key: "m", modifiers: [.command, .shift],
                            menu: "Model") { $0.materialLibraryPresented = true })
        c.append(AppCommand(id: "model.check", title: "Check Model", menu: "Model") { $0.issuesPresented = true })
        c.append(AppCommand(id: "model.samples", title: "Add Sample Devices", menu: "Model") { ctx in
            ctx.edit("Add Sample Devices") { try $0.addSampleSpeakerModels() }
        })
        for t in railTools(.geometry).flatMap({ $0 }) where t != .select && t != .move {
            c.append(AppCommand(id: "tool.\(t.rawValue)", title: t.title, alias: t.aliases, icon: t.icon, workspace: .geometry, menu: "Model") { ctx in
                if ctx.workspace != .geometry { ctx.workspace = .geometry; ctx.post("Switched to Geometry") }
                ctx.tool.activate(t)
            })
        }
        c.append(AppCommand(id: "source.newStack", title: "New Speaker Stack…", keywords: ["cardioid", "endfire", "sub array", "cluster"], icon: .stack,
                            workspace: .sources, menu: "Model") { $0.newStack() })
        c.append(AppCommand(id: "source.newLineArray", title: "New Line Array…", keywords: ["array", "splay", "rigging", "GLL"], icon: .lineArray,
                            workspace: .sources, menu: "Model") { $0.newLineArray() })
        c.append(AppCommand(id: "source.editArray", title: "Edit Stack or Array…", menu: "Model",
                            isEnabled: { ctx in ctx.selectedGroups.contains { !(ctx.store.groupsById[$0]?.kind.isEmpty ?? true) } }) { ctx in
            if let g = ctx.selectedGroups.first(where: { !(ctx.store.groupsById[$0]?.kind.isEmpty ?? true) }) { ctx.editAssembly(g) }
        })
        for t in [ToolID.omni, .loudspeaker, .subwoofer, .receiver, .receiverGrid, .aim] {
            c.append(AppCommand(id: "tool.\(t.rawValue)", title: t == .aim ? "Aim at Target" : "Add \(t.title)", icon: t.icon, workspace: .sources, menu: "Model") { ctx in
                if ctx.workspace != .sources { ctx.workspace = .sources; ctx.post("Switched to Sources & receivers") }
                ctx.tool.activate(t)
            })
        }
        // Analysis
        c.append(AppCommand(id: "analysis.solve", title: "Solve", shortcut: "⌘R", key: "r", icon: .play, menu: "Analysis",
                            isEnabled: { !$0.runs.solving && $0.store.blockingIssue == nil },
                            disabledReason: store.blockingIssue?.message) { $0.runs.solve() })
        c.append(AppCommand(id: "analysis.cancel", title: "Cancel Solve", shortcut: "⌘.", key: ".", menu: "Analysis",
                            isEnabled: { $0.runs.solving }) { $0.runs.cancel() })
        c.append(AppCommand(id: "analysis.settings", title: "Solve Settings…", shortcut: "⇧⌘R", key: "r", modifiers: [.command, .shift], menu: "Analysis") {
            $0.solveSettingsPresented = true
        })
        for t in [ToolID.cutPlane, .microphone, .probeLine] {
            c.append(AppCommand(id: "tool.\(t.rawValue)", title: "Add \(t.title)", icon: t.icon, workspace: .results, menu: "Analysis",
                                disabledReason: nil) { ctx in
                if ctx.workspace != .results { ctx.workspace = .results; ctx.post("Switched to Results") }
                ctx.tool.activate(t)
            })
        }
        c.append(AppCommand(id: "results.prev", title: "Previous Frequency", shortcut: ",", menu: "Analysis") { $0.stepFrequency(-1) })
        c.append(AppCommand(id: "results.next", title: "Next Frequency", shortcut: ".", menu: "Analysis") { $0.stepFrequency(1) })
        return c
    }

    func selectAll() {
        switch workspace {
        case .geometry:
            select((store.room?.surfaces ?? []).filter { store.layersById[$0.layerId]?.visible ?? true }.map { .surface($0.key) })
        case .sources:
            select(store.speakers.map { .speaker($0.id) } + store.microphones.map { .microphone($0.id) })
        case .results:
            select(store.cutPlanes.map { .plane($0.id) } + store.microphones.map { .microphone($0.id) })
        }
    }
}

/// ⌘K: fuzzy search over commands, entities (by code or name) and workspaces.
struct CommandPalette: View {
    @Bindable var ctx: WindowContext
    @State private var query = ""
    @State private var index = 0
    @FocusState private var focused: Bool

    struct Item: Identifiable {
        let id: String
        let title: String
        let hint: String
        let shortcut: String?
        let icon: WDIconName?
        let enabled: Bool
        let reason: String?
        let run: @MainActor () -> Void
    }

    var items: [Item] {
        var out: [Item] = []
        for c in ctx.commands {
            let enabled = c.isEnabled(ctx)
            out.append(Item(id: c.id, title: c.title, hint: c.workspace?.title ?? c.menu, shortcut: c.shortcut, icon: c.icon,
                            enabled: enabled, reason: enabled ? nil : (c.disabledReason ?? "Not available now")) { c.perform(ctx) })
        }
        let store = ctx.store
        for s in store.room?.surfaces ?? [] {
            out.append(Item(id: s.key, title: "\(s.code) · \(store.name(.surface(s.key)))", hint: "Surface", shortcut: nil, icon: .surface,
                            enabled: true, reason: nil) { ctx.reveal(.surface(s.key), in: .geometry) })
        }
        for s in store.speakers {
            out.append(Item(id: "spk\(s.id)", title: "\(s.code) · \(s.name)", hint: "Source", shortcut: nil, icon: .speaker, enabled: true, reason: nil) {
                ctx.reveal(.speaker(s.id))
            })
        }
        for m in store.microphones {
            out.append(Item(id: "mic\(m.id)", title: "\(m.code) · \(m.name)", hint: "Receiver", shortcut: nil, icon: .microphone, enabled: true, reason: nil) {
                ctx.reveal(.microphone(m.id))
            })
        }
        for p in store.cutPlanes {
            out.append(Item(id: "plane\(p.id)", title: "\(p.code) · \(p.name)", hint: "Cut plane", shortcut: nil, icon: .cutPlane, enabled: true, reason: nil) {
                ctx.reveal(.plane(p.id))
            })
        }
        guard !query.isEmpty else { return Array(out.prefix(40)) }
        let q = query.lowercased()
        return out.compactMap { i -> (Item, Int)? in
            let hay = (i.title + " " + i.hint).lowercased()
            guard let score = fuzzy(q, hay) else { return nil }
            return (i, score)
        }.sorted { $0.1 > $1.1 }.prefix(40).map(\.0)
    }

    /// Subsequence match; higher is better (contiguous and early matches win).
    func fuzzy(_ q: String, _ s: String) -> Int? {
        if let r = s.range(of: q) { return 1000 - s.distance(from: s.startIndex, to: r.lowerBound) }
        var score = 0
        var it = s.makeIterator()
        for ch in q {
            var found = false
            while let c = it.next() {
                if c == ch { found = true; score += 1; break }
            }
            if !found { return nil }
        }
        return score
    }

    var body: some View {
        let list = items
        ZStack(alignment: .top) {
            Color.black.opacity(0.2).ignoresSafeArea().onTapGesture { ctx.palettePresented = false }
            VStack(spacing: 0) {
                TextField("Search commands, entities (W-07, S1, M2), workspaces", text: $query)
                    .textFieldStyle(.plain)
                    .font(.wd.ui(13))
                    .padding(.horizontal, .wd.s12)
                    .frame(height: 40)
                    .focused($focused)
                    .onSubmit { run(list) }
                    .onKeyPress(.downArrow) { index = min(index + 1, max(list.count - 1, 0)); return .handled }
                    .onKeyPress(.upArrow) { index = max(index - 1, 0); return .handled }
                    .onExitCommand { ctx.palettePresented = false }
                    .onChange(of: query) { _, _ in index = 0 }
                Rectangle().fill(Color.wd.hairline).frame(height: 1)
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            ForEach(Array(list.enumerated()), id: \.element.id) { i, item in
                                HStack(spacing: .wd.s8) {
                                    if let icon = item.icon { WDIcon(icon, size: 12).foregroundStyle(Color.wd.textTertiary) } else { Color.clear.frame(width: 12) }
                                    VStack(alignment: .leading, spacing: 0) {
                                        Text(item.title).font(.wd.body).foregroundStyle(Color.wd.textSecondary)
                                        if let r = item.reason { Text(r).font(.wd.meta).foregroundStyle(Color.wd.textTertiary) }
                                    }
                                    Spacer()
                                    Text(item.hint).font(.wd.detail).foregroundStyle(Color.wd.textTertiary)
                                    if let s = item.shortcut { Text(s).font(.wd.meta).foregroundStyle(Color.wd.textTertiary).frame(minWidth: 40, alignment: .trailing) }
                                }
                                .padding(.horizontal, .wd.s12)
                                .frame(height: 28)
                                .background(i == index ? Color.wd.selectionRow : .clear)
                                .opacity(item.enabled ? 1 : 0.5)
                                .contentShape(Rectangle())
                                .onTapGesture { index = i; run(list) }
                                .id(i)
                            }
                        }
                    }
                    .onChange(of: index) { _, i in proxy.scrollTo(i) }
                }
                .frame(maxHeight: 320)
            }
            .frame(width: 520)
            .background(RoundedRectangle(cornerRadius: .wd.radiusPopover).fill(Color.wd.surfacePanel))
            .overlay(RoundedRectangle(cornerRadius: .wd.radiusPopover).strokeBorder(Color.wd.hairline, lineWidth: 1))
            .shadow(color: .black.opacity(0.45), radius: 12, y: 8)
            .padding(.top, 90)
        }
        .onAppear { focused = true }
    }

    private func run(_ list: [Item]) {
        guard index < list.count, list[index].enabled else { return }
        ctx.palettePresented = false
        list[index].run()
    }
}
