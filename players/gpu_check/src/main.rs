//! The check that holds the wgpu renderer to the CPU reference renderer.
//!
//! For each reference scene it draws one render list with both renderers and
//! compares the pictures. There is no per-channel tolerance: a pixel that
//! differs must lie on an edge of the reference picture (the reference knows
//! who owns each pixel), and a scene may have only so many of them
//! (`budgets.toml`, `[players] pixels_apart`).
//!
//! Run it through `cargo xtask gpu`, which also measures the players'
//! dependency and build budgets. Options when run by hand:
//!
//! ```text
//! --max-pixels N        differing pixels allowed per scene (default 50)
//! --frame-budget-us N   limit for a frame of 50,000 boxes on a real GPU
//! --scene TEXT          only scenes whose name contains TEXT
//! --pictures            write every scene's pictures, not only failing ones
//! --study               measure the options of ADR 0021 on this adapter
//! --no-timing           skip the frame times
//! ```
//!
//! wgpu's own variables choose the adapter: `WGPU_BACKEND` (vulkan, metal,
//! dx12), and here also `WGPU_ADAPTER_NAME` (part of the name) and
//! `WGPU_FORCE_FALLBACK_ADAPTER=1` (software adapters only).

// This tool measures wall-clock time on purpose; the ban is for the engine.
#![allow(clippy::disallowed_methods)]

mod scenes;
#[path = "../../../xtask/src/town.rs"]
mod town;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use rwgpuengine_gpu::{Blend, CardDepth, FaceColor, Options, Origin, Rank, Renderer, wgpu};
use rwgpuengine_render::{Frame, Projected, RenderList, image::encode_png, project, render};

use scenes::Scene;

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("gpu check: FAILED: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Args {
    max_pixels: usize,
    frame_budget_us: u64,
    only: Option<String>,
    pictures: bool,
    study: bool,
    timing: bool,
    out: PathBuf,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn args() -> Result<Args, String> {
    let mut a = Args {
        max_pixels: 50,
        frame_budget_us: 4_000,
        only: None,
        pictures: false,
        study: false,
        timing: true,
        out: root().join("target/gpu-check"),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--max-pixels" => {
                a.max_pixels = value("--max-pixels")?.parse().map_err(|e| format!("--max-pixels: {e}"))?
            }
            "--frame-budget-us" => {
                a.frame_budget_us =
                    value("--frame-budget-us")?.parse().map_err(|e| format!("--frame-budget-us: {e}"))?
            }
            "--scene" => a.only = Some(value("--scene")?),
            "--out" => a.out = PathBuf::from(value("--out")?),
            "--pictures" => a.pictures = true,
            "--study" => a.study = true,
            "--no-timing" => a.timing = false,
            other => return Err(format!("unknown option `{other}` (see the top of players/gpu_check/src/main.rs)")),
        }
    }
    Ok(a)
}

/// Run a future that never really waits: natively, wgpu's are ready at once.
fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    info: wgpu::AdapterInfo,
}

impl Gpu {
    /// Is this a real graphics card (not a software adapter)?
    fn real(&self) -> bool {
        self.info.device_type != wgpu::DeviceType::Cpu
    }
}

fn describe(info: &wgpu::AdapterInfo) -> String {
    format!(
        "{} ({:?}, {:?}; driver \"{}\", \"{}\")",
        info.name, info.backend, info.device_type, info.driver, info.driver_info
    )
}

fn gpu() -> Result<Gpu, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let fallback = std::env::var("WGPU_FORCE_FALLBACK_ADAPTER").is_ok_and(|v| v == "1");
    let adapter = match std::env::var("WGPU_ADAPTER_NAME") {
        Ok(want) => block_on(instance.enumerate_adapters(wgpu::Backends::all()))
            .into_iter()
            .find(|a| a.get_info().name.to_lowercase().contains(&want.to_lowercase())),
        Err(_) => block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: fallback,
            ..Default::default()
        }))
        .ok(),
    };
    let Some(adapter) = adapter else {
        return Err(format!(
            "no graphics adapter was found (WGPU_BACKEND={:?}, WGPU_ADAPTER_NAME={:?}, software only: {fallback}).\n\
             This check needs Vulkan, Metal or Direct3D 12. On Linux without a graphics card, install Mesa's \
             software Vulkan driver: `sudo apt-get install mesa-vulkan-drivers libvulkan1`.",
            std::env::var("WGPU_BACKEND").ok(),
            std::env::var("WGPU_ADAPTER_NAME").ok(),
        ));
    };
    let info = adapter.get_info();
    let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
    let (device, queue) =
        block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_limits: limits, ..Default::default() }))
            .map_err(|e| format!("{}: no device: {e}", describe(&info)))?;
    Ok(Gpu { device, queue, info })
}

