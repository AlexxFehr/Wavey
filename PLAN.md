# Wavey — Project Plan

Wavey is a Mac app for simulating the sound field that loudspeakers produce in rooms. It uses a CAD-like workflow: draw the room in 2D, extrude it into 3D, assign materials, place speakers, simulate, then inspect the results.

- Repo: https://github.com/AlexxFehr/Wavey
- Status: milestones 0–7 implemented (see §14 for status and follow-ups)
- Target: macOS first (Apple Silicon); the solver is built to run on other GPUs (NVIDIA/AMD, Linux/Windows, cloud) later.

---

## 1. Goals

### 1.1 Product goals
1. **Sketch.** Draw a 2D plan of the room's walls and boundaries with snapping and numeric input.
2. **Extrude.** Extrude the sketch lines straight up into walls, and add a flat floor and a flat ceiling. Closed loops drawn inside a room become pillars that run from floor to ceiling.
3. **Select interiors.** Click the closed regions formed by the lines to choose which ones are simulated.
4. **Materials.** Assign a material to each surface. Acoustic parameters (absorption, scattering, optionally complex impedance) can be a single scalar or values over frequency.
5. **Sources.** Place loudspeakers built from data files containing complex (magnitude + phase) response, directivity over frequency, sensitivity, max SPL and so on. Set position, aiming, gain, delay and polarity for each one.
6. **Simulate.** Choose the frequency range and resolution, then compute the full sound field including reflections, with the phase information intact.
7. **Post-process.**
   - Cut planes: SPL maps on horizontal or vertical slices, per frequency or per band.
   - Microphones: SPL, magnitude and phase response at chosen points.
   - Exports: CSV and images.

### 1.2 Technical goals
- **Full audio band, 20 Hz to 20 kHz, up to venue scale**, using a hybrid solver.
- **Portable GPU compute.** The solver is written once (Rust + `wgpu`/WGSL). It runs on Metal today and on Vulkan/DX12 later. A headless CLI runs the same projects on GPU servers.
- **Verifiable physics.** A CPU reference implementation exists for every GPU kernel. It is tested against analytic solutions, and the GPU kernels are tested against it for parity.

### 1.3 Non-goals (for now)
- Sloped or stepped floors and ceilings, balconies, and non-vertical walls. The data model should allow them later.
- Curved surfaces other than polyline approximations.
- Auralization (convolution playback). Possible later, because the solver produces impulse responses.
- GLL import. GLL is a proprietary, encrypted format.

---

## 2. Decisions

| Topic | Decision | Reasoning |
|---|---|---|
| Solver | Hybrid, phased: coherent ISM → ray tracing → FDTD | Standard practice: no single method covers 20 Hz to 20 kHz in a venue |
| Core language | Rust | Fast, safe, portable; good GPU (`wgpu`) and FFI (UniFFI) tooling |
| GPU API | `wgpu` + WGSL compute shaders | Metal now; Vulkan/DX12 later from the same shaders |
| Mac UI | SwiftUI (+ RealityKit for 3D, Swift Charts for plots) | Native Mac UX |
| Bridge | UniFFI (generates Swift bindings) | Generated, type-safe API with no hand-written C |
| Speaker data | Own open format (JSON + binary arrays) + CLF importer | Open and documented; can carry complex directivity |
| Units | SI throughout; right-handed coordinates, **z up**, meters | |

---

## 3. User workflow

```
 Sketch ──► Model (3D) ──► Materials ──► Sources ──► Simulate ──► Results
   2D lines   extrude,       pick surface   place/aim    freq range,   cut planes,
   snapping   pick interior  assign mat.    speakers     resolution    microphones
```

1. **Sketch mode (2D).** Tools: line, polyline, rectangle, circle/polygon (for pillars), select/move, dimension entry. The view supports pan and zoom and has a grid. Snapping works to the grid, endpoints, midpoints, intersections, and perpendicular/parallel directions.
2. **Model mode (3D).** The user sets the ceiling height. They click regions to mark them as interior, and Wavey extrudes them. The 3D view can orbit, pan, zoom, and show sections.
3. **Materials mode.** Click a surface (or multi-select) and assign a material from the library. A material editor offers scalar entry or a frequency table, with a graph.
4. **Sources mode.** Import or select a speaker model, then place it with an xyz position, an aim (yaw/pitch/roll), gain, delay and polarity. An optional directivity balloon can be shown in the viewport.
5. **Simulate mode.** Settings: f_min, f_max, resolution (1/N octave or linear Δf), cut-plane grid spacing, ISM order, ray count, FDTD crossover, air conditions (temperature, humidity, pressure). Shows a cost estimate before the run and progress during it, and the run can be cancelled.
6. **Results mode.** Add or move cut planes and pick a frequency or band. Choose a colormap and dB range. Add microphones and view their plots. Export results.

