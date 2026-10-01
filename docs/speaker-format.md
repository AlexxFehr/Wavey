# Wavey speaker format (`.waveyspk`, version 1)

A `.waveyspk` file is a single JSON document that describes one loudspeaker. It covers the on-axis complex response, the complex directivity balloon, sensitivity reference, limits and physical size. It is open and documented on purpose, so that measurement data from any tool can be converted into it.

## Coordinate conventions

| | |
|---|---|
| Local frame | **+x = on-axis**, +y = the speaker's left (seen from behind), +z = up |
| θ (theta) | angle from +x, 0…180° |
| φ (phi) | rotation about +x, measured from +y toward +z, 0…360° |
| Direction | (cos θ, sin θ cos φ, sin θ sin φ) |

So φ = 0° is the horizontal plane toward the left, φ = 90° points up, φ = 180° is the horizontal plane toward the right, and φ = 270° points down.

## Fields

```json
{
  "format": "waveyspk",
  "version": 1,
  "id": "acme-x12",
  "name": "ACME X12",
  "manufacturer": "ACME",
  "frequencies_hz": [20.0, 22.4, ...],
  "reference": { "distance_m": 1.0, "input_voltage_rms": 2.83 },
  "on_axis": {
    "magnitude_db_spl": [ ... one per frequency ... ],
    "phase_deg":        [ ... one per frequency ... ]
  },
  "max_input_voltage_rms": 40.0,
  "max_spl_db": 128.0,
  "latency_s": 0.0012,
  "acoustic_center_m": [0.0, 0.0, 0.05],
  "enclosure": { "width_m": 0.36, "height_m": 0.6, "depth_m": 0.38 },
  "balloon": {
    "theta_deg": [0, 5, ..., 180],
    "phi_deg":   [0, 10, ..., 350],
    "magnitude_db": [ ... ],
    "phase_deg":    [ ... ]
  },
  "phase_estimated": false
}
```

| Field | Meaning |
|---|---|
| `frequencies_hz` | Positive and strictly increasing. Any spacing works; 1/6 to 1/24 octave is typical. |
| `reference` | The distance and input voltage at which `on_axis` is given. |
| `on_axis.magnitude_db_spl` | On-axis SPL (dB re 20 µPa) at the reference distance for the reference input. |
| `on_axis.phase_deg` | On-axis phase with the **propagation delay removed**, so that phase is referenced to the acoustic center. Phase may be wrapped. |
| `latency_s` | Processing or electrical delay that is *not* included in `phase_deg`. Wavey adds it as a pure delay. |
| `acoustic_center_m` | Acoustic center in the local frame, relative to the enclosure center. Speaker positions in a project refer to the enclosure center. |
| `balloon` | Complex directivity normalized to on-axis, so that D(θ = 0) = 0 dB, 0°. Arrays are flattened in the order `[frequency][theta][phi]`, with length `frequencies × θ × φ`. Omit the balloon for an omnidirectional source. The θ grid must start at 0°. The φ grid wraps around from its last value to its first. |
| `phase_estimated` | Set this when the phase was not measured. The UI then shows a warning. |

## Evaluation

Wavey evaluates pressure from one speaker at distance *r* in local direction (θ, φ) as follows:

p(f) = g · H₀(f) · D(θ, φ, f) · (r_ref / r) · e^{−j2πf r / c} · e^{−j2πf(τ_user + latency)} · A_air(f, r)

- H₀ is the on-axis complex pressure.
- D is the balloon value.
- g is the user gain multiplied by polarity.

Interpolation works as follows:

- Across frequency: linear in log-frequency.
- Across angle: bilinear on the θ/φ grid.
- Magnitude is interpolated in dB, and phase is unwrapped against the nearest sample before it is interpolated.

## Importing measurement tables

**Speaker → Import table…** reads a CSV, TSV or whitespace-separated file. It has one row per measurement point:

```
# frequency_hz, theta_deg, phi_deg, magnitude_db_spl[, phase_deg]
1000, 0, 0, 96.1, -12.0
1000, 5, 0, 96.0, -12.4
...
```

- Every (frequency, θ, φ) combination of the grid must be present.
- θ = 0 rows define the on-axis response.
- Phase is optional. Without it, the model is flagged `phase_estimated` and uses zero phase.

Data exported from CLF tools or measurement software can be converted into this table. GLL files are proprietary and encrypted, and cannot be imported.
