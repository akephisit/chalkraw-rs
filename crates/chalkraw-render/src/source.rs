use crate::device::RenderDevice;

/// A linear RGBA16Float source texture on the GPU.
///
/// `LinearImage` arrives as f32; we convert to f16 on upload via bytemuck
/// for the RGBA16Float texture format.
pub struct SourceTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
}

impl SourceTexture {
    /// Upload `pixels` (length = width*height*4, each pixel RGBA f32 in 0..1).
    pub fn upload(rd: &RenderDevice, width: u32, height: u32, pixels: &[f32]) -> Self {
        assert_eq!(
            pixels.len() as u64,
            width as u64 * height as u64 * 4,
            "pixel buffer size mismatch"
        );

        // Convert f32 → f16 (half) for RGBA16Float. wgpu accepts u16 bit patterns.
        let half_pixels: Vec<u16> = pixels.iter().map(|&v| f32_to_f16_bits(v)).collect();

        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = rd.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("chalkraw source"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        rd.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&half_pixels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4 * 2), // 4 channels × 2 bytes (f16)
                rows_per_image: Some(height),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { texture, view, width, height }
    }
}

/// IEEE-754 binary32 → binary16 (round-to-nearest-even).
fn f32_to_f16_bits(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x007f_ffff;

    if exp == 0xff {
        // Inf / NaN
        let m = if mant != 0 { 0x0200 } else { 0 };
        return sign | 0x7c00 | m;
    }
    let unbiased = exp - 127 + 15;
    if unbiased >= 0x1f {
        return sign | 0x7c00; // overflow → Inf
    }
    if unbiased <= 0 {
        if unbiased < -10 {
            return sign; // underflow → 0
        }
        let mant = mant | 0x0080_0000;
        let shift = 14 - unbiased;
        let half = (mant >> shift) as u16;
        let round = ((mant >> (shift - 1)) & 1) as u16;
        return sign | (half + round);
    }
    let half_exp = (unbiased as u16) << 10;
    let half_mant = (mant >> 13) as u16;
    let round = ((mant >> 12) & 1) as u16;
    sign | half_exp | (half_mant + round)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uploads_small_image_or_skips_in_sandbox() {
        let rd = match RenderDevice::new_headless() {
            Ok(rd) => rd,
            Err(_) => {
                eprintln!("skipping: no GPU");
                return;
            }
        };
        let w = 4;
        let h = 4;
        let pixels: Vec<f32> = (0..w * h)
            .flat_map(|i| [(i as f32) / 16.0, 0.5, 1.0, 1.0])
            .collect();
        let src = SourceTexture::upload(&rd, w, h, &pixels);
        assert_eq!(src.width, 4);
        assert_eq!(src.height, 4);
    }

    #[test]
    fn f16_for_known_values_matches_ieee_754_binary16() {
        // Known IEEE-754 binary16 bit patterns. See Wikipedia "Half-precision floating-point format".
        let cases: &[(f32, u16)] = &[
            (0.0,  0x0000), // +0
            (1.0,  0x3c00), // 1.0 = 0 01111 0000000000
            (0.5,  0x3800), // 0.5 = 0 01110 0000000000
            (0.25, 0x3400), // 0.25 = 0 01101 0000000000
            (2.0,  0x4000), // 2.0 = 0 10000 0000000000
        ];
        for &(input, expected) in cases {
            let actual = f32_to_f16_bits(input);
            assert_eq!(
                actual, expected,
                "f32_to_f16_bits({input}) = 0x{actual:04x}, expected 0x{expected:04x}"
            );
        }
    }
}
