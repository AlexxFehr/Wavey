//! Synthetic speaker models for testing and demos.

use std::f64::consts::PI;

use glam::DVec3;
use num_complex::Complex64;

use crate::model::{Balloon, Enclosure, OnAxis, Reference, SpeakerModel};

const C: f64 = 343.0;

/// 1/6-octave grid from 20 Hz to 20.48 kHz.
pub fn sixth_octave_grid() -> Vec<f64> {
    (0..=60).map(|k| 20.0 * 2f64.powf(k as f64 / 6.0)).collect()
}

const THETA_STEP: f64 = 5.0;
const PHI_STEP: f64 = 10.0;

fn j(f: f64) -> Complex64 {
    Complex64::new(0.0, 2.0 * PI * f)
}

/// Second-order Butterworth high-pass.
fn highpass2(f: f64, fc: f64) -> Complex64 {
    let s = j(f) / (2.0 * PI * fc);
    s * s / (s * s + s * 2f64.sqrt() + 1.0)
}

/// Linkwitz–Riley 4th-order low-pass and high-pass (in phase, summing flat).
fn lr4(f: f64, fc: f64) -> (Complex64, Complex64) {
    let s = j(f) / (2.0 * PI * fc);
    let b2 = Complex64::new(1.0, 0.0) / (s * s + s * 2f64.sqrt() + 1.0);
    let lp = b2 * b2;
    let hp = b2 * b2 * s * s * s * s;
    (lp, hp)
}

