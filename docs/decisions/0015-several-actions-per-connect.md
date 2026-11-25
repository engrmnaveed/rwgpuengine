# ADR 0015: Several actions per `[[connect]]`

- Status: accepted and built, 2026-11-25
- Decided by: the project owner approved building it ("Yes, build it"); the shape by the KPI rule (ADR 0011)

## Context

After builder kit v2 (ADR 0014), the remaining cost was volume. Run #8 took 41 `[[connect]]` tables for a
small park game, 12 of them for four buttons. The agent ranked "several actions in one `[[connect]]`"
first. Across the three builder games, merging connections that share source, signal and `when` would cut
park / farm / Tiny Town from 41 / 30 / 27 to 27 / 19 / 24.

Separate connections also have a trap: each re-checks its own `when` after the earlier ones have acted. With
"pay, then say *bought*" both gated on `coins >= 3`, paying the last 3 coins makes the second one skip.

## Decision

A `[[connect]]` takes either `action` (with `to`/`prop`/`value`/`prefab`, unchanged) or
`actions = [{ action = ..., to = ..., ... }, ...]`. Each item has exactly the keys of a single action
(`slate explain action`).

| rule | why (and the option it beat) |
|---|---|
| `when` is checked **once**, before the first action | The list is one decision. Re-checking per item (as separate connections do) was the trap above. A per-item `when` would turn a list into a small program; write another `[[connect]]` for that. |
| Actions run **in order** and each logs its own `action`/`skipped` event | The log reads the same as separate connections, so `[[expect]] event = "action"`/`"skipped"` with `entity` keeps meaning "this target". One event per list was shorter, but `entity` would then have no single answer. |
| A **refused `spawn` stops the rest** of its list (each later action logs `skipped`, `stopped_by = "refused spawn"`) | `[spawn, pay]` is what an agent writes first. Without the stop it pays for houses that were never placed, a silent bug. With it, the shortest program is correct and needs no `spawned` connection. |
| `action` and `actions` are exclusive; with `actions`, `to`/`prop`/`value`/`prefab` may not be on the [[connect]] | One way to read each table. A stray top-level `to` beside a list is a mistake, not a default (E0105). |
| No new names for signals, events or codes | A list is the same actions, grouped. Misuse reuses E0104/E0105, and item errors point at the item. |

One ordering consequence is documented in the recipe. Paying inside a `[spawn, pay]` list happens at once,
so a "not enough coins" connection on the same tap must come **before** the spawn in the file: connections
on one signal run in file order and see earlier changes. That rule already existed. Paying on `spawned`
remains the order-free alternative.

## Evidence

Tiny Town regrouped:
- Each button is `set tool` + `clear hint`, so the separate `from_prefab = "button"` clear goes.
- Each spawn pays in its list, so the three `spawned` connections go.
- Each clear is `disable self` + `refund`.
- 27 → 20 connections and 354 → 299 lines. All three playtests end in the **same state hashes as before**
  (`82c150f3d192812e`, `9fee04d78014de93`, `33f47acceed0a2d8`), natively and in the web player, with the same
  reference images: the same game in fewer tables.
- `refusals` gained a guard for the new ordering rule. Two mutation checks fail it:
  - paying before the spawn (`[pay, spawn]`) charges for refused houses;
  - moving the coin hints after the spawns shows "needs 1 coin" right after the last coin is spent.

## Consequences

- **Hashes.** Old scenes need no change and keep their exact hashes: iso_garden `469ae7c92124d43d`,
  coin_vault solve `847b98297f2eefd8`, smoke `94fd42c60d7a4c92`.
- **Size.** The wasm grew from 657,589 to 664,154 bytes (+6.6 KB: 4.4 KB of code, mostly per-item loader
  checks; budget 700,000).
- **Model.** `ConnectDef` now holds `actions: Vec<ActionDef>` and the runtime `Connection` holds
  `actions: Vec<Act>`. A single `action` is a list of one, so there is one code path.

## Acceptance

**Result ([run #9](../experiments/2026-11-25-cold-agent-9.md)): met.** A cold agent got run #8's park task
word for word. Every requirement was supported with 0 warnings, in **24 connections instead of 41** (−41 %;
the estimate was 27), 369 scene lines instead of 470, and 5 min 54 s instead of 6 min 24 s. It used
`[spawn, pay, clear message]` lists and put the coin messages before the spawns, as the recipe says. Its
findings are fixed:
- a short flag (`-o`) was read as a path;
- list messages now say "action 3 of 3";
- `[[expect]] event = "action"` takes `changed = true`;
- three recipe facts.

## Not built

A per-item `when`; actions that target the copy just spawned (use its `spawned` signal); "else" branches.
Run #8's next requests stay candidates: a "not equal" condition, prefab costs, per-terrain refusal reasons.
