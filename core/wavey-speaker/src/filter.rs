//! Per-speaker processing filters (analog prototypes evaluated at f).

use std::f64::consts::PI;

use num_complex::Complex64;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slope {
    /// Butterworth, 12 dB/octave.
    Butterworth12,
    /// Butterworth, 24 dB/octave.
    Butterworth24,
    /// Linkwitz–Riley, 24 dB/octave (the usual crossover).
    LinkwitzRiley24,
    /// Linkwitz–Riley, 48 dB/octave.
    LinkwitzRiley48,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Filter {
    HighPass { freq_hz: f64, slope: Slope },
    LowPass { freq_hz: f64, slope: Slope },
    /// Second-order low shelf: `gain_db` below `freq_hz` (the midpoint);
    /// `q` sets the transition (default 1/√2, the gentlest without overshoot).
    LowShelf {
        freq_hz: f64,
        gain_db: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        q: Option<f64>,
    },
    /// Second-order high shelf: `gain_db` above `freq_hz` (the midpoint).
    HighShelf {
        freq_hz: f64,
        gain_db: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        q: Option<f64>,
    },
    /// Peaking EQ.
    Peak { freq_hz: f64, gain_db: f64, q: f64 },
}

/// Second-order sections (as normalized Q values) of a Butterworth filter.
fn butterworth_qs(order: usize) -> Vec<f64> {
    (0..order / 2)
        .map(|k| 1.0 / (2.0 * (PI * (2 * k + 1) as f64 / (2 * order) as f64).sin()))
        .collect()
}

/// Q values per biquad section of each slope.
fn sections(slope: Slope) -> Vec<f64> {
    match slope {
        Slope::Butterworth12 => butterworth_qs(2),
        Slope::Butterworth24 => butterworth_qs(4),
        // Linkwitz–Riley = Butterworth squared.
        Slope::LinkwitzRiley24 => [butterworth_qs(2), butterworth_qs(2)].concat(),
        Slope::LinkwitzRiley48 => [butterworth_qs(4), butterworth_qs(4)].concat(),
    }
}

impl Filter {
    pub fn response(&self, f: f64) -> Complex64 {
        let one = Complex64::new(1.0, 0.0);
        match *self {
            Filter::HighPass { freq_hz, slope } => {
                let s = Complex64::new(0.0, f / freq_hz);
                sections(slope).iter().map(|q| s * s / (s * s + s / q + one)).product()
            }
            Filter::LowPass { freq_hz, slope } => {
                let s = Complex64::new(0.0, f / freq_hz);
                sections(slope).iter().map(|q| one / (s * s + s / q + one)).product()
            }
            Filter::LowShelf { freq_hz, gain_db, q } => {
                // RBJ analog prototype (default Q = 1/√2, i.e. S = 1).
                let a = 10f64.powf(gain_db / 40.0);
                let s = Complex64::new(0.0, f / freq_hz);
                let q = q.unwrap_or(std::f64::consts::FRAC_1_SQRT_2).max(0.1);
                a * (s * s + a.sqrt() / q * s + a) / (a * s * s + a.sqrt() / q * s + one)
            }
            Filter::HighShelf { freq_hz, gain_db, q } => {
                let a = 10f64.powf(gain_db / 40.0);
                let s = Complex64::new(0.0, f / freq_hz);
                let q = q.unwrap_or(std::f64::consts::FRAC_1_SQRT_2).max(0.1);
                a * (a * s * s + a.sqrt() / q * s + one) / (s * s + a.sqrt() / q * s + a)
            }
            Filter::Peak { freq_hz, gain_db, q } => {
                let a = 10f64.powf(gain_db / 40.0);
                let s = Complex64::new(0.0, f / freq_hz);
                let q = q.max(0.05);
                (s * s + s * (a / q) + one) / (s * s + s / (a * q) + one)
            }
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let (f, ok) = match *self {
            Filter::HighPass { freq_hz, .. } | Filter::LowPass { freq_hz, .. } => (freq_hz, true),
            Filter::LowShelf { freq_hz, gain_db, q } | Filter::HighShelf { freq_hz, gain_db, q } => {
                (freq_hz, gain_db.abs() <= 40.0 && q.is_none_or(|q| q > 0.0))
            }
            Filter::Peak { freq_hz, gain_db, q } => (freq_hz, gain_db.abs() <= 40.0 && q > 0.0),
        };
        if !(f > 1.0 && f < 40000.0) || !ok {
            return Err("filter frequency must be 1 Hz–40 kHz and gains within ±40 dB".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(c: Complex64) -> f64 {
        20.0 * c.norm().log10()
    }

    #[test]
    fn shelf_q_defaults_to_the_old_curve_and_steepens() {
        let q0 = std::f64::consts::FRAC_1_SQRT_2;
        for f in [20.0, 60.0, 200.0, 2000.0] {
            let a = Filter::LowShelf { freq_hz: 100.0, gain_db: 6.0, q: None }.response(f);
            let b = Filter::LowShelf { freq_hz: 100.0, gain_db: 6.0, q: Some(q0) }.response(f);
            assert!((a - b).norm() < 1e-12);
        }
        // Higher Q: a steeper transition (closer to full gain an octave below).
        let g = |q| db(Filter::LowShelf { freq_hz: 100.0, gain_db: 6.0, q: Some(q) }.response(50.0));
        assert!(g(1.4) > g(q0));
    }

    #[test]
    fn linkwitz_riley_crossover_sums_flat() {
        for slope in [Slope::LinkwitzRiley24, Slope::LinkwitzRiley48] {
            let hp = Filter::HighPass { freq_hz: 80.0, slope };
            let lp = Filter::LowPass { freq_hz: 80.0, slope };
            assert!((db(hp.response(80.0)) + 6.02).abs() < 0.01);
            for f in [20.0, 50.0, 80.0, 120.0, 400.0] {
                let sum = hp.response(f) + lp.response(f);
                // LR24 sums in phase; LR48 sums flat (both all-pass).
                assert!((sum.norm() - 1.0).abs() < 1e-9, "{slope:?} {f}");
            }
        }
    }

    #[test]
    fn butterworth_is_minus_3db_at_cutoff() {
        for slope in [Slope::Butterworth12, Slope::Butterworth24] {
            assert!((db(Filter::LowPass { freq_hz: 100.0, slope }.response(100.0)) + 3.01).abs() < 0.01);
        }
        // 24 dB/octave far below cutoff.
        let hp = Filter::HighPass { freq_hz: 100.0, slope: Slope::Butterworth24 };
        assert!((db(hp.response(10.0)) + 80.0).abs() < 0.1);
    }

    #[test]
    fn shelves_and_peak() {
        let ls = Filter::LowShelf { freq_hz: 60.0, gain_db: 12.0, q: None };
        assert!((db(ls.response(5.0)) - 12.0).abs() < 0.05);
        assert!(db(ls.response(2000.0)).abs() < 0.05);
        assert!((db(ls.response(60.0)) - 6.0).abs() < 0.05);
        let hs = Filter::HighShelf { freq_hz: 1000.0, gain_db: -6.0, q: None };
        assert!((db(hs.response(20000.0)) + 6.0).abs() < 0.05);
        let pk = Filter::Peak { freq_hz: 1000.0, gain_db: 4.0, q: 2.0 };
        assert!((db(pk.response(1000.0)) - 4.0).abs() < 1e-9);
        assert!(db(pk.response(100.0)).abs() < 0.1);
    }
}
