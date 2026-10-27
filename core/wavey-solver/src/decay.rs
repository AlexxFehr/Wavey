//! Reverberation time (EDT, T20, T30) from a complex transfer function.
//!
//! A band of the transfer function sampled on a uniform frequency grid has
//! an impulse response that repeats every `1 / step` seconds, and the
//! magnitude of its analytic form (the inverse DFT of the band, whatever the
//! band's position) is the energy envelope of the band-filtered response.
//! That envelope goes through the same Schroeder integration and Lundeby
//! compensation as a measured response ([`crate::analysis`]).
//!
//! Because it uses the phase of the mixed transfer function, filters and
//! delays on the sources change the result.
//!
//! Limits: the grid must be uniform and fine enough that the window
//! (`1 / step`) holds the decay; coarser grids give NaN.

use num_complex::Complex64;

use crate::analysis::energy_params;
use crate::freq::band_edges;

/// Fewest grid points a band needs.
const MIN_POINTS: usize = 6;
/// Share of the band the grid must cover.
const MIN_COVERAGE: f64 = 0.7;
/// Most of the energy that may arrive late in the window (-35 dB).
const MAX_TAIL: f64 = 3.2e-4;
/// The time grid is at least this long.
const MIN_FFT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decay {
    pub edt: f64,
    pub t20: f64,
    pub t30: f64,
    /// Length of the window the decay was measured in (s).
    pub window_s: f64,
}

impl Decay {
    pub const NONE: Decay = Decay { edt: f64::NAN, t20: f64::NAN, t30: f64::NAN, window_s: f64::NAN };
}

/// The step of a uniform ascending grid (relative tolerance 1e-6), if it is one.
pub fn uniform_step(freqs: &[f64]) -> Option<f64> {
    if freqs.len() < 2 {
        return None;
    }
    let step = (freqs[freqs.len() - 1] - freqs[0]) / (freqs.len() - 1) as f64;
    (step > 0.0 && freqs.windows(2).all(|w| ((w[1] - w[0]) - step).abs() <= 1e-6 * step)).then_some(step)
}

/// Grid points inside the band (indices) and whether they cover enough of it.
pub fn band_points(freqs: &[f64], centre: f64, fraction: f64) -> Option<std::ops::Range<usize>> {
    let step = uniform_step(freqs)?;
    let (lo, hi) = band_edges(centre, fraction);
    let first = freqs.iter().position(|f| *f >= lo)?;
    let last = freqs.iter().rposition(|f| *f < hi)?;
    if last < first || last + 1 - first < MIN_POINTS {
        return None;
    }
    let covered = (freqs[last] - freqs[first] + step).min(hi - lo);
    (covered >= MIN_COVERAGE * (hi - lo)).then_some(first..last + 1)
}

/// Taper across the outer fifth of the band at each side, so the band
/// filter has no sharp edge to ring on.
fn weight(f: f64, lo: f64, hi: f64) -> f64 {
    let edge = 0.2 * (hi - lo);
    let d = (f - lo).min(hi - f);
    if d >= edge { 1.0 } else { 0.5 - 0.5 * (std::f64::consts::PI * (d / edge).max(0.0)).cos() }
}

/// In-place radix-2 FFT with positive exponent: `x[m] = Σ a[k] e^{+j2πkm/n}`.
fn fft_inverse(a: &mut [Complex64]) {
    let n = a.len();
    debug_assert!(n.is_power_of_two());
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let w = Complex64::from_polar(1.0, 2.0 * std::f64::consts::PI / len as f64);
        for chunk in a.chunks_mut(len) {
            let mut wk = Complex64::new(1.0, 0.0);
            let (lo, hi) = chunk.split_at_mut(len / 2);
            for (x, y) in lo.iter_mut().zip(hi) {
                let t = *y * wk;
                *y = *x - t;
                *x += t;
                wk *= w;
            }
        }
        len <<= 1;
    }
}

