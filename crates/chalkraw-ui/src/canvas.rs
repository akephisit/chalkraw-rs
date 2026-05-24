use chalkraw_core::EditState;
use chalkraw_io::LinearImage;
use chalkraw_render::{
    create_pingpong, BlurPipeline, DevelopPipeline, EditUniforms, PipelineConfig, RenderDevice,
    SourceTexture,
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
    /// Intermediate ping-pong texture A (horizontal blur output).
    #[allow(dead_code)]
    pub blur_tex_a: ewgpu::Texture,
    pub blur_view_a: ewgpu::TextureView,
    /// Intermediate ping-pong texture B (vertical blur output — final blur result).
    #[allow(dead_code)]
    pub blur_tex_b: ewgpu::Texture,
    pub blur_view_b: ewgpu::TextureView,
    pub bind_group: ewgpu::BindGroup,
}

impl CanvasGpu {
    pub fn new(rd: &RenderDevice, img: &LinearImage, output_format: ewgpu::TextureFormat) -> Self {
        let source = SourceTexture::upload(rd, img.width, img.height, &img.pixels);
        let pipeline = DevelopPipeline::new(rd, PipelineConfig { output_format });
        let blur_pipeline = BlurPipeline::new(rd);
        let (blur_tex_a, blur_view_a, blur_tex_b, blur_view_b) =
            create_pingpong(rd, img.width, img.height);
        // Build the develop bind group pointing at blur_view_b (the final blur result).
        let bind_group = pipeline.make_bind_group(&source, &blur_view_b);
        let me = Self {
            source,
            pipeline,
            blur_pipeline,
            blur_tex_a,
            blur_view_a,
            blur_tex_b,
            blur_view_b,
            bind_group,
        };
        // Run an initial blur at image-load time (sigma=16 px).
        me.run_blur(16.0);
        me
    }

    /// Run the two-pass separable Gaussian blur and store the result in blur_view_b.
    /// Horizontal pass: source → blur_view_a.
    /// Vertical pass:   blur_view_a → blur_view_b.
    pub fn run_blur(&self, sigma: f32) {
        self.blur_pipeline.render_pass(
            &self.source.view,
            &self.blur_view_a,
            self.source.width,
            self.source.height,
            true,
            sigma,
        );
        self.blur_pipeline.render_pass(
            &self.blur_view_a,
            &self.blur_view_b,
            self.source.width,
            self.source.height,
            false,
            sigma,
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
