# ADR 0021: The players' renderer — `rwgpuengine_gpu` draws the render list with wgpu and is held to the CPU reference

- Status: accepted and built, 2026-11-26 (step 3 of the [3D proposal](../proposals/3d-architecture.md),
  piece 2 of 3: the renderer, its check and the players' budgets; the window is piece 3)
- Decided by: the owner approved the two halves, "a visual feature exists in the reference first", separate
  budgets for players, and WebGPU only on the web. The shape of each piece here was chosen by measurement;
  the options are in the tables below. The check found a defect in the reference renderer; it was fixed
  the same day ([what the check found in the reference](#what-the-check-found-in-the-reference-and-what-stays-undefined)).
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
3. **What decides a pixel comes from the reference's own code.** `rwgpuengine_render` makes public: the
   `Projector`'s matrix, range and `card`, the depth of level planes over the screen, a card's shapes, the
   far-to-near key of see-through things, the table of faces and `shade`. The GPU projects corners and
   fills pixels.
4. **How each rule is drawn.**
   - *Instances.* One instanced draw for the sides of all boxes, one for their level faces, one for all flat
     rectangles. The vertex shader multiplies by the `Projector`'s matrix, with positions taken from the
     camera's center. In the iso view an upright edge of a box is one pixel column from top to bottom, and a
     single-precision sum can put that column one sub-pixel step beside the reference's; so there the CPU
     gives each instance's four corner columns, computed and snapped as the reference does (16 bytes per
     instance, sent when the camera or the instances changed).
   - *Color.* A table of 6 faces × 256 levels, filled by the reference's `shade`. The GPU selects; it does
     no color arithmetic and writes whole 8-bit levels.
   - *The camera's range, and how depth is stored.* Depth is reversed (1 nearest, the farthest next to 0,
     the larger wins), mapped so that the GPU's own clipping cuts exactly where the reference cuts near and
     far. In perspective the reference's depth goes onto 1..0 in a straight line: it already is dense
     where the scene is. In the iso view the scene sits in the middle of the range, where a straight line
     would land at 0.5 and leave a float 100 times coarser than the reference's. So the iso range is laid
     along the float's exponent: every 1/60 of it halves the value, and within such a step the value falls
     in a straight line. That is monotonic, put together from bits (no `exp2`), and about 60 times finer:
     a depth step is 2e-9 of the range. In the iso view every face writes its depth per pixel for this; in
     perspective the sides keep the rasterizer's depth.
   - *Level faces in one plane.* A level face writes its depth per pixel from the plane's equation
     (`Projector::level_planes`, the equation the reference uses too) and adds two depth steps per rank.
     Every face of one plane computes the same bits, whatever triangle it is part of, so faces in one plane
     differ in nothing but rank and one ordinary depth test orders them: the higher rank is nearer, and
     equal ranks are equal depths (the first drawn stays). No bits are cleared, so against anything that is
     not in the plane the depth is within a few steps of the true one. Sides keep their own depth, so a
     tile cannot show through the foot of a box.
   - *Cards.* Placed on the CPU by `Projector::card` and `card_shapes`, so a sprite's rectangle is rounded
     as the reference rounds it. Each shape is two triangles over its box; the fragment shader tests the
     reference's integer coverage rule (rectangle, ellipse in 64-bit arithmetic, quad, glyph bits) and
     writes a depth per pixel row. Above the anchor the card is upright, at its own depth. At and below
     the anchor it lies in the anchor's level plane, 14 steps nearer than the plane: in front of any floor
     there. In the one row just above the anchor it is held to at least that, so a card never falls behind
     the floor it stands on. A label adds its lift to its sprite's depth, so it is always in front.
   - *See-through things.* After the solid ones, far to near in the reference's order, testing depth and
     writing none. Their color is mixed in the fragment shader with the reference's integer formula, read
     from a copy of the picture so far. Things that do not overlap on the screen share a pass; one that
     overlaps what was drawn since the last copy starts a new pass.
   - *The 2D layer.* The same shapes with no depth test, in list order, last.
   - *Disabled items* are left out when the buffers are filled.
5. **The check, `cargo xtask gpu`.** 271 scenes. 71 are hand-made: the six frames the example playtests
   shoot (the turned and the perspective camera of `turn_and_build` among them); the generated town of
   5,000 boxes at 1280 × 720 through its own perspective camera and four more; a scene per rule under nine
   cameras (five iso, four perspective; turned, zoomed, panned); and three small copies of what random
   scenes found. 200 are random: boxes, flat things of one to nine cells, sprites and text above, on and
   below their anchor, a HUD, some see-through, some disabled, each under a random camera, the same on
   every run. Each scene's list is drawn by both renderers. **There is no per-channel tolerance.** A pixel
   that differs must lie on an edge of the reference picture (a neighbor has another owner or another
   color), and a scene may have at most `pixels_apart` of them. A failing scene prints its pixels with
   both colors and the reference's owner, and writes `target/gpu-check/<scene>.{reference,gpu,diff}.png`
   and the scene file. The command prints the adapter; with none it fails and says what to install.
6. **Players' budgets** (`budgets.toml`, `[players]`), measured and enforced by `cargo xtask gpu`:

   | budget | limit | measured |
   |---|---|---|
   | rented crates, on the widest system | 130 | Windows 76, macOS 67, Linux 65; the renderer alone on wasm32 54 |
   | proc-macro crates, on the widest system | 6 | Windows 6, macOS and Linux 4 |
   | clean release build of the check | 60 s | 12 to 17 s on an Apple M5 Pro; 37 to 55 s, 66 to 79 s and 85 to 113 s on the Linux, macOS and Windows runners, where the limit is 240 s |
   | GPU frame, 50,000 boxes at 1280 × 720, nothing uploaded | 4 ms (real GPUs only) | 1.0 to 1.75 ms on an Apple M5 Pro; 6.2 to 9.0 ms on the macOS runner's virtual GPU, where the limit is 16 ms; 67 to 111 ms on lavapipe and 98 to 149 ms on WARP, printed and not enforced |
   | pixels apart per scene, all on an edge | 24 | at most 7 on Metal, 5 on WARP, 12 on lavapipe |

   The 1,500,000-byte web download budget belongs to step 4 and is not here.
7. **CI.** A `gpu` job runs `cargo xtask gpu` on `ubuntu-latest` (Mesa lavapipe, after installing
   `mesa-vulkan-drivers libvulkan1`), `windows-latest` (WARP) and `macos-latest` (Metal). The `ci` job is
   unchanged. The same job then prints the option study below for its adapter, as a measurement.

## Options that were measured

Pixels apart from the reference, summed over the scenes, on each adapter; in brackets how many of them
are *not* on an edge. `cargo xtask gpu --study` repeats the measurement on the adapter at hand, and the CI
job prints it on every run.

The first table was measured with the 256 scenes of the first version (CI run 45769343354). The depth-bias
option is no longer in the renderer: it cannot place a card's lower part in a plane, so it stopped drawing
the same rules when the reference was fixed.

| question | option | Apple M5 Pro, Metal | macOS runner, Metal | lavapipe, Vulkan | WARP, Direct3D 12 |
|---|---|---|---|---|---|
| | **what was chosen** | **33 (0)** | **33 (0)** | **79 (0)** | **63 (0)** |
| A face's color | rounded in the vertex shader | 33 (0) | 33 (0) | 79 (0) | 63 (0) |
| See-through colors | the GPU's own blender | 33 (0) | 33 (0) | 1,486,744 (1,292,109) | 63 (0) |
| Faces in one plane | the rasterizer's depth, 16 steps nearer per rank | 35 (0) | 35 (0) | 98 (0) | 65 (0) |
| | 256 steps nearer per rank | 788 (105) | 788 (105) | 851 (105) | 818 (105) |
| | 4,096 steps nearer per rank | 25,404 (20,653) | 25,404 (20,653) | 25,467 (20,653) | 25,434 (20,653) |
| A card's depth | the anchor's depth everywhere | 21,141 (17,757) | 21,141 (17,757) | 21,186 (17,757) | 21,171 (17,757) |
| Corner positions | taken from the world's origin | 35 (0) | 35 (0) | 83 (0) | 63 (0) |
| Iso depth | a straight line onto 1..0 | 240 (0) | 240 (0) | 280 (0) | 268 (0) |

A frame of 50,000 boxes on the Apple M5 Pro took 0.96 to 1.02 ms under every option: none of them is a
question of speed there.

Measured after the reference was fixed, with the 271 scenes (CI run 45794096174):

| question | option | Apple M5 Pro, Metal | macOS runner, Metal | lavapipe, Vulkan | WARP, Direct3D 12 |
|---|---|---|---|---|---|
| | **what was chosen** | **34 (0)** | **34 (0)** | **91 (0)** | **61 (0)** |
| Iso corner columns | projected by the GPU | 99 (0) | 99 (0) | 105 (0) | 67 (0) |
| Iso depth | a straight line onto 1..0 | 383 (0) | 383 (0) | 429 (0) | 410 (0) |
| A card's depth | the anchor's depth everywhere | 111,721 (87,204) | 111,721 (87,204) | 111,777 (87,204) | 111,748 (87,204) |
| See-through colors | the GPU's own blender | 34 (0) | 34 (0) | 1,210,059 (1,058,581) | 61 (0) |

What the rows say:

- **Color.** No difference on any adapter. Once the color travels as whole numbers and is not interpolated,
  rounding in the shader gives the reference's levels too. The table was chosen because it leaves the GPU
  no arithmetic to get wrong: it is the reference's own function, for any light, on any driver, for 1,536
  bytes.
- **See-through.** Metal and WARP blend exactly. Lavapipe does not: nearly every see-through pixel is
  wrong, 1.3 million of them inside faces. Mixing in the shader is exact on all four. Its price is a copy
  of the frame and a pass wherever see-through things overlap: 5 to 7 passes in the see-through scene, 1 or
  2 in the example games.
- **Rank.** A depth bias has to be larger than the rasterizer's own noise between two triangles of one
  plane, and as soon as it is much larger, it shows the flat thing through the foot of whatever stands on
  it. At 16 steps it costs 2 to 19 pixels more than the chosen design; at 256 steps 105 pixels are inside
  faces. There is a window, and it is narrow. The bits of a computed depth need no number chosen for them,
  and they cost no time.
- **Cards.** One depth per card cuts tall sprites by the wall behind them, as ADR 0020 measured in the
  reference.
- **Iso depth.** With a straight line the scene sits at 0.5 of the depth buffer, where three rank bits and
  a card's lead stand for 0.08 × zoom pixels of height. Where a sprite crosses the top of a box that close
  to a pixel center, a whole row goes the wrong way: 46 and 53 pixels in two of the first 300 random
  scenes, over the limit. The hand-made scenes did not show it; the random ones did. Along the exponent a
  depth step is 60 times finer and both scenes are exact.
- **Corners.** Taking positions from the camera's center keeps the numbers in the vertex shader small. It
  is free.
- **Iso corner columns.** Projected by the GPU, a box two units high was drawn one pixel off along a whole
  upright edge (64 pixels) under one camera in about a thousand. From the CPU the edge is the reference's.
  It costs 16 bytes per instance when an iso camera moves: 1.6 ms a frame for 50,000 boxes while the
  camera turns, against 1.2 ms when it is still.
- **Instancing against projecting on the CPU.** The reference's own projection of 50,000 boxes takes
  6.2 ms on an Apple M5 Pro before a pixel is drawn, over the 4 ms budget by itself. The GPU projects them
  inside a frame of 1.0 to 1.75 ms.
- **Text and the 2D layer as shapes against a texture painted by the reference.** Not built. A texture
  cannot carry see-through shapes that must be mixed with the world one after another, and every label
  change would upload it again. The shapes need 88 bytes each.

## Evidence

CI run 45794096174 and the study's machine, after the reference was fixed. A scene has 307,200 pixels (the
town 921,600).

