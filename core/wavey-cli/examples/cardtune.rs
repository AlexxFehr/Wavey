//! Cardioid tuning from a solved project: for each stack (a forward sub and
//! a reversed, polarity-inverted sub), the rear-mic level relative to a
//! front mic for the current settings, for delay = spacing/c, and for the
//! delay and gain that minimise it. Delay, gain and polarity are applied at
//! mix time, so this needs no re-solve.
//! `cargo run --example cardtune -- <project.wavey> <front mic> <spk fwd>,<spk rev>,<rear mic> …`
use num_complex::Complex64;
use wavey_project::Document;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let doc = Document::open(std::path::Path::new(&a[1])).unwrap();
    let r = doc.results().expect("project has no results").clone();
    let p = doc.project();
    let mic = |name: &str| r.mics.iter().position(|m| m.mic.name == name).unwrap_or_else(|| panic!("no mic {name}"));
    let front = mic(&a[2]);
    for stack in &a[3..] {
        let parts: Vec<&str> = stack.split(',').collect();
        let (fwd, rev): (u64, u64) = (parts[0].parse().unwrap(), parts[1].parse().unwrap());
        let rear = mic(parts[2]);
        let fi_ = |id: u64| r.speaker_ids.iter().position(|s| *s == id).unwrap();
        let (i_f, i_r) = (fi_(fwd), fi_(rev));
        let level = |delay_ms: f64, gain_db: f64, invert: bool| {
            let mut drives = vec![vec![Complex64::new(0.0, 0.0); r.freqs.len()]; r.speaker_ids.len()];
            let base = p.drives(&r.speaker_ids, &r.freqs);
            drives[i_f] = base[i_f].clone();
            let mut inst = p.speakers[&rev].clone();
            inst.delay_ms = 0.0;
            inst.gain_db = p.speakers[&fwd].gain_db + gain_db;
            inst.polarity_inverted = invert;
            for (k, f) in r.freqs.iter().enumerate() {
                drives[i_r][k] = wavey_solver::user_drive(&inst, *f) * Complex64::from_polar(1.0, -2.0 * std::f64::consts::PI * f * delay_ms / 1000.0);
            }
            let spl = |m: usize| {
                let pr = r.mic_pressure(m, &drives);
                wavey_solver::result::spl(pr.iter().map(|v| v.norm_sqr()).sum::<f64>() / pr.len() as f64)
            };
            (spl(front), spl(rear))
        };
        let (s_f, s_r) = (&p.speakers[&fwd], &p.speakers[&rev]);
        println!("stack {} + {} (rear mic `{}`):", s_f.name, s_r.name, parts[2]);
        let (f0, b0) = level(0.0, 0.0, false);
        println!("  both in phase, no delay:   front {f0:.1} dB, rear {b0:.1} dB → rear − front {:+.1} dB", b0 - f0);
        let (f1, b1) = level(s_r.delay_ms, s_r.gain_db - s_f.gain_db, s_r.polarity_inverted);
        println!("  current ({:.2} ms, {}):   front {f1:.1} dB, rear {b1:.1} dB → {:+.1} dB", s_r.delay_ms, if s_r.polarity_inverted { "inverted" } else { "normal" }, b1 - f1);
        let spacing = (s_f.position - s_r.position).truncate().length().max(0.72);
        let naive = spacing / 343.0 * 1000.0;
        let (f2, b2) = level(naive, 0.0, true);
        println!("  spacing/c ({naive:.2} ms, inverted): front {f2:.1} dB, rear {b2:.1} dB → {:+.1} dB", b2 - f2);
        let mut best = (0.0, 0.0, f64::INFINITY, 0.0);
        for d in 0..=160 {
            for g in -24..=24 {
                let (delay, gain) = (d as f64 * 0.05, g as f64 * 0.25);
                let (f, b) = level(delay, gain, true);
                if b - f < best.2 {
                    best = (delay, gain, b - f, f);
                }
            }
        }
        println!("  tuned: delay {:.2} ms, rev gain {:+.2} dB, inverted → rear − front {:+.1} dB (front {:+.1} dB vs in phase)", best.0, best.1, best.2, best.3 - f0);
    }
}
