# Open questions — decisions for the project owner

Things the code cannot decide. Each has a recommendation and what it blocks. When one is decided, record
the answer here (and an ADR if it is architectural), then remove it from the "open" list.

| # | question | recommendation | blocks |
|---|---|---|---|
| Q1 | **Final name.** "slate" is a working codename (tablet + learning + clean slate). `slate` and `slate_core` are taken on crates.io; `slate-engine` and `isoslate` were free on 2026-11-24. | Keep "slate" as the product/CLI name if no trademark issue; publish crates under a free prefix. Rename is a find-and-replace today and gets costlier every week. | publishing, repo name |
| Q2 | **License.** Nothing is licensed yet (`publish = false`, no LICENSE file). | `MIT OR Apache-2.0`, the Rust ecosystem default. | open-sourcing, outside contributions |
| Q3 | **First reference game** and its one-page spec: isometric builder, puzzle, or tracing? | Isometric builder: it exercises the most engine (placement, multi-cell objects, tap-to-build, sorting). The puzzle is already half there (iso_garden). | M3 scope |
| Q4 | **GPU backend timing.** v0.1 renders on the CPU (deterministic, 467 KB wasm). When do we add wgpu? | Only when a reference game misses 60 fps on the target tablet. Keep CPU as the reference renderer forever (tests, reports). | M2/M3 perf work |
| Q5 | **Text rendering.** Owned bitmap font (deterministic, tiny) vs rented shaping (cosmic-text/fontdue; complex scripts). | Bitmap font for UI/debug now; rent shaping when lessons need non-Latin scripts. | quiz/lessons kit, UI |
| Q6 | **Image assets.** PNG sprites and atlases: how are they referenced, packed and validated? | `sprite = { image = "art/hero.png" }` resolved via the Vfs; `check` validates paths/sizes; atlas packing later. | art-heavy games |
| Q7 | **Rust game code delivery.** Game binary embedding the CLI (`slate_cli::main_with`) natively; for the web, compile the game crate to wasm with the same C ABI. Hot-patching via subsecond. | As described; prototype in M4 with one reference game. | M4 |
| Q8 | **Target tablet** and budget numbers (startup ms, frame ms, download size). The direction doc asks for "a mid-range tablet". | Pick one Android tablet and one iPad as the reference devices; measure the web player cold start there. | `tablet.startup_ms` budget |
| Q9 | **MCP adapter**: when, and which tools? | After M3; ≤5 tools mapping 1:1 to `check`, `run`, `shot`, `inspect`, `explain`. | M6 |
| Q10 | **Hosting.** GitHub org/repo name, public from day one? | Public repo once Q1/Q2 are decided; CI is ready (`.github/workflows/ci.yml`). | CI on GitHub |
| Q11 | **Web distribution.** Embed the wasm in the `slate` binary (bigger CLI, zero setup) or ship it separately? Add `slate export web`? | Embed at release time via a build feature; add `export web` in M2. | `slate serve` outside the repo |
| Q12 | **Iso input mapping.** `up` = −y (screen up-right), swipes follow iso axes. Is that intuitive for children on tablets? | Validate with a quick playtest with real users; the mapping is one table to change. | M5 UX |
| Q13 | **Audio crate** (kira / cpal / web audio) | Decide when a reference game needs sound. | M5 |

## Decided

| # | decision | date |
|---|---|---|
| — | Build from scratch, files + CLI first, Rust game code, web + tablets first (the direction doc) | 2026-11-24 |
| — | Working codename "slate" (provisional, see Q1) | 2026-11-24 |
