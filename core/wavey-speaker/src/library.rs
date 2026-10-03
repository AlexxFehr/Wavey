//! A folder of speaker models (the stock library): every `.waveyspk` below
//! it, summarised without loading the balloons. Summaries are cached in an
//! index file in the folder, keyed by path, size and modification time, so
//! only new or changed files are read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

use crate::Enclosure;

pub const INDEX_FILE: &str = ".wavey-library-index.json";

/// One model file in a library folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub path: PathBuf,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub manufacturer: Option<String>,
    pub min_hz: f64,
    pub max_hz: f64,
    #[serde(default)]
    pub nominal_band_hz: Option<[f64; 2]>,
    pub has_balloon: bool,
    pub has_rigging: bool,
    pub enclosure: Enclosure,
    pub size: u64,
    pub modified: u64,
}

impl LibraryEntry {
    /// Upper end of the band the device is used in.
    pub fn top_hz(&self) -> f64 {
        self.nominal_band_hz.map_or(self.max_hz, |b| b[1])
    }
}

/// The fields of a `.waveyspk` the library shows; the rest is skipped.
#[derive(Deserialize)]
struct Header {
    id: String,
    name: String,
    #[serde(default)]
    manufacturer: Option<String>,
    frequencies_hz: Vec<f64>,
    #[serde(default)]
    nominal_band_hz: Option<[f64; 2]>,
    #[serde(default)]
    enclosure: Option<Enclosure>,
    #[serde(default)]
    balloon: Option<IgnoredAny>,
    #[serde(default)]
    rigging: Option<IgnoredAny>,
}

#[derive(Default, Serialize, Deserialize)]
struct Index {
    entries: BTreeMap<String, LibraryEntry>,
}

fn stamp(path: &Path) -> Option<(u64, u64)> {
    let md = std::fs::metadata(path).ok()?;
    let t = md.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some((md.len(), t))
}

/// Reads one model file's summary.
pub fn read_entry(path: &Path) -> Result<LibraryEntry, String> {
    let (size, modified) = stamp(path).ok_or("cannot read the file")?;
    let text = std::fs::read(path).map_err(|e| e.to_string())?;
    let h: Header = serde_json::from_slice(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(LibraryEntry {
        path: path.to_path_buf(),
        id: h.id,
        name: h.name,
        manufacturer: h.manufacturer.filter(|m| !m.trim().is_empty()),
        min_hz: h.frequencies_hz.first().copied().unwrap_or(0.0),
        max_hz: h.frequencies_hz.last().copied().unwrap_or(0.0),
        nominal_band_hz: h.nominal_band_hz,
        has_balloon: h.balloon.is_some(),
        has_rigging: h.rigging.is_some(),
        enclosure: h.enclosure.unwrap_or_default(),
        size,
        modified,
    })
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let hidden = p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'));
        if hidden {
            continue;
        }
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "waveyspk") {
            out.push(p);
        }
    }
}

/// All models below `dir`, sorted by manufacturer then name. Unreadable
/// files are reported in the second list. The index file is updated when
/// anything changed (and silently skipped if the folder is read-only).
pub fn scan(dir: &Path) -> (Vec<LibraryEntry>, Vec<String>) {
    let index_path = dir.join(INDEX_FILE);
    let old: Index = std::fs::read(&index_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let mut files = vec![];
    walk(dir, &mut files);
    let mut index = Index::default();
    let mut errors = vec![];
    let mut changed = false;
    for f in files {
        let key = f.to_string_lossy().to_string();
        let cached = old.entries.get(&key).filter(|e| stamp(&f) == Some((e.size, e.modified)));
        let entry = match cached {
            Some(e) => e.clone(),
            None => {
                changed = true;
                match read_entry(&f) {
                    Ok(e) => e,
                    Err(e) => {
                        errors.push(e);
                        continue;
                    }
                }
            }
        };
        index.entries.insert(key, entry);
    }
    changed |= index.entries.len() != old.entries.len();
    if changed && let Ok(json) = serde_json::to_vec(&index) {
        let _ = std::fs::write(&index_path, json);
    }
    let mut list: Vec<LibraryEntry> = index.entries.into_values().collect();
    list.sort_by(|a, b| {
        let ma = a.manufacturer.as_deref().unwrap_or("").to_lowercase();
        let mb = b.manufacturer.as_deref().unwrap_or("").to_lowercase();
        ma.cmp(&mb).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    (list, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_and_caches() {
        let dir = std::env::temp_dir().join(format!("wavey-lib-{}", std::process::id()));
        let sub = dir.join("Acme");
        std::fs::create_dir_all(&sub).unwrap();
        let mut m = crate::samples::two_way();
        m.manufacturer = Some("Acme".into());
        m.save(&sub.join("two-way.waveyspk")).unwrap();
        let mut o = crate::samples::omni();
        o.manufacturer = None;
        o.save(&dir.join("omni.waveyspk")).unwrap();
        std::fs::write(dir.join("broken.waveyspk"), "{").unwrap();
        let (list, errors) = scan(&dir);
        assert_eq!(list.len(), 2);
        assert_eq!(errors.len(), 1);
        // No manufacturer sorts first; the balloon and band are summarised.
        assert_eq!(list[0].manufacturer, None);
        assert_eq!(list[1].manufacturer.as_deref(), Some("Acme"));
        assert!(list[1].has_balloon && !list[1].has_rigging && list[1].max_hz > list[1].min_hz);
        assert!(dir.join(INDEX_FILE).exists());
        // Second scan from the index gives the same result.
        assert_eq!(scan(&dir).0, list);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
