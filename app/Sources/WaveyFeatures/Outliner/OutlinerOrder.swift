import AppKit
import WaveyCore
import WaveyModel

extension WindowContext {
    /// The outliner's rows in display order (expanded groups included), for
    /// Shift-click range selection.
    func outlinerOrder() -> [EntityRef] {
        var out: [EntityRef] = []
        let groups = store.groups
        func sources() {
            let speakerGroups = groups.filter { !$0.members.isEmpty || $0.receivers.isEmpty }
            let grouped = Set(speakerGroups.flatMap(\.members))
            for g in speakerGroups {
                out.append(.group(g.id))
                if groupExpanded(g.id) { out += g.members.map { .speaker($0) } }
            }
            out += store.speakers.filter { !grouped.contains($0.id) }.map { .speaker($0.id) }
        }
        func receivers(all: Bool) {
            let micGroups = groups.filter { !$0.receivers.isEmpty && $0.members.isEmpty }
            let grouped = Set(micGroups.flatMap(\.receivers))
            for g in micGroups {
                out.append(.group(g.id))
                if groupExpanded(g.id) { out += g.receivers.map { .microphone($0) } }
            }
            out += store.microphones.filter { (all || $0.includeInSolve) && !grouped.contains($0.id) }.map { .microphone($0.id) }
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
