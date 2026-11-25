# Progress log

Newest first. Commit messages carry the detail; this page is the narrative and the place to record
findings that are not code (experiment results, surprises, things to remember).

## 2026-11-25 — Builder kit v2: `refused` and `counts` (M3.2)

- Built the two gaps that Tiny Town and run #7 both showed
  ([ADR 0014](decisions/0014-refusal-signal-and-count-props.md)):
  - **`refused`**: a placement that fails makes the signal's source emit `refused` (the ground, for ground
    taps). `when = [{ reason = "terrain" }]` picks one reason (`edge`, `terrain`, `occupied`).
  - **`counts`**: `counts = { houses = "house" }` is an integer prop the engine keeps equal to the number of
    enabled copies. It updates in the step that changes it, logs `counted` and emits `changed`.
- Chose a count *prop* over the requested count *condition* on KPIs. A condition alone has nothing to
  re-check on after a removal, and file order would decide what it sees. It also cannot be shown in a label.
  A prop fixes both and adds no new condition kind.
- A `when` failure ("not enough coins") is still `skipped`, not `refused`. In a toolbar game every other
  tool's spawn is skipped on each tap, so a signal there would fire constantly.
- Tiny Town:
  - the job card is `counts` (6 counting connections → 0);
  - a hint line says "Build on grass!", "No room there!" or "A house needs 3 coins";
  - 23 expects in `refusals` (up from 14);
  - two mutation checks fail exactly the expectations that cover them.
- Old examples keep their exact hashes. Tiny Town native = web on all three playtests. Wasm 657,498 bytes
  (+15.7 KB). `get_mut` instead of `BTreeMap::insert` saved 3.2 KB of map code.

## 2026-11-25 — Cold-agent test #7: a farm builder, accepted

- A new builder task (pond and path terrain, a 3 × 2 barn, crops, fences, seeds on screen, remove with refund,
  a ready message, six required proofs) on builder kit v1. Every requirement supported, nothing approximated
  except the extra feedback the engine cannot give. 6 playtests with 59 expects, 0 warnings. 8 min 44 s,
  28 tool calls, ~156K tokens. Re-verified independently. ADR 0013's acceptance is met.
- Its most important finding was a **false pass**: `slate test out/x.toml` ran the project's tests instead of
  that file. Now it runs exactly that file, and a scene or prefab path is an error; the smoke chain guards it.
