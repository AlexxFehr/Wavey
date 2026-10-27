#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use glam::{DVec2, DVec3};
use wavey_geom::{Sketch, build_room};
use wavey_materials::{Material, Medium, Spectrum};
use wavey_solver::*;
use wavey_speaker::{SpeakerInstance, samples};

pub fn material(alpha: f64) -> Material {
    Material {
        id: format!("a{alpha}"),
        name: "test".into(),
        absorption: Spectrum::Scalar(alpha),
        scattering: Spectrum::Scalar(0.0),
        impedance: None,
        angle_dependent: false,
        color: "#888888".into(),
        builtin: false,
        source: None,
    }
}

pub fn shoebox(lx: f64, ly: f64, lz: f64) -> wavey_geom::Room {
    let mut s = Sketch::new();
    s.add_polygon(&[DVec2::new(0.0, 0.0), DVec2::new(lx, 0.0), DVec2::new(lx, ly), DVec2::new(0.0, ly)]);
    build_room(&s, lz, &BTreeMap::from([(1, DVec2::new(lx / 2.0, ly / 2.0))])).unwrap()
}

pub fn omni_at(p: DVec3) -> SourceInput {
    SourceInput {
        instance: SpeakerInstance {
            id: 1,
            name: "src".into(),
            model_id: "sample-omni".into(),
            position: p,
            yaw_deg: 0.0,
            pitch_deg: 0.0,
            roll_deg: 0.0,
            gain_db: 0.0,
            delay_ms: 0.0,
            polarity_inverted: false,
            muted: false,
            on_floor: false,
            bottom_height_m: 0.0,
            filters: vec![],
            fir: None,
        },
        model: Arc::new(samples::omni()),
    }
}

pub fn input(room: wavey_geom::Room, alpha: f64, source: SourceInput, mics: &[DVec3], order: u32, freqs: (f64, f64)) -> SimulationInput {
    let n = room.surfaces.len();
    SimulationInput {
        room,
        surface_materials: vec![material(alpha); n],
        sources: vec![source],
        medium: Medium::default(),
        microphones: mics
            .iter()
            .enumerate()
            .map(|(i, p)| Microphone { id: i as u64 + 1, name: format!("m{i}"), position: *p })
            .collect(),
        planes: vec![],
        settings: SimSettings {
            f_min: freqs.0,
            f_max: freqs.1,
            resolution: Resolution::FractionalOctave { n: 3 },
            max_order: order,
            backend: BackendChoice::Cpu,
            late_reverb: false,
            rays_per_source: 0,
            fdtd: None,
            diffraction: false,
            impedance_reflection: false,
            mode: SimMode::Room,
        },
        ground: None,
        ground_patches: vec![],
    }
}

pub fn run(input: &SimulationInput) -> SimulationResult {
    simulate(input, &|_, _| {}, &AtomicBool::new(false)).unwrap()
}

/// Unit drive for every speaker.
pub fn unit_drives(r: &SimulationResult) -> Vec<Vec<num_complex::Complex64>> {
    vec![vec![num_complex::Complex64::new(1.0, 0.0); r.freqs.len()]; r.speaker_ids.len()]
}
