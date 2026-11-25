# Roadmap

Milestones after the MVP. Order and scope are proposals: guardrail 2 says **reference games drive every
feature**, so each milestone names the game need that pulls it in. Items marked **(Q#)** wait on a
decision in [open-questions.md](open-questions.md).

## M1.5 — Close the loop on the idea (next)

- ~~Run the cold-agent test~~ — done: [run #1](experiments/2026-11-25-cold-agent-1.md) and
  [run #2](experiments/2026-11-25-cold-agent-2.md) and [run #3](experiments/2026-11-25-cold-agent-3.md)
  (beyond the engine's limits) and [run #4](experiments/2026-11-25-cold-agent-4.md) (run #3 again with the
  limits table: verdicts at command 2, −25% time) all PASS; their findings are fixed. Next cold run after
  Q14 is decided and built.
- Push to a remote and get CI green on GitHub (Q10).
- Small fixes found during M1: validate required keys after prefab merge; block edge shading so adjacent
  blocks read as separate. (Ground taps were built in M3.1.)
- Feature candidates from [cold-agent run #3](experiments/2026-11-25-cold-agent-3.md): text/number display,
  prop actions, conditions and a HUD layer were built in M1.6 (ADR 0011). Still candidates, waiting for a
  game that needs them: pushable blocks or weights for plates; a single "cell is passable" expectation.

## M1.6 — Data logic v1 ([ADR 0011](decisions/0011-game-logic-data-first-rust-escape-hatch.md)) — done 2026-11-25

Need: scores, counts and conditions (cold-agent runs #3/#4 spent 14–19 min building them from parts).

- `[[connect]]` actions `add` / `set` on props (target emits `changed`); props become runtime state.
- `when = [...]` conditions on `[[connect]]` (occupied / enabled / prop comparisons; all must hold).
- `label` component with `{prop}` placeholders, owned bitmap font; `screen_pos = [x, y]` anchoring for HUDs.
- Static checks, events (condition results, prop before/after), frame report "clipped text".
- Acceptance: run #3's task with no approximations in ≤ 5 min; budgets and determinism hold.
- Status: all of the above is built, with `examples/coin_vault` as the reference example (4 playtests).
  Budgets and determinism hold (native = web for state and image hashes).
- Acceptance ([run #5](experiments/2026-11-25-cold-agent-5.md)): every logic requirement supported, 0 warnings,
  10 min 2 s (target ≤ 5 min missed; run #4 took 14 min 6 s). The remaining approximation is the plate
  holder, a separate limit.
- Follow-up built: `pushable` entities that movers push and triggers count ([ADR 0012](decisions/0012-pushable-entities-hold-plates.md)),
  evidence from runs #3–#5. Accepted by [run #6](experiments/2026-11-25-cold-agent-6.md): the full puzzle, no
  stand-ins, 0 warnings, 7 min 41 s.
- Still candidates: switching control between several movers; pulling or chain-pushing; an expectation that
  holds on every frame of a range ("only while" claims); `enabled_when` derived state (fewer mirrored
  connections for AND doors).

## M2 — The live loop

Need: people (and agents) iterate on feel, not just on end states.

- `slate export web` → static site (wasm + page + bundle) for hosting on any static server (Q11).
- Record from the web player: export the live session as a playtest (`[[input]]` list) — record/replay.
- `slate watch`: re-run `check`/`test` on file change, JSON lines out.
- (Text labels moved to M1.6.)
- Image sprites from PNG files (Q6), still deterministic in the CPU rasterizer.
- Native window player (winit + softbuffer or wgpu) — only if the web player is not enough (Q4).

## M3 — Reference game 1 and its kit

Need: the first real game. Q3 is decided: the **isometric builder**, Tiny Town
([spec](games/tiny-town.md), [ADR 0013](decisions/0013-first-reference-game-isometric-builder.md)).

- **M3.1 — builder kit v1 + Tiny Town: done 2026-11-25.**
  - Terrain maps (`[ground] map` + `[[terrain]]`, `build`).
  - Ground-cell taps (`from = "ground"`).
  - `spawn` with placement rules (edge, terrain, occupied).
  - Multi-cell `footprint` with sliced draw order.
  - `from_prefab` / `to = "self"`, `block.top`, `[[expect]] prefab` + `count`.
  - `slate inspect --place` ("can I build here?", with tap pixels).
  - `examples/tiny_town` with 3 playtests; native = web.
- Next: cold-agent run #7 on a different builder task (ADR 0013's acceptance).
- Candidates, waiting for evidence from the game or a cold run:
  - a signal on refused placements (a "can't build there" message);
  - per-prefab costs or grouped connections (Tiny Town needs 26 connections, 9 of them for clear/refund);
  - drag input (ADR 0010);
  - terrain that blocks walking;
  - rotation;
  - `opt-level = "z"` for the web build (−40 KB measured, frame time not yet measured).

## M4 — Rust game code, fast

Need: logic that data alone cannot express.

- `slate new --rust`: a game crate whose binary is `slate_cli::main_with(MyGame)`.
- Hot-patching with subsecond for code edits; budget the code edit loop.
- Game code in the browser: build the game crate to wasm with the same C ABI (Q7).
- Typed props access helpers; custom component registration (maybe) if props are not enough.

## M5 — Tablets and the learning kits

- Tracing kit (follow-the-path letters/shapes, stroke checking) — reference game 2.
- Quiz/lessons kit (lesson content as text files with a schema) — reference game 3.
- Packaging for iPad/Android (rented platform crates); measure the `tablet.startup_ms` budget (Q8).
- Audio (rented; Q13).

## M6 — Optional MCP adapter

- `slate mcp`: a thin stdio server with a handful of tools that map 1:1 onto CLI commands (`check`, `run`,
  `shot`, `inspect`, `explain`), discovered progressively (Q9). The CLI stays the source of truth.

## Always

- Budgets only tighten unless a commit says why.
- Every release regenerates docs/reference and the template AGENTS.md; migration notes are
  machine-readable (a `migrations/` list of `{from, to, file, key, action}` entries) once formats change.
