use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU32, Ordering};

use deplace_core::structure::Structure;
use deplace_core::theme::Theme;
use iced::Rectangle;
use iced::wgpu;
use iced::widget::shader::{self, Pipeline, Primitive, Viewport};

use super::animation_clock;

/// Fixed number of `floating_tile`s this pipeline can draw in a single frame.
/// Shared by every tile instance, so this only needs to cover the busiest screen.
const TILE_CAPACITY: u64 = 64;

/// The animated background is rendered at this fraction of the window's
/// physical resolution before being blurred. High enough to avoid aliasing in
/// the source pattern, low enough to keep three extra render passes cheap.
const SCENE_DOWNSCALE: f32 = 2.0;
const MIN_SCENE_SIZE: u32 = 64;
const MAX_SCENE_SIZE: u32 = 2048;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SceneUniforms {
    time: f32,
    last_changed_time: f32,
    state: f32,
    prev_state: f32,
    resolution: [f32; 2],
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurUniforms {
    texel_size: [f32; 2],
    direction: [f32; 2],
    radius: f32,
    _pad: f32,
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

fn create_render_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    size: (u32, u32),
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tile_blur_render_texture"),
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

fn create_blur_pass_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform_buffer: &wgpu::Buffer,
    source_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("tile_blur_pass_bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(source_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn create_composite_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    instance_buffer: &wgpu::Buffer,
    bg_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
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
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn create_fullscreen_pipeline(
    device: &wgpu::Device,
    label: &str,
    shader_source: &str,
    bind_group_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(shader_source.into()),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(bind_group_layout)],
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
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
    })
}

fn color_attachment(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
        },
    }
}

pub(crate) struct TileBackgroundPipeline {
    format: wgpu::TextureFormat,
    sampler: wgpu::Sampler,
    scene_size: (u32, u32),

    // Renders a downscaled copy of the app's animated background shader.
    scene_pipeline: wgpu::RenderPipeline,
    scene_uniform_buffer: wgpu::Buffer,
    scene_bind_group: wgpu::BindGroup,
    scene_texture: wgpu::Texture,
    scene_view: wgpu::TextureView,

    // Two-pass separable Gaussian blur: horizontal (scene -> tmp), then
    // vertical (tmp -> scene, overwriting the now-unneeded raw render).
    // `scene_view` ends up holding the final blurred result either way.
    blur_pipeline: wgpu::RenderPipeline,
    blur_bind_group_layout: wgpu::BindGroupLayout,
    blur_tmp_texture: wgpu::Texture,
    blur_tmp_view: wgpu::TextureView,
    blur_h_uniform_buffer: wgpu::Buffer,
    blur_h_bind_group: wgpu::BindGroup,
    blur_v_uniform_buffer: wgpu::Buffer,
    blur_v_bind_group: wgpu::BindGroup,

    // Draws each tile: rounded-rect + border, filled with a sample of the
    // blurred `scene_view`.
    composite_pipeline: wgpu::RenderPipeline,
    composite_bind_group_layout: wgpu::BindGroupLayout,
    composite_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    stride: u64,

    next_index: u64,
}

impl TileBackgroundPipeline {
    fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if size == self.scene_size {
            return;
        }

        let (scene_texture, scene_view) = create_render_texture(device, self.format, size);
        let (blur_tmp_texture, blur_tmp_view) = create_render_texture(device, self.format, size);

        self.blur_h_bind_group = create_blur_pass_bind_group(
            device,
            &self.blur_bind_group_layout,
            &self.blur_h_uniform_buffer,
            &scene_view,
            &self.sampler,
        );
        self.blur_v_bind_group = create_blur_pass_bind_group(
            device,
            &self.blur_bind_group_layout,
            &self.blur_v_uniform_buffer,
            &blur_tmp_view,
            &self.sampler,
        );
        self.composite_bind_group = create_composite_bind_group(
            device,
            &self.composite_bind_group_layout,
            &self.instance_buffer,
            &scene_view,
            &self.sampler,
        );

