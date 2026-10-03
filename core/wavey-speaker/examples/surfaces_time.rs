//! Times face recovery on speaker files: `cargo run --example surfaces_time -- a.waveyspk …`
fn main() {
    for p in std::env::args().skip(1) {
        let m = wavey_speaker::SpeakerModel::load(std::path::Path::new(&p)).unwrap();
        let Some(g) = &m.geometry else { continue };
        let t = std::time::Instant::now();
        let f = wavey_speaker::surfaces::faces(&g.vertices, &g.edges);
        let tris: usize = f.iter().map(|f| f.triangles.len()).sum();
        let nested = f.iter().filter(|f| f.depth > 0).count();
        let simple = wavey_speaker::surfaces::solids(&g.vertices, &g.edges, g.style).simple;
        println!("  style: {}", if simple { "simple" } else { "detailed" });
        let sh = wavey_speaker::surfaces::shells(&f);
        let closed = sh.iter().filter(|(_, o)| *o == 0).count();
        let summary: Vec<String> = sh.iter().take(6).map(|(g, o)| format!("{}f/{o}open", g.len())).collect();
        println!("  shells {} ({closed} closed): {}", sh.len(), summary.join(" "));
        println!("{}: {} edges -> {} faces ({nested} nested), {tris} triangles in {:.1} ms", m.name, g.edges.len(), f.len(), t.elapsed().as_secs_f64() * 1e3);
    }
}
