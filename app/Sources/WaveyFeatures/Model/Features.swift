import simd
import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel
import WaveyViewport

// MARK: Placing features

extension WindowContext {
    /// Room height to size components by (the drawn height, else the room's top).
    var roomTop: Double { store.room?.boundsMax.z ?? store.levels.first { $0.kind == .ceiling }?.z ?? 3 }

    /// Places a component from the tool's clicks: a plan rectangle `a`–`b`
    /// (panel, crowd), a line (beam) or a point (column).
    func placeComponent(_ tool: ToolID, a: SIMD2<Double>, b: SIMD2<Double>) {
        let lo = Point2(x: min(a.x, b.x), y: min(a.y, b.y)), hi = Point2(x: max(a.x, b.x), y: max(a.y, b.y))
        let (w, d) = (hi.x - lo.x, hi.y - lo.y)
        let top = roomTop, base = planZ
        let rect = [Point2(x: lo.x, y: lo.y), Point2(x: hi.x, y: lo.y), Point2(x: hi.x, y: hi.y), Point2(x: lo.x, y: hi.y)]
        let kind: FeatureKindRec
        switch tool {
        case .panel:
            guard w > 0.1, d > 0.1 else { post("Draw the panel as a rectangle", .info); return }
            let z = base > 0 ? base : max(top - 1.0, 0.5)
            kind = .panel(plane: WorkPlaneRec(origin: Point3(x: 0, y: 0, z: z), u: Point3(x: 1, y: 0, z: 0), v: Point3(x: 0, y: 1, z: 0)), outline: rect)
        case .ground:
            guard w > 1, d > 1 else { post("Draw the ground as a rectangle", .info); return }
            kind = .ground(outline: rect)
        case .crowd:
            guard w > 0.5, d > 0.5 else { post("Draw the crowd as a rectangle on the floor", .info); return }
            kind = .crowd(outline: rect, density: 2)
        case .beam:
            guard simd_distance(a, b) > 0.2 else { post("Draw the beam from end to end", .info); return }
            kind = .beam(a: Point2(x: a.x, y: a.y), b: Point2(x: b.x, y: b.y), width: 0.3, bottom: top - 0.6, depth: 1.0)
        case .column:
            kind = .column(centre: Point2(x: a.x, y: a.y), size: Point2(x: 0.5, y: 0.5), round: true, bottom: base - 0.1, top: top + 0.1)
        default:
            return
        }
        var id: UInt64 = 0
        edit("Add \(tool.title)") { id = try $0.addFeature(kind: kind, name: nil) }
        if id != 0 {
            select([.feature(id)])
            if let f = store.featuresById[id], let e = f.error { post(e, .warning) }
        }
    }

    /// Split surface: a 1 × 1 m patch of the surface around the click, with
    /// its own material (size and position in the inspector).
    func addImprint(on key: String, at w: Vec3) {
        guard let s = store.surfacesByKey[key] else { return }
        let n = SIMD3(s.normal.x, s.normal.y, s.normal.z)
        var u = simd_cross(SIMD3(0, 0, 1), n)
        if simd_length(u) < 1e-6 { u = SIMD3(1, 0, 0) }
        u = simd_normalize(u)
        let v = simd_normalize(simd_cross(n, u))
        let plane = WorkPlaneRec(origin: Point3(x: w.x, y: w.y, z: w.z), u: Point3(x: u.x, y: u.y, z: u.z), v: Point3(x: v.x, y: v.y, z: v.z))
        let sq = [Point2(x: -0.5, y: -0.5), Point2(x: 0.5, y: -0.5), Point2(x: 0.5, y: 0.5), Point2(x: -0.5, y: 0.5)]
        var id: UInt64 = 0
        edit("Split Surface") { id = try $0.addFeature(kind: .imprint(target: key, plane: plane, outline: sq), name: "Patch on \(s.code)") }
        if id != 0 { select([.feature(id)]); post("Patch added: give it a material and size in the inspector", .info) }
    }

