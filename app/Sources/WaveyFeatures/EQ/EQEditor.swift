import AppKit
import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel

/// What an EQ window edits: the filter chain of some speakers, or a group's EQ.
enum EQTarget: Hashable {
    case speakers([UInt64])
    case group(UInt64)
}

/// The EQ of one target, edited live (mix-time: no re-solve).
@MainActor @Observable
final class EQModel {
    unowned let ctx: WindowContext
    let target: EQTarget
    var selected: Int?
    var hovered: Int?
    var showDevice = true
    @ObservationIgnored var dragKey = 0
    /// The band the last click added (a double-click there does not remove it).
    @ObservationIgnored var justAdded: Int?
    @ObservationIgnored private var scrollMonitor: Any?
    @ObservationIgnored private var scrollKey = 0
    @ObservationIgnored private var lastScroll = Date.distantPast
    /// The window the graph is in (scrolls elsewhere are left alone).
    @ObservationIgnored weak var window: NSWindow?

    init(ctx: WindowContext, target: EQTarget) {
        self.ctx = ctx
        self.target = target
    }

    var title: String {
        switch target {
        case .speakers(let ids):
            let s = ids.compactMap { ctx.store.speakersById[$0] }
            return s.count == 1 ? "\(s[0].code) · \(s[0].name)" : "\(s.count) sources"
        case .group(let id):
            return "Group EQ · \(ctx.store.groupsById[id]?.name ?? "")"
        }
    }

    /// The chain shown: the group's, or the first speaker's.
    var filters: [FilterRec] {
        switch target {
        case .speakers(let ids): ids.first.flatMap { ctx.store.speakersById[$0]?.filters } ?? []
        case .group(let id): ctx.store.groupsById[id]?.filters ?? []
        }
    }

    /// Selected speakers with differing chains (an edit gives them all this one).
    var mixed: Bool {
        guard case .speakers(let ids) = target, ids.count > 1 else { return false }
        let chains = ids.compactMap { ctx.store.speakersById[$0]?.filters }
        return Set(chains.map { "\($0)" }).count > 1
    }

    /// Device on-axis response of a single speaker (for the overlay).
    var deviceModel: String? {
        if case .speakers(let ids) = target, let s = ids.first.flatMap({ ctx.store.speakersById[$0] }) { return s.modelId }
        if case .group(let id) = target, let m = ctx.store.groupsById[id]?.members.first { return ctx.store.speakersById[m]?.modelId }
        return nil
    }

    func set(_ f: [FilterRec], name: String = "Change EQ", coalesce: Bool = false) {
        let key = coalesce ? "eq-\(target)-\(dragKey)" : nil
        switch target {
        case .speakers(let ids):
            let recs = ids.compactMap { ctx.store.speakersById[$0] }.map { r -> Speaker in var r = r; r.filters = f; return r }
            ctx.edit(name, coalesce: key) { try $0.updateSpeakers(speakers: recs) }
        case .group(let id):
            guard var g = ctx.store.groupsById[id] else { return }
            g.filters = f
            ctx.edit(name, coalesce: key) { try $0.updateGroup(group: g) }
        }
    }

    func add(_ f: FilterRec) {
        var c = filters
        c.append(f)
        set(c, name: "Add EQ Band")
        selected = c.count - 1
    }

    func remove(_ i: Int) {
        var c = filters
        guard c.indices.contains(i) else { return }
        c.remove(at: i)
        set(c, name: "Remove EQ Band")
        selected = nil
    }

    /// Scrolling over a band changes its Q (peaks, shelves) or slope
    /// (high- and low-pass); one undo step per scroll gesture.
    func installScroll() {
        guard scrollMonitor == nil else { return }
        scrollMonitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { [weak self] e in
            nonisolated(unsafe) let event = e
            let handled = MainActor.assumeIsolated { self?.scroll(event) ?? false }
            return handled ? nil : e
        }
    }

    func removeScroll() {
        if let m = scrollMonitor { NSEvent.removeMonitor(m) }
        scrollMonitor = nil
    }

