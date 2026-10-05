import AppKit
import SwiftUI
import UniformTypeIdentifiers
import WaveyCore
import WaveyDesign
import WaveyModel

/// The outliner: every entity type is reachable from every workspace; the
/// workspace only decides which sections come first and are expanded.
struct LeftPanel: View {
    @Bindable var ctx: WindowContext
    /// Height of the Library pane (Sources workspace).
    @State private var libraryHeight: Double = 320

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(order, id: \.self) { section($0) }
                }
                .padding(.bottom, .wd.s12)
            }
            .scrollIndicators(.never)
            .frame(maxHeight: .infinity)
            if ctx.workspace == .sources { libraryPane }
        }
    }

    /// The Library is its own pane under the scene outliner: a bar between
    /// them, a pinned header, and its own scrolling.
    private var libraryPane: some View {
        let closed = collapsed("library")
        return VStack(spacing: 0) {
            Rectangle().fill(Color.wd.surfaceChrome).frame(height: 5)
                .overlay(alignment: .top) { Rectangle().fill(Color.wd.hairline).frame(height: 1) }
                .overlay {
                    PanelDivider(axis: .horizontal) { d in libraryHeight = min(max(libraryHeight - d, 120), 700) }
                }
            LibrarySection(ctx: ctx, collapsed: closed, part: .header) { toggle("library") }
                .background(Color.wd.surfaceBar)
            if !closed {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        LibrarySection(ctx: ctx, collapsed: false, part: .content) {}
                    }
                    .padding(.vertical, .wd.s4)
                }
                .scrollIndicators(.never)
                .frame(height: CGFloat(libraryHeight))
            }
        }
    }

    /// Outliner sections per workspace. Geometry shows the model only;
    /// Sources shows the scene (the Library has its own pane); Results its
    /// runs and post-processing first.
    private var order: [String] {
        switch ctx.workspace {
        case .geometry: ["model", "layers", "surfaces"]
        case .sources: ["scene", "sources", "receivers"]
        case .results: ["results", "post"]
        }
    }

    func collapsed(_ id: String) -> Bool { ctx.state.collapsed.contains(id) }
    func toggle(_ id: String) {
        if NSEvent.modifierFlags.contains(.option) {
            let all = Set(order)
            ctx.state.collapsed = collapsed(id) ? [] : all
        } else if collapsed(id) { ctx.state.collapsed.remove(id) } else { ctx.state.collapsed.insert(id) }
    }

    @ViewBuilder func section(_ id: String) -> some View {
        switch id {
        case "model": ModelSection(ctx: ctx, collapsed: collapsed(id)) { toggle(id) }
        case "layers": LayersSection(ctx: ctx, collapsed: collapsed(id)) { toggle(id) }
        case "surfaces": SurfacesSection(ctx: ctx, collapsed: collapsed(id)) { toggle(id) }
        case "sources": SourcesSection(ctx: ctx, collapsed: collapsed(id)) { toggle(id) }
        case "receivers": ReceiversSection(ctx: ctx, collapsed: collapsed(id)) { toggle(id) }
        case "results": RunSection(ctx: ctx, collapsed: collapsed(id)) { toggle(id) }
        case "post": PostSection(ctx: ctx, collapsed: collapsed(id)) { toggle(id) }
        case "scenetoggles": SceneTogglesSection(ctx: ctx, collapsed: collapsed(id)) { toggle(id) }
        case "scene": SceneSummarySection(ctx: ctx, collapsed: collapsed(id)) { toggle(id) }
        default: EmptyView()
        }
    }
}

// MARK: Row

/// Outliner row (24 pt): swatch or icon, name, meta, and eye/lock buttons.
struct OutlinerRow: View {
    @Bindable var ctx: WindowContext
    let entity: EntityRef?
    var icon: WDIconName?
    var iconColor: Color = .wd.textTertiary
    var swatch: Color?
    var code: String?
    let title: String
    var meta: String?
    var indent = 0
    var dimmed = false
    var bold = false
    var current = false
    var warning: String?
    var visible: Bool?
    var locked: Bool?
    var onVisible: (() -> Void)?
    var onLock: (() -> Void)?
    var onRename: ((String) -> Void)?
    var onTap: (() -> Void)?
    var onDoubleTap: (() -> Void)?
    /// Parent rows: expanded state and its toggle (a disclosure chevron).
    var expanded: Bool?
    var onExpand: (() -> Void)?
    @State private var hover = false
    @State private var renaming = false