/// Draw `list` and wait for the picture.
fn gpu_frame(gpu: &Gpu, renderer: &mut Renderer, list: &RenderList) -> Result<Vec<u8>, String> {
    renderer.draw(&gpu.device, &gpu.queue, list);
    let copy = renderer.copy_frame(&gpu.device, &gpu.queue).ok_or("nothing was drawn")?;
    gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).map_err(|e| e.to_string())?;
    copy.take().ok_or("the frame did not arrive")?
}

/// How a GPU picture differs from the reference's.
#[derive(Default)]
struct Outcome {
    differing: usize,
    /// Differing pixels that are not on an edge of the reference picture.
    inside: usize,
    /// The first few, described.
    first: Vec<String>,
    /// 1 for each differing pixel.
    mask: Vec<u8>,
}

/// Is pixel (x, y) on an edge of the reference picture: does a neighbor have
/// another owner or another color?
fn on_edge(frame: &Frame, x: usize, y: usize) -> bool {
    let (w, h) = (frame.width as usize, frame.height as usize);
    let at = |x: usize, y: usize| (frame.owner[y * w + x], &frame.rgba[(y * w + x) * 4..(y * w + x) * 4 + 4]);
    let me = at(x, y);
    for ny in y.saturating_sub(1)..(y + 2).min(h) {
        for nx in x.saturating_sub(1)..(x + 2).min(w) {
            if at(nx, ny) != me {
                return true;
            }
        }
    }
    false
}

fn compare(seen: &Projected, frame: &Frame, gpu: &[u8]) -> Outcome {
    let w = frame.width as usize;
    let mut out = Outcome { mask: vec![0; frame.owner.len()], ..Outcome::default() };
    for (i, (want, got)) in frame.rgba.as_chunks::<4>().0.iter().zip(gpu.as_chunks::<4>().0).enumerate() {
        if want == got {
            continue;
        }
        let (x, y) = (i % w, i / w);
        let edge = on_edge(frame, x, y);
        out.differing += 1;
        out.inside += usize::from(!edge);
        out.mask[i] = 1;
        // Pixels inside a face are the ones worth reading first.
        if out.first.len() < 8 || (!edge && out.inside <= 8) {
            let owner = match frame.owner[i].checked_sub(1).map(|k| &seen.items[k as usize]) {
                None => "the background".to_string(),
                Some(item) => match item.entity {
                    Some(id) => format!("the {} of entity {id} at cell {:?}", item.part.name(), item.cell),
                    None => format!("the ground at cell {:?}", item.cell),
                },
            };
            let hex = |p: &[u8; 4]| format!("#{:02x}{:02x}{:02x}{:02x}", p[0], p[1], p[2], p[3]);
            out.first.push(format!(
                "({x}, {y}): reference {} gpu {}; the reference shows {owner}{}",
                hex(want),
                hex(got),
                if edge { "" } else { "; NOT on an edge" }
            ));
        }
    }
    out
}

