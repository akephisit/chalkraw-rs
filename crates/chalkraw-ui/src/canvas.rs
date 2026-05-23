use chalkraw_core::EditState;
use chalkraw_io::LinearImage;
use chalkraw_render::{DevelopPipeline, EditUniforms, PipelineConfig, RenderDevice, SourceTexture};
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
    pub bind_group: ewgpu::BindGroup,
}

impl CanvasGpu {
    pub fn new(rd: &RenderDevice, img: &LinearImage, output_format: ewgpu::TextureFormat) -> Self {
        let source = SourceTexture::upload(rd, img.width, img.height, &img.pixels);
        let pipeline = DevelopPipeline::new(rd, PipelineConfig { output_format });
        let bind_group = pipeline.make_bind_group(&source);
        Self { source, pipeline, bind_group }
    }

    pub fn update(&self, edit: &EditState) {
        self.pipeline.update_uniforms(&EditUniforms::from(edit));
    }
}

/// One-frame draw callback that the egui-wgpu integration runs inside the
/// current render pass.
///
/// # egui-wgpu 0.34 API notes
///
/// `CallbackTrait::paint` takes `&mut wgpu::RenderPass<'static>` — the `'static`
/// lifetime is intentional (the renderer uses `forget_lifetime` internally so
/// callbacks operate on a `'static`-erased pass).
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