- Also fixed:
  - event expectations filter by `prefab` (Tiny Town's refusals test uses it);
  - `[[terrain]]` and `when` condition tables are in AGENTS.md;
  - the recipe explains why tapping a building never yields `occupied`;
  - removal wording is consistent;
  - `inspect` labels `screen_pos`.
- Next, with evidence from two games: a refusal signal and conditions that count copies.
  [Experiment report](experiments/2026-11-25-cold-agent-7.md).

## 2026-11-25 — Q3 decided: the isometric builder; builder kit v1 and Tiny Town (M3.1)

- The owner picked the isometric builder as reference game 1. Tiny Town ([spec](games/tiny-town.md)): tap a
  tool, tap the island, build 2 houses, 3 trees and a well within 12 coins, with Clear to take things back.
  Built entirely in data; 3 playtests (`build_town`, `refusals`, `clear`). Two mutation checks (an
  affordability rule, a refund value) fail exactly the playtests that cover them.
- Builder kit v1 ([ADR 0013](decisions/0013-first-reference-game-isometric-builder.md)):
  - terrain maps (`[ground] map` + `[[terrain]]` with `build`);
  - ground taps (`from = "ground"`);
  - `spawn` with placement rules, refusals logged as `blocked` (`edge`/`terrain`/`occupied`);
  - multi-cell `footprint`, drawn as per-cell slices;
  - `from_prefab` and `to = "self"`, `block.top`;
  - `[[expect]] prefab` + `count`;
  - `slate inspect --place house[@x,y]`, with a guaranteed tap pixel per cell.
- Found while building:
  - **Data-only entities blocked building at [0, 0].** A props-only `town` entity sits at the default
    position and counted as occupying it. Now only entities that are drawn, solid, pushable, actors or
    triggers take up cells (a sim test caught it).
  - **Hidden cells can't be tapped.** A cell hidden behind a house has no tap pixel, correctly: a player
    can't tap it either. `--place` says so instead of handing out a pixel that would hit the house.
  - **Agents needed tap pixels for ground cells.** They had them for entities only; `--place` now gives them.
- Determinism: `build_town` reaches state `537ae6f9300e9b67` and image `58bd8a0dbea14409` natively and in
  the browser. Older scenes keep their hashes and images. Live tapping in the web player builds as expected.
- wasm: 586,226 → 640,798 bytes (+9.3%, budget 700,000). Replacing a `BTreeMap<String, EntityDef>` with a
  `Vec` saved 15 KB of generic code. `opt-level = "z"` measures 600,909 but is not adopted until frame time is
  measured.
- The scene has 26 connections, 9 of them for clear/refund. That is the evidence to watch for grouped
  connections or per-prefab costs.

## 2026-11-25 — Cold-agent test #6: the vault task with no stand-ins

- On the pushable build, the agent built the full puzzle: three coins counted on a HUD, two crates pushed onto
  the plates, and the player walks into the vault. 0 warnings, 5 playtests with 49 expects plus its own
  mutation check. Re-verified independently.
- 7 min 41 s, 29 tool calls, ~136K tokens. That is the fastest of the four vault runs (18:42 → 14:06 →
  10:02 → 7:41), and the first file was written at 2:39.
- Its table kept one conservative "approximated" (no second character to hold a plate), prompted by our
  limits wording. That row is reworded.
- Fixes from its findings:
  - the starter `crate` prefab is now pushable, and a new `wall` prefab does the blocking;
  - crate-off-plate behaviour is documented;
  - the report says when a plate's coverer stands on it;
  - `slate test` prints "final state";
  - back-to-back holds are clarified.
- [Experiment report](experiments/2026-11-25-cold-agent-6.md).

## 2026-11-25 — Pushable crates hold plates (ADR 0012)

- `pushable = true`: a mover walking into it pushes it one cell if the cell beyond is free. Otherwise the
  mover is refused with the new reason `push`, which says what is in the way. Patrols and wanderers can't
  push. Triggers and `occupied` count pushables, so a crate holds a plate.
- coin_vault's statue workaround is now a crate the player pushes onto plate_a. The solve test asserts the
  crate's position and a refused second push at the ground's edge. Making the crate an immovable solid on a
  copy fails exactly the two playtests that should fail.
- Found while testing: a mover that pushes a crate off a plate steps onto that plate in the same step, so the
  plate never empties and never emits `released`. That is correct, and a test covers it.
- Hashes of scenes without pushables are unchanged (iso_garden `469ae7c92124d43d`).

## 2026-11-25 — Cold-agent test #5: logic supported, two targets missed

- Run #3's task on the data-logic build. AND door, re-closing and the on-screen counter were all supported
  with no approximation (`when`, `add`, `label`, `screen_pos`). The first scene checked clean, 0 warnings
  throughout, 3 playtests with 42 expects. Re-verified independently.
- Time: 10 min 2 s (run #4 14:06, run #3 18:42); 33 tool calls, ~149K tokens. The scene is a third the size
  of run #4's.
- ADR 0011's acceptance targets: "≤ 5 min" was missed. "No approximations" was miscalibrated: the task needs
  two plate holders, and objects can't hold plates (a separate limit ADR 0011 didn't cover). Every verdict was
  reachable at 0:26; most of the remaining time was designing around the second holder.
- Fixes from its findings:
  - the AND recipe says who can hold the plates;
  - the limits table carries the wording tasks use;
  - `action` events carry `changed` and the docs admit no-ops;
  - W0304's title is more precise;
  - reports print `screen` for HUD entities;
  - an entity's self-overlap no longer counts as hidden.
- Evidence for the next feature: runs #3–#5 all built a patrol stand-in to hold a plate.
  [Experiment report](experiments/2026-11-25-cold-agent-5.md).

## 2026-11-25 — Data logic v1 built (M1.6)

- Built ADR 0011's set: `[[connect]]` `add`/`set` on props (the owner emits `changed`), `when` conditions
  (`occupied`, `enabled`, prop `equals`/`at_least`/`at_most`; all must hold), a `label` component drawn
  with an owned 5×8 pixel font with `{prop}` placeholders, and `screen_pos` for HUD entities. New:
  `skipped` events, `[[expect]] text`, codes E0206 unknown-prop, E0207 prop-type, W0207 unsupported-char,
  W0305 label-cut.
- `examples/coin_vault` is the reference example: three coins counted on a HUD, a vault that opens at 3
  (any order), an AND lock (statue on one plate, player on the other), a status banner set by `set`. 4
  playtests, two of them negative; a mutation check (door at 2 coins, AND removed) fails exactly the
  playtests that should fail.
- Determinism: native and web agree on coin_vault (state `41f58b0904b83261`, image `f15c777308fdf361`);
  iso_garden's hashes and reference image are unchanged.
- Budgets: wasm 504,174 → 580,108 bytes (+15%, limit 700,000; mostly the static checks, which run in the
  browser too), CLI 1.63 → 1.71 MB, still 16 runtime crates and 0 proc macros, smoke 418 ms, rebuild 458 ms.
- Found while building: screen entities were ordered by part before entity, so every HUD label covered
  every later HUD sprite; fixed (file order). The recipe TOML snippets are now tested.
- Not done: the acceptance run (cold agent #5 on run #3's task: ≤ 5 min, nothing approximated, 0 warnings).

## 2026-11-25 — Q14 decided on KPIs: data logic first, Rust as the escape hatch

- Owner's rule: choose on KPIs only (performance and flexibility trade-off). Measured edit loops: data edit →
  `check` + `test` 0.01 s warm with no build; Rust edit → 0.39–0.49 s dev rebuild, 1.4 s release, 0.9–1.3 s
  browser module (and per-game web builds don't exist yet).
- Rust is more flexible and faster per operation, but loses every agent-loop KPI (static checks,
  queryability, guaranteed determinism, web delivery, authoring time). Its per-operation speed doesn't matter
  for event-driven logic at this scale. So: a small data-level logic set for the common cases, Rust for the
  rest. [ADR 0011](decisions/0011-game-logic-data-first-rust-escape-hatch.md) has the KPI table, the spec and
  the acceptance targets (run #3's task in ≤ 5 min with no approximations).

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
