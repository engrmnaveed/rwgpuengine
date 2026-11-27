# ADR 0029: Shapes as text, and entities that turn in quarter turns

- Status: accepted and built, 2026-11-28 (step 6 of the [3D proposal](../proposals/3d-architecture.md),
  piece 1; pulled by [River Town](../games/river-town.md))
- Decided by: the step's brief and the game's spec (five shapes, quarter turns, the reference first). The
  choices the brief left open were measured; the tables are here. Every file that loaded before loads
  unchanged and draws the same pixels: every example playtest that existed keeps its state and image
  hashes (eleven, with the one piece 6.2 added), both oracles of the simulation hold, and no budget limit
  moved. Two codes are new (W0208, W0209) and one
  limit (64 shapes in a list, E0209).
- Builds on: [ADR 0003](0003-toml-and-schema-registry.md) (one registry of formats),
  [ADR 0005](0005-determinism-model.md) (exact arithmetic),
  [ADR 0020](0020-render-list-camera-and-cpu-reference-renderer.md) (the render list and the reference),
  [ADR 0021](0021-wgpu-renderer-held-to-the-reference.md) (the GPU held to the reference),
  [ADR 0028](0028-loader-as-a-pipeline-linear-and-limits-on-sizes.md) (the loader and its oracle).

## Context

Everything solid was a box (`block`) or a flat rectangle (`tile`), and nothing could turn. River Town's
ten buildings need roofs, awnings, round towers and spires, a bridge that crosses a river in either
direction and a street of houses that face the road. The proposal (4.5) said: a prefab lists shapes, the
engine builds one mesh per prefab and draws its copies as instances. The brief asked for that to be
measured against instances of unit shapes, and for the roundness of a cylinder and the shading rule to be
chosen by measurement too.

## Decision

### 1. The format: a `[[shape]]` list

```toml
# prefabs/house.toml
footprint = [2, 2]

[[shape]]                  # the walls
kind = "box"
at = [0.15, 0.15, 0]
size = [1.7, 1.7, 1.1]
color = "#e8d8b0"

[[shape]]                  # a gable roof on them; its ridge runs along x
kind = "roof"
at = [0.05, 0.05, 1.1]
size = [1.9, 1.9, 0.85]
color = "#b5523b"
```

| key | | |
|---|---|---|
| `kind` | required | `box`, `roof` (a gable roof), `wedge` (a ramp, a lean-to roof), `cylinder`, `cone` |
| `size` | required | the box the solid fills, along x, y and z, in cells; each above 0 and at most 64 |
| `at` | `[0, 0, 0]` | the box's lowest corner, from the entity's back corner at its height |
| `color` | `"#9aa0a6"` | flat and opaque (a see-through color is E0105) |
| `turn` | `0` | 0, 90, 180 or 270: the solid turned inside its box. A roof's ridge runs along y at 90; a wedge rises toward +x, +y, −x, −y |

Five keys, five kinds. A list is written in a prefab file or on an `[[entity]]`; an entity's own list
replaces its prefab's (lists are not merged). Every entity made from a prefab is drawn from the prefab's
one list: the loader decodes it once and the copies share it, and the world holds each list once.

**`block` and `tile` stay as they are.** They are the two simplest shapes in meaning, a box and a flat
rectangle that fill the footprint, and they keep their keys, their two colors and their drawing path, so
no existing file changes by a byte or a pixel. A box shape that fills a footprint is drawn in exactly a
block's tones (a test compares the two pictures). An entity may have a tile, a block and shapes together;
shapes are counted from the entity's own height, not from the top of its block. An entity with shapes
takes space as one with a block does, and a sprite or a label stands on top of the tallest of them.

### 2. One mesh per prefab

The render list gained `models`: one `Model` for each (shape list, footprint, turn) that is drawn, and
`Mesh::Model(n)` on an instance. An entity with shapes is **one instance**, whatever its list holds.
`rwgpuengine_render::model::bake` turns a model into flat faces, each with its final color under the
light. Both renderers draw the baked faces:

- **The reference** bakes the list's models when it projects a frame, and draws each instance's faces: a
  level face in its plane with the rank of a box's top, every other face by the depth of its corners. Each
  shape is its own item, so the frame report and picking know which shape a pixel belongs to.