    private func scroll(_ e: NSEvent) -> Bool {
        guard let i = hovered ?? (dragging ? selected : nil), filters.indices.contains(i), e.window != nil, e.window === window else { return false }
        let dy = e.hasPreciseScrollingDeltas ? e.scrollingDeltaY : e.scrollingDeltaY * 8
        guard dy != 0 else { return true }
        if Date().timeIntervalSince(lastScroll) > 0.6 { scrollKey += 1 }
        lastScroll = Date()
        let b = filters[i]
        switch b {
        case .highPass(let f, let sl), .lowPass(let f, let sl):
            // Slope steps: one notch per scroll tick.
            guard abs(dy) >= 4 || !e.hasPreciseScrollingDeltas else { return true }
            let k = slopeOrder.firstIndex { $0.0 == sl } ?? 2
            let j = min(max(k + (dy > 0 ? 1 : -1), 0), slopeOrder.count - 1)
            let n: FilterRec = b.kind == .highPass ? .highPass(freqHz: f, slope: slopeOrder[j].0) : .lowPass(freqHz: f, slope: slopeOrder[j].0)
            replaceScrolled(i, n)
        default:
            replaceScrolled(i, b.with(q: (b.q ?? 1) * pow(2, Double(dy) / 60)))
        }
        selected = i
        return true
    }

    private func replaceScrolled(_ i: Int, _ f: FilterRec) {
        var c = filters
        c[i] = f
        let key = "eq-scroll-\(target)-\(scrollKey)"
        switch target {
        case .speakers(let ids):
            let recs = ids.compactMap { ctx.store.speakersById[$0] }.map { r -> Speaker in var r = r; r.filters = c; return r }
            ctx.edit("Change EQ", coalesce: key) { try $0.updateSpeakers(speakers: recs) }
        case .group(let id):
            guard var g = ctx.store.groupsById[id] else { return }
            g.filters = c
            ctx.edit("Change EQ", coalesce: key) { try $0.updateGroup(group: g) }
        }
    }

    /// A node is being dragged.
    @ObservationIgnored var dragging = false

    func replace(_ i: Int, _ f: FilterRec, coalesce: Bool = false) {
        var c = filters
        guard c.indices.contains(i) else { return }
        c[i] = f
        set(c, coalesce: coalesce)
    }
}

// MARK: - Band helpers

enum BandKind: String, CaseIterable {
    case highPass = "High-pass", lowShelf = "Low shelf", peak = "Peak", highShelf = "High shelf", lowPass = "Low-pass"
}

extension FilterRec {
    var kind: BandKind {
        switch self {
        case .highPass: .highPass
        case .lowPass: .lowPass
        case .lowShelf: .lowShelf
        case .highShelf: .highShelf
        case .peak: .peak
        }
    }

    var freq: Double {
        switch self {
        case .highPass(let f, _), .lowPass(let f, _), .lowShelf(let f, _, _), .highShelf(let f, _, _), .peak(let f, _, _): f
        }
    }

    /// Gain the node sits at (0 dB for the passes).
    var gain: Double {
        switch self {
        case .lowShelf(_, let g, _), .highShelf(_, let g, _), .peak(_, let g, _): g
        default: 0
        }
    }

    var q: Double? {
        switch self {
        case .peak(_, _, let q): q
        case .lowShelf(_, _, let q), .highShelf(_, _, let q): q ?? 0.707
        default: nil
        }
    }

    func with(freq f: Double? = nil, gain g: Double? = nil, q: Double? = nil) -> FilterRec {
        let f = min(max(f ?? freq, 10), 30000), g = min(max(g ?? gain, -24), 24)
        switch self {
        case .highPass(_, let s): return .highPass(freqHz: f, slope: s)
        case .lowPass(_, let s): return .lowPass(freqHz: f, slope: s)
        case .lowShelf(_, _, let q0): return .lowShelf(freqHz: f, gainDb: g, q: q.map { min(max($0, 0.3), 4) } ?? q0)
        case .highShelf(_, _, let q0): return .highShelf(freqHz: f, gainDb: g, q: q.map { min(max($0, 0.3), 4) } ?? q0)
        case .peak(_, _, let q0): return .peak(freqHz: f, gainDb: g, q: min(max(q ?? q0, 0.1), 20))
        }
    }

    static func make(_ k: BandKind, freq: Double, gain: Double = 0) -> FilterRec {
        switch k {
        case .highPass: .highPass(freqHz: freq, slope: .linkwitzRiley24)
        case .lowPass: .lowPass(freqHz: freq, slope: .linkwitzRiley24)
        case .lowShelf: .lowShelf(freqHz: freq, gainDb: gain, q: nil)
        case .highShelf: .highShelf(freqHz: freq, gainDb: gain, q: nil)
        case .peak: .peak(freqHz: freq, gainDb: gain, q: 1.4)
        }
    }

