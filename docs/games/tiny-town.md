# Tiny Town — reference game 1 (one-page spec)

> Decided in [ADR 0013](../decisions/0013-first-reference-game-isometric-builder.md) (Q3). Source of truth for
> what the game is; `examples/tiny_town` is the build. Original design.

## In one sentence

An isometric island where a young child (5–8) taps a tool, then taps the ground to build a small town within
a coin budget: counting, adding and spending on a tablet.

## What the player learns

- **Counting and adding up costs**: a house costs 3 coins, a tree 1, a well 2. The job (2 houses, 3 trees, a
  well) costs 11 of the 12 coins, so careless spending runs out.
- **Cause and effect with shapes**: a house covers 2 × 2 cells, so it only fits where four grass cells are free.
- **Undo is allowed**: "Clear" takes a building back and returns its coins.

## Screen (640 × 480, landscape tablet)

| where | what |
|---|---|
| top left | coins left and the chosen tool: `Coins: 12` / `Tool: house` |
| top right | the job card: `Houses 0/2`, `Trees 0/3`, `Wells 0/1` |
| middle | a 10 × 8 island: grass (buildable), sand beach and a river (not buildable), two boulders |
| above the toolbar | the banner: "Build 2 houses, 3 trees and a well", then "Tiny Town is ready!" |
| bottom | four big buttons: `House 3`, `Tree 1`, `Well 2`, `Clear` |

## Rules

1. Tap a button to choose a tool (House at the start).
2. Tap a ground cell to build the chosen thing there. A house grows from the tapped cell toward the viewer
   (+x and +y); trees and wells take one cell.
3. A building appears only if every cell it needs is grass, inside the island and empty, and the player has
   enough coins. Otherwise nothing happens and nothing is paid.
4. Building costs coins and counts on the job card. With Clear, tapping a building removes it, returns its
   coins and uncounts it.
5. When the job card is complete, the banner says the town is ready. Clearing something afterwards takes that
   back.

There is no losing and no timer: the worst case is running out of coins, and Clear fixes that.

## Controls

Tap only (touch first, ADR 0010). Buttons are screen entities; the island is ground taps. No dragging.

## What the engine provides vs what the game data says

| engine (builder kit v1, ADR 0013) | game data (`examples/tiny_town`) |
|---|---|
| `[ground] map` + `[[terrain]]` with `build` | the island map and its three terrains |
| ground taps (`from = "ground"`) and `spawn` with placement rules | one spawn connection per tool, gated by tool and coins |
| `footprint`, multi-cell drawing and blocking | the 2 × 2 house prefab |
| `from_prefab`, `to = "self"`, `spawned` | paying, counting and clearing for every copy |
| props, `when`, `label`, `screen_pos` (ADR 0011) | coins, tool, job card, banner, toolbar |
| `[[expect]] prefab` + `count`, `inspect --place` | three playtests and their tap pixels |

## Playtests (all must pass in CI)

- `build_town`: the full job for 11 coins; the banner says ready; nothing refused; reference image.
- `refusals`: water, beach, a reach into the river, a boulder and a taken cell are refused; the last tap with
  0 coins buys nothing.
- `clear`: a house is built, cleared (coins back) and built again on the same spot; reference image.

## Out of scope for v1 (candidates if the game needs them)

A "can't build there" message (needs a refusal signal), sounds, dragging to build rows, rotating buildings,
more islands or levels, people walking around the town, image sprites.
