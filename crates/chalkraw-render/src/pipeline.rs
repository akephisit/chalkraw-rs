use crate::device::RenderDevice;
use crate::source::SourceTexture;
use crate::uniforms::EditUniforms;
use bytemuck::bytes_of;
use std::sync::Arc;

/// Output texture format. The UI uses `Bgra8UnormSrgb` (matches egui-wgpu's
/// surface); offscreen tests use `Rgba8UnormSrgb`.
#[derive(Debug, Clone, Copy)]
pub struct PipelineConfig {
    pub output_format: wgpu::TextureFormat,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self { output_format: wgpu::TextureFormat::Rgba8UnormSrgb }
    }
}

pub struct DevelopPipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    pub uniform_buffer: wgpu::Buffer,
    /// True when the output format is NOT sRGB-coded (e.g. Bgra8Unorm).
    /// The fragment shader reads this flag and applies a manual IEC 61966-2-1
    /// linear→sRGB encode at the end of fs_main.  sRGB-coded formats
    /// (Bgra8UnormSrgb, Rgba8UnormSrgb) and linear-float formats
    /// (Rgba16Float, Rgba32Float) do not need it.
    pub manual_srgb_needed: bool,
    /// Per-image atmospheric light for DCP Dehaze, estimated once at source
    /// upload from the top 0.1% brightest dark-channel pixels.
    pub atmospheric_light: [f32; 3],
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
}

impl DevelopPipeline {
    pub fn new(rd: &RenderDevice, cfg: PipelineConfig) -> Self {
        // Determine whether the fragment shader must manually apply sRGB encoding.
        // Float formats (Rgba16Float, Rgba32Float) are linear targets used in
        // off-screen export — no encode needed there either.
        let manual_srgb_needed = !cfg.output_format.is_srgb()
            && !matches!(
                cfg.output_format,
                wgpu::TextureFormat::Rgba16Float | wgpu::TextureFormat::Rgba32Float
            );

        let shader = rd.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("develop.wgsl"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/develop.wgsl").into(),
            ),
        });

        let bind_group_layout = rd.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("develop bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<EditUniforms>() as u64),
                    },
                    count: None,
                },
                // Phase 2E.1: large-sigma pre-blurred source for Clarity local-contrast.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Phase 2E.2: small-sigma pre-blurred source for Sharpening (unsharp mask).
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Phase 2E.3: mid-sigma pre-blurred source for Texture local-contrast.
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Phase 2E.4: small-sigma pre-blurred source for Noise Reduction (sigma=2px).
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Phase 2D polish: 256-entry R16Float 1D LUT for the point curve.
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D1,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = rd.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("develop pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = rd.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("develop pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: cfg.output_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = rd.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("develop sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniform_buffer = rd.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("develop uniforms"),
            size: std::mem::size_of::<EditUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            bind_group_layout,
            sampler,
            uniform_buffer,
            manual_srgb_needed,
            // Default to white until the caller sets a per-image estimate.
            atmospheric_light: [0.95, 0.95, 0.95],
            device: rd.device.clone(),
            queue: rd.queue.clone(),
        }
    }

    /// Override the atmospheric light used by the Dehaze shader block.
    /// Call once per source upload with the result of
    /// `chalkraw_render::source::estimate_atmospheric_light`.
    pub fn set_atmospheric_light(&mut self, atmos: [f32; 3]) {
        self.atmospheric_light = atmos;
    }

    pub fn update_uniforms(&self, u: &EditUniforms) {
        // Patch in the sRGB flag that depends on the pipeline's output format,
        // not on the edit state, then write the whole struct to the GPU buffer.
        let mut copy = *u;
        copy.srgb_output = if self.manual_srgb_needed { 1 } else { 0 };
        copy._pad_srgb = [0; 3];
        copy.atmospheric_light = [
            self.atmospheric_light[0],
            self.atmospheric_light[1],
            self.atmospheric_light[2],
            0.0,
        ];
        self.queue.write_buffer(&self.uniform_buffer, 0, bytes_of(&copy));
    }

    /// Build a bind group for the develop pipeline.
    /// `clarity_blur_view` should be an Rgba16Float view of the large-sigma
    /// pre-blurred source (same dimensions as `source`).
    /// `sharp_blur_view` should be an Rgba16Float view of the small-sigma
    /// pre-blurred source.
    /// `texture_blur_view` should be an Rgba16Float view of the mid-sigma
    /// pre-blurred source (sigma ≈ 5 px) for Texture local-contrast.
    /// `nr_blur_view` should be an Rgba16Float view of the NR blur (sigma=2px).
    /// `tone_curve_lut_view` should be an R16Float 1D texture view (256 entries)
    /// for the point-curve LUT.
    /// Pass `&source.view` for any arg in callers that do not exercise those
    /// effects — with the relevant slider at 0 the term is zero.
    pub fn make_bind_group(
        &self,
        source: &SourceTexture,
        clarity_blur_view: &wgpu::TextureView,
        sharp_blur_view: &wgpu::TextureView,
        texture_blur_view: &wgpu::TextureView,
        nr_blur_view: &wgpu::TextureView,
        tone_curve_lut_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("develop bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: self.uniform_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(clarity_blur_view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(sharp_blur_view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(texture_blur_view) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(nr_blur_view) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(tone_curve_lut_view) },
            ],
        })
    }

    pub fn render(&self, target: &wgpu::TextureView, bind_group: &wgpu::BindGroup) {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("develop encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("develop pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
    }
}
