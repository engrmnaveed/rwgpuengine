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
cargo test                           # all unit tests (fast)
cargo rwgpu <command> ...            # the CLI from source, e.g. cargo rwgpu check examples/iso_garden
cargo xtask web                      # build the wasm web player
cargo rwgpu serve examples/iso_garden   # web player at http://127.0.0.1:4410/
cargo xtask docs                     # regenerate docs/reference after changing commands/formats/codes
cargo xtask budgets                  # measure size/time/dependency budgets
cargo xtask gpu                      # the players' gate: the wgpu renderer against the CPU reference, the native player against `rwgpu test`, players' budgets
cargo xtask gpu --fast               # only the picture check, built incrementally (the edit loop for players/)
cargo xtask gpu --fast -- --random 3000   # random scenes: what the fixed ones miss
cargo xtask gpu --window             # the gate plus a replay in a real window (needs a desktop or `xvfb-run`)
cargo install --locked --path players/rwgpuengine_player   # the native player, so `cargo rwgpu play examples/tiny_town` opens a window
```

`cargo xtask ci` never needs a GPU. `cargo xtask gpu` needs one, or a software adapter (on Linux:
`mesa-vulkan-drivers`); run it before every commit that touches `players/`, `rwgpuengine_render` or anything
visual. When a scene fails it names the pixels and writes `target/gpu-check/<scene>.{reference,gpu,diff}.png`.
The native player ([ADR 0022](docs/decisions/0022-native-player-winit-and-rwgpu-play.md)) reports like a
command: `rwgpu play <project> --input <test> --headless --json` (or without `--headless`, in a window) prints
the frame and the state hash, which must match `rwgpu run` for the same file.

## Rules

- **Determinism is a contract** ([ADR 0005](docs/decisions/0005-determinism-model.md)): no libm trig/exp,
  no `HashMap`/`HashSet`, no wall-clock time in engine crates — clippy enforces it. If a change alters
  state hashes or reference images, say so in the commit and update references only after looking at them.
- **No proc macros in the runtime tree, few dependencies** ([ADR 0004](docs/decisions/0004-few-dependencies-no-proc-macros.md)).
  No serde/clap/rand: use `rwgpuengine_core::json`, `rwgpuengine_cli::args`, `rwgpuengine_core::rng`.
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
- **Commands are data** (`rwgpuengine_cli::args::COMMANDS`); help and docs are generated from it.
- **Reference games drive features**: do not add engine features no example or reference game uses.
- Run `cargo xtask docs` after changing commands, formats or codes, and commit the regenerated reference.

## Common changes

| change | touch |
|---|---|
| new component | `schema.rs` (Spec + Field) → `defs.rs` (decode) → `world.rs` (Entity field, behavior, state_hash) → `dump.rs` (JSON) → `list.rs` if visual (an instance or a card in the render list; `reference.rs` if it needs a new mesh) → tests → `cargo xtask docs` |
| new diagnostic | `rwgpuengine_core/src/codes.rs` (next free number in range) → emit with `.at(src, span).hint(..)` → test asserts the code |
| new command | `rwgpuengine_cli/src/args.rs` (Command entry with examples) → `lib.rs` dispatch → `commands.rs` → `cargo xtask docs` |
| new event kind | `rwgpuengine_sim/src/events.rs` + `schema::EVENT_KINDS` (a test keeps them equal) |
| player input or window | `players/rwgpuengine_player`: `input.rs` (keys, taps, camera gestures; the web player's mapping for the game's input) and the `play` entry's text in `rwgpuengine_cli/src/args.rs` (a test keeps them equal), `window.rs` (the event loop), `clock.rs` (the fixed step) → `cargo xtask docs` → `cargo xtask gpu --window`. No game rule and no visual feature here: those go in the engine and the reference |
| visual change | the reference first (`reference.rs`, `list.rs`, `camera.rs`): run `cargo rwgpu test examples/iso_garden`; if refs differ on purpose, `--update-refs`, then look at the PNG → the same rule in `players/rwgpuengine_gpu` (`pack.rs` for what the CPU prepares, `shader.wgsl` for what the GPU draws) → a scene for it in `players/gpu_check/src/scenes.rs` under an iso and a perspective camera → `cargo xtask gpu` → a few thousand random scenes, `cargo xtask gpu --fast -- --random 3000` (the hand-made scenes were green twice while a real fault was there) |

## Commits

Conventional style with a detailed body — the git log is the progress log
([conventions](docs/conventions.md#commits)). Include measurements and what was verified; say what is not
done. Add a dated entry to [docs/progress-log.md](docs/progress-log.md) for milestones and findings.
