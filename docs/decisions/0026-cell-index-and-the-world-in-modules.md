# ADR 0026: The simulation at scale — a cell index beside the entities, and `world.rs` in modules

- Status: accepted and built, 2026-11-27 (step 5 of the [3D proposal](../proposals/3d-architecture.md),
  piece 1 of 2; the loader, limits on work and events at scale are piece 2)
- Decided by: the owner approved the cell index first and column storage later, with a measurement
  (proposal 4.4), and the budget of 2 ms for one step with 50,000 moving actors (section 5). The index's
  structure and the storage were chosen by measurement; the tables are below. No state hash, no reference
  image and no file format changed. No budget limit was raised. The 2 ms row is **not** added yet; see
  [The 2 ms budget](#the-2-ms-budget).
- Replaces: the storage part of [ADR 0006](0006-fat-entities-and-game-trait.md) ("linear scans per
  move/trigger; fine at learning-game scale"). Fat entities, props and ids by scene order stand.
- Builds on: [ADR 0005](0005-determinism-model.md) (the determinism contract).

## Context

One step with N actors that each try one cell took 1.3 ms at 1,000 actors, 26.5 ms at 5,000, 410 ms at
20,000 and 2,659 ms at 50,000. `world.rs` was one file of 1,418 lines that held movement, triggers, signals,
actions, placement, payment, counting and hashing.

### Where the time went

`cargo run --release -p xtask -- sim-bench` steps four generated worlds. A copy with a timer around each scan
(a patch for measuring; not in the tree) gave this on an Apple M5 Pro, per step:

| world | what scans | how often | 5,000 | 20,000 | grows as |
|---|---|---|---|---|---|
| town: N actors, one try each | `blockers_at`: every entity | per move | 25.6 of 27.3 ms | 422 of 432 ms | N² |
| plates: N/2 plates, N/2 actors | `occupants_of`: every entity | per plate | 22.2 of 36.8 ms | 355 of 565 ms | N² |
| the same | `blockers_at` | per move | 12.5 ms | 202 ms | N² |
| wired: the same, a `[[connect]]` per plate | every connection | per signal and per `is_listened` | 6.5 of 43.3 ms | 37.0 of 605 ms | signals × connections; signals stop at 1,000 a step |
| build: N trees, 64 ground taps a step | `count_changes`: every entity | per counted prefab, per spawn | 0.92 of 2.0 ms | 2.7 of 5.4 ms | N per spawn |
| the same | `can_place`: every entity | per cell of a spawn | 0.88 ms | 2.4 ms | N per spawn |
| every world | `ids_with`: every entity | 4 times a step | 0.03 ms | 0.14 to 0.26 ms | N |

Rules against events, in the town: building and storing events took 1.6 of the 27.3 ms at 5,000 (6 %) and
about 9 of the 432 ms at 20,000 (2 %), 0.4 to 0.5 µs each. With no event written the step reached the same
state hash. The scan hid what events cost.

## Decision

### 1. The world keeps an index, and no rule looks at every entity

`world/index.rs` and `world/cells.rs`. Five things, all derived from the entities:

| what | for | how |
|---|---|---|
| per cell, the enabled entities on it, in id order | what blocks a cell, what takes a cell, who stands on a plate | the ground's cells are an array of list heads, 4 bytes a cell; a cell's list is a chain through one pool of 8-byte nodes; cells outside the ground, and every cell of a world without a ground, are in a sorted map |
| per entity, a 24-byte fact: cell, footprint, and bits for enabled, takes space, solid, actor, pushable, each behavior | answering without reading the 432-byte entity; knowing what was entered when the entity changes | one array, by id |
| who has each behavior, in id order | whose turn it is in each part of the step | four sorted lists, in place of four scans a step |
| enabled copies per prefab | `counts` props | a number per prefab name |
| per signal and source, the connections that listen | dispatch and "does anything listen?" | one list sorted by (signal, source), searched by halving (`world/wiring.rs`) |

An entity is entered on every cell its body covers, and on the cell it stands on if it can hold a plate. For
anything a scene file can describe these are the same cells.

### 2. The same answers, in the same order

`world/scan.rs` holds the scans as they were. They are the definition; no step calls them. Every list is in
id order, so "the first blocker", "exactly one blocker" (what makes a push), "the first entity that takes the
cell" and a plate's occupants are what the scan gave. That is what keeps the state hashes and the event log.

### 3. Derived, never state

The index is not in the state hash. `Index::build` makes it from the entities alone. `World::verify_index`
compares the kept one with a rebuilt one by what they say, not by where their nodes lie.
`World::rebuild_index` replaces it.

It stays right in two ways:

- **The rules tell it** what they change: a move (`put`), a spawn (`enter`), enable, disable and toggle
  (`resync`). Nothing else in the rules changes where an entity is or what it is.
- **Everything else goes through a guard.** `World::entities` reads like a slice and cannot be written.
  `World::get_mut` returns an `EntityMut`; when it is let go, the world brings the index up to date with
  whatever changed. Rust game code and tests are written as before: `world.get_mut(id)?.enabled = false`.

### 4. Two checks that can catch it being wrong

- **Debug builds** compare the index with a rebuild at the end of every step and panic where they differ.
  `cargo test` and the gate's debug runs of the example playtests go through it.
- **`World::check_against_scans(true)`** answers every question both ways, the index's and the scan's, and
  panics where they differ. It is in debug builds and in the crate's tests. A release build leaves it out:
  it would be 8 KB of the web module. The call says whether the check is on.

### 5. Entities stay as they are

Measured below: with the index, fat entities meet the budget at 50,000 with room to spare. Column storage and
entity ids that can be reused are not built. They wait for the first game at city scale, as the proposal said.

### 6. `world.rs` in modules

`world.rs` (274 lines) is the world, how it is built and the order of a step. The rest:

| module | holds |
|---|---|
| `world/entity.rs` | `Entity`, its components, `Entities` (the read-only list), `EntityMut` (the guard) |
| `world/wiring.rs` | `Connection`, `Act`, `Cost`, `Condition`; names resolved; the routes |
| `world/cells.rs`, `world/index.rs` | the cell index |
| `world/scan.rs` | the scans, as the definition |
| `world/taps.rs` | input: pressed actions and taps |
| `world/movement.rs` | what blocks a cell, `try_move`, movers, patrols, wanderers |
| `world/triggers.rs` | `occupants_of`, `run_triggers` |
| `world/signals.rs` | the queue, conditions, dispatch, `run_connection` |
| `world/actions.rs` | enable, disable, toggle, add, set |
| `world/placement.rs`, `world/payment.rs` | `can_place`, `spawn`; a spawn's cost |
| `world/counting.rs` | `counts` props |
| `world/hashing.rs` | the state hash, the same text |

The split is its own commit, made before the index, and it moved lines only: a script cut the old file by
line ranges, and a check found every old line in the new files and nothing new but module docs, imports,
`impl World {` and `}`, and one function around the expression that resolves connections.

## Options that were measured

All on an Apple M5 Pro, release build, the town (actors on every other cell of a square ground, each trying
one step per frame), median of up to 400 steps. "Rules alone" is a copy of the engine that writes no event
(a patch for measuring; the state hash it prints is the engine's).

### a. The structure under the cells

Five structures with one interface (`add`, `remove`, the ids on a cell in order). Each passed all 17 worlds of
the oracle before it was timed. The town's rules alone, ms per step:

| structure | 5,000 | 50,000 | 500,000 | no ground, 50,000 | bytes for a 448 × 448 ground |
|---|---|---|---|---|---|
| **an array of list heads, lists chained through a pool of nodes; a sorted map outside** (chosen) | 0.073 | 0.84 | 21.3 | 9.5 | 0.8 MB + 8 per entry |
| an array of two inline ids per cell, a `Vec` where there are more; a sorted map outside | 0.077 | 0.90 | 21.5 | 9.4 | 1.6 MB |
| an array of `Vec`s; a sorted map of `Vec`s outside | 0.081 | 1.09 | 23.5 | 8.3 | 4.8 MB + what the `Vec`s hold |
| one sorted set of (x, y, id), no array | 0.56 | 6.9 | 86.7 | 6.9 | 16 per entry |
| a sorted map of `Vec`s, no array | 0.67 | 8.0 | 106 | 8.0 | about 56 per cell in use |

- An array for the ground is 8 to 10 times faster than any sorted structure. `HashMap` is not allowed
  (ADR 0005) and was not measured.
- Among the arrays the chained lists are the fastest and the smallest, and a move allocates nothing: it
  frees a node and takes it again.
- **A world without a ground is in the sorted map**: 180 ns an actor, against 17 on a ground. See the limits.

### b. Storage

Three ways to store what a step touches, the same index under each. The town's rules alone, ms per step,
the middle of three runs:

| actors | A: the index; a question reads the entity | B: the index and a 24-byte fact per entity (built) | C: positions and wander timers in arrays of their own (a prototype) |
|---|---|---|---|
| 5,000 | 0.074 | 0.075 | 0.059 |
| 50,000 | 1.14 | 0.90 | 0.63 |
| 500,000 | 22.1 | 20.5 | 8.1 |

- C never reads or writes the 432-byte entity during a step. It was written into the real engine for the
  wander part only, with a call that writes the arrays back before the state hash is taken; its hashes are
  the engine's. It is not in the tree.
- **At 50,000, B is under half of the budget and C is 1.4 times faster. At 500,000, C is 2.5 times faster.**
  The proposal expected that fat entities are enough at 50,000 and that columns matter at city scale (its
  scratch benchmark said 3.5 times at 500,000). The engine confirms both, with a smaller factor.
- B is a fifth faster than A at 50,000 and the same at 5,000: a question then reads 24 bytes, not an entity.
  The fact is needed anyway, to know what to take out of the index when an entity changes.
- These were measured on the code as committed, the table of structures above a few changes earlier; there
  the chosen structure read 0.073, 0.84 and 21.3 where it reads 0.075, 0.90 and 20.5 here.

## Evidence

### The oracle

`crates/rwgpuengine_sim/src/oracle.rs` was committed first, on the engine that scanned. It generates 17
worlds, each a scene file that goes through the loader, driven by input made with the engine's own random
numbers. For each it pins three numbers: the state hash at the last frame, a hash over the state hash of
every frame, and a hash over every event written.

| world | size | what it asks |
|---|---|---|
| town | 200 actors in the gate; 1,000 and 5,000 by hand | blocked moves: actor and edge |
| plates | 150; 2,000 | a plate under every other actor, a signal for each change, a loop that the signal limit cuts |
| crowd, packed | 20 × 16, 16 × 16; up to 100 × 100, 4,081 entities | four cells in ten taken: walls, halls of 2 × 2, gates, crates, movers, patrols, wanderers (with an area, off the middle of a cell, at a height), plates with `only`, pads of several cells, two plates on one cell, weights, things outside the ground, a toolbar, spawns with costs, refusals for all four reasons, counts, one to three actions on up to two conditions for every plate |
| builder | 24 × 24; 96 × 96 | little to start with; four to six taps a frame build it |
| open | 160; 3,000 | no ground: cells below zero and far away |
| meddled | 20 × 16, open 120; 64 × 48 | Rust game code that each step flips an entity, moves one, shoves one, makes one emit and asks `blocker_at`, `occupants_of`, `can_place`, `is_listened`; the answers are written as events and so pinned |

The engine with the index reaches all 51 numbers. `oracle.rs` is byte for byte the file that recorded them.
The pins hold on aarch64 and x86-64 macOS and on x86-64 Linux; CI adds Windows.

### The index against the scans

`crates/rwgpuengine_sim/src/equivalence.rs`:

- **16,000 random steps on 400 random worlds**, with the check against the scans on. The worlds are put
  together from parts, not loaded, so they hold what no loader lets through and Rust game code can still
  make: an actor on the screen, a footprint of nothing or less, a pushable three cells wide, things 900,000
  cells away, grounds of one row, of nothing and none at all. Game code changes entities through `get_mut`
  between the rules of every step, in eight ways, and asks the world's questions on random cells. Every
  answer is the scan's, and after every step the index is what a rebuild gives.
- A world reaches the same state, the same events and the same answers with the check on or off.
- A world stepped, and a copy whose index is thrown away and rebuilt every third step, agree at every step.
- An index that was not told of a change is found by `verify_index`.
- The oracle's small worlds again with the check on: the same pins.
- The routes against a scan of the connections: 300 random sets of connections, every emitter and signal.

### Do the tests notice a fault?

Twelve faults were planted in the index, one at a time: a move that leaves its old entry, a list kept in
arrival order, a disable that does not reach the index, a spawn that is not entered, copies not counted down,
a body mistaken for a weight, and so on. Each is noticed by 2 to 20 tests. Three are noticed only by the
random worlds, because only they hold a footprint of nothing or an actor on the screen. The first run missed
one: the order in which a signal's entity and prefab connections are merged. The direct test of the routes
was added for it.

### The gates

`cargo xtask ci`: the ten example playtests keep their state and image hashes, natively and in the wasm
module (the 15 distinct frame, state and image triples the run prints are the same as before).
`cargo xtask gpu`: green.

### Before and after

`sim-bench` with `--steps`, so both engines stop at the same frame. All 16 state hashes are equal. Median ms
per step, events as they are today:

| world | 1,000 | 5,000 | 20,000 | 50,000 |
|---|---|---|---|---|
| town | 1.25 → 0.29 | 26.5 → 1.14 | 410 → 5.2 | 2,659 → 15.8 |
| plates | 1.74 → 0.30 | 35.6 → 1.49 | 553 → 4.8 | 4,356 → 12.2 |
| wired | 2.03 → 0.30 | 42.7 → 1.51 | 595 → 4.9 | 4,347 → 12.6 |
| build | 0.56 → 0.14 | 1.90 → 0.13 | 5.3 → 0.15 | 19.8 → 0.16 |

The town over more steps (up to 400), split:

| actors | with events | rules alone | per actor, rules alone | events' share | the index holds |
|---|---|---|---|---|---|
| 1,000 | 0.22 ms | 0.014 ms | 14 ns | 93 % | 51 KB |
| 5,000 | 1.06 ms | 0.075 ms | 15 ns | 93 % | 292 KB |
| 20,000 | 4.6 ms | 0.33 ms | 16 ns | 93 % | 1.2 MB |
| 50,000 | 14.1 ms | 0.88 ms | 18 ns | 94 % | 2.8 MB |
| 500,000 | 140 ms | 20.2 ms | 40 ns | 86 % | 25.7 MB |

The time per actor is flat from 1,000 to 50,000. At 500,000 it doubles: 216 MB of entities no longer fit in
any cache, which is what columns would cure.

Each question, before and after, per step (the timer itself is about 17 ns a call, most of the right column):

| question | calls a step | before | after |
|---|---|---|---|
| what blocks a cell (town, 20,000) | 15,966 | 422 ms | 0.26 ms |
| who stands on a plate (plates, 20,000) | 10,000 | 355 ms | 0.22 ms |
| who listens (wired, 20,000) | 2,000 | 37.0 ms | 0.09 ms |
| how many copies (build, 20,000) | 45 | 2.7 ms | 0.003 ms |
| what takes a cell (build, 20,000) | 64 | 2.4 ms | 0.003 ms |
| whose turn (town, 20,000) | 4 | 0.17 ms | 0.016 ms |

### Memory

An entity is 432 bytes, as before. The index is 56 bytes an entity for the town of 50,000 (2.8 MB beside
21.6 MB of entities): 4 bytes per ground cell (200,704 cells), 8 per entry, 24 per fact, 4 per actor in a
list.

### The existing budgets

| budget | before | after | limit |
|---|---|---|---|
| `web.wasm_bytes` | 632,531 | **658,555** | 800,000 |
| `cli.binary_bytes` | 2,133,296 | **2,183,600** | 3,000,000 |
| `time.render_frame_us` | 1,470 | 1,402 | 6,000 |
| `time.paint_frame_us` | 163 | 154 | 200 |
| `time.reference_town_frame_us` | 4,971 | 4,792 | 20,000 |
| `time.smoke_chain_ms` | 290 to 361 (six runs) | 305 to 352 (six runs) | 4,000 |
| `time.incremental_rebuild_ms` | 535 | 560 | 10,000 |

Times are within noise. **The sizes grew, and that is a cost of this piece**: the web module by 26,024 bytes
(4.1 %). Of that, about 9 KB is the sorted map for cells outside the ground and 6 KB the routes; the rest is
the cells, the facts, the lists and the guard. The first version was 676,434: the routes were three kinds of
sorted map (now one sorted list and a map type the world already had), and the check against the scans was
in every build.

`cargo test -p rwgpuengine_sim` takes 2.9 s in a debug build. It was 0.01 s; the oracle's small worlds took it
to 0.86 s on the scanning engine, and the random worlds are the rest.

## The 2 ms budget

One step with 50,000 moving actors, on this Mac:

- **with events as they are: 14.1 ms** (11 to 16 over runs). Over the budget, seven times;
- **the rules alone: 0.88 ms.** Under it, with more than half to spare.

The whole difference is the event log: the town writes 40,000 events a step (a `moved` or a `blocked` for
every try), and each costs 0.33 µs: a sentence, a table of fields, about twelve allocations, and later the
freeing when the log drops its oldest half (it keeps 100,000, so at this size it holds the last two steps).

So the row is not in `budgets.toml`. Piece 2 changes how events are written at scale (proposal 4.3: events
for the entities an agent asks about, counts for the rest) and adds the row, measured by `sim-bench`'s town.

## Consequences and limits

- **A world without a ground is ten times slower per move** (180 ns against 17), because all its cells are in
  the sorted map. That is 9 ms of rules for 50,000 actors. All three example games have a ground.
- **An entity costs one entry per cell it covers**, and a plate asks about every cell it covers each step.
  A footprint of 300 × 300 is 90,000 entries. The loader does not limit footprints (or the ground's size)
  yet: limits on work are piece 2.
- **The ground's array is 4 bytes a cell whether the cell is used or not**, up to 4,096 × 4,096 cells
  (64 MB). A larger ground is kept in the sorted map.
- **Rust game code changes entities through `World::get_mut`.** Code that wrote `world.entities[i]` no longer
  compiles; two tests did. `World::connections` is read through `connections()`.
- **Debug builds do more per step**: a rebuild of the index to compare with, so a debug step grows with the
  number of entities and ground cells. Release builds and the web module do not.
- **`check_against_scans` is not in release builds.**
- The state hash is still kept by hand, and still leaves out `prefab` and `name` (the audit, 3.6).

## Not done

- **Column storage and reusable entity ids.** Measured above (2.5 times at 500,000); they wait for a game at
  city scale.
- **An array for a world without a ground**, or a ground in chunks so that an empty part costs nothing. Either
  would lift the first and third limits. No game needs them yet.
- **Events at scale, limits on work, the loader** (2.4 s to load and check 50,000 actors; `sim-bench` builds
  its worlds from definitions to get around it): piece 2.
- **The render side at scale** (a ground instance per cell, the frame report's cost) and free movement off
  the grid.
- **The connections a signal reaches are still cloned** for each signal, and `try_move` still builds its
  sentence before it knows the outcome. Both are part of what events cost.

## Revisit when

- A reference game has more than about 100,000 moving things, or creates and removes entities all the time:
  measure columns and reusable ids again, on that game.
- A game has no ground and thousands of actors, or a ground much larger than what stands on it.
- Rust game code gets its systems interface (step 7): `get_mut` and the guard are then part of that design.
