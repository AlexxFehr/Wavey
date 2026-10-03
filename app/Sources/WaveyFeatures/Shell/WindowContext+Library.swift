import AppKit
import simd
import SwiftUI
import WaveyDesign
import UniformTypeIdentifiers
import WaveyCore
import WaveyModel

/// The stock speaker library: using its devices in the project, adding to it,
/// and where it lives.
extension WindowContext {
    var library: SpeakerLibrary { SpeakerLibrary.shared }

    /// The project's model id for a library file, adding the model to the
    /// project if it is not there yet (an undoable step).
    @discardableResult
    func useLibraryFile(_ path: String) -> String? {
        if let e = library.entriesByPath[path], store.modelsById[e.id] != nil { return e.id }
        var id: String?
        edit("Add Device") { id = try $0.useLibraryModel(path: path) }
        return id
    }

    /// Places a library file's device at the room centre and selects it (`--place`, for checks).
    public func placeLibraryFile(_ path: String) {
        if let id = useLibraryFile(path) { placeDeviceAtCentre(id) }
    }

    /// The library file of a project device (same id), if any.
    func libraryFile(of id: String) -> String? {
        library.entries.first { $0.id == id }?.path
    }

    /// Replaces a project device with its library file's version (e.g. a
    /// newer conversion); placed speakers keep their position and settings.
    func updateFromLibrary(_ id: String) {
        guard let path = libraryFile(of: id) else { post("No library file for this device", .info); return }
        edit("Update Device") { _ = try $0.updateModelFromLibrary(path: path) }
        store.modelReplaced(id)
        balloonCache = [:]
        PreviewCache.shared.forget("model:\(id)")
        post("Updated \(store.modelsById[id]?.name ?? id) from the library", .info)
    }

    /// How a device's cabinet is drawn (auto, detailed or simple).
    func setCabinetStyle(_ id: String, _ style: String) {
        edit("Cabinet Style") { try $0.setCabinetStyle(modelId: id, style: style) }
        PreviewCache.shared.forget("model:\(id)")
    }

    func deviceID(for item: LibrarySection.Item) -> String? {
        item.projectID ?? item.path.flatMap { useLibraryFile($0) }
    }

    func addFilesToLibrary() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "waveyspk") ?? .data]
        panel.allowsMultipleSelection = true
        panel.message = "Copy speaker models into the library (\(library.location.path))"
        guard panel.runModal() == .OK else { return }
        do {
            let n = try library.add(files: panel.urls)
            post("Added \(n) device\(n == 1 ? "" : "s") to the library")
        } catch { post("\(error)", .error) }
    }

    func addProjectModelToLibrary(_ id: String) {
        do {
            try library.add(model: id, from: store.doc, manufacturer: store.modelsById[id]?.manufacturer)
            post("Added \(store.modelsById[id]?.name ?? id) to the library")
        } catch { post("\(error)", .error) }
    }

    func chooseLibraryLocation() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.prompt = "Use as Library"
        panel.message = "Choose the folder of the stock speaker library"
        panel.directoryURL = library.location
        guard panel.runModal() == .OK, let url = panel.url else { return }
        library.setLocation(url)
        post("Library: \(url.path)")
    }
}

/// A library device being dragged: enough to draw its ghost.
public struct DragDevice: Equatable {
    public let name: String
    public let width: Double
    public let height: Double
    public let depth: Double
    public let sub: Bool
}

extension WindowContext {
    /// Starts a library drag (called when the payload is made): remembers
    /// the device for the drop ghost and returns the drag payload.
    func beginDeviceDrag(_ item: LibrarySection.Item) -> String {
        dragDevice = DragDevice(name: item.name, width: item.width, height: item.height, depth: item.depth, sub: item.sub)
        return item.projectID.map { "wavey-device:\($0)" } ?? "wavey-library:\(item.path ?? "")"
    }
}

/// A microphone line (or grid) waiting for its count and height.
public struct MicArrayRequest: Equatable {
    public let grid: Bool
    public let a: SIMD3<Double>
    public let b: SIMD3<Double>
    public let at: CGPoint
}

