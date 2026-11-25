# Architecture audit — 2026-11-25

Written by the senior architect session (Claude Fable 5.2) that takes over as main supervisor, on commit
`a028bea` (tag `baseline-2026-11-25`). It answers the first item of the
[handoff brief](../handoff/senior-architect-brief.md): is slate going where the owner wants, and where is it
not? Every number below was measured in this session on an Apple M5 Pro unless it says otherwise. The scripts
are in [2026-11-25/tools/](2026-11-25/tools/), so each number can be measured again.

Nothing in the engine was changed. This report, its tools and a progress-log entry are the only new files.

## 1. Verdict

**The base is sound and better than its own documents claim. The direction has drifted: the work so far has
perfected the part of the vision that slate shares with its prior art, and has not started any of the three
things the vision says set it apart.**

What is sound:

- The gate is green and every baseline KPI reproduces (section 2).
- Determinism is stronger than claimed. The baseline had checked two targets. This audit ran all nine
  example playtests on three more, and on wasm without a browser: every state hash is equal (section 3.1).
- 2,000 runs on 400 deliberately broken projects produced no panic and no malformed JSON (section 3.4).
- The agent loop works: nine cold-agent runs passed, and none faked a result.

Where it has drifted, in the vision's own words ("Stand apart on Rust for the game code, web and tablets
first, and the learning-game kits"):

| the vision's differentiator | state at the baseline |
|---|---|
| Rust for game code | The `Game` trait exists. No game, example or test implements it; the only implementations are the two empty defaults. Zero uses. |
| Web and tablets first | Never measured on a tablet. A game cannot be exported or hosted: `slate serve` is a dev server, and it finds its wasm through a path into this repository's build folder. |
| Learning-game kits | Tracing and quiz/lessons are not started. The one kit that exists (builder) lives inside the core simulation, not in a kit layer. |

The second-order drift is in guardrail 2 ("reference games drive every feature"). One reference game exists.
Six feature ADRs (0011–0016) landed in one day. Four of them (0011, 0012, 0015, 0016) were pulled by
cold-agent tasks alone and not by a spec'd game, and the last four all serve one genre: tap to build with
coins. The result is a core that knows about prices
(`cost`, the `funds` refusal) but not about sound, images, more than one scene, or saving progress.

So the answer to "is it going where the owner wants?" is: the engineering is, the product is not yet. A real
demo built today would be one scene of coloured shapes with a 5 × 8 pixel ASCII font, no sound, no levels and
no way to put it on a tablet. Section 5 ranks the gaps, section 7 lists the decisions that are the owner's,
and section 8 proposes the order to close the gaps.

## 2. The baseline reproduces

`cargo xtask ci` passes on this machine (exit 0): fmt, clippy for native and wasm, 128 unit tests, docs check,
9 example playtests, smoke chain, budgets.

| KPI | baseline | this audit | budget |
|---|---|---|---|
| web wasm | 643,317 bytes | 643,317 | 800,000 |
| CLI binary | 1,911,888 bytes | 1,911,888 | 3,000,000 |
| render, Tiny Town 640 × 480, native | 2,253 µs | 2,343 µs | 6,000 |
| smoke chain | 324–416 ms | 417 ms | 4,000 |
| incremental rebuild | 436 ms | 436 ms | 10,000 |
| runtime crates / proc macros | 16 / 0 | 16 / 0 | 20 / 0 |
| state hashes, 6 fingerprints in the baseline | — | all equal | — |

## 3. New measurements

### 3.1 Determinism holds on five targets, not two

The claim is "same state hash and same pixels on every platform". The baseline had checked macOS arm64 and
wasm in Chromium. I ran all 9 example playtests on three more native targets, and on wasm under Node, with no
network and nothing installed:

| target | how | result |
|---|---|---|
| aarch64-apple-darwin | native | reference |
| x86_64-apple-darwin | release build, run under Rosetta 2 | 9 of 9 state hashes equal; reference images pass |
| x86_64-unknown-linux-musl | cross-built, run in an emulated container | 9 of 9 equal; reference images pass |
| aarch64-unknown-linux-musl | built in a `rust:1.100.0` container | 9 of 9 equal; **all 128 unit tests pass** |
| wasm32 (Node 26, no browser) | `tools/wasm_check.mjs` | 9 of 9 state **and** 9 of 9 image hashes equal to native |
| wasm32 (Chromium) | the baseline's check | equal (not repeated; the same V8 engine as Node) |

