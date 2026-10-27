//! A standing crowd on the floor.

use glam::DVec2;
use wavey_geom::{SurfaceKey, SurfaceKind};
use wavey_project::model::FeatureKind;
use wavey_project::{Document, Project};

/// A 10 × 6 m room, 3 m high.
fn doc() -> Document {
    let mut doc = Document::new();
    doc.edit(|p| {
        p.ceiling_height = 3.0;
        p.add_polygon(&[DVec2::new(0.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(10.0, 6.0), DVec2::new(0.0, 6.0)]);
        p.interior_seeds.insert(1, DVec2::new(1.0, 1.0));
    });
    doc
}

fn crowd(density: f64) -> FeatureKind {
    // 4 × 3 m near one end.
    FeatureKind::Crowd { outline: vec![DVec2::new(1.0, 1.0), DVec2::new(5.0, 1.0), DVec2::new(5.0, 4.0), DVec2::new(1.0, 4.0)], density }
}

fn floor_area(doc: &mut Document) -> f64 {
    doc.room().unwrap().surfaces.iter().filter(|s| s.kind == SurfaceKind::Floor).map(|s| s.area).sum()
}

fn crowd_surface(doc: &mut Document, id: u64) -> (f64, SurfaceKey) {
    let room = doc.room().unwrap();
    let s = room.surfaces.iter().find(|s| matches!(s.key, SurfaceKey::Feature { feature, .. } if feature == id)).expect("the crowd has a surface");
    assert_eq!(s.kind, SurfaceKind::Floor);
    (s.area, s.key)
}

fn alpha(p: &Project, key: &SurfaceKey, f: f64) -> f64 {
    p.material_for(key).absorption.eval(f)
}

#[test]
fn a_crowd_takes_its_outline_out_of_the_floor_and_keeps_the_total() {
    let mut d = doc();
    let before = floor_area(&mut d);
    let id = d.edit(|p| p.add_feature(crowd(2.0), None));
    let (area, _) = crowd_surface(&mut d, id);
    assert!((area - 12.0).abs() < 1e-6, "{area}");
    assert!((floor_area(&mut d) - before).abs() < 1e-6);
}

#[test]
fn the_floor_under_a_crowd_absorbs_more_the_denser_it_is() {
    let mut d = doc();
    let id = d.edit(|p| p.add_feature(crowd(1.0), None));
    let (_, key) = crowd_surface(&mut d, id);
    let sparse = alpha(d.project(), &key, 500.0);
    let floor = alpha(d.project(), &SurfaceKey::Floor { seed: 1 }, 500.0);
    assert!(sparse > floor + 0.1, "{sparse} vs {floor}");
    // A denser crowd on the same feature: same surface, more absorption.
    let mut f = d.project().features[0].clone();
    f.kind = crowd(4.0);
    d.edit(|p| p.update_feature(f).unwrap());
    let (_, key2) = crowd_surface(&mut d, id);
    assert_eq!(key, key2);
    assert!(alpha(d.project(), &key2, 500.0) > sparse + 0.05);
}

#[test]
fn a_denser_crowd_makes_a_run_stale_and_removing_it_restores_the_floor() {
    let mut d = doc();
    let plain = d.project().simulation_fingerprint();
    let id = d.edit(|p| p.add_feature(crowd(1.0), None));
    let one = d.project().simulation_fingerprint();
    assert_ne!(plain, one);
    let mut f = d.project().features[0].clone();
    f.kind = crowd(3.0);
    d.edit(|p| p.update_feature(f).unwrap());
    assert_ne!(d.project().simulation_fingerprint(), one);
    d.edit(|p| p.remove_feature(id));
    assert_eq!(d.project().simulation_fingerprint(), plain);
    assert!(d.room().unwrap().surfaces.iter().all(|s| !matches!(s.key, SurfaceKey::Feature { .. })));
}

#[test]
fn a_crowd_outside_the_room_changes_nothing() {
    let mut d = doc();
    let before = floor_area(&mut d);
    d.edit(|p| {
        p.add_feature(
            FeatureKind::Crowd { outline: vec![DVec2::new(20.0, 20.0), DVec2::new(22.0, 20.0), DVec2::new(22.0, 22.0), DVec2::new(20.0, 22.0)], density: 2.0 },
            None,
        )
    });
    assert!((floor_area(&mut d) - before).abs() < 1e-6);
}

#[test]
fn a_crowd_alone_is_still_open_air_and_is_a_patch_of_the_ground() {
    let mut p = Project::default();
    p.add_speaker_model(wavey_speaker::samples::piston("p", "Piston", 0.1)).unwrap();
    p.add_speaker("p", glam::DVec3::new(0.0, 0.0, 1.5)).unwrap();
    p.add_microphone(glam::DVec3::new(20.0, 0.0, 1.6));
    p.add_feature(FeatureKind::Ground { outline: vec![DVec2::new(-50.0, -50.0), DVec2::new(50.0, -50.0), DVec2::new(50.0, 50.0)] }, None);
    assert!(p.open_air());
    let id = p.add_feature(
        FeatureKind::Crowd { outline: vec![DVec2::new(10.0, -5.0), DVec2::new(15.0, -5.0), DVec2::new(15.0, 5.0), DVec2::new(10.0, 5.0)], density: 3.0 },
        None,
    );
    assert!(p.open_air());
    let input = p.open_air_input().unwrap();
    assert_eq!(input.ground_patches.len(), 1);
    assert!(input.ground_patches[0].material.absorption.eval(1000.0) > input.ground.as_ref().unwrap().absorption.eval(1000.0));
    // A suppressed crowd is not on the ground.
    p.features.iter_mut().find(|f| f.id == id).unwrap().suppressed = true;
    assert!(p.open_air_input().unwrap().ground_patches.is_empty());
    // A wall makes it a room again.
    p.add_polygon(&[DVec2::new(-30.0, -30.0), DVec2::new(30.0, -30.0), DVec2::new(30.0, 30.0), DVec2::new(-30.0, 30.0)]);
    assert!(!p.open_air());
}

#[test]
fn an_open_sky_is_the_ceiling_given_the_anechoic_material() {
    let mut d = doc();
    let ceiling = SurfaceKey::Ceiling { seed: 1 };
    let closed = alpha(d.project(), &ceiling, 125.0);
    let before = d.project().simulation_fingerprint();
    d.edit(|p| p.assign(&[ceiling], "anechoic").unwrap());
    assert!(alpha(d.project(), &ceiling, 125.0) > 0.99 && closed < 0.99);
    assert_ne!(d.project().simulation_fingerprint(), before);
}
