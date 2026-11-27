# AGENTS.md — working on rwgpuengine

This file is for agents (and people) changing **the engine itself**. Games made with rwgpuengine get their own
AGENTS.md from `rwgpu new` (preview it with `cargo rwgpu docs --agents`).

## Orientation (read in this order)

1. [docs/vision.md](docs/vision.md) — what we are building and the three guardrails.
2. [docs/architecture.md](docs/architecture.md) — crates, layers, data flow, the step, determinism.
3. [docs/mvp-plan.md](docs/mvp-plan.md) and [docs/roadmap.md](docs/roadmap.md) — status and what is next.
4. [docs/open-questions.md](docs/open-questions.md) — decisions that belong to the owner; do not decide
   them silently. [docs/decisions/](docs/decisions/) — decisions already made (ADRs).

## Commands

```bash
cargo xtask ci                       # the full gate: run before every commit
cargo xtask repo-check               # its first step, fast: no home path, no build output, no huge file in git
cargo xtask dist                     # release artifacts with no path of this machine (--agent: rwgpu + web module)
cargo test                           # all unit tests (fast)
cargo rwgpu <command> ...            # the CLI from source, e.g. cargo rwgpu check examples/iso_garden
cargo xtask web                      # build the CPU web player (the wasm module of the agent's half)
cargo xtask web-player               # build the WebGPU web player into target/web-player (the players' half)
cargo rwgpu serve examples/tiny_town # web player at http://127.0.0.1:4410/ (WebGPU if built, else the CPU player)
cargo rwgpu export web examples/tiny_town --out target/site   # a folder any static web host can serve, with the same player
cargo rwgpu serve --site target/site    # that folder, served as a host would serve it
cargo xtask export-check             # every example exported (CPU player) and played from a static server (part of ci)
cargo xtask pages                    # the folder the Pages workflow publishes: dist, Tiny Town with the WebGPU player, proved in a browser under /rwgpuengine/
cargo xtask docs                     # regenerate docs/reference after changing commands/formats/codes
cargo xtask budgets                  # measure size/time/dependency budgets
cargo run --release -p xtask -- sim-bench town 50000   # one step at scale (--all: every event written; --write DIR: the town as a project)
cargo run --release -p xtask -- sim-bench   # one simulation step in generated worlds of 1,000 to 50,000 entities (--steps N: stop at a frame and compare state hashes; --loader: the worlds from scene text)
cargo run --release -p xtask -- load-bench  # load and check generated projects written as files, stage by stage (town, linked, crowded, ...; the limits' sizes: ground, footprint, houses, copies, inputs)
cargo xtask gpu                      # the players' gate: the wgpu renderer against the CPU reference, the native player and the web player (headless Chrome) against `rwgpu test`, every example exported and played from a static host, players' budgets
cargo xtask gpu --fast               # only the picture check, built incrementally (the edit loop for players/)
cargo xtask gpu --fast -- --random 3000   # random scenes: what the fixed ones miss
cargo xtask gpu --window             # the gate plus a replay in a real window (needs a desktop or `xvfb-run`)
cargo install --locked --path players/rwgpuengine_player   # the native player on this machine, so `cargo rwgpu play examples/tiny_town` opens a window (to share it: `cargo xtask dist`)
```

`cargo xtask ci` never needs a GPU. `cargo xtask gpu` needs one, or a software adapter (on Linux:
`mesa-vulkan-drivers`); run it before every commit that touches `players/`, `rwgpuengine_render` or anything
visual. When a scene fails it names the pixels and writes `target/gpu-check/<scene>.{reference,gpu,diff}.png`.
The native player ([ADR 0022](docs/decisions/0022-native-player-winit-and-rwgpu-play.md)) reports like a
command: `rwgpu play <project> --input <test> --headless --json` (or without `--headless`, in a window) prints
the frame and the state hash, which must match `rwgpu run` for the same file. The web player
([ADR 0024](docs/decisions/0024-web-player-on-webgpu.md)) is checked in a headless Chrome by the same gate,
on WebGPU and on its CPU fallback; it needs Node and a Chromium browser (`RWGPU_BROWSER` names one).
Where the browser loses the WebGPU device of a canvas (Chrome on SwiftShader, as on the Linux runner),
`RWGPU_BROWSER_LOSES_DEVICE=1` makes it check the handover to the CPU instead of the WebGPU picture.

## Rules

- **Determinism is a contract** ([ADR 0005](docs/decisions/0005-determinism-model.md)): no libm trig/exp,
  no `HashMap`/`HashSet`, no wall-clock time in engine crates — clippy enforces it. If a change alters
  state hashes or reference images, say so in the commit and update references only after looking at them.