        self.scene_texture = scene_texture;
        self.scene_view = scene_view;
        self.blur_tmp_texture = blur_tmp_texture;
        self.blur_tmp_view = blur_tmp_view;
        self.scene_size = size;
    }

    fn render_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
        radius_texels: f32,
        time: f32,
    ) {
        self.resize(device, size);

        let (state, prev_state, last_changed_time) = animation_clock::screen_state();
        let scene_uniforms = SceneUniforms {
            time,
            last_changed_time,
            state,
            prev_state,
            resolution: [size.0 as f32, size.1 as f32],
            _pad: [0.0; 2],
        };
        queue.write_buffer(
            &self.scene_uniform_buffer,
            0,
            bytemuck::bytes_of(&scene_uniforms),
        );

        let texel_size = [1.0 / size.0 as f32, 1.0 / size.1 as f32];
        queue.write_buffer(
            &self.blur_h_uniform_buffer,
            0,
            bytemuck::bytes_of(&BlurUniforms {
                texel_size,
                direction: [1.0, 0.0],
                radius: radius_texels,
                _pad: 0.0,
            }),
        );
        queue.write_buffer(
            &self.blur_v_uniform_buffer,
            0,
            bytemuck::bytes_of(&BlurUniforms {
                texel_size,
                direction: [0.0, 1.0],
                radius: radius_texels,
                _pad: 0.0,
            }),
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("tile_blur_encoder"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("tile_blur_scene_pass"),
                color_attachments: &[Some(color_attachment(&self.scene_view))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.scene_pipeline);
            pass.set_bind_group(0, &self.scene_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("tile_blur_horizontal_pass"),
                color_attachments: &[Some(color_attachment(&self.blur_tmp_view))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.blur_pipeline);
            pass.set_bind_group(0, &self.blur_h_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("tile_blur_vertical_pass"),
                color_attachments: &[Some(color_attachment(&self.scene_view))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.blur_pipeline);
            pass.set_bind_group(0, &self.blur_v_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        queue.submit(std::iter::once(encoder.finish()));
    }
}

impl Pipeline for TileBackgroundPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let scene_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("tile_blur_scene_bind_group_layout"),
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

        let scene_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tile_blur_scene_uniforms"),
            size: std::mem::size_of::<SceneUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let scene_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tile_blur_scene_bind_group"),
            layout: &scene_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: scene_uniform_buffer.as_entire_binding(),
            }],
        });

        let scene_pipeline = create_fullscreen_pipeline(
            device,
            "tile_blur_scene_pipeline",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../assets/loading.wgsl"
            )),
            &scene_bind_group_layout,
            format,
            Some(wgpu::BlendState::REPLACE),
        );

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
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

        let blur_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("tile_blur_pass_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
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

        let blur_pipeline = create_fullscreen_pipeline(
            device,
            "tile_blur_pass_pipeline",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../assets/blur_pass.wgsl"
            )),
            &blur_bind_group_layout,
            format,
            Some(wgpu::BlendState::REPLACE),
        );

        let blur_h_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tile_blur_h_uniforms"),
            size: std::mem::size_of::<BlurUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blur_v_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tile_blur_v_uniforms"),
            size: std::mem::size_of::<BlurUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let scene_size = (64, 64);
        let (scene_texture, scene_view) = create_render_texture(device, format, scene_size);
        let (blur_tmp_texture, blur_tmp_view) = create_render_texture(device, format, scene_size);

        let blur_h_bind_group = create_blur_pass_bind_group(
            device,
            &blur_bind_group_layout,
            &blur_h_uniform_buffer,
            &scene_view,
            &sampler,
        );
        let blur_v_bind_group = create_blur_pass_bind_group(
            device,
            &blur_bind_group_layout,
            &blur_v_uniform_buffer,
            &blur_tmp_view,
            &sampler,
        );

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

        let composite_pipeline = create_fullscreen_pipeline(
            device,
            "tile_background_pipeline",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../assets/tile_background.wgsl"
            )),
            &composite_bind_group_layout,
            format,
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );

        let composite_bind_group = create_composite_bind_group(
            device,
            &composite_bind_group_layout,
            &instance_buffer,
            &scene_view,
            &sampler,
        );

        Self {
            format,
            sampler,
            scene_size,
            scene_pipeline,
            scene_uniform_buffer,
            scene_bind_group,
            scene_texture,
            scene_view,
            blur_pipeline,
            blur_bind_group_layout,
            blur_tmp_texture,
            blur_tmp_view,
            blur_h_uniform_buffer,
            blur_h_bind_group,
            blur_v_uniform_buffer,
            blur_v_bind_group,
            composite_pipeline,
            composite_bind_group_layout,
            composite_bind_group,
            instance_buffer,
            stride,
            next_index: 0,
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
            let scene_w = ((physical_size.width as f32 / SCENE_DOWNSCALE).round() as u32)
                .clamp(MIN_SCENE_SIZE, MAX_SCENE_SIZE);
            let scene_h = ((physical_size.height as f32 / SCENE_DOWNSCALE).round() as u32)
                .clamp(MIN_SCENE_SIZE, MAX_SCENE_SIZE);
            // `blur` is specified in logical screen pixels; convert to texels
            // of the (downscaled) scene texture.
            let radius_texels = (self.blur.max(1.0) * scale) / SCENE_DOWNSCALE;
            let time = animation_clock::elapsed_seconds();
            pipeline.render_frame(device, queue, (scene_w, scene_h), radius_texels, time);
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
/// blurred copy of the app's animated background shader, tinted with
/// `theme.background`.
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
