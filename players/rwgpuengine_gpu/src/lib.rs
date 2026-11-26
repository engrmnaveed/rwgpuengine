//! `rwgpuengine_gpu` — a render list drawn with wgpu, for players.
//!
//! The agent's half of the engine never needs a GPU: the CPU reference
//! renderer (`rwgpuengine_render::reference`) is the source of truth for
//! every visual check. This crate is the players' half. It draws the same
//! [`RenderList`] by the same rules, fast, and `cargo xtask gpu` holds its
//! picture to the reference's (ADR 0021).
//!
//! There is no window here and nothing blocks. A [`Renderer`] is handed a
//! ready `wgpu::Device` and `wgpu::Queue`:
//!
//! ```text
//! let mut renderer = Renderer::new(&device, Options::default());
//! renderer.draw(&device, &queue, &list);            // into its own frame
//! renderer.present(&device, &queue, &view, format, None, false);   // a window or a canvas
//! let copy = renderer.copy_frame(&device, &queue);  // or the bytes, for a check
//! ```
//!
//! The frame is the list's size (`list.view`) in RGBA8 with no sRGB
//! encoding, so its bytes are the reference's bytes. [`Renderer::present`]
//! scales it onto whatever the player shows it on.

mod pack;

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use rwgpuengine_render::{Instance, Rect, RenderList};

use pack::{GLOBALS_BYTES, Glass, INSTANCE_BYTES, SHAPE_BYTES};

/// The exact wgpu this crate is written against.
pub use wgpu;

const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// 32-bit float depth: the renderer writes exact bit patterns into it.
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// How see-through colors are mixed with what is behind them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// In the fragment shader, with the reference's integer arithmetic, from
    /// a copy of the picture so far. Things that overlap need a pass each.
    Exact,
    /// By the GPU's blender. One pass, but some adapters round differently.
    Hardware,
}

/// Where the 8-bit color of a face under the light comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceColor {
    /// A table filled by the reference's own function; the GPU only selects.
    Table,
    /// Multiplied and rounded in the vertex shader.
    Shader,
}

/// How level faces in one plane are ordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rank {
    /// The plane's depth is computed per pixel and its lowest bits hold the
    /// rank: faces in one plane differ in nothing else.
    DepthBits,
    /// The rasterizer's depth, moved this many depth steps nearer per rank.
    Bias(u32),
}

/// The depth of a card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardDepth {
    /// Upright above its anchor, lying below it: a depth per pixel row.
    TwoPlanes,
    /// The anchor's depth everywhere.
    Anchor,
}

/// How the iso view's depth is stored in the depth buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsoDepth {
    /// Along the float's exponent: the precision is where the scene is.
    Exponent,
    /// A straight line from 1 to 0, which puts the scene at 0.5.
    Line,
}

/// The point positions are measured from before the GPU projects them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The camera's center: small numbers where the picture is.
    Camera,
    World,
}

/// The choices ADR 0021 measured. [`Options::default`] is what was chosen
/// and what players get; the others exist so the measurement can be repeated
/// (`cargo xtask gpu --study`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub blend: Blend,
    pub color: FaceColor,
    pub rank: Rank,
    pub cards: CardDepth,
    pub origin: Origin,
    pub iso_depth: IsoDepth,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            blend: Blend::Exact,
            color: FaceColor::Table,
            rank: Rank::DepthBits,
            cards: CardDepth::TwoPlanes,
            origin: Origin::Camera,
            iso_depth: IsoDepth::Exponent,
        }
    }
}

/// What the last [`Renderer::draw`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Render passes: one, plus one for each time see-through things
    /// overlapped what was drawn since the last copy of the picture.
    pub passes: u32,
    pub draws: u32,
    /// Were the instances sent to the GPU again (they changed)?
    pub uploaded_instances: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pipe {
    Side,
    Level,
    Card,
    Overlay,
    SideGlass,
    LevelGlass,
    CardGlass,
    OverlayGlass,
}

impl Pipe {
    fn glass(self) -> bool {
        matches!(self, Pipe::SideGlass | Pipe::LevelGlass | Pipe::CardGlass | Pipe::OverlayGlass)
    }
}

struct Cmd {
    pipe: Pipe,
    vertices: std::ops::Range<u32>,
    instances: std::ops::Range<u32>,
}

/// A buffer that grows when it has to.
struct Growing {
    buffer: wgpu::Buffer,
    usage: wgpu::BufferUsages,
}

