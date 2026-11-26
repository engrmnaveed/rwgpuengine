# ADR 0020: The world is drawn in 3D — the render list, the camera as state, a CPU reference renderer

- Status: accepted and built, 2026-11-26 (step 2 of the [3D proposal](../proposals/3d-architecture.md))
- Decided by: the owner approved the proposal's two halves (a GPU-free agent loop whose CPU reference
  renderer is the source of truth for checks) and said to start this step. The shape of each piece was
  chosen by measurement; the options are in the table below.
- Replaces, for the world: the 2D draw list of [ADR 0007](0007-draw-list-and-cpu-rasterizer.md) and the flat
  paint path of [ADR 0018](0018-paint-path-for-players.md). Their ideas stand: one list that every consumer
  reads, and a player that repaints only what changed.

## Context

The engine's target is 3D on the GPU. Before any GPU code, the half an agent works with has to be 3D: what
`rwgpu shot` draws, what the frame report measures and what a tap hits. The step was done when Tiny Town
played in 3D from its unchanged files, with unchanged state hashes.

One fact shaped the design. A playtest taps **screen pixels** (`tap = [384, 192]`), so "unchanged files,
unchanged state" means the camera a scene starts with must put every ground cell on exactly the pixels the
2.5D formula did.

## Decision

1. **The render list** (`rwgpuengine_render::list`) is what every renderer reads: the view, the camera, one
   light, and three kinds of things.
   - An **instance** is solid: a box, or a flat rectangle lying on a surface. Ground cells, `tile` and
     `block` are the first instances.
   - A **card** is a flat picture that faces the camera and stands at a world point: a world entity's
     `sprite` or `label`.
   - The **2D layer** is the screen entities (`screen_pos`), drawn last, ignoring the camera.

   Nothing in it depends on a GPU. Instances have no turn yet, because no entity can turn yet.
2. **The camera is session state**: `view` (`iso` or `perspective`), `center`, `turn`, `zoom`. A scene's
   `[camera]` sets where it starts; a playtest's `[[input]] camera = { turn = 90 }` (also `zoom`, `pan`,
   `view`) moves it, and a player's gesture calls `Session::move_camera`. A move at frame N happens before
   that frame's taps. The camera is not in the world, so not in the state hash; `inspect` and `run` report it.
   - `iso` is the isometric view of `[iso]`. With no turn and zoom 1 it gives the 2.5D formula's pixels bit
     for bit (the offset is snapped to 1/256 of a pixel, which removes the rounding a center brings in).
   - `perspective` is 30° above the horizon with a 45° field of view, placed so that `center` is drawn at
     the iso view's size.
   - Turning needs sine and cosine. `rwgpuengine_core::math` owns them: exact operations in a fixed order,
     exact at quarter turns, pinned bits (ADR 0005).
3. **The CPU reference renderer** (`rwgpuengine_render::reference`) draws the list in two stages. `project`
   takes it to the screen: corners in double precision, snapped to a 1/256 pixel grid, a box's corners
   projected once so its faces meet without gaps, geometry cut at the camera's range. `render` fills pixels
   through a depth buffer with exact integer coverage (the top-left rule of ADR 0007), solving each
   triangle row's run of pixels, and records which item owns each pixel. The light is
   `base + normal · toward`: a top keeps its color, and the four sides get 0.8, 0.65, 0.5 and 0.35.
4. **Three rules go beyond "the nearest wins".**
   - *Level faces in one plane.* A tile lies in the plane of the ground or of a box's top; depth cannot
     order them. Level faces carry a rank (ground and box tops 0, tile 1, block of height 0: 2), and where
     two are at the same depth the higher rank shows. Sides are ordered by depth alone.
   - *Cards have a place.* Above its anchor a card stands upright, so higher rows are nearer a camera
     looking down; below its anchor it lies on the level surface toward the viewer. A label is one small
     step in front of its own sprite. A card of an entity with a block stands on top of the block. A sprite
     grows and shrinks with the camera; text keeps its pixel size.
   - *See-through things* are drawn after the solid ones, far to near, and hide nothing drawn later.
5. **The frame report and picking read the same projection.** A covered pixel is credited to the entity that
   shows instead. `pick` replays the renderer's rules at one pixel; `tap` points come from the owner buffer.
6. **Players paint with a `Painter`**: the same drawing without owners. The `Presenter` compares render
   lists, so a moved camera is a new picture and a still frame costs nothing.

## Options that were measured

| question | option | result |
|---|---|---|
| Flat things on surfaces | draw them 1/64 unit nearer | a line of tile color along the foot of a crate standing on a plate (seen in `coin_vault`) |
| | draw them 1/1024 unit nearer | 2 wrong pixels under one of seven test cameras |
| | **rank for level faces** | **0 wrong pixels under all seven** |
| A card's depth | the anchor's depth everywhere | a tall sprite in front of a wall lost 660 of 3,200 pixels to the wall behind it |
| | **upright above the anchor, lying below it** | **0 lost** |
| Filling a triangle | test every pixel of the bounding box | Tiny Town frame painted in 263 µs (limit 200) |
| | solve each row's run | 210 µs |
| | **solve the run and write through row slices** | **161 µs** |
| A 2D renderer beside the 3D one | keep both | not built: two renderers held equal by tests is what the audit removed |

## Evidence

- **State:** all nine existing playtests end in the hashes they had before (`82c150f3d192812e` for
  `build_town`). No example file changed.