| adapter | scenes failed | pixels apart: all 271 scenes / worst scene | not on an edge | frame, 5,000 / 50,000 boxes, nothing uploaded | 50,000 boxes, iso view, still / turning | every instance uploaded again | the `gpu` job, warm / cold |
|---|---|---|---|---|---|---|---|
| Apple M5 Pro, Metal | 0 | 34 / 7 | 0 | 0.4 / 1.0 ms | 1.2 / 1.6 ms | 1.6 ms | — |
| `macos-latest`: Apple Paravirtual device, Metal | 0 | 34 / 7 | 0 | 3.4 / 9.0 ms | 8.4 / 12.2 ms | 13.6 ms | 2 min 55 s / 6 min 8 s |
| `ubuntu-latest`: Mesa lavapipe 26.0.8, Vulkan | 0 | 91 / 12 | 0 | 16 / 111 ms | 114 / 116 ms | 113 ms | 2 min 10 s / 3 min 42 s |
| `windows-latest`: WARP, Direct3D 12 | 0 | 61 / 5 | 0 | 22 / 149 ms | 222 / 164 ms | 170 ms | 3 min 46 s / 7 min 27 s |

- The same 256 scenes before and after the reference was fixed (pixels apart in all): 33 → 18 on the Apple
  M5 Pro and on the macOS runner, 79 → 68 on lavapipe, 63 → 52 on WARP. Half of what was left on Metal was
  the reference's own rounding.
