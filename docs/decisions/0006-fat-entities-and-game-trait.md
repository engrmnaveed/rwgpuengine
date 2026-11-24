# ADR 0006: Fat entities + props instead of an ECS; Rust code via a `Game` trait

- Status: accepted, 2026-11-24; revisit at M3

## Context
The world must be easy to query and to dump as JSON ("what exists, where, why"), map 1:1 onto scene files,
and be small enough for an agent to hold in its head. Target games (learning games on tablets) have tens to
a few thousand entities. An ECS brings archetypes, queries and scheduling that agents must learn, and
usually proc-macro-heavy APIs.

## Decision
- `Entity` is one struct with `Option` fields for built-in components (`tile block sprite mover patrol
  wander trigger`) plus `props: BTreeMap<String, PropValue>` for free-form game data.
- Entity ids are indices in scene order: stable for a given file, readable in output.
- Rust game logic implements `slate_runtime::Game` (`Systems::tick` + `name`, `on_start`, `inspect`),
  runs after the built-in behaviors each step, and ships as a binary calling `slate_cli::main_with`.

## Consequences
- `inspect` output is the entity struct; scene keys, JSON keys and docs use the same names.
- Linear scans per move/trigger; fine at learning-game scale, measured by budgets if it becomes a problem.
- Custom typed components are not supported yet; props cover game data for now.

## Revisit when
A reference game needs >10k entities, spawning/despawning at scale (ids would need generations), or typed
custom components that props cannot express cleanly.
