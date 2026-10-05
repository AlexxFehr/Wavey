import simd
import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel

/// Run selector and run metadata.
struct RunSection: View {
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    let toggle: () -> Void

    var body: some View {
        WDSectionHeader("Result set", collapsed: collapsed, onToggle: toggle) {
            Menu {
                if let r = ctx.store.activeRun {
                    Button("Rename…") { ctx.renameRun(r) }
                    Button("Delete \(r.name)") { ctx.deleteRun(r) }
                }
                Button("Solve settings…") { ctx.solveSettingsPresented = true }
            } label: { WDIcon(.more, size: 12) }
                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
        }
        if !collapsed {
            VStack(alignment: .leading, spacing: .wd.s4) {
                if ctx.store.runs.isEmpty {
                    Text("No runs yet").font(.wd.body).foregroundStyle(Color.wd.textTertiary)
                } else {
                    WDDropdown(ctx.store.runs.reversed().map { r in
                        WDDropdownItem(r.id, "\(r.name) · \(method(r))", swatch: r.stale ? Color.wd.statusOutdated : Color.wd.statusCurrent,
                                       detail: formatDuration(r.seconds))
                    }, selection: ctx.store.activeRun?.id) { id in
                        _ = ctx.store.doc.setActiveRun(id: id)
                        ctx.store.refresh()
                        ctx.afterChange()
                    }
                    if let r = ctx.store.activeRun {
                        Group {
                            Text(rangeLine(r))
                            Text(sourcesLine(r))
                            Text("Solved \(Date(timeIntervalSince1970: TimeInterval(r.created)).formatted(date: .abbreviated, time: .shortened)) · \(formatDuration(r.seconds))")
                        }
                        .font(.wd.meta)
                        .foregroundStyle(Color.wd.textTertiary)
                        .lineLimit(1)
                        if r.stale {
                            HStack(spacing: .wd.s6) {
                                WDChip("Outdated · \(ctx.store.changesSinceRun.joined(separator: ", "))", isOn: false) { ctx.runPopoverPresented = true }
                            }
                        }
                    }
                }
            }
            .padding(.horizontal, .wd.s12)
            .padding(.bottom, .wd.s6)
        }
    }

    private func method(_ r: RunRec) -> String {
        r.backend.lowercased().contains("gpu") ? "GPU" : "CPU"
    }

    private func rangeLine(_ r: RunRec) -> String {
        let res = r.octaveFraction > 0 ? "Δf 1/\(r.octaveFraction) oct" : String(format: "Δf %.2g Hz", r.linearStepHz)
        return "\(Int(r.fMin))–\(Int(r.fMax)) Hz · \(res)"
    }

    private func sourcesLine(_ r: RunRec) -> String {
        let s = r.sources.isEmpty ? "\(ctx.store.speakers.count) sources" : r.sources.joined(separator: "+")
        return "\(s) · coherent sum"
    }
}

/// Receivers by category: microphones (lines and grids as groups),
/// receiver grids (cut planes) and probe lines. Double-click a microphone
/// for its responses.
struct PostSection: View {
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    let toggle: () -> Void

