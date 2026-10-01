//! GPU accumulation must match the CPU reference.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::*;
use glam::{DVec2, DVec3};
use wavey_solver::*;

#[test]
fn gpu_matches_cpu() {
    let Some(gpu) = wavey_solver::gpu::gpu() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    eprintln!("GPU: {}", gpu.name);

    let mut s = wavey_geom::Sketch::new();
    s.add_polygon(&[DVec2::new(0.0, 0.0), DVec2::new(30.0, 0.0), DVec2::new(30.0, 20.0), DVec2::new(0.0, 20.0)]);
    s.add_polygon(&[DVec2::new(14.0, 9.0), DVec2::new(15.0, 9.0), DVec2::new(15.0, 10.0), DVec2::new(14.0, 10.0)]);
    let room = wavey_geom::build_room(&s, 8.0, &BTreeMap::from([(1, DVec2::new(1.0, 1.0))])).unwrap();

    let mut src = omni_at(DVec3::new(2.0, 10.0, 4.0));
    src.model = Arc::new(wavey_speaker::samples::two_way());
    src.instance.yaw_deg = 10.0;
    src.instance.pitch_deg = -8.0;
    let mics: Vec<DVec3> = (0..40)
        .map(|i| DVec3::new(4.0 + (i % 8) as f64 * 3.2, 2.0 + (i / 8) as f64 * 3.7, 1.2 + (i % 3) as f64))
        .collect();
    let mut inp = input(room, 0.15, src, &mics, 3, (40.0, 20000.0));
    inp.settings.resolution = Resolution::FractionalOctave { n: 6 };
    // Angle-dependent reflection on half the surfaces.
    for (i, m) in inp.surface_materials.iter_mut().enumerate() {
        if i % 2 == 0 {
            m.angle_dependent = true;
            m.absorption = wavey_materials::Spectrum::bands(&[125.0, 4000.0], &[0.05, 0.6]);
            m.scattering = wavey_materials::Spectrum::Scalar(0.2);
        }
    }

    inp.settings.backend = BackendChoice::Cpu;
    let cpu = run(&inp);
    inp.settings.backend = BackendChoice::Gpu;
    let gpu_res = run(&inp);
    assert!(gpu_res.stats.backend.starts_with("gpu"), "{:?}", gpu_res.stats);
    eprintln!("paths: {}, cpu {:.3}s gpu {:.3}s", cpu.stats.paths, cpu.stats.seconds, gpu_res.stats.seconds);

    let d = unit_drives(&cpu);
    let mut worst = 0.0f64;
    let mut worst_phase = 0.0f64;
    for m in 0..mics.len() {
        let a = cpu.mic_pressure(m, &d);
        let b = gpu_res.mic_pressure(m, &d);
        let scale = a.iter().map(|p| p.norm()).fold(0.0, f64::max);
        for (x, y) in a.iter().zip(&b) {
            worst = worst.max((x - y).norm() / scale);
            if x.norm() > 0.1 * scale {
                worst_phase = worst_phase.max((x / y).arg().abs());
            }
        }
    }
    eprintln!("worst relative error {worst:.2e}, worst phase error {worst_phase:.2e} rad");
    assert!(worst < 2e-4);
    assert!(worst_phase < 1e-3);
}
