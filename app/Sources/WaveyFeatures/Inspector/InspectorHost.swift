import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel

/// Right panel: content follows the primary selected entity type, never the workspace.
struct InspectorHost: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Inspector").font(.wd.section).foregroundStyle(Color.wd.sectionTitle)
                Spacer()
                WDIconButton(.close, size: .small, help: "Close inspector · reopen from Window ▸ Toggle Inspector (⌥⌘2) or by double-clicking an item") { ctx.inspectorVisible = false }
            }
            .padding(.horizontal, .wd.s12)
            .frame(height: .wd.sectionHeader)
            content
        }
    }

    @ViewBuilder private var content: some View {
        let sel = ctx.selection.ids
        let kinds = Set(sel.map(\.kind))
        if sel.isEmpty {
            if ctx.workspace == .results, let plane = ctx.store.cutPlanes.first(where: { $0.style.visible }) ?? ctx.store.cutPlanes.first {
                CutPlaneInspector(ctx: ctx, planes: [plane])
            } else if ctx.workspace == .results {
                ResultsFrequencyOnly(ctx: ctx)
            } else {
                ModelSummaryInspector(ctx: ctx)
            }
        } else if kinds.count > 1 {
            MixedInspector(ctx: ctx)
        } else {
            switch sel[0].kind {
            case .surface: SurfaceInspector(ctx: ctx, keys: ctx.selection.surfaces)
            case .segment: SegmentInspector(ctx: ctx, ids: ctx.selection.segments)
            case .speaker: SourceInspector(ctx: ctx, ids: ctx.selection.speakers)
            case .microphone: MicrophoneInspector(ctx: ctx, ids: ctx.selection.microphones)
            case .plane: CutPlaneInspector(ctx: ctx, planes: ctx.selection.planes.compactMap { ctx.store.planesById[$0] })
            case .group: GroupInspector(ctx: ctx, id: sel[0].groupID!)
            case .layer: LayerInspector(ctx: ctx, id: sel[0].layerID!)
            case .level: LevelInspector(ctx: ctx, id: sel[0].levelID!)
            case .device: DeviceInspector(ctx: ctx, id: sel[0].deviceID!)
            case .libraryItem: LibraryItemInspector(ctx: ctx, path: sel[0].libraryPath!)
            case .material: MaterialInspectorView(ctx: ctx, id: sel[0].materialID!)
            case .run: ModelSummaryInspector(ctx: ctx)
            }
        }
    }
}

// MARK: Building blocks

/// Entity header: icon tile, editable name, subtitle and trailing toggles.
struct InspectorHeader<Trailing: View>: View {
    let icon: WDIconName
    var tileFill: Color = .wd.accentFill
    var tileStroke: Color?
    var iconColor: Color = .wd.accentText
    let name: String
    let subtitle: String
    var onRename: ((String) -> Void)?
    let trailing: Trailing

    init(icon: WDIconName, tileFill: Color = .wd.accentFill, tileStroke: Color? = nil, iconColor: Color = .wd.accentText,
         name: String, subtitle: String, onRename: ((String) -> Void)? = nil, @ViewBuilder trailing: () -> Trailing) {
        self.icon = icon
        self.tileFill = tileFill
        self.tileStroke = tileStroke
        self.iconColor = iconColor
        self.name = name
        self.subtitle = subtitle
        self.onRename = onRename
        self.trailing = trailing()
    }

    var body: some View {
        HStack(alignment: .center, spacing: .wd.s8) {
            WDIcon(icon, size: 12)
                .foregroundStyle(iconColor)
                .frame(width: 22, height: 22)
                .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(tileFill))
                .overlay { if let tileStroke { RoundedRectangle(cornerRadius: .wd.radiusField).strokeBorder(tileStroke, lineWidth: 1) } }
            VStack(alignment: .leading, spacing: 1) {
                if let onRename {
                    WDTextField(name, chrome: false, font: .wd.title, onCommit: onRename)
                } else {
                    Text(name).font(.wd.title).foregroundStyle(Color.wd.textPrimary).lineLimit(1)
                }
                Text(subtitle).font(.wd.detail).foregroundStyle(Color.wd.textTertiary).lineLimit(1)
            }
            Spacer(minLength: .wd.s4)
            trailing
        }
        .padding(.horizontal, .wd.s12)
        .padding(.vertical, .wd.s8)
    }
}

