//! Export pipeline: single-photo and batch export with optional PNG watermark.

pub mod text;

use chalkraw_core::EditState;
use chalkraw_io::LinearImage;
use chalkraw_render::{
    create_pingpong, make_identity_3d_lut, make_target, make_tone_curve_lut, read_to_cpu,
    BilateralPipeline, BlurPipeline, DevelopPipeline, EditUniforms, PipelineConfig, RenderDevice,
    SourceTexture,
};
use std::collections::HashMap;
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
    /// Rotation in degrees. Snapped to nearest 90° increment (v1).
    pub rotation_deg: f32,
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

struct ExportContext {
    pipeline: DevelopPipeline,
    blur: BlurPipeline,
    bilat: BilateralPipeline,
    _display_lut_tex: wgpu::Texture,
    display_lut_view: wgpu::TextureView,
    scratch: Option<ExportScratch>,
    target: Option<ExportTarget>,
    watermark_cache: WatermarkImageCache,
}

impl ExportContext {
    fn new(rd: &RenderDevice) -> Self {
        let pipeline = DevelopPipeline::new(
            rd,
            PipelineConfig {
                output_format: wgpu::TextureFormat::Rgba8UnormSrgb,
            },
        );
        let blur = BlurPipeline::new(rd);
        let bilat = BilateralPipeline::new(rd);
        let (_display_lut_tex, display_lut_view) = make_identity_3d_lut(rd);
        Self {
            pipeline,
            blur,
            bilat,
            _display_lut_tex,
            display_lut_view,
            scratch: None,
            target: None,
            watermark_cache: WatermarkImageCache::default(),
        }
    }

    fn ensure_scratch(&mut self, rd: &RenderDevice, width: u32, height: u32) {
        let matches_size = self
            .scratch
            .as_ref()
            .map(|scratch| scratch.width == width && scratch.height == height)
            .unwrap_or(false);
        if !matches_size {
            self.scratch = Some(ExportScratch::new(rd, width, height));
        }
    }

    fn ensure_target(&mut self, rd: &RenderDevice, width: u32, height: u32) {
        let matches_size = self
            .target
            .as_ref()
            .map(|target| target.width == width && target.height == height)
            .unwrap_or(false);
        if !matches_size {
            self.target = Some(ExportTarget::new(rd, width, height));
        }
    }
}

struct ExportScratch {
    width: u32,
    height: u32,
    _clarity_tex_a: wgpu::Texture,
    clarity_view_a: wgpu::TextureView,
    _clarity_tex_b: wgpu::Texture,
    clarity_view_b: wgpu::TextureView,
    _sharp_tex_a: wgpu::Texture,
    sharp_view_a: wgpu::TextureView,
    _sharp_tex_b: wgpu::Texture,
    sharp_view_b: wgpu::TextureView,
    _texture_tex_a: wgpu::Texture,
    texture_view_a: wgpu::TextureView,
    _texture_tex_b: wgpu::Texture,
    texture_view_b: wgpu::TextureView,
    _nr_tex_a: wgpu::Texture,
    _nr_view_a: wgpu::TextureView,
    _nr_tex_b: wgpu::Texture,
    nr_view_b: wgpu::TextureView,
}

impl ExportScratch {
    fn new(rd: &RenderDevice, width: u32, height: u32) -> Self {
        let (_clarity_tex_a, clarity_view_a, _clarity_tex_b, clarity_view_b) =
            create_pingpong(rd, width, height);
        let (_sharp_tex_a, sharp_view_a, _sharp_tex_b, sharp_view_b) =
            create_pingpong(rd, width, height);
        let (_texture_tex_a, texture_view_a, _texture_tex_b, texture_view_b) =
            create_pingpong(rd, width, height);
        let (_nr_tex_a, _nr_view_a, _nr_tex_b, nr_view_b) = create_pingpong(rd, width, height);

        Self {
            width,
            height,
            _clarity_tex_a,
            clarity_view_a,
            _clarity_tex_b,
            clarity_view_b,
            _sharp_tex_a,
            sharp_view_a,
            _sharp_tex_b,
            sharp_view_b,
            _texture_tex_a,
            texture_view_a,
            _texture_tex_b,
            texture_view_b,
            _nr_tex_a,
            _nr_view_a,
            _nr_tex_b,
            nr_view_b,
        }
    }
}

