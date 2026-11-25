# Roadmap

Milestones after the MVP. Order and scope are proposals: guardrail 2 says **reference games drive every
feature**, so each milestone names the game need that pulls it in. Items marked **(Q#)** wait on a
decision in [open-questions.md](open-questions.md).

> **Out of date from M2 on.** On 2026-11-25 the owner set the target to **3D on the GPU (wgpu), native and web**
> ([direction update](vision.md#direction-update--2026-11-25-owner)). The milestones below were written for a
> 2D engine for learning games on tablets. They stay as a record until the 3D direction study replaces them.

## Now (2026-11-25, after the audit and the direction update)

1. ~~Audit~~: done ([report](audits/2026-11-25-architecture-audit.md)).
2. ~~First consolidation step, the players' paint path~~: done
   ([ADR 0018](decisions/0018-paint-path-for-players.md)).
3. ~~The name~~: done. The engine is **rwgpuengine** and the command is `rwgpu`
   ([ADR 0019](decisions/0019-name-rwgpuengine.md)).
4. ~~The 3D direction study~~: done. [The 3D architecture proposal](proposals/3d-architecture.md); the owner
   said to continue.
5. **Open-source readiness and the first public push** (in progress). Done locally: LICENSE (Apache-2.0),
   crate metadata, CI for Linux, Windows and macOS with the wasm check in the gate, README and CONTRIBUTING.
   Waiting for the owner's word, because they cannot be undone: rewriting history to the owner's no-reply
   address without co-author lines, and the push itself to the public repository.
6. Then, in the order the proposal gives (its section 7): the GPU-free 3D core with a CPU reference
   renderer; wgpu players natively and on the web; the simulation at scale; shapes as text and the town
   reference game; Rust game code; templates per kind of game. Then demos, the MCP adapter and the desktop
   app.

Not started and now in question: the audit's step 4 (splitting `load.rs` and `world.rs`). The 3D world model
will reshape both files, so they are restructured as part of that work, not before it.

## Before the direction update: pause, baseline, consolidate (owner's order, 2026-11-25)

Core feature work is **paused** at [baseline 2026-11-25](baselines/2026-11-25.md) (git tag
`baseline-2026-11-25`). The order from here:

1. **Owner review** of the baseline and the handoff notes.
2. **The senior architect session.** The owner starts a new session in this repository on a higher-end model
   (Fable 5.2, Extra mode) with the [brief](handoff/senior-architect-brief.md). It:
   - audits rwgpuengine against the vision;
   - consolidates the core and lays the complete platform foundation;
   - becomes the project's **main supervisor** from then on.

   Its plan, in approved steps, replaces the milestones below where they differ.
3. **Demos**: real games built with rwgpuengine on the consolidated platform, each with the procedural steps of
   how it was built (`BUILD.md`). They are also the foundation's acceptance test.
4. **The desktop app** for game producers, with agentic support through a bridge to Claude Desktop and
   other agent platforms later ([proposal](proposals/desktop-app.md)). It starts only after the foundation.

The M3 feature candidates below are the architect's backlog to judge.

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

- `rwgpu export web` → static site (wasm + page + bundle) for hosting on any static server (Q11).
- Record from the web player: export the live session as a playtest (`[[input]]` list) — record/replay.
- `rwgpu watch`: re-run `check`/`test` on file change, JSON lines out.
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
  - `rwgpu inspect --place` ("can I build here?", with tap pixels).
  - `examples/tiny_town` with 3 playtests; native = web.
- Accepted by [cold-agent run #7](experiments/2026-11-25-cold-agent-7.md): a farm builder, every requirement
  supported, 0 warnings, 8 min 44 s. Its findings are fixed (`rwgpu test <file>` false pass, prefab filter on
  event expectations, two missing format tables).
- **M3.2 — builder kit v2: done 2026-11-25** ([ADR 0014](decisions/0014-refusal-signal-and-count-props.md)),
  from the evidence of Tiny Town **and** run #7:
  - refused spawns make their source emit `refused`; `when = [{ reason = "terrain" }]` picks one reason;
  - `counts = { houses = "house" }` props count enabled copies and emit `changed`, replacing hand-kept
    counters (Tiny Town: 6 counting connections → 0; a hint line now says why nothing was built).
- Accepted by [cold-agent run #8](experiments/2026-11-25-cold-agent-8.md): a park builder with three refusal
  messages and a count-driven goal, everything supported, 0 warnings, 6 min 24 s. Its findings are fixed
  (quiet `--place`, prefab names on unnamed copies, two recipe facts).
- **M3.3 — several actions per `[[connect]]`: done 2026-11-25**
  ([ADR 0015](decisions/0015-several-actions-per-connect.md)). `actions = [...]` shares one source, signal and
  `when` (checked once); a refused spawn stops the rest, so `[spawn, pay]` is correct. Tiny Town went from 27
  to 20 connections with the same final state hashes. Evidence: run #8 ranked it first; it would cut the
  three builder games from 41 / 30 / 27 to 27 / 19 / 24 tables.
- Accepted by [cold-agent run #9](experiments/2026-11-25-cold-agent-9.md): run #8's task again, **24
  connections instead of 41**, 5 min 54 s, 0 warnings. Its findings are fixed (short flags, "action i of n" in
  list messages, `[[expect]] changed`, three recipe facts).
- **M3.4 — spawn costs: done 2026-11-25** ([ADR 0016](decisions/0016-spawn-costs-and-funds-refusal.md)).
  `cost = { entity, prop, amount }` on a spawn is checked first and paid only if placed; too little is a
  refusal with reason `funds`. Tiny Town's conditions no longer mention coins, and the file-order rule for
  coin messages is gone. Asked for by runs #8 and #9.
- **Size pass: done 2026-11-25** ([ADR 0017](decisions/0017-web-profile-and-budgets.md)). The web profile is
  size-first with a fast renderer: 643,317 bytes (−31 KB) and −37 % frame time. Budget 800,000 (owner), plus a
  new render-time budget.
- Backlog for the architect (see "Now" above):
  - a "not equal" / `in` condition, or an else branch (runs #8 and #9 both added a flag prop);
  - a cold-agent run on M3.4.
- Candidates, waiting for more evidence:
  - a tap on a building with a build tool emits `refused` (`occupied`) (run #9);
  - derived boolean props (`open = all(...)`) (runs #8, #9);
  - `when` on the refused terrain's name (run #8);
  - a placement anchor (centre a multi-cell building on the tap; run #7);
  - drag input (ADR 0010);
  - terrain that blocks walking;
  - rotation;

## M4 — Rust game code, fast

Need: logic that data alone cannot express.

- `rwgpu new --rust`: a game crate whose binary is `rwgpuengine_cli::main_with(MyGame)`.
- Hot-patching with subsecond for code edits; budget the code edit loop.
- Game code in the browser: build the game crate to wasm with the same C ABI (Q7).
- Typed props access helpers; custom component registration (maybe) if props are not enough.

## M5 — Tablets and the learning kits

- Tracing kit (follow-the-path letters/shapes, stroke checking) — reference game 2.
- Quiz/lessons kit (lesson content as text files with a schema) — reference game 3.
- Packaging for iPad/Android (rented platform crates); measure the `tablet.startup_ms` budget (Q8).
- Audio (rented; Q13).

## M6 — Optional MCP adapter

- `rwgpu mcp`: a thin stdio server with a handful of tools that map 1:1 onto CLI commands (`check`, `run`,
  `shot`, `inspect`, `explain`), discovered progressively (Q9). The CLI stays the source of truth.

## Always

- Budgets only tighten unless a commit says why.
- Every release regenerates docs/reference and the template AGENTS.md; migration notes are
  machine-readable (a `migrations/` list of `{from, to, file, key, action}` entries) once formats change.
