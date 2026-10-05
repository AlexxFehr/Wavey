//! Speaker assemblies: stacks (sub arrays, cardioid stacks, endfire, …) and
//! line arrays. An assembly is a recipe kept on a group; [`layout`] turns it
//! into speaker poses, and the group's members are ordinary speakers laid
//! out from it, so solving needs nothing new.
//!
//! Line arrays hang by the module's rigging (from the GLL file): the top box
//! hangs from the frame, each box from the one above at its `next_pivot`,
//! tilted further down by the splay between them. The frame's tilt is set
//! directly or follows from the pick point by gravity (the combined centre
//! of mass hangs straight below it).

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{DMat3, DVec3};
use serde::{Deserialize, Serialize};
use wavey_speaker::{Filter, SpeakerInstance, SpeakerModel};

use crate::Project;

/// Per-element signal settings (applied when mixing).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ElementSettings {
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub delay_ms: f64,
    #[serde(default)]
    pub polarity_inverted: bool,
    #[serde(default)]
    pub muted: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<Filter>,
}

/// One speaker of a stack, relative to the stack origin (the centre of its
/// footprint at the bottom) before the stack's yaw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StackElement {
    pub model_id: String,
    pub offset: DVec3,
    #[serde(default)]
    pub yaw_deg: f64,
    #[serde(default)]
    pub pitch_deg: f64,
    #[serde(default)]
    pub roll_deg: f64,
    #[serde(default)]
    pub settings: ElementSettings,
}

fn one() -> u32 {
    1
}

/// How a stack was generated; the elements stay editable afterwards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StackPreset {
    Custom,
    /// A plain stack, all boxes facing front: `count` high and `columns`
    /// wide (side by side, touching).
    Column {
        model_id: String,
        count: u32,
        #[serde(default = "one")]
        columns: u32,
    },
    /// Boxes stacked vertically: `forward` facing front at the bottom,
    /// `reversed` facing back above them, polarity inverted and delayed by
    /// the cabinet depth.
    Cardioid { model_id: String, forward: u32, reversed: u32 },
    /// `count` boxes in line front to back, delayed so they add in front.
    Endfire { model_id: String, count: u32, spacing_m: f64 },
    /// Two boxes in line, the rear one inverted and delayed by spacing/c.
    Gradient { model_id: String, spacing_m: f64 },
    /// `count` boxes side by side; `arc_deg` > 0 delays the inner ones to
    /// curve the wavefront like an arc of that angle.
    Broadside { model_id: String, count: u32, spacing_m: f64, arc_deg: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StackRecipe {
    pub preset: StackPreset,
    pub elements: Vec<StackElement>,
}

/// How a line array's frame is tilted.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArrayTilt {
    /// Frame tilt in degrees, positive tilting the array down.
    Manual { deg: f64 },
    /// Hung from this pick point of the frame; the tilt follows by gravity.
    Pick { index: usize },
}

/// Audience the auto-splay aims at: a line in the array's vertical plane
/// from `first_m` to `last_m` (horizontal distance from the origin), at
/// `listener_z` at the first row, rising by `rake_deg`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutoSplay {
    pub first_m: f64,
    pub last_m: f64,
    pub listener_z: f64,
    #[serde(default)]
    pub rake_deg: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineArrayRecipe {
    pub model_id: String,
    /// Rigging frame key; None hangs the top box directly at the origin.
    #[serde(default)]
    pub frame: Option<String>,
    /// Angle between the frame and the adjacent box (an allowed frame splay).
    #[serde(default)]
    pub frame_splay_deg: f64,
    /// Box-to-box splays, top to bottom (one fewer than boxes).
    pub splays_deg: Vec<f64>,
    pub tilt: ArrayTilt,
    /// One per box, top to bottom.
    pub elements: Vec<ElementSettings>,
    #[serde(default)]
    pub auto: Option<AutoSplay>,
}

impl LineArrayRecipe {
    pub fn count(&self) -> usize {
        self.elements.len()
    }

    /// Changes the box count, keeping existing boxes and repeating the last
    /// splay for added ones.
    pub fn set_count(&mut self, n: usize) {
        let n = n.max(1);
        let last = self.splays_deg.last().copied().unwrap_or(0.0);
        self.splays_deg.resize(n - 1, last);
        self.elements.resize(n, ElementSettings::default());
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssemblyKind {
    Stack(StackRecipe),
    LineArray(LineArrayRecipe),
}

/// A stack or line array: the recipe and where it stands. `origin` is the
/// stack's footprint centre, a flown array's pick point (or frame reference
/// with a manual tilt), or a ground stack's frame reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assembly {
    pub origin: DVec3,
    #[serde(default)]
    pub yaw_deg: f64,
    pub kind: AssemblyKind,
}

/// One laid-out speaker.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementPose {
    pub model_id: String,
    /// Enclosure centre.
    pub position: DVec3,
    pub yaw_deg: f64,
    pub pitch_deg: f64,
    pub roll_deg: f64,
    pub settings: ElementSettings,
}

impl ElementPose {
    pub fn rotation(&self) -> DMat3 {
        DMat3::from_rotation_z(self.yaw_deg.to_radians())
            * DMat3::from_rotation_y(-self.pitch_deg.to_radians())
            * DMat3::from_rotation_x(self.roll_deg.to_radians())
    }
}

