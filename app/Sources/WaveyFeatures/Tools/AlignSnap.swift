import simd
import WaveyCore
import WaveyModel

/// An axis-aligned world box (a cabinet's extent).
struct Box3 {
    var lo: SIMD3<Double>
    var hi: SIMD3<Double>

    var mid: SIMD3<Double> { (lo + hi) / 2 }
    func offset(_ d: SIMD3<Double>) -> Box3 { Box3(lo: lo + d, hi: hi + d) }
    func union(_ b: Box3) -> Box3 { Box3(lo: simd_min(lo, b.lo), hi: simd_max(hi, b.hi)) }

    /// Plan gap to another box (0 when they overlap in plan).
    func planGap(_ b: Box3) -> Double {
        let dx = max(0, max(b.lo.x - hi.x, lo.x - b.hi.x)), dy = max(0, max(b.lo.y - hi.y, lo.y - b.hi.y))
        return (dx * dx + dy * dy).squareRoot()
    }
}

extension SceneBuilder {
    /// World box of a speaker's cabinet (its mesh as placed, else the
    /// model's box around the position).
    static func box(_ spk: Speaker, store: DocumentStore) -> Box3 {
        let p = SIMD3<Double>(spk.position.x, spk.position.y, spk.position.z)
        let r = rotation(spk)
        if let mesh = store.cabinetMeshes[spk.modelId], mesh.positions.count >= 3 {
            var lo = SIMD3<Double>(repeating: .infinity), hi = SIMD3<Double>(repeating: -.infinity)
            for k in 0..<(mesh.positions.count / 3) {
                let w = SIMD3<Double>(r * SIMD3(mesh.positions[k * 3], mesh.positions[k * 3 + 1], mesh.positions[k * 3 + 2])) + p
                lo = simd_min(lo, w)
                hi = simd_max(hi, w)
            }
            return Box3(lo: lo, hi: hi)
        }
        let m = store.modelsById[spk.modelId]
        let h = SIMD3<Double>(m?.depthM ?? 0.4, m?.widthM ?? 0.4, m?.heightM ?? 0.6) / 2
        return Box3(lo: p - h, hi: p + h)
    }

    /// Union box of some speakers.
    static func box(_ ids: some Sequence<UInt64>, store: DocumentStore) -> Box3? {
        ids.compactMap { store.speakersById[$0] }.map { box($0, store: store) }.reduce(nil) { $0?.union($1) ?? $1 }
    }
}

/// Illustrator-style smart snapping of moving cabinets against the others:
/// side faces flush (touching), or edges and centres aligned, per plan axis.
enum AlignSnap {
    struct Result {
        var correction = SIMD2<Double>(0, 0)
        /// Guide segments to draw (world).
        var guides: [(SIMD3<Double>, SIMD3<Double>)] = []
        /// What snapped, for the status bar ("flush", "aligned").
        var labels: [String] = []
    }

    /// Others nearer than `reach` (plan gap) take part; `tol` is the snap
    /// distance in metres (a few pixels at the current zoom).
    static func snap(_ moving: Box3, others: [Box3], tol: Double, reach: Double = 1.5) -> Result {
        var out = Result()
        let near = others.filter { moving.planGap($0) < reach }
        guard !near.isEmpty else { return out }
        for axis in 0..<2 {
            // (distance, correction, target value, other box, flush)
            var best: (Double, Double, Double, Box3, Bool)?
            for o in near {
                let m = [moving.lo[axis], moving.mid[axis], moving.hi[axis]]
                let t = [o.lo[axis], o.mid[axis], o.hi[axis]]
                // Flush: my low face on its high face and vice versa; aligned: like faces and centres.
                let pairs: [(Int, Int, Bool)] = [(0, 2, true), (2, 0, true), (0, 0, false), (2, 2, false), (1, 1, false)]
                for (i, j, flush) in pairs {
                    let c = t[j] - m[i]
                    // Flush contact counts only when the boxes face each other across this axis.
                    if flush {
                        let other = 1 - axis
                        let overlap = min(moving.hi[other], o.hi[other]) - max(moving.lo[other], o.lo[other])
                        if overlap < -tol { continue }
                    }
                    let score = abs(c) - (flush ? tol * 0.25 : 0)
                    if abs(c) <= tol, best == nil || score < best!.0 { best = (score, c, t[j], o, flush) }
                }
            }
            guard let (_, c, v, o, flush) = best else { continue }
            out.correction[axis] = c
            out.labels.append(flush ? "flush" : "aligned")
            // Guide: along the other axis, spanning both boxes, at floor level of the higher bottom.
            let other = 1 - axis
            let snapped = moving.offset(axis == 0 ? SIMD3(c, 0, 0) : SIMD3(0, c, 0))
            let a = min(snapped.lo[other], o.lo[other]) - 0.15, b = max(snapped.hi[other], o.hi[other]) + 0.15
            let z = max(snapped.lo.z, o.lo.z) + 0.01
            var p = SIMD3<Double>(0, 0, z), q = SIMD3<Double>(0, 0, z)
            p[axis] = v; q[axis] = v
            p[other] = a; q[other] = b
            out.guides.append((p, q))
            // A second guide at the top for height cues in 3D.
            let zt = min(snapped.hi.z, o.hi.z)
            out.guides.append((SIMD3(p.x, p.y, zt), SIMD3(q.x, q.y, zt)))
        }
        return out
    }
}
