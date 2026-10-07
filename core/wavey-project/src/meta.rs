//! Organisational data the app edits alongside the acoustic model: layers,
//! levels, groups, display codes and per-entity display settings.
//!
//! None of it changes simulation results except `MicMeta::include_in_solve`
//! and source solo (a mix-time option), so it stays out of the fingerprint.

use std::collections::BTreeMap;

use glam::DVec3;
use serde::{Deserialize, Serialize};
use wavey_geom::SegmentId;

pub const LAYER_WALLS: &str = "walls";
pub const LAYER_FLOOR: &str = "floor";
pub const LAYER_CEILING: &str = "ceiling";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: String,
    pub name: String,
    pub color: String,
    pub visible: bool,
    pub locked: bool,
}

/// A named horizontal datum. The floor (z 0) and ceiling (the room height)
/// levels are implied; these are additional reference levels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Level {
    pub id: String,
    pub name: String,
    pub z: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: u64,
    pub name: String,
    /// Speaker ids.
    pub members: Vec<u64>,
    /// Microphone ids (groups of receivers, e.g. a microphone line).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub receivers: Vec<u64>,
    /// Group EQ: applied to every member speaker on top of its own filters
    /// (when mixing, so it needs no re-solve).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<wavey_speaker::Filter>,
    /// Group gain, delay and polarity: like the EQ, applied on top of each
    /// member's own settings when mixing (the members keep their values).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gain_db: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub delay_ms: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub polarity_inverted: bool,
    /// Property edits on one member apply to all.
    #[serde(default)]
    pub linked: bool,
    /// Stack or line-array recipe the members are laid out from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly: Option<crate::assembly::Assembly>,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl Group {
    /// The group's processing at `f`: EQ, gain, delay and polarity.
    pub fn response(&self, f: f64) -> num_complex::Complex64 {
        let g = 10f64.powf(self.gain_db / 20.0) * if self.polarity_inverted { -1.0 } else { 1.0 };
        let delay = num_complex::Complex64::from_polar(g, -2.0 * std::f64::consts::PI * f * self.delay_ms / 1000.0);
        self.filters.iter().map(|q| q.response(f)).product::<num_complex::Complex64>() * delay
    }
}

