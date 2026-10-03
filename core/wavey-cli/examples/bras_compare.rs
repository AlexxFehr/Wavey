//! Wavey against measurements and reference simulators on a BRAS room:
//! T20, EDT, D50 and C80 per octave band (mean over all source–receiver
//! pairs), and the mean error in JNDs (ISO 3382-1: 5 % for T20/EDT,
//! 0.05 for D50, 1 dB for C80).
//!
//! `cargo run --example bras_compare -- <project.wavey> <measured wav dir> <reference results dir>`
use std::collections::BTreeMap;
use std::path::Path;

use wavey_project::Document;
use wavey_solver::analysis::{OCTAVES, octave_params, read_wav};

const PARAMS: [&str; 4] = ["T20", "EDT", "D50", "C80"];

/// Mean JND error of `sim` against `meas` for one parameter over bands with data.
fn jnd(param: &str, sim: &[f64], meas: &[f64]) -> f64 {
    let errs: Vec<f64> = sim
        .iter()
        .zip(meas)
        .filter(|(s, m)| s.is_finite() && m.is_finite())
        .map(|(s, m)| match param {
            "T20" | "EDT" => (s - m).abs() / (0.05 * m),
            "D50" => (s - m).abs() / 0.05,
            _ => (s - m).abs() / 1.0,
        })
        .collect();
    errs.iter().sum::<f64>() / errs.len().max(1) as f64
}

fn mean_over(rows: &[[f64; 8]]) -> [f64; 8] {
    let mut out = [f64::NAN; 8];
    for b in 0..8 {
        let v: Vec<f64> = rows.iter().map(|r| r[b]).filter(|x| x.is_finite()).collect();
        if !v.is_empty() {
            out[b] = v.iter().sum::<f64>() / v.len() as f64;
        }
    }
    out
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut tools: BTreeMap<String, BTreeMap<&str, [f64; 8]>> = BTreeMap::new();

    // Measured: ISO 3382 analysis of the omni (dodecahedron) RIRs.
    let mut meas: BTreeMap<&str, Vec<[f64; 8]>> = BTreeMap::new();
    for e in std::fs::read_dir(&a[2]).unwrap().flatten() {
        let p = e.path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if !name.contains("Dodecahedron") || !name.ends_with(".wav") {
            continue;
        }
        let (h, fs) = read_wav(&p).unwrap();
        let bp = octave_params(&h, fs);
        let row = |f: fn(&wavey_solver::analysis::BandParams) -> f64| {
            let mut r = [0.0; 8];
            for (i, b) in bp.iter().enumerate() {
                r[i] = f(b);
            }
            r
        };
        meas.entry("T20").or_default().push(row(|b| b.t20));
        meas.entry("EDT").or_default().push(row(|b| b.edt));
        meas.entry("D50").or_default().push(row(|b| b.d50));
        meas.entry("C80").or_default().push(row(|b| b.c80));
    }
    let measured: BTreeMap<&str, [f64; 8]> = meas.iter().map(|(k, v)| (*k, mean_over(v))).collect();

    // Wavey: hybrid echograms per source and microphone.
    let doc = Document::open(Path::new(&a[1])).unwrap();
    let r = doc.results().expect("no results");
    let mut w: BTreeMap<&str, Vec<[f64; 8]>> = BTreeMap::new();
    for m in &r.mics {
        for e in &m.echograms {
            let mt = wavey_solver::raytrace::metrics(e);
            w.entry("T20").or_default().push(mt.t20_s);
            w.entry("EDT").or_default().push(mt.edt_s);
            w.entry("D50").or_default().push(mt.d50);
            w.entry("C80").or_default().push(mt.c80_db);
        }
    }
    tools.insert("Wavey".into(), w.iter().map(|(k, v)| (*k, mean_over(v))).collect());

    // Reference simulators: *_mean_1_1.csv (D50 in %).
    fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.to_string_lossy().ends_with("mean_1_1.csv") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(Path::new(&a[3]), &mut files);
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy().split("_mean").next().unwrap().to_string();
        let mut t = BTreeMap::new();
        for line in std::fs::read_to_string(&f).unwrap().lines() {
            let c: Vec<&str> = line.split(',').collect();
            let key = PARAMS.iter().find(|p| **p == c[0].trim());
            if let Some(k) = key {
                let mut row = [f64::NAN; 8];
                for (i, v) in c[1..].iter().take(8).enumerate() {
                    row[i] = v.trim().parse().unwrap_or(f64::NAN) / if *k == "D50" { 100.0 } else { 1.0 };
                }
                t.insert(*k, row);
            }
        }
        tools.insert(name, t);
    }

    for p in PARAMS {
        println!("\n{p:<16}{}", OCTAVES.iter().map(|f| format!("{f:>7}")).collect::<String>() + "   mean JND err");
        let m = measured[p];
        println!("{:<16}{}", "Measured", m.iter().map(|v| format!("{v:>7.3}")).collect::<String>());
        for (name, t) in &tools {
            if let Some(v) = t.get(p) {
                println!("{:<16}{}   {:>6.2}", name, v.iter().map(|x| format!("{x:>7.3}")).collect::<String>(), jnd(p, v, &m));
            }
        }
    }
    // Machine-readable copy for plotting: tool,param,values…
    if let Some(out) = a.get(4) {
        let mut csv = String::from("tool,param,63,125,250,500,1000,2000,4000,8000\n");
        for p in PARAMS {
            csv += &format!("Measured,{p},{}\n", measured[p].iter().map(|v| format!("{v}")).collect::<Vec<_>>().join(","));
            for (name, t) in &tools {
                if let Some(v) = t.get(p) {
                    csv += &format!("{name},{p},{}\n", v.iter().map(|v| format!("{v}")).collect::<Vec<_>>().join(","));
                }
            }
        }
        std::fs::write(out, csv).unwrap();
    }
    println!("\nOverall mean JND error (T20, EDT, D50, C80):");
    for (name, t) in &tools {
        let e: Vec<f64> = PARAMS.iter().filter_map(|p| t.get(p).map(|v| jnd(p, v, &measured[p]))).collect();
        println!("  {name:<16} {:>6.2}  ({} params)", e.iter().sum::<f64>() / e.len().max(1) as f64, e.len());
    }
}