extension InspectorHeader where Trailing == EmptyView {
    init(icon: WDIconName, tileFill: Color = .wd.accentFill, tileStroke: Color? = nil, iconColor: Color = .wd.accentText,
         name: String, subtitle: String, onRename: ((String) -> Void)? = nil) {
        self.init(icon: icon, tileFill: tileFill, tileStroke: tileStroke, iconColor: iconColor, name: name, subtitle: subtitle, onRename: onRename) { EmptyView() }
    }
}

/// Collapsible inspector section with a right-aligned note.
struct InspectorSection<Content: View>: View {
    let title: String
    var note: String?
    var noteAccent = false
    let content: Content
    @State private var collapsed = false

    init(_ title: String, note: String? = nil, noteAccent: Bool = false, @ViewBuilder content: () -> Content) {
        self.title = title
        self.note = note
        self.noteAccent = noteAccent
        self.content = content()
    }

    var body: some View {
        WDSectionHeader(title, collapsed: collapsed, onToggle: { collapsed.toggle() }) {
            if let note {
                Text(note).font(noteAccent ? .wd.meta : .wd.detail).foregroundStyle(noteAccent ? Color.wd.accentText : Color.wd.textTertiary)
            }
        }
        if !collapsed {
            VStack(alignment: .leading, spacing: 0) { content }.padding(.bottom, .wd.s6)
        }
    }
}

/// Scrolling body with an optional pinned footer of up to two buttons.
struct InspectorBody<Content: View, Footer: View>: View {
    let content: Content
    let footer: Footer
    init(@ViewBuilder content: () -> Content, @ViewBuilder footer: () -> Footer) {
        self.content = content()
        self.footer = footer()
    }
    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) { content }.padding(.bottom, .wd.s12)
            }
            .scrollIndicators(.never)
            HStack(spacing: .wd.s8) { footer }
                .padding(.wd.s12)
        }
    }
}

extension InspectorBody where Footer == EmptyView {
    init(@ViewBuilder content: () -> Content) {
        self.init(content: content) { EmptyView() }
    }
}

/// Shared value of a property across a multi-selection (nil = mixed).
func shared<T: Equatable, V: Equatable>(_ items: [T], _ f: (T) -> V) -> V? {
    guard let first = items.first.map(f) else { return nil }
    return items.dropFirst().allSatisfy { f($0) == first } ? first : nil
}

// MARK: Mixed and summary

