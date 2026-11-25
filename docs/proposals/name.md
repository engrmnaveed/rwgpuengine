# Proposal: the engine's name (open question Q1) — not decided

> Status: **shortlist for the owner**, checked on 2026-11-25. The owner decided to rename and asked that the
> name may carry the 16xbrains family branding, like `diskuse16x` and `macdisplay16x`.

## What the family pattern is

`<what it is about>16x`: one plain lowercase word or compound, then `16x`. The existing tool is a Rust project
published on crates.io under exactly that name. The suffix form is also the only one Rust allows: a crate name
and a Rust identifier cannot start with a digit, so `16x…` is not possible.

## What was checked

| check | result |
|---|---|
| 42 candidate names of the form `<word>16x` on crates.io, each also with `_core` | all free |
| the same 42 on npm (a web player may be published there) | all free |
| the ten finalists as a GitHub repository name and as a GitHub account name | none in use |
| web search for the finalists as exact strings | no pages found |
| is "16x" used by someone else near this field? | **Yes: "16x Prompt"**, a desktop app for writing prompts for AI coding assistants, at `prompt.16x.engineer` |

Because every candidate is free, availability does not choose between them. Meaning, daily use and
neighbours do.

## Shortlist

The command is typed in every step of the agent's loop, so each row shows how it reads there.

| name | reads as | for | against |
|---|---|---|---|
| **`game16x`** (recommended) | `game16x new town` · `game16x check` · `game16x test` | Follows the family pattern exactly. Seven characters. Covers every kind of game in the direction (strategy, city builders, car-building, simulation, children's). Commands read as sentences. | A plain word; it is distinctive only through the suffix. |
| `forge16x` | `forge16x new town` | More of a product name; a verb that suits making things. | "Forge" is crowded in software and in game technology (a rendering framework, a modding platform, a build tool all use it). |
| `diorama16x` | `diorama16x new town` | Says what the art direction is: small 3D worlds built from simple shapes. Nothing else in game technology uses the word. | Ten characters before the suffix is typed. Fits a town better than a car-building game. |
| `sim16x` | `sim16x run` | Six characters. Names the engine's strongest part, the deterministic simulation, and fits strategy, city and simulation games. | A children's fun game is not a "sim". |
| `play16x` | `play16x run` | Friendly; fits children's games and strategy alike. | Sounds like a media player, or like playback at 16× speed. |
| `world16x` | `world16x inspect` | The queryable world is the engine's centre. | The vaguest of the six. |

Not on the list, on purpose:
- **`engine16x`**: the most literal name, but one letter away from the other vendor's "16x.engineer". In the
  field of AI coding tools the two would be mixed up.
- **`gamekit16x`**, **`gamemaker16x`**: "GameKit" and "GameMaker" are existing products' names.
- **`anvil16x`**, **`atlas16x`**, **`tessera16x`**, **`lattice16x`**: each bare word is already a game engine,
  game or game tool.

## Recommendation

**`game16x`.** It is the one that fits the family, covers all the kinds of game the owner listed, and makes
the commands read naturally. `forge16x` is the pick if a more product-like name matters more than the
family pattern.

A note for the owner, not a recommendation: "16x Prompt" at `16x.engineer` belongs to someone else and sits
close to this field (tools for AI coding). 16xbrains already ships under `16x`, so this is the owner's call;
keeping "16xbrains" visible next to the name (repository owner, README, crate authors) makes the difference
clear.

## What the rename touches (after the owner picks)

| thing | today | then (with `game16x` as the example) |
|---|---|---|
| command | `slate` | `game16x` |
| crates | `slate_core`, `slate_data`, … | `game16x_core`, `game16x_data`, … |
| project file | `slate.toml` | to decide at the rename: `game16x.toml`, or the neutral `game.toml` |
| web ABI | `slate_step`, … | `game16x_step`, … |
| output envelope | `"slate": "0.1.0"` | `"game16x": "…"` (a format version bump) |
| repository | — | `16xbrains/game16x`, if the `16xbrains` account on GitHub is the owner's; it exists and has no public repositories |
| authors and copyright | — | "The 16xbrains developers" in crate metadata and the Apache-2.0 notice |

It is one mechanical commit, proven by unchanged state and image hashes, and it is best done before the 3D
work adds new crates.
