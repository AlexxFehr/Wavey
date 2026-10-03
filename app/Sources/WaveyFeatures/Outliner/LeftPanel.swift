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

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(order, id: \.self) { section($0) }
            }
            .padding(.bottom, .wd.s12)
        }
        .scrollIndicators(.never)
    }

    private var order: [String] {
        switch ctx.workspace {
        case .geometry: ["model", "layers", "surfaces", "sources", "receivers", "groups", "library", "results"]
        case .sources: ["scene", "sources", "receivers", "groups", "library", "model", "layers", "surfaces", "results"]
        case .results: ["results", "post", "scenetoggles", "sources", "receivers", "model", "layers", "surfaces", "groups", "library"]
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
        case "groups": GroupsSection(ctx: ctx, collapsed: collapsed(id)) { toggle(id) }
        case "library": LibrarySection(ctx: ctx, collapsed: collapsed(id)) { toggle(id) }
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
    @State private var hover = false
    @State private var renaming = false

    var selected: Bool { entity.map { ctx.selection.contains($0) } ?? false }
    /// Local hover only: reading the global hover here would re-evaluate
    /// every row on each viewport hover change.
    var hovered: Bool { hover }

    var body: some View {
        HStack(spacing: .wd.s6) {
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
        .gesture(TapGesture(count: 2).onEnded {
            if onRename != nil { renaming = true } else { onDoubleTap?() }
        }.exclusively(before: TapGesture().onEnded { tap() }))
        .contextMenu { if let entity { EntityMenu(ctx: ctx, entity: entity) } }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }

    private func tap() {
        if let onTap { onTap(); return }
        guard let entity else { return }
        let f = NSEvent.modifierFlags
        if f.contains(.command) { ctx.selection.toggle(entity) } else if f.contains(.shift) { ctx.selection.extend([entity]) } else { ctx.selection.select([entity]) }
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

struct SourcesSection: View {
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    let toggle: () -> Void

    var body: some View {
        WDSectionHeader("Sources", collapsed: collapsed, onToggle: toggle) {
            WDCount(ctx.store.speakers.count)
            WDIconButton(.plus, size: .small, help: "Add source") { ctx.tool.activate(.loudspeaker); ctx.workspace = .sources }
        }
        if !collapsed {
            ForEach(ctx.store.speakers, id: \.id) { s in
                let warn = ctx.store.warnings.first { $0.entity == "spk:\(s.id)" }?.message
                OutlinerRow(ctx: ctx, entity: .speaker(s.id), icon: isSub(s) ? .subwoofer : .speaker, iconColor: Color.wd.source,
                            code: s.code, title: s.name, meta: level(s), dimmed: s.muted, warning: warn,
                            onRename: { n in var r = s; r.name = n; ctx.edit("Rename Source") { try $0.updateSpeaker(speaker: r) } })
            }
        }
    }

    private func isSub(_ s: Speaker) -> Bool {
        guard let m = ctx.store.modelsById[s.modelId] else { return false }
        return (m.nominalMaxHz ?? m.maxHz) <= 250
    }

    private func level(_ s: Speaker) -> String {
        guard let m = ctx.store.modelsById[s.modelId] else { return "" }
        return String(format: "%.1f", m.sensitivityDb + s.gainDb)
    }
}

struct ReceiversSection: View {
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    let toggle: () -> Void

    var body: some View {
        let solved = ctx.store.microphones.filter(\.includeInSolve)
        WDSectionHeader("Receivers", collapsed: collapsed, onToggle: toggle) {
            WDCount(solved.count + ctx.store.cutPlanes.count)
            WDIconButton(.plus, size: .small, help: "Add receiver") { ctx.tool.activate(.receiver); ctx.workspace = .sources }
        }
        if !collapsed {
            ForEach(solved, id: \.id) { m in
                let warn = ctx.store.warnings.first { $0.entity == "mic:\(m.id)" }?.message
                OutlinerRow(ctx: ctx, entity: .microphone(m.id), icon: .microphone, code: m.code, title: m.name,
                            meta: UnitFormatter(.length).string(m.position.z), warning: warn,
                            onRename: { n in var r = m; r.name = n; ctx.edit("Rename Receiver") { try $0.updateMicrophone(mic: r) } })
            }
            ForEach(ctx.store.cutPlanes, id: \.id) { p in
                OutlinerRow(ctx: ctx, entity: .plane(p.id), icon: .receiverGrid, code: p.code, title: p.name,
                            meta: String(format: "%.1f m", p.spacing),
                            onRename: { n in var r = p; r.name = n; ctx.edit("Rename Grid") { try $0.updateCutPlane(plane: r) } })
            }
        }
    }
}

struct GroupsSection: View {
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    let toggle: () -> Void

    var body: some View {
        WDSectionHeader("Groups", collapsed: collapsed, onToggle: toggle) {
            WDCount(ctx.store.groups.count)
            WDIconButton(.plus, size: .small, help: "Group selected sources (⌘G)") { ctx.groupSelection() }
        }
        if !collapsed {
            if ctx.store.groups.isEmpty {
                Text("Select sources and press ⌘G").font(.wd.detail).foregroundStyle(Color.wd.textTertiary).padding(.leading, .wd.s12).frame(height: .wd.row)
            }
            ForEach(ctx.store.groups, id: \.id) { g in
                OutlinerRow(ctx: ctx, entity: .group(g.id), icon: g.kind == "stack" ? .stack : g.kind == "line_array" ? .lineArray : .link, iconColor: g.linked ? Color.wd.accentText : Color.wd.textTertiary,
                            code: g.code, title: g.name, meta: "\(g.members.count)",
                            onRename: { n in var r = g; r.name = n; ctx.edit("Rename Group") { try $0.updateGroup(group: r) } })
            }
        }
    }
}

struct LibrarySection: View {
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    let toggle: () -> Void
    @State private var query = ""

    var body: some View {
        WDSectionHeader("Library", collapsed: collapsed, onToggle: toggle) {
            WDIconButton(.plus, size: .small, help: "Import device (.waveyspk)") { ctx.importDevice() }
            Menu {
                Button("Import .waveyspk…") { ctx.importDevice() }
                Button("Import polar table…") { ctx.importDeviceTable() }
                Button("Add sample devices") { ctx.edit("Add Sample Devices") { try $0.addSampleSpeakerModels() } }
            } label: { WDIcon(.more, size: 12) }
                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
        }
        if !collapsed {
            TextField("Search devices…", text: $query)
                .textFieldStyle(.plain)
                .font(.wd.body)
                .padding(.horizontal, 7)
                .frame(height: .wd.control)
                .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(Color.wd.controlFill))
                .padding(.horizontal, .wd.s8)
                .padding(.bottom, .wd.s4)
            let models = ctx.store.speakerModels.filter { query.isEmpty || $0.name.localizedCaseInsensitiveContains(query) || ($0.manufacturer ?? "").localizedCaseInsensitiveContains(query) }
            if ctx.store.speakerModels.isEmpty {
                WDEmptyState("No devices", detail: "Import a .waveyspk or add the samples.", actionTitle: "Add sample devices") {
                    ctx.edit("Add Sample Devices") { try $0.addSampleSpeakerModels() }
                }
            }
            ForEach(models, id: \.id) { m in
                let sub = (m.nominalMaxHz ?? m.maxHz) <= 250
                OutlinerRow(ctx: ctx, entity: .device(m.id), icon: !m.hasBalloon ? .pointSource : sub ? .subwoofer : .speaker,
                            title: m.name, meta: m.hasBalloon ? (m.phaseEstimated ? "est." : "spk") : "—",
                            onTap: { ctx.libraryDevice = m.id; ctx.select([.device(m.id)]) },
                            onDoubleTap: { ctx.placeDeviceAtCentre(m.id) })
                    .onDrag { NSItemProvider(object: "wavey-device:\(m.id)" as NSString) }
                    .contextMenu {
                        Button("Place at centre") { ctx.placeDeviceAtCentre(m.id) }
                        Button("Export…") { ctx.exportDevice(m.id) }
                        Button("Remove from project") { ctx.edit("Remove Device") { try $0.removeSpeakerModel(id: m.id) } }
                    }
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