/// What a source keeps pointing at; its yaw and pitch follow when either end moves.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AimTarget {
    Microphone { id: u64 },
    Point { p: DVec3 },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceMeta {
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub aim: Option<AimTarget>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MicMeta {
    pub color: String,
    pub visible: bool,
    pub include_in_solve: bool,
}

impl Default for MicMeta {
    fn default() -> Self {
        Self { color: String::new(), visible: true, include_in_solve: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RangeMode {
    Auto,
    Fixed,
    Symmetric,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaneStyle {
    pub color_map: String,
    pub range: RangeMode,
    pub range_min: f64,
    pub range_max: f64,
    /// Auto range span (dB).
    pub auto_span: f64,
    pub contour_step: f64,
    pub show_contours: bool,
    pub opacity: f64,
    pub visible: bool,
}

impl Default for PlaneStyle {
    fn default() -> Self {
        Self {
            color_map: "inferno".into(),
            range: RangeMode::Auto,
            range_min: 60.0,
            range_max: 100.0,
            auto_span: 30.0,
            contour_step: 3.0,
            show_contours: false,
            opacity: 0.92,
            visible: true,
        }
    }
}

/// Imported linework to trace over (a DXF floor plan or section), in
/// metres with z up. Display only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Underlay {
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub lines: Vec<[[f64; 3]; 2]>,
}

/// Everything above, stored in the project under `meta`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub layers: Vec<Layer>,
    pub current_layer: String,
    /// Layer of each sketch segment (and so of its walls); missing = walls.
    #[serde(default)]
    pub segment_layers: BTreeMap<SegmentId, String>,
    #[serde(default)]
    pub levels: Vec<Level>,
    #[serde(default)]
    pub groups: BTreeMap<u64, Group>,
    /// User names of room surfaces by surface key.
    #[serde(default)]
    pub surface_names: BTreeMap<String, String>,
    /// Display codes ("W-07", "S1", "M2", "CP-1") by entity key
    /// (`wall:…`, `spk:{id}`, `mic:{id}`, `plane:{id}`, `group:{id}`).
    #[serde(default)]
    pub codes: BTreeMap<String, String>,
    /// Last number handed out per code prefix.
    #[serde(default)]
    pub code_counters: BTreeMap<String, u32>,
    #[serde(default)]
    pub sources: BTreeMap<u64, SourceMeta>,
    #[serde(default)]
    pub mics: BTreeMap<u64, MicMeta>,
    #[serde(default)]
    pub planes: BTreeMap<u64, PlaneStyle>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub underlays: Vec<Underlay>,
}

impl Default for ProjectMeta {
    fn default() -> Self {
        let layer = |id: &str, name: &str, color: &str| Layer {
            id: id.into(),
            name: name.into(),
            color: color.into(),
            visible: true,
            locked: false,
        };
        Self {
            layers: vec![
                layer(LAYER_WALLS, "Walls", "#c9c2b4"),
                layer(LAYER_FLOOR, "Floor", "#8c6a4a"),
                layer(LAYER_CEILING, "Ceiling", "#9aa4ae"),
            ],
            current_layer: LAYER_WALLS.into(),
            segment_layers: BTreeMap::new(),
            levels: vec![],
            groups: BTreeMap::new(),
            surface_names: BTreeMap::new(),
            codes: BTreeMap::new(),
            code_counters: BTreeMap::new(),
            sources: BTreeMap::new(),
            mics: BTreeMap::new(),
            planes: BTreeMap::new(),
            underlays: Vec::new(),
        }
    }
}

/// Code prefix and zero padding per entity kind.
fn code_format(entity_key: &str) -> (&'static str, usize, &'static str) {
    match entity_key.split(':').next().unwrap_or("") {
        "wall" => ("W", 2, "-"),
        "floor" => ("F", 2, "-"),
        "ceiling" => ("C", 2, "-"),
        "spk" => ("S", 0, ""),
        "mic" => ("M", 0, ""),
        "plane" => ("CP", 0, "-"),
        "group" => ("G", 0, ""),
        _ => ("X", 0, "-"),
    }
}

impl ProjectMeta {
    /// The entity's code, assigning the next free one if it has none.
    pub fn ensure_code(&mut self, entity_key: &str) -> String {
        if let Some(c) = self.codes.get(entity_key) {
            return c.clone();
        }
        let (prefix, pad, sep) = code_format(entity_key);
        let n = self.code_counters.entry(prefix.to_string()).or_insert(0);
        *n += 1;
        let code = format!("{prefix}{sep}{:0pad$}", *n, pad = pad);
        self.codes.insert(entity_key.to_string(), code.clone());
        code
    }

    pub fn code(&self, entity_key: &str) -> Option<&str> {
        self.codes.get(entity_key).map(String::as_str)
    }

    pub fn layer(&self, id: &str) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    /// Layer of a wall segment.
    pub fn segment_layer(&self, segment: SegmentId) -> &str {
        self.segment_layers.get(&segment).map(String::as_str).unwrap_or(LAYER_WALLS)
    }

    /// Removes everything attached to an entity that no longer exists.
    pub fn forget(&mut self, entity_key: &str) {
        self.codes.remove(entity_key);
        let id = |p: &str| entity_key.strip_prefix(p).and_then(|s| s.parse::<u64>().ok());
        if let Some(id) = id("spk:") {
            self.sources.remove(&id);
            for g in self.groups.values_mut() {
                g.members.retain(|m| *m != id);
            }
        } else if let Some(id) = id("mic:") {
            self.mics.remove(&id);
            for g in self.groups.values_mut() {
                g.receivers.retain(|m| *m != id);
            }
        } else if let Some(id) = id("plane:") {
            self.planes.remove(&id);
        } else if let Some(id) = id("group:") {
            self.groups.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_and_never_reused() {
        let mut m = ProjectMeta::default();
        assert_eq!(m.ensure_code("wall:3:f"), "W-01");
        assert_eq!(m.ensure_code("wall:5:f"), "W-02");
        assert_eq!(m.ensure_code("wall:3:f"), "W-01");
        assert_eq!(m.ensure_code("spk:9"), "S1");
        assert_eq!(m.ensure_code("plane:2"), "CP-1");
        m.forget("spk:9");
        assert_eq!(m.ensure_code("spk:10"), "S2");
    }
}
