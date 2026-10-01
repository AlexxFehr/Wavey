use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Resolution {
    /// 1/N octave spacing, anchored at 1 kHz.
    FractionalOctave { n: u32 },
    /// Constant spacing in Hz, starting at f_min.
    Linear { step_hz: f64 },
}

/// Simulation frequencies between `f_min` and `f_max` (inclusive). Octave
/// grids are exact base-2 bands, so the limits have a 3 % tolerance: that
/// keeps e.g. the 19.7 Hz and 20.2 kHz bands for a 20 Hz–20 kHz range.
pub fn frequency_grid(f_min: f64, f_max: f64, resolution: Resolution) -> Vec<f64> {
    match resolution {
        Resolution::FractionalOctave { n } => {
            let n = n.max(1) as f64;
            let k0 = (n * (f_min / 1.03 / 1000.0).log2()).ceil() as i64;
            let k1 = (n * (f_max * 1.03 / 1000.0).log2()).floor() as i64;
            (k0..=k1).map(|k| 1000.0 * 2f64.powf(k as f64 / n)).collect()
        }
        Resolution::Linear { step_hz } => {
            let step = step_hz.max(1e-3);
            let count = ((f_max - f_min) / step + 1e-9).floor() as usize + 1;
            (0..count).map(|i| f_min + i as f64 * step).collect()
        }
    }
}

/// Edges of the 1/N-octave band centred on `f`.
pub fn band_edges(f: f64, fraction: f64) -> (f64, f64) {
    let h = 2f64.powf(0.5 / fraction);
    (f / h, f * h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn third_octave_grid() {
        let g = frequency_grid(20.0, 20000.0, Resolution::FractionalOctave { n: 3 });
        assert_eq!(g.len(), 31);
        assert!(g.iter().any(|f| (f - 1000.0).abs() < 1e-9));
        assert!((g[0] - 19.69).abs() < 0.01 && (g[30] - 20158.7).abs() < 0.1);
    }

    #[test]
    fn linear_grid() {
        let g = frequency_grid(20.0, 100.0, Resolution::Linear { step_hz: 10.0 });
        assert_eq!(g, vec![20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0]);
    }
}
