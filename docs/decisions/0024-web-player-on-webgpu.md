# ADR 0024: The web player on WebGPU — one module, the CPU reference renderer as its fallback, and a browser in the gate

- Status: accepted and built, 2026-11-26 (step 4 of the [3D proposal](../proposals/3d-architecture.md),
  piece 2 of 3; `rwgpu export web` is piece 3)
- Decided by: the owner approved WebGPU only on the web with the CPU reference renderer as the fallback and no
  WebGL2 (decision A2), the 1,500,000-byte web download budget (section 5) and "a player adds no visual
  feature" (A4). The shape of the player, the glue tool and the browser check were chosen by measurement; the
  tables are below. No budget limit was raised.
- Replaces, for the player: [ADR 0008](0008-web-via-c-abi.md) (a hand-written C ABI and no wasm-bindgen).
  ADR 0008 stands for the CPU module `rwgpuengine_web`.
- Builds on: [ADR 0021](0021-wgpu-renderer-held-to-the-reference.md) (the renderer and its check),
  [ADR 0022](0022-native-player-winit-and-rwgpu-play.md) (the native player, its loop and its bindings) and
  [ADR 0023](0023-release-builds-without-local-paths.md) (release builds).

## Context

`rwgpuengine_gpu` draws a render list with wgpu, natively and, compiled for wasm32, on WebGPU. The native
player puts it in a window. The browser had only the CPU player of ADR 0008: `rwgpuengine_web`, a wasm module
with plain C exports that paints with the CPU, driven by `web/player.js`. Step 4 is done when a link opens the
town. This piece is the player on WebGPU and the proof that it plays the engine's game.

## Measurements

### a. A browser with WebGPU, run without a person

`docs/proposals/3d-architecture/spikes/web_probe/probe.mjs` (Node only) runs each Chromium browser it finds
headless under six sets of flags and asks for the default and the fallback adapter; each gives a device,
clears a texture and reads it back. On GitHub's runners it is `.github/workflows/web-probe.yml`, run
45841919897. The picture column is the web player's (below), drawn at the end of all ten playtests.

