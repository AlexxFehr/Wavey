//! ISO 3382-1 octave-band parameters of impulse responses (WAV), each and
//! averaged. `cargo run --example irparams -- a.wav b.wav …`
use wavey_solver::analysis::{OCTAVES, octave_params, read_wav};

fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    let mut all = Vec::new();
    for f in &files {
        let (h, fs) = read_wav(std::path::Path::new(f)).unwrap();
        let p = octave_params(&h, fs);
        let name = std::path::Path::new(f).file_stem().unwrap().to_string_lossy().to_string();
        println!("{name}");
        for (row, get) in rows() {
            print!("  {row:<5}");
            for b in &p { print!(" {:>7.3}", get(b)); }
            println!();
        }
        all.push(p);
    }
    println!("MEAN of {} responses   {}", all.len(), OCTAVES.iter().map(|f| format!("{f:>7}")).collect::<String>());
    for (row, get) in rows() {
        print!("  {row:<5}");
        for b in 0..OCTAVES.len() {
            let v: Vec<f64> = all.iter().map(|p| get(&p[b])).filter(|v| v.is_finite()).collect();
            print!(" {:>7.3}", v.iter().sum::<f64>() / v.len().max(1) as f64);
        }
        println!();
    }
}

type Get = fn(&wavey_solver::analysis::BandParams) -> f64;
fn rows() -> Vec<(&'static str, Get)> {
    vec![("T20", |b| b.t20), ("T30", |b| b.t30), ("EDT", |b| b.edt), ("C80", |b| b.c80), ("D50", |b| b.d50 * 100.0), ("Ts", |b| b.ts)]
}
