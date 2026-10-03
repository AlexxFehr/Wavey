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
        if let lib = value("--place") { ctx.placeLibraryFile(lib) }
        if args.contains("--frame") || args.contains("--deselect") || value("--yaw") != nil {
            // After the snapshot's fit-all: frame the selection, then optionally clear it.
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { [ctx] in
                if args.contains("--frame") { ctx.frameSelection() }
                if let y = value("--yaw").flatMap(Double.init) { ctx.viewport.orbit.yaw = y }
                if args.contains("--deselect") { ctx.select([]) }
                ctx.rebuildScenes()
            }
        }
        if args.contains("--demo-assemblies") { ctx.insertDemoAssemblies() }
        if args.contains("--dump-menus") {
            // The menu tree as the registry builds it (checks the categories).
            for menu in ["File", "Edit", "View", "Geometry", "Sources", "Analysis", "Library"] {
                print(menu)
                let cmds = ctx.commands.filter { $0.menu == menu }
                for c in cmds where c.submenu == nil { print("  \(c.title)\(c.shortcut.map { "  \($0)" } ?? "")") }
                for sub in cmds.compactMap(\.submenu).reduce(into: [String]()) { if !$0.contains($1) { $0.append($1) } } {
                    print("  \(sub) ▸")
                    for c in cmds where c.submenu == sub { print("      \(c.title)\(c.shortcut.map { "  \($0)" } ?? "")") }
                }
            }
            exit(0)
        }
        if args.contains("--no-dock") { ctx.state.dockVisible = false }
        if args.contains("--select-first-group"), let g = ctx.store.groups.last { ctx.select([.group(g.id)]) }
        if args.contains("--selftest") { DispatchQueue.main.async { self.ctx.runSelfTest() } }
        if args.contains("--bench") { DispatchQueue.main.async { self.ctx.runBench() } }
        if let path = value("--snapshot") {
            if args.contains("--eq-editor"), let view = ctx.eqSnapshotView() {
                ShellSnapshot.schedule(ctx: ctx, to: path, size: CGSize(width: 920, height: 520), root: view)
            } else if let lib = value("--library-preview") {
                ShellSnapshot.schedule(ctx: ctx, to: path, size: CGSize(width: 354, height: 720), root: ctx.libraryPreviewView(path: lib))
            } else if let kind = value("--assembly-editor") {
                // The stack or line-array editor (of the first such group, else a new one).
                ShellSnapshot.schedule(ctx: ctx, to: path, size: CGSize(width: 1240, height: 760), root: ctx.assemblyEditorView(kind: kind))
            } else {
                ShellSnapshot.schedule(ctx: ctx, to: path)
            }
        }
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
        CommandGroup(replacing: .pasteboard) {
            Button("Cut") { NSApp.sendAction(#selector(NSText.cut(_:)), to: nil, from: nil) }.keyboardShortcut("x")
            Button("Copy") { if editingText { NSApp.sendAction(#selector(NSText.copy(_:)), to: nil, from: nil) } else { app.ctx.copySelection() } }
                .keyboardShortcut("c")
            Button("Paste") { if editingText { NSApp.sendAction(#selector(NSText.paste(_:)), to: nil, from: nil) } else { app.ctx.paste() } }
                .keyboardShortcut("v")
        }
        CommandGroup(after: .newItem) { menu("File", skip: ["file.open"]) }
        CommandGroup(after: .pasteboard) { menu("Edit", skip: []) }
        CommandGroup(after: .toolbar) { menu("View", skip: []) }
        Group {
            CommandMenu("Geometry") { menu("Geometry", skip: []) }
            CommandMenu("Sources") { menu("Sources", skip: []) }
            CommandMenu("Analysis") { menu("Analysis", skip: []) }
            CommandMenu("Library") { menu("Library", skip: []) }
        }
    }

    /// A text field or view has the keyboard: the pasteboard items act on its text.
    private var editingText: Bool { NSApp.keyWindow?.firstResponder is NSText }

    /// A menu from the registry: top-level items first (in order), then a
    /// submenu per `submenu`, separated where the category changes.
    @ViewBuilder private func menu(_ name: String, skip: [String]) -> some View {
        let ctx = app.ctx
        let all = ctx.commands.filter { $0.menu == name && !skip.contains(where: $0.id.hasPrefix) && !($0.id == "file.save" || $0.id == "file.saveAs") }
        let top = all.filter { $0.submenu == nil }
        let subs = all.compactMap(\.submenu).reduce(into: [String]()) { if !$0.contains($1) { $0.append($1) } }
        ForEach(top) { c in item(c, ctx) }
        if !subs.isEmpty && !top.isEmpty { Divider() }
        ForEach(subs, id: \.self) { sub in
            Menu(sub) { ForEach(all.filter { $0.submenu == sub }) { c in item(c, ctx) } }
        }
    }

    @ViewBuilder private func item(_ c: AppCommand, _ ctx: WindowContext) -> some View {
        if let key = c.key {
            Button(c.title) { c.perform(ctx) }.keyboardShortcut(key, modifiers: c.modifiers).disabled(!c.isEnabled(ctx))
        } else {
            Button(c.title) { c.perform(ctx) }.disabled(!c.isEnabled(ctx))
        }
    }
}
