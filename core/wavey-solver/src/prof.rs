//! Stage timers for profiling (printed by `report` when WAVEY_PROFILE is set).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Instant;

static TIMES: Mutex<BTreeMap<&'static str, (f64, usize)>> = Mutex::new(BTreeMap::new());

/// Runs `f`, adding its wall time to `label`.
pub fn time<T>(label: &'static str, f: impl FnOnce() -> T) -> T {
    let t0 = Instant::now();
    let r = f();
    let dt = t0.elapsed().as_secs_f64();
    let mut m = TIMES.lock().unwrap();
    let e = m.entry(label).or_default();
    e.0 += dt;
    e.1 += 1;
    r
}

/// Adds a measured time to `label`.
pub fn add(label: &'static str, secs: f64) {
    let mut m = TIMES.lock().unwrap();
    let e = m.entry(label).or_default();
    e.0 += secs;
    e.1 += 1;
}

/// Prints and clears the timers (only with WAVEY_PROFILE set).
pub fn report() {
    let mut m = TIMES.lock().unwrap();
    if std::env::var_os("WAVEY_PROFILE").is_some() {
        let mut v: Vec<_> = m.iter().collect();
        v.sort_by(|a, b| b.1.0.partial_cmp(&a.1.0).unwrap());
        for (k, (s, n)) in v {
            eprintln!("prof {s:8.3} s  {k} ×{n}");
        }
    }
    m.clear();
}
