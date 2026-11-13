use crate::video::Frame;
use iced_wgpu::primitive::{Pipeline, Primitive};
use iced_wgpu::wgpu;
use std::{
    collections::{BTreeMap, btree_map::Entry},
    num::NonZero,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

#[repr(C)]
struct Uniforms {
    rect: [f32; 4],
    /// Corner radiusies (top left, top right, bottom right, bottom left) of the
    /// drawn quad, in physical pixels
    radius: [f32; 4],
    /// Size of the drawn quad (width, height, paddings), in pixels,
    /// the shader works out each fragment's pixel position from this and
    /// its uv for the rounded corners.
    size: [f32; 4],
    // because wgpu min_uniform_buffer_offset_alignment
    _pad: [u8; 208],
}

/// Slots in each video's uniform ring buffer. Preparing wraps around this
/// many uniforms, so a video would have to be prepared this many times
/// between draws before its data starts being overwritten.
const MAX_UNIFORM_SLOTS: usize = 256;

struct VideoEntry {
    texture_y: wgpu::Texture,
    texture_uv: wgpu::Texture,
    instances: wgpu::Buffer,
    bg0: wgpu::BindGroup,
    alive: Arc<AtomicBool>,

    prepare_index: AtomicUsize,
}

pub(crate) struct VideoPipeline {
    pipeline: wgpu::RenderPipeline,
    bg0_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    videos: BTreeMap<u64, VideoEntry>,
}

impl Pipeline for VideoPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("iced_video_player shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let bg0_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("iced_video_player bind group 0 layout"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    // Fragment too, since the fragment shader reads the corner
                    // radii and size for the rounded-corner clip.
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("iced_video_player pipeline layout"),
            bind_group_layouts: &[Some(&bg0_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("iced_video_player pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Alpha blending so the shader's rounded corners can fade
                    // out to transparency instead of clipping abruptly.
                    // Let's see if this makes a meaningful difference,
                    // otherwise i can remove it again
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("iced_video_player sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 1.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        });

        VideoPipeline {
            pipeline,
            bg0_layout,
            sampler,
            videos: BTreeMap::new(),
        }
    }

    fn trim(&mut self) {
        let ids: Vec<_> = self
            .videos
            .iter()
            .filter_map(|(id, entry)| (!entry.alive.load(Ordering::SeqCst)).then_some(*id))
            .collect();
        for id in ids {
            if let Some(video) = self.videos.remove(&id) {
                video.texture_y.destroy();
                video.texture_uv.destroy();
                video.instances.destroy();
            }
        }
    }
}

impl VideoPipeline {
    #[allow(clippy::too_many_lines)]
    /// Whether this pipeline already owns textures for the video: every
    /// renderer starts out with an empty `videos`
    /// map, so a video that keeps playing through a window recreation
    ///  has to be uploaded once into the new
    /// pipeline to become visible again.
    fn needs_upload(&self, video_id: u64) -> bool {
        !self.videos.contains_key(&video_id)
    }

    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        video_id: u64,
        alive: &Arc<AtomicBool>,
        (width, height): (u32, u32),
        frame: &[u8],
        stride: Option<u32>,
    ) {
        // Use stride from GStreamer's VideoMeta if available, otherwise assume stride == width
        let stride = stride.unwrap_or(width);
        if let Entry::Vacant(entry) = self.videos.entry(video_id) {
            let texture_y = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("iced_video_player texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });

            let texture_uv = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("iced_video_player texture"),
                size: wgpu::Extent3d {
                    width: width / 2,
                    height: height / 2,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rg8Unorm,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });

            let view_y = texture_y.create_view(&wgpu::TextureViewDescriptor {
                label: Some("iced_video_player texture view"),
                format: None,
                dimension: None,
                aspect: wgpu::TextureAspect::All,
                base_mip_level: 0,
                mip_level_count: None,
                base_array_layer: 0,
                array_layer_count: None,
                usage: None,
            });

            let view_uv = texture_uv.create_view(&wgpu::TextureViewDescriptor {
                label: Some("iced_video_player texture view"),
                format: None,
                dimension: None,
                aspect: wgpu::TextureAspect::All,
                base_mip_level: 0,
                mip_level_count: None,
                base_array_layer: 0,
                array_layer_count: None,
                usage: None,
            });

            let instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("iced_video_player uniform buffer"),
                size: MAX_UNIFORM_SLOTS as u64 * std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
                mapped_at_creation: false,
            });

            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iced_video_player bind group"),
                layout: &self.bg0_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view_y),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view_uv),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &instances,
                            offset: 0,
                            size: Some(NonZero::new(std::mem::size_of::<Uniforms>() as _).unwrap()),
                        }),
                    },
                ],
            });

            entry.insert(VideoEntry {
                texture_y,
                texture_uv,
                instances,
                bg0: bind_group,
                alive: Arc::clone(alive),

                prepare_index: AtomicUsize::new(0),
            });
        }

        let VideoEntry {
            texture_y,
            texture_uv,
            ..
        } = self.videos.get(&video_id).unwrap();

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: texture_y,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame[..(stride * height) as usize],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: texture_uv,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame[(stride * height) as usize..],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(height / 2),
            },
            wgpu::Extent3d {
                width: width / 2,
                height: height / 2,
                depth_or_array_layers: 1,
            },
        );
    }

    fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        video_id: u64,
        bounds: &iced::Rectangle,
        radius: &[f32; 4],
        size: [f32; 2],
    ) {
        if let Some(video) = self.videos.get_mut(&video_id) {
            let uniforms = Uniforms {
                rect: [
                    bounds.x,
                    bounds.y,
                    bounds.x + bounds.width,
                    bounds.y + bounds.height,
                ],
                radius: *radius,
                size: [size[0], size[1], 0.0, 0.0],
                _pad: [0; 208],
            };

            // `prepare_index` counts prepares monotonically and the write
            // wraps around the uniform ring buffer: prepares can significantly
            // outnumber draws (e.g. while the window is unfocused widgets are
            // still prepared, but nothing is drawn), and without the wrap the
            // offset would eventually run past the end of the 256-slot buffer,
            // which wgpu treats as a fatal validation error.
            let slot = video.prepare_index.fetch_add(1, Ordering::Relaxed) % MAX_UNIFORM_SLOTS;
            queue.write_buffer(
                &video.instances,
                (slot * std::mem::size_of::<Uniforms>()) as u64,
                unsafe {
                    std::slice::from_raw_parts(
                        &uniforms as *const _ as *const u8,
                        std::mem::size_of::<Uniforms>(),
                    )
                },
            );
        }
    }

    fn draw(
        &self,
        target: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
        clip: &iced::Rectangle<u32>,
        video_id: u64,
    ) {
        if let Some(video) = self.videos.get(&video_id) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("iced_video_player render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&self.pipeline);
            // Read the most recently prepared uniform: like prepares, draws
            // can run in any proportion to prepares (e.g. after focusing the
            // window again), so the draw tracks the prepare count instead of
            // keeping its own unbounded counter.
            let prepared = video.prepare_index.load(Ordering::Relaxed);
            let slot = if prepared == 0 {
                0
            } else {
                (prepared - 1) % MAX_UNIFORM_SLOTS
            };
            pass.set_bind_group(
                0,
                &video.bg0,
                &[(slot * std::mem::size_of::<Uniforms>()) as u32],
            );
            pass.set_scissor_rect(clip.x as _, clip.y as _, clip.width as _, clip.height as _);
            pass.draw(0..6, 0..1);
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct VideoPrimitive {
    video_id: u64,
    alive: Arc<AtomicBool>,
    frame: Arc<Mutex<Frame>>,
    size: (u32, u32),
    upload_frame: bool,
    radius: [f32; 4],
}

impl VideoPrimitive {
    pub fn new(
        video_id: u64,
        alive: Arc<AtomicBool>,
        frame: Arc<Mutex<Frame>>,
        size: (u32, u32),
        upload_frame: bool,
        radius: [f32; 4],
    ) -> Self {
        VideoPrimitive {
            video_id,
            alive,
            frame,
            size,
            upload_frame,
            radius,
        }
    }
}

impl Primitive for VideoPrimitive {
    type Pipeline = VideoPipeline;

    fn prepare(
        &self,
        pipeline: &mut VideoPipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &iced::Rectangle,
        viewport: &iced_wgpu::graphics::Viewport,
    ) {
        // Upload a freshly decoded frame, but also the stored current one
        // when this pipeline has no textures for the video yet: a paused
        // video produces no new samples, so without this a recreated
        // window would never show its frame again.
        if self.upload_frame || pipeline.needs_upload(self.video_id) {
            let frame_guard = self.frame.lock().expect("lock frame mutex");
            let stride = frame_guard.stride();
            if let Some(readable) = frame_guard.readable() {
                pipeline.upload(
                    device,
                    queue,
                    self.video_id,
                    &self.alive,
                    self.size,
                    readable.as_slice(),
                    stride,
                );
            };
        }

        pipeline.prepare(
            queue,
            self.video_id,
            &(*bounds
                * iced::Transformation::orthographic(
                    viewport.logical_size().width as _,
                    viewport.logical_size().height as _,
                )),
            &self.radius,
            // In pixels, unlike the rectangle above, the shader
            // works out the rounded corner distances from the quad's pixel
            // size.
            [bounds.width, bounds.height],
        );
    }

    fn render(
        &self,
        pipeline: &Self::Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &iced::Rectangle<u32>,
    ) {
        pipeline.draw(target, encoder, clip_bounds, self.video_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Transformation;

    /// Renders the shader offscreen with a synthetic frame and reads the
    /// pixels back, so the rounded-corner clip and output color can be
    /// verified without a window or a real video.
    #[test]
    fn shader_renders_frame_with_rounded_corners() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("request adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("request device");

        let mut pipeline = VideoPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);

        // 64x64 synthetic frame: bright neutral luma and neutral chroma.
        let (width, height) = (64u32, 64u32);
        let mut frame = vec![200u8; (width * height) as usize];
        frame.extend(vec![128u8; ((width / 2) * (height / 2) * 2) as usize]);
        pipeline.upload(
            &device,
            &queue,
            1,
            &Arc::new(AtomicBool::new(true)),
            (width, height),
            &frame,
            None,
        );

        // Same mapping VideoPrimitive::prepare applies to the widget bounds:
        // NDC rectangle plus the logical pixel size for the corner clip.
        let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(64.0, 64.0));
        let ndc = bounds * Transformation::orthographic(64u32, 64u32);
        pipeline.prepare(&queue, 1, &ndc, &[16.0; 4], [64.0, 64.0]);

        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("iced_video_player test target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("iced_video_player test readback"),
            // bytes_per_row must be a multiple of 256: 64 * 4 == 256.
            size: (height * 256) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("iced_video_player test encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("iced_video_player test pass"),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.create_view(&wgpu::TextureViewDescriptor::default()),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Red so untouched pixels are easy to tell apart
                        // from anything the shader produces.
                        load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
            });
            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &pipeline.videos[&1].bg0, &[0]);
            pass.set_scissor_rect(0, 0, width, height);
            pass.draw(0..6, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let _ = queue.submit([encoder.finish()]);
        instance.poll_all(true);

        let (sender, receiver) = std::sync::mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |result| {
            sender.send(result).expect("send map result");
        });
        instance.poll_all(true);
        receiver.recv().expect("map result").expect("map");

        let mapped = readback.get_mapped_range(..).expect("get mapped range");
        let pixel = |x: u32, y: u32| -> [u8; 4] {
            let offset = (y * 256 + x * 4) as usize;
            [
                mapped[offset],
                mapped[offset + 1],
                mapped[offset + 2],
                mapped[offset + 3],
            ]
        };
        for (x, y) in [
            (0, 0),
            (2, 2),
            (12, 12),
            (16, 16),
            (32, 32),
            (60, 60),
            (63, 63),
        ] {
            println!("pixel ({x},{y}) = {:?}", pixel(x, y));
        }
        // Center pixel: fully opaque video, and the raw texture color is

        // Center pixel: fully opaque video, and the raw texture color is
        // around 200/255 luma neutral gray, so all channels should be close.
        let center = pixel(32, 32);
        assert_eq!(center[3], 255, "center must be opaque, got {center:?}");
        for channel in center {
            assert!(
                (150..=255).contains(&channel),
                "center should be a neutral gray from the synthetic frame, got {center:?}"
            );
        }

        // Corner pixel (2, 2) is outside the radius-16 rounded corner and
        // must show the untouched clear color instead of the video.
        let corner = pixel(2, 2);
        assert_eq!(
            corner,
            [255, 0, 0, 255],
            "corner must be clipped, got {corner:?}"
        );

        // Inside the corner circle radius near the corner the video shows.
        let inset = pixel(12, 12);
        assert_eq!(
            inset[3], 255,
            "inset near corner must be opaque, got {inset:?}"
        );
    }
}