struct MixedInspector: View {
    @Bindable var ctx: WindowContext
    var body: some View {
        let groups = Dictionary(grouping: ctx.selection.ids, by: \.kind)
        InspectorBody {
            InspectorHeader(icon: .layers, name: "\(ctx.selection.ids.count) selected", subtitle: "Mixed selection")
            ForEach(groups.keys.sorted { $0.rawValue < $1.rawValue }, id: \.self) { k in
                Button { ctx.select(groups[k]!) } label: {
                    HStack {
                        Text(k.rawValue.capitalized + "s").font(.wd.body).foregroundStyle(Color.wd.textSecondary)
                        Spacer()
                        WDCount(groups[k]!.count)
                    }
                    .padding(.horizontal, .wd.s12)
                    .frame(height: .wd.row)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
    }
}

struct ModelSummaryInspector: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        let s = ctx.store
        let len = UnitFormatter(.length)
        InspectorBody {
            InspectorHeader(icon: .cube, name: s.title, subtitle: "Model · nothing selected")
            InspectorSection("Model") {
                if let r = s.room {
                    WDPropertyRow("Volume") { WDValueText(String(format: "%.1f m³", r.volume)) }
                    WDPropertyRow("Floor area") { WDValueText(String(format: "%.1f m²", r.floorArea)) }
                    WDPropertyRow("Surface area") { WDValueText(String(format: "%.1f m²", r.totalSurfaceArea)) }
                    WDPropertyRow("Surfaces") { WDValueText("\(r.surfaces.count)") }
                    WDPropertyRow("Bounds") {
                        WDValueText("\(len.string(r.boundsMax.x - r.boundsMin.x)) × \(len.string(r.boundsMax.y - r.boundsMin.y)) × \(len.string(r.boundsMax.z - r.boundsMin.z)) m")
                    }
                    WDPropertyRow("Closed") { WDValueText("✓ volume built", color: Color.wd.statusCurrent) }
                    WDPropertyRow("Mean α 500") { WDValueText(String(format: "%.2f", meanAlpha(r))) }
                } else {
                    WDPropertyRow("Volume") { WDValueText(s.roomError ?? "No closed interior", color: Color.wd.statusOutdated) }
                }
                WDPropertyRow("Sources") { WDValueText("\(s.speakers.count)") }
                WDPropertyRow("Receivers") { WDValueText("\(s.microphones.filter(\.includeInSolve).count) points · \(s.cutPlanes.count) grids") }
                if let run = s.activeRun {
                    WDPropertyRow("Active run") { WDValueText("\(run.name) · \(run.stale ? "outdated" : "current")") }
                }
            }
            InspectorSection("Issues", note: "\(s.issues.count)") {
                if s.issues.isEmpty {
                    Text("No issues").font(.wd.body).foregroundStyle(Color.wd.textTertiary).padding(.horizontal, .wd.s12)
                }
                ForEach(Array(s.issues.enumerated()), id: \.offset) { _, i in
                    HStack(spacing: .wd.s6) {
                        Circle().fill(i.severity == .error ? Color.wd.statusError : Color.wd.statusOutdated).frame(width: 6, height: 6)
                        Text(i.message).font(.wd.body).foregroundStyle(Color.wd.textSecondary).lineLimit(2)
                        Spacer()
                        if let key = i.entity, let e = EntityRef(engineKey: key) {
                            WDTextButton("Zoom to") { ctx.reveal(e) }
                        }
                    }
                    .padding(.horizontal, .wd.s12)
                    .frame(minHeight: .wd.row)
                }
            }
            InspectorSection("Air") {
                let m = s.medium
                WDPropertyRow("Temperature") {
                    WDNumberField(m.temperatureC, UnitFormatter(.temperature), range: -50...60) { v in setMedium { $0.temperatureC = v } }
                }
                WDPropertyRow("Humidity") {
                    WDNumberField(m.relativeHumidityPct, UnitFormatter(.percent), range: 0...100) { v in setMedium { $0.relativeHumidityPct = v } }
                }
                WDPropertyRow("Pressure") {
                    WDNumberField(m.pressureKpa, UnitFormatter(.plain, decimals: 1), range: 50...120, unit: "kPa") { v in setMedium { $0.pressureKpa = v } }
                }
                WDPropertyRow("c") { WDValueText(String(format: "%.1f m/s", m.speedOfSound)) }
            }
        }
    }

    private func setMedium(_ f: (inout MediumRec) -> Void) {
        var m = ctx.store.medium
        f(&m)
        ctx.edit("Set Air") { try $0.setMedium(medium: m) }
    }

    private func meanAlpha(_ r: RoomSnapshot) -> Double {
        var a = 0.0, area = 0.0
        for s in r.surfaces {
            guard let m = ctx.store.materialsById[s.materialId] else { continue }
            let spec = m.absorption
            let v = spec.freqsHz.isEmpty ? (spec.values.first ?? 0) : spec.values[spec.freqsHz.indices.min { abs(spec.freqsHz[$0] - 500) < abs(spec.freqsHz[$1] - 500) }!]
            a += v * s.area
            area += s.area
        }
        return area > 0 ? a / area : 0
    }
}