/// The reference picture, the GPU's, and the reference dimmed with every
/// differing pixel in red.
fn write_pictures(dir: &Path, name: &str, frame: &Frame, gpu: &[u8], mask: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let mut diff = frame.rgba.clone();
    for (px, m) in diff.as_chunks_mut::<4>().0.iter_mut().zip(mask) {
        *px = if *m != 0 { [255, 0, 0, 255] } else { [px[0] / 3 + 40, px[1] / 3 + 40, px[2] / 3 + 40, 255] };
    }
    for (kind, bytes) in [("reference", &frame.rgba), ("gpu", &gpu.to_vec()), ("diff", &diff)] {
        let png = encode_png(frame.width, frame.height, bytes)?;
        let path = dir.join(format!("{file}.{kind}.png"));
        std::fs::write(&path, png).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// Median milliseconds for `frame`, over up to `max` runs; a slow adapter
/// stops after `budget_ms` once it has three.
fn timed(max: usize, budget_ms: f64, mut frame: impl FnMut()) -> (f64, usize) {
    let start = Instant::now();
    let mut v = Vec::new();
    while v.len() < max && (v.len() < 3 || start.elapsed().as_secs_f64() * 1e3 < budget_ms) {
        let t = Instant::now();
        frame();
        v.push(t.elapsed().as_secs_f64() * 1e3);
    }
    let n = v.len();
    (median(v), n)
}

/// Draw the list and wait until the GPU has finished it.
fn draw_and_wait(gpu: &Gpu, renderer: &mut Renderer, list: &RenderList) {
    let index = renderer.draw(&gpu.device, &gpu.queue, list);
    gpu.device.poll(wgpu::PollType::Wait { submission_index: Some(index), timeout: None }).expect("poll");
}

/// Frame times of the generated town: (nothing uploaded, every instance
/// uploaded again), in milliseconds.
fn town_times(gpu: &Gpu, options: Options, boxes: usize) -> Result<(f64, f64, usize), String> {
    let list = scenes::town_list(boxes)?;
    let mut renderer = Renderer::new(&gpu.device, options);
    timed(10, 1_000.0, || draw_and_wait(gpu, &mut renderer, &list));
    let (still, n) = timed(60, 3_000.0, || draw_and_wait(gpu, &mut renderer, &list));
    // A list that differs in one color each frame: all of it is packed and sent again.
    let mut moving = list.clone();
    let mut tick = 0u8;
    let (changed, _) = timed(60, 3_000.0, || {
        tick = tick.wrapping_add(1);
        moving.instances[0].color.r = tick;
        draw_and_wait(gpu, &mut renderer, &moving);
    });
    Ok((still, changed, n))
}

struct Row {
    name: String,
    differing: usize,
    inside: usize,
    ok: bool,
}

/// Compare every scene under `options`. Pictures of failing scenes (or of
/// all, with `pictures`) go to `out`.
fn check(gpu: &Gpu, options: Options, scenes: &[Scene], a: &Args, loud: bool) -> Result<Vec<Row>, String> {
    let mut renderer = Renderer::new(&gpu.device, options);
    let mut rows = Vec::new();
    for scene in scenes {
        let seen = project(&scene.list);
        let frame = render(&seen);
        let got = gpu_frame(gpu, &mut renderer, &scene.list)?;
        if got.len() != frame.rgba.len() {
            return Err(format!(
                "{}: the GPU frame has {} bytes, the reference {}",
                scene.name,
                got.len(),
                frame.rgba.len()
            ));
        }
        let outcome = compare(&seen, &frame, &got);
        let ok = outcome.inside == 0 && outcome.differing <= a.max_pixels;
        if loud {
            let stats = renderer.stats();
            println!(
                "{} {:<44} {:>4} x {:<4} {:>6} apart, {:>5} not on an edge   ({} passes, {} draws)",
                if ok { "ok  " } else { "FAIL" },
                scene.name,
                frame.width,
                frame.height,
                outcome.differing,
                outcome.inside,
                stats.passes,
                stats.draws
            );
            if !ok {
                for line in &outcome.first {
                    println!("       {line}");
                }
            }
            if !ok || a.pictures {
                write_pictures(&a.out, &scene.name, &frame, &got, &outcome.mask)?;
            }
        }
        rows.push(Row { name: scene.name.clone(), differing: outcome.differing, inside: outcome.inside, ok });
    }
    Ok(rows)
}

fn run() -> Result<bool, String> {
    let a = args()?;
    let gpu = gpu()?;
    println!("adapter: {}", describe(&gpu.info));
    let mut scenes = scenes::all(&root())?;
    if let Some(only) = &a.only {
        scenes.retain(|s| s.name.contains(only.as_str()));
    }
    if a.study {
        return study(&gpu, &scenes, &a).map(|()| true);
    }

    println!("\nthe two renderers, scene by scene (limit: {} pixels apart, all of them on an edge)", a.max_pixels);
    let rows = check(&gpu, Options::default(), &scenes, &a, true)?;
    let failed: Vec<&Row> = rows.iter().filter(|r| !r.ok).collect();
    let worst = rows.iter().map(|r| r.differing).max().unwrap_or(0);
    println!(
        "\n{} scenes, {} failed; the most pixels apart in one scene: {worst}; pixels apart in all: {}",
        rows.len(),
        failed.len(),
        rows.iter().map(|r| r.differing).sum::<usize>()
    );
    let mut ok = failed.is_empty();
    let mut results = String::new();
    if !ok {
        println!("failed: {}", failed.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", "));
        println!("pictures (reference, gpu, diff) are in {}", a.out.display());
    }

    if a.timing && a.only.is_none() {
        println!("\nframe time, generated town at 1280 x 720 (median; the GPU has finished the frame)");
        for boxes in [5_000, 50_000] {
            let (still, changed, n) = town_times(&gpu, Options::default(), boxes)?;
            println!(
                "{boxes:>6} boxes: {still:>8.3} ms with nothing uploaded, {changed:>8.3} ms with every instance \
                 packed and uploaded again ({n} frames)"
            );
            if boxes == 50_000 {
                results.push_str(&format!("gpu_frame_us {}\n", (still * 1e3).round() as u64));
                let limit = a.frame_budget_us as f64 / 1e3;
                if !gpu.real() {
                    println!("       not enforced: this is a software adapter (the limit on a real GPU is {limit} ms)");
                } else if still > limit {
                    println!("       OVER the limit of {limit} ms");
                    ok = false;
                } else {
                    println!("       within the limit of {limit} ms");
                }
            }
        }
    }
    if a.only.is_none() {
        match present_check(&gpu, &scenes) {
            Ok(()) => println!(
                "\nshowing the frame: its own size gives its bytes; twice the size gives each pixel four times"
            ),
            Err(e) => {
                println!("\nshowing the frame: FAILED: {e}");
                ok = false;
            }
        }
    }
    // For `cargo xtask gpu`, which puts these next to the other budgets.
    results.push_str(&format!("pixels_apart {worst}\nreal_gpu {}\n", u8::from(gpu.real())));
    std::fs::create_dir_all(&a.out).map_err(|e| format!("{}: {e}", a.out.display()))?;
    std::fs::write(a.out.join("results.txt"), results).map_err(|e| e.to_string())?;
    println!("\ngpu check: {}", if ok { "all green" } else { "FAILED" });
    Ok(ok)
}

/// `Renderer::present` onto a texture: at the frame's own size the target
/// gets the frame's bytes, and at twice the size each pixel becomes four.
fn present_check(gpu: &Gpu, scenes: &[Scene]) -> Result<(), String> {
    let Some(scene) = scenes.first() else { return Ok(()) };
    let mut renderer = Renderer::new(&gpu.device, Options::default());
    let want = gpu_frame(gpu, &mut renderer, &scene.list)?;
    let (w, h) = renderer.size().ok_or("no frame")?;
    for scale in [1u32, 2] {
        let size = wgpu::Extent3d { width: w * scale, height: h * scale, depth_or_array_layers: 1 };
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        renderer
            .present(&gpu.device, &gpu.queue, &target.create_view(&Default::default()), format, None, false)
            .ok_or("nothing to present")?;
        let row = (size.width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: row as u64 * size.height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(size.height),
                },
            },
            size,
        );
        gpu.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("map"));
        gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).map_err(|e| e.to_string())?;
        let got = buffer.slice(..).get_mapped_range().map_err(|e| e.to_string())?;
        for y in 0..size.height {
            for x in 0..size.width {
                let at = (y * row + x * 4) as usize;
                let from = (((y / scale) * w + x / scale) * 4) as usize;
                if got[at..at + 4] != want[from..from + 4] {
                    return Err(format!(
                        "at {scale} times the size, pixel ({x}, {y}) is {:?}, the frame has {:?}",
                        &got[at..at + 4],
                        &want[from..from + 4]
                    ));
                }
            }
        }
    }
    Ok(())
}

