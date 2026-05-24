use chalkraw_core::EditState;
use chalkraw_io::LinearImage;
use chalkraw_render::{
    create_pingpong, BilateralPipeline, BlurPipeline, DevelopPipeline, EditUniforms,
    PipelineConfig, RenderDevice, SourceTexture,
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
    pub bind_group: ewgpu::BindGroup,
}

impl CanvasGpu {
    pub fn new(rd: &RenderDevice, img: &LinearImage, output_format: ewgpu::TextureFormat) -> Self {
        let source = SourceTexture::upload(rd, img.width, img.height, &img.pixels);
        let pipeline = DevelopPipeline::new(rd, PipelineConfig { output_format });
        let blur_pipeline = BlurPipeline::new(rd);
        let bilateral_pipeline = BilateralPipeline::new(rd);
        let (clarity_tex_a, clarity_view_a, clarity_tex_b, clarity_view_b) =
            create_pingpong(rd, img.width, img.height);
        let (sharp_tex_a, sharp_view_a, sharp_tex_b, sharp_view_b) =
            create_pingpong(rd, img.width, img.height);
        let (texture_tex_a, texture_view_a, texture_tex_b, texture_view_b) =
            create_pingpong(rd, img.width, img.height);
        // NR uses a single ping-pong texture; the bilateral filter writes to nr_view_b directly.
        let (nr_tex_a, nr_view_a, nr_tex_b, nr_view_b) =
            create_pingpong(rd, img.width, img.height);
        // Build the develop bind group pointing at the final blur result textures.
        let bind_group = pipeline.make_bind_group(&source, &clarity_view_b, &sharp_view_b, &texture_view_b, &nr_view_b);
        let me = Self {
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
            bind_group,
        };
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
    pub fn run_blurs(&self, clarity_sigma: f32, sharp_sigma: f32, texture_sigma: f32, nr_amount: f32) {
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
