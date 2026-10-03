//! Speaker models and placed speakers.

use std::path::PathBuf;

use wavey_geom::DVec3;
use wavey_speaker::{Filter, Reference, Slope, SpeakerInstance, SpeakerModel, import_table, samples};

use wavey_project::Project;
use wavey_project::meta::AimTarget;

use crate::{Point3, WaveyDocument, WaveyError};

#[derive(Debug, Clone, uniffi::Record)]
pub struct SpeakerModelRec {
    pub id: String,
    pub name: String,
    pub manufacturer: Option<String>,
    pub min_hz: f64,
    pub max_hz: f64,
    /// On-axis sensitivity at `reference_hz` (1 kHz, or the band centre).
    pub sensitivity_db: f64,
    pub reference_hz: f64,
    pub nominal_min_hz: Option<f64>,
    pub nominal_max_hz: Option<f64>,
    pub reference_distance_m: f64,
    pub reference_voltage_rms: f64,
    pub has_balloon: bool,
    pub phase_estimated: bool,
    pub width_m: f64,
    pub height_m: f64,
    pub depth_m: f64,
}

impl From<&SpeakerModel> for SpeakerModelRec {
    fn from(m: &SpeakerModel) -> Self {
        let (lo, hi) = m.frequency_range();
        SpeakerModelRec {
            id: m.id.clone(),
            name: m.name.clone(),
            manufacturer: m.manufacturer.clone(),
            min_hz: lo,
            max_hz: hi,
            sensitivity_db: m.sensitivity_db(m.reference_frequency()),
            reference_hz: m.reference_frequency(),
            nominal_min_hz: m.nominal_band_hz.map(|b| b[0]),
            nominal_max_hz: m.nominal_band_hz.map(|b| b[1]),
            reference_distance_m: m.reference.distance_m,
            reference_voltage_rms: m.reference.input_voltage_rms,
            has_balloon: m.balloon.is_some(),
            phase_estimated: m.phase_estimated,
            width_m: m.enclosure.width_m,
            height_m: m.enclosure.height_m,
            depth_m: m.enclosure.depth_m,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SlopeRec {
    Butterworth12,
    Butterworth24,
    LinkwitzRiley24,
    LinkwitzRiley48,
}

impl From<Slope> for SlopeRec {
    fn from(s: Slope) -> Self {
        match s {
            Slope::Butterworth12 => Self::Butterworth12,
            Slope::Butterworth24 => Self::Butterworth24,
            Slope::LinkwitzRiley24 => Self::LinkwitzRiley24,
            Slope::LinkwitzRiley48 => Self::LinkwitzRiley48,
        }
    }
}

impl From<SlopeRec> for Slope {
    fn from(s: SlopeRec) -> Self {
        match s {
            SlopeRec::Butterworth12 => Self::Butterworth12,
            SlopeRec::Butterworth24 => Self::Butterworth24,
            SlopeRec::LinkwitzRiley24 => Self::LinkwitzRiley24,
            SlopeRec::LinkwitzRiley48 => Self::LinkwitzRiley48,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum FilterRec {
    HighPass { freq_hz: f64, slope: SlopeRec },
    LowPass { freq_hz: f64, slope: SlopeRec },
    LowShelf { freq_hz: f64, gain_db: f64 },
    HighShelf { freq_hz: f64, gain_db: f64 },
    Peak { freq_hz: f64, gain_db: f64, q: f64 },
}

impl From<&Filter> for FilterRec {
    fn from(f: &Filter) -> Self {
        match *f {
            Filter::HighPass { freq_hz, slope } => Self::HighPass { freq_hz, slope: slope.into() },
            Filter::LowPass { freq_hz, slope } => Self::LowPass { freq_hz, slope: slope.into() },
            Filter::LowShelf { freq_hz, gain_db } => Self::LowShelf { freq_hz, gain_db },
            Filter::HighShelf { freq_hz, gain_db } => Self::HighShelf { freq_hz, gain_db },
            Filter::Peak { freq_hz, gain_db, q } => Self::Peak { freq_hz, gain_db, q },
        }
    }
}

impl From<FilterRec> for Filter {
    fn from(f: FilterRec) -> Self {
        match f {
            FilterRec::HighPass { freq_hz, slope } => Self::HighPass { freq_hz, slope: slope.into() },
            FilterRec::LowPass { freq_hz, slope } => Self::LowPass { freq_hz, slope: slope.into() },
            FilterRec::LowShelf { freq_hz, gain_db } => Self::LowShelf { freq_hz, gain_db },
            FilterRec::HighShelf { freq_hz, gain_db } => Self::HighShelf { freq_hz, gain_db },
            FilterRec::Peak { freq_hz, gain_db, q } => Self::Peak { freq_hz, gain_db, q },
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SpeakerRec {
    pub id: u64,
    pub name: String,
    pub model_id: String,
    pub position: Point3,
    pub yaw_deg: f64,
    pub pitch_deg: f64,
    pub roll_deg: f64,
    pub gain_db: f64,
    pub delay_ms: f64,
    pub polarity_inverted: bool,
    pub muted: bool,
    /// Stand on the floor; `position.z` then follows from `bottom_height_m`.
    pub on_floor: bool,
    pub bottom_height_m: f64,
    pub filters: Vec<FilterRec>,
    /// Display code ("S1"); ignored on input.
    pub code: String,
    /// Only soloed speakers play in result mixes (when any is soloed).
    pub solo: bool,
    /// Keeps yaw and pitch pointed at this target.
    pub aim: Option<AimTargetRec>,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum AimTargetRec {
    Microphone { id: u64 },
    Point { p: Point3 },
}

impl From<AimTarget> for AimTargetRec {
    fn from(a: AimTarget) -> Self {
        match a {
            AimTarget::Microphone { id } => Self::Microphone { id },
            AimTarget::Point { p } => Self::Point { p: p.into() },
        }
    }
}

impl From<AimTargetRec> for AimTarget {
    fn from(a: AimTargetRec) -> Self {
        match a {
            AimTargetRec::Microphone { id } => Self::Microphone { id },
            AimTargetRec::Point { p } => Self::Point { p: p.into() },
        }
    }
}

impl SpeakerRec {
    pub(crate) fn new(p: &Project, s: &SpeakerInstance) -> Self {
        let meta = p.meta.sources.get(&s.id).cloned().unwrap_or_default();
        SpeakerRec {
            code: p.meta.code(&format!("spk:{}", s.id)).unwrap_or_default().to_string(),
            solo: meta.solo,
            aim: meta.aim.map(Into::into),
            ..SpeakerRec::from(s)
        }
    }
}

impl From<&SpeakerInstance> for SpeakerRec {
    fn from(s: &SpeakerInstance) -> Self {
        SpeakerRec {
            code: String::new(),
            solo: false,
            aim: None,
            id: s.id,
            name: s.name.clone(),
            model_id: s.model_id.clone(),
            position: s.position.into(),
            yaw_deg: s.yaw_deg,
            pitch_deg: s.pitch_deg,
            roll_deg: s.roll_deg,
            gain_db: s.gain_db,
            delay_ms: s.delay_ms,
            polarity_inverted: s.polarity_inverted,
            muted: s.muted,
            on_floor: s.on_floor,
            bottom_height_m: s.bottom_height_m,
            filters: s.filters.iter().map(FilterRec::from).collect(),
        }
    }
}

impl From<SpeakerRec> for SpeakerInstance {
    fn from(s: SpeakerRec) -> Self {
        SpeakerInstance {
            id: s.id,
            name: s.name,
            model_id: s.model_id,
            position: s.position.into(),
            yaw_deg: s.yaw_deg,
            pitch_deg: s.pitch_deg,
            roll_deg: s.roll_deg,
            gain_db: s.gain_db,
            delay_ms: s.delay_ms,
            polarity_inverted: s.polarity_inverted,
            muted: s.muted,
            on_floor: s.on_floor,
            bottom_height_m: s.bottom_height_m,
            filters: s.filters.into_iter().map(Filter::from).collect(),
        }
    }
}

/// Solid cabinet mesh (convex hull) plus the outline edges, in the
/// speaker's local frame relative to the enclosure centre.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CabinetMeshRec {
    /// xyz triples.
    pub positions: Vec<f32>,
    pub indices: Vec<u32>,
    /// Pairs of xyz triples, one pair per outline edge.
    pub edge_positions: Vec<f32>,
    /// Acoustic centre relative to the enclosure centre.
    pub acoustic_center: Point3,
}

/// Horizontal (φ = 0 plane, +y = left) and vertical (+z up) polar responses
/// in dB relative to on-axis, at `angles_deg` (0 = on-axis).
#[derive(Debug, Clone, uniffi::Record)]
pub struct PolarRec {
    pub angles_deg: Vec<f64>,
    pub horizontal_db: Vec<f64>,
    pub vertical_db: Vec<f64>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ResponseRec {
    pub freqs_hz: Vec<f64>,
    pub magnitude_db: Vec<f64>,
    pub phase_deg: Vec<f64>,
}

/// Directivity balloon mesh in the speaker's local frame. Radius is 1 at
/// 0 dB and 0 at `floor_db`; `level_db` per vertex is for coloring.
#[derive(Debug, Clone, uniffi::Record)]
pub struct BalloonMeshRec {
    pub positions: Vec<f32>,
    pub level_db: Vec<f32>,
    pub indices: Vec<u32>,
    pub floor_db: f64,
}

fn rename(mut m: SpeakerModel, name: Option<String>) -> SpeakerModel {
    if let Some(n) = name.filter(|n| !n.trim().is_empty()) {
        m.name = n;
    }
    m
}

fn slug(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_string()
}

#[uniffi::export]
impl WaveyDocument {
    pub fn speaker_models(&self) -> Vec<SpeakerModelRec> {
        self.lock().project().speaker_models.values().map(|m| SpeakerModelRec::from(m.as_ref())).collect()
    }

    pub fn add_sample_speaker_models(&self) -> Result<(), WaveyError> {
        self.lock()
            .try_edit(|p| {
                for m in samples::all() {
                    p.add_speaker_model(m)?;
                }
                Ok::<_, String>(())
            })
            .map_err(WaveyError::new)
    }

    /// Imports a `.waveyspk` file and returns the model id.
    pub fn import_speaker_file(&self, path: String) -> Result<String, WaveyError> {
        let m = SpeakerModel::load(&PathBuf::from(path)).map_err(WaveyError::new)?;
        self.lock().try_edit(|p| p.add_speaker_model(m)).map_err(WaveyError::new)
    }

    /// Imports a polar data table (see docs/speaker-format.md) and returns the model id.
    pub fn import_speaker_table(
        &self,
        path: String,
        name: String,
        reference_distance_m: f64,
        reference_voltage_rms: f64,
    ) -> Result<String, WaveyError> {
        let text = std::fs::read_to_string(&path).map_err(WaveyError::new)?;
        let reference = Reference { distance_m: reference_distance_m, input_voltage_rms: reference_voltage_rms };
        let id = if slug(&name).is_empty() { "imported".to_string() } else { slug(&name) };
        let m = import_table(&text, &id, &name, reference).map_err(WaveyError::new)?;
        self.lock().try_edit(|p| p.add_speaker_model(m)).map_err(WaveyError::new)
    }

    pub fn export_speaker_model(&self, id: String, path: String) -> Result<(), WaveyError> {
        let doc = self.lock();
        let m = doc.project().speaker_models.get(&id).ok_or_else(|| WaveyError::new("no such model"))?;
        m.save(&PathBuf::from(path)).map_err(WaveyError::new)
    }

    pub fn rename_speaker_model(&self, id: String, name: Option<String>) -> Result<(), WaveyError> {
        self.lock()
            .try_edit(|p| {
                let m = p.speaker_models.get(&id).ok_or("no such model")?;
                let renamed = rename((**m).clone(), name);
                p.speaker_models.insert(id.clone(), std::sync::Arc::new(renamed));
                Ok::<_, &str>(())
            })
            .map_err(WaveyError::new)
    }

    pub fn remove_speaker_model(&self, id: String) -> Result<(), WaveyError> {
        self.lock().try_edit(|p| p.remove_speaker_model(&id)).map_err(WaveyError::new)
    }

    pub fn speakers(&self) -> Vec<SpeakerRec> {
        let doc = self.lock();
        let p = doc.project();
        p.speakers.values().map(|s| SpeakerRec::new(p, s)).collect()
    }

    /// Places a speaker. With `on_floor` it stands on the floor instead of
    /// being centred at `position.z`.
    pub fn add_speaker(&self, model_id: String, position: Point3, on_floor: bool) -> Result<SpeakerRec, WaveyError> {
        let mut doc = self.lock();
        let id = doc.try_edit(|p| p.place_speaker(&model_id, position.into(), on_floor)).map_err(WaveyError::new)?;
        let p = doc.project();
        Ok(SpeakerRec::new(p, &p.speakers[&id]))
    }

    pub fn cabinet_mesh(&self, model_id: String) -> Result<CabinetMeshRec, WaveyError> {
        let doc = self.lock();
        let m = doc.project().speaker_models.get(&model_id).ok_or_else(|| WaveyError::new("no such model"))?;
        let (verts, tris) = m.cabinet_mesh();
        let mut edge_positions = Vec::new();
        match &m.geometry {
            // Detailed manufacturer wireframes include drivers and horn
            // walls inside the cabinet; draw the hull's corners instead.
            Some(g) if g.edges.len() > 64 => {
                for [a, b] in wavey_speaker::hull::feature_edges(&verts, &tris, 20.0) {
                    for v in [verts[a as usize], verts[b as usize]] {
                        edge_positions.extend([v.x as f32, v.y as f32, v.z as f32]);
                    }
                }
            }
            Some(g) if !g.edges.is_empty() => {
                for e in &g.edges {
                    for i in e {
                        edge_positions.extend(g.vertices[*i as usize].map(|c| c as f32));
                    }
                }
            }
            _ => {
                // Box outline: the 12 edges between hull corners that differ in one axis.
                for (i, a) in verts.iter().enumerate() {
                    for b in &verts[i + 1..] {
                        let d = *b - *a;
                        if [d.x, d.y, d.z].iter().filter(|c| c.abs() > 1e-9).count() == 1 {
                            edge_positions.extend([a.x, a.y, a.z, b.x, b.y, b.z].map(|c| c as f32));
                        }
                    }
                }
            }
        }
        Ok(CabinetMeshRec {
            positions: verts.iter().flat_map(|v| [v.x as f32, v.y as f32, v.z as f32]).collect(),
            indices: tris.iter().flatten().copied().collect(),
            edge_positions,
            acoustic_center: wavey_geom::DVec3::from_array(m.acoustic_center_m).into(),
        })
    }

    pub fn update_speaker(&self, speaker: SpeakerRec) -> Result<(), WaveyError> {
        self.update_speakers(vec![speaker])
    }

    /// Updates several speakers as one undo step.
    pub fn update_speakers(&self, speakers: Vec<SpeakerRec>) -> Result<(), WaveyError> {
        self.lock()
            .try_edit(|p| {
                for s in speakers {
                    let (id, solo, aim) = (s.id, s.solo, s.aim.map(AimTarget::from));
                    p.update_speaker(s.into())?;
                    p.sync_assembly_settings(id);
                    let m = p.meta.sources.entry(id).or_default();
                    m.solo = solo;
                    m.aim = aim;
                }
                Ok::<_, String>(())
            })
            .map_err(WaveyError::new)
    }

    pub fn remove_speakers(&self, ids: Vec<u64>) {
        self.lock().edit(|p| ids.iter().for_each(|id| p.remove_speaker(*id)));
    }

    /// Magnitude (dB) of a speaker's processing (gain + filters) on a log grid,
    /// for plotting.
    pub fn speaker_processing(&self, speaker: SpeakerRec) -> ResponseRec {
        let inst: SpeakerInstance = speaker.into();
        let freqs: Vec<f64> = (0..=120).map(|i| 20.0 * 1000f64.powf(i as f64 / 120.0)).collect();
        let h: Vec<_> = freqs.iter().map(|f| inst.processing(*f)).collect();
        ResponseRec {
            magnitude_db: h.iter().map(|v| 20.0 * v.norm().max(1e-9).log10()).collect(),
            phase_deg: h.iter().map(|v| v.arg().to_degrees()).collect(),
            freqs_hz: freqs,
        }
    }

    pub fn speaker_polar(&self, model_id: String, freq_hz: f64) -> Result<PolarRec, WaveyError> {
        let doc = self.lock();
        let m = doc.project().speaker_models.get(&model_id).ok_or_else(|| WaveyError::new("no such model"))?;
        let angles: Vec<f64> = (0..72).map(|i| i as f64 * 5.0).collect();
        let level = |d: DVec3| 20.0 * m.directivity(freq_hz, d).norm().max(1e-6).log10();
        let (mut h, mut v) = (Vec::new(), Vec::new());
        for a in &angles {
            let r = a.to_radians();
            h.push(level(DVec3::new(r.cos(), r.sin(), 0.0)));
            v.push(level(DVec3::new(r.cos(), 0.0, r.sin())));
        }
        Ok(PolarRec { angles_deg: angles, horizontal_db: h, vertical_db: v })
    }

    pub fn speaker_response(&self, model_id: String) -> Result<ResponseRec, WaveyError> {
        let doc = self.lock();
        let m = doc.project().speaker_models.get(&model_id).ok_or_else(|| WaveyError::new("no such model"))?;
        Ok(ResponseRec {
            freqs_hz: m.frequencies_hz.clone(),
            magnitude_db: m.on_axis.magnitude_db_spl.clone(),
            phase_deg: m.on_axis.phase_deg.clone(),
        })
    }

    pub fn speaker_balloon(&self, model_id: String, freq_hz: f64, floor_db: f64) -> Result<BalloonMeshRec, WaveyError> {
        let doc = self.lock();
        let m = doc.project().speaker_models.get(&model_id).ok_or_else(|| WaveyError::new("no such model"))?;
        let (nt, np) = (36usize, 48usize);
        let mut positions = Vec::new();
        let mut level_db = Vec::new();
        for i in 0..=nt {
            let th = std::f64::consts::PI * i as f64 / nt as f64;
            for j in 0..np {
                let ph = 2.0 * std::f64::consts::PI * j as f64 / np as f64;
                let d = DVec3::new(th.cos(), th.sin() * ph.cos(), th.sin() * ph.sin());
                let db = (20.0 * m.directivity(freq_hz, d).norm().max(1e-6).log10()).max(floor_db);
                let r = 1.0 - db / floor_db;
                positions.extend([(d.x * r) as f32, (d.y * r) as f32, (d.z * r) as f32]);
                level_db.push(db as f32);
            }
        }
        let mut indices = Vec::new();
        for i in 0..nt as u32 {
            for j in 0..np as u32 {
                let a = i * np as u32 + j;
                let b = i * np as u32 + (j + 1) % np as u32;
                let (c, d) = (a + np as u32, b + np as u32);
                indices.extend([a, c, b, b, c, d]);
            }
        }
        Ok(BalloonMeshRec { positions, level_db, indices, floor_db })
    }
}
