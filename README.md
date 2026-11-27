# rwgpuengine

**An agent-native game engine in Rust**: a game is text files, and the engine is a command line that
checks, runs, renders and inspects them, deterministically, without a screen, with JSON output and
diagnostics in the style of the Rust compiler. It is built for coding agents first, and for the people who
direct them. The name is **R**ust + **wgpu** + engine; the command is `rwgpu`.

> **Status: early.** What works today (v0.1.0) is the half of the engine an agent works with: text formats
> with a schema, the command line, a deterministic simulation, and a world drawn in 3D by a deterministic
> CPU reference renderer, with a camera that turns, zooms and switches between an isometric and a
> perspective view, visual checks, and a web player. It runs natively and in the browser with identical
> results. There is **no GPU renderer yet**: boxes, flat tiles and camera-facing sprites are all it draws.
>
> **Where it is going:** a **3D engine on the GPU through wgpu, native and web**, for strategy games, city
> builders, car-building and simulation games, and children's games, with art made of simple shapes written
> as text. The agent's loop stays free of the GPU. Read the
> [direction](docs/vision.md#direction-update--2026-11-25-owner) and the
> [3D architecture proposal](docs/proposals/3d-architecture.md), which has the measurements behind it; its
> next step is the wgpu players.
>
> Until 2026-11-25 the engine had the working codename "slate"; older records still use it
> ([ADR 0019](docs/decisions/0019-name-rwgpuengine.md)). Nothing is published on crates.io yet.

```console
$ rwgpu new demo && cd demo
$ rwgpu check
check ok: 1 scene(s), 5 prefab(s), 1 playtest(s) — 0 error(s), 0 warning(s)
$ rwgpu run --frames 300 --input tests/smoke.toml
ran scenes/main.toml for 300 frames (seed 1, input tests/smoke.toml)
  state hash  157c6d8835b22965
  entities    8 (7 enabled)
  events      input 2, moved 6, trigger 2, signal 2, action 1
$ rwgpu shot --input tests/smoke.toml --report        # PNG + what is drawn where
$ rwgpu inspect --frame 300 --entity door --events --json
$ rwgpu test
test smoke ... ok (3 expect(s), 0 shot(s), 120 frames, final state acf3bb04f63b4b72)
```

Mistakes come back like this (also as JSON with `--json`):

```text
error[E0103]: unknown key `sprit` in entity
  --> scenes/main.toml:41:1
   |
41 | sprit = { color = "#fff" }
   | ^^^^^
   = help: did you mean `sprite`?
```

## Why

Coding agents are best at reading files and running commands. rwgpuengine makes those the engine's main
interface: text files with a published schema, a CLI where every command speaks JSON, a deterministic
headless core, a queryable world with an event log that says *why* things happened, and built-in visual
checks. The same state hashes and the same pixels have been measured on macOS, Linux and Windows, on x86-64 and
arm64, and in the browser, and the gate checks the web build against the native one on every push. Read
[docs/vision.md](docs/vision.md).

## Quick start (from source)

Requires `rustup`; the toolchain (Rust 1.100.0 + wasm32) installs itself from `rust-toolchain.toml`.

```bash
cargo build --release            # target/release/rwgpu
cargo rwgpu test examples/iso_garden
```

In a browser, drawn with WebGPU where the browser has it and by the CPU reference renderer where it has not
(the page says which in one line under the picture):

```bash
cargo xtask web-player                  # once: the web player in target/web-player (installs wasm-bindgen 0.2.130 into target/tools the first time)
cargo rwgpu serve examples/tiny_town    # open http://127.0.0.1:4410/
```

Tested headless in Chrome 157 and 158 and Edge 157 and 158; the 3D proposal lists the other browsers with
WebGPU. Add
`?input=tests/turn_and_build.toml&frames=90` to the URL to replay a playtest; the state hash on the page
matches `rwgpu run`. `?renderer=cpu` draws with the CPU reference renderer. Edit any file in the project and
the page reloads it. Without `cargo xtask web-player`, `rwgpu serve` serves the CPU player instead
(`cargo xtask web` builds it); its first line says which it serves
([ADR 0024](docs/decisions/0024-web-player-on-webgpu.md)).

Put a game on the web: `rwgpu export web` writes a folder that any static web host can serve (GitHub Pages,
any web server, a folder on a site), with the same player `rwgpu serve` finds. The project must pass
`rwgpu check` first.

```bash
cargo xtask dist                 # target/release/rwgpu, with both web players next to it
target/release/rwgpu export web examples/tiny_town --out target/site
target/release/rwgpu serve --site target/site   # try it as a host would serve it: http://127.0.0.1:4410/
```

Copy the folder to the host as it is. Every address in it is relative, so it works at the root of a site or
under a path such as `/games/town/`. It needs a web server, not a file opened from disk; WebGPU also needs a
secure context (https, or http on `localhost`), and elsewhere the page draws with the CPU. A host that sends
`.wasm` as `application/wasm` lets the browser compile while it downloads (others work too, a little slower).
The folder holds the page, the player and the game's text files, and no path of the machine that made it;
`--json` says which player and lists every file with its size and hash. `cargo xtask dist --agent` builds
only the CPU player, which needs no wasm-bindgen
([ADR 0025](docs/decisions/0025-rwgpu-export-web.md)).

A window, drawn with the GPU (Vulkan, Metal or Direct3D 12; on Linux without a graphics card, Mesa's
`mesa-vulkan-drivers`). The native player is a program of its own, so `rwgpu` never needs a GPU; build and
install it once:

```bash
cargo install --locked --path players/rwgpuengine_player   # puts rwgpu-player on PATH
cargo rwgpu play examples/tiny_town
cargo rwgpu play examples/tiny_town --input tests/turn_and_build.toml   # watch a playtest; prints its state hash
```

That build is for your own machine: like any plain Cargo build, it writes this machine's folders into the
program. To give `rwgpu` or `rwgpu-player` to someone else, build them with `cargo xtask dist`, which
leaves those paths out and checks that it did ([ADR 0023](docs/decisions/0023-release-builds-without-local-paths.md)).

Arrows or W A S D, Enter or Space and Escape are the game's keys, and a click is a tap. Q and E turn the
camera, right-drag turns it freely, middle-drag pans, the wheel zooms and V switches to perspective
(`rwgpu help play` lists them all). The web player has the same bindings, and on a touch screen a finger
taps and swipes and two fingers pinch.

## Repository map

| path | what |
|---|---|
| `crates/rwgpuengine_core` | JSON, diagnostics + stable codes, RNG, hashing (zero deps) |
| `crates/rwgpuengine_data` | file formats: schema registry, validation, loaders, static checks |
| `crates/rwgpuengine_sim` | deterministic world, built-in behaviors, signals, event log |
| `crates/rwgpuengine_render` | the camera, the render list, the CPU reference renderer (3D), frame report, picking, PNG |
| `crates/rwgpuengine_runtime` | sessions, scripted input, playtests, the `Game` trait for Rust code |
| `crates/rwgpuengine_cli` | the `rwgpu` command line (+ project template, dev server) |
| `crates/rwgpuengine_web` | the CPU player's wasm module, with a small C ABI; `web/` holds its page and JS glue |
| `players/rwgpuengine_gpu` | the players' renderer: the render list drawn with wgpu, no window (its own workspace, `players/`) |
| `players/rwgpuengine_player` | the players: `rwgpu-player`, a project in a winit window (`rwgpu play` starts it), and the web player on WebGPU, the same code built for the browser with its page in `web/` (`rwgpu serve` serves it); both stepped by the engine and drawn by that renderer |
| `players/gpu_check` | holds that renderer to the CPU reference renderer, scene by scene, and replays every example playtest with the player |
| `examples/iso_garden` | reference example: plate opens gate, goal raises flag; 2 playtests |
| `examples/coin_vault` | reference example for data logic: coin counter on a HUD, vault opens at 3, AND lock held by a pushed crate; 4 playtests |
| `examples/tiny_town` | **reference game 1** ([spec](docs/games/tiny-town.md)): a builder — terrain map, tap to build 2 × 2 houses, trees and a well within a coin budget, clear with refund, a job card that counts copies, hints that say why nothing was built; 4 playtests, one of which turns the camera and builds in perspective |
| `xtask` | `cargo xtask ci | repo-check | dist | web | web-player | smoke | budgets | docs | wasm-check | export-check | pages | gpu` |
| `web/` | the CPU player's page and glue; `check.mjs`, which holds its wasm module to the native command; `export-check.mjs` and `static-host.mjs`, which play an exported folder from a plain static host |
| `docs/` | vision, architecture, decisions (ADRs), proposals, roadmap, open questions, reference |

## Development

```bash
cargo xtask ci        # repository check, fmt, clippy (native+wasm), tests, docs drift, examples, budgets, wasm check, export check — what CI runs
cargo xtask dist      # release artifacts to share: no path of the machine that built them (ADR 0023)
cargo xtask gpu       # the players' half: the wgpu renderer against the CPU reference, the native player, the web player in a headless Chrome, players' budgets (needs a GPU)
```

Hard limits in [budgets.toml](budgets.toml) fail the build (wasm size, CLI size, dependency count,
zero proc macros, frame time, smoke-chain time, rebuild time). The wasm check needs Node.js; without it the
check is skipped locally and CI still runs it. Conventions: [docs/conventions.md](docs/conventions.md).
Contributing: [CONTRIBUTING.md](CONTRIBUTING.md). Agents working on this repo: start with [AGENTS.md](AGENTS.md).

## Documentation

- [Vision](docs/vision.md) · [Architecture](docs/architecture.md) · [Decisions](docs/decisions/README.md)
- [MVP plan and status](docs/mvp-plan.md) · [Roadmap](docs/roadmap.md) · [Open questions](docs/open-questions.md)
- Reference (generated): [CLI](docs/reference/cli.md) · [File formats](docs/reference/formats.md) ·
  [Diagnostic codes](docs/reference/errors.md) · [JSON Schemas](docs/reference/schemas/)
- [Progress log](docs/progress-log.md) · [Audit of the first baseline](docs/audits/2026-11-25-architecture-audit.md)

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