Undo/redo covers every edit. It is implemented in the Rust document model as a command log, so the app and the CLI share the same model.

---

## 4. Architecture

```
Wavey/
  Cargo.toml                 workspace
  .cargo/config.toml         target-dir = "target.nosync"  (repo lives in iCloud Drive)
  core/
    wavey-geom/              sketch model, planar-graph regions, extrusion, triangulation, BVH
    wavey-materials/         materials, band interpolation, reflection coefficients, air absorption
    wavey-speaker/           speaker format, loader, CLF import, directivity interpolation, aiming
    wavey-solver/            frequency grid, receivers, ISM, ray tracer, FDTD, combiner
      backend/cpu            reference implementation (rayon, f64)
      backend/gpu            wgpu + WGSL kernels (f32)
    wavey-project/           .wavey document package, schema versioning, undo/redo command log
    wavey-ffi/               UniFFI exports → Swift bindings + static lib (XCFramework)
    wavey-cli/               `wavey simulate project.wavey --backend gpu|cpu`
  app/                       SwiftUI app (Swift package; opens in Xcode)
    Sketch/  Viewport3D/  Inspector/  Results/
  docs/
    speaker-format.md
  scripts/
    build-core.sh            builds Rust staticlib + generates Swift bindings
```

**Core principle: all model state, geometry and physics live in Rust.** Swift handles UI and rendering only. It sends commands ("add segment", "assign material", "run simulation") and gets back state snapshots, meshes and result buffers. The CLI and any future frontend therefore behave exactly like the app.

**Compute backend abstraction.**
```rust
trait ComputeBackend {
    fn ism_accumulate(&self, job: &IsmJob) -> Result<ComplexField>;
    fn trace_rays(&self, job: &RayJob) -> Result<EnergyHistograms>;
    fn fdtd_run(&self, job: &FdtdJob, progress: &dyn Progress) -> Result<ReceiverSignals>;
}
```
There are two implementations: `CpuBackend` (f64, rayon; the reference and the fallback) and `GpuBackend` (wgpu). A CUDA backend can be added later behind the same trait, but only if profiling on NVIDIA shows that wgpu/Vulkan falls short.

---

## 5. Geometry (`wavey-geom`)

### 5.1 Sketch model
- **Vertices**: 2D points with stable IDs.
- **Segments**: vertex pairs. Arcs and circles are stored as parameters and tessellated into segments, with a user-set tolerance.
- Intersections are resolved when a segment is added (both segments are split at the crossing), so the sketch is always a valid **planar graph**.

### 5.2 Regions
- Faces are found in the planar graph with a half-edge walk: take the next edge clockwise at each vertex, which gives every bounded face.
- Each face is classified by **nesting**. A face contained inside an *interior* region whose own boundary is closed is a **hole**. A hole that is not itself marked interior becomes a **pillar**.
- The user marks faces as interior. The simulated domain is the union of the marked faces.

### 5.3 Extrusion → 3D mesh
- Each boundary segment of an interior face becomes a **wall quad** from z=0 up to z=H. Walls of interior holes (pillars) face into the room.
- **Floor** and **ceiling** are the face polygon with its holes, triangulated with `earcutr`.
- Every planar face gets a stable **SurfaceId**, derived from the segment ID plus its role (wall/floor/ceiling). Material assignments are keyed by SurfaceId, so they survive re-extrusion.
- Segments shared between two interior faces are internal: no wall is generated, and sound passes through. A later option will be a "partition wall" with transmission loss.
- Output: a watertight triangle mesh with per-triangle SurfaceId, outward normals, and a **plane list** (one plane per planar surface, used by ISM).

### 5.4 Spatial acceleration
- A BVH (SAH split) over the triangles, used for ray–mesh intersection and segment visibility.
- A flattened BVH (node array + triangle array, SoA) is uploaded to the GPU for ray tracing and ISM visibility.

---

