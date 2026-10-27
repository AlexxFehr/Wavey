//! FIR filters on sources and groups.

use std::sync::Arc;

use glam::DVec3;
use wavey_project::Project;
use wavey_speaker::{Fir, samples};

fn project() -> (Project, u64) {
    let mut p = Project::default();
    p.add_speaker_model(samples::piston("p", "Piston", 0.1)).unwrap();
    let id = p.add_speaker("p", DVec3::new(1.0, 1.0, 1.0)).unwrap();
    (p, id)
}

/// `n` samples of delay at 48 kHz.
fn delay(n: usize) -> Arc<Fir> {
    let mut taps = vec![0.0; n + 1];
    taps[n] = 1.0;
    Arc::new(Fir::new("delay", 48_000.0, taps).unwrap())
}

#[test]
fn a_source_fir_is_part_of_its_drive() {
    let (mut p, id) = project();
    let freqs = [100.0, 250.0];
    let plain = p.drives(&[id], &freqs);
    // 48 samples = 1 ms.
    p.speakers.get_mut(&id).unwrap().fir = Some(delay(48));
    let with = p.drives(&[id], &freqs);
    for fi in 0..2 {
        let want = plain[0][fi] * num_complex::Complex64::from_polar(1.0, -2.0 * std::f64::consts::PI * freqs[fi] * 1e-3);
        assert!((with[0][fi] - want).norm() < 1e-9, "{fi}");
    }
}

#[test]
fn a_group_fir_applies_to_each_member_on_top_of_its_own() {
    let (mut p, id) = project();
    let g = p.add_group("G", vec![id]);
    p.speakers.get_mut(&id).unwrap().fir = Some(delay(48));
    p.meta.groups.get_mut(&g).unwrap().fir = Some(delay(96));
    let h = p.drives(&[id], &[250.0])[0][0];
    // 1 ms + 2 ms at 250 Hz: -270 degrees.
    assert!((h.arg() - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{}", h.arg());
    assert!((h.norm() - 1.0).abs() < 1e-9);
}

#[test]
fn a_fir_does_not_make_results_stale() {
    let (mut p, id) = project();
    let before = p.simulation_fingerprint();
    p.speakers.get_mut(&id).unwrap().fir = Some(delay(48));
    let g = p.add_group("G", vec![id]);
    p.meta.groups.get_mut(&g).unwrap().fir = Some(delay(10));
    assert_eq!(p.simulation_fingerprint(), before);
}

#[test]
fn a_settings_round_trip_keeps_the_fir() {
    let (mut p, id) = project();
    p.speakers.get_mut(&id).unwrap().fir = Some(delay(48));
    // The app sends settings back without taps.
    let mut s = p.speakers[&id].clone();
    s.fir = None;
    s.gain_db = -3.0;
    p.update_speaker(s).unwrap();
    assert!(p.speakers[&id].fir.is_some());
    assert_eq!(p.speakers[&id].gain_db, -3.0);
}

#[test]
fn a_fir_survives_saving_and_loading() {
    let (mut p, id) = project();
    p.speakers.get_mut(&id).unwrap().fir = Some(delay(48));
    let back: Project = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(back.speakers[&id].fir.as_deref(), p.speakers[&id].fir.as_deref());
}
