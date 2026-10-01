use glam::{DMat3, DVec3};
use num_complex::Complex64;
use serde::{Deserialize, Serialize};

use crate::model::SpeakerModel;

/// A placed loudspeaker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeakerInstance {
    pub id: u64,
    pub name: String,
    pub model_id: String,
    /// Enclosure center in room coordinates (m).
    pub position: DVec3,
    /// Rotation about +z, counter-clockwise from +x (degrees).
    pub yaw_deg: f64,
    /// Positive tilts the axis upward (degrees).
    pub pitch_deg: f64,
    /// Rotation about the speaker's own axis (degrees).
    pub roll_deg: f64,
    pub gain_db: f64,
    pub delay_ms: f64,
    #[serde(default)]
    pub polarity_inverted: bool,
    #[serde(default)]
    pub muted: bool,
}

impl SpeakerInstance {
    /// Local-to-room rotation.
    pub fn rotation(&self) -> DMat3 {
        DMat3::from_rotation_z(self.yaw_deg.to_radians())
            * DMat3::from_rotation_y(-self.pitch_deg.to_radians())
            * DMat3::from_rotation_x(self.roll_deg.to_radians())
    }

    /// Room position of the acoustic center.
    pub fn acoustic_center(&self, model: &SpeakerModel) -> DVec3 {
        self.position + self.rotation() * DVec3::from_array(model.acoustic_center_m)
    }

    /// Converts a room-space direction to the speaker's local frame.
    pub fn to_local(&self, dir: DVec3) -> DVec3 {
        self.rotation().transpose() * dir
    }

    /// Room-space aim direction (+x local).
    pub fn axis(&self) -> DVec3 {
        self.rotation() * DVec3::X
    }

    /// Complex drive factor: gain, polarity and delay (latency included) at `f`.
    pub fn drive(&self, model: &SpeakerModel, f: f64) -> Complex64 {
        if self.muted {
            return Complex64::new(0.0, 0.0);
        }
        let g = 10f64.powf(self.gain_db / 20.0) * if self.polarity_inverted { -1.0 } else { 1.0 };
        let tau = self.delay_ms * 1e-3 + model.latency_s;
        Complex64::from_polar(g, -2.0 * std::f64::consts::PI * f * tau)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(yaw: f64, pitch: f64) -> SpeakerInstance {
        SpeakerInstance {
            id: 1,
            name: "s".into(),
            model_id: "m".into(),
            position: DVec3::ZERO,
            yaw_deg: yaw,
            pitch_deg: pitch,
            roll_deg: 0.0,
            gain_db: 0.0,
            delay_ms: 0.0,
            polarity_inverted: false,
            muted: false,
        }
    }

    #[test]
    fn aiming() {
        assert!((inst(90.0, 0.0).axis() - DVec3::Y).length() < 1e-12);
        let up = inst(0.0, 30.0).axis();
        assert!(up.z > 0.49 && up.z < 0.51);
        let s = inst(45.0, -10.0);
        assert!((s.to_local(s.axis()) - DVec3::X).length() < 1e-12);
    }
}