struct ExportTarget {
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl ExportTarget {
    fn new(rd: &RenderDevice, width: u32, height: u32) -> Self {
        let (texture, view) = make_target(rd, width, height);
        Self {
            width,
            height,
            texture,
            view,
        }
    }
}

#[derive(Default)]
struct WatermarkImageCache {
    images: HashMap<PathBuf, image::RgbaImage>,
}

impl WatermarkImageCache {
    fn get(&mut self, path: &Path) -> Result<&image::RgbaImage, ExportError> {
        let key = path.to_path_buf();
        if !self.images.contains_key(&key) {
            let bytes = std::fs::read(path)?;
            let image = image::load_from_memory(&bytes)?.to_rgba8();
            self.images.insert(key.clone(), image);
        }
        Ok(self
            .images
            .get(&key)
            .expect("watermark image must be cached after insert"))
    }
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
    let mut pipeline = DevelopPipeline::new(
        rd,
        PipelineConfig {
            output_format: wgpu::TextureFormat::Rgba8UnormSrgb,
        },
    );
    pipeline.set_atmospheric_light(chalkraw_render::source::estimate_atmospheric_light(
        &image.pixels,
        image.width,
        image.height,
    ));
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
    blur.render_pass(
        &source.view,
        &clarity_a,
        image.width,
        image.height,
        true,
        16.0,
    );
    blur.render_pass(
        &clarity_a,
        &clarity_b,
        image.width,
        image.height,
        false,
        16.0,
    );
    blur.render_pass(
        &source.view,
        &sharp_a,
        image.width,
        image.height,
        true,
        sharp_sigma,
    );
    blur.render_pass(
        &sharp_a,
        &sharp_b,
        image.width,
        image.height,
        false,
        sharp_sigma,
    );
    blur.render_pass(
        &source.view,
        &texture_a,
        image.width,
        image.height,
        true,
        5.0,
    );
    blur.render_pass(
        &texture_a,
        &texture_b,
        image.width,
        image.height,
        false,
        5.0,
    );
    let nr_amount =
        (edit.detail.noise_reduction.luminance + edit.detail.noise_reduction.color) / 2.0;
    let sigma_range = 0.01 + (nr_amount / 100.0) * 0.2;
    bilat.render_pass(&source.view, &nr_b, 2.0, sigma_range, 3.0);

