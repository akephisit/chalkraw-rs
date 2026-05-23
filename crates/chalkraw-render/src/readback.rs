//! Read a render target back to CPU memory. Test-only utility.

use crate::device::RenderDevice;
use crate::error::RenderError;

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
