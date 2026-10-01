//! Import of tabular polar data (CSV/TSV/whitespace).
//!
//! Columns: `frequency_hz, theta_deg, phi_deg, magnitude_db_spl[, phase_deg]`,
//! one row per measurement point, using the Wavey angle convention. Lines
//! starting with `#` and a non-numeric header row are ignored. Every
//! (frequency, θ, φ) combination of the grid must be present, and θ = 0 rows
//! define the on-axis response.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{Balloon, Enclosure, OnAxis, Reference, SpeakerModel};

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("line {line}: {msg}")]
    Parse { line: usize, msg: String },
    #[error("{0}")]
    Grid(String),
}

#[derive(PartialEq, PartialOrd, Eq, Ord, Clone, Copy)]
struct Key(i64);

fn key(x: f64) -> Key {
    Key((x * 1000.0).round() as i64)
}

pub fn import_table(
    text: &str,
    id: &str,
    name: &str,
    reference: Reference,
) -> Result<SpeakerModel, ImportError> {
    let mut rows: BTreeMap<(Key, Key, Key), (f64, Option<f64>)> = BTreeMap::new();
    let (mut freqs, mut thetas, mut phis) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let mut has_phase = None;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> =
            line.split(|c: char| c == ',' || c == ';' || c == '\t' || c.is_whitespace()).filter(|s| !s.is_empty()).collect();
        let nums: Result<Vec<f64>, _> = cols.iter().map(|c| c.parse::<f64>()).collect();
        let Ok(nums) = nums else {
            if rows.is_empty() {
                continue; // header
            }
            return Err(ImportError::Parse { line: n + 1, msg: "expected numbers".into() });
        };
        if nums.len() < 4 || nums.len() > 5 {
            return Err(ImportError::Parse { line: n + 1, msg: "expected 4 or 5 columns".into() });
        }
        let row_phase = nums.len() == 5;
        if *has_phase.get_or_insert(row_phase) != row_phase {
            return Err(ImportError::Parse { line: n + 1, msg: "inconsistent column count".into() });
        }
        let (f, t, p) = (nums[0], nums[1], nums[2].rem_euclid(360.0));
        if !(f > 0.0) || !(0.0..=180.0).contains(&t) {
            return Err(ImportError::Parse { line: n + 1, msg: "frequency must be > 0 and θ within 0…180".into() });
        }
        freqs.insert(key(f));
        thetas.insert(key(t));
        phis.insert(key(p));
        rows.insert((key(f), key(t), key(p)), (nums[3], nums.get(4).copied()));
    }
    if rows.is_empty() {
        return Err(ImportError::Grid("no data rows".into()));
    }
    let to_f = |k: &Key| k.0 as f64 / 1000.0;
    let freqs: Vec<Key> = freqs.into_iter().collect();
    let thetas: Vec<Key> = thetas.into_iter().collect();
    let phis: Vec<Key> = phis.into_iter().collect();
    if thetas[0] != key(0.0) {
        return Err(ImportError::Grid("data must include θ = 0 (on-axis)".into()));
    }
    let get = |f: Key, t: Key, p: Key| {
        rows.get(&(f, t, p)).copied().ok_or_else(|| {
            ImportError::Grid(format!("missing point f={} θ={} φ={}", to_f(&f), to_f(&t), to_f(&p)))
        })
    };

    let mut on_mag = Vec::new();
    let mut on_ph = Vec::new();
    for &f in &freqs {
        // On-axis is the same for every φ at θ = 0; use the first.
        let (m, ph) = get(f, thetas[0], phis[0])?;
        on_mag.push(m);
        on_ph.push(ph.unwrap_or(0.0));
    }
    let mut magnitude_db = Vec::new();
    let mut phase_deg = Vec::new();
    for (fi, &f) in freqs.iter().enumerate() {
        for &t in &thetas {
            for &p in &phis {
                let (m, ph) = get(f, t, p)?;
                magnitude_db.push((m - on_mag[fi]) as f32);
                phase_deg.push((ph.unwrap_or(0.0) - on_ph[fi]) as f32);
            }
        }
    }
    let balloon = (thetas.len() > 1).then(|| Balloon {
        theta_deg: thetas.iter().map(to_f).collect(),
        phi_deg: phis.iter().map(to_f).collect(),
        magnitude_db,
        phase_deg,
    });
    let m = SpeakerModel {
        format: crate::model::FORMAT.into(),
        version: crate::model::VERSION,
        id: id.into(),
        name: name.into(),
        manufacturer: None,
        frequencies_hz: freqs.iter().map(to_f).collect(),
        reference,
        on_axis: OnAxis { magnitude_db_spl: on_mag, phase_deg: on_ph },
        max_input_voltage_rms: None,
        max_spl_db: None,
        latency_s: 0.0,
        acoustic_center_m: [0.0; 3],
        enclosure: Enclosure::default(),
        balloon,
        phase_estimated: has_phase != Some(true),
    };
    m.validate().map_err(|e| ImportError::Grid(e.to_string()))?;
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    #[test]
    fn imports_grid() {
        let mut text = String::from("freq,theta,phi,mag,phase\n");
        for f in [100.0, 1000.0] {
            for t in [0.0, 90.0, 180.0] {
                for p in [0.0, 90.0, 180.0, 270.0] {
                    let mag = 90.0 - t / 10.0;
                    text += &format!("{f},{t},{p},{mag},{}\n", -t / 2.0);
                }
            }
        }
        let m = import_table(&text, "x", "X", Reference { distance_m: 1.0, input_voltage_rms: 2.83 }).unwrap();
        assert!(!m.phase_estimated);
        assert!((m.sensitivity_db(1000.0) - 90.0).abs() < 1e-9);
        let side = m.directivity(1000.0, DVec3::Y);
        assert!((20.0 * side.norm().log10() + 9.0).abs() < 1e-4);
        assert!((side.arg().to_degrees() + 45.0).abs() < 1e-4);
    }

    #[test]
    fn missing_point_is_reported() {
        let text = "100 0 0 90\n100 90 0 85\n100 90 90 85\n";
        assert!(import_table(text, "x", "X", Reference { distance_m: 1.0, input_voltage_rms: 2.83 }).is_err());
    }
}
