//! Microphones, cut planes, simulation runs and results.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use wavey_solver::{CutPlane, PlaneAxis, PlaneMode, Resolution, SimSettings, result::spl};

use wavey_project::meta::{MicMeta, PlaneStyle, RangeMode};

use crate::{Point3, WaveyDocument, WaveyError};
use wavey_geom::SurfaceKey;

#[derive(Debug, Clone, uniffi::Record)]
pub struct MicrophoneRec {
    pub id: u64,
    pub name: String,
    pub position: Point3,
    /// Display code ("M1"); ignored on input.
    pub code: String,
    /// Plot colour (hex); empty = palette default.
    pub color: String,
    pub visible: bool,
    pub include_in_solve: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RangeModeRec {
    Auto,
    Fixed,
    Symmetric,
}

/// How a cut plane is drawn (display only; not part of results).
#[derive(Debug, Clone, uniffi::Record)]
pub struct PlaneStyleRec {
    pub color_map: String,
    pub range: RangeModeRec,
    pub range_min: f64,
    pub range_max: f64,
    pub auto_span: f64,
    pub contour_step: f64,
    pub show_contours: bool,
    pub opacity: f64,
    pub visible: bool,
    /// Solved with the rest (planes are most of a solve's memory).
    pub include_in_solve: bool,
}

impl From<&PlaneStyle> for PlaneStyleRec {
    fn from(s: &PlaneStyle) -> Self {
        PlaneStyleRec {
            color_map: s.color_map.clone(),
            range: match s.range {
                RangeMode::Auto => RangeModeRec::Auto,
                RangeMode::Fixed => RangeModeRec::Fixed,
                RangeMode::Symmetric => RangeModeRec::Symmetric,
            },
            range_min: s.range_min,
            range_max: s.range_max,
            auto_span: s.auto_span,
            contour_step: s.contour_step,
            show_contours: s.show_contours,
            opacity: s.opacity,
            visible: s.visible,
            include_in_solve: s.include_in_solve,
        }
    }
}

impl From<PlaneStyleRec> for PlaneStyle {
    fn from(s: PlaneStyleRec) -> Self {
        PlaneStyle {
            color_map: s.color_map,
            range: match s.range {
                RangeModeRec::Auto => RangeMode::Auto,
                RangeModeRec::Fixed => RangeMode::Fixed,
                RangeModeRec::Symmetric => RangeMode::Symmetric,
            },
            range_min: s.range_min,
            range_max: s.range_max,
            auto_span: s.auto_span,
            contour_step: s.contour_step,
            show_contours: s.show_contours,
            opacity: s.opacity.clamp(0.0, 1.0),
            visible: s.visible,
            include_in_solve: s.include_in_solve,
        }
    }
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
    /// Display code ("CP-1"); ignored on input.
    pub code: String,
    pub style: PlaneStyleRec,
    /// Audience area: height above the floor (None: a flat plane at `offset`).
    pub drape_height: Option<f64>,
    /// Floor surfaces it covers (empty: all floors).
    pub drape_surfaces: Vec<String>,
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
    pub fdtd_points_per_wavelength: f64,
    /// Cabinets as rigid obstacles in the wave solver (directivity simulated).
    pub fdtd_cabinets: bool,
    /// Grid headroom of the wave solver above the top frequency it needs.
    pub fdtd_accuracy: WaveAccuracyRec,
    /// Direct sound only, in free field: nothing reflects, blocks or diffracts.
    pub free_field: bool,
    /// Edge diffraction around walls and pillars.
    pub diffraction: bool,
    /// Angle-dependent reflection from fitted impedances at all frequencies.
    pub impedance_reflection: bool,
}

/// How finely the wave solver resolves the top of the band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum WaveAccuracyRec {
    Draft,
    Standard,
    High,
}

impl From<wavey_solver::fdtd::WaveAccuracy> for WaveAccuracyRec {
    fn from(a: wavey_solver::fdtd::WaveAccuracy) -> Self {
        use wavey_solver::fdtd::WaveAccuracy as A;
        match a {
            A::Draft => WaveAccuracyRec::Draft,
            A::Standard => WaveAccuracyRec::Standard,
            A::High => WaveAccuracyRec::High,
        }
    }
}

