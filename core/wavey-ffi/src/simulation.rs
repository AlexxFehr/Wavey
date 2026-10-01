//! Microphones, cut planes, simulation runs and results.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use wavey_solver::{CutPlane, Microphone, PlaneAxis, PlaneMode, Resolution, SimSettings, result::spl};

use crate::{Point3, WaveyDocument, WaveyError};

#[derive(Debug, Clone, uniffi::Record)]
pub struct MicrophoneRec {
    pub id: u64,
    pub name: String,
    pub position: Point3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PlaneAxisRec {
    Horizontal,
    ConstX,
    ConstY,
}

impl From<PlaneAxis> for PlaneAxisRec {
    fn from(a: PlaneAxis) -> Self {
        match a {
            PlaneAxis::Horizontal => Self::Horizontal,
            PlaneAxis::ConstX => Self::ConstX,
            PlaneAxis::ConstY => Self::ConstY,
        }
    }
}

impl From<PlaneAxisRec> for PlaneAxis {
    fn from(a: PlaneAxisRec) -> Self {
        match a {
            PlaneAxisRec::Horizontal => Self::Horizontal,
            PlaneAxisRec::ConstX => Self::ConstX,
            PlaneAxisRec::ConstY => Self::ConstY,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CutPlaneRec {
    pub id: u64,
    pub name: String,
    pub axis: PlaneAxisRec,
    pub offset: f64,
    pub spacing: f64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SimSettingsRec {
    pub f_min: f64,
    pub f_max: f64,
    /// 1/N octave if > 0, otherwise linear with `linear_step_hz`.
    pub octave_fraction: u32,
    pub linear_step_hz: f64,
    pub max_order: u32,
    pub use_gpu: bool,
    pub late_reverb: bool,
    pub rays_per_source: u32,
    pub fdtd_enabled: bool,
    pub fdtd_crossover_hz: f64,
    pub fdtd_duration_s: f64,
}

impl From<&SimSettings> for SimSettingsRec {
    fn from(s: &SimSettings) -> Self {
        let (octave_fraction, linear_step_hz) = match s.resolution {
            Resolution::FractionalOctave { n } => (n, 1.0),
            Resolution::Linear { step_hz } => (0, step_hz),
        };
        let fd = s.fdtd.clone().unwrap_or_default();
        SimSettingsRec {
            f_min: s.f_min,
            f_max: s.f_max,
            octave_fraction,
            linear_step_hz,
            max_order: s.max_order,
            use_gpu: s.backend != wavey_solver::BackendChoice::Cpu,
            late_reverb: s.late_reverb,
            rays_per_source: s.rays_per_source,
            fdtd_enabled: s.fdtd.is_some(),
            fdtd_crossover_hz: fd.crossover_hz,
            fdtd_duration_s: fd.duration_s,
        }
    }
}

impl From<SimSettingsRec> for SimSettings {
    fn from(r: SimSettingsRec) -> Self {
        SimSettings {
            f_min: r.f_min,
            f_max: r.f_max,
            resolution: if r.octave_fraction > 0 {
                Resolution::FractionalOctave { n: r.octave_fraction }
            } else {
                Resolution::Linear { step_hz: r.linear_step_hz }
            },
            max_order: r.max_order,
            backend: if r.use_gpu { wavey_solver::BackendChoice::Auto } else { wavey_solver::BackendChoice::Cpu },
            late_reverb: r.late_reverb,
            rays_per_source: r.rays_per_source,
            fdtd: r.fdtd_enabled.then_some(wavey_solver::fdtd::FdtdSettings {
                crossover_hz: r.fdtd_crossover_hz,
                duration_s: r.fdtd_duration_s,
            }),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct EstimateRec {
    pub frequencies: u64,
    pub receivers: u64,
    pub images_per_source: u64,
    pub sources: u64,
    pub result_megabytes: f64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ResultInfo {
    pub freqs_hz: Vec<f64>,
    pub stale: bool,
    pub backend: String,
    pub seconds: f64,
    pub paths: u64,
    pub receivers: u64,
    pub notes: Vec<String>,
    pub has_late: bool,
    pub mic_ids: Vec<u64>,
    pub plane_ids: Vec<u64>,
    pub speaker_ids: Vec<u64>,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum PlaneModeRec {
    Single { freq_hz: f64 },
    Band { freq_hz: f64, octave_fraction: f64 },
    Broadband,
}

impl From<PlaneModeRec> for PlaneMode {
    fn from(m: PlaneModeRec) -> Self {
        match m {
            PlaneModeRec::Single { freq_hz } => PlaneMode::Single { f: freq_hz },
            PlaneModeRec::Band { freq_hz, octave_fraction } => PlaneMode::Band { f: freq_hz, fraction: octave_fraction },
            PlaneModeRec::Broadband => PlaneMode::Broadband,
        }
    }
}

/// SPL over a cut plane, row-major (v outer, u inner); NaN outside the room.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PlaneMapRec {
    pub plane_id: u64,
    pub axis: PlaneAxisRec,
    pub offset: f64,
    pub nu: u32,
    pub nv: u32,
    pub u0: f64,
    pub v0: f64,
    pub du: f64,
    pub dv: f64,
    pub values_db: Vec<f32>,
    pub min_db: f32,
    pub max_db: f32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MicResponseRec {
    pub mic_id: u64,
    pub freqs_hz: Vec<f64>,
    pub spl_db: Vec<f64>,
    pub phase_deg: Vec<f64>,
}

/// Room-acoustic metrics at a mic per octave band (NaN where undefined).
#[derive(Debug, Clone, uniffi::Record)]
pub struct MetricsRec {
    pub bands_hz: Vec<f64>,
    pub edt_s: Vec<f64>,
    pub t20_s: Vec<f64>,
    pub t30_s: Vec<f64>,
    pub c80_db: Vec<f64>,
    pub d50: Vec<f64>,
    pub sabine_s: Vec<f64>,
    pub eyring_s: Vec<f64>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SimProgress {
    pub fraction: f64,
    pub stage: String,
    pub finished: bool,
    pub error: Option<String>,
}

/// A running (or finished) simulation. Results are stored in the document
/// when it completes.
#[derive(uniffi::Object)]
pub struct SimulationJob {
    state: Arc<Mutex<SimProgress>>,
    cancel: Arc<AtomicBool>,
}

#[uniffi::export]
impl SimulationJob {
    pub fn progress(&self) -> SimProgress {
        self.state.lock().unwrap().clone()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[uniffi::export]
impl WaveyDocument {
    // ---- microphones ----

    pub fn microphones(&self) -> Vec<MicrophoneRec> {
        self.lock()
            .project()
            .microphones
            .values()
            .map(|m| MicrophoneRec { id: m.id, name: m.name.clone(), position: m.position.into() })
            .collect()
    }

    pub fn add_microphone(&self, position: Point3) -> u64 {
        self.lock().edit(|p| {
            let id = p.alloc_id();
            let name = format!("Mic {}", p.microphones.len() + 1);
            p.microphones.insert(id, Microphone { id, name, position: position.into() });
            id
        })
    }

    pub fn update_microphone(&self, mic: MicrophoneRec) -> Result<(), WaveyError> {
        self.lock()
            .try_edit(|p| match p.microphones.get_mut(&mic.id) {
                Some(m) => {
                    m.name = mic.name;
                    m.position = mic.position.into();
                    Ok(())
                }
                None => Err("no such microphone"),
            })
            .map_err(WaveyError::new)
    }

    pub fn remove_microphones(&self, ids: Vec<u64>) {
        self.lock().edit(|p| ids.iter().for_each(|id| {
            p.microphones.remove(id);
        }));
    }

    // ---- cut planes ----

    pub fn cut_planes(&self) -> Vec<CutPlaneRec> {
        self.lock()
            .project()
            .cut_planes
            .values()
            .map(|c| CutPlaneRec { id: c.id, name: c.name.clone(), axis: c.axis.into(), offset: c.offset, spacing: c.spacing })
            .collect()
    }

    pub fn add_cut_plane(&self, axis: PlaneAxisRec, offset: f64, spacing: f64) -> u64 {
        self.lock().edit(|p| {
            let id = p.alloc_id();
            let name = format!("Plane {}", p.cut_planes.len() + 1);
            p.cut_planes.insert(id, CutPlane { id, name, axis: axis.into(), offset, spacing });
            id
        })
    }

    pub fn update_cut_plane(&self, plane: CutPlaneRec) -> Result<(), WaveyError> {
        if !(plane.spacing > 0.0) {
            return Err(WaveyError::new("spacing must be positive"));
        }
        self.lock()
            .try_edit(|p| match p.cut_planes.get_mut(&plane.id) {
                Some(c) => {
                    *c = CutPlane { id: plane.id, name: plane.name, axis: plane.axis.into(), offset: plane.offset, spacing: plane.spacing };
                    Ok(())
                }
                None => Err("no such cut plane"),
            })
            .map_err(WaveyError::new)
    }

    pub fn remove_cut_planes(&self, ids: Vec<u64>) {
        self.lock().edit(|p| ids.iter().for_each(|id| {
            p.cut_planes.remove(id);
        }));
    }

    // ---- settings ----

    pub fn sim_settings(&self) -> SimSettingsRec {
        (&self.lock().project().sim_settings).into()
    }

    pub fn set_sim_settings(&self, settings: SimSettingsRec) -> Result<(), WaveyError> {
        let s: SimSettings = settings.into();
        s.validate().map_err(WaveyError::new)?;
        self.lock().edit(|p| p.sim_settings = s);
        Ok(())
    }

    pub fn estimate(&self) -> Result<EstimateRec, WaveyError> {
        let mut doc = self.lock();
        let room = doc.room().map_err(WaveyError::new)?.clone();
        let input = doc.project().simulation_input(&room);
        let e = wavey_solver::estimate(&input).map_err(WaveyError::new)?;
        Ok(EstimateRec {
            frequencies: e.frequencies as u64,
            receivers: e.receivers as u64,
            images_per_source: e.images_per_source as u64,
            sources: input.sources.len() as u64,
            result_megabytes: e.result_bytes as f64 / 1e6,
        })
    }

    // ---- running ----

    /// Starts a simulation on a background thread.
    pub fn start_simulation(&self) -> Result<Arc<SimulationJob>, WaveyError> {
        let (input, fingerprint) = {
            let mut doc = self.lock();
            let room = doc.room().map_err(WaveyError::new)?.clone();
            let p = doc.project();
            (p.simulation_input(&room), p.simulation_fingerprint())
        };
        let state = Arc::new(Mutex::new(SimProgress { fraction: 0.0, stage: "starting".into(), finished: false, error: None }));
        let cancel = Arc::new(AtomicBool::new(false));
        let job = Arc::new(SimulationJob { state: state.clone(), cancel: cancel.clone() });
        let doc = self.doc.clone();
        std::thread::Builder::new()
            .name("wavey-simulation".into())
            .spawn(move || {
                let report = |f: f64, stage: &str| {
                    let mut s = state.lock().unwrap();
                    s.fraction = f;
                    s.stage = stage.to_string();
                };
                let outcome = wavey_solver::simulate(&input, &report, &cancel);
                let mut s = state.lock().unwrap();
                match outcome {
                    Ok(mut r) => {
                        r.fingerprint = fingerprint;
                        doc.lock().unwrap_or_else(|e| e.into_inner()).set_results(r);
                        s.fraction = 1.0;
                        s.stage = "done".into();
                    }
                    Err(e) => s.error = Some(e.to_string()),
                }
                s.finished = true;
            })
            .map_err(WaveyError::new)?;
        Ok(job)
    }

    // ---- results ----

    pub fn result_info(&self) -> Option<ResultInfo> {
        let doc = self.lock();
        let r = doc.results()?;
        Some(ResultInfo {
            freqs_hz: r.freqs.clone(),
            stale: doc.results_stale(),
            backend: r.stats.backend.clone(),
            seconds: r.stats.seconds,
            paths: r.stats.paths as u64,
            receivers: r.stats.receivers as u64,
            notes: r.stats.notes.clone(),
            has_late: r.has_late,
            mic_ids: r.mics.iter().map(|m| m.mic.id).collect(),
            plane_ids: r.planes.iter().map(|p| p.plane.id).collect(),
            speaker_ids: r.speaker_ids.clone(),
        })
    }

    /// Microphone response mixed with the current speaker settings. With
    /// `solo_speaker`, only that speaker contributes.
    pub fn mic_response(&self, mic_id: u64, solo_speaker: Option<u64>) -> Result<MicResponseRec, WaveyError> {
        let doc = self.lock();
        let r = doc.results().ok_or_else(|| WaveyError::new("no results"))?;
        let i = r.mics.iter().position(|m| m.mic.id == mic_id).ok_or_else(|| WaveyError::new("microphone not in results"))?;
        let mut drives = doc.project().drives(&r.speaker_ids, &r.freqs);
        if let Some(solo) = solo_speaker {
            for (d, id) in drives.iter_mut().zip(&r.speaker_ids) {
                if *id != solo {
                    d.iter_mut().for_each(|v| *v = num_complex::Complex64::new(0.0, 0.0));
                }
            }
        }
        let p = r.mic_pressure(i, &drives);
        Ok(MicResponseRec {
            mic_id,
            freqs_hz: r.freqs.clone(),
            spl_db: p.iter().map(|v| spl(v.norm_sqr())).collect(),
            phase_deg: p.iter().map(|v| v.arg().to_degrees()).collect(),
        })
    }

    pub fn mic_metrics(&self, mic_id: u64) -> Result<Option<MetricsRec>, WaveyError> {
        let doc = self.lock();
        let r = doc.results().ok_or_else(|| WaveyError::new("no results"))?;
        let i = r.mics.iter().position(|m| m.mic.id == mic_id).ok_or_else(|| WaveyError::new("microphone not in results"))?;
        let drives = doc.project().drives(&r.speaker_ids, &r.freqs);
        Ok(r.mic_metrics(i, &drives).map(|m| MetricsRec {
            bands_hz: wavey_solver::raytrace::BANDS.to_vec(),
            edt_s: m.edt_s.to_vec(),
            t20_s: m.t20_s.to_vec(),
            t30_s: m.t30_s.to_vec(),
            c80_db: m.c80_db.to_vec(),
            d50: m.d50.to_vec(),
            sabine_s: r.rt_sabine.to_vec(),
            eyring_s: r.rt_eyring.to_vec(),
        }))
    }

    pub fn plane_map(&self, plane_id: u64, mode: PlaneModeRec) -> Result<PlaneMapRec, WaveyError> {
        let doc = self.lock();
        let r = doc.results().ok_or_else(|| WaveyError::new("no results"))?;
        let i = r.planes.iter().position(|p| p.plane.id == plane_id).ok_or_else(|| WaveyError::new("plane not in results"))?;
        let drives = doc.project().drives(&r.speaker_ids, &r.freqs);
        let values = r.plane_spl(i, &drives, mode.into());
        let finite = values.iter().copied().filter(|v| v.is_finite());
        let (min_db, max_db) = finite.fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        let g = &r.planes[i].grid;
        Ok(PlaneMapRec {
            plane_id,
            axis: g.axis.into(),
            offset: g.offset,
            nu: g.nu as u32,
            nv: g.nv as u32,
            u0: g.u0,
            v0: g.v0,
            du: g.du,
            dv: g.dv,
            values_db: values,
            min_db,
            max_db,
        })
    }

    pub fn clear_results(&self) {
        let mut doc = self.lock();
        if doc.results().is_some() {
            doc.clear_results();
        }
    }
}
