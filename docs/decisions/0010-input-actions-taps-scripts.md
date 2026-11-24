# ADR 0010: Input = abstract actions + taps; scripted input = playtest files

- Status: accepted, 2026-11-24

## Context
Tablets come first, so touch is the primary input; keyboards and scripts must drive the same game.
Scripted playtests must be text, deterministic and replayable.

## Decision
- Games see **actions** (`up down left right confirm cancel`), as `pressed` (edge) and `down` (held) sets,
  plus **taps** in frame pixels. Taps are resolved by picking the drawn frame (topmost enabled entity), so
  the simulation sees "tap hit 'lever'" — or a miss naming disabled entities under the point.
- Direction actions map to grid axes: `up` = −y, `down` = +y, `left` = −x, `right` = +x. In the web
  player, swipes follow the iso axes (up-right = up, down-left = down, up-left = left, down-right = right).
- Scripted input lives in playtest files (`[[input]]` with `press`, `hold` + `for`, or `tap`), usable by
  `test` and, via `--input`, by `run`, `shot` and `inspect`. Live and scripted input merge per step.

## Consequences
- One file format is both the test script and the replay format; M2's recorder will write it.
- Mover cooldown makes rapid presses ignorable; every ignored press is logged with reason `cooldown` so
  scripts are debuggable.

## Revisit when
A reference game needs pointer drag/hold (builders, tracing): add `drag` inputs and pointer state.