impl From<WaveAccuracyRec> for wavey_solver::fdtd::WaveAccuracy {
    fn from(a: WaveAccuracyRec) -> Self {
        use wavey_solver::fdtd::WaveAccuracy as A;
        match a {
            WaveAccuracyRec::Draft => A::Draft,
            WaveAccuracyRec::Standard => A::Standard,
            WaveAccuracyRec::High => A::High,
        }
    }
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
            fdtd_points_per_wavelength: fd.points_per_wavelength,
            fdtd_cabinets: fd.cabinets,
            fdtd_accuracy: fd.accuracy.into(),
            free_field: s.mode == wavey_solver::SimMode::FreeField,
            diffraction: s.diffraction,
            impedance_reflection: s.impedance_reflection,
        }
    }
}

impl From<SimSettingsRec> for SimSettings {
    fn from(r: SimSettingsRec) -> Self {
        SimSettings {
            f_min: r.f_min,
            f_max: r.f_max,
            resolution: if r.octave_fraction > 0 {
                Resolution::FractionalOctave {
                    n: r.octave_fraction,
                }
            } else {
                Resolution::Linear {
                    step_hz: r.linear_step_hz,
                }
            },
            max_order: r.max_order,
            backend: if r.use_gpu {
                wavey_solver::BackendChoice::Auto
            } else {
                wavey_solver::BackendChoice::Cpu
            },
            late_reverb: r.late_reverb,
            rays_per_source: r.rays_per_source,
            fdtd: r.fdtd_enabled.then_some(wavey_solver::fdtd::FdtdSettings {
                crossover_hz: r.fdtd_crossover_hz,
                duration_s: r.fdtd_duration_s,
                points_per_wavelength: r.fdtd_points_per_wavelength.clamp(6.0, 40.0),
                source_order: 1,
                cabinets: r.fdtd_cabinets,
                accuracy: r.fdtd_accuracy.into(),
            }),
            diffraction: r.diffraction,
            impedance_reflection: r.impedance_reflection,
            mode: if r.free_field { wavey_solver::SimMode::FreeField } else { wavey_solver::SimMode::Room },
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
    /// Wave-solver crossover, if it ran (results are wave-based below it).
    pub fdtd_crossover_hz: Option<f64>,
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
            PlaneModeRec::Band {
                freq_hz,
                octave_fraction,
            } => PlaneMode::Band {
                f: freq_hz,
                fraction: octave_fraction,
            },
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
    /// Audience areas: height of each point (`[v][u]`, NaN outside); empty
    /// for flat planes.
    pub heights: Vec<f32>,
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

/// Which decay time a map shows.
#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum DecayKindRec {
    Edt,
    T20,
    T30,
}

/// Decay times at a mic per band, from the phase-aware mix of the sources
/// (NaN where the run cannot say).
#[derive(Debug, Clone, uniffi::Record)]
pub struct SpectralDecayRec {
    pub bands_hz: Vec<f64>,
    pub edt_s: Vec<f64>,
    pub t20_s: Vec<f64>,
    pub t30_s: Vec<f64>,
    /// Length of the window the decays were measured in (`1 / frequency step`).
    pub window_s: f64,
}

/// What a decay time in a band needs from the run's frequency step.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DecayNeedRec {
    /// The room's statistical (Eyring) reverberation time near the band (s); NaN if unknown.
    pub expected_rt_s: f64,
    /// Largest step that holds that decay for this quantity (Hz); NaN if unknown.
    pub step_needed_hz: f64,
    /// The run's step (Hz); NaN if its grid is not uniform.
    pub run_step_hz: f64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SimProgress {
    pub fraction: f64,
    pub stage: String,
    /// Estimated seconds left, once the run has gone far enough to tell.
    pub seconds_left: Option<f64>,
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
        let doc = self.lock();
        let p = doc.project();
        p.microphones
            .values()
            .map(|m| {
                let meta = p.meta.mics.get(&m.id).cloned().unwrap_or_default();
                MicrophoneRec {
                    id: m.id,
                    name: m.name.clone(),
                    position: m.position.into(),
                    code: p
                        .meta
                        .code(&format!("mic:{}", m.id))
                        .unwrap_or_default()
                        .to_string(),
                    color: meta.color,
                    visible: meta.visible,
                    include_in_solve: meta.include_in_solve,
                }
            })
            .collect()
    }

    pub fn add_microphone(&self, position: Point3) -> u64 {
        self.lock().edit(|p| p.add_microphone(position.into()))
    }

    pub fn update_microphone(&self, mic: MicrophoneRec) -> Result<(), WaveyError> {
        self.update_microphones(vec![mic])
    }

    /// Updates several microphones as one undo step.
    pub fn update_microphones(&self, mics: Vec<MicrophoneRec>) -> Result<(), WaveyError> {
        self.lock()
            .try_edit(|p| {
                for mic in mics {
                    let m = p.microphones.get_mut(&mic.id).ok_or("no such microphone")?;
                    m.name = mic.name;
                    m.position = mic.position.into();
                    p.meta.mics.insert(
                        mic.id,
                        MicMeta {
                            color: mic.color,
                            visible: mic.visible,
                            include_in_solve: mic.include_in_solve,
                        },
                    );
                }
                Ok::<_, &str>(())
            })
            .map_err(WaveyError::new)
    }

    pub fn remove_microphones(&self, ids: Vec<u64>) {
        self.lock()
            .edit(|p| ids.iter().for_each(|id| p.remove_microphone(*id)));
    }

    // ---- cut planes ----

    pub fn cut_planes(&self) -> Vec<CutPlaneRec> {
        let doc = self.lock();
        let p = doc.project();
        p.cut_planes
            .values()
            .map(|c| CutPlaneRec {
                id: c.id,
                name: c.name.clone(),
                axis: c.axis.into(),
                offset: c.offset,
                spacing: c.spacing,
                code: p
                    .meta
                    .code(&format!("plane:{}", c.id))
                    .unwrap_or_default()
                    .to_string(),
                style: (&p.meta.planes.get(&c.id).cloned().unwrap_or_default()).into(),
                drape_height: c.drape.as_ref().map(|d| d.height),
                drape_surfaces: c.drape.as_ref().map(|d| d.surfaces.iter().map(|k| k.to_string()).collect()).unwrap_or_default(),
            })
            .collect()
    }

    /// The receiver points of an audience area (xyz, inside the room), to
    /// show before solving. Empty for flat planes.
    pub fn audience_area_points(&self, plane_id: u64) -> Vec<f32> {
        let mut doc = self.lock();
        let Some(plane) = doc.project().cut_planes.get(&plane_id).filter(|c| c.drape.is_some()).cloned() else { return vec![] };
        let Ok(room) = doc.room() else { return vec![] };
        plane.grid(room).map(|g| g.inside_points().iter().flat_map(|p| [p.x as f32, p.y as f32, p.z as f32]).collect()).unwrap_or_default()
    }

    /// Adds an audience area `height` above the given floor surfaces (keys;
    /// empty: all floors).
    pub fn add_audience_area(&self, surface_keys: Vec<String>, height: f64, spacing: f64) -> Result<u64, WaveyError> {
        let keys = surface_keys.iter().map(|k| k.parse::<SurfaceKey>()).collect::<Result<Vec<_>, _>>().map_err(WaveyError::new)?;
        Ok(self.lock().edit(|p| p.add_audience_area(keys, height, spacing)))
    }

    pub fn add_cut_plane(&self, axis: PlaneAxisRec, offset: f64, spacing: f64) -> u64 {
        self.lock()
            .edit(|p| p.add_cut_plane(axis.into(), offset, spacing))
    }

    pub fn update_cut_plane(&self, plane: CutPlaneRec) -> Result<(), WaveyError> {
        if !(plane.spacing > 0.0) {
            return Err(WaveyError::new("spacing must be positive"));
        }
        self.lock()
            .try_edit(|p| match p.cut_planes.get_mut(&plane.id) {
                Some(c) => {
                    let drape = match plane.drape_height {
                        Some(height) => Some(wavey_solver::Drape {
                            height,
                            surfaces: plane.drape_surfaces.iter().filter_map(|k| k.parse().ok()).collect(),
                        }),
                        None => None,
                    };
                    *c = CutPlane {
                        id: plane.id,
                        name: plane.name,
                        axis: plane.axis.into(),
                        offset: plane.offset,
                        spacing: plane.spacing,
                        drape,
                    };
                    p.meta.planes.insert(plane.id, plane.style.into());
                    Ok(())
                }
                None => Err("no such cut plane"),
            })
            .map_err(WaveyError::new)
    }

    pub fn remove_cut_planes(&self, ids: Vec<u64>) {
        self.lock()
            .edit(|p| ids.iter().for_each(|id| p.remove_cut_plane(*id)));
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

    /// The project has no walls: a solve is over open air with a ground.
    pub fn is_open_air(&self) -> bool {
        self.lock().project().open_air()
    }

    pub fn estimate(&self) -> Result<EstimateRec, WaveyError> {
        let mut doc = self.lock();
        let input = doc.solver_input().map_err(WaveyError::new)?;
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
        // What the run is of: taken now, not when it ends (the project may be
        // edited while it runs).
        let (input, fingerprint, settings, parts) = {
            let mut doc = self.lock();
            let input = doc.solver_input().map_err(WaveyError::new)?;
            let p = doc.project();
            (input, p.simulation_fingerprint(), p.sim_settings.clone(), p.fingerprint_parts())
        };
        let state = Arc::new(Mutex::new(SimProgress {
            fraction: 0.0,
            stage: "starting".into(),
            seconds_left: None,
            finished: false,
            error: None,
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let job = Arc::new(SimulationJob {
            state: state.clone(),
            cancel: cancel.clone(),
        });
        let doc = self.doc.clone();
        std::thread::Builder::new()
            .name("wavey-simulation".into())
            .spawn(move || {
                let started = std::time::Instant::now();
                let report = |f: f64, stage: &str| {
                    let mut s = state.lock().unwrap();
                    s.fraction = f;
                    s.stage = stage.to_string();
                    s.seconds_left = wavey_solver::progress::seconds_left(started.elapsed().as_secs_f64(), f);
                };
                // A panic in the solver must still finish the job, or the
                // app would wait on it forever.
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| wavey_solver::simulate(&input, &report, &cancel)));
                let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                let outcome = outcome.unwrap_or_else(|p| {
                    let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|m| m.to_string()));
                    Err(wavey_solver::SolverError::Invalid(format!("solver crashed: {}", msg.unwrap_or_default())))
                });
                match outcome {
                    Ok(mut r) => {
                        r.fingerprint = fingerprint;
                        doc.lock().unwrap_or_else(|e| e.into_inner()).add_solved_run(r, settings, parts);
                        s.fraction = 1.0;
                        s.stage = "done".into();
                        s.seconds_left = None;
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
            fdtd_crossover_hz: r.fdtd.as_ref().map(|f| f.crossover_hz),
            mic_ids: r.mics.iter().map(|m| m.mic.id).collect(),
            plane_ids: r.planes.iter().map(|p| p.plane.id).collect(),
            speaker_ids: r.speaker_ids.clone(),
        })
    }

    /// Microphone response mixed with the current speaker settings. With
    /// `solo_speaker`, only that speaker contributes.
    pub fn mic_response(
        &self,
        mic_id: u64,
        solo_speaker: Option<u64>,
    ) -> Result<MicResponseRec, WaveyError> {
        let doc = self.lock();
        let r = doc.results().ok_or_else(|| WaveyError::new("no results"))?;
        let i = r
            .mics
            .iter()
            .position(|m| m.mic.id == mic_id)
            .ok_or_else(|| WaveyError::new("microphone not in results"))?;
        let mut drives = doc.project().drives(&r.speaker_ids, &r.freqs);
        if let Some(solo) = solo_speaker {
            for (d, id) in drives.iter_mut().zip(&r.speaker_ids) {
                if *id != solo {
                    d.iter_mut()
                        .for_each(|v| *v = num_complex::Complex64::new(0.0, 0.0));
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
        let i = r
            .mics
            .iter()
            .position(|m| m.mic.id == mic_id)
            .ok_or_else(|| WaveyError::new("microphone not in results"))?;
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
        let i = r
            .planes
            .iter()
            .position(|p| p.plane.id == plane_id)
            .ok_or_else(|| WaveyError::new("plane not in results"))?;
        let drives = doc.project().drives(&r.speaker_ids, &r.freqs);
        let values = r.plane_spl(i, &drives, mode.into());
        let finite = values.iter().copied().filter(|v| v.is_finite());
        let (min_db, max_db) = finite.fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| {
            (a.min(v), b.max(v))
        });
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
            heights: g.z.iter().map(|z| *z as f32).collect(),
        })
    }

    /// Centres of the `1/octave_fraction`-octave bands the active run can
    /// give decay times for (inside the wave solver's range, on a uniform
    /// frequency grid).
    pub fn decay_bands(&self, octave_fraction: f64) -> Vec<f64> {
        self.lock().results().map(|r| r.decay_bands(octave_fraction)).unwrap_or_default()
    }

    /// Decay times at a mic in each band the run can give.
    pub fn mic_decay(&self, mic_id: u64, octave_fraction: f64) -> Result<SpectralDecayRec, WaveyError> {
        let doc = self.lock();
        let r = doc.results().ok_or_else(|| WaveyError::new("no results"))?;
        let i = r.mics.iter().position(|m| m.mic.id == mic_id).ok_or_else(|| WaveyError::new("microphone not in results"))?;
        let drives = doc.project().drives(&r.speaker_ids, &r.freqs);
        let bands = r.decay_bands(octave_fraction);
        let d: Vec<_> = bands.iter().map(|c| r.mic_decay(i, &drives, *c, octave_fraction)).collect();
        Ok(SpectralDecayRec {
            window_s: d.first().map_or(f64::NAN, |x| x.window_s),
            edt_s: d.iter().map(|x| x.edt).collect(),
            t20_s: d.iter().map(|x| x.t20).collect(),
            t30_s: d.iter().map(|x| x.t30).collect(),
            bands_hz: bands,
        })
    }

    /// What the decay time `kind` in the band at `centre_hz` needs from the step.
    pub fn decay_need(&self, centre_hz: f64, kind: DecayKindRec) -> Option<DecayNeedRec> {
        let doc = self.lock();
        let r = doc.results()?;
        let q = match kind {
            DecayKindRec::Edt => wavey_solver::DecayQuantity::Edt,
            DecayKindRec::T20 => wavey_solver::DecayQuantity::T20,
            DecayKindRec::T30 => wavey_solver::DecayQuantity::T30,
        };
        Some(DecayNeedRec {
            expected_rt_s: r.expected_rt(centre_hz),
            step_needed_hz: r.decay_step_needed(centre_hz, q).unwrap_or(f64::NAN),
            run_step_hz: wavey_solver::decay::uniform_step(&r.freqs).unwrap_or(f64::NAN),
        })
    }

    /// A decay time (s) over a plane in the band at `centre_hz`, one value
    /// per grid cell like the stack's slices (NaN outside the room or where
    /// the run cannot say).
    pub fn plane_decay(&self, plane_id: u64, centre_hz: f64, octave_fraction: f64, kind: DecayKindRec) -> Result<Vec<f32>, WaveyError> {
        let doc = self.lock();
        let r = doc.results().ok_or_else(|| WaveyError::new("no results"))?;
        let i = r.planes.iter().position(|p| p.plane.id == plane_id).ok_or_else(|| WaveyError::new("plane not in results"))?;
        let drives = doc.project().drives(&r.speaker_ids, &r.freqs);
        let q = match kind {
            DecayKindRec::Edt => wavey_solver::DecayQuantity::Edt,
            DecayKindRec::T20 => wavey_solver::DecayQuantity::T20,
            DecayKindRec::T30 => wavey_solver::DecayQuantity::T30,
        };
        Ok(r.plane_decay(i, &drives, centre_hz, octave_fraction, q))
    }

    pub fn clear_results(&self) {
        let mut doc = self.lock();
        if doc.results().is_some() {
            doc.clear_results();
        }
    }
}
