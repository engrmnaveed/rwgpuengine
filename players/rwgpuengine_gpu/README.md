# rwgpuengine_gpu

Draws a render list with wgpu, for players. It is the players' half of the renderer: the CPU reference
renderer in `rwgpuengine_render` decides what a frame looks like, and this crate draws the same list by the
same rules, fast. `cargo xtask gpu` holds its picture to the reference's
([ADR 0021](../../docs/decisions/0021-wgpu-renderer-held-to-the-reference.md)).

There is no window in this crate and nothing in it blocks. A window, or a browser canvas, is put around it by
the code that owns the `wgpu::Device`.

## Calling it

```rust
use rwgpuengine_gpu::{Options, Renderer, wgpu};   // the crate re-exports the exact wgpu it is written for

// Once, with a device you already have.
let mut renderer = Renderer::new(&device, Options::default());

// Each time the picture changed.
let list = session.render_list(false);            // rwgpuengine_runtime::Session
renderer.draw(&device, &queue, &list);            // into the renderer's own frame

// Each time the window is shown.
let view = surface_texture.texture.create_view(&Default::default());
renderer.present(&device, &queue, &view, surface_format, Some([x, y, w, h]), false);
```

In this order:

1. **`Renderer::new(&device, Options::default())`** builds the pipelines. `Options::default()` is what players
   get. The other values exist so that the measurements of ADR 0021 can be repeated; do not ship them.
2. **`draw(&device, &queue, &list)`** draws the list into the renderer's own frame and returns the
   `wgpu::SubmissionIndex`. The frame has the list's size (`list.view.width` × `list.view.height`, 640 × 480
   for the example games), in `Rgba8Unorm`. It does not wait for the GPU.
3. **`present(&device, &queue, &target, format, area, smooth)`** scales the frame onto a target view: a
   window's surface or a canvas. `area` is `[x, y, width, height]` in the target's pixels, for a picture that
   keeps its shape inside a window of another shape; the rest of the target is cleared to black. `None` fills
   the target. `smooth` blends neighboring pixels when scaling; without it each frame pixel is a sharp block.
4. For a check, not for a window: **`copy_frame(&device, &queue)`** starts a copy of the frame to memory.
   Give the device time (natively `device.poll(wgpu::PollType::Wait { .. })`; in a browser, return to the
   event loop), then `FrameCopy::take()` has the frame as RGBA8 bytes, top row first, in the layout of the
   reference's `Frame::rgba`.

`size()` gives the frame's size, `frame()` its texture view for a caller that wants to show it another way,
and `stats()` says what the last `draw` did (passes, draws, what was sent to the GPU).

## The device and the surface

- **No wgpu feature is needed.** `wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits())` is
  what the check asks for, and it is enough.
- **Pick the adapter as wgpu does.** `request_adapter` with `PowerPreference::HighPerformance` finds the
  Metal device on macOS, Mesa lavapipe on a Linux machine with no graphics card, and WARP on such a Windows
  machine, with nothing forced. The backends compiled in are Vulkan, Metal, Direct3D 12 and WebGPU; there is
  no GL.
- **Use a surface format that does not encode sRGB**: `Bgra8Unorm` or `Rgba8Unorm`, or an sRGB surface with
  such a view format. The frame's bytes already are what the screen should show. At the frame's own size
  the target then gets the reference's bytes. For an sRGB target `present` takes the encoding out in its
  shader; that is right to within one level, and it is the second-best path.
- **The renderer never draws into the window itself.** It needs to copy its own frame (see-through colors
  are mixed from a copy of the picture so far), and the frame has the game's size, not the window's.

## What a frame costs

- **Skip `draw` when the list did not change.** `RenderList` is `PartialEq`; `Presenter` in
  `rwgpuengine_runtime` does this for the CPU painter.
- `draw` keeps the last list's instances. They are packed and sent to the GPU again only when
  `list.instances` differs (`stats().uploaded_instances`). A camera that moves over a still world sends the
  uniforms (1,744 bytes) and the cards, and in the iso view 16 bytes per instance
  (`stats().uploaded_columns`).
- Cards (sprites and text in the world) and the 2D layer are placed on the CPU on every `draw`, by the
  reference's own code.
- See-through things that overlap on the screen take one copy of the frame and one pass each
  (`stats().passes`). The example games take 1 or 2 passes.
- Measured on an Apple M5 Pro, 50,000 boxes on a 224 × 224 ground at 1280 × 720: 1.0 ms with nothing sent,
  1.6 ms with every instance packed and sent again, 1.2 ms through the iso view and 1.6 ms while that camera
  turns.

## The browser

The crate compiles for `wasm32-unknown-unknown` (the gate checks it) and uses wgpu's WebGPU backend there.
Nothing waits: `draw` and `present` return at once, and `FrameCopy::take` returns `None` until the browser
has delivered the bytes. The web player (`rwgpuengine_player`'s `web` module,
[ADR 0024](../../docs/decisions/0024-web-player-on-webgpu.md)) gets its device with `Gpu::request`, shows
the frame on a canvas configured `Bgra8Unorm` or `Rgba8Unorm`, and asks for `take` again at each animation
frame. In Chrome its frames are 0 pixels apart from the reference on Metal and on SwiftShader.

## Working on it

```bash
cargo xtask gpu                        # the gate: lints, tests, budgets, 271 scenes against the reference
cargo xtask gpu --fast                 # only the scenes, built incrementally: the edit loop
cargo xtask gpu --fast -- --scene cards --pictures    # one group, with PNGs in target/gpu-check/
cargo xtask gpu --fast -- --random 3000               # 3,000 random scenes: look for what the fixed ones miss
cargo xtask gpu --study                # the options of ADR 0021 on this machine's adapter
```

- `src/pack.rs` is the CPU side: a render list becomes bytes. It needs no GPU and has unit tests.
- `src/shader.wgsl` is what the GPU draws. `src/blit.wgsl` is `present`.
- `src/lib.rs` is the `Renderer`: pipelines, passes, the copy of the frame.
- The scenes are in `../gpu_check/src/scenes.rs`. A failing scene prints its pixels and writes
  `target/gpu-check/<scene>.{reference,gpu,diff}.png`, and its scene file if it was made there.
- **A visual rule goes into the reference renderer first**, then here, then a scene for it under an iso and
  a perspective camera. Never fix a difference by changing what the reference draws or by adding a tolerance.
- After a change, run a few thousand random scenes as well as the gate. The hand-made scenes were green
  twice while a real fault was there.

## Budgets

`budgets.toml`, section `[players]`, is measured on this workspace by `cargo xtask gpu`. A window library
goes into `players/Cargo.toml`, never into the root workspace. Count what it adds before adopting it: on
Windows the proc-macro budget is already full (6 of 6; two come with Direct3D 12), and Linux has 65 of 130
rented crates.