| machine | browser | flags | WebGPU adapter | picture against the reference |
|---|---|---|---|---|
| Apple M5 Pro | Chrome 158.0.8127.38, Chrome for Testing 158.0.8127.18 | none | `apple metal-3` (Metal) | 0 of 307,200 pixels apart, ten playtests |
| Apple M5 Pro | the same | `--enable-unsafe-webgpu`, page asks for the fallback adapter | `google swiftshader` (software) | 0 apart, ten playtests |
| Apple M5 Pro | chrome-headless-shell 158.0.8127.18 | none / `--enable-unsafe-webgpu` | none / SwiftShader only | not used |
| `macos-latest` | Chrome 158.0.8127.37, Edge 158.0.4370.33 | none | `apple` (the Paravirtual device) | see [CI](#what-ci-proves-on-each-runner) |
| `windows-latest` | Chrome 157.0.8103.79 | none, or `--enable-unsafe-webgpu` alone | none | — |
| `windows-latest` | Chrome 157.0.8103.79 | `--enable-unsafe-webgpu --ignore-gpu-blocklist` | `microsoft warp` (Direct3D 12, software) | see CI |
| `windows-latest` | Chrome 157 / Edge 157.0.4341.60 | `--enable-unsafe-webgpu --use-webgpu-adapter=swiftshader` / `--enable-unsafe-webgpu` | SwiftShader | not used |
| `ubuntu-latest` | Chrome 157.0.8103.78 | none | none | — |
| `ubuntu-latest` | Chrome 157.0.8103.78 | `--enable-unsafe-webgpu`, with any of `--enable-features=Vulkan`, `--ignore-gpu-blocklist` | `google swiftshader` (software); Mesa's lavapipe, installed or not, never shows | see CI |

- Every runner gives a WebGPU adapter headless, with no new software: Chrome is on all three images and Node
  drives it over the DevTools protocol on a pipe. The gate passes `--enable-unsafe-webgpu
  --ignore-gpu-blocklist` everywhere; on this Mac they change nothing about the default adapter.
- **On SwiftShader, Chrome loses a device that shows a WebGPU canvas.** Found on the Linux runner (run
  45849007609: "the GPU could not map the frame for reading"), then shown in plain WebGPU, without wgpu:
  web_probe's canvas test draws 60 frames into a 640 × 480 texture, shows each on a WebGPU canvas, then
  reads the texture back. On the Linux runner (run 45852696750) it fails under all eight flag sets, with
  Mesa's lavapipe or without: `mapAsync` rejects with "A valid external Instance reference no longer
  exists", and the device is lost, reason `destroyed` ("Device was destroyed") or `unknown`. On the Windows
  runner it fails the same way with the SwiftShader adapter and works with WARP; on the macOS runner it
  works under every flag set. On this Mac it fails with SwiftShader unless `--enable-unsafe-swiftshader` or
  `--use-angle=swiftshader` is given; on the Linux runner those two do not help, and on the Windows runner
  the first kept a fresh tab from drawing after a device was destroyed, so the gate does not pass it.
  Without the canvas the read-back works everywhere. A replay that never yields to the browser can finish
  before the loss arrives, which is how a first run here passed by luck.
- **The fault this found in the player.** Chrome reports that loss as reason `destroyed`, which `Gpu` ignores
  (natively it means the program dropped its own device). The web player now records every loss the browser
  reports and goes on with the CPU reference renderer, saying "the browser lost the device (destroyed): …
  [E0503]". Shown on this Mac with SwiftShader as the default adapter: all four Tiny Town playtests reach
  their states, drawn by the CPU after the loss.
- **SwiftShader's edges depend on how Chrome starts it.** On this Mac, SwiftShader as the fallback adapter
  draws all ten playtests 0 pixels apart from the reference; under `--use-webgpu-adapter=swiftshader` it
  draws `turn_and_build` 61 apart, every one a lone pixel on a slanted edge of the perspective view, and the
  other nine 0. The adapter's info, features and limits are the same both ways.
- On Linux the adapter is SwiftShader, Chrome's own CPU implementation of Vulkan under its WebGPU (Dawn).
  It is a real WebGPU: the page, wgpu's WebGPU backend, the browser's WebGPU and its canvas all run.
- Chrome on Windows names WARP only without its GPU blocklist. Edge gives SwiftShader with
  `--enable-unsafe-webgpu` alone.

### b. The shape of the download

Bytes a browser downloads for the player, before a game's own files; gzip `-9` in brackets. All measured with
the same plain `web` profile build (the release build of the chosen shape, remapped by ADR 0023, is 670,234
bytes of wasm). Crates and proc macros: `cargo tree --workspace --target wasm32-unknown-unknown -e normal` in
`players/`, as xtask counts the other systems.

| shape | WebGPU path | fallback path | wasm32 crates (proc macros) | after a lost device |
|---|---|---|---|---|
| **one module with the CPU painter, the page's own script** | **770,648 (297,321)**: 673,058 wasm, 78,924 glue, 13,886 script, 4,780 page | the same file, nothing more | **54 (4)** | the same session goes on, painted by the CPU |
| two modules: the GPU module without the painter, and `rwgpuengine_web` when WebGPU is missing | 758,829 (290,148): 661,239 wasm | 645,293 (238,928): `rwgpuengine_web.wasm` 626,627 | 54 (4), and the CPU module's 2 | the game restarts in the other module, or its input is replayed there |
| one module on winit's web backend (`EventLoop::spawn_app`, the window on the canvas) | about 938,000 (about 350,000): 819,049 wasm, 107,233 glue | the same | 62 (5): `pin-project-internal` | the same as the first |

- **One module, the page's own script.** The painter costs 11,819 bytes of wasm (7,199 gzipped) on the
  WebGPU path. Two modules would save that and 125 KB on the fallback path, but the fallback would be a
  second program: a device lost in the middle of a game would end it, and the bindings would be written twice,
  in Rust for the GPU module and in JavaScript for the C ABI, which has no camera and no pan solver. One
  module has one set of bindings, the native player's own code.
- **The page's own script, not winit.** winit's web backend adds 146 KB of wasm, 28 KB of glue, 8 crates and a
  proc macro on wasm32, and still leaves the canvas's size in device pixels, the fetches, hot reloading and
  the page's panels to JavaScript. The page's script that does all of it is 13,886 bytes.

### c. The glue: the `wasm-bindgen` tool

wgpu's WebGPU backend is written on `web-sys`, so the module's imports are wasm-bindgen's and only the tool of
the crate's exact version can write their JavaScript. Measured on an Apple M5 Pro:

| option | where it comes from | time | in the players' tree |
|---|---|---|---|
| **`cargo install wasm-bindgen-cli --version =0.2.130 --locked --bin wasm-bindgen --root target/tools/wasm-bindgen-0.2.130`, by xtask on first use** | the crate's source from crates.io, with its own published lockfile | 23.9 to 25.4 s (162 crates), then 0.05 s per run | nothing: it is a program run at build time, not a dependency; xtask counts the workspace's normal dependencies, and the tool is not one |
| a prebuilt program from the tool's release page | a download of a program | seconds | the same; not taken: it would be the one program the build downloads |
| `wasm-bindgen-cli-support` as a library of xtask or of a players' crate | crates.io | about the same | its tree in a counted tree (the agent's half, or the players'), with proc macros |

- xtask looks for that exact version in `WASM_BINDGEN`, then in `target/tools/`, then on `PATH`, and installs
  it into `target/tools/` (inside the repository's build folder, nowhere else) when none is it. A failed
  install is **E0007** `web-tool-missing`, with the command to install it by hand.
- The players' manifest pins the crate (`wasm-bindgen = "=0.2.130"`), and a test keeps the pin and the
  lockfile equal. CI caches `target/tools` with the build (`Swatinem/rust-cache`, `cache-directories`).

## Decision

1. **One module.** The web player is `rwgpuengine_player`'s library built for wasm32 as a cdylib (its `web`
   module), with wasm-bindgen 0.2.130. The window, winit and the command line are native-only dependencies
   of the same crate. `cargo xtask web-player` builds it as a release (ADR 0023's flags, profile `web`:
   size-first, the renderer and the core at `opt-level = 3`, as ADR 0017), writes the glue and copies the
   page: `index.html`, `player.js`, `rwgpu_web.js`, `rwgpu_web_bg.wasm` in `target/web-player/`. Every file is
   checked for paths of this machine. `cargo xtask dist` builds it too and puts it beside the release `rwgpu`.
2. **One simulation, the engine's,** as in ADR 0022: `Session::step` with the live input at the project's
   tick rate, `Session::render_list`, `Renderer::draw`, `Renderer::present` onto the canvas. What is shared
   with the native player, unchanged: `play::Player` (step, build the list, draw only when it changed),
   `fit` (letterbox, canvas pixels to frame pixels), `clock::Steps` (fixed steps, at most five at a wake-up),
   `input` (keys, buttons, the wheel, pinch, camera moves), `play::pan_move` and `play::key_move`. What could
   not be shared: `clock::Watch` reads `performance.now()` in a browser (`std::time::Instant` does not exist
   there); `gpu::Gpu::open` blocks natively, so the browser calls `Gpu::request` and awaits it; and the
   window's event loop is the page's animation frames.
3. **WebGPU, and the fallback.** The page asks for WebGPU when `navigator.gpu` exists. Where it does not,
   where the browser gives no adapter or no device (E0501, E0503), or where the device is lost while playing,
   the reference's painter draws the frame (`Presenter`) and the page puts its pixels on a 2D canvas of the
   frame's size. The game goes on where it was: it is the same session. One line under the canvas says which:
   `WebGPU · apple metal-3`, or `CPU reference renderer · this browser has no WebGPU`. No WebGL2.
4. **The same input.** The page hands the module `KeyboardEvent.code`, pointer events and wheel events; the
   bindings are the native player's table, now keyed by W3C key codes (winit's `KeyCode` names are the same,
   and a test holds them equal). Added for touch screens: one finger taps and swipes, two fingers pinch. A
   test keeps the page's help naming every bound key.
