use crate::device::RenderDevice;
use bytemuck::{Pod, Zeroable};
use std::sync::Arc;

/// Uniform layout for bilateral.wgsl — must mirror the WGSL struct byte-for-byte.
/// Total: 2+1+1+1+3 floats = 8 floats = 32 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct BilateralUniforms {
    /// Unused (full 2D pass); kept so the layout matches BlurUniforms.
    pub direction: [f32; 2], // offset  0
    pub sigma_spatial: f32, // offset  8 — spatial Gaussian sigma in pixels
    pub sigma_range: f32,   // offset 12 — range Gaussian sigma in linear RGB units
    pub radius: f32,        // offset 16 — half-window size (3 = 7×7)
    pub _pad: [f32; 3],     // offset 20 — pad to 32 bytes
}

/// GPU bilateral-filter pipeline (single 2D pass, not separable).
pub struct BilateralPipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    pub uniform_buffer: wgpu::Buffer,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
}

impl BilateralPipeline {
    pub fn new(rd: &RenderDevice) -> Self {
        let shader = rd
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("bilateral.wgsl"),
                source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/bilateral.wgsl").into()),
            });

        let bgl = rd
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("bilateral bgl"),
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
                            min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<
                                BilateralUniforms,
                            >()
                                as u64),
                        },
                        count: None,
                    },
                ],
            });

        let layout = rd
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("bilateral pl"),
                bind_group_layouts: &[&bgl],
                push_constant_ranges: &[],
            });

        let pipeline = rd
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("bilateral pipeline"),
                layout: Some(&layout),
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
                        format: wgpu::TextureFormat::Rgba16Float,
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
            label: Some("bilateral sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniform_buffer = rd.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bilateral uniforms"),
            size: std::mem::size_of::<BilateralUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            bind_group_layout: bgl,
            sampler,
            uniform_buffer,
            device: rd.device.clone(),
            queue: rd.queue.clone(),
        }
    }

    /// Run a single bilateral filter pass from `input_view` into `output_view`.
    /// Both must be Rgba16Float views. `sigma_spatial` and `sigma_range` are in
    /// linear-light units; `radius` is the half-window size (3 = 7×7 window).
    pub fn render_pass(
        &self,
        input_view: &wgpu::TextureView,
        output_view: &wgpu::TextureView,
        sigma_spatial: f32,
        sigma_range: f32,
        radius: f32,
    ) {
        let u = BilateralUniforms {
            direction: [0.0; 2],
            sigma_spatial,
            sigma_range,
            radius,
            _pad: [0.0; 3],
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&u));

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bilateral bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("bilateral encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bilateral pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
    }
}
