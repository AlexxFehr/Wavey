import Foundation
import Observation

/// Global selection, shared by viewport, outliner, tables and inspector.
/// Order is selection order; `primary` drives the inspector and gizmo.
@MainActor @Observable
public final class SelectionState {
    public private(set) var ids: [EntityRef] = []
    public private(set) var primary: EntityRef?
    /// Entity under the pointer (viewport or outliner), for cross-highlighting.
    public var hover: EntityRef?

    public init() {}

    public var set: Set<EntityRef> { Set(ids) }
    public var isEmpty: Bool { ids.isEmpty }
    public func contains(_ e: EntityRef) -> Bool { ids.contains(e) }

    public func select(_ new: [EntityRef], primary p: EntityRef? = nil) {
        var seen = Set<EntityRef>()
        ids = new.filter { seen.insert($0).inserted }
        primary = p.flatMap { ids.contains($0) ? $0 : nil } ?? ids.last
    }

    /// ⌘-click.
    public func toggle(_ e: EntityRef) {
        if let i = ids.firstIndex(of: e) {
            ids.remove(at: i)
            if primary == e { primary = ids.last }
        } else {
            ids.append(e)
            primary = e
        }
    }

    /// ⇧-click / marquee with ⇧.
    public func extend(_ new: [EntityRef]) {
        for e in new where !ids.contains(e) { ids.append(e) }
        if let last = new.last { primary = last }
    }

    /// ⌥ marquee.
    public func subtract(_ remove: [EntityRef]) {
        ids.removeAll { remove.contains($0) }
        if let p = primary, remove.contains(p) { primary = ids.last }
    }

    public func clear() {
        ids = []
        primary = nil
    }

    /// Drops entities that no longer exist.
    public func prune(_ exists: (EntityRef) -> Bool) {
        let kept = ids.filter(exists)
        if kept != ids {
            ids = kept
            if let p = primary, !exists(p) { primary = ids.last }
        }
        if let h = hover, !exists(h) { hover = nil }
    }

    /// Selected entities of one kind.
    public func of(_ kind: EntityRef.Kind) -> [EntityRef] { ids.filter { $0.kind == kind } }
    public var speakers: [UInt64] { ids.compactMap(\.speakerID) }
    public var microphones: [UInt64] { ids.compactMap(\.microphoneID) }
    public var planes: [UInt64] { ids.compactMap(\.planeID) }
    public var surfaces: [String] { ids.compactMap(\.surfaceKey) }
    public var segments: [UInt64] { ids.compactMap(\.segmentID) }
}