/// The options ADR 0021 chose between, measured on this adapter: pixels
/// apart over all scenes, and the frame time of the town.
fn study(gpu: &Gpu, scenes: &[Scene], a: &Args) -> Result<(), String> {
    let chosen = Options::default();
    let variants: [(&str, Options); 9] = [
        ("chosen", chosen),
        ("color: rounded in the shader", Options { color: FaceColor::Shader, ..chosen }),
        ("see-through: the GPU's blender", Options { blend: Blend::Hardware, ..chosen }),
        ("rank: depth bias, 16 steps", Options { rank: Rank::Bias(16), ..chosen }),
        ("rank: depth bias, 256 steps", Options { rank: Rank::Bias(256), ..chosen }),
        ("rank: depth bias, 4096 steps", Options { rank: Rank::Bias(4096), ..chosen }),
        ("cards: the anchor's depth everywhere", Options { cards: CardDepth::Anchor, ..chosen }),
        ("corners: measured from the world's origin", Options { origin: Origin::World, ..chosen }),
        ("chosen, again", chosen),
    ];
    let boxes = if gpu.real() { 50_000 } else { 5_000 };
    println!("\noptions on this adapter: pixels apart over {} scenes, and a frame of {boxes} boxes", scenes.len());
    println!("{:<44} {:>8} {:>10} {:>8} {:>12}", "option", "apart", "not edge", "scenes", "frame ms");
    for (name, options) in variants {
        let rows = check(gpu, options, scenes, a, false)?;
        let (still, _, _) = if a.timing { town_times(gpu, options, boxes)? } else { (0.0, 0.0, 0) };
        println!(
            "{name:<44} {:>8} {:>10} {:>8} {still:>12.3}",
            rows.iter().map(|r| r.differing).sum::<usize>(),
            rows.iter().map(|r| r.inside).sum::<usize>(),
            rows.iter().filter(|r| r.differing > 0).count(),
        );
        for r in rows.iter().filter(|r| r.differing > 0) {
            println!("    {:<44} {:>6} {:>10}", r.name, r.differing, r.inside);
        }
    }
    gaps(gpu, a)?;
    // What projecting every corner on the CPU would cost, as the reference does.
    let list = scenes::town_list(boxes)?;
    let (ms, _) = timed(10, 2_000.0, || {
        std::hint::black_box(project(&list));
    });
    println!("\nfor comparison, the reference's own projection of {boxes} boxes on the CPU: {ms:.3} ms per frame");
    Ok(())
}