5. **The canvas.** On WebGPU the canvas covers its device pixels (`devicePixelContentBoxSize`) and the frame
   is scaled onto it as in the window: the largest rectangle with the frame's aspect, nearest pixel, bars
   around it. On the CPU path the canvas is the frame's size and CSS scales it with `image-rendering:
   pixelated`. A pointer is taken to frame pixels by `fit::to_frame`.
6. **Time.** Each animation frame takes the steps that are due at the project's rate (at most five; more
   lost time is let go), builds the list once, and draws and shows it only if it changed. A hidden tab gets
   no animation frames, so it costs nothing; when it comes back, at most five steps are caught up.
7. **`rwgpu serve`** serves the WebGPU player when it finds its folder: `--web-player DIR`, then
   `RWGPU_WEB_PLAYER`, then `web-player` beside the `rwgpu` program, then (debug builds only) the workspace's
   `target/web-player`. Otherwise it serves the CPU player of ADR 0008, and it is not an error. Its first
   line says which it serves and where it looked. `rwgpu` links nothing of `players/`: it serves files.
   `--port 0` picks a free port and prints it. Each connection is answered on a thread of its own and is
   dropped after 10 s of silence: answered one at a time, as before, a connection the browser opened ahead
   of time and left idle held up every request behind it, and on the Windows runner a page then never
   started (run 45861940621).
8. **The browser check, in `cargo xtask gpu`.** `players/rwgpuengine_player/web/check.mjs` starts
   `rwgpu serve` for each example and a headless Chromium with a profile folder of its own under
   `target/web-check/`. For every playtest of the three examples, on the WebGPU path and on the CPU path, the
   page plays it to its end as fast as the steps go, drawing every frame whose list changed:
   - the state hash must be `rwgpu test`'s, at its frame, on both paths, and every playtest must be drawn by
     the path it was meant for: a device lost on the way would hand the game to the CPU and still reach the
     state. The page lets three animation frames pass after a replay, so that a loss the browser reports
     late is seen;
   - the frame WebGPU drew (`Renderer::copy_frame`, read back in the page) goes to `gpu_check --web-frames`,
     which replays the playtest natively and holds the two pictures to `pixels_apart` with every differing
     pixel on an edge, the rule of ADR 0021;
   - on the CPU path the canvas is read back, and its bytes must hash to `rwgpu shot`'s image hash;
   - on Tiny Town: a playtest replayed in real time, the device lost at frame 45 of `turn_and_build` (the CPU
     path must take over at the same state), the scene left alone for 3 s (one frame drawn), and the camera
     turning for 3 s (time per drawn frame).
   Without Node or a browser the check is skipped on a developer's machine and fails in CI.
   `RWGPU_BROWSER_RUNS` repeats it.
9. **Budgets.** `[players] web_download_bytes = 1,500,000`: the four files, uncompressed, as `rwgpu serve`
   sends them. The players' crate and proc-macro counts now include the workspace on wasm32.

## Evidence

On an Apple M5 Pro, Chrome 158 headless:

- **Same state.** All ten playtests reach `rwgpu test`'s state hashes on WebGPU (Metal), on the fallback
  adapter (SwiftShader) and on the CPU path.
- **Same picture.** The 20 WebGPU frames (ten on Metal, ten on SwiftShader) are 0 pixels apart from the
  reference. On the CPU path the canvas holds the reference's pixels exactly: ten image hashes equal to
  `rwgpu shot`'s.
- **A lost device:** `turn_and_build` drawn by WebGPU to frame 45, the device destroyed, then by the CPU
  path to frame 90: state `5775c67554808f1a`, the playtest's.
- **Times.** Navigation to the first frame: 39 to 95 ms on WebGPU (the first page a browser loads is the
  slow one), 31 to 52 ms on the CPU path. Tiny Town with the camera turning for 3 s: 180 or 181 frames
  drawn, per drawn frame (means) 23 to 44 µs to build and compare the list, 162 to 297 µs in
  `Renderer::draw`, 66 to 98 µs to show it on the canvas: about 0.25 to 0.4 ms of the page's time.
  `turn_and_build` in real time: 90 frames in 1,505 to 1,529 ms, no step let go. Left alone for 3 s: 178
  steps, one frame drawn.
- **Runs.** The browser check ran 5 times in a row in one `cargo xtask gpu`, 3 times in another and 4 times by
  hand: green 12 of 12 times. The whole check takes 9 s, 8 of them the real-time measurements.

| budget | before | now | limit |
|---|---|---|---|
| players: web download | — | 767,824 bytes (297,481 gzipped) | 1,500,000 |
| players: rented crates, widest system | Linux 105 | Linux 105, Windows 87, macOS 84, wasm32 54 | 130 |
| players: proc-macro crates, widest system | Linux 6, Windows 6 | the same; macOS 5, wasm32 4 | 6 |
| the CPU web module | 626,627 bytes | unchanged | 800,000 |
| root `Cargo.lock` | 33 packages | unchanged | — |

The wasm32 proc macros are `bytemuck_derive`, `document-features`, `thiserror-impl` 2 (all with wgpu) and
`wasm-bindgen-macro`.

## What CI proves on each runner

The `gpu` job runs the whole browser check inside `cargo xtask gpu --window`, three times in a row
(`RWGPU_BROWSER_RUNS=3`). (Filled in from the pull request's runs.)

## Consequences and limits

- `cargo xtask gpu` and `cargo xtask dist` now need the `wasm-bindgen` tool; the first run installs it
  (25 s here). `cargo xtask ci` does not: the agent's half and its CPU module are unchanged.
- The CPU player of ADR 0008 is what `rwgpu serve` gives without the players' build. It keeps its bindings:
  keys, taps and swipes, no camera. The WebGPU player's fallback has every binding.
- On the runners WebGPU is a software adapter (SwiftShader, WARP) or a virtual GPU: the check proves the code
  path and the picture, not the speed of a real GPU in a browser.
- `performance.now()` is rounded to 100 µs on a page that is not cross-origin isolated; the times above are
  means over many frames for that reason.
- The adapter's name: wgpu reads only `GPUAdapterInfo.description`, which Chrome leaves empty; the page names
  the adapter from `GPUAdapter.info` itself.
- Awkward from a browser, and left as it is: `Player::update` and `draw` take a `Watch`, which had to learn
  the page's clock; `Gpu::open` blocks; `FrameCopy::take` must be polled across animation frames, so the page
  waits for it in a loop of `setTimeout(0)`; a device lost on purpose (`destroy`) is reported by wgpu as
  "destroyed", which `Gpu` ignores, so the check's `lose_device` flags the loss itself.
- Not tested in a person's browser on a phone or a tablet; touch was tested by unit tests of the bindings only.

## Not done

`rwgpu export web` and hosting (piece 3), WebGL2, offline or installable pages, sound, gamepads, phone and
tablet shells, full screen, the camera in the CPU player of ADR 0008.

## Revisit when

wasm-bindgen or wgpu changes version (the tool's version follows the crate's), a runner's browser stops giving
an adapter headless, the web download nears its budget, or winit's web backend becomes smaller than the page's
script.
