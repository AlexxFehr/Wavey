# Pilot round robin on wave-based room acoustics: lecture hall

The "Pilot Studio Benchmark for a Round Robin Test on Room Acoustics
Wave-Based Simulations" (G. Fratoni et al., University of Bologna): a
historical lecture hall with stepped seating, about 830 m³. The dataset
ships a SketchUp 2017 model, octave-band absorption and scattering
coefficients for four materials (plaster/floor, wood, windows, seats) and
the measured EDT, T30, C50 and Ts (source S1, mean over all receivers).
The hall and a full-band FDTD and a GA model of it are described in
M. Cingolani et al., "A Trial Acoustic Improvement in a Lecture Hall with
MPP Sound Absorbers and FDTD Acoustic Simulations", Appl. Sci. 11 (2021)
2445; its Table 6 gives their T30 ("without MPP").

`classroom.json` builds the room with S1 and R1–R26 at the positions
labelled in the SketchUp model:

    wavey build rooms/fratoni/classroom.json projects/rrt-classroom.wavey
    # settings as for BRAS: 63 Hz–8 kHz octaves, image sources to order 2,
    # 100 000 rays, no wave solver
    wavey simulate projects/rrt-classroom.wavey --backend cpu
    target.nosync/release/examples/rrt_compare projects/rrt-classroom.wavey docs/validation/rrt-classroom.csv
    python3 docs/validation/plot_rrt.py

## Geometry conversion

`classroom.obj` comes from `Classroom.skp` through `tools/skp2obj.py`
(openskp 1.3.0, ear-clipped faces in metres, one group per layer) and
`tools/thin.py`:

- openskp stops reading the model at the first text label ("(S1 / S1)"):
  its text reader expects the byte before the string marker to be 1, this
  file has 0. With `blk[10] in (0, 1)` in `openskp/legacy.py::_read_text`
  all 1099 entities are read (276 faces; without it 13 faces, the floor
  under the side seating among them, and all labels are lost).
- The suspended ceilings and a few other elements are single faces with
  air on both sides. `thin.py` finds them (voxel flood fill from outside;
  faces with air on both sides), gives them a 2 cm thickness and orients
  every face towards the air, so the mesh is closed for ray-parity tests
  (3000 random points: 4 disagree with the flood fill, all next to faces).

## Results (mean JND error, 125 Hz – 4 kHz)

| | EDT | T30 | C50 | Ts |
|---|---:|---:|---:|---:|
| Wavey | 0.95 | 1.21 | 0.53 | 0.56 |
| FDTD (Cingolani 2021) | – | 1.26 | – | – |
| GA (Cingolani 2021) | – | 0.80 | – | – |

JND: 5 % for EDT/T30, 1 dB for C50, 10 ms for Ts. The absorption data is
the set the GA model was calibrated with, which favours that model. Wavey
is short at 125 Hz (T30 1.29 s vs 1.56 s measured; Sabine with these
inputs gives 1.26 s): absorption sits almost all in the seating on the
floor, and the measured field is less diffuse than the ray model's. The
wave solver alone (`bras_wave … - 500 2.5`) overshoots instead (T30 2.5 s
at 125 Hz, a sagging double slope; 1.34 s at 250 Hz vs 1.36 s measured).

Stepped seating at low frequencies: a staircase whose steps are small
against the wavelength reflects like the slope it forms. Small facets of
one material that share vertices are therefore clustered per octave band
(`raytrace::smooth_clusters`); when the cluster deviates from its mean
plane by less than λ/8 rms, rays reflect off that plane with the
cluster's panel size. This took the hall's T30 error from 1.44 to 1.21
(EDT 1.04 → 0.95) and left BRAS CR2 unchanged.
