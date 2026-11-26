# ADR 0021: The players' renderer — `rwgpuengine_gpu` draws the render list with wgpu and is held to the CPU reference

- Status: accepted and built, 2026-11-26 (step 3 of the [3D proposal](../proposals/3d-architecture.md),
  piece 2 of 3: the renderer, its check and the players' budgets; the window is piece 3)
- Decided by: the owner approved the two halves, "a visual feature exists in the reference first", separate
  budgets for players, and WebGPU only on the web. The shape of each piece here was chosen by measurement;
  the options are in the tables below. **One point is the owner's to decide** and is only proposed here:
  [what the reference leaves to rounding](#what-the-reference-leaves-to-rounding-the-owners-call).
- Builds on: [ADR 0020](0020-render-list-camera-and-cpu-reference-renderer.md) (the render list and the
  reference renderer) and the measurement of wgpu on GitHub's runners
  ([spikes README](../proposals/3d-architecture/spikes/README.md#on-githubs-runners)).

## Context

The agent's half never needs a GPU, and the CPU reference renderer is the source of truth for what a frame
looks like. Players need the same picture fast. The measurement before this step showed two things. A GPU
can come within a few pixels of the reference, but only if it writes whole 8-bit colors. And no GPU picture
is the same on two systems, so a GPU picture can never be a reference image.

So the renderer has one job: draw the render list by the reference's rules. And the check has one question:
is every differing pixel on an edge of the reference picture, and are there few of them?

## Decision

1. **Two halves, two workspaces.** `players/` is a Cargo workspace of its own, with its own `Cargo.lock`,
   lints and profiles. It holds `rwgpuengine_gpu` and the tool `gpu_check`; the window crate comes next.
   The root workspace (`crates/`, `xtask`) does not list it. Why this and not one workspace with
   `default-members`:

   | | one workspace, `default-members` | **a players' workspace** |
   |---|---|---|
   | root `Cargo.lock` | grows from 33 packages to 130 | **unchanged: 33** |
   | `cargo test --workspace`, `cargo clippy --workspace` at the root | compile wgpu | **cannot: wgpu is not in the lockfile** |
   | what `cargo xtask ci` covers | every `--workspace` needs an exclusion | **unchanged** |
   | the players' crate counts and build time | measured inside a mixed tree | **measured on their own tree** |

   The players' crates depend on the agent's by path, never the other way round. `cargo xtask gpu` first
   checks that the root lockfile names no GPU or window crate. `rwgpu` stays GPU-free.
2. **`rwgpuengine_gpu::Renderer`** (wgpu 30.0.2, the exact version) is handed a `wgpu::Device` and a
   `wgpu::Queue`. It has no window and no executor, and it compiles for wasm32. `draw(list)` draws into the
   renderer's own frame: the list's size, RGBA8, no sRGB encoding, so the frame's bytes are the reference's
   bytes. `copy_frame` starts a copy to RGBA bytes; `present` scales the frame onto any target view. It needs
   no wgpu feature, and `Limits::downlevel_defaults()` is enough.
3. **What decides a pixel comes from the reference's own code.** `rwgpuengine_render` makes public, with no
   change in behavior: the `Projector`'s matrix, range and `card`, a card's shapes, the far-to-near key of
   see-through things, the table of faces and `shade`. The GPU projects corners and fills pixels.
4. **How each rule is drawn.**
   - *Instances.* One instanced draw for the sides of all boxes, one for their level faces, one for all flat
     rectangles. The vertex shader multiplies by the `Projector`'s matrix, with positions taken from the
     camera's center.
   - *Color.* A table of 6 faces × 256 levels, filled by the reference's `shade`. The GPU selects; it does
     no color arithmetic and writes whole 8-bit levels.
   - *The camera's range.* Depth is reversed (1 nearest, 0 farthest, the larger wins), mapped so that the
     GPU's own clipping cuts exactly where the reference cuts near and far. A float is densest near 0, so
     far things keep their precision, as in the reference.
   - *Level faces in one plane.* A level face writes its depth per pixel from the plane's equation and puts
     its rank in the three lowest bits. Every face of one plane computes the same bits, whatever triangle it
     is part of, so faces in one plane differ in nothing but rank and one ordinary depth test orders them:
     the higher rank is nearer, and equal ranks are equal depths (the first drawn stays). Sides keep the
     rasterizer's depth, so a tile cannot show through the foot of a box.
   - *Cards.* Placed on the CPU by `Projector::card` and `card_shapes`, so a sprite's rectangle is rounded
     as the reference rounds it. Each shape is two triangles over its box; the fragment shader tests the
     reference's integer coverage rule (rectangle, ellipse in 64-bit arithmetic, quad, glyph bits) and
     writes a depth per pixel row. Above the anchor the card is upright. Below it the card lies in the
     anchor's level plane, one step behind any floor in that plane. A label adds its lift to its sprite's
     depth, so it is always in front.
   - *See-through things.* After the solid ones, far to near in the reference's order, testing depth and
     writing none. Their color is mixed in the fragment shader with the reference's integer formula, read
     from a copy of the picture so far. Things that do not overlap on the screen share a pass; one that
     overlaps what was drawn since the last copy starts a new pass.
   - *The 2D layer.* The same shapes with no depth test, in list order, last.
   - *Disabled items* are left out when the buffers are filled.
5. **The check, `cargo xtask gpu`.** 56 scenes: the six frames the example playtests shoot (the turned and
   the perspective camera of `turn_and_build` among them), the generated town of 5,000 boxes at 1280 × 720
   in perspective, and a scene per rule under nine cameras (five iso, four perspective; turned, zoomed,
   panned). Each scene's list is drawn by both renderers. **There is no per-channel tolerance.** A pixel that
   differs must lie on an edge of the reference picture (a neighbor has another owner or another color), and
   a scene may have at most `pixels_apart` of them. A failing scene prints its pixels with both colors and
   the reference's owner, and writes `target/gpu-check/<scene>.{reference,gpu,diff}.png`. The command prints
   the adapter; with none it fails and says what to install.
6. **Players' budgets** (`budgets.toml`, `[players]`), measured and enforced by `cargo xtask gpu`:

   | budget | limit | measured |
   |---|---|---|
   | rented crates, on the widest system | 130 | Windows 76, macOS 67, Linux 65; the renderer alone on wasm32 54 |
   | proc-macro crates, on the widest system | 6 | Windows 6, macOS and Linux 4 |
   | clean release build of the check | 60 s | 12.2 s on an Apple M5 Pro; 39 s, 72 s and 89 s on the Linux, macOS and Windows runners, where the limit is 240 s |
   | GPU frame, 50,000 boxes at 1280 × 720, nothing uploaded | 4 ms (real GPUs only) | 1.0 to 1.75 ms on an Apple M5 Pro; 7.0 ms on the macOS runner's virtual GPU, where the limit is 16 ms; 67 ms on lavapipe and 103 ms on WARP, printed and not enforced |
   | pixels apart per scene, all on an edge | 24 | at most 3 on Metal and WARP, 6 on lavapipe |

   The 1,500,000-byte web download budget belongs to step 4 and is not here.
7. **CI.** A `gpu` job runs `cargo xtask gpu` on `ubuntu-latest` (Mesa lavapipe, after installing
   `mesa-vulkan-drivers libvulkan1`), `windows-latest` (WARP) and `macos-latest` (Metal). The `ci` job is
   unchanged. The same job then prints the option study below for its adapter, as a measurement.

## Options that were measured

Pixels apart from the reference, summed over the 56 scenes, on each adapter; in brackets how many of them
are *not* on an edge. `cargo xtask gpu --study` repeats this table on the adapter at hand.

| question | option | Apple M5 Pro, Metal | macOS runner, Metal | lavapipe, Vulkan | WARP, Direct3D 12 |
|---|---|---|---|---|---|
| | **what was chosen** | **16 (0)** | **16 (0)** | **26 (0)** | **22 (0)** |
| A face's color | rounded in the vertex shader | 16 (0) | 16 (0) | 26 (0) | 22 (0) |
| See-through colors | the GPU's own blender | 16 (0) | 16 (0) | 50,949 (43,747) | 22 (0) |
| Faces in one plane | the rasterizer's depth, 16 steps nearer per rank | 29 (0) | 29 (0) | 45 (0) | 41 (0) |
| | 256 steps nearer per rank | 589 (99) | 589 (99) | 606 (99) | 601 (99) |
| | 4,096 steps nearer per rank | 7,957 (6,566) | 7,957 (6,566) | 7,974 (6,566) | 7,969 (6,566) |
| A card's depth | the anchor's depth everywhere | 12,295 (11,348) | 12,295 (11,348) | 12,305 (11,348) | 12,301 (11,348) |
| Corner positions | taken from the world's origin | 18 (0) | 18 (0) | 28 (0) | 23 (0) |

A frame of 50,000 boxes on the Apple M5 Pro took 0.95 to 1.16 ms under every option: none of them is a
question of speed there.

What the rows say:

- **Color.** No difference on any adapter. Once the color travels as whole numbers and is not interpolated,
  rounding in the shader gives the reference's levels too. The table was chosen because it leaves the GPU
  no arithmetic to get wrong: it is the reference's own function, for any light, on any driver, for 1,536
  bytes.
- **Rank.** A depth bias has to be larger than the rasterizer's own noise between two triangles of one
  plane, and as soon as it is, it shows the flat thing through the foot of whatever stands on it. At 16
  steps one scene alone has 19 wrong pixels; at 256 steps 99 pixels are inside faces. The bits of a
  computed depth have no such trade.
- **Cards.** One depth per card cuts tall sprites by the wall behind them, as ADR 0020 measured in the
  reference.
- **See-through.** Metal and WARP blend exactly. Lavapipe does not: 50,949 pixels differ, 43,747 of them
  inside faces. Mixing in the shader is exact on all four. Its price is a copy of the frame and a pass
  wherever see-through things overlap: 5 to 7 passes in the see-through scene, 1 or 2 in the example games.
- **Corners.** Taking positions from the camera's center keeps the numbers in the vertex shader small. It
  is free.
- **Instancing against projecting on the CPU.** The reference's own projection of 50,000 boxes takes
  6.2 ms on an Apple M5 Pro before a pixel is drawn, over the 4 ms budget by itself. The GPU projects them
  inside a frame of 1.0 to 1.75 ms.
- **Text and the 2D layer as shapes against a texture painted by the reference.** Not built. A texture
  cannot carry see-through shapes that must be mixed with the world one after another, and every label
  change would upload it again. The shapes need 88 bytes each.

## Evidence

CI run 45766846084 (cold caches) and the study's machine. Pixels are out of 307,200 per scene (921,600 for
the town).

| adapter | scenes failed | pixels apart: all 56 scenes / worst scene | not on an edge | frame, 5,000 / 50,000 boxes, nothing uploaded | every instance uploaded again | clean release build | the `gpu` job |
|---|---|---|---|---|---|---|---|
| Apple M5 Pro, Metal | 0 | 16 / 3 | 0 | 0.3–0.7 / 1.0–1.75 ms | 0.4–0.6 / 1.5–2.8 ms | 12.2 s | — |
| `macos-latest`: Apple Paravirtual device, Metal | 0 | 16 / 3 | 0 | 2.7 / 7.0 ms | 4.6 / 9.9 ms | 72 s | 6 min 35 s |
| `ubuntu-latest`: Mesa lavapipe 26.0.8, Vulkan | 0 | 26 / 6 | 0 | 11.2 / 66.8 ms | 11.3 / 68.2 ms | 39 s | 3 min 58 s |
| `windows-latest`: WARP, Direct3D 12 | 0 | 22 / 3 | 0 | 19.4 / 103 ms | 89 / 715 ms | 89 s | 7 min 32 s |

- The six frames the example playtests shoot are identical to the reference, byte for byte, on Metal and
  WARP. On lavapipe five are, and one differs in one pixel.
- The worst scene is the town on lavapipe (6 pixels); on Metal and WARP it is 3 pixels in a small scene.
  The limit is set at four times the worst: `pixels_apart = 24`.
- The macOS runner's pictures match the M5 Pro's count for count, as the measurement before this step found.

- All ten playtests keep their state and image hashes; no example file and no reference image changed.
- The agent's half before and after, clean builds on the same machine: root `Cargo.lock` unchanged
  (33 packages, 16 rented crates in the runtime tree, 0 proc macros); web module 619,742 → 619,866 bytes;
  command 1,979,088 → 1,979,136 bytes; clean release build of `rwgpu` 2.8 s and clean dev build 2.2 s,
  both unchanged; painting a frame for a player 152 to 160 µs (limit 200).
- `present` at the frame's own size gives the frame's bytes, and at twice the size each pixel four times
  (checked on every adapter).

## What the reference leaves to rounding (the owner's call)

Building the check showed two places where the reference's own picture is decided by rounding, not by a
rule. A GPU cannot match rounding, so the gate's scenes avoid both, and `--study` prints them instead.

| case | what the reference does | what `rwgpuengine_gpu` does |
|---|---|---|
| **A tile larger than one cell** (its corners are not the ground cells' corners), under a camera that does not put corners on whole pixels | The ground shows through it in patches. Of a 3 × 3 tile it shows 59 % (iso, turned 217°, zoom 1.75) and 39 %, 68 %, 24 % and 52 % (the four perspective cameras); all of it under the other four iso cameras. | all of the tile, under every camera |
| **The part of a card below its anchor, on a floor at exactly the anchor's height** | In the scene's starting view, and under two more iso cameras, the floor always wins: a sprite nudged down 10 pixels loses those 10 rows. Under the other six cameras it is a coin toss per ground triangle: 39 % to 68 % of those pixels show. A sprite seen at another size than its own can have its last row there too (51 pixels under one of the nine cameras). | the floor always wins |

Why: two faces in one plane get their depths from different triangles, whose corners were snapped to the
1/256 pixel grid. The depths then differ by more than the tolerance of the rank rule, and a card has no rank
at all. No example game has either case: their tiles are one cell and their sprites stand on or above
their anchor.

**Proposal** (not part of this change, because it changes the reference): take a level face's depth from
its plane's equation, as the GPU renderer does, and let the part of a card below its anchor win against a
floor in its own plane, as ADR 0020 says it should ("a sprite nudged down a few pixels is not cut by the
floor"). Tried in a scratch copy, about 60 lines in `reference.rs`: all ten playtests keep their image and
state hashes, all 44 render tests pass, the tile and the sprites show whole under every camera, and
painting takes 151 µs against 152. The GPU side would then change one line (the card's lower part goes one
step in front of the floor), and both cases could enter the gate.

## Consequences and limits

- `cargo xtask ci` covers what it covered. `cargo xtask gpu` is a second gate, for changes to `players/`,
  to `rwgpuengine_render` and to anything visual. On a machine with no adapter it fails with a message.
- Level faces and cards compute their depth in the fragment shader, so the GPU cannot reject them before
  shading. Measured: a frame of 50,000 boxes takes the same time with the rank as a depth bias (no
  per-pixel depth) as with the chosen design, within the noise.
- See-through things that overlap cost one copy of the frame and one pass each. The see-through scene
  takes 5 to 7 passes. A game with hundreds of overlapping see-through things would need a faster path that
  is not held to the reference; none is built.
- Cards are placed on the CPU every frame (two calls of `Projector::card` for a label). Instances are sent
  to the GPU only when they changed, so a camera moving over a still world uploads 1,728 bytes and the cards.
- A rank above 7 (a layer above 6) is treated as 7. A sprite larger than 32,767 pixels is not drawn
  exactly. Where a flat rectangle and the top of a box have the same rank in one plane, the rectangle shows
  (the reference shows whichever is first in the file). Where a card stands at the height of a floor that
  comes later in the file, the reference lets the card's lower part win in the starting view; here the
  floor wins.
- Windows has no room left in the proc-macro budget: `windows-implement` and `windows-interface` come with
  Direct3D 12. On Linux, where winit will add the most crates, 65 of 130 are used; the proposal counted 51
  more for winit.
- No GL backend: Vulkan, Metal, Direct3D 12 and WebGPU. Turning GL on adds 4 crates on Linux and Windows.

## Not done

A window, input, camera gestures, `rwgpu play`, the web player and its download budget, culling and levels
of detail, anti-aliasing (it would be presentation only and outside the comparison), a GL backend, the fix
to the reference proposed above.

## Revisit when

The reference gains a rule the fragment shader cannot compute exactly; a game needs many overlapping
see-through things; a wgpu upgrade changes the picture on an adapter (the check will say so); or the owner
decides the proposal above.
