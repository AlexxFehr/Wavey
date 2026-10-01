# Wavey

Wavey simulates the sound field that loudspeakers produce in rooms. The workflow:

1. Sketch the room in 2D.
2. Extrude it into 3D, with pillars.
3. Assign frequency-dependent materials.
4. Place speakers from complex magnitude/phase and directivity data.
5. Simulate the full field with reflections.
6. Inspect SPL cut planes, microphone responses and room-acoustic metrics.

See [PLAN.md](PLAN.md) for the full design and solver description, and [docs/speaker-format.md](docs/speaker-format.md) for the open speaker format.

## Solver

The solver is hybrid. Each stage is validated against analytic results in `core/wavey-solver/tests`.

| Stage | Covers | Runs on |
|---|---|---|
| Coherent image sources | early specular reflections, all frequencies, with phase | GPU (wgpu) or CPU |
| Stochastic ray tracing | late diffuse field, T20/T30/EDT/C80/D50 | CPU (rayon) |
| FDTD wave solver | low frequencies (modes, diffraction), below a crossover | GPU (wgpu) or CPU |

Results are stored per speaker without gain, delay or polarity. Changing those settings re-mixes instantly, without re-simulating.

## Build

Requirements: Rust (rustup), Swift 6 (Xcode or the Command Line Tools), macOS 15+.

```sh
make test     # all Rust tests (release mode)
make run      # build core + bindings + app, then launch
make bundle   # target.nosync/Wavey.app (ad-hoc signed)
make cli      # headless `wavey` CLI
```

Build output goes to `target.nosync/`. iCloud Drive ignores `*.nosync` folders.

## Headless / GPU servers

```sh
wavey demo hall.wavey                       # sample project
wavey simulate hall.wavey --backend gpu     # results are saved into the project
wavey simulate hall.wavey --fdtd 250        # with the wave solver below 250 Hz
```

The solver uses wgpu: Metal on macOS, and Vulkan or DX12 elsewhere. `docker/Dockerfile.gpu` packages the CLI for NVIDIA GPUs. CI runs the GPU kernels on Linux through Mesa's software Vulkan driver.
