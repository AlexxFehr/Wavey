//! Modelling features on a drawn hall: volumes against hand calculations,
//! surfaces and keys, and that the result solves.

use glam::{DVec2, DVec3};
use wavey_geom::{SurfaceKey, SurfaceKind};
use wavey_project::Project;
use wavey_project::model::{FeatureKind, PlanAxis, WorkPlane};

/// A 10 × 8 m hall, 6 m high.
fn hall() -> Project {
    let mut p = Project::default();
    p.ceiling_height = 6.0;
    p.add_polygon(&[DVec2::new(0.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(10.0, 8.0), DVec2::new(0.0, 8.0)]);
    p.interior_seeds.insert(1, DVec2::new(1.0, 1.0));
    p
}

fn volume(p: &Project) -> f64 {
    p.build_room().unwrap().volume()
}

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} vs {b}");
}

#[test]
fn without_features_the_drawn_room_is_unchanged() {
    let p = hall();
    let r = p.build_room().unwrap();
    assert!(r.prism);
    close(r.volume(), 480.0, 1e-9);
}

#[test]
fn column_and_beam_take_air() {
    let mut p = hall();
    p.add_feature(FeatureKind::Column { centre: DVec2::new(3.0, 3.0), size: DVec2::new(0.6, 0.4), round: false, bottom: -1.0, top: 7.0 }, None);
    p.add_feature(FeatureKind::Beam { a: DVec2::new(-1.0, 6.0), b: DVec2::new(11.0, 6.0), width: 0.3, bottom: 5.4, depth: 1.0 }, None);
    let r = p.build_room().unwrap();
    assert!(!r.prism);
    close(r.volume(), 480.0 - 0.6 * 0.4 * 6.0 - 10.0 * 0.3 * 0.6, 1e-6);
    // The column's four sides are walls of the room now.
    let sides = r.surfaces.iter().filter(|s| matches!(s.key, SurfaceKey::Feature { face: 2.., .. }) && s.kind == SurfaceKind::Wall).count();
    assert!(sides >= 4);
}

#[test]
fn round_column_is_close_to_a_cylinder() {
    let mut p = hall();
    p.add_feature(FeatureKind::Column { centre: DVec2::new(5.0, 4.0), size: DVec2::new(1.0, 1.0), round: true, bottom: -1.0, top: 7.0 }, None);
    // 24-gon of radius 0.5: area = ½·24·r²·sin(15°).
    let a = 0.5 * 24.0 * 0.25 * (15f64).to_radians().sin();
    close(volume(&p), 480.0 - a * 6.0, 1e-6);
}

#[test]
fn balcony_slab_and_parapet() {
    let mut p = hall();
    // Along the back wall (x 7–10), front edge first (x 7, y 8 → y 0... given as the first edge).
    let outline = vec![DVec2::new(7.0, 0.0), DVec2::new(7.0, 8.0), DVec2::new(10.5, 8.0), DVec2::new(10.5, 0.0)];
    p.add_feature(FeatureKind::Balcony { outline, underside: 3.0, thickness: 0.4, parapet: 1.0 }, None);
    // Slab inside the hall: 3 × 8 × 0.4; parapet 0.1 × 8 × 1.0 on top (plus the 1 cm it overlaps the slab).
    close(volume(&p), 480.0 - 3.0 * 8.0 * 0.4 - 0.1 * 8.0 * 1.0, 1e-6);
}

#[test]
fn stepped_rake_volume() {
    let mut p = hall();
    p.add_feature(
        FeatureKind::Rake { min: DVec2::new(4.0, 0.0), max: DVec2::new(9.0, 8.0), axis: PlanAxis::X, reverse: false, base: 0.0, rows: 5, riser: 0.2, stepped: true },
        None,
    );
    // Steps 1 m deep, 0.2 … 1.0 m high, 8 m wide.
    close(volume(&p), 480.0 - 8.0 * (0.2 + 0.4 + 0.6 + 0.8 + 1.0), 1e-6);
    let r = p.build_room().unwrap();
    // Treads face up: five floors besides the hall floor.
    let treads = r.surfaces.iter().filter(|s| matches!(s.key, SurfaceKey::Feature { .. }) && s.kind == SurfaceKind::Floor).count();
    assert_eq!(treads, 5);
    // Reversed: the same volume, rising the other way.
    let mut q = hall();
    q.add_feature(
        FeatureKind::Rake { min: DVec2::new(4.0, 0.0), max: DVec2::new(9.0, 8.0), axis: PlanAxis::X, reverse: true, base: 0.0, rows: 5, riser: 0.2, stepped: true },
        None,
    );
    close(volume(&q), volume(&p), 1e-6);
    let rq = q.build_room().unwrap();
    // Highest tread at the x = 4 end.
    let high = rq.surfaces.iter().filter(|s| matches!(s.key, SurfaceKey::Feature { .. }) && s.kind == SurfaceKind::Floor).max_by(|a, b| a.d.total_cmp(&b.d)).unwrap();
    let c = rq.triangles[high.triangles[0] as usize].iter().map(|&i| rq.positions[i as usize]).sum::<DVec3>() / 3.0;
    assert!(c.x < 5.0, "{c}");
}

