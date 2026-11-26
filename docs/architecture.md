# Architecture

How rwgpuengine is built, as of v0.1.0 (first milestone). Decisions behind each choice are in
[decisions/](decisions/). The vision is in [vision.md](vision.md).

## Crates and layers

```text
                ┌──────────────────────────────────────────────────────────────┐
 agent surface  │ rwgpuengine_cli   `rwgpu` binary + library: commands, --json,       │
                │             help/docs from registries, dev server (serve)      │
                │ rwgpuengine_web   wasm32 cdylib, C ABI for web/player.js (the CPU player) │
                └───────────────┬──────────────────────────────┬───────────────┘
                                │                              │
                ┌───────────────▼──────────────────────────────▼───────────────┐
 composition    │ rwgpuengine_runtime  Session (project+scene+script), stepping,      │
                │                tap picking, inspect views, playtests, Game     │
                └──────┬───────────────────────┬────────────────────┬──────────┘
                       │                       │                    │
 core runtime   ┌──────▼──────┐        ┌───────▼───────┐            │
                │ rwgpuengine_sim   │◄───────┤ rwgpuengine_render  │            │
                │ world, step,│        │ camera, render│            │
                │ signals,    │        │ list, CPU ref.│            │
                │ event log   │        │ render, report│            │
                └──────┬──────┘        └───────┬───────┘            │
                       │                       │                    │
                ┌──────▼───────────────────────▼────────────────────▼──────────┐
 text + schema  │ rwgpuengine_data  TOML → span tree → schema check → typed defs;     │
                │             Vfs, loaders, static checks (`rwgpu check`)        │
                └──────────────────────────────┬───────────────────────────────┘
                ┌──────────────────────────────▼───────────────────────────────┐
 foundation     │ rwgpuengine_core  JSON, diagnostics + code registry, PCG32, FNV,    │
                │             colors, suggestions. Zero dependencies.            │
                └──────────────────────────────────────────────────────────────┘
 rented         toml_edit (parsing) · png (encode/decode)
 dev tooling    xtask: ci, repo-check, dist, web, web-player, smoke, budgets, docs, wasm-check, gpu
```

Dependency rule: arrows only point down. `rwgpuengine_sim` knows nothing about rendering; `rwgpuengine_render` reads
the world but never changes it; only `rwgpuengine_runtime` holds both (picking needs the drawn frame).

### Two halves

Everything above is the **agent's half**: it never needs a GPU, and `cargo xtask ci` is its gate. The
**players' half** is a second Cargo workspace, `players/`, with its own `Cargo.lock`, its own budgets and
its own gate, `cargo xtask gpu` ([ADR 0021](decisions/0021-wgpu-renderer-held-to-the-reference.md)).

```text
 players/                                              the agent's half (crates/)
 ┌──────────────────────────────────────────────┐
 │ rwgpuengine_player  `rwgpu-player`: a winit  │────► rwgpuengine_cli     (play's arguments, load_session,
 │                  window; and, built for      │                           output and diagnostics; native only)
 │                  wasm32, the web player on a │────► rwgpuengine_runtime (Session::step, render_list,
 │                  canvas. The clock, keys and │                           move_camera, Presenter)
 │                  taps; steps the Session     │
 │ gpu_check        draws every reference scene │────► rwgpuengine_runtime (sessions of the example games)
 │                  with both renderers and     │────► rwgpuengine_render  (the reference picture, owners)
 │                  compares the pictures;      │
 │                  replays playtests by player │
 │ rwgpuengine_gpu  a render list drawn with    │────► rwgpuengine_render  (the render list, the Projector,
 │                  wgpu; no window, no blocking│                           a card's place, a face's color)
 └──────────────────────────────────────────────┘
 rented         wgpu 30.0.2 (Vulkan, Metal, Direct3D 12, WebGPU) · winit 0.30.13 (X11, Wayland, AppKit, Win32)
                · wasm-bindgen 0.2.130 (the browser; its tool writes the page's glue) [later: audio, text]
```

The players' crates depend on the agent's, never the other way round. wgpu and winit are not in the root
`Cargo.lock`, so no command in the agent's half can compile them. `rwgpu play` runs the player as a separate
program, found next to `rwgpu` or on `PATH` ([ADR 0022](decisions/0022-native-player-winit-and-rwgpu-play.md)).
`rwgpu serve` serves the web player's built files as they are
([ADR 0024](decisions/0024-web-player-on-webgpu.md)).

