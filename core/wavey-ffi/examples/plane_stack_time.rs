//! Times opening a project and fetching each plane's stack.
//!   cargo run --release -p wavey-ffi --example plane_stack_time -- <project>
fn main() {
    let path = std::env::args().nth(1).expect("project path");
    let t = std::time::Instant::now();
    let doc = wavey_ffi::WaveyDocument::open(path).unwrap();
    println!("open {:.1} s", t.elapsed().as_secs_f64());
    let info = doc.result_info().expect("results");
    for id in info.plane_ids {
        let t = std::time::Instant::now();
        match doc.plane_stack(id) {
            Ok(s) => println!("plane {id}: {} values, {:.1} s", s.values_f32le.len() / 4, t.elapsed().as_secs_f64()),
            Err(e) => println!("plane {id}: {e:?}"),
        }
    }
}