#[test]
fn vault_and_dome_add_air() {
    let mut p = hall();
    p.add_feature(FeatureKind::Vault { min: DVec2::new(0.0, 0.0), max: DVec2::new(10.0, 8.0), axis: PlanAxis::X, spring: 6.0, rise: 2.0, segments: 32 }, None);
    // Circular segment, chord 8, rise 2: R = 5, half-angle asin(0.8).
    let (r, th) = (5.0f64, (0.8f64).asin());
    let seg = r * r * (2.0 * th - (2.0 * th).sin()) / 2.0;
    close(volume(&p), 480.0 + seg * 10.0, seg * 10.0 * 0.01);

    let mut q = hall();
    q.add_feature(FeatureKind::Dome { centre: DVec2::new(5.0, 4.0), radius: 3.0, spring: 6.0, rise: 1.5, segments: 48 }, None);
    // Spherical cap: πh²(3R − h)/3, R = (a² + h²)/(2h).
    let (a, h) = (3.0f64, 1.5f64);
    let rr = (a * a + h * h) / (2.0 * h);
    let cap = std::f64::consts::PI * h * h * (3.0 * rr - h) / 3.0;
    close(volume(&q), 480.0 + cap, cap * 0.03);
}

#[test]
fn extrusion_on_a_section_plane_with_a_slope() {
    // A fly tower: 4 × 4 m added above the stage end, 6 → 12 m.
    let mut p = hall();
    let plan = WorkPlane::horizontal(0.0);
    p.add_feature(
        FeatureKind::Extrude {
            plane: plan,
            outline: vec![DVec2::new(0.0, 2.0), DVec2::new(4.0, 2.0), DVec2::new(4.0, 6.0), DVec2::new(0.0, 6.0)],
            holes: vec![],
            from: 5.0,
            to: 12.0,
            slope: 0.0,
            slope_dir: DVec2::ZERO,
        },
        None,
    );
    close(volume(&p), 480.0 + 16.0 * 6.0, 1e-6);
    // A pitched ceiling from a section: triangle profile over the width, swept along x.
    let mut q = hall();
    q.add_feature(
        FeatureKind::Extrude {
            plane: WorkPlane::section_x(0.0),
            outline: vec![DVec2::new(0.0, 6.0), DVec2::new(8.0, 6.0), DVec2::new(4.0, 8.0)],
            holes: vec![],
            from: 0.0,
            to: 10.0,
            slope: 0.0,
            slope_dir: DVec2::ZERO,
        },
        None,
    );
    close(volume(&q), 480.0 + 8.0 * 2.0 / 2.0 * 10.0, 1e-6);
}

#[test]
fn panel_imprint_and_thin_walls() {
    let mut p = hall();
    // A free-standing wall in the drawing stays two-sided.
    p.add_segment(DVec2::new(2.0, 2.0), DVec2::new(2.0, 5.0));
    p.add_feature(FeatureKind::Column { centre: DVec2::new(8.0, 6.0), size: DVec2::new(0.5, 0.5), round: false, bottom: -1.0, top: 7.0 }, None);
    // A hanging reflector.
    let refl = p.add_feature(
        FeatureKind::Panel { plane: WorkPlane::horizontal(4.5), outline: vec![DVec2::new(4.0, 3.0), DVec2::new(6.0, 3.0), DVec2::new(6.0, 5.0), DVec2::new(4.0, 5.0)] },
        None,
    );
    // An absorber on the back wall (x = 10): 2 × 1.5 m.
    let back = p.build_room().unwrap().surfaces.iter().find(|s| s.normal.x < -0.99 && s.d.abs() > 9.9).unwrap().key;
    let wall_plane = WorkPlane { origin: DVec3::new(10.0, 0.0, 0.0), u: DVec3::Y, v: DVec3::Z };
    let abs = p.add_feature(
        FeatureKind::Imprint { target: back, plane: wall_plane, outline: vec![DVec2::new(3.0, 1.0), DVec2::new(5.0, 1.0), DVec2::new(5.0, 2.5), DVec2::new(3.0, 2.5)] },
        None,
    );
    let r = p.build_room().unwrap();
    close(r.volume(), 480.0 - 0.25 * 6.0, 1e-6);
    let area = |k: SurfaceKey| r.surfaces.iter().filter(|s| s.key == k).map(|s| s.area).sum::<f64>();
    close(area(SurfaceKey::Feature { feature: refl, face: 0, facet: 0 }), 4.0, 1e-9);
    close(area(SurfaceKey::Feature { feature: refl, face: 1, facet: 0 }), 4.0, 1e-9);
    close(area(SurfaceKey::Feature { feature: abs, face: 0, facet: 0 }), 3.0, 1e-9);
    close(area(back), 48.0 - 3.0, 1e-9);
    // Both sides of the drawn free-standing wall.
    let thin = r.surfaces.iter().filter(|s| matches!(s.key, SurfaceKey::Wall { .. }) && (s.d.abs() - 2.0).abs() < 1e-9 && s.normal.x.abs() > 0.99).count();
    assert_eq!(thin, 2);
}

