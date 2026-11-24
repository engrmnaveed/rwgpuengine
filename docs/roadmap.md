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
- Small fixes found during M1: validate required keys after prefab merge; `tap_ground` event with the
  cell under a tap; block edge shading so adjacent blocks read as separate.
- Feature candidates from [cold-agent run #3](experiments/2026-11-25-cold-agent-3.md): text/number display,
  prop actions, conditions and a HUD layer are scheduled as M1.6 (ADR 0011). Still candidates, waiting for a
  game that needs them: pushable blocks or weights for plates; a single "cell is passable" expectation.

## M1.6 — Data logic v1 ([ADR 0011](decisions/0011-game-logic-data-first-rust-escape-hatch.md)) — next

Need: scores, counts and conditions (cold-agent runs #3/#4 spent 14–19 min building them from parts).

- `[[connect]]` actions `add` / `set` on props (target emits `changed`); props become runtime state.
- `when = [...]` conditions on `[[connect]]` (occupied / enabled / prop comparisons; all must hold).
- `label` component with `{prop}` placeholders, owned bitmap font; `screen = [x, y]` anchoring for HUDs.
- Static checks, events (condition results, prop before/after), frame report "clipped text".
- Acceptance: run #3's task with no approximations in ≤ 5 min; budgets and determinism hold.

## M2 — The live loop

Need: people (and agents) iterate on feel, not just on end states.

- `slate export web` → static site (wasm + page + bundle) for hosting on any static server (Q11).
- Record from the web player: export the live session as a playtest (`[[input]]` list) — record/replay.
- `slate watch`: re-run `check`/`test` on file change, JSON lines out.
- (Text labels moved to M1.6.)
- Image sprites from PNG files (Q6), still deterministic in the CPU rasterizer.
- Native window player (winit + softbuffer or wgpu) — only if the web player is not enough (Q4).

## M3 — Reference game 1 and its kit

Need: the first real game (Q3 picks which). Likely the **isometric builder**, which pulls in:

- Iso grid kit: multi-cell objects, placement rules, "can I build here" queries, ground-cell taps.
- Draw sorting for multi-cell and tall objects.
- Tilemaps kit (layered ground, terrain types).
- Score/state: props + signals cover most; add counters/timers if the game needs them.

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
