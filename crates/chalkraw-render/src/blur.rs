use crate::device::RenderDevice;
use bytemuck::{Pod, Zeroable};
use std::sync::Arc;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct BlurUniforms {
    pub direction: [f32; 2],
    pub sigma: f32,
    pub radius: f32,
    pub _pad: [f32; 2],
}

pub struct BlurPipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    pub uniform_buffer: wgpu::Buffer,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
}

impl BlurPipeline {
    pub fn new(rd: &RenderDevice) -> Self {
        let shader = rd.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gaussian_blur.wgsl"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/gaussian_blur.wgsl").into(),
            ),
        });

        let bgl = rd.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur bgl"),
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
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<BlurUniforms>() as u64,
                        ),
                    },
                    count: None,
                },
            ],
        });

        let layout = rd.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blur pl"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });

        let pipeline = rd.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blur pipeline"),
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
            label: Some("blur sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniform_buffer = rd.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("blur uniforms"),
            size: std::mem::size_of::<BlurUniforms>() as u64,
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

    /// Run one separable blur pass (horizontal OR vertical) from `input_view`
    /// into `output_view`. Both must be Rgba16Float views of the same
    /// width/height. Sigma is in pixels.
    pub fn render_pass(
        &self,
        input_view: &wgpu::TextureView,
        output_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        horizontal: bool,
        sigma: f32,
    ) {
        let direction = if horizontal {
            [1.0 / width as f32, 0.0]
        } else {
            [0.0, 1.0 / height as f32]
        };
        let radius = (3.0 * sigma).ceil().max(1.0);
        let u = BlurUniforms { direction, sigma, radius, _pad: [0.0; 2] };
        self.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&u));

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blur bg"),
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

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("blur encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blur pass"),
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

/// Allocate two Rgba16Float ping-pong textures of the given dimensions.
/// Returns (tex_a, view_a, tex_b, view_b).
pub fn create_pingpong(
    rd: &RenderDevice,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView, wgpu::Texture, wgpu::TextureView) {
    let desc = wgpu::TextureDescriptor {
        label: Some("pingpong"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    };
    let a = rd.device.create_texture(&desc);
    let b = rd.device.create_texture(&desc);
    let va = a.create_view(&Default::default());
    let vb = b.create_view(&Default::default());
    (a, va, b, vb)
}
