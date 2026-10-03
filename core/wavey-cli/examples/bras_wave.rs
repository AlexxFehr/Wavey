//! The wave solver alone on a BRAS room: impulse responses from the FDTD
//! grid at every source–microphone pair, analysed like the measured RIRs
//! (ISO 3382-1), in the octave bands the grid resolves.
//!
//! `cargo run --release --example bras_wave -- <project.wavey> <measured wav dir> [grid f_max Hz] [duration s]`
use std::path::Path;
use std::sync::atomic::AtomicBool;

use wavey_project::Document;
use wavey_solver::analysis::{OCTAVES, octave_filter, octave_params, onset, band_params, read_wav};
use wavey_solver::fdtd;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let f_max: f64 = a.get(3).map_or(500.0, |v| v.parse().unwrap());
    let duration: f64 = a.get(4).map_or(2.5, |v| v.parse().unwrap());
    let bands: Vec<usize> = (0..OCTAVES.len()).filter(|&b| OCTAVES[b] * 2f64.sqrt() <= f_max * 0.75).collect();

    let mut doc = Document::open(Path::new(&a[1])).unwrap();
    let room = doc.room().unwrap().clone();
    let input = doc.project().simulation_input(&room);
    let c = input.medium.speed_of_sound();
    let fits = fdtd::fit_surfaces(&input.surface_materials, (20.0, f_max));
    let worst = fits.iter().map(|f| f.max_error).fold(0.0, f64::max);
    let grid = fdtd::FdtdGrid::build(&room, &fits, c, f_max, 10.0).unwrap();
    let steps = (duration / grid.dt).ceil() as usize;
    eprintln!(
        "grid: {:.2} M air cells, dx {:.1} cm, dt {:.3} ms, {steps} steps, worst α fit error {worst:.3}",
        grid.air_cells() as f64 / 1e6,
        grid.dx * 100.0,
        grid.dt * 1e3
    );
    // Wider pulse band than the grid's, so the analysed bands see a flat source.
    let signal = fdtd::source_signal(2.0 * f_max, grid.dt, steps);
    let fs = 1.0 / grid.dt;

    // [band] → per-pair (t20, edt, d50, c80); also T30, C50 and Ts.
    let mut extra: Vec<Vec<[f64; 3]>> = vec![Vec::new(); OCTAVES.len()];
    let mut sim: Vec<Vec<[f64; 4]>> = vec![Vec::new(); OCTAVES.len()];
    let rx: Vec<usize> = input.microphones.iter().map(|m| grid.nearest_air(m.position).expect("mic outside grid")).collect();
    for src in &input.sources {
        let node = grid.nearest_air(src.instance.acoustic_center(&src.model)).expect("source outside grid");
        let t = std::time::Instant::now();
        let irs = fdtd::run_cpu_ir(&grid, &[(node, 1.0)], &signal, &rx, &AtomicBool::new(false), &|_| {}).unwrap();
        eprintln!("source {}: {:.0} s", src.instance.name, t.elapsed().as_secs_f64());
        for p in irs {
            // Response to the Gaussian (the signal's integral): flat at low f.
            let mut acc = 0.0;
            let h: Vec<f64> = p.iter().map(|v| {
                acc += v;
                acc
            }).collect();
            let t0 = onset(&h);
            for &b in &bands {
                let bp = band_params(&octave_filter(&h, OCTAVES[b], fs), fs, t0);
                sim[b].push([bp.t20, bp.edt, bp.d50, bp.c80]);
                extra[b].push([bp.t30, 10.0 * (bp.d50 / (1.0 - bp.d50)).log10(), bp.ts * 1000.0]);
            }
        }
    }

    // Measured, as in bras_compare.
    let mut meas: Vec<Vec<[f64; 4]>> = vec![Vec::new(); OCTAVES.len()];
    for e in std::fs::read_dir(&a[2]).into_iter().flatten().flatten() {
        let p = e.path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if !name.contains("Dodecahedron") || !name.ends_with(".wav") {
            continue;
        }
        let (h, rate) = read_wav(&p).unwrap();
        for (b, bp) in octave_params(&h, rate).iter().enumerate() {
            meas[b].push([bp.t20, bp.edt, bp.d50, bp.c80]);
        }
    }
    // Current hybrid result, if any.
    let mut hyb: Vec<Vec<[f64; 4]>> = vec![Vec::new(); OCTAVES.len()];
    if let Some(r) = doc.results() {
        for m in &r.mics {
            for e in &m.echograms {
                let mt = wavey_solver::raytrace::metrics(e);
                for b in 0..OCTAVES.len() {
                    hyb[b].push([mt.t20_s[b], mt.edt_s[b], mt.d50[b], mt.c80_db[b]]);
                }
            }
        }
    }
    let mean = |v: &[[f64; 4]], k: usize| {
        let x: Vec<f64> = v.iter().map(|r| r[k]).filter(|x| x.is_finite()).collect();
        x.iter().sum::<f64>() / x.len().max(1) as f64
    };
    let jnd = |k: usize, s: f64, m: f64| match k {
        0 | 1 => (s - m).abs() / (0.05 * m),
        2 => (s - m).abs() / 0.05,
        _ => (s - m).abs(),
    };
    for &b in &bands {
        let m = |k: usize| {
            let x: Vec<f64> = extra[b].iter().map(|r| r[k]).filter(|x| x.is_finite()).collect();
            x.iter().sum::<f64>() / x.len().max(1) as f64
        };
        println!("{:>5} Hz  wave T30 {:6.3} s  C50 {:6.2} dB  Ts {:6.1} ms", OCTAVES[b], m(0), m(1), m(2));
    }
    for (k, name) in ["T20", "EDT", "D50", "C80"].iter().enumerate() {
        println!("{name}");
        for &b in &bands {
            let (m, w, h) = (mean(&meas[b], k), mean(&sim[b], k), mean(&hyb[b], k));
            println!(
                "  {:>5} Hz  measured {m:7.3}  wave {w:7.3} ({:4.2} JND)  hybrid {h:7.3} ({:4.2} JND)",
                OCTAVES[b],
                jnd(k, w, m),
                jnd(k, h, m)
            );
        }
    }
}
