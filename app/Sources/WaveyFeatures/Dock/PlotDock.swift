import simd
import Accelerate
import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel
import WaveyPlots

/// Magnitude or phase of the visible microphones (or of one).
struct ResponsePlotDock: View {
    @Bindable var ctx: WindowContext
    let phase: Bool
    /// Show only this microphone.
    var only: UInt64?

    var body: some View {
        VStack(spacing: 0) {
            if phase {
                HStack {
                    Spacer()
                    WDCheckbox("Remove delay", isOn: ctx.results.removeDelay) { ctx.results.removeDelay = $0 }
                }
                .padding(.horizontal, .wd.s8).padding(.top, .wd.s4)
            }
            plot
        }
    }

    @ViewBuilder private var plot: some View {
        if ctx.store.runs.isEmpty {
            WDEmptyState("No results", detail: "Solve to see responses at the microphones.").frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            let s = series()
            let range = xRange
            if s.isEmpty {
                WDEmptyState("No microphone data", detail: "Add microphones in the solve, or place one on a solved cut plane.")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                LinePlot(series: s, yAxis: phase ? .phase : .decibels, xRange: range, cursor: ctx.results.frequency,
                         onCursor: { f in ctx.setFrequency(f) },
                         onLegend: { id, solo in toggleLegend(id, solo: solo) })
            }
        }
    }

    private var xRange: ClosedRange<Double> {
        let f = ctx.store.resultInfo?.freqsHz ?? [20, 500]
        let lo = max(f.first ?? 20, 1), hi = max(f.last ?? 500, lo * 2)
        return lo...hi
    }

    private func toggleLegend(_ id: String, solo: Bool) {
        guard let mid = UInt64(id) else { return }
        if solo {
            ctx.results.hiddenMics = Set(ctx.store.microphones.map(\.id)).subtracting([mid])
        } else if ctx.results.hiddenMics.contains(mid) {
            ctx.results.hiddenMics.remove(mid)
        } else {
            ctx.results.hiddenMics.insert(mid)
        }
    }

    func series() -> [PlotSeries] {
        let c = ctx.store.medium.speedOfSound
        var out: [PlotSeries] = []
        for (i, m) in ctx.store.microphones.enumerated() where only.map({ $0 == m.id }) ?? (m.visible && !ctx.results.hiddenMics.contains(m.id)) {
            guard let r = ctx.micResponse(m.id) else { continue }
            let color = m.color.isEmpty ? Color.wd.curve(i) : (Color(hex: m.color) ?? Color.wd.curve(i))
            var y: [Double]
            if phase {
                let d = ctx.results.removeDelay ? nearestSourceDistance(m) : 0
                y = zip(r.freqsHz, r.phaseDeg).map { f, p in
                    var v = p + 360 * f * d / c
                    v = (v + 180).truncatingRemainder(dividingBy: 360)
                    if v < 0 { v += 360 }
                    return v - 180
                }
            } else {
                y = r.splDb
                if ctx.results.smoothing > 0 { y = smooth(r.freqsHz, y, octaveFraction: Double(ctx.results.smoothing)) }
            }
            out.append(PlotSeries(id: "\(m.id)", name: "\(m.code) \(m.name)", color: color, x: r.freqsHz, y: y))
        }
        return out
    }

    private func nearestSourceDistance(_ m: MicrophoneRec) -> Double {
        ctx.store.speakers.map { s in
            let dx = s.position.x - m.position.x, dy = s.position.y - m.position.y, dz = s.position.z - m.position.z
            return (dx * dx + dy * dy + dz * dz).squareRoot()
        }.min() ?? 0
    }
}

/// SPL along the probe line at the current frequency (sampled from the plane stacks).
struct SpatialPlot: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        if let (a, b) = ctx.probeLine, let (xs, ys) = samples(a, b) {
            LinePlot(series: [PlotSeries(id: "probe", name: "PL-1", color: Color.wd.accent, x: xs, y: ys)],
                     xAxis: .linear(unit: "m"), yAxis: .decibels, xRange: 0...max(xs.last ?? 1, 0.01))
        } else {
            WDEmptyState("No probe line", detail: "Draw one with the Probe line tool (P) across a solved cut plane.")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func samples(_ a: SIMD3<Double>, _ b: SIMD3<Double>) -> ([Double], [Double])? {
        let planes = ctx.store.cutPlanes.filter { $0.style.visible && $0.axis == .horizontal }
        guard let plane = planes.min(by: { abs($0.offset - a.z) < abs($1.offset - a.z) }), let f = ctx.fieldValues(plane) else { return nil }
        let s = f.stack
        let n = 100
        var xs: [Double] = [], ys: [Double] = []
        let len = simd_distance(SIMD2(a.x, a.y), SIMD2(b.x, b.y))
        for i in 0...n {
            let t = Double(i) / Double(n)
            let p = a + (b - a) * t
            let iu = Int(((p.x - s.u0) / s.du).rounded()), iv = Int(((p.y - s.v0) / s.dv).rounded())
            guard iu >= 0, iv >= 0, iu < Int(s.nu), iv < Int(s.nv) else { continue }
            let v = f.values[iv * Int(s.nu) + iu]
            guard v.isFinite else { continue }
            xs.append(t * len)
            ys.append(Double(v))
        }
        return xs.isEmpty ? nil : (xs, ys)
    }
}

