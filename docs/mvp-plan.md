# MVP plan

The MVP is the direction doc's **first milestone: a thin working version** — one chain of commands
working end to end, natively and in the browser — plus the foundation it needs to grow without rework.

Status legend: ✅ done and verified · 🟡 done, not fully verified · ⬜ not started

## M0 — Foundation ✅

| item | status | evidence |
|---|---|---|
| Git repo, pinned toolchain (Rust 1.100.0 + wasm32 target) | ✅ | `rust-toolchain.toml` |
| Workspace with layered crates (core → data → sim/render → runtime → cli/web) | ✅ | [architecture.md](architecture.md) |
| Determinism guardrails in the linter (no libm transcendentals, no HashMap, no wall-clock) | ✅ | `clippy.toml` |
| Stable diagnostic codes with explanations | ✅ | `slate_core::codes`, [reference/errors.md](reference/errors.md) |
| Schema registry driving validation, defaults, JSON Schema and docs | ✅ | `slate_data::schema` |
| One-command CI (`cargo xtask ci`) + GitHub Actions | 🟡 | passes locally; the workflow has not run on GitHub (no remote yet) |
| Hard budgets that fail the build | ✅ | `budgets.toml`, `cargo xtask budgets` |
| Docs: vision, architecture, ADRs, plan, roadmap, conventions, open questions, progress log | ✅ | `docs/` |

## M1 — Thin working version (the first milestone) ✅

Acceptance criteria, straight from the direction doc:

| # | step | status | how it was verified |
|---|---|---|---|
| 1 | `new` creates a project | ✅ | `slate new demo` → 10 files incl. a version-matched AGENTS.md; self-checks clean |
| 2 | Write a small isometric scene as a text file | ✅ | `cargo xtask smoke` writes `scenes/smoke.toml` from scratch |
| 3 | `check` validates it | ✅ | `slate check` — 0 errors; broken scenes give rustc-style E01xx/E02xx with fix hints |
| 4 | Run it headless for 300 frames with scripted input | ✅ | `slate run --frames 300 --input tests/…` → state hash, event counts |
| 5 | `shot` saves a PNG | ✅ | `slate shot` → PNG + frame report; reference image reviewed by eye |
| 6 | `inspect` dumps the world state as JSON | ✅ | `slate inspect --json` (+ `--entity`, `--component`, `--events`) |
| — | …natively | ✅ | aarch64-apple-darwin; `cargo xtask smoke` asserts each step's JSON |
| — | …and in the browser | ✅ | `slate serve` + web player: walkthrough frame 300 → state `469ae7c92124d43d`, image `bfca8c0b40af6828`, **identical** to native |
| — | **The test of the whole idea:** a coding agent starting cold, with only the AGENTS.md, builds and verifies that scene without help | ✅ | [cold-agent test #1](experiments/2026-11-25-cold-agent-1.md): PASS in 3 min 58 s, original level, independently re-verified; its 10 confusion points were fixed |

Beyond the letter of the milestone, M1 also shipped: `test` (playtests with expectations and reference
images), `explain`, `schema`, `docs`, `help --json`, `serve` with instant data reload, touch input
(tap/swipe/pad), and the `Game` trait for Rust systems.

### The cold-agent test

Run it as an experiment and keep the record in [experiments/](experiments/) (it is the most important
signal we have). Run #1: [plate → door → goal](experiments/2026-11-25-cold-agent-1.md) — PASS in 3 min 58 s.
Run #2: [tap lever + patrol timing + negative tests](experiments/2026-11-25-cold-agent-2.md) — PASS in
6 min 7 s.

1. Build `slate` and put it on `PATH` (`cargo build --release` → `target/release/slate`).
2. In an empty folder **outside this repo**, start a fresh coding agent with no other context.
3. Prompt (verbatim): *"Run `slate new garden` and read garden/AGENTS.md. Then build a small isometric
   scene of your own in that project — a player who must step on a pressure plate to open a door and reach
   a goal — and prove it works with a playtest and a reference screenshot. Use only the slate CLI."*
4. Pass if, without human help: `slate check` is clean, `slate test` passes with at least one `[[expect]]`
   on the end state and one `[[shot]]`, and the agent looked at the PNG.
5. Record: wall time, number of commands, every diagnostic hit, every place the agent was confused or read
   something other than AGENTS.md / `slate help` / `slate explain`. Each confusion is a docs or
   diagnostics bug: file it in [progress-log.md](progress-log.md) and fix it.

## Known limitations

Deliberate gaps in v0.1 — each waits for a reference game that needs it (guardrail 2):

- **No text rendering**, **no image sprites**, **no audio** ([open-questions](open-questions.md) Q5, Q6).
- **Single-cell objects only**: draw order is per cell (`x + y`); multi-cell buildings need M3 sorting.
- **CPU rendering only** (deterministic, fine at 640×480; GPU via wgpu later — Q4).
- **No native window**: humans use the web player (`slate serve`).
- **Prefab components can't omit required keys**: a prefab with `patrol = { step_frames = 30 }` fails
  because `patrol.path` is required; put the patrol in the scene. Fix: validate required keys after merge.
- **Adjacent same-colored blocks merge visually** (no outlines yet; the cold agent noticed it too).
- **Taps on bare ground hit nothing** (no "tap a cell" event yet; needed for tap-to-move / builders).
- **`slate serve` is single-threaded and dev-only**; there is no `export web` for static hosting yet (Q11).
- **Game code in the browser**: the web player runs data-only projects; Rust `Game` code runs natively.

## Next

Cold-agent run #3 on a task the engine cannot do yet (to test how fast limits are recognised), then M2 in
[roadmap.md](roadmap.md).
