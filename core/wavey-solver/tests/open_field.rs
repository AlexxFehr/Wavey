//! Free field (anechoic) and open air with a ground: direct sound, and the
//! ground's reflection, with nothing else in the way.

mod common;

use std::collections::BTreeMap;
use std::f64::consts::PI;

use common::*;
use glam::{DVec2, DVec3};
use num_complex::Complex64;
use wavey_geom::{Sketch, build_room};
use wavey_materials::Medium;
use wavey_solver::*;

/// Pressure at mic 0 for a unit drive, per frequency.
fn pressure(r: &SimulationResult) -> Vec<Complex64> {
    r.mic_pressure(0, &unit_drives(r))
}

fn free_field(mut inp: SimulationInput) -> SimulationInput {
    inp.settings.mode = SimMode::FreeField;
    inp
}

fn hall() -> wavey_geom::Room {
    shoebox(30.0, 20.0, 10.0)
}

/// 30 × 20 m hall with a 2 m pillar block at x 14–16, y 8–12.
fn hall_with_pillar() -> wavey_geom::Room {
    let mut s = Sketch::new();
    s.add_polygon(&[DVec2::new(0.0, 0.0), DVec2::new(30.0, 0.0), DVec2::new(30.0, 20.0), DVec2::new(0.0, 20.0)]);
    s.add_polygon(&[DVec2::new(14.0, 8.0), DVec2::new(16.0, 8.0), DVec2::new(16.0, 12.0), DVec2::new(14.0, 12.0)]);
    build_room(&s, 10.0, &BTreeMap::from([(1, DVec2::new(2.0, 2.0))])).unwrap()
}

#[test]
fn free_field_falls_6_db_per_doubling_with_the_right_phase() {
    let src = DVec3::new(5.0, 10.0, 3.0);
    let (near, far) = (DVec3::new(10.0, 10.0, 3.0), DVec3::new(15.0, 10.0, 3.0));
    let r = run(&free_field(input(hall(), 0.3, omni_at(src), &[near, far], 3, (100.0, 400.0))));
    let d = unit_drives(&r);
    let (a, b) = (r.mic_pressure(0, &d), r.mic_pressure(1, &d));
    for (fi, f) in r.freqs.iter().enumerate() {
        // 5 m and 10 m: half the pressure (air absorption is negligible here).
        let ratio = b[fi].norm() / a[fi].norm();
        assert!((ratio - 0.5).abs() < 0.01, "{f} Hz: {ratio}");
        // The extra 5 m of path delays the wave by k · 5 m.
        let k = 2.0 * PI * f / Medium::default().speed_of_sound();
        let phase = (b[fi] / a[fi]).arg() + k * 5.0;
        assert!(((phase + PI).rem_euclid(2.0 * PI) - PI).abs() < 0.05, "{f} Hz: {phase}");
    }
}

#[test]
fn free_field_equals_the_direct_sound_of_an_open_room() {
    let src = DVec3::new(5.0, 10.0, 3.0);
    let mic = DVec3::new(12.0, 6.0, 1.5);
    // Room mode with no reflections and nothing in the way is the direct sound.
    let room = pressure(&run(&input(hall(), 0.3, omni_at(src), &[mic], 0, (100.0, 400.0))));
    let free = pressure(&run(&free_field(input(hall(), 0.3, omni_at(src), &[mic], 3, (100.0, 400.0)))));
    for (a, b) in room.iter().zip(&free) {
        assert!((a - b).norm() / a.norm() < 1e-4, "{a} vs {b}");
    }
}

#[test]
fn free_field_does_not_care_about_obstacles_or_where_the_speaker_stands() {
    let src = DVec3::new(10.0, 10.0, 3.0);
    let behind = DVec3::new(20.0, 10.0, 3.0);
    let blocked = input(hall_with_pillar(), 0.3, omni_at(src), &[behind], 0, (100.0, 400.0));
    let in_the_open = input(hall(), 0.3, omni_at(src), &[behind], 0, (100.0, 400.0));
    // The pillar is in the way in the room…
    let shadow = pressure(&run(&blocked));
    let clear = pressure(&run(&in_the_open));
    assert!(shadow.iter().zip(&clear).all(|(s, c)| s.norm() < 0.1 * c.norm()));
    // …and not in free field.
    let free = pressure(&run(&free_field(blocked.clone())));
    for (a, b) in clear.iter().zip(&free) {
        assert!((a - b).norm() / a.norm() < 1e-4);
    }
    // A speaker outside the room is fine in free field.
    let mut outside = free_field(blocked);
    outside.sources[0].instance.position = DVec3::new(-5.0, 10.0, 3.0);
    assert!(simulate(&outside, &|_, _| {}, &std::sync::atomic::AtomicBool::new(false)).is_ok());
}

