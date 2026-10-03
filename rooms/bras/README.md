# BRAS benchmark rooms

Geometry (simplified models), fitted random-incidence absorption and
scattering, and source/receiver positions of the BRAS rooms, as used in
L. Dijkman, N. Hoekstra, M. Hornikx, "Comparing open-source room acoustics
simulation tools: performance and usability insights", Forum Acusticum 2025
(data: https://github.com/Building-acoustics-TU-Eindhoven/OSS_RoomAcoustics),
from the Benchmark for Room Acoustical Simulation (BRAS), L. Aspöck et al.,
TU Berlin, CC BY 4.0.

`wavey build rooms/bras/cr2/cr2.json projects/bras-cr2.wavey` builds the
seminar room CR2 with omnidirectional sources S1, S2 and receivers R1–R5.
`rooms/bras/cr4/cr4.json` builds the auditorium CR4 the same way.

## Reproducing the comparison

Settings used: 63 Hz–8 kHz in octaves, image sources to order 3 (CR2) /
2 (CR4), 100 000 rays, no wave solver, medium 19.5 °C / 41.7 % RH (CR2).

    wavey simulate projects/bras-cr2.wavey --backend cpu
    target.nosync/release/examples/bras_compare projects/bras-cr2.wavey \
        "<BRAS CR2>/RIRs/wav" "<OSS_RoomAcoustics>/05_Results/05.2_RAParam/05.2.1_BRAS_CR2" \
        docs/validation/bras-cr2.csv
    python3 docs/validation/plot_bras.py docs/validation/bras-cr2.csv CR2
    python3 docs/validation/plot_roundrobin.py

The measured parameters come from the BRAS dodecahedron RIRs through
`wavey_solver::analysis` (ISO 3382-1: octave filters, Lundeby noise
compensation, Schroeder integration). Errors are mean JND errors over the
bands (5 % for T20/EDT, 0.05 for D50, 1 dB for C80).

## Results (mean JND error over T20, EDT, D50, C80)

| Room | Wavey | I-Simpa | Treble | Image2Reverb | dg-acoustics | PyRoomAcoustics |
|------|-------|---------|--------|--------------|--------------|-----------------|
| CR2  | **1.10** | 1.77 | 2.10 | 2.73 | 3.89 | 5.03 |
| CR4  | **3.15** | 3.75 | 3.47 | 3.35 | – | 7.56 |

Against the 2019 round robin (participants A–F: BRASS, EASE, ODEON, RAVEN,
RAZR, ODEON; values read off Aspöck 2020, Figs. 4.12/4.18, EDT and T20,
125 Hz–4 kHz) Wavey has the lowest error in CR2 (1.36 JND; best participant
1.82) and in CR4 with matched absorption areas (1.37; best participant
1.87). The participants used the round-robin input data, so this is
indicative rather than like-for-like.

### CR4: simplified model vs. fitted coefficients

The BRAS coefficients were fitted (Eyring) for the detailed model,
S = 5851 m² (`mat_CR4.txt`). The simplified OBJ used by the comparison has
4554 m²: 1062 instead of 1774 m² concrete and 1099 instead of 1655 m² white
panels. With the same α it lacks ~20 % of the absorption, which is why every
tool overestimates CR4's reverberation (Eyring of the inputs is already
10–25 % above the measurement). `cr4/cr4-area-matched.json` scales each
material's α by its area ratio so S·α matches the fit (`absorption_scale`);
Wavey then reaches 1.40 JND (`docs/validation/bras-cr4-area-*.png`). The
headline numbers above use the inputs unchanged.

### What moved the numbers

- Exact early energy: direct sound and specular reflections up to the
  image-source order come from the image sources; rays fill in only after
  scattering or beyond that order.
- Finite-panel scattering (Rindel 1986; Christensen & Rindel 2005, as in
  ODEON): a panel of size w × l reflects specularly only above
  f_g = c·a*/(2 d² cosθ) per dimension; below it the energy is scattered.
  This fixed the low-band overestimate (CR2 125/250 Hz now within the JND).
- One octave band per ray, so each band's specular/diffuse choice uses its
  own scattering without per-band weights.

## Wave solver alone (Li et al. 2022)

The numbers above come from the hybrid's echograms; the wave solver only
contributes frequency responses there. `bras_wave` runs the FDTD grid by
itself, records the impulse response at every source–receiver pair and
analyses it like the measured RIRs, so the wave solver is tested against
the same measurement:

    target.nosync/release/examples/bras_wave projects/bras-cr2.wavey \
        "<BRAS CR2>/RIRs/wav" 700 2.5
    python3 docs/validation/plot_li2022.py

Y. Li, J. Meyer, T. Lokki et al., "Benchmarking of finite-difference
time-domain method and fast multipole boundary element method for room
acoustics", Appl. Acoust. 191 (2022) 108662, ran FDTD and FMBEM on CR2
with the same fitted BRAS materials (their scenario e, Table 1). Mean JND
error over T20, EDT, C80 and D50 at 63–250 Hz, each against its own
measurement analysis (`docs/validation/li2022-cr2.png`):

| FMBEM (Li) | FDTD (Li) | Wavey wave solver | Wavey hybrid |
|-----------:|----------:|------------------:|-------------:|
| 4.40 | 3.88 | 2.52 | 1.36 |

All wave solvers overestimate the reverberation at 125 Hz with these
inputs (Li et al. attribute it to the material data). Wavey's FDTD uses
frequency-dependent wall admittances fitted to α(f) in one run, where Li
et al.'s FDTD needed one run per third-octave band. The 1 m cube of their
Fig. 5 is `rigid_cube_modes_to_800_hz` in `core/wavey-solver/tests/fdtd.rs`:
at the default 15 points per wavelength every analytic mode up to 800 Hz
is within 0.35 % (the scheme's predicted axial dispersion).
