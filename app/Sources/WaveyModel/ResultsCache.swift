import Foundation
import Observation
@preconcurrency import WaveyCore

/// Field stacks and responses for the active run, fetched off the main
/// actor and cached until the results or the mix change. Scrubbing the
/// frequency reads from these; it never calls the engine.
@MainActor @Observable
public final class ResultsCache {
    public private(set) var stacks: [UInt64: PlaneStack] = [:]
    public private(set) var responses: [UInt64: MicResponseRec] = [:]
    public private(set) var loading: Set<UInt64> = []
    public private(set) var modes: [RoomModeRec] = []
    public private(set) var error: String?

    private let store: DocumentStore
    private var generation = 0
    private var seenRevision = -1

    public init(store: DocumentStore) {
        self.store = store
    }

    /// Drops everything if the results or the speaker mix changed.
    public func sync() {
        guard store.resultsRevision != seenRevision else { return }
        seenRevision = store.resultsRevision
        generation += 1
        stacks = [:]
        responses = [:]
        loading = []
        modes = []
        error = nil
        if store.resultInfo != nil {
            let doc = store.doc
            let gen = generation
            let top = min(store.resultInfo?.freqsHz.last ?? 500, 2000)
            Task.detached(priority: .userInitiated) {
                let m = (try? doc.roomModes(upToHz: top)) ?? []
                await MainActor.run { [weak self] in
                    guard let self, self.generation == gen else { return }
                    self.modes = m
                }
            }
        }
    }

    /// The plane's stack, requesting it if missing (nil until it arrives).
    public func stack(_ plane: UInt64) -> PlaneStack? {
        sync()
        if let s = stacks[plane] { return s }
        guard store.resultInfo?.planeIds.contains(plane) == true, !loading.contains(plane) else { return nil }
        loading.insert(plane)
        let doc = store.doc
        let gen = generation
        Task.detached(priority: .userInitiated) {
            let r = Result { try PlaneStack(doc.planeStack(planeId: plane)) }
            await MainActor.run { [weak self] in
                guard let self, self.generation == gen else { return }
                self.loading.remove(plane)
                switch r {
                case .success(let s): self.stacks[plane] = s
                case .failure(let e): self.error = describe(e)
                }
            }
        }
        return nil
    }

    /// Response at an unsolved mic that lies on a solved plane (within half
    /// a grid step), interpolated from the plane. Nil until it arrives or if
    /// no plane contains the point.
    public func probe(mic: MicrophoneRec, planes: [CutPlaneRec]) -> MicResponseRec? {
        sync()
        let key = mic.id | (1 << 62)
        if let r = responses[key] { return r }
        guard !loading.contains(key) else { return nil }
        let p = mic.position
        guard let plane = planes.first(where: { pl in
            let c = pl.axis == .horizontal ? p.z : pl.axis == .constX ? p.x : p.y
            return abs(c - pl.offset) <= pl.spacing / 2
        }) else { return nil }
        loading.insert(key)
        let doc = store.doc
        let gen = generation
        Task.detached(priority: .userInitiated) {
            let r = try? doc.planeProbe(planeId: plane.id, point: p)
            await MainActor.run { [weak self] in
                guard let self, self.generation == gen else { return }
                self.loading.remove(key)
                if let r = r ?? nil { self.responses[key] = r }
            }
        }
        return nil
    }

    /// A mic's response (mixed with the current speakers), requesting it if missing.
    public func response(_ mic: UInt64) -> MicResponseRec? {
        sync()
        if let r = responses[mic] { return r }
        guard store.resultInfo?.micIds.contains(mic) == true else { return nil }
        let key = mic | (1 << 63)
        guard !loading.contains(key) else { return nil }
        loading.insert(key)
        let doc = store.doc
        let gen = generation
        Task.detached(priority: .userInitiated) {
            let r = try? doc.micResponse(micId: mic, soloSpeaker: nil)
            await MainActor.run { [weak self] in
                guard let self, self.generation == gen else { return }
                self.loading.remove(key)
                if let r { self.responses[mic] = r }
            }
        }
        return nil
    }
}

/// A plane's SPL at every solved frequency (see `WaveyDocument.planeStack`).
public struct PlaneStack: Sendable {
    public let planeId: UInt64
    public let axis: PlaneAxisRec
    public let offset: Double
    public let nu: UInt32
    public let nv: UInt32
    public let u0: Double
    public let v0: Double
    public let du: Double
    public let dv: Double
    public let freqsHz: [Double]
    /// `[freq][v][u]` dB, NaN outside the room.
    public let valuesDb: [Float]
    /// Audience areas: z of each point (`[v][u]`); empty for flat planes.
    public let heights: [Float]

    public init(_ r: PlaneStackRec) {
        planeId = r.planeId
        axis = r.axis
        offset = r.offset
        nu = r.nu
        nv = r.nv
        u0 = r.u0
        v0 = r.v0
        du = r.du
        dv = r.dv
        freqsHz = r.freqsHz
        heights = r.heights
        // One copy: every Mac is little-endian, like the bytes.
        let bytes = r.valuesF32le
        valuesDb = [Float](unsafeUninitializedCapacity: bytes.count / 4) { buf, n in
            n = bytes.copyBytes(to: buf) / 4
        }
    }
}

public extension PlaneStack {
    var cells: Int { Int(nu) * Int(nv) }

    /// Index of the solved frequency nearest to `f` (log scale).
    func nearestIndex(_ f: Double) -> Int {
        guard !freqsHz.isEmpty else { return 0 }
        var best = 0
        var bd = Double.infinity
        for (i, x) in freqsHz.enumerated() {
            let d = abs(log(x) - log(f))
            if d < bd { bd = d; best = i }
        }
        return best
    }

    /// One frequency slice (dB, NaN outside).
    func slice(_ fi: Int) -> ArraySlice<Float> {
        let c = cells
        return valuesDb[(fi * c)..<((fi + 1) * c)]
    }

    /// Mean-square average over a set of frequency indices (dB).
    func average(_ indices: [Int]) -> [Float] {
        let c = cells
        guard !indices.isEmpty else { return [] }
        var out = [Float](repeating: 0, count: c)
        for cell in 0..<c {
            var acc = 0.0
            var nan = false
            for fi in indices {
                let v = valuesDb[fi * c + cell]
                if v.isNaN { nan = true; break }
                acc += pow(10, Double(v) / 10)
            }
            out[cell] = nan ? .nan : Float(10 * log10(acc / Double(indices.count)))
        }
        return out
    }

    /// Maximum over all frequencies (peak hold).
    func peakHold() -> [Float] {
        let c = cells
        var out = [Float](repeating: -.infinity, count: c)
        for fi in 0..<freqsHz.count {
            for cell in 0..<c {
                let v = valuesDb[fi * c + cell]
                out[cell] = v.isNaN ? .nan : max(out[cell], v)
            }
        }
        return out
    }
}