/// A placed rigging frame (for drawing).
#[derive(Debug, Clone, PartialEq)]
pub struct FramePose {
    pub key: String,
    pub reference: DVec3,
    pub rotation: DMat3,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub elements: Vec<ElementPose>,
    pub frame: Option<FramePose>,
    /// Line arrays: frame tilt and each box's downward tilt (degrees).
    pub frame_tilt_deg: f64,
    pub box_tilt_deg: Vec<f64>,
    /// Line arrays: total weight (frame and boxes) when known.
    pub weight_kg: f64,
    /// Things outside the manufacturer's limits.
    pub warnings: Vec<String>,
}

pub type Models = BTreeMap<String, Arc<SpeakerModel>>;

fn model<'a>(models: &'a Models, id: &str) -> Result<&'a SpeakerModel, String> {
    models.get(id).map(|m| m.as_ref()).ok_or_else(|| format!("no speaker model `{id}`"))
}

fn v3(a: [f64; 3]) -> DVec3 {
    DVec3::from_array(a)
}

/// Lays out an assembly into speaker poses (room coordinates).
pub fn layout(a: &Assembly, models: &Models) -> Result<Layout, String> {
    let yaw = DMat3::from_rotation_z(a.yaw_deg.to_radians());
    match &a.kind {
        AssemblyKind::Stack(s) => {
            let elements = s
                .elements
                .iter()
                .map(|e| {
                    model(models, &e.model_id)?;
                    Ok(ElementPose {
                        model_id: e.model_id.clone(),
                        position: a.origin + yaw * e.offset,
                        yaw_deg: e.yaw_deg + a.yaw_deg,
                        pitch_deg: e.pitch_deg,
                        roll_deg: e.roll_deg,
                        settings: e.settings.clone(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(Layout { elements, frame: None, frame_tilt_deg: 0.0, box_tilt_deg: vec![], weight_kg: 0.0, warnings: vec![] })
        }
        AssemblyKind::LineArray(r) => line_array(a, r, models),
    }
}

/// Rotation of a box (or frame) tilted down by `deg` (before yaw).
fn tilt(deg: f64) -> DMat3 {
    DMat3::from_rotation_y(deg.to_radians())
}

/// Line array in the yaw-free frame with the frame reference at `f_ref`
/// and frame tilt `t`: box centres, box tilts, centre of mass and weight.
struct Chain {
    centres: Vec<DVec3>,
    tilts: Vec<f64>,
    com: DVec3,
    weight: f64,
}

fn chain(r: &LineArrayRecipe, m: &SpeakerModel, frame: Option<&wavey_speaker::RigFrame>, f_ref: DVec3, t: f64) -> Chain {
    let rig = m.rigging.as_ref().expect("checked by caller");
    let (pivot, next) = (v3(rig.pivot), v3(rig.next_pivot));
    let n = r.count();
    let mut centres = vec![DVec3::ZERO; n];
    let mut tilts = vec![0.0; n];
    let flown = frame.is_none_or(|f| f.flown);
    let attach = frame.map_or(DVec3::ZERO, |f| f_ref + tilt(t) * v3(f.attach));
    if flown {
        // Top box's pivot on the frame; each next box hangs from the one above.
        let mut a = t + r.frame_splay_deg;
        let mut hinge = attach;
        for k in 0..n {
            if k > 0 {
                a += r.splays_deg[k - 1];
            }
            tilts[k] = a;
            centres[k] = hinge - tilt(a) * pivot;
            hinge = centres[k] + tilt(a) * next;
        }
    } else {
        // Ground stack: the bottom box rests on the frame, the others stand on it.
        let mut a = t + r.frame_splay_deg;
        let mut hinge = attach;
        for k in (0..n).rev() {
            if k + 1 < n {
                a -= r.splays_deg[k];
            }
            tilts[k] = a;
            centres[k] = hinge - tilt(a) * next;
            hinge = centres[k] + tilt(a) * pivot;
        }
    }
    let box_com = rig.center_of_mass.map_or(DVec3::ZERO, v3);
    let mut weight = 0.0;
    let mut moment = DVec3::ZERO;
    for k in 0..n {
        let w = rig.weight_kg.max(1e-3);
        weight += w;
        moment += (centres[k] + tilt(tilts[k]) * box_com) * w;
    }
    if let Some(f) = frame {
        let w = f.weight_kg.max(0.0);
        weight += w;
        moment += (f_ref + tilt(t) * f.center_of_mass.map_or(DVec3::ZERO, v3)) * w;
    }
    Chain { centres, tilts, com: moment / weight.max(1e-9), weight }
}

fn line_array(a: &Assembly, r: &LineArrayRecipe, models: &Models) -> Result<Layout, String> {
    let m = model(models, &r.model_id)?;
    let rig = m.rigging.as_ref().ok_or_else(|| format!("{} has no rigging data (import it from a GLL file)", m.name))?;
    if r.count() == 0 {
        return Err("a line array needs at least one box".into());
    }
    if r.splays_deg.len() + 1 != r.count() {
        return Err("one splay per pair of boxes".into());
    }
    let frame = match &r.frame {
        Some(k) => Some(rig.frames.iter().find(|f| &f.key == k).ok_or_else(|| format!("{} has no frame `{k}`", m.name))?),
        None => None,
    };
    let mut warnings = vec![];
    // Frame reference and tilt in the yaw-free frame (origin at 0).
    let (f_ref, t) = match (r.tilt, frame) {
        (ArrayTilt::Manual { deg }, _) => (DVec3::ZERO, deg),
        (ArrayTilt::Pick { index }, Some(f)) if f.flown => {
            let pick = f.pick_points.get(index).ok_or_else(|| format!("frame `{}` has no pick point {}", f.label, index + 1))?;
            let p = v3(pick.position);
            // Tilt where the centre of mass hangs straight below the pick
            // point (bisection on its horizontal offset).
            let offset = |t: f64| {
                let f_ref = -(tilt(t) * p);
                chain(r, m, frame, f_ref, t).com.x
            };
            let (mut lo, mut hi) = (-80.0, 80.0);
            let (flo, fhi) = (offset(lo), offset(hi));
            if flo.signum() == fhi.signum() {
                warnings.push("the array cannot balance on this pick point".into());
            }
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if offset(mid).signum() == flo.signum() {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let t = 0.5 * (lo + hi);
            (-(tilt(t) * p), t)
        }
        (ArrayTilt::Pick { .. }, _) => return Err("a pick point needs a flown frame".into()),
    };
    let c = chain(r, m, frame, f_ref, t);
    // Limits.
    if let Some(f) = frame {
        if let Some(max) = f.max_boxes {
            if r.count() > max as usize {
                warnings.push(format!("{} boxes exceed the {} limit of {max}", r.count(), f.label));
            }
        }
        if !allowed(&f.splays_deg, r.frame_splay_deg) {
            warnings.push(format!("frame angle {:.1}° is not an available setting", r.frame_splay_deg));
        }
    }
    for (k, s) in r.splays_deg.iter().enumerate() {
        if !allowed(&rig.splays_deg, *s) {
            warnings.push(format!("splay {:.1}° between boxes {} and {} is not an available setting", s, k + 1, k + 2));
        }
    }
    let yaw = DMat3::from_rotation_z(a.yaw_deg.to_radians());
    let elements = (0..r.count())
        .map(|k| ElementPose {
            model_id: r.model_id.clone(),
            position: a.origin + yaw * c.centres[k],
            yaw_deg: a.yaw_deg,
            pitch_deg: -c.tilts[k],
            roll_deg: 0.0,
            settings: r.elements[k].clone(),
        })
        .collect();
    let frame = frame.map(|f| FramePose { key: f.key.clone(), reference: a.origin + yaw * f_ref, rotation: yaw * tilt(t) });
    Ok(Layout { elements, frame, frame_tilt_deg: t, box_tilt_deg: c.tilts, weight_kg: c.weight, warnings })
}

fn allowed(set: &[f64], v: f64) -> bool {
    set.is_empty() || set.iter().any(|s| (s - v).abs() < 1e-6)
}

/// Nearest allowed value (any value if the set is empty).
pub fn snap(set: &[f64], v: f64) -> f64 {
    set.iter().copied().min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs())).unwrap_or(v)
}

/// Sets the splays (and the frame tilt or pick point) so the boxes aim at
/// the audience line, from the far end (top box) to the first row (bottom
/// box), each box covering an equal stretch of the audience (which gives
/// the usual J: small splays at the top, large at the bottom); splays are
/// snapped to the module's available settings.
pub fn auto_splay(a: &mut Assembly, models: &Models) -> Result<(), String> {
    let AssemblyKind::LineArray(r) = &a.kind else { return Err("auto-splay is for line arrays".into()) };
    let target = r.auto.ok_or("set the audience (first and last listener) first")?;
    if !(target.first_m > 0.0 && target.last_m > target.first_m) {
        return Err("the last listener must be farther than the first".into());
    }
    let m = model(models, &r.model_id)?.clone();
    let rig = m.rigging.clone().ok_or("the module has no rigging data")?;
    let n = r.count();
    let fwd = DMat3::from_rotation_z(a.yaw_deg.to_radians()) * DVec3::X;
    let aim = |k: usize| {
        let s = if n > 1 { k as f64 / (n - 1) as f64 } else { 0.5 };
        let d = target.last_m + (target.first_m - target.last_m) * s;
        (d, target.listener_z + (d - target.first_m) * target.rake_deg.to_radians().tan())
    };
    // Downward angle from an acoustic centre to an aim point.
    let want = |q: DVec3, origin: DVec3, k: usize| {
        let (d, z) = aim(k);
        let dist = (q - origin).dot(fwd);
        ((q.z - z).atan2(d - dist)).to_degrees()
    };
    let centre = v3(m.acoustic_center_m);
    let flown = {
        let AssemblyKind::LineArray(r) = &a.kind else { unreachable!() };
        r.frame.as_ref().and_then(|k| rig.frames.iter().find(|f| &f.key == k)).is_none_or(|f| f.flown)
    };
    for _ in 0..6 {
        let lay = layout(a, models)?;
        let AssemblyKind::LineArray(r) = &mut a.kind else { unreachable!() };
        let origin = a.origin;
        let q: Vec<DVec3> = lay.elements.iter().map(|e| e.position + e.rotation() * centre).collect();
        // Top box: the frame tilt (manual) or the best pick point.
        let top = want(q[0], origin, 0);
        match r.tilt {
            ArrayTilt::Manual { deg } => {
                let frame_splays = r.frame.as_ref().and_then(|k| rig.frames.iter().find(|f| &f.key == k)).map(|f| f.splays_deg.clone()).unwrap_or_default();
                let delta = top - lay.box_tilt_deg[0];
                if flown {
                    r.tilt = ArrayTilt::Manual { deg: deg + delta };
                } else {
                    // Ground stacks tilt by the frame angle only.
                    r.frame_splay_deg = snap(&frame_splays, r.frame_splay_deg + delta);
                }
            }
            ArrayTilt::Pick { .. } => {
                let frame = r.frame.as_ref().and_then(|k| rig.frames.iter().find(|f| &f.key == k)).ok_or("no frame")?;
                let mut best = (f64::INFINITY, 0);
                for i in 0..frame.pick_points.len() {
                    let mut trial = a.clone();
                    if let AssemblyKind::LineArray(tr) = &mut trial.kind {
                        tr.tilt = ArrayTilt::Pick { index: i };
                    }
                    if let Ok(l) = layout(&trial, models) {
                        let err = (l.box_tilt_deg[0] - top).abs();
                        if err < best.0 {
                            best = (err, i);
                        }
                    }
                }
                let AssemblyKind::LineArray(r) = &mut a.kind else { unreachable!() };
                r.tilt = ArrayTilt::Pick { index: best.1 };
            }
        }
        // Box by box: the splay that aims each box at its point.
        let lay = layout(a, models)?;
        let AssemblyKind::LineArray(r) = &mut a.kind else { unreachable!() };
        let mut prev = lay.box_tilt_deg[0];
        for k in 1..n {
            let s = snap(&rig.splays_deg, want(q[k], origin, k) - prev);
            r.splays_deg[k - 1] = s;
            prev += s;
        }
    }
    Ok(())
}

impl StackPreset {
    /// The elements this preset describes; delays from the speed of sound `c`.
    pub fn generate(&self, models: &Models, c: f64) -> Result<Vec<StackElement>, String> {
        let el = |model_id: &str, offset: DVec3, yaw: f64, settings: ElementSettings| StackElement {
            model_id: model_id.into(),
            offset,
            yaw_deg: yaw,
            pitch_deg: 0.0,
            roll_deg: 0.0,
            settings,
        };
        let size = |id: &str| -> Result<(f64, f64, f64), String> {
            let m = model(models, id)?;
            let (lo, hi) = m.outline().iter().fold((DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
            let d = hi - lo;
            Ok((d.x, d.y, d.z)) // depth, width, height
        };
        Ok(match self {
            StackPreset::Custom => vec![],
            StackPreset::Column { model_id, count, columns } => {
                let (_, w, h) = size(model_id)?;
                let cols = (*columns).max(1);
                let mut out = vec![];
                for c in 0..cols {
                    let y = (c as f64 - (cols - 1) as f64 / 2.0) * w;
                    for k in 0..(*count).max(1) {
                        out.push(el(model_id, DVec3::new(0.0, y, h * (k as f64 + 0.5)), 0.0, ElementSettings::default()));
                    }
                }
                out
            }
            StackPreset::Cardioid { model_id, forward, reversed } => {
                let (depth, _, h) = size(model_id)?;
                let mut out = vec![];
                for k in 0..(forward + reversed) {
                    let rev = k >= *forward;
                    let z = h * (k as f64 + 0.5);
                    let settings = if rev {
                        ElementSettings { polarity_inverted: true, delay_ms: depth / c * 1e3, ..Default::default() }
                    } else {
                        ElementSettings::default()
                    };
                    out.push(el(model_id, DVec3::new(0.0, 0.0, z), if rev { 180.0 } else { 0.0 }, settings));
                }
                out
            }
            StackPreset::Endfire { model_id, count, spacing_m } => {
                let (_, _, h) = size(model_id)?;
                let n = (*count).max(1) as usize;
                (0..n)
                    .map(|i| {
                        let x = (i as f64 - (n - 1) as f64 / 2.0) * spacing_m;
                        let delay = i as f64 * spacing_m / c * 1e3;
                        el(model_id, DVec3::new(x, 0.0, h / 2.0), 0.0, ElementSettings { delay_ms: delay, ..Default::default() })
                    })
                    .collect()
            }
            StackPreset::Gradient { model_id, spacing_m } => {
                let (_, _, h) = size(model_id)?;
                vec![
                    el(model_id, DVec3::new(spacing_m / 2.0, 0.0, h / 2.0), 0.0, ElementSettings::default()),
                    el(
                        model_id,
                        DVec3::new(-spacing_m / 2.0, 0.0, h / 2.0),
                        0.0,
                        ElementSettings { polarity_inverted: true, delay_ms: spacing_m / c * 1e3, ..Default::default() },
                    ),
                ]
            }
            StackPreset::Broadside { model_id, count, spacing_m, arc_deg } => {
                let (_, _, h) = size(model_id)?;
                let n = (*count).max(1) as usize;
                let half = (n - 1) as f64 / 2.0 * spacing_m;
                // Delays that make a straight row radiate like an arc: the
                // inner boxes wait for the outer ones' extra path.
                let radius = if *arc_deg > 0.1 && half > 0.0 { half / (arc_deg.to_radians() / 2.0).sin() } else { f64::INFINITY };
                (0..n)
                    .map(|i| {
                        let y = (i as f64 - (n - 1) as f64 / 2.0) * spacing_m;
                        let delay = if radius.is_finite() { ((radius * radius - y * y).sqrt() - (radius * radius - half * half).sqrt()) / c * 1e3 } else { 0.0 };
                        el(model_id, DVec3::new(0.0, y, h / 2.0), 0.0, ElementSettings { delay_ms: delay, ..Default::default() })
                    })
                    .collect()
            }
        })
    }
}

// ---- project operations ----

/// Clipboard contents: groups (with their recipes) and loose speakers,
/// positions relative to `anchor`, and the models they use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub format: String,
    pub anchor: DVec3,
    pub groups: Vec<ClipGroup>,
    pub speakers: Vec<SpeakerInstance>,
    pub models: Vec<SpeakerModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipGroup {
    pub name: String,
    pub linked: bool,
    pub assembly: Option<Assembly>,
    pub members: Vec<SpeakerInstance>,
    #[serde(default)]
    pub receivers: Vec<wavey_solver::Microphone>,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub delay_ms: f64,
    #[serde(default)]
    pub polarity_inverted: bool,
}

const CLIP_FORMAT: &str = "wavey-clip-1";

impl Project {
    fn new_speaker(&mut self, model_id: &str) -> Result<u64, String> {
        self.add_speaker(model_id, DVec3::ZERO)
    }

    /// Writes an assembly's layout into its group's members: existing
    /// members in order are reused, missing ones added, extra ones removed.
    fn apply_assembly(&mut self, group: u64) -> Result<(), String> {
        let g = self.meta.groups.get(&group).ok_or("no such group")?;
        let a = g.assembly.clone().ok_or("the group is not a stack or array")?;
        let lay = layout(&a, &self.speaker_models)?;
        let name = g.name.clone();
        let mut members = g.members.clone();
        members.retain(|m| self.speakers.contains_key(m));
        for (k, pose) in lay.elements.iter().enumerate() {
            let (id, created) = match members.get(k) {
                Some(id) => (*id, false),
                None => {
                    let id = self.new_speaker(&pose.model_id)?;
                    members.push(id);
                    (id, true)
                }
            };
            let s = self.speakers.get_mut(&id).unwrap();
            // Members keep names the user gave them.
            let keep_name = !created && !s.name.is_empty() && !s.name.starts_with(&format!("{name} · "));
            s.model_id = pose.model_id.clone();
            s.position = pose.position;
            s.yaw_deg = pose.yaw_deg;
            s.pitch_deg = pose.pitch_deg;
            s.roll_deg = pose.roll_deg;
            s.gain_db = pose.settings.gain_db;
            s.delay_ms = pose.settings.delay_ms;
            s.polarity_inverted = pose.settings.polarity_inverted;
            s.muted = pose.settings.muted;
            s.filters = pose.settings.filters.clone();
            s.on_floor = false;
            if !keep_name {
                s.name = format!("{name} · {}", k + 1);
            }
            // An assembly owns its members' aim.
            if let Some(meta) = self.meta.sources.get_mut(&id) {
                meta.aim = None;
            }
        }
        for extra in members.split_off(lay.elements.len()) {
            self.remove_speaker(extra);
        }
        self.meta.groups.get_mut(&group).unwrap().members = members;
        Ok(())
    }

    /// Inserts an assembly as a new group of speakers.
    pub fn insert_assembly(&mut self, name: &str, a: Assembly) -> Result<u64, String> {
        layout(&a, &self.speaker_models)?;
        let id = self.add_group(name, vec![]);
        self.meta.groups.get_mut(&id).unwrap().assembly = Some(a);
        if let Err(e) = self.apply_assembly(id) {
            self.meta.forget(&format!("group:{id}"));
            return Err(e);
        }
        Ok(id)
    }

    /// Replaces a group's recipe and re-lays out its members.
    pub fn update_assembly(&mut self, group: u64, a: Assembly) -> Result<(), String> {
        layout(&a, &self.speaker_models)?;
        let g = self.meta.groups.get_mut(&group).ok_or("no such group")?;
        g.assembly = Some(a);
        self.apply_assembly(group)
    }

    /// Moves and turns a stack or array as a whole.
    pub fn transform_assembly(&mut self, group: u64, origin: DVec3, yaw_deg: f64) -> Result<(), String> {
        let g = self.meta.groups.get_mut(&group).ok_or("no such group")?;
        let a = g.assembly.as_mut().ok_or("the group is not a stack or array")?;
        a.origin = origin;
        a.yaw_deg = yaw_deg;
        self.apply_assembly(group)
    }

    /// Drops the recipe; the speakers stay (as a plain group).
    pub fn detach_assembly(&mut self, group: u64) -> Result<(), String> {
        self.meta.groups.get_mut(&group).ok_or("no such group")?.assembly = None;
        Ok(())
    }

    /// Whether the members still sit where the recipe puts them.
    pub fn assembly_in_sync(&self, group: u64) -> bool {
        let Some(g) = self.meta.groups.get(&group) else { return false };
        let Some(a) = &g.assembly else { return true };
        let Ok(lay) = layout(a, &self.speaker_models) else { return false };
        lay.elements.len() == g.members.len()
            && lay.elements.iter().zip(&g.members).all(|(p, id)| {
                self.speakers.get(id).is_some_and(|s| {
                    (s.position - p.position).length() < 1e-3 && (s.yaw_deg - p.yaw_deg).abs() < 0.01 && (s.pitch_deg - p.pitch_deg).abs() < 0.01
                })
            })
    }

    /// After a member's settings changed: carries gain, delay, polarity,
    /// mute and filters into its group's recipe.
    pub fn sync_assembly_settings(&mut self, speaker: u64) {
        let Some(s) = self.speakers.get(&speaker).cloned() else { return };
        for g in self.meta.groups.values_mut() {
            let Some(k) = g.members.iter().position(|m| *m == speaker) else { continue };
            let settings = ElementSettings {
                gain_db: s.gain_db,
                delay_ms: s.delay_ms,
                polarity_inverted: s.polarity_inverted,
                muted: s.muted,
                filters: s.filters.clone(),
            };
            match g.assembly.as_mut().map(|a| &mut a.kind) {
                Some(AssemblyKind::Stack(r)) => {
                    if let Some(e) = r.elements.get_mut(k) {
                        e.settings = settings;
                    }
                }
                Some(AssemblyKind::LineArray(r)) => {
                    if let Some(e) = r.elements.get_mut(k) {
                        *e = settings;
                    }
                }
                None => {}
            }
        }
    }

    /// Copies groups (with recipes) and loose speakers for pasting, here or
    /// in another project.
    pub fn clip(&self, groups: &[u64], speakers: &[u64]) -> Result<String, String> {
        let mut used = std::collections::BTreeSet::new();
        let mut clip = Clip { format: CLIP_FORMAT.into(), anchor: DVec3::ZERO, groups: vec![], speakers: vec![], models: vec![] };
        let mut pts = vec![];
        for gid in groups {
            let g = self.meta.groups.get(gid).ok_or("no such group")?;
            let members: Vec<SpeakerInstance> = g.members.iter().filter_map(|m| self.speakers.get(m).cloned()).collect();
            members.iter().for_each(|m| {
                used.insert(m.model_id.clone());
                pts.push(m.position);
            });
            let receivers: Vec<wavey_solver::Microphone> = g.receivers.iter().filter_map(|m| self.microphones.get(m).cloned()).collect();
            pts.extend(receivers.iter().map(|m| m.position));
            clip.groups.push(ClipGroup {
                name: g.name.clone(),
                linked: g.linked,
                assembly: g.assembly.clone(),
                members,
                receivers,
                filters: g.filters.clone(),
                gain_db: g.gain_db,
                delay_ms: g.delay_ms,
                polarity_inverted: g.polarity_inverted,
            });
        }
        for id in speakers {
            if clip.groups.iter().any(|g| g.members.iter().any(|m| m.id == *id)) {
                continue;
            }
            let s = self.speakers.get(id).ok_or("no such speaker")?.clone();
            used.insert(s.model_id.clone());
            pts.push(s.position);
            clip.speakers.push(s);
        }
        if pts.is_empty() {
            return Err("nothing to copy".into());
        }
        clip.anchor = pts.iter().copied().sum::<DVec3>() / pts.len() as f64;
        clip.models = used.iter().filter_map(|id| self.speaker_models.get(id).map(|m| (**m).clone())).collect();
        serde_json::to_string(&clip).map_err(|e| e.to_string())
    }

    /// Pastes a clip moved by `offset` (relative to where it was copied).
    /// Returns the new groups and loose speakers.
    pub fn paste_clip(&mut self, json: &str, offset: DVec3) -> Result<(Vec<u64>, Vec<u64>), String> {
        let clip: Clip = serde_json::from_str(json).map_err(|e| format!("not a Wavey clipboard: {e}"))?;
        if clip.format != CLIP_FORMAT {
            return Err("unsupported clipboard format".into());
        }
        // Models: reuse identical ones, add the others (renamed on clashes).
        let mut rename = BTreeMap::new();
        for m in clip.models {
            let old = m.id.clone();
            let new = self.add_speaker_model(m)?;
            rename.insert(old, new);
        }
        let model_of = |id: &str| rename.get(id).cloned().unwrap_or_else(|| id.to_string());
        let mut new_groups = vec![];
        let copy_speaker = |p: &mut Project, s: &SpeakerInstance| -> Result<u64, String> {
            let id = p.new_speaker(&model_of(&s.model_id))?;
            let mut c = s.clone();
            c.id = id;
            c.model_id = model_of(&s.model_id);
            c.position += offset;
            p.speakers.insert(id, c);
            Ok(id)
        };
        for g in &clip.groups {
            let mut members = vec![];
            for s in &g.members {
                members.push(copy_speaker(self, s)?);
            }
            let mut mics = vec![];
            for m in &g.receivers {
                let id = self.add_microphone(m.position + offset);
                if let Some(new) = self.microphones.get_mut(&id) {
                    new.name = m.name.clone();
                }
                mics.push(id);
            }
            let gid = self.add_group(&g.name, members);
            let ng = self.meta.groups.get_mut(&gid).unwrap();
            ng.linked = g.linked;
            ng.receivers = mics;
            ng.filters = g.filters.clone();
            (ng.gain_db, ng.delay_ms, ng.polarity_inverted) = (g.gain_db, g.delay_ms, g.polarity_inverted);
            if let Some(mut a) = g.assembly.clone() {
                a.origin += offset;
                match &mut a.kind {
                    AssemblyKind::Stack(r) => r.elements.iter_mut().for_each(|e| e.model_id = model_of(&e.model_id)),
                    AssemblyKind::LineArray(r) => r.model_id = model_of(&r.model_id),
                }
                ng.assembly = Some(a);
            }
            new_groups.push(gid);
        }
        let mut loose = vec![];
        for s in &clip.speakers {
            loose.push(copy_speaker(self, s)?);
        }
        Ok((new_groups, loose))
    }

    /// Duplicates a group (and its recipe), moved by `offset`.
    pub fn duplicate_group(&mut self, group: u64, offset: DVec3) -> Result<u64, String> {
        let json = self.clip(&[group], &[])?;
        let (groups, _) = self.paste_clip(&json, offset)?;
        let id = groups[0];
        let g = self.meta.groups.get_mut(&id).unwrap();
        g.name = format!("{} copy", g.name);
        Ok(id)
    }
}

// ---- quick free-field prediction ----

/// Speakers of a layout as solver inputs.
pub fn layout_instances(lay: &Layout) -> Vec<SpeakerInstance> {
    lay.elements
        .iter()
        .enumerate()
        .map(|(k, e)| SpeakerInstance {
            id: k as u64 + 1,
            name: format!("{}", k + 1),
            model_id: e.model_id.clone(),
            position: e.position,
            yaw_deg: e.yaw_deg,
            pitch_deg: e.pitch_deg,
            roll_deg: e.roll_deg,
            gain_db: e.settings.gain_db,
            delay_ms: e.settings.delay_ms,
            polarity_inverted: e.settings.polarity_inverted,
            muted: e.settings.muted,
            on_floor: false,
            bottom_height_m: 0.0,
            filters: e.settings.filters.clone(),
        })
        .collect()
}

/// Plane of a polar prediction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolarPlane {
    /// Around the vertical axis; 0° straight ahead, positive to the left.
    Horizontal,
    /// In the vertical plane of the aim direction; 0° ahead, 90° up, 180° behind.
    Vertical,
}

/// Free-field SPL (dB, `width_oct`-band energy average) on a circle of
/// `radius` m around the assembly's acoustic centroid, per frequency, at
/// angles −180…180° in `step_deg` steps.
pub fn predict_polar(
    a: &Assembly,
    models: &Models,
    medium: &wavey_materials::Medium,
    freqs: &[f64],
    plane: PolarPlane,
    radius: f64,
    step_deg: f64,
    width_oct: f64,
) -> Result<(Vec<f64>, Vec<Vec<f64>>), String> {
    let lay = layout(a, models)?;
    let inst = layout_instances(&lay);
    let srcs: Vec<(&SpeakerModel, &SpeakerInstance)> = inst.iter().map(|s| (models[&s.model_id].as_ref(), s)).collect();
    if srcs.is_empty() {
        return Ok((vec![], vec![]));
    }
    let centre = srcs.iter().map(|(m, s)| s.acoustic_center(m)).sum::<DVec3>() / srcs.len() as f64;
    let yaw = a.yaw_deg.to_radians();
    let (fwd, left) = (DVec3::new(yaw.cos(), yaw.sin(), 0.0), DVec3::new(-yaw.sin(), yaw.cos(), 0.0));
    let n = (360.0 / step_deg.max(0.5)).round() as usize;
    let angles: Vec<f64> = (0..=n).map(|i| -180.0 + i as f64 * 360.0 / n as f64).collect();
    let values = freqs
        .iter()
        .map(|f| {
            angles
                .iter()
                .map(|deg| {
                    let t = deg.to_radians();
                    let dir = match plane {
                        PolarPlane::Horizontal => fwd * t.cos() + left * t.sin(),
                        PolarPlane::Vertical => fwd * t.cos() + DVec3::Z * t.sin(),
                    };
                    wavey_solver::freefield::band_spl(&srcs, medium, centre + dir * radius, *f, width_oct, if width_oct > 0.0 { 7 } else { 1 })
                })
                .collect()
        })
        .collect();
    Ok((angles, values))
}

/// Free-field SPL along the audience line of a line array (or ahead of a
/// stack): horizontal distances from the origin along the aim direction,
/// at height `z0` rising by `rake_deg`, per frequency band.
#[allow(clippy::too_many_arguments)]
pub fn predict_distance(
    a: &Assembly,
    models: &Models,
    medium: &wavey_materials::Medium,
    freqs: &[f64],
    from_m: f64,
    to_m: f64,
    z0: f64,
    rake_deg: f64,
    points: usize,
    width_oct: f64,
) -> Result<(Vec<f64>, Vec<Vec<f64>>), String> {
    let lay = layout(a, models)?;
    let inst = layout_instances(&lay);
    let srcs: Vec<(&SpeakerModel, &SpeakerInstance)> = inst.iter().map(|s| (models[&s.model_id].as_ref(), s)).collect();
    let yaw = a.yaw_deg.to_radians();
    let fwd = DVec3::new(yaw.cos(), yaw.sin(), 0.0);
    let n = points.max(2);
    let dists: Vec<f64> = (0..n).map(|i| from_m + (to_m - from_m) * i as f64 / (n - 1) as f64).collect();
    let values = freqs
        .iter()
        .map(|f| {
            dists
                .iter()
                .map(|d| {
                    let mut x = a.origin + fwd * *d;
                    x.z = z0 + (d - from_m) * rake_deg.to_radians().tan();
                    wavey_solver::freefield::band_spl(&srcs, medium, x, *f, width_oct, if width_oct > 0.0 { 7 } else { 1 })
                })
                .collect()
        })
        .collect();
    Ok((dists, values))
}

/// Rear rejection of a stack: tunes the delay and gain of its
/// polarity-inverted (rear-cancelling) elements, together, for the lowest
/// rear-to-front energy ratio over `f_lo`…`f_hi` at 20 m (free field). The
/// other elements keep their settings. Returns the rear − front level (dB).
pub fn tune_rear(a: &mut Assembly, models: &Models, medium: &wavey_materials::Medium, f_lo: f64, f_hi: f64) -> Result<f64, String> {
    let AssemblyKind::Stack(r) = &a.kind else { return Err("rear tuning is for stacks".into()) };
    let rear: Vec<usize> = r.elements.iter().enumerate().filter(|(_, e)| e.settings.polarity_inverted).map(|(i, _)| i).collect();
    if rear.is_empty() {
        return Err("no polarity-inverted (rear-cancelling) elements to tune".into());
    }
    let base: Vec<(f64, f64)> = rear.iter().map(|&i| (r.elements[i].settings.delay_ms, r.elements[i].settings.gain_db)).collect();
    let n = 9;
    let freqs: Vec<f64> = (0..n).map(|i| f_lo * (f_hi / f_lo).powf(i as f64 / (n - 1) as f64)).collect();
    let lay0 = layout(a, models)?;
    let inst0 = layout_instances(&lay0);
    let yaw = a.yaw_deg.to_radians();
    let fwd = DVec3::new(yaw.cos(), yaw.sin(), 0.0);
    let centre = inst0.iter().map(|s| s.acoustic_center(&models[&s.model_id])).sum::<DVec3>() / inst0.len() as f64;
    let (front, back) = (centre + fwd * 20.0, centre - fwd * 20.0);
    // Pressures per element at the two points (unit processing), so the
    // search only recombines them.
    let unit = |s: &SpeakerInstance| {
        let mut u = s.clone();
        u.gain_db = 0.0;
        u.delay_ms = 0.0;
        u.polarity_inverted = false;
        u
    };
    let p = |x: DVec3, k: usize, f: f64| {
        let s = unit(&inst0[k]);
        wavey_solver::freefield::pressure(&[(models[&s.model_id].as_ref(), &s)], medium, x, f)
    };
    let pf: Vec<Vec<num_complex::Complex64>> = (0..inst0.len()).map(|k| freqs.iter().map(|f| p(front, k, *f)).collect()).collect();
    let pb: Vec<Vec<num_complex::Complex64>> = (0..inst0.len()).map(|k| freqs.iter().map(|f| p(back, k, *f)).collect()).collect();
    let ratio = |dd: f64, dg: f64| {
        let (mut ef, mut eb) = (0.0, 0.0);
        for (fi, f) in freqs.iter().enumerate() {
            let (mut sf, mut sb) = (num_complex::Complex64::new(0.0, 0.0), num_complex::Complex64::new(0.0, 0.0));
            for (k, s) in inst0.iter().enumerate() {
                let (delay, gain) = match rear.iter().position(|&i| i == k) {
                    Some(j) => (base[j].0 + dd, base[j].1 + dg),
                    None => (s.delay_ms, s.gain_db),
                };
                let mut d = s.clone();
                d.delay_ms = delay.max(0.0);
                d.gain_db = gain;
                let h = d.processing(*f);
                sf += pf[k][fi] * h;
                sb += pb[k][fi] * h;
            }
            ef += sf.norm_sqr();
            eb += sb.norm_sqr();
        }
        10.0 * (eb / ef.max(1e-30)).log10()
    };
    // Coarse grid, then refine around the best.
    let mut best = (0.0, 0.0, ratio(0.0, 0.0));
    let (mut dd_step, mut dg_step) = (0.25, 0.5);
    let (mut dd_lo, mut dd_hi, mut dg_lo, mut dg_hi) = (-base.iter().map(|b| b.0).fold(f64::INFINITY, f64::min), 15.0, -15.0, 6.0);
    for _ in 0..3 {
        let mut dd = dd_lo;
        while dd <= dd_hi + 1e-9 {
            let mut dg = dg_lo;
            while dg <= dg_hi + 1e-9 {
                let v = ratio(dd, dg);
                if v < best.2 {
                    best = (dd, dg, v);
                }
                dg += dg_step;
            }
            dd += dd_step;
        }
        (dd_lo, dd_hi) = (best.0 - 2.0 * dd_step, best.0 + 2.0 * dd_step);
        (dg_lo, dg_hi) = (best.1 - 2.0 * dg_step, best.1 + 2.0 * dg_step);
        dd_step /= 5.0;
        dg_step /= 5.0;
    }
    let AssemblyKind::Stack(r) = &mut a.kind else { unreachable!() };
    for (j, &i) in rear.iter().enumerate() {
        r.elements[i].settings.delay_ms = ((base[j].0 + best.0).max(0.0) * 100.0).round() / 100.0;
        r.elements[i].settings.gain_db = ((base[j].1 + best.1) * 10.0).round() / 10.0;
    }
    Ok(best.2)
}
