//! A project without walls is open air: the ground plane placed in Geometry
//! reflects, nothing else does. Free field switches even that off.

use std::sync::atomic::AtomicBool;

use glam::{DVec2, DVec3};
use wavey_project::areas::Areas;
use wavey_project::model::FeatureKind;
use wavey_project::{Document, Project};
use wavey_solver::{BackendChoice, Resolution, SimMode, simulate};
use wavey_speaker::samples;

/// Sets the material of the project's ground plane (placing one if needed).
fn set_ground(p: &mut Project, material: &str) {
    let id = match p.ground_feature() {
        Some(f) => f.id,
        None => p.add_feature(FeatureKind::Ground { outline: vec![DVec2::new(-50.0, -50.0), DVec2::new(50.0, -50.0), DVec2::new(50.0, 50.0), DVec2::new(-50.0, 50.0)] }, None),
    };
    let mut f = p.features.iter().find(|f| f.id == id).unwrap().clone();
    f.material = Some(material.into());
    p.update_feature(f).unwrap();
}

/// No walls: a speaker 2 m up, a microphone 10 m away, a ground plane of asphalt.
fn open_air() -> Project {
    let mut p = Project::default();
    p.add_speaker_model(samples::omni()).unwrap();
    let model = p.speaker_models.keys().next().unwrap().clone();
    p.add_speaker(&model, DVec3::new(0.0, 0.0, 2.0)).unwrap();
    p.add_microphone(DVec3::new(10.0, 0.0, 1.5));
    p.sim_settings.f_min = 100.0;
    p.sim_settings.f_max = 400.0;
    p.sim_settings.resolution = Resolution::FractionalOctave { n: 3 };
    p.sim_settings.backend = BackendChoice::Cpu;
    set_ground(&mut p, "asphalt");
    p
}

fn walled() -> Project {
    let mut p = open_air();
    p.ceiling_height = 4.0;
    p.add_polygon(&[DVec2::new(-20.0, -20.0), DVec2::new(20.0, -20.0), DVec2::new(20.0, 20.0), DVec2::new(-20.0, 20.0)]);
    p.interior_seeds.insert(1, DVec2::new(1.0, 1.0));
    p
}

fn level_at_mic(p: &Project) -> Vec<f64> {
    let mut doc = Document::new();
    doc.edit(|d| *d = p.clone());
    let input = doc.solver_input().unwrap();
    let r = simulate(&input, &|_, _| {}, &AtomicBool::new(false)).unwrap();
    let drives = vec![vec![num_complex::Complex64::new(1.0, 0.0); r.freqs.len()]; r.speaker_ids.len()];
    r.mic_pressure(0, &drives).iter().map(|v| v.norm()).collect()
}

#[test]
fn a_project_without_walls_is_open_air_with_a_ground() {
    let p = open_air();
    assert!(p.open_air());
    assert!(!walled().open_air());
    let input = p.open_air_input().unwrap();
    assert_eq!(input.ground.as_ref().unwrap().id, "asphalt");
    // The box holds the sources and receivers with room to spare.
    assert!(input.room.contains(DVec3::new(-30.0, 0.0, 10.0)) && input.room.contains(DVec3::new(45.0, 0.0, 1.5)));
}

#[test]
fn it_solves_and_the_ground_shapes_the_result() {
    let mut p = open_air();
    let asphalt = level_at_mic(&p);
    set_ground(&mut p, "grass");
    let grass = level_at_mic(&p);
    assert!(asphalt.iter().all(|v| *v > 0.0));
    // Hard and soft ground interfere with the direct sound differently.
    assert!(asphalt.iter().zip(&grass).any(|(a, g)| (20.0 * (a / g).log10()).abs() > 0.5));
    // Free field leaves only the direct sound, whichever the ground.
    p.sim_settings.mode = SimMode::FreeField;
    let free_grass = level_at_mic(&p);
    set_ground(&mut p, "asphalt");
    assert_eq!(free_grass, level_at_mic(&p));
}

#[test]
fn the_ground_is_part_of_what_a_run_depends_on_over_open_air_only() {
    let mut p = open_air();
    let before = p.simulation_fingerprint();
    set_ground(&mut p, "grass");
    assert_ne!(before, p.simulation_fingerprint());
    assert!(p.fingerprint_parts().len() == open_air().fingerprint_parts().len());

    // Free field has no ground either.
    let mut free = open_air();
    free.sim_settings.mode = SimMode::FreeField;
    let before = free.simulation_fingerprint();
    set_ground(&mut free, "grass");
    assert_eq!(before, free.simulation_fingerprint());
    assert_ne!(before, open_air().simulation_fingerprint(), "the mode itself is part of it");

    // With walls the ground is not used, so choosing it changes nothing.
    let mut w = walled();
    let before = w.simulation_fingerprint();
    set_ground(&mut w, "grass");
    assert_eq!(before, w.simulation_fingerprint());
}

#[test]
fn choosing_a_ground_is_a_meta_change() {
    let mut doc = Document::new();
    doc.edit(|p| *p = open_air());
    doc.take_changed();
    doc.edit(|p| set_ground(p, "grass"));
    assert_eq!(doc.take_changed(), Areas { geometry: true, results: false, ..Areas::default() });
    assert!(doc.project().materials.contains_key("grass"));
}

#[test]
fn open_air_needs_a_ground_plane_unless_it_is_free_field() {
    let mut p = open_air();
    let id = p.ground_feature().unwrap().id;
    p.remove_feature(id);
    assert!(p.open_air());
    assert!(p.open_air_input().unwrap_err().contains("ground plane"));
    // Free field has no ground to place.
    p.sim_settings.mode = SimMode::FreeField;
    assert!(p.open_air_input().unwrap().ground.is_none());
    // A suppressed ground plane is no ground plane.
    let mut q = open_air();
    let mut f = q.features[0].clone();
    f.suppressed = true;
    q.update_feature(f).unwrap();
    assert!(q.open_air_input().is_err());
}

#[test]
fn a_new_ground_plane_is_asphalt_and_a_room_does_not_use_it() {
    let mut p = Project::default();
    let id = p.add_feature(FeatureKind::Ground { outline: vec![DVec2::ZERO, DVec2::X, DVec2::ONE] }, None);
    assert_eq!(p.features.iter().find(|f| f.id == id).unwrap().material.as_deref(), Some("asphalt"));
    assert!(p.materials.contains_key("asphalt"));
    // With walls the floor is the ground: the plane takes no part.
    let mut w = walled();
    set_ground(&mut w, "grass");
    let mut doc = Document::new();
    doc.edit(|d| *d = w);
    let input = doc.solver_input().unwrap();
    assert!(input.ground.is_none());
    assert!(doc.room().is_ok());
}

#[test]
fn a_room_project_solves_in_its_room() {
    let mut doc = Document::new();
    doc.edit(|p| *p = walled());
    let input = doc.solver_input().unwrap();
    assert!(input.ground.is_none());
    assert!(input.room.contains(DVec3::new(1.0, 1.0, 2.0)));
}
