//! Line-array rigging in .waveyspk files (from gll2wavey). The Arcline 8
//! model: `scripts/gll2wavey.sh -in Void_Acoustics_Arcline_8_v20a.gll
//! -out speakers/void-arcline-8.waveyspk -filters keyFilter_HF,keyFilter_MF,keyFilter_LF`.

use std::path::Path;

use wavey_speaker::SpeakerModel;

/// A converted model from `speakers/` (not in the repository: made with
/// `scripts/gll2wavey.sh`); None when absent.
fn speaker(name: &str) -> Option<SpeakerModel> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../speakers").join(name);
    if !p.exists() {
        eprintln!("skipped: {} not found", p.display());
        return None;
    }
    Some(SpeakerModel::load(&p).unwrap())
}

#[test]
fn arcline_rigging_is_read() {
    let Some(m) = speaker("void-arcline-8.waveyspk") else { return };
    let r = m.rigging.as_ref().expect("rigging");
    // Hinges on the centre line, one box height apart.
    assert!(r.pivot[1].abs() < 1e-6 && r.next_pivot[1].abs() < 1e-6);
    let pitch = r.pivot[2] - r.next_pivot[2];
    assert!((pitch - 0.2865).abs() < 1e-3, "hinge spacing {pitch}");
    assert_eq!(r.splays_deg.first(), Some(&0.0));
    assert_eq!(r.splays_deg.last(), Some(&10.0));
    let fly = r.frames.iter().find(|f| f.flown).unwrap();
    assert_eq!(fly.pick_points.len(), 20);
    assert_eq!(fly.max_boxes, Some(24));
    assert!(r.frames.iter().any(|f| !f.flown && f.max_boxes == Some(6)));
    // Round trip.
    let back: SpeakerModel = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(back.rigging, m.rigging);
}

#[test]
fn models_without_rigging_still_load() {
    if let Some(m) = speaker("yamaha-dxs18xlf.waveyspk") {
        assert!(m.rigging.is_none());
    }
}
