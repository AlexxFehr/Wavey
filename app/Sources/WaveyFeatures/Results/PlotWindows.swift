import AppKit
import SwiftUI
import WaveyCore
import WaveyDesign
import WaveyModel

/// A microphone's responses: magnitude, phase and impulse response.
struct MicResponseView: View {
    @Bindable var ctx: WindowContext
    let mic: UInt64
    @State private var tab = "magnitude"

    var body: some View {
        let m = ctx.store.micsById[mic]
        VStack(spacing: 0) {
            // Like the top bar: name on the left, the view switcher centred,
            // smoothing on the right (always there; off for phase and impulse).
            ZStack {
                HStack(spacing: .wd.s8) {
                    Color.clear.frame(width: .wd.trafficLights)
                    WDIcon(.microphone).foregroundStyle(Color.wd.receiver)
                    Text(m.map { "\($0.code) · \($0.name)" } ?? "Microphone").font(.wd.title).foregroundStyle(Color.wd.textPrimary)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    if tab == "impulse" {
                        let linear = ctx.store.activeRun?.octaveFraction == 0
                        WDTextButton("Export…") { ctx.exportImpulse(mic: mic) }
                            .disabled(!linear)
                            .help(linear ? "Save this impulse response as a WAV or CSV file" : "Impulse responses need a run with a linear frequency step")
                    }
                    HeaderLabel("Smoothing")
                    WDChipGroup([(0, "None"), (6, "1/6"), (3, "1/3")], selection: Binding(get: { ctx.results.smoothing }, set: { ctx.results.smoothing = $0 }))
                        .disabled(tab != "magnitude")
                        .opacity(tab == "magnitude" ? 1 : 0.4)
                }
                WDWorkspaceSwitcher([("magnitude", "Magnitude", WDIconName.chart), ("phase", "Phase", .waveform), ("impulse", "Impulse", .line)],
                                    selection: tab) { tab = $0 }
            }
            .padding(.trailing, .wd.s12)
            .frame(height: .wd.topBar)
            .background(Color.wd.surfaceBar)
            Rectangle().fill(Color.wd.hairline).frame(height: 1)
            Group {
                switch tab {
                case "phase": ResponsePlotDock(ctx: ctx, phase: true, only: mic)
                case "impulse": ImpulseDock(ctx: ctx, only: mic)
                default: ResponsePlotDock(ctx: ctx, phase: false, only: mic)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color.wd.surfacePanel)
        }
        .background(Color.wd.surfaceChrome)
        .environment(\.colorScheme, .dark)
    }
}

/// SPL along the probe line.
struct ProbeLineView: View {
    @Bindable var ctx: WindowContext

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: .wd.s8) {
                Color.clear.frame(width: .wd.trafficLights)
                WDIcon(.probeLine).foregroundStyle(Color.wd.accent)
                Text("Probe line").font(.wd.title).foregroundStyle(Color.wd.textPrimary)
                Text(String(format: "%.1f Hz", ctx.results.frequency)).font(.wd.numeric).foregroundStyle(Color.wd.textTertiary)
                Spacer()
            }
            .padding(.trailing, .wd.s12)
            .frame(height: .wd.topBar)
            .background(Color.wd.surfaceBar)
            Rectangle().fill(Color.wd.hairline).frame(height: 1)
            SpatialPlot(ctx: ctx)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(Color.wd.surfacePanel)
        }
        .background(Color.wd.surfaceChrome)
        .environment(\.colorScheme, .dark)
    }
}

/// One plot window per key (a microphone, the probe line).
@MainActor
final class PlotWindow: NSWindowController, NSWindowDelegate {
    private static var open: [String: PlotWindow] = [:]
    let key: String

    static func show(_ key: String, title: String, size: NSSize = NSSize(width: 820, height: 460), content: () -> AnyView) {
        if let w = open[key] { front(w); return }
        let w = PlotWindow(key: key, title: title, size: size, content: content())
        open[key] = w
        w.window?.center()
        front(w)
    }

    /// Shows the window in front once the click that asked for it is over
    /// (opened mid-click, the main window came back over it on mouse-up).
    private static func front(_ w: PlotWindow) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
            w.showWindow(nil)
            w.window?.makeKeyAndOrderFront(nil)
        }
    }

    init(key: String, title: String, size: NSSize, content: AnyView) {
        self.key = key
        let win = NSWindow(contentRect: NSRect(origin: .zero, size: size),
                           styleMask: [.titled, .closable, .resizable, .miniaturizable, .fullSizeContentView], backing: .buffered, defer: false)
        win.titlebarAppearsTransparent = true
        win.titleVisibility = .hidden
        win.appearance = NSAppearance(named: .darkAqua)
        win.backgroundColor = NSColor(Color.wd.surfaceChrome)
        win.minSize = NSSize(width: 520, height: 300)
        win.title = title
        win.isReleasedWhenClosed = false
        super.init(window: win)
        win.delegate = self
        win.contentView = NSHostingView(rootView: content)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func windowWillClose(_ notification: Notification) { Self.open[key] = nil }
}

extension WindowContext {
    /// Double-click on an item: show it in the inspector (reopening it),
    /// and for a microphone with results, its response window.
    func openDetails(_ e: EntityRef) {
        if !selection.contains(e) { select([e]) }
        inspectorVisible = true
        if case .microphone(let id) = e, store.resultInfo != nil { openMicResponse(id) }
    }

