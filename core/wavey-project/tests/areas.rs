//! What each kind of edit reports as changed, so the app re-reads only
//! those areas.

use glam::{DVec2, DVec3};
use wavey_project::areas::Areas;
use wavey_project::{Document, Project};
use wavey_solver::SimulationResult;
use wavey_speaker::samples;

const NONE: Areas = Areas { geometry: false, sources: false, receivers: false, results: false, meta: false };
const GEOMETRY: Areas = Areas { geometry: true, ..NONE };
const SOURCES: Areas = Areas { sources: true, ..NONE };
const RECEIVERS: Areas = Areas { receivers: true, ..NONE };
const RESULTS: Areas = Areas { results: true, ..NONE };
const META: Areas = Areas { meta: true, ..NONE };

/// A 6 × 4 m room with a speaker model, one speaker and one microphone,
/// with the changes so far taken.
fn doc() -> Document {
    let mut doc = Document::new();
    doc.edit(|p| {
        p.ceiling_height = 3.0;
        p.add_polygon(&[DVec2::new(0.0, 0.0), DVec2::new(6.0, 0.0), DVec2::new(6.0, 4.0), DVec2::new(0.0, 4.0)]);
        p.interior_seeds.insert(1, DVec2::new(1.0, 1.0));
        p.add_speaker_model(samples::piston("p", "Piston", 0.1)).unwrap();
        p.add_speaker("p", DVec3::new(1.0, 1.0, 1.0)).unwrap();
        p.add_microphone(DVec3::new(4.0, 2.0, 1.5));
    });
    doc.room().unwrap();
    doc.take_changed();
    doc
}

fn first_speaker(doc: &Document) -> u64 {
    *doc.project().speakers.keys().next().unwrap()
}

/// Runs `edit` and returns what it changed.
fn changed(doc: &mut Document, edit: impl FnOnce(&mut Project)) -> Areas {
    doc.edit(edit);
    doc.take_changed()
}

fn result_for(doc: &Document) -> SimulationResult {
    SimulationResult {
        freqs: vec![100.0],
        speaker_ids: vec![],
        mics: vec![],
        planes: vec![],
        stats: Default::default(),
        fingerprint: doc.project().simulation_fingerprint(),
        rt_sabine: Default::default(),
        rt_eyring: Default::default(),
        has_late: false,
        fdtd: None,
    }
}

