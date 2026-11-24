# Progress log

Newest first. Commit messages carry the detail; this page is the narrative and the place to record
findings that are not code (experiment results, surprises, things to remember).

## 2026-11-25 — Cold-agent test #4: the limits table works

- Run #3's task again, same prompt, with the new limits table. Every verdict came at command 2
  (`slate explain limits`) instead of commands 7–21. 14 min 6 s vs 18 min 42 s, 37 vs 59 tool calls,
  0 warnings vs 40. Re-verified independently.
- It spent the saved time on a better approximation: a 2-bit counter kept in hidden stationary actors read by
  triggers, correct in any pickup order, with 6 thorough playtests.
- Remaining gaps were recipes that were vague or subtly wrong (collectibles could double-count, "a chain of
  toggles" can't count in any order). Fixed with a one-shot collectible recipe, a stationary-actor recipe, an
  any-order counting recipe, documented trigger semantics (enable/disable counts; occupied at frame 0 does not
  fire), value ranges in the reference, the projection formula, and a new W0206 actor-in-solid warning.
- Across four runs, each round removed one class of gap and the next found a subtler one.

## 2026-11-25 — Cold-agent test #3: PASS, honestly, but slow

- Task deliberately beyond the engine: a door that opens only while two plates are both held, plus an
  on-screen score counter. slate has no AND, no text, no counters.
- The agent built a real AND (two stacked solid door leaves, each opened by its own plate, held by patrolling
  sentries) and a seven-segment digit from 17 layered sprites that is correct for every pickup order, and said
  clearly what was approximated and why. Nothing faked. Re-verified independently.
- But it took **18 min 42 s**, 3× run #2: it found every limit by noticing absences in the schema. Fix: a
  "Limits in this version" table (with workarounds and their costs) in AGENTS.md and `slate explain limits`.
- Also: `allow = [...]` to silence intended warnings (its level: 40 → 0), documented draw-order ties and
  shared mover input, `--input` reads files outside the project. Its feature wishes went to the roadmap and
  to a new owner decision, Q14 (game logic in data or Rust?).

## 2026-11-25 — Cold-agent test #2: PASS (harder task)

- Task: tap a lever to open gate 1, slip past a patrolling guard, press a plate for gate 2, reach the goal;
  plus playtests proving the guard blocks bad timing and gate 2 stays shut without the plate.
- PASS in **6 min 7 s** (26 tool calls). Tap coordinates, guard timing and the solution were right on the
  first try. Re-verified independently. Record: [experiments/2026-11-25-cold-agent-2.md](experiments/2026-11-25-cold-agent-2.md).
- Gaps moved from "how do I write this" (run #1) to "how do I prove this precisely": it couldn't assert
  *who* blocked the player or that something *never* happened, and it had to measure patrol timing.
- Fixed: `[[expect]] other` and `at_most`; exact event-frame, patrol, wander and mover-retry semantics;
  `slate explain events`; `run --track NAME` (cell timeline with frame ranges); guaranteed `tap` points in
  `inspect`; runtime state in the text table; the template's smoke test names its scene.

## 2026-11-25 — Cold-agent test #1: PASS

- A fresh agent with only the binary and an empty folder ran `slate new`, read AGENTS.md and built an
  original walled-garden level with a plate, gate, goal and gem, plus a solving playtest and a negative
  one, in **3 min 58 s** (27 tool calls). `check` clean, `test` green, references viewed. Re-verified
  independently. Record: [experiments/2026-11-25-cold-agent-1.md](experiments/2026-11-25-cold-agent-1.md).
- Its first scene was valid on the first try; it hit no check errors and no failed expectations.
- Its 10 confusion points became fixes: exact commands in image diagnostics, a Recipes section (goal,
  doors, collectibles, negative tests, taps), `[[expect]] reason`, subject-only `entity` for event
  expects, precise mover timing, W0304 hidden-trigger + frame-report warnings in `slate test`,
  `--update-refs` lists files, `slate explain` lists topics.
- Lesson: the frame report already knew about the hidden goal tile, but only `shot --report` showed it.
  Information the engine has should reach the agent where it already looks (`slate test`).

## 2026-11-25 — M1 thin working version: native and browser

- The first-milestone chain works end to end: `new → (write scene) → check → run 300 frames with input →
  shot → inspect`, plus `test`, `explain`, `schema`, `docs`, `serve`.
- **Native and browser agree bit for bit**: `examples/iso_garden` walkthrough at frame 300 → state
  `469ae7c92124d43d`, image `bfca8c0b40af6828` in both.
- Browser data reload verified: a typo in a prefab showed `E0103 … did you mean step_frames?` in the page
  within a second; fixing it reloaded the scene. No build step involved.
- Budgets measured (M5 Pro): wasm 467 KB, release CLI 1.5 MB, 16 external runtime crates, 0 proc macros,
  6-command smoke chain 406 ms, slate_sim→cli recompile 0.38 s.
- Finding: cargo fingerprints file contents, so "touch a file" does not measure a rebuild. The budget
  probe now uses an env var read with `option_env!`.
- Finding: the frame report immediately surfaced a real (correct) occlusion — the player standing behind
  the 1.2-high gate is 56% visible. Good sign for the report's usefulness.
- Not done: the cold-agent test (needs a fresh agent); CI has not run on GitHub (no remote yet).

## 2026-11-24 — M0 foundation

- Read the direction doc; chose the codename "slate" (provisional, Q1).
- Pinned Rust 1.100.0 + wasm32; workspace with core/data/sim/render/runtime/cli/web layers.
- Determinism guardrails in clippy.toml; diagnostics with stable codes; schema registry.
