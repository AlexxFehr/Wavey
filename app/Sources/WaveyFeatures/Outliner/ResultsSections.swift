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

/// Cut planes, microphones and probe lines.
struct PostSection: View {
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    let toggle: () -> Void

    var body: some View {
        WDSectionHeader("Post-processing", collapsed: collapsed, onToggle: toggle) {
            Menu {
                Button("Cut plane") { ctx.tool.activate(.cutPlane) }
                Button("Microphone") { ctx.tool.activate(.microphone) }
                Button("Probe line") { ctx.tool.activate(.probeLine) }
            } label: { WDIcon(.plus, size: 12) }
                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
        }
        if !collapsed {
            OutlinerRow(ctx: ctx, entity: nil, icon: .chevronDown, title: "Cut planes", onTap: {})
            ForEach(ctx.store.cutPlanes, id: \.id) { p in
                let axis = p.axis == .horizontal ? "XY z" : p.axis == .constX ? "YZ x" : "XZ y"
                let solved = ctx.store.resultInfo?.planeIds.contains(p.id) ?? false
                OutlinerRow(ctx: ctx, entity: .plane(p.id), icon: .cutPlane, iconColor: ctx.selection.contains(.plane(p.id)) ? Color.wd.accent : Color.wd.textTertiary,
                            code: p.code, title: "\(axis) \(UnitFormatter(.length).string(p.offset))", meta: solved ? "SPL" : "needs solve", indent: 1,
                            visible: p.style.visible,
                            onVisible: { var r = p; r.style.visible.toggle(); ctx.edit("Toggle Plane") { try $0.updateCutPlane(plane: r) } })
            }
            OutlinerRow(ctx: ctx, entity: nil, icon: .chevronDown, title: "Microphones", onTap: {})
            ForEach(Array(ctx.store.microphones.enumerated()), id: \.element.id) { i, m in
                let c = m.color.isEmpty ? Color.wd.curve(i) : (Color(hex: m.color) ?? Color.wd.curve(i))
                OutlinerRow(ctx: ctx, entity: .microphone(m.id), icon: nil, swatch: nil, code: m.code, title: m.name,
                            meta: ctx.micLevel(m.id).map { String(format: "%.1f", $0) } ?? "—", indent: 1, visible: m.visible,
                            onVisible: { var r = m; r.visible.toggle(); ctx.edit("Toggle Microphone") { try $0.updateMicrophone(mic: r) } })
                    .overlay(alignment: .leading) {
                        Circle().strokeBorder(c, lineWidth: 1.5).frame(width: 10, height: 10).padding(.leading, 28).allowsHitTesting(false)
                    }
            }
            OutlinerRow(ctx: ctx, entity: nil, icon: .chevronDown, title: "Probe lines", onTap: {})
            if let (a, b) = ctx.probeLine {
                OutlinerRow(ctx: ctx, entity: nil, icon: .probeLine, code: "PL-1", title: String(format: "%.2f m line", simd_distance(a, b)),
                            meta: "SPL", indent: 1, onTap: { ctx.results.plotMode = "spatial" })
            }
        }
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
