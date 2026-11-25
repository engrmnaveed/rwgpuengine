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
- Accepted by [cold-agent run #7](experiments/2026-11-25-cold-agent-7.md): a farm builder, every requirement
  supported, 0 warnings, 8 min 44 s. Its findings are fixed (`slate test <file>` false pass, prefab filter on
  event expectations, two missing format tables).
- **M3.2 — builder kit v2: done 2026-11-25** ([ADR 0014](decisions/0014-refusal-signal-and-count-props.md)),
  from the evidence of Tiny Town **and** run #7:
  - refused spawns make their source emit `refused`; `when = [{ reason = "terrain" }]` picks one reason;
  - `counts = { houses = "house" }` props count enabled copies and emit `changed`, replacing hand-kept
    counters (Tiny Town: 6 counting connections → 0; a hint line now says why nothing was built).
- Accepted by [cold-agent run #8](experiments/2026-11-25-cold-agent-8.md): a park builder with three refusal
  messages and a count-driven goal, everything supported, 0 warnings, 6 min 24 s. Its findings are fixed
  (quiet `--place`, prefab names on unnamed copies, two recipe facts).
- Next (evidence from three builder games): **several actions per `[[connect]]`**. Merging connections that
  share source, signal and `when` would cut the park from 41 to 27, the farm from 30 to 19, and Tiny Town
  from 27 to 24. Run #8 ranked it first.
- Candidates, waiting for more evidence:
  - a "not equal" / `in` condition (run #8 used a flag prop);
  - per-prefab costs with an automatic "can't afford" refusal (runs #7 and #8);
  - `when` on the refused terrain's name (run #8);
  - a placement anchor (centre a multi-cell building on the tap; run #7);
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
