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
    LibraryScanRec { entries: entries.into_iter().map(Into::into).collect(), errors }
}

#[uniffi::export]
impl WaveyDocument {
    /// The project's model for a library file: the one already embedded
    /// under the same id, or the file added to the project. Returns its id.
    pub fn use_library_model(&self, path: String) -> Result<String, WaveyError> {
        let entry = wavey_speaker::library::read_entry(&PathBuf::from(&path)).map_err(WaveyError::new)?;
        if self.lock().project().speaker_models.contains_key(&entry.id) {
            return Ok(entry.id);
        }
        let m = SpeakerModel::load(&PathBuf::from(path)).map_err(WaveyError::new)?;
        self.lock().try_edit(|p| p.add_speaker_model(m)).map_err(WaveyError::new)
    }
}
