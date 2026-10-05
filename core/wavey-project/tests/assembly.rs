//! Stacks and line arrays: layout, presets, auto-splay and the group
//! operations (insert, update, move, duplicate, copy/paste).

use glam::DVec3;
use wavey_project::Project;
use wavey_project::assembly::*;
use wavey_speaker::{RigFrame, RigPoint, Rigging, samples};

const C: f64 = 343.0;

/// A 30 cm line-array module with hinges at its front, 0.29 m apart, and a
/// fly frame with 10 pick points along its depth.
fn module() -> wavey_speaker::SpeakerModel {
    let mut m = samples::piston("la", "Line module", 0.1);
    m.enclosure = wavey_speaker::Enclosure { width_m: 0.6, height_m: 0.3, depth_m: 0.4 };
    m.rigging = Some(Rigging {
        pivot: [0.18, 0.0, 0.145],
        next_pivot: [0.18, 0.0, -0.145],
        center_of_mass: Some([-0.02, 0.0, 0.0]),
        weight_kg: 30.0,
        splays_deg: (0..=10).map(|d| d as f64).collect(),
        frames: vec![
            RigFrame {
                key: "fly".into(),
                label: "Fly frame".into(),
                flown: true,
                weight_kg: 25.0,
                center_of_mass: Some([-0.35, 0.0, -0.05]),
                attach: [-0.05, 0.0, -0.08],
                pick_points: (0..10).map(|i| RigPoint { label: format!("{}", i + 1), position: [-0.1 - 0.06 * i as f64, 0.0, -0.04] }).collect(),
                splays_deg: (-5..=5).map(|d| d as f64).collect(),
                max_boxes: Some(16),
                geometry: None,
            },
            RigFrame {
                key: "ground".into(),
                label: "Ground stack".into(),
                flown: false,
                weight_kg: 25.0,
                center_of_mass: None,
                attach: [0.18, 0.0, 0.0],
                pick_points: vec![],
                splays_deg: (-5..=5).map(|d| d as f64).collect(),
                max_boxes: Some(4),
                geometry: None,
            },
        ],
        presets: vec![],
    });
    m
}

fn project() -> Project {
    let mut p = Project::default();
    p.add_speaker_model(module()).unwrap();
    p.add_speaker_model(samples::piston("sub", "Sub", 0.2)).unwrap();
    p
}

fn array(n: usize, splay: f64, tilt: ArrayTilt) -> Assembly {
    Assembly {
        origin: DVec3::new(2.0, 3.0, 8.0),
        yaw_deg: 0.0,
        kind: AssemblyKind::LineArray(LineArrayRecipe {
            model_id: "la".into(),
            frame: Some("fly".into()),
            frame_splay_deg: 0.0,
            splays_deg: vec![splay; n - 1],
            tilt,
            elements: vec![ElementSettings::default(); n],
            auto: None,
        }),
    }
}

#[test]
fn boxes_hang_from_each_other() {
    let p = project();
    let m = module();
    let rig = m.rigging.clone().unwrap();
    let a = array(8, 3.0, ArrayTilt::Manual { deg: 2.0 });
    let lay = layout(&a, &p.speaker_models).unwrap();
    assert_eq!(lay.elements.len(), 8);
    assert!(lay.warnings.is_empty(), "{:?}", lay.warnings);
    for k in 0..8 {
        // Box angle: frame tilt + Σ splays.
        assert!((lay.box_tilt_deg[k] - (2.0 + 3.0 * k as f64)).abs() < 1e-9);
        assert!((lay.elements[k].pitch_deg + lay.box_tilt_deg[k]).abs() < 1e-9);
    }
    // Each box's top hinge sits on the bottom hinge of the box above.
    for k in 1..8 {
        let (up, low) = (&lay.elements[k - 1], &lay.elements[k]);
        let hinge_up = up.position + up.rotation() * DVec3::from_array(rig.next_pivot);
        let hinge_low = low.position + low.rotation() * DVec3::from_array(rig.pivot);
        assert!((hinge_up - hinge_low).length() < 1e-9);
    }
    // The array hangs below the frame and curves away (later boxes further back and lower).
    assert!(lay.elements.windows(2).all(|w| w[1].position.z < w[0].position.z));
    assert!(lay.elements[7].position.x < lay.elements[0].position.x);
}

