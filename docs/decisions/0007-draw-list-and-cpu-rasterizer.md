# ADR 0007: The draw list is the central render artifact; CPU rasterizer first

- Status: accepted, 2026-11-24. Since 2026-11-26 the world is drawn from the render list of
  [ADR 0020](0020-render-list-camera-and-cpu-reference-renderer.md), in 3D; this record's idea (one list every
  consumer reads, a deterministic CPU reference) carries over, and its flat shapes remain for the 2D layer.

## Context
The engine must render offscreen to PNG, compare against reference images, report what is drawn where,
and resolve taps — headless, in CI, in the browser, with or without a GPU. GPU output differs slightly
across vendors, which makes zero-tolerance reference images impossible.

## Decision
- `draw::build` produces a back-to-front **draw list** of shapes (convex quads, ellipses, rects) with
  colors and owning entities. Every consumer reads it: the rasterizer, the frame report, picking.
- The v0.1 backend is a **deterministic CPU rasterizer**: exact integer coverage at pixel centers
  (doubled coordinates, top-left fill rule — adjacent tiles share edges with no gaps or overlaps),
  integer shading and blending, per-pixel owner ids for visibility and occlusion statistics.
- The same rasterizer drives the web player (`putImageData`), so web pixels equal native pixels.
- A GPU backend (wgpu) will be *added* to consume the same draw list for performance, while the CPU path
  stays the reference for tests and reports.

## Consequences
- Reference images compare with `max_diff_pixels = 0` by default.
- Picking and the frame report cannot disagree with what is drawn.
- CPU cost grows with resolution × overdraw; fine at 640×480, to be measured on tablets (Q4, Q8).

## Revisit when
Frame time on the target tablet misses 60 fps for a reference game, or effects need shaders.
