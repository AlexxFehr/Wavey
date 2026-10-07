//! Image-source solver against analytic solutions.

mod common;

use common::*;
use glam::DVec3;
use num_complex::Complex64;
use wavey_materials::Medium;
use wavey_solver::*;

const P90: f64 = 0.632_455_532; // 90 dB SPL in Pa (20 µPa · 10^(90/20))

#[test]
fn free_field_matches_point_source() {
    let src = DVec3::new(2.0, 3.0, 1.5);
    let mic = DVec3::new(7.0, 4.0, 1.2);
    let inp = input(shoebox(10.0, 8.0, 3.0), 1.0, omni_at(src), &[mic], 3, (100.0, 10000.0));
    let res = run(&inp);
    let p = res.mic_pressure(0, &unit_drives(&res));
    let m = Medium::default();
    let r = (mic - src).length();
    for (fi, f) in res.freqs.iter().enumerate() {
        let k = 2.0 * std::f64::consts::PI * f / m.speed_of_sound();
        let want = P90 / r * m.air_amplitude(*f, r) * Complex64::from_polar(1.0, -k * r);
        let rel = (p[fi] - want).norm() / want.norm();
        assert!(rel < 1e-5, "{f} Hz: {} vs {}", p[fi], want);
    }
}

/// Allen–Berkley lattice of images for a rectangular room.
fn lattice(l: [f64; 3], s: DVec3, x: DVec3, beta: f64, max_order: i32, f: f64, m: &Medium) -> Complex64 {
    let k = 2.0 * std::f64::consts::PI * f / m.speed_of_sound();
    let s = [s.x, s.y, s.z];
    let x = [x.x, x.y, x.z];
    let mut sum = Complex64::new(0.0, 0.0);
    let n = max_order;
    for nx in -n..=n {
        for ny in -n..=n {
            for nz in -n..=n {
                for ux in 0..2 {
                    for uy in 0..2 {
                        for uz in 0..2 {
                            let ns = [nx, ny, nz];
                            let us = [ux, uy, uz];
                            let mut order = 0;
                            let mut d2 = 0.0;
                            for a in 0..3 {
                                let img = (1 - 2 * us[a]) as f64 * s[a] + 2.0 * ns[a] as f64 * l[a];
                                d2 += (img - x[a]).powi(2);
                                order += (ns[a] - us[a]).abs() + ns[a].abs();
                            }
                            if order > max_order {
                                continue;
                            }
                            let d = d2.sqrt();
                            sum += beta.powi(order) * P90 / d * m.air_amplitude(f, d) * Complex64::from_polar(1.0, -k * d);
                        }
                    }
                }
            }
        }
    }
    sum
}

#[test]
fn shoebox_matches_lattice_sum() {
    let (l, alpha) = ([6.0, 4.5, 3.0], 0.19); // |R| = 0.9
    let src = DVec3::new(1.3, 1.1, 1.4);
    let mic = DVec3::new(4.6, 3.2, 1.7);
    for order in [1, 2, 4] {
        let inp = input(shoebox(l[0], l[1], l[2]), alpha, omni_at(src), &[mic], order as u32, (63.0, 4000.0));
        let res = run(&inp);
        let p = res.mic_pressure(0, &unit_drives(&res));
        for (fi, f) in res.freqs.iter().enumerate() {
            let want = lattice(l, src, mic, 0.9, order, *f, &Medium::default());
            let rel = (p[fi] - want).norm() / want.norm();
            assert!(rel < 1e-4, "order {order}, {f} Hz: {} vs {} (rel {rel})", p[fi], want);
        }
    }
}