| crate | owns | must not |
|---|---|---|
| `rwgpuengine_core` | JSON, `Diagnostic`, the code registry, RNG, hashing, colors | depend on anything |
| `rwgpuengine_data` | file formats: the schema registry, validation, typed definitions, `Vfs`, static checks | run anything |
| `rwgpuengine_sim` | `World`, entities, built-in behaviors, signals, event log, state hash | render, read files, use wall-clock time |
| `rwgpuengine_render` | the camera, the render list, the CPU reference renderer, frame report, picking, PNG | change the world |
| `rwgpuengine_runtime` | sessions, scripted input, playtests, the `Game` trait | touch the filesystem (the CLI does) |
| `rwgpuengine_cli` | commands, output envelope, docs generation, templates, dev server | contain engine logic |
| `rwgpuengine_web` | the CPU player's wasm C ABI | contain engine logic |
| `rwgpuengine_gpu` (players) | the render list drawn with wgpu: frame, copy to bytes, show on a target | open a window, block, decide what a frame looks like (the reference does) |
| `rwgpuengine_player` (players) | `rwgpu-player`: the window, the clock, keys, taps and camera gestures, `--headless` and `--screenshot`; the web player (`web`, for wasm32) and its page | have a game rule or a visual feature (the engine and the reference do), load a project its own way |
| `gpu_check` (players) | the scenes both renderers draw, the comparison, frame times | be depended on |

## Data flow of one command

```text
disk ──Vfs::from_dir──► Vfs ──load_project/SceneLoader──► ProjectDef, SceneDef ──World::new──► World
                         │                                   (diagnostics on failure)            │
                         └──load_playtest──► PlaytestDef ──Script──► InputFrame per step ─────────┤
                                                                                                    ▼
                     Session::step ×N  (camera moves, then taps resolved by picking the drawn frame)
                                                                                                    │
            inspect ◄── Session::inspect (world JSON + screen rects + events)                       │
            shot    ◄── list::build → project → render (pixels, owners) → build_report → PNG + report ◄──┘
            run     ◄── summary + state hash (+ events, + hash trace)
            test    ◄── run_playtest: [[expect]] at frames, [[shot]] captures → compare refs (CLI)
```

Every command is a **pure function of (files, input script, seed, frame)**. `shot --frame 300` replays
300 steps; nothing is cached between commands, so nothing can be stale.

## The file model

- `rwgpu.toml` marks the project root (found by walking up, like Cargo.toml).
- `scenes/*.toml`: `[scene]`, `[camera]`, `[ground]` (a size or a terrain `map`), `[[terrain]]`, `[[entity]]`,
  `[[connect]]`.
- `prefabs/<name>.toml`: an entity template; entity keys override it, component tables merge key by key.
- `tests/*.toml`: playtests — `[test]`, `[[input]]`, `[[expect]]`, `[[shot]]`.

Parsing pipeline (ADR 0003): `toml_edit` parses → converted at once into an owned `Node` tree that keeps
byte spans for every key and value → `Checker` validates against the **schema registry**
(`rwgpuengine_data::schema`) → `Fields` decodes with defaults taken from the same registry. The registry also
generates JSON Schemas, the reference docs and `rwgpu explain <component>`; one table, four uses.

## The world

"Fat" entities (ADR 0006): one `Entity` struct with optional built-in components (`tile`, `block`,
`sprite`, `label`, `mover`, `patrol`, `wander`, `trigger`), an optional `screen_pos` (HUD entities), a
`footprint` (cells covered from `pos` toward +x/+y, default 1 × 1) and a `props` map. Props are runtime state with types fixed by their declared starting values: `[[connect]]`
`add`/`set` change them, `when` conditions and label placeholders read them (ADR 0011); `counts` props are
kept equal to the number of enabled copies of a prefab by the engine (ADR 0014). Entity ids are their
index in scene order — stable for a given file, usable in scripts and tests. Entities made at runtime by a
`spawn` action get the next ids in spawn order and no name; there is no deletion (disabled entities keep their
ids). The world also holds the ground (size, and a terrain per cell when the scene has a `map`) and the spawn
templates (the prefabs `spawn` actions name, decoded at load time), so placement rules need no files at runtime
(ADR 0013).