    func converted(to k: BandKind) -> FilterRec { FilterRec.make(k, freq: freq, gain: gain) }
}

let slopeOrder: [(SlopeRec, String)] = [(.butterworth12, "12 dB/oct BW"), (.butterworth24, "24 dB/oct BW"), (.linkwitzRiley24, "24 dB/oct LR"), (.linkwitzRiley48, "48 dB/oct LR")]

// MARK: - Graph

/// The interactive EQ graph: log frequency, ±24 dB, total curve, band curves, nodes.
struct EQGraph: View {
    @Bindable var model: EQModel
    static let freqs: [Double] = (0...240).map { 20 * pow(1000, Double($0) / 240) }
    static let range = 24.0
    @State private var drag: (index: Int, start: FilterRec, startPoint: CGPoint)?

    var body: some View {
        GeometryReader { geo in
            let size = geo.size
            let f = model.filters
            let curves = filterCurves(filters: f, freqs: Self.freqs)
            Canvas { g, sz in draw(&g, sz, f, curves) }
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 1).onChanged { v in dragChanged(v, size); model.dragging = drag != nil }.onEnded { _ in drag = nil; model.dragging = false })
                .background(WindowReader { model.window = $0 })
                .onTapGesture(count: 1, coordinateSpace: .local) { p in
                    // A click on a node selects it, a double-click removes it;
                    // elsewhere a click adds a band: high-pass on the left,
                    // low-pass on the right, a peak between.
                    let double = (NSApp.currentEvent?.clickCount ?? 1) >= 2
                    if let i = node(near: p, size) {
                        // (Not the band the first click of this double-click just added.)
                        if double, model.justAdded != i { model.remove(i) } else { model.selected = i }
                        model.justAdded = nil
                        return
                    }
                    let f = freq(p.x, size.width)
                    switch Self.zone(p.x, size.width) {
                    case .highPass: model.add(.make(.highPass, freq: f))
                    case .lowPass: model.add(.make(.lowPass, freq: f))
                    default: model.add(.make(.peak, freq: f, gain: gain(p.y, size.height)))
                    }
                    model.justAdded = model.selected
                }
                .onAppear { model.installScroll() }
                .onDisappear { model.removeScroll() }
                .onContinuousHover(coordinateSpace: .local) { phase in
                    if case .active(let p) = phase { model.hovered = node(near: p, size) } else { model.hovered = nil }
                }
                .contextMenu {
                    if let i = model.hovered ?? model.selected, model.filters.indices.contains(i) {
                        let b = model.filters[i]
                        Text("Band \(i + 1) · \(b.kind.rawValue)")
                        ForEach(BandKind.allCases, id: \.self) { k in
                            Button(k.rawValue) { model.replace(i, b.converted(to: k)) }.disabled(k == b.kind)
                        }
                        Divider()
                        Button("Delete Band") { model.remove(i) }
                    }
                }
        }
    }

    /// Click zones: the left and right fifths add passes, the rest peaks.
    static func zone(_ x: CGFloat, _ w: CGFloat) -> BandKind {
        x < w * 0.2 ? .highPass : x > w * 0.8 ? .lowPass : .peak
    }

    // Coordinates
    private func x(_ f: Double, _ w: CGFloat) -> CGFloat { CGFloat(log(f / 20) / log(1000)) * w }
    private func freq(_ x: CGFloat, _ w: CGFloat) -> Double { 20 * pow(1000, Double(x / max(w, 1))) }
    private func y(_ db: Double, _ h: CGFloat) -> CGFloat { h / 2 - CGFloat(db / Self.range) * (h / 2 - 8) }
    private func gain(_ y: CGFloat, _ h: CGFloat) -> Double { Double((h / 2 - y) / (h / 2 - 8)) * Self.range }

    private func nodePoint(_ b: FilterRec, _ s: CGSize) -> CGPoint { CGPoint(x: x(b.freq, s.width), y: y(b.gain, s.height)) }

    private func node(near p: CGPoint, _ s: CGSize) -> Int? {
        model.filters.enumerated().map { ($0.offset, hypot(nodePoint($0.element, s).x - p.x, nodePoint($0.element, s).y - p.y)) }
            .filter { $0.1 < 14 }.min { $0.1 < $1.1 }?.0
    }

    private func dragChanged(_ v: DragGesture.Value, _ s: CGSize) {
        let mods = NSEvent.modifierFlags
        if drag == nil {
            guard let i = node(near: v.startLocation, s) else { return }
            model.dragKey += 1
            model.selected = i
            drag = (i, model.filters[i], v.startLocation)
        }
        guard let d = drag else { return }
        let fine: CGFloat = mods.contains(.command) ? 0.2 : 1
        var dx = v.translation.width * fine, dy = v.translation.height * fine
        if mods.contains(.shift) { if abs(dx) > abs(dy) { dy = 0 } else { dx = 0 } }
        let b = d.start
        if mods.contains(.option) {
            // Q (peaks, shelves) or slope (passes) by vertical motion.
            switch b {
            case .highPass(let f, let sl), .lowPass(let f, let sl):
                let i = slopeOrder.firstIndex { $0.0 == sl } ?? 2
                let j = min(max(i + Int((-dy / 30).rounded()), 0), slopeOrder.count - 1)
                let n: FilterRec = b.kind == .highPass ? .highPass(freqHz: f, slope: slopeOrder[j].0) : .lowPass(freqHz: f, slope: slopeOrder[j].0)
                model.replace(d.index, n, coalesce: true)
            default:
                model.replace(d.index, b.with(q: (b.q ?? 1) * pow(2, Double(-dy / 60))), coalesce: true)
            }
            return
        }
        let p0 = nodePoint(b, s)
        let nf = freq(p0.x + dx, s.width)
        let ng = b.kind == .highPass || b.kind == .lowPass ? 0 : gain(p0.y + dy, s.height)
        model.replace(d.index, b.with(freq: nf, gain: ng), coalesce: true)
    }

    // Drawing
    private func draw(_ g: inout GraphicsContext, _ s: CGSize, _ f: [FilterRec], _ c: FilterCurvesRec) {
        // Grid: decades and octave labels, ±6 dB lines.
        for fr in [20.0, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000] {
            let xx = x(fr, s.width)
            g.stroke(Path { $0.move(to: CGPoint(x: xx, y: 0)); $0.addLine(to: CGPoint(x: xx, y: s.height)) }, with: .color(Color.wd.plotGridMinor), lineWidth: 1)
            g.draw(Text(fr >= 1000 ? "\(Int(fr / 1000))k" : "\(Int(fr))").font(.wd.chip).foregroundColor(Color.wd.textTertiary),
                   at: CGPoint(x: xx + 3, y: s.height - 8), anchor: .leading)
        }
        for db in stride(from: -18.0, through: 18, by: 6) {
            let yy = y(db, s.height)
            g.stroke(Path { $0.move(to: CGPoint(x: 0, y: yy)); $0.addLine(to: CGPoint(x: s.width, y: yy)) },
                     with: .color(db == 0 ? Color.wd.plotGridMajor : Color.wd.plotGridMinor), lineWidth: db == 0 ? 1.2 : 1)
            g.draw(Text(db == 0 ? "0" : String(format: "%+.0f", db)).font(.wd.chip).foregroundColor(Color.wd.textTertiary), at: CGPoint(x: 4, y: yy - 7), anchor: .leading)
        }
        // Click-zone hints along the top.
        for (label, xx, anchor) in [("click: high-pass", 6.0, UnitPoint.leading), ("click: peak", Double(s.width) / 2, .center), ("click: low-pass", Double(s.width) - 6, .trailing)] {
            g.draw(Text(label).font(.wd.chip).foregroundColor(Color.wd.textTertiary.opacity(0.6)), at: CGPoint(x: xx, y: 10), anchor: anchor)
        }
        for edge in [0.2, 0.8] {
            let xx = s.width * edge
            g.stroke(Path { $0.move(to: CGPoint(x: xx, y: 0)); $0.addLine(to: CGPoint(x: xx, y: 18)) }, with: .color(Color.wd.plotGridMajor), lineWidth: 1)
        }
        func curve(_ v: [Double]) -> Path {
            Path { p in
                for (i, fr) in Self.freqs.enumerated() where i < v.count {
                    let pt = CGPoint(x: x(fr, s.width), y: y(max(min(v[i], Self.range + 6), -Self.range - 6), s.height))
                    if i == 0 { p.move(to: pt) } else { p.addLine(to: pt) }
                }
            }
        }
        // Device response with this EQ (its passband, the 90th percentile, at 0 dB), faint.
        if model.showDevice, let m = model.deviceModel, let r = try? model.ctx.store.doc.speakerResponse(modelId: m), r.freqsHz.count > 1 {
            let dev = Self.freqs.map { fr -> Double in
                var k = 0
                while k + 1 < r.freqsHz.count && r.freqsHz[k + 1] < fr { k += 1 }
                return r.magnitudeDb[k]
            }
            let med = dev.sorted()[dev.count * 9 / 10]
            let total = zip(dev, c.totalDb).map { $0 - med + $1 }
            g.stroke(curve(total), with: .color(Color.wd.textTertiary.opacity(0.6)), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
        }
        for (i, band) in c.bandsDb.enumerated() {
            let col = Color.wd.curve(i)
            var fill = curve(band)
            fill.addLine(to: CGPoint(x: s.width, y: y(0, s.height)))
            fill.addLine(to: CGPoint(x: 0, y: y(0, s.height)))
            g.fill(fill, with: .color(col.opacity(model.selected == i ? 0.16 : 0.07)))
            g.stroke(curve(band), with: .color(col.opacity(0.55)), lineWidth: 1)
        }
        g.stroke(curve(c.totalDb), with: .color(Color.wd.textPrimary), lineWidth: 2)
        for (i, b) in f.enumerated() {
            let p = nodePoint(b, s)
            let r: CGFloat = model.selected == i || model.hovered == i ? 7 : 5.5
            let rect = CGRect(x: p.x - r, y: p.y - r, width: 2 * r, height: 2 * r)
            g.fill(Path(ellipseIn: rect), with: .color(Color.wd.curve(i)))
            g.stroke(Path(ellipseIn: rect), with: .color(model.selected == i ? Color.wd.textPrimary : Color.wd.surfaceChrome), lineWidth: 1.5)
            g.draw(Text("\(i + 1)").font(.wd.chip).foregroundColor(Color.wd.surfaceChrome), at: p)
        }
    }
}

