//! Export pipeline. Single-photo export today; batch + watermark land later.

use chalkraw_core::EditState;
use chalkraw_io::LinearImage;
use chalkraw_render::{
    make_target, read_to_cpu, DevelopPipeline, EditUniforms, PipelineConfig, RenderDevice,
    SourceTexture,
};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub enum ExportFormat {
    Jpeg { quality: u8 },
    Png,
    Tiff,
}

#[derive(Debug, Clone, Copy)]
pub enum ExportResize {
    Original,
    LongEdge(u32),
}

#[derive(Debug, Clone, Copy)]
pub struct ExportOptions {
    pub format: ExportFormat,
    pub resize: ExportResize,
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("render error: {0}")]
    Render(#[from] chalkraw_render::RenderError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("image encode error: {0}")]
    ImageEncode(#[from] image::ImageError),
}

/// Render the current photo with the given edits and save to `output_path`.
pub fn export_current(
    rd: &RenderDevice,
    image: &LinearImage,
    edit: &EditState,
    output_path: &Path,
    options: ExportOptions,
) -> Result<(), ExportError> {
    let (out_w, out_h) = compute_output_size(image.width, image.height, options.resize);

    // Build pipeline targeting Rgba8UnormSrgb so readback gives sRGB bytes.
    let source = SourceTexture::upload(rd, image.width, image.height, &image.pixels);
    let pipeline = DevelopPipeline::new(rd, PipelineConfig {
        output_format: wgpu::TextureFormat::Rgba8UnormSrgb,
    });
    pipeline.update_uniforms(&EditUniforms::from(edit));
    let bind_group = pipeline.make_bind_group(&source);

    let (target, view) = make_target(rd, out_w, out_h);
    pipeline.render(&view, &bind_group);
    let pixels_rgba = read_to_cpu(rd, &target, out_w, out_h)?;

    // Drop alpha (it's always 1.0 for our pipeline today; many encoders prefer RGB).
    let pixels_rgb = strip_alpha(&pixels_rgba);

    encode_and_write(output_path, &pixels_rgb, out_w, out_h, options.format)?;
    Ok(())
}

fn compute_output_size(w: u32, h: u32, resize: ExportResize) -> (u32, u32) {
    match resize {
        ExportResize::Original => (w, h),
        ExportResize::LongEdge(long) => {
            if w >= h {
                let scale = long as f32 / w as f32;
                (long.max(1), ((h as f32) * scale).round().max(1.0) as u32)
            } else {
                let scale = long as f32 / h as f32;
                (((w as f32) * scale).round().max(1.0) as u32, long.max(1))
            }
        }
    }
}

fn strip_alpha(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len() / 4 * 3);
    for chunk in rgba.chunks_exact(4) {
        out.push(chunk[0]);
        out.push(chunk[1]);
        out.push(chunk[2]);
    }
    out
}

fn encode_and_write(
    path: &Path,
    rgb: &[u8],
    w: u32,
    h: u32,
    format: ExportFormat,
) -> Result<(), ExportError> {
    let file = std::fs::File::create(path)?;
    let writer = std::io::BufWriter::new(file);
    use image::ImageEncoder;
    match format {
        ExportFormat::Jpeg { quality } => {
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(writer, quality);
            encoder.write_image(rgb, w, h, image::ExtendedColorType::Rgb8)?;
        }
        ExportFormat::Png => {
            let encoder = image::codecs::png::PngEncoder::new(writer);
            encoder.write_image(rgb, w, h, image::ExtendedColorType::Rgb8)?;
        }
        ExportFormat::Tiff => {
            let encoder = image::codecs::tiff::TiffEncoder::new(writer);
            encoder.write_image(rgb, w, h, image::ExtendedColorType::Rgb8)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_output_size_original() {
        assert_eq!(compute_output_size(1024, 768, ExportResize::Original), (1024, 768));
    }

    #[test]
    fn compute_output_size_long_edge_landscape() {
        let (w, h) = compute_output_size(1024, 768, ExportResize::LongEdge(512));
        assert_eq!(w, 512);
        assert_eq!(h, 384);
    }

    #[test]
    fn compute_output_size_long_edge_portrait() {
        let (w, h) = compute_output_size(768, 1024, ExportResize::LongEdge(512));
        assert_eq!(w, 384);
        assert_eq!(h, 512);
    }

    #[test]
    fn strip_alpha_drops_fourth_byte() {
        let rgba = vec![1, 2, 3, 255, 4, 5, 6, 255];
        assert_eq!(strip_alpha(&rgba), vec![1, 2, 3, 4, 5, 6]);
    }
}
