//! Whether each source and mic of a project lies inside the room's air
//! volume, and the clearance to the nearest surface below and above.
use glam::DVec3;
use wavey_project::Document;

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let mut doc = Document::open(std::path::Path::new(&path)).unwrap();
    let room = doc.room().unwrap().clone();
    let p = doc.project();
    let pts: Vec<(String, DVec3)> = p
        .speakers
        .values()
        .map(|s| (s.name.clone(), s.position))
        .chain(p.microphones.values().map(|m| (m.name.clone(), m.position)))
        .collect();
    for (name, x) in pts {
        let down = room.bvh.intersect(x, DVec3::NEG_Z, 1e-9, 100.0).map(|h| h.t);
        let up = room.bvh.intersect(x, DVec3::Z, 1e-9, 100.0).map(|h| h.t);
        println!("{name:<4} {:?} inside {}  floor below {:?} m, ceiling above {:?} m", x.to_array(), room.contains(x), down, up);
    }
}