#[test]
fn yaw_and_origin_move_the_array_rigidly() {
    let p = project();
    let a = array(6, 2.0, ArrayTilt::Manual { deg: 0.0 });
    let mut b = a.clone();
    b.origin += DVec3::new(1.0, -2.0, 0.5);
    b.yaw_deg = 90.0;
    let (la, lb) = (layout(&a, &p.speaker_models).unwrap(), layout(&b, &p.speaker_models).unwrap());
    let rot = glam::DMat3::from_rotation_z(90f64.to_radians());
    for (ea, eb) in la.elements.iter().zip(&lb.elements) {
        let expect = b.origin + rot * (ea.position - a.origin);
        assert!((eb.position - expect).length() < 1e-9);
        assert!((eb.yaw_deg - 90.0).abs() < 1e-9);
    }
}

#[test]
fn pick_point_tilt_balances_the_centre_of_mass() {
    let p = project();
    // Picking further back tilts the array further down.
    let tilts: Vec<f64> = [0usize, 4, 9]
        .iter()
        .map(|&i| layout(&array(10, 2.0, ArrayTilt::Pick { index: i }), &p.speaker_models).unwrap().frame_tilt_deg)
        .collect();
    assert!(tilts[0] < tilts[1] && tilts[1] < tilts[2], "{tilts:?}");
    // The combined centre of mass is straight below the pick point.
    let m = module();
    let rig = m.rigging.unwrap();
    let a = array(10, 2.0, ArrayTilt::Pick { index: 4 });
    let lay = layout(&a, &p.speaker_models).unwrap();
    let f = lay.frame.as_ref().unwrap();
    let fly = &rig.frames[0];
    let mut moment = (f.reference + f.rotation * DVec3::from_array(fly.center_of_mass.unwrap())) * fly.weight_kg;
    let mut w = fly.weight_kg;
    for e in &lay.elements {
        moment += (e.position + e.rotation() * DVec3::from_array(rig.center_of_mass.unwrap())) * rig.weight_kg;
        w += rig.weight_kg;
    }
    let com = moment / w;
    let pick = f.reference + f.rotation * DVec3::from_array(fly.pick_points[4].position);
    assert!((pick - a.origin).length() < 1e-9, "the origin is the pick point");
    assert!((com.x - pick.x).abs() < 1e-6 && (com.y - pick.y).abs() < 1e-6 && com.z < pick.z);
}

#[test]
fn ground_stack_builds_upwards() {
    let p = project();
    let mut a = array(3, 4.0, ArrayTilt::Manual { deg: 0.0 });
    a.origin = DVec3::new(0.0, 0.0, 0.0);
    if let AssemblyKind::LineArray(r) = &mut a.kind {
        r.frame = Some("ground".into());
    }
    let lay = layout(&a, &p.speaker_models).unwrap();
    assert!(lay.elements.windows(2).all(|w| w[0].position.z > w[1].position.z), "top box first");
    assert!(lay.elements.iter().all(|e| e.position.z > 0.0));
    // The bottom box stands level on the frame; the ones above tilt up.
    assert!(lay.box_tilt_deg[2].abs() < 1e-9 && (lay.box_tilt_deg[0] + 8.0).abs() < 1e-9);
}

#[test]
fn limits_are_reported() {
    let p = project();
    let lay = layout(&array(18, 3.5, ArrayTilt::Manual { deg: 0.0 }), &p.speaker_models).unwrap();
    assert!(lay.warnings.iter().any(|w| w.contains("limit of 16")));
    assert!(lay.warnings.iter().any(|w| w.contains("3.5°")));
}

