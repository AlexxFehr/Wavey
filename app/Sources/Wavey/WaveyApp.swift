import AppKit
import SwiftUI
import WaveyCore

final class AppDelegate: NSObject, NSApplicationDelegate {
    /// Set by the app once its model exists; receives Finder "open" requests.
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
        // Needed when launched as a bare executable rather than an .app bundle.
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

/// The previous six-mode UI, kept behind `--legacy-ui` until the new shell covers everything.
struct LegacyApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @State private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            ContentView(model: model)
                .frame(minWidth: 1000, minHeight: 650)
                .navigationTitle(model.title)
                .onAppear { delegate.connect { model.open($0) } }
        }
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("New") { model.newDocument() }.keyboardShortcut("n")
                Button("Open…") { model.openWithPanel() }.keyboardShortcut("o")
            }
            CommandGroup(replacing: .saveItem) {
                Button("Save") { model.save() }.keyboardShortcut("s")
                Button("Save As…") { model.saveAs() }.keyboardShortcut("s", modifiers: [.command, .shift])
            }
            CommandGroup(replacing: .undoRedo) {
                Button("Undo") { model.undo() }.keyboardShortcut("z")
                Button("Redo") { model.redo() }.keyboardShortcut("z", modifiers: [.command, .shift])
            }
        }
    }
}