    /// Plan footprint of a feature (segments at height `z`), for drawing and picking.
    func featureFootprint(_ f: FeatureRec, z: Float) -> [Float] {
        func ring(_ p: [Point2]) -> [Float] {
            var out: [Float] = []
            for i in 0..<p.count {
                let a = p[i], b = p[(i + 1) % p.count]
                out += [Float(a.x), Float(a.y), z, Float(b.x), Float(b.y), z]
            }
            return out
        }
        func rect(_ lo: Point2, _ hi: Point2) -> [Float] {
            ring([lo, Point2(x: hi.x, y: lo.y), hi, Point2(x: lo.x, y: hi.y)])
        }
        func circle(_ c: Point2, _ r: Double) -> [Float] {
            ring((0..<32).map { k in let t = Double(k) / 32 * 2 * .pi; return Point2(x: c.x + r * cos(t), y: c.y + r * sin(t)) })
        }
        switch f.kind {
        case .rake(let lo, let hi, _, _, _, _, _, _), .vault(let lo, let hi, _, _, _, _): return rect(lo, hi)
        case .balcony(let outline, _, _, _), .crowd(let outline, _), .ground(let outline): return ring(outline)
        case .dome(let c, let r, _, _, _): return circle(c, r)
        case .column(let c, let size, let round, _, _):
            return round ? circle(c, size.x / 2) : rect(Point2(x: c.x - size.x / 2, y: c.y - size.y / 2), Point2(x: c.x + size.x / 2, y: c.y + size.y / 2))
        case .beam(let a, let b, let width, _, _):
            let d = simd_normalize(SIMD2(b.x - a.x, b.y - a.y)), n = SIMD2(-d.y, d.x) * width / 2
            return ring([Point2(x: a.x - n.x, y: a.y - n.y), Point2(x: b.x - n.x, y: b.y - n.y), Point2(x: b.x + n.x, y: b.y + n.y), Point2(x: a.x + n.x, y: a.y + n.y)])
        case .extrude(let plane, let outline, _, _, _, _, _), .panel(let plane, let outline):
            // Plan planes only.
            guard abs(plane.u.z) < 1e-9, abs(plane.v.z) < 1e-9 else { return [] }
            return ring(outline.map { p in Point2(x: plane.origin.x + plane.u.x * p.x + plane.v.x * p.y, y: plane.origin.y + plane.u.y * p.x + plane.v.y * p.y) })
        case .revolve, .body, .imprint: return []
        }
    }
}

// MARK: Outliner

/// Geometry: the modelling features in evaluation order (each adds or
/// takes air from what is above it).
struct FeaturesSection: View {
    @Bindable var ctx: WindowContext
    let collapsed: Bool
    let toggle: () -> Void

    var body: some View {
        let fs = ctx.store.features
        WDSectionHeader("Features", collapsed: collapsed, onToggle: toggle) {
            WDCount(fs.count)
            Menu {
                ForEach(ToolID.components, id: \.self) { t in Button(t.title) { ctx.useTool(t) } }
            } label: { WDIcon(.plus, size: 12).foregroundStyle(Color.wd.textTertiary) }
                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize().help("Add a room component")
        }
        if !collapsed {
            if fs.isEmpty {
                Text("Column, beam, panel: add with the Components tool or +. Split a surface to give a part of it its own material. For a more complex room, import it from CAD.")
                    .font(.wd.detail).foregroundStyle(Color.wd.textTertiary).padding(.horizontal, .wd.s12).padding(.vertical, .wd.s4)
                    .fixedSize(horizontal: false, vertical: true)
            }
            ForEach(Array(fs.enumerated()), id: \.element.id) { i, f in
                OutlinerRow(ctx: ctx, entity: .feature(f.id), icon: f.subtract ? .minus : .plus,
                            iconColor: f.error != nil ? Color.wd.statusError : (f.subtract ? Color.wd.textTertiary : Color.wd.accent),
                            title: f.name, meta: f.label, dimmed: f.suppressed, warning: f.error,
                            visible: !f.suppressed,
                            onVisible: { update(f, "Suppress Feature") { $0.suppressed.toggle() } },
                            onRename: { n in update(f, "Rename Feature") { $0.name = n } })
                    .contextMenu {
                        Button(f.subtract ? "Add air instead" : "Take air instead") { update(f, "Change Feature") { $0.subtract.toggle() } }
                        Button("Move up") { ctx.edit("Reorder Features") { $0.moveFeature(id: f.id, index: UInt32(max(i - 1, 0))) } }.disabled(i == 0)
                        Button("Move down") { ctx.edit("Reorder Features") { $0.moveFeature(id: f.id, index: UInt32(i + 1)) } }.disabled(i == fs.count - 1)
                        Divider()
                        Button("Delete") { ctx.edit("Delete Feature") { $0.removeFeatures(ids: [f.id]) } }
                    }
            }
        }
    }

    private func update(_ f: FeatureRec, _ name: String, _ change: (inout FeatureRec) -> Void) {
        var r = f
        change(&r)
        ctx.edit(name) { try $0.updateFeature(feature: r) }
    }
}

