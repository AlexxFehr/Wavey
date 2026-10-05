import AppKit
import SwiftUI
import WaveyDesign

/// One curve.
public struct PlotSeries: Identifiable, Sendable {
    public let id: String
    public let name: String
    public let color: Color
    public let x: [Double]
    public let y: [Double]
    public let dashed: Bool
    public let opacity: Double

    public init(id: String, name: String, color: Color, x: [Double], y: [Double], dashed: Bool = false, opacity: Double = 1) {
        self.id = id
        self.name = name
        self.color = color
        self.x = x
        self.y = y
        self.dashed = dashed
        self.opacity = opacity
    }
}

public enum PlotYAxis: Sendable, Equatable {
    /// dB, auto-ranged to whole 10 dB around the visible curves (min span 30).
    case decibels
    /// Degrees, −180…180 in 90° steps.
    case phase
    /// Linear, auto.
    case linear(unit: String)
}

public enum PlotXAxis: Sendable, Equatable {
    case logHz
    case linear(unit: String)
}

/// Fractional-octave smoothing (display only) on power, log-x grid.
public func smooth(_ x: [Double], _ y: [Double], octaveFraction n: Double, decibels: Bool = true) -> [Double] {
    guard n > 0, x.count > 2 else { return y }
    let half = pow(2, 1 / (2 * n))
    var out = [Double](repeating: 0, count: y.count)
    var lo = 0, hi = 0
    var acc = 0.0, cnt = 0
    for i in 0..<x.count {
        let a = x[i] / half, b = x[i] * half
        while hi < x.count && x[hi] <= b { acc += decibels ? pow(10, y[hi] / 10) : y[hi]; cnt += 1; hi += 1 }
        while lo < hi && x[lo] < a { acc -= decibels ? pow(10, y[lo] / 10) : y[lo]; cnt -= 1; lo += 1 }
        out[i] = cnt > 0 ? (decibels ? 10 * log10(max(acc / Double(cnt), 1e-30)) : acc / Double(cnt)) : y[i]
    }
    return out
}

/// Line plot with log or linear x, grid, cursor (drag to move), hover
/// readout and a legend card. Drawn with Canvas; curves are decimated to
/// ≤ 2 points per pixel column.
public struct LinePlot: View {
    let series: [PlotSeries]
    let xAxis: PlotXAxis
    let yAxis: PlotYAxis
    let xRange: ClosedRange<Double>
    let cursor: Double?
    let onCursor: ((Double) -> Void)?
    let onLegend: ((String, Bool) -> Void)?
    let ticksTop: [Double]
    let showsLegend: Bool
    @State private var hover: CGPoint?

    public init(series: [PlotSeries], xAxis: PlotXAxis = .logHz, yAxis: PlotYAxis = .decibels, xRange: ClosedRange<Double>,
                cursor: Double? = nil, ticksTop: [Double] = [], showsLegend: Bool = true,
                onCursor: ((Double) -> Void)? = nil, onLegend: ((String, Bool) -> Void)? = nil) {
        self.showsLegend = showsLegend
        self.series = series
        self.xAxis = xAxis
        self.yAxis = yAxis
        self.xRange = xRange
        self.cursor = cursor
        self.onCursor = onCursor
        self.onLegend = onLegend
        self.ticksTop = ticksTop
    }

    let gutterL: CGFloat = 40, gutterT: CGFloat = 10, gutterB: CGFloat = 25, gutterR: CGFloat = 12

    func yRange() -> ClosedRange<Double> {
        switch yAxis {
        case .phase: return -180...180
        case .decibels:
            var lo = Double.infinity, hi = -Double.infinity
            for s in series {
                for (x, y) in zip(s.x, s.y) where y.isFinite && xRange.contains(x) { lo = min(lo, y); hi = max(hi, y) }
            }
            guard lo.isFinite else { return 40...100 }
            // At most 60 dB below the peak, so steep filters do not flatten the plot.
            lo = max(lo, hi - 60)
            var a = floor(lo / 10) * 10, b = ceil(hi / 10) * 10
            if b - a < 30 { let mid = (a + b) / 2; a = (mid / 10).rounded(.down) * 10 - 10; b = a + 30 }
            return a...b
        case .linear:
            var lo = Double.infinity, hi = -Double.infinity
            for s in series { for (x, y) in zip(s.x, s.y) where y.isFinite && xRange.contains(x) { lo = min(lo, y); hi = max(hi, y) } }
            guard lo.isFinite, hi > lo else { return -1...1 }
            let pad = (hi - lo) * 0.05
            return (lo - pad)...(hi + pad)
        }
    }