/// Bessel function J1 via its integral representation.
fn bessel_j1(x: f64) -> f64 {
    const N: usize = 400;
    let h = PI / N as f64;
    let g = |t: f64| (t - x * t.sin()).cos();
    // Simpson's rule.
    let mut s = g(0.0) + g(PI);
    for i in 1..N {
        s += g(i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    s * h / 3.0 / PI
}

/// Circular piston directivity (baffled for the front hemisphere), with the
/// rear attenuated progressively as `ka` grows, like a driver in a box.
pub fn piston_directivity(ka: f64, theta: f64) -> Complex64 {
    let x = ka * theta.sin();
    let front = if x.abs() < 1e-6 { 1.0 } else { 2.0 * bessel_j1(x) / x };
    let rear = if theta > PI / 2.0 {
        let rho = (1.0 / (1.0 + 4.0 * ka * ka)).max(0.05);
        1.0 - (1.0 - rho) * (-theta.cos())
    } else {
        1.0
    };
    Complex64::new(front * rear, 0.0)
}

fn balloon_from(freqs: &[f64], d: impl Fn(f64, DVec3) -> Complex64) -> Balloon {
    let theta: Vec<f64> = (0..=36).map(|i| i as f64 * THETA_STEP).collect();
    let phi: Vec<f64> = (0..36).map(|i| i as f64 * PHI_STEP).collect();
    let mut magnitude_db = Vec::with_capacity(freqs.len() * theta.len() * phi.len());
    let mut phase_deg = Vec::with_capacity(magnitude_db.capacity());
    for &f in freqs {
        let on = d(f, DVec3::X);
        for &t in &theta {
            for &p in &phi {
                let (t, p) = (t.to_radians(), p.to_radians());
                let dir = DVec3::new(t.cos(), t.sin() * p.cos(), t.sin() * p.sin());
                let v = d(f, dir) / on;
                magnitude_db.push((20.0 * v.norm().max(1e-3).log10()) as f32);
                phase_deg.push(v.arg().to_degrees() as f32);
            }
        }
    }
    Balloon { theta_deg: theta, phi_deg: phi, magnitude_db, phase_deg }
}

fn base(id: &str, name: &str, freqs: Vec<f64>, h0: impl Fn(f64) -> Complex64) -> SpeakerModel {
    let mut mag = Vec::new();
    let mut ph = Vec::new();
    for &f in &freqs {
        let h = h0(f);
        mag.push(20.0 * h.norm().max(1e-12).log10());
        ph.push(h.arg().to_degrees());
    }
    SpeakerModel {
        format: crate::model::FORMAT.into(),
        version: crate::model::VERSION,
        id: id.into(),
        name: name.into(),
        manufacturer: Some("Wavey samples".into()),
        frequencies_hz: freqs,
        reference: Reference { distance_m: 1.0, input_voltage_rms: 2.83 },
        on_axis: OnAxis { magnitude_db_spl: mag, phase_deg: ph },
        max_input_voltage_rms: Some(40.0),
        max_spl_db: None,
        latency_s: 0.0,
        acoustic_center_m: [0.0; 3],
        enclosure: Enclosure::default(),
        balloon: None,
        phase_estimated: false,
        geometry: None,
        nominal_band_hz: None,
    }
}

/// Ideal point source: 90 dB SPL at 1 m / 2.83 V, flat, zero phase.
pub fn omni() -> SpeakerModel {
    let mut m = base("sample-omni", "Ideal omni (90 dB)", sixth_octave_grid(), |_| {
        Complex64::new(10f64.powf(90.0 / 20.0), 0.0)
    });
    m.enclosure = Enclosure { width_m: 0.2, height_m: 0.2, depth_m: 0.2 };
    m
}

/// Circular piston of radius `a` in a box, 95 dB sensitivity, 60 Hz high-pass.
pub fn piston(id: &str, name: &str, a: f64) -> SpeakerModel {
    let freqs = sixth_octave_grid();
    let mut m = base(id, name, freqs.clone(), |f| highpass2(f, 60.0) * 10f64.powf(95.0 / 20.0));
    m.balloon = Some(balloon_from(&freqs, |f, d| {
        piston_directivity(2.0 * PI * f / C * a, d.x.clamp(-1.0, 1.0).acos())
    }));
    m.enclosure = Enclosure { width_m: 2.6 * a, height_m: 2.6 * a, depth_m: 2.2 * a };
    m
}

/// Two-way speaker: 11 cm woofer below, 1.8 cm tweeter above, LR4 at 1.8 kHz.
/// Vertical driver spacing produces realistic lobing around the crossover.
pub fn two_way() -> SpeakerModel {
    let freqs = sixth_octave_grid();
    let (zw, zt) = (-0.10, 0.12);
    let (aw, at) = (0.11, 0.018);
    let fc = 1800.0;
    let total = move |f: f64, d: DVec3| {
        let k = 2.0 * PI * f / C;
        let (lp, hp) = lr4(f, fc);
        let th = d.x.clamp(-1.0, 1.0).acos();
        // Far field: a source displaced toward the listener arrives earlier.
        let w = lp * piston_directivity(k * aw, th) * Complex64::from_polar(1.0, k * d.z * zw);
        let t = hp * piston_directivity(k * at, th) * Complex64::from_polar(1.0, k * d.z * zt);
        w + t
    };
    let mut m = base("sample-2way", "Synthetic 2-way 12\"", freqs.clone(), |f| {
        highpass2(f, 55.0) * total(f, DVec3::X) * 10f64.powf(97.0 / 20.0)
    });
    m.balloon = Some(balloon_from(&freqs, total));
    m.enclosure = Enclosure { width_m: 0.36, height_m: 0.6, depth_m: 0.38 };
    m
}

pub fn all() -> Vec<SpeakerModel> {
    vec![omni(), piston("sample-piston", "Piston 10 cm (95 dB)", 0.1), two_way()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn j1_reference_values() {
        assert!((bessel_j1(1.0) - 0.440_050_585_7).abs() < 1e-9);
        assert!((bessel_j1(3.8317) - 0.0).abs() < 1e-4);
        assert!((bessel_j1(10.0) - 0.043_472_746_2).abs() < 1e-9);
    }

    #[test]
    fn lr4_sums_flat() {
        for f in [200.0, 1000.0, 1800.0, 4000.0] {
            let (lp, hp) = lr4(f, 1800.0);
            assert!(((lp + hp).norm() - 1.0).abs() < 1e-9);
        }
    }
}
