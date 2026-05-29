use chalkraw_core::{interpolate_curve, CurvePoint, EditState};
use chalkraw_io::LinearImage;
use chalkraw_render::{
    create_pingpong, display_profile, f32_to_f16_bits, BilateralPipeline, BlurPipeline,
    DevelopPipeline, EditUniforms, PipelineConfig, RenderDevice, SourceTexture,
};
use egui::PaintCallbackInfo;
use egui_wgpu::CallbackTrait;
use std::sync::Arc;

// Use `egui_wgpu::wgpu` as the canonical wgpu re-export to guarantee type identity
// with what egui-wgpu itself uses internally (avoids "two versions of crate wgpu"
// errors when the workspace wgpu and egui-wgpu's wgpu happen to differ).
use egui_wgpu::wgpu as ewgpu;

/// GPU-side resources that live for the lifetime of the loaded photo.
pub struct CanvasGpu {
    /// Source texture — kept alive so the bind group's texture view stays valid.
    /// The field is not read directly but its Drop keeps the GPU allocation alive.
    #[allow(dead_code)]
    pub source: SourceTexture,
    pub pipeline: DevelopPipeline,
    pub blur_pipeline: BlurPipeline,
    /// Bilateral filter pipeline — edge-preserving NR (Phase 2E polish).
    pub bilateral_pipeline: BilateralPipeline,
    /// Clarity blur ping-pong — large sigma (~16 px), used for Clarity local-contrast.
    #[allow(dead_code)]
    pub clarity_tex_a: ewgpu::Texture,
    pub clarity_view_a: ewgpu::TextureView,
    #[allow(dead_code)]
    pub clarity_tex_b: ewgpu::Texture,
    pub clarity_view_b: ewgpu::TextureView,
    /// Sharpening blur ping-pong — small sigma (~1-3 px), used for unsharp mask.
    #[allow(dead_code)]
    pub sharp_tex_a: ewgpu::Texture,
    pub sharp_view_a: ewgpu::TextureView,
    #[allow(dead_code)]
    pub sharp_tex_b: ewgpu::Texture,
    pub sharp_view_b: ewgpu::TextureView,
    /// Texture blur ping-pong — mid sigma (~5 px), used for Texture local-contrast.
    #[allow(dead_code)]
    pub texture_tex_a: ewgpu::Texture,
    pub texture_view_a: ewgpu::TextureView,
    #[allow(dead_code)]
    pub texture_tex_b: ewgpu::Texture,
    pub texture_view_b: ewgpu::TextureView,
    /// NR ping-pong — bilateral filter writes directly to nr_view_b (nr_view_a unused).
    #[allow(dead_code)]
    pub nr_tex_a: ewgpu::Texture,
    #[allow(dead_code)]
    pub nr_view_a: ewgpu::TextureView,
    #[allow(dead_code)]
    pub nr_tex_b: ewgpu::Texture,
    pub nr_view_b: ewgpu::TextureView,
    /// Point curve 1D LUT — 256 × R16Float entries. Identity ramp by default.
    #[allow(dead_code)]
    pub tone_curve_lut_tex: ewgpu::Texture,
    #[allow(dead_code)]
    pub tone_curve_lut_view: ewgpu::TextureView,
    /// Display 3D LUT — 32×32×32 Rgba16Float. Identity when no display profile,
    /// sRGB→display mapping when a non-sRGB monitor ICC profile was found.
    #[allow(dead_code)]
    pub display_lut_tex: ewgpu::Texture,
    #[allow(dead_code)]
    pub display_lut_view: ewgpu::TextureView,
    /// True when the display LUT contains a real non-identity mapping.
    #[allow(dead_code)]
    pub display_lut_active: bool,
    pub bind_group: ewgpu::BindGroup,
    /// wgpu queue needed for LUT uploads after initial creation.
    queue: Arc<ewgpu::Queue>,
}