    var selected: Bool { entity.map { ctx.selection.contains($0) } ?? false }
    /// Local hover only: reading the global hover here would re-evaluate
    /// every row on each viewport hover change.
    var hovered: Bool { hover }

    var body: some View {
        HStack(spacing: .wd.s6) {
            if let expanded {
                Button { onExpand?() } label: {
                    WDIcon(expanded ? .chevronDown : .chevronRight, size: 12).foregroundStyle(Color.wd.textTertiary).frame(width: 12, height: 12).contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            if current {
                Circle().fill(Color.wd.accent).frame(width: 6, height: 6)
            }
            if let swatch {
                RoundedRectangle(cornerRadius: 2).fill(swatch).frame(width: 8, height: 8)
            } else if let icon {
                WDIcon(icon, size: 12).foregroundStyle(selected ? Color.wd.accentText : iconColor)
            }
            if let code {
                Text(code).font(.wd.meta).foregroundStyle(selected ? Color.wd.accentText : Color.wd.textTertiary).fixedSize()
            }
            if renaming, let onRename {
                WDTextField(title, chrome: true) { onRename($0); renaming = false }
                    .onExitCommand { renaming = false }
            } else {
                Text(title)
                    .font(bold ? .wd.ui(11.5, .semibold) : .wd.body)
                    .foregroundStyle(selected ? Color.wd.accentTextOnFill : dimmed || visible == false ? Color.wd.textDisabled : Color.wd.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
            if let warning {
                WDIcon(.warning, size: 12).foregroundStyle(Color.wd.source).help(warning)
            }
            Spacer(minLength: .wd.s4)
            if let meta {
                Text(meta).font(.wd.meta).foregroundStyle(Color.wd.textTertiary).lineLimit(1).fixedSize()
            }
            if let visible, let onVisible, hovered || !visible || locked != nil {
                WDIconButton(visible ? .eye : .eyeOff, size: .small, help: "Toggle visibility") { onVisible() }
            }
            if let locked, let onLock {
                WDIconButton(locked ? .lock : .unlock, size: .small, isOn: false, help: "Toggle lock") { onLock() }
                    .opacity(locked || hovered ? 1 : 0.6)
            }
        }
        .padding(.leading, .wd.s8 + CGFloat(indent) * .wd.rowIndent)
        .padding(.trailing, .wd.s4)
        .frame(height: .wd.row)
        .background(RoundedRectangle(cornerRadius: .wd.radiusField)
            .fill(selected ? Color.wd.selectionRow : hovered ? Color.wd.rowHover : .clear))
        .padding(.horizontal, .wd.s4)
        .contentShape(Rectangle())
        .onHover { h in
            hover = h
            guard let entity else { return }
            ctx.selection.hover = h ? entity : nil
            ctx.rebuildScenes()
        }
        // One tap gesture, acting at once: a separate double-tap gesture made
        // SwiftUI hold every single click for the double-click interval.
        // The second click of a double-click opens the item: the row's own
        // action, else the inspector (and a microphone's responses); rows
        // without an item rename.
        .onTapGesture {
            if (NSApp.currentEvent?.clickCount ?? 1) >= 2 {
                if let onDoubleTap { onDoubleTap() } else if let entity { ctx.openDetails(entity) } else if onRename != nil { renaming = true }
            } else {
                tap()
            }
        }
        .contextMenu { if let entity { EntityMenu(ctx: ctx, entity: entity) } }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }

    private func tap() {
        if let onTap { onTap(); return }
        guard let entity else { return }
        let f = NSEvent.modifierFlags
        if f.contains(.shift) {
            ctx.selectRange(to: entity)
        } else {
            if f.contains(.command) { ctx.selection.toggle(entity) } else { ctx.selection.select([entity]) }
            ctx.outlinerAnchor = entity
        }
        ctx.rebuildScenes()
    }
}

/// Context menu items for an entity (outliner, tables, viewport).
struct EntityMenu: View {
    @Bindable var ctx: WindowContext
    let entity: EntityRef

    var body: some View {
        Button("Zoom to") { ctx.reveal(entity) }
        Menu("Reveal in") {
            ForEach(Workspace.allCases, id: \.self) { ws in Button(ws.title) { ctx.reveal(entity, in: ws) } }
        }
        Divider()
        switch entity {
        case .speaker, .microphone:
            Button("Duplicate") { ctx.duplicateSelection() }
            Button("Delete") { ctx.deleteSelection() }
        case .plane, .group:
            Button("Delete") { ctx.deleteSelection() }
        case .surface(let k):
            Menu("Assign material") {
                ForEach(ctx.store.materials, id: \.id) { m in
                    Button(m.name) { ctx.assignMaterial(m.id, to: [k]) }
                }
            }
            Button("Delete edge") { ctx.deleteSelection() }
        default:
            EmptyView()
        }
        if case .speaker = entity {
            Button("Assign to new group") { ctx.groupSelection() }
        }
    }
}

// MARK: Sections

struct ModelSection: View {
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    let toggle: () -> Void

    var body: some View {
        WDSectionHeader("Model", collapsed: collapsed, onToggle: toggle) {
            WDIconButton(.plus, size: .small, help: "Add level") {
                ctx.edit("Add Level") { _ = try $0.upsertLevel(level: LevelRec(id: "", name: "Level \(ctx.store.levels.count)", z: 1.0, kind: .datum)) }
            }
        }
        if !collapsed {
            OutlinerRow(ctx: ctx, entity: nil, icon: .chevronDown, title: "Levels", onTap: {})
            ForEach(Array(ctx.store.levels.enumerated()), id: \.element.id) { i, l in
                OutlinerRow(ctx: ctx, entity: .level(l.id), code: "L\(i)", title: l.name, meta: UnitFormatter(.length).string(l.z),
                            indent: 1, bold: l.id == ctx.planLevel,
                            onRename: l.kind == .datum ? { n in ctx.edit("Rename Level") { _ = try $0.upsertLevel(level: LevelRec(id: l.id, name: n, z: l.z, kind: l.kind)) } } : nil,
                            onDoubleTap: { ctx.planLevel = l.id; ctx.rebuildScenes() })
            }
        }
    }
}

struct LayersSection: View {
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    let toggle: () -> Void

    var body: some View {
        WDSectionHeader("Layers", collapsed: collapsed, onToggle: toggle) {
            WDIconButton(.plus, size: .small, help: "Add layer") {
                let swatch = Color.wd.swatches[ctx.store.layers.count % Color.wd.swatches.count]
                ctx.edit("Add Layer") { _ = try $0.upsertLayer(layer: LayerRec(id: "", name: "Layer \(ctx.store.layers.count + 1)", color: swatch, visible: true, locked: false, builtin: false)) }
            }
        }
        if !collapsed {
            ForEach(ctx.store.layers, id: \.id) { l in
                let count = count(l.id)
                OutlinerRow(ctx: ctx, entity: .layer(l.id), swatch: Color(hex: l.color) ?? .gray, title: l.name, meta: "\(count)",
                            bold: l.id == ctx.store.currentLayer, current: l.id == ctx.store.currentLayer,
                            visible: l.visible, locked: l.locked,
                            onVisible: { update(l) { $0.visible.toggle() } },
                            onLock: { update(l) { $0.locked.toggle() } },
                            onRename: { n in update(l) { $0.name = n } },
                            onDoubleTap: nil)
                    .contextMenu {
                        Button("Make current") { ctx.edit("Set Current Layer") { try $0.setCurrentLayer(id: l.id) } }
                        Button("Move selection here") { ctx.moveSelection(toLayer: l.id) }
                        if !l.builtin { Button("Delete layer") { ctx.edit("Delete Layer") { try $0.removeLayer(id: l.id) } } }
                    }
            }
        }
    }

    private func count(_ layer: String) -> Int {
        switch layer {
        case "floor": ctx.store.room?.surfaces.filter { $0.kind == .floor }.count ?? 0
        case "ceiling": ctx.store.room?.surfaces.filter { $0.kind == .ceiling }.count ?? 0
        default: ctx.store.sketch.segments.filter { $0.layerId == layer }.count
        }
    }

    private func update(_ l: LayerRec, _ f: (inout LayerRec) -> Void) {
        var n = l
        f(&n)
        ctx.edit("Change Layer") { _ = try $0.upsertLayer(layer: n) }
    }
}

struct SurfacesSection: View {
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    let toggle: () -> Void
    @State private var filter = ""
    @State private var filterOpen = false

    var body: some View {
        let all = (ctx.store.room?.surfaces ?? []).sorted { $0.code.localizedStandardCompare($1.code) == .orderedAscending }
        let shown = filter.isEmpty ? all : all.filter {
            $0.code.localizedCaseInsensitiveContains(filter) || ctx.store.name(.surface($0.key)).localizedCaseInsensitiveContains(filter)
                || (ctx.store.materialsById[$0.materialId]?.name.localizedCaseInsensitiveContains(filter) ?? false)
        }
        WDSectionHeader("Surfaces", collapsed: collapsed, onToggle: toggle) {
            WDCount(all.count)
            WDIconButton(.filter, size: .small, isOn: !filter.isEmpty, help: "Filter surfaces") { filterOpen.toggle() }
                .popover(isPresented: $filterOpen) {
                    TextField("Code, name or material", text: $filter)
                        .textFieldStyle(.plain).font(.wd.body)
                        .padding(.horizontal, 7).frame(width: 220, height: .wd.control)
                        .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(Color.wd.controlFill))
                        .padding(.wd.s8)
                }
        }
        if !collapsed {
            ForEach(shown, id: \.key) { s in
                let m = ctx.store.materialsById[s.materialId]
                OutlinerRow(ctx: ctx, entity: .surface(s.key), code: s.code, title: ctx.store.name(.surface(s.key)),
                            dimmed: !(ctx.store.layersById[s.layerId]?.visible ?? true),
                            onRename: { n in ctx.edit("Rename Surface") { try $0.setSurfaceNames(surfaceKeys: [s.key], name: n) } })
                    .overlay(alignment: .trailing) {
                        Circle().fill(Color(hex: m?.color ?? "") ?? .gray).frame(width: 7, height: 7).padding(.trailing, 14).allowsHitTesting(false)
                    }
            }
        }
    }
}

/// Speakers as a tree: stacks, line arrays and groups are parents with
/// their speakers under them; ungrouped speakers follow. Rows drag onto a
/// plain group to join it, or onto the section header to leave it.
struct SourcesSection: View {
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    let toggle: () -> Void

    var body: some View {
        let groups = ctx.store.groups.filter { !$0.members.isEmpty || $0.receivers.isEmpty }
        let grouped = Set(groups.flatMap(\.members))
        WDSectionHeader("Sources", collapsed: collapsed, onToggle: toggle) {
            WDCount(ctx.store.speakers.count)
            Menu {
                Button("New Speaker Stack…") { ctx.newStack() }
                Button("New Line Array…") { ctx.newLineArray() }
                Divider()
                Button("Group Selected Sources (⌘G)") { ctx.groupSelection() }
            } label: { WDIcon(.plus, size: 12) }
                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                .help("Speakers: drag from the Library · stacks and line arrays")
        }
        .dropDestination(for: String.self) { items, _ in ctx.ungroup(items) }
        if !collapsed {
            if ctx.store.speakers.isEmpty {
                Text("Drag speakers from the Library into the view").font(.wd.detail).foregroundStyle(Color.wd.textTertiary)
                    .padding(.leading, .wd.s12).frame(height: .wd.row)
            }
            ForEach(groups, id: \.id) { g in
                GroupRow(ctx: ctx, group: g)
                if ctx.groupExpanded(g.id) {
                    ForEach(g.members, id: \.self) { id in
                        if let s = ctx.store.speakersById[id] { speakerRow(s, indent: 2) }
                    }
                }
            }
            ForEach(ctx.store.speakers.filter { !grouped.contains($0.id) }, id: \.id) { s in speakerRow(s, indent: 0) }
        }
    }

    private func speakerRow(_ s: Speaker, indent: Int) -> some View {
        let warn = ctx.store.warnings.first { $0.entity == "spk:\(s.id)" }?.message
        return OutlinerRow(ctx: ctx, entity: .speaker(s.id), icon: isSub(s) ? .subwoofer : .speaker, iconColor: Color.wd.source,
                           code: s.code, title: s.name, meta: level(s), indent: indent, dimmed: s.muted, warning: warn,
                           onRename: { n in var r = s; r.name = n; ctx.edit("Rename Source") { try $0.updateSpeaker(speaker: r) } })
            .draggable("wavey-entity:spk:\(s.id)")
    }

    private func isSub(_ s: Speaker) -> Bool {
        guard let m = ctx.store.modelsById[s.modelId] else { return false }
        return (m.nominalMaxHz ?? m.maxHz) <= 250
    }

    private func level(_ s: Speaker) -> String {
        String(format: "%+.1f dB", s.gainDb)
    }
}

/// Receivers as a tree: microphone lines and grids (groups of receivers)
/// with their microphones, single microphones, then receiver grids.
struct ReceiversSection: View {
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    let toggle: () -> Void

    var body: some View {
        let solved = ctx.store.microphones.filter(\.includeInSolve)
        let groups = ctx.store.groups.filter { !$0.receivers.isEmpty && $0.members.isEmpty }
        let grouped = Set(groups.flatMap(\.receivers))
        WDSectionHeader("Receivers", collapsed: collapsed, onToggle: toggle) {
            WDCount(solved.count + ctx.store.cutPlanes.count)
            Menu {
                Button("Microphone") { ctx.workspace = .sources; ctx.useTool(.microphone) }
                Button("Microphone Line") { ctx.workspace = .sources; ctx.useTool(.micLine) }
                Button("Microphone Grid") { ctx.workspace = .sources; ctx.useTool(.micGrid) }
                Divider()
                Button("Horizontal Cut Plane") { ctx.workspace = .sources; ctx.useTool(.planeH) }
                Button("Cut Plane on Surface") { ctx.workspace = .sources; ctx.useTool(.cutPlane) }
                Button("Receiver Grid…") { ctx.workspace = .sources; ctx.useTool(.receiverGrid) }
            } label: { WDIcon(.plus, size: 12) }
                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                .help("Add receivers")
        }
        .dropDestination(for: String.self) { items, _ in ctx.ungroup(items) }
        if !collapsed {
            // By category, as in Results: microphones (lines and grids as
            // groups under them), then receiver grids.
            category("mics", "Microphones", icon: .microphone, count: solved.count)
            if open("mics") {
                ForEach(groups, id: \.id) { g in
                    GroupRow(ctx: ctx, group: g, indent: 1)
                    if ctx.groupExpanded(g.id) {
                        ForEach(g.receivers, id: \.self) { id in
                            if let m = ctx.store.micsById[id] { micRow(m, indent: 3) }
                        }
                    }
                }
                ForEach(solved.filter { !grouped.contains($0.id) }, id: \.id) { m in micRow(m, indent: 2) }
                if solved.isEmpty { empty("Place one with the Microphone tool (R)") }
            }
            category("planes", "Receiver grids", icon: .cutPlane, count: ctx.store.cutPlanes.count)
            if open("planes") {
                ForEach(ctx.store.cutPlanes, id: \.id) { p in
                    OutlinerRow(ctx: ctx, entity: .plane(p.id), icon: .cutPlane, code: p.code, title: p.name,
                                meta: String(format: "%@ %.2f m", p.axis == .horizontal ? "z" : p.axis == .constX ? "x" : "y", p.offset), indent: 2,
                                onRename: { n in var r = p; r.name = n; ctx.edit("Rename Grid") { try $0.updateCutPlane(plane: r) } })
                }
                if ctx.store.cutPlanes.isEmpty { empty("Add one with the Cut plane tool (C)") }
            }
        }
    }

    private func open(_ key: String) -> Bool { !ctx.state.collapsed.contains("rcv." + key) }

    private func category(_ key: String, _ title: String, icon: WDIconName, count: Int) -> some View {
        OutlinerRow(ctx: ctx, entity: nil, icon: icon, title: title, meta: "\(count)", bold: true,
                    onTap: { flip(key) }, expanded: open(key), onExpand: { flip(key) })
    }

    private func flip(_ key: String) {
        let k = "rcv." + key
        if ctx.state.collapsed.contains(k) { ctx.state.collapsed.remove(k) } else { ctx.state.collapsed.insert(k) }
    }

    private func empty(_ text: String) -> some View {
        Text(text).font(.wd.detail).foregroundStyle(Color.wd.textTertiary)
            .padding(.leading, .wd.s8 + 2 * .wd.rowIndent).frame(height: .wd.row)
    }

    private func micRow(_ m: Microphone, indent: Int) -> some View {
        let warn = ctx.store.warnings.first { $0.entity == "mic:\(m.id)" }?.message
        return OutlinerRow(ctx: ctx, entity: .microphone(m.id), icon: .microphone, code: m.code, title: m.name,
                           meta: UnitFormatter(.length).string(m.position.z), indent: indent, warning: warn,
                           onRename: { n in var r = m; r.name = n; ctx.edit("Rename Receiver") { try $0.updateMicrophone(mic: r) } })
            .draggable("wavey-entity:mic:\(m.id)")
    }
}

/// A group as a parent row: disclosure, kind icon, member count; drop
/// sources or receivers on it to add them (plain groups).
struct GroupRow: View {
    @Bindable var ctx: WindowContext
    let group: GroupRec
    var indent = 0

    var body: some View {
        let g = group
        let n = g.members.count + g.receivers.count
        let muted = !g.members.isEmpty && g.members.allSatisfy { ctx.store.speakersById[$0]?.muted ?? false }
        OutlinerRow(ctx: ctx, entity: .group(g.id),
                    icon: g.kind == "stack" ? .stack : g.kind == "line_array" ? .lineArray : g.members.isEmpty ? .receiverGrid : .link,
                    iconColor: g.members.isEmpty ? Color.wd.textTertiary : Color.wd.source,
                    code: g.code, title: g.name, meta: "\(n)\(g.filters.isEmpty ? "" : " · EQ")", indent: indent, dimmed: muted, bold: true,
                    onRename: { name in var r = g; r.name = name; ctx.edit("Rename Group") { try $0.updateGroup(group: r) } },
                    expanded: ctx.groupExpanded(g.id), onExpand: { ctx.toggleGroupExpanded(g.id) })
            .dropDestination(for: String.self) { items, _ in ctx.addToGroup(g.id, items) }
            .contextMenu {
                if !g.kind.isEmpty { Button(g.kind == "stack" ? "Edit Stack…" : "Edit Line Array…") { ctx.editAssembly(g.id) } }
                if !g.members.isEmpty {
                    Button("Group EQ…") { ctx.openEQ(group: g.id) }
                    Button(muted ? "Unmute All" : "Mute All") { ctx.setGroupMuted(g.id, !muted) }
                }
                Button("Duplicate") { ctx.duplicateGroups([g.id]) }
                Divider()
                if !g.kind.isEmpty { Button("Ungroup Array (keep speakers)") { ctx.detachAssembly(g.id) } }
                Button("Ungroup") { ctx.edit("Ungroup") { $0.removeGroups(ids: [g.id]) } }
                Button("Delete Group and Members") { ctx.select([.group(g.id)]); ctx.deleteSelection() }
            }
    }
}

/// Devices by manufacturer: the stock library (folder) merged with the
/// models embedded in this project. Library devices are added to the
/// project when used (placed, dropped, picked in an editor).
struct LibrarySection: View {
    enum Part { case header, content }
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    var part: Part = .content
    let toggle: () -> Void
    @State private var query = ""
    @State private var closed: Set<String> = []
    @State private var previewing: String?
    @State private var hoverTask: Task<Void, Never>?
    private var library: SpeakerLibrary { SpeakerLibrary.shared }

    /// One device row: in the project (model id) and/or in the library (file).
    struct Item: Identifiable {
        var id: String { projectID.map { "p:\($0)" } ?? "l:\(path ?? name)" }
        let name: String
        let manufacturer: String
        let projectID: String?
        let path: String?
        let sub: Bool
        let omni: Bool
        let rigging: Bool
        var width = 0.4
        var height = 0.6
        var depth = 0.4
    }

    private var items: [Item] {
        var out: [Item] = []
        var inProject = Set<String>()
        for m in ctx.store.speakerModels {
            inProject.insert(m.id)
            let lib = library.entries.first { $0.id == m.id }
            // The library's spelling wins ("Yamaha" over a file's "YAMAHA").
            out.append(Item(name: m.name, manufacturer: SpeakerLibrary.groupName(lib?.manufacturer ?? m.manufacturer), projectID: m.id, path: lib?.path,
                            sub: (m.nominalMaxHz ?? m.maxHz) <= 250, omni: !m.hasBalloon, rigging: lib?.hasRigging ?? false,
                            width: m.widthM, height: m.heightM, depth: m.depthM))
        }
        for e in library.entries where !inProject.contains(e.id) {
            out.append(Item(name: e.name, manufacturer: SpeakerLibrary.groupName(e.manufacturer), projectID: nil, path: e.path,
                            sub: (e.nominalMaxHz ?? e.maxHz) <= 250, omni: !e.hasBalloon, rigging: e.hasRigging,
                            width: e.widthM, height: e.heightM, depth: e.depthM))
        }
        let q = query.trimmingCharacters(in: .whitespaces)
        return out.filter { q.isEmpty || $0.name.localizedCaseInsensitiveContains(q) || $0.manufacturer.localizedCaseInsensitiveContains(q) }
    }

    var body: some View {
        switch part {
        case .header: header
        case .content: content
        }
    }

    private var header: some View {
        WDSectionHeader("Library", collapsed: collapsed, onToggle: toggle) {
            if library.scanning { Text("scanning…").font(.wd.detail).foregroundStyle(Color.wd.textTertiary) }
            WDIconButton(.plus, size: .small, help: "Add speaker files to the library (.waveyspk)") { ctx.addFilesToLibrary() }
            Menu {
                Button("Add Files to Library…") { ctx.addFilesToLibrary() }
                Button("Import into Project Only…") { ctx.importDevice() }
                Button("Import Polar Table…") { ctx.importDeviceTable() }
                Divider()
                Button("Show Library in Finder") { library.reveal() }
                Button("Change Library Location…") { ctx.chooseLibraryLocation() }
                if !library.isDefaultLocation { Button("Use Default Location") { library.resetLocation() } }
                Button("Refresh") { library.refresh() }
            } label: { WDIcon(.more, size: 12) }
                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                .help("Library: \(library.location.path)")
        }
    }

    /// Search, devices by manufacturer, read errors.
    @ViewBuilder private var content: some View {
        if !collapsed {
            TextField("Search devices…", text: $query)
                .textFieldStyle(.plain)
                .font(.wd.body)
                .padding(.horizontal, 7)
                .frame(height: .wd.control)
                .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(Color.wd.controlFill))
                .padding(.horizontal, .wd.s8)
                .padding(.bottom, .wd.s4)
            let all = items
            if all.isEmpty {
                WDEmptyState(query.isEmpty ? "No devices" : "No matches",
                             detail: query.isEmpty ? "Add .waveyspk files to the library (\(library.location.lastPathComponent))." : nil,
                             actionTitle: query.isEmpty ? "Show library folder" : nil) { library.reveal() }
            }
            // One group per maker however it is spelled ("YAMAHA", "Funktion One"),
            // named as the library spells it, in a stable order.
            let groups = Dictionary(grouping: all) { Self.makerKey($0.manufacturer) }
            let names: [String: String] = groups.mapValues { items in
                let spellings = items.map(\.manufacturer)
                return spellings.first { s in library.entries.contains { SpeakerLibrary.groupName($0.manufacturer) == s } } ?? spellings.sorted().first ?? "Other"
            }
            ForEach(groups.keys.sorted { $0 == "other" ? false : $1 == "other" ? true : $0 < $1 }, id: \.self) { key in
                let maker = names[key] ?? key
                let rows = (groups[key] ?? []).sorted { ($0.name.lowercased(), $0.id) < ($1.name.lowercased(), $1.id) }
                let isOpen = !closed.contains(maker) || !query.isEmpty
                Button {
                    if closed.contains(maker) { closed.remove(maker) } else { closed.insert(maker) }
                } label: {
                    HStack(spacing: .wd.s4) {
                        WDIcon(isOpen ? .chevronDown : .chevronRight, size: 12).foregroundStyle(Color.wd.textTertiary)
                        Text(maker).font(.wd.label.weight(.medium)).foregroundStyle(Color.wd.textSecondary)
                        Spacer()
                        Text("\(rows.count)").font(.wd.numeric).foregroundStyle(Color.wd.textTertiary)
                    }
                    .padding(.horizontal, .wd.s12)
                    .frame(height: .wd.row)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                if isOpen {
                    ForEach(rows) { item in row(item) }
                }
            }
            if !library.errors.isEmpty {
                Text("\(library.errors.count) file\(library.errors.count == 1 ? "" : "s") could not be read").font(.wd.detail)
                    .foregroundStyle(Color.wd.statusOutdated).padding(.horizontal, .wd.s12).help(library.errors.joined(separator: "\n"))
            }
        }
    }

    /// Grouping key of a manufacturer: letters and digits, lower case.
    static func makerKey(_ m: String) -> String {
        String(m.lowercased().unicodeScalars.filter { CharacterSet.alphanumerics.contains($0) })
    }

    /// The name without the manufacturer the group already shows
    /// ("Void Acoustics Arcline 8" → "Arcline 8", "JBL Control 23-1" under
    /// "JBL Professional" → "Control 23-1").
    static func shortName(_ name: String, maker: String) -> String {
        for prefix in [maker, maker.split(separator: " ").first.map(String.init) ?? ""] where !prefix.isEmpty {
            if name.lowercased().hasPrefix(prefix.lowercased() + " "), name.count > prefix.count + 1 {
                return String(name.dropFirst(prefix.count + 1))
            }
        }
        return name
    }

    @ViewBuilder private func row(_ item: Item) -> some View {
        let icon: WDIconName = item.rigging ? .lineArray : item.omni ? .pointSource : item.sub ? .subwoofer : .speaker
        let entity: EntityRef = item.projectID.map { .device($0) } ?? .libraryItem(item.path ?? "")
        OutlinerRow(ctx: ctx, entity: entity, icon: icon, title: Self.shortName(item.name, maker: item.manufacturer),
                    meta: item.projectID != nil ? "in project" : "library", indent: 1, dimmed: false,
                    onTap: {
                        ctx.libraryDevice = item.projectID
                        ctx.select([entity])
                    },
                    onDoubleTap: { if let id = ctx.deviceID(for: item) { ctx.placeDeviceAtCentre(id) } })
            .onHover { inside in
                hoverTask?.cancel()
                if inside {
                    hoverTask = Task { @MainActor in
                        try? await Task.sleep(for: .milliseconds(400))
                        if !Task.isCancelled { previewing = item.id }
                    }
                } else if previewing == item.id {
                    previewing = nil
                }
            }
            .popover(isPresented: Binding(get: { previewing == item.id }, set: { if !$0 { previewing = nil } }), arrowEdge: .trailing) {
                SpeakerPreviewCard(ctx: ctx, path: item.path, modelID: item.projectID)
            }
            .draggable(ctx.beginDeviceDrag(item)) {
                HStack(spacing: .wd.s6) {
                    WDIcon(icon, size: 12).foregroundStyle(Color.wd.source)
                    Text(item.name).font(.wd.body).foregroundStyle(Color.wd.textPrimary)
                }
                .padding(.horizontal, .wd.s8).frame(height: .wd.row)
                .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(Color.wd.surfacePanel))
            }
            .contextMenu {
                Button("Place at centre") { if let id = ctx.deviceID(for: item) { ctx.placeDeviceAtCentre(id) } }
                if item.projectID == nil {
                    Button("Add to project") { _ = ctx.deviceID(for: item) }
                }
                if let p = item.path {
                    Button("Show in Finder") { library.reveal(p) }
                } else if let id = item.projectID {
                    Button("Add to library") { ctx.addProjectModelToLibrary(id) }
                }
                if let id = item.projectID {
                    if ctx.libraryFile(of: id) != nil {
                        Button("Update from library") { ctx.updateFromLibrary(id) }
                    }
                    Button("Export…") { ctx.exportDevice(id) }
                    Button("Remove from project") { ctx.edit("Remove Device") { try $0.removeSpeakerModel(id: id) } }
                }
            }
    }
}

/// Sources workspace: the geometry as one dimmed, locked row.
struct SceneSummarySection: View {
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    let toggle: () -> Void

    var body: some View {
        WDSectionHeader("Scene", collapsed: collapsed, onToggle: toggle)
        if !collapsed {
            OutlinerRow(ctx: ctx, entity: nil, icon: .layers, title: "Geometry", meta: nil, locked: true, onLock: {}, onTap: { ctx.workspace = .geometry })
            OutlinerRow(ctx: ctx, entity: nil, icon: .surface, title: "\(ctx.store.title) · \(ctx.store.room?.surfaces.count ?? 0) srf",
                        meta: ctx.store.levels.count >= 2 ? "L0–L1" : nil, indent: 1, dimmed: true, onTap: {})
        }
    }
}
