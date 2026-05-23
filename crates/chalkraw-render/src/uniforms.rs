use bytemuck::{Pod, Zeroable};
use chalkraw_core::EditState;

/// std140-ish uniform layout for the develop shader. Padded so every field aligns
/// on 16 bytes in WGSL.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct EditUniforms {
    pub exposure: f32,    // EV stops
    // WGSL vec3<f32> has 16-byte alignment, so _pad_tone starts at offset 16.
    // The 12 bytes here bridge the 4-byte gap left by `exposure`.
    pub _pre_pad: [f32; 3],
    pub _pad_tone: [f32; 3],
    // Reserved slots for Phase 2 fields. Holes are intentional so adding fields
    // later does not invalidate Phase 1 shader bindings — WGSL just reads more.
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub _pad_basic: [f32; 3],
    pub temp_kelvin: f32,
    pub tint: f32,
    // vec2<f32> has 8-byte alignment; tint ends at offset 68, so we need 4 bytes
    // of padding here to align _pad_wb to offset 72.
    pub _post_tint_pad: f32,
    pub _pad_wb: [f32; 2],
    pub vibrance: f32,
    pub saturation: f32,
    pub texture: f32,
    pub clarity: f32,
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
        }
    }
}
