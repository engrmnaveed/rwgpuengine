# ADR 0018: A paint path for players; the reference rasterizer stays for checks

- Status: accepted and built, 2026-11-25. Since 2026-11-26 players paint the world with the `Painter` of
  [ADR 0020](0020-render-list-camera-and-cpu-reference-renderer.md) (161 µs for the frame measured here at
  55); the `Presenter` and the span filling of this record remain, the latter for cards and the 2D layer.
- Decided by: the owner chose "consolidate first" after the
  [architecture audit](../audits/2026-11-25-architecture-audit.md); the shape by the KPI rule (ADR 0011)
- Scope note: on the same day the owner decided that the engine's target is 3D on the GPU
  ([open questions](../open-questions.md), Q4). This ADR is about the CPU player that exists today. It does
  **not** argue that a GPU backend can wait.

## Context

The audit measured where a frame of the web player goes (Tiny Town, 640 × 480, native release):

| part | time |
|---|---|
| one simulation step | 1.5 µs |
| build the draw list | 4 µs |
| rasterize (reference: every pixel of each bounding box, an owner per pixel, a map of who covers whom) | 1,145 µs |
| build the frame report, which the player throws away | 1,085 µs |

Only 131 of the 1,560 frames in the nine example playtests change any pixel, and the player repainted all of
them.

## Options

| option | frame time | pixels | cost |
|---|---|---|---|
| (a) Keep one rasterizer; only stop building the report in the player | about half | same | none, but the slow half stays |
| (b) Replace the reference rasterizer with a span rasterizer that also tracks owners | fast | same | The report needs an owner per pixel, so most of the gain is lost there, and the checks lose their simple oracle. |
| **(c) Add a paint path beside the reference** | **41× less** | **same, held by tests** | about 200 lines and two exports |

## Decision

1. **`slate_render::paint`** fills a pixel buffer from a draw list: colors only. For each shape it solves the
   coverage rule for a whole run of pixels per row. The rule is the same exact pixel-centre rule as
   `Shape::covers`; shapes too large for the solver's integer range fall back to the per-pixel test. Long
   opaque runs are filled by block copies, which is what makes it fast in wasm, where there are no vector
   instructions.
2. **`rasterize` is unchanged and stays the reference.** `slate shot`, playtest `[[shot]]`s, warnings and the
   frame report use it. The CLI's output does not change by a byte.
3. **`slate_runtime::Presenter`** keeps a player's buffer and repaints only when the draw list differs from
   the one it painted. `Session::paint` paints once.
4. **Web ABI:** `slate_present()` returns 1 when the pixels at `slate_pixels()` are new. `slate_render()`
   still always paints, now through the paint path. `slate_image_hash()` hashes the painted pixels, so the
   web module no longer contains the reference rasterizer or the report. 22 exports (ADR 0008 revisits at 40).
5. **Budget:** new `time.paint_frame_us = 200` (measured 55–60). `time.render_frame_us` keeps guarding the
   reference path.

## Evidence

| KPI | before | after |
|---|---|---|
| a changed frame, native, 640 × 480 | 2,256 µs | **55 µs** (41×) |
| a changed frame, wasm in Node, 640 × 480 | 2.81 ms | **0.09 ms** (31×) |
| a changed frame, wasm in Node, 2560 × 1920 | 42.2 ms | **0.91 ms** (46×) |
| frames repainted in the example playtests | 1,569 of 1,569 | **140** (one in eleven) |
| web wasm | 643,317 bytes (234,702 gzipped) | **585,725** (215,418): −57,592 |
| CLI output | — | unchanged |

Pixel identity is held four ways:
- 9,000 random shapes (any four points as a quad, boxes, diamonds, ellipses, rects, glyphs; clipped,
  off-screen and degenerate ones included): the runs cover exactly the pixels `covers` accepts, each once.
- A scene with every kind of shape, see-through colors and a HUD: `paint` equals `rasterize` byte for byte.
- Every frame of all nine example playtests: whenever the presenter repaints, its pixels equal the
  reference's; a skipped frame is checked against the reference on a sample.
- In wasm: for all nine playtests the painted pixels hash to the image hash of native `slate shot`
  (`docs/audits/2026-11-25/tools/wasm_check.mjs --native`).

Five mutation checks on the solver (each edge rule, the ellipse reach, glyph clipping) fail the shape test.

The web profile of ADR 0017 was measured again on the new path, and it stays:

| web profile | wasm bytes | paint 640 × 480 | paint 2560 × 1920 |
|---|---|---|---|
| **`"z"`, with `slate_render` and `slate_core` at `3`** (kept) | 585,750 | 0.26 ms | 3.9 ms |
| `"z"` everywhere | 571,411 | 0.63 ms | 9.6 ms |
| `"z"`, with only `slate_render` at `3` | 581,195 | 0.30 ms | 4.5 ms |
| `3` everywhere | 697,304 | 0.26 ms | 3.9 ms |

These four were measured before the block-copy fill, which then took the kept profile to 0.09 ms and 0.91 ms
at the same size.

## Consequences

- A correction to the audit: it estimated wasm paint time as native time × 1.2, the ratio of the old path.
  Measured, the first paint path was six times slower in wasm than natively at 2560 × 1920, because native
  code vectorises the fills and wasm does not. The block-copy fill brought wasm to 0.91 ms. The audit's tablet
  estimate for that size should read 4–6 ms, not 3–4.5 ms. No tablet has been measured.
- A list that differs can still paint the same pixels (something moves behind a wall). The presenter then
  repaints. It never skips a frame whose picture changed.
- The frame report is no longer available inside the web module. If a tool in the browser needs it, the
  reference path comes back for about 58 KB.
- `slate_web` grew by two exports and `slate_runtime` by one type.

## Not built

A presentation scale for larger screens (audit, D6); wasm vector instructions; anything on the GPU. All
three now belong to the 3D direction the owner set, not to this path.

## Revisit when

The GPU renderer presents the game. This path then remains for machines without a GPU and for whatever 2D
layer the CPU reference keeps.