// MARK: - Window content

struct EQEditorView: View {
    @Bindable var model: EQModel

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: .wd.s8) {
                Color.clear.frame(width: .wd.trafficLights)
                WDIcon(.filter).foregroundStyle(Color.wd.source)
                Text(model.title).font(.wd.title).foregroundStyle(Color.wd.textPrimary)
                if model.mixed { Text("mixed chains · edits apply this one to all").font(.wd.detail).foregroundStyle(Color.wd.statusOutdated) }
                Spacer()
                if model.deviceModel != nil { WDCheckbox("Device response", isOn: model.showDevice) { model.showDevice = $0 } }
            }
            .padding(.trailing, .wd.s12)
            .frame(height: .wd.topBar)
            .background(Color.wd.surfaceBar)
            Rectangle().fill(Color.wd.hairline).frame(height: 1)
            EQGraph(model: model)
                .background(LinearGradient(colors: [Color.wd.viewportCentre, Color.wd.viewportEdge], startPoint: .top, endPoint: .bottom))
                .frame(minHeight: 260)
            Rectangle().fill(Color.wd.hairline).frame(height: 1)
            ScrollView(.horizontal) {
                HStack(alignment: .top, spacing: .wd.s8) {
                    ForEach(Array(model.filters.enumerated()), id: \.offset) { i, b in band(i, b) }
                    if model.filters.isEmpty {
                        Text("Click the graph to add a band: left edge high-pass, right edge low-pass, elsewhere a peak at that gain. Drag a node: frequency and gain · scroll over it (or ⌥-drag): Q or slope · ⇧ locks an axis · ⌘ fine · double-click a node to remove it · right-click to change its type.")
                            .font(.wd.detail).foregroundStyle(Color.wd.textTertiary).padding(.wd.s12)
                            .frame(width: 640, alignment: .leading).fixedSize(horizontal: false, vertical: true)
                    }
                }
                .padding(.wd.s8)
            }
            .frame(height: 132)
            .background(Color.wd.surfacePanel)
        }
        .background(Color.wd.surfaceChrome)
    }

    private func band(_ i: Int, _ b: FilterRec) -> some View {
        let hz = UnitFormatter(.frequency, decimals: 0), db = UnitFormatter(.level)
        return VStack(alignment: .leading, spacing: .wd.s4) {
            HStack(spacing: .wd.s4) {
                Circle().fill(Color.wd.curve(i)).frame(width: 9, height: 9)
                WDDropdown(BandKind.allCases.map { WDDropdownItem($0.rawValue, $0.rawValue) }, selection: b.kind.rawValue) { k in
                    model.replace(i, b.converted(to: BandKind(rawValue: k) ?? .peak))
                }
                WDIconButton(.close, size: .small, help: "Remove band") { model.remove(i) }
            }
            WDNumberField(b.freq, hz, range: 10...30000) { model.replace(i, b.with(freq: $0)) }
            switch b {
            case .highPass(let f, let sl), .lowPass(let f, let sl):
                WDDropdown(slopeOrder.map { WDDropdownItem($0.1, $0.1) }, selection: slopeOrder.first { $0.0 == sl }?.1) { name in
                    let n = slopeOrder.first { $0.1 == name }?.0 ?? .linkwitzRiley24
                    model.replace(i, b.kind == .highPass ? .highPass(freqHz: f, slope: n) : .lowPass(freqHz: f, slope: n))
                }
            default:
                HStack(spacing: .wd.s4) {
                    WDNumberField(b.gain, db, range: -24...24) { model.replace(i, b.with(gain: $0)) }
                    WDNumberField(b.q, UnitFormatter(.plain), range: 0.1...20, unit: "Q") { model.replace(i, b.with(q: $0)) }
                }
            }
        }
        .padding(.wd.s8)
        .frame(width: 196)
        .background(RoundedRectangle(cornerRadius: .wd.radiusSegment).fill(model.selected == i ? Color.wd.selectionRow : Color.wd.controlFill.opacity(0.5)))
        .onTapGesture { model.selected = i }
    }
}