Two things follow:

1. The contract is real. Nothing needed fixing.
2. The native-equals-web check does not need a browser or a person. A 60-line Node script does it, so it can
   be a CI gate. Today it is a manual step that the gate does not run.

Not checked: Windows, real x86 hardware (both x86 runs were emulated on Apple silicon), Safari/WebKit (the
engine every iPad browser uses) and Firefox.

### 3.2 The player spends 97 % of each frame on work it can skip

`tools/raster_spans_bench.rs` splits one frame of Tiny Town (`build_town`, frame 120) into its parts, and
tests a prototype rasterizer that fills one span per scanline using the same exact pixel-centre rule.

| frame size | today: rasterize + report | prototype: spans, no report | ratio | wasm today (Node) |
|---|---|---|---|---|
| 640 × 480 | 1,145 + 1,085 = 2,230 µs | 59 µs | 38× | 2.8 ms |
| 1280 × 960 | 4,287 + 4,317 = 8,604 µs | 173 µs | 50× | 10.9 ms |
| 1920 × 1440 | 9,498 + 9,694 = 19,192 µs | 362 µs | 53× | 24.2 ms |
| 2560 × 1920 | 18,146 + 17,240 = 35,386 µs | 627 µs | 56× | 42.2 ms |

- **Pixel identity:** the prototype's image hash equals the reference rasterizer's on every frame of every
  playtest, 1,932 frames out of 1,932 (all nine playtests at 640 × 480, plus `build_town` at 2×, 3× and 4×).
- **Where the time goes today:**
  - The reference rasterizer tests every pixel of each shape's bounding box against four edge equations, and
    updates a `BTreeMap` for each overdrawn pixel.
  - The web player calls `Session::render`, which also builds the frame report and throws it away. That is
    half of every frame.
  - The player redraws every frame. Only **131 of 1,560 frames (8.4 %)** change any pixel.
- **The simulation is not the cost:** one step takes 0.3–2.3 µs. Replaying 100,000 frames takes about 0.2 s.

What this changes:

- **Q4 (GPU).** ADR 0017 estimates a mid-range tablet at 4–6 times slower than this machine. On that
  estimate, today's player needs 11–17 ms for a 640 × 480 frame, which is at the edge of 60 fps, and 170–250 ms
  at 2560 × 1920. With the prototype's path the same tablet would need about 0.3–0.4 ms and 3–4.5 ms
  (the native times above, times 1.2 for wasm as measured today, times 4–6). A CPU
  renderer that is fast enough at full tablet resolution removes the reason to add wgpu for flat shapes, and
  keeps the 643 KB zero-import wasm. This is an estimate until a tablet is measured (Q8).
- **The reference rasterizer stays** as it is for `shot --report`, warnings and tests. The fast path is an
  addition that a test holds pixel-identical.

The prototype is 150 lines in a scratch crate. It is evidence, not a proposal to merge as it stands.

> **Added later the same day.** The paint path was built ([ADR 0018](../decisions/0018-paint-path-for-players.md)).
> Two things above no longer stand:
> - The tablet estimate used native time × 1.2 for wasm. Measured, wasm paint at 2560 × 1920 is 0.91 ms on this
>   machine, so the estimate is 4–6 ms on the tablet, not 3–4.5 ms.
> - The owner decided that the target is 3D on the GPU (D8, below). The argument here that wgpu can wait is
>   withdrawn; it only ever applied to flat 2D shapes.

### 3.3 Everything on screen is tied to one frame size

The display is one fixed size (640 × 480 in every example). HUD positions, sprite sizes, label scale and
playtest taps are all written in those pixels. `tools/scale_town.py` multiplies them by 2, 3 and 4: all three
playtests still pass at every size, so the **logic** does not depend on resolution. The state hash does
change, because sprite sizes and label scale are part of the hashed state.

Tablets differ in size, aspect ratio and pixel density. The engine has no answer yet: no scale factor, no
letterbox rule, no logical units. This needs a decision before demos (section 6, D6).

### 3.4 Robustness: no panics, one hang

`tools/fuzz.py` broke the three example projects 400 times (deleted, duplicated and swapped lines, extreme
values, stray tables) and ran `check`, `test`, `run`, `shot --report` and `inspect --events` with `--json` on
each.

| outcome | runs |
|---|---|
| exit 0 or 1 with valid JSON | 1,993 |
| panic | 0 |
| output that is not JSON | 0 |
| no result after 20 s | 7 |

