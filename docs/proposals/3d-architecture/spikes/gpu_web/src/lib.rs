//! Study spike (not engine code): the same N instanced boxes through wgpu's
//! WebGPU backend in a browser. Returns timings and a hash of the pixels.
#[path = "../../shared/scene.rs"]
mod scene;

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

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

fn now() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
}

fn bytes_f32(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Resolves when everything submitted so far has finished on the GPU.
async fn finished(queue: &wgpu::Queue) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        queue.on_submitted_work_done(move || {
            let _ = resolve.call0(&JsValue::NULL);
        });
    });
    let _ = JsFuture::from(promise).await;
}

fn err(e: impl core::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

#[wasm_bindgen]
pub async fn run(n: u32, frames: u32) -> Result<String, JsValue> {
    let n = n as usize;
    let t0 = now();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions::default()).await.map_err(err)?;
    let info = adapter.get_info();
    let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor::default()).await.map_err(err)?;
    let ready_ms = now() - t0;

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

    let mut cube = Vec::new();
    for (normal, q) in scene::FACES {
        for i in [0, 1, 2, 0, 2, 3] {
            cube.extend(q[i]);
            cube.extend(normal);
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

    let town = scene::town(n);
    let mut data = Vec::with_capacity(n * 28);
    for i in &town {
        data.extend(bytes_f32(&i.pos));
        data.extend(bytes_f32(&i.size));
        data.extend(i.color);
    }
    let ib = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: data.len() as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    queue.write_buffer(&ib, 0, &data);
    let mut g: Vec<f32> = scene::view_proj(n).iter().flatten().copied().collect();
    g.extend([scene::LIGHT[0], scene::LIGHT[1], scene::LIGHT[2], scene::AMBIENT]);
    queue.write_buffer(&globals, 0, &bytes_f32(&g));

    let encode = |upload: bool, copy: bool| {
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
        queue.submit([enc.finish()]);
    };

    for _ in 0..5 {
        encode(false, false);
        finished(&queue).await;
    }
    let mut times = [0.0f64; 2];
    for (slot, upload) in [(0, false), (1, true)] {
        let t = now();
        for _ in 0..frames {
            encode(upload, false);
            finished(&queue).await;
        }
        times[slot] = (now() - t) / frames as f64;
    }
    // CPU cost of encoding alone, without waiting for the GPU.
    let t = now();
    for _ in 0..frames {
        encode(false, false);
    }
    let encode_ms = (now() - t) / frames as f64;
    finished(&queue).await;

    encode(false, true);
    let slice = readback.slice(..);
    let mapped = js_sys::Promise::new(&mut |resolve, _| {
        slice.map_async(wgpu::MapMode::Read, move |_| {
            let _ = resolve.call0(&JsValue::NULL);
        });
    });
    let _ = JsFuture::from(mapped).await;
    let px = slice.get_mapped_range().map_err(err)?.to_vec();
    readback.unmap();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in &px {
        hash = (hash ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    Ok(format!(
        "n={n}: adapter \"{}\" ({:?}); device ready {ready_ms:.0} ms; frame {:.2} ms (nothing uploaded), {:.2} ms (all instances re-uploaded); encode only {encode_ms:.3} ms; pixels {hash:016x}",
        info.name, info.backend, times[0], times[1]
    ))
}