#[test]
fn free_field_ignores_the_ground() {
    let src = DVec3::new(5.0, 10.0, 2.0);
    let mic = DVec3::new(15.0, 10.0, 1.5);
    let mut with_ground = free_field(input(hall(), 0.3, omni_at(src), &[mic], 0, (100.0, 400.0)));
    with_ground.ground = Some(material(0.02));
    let without = free_field(input(hall(), 0.3, omni_at(src), &[mic], 0, (100.0, 400.0)));
    assert_eq!(pressure(&run(&with_ground)), pressure(&run(&without)));
}

#[test]
fn over_a_hard_ground_the_direct_and_reflected_paths_add() {
    let src = DVec3::new(5.0, 10.0, 2.0);
    let mic = DVec3::new(15.0, 10.0, 1.5);
    let mut open = input(hall(), 0.3, omni_at(src), &[mic], 0, (100.0, 800.0));
    open.ground = Some(material(0.001));
    let with_ground = pressure(&run(&open));
    let direct = pressure(&run(&free_field(open.clone())));
    let (r1, r2) = ((mic - src).length(), (mic - DVec3::new(src.x, src.y, -src.z)).length());
    let freqs = run(&open).freqs;
    for (fi, f) in freqs.iter().enumerate() {
        let k = 2.0 * PI * f / Medium::default().speed_of_sound();
        // Pressure of the two paths, 1/r each, the second one delayed and a rigid reflection.
        let expected = (Complex64::new(1.0 / r1, 0.0) + Complex64::from_polar(1.0 / r2, -k * (r2 - r1))).norm() / (1.0 / r1);
        let got = with_ground[fi].norm() / direct[fi].norm();
        let db = 20.0 * (got / expected).log10();
        assert!(db.abs() < 0.5, "{f} Hz: {db:+.2} dB (got {got:.3}, expected {expected:.3})");
    }
}

#[test]
fn a_soft_ground_reflects_less_than_a_hard_one() {
    let src = DVec3::new(5.0, 10.0, 2.0);
    let mic = DVec3::new(15.0, 10.0, 1.5);
    let level = |alpha: f64| {
        let mut inp = input(hall(), 0.3, omni_at(src), &[mic], 0, (100.0, 800.0));
        inp.ground = Some(material(alpha));
        let free = pressure(&run(&free_field(inp.clone())));
        let g = pressure(&run(&inp));
        // How far the ground moves the level from the free field, summed over the band.
        g.iter().zip(&free).map(|(a, b)| (20.0 * (a.norm() / b.norm()).log10()).abs()).sum::<f64>()
    };
    assert!(level(0.001) > level(0.8) + 2.0);
}

#[test]
fn the_mode_is_only_stored_when_it_is_not_the_room() {
    let mut s = SimSettings::default();
    assert!(!serde_json::to_string(&s).unwrap().contains("mode"));
    s.mode = SimMode::FreeField;
    assert!(serde_json::to_string(&s).unwrap().contains("\"mode\":\"free_field\""));
    let old = r#"{"f_min":20.0,"f_max":200.0,"resolution":{"kind":"fractional_octave","n":3},"max_order":2}"#;
    assert_eq!(serde_json::from_str::<SimSettings>(old).unwrap().mode, SimMode::Room);
}

#[test]
fn a_ground_patch_changes_the_reflection_only_where_it_reflects() {
    // Source at x = 5, mic at x = 15, both on y = 10: the ground reflection
    // point is a bit past x = 9, nearer the source.
    let src = DVec3::new(5.0, 10.0, 2.0);
    let mic = DVec3::new(15.0, 10.0, 1.5);
    let square = |x0: f64, x1: f64| vec![DVec2::new(x0, 8.0), DVec2::new(x1, 8.0), DVec2::new(x1, 12.0), DVec2::new(x0, 12.0)];
    let level = |patch: Option<Vec<DVec2>>| {
        let mut inp = input(hall(), 0.3, omni_at(src), &[mic], 0, (100.0, 800.0));
        inp.ground = Some(material(0.001));
        if let Some(outline) = patch {
            inp.ground_patches = vec![GroundPatch { outline, material: material(0.95) }];
        }
        pressure(&run(&inp))
    };
    let hard = level(None);
    let under_the_bounce = level(Some(square(7.0, 12.0)));
    let elsewhere = level(Some(square(18.0, 22.0)));
    // A patch away from the bounce changes nothing…
    assert_eq!(hard, elsewhere);
    // …and one under it takes most of the reflection away.
    let direct = pressure(&run(&free_field({
        let mut inp = input(hall(), 0.3, omni_at(src), &[mic], 0, (100.0, 800.0));
        inp.ground = Some(material(0.001));
        inp
    })));
    let far_from_hard = hard.iter().zip(&direct).map(|(h, d)| (h.norm() / d.norm() - 1.0).abs()).sum::<f64>();
    let far_from_soft = under_the_bounce.iter().zip(&direct).map(|(h, d)| (h.norm() / d.norm() - 1.0).abs()).sum::<f64>();
    assert!(far_from_soft < 0.5 * far_from_hard, "{far_from_soft} vs {far_from_hard}");
}
