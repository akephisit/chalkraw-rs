//! Read a render target back to CPU memory.
//! Also contains helpers for creating LUT textures used by the develop pipeline.

use bytemuck;
use crate::device::RenderDevice;
use crate::error::RenderError;
use crate::source::f32_to_f16_bits;

/// Create a 256-entry R16Float 1D identity LUT texture + view.
/// Used by render tests as a stand-in for the real tone-curve LUT
/// (identity ramp → no effect on the image).
pub fn make_identity_lut(rd: &RenderDevice) -> (wgpu::Texture, wgpu::TextureView) {
    // Default Curve is [(0,0),(1,1)] — the identity ramp.
    let identity = chalkraw_core::Curve::default();
    make_tone_curve_lut(rd, &identity.0)
}

/// Create a 256-entry R16Float 1D tone-curve LUT texture + view from a slice of
/// `CurvePoint`s. The LUT is sampled as `output = LUT[input * 255]` in the shader.
///
/// Exported as a public helper so the export pipeline and tests can both use it.
pub fn make_tone_curve_lut(rd: &RenderDevice, points: &[chalkraw_core::CurvePoint]) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = rd.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tone curve lut"),
        size: wgpu::Extent3d { width: 256, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D1,
        format: wgpu::TextureFormat::R16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let lut: Vec<u16> = (0u32..256)
        .map(|i| {
            let x = i as f32 / 255.0;
            let y = chalkraw_core::interpolate_curve(points, x);
            f32_to_f16_bits(y.clamp(0.0, 1.0))
        })
        .collect();
    rd.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(&lut),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(256 * 2),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d { width: 256, height: 1, depth_or_array_layers: 1 },
    );
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    (tex, view)
}

pub fn make_target(rd: &RenderDevice, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = rd.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("readback target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

pub fn read_to_cpu(
    rd: &RenderDevice,
    target: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, RenderError> {
    // wgpu requires 256-byte row alignment for buffer copies.
    let bytes_per_row_unpadded = width * 4;
    let padded_row = bytes_per_row_unpadded.div_ceil(256) * 256;
    let buffer_size = (padded_row * height) as u64;

    let buffer = rd.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback buffer"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = rd.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    rd.queue.submit(std::iter::once(encoder.finish()));

    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    // wgpu 29: poll takes PollType (not Maintain) and returns Result<PollStatus, PollError>.
    // wait_indefinitely() blocks until the most recent submission is complete.
    rd.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().expect("map_async channel closed").map_err(RenderError::from)?;

    let mapped = slice.get_mapped_range();
    let mut out = Vec::with_capacity((width * 4 * height) as usize);
    for row in 0..height {
        let start = (row * padded_row) as usize;
        let end = start + (width * 4) as usize;
        out.extend_from_slice(&mapped[start..end]);
    }
    drop(mapped);
    buffer.unmap();

    Ok(out)
}
