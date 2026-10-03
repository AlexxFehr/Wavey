import AppKit
import UniformTypeIdentifiers
import WaveyCore
import WaveyModel

/// The stock speaker library: using its devices in the project, adding to it,
/// and where it lives.
extension WindowContext {
    var library: SpeakerLibrary { SpeakerLibrary.shared }

    /// The project's model id for a library file, adding the model to the
    /// project if it is not there yet (an undoable step).
    @discardableResult
    func useLibraryFile(_ path: String) -> String? {
        if let e = library.entriesByPath[path], store.modelsById[e.id] != nil { return e.id }
        var id: String?
        edit("Add Device") { id = try $0.useLibraryModel(path: path) }
        return id
    }

    func deviceID(for item: LibrarySection.Item) -> String? {
        item.projectID ?? item.path.flatMap { useLibraryFile($0) }
    }

    func addFilesToLibrary() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "waveyspk") ?? .data]
        panel.allowsMultipleSelection = true
        panel.message = "Copy speaker models into the library (\(library.location.path))"
        guard panel.runModal() == .OK else { return }
        do {
            let n = try library.add(files: panel.urls)
            post("Added \(n) device\(n == 1 ? "" : "s") to the library")
        } catch { post("\(error)", .error) }
    }

    func addProjectModelToLibrary(_ id: String) {
        do {
            try library.add(model: id, from: store.doc, manufacturer: store.modelsById[id]?.manufacturer)
            post("Added \(store.modelsById[id]?.name ?? id) to the library")
        } catch { post("\(error)", .error) }
    }

    func chooseLibraryLocation() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.prompt = "Use as Library"
        panel.message = "Choose the folder of the stock speaker library"
        panel.directoryURL = library.location
        guard panel.runModal() == .OK, let url = panel.url else { return }
        library.setLocation(url)
        post("Library: \(url.path)")
    }
}