/// One EQ window per target.
@MainActor
final class EQWindow: NSWindowController, NSWindowDelegate {
    private static var open: [EQTarget: EQWindow] = [:]
    let model: EQModel

    static func show(ctx: WindowContext, target: EQTarget) {
        if let w = open[target] { w.window?.makeKeyAndOrderFront(nil); return }
        let w = EQWindow(model: EQModel(ctx: ctx, target: target))
        open[target] = w
        w.window?.center()
        w.showWindow(nil)
    }

    init(model: EQModel) {
        self.model = model
        let win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 920, height: 520),
                           styleMask: [.titled, .closable, .resizable, .miniaturizable, .fullSizeContentView], backing: .buffered, defer: false)
        win.titlebarAppearsTransparent = true
        win.titleVisibility = .hidden
        win.appearance = NSAppearance(named: .darkAqua)
        win.backgroundColor = NSColor(Color.wd.surfaceChrome)
        win.minSize = NSSize(width: 640, height: 420)
        win.title = model.title
        win.isReleasedWhenClosed = false
        super.init(window: win)
        win.delegate = self
        win.contentView = NSHostingView(rootView: EQEditorView(model: model))
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func windowWillClose(_ notification: Notification) { Self.open[model.target] = nil }

    static func snapshotView(ctx: WindowContext, target: EQTarget) -> AnyView {
        AnyView(EQEditorView(model: EQModel(ctx: ctx, target: target)))
    }
}

extension WindowContext {
    /// EQ of the selected speakers (or the selected group).
    public func openEQ() {
        if let g = selection.ids.compactMap(\.groupID).first { return openEQ(group: g) }
        let s = selection.speakers
        guard !s.isEmpty else { post("Select sources or a group to edit its EQ", .info); return }
        EQWindow.show(ctx: self, target: .speakers(s))
    }

    func openEQ(group: UInt64) { EQWindow.show(ctx: self, target: .group(group)) }
    func openEQ(speakers: [UInt64]) { EQWindow.show(ctx: self, target: .speakers(speakers)) }

    public func eqSnapshotView() -> AnyView? {
        if let g = store.groups.first(where: { !$0.members.isEmpty }) { return EQWindow.snapshotView(ctx: self, target: .group(g.id)) }
        if let s = store.speakers.first { return EQWindow.snapshotView(ctx: self, target: .speakers([s.id])) }
        return nil
    }
}
