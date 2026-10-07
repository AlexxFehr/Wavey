import AppKit
import SwiftUI
import UniformTypeIdentifiers
import WaveyCore
import WaveyDesign
import WaveyModel

/// A model file to import (the sheet's item).
public struct ImportRequest: Identifiable {
    public let id = UUID()
    public let url: URL
}

extension WindowContext {
    /// File ▸ Import ▸ Room Model: pick a file, then the import sheet.
    func importRoomModel() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = ["obj", "gltf", "glb", "stl", "dxf"].compactMap { UTType(filenameExtension: $0) }
        panel.message = "Choose a room model: OBJ, glTF/GLB, STL or DXF (SketchUp and Revit can export these)"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        importRequest = ImportRequest(url: url)
    }

    /// Checks the imported room model for gaps and shows them in red.
    func checkRoomModel() {
        guard let r = store.doc.checkRoomModel() else { post("The room is drawn, not imported", .info); return }
        openEdges = r.openEdgePositions
        rebuildScenes()
        if r.watertight {
            post("Room model is closed: no gaps", .info)
        } else {
            post("Room model has \(r.openEdges) open edges (shown in red): sound leaks through gaps", .warning)
        }
    }
}

/// Import a room model: units, axes and origin, material per group, and
/// what the repair found. Replaces the room, or adds the file as underlay.
public struct ImportSheet: View {
    @Bindable var ctx: WindowContext
    let url: URL
    public init(ctx: WindowContext, url: URL) {
        self.ctx = ctx
        self.url = url
    }
    @Environment(\.dismiss) private var dismiss
    @State private var preview: ImportPreviewRec?
    @State private var options: ImportOptionsRec?
    @State private var materials: [String: String] = [:]
    @State private var error: String?
    @State private var loading = true

    static let units: [(Double, String)] = [(0.001, "Millimetres"), (0.01, "Centimetres"), (1, "Metres"), (0.0254, "Inches"), (0.3048, "Feet")]