- **No proc macros in the runtime tree, few dependencies** ([ADR 0004](docs/decisions/0004-few-dependencies-no-proc-macros.md)).
  No serde/clap/rand: use `rwgpuengine_core::json`, `rwgpuengine_cli::args`, `rwgpuengine_core::rng`.
- **No rule looks at every entity** ([ADR 0026](docs/decisions/0026-cell-index-and-the-world-in-modules.md)).
  A step asks the cell index (`rwgpuengine_sim/src/world/index.rs`): what blocks a cell, who stands on a
  plate, what takes a cell, how many copies, who listens. The scans in `world/scan.rs` are the definition and
  run only in checks. The index is derived and not hashed; a rule that moves, spawns, enables or disables
  tells it, and anything else changes an entity through `World::get_mut`. The first two numbers of each pin
  in `rwgpuengine_sim/src/oracle.rs` are state hashes: never updated to make a test pass. The third pins the
  event log; it changes only with a change to events that says so.
- **Layers point down only**: core ← data ← sim ← render ← runtime ← cli/web. `rwgpuengine_sim` never renders;
  `rwgpuengine_render` never mutates the world; only `rwgpuengine_cli` touches the filesystem.
- **Two halves, two workspaces** ([ADR 0021](docs/decisions/0021-wgpu-renderer-held-to-the-reference.md)).
  `crates/` is the agent's half and never needs a GPU. `players/` is the players' half, a workspace of its
  own with its own `Cargo.lock` and budgets. wgpu and window crates live only in `players/`; nothing in
  `crates/` or `xtask` may depend on a crate there. `rwgpu` stays GPU-free.
- **The reference first.** A visual feature is built in the CPU reference renderer, then in
  `rwgpuengine_gpu`, and `cargo xtask gpu` holds the second to the first. Never fix a mismatch by changing
  what the reference draws, and never by adding a tolerance; if the two cannot agree, stop and report.
- **Formats live in one registry** (`rwgpuengine_data::schema`). Never parse a key that is not declared there.
- **Every problem is a `Diagnostic` with a registered code** (`rwgpuengine_core::codes`), a location and, when
  possible, a hint. Never reuse a code. Never print errors any other way.
- **Limits live in one table** (`rwgpuengine_core::limits`, [ADR 0027](docs/decisions/0027-events-at-scale-and-limits-on-work.md)).
  A step that reaches one stops there with an `error` event and the limit's code; never a hang, never a
  silent stop. Nothing a game file describes may make one step do work that no limit bounds.
- **Events of the per-actor kinds ask first** (`moved`, `blocked` for a move, `trigger`, `custom`): a rule
  calls `World::wants` and builds the sentence only on a yes, so a step's events cost the same at any size.
  Every other kind is always written. What is written never changes what happens.
