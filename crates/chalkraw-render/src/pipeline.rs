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
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
}

impl DevelopPipeline {
    pub fn new(rd: &RenderDevice, cfg: PipelineConfig) -> Self {
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
            device: rd.device.clone(),
            queue: rd.queue.clone(),
        }
    }

    pub fn update_uniforms(&self, u: &EditUniforms) {
        self.queue.write_buffer(&self.uniform_buffer, 0, bytes_of(u));
    }

    pub fn make_bind_group(&self, source: &SourceTexture) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("develop bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: self.uniform_buffer.as_entire_binding() },
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
