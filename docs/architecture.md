# Architecture

How rwgpuengine is built, as of v0.1.0 (first milestone). Decisions behind each choice are in
[decisions/](decisions/). The vision is in [vision.md](vision.md).

## Crates and layers

```text
                ┌──────────────────────────────────────────────────────────────┐
 agent surface  │ rwgpuengine_cli   `rwgpu` binary + library: commands, --json,       │
                │             help/docs from registries, serve, export web       │
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
 dev tooling    xtask: ci, repo-check, dist, web, web-player, smoke, budgets, docs, wasm-check, export-check, pages, gpu,
                sim-bench
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
([ADR 0024](decisions/0024-web-player-on-webgpu.md)), and `rwgpu export web` copies them into a folder for a
web host ([ADR 0025](decisions/0025-rwgpu-export-web.md)).

| crate | owns | must not |
|---|---|---|
| `rwgpuengine_core` | JSON, `Diagnostic`, the code registry, the limits, RNG, hashing, colors | depend on anything |
| `rwgpuengine_data` | file formats: the schema registry, validation, typed definitions, `Vfs`, static checks | run anything |
| `rwgpuengine_sim` | `World`, entities, the cell index, built-in behaviors, signals, event log, state hash | render, read files, use wall-clock time, look at every entity in a step |
| `rwgpuengine_render` | the camera, the render list, the CPU reference renderer, frame report, picking, PNG | change the world |
| `rwgpuengine_runtime` | sessions, scripted input, playtests, the `Game` trait | touch the filesystem (the CLI does) |
| `rwgpuengine_cli` | commands, output envelope, docs generation, templates, dev server, the web players' lookup and `export web` | contain engine logic |
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

`world.rs` is the world, how it is built and the order of a step. The rules are in `world/`, one module per
responsibility ([ADR 0026](decisions/0026-cell-index-and-the-world-in-modules.md)):

| module | holds |
|---|---|
| `entity.rs` | `Entity` and its components; `Entities`, the read-only list; `EntityMut`, what `World::get_mut` gives |
| `wiring.rs` | `[[connect]]` with its names resolved, and the routes from a signal to the connections that listen |
| `cells.rs`, `index.rs` | the cell index |
| `scan.rs` | every question of the index answered by looking at everything: the definition, used by checks only |
| `taps.rs`, `movement.rs`, `triggers.rs`, `signals.rs`, `actions.rs` | the parts of a step |
| `placement.rs`, `payment.rs`, `counting.rs` | `spawn` and `can_place`, a spawn's cost, `counts` props |
| `hashing.rs` | the state hash |

### The cell index

No rule looks at every entity. The world keeps, beside the entities:

- **who is on each cell**: the ground's cells are an array of list heads, each list in id order; a world
  without a ground has the array over where its entities are when it is built; every other cell is in a
  sorted map. An enabled entity is on every cell its body covers;
- **a fact per entity** (24 bytes: cell, footprint, and whether it is enabled, takes space, is solid, an
  actor, pushable), so a question about space never reads the entity;
- **who has each behavior**, in id order: whose turn it is in each part of the step;
- **enabled copies per prefab**, and who counts each prefab, for `counts`;
- **routes**: for a signal and where it came from, the connections that listen.

"What stops a move onto this cell?", "who stands on this plate?", "is there room for a house?" and "who
listens to this signal?" are answered from these, with the answers the scans gave, in the same order: every
list is in id order. One step with 50,000 actors takes 1.06 ms, of which 0.88 ms are the rules and the rest
the events a step writes (it was 2,659 ms before the index, 14.1 ms before events at scale).

All of it is **derived, never state**. It is not in the state hash and can be rebuilt from the entities at
any time (`World::rebuild_index`). The rules tell it what they change: a move, a spawn, enable and disable.
Anything else changes an entity through `World::get_mut`, whose result brings the index up to date when it is
let go; `World::entities` itself cannot be written. Two checks hold it: a debug build compares the index with
a rebuild at the end of every step, and `World::check_against_scans` answers every question both ways and
panics where they differ.

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
7. **signal dispatch** — FIFO through `[[connect]]` wiring, within the limits on work below. Each
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

Then the step's `omitted` events are written (below) and `frame += 1`. Frame N is the state after N steps;
input scheduled at frame N is applied by step N.

### Limits on work

A game file must not be able to make one step do unbounded work, or the world grow without end
([ADR 0027](decisions/0027-events-at-scale-and-limits-on-work.md)). The limits are one table,
`rwgpuengine_core::limits`, printed by `rwgpu explain limits` and the reference:

| limit | number | code |
|---|---|---|
| signals per step | 1,000 | E0301 |
| actions per step, run or skipped | 10,000 | E0302 |
| `counts` props changed per step | 10,000 | E0303 |
| entities in a world | 1,000,000 | E0304 |
| taps per step (the rest are not picked) | 256 | E0305 |

A step that reaches one stops there (`World::cut_step`): the signals still queued are dropped and an `error`
event says what was cut off. The world keeps what it reached (`World::limits_hit`); the session turns that
into diagnostics, so `rwgpu run`, `inspect`, `shot` and `test` fail with the code. Later steps run as usual.

### The event log

Every step appends structured events — `input tap tap_miss moved blocked trigger signal action skipped
spawned counted error custom omitted` — each with a complete sentence and fields. Refusals are first-class: a `blocked` event says
which solid, actor, edge, area or cooldown stopped a move; a `signal` with zero connections says which
`[[connect]]` to add; a `skipped` event names the `when` condition that failed and what was seen; a
`tap_miss` names the disabled entity under the finger. `action` events carry prop values before and after.

At scale ([ADR 0027](decisions/0027-events-at-scale-and-limits-on-work.md)):

```text
 a rule: one move, refusal or plate (moved, blocked, trigger; custom from game code)
   └─ World::wants(kind, who, other)?  ── watched (entity, kind or all) ──────────► built and written
                                       ── among the first 256 of its kind this step ► built and written
                                       └─ otherwise: counted, nothing built
 every other kind (input, taps, signals, actions, spawns, counts, errors) ───────► built and written
 the end of the step: one `omitted` event per kind with events counted and not written
