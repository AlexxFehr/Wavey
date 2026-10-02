import CoreGraphics
import Foundation
import simd
import WaveyModel
import WaveyViewport

/// Snap result kinds (priority order).
public enum SnapKind: String, Sendable, CaseIterable {
    case endpoint = "END", intersection = "INT", midpoint = "MID", perpendicular = "PERP", nearest = "NEAR", grid = "GRID", centre = "CEN"
}

public struct SnapResult: Sendable {
    public var point: SIMD2<Double>
    public var kind: SnapKind?
}

/// Point snapping on the plan: sketch vertices, midpoints, intersections,
/// perpendicular feet, nearest-on-edge and the grid, then ortho/polar
/// constraints relative to the previous point. Tolerance 10 pt.
@MainActor
struct SnapEngine {
    let store: DocumentStore
    let enabled: Set<SnapKind>
    let ortho: Bool
    let polarDeg: Double?
    let grid: Double
    /// World metres per screen point.
    let mpp: Double

    func snap(_ raw: SIMD2<Double>, from last: SIMD2<Double>?, fine: Bool, off: Bool) -> SnapResult {
        if off { return SnapResult(point: raw, kind: nil) }
        let tol = 10 * mpp
        let segs = store.sketch.segments.map { (SIMD2($0.pa.x, $0.pa.y), SIMD2($0.pb.x, $0.pb.y)) }
        func near(_ p: SIMD2<Double>) -> Bool { simd_distance(p, raw) <= tol }

        if enabled.contains(.endpoint) {
            var best: (SIMD2<Double>, Double)?
            for v in store.sketch.vertices {
                let p = SIMD2(v.p.x, v.p.y)
                let d = simd_distance(p, raw)
                if d <= tol, d < (best?.1 ?? .infinity) { best = (p, d) }
            }
            if let b = best { return SnapResult(point: b.0, kind: .endpoint) }
        }
        if enabled.contains(.intersection) {
            for i in 0..<segs.count {
                for j in (i + 1)..<segs.count {
                    if let x = intersect(segs[i], segs[j]), near(x) { return SnapResult(point: x, kind: .intersection) }
                }
            }
        }
        if enabled.contains(.midpoint) {
            for s in segs {
                let m = (s.0 + s.1) / 2
                if near(m) { return SnapResult(point: m, kind: .midpoint) }
            }
        }
        if enabled.contains(.perpendicular), let last {
            for s in segs {
                let foot = project(last, onto: s, clamp: false)
                if near(foot) { return SnapResult(point: foot, kind: .perpendicular) }
            }
        }
        // Ortho / polar from the previous point.
        if let last, ortho || polarDeg != nil {
            let d = raw - last
            let len = simd_length(d)
            if len > 1e-9 {
                let step = ortho ? 90.0 : (fine ? 1.0 : polarDeg!)
                let a = atan2(d.y, d.x) * 180 / .pi
                let snapped = (a / step).rounded() * step
                if ortho || abs(snapped - a) < (fine ? 0.5 : 3) {
                    let r = snapped * .pi / 180
                    var q = last + SIMD2(cos(r), sin(r)) * len
                    if enabled.contains(.grid) { q = last + SIMD2(cos(r), sin(r)) * (len / grid).rounded() * grid }
                    return SnapResult(point: q, kind: enabled.contains(.grid) ? .grid : nil)
                }
            }
        }
        if enabled.contains(.nearest) {
            for s in segs {
                let p = project(raw, onto: s, clamp: true)
                if simd_distance(p, raw) <= 6 * mpp { return SnapResult(point: p, kind: .nearest) }
            }
        }
        if enabled.contains(.grid) {
            let g = fine ? 0.05 : grid
            return SnapResult(point: (raw / g).rounded(.toNearestOrAwayFromZero) * g, kind: .grid)
        }
        return SnapResult(point: raw, kind: nil)
    }

    func project(_ p: SIMD2<Double>, onto s: (SIMD2<Double>, SIMD2<Double>), clamp: Bool) -> SIMD2<Double> {
        let d = s.1 - s.0
        let l2 = simd_length_squared(d)
        guard l2 > 1e-12 else { return s.0 }
        var t = simd_dot(p - s.0, d) / l2
        if clamp { t = min(max(t, 0), 1) }
        return s.0 + d * t
    }

    func intersect(_ a: (SIMD2<Double>, SIMD2<Double>), _ b: (SIMD2<Double>, SIMD2<Double>)) -> SIMD2<Double>? {
        let r = a.1 - a.0, s = b.1 - b.0
        let den = r.x * s.y - r.y * s.x
        guard abs(den) > 1e-12 else { return nil }
        let q = b.0 - a.0
        let t = (q.x * s.y - q.y * s.x) / den, u = (q.x * r.y - q.y * r.x) / den
        guard t > 1e-6, t < 1 - 1e-6, u > 1e-6, u < 1 - 1e-6 else { return nil }
        return a.0 + r * t
    }
}

/// Command-line point grammar: `x,y`, `x,y,z`, `@dx,dy`, `@dist<angle`,
/// `#x,y`, or a bare distance along the current direction.
enum PointInput {
    case absolute(SIMD2<Double>)
    case relative(SIMD2<Double>)
    case distance(Double)

    static func parse(_ text: String, unit: (String) -> Double?) -> PointInput? {
        var s = text.trimmingCharacters(in: .whitespaces)
        guard !s.isEmpty else { return nil }
        if s.hasPrefix("#") { s.removeFirst(); return xy(s, unit).map { .absolute($0) } }
        if s.hasPrefix("@") {
            s.removeFirst()
            if let lt = s.firstIndex(of: "<") {
                guard let d = unit(String(s[..<lt])), let a = Double(s[s.index(after: lt)...].trimmingCharacters(in: .whitespaces)) else { return nil }
                let r = a * .pi / 180
                return .relative(SIMD2(cos(r), sin(r)) * d)
            }
            return xy(s, unit).map { .relative($0) }
        }
        if s.contains(",") || s.contains(";") { return xy(s, unit).map { .absolute($0) } }
        return unit(s).map { .distance($0) }
    }

    private static func xy(_ s: String, _ unit: (String) -> Double?) -> SIMD2<Double>? {
        let parts = s.split(whereSeparator: { $0 == "," || $0 == ";" }).map(String.init)
        guard parts.count >= 2, let x = unit(parts[0]), let y = unit(parts[1]) else { return nil }
        return SIMD2(x, y)
    }
}