- **Commands are data** (`rwgpuengine_cli::args::COMMANDS`); help and docs are generated from it.
- **Reference games drive features**: do not add engine features no example or reference game uses.
- Run `cargo xtask docs` after changing commands, formats or codes, and commit the regenerated reference.
- **The repository is public; every push publishes** ([privacy](docs/conventions.md#privacy),
  [ADR 0023](docs/decisions/0023-release-builds-without-local-paths.md)). Stage by explicit path, never
  `git add -A` or `git add .`; read `git status --short` first. No absolute home path in a file or a commit
  message, even as an example (write `someone`). Build scratch outside the repository. Release artifacts only
  from `cargo xtask dist`.

## Common changes

| change | touch |
|---|---|
| new component | `schema.rs` (Spec + Field) → `defs.rs` (decode) → `world/entity.rs` (Entity field, `make_entity`) → its behavior in the module of `world/` it belongs to, and `World::tick` in `world.rs` for a new part of the step → `world/hashing.rs` (state_hash) → `world/index.rs` (`Fact::of`) if it decides what blocks, takes space or holds a plate → `dump.rs` (JSON) → `list.rs` if visual (an instance or a card in the render list; `reference.rs` if it needs a new mesh) → tests → `cargo xtask docs` |
| a rule that asks about space, or about all entities | never a loop over `world.entities` in a step: a question to `world/index.rs` → the same question as a scan in `world/scan.rs`, compared where `self.checking()` → `rwgpuengine_sim/src/equivalence.rs` asks it on random worlds → `cargo test -p rwgpuengine_sim`, then the large oracle worlds: `cargo test --release -p rwgpuengine_sim oracle -- --ignored` → `cargo run --release -p xtask -- sim-bench` before and after |
| a rule that changes where an entity is, whether it is enabled, or what it is | tell the index in the same place: `put` (a move), `resync` (anything), `enter` (a new entity); debug builds compare the index with a rebuild after every step and panic if it was not told |
| new diagnostic | `rwgpuengine_core/src/codes.rs` (next free number in range) → emit with `.at(src, span).hint(..)` → test asserts the code |
| a check at load | the loader is a pipeline ([ADR 0028](docs/decisions/0028-loader-as-a-pipeline-linear-and-limits-on-sizes.md)): a rule on resolved data in `rwgpuengine_data/src/load/check/` (the file of its family), what a name refers to in `load/resolve.rs`; its place in the order agents read problems in `load/order.rs`. Never a scan of every entity per item: look names up in `Resolved::index`. Cases in `rwgpuengine_data/src/corpus.rs`: every existing pin must hold; print the new ones with `cargo test -p rwgpuengine_data corpus::print -- --ignored --nocapture` → `cargo run --release -p xtask -- load-bench` before and after |
| a limit on a size | `rwgpuengine_core/src/limits.rs` (a `Limit` with `Applies::Load` in `ALL`, E0209 whose explanation says the number; measure with `load-bench`; nothing in the repository may reach it) → check it where the size is first known, before the work it would cause, and report E0209 → corpus cases at and over it → `cargo xtask docs` |
| new command | `rwgpuengine_cli/src/args.rs` (Command entry with examples) → `lib.rs` dispatch → `commands.rs` → `cargo xtask docs` |
| web player file | `web/` for the CPU player, `players/rwgpuengine_player/web/` for the WebGPU player (relative addresses only) → `CPU_PLAYER_FILES` or `WEB_PLAYER_FILES` in `rwgpuengine_cli/src/web.rs` (each player's files, where each comes from and the one lookup; `serve`, `export web` and xtask read them) → `cargo xtask export-check` (CPU) and `cargo xtask gpu` (WebGPU, in a browser) |
| new event kind | `rwgpuengine_sim/src/events.rs` + `schema::EVENT_KINDS` (a test keeps them equal); if there is one per actor in a step, write it through `World::wants` and add it to `World::per_actor` |
| new limit on work | `rwgpuengine_core/src/limits.rs` (a `Limit` in `ALL`) and its code in `codes.rs` (the explanation says the number; a test checks) → check it where the work is done and stop with `World::cut_step` → a hostile scene that reaches it in `rwgpuengine_sim/src/scale_tests.rs` → `cargo xtask docs` |
| player input or window | `players/rwgpuengine_player`: `input.rs` (keys, taps, camera gestures; the web player's mapping for the game's input) and the `play` entry's text in `rwgpuengine_cli/src/args.rs` (a test keeps them equal), `window.rs` (the event loop), `clock.rs` (the fixed step) → `cargo xtask docs` → `cargo xtask gpu --window`. Every camera move stays reachable by touch alone ([ADR 0030](docs/decisions/0030-camera-over-the-ground-and-by-touch.md)): `gestures.rs` holds the two-finger thresholds to their measured rates, and the browser check touches the page (`check.mjs --touch`). No game rule and no visual feature here: those go in the engine and the reference |
| web player | `players/rwgpuengine_player/src/web.rs` (what the page calls) and `web/player.js` (the page: canvas, events, animation frames; nothing that decides); bindings stay in `input.rs`, shared with the window (a test keeps the page's help equal) → `cargo xtask web-player`, `cargo rwgpu serve examples/tiny_town` → `cargo xtask gpu` (the browser check: `web/check.mjs`, then `gpu_check --web-frames`) |
| visual change | the reference first (`reference.rs`, `list.rs`, `camera.rs`): run `cargo rwgpu test examples/iso_garden`; if refs differ on purpose, `--update-refs`, then look at the PNG → the same rule in `players/rwgpuengine_gpu` (`pack.rs` for what the CPU prepares, `shader.wgsl` for what the GPU draws) → a scene for it in `players/gpu_check/src/scenes.rs` under an iso and a perspective camera → `cargo xtask gpu` → a few thousand random scenes, `cargo xtask gpu --fast -- --random 3000` (the hand-made scenes were green twice while a real fault was there) |

## Commits

Conventional style with a detailed body — the git log is the progress log
([conventions](docs/conventions.md#commits)). Include measurements and what was verified; say what is not
done. Add a dated entry to [docs/progress-log.md](docs/progress-log.md) for milestones and findings.