impl Growing {
    fn new(device: &wgpu::Device, usage: wgpu::BufferUsages) -> Growing {
        Growing { buffer: buffer(device, 4096, usage), usage }
    }

    fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) {
        if bytes.len() as u64 > self.buffer.size() {
            self.buffer = buffer(device, (bytes.len() as u64).next_power_of_two(), self.usage);
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytes);
        }
    }
}

fn buffer(device: &wgpu::Device, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage, mapped_at_creation: false })
}

/// The textures a frame is drawn into.
struct Target {
    width: u32,
    height: u32,
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    /// A copy of the picture so far, for see-through colors.
    prev: wgpu::Texture,
    prev_bind: wgpu::BindGroup,
}

/// Draws render lists with wgpu.
pub struct Renderer {
    options: Options,
    /// Each pipeline, and the view it is for if it is for one (sides store
    /// their depth differently in the iso view).
    pipes: Vec<(Pipe, Option<bool>, wgpu::RenderPipeline)>,
    globals: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    prev_layout: wgpu::BindGroupLayout,
    sides: wgpu::Buffer,
    levels: wgpu::Buffer,
    instances: Growing,
    shapes: Growing,
    target: Option<Target>,
    /// The instances on the GPU, and the list they were made from.
    held: Option<(Vec<Instance>, pack::Instances)>,
    blit: Blit,
    stats: Stats,
}

struct Blit {
    module: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    sharp: wgpu::Sampler,
    smooth: wgpu::Sampler,
    pipelines: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
}

