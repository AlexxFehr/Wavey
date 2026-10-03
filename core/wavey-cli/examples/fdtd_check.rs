//! Builds a project's wave-solver grid (as `simulate` would, cabinets
//! included) and reports, per speaker, whether its acoustic centre is in
//! the room and finds an air node — without running the solver.
//! `cargo run --release --example fdtd_check -- <project.wavey>`
use std::path::Path;

use wavey_project::Document;
use wavey_solver::fdtd::{FdtdGrid, Solid, fit_surfaces};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut doc = Document::open(Path::new(&a[1])).unwrap();
    let room = doc.room().unwrap().clone();
    let input = doc.project().simulation_input(&room);
    let fs = input.settings.fdtd.clone().expect("project has no wave solver settings");
    let c = input.medium.speed_of_sound();
    let fits = fit_surfaces(&input.surface_materials, (20.0, fs.f_max()));
    let solids: Vec<Solid> = input
        .sources
        .iter()
        .filter(|s| fs.cabinets && s.model.balloon.is_some())
        .filter_map(|s| {
            let (v, t) = s.model.cabinet_mesh();
            Solid::from_mesh(&v, &t, s.instance.rotation(), s.instance.position)
        })
        .collect();
    let t = std::time::Instant::now();
    let grid = FdtdGrid::build_with(&room, &solids, &fits, c, fs.f_max(), fs.points_per_wavelength).unwrap();
    println!("grid {:?} dx {:.3} m, {:.1} M air cells, {:.1} s", grid.n, grid.dx, grid.air_cells() as f64 / 1e6, t.elapsed().as_secs_f64());
    for s in &input.sources {
        let ctr = s.instance.acoustic_center(&s.model);
        let inside = room.contains(ctr);
        let node = grid.nearest_air(ctr);
        let in_solid: Vec<usize> = solids.iter().enumerate().filter(|(_, so)| so.contains(ctr)).map(|(i, _)| i).collect();
        println!(
            "{:<24} centre ({:.3}, {:.3}, {:.3}) in room {inside}, air node {}, inside cabinets {:?}",
            s.instance.name,
            ctr.x,
            ctr.y,
            ctr.z,
            node.is_some(),
            in_solid
        );
    }
}
