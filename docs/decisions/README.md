# Architecture decision records

One file per decision: context, decision, consequences, and **when to revisit**. Decisions are changed by
adding a new ADR that supersedes the old one (and marking the old one), never by silently editing.

| ADR | decision | status |
|---|---|---|
| [0001](0001-build-from-scratch.md) | Build from scratch; own the core, rent platform layers | accepted |
| [0002](0002-cli-first-agent-surface.md) | Files + CLI with JSON are the agent interface; MCP is a later, thin adapter | accepted |
| [0003](0003-toml-and-schema-registry.md) | TOML text formats behind a span tree; one schema registry for validation, defaults, docs, schemas | accepted |
| [0004](0004-few-dependencies-no-proc-macros.md) | Few dependencies, zero proc macros in the runtime tree | accepted |
| [0005](0005-determinism-model.md) | Determinism contract: fixed step, seeded PCG32, exact float ops, integer raster | accepted |
| [0006](0006-fat-entities-and-game-trait.md) | Fat entities + props instead of an ECS; Rust code via a `Game` trait | accepted (revisit at M3) |
| [0007](0007-draw-list-and-cpu-rasterizer.md) | The draw list is the central render artifact; deterministic CPU rasterizer first | accepted (revisit for GPU) |
| [0008](0008-web-via-c-abi.md) | Web: wasm32 cdylib with a hand-written C ABI, no wasm-bindgen | accepted |
| [0009](0009-stable-diagnostics.md) | Stable diagnostic codes, rustc-style text, `explain` | accepted |
| [0010](0010-input-actions-taps-scripts.md) | Input = abstract actions + taps (touch first); scripted input = playtest files | accepted |
| [0011](0011-game-logic-data-first-rust-escape-hatch.md) | Game logic: small data-level set (prop actions, `when` conditions, `label`, screen anchoring); Rust is the escape hatch | accepted, built (M1.6) |
| [0012](0012-pushable-entities-hold-plates.md) | `pushable` entities: movers push them one cell; triggers count them | accepted, built |
| [0013](0013-first-reference-game-isometric-builder.md) | First reference game: Tiny Town, an isometric builder; builder kit v1 (terrain maps, ground taps, `spawn`, `footprint`, `from_prefab`) | accepted, built (M3.1) |
| [0014](0014-refusal-signal-and-count-props.md) | Builder kit v2: refused spawns emit `refused` (with `when` `reason`); `counts` props count enabled copies of a prefab | accepted, built (M3.2); cold-agent run #8 |
| [0015](0015-several-actions-per-connect.md) | Several actions per `[[connect]]` (`actions = [...]`): one `when`, in order, a refused spawn stops the rest | accepted, built (M3.3); cold-agent run #9 |
| [0016](0016-spawn-costs-and-funds-refusal.md) | Spawn `cost` (paid only if placed) and the `funds` refusal reason | accepted, built (M3.4) |
| [0017](0017-web-profile-and-budgets.md) | Web profile: size-first with the renderer at `opt-level = 3`; budgets 800 KB wasm and a render-time limit | accepted, built |
| [0018](0018-paint-path-for-players.md) | A paint path for players (`paint`, `Presenter`, `slate_present`): the reference pixels 41× faster, unchanged frames skipped; the reference rasterizer stays for checks | accepted, built |

Template: copy any ADR; keep it under a page.