/// Three cases the gate leaves out, because there the reference's own
/// picture is decided by rounding (ADR 0021). For each camera: how much of
/// the thing the reference shows, how much the GPU shows, and how far apart
/// the two pictures are.
fn gaps(gpu: &Gpu, a: &Args) -> Result<(), String> {
    use rwgpuengine_render::reference::coverage;
    println!(
        "\noutside the gate: what the reference leaves to rounding (pixels of the thing shown, of those it covers)"
    );
    println!("{:<64} {:>16} {:>16} {:>8} {:>10}", "case", "reference", "gpu", "apart", "not edge");
    let mut renderer = Renderer::new(&gpu.device, Options::default());
    for (name, part, scenes) in scenes::gaps()? {
        for scene in scenes {
            let seen = project(&scene.list);
            let frame = render(&seen);
            let got = gpu_frame(gpu, &mut renderer, &scene.list)?;
            let outcome = compare(&seen, &frame, &got);
            // The thing's pixels, and those of them each picture gives to it:
            // every item of that part has one color no other thing has.
            let cov = coverage(&seen, &frame);
            let (mut covered, mut shown) = (0u32, 0u32);
            for (k, _) in seen.items.iter().enumerate().filter(|(_, i)| i.part == part && i.entity.is_some()) {
                covered += cov.items[k].drawn;
                shown += cov.items[k].visible;
            }
            let red = |rgba: &[u8]| rgba.as_chunks::<4>().0.iter().filter(|p| **p == [255, 0, 0, 255]).count();
            debug_assert_eq!(red(&frame.rgba) as u32, shown);
            println!(
                "{:<64} {:>16} {:>16} {:>8} {:>10}",
                format!("{name}, {}", scene.name),
                format!("{shown} of {covered}"),
                format!("{} of {covered}", red(&got)),
                outcome.differing,
                outcome.inside
            );
            if a.pictures {
                write_pictures(&a.out, &format!("gap {name} {}", scene.name), &frame, &got, &outcome.mask)?;
            }
        }
    }
    Ok(())
}
