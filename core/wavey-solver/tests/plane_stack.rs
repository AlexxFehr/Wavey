//! The all-frequency plane stack and the plane probe against the existing
//! per-frequency plane map and mic response.

mod common;

use std::sync::atomic::AtomicBool;

use common::*;
use glam::DVec3;
use num_complex::Complex64;
use wavey_solver::*;

#[test]
fn stack_matches_single_maps_and_probe_matches_grid_point() {
    let room = shoebox(5.0, 4.0, 3.0);
    let mut inp = input(room, 0.2, omni_at(DVec3::new(1.2, 1.1, 1.4)), &[], 3, (100.0, 400.0));
    inp.settings.late_reverb = true;
    inp.settings.rays_per_source = 4000;
    inp.planes = vec![CutPlane { id: 1, name: "ear".into(), axis: PlaneAxis::Horizontal, offset: 1.2, spacing: 0.5 }];
    let r = simulate(&inp, &|_, _| {}, &AtomicBool::new(false)).unwrap();
    let drives = vec![vec![Complex64::new(1.0, 0.0); r.freqs.len()]];
    let stack = r.plane_spl_stack(0, &drives);
    let g = &r.planes[0].grid;
    let cells = g.nu * g.nv;
    assert_eq!(stack.len(), cells * r.freqs.len());
    for (fi, f) in r.freqs.iter().enumerate() {
        let map = r.plane_spl(0, &drives, PlaneMode::Single { f: *f });
        for c in 0..cells {
            let (a, b) = (stack[fi * cells + c], map[c]);
            assert!((a.is_nan() && b.is_nan()) || (a - b).abs() < 1e-3, "f {f} cell {c}: {a} vs {b}");
        }
    }
    // A probe exactly on an inside grid point equals that point.
    let c = g.inside.iter().position(|b| *b).unwrap();
    let (iu, iv) = (c % g.nu, c / g.nu);
    let p = r.plane_probe(0, g.u0 + iu as f64 * g.du, g.v0 + iv as f64 * g.dv, &drives).unwrap();
    for fi in 0..r.freqs.len() {
        let db = result::spl(p[fi].norm_sqr()) as f32;
        assert!((db - stack[fi * cells + c]).abs() < 1e-3);
    }
}
