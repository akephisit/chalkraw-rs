use bytemuck::{Pod, Zeroable};
use chalkraw_core::EditState;

/// std140-ish uniform layout for the develop shader. Padded so every field
/// aligns on 16 bytes in WGSL. The byte layout must mirror the WGSL struct
/// in `develop.wgsl` field-for-field.
///
/// Layout map (offsets in bytes):
///   0  exposure         f32
///   4  _pre_pad         [f32;3]  (pads to 16, bridging gap before _pad_tone)
///  16  _pad_tone        [f32;3]  (vec3<f32> occupies 12 bytes at align 16)
///  28  contrast         f32
///  32  highlights       f32
///  36  shadows          f32
///  40  whites           f32
///  44  blacks           f32
///  48  _pad_basic       [f32;3]
///  60  temp_kelvin      f32
///  64  tint             f32
///  68  _post_tint_pad   f32
///  72  _pad_wb          [f32;2]
///  80  vibrance         f32
///  84  saturation       f32
///  88  texture          f32
///  92  clarity          f32
///  96  vignette_amount  f32
/// 100  vignette_midpoint f32
/// 104  vignette_feather f32
/// 108  vignette_roundness f32
/// 112  grain_amount     f32
/// 116  grain_size       f32
/// 120  grain_roughness  f32
/// 124  _pad_grain       f32
/// Total: 128 bytes
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct EditUniforms {
    pub exposure: f32,          // offset   0
    pub _pre_pad: [f32; 3],     // offset   4  → pads to 16
    pub _pad_tone: [f32; 3],    // offset  16  → vec3<f32> slot in WGSL
    pub contrast: f32,          // offset  28
    pub highlights: f32,        // offset  32
    pub shadows: f32,           // offset  36
    pub whites: f32,            // offset  40
    pub blacks: f32,            // offset  44
    pub _pad_basic: [f32; 3],   // offset  48  → pads to 60
    pub temp_kelvin: f32,       // offset  60
    pub tint: f32,              // offset  64
    pub _post_tint_pad: f32,    // offset  68
    pub _pad_wb: [f32; 2],      // offset  72
    pub vibrance: f32,          // offset  80
    pub saturation: f32,        // offset  84
    pub texture: f32,           // offset  88
    pub clarity: f32,           // offset  92
    // Vignette: 4 × f32 = 16 bytes, naturally aligned to 16-byte boundary.
    pub vignette_amount: f32,   // offset  96
    pub vignette_midpoint: f32, // offset 100
    pub vignette_feather: f32,  // offset 104
    pub vignette_roundness: f32,// offset 108
    // Grain: 3 × f32 + 1 pad = 16 bytes.
    pub grain_amount: f32,      // offset 112
    pub grain_size: f32,        // offset 116
    pub grain_roughness: f32,   // offset 120  (reserved; no shader effect in 2A)
    pub _pad_grain: f32,        // offset 124  → pads to 128
}

impl From<&EditState> for EditUniforms {
    fn from(e: &EditState) -> Self {
        Self {
            exposure: e.tone.exposure,
            _pre_pad: [0.0; 3],
            _pad_tone: [0.0; 3],
            contrast: e.tone.contrast,
            highlights: e.tone.highlights,
            shadows: e.tone.shadows,
            whites: e.tone.whites,
            blacks: e.tone.blacks,
            _pad_basic: [0.0; 3],
            temp_kelvin: e.white_balance.temp_kelvin,
            tint: e.white_balance.tint,
            _post_tint_pad: 0.0,
            _pad_wb: [0.0; 2],
            vibrance: e.color.vibrance,
            saturation: e.color.saturation,
            texture: e.presence.texture,
            clarity: e.presence.clarity,
            vignette_amount: e.effects.vignette.amount,
            vignette_midpoint: e.effects.vignette.midpoint,
            vignette_feather: e.effects.vignette.feather,
            vignette_roundness: e.effects.vignette.roundness,
            grain_amount: e.effects.grain.amount,
            grain_size: e.effects.grain.size,
            grain_roughness: e.effects.grain.roughness,
            _pad_grain: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_uniforms_size_matches_wgsl() {
        // Must be 128 bytes to match the WGSL EditUniforms struct layout.
        // If this fails, check that the Rust and WGSL fields are in sync.
        assert_eq!(
            std::mem::size_of::<EditUniforms>(),
            128,
            "EditUniforms size mismatch — Rust and WGSL structs are out of sync"
        );
    }

    #[test]
    fn from_edit_state_maps_all_fields() {
        let mut s = EditState::default();
        s.tone.exposure = 1.5;
        s.tone.contrast = 30.0;
        s.effects.vignette.amount = -50.0;
        s.effects.grain.amount = 25.0;
        let u = EditUniforms::from(&s);
        assert_eq!(u.exposure, 1.5);
        assert_eq!(u.contrast, 30.0);
        assert_eq!(u.vignette_amount, -50.0);
        assert_eq!(u.grain_amount, 25.0);
    }
}