#[test]
fn auto_splay_aims_down_the_audience() {
    let p = project();
    let mut a = array(12, 0.0, ArrayTilt::Manual { deg: 0.0 });
    a.origin = DVec3::new(0.0, 0.0, 9.0);
    if let AssemblyKind::LineArray(r) = &mut a.kind {
        r.auto = Some(AutoSplay { first_m: 5.0, last_m: 30.0, listener_z: 1.2, rake_deg: 0.0 });
    }
    auto_splay(&mut a, &p.speaker_models).unwrap();
    let AssemblyKind::LineArray(r) = &a.kind else { unreachable!() };
    // Allowed values, small at the top (far throw), large at the bottom.
    assert!(r.splays_deg.iter().all(|s| s.fract() == 0.0 && (0.0..=10.0).contains(s)), "{:?}", r.splays_deg);
    assert!(r.splays_deg.first() <= r.splays_deg.last());
    assert!(r.splays_deg.iter().sum::<f64>() > 10.0, "{:?}", r.splays_deg);
    // The top box points near the last row, the bottom box near the first.
    let lay = layout(&a, &p.speaker_models).unwrap();
    let hit = |k: usize| {
        let e = &lay.elements[k];
        let axis = e.rotation() * DVec3::X;
        let t = (1.2 - e.position.z) / axis.z;
        (e.position + axis * t).x - a.origin.x
    };
    assert!((hit(0) - 30.0).abs() < 6.0, "top box lands at {:.1} m", hit(0));
    assert!((hit(11) - 5.0).abs() < 2.5, "bottom box lands at {:.1} m", hit(11));
    let hits: Vec<f64> = (0..12).map(hit).collect();
    assert!(hits.windows(2).all(|w| w[1] <= w[0] + 0.5), "aim points move forward down the array: {hits:?}");
}

#[test]
fn stack_presets() {
    let p = project();
    let m = &p.speaker_models;
    let ef = StackPreset::Endfire { model_id: "sub".into(), count: 4, spacing_m: 1.0 }.generate(m, C).unwrap();
    for (i, e) in ef.iter().enumerate() {
        assert!((e.settings.delay_ms - i as f64 / C * 1e3).abs() < 1e-9);
    }
    assert!(ef.windows(2).all(|w| w[1].offset.x > w[0].offset.x), "front-most element last (most delay)");
    let col = StackPreset::Column { model_id: "sub".into(), count: 3, columns: 1 }.generate(m, C).unwrap();
    assert_eq!(col.len(), 3);
    let wide = StackPreset::Column { model_id: "sub".into(), count: 2, columns: 2 }.generate(m, C).unwrap();
    assert_eq!(wide.len(), 4);
    assert!(wide[0].offset.y < 0.0 && wide[2].offset.y > 0.0 && (wide[0].offset.y + wide[2].offset.y).abs() < 1e-9, "columns side by side, centred");
    assert!(col.iter().all(|e| e.yaw_deg == 0.0 && !e.settings.polarity_inverted && e.settings.delay_ms == 0.0));
    assert!(col[1].offset.z > col[0].offset.z && col[2].offset.z > col[1].offset.z);
    let card = StackPreset::Cardioid { model_id: "sub".into(), forward: 2, reversed: 1 }.generate(m, C).unwrap();
    assert_eq!(card.len(), 3);
    assert!(card[2].settings.polarity_inverted && (card[2].yaw_deg - 180.0).abs() < 1e-9 && card[2].settings.delay_ms > 0.0);
    assert!(card[2].offset.z > card[1].offset.z && card[1].offset.z > card[0].offset.z);
    let bs = StackPreset::Broadside { model_id: "sub".into(), count: 5, spacing_m: 1.0, arc_deg: 60.0 }.generate(m, C).unwrap();
    assert!(bs[2].settings.delay_ms > bs[1].settings.delay_ms && bs[1].settings.delay_ms > bs[0].settings.delay_ms);
    assert!(bs[0].settings.delay_ms.abs() < 1e-12 && (bs[0].settings.delay_ms - bs[4].settings.delay_ms).abs() < 1e-12);
}