## 6. Materials (`wavey-materials`)

### 6.1 Material definition
```json
{
  "name": "Painted concrete",
  "absorption": { "bands_hz": [125,250,500,1000,2000,4000], "values": [0.01,0.01,0.02,0.02,0.02,0.03] },
  "scattering": 0.05,
  "impedance": null
}
```
- `absorption` / `scattering`: either a scalar or a band table (1/1 or 1/3 octave, any frequencies).
- `impedance` (optional): normalized complex surface impedance ζ(f) = Z/(ρc). When present, it gives phase-accurate, angle-dependent reflection.
- Values are interpolated to the simulation grid linearly in log-frequency and held constant outside the table range.

### 6.2 Reflection coefficient
- **From impedance** (locally reacting, plane wave):  R(θ, f) = (ζ cos θ − 1) / (ζ cos θ + 1)
- **From absorption only**: |R| = √(1 − α), with phase 0 (real, positive) and no angle dependence. This is a documented approximation. As an option, an equivalent real impedance can be fitted to the random-incidence α (Paris formula) to get angle dependence.
- **Scattering split**: a fraction s of the reflected energy is scattered diffusely and the rest is specular. The specular amplitude used by ISM is therefore √(1−s)·R. The scattered energy s·|R|² is handled by the ray tracer (see §8.4).

### 6.3 Medium
- Speed of sound c(T) = 331.3·√(1 + T/273.15) m/s; density ρ(T, p).
- **Air absorption** follows ISO 9613-1 from temperature, relative humidity and pressure, giving the attenuation m(f) in dB/m. It is applied as the amplitude factor `10^(−m·r/20)`.

### 6.4 Built-in library
Includes concrete, brick, plaster/drywall, glass, wood panel, carpet, curtain, acoustic tile, mineral wool panels (various thicknesses), audience/seating and others. Values come from published tables, with sources cited in the library file.

---

## 7. Loudspeakers (`wavey-speaker`) — format spec in `docs/speaker-format.md`

### 7.1 Native format (`.waveyspk`, a folder or a zip)
```
manifest.json
onaxis.bin        complex f32 [n_freq]
balloon.bin       complex f32 [n_freq][n_theta][n_phi], normalized to on-axis
```
```json
{
  "format": "waveyspk", "version": 1,
  "name": "Example 2-way 12\"",
  "frequencies_hz": [ ... ],
  "reference": { "distance_m": 1.0, "input": { "kind": "voltage_rms", "value": 2.83 } },
  "sensitivity_db_spl": 98.0,
  "max_input": { "kind": "voltage_rms", "value": 60.0 },
  "max_spl_db": 128.0,
  "latency_s": 0.0012,
  "acoustic_center_m": [0.0, 0.0, 0.05],
  "balloon": { "theta_deg": [0, 5, ..., 180], "phi_deg": [0, 5, ..., 355], "convention": "theta from +x (on-axis), phi around x" },
  "phase_reference": "acoustic_center",
  "enclosure": { "width_m": 0.38, "height_m": 0.62, "depth_m": 0.40 }
}
```
- Local speaker frame: **+x is on-axis**, +z is up.
- The on-axis complex response H₀(f) is pressure at the reference distance for the reference input.
- The balloon holds D(θ, φ, f) as a complex number normalized so that D(0, ·, f) = 1.

### 7.2 Evaluation
p_free(r⃗, f) = g · H₀(f) · D(θ, φ, f) · (r_ref / r) · e^{−jk(r − r_ref)} · e^{−j2πf·τ} · A_air(f, r)

- g: user gain × polarity (±1); τ: user delay plus latency; (θ, φ) are angles in the speaker's local frame after the aiming rotation.
- **Directivity interpolation**: bilinear on the sphere. Magnitude is interpolated in dB and phase is interpolated after unwrapping along the grid. Points near the poles get special handling.

### 7.3 CLF import
- CF1/CF2 text files are parsed into a balloon. Most CLF files contain magnitude only. In that case phase is set to the on-axis phase (minimum phase computed from on-axis magnitude via Hilbert transform), and the model is flagged "phase estimated" in the UI.

### 7.4 Bundled sample speakers
- Ideal omni.
- Analytic baffled piston (radius parameter).
- Synthetic 2-way with crossover phase.

These are also used as test fixtures.

---

## 8. Solver (`wavey-solver`) — how it works

### 8.1 Overview

