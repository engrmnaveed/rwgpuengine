# ADR 0017: Web build profile (size-first, fast renderer) and new budgets

- Status: accepted and built, 2026-11-25
- Decided by: the project owner asked for a size pass and "a little" more budget for features; the profile by
  the KPI rule (ADR 0011)

## Context

After M3.4 the wasm was 674,719 bytes against a 700,000 budget. `opt-level = "z"` had been kept in reserve
(−40 KB, ADR 0013) with its frame-time cost unmeasured. The owner asked for a size pass and a somewhat
larger budget, so more features fit without a fight over every kilobyte.

## Measurements

I built Tiny Town in the browser pane (Chromium) and rendered its built town (`build_town`, frame 120,
640 × 480). Each figure is the median of 5 batches of 200 frames, and all variants were re-run to check
stability. **Every variant gives the same state and image hashes** (`82c150f3d192812e`, `58bd8a0dbea14409`):
optimisation does not touch determinism.

| web profile | wasm bytes | gzip -9 | render ms/frame |
|---|---|---|---|
| `opt-level = "s"` (before) | 674,719 | 239,578 | 4.29 |
| `"z"` | 631,342 | — | 5.22 (**+22 %**) |
| `3` | 754,461 | 278,444 | 2.72 (**−37 %**) |
| **`"z"`, with `slate_render` and `slate_core` at `3`** | **643,317** | **234,702** | **2.72** |
| `"s"`, with the same two at `3` | 681,597 | — | 2.74 |

Frame time is spent in the CPU rasterizer (`slate_render`, with helpers in `slate_core`). Simulation steps
are too small to measure next to it. Loading, diagnostics and docs code is cold.

## Decision

1. **Profile:** `[profile.web] opt-level = "z"`, with `opt-level = 3` overrides for `slate_render` and
   `slate_core`. It beats the old profile on both KPIs: **−31 KB and −37 % frame time**. "z" alone was
   rejected because frame time on a tablet matters more than 43 KB, and plain `3` because it costs 111 KB
   for the same speed.
2. **Budgets:**
   - `web.wasm_bytes` goes from 700,000 to **800,000** (owner's call). With the new profile that is 157 KB of
     headroom, about ten features the size of M3.2–M3.4.
   - New **`time.render_frame_us = 6,000`**: a native release render of the same frame, measured by
     `cargo xtask render-bench` at about 2,260 µs. It catches rasterizer regressions of about 2.5× and more,
     allowing for slower CI machines.
   - Native release builds are always `opt-level 3`, so they cannot see the web profile. `cargo xtask
     budgets` therefore also **checks that the two overrides exist**, and fails if a later edit drops them.

## Consequences

- The web player renders a full Tiny Town frame in 2.7 ms on desktop. That leaves a real margin for 60 fps
  on a mid-range tablet, which is roughly 4–6 times slower; it is still unmeasured, see Q8.
- **What a tablet downloads:** 234,702 bytes gzipped, the least of all variants.
- **Not measured:** a real tablet (Q8 is open), and the rasterizer alone at `opt-level = 2`.