### The step

`World::tick(input, systems)` runs, in this fixed order:

1. **input** — log pressed actions; each resolved tap logs `tap`/`tap_miss` and emits `tapped`. A tap that
   hits no entity over a ground cell is a ground tap: the ground emits `tapped` with that cell, if a
   `[[connect]] from = "ground"` listens.
2. **movers** — one cell per direction input, gated by `step_frames` cooldown; a mover walking into a
   `pushable` pushes it one cell if the cell beyond is free (ADR 0012).
3. **patrols** — step toward the next path cell every `step_frames`.
4. **wanderers** — one RNG draw each, in entity order.
5. **custom systems** — Rust game code (`Game: Systems`).
6. **triggers** — compare cell occupancy (enabled actors and pushables) with last step; emit `on_enter` /
   `on_exit`.
7. **signal dispatch** — FIFO through `[[connect]]` wiring, capped at 1000 per step (E0301). Each
   connection first checks its `when` conditions against the current state, once (logging `skipped` for each
   action if one fails), then applies its actions in order: `enable`/`disable`/`toggle`, `add`/`set` on a
   prop, or `spawn`; a refused spawn stops the rest of its `actions` list (ADR 0015). A spawn with a `cost`
   checks the payer's prop first (refusal reason `funds`) and pays only when the copy is placed (ADR 0016). A connection listens
   to one entity, every copy of a prefab (`from_prefab`), or the ground, and `to = "self"` targets the
   emitter. Each queued signal carries a cell (the tapped cell, or the emitter's), which `spawn` builds on
   after checking placement (inside the ground, buildable terrain, no entity taking up a cell). A prop that
   really changes makes its owner emit `changed`, and a new copy emits `spawned` (queued at the back, same
   step), if a connection listens. A refused placement makes the signal's source (the ground, or the emitter)
   emit `refused` with the reason, which `when = [{ reason = ... }]` tests. After every spawn, enable,
   disable or toggle (and once before dispatch, for custom systems), `counts` props are brought up to date:
   each change logs `counted` and makes its owner emit `changed` (ADR 0014).

Then `frame += 1`. Frame N is the state after N steps; input scheduled at frame N is applied by step N.

### The event log

Every step appends structured events — `input tap tap_miss moved blocked trigger signal action skipped
spawned counted error custom` — each with a complete sentence and fields. Refusals are first-class: a `blocked` event says
which solid, actor, edge, area or cooldown stopped a move; a `signal` with zero connections says which
`[[connect]]` to add; a `skipped` event names the `when` condition that failed and what was seen; a
`tap_miss` names the disabled entity under the finger. `action` events carry prop values before and after.

## Determinism

ADR 0005 has the full contract. In short: fixed timestep; one PCG32 RNG drawn in a fixed order; f32 with
only IEEE-exact operations (clippy bans libm transcendental functions); `BTreeMap` only (clippy bans
`HashMap`/`HashSet`); no wall-clock time in the engine; integer rasterization. `World::state_hash()` and
`Frame::image_hash()` (FNV-1a 64) make equality a one-line check.

Verified on 2026-11-25 ([audit](audits/2026-11-25-architecture-audit.md), section 3.1): all nine example
playtests reach the same state hashes on aarch64 and x86-64 macOS, aarch64 and x86-64 Linux, and wasm32, and
the wasm's pixels hash to native `rwgpu shot`'s image hashes. The current fingerprints are in the
[baseline](baselines/2026-11-25.md); this page does not repeat them, because copies go stale.

## Rendering

[ADR 0020](decisions/0020-render-list-camera-and-cpu-reference-renderer.md). The world is drawn in 3D, and the
**render list** is the central artifact:

- `list::build` turns the world into a view, a camera, one light and three kinds of things: **instances**
  (solid: a box per `block`, a flat rectangle per `tile` and per ground cell), **cards** (flat pictures that
  face the camera: a world entity's `sprite` or `label`) and the **2D layer** (screen entities, in file
  order). Nothing in it depends on a GPU; a GPU renderer for players will read the same list.
- The **camera** (`camera.rs`) is session state: `view` (`iso` or `perspective`), `center`, `turn`, `zoom`.
  The iso view with no turn and zoom 1 is the formula below, bit for bit, so scenes and taps written for
  the 2.5D engine stay valid. Its math is exact float operations plus the owned sine and cosine of
  `rwgpuengine_core::math`.
- `reference::project` takes the list to the screen: triangles with corners on a 1/256 pixel grid and a
  depth at each corner, cards as flat shapes with a depth per row. `reference::render` fills pixels through a
  depth buffer with exact integer coverage (top-left rule) and records which item owns each pixel. It is the
  **reference**: `rwgpu shot`, playtest `[[shot]]`s and the frame report use it. Three rules go beyond "the
  nearest wins": level faces in one plane are ordered by rank (a tile shows on the ground it lies on), a card
  stands upright above its anchor and lies on the surface below it, and see-through things are drawn last,
  far to near. A level face takes its depth from its plane's equation, not from its corners, so the first
  two rules hold whatever the corners are: a tile of nine cells lies on nine ground cells, and the part of a
  card below its anchor shows on the floor it lies on.
- `Painter` gives the same pixels without owners, for players, through `rwgpuengine_runtime::Presenter`,
  which repaints only when the render list changed. Tests hold the two pixel-identical on every frame of
  every example playtest.
- Text is drawn with an owned 5×8 pixel font (`font.rs`, printable ASCII); each glyph is a shape whose
  coverage is a bit test, so text is as deterministic, reportable and pickable as everything else.
- `build_report` turns ownership into per-entity drawn/visible pixels and "covered by", and flags W0301
  off-screen, W0302 mostly-hidden sprites, W0303 clipped.
- `pick` replays the renderer's rules at one pixel, so a tap hits exactly what is drawn there; when no
  entity is hit, the ground cell under the point is the tapped cell. `ground_tap_point` finds a pixel that
  taps a given cell (what `inspect --place` reports).

The iso view a scene starts with: `sx = ox + (x − y)·tw/2`, `sy = oy + (x + y)·th/2 − z·zh`. +x is
down-right, +y down-left, z up. The camera centers the ground by default.

### The players' renderer

[ADR 0021](decisions/0021-wgpu-renderer-held-to-the-reference.md). `rwgpuengine_gpu::Renderer` draws the
same render list with wgpu. It is handed a `wgpu::Device` and a `wgpu::Queue`, so a native window and a
browser canvas can both wrap it. It draws into its own frame, which has the list's size and holds RGBA8
with no sRGB encoding: the frame's bytes are the reference's bytes. `present` scales the frame onto a
window; `copy_frame` gives the bytes.

What decides a pixel comes from the reference renderer's own code; the GPU fills pixels:

- **Instances** are drawn instanced: one draw for the sides of all boxes, one for their level faces, one
  for all flat rectangles. The vertex shader projects corners with the `Projector`'s matrix.
- **A face's color** is selected from a table of 6 × 256 levels filled by the reference's `shade`. The GPU
  does no color arithmetic and writes whole 8-bit levels.
- **Level faces** write their depth per pixel from the plane's equation, the one the reference uses, two
  depth steps nearer per rank. Two faces in one plane then differ only in rank, on every adapter, and one
  depth test orders them.
- **Depth is stored where the scene is.** In perspective that is a straight line onto the depth buffer. In
  the iso view the range is laid along the float's exponent, which is 60 times finer around the scene.
- **Cards** are placed on the CPU (`Projector::card`, `reference::card_shapes`). The fragment shader tests
  the reference's integer coverage rule per shape and writes a depth per pixel row: upright above the
  anchor, and at and below it in the anchor's level plane, in front of any floor there.
- **In the iso view the corner columns come from the CPU**, computed and snapped as the reference does, so
  an upright edge of a box is drawn in the reference's pixel column.
- **See-through colors** are mixed in the fragment shader with the reference's integer formula, from a copy
  of the picture so far. Things that overlap on the screen take a pass each; things that do not share one.
- **The 2D layer** is the same shapes with no depth test, in list order.

`cargo xtask gpu` draws 271 scenes with both renderers (every frame the example playtests shoot, a town of
5,000 boxes under five cameras, a scene per rule under iso and perspective cameras, three copies of what
random scenes found, and 200 random scenes that are the same on every run). A pixel that differs must lie on an
edge of the reference picture, and a scene may have at most `pixels_apart` of them (`budgets.toml`). There
is no per-channel tolerance. No GPU picture is ever a reference image: they differ between systems by a
few edge pixels, and the CPU reference does not.

### The native player

[ADR 0022](decisions/0022-native-player-winit-and-rwgpu-play.md). `rwgpu-player` (`rwgpu play`) drives a
`Session` as the web player does, with the GPU drawing:

```text
 wake up (the next step is due, or an event came)
   ├─ keys, clicks, gestures ──► LiveInput for the next step; camera gestures ──► Session::move_camera
   ├─ take the steps that are due (at most 5; more lost time is let go, never a longer step)
   ├─ Session::render_list ──► equal to the one drawn? stop here
   └─ Renderer::draw ──► Renderer::present onto the window (scaled, bars) ──► sleep until the next step
```

The frame is the list's size; the window opens at that size in logical pixels and scales it to fit. A click is
taken back to frame pixels, so a tap hits what `rwgpu inspect --place` says is there. `--headless` takes the
same steps with no window and no clock; `--frames` and `--input` end a run and print the state hash.

## The agent surface

- **CLI** (`rwgpuengine_cli`): the command registry (`args::COMMANDS`) is data; parsing, `help`, `help --json`
  and docs/reference/cli.md all come from it.
- **Output envelope**: with `--json`, exactly one object:
  `{"rwgpu","format","command","ok",…command fields…,"errors","warnings","diagnostics"}`.
  Exit codes: 0 ok, 1 errors/failed checks, 2 bad command line.
- **Diagnostics**: stable codes in `rwgpuengine_core::codes` (never reused), rustc-style text, JSON fields
  `code severity message file line column hint notes`.
- **Docs**: `rwgpu docs` and the AGENTS.md written by `rwgpu new` are generated from the registries of
  the running binary.

## The web player

[ADR 0024](decisions/0024-web-player-on-webgpu.md). `rwgpuengine_player`'s library built for wasm32 is one
module (670 KB, with the glue the `wasm-bindgen` tool writes; 768 KB to download in all, budget 1,500,000).
It holds the engine, the GPU renderer on WebGPU and the CPU reference painter. The page's own script
(`players/rwgpuengine_player/web/player.js`, no framework, no build step) hands it the canvas, the events
and the animation frames:

```text
 animation frame ──► tick(now): the steps that are due (Steps, at most 5) ──► Session::step
 keys, pointers, wheel ──► input.rs (the window's bindings) ──► LiveInput, Session::move_camera
 list changed? ── WebGPU ──► Renderer::draw ──► Renderer::present onto the canvas (device pixels, bars)
                └ no WebGPU, no adapter, a lost device ──► Presenter (the reference's pixels) ──► 2D canvas
```

`rwgpu serve` finds the built player (`cargo xtask web-player`, `target/web-player`) and serves its four
files, the project as a bundle rebuilt from disk per request, and a content hash the page polls to reload.
Without it, it serves the CPU player of ADR 0008: `rwgpuengine_web` (a wasm module with zero imports, about
620 KB, budget 800,000), driven by `web/player.js`, which copies the pixels to the canvas only when the
picture changed. Diagnostics show in the page. Touch first: tap = tap, swipe = move along the iso axes,
on-screen pad; the WebGPU player adds the camera's bindings and a two-finger pinch.

## Extension points

- **Rust game code**: implement `rwgpuengine_runtime::Game` (a `Systems` with `name`, `on_start`, `inspect`)
  and ship a binary that calls `rwgpuengine_cli::main_with(args, &|| Box::new(MyGame))`. It runs after the
  built-ins each step and can read/write entities, props and emit signals. (Hot-patching is M4.)
- **New component**: add a `Spec` + `Field` in `rwgpuengine_data::schema`, decode it in `defs.rs`, give
  `Entity` a field, add behavior to `World::tick`, draw it in `draw::build` if visual, add it to
  `dump::entity_json` and `state_hash`. Docs and schemas follow from the registry.
- **New diagnostic**: add it to `rwgpuengine_core::codes` (next free number in its range, never reuse).
- **New command**: add a `Command` to `args::COMMANDS` and dispatch it in `lib.rs`.

## Known limits of v0.1 (by design, for now)

See [mvp-plan.md](mvp-plan.md#known-limitations) — no images or audio; ASCII text only; no rotation or
dragging; the command renders on the CPU; the GPU draws in the players, natively and in a browser.