    // Build the tone-curve LUT from the current edit's point curve.
    // Export always uses an identity display LUT (no monitor ICC transform needed for file export).
    let (_lut_tex, lut_view) = make_tone_curve_lut(rd, &edit.tone_curve.rgb.0);
    let (_dlut_tex, dlut_view) = make_identity_3d_lut(rd);
    let bind_group = pipeline.make_bind_group(
        &source, &clarity_b, &sharp_b, &texture_b, &nr_b, &lut_view, &dlut_view,
    );

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
    let mut ctx = ExportContext::new(rd);
    for (idx, item) in items.iter().enumerate() {
        on_progress(idx + 1, total, &item.original_name);
        let output_path = compose_output_path(
            &opts.output_dir,
            &opts.name_pattern,
            &item.original_name,
            opts.format,
        );
        let result = export_single_item(rd, &mut ctx, item, opts, &output_path);
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
    ctx: &mut ExportContext,
    item: &BatchItem,
    opts: &BatchOptions,
    output_path: &Path,
) -> Result<(), ExportError> {
    let image = chalkraw_io::decode_image(&item.source_path)?;
    let (out_w, out_h) = compute_output_size(image.width, image.height, opts.resize);

    let source = SourceTexture::upload(rd, image.width, image.height, &image.pixels);
    ctx.pipeline
        .set_atmospheric_light(chalkraw_render::source::estimate_atmospheric_light(
            &image.pixels,
            image.width,
            image.height,
        ));
    ctx.pipeline
        .update_uniforms(&EditUniforms::from(&item.edit));

    // Run Phase 2E blur passes and bilateral NR so Clarity, Texture, Sharpening, NR,
    // and Dehaze are correctly applied in the exported file. See export_current for rationale.
    ctx.ensure_scratch(rd, image.width, image.height);
    ctx.ensure_target(rd, out_w, out_h);
    let scratch = ctx
        .scratch
        .as_ref()
        .expect("export scratch must be initialised");

    let sharp_sigma = item.edit.detail.sharpening.radius.max(0.5);
    ctx.blur.render_pass(
        &source.view,
        &scratch.clarity_view_a,
        image.width,
        image.height,
        true,
        16.0,
    );
    ctx.blur.render_pass(
        &scratch.clarity_view_a,
        &scratch.clarity_view_b,
        image.width,
        image.height,
        false,
        16.0,
    );
    ctx.blur.render_pass(
        &source.view,
        &scratch.sharp_view_a,
        image.width,
        image.height,
        true,
        sharp_sigma,
    );
    ctx.blur.render_pass(
        &scratch.sharp_view_a,
        &scratch.sharp_view_b,
        image.width,
        image.height,
        false,
        sharp_sigma,
    );
    ctx.blur.render_pass(
        &source.view,
        &scratch.texture_view_a,
        image.width,
        image.height,
        true,
        5.0,
    );
    ctx.blur.render_pass(
        &scratch.texture_view_a,
        &scratch.texture_view_b,
        image.width,
        image.height,
        false,
        5.0,
    );
    let nr_amount =
        (item.edit.detail.noise_reduction.luminance + item.edit.detail.noise_reduction.color) / 2.0;
    let sigma_range = 0.01 + (nr_amount / 100.0) * 0.2;
    ctx.bilat
        .render_pass(&source.view, &scratch.nr_view_b, 2.0, sigma_range, 3.0);

    // Build the tone-curve LUT from the current edit's point curve.
    // Export always uses an identity display LUT (no monitor ICC transform needed for file export).
    let (_lut_tex, lut_view) = make_tone_curve_lut(rd, &item.edit.tone_curve.rgb.0);
    let bind_group = ctx.pipeline.make_bind_group(
        &source,
        &scratch.clarity_view_b,
        &scratch.sharp_view_b,
        &scratch.texture_view_b,
        &scratch.nr_view_b,
        &lut_view,
        &ctx.display_lut_view,
    );

    let target = ctx
        .target
        .as_ref()
        .expect("export target must be initialised");
    ctx.pipeline.render(&target.view, &bind_group);
    let mut pixels_rgba = read_to_cpu(rd, &target.texture, out_w, out_h)?;

    if let Some(ref preset) = opts.watermark_preset {
        if let Err(e) = apply_watermark_preset_with_cache(
            &mut pixels_rgba,
            out_w,
            out_h,
            preset,
            &mut ctx.watermark_cache,
        ) {
            log::warn!("watermark preset failed, exporting without stamp: {e}");
        }
    } else if let Some(ref wm) = opts.watermark {
        // Non-fatal: if the stamp fails, log and continue without it.
        if let Err(e) =
            apply_watermark_with_cache(&mut pixels_rgba, out_w, out_h, wm, &mut ctx.watermark_cache)
        {
            log::warn!("watermark failed, exporting without stamp: {e}");
        }
    }

    let pixels_rgb = strip_alpha(&pixels_rgba);
    encode_and_write(output_path, &pixels_rgb, out_w, out_h, opts.format)?;
    Ok(())
}

/// Composite each layer of a `WatermarkPreset` onto `base_rgba` in order.
/// Rotation is applied by snapping to the nearest 90° increment (v1).
pub fn apply_watermark_preset(
    base_rgba: &mut [u8],
    base_w: u32,
    base_h: u32,
    preset: &chalkraw_core::WatermarkPreset,
) -> Result<(), ExportError> {
    let mut cache = WatermarkImageCache::default();
    apply_watermark_preset_with_cache(base_rgba, base_w, base_h, preset, &mut cache)
}

fn apply_watermark_preset_with_cache(
    base_rgba: &mut [u8],
    base_w: u32,
    base_h: u32,
    preset: &chalkraw_core::WatermarkPreset,
    cache: &mut WatermarkImageCache,
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
                    rotation_deg: img_layer.rotation_deg,
                };
                apply_watermark_with_cache(base_rgba, base_w, base_h, &stamp, cache)?;
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
    // Apply rotation BEFORE positioning; re-read dimensions in case 90°/270° swaps them.
    let text_img = if layer.rotation_deg.abs() > 0.1 {
        rotate_image(&text_img, layer.rotation_deg)
    } else {
        text_img
    };
    let (new_w, new_h) = text_img.dimensions();
    let margin = (layer.margin_pct / 100.0 * long_edge).round() as i64;
    let (anchor_x, anchor_y) = match layer.anchor {
        chalkraw_core::WatermarkAnchor::TopLeft => (margin, margin),
        chalkraw_core::WatermarkAnchor::TopCenter => ((base_w as i64 - new_w as i64) / 2, margin),
        chalkraw_core::WatermarkAnchor::TopRight => (base_w as i64 - new_w as i64 - margin, margin),
        chalkraw_core::WatermarkAnchor::CenterLeft => (margin, (base_h as i64 - new_h as i64) / 2),
        chalkraw_core::WatermarkAnchor::Center => (
            (base_w as i64 - new_w as i64) / 2,
            (base_h as i64 - new_h as i64) / 2,
        ),
        chalkraw_core::WatermarkAnchor::CenterRight => (
            base_w as i64 - new_w as i64 - margin,
            (base_h as i64 - new_h as i64) / 2,
        ),
        chalkraw_core::WatermarkAnchor::BottomLeft => {
            (margin, base_h as i64 - new_h as i64 - margin)
        }
        chalkraw_core::WatermarkAnchor::BottomCenter => (
            (base_w as i64 - new_w as i64) / 2,
            base_h as i64 - new_h as i64 - margin,
        ),
        chalkraw_core::WatermarkAnchor::BottomRight => (
            base_w as i64 - new_w as i64 - margin,
            base_h as i64 - new_h as i64 - margin,
        ),
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

fn apply_watermark_with_cache(
    base_rgba: &mut [u8],
    base_w: u32,
    base_h: u32,
    stamp: &WatermarkStamp,
    cache: &mut WatermarkImageCache,
) -> Result<(), ExportError> {
    let wm = cache.get(&stamp.png_path)?;
    apply_watermark_image(base_rgba, base_w, base_h, stamp, wm)
}

fn apply_watermark_image(
    base_rgba: &mut [u8],
    base_w: u32,
    base_h: u32,
    stamp: &WatermarkStamp,
    wm: &image::RgbaImage,
) -> Result<(), ExportError> {
    let long_edge = base_w.max(base_h) as f32;
    let target_long = (stamp.size_pct / 100.0 * long_edge).max(1.0) as u32;
    let (orig_w, orig_h) = wm.dimensions();
    let scale = target_long as f32 / (orig_w.max(orig_h) as f32);
    let new_w = ((orig_w as f32) * scale).max(1.0) as u32;
    let new_h = ((orig_h as f32) * scale).max(1.0) as u32;
    let resized = image::imageops::resize(wm, new_w, new_h, image::imageops::FilterType::Triangle);

    // Apply rotation BEFORE positioning; re-read dimensions in case 90°/270° swaps them.
    let resized = if stamp.rotation_deg.abs() > 0.1 {
        rotate_image(&resized, stamp.rotation_deg)
    } else {
        resized
    };
    let (new_w, new_h) = resized.dimensions();

    let margin = (stamp.margin_pct / 100.0 * long_edge).round() as i64;
    let (anchor_x, anchor_y) = match stamp.anchor {
        WatermarkAnchor::TopLeft => (margin, margin),
        WatermarkAnchor::TopCenter => ((base_w as i64 - new_w as i64) / 2, margin),
        WatermarkAnchor::TopRight => (base_w as i64 - new_w as i64 - margin, margin),
        WatermarkAnchor::CenterLeft => (margin, (base_h as i64 - new_h as i64) / 2),
        WatermarkAnchor::Center => (
            (base_w as i64 - new_w as i64) / 2,
            (base_h as i64 - new_h as i64) / 2,
        ),
        WatermarkAnchor::CenterRight => (
            base_w as i64 - new_w as i64 - margin,
            (base_h as i64 - new_h as i64) / 2,
        ),
        WatermarkAnchor::BottomLeft => (margin, base_h as i64 - new_h as i64 - margin),
        WatermarkAnchor::BottomCenter => (
            (base_w as i64 - new_w as i64) / 2,
            base_h as i64 - new_h as i64 - margin,
        ),
        WatermarkAnchor::BottomRight => (
            base_w as i64 - new_w as i64 - margin,
            base_h as i64 - new_h as i64 - margin,
        ),
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

// ── Rotation helper ───────────────────────────────────────────────────────────

/// Rotate an RGBA image by an arbitrary angle using bilinear sampling.
///
/// For angles within 0.5° of a 90° multiple the fast lossless `image::imageops`
/// path is taken.  All other angles use a bilinear-sampled affine transform that
/// expands the canvas to the rotated bounding box and leaves transparent pixels
/// where the original had no coverage.
pub fn rotate_image(img: &image::RgbaImage, angle_deg: f32) -> image::RgbaImage {
    if angle_deg.abs() < 0.1 {
        return img.clone();
    }
    let snapped = ((angle_deg / 90.0).round() as i32).rem_euclid(4);
    // Snap to 90° if the requested angle is close enough — avoids unnecessary
    // bilinear sampling for the common axis-aligned case.
    if (angle_deg - (snapped as f32 * 90.0)).abs() < 0.5 {
        return match snapped {
            1 => image::imageops::rotate90(img),
            2 => image::imageops::rotate180(img),
            3 => image::imageops::rotate270(img),
            _ => img.clone(),
        };
    }

    // General case: bilinear sample of rotated coordinates.
    let (w, h) = img.dimensions();
    let theta = angle_deg.to_radians();
    let cos_t = theta.cos();
    let sin_t = theta.sin();

    // Bounding box of the rotated image.
    let (cx, cy) = (w as f32 * 0.5, h as f32 * 0.5);
    let corners = [
        (0.0 - cx, 0.0 - cy),
        (w as f32 - cx, 0.0 - cy),
        (0.0 - cx, h as f32 - cy),
        (w as f32 - cx, h as f32 - cy),
    ];
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for (x, y) in corners {
        let rx = x * cos_t - y * sin_t;
        let ry = x * sin_t + y * cos_t;
        if rx < min_x {
            min_x = rx;
        }
        if ry < min_y {
            min_y = ry;
        }
        if rx > max_x {
            max_x = rx;
        }
        if ry > max_y {
            max_y = ry;
        }
    }
    let out_w = (max_x - min_x).ceil() as u32;
    let out_h = (max_y - min_y).ceil() as u32;
    let mut out = image::ImageBuffer::from_pixel(out_w, out_h, image::Rgba([0, 0, 0, 0]));

    for oy in 0..out_h {
        for ox in 0..out_w {
            // Map output (ox, oy) back to source coordinates by inverse rotation.
            let cx2 = ox as f32 + min_x;
            let cy2 = oy as f32 + min_y;
            let sx = cx2 * cos_t + cy2 * sin_t + cx;
            let sy = -cx2 * sin_t + cy2 * cos_t + cy;
            if sx < 0.0 || sy < 0.0 || sx >= (w - 1) as f32 || sy >= (h - 1) as f32 {
                continue;
            }
            // Bilinear sample.
            let x0 = sx.floor() as u32;
            let y0 = sy.floor() as u32;
            let x1 = x0 + 1;
            let y1 = y0 + 1;
            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;
            let p00 = img.get_pixel(x0, y0);
            let p10 = img.get_pixel(x1, y0);
            let p01 = img.get_pixel(x0, y1);
            let p11 = img.get_pixel(x1, y1);
            let mut blended = [0.0_f32; 4];
            for c in 0..4 {
                let top = p00[c] as f32 * (1.0 - fx) + p10[c] as f32 * fx;
                let bot = p01[c] as f32 * (1.0 - fx) + p11[c] as f32 * fx;
                blended[c] = top * (1.0 - fy) + bot * fy;
            }
            out.put_pixel(
                ox,
                oy,
                image::Rgba([
                    blended[0] as u8,
                    blended[1] as u8,
                    blended[2] as u8,
                    blended[3] as u8,
                ]),
            );
        }
    }
    out
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
        assert_eq!(
            compute_output_size(1024, 768, ExportResize::Original),
            (1024, 768)
        );
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
        let p = compose_output_path(
            &dir,
            "{name}_edited",
            "myphoto",
            ExportFormat::Jpeg { quality: 80 },
        );
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

    #[test]
    fn rotate_image_45_produces_larger_bounding_box() {
        let img = image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(
            10,
            10,
            image::Rgba([255, 255, 255, 255]),
        );
        let rotated = super::rotate_image(&img, 45.0);
        let (w, h) = rotated.dimensions();
        // sqrt(2) * 10 ≈ 14.14
        assert!((14..=16).contains(&w));
        assert!((14..=16).contains(&h));
    }
}
