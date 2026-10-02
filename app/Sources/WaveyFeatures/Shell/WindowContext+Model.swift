import AppKit
import Foundation
import simd
import WaveyCore
import WaveyDesign
import WaveyModel
import WaveyViewport

struct ProbeSegment: Equatable {
    var a: Vec3
    var b: Vec3
}

/// One plane's display values at the current frequency/band.
struct FieldValues {
    let stack: PlaneStackRec
    let values: [Float]
    let range: ClosedRange<Float>
    let version: Int
}

extension WindowContext {
    var planZ: Double { store.levels.first { $0.id == planLevel }?.z ?? 0 }

    var snapKinds: Set<SnapKind> {
        var k = Set<SnapKind>()
        if flag("snap.end") { k.insert(.endpoint) }
        if flag("snap.mid") { k.insert(.midpoint) }
        if flag("snap.int") { k.insert(.intersection) }
        if flag("snap.grid") { k.insert(.grid) }
        if flag("snap.perp") { k.insert(.perpendicular) }
        k.insert(.nearest)
        return k
    }

    // MARK: Commands on the selection

    func deleteSelection() {
        let sel = selection.ids
        guard !sel.isEmpty else { return }
        let spk = sel.compactMap(\.speakerID), mics = sel.compactMap(\.microphoneID), planes = sel.compactMap(\.planeID)
        var segs = sel.compactMap(\.segmentID)
        segs += sel.compactMap(\.surfaceKey).compactMap { store.surfacesByKey[$0]?.segmentId }
        let groups = sel.compactMap(\.groupID)
        let n = spk.count + mics.count + planes.count + Set(segs).count + groups.count
        guard n > 0 else { post("Floors and ceilings follow their outline; delete the outline's edges instead", .info); return }
        edit(n == 1 ? "Delete" : "Delete \(n) Items") { doc in
            if !spk.isEmpty { doc.removeSpeakers(ids: spk) }
            if !mics.isEmpty { doc.removeMicrophones(ids: mics) }
            if !planes.isEmpty { doc.removeCutPlanes(ids: planes) }
            if !segs.isEmpty { doc.removeSegments(ids: Array(Set(segs))) }
            if !groups.isEmpty { doc.removeGroups(ids: groups) }
        }
        selection.clear()
        rebuildScenes()
    }

    /// Hides the layers of selected walls, or the selected mics/planes.
    func hideSelection() {
        let mics = selection.microphones.compactMap { store.micsById[$0] }.map { m -> MicrophoneRec in var m = m; m.visible = false; return m }
        let planes = selection.planes.compactMap { store.planesById[$0] }.map { p -> CutPlaneRec in var p = p; p.style.visible = false; return p }
        guard !mics.isEmpty || !planes.isEmpty else { post("Select microphones or cut planes to hide; walls hide with their layer", .info); return }
        edit("Hide") { doc in
            if !mics.isEmpty { try doc.updateMicrophones(mics: mics) }
            for p in planes { try doc.updateCutPlane(plane: p) }
        }
    }

    func commandEcho(_ text: String, level: Int = 0) {
        commandOutput.append((text, level))
        if commandOutput.count > 200 { commandOutput.removeFirst(commandOutput.count - 200) }
    }

    func contextMenu(for e: EntityRef?, at pt: CGPoint) {
        if let e, !selection.contains(e) { select([e]) }
        contextTarget = ContextTarget(entity: e, point: pt)
        showViewportMenu(for: e)
    }

    // MARK: Directivity caches

    /// Frequencies for a device's polar curves: 250 / 1k / 4k, or three
    /// spread over its nominal band when that excludes them (subwoofers).
    func polarFrequencies(model: String) -> [Double] {
        guard let m = store.modelsById[model] else { return [250, 1000, 4000] }
        let lo = m.nominalMinHz ?? m.minHz, hi = m.nominalMaxHz ?? m.maxHz
        if lo <= 250 && hi >= 4000 { return [250, 1000, 4000] }
        let a = max(lo, 20), b = max(hi, a * 2)
        return [a * pow(b / a, 0.2), sqrt(a * b), a * pow(b / a, 0.8)].map { ($0 * 10).rounded() / 10 }
    }

