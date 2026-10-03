//! Simulated cabinet directivity (rigid box + driver on the baffle) against
//! the manufacturer's measured balloon. `cargo run --example cabcal -- <file.waveyspk>`
use std::sync::atomic::AtomicBool;

use glam::DVec3;
use wavey_solver::fdtd::{calibrate_cabinet, run_cpu};
use wavey_speaker::SpeakerModel;

fn main() {
    let path = std::env::args().nth(1).expect("waveyspk path");
    let m = SpeakerModel::load(std::path::Path::new(&path)).unwrap();
    let (v, t) = m.cabinet_mesh();
    let lo = v.iter().fold(DVec3::splat(f64::INFINITY), |a, b| a.min(*b));
    let hi = v.iter().fold(DVec3::splat(f64::NEG_INFINITY), |a, b| a.max(*b));
    println!("{}: cabinet {:.2} × {:.2} × {:.2} m, acoustic centre {:?}", m.name, hi.x - lo.x, hi.y - lo.y, hi.z - lo.z, m.acoustic_center_m);
    let freqs = [31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0];
    let cancel = AtomicBool::new(false);
    let run = |g: &wavey_solver::fdtd::FdtdGrid, s: &[(usize, f64)], sig: &[f64], rx: &[usize], f: &[f64]| run_cpu(g, s, sig, rx, f, &cancel, &|_| {});
    let t0 = std::time::Instant::now();
    let cal = calibrate_cabinet(&v, &t, DVec3::from_array(m.acoustic_center_m), 343.0, 160.0, 15.0, &freqs, &run).unwrap();
    println!("calibrated in {:.1} s", t0.elapsed().as_secs_f64());
    println!("  f Hz | on-axis gain |   90° sim / meas   |  180° sim / meas");
    for (j, f) in freqs.iter().enumerate() {
        let db = |c: num_complex::Complex64| 20.0 * c.norm().max(1e-9).log10();
        let (i90, i180) = (cal.angles_deg.iter().position(|a| *a == 90.0).unwrap(), cal.angles_deg.iter().position(|a| *a == 180.0).unwrap());
        println!(
            "{f:>6} | {:>+8.1} dB | {:>+6.1} / {:>+6.1} dB | {:>+6.1} / {:>+6.1} dB",
            db(cal.on_axis[j]),
            db(cal.directivity[i90][j]),
            db(m.directivity(*f, DVec3::Y)),
            db(cal.directivity[i180][j]),
            db(m.directivity(*f, DVec3::NEG_X))
        );
    }
}