    func openMicResponse(_ id: UInt64) {
        let name = store.micsById[id].map { "\($0.code) · \($0.name)" } ?? "Microphone"
        PlotWindow.show("mic-\(id)", title: name) { AnyView(MicResponseView(ctx: self, mic: id)) }
    }

    func openProbeLine() {
        PlotWindow.show("probe", title: "Probe line", size: NSSize(width: 720, height: 380)) { AnyView(ProbeLineView(ctx: self)) }
    }

    /// The response window of the first microphone (offscreen snapshots).
    public func micResponseSnapshotView() -> AnyView? {
        guard let m = store.microphones.first(where: { store.resultInfo?.micIds.contains($0.id) == true }) ?? store.microphones.first else { return nil }
        return AnyView(MicResponseView(ctx: self, mic: m.id))
    }
}

extension WindowContext {
    /// Saves a mic's impulse response (the current mix) as a 32-bit float WAV
    /// in pascals, or as CSV, at a chosen sample rate. One period of the
    /// run's frequency step: 1 / step seconds long.
    func exportImpulse(mic: UInt64) {
        guard let r = micResponse(mic), !r.freqsHz.isEmpty, let m = store.micsById[mic] else { post("No response for this microphone", .info); return }
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(m.code) \(m.name) impulse.wav"
        panel.allowedContentTypes = [.wav, .commaSeparatedText]
        panel.allowsOtherFileTypes = false
        panel.message = "Impulse response in pascals: WAV (32-bit float) or CSV (time, pressure)"
        let rates = [44100.0, 48000.0, 96000.0]
        let popup = NSPopUpButton(frame: .zero, pullsDown: false)
        popup.addItems(withTitles: rates.map { String(format: "%g kHz", $0 / 1000) })
        popup.selectItem(at: 1)
        let row = NSStackView(views: [NSTextField(labelWithString: "Sample rate:"), popup])
        row.spacing = 8
        row.edgeInsets = NSEdgeInsets(top: 8, left: 12, bottom: 8, right: 12)
        panel.accessoryView = row
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let rate = rates[max(0, popup.indexOfSelectedItem)]
        // dB SPL and phase back to complex pressure.
        let mag = r.splDb.map { pow(10, $0 / 20) * 2e-5 }
        let re = zip(mag, r.phaseDeg).map { $0 * cos($1 * .pi / 180) }
        let im = zip(mag, r.phaseDeg).map { $0 * sin($1 * .pi / 180) }
        do {
            let n = try exportImpulseResponse(path: url.path, freqs: r.freqsHz, re: re, im: im, sampleRateHz: rate)
            post(String(format: "Saved %@: %d samples, %.2f s at %g kHz", url.lastPathComponent, n, Double(n) / rate, rate / 1000))
        } catch { post("\(error)", .error) }
    }
}

extension WindowContext {
    /// Saves the active run's impulse responses, every solved mic × every
    /// source, into a folder (WAV per pair and a manifest with positions and
    /// arrivals), for filter design outside Wavey.
    func exportImpulseMatrix() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.prompt = "Export Here"
        panel.message = "A WAV per microphone and source (pascals, 32-bit float) and manifest.json"
        let rates = [8000.0, 16000.0, 44100.0, 48000.0]
        let rate = NSPopUpButton(frame: .zero, pullsDown: false)
        rate.addItems(withTitles: rates.map { String(format: "%g kHz", $0 / 1000) })
        let lengths = [1.0, 2.0, 4.0, 8.0, 1000.0]
        let length = NSPopUpButton(frame: .zero, pullsDown: false)
        length.addItems(withTitles: ["1 s", "2 s", "4 s", "8 s", "Whole window"])
        length.selectItem(at: 4)
        let kinds: [ImpulseProcessingRec] = [.processing, .room, .processingAndFir]
        let kind = NSPopUpButton(frame: .zero, pullsDown: false)
        kind.addItems(withTitles: ["With gain, delay and EQ (no FIR)", "Room only", "As heard (FIR included)"])
        let row = NSStackView(views: [NSTextField(labelWithString: "Rate:"), rate, NSTextField(labelWithString: "Length:"), length, kind])
        row.spacing = 8
        row.edgeInsets = NSEdgeInsets(top: 8, left: 12, bottom: 8, right: 12)
        panel.accessoryView = row
        panel.isAccessoryViewDisclosed = true
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let r = rates[max(0, rate.indexOfSelectedItem)], seconds = lengths[max(0, length.indexOfSelectedItem)], k = kinds[max(0, kind.indexOfSelectedItem)]
        post("Exporting impulse responses…", .info)
        let doc = store.doc
        Task.detached(priority: .userInitiated) {
            let result = Result { try doc.exportImpulseMatrix(folder: url.path, sampleRateHz: r, seconds: seconds, processing: k) }
            await MainActor.run { [weak self] in
                switch result {
                case .success(let e): self?.post(String(format: "Exported %d impulse responses, %.2f s each, to %@", e.files, e.seconds, url.lastPathComponent))
                case .failure(let e): self?.post(describe(e), .error)
                }
            }
        }
    }
}
