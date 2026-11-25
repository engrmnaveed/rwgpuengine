# Architecture

How slate is built, as of v0.1.0 (first milestone). Decisions behind each choice are in
[decisions/](decisions/). The vision is in [vision.md](vision.md).

## Crates and layers

```text
                ┌──────────────────────────────────────────────────────────────┐
 agent surface  │ slate_cli   `slate` binary + library: commands, --json,       │
                │             help/docs from registries, dev server (serve)      │
                │ slate_web   wasm32 cdylib, C ABI for web/player.js             │
                └───────────────┬──────────────────────────────┬───────────────┘
                                │                              │
                ┌───────────────▼──────────────────────────────▼───────────────┐
 composition    │ slate_runtime  Session (project+scene+script), stepping,      │
                │                tap picking, inspect views, playtests, Game     │
                └──────┬───────────────────────┬────────────────────┬──────────┘
                       │                       │                    │
 core runtime   ┌──────▼──────┐        ┌───────▼───────┐            │
                │ slate_sim   │◄───────┤ slate_render  │            │
                │ world, step,│        │ iso view, draw│            │
                │ signals,    │        │ list, CPU     │            │
                │ event log   │        │ raster, report│            │
                └──────┬──────┘        └───────┬───────┘            │
                       │                       │                    │
                ┌──────▼───────────────────────▼────────────────────▼──────────┐
 text + schema  │ slate_data  TOML → span tree → schema check → typed defs;     │
                │             Vfs, loaders, static checks (`slate check`)        │
                └──────────────────────────────┬───────────────────────────────┘
                ┌──────────────────────────────▼───────────────────────────────┐
 foundation     │ slate_core  JSON, diagnostics + code registry, PCG32, FNV,    │
                │             colors, suggestions. Zero dependencies.            │
                └──────────────────────────────────────────────────────────────┘
 rented         toml_edit (parsing) · png (encode/decode)         [later: wgpu, winit, audio, text]
 dev tooling    xtask: ci, web, smoke, budgets, docs
```

Dependency rule: arrows only point down. `slate_sim` knows nothing about rendering; `slate_render` reads
the world but never changes it; only `slate_runtime` holds both (picking needs the drawn frame).

| crate | owns | must not |
|---|---|---|
| `slate_core` | JSON, `Diagnostic`, the code registry, RNG, hashing, colors | depend on anything |
| `slate_data` | file formats: the schema registry, validation, typed definitions, `Vfs`, static checks | run anything |
| `slate_sim` | `World`, entities, built-in behaviors, signals, event log, state hash | render, read files, use wall-clock time |
| `slate_render` | iso projection, draw list, rasterizer, frame report, picking, PNG | change the world |
| `slate_runtime` | sessions, scripted input, playtests, the `Game` trait | touch the filesystem (the CLI does) |
| `slate_cli` | commands, output envelope, docs generation, templates, dev server | contain engine logic |
| `slate_web` | the wasm C ABI | contain engine logic |

## Data flow of one command

```text
disk ──Vfs::from_dir──► Vfs ──load_project/SceneLoader──► ProjectDef, SceneDef ──World::new──► World
                         │                                   (diagnostics on failure)            │
                         └──load_playtest──► PlaytestDef ──Script──► InputFrame per step ─────────┤
                                                                                                    ▼
                                    Session::step ×N  (taps resolved by picking the drawn frame)
                                                                                                    │
            inspect ◄── Session::inspect (world JSON + screen rects + events)                       │
            shot    ◄── draw::build → rasterize → (pixels, stats) → build_report → PNG + report ◄──┘
            run     ◄── summary + state hash (+ events, + hash trace)
            test    ◄── run_playtest: [[expect]] at frames, [[shot]] captures → compare refs (CLI)
```

Every command is a **pure function of (files, input script, seed, frame)**. `shot --frame 300` replays
300 steps; nothing is cached between commands, so nothing can be stale.

## The file model

- `slate.toml` marks the project root (found by walking up, like Cargo.toml).
- `scenes/*.toml`: `[scene]`, `[camera]`, `[ground]`, `[[entity]]`, `[[connect]]`.
- `prefabs/<name>.toml`: an entity template; entity keys override it, component tables merge key by key.
- `tests/*.toml`: playtests — `[test]`, `[[input]]`, `[[expect]]`, `[[shot]]`.

Parsing pipeline (ADR 0003): `toml_edit` parses → converted at once into an owned `Node` tree that keeps
byte spans for every key and value → `Checker` validates against the **schema registry**
(`slate_data::schema`) → `Fields` decodes with defaults taken from the same registry. The registry also
generates JSON Schemas, the reference docs and `slate explain <component>`; one table, four uses.

## The world

"Fat" entities (ADR 0006): one `Entity` struct with optional built-in components (`tile`, `block`,
`sprite`, `label`, `mover`, `patrol`, `wander`, `trigger`), an optional `screen_pos` (HUD entities) and a
`props` map. Props are runtime state with types fixed by their declared starting values: `[[connect]]`
`add`/`set` change them, `when` conditions and label placeholders read them (ADR 0011). Entity ids are their
index in scene order — stable for a given file, usable in scripts and tests.

### The step

`World::tick(input, systems)` runs, in this fixed order:

1. **input** — log pressed actions; each resolved tap logs `tap`/`tap_miss` and emits `tapped`.
2. **movers** — one cell per direction input, gated by `step_frames` cooldown.
3. **patrols** — step toward the next path cell every `step_frames`.
4. **wanderers** — one RNG draw each, in entity order.
5. **custom systems** — Rust game code (`Game: Systems`).
6. **triggers** — compare cell occupancy with last step; emit `on_enter` / `on_exit`.
7. **signal dispatch** — FIFO through `[[connect]]` wiring, capped at 1000 per step (E0301). Each
   connection first checks its `when` conditions against the current state (logging `skipped` if one
   fails), then applies `enable`/`disable`/`toggle` or `add`/`set` on a prop. A prop that really changes
   makes its owner emit `changed` (queued at the back, same step) if any connection listens to it.

Then `frame += 1`. Frame N is the state after N steps; input scheduled at frame N is applied by step N.

### The event log

Every step appends structured events — `input tap tap_miss moved blocked trigger signal action skipped
error custom` — each with a complete sentence and fields. Refusals are first-class: a `blocked` event says
which solid, actor, edge, area or cooldown stopped a move; a `signal` with zero connections says which
`[[connect]]` to add; a `skipped` event names the `when` condition that failed and what was seen; a
`tap_miss` names the disabled entity under the finger. `action` events carry prop values before and after.

## Determinism

ADR 0005 has the full contract. In short: fixed timestep; one PCG32 RNG drawn in a fixed order; f32 with
only IEEE-exact operations (clippy bans libm transcendental functions); `BTreeMap` only (clippy bans
`HashMap`/`HashSet`); no wall-clock time in the engine; integer rasterization. `World::state_hash()` and
`Frame::image_hash()` (FNV-1a 64) make equality a one-line check.

Verified: the example walkthrough reaches state `469ae7c92124d43d` and image `bfca8c0b40af6828` both
natively (aarch64-apple-darwin) and in the browser (wasm32); coin_vault's solve (props, conditions, text,
HUD) reaches state `41f58b0904b83261` and image `f15c777308fdf361` on both.

## Rendering

ADR 0007. The **draw list** is the central artifact:

- `draw::build` turns the world into back-to-front items: ground cells, then tiles, then block faces,
  sprites and label glyphs sorted by `(x + y, z, part, id)`, then screen entities (`screen_pos`) in file
  order. Text is drawn with an owned 5×8 pixel font (`font.rs`, printable ASCII); each glyph is a shape
  whose coverage is a bit test, so text is as deterministic, reportable and pickable as everything else.
- `rasterize` fills shapes with exact integer coverage tests at pixel centers (doubled coordinates,
  top-left fill rule) and integer shading/blending; it records which item owns each pixel.
- `build_report` turns ownership into per-entity drawn/visible pixels and "covered by", and flags W0301
  off-screen, W0302 mostly-hidden sprites, W0303 clipped.
- `pick` walks the same list front to back with the same coverage test, so taps hit exactly what is drawn.

Isometric projection: `sx = ox + (x − y)·tw/2`, `sy = oy + (x + y)·th/2 − z·zh`. +x is down-right,
+y down-left, z up. The camera centers the ground by default.

## The agent surface

- **CLI** (`slate_cli`): the command registry (`args::COMMANDS`) is data; parsing, `help`, `help --json`
  and docs/reference/cli.md all come from it.
- **Output envelope**: with `--json`, exactly one object:
  `{"slate","format","command","ok",…command fields…,"errors","warnings","diagnostics"}`.
  Exit codes: 0 ok, 1 errors/failed checks, 2 bad command line.
- **Diagnostics**: stable codes in `slate_core::codes` (never reused), rustc-style text, JSON fields
  `code severity message file line column hint notes`.
- **Docs**: `slate docs` and the AGENTS.md written by `slate new` are generated from the registries of
  the running binary.

## The web player

ADR 0008. `slate_web` compiles to a 467 KB wasm module with zero imports. `web/player.js` (no framework,
no build step) loads it, fetches the project as a bundle (`path\0text\0…`), steps at the project's
tick rate, and paints the CPU-rendered frame with `putImageData`. `slate serve` (std-only HTTP) rebuilds
the bundle from disk per request; the page polls a content hash and reloads on change, showing
diagnostics in the page. Touch first: tap = tap, swipe = move along the iso axes, on-screen pad.

## Extension points

- **Rust game code**: implement `slate_runtime::Game` (a `Systems` with `name`, `on_start`, `inspect`)
  and ship a binary that calls `slate_cli::main_with(args, &|| Box::new(MyGame))`. It runs after the
  built-ins each step and can read/write entities, props and emit signals. (Hot-patching is M4.)
- **New component**: add a `Spec` + `Field` in `slate_data::schema`, decode it in `defs.rs`, give
  `Entity` a field, add behavior to `World::tick`, draw it in `draw::build` if visual, add it to
  `dump::entity_json` and `state_hash`. Docs and schemas follow from the registry.
- **New diagnostic**: add it to `slate_core::codes` (next free number in its range, never reuse).
- **New command**: add a `Command` to `args::COMMANDS` and dispatch it in `lib.rs`.

## Known limits of v0.1 (by design, for now)

See [mvp-plan.md](mvp-plan.md#known-limitations) — no images or audio; ASCII text only; single-cell
objects only; CPU rendering only; no native window.
