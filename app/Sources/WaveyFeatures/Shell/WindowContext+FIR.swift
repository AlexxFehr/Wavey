import AppKit
import SwiftUI
import UniformTypeIdentifiers
import WaveyCore
import WaveyDesign
import WaveyModel

extension WindowContext {
    /// Asks for a WAV or text file of taps and sets it as the FIR filter of
    /// `target`. A WAV carries its own sample rate; a text file takes the one
    /// chosen in the panel.
    func chooseFIR(_ target: FirTarget) {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.wav, .plainText, .commaSeparatedText, UTType(filenameExtension: "txt") ?? .plainText]
        panel.message = "Choose a FIR filter: a WAV impulse response, or a text file of taps"
        panel.prompt = "Use as FIR"
        let rates = [44100.0, 48000.0, 88200.0, 96000.0]
        let popup = NSPopUpButton(frame: .zero, pullsDown: false)
        popup.addItems(withTitles: rates.map { String(format: "%g kHz", $0 / 1000) })
        popup.selectItem(at: 1)
        let label = NSTextField(labelWithString: "Text file sample rate:")
        let row = NSStackView(views: [label, popup])
        row.spacing = 8
        row.edgeInsets = NSEdgeInsets(top: 8, left: 12, bottom: 8, right: 12)
        panel.accessoryView = row
        panel.isAccessoryViewDisclosed = true
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let rate = rates[max(0, popup.indexOfSelectedItem)]
        var info: FirInfoRec?
        if edit("Set FIR Filter", { info = try $0.setFir(target: target, path: url.path, textSampleRateHz: rate) }), let i = info {
            post(String(format: "FIR %@: %d taps at %g kHz, latency %.2f ms", i.name, i.taps, i.sampleRateHz / 1000, i.latencyMs))
        }
    }

    func clearFIR(_ target: FirTarget) {
        edit("Remove FIR Filter") { $0.clearFir(target: target) }
    }
}

/// The FIR row of a source or group: its summary, and buttons to load or remove one.
struct FIRRow: View {
    let fir: FirInfoRec?
    let mixed: Bool
    let target: FirTarget
    let ctx: WindowContext

    var body: some View {
        WDPropertyRow("FIR") {
            HStack(spacing: .wd.s6) {
                Text(summary).font(.wd.numeric).foregroundStyle(Color.wd.textSecondary).lineLimit(1).truncationMode(.middle)
                Spacer(minLength: .wd.s4)
                Button(fir == nil && !mixed ? "Load…" : "Replace…") { ctx.chooseFIR(target) }
                    .buttonStyle(.plain).font(.wd.detail).foregroundStyle(Color.wd.textTertiary)
                    .help("Load a WAV impulse response or a text file of taps")
                if fir != nil || mixed {
                    WDIconButton(.close, size: .small, help: "Remove the FIR filter") { ctx.clearFIR(target) }
                }
            }
        }
    }

    private var summary: String {
        if mixed { return "mixed" }
        guard let f = fir else { return "None" }
        return "\(f.name) · \(f.taps) taps · \(String(format: "%.1f", f.latencyMs)) ms"
    }
}