The quantity computed is **complex sound pressure p(x⃗, f)** at each receiver and each simulated frequency, summed coherently over all speakers. A complex result keeps phase, so speaker-to-speaker interference, comb filtering and room modes all appear.

No single method works across 20 Hz to 20 kHz in a large room:

| Method | Good at | Limit |
|---|---|---|
| **ISM**, coherent image sources | Early specular reflections with exact phase; any frequency | Image count grows exponentially with order; no diffraction; no diffuse tail |
| **Ray tracing**, stochastic energy | Late diffuse reverberation, scattering, decay | Energy only (no phase) |
| **FDTD**, wave-based, on GPU | Low frequencies: modes, diffraction, exact interference | Cost ∝ f⁴; practical up to roughly 0.5–1 kHz in venues |

The hybrid result is:

```
            f < f_c − ½oct        crossover band            f > f_c + ½oct
 p(f) =   p_FDTD(f)          w(f)·p_FDTD + (1−w)·p_GA     p_GA(f)

 p_GA: coherent ISM up to transition order/time, + late diffuse energy from ray tracing
```

### 8.2 Inputs, grid and receivers
- **Frequency grid**: log spacing at 1/N octave (N = 1…48) or linear Δf, between f_min and f_max.
- **Receivers**: microphones (single points) and cut planes (regular grids of points with user spacing). Receivers outside the interior or inside pillars are masked.
- **Spatial sampling warning.** At 20 kHz the interference pattern varies over λ/2 ≈ 8.6 mm, which a venue-sized grid cannot resolve. Cut planes therefore default to a **band-averaged** display (§8.6). The UI warns when the spacing is coarser than λ/2 at the displayed frequency.
- **Cost estimator.** Before a run, Wavey estimates the image count, receiver × frequency count, FDTD cell count and memory, and shows an expected runtime.

### 8.3 Stage 1 — Image Source Method (coherent)

**Image generation (CPU, f64)**
1. Start from each speaker position S. For every planar surface P that faces S, mirror S across the plane to get image S₁ and record its parent and the surface.
2. Repeat recursively up to the maximum order N_ism, or until the image's minimum possible path length exceeds c·t_transition.
3. **Prune** an image when:
   - its parent lies behind the reflecting plane (invalid geometry);
   - the reflecting polygon is not visible from the parent's aperture (beam/frustum test);
   - the image's best-case amplitude, given the product of max |R| and distance, falls below a threshold such as −60 dB relative to the direct sound.

**Path validation (per image × receiver; GPU)**

An image contributes at receiver X only if the reflection path is valid:
1. Back-trace the path. Intersect segment X→Sₙ with surface Pₙ. The hit point must lie inside the polygon, not just on its infinite plane.
2. Continue toward Sₙ₋₁ and repeat the test for each surface down to the source.
3. Check every path leg for **occlusion** against the BVH. Pillars and re-entrant walls block paths here.

On the GPU this is one thread per (image, receiver) pair. Each thread writes a validity bit and the reflection angles of each bounce, which angle-dependent R needs.

**Coherent accumulation (GPU)**

For each valid (image i, receiver X, frequency f):

p_i(X, f) = g · H₀(f) · D(θᵢ, φᵢ, f) · ∏ₘ R_m(θ_m, f) · √(1 − s_m) · (r_ref / rᵢ) · e^{−j k rᵢ} · A_air(f, rᵢ) · e^{−j2πfτ}

- (θᵢ, φᵢ) is the **departure direction from the real speaker**, i.e. the first leg of the unfolded path, expressed in speaker coordinates.
- The kernel layout puts receivers on the X dimension and frequencies on Y, and loops over images. Directivity is pre-sampled per image (departure direction × frequency) on the CPU, so the kernel does no balloon lookups.
- **fp32 safety.** WGSL has no f64, and k·r reaches about 3.7·10⁴ rad at 20 kHz over 100 m. The kernel computes the phase as `2π·fract(r·f/c)`, reducing in cycles before multiplying by 2π, which keeps phase error below 10⁻³ rad. Distances are stored relative to a local origin.

**Multi-speaker summation**

The solver stores **each speaker's transfer function separately** for microphones, and optionally for cut planes when memory allows. The total is p = Σ_s p_s. This means that changing a speaker's gain, delay or polarity after a run re-mixes instantly, with no re-simulation.