All seven hangs have one cause: `footprint = [4096, 4096]`. The schema allows it, `slate check` reports only
warnings, and `shot` then runs for more than 90 s (16.7 million cells per entity). An agent that writes one
wrong number gets a command that never returns and no diagnostic. "Errors are data" has this one hole: there
are no limits on work.

### 3.5 Agent context: 61 % of AGENTS.md is already available on demand

| section of the generated AGENTS.md | bytes | share |
|---|---|---|
| File formats | 41,184 | 61.2 % |
| Recipes | 12,955 | 19.2 % |
| Limits in this version | 4,989 | 7.4 % |
| everything else (work loop, layout, example, tips, commands, codes) | 8,185 | 12.2 % |
| **total** | **67,313** | about 17K tokens |

The file-format tables are the same text `slate explain <topic>` prints. The vision's own argument about MCP
(do not load every definition upfront) applies to this file. A core of 15–25 KB with formats fetched on
demand is plausible, but whether it is *better* is a question for a measurement, not an opinion: fewer
tokens read upfront against more tool calls.

### 3.6 Other facts found

- **Released binaries would carry the build machine's home path.** The release CLI contains 48 strings with
  the absolute home directory and the wasm contains 21 (cargo registry paths, and `serve`'s default wasm
  path). They must be remapped before any binary is published.
- **`slate serve` depends on this repository.** The wasm is not embedded (Q11). The binary looks next to
  itself, then follows an absolute path into this repository's `target/` folder that was baked in at build
  time. On any other machine a new project cannot serve itself.
- **The state hash is maintained by hand.** `World::state_hash` lists fields one by one, with special cases
  so older scenes keep their hashes. An entity's `prefab` is not hashed, although `counts` and `from_prefab`
  depend on it. A new field that is forgotten there would not fail any test.
- **Hand-written numbers in docs go stale; generated ones do not.** `architecture.md` says "Verified" beside
  hashes that coin_vault and Tiny Town no longer have (`41f58b09…`, `537ae6f9…`).
- **The JSON output is a public contract with no snapshot test.** The smoke chain checks a few fields. A
  desktop app and an MCP adapter would both build on this contract.
- **The CI workflow has never run**, has no Windows job, and does not run the wasm.

## 4. The vision, principle by principle

| principle (direction PDF) | state | note |
|---|---|---|
| Text files are the source of truth | mostly | Scenes, prefabs and playtests, yes. "UI and lesson content" have no format yet. |
| Command line first, JSON everywhere | done | 12 commands. |
| Deterministic core without a screen | done | Five targets. "Record/replay": replay yes, **record no**. |
| A world the agent can query | done | |
| Built-in visual checks | done | |
| Docs tied to the installed version | mostly | Generated AGENTS.md, yes. Machine-readable migration notes: none (no format has changed yet). |
| The edit-and-verify loop | half | Data edits reload instantly. **Hot-patching code edits (subsecond): not started.** |
| Hard limits enforced in CI | half | Size and rebuild time, yes. **Tablet startup: no device, limit 0.** CI has never run remotely. |
| Familiar terms | done | |
| Guardrail 1: own the core, rent the commodity | open | Nothing is rented but TOML and PNG. Section 3.2 argues the GPU can wait; audio and real text cannot wait as long. |
| Guardrail 2: reference games drive features | stretched | One of three games. See section 1. |
| Guardrail 3: stand apart from prior art | not yet | None of the three differentiators is started. |

## 5. Gaps, ranked by what they block

