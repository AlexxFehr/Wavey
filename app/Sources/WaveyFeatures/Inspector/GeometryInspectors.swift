import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel
import WaveyPlots

/// Surfaces (walls, floors, ceilings): geometry, material, display.
struct SurfaceInspector: View {
    @Bindable var ctx: WindowContext
    let keys: [String]
    @State private var picker = false

    var body: some View {
        let s = ctx.store
        let surfaces = keys.compactMap { s.surfacesByKey[$0] }
        if let first = surfaces.first {
            let len = UnitFormatter(.length)
            let materialID = shared(surfaces) { $0.materialId }
            let material = materialID.flatMap { s.materialsById[$0] }
            let layerName = s.layersById[first.layerId]?.name ?? first.layerId
            InspectorBody {
                InspectorHeader(icon: .surface, name: surfaces.count == 1 ? "\(first.code) · \(s.name(.surface(first.key)))" : "\(surfaces.count) surfaces",
                                subtitle: "\(kindName(first.kind)) surface · Layer \(layerName) · \(surfaces.count) selected",
                                onRename: { n in
                                    let name = surfaces.count == 1 ? n.replacingOccurrences(of: "\(first.code) · ", with: "") : n
                                    ctx.edit("Rename Surface") { try $0.setSurfaceNames(surfaceKeys: keys, name: name) }
                                })
                if surfaces.count == 1, let fid = s.doc.featureOfSurface(surfaceKey: first.key), let f = s.featuresById[fid] {
                    InspectorSection("Made by") {
                        WDPropertyRow(f.label) {
                            WDTextButton("\(f.name) · edit") { ctx.select([.feature(fid)]) }
                        }
                    }
                }
                InspectorSection("Geometry", note: "m") {
                    if surfaces.count == 1 {
                        geometryRows(first, len)
                    } else {
                        WDPropertyRow("Area") { WDValueText(String(format: "%.3f m²", surfaces.map(\.area).reduce(0, +))) }
                    }
                }
                InspectorSection("Acoustic material") {
                    WDPropertyRow("Material") {
                        Button { picker.toggle() } label: {
                            HStack(spacing: .wd.s6) {
                                Circle().fill(Color(hex: material?.color ?? "") ?? .gray).frame(width: 8, height: 8)
                                Text(material?.name ?? "—").font(.wd.body).foregroundStyle(Color.wd.textSecondary).lineLimit(1)
                                Spacer()
                                WDIcon(.chevronDown, size: 12).foregroundStyle(Color.wd.textTertiary)
                            }
                            .padding(.horizontal, 7)
                            .frame(height: .wd.control)
                            .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(Color.wd.controlFill))
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .popover(isPresented: $picker, arrowEdge: .leading) {
                            MaterialPicker(ctx: ctx, selected: materialID) { id in
                                picker = false
                                ctx.assignMaterial(id, to: keys)
                            }
                        }
                    }
                    WDPropertyRow("Model") {
                        WDValueText(material.map { $0.angleDependent ? "Angle-dependent (impedance)" : "Random-incidence α" } ?? "—")
                    }
                    WDPropertyRow("Active side") {
                        WDSegmented([("front", "Front")], selection: "front") { _ in }
                            .help("Walls are single-sided towards the room; the inside face is the active side")
                    }
                    if let m = material { MaterialBands(material: m).padding(.horizontal, .wd.s12).padding(.top, .wd.s6) }
                }
                InspectorSection("Display") {
                    WDPropertyRow("Layer") {
                        if first.kind == .wall {
                            WDDropdown(s.layers.filter { !["floor", "ceiling"].contains($0.id) }.map { WDDropdownItem($0.id, $0.name, swatch: Color(hex: $0.color)) },
                                       selection: shared(surfaces) { $0.layerId }) { ctx.moveSelection(toLayer: $0) }
                        } else {
                            WDValueText(layerName)
                        }
                    }
                }
            } footer: {
                Button("Match properties") { ctx.matchProperties(from: first) }.buttonStyle(.wd).frame(maxWidth: .infinity)
                Button("Assign to layer…") {}.buttonStyle(.wd).frame(maxWidth: .infinity).hidden()
            }
        }
    }

