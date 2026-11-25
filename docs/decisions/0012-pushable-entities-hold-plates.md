# ADR 0012: Pushable entities hold pressure plates

- Status: accepted and built, 2026-11-25
- Decided by: the project owner's go-ahead plus the KPI rule (ADR 0011); evidence from cold-agent runs #3–#5

## Context

The vault task (two plates held at once) needs two plate holders, but the only thing that sets off a
trigger is an actor, and every mover follows the same input. Runs [#3](../experiments/2026-11-25-cold-agent-3.md),
[#4](../experiments/2026-11-25-cold-agent-4.md) and [#5](../experiments/2026-11-25-cold-agent-5.md) all built
a patrol stand-in to hold the second plate. In run #5, designing around that limit took most of the 4.5 minutes
before the first file was written, plus a probe project. All three agents ranked "objects that hold plates" as
the most useful missing feature. ADR 0011's rule for growing the engine is a pattern agents keep rebuilding from
parts, and this is that pattern.

## Options and KPIs

| option | player agency | covers static weights | new rules to learn | risk |
|---|---|---|---|---|
| **(a) `weight` flag**: an object triggers count | none (placed in data) | yes | 1 | no puzzle value beyond today's stationary patrol |
| **(b) `pushable`**: movers push it one cell; triggers count it | yes (Sokoban-style, a well-known pattern) | yes (a crate placed on a plate in data) | 1 flag, 1 block reason | crates pushed into corners: a design concern, logged |
| (c) switch control between several movers | yes | no | input routing, UI, events | large; touches input, web player and docs |
| (d) triggers count solid blocks | no | yes | 0 | doors and walls would press plates: surprising |

(b) covers (a) and adds a real puzzle mechanic for one flag. (c) stays on the candidate list.

## Decision

`pushable = true` on an entity (or prefab):
- A mover that walks into it pushes it one cell, if the cell beyond is inside the ground and holds no solid,
  actor or other pushable. Otherwise the mover is refused: a `blocked` event with reason `push`, `other` = the
  pushable, and `push_blocked` / `push_blocked_by` saying why. A successful push logs the pushable's `moved`
  event (cause `pushed`, `other` = the pusher) just before the mover's own move.
- Patrols and wanderers never push. To them it blocks like a solid (reason `solid`, the message says only
  movers push).
- Triggers and `occupied` conditions count it like an actor. `only` can name it.
- It cannot have `mover`, `patrol`, `wander` or `screen_pos` (E0105). It counts as solid for W0201 and
  W0206.
- No chains (one push moves one pushable) and no pulling. Both are listed in Limits.

## Consequences

- The "Holding a plate with an object" limit is gone. The AND recipe says to push a crate onto the plate the
  player cannot stand on, and to put crates on both plates if the player must walk through.
- `examples/coin_vault` replaces its stationary-patrol statue with a crate the player pushes.
- State hashes of scenes without pushables are unchanged (the flag is only hashed when true).

## Acceptance

A cold-agent re-run of the vault task (run #6) should mark every requirement supported with no approximation
and 0 warnings, faster than run #5 (10 min 2 s).

**Result ([run #6](../experiments/2026-11-25-cold-agent-6.md)):**
- The agent built the complete puzzle: both plates held by pushed crates, and the player walks into the vault.
- 0 warnings, 7 min 41 s.
- Its table still lists the plate holder as "approximated" (no second character), a conservative reading
  prompted by our limits wording, which is now fixed. The level itself has no stand-ins.
