//! The lecture hall of the pilot round robin on wave-based room acoustics
//! (Fratoni et al.; Cingolani et al., Appl. Sci. 11 (2021) 2445): EDT, T30,
//! C50 and Ts at 125 Hz – 4 kHz, mean over R1–R26 for source S1, against the
//! measurement shipped with the dataset and the paper's FDTD and GA T30
//! (Table 6, "without MPP").
//!
//! `cargo run --release --example rrt_compare -- <project.wavey> [out.csv]`
use std::path::Path;

use wavey_project::Document;
use wavey_solver::raytrace::{BIN, metrics};

const BANDS: [f64; 6] = [125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0];
// Experimental Room Criteria.xlsx (S1, all receivers).
const MEAS_EDT: [f64; 6] = [1.333, 1.311, 1.556, 1.840, 1.794, 1.438];
const MEAS_T30: [f64; 6] = [1.564, 1.360, 1.620, 1.851, 1.830, 1.500];
const MEAS_C50: [f64; 6] = [-1.9, -1.5, -2.0, -3.4, -3.0, -1.9];
const MEAS_TS: [f64; 6] = [98.8, 93.0, 108.5, 132.9, 127.1, 102.3];
// Cingolani et al. 2021, Table 6: T30 of the calibrated FDTD and GA models.
const FDTD_T30: [f64; 6] = [1.44, 1.25, 1.54, 1.73, 1.70, 1.45];
const GA_T30: [f64; 6] = [1.43, 1.25, 1.62, 1.79, 1.78, 1.52];

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let doc = Document::open(Path::new(&a[1])).unwrap();
    let r = doc.results().expect("no results");
    // Octave index of each reported band in the solver's 63 Hz – 8 kHz set.
    let idx: Vec<usize> = BANDS.iter().map(|f| wavey_solver::raytrace::BANDS.iter().position(|b| b == f).unwrap()).collect();
    let mut sums = [[0.0f64; 6]; 4];
    let mut n = 0.0;
    for m in &r.mics {
        for e in &m.echograms {
            let mt = metrics(e);
            for (k, &b) in idx.iter().enumerate() {
                let d50 = mt.d50[b];
                // Centre time from the direct sound on.
                let h = &e.bands[b];
                let t0 = h.iter().position(|v| *v > 0.0).unwrap_or(0);
                let (mut num, mut den) = (0.0, 0.0);
                for (i, v) in h.iter().enumerate().skip(t0) {
                    num += (i - t0) as f64 * BIN * *v as f64;
                    den += *v as f64;
                }
                sums[0][k] += mt.edt_s[b];
                sums[1][k] += mt.t30_s[b];
                sums[2][k] += 10.0 * (d50 / (1.0 - d50)).log10();
                sums[3][k] += 1000.0 * num / den;
            }
            n += 1.0;
        }
    }
    let sim: Vec<[f64; 6]> = sums.iter().map(|s| s.map(|v| v / n)).collect();
    let jnd = |k: usize, s: f64, m: f64| match k {
        0 | 1 => (s - m).abs() / (0.05 * m),
        2 => (s - m).abs(),
        _ => (s - m).abs() / 10.0,
    };
    let meas = [MEAS_EDT, MEAS_T30, MEAS_C50, MEAS_TS];
    let names = ["EDT (s)", "T30 (s)", "C50 (dB)", "Ts (ms)"];
    println!("{:<12}{}", "", BANDS.iter().map(|f| format!("{f:>8}")).collect::<String>() + "   mean JND err");
    for k in 0..4 {
        println!("{:<12}{}", format!("{} meas", &names[k][..3]), meas[k].iter().map(|v| format!("{v:>8.2}")).collect::<String>());
        let e: f64 = (0..6).map(|b| jnd(k, sim[k][b], meas[k][b])).sum::<f64>() / 6.0;
        println!("{:<12}{}   {e:>6.2}", "  Wavey", sim[k].iter().map(|v| format!("{v:>8.2}")).collect::<String>());
        if k == 1 {
            for (name, t) in [("  FDTD (2021)", FDTD_T30), ("  GA (2021)", GA_T30)] {
                let e: f64 = (0..6).map(|b| jnd(1, t[b], MEAS_T30[b])).sum::<f64>() / 6.0;
                println!("{name:<12}{}   {e:>6.2}", t.iter().map(|v| format!("{v:>8.2}")).collect::<String>());
            }
        }
    }
    if let Some(out) = a.get(2) {
        let mut csv = String::from("tool,param,125,250,500,1000,2000,4000\n");
        for k in 0..4 {
            let p = ["EDT", "T30", "C50", "Ts"][k];
            let row = |v: &[f64; 6]| v.iter().map(|x| format!("{x}")).collect::<Vec<_>>().join(",");
            csv += &format!("Measured,{p},{}\nWavey,{p},{}\n", row(&meas[k]), row(&sim[k]));
        }
        csv += &format!("FDTD (Cingolani 2021),T30,{}\n", FDTD_T30.iter().map(|x| format!("{x}")).collect::<Vec<_>>().join(","));
        csv += &format!("GA (Cingolani 2021),T30,{}\n", GA_T30.iter().map(|x| format!("{x}")).collect::<Vec<_>>().join(","));
        std::fs::write(out, csv).unwrap();
    }
}