- The six frames the example playtests shoot are identical to the reference, byte for byte, on Metal and
  WARP. On lavapipe five are, and one differs in one pixel.
- The worst scene is the town of 5,000 boxes in perspective, turned and zoomed in: 12 pixels on lavapipe, 7 on Metal, 5 on
  WARP. That scene is new; on the scenes that were there before, every adapter has fewer pixels apart than
  it had. The limit stays at `pixels_apart = 24`.
- 20,000 random scenes on the Apple M5 Pro: none failed. 18,525 are identical to the reference, 1,280
  differ in one pixel, 167 in two, 27 in three to five, and one in 13 (a ground edge seen through a
  see-through side).
- The macOS runner's pictures match the M5 Pro's count for count, as the measurement before this step found.
- All ten playtests keep their state and image hashes; no example file and no reference image changed.
- The agent's half before and after the first version, clean builds on the same machine: root `Cargo.lock`
  unchanged (33 packages, 16 rented crates in the runtime tree, 0 proc macros); web module 619,742 →
  619,866 bytes; command 1,979,088 → 1,979,136 bytes; clean release build of `rwgpu` 2.8 s → 2.8 s, clean
  dev build 2.2 s → 2.3 s, clean web build 3.0 s → 3.1 s; painting a frame for a player 151 to 160 µs
  (limit 200).