#[test]
fn a_new_or_opened_document_reports_everything_once() {
    let mut doc = Document::new();
    assert_eq!(doc.take_changed(), Areas::ALL);
    assert_eq!(doc.take_changed(), NONE);

    let dir = std::env::temp_dir().join(format!("wavey-areas-{}.wavey", std::process::id()));
    doc.save(&dir).unwrap();
    let mut back = Document::open(&dir).unwrap();
    assert_eq!(back.take_changed(), Areas::ALL);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn moving_a_speaker_changes_sources_and_not_geometry() {
    let mut d = doc();
    let id = first_speaker(&d);
    assert_eq!(changed(&mut d, |p| p.speakers.get_mut(&id).unwrap().position.x = 2.0), SOURCES);
}

#[test]
fn table_of_edits_and_the_areas_they_change() {
    type Edit = Box<dyn Fn(&mut Project)>;
    let cases: Vec<(&str, Edit, Areas)> = vec![
        ("ceiling height", Box::new(|p| p.ceiling_height = 3.5), GEOMETRY),
        ("draw a wall", Box::new(|p| drop(p.add_segment(DVec2::new(1.0, 1.0), DVec2::new(2.0, 1.0)))), GEOMETRY),
        ("assign a material", Box::new(|p| p.default_material = "wood-floor".into()), GEOMETRY),
        ("name a surface", Box::new(|p| drop(p.meta.surface_names.insert("floor:1".into(), "Stage".into()))), GEOMETRY),
        ("add a feature", Box::new(|p| {
            use wavey_project::model::FeatureKind;
            p.add_feature(FeatureKind::Column { centre: DVec2::new(3.0, 2.0), size: DVec2::new(0.4, 0.4), round: false, bottom: 0.0, top: 3.0 }, None);
        }), GEOMETRY),
        ("add a speaker", Box::new(|p| { p.add_speaker("p", DVec3::new(2.0, 2.0, 1.0)).unwrap(); }), SOURCES),
        ("mute a speaker", Box::new(|p| p.speakers.values_mut().for_each(|s| s.muted = true)), SOURCES),
        ("add a group", Box::new(|p| { p.add_group("G", vec![]); }), SOURCES),
        ("add a microphone", Box::new(|p| { p.add_microphone(DVec3::new(3.0, 3.0, 1.5)); }), RECEIVERS),
        ("add a cut plane", Box::new(|p| { p.add_cut_plane(wavey_solver::PlaneAxis::Horizontal, 1.5, 0.5); }), RECEIVERS),
        ("hide a layer", Box::new(|p| p.meta.layers[0].visible = false), META),
        ("current layer", Box::new(|p| p.meta.current_layer = "floor".into()), META),
        ("simulation settings", Box::new(|p| p.sim_settings.max_order = 2), META),
        ("medium", Box::new(|p| p.medium.temperature_c = 25.0), META),
        ("an edit that changes nothing", Box::new(|_| {}), NONE),
        ("a value set to what it was", Box::new(|p| p.ceiling_height = 3.0), NONE),
    ];
    for (what, edit, want) in cases {
        let mut d = doc();
        assert_eq!(changed(&mut d, edit), want, "{what}");
    }
}

#[test]
fn adding_a_speaker_model_changes_sources() {
    let mut d = doc();
    assert_eq!(changed(&mut d, |p| drop(p.add_speaker_model(samples::piston("q", "Other", 0.2)).unwrap())), SOURCES);
}

#[test]
fn undo_and_redo_report_what_the_edit_changed() {
    let mut d = doc();
    let id = first_speaker(&d);
    d.edit(|p| p.speakers.get_mut(&id).unwrap().position.x = 2.0);
    d.take_changed();
    assert!(d.undo());
    assert_eq!(d.take_changed(), SOURCES);
    assert!(d.redo());
    assert_eq!(d.take_changed(), SOURCES);

    d.edit(|p| p.ceiling_height = 4.0);
    d.take_changed();
    assert!(d.undo());
    assert_eq!(d.take_changed(), GEOMETRY);
}

#[test]
fn coalesced_edits_report_each_step() {
    let mut d = doc();
    for h in [3.1, 3.2] {
        d.label_next_edit("Height", Some("height"));
        d.edit(|p| p.ceiling_height = h);
    }
    // The setup edit and one merged step.
    assert_eq!(d.undo_depth(), 2);
    assert_eq!(d.take_changed(), GEOMETRY);
}

#[test]
fn run_changes_report_results() {
    let mut d = doc();
    let r = result_for(&d);
    d.add_run(r);
    assert_eq!(d.take_changed(), RESULTS);
    d.rename_run(1, "Renamed");
    assert_eq!(d.take_changed(), RESULTS);
    d.add_run(result_for(&d));
    d.take_changed();
    assert!(d.set_active_run(1));
    assert_eq!(d.take_changed(), RESULTS);
    assert!(d.delete_run(2));
    assert_eq!(d.take_changed(), RESULTS);
    d.clear_results();
    assert_eq!(d.take_changed(), RESULTS);
}

#[test]
fn an_edit_that_makes_a_run_stale_also_changes_results() {
    let mut d = doc();
    d.add_run(result_for(&d));
    d.take_changed();
    let id = first_speaker(&d);
    // Moving the speaker changes what the run was computed from.
    assert_eq!(changed(&mut d, |p| p.speakers.get_mut(&id).unwrap().position.x = 2.0), Areas { sources: true, results: true, ..NONE });
    assert!(d.results_stale());
    // Renaming it does not.
    assert_eq!(changed(&mut d, |p| p.speakers.get_mut(&id).unwrap().name = "Left".into()), SOURCES);
    // Undoing the move makes the run current again.
    assert!(d.undo() && d.undo());
    assert!(!d.results_stale());
    assert_eq!(d.take_changed(), Areas { sources: true, results: true, ..NONE });
}

#[test]
fn a_run_keeps_the_settings_it_was_solved_with() {
    let mut d = doc();
    // The solve starts with these settings…
    let (settings, parts) = (d.project().sim_settings.clone(), d.project().fingerprint_parts());
    let r = result_for(&d);
    // …and the project is edited while it runs.
    d.edit(|p| p.sim_settings.f_max = 90.0);
    let id = d.add_solved_run(r, settings.clone(), parts);
    let run = d.runs().iter().find(|r| r.meta.id == id).unwrap();
    assert_eq!(run.meta.settings.f_max, settings.f_max);
    assert_ne!(run.meta.settings.f_max, 90.0);
}

#[test]
fn a_plane_left_out_of_the_solve_is_not_solved_and_makes_the_run_stale() {
    let mut d = doc();
    let plane = d.edit(|p| p.add_cut_plane(wavey_solver::PlaneAxis::Horizontal, 1.5, 0.5));
    d.room().unwrap();
    assert_eq!(d.solver_input().unwrap().planes.len(), 1);
    let with = d.project().simulation_fingerprint();
    d.take_changed();
    // Microphones only.
    assert_eq!(changed(&mut d, |p| p.meta.planes.entry(plane).or_default().include_in_solve = false), RECEIVERS);
    assert!(d.solver_input().unwrap().planes.is_empty());
    assert_ne!(d.project().simulation_fingerprint(), with);
    // Back in: the same fingerprint as before.
    d.edit(|p| p.meta.planes.get_mut(&plane).unwrap().include_in_solve = true);
    assert_eq!(d.project().simulation_fingerprint(), with);
}

#[test]
fn the_plane_fingerprint_is_the_one_from_before_the_solve_switch() {
    // Runs solved before the switch existed must stay current: with every
    // plane solved, the planes hash as the plane map did.
    let mut d = doc();
    d.edit(|p| { p.add_cut_plane(wavey_solver::PlaneAxis::Horizontal, 1.5, 0.5); });
    let p = d.project();
    let map: std::collections::BTreeMap<u64, &wavey_solver::CutPlane> = p.solved_planes().map(|c| (c.id, c)).collect();
    assert_eq!(serde_json::to_string(&map).unwrap(), serde_json::to_string(&p.cut_planes).unwrap());
}