    public var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: .wd.s8) {
                WDIcon(.cube, size: 16).foregroundStyle(Color.wd.textSecondary)
                VStack(alignment: .leading, spacing: 1) {
                    Text("Import room model").font(.wd.title)
                    Text(url.lastPathComponent).font(.wd.detail).foregroundStyle(Color.wd.textTertiary).lineLimit(1).truncationMode(.middle)
                }
                Spacer()
            }
            .padding(.wd.s12)
            Rectangle().fill(Color.wd.hairline).frame(height: 1)
            content.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            Rectangle().fill(Color.wd.hairline).frame(height: 1)
            HStack {
                Button("Use as underlay") { importUnderlay() }
                    .buttonStyle(.wd).disabled(preview == nil)
                    .help("Add the file's lines (or edges) as greyed linework to trace over")
                Spacer()
                Button("Cancel") { dismiss() }.buttonStyle(.wd).keyboardShortcut(.cancelAction)
                Button(ctx.store.roomModel == nil && !ctx.store.sketch.segments.isEmpty ? "Replace drawn room" : "Import room") { importRoom() }
                    .buttonStyle(.wdPrimary).keyboardShortcut(.defaultAction)
                    .disabled((preview?.faces ?? 0) == 0)
            }
            .padding(.wd.s12)
        }
        .frame(width: 640, height: 560)
        .background(Color.wd.surfacePanel)
        .environment(\.colorScheme, .dark)
        .task { load(nil) }
    }

    @ViewBuilder private var content: some View {
        if let error {
            WDEmptyState("Could not read the file", detail: error).frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if let p = preview, let o = options {
            ScrollView {
                VStack(alignment: .leading, spacing: .wd.s12) {
                    VStack(spacing: .wd.s4) {
                        WDPropertyRow("Units") {
                            WDDropdown(Self.units.map { WDDropdownItem($0.0, $0.1) } + customUnit(o.scale), selection: o.scale) { s in
                                var n = o; n.scale = s; load(n)
                            }
                        }
                        if p.unitsFromFile {
                            Text("Units from the file").font(.wd.meta).foregroundStyle(Color.wd.textTertiary).frame(maxWidth: .infinity, alignment: .trailing)
                        }
                        WDPropertyRow("Up axis") {
                            WDSegmented([(false, "Z up (CAD, SketchUp)"), (true, "Y up (glTF, Blender export)")], selection: o.yUp) { v in
                                var n = o; n.yUp = v; load(n)
                            }
                        }
                        WDPropertyRow("Origin") {
                            WDSegmented([(ImportOrigin.center, "Centre, floor at 0"), (.floorAtZero, "Floor at 0"), (.keep, "As in file")], selection: o.origin) { v in
                                var n = o; n.origin = v; load(n)
                            }
                        }
                    }
                    summary(p)
                    if !p.groups.isEmpty {
                        HStack {
                            Text("Materials").font(.wd.title)
                            Spacer()
                            Text("by group in the file").font(.wd.meta).foregroundStyle(Color.wd.textTertiary)
                        }
                        .padding(.top, .wd.s4)
                        VStack(spacing: .wd.s4) {
                            ForEach(p.groups, id: \.name) { g in
                                HStack(spacing: .wd.s8) {
                                    Text(g.name).font(.wd.body).lineLimit(1).truncationMode(.middle)
                                    Text("\(g.faces) faces").font(.wd.meta).foregroundStyle(Color.wd.textTertiary)
                                    Spacer()
                                    WDDropdown(ctx.store.materials.map { WDDropdownItem($0.id, $0.name, swatch: Color(hex: $0.color)) },
                                               selection: materials[g.name] ?? g.materialId) { materials[g.name] = $0 }
                                        .frame(width: 220)
                                }
                            }
                        }
                    }
                }
                .padding(.wd.s12)
            }
        } else if loading {
            ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func summary(_ p: ImportPreviewRec) -> some View {
        let f = UnitFormatter(.length, decimals: 2)
        let r = p.report
        var fixes: [String] = []
        if r.verticesWelded > 0 { fixes.append("welded \(r.verticesWelded) vertices") }
        if r.tJunctionsFixed > 0 { fixes.append("joined \(r.tJunctionsFixed) edges") }
        if r.holesFilled > 0 { fixes.append("closed \(r.holesFilled) small holes") }
        if r.duplicatesRemoved + r.degenerateRemoved > 0 { fixes.append("removed \(r.duplicatesRemoved + r.degenerateRemoved) empty or duplicate faces") }
        let tooBig = max(p.size.x, p.size.y, p.size.z) > 400
        let tooSmall = max(p.size.x, p.size.y, p.size.z) < 1
        return VStack(alignment: .leading, spacing: .wd.s4) {
            Text("\(f.string(p.size.x)) × \(f.string(p.size.y)) × \(f.string(p.size.z)) m"
                 + (p.volume > 0 ? String(format: " · %.0f m³", p.volume) : "")
                 + " · \(p.faces) faces" + (p.lines > 0 ? " · \(p.lines) lines" : ""))
                .font(.wd.body)
            if tooBig || tooSmall {
                note(.warning, "That is \(tooBig ? "very large" : "very small") for a room: check the units.", Color.wd.statusOutdated)
            }
            if !fixes.isEmpty {
                note(.check, "Repaired: " + fixes.joined(separator: ", ") + ".", Color.wd.textSecondary)
            }
            if p.faces == 0 {
                note(.warning, "The file has lines only: use it as underlay to trace the room.", Color.wd.statusOutdated)
            } else if !r.watertight {
                note(.warning, "\(r.openEdges) open edges remain: sound leaks through the gaps. Close them in the CAD model, or import and check the model (Geometry ▸ Check Room Model).", Color.wd.statusOutdated)
            } else {
                note(.check, "The room is closed.", Color.wd.textSecondary)
            }
        }
        .padding(.wd.s8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: .wd.radiusField).fill(Color.wd.controlFill.opacity(0.5)))
    }

    private func note(_ icon: WDIconName, _ text: String, _ color: Color) -> some View {
        HStack(alignment: .top, spacing: .wd.s6) {
            WDIcon(icon, size: 12).foregroundStyle(color)
            Text(text).font(.wd.detail).foregroundStyle(color).fixedSize(horizontal: false, vertical: true)
        }
    }

    private func customUnit(_ s: Double) -> [WDDropdownItem<Double>] {
        Self.units.contains { $0.0 == s } ? [] : [WDDropdownItem(s, String(format: "%g m per unit", s))]
    }

    private func load(_ o: ImportOptionsRec?) {
        loading = true
        let doc = ctx.store.doc, path = url.path
        Task.detached {
            let r = Result { try doc.previewImport(path: path, options: o) }
            await MainActor.run {
                loading = false
                switch r {
                case .success(let p):
                    preview = p
                    options = p.options
                case .failure(let e):
                    error = describe(e)
                }
            }
        }
    }

    private func groups() -> [ImportGroupRec] {
        (preview?.groups ?? []).map { ImportGroupRec(name: $0.name, faces: $0.faces, materialId: materials[$0.name] ?? $0.materialId) }
    }

    private func importRoom() {
        guard let o = options else { return }
        var report: RepairReportRec?
        let ok = ctx.edit("Import Room Model") { report = try $0.importRoom(path: url.path, options: o, groups: groups()) }
        guard ok else { return }
        ctx.openEdges = report?.openEdgePositions ?? []
        ctx.workspace = .geometry
        ctx.viewport.zoomToFit()
        ctx.post(report?.watertight == false ? "Room model imported with \(report!.openEdges) open edges (shown in red)" : "Room model imported",
                 report?.watertight == false ? .warning : .info)
        dismiss()
    }

    private func importUnderlay() {
        guard let o = options else { return }
        if ctx.edit("Import Underlay", { _ = try $0.importUnderlay(path: url.path, options: o) }) {
            ctx.workspace = .geometry
            ctx.post("Underlay added: trace over it with the drawing tools", .info)
            dismiss()
        }
    }
}
