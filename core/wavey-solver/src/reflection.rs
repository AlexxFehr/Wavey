//! Reflection of a spherical wave off a locally reacting surface.
//!
//! The plane-wave factor R = (ζ cos θ − 1)/(ζ cos θ + 1) tends to −1 at
//! grazing incidence for any finite impedance, which badly overstates the
//! loss of a nearly rigid floor seen at a shallow angle. For a point source
//! the Weyl–van der Pol formula adds the ground-wave term:
//!
//!   Q = R + (1 − R) F(w),   F(w) = 1 + j√π w e^{−w²} erfc(−jw)
//!
//! with the numerical distance w = ½(1 + j)√(kr)(cos θ + 1/ζ) (written here
//! in the e^{−iωt} convention of the literature; the solver uses e^{jωt}, so
//! inputs and result are conjugated). Q → 1 near grazing when |w| is small,
//! and Q → R far from the source.

use std::f64::consts::PI;
use std::sync::LazyLock;

use num_complex::Complex64;

const N: usize = 32;

/// Weideman (1994) rational-series coefficients for the Faddeeva function.
static WEIDEMAN: LazyLock<(f64, [f64; N])> = LazyLock::new(|| {
    let m = 2 * N;
    let m2 = 2 * m;
    let l = (N as f64 / 2f64.sqrt()).sqrt();
    // f = [0, f(−M+1) … f(M−1)], length 2M.
    let mut f = vec![0.0; m2];
    for (i, k) in (-(m as i64) + 1..m as i64).enumerate() {
        let t = l * (k as f64 * PI / m as f64 / 2.0).tan();
        f[i + 1] = (-t * t).exp() * (l * l + t * t);
    }
    // fftshift, then the real part of the DFT.
    let shifted: Vec<f64> = (0..m2).map(|i| f[(i + m2 / 2) % m2]).collect();
    let a: Vec<f64> = (0..m2)
        .map(|k| {
            shifted.iter().enumerate().map(|(j, v)| v * (-2.0 * PI * (j * k) as f64 / m2 as f64).cos()).sum::<f64>() / m2 as f64
        })
        .collect();
    // Polynomial coefficients, highest degree first: a(N) … a(1).
    let mut c = [0.0; N];
    for (i, ci) in c.iter_mut().enumerate() {
        *ci = a[N - i];
    }
    (l, c)
});

/// WGSL constants `WL` and `WC` (the Weideman series) for the GPU kernel.
pub fn wgsl_constants() -> String {
    let (l, c) = &*WEIDEMAN;
    let list: Vec<String> = c.iter().map(|v| format!("{v:e}")).collect();
    format!("const WL: f32 = {l:e};\nconst WC = array<f32, {N}>({});\n", list.join(", "))
}

/// Faddeeva function w(z) = e^{−z²} erfc(−iz).
pub fn faddeeva(z: Complex64) -> Complex64 {
    if z.im < 0.0 {
        return 2.0 * (-z * z).exp() - faddeeva(-z);
    }
    let (l, c) = &*WEIDEMAN;
    let i = Complex64::i();
    let den = *l - i * z;
    let zz = (*l + i * z) / den;
    let mut p = Complex64::new(0.0, 0.0);
    for ci in c {
        p = p * zz + ci;
    }
    2.0 * p / (den * den) + (1.0 / PI.sqrt()) / den
}

/// Spherical-wave reflection factor (solver convention, e^{jωt}) for a
/// surface of normalized impedance `zeta`, incidence `cos` (to the normal),
/// wavenumber `k` and unfolded path length `r`.
pub fn spherical(zeta: Complex64, cos: f64, k: f64, r: f64) -> Complex64 {
    let cos = cos.clamp(0.0, 1.0);
    // Literature convention: conjugate impedance.
    let beta = 1.0 / zeta.conj();
    let rp = (cos - beta) / (cos + beta);
    let w = Complex64::new(0.5, 0.5) * (k * r).sqrt() * (cos + beta);
    let f = 1.0 + Complex64::i() * PI.sqrt() * w * faddeeva(w);
    (rp + (1.0 - rp) * f).conj()
}

/// Plane-wave reflection factor (solver convention).
pub fn plane(zeta: Complex64, cos: f64) -> Complex64 {
    let zc = zeta * cos.max(1e-6);
    (zc - 1.0) / (zc + 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faddeeva_reference_values() {
        // Abramowitz & Stegun 7.1; w(i) = e·erfc(1); w(1) = e^{-1} + 2i/√π·D(1).
        let cases = [
            (Complex64::new(0.0, 0.0), Complex64::new(1.0, 0.0)),
            (Complex64::new(0.0, 1.0), Complex64::new(0.427_583_576_155_807, 0.0)),
            (Complex64::new(1.0, 0.0), Complex64::new(0.367_879_441_171_442, 0.607_157_705_841_394)),
            (Complex64::new(2.0, 1.0), Complex64::new(0.140_239_581_366_278, 0.222_213_440_179_899)),
        ];
        for (z, want) in cases {
            let got = faddeeva(z);
            assert!((got - want).norm() < 1e-12, "w({z}) = {got}, want {want}");
        }
        // Lower half-plane via the reflection formula: w(−i) = 2e − w(i).
        let got = faddeeva(Complex64::new(0.0, -1.0));
        let want = 2.0 * 1f64.exp() - 0.427_583_576_155_807;
        assert!((got.re - want).abs() < 1e-11 && got.im.abs() < 1e-11, "{got}");
    }

    #[test]
    fn spherical_limits() {
        let zeta = Complex64::new(12.0, -8.0);
        // Rigid surface reflects fully.
        assert!((spherical(Complex64::new(1e12, 0.0), 0.3, 5.0, 10.0) - 1.0).norm() < 1e-6);
        // Far from the source, Q approaches the plane-wave factor.
        let (q, r) = (spherical(zeta, 0.6, 50.0, 1e4), plane(zeta, 0.6));
        assert!((q - r).norm() < 1e-3, "{q} vs {r}");
        // Near grazing on a hard surface, Q stays near +1 while R → −1.
        let q = spherical(Complex64::new(80.0, -20.0), 0.01, 5.0, 10.0);
        assert!(q.re > 0.5, "{q}");
        assert!(plane(Complex64::new(80.0, -20.0), 0.01).re < 0.0);
    }
}
