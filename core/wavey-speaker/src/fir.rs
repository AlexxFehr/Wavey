//! FIR filters: impulse responses applied to a source on top of its EQ.
//!
//! A FIR is kept as its taps and evaluated at the result frequencies when
//! results are mixed, so changing one needs no re-solve. The taps carry their
//! own gain and delay (a linear-phase filter delays by half its length).

use num_complex::Complex64;
use serde::{Deserialize, Serialize};

/// Longest accepted filter, in taps.
pub const MAX_TAPS: usize = 65_536;

#[derive(Debug, Clone, thiserror::Error)]
pub enum FirError {
    #[error("{0}")]
    Format(String),
}

fn err<T>(s: impl Into<String>) -> Result<T, FirError> {
    Err(FirError::Format(s.into()))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fir {
    /// Where the taps came from (a file name).
    pub name: String,
    pub sample_rate_hz: f64,
    pub taps: Vec<f32>,
}

impl Fir {
    pub fn new(name: impl Into<String>, sample_rate_hz: f64, taps: Vec<f32>) -> Result<Self, FirError> {
        if taps.is_empty() {
            return err("the filter has no taps");
        }
        if taps.len() > MAX_TAPS {
            return err(format!("the filter has {} taps; the limit is {MAX_TAPS}", taps.len()));
        }
        if !(8_000.0..=384_000.0).contains(&sample_rate_hz) {
            return err(format!("sample rate {sample_rate_hz} Hz is outside 8–384 kHz"));
        }
        if taps.iter().any(|t| !t.is_finite()) {
            return err("the filter has taps that are not finite numbers");
        }
        Ok(Self { name: name.into(), sample_rate_hz, taps })
    }

    /// Complex response at `f` (Hz).
    pub fn response(&self, f: f64) -> Complex64 {
        // Horner on z = e^{-jω}: H = ((h[N-1] z + h[N-2]) z + …) z + h[0].
        let z = Complex64::from_polar(1.0, -2.0 * std::f64::consts::PI * f / self.sample_rate_hz);
        self.taps.iter().rev().fold(Complex64::new(0.0, 0.0), |acc, &h| acc * z + h as f64)
    }

    /// Time of the largest tap (ms): the filter's latency.
    pub fn latency_ms(&self) -> f64 {
        let peak = self.taps.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).map_or(0, |(i, _)| i);
        peak as f64 / self.sample_rate_hz * 1e3
    }

    /// Reads a mono WAV (the first channel of a multichannel one): 16, 24
    /// or 32-bit PCM, or 32 or 64-bit float. The sample rate is the file's.
    pub fn from_wav(name: &str, bytes: &[u8]) -> Result<Self, FirError> {
        let u16at = |i: usize| bytes.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
        let u32at = |i: usize| bytes.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
            return err("not a WAV file");
        }
        let (mut fmt, mut data) = (None, None);
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let id = &bytes[at..at + 4];
            let size = u32at(at + 4).unwrap_or(0) as usize;
            let body = at + 8;
            let end = (body + size).min(bytes.len());
            match id {
                b"fmt " => fmt = Some((body, end)),
                b"data" => data = Some((body, end)),
                _ => {}
            }
            at = body + size + (size & 1);
        }
        let (Some((f0, f1)), Some((d0, d1))) = (fmt, data) else { return err("the WAV file has no fmt or data chunk") };
        if f1 - f0 < 16 {
            return err("the WAV fmt chunk is too short");
        }
        let mut tag = u16at(f0).unwrap_or(0);
        let channels = u16at(f0 + 2).unwrap_or(1).max(1) as usize;
        let rate = u32at(f0 + 4).unwrap_or(0) as f64;
        let bits = u16at(f0 + 14).unwrap_or(0) as usize;
        if tag == 0xFFFE && f1 - f0 >= 26 {
            tag = u16at(f0 + 24).unwrap_or(0); // extensible: the sub-format's first two bytes
        }
        let width = bits / 8;
        let frame = width * channels;
        if frame == 0 {
            return err("the WAV file has no sample width");
        }
        let sample = |b: &[u8]| -> Option<f32> {
            match (tag, bits) {
                (1, 16) => Some(i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0),
                (1, 24) => Some((i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0),
                (1, 32) => Some(i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0),
                (3, 32) => Some(f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
                (3, 64) => Some(f64::from_le_bytes(b[0..8].try_into().ok()?) as f32),
                _ => None,
            }
        };
        let taps: Option<Vec<f32>> = bytes[d0..d1].chunks_exact(frame).map(|c| sample(&c[..width])).collect();
        match taps {
            Some(t) => Self::new(name, rate, t),
            None => err(format!("unsupported WAV format (tag {tag}, {bits} bits)")),
        }
    }

    /// Reads taps from text: numbers separated by whitespace, commas or
    /// semicolons; lines starting with `#` or `//` are comments.
    pub fn from_text(name: &str, text: &str, sample_rate_hz: f64) -> Result<Self, FirError> {
        let mut taps = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            for tok in line.split(|c: char| c.is_whitespace() || c == ',' || c == ';').filter(|t| !t.is_empty()) {
                match tok.parse::<f32>() {
                    Ok(v) => taps.push(v),
                    Err(_) => return err(format!("`{tok}` is not a number")),
                }
            }
        }
        Self::new(name, sample_rate_hz, taps)
    }

    /// WAV (by its header) or text (at `text_rate_hz`).
    pub fn from_bytes(name: &str, bytes: &[u8], text_rate_hz: f64) -> Result<Self, FirError> {
        if bytes.get(0..4) == Some(b"RIFF") {
            return Self::from_wav(name, bytes);
        }
        match std::str::from_utf8(bytes) {
            Ok(t) => Self::from_text(name, t, text_rate_hz),
            Err(_) => err("the file is neither a WAV nor text"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav16(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut v = b"RIFF".to_vec();
        v.extend((36 + data.len() as u32).to_le_bytes());
        v.extend(b"WAVEfmt ");
        v.extend(16u32.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(channels.to_le_bytes());
        v.extend(rate.to_le_bytes());
        v.extend((rate * 2 * channels as u32).to_le_bytes());
        v.extend((2 * channels).to_le_bytes());
        v.extend(16u16.to_le_bytes());
        v.extend(b"data");
        v.extend((data.len() as u32).to_le_bytes());
        v.extend(data);
        v
    }

    #[test]
    fn a_unit_impulse_is_flat_and_a_delayed_one_is_a_delay() {
        let flat = Fir::new("i", 48_000.0, vec![1.0]).unwrap();
        let h = flat.response(1_000.0);
        assert!((h.norm() - 1.0).abs() < 1e-12 && h.arg().abs() < 1e-12);

        // 48 samples at 48 kHz = 1 ms: the phase at 250 Hz is -90 degrees.
        let mut taps = vec![0.0; 49];
        taps[48] = 1.0;
        let d = Fir::new("d", 48_000.0, taps).unwrap();
        let h = d.response(250.0);
        assert!((h.norm() - 1.0).abs() < 1e-9);
        assert!((h.arg() + std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{}", h.arg());
        assert!((d.latency_ms() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_two_tap_average_has_a_null_at_the_nyquist_frequency() {
        let f = Fir::new("avg", 48_000.0, vec![0.5, 0.5]).unwrap();
        assert!(f.response(24_000.0).norm() < 1e-9);
        assert!((f.response(0.0).norm() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn reads_a_stereo_wav_as_its_first_channel() {
        let bytes = wav16(48_000, 2, &[16384, -16384, 0, 8192]);
        let f = Fir::from_wav("x.wav", &bytes).unwrap();
        assert_eq!(f.sample_rate_hz, 48_000.0);
        assert_eq!(f.taps, vec![0.5, 0.0]);
    }

    #[test]
    fn reads_text_with_comments_and_mixed_separators() {
        let f = Fir::from_text("t", "# taps\n1, 0.5\n// more\n-0.25;0\n", 44_100.0).unwrap();
        assert_eq!(f.taps, vec![1.0, 0.5, -0.25, 0.0]);
        assert!(Fir::from_text("t", "1 x", 44_100.0).is_err());
    }

    #[test]
    fn refuses_what_it_cannot_use() {
        assert!(Fir::new("e", 48_000.0, vec![]).is_err());
        assert!(Fir::new("r", 100.0, vec![1.0]).is_err());
        assert!(Fir::new("n", 48_000.0, vec![f32::NAN]).is_err());
        assert!(Fir::new("l", 48_000.0, vec![0.0; MAX_TAPS + 1]).is_err());
        assert!(Fir::from_wav("x", b"nope").is_err());
        assert!(Fir::from_bytes("x", &[0xff, 0xfe, 0xfd], 48_000.0).is_err());
    }
}
