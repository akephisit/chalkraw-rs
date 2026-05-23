use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const EDIT_SCHEMA_VERSION: u32 = 1;
pub const MAX_HISTORY: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WhiteBalance {
    pub temp_kelvin: f32, // identity: 5500.0
    pub tint: f32,        // identity: 0.0, range -150..150
}

impl Default for WhiteBalance {
    fn default() -> Self {
        Self { temp_kelvin: 5500.0, tint: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Tone {
    pub exposure: f32,    // EV stops, identity 0.0, range -5..5
    pub contrast: f32,    // identity 0.0, range -100..100
    pub highlights: f32,  // identity 0.0, range -100..100
    pub shadows: f32,     // identity 0.0, range -100..100
    pub whites: f32,      // identity 0.0, range -100..100
    pub blacks: f32,      // identity 0.0, range -100..100
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Presence {
    pub texture: f32,     // identity 0.0, range -100..100
    pub clarity: f32,     // identity 0.0, range -100..100
    pub dehaze: f32,      // identity 0.0, range -100..100
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ColorMix {
    pub vibrance: f32,    // identity 0.0, range -100..100
    pub saturation: f32,  // identity 0.0, range -100..100
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub x: f32, // input 0..1
    pub y: f32, // output 0..1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Curve(pub Vec<CurvePoint>);

impl Default for Curve {
    /// Linear curve: y = x. Identity.
    fn default() -> Self {
        Self(vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 1.0, y: 1.0 },
        ])
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ToneCurve {
    pub rgb: Curve,
    pub red: Curve,
    pub green: Curve,
    pub blue: Curve,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct HslAdjustment {
    pub hue: f32,        // identity 0.0, range -100..100
    pub saturation: f32, // identity 0.0, range -100..100
    pub luminance: f32,  // identity 0.0, range -100..100
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum HslColor {
    Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct GradeTone {
    pub hue: f32,        // 0..360 degrees, identity 0
    pub saturation: f32, // 0..100, identity 0
    pub luminance: f32,  // -100..100, identity 0
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ColorGrading {
    pub shadows: GradeTone,
    pub midtones: GradeTone,
    pub highlights: GradeTone,
    pub global: GradeTone,
    pub blending: f32,   // 0..100, identity 50
    pub balance: f32,    // -100..100, identity 0
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sharpening {
    pub amount: f32,  // 0..150, identity 0
    pub radius: f32,  // 0.5..3.0, identity 1.0
    pub detail: f32,  // 0..100, identity 25
    pub masking: f32, // 0..100, identity 0
}

impl Default for Sharpening {
    fn default() -> Self {
        Self { amount: 0.0, radius: 1.0, detail: 25.0, masking: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct NoiseReduction {
    pub luminance: f32, // 0..100, identity 0
    pub color: f32,     // 0..100, identity 0
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Detail {
    pub sharpening: Sharpening,
    pub noise_reduction: NoiseReduction,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vignette {
    pub amount: f32,    // -100..100, identity 0
    pub midpoint: f32,  // 0..100, identity 50
    pub feather: f32,   // 0..100, identity 50
    pub roundness: f32, // -100..100, identity 0
}

impl Default for Vignette {
    fn default() -> Self {
        Self { amount: 0.0, midpoint: 50.0, feather: 50.0, roundness: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Grain {
    pub amount: f32,    // 0..100, identity 0
    pub size: f32,      // 0..100, identity 25
    pub roughness: f32, // 0..100, identity 50
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Effects {
    pub vignette: Vignette,
    pub grain: Grain,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct LensCorrection {
    pub distortion: f32, // -100..100, identity 0
    pub vignetting: f32, // 0..100, identity 0 (correction amount)
    pub auto_profile: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    pub x_pct: f32,   // 0..1
    pub y_pct: f32,
    pub w_pct: f32,
    pub h_pct: f32,
    pub rotation_deg: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditSnapshot {
    pub state: Box<EditState>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditState {
    pub white_balance: WhiteBalance,
    pub tone: Tone,
    pub presence: Presence,
    pub color: ColorMix,
    pub tone_curve: ToneCurve,
    pub hsl: [HslAdjustment; 8],
    pub color_grading: ColorGrading,
    pub detail: Detail,
    pub effects: Effects,
    pub lens_correction: LensCorrection,
    pub crop: Option<Crop>,
    pub history: VecDeque<EditSnapshot>,
    pub version: u32,
}

impl Default for EditState {
    fn default() -> Self {
        Self {
            white_balance: WhiteBalance::default(),
            tone: Tone::default(),
            presence: Presence::default(),
            color: ColorMix::default(),
            tone_curve: ToneCurve::default(),
            hsl: [HslAdjustment::default(); 8],
            color_grading: ColorGrading::default(),
            detail: Detail::default(),
            effects: Effects::default(),
            lens_correction: LensCorrection::default(),
            crop: None,
            history: VecDeque::with_capacity(MAX_HISTORY),
            version: EDIT_SCHEMA_VERSION,
        }
    }
}

impl EditState {
    /// True if every adjustment is at its identity (no-op) value.
    pub fn is_identity(&self) -> bool {
        let d = Self::default();
        self.white_balance == d.white_balance
            && self.tone == d.tone
            && self.presence == d.presence
            && self.color == d.color
            && self.tone_curve == d.tone_curve
            && self.hsl == d.hsl
            && self.color_grading == d.color_grading
            && self.detail == d.detail
            && self.effects == d.effects
            && self.lens_correction == d.lens_correction
            && self.crop == d.crop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_identity() {
        let s = EditState::default();
        assert!(s.is_identity(), "default EditState must be identity");
        assert_eq!(s.version, EDIT_SCHEMA_VERSION);
        assert_eq!(s.white_balance.temp_kelvin, 5500.0);
        assert_eq!(s.tone.exposure, 0.0);
        assert_eq!(s.tone_curve.rgb.0.len(), 2); // linear curve
    }

    #[test]
    fn exposure_change_breaks_identity() {
        let mut s = EditState::default();
        s.tone.exposure = 1.0;
        assert!(!s.is_identity());
    }

    #[test]
    fn edit_state_roundtrips_through_bincode() {
        let mut s = EditState::default();
        s.tone.exposure = 0.5;
        s.white_balance.temp_kelvin = 6500.0;
        let bytes = bincode::serialize(&s).unwrap();
        let back: EditState = bincode::deserialize(&bytes).unwrap();
        assert_eq!(s, back);
    }
}
