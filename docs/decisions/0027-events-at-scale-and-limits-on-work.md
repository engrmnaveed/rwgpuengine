# ADR 0027: Events at scale, and limits on the work of a step

- Status: accepted and built, 2026-11-27 (step 5 of the [3D proposal](../proposals/3d-architecture.md),
  piece 2 of 3; the loader and limits at load are piece 3)
- Decided by: the owner set the budget (one step with 50,000 moving actors in 2 ms, proposal section 5) and the
  direction (proposal 4.3: "events become something an agent turns on for the entities it is asking about,
  with counts for the rest"). The design and the numbers were chosen by measurement; the tables are below.
  No state hash, no reference image and no file format changed. The ten example playtests print the same
  events, byte for byte. No budget limit was raised; one row was added (`time.sim_step_us`).
- Builds on: [ADR 0026](0026-cell-index-and-the-world-in-modules.md) (the cell index),
  [ADR 0005](0005-determinism-model.md) (determinism), [ADR 0009](0009-stable-diagnostics.md) (codes).

## Context

After the cell index, one step of the town with 50,000 actors took 14.1 ms, of which 0.88 ms were the rules:
the event log was 94 % of the step. The town writes 40,000 events a step, one `moved` or `blocked` for each
actor. The log kept 100,000 events, so at that size it held the last two and a half steps. And nothing but
the signal limit (1,000 a step) bounded what a game file could make one step do.

### Where an event's time went

A measuring copy of the engine (not in the tree) switched parts of writing an event off, one at a time. The
town, one step, median ms, Apple M5 Pro, release build:

| what is done for each event | 5,000 | 50,000 |
|---|---|---|
| everything (as it was) | 1.07 | 15.0 |
| nothing: the rules alone | 0.081 | 0.97 |
| the sentence and the fields built, then dropped at once (not stored) | 1.50 | 15.7 |
| the fields built and stored, no sentence | 0.36 | 6.3 |
| the sentence built and stored, no fields | 0.86 | 9.7 |
| everything, and the log never full (nothing dropped) | 1.27 | 14.4 |
| a 32-byte fact stored instead (frame, kind, who, the cells, the reason) | 0.085 | 1.01 |
| a counter per kind, nothing else | 0.080 | 0.93 |

The sentence is about 60 % of an event's cost and the fields about 35 %: each is a handful of allocations
(the entity's description, `format!`, a key `String` per field, the arrays of cells). Storing and dropping
are within the noise. A fact or a count costs about 1 ns.

### Who reads events

| reader | needs |
|---|---|
| `rwgpu run --events`, text | frame, kind, sentence, in order |
| `rwgpu run --events --json`, `inspect --events`, both web players' panels | the sentence and the fields, entity labels, in order; `inspect` filters by entity and `--since` |
| `rwgpu run` and `inspect` summaries | a count per kind |
| playtest `[[expect]] event` | counts by kind, entity, other, signal, `reason`, `changed`, `prefab`, before a frame |
| playtest failures (E0401) | the last three sentences about an entity; refused spawns of a prefab |
| the oracle and the sim tests | everything, in order |
| `sim-bench` | how many |
| the native player | nothing |

## Decision

### 1. A budget per kind per step, for the kinds there is one of per actor

Four kinds happen once per actor or plate in a step: `moved`, `blocked` for a move (edge, solid, actor, push,
cooldown, area), `trigger`, and `custom` (Rust game code). In each step the first 256 of each of these kinds
are written. The rest are counted, and at the end of the step one `omitted` event per kind says how many:

```text
[   37] omitted   29956 more 'moved' events in this step were counted, not written: the first 256 of a kind
                  are written each step. Watch an entity to have all of its events written (`--watch NAME` on
                  rwgpu run and inspect)
```

A rule asks the log before it builds the sentence (`World::wants`); a no costs a counter. Every other kind is
always written: `input`, `tap`, `tap_miss`, `signal`, `action`, `skipped`, `spawned`, `counted`, `error`, and
`blocked` for a spawn. The limits on work (section 3) bound how many of those a step can have. They are not
budgeted because they are what a game's logic did and why: the oracle's `plates-150`, a small world, writes
2,018 `action` events in one step through its signal loop.

`event_counts` counts every event that happened, written or not. `events_omitted` (per kind) and
`events_dropped` appear in `run` and `inspect` when they are not zero, and `run` prints a line for each with
how to get the detail.

### 2. Watching

A `Watch` names what is written in full whatever the count: entities (as subject or other party), kinds, or
everything. Watched events do not take a place in the budget, so watching changes nothing else that is
written.

- `rwgpu run --watch NAME,...` and `rwgpu inspect --watch NAME,...`: entity names, `#ID`, event kinds, `all`.
- `rwgpu inspect --entity NAME --events` watches that entity.
- `rwgpu test` watches every entity its `[[expect]]` tables name, and the kind of an `event` expectation
  that names no entity, so expectations count every event they ask about at any scale.
- `Session::watch` for players and tools; `World::log_with` for game code that builds a message only when it
  is written.

A replay is deterministic, so an agent can always run again with a watch and get the detail of any step.

### 3. Limits on the work of a step

`rwgpuengine_core::limits` holds the engine's limits in one table; `rwgpu explain limits`, the generated
reference (`docs/reference/formats.md`, runtime rules) and the AGENTS.md of a new project print it. A step
that reaches a limit stops there: the signals still queued are dropped, an `error` event says what was cut
off and why, and `rwgpu run`, `inspect`, `shot` and `test` report the limit's code as an error, with the
step, the command that shows it and how many steps reached it. Nothing hangs and nothing stops silently.

| limit | number | code | what a game file could do without it |
|---|---|---|---|
| signals per step | 1,000 (as before) | E0301 | a loop of connections |
| actions per step, run or skipped | 10,000 | E0302 | one signal reaches many connections with many actions; a few that feed each other through `changed` reach hundreds of thousands within 1,000 signals |
| count changes per step | 10,000 | E0303 | `counts` on many entities times many changes: copies that count their own kind and spawn more |
| entities in a world | 1,000,000 | E0304 | a spawn every step, or a chain where each new copy spawns more, grows the world until memory runs out |
| taps per step | 256 | E0305 | a playtest with tens of thousands of taps in one frame, each resolved against the drawn frame |
| events of one kind per step | 256 | — | (section 1; not an error) |
| events kept | 100,000 | — | (as before; not an error; dropped events are counted per kind) |

Counts needed a change to be bounded at all: after a spawn or a switch, the world looked at every entity
with `counts` even when none counted that prefab. It now keeps, per prefab, who counts it, and looks only
at those; once a step, and when the world is built, it still looks at all of them. The answers and their
order are the same, because every count is up to date before each change; a check in the tests compares
every such look with a look at all.

Two more things were cheap and are done: dispatching a signal no longer copies the connections it reaches
(they are shared), and a world without a ground keeps its cells in an array over where its entities are
when it is built, so it is no longer ten times slower per move.

### 4. The budget row

`budgets.toml` `[time] sim_step_us = 2_000`: one step of the town of 50,000 wanderers, events as a game
writes them by default, the median of 200 steps, measured by `cargo xtask budgets` (`cargo xtask sim-step`,
a release build). Scaled on CI like the other time budgets.

## Options that were measured

The town, ms per step (median of three runs of 200 steps; 20 at 500,000), the same state hash in every row
of a column:

| design | 5,000 | 50,000 | written per step | small games | why actor #31005 did not move at step 37 of a run |
|---|---|---|---|---|---|
| every event as a sentence (as it was) | 1.11 | 13.9 | 40,000 | unchanged | dropped from the log unless asked within two steps |
| written when read: a 32-byte fact, the sentence made on reading (prototype) | 0.085 | 1.01 | 40,000 facts, 1.3 MB | unchanged, if all seven sentence forms are re-made exactly | the log still holds 2.5 steps; every reader changes |
| a budget per kind per step, alone | 0.23 | 1.06 | ≤ 514 | unchanged | not written: past the first 256 |
| watching alone, the rest only counted | 0.080 | 0.93 | watched only | changed: a small game would write nothing | written |
| **a budget and watching** (built) | **0.23** | **1.06** | **≤ 514, and what is watched** | **unchanged** | **written, one command** |

- Written when read is the cheapest per event, but at 50,000 it still writes 40,000 facts a step, so memory
  grows with the world and the log still holds two and a half steps. Building it means re-making every
  sentence and field from a fact on reading, through the world, for about 60 readers. The budget leaves it
  0.18 ms to save at 50,000. Not built.
- Watching alone cannot keep small games unchanged without a flag. The budget alone cannot explain one actor
  in a crowd. Together they keep both.
- A budget on every kind would change `plates-150`, a small world of the oracle, and hide the actions of a
  loop, which is what an agent reads to find it. The limits on work bound those kinds instead.
- 256: the most events of one per-actor kind in one step is 6 in the example playtests and 136 in the
  oracle's small worlds (`town-200`); 256 leaves room for both and costs 0.17 ms a step at any size.

### Before and after

The town, ms per step. "Rules alone" is a measuring copy that writes no event; its state hash is the engine's.

| actors | before: events | before: rules alone | events' share | after: default | after: every event (`--all`) | after: rules alone | events' share |
|---|---|---|---|---|---|---|---|
| 1,000 | 0.233 | 0.015 | 94 % | 0.148 | 0.229 | 0.015 | 90 % |
| 5,000 | 1.108 | 0.079 | 93 % | 0.231 | 1.106 | 0.076 | 67 % |
| 20,000 | 4.62 | 0.336 | 93 % | 0.498 | 4.64 | 0.328 | 34 % |
| 50,000 | 13.9 | 0.909 | 93 % | **1.063** | 14.0 | 0.883 | 17 % |
| 500,000 | 141 | 19.9 | 86 % | 20.8 | 146 | 20.5 | 1.4 % |

What events cost a step no longer grows with the world: at most 4 × 256 events of the budgeted kinds and 4
`omitted` events, about 0.18 ms, plus a counter for each event not written. The town without a ground, 50,000
actors: 24.2 ms before (9.9 ms rules alone), 1.6 to 1.7 ms after, the same state hash.

### The worst step a small file can write

Each file is a few dozen lines (`scale_tests.rs` has one per limit). The slowest step, release build:

| file | before | after |
|---|---|---|
| a loop through `changed` | 1,000 signals, 2,000 events | 0.95 ms, E0301 (as before) |
| 30 connections of 30 adds on one `changed` | 900,000 actions in one step, about 650 ms | 7.2 ms, E0302 |
| eggs that count eggs and spawn more | 3 s for 3 steps, then worse | 10.2 ms, E0303 |
| each egg spawns ten more | 150,000 entities after 30 steps, without end | 27 ms (10,000 spawns), stops at 1,000,000 entities in step 200 (1.2 GB resident), E0304 |
| 20,000 taps in one frame | 20,000 picks and taps | 6.3 ms, E0305 |

## The worked example

A town of 50,000 named actors (`cargo xtask sim-bench town 50000 --write DIR`). Why did `walker_31005` not
move in step 37?

```text
$ rwgpu run --frames 38
  events      moved 1161913, blocked 358230, omitted 76
  omitted     moved 1152185, blocked 348502 (counted, not written: the first 256 of a kind are written each
              step; --watch NAME writes all of an entity's events)

$ rwgpu inspect --frame 38 --entity walker_31005 --events --since 37
frame 38 of scenes/main.toml (seed 7, state e7d0221d65e8e85f)
  31005 walker_31005 cell [186,284] wander               screen null
events (1):
  [   37] blocked   'walker_31005' (#31005) could not move wander from (186, 284) to (186, 285): blocked by
                    actor 'walker_32798' (#32798)
```

One command, 2.0 s (the load is 1.5 s of it; piece 3). Before, `--frame 38` found it only because the log
still held the last two steps; asked from frame 60, the log had dropped 2,350,000 events and the sentence
with them.

## Evidence

- The ten example playtests: `rwgpu run --events` (text and JSON), `inspect --events --json` and `test --json`
  for each, the three examples run for 600 frames, and `rwgpu test` on each project: 49 outputs, the same
  bytes before and after. Their state and image hashes are unchanged, natively and in the wasm module.
- The oracle runs each of its 17 worlds twice. With every event written it reaches the scanning engine's three
  numbers, so every sentence is the one it wrote. By default the two state numbers are the same and the
  events differ in five large worlds, the ones with more than 256 events of a per-actor kind in a step:
  `town-1000`, `town-5000`, `plates-2000`, `crowd-100x100` and `open-3000`. Their fourth number was recorded
  with the budget. The other twelve, all eight small ones among them, write the same events by default.
- A world reaches the same state hash with nothing watched, everything watched and some entities watched; a
  watched entity's events are word for word those of a run that writes everything; written plus omitted is
  what happened.
- A test per limit, each a small hostile scene that reaches it; the session's tests check the diagnostic, its
  hint and its notes, and that a playtest fails with the code.

### Sizes

| | before | after | limit |
|---|---|---|---|
| `web.wasm_bytes` | 658,555 | **680,792** | 800,000 |
| `cli.binary_bytes` | 2,183,600 | **2,234,144** | 3,000,000 |
| `time.sim_step_us` | — | **1,030 to 1,060** on a quiet machine, 1,460 to 1,830 with other work on it | 2,000 |

The web module grew by 22,237 bytes (3.4 %) and `rwgpu` by 50,544. That is the cost of this piece: the
watch, the budget and the counts per kind in the log, the limits and their messages, the five codes'
explanations, the counts per prefab, and the cells over an origin.

## Measured, not built: the render side at 50,000

The same town at frame 38, release build: the render list 1.35 ms (200,704 ground cells; 1.60 ms with a sprite
per actor), its projection 10.8 ms (11.4), the pixels and owners at 640 × 480 1.2 ms, the frame report 2.8 ms
(26.6 ms with 50,000 sprites), `rwgpu shot`'s whole frame 17 ms (42 ms). Loading and checking the scene:
2.2 s.

## Consequences and limits

- **A large world's log is a sample.** The first 256 moves of a step are those of the lowest ids. Counts are
  exact; the detail of the others needs a watch and a replay.
- **`event_counts` now counts what happened**, not what the log kept: for a run whose log was full it is
  larger than before. For every run that fits in the log, it is the same number.
- **A playtest fails when its game reaches a limit**, with the limit's code. Before, a step cut by E0301 was
  only an event.
- **A step that reaches a limit drops the rest of its signals**, whatever they were; later steps run.
  At the count limit, the counts left behind are brought up to date by the next step.
- **Watching a kind costs what that kind costs**: `--watch moved` on 50,000 actors writes 30,000 events a step.
- **The entity limit is a lot of memory**: 1,000,000 props-only entities held 1.2 GB natively.
- **A world without a ground** has its array over the cells its entities cover when it is built (at most 16
  cells an entity). Actors that walk far from there are in the sorted map again: the town without a ground
  rises from 1.2 to 1.7 ms over 100 steps.
- `rwgpu test` looks a playtest's `[[expect]]` and `[[shot]]` tables up by frame; before, every frame read all
  of them (20,000 expectations over 3,000 frames: 70 → 40 ms in all). The script still reads every
  `[[input]]` each step: 10 us a step for 20,000 inputs. An index of the inputs was built three ways (sorted
  places, one sort for both lists, a map from frame to places): each answered what the scan answers on 300
  random scripts, and each made the web module 8,046 to 11,949 bytes larger. Not kept: the gain is at most
  10 us a step, and piece 3 limits the tables at load.

## Not done

- The loader and limits at load: footprints, the ground's size, entities in a scene (piece 3). A spawned
  prefab's footprint is limited there too.
- Written when read: measured, not built (above).
- Watching cells: no reader asks by cell; `--track NAME` follows one entity's cells.
- A watch in the web players' page (`Session::watch` is there for it).
- The render side at scale (measured above).

## Revisit when

- A reader needs every event of a large world (a replay viewer, a heat map): then store facts and write when
  read.
- A game needs more than 256 events of one kind a step to be readable by default.
- A reference game reaches a limit while doing what it is meant to do.
