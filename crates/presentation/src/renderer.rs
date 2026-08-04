//! wgpu renderer. One instanced pipeline, flat circles.
//!
//! Phase 3.5 is explicitly a throwaway: no SDF text, no terrain, no post
//! effects. The one thing done properly is the *shape* of the pipeline —
//! instances uploaded as a packed buffer, one draw call for every unit on the
//! map — because that is the part Phase 9L builds on rather than replaces.
//!
//! Circles are drawn as quads with the fragment shader discarding outside the
//! radius. That gives a resolution-independent edge for free, which is the
//! same trick the SDF pipeline will use at Phase 9 with a real distance field.

use std::sync::Arc;

use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::render_state::Instance;

const SHADER: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) colour: vec4<f32>,
};

@vertex
fn vs(
    @builtin(vertex_index) vi: u32,
    @location(0) ndc: vec2<f32>,
    @location(1) half_extent: vec2<f32>,
    @location(2) colour: vec4<f32>,
) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>( 1.0, -1.0), vec2<f32>( 1.0,  1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>( 1.0,  1.0), vec2<f32>(-1.0,  1.0),
    );
    let c = corners[vi];
    var out: VsOut;
    out.pos = vec4<f32>(ndc + c * half_extent, 0.0, 1.0);
    out.local = c;
    out.colour = colour;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let d = length(in.local);
    if (d > 1.0) {
        discard;
    }
    // Feathered edge. Cheap anti-aliasing that also makes overlapping units
    // readable as separate shapes rather than one blob.
    let a = 1.0 - smoothstep(0.82, 1.0, d);
    return vec4<f32>(in.colour.rgb, in.colour.a * a);
}
"#;

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    instance_buf: wgpu::Buffer,
    instance_cap: usize,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Renderer {
        pollster::block_on(Renderer::new_async(window))
    }

    async fn new_async(window: Arc<Window>) -> Renderer {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .expect("no suitable GPU adapter");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("trilateral device"),
                // Defaults deliberately: PRD §4 targets a 2015 integrated GPU,
                // so anything requiring more than the baseline needs a reason.
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                ..Default::default()
            })
            .await
            .expect("request device");

        // Ask the surface for a configuration it already considers valid rather
        // than assembling one field by field. wgpu gains fields between major
        // versions (30 added colour-space handling), and a hand-built config is
        // a compile error every upgrade for no benefit.
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("surface is not supported by this adapter");
        // Prefer an sRGB target so colours are not washed out.
        let caps = surface.get_capabilities(&adapter);
        if let Some(srgb) = caps.formats.iter().copied().find(|f| f.is_srgb()) {
            config.format = srgb;
        }
        let format = config.format;
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("circle shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("circle layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("circle pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2, 1 => Float32x2, 2 => Float32x4
                    ],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let instance_cap = 4096;
        let instance_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("instances"),
            contents: bytemuck::cast_slice(&vec![
                Instance {
                    ndc: [0.0, 0.0],
                    half: [0.0, 0.0],
                    colour: [0.0; 4],
                };
                instance_cap
            ]),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });

        Renderer {
            surface,
            device,
            queue,
            config,
            pipeline,
            instance_buf,
            instance_cap,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        // A minimised window reports zero, which is an invalid surface size.
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn render(&mut self, instances: &[Instance]) {
        if instances.len() > self.instance_cap {
            // Grow rather than truncate. Silently dropping units would make a
            // capacity bug look like a gameplay bug.
            self.instance_cap = instances.len().next_power_of_two();
            self.instance_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instances"),
                size: (self.instance_cap * std::mem::size_of::<Instance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !instances.is_empty() {
            self.queue
                .write_buffer(&self.instance_buf, 0, bytemuck::cast_slice(instances));
        }

        // wgpu 30 reports acquisition outcomes as an enum rather than a Result.
        // Every non-Success case is transient — a resize, a display change, a
        // minimised window — so reconfigure and skip one frame rather than
        // crash the game because the user dragged the window to another screen.
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                self.surface.configure(&self.device, &self.config);
                f
            }
            _ => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.055,
                            g: 0.06,
                            b: 0.075,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, self.instance_buf.slice(..));
            // One draw call for every unit on the map — the property Phase 9L
            // keeps when the shapes become SDFs.
            pass.draw(0..6, 0..instances.len() as u32);
        }
        self.queue.submit(Some(encoder.finish()));
        // wgpu 30 moved presentation onto the queue.
        self.queue.present(frame);
    }
}
