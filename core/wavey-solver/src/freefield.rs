//! Free-field coherent sum of placed speakers: each speaker's measured
//! on-axis response and directivity toward the point, its processing and
//! latency, spherical spreading from its acoustic centre, propagation phase
//! and air absorption. Used for quick array predictions (no room).

use glam::DVec3;
use num_complex::Complex64;
use wavey_materials::Medium;
use wavey_speaker::{SpeakerInstance, SpeakerModel};

/// Complex pressure (Pa RMS) at `x` from all `sources` at frequency `f`.
pub fn pressure(sources: &[(&SpeakerModel, &SpeakerInstance)], medium: &Medium, x: DVec3, f: f64) -> Complex64 {
    let c = medium.speed_of_sound();
    let k = 2.0 * std::f64::consts::PI * f / c;
    let air = medium.air_intensity_coefficient(f) / 2.0;
    sources
        .iter()
        .map(|(m, s)| {
            let from = s.acoustic_center(m);
            let d = x - from;
            let r = d.length().max(1e-3);
            let dir = s.to_local(d / r);
            m.on_axis(f) * m.directivity(f, dir) * s.drive(m, f) * Complex64::from_polar(m.reference.distance_m * (-air * r).exp() / r, -k * r)
        })
        .sum()
}

/// dB SPL of a pressure.
pub fn spl(p: Complex64) -> f64 {
    20.0 * (p.norm() / 20e-6).max(1e-12).log10()
}

/// Energy-averaged SPL over a band around `f` (`width_oct` octaves wide,
/// `n` frequencies), which smooths the fine interference of large arrays.
pub fn band_spl(sources: &[(&SpeakerModel, &SpeakerInstance)], medium: &Medium, x: DVec3, f: f64, width_oct: f64, n: usize) -> f64 {
    let n = n.max(1);
    let e: f64 = (0..n)
        .map(|i| {
            let s = if n == 1 { 0.0 } else { i as f64 / (n - 1) as f64 - 0.5 };
            pressure(sources, medium, x, f * 2f64.powf(s * width_oct)).norm_sqr()
        })
        .sum::<f64>()
        / n as f64;
    10.0 * (e / 4e-10).max(1e-24).log10()
}
