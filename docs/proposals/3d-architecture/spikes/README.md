# Spikes behind the 3D architecture proposal

Scratch programs that produced the numbers in [the proposal](../../3d-architecture.md). They are not engine
code and not part of the gate. Each is its own small workspace; build them one at a time. Three of them
download wgpu and its dependencies from crates.io on the first build.

| folder | what it measures | how to run |
|---|---|---|
| `shared/scene.rs` | The town, the camera and the light that both renderers use. Exact float operations only. | included by the others |
| `gpu_native` | N instanced boxes through wgpu 30 with no window: time until the GPU finished, and one frame written as raw RGBA. | `cargo run --release -- <output folder>` |
| `gpu_web` | The same through wgpu's WebGPU backend in a browser, with a hash of the pixels. | `cargo build --release --target wasm32-unknown-unknown`, then `wasm-bindgen --target web --out-dir web/pkg target/wasm32-unknown-unknown/release/gpu_web.wasm`, then serve `web/` and open it. Add `--features webgl` to the build to measure the size of the WebGL2 fallback. |
| `cpu_ref` | A deterministic CPU renderer for the same town, with depth and an owner per pixel; compares itself with `gpu_native`'s frames if they are in the output folder. | `cargo run --release -- <output folder>` |
| `web_probe` | Which browsers give WebGPU when run headless, and with which flags: the default and the fallback adapter, a device on each, a texture cleared and read back. Node only, no build. | `node web_probe/probe.mjs`; on GitHub's runners `.github/workflows/web-probe.yml` (ADR 0024) |
| `scale_bench` | Moving N actors on a grid with today's storage, with a cell index, and with positions in a column. | `cargo run --release` |

The engine's own scale figures in the proposal came from `rwgpu run` on generated scenes of N wandering
actors (`wander = { step_frames = 1 }` on every other cell).

Results on an Apple M5 Pro, 2026-11-25, are in the proposal's section 3.

### Choosing what the spikes run (environment variables)

All optional. Without them the spikes run exactly as above and give the same pictures.

