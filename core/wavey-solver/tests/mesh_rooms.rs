//! Rooms that are not extrusions: the solver must treat a mesh copy of an
//! extruded room the same way, and use the local floor–ceiling column for
//! the late-field coherence.

mod common;

use common::*;
use glam::DVec3;
use wavey_geom::room_from_mesh;

/// The extruded room's triangles as a mesh (one group per surface).
fn as_mesh(room: &wavey_geom::Room) -> wavey_geom::Room {
    let faces: Vec<(Vec<u32>, u32)> = room.triangles.iter().zip(&room.tri_surface).map(|(t, s)| (t.to_vec(), *s)).collect();
    room_from_mesh(&room.positions, &faces).unwrap()
}

/// A profile in (x, z) swept along y from 0 to `w`; `caps` index the profile.
fn swept(profile: &[(f64, f64)], caps: &[&[usize]], w: f64) -> wavey_geom::Room {
    let n = profile.len() as u32;
    let mut v: Vec<DVec3> = profile.iter().map(|&(x, z)| DVec3::new(x, 0.0, z)).collect();
    v.extend(profile.iter().map(|&(x, z)| DVec3::new(x, w, z)));
    let mut faces: Vec<(Vec<u32>, u32)> = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        faces.push((vec![i, j, n + j, n + i], i));
    }
    for c in caps {
        faces.push((c.iter().map(|&i| i as u32).collect(), n));
        faces.push((c.iter().map(|&i| n + i as u32).collect(), n + 1));
    }
    room_from_mesh(&v, &faces).unwrap()
}

fn late_settings(i: &mut wavey_solver::SimulationInput) {
    i.settings.late_reverb = true;
    i.settings.rays_per_source = 2000;
}

#[test]
fn mesh_shoebox_has_the_same_late_coherence() {
    let room = shoebox(8.0, 6.0, 3.0);
    let mics = [DVec3::new(5.0, 3.0, 1.2), DVec3::new(2.0, 4.5, 1.7)];
    let mut a = input(room.clone(), 0.15, omni_at(DVec3::new(2.0, 2.0, 1.5)), &mics, 1, (100.0, 400.0));
    late_settings(&mut a);
    let mut b = a.clone();
    b.room = as_mesh(&room);
    assert!(!b.room.prism);
    let (ra, rb) = (run(&a), run(&b));
    for (ma, mb) in ra.mics.iter().zip(&rb.mics) {
        let (ga, gb) = (&ma.late_gain[0], &mb.late_gain[0]);
        assert!(ga.iter().any(|g| (g - 1.0).abs() > 0.05), "a correction is applied");
        let worst = ga.iter().zip(gb).map(|(x, y)| (10.0 * (y / x).log10()).abs()).fold(0.0f32, f32::max);
        assert!(worst < 0.01, "late gain differs by {worst} dB");
    }
}

#[test]
fn stepped_floor_uses_the_local_column() {
    // Low part 0–6 m (floor 0, ceiling 4), raised part 6–10 m (floor 1).
    let profile = [(0.0, 0.0), (6.0, 0.0), (6.0, 1.0), (10.0, 1.0), (10.0, 4.0), (0.0, 4.0)];
    let room = swept(&profile, &[&[0, 1, 2, 3, 4, 5]], 5.0);
    let col = wavey_solver::vertical::column(&room, DVec3::new(8.0, 2.5, 2.0)).unwrap();
    assert_eq!((col.z_floor, col.height), (1.0, 3.0));
    let mics = [DVec3::new(3.0, 2.5, 1.2), DVec3::new(8.0, 2.5, 2.2)];
    let mut i = input(room, 0.15, omni_at(DVec3::new(2.0, 2.5, 1.5)), &mics, 1, (100.0, 400.0));
    late_settings(&mut i);
    let r = run(&i);
    let same = &r.mics[0].late_gain[0];
    let other = &r.mics[1].late_gain[0];
    assert!(same.iter().any(|g| (g - 1.0).abs() > 0.05), "same column: corrected");
    assert!(other.iter().all(|g| *g == 1.0), "source under another column: no correction");
}

#[test]
fn column_fill_matches_the_inside_test() {
    use wavey_solver::fdtd::FdtdGrid;
    // Stepped floor plus a balcony-like overhang: a slab from x 0–3 at z 2.2–2.6.
    let profile = [(0.0, 0.0), (6.0, 0.0), (6.0, 1.0), (10.0, 1.0), (10.0, 4.0), (0.0, 4.0), (0.0, 2.6), (3.0, 2.6), (3.0, 2.2), (0.0, 2.2)];
    let room = swept(&profile, &[&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]], 5.0);
    let fits = wavey_solver::fdtd::fit_surfaces(&vec![material(0.1); room.surfaces.len()], (20.0, 200.0));
    let g = FdtdGrid::build_dx(&room, &[], &fits, 343.0, 0.1).unwrap();
    let mut mismatch = 0;
    let mut air = 0;
    for z in 1..g.n[2] - 1 {
        for y in 1..g.n[1] - 1 {
            for x in 1..g.n[0] - 1 {
                let i = g.index(x, y, z);
                let a = g.flags[i] & wavey_solver::fdtd::AIR != 0;
                let b = room.contains(g.center(x, y, z));
                air += b as usize;
                mismatch += (a != b) as usize;
            }
        }
    }
    eprintln!("{air} air cells, {mismatch} mismatches");
    assert!(air > 100_000);
    assert_eq!(mismatch, 0);
}
