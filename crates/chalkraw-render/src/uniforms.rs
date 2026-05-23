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
/// Phase 2B (HSL): 8 colors × {hue, sat, lum}, stored as 6 vec4 chunks of 4 colors each.
/// 128  hsl_hue_a        [f32;4]  red, orange, yellow, green
/// 144  hsl_hue_b        [f32;4]  aqua, blue, purple, magenta
/// 160  hsl_sat_a        [f32;4]
/// 176  hsl_sat_b        [f32;4]
/// 192  hsl_lum_a        [f32;4]
/// 208  hsl_lum_b        [f32;4]
/// Phase 2C (Color Grading): 4 regions × {hue, sat, lum} + blending/balance.
/// 224  cg_hue           [f32;4]  hue for [shadows, midtones, highlights, global]
/// 240  cg_sat           [f32;4]  saturation for [shadows, midtones, highlights, global]
/// 256  cg_lum           [f32;4]  luminance for [shadows, midtones, highlights, global]
/// 272  cg_blend_balance [f32;4]  [blending, balance, 0, 0]
/// Total: 288 bytes
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
    // Phase 2B (HSL): 8 colors × {hue, sat, lum}, stored as 6 vec4 chunks of 4 colors each.
    // Each [f32; 4] is naturally 16-byte aligned, matching WGSL vec4<f32>.
    pub hsl_hue_a: [f32; 4],   // offset 128  red, orange, yellow, green
    pub hsl_hue_b: [f32; 4],   // offset 144  aqua, blue, purple, magenta
    pub hsl_sat_a: [f32; 4],   // offset 160
    pub hsl_sat_b: [f32; 4],   // offset 176
    pub hsl_lum_a: [f32; 4],   // offset 192
    pub hsl_lum_b: [f32; 4],   // offset 208
    // Phase 2C (Color Grading): 4 regions × {hue, sat, lum} + blending/balance.
    pub cg_hue: [f32; 4],          // offset 224  hue for [shadows, midtones, highlights, global]
    pub cg_sat: [f32; 4],          // offset 240  saturation for [shadows, midtones, highlights, global]
    pub cg_lum: [f32; 4],          // offset 256  luminance for [shadows, midtones, highlights, global]
    pub cg_blend_balance: [f32; 4], // offset 272  [blending, balance, 0, 0]
}

impl From<&EditState> for EditUniforms {
    fn from(e: &EditState) -> Self {
        // HSL: 8 bands in order red(0), orange(1), yellow(2), green(3),
        //                       aqua(4), blue(5), purple(6), magenta(7).
        let h = &e.hsl;
        let cg = &e.color_grading;
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
            hsl_hue_a: [h[0].hue, h[1].hue, h[2].hue, h[3].hue],
            hsl_hue_b: [h[4].hue, h[5].hue, h[6].hue, h[7].hue],
            hsl_sat_a: [h[0].saturation, h[1].saturation, h[2].saturation, h[3].saturation],
            hsl_sat_b: [h[4].saturation, h[5].saturation, h[6].saturation, h[7].saturation],
            hsl_lum_a: [h[0].luminance, h[1].luminance, h[2].luminance, h[3].luminance],
            hsl_lum_b: [h[4].luminance, h[5].luminance, h[6].luminance, h[7].luminance],
            // Phase 2C: Color Grading — 4 regions: shadows(0), midtones(1), highlights(2), global(3).
            cg_hue: [cg.shadows.hue, cg.midtones.hue, cg.highlights.hue, cg.global.hue],
            cg_sat: [cg.shadows.saturation, cg.midtones.saturation, cg.highlights.saturation, cg.global.saturation],
            cg_lum: [cg.shadows.luminance, cg.midtones.luminance, cg.highlights.luminance, cg.global.luminance],
            cg_blend_balance: [cg.blending, cg.balance, 0.0, 0.0],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_uniforms_size_matches_wgsl() {
        // Must be 288 bytes to match the WGSL EditUniforms struct layout.
        // Phase 2A: 128 bytes. Phase 2B adds 6 × vec4<f32> = 96 bytes → 224.
        // Phase 2C adds 4 × vec4<f32> = 64 bytes → 288.
        // If this fails, check that the Rust and WGSL fields are in sync.
        assert_eq!(
            std::mem::size_of::<EditUniforms>(),
            288,
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