impl Renderer {
    /// The device needs no features and no limits beyond wgpu's defaults
    /// for WebGPU; `wgpu::Limits::downlevel_defaults()` is enough.
    pub fn new(device: &wgpu::Device, options: Options) -> Renderer {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rwgpuengine_gpu"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let prev_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let plain = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        let with_prev = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&globals_layout), Some(&prev_layout)],
            immediate_size: 0,
        });

        let corner = [wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint8x4, offset: 0, shader_location: 0 }];
        let instance = [
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 1 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 12, shader_location: 2 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint8x4, offset: 24, shader_location: 3 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint8x4, offset: 28, shader_location: 4 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint8x4, offset: 32, shader_location: 5 },
        ];
        let shape = [
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Sint32x4, offset: 0, shader_location: 0 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Sint32x4, offset: 16, shader_location: 1 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Sint32x4, offset: 32, shader_location: 2 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint8x4, offset: 48, shader_location: 3 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint8x4, offset: 52, shader_location: 4 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 56, shader_location: 5 },
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 72, shader_location: 6 },
        ];
        let mesh_buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: 4,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &corner,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: INSTANCE_BYTES as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &instance,
            }),
        ];
        let shape_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: SHAPE_BYTES as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &shape,
        })];

        let exact = options.blend == Blend::Exact;
        // The GPU's own blender, for the study: color over what is there,
        // and the target stays opaque.
        let hardware = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let make = |pipe: Pipe, vs: &str, fs: &str, fs_x: &str, mesh: bool, in_world: bool| {
            let glass = pipe.glass();
            let fs = if glass && exact { fs_x } else { fs };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(if glass && exact { &with_prev } else { &plain }),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: if mesh { &mesh_buffers } else { &shape_buffers },
                },
                primitive: wgpu::PrimitiveState {
                    // The reference's faces are clockwise on the screen.
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: mesh.then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    // See-through things hide nothing drawn after them.
                    depth_write_enabled: Some(in_world && !glass),
                    // Reversed depth: the larger value is the nearer one.
                    depth_compare: Some(if in_world {
                        wgpu::CompareFunction::Greater
                    } else {
                        wgpu::CompareFunction::Always
                    }),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR,
                        blend: (glass && !exact).then_some(hardware),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // With the rank as a depth bias (study), level faces keep the
        // rasterizer's depth, like sides.
        let biased = options.rank != Rank::DepthBits;
        let mut pipes = vec![
            (Pipe::Card, None, make(Pipe::Card, "vs_shape", "fs_card", "fs_card_x", false, true)),
            (Pipe::Overlay, None, make(Pipe::Overlay, "vs_shape", "fs_overlay", "fs_overlay_x", false, false)),
            (Pipe::CardGlass, None, make(Pipe::CardGlass, "vs_shape", "fs_card", "fs_card_x", false, true)),
            (
                Pipe::OverlayGlass,
                None,
                make(Pipe::OverlayGlass, "vs_shape", "fs_overlay", "fs_overlay_x", false, false),
            ),
        ];
        for iso in [false, true] {
            let (side, side_x) = if iso { ("fs_side_iso", "fs_side_iso_x") } else { ("fs_side", "fs_side_x") };
            pipes.push((Pipe::Side, Some(iso), make(Pipe::Side, "vs_mesh", side, side_x, true, true)));
            pipes.push((Pipe::SideGlass, Some(iso), make(Pipe::SideGlass, "vs_mesh", side, side_x, true, true)));
            if biased {
                pipes.push((Pipe::Level, Some(iso), make(Pipe::Level, "vs_mesh", side, side_x, true, true)));
                pipes.push((Pipe::LevelGlass, Some(iso), make(Pipe::LevelGlass, "vs_mesh", side, side_x, true, true)));
            }
        }
        if !biased {
            pipes.push((Pipe::Level, None, make(Pipe::Level, "vs_mesh", "fs_level", "fs_level_x", true, true)));
            pipes.push((
                Pipe::LevelGlass,
                None,
                make(Pipe::LevelGlass, "vs_mesh", "fs_level", "fs_level_x", true, true),
            ));
        }

        let globals = buffer(device, GLOBALS_BYTES as u64, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let vertex = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
        let mesh = |faces: std::ops::Range<usize>| {
            let bytes = pack::mesh(faces);
            let b = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: bytes.len() as u64,
                usage: wgpu::BufferUsages::VERTEX,
                mapped_at_creation: true,
            });
            b.get_mapped_range_mut(..).expect("a new buffer is mapped").copy_from_slice(&bytes);
            b.unmap();
            b
        };

        let blit_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rwgpuengine_gpu blit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
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
            ],
        });
        let sampler = |filter: wgpu::FilterMode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: filter,
                min_filter: filter,
                ..Default::default()
            })
        };

        Renderer {
            options,
            pipes,
            globals,
            globals_bind,
            prev_layout,
            sides: mesh(0..4),
            levels: mesh(4..6),
            instances: Growing::new(device, vertex),
            shapes: Growing::new(device, vertex),
            target: None,
            held: None,
            blit: Blit {
                module: blit_module,
                layout: blit_layout,
                sharp: sampler(wgpu::FilterMode::Nearest),
                smooth: sampler(wgpu::FilterMode::Linear),
                pipelines: Vec::new(),
            },
            stats: Stats::default(),
        }
    }

    pub fn options(&self) -> Options {
        self.options
    }

    /// What the last [`draw`](Renderer::draw) did.
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// The frame's size in pixels: the size of the last list drawn.
    pub fn size(&self) -> Option<(u32, u32)> {
        self.target.as_ref().map(|t| (t.width, t.height))
    }

    /// The frame as a texture view (RGBA8, not sRGB), for a caller that
    /// wants to show it its own way.
    pub fn frame(&self) -> Option<&wgpu::TextureView> {
        self.target.as_ref().map(|t| &t.color_view)
    }

    fn pipe(&self, pipe: Pipe, iso: bool) -> &wgpu::RenderPipeline {
        let found = self.pipes.iter().find(|(p, view, _)| *p == pipe && view.is_none_or(|v| v == iso));
        &found.expect("every pipeline is built").2
    }

    fn target(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.target.as_ref().is_some_and(|t| (t.width, t.height) == (width, height)) {
            return;
        }
        let size = wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 };
        let texture = |format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        use wgpu::TextureUsages as U;
        let color = texture(COLOR, U::RENDER_ATTACHMENT | U::COPY_SRC | U::TEXTURE_BINDING);
        let depth = texture(DEPTH, U::RENDER_ATTACHMENT);
        let prev = texture(COLOR, U::COPY_DST | U::TEXTURE_BINDING);
        let prev_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.prev_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&prev.create_view(&Default::default())),
            }],
        });
        self.target = Some(Target {
            width,
            height,
            color_view: color.create_view(&Default::default()),
            depth_view: depth.create_view(&Default::default()),
            color,
            prev,
            prev_bind,
        });
    }

    /// Draw `list` into the renderer's frame, replacing what was there.
    ///
    /// Nothing waits for the GPU here. Instances are sent to the GPU only
    /// when they differ from the last list's, so a camera that moves over a
    /// still world uploads a few hundred bytes.
    pub fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, list: &RenderList) -> wgpu::SubmissionIndex {
        self.target(device, list.view.width, list.view.height);
        let mut stats = Stats::default();

        if self.held.as_ref().is_none_or(|(of, _)| *of != list.instances) {
            let packed = pack::instances(&list.instances);
            self.instances.write(device, queue, &packed.bytes);
            self.held = Some((list.instances.clone(), packed));
            stats.uploaded_instances = true;
        }
        let solid = &self.held.as_ref().expect("just set").1;
        let view = pack::View::new(list);
        let iso = view.iso();
        queue.write_buffer(&self.globals, 0, &pack::globals(list, &view, &self.options));
        let shapes = pack::shapes(list, &view, solid);
        self.shapes.write(device, queue, &shapes.bytes);

        // The frame as runs of draws. A new run starts where a see-through
        // color needs the picture so far and that picture has changed there
        // since the last copy.
        let exact = self.options.blend == Blend::Exact;
        let frame = Rect { x0: 0, y0: 0, x1: list.view.width as i32, y1: list.view.height as i32 };
        let mut runs: Vec<Vec<Cmd>> = vec![Vec::new()];
        let cmd = |pipe, vertices, instances: std::ops::Range<u32>| Cmd { pipe, vertices, instances };
        let first = &mut runs[0];
        if !solid.flats.is_empty() {
            first.push(cmd(Pipe::Level, 0..6, solid.flats.clone()));
        }
        if !solid.boxes.is_empty() {
            first.push(cmd(Pipe::Level, 0..12, solid.boxes.clone()));
            first.push(cmd(Pipe::Side, 0..24, solid.boxes.clone()));
        }
        if !shapes.cards.is_empty() {
            first.push(cmd(Pipe::Card, 0..6, shapes.cards.clone()));
        }
        // Pixels drawn since the last copy: everything, after the clear.
        let mut touched = vec![frame];
        let mut see_through = |runs: &mut Vec<Vec<Cmd>>, bounds: &Rect, glass: bool| {
            if exact && glass && touched.iter().any(|t| !t.intersect(bounds).is_empty()) {
                runs.push(Vec::new());
                touched.clear();
            }
            if touched.len() < 64 {
                touched.push(*bounds);
            } else {
                touched = vec![frame];
            }
        };
        for (what, bounds) in &shapes.glass {
            see_through(&mut runs, bounds, true);
            let run = runs.last_mut().expect("never empty");
            match what {
                Glass::Flat(at) => run.push(cmd(Pipe::LevelGlass, 0..6, *at..*at + 1)),
                Glass::Box(at) => {
                    run.push(cmd(Pipe::LevelGlass, 0..12, *at..*at + 1));
                    run.push(cmd(Pipe::SideGlass, 0..24, *at..*at + 1));
                }
                Glass::Card(r) => run.push(cmd(Pipe::CardGlass, 0..6, r.clone())),
            }
        }
        for (at, bounds, glass) in &shapes.overlay {
            see_through(&mut runs, bounds, *glass);
            let run = runs.last_mut().expect("never empty");
            let pipe = if *glass { Pipe::OverlayGlass } else { Pipe::Overlay };
            // Neighbors in the list that draw the same way share a draw.
            match run.last_mut() {
                Some(last) if last.pipe == pipe && last.instances.end == *at => last.instances.end += 1,
                _ => run.push(cmd(pipe, 0..6, *at..*at + 1)),
            }
        }

        let target = self.target.as_ref().expect("made above");
        let bg = list.view.background;
        let mut encoder = device.create_command_encoder(&Default::default());
        for (k, run) in runs.iter().enumerate() {
            if k > 0 {
                encoder.copy_texture_to_texture(
                    target.color.as_image_copy(),
                    target.prev.as_image_copy(),
                    wgpu::Extent3d { width: target.width, height: target.height, depth_or_array_layers: 1 },
                );
            }
            let clear = wgpu::Color { r: bg.r as f64 / 255.0, g: bg.g as f64 / 255.0, b: bg.b as f64 / 255.0, a: 1.0 };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if k == 0 { wgpu::LoadOp::Clear(clear) } else { wgpu::LoadOp::Load },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: if k == 0 { wgpu::LoadOp::Clear(0.0) } else { wgpu::LoadOp::Load },
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_bind, &[]);
            let mut bound = None;
            for c in run {
                if bound != Some(c.pipe) {
                    pass.set_pipeline(self.pipe(c.pipe, iso));
                    match c.pipe {
                        Pipe::Side | Pipe::SideGlass => {
                            pass.set_vertex_buffer(0, self.sides.slice(..));
                            pass.set_vertex_buffer(1, self.instances.buffer.slice(..));
                        }
                        Pipe::Level | Pipe::LevelGlass => {
                            pass.set_vertex_buffer(0, self.levels.slice(..));
                            pass.set_vertex_buffer(1, self.instances.buffer.slice(..));
                        }
                        _ => pass.set_vertex_buffer(0, self.shapes.buffer.slice(..)),
                    }
                    if exact && c.pipe.glass() {
                        pass.set_bind_group(1, &target.prev_bind, &[]);
                    }
                    bound = Some(c.pipe);
                }
                pass.draw(c.vertices.clone(), c.instances.clone());
                stats.draws += 1;
            }
            stats.passes += 1;
        }
        self.stats = stats;
        queue.submit([encoder.finish()])
    }

    /// Show the frame on `target` (a window's surface or a canvas), scaled
    /// to `area` (x, y, width, height in the target's pixels; all of it if
    /// `None`) with the rest black. `smooth` blends neighboring pixels when
    /// scaling; without it each frame pixel stays a sharp block.
    ///
    /// The frame's bytes are what the screen should show. Prefer a target
    /// format that does not encode sRGB (`Bgra8Unorm`, `Rgba8Unorm`): at the
    /// frame's own size the target then gets the reference's bytes. For an
    /// sRGB format the encoding is taken out first, to within a level.
    pub fn present(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        area: Option<[u32; 4]>,
        smooth: bool,
    ) -> Option<wgpu::SubmissionIndex> {
        let frame = self.target.as_ref()?;
        if !self.blit.pipelines.iter().any(|(f, _)| *f == format) {
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("blit"),
                layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &[Some(&self.blit.layout)],
                    immediate_size: 0,
                })),
                vertex: wgpu::VertexState {
                    module: &self.blit.module,
                    entry_point: Some("vs_blit"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.blit.module,
                    entry_point: Some(if format.is_srgb() { "fs_blit_srgb" } else { "fs_blit" }),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            self.blit.pipelines.push((format, pipeline));
        }
        let pipeline = &self.blit.pipelines.iter().find(|(f, _)| *f == format).expect("just made").1;
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.blit.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&frame.color_view) },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(if smooth { &self.blit.smooth } else { &self.blit.sharp }),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
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
            if let Some([x, y, w, h]) = area {
                pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
            }
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        Some(queue.submit([encoder.finish()]))
    }

    /// Start copying the frame off the GPU. Give the device time (natively:
    /// `device.poll(wgpu::PollType::Wait { .. })`; in a browser: return to
    /// the event loop), then [`FrameCopy::take`] has the bytes.
    pub fn copy_frame(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<FrameCopy> {
        let target = self.target.as_ref()?;
        // Rows in a copy start at multiples of 256 bytes.
        let row = (target.width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = buffer(
            device,
            row as u64 * target.height as u64,
            wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            target.color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(target.height),
                },
            },
            wgpu::Extent3d { width: target.width, height: target.height, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        let state = Arc::new(AtomicU8::new(WAITING));
        let done = state.clone();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            done.store(if r.is_ok() { READY } else { FAILED }, Ordering::Release);
        });
        Some(FrameCopy { buffer, state, row: row as usize, width: target.width, height: target.height })
    }
}

const WAITING: u8 = 0;
const READY: u8 = 1;
const FAILED: u8 = 2;

/// A frame on its way off the GPU ([`Renderer::copy_frame`]).
pub struct FrameCopy {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    row: usize,
    pub width: u32,
    pub height: u32,
}

impl FrameCopy {
    /// The frame as RGBA8 bytes, row-major, top row first: the layout of the
    /// reference's `Frame::rgba`. `None` until the GPU has delivered it;
    /// `Some(Err)` if it could not.
    pub fn take(&self) -> Option<Result<Vec<u8>, String>> {
        match self.state.load(Ordering::Acquire) {
            WAITING => None,
            READY => Some((|| {
                let mapped = self.buffer.slice(..).get_mapped_range().map_err(|e| e.to_string())?;
                let line = self.width as usize * 4;
                let mut out = Vec::with_capacity(line * self.height as usize);
                for row in mapped.chunks(self.row).take(self.height as usize) {
                    out.extend_from_slice(&row[..line]);
                }
                Ok(out)
            })()),
            _ => Some(Err("the GPU could not map the frame for reading".into())),
        }
    }
}
