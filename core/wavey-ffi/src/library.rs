//! The stock speaker library: a folder of `.waveyspk` files the app lists
//! (grouped by manufacturer) and adds to projects on use.

use std::path::PathBuf;

use wavey_speaker::SpeakerModel;
use wavey_speaker::library::{LibraryEntry, scan};

use crate::{WaveyDocument, WaveyError};

#[derive(Debug, Clone, uniffi::Record)]
pub struct LibraryEntryRec {
    pub path: String,
    pub id: String,
    pub name: String,
    /// Empty when the file names none.
    pub manufacturer: String,
    pub min_hz: f64,
    pub max_hz: f64,
    pub nominal_min_hz: Option<f64>,
    pub nominal_max_hz: Option<f64>,
    pub has_balloon: bool,
    /// Line-array rigging (pivots, frames, splays).
    pub has_rigging: bool,
    pub width_m: f64,
    pub height_m: f64,
    pub depth_m: f64,
    pub file_bytes: u64,
}

impl From<LibraryEntry> for LibraryEntryRec {
    fn from(e: LibraryEntry) -> Self {
        Self {
            path: e.path.to_string_lossy().to_string(),
            id: e.id,
            name: e.name,
            manufacturer: e.manufacturer.unwrap_or_default(),
            min_hz: e.min_hz,
            max_hz: e.max_hz,
            nominal_min_hz: e.nominal_band_hz.map(|b| b[0]),
            nominal_max_hz: e.nominal_band_hz.map(|b| b[1]),
            has_balloon: e.has_balloon,
            has_rigging: e.has_rigging,
            width_m: e.enclosure.width_m,
            height_m: e.enclosure.height_m,
            depth_m: e.enclosure.depth_m,
            file_bytes: e.size,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct LibraryScanRec {
    pub entries: Vec<LibraryEntryRec>,
    /// Files that could not be read.
    pub errors: Vec<String>,
}

/// Every speaker model below `dir`, sorted by manufacturer and name.
#[uniffi::export]
pub fn scan_speaker_library(dir: String) -> LibraryScanRec {
    let (entries, errors) = scan(&PathBuf::from(dir));
    LibraryScanRec {
        entries: entries.into_iter().map(Into::into).collect(),
        errors,
    }
}

#[uniffi::export]
impl WaveyDocument {
    /// The project's model for a library file: the one already embedded
    /// under the same id, or the file added to the project. Returns its id.
    pub fn use_library_model(&self, path: String) -> Result<String, WaveyError> {
        let entry =
            wavey_speaker::library::read_entry(&PathBuf::from(&path)).map_err(WaveyError::new)?;
        if self.lock().project().speaker_models.contains_key(&entry.id) {
            return Ok(entry.id);
        }
        let m = SpeakerModel::load(&PathBuf::from(path)).map_err(WaveyError::new)?;
        self.lock()
            .try_edit(|p| p.add_speaker_model(m))
            .map_err(WaveyError::new)
    }
}

/// Everything the library hover preview shows of a speaker model.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SpeakerPreviewRec {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub width_m: f64,
    pub height_m: f64,
    pub depth_m: f64,
    /// Cabinet outline: pairs of xyz triples (local frame, x forward).
    pub edge_positions: Vec<f32>,
    pub freqs_hz: Vec<f64>,
    pub on_axis_db: Vec<f64>,
    pub sensitivity_1k_db: f64,
    pub max_spl_db: Option<f64>,
    pub nominal_min_hz: Option<f64>,
    pub nominal_max_hz: Option<f64>,
    pub has_balloon: bool,
    /// Polar angles (0…355°) and levels (dB re on-axis) at 1 kHz and 4 kHz.
    pub polar_angles_deg: Vec<f64>,
    pub polar_1k_h: Vec<f64>,
    pub polar_1k_v: Vec<f64>,
    pub polar_4k_h: Vec<f64>,
    pub polar_4k_v: Vec<f64>,
    /// −6 dB full coverage angles (H × V) at 1 kHz and 4 kHz.
    pub coverage_1k: Vec<f64>,
    pub coverage_4k: Vec<f64>,
    /// Line-array rigging summary ("" when none), e.g. "Fly Frame (max 24) · Ground Stack (max 6) · splays 0–10°".
    pub rigging: String,
    pub weight_kg: Option<f64>,
}

/// The cabinet's outer hull edges (manufacturer wireframes include horns
/// and drivers, too busy for a small preview).
fn hull_edges(m: &SpeakerModel) -> Vec<f32> {
    if m.geometry.is_none() {
        return crate::speakers::outline_edges(m);
    }
    let (verts, tris) = m.cabinet_mesh();
    let mut out = Vec::new();
    for [a, b] in wavey_speaker::hull::feature_edges(&verts, &tris, 20.0) {
        for v in [verts[a as usize], verts[b as usize]] {
            out.extend([v.x as f32, v.y as f32, v.z as f32]);
        }
    }
    out
}

fn preview_of(m: &SpeakerModel) -> SpeakerPreviewRec {
    use wavey_geom::DVec3;
    let angles: Vec<f64> = (0..72).map(|i| i as f64 * 5.0).collect();
    let polar = |f: f64, vertical: bool| -> Vec<f64> {
        angles
            .iter()
            .map(|a| {
                let r = a.to_radians();
                let d = if vertical {
                    DVec3::new(r.cos(), 0.0, r.sin())
                } else {
                    DVec3::new(r.cos(), r.sin(), 0.0)
                };
                20.0 * m.directivity(f, d).norm().max(1e-6).log10()
            })
            .collect()
    };
    let coverage = |p: &[f64]| -> f64 {
        // First angle either side of 0° where the level falls below −6 dB.
        let n = p.len();
        let side = |dir: isize| {
            (1..n / 2)
                .find(|&k| p[((k as isize * dir).rem_euclid(n as isize)) as usize] < -6.0)
                .unwrap_or(n / 2) as f64
                * 5.0
        };
        side(1) + side(-1)
    };
    let (h1, v1, h4, v4) = (
        polar(1000.0, false),
        polar(1000.0, true),
        polar(4000.0, false),
        polar(4000.0, true),
    );
    let rigging = m.rigging.as_ref().map_or(String::new(), |r| {
        let frames: Vec<String> = r
            .frames
            .iter()
            .map(|f| {
                format!(
                    "{}{}",
                    f.label,
                    f.max_boxes
                        .map(|n| format!(" (max {n})"))
                        .unwrap_or_default()
                )
            })
            .collect();
        let (lo, hi) = (
            r.splays_deg.first().copied().unwrap_or(0.0),
            r.splays_deg.last().copied().unwrap_or(0.0),
        );
        format!("{} · splays {lo:.0}–{hi:.0}°", frames.join(" · "))
    });
    SpeakerPreviewRec {
        id: m.id.clone(),
        name: m.name.clone(),
        manufacturer: m.manufacturer.clone().unwrap_or_default(),
        width_m: m.enclosure.width_m,
        height_m: m.enclosure.height_m,
        depth_m: m.enclosure.depth_m,
        edge_positions: hull_edges(m),
        freqs_hz: m.frequencies_hz.clone(),
        on_axis_db: m.on_axis.magnitude_db_spl.clone(),
        sensitivity_1k_db: m.sensitivity_db(1000.0),
        max_spl_db: m.max_spl_db,
        nominal_min_hz: m.nominal_band_hz.map(|b| b[0]),
        nominal_max_hz: m.nominal_band_hz.map(|b| b[1]),
        has_balloon: m.balloon.is_some(),
        coverage_1k: vec![coverage(&h1), coverage(&v1)],
        coverage_4k: vec![coverage(&h4), coverage(&v4)],
        polar_angles_deg: angles.clone(),
        polar_1k_h: h1,
        polar_1k_v: v1,
        polar_4k_h: h4,
        polar_4k_v: v4,
        rigging,
        weight_kg: m.rigging.as_ref().map(|r| r.weight_kg).filter(|w| *w > 0.0),
    }
}

/// Preview data of a model file (loads it; the app caches the result).
#[uniffi::export]
pub fn library_preview(path: String) -> Result<SpeakerPreviewRec, WaveyError> {
    let m = SpeakerModel::load(&PathBuf::from(path)).map_err(WaveyError::new)?;
    Ok(preview_of(&m))
}

/// Combined and per-band magnitude (dB) of a filter chain.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FilterCurvesRec {
    pub total_db: Vec<f64>,
    pub bands_db: Vec<Vec<f64>>,
}

/// Magnitude responses of a filter chain at `freqs` (pure; no document).
#[uniffi::export]
pub fn filter_curves(filters: Vec<crate::FilterRec>, freqs: Vec<f64>) -> FilterCurvesRec {
    let fs: Vec<wavey_speaker::Filter> = filters.into_iter().map(Into::into).collect();
    let db = |c: num_complex::Complex64| 20.0 * c.norm().max(1e-9).log10();
    let bands_db: Vec<Vec<f64>> = fs
        .iter()
        .map(|f| freqs.iter().map(|x| db(f.response(*x))).collect())
        .collect();
    let total_db = (0..freqs.len())
        .map(|i| bands_db.iter().map(|b| b[i]).sum())
        .collect();
    FilterCurvesRec { total_db, bands_db }
}

#[uniffi::export]
impl WaveyDocument {
    /// Preview data of a model embedded in the project.
    pub fn model_preview(&self, model_id: String) -> Result<SpeakerPreviewRec, WaveyError> {
        let doc = self.lock();
        let m = doc
            .project()
            .speaker_models
            .get(&model_id)
            .ok_or_else(|| WaveyError::new("no such model"))?;
        Ok(preview_of(m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_curves_total_is_the_sum_of_the_bands_in_db() {
        let r = filter_curves(
            vec![
                crate::FilterRec::Peak {
                    freq_hz: 1000.0,
                    gain_db: 6.0,
                    q: 2.0,
                },
                crate::FilterRec::LowShelf {
                    freq_hz: 100.0,
                    gain_db: -4.0,
                    q: None,
                },
            ],
            vec![50.0, 1000.0, 8000.0],
        );
        for i in 0..3 {
            assert!((r.total_db[i] - r.bands_db[0][i] - r.bands_db[1][i]).abs() < 1e-12);
        }
        assert!((r.bands_db[0][1] - 6.0).abs() < 1e-9);
    }
}
