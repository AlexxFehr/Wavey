import AppKit
import SwiftUI
import WaveyDesign
import WaveyFeatures
import WaveyModel

@main
enum Main {
    static func main() {
        if CommandLine.arguments.contains("--legacy-ui") {
            LegacyApp.main()
        } else {
            WaveyApp.main()
        }
    }
}

final class ShellDelegate: NSObject, NSApplicationDelegate {
    @MainActor var openFile: ((URL) -> Void)?
    @MainActor private var pending: [URL] = []

    @MainActor func application(_ application: NSApplication, open urls: [URL]) {
        if let openFile { urls.forEach(openFile) } else { pending += urls }
    }

    @MainActor func connect(_ handler: @escaping (URL) -> Void) {
        openFile = handler
        pending.forEach(handler)
        pending = []
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.appearance = NSAppearance(named: .darkAqua)
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

/// Holds the window's context; replaced when another document is opened.
@MainActor @Observable
final class AppState {
    var ctx: WindowContext

    init() {
        WDFonts.register()
        let args = Array(CommandLine.arguments.dropFirst())
        var store = DocumentStore()
        if let path = args.first(where: { $0.hasSuffix(".wavey") }) {
            do { store = try DocumentStore(opening: URL(fileURLWithPath: path)) } catch { NSLog("Wavey: \(describe(error))") }
        }
        ctx = WindowContext(store: store)
        configure()
        if !ctx.loadUIState() { ctx.fitAll() }
        applyArguments(args)
    }

    private func configure() {
        ctx.openRequested = { [weak self] url in self?.open(url) }
    }

    func open(_ url: URL) {
        do {
            let store = try DocumentStore(opening: url)
            ctx.saveUIState()
            ctx = WindowContext(store: store)
            configure()
            if !ctx.loadUIState() { ctx.fitAll() }
            NSDocumentController.shared.noteNewRecentDocumentURL(url)
        } catch {
            ctx.post(describe(error), .error)
        }
    }

    func newDocument() {
        ctx = WindowContext(store: DocumentStore())
        configure()
        ctx.fitAll()
    }

    /// `--workspace geometry|sources|results`, `--freq <hz>`, `--select-first-speaker`,
    /// `--select-first-plane`, `--snapshot <png>`.
    private func applyArguments(_ args: [String]) {
        func value(_ flag: String) -> String? {
            guard let i = args.firstIndex(of: flag), i + 1 < args.count else { return nil }
            return args[i + 1]
        }
        if let w = value("--workspace"), let ws = Workspace(rawValue: w) { ctx.workspace = ws }
        if let f = value("--freq").flatMap(Double.init) { ctx.setFrequency(f) }
        if args.contains("--select-first-speaker"), let s = ctx.store.speakers.first { ctx.select([.speaker(s.id)]) }
        if args.contains("--select-first-plane"), let p = ctx.store.cutPlanes.first { ctx.select([.plane(p.id)]) }
        if args.contains("--select-first-surface"), let s = ctx.store.room?.surfaces.first(where: { $0.kind == .wall }) { ctx.select([.surface(s.key)]) }
        if args.contains("--selftest") { DispatchQueue.main.async { self.ctx.runSelfTest() } }
        if args.contains("--bench") { DispatchQueue.main.async { self.ctx.runBench() } }
        if let path = value("--snapshot") { ShellSnapshot.schedule(ctx: ctx, to: path) }
    }
}

struct WaveyApp: App {
    @NSApplicationDelegateAdaptor(ShellDelegate.self) private var delegate
    @State private var app = AppState()

    var body: some Scene {
        WindowGroup {
            ShellView(context: app.ctx)
                .id(ObjectIdentifier(app.ctx))
                .frame(minWidth: 1180, minHeight: 720)
                .ignoresSafeArea()
                .onAppear { delegate.connect { app.open($0) } }
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 1440, height: 900)
        .commands { AppCommands(app: app) }
    }
}

/// Native menus generated from the command registry.
struct AppCommands: Commands {
    let app: AppState

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New") { app.newDocument() }.keyboardShortcut("n")
            Button("Open…") { app.ctx.openPanel() }.keyboardShortcut("o")
        }
        CommandGroup(replacing: .saveItem) {
            Button("Save") { app.ctx.save() }.keyboardShortcut("s")
            Button("Save As…") { app.ctx.saveAs() }.keyboardShortcut("s", modifiers: [.command, .shift])
        }
        CommandGroup(replacing: .undoRedo) {
            Button(app.ctx.store.undoName.map { "Undo \($0)" } ?? "Undo") { app.ctx.undo() }.keyboardShortcut("z")
            Button(app.ctx.store.redoName.map { "Redo \($0)" } ?? "Redo") { app.ctx.redo() }.keyboardShortcut("z", modifiers: [.command, .shift])
        }
        CommandGroup(after: .pasteboard) { menu("Edit", skip: ["file."]) }
        CommandGroup(after: .toolbar) { menu("View", skip: []) }
        CommandMenu("Model") { menu("Model", skip: []) }
        CommandMenu("Analysis") { menu("Analysis", skip: []) }
    }

    @ViewBuilder private func menu(_ name: String, skip: [String]) -> some View {
        let ctx = app.ctx
        ForEach(ctx.commands.filter { $0.menu == name && !skip.contains(where: $0.id.hasPrefix) }) { c in
            if let key = c.key {
                Button(c.title) { c.perform(ctx) }.keyboardShortcut(key, modifiers: c.modifiers).disabled(!c.isEnabled(ctx))
            } else {
                Button(c.title) { c.perform(ctx) }.disabled(!c.isEnabled(ctx))
            }
        }
    }
}
