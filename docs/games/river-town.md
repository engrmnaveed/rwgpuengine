# River Town — reference game 2 (spec)

> **Status: approved by the owner on 2026-11-28**, with the four choices below as recommended. Step 6 of the
> [3D proposal](../proposals/3d-architecture.md#7-order-of-work): "shapes as text and the town reference
> game". Source of truth for what the game is; `examples/river_town` will be the build. Original design.
> [Tiny Town](tiny-town.md) stays as it is.

## Decided by the owner

| # | choice | decided |
|---|---|---|
| T1 | **What the game is:** a planning builder for ages 10 and up, in three stages (below). | Yes. It is the seed of the city-builder template and of the strategy games in the direction. |
| T2 | **Buildings turn** in quarter turns, chosen with a Turn button. | Yes. Turning is part of the 3D world model the proposal fixed (4.3), and bridges, mills and every later vehicle need it. |
| T3 | **People and carts are not in step 6.** They come in step 7, written in Rust, in this same town. | Yes. Walking along roads needs path finding, which data rules cannot express (decision D4): it is step 7's test. One game grows over two steps. |
| T4 | **The name** "River Town" and the folder `examples/river_town`. | Yes, as a working title. |
| T5 | **Publishing:** add it to the Pages site next to Tiny Town. | Not decided: asked again at the end of step 6, with the page to look at. |

## In one sentence

A river valley where the player lays roads and builds houses, farms, a mill, a market and a town hall, stage
by stage, within a budget of coins, under rules that make the plan matter: everything needs a road, the mill
needs the river, and the far bank needs a bridge.

## What it has to prove (why this game)

1. **Shapes as text:** a town an agent can write, check and verify in a picture, with no art files.
2. **A map larger than the screen:** playable only with the camera, natively, in a browser and by touch.
3. **Rules of a builder beyond Tiny Town:** placement that depends on what is next to a cell.
4. **A cold agent can build it** from this page and the engine's own output.
5. **A stage for step 7:** roads and buildings that people will later walk between.

## The player's view (960 × 540, landscape)

| where | what |
|---|---|
| top left | `Coins: 40`, the chosen tool and its facing; under it the hint: why the last tap built nothing |
| top right | the charter: the stage's name and its job card, e.g. `Hamlet` / `Houses 2/4` / `Well 0/1` / `Farm 0/1` |
| middle | the valley, about 40 × 28 cells: grass, a river three cells wide, meadow on the far bank, rock |
| above the toolbar | the banner: what the stage asks; "The hamlet is ready!", then "River Town is a village!", at the end "River Town is a town!" |
| bottom | the toolbar: `Road 1`, `House 4`, `Well 3`, `Farm 8`, `Tree 1`, later `Mill 12`, `Market 15`, `Bridge 3`, `Workshop 8`, `Hall 25`; then `Turn` and `Clear` |

## Rules

1. Tap a button to choose a tool. `Turn` turns the tool a quarter turn; the toolbar shows the facing.
2. Tap a ground cell to build there. A building covers its footprint from the tapped cell, turned by the
   tool's facing: house 2 × 2, farm 3 × 3, workshop 2 × 3, mill 2 × 2, market 3 × 3, hall 3 × 3; road, well,
   tree and bridge one cell.
3. A building appears only if it fits (inside the valley, every cell free and of a terrain it may stand on)
   and the player can pay. In addition:
   - every building except a road, a tree and a bridge must **touch a road** (a cell beside its footprint);
   - the **mill** must touch the river;
   - a **bridge** stands only on the river, and must touch a road or another bridge;
   - the **hall** stands only on the meadow of the far bank.
   Otherwise nothing is built or paid, and the hint says why: "Build next to a road!", "The mill needs the
   river!", "A bridge goes on the river!", "A market needs 15 coins".
4. `Clear`, then a tap on a building, removes it and returns its coins.
5. **The charter has three stages.** Completing a stage's job card pays the next stage's coins, unlocks its
   tools and changes the banner. A stage, once reached, is kept.

| stage | job card (totals) | coins | unlocks |
|---|---|---|---|
| Hamlet | 4 houses, 1 well, 1 farm | 40 at the start | — |
| Village | 8 houses, a mill, a market, a bridge to the far bank | + 70 | Mill, Market, Bridge |
| Town | 16 houses, 3 farms, 2 workshops, the hall, 6 trees | + 110 | Workshop, Hall |

The prices and coins are starting values. The game piece tunes them so the charter can be completed with a
little to spare, and the `town` playtest proves it. There is no losing and no timer.

## Controls

- **Build:** tap (a click). No dragging to build.
- **Camera:** drag pans, pinch or wheel zooms, two buttons turn the view a quarter turn, one switches between
  the isometric and the perspective view. Every camera move must be reachable by touch alone; keys as in
  `rwgpu help play`. The camera's center stays over the valley.

## The buildings as shapes

Each is a prefab with a `[[shape]]` list (proposal 4.5), a few flat colors, no textures.

| prefab | shapes |
|---|---|
| road | a flat box, one cell |
| tree | cylinder (trunk), cone (crown) |
| well | cylinder (wall), a small roof on two boxes |
| house | box, gable roof, box (chimney) |
| farm | box and gable roof (barn), flat boxes in rows (the field) |
| mill | box, gable roof, cylinder and cone (the tower) |
| market | flat box (floor), four boxes (posts), wedges (awnings) |
| workshop | box, wedge (lean-to roof), cylinder (chimney) |
| bridge | flat box (deck), two wedges (ramps), boxes (rails) |
| hall | box, gable roof, box and cone (the tower with its spire) |

**The shape set this pulls: box, roof, wedge, cylinder, cone.** The proposal also named a prism and an
extruded outline; this town does not need them, so they are not built in step 6.

## What the engine has, and what this game pulls

| already there | used for |
|---|---|
| ground maps with terrains, ground taps, `spawn` with `footprint` and `cost`, `refused` with reasons | building, paying, the hints |
| props, `when` with `at_least`, `actions` lists, `counts`, `enable` / `disable` | the charter's stages, unlocking tools, the job card, Clear |
| `label`, `screen_pos` | the HUD and the toolbar |
| the camera as state, `[[input]] camera`, both players' camera input | a map larger than the screen; playtests that tap after a turn |

| new, pulled by this game | why it cannot be done without |
|---|---|
| **`[[shape]]` in prefabs**: box, roof, wedge, cylinder, cone; drawn by the reference renderer first, then by wgpu, named in the frame report, checked by `rwgpu check` | the point of step 6 |
| **`turn`** on an entity, in quarter turns, and on `spawn` from a prop (the tool's facing); footprints turn with it | T2: bridges across a river in either direction, a mill facing the water, a street of houses facing the road |
| **placement by neighbours and terrain**, on a prefab: `place = { on = ["river"], touching = ["road", "bridge"] }`, with a refusal reason for each | rules 3: without it a builder has no planning in it |
| **the camera stays over the ground**, and every camera move by touch | a player on a tablet must not lose the town or be unable to turn it |

Not pulled, so not built: terrain height, curved roads, lights and shadows beyond today's shading, mesh
import, sound, saving a town, income over time (a timer is outside the data rules: step 7).

## Playtests (all must pass in CI)

- `hamlet`: stage 1 from the first tap until its card is complete, the Village's coins are paid and its
  tools are unlocked; a reference image.
- `needs`: every refusal of rule 3 and its hint: no road beside it, the mill away from the river, a bridge on
  grass, the hall on the near bank, too few coins. Nothing is built or paid.
- `village`: the bridge and the mill, with a pan and a quarter turn of the camera before the taps on the far
  bank; a reference image in perspective.
- `turn_and_clear`: a workshop built in both directions, cleared, its coins returned, built again.
- `town`: the whole charter, to "River Town is a town!"; reference images in both views. This is also the
  scene the budgets measure.

## Done when

- The five playtests pass on the three systems, and both players show the town within the pixel limit.
- `cargo xtask gpu` holds every shape to the reference under an isometric and a perspective camera, with
  random scenes of shapes.
- Measured on the finished town and written into the budgets' report: loading and checking, one step, a
  reference frame at 960 × 540, a GPU frame, the exported folder's size. No budget limit is raised.
- **Cold-agent run 10:** a fresh agent, an empty folder, `rwgpu new` and this page from "In one sentence" to
  "Controls". It passes if every rule is supported, `rwgpu check` is clean, its own playtests pass and its
  picture reads as a town. Time, tool calls, connections and what it stumbled on are reported as before.

## Pieces and agents

| piece | what | agent |
|---|---|---|
| 6.1 | shapes as text and quarter turns, through the reference renderer, wgpu, the check and the frame report; the ten prefabs above as a catalog scene with reference images | **Fable 5.2 Extra** (needs the owner's yes) |
| 6.2 | the camera over the ground, and every camera move by touch | Opus 5.5 Extra, beside 6.1 (other files) |
| 6.3 | placement by neighbours and terrain | Opus 5.5 Extra, after 6.1 (turned footprints) |
| 6.4 | the game, its map, its five playtests, the measurements | Opus 5.5 Extra |
| 6.5 | cold-agent run 10 and what it found | Opus 5.5 Extra |
