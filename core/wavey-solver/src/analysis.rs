//! ISO 3382-1 room-acoustic parameters from impulse responses: octave-band
//! filtering, Schroeder backward integration with Lundeby noise
//! compensation, EDT, T20, T30, C80, D50 and centre time.
//!
//! Used for measured responses (validation) and simulated ones alike, so
//! both go through the same processing.

use std::f64::consts::PI;

/// Octave band centres 63 Hz – 8 kHz.
pub const OCTAVES: [f64; 8] = [63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0];

/// Parameters for one band (NaN where undefined).
#[derive(Debug, Clone, Copy)]
pub struct BandParams {
    pub edt: f64,
    pub t20: f64,
    pub t30: f64,
    pub c80: f64,
    pub d50: f64,
    pub ts: f64,
}

/// Reads a mono (first channel) WAV file: PCM 16/24/32-bit or float 32/64.
pub fn read_wav(path: &std::path::Path) -> Result<(Vec<f64>, f64), String> {
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let (mut fmt, mut rate, mut ch, mut bits) = (0u16, 0u32, 1u16, 16u16);
    let mut i = 12;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32_at(i + 4) as usize;
        let body = i + 8;
        if id == b"fmt " {
            fmt = u16_at(body);
            ch = u16_at(body + 2);
            rate = u32_at(body + 4);
            bits = u16_at(body + 14);
            if fmt == 0xFFFE && len >= 26 {
                fmt = u16_at(body + 24); // WAVE_FORMAT_EXTENSIBLE sub-format
            }
        } else if id == b"data" {
            let data = &b[body..(body + len).min(b.len())];
            let bps = (bits / 8) as usize;
            let frame = bps * ch as usize;
            let n = data.len() / frame;
            let mut out = Vec::with_capacity(n);
            for k in 0..n {
                let s = &data[k * frame..k * frame + bps];
                let v = match (fmt, bits) {
                    (1, 16) => i16::from_le_bytes([s[0], s[1]]) as f64 / 32768.0,
                    (1, 24) => ((i32::from_le_bytes([0, s[0], s[1], s[2]])) >> 8) as f64 / 8_388_608.0,
                    (1, 32) => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64 / 2_147_483_648.0,
                    (3, 32) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64,
                    (3, 64) => f64::from_le_bytes(s[0..8].try_into().unwrap()),
                    _ => return Err(format!("unsupported WAV format {fmt}/{bits} bit")),
                };
                out.push(v);
            }
            return Ok((out, rate as f64));
        }
        i = body + len + (len & 1);
    }
    Err("no data chunk".into())
}

/// Impulse response at sample rate `fs` from a one-sided complex spectrum
/// `h` on the uniform grid `freqs` (step Δf): one period, 1/Δf long, whose
/// DFT at each grid frequency is the given value (so `h` in Pa gives Pa per
/// unit input sample). Frequencies at or above fs/2 are left out.
pub fn impulse_from_spectrum(freqs: &[f64], h: &[num_complex::Complex64], fs: f64) -> Result<Vec<f64>, String> {
    let step = crate::decay::uniform_step(freqs).ok_or("the frequency grid is not evenly spaced")?;
    let n = (fs / step).round() as usize;
    if n < 2 || n > 50_000_000 {
        return Err(format!("{n} samples is out of range"));
    }
    let used: Vec<(f64, num_complex::Complex64)> = freqs.iter().zip(h).filter(|(f, _)| **f > 0.0 && **f < fs / 2.0).map(|(f, v)| (*f, *v)).collect();
    use rayon::prelude::*;
    Ok((0..n)
        .into_par_iter()
        .map(|i| {
            let t = i as f64 / fs;
            let sum: f64 = used.iter().map(|(f, v)| (v * num_complex::Complex64::from_polar(1.0, 2.0 * PI * f * t)).re).sum();
            2.0 * step / fs * sum
        })
        .collect())
}

