# AGENTS.md — working on the slate engine

This file is for agents (and people) changing **the engine itself**. Games made with slate get their own
AGENTS.md from `slate new` (preview it with `cargo slate docs --agents`).

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
cargo slate <command> ...            # the CLI from source, e.g. cargo slate check examples/iso_garden
cargo xtask web                      # build the wasm web player
cargo slate serve examples/iso_garden   # web player at http://127.0.0.1:4410/
cargo xtask docs                     # regenerate docs/reference after changing commands/formats/codes
cargo xtask budgets                  # measure size/time/dependency budgets
```

## Rules

- **Determinism is a contract** ([ADR 0005](docs/decisions/0005-determinism-model.md)): no libm trig/exp,
  no `HashMap`/`HashSet`, no wall-clock time in engine crates — clippy enforces it. If a change alters
  state hashes or reference images, say so in the commit and update references only after looking at them.
- **No proc macros in the runtime tree, few dependencies** ([ADR 0004](docs/decisions/0004-few-dependencies-no-proc-macros.md)).
  No serde/clap/rand: use `slate_core::json`, `slate_cli::args`, `slate_core::rng`.
- **Layers point down only**: core ← data ← sim ← render ← runtime ← cli/web. `slate_sim` never renders;
  `slate_render` never mutates the world; only `slate_cli` touches the filesystem.
- **Formats live in one registry** (`slate_data::schema`). Never parse a key that is not declared there.
- **Every problem is a `Diagnostic` with a registered code** (`slate_core::codes`), a location and, when
  possible, a hint. Never reuse a code. Never print errors any other way.
- **Commands are data** (`slate_cli::args::COMMANDS`); help and docs are generated from it.
- **Reference games drive features**: do not add engine features no example or reference game uses.
- Run `cargo xtask docs` after changing commands, formats or codes, and commit the regenerated reference.

## Common changes

| change | touch |
|---|---|
| new component | `schema.rs` (Spec + Field) → `defs.rs` (decode) → `world.rs` (Entity field, behavior, state_hash) → `dump.rs` (JSON) → `draw.rs` if visual → tests → `cargo xtask docs` |
| new diagnostic | `slate_core/src/codes.rs` (next free number in range) → emit with `.at(src, span).hint(..)` → test asserts the code |
| new command | `slate_cli/src/args.rs` (Command entry with examples) → `lib.rs` dispatch → `commands.rs` → `cargo xtask docs` |
| new event kind | `slate_sim/src/events.rs` + `schema::EVENT_KINDS` (a test keeps them equal) |
| visual change | run `cargo slate test examples/iso_garden`; if refs differ on purpose, `--update-refs`, then look at the PNG |

## Commits

Conventional style with a detailed body — the git log is the progress log
([conventions](docs/conventions.md#commits)). Include measurements and what was verified; say what is not
done. Add a dated entry to [docs/progress-log.md](docs/progress-log.md) for milestones and findings.
