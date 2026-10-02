// swift-tools-version: 6.0
import PackageDescription

// Run `scripts/build-core.sh` first: it builds the Rust core and generates
// the Swift bindings in Sources/WaveyCore and Sources/WaveyCoreFFI.
//
// Module layers (lower may not import higher):
//   WaveyDesign            tokens, fonts, icons, base controls
//   WaveyModel             UI value types and the adapter over WaveyCore
//   WaveyPlots, WaveyViewport
//   WaveyFeatures          shell, workspaces, inspectors, tools
//   Wavey                  the app
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
        .target(
            name: "WaveyDesign",
            path: "Sources/WaveyDesign",
            resources: [.copy("Resources/Fonts")]
        ),
        .target(
            name: "WaveyModel",
            dependencies: ["WaveyCore"],
            path: "Sources/WaveyModel"
        ),
        .target(
            name: "WaveyViewport",
            path: "Sources/WaveyViewport"
        ),
        .target(
            name: "WaveyPlots",
            dependencies: ["WaveyDesign"],
            path: "Sources/WaveyPlots"
        ),
        .target(
            name: "WaveyFeatures",
            dependencies: ["WaveyCore", "WaveyDesign", "WaveyModel", "WaveyViewport", "WaveyPlots"],
            path: "Sources/WaveyFeatures"
        ),
        .executableTarget(
            name: "Wavey",
            dependencies: ["WaveyCore", "WaveyDesign", "WaveyModel", "WaveyViewport", "WaveyFeatures"],
            path: "Sources/Wavey"
        ),
        .testTarget(
            name: "WaveyDesignTests",
            dependencies: ["WaveyDesign"],
            path: "Tests/WaveyDesignTests"
        ),
    ]
)
