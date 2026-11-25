# ADR 0014: Builder kit v2 — the `refused` signal and `counts` props

- Status: accepted and built, 2026-11-25
- Decided by: the project owner approved building both ("Yes, build them"); the shape of each by the KPI rule
  (ADR 0011)

## Context

Tiny Town (ADR 0013) and cold-agent run #7 (a farm builder) hit the same two gaps:

1. **A refused placement is silent.** `spawn` logs `blocked`, but the game cannot react, so there is no
   "can't build there". Run #7's agent called this the most important missing feature for a builder game.
2. **Counting copies by hand.** Both games keep counters ("Houses 2/2", "a barn and 3 crops") that two
   connections per prefab must keep right: `+1` on `spawned`, `−1` on the remove tap. That's 6 of Tiny Town's
   26 connections, and every one is a chance to forget the matching half.

## Decision

### 1. `refused`: the source of a refused spawn says so

When a `spawn` cannot be placed (reason `edge`, `terrain` or `occupied`), the source of the signal that
started it emits `refused` on that cell, if a `[[connect]]` listens. That source is the ground for ground
taps, or else the emitting entity. A new condition, `when = [{ reason = "terrain" }]`, picks one reason. It
has no entity and is valid only on `signal = "refused"`.

| option | why not |
|---|---|
| one signal per reason (`refused_terrain`, …) | three connections for the common "can't build there" case. With `refused` + `reason`, the common case needs one connection and a per-reason message needs one each. |
| put the reason in a value (`value = "{reason}"`) | the player would see `terrain`. Games need their own words ("Build on grass!"). |
| also emit `refused` when the spawn's `when` fails ("not enough coins") | wrong in practice. In a toolbar game every other tool's spawn connection is also skipped on each tap, so "can't build" would fire on every tap. A `when` failure stays `skipped`. The documented answer is a connection with the opposite condition. |

### 2. `counts`: props the engine keeps equal to the number of copies

`counts = { houses = "house" }` on an entity declares an integer prop `houses`. It always equals the
number of enabled `house` entities, placed in the scene or spawned. It changes in the same step as the
spawn, `disable`, `enable` or `toggle` that changes the number (and after custom Rust systems). Each change
is logged as a new `counted` event, and the owner then emits `changed`. Labels (`{houses}`), `when`
conditions and `[[expect]] props` read it like any prop. `add`/`set` on it is an error.

The approved request was "`when` conditions that count copies" (`{ prefab = "crop", at_least = 3 }`). A count
prop delivers that and fixes two problems a bare condition leaves:

| | condition `{ prefab = "crop", at_least = 3 }` | `counts` prop (built) |
|---|---|---|
| re-check when the number changes | nothing emits on a removal. The game must hang the check on the remove tap, and file order decides whether it sees the copy before or after it is disabled. | the owner emits `changed` after the count moves, so the check always sees the new number |
| show the number ("Houses 2/2") | still needs a hand-kept counter | `{houses}` in a label |
| concepts to learn | a second kind of condition | none: it's a prop; conditions are unchanged |
| hand wiring per prefab | the counter still needs 2 connections | 0 |

A condition is then just `{ entity = "job", prop = "houses", at_least = 2 }`.

## Evidence

Tiny Town now uses both:
- The job card is `counts = { houses = "house", trees = "tree", wells = "well" }`, and the 6 counting
  connections are gone.
- A hint line says "Build on grass!" / "No room there!" (from `refused`) or "A house needs 3 coins" (the
  opposite of the spawn's `when`). It clears on the next ground or button tap.
- 26 → 27 connections, with 7 new ones adding feedback the game could not give before. The wiring for
  counting went from 6 connections to 0.
- `refusals` checks every hint and the counts (23 expects, up from 14).
- Two mutation checks fail exactly the expectations that cover them: swapping the two reason messages fails
  4; counting the wrong prefab fails `build_town` and `refusals`.
- AGENTS.md gains a recipe with a checked snippet ("Count what was built, and say why nothing was").
- The limits row for refusal feedback is replaced by the narrower real limit: a `when` failure emits
  nothing.

## Consequences

- **Hashes and images.** iso_garden, coin_vault and smoke keep their exact state hashes. Tiny Town's states
  change because of the new hint entity: `build_town` is now `82c150f3d192812e`, `clear` `9fee04d78014de93`,
  `refusals` `33f47acceed0a2d8`. The web player matches natively. Its reference images are pixel-identical,
  because the hint is empty at shot time.
- **Size.** The wasm grew from 641,758 to 657,498 bytes (+15.7 KB, +2.5%; budget 700,000). Setting counts
  through `get_mut` instead of `BTreeMap::insert` saved 3.2 KB of map code. 42 KB of headroom is left, and
  `opt-level = "z"` (−40 KB measured in ADR 0013) is still the reserve.
- **Cost.** A count refresh is O(entities × count props) per enabling action or spawn, plus one per step
  when counts exist. Tiny Town has 3 count props over 20–30 entities.
- **New names.** Event kind `counted`, signal `refused`, condition key `reason` (`REFUSE_REASONS`), entity
  and prefab key `counts` (`Ty::Counts`). No new diagnostic codes: misuse is E0105/E0201/E0207/W0203.

## Not built

A placement anchor (centring a multi-cell building on the tap, run #7's third request); per-prefab cost
tables; a signal for `when` failures. Each waits for a game that needs it.

## Acceptance

Accepted on Tiny Town's playtests and the mutation checks above, and by a cold agent:

**Result ([run #8](../experiments/2026-11-25-cold-agent-8.md)): met.** It was a park builder with three
refusal messages (terrain, taken, coins), a sign whose counts include two trees already in the scene, and a
"park is open" goal that switches off again. Every requirement was supported. It wrote 6 playtests (78
expects) with 0 warnings in 6 min 24 s, the fastest builder run so far. It used `counts` with no counting
connections, and `refused` with all three reasons, including `edge`, which it found on its own. Its findings
were output noise and one fact that lived in a different recipe; both are fixed. Its top request is several
actions per `[[connect]]`.
