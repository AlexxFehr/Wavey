import Foundation
import WaveyCore
import WaveyModel

/// Groups in the outliner tree: expanding, membership by drag, muting.
extension WindowContext {
    func groupExpanded(_ id: UInt64) -> Bool { !state.collapsed.contains("group:\(id)") }

    func toggleGroupExpanded(_ id: UInt64) {
        let k = "group:\(id)"
        if state.collapsed.contains(k) { state.collapsed.remove(k) } else { state.collapsed.insert(k) }
    }

    /// Dragged outliner rows (`wavey-entity:spk:3`, `wavey-entity:mic:4`).
    private func dragged(_ items: [String]) -> (speakers: [UInt64], mics: [UInt64]) {
        var s: [UInt64] = [], m: [UInt64] = []
        for i in items where i.hasPrefix("wavey-entity:") {
            let parts = i.dropFirst(13).split(separator: ":")
            guard parts.count == 2, let id = UInt64(parts[1]) else { continue }
            if parts[0] == "spk" { s.append(id) } else if parts[0] == "mic" { m.append(id) }
        }
        return (s, m)
    }

    /// Adds dragged speakers / receivers to a plain group (leaving their other plain groups).
    func addToGroup(_ group: UInt64, _ items: [String]) -> Bool {
        let (s, m) = dragged(items)
        guard (!s.isEmpty || !m.isEmpty), var g = store.groupsById[group] else { return false }
        guard g.kind.isEmpty else { post("Stacks and line arrays take their speakers from the editor", .info); return false }
        let key = "regroup-\(UUID().uuidString)"
        edit("Add to Group", coalesce: key) { doc in
            for var other in store.groups where other.id != group && other.kind.isEmpty {
                let before = other.members.count + other.receivers.count
                other.members.removeAll { s.contains($0) }
                other.receivers.removeAll { m.contains($0) }
                if other.members.count + other.receivers.count != before {
                    try doc.updateGroup(group: other)
                    doc.labelNextEdit(name: "Add to Group", coalesceKey: key)
                }
            }
            g.members += s.filter { !g.members.contains($0) }
            g.receivers += m.filter { !g.receivers.contains($0) }
            try doc.updateGroup(group: g)
        }
        return true
    }

    /// Dragged rows dropped on a section header leave their plain groups.
    func ungroup(_ items: [String]) -> Bool {
        let (s, m) = dragged(items)
        let affected = store.groups.filter { $0.kind.isEmpty && ($0.members.contains { s.contains($0) } || $0.receivers.contains { m.contains($0) }) }
        guard !affected.isEmpty else { return false }
        let key = "ungroup-\(UUID().uuidString)"
        edit("Remove from Group", coalesce: key) { doc in
            for var g in affected {
                g.members.removeAll { s.contains($0) }
                g.receivers.removeAll { m.contains($0) }
                try doc.updateGroup(group: g)
                doc.labelNextEdit(name: "Remove from Group", coalesceKey: key)
            }
        }
        return true
    }

    func setGroupMuted(_ group: UInt64, _ muted: Bool) {
        guard let g = store.groupsById[group] else { return }
        let recs = g.members.compactMap { store.speakersById[$0] }.map { r -> Speaker in var r = r; r.muted = muted; return r }
        edit(muted ? "Mute Group" : "Unmute Group") { try $0.updateSpeakers(speakers: recs) }
    }
}
