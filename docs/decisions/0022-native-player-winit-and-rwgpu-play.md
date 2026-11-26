# ADR 0022: The native player — `rwgpu-player` plays a project in a winit window, and `rwgpu play` starts it

- Status: accepted and built, 2026-11-26 (step 3 of the [3D proposal](../proposals/3d-architecture.md),
  piece 3 of 3: the window around the renderer)
- Decided by: the owner approved step 3 ("the same town in a window on three systems, within the pixel
  limit"), the two halves, and "a player adds no visual feature". The window library and the shape of the
  player were chosen by measurement; the tables are below. No budget limit was raised.
- Builds on: [ADR 0021](0021-wgpu-renderer-held-to-the-reference.md) (the renderer and its check),
  [ADR 0020](0020-render-list-camera-and-cpu-reference-renderer.md) (the camera as session state),
  [ADR 0010](0010-input-actions-taps-scripts.md) (actions, taps, scripts) and
  [ADR 0018](0018-paint-path-for-players.md) (a player draws only what changed).

## Context

`rwgpuengine_gpu` draws a render list with wgpu into its own frame. A person needs it in a window, with
keys, clicks and the camera; an agent needs to prove what the window did without watching it. `rwgpu` must
stay GPU-free, and nothing here may make the web player of step 4 harder.

## Decision

1. **winit 0.30.13**, the exact version, with X11 and Wayland, both loaded at run time
   (`rwh_06, x11, wayland, wayland-dlopen`), and without `wayland-csd-adwaita`. The measured options are in
   the first table below. 0.31 is still a beta.
2. **`players/rwgpuengine_player`**, a library and the program `rwgpu-player`. `rwgpu-player <project>`
   loads the project, opens a window and plays it. The library has the parts the check also uses:
   `play::Player` (step, build the list, draw it only when it changed), `clock`, `fit`, `input`, `gpu`,
   `window` and `cli`.
3. **One simulation, the engine's.** The player calls `Session::step` with the live input at the project's
   `tick_rate`, `Session::render_list`, `Renderer::draw` and `Renderer::present`. Gestures call
   `Session::move_camera`. It has no game rule, no drawing rule and no visual feature of its own.
4. **No second loader.** The players' half depends on the library of `rwgpuengine_cli` by path (only its
   library is built there, never `rwgpu`). The player's arguments are the `play` entry of
   `rwgpuengine_cli::args::COMMANDS`, parsed by the same parser. The project is found and loaded by
   `rwgpuengine_cli::commands::load_session`, made public for this, with `--input` and `--seed` as in
   `rwgpu run`. Results and diagnostics are printed by `rwgpuengine_cli::out::emit`, `--json` included, so
   the player's output is `rwgpu`'s envelope with `"command": "play"`.
5. **The clock lives in the player.** Steps are counted from the start in whole nanoseconds; the loop
   sleeps until the next step is due (`ControlFlow::WaitUntil`) or an event comes. A wake-up takes the steps
   that are due, at most five. When more were due (a dragged window, a laptop waking up), it takes five and
   lets the rest of the lost time go: the game slows down for a moment and never takes a longer step. The
   list is built once after the steps, compared with the one drawn, and drawn and shown only if it changed.
   A gesture rebuilds it at once. `clock.rs` is the one place that reads the wall clock.
6. **The window.** It opens at the frame's size in logical pixels, so on a display that scales by 2 every
   frame pixel is a block of 2 × 2. The frame is scaled to the largest rectangle with its aspect that fits,
   with black bars, nearest-pixel. The surface format is `Bgra8Unorm` or `Rgba8Unorm` where the adapter
   offers one: the frame's bytes then reach the screen as they are. Resizing reconfigures the surface and
   shows the frame again without drawing it. The title is the project's name.
7. **Input**, as `web/player.js` gives it, so a keyboard, a tablet and a playtest give the same
   `LiveInput`. Camera bindings use keys and buttons the game does not.

   | input | does |
   |---|---|
   | arrows, W A S D | up, down, left, right |
   | Enter, Space / Escape | confirm / cancel |
   | left click (moved less than 12 frame pixels) | a tap at the frame pixel under the press: one tap per step |
   | left drag (12 frame pixels or more) | a swipe along the iso axes: up-right is up |
   | Q / E | turn −90° / +90° |
   | right-drag | turn, 0.5° per frame pixel; the side nearest the viewer follows the pointer |
   | middle-drag, or Shift + right-drag | pan: the ground under the center follows the pointer |
   | wheel (40 scrolled pixels are a notch), pinch, - and = | zoom by 1.1 a notch, by the pinch, by 1.25 a key |
   | V | `iso` ↔ `perspective` |
   | Home | back to the scene's camera, as one move |

   A pan is solved through `Session::screen_point`: one cell's screen directions give a first answer,
   exact in the iso view, and two corrections make it exact to 0.05 pixel in perspective. Turning, zooming
   and panning are the camera's own moves; there is no tilt. A test keeps the bindings and the text of
   `rwgpu help play` equal.
