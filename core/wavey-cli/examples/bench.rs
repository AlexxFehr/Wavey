//! Solver benchmark: times each stage of a solve and checks the result
//! against a stored reference (first run with a new reference saves it).
//!
//!   cargo run --release -p wavey-cli --example bench -- <project> <ref dir> [cpu|gpu]
use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use wavey_project::Document;
use wavey_solver::BackendChoice;
use wavey_solver::result::SimulationResult;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (path, refdir) = (&args[1], std::path::Path::new(&args[2]));
    let mut doc = Document::open(std::path::Path::new(path)).expect("open");
    if let Some(b) = args.get(3) {
        doc.edit(|p| p.sim_settings.backend = if b == "cpu" { BackendChoice::Cpu } else { BackendChoice::Gpu });
    }
    let room = doc.room().expect("room").clone();
    let input = doc.project().simulation_input(&room);

    // Stage times: by the stage name after "<source>: ".
    let times = std::sync::Mutex::new((BTreeMap::<String, f64>::new(), String::new(), Instant::now()));
    let progress = |_: f64, stage: &str| {
        let mut t = times.lock().unwrap();
        let key = stage.rsplit(": ").next().unwrap_or(stage).to_string();
        if key != t.1 {
            let now = Instant::now();
            let dt = now.duration_since(t.2).as_secs_f64();
            let prev = t.1.clone();
            *t.0.entry(prev).or_default() += dt;
            t.1 = key;
            t.2 = now;
        }
    };
    if std::env::var_os("BENCH_TWICE").is_some() {
        // A first solve in this process (as before a re-solve in the app).
        let t = Instant::now();
        wavey_solver::simulate(&input, &|_, _| {}, &AtomicBool::new(false)).expect("solve");
        println!("first solve {:.2} s", t.elapsed().as_secs_f64());
        times.lock().unwrap().2 = Instant::now();
    }
    let t0 = Instant::now();
    let r = wavey_solver::simulate(&input, &progress, &AtomicBool::new(false)).expect("solve");
    let total = t0.elapsed().as_secs_f64();
    {
        let mut t = times.lock().unwrap();
        let dt = t.2.elapsed().as_secs_f64();
        let k = t.1.clone();
        *t.0.entry(k).or_default() += dt;
        println!("total {total:.2} s · backend {}", r.stats.backend);
        let mut v: Vec<_> = t.0.iter().filter(|(k, _)| !k.is_empty()).collect();
        v.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
        for (k, s) in v {
            println!("  {s:7.2} s  {k}");
        }
    }

    if !refdir.join("results.bin").exists() {
        std::fs::create_dir_all(refdir).unwrap();
        r.save(refdir).expect("save ref");
        println!("reference saved");
        return;
    }
    let reference = SimulationResult::load(refdir).expect("load ref");
    compare(&reference, &r);
}

/// dB differences of every transfer value within 60 dB of its receiver's
/// peak, and of the late energies.
fn compare(a: &SimulationResult, b: &SimulationResult) {
    let mut diffs: Vec<f64> = vec![];
    let mut push = |x: &[num_complex::Complex32], y: &[num_complex::Complex32]| {
        let peak = x.iter().map(|v| v.norm_sqr()).fold(0f32, f32::max);
        for (p, q) in x.iter().zip(y) {
            if p.norm_sqr() > peak * 1e-6 {
                diffs.push((10.0 * ((q.norm_sqr() as f64 + 1e-30) / (p.norm_sqr() as f64 + 1e-30)).log10()).abs());
            }
        }
    };
    for (m, n) in a.mics.iter().zip(&b.mics) {
        for (x, y) in m.transfer.iter().zip(&n.transfer) {
            push(x, y);
        }
    }
    for (pa, pb) in a.planes.iter().zip(&b.planes) {
        for (x, y) in pa.transfer.iter().zip(&pb.transfer) {
            push(x, y);
        }
    }
    let mut late: Vec<f64> = vec![];
    for (m, n) in a.mics.iter().zip(&b.mics) {
        for (x, y) in m.late.iter().zip(&n.late) {
            for (p, q) in x.iter().zip(y) {
                if *p > 0.0 {
                    late.push((10.0 * (*q as f64 / *p as f64).log10()).abs());
                }
            }
        }
    }
    for (pa, pb) in a.planes.iter().zip(&b.planes) {
        for (x, y) in pa.late.iter().zip(&pb.late) {
            for (u, w) in x.iter().zip(y) {
                for (p, q) in u.iter().zip(w) {
                    if *p > 0.0 {
                        late.push((10.0 * (*q as f64 / *p as f64).log10()).abs());
                    }
                }
            }
        }
    }
    let stats = |mut v: Vec<f64>| -> String {
        if v.is_empty() {
            return "—".into();
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let q = |f: f64| v[((v.len() - 1) as f64 * f) as usize];
        format!("median {:.4} · p99 {:.3} · max {:.3} dB ({} values)", q(0.5), q(0.99), v[v.len() - 1], v.len())
    };
    println!("vs reference  transfer: {}", stats(diffs));
    println!("              late:     {}", stats(late));
}