/// Writes a mono 32-bit float WAV.
pub fn write_wav(path: &std::path::Path, x: &[f64], rate: f64) -> std::io::Result<()> {
    let mut b = Vec::with_capacity(44 + x.len() * 4);
    let data_len = (x.len() * 4) as u32;
    b.extend(b"RIFF");
    b.extend((36 + data_len).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(3u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend((rate as u32).to_le_bytes());
    b.extend((rate as u32 * 4).to_le_bytes());
    b.extend(4u16.to_le_bytes());
    b.extend(32u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data_len.to_le_bytes());
    for v in x {
        b.extend((*v as f32).to_le_bytes());
    }
    std::fs::write(path, b)
}

/// Second-order section (direct form I).
#[derive(Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 3],
}

impl Biquad {
    fn run(&self, x: &mut [f64]) {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        for v in x.iter_mut() {
            let y = self.b[0] * *v + self.b[1] * x1 + self.b[2] * x2 - self.a[1] * y1 - self.a[2] * y2;
            x2 = x1;
            x1 = *v;
            y2 = y1;
            y1 = y;
            *v = y;
        }
    }
}

/// Octave band-pass as a cascade of Butterworth high- and low-pass
/// sections (3rd order each side via 2nd + 1st order in bilinear form,
/// approximated here by three 2nd-order sections per side with Q values of
/// a 6th-order Butterworth), band edges f/√2 and f·√2 (IEC 61260 class-1-like).
fn octave_sections(fc: f64, fs: f64) -> Vec<Biquad> {
    let (lo, hi) = (fc / 2f64.sqrt(), (fc * 2f64.sqrt()).min(0.45 * fs));
    // 6th-order Butterworth pole Qs.
    let qs = [0.517_638, 0.707_107, 1.931_852];
    let mut v = Vec::new();
    for &q in &qs {
        // High-pass at lo.
        let w = 2.0 * PI * lo / fs;
        let (c, s) = (w.cos(), w.sin());
        let alpha = s / (2.0 * q);
        let a0 = 1.0 + alpha;
        v.push(Biquad { b: [(1.0 + c) / 2.0 / a0, -(1.0 + c) / a0, (1.0 + c) / 2.0 / a0], a: [1.0, -2.0 * c / a0, (1.0 - alpha) / a0] });
        // Low-pass at hi.
        let w = 2.0 * PI * hi / fs;
        let (c, s) = (w.cos(), w.sin());
        let alpha = s / (2.0 * q);
        let a0 = 1.0 + alpha;
        v.push(Biquad { b: [(1.0 - c) / 2.0 / a0, (1.0 - c) / a0, (1.0 - c) / 2.0 / a0], a: [1.0, -2.0 * c / a0, (1.0 - alpha) / a0] });
    }
    v
}

/// Zero-phase octave band filtering (forward and backward, so arrival
/// times are not shifted; the magnitude response is squared, which the
/// 6th-order design accounts for at the band edges only approximately).
pub fn octave_filter(x: &[f64], fc: f64, fs: f64) -> Vec<f64> {
    let secs = octave_sections(fc, fs);
    let mut y = x.to_vec();
    for s in &secs {
        s.run(&mut y);
    }
    y.reverse();
    for s in &secs {
        s.run(&mut y);
    }
    y.reverse();
    y
}

/// Index of the direct sound: first sample within 20 dB of the maximum (ISO 3382-1 A.3.4).
pub fn onset(x: &[f64]) -> usize {
    let peak = x.iter().map(|v| v * v).fold(0.0, f64::max);
    x.iter().position(|v| v * v >= peak * 0.01).unwrap_or(0)
}

/// Lundeby: truncation point and noise-compensation energy for a squared
/// band IR `e` from the onset. Returns (end index, energy to add at the end).
fn lundeby(e: &[f64], fs: f64) -> (usize, f64) {
    let n = e.len();
    if n < 100 {
        return (n, 0.0);
    }
    // Smoothed envelope in 10 ms blocks.
    let win = ((0.01 * fs) as usize).max(1);
    let blocks: Vec<f64> = e.chunks(win).map(|c| c.iter().sum::<f64>() / c.len() as f64).collect();
    let t_of = |b: usize| (b * win + win / 2) as f64 / fs;
    let noise_from = |start: usize| -> f64 {
        let tail = &e[start.min(n - 1)..];
        (tail.iter().sum::<f64>() / tail.len().max(1) as f64).max(1e-300)
    };
    let mut noise = noise_from(n * 9 / 10);
    let db = |v: f64| 10.0 * v.max(1e-300).log10();
    let mut cross = n;
    for _ in 0..5 {
        // Regression from the peak down to 5 dB above the noise.
        let peak_b = blocks.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|x| x.0).unwrap_or(0);
        let lim = db(noise) + 5.0;
        let end_b = (peak_b..blocks.len()).find(|&b| db(blocks[b]) < lim).unwrap_or(blocks.len() - 1);
        if end_b <= peak_b + 2 {
            break;
        }
        let pts: Vec<(f64, f64)> = (peak_b..end_b).map(|b| (t_of(b), db(blocks[b]))).collect();
        let (slope, icpt) = linreg(&pts);
        if slope >= 0.0 {
            break;
        }
        // Crossing of the regression with the noise level.
        let tc = (db(noise) - icpt) / slope;
        let new_cross = ((tc * fs) as usize).clamp(1, n);
        // Noise estimate from 10 dB below the crossing (or the last 10 %).
        let t_noise = tc + 10.0 / -slope;
        let start = ((t_noise * fs) as usize).min(n * 9 / 10);
        noise = noise_from(start);
        if (new_cross as i64 - cross as i64).abs() < (0.001 * fs) as i64 {
            cross = new_cross;
            break;
        }
        cross = new_cross;
    }
    // Compensation: the missing tail beyond the crossing, extrapolated with the late slope.
    let tail_from = cross.saturating_sub((0.1 * fs) as usize);
    let pts: Vec<(f64, f64)> = (tail_from / win..cross / win).map(|b| (t_of(b), db(blocks[b.min(blocks.len() - 1)]))).collect();
    let comp = if pts.len() > 2 {
        let (slope, icpt) = linreg(&pts);
        if slope < 0.0 {
            let k = -slope * std::f64::consts::LN_10 / 10.0; // energy decay rate (1/s)
            let e_cross = 10f64.powf((icpt + slope * cross as f64 / fs) / 10.0);
            e_cross / k * fs
        } else {
            0.0
        }
    } else {
        0.0
    };
    (cross.max(1), comp)
}

