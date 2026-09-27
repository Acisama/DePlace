use iced::Event;
use iced::Rectangle;
use iced::wgpu;
use iced::widget::Action;
use iced::widget::shader::{self, Pipeline, Primitive, Viewport};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    time: f32,
    last_changed_time: f32,
    state: f32,
    prev_state: f32,
    resolution: [f32; 2],
    _pad: [f32; 2], // keeps the buffer a multiple of the vec2's 8-byte alignment
}

pub(crate) struct LoadingPipeline {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl Pipeline for LoadingPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("loading_shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/loading.wgsl")).into(),
            ),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("loading_bind_group_layout"),
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

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("loading_uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("loading_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("loading_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("loading_pipeline"),
            layout: Some(&pipeline_layout),
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

        Self {
            pipeline,
            uniform_buffer,
            bind_group,
        }
    }
}

#[derive(Debug)]
pub(crate) struct LoadingPrimitive {
    uniforms: Uniforms,
}

impl Primitive for LoadingPrimitive {
    type Pipeline = LoadingPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let physical_size = viewport.physical_size();
        let uniforms = Uniforms {
            resolution: [physical_size.width as f32, physical_size.height as f32],
            ..self.uniforms
        };

        queue.write_buffer(&pipeline.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        render_pass.set_pipeline(&pipeline.pipeline);
        render_pass.set_bind_group(0, &pipeline.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
        true
    }
}

/// State needed to animate the loading indicator's shader.
///
/// `state`/`prev_state`/`last_changed` are meant to track [`crate::components::Screen`]
/// transitions (0 = Loading, 1 = Discovery, 2 = Login, 3 = Home); update them
/// whenever the active screen changes so the shader eases between them.
#[derive(Clone, Copy, Default)]
pub(crate) struct LoadingIndicator {
    last_changed: f32,
    state: f32,
    prev_state: f32,
}

impl LoadingIndicator {
    /// Eases the shader towards `state`. No-ops if already at `state`, so this can be called on every
    /// `Root::update` without resetting the transition clock each time.
    pub(crate) fn transition_to(&mut self, state: f32) {
        if state == self.state {
            return;
        }

        self.prev_state = self.state;
        self.state = state;
        self.last_changed = super::animation_clock::elapsed_seconds();
    }
}

impl<Message> shader::Program<Message> for LoadingIndicator {
    type State = ();
    type Primitive = LoadingPrimitive;

    fn update(
        &self,
        _state: &mut Self::State,
        _event: &Event,
        _bounds: Rectangle,
        _cursor: iced::advanced::mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        Some(Action::<Message>::request_redraw())
    }

    fn draw(
        &self,
        _state: &(),
        _cursor: iced::mouse::Cursor,
        bounds: Rectangle,
    ) -> LoadingPrimitive {
        LoadingPrimitive {
            uniforms: Uniforms {
                time: super::animation_clock::elapsed_seconds(),
                last_changed_time: self.last_changed,
                state: self.state,
                prev_state: self.prev_state,
                // Logical-pixel placeholder; `LoadingPrimitive::prepare` overwrites
                // this with the physical size from the `Viewport` before drawing.
                resolution: [bounds.width, bounds.height],
                _pad: [0.0; 2],
            },
        }
    }
}
