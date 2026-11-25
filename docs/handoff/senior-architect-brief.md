# Brief for the senior architect session

Written 2026-11-25 by the session that built slate from the first commit to tag `baseline-2026-11-25`
(Claude Opus). It is for the agent the owner starts next, intended to be **Fable 5.2 in Extra mode**, in a new
session inside this repository. That agent becomes the project's **main supervisor**. The owner directs, and
you plan, decide technical questions on KPIs, build, and supervise any sub-sessions.

## 1. Your mandate (from the owner)

1. **Audit** slate against the vision: is it going where the owner wants? Say plainly where it is not.
2. **Consolidate the core and lay the complete platform foundation**, so everything after it stands on
   solid ground.
3. Only then, with the owner: real **demos** with written build logs, and the **desktop app** (an outline is
   in the [proposal](../proposals/desktop-app.md)).

Core feature work is **paused** at the baseline. The feature candidates in the roadmap are your backlog to
judge, not a queue to work through.

## 2. Standing rules from the owner

- **Choose only on KPIs.** "Which option is more performant and flexible, opt that. Best trade-off. As it's
  an agentic-native engine, no choice is based on anything aside better KPI." Write the comparison into the
  ADR: the options, the measurements, and why the winner wins.
- **Decisions that belong to the owner** (product direction, naming, licence, hosting, target devices,
  anything in `docs/open-questions.md`) are proposed with a recommendation, never made silently.
- **One step at a time, approved.** End each step with what changed, what was measured, what is not done,
  and one proposed next step. The owner answers with a short "Yes, …".
- **Privacy:** the owner's name and email never go to external services.

## 3. Read first, in this order

1. [vision.md](../vision.md), then the direction PDF at the repo root, which wins if the two disagree.
2. [AGENTS.md](../../AGENTS.md): the rules for changing the engine (determinism, no proc macros, layering,
   one format registry, stable codes, commands as data).
3. [baselines/2026-11-25.md](../baselines/2026-11-25.md): what exists and every measured number.
4. [architecture.md](../architecture.md), [roadmap.md](../roadmap.md), [open-questions.md](../open-questions.md),
   and the ADRs in [decisions/](../decisions/) (0001–0017).
5. [progress-log.md](../progress-log.md) and the 9 cold-agent reports in [experiments/](../experiments/):
   these hold the evidence behind every feature.

## 4. Where I think slate stands against the vision

This is my own honest read, written for you to check; it is not a substitute for your audit.

**On track:**
- **The agent loop works.** Nine cold agents built every task with only the generated docs and passed every
  run. Times fell from 18:42 (run 3) to 5:54 (run 9) as the docs and features answered what they hit.
- **Determinism holds.** State and image hashes match natively and in the browser, and across web
  optimisation levels.
- **Budgets are real numbers checked in CI.**
- **Errors are data.** Diagnostics have stable codes, locations and hints.

**Where it may be drifting, for you to judge:**
1. **The data logic is growing into a small language:** props, `when`, `actions`, `counts`, `refused`,
   `cost`. Each step was KPI-driven and evidence-backed. But the vision's bet is "**Rust for game code**", and
   that path (M4, Q7) has not started. Decide where data logic should stop and Rust start before more
   features land. Run 8 and 9's requests (a "not equal" condition, derived props, else-branches) are exactly
   that boundary.
2. **"Reference games drive every feature" has been stretched.** Tiny Town is the only spec'd reference game.
   Runs 7–9 (farm, park) drove features through cold-agent tasks. Decide whether cold-agent tasks count as
   evidence, or whether the second and third reference games (puzzle, tracing) must come first.
3. **The rented platform layer is empty**: no wgpu, winit, audio or text shaping. That was deliberate (Q4,
   Q5, Q13), but "complete platform foundation" probably means deciding at least the native window/player
   path and audio. The desktop app may need it too.
4. **Tablets are unmeasured** (Q8). The product targets tablets, and every frame-time claim so far is from a
   desktop browser.
5. **Agent context cost.** The generated AGENTS.md is 67 KB, about 17K tokens, read by every new agent. The
   vision argues hard about context cost (MCP tool definitions), so the docs should get the same scrutiny:
   progressive disclosure through `slate explain`, a shorter core, measured with a cold-agent run.