- `present` at the frame's own size gives the frame's bytes, at twice the size each pixel four times, and
  on an sRGB target the same bytes to within one level (checked on every adapter).

## What the check found in the reference, and what stays undefined

Building the check showed that the reference renderer did not keep two rules of ADR 0020 where two faces of
one plane have different corners. A face's depth came from its triangle's corners, and corners are snapped
to the 1/256 pixel grid, so two triangles of one plane gave depths that differ by more than the rank rule's
tolerance; and a card had no rank at all.

| case | what the reference did | now |
|---|---|---|
| **A flat thing larger than what it lies on**: a tile of 3 × 3 cells on the ground, a mat on one cell of a 2 × 2 table | Under a turned or perspective camera the ground showed through it in patches. Of the 3 × 3 tile it showed 59 % (iso, turned 217°, zoom 1.75) and 39 %, 68 %, 24 % and 52 % (four perspective cameras). | all of it, under every camera |
| **Two flat things of one rank in one plane** | which one showed was decided the same way | the first in the file |
| **The part of a card below its anchor, on a floor at the anchor's height** | In the scene's starting view the floor always won: a sprite nudged down 10 pixels lost those 10 rows. Under six of nine cameras it was a coin toss per ground triangle (39 % to 68 % shown). A sprite seen at another size than its own could lose its last row. | all of it shows, as ADR 0020 says |
| **The row of a card just above its anchor**, when the anchor is a hair above a pixel center | (after the first fix) a gap of one row across the card | the row shows |

