//! Layers, levels, groups, surface names, runs, validation and the
//! result queries the app's results workspace is built on.

use wavey_geom::SurfaceKey;
use wavey_project::meta::{Group, LAYER_CEILING, LAYER_FLOOR, LAYER_WALLS, Layer, Level};

use crate::{MicResponseRec, PlaneAxisRec, Point3, WaveyDocument, WaveyError};

#[derive(Debug, Clone, uniffi::Record)]
pub struct LayerRec {
    pub id: String,
    pub name: String,
    pub color: String,
    pub visible: bool,
    pub locked: bool,
    /// Built-in layers (walls, floor, ceiling) cannot be removed.
    pub builtin: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LevelKind {
    Floor,
    Ceiling,
    Datum,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct LevelRec {
    pub id: String,
    pub name: String,
    pub z: f64,
    pub kind: LevelKind,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct GroupRec {
    pub id: u64,
    pub code: String,
    pub name: String,
    pub members: Vec<u64>,
    /// Microphone ids.
    pub receivers: Vec<u64>,
    /// Group EQ (applied to member speakers on top of their own filters).
    pub filters: Vec<crate::FilterRec>,
    /// Group FIR filter after the EQ; ignored on input (see `set_fir`).
    pub fir: Option<crate::speakers::FirInfoRec>,
    /// Group gain, delay and polarity, on top of each member's own.
    pub gain_db: f64,
    pub delay_ms: f64,
    pub polarity_inverted: bool,
    pub linked: bool,
    /// "stack", "line_array" or "" for a plain group; ignored on input.
    pub kind: String,
    /// Members still where the recipe puts them; ignored on input.
    pub in_sync: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RunRec {
    pub id: u32,
    pub name: String,
    /// Seconds since the Unix epoch.
    pub created: u64,
    pub active: bool,
    /// Results no longer match the model.
    pub stale: bool,
    pub f_min: f64,
    pub f_max: f64,
    /// 1/N octave if > 0, otherwise linear with `linear_step_hz`.
    pub octave_fraction: u32,
    pub linear_step_hz: f64,
    pub frequencies: u64,
    /// Codes of the sources that took part.
    pub sources: Vec<String>,
    pub seconds: f64,
    pub backend: String,
    pub notes: Vec<String>,
}

/// SPL of a plane at every solved frequency.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PlaneStackRec {
    pub plane_id: u64,
    pub axis: PlaneAxisRec,
    pub offset: f64,
    pub nu: u32,
    pub nv: u32,
    pub u0: f64,
    pub v0: f64,
    pub du: f64,
    pub dv: f64,
    pub freqs_hz: Vec<f64>,
    /// `[freq][v][u]` dB, NaN outside the room, as little-endian f32
    /// bytes: a fine-resolution run has ~10⁸ values, and bytes cross the
    /// bridge as one copy where a float list is read value by value.
    pub values_f32le: Vec<u8>,
    /// Audience areas: height of each point (`[v][u]`, NaN outside); empty
    /// for flat planes.
    pub heights: Vec<f32>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RoomModeRec {
    pub nx: u32,
    pub ny: u32,
    pub nz: u32,
    pub freq_hz: f64,
    /// "Axial", "Tangential" or "Oblique".
    pub kind: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum IssueSeverity {
    Warning,
    /// Solving is impossible until fixed.
    Error,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct IssueRec {
    pub severity: IssueSeverity,
    pub message: String,
    /// Entity key (`spk:3`, `mic:4`, `plane:5`) to select and frame, if any.
    pub entity: Option<String>,
}

fn layer_rec(l: &Layer) -> LayerRec {
    LayerRec {
        id: l.id.clone(),
        name: l.name.clone(),
        color: l.color.clone(),
        visible: l.visible,
        locked: l.locked,
        builtin: [LAYER_WALLS, LAYER_FLOOR, LAYER_CEILING].contains(&l.id.as_str()),
    }
}

#[uniffi::export]
impl WaveyDocument {
    // ---- layers ----

    pub fn layers(&self) -> Vec<LayerRec> {
        self.lock()
            .project()
            .meta
            .layers
            .iter()
            .map(layer_rec)
            .collect()
    }

    pub fn current_layer(&self) -> String {
        self.lock().project().meta.current_layer.clone()
    }

    pub fn set_current_layer(&self, id: String) -> Result<(), WaveyError> {
        self.lock()
            .try_edit(|p| {
                p.meta.layer(&id).ok_or("no such layer")?;
                p.meta.current_layer = id;
                Ok::<_, &str>(())
            })
            .map_err(WaveyError::new)
    }

    /// Adds a layer (empty id) or updates one. Returns its id.
    pub fn upsert_layer(&self, layer: LayerRec) -> Result<String, WaveyError> {
        self.lock()
            .try_edit(|p| {
                if layer.name.trim().is_empty() {
                    return Err("layer name must not be empty");
                }
                let mut id = layer.id.clone();
                if id.is_empty() {
                    id = format!("layer-{}", p.alloc_id());
                }
                let l = Layer {
                    id: id.clone(),
                    name: layer.name,
                    color: layer.color,
                    visible: layer.visible,
                    locked: layer.locked,
                };
                match p.meta.layers.iter_mut().find(|x| x.id == id) {
                    Some(slot) => *slot = l,
                    None => p.meta.layers.push(l),
                }
                Ok(id)
            })
            .map_err(WaveyError::new)
    }

    /// Removes a layer; its segments move to the walls layer.
    pub fn remove_layer(&self, id: String) -> Result<(), WaveyError> {
        self.lock()
            .try_edit(|p| {
                if [LAYER_WALLS, LAYER_FLOOR, LAYER_CEILING].contains(&id.as_str()) {
                    return Err("built-in layers cannot be removed");
                }
                p.meta.layers.retain(|l| l.id != id);
                p.meta.segment_layers.retain(|_, l| *l != id);
                if p.meta.current_layer == id {
                    p.meta.current_layer = LAYER_WALLS.into();
                }
                Ok(())
            })
            .map_err(WaveyError::new)
    }

    /// Moves sketch segments (and their walls) to a layer.
    pub fn set_segment_layer(
        &self,
        segment_ids: Vec<u64>,
        layer_id: String,
    ) -> Result<(), WaveyError> {
        self.lock()
            .try_edit(|p| {
                p.meta.layer(&layer_id).ok_or("no such layer")?;
                for s in segment_ids {
                    if layer_id == LAYER_WALLS {
                        p.meta.segment_layers.remove(&s);
                    } else {
                        p.meta.segment_layers.insert(s, layer_id.clone());
                    }
                }
                Ok::<_, &str>(())
            })
            .map_err(WaveyError::new)
    }

    // ---- levels ----

    /// Floor (z 0), ceiling (room height) and reference levels, by height.
    pub fn levels(&self) -> Vec<LevelRec> {
        let doc = self.lock();
        let p = doc.project();
        let mut v = vec![
            LevelRec {
                id: "floor".into(),
                name: "Floor".into(),
                z: 0.0,
                kind: LevelKind::Floor,
            },
            LevelRec {
                id: "ceiling".into(),
                name: "Ceiling".into(),
                z: p.ceiling_height,
                kind: LevelKind::Ceiling,
            },
        ];
        v.extend(p.meta.levels.iter().map(|l| LevelRec {
            id: l.id.clone(),
            name: l.name.clone(),
            z: l.z,
            kind: LevelKind::Datum,
        }));
        v.sort_by(|a, b| a.z.total_cmp(&b.z));
        v
    }

    /// Adds (empty id) or updates a level. Moving the ceiling level changes
    /// the room height; the floor level is fixed at z 0.
    pub fn upsert_level(&self, level: LevelRec) -> Result<String, WaveyError> {
        self.lock()
            .try_edit(|p| match level.id.as_str() {
                "floor" => Err("the floor level is fixed at z 0"),
                "ceiling" => {
                    if !(level.z > 0.0) {
                        return Err("ceiling height must be positive");
                    }
                    p.ceiling_height = level.z;
                    Ok("ceiling".to_string())
                }
                _ => {
                    let id = if level.id.is_empty() {
                        format!("level-{}", p.alloc_id())
                    } else {
                        level.id.clone()
                    };
                    let l = Level {
                        id: id.clone(),
                        name: level.name,
                        z: level.z,
                    };
                    match p.meta.levels.iter_mut().find(|x| x.id == id) {
                        Some(slot) => *slot = l,
                        None => p.meta.levels.push(l),
                    }
                    Ok(id)
                }
            })
            .map_err(WaveyError::new)
    }

    pub fn remove_level(&self, id: String) {
        self.lock().edit(|p| p.meta.levels.retain(|l| l.id != id));
    }

    // ---- surfaces ----

    /// Sets the user names of surfaces (empty clears).
    pub fn set_surface_names(
        &self,
        surface_keys: Vec<String>,
        name: String,
    ) -> Result<(), WaveyError> {
        for k in &surface_keys {
            k.parse::<SurfaceKey>().map_err(WaveyError::new)?;
        }
        self.lock().edit(|p| {
            for k in surface_keys {
                if name.trim().is_empty() {
                    p.meta.surface_names.remove(&k);
                } else {
                    p.meta.surface_names.insert(k, name.trim().to_string());
                }
            }
        });
        Ok(())
    }

    // ---- groups ----

    pub fn groups(&self) -> Vec<GroupRec> {
        let doc = self.lock();
        let p = doc.project();
        p.meta
            .groups
            .values()
            .map(|g| GroupRec {
                id: g.id,
                code: p
                    .meta
                    .code(&format!("group:{}", g.id))
                    .unwrap_or_default()
                    .to_string(),
                name: g.name.clone(),
                members: g.members.clone(),
                receivers: g.receivers.clone(),
                filters: g.filters.iter().map(crate::FilterRec::from).collect(),
                fir: g.fir.as_deref().map(crate::speakers::FirInfoRec::from),
                gain_db: g.gain_db,
                delay_ms: g.delay_ms,
                polarity_inverted: g.polarity_inverted,
                linked: g.linked,
                kind: match g.assembly.as_ref().map(|a| &a.kind) {
                    Some(wavey_project::assembly::AssemblyKind::Stack(_)) => "stack".into(),
                    Some(wavey_project::assembly::AssemblyKind::LineArray(_)) => {
                        "line_array".into()
                    }
                    None => String::new(),
                },
                in_sync: p.assembly_in_sync(g.id),
            })
            .collect()
    }

    pub fn add_group(&self, name: String, members: Vec<u64>) -> u64 {
        self.lock().edit(|p| p.add_group(&name, members))
    }

    /// A group of sources and/or receivers.
    pub fn add_group_with(&self, name: String, members: Vec<u64>, receivers: Vec<u64>) -> u64 {
        self.lock().edit(|p| {
            let id = p.add_group(&name, members);
            p.meta.groups.get_mut(&id).unwrap().receivers = receivers;
            id
        })
    }

    pub fn update_group(&self, group: GroupRec) -> Result<(), WaveyError> {
        self.lock()
            .try_edit(|p| {
                let mut members = group.members.clone();
                members.retain(|m| p.speakers.contains_key(m));
                members.dedup();
                let mut receivers = group.receivers.clone();
                receivers.retain(|m| p.microphones.contains_key(m));
                receivers.dedup();
                let filters: Vec<wavey_speaker::Filter> =
                    group.filters.iter().cloned().map(Into::into).collect();
                for f in &filters {
                    f.validate()?;
                }
                let g = p.meta.groups.get_mut(&group.id).ok_or("no such group")?;
                let assembly = g.assembly.take();
                let fir = g.fir.take();
                *g = Group {
                    id: group.id,
                    name: group.name,
                    members,
                    receivers,
                    filters,
                    fir,
                    gain_db: group.gain_db.clamp(-60.0, 40.0),
                    delay_ms: group.delay_ms.clamp(0.0, 2000.0),
                    polarity_inverted: group.polarity_inverted,
                    linked: group.linked,
                    assembly,
                };
                Ok::<_, String>(())
            })
            .map_err(WaveyError::new)
    }

    pub fn remove_groups(&self, ids: Vec<u64>) {
        self.lock().edit(|p| {
            ids.iter()
                .for_each(|id| p.meta.forget(&format!("group:{id}")))
        });
    }

    // ---- runs ----

    pub fn runs(&self) -> Vec<RunRec> {
        let doc = self.lock();
        let active = doc.active_run().map(|r| r.meta.id);
        doc.runs()
            .iter()
            .map(|r| {
                let (octave_fraction, linear_step_hz) = match r.meta.settings.resolution {
                    wavey_solver::Resolution::FractionalOctave { n } => (n, 1.0),
                    wavey_solver::Resolution::Linear { step_hz } => (0, step_hz),
                };
                RunRec {
                    id: r.meta.id,
                    name: r.meta.name.clone(),
                    created: r.meta.created,
                    active: Some(r.meta.id) == active,
                    stale: doc.run_stale(r),
                    f_min: r.meta.settings.f_min,
                    f_max: r.meta.settings.f_max,
                    octave_fraction,
                    linear_step_hz,
                    frequencies: r.meta.frequencies as u64,
                    sources: r.meta.sources.clone(),
                    seconds: r.meta.stats.seconds,
                    backend: r.meta.stats.backend.clone(),
                    notes: r.meta.stats.notes.clone(),
                }
            })
            .collect()
    }

    pub fn set_active_run(&self, id: u32) -> bool {
        self.lock().set_active_run(id)
    }

    pub fn rename_run(&self, id: u32, name: String) -> bool {
        self.lock().rename_run(id, &name)
    }

    pub fn delete_run(&self, id: u32) -> bool {
        self.lock().delete_run(id)
    }

    /// Result-relevant categories changed since the active run
    /// ("geometry", "materials", "sources", "receivers", "environment", "settings").
    pub fn changes_since_run(&self) -> Vec<String> {
        self.lock().changes_since_run()
    }

    // ---- result queries ----

    /// SPL of a plane at every solved frequency, mixed with the current
    /// speaker settings. Computed without holding the document.
    pub fn plane_stack(&self, plane_id: u64) -> Result<PlaneStackRec, WaveyError> {
        let (r, drives) = {
            let doc = self.lock();
            let r = doc
                .results()
                .ok_or_else(|| WaveyError::new("no results"))?
                .clone();
            let drives = doc.project().drives(&r.speaker_ids, &r.freqs);
            (r, drives)
        };
        let i = r
            .planes
            .iter()
            .position(|p| p.plane.id == plane_id)
            .ok_or_else(|| WaveyError::new("plane not in results"))?;
        let values_db = r.plane_spl_stack(i, &drives);
        let g = &r.planes[i].grid;
        Ok(PlaneStackRec {
            plane_id,
            axis: g.axis.into(),
            offset: g.offset,
            nu: g.nu as u32,
            nv: g.nv as u32,
            u0: g.u0,
            v0: g.v0,
            du: g.du,
            dv: g.dv,
            freqs_hz: r.freqs.clone(),
            values_f32le: values_db.iter().flat_map(|v| v.to_le_bytes()).collect(),
            heights: g.z.iter().map(|z| *z as f32).collect(),
        })
    }

    /// Response at a point on a solved plane, interpolated from the plane's
    /// grid (no re-solve needed). None if the point is outside the plane's
    /// inside cells.
    pub fn plane_probe(
        &self,
        plane_id: u64,
        point: Point3,
    ) -> Result<Option<MicResponseRec>, WaveyError> {
        let (r, drives) = {
            let doc = self.lock();
            let r = doc
                .results()
                .ok_or_else(|| WaveyError::new("no results"))?
                .clone();
            let drives = doc.project().drives(&r.speaker_ids, &r.freqs);
            (r, drives)
        };
        let i = r
            .planes
            .iter()
            .position(|p| p.plane.id == plane_id)
            .ok_or_else(|| WaveyError::new("plane not in results"))?;
        let (u, v) = match r.planes[i].grid.axis {
            wavey_solver::PlaneAxis::Horizontal => (point.x, point.y),
            wavey_solver::PlaneAxis::ConstX => (point.y, point.z),
            wavey_solver::PlaneAxis::ConstY => (point.x, point.z),
        };
        Ok(r.plane_probe(i, u, v, &drives).map(|p| MicResponseRec {
            mic_id: 0,
            freqs_hz: r.freqs.clone(),
            spl_db: p
                .iter()
                .map(|v| wavey_solver::result::spl(v.norm_sqr()))
                .collect(),
            phase_deg: p.iter().map(|v| v.arg().to_degrees()).collect(),
        }))
    }

    /// Modes of the room's bounding box up to `up_to_hz` (rectangular
    /// approximation), by frequency.
    pub fn room_modes(&self, up_to_hz: f64) -> Result<Vec<RoomModeRec>, WaveyError> {
        let mut doc = self.lock();
        let c = doc.project().medium.speed_of_sound();
        let room = doc.room().map_err(WaveyError::new)?;
        let (lo, hi) = room.bounds();
        let l = hi - lo;
        let max = |len: f64| (2.0 * up_to_hz * len / c).floor() as u32;
        let mut modes = Vec::new();
        for nx in 0..=max(l.x) {
            for ny in 0..=max(l.y) {
                for nz in 0..=max(l.z) {
                    if nx + ny + nz == 0 {
                        continue;
                    }
                    let f = c / 2.0
                        * ((nx as f64 / l.x).powi(2)
                            + (ny as f64 / l.y).powi(2)
                            + (nz as f64 / l.z).powi(2))
                        .sqrt();
                    if f <= up_to_hz {
                        let nonzero = [nx, ny, nz].iter().filter(|n| **n > 0).count();
                        let kind = ["", "Axial", "Tangential", "Oblique"][nonzero].to_string();
                        modes.push(RoomModeRec {
                            nx,
                            ny,
                            nz,
                            freq_hz: f,
                            kind,
                        });
                    }
                    if modes.len() > 20_000 {
                        break;
                    }
                }
            }
        }
        modes.sort_by(|a, b| a.freq_hz.total_cmp(&b.freq_hz));
        Ok(modes)
    }

    // ---- validation ----

    /// Non-blocking warnings plus the errors that make a solve impossible.
    pub fn issues(&self) -> Vec<IssueRec> {
        let mut doc = self.lock();
        let mut out = Vec::new();
        let issue = |severity, message: String, entity: Option<String>| IssueRec {
            severity,
            message,
            entity,
        };
        let room = doc.room().map(Clone::clone);
        let p = doc.project();
        let room = match room {
            Ok(r) => Some(r),
            // Without walls the solve is over open air, with a ground.
            Err(_) if p.open_air() => None,
            Err(e) => {
                out.push(issue(
                    IssueSeverity::Error,
                    format!("No simulated room: {e}"),
                    None,
                ));
                None
            }
        };
        let playing: Vec<_> = p.speakers.values().filter(|s| !s.muted).collect();
        if playing.is_empty() {
            out.push(issue(
                IssueSeverity::Error,
                "No sources (all muted or none placed)".into(),
                None,
            ));
        }
        let code = |key: String| p.meta.code(&key).map(str::to_string).unwrap_or(key);
        if let Some(room) = &room {
            for s in &playing {
                if let Some(m) = p.speaker_models.get(&s.model_id)
                    && !room.contains(s.acoustic_center(m))
                {
                    let key = format!("spk:{}", s.id);
                    out.push(issue(
                        IssueSeverity::Warning,
                        format!("{} is outside the volume", code(key.clone())),
                        Some(key),
                    ));
                }
            }
            for m in p.solved_microphones() {
                if !room.contains(m.position) {
                    let key = format!("mic:{}", m.id);
                    out.push(issue(
                        IssueSeverity::Warning,
                        format!("{} is outside the volume", code(key.clone())),
                        Some(key),
                    ));
                }
            }
            let (lo, hi) = room.bounds();
            for c in p.cut_planes.values() {
                let (a, b) = match c.axis {
                    wavey_solver::PlaneAxis::Horizontal => (lo.z, hi.z),
                    wavey_solver::PlaneAxis::ConstX => (lo.x, hi.x),
                    wavey_solver::PlaneAxis::ConstY => (lo.y, hi.y),
                };
                if !(a..=b).contains(&c.offset) {
                    let key = format!("plane:{}", c.id);
                    out.push(issue(
                        IssueSeverity::Warning,
                        format!("{} lies outside the room", code(key.clone())),
                        Some(key),
                    ));
                }
            }
        }
        for (i, a) in playing.iter().enumerate() {
            for b in &playing[i + 1..] {
                if (a.position - b.position).length() < 0.01 {
                    let key = format!("spk:{}", b.id);
                    out.push(issue(
                        IssueSeverity::Warning,
                        format!(
                            "{} and {} coincide",
                            code(format!("spk:{}", a.id)),
                            code(key.clone())
                        ),
                        Some(key),
                    ));
                }
            }
        }
        out
    }
}