#[test]
fn insert_update_move_and_undo() {
    let mut p = project();
    let a = array(6, 2.0, ArrayTilt::Manual { deg: 1.0 });
    let g = p.insert_assembly("Main L", a.clone()).unwrap();
    let members = p.meta.groups[&g].members.clone();
    assert_eq!(members.len(), 6);
    assert!(p.assembly_in_sync(g));
    // More boxes: the first six keep their ids.
    let mut b = a.clone();
    if let AssemblyKind::LineArray(r) = &mut b.kind {
        r.set_count(9);
    }
    p.update_assembly(g, b.clone()).unwrap();
    let now = p.meta.groups[&g].members.clone();
    assert_eq!(now.len(), 9);
    assert_eq!(&now[..6], &members[..]);
    // Fewer: the tail is removed.
    if let AssemblyKind::LineArray(r) = &mut b.kind {
        r.set_count(4);
    }
    p.update_assembly(g, b).unwrap();
    assert_eq!(p.meta.groups[&g].members.len(), 4);
    assert!(!p.speakers.contains_key(&now[8]));
    // Moving the whole array.
    p.transform_assembly(g, DVec3::new(5.0, 5.0, 7.0), 45.0).unwrap();
    assert!(p.assembly_in_sync(g));
    let first = p.meta.groups[&g].members[0];
    assert!((p.speakers[&first].yaw_deg - 45.0).abs() < 1e-9);
    // A member moved on its own: no longer where the recipe puts it.
    let mut s = p.speakers[&first].clone();
    s.position.z += 0.5;
    p.update_speaker(s).unwrap();
    assert!(!p.assembly_in_sync(g));
    // Settings edits go into the recipe.
    let mut s = p.speakers[&first].clone();
    s.gain_db = -3.0;
    p.update_speaker(s).unwrap();
    p.sync_assembly_settings(first);
    let AssemblyKind::LineArray(r) = &p.meta.groups[&g].assembly.as_ref().unwrap().kind else { unreachable!() };
    assert_eq!(r.elements[0].gain_db, -3.0);
}

#[test]
fn document_undo_restores_the_array() {
    let mut doc = wavey_project::Document::default();
    doc.edit(|p| {
        p.add_speaker_model(module()).unwrap();
    });
    let g = doc.edit(|p| p.insert_assembly("Main", array(5, 2.0, ArrayTilt::Manual { deg: 0.0 })).unwrap());
    assert_eq!(doc.project().speakers.len(), 5);
    doc.undo();
    assert!(doc.project().meta.groups.get(&g).is_none() && doc.project().speakers.is_empty());
    doc.redo();
    assert_eq!(doc.project().meta.groups[&g].members.len(), 5);
}

#[test]
fn duplicate_and_paste_keep_the_recipe() {
    let mut p = project();
    let g = p.insert_assembly("Main L", array(6, 2.0, ArrayTilt::Manual { deg: 1.0 })).unwrap();
    let d = p.duplicate_group(g, DVec3::new(0.0, 10.0, 0.0)).unwrap();
    assert_eq!(p.meta.groups[&d].name, "Main L copy");
    assert!(p.assembly_in_sync(d), "the copy's members match its moved recipe");
    let (a, b) = (&p.meta.groups[&g], &p.meta.groups[&d]);
    assert_eq!(b.assembly.as_ref().unwrap().origin, a.assembly.as_ref().unwrap().origin + DVec3::new(0.0, 10.0, 0.0));
    assert!(a.members.iter().all(|m| !b.members.contains(m)));
    // Into another project that lacks the model.
    let clip = p.clip(&[g], &[]).unwrap();
    let mut other = Project::default();
    let (groups, loose) = other.paste_clip(&clip, DVec3::ZERO).unwrap();
    assert_eq!((groups.len(), loose.len()), (1, 0));
    assert!(other.speaker_models.contains_key("la"));
    assert!(other.assembly_in_sync(groups[0]));
    // Save/load keeps the recipe.
    let json = serde_json::to_string(&other).unwrap();
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.meta.groups[&groups[0]].assembly, other.meta.groups[&groups[0]].assembly);
}