// MARK: Inspector

struct FeatureInspector: View {
    @Bindable var ctx: WindowContext
    let id: UInt64

    var body: some View {
        if let f = ctx.store.featuresById[id] {
            InspectorBody {
                InspectorHeader(icon: f.subtract ? .minus : .plus, name: f.name,
                                subtitle: f.error ?? "\(f.label) · \(f.subtract ? "takes air (solid)" : "adds air")",
                                onRename: { n in set("Rename Feature") { $0.name = n } })
                InspectorSection("Feature") {
                    // A crowd or a ground plane is neither solid nor air; a crowd's material follows its density.
                    if f.label != "Crowd" && f.label != "Ground" {
                        WDPropertyRow("Air") {
                            WDSegmented([(false, "Add"), (true, "Take away")], selection: f.subtract) { v in set("Change Feature") { $0.subtract = v } }
                        }
                    }
                    if f.label != "Crowd" {
                        WDPropertyRow(f.label == "Ground" ? "Ground" : "Material") {
                            WDDropdown([WDDropdownItem("", "Default")] + ctx.store.materials.map { WDDropdownItem($0.id, $0.name, swatch: Color(hex: $0.color)) },
                                       selection: f.materialId ?? "") { m in set("Set Feature Material") { $0.materialId = m.isEmpty ? nil : m } }
                        }
                    }
                    WDPropertyRow("Active") {
                        WDCheckbox("Suppressed", isOn: f.suppressed) { v in set("Suppress Feature") { $0.suppressed = v } }
                    }
                }
                InspectorSection(f.label) { parameters(f) }
            } footer: {
                Button("Delete") { ctx.edit("Delete Feature") { $0.removeFeatures(ids: [f.id]) } }.buttonStyle(.wd).frame(maxWidth: .infinity)
            }
        }
    }

    private func set(_ name: String, coalesce: String? = nil, _ change: (inout FeatureRec) -> Void) {
        guard var r = ctx.store.featuresById[id] else { return }
        change(&r)
        ctx.edit(name, coalesce: coalesce.map { "\($0)-\(id)" }) { try $0.updateFeature(feature: r) }
    }

    private func kind(_ k: FeatureKindRec) { set("Edit \(ctx.store.featuresById[id]?.label ?? "Feature")", coalesce: "kind") { $0.kind = k } }

    private func len(_ label: String, _ v: Double, range: ClosedRange<Double>? = nil, _ commit: @escaping (Double) -> Void) -> some View {
        WDPropertyRow(label) { WDNumberField(v, UnitFormatter(.length), range: range, onCommit: commit) }
    }

    private func pair(_ label: String, _ a: Double, _ b: Double, _ ca: @escaping (Double) -> Void, _ cb: @escaping (Double) -> Void) -> some View {
        WDPropertyRow(label) {
            HStack(spacing: .wd.s4) {
                WDNumberField(a, UnitFormatter(.length), onCommit: ca)
                WDNumberField(b, UnitFormatter(.length), onCommit: cb)
            }
        }
    }

    private func count(_ label: String, _ v: UInt32, range: ClosedRange<Double>, _ commit: @escaping (UInt32) -> Void) -> some View {
        WDPropertyRow(label) { WDNumberField(Double(v), UnitFormatter(.count), range: range) { commit(UInt32($0.rounded())) } }
    }

