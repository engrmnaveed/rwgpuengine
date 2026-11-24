# Progress log

Newest first. Commit messages carry the detail; this page is the narrative and the place to record
findings that are not code (experiment results, surprises, things to remember).

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
