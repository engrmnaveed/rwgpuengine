# ADR 0016: Spawn costs and the `funds` refusal

- Status: accepted and built, 2026-11-25
- Decided by: the project owner approved building it ("Yes, build costs"); the shape by the KPI rule
  (ADR 0011)

## Context

Runs #8 and #9 both asked for prices as data with a "can't afford" refusal. In every builder game so far,
a price took three hand-written parts:
1. a `coins >= price` condition on the spawn;
2. a payment (an `add` after the spawn, or on `spawned`);
3. a mirror connection with `coins <= price − 1` to say "not enough coins".

That mirror connection was the last ordering trap in builder games (ADR 0015). It had to come before a
spawn that pays in its list, or it fired right after the last coins were spent. Run #9 placed it correctly,
but its tests could not tell; a mutation passed all 7 of them.

## Decision

A `spawn` action takes `cost = { entity = "town", prop = "coins", amount = 3 }`.

| rule | why (and the option it beat) |
|---|---|
| The cost lives **on the spawn action** | A prefab is scene-independent and cannot name `town`. The cost on the prefab plus a "payer" on the spawn would split one fact across two files. |
| **Funds are checked first**, before the cell | With too few coins, no tap anywhere can succeed, so "not enough coins" is the useful message even over water. Run #8's agent guessed this order unprompted. |
| Too little means **`blocked` + `refused` with reason `funds`** | It is the same path as `edge`/`terrain`/`occupied`. One `refused` connection with `when = [{ reason = "funds" }, ...]` replaces the mirror connection and its ordering rule. |
| **Paid only if placed**, in the same step; the payer then emits `changed` | A refused placement costs nothing, whatever the reason. HUD logic that listens to `changed` sees the new number. |
| The payment is logged **in the `spawned` event** (`paid`: before/after) | One purchase is one event. A separate `action` event was another line to read for the same fact. |
| **Refunds stay explicit**: an `add` in the removal's `actions` | Refunding automatically on `disable` would be magic: a copy can be disabled for other reasons. Scene-placed copies cost nothing yet some games refund them (run #8's old trees). |
| The amount is checked: above 0, a number prop, not a `counts` prop, an integer for an integer prop | Each is a mistake an agent can make, and each is caught by `slate check` with a hint, using existing codes (E0202/E0206/E0207/E0105). |

## Evidence

Tiny Town:
- The three spawns drop their coin conditions and become plain `spawn` + `cost`.
- The three "needs N coins" messages are `refused` connections with
  `when = [{ reason = "funds" }, { tool }]`.
- The comment explaining the file-order rule is gone, because the rule no longer applies.
- 20 connections (unchanged in number), 299 → 291 lines, and **no condition mentions coins any more**.
- All three playtests end in the same state hashes, natively and in the web player, with the same images.
- `refusals` now proves the coin case as what it is: `event = "blocked"`, `reason = "funds"`,
  `prefab = "tree"`, none before the last tap and exactly one after it.
- Mutation checks:
  - a wrong price (house 2) fails all three playtests;
  - a funds message wired to the wrong tool fails `refusals`.

## Consequences

- **Hashes.** Old scenes are unchanged: iso_garden `469ae7c92124d43d`, coin_vault solve
  `847b98297f2eefd8`, smoke `94fd42c60d7a4c92`.
- **Size.** The wasm grew from 665,106 to 674,719 bytes (+9.6 KB):
  - about 3 KB of loader checks;
  - 2 KB of funds and paying in the spawn;
  - 1.8 KB of doc strings.

  Headroom is now 25 KB (budget 700,000). A size pass (`opt-level = "z"`, −40 KB measured, frame time
  unmeasured) should come before the next large feature.
- **New names:** the refusal reason `funds` (in `REFUSE_REASONS` and `BLOCK_REASONS`) and the spec `cost`.
  No new codes.

## Not built

Automatic refunds, costs in several currencies, and a price shown by `inspect --place`, which stays
placement-only.