fn linreg(p: &[(f64, f64)]) -> (f64, f64) {
    let n = p.len() as f64;
    let (sx, sy) = p.iter().fold((0.0, 0.0), |a, q| (a.0 + q.0, a.1 + q.1));
    let (mx, my) = (sx / n, sy / n);
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for (x, y) in p {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx) * (x - mx);
    }
    let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    (slope, my - slope * mx)
}

/// Decay time from the Schroeder curve `edc` (dB, sample rate `fs`) fitted
/// between `from` and `to` dB, extrapolated to 60 dB.
fn decay_time(edc: &[f64], fs: f64, from: f64, to: f64) -> f64 {
    let a = edc.iter().position(|v| *v <= from);
    let b = edc.iter().position(|v| *v <= to);
    let (Some(a), Some(b)) = (a, b) else { return f64::NAN };
    if b <= a + 2 {
        return f64::NAN;
    }
    let step = ((b - a) / 400).max(1);
    let pts: Vec<(f64, f64)> = (a..b).step_by(step).map(|i| (i as f64 / fs, edc[i])).collect();
    let (slope, _) = linreg(&pts);
    if slope < 0.0 { -60.0 / slope } else { f64::NAN }
}

/// Parameters of one band from a band-filtered IR (pressure samples),
/// starting at sample `t0` (the broadband direct-sound onset).
pub fn band_params(h: &[f64], fs: f64, t0: usize) -> BandParams {
    energy_params(h[t0..].iter().map(|v| v * v).collect(), fs)
}

