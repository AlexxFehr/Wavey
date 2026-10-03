//! Speaker stacks and line arrays: recipes, layout, quick free-field
//! predictions and the group operations behind the stack and line-array
//! editors.

use wavey_geom::DVec3;
use wavey_project::assembly::{
    self as asm, ArrayTilt, Assembly, AssemblyKind, AutoSplay, ElementSettings, LineArrayRecipe, PolarPlane, StackElement, StackPreset, StackRecipe,
};
use wavey_speaker::Filter;

use crate::speakers::FilterRec;
use crate::{Point3, WaveyDocument, WaveyError};

#[derive(Debug, Clone, uniffi::Record)]
pub struct ElementSettingsRec {
    pub gain_db: f64,
    pub delay_ms: f64,
    pub polarity_inverted: bool,
    pub muted: bool,
    pub filters: Vec<FilterRec>,
}

impl From<&ElementSettings> for ElementSettingsRec {
    fn from(s: &ElementSettings) -> Self {
        Self { gain_db: s.gain_db, delay_ms: s.delay_ms, polarity_inverted: s.polarity_inverted, muted: s.muted, filters: s.filters.iter().map(FilterRec::from).collect() }
    }
}

impl From<ElementSettingsRec> for ElementSettings {
    fn from(s: ElementSettingsRec) -> Self {
        Self { gain_db: s.gain_db, delay_ms: s.delay_ms, polarity_inverted: s.polarity_inverted, muted: s.muted, filters: s.filters.into_iter().map(Filter::from).collect() }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct StackElementRec {
    pub model_id: String,
    /// Enclosure centre relative to the stack origin (footprint centre at the bottom), before the stack's yaw.
    pub offset: Point3,
    pub yaw_deg: f64,
    pub pitch_deg: f64,
    pub roll_deg: f64,
    pub settings: ElementSettingsRec,
}

impl From<&StackElement> for StackElementRec {
    fn from(e: &StackElement) -> Self {
        Self { model_id: e.model_id.clone(), offset: e.offset.into(), yaw_deg: e.yaw_deg, pitch_deg: e.pitch_deg, roll_deg: e.roll_deg, settings: (&e.settings).into() }
    }
}

impl From<StackElementRec> for StackElement {
    fn from(e: StackElementRec) -> Self {
        Self { model_id: e.model_id, offset: e.offset.into(), yaw_deg: e.yaw_deg, pitch_deg: e.pitch_deg, roll_deg: e.roll_deg, settings: e.settings.into() }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum StackPresetRec {
    Custom,
    Cardioid { model_id: String, forward: u32, reversed: u32 },
    Endfire { model_id: String, count: u32, spacing_m: f64 },
    Gradient { model_id: String, spacing_m: f64 },
    Broadside { model_id: String, count: u32, spacing_m: f64, arc_deg: f64 },
}

impl From<&StackPreset> for StackPresetRec {
    fn from(p: &StackPreset) -> Self {
        match p.clone() {
            StackPreset::Custom => Self::Custom,
            StackPreset::Cardioid { model_id, forward, reversed } => Self::Cardioid { model_id, forward, reversed },
            StackPreset::Endfire { model_id, count, spacing_m } => Self::Endfire { model_id, count, spacing_m },
            StackPreset::Gradient { model_id, spacing_m } => Self::Gradient { model_id, spacing_m },
            StackPreset::Broadside { model_id, count, spacing_m, arc_deg } => Self::Broadside { model_id, count, spacing_m, arc_deg },
        }
    }
}

impl From<StackPresetRec> for StackPreset {
    fn from(p: StackPresetRec) -> Self {
        match p {
            StackPresetRec::Custom => Self::Custom,
            StackPresetRec::Cardioid { model_id, forward, reversed } => Self::Cardioid { model_id, forward, reversed },
            StackPresetRec::Endfire { model_id, count, spacing_m } => Self::Endfire { model_id, count, spacing_m },
            StackPresetRec::Gradient { model_id, spacing_m } => Self::Gradient { model_id, spacing_m },
            StackPresetRec::Broadside { model_id, count, spacing_m, arc_deg } => Self::Broadside { model_id, count, spacing_m, arc_deg },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum ArrayTiltRec {
    /// Frame tilt in degrees, positive tilting the array down.
    Manual { deg: f64 },
    /// Hung from this pick point (index); the tilt follows by gravity.
    Pick { index: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct AutoSplayRec {
    pub first_m: f64,
    pub last_m: f64,
    pub listener_z: f64,
    pub rake_deg: f64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct LineArrayRec {
    pub model_id: String,
    pub frame: Option<String>,
    pub frame_splay_deg: f64,
    /// Box-to-box splays, top to bottom (one fewer than boxes).
    pub splays_deg: Vec<f64>,
    pub tilt: ArrayTiltRec,
    /// One per box, top to bottom.
    pub elements: Vec<ElementSettingsRec>,
    pub auto: Option<AutoSplayRec>,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum AssemblyKindRec {
    Stack { preset: StackPresetRec, elements: Vec<StackElementRec> },
    LineArray { recipe: LineArrayRec },
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AssemblyRec {
    pub origin: Point3,
    pub yaw_deg: f64,
    pub kind: AssemblyKindRec,
}

impl From<&Assembly> for AssemblyRec {
    fn from(a: &Assembly) -> Self {
        let kind = match &a.kind {
            AssemblyKind::Stack(s) => AssemblyKindRec::Stack { preset: (&s.preset).into(), elements: s.elements.iter().map(StackElementRec::from).collect() },
            AssemblyKind::LineArray(r) => AssemblyKindRec::LineArray {
                recipe: LineArrayRec {
                    model_id: r.model_id.clone(),
                    frame: r.frame.clone(),
                    frame_splay_deg: r.frame_splay_deg,
                    splays_deg: r.splays_deg.clone(),
                    tilt: match r.tilt {
                        ArrayTilt::Manual { deg } => ArrayTiltRec::Manual { deg },
                        ArrayTilt::Pick { index } => ArrayTiltRec::Pick { index: index as u32 },
                    },
                    elements: r.elements.iter().map(ElementSettingsRec::from).collect(),
                    auto: r.auto.map(|a| AutoSplayRec { first_m: a.first_m, last_m: a.last_m, listener_z: a.listener_z, rake_deg: a.rake_deg }),
                },
            },
        };
        Self { origin: a.origin.into(), yaw_deg: a.yaw_deg, kind }
    }
}

impl From<AssemblyRec> for Assembly {
    fn from(a: AssemblyRec) -> Self {
        let kind = match a.kind {
            AssemblyKindRec::Stack { preset, elements } => {
                AssemblyKind::Stack(StackRecipe { preset: preset.into(), elements: elements.into_iter().map(StackElement::from).collect() })
            }
            AssemblyKindRec::LineArray { recipe: r } => AssemblyKind::LineArray(LineArrayRecipe {
                model_id: r.model_id,
                frame: r.frame,
                frame_splay_deg: r.frame_splay_deg,
                splays_deg: r.splays_deg,
                tilt: match r.tilt {
                    ArrayTiltRec::Manual { deg } => ArrayTilt::Manual { deg },
                    ArrayTiltRec::Pick { index } => ArrayTilt::Pick { index: index as usize },
                },
                elements: r.elements.into_iter().map(ElementSettings::from).collect(),
                auto: r.auto.map(|a| AutoSplay { first_m: a.first_m, last_m: a.last_m, listener_z: a.listener_z, rake_deg: a.rake_deg }),
            }),
        };
        Self { origin: a.origin.into(), yaw_deg: a.yaw_deg, kind }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ElementPoseRec {
    pub model_id: String,
    pub position: Point3,
    pub yaw_deg: f64,
    pub pitch_deg: f64,
    pub roll_deg: f64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FramePoseRec {
    pub key: String,
    pub reference: Point3,
    /// Frame axes in room coordinates (x forward, y left, z up).
    pub axis_x: Point3,
    pub axis_y: Point3,
    pub axis_z: Point3,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AssemblyLayoutRec {
    pub elements: Vec<ElementPoseRec>,
    pub frame: Option<FramePoseRec>,
    pub frame_tilt_deg: f64,
    /// Downward tilt of each box (line arrays).
    pub box_tilt_deg: Vec<f64>,
    pub weight_kg: f64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RigPointRec {
    pub label: String,
    pub position: Point3,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RigFrameRec {
    pub key: String,
    pub label: String,
    pub flown: bool,
    pub weight_kg: f64,
    pub attach: Point3,
    pub pick_points: Vec<RigPointRec>,
    pub splays_deg: Vec<f64>,
    pub max_boxes: Option<u32>,
    /// Pairs of xyz triples, one pair per wireframe edge (frame coordinates).
    pub edge_positions: Vec<f32>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RigPresetRec {
    pub label: String,
    pub splays_deg: Vec<f64>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RiggingRec {
    pub pivot: Point3,
    pub next_pivot: Point3,
    pub weight_kg: f64,
    pub splays_deg: Vec<f64>,
    pub frames: Vec<RigFrameRec>,
    pub presets: Vec<RigPresetRec>,
}

/// A prediction: x values (angles in degrees or distances in metres) and
/// dB SPL per frequency.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PredictionRec {
    pub x: Vec<f64>,
    pub freqs: Vec<f64>,
    pub spl_db: Vec<Vec<f64>>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct TunedRec {
    pub assembly: AssemblyRec,
    pub rear_minus_front_db: f64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PasteRec {
    pub groups: Vec<u64>,
    pub speakers: Vec<u64>,
}

fn p3(v: DVec3) -> Point3 {
    v.into()
}

#[uniffi::export]
impl WaveyDocument {
    /// Rigging of a line-array module, if it has any.
    pub fn speaker_rigging(&self, model_id: String) -> Option<RiggingRec> {
        let doc = self.lock();
        let m = doc.project().speaker_models.get(&model_id)?;
        let r = m.rigging.as_ref()?;
        let v = |a: [f64; 3]| p3(DVec3::from_array(a));
        Some(RiggingRec {
            pivot: v(r.pivot),
            next_pivot: v(r.next_pivot),
            weight_kg: r.weight_kg,
            splays_deg: r.splays_deg.clone(),
            frames: r
                .frames
                .iter()
                .map(|f| RigFrameRec {
                    key: f.key.clone(),
                    label: f.label.clone(),
                    flown: f.flown,
                    weight_kg: f.weight_kg,
                    attach: v(f.attach),
                    pick_points: f.pick_points.iter().map(|p| RigPointRec { label: p.label.clone(), position: v(p.position) }).collect(),
                    splays_deg: f.splays_deg.clone(),
                    max_boxes: f.max_boxes,
                    edge_positions: f
                        .geometry
                        .as_ref()
                        .map(|g| g.edges.iter().flat_map(|e| e.iter().flat_map(|i| g.vertices[*i as usize].map(|c| c as f32))).collect())
                        .unwrap_or_default(),
                })
                .collect(),
            presets: r.presets.iter().map(|p| RigPresetRec { label: p.label.clone(), splays_deg: p.splays_deg.clone() }).collect(),
        })
    }

    /// Elements a stack preset generates (delays from the project's speed of sound).
    pub fn stack_preset_elements(&self, preset: StackPresetRec) -> Result<Vec<StackElementRec>, WaveyError> {
        let doc = self.lock();
        let p = doc.project();
        let c = p.medium.speed_of_sound();
        let els = StackPreset::from(preset).generate(&p.speaker_models, c).map_err(WaveyError::new)?;
        Ok(els.iter().map(StackElementRec::from).collect())
    }

    /// Speaker poses, frame and limit warnings of an assembly (no edit).
    pub fn assembly_layout(&self, assembly: AssemblyRec) -> Result<AssemblyLayoutRec, WaveyError> {
        let models = self.lock().project().speaker_models.clone();
        let lay = asm::layout(&assembly.into(), &models).map_err(WaveyError::new)?;
        Ok(AssemblyLayoutRec {
            elements: lay
                .elements
                .iter()
                .map(|e| ElementPoseRec { model_id: e.model_id.clone(), position: p3(e.position), yaw_deg: e.yaw_deg, pitch_deg: e.pitch_deg, roll_deg: e.roll_deg })
                .collect(),
            frame: lay.frame.map(|f| FramePoseRec {
                key: f.key,
                reference: p3(f.reference),
                axis_x: p3(f.rotation.x_axis),
                axis_y: p3(f.rotation.y_axis),
                axis_z: p3(f.rotation.z_axis),
            }),
            frame_tilt_deg: lay.frame_tilt_deg,
            box_tilt_deg: lay.box_tilt_deg,
            weight_kg: lay.weight_kg,
            warnings: lay.warnings,
        })
    }

    /// The assembly with splays (and frame tilt or pick point) aimed at its audience.
    pub fn assembly_auto_splay(&self, assembly: AssemblyRec) -> Result<AssemblyRec, WaveyError> {
        let models = self.lock().project().speaker_models.clone();
        let mut a: Assembly = assembly.into();
        asm::auto_splay(&mut a, &models).map_err(WaveyError::new)?;
        Ok((&a).into())
    }

    /// Tunes the delay and gain of a stack's polarity-inverted elements for
    /// the most rear rejection over `f_lo`…`f_hi` (free field); returns the
    /// tuned assembly and the rear − front level (dB).
    pub fn assembly_tune_rear(&self, assembly: AssemblyRec, f_lo: f64, f_hi: f64) -> Result<TunedRec, WaveyError> {
        let (models, medium) = {
            let doc = self.lock();
            (doc.project().speaker_models.clone(), doc.project().medium)
        };
        let mut a: Assembly = assembly.into();
        let rear_db = asm::tune_rear(&mut a, &models, &medium, f_lo, f_hi).map_err(WaveyError::new)?;
        Ok(TunedRec { assembly: (&a).into(), rear_minus_front_db: rear_db })
    }

    /// Free-field polar SPL around the assembly (horizontal or vertical
    /// plane), `width_oct`-band averaged (0: single frequencies).
    pub fn assembly_polar(&self, assembly: AssemblyRec, freqs: Vec<f64>, vertical: bool, radius_m: f64, step_deg: f64, width_oct: f64) -> Result<PredictionRec, WaveyError> {
        let (models, medium) = {
            let doc = self.lock();
            (doc.project().speaker_models.clone(), doc.project().medium)
        };
        let plane = if vertical { PolarPlane::Vertical } else { PolarPlane::Horizontal };
        let (x, spl_db) = asm::predict_polar(&assembly.into(), &models, &medium, &freqs, plane, radius_m, step_deg, width_oct).map_err(WaveyError::new)?;
        Ok(PredictionRec { x, freqs, spl_db })
    }

    /// Free-field SPL along a line ahead of the assembly (the audience of a
    /// line array): distances `from_m`…`to_m`, height `z0` rising by `rake_deg`.
    #[allow(clippy::too_many_arguments)]
    pub fn assembly_distance(
        &self,
        assembly: AssemblyRec,
        freqs: Vec<f64>,
        from_m: f64,
        to_m: f64,
        z0: f64,
        rake_deg: f64,
        points: u32,
        width_oct: f64,
    ) -> Result<PredictionRec, WaveyError> {
        let (models, medium) = {
            let doc = self.lock();
            (doc.project().speaker_models.clone(), doc.project().medium)
        };
        let (x, spl_db) = asm::predict_distance(&assembly.into(), &models, &medium, &freqs, from_m, to_m, z0, rake_deg, points as usize, width_oct)
            .map_err(WaveyError::new)?;
        Ok(PredictionRec { x, freqs, spl_db })
    }

    /// Inserts an assembly as a new group; returns the group id.
    pub fn insert_assembly(&self, name: String, assembly: AssemblyRec) -> Result<u64, WaveyError> {
        self.lock().try_edit(|p| p.insert_assembly(&name, assembly.into())).map_err(WaveyError::new)
    }

    /// Replaces a group's recipe and re-lays out its members.
    pub fn update_assembly(&self, group: u64, assembly: AssemblyRec) -> Result<(), WaveyError> {
        self.lock().try_edit(|p| p.update_assembly(group, assembly.into())).map_err(WaveyError::new)
    }

    /// Moves and turns a stack or array as a whole.
    pub fn transform_assembly(&self, group: u64, origin: Point3, yaw_deg: f64) -> Result<(), WaveyError> {
        self.lock().try_edit(|p| p.transform_assembly(group, origin.into(), yaw_deg)).map_err(WaveyError::new)
    }

    /// Keeps the speakers as a plain group, dropping the recipe.
    pub fn detach_assembly(&self, group: u64) -> Result<(), WaveyError> {
        self.lock().try_edit(|p| p.detach_assembly(group)).map_err(WaveyError::new)
    }

    pub fn group_assembly(&self, group: u64) -> Option<AssemblyRec> {
        self.lock().project().meta.groups.get(&group)?.assembly.as_ref().map(AssemblyRec::from)
    }

    /// Duplicates a group (with its recipe) moved by `offset`; returns the new group.
    pub fn duplicate_group(&self, group: u64, offset: Point3) -> Result<u64, WaveyError> {
        self.lock().try_edit(|p| p.duplicate_group(group, offset.into())).map_err(WaveyError::new)
    }

    /// Clipboard text for groups (with recipes) and loose speakers.
    pub fn copy_clip(&self, groups: Vec<u64>, speakers: Vec<u64>) -> Result<String, WaveyError> {
        self.lock().project().clip(&groups, &speakers).map_err(WaveyError::new)
    }

    /// Pastes clipboard text, moved by `offset`.
    pub fn paste_clip(&self, text: String, offset: Point3) -> Result<PasteRec, WaveyError> {
        let (groups, speakers) = self.lock().try_edit(|p| p.paste_clip(&text, offset.into())).map_err(WaveyError::new)?;
        Ok(PasteRec { groups, speakers })
    }
}