| variable | read by | effect |
|---|---|---|
| `WGPU_BACKEND` | wgpu (`gpu_native`) | `vulkan`, `gl`, `dx12`, `metal`, or a comma list. wgpu's own variable. |
| `WGPU_ADAPTER_NAME` | `gpu_native` | Use the first adapter whose name contains this text. |
| `WGPU_FORCE_FALLBACK_ADAPTER=1` | `gpu_native` | Only software adapters (`force_fallback_adapter`). |
| `SPIKE_BOXES` | both | Town sizes, for example `5000,50000`. Default `5000,50000,500000`. |
| `SPIKE_SHADER` | `gpu_native` | `smooth` (the study's shader, default), `flat` (the color is not interpolated across a triangle) or `flat-rounded` (flat, and rounded to a whole 8-bit level in the fragment shader). |

`gpu_native` first lists every adapter on every backend and the one wgpu would pick by itself, then names the
one it uses. On a slow (software) adapter it stops timing after about 3 s once it has at least 3 frames. Each
frame line ends with a hash of the picture. `cpu_ref` prints the same hash for its own picture, and for each GPU
frame in the folder it prints: how many pixels differ, the largest channel difference, the pixels grouped by
that difference, how many lie on an edge of the reference (a neighbour has another owner or color) and how many
took a neighbour's reference color.

## On GitHub's runners

`.github/workflows/gpu-probe.yml` runs both spikes on GitHub's hosted runners, which have no real graphics
card. It runs on pull requests that touch this folder or the workflow, and by hand (`gh workflow run
gpu-probe.yml --ref <branch>`). Each job prints its numbers and its timing to the job summary and uploads the
GPU frames. Measured 2026-11-26 in runs 45756166643 (cold cache) and 45757026172 (warm cache): wgpu 30.0.2,
Rust 1.100.0. A third run, 45757795785, gave the same picture hashes on every runner as the first two.

**What each runner offers.** Each job lists exactly one adapter, and it is the one wgpu picks by itself.
Nothing needed `force_fallback_adapter`.

| runner (image) | backend | adapter | wgpu device type | driver / driver info | installed |
|---|---|---|---|---|---|
| `ubuntu-latest` (ubuntu26 20261116, x64, 4 CPUs) | Vulkan | `llvmpipe (LLVM 21.1.8, 256 bits)`: Mesa lavapipe | `Cpu` | `llvmpipe` / Mesa 26.0.8 | `mesa-vulkan-drivers` (19 MB): 6, 23 and 28 s in three runs |
| `ubuntu-latest` | GL | `llvmpipe (LLVM 21.1.8, 256 bits)` | `Cpu` | GL 4.5 core, Mesa 26.0.8 | `libegl1 libegl-mesa0` (164 kB; the llvmpipe DRI driver is on the image): 4, 6 and 19 s |
| `windows-latest` (win25-vs2026 20261116, x64, 4 CPUs) | DX12 | `Microsoft Basic Render Driver`: WARP | `Cpu` | 10.0.26100.33648 | nothing. No GL or Vulkan adapter is listed |
| `macos-latest` (macos26 20261117, arm64, 3 CPUs) | Metal | `Apple Paravirtual device` | `IntegratedGpu` (not `VirtualGpu`) | none reported | nothing |
| Apple M5 Pro (the study's machine) | Metal | `Apple M5 Pro` | `IntegratedGpu` | none reported | — |

**Speed.** Median ms per frame with nothing uploaded, 1280 × 720; the two numbers are runs 1 and 2. 500,000
boxes took about half a second per frame on the software adapters. That is slow but bearable, so the probe still
runs them (7 to 11 timed frames). The CPU reference is in the last column for comparison: on the runners it is as fast as the
software adapters at 5,000 boxes and faster above.

| adapter | 5,000 | 50,000 | 500,000 | CPU reference, same runner (5,000 / 50,000 / 500,000) |
|---|---|---|---|---|
| lavapipe (Vulkan) | 9.1 / 7.5 | 69 / 49 | 479 / 364 | 7.4–7.9 / 32 / 223–226 |
| llvmpipe (GL) | 9.1 / 9.0 | 55 / 49 | 481 / 440 | 7.4–10.9 / 32–42 / 227–289 |
| WARP (DX12) | 8.4 / 5.6 | 63 / 41 | 452 / 265 | 5.2–8.3 / 20–32 / 129–207 |
| Apple Paravirtual (Metal) | 0.75 / 1.09 | 2.7 / 2.9 | 9.7 / 9.3 | 6.0–10.2 / 19–22 / 115–161 |
| Apple M5 Pro (Metal) | 0.5–0.8 | 0.7–0.9 | 2.0 | 5.5 / 11 / 62 |

WARP is slow to take uploads: re-uploading all instances every frame costs 27, 241–247 and 2,149–2,286 ms.

**Pixels that differ from the CPU reference**, of 921,600; in brackets the largest channel difference. The
variants were run at 5,000 and 50,000 boxes only.

| adapter | `smooth` 5,000 | `smooth` 50,000 | `smooth` 500,000 | `flat` 5,000 / 50,000 | `flat-rounded` 5,000 / 50,000 |
|---|---|---|---|---|---|
| lavapipe | 36,514 = 36,511 by 1 + 3 (100) | 35,421 = 35,408 + 13 (124) | 35,002 = 34,976 + 26 (180) | 50,966 / 49,804 | **3 / 13** |
| llvmpipe | 36,268 = 36,265 + 3 (100) | 35,511 = 35,501 + 10 (124) | 35,243 = 35,215 + 28 (180) | 50,966 / 49,802 | **3 / 10** |
| WARP | 36,831 = 36,828 + 3 (100) | 35,701 = 35,690 + 11 (124) | 35,422 = 35,394 + 28 (180) | 50,966 / 49,803 | **3 / 11** |
| Apple Paravirtual | 1 (84) | 8 (151) | 18 (180) | 1 / 8 | 1 / 8 |
| Apple M5 Pro | 1 (84) | 8 (151) | 18 (180) | 1 / 8 | 1 / 8 |

Two kinds of difference, and nothing in between (no pixel differs by 2 to 15):

- **Coverage, at edges.** 1 to 28 pixels per picture on every adapter, Metal included. Each is a lone pixel
  where two faces meet, painted in the other face's color, so it differs by 16 levels or more (up to
  180). A per-channel tolerance does not help here. Only a count does.
- **One level, inside faces, on the software adapters only.** All of these pixels are on three of the town's
  13 colors: the sides lit by ambient light only (k = 0.35), where color × 0.35 × 255 is an exact .5 tie
  (190, 230, 90 and 110 × 0.35 = 66.5, 80.5, 31.5, 38.5). The reference rounds a tie up. With `smooth`,
  lavapipe, llvmpipe and WARP interpolate the constant color across each triangle with a tiny error, so in
  irregular patches covering about 70 % of each such face it falls below the tie and rounds down. With `flat`,
  whole tie faces round down: the three software adapters write a tie one level lower than Metal does, and
  they give identical pictures at 5,000 boxes. With `flat-rounded` the shader writes a whole level, there is no
  tie left to round, and every one-level difference is gone.

**Are the GPU pictures equal to each other?** The macOS runner's pictures are byte-identical to the M5 Pro's at
all three sizes (`8b190eb126014d8e`, `7c901f08ce884e57`, `15292f1b72c6f518`). Every other pair differs. With
`smooth` the software adapters differ from each other in 16,000 to 25,000 pixels. With `flat-rounded`, lavapipe,
llvmpipe and WARP are identical at 5,000 boxes (`fd449cacfacbda6f`) but differ in 1 to 3 pixels at 50,000.
Metal differs from them in 2 and 8 to 9 pixels. So a GPU picture can't be the reference image across
systems. The CPU reference can: its hashes (`37eb4ea4454f9775`, `1e8b892209b292c1`, `c81d0bc855002257`) are
the same on all four runners and the M5 Pro.

**Time per job**, from the job's step times: run 1 with a cold cache, then run 2 with a warm cache. The run
step times benchmarks (60 frames or 3 s per size) at three sizes; a check that draws each picture once would
take well under a second per picture after the device is ready (0.1 to 0.5 s).

| job | toolchain + cache restore | drivers | build | run | whole job |
|---|---|---|---|---|---|
| ubuntu / Vulkan | 11 s / 17 s | 6 s / 23 s | 42 s / 1 s | 19 s / 22 s | 90 s / 91 s |
| ubuntu / GL | 12 s / 13 s | 4 s / 6 s | 49 s / 2 s | 19 s / 18 s | 95 s / 62 s |
| windows / DX12 | 20 s / 19 s | — | 121 s / 10 s | 24 s / 22 s | 192 s / 79 s |
| macos / Metal | 9 s / 13 s | — | 57 s / 8 s | 5 s / 3 s | 93 s / 35 s |

Run 2 also ran the two shader variants (15 to 17 s on the software adapters, 2 s on Metal), which are included
in its whole-job time. The 23 s Vulkan install in run 2 was apt itself: 9 s before it read its package lists
and 7 s in triggers. The 19 MB download took 1 s both times. So budget up to about 30 s for apt on Linux.

What these numbers suggest for the step-3 check (backends, the shader's color, the limit, the minutes) is in
the [progress log](../../../progress-log.md), entry of 2026-11-26.