    /// The balloon frequency for a device: the chosen one if it is in the
    /// device's band, else the band's geometric centre.
    func balloonFrequency(model: String) -> Double {
        guard let m = store.modelsById[model] else { return balloonFrequency }
        let lo = m.nominalMinHz ?? m.minHz, hi = m.nominalMaxHz ?? m.maxHz
        return (lo...hi).contains(balloonFrequency) ? balloonFrequency : (lo * hi).squareRoot()
    }

    func balloon(model: String, frequency: Double) -> BalloonMeshRec? {
        let key = "\(model)@\(Int(frequency))"
        if let b = balloonCache[key] { return b }
        guard let b = try? store.doc.speakerBalloon(modelId: model, freqHz: frequency, floorDb: -30) else { return nil }
        if balloonCache.count > 64 { balloonCache = [:] }
        balloonCache[key] = b
        return b
    }

    func polar(model: String, frequency: Double) -> PolarRec? {
        let key = "\(model)@\(Int(frequency))"
        if let p = polarCache[key] { return p }
        guard let p = try? store.doc.speakerPolar(modelId: model, freqHz: frequency) else { return nil }
        if polarCache.count > 128 { polarCache = [:] }
        polarCache[key] = p
        return p
    }

    // MARK: Results values

    /// Indices of the solved frequencies shown for the current frequency and band mode.
    func frequencyIndices(_ freqs: [Double]) -> [Int] {
        guard !freqs.isEmpty else { return [] }
        let f = results.frequency
        let nearest = freqs.indices.min { abs(log(freqs[$0] / f)) < abs(log(freqs[$1] / f)) }!
        switch results.band {
        case .single, .peak: return [nearest]
        case .third, .octave:
            let n = results.band == .third ? 3.0 : 1.0
            let lo = f * pow(2, -1 / (2 * n)), hi = f * pow(2, 1 / (2 * n))
            let idx = freqs.indices.filter { freqs[$0] >= lo && freqs[$0] < hi }
            return idx.isEmpty ? [nearest] : idx
        }
    }

    /// Display values of a plane, cached per (plane, frequency selection, style).
    func fieldValues(_ plane: CutPlaneRec) -> FieldValues? {
        guard let stack = cache.stack(plane.id) else { return nil }
        let idx = frequencyIndices(stack.freqsHz)
        let key = "\(plane.id)|\(results.band.rawValue)|\(idx.first ?? 0)|\(idx.count)|\(store.resultsRevision)"
        let styleKey = "\(plane.style.range)|\(plane.style.rangeMin)|\(plane.style.rangeMax)|\(plane.style.autoSpan)"
        if let v = fieldMemo[key + styleKey] { return v }
        let values: [Float]
        switch results.band {
        case .single: values = Array(stack.slice(idx.first ?? 0))
        case .peak: values = stack.peakHold()
        default: values = stack.average(idx)
        }
        let finite = values.filter { $0.isFinite }
        let hi = finite.max() ?? 100, lo = finite.min() ?? 60
        let range: ClosedRange<Float>
        switch plane.style.range {
        case .auto:
            let top = hi.rounded(.up)
            range = (top - Float(plane.style.autoSpan))...top
        case .fixed:
            range = Float(min(plane.style.rangeMin, plane.style.rangeMax - 1))...Float(plane.style.rangeMax)
        case .symmetric:
            let m = max(abs(lo), abs(hi))
            range = -m...max(m, 1)
        }
        var h = Hasher()
        h.combine(key)
        h.combine(styleKey)
        let v = FieldValues(stack: stack, values: values, range: range, version: h.finalize())
        if fieldMemo.count > 64 { fieldMemo = [:] }
        fieldMemo[key + styleKey] = v
        return v
    }

    /// Level (dB) of a microphone at the current frequency/band, if known.
    func micLevel(_ id: UInt64) -> Double? {
        guard let r = micResponse(id), !r.freqsHz.isEmpty else { return nil }
        let idx = frequencyIndices(r.freqsHz)
        let p = idx.map { pow(10, r.splDb[$0] / 10) }.reduce(0, +) / Double(idx.count)
        return 10 * log10(p)
    }

    /// Response of a mic: solved directly, or probed from a solved plane it lies on.
    func micResponse(_ id: UInt64) -> MicResponseRec? {
        if let r = cache.response(id) { return r }
        guard let m = store.micsById[id], let info = store.resultInfo, !info.micIds.contains(id) else { return nil }
        return cache.probe(mic: m, planes: store.cutPlanes.filter { info.planeIds.contains($0.id) })
    }
}