- **The GPU** (`pack.rs`, `shader.wgsl`) uploads each model's faces once as a mesh of 20-byte corners
  (where the corner is, its face's color, the rank of a level face, its corner column) and draws the
  instances of a model in one instanced draw for its level faces and one for the rest. It does no color
  arithmetic: the colors in the mesh are the ones `bake` computed. In the iso view the corner columns come
  from the CPU, as for boxes, through one storage buffer read in the vertex stage
  (`Limits::downlevel_defaults()` is still enough).

The other design, **instances of unit shapes** (an instance for every shape of every entity, five fixed
meshes), was built as a working prototype and measured ([Options](#options-that-were-measured)).

### 3. The shading rule

A face's color is its shape's `color` at the tone `clamp(base + n · toward)`, where `n` is the face's unit
normal: the cross product of two of its edges divided by its length (+, −, ×, ÷ and one square root, all
exact under ADR 0005). It is the rule boxes already had (`Light::shade`), given the true normal of a
slanted face. The tone is applied per channel in integers, as for a block. Nothing else lights a face: no
shadows, no smoothing across the sides of a round shape.

### 4. Round shapes have 16 sides

A cylinder and a cone are 16 flat sides (`ROUND_SIDES`), their ring from the engine's own sine and cosine.

### 5. Turning

`turn = 90` on an entity or in its prefab: 0, 90, 180 or 270, from +x toward +y, the way a camera turned
as far shows the world.

- **The footprint turns too**: `footprint = [2, 3]` covers 3 cells along x and 2 along y at 90 and 270.
  The world holds the cells as covered, so blocking, triggers, taps, the cell index and placement need no
  rule of their own.
- **`pos` names the covered cell with the lowest x and y**, at every turn. A building covers its cells
  from the same cell however it is turned, and a tap on a cell places a turned copy on that cell.
- **Shapes turn about the middle of the covered cells**, and are written for the entity at turn 0. With a
  footprint of w by h, a point (x, y) goes to (h − y, x) at 90, (w − x, h − y) at 180 and (y, w − x) at
  270: whole and half cells stay exact, so a turned building is its shapes written the other way round,
  bit for bit (a test draws both).
- **A `spawn` turns its copy**: `turn = 90`, or `turn = { entity = "tool", prop = "facing" }`, read when
  the copy is placed. The prop is an integer in degrees; whole quarter turns count and the number wraps, so
  a Turn button is one `add` of 90. The key has the form of `cost`. The turned footprint is what is checked
  against the ground, the terrain and what stands there; `rwgpu inspect --place workshop@3,4,90` answers
  for a turned copy. A `spawned` event says the turn when it is not 0.
- **The state hash** includes an entity's shape list and its turn only when it has them, and the table of
  lists only when there is one, so a world without shapes or turns hashes as it did.

### 6. Checked at load

Every key is in the schema registry (`SHAPE`, `TURN_FROM`; `shape` and `turn` on `[[entity]]` and prefab
files; `turn` on a spawn).

| | |
|---|---|
| **W0208** shape-outside-footprint | a shape reaches past the cells its entity covers, along x or y. Placement and taps go by the footprint; `allow = ["W0208"]` for eaves that are meant |
| **W0209** shapes-in-one-plane | two shapes of a list each have an upright side in one plane, facing the same way, where the two overlap (a chimney flush with a wall). No rule says which shows ([undefined](#what-stays-undefined)) |
| **E0209** too-large | a list of more than 64 `[[shape]]` tables (`limits::SHAPES_IN_A_LIST`). The longest list of River Town's ten buildings has 9 |
| E0105 | a see-through shape color; a `turn` that is not a quarter turn; a size of 0 or above 64 |
| E0202, E0206, E0207 | a spawn's `turn` names a missing entity, a prop its entity does not declare, or a prop that is not an integer |

A prefab's list is checked once, in the prefab's file; an entity that takes its prefab's list and sets its
own `footprint` is checked against that. A list with a problem the schema found gets no follow-up warnings.
61 cases joined the loader's oracle (535 in all). Five older pins changed, and only by the names `shape`
and `turn` in the "valid keys" hint of an unknown key: a script compared every line of every record of
before with after.

### 7. The frame report

`rwgpu shot --json` lists an entity's shapes, one by one, with the pixels each drew and shows
(`shapes: [{ kind, drawn_px, visible_px }]`), and its `turn` when it has one; `rwgpu inspect` prints the
list under `shape`. A tap on any shape hits its entity.

## Options that were measured

Apple M5 Pro, release builds. `cargo xtask shape-bench` and `cargo xtask gpu` print the numbers of the
built design again; the prototype and the study switches are patches outside the tree. The options were
measured before piece 6.2 ([ADR 0030](0030-camera-over-the-ground-and-by-touch.md)) landed under this
one: the gate had 382 scenes then (383 with that piece's picture) and the sizes are those of that base.

### One mesh per prefab, or instances of unit shapes

The town is `xtask/src/town.rs::shaped_town`: copies of six prefabs (2 to 9 shapes, two of them round), a
building every three cells, each at one of the four turns, at 1280 × 720 in perspective. The ground is an
instance a cell in both.

| reference renderer | buildings | shapes | instances | list bytes | list µs | frame µs | paint µs |
|---|---|---|---|---|---|---|---|
| **a mesh per prefab** | 1,000 | 4,650 | 10,216 | 658,176 | 52 | 4,875 | 4,648 |
| unit shapes | | | 13,866 | 887,424 | 77 | 5,433 | 5,188 |
| **a mesh per prefab** | 5,000 | 23,237 | 50,369 | 3,227,968 | 134 | 13,291 | 13,138 |
| unit shapes | | | 68,606 | 4,390,784 | 385 | 16,429 | 16,279 |
| **a mesh per prefab** | 50,000 | 233,472 | 501,584 | 32,105,728 | 2,800 | 77,102 | 81,241 |
| unit shapes | | | 685,056 | 43,843,584 | 3,797 | 102,455 | 102,866 |

Baking the town's 24 models takes 26 µs a frame. An instance is 64 bytes, a model 32, a shape 32.

| GPU (Metal), ms a frame | buildings | nothing uploaded | every instance uploaded again | iso view |
|---|---|---|---|---|
| **a mesh per prefab** | 5,000 | 0.725 | 1.069 | 0.733 |
| unit shapes | | 0.775 | 1.341 | 0.803 |
| **a mesh per prefab** | 50,000 | 5.233 | 10.105 | 5.037 |
| unit shapes | | 6.038 | 15.389 | 5.927 |

| | a mesh per prefab | unit shapes |
|---|---|---|
| web module, bytes | 733,803 | 734,649 |
| players' web download, bytes | 859,009 | 856,567 |
| the gate's 382 scenes: failed, most apart in one, apart in all | 0, 7, 54 | 0, 11, 64 |
| its 109 scenes of shapes: apart in all, most in one | 20, 2 | 30, 11 |

The two designs draw River Town's catalog to the same reference pixels. **A mesh per prefab was kept:** the
list is a quarter smaller and built 1.4 to 2.9 times faster, the reference frame is 10 to 25 % faster, the
GPU frame 6 to 15 % faster when nothing changed and 20 to 34 % when every instance is uploaded again, and
the sizes are within 2,500 bytes of each other. It is also the design in which the GPU computes nothing
that decides a pixel: the prototype's vertex shader computed each face's tone, which agreed on Metal and
is the arithmetic ADR 0021 took a table to avoid. What it costs: a model for each turn of a prefab that is
on the map (four for a prefab seen at every turn), and 26 to 50 µs a frame to bake them in the reference,
which keeps nothing between frames.

### How round

The catalog's pictures (960 × 540) with 8 to 48 sides, counted against the same pictures with 128 sides;
the reference frame of the town of 5,000 buildings; the GPU frame of 50,000 with nothing uploaded.

| sides | iso, zoom 0.6 | iso, zoom 1.5 | iso, zoom 3 | perspective | reference frame, ms | faces of the town's 24 models | GPU frame, ms |
|---|---|---|---|---|---|---|---|
| 8 | 223 | 786 | 303 | 299 | 11.8 | 712 | 4.84 |
| 12 | 191 | 596 | 196 | 141 | 12.3 | 792 | 5.06 |
| **16** | **51** | **171** | **73** | **85** | **13.0** | **872** | **5.22** |
| 20 | 100 | 214 | 90 | 41 | 13.6 | 952 | |
| 24 | 25 | 58 | 31 | 31 | 16.2 to 17.1 | 1,032 | 5.49 |
| 32 | 17 | 38 | 22 | 13 | 18.9 | 1,192 | 5.80 |
| 48 | 12 | 25 | 9 | 5 | 22.5 | 1,512 | |

The numbers are pixels that differ from the picture with 128 sides, over the whole catalog (its round
shapes at the four turns). From 12 to 16 sides the outline gains 1.7 to 3.7 times for 0.7 ms; from 16 to 24 it gains
2 to 3 times for 3 to 4 ms of a 13 ms frame. A multiple of 8 puts corners where the iso view's outline
turns, which is why 20 is worse than 16 in three views of four. The two renderers agree the same at every
count (111 scenes of shapes, 0 failed, at most 2 pixels apart). **16** was chosen: at the catalog's own
zoom the whole picture is 51 pixels from round.

### The shading rule: its cost, and that it is exact

Baking River Town's catalog (40 models, 1,356 faces), and the reference frame:

| rule | bake, µs | a face, ns | frame, µs |
|---|---|---|---|
| **`base + n · toward`, the true normal** | 41.8 to 49.5 | 31 to 36 | 663 to 676 |
| no tone at all (every face its color) | 38.2 | 28 | 662 |
| the tone of the nearest of a box's six faces | 47.2 | 35 | 669 |

The rule costs 3 to 8 ns a face over no shading at all, 4 to 11 µs of a 670 µs frame, and nothing on the
GPU, which is handed colors. Without a tone a roof's two sides are one color and a round tower is a flat
patch. The tone of the nearest box face is no cheaper and gives a round tower only the tones a box has.

**Exact:** the catalog reaches state `7fbfa1ee06b8ff42` and image hashes `9a5d2966c050fd5e` (iso) and
`0f455bb09d9ad0a8` (perspective) on aarch64 natively, on x86-64 (the same Mac under Rosetta) and on wasm32
(`cargo xtask ci` compares the web module's pixels with `rwgpu shot`'s). The pull request's checks run it
on Linux, Windows and macOS.

## Evidence

### The check

`players/gpu_check/src/scenes.rs` gained: a scene with every kind at every turn, flat, tall, in the air,
disabled, with tiles, sprites and labels on them and entities turned 90, 180 and 270, under five iso and
four perspective cameras; 100 random scenes of shapes on every run (half of their entities turned), and
`--random N` now draws N of each kind; River Town's two catalog pictures. The limit is unchanged: no color
tolerance, a differing pixel must lie on an edge of the reference, at most 24 in a scene.

| | scenes | failed | most apart in one | apart in all |
|---|---|---|---|---|
| the gate before step 6 | 271 | 0 | 7 | 34 |
| the gate now (one picture is piece 6.2's) | 383 | 0 | 7 | 54 |
| the nine pictures of the shapes scene | 9 | 0 | 2 | 3 |
| River Town's catalog, both views | 2 | 0 | 0 | 0 |
| `--random 3000`: 3,000 scenes of the older kind | 3,000 | 0 | 13 | 262 |
| `--random 3000`: 3,000 scenes of shapes | 3,000 | 0 | 11 | 561 |

Of the 3,000 random scenes of shapes 2,567 are identical, 340 have one pixel apart, 70 two, 19 three, 2
four, 1 five and 1 eleven; none has a pixel off an edge. The catalog is 0 pixels apart natively and in
Chrome, on WebGPU and on the CPU fallback, and the window player and both web paths reach its state.
The numbers are those of the last commit, with piece 6.2 under it.

### Sizes and budgets

Against `river-town` with piece 6.2 on it, which is what this piece is built on:

| | before | now | this piece | limit |
|---|---|---|---|---|
| web module | 706,544 | 735,503 | +28,959 | 800,000 |
| `rwgpu` | 2,334,576 | 2,368,560 | +33,984 | 3,000,000 |
| players' web download | 838,391 | 872,382 (339,880 gzipped) | +33,991 | 1,500,000 |

The web module grew by 4.1 % and stands at 91.9 % of its limit; 64,497 bytes are left. By part, as each
commit was measured before piece 6.2 landed (the base was 704,851, 2,334,496 and 824,958 bytes then):

| | the format and its checks | the world and the reference | the GPU | turning | all |
|---|---|---|---|---|---|
| web module | +12,354 | +12,366 | 0 | +4,232 | +28,952 |
| `rwgpu` | +432 | +16,912 | 0 | +16,640 | +33,984 |
| players' web download | +8,128 | +11,164 | +10,978 | +3,781 | +34,051 |

The first version of the format alone cost 19,406 bytes in the web module. Comparing the sizes of each
function in two builds found where: a map of a new value type costs 3,500 to 5,000 bytes of generated code
(the prefab cache is now a map the loader already had, onto a list), and `bake` written with lists of lists
was 8,600 bytes (static tables: 3,500).

| budget | before step 6 | now | limit |
|---|---|---|---|
| `time.render_frame_us` | 1,466 | 1,397 | 6,000 |
| `time.paint_frame_us` | 160 | 156 | 200 |
| `time.reference_town_frame_us` | 5,015 | 4,674 | 20,000 |
| `time.sim_step_us` | 1,350 | 1,094 | 2,000 |
| `time.load_check_ms` | 80 | 76 | 250 |
| `players.gpu_frame_us` | 1,052 | 994 | 4,000 |
| `players.pixels_apart` | 7 | 7 | 24 |
| `players.crates`, `players.proc_macro_crates` | 105, 6 | 105, 6 | 130, 6 |
| `deps.runtime_crates`, `deps.proc_macro_crates` | 16, 0 | 16, 0 | 20, 0 |

No limit was raised and no row was added. The differences in time are run to run.

### The catalog

`examples/river_town/`: the ten prefabs of the spec (road, tree, well, house, farm, mill, market, workshop,
bridge, hall; 1 to 9 shapes), `scenes/catalog.toml` with each at the four turns on a street of its own,
and a playtest that pins the picture in each view (`tests/refs/catalog_iso.png`,
`tests/refs/catalog_perspective.png`) with no warning in the frame report. It is the catalog, not the game.

## What stays undefined

ADR 0021 left one thing to rounding: two faces that are not level, in one plane. Shapes make it easy to
write, so it is now said at load.

- **Two upright sides of one list in one plane** (W0209): the reference takes such a face's depth from its
  snapped corners, so which shows can change with the camera and between the renderers. The fix is to set
  one shape in by a little. The check's random scenes never write it.
- **Two slanted faces in one plane** (two roofs of one slope that overlap) are the same case and are not
  warned about: no building of the catalog has it, and finding it takes more than comparing boxes.
- **An edge between two sides of a cone through pixel centers.** Random scene 615 of shapes is 11 pixels
  apart: in that perspective view the edge between two sides of a cone passes through a pixel center every
  second row, and the two renderers round it to different sides. All 11 lie on that edge, inside the limit.
  It is ADR 0021's single-pixel tie on a slanted edge, eleven times on one edge.

## Consequences and limits

- A building is cheap to copy: one instance, and its faces once per model. A map with every prefab at
  every turn has four models per prefab.
- Shapes are flat-colored and flat-shaded. A round tower shows its 16 sides at a high zoom.
- A shape's `turn` and an entity's `turn` are quarter turns. Nothing turns freely.
- W0208 reads the footprint, not the neighbours: eaves that are allowed to reach out are drawn into
  whatever stands next door.
- The reference bakes the models of a frame on every frame (26 to 50 µs). A map with thousands of
  different shape lists would pay more; a prefab's copies share one.

## Not done

- Placement by neighbours and terrain, the camera's bounds and touch input (piece 6.2), the game itself.
- No budget row for a town of shapes: the numbers above are printed by `shape-bench` and the GPU gate.
- A free angle, people and carts, lighting beyond the tone of a face, mesh import, a prism and an
  extruded outline (the proposal named them; this town does not need them).
- A warning for two slanted faces in one plane.

## Revisit when

- A game turns something by a free angle (vehicles): models keyed by quarter turns will not do.
- A map holds thousands of different shape lists: bake once per list instead of once per frame.
- Round shapes are looked at closely (a zoom above 3): 24 sides cost 3 to 4 ms more on a town of 5,000.
- The web module nears its limit: shapes took 28,959 of the 93,456 bytes that were free.
