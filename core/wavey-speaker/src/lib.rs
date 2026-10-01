//! Loudspeaker models: the `.waveyspk` format, directivity evaluation,
//! placement, tabular import and synthetic sample speakers.
//!
//! Conventions (see `docs/speaker-format.md`):
//! - local frame: +x on-axis, +y to the speaker's left, +z up;
//! - θ is the angle from +x (0…180°), φ the rotation about +x measured from
//!   +y toward +z (0…360°);
//! - on-axis response: complex pressure at the reference distance for the
//!   reference input, with propagation delay removed (phase referenced to
//!   the acoustic center);
//! - balloon: complex directivity normalized so that D(θ = 0) = 1.

mod import;
mod instance;
mod model;
pub mod samples;

pub use import::{ImportError, import_table};
pub use instance::SpeakerInstance;
pub use model::{Balloon, Enclosure, OnAxis, Reference, SpeakerError, SpeakerModel};