    @ViewBuilder private func parameters(_ f: FeatureRec) -> some View {
        switch f.kind {
        case let .rake(lo, hi, alongX, reverse, base, rows, riser, stepped):
            let len = alongX ? hi.x - lo.x : hi.y - lo.y
            pair("X from · to", lo.x, hi.x, { v in kind(.rake(min: Point2(x: v, y: lo.y), max: hi, alongX: alongX, reverse: reverse, base: base, rows: rows, riser: riser, stepped: stepped)) },
                 { v in kind(.rake(min: lo, max: Point2(x: v, y: hi.y), alongX: alongX, reverse: reverse, base: base, rows: rows, riser: riser, stepped: stepped)) })
            pair("Y from · to", lo.y, hi.y, { v in kind(.rake(min: Point2(x: lo.x, y: v), max: hi, alongX: alongX, reverse: reverse, base: base, rows: rows, riser: riser, stepped: stepped)) },
                 { v in kind(.rake(min: lo, max: Point2(x: hi.x, y: v), alongX: alongX, reverse: reverse, base: base, rows: rows, riser: riser, stepped: stepped)) })
            let dirs: [(Int, String)] = [(0, "+X"), (1, "−X"), (2, "+Y"), (3, "−Y")]
            let dir: Int = (alongX ? 0 : 2) + (reverse ? 1 : 0)
            WDPropertyRow("Rises towards") {
                WDSegmented(dirs, selection: dir) { (s: Int) in
                    kind(.rake(min: lo, max: hi, alongX: s < 2, reverse: s % 2 == 1, base: base, rows: rows, riser: riser, stepped: stepped))
                }
            }
            self.len("First row at", base) { v in kind(.rake(min: lo, max: hi, alongX: alongX, reverse: reverse, base: v, rows: rows, riser: riser, stepped: stepped)) }
            count("Rows", rows, range: 1...200) { v in kind(.rake(min: lo, max: hi, alongX: alongX, reverse: reverse, base: base, rows: v, riser: riser, stepped: stepped)) }
            self.len("Riser", riser, range: 0...2) { v in kind(.rake(min: lo, max: hi, alongX: alongX, reverse: reverse, base: base, rows: rows, riser: v, stepped: stepped)) }
            WDPropertyRow("Shape") {
                WDSegmented([(true, "Steps"), (false, "Slope")], selection: stepped) { v in
                    kind(.rake(min: lo, max: hi, alongX: alongX, reverse: reverse, base: base, rows: rows, riser: riser, stepped: v))
                }
            }
            note(String(format: "Rows %.2f m deep · top row %.2f m up", len / Double(max(rows, 1)), riser * Double(rows)))
        case let .balcony(outline, underside, thickness, parapet):
            self.len("Underside", underside) { v in kind(.balcony(outline: outline, underside: v, thickness: thickness, parapet: parapet)) }
            self.len("Slab", thickness, range: 0.05...3) { v in kind(.balcony(outline: outline, underside: underside, thickness: v, parapet: parapet)) }
            self.len("Parapet", parapet, range: 0...3) { v in kind(.balcony(outline: outline, underside: underside, thickness: thickness, parapet: v)) }
            note("Front: the first edge of its outline. Add a Rake on top for tiered balcony seating.")
        case let .vault(lo, hi, alongX, spring, rise, segments):
            pair("X from · to", lo.x, hi.x, { v in kind(.vault(min: Point2(x: v, y: lo.y), max: hi, alongX: alongX, spring: spring, rise: rise, segments: segments)) },
                 { v in kind(.vault(min: lo, max: Point2(x: v, y: hi.y), alongX: alongX, spring: spring, rise: rise, segments: segments)) })
            pair("Y from · to", lo.y, hi.y, { v in kind(.vault(min: Point2(x: lo.x, y: v), max: hi, alongX: alongX, spring: spring, rise: rise, segments: segments)) },
                 { v in kind(.vault(min: lo, max: Point2(x: hi.x, y: v), alongX: alongX, spring: spring, rise: rise, segments: segments)) })
            WDPropertyRow("Runs along") {
                WDSegmented([(true, "X"), (false, "Y")], selection: alongX) { v in kind(.vault(min: lo, max: hi, alongX: v, spring: spring, rise: rise, segments: segments)) }
            }
            self.len("Springs at", spring) { v in kind(.vault(min: lo, max: hi, alongX: alongX, spring: v, rise: rise, segments: segments)) }
            self.len("Rise", rise, range: 0.05...50) { v in kind(.vault(min: lo, max: hi, alongX: alongX, spring: spring, rise: v, segments: segments)) }
            count("Facets", segments, range: 4...96) { v in kind(.vault(min: lo, max: hi, alongX: alongX, spring: spring, rise: rise, segments: v)) }
        case let .dome(c, r, spring, rise, segments):
            pair("Centre X · Y", c.x, c.y, { v in kind(.dome(centre: Point2(x: v, y: c.y), radius: r, spring: spring, rise: rise, segments: segments)) },
                 { v in kind(.dome(centre: Point2(x: c.x, y: v), radius: r, spring: spring, rise: rise, segments: segments)) })
            self.len("Radius", r, range: 0.1...100) { v in kind(.dome(centre: c, radius: v, spring: spring, rise: rise, segments: segments)) }
            self.len("Springs at", spring) { v in kind(.dome(centre: c, radius: r, spring: v, rise: rise, segments: segments)) }
            self.len("Rise", rise, range: 0.05...100) { v in kind(.dome(centre: c, radius: r, spring: spring, rise: v, segments: segments)) }
            count("Facets", segments, range: 8...96) { v in kind(.dome(centre: c, radius: r, spring: spring, rise: rise, segments: v)) }
        case let .column(c, size, round, bottom, top):
            pair("Centre X · Y", c.x, c.y, { v in kind(.column(centre: Point2(x: v, y: c.y), size: size, round: round, bottom: bottom, top: top)) },
                 { v in kind(.column(centre: Point2(x: c.x, y: v), size: size, round: round, bottom: bottom, top: top)) })
            WDPropertyRow("Section") {
                WDSegmented([(true, "Round"), (false, "Square")], selection: round) { v in kind(.column(centre: c, size: size, round: v, bottom: bottom, top: top)) }
            }
            if round {
                self.len("Diameter", size.x, range: 0.02...20) { v in kind(.column(centre: c, size: Point2(x: v, y: v), round: round, bottom: bottom, top: top)) }
            } else {
                pair("Width · depth", size.x, size.y, { v in kind(.column(centre: c, size: Point2(x: v, y: size.y), round: round, bottom: bottom, top: top)) },
                     { v in kind(.column(centre: c, size: Point2(x: size.x, y: v), round: round, bottom: bottom, top: top)) })
            }
            pair("Bottom · top", bottom, top, { v in kind(.column(centre: c, size: size, round: round, bottom: v, top: top)) },
                 { v in kind(.column(centre: c, size: size, round: round, bottom: bottom, top: v)) })
        case let .beam(a, b, width, bottom, depth):
            pair("Start X · Y", a.x, a.y, { v in kind(.beam(a: Point2(x: v, y: a.y), b: b, width: width, bottom: bottom, depth: depth)) },
                 { v in kind(.beam(a: Point2(x: a.x, y: v), b: b, width: width, bottom: bottom, depth: depth)) })
            pair("End X · Y", b.x, b.y, { v in kind(.beam(a: a, b: Point2(x: v, y: b.y), width: width, bottom: bottom, depth: depth)) },
                 { v in kind(.beam(a: a, b: Point2(x: b.x, y: v), width: width, bottom: bottom, depth: depth)) })
            self.len("Width", width, range: 0.02...10) { v in kind(.beam(a: a, b: b, width: v, bottom: bottom, depth: depth)) }
            self.len("Underside", bottom) { v in kind(.beam(a: a, b: b, width: width, bottom: v, depth: depth)) }
            self.len("Depth", depth, range: 0.02...10) { v in kind(.beam(a: a, b: b, width: width, bottom: bottom, depth: v)) }
        case let .extrude(plane, outline, holes, from, to, slope, slopeDir):
            pair("From · to", from, to, { v in kind(.extrude(plane: plane, outline: outline, holes: holes, from: v, to: to, slope: slope, slopeDir: slopeDir)) },
                 { v in kind(.extrude(plane: plane, outline: outline, holes: holes, from: from, to: v, slope: slope, slopeDir: slopeDir)) })
            WDPropertyRow("Top slope") {
                HStack(spacing: .wd.s4) {
                    WDNumberField(slope * 100, UnitFormatter(.percent), range: -500...500) { v in
                        kind(.extrude(plane: plane, outline: outline, holes: holes, from: from, to: to, slope: v / 100, slopeDir: slopeDir))
                    }
                    WDSegmented([(0, "→X"), (1, "→Y")], selection: abs(slopeDir.y) > abs(slopeDir.x) ? 1 : 0) { s in
                        kind(.extrude(plane: plane, outline: outline, holes: holes, from: from, to: to, slope: slope, slopeDir: s == 0 ? Point2(x: 1, y: 0) : Point2(x: 0, y: 1)))
                    }
                }
            }
            note("\(outline.count) corners. A slope tilts the top: a pitched or raked ceiling.")
        case let .panel(plane, outline):
            self.len("Height", plane.origin.z) { v in
                var p = plane
                p.origin = Point3(x: p.origin.x, y: p.origin.y, z: v)
                kind(.panel(plane: p, outline: outline))
            }
            note("Two-sided: a reflector, canopy or thin wall. Give it a material.")
        case let .imprint(target, plane, outline):
            let xs = outline.map(\.x), ys = outline.map(\.y)
            let (w, h) = ((xs.max() ?? 0) - (xs.min() ?? 0), (ys.max() ?? 0) - (ys.min() ?? 0))
            let (cx, cy) = (((xs.max() ?? 0) + (xs.min() ?? 0)) / 2, ((ys.max() ?? 0) + (ys.min() ?? 0)) / 2)
            let rect = { (cx: Double, cy: Double, w: Double, h: Double) in
                [Point2(x: cx - w / 2, y: cy - h / 2), Point2(x: cx + w / 2, y: cy - h / 2), Point2(x: cx + w / 2, y: cy + h / 2), Point2(x: cx - w / 2, y: cy + h / 2)]
            }
            WDPropertyRow("On") { WDValueText(ctx.store.surfacesByKey[target]?.code ?? target) }
            pair("Width · height", w, h, { v in kind(.imprint(target: target, plane: plane, outline: rect(cx, cy, max(v, 0.01), h))) },
                 { v in kind(.imprint(target: target, plane: plane, outline: rect(cx, cy, w, max(v, 0.01)))) })
            pair("Offset across · up", cx, cy, { v in kind(.imprint(target: target, plane: plane, outline: rect(v, cy, w, h))) },
                 { v in kind(.imprint(target: target, plane: plane, outline: rect(cx, v, w, h))) })
            note("A patch with its own material: an absorber, a curtain, a carpet aisle.")
        case let .ground(outline):
            let xs = outline.map(\.x), ys = outline.map(\.y)
            let (x0, x1, y0, y1) = (xs.min() ?? 0, xs.max() ?? 0, ys.min() ?? 0, ys.max() ?? 0)
            let rect = { (x0: Double, x1: Double, y0: Double, y1: Double) in
                [Point2(x: x0, y: y0), Point2(x: x1, y: y0), Point2(x: x1, y: y1), Point2(x: x0, y: y1)]
            }
            pair("X from · to", x0, x1, { v in kind(.ground(outline: rect(v, x1, y0, y1))) }, { v in kind(.ground(outline: rect(x0, v, y0, y1))) })
            pair("Y from · to", y0, y1, { v in kind(.ground(outline: rect(x0, x1, v, y1))) }, { v in kind(.ground(outline: rect(x0, x1, y0, v))) })
            note("An open scene is one ground plane and no walls. It reflects everywhere at floor level with this material (asphalt, grass, …); the rectangle is the part drawn and laid out in. Walls make it a room, with its own floor.")
        case let .crowd(outline, density):
            let xs = outline.map(\.x), ys = outline.map(\.y)
            let (x0, x1, y0, y1) = (xs.min() ?? 0, xs.max() ?? 0, ys.min() ?? 0, ys.max() ?? 0)
            let rect = { (x0: Double, x1: Double, y0: Double, y1: Double) in
                [Point2(x: x0, y: y0), Point2(x: x1, y: y0), Point2(x: x1, y: y1), Point2(x: x0, y: y1)]
            }
            pair("X from · to", x0, x1, { v in kind(.crowd(outline: rect(v, x1, y0, y1), density: density)) }, { v in kind(.crowd(outline: rect(x0, v, y0, y1), density: density)) })
            pair("Y from · to", y0, y1, { v in kind(.crowd(outline: rect(x0, x1, v, y1), density: density)) }, { v in kind(.crowd(outline: rect(x0, x1, y0, v), density: density)) })
            WDPropertyRow("Density") {
                WDSegmented([(0.5, "Sparse"), (1.0, "Medium"), (2.0, "Dense"), (4.0, "Packed")], selection: Self.nearestDensity(density)) { v in
                    kind(.crowd(outline: outline, density: v))
                }
            }
            WDPropertyRow("People per m²") {
                WDNumberField(density, UnitFormatter(.count), range: 0.1...8) { v in kind(.crowd(outline: outline, density: v)) }
            }
            let area = (x1 - x0) * (y1 - y0)
            note(String(format: "About %.0f people on %.0f m². The floor under them absorbs and scatters like a standing audience (about 3 per m² in the library, scaled by density). They are not modelled as obstacles: sound travelling over their heads is not shadowed.", area * density, area))
        case let .revolve(_, outline, _, _, angle, segments):
            note("\(outline.count)-corner profile turned \(Int(angle))° in \(segments) facets.")
        case let .body(faces):
            note("Imported model with \(faces) faces.")
        }
    }

    /// The preset a density is closest to, for the segmented control.
    static func nearestDensity(_ d: Double) -> Double {
        [0.5, 1.0, 2.0, 4.0].min { abs($0 - d) < abs($1 - d) } ?? 2.0
    }

    private func note(_ s: String) -> some View {
        Text(s).font(.wd.detail).foregroundStyle(Color.wd.textTertiary).fixedSize(horizontal: false, vertical: true)
    }
}
