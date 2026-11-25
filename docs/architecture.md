# Architecture

How rwgpuengine is built, as of v0.1.0 (first milestone). Decisions behind each choice are in
[decisions/](decisions/). The vision is in [vision.md](vision.md).

## Crates and layers

```text
                ┌──────────────────────────────────────────────────────────────┐
 agent surface  │ rwgpuengine_cli   `rwgpu` binary + library: commands, --json,       │
                │             help/docs from registries, dev server (serve)      │
                │ rwgpuengine_web   wasm32 cdylib, C ABI for web/player.js             │
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
 rented         toml_edit (parsing) · png (encode/decode)         [later: wgpu, winit, audio, text]
 dev tooling    xtask: ci, web, smoke, budgets, docs
```

Dependency rule: arrows only point down. `rwgpuengine_sim` knows nothing about rendering; `rwgpuengine_render` reads
the world but never changes it; only `rwgpuengine_runtime` holds both (picking needs the drawn frame).

| crate | owns | must not |
|---|---|---|
| `rwgpuengine_core` | JSON, `Diagnostic`, the code registry, RNG, hashing, colors | depend on anything |
| `rwgpuengine_data` | file formats: the schema registry, validation, typed definitions, `Vfs`, static checks | run anything |
| `rwgpuengine_sim` | `World`, entities, built-in behaviors, signals, event log, state hash | render, read files, use wall-clock time |
| `rwgpuengine_render` | the camera, the render list, the CPU reference renderer, frame report, picking, PNG | change the world |
| `rwgpuengine_runtime` | sessions, scripted input, playtests, the `Game` trait | touch the filesystem (the CLI does) |
| `rwgpuengine_cli` | commands, output envelope, docs generation, templates, dev server | contain engine logic |
| `rwgpuengine_web` | the wasm C ABI | contain engine logic |

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
  far to near.
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

ADR 0008. `rwgpuengine_web` compiles to a wasm module with zero imports (about 620 KB with the 3D reference
renderer; budget 800,000). `web/player.js` (no framework, no build step) loads it, fetches the project as a
bundle (`path\0text\0…`), steps at the project's tick rate, and asks `rwgpu_present` each frame: only when
the picture changed does it copy the pixels to the canvas with `putImageData`. `rwgpu serve` (std-only HTTP) rebuilds
the bundle from disk per request; the page polls a content hash and reloads on change, showing
diagnostics in the page. Touch first: tap = tap, swipe = move along the iso axes, on-screen pad.

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
dragging; CPU rendering only; no native window.