### 8.4 Stage 2 — Stochastic ray tracing (late diffuse field)
- Emit N rays per speaker (for example 10⁴–10⁶), distributed according to the speaker's per-band directivity energy. Each ray carries a per-band energy vector (1/1 or 1/3 octave bands).
- At each hit:
  - multiply energy by (1 − α);
  - choose a specular direction with probability (1 − s), otherwise a Lambert-distributed direction;
  - apply air absorption along the leg.
- **Receivers** collect energy in time-bin histograms E(t, band). The collector is a sphere detector of radius ~ √(V/(πN))·k for microphones, and a grid-cell volume detector for cut planes. Diffuse rain is added in later versions.
- Only energy arriving **after the transition time t_tr** is kept, which avoids double-counting the ISM. Phase 1 uses specular-only reflection up to order N_ism, so the ISM covers t < t_tr and t_tr ≈ the arrival time of the N_ism-order images.
- **Combination with ISM.** The late part has no meaningful phase. Mean-square pressures add:
  |p_GA(f)|² = |p_ISM(f)|² + E_late(band(f)) · (ρc · normalization)
  Phase is taken from p_ISM. Speakers sum incoherently for the late part, since late fields from different speakers are uncorrelated.
- **By-products**: energy-time curves, RT60 (T20/T30), EDT, C50/C80, D50. Room-acoustics metrics come almost for free.

### 8.5 Stage 3 — FDTD (low frequencies, wave-based, GPU)

**Scheme**
- 3D pressure-only leapfrog on a cubic voxel grid (the standard rectilinear "SRL" scheme):
  p^{n+1} = 2pⁿ − p^{n−1} + λ² ∇²pⁿ, with Courant number λ = c·Δt/Δx ≤ 1/√3.
- Grid spacing Δx ≈ λ_min/10 at f_max,FDTD to keep dispersion error acceptable. An interpolated wideband scheme (IWB) can be used later to allow coarser grids.
- Geometry is voxelized from the mesh. Each boundary cell carries the SurfaceId of its surface, and from it the material.

**Boundaries**
- Frequency-dependent, locally reacting impedance boundaries. Each material's α(f) (or ζ(f)) is fitted to a passive impedance built from a small number of mass–spring–damper branches (a vector-fitting style fit). These are updated with one auxiliary state per branch per boundary cell, following the Bilbao / Kowalczyk–van Walstijn formulation. Passivity of the fit guarantees stability.

**Source and receivers**
- A soft source injects a band-limited pulse, such as a differentiated Gaussian with energy up to f_max,FDTD, at the speaker's acoustic center.
- Receivers record pⁿ time series. An FFT gives P(f). Dividing by the source spectrum gives the transfer function, which is then multiplied by the speaker's H₀(f) and gain/delay.
- **Directivity.** Below a few hundred Hz most speakers are nearly omnidirectional, so phase 3 uses an omni source with on-axis response. A low-order spherical-harmonic multipole source (dipole/cardioid for subwoofer arrays) can be added later.
- Cut planes are sampled from the field each step and accumulated with a running DFT at the requested frequencies, so no full time series is stored.

**Run length and cost.** Simulation time ≈ min(RT60, user cap). Rough figures on an M1 Pro:

| Venue 40×30×15 m | Δx | Cells | Memory | Steps (2 s) | Est. time |
|---|---|---|---|---|---|
| f_max 250 Hz | 0.137 m | ~7 M | ~60 MB | ~8.7 k | ~10 s |
| f_max 500 Hz | 0.069 m | ~56 M | ~0.5 GB | ~17 k | ~2–3 min |
| f_max 1 kHz | 0.034 m | ~450 M | ~4 GB | ~35 k | ~30–45 min |

The default crossover is set automatically from room volume and available memory, and the user can override it.

**Time-origin consistency.** FDTD and ISM must use the same time origin and the same source normalization, because the crossover blends their complex values. This is verified with a free-field test case in which both methods must agree in magnitude and phase across the crossover band.

### 8.6 Crossover and output modes
- **Crossover**: the weight w(f) is a raised-cosine over one octave centered at f_c, applied to complex pressure.
- **Output modes**:
  - **Coherent**: the complex sum, the physically correct single-frequency field.
  - **Band-averaged**: mean-square over a 1/N-octave band around the display frequency. This is the default for cut planes at high frequency.
  - **Incoherent**: sum of |p|² over sources and images, which gives an "average" coverage map.
  - **Broadband**: SPL weighted by a source spectrum (pink noise or a user spectrum) and A/C/Z weighting, summed over the band.