/// Impulse response of each visible mic: inverse FFT of the complex
/// response. Needs a linear frequency grid starting near 0 Hz.
struct ImpulseDock: View {
    @Bindable var ctx: WindowContext
    var only: UInt64?

    var body: some View {
        if let run = ctx.store.activeRun, run.octaveFraction == 0 {
            let s = series(step: run.linearStepHz)
            if s.isEmpty {
                WDEmptyState("No microphone data").frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                let tmax = s.compactMap(\.x.last).max() ?? 100
                LinePlot(series: s, xAxis: .linear(unit: "ms"), yAxis: .linear(unit: ""), xRange: 0...min(tmax, 1000))
            }
        } else {
            WDEmptyState("Impulse responses need a linear frequency grid",
                         detail: "This run used fractional-octave resolution. Solve with a linear step (Solve settings › Resolution) to get impulse responses.")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func series(step: Double) -> [PlotSeries] {
        var out: [PlotSeries] = []
        for (i, m) in ctx.store.microphones.enumerated() where only.map({ $0 == m.id }) ?? m.visible {
            guard let r = ctx.micResponse(m.id), r.freqsHz.count > 4 else { continue }
            guard let (t, h) = impulse(r, step: step) else { continue }
            let peak = h.map(abs).max() ?? 1
            let color = m.color.isEmpty ? Color.wd.curve(i) : (Color(hex: m.color) ?? Color.wd.curve(i))
            out.append(PlotSeries(id: "\(m.id)", name: "\(m.code) \(m.name)", color: color, x: t, y: h.map { $0 / max(peak, 1e-12) }))
        }
        return out
    }

    /// Real IFFT of the one-sided spectrum on a uniform grid (missing low bins are zero).
    private func impulse(_ r: MicResponseRec, step: Double) -> ([Double], [Double])? {
        let fmax = r.freqsHz.last ?? 0
        var n = 1
        while Double(n) * step < 2 * fmax * 1.0001 { n <<= 1 }
        n = max(n, 64)
        let half = n / 2
        var re = [Double](repeating: 0, count: half), im = [Double](repeating: 0, count: half)
        for (f, (db, ph)) in zip(r.freqsHz, zip(r.splDb, r.phaseDeg)) {
            let k = Int((f / step).rounded())
            guard k > 0, k < half else { continue }
            let mag = pow(10, db / 20) * 2e-5
            re[k] = mag * cos(ph * .pi / 180)
            im[k] = mag * sin(ph * .pi / 180)
        }
        guard let setup = vDSP_create_fftsetupD(vDSP_Length(log2(Double(n))), FFTRadix(kFFTRadix2)) else { return nil }
        defer { vDSP_destroy_fftsetupD(setup) }
        var outRe = re, outIm = im
        outIm[0] = 0
        outRe.withUnsafeMutableBufferPointer { pr in
            outIm.withUnsafeMutableBufferPointer { pi in
                var split = DSPDoubleSplitComplex(realp: pr.baseAddress!, imagp: pi.baseAddress!)
                vDSP_fft_zripD(setup, &split, 1, vDSP_Length(log2(Double(n))), FFTDirection(kFFTDirection_Inverse))
            }
        }
        var h = [Double](repeating: 0, count: n)
        for i in 0..<half {
            h[2 * i] = outRe[i]
            h[2 * i + 1] = outIm[i]
        }
        let fs = Double(n) * step
        let t = (0..<n).map { Double($0) / fs * 1000 }
        let keep = min(n, Int(fs * 1.0))
        return (Array(t.prefix(keep)), Array(h.prefix(keep)))
    }
}

extension WindowContext {
    /// Sets the global results frequency (snapped to the nearest solved one).
    public func setFrequency(_ f: Double) {
        let freqs = store.resultInfo?.freqsHz ?? []
        let snapped = freqs.min { abs(log($0 / f)) < abs(log($1 / f)) } ?? f
        guard snapped != results.frequency else { return }
        results.frequency = snapped
        rebuildScenes()
    }

    /// Steps the frequency by `n` grid points (`,` and `.`).
    func stepFrequency(_ n: Int) {
        guard let freqs = store.resultInfo?.freqsHz, !freqs.isEmpty else { return }
        let i = freqs.indices.min { abs(log(freqs[$0] / results.frequency)) < abs(log(freqs[$1] / results.frequency)) } ?? 0
        results.frequency = freqs[max(0, min(freqs.count - 1, i + n))]
        rebuildScenes()
    }
}
