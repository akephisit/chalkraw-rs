//! Export pipeline: single-photo and batch export with optional PNG watermark.

pub mod text;

use chalkraw_core::EditState;
use chalkraw_io::LinearImage;
use chalkraw_render::{
    make_target, read_to_cpu, BilateralPipeline, BlurPipeline, create_pingpong, DevelopPipeline,
    EditUniforms, PipelineConfig, RenderDevice, SourceTexture,
};
use std::path::{Path, PathBuf};

// ── Public types ─────────────────────────────────────────────────────────────

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

// ── Watermark types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatermarkAnchor {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

#[derive(Debug, Clone)]
pub struct WatermarkStamp {
    pub png_path: PathBuf,
    pub anchor: WatermarkAnchor,
    /// Percent of output long edge (1..50).
    pub size_pct: f32,
    /// 0..1
    pub opacity: f32,
    /// Percent of output long edge (0..20).
    pub margin_pct: f32,
}

// ── Batch types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BatchItem {
    pub source_path: PathBuf,
    pub edit: EditState,
    /// File stem of source, used for the `{name}` token.
    pub original_name: String,
}

#[derive(Debug, Clone)]
pub struct BatchOptions {
    pub format: ExportFormat,
    pub resize: ExportResize,
    pub output_dir: PathBuf,
    /// May contain `{name}`, `{date}`, `{ext}` tokens.
    pub name_pattern: String,
    /// Single-layer watermark stamp (back-compat; used when `watermark_preset` is None).
    pub watermark: Option<WatermarkStamp>,
    /// Multi-layer watermark preset. Takes priority over `watermark` when Some.
    pub watermark_preset: Option<chalkraw_core::WatermarkPreset>,
}

