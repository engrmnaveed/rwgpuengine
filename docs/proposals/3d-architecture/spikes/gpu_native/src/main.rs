//! Study spike (not engine code): N instanced boxes through wgpu, headless.
//! Prints build-independent facts and frame times; writes one frame as raw RGBA.
#[path = "../../shared/scene.rs"]
mod scene;

use std::time::Instant;

const SHADER: &str = r#"
struct Globals { view_proj: mat4x4<f32>, light: vec4<f32> };
@group(0) @binding(0) var<uniform> g: Globals;
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) color: vec4<f32> };
@vertex fn vs(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>,
              @location(2) offset: vec3<f32>, @location(3) size: vec3<f32>, @location(4) color: vec4<f32>) -> VOut {
  var o: VOut;
  o.clip = g.view_proj * vec4<f32>(pos * size + offset, 1.0);
  let k = g.light.w + (1.0 - g.light.w) * max(dot(normal, g.light.xyz), 0.0);
  o.color = vec4<f32>(color.rgb * k, 1.0);
  return o;
}
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> { return i.color; }
"#;

fn bytes_f32(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn instance_bytes(town: &[scene::Inst]) -> Vec<u8> {
    let mut b = Vec::with_capacity(town.len() * 28);
    for i in town {
        b.extend(bytes_f32(&i.pos));
        b.extend(bytes_f32(&i.size));
        b.extend(i.color);
    }
    b
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// Calls `frame` up to `max` times and returns the median and the count. It stops early once
/// `budget_ms` has passed and at least `min` frames are in, so a slow software adapter keeps
/// the run short; a real GPU reaches `max` well inside the budget.
fn timed(max: usize, min: usize, budget_ms: f64, mut frame: impl FnMut() -> f64) -> (f64, usize) {
    let t = Instant::now();
    let mut v = Vec::new();
    while v.len() < max && (v.len() < min || t.elapsed().as_secs_f64() * 1e3 < budget_ms) {
        v.push(frame());
    }
    let n = v.len();
    (median(v), n)
}

fn describe(info: &wgpu::AdapterInfo) -> String {
    format!(
        "{} | {:?} | {:?} | driver \"{}\" | driver info \"{}\" | vendor 0x{:04x} device 0x{:04x}",
        info.name, info.backend, info.device_type, info.driver, info.driver_info, info.vendor, info.device
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out_dir = args.first().cloned().unwrap_or_else(|| ".".into());

    // Every adapter this machine offers, on every backend compiled in.
    {
        let everything = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        for (i, a) in pollster::block_on(everything.enumerate_adapters(wgpu::Backends::all())).iter().enumerate() {
            println!("adapter {i}: {}", describe(&a.get_info()));
        }
        let pick = pollster::block_on(everything.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }));
        println!("wgpu's own pick: {}", pick.map(|a| describe(&a.get_info())).unwrap_or_else(|e| format!("none ({e})")));
    }

    // The one to use. wgpu itself reads WGPU_BACKEND (vulkan, gl, dx12, metal, or a comma list)
    // and its other WGPU_* variables here. The spike reads two more: WGPU_ADAPTER_NAME (part of
    // the name, as in wgpu::util) and WGPU_FORCE_FALLBACK_ADAPTER=1 (software adapters only).
    let t0 = Instant::now();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let fallback = std::env::var("WGPU_FORCE_FALLBACK_ADAPTER").is_ok_and(|v| v == "1");
    let adapter = match std::env::var("WGPU_ADAPTER_NAME") {
        Ok(want) => pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
            .into_iter()
            .find(|a| a.get_info().name.to_lowercase().contains(&want.to_lowercase())),
        Err(_) => pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: fallback,
            ..Default::default()
        }))
        .map_err(|e| eprintln!("request_adapter: {e}"))
        .ok(),
    };
    let Some(adapter) = adapter else {
        eprintln!("no adapter (WGPU_BACKEND={:?}, fallback only: {fallback})", std::env::var("WGPU_BACKEND").ok());
        std::process::exit(1);
    };
    let info = adapter.get_info();
    // The spike needs very little; ask for the WebGL2 floor so GL and software adapters qualify.
    let limits = wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits());
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_limits: limits, ..Default::default() }))
        .expect("no device");
    println!("adapter: {} ({:?}, {:?}); device ready in {:.0} ms", info.name, info.backend, info.device_type, t0.elapsed().as_secs_f64() * 1e3);
    println!("adapter used: {}", describe(&info));

    let (w, h) = (scene::W, scene::H);
    let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
    let color = device.create_texture(&wgpu::TextureDescriptor {
        label: None, size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[],
    });
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: None, size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float, usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[],
    });
    let color_view = color.create_view(&Default::default());
    let depth_view = depth.create_view(&Default::default());

    let t1 = Instant::now();
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let vertex_attrs = [
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 12, shader_location: 1 },
    ];
    let instance_attrs = [
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 2 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 12, shader_location: 3 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Unorm8x4, offset: 24, shader_location: 4 },
    ];
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader, entry_point: Some("vs"), compilation_options: Default::default(),
            buffers: &[
                Some(wgpu::VertexBufferLayout { array_stride: 24, step_mode: wgpu::VertexStepMode::Vertex, attributes: &vertex_attrs }),
                Some(wgpu::VertexBufferLayout { array_stride: 28, step_mode: wgpu::VertexStepMode::Instance, attributes: &instance_attrs }),
            ],
        },
        primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(), bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader, entry_point: Some("fs"), compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    });
    println!("shader + pipeline: {:.1} ms", t1.elapsed().as_secs_f64() * 1e3);

    // Cube: two triangles per face, position + normal.
    let mut cube = Vec::new();
    for (n, q) in scene::FACES {
        for i in [0, 1, 2, 0, 2, 3] {
            cube.extend(q[i]);
            cube.extend(n);
        }
    }
    let vb = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (cube.len() * 4) as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    queue.write_buffer(&vb, 0, &bytes_f32(&cube));
    let globals = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 80, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None, layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (w * h * 4) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });

    for n in scene::sizes() {
        let town = scene::town(n);
        let t = Instant::now();
        let data = instance_bytes(&town);
        let build_ms = t.elapsed().as_secs_f64() * 1e3;
        let ib = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: data.len() as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        queue.write_buffer(&ib, 0, &data);
        let vp = scene::view_proj(n);
        let mut g: Vec<f32> = vp.iter().flatten().copied().collect();
        g.extend([scene::LIGHT[0], scene::LIGHT[1], scene::LIGHT[2], scene::AMBIENT]);
        queue.write_buffer(&globals, 0, &bytes_f32(&g));

        let frame = |upload: bool, copy: bool| {
            let t = Instant::now();
            if upload {
                queue.write_buffer(&ib, 0, &data);
            }
            let mut enc = device.create_command_encoder(&Default::default());
            {
                let c = scene::CLEAR;
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &color_view, depth_slice: None, resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color { r: c[0] as f64 / 255.0, g: c[1] as f64 / 255.0, b: c[2] as f64 / 255.0, a: 1.0 }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.set_vertex_buffer(1, ib.slice(..));
                pass.draw(0..36, 0..n as u32);
            }
            if copy {
                enc.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo { texture: &color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    wgpu::TexelCopyBufferInfo { buffer: &readback, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) } },
                    size,
                );
            }
            let idx = queue.submit([enc.finish()]);
            device.poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None }).expect("poll");
            t.elapsed().as_secs_f64() * 1e3
        };
        timed(10, 1, 1_000.0, || frame(false, false));
        let (fixed, fixed_n) = timed(60, 3, 3_000.0, || frame(false, false));
        let (moving, moving_n) = timed(60, 3, 3_000.0, || frame(true, false));
        frame(false, true);
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map"));
        device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).expect("poll");
        let px = slice.get_mapped_range().expect("range").to_vec();
        readback.unmap();
        std::fs::write(format!("{out_dir}/gpu_{n}.rgba"), &px).expect("write");
        println!(
            "n={n:>7}: {} triangles; instance data {:.2} MB built in {build_ms:.2} ms; frame {fixed:.2} ms (nothing uploaded), {moving:.2} ms (all instances re-uploaded), medians of {fixed_n} and {moving_n} frames; image {:016x}",
            n * 12,
            data.len() as f64 / 1e6,
            scene::fnv(&px)
        );
    }
}