#[test]
fn stack_insert_matches_its_preset() {
    let mut p = project();
    let elements = StackPreset::Cardioid { model_id: "sub".into(), forward: 2, reversed: 1 }.generate(&p.speaker_models, C).unwrap();
    let a = Assembly {
        origin: DVec3::new(4.0, 2.0, 0.0),
        yaw_deg: 180.0,
        kind: AssemblyKind::Stack(StackRecipe { preset: StackPreset::Cardioid { model_id: "sub".into(), forward: 2, reversed: 1 }, elements }),
    };
    let g = p.insert_assembly("Sub L", a).unwrap();
    let m: Vec<_> = p.meta.groups[&g].members.iter().map(|id| p.speakers[id].clone()).collect();
    assert_eq!(m.len(), 3);
    assert!((m[0].yaw_deg - 180.0).abs() < 1e-9 && (m[2].yaw_deg - 360.0).abs() < 1e-9);
    assert!(m[2].polarity_inverted && !m[0].polarity_inverted);
    assert!(m.iter().all(|s| (s.position.x - 4.0).abs() < 1e-9 && (s.position.y - 2.0).abs() < 1e-9));
}

#[test]
fn endfire_prediction_matches_the_coherent_sum() {
    let mut p = Project::default();
    p.add_speaker_model(samples::omni()).unwrap();
    let medium = wavey_materials::Medium::default();
    let c = medium.speed_of_sound();
    let id = p.speaker_models.keys().next().unwrap().clone();
    let elements = StackPreset::Endfire { model_id: id.clone(), count: 4, spacing_m: 1.0 }.generate(&p.speaker_models, c).unwrap();
    let a = Assembly { origin: DVec3::ZERO, yaw_deg: 0.0, kind: AssemblyKind::Stack(StackRecipe { preset: StackPreset::Custom, elements: elements.clone() }) };
    let freqs = [31.5, 50.0, 80.0, 125.0];
    let (angles, front) = predict_polar(&a, &p.speaker_models, &medium, &freqs, PolarPlane::Horizontal, 30.0, 90.0, 0.0).unwrap();
    assert_eq!(angles, vec![-180.0, -90.0, 0.0, 90.0, 180.0]);
    let lay = layout(&a, &p.speaker_models).unwrap();
    let m = &p.speaker_models[&id];
    let centre = lay.elements.iter().map(|e| e.position + e.rotation() * DVec3::from_array(m.acoustic_center_m)).sum::<DVec3>() / 4.0;
    for (fi, f) in freqs.iter().enumerate() {
        for (ai, deg) in angles.iter().enumerate() {
            let t = (*deg as f64).to_radians();
            let x = centre + DVec3::new(t.cos(), t.sin(), 0.0) * 30.0;
            let k = 2.0 * std::f64::consts::PI * f / c;
            let air = medium.air_intensity_coefficient(*f) / 2.0;
            let sum: num_complex::Complex64 = lay
                .elements
                .iter()
                .map(|e| {
                    let r = (x - (e.position + e.rotation() * DVec3::from_array(m.acoustic_center_m))).length();
                    m.on_axis(*f).norm() * num_complex::Complex64::from_polar(m.reference.distance_m * (-air * r).exp() / r, -k * r - 2.0 * std::f64::consts::PI * f * e.settings.delay_ms * 1e-3)
                })
                .sum();
            let expect = 20.0 * (sum.norm() / 20e-6).log10();
            assert!((front[fi][ai] - expect).abs() < 0.05, "{f} Hz {deg}°: {} vs {expect}", front[fi][ai]);
        }
        // Endfire: front well above rear.
        assert!(front[fi][2] - front[fi][0] > 6.0, "{f} Hz: front {} rear {}", front[fi][2], front[fi][0]);
    }
}

#[test]
fn tuning_a_cardioid_stack_cancels_the_rear() {
    let mut p = Project::default();
    // A sub whose rear output is 4 dB down, with its acoustic centre at the front.
    let mut sub = samples::with_directivity("sub", "Sub", |_, d: DVec3| num_complex::Complex64::new(if d.x >= 0.0 { 1.0 } else { 0.63 }, 0.0));
    sub.enclosure = wavey_speaker::Enclosure { width_m: 0.6, height_m: 0.6, depth_m: 0.7 };
    sub.acoustic_center_m = [0.3, 0.0, 0.0];
    p.add_speaker_model(sub).unwrap();
    let medium = wavey_materials::Medium::default();
    let preset = StackPreset::Cardioid { model_id: "sub".into(), forward: 2, reversed: 1 };
    let elements = preset.generate(&p.speaker_models, medium.speed_of_sound()).unwrap();
    let mut a = Assembly { origin: DVec3::ZERO, yaw_deg: 0.0, kind: AssemblyKind::Stack(StackRecipe { preset, elements }) };
    let rr = tune_rear(&mut a, &p.speaker_models, &medium, 40.0, 100.0).unwrap();
    assert!(rr < -15.0, "rear − front after tuning: {rr:.1} dB");
    let AssemblyKind::Stack(r) = &a.kind else { unreachable!() };
    assert!(r.elements[2].settings.delay_ms > 0.0 && r.elements[0].settings.delay_ms == 0.0);
}