    private func kindName(_ k: SurfaceKindRec) -> String {
        switch k {
        case .wall: "Wall"
        case .floor: "Floor"
        case .ceiling: "Ceiling"
        }
    }

    @ViewBuilder private func geometryRows(_ srf: SurfaceMesh, _ len: UnitFormatter) -> some View {
        if srf.kind == .wall, let segID = srf.segmentId, let seg = ctx.store.segmentsById[segID] {
            WDPropertyRow("Start X, Y") {
                WDVec3Field([seg.pa.x, seg.pa.y], len) { i, v in
                    ctx.edit("Move Vertex", coalesce: "v\(seg.a)") { $0.moveVertex(id: seg.a, to: Point2(x: i == 0 ? v : seg.pa.x, y: i == 1 ? v : seg.pa.y)) }
                }
            }
            WDPropertyRow("End X, Y") {
                WDVec3Field([seg.pb.x, seg.pb.y], len) { i, v in
                    ctx.edit("Move Vertex", coalesce: "v\(seg.b)") { $0.moveVertex(id: seg.b, to: Point2(x: i == 0 ? v : seg.pb.x, y: i == 1 ? v : seg.pb.y)) }
                }
            }
            let dx = seg.pb.x - seg.pa.x, dy = seg.pb.y - seg.pa.y
            WDPropertyRow("Length") {
                HStack(spacing: .wd.s4) {
                    WDNumberField((dx * dx + dy * dy).squareRoot(), len, range: 0.001...1000) { v in setLength(seg, v) }
                    WDValueText("∠ " + UnitFormatter(.angle, decimals: 2).string(atan2(dy, dx) * 180 / .pi) + "°", color: Color.wd.textTertiary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            let h = ctx.store.levels.first { $0.kind == .ceiling }?.z ?? 0
            WDPropertyRow("Base / Top Z") {
                HStack(spacing: .wd.s4) {
                    WDNumberField(0, len) { _ in }.disabled(true)
                    WDNumberField(h, len, range: 0.1...200) { v in
                        ctx.edit("Set Height") { _ = try $0.upsertLevel(level: LevelRec(id: "ceiling", name: "Ceiling", z: v, kind: .ceiling)) }
                    }
                }
            }
        } else {
            WDPropertyRow("Base / Top Z") { WDValueText(len.string(Double(srf.positions.count > 2 ? srf.positions[2] : 0))) }
        }
        WDPropertyRow("Normal") { WDValueText(String(format: "%.3f · %.3f · %.3f", srf.normal.x, srf.normal.y, srf.normal.z).replacingOccurrences(of: "-", with: "\u{2212}")) }
        WDPropertyRow("Area") { WDValueText(String(format: "%.3f m²", srf.area)) }
    }

    /// Moves the end vertex along the wall to set its length.
    private func setLength(_ seg: SketchSegment, _ v: Double) {
        let dx = seg.pb.x - seg.pa.x, dy = seg.pb.y - seg.pa.y
        let l = (dx * dx + dy * dy).squareRoot()
        guard l > 1e-9 else { return }
        let p = Point2(x: seg.pa.x + dx / l * v, y: seg.pa.y + dy / l * v)
        ctx.edit("Set Length") { $0.moveVertex(id: seg.b, to: p) }
    }
}

extension WindowContext {
    /// Match properties: copies the material of `source` to the other selected surfaces.
    func matchProperties(from source: SurfaceMesh) {
        let targets = selection.surfaces.filter { $0 != source.key }
        guard !targets.isEmpty else { post("Select the target surfaces too (⇧-click), with the source surface first", .info); return }
        assignMaterial(source.materialId, to: targets)
    }
}

/// Construction edges selected in the plan that do not bound the volume.
struct SegmentInspector: View {
    @Bindable var ctx: WindowContext
    let ids: [UInt64]
    var body: some View {
        let segs = ids.compactMap { ctx.store.segmentsById[$0] }
        let len = UnitFormatter(.length)
        InspectorBody {
            InspectorHeader(icon: .line, name: segs.count == 1 ? "Edge \(segs[0].id)" : "\(segs.count) edges", subtitle: "Sketch edge · not part of the volume")
            InspectorSection("Geometry", note: "m") {
                if segs.count == 1, let s = segs.first {
                    WDPropertyRow("Start X, Y") { WDValueText("\(len.string(s.pa.x)), \(len.string(s.pa.y))") }
                    WDPropertyRow("End X, Y") { WDValueText("\(len.string(s.pb.x)), \(len.string(s.pb.y))") }
                }
                WDPropertyRow("Length") {
                    WDValueText(len.withUnit(segs.map { hypot($0.pb.x - $0.pa.x, $0.pb.y - $0.pa.y) }.reduce(0, +)))
                }
            }
            InspectorSection("Display") {
                WDPropertyRow("Layer") {
                    WDDropdown(ctx.store.layers.filter { !["floor", "ceiling"].contains($0.id) }.map { WDDropdownItem($0.id, $0.name, swatch: Color(hex: $0.color)) },
                               selection: shared(segs) { $0.layerId }) { ctx.moveSelection(toLayer: $0) }
                }
            }
        }
    }
}

struct LayerInspector: View {
    @Bindable var ctx: WindowContext
    let id: String
    var body: some View {
        if let l = ctx.store.layersById[id] {
            InspectorBody {
                InspectorHeader(icon: .layers, name: l.name, subtitle: l.builtin ? "Layer · built-in" : "Layer", onRename: { n in update(l) { $0.name = n } })
                InspectorSection("Layer") {
                    WDPropertyRow("Colour") {
                        HStack(spacing: 4) {
                            ForEach(Color.wd.swatches, id: \.self) { hex in
                                Circle().fill(Color(hex: hex) ?? .gray).frame(width: 14, height: 14)
                                    .overlay(Circle().strokeBorder(hex == l.color ? Color.wd.textPrimary : .clear, lineWidth: 1.5))
                                    .onTapGesture { update(l) { $0.color = hex } }
                            }
                        }
                    }
                    WDPropertyRow("Visible") { WDCheckbox(isOn: l.visible) { v in update(l) { $0.visible = v } } }
                    WDPropertyRow("Locked") { WDCheckbox(isOn: l.locked) { v in update(l) { $0.locked = v } } }
                    WDPropertyRow("Current") {
                        WDCheckbox(isOn: ctx.store.currentLayer == l.id) { _ in ctx.edit("Set Current Layer") { try $0.setCurrentLayer(id: l.id) } }
                    }
                }
            }
        }
    }
    private func update(_ l: LayerRec, _ f: (inout LayerRec) -> Void) {
        var n = l
        f(&n)
        ctx.edit("Change Layer") { _ = try $0.upsertLayer(layer: n) }
    }
}

struct LevelInspector: View {
    @Bindable var ctx: WindowContext
    let id: String
    var body: some View {
        if let l = ctx.store.levels.first(where: { $0.id == id }) {
            InspectorBody {
                InspectorHeader(icon: .layers, name: l.name, subtitle: l.kind == .datum ? "Reference level" : "Level · \(l.kind == .floor ? "floor" : "ceiling (room height)")")
                InspectorSection("Level", note: "m") {
                    WDPropertyRow("z") {
                        WDNumberField(l.z, UnitFormatter(.length), range: l.kind == .ceiling ? 0.1...200 : -100...200) { v in
                            ctx.edit(l.kind == .ceiling ? "Set Height" : "Move Level") { _ = try $0.upsertLevel(level: LevelRec(id: l.id, name: l.name, z: v, kind: l.kind)) }
                        }
                        .disabled(l.kind == .floor)
                    }
                    if l.kind == .datum {
                        WDPropertyRow("") { WDTextButton("Delete level") { ctx.edit("Delete Level") { $0.removeLevel(id: l.id) }; ctx.select([]) } }
                    }
                }
            }
        }
    }
}

/// α and s bars plus the band table.
struct MaterialBands: View {
    let material: MaterialRec
    static let bands: [Double] = [63, 125, 250, 500, 1000, 2000, 4000, 8000]

    var body: some View {
        let a = values(material.absorption), s = values(material.scattering)
        VStack(alignment: .leading, spacing: .wd.s4) {
            BandBars(bands: Self.bands, alpha: a, scattering: s).frame(height: 64)
            Grid(alignment: .trailing, horizontalSpacing: 0, verticalSpacing: 3) {
                GridRow {
                    Text("Hz").foregroundStyle(Color.wd.textTertiary).gridColumnAlignment(.leading)
                    ForEach(Self.bands, id: \.self) { b in Text(b >= 1000 ? "\(Int(b / 1000))k" : "\(Int(b))").foregroundStyle(Color.wd.textTertiary) }
                }
                GridRow {
                    Text("α").foregroundStyle(Color.wd.absorption).gridColumnAlignment(.leading)
                    ForEach(0..<8, id: \.self) { i in Text(String(format: "%.2f", a[i])).foregroundStyle(Color.wd.textSecondary) }
                }
                GridRow {
                    Text("s").foregroundStyle(Color.wd.scattering).gridColumnAlignment(.leading)
                    ForEach(0..<8, id: \.self) { i in Text(String(format: "%.2f", s[i])).foregroundStyle(Color.wd.textSecondary) }
                }
            }
            .font(.wd.meta)
        }
    }

    func values(_ sp: SpectrumRec) -> [Double] {
        if sp.freqsHz.isEmpty { return Array(repeating: sp.values.first ?? 0, count: 8) }
        return Self.bands.map { f in
            // Log-frequency interpolation, clamped at the ends.
            guard let hi = sp.freqsHz.firstIndex(where: { $0 >= f }) else { return sp.values.last ?? 0 }
            if hi == 0 { return sp.values[0] }
            let (f0, f1) = (sp.freqsHz[hi - 1], sp.freqsHz[hi])
            let t = log(f / f0) / log(f1 / f0)
            return sp.values[hi - 1] + (sp.values[hi] - sp.values[hi - 1]) * t
        }
    }
}

/// Material picker popover: search, list (α @ 500 Hz), preview, Edit / Import.
struct MaterialPicker: View {
    @Bindable var ctx: WindowContext
    let selected: String?
    let onPick: (String) -> Void
    @State private var query = ""
    @State private var hover: String?

    var body: some View {
        let mats = ctx.store.materials.filter { query.isEmpty || $0.name.localizedCaseInsensitiveContains(query) }
        let preview = ctx.store.materialsById[hover ?? selected ?? ""]
        VStack(alignment: .leading, spacing: .wd.s6) {
            TextField("Search materials", text: $query)
                .textFieldStyle(.plain).font(.wd.body)
                .padding(.horizontal, 7).frame(height: .wd.control)
                .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(Color.wd.controlFill))
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(mats, id: \.id) { m in
                        HStack(spacing: .wd.s6) {
                            Circle().fill(Color(hex: m.color) ?? .gray).frame(width: 8, height: 8)
                            Text(m.name).font(.wd.body).foregroundStyle(m.id == selected ? Color.wd.accentTextOnFill : Color.wd.textSecondary).lineLimit(1)
                            Spacer()
                            Text(String(format: "%.2f", MaterialBands(material: m).values(m.absorption)[3])).font(.wd.meta).foregroundStyle(Color.wd.textTertiary)
                        }
                        .padding(.horizontal, .wd.s6)
                        .frame(height: .wd.row)
                        .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(m.id == selected ? Color.wd.selectionRow : hover == m.id ? Color.wd.rowHover : .clear))
                        .contentShape(Rectangle())
                        .onHover { if $0 { hover = m.id } }
                        .onTapGesture { onPick(m.id) }
                    }
                }
            }
            .frame(height: 150)
            if let p = preview { MaterialBands(material: p) }
            HStack {
                Button("Edit…") { ctx.materialLibraryPresented = true }.buttonStyle(.wd)
                Spacer()
            }
        }
        .padding(.wd.s12)
        .frame(width: 320)
        .background(Color.wd.surfacePanel)
    }
}
