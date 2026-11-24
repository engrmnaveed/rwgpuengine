# ADR 0011: Game logic — a small data-level logic set, Rust as the escape hatch

- Status: accepted, 2026-11-25 (resolves open question Q14; partly resolves Q5)
- Decided by: the project owner's rule "choose on KPIs only: best performance and flexibility trade-off"

## Context

Cold-agent runs [#3](../experiments/2026-11-25-cold-agent-3.md) and [#4](../experiments/2026-11-25-cold-agent-4.md)
asked for a door that needs two plates held (AND) and an on-screen score. slate 0.1 has neither conditions nor
counters nor text in data files, and Rust game code (`Game` trait) cannot be loaded by `slate new` projects
yet. Agents built honest workarounds from parts (stacked doors, sprite digits, actor "bits"), at a cost of
14–19 minutes and dozens of entities each time. Learning games need scores, counts and conditions constantly.

Options:
- **(a) Data-first:** add a deliberately small logic set to the data files.
- **(b) Rust-first:** keep data minimal; make Rust game code easy to add (M4) and put all logic there.

## The KPIs

| KPI | (a) data logic | (b) Rust logic | evidence |
|---|---|---|---|
| **Edit → verified** | 0.01 s warm (`check` + `test`, no build); browser reloads in ~1 s | native rebuild 0.39–0.49 s (dev), 1.4 s (release); browser module 0.9–1.3 s, and growing with game code size | measured 2026-11-25, M5 Pro, engine-sized crates |
| **Agent time for a counter / AND / score display** | a few lines of TOML each (target: run #3's task in ≤ 5 min) | the same few lines of Rust plus a crate, a build and a per-game wasm build | runs #3/#4 took 14–19 min building them from parts |
| **Checked before running** | yes: `slate check` validates names, props, types, references, with codes and fix hints | only what rustc catches; logic errors surface in playtests | ADR 0003, 0009 |
| **Queryable / assertable** | logic state lives in props: `inspect` shows it, `[[expect]] props` asserts it, events explain it | only if the game implements `Game::inspect`; otherwise opaque | ADR 0006 |
| **Determinism** | guaranteed by the engine (same rules as everything else) | the author's responsibility; clippy rules do not apply to user crates | ADR 0005 |
| **Web / tablet delivery** | runs in the one shared 504 KB player, no build | needs a per-game wasm build pipeline (not built yet, Q7) | ADR 0008 |
| **Runtime performance** | event-driven: evaluated only when a signal fires; cost is negligible next to rendering | faster per operation; matters only for heavy per-frame logic (AI, pathfinding, simulation) | the frame is dominated by rasterization |
| **Flexibility** | bounded by design (a small set) | unbounded (Turing-complete) | — |

Neither option wins alone. (b) is more flexible and faster per operation, but loses every agent-loop KPI. Its
per-operation speed is irrelevant at this logic scale, and its flexibility is only needed for the minority of
logic that data cannot express. (a) wins the agent-loop KPIs but is bounded.

## Decision

**Both, split by job: data for the common logic, Rust for the rest.**

1. A deliberately small data-level logic set, enough for counters, conditions and displaying values:
   - **Prop actions** on `[[connect]]`: `action = "add"` or `"set"` with `prop = "score"` and `value = 1`. The
     target then emits `changed`. Props become runtime state, visible in `inspect` and assertable.
   - **Conditions** on `[[connect]]`: `when = [ ... ]`, all of which must hold (AND). Each item is one of:
     `{ entity = "plate_b", occupied = true }`, `{ entity = "gate", enabled = false }`,
     `{ entity = "hud", prop = "score", at_least = 3 }` (also `at_most`, `equals`). OR = several connects;
     NOT = `false` / `at_most`.
   - **A `label` component**: text with `{prop}` placeholders filled from the entity's props, drawn with an
     owned, deterministic bitmap font (no new dependency; resolves Q5 for Latin text now).
   - **Screen anchoring**: `screen = [x, y]` places a label or sprite in frame pixels (a HUD), independent of
     the camera.
   - Static checks for all of it (unknown entities and props, impossible conditions); a frame report that
     flags clipped text; events that log every condition evaluated and every prop change (with
     before/after values).
2. **Rust stays the escape hatch** for anything beyond that set (AI, pathfinding, procedural generation). The
   `Game` trait is unchanged; making it loadable from projects remains M4 (Q7).
3. **No expression language.** Conditions are structured TOML, not strings to parse. Data logic is expanded
   only when a reference game or a cold-agent run shows a pattern that agents keep rebuilding from parts.

## Acceptance (measured after implementation)

- Run #3's task again, same prompt: every requirement **supported** (no approximations), `slate check` clean,
  0 warnings, done in **≤ 5 min** (run #4: 14 min 6 s).
- Budgets hold: wasm ≤ 700 KB, zero proc macros, no new runtime dependency, rebuild ≤ 10 s, smoke chain ≤ 4 s.
- Determinism holds: native and web state and image hashes still match; existing reference images unchanged.

## Consequences

- The "Limits in this version" table shrinks: text, counters and conditions move to "supported", and the
  workaround recipes for them are retired.
- Props change at runtime. `state_hash` already covers them. Docs must stop saying they are read-only.
- More engine surface to keep small: every addition to the logic set needs evidence (a game or an agent run).

## Revisit when

A reference game needs logic that would require loops, variables beyond props, or an expression language.
That is the signal to make Rust game code first-class (M4) rather than grow the data language.