**Fixed in the reference** (decided by the supervisor session, as a fix that makes the reference keep rules
the owner approved in ADR 0020; reported to the owner on 2026-11-26;
[note in ADR 0020](0020-render-list-camera-and-cpu-reference-renderer.md#note-2026-11-26-faces-in-one-plane-and-cards-on-a-floor)).
A level face takes its depth from its plane's equation (`Projector::level_planes`), so two faces at one
height have the same depth in every pixel, bit for bit, and the rank decides. The part of a card at and
below its anchor has that plane's depth and the highest rank. Five render tests fail on the old code. All
ten playtests keep their state and image hashes, and painting is no slower (161 to 164 µs against 164 to
166, measured in turns on the same machine). The GPU renderer reads the same plane equation; both cases are
in the gate's scenes.

### What stays undefined

Two things have the same depth, no rule of ADR 0020 says which shows, and each renderer's own rounding
decides. Random scenes keep both out by construction (the top of `players/gpu_check/src/scenes.rs` names
them), and `cargo xtask gpu --study` prints the first.

1. **Two faces that are not level and have the same depth.** The reference still takes a side's depth from
   its snapped corners.
   - *Two boxes in one cell*: their sides share a plane. With the lower box second in the file, the two
     renderers differ under five of nine cameras, in 21 to 8,092 pixels on Metal; lavapipe and WARP differ
     from the reference in other pixels again.
   - *Two boxes side by side, in the iso view, when the reference puts their shared upright edge exactly on
     a column of pixel centers*: along that column a side of each box has the same depth, and the
     reference showed the hidden one (random scene 596: 40 pixels on Metal, 52 on lavapipe, 28 on WARP).
     Turned a tenth of a degree, the scene is exact to 2 pixels on all of them. Where a slanted edge passes through a single pixel center the same tie is one pixel on
     an edge, inside the limit.

   Taking the depth of a box's sides from their plane too (`Projector::face_planes` gives it) decides
   these in the reference by where the faces really are: scene 596 then matches. **It changes one pixel of
   `examples/tiny_town/tests/refs/perspective.png`**, so it is not done here. It is the same defect and the
   same fix; whether to accept that pixel is a decision for the owner.
2. **Two cards of different entities on one upright line, at different heights**: a label on a box and the
   label of something standing in the box's cell. Both are upright in one plane, one above the other, and
   their depths are equal to the last bit or differ in it (8 pixels in random scene 4095). At one height
   they are the same arithmetic, and the first in the file shows. A rule would have to say which card is
   in front; none does.

## Consequences and limits

- `cargo xtask ci` covers what it covered. `cargo xtask gpu` is a second gate, for changes to `players/`,
  to `rwgpuengine_render` and to anything visual. On a machine with no adapter it fails with a message.
- Level faces and cards compute their depth in the fragment shader, and in the iso view the sides do too,
  so the GPU cannot reject them before shading. Measured: a frame of 50,000 boxes takes the same time with
  the rank as a depth bias (no per-pixel depth) as with the chosen design, within the noise; and the iso
  view takes 1.03 ms where perspective takes 0.97 (6.5 against 6.3 on the macOS runner).
- See-through things that overlap cost one copy of the frame and one pass each. The see-through scene
  takes 5 to 7 passes. A game with hundreds of overlapping see-through things would need a faster path that
  is not held to the reference; none is built.
- Cards are placed on the CPU every frame (two calls of `Projector::card` for a label). Instances are sent
  to the GPU only when they changed, so a camera moving over a still world uploads 1,744 bytes and the
  cards, and in the iso view 16 bytes per instance.
- The hand-made scenes were green twice while a real fault was there (the iso depth; then a card crossing
  a box top, and an upright edge one step off). Random scenes found each. After a change to either
  renderer, run a few thousand of them (`cargo xtask gpu --fast -- --random 3000`), not only the gate.
- A rank above 6 (a layer above 5) is treated as 6. A sprite larger than 32,767 pixels is not drawn
  exactly. Where a flat rectangle and the top of a box have the same rank in one plane, the rectangle shows
  (the reference shows whichever is first in the file).
- A level plane the camera sees edge-on, or within a thousandth of it, keeps its corners' depths in the
  reference; the GPU uses the equation there too. Such a plane covers a row of pixels at most.
- Windows has no room left in the proc-macro budget: `windows-implement` and `windows-interface` come with
  Direct3D 12. On Linux, where winit will add the most crates, 65 of 130 are used; the proposal counted 51
  more for winit.
- No GL backend: Vulkan, Metal, Direct3D 12 and WebGPU. Turning GL on adds 4 crates on Linux and Windows.

## Not done

A window, input, camera gestures, `rwgpu play`, the web player and its download budget, culling and levels
of detail, anti-aliasing (it would be presentation only and outside the comparison), a GL backend, the
sides of boxes by their plane's equation (it changes one pixel of a reference image), a rule for two cards
on one upright line.

## Revisit when

The reference gains a rule the fragment shader cannot compute exactly; a game needs many overlapping
see-through things; a wgpu upgrade changes the picture on an adapter (the check will say so); or the owner
decides about the one pixel above.