#[test]
fn pillar_blocks_direct_sound() {
    use glam::DVec2;
    use std::collections::BTreeMap;
    let mut s = wavey_geom::Sketch::new();
    s.add_polygon(&[DVec2::new(0.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(10.0, 6.0), DVec2::new(0.0, 6.0)]);
    s.add_polygon(&[DVec2::new(4.5, 2.5), DVec2::new(5.5, 2.5), DVec2::new(5.5, 3.5), DVec2::new(4.5, 3.5)]);
    let room = wavey_geom::build_room(&s, 3.0, &BTreeMap::from([(1, DVec2::new(1.0, 1.0))])).unwrap();
    let src = DVec3::new(2.0, 3.0, 1.5);
    let behind = DVec3::new(8.0, 3.0, 1.5);
    let visible = DVec3::new(8.0, 1.0, 1.5);
    let inp = input(room, 1.0, omni_at(src), &[behind, visible], 2, (500.0, 1000.0));
    let res = run(&inp);
    let d = unit_drives(&res);
    assert!(res.mic_pressure(0, &d).iter().all(|p| p.norm() < 1e-9));
    assert!(res.mic_pressure(1, &d).iter().all(|p| p.norm() > 0.05));
}

#[test]
fn pillar_reflection_appears_when_rigid() {
    // With a rigid pillar and anechoic walls, a receiver beside the pillar
    // gets a first-order reflection from the pillar face.
    use glam::DVec2;
    use std::collections::BTreeMap;
    let mut s = wavey_geom::Sketch::new();
    s.add_polygon(&[DVec2::new(0.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(10.0, 6.0), DVec2::new(0.0, 6.0)]);
    s.add_polygon(&[DVec2::new(4.5, 2.5), DVec2::new(5.5, 2.5), DVec2::new(5.5, 3.5), DVec2::new(4.5, 3.5)]);
    let room = wavey_geom::build_room(&s, 3.0, &BTreeMap::from([(1, DVec2::new(1.0, 1.0))])).unwrap();
    let mut inp = input(room, 1.0, omni_at(DVec3::new(2.0, 3.2, 1.5)), &[DVec3::new(2.0, 2.8, 1.5)], 1, (1000.0, 1000.0));
    let pillar_west = inp.room.surfaces.iter().position(|s| s.normal.x < -0.99 && s.d.abs() < 4.6 && s.d.abs() > 4.4).unwrap();
    inp.surface_materials[pillar_west] = material(0.0);
    let res = run(&inp);
    let p = res.mic_pressure(0, &unit_drives(&res))[0];
    // Direct path 0.4 m, reflection via x = 4.5: unfolded √(5² + 0.4²).
    let m = Medium::default();
    let k = 2.0 * std::f64::consts::PI * res.freqs[0] / m.speed_of_sound();
    let path = |r: f64| P90 / r * m.air_amplitude(res.freqs[0], r) * Complex64::from_polar(1.0, -k * r);
    let want = path(0.4) + path((25.0f64 + 0.16).sqrt());
    assert!((p - want).norm() / want.norm() < 1e-5, "{p} vs {want}");
}

#[test]
fn gain_and_polarity_remix() {
    let inp = input(shoebox(5.0, 4.0, 3.0), 0.3, omni_at(DVec3::new(1.0, 1.0, 1.0)), &[DVec3::new(3.0, 2.0, 1.5)], 2, (200.0, 400.0));
    let res = run(&inp);
    let unit = res.mic_pressure(0, &unit_drives(&res));
    let mut s = inp.sources[0].instance.clone();
    s.gain_db = 6.0;
    s.polarity_inverted = true;
    let drive: Vec<Complex64> = res.freqs.iter().map(|f| user_drive(&s, *f)).collect();
    let mixed = res.mic_pressure(0, &[drive]);
    for (a, b) in unit.iter().zip(&mixed) {
        assert!((b + a * 10f64.powf(6.0 / 20.0)).norm() < 1e-6 * a.norm());
    }
}

#[test]
fn cut_plane_and_storage_round_trip() {
    let mut inp = input(shoebox(5.0, 4.0, 3.0), 0.3, omni_at(DVec3::new(1.0, 1.0, 1.0)), &[DVec3::new(3.0, 2.0, 1.5)], 2, (200.0, 400.0));
    inp.planes.push(CutPlane { id: 9, name: "ear".into(), axis: PlaneAxis::Horizontal, offset: 1.2, spacing: 0.25, drape: None });
    let res = run(&inp);
    let map = res.plane_spl(0, &unit_drives(&res), PlaneMode::Band { f: 315.0, fraction: 3.0 });
    assert_eq!(map.len(), 20 * 16);
    assert!(map.iter().all(|v| v.is_finite() && *v > 40.0 && *v < 120.0));
    let dir = std::env::temp_dir().join(format!("wavey-res-{}", std::process::id()));
    res.save(&dir).unwrap();
    let back = SimulationResult::load(&dir).unwrap();
    assert_eq!(back.planes[0].transfer, res.planes[0].transfer);
    assert_eq!(back.mics[0].transfer, res.mics[0].transfer);
    std::fs::remove_dir_all(dir).unwrap();
}