impl CanvasGpu {
    pub fn new(rd: &RenderDevice, img: &LinearImage, output_format: ewgpu::TextureFormat) -> Self {
        let source = SourceTexture::upload(rd, img.width, img.height, &img.pixels);
        let mut pipeline = DevelopPipeline::new(rd, PipelineConfig { output_format });
        let atmos =
            chalkraw_render::source::estimate_atmospheric_light(&img.pixels, img.width, img.height);
        pipeline.set_atmospheric_light(atmos);
        let blur_pipeline = BlurPipeline::new(rd);
        let bilateral_pipeline = BilateralPipeline::new(rd);
        let (clarity_tex_a, clarity_view_a, clarity_tex_b, clarity_view_b) =
            create_pingpong(rd, img.width, img.height);
        let (sharp_tex_a, sharp_view_a, sharp_tex_b, sharp_view_b) =
            create_pingpong(rd, img.width, img.height);
        let (texture_tex_a, texture_view_a, texture_tex_b, texture_view_b) =
            create_pingpong(rd, img.width, img.height);
        // NR uses a single ping-pong texture; the bilateral filter writes to nr_view_b directly.
        let (nr_tex_a, nr_view_a, nr_tex_b, nr_view_b) = create_pingpong(rd, img.width, img.height);

        // Point curve LUT — 256-entry R16Float 1D texture, initialised to identity ramp.
        let tone_curve_lut_tex = rd.device.create_texture(&ewgpu::TextureDescriptor {
            label: Some("tone curve lut"),
            size: ewgpu::Extent3d {
                width: 256,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: ewgpu::TextureDimension::D1,
            format: ewgpu::TextureFormat::R16Float,
            usage: ewgpu::TextureUsages::TEXTURE_BINDING | ewgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let tone_curve_lut_view =
            tone_curve_lut_tex.create_view(&ewgpu::TextureViewDescriptor::default());
        // Upload identity ramp: entry i = i / 255 as f16.
        let identity_lut: Vec<u16> = (0u16..256)
            .map(|i| f32_to_f16_bits(i as f32 / 255.0))
            .collect();
        rd.queue.write_texture(
            ewgpu::TexelCopyTextureInfo {
                texture: &tone_curve_lut_tex,
                mip_level: 0,
                origin: ewgpu::Origin3d::ZERO,
                aspect: ewgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&identity_lut),
            ewgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256 * 2), // 1 channel × 2 bytes (f16)
                rows_per_image: Some(1),
            },
            ewgpu::Extent3d {
                width: 256,
                height: 1,
                depth_or_array_layers: 1,
            },
        );

        // Phase 8 polish: build display 3D LUT from monitor ICC profile (Windows only;
        // macOS/Linux fall back to identity LUT, display_lut_active = false).
        let (display_lut_tex, display_lut_view, display_lut_active) = {
            let maybe_lut = display_profile::read_display_icc_profile()
                .and_then(|icc| display_profile::build_srgb_to_display_lut(&icc));
            match maybe_lut {
                Some(lut) => {
                    // Use egui_wgpu's wgpu type aliases — the RenderDevice's device/queue
                    // are already the same type (they come from eframe's wgpu instance).
                    let (tex, view) = display_profile::upload_lut_3d(rd, &lut);
                    (tex, view, true)
                }
                None => {
                    // Identity LUT: no colour transformation, display_lut_active stays false.
                    let identity = display_profile::build_identity_lut();
                    let (tex, view) = display_profile::upload_lut_3d(rd, &identity);
                    (tex, view, false)
                }
            }
        };

        // Build the develop bind group pointing at the final blur result textures.
        let bind_group = pipeline.make_bind_group(
            &source,
            &clarity_view_b,
            &sharp_view_b,
            &texture_view_b,
            &nr_view_b,
            &tone_curve_lut_view,
            &display_lut_view,
        );
        let queue = rd.queue.clone();
        let mut me = Self {
            source,
            pipeline,
            blur_pipeline,
            bilateral_pipeline,
            clarity_tex_a,
            clarity_view_a,
            clarity_tex_b,
            clarity_view_b,
            sharp_tex_a,
            sharp_view_a,
            sharp_tex_b,
            sharp_view_b,
            texture_tex_a,
            texture_view_a,
            texture_tex_b,
            texture_view_b,
            nr_tex_a,
            nr_view_a,
            nr_tex_b,
            nr_view_b,
            tone_curve_lut_tex,
            tone_curve_lut_view,
            display_lut_tex,
            display_lut_view,
            display_lut_active,
            bind_group,
            queue,
        };
        me.pipeline.set_display_lut_active(display_lut_active);
        // Run initial blurs at image-load time.
        // Clarity sigma=16 px (large); Sharpening default radius=1.0 px (small);
        // Texture sigma=5 px (mid-frequency); NR uses bilateral filter (nr_amount=0 → identity).
        me.run_blurs(16.0, 1.0, 5.0, 0.0);
        me
    }

    /// Run Gaussian blurs for Clarity/Sharpening/Texture plus bilateral NR.
    /// Clarity:    source → clarity_view_a (H) → clarity_view_b (V).
    /// Sharpening: source → sharp_view_a   (H) → sharp_view_b   (V).
    /// Texture:    source → texture_view_a (H) → texture_view_b (V).
    /// NR:         source → nr_view_b (single bilateral pass; nr_view_a unused).
    ///
    /// `nr_amount` is 0..100 (average of luminance and color NR sliders).
    /// Maps to sigma_range: 0.01 (tight/edge-preserving) .. 0.21 (looser) at 100.
    pub fn run_blurs(
        &self,
        clarity_sigma: f32,
        sharp_sigma: f32,
        texture_sigma: f32,
        nr_amount: f32,
    ) {
        // Clarity blur (large sigma).
        self.blur_pipeline.render_pass(
            &self.source.view,
            &self.clarity_view_a,
            self.source.width,
            self.source.height,
            true,
            clarity_sigma,
        );
        self.blur_pipeline.render_pass(
            &self.clarity_view_a,
            &self.clarity_view_b,
            self.source.width,
            self.source.height,
            false,
            clarity_sigma,
        );
        // Sharpening blur (small sigma).
        self.blur_pipeline.render_pass(
            &self.source.view,
            &self.sharp_view_a,
            self.source.width,
            self.source.height,
            true,
            sharp_sigma,
        );
        self.blur_pipeline.render_pass(
            &self.sharp_view_a,
            &self.sharp_view_b,
            self.source.width,
            self.source.height,
            false,
            sharp_sigma,
        );
        // Texture blur (mid sigma).
        self.blur_pipeline.render_pass(
            &self.source.view,
            &self.texture_view_a,
            self.source.width,
            self.source.height,
            true,
            texture_sigma,
        );
        self.blur_pipeline.render_pass(
            &self.texture_view_a,
            &self.texture_view_b,
            self.source.width,
            self.source.height,
            false,
            texture_sigma,
        );
        // NR: single bilateral filter pass. sigma_range scales with the NR amount slider:
        // nr_amount=0 → sigma_range=0.01 (very tight: almost no smoothing across any edge);
        // nr_amount=100 → sigma_range=0.21 (looser: more smoothing of subtle edges).
        // sigma_spatial=2.0 px is fixed (controls the spatial reach of the kernel).
        let sigma_range = 0.01 + (nr_amount / 100.0) * 0.2;
        self.bilateral_pipeline.render_pass(
            &self.source.view,
            &self.nr_view_b,
            2.0,
            sigma_range,
            3.0,
        );
    }

    pub fn update(&self, edit: &EditState) {
        self.pipeline.update_uniforms(&EditUniforms::from(edit));
    }

    /// Re-upload the tone curve LUT from the given control points.
    /// This generates a 256-entry piecewise-linear interpolation and writes it
    /// to the R16Float 1D texture on the GPU.
    pub fn upload_tone_curve(&self, points: &[CurvePoint]) {
        let lut_f16: Vec<u16> = (0u32..256)
            .map(|i| {
                let x = i as f32 / 255.0;
                let y = interpolate_curve(points, x);
                f32_to_f16_bits(y.clamp(0.0, 1.0))
            })
            .collect();
        self.queue.write_texture(
            ewgpu::TexelCopyTextureInfo {
                texture: &self.tone_curve_lut_tex,
                mip_level: 0,
                origin: ewgpu::Origin3d::ZERO,
                aspect: ewgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&lut_f16),
            ewgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256 * 2),
                rows_per_image: Some(1),
            },
            ewgpu::Extent3d {
                width: 256,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }
}

/// One-frame draw callback that the egui-wgpu integration runs inside the
/// current render pass.
///
/// # egui-wgpu 0.33.3 API notes
///
/// `CallbackTrait::paint` takes `&mut wgpu::RenderPass<'static>` — the `'static`
/// lifetime is intentional in this version (the renderer uses `forget_lifetime`
/// internally so callbacks operate on a `'static`-erased pass). The spec draft
/// showed `'_`; the actual trait requires `'static`.
pub struct CanvasCallback {
    pub gpu: Arc<CanvasGpu>,
}

impl CallbackTrait for CanvasCallback {
    fn paint(
        &self,
        _info: PaintCallbackInfo,
        render_pass: &mut ewgpu::RenderPass<'static>,
        _callback_resources: &egui_wgpu::CallbackResources,
    ) {
        render_pass.set_pipeline(&self.gpu.pipeline.pipeline);
        render_pass.set_bind_group(0, &self.gpu.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}
