import AppKit
import WaveyViewport

extension WindowContext {
    /// Keys pressed while focus is in the outliner, inspector or a plot
    /// (not a text field, not the viewport) go to the viewport's tools, so
    /// Delete, Esc and tool keys work wherever the selection was made.
    /// True if the key was handled.
    public func routeShellKey(_ e: NSEvent) -> Bool {
        guard let w = e.window, w === shellWindow, w.attachedSheet == nil else { return false }
        let responder = w.firstResponder
        if responder is NSText || responder is NSTextField { return false }
        if let r = responder, String(describing: type(of: r)) == "ViewportMTKView" { return false }
        let flags = e.modifierFlags.intersection([.command, .control, .option])
        guard flags.isEmpty else { return false }
        let chars = e.charactersIgnoringModifiers ?? ""
        let deleteOrEscape = [51, 117, 53].contains(e.keyCode)
        guard deleteOrEscape || chars.unicodeScalars.allSatisfy({ CharacterSet.alphanumerics.contains($0) || ",.<>".unicodeScalars.contains($0) }), !chars.isEmpty || deleteOrEscape else { return false }
        var mods: ModifierSet = []
        if e.modifierFlags.contains(.shift) { mods.insert(.shift) }
        tool.handle(.key(chars, keyCode: e.keyCode, modifiers: mods), in: workspace)
        return true
    }
}