    func xToUnit(_ x: Double) -> Double {
        switch xAxis {
        case .logHz: return log(x / xRange.lowerBound) / log(xRange.upperBound / xRange.lowerBound)
        case .linear: return (x - xRange.lowerBound) / (xRange.upperBound - xRange.lowerBound)
        }
    }

    func unitToX(_ t: Double) -> Double {
        switch xAxis {
        case .logHz: return xRange.lowerBound * pow(xRange.upperBound / xRange.lowerBound, t)
        case .linear: return xRange.lowerBound + t * (xRange.upperBound - xRange.lowerBound)
        }
    }

    public var body: some View {
        GeometryReader { geo in
            let plot = CGRect(x: gutterL, y: gutterT, width: max(geo.size.width - gutterL - gutterR, 10), height: max(geo.size.height - gutterT - gutterB, 10))
            let yr = yRange()
            ZStack(alignment: .topLeading) {
                Canvas { g, _ in
                    drawGrid(g, plot, yr)
                    for s in series { drawSeries(g, s, plot, yr) }
                    for t in ticksTop where xRange.contains(t) {
                        let x = plot.minX + CGFloat(xToUnit(t)) * plot.width
                        var p = Path(); p.move(to: CGPoint(x: x, y: plot.minY)); p.addLine(to: CGPoint(x: x, y: plot.minY + 5))
                        g.stroke(p, with: .color(Color.wd.textTertiary), lineWidth: 1)
                    }
                }
                cursorLayer(plot, yr)
                hoverLayer(plot, yr)
                if showsLegend { legend.padding(.top, gutterT + 4).padding(.trailing, gutterR + 4).frame(maxWidth: .infinity, alignment: .topTrailing) }
            }
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0).onChanged { g in
                guard let onCursor, plot.width > 0 else { return }
                let t = min(max((g.location.x - plot.minX) / plot.width, 0), 1)
                onCursor(unitToX(Double(t)))
            })
            .onContinuousHover { phase in
                if case .active(let p) = phase { hover = p } else { hover = nil }
            }
        }
    }

    private func drawGrid(_ g: GraphicsContext, _ r: CGRect, _ yr: ClosedRange<Double>) {
        // X
        let majors: [Double]
        var minors: [Double] = []
        switch xAxis {
        case .logHz:
            var m: [Double] = []
            var dec = pow(10, floor(log10(xRange.lowerBound)))
            while dec <= xRange.upperBound * 10 {
                for k in 1...9 {
                    let v = Double(k) * dec
                    guard xRange.contains(v) else { continue }
                    if k == 1 || k == 2 || k == 5 { m.append(v) } else { minors.append(v) }
                }
                dec *= 10
            }
            majors = m
        case .linear:
            let span = xRange.upperBound - xRange.lowerBound
            let step = niceStep(span / 6)
            majors = stride(from: (xRange.lowerBound / step).rounded(.up) * step, through: xRange.upperBound, by: step).map { $0 }
        }
        // One path per grid colour (one stroke each, not one per line).
        var minorPath = Path(), majorPath = Path()
        for v in minors {
            let x = r.minX + CGFloat(xToUnit(v)) * r.width
            minorPath.move(to: CGPoint(x: x, y: r.minY)); minorPath.addLine(to: CGPoint(x: x, y: r.maxY))
        }
        g.stroke(minorPath, with: .color(Color.wd.plotGridMinor), lineWidth: 1)
        for v in majors {
            let x = r.minX + CGFloat(xToUnit(v)) * r.width
            majorPath.move(to: CGPoint(x: x, y: r.minY)); majorPath.addLine(to: CGPoint(x: x, y: r.maxY))
        }
        g.stroke(majorPath, with: .color(Color.wd.plotGridMajor), lineWidth: 1)
        for v in majors {
            let x = r.minX + CGFloat(xToUnit(v)) * r.width
            let label = v >= 1000 ? (v.truncatingRemainder(dividingBy: 1000) == 0 ? "\(Int(v / 1000))k" : String(format: "%.1fk", v / 1000)) : (v == v.rounded() ? "\(Int(v))" : String(format: "%.1f", v))
            g.draw(Text(label).font(.wd.tick).foregroundColor(Color.wd.textTertiary), at: CGPoint(x: x, y: r.maxY + 10))
        }
        let unit: String = switch xAxis { case .logHz: "Hz"; case .linear(let u): u }
        g.draw(Text(unit).font(.wd.tick).foregroundColor(Color.wd.textTertiary), at: CGPoint(x: r.maxX - 6, y: r.maxY + 20))
        // Y
        let ystep: Double = switch yAxis {
        case .phase: 90
        case .decibels: max(10, niceStep((yr.upperBound - yr.lowerBound) / max(Double(r.height) / 22, 2)))
        case .linear: niceStep((yr.upperBound - yr.lowerBound) / 4)
        }
        var v = (yr.lowerBound / ystep).rounded(.up) * ystep
        var yPath = Path()
        var y0 = v
        while y0 <= yr.upperBound + 1e-9 {
            let y = r.maxY - CGFloat((y0 - yr.lowerBound) / (yr.upperBound - yr.lowerBound)) * r.height
            yPath.move(to: CGPoint(x: r.minX, y: y)); yPath.addLine(to: CGPoint(x: r.maxX, y: y))
            y0 += ystep
        }
        g.stroke(yPath, with: .color(Color.wd.plotGridY), lineWidth: 1)
        while v <= yr.upperBound + 1e-9 {
            let y = r.maxY - CGFloat((v - yr.lowerBound) / (yr.upperBound - yr.lowerBound)) * r.height
            let t = UnitFormatter.thinMinus(abs(v) < 1e-9 ? "0" : String(format: ystep < 1 ? "%.2f" : "%.0f", v))
            g.draw(Text(t).font(.wd.tick).foregroundColor(Color.wd.textTertiary), at: CGPoint(x: r.minX - 6, y: y), anchor: .trailing)
            v += ystep
        }
        var axis = Path()
        axis.move(to: CGPoint(x: r.minX, y: r.minY)); axis.addLine(to: CGPoint(x: r.minX, y: r.maxY)); axis.addLine(to: CGPoint(x: r.maxX, y: r.maxY))
        g.stroke(axis, with: .color(Color.wd.hairline), lineWidth: 1)
    }

    private func niceStep(_ raw: Double) -> Double {
        guard raw > 0 else { return 1 }
        let e = pow(10, floor(log10(raw)))
        for m in [1.0, 2, 5, 10] where m * e >= raw { return m * e }
        return 10 * e
    }

    private func drawSeries(_ g: GraphicsContext, _ s: PlotSeries, _ r: CGRect, _ yr: ClosedRange<Double>) {
        let cols = max(Int(r.width), 1)
        // Min/max decimation per pixel column.
        var colMin = [Double](repeating: .infinity, count: cols), colMax = [Double](repeating: -.infinity, count: cols)
        var first = [Double](repeating: .nan, count: cols), last = [Double](repeating: .nan, count: cols)
        for (x, y) in zip(s.x, s.y) where y.isFinite && x >= xRange.lowerBound && x <= xRange.upperBound {
            let c = min(cols - 1, max(0, Int(xToUnit(x) * Double(cols))))
            colMin[c] = min(colMin[c], y); colMax[c] = max(colMax[c], y)
            if first[c].isNaN { first[c] = y }
            last[c] = y
        }
        var path = Path()
        var started = false
        func pt(_ c: Int, _ y: Double) -> CGPoint {
            CGPoint(x: r.minX + (CGFloat(c) + 0.5), y: r.maxY - CGFloat((min(max(y, yr.lowerBound), yr.upperBound) - yr.lowerBound) / (yr.upperBound - yr.lowerBound)) * r.height)
        }
        for c in 0..<cols where colMin[c].isFinite {
            let pts = colMin[c] == colMax[c] ? [first[c]] : [first[c], colMin[c], colMax[c], last[c]]
            for y in pts {
                if started { path.addLine(to: pt(c, y)) } else { path.move(to: pt(c, y)); started = true }
            }
        }
        g.stroke(path, with: .color(s.color.opacity(s.opacity)),
                 style: StrokeStyle(lineWidth: 1.4, lineCap: .round, lineJoin: .round, dash: s.dashed ? [4, 3] : []))
    }

    @ViewBuilder private func cursorLayer(_ r: CGRect, _ yr: ClosedRange<Double>) -> some View {
        if let c = cursor, xRange.contains(c) {
            let x = r.minX + CGFloat(xToUnit(c)) * r.width
            ZStack(alignment: .topLeading) {
                Rectangle().fill(Color.wd.accent).frame(width: 1, height: r.height).position(x: x, y: r.midY)
                Text(c >= 1000 ? String(format: "%.2fk", c / 1000) : String(format: "%.1f", c))
                    .font(.wd.meta)
                    .foregroundStyle(Color.wd.accentTextOnFill)
                    .frame(width: 52, height: 15)
                    .background(RoundedRectangle(cornerRadius: 3).fill(Color.wd.accentFill))
                    .position(x: x, y: r.minY + 7)
                ForEach(series) { s in
                    if let y = value(s, at: c) {
                        let yy = r.maxY - CGFloat((min(max(y, yr.lowerBound), yr.upperBound) - yr.lowerBound) / (yr.upperBound - yr.lowerBound)) * r.height
                        Circle().strokeBorder(s.color, lineWidth: 1).frame(width: 6, height: 6).position(x: x, y: yy)
                    }
                }
            }
            .allowsHitTesting(false)
        }
    }

    @ViewBuilder private func hoverLayer(_ r: CGRect, _ yr: ClosedRange<Double>) -> some View {
        if let h = hover, r.contains(h) {
            let f = unitToX(Double((h.x - r.minX) / r.width))
            ZStack(alignment: .topLeading) {
                Rectangle().fill(Color.wd.textTertiary.opacity(0.5)).frame(width: 1, height: r.height).position(x: h.x, y: r.midY)
                VStack(alignment: .leading, spacing: 2) {
                    Text(xAxisLabel(f)).foregroundStyle(Color.wd.textSecondary)
                    ForEach(series) { s in
                        if let y = value(s, at: f) {
                            HStack(spacing: 4) {
                                Rectangle().fill(s.color).frame(width: 10, height: 2)
                                Text(UnitFormatter.thinMinus(String(format: "%.1f", y))).foregroundStyle(Color.wd.textPrimary)
                            }
                        }
                    }
                }
                .font(.wd.meta)
                .padding(5)
                .wdOverlayCard()
                .fixedSize()
                .position(x: min(h.x + 60, r.maxX - 50), y: max(h.y - 20, r.minY + 30))
            }
            .allowsHitTesting(false)
        }
    }

    private func xAxisLabel(_ f: Double) -> String {
        switch xAxis {
        case .logHz: f >= 1000 ? String(format: "%.2f kHz", f / 1000) : String(format: "%.1f Hz", f)
        case .linear(let u): String(format: "%.2f %@", f, u)
        }
    }

    /// Linear interpolation of a series at x.
    func value(_ s: PlotSeries, at x: Double) -> Double? {
        guard let i = s.x.firstIndex(where: { $0 >= x }) else { return nil }
        if i == 0 { return s.y.first }
        let (x0, x1, y0, y1) = (s.x[i - 1], s.x[i], s.y[i - 1], s.y[i])
        let t = (x - x0) / max(x1 - x0, 1e-12)
        let v = y0 + (y1 - y0) * t
        return v.isFinite ? v : nil
    }

    private var legend: some View {
        VStack(alignment: .leading, spacing: 3) {
            ForEach(series.filter { !$0.dashed }) { s in
                HStack(spacing: 6) {
                    Rectangle().fill(s.color).frame(width: 12, height: 2)
                    Text(s.name).font(.wd.meta).foregroundStyle(Color.wd.textSecondary).lineLimit(1)
                }
                .contentShape(Rectangle())
                .onTapGesture { onLegend?(s.id, NSEvent.modifierFlags.contains(.option)) }
            }
        }
        .padding(6)
        .background(RoundedRectangle(cornerRadius: .wd.radiusSegment).fill(Color.wd.surfaceChrome.opacity(WDMetrics.overlayAlpha)))
        .overlay(RoundedRectangle(cornerRadius: .wd.radiusSegment).strokeBorder(Color.wd.legendEdge, lineWidth: 1))
        .opacity(series.isEmpty ? 0 : 1)
    }
}

