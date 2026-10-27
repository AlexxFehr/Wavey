//! FIR filters on sources and groups.

use std::sync::Arc;

use wavey_speaker::Fir;

use crate::speakers::FirInfoRec;
use crate::{ResponseRec, WaveyDocument, WaveyError};

/// What a FIR filter is set on.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum FirTarget {
    /// Each of these sources gets the same filter.
    Sources { ids: Vec<u64> },
    /// A group: applied to all its members on top of their own.
    Group { id: u64 },
}

fn check(p: &wavey_project::Project, target: &FirTarget) -> Result<(), String> {
    match target {
        FirTarget::Sources { ids } => ids.iter().try_for_each(|id| p.speakers.contains_key(id).then_some(()).ok_or(format!("no speaker {id}"))),
        FirTarget::Group { id } => p.meta.groups.contains_key(id).then_some(()).ok_or(format!("no group {id}")),
    }
}

fn put(p: &mut wavey_project::Project, target: &FirTarget, fir: Option<Arc<Fir>>) {
    match target {
        FirTarget::Sources { ids } => ids.iter().for_each(|id| p.speakers.get_mut(id).unwrap().fir = fir.clone()),
        FirTarget::Group { id } => p.meta.groups.get_mut(id).unwrap().fir = fir,
    }
}

#[uniffi::export]
impl WaveyDocument {
    /// Reads a WAV (its own sample rate) or a text file of taps (at
    /// `text_sample_rate_hz`) and sets it as the FIR filter of `target`.
    pub fn set_fir(&self, target: FirTarget, path: String, text_sample_rate_hz: f64) -> Result<FirInfoRec, WaveyError> {
        let bytes = std::fs::read(&path).map_err(|e| WaveyError::new(format!("cannot read {path}: {e}")))?;
        let name = std::path::Path::new(&path).file_name().map_or(path.clone(), |n| n.to_string_lossy().into_owned());
        let fir = Arc::new(Fir::from_bytes(&name, &bytes, text_sample_rate_hz).map_err(|e| WaveyError::new(e.to_string()))?);
        let info = FirInfoRec::from(&*fir);
        self.lock()
            .try_edit(|p| {
                check(p, &target)?;
                put(p, &target, Some(fir));
                Ok::<_, String>(())
            })
            .map_err(WaveyError::new)?;
        Ok(info)
    }

    pub fn clear_fir(&self, target: FirTarget) {
        self.lock().edit(|p| {
            if check(p, &target).is_ok() {
                put(p, &target, None);
            }
        });
    }

    /// Magnitude and phase of a source's or group's FIR on a log grid from
    /// 20 Hz to 20 kHz; empty when it has none.
    pub fn fir_response(&self, target: FirTarget) -> ResponseRec {
        let doc = self.lock();
        let p = doc.project();
        let fir = match &target {
            FirTarget::Sources { ids } => ids.first().and_then(|id| p.speakers.get(id)).and_then(|s| s.fir.clone()),
            FirTarget::Group { id } => p.meta.groups.get(id).and_then(|g| g.fir.clone()),
        };
        let freqs: Vec<f64> = (0..=120).map(|i| 20.0 * 1000f64.powf(i as f64 / 120.0)).collect();
        let Some(fir) = fir else { return ResponseRec { freqs_hz: vec![], magnitude_db: vec![], phase_deg: vec![] } };
        let h: Vec<_> = freqs.iter().map(|f| fir.response(*f)).collect();
        ResponseRec {
            magnitude_db: h.iter().map(|v| 20.0 * v.norm().max(1e-9).log10()).collect(),
            phase_deg: h.iter().map(|v| v.arg().to_degrees()).collect(),
            freqs_hz: freqs,
        }
    }
}