#[test]
fn feature_materials_and_keys_survive_edits() {
    let mut p = hall();
    let id = p.add_feature(FeatureKind::Column { centre: DVec2::new(3.0, 3.0), size: DVec2::new(0.5, 0.5), round: false, bottom: -1.0, top: 7.0 }, None);
    p.features[0].material = Some("brick".into());
    let side = SurfaceKey::Feature { feature: id, face: 3, facet: 0 };
    p.assign(&[side], "wood-panel").unwrap();
    let r = p.build_room().unwrap();
    assert!(r.surface_index(side).is_some());
    assert_eq!(p.material_for(&SurfaceKey::Feature { feature: id, face: 2, facet: 0 }).id, "brick");
    assert_eq!(p.material_for(&side).id, "wood-panel");
    // Moving the column keeps the face's key and material.
    if let FeatureKind::Column { centre, .. } = &mut p.features[0].kind {
        *centre = DVec2::new(6.0, 5.0);
    }
    let r = p.build_room().unwrap();
    assert!(r.surface_index(side).is_some());
    assert_eq!(p.material_for(&side).id, "wood-panel");
    p.remove_feature(id);
    assert!(p.assignments.keys().all(|k| !k.starts_with("feat:")));
}

#[test]
fn modelled_hall_is_closed_and_solves() {
    use std::sync::atomic::AtomicBool;
    let mut p = hall();
    p.add_feature(
        FeatureKind::Rake { min: DVec2::new(4.0, 0.0), max: DVec2::new(9.0, 8.0), axis: PlanAxis::X, reverse: false, base: 0.0, rows: 5, riser: 0.2, stepped: true },
        None,
    );
    let outline = vec![DVec2::new(7.0, 0.0), DVec2::new(7.0, 8.0), DVec2::new(10.5, 8.0), DVec2::new(10.5, 0.0)];
    p.add_feature(FeatureKind::Balcony { outline, underside: 3.0, thickness: 0.4, parapet: 1.0 }, None);
    p.add_feature(FeatureKind::Vault { min: DVec2::new(0.0, 0.0), max: DVec2::new(10.0, 8.0), axis: PlanAxis::X, spring: 6.0, rise: 2.0, segments: 16 }, None);
    let room = p.build_room().unwrap();
    // Closed: every edge shared (after T-junctions are split).
    let verts: Vec<DVec3> = room.positions.clone();
    let faces: Vec<(Vec<u32>, u32)> = room.triangles.iter().map(|t| (t.to_vec(), 0)).collect();
    let (_, _, rep) = wavey_geom::repair(&verts, &faces, wavey_geom::RepairOptions { weld: 1e-5, max_hole_edges: 0 });
    assert!(rep.watertight(), "{} open edges", rep.open_edges.len());
    // Diffracting edges: tread nosings, balcony front and parapet edges.
    let edges = wavey_solver::diffraction::find_edges(&room);
    assert!(edges.len() >= 7, "{} edges", edges.len());

    let model = wavey_speaker::samples::omni();
    let mid = p.add_speaker_model(model).unwrap();
    p.add_speaker(&mid, DVec3::new(2.0, 4.0, 1.5)).unwrap();
    p.add_microphone(DVec3::new(6.5, 4.0, 1.8));
    p.add_microphone(DVec3::new(8.5, 2.0, 2.2)); // under the balcony
    p.sim_settings.f_min = 100.0;
    p.sim_settings.f_max = 400.0;
    p.sim_settings.max_order = 2;
    p.sim_settings.diffraction = true;
    p.sim_settings.backend = wavey_solver::BackendChoice::Cpu;
    let input = p.simulation_input(&room);
    let r = wavey_solver::simulate(&input, &|_, _| {}, &AtomicBool::new(false)).unwrap();
    assert!(r.stats.diffracted_paths > 0);
    assert!(r.mics.iter().all(|m| m.transfer[0].iter().all(|v| v.norm().is_finite() && v.norm() > 0.0)));
}
