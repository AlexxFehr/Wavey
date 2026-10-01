# Wavey

Acoustic simulation of loudspeakers in rooms: sketch the room in 2D, extrude it to 3D, assign materials, place speakers, simulate the sound field (hybrid image-source / ray tracing / FDTD), and inspect SPL cut planes and microphone responses.

See [PLAN.md](PLAN.md) for goals, design and solver details.

## Build

Requirements: Rust (rustup), Swift 6 (Xcode or Command Line Tools), macOS 15+.

```sh
make test   # Rust tests
make run    # build core + bindings + SwiftUI app, then launch
make cli    # headless `wavey` CLI
```

Build output goes to `target.nosync/` (iCloud Drive ignores `*.nosync`).
