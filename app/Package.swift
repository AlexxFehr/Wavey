// swift-tools-version: 6.0
import PackageDescription

// Run `scripts/build-core.sh` first: it builds the Rust core and generates
// the Swift bindings in Sources/WaveyCore and Sources/WaveyCoreFFI.
let package = Package(
    name: "Wavey",
    platforms: [.macOS(.v15)],
    products: [
        .executable(name: "Wavey", targets: ["Wavey"]),
    ],
    targets: [
        .systemLibrary(name: "wavey_ffiFFI", path: "Sources/WaveyCoreFFI"),
        .target(
            name: "WaveyCore",
            dependencies: ["wavey_ffiFFI"],
            path: "Sources/WaveyCore",
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [
                .unsafeFlags(["-L../target.nosync/lib"]),
                .linkedLibrary("wavey_ffi"),
            ]
        ),
        .executableTarget(
            name: "Wavey",
            dependencies: ["WaveyCore"],
            path: "Sources/Wavey"
        ),
    ]
)
