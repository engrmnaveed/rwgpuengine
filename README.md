# slate

**An agent-native 2D/2.5D game engine in Rust**, for learning games on the web and tablets.
The game is text files; the engine is a command line that checks, runs, renders and inspects them —
deterministically, without a screen, with JSON output and rustc-quality diagnostics.

> Status: **v0.1.0 — first milestone done** (natively and in the browser). Working codename "slate"
> (see [open questions](docs/open-questions.md) Q1). Not published; no license chosen yet (Q2).

```console
$ slate new demo && cd demo
$ slate check
check ok: 1 scene(s), 4 prefab(s), 1 playtest(s) — 0 error(s), 0 warning(s)
$ slate run --frames 300 --input tests/smoke.toml
ran scenes/main.toml for 300 frames (seed 1, input tests/smoke.toml)
  state hash  f21213c8b806d36f
  entities    8 (7 enabled)
  events      input 2, moved 6, trigger 2, signal 2, action 1
$ slate shot --input tests/smoke.toml --report        # PNG + what is drawn where
$ slate inspect --frame 300 --entity door --events --json
$ slate test
test smoke ... ok (3 expect(s), 0 shot(s), 120 frames, state 366ea4f4954ba87c)
```

Mistakes come back like this (also as JSON with `--json`):

```text
error[E0103]: unknown key `sprit` in entity
  --> scenes/main.toml:47:1
   |
47 | sprit = { color = "#fff" }
   | ^^^^^
   = help: did you mean `sprite`?
```

## Why

Coding agents are best at reading files and running commands. slate makes those the engine's main
interface: text files with a published schema, a CLI where every command speaks JSON, a deterministic
headless core, a queryable world with an event log that says *why* things happened, and built-in visual
checks. Read [docs/vision.md](docs/vision.md).

## Quick start (from source)

Requires `rustup`; the toolchain (Rust 1.100.0 + wasm32) installs itself from `rust-toolchain.toml`.

```bash
cargo build --release            # target/release/slate
cargo slate test examples/iso_garden
```

Web player (same engine compiled to wasm, same pixels as native):

```bash
cargo xtask web                  # builds target/wasm32-unknown-unknown/web/slate_web.wasm
cargo slate serve examples/iso_garden   # open http://127.0.0.1:4410/
```

Add `?input=tests/walkthrough.toml&frames=300` to the URL to replay a playtest; the state hash on the
page matches `slate run`. Edit any file in the project and the page reloads it.

## Repository map

| path | what |
|---|---|
| `crates/slate_core` | JSON, diagnostics + stable codes, RNG, hashing (zero deps) |
| `crates/slate_data` | file formats: schema registry, validation, loaders, static checks |
| `crates/slate_sim` | deterministic world, built-in behaviors, signals, event log |
| `crates/slate_render` | iso projection, draw list, CPU rasterizer, frame report, picking, PNG |
| `crates/slate_runtime` | sessions, scripted input, playtests, the `Game` trait for Rust code |
| `crates/slate_cli` | the `slate` command line (+ project template, dev server) |
| `crates/slate_web` | wasm build with a small C ABI; `web/` holds the page and JS glue |
| `examples/iso_garden` | reference example: plate opens gate, goal raises flag; 2 playtests |
| `examples/coin_vault` | reference example for data logic: coin counter on a HUD, vault opens at 3, AND lock held by a pushed crate; 4 playtests |
| `examples/tiny_town` | **reference game 1** ([spec](docs/games/tiny-town.md)): an isometric builder — terrain map, tap to build 2 × 2 houses, trees and a well within a coin budget, clear with refund; 3 playtests |
| `xtask` | `cargo xtask ci | web | smoke | budgets | docs` |
| `docs/` | vision, architecture, decisions (ADRs), MVP plan, roadmap, open questions, reference |

## Development

```bash
cargo xtask ci        # fmt, clippy (native+wasm), tests, docs drift, examples, budgets — what CI runs
```

Hard limits in [budgets.toml](budgets.toml) fail the build (wasm size, CLI size, dependency count,
zero proc macros, smoke-chain time, rebuild time). Conventions: [docs/conventions.md](docs/conventions.md).
Agents working on this repo: start with [AGENTS.md](AGENTS.md).

## Documentation

- [Vision](docs/vision.md) · [Architecture](docs/architecture.md) · [Decisions](docs/decisions/README.md)
- [MVP plan and status](docs/mvp-plan.md) · [Roadmap](docs/roadmap.md) · [Open questions](docs/open-questions.md)
- Reference (generated): [CLI](docs/reference/cli.md) · [File formats](docs/reference/formats.md) ·
  [Diagnostic codes](docs/reference/errors.md) · [JSON Schemas](docs/reference/schemas/)
- [Progress log](docs/progress-log.md) · founding direction: `Agent-native Rust game engine architecture direction.pdf`