#[derive(Debug)]
pub struct BatchItemResult {
    pub source_path: PathBuf,
    pub output_path: Option<PathBuf>,
    pub error: Option<String>,
}

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("render error: {0}")]
    Render(#[from] chalkraw_render::RenderError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("image encode error: {0}")]
    ImageEncode(#[from] image::ImageError),
    #[error("decode error: {0}")]
    Decode(#[from] chalkraw_io::IoError),
}

// ── Single-photo export ───────────────────────────────────────────────────────

/// Render the current photo with the given edits and save to `output_path`.
pub fn export_current(
    rd: &RenderDevice,
    image: &LinearImage,
    edit: &EditState,
    output_path: &Path,
    options: ExportOptions,
) -> Result<(), ExportError> {
    let (out_w, out_h) = compute_output_size(image.width, image.height, options.resize);

    let source = SourceTexture::upload(rd, image.width, image.height, &image.pixels);
    let pipeline = DevelopPipeline::new(rd, PipelineConfig {
        output_format: wgpu::TextureFormat::Rgba8UnormSrgb,
    });
    pipeline.update_uniforms(&EditUniforms::from(edit));

    // Run the Phase 2E blur passes and bilateral NR. These mirror what the UI canvas does
    // on every edit change. Skipping them causes Clarity, Texture, Sharpening, NR, and
    // Dehaze to contribute nothing to the exported file.
    let blur = BlurPipeline::new(rd);
    let bilat = BilateralPipeline::new(rd);
    let (_, clarity_a, _, clarity_b) = create_pingpong(rd, image.width, image.height);
    let (_, sharp_a, _, sharp_b) = create_pingpong(rd, image.width, image.height);
    let (_, texture_a, _, texture_b) = create_pingpong(rd, image.width, image.height);
    let (_, _nr_a, _, nr_b) = create_pingpong(rd, image.width, image.height);

    let sharp_sigma = edit.detail.sharpening.radius.max(0.5);
    blur.render_pass(&source.view, &clarity_a,  image.width, image.height, true,  16.0);
    blur.render_pass(&clarity_a,   &clarity_b,  image.width, image.height, false, 16.0);
    blur.render_pass(&source.view, &sharp_a,    image.width, image.height, true,  sharp_sigma);
    blur.render_pass(&sharp_a,     &sharp_b,    image.width, image.height, false, sharp_sigma);
    blur.render_pass(&source.view, &texture_a,  image.width, image.height, true,  5.0);
    blur.render_pass(&texture_a,   &texture_b,  image.width, image.height, false, 5.0);
    let nr_amount = (edit.detail.noise_reduction.luminance + edit.detail.noise_reduction.color) / 2.0;
    let sigma_range = 0.01 + (nr_amount / 100.0) * 0.2;
    bilat.render_pass(&source.view, &nr_b, 2.0, sigma_range, 3.0);

    let bind_group = pipeline.make_bind_group(&source, &clarity_b, &sharp_b, &texture_b, &nr_b);

    let (target, view) = make_target(rd, out_w, out_h);
    pipeline.render(&view, &bind_group);
    let pixels_rgba = read_to_cpu(rd, &target, out_w, out_h)?;

    let pixels_rgb = strip_alpha(&pixels_rgba);
    encode_and_write(output_path, &pixels_rgb, out_w, out_h, options.format)?;
    Ok(())
}

// ── Batch export ──────────────────────────────────────────────────────────────

/// Export a list of photos. `on_progress` is called once per photo with the
/// 1-based index, total count, and source name BEFORE that photo starts
/// processing. Returns per-item results. Failures do not abort the batch.
pub fn export_batch(
    rd: &RenderDevice,
    items: &[BatchItem],
    opts: &BatchOptions,
    mut on_progress: impl FnMut(usize, usize, &str),
) -> Vec<BatchItemResult> {
    let mut results = Vec::with_capacity(items.len());
    let total = items.len();
    for (idx, item) in items.iter().enumerate() {
        on_progress(idx + 1, total, &item.original_name);
        let output_path =
            compose_output_path(&opts.output_dir, &opts.name_pattern, &item.original_name, opts.format);
        let result = export_single_item(rd, item, opts, &output_path);
        match result {
            Ok(()) => results.push(BatchItemResult {
                source_path: item.source_path.clone(),
                output_path: Some(output_path),
                error: None,
            }),
            Err(e) => results.push(BatchItemResult {
                source_path: item.source_path.clone(),
                output_path: None,
                error: Some(e.to_string()),
            }),
        }
    }
    results
}

fn compose_output_path(dir: &Path, pattern: &str, name: &str, format: ExportFormat) -> PathBuf {
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let ext = match format {
        ExportFormat::Jpeg { .. } => "jpg",
        ExportFormat::Png => "png",
        ExportFormat::Tiff => "tiff",
    };
    let mut rendered = pattern
        .replace("{name}", name)
        .replace("{date}", &date)
        .replace("{ext}", ext);
    if !rendered.ends_with(&format!(".{ext}")) {
        rendered.push('.');
        rendered.push_str(ext);
    }
    dir.join(rendered)
}

fn export_single_item(
    rd: &RenderDevice,
    item: &BatchItem,
    opts: &BatchOptions,
    output_path: &Path,
) -> Result<(), ExportError> {
    let image = chalkraw_io::decode_image(&item.source_path)?;
    let (out_w, out_h) = compute_output_size(image.width, image.height, opts.resize);

    let source = SourceTexture::upload(rd, image.width, image.height, &image.pixels);
    let pipeline = DevelopPipeline::new(rd, PipelineConfig {
        output_format: wgpu::TextureFormat::Rgba8UnormSrgb,
    });
    pipeline.update_uniforms(&EditUniforms::from(&item.edit));

    // Run Phase 2E blur passes and bilateral NR so Clarity, Texture, Sharpening, NR,
    // and Dehaze are correctly applied in the exported file. See export_current for rationale.
    let blur = BlurPipeline::new(rd);
    let bilat = BilateralPipeline::new(rd);
    let (_, clarity_a, _, clarity_b) = create_pingpong(rd, image.width, image.height);
    let (_, sharp_a, _, sharp_b) = create_pingpong(rd, image.width, image.height);
    let (_, texture_a, _, texture_b) = create_pingpong(rd, image.width, image.height);
    let (_, _nr_a, _, nr_b) = create_pingpong(rd, image.width, image.height);

    let sharp_sigma = item.edit.detail.sharpening.radius.max(0.5);
    blur.render_pass(&source.view, &clarity_a,  image.width, image.height, true,  16.0);
    blur.render_pass(&clarity_a,   &clarity_b,  image.width, image.height, false, 16.0);
    blur.render_pass(&source.view, &sharp_a,    image.width, image.height, true,  sharp_sigma);
    blur.render_pass(&sharp_a,     &sharp_b,    image.width, image.height, false, sharp_sigma);
    blur.render_pass(&source.view, &texture_a,  image.width, image.height, true,  5.0);
    blur.render_pass(&texture_a,   &texture_b,  image.width, image.height, false, 5.0);
    let nr_amount = (item.edit.detail.noise_reduction.luminance + item.edit.detail.noise_reduction.color) / 2.0;
    let sigma_range = 0.01 + (nr_amount / 100.0) * 0.2;
    bilat.render_pass(&source.view, &nr_b, 2.0, sigma_range, 3.0);

    let bind_group = pipeline.make_bind_group(&source, &clarity_b, &sharp_b, &texture_b, &nr_b);

    let (target, view) = make_target(rd, out_w, out_h);
    pipeline.render(&view, &bind_group);
    let mut pixels_rgba = read_to_cpu(rd, &target, out_w, out_h)?;

    if let Some(ref preset) = opts.watermark_preset {
        if let Err(e) = apply_watermark_preset(&mut pixels_rgba, out_w, out_h, preset) {
            log::warn!("watermark preset failed, exporting without stamp: {e}");
        }
    } else if let Some(ref wm) = opts.watermark {
        // Non-fatal: if the stamp fails, log and continue without it.
        if let Err(e) = apply_watermark(&mut pixels_rgba, out_w, out_h, wm) {
            log::warn!("watermark failed, exporting without stamp: {e}");
        }
    }

    let pixels_rgb = strip_alpha(&pixels_rgba);
    encode_and_write(output_path, &pixels_rgb, out_w, out_h, opts.format)?;
    Ok(())
}

/// Composite each layer of a `WatermarkPreset` onto `base_rgba` in order.
/// Rotation is in the data model but not applied (same compromise as Phase 5A
/// for image layers). Phase 5 polish will add rotation support.
pub fn apply_watermark_preset(
    base_rgba: &mut [u8],
    base_w: u32,
    base_h: u32,
    preset: &chalkraw_core::WatermarkPreset,
) -> Result<(), ExportError> {
    for layer in &preset.layers {
        match layer {
            chalkraw_core::WatermarkLayer::Image(img_layer) => {
                let stamp = WatermarkStamp {
                    png_path: img_layer.png_path.clone(),
                    anchor: map_anchor(img_layer.anchor),
                    size_pct: img_layer.size_pct,
                    opacity: img_layer.opacity,
                    margin_pct: img_layer.margin_pct,
                };
                apply_watermark(base_rgba, base_w, base_h, &stamp)?;
            }
            chalkraw_core::WatermarkLayer::Text(text_layer) => {
                apply_text_layer(base_rgba, base_w, base_h, text_layer)?;
            }
        }
    }
    Ok(())
}

fn apply_text_layer(
    base_rgba: &mut [u8],
    base_w: u32,
    base_h: u32,
    layer: &chalkraw_core::TextLayer,
) -> Result<(), ExportError> {
    let long_edge = base_w.max(base_h) as f32;
    let px_size = (layer.font_size_pct / 100.0 * long_edge).max(8.0);
    let color = [layer.color.r, layer.color.g, layer.color.b, layer.color.a];
    let text_img = match crate::text::rasterise_text(&layer.text, px_size, color) {
        Some(img) => img,
        None => return Ok(()), // empty text or font failure — silently skip
    };
    let (new_w, new_h) = text_img.dimensions();
    let margin = (layer.margin_pct / 100.0 * long_edge).round() as i64;
    let (anchor_x, anchor_y) = match layer.anchor {
        chalkraw_core::WatermarkAnchor::TopLeft => (margin, margin),
        chalkraw_core::WatermarkAnchor::TopCenter => {
            ((base_w as i64 - new_w as i64) / 2, margin)
        }
        chalkraw_core::WatermarkAnchor::TopRight => {
            (base_w as i64 - new_w as i64 - margin, margin)
        }
        chalkraw_core::WatermarkAnchor::CenterLeft => {
            (margin, (base_h as i64 - new_h as i64) / 2)
        }
        chalkraw_core::WatermarkAnchor::Center => {
            ((base_w as i64 - new_w as i64) / 2, (base_h as i64 - new_h as i64) / 2)
        }
        chalkraw_core::WatermarkAnchor::CenterRight => {
            (base_w as i64 - new_w as i64 - margin, (base_h as i64 - new_h as i64) / 2)
        }
        chalkraw_core::WatermarkAnchor::BottomLeft => {
            (margin, base_h as i64 - new_h as i64 - margin)
        }
        chalkraw_core::WatermarkAnchor::BottomCenter => {
            ((base_w as i64 - new_w as i64) / 2, base_h as i64 - new_h as i64 - margin)
        }
        chalkraw_core::WatermarkAnchor::BottomRight => {
            (base_w as i64 - new_w as i64 - margin, base_h as i64 - new_h as i64 - margin)
        }
    };
    let global_alpha = layer.opacity.clamp(0.0, 1.0);
    for wy in 0..new_h {
        for wx in 0..new_w {
            let bx = anchor_x + wx as i64;
            let by = anchor_y + wy as i64;
            if bx < 0 || by < 0 || bx >= base_w as i64 || by >= base_h as i64 {
                continue;
            }
            let text_px = text_img.get_pixel(wx, wy);
            let a = (text_px[3] as f32 / 255.0) * global_alpha;
            if a <= 0.0 {
                continue;
            }
            let base_idx = ((by as u32 * base_w + bx as u32) * 4) as usize;
            for c in 0..3 {
                let dst = base_rgba[base_idx + c] as f32;
                let src = text_px[c] as f32;
                base_rgba[base_idx + c] =
                    (dst * (1.0 - a) + src * a).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(())
}

fn map_anchor(a: chalkraw_core::WatermarkAnchor) -> WatermarkAnchor {
    use chalkraw_core::WatermarkAnchor as Core;
    match a {
        Core::TopLeft => WatermarkAnchor::TopLeft,
        Core::TopCenter => WatermarkAnchor::TopCenter,
        Core::TopRight => WatermarkAnchor::TopRight,
        Core::CenterLeft => WatermarkAnchor::CenterLeft,
        Core::Center => WatermarkAnchor::Center,
        Core::CenterRight => WatermarkAnchor::CenterRight,
        Core::BottomLeft => WatermarkAnchor::BottomLeft,
        Core::BottomCenter => WatermarkAnchor::BottomCenter,
        Core::BottomRight => WatermarkAnchor::BottomRight,
    }
}

fn apply_watermark(
    base_rgba: &mut [u8],
    base_w: u32,
    base_h: u32,
    stamp: &WatermarkStamp,
) -> Result<(), ExportError> {
    let wm_bytes = std::fs::read(&stamp.png_path)?;
    let wm = image::load_from_memory(&wm_bytes)?.to_rgba8();

    let long_edge = base_w.max(base_h) as f32;
    let target_long = (stamp.size_pct / 100.0 * long_edge).max(1.0) as u32;
    let (orig_w, orig_h) = wm.dimensions();
    let scale = target_long as f32 / (orig_w.max(orig_h) as f32);
    let new_w = ((orig_w as f32) * scale).max(1.0) as u32;
    let new_h = ((orig_h as f32) * scale).max(1.0) as u32;
    let resized =
        image::imageops::resize(&wm, new_w, new_h, image::imageops::FilterType::Triangle);

    let margin = (stamp.margin_pct / 100.0 * long_edge).round() as i64;
    let (anchor_x, anchor_y) = match stamp.anchor {
        WatermarkAnchor::TopLeft => (margin, margin),
        WatermarkAnchor::TopCenter => ((base_w as i64 - new_w as i64) / 2, margin),
        WatermarkAnchor::TopRight => (base_w as i64 - new_w as i64 - margin, margin),
        WatermarkAnchor::CenterLeft => (margin, (base_h as i64 - new_h as i64) / 2),
        WatermarkAnchor::Center => {
            ((base_w as i64 - new_w as i64) / 2, (base_h as i64 - new_h as i64) / 2)
        }
        WatermarkAnchor::CenterRight => {
            (base_w as i64 - new_w as i64 - margin, (base_h as i64 - new_h as i64) / 2)
        }
        WatermarkAnchor::BottomLeft => (margin, base_h as i64 - new_h as i64 - margin),
        WatermarkAnchor::BottomCenter => {
            ((base_w as i64 - new_w as i64) / 2, base_h as i64 - new_h as i64 - margin)
        }
        WatermarkAnchor::BottomRight => {
            (base_w as i64 - new_w as i64 - margin, base_h as i64 - new_h as i64 - margin)
        }
    };

    let global_alpha = stamp.opacity.clamp(0.0, 1.0);
    for wy in 0..new_h {
        for wx in 0..new_w {
            let bx = anchor_x + wx as i64;
            let by = anchor_y + wy as i64;
            if bx < 0 || by < 0 || bx >= base_w as i64 || by >= base_h as i64 {
                continue;
            }
            let wm_px = resized.get_pixel(wx, wy);
            let a = (wm_px[3] as f32 / 255.0) * global_alpha;
            if a <= 0.0 {
                continue;
            }
            let base_idx = ((by as u32 * base_w + bx as u32) * 4) as usize;
            for c in 0..3 {
                let dst = base_rgba[base_idx + c] as f32;
                let src = wm_px[c] as f32;
                base_rgba[base_idx + c] =
                    (dst * (1.0 - a) + src * a).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(())
}

// ── Internal helpers ──────────────────────────────────────────────────────────

pub(crate) fn compute_output_size(w: u32, h: u32, resize: ExportResize) -> (u32, u32) {
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

pub(crate) fn strip_alpha(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len() / 4 * 3);
    for chunk in rgba.chunks_exact(4) {
        out.push(chunk[0]);
        out.push(chunk[1]);
        out.push(chunk[2]);
    }
    out
}

pub(crate) fn encode_and_write(
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

// ── Unit tests ────────────────────────────────────────────────────────────────

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

    #[test]
    fn compose_output_path_substitutes_tokens() {
        use std::path::PathBuf;
        let dir = PathBuf::from("/tmp/out");
        let p = compose_output_path(&dir, "{name}_edited", "myphoto", ExportFormat::Jpeg { quality: 80 });
        let name = p.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("myphoto_edited"), "got: {name}");
        assert!(name.ends_with(".jpg"), "got: {name}");
    }

    #[test]
    fn compose_output_path_date_token() {
        let dir = std::path::PathBuf::from("/tmp");
        let p = compose_output_path(&dir, "{name}_{date}", "x", ExportFormat::Png);
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        // date is YYYY-MM-DD
        assert!(name.contains('-'), "expected date in name: {name}");
        assert!(name.ends_with(".png"), "got: {name}");
    }
}