```

`EventLog` keeps the written events (100,000 at most; then the oldest half is dropped and counted) and a
count of every event per kind: `event_counts` is what happened, `events_omitted` and `events_dropped` what
is not in the log. A `Watch` is set before stepping: `--watch` on `run` and `inspect`, `inspect --entity
NAME --events`, and a playtest watches what its `[[expect]]` tables name. What is written never changes the
world: the oracle runs every world with everything written and by default, and both reach the same state.

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
module (670 KB, with the glue the `wasm-bindgen` tool writes; 772 KB to download in all, budget 1,500,000).
It holds the engine, the GPU renderer on WebGPU and the CPU reference painter. The page's own script
(`players/rwgpuengine_player/web/player.js`, no framework, no build step) hands it the canvas, the events
and the animation frames:

```text
 animation frame ──► tick(now): the steps that are due (Steps, at most 5) ──► Session::step
 keys, pointers, wheel ──► input.rs (the window's bindings) ──► LiveInput, Session::move_camera
 list changed? ── WebGPU ──► Renderer::draw ──► Renderer::present onto the canvas (device pixels, bars)
                └ no WebGPU, no adapter, a lost device ──► Presenter (the reference's pixels) ──► 2D canvas
```

Without the WebGPU player, the CPU player of ADR 0008 is served: `rwgpuengine_web` (a wasm module with zero
imports, about 630 KB, budget 800,000), driven by `web/player.js`, which copies the pixels to the canvas only
when the picture changed. Diagnostics show in the page. Touch first: tap = tap, swipe = move along the iso
axes, on-screen pad; the WebGPU player adds the camera's bindings and a two-finger pinch.

Both pages ask only for files next to them, by relative addresses, and one lookup in `rwgpuengine_cli/src/web.rs`
finds the player for `serve` and for `export web` ([ADR 0025](decisions/0025-rwgpu-export-web.md)):

```text
 the WebGPU player: index.html, player.js,  ─┐  --web-player, --wasm, RWGPU_WEB_PLAYER, RWGPU_WEB_WASM
   rwgpu_web.js, rwgpu_web_bg.wasm           ├─ name the one place; else web-player next to rwgpu, the
 or the CPU player: index.html, player.js    │  workspace build, then rwgpuengine_web.wasm (E0008: none)
   (in rwgpu), player.wasm                  ─┤
 project.json, bundle.txt ── web::game(Vfs) ─┤  the files the engine reads, nothing else
                                             ├──► rwgpu serve PATH      made per request; also bundle-hash.txt,
                                             │                          which the page polls to reload
                                             └──► rwgpu export web      written once into a folder that any
                                                  PATH --out DIR        static host serves, at a site's root or
                                                                        under a path
```

`rwgpu export web` validates the project first, writes the same bytes for the same project and player, and
never writes outside `--out`; `rwgpu serve --site DIR` serves such a folder as a host would. Each folder is
played from a plain static host (`web/static-host.mjs`) at `/` and under `/rwgpuengine/`: with the CPU player
under Node in `cargo xtask ci` (`web/export-check.mjs`), with the WebGPU player in a headless Chrome on both
paths in `cargo xtask gpu`; `cargo xtask pages` proves the folder the Pages workflow publishes the same way.

## Extension points

- **Rust game code**: implement `rwgpuengine_runtime::Game` (a `Systems` with `name`, `on_start`, `inspect`)
  and ship a binary that calls `rwgpuengine_cli::main_with(args, &|| Box::new(MyGame))`. It runs after the
  built-ins each step and can read entities (`world.entities`), change them (`world.get_mut(id)`), and emit
  signals. (Hot-patching is M4.)
- **New component**: add a `Spec` + `Field` in `rwgpuengine_data::schema`, decode it in `defs.rs`, give
  `Entity` a field (`world/entity.rs`), add its behavior in the module of `world/` it belongs to and to
  `World::tick`, tell the index if it decides what blocks, takes space or holds a plate (`world/index.rs`),
  draw it in `list::build` if visual, add it to `dump::entity_json` and `state_hash` (`world/hashing.rs`).
  Docs and schemas follow from the registry.
- **New diagnostic**: add it to `rwgpuengine_core::codes` (next free number in its range, never reuse).
- **New command**: add a `Command` to `args::COMMANDS` and dispatch it in `lib.rs`.

## Known limits of v0.1 (by design, for now)

See [mvp-plan.md](mvp-plan.md#known-limitations) — no images or audio; ASCII text only; no rotation or
dragging; the command renders on the CPU; the GPU draws in the players, natively and in a browser.
