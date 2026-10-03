import AppKit
import SwiftUI
import WaveyCore
import WaveyModel
import WaveyViewport

/// Stacks and line arrays: editor windows, copy/paste and group moves.
extension WindowContext {
    static let clipType = NSPasteboard.PasteboardType("com.wavey.assembly+json")

    public func newStack() { AssemblyEditorWindow.show(ctx: self, kind: .stack) }
    public func newLineArray() { AssemblyEditorWindow.show(ctx: self, kind: .lineArray) }

    /// Reopens the editor of a stack or array group.
    public func editAssembly(_ group: UInt64) {
        guard let g = store.groupsById[group], !g.kind.isEmpty else { post("Only stacks and line arrays have an editor", .info); return }
        AssemblyEditorWindow.show(ctx: self, kind: g.kind == "stack" ? .stack : .lineArray, group: group)
    }

    /// Keeps the speakers as a plain group.
    public func detachAssembly(_ group: UInt64) {
        edit("Ungroup Array") { try $0.detachAssembly(group: group) }
    }

    /// Groups in the selection, and the stack/array groups of selected members.
    public var selectedGroups: [UInt64] { selection.ids.compactMap(\.groupID) }

    public func copySelection() {
        let groups = selectedGroups, spk = selection.speakers
        guard !groups.isEmpty || !spk.isEmpty else { post("Select sources, stacks or arrays to copy", .info); return }
        do {
            let text = try store.doc.copyClip(groups: groups, speakers: spk)
            let pb = NSPasteboard.general
            pb.clearContents()
            pb.setString(text, forType: Self.clipType)
            post("Copied \(groups.count + spk.count) item\(groups.count + spk.count == 1 ? "" : "s")", .info)
        } catch {
            post("\(error)", .error)
        }
    }

    public func paste() {
        let pb = NSPasteboard.general
        guard let text = pb.string(forType: Self.clipType) else {
            post("Nothing to paste", .info)
            return
        }
        let step = gridStep > 0 ? gridStep : 0.5
        var pasted: PasteRec?
        edit("Paste") { pasted = try $0.pasteClip(text: text, offset: Point3(x: step * 2, y: 0, z: 0)) }
        if let p = pasted { select(p.groups.map { .group($0) } + p.speakers.map { .speaker($0) }) }
    }

    public func duplicateGroups(_ groups: [UInt64]) {
        let step = gridStep > 0 ? gridStep : 0.5
        var new: [UInt64] = []
        edit(groups.count == 1 ? "Duplicate Group" : "Duplicate \(groups.count) Groups") { doc in
            for g in groups { new.append(try doc.duplicateGroup(group: g, offset: Point3(x: step * 4, y: 0, z: 0))) }
        }
        select(new.map { .group($0) })
    }

    /// Moves a group: stacks and arrays as a whole (their recipe follows),
    /// plain groups member by member.
    func moveGroup(_ group: UInt64, by d: Vec3, key: String) {
        guard let g = store.groupsById[group] else { return }
        if !g.kind.isEmpty, let a = store.doc.groupAssembly(group: group) {
            let o = Point3(x: a.origin.x + d.x, y: a.origin.y + d.y, z: a.origin.z + d.z)
            edit(g.kind == "stack" ? "Move Stack" : "Move Line Array", coalesce: key) { try $0.transformAssembly(group: group, origin: o, yawDeg: a.yawDeg) }
        }
    }

    /// Inserts a cardioid stack and an auto-splayed line array through the
    /// editors' own commit path (`--demo-assemblies`, for checks).
    public func insertDemoAssemblies() {
        let s = AssemblyEditorModel(ctx: self, kind: .stack)
        s.setOrigin(Point3(x: 8, y: 7, z: 0))
        s.setYaw(0)
        s.tuneRear()
        s.commit()
        let l = AssemblyEditorModel(ctx: self, kind: .lineArray)
        l.setBoxCount(12)
        l.autoSplay()
        l.commit()
    }

    /// The stack or line-array editor (of the first such group, else a new one) as a view, for snapshots.
    public func assemblyEditorView(kind: String) -> AnyView {
        let group = store.groups.first { $0.kind == (kind == "stack" ? "stack" : "line_array") }?.id
        return AssemblyEditorWindow.snapshotView(ctx: self, kind: kind == "stack" ? .stack : .lineArray, group: group)
    }
}
