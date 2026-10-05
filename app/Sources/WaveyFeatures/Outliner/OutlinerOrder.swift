import AppKit
import WaveyCore
import WaveyModel

extension WindowContext {
    /// Top-level source rows in list order: each group where its first
    /// member is (stacks and arrays without members first), then speakers.
    func outlinerSourceItems(_ groups: [GroupRec]) -> [EntityRef] {
        var out: [EntityRef] = groups.filter(\.members.isEmpty).map { .group($0.id) }
        var placed = Set<UInt64>()
        for s in store.speakers {
            if let g = groups.first(where: { $0.members.contains(s.id) }) {
                if placed.insert(g.id).inserted { out.append(.group(g.id)) }
            } else {
                out.append(.speaker(s.id))
            }
        }
        return out
    }

    /// Top-level microphone rows in list order (a group where its first member is).
    func outlinerMicItems(_ groups: [GroupRec], mics: [Microphone]) -> [EntityRef] {
        var out: [EntityRef] = []
        var placed = Set<UInt64>()
        for m in store.microphones {
            if let g = groups.first(where: { $0.receivers.contains(m.id) }) {
                if placed.insert(g.id).inserted { out.append(.group(g.id)) }
            } else if mics.contains(where: { $0.id == m.id }) {
                out.append(.microphone(m.id))
            }
        }
        return out
    }

    /// The outliner's rows in display order (expanded groups included), for
    /// Shift-click range selection.
    func outlinerOrder() -> [EntityRef] {
        var out: [EntityRef] = []
        let groups = store.groups
        func expand(_ items: [EntityRef]) {
            for e in items {
                out.append(e)
                if case .group(let id) = e, let g = store.groupsById[id], groupExpanded(id) {
                    out += g.members.map { .speaker($0) } + g.receivers.map { .microphone($0) }
                }
            }
        }
        func sources() {
            expand(outlinerSourceItems(groups.filter { !$0.members.isEmpty || $0.receivers.isEmpty }))
        }
        func receivers(all: Bool) {
            expand(outlinerMicItems(groups.filter { !$0.receivers.isEmpty && $0.members.isEmpty },
                                    mics: store.microphones.filter { all || $0.includeInSolve }))
            out += store.cutPlanes.map { .plane($0.id) }
        }
        switch workspace {
        case .geometry:
            out += store.levels.map { .level($0.id) }
            out += store.layers.map { .layer($0.id) }
            out += (store.room?.surfaces ?? []).sorted { $0.code.localizedStandardCompare($1.code) == .orderedAscending }.map { .surface($0.key) }
        case .sources:
            sources()
            receivers(all: false)
        case .results:
            receivers(all: true)
        }
        return out
    }

    /// Shift-click: everything between the anchor (the last plain or ⌘
    /// click) and `e`, in outliner order; without an anchor in the list,
    /// just `e` is added.
    func selectRange(to e: EntityRef) {
        let order = outlinerOrder()
        guard let b = order.firstIndex(of: e) else { selection.extend([e]); return }
        guard let anchor = outlinerAnchor, let a = order.firstIndex(of: anchor) else {
            selection.extend([e])
            outlinerAnchor = e
            return
        }
        let range = Array(order[min(a, b)...max(a, b)])
        selection.select(range, primary: e)
    }
}
