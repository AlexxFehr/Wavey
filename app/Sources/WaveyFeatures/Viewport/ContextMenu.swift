import AppKit
import WaveyModel
import WaveyViewport

/// NSMenu item that runs a closure.
final class ClosureMenuItem: NSMenuItem {
    private let run: () -> Void
    init(_ title: String, enabled: Bool = true, _ run: @escaping () -> Void) {
        self.run = run
        super.init(title: title, action: #selector(fire), keyEquivalent: "")
        target = self
        isEnabled = enabled
    }
    @available(*, unavailable)
    required init(coder: NSCoder) { fatalError() }
    @objc private func fire() { run() }
}

extension WindowContext {
    /// Native context menu for the viewport: entity actions, or view actions on empty space.
    func showViewportMenu(for e: EntityRef?) {
        let menu = NSMenu()
        menu.autoenablesItems = false
        func add(_ t: String, enabled: Bool = true, _ f: @escaping () -> Void) { menu.addItem(ClosureMenuItem(t, enabled: enabled, f)) }
        if let e {
            add("Zoom to") { [weak self] in self?.reveal(e) }
            let reveal = NSMenuItem(title: "Reveal in", action: nil, keyEquivalent: "")
            let sub = NSMenu()
            for ws in Workspace.allCases { sub.addItem(ClosureMenuItem(ws.title) { [weak self] in self?.reveal(e, in: ws) }) }
            reveal.submenu = sub
            menu.addItem(reveal)
            menu.addItem(.separator())
            switch e {
            case .speaker, .microphone:
                add("Duplicate") { [weak self] in self?.duplicateSelection() }
                if case .speaker = e {
                    add("Aim at…") { [weak self] in self?.tool.activate(.aim) }
                    add("Mirror about centre line") { [weak self] in self?.mirrorSelection() }
                    add("Group (⌘G)") { [weak self] in self?.groupSelection() }
                }
                if case .microphone = e { add("Hide") { [weak self] in self?.hideSelection() } }
                add("Delete") { [weak self] in self?.deleteSelection() }
            case .surface(let k):
                let mat = NSMenuItem(title: "Assign material", action: nil, keyEquivalent: "")
                let sub = NSMenu()
                for m in store.materials {
                    sub.addItem(ClosureMenuItem(m.name) { [weak self] in
                        guard let self else { return }
                        self.assignMaterial(m.id, to: self.selection.surfaces.isEmpty ? [k] : self.selection.surfaces)
                    })
                }
                mat.submenu = sub
                menu.addItem(mat)
                add("Delete edge") { [weak self] in self?.deleteSelection() }
            case .plane:
                add("Hide") { [weak self] in self?.hideSelection() }
                add("Delete") { [weak self] in self?.deleteSelection() }
            default:
                add("Delete") { [weak self] in self?.deleteSelection() }
            }
        } else {
            add("Select all") { [weak self] in self?.selectAll() }
            add("Zoom to fit") { [weak self] in self?.viewport.zoomToFit() }
            menu.addItem(.separator())
            for (title, mode) in [("Perspective", WaveyViewport.ViewMode.perspective), ("Top", .top), ("Front", .front), ("Side", .side)] {
                add(title) { [weak self] in self?.viewport.mode = mode; self?.rebuildScenes() }
            }
            if workspace == .geometry { add("Plan") { [weak self] in self?.viewport.mode = .plan; self?.rebuildScenes() } }
        }
        guard let event = NSApp.currentEvent, let view = NSApp.keyWindow?.contentView else { return }
        NSMenu.popUpContextMenu(menu, with: event, for: view)
    }
}