/// Polar plot: rings at 0…−24 dB, radial lines every 45°, one curve per frequency.
public struct PolarPlot: View {
    public struct Curve: Identifiable {
        public let id: String
        public let label: String
        public let color: Color
        public let anglesDeg: [Double]
        public let db: [Double]
        public init(id: String, label: String, color: Color, anglesDeg: [Double], db: [Double]) {
            self.id = id
            self.label = label
            self.color = color
            self.anglesDeg = anglesDeg
            self.db = db
        }
    }

    let curves: [Curve]
    let floorDb = -24.0
    @State private var hover: CGPoint?

    public init(curves: [Curve]) { self.curves = curves }

    public var body: some View {
        VStack(spacing: .wd.s6) {
            GeometryReader { geo in
                let r = min(geo.size.width, geo.size.height) / 2 - 16
                let c = CGPoint(x: geo.size.width / 2, y: geo.size.height / 2 + 4)
                ZStack {
                    Canvas { g, _ in
                        for (i, db) in [0.0, -6, -12, -18, -24].enumerated() {
                            let rr = r * CGFloat(1 - db / floorDb)
                            g.stroke(Path(ellipseIn: CGRect(x: c.x - rr, y: c.y - rr, width: 2 * rr, height: 2 * rr)), with: .color(Color.wd.hairline), lineWidth: 1)
                            if i < 4 {
                                g.draw(Text(i == 0 ? "0" : UnitFormatter.thinMinus("\(Int(db))")).font(.wd.ring).foregroundColor(Color.wd.polarRing),
                                       at: CGPoint(x: c.x + 3, y: c.y - rr + 6), anchor: .leading)
                            }
                        }
                        for k in 0..<8 {
                            let a = Double(k) * .pi / 4
                            var p = Path(); p.move(to: c); p.addLine(to: CGPoint(x: c.x + r * CGFloat(sin(a)), y: c.y - r * CGFloat(cos(a))))
                            g.stroke(p, with: .color(Color.wd.hairline), lineWidth: 1)
                        }
                        g.draw(Text("0°").font(.wd.ring).foregroundColor(Color.wd.polarRing), at: CGPoint(x: c.x, y: c.y - r - 8))
                        g.draw(Text(UnitFormatter.thinMinus("-90°")).font(.wd.ring).foregroundColor(Color.wd.polarRing), at: CGPoint(x: c.x - r - 12, y: c.y))
                        g.draw(Text("90°").font(.wd.ring).foregroundColor(Color.wd.polarRing), at: CGPoint(x: c.x + r + 10, y: c.y))
                        for curve in curves {
                            var p = Path()
                            for (i, (a, db)) in zip(curve.anglesDeg, curve.db).enumerated() {
                                let rr = r * CGFloat(max(0, 1 - max(db, floorDb) / floorDb))
                                let ar = a * .pi / 180
                                // 0° up; +angle (left in the model) drawn to the left.
                                let q = CGPoint(x: c.x - rr * CGFloat(sin(ar)), y: c.y - rr * CGFloat(cos(ar)))
                                if i == 0 { p.move(to: q) } else { p.addLine(to: q) }
                            }
                            p.closeSubpath()
                            g.stroke(p, with: .color(curve.color), lineWidth: 1.5)
                        }
                    }
                    if let h = hover {
                        let ang = atan2(-(h.x - c.x), -(h.y - c.y)) * 180 / .pi
                        VStack(alignment: .leading, spacing: 2) {
                            Text(UnitFormatter.thinMinus(String(format: "%.0f°", ang))).foregroundStyle(Color.wd.textSecondary)
                            ForEach(curves) { cv in
                                if let v = value(cv, ang) {
                                    Text(UnitFormatter.thinMinus(String(format: "%@ %.1f dB", cv.label, v))).foregroundStyle(cv.color)
                                }
                            }
                        }
                        .font(.wd.meta).padding(5).wdOverlayCard().fixedSize()
                        .position(x: min(h.x + 50, geo.size.width - 40), y: h.y - 20)
                        .allowsHitTesting(false)
                    }
                }
                .onContinuousHover { if case .active(let p) = $0 { hover = p } else { hover = nil } }
            }
            HStack(spacing: .wd.s12) {
                ForEach(curves) { c in
                    HStack(spacing: 4) {
                        Rectangle().fill(c.color).frame(width: 12, height: 2)
                        Text(c.label).font(.wd.meta).foregroundStyle(Color.wd.textSecondary)
                    }
                }
            }
        }
    }

