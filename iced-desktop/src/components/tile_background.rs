use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use deplace_core::structure::Structure;
use deplace_core::theme::Theme;
use iced::Rectangle;
use iced::wgpu;
use iced::widget::shader::{self, Pipeline, Primitive, Viewport};

/// Fixed number of `floating_tile`s this pipeline can draw in a single frame.
/// Shared by every tile instance, so this only needs to cover the busiest screen.
const TILE_CAPACITY: u64 = 64;
const MIN_BLUR_TEXTURE_SIZE: u32 = 4;
const MAX_BLUR_TEXTURE_SIZE: u32 = 960;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BackgroundUniforms {
    time: f32,
    last_changed_time: f32,
    state: f32,
    prev_state: f32,
    resolution: [f32; 2],
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct TileUniforms {
    origin: [f32; 2],
    size: [f32; 2],
    radius: f32,
    border_width: f32,
    resolution: [f32; 2],
    border_color: [f32; 4],
    tint_color: [f32; 4],
}

fn create_blur_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    size: (u32, u32),
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tile_blur_texture"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_composite_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    instance_buffer: &wgpu::Buffer,
    bg_view: &wgpu::TextureView,
    bg_sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("tile_background_bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: instance_buffer,
                    offset: 0,
                    size: NonZeroU64::new(std::mem::size_of::<TileUniforms>() as u64),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(bg_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(bg_sampler),
            },
        ],
    })
}

pub(crate) struct TileBackgroundPipeline {
    format: wgpu::TextureFormat,

    // Renders a tiny copy of the app's animated background shader every frame;
    // sampling it back up with linear filtering is what fakes the blur.
    bg_pipeline: wgpu::RenderPipeline,
    bg_uniform_buffer: wgpu::Buffer,
    bg_uniform_bind_group: wgpu::BindGroup,
    bg_texture: wgpu::Texture,
    bg_view: wgpu::TextureView,
    bg_sampler: wgpu::Sampler,
    bg_size: (u32, u32),

    // Draws each tile: rounded-rect + border, filled with a sample of `bg_view`.
    composite_pipeline: wgpu::RenderPipeline,
    composite_bind_group_layout: wgpu::BindGroupLayout,
    composite_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    stride: u64,

    next_index: u64,
    start: Instant,
}

impl TileBackgroundPipeline {
    fn refresh_blur_source(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
        time: f32,
    ) {
        if size != self.bg_size {
            let (texture, view) = create_blur_texture(device, self.format, size);
            self.bg_texture = texture;
            self.bg_view = view;
            self.bg_size = size;
            self.composite_bind_group = create_composite_bind_group(
                device,
                &self.composite_bind_group_layout,
                &self.instance_buffer,
                &self.bg_view,
                &self.bg_sampler,
            );
        }

        let uniforms = BackgroundUniforms {
            time,
            last_changed_time: 0.0,
            state: 4.0,
            prev_state: 4.0,
            resolution: [size.0 as f32, size.1 as f32],
            _pad: [0.0; 2],
        };
        queue.write_buffer(&self.bg_uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("tile_blur_source_encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("tile_blur_source_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.bg_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.bg_pipeline);
            pass.set_bind_group(0, &self.bg_uniform_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(std::iter::once(encoder.finish()));
    }
}

impl Pipeline for TileBackgroundPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let bg_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tile_blur_source_shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../assets/loading.wgsl"
                ))
                .into(),
            ),
        });

        let bg_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("tile_blur_source_bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let bg_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tile_blur_source_uniforms"),
            size: std::mem::size_of::<BackgroundUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bg_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tile_blur_source_bind_group"),
            layout: &bg_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: bg_uniform_buffer.as_entire_binding(),
            }],
        });

        let bg_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tile_blur_source_pipeline_layout"),
            bind_group_layouts: &[Some(&bg_bind_group_layout)],
            immediate_size: 0,
        });

        let bg_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tile_blur_source_pipeline"),
            layout: Some(&bg_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &bg_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &bg_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let bg_size = (64, 36);
        let (bg_texture, bg_view) = create_blur_texture(device, format, bg_size);
        let bg_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tile_blur_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 0.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        });

        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tile_background_shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../assets/tile_background.wgsl"
                ))
                .into(),
            ),
        });

        let raw_size = std::mem::size_of::<TileUniforms>() as u64;
        let align = device.limits().min_uniform_buffer_offset_alignment as u64;
        let stride = raw_size.div_ceil(align) * align;

        let composite_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("tile_background_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: true,
                            min_binding_size: NonZeroU64::new(raw_size),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tile_background_instances"),
            size: stride * TILE_CAPACITY,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("tile_background_pipeline_layout"),
                bind_group_layouts: &[Some(&composite_bind_group_layout)],
                immediate_size: 0,
            });

        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tile_background_pipeline"),
            layout: Some(&composite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &composite_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &composite_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let composite_bind_group = create_composite_bind_group(
            device,
            &composite_bind_group_layout,
            &instance_buffer,
            &bg_view,
            &bg_sampler,
        );

        Self {
            format,
            bg_pipeline,
            bg_uniform_buffer,
            bg_uniform_bind_group,
            bg_texture,
            bg_view,
            bg_sampler,
            bg_size,
            composite_pipeline,
            composite_bind_group_layout,
            composite_bind_group,
            instance_buffer,
            stride,
            next_index: 0,
            start: Instant::now(),
        }
    }

    fn trim(&mut self) {
        self.next_index = 0;
    }
}

