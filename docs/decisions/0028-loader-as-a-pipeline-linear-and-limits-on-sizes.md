# ADR 0028: The loader at scale — a pipeline, linear, and limits on a project's sizes

- Status: accepted and built, 2026-11-28 (step 5 of the [3D proposal](../proposals/3d-architecture.md),
  piece 3; piece 1 is [ADR 0026](0026-cell-index-and-the-world-in-modules.md), piece 2 ADR 0027)
- Decided by: the step's brief (the audit asked for "`load.rs` as a parse → resolve → check pipeline").
  Every choice below was measured; the tables are here. No diagnostic of a file that loads or fails
  without reaching a limit changed, in content or order; no state hash, no reference image and no file
  format changed. One code is new (E0209). One budget row is new (`time.load_check_ms`); no limit was
  raised. Proposals for the owner: [Open](#open-for-the-owner).
- Builds on: [ADR 0003](0003-toml-and-schema-registry.md) (one registry of formats),
  [ADR 0005](0005-determinism-model.md) (determinism, the order of diagnostics),
  [ADR 0009](0009-stable-diagnostics.md) (stable codes).

## Context

`rwgpuengine_data/src/load.rs` was 1,714 lines; `load_scene` was one function of 370 that parsed,
resolved and checked. Loading and checking grew with the square of the project: `rwgpu check` of the
study's town (N wanderers on every other cell) took 3.8 ms at 1,000, 25.9 at 5,000, 279 at 20,000 and
1,635 at 50,000. Nothing bounded what a file could ask: a `footprint` or a ground of any size, any number
of entities, names of any length.

### Where the time went

`cargo xtask load-bench` writes generated projects as files and times each stage. A copy with a timer
around each part of `load_scene` and `check_project` (a patch for measuring; not in the tree) gave, at
50,000, release, Apple M5 Pro (ms):

| part | town | wired | linked | grows as | why |
|---|---|---|---|---|---|
| parse and schema | 57 | 79 | 392 | N | toml_edit and `schema::Checker` |
| decode entities | 99 | 132 | 202 | N | every missing key's default parsed by toml_edit, several per entity |
| W0206, an actor inside a solid | **1,484** | 747 | 2 | actors × entities | every entity scanned per actor |
| a [[connect]]'s source | – | 462 | **1,491** | connections × entities | `from` looked up by scanning the entities |
| W0203, the signal | – | 4 | **1,358** | connections × spawning connections | "does this source spawn" searched a list of every spawning connection |
| the actions | – | 3 | **3,127** | actions × entities | `to` and a cost's payer looked up by scan |
| `when` | – | 0 | **1,505** | conditions × entities | the condition's entity looked up by scan |
| playtest [[expect]]s | – | – | **3,045** | expects × entities | names looked up by scan; the scene copied per playtest |

`linked` names everything (50,000 named entities, a [[connect]] each with a target, a payer and a
condition, a playtest naming every entity); `wired` is `sim-bench wired` as text. On the error path,
`crowded` (every actor inside a wall, every [[connect]] naming a missing entity) took 7,755 ms at 5,000:
"did you mean" scored every name in full for every unknown one, with about ten allocations a pair.

## Decision

### 1. The loader is a pipeline

`load/`, one stage or one family of rules per file:

| file | stage | what |
|---|---|---|
| `parse.rs` | 1, parse | text to nodes, checked against the schema |
| `resolve.rs`, `ground.rs` | 2, resolve | the ground and its terrains; every [[entity]] merged onto its prefab and decoded; names; for every [[connect]] who emits, what each action acts on, which prefab a spawn places, who pays. What cannot be found is reported here (E0201, E0202, E0203, E0205, a prefab file's own problems where it is first used), and the scene's sizes |
| `check/` | 3, check | each rule on its own: `entity.rs` (the rules on a merged definition, `counts`), `connect.rs` (source, signal, form of the actions, spawn, target, cost, prop logic), `when.rs`, `placement.rs` (W0201, W0206, W0202) |
| `scene.rs` | 4, build | `SceneLoader`, the prefab cache, and the scene's definitions |
| `playtest.rs` | | playtests, and the checks of their [[expect]]s against their scene |
| `order.rs` | | where each problem stands in the order agents read them |
| `mod.rs`, `vfs.rs` | | the public API, unchanged, and `check_project` |

**The order of problems is explicit.** Each stage runs over the whole scene. Every problem carries its
place: the file, the ground, the n-th [[entity]] and its step (prefab, cells, sizes, rules, counts, name),
the n-th [[connect]] and its step (source, signal, form, each action's steps, `when`), the n-th entity's
placement (overlap, inside a solid, outside). Each stage visits items in file order, so its problems are
in order already; the stages' lists are merged. That gives back exactly the order the old loader wrote
problems in, which a session shows (`rwgpu run`, `shot`, the players); `rwgpu check` still sorts by file
and line on top of it. A debug assertion checks each stage's order.

The playtest and the project file have no references of their own to resolve: they are parsed and
decoded with their rules, as before.

### 2. What was quadratic, and what replaced it

| was | now |
|---|---|
| W0206 scanned every entity per actor | a map from each cell to the first enabled solid on it that is not an actor, built once |
| `from`, `to`, a cost's payer, a `when` entity, a playtest's [[expect]] found their entity by scan | one index per scene: name → the first entity that gives it (what the scan found) |
| "does this source spawn" searched a list per [[connect]]; a prefab's named copies were scanned per `from_prefab` | sets, made once |
| `check_project` copied a playtest's scene per playtest | borrowed, with its names made once |
| a map character looked up in the list of terrains, a terrain's character in the terrains before it; the "declared:" note rebuilt per row | a map; the note made once |
| a spawn's prefab looked up among the templates by scan | a set |
| every missing key's default parsed by toml_edit | the registry's defaults are simple literals, read without the parser (a test holds that each one is, and reads as TOML reads it) |
| a prefab's table copied twice per entity | once |
| "did you mean": every candidate scored in full, ten allocations a pair | `closest` gives the same answer comparing each candidate only as far as it could still win: no allocation a pair, a length bound, a stop once a row of the distance is past the best so far, a direct comparison once only an exact match could win. A test holds it equal to the old function on 20,000 random cases |

No `HashMap`: sorted maps, of key types the web module has already (a map keyed by `char` cost it 4.8 KB,
so the terrain map is keyed by text, asked with the character encoded on the stack).

### 3. Limits on a project's sizes

A project file must not make loading, or the first step, take unbounded work by its sizes alone. Each
size below had no limit. They are rows of the engine's one table of limits, `rwgpuengine_core::limits`
(ADR 0027 made it for the limits on a step), checked at `load`, each with the code E0209 `too-large`, and
printed with the others in the generated reference (formats.md, `rwgpu explain limits`, a new project's
AGENTS.md). Entities in a scene take their number from the table's entities in a world, so a scene can
never hold more than a world may. Over a limit is reported where it goes over, before the work it would cause, and
nothing after that point in the file is read. Measured on an Apple M5 Pro, release:

| limit | number | measured at and around it | room |
|---|---|---|---|
| bytes in a file | 64 MiB | town of 500,000 (29 MB): `rwgpu check` 1.0 s, 2.8 GB; 1,000,000 (59 MB): 2.0 s, 5.6 GB. About 95 bytes of memory per byte of a scene at its peak | the 500,000 town 2.2× |
| entities in a scene (each cell of `cells` one) | 1,000,000 | 500,000: check 0.8 s, first step 139 ms; 1,000,000: 1.7 s and 275 ms | the 500,000 town 2× |
| cells of a ground | 4096 × 4096 | a byte a cell; 67,108,864 cells load in 0.4 ms and build in 1.1 ms. The simulation's cell index is dense up to this size; the 1,000,000 town stands on 2000 × 2000 | 4× that town |
| cells covered in a scene, every `footprint` counted in full | 2048 × 2048 | one footprint of 4,194,304 cells: check 0.9 s, world 0.5 s, 563 MB; 4 × 4 houses covering as many: 1.2 s, 2.5 GB. Four times as many: 4.3 to 5.3 s and 1.7 to 7.1 GB | the 1,000,000 town (1,000,000 cells) 4× |
| prefab bytes copied in a scene (a prefab's size, once per entity made from it) | 128 MiB | 50,000 copies of a 4 KB prefab (215 MB): check 0.38 s, 2.7 GB; 30,000 (125 MiB): 0.24 s | the 500,000 town with a 256-byte prefab |
| characters in a name or a key | 128 | "did you mean" compares names letter by letter, so a long unknown name costs its length times every other name's | the examples' longest, 15: 8× |
| tables in a playtest ([[input]], [[expect]], [[shot]]) | 100,000 | 100,000 inputs check in 75 ms; 1,000,000 in 0.87 s and 3 GB. The runtime reads every table at every frame (ADR 0027's area) | the examples' longest playtest, 37 tables |

The rest was looked at and needs no new limit: a prefab cannot name a prefab, so there is no nesting; a
playtest's length (`frames`), `hold` `for`, `step_frames`, `count`, a display and a label's `scale` are
ranges of the schema already; a text's length (a label, a prop string) is bounded by the file, and drawing
it is the render side; [[connect]] tables, actions and `when` items are read once, in linear time.

Every limit is far above what anything in the repository needs: the three example games, the oracle's
generated worlds (the simulation's, through the loader), `sim-bench`'s four worlds as text, and
`load-bench`'s worlds at their default sizes all load as before. Nothing in the repository reaches one.

### 4. The oracle

`rwgpuengine_data/src/corpus.rs` records, for 474 file sets, everything an agent sees: every diagnostic
with its code, severity, file, line, column, width, message, hint, notes and source line, in order, four
ways (`check_project`; `load_project`; one `SceneLoader` per scene, the order a session shows;
`load_playtest` per playtest), and a hash of the Debug form of every definition that loads. `PINS` holds
a hash of each record.

The corpus: the three examples; every input of the loader tests in `tests.rs`; about 250 inputs made to
reach each place the loader reports a problem, each group also all at once in one scene and spread over
the files of one project; tie cases for hints; generated worlds of 1,000 entities; and 21 cases at and
over each limit. Every code the loader can report (23 with E0209) is reached alone and with others
(E0001 only alone: `load_playtest` stops at a missing file). The pins of the 453 cases without `limits/`
were recorded with the old loader, before anything changed, and hold in every later commit. On the step
branch after piece 2, three of them changed with its new event kind `omitted`, by that word only: an
invalid `event` value's message lists the kinds (`playtest/expect-unknown-event`, and `playtest/all` and
`everything`, which hold it). They were recorded again with the old loader on that base, where the old and
the new loader again gave records identical byte for byte.

## Options that were measured

- **The order of problems.** Sorting the problems by their place at the end cost the web module 4.6 KB
  (a stable sort for a new type); merging each stage's list, already in order, costs little. Merged.
- **Defaults.** A cache of every default, parsed once (`OnceLock` and a map), made decoding 2.2× faster and
  cost the web module 10.7 KB; reading the literals without the parser is as fast and costs 2.5 KB.
- **"Did you mean" on the error path.** Exact and faster: `crowded` at 5,000 went from 7,755 ms to 542.
  It still compares every unknown name with every name: 20,000 takes about 10 s. A bound that keeps the
  hints means a different structure for names (an index of near words); what is cheap changes what agents
  see. See [Open](#open-for-the-owner).

## Evidence

### Before and after

`rwgpu check` stage of `cargo xtask load-bench`, release, Apple M5 Pro, ms:

| world | 1,000 | 5,000 | 20,000 | 50,000 | µs per entity, 5,000 → 50,000 |
|---|---|---|---|---|---|
| town, before | 3.8 | 25.9 | 279 | 1,635 | 5.2 → 32.7 |
| town, after | 1.3 | 7.2 | 29.5 | 78.7 | 1.44 → 1.57 |
| plates, before → after | 3.9 → 1.3 | 24.0 → 7.2 | 185 → 30.4 | 935 → 76.1 | 1.44 → 1.52 |
| wired, before → after | 5.1 → 2.6 | 36.3 → 13.7 | 323 → 57.3 | 1,625 → 156 | 2.74 → 3.13 |
| build, before → after | 4.7 → 1.4 | 23.2 → 8.1 | 95.7 → 33.2 | 244 → 85.5 | 1.62 → 1.71 |
| linked, before → after | 20.2 → 11.0 | 183 → 57.0 | 1,707 → 237 | 12,777 → 602 | 11.4 → 12.0 |
| crowded, before → after | 271 → 26 | 7,755 → 542 (5,000) | | | errors and warnings: still pairs of names |

`rwgpu check` of the 50,000 town as a command: 1.80 s → 0.10 s. Peak memory is unchanged, 284 MB (about
5.7 KB an entity, most of it the parser's document). At 500,000: 0.99 s and 2.8 GB. Parsing is now 70% of
the town's check.

### The pins and the faults

- Every pin holds after each commit. Running the old and the new loader on all 453 cases gave records
  identical byte for byte (960 KB).
- 15 faults planted in the old loader one at a time: 14 noticed; the 15th (no `dedup` in `check_project`)
  changes nothing for any input found. Swapping any two adjacent steps of `order.rs`, or sorting unstably,
  is noticed, except source before signal, which no input can show (a problem with the source leaves no
  source to check the signal of).
- The simulation's oracle, small and large worlds, all through the loader: every pin holds.
- `cargo xtask sim-bench --loader` makes the four worlds from their scene text: the state hash at the last
  frame is the same as from definitions, at 1,000 and 50,000.

### Sizes and budgets

- The web module: 680,792 → 704,851 bytes on the step branch after piece 2 (limit 800,000), so this piece
  costs 24,059 bytes; it cost the same on the branch before piece 2 (658,555 → 682,612): the pipeline and
  the speed-up 14.8 KB, the limits 9.3 KB. `rwgpu`: 2,234,912 → 2,334,496 (limit 3,000,000), both built
  here with `cargo xtask dist --agent`. The first versions were larger: a sort (4.6 KB), a cache of
  defaults (10.7 KB) and a map keyed by `char` (4.8 KB), all replaced as above.
- The existing time budgets, before → after (two runs, and on the branch after piece 2): render 1,587 →
  1,573, 1,629 and 1,431 µs, paint 160 → 160, 164 and 159, the reference town 5,041 → 5,036, 5,285 and
  5,005, the smoke chain 382 → 405, 298 and 434 ms, the incremental rebuild 569 → 528, 543 and 629 ms;
  piece 2's step 1,197 µs of 2,000. Noise.
- The gates, warm, this Mac: `cargo xtask ci` 19.2 s before, 23.5 s after: the data crate's tests take
  2.1 s instead of 0.6 (cases as large as the limits, recorded on several threads) and the new budget step
  about a second. `cargo xtask gpu`: 38.8 s warm after (95 s on its first run here, before).

## The budget

`time.load_check_ms = 250`: load and check the study's town of 50,000, written as files, as `rwgpu check`
does; the median of 7 runs (`cargo xtask load-check`, which `budgets` runs as a release build). Measured
76 to 83 ms; the limit leaves 3×. A return to the square (1,635 ms) fails here and on a CI runner at 4×.

## Found, and left as it is

The oracle pins these as they are; changing any of them changes what agents see.

- **Hints for the same misspelled name differ by key.** `from` suggests from the sorted names; `to`,
  `cost`, `when` and [[expect]] in file order. Two names equally close give different hints.
- **A ground of more than 255 terrains is dropped without a diagnostic**, and the scene loads with no
  ground.
- **A position near `i32::MAX` with a footprint** overflows `footprint_cells` in a debug build; a release
  build wraps.
- **Memory.** About 95 bytes per byte of a scene at the peak of `rwgpu check`, the same as before: the
  parser's document and the node tree side by side.
- **Every diagnostic carries its source line.** Many problems on one very long line make a large output;
  the work is the output's size.

## Open, for the owner

- **"Did you mean" with many unknown names.** Hints are exact and 10 to 14 times faster, but a file with N
  unknown names in a scene of N names is still N² comparisons (20,000: about 10 s). Either an index of near
  words that gives the same hints (more code), or a bound on how many hints one file gets (the rest say how
  to add the name): the second changes the output of such files.

## Not done

- The error path above. The parser's memory. A shared index of names between scenes.
- The render side at scale (not this step).

## Revisit when

- A game needs a larger scene, ground or footprint than the limits allow: measure again with
  `cargo xtask load-bench` (`ground`, `footprint`, `houses`, `copies`, `inputs` probe each limit).
- The parser is changed (ADR 0003): parse time and memory are most of what is left.