    private func value(_ c: Curve, _ angle: Double) -> Double? {
        var a = angle
        if a < 0 { a += 360 }
        guard let i = c.anglesDeg.indices.min(by: { abs(c.anglesDeg[$0] - a) < abs(c.anglesDeg[$1] - a) }) else { return nil }
        return c.db[i]
    }
}

/// Absorption (α) and scattering (s) per octave band, 0…1.
public struct BandBars: View {
    let bands: [Double]
    let alpha: [Double]
    let scattering: [Double]

    public init(bands: [Double], alpha: [Double], scattering: [Double]) {
        self.bands = bands
        self.alpha = alpha
        self.scattering = scattering
    }

    public var body: some View {
        Canvas { g, size in
            let n = max(bands.count, 1)
            let w = size.width / CGFloat(n)
            for v in [0.0, 0.5, 1.0] {
                let y = size.height - CGFloat(v) * (size.height - 2)
                var p = Path(); p.move(to: CGPoint(x: 0, y: y)); p.addLine(to: CGPoint(x: size.width, y: y))
                g.stroke(p, with: .color(Color.wd.hairline), lineWidth: 1)
            }
            for i in 0..<n {
                let a = i < alpha.count ? alpha[i] : 0
                let s = i < scattering.count ? scattering[i] : 0
                let x = CGFloat(i) * w + w * 0.15
                let bw = w * 0.5
                let ha = CGFloat(min(max(a, 0), 1)) * (size.height - 2)
                g.fill(Path(CGRect(x: x, y: size.height - ha, width: bw, height: ha)), with: .color(Color.wd.absorption.opacity(0.85)))
                let hs = CGFloat(min(max(s, 0), 1)) * (size.height - 2)
                g.fill(Path(CGRect(x: x + bw + 1, y: size.height - hs, width: 6, height: hs)), with: .color(Color.wd.scattering))
            }
        }
    }
}

