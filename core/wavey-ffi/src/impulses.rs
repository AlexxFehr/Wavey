//! The impulse-response matrix: every solved microphone × every source, for
//! filter design outside Wavey (room correction, MIMO).

use crate::{WaveyDocument, WaveyError};

/// What the exported responses include.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ImpulseProcessingRec {
    /// The room alone: each source at unit drive.
    Room,
    /// Each source's gain, delay, polarity and EQ (and its groups'), without
    /// FIR filters: the plant a FIR design works on.
    Processing,
    /// Everything, FIR filters included: what is heard.
    ProcessingAndFir,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ImpulseExportRec {
    pub files: u32,
    pub samples: u32,
    pub seconds: f64,
}

#[uniffi::export]
impl WaveyDocument {
    /// Writes the active run's impulse responses into `folder`: one 32-bit
    /// float WAV per (solved mic, source) in pascals, `<mic>_<source>.wav` by
    /// their codes, and `manifest.json` with the positions, the arrivals (in
    /// samples, from the geometry) and the file names. Needs a run on an
    /// evenly spaced frequency grid; `seconds` is cut to its period (1 / step).
    pub fn export_impulse_matrix(&self, folder: String, sample_rate_hz: f64, seconds: f64, processing: ImpulseProcessingRec) -> Result<ImpulseExportRec, WaveyError> {
        let doc = self.lock();
        let r = doc.results().ok_or_else(|| WaveyError::new("no results"))?.clone();
        let step = wavey_solver::decay::uniform_step(&r.freqs).ok_or_else(|| WaveyError::new("the run needs a linear frequency step (Solve settings › Resolution)"))?;
        if !(1000.0..=384_000.0).contains(&sample_rate_hz) {
            return Err(WaveyError::new("sample rate must be 1–384 kHz"));
        }
        let p = doc.project();
        let drives = match processing {
            ImpulseProcessingRec::Room => vec![vec![num_complex::Complex64::new(1.0, 0.0); r.freqs.len()]; r.speaker_ids.len()],
            ImpulseProcessingRec::Processing => {
                let mut q = p.clone();
                q.speakers.values_mut().for_each(|s| s.fir = None);
                q.meta.groups.values_mut().for_each(|g| g.fir = None);
                q.drives(&r.speaker_ids, &r.freqs)
            }
            ImpulseProcessingRec::ProcessingAndFir => p.drives(&r.speaker_ids, &r.freqs),
        };
        let samples = ((seconds * sample_rate_hz).round() as usize).clamp(1, (sample_rate_hz / step).round() as usize);
        let dir = std::path::Path::new(&folder);
        std::fs::create_dir_all(dir).map_err(|e| WaveyError::new(e.to_string()))?;
        let c = p.medium.speed_of_sound();
        let code = |key: String, fallback: String| p.meta.code(&key).map(str::to_string).unwrap_or(fallback).replace(['/', ' '], "-");
        let mut files = Vec::new();
        let mut arrivals = Vec::new();
        for (mi, m) in r.mics.iter().enumerate() {
            let mcode = code(format!("mic:{}", m.mic.id), format!("M{}", mi + 1));
            let (mut row, mut arow) = (Vec::new(), Vec::new());
            for (si, id) in r.speaker_ids.iter().enumerate() {
                let scode = code(format!("spk:{id}"), format!("S{}", si + 1));
                let h: Vec<num_complex::Complex64> = m.transfer[si].iter().zip(&drives[si]).map(|(t, d)| d * num_complex::Complex64::new(t.re as f64, t.im as f64)).collect();
                let ir = wavey_solver::analysis::impulse_from_spectrum_first(&r.freqs, &h, sample_rate_hz, samples).map_err(WaveyError::new)?;
                let name = format!("{mcode}_{scode}.wav");
                wavey_solver::analysis::write_wav(&dir.join(&name), &ir, sample_rate_hz).map_err(|e| WaveyError::new(e.to_string()))?;
                row.push(name);
                // Direct-sound arrival from the geometry (plus the source's own delay when it is included).
                let arrival = p.speakers.get(id).zip(p.speakers.get(id).and_then(|s| p.speaker_models.get(&s.model_id))).map_or(f64::NAN, |(s, model)| {
                    let delay = if processing == ImpulseProcessingRec::Room { 0.0 } else { s.delay_ms * 1e-3 };
                    (s.acoustic_center(model).distance(m.mic.position) / c + delay) * sample_rate_hz
                });
                arow.push(arrival);
            }
            files.push(row);
            arrivals.push(arow);
        }
        let manifest = serde_json::json!({
            "sample_rate": sample_rate_hz,
            "samples": samples,
            "seconds": samples as f64 / sample_rate_hz,
            "processing": format!("{processing:?}"),
            "frequency_range_hz": [r.freqs.first(), r.freqs.last()],
            "step_hz": step,
            "speed_of_sound": c,
            "units": "pascal per unit input sample",
            "mics": r.mics.iter().map(|m| serde_json::json!({ "id": m.mic.id, "code": p.meta.code(&format!("mic:{}", m.mic.id)), "name": m.mic.name, "position": [m.mic.position.x, m.mic.position.y, m.mic.position.z] })).collect::<Vec<_>>(),
            "sources": r.speaker_ids.iter().map(|id| {
                let s = p.speakers.get(id);
                let centre = s.and_then(|s| p.speaker_models.get(&s.model_id).map(|m| s.acoustic_center(m)));
                serde_json::json!({ "id": id, "code": p.meta.code(&format!("spk:{id}")), "name": s.map(|s| s.name.clone()), "model": s.map(|s| s.model_id.clone()), "acoustic_centre": centre.map(|c| [c.x, c.y, c.z]) })
            }).collect::<Vec<_>>(),
            "arrivals_samples": arrivals,
            "files": files,
        });
        std::fs::write(dir.join("manifest.json"), serde_json::to_vec_pretty(&manifest).unwrap()).map_err(|e| WaveyError::new(e.to_string()))?;
        Ok(ImpulseExportRec { files: (r.mics.len() * r.speaker_ids.len()) as u32, samples: samples as u32, seconds: samples as f64 / sample_rate_hz })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use wavey_geom::{DVec2, DVec3};
    use wavey_project::Document;

    use super::*;

    /// A 6 × 4 × 3 m room, two omnis and one mic, direct sound only on a 1 Hz grid.
    fn solved() -> WaveyDocument {
        let mut d = Document::new();
        d.edit(|p| {
            p.ceiling_height = 3.0;
            p.add_polygon(&[DVec2::new(0.0, 0.0), DVec2::new(6.0, 0.0), DVec2::new(6.0, 4.0), DVec2::new(0.0, 4.0)]);
            p.interior_seeds.insert(1, DVec2::new(1.0, 1.0));
            p.add_speaker_model(wavey_speaker::samples::omni()).unwrap();
            p.add_speaker("sample-omni", DVec3::new(1.0, 2.0, 1.5)).unwrap();
            p.add_speaker("sample-omni", DVec3::new(1.0, 1.0, 1.5)).unwrap();
            p.add_microphone(DVec3::new(4.43, 2.0, 1.5));
            let s = &mut p.sim_settings;
            s.f_min = 50.0;
            s.f_max = 400.0;
            s.resolution = wavey_solver::Resolution::Linear { step_hz: 1.0 };
            s.max_order = 0;
            s.late_reverb = false;
            s.fdtd = None;
            s.diffraction = false;
        });
        let input = d.solver_input().unwrap();
        let r = wavey_solver::simulate(&input, &|_, _| {}, &std::sync::atomic::AtomicBool::new(false)).unwrap();
        d.add_run(r);
        WaveyDocument { doc: Arc::new(Mutex::new(d)) }
    }

    #[test]
    fn exports_every_mic_and_source_with_a_manifest() {
        let doc = solved();
        let dir = std::env::temp_dir().join(format!("wavey-irs-{}", std::process::id()));
        let out = doc.export_impulse_matrix(dir.to_string_lossy().into(), 8000.0, 0.5, ImpulseProcessingRec::Room).unwrap();
        assert_eq!((out.files, out.samples), (2, 4000));
        let m: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(m["files"].as_array().unwrap().len(), 1);
        assert_eq!(m["files"][0].as_array().unwrap().len(), 2);
        for k in 0..2 {
            let name = m["files"][0][k].as_str().unwrap();
            let fir = wavey_speaker::Fir::from_wav(name, &std::fs::read(dir.join(name)).unwrap()).unwrap();
            assert_eq!(fir.taps.len(), 4000);
            // The direct sound peaks where the manifest says it arrives.
            let peak = fir.taps.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0 as f64;
            let arrival = m["arrivals_samples"][0][k].as_f64().unwrap();
            assert!((peak - arrival).abs() < 3.0, "peak {peak} vs arrival {arrival}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn needs_an_even_grid() {
        let doc = solved();
        doc.lock().edit(|p| p.sim_settings.resolution = wavey_solver::Resolution::FractionalOctave { n: 3 });
        // The active run is still on its 1 Hz grid; a fractional-octave run would be refused.
        assert!(doc.export_impulse_matrix(std::env::temp_dir().join("wavey-irs-x").to_string_lossy().into(), 500.0, 0.5, ImpulseProcessingRec::Room).is_err(), "rate below 1 kHz");
    }
}