6. **Code concentration.** `slate_data/load.rs` (1,714 lines), `schema.rs` (1,486) and `slate_sim/world.rs`
   (1,418) carry most of the logic. `load.rs` mixes parsing, reference resolution and many checks in one
   `load_scene`. It works and is tested, but it is the first place I would consolidate: a checks pipeline,
   per-concern modules, shared diagnostic helpers.
7. **Never run outside one machine.** CI has only run locally on macOS arm64. Linux, Windows and x86 have
   never been checked, though the determinism claim says "on every platform".
8. **Open-source readiness is blocked by the owner's questions**: name (Q1), licence (Q2), hosting (Q10).
9. **There is no stable machine interface for a long-running tool** (a session server or protocol). The
   desktop app will need one; the CLI is per-invocation.

## 5. Suggested shape of your first phase (yours to change)

1. **Audit report** (`docs/audits/…`): vision against reality, with evidence, the gaps ranked by KPI impact,
   and a proposed consolidation and foundation plan as a sequence of approved steps.
2. **Owner decisions** you need, asked together and early: Q1, Q2, Q8, Q10, the data-logic boundary,
   reference games 2 and 3.
3. **Consolidation** (behaviour-preserving, proven by the same state hashes): loader and checks structure,
   world structure, shared helpers. A cold-agent re-run should show no regression.
4. **Foundation**:
   - CI on GitHub with Linux, Windows, macOS and wasm, with a cross-platform determinism check;
   - the Rust game-code path (M4);
   - the agent-surface protocol that the desktop app and MCP (Q9) will share;
   - a tablet measurement.
5. Then **demos** and the **desktop app**, as the owner sequenced them.

## 6. How the work has been done (keep it, or improve it with a reason)

- **The gate:** `cargo xtask ci` before every commit (fmt, clippy with `-D warnings` for native and wasm,
  tests, examples, docs check, smoke, budgets). Regenerate the reference with `cargo xtask docs` after changing
  commands, formats or codes.
- **Commits:** conventional, with a detailed body that gives measurements, what was verified, and what is not
  done. The git log is the progress log. Add a dated entry to `docs/progress-log.md` for milestones.
- **An ADR for every decision,** with the options compared on KPIs. Do not decide owner questions.
- **Behaviour-preserving changes** are proven by unchanged state hashes (section 5 of the baseline) and
  pixel-identical reference images.
- **Native = web** is checked by serving an example (`slate serve` via `.claude/launch.json`) and reading the
  state hash from the player URL `?input=tests/x.toml&frames=N`, which runs to that frame and pauses. Compare
  with `slate test`'s "final state".
- **Mutation checks:** after adding a playtest, break the rule it guards in a copy (scratchpad) and confirm the
  test fails.
- **Wasm size analysis:** build with `CARGO_PROFILE_WEB_STRIP=false` into a separate target dir and diff
  per-function code sizes against a worktree of the previous commit. Two small Python wasm-section parsers did
  this; they were scratch files, so rewrite them, or add an `xtask` task if it keeps being useful.
- **Cold-agent test protocol** (the most important signal):
  - a fresh general-purpose agent, an empty folder **outside** the repo, and the release binary copied next
    to it;
  - it is told to read only `slate new`'s files and slate's output, and to report in a fixed format: a
    requirement / verdict / how / evidence table, final check and test lines, diagnostics hit, confusions,
    tool calls, wanted features;
  - then re-verify independently: re-run check and test, view its images, read its wiring for faking, and
    mutation-test its playtests;
  - write it up in `docs/experiments/` and archive its files;
  - fix every confusion as a docs or diagnostics bug.

  The verbatim task texts are in each experiment report. Repeating a task word for word (runs 3–6 and 8–9)
  isolates the effect of one change.

## 7. Open threads you inherit

- **Feature candidates** (roadmap M3):
  - a "not equal" / `in` condition (runs 8 and 9);
  - derived boolean props;
  - a tap on a building with a build tool emits `refused` (`occupied`);
  - `when` on the refused terrain's name;
  - a placement anchor;
  - drag input;
  - terrain that blocks walking;
  - rotation;
  - runtime style changes.
- **No cold-agent run has used `cost` yet** (ADR 0016).
- **Demos**: the owner wants real games built with slate, each with the procedural steps of how it was built.
  The cold-agent protocol already produces build reports; a demo could be a cold-agent build on the
  consolidated platform with a curated `BUILD.md`.
- **The desktop app**: see the [proposal](../proposals/desktop-app.md). Start it only after the foundation.
