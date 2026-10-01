import SwiftUI
import WaveyCore

@main
struct WaveyApp: App {
    var body: some Scene {
        WindowGroup("Wavey") {
            Text("Wavey core \(coreVersion())")
                .padding(40)
        }
    }
}