8. **Problems are diagnostics with codes**, printed as `rwgpu` prints them: E0501 `no-gpu-adapter`, E0502
   `no-window-system`, E0503 `gpu-failed` (a refused device, a surface error, an error the device reports
   while drawing, a failed read-back). A project that does not load gives the loader's own diagnostics.
9. **Made to be checked without a person.** `--frames N` closes after N frames and prints the frame and the
   state hash; with `--input` the default is the playtest's frames. `--headless` plays the same steps as
   fast as they go and draws each changed frame off screen. `--screenshot FILE` saves what the window
   showed: read back from the window's own surface after it was shown, or, where a surface cannot be read,
   drawn again into a texture of its size (the result says which). The result also has the adapter, the
   frames drawn and the time each part took.
10. **`rwgpu play`** is a command of the agent's half. It looks for `rwgpu-player` in the folder of the
    `rwgpu` program, then in each folder on `PATH`, and runs it with the same arguments; the player prints
    the result. `rwgpu` links nothing from `players/`. Without the player it is E0006 `player-not-found`,
    with the hint `cargo install --locked --path players/rwgpuengine_player`. Next to itself first, so a
    package that ships both programs in one folder never picks up another version from `PATH`.

## The window library: measured options

Rented crates in the players' tree (renderer, check and a window library), and proc-macro crates in
brackets, counted as `cargo xtask gpu` counts them (`cargo tree --target`, normal edges; a proc macro once
per version in that system's tree). Limits: 130 crates and 6 proc macros on the widest system.

| option | Linux | Windows | macOS | gives up |
|---|---|---|---|---|
| no window (piece 2) | 65 (4) | 76 (6) | 67 (4) | |
| winit 0.30.13, default features | 114 (6) | 87 (6) | 84 (5) | nothing |
| winit 0.30.13, X11 only | 86 (5) | 87 (6) | 84 (5) | native Wayland: runs through XWayland, which a fractional scale blurs |
| winit 0.30.13, Wayland only | 99 (6) | 87 (6) | 84 (5) | X11 desktops, and `xvfb` in CI |
| **winit 0.30.13, X11 + Wayland, no drawn title bar** | **105 (6)** | **87 (6)** | **84 (5)** | on GNOME under Wayland the title bar is plain: buttons, no title text |
| winit 0.31.0-beta.4, default | 118 (5) | 90 (6) | 81 (4) | a beta; the API changes again |
| winit 0.31.0-beta.4, X11 + Wayland | 109 (5) | 90 (6) | 81 (4) | the same |
| SDL3 (`sdl3` 0.22.1) | 67 (4) | 79 (6) | 69 (4) | the SDL3 C library on every machine, or built from source with CMake |
| GLFW (`glfw` 0.63.0) | 67 (4) | 79 (6) | 70 (4) | the GLFW C library, the same way |
| minifb 0.29.0 | 86 (5) | 79 (6) | 69 (4) | a pixel-buffer window polled by `update`, with a frame-rate limiter for waiting |

- The window library adds no proc macro on Windows; Windows stays at 6 of 6 because of Direct3D 12.
  winit brings `thiserror` 1 on Linux (through `calloop`), so Linux builds two versions of
  `thiserror-impl`; with `wayland-scanner` that is 6 of 6 there too. Before this change the gate matched
  proc macros by name across the whole lockfile and counted the Linux-only `thiserror-impl` 1.0.69 on
  Windows and macOS as well (7 and 6); it now matches name and version.
- The drawn Wayland title bar costs 9 crates (`sctk-adwaita` with a font rasterizer and `tiny-skia`). Native
  Wayland on top of X11 costs 19 crates and `wayland-scanner`. X11 only is the lever if the Linux count gets
  tight; it would make Windows the widest system at 87.
- SDL3 and GLFW count few Rust crates because the window system is a C library that the count does not see;
  building it needs CMake and a C compiler on each runner, or an installed library. That is a different
  dependency rule (ADR 0004 counts what we build), not a smaller one. minifb has no event wait, so a still
  scene would wake on a timer of its own choosing.

## Evidence

All ten example playtests, measured on an Apple M5 Pro (Metal) unless a runner is named.

- **Same state.** The player's loop with no window (in `gpu_check`) and the `rwgpu-player` program
  (`--headless`, run by `cargo xtask gpu`) end every playtest in the state hash `rwgpu test` reports, at
  the playtest's frame. `rwgpu play` finds the program on `PATH` and reaches the same hash.
- **Same picture.** The last frame the player drew, read with `copy_frame`, is 0 pixels apart from the
  reference picture of the same list in every playtest (limit 24, all on edges).
- **A real window on this Mac.** `rwgpu play examples/tiny_town --input tests/turn_and_build.toml
  --screenshot target/player/turn_and_build.png` opens a 1280 × 960 window (scale 2), replays the playtest
  in 1.7 s, closes, exits 0, and prints `5775c67554808f1a`, the playtest's hash. The picture read back
  from the window is the reference picture at twice its size: 0 of 307,200 frame pixels apart, each one a
  block of one color.
- **Time per drawn frame, Tiny Town in the window**, the camera turning every step for 5 s (298 frames
  drawn): building and comparing the list 38 µs (median), `Renderer::draw` (prepare and submit) 589 µs
  (median, at most 1.0 ms), showing it on the window 214 µs. About 0.8 ms of CPU per frame.
- **Idle.** Tiny Town with nothing happening: 0.08 to 0.09 s of CPU for 10 s more of play (an 11 s run
  against a 1 s run, user and system time, three times), 0.8 to 0.9 % of one core. One frame was drawn in 11 s; the loop woke 60 times
  a second to step.
- **CI.** See [the window on GitHub's runners](#the-window-on-githubs-runners).

| budget | before | now | limit |
|---|---|---|---|
| players: rented crates, widest system | Windows 76 | Linux 105 | 130 |
| players: proc-macro crates, widest system | Windows 6 | Linux 6, Windows 6 | 6 |
| players: clean release build (now the whole players' workspace) | 12–13 s | 13.6–17.8 s | 60 s |
| `rwgpu` | 1,979,136 bytes | 2,014,768 | 3,000,000 |
| web module | 619,866 bytes | 624,202 (the new codes' explanations) | 800,000 |
| root `Cargo.lock` | 33 packages, no GPU or window crate | unchanged | — |

`cargo install --locked --path players/rwgpuengine_player` builds and installs the player in 21 s on the
same machine; the program is 5,988,384 bytes.

## The window on GitHub's runners

GitHub's runners have no graphics card. Each tried the real window: `rwgpu-player` replaying
`turn_and_build` in a window, five times in a row, with `--screenshot`. Linux has no desktop, so it ran under
`xvfb-run` (with `xvfb` and `libxkbcommon-x11-0` installed; winit takes X11 when there is no Wayland).

| runner | adapter | windows opened / tried | each replay | state hash | picture read back from the window against the reference |
|---|---|---|---|---|---|
| `ubuntu-latest`, Xvfb | Mesa lavapipe (Vulkan) | 5 / 5 | 2 s | `5775c67554808f1a` every time | 1 of 307,200 pixels apart |
| `windows-latest` | WARP (Direct3D 12) | 5 / 5 | 2 s | the same | 0 apart |
| `macos-latest` | Apple Paravirtual device (Metal) | 5 / 5 | 2 to 4 s | the same | 0 apart |

The windows open at 640 × 480 (scale 1) on all three, and every screenshot came from the window's own surface.
On Linux with no display at all the player stops with E0502 and winit's sentence ("neither WAYLAND_DISPLAY nor
WAYLAND_SOCKET nor DISPLAY is set"). Because it worked every time, the `gpu` job runs `cargo xtask gpu
--window` on all three systems, Linux under `xvfb-run`; the windowless checks run there as well.

In the same run the ten playtests replayed by the player reached the engine's hashes on all three runners, and
their last frames were 0 pixels apart on Metal and WARP and 1 in all on lavapipe. Clean release builds of the
players' workspace: 96 s on Linux, 69 s on Windows, 99 s on macOS (the limit on CI is 240 s).

The `gpu` job took 6 min 58 s on Linux, 6 min 26 s on Windows and 10 min 48 s on macOS, with winit and the
player compiled from nothing and the five probe replays included. Before this change it took 1 min 44 s to
2 min 10 s, 3 min 46 s to 3 min 51 s and 2 min 22 s to 2 min 55 s warm.

## Consequences and limits

- `rwgpu play` needs the player installed beside `rwgpu` or on `PATH`; a clone has to build it once.
- The players' crate counts now include winit: Linux has 25 crates left, and Linux and Windows have no proc
  macro left. A dependency that brings a proc macro to either needs the owner's decision.
- The clean release build times the whole players' workspace (check and player), not the check alone.
- Closing the window early ends the run; the result says so (`"finished": false`). On macOS the Quit menu
  item ends the process without printing a result.
- A still scene still steps 60 times a second: the world is not known to be still until a step is taken.
- The frame is scaled with nearest pixels; at a scale that is not whole, frame pixels are one window pixel
  wider or narrower than their neighbors.

## Not done

The web player on WebGPU (step 4), `rwgpu export web`, sound, gamepads, touch screens, full screen, a file
for key bindings, packages and installers, reloading files while playing (the web player does it), an
`[[expect]]` on the camera.

## Revisit when

A game needs keys the bindings take, a second window or full screen, winit 0.31 is released, or a
dependency would bring a proc macro to Linux or Windows.