#[derive(Debug)]
pub(crate) struct TilePrimitive {
    radius: f32,
    border_width: f32,
    border_color: [f32; 4],
    tint_color: [f32; 4],
    blur: f32,
    slot: AtomicU32,
}

impl Primitive for TilePrimitive {
    type Pipeline = TileBackgroundPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let scale = viewport.scale_factor();
        let physical_size = viewport.physical_size();

        // Only the first tile prepared this frame needs to actually refresh the
        // shared blur source; every other tile just reads the result.
        if pipeline.next_index == 0 {
            // `loading.wgsl`'s pattern has fine, high-frequency detail (the
            // diagonal stripe wave is ~14 cycles across the screen). Downscaling
            // by the full `blur` factor pushes the render target below that
            // pattern's Nyquist rate, so `fwidth`-based anti-aliasing there blows
            // up and washes everything out to a near-flat average - "blurred"
            // into nothing rather than a soft, still-varying blur. Dividing the
            // knob down keeps the target closer to the pattern's own resolution.
            let downscale = ((self.blur.max(1.0) / 5.0) * scale).max(1.0);
            let tex_w = ((physical_size.width as f32 / downscale).round() as u32)
                .clamp(MIN_BLUR_TEXTURE_SIZE, MAX_BLUR_TEXTURE_SIZE);
            let tex_h = ((physical_size.height as f32 / downscale).round() as u32)
                .clamp(MIN_BLUR_TEXTURE_SIZE, MAX_BLUR_TEXTURE_SIZE);
            let time = pipeline.start.elapsed().as_secs_f32();
            pipeline.refresh_blur_source(device, queue, (tex_w, tex_h), time);
        }

        let index = pipeline.next_index.min(TILE_CAPACITY - 1);
        pipeline.next_index += 1;
        self.slot.store(index as u32, Ordering::Relaxed);

        let uniforms = TileUniforms {
            origin: [bounds.x * scale, bounds.y * scale],
            size: [bounds.width * scale, bounds.height * scale],
            radius: self.radius * scale,
            border_width: self.border_width * scale,
            resolution: [physical_size.width as f32, physical_size.height as f32],
            border_color: self.border_color,
            tint_color: self.tint_color,
        };

        queue.write_buffer(
            &pipeline.instance_buffer,
            index * pipeline.stride,
            bytemuck::bytes_of(&uniforms),
        );
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        let offset = (self.slot.load(Ordering::Relaxed) as u64 * pipeline.stride) as u32;
        render_pass.set_pipeline(&pipeline.composite_pipeline);
        render_pass.set_bind_group(0, &pipeline.composite_bind_group, &[offset]);
        render_pass.draw(0..3, 0..1);
        true
    }
}

/// Fake-blurred background of a [`crate::components::floating_tile`]: rather than
/// blurring whatever actually sits behind the tile, this samples a shared,
/// heavily-downscaled copy of the app's animated background shader, tinted with
/// `theme.background`
pub(crate) struct TileBackground {
    theme: Theme,
    structure: Structure,
}

impl TileBackground {
    pub(crate) fn new(theme: Theme, structure: Structure) -> Self {
        Self { theme, structure }
    }
}

impl<Message> shader::Program<Message> for TileBackground {
    type State = ();
    type Primitive = TilePrimitive;

    fn draw(&self, _state: &(), _cursor: iced::mouse::Cursor, _bounds: Rectangle) -> TilePrimitive {
        let border = self.theme.border;
        let tint = self.theme.background;

        TilePrimitive {
            radius: self.structure.outer_border_radius,
            border_width: self.structure.border_thickness,
            border_color: [border.r, border.g, border.b, border.a],
            tint_color: [tint.r, tint.g, tint.b, tint.a],
            blur: self.theme.blur,
            slot: AtomicU32::new(0),
        }
    }
}