extension WindowContext {
    /// Adds a line of `count` microphones from `a` to `b` (or a `rows` × `cols`
    /// grid over the rectangle a–b) at height `z`, as one receiver group.
    @discardableResult
    func addMicArray(_ r: MicArrayRequest, count: Int, rows: Int, cols: Int, z: Double) -> UInt64? {
        var pts: [Point3] = []
        if r.grid {
            let (nr, nc) = (max(rows, 1), max(cols, 1))
            for i in 0..<nr {
                for j in 0..<nc {
                    let u = nc == 1 ? 0.5 : Double(j) / Double(nc - 1), v = nr == 1 ? 0.5 : Double(i) / Double(nr - 1)
                    pts.append(Point3(x: r.a.x + (r.b.x - r.a.x) * u, y: r.a.y + (r.b.y - r.a.y) * v, z: z))
                }
            }
        } else {
            let n = max(count, 1)
            for k in 0..<n {
                let t = n == 1 ? 0.5 : Double(k) / Double(n - 1)
                pts.append(Point3(x: r.a.x + (r.b.x - r.a.x) * t, y: r.a.y + (r.b.y - r.a.y) * t, z: z))
            }
        }
        let kind = r.grid ? "Mic grid" : "Mic line"
        let n = store.groups.filter { $0.name.hasPrefix(kind) }.count + 1
        var group: UInt64?
        // Several engine edits, one undo step: each is labelled with the same coalescing key.
        let name = r.grid ? "Add Microphone Grid" : "Add Microphone Line", key = "mic-array-\(UUID().uuidString)"
        edit(name, coalesce: key) { doc in
            var ids: [UInt64] = []
            for (k, p) in pts.enumerated() {
                if k > 0 { doc.labelNextEdit(name: name, coalesceKey: key) }
                let id = doc.addMicrophone(position: p)
                if var m = doc.microphones().first(where: { $0.id == id }) {
                    m.name = "\(kind) \(n) · \(k + 1)"
                    doc.labelNextEdit(name: name, coalesceKey: key)
                    try doc.updateMicrophone(mic: m)
                }
                ids.append(id)
            }
            doc.labelNextEdit(name: name, coalesceKey: key)
            group = doc.addGroupWith(name: "\(kind) \(n)", members: [], receivers: ids)
        }
        if let g = group { select([.group(g)]) }
        return group
    }
}

/// Count and height of a microphone line or grid.
struct MicArrayPopover: View {
    @Bindable var ctx: WindowContext
    let request: MicArrayRequest
    @State private var count = 5.0
    @State private var rows = 3.0
    @State private var cols = 4.0
    @State private var z = 1.2

    var body: some View {
        let len = simd_distance(SIMD2(request.a.x, request.a.y), SIMD2(request.b.x, request.b.y))
        VStack(alignment: .leading, spacing: 0) {
            Text(request.grid ? "Microphone grid" : "Microphone line").font(.wd.title).foregroundStyle(Color.wd.textPrimary)
                .padding(.horizontal, .wd.s12).padding(.top, .wd.s8)
            if request.grid {
                WDPropertyRow("Rows") { WDNumberField(rows, UnitFormatter(.count), range: 1...50, step: 1) { rows = $0.rounded() } }
                WDPropertyRow("Columns") { WDNumberField(cols, UnitFormatter(.count), range: 1...50, step: 1) { cols = $0.rounded() } }
                WDPropertyRow("Size") { WDValueText(String(format: "%.2f × %.2f m", abs(request.b.x - request.a.x), abs(request.b.y - request.a.y))) }
            } else {
                WDPropertyRow("Count") { WDNumberField(count, UnitFormatter(.count), range: 1...100, step: 1) { count = $0.rounded() } }
                WDPropertyRow("Spacing") { WDValueText(count > 1 ? String(format: "%.2f m over %.2f m", len / (count - 1), len) : "—") }
            }
            WDPropertyRow("Height z") { WDNumberField(z, UnitFormatter(.length), range: 0...100) { z = $0 } }
            HStack {
                Button("Cancel") { ctx.micArrayPopover = nil }.buttonStyle(.wd)
                Spacer()
                Button("Create") {
                    ctx.micArrayPopover = nil
                    ctx.addMicArray(request, count: Int(count), rows: Int(rows), cols: Int(cols), z: z)
                }
                .buttonStyle(.wdPrimary)
            }
            .padding(.wd.s12)
        }
        .frame(width: 280)
        .wdPopoverChrome()
    }
}