/// Small response line for inspectors.
public struct Sparkline: View {
    let x: [Double]
    let y: [Double]
    let color: Color
    let cursor: Double?

    public init(x: [Double], y: [Double], color: Color, cursor: Double? = nil) {
        self.x = x
        self.y = y
        self.color = color
        self.cursor = cursor
    }

    public var body: some View {
        Canvas { g, size in
            let fin = y.filter(\.isFinite)
            guard let lo = fin.min(), let hi = fin.max(), let x0 = x.first, let x1 = x.last, x1 > x0 else { return }
            let span = max(hi - lo, 1)
            func px(_ v: Double) -> CGFloat { CGFloat(log(v / x0) / log(x1 / x0)) * size.width }
            var p = Path()
            for (i, (xv, yv)) in zip(x, y).enumerated() where yv.isFinite {
                let q = CGPoint(x: px(xv), y: size.height - CGFloat((yv - lo) / span) * (size.height - 2) - 1)
                if i == 0 { p.move(to: q) } else { p.addLine(to: q) }
            }
            g.stroke(p, with: .color(color), lineWidth: 1.2)
            if let c = cursor, c > x0, c < x1 {
                var l = Path(); l.move(to: CGPoint(x: px(c), y: 0)); l.addLine(to: CGPoint(x: px(c), y: size.height))
                g.stroke(l, with: .color(Color.wd.accent), lineWidth: 1)
            }
        }
    }
}
