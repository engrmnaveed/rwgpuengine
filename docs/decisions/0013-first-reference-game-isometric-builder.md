# ADR 0013: First reference game is an isometric builder (Tiny Town); builder kit v1

- Status: accepted and built, 2026-11-25
- Decided by: the project owner (Q3: "Yes, the builder"); the kit's design choices by the KPI rule (ADR 0011)

## Context

Guardrail 2 says reference games drive features. Q3 asked which game comes first: the isometric builder, a
puzzle, or tracing. The puzzle is already half there (iso_garden, coin_vault). Tracing needs drag input,
which ADR 0010 left for later. The builder pulls in the most missing engine: tap-to-build on ground cells,
placement rules, multi-cell objects and their draw order, and terrain. Those were also open limits in the
docs ("taps on bare ground hit nothing", "single-cell objects only").

## Decision

The first reference game is **Tiny Town** ([one-page spec](../games/tiny-town.md)): a child taps a tool, then
taps an island to build houses (2 × 2), trees and a well within a coin budget. It is built entirely in data
(`examples/tiny_town`, 3 playtests). The engine features it needs, **builder kit v1**:

| feature | choice (and the option it beat, on KPIs) |
|---|---|
| Terrain | `[ground] map = ["~~gg", ...]` + one `[[terrain]]` per character (`name`, colors, `build`). One file, the map is visible in the diff. It beat one tile entity per cell (80 tables for a 10 × 8 island) and a separate tilemap file (another file to open and keep in step). |
| Ground taps | A tap that hits no entity but lands on a ground cell makes the ground emit `tapped` with the cell, when a `[[connect]] from = "ground"` listens (otherwise it is a `tap_miss` that now carries the cell). `ground` and `self` become reserved names. |
| Building | `action = "spawn"` + `prefab`: a new copy goes on the signal's cell (the tapped cell, or the emitter's) if every footprint cell is inside the ground, on terrain with `build = true` and free. A refusal is a `blocked` event (`cause = "spawn"`, reason `edge`/`terrain`/`occupied`, the cell that failed). Success logs `spawned`; the copy gets the next id, has no name, and emits `spawned` if anything listens. |
| Every copy | `from_prefab = "house"` listens to every copy of a prefab (placed or spawned); `to = "self"` targets the copy that emitted. Spawned copies have no names, so this is required, and it also collapses per-copy wiring (coin_vault's three coin connections could be one). |
| Multi-cell | `footprint = [w, h]` from `pos` toward +x/+y; blocks, triggers, taps and placement use every covered cell. Only things that stay put (no mover/patrol/wander/pushable/screen_pos). |
| Draw order | A multi-cell block is drawn as one slice per cell, each sorted by its own cell (sprites and labels sort with the front cell). It beat a topological sort: no per-frame graph, existing scenes keep their exact pixels, and a test proves a 2 × 2 block draws the same pixels as four 1 × 1 blocks. |
| Looks | `block.top`: a top-face color (roofs, water in a well). |
| Proving | `[[expect]] prefab = "house"` + `count` (enabled copies, optionally at `pos`). |
| "Can I build here?" | `slate inspect --place house@3,4` (fits or why not, at any frame) and `--place house` (every cell where it fits), each with a pixel that taps that ground cell, verified by picking. `inspect` JSON shows the map and terrain legend under `ground`. |

Two rules found while building it:
- **Data-only entities take no space.** A props-only entity sits at the default `[0, 0]`; counting it as
  "occupied" made that cell unbuildable with nothing visible there. Only entities that are drawn there, solid,
  pushable, actors or triggers take up cells.
- **Removal is `disable`.** A disabled copy takes no space and is not drawn; it stays in `inspect` with its id.
  No `remove` action and no id reuse, so ids stay stable and the event log stays readable.

## Not built (listed in Limits)

Rotation; dragging (rows, moving buildings); a signal on refused placements (so a game could say "can't build
there"; built in [ADR 0014](0014-refusal-signal-and-count-props.md)); terrain that blocks walking; deleting
entities; cost tables. Tiny Town's scene has 26 `[[connect]]`
tables, 9 of them the clear/refund wiring (3 per prefab): that is the evidence to watch for a later
"per-prefab cost" or grouped-connection feature.

## Consequences

- State hashes and images of scenes without maps, footprints or top colors are unchanged (iso_garden
  `469ae7c92124d43d`, coin_vault solve `847b98297f2eefd8`).
- Determinism holds: Tiny Town's `build_town` ends at state `537ae6f9300e9b67`, image `58bd8a0dbea14409`,
  natively and in the browser.
- The wasm grew from 586,226 to 640,798 bytes (+54.6 KB, +9.3%; budget 700,000). Storing spawn templates in a
  `Vec` instead of a `BTreeMap` saved 15 KB of generic code. `opt-level = "z"` would bring it to 600,909 bytes,
  but its frame-time cost is unmeasured, so it is not adopted here.
- New codes: E0208 unknown-terrain. New event kind `spawned`; new block reasons `terrain`, `occupied`.

## Acceptance

A cold agent builds a different small builder game from a short spec with the generated AGENTS.md only:
terrain, tap-to-build with costs, a multi-cell building, clear with refund and playtests that prove each rule,
with 0 warnings. That is cold-agent run #7, offered as the next step.

**Result ([run #7](../experiments/2026-11-25-cold-agent-7.md)): met.**
- A farm builder (pond and path terrain, a 3 × 2 barn, crops, fences, remove with refund, a ready message):
  every requirement supported, 6 playtests, 0 warnings, 8 min 44 s.
- Its findings are fixed: `slate test <file>` outside `tests/` silently ran everything; event expectations
  can now filter by `prefab`; the `[[terrain]]` and condition key tables are in AGENTS.md; the taken-cell and
  removal wording.
- Its top feature requests match Tiny Town's gaps: a signal on refused placements, and conditions that count
  copies of a prefab. Both are built in [ADR 0014](0014-refusal-signal-and-count-props.md) (`refused`,
  `counts`).