**A. They block a real demo** (the owner's next phase):

1. **No way to ship a game.** No `export web`, no embedded player, no install on a tablet.
2. **A game is one scene.** No scene change, no levels, no saved progress. Neither is on the roadmap.
3. **No sound, no images, ASCII pixel text.** A learning game for children needs all three.
4. **The frame is fixed at one pixel size** (3.3), and the player wastes 97 % of its frame time (3.2).
5. **Tablets are unmeasured** (Q8).

**B. They block the differentiators:**

6. **Rust game code has no path and no user.** No `slate new --rust`, no web build of game code, and the
   `Game` trait hands Rust code a `&mut World` of public fields: nothing such code does is checked by
   `slate check` or explained in the event log unless its author writes that by hand, and those two are
   what make data logic good for agents.
7. **There is no kit layer.** Builder rules (terrain build flags, placement, `spawn`, `cost`, `counts`) sit
   in `slate_sim::world` and `slate_data::load`. A tracing kit and a quiz kit added the same way would double
   both files.
8. **No pointer drag** (ADR 0010 left it for tracing).

**C. They block open-sourcing:**

9. **Name, licence, hosting** (Q1, Q2, Q10) are the owner's and are open.
10. **Identity.** All 23 commits carry the owner's name and a personal email address, and the PDF at the root
    carries a handle. The owner's standing rule says these never go to external services, so a public push
    needs an explicit decision (D3).
11. **Binaries leak the home path** (3.6). **No LICENSE, CONTRIBUTING, SECURITY or CHANGELOG file.**
12. **CI has never run remotely; Windows is unchecked; the wasm check is manual.**

**D. They slow everything after:**

13. **Code concentration.** `load_scene` is one 370-line function that parses, resolves and checks.
    `world.rs` holds movement, triggers, signals, actions, placement, payment, counting and hashing.
14. **No limits on work** (the hang in 3.4), and a hand-kept state hash.
15. **Agent context cost** (3.5).
16. **The evidence is thin in one way.** Each cold-agent condition was run once, on one model, by agents
    spawned from the session that wrote the docs. "5:54 instead of 6:24" from one run each is not a result.
    The engine is meant for coding agents in general, and cheaper models are what many indie developers use.

## 6. On the brief's nine points

| # | the brief said | my judgment |
|---|---|---|
| 1 | Data logic is growing into a small language | Agree, and it is recoverable. What exists is an event–condition–action rule set, which is a bounded and checkable form. It needs a written stop line (D4), not a rollback. |
| 2 | "Reference games drive features" has been stretched | Agree. Cold-agent tasks are good evidence for docs, diagnostics and ergonomics. They are not evidence for engine scope. |
| 3 | The rented platform layer is empty | Partly agree. For web and tablets the browser is the rented platform, and 3.2 shows the GPU can wait. Audio and text cannot. |
| 4 | Tablets are unmeasured | Agree. It is the largest unknown in the product claim. |
| 5 | Agent context cost | Agree; measured in 3.5. Decide by experiment. |
| 6 | Code concentration | Agree. The seam to cut along is core against kit, not only file size. |
| 7 | Never run outside one machine | Now run on three more native targets locally (3.1). Windows and a remote CI are still missing. |
| 8 | Open-source readiness waits on the owner | Agree, and two items were not on the list: identity in history, and paths in binaries. |
| 9 | No stable interface for a long-running tool | Disagree on priority. Replay is so cheap (0.2 s for 100,000 frames) that a session server is an optimisation. What the app and MCP need first is a *tested* contract: snapshot tests on the JSON output and the wasm ABI. |

## 7. Decisions for the owner

Each has my recommendation. None is made.

> **Answered by the owner on 2026-11-25**; see [open-questions.md](../open-questions.md). D8 was answered
> against the recommendation: the GPU is a must, because the engine's target is 3D. That reopens D5, D6 and
> D7 and replaces steps 5–9 of the plan in section 8.

| # | decision | recommendation | why |
|---|---|---|---|
| D1 | **Name** (Q1) | Rename before the first public push. I bring a shortlist with crates.io and GitHub availability checked as the next step. | `slate` and `slate_core` are taken on crates.io (open-questions.md, checked 2026-11-24), and "Slate" is already a well-known editor framework, a docs generator and a magazine. A project that cannot be found or published under its own name loses on discoverability. Renaming is a find-and-replace today. |
| D2 | **Licence** (Q2) | `MIT OR Apache-2.0`. | The Rust ecosystem's default; the widest adoption. |
| D3 | **Identity in the public history** | Rewrite the 23 commits to a GitHub no-reply address before the first push, keep the name or a handle as the owner prefers, and move the PDF's content fully into `vision.md`. | It is free before the first push and impossible to undo after. |
| D4 | **Where data logic stops** | Data is event–condition–action rules over declared props, with a closed set of conditions. Inside the line: `not_equals` / `in`, and derived boolean props. Outside it, for good: else-branches, per-action conditions, expressions, timers as a language. Everything outside goes to Rust. | The inside items are plain predicates that two runs asked for. The outside items turn a rule list into a program that `check` can no longer reason about. |
| D5 | **Reference games 2 and 3** | Tracing second, then a multi-level puzzle. Each spec names the foundation it must pull. | Tracing pulls pointer drag, strokes, sound and the kit layer, and its stroke matching is real computation, so it is the honest first user of Rust game code. A puzzle with levels pulls scene flow and saved progress. Another builder variant pulls nothing new. |
| D6 | **Resolution** | A fixed logical frame per game, with a whole-number presentation scale chosen by the player. Tests stay at scale 1. | Authoring, hashes and reference images stay exactly as they are. 3.3 shows logic is already independent of size, and 3.2 shows the CPU can pay for it. |
| D7 | **Target tablets** (Q8) | Name one low-cost Android tablet and the base iPad. Until one is in hand, track a proxy budget in CI (wasm frame time at 1280 × 800). | The claim "tablets first" needs a device. A proxy is better than the current 0. |
| D8 | **GPU** (Q4) | Do not add wgpu now. Revisit when a reference game needs scaled or rotated images, or effects. | 3.2. It would also multiply the wasm size. |
| D9 | **Audio** (Q13) | Sound is an event the simulation logs; the web player plays it with Web Audio. No Rust audio crate. | Testable without a speaker (for example `[[expect]] event = "sound"`), zero wasm bytes, no new dependency. |
| D10 | **Hosting** (Q10) | A GitHub organisation named after the engine. Private first, until CI is green on three systems and the branding is final; then public. | The first public impression is a green badge on Linux, Windows, macOS and wasm. |

## 8. Proposed plan

One step at a time, each approved, each ending with measurements. The order puts the public push early
because the owner wants it soon, and because every later step is safer with CI on three systems.

| step | what | done when |
|---|---|---|
| 1 | This audit | — |
| 2 | **Open-source readiness.** Name shortlist (D1), licence, identity (D3), path remapping, community files, README. CI on GitHub for Linux, Windows, macOS, with the headless wasm check as a gate. | A private repository with a green matrix; the owner flips it public. |
| 3 | **The presentation path** (behaviour-preserving). Span rasterizer for the player, no report on the hot path, skip unchanged frames, presentation scale (D6). | Image hashes identical on every frame of every playtest; frame time down at least 20×; wasm growth under 5 KB. |
| 4 | **Core structure** (behaviour-preserving). `load.rs` as a parse → resolve → check pipeline; `world.rs` in modules along the core/kit line; a state hash that cannot miss a field; limits on work with a diagnostic; snapshot tests for the JSON contract. | All state hashes unchanged; the fuzz run has no hangs. |
| 5 | **Agent context experiment.** Full AGENTS.md against a short core with `slate explain`. Three runs each, on two model tiers. | Tokens, time and pass rate for both; the better one ships. |
| 6 | **The ship path.** `slate export web`, the player embedded in the CLI (Q11), offline install, and the first tablet measurement (D7). | A URL that opens Tiny Town on a tablet, and a number in `tablet.startup_ms`. |
| 7 | **Reference game 2: tracing** (D5). Pointer drag, stroke shapes, sound events (D9), the kit layer, and the Rust path designed with it: Rust code adds actions and conditions that data wires, so `check`, `inspect` and the event log still see everything. | The game, its playtests, an ADR for the Rust path, and a cold-agent run. |
| 8 | **Reference game 3: a puzzle with levels** (D5). Scene flow, saved progress, image and font assets. | The game and its playtests. |
| 9 | **The shared protocol**: MCP adapter over the tested contract (Q9). | Then demos, then the desktop app, as the owner sequenced them. |

Steps 3 and 4 are the "consolidate" half of the mandate, and 6 to 9 are the "complete platform foundation"
half. Demos come after step 8, because before then a demo cannot have levels, sound or a link to open.

## 9. Limits of this audit

- The tablet figures are estimates from a desktop and ADR 0017's 4–6× factor. No tablet was measured.
- Both x86 runs were emulated on Apple silicon. Windows, Safari and Firefox were not run.
- The span rasterizer is a prototype checked on 1,932 frames of three projects. It has no tests of its own.
- I did not run a cold-agent test. Section 3.5 says what to measure, not what the answer is.
- The fuzz run mutates valid projects. It is not a coverage-guided fuzzer and it did not target the wasm ABI.
- I read `world.rs`, `session.rs`, `raster.rs`, the web crate, the player, `xtask`, and about half of
  `load.rs` and `draw.rs`. I outlined but did not read `schema.rs`, `defs.rs`, `commands.rs`, `docs.rs`
  and `report.rs` line by line.
- Arcane and nAIVE (guardrail 3) were not re-examined; I did not compare features against them.