#[test]
fn group_eq_applies_when_mixing_and_needs_no_solve() {
    let mut p = project();
    let g = p.insert_assembly("Main", array(4, 2.0, ArrayTilt::Manual { deg: 0.0 })).unwrap();
    let ids = p.meta.groups[&g].members.clone();
    let before = p.drives(&ids, &[100.0, 1000.0]);
    let fp = p.simulation_fingerprint();
    p.meta.groups.get_mut(&g).unwrap().filters = vec![wavey_speaker::Filter::Peak { freq_hz: 1000.0, gain_db: -6.0, q: 1.0 }];
    let after = p.drives(&ids, &[100.0, 1000.0]);
    assert_eq!(p.simulation_fingerprint(), fp, "group EQ is a mix-time setting");
    let db = |c: num_complex::Complex64| 20.0 * c.norm().log10();
    assert!((db(after[0][1]) - db(before[0][1]) + 6.0).abs() < 1e-6);
    assert!((db(after[0][0]) - db(before[0][0])).abs() < 0.5);
    // Receivers in a group survive a copy and paste.
    let m = p.add_microphone(DVec3::new(5.0, 3.0, 1.2));
    p.meta.groups.get_mut(&g).unwrap().receivers = vec![m];
    let clip = p.clip(&[g], &[]).unwrap();
    let (gs, _) = p.paste_clip(&clip, DVec3::new(0.0, 5.0, 0.0)).unwrap();
    assert_eq!(p.meta.groups[&gs[0]].receivers.len(), 1);
    assert_ne!(p.meta.groups[&gs[0]].receivers[0], m);
    assert_eq!(p.meta.groups[&gs[0]].filters.len(), 1);
}

#[test]
fn group_gain_delay_and_polarity_mix_without_touching_members() {
    let mut p = project();
    let g = p.insert_assembly("Main", array(4, 2.0, ArrayTilt::Manual { deg: 0.0 })).unwrap();
    let ids = p.meta.groups[&g].members.clone();
    let gains: Vec<f64> = ids.iter().map(|i| p.speakers[i].gain_db).collect();
    let before = p.drives(&ids, &[1000.0]);
    let fp = p.simulation_fingerprint();
    let grp = p.meta.groups.get_mut(&g).unwrap();
    (grp.gain_db, grp.delay_ms, grp.polarity_inverted) = (-6.0, 0.25, true);
    let after = p.drives(&ids, &[1000.0]);
    assert_eq!(p.simulation_fingerprint(), fp, "group signal settings are mix-time");
    assert_eq!(ids.iter().map(|i| p.speakers[i].gain_db).collect::<Vec<_>>(), gains, "members keep their gain");
    // −6 dB, a quarter period late at 1 kHz (−90°), inverted (+180°): +90° in all.
    let ratio = after[0][0] / before[0][0];
    assert!((20.0 * ratio.norm().log10() + 6.0).abs() < 1e-9);
    assert!((ratio.arg().to_degrees() - 90.0).abs() < 1e-6, "phase {}", ratio.arg().to_degrees());
    // They survive a copy and paste.
    let clip = p.clip(&[g], &[]).unwrap();
    let (gs, _) = p.paste_clip(&clip, DVec3::new(0.0, 5.0, 0.0)).unwrap();
    let pasted = &p.meta.groups[&gs[0]];
    assert_eq!((pasted.gain_db, pasted.delay_ms, pasted.polarity_inverted), (-6.0, 0.25, true));
}