    var body: some View {
        WDSectionHeader("Receivers", collapsed: collapsed, onToggle: toggle) {
            Menu {
                Button("Microphone") { ctx.tool.activate(.microphone) }
                Button("Cut plane") { ctx.tool.activate(.cutPlane) }
                Button("Probe line") { ctx.tool.activate(.probeLine) }
            } label: { WDIcon(.plus, size: 12) }
                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
        }
        if !collapsed {
            let mics = ctx.store.microphones
            let micGroups = ctx.store.groups.filter { !$0.receivers.isEmpty && $0.members.isEmpty }
            category("mics", "Microphones", icon: .microphone, count: mics.count)
            if open("mics") {
                // A group sits where its first member is.
                ForEach(ctx.outlinerMicItems(micGroups, mics: mics), id: \.self) { item in
                    switch item {
                    case .group(let gid):
                        if let g = ctx.store.groupsById[gid] {
                            OutlinerRow(ctx: ctx, entity: .group(g.id), icon: .receiverGrid, code: g.code, title: g.name, meta: "\(g.receivers.count)", indent: 1, bold: true,
                                        onRename: { n in var r = g; r.name = n; ctx.edit("Rename Group") { try $0.updateGroup(group: r) } },
                                        expanded: ctx.groupExpanded(g.id), onExpand: { ctx.toggleGroupExpanded(g.id) })
                            if ctx.groupExpanded(g.id) {
                                ForEach(g.receivers, id: \.self) { id in
                                    if let i = mics.firstIndex(where: { $0.id == id }) { micRow(mics[i], index: i, indent: 3) }
                                }
                            }
                        }
                    case .microphone(let id):
                        if let i = mics.firstIndex(where: { $0.id == id }) { micRow(mics[i], index: i, indent: 2) }
                    default:
                        EmptyView()
                    }
                }
                if mics.isEmpty { empty("Place one with the Microphone tool (R)") }
            }
            category("planes", "Receiver grids", icon: .cutPlane, count: ctx.store.cutPlanes.count)
            if open("planes") {
                ForEach(ctx.store.cutPlanes, id: \.id) { p in
                    let axis = p.axis == .horizontal ? "XY z" : p.axis == .constX ? "YZ x" : "XZ y"
                    let solved = ctx.store.resultInfo?.planeIds.contains(p.id) ?? false
                    OutlinerRow(ctx: ctx, entity: .plane(p.id), icon: .cutPlane, iconColor: ctx.selection.contains(.plane(p.id)) ? Color.wd.accent : Color.wd.textTertiary,
                                code: p.code, title: "\(axis) \(UnitFormatter(.length).string(p.offset))", meta: solved ? "SPL" : "needs solve", indent: 2,
                                visible: p.style.visible,
                                onVisible: { var r = p; r.style.visible.toggle(); ctx.edit("Toggle Plane") { try $0.updateCutPlane(plane: r) } })
                }
                if ctx.store.cutPlanes.isEmpty { empty("Add one with the Cut plane tool (C)") }
            }
            category("probes", "Probe lines", icon: .probeLine, count: ctx.probeLine == nil ? 0 : 1)
            if open("probes") {
                if let (a, b) = ctx.probeLine {
                    OutlinerRow(ctx: ctx, entity: nil, icon: .probeLine, code: "PL-1", title: String(format: "%.2f m line", simd_distance(a, b)),
                                meta: "SPL", indent: 2, onTap: { ctx.openProbeLine() })
                } else {
                    empty("Draw one with the Probe line tool (P)")
                }
            }
        }
    }

    private func open(_ key: String) -> Bool { !ctx.state.collapsed.contains("post." + key) }

    private func category(_ key: String, _ title: String, icon: WDIconName, count: Int) -> some View {
        OutlinerRow(ctx: ctx, entity: nil, icon: icon, title: title, meta: "\(count)", bold: true,
                    onTap: { flip(key) }, expanded: open(key), onExpand: { flip(key) })
    }

    private func flip(_ key: String) {
        let k = "post." + key
        if ctx.state.collapsed.contains(k) { ctx.state.collapsed.remove(k) } else { ctx.state.collapsed.insert(k) }
    }

    private func empty(_ text: String) -> some View {
        Text(text).font(.wd.detail).foregroundStyle(Color.wd.textTertiary)
            .padding(.leading, .wd.s8 + 2 * .wd.rowIndent).frame(height: .wd.row)
    }

    private func micRow(_ m: Microphone, index i: Int, indent: Int) -> some View {
        let c = m.color.isEmpty ? Color.wd.curve(i) : (Color(hex: m.color) ?? Color.wd.curve(i))
        return OutlinerRow(ctx: ctx, entity: .microphone(m.id), icon: nil, swatch: c, code: m.code, title: m.name,
                           meta: ctx.micLevel(m.id).map { String(format: "%.1f dB", $0) } ?? "—", indent: indent, visible: m.visible,
                           onVisible: { var r = m; r.visible.toggle(); ctx.edit("Toggle Microphone") { try $0.updateMicrophone(mic: r) } },
                           onRename: { n in var r = m; r.name = n; ctx.edit("Rename Receiver") { try $0.updateMicrophone(mic: r) } })
            .help("Double-click for its responses")
    }
}

struct SceneTogglesSection: View {
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    let toggle: () -> Void

    var body: some View {
        WDSectionHeader("Scene", collapsed: collapsed, onToggle: toggle)
        if !collapsed {
            OutlinerRow(ctx: ctx, entity: nil, icon: .layers, title: "Geometry", meta: "wire", visible: ctx.flag("show.wire"),
                        onVisible: { ctx.toggleFlag("show.wire") }, onTap: {})
            OutlinerRow(ctx: ctx, entity: nil, icon: .speaker, iconColor: Color.wd.source, title: "Sources", meta: "\(ctx.store.speakers.count)",
                        visible: ctx.flag("show.sources"), onVisible: { ctx.toggleFlag("show.sources") }, onTap: {})
        }
    }
}
