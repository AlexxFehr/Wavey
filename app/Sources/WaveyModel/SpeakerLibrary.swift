import AppKit
import Foundation
import Observation
import WaveyCore

/// The stock speaker library: a folder of `.waveyspk` files (by default
/// ~/Documents/Wavey Library, created on first use), shared by all windows.
/// Projects embed the models they use; the library is where they come from.
@MainActor @Observable
public final class SpeakerLibrary {
    public static let shared = SpeakerLibrary()
    static let locationKey = "stockLibraryPath"

    /// The library folder (its speakers live below `Speakers/`, original
    /// GLL files below `GLL/`; any `.waveyspk` below the folder is listed).
    public private(set) var location: URL
    public private(set) var entries: [LibraryEntryRec] = []
    public private(set) var errors: [String] = []
    public private(set) var scanning = false
    public private(set) var entriesByPath: [String: LibraryEntryRec] = [:]

    public static var defaultLocation: URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("Wavey Library", isDirectory: true)
    }

    private init() {
        if let p = UserDefaults.standard.string(forKey: Self.locationKey), !p.isEmpty {
            location = URL(fileURLWithPath: p, isDirectory: true)
        } else {
            location = Self.defaultLocation
        }
        ensureFolders()
        refresh()
    }

    public var isDefaultLocation: Bool { location.standardizedFileURL == Self.defaultLocation.standardizedFileURL }

    /// Creates the folder and its `Speakers/` and `GLL/` subfolders.
    public func ensureFolders() {
        let fm = FileManager.default
        for sub in ["", "Speakers", "GLL"] {
            try? fm.createDirectory(at: location.appendingPathComponent(sub, isDirectory: true), withIntermediateDirectories: true)
        }
    }

    /// Links another folder as the stock location.
    public func setLocation(_ url: URL) {
        location = url
        UserDefaults.standard.set(url.path, forKey: Self.locationKey)
        ensureFolders()
        refresh()
    }

    public func resetLocation() {
        UserDefaults.standard.removeObject(forKey: Self.locationKey)
        setLocation(Self.defaultLocation)
        UserDefaults.standard.removeObject(forKey: Self.locationKey)
    }

    /// Rescans the folder (in the background; unchanged files come from the index).
    public func refresh() {
        let dir = location.path
        scanning = true
        Task.detached(priority: .utility) {
            let r = scanSpeakerLibrary(dir: dir)
            await MainActor.run {
                guard self.location.path == dir else { return }
                self.entries = r.entries
                self.errors = r.errors
                self.entriesByPath = Dictionary(r.entries.map { ($0.path, $0) }, uniquingKeysWith: { a, _ in a })
                self.scanning = false
            }
        }
    }

    /// Manufacturer used for grouping (case-folded variants merge).
    public static func groupName(_ manufacturer: String?) -> String {
        let m = (manufacturer ?? "").trimmingCharacters(in: .whitespaces)
        return m.isEmpty ? "Other" : m
    }

    /// Copies model files into `Speakers/<Manufacturer>/`.
    public func add(files: [URL]) throws -> Int {
        var n = 0
        for f in files {
            let manufacturer = SpeakerLibrary.groupName(entryManufacturer(f))
            let dir = location.appendingPathComponent("Speakers", isDirectory: true).appendingPathComponent(manufacturer, isDirectory: true)
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            let dest = dir.appendingPathComponent(f.lastPathComponent)
            if FileManager.default.fileExists(atPath: dest.path) { try FileManager.default.removeItem(at: dest) }
            try FileManager.default.copyItem(at: f, to: dest)
            n += 1
        }
        refresh()
        return n
    }

    /// Writes a project model into the library.
    public func add(model id: String, from doc: WaveyDocument, manufacturer: String?) throws {
        let dir = location.appendingPathComponent("Speakers", isDirectory: true).appendingPathComponent(SpeakerLibrary.groupName(manufacturer), isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try doc.exportSpeakerModel(id: id, path: dir.appendingPathComponent("\(id).waveyspk").path)
        refresh()
    }

    private func entryManufacturer(_ url: URL) -> String? {
        scanSpeakerLibrary(dir: url.deletingLastPathComponent().path).entries.first { $0.path == url.path }?.manufacturer
    }

    public func reveal(_ path: String? = nil) {
        if let p = path {
            NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: p)])
        } else {
            NSWorkspace.shared.open(location)
        }
    }
}
