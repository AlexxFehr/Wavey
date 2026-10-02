import AppKit
import CoreText
import SwiftUI

/// Geist and Geist Mono (SIL OFL), bundled and registered at launch. If
/// registration fails the system faces are used.
public enum WDFonts {
    private nonisolated(unsafe) static var registered = false

    /// Registers the bundled fonts with the process. Safe to call repeatedly.
    @MainActor public static func register() {
        guard !registered else { return }
        registered = true
        guard let dir = Bundle.module.url(forResource: "Fonts", withExtension: nil),
              let files = try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil) else { return }
        for url in files where url.pathExtension == "ttf" {
            CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
        }
    }

    static func uiName(_ weight: Font.Weight) -> String {
        switch weight {
        case .semibold, .bold, .heavy, .black: "Geist-SemiBold"
        case .medium: "Geist-Medium"
        default: "Geist-Regular"
        }
    }

    static func monoName(_ weight: Font.Weight) -> String {
        weight == .regular || weight == .light || weight == .thin || weight == .ultraLight ? "GeistMono-Regular" : "GeistMono-Medium"
    }

    /// AppKit font for tables and text fields.
    public static func nsUI(_ size: CGFloat, _ weight: NSFont.Weight = .regular) -> NSFont {
        let name = weight >= .semibold ? "Geist-SemiBold" : weight >= .medium ? "Geist-Medium" : "Geist-Regular"
        return NSFont(name: name, size: size) ?? .systemFont(ofSize: size, weight: weight)
    }

    public static func nsMono(_ size: CGFloat, _ weight: NSFont.Weight = .regular) -> NSFont {
        let name = weight >= .medium ? "GeistMono-Medium" : "GeistMono-Regular"
        return NSFont(name: name, size: size) ?? .monospacedSystemFont(ofSize: size, weight: weight)
    }
}

public extension Font {
    static let wd = WDFontSet()
}

public struct WDFontSet: Sendable {
    /// Geist (falls back to SF Pro).
    public func ui(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        NSFont(name: WDFonts.uiName(weight), size: size) != nil
            ? .custom(WDFonts.uiName(weight), fixedSize: size)
            : .system(size: size, weight: weight)
    }

    /// Geist Mono with tabular figures (falls back to SF Mono).
    public func mono(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        let f: Font = NSFont(name: WDFonts.monoName(weight), size: size) != nil
            ? .custom(WDFonts.monoName(weight), fixedSize: size)
            : .system(size: size, weight: weight, design: .monospaced)
        return f.monospacedDigit()
    }

    // Roles (spec §3.2)
    public var appName: Font { ui(13, .semibold) }
    public var switcher: Font { ui(12, .medium) }
    public var title: Font { ui(12, .semibold) }
    public var body: Font { ui(11.5) }
    public var button: Font { ui(11.5, .medium) }
    public var section: Font { ui(11, .semibold) }
    public var label: Font { ui(11.5) }
    public var detail: Font { ui(11) }
    public var numeric: Font { mono(11) }
    public var chip: Font { mono(10.5) }
    public var status: Font { mono(10.5) }
    public var meta: Font { mono(10) }
    public var tick: Font { mono(9.5) }
    public var ring: Font { mono(8.5) }
    public var readout: Font { mono(15, .medium) }
}
