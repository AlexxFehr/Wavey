import AppKit
import Foundation
import UniformTypeIdentifiers
import WaveyCore
import WaveyDesign
import WaveyModel
import WaveyViewport

/// User actions shared by menus, the palette, outliner and inspectors.
extension WindowContext {
    // MARK: Files

    public func openPanel() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = true
        panel.treatsFilePackagesAsDirectories = false
        panel.allowedContentTypes = [UTType(exportedAs: "app.wavey.project"), .folder]
        panel.message = "Choose a .wavey project"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        openRequested?(url)
    }

    public func save() {
        if store.fileURL == nil { saveAs(); return }
        store.save()
        saveUIState()
    }

    public func saveAs() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(store.title).wavey"
        panel.canCreateDirectories = true
        guard panel.runModal() == .OK, var url = panel.url else { return }
        if url.pathExtension != "wavey" { url.appendPathExtension("wavey") }
        store.save(to: url)
        saveUIState()
    }

    // MARK: Selection edits

    func duplicateSelection() {
        let groups = selectedGroups
        if !groups.isEmpty { duplicateGroups(groups); return }
        let spk = selection.speakers.compactMap { store.speakersById[$0] }
        let mics = selection.microphones.compactMap { store.micsById[$0] }
        guard !spk.isEmpty || !mics.isEmpty else { return }
        var new: [EntityRef] = []
        let step = gridStep > 0 ? gridStep : 0.5
        edit(spk.count + mics.count == 1 ? "Duplicate" : "Duplicate \(spk.count + mics.count) Items") { doc in
            for s in spk {
                var copy = try doc.addSpeaker(modelId: s.modelId, position: s.position, onFloor: s.onFloor)
                let id = copy.id, code = copy.code
                copy = s
                copy.id = id
                copy.code = code
                copy.name = s.name + " copy"
                copy.position.x += step
                copy.aim = nil
                try doc.updateSpeaker(speaker: copy)
                new.append(.speaker(id))
            }
            for m in mics {
                let id = doc.addMicrophone(position: Point3(x: m.position.x + step, y: m.position.y, z: m.position.z))
                if var r = doc.microphones().first(where: { $0.id == id }) {
                    r.name = m.name + " copy"
                    r.includeInSolve = m.includeInSolve
                    try doc.updateMicrophone(mic: r)
                }
                new.append(.microphone(id))
            }
        }
        select(new)
    }

    /// Mirrors selected sources about the room's centre line (x = const), as an L/R pair.
    func mirrorSelection() {
        guard let r = store.room else { return }
        let cx = (r.boundsMin.x + r.boundsMax.x) / 2
        let spk = selection.speakers.compactMap { store.speakersById[$0] }
        guard !spk.isEmpty else { post("Select sources to mirror", .info); return }
        var new: [EntityRef] = []
        edit("Mirror Sources") { doc in
            for s in spk {
                var c = try doc.addSpeaker(modelId: s.modelId, position: s.position, onFloor: s.onFloor)
                let id = c.id, code = c.code
                c = s
                c.id = id
                c.code = code
                c.position.x = 2 * cx - s.position.x
                c.yawDeg = 180 - s.yawDeg
                c.name = s.name.hasSuffix(" L") ? String(s.name.dropLast()) + "R" : s.name + " (mirror)"
                c.aim = nil
                try doc.updateSpeaker(speaker: c)
                new.append(.speaker(id))
            }
        }
        select(new)
    }

    func assignMaterial(_ id: String, to keys: [String]) {
        guard !keys.isEmpty else { return }
        edit("Set Material") { try $0.assignMaterial(surfaceKeys: keys, materialId: id) }
    }

    func groupSelection() {
        let members = selection.speakers
        guard !members.isEmpty else { post("Select sources to group", .info); return }
        var id: UInt64 = 0
        edit("Group") { id = $0.addGroup(name: "Group \(store.groups.count + 1)", members: members) }
        if id != 0 { select([.group(id)]) }
    }

    func moveSelection(toLayer layer: String) {
        var segs = selection.segments
        segs += selection.surfaces.compactMap { store.surfacesByKey[$0]?.segmentId }
        guard !segs.isEmpty else { post("Select walls or edges to move to a layer", .info); return }
        edit("Change Layer") { try $0.setSegmentLayer(segmentIds: Array(Set(segs)), layerId: layer) }
    }

    // MARK: Devices

    func importDevice() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "waveyspk") ?? .data]
        panel.allowsMultipleSelection = true
        guard panel.runModal() == .OK else { return }
        for url in panel.urls {
            edit("Import Device") { _ = try $0.importSpeakerFile(path: url.path) }
        }
    }

    func importDeviceTable() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.commaSeparatedText, .plainText, .tabSeparatedText]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let name = url.deletingPathExtension().lastPathComponent
        edit("Import Device") { _ = try $0.importSpeakerTable(path: url.path, name: name, referenceDistanceM: 1, referenceVoltageRms: 2.83) }
    }

    func exportDevice(_ id: String) {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(store.modelsById[id]?.name ?? id).waveyspk"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do { try store.doc.exportSpeakerModel(id: id, path: url.path) } catch { post(describe(error), .error) }
    }

    /// Places a device at the middle of the floor (z 1.35 m, subwoofers on the floor).
    func placeDeviceAtCentre(_ id: String) {
        guard let m = store.modelsById[id] else { return }
        let c = store.room.map { Vec3(($0.boundsMin.x + $0.boundsMax.x) / 2, ($0.boundsMin.y + $0.boundsMax.y) / 2, 0) } ?? .zero
        let sub = (m.nominalMaxHz ?? m.maxHz) <= 250
        var placed: SpeakerRec?
        edit("Add Source") { placed = try $0.addSpeaker(modelId: id, position: Point3(x: c.x, y: c.y, z: sub ? 0.2 : 1.35), onFloor: sub) }
        if let p = placed { select([.speaker(p.id)]) }
    }

    /// Library drag into the viewport: place each device at the floor point under the drop.
    func dropDevices(_ items: [String], at location: CGPoint) -> Bool {
        let ids = items.compactMap { $0.hasPrefix("wavey-device:") ? String($0.dropFirst(13)) : nil }
        guard !ids.isEmpty, let hit = viewport.projection.onPlane(location, z: 0) else { return false }
        var placed: [EntityRef] = []
        for (i, id) in ids.enumerated() {
            guard let m = store.modelsById[id] else { continue }
            let sub = (m.nominalMaxHz ?? m.maxHz) <= 250
            var rec: SpeakerRec?
            let p = Point3(x: hit.x + Double(i) * 0.6, y: hit.y, z: sub ? 0.2 : 1.35)
            edit("Add Source") { rec = try $0.addSpeaker(modelId: id, position: p, onFloor: sub) }
            if let r = rec { placed.append(.speaker(r.id)) }
        }
        if workspace != .sources && !placed.isEmpty { post("Placed \(placed.count) source\(placed.count == 1 ? "" : "s")") }
        select(placed)
        return !placed.isEmpty
    }

    // MARK: Runs

    func renameRun(_ r: RunRec) {
        let alert = NSAlert()
        alert.messageText = "Rename run"
        let field = NSTextField(string: r.name)
        field.frame = NSRect(x: 0, y: 0, width: 240, height: 24)
        alert.accessoryView = field
        alert.addButton(withTitle: "Rename")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        _ = store.doc.renameRun(id: r.id, name: field.stringValue)
        store.refresh()
    }

    func deleteRun(_ r: RunRec) {
        let alert = NSAlert()
        alert.messageText = "Delete \(r.name)?"
        alert.informativeText = "Its results are removed from the document when you save."
        alert.addButton(withTitle: "Cancel")
        alert.addButton(withTitle: "Delete")
        guard alert.runModal() == .alertSecondButtonReturn else { return }
        _ = store.doc.deleteRun(id: r.id)
        store.refresh()
        afterChange()
    }

    // MARK: Export

    func exportViewImage() {
        guard let img = viewport.image(scale: 2) else { return }
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(store.title) \(workspace.title).png"
        panel.allowedContentTypes = [.png]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let rep = NSBitmapImageRep(cgImage: img)
        try? rep.representation(using: .png, properties: [:])?.write(to: url)
    }

    // MARK: UI state

    struct UIStateFile: Codable {
        var version = 1
        var workspace: Workspace
        var perWorkspace: [String: WorkspaceUIState]
        var frequency: Double
        var band: BandMode
        var gridStep: Double
        var leftVisible: Bool
        var inspectorVisible: Bool
        var commandHistory: [String]
    }

    public func saveUIState() {
        guard let url = store.fileURL else { return }
        var per: [String: WorkspaceUIState] = [:]
        for ws in Workspace.allCases {
            var s = ui[ws]!
            s.orbit = viewports[ws]!.orbit
            s.plan = viewports[ws]!.plan
            s.viewMode = viewports[ws]!.mode
            per[ws.rawValue] = s
        }
        let f = UIStateFile(workspace: workspace, perWorkspace: per, frequency: results.frequency, band: results.band, gridStep: gridStep,
                            leftVisible: leftVisible, inspectorVisible: inspectorVisible, commandHistory: Array(commandHistory.suffix(200)))
        if let data = try? JSONEncoder().encode(f) {
            try? data.write(to: url.appendingPathComponent("ui-state.json"), options: .atomic)
        }
    }

    /// Restores layout, cameras and the results frequency; ignores anything missing or invalid.
    public func loadUIState() -> Bool {
        guard let url = store.fileURL, let data = try? Data(contentsOf: url.appendingPathComponent("ui-state.json")),
              let f = try? JSONDecoder().decode(UIStateFile.self, from: data) else { return false }
        for ws in Workspace.allCases {
            guard let s = f.perWorkspace[ws.rawValue] else { continue }
            ui[ws] = s
            viewports[ws]!.mode = s.viewMode
            viewports[ws]!.orbit = s.orbit
            viewports[ws]!.plan = s.plan
        }
        workspace = f.workspace
        results.frequency = f.frequency
        results.band = f.band
        gridStep = f.gridStep
        leftVisible = f.leftVisible
        inspectorVisible = f.inspectorVisible
        commandHistory = f.commandHistory
        rebuildScenes()
        return true
    }
}