/// Parameters of one band from its squared IR `e` (sampled at `fs`),
/// starting at the direct sound.
pub fn energy_params(e: Vec<f64>, fs: f64) -> BandParams {
    let (end, comp) = lundeby(&e, fs);
    params_from(&e[..end.min(e.len())], comp, fs)
}

/// Parameters of one band from a squared IR without background noise (a
/// simulated one): Schroeder integration over all of it, no noise
/// truncation or compensation.
pub fn noise_free_params(e: &[f64], fs: f64) -> BandParams {
    params_from(e, 0.0, fs)
}

/// Schroeder integration of `e` with `comp` (energy beyond its end) added.
fn params_from(e: &[f64], comp: f64, fs: f64) -> BandParams {
    // Schroeder backward integration with the compensation added.
    let mut edc = vec![0.0; e.len()];
    let mut acc = comp;
    for i in (0..e.len()).rev() {
        acc += e[i];
        edc[i] = acc;
    }
    let total = edc.first().copied().unwrap_or(0.0).max(1e-300);
    let edc_db: Vec<f64> = edc.iter().map(|v| 10.0 * (v / total).max(1e-300).log10()).collect();
    let sum_to = |t: f64| e[..((t * fs) as usize).min(e.len())].iter().sum::<f64>();
    let all: f64 = e.iter().sum::<f64>() + comp;
    let (e50, e80) = (sum_to(0.05), sum_to(0.08));
    let ts = e.iter().enumerate().map(|(i, v)| i as f64 / fs * v).sum::<f64>() / all.max(1e-300);
    BandParams {
        edt: decay_time(&edc_db, fs, -0.0 - 1e-9, -10.0),
        t20: decay_time(&edc_db, fs, -5.0, -25.0),
        t30: decay_time(&edc_db, fs, -5.0, -35.0),
        c80: 10.0 * (e80 / (all - e80).max(1e-300)).log10(),
        d50: e50 / all.max(1e-300),
        ts,
    }
}

/// Parameters in all octave bands (63 Hz – 8 kHz) of a broadband IR.
pub fn octave_params(h: &[f64], fs: f64) -> Vec<BandParams> {
    let t0 = onset(h);
    OCTAVES.iter().map(|&fc| band_params(&octave_filter(h, fc, fs), fs, t0)).collect()
}

#[cfg(test)]
mod impulse_tests {
    use super::*;
    use num_complex::Complex64;

    #[test]
    fn a_delay_in_the_spectrum_is_a_peak_at_that_time() {
        // Flat 1 Pa from 1 to 4000 Hz in 1 Hz steps, delayed 10 ms.
        let freqs: Vec<f64> = (1..=4000).map(|f| f as f64).collect();
        let h: Vec<Complex64> = freqs.iter().map(|f| Complex64::from_polar(1.0, -2.0 * PI * f * 0.010)).collect();
        let ir = impulse_from_spectrum(&freqs, &h, 8000.0).unwrap();
        assert_eq!(ir.len(), 8000);
        let peak = ir.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
        assert_eq!(peak, 80);
        // Back to the spectrum: the DFT at a grid frequency is the value given.
        let k = 1000.0;
        let back: Complex64 = ir.iter().enumerate().map(|(i, x)| Complex64::from_polar(*x, -2.0 * PI * k * i as f64 / 8000.0)).sum();
        assert!((back - h[999]).norm() < 1e-6, "{back}");
    }

    #[test]
    fn an_uneven_grid_is_refused() {
        assert!(impulse_from_spectrum(&[20.0, 21.0, 23.0], &[Complex64::new(1.0, 0.0); 3], 48000.0).is_err());
    }
}
