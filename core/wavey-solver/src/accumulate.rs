//! Coherent summation of path contributions (CPU reference, f64).

use num_complex::{Complex32, Complex64};
use rayon::prelude::*;

use crate::directivity::Directivity;
use crate::ism::PathSet;
use crate::scene::{Spectra, SurfaceAcoustics};

/// Inputs shared by the CPU and GPU accumulators for one source.
pub struct AccumulateJob<'a> {
    pub paths: &'a PathSet,
    pub spectra: &'a Spectra,
    /// Source spectrum (on-axis pressure at r_ref, incl. latency).
    pub h0: &'a [Complex64],
    pub r_ref: f64,
    pub directivity: &'a Directivity,
    pub surfaces: &'a [SurfaceAcoustics],
}

impl AccumulateJob<'_> {
    pub fn receivers(&self) -> usize {
        self.paths.offsets.len() - 1
    }
}

/// Returns `[receiver][freq]` complex pressure.
pub fn accumulate_cpu(job: &AccumulateJob) -> Vec<Complex32> {
    let nf = job.spectra.freqs.len();
    let mut out = vec![Complex32::new(0.0, 0.0); job.receivers() * nf];
    out.par_chunks_mut(nf).enumerate().for_each(|(ri, row)| {
        for p in job.paths.receiver_paths(ri) {
            let refl = job.paths.reflections_of(p);
            for fi in 0..nf {
                let mut a = job.h0[fi] * job.directivity.eval(fi, &p.cell);
                for &(s, cos) in refl {
                    a *= job.surfaces[s as usize].reflection(fi, cos as f64);
                }
                let k = job.spectra.k[fi];
                let mag = job.r_ref / p.r * (-job.spectra.air_np[fi] * p.r).exp();
                let v = a * Complex64::from_polar(mag, -k * p.r);
                row[fi] += Complex32::new(v.re as f32, v.im as f32);
            }
        }
    });
    out
}
