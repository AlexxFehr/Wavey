//! Raw directivity around the horizontal and vertical circles at one frequency.
use glam::DVec3;
use wavey_speaker::SpeakerModel;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let m = SpeakerModel::load(std::path::Path::new(&a[1])).unwrap();
    let f: f64 = a[2].parse().unwrap();
    for (name, plane) in [("H", 0), ("V", 1)] {
        let mut line = format!("{name} {f} Hz:");
        for deg in (0..360).step_by(15) {
            let r = (deg as f64).to_radians();
            let u = if plane == 0 { DVec3::new(r.cos(), r.sin(), 0.0) } else { DVec3::new(r.cos(), 0.0, r.sin()) };
            let d = m.directivity(f, u);
            line += &format!(" {deg}:{:.1}/{:.0}", 20.0 * d.norm().log10(), d.arg().to_degrees());
        }
        println!("{line}");
    }
}