- SPL = 20·log₁₀(|p_rms| / 20 µPa).

### 8.7 Diffraction (later)
ISM and ray tracing miss diffraction around pillar edges. Below f_c, FDTD covers it. Above f_c, an edge-diffraction add-on (UTD or Biot–Tolstoy–Medwin, first order) can be added for pillar edges if results show audible shadow errors.

### 8.8 Execution and data flow
```
project ─► mesh + planes + BVH ─► image tree (CPU) ─► GPU: validate paths ─► GPU: coherent accumulate ─┐
       └─► voxel grid ─► GPU: FDTD loop ─► running DFT at receivers ─────────────────────────────────┤──► combine ─► results
       └─► GPU: ray tracing ─► energy histograms ─► late energy per band ──────────────────────────────┘
```
- Work is chunked (frequency blocks × receiver tiles) so memory stays bounded, progress can be reported, and the run can be cancelled.
- The three stages are independent and can run concurrently.
- Results are written to the project package as they complete.

### 8.9 Results storage
- Microphones: complex f32, per speaker, per frequency. This is small, and the impulse response can be reconstructed by IFFT for linear grids.
- Cut planes: per-frequency dB (f16) for the combined result, plus band-averaged maps. Per-speaker complex fields are stored only if they fit within a size budget, for example 500×500 × 240 freqs × 8 B = 480 MB per speaker. Large arrays use zstd compression.

---

## 9. Mac app (SwiftUI)
- **Sketch canvas**: SwiftUI `Canvas` with gesture handling. It can move to a Metal-backed `NSView` if large sketches become slow. Snapping hints and live dimensions are drawn as an overlay.
- **3D viewport**: RealityKit (SceneKit is soft-deprecated). It shows meshes with material colors and highlights on hover and selection. Speakers are shown as boxes with an aim arrow and an optional balloon mesh. Cut planes are textured quads with colormap textures generated from result buffers.
- **Inspector** (right sidebar): properties of the current selection (surface/material, speaker, mic, plane).
- **Plots**: Swift Charts for the mic SPL/magnitude/phase over a log frequency axis, overlays for multiple mics, and the material α(f).
- **Document**: `.wavey` document package (a folder bundle containing `project.json` and `results/`), integrated with `DocumentGroup` for standard Mac open/save/versions.
- **Threading**: simulations run on Rust threads. Progress comes back through a UniFFI callback interface, and results are pulled as buffers.

---

## 10. Milestones
Each milestone ends with something runnable, and is committed and pushed.

| # | Milestone | Done when |
|---|---|---|
| 0 | **Setup** | Rust toolchain, git and remote, workspace skeleton, UniFFI → Swift "hello" running in a SwiftUI window, `target.nosync` outside iCloud sync |
| 1 | **Geometry** | Sketch a room with pillars, select interior, extrude, view in 3D, save/load `.wavey` |
| 2 | **Materials** | Library + editor (scalar/band table), assign by clicking surfaces |
| 3 | **Speakers** | Format spec, loader, CLF import, sample speakers, place/aim, balloon preview |
| 4 | **ISM solver** | CPU reference → GPU kernels; sim settings; cut planes + microphones. **First end-to-end usable version** |
| 5 | **Ray tracing + hybrid** | Late tail, combined output modes, RT60/C80 metrics |
| 6 | **FDTD** | Low-frequency wave solver on GPU, impedance boundaries, crossover blending |
| 7 | **Headless/portable** | CLI validated on Linux + NVIDIA (Vulkan); CUDA backend only if profiling justifies it |

---

## 11. Verification

**Analytic tests (CPU reference, `cargo test`)**
- Free field: 1/r decay and e^{−jkr} phase; speaker evaluation against the analytic piston directivity.
- Rigid shoebox: ISM steady-state field compared with the modal sum solution, and FDTD mode frequencies within 0.5% of (c/2)·√((nx/Lx)²+(ny/Ly)²+(nz/Lz)²).
- Single reflecting plane: ISM against the analytic two-path interference (comb filter notches).
- Diffuse shoebox: ray-traced RT60 compared with Sabine/Eyring.
- Geometry: region detection with nested holes, watertight extrusion, outward normals, stable SurfaceIds after edits.
- Materials: impedance → R(θ) → α round-trip; ISO 9613-1 reference values.