- **Pictures against the old references:** Tiny Town differs in 9,632 and 832 pixels, each by one color
  level (a side's shade is now rounded, not cut off). `iso_garden` differs in 7,648 and `coin_vault` in
  4,352; 128 and 288 of those by more than one level, every one of them on the edge of the top of a box
  0.8 or 1.2 high. Those heights are 25.6 and 38.4 pixels, and the tops are now drawn there, not rounded to
  a whole pixel. All six reference images were looked at; two are new (`turned.png`, `perspective.png`).
- **A new playtest, `turn_and_build`,** builds a house, a tree and a well from three cameras (a quarter turn,
  perspective zoomed, then half round and panned), with tap points taken from `inspect --place`.
- **Native equals web:** under Node the wasm module reaches the native state and image hashes for all ten
  playtests, turned and perspective frames included. So the owned sine and cosine and the renderer give the
  same bits on aarch64 and wasm32.
- **Picking equals the picture:** on 19,200 pixels of each of 14 scene and camera pairs, a tap gets the
  entity or ground cell that owns the pixel. A triangle's fill matches the edge rule on 4,000 random
  triangles, and a pixel's depth is the same bits whatever the clip.

| budget | before | now | limit |
|---|---|---|---|
| reference frame with report, Tiny Town 640 × 480 | 2,225 µs | 1,587 µs | 6,000 |
| paint for a player, same frame | 59 µs | 161 µs | 200 |
| reference frame, 5,000 boxes at 1280 × 720 (new budget) | — | 5.2 ms | 20 ms |
| web module | 585,789 bytes | 619,742 | 800,000 |
| command | 1,912,336 bytes | 1,979,088 | 3,000,000 |

## Consequences and limits

- Painting for a player costs 2.7 times what the flat painter did, because every pixel goes through a depth
  test. It is within the unchanged limit, with 1.25× to spare on the reference machine where there was 3.4×.
- The frame report of the 5,000-box town takes about 14 ms on top of the 5 ms frame. It is not budgeted.
- The ground is one instance per cell (5,041 for that town). Merging it belongs to the scale step.
- The camera cannot tilt, and entities cannot turn. Nothing needs either yet.
- A sprite wider than about 0.7 of a cell reaches over neighboring cells and is hidden by boxes there. The
  part of a card below its anchor lies in the anchor's level plane even where the ground drops away.
- A GPU renderer has to reproduce the rank rule (a depth bias does it) and the cards' two planes. How close
  it must come is decided at step 3.
- Not done: camera gestures in the web player, a command-line flag to move the camera for one `shot`, and an
  `[[expect]]` on the camera.

## Note, 2026-11-26: faces in one plane, and cards on a floor

The GPU check of step 3 ([ADR 0021](0021-wgpu-renderer-held-to-the-reference.md)) found that the renderer did
not keep two rules of decision 4 in every case. The fix was decided by the supervisor session, as a fix that
makes the reference keep rules the owner approved here; it was reported to the owner on 2026-11-26.

**What was wrong.** A face's depth came from its triangle's corners, and corners are snapped to the 1/256
pixel grid. Two triangles of one plane with different corners then gave depths that differ by more than the
rank rule's tolerance, and a card had no rank. The table above measured "rank for level faces: 0 wrong pixels"
with tiles of one cell, whose triangles are the ground's own.

- A tile of 3 × 3 cells showed only 24 % to 68 % of its pixels under five of nine cameras; the ground showed
  through the rest.
- The part of a card below its anchor always lost to the floor in the scene's starting view (a sprite nudged
  down 10 pixels lost those rows), and under most other cameras it was a coin toss.

**What the reference guarantees now.**

- **A level face takes its depth from its plane's equation** (`Projector::level_planes`), not from its
  corners. Two faces at one height have the same depth in every pixel, bit for bit, whatever their corners
  are. The higher rank shows; of equal ranks the first in the list stays. The height is a single-precision
  number, as scene files hold it, so a tile at 1.2 and the top of a box from 0.4 to 1.2 are one plane.
- **The part of a card at and below its anchor has the depth of the level plane through the anchor and the
  highest rank.** It shows over any floor in that plane, and anything really in front still covers it. In the
  one row just above the anchor, where a card is upright by less than a depth can hold, it lies on the plane
  too if it is not nearer than the plane: a card never falls behind the floor it stands on. A lifted card (a
  label) keeps its own depth, one step in front.
- A plane the camera sees edge-on, or within a thousandth of it, keeps its corners' depths.
- Picking and the frame report read the same depth and rank.

**Measured.** Five render tests fail on the old code and pass now (flat things larger than what they lie on;
of two tiles in one plane the first stays; sprites nudged down, on the ground, on a tile and on their own box;
a sprite's last row at any size; the row just above an anchor). All ten playtests keep their state and image
hashes, natively and in the wasm module, and no reference image changed. Render, paint and town time,
measured in turns with the old code on one machine: 1,513–1,532 / 161–164 / 5,071–5,389 µs now against
1,521–1,547 / 164–166 / 5,215–5,592 (limits 6,000 / 200 / 20,000). The web module grew from 619,866 to
623,739 bytes (limit 800,000). Against the GPU renderer the same 256 scenes went from 33 to 18 pixels apart
on Metal.

**What stays undefined.** Sides still take their depth from their corners. Where two sides of different
boxes have the same depth (two boxes in one cell; or two boxes side by side whose shared upright edge falls
exactly on a column of pixel centers in the iso view) the snapping of their corners decides which shows.
Taking a side's depth from its plane as well decides these by where the faces are; it changes one pixel of
`examples/tiny_town/tests/refs/perspective.png` and is not done. Two cards of different entities on one
upright line at different heights are equal in depth too, and no rule says which is in front. ADR 0021 has
both with numbers.

## Revisit when

A GPU renderer cannot keep one of the three rules within the pixel limit, or a reference game needs a tilting
camera, turning entities or shadows.