/// Decay of the band `centre` (a `1/fraction` octave) of `h`, the complex
/// transfer function on the ascending uniform grid `freqs`.
pub fn decay(freqs: &[f64], h: &[Complex64], centre: f64, fraction: f64) -> Decay {
    let (Some(step), Some(range)) = (uniform_step(freqs), band_points(freqs, centre, fraction)) else { return Decay::NONE };
    // Taper at the edges of what the grid covers: a grid that stops inside
    // the band would otherwise end it with a sharp edge.
    let (lo, hi) = band_edges(centre, fraction);
    let (lo, hi) = (lo.max(freqs[range.start] - 0.5 * step), hi.min(freqs[range.end - 1] + 0.5 * step));
    let k = range.len();
    let m = (4 * k).next_power_of_two().max(MIN_FFT);
    let mut a = vec![Complex64::new(0.0, 0.0); m];
    for (slot, i) in a.iter_mut().zip(range) {
        *slot = h[i] * weight(freqs[i], lo, hi);
    }
    fft_inverse(&mut a);
    let e: Vec<f64> = a.iter().map(|v| v.norm_sqr()).collect();
    let peak = e.iter().copied().fold(0.0, f64::max);
    if peak <= 0.0 {
        return Decay::NONE;
    }
    // Start at the direct sound (first sample within 20 dB of the peak). The
    // response repeats, so what comes before it is the tail of the last period.
    let onset = e.iter().position(|v| *v >= peak * 0.01).unwrap_or(0);
    let rotated: Vec<f64> = e[onset..].iter().chain(&e[..onset]).copied().collect();
    // The window must hold the whole decay. If energy is still arriving
    // between three quarters and nine tenths of it, the tail has wrapped
    // round onto the start and every time read from it would be wrong, so
    // none is given. (The last tenth is left out: the band filter's
    // precursor of the direct sound lies there.)
    let total: f64 = rotated.iter().sum();
    let n = rotated.len();
    let tail: f64 = rotated[n * 3 / 4..n * 9 / 10].iter().sum();
    if tail > total * MAX_TAIL {
        return Decay { window_s: 1.0 / step, ..Decay::NONE };
    }
    let fs = m as f64 * step;
    let p = energy_params(rotated, fs);
    Decay { edt: p.edt, t20: p.t20, t30: p.t30, window_s: 1.0 / step }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Transfer function of an exponentially decaying noise-like tail with
    /// reverberation time `rt`, `delay` seconds after t = 0, on a uniform grid.
    fn room(step: f64, f0: f64, n: usize, rt: f64, delay: f64) -> (Vec<f64>, Vec<Complex64>) {
        let freqs: Vec<f64> = (0..n).map(|i| f0 + i as f64 * step).collect();
        // Impulse response: pseudo-random carrier under a decaying envelope,
        // built in the time domain and transformed.
        let fs_t = 16.0 * n as f64 * step;
        let len = (fs_t / step) as usize; // one window
        let mut seed = 12345u64;
        let mut rnd = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 31) as f64) - 0.5
        };
        let ir: Vec<f64> = (0..len)
            .map(|i| {
                let t = i as f64 / fs_t;
                if t < delay { 0.0 } else { rnd() * (-(t - delay) * 6.9078 / rt).exp() }
            })
            .collect();
        let h = freqs
            .iter()
            .map(|f| ir.iter().enumerate().map(|(i, x)| Complex64::from_polar(*x, -2.0 * std::f64::consts::PI * f * i as f64 / fs_t)).sum())
            .collect();
        (freqs, h)
    }

    #[test]
    fn a_decaying_response_gives_its_reverberation_time() {
        // 0.25 Hz step: a 4 s window holds a 1 s decay.
        let (freqs, h) = room(0.25, 40.0, 160, 1.0, 0.02);
        let d = decay(&freqs, &h, 63.0, 1.0);
        assert!((d.window_s - 4.0).abs() < 1e-9);
        assert!((d.t30 - 1.0).abs() < 0.25, "T30 {}", d.t30);
        assert!((d.t20 - 1.0).abs() < 0.25, "T20 {}", d.t20);
    }

    #[test]
    fn a_longer_decay_gives_a_longer_time() {
        let (f, short) = room(0.25, 40.0, 160, 0.5, 0.02);
        let (_, long) = room(0.25, 40.0, 160, 1.5, 0.02);
        assert!(decay(&f, &long, 63.0, 1.0).t30 > decay(&f, &short, 63.0, 1.0).t30 * 1.8);
    }

    #[test]
    fn a_delay_does_not_change_the_time() {
        let (f, a) = room(0.25, 40.0, 160, 1.0, 0.02);
        let (_, b) = room(0.25, 40.0, 160, 1.0, 0.5);
        let (da, db) = (decay(&f, &a, 63.0, 1.0), decay(&f, &b, 63.0, 1.0));
        assert!((da.t30 - db.t30).abs() < 0.25, "{} vs {}", da.t30, db.t30);
    }

    #[test]
    fn gives_nothing_where_the_grid_cannot_say() {
        let (f, h) = room(0.25, 40.0, 160, 1.0, 0.02);
        // Octave band with too little coverage: 1 kHz is not on the grid.
        assert!(decay(&f, &h, 1000.0, 1.0).t30.is_nan());
        // A grid that is not uniform.
        let mut g = f.clone();
        g[10] += 0.05;
        assert!(decay(&g, &h, 63.0, 1.0).t30.is_nan());
        // Too coarse to hold the decay: 1 Hz step is a 1 s window.
        let (f1, h1) = room(1.0, 40.0, 40, 3.0, 0.02);
        let d = decay(&f1, &h1, 63.0, 1.0);
        assert!(d.t30.is_nan() && d.t20.is_nan() && d.edt.is_nan());
        assert_eq!(d.window_s, 1.0);
    }

    #[test]
    fn the_fft_matches_the_definition() {
        let a: Vec<Complex64> = (0..8).map(|i| Complex64::new((i * i) as f64 % 5.0, i as f64 * 0.3)).collect();
        let mut b = a.clone();
        fft_inverse(&mut b);
        for m in 0..8 {
            let want: Complex64 = a.iter().enumerate().map(|(k, v)| v * Complex64::from_polar(1.0, 2.0 * std::f64::consts::PI * (k * m) as f64 / 8.0)).sum();
            assert!((b[m] - want).norm() < 1e-9, "{m}");
        }
    }
}