**Parity tests.** Every GPU kernel is compared with the CPU reference within tolerance, with phase error < 10⁻³ rad at 20 kHz / 100 m.

**Cross-method tests.** ISM and FDTD must agree within ±1 dB / ±10° in the crossover band for a simple room.

**Benchmarks.** `criterion` benchmarks track ISM throughput (paths/s) and FDTD throughput (cells/s) on the M1 Pro.

**End-to-end.**
1. Sketch a rectangle with a pillar.
2. Extrude it, assign materials, and place two speakers.
3. Simulate 20 Hz–20 kHz at 1/12 octave.
4. Check the cut planes and mic responses.
5. Run `wavey simulate` on the same project and get identical results.

---

## 12. Risks and open questions
- **ISM image explosion** in complex rooms. Mitigated by beam pruning, the amplitude threshold, and a time-based transition to ray tracing.
- **FDTD memory** at high crossover frequencies in large venues. Mitigated by the automatic crossover from available memory, and later by the IWB scheme or a multi-GPU/cloud run.
- **Impedance fitting from α only** is non-unique. Use passive, minimum-phase fits and document the assumption.
- **fp32 precision** on the GPU. Mitigated by phase reduction in cycles, local coordinates and CPU parity tests.
- **Measured phase data quality** in speaker files: the acoustic center and time reference must be consistent. The format spec defines this, and the loader validates it.
- **Open:** partition walls with transmission loss, audience areas as absorbing volumes, per-region ceiling heights, and auralization.

---

## 13. Environment notes
- Machine: Apple M1 Pro. Rust is installed via rustup. Xcode is **not** installed, so the app starts as a Swift package buildable with the Command Line Tools. Install Xcode for app bundling, signing and Instruments.
- The repo is in iCloud Drive. Build outputs go to `*.nosync` folders, which iCloud skips, and are git-ignored.

---

## 14. Status (2026-10-01)

| # | Milestone | Status |
|---|---|---|
| 0 | Setup | Done. Rust workspace, UniFFI bridge, SwiftPM app. The app builds with the Command Line Tools, and `make bundle` produces `Wavey.app`. |
| 1 | Geometry | Done. Planar sketch with automatic splitting, regions with holes and pillars, extrusion, BVH, sketch canvas, 3D viewport, `.wavey` files with undo. |
| 2 | Materials | Done. Library, editor, per-surface assignment, ISO 9613-1 air absorption, impedance fit for angle dependence. |
| 3 | Speakers | Done. `.waveyspk`, tabular polar import, synthetic samples, placement and aiming, polar plot, balloon. |
| 4 | ISM solver | Done. CPU reference plus GPU (WGSL) accumulation, cut planes, microphones, CLI. |
| 5 | Ray tracing + hybrid | Done. Late field via a voxel volume detector, room metrics. |
| 6 | FDTD | Done. GPU and CPU leapfrog, calibrated source, crossover blending. |
| 7 | Headless / portable | Partly done. The CLI and core type-check for Linux. CI runs the GPU kernels on software Vulkan. A Dockerfile exists for NVIDIA. **Not yet run on real NVIDIA hardware.** |

**Deviations from the plan**
- The 3D viewport uses SceneKit instead of RealityKit. SceneKit's view-level hit testing and unprojection suit CAD-style picking and placement. The viewport is isolated in `app/Sources/Wavey/Viewport3D`.
- CLF import is replaced by a generic tabular polar importer. No public spec of the CLF text syntax was available to test against, and CF1/CF2 are protected binaries.
- Ray tracing runs on the CPU. It is fast enough so far: 20k rays per source take about a second.

**Known limitations and follow-ups**
- FDTD boundaries are frequency-independent, with each surface's admittance fitted at the crossover. Planned: multi-branch frequency-dependent boundaries.
- The FDTD source is omnidirectional with the on-axis response. Planned: low-order multipole directivity.
- The FDTD scheme is the rectilinear one, whose axial dispersion grows with distance. The run notes report the estimate. Planned: the IWB scheme.
- There is no edge diffraction above the crossover (UTD/BTM).
- Path validation for the ISM runs on the CPU. Moving it to the GPU is the next speed-up for high reflection orders.
- A dedicated CLF parser needs sample files.
