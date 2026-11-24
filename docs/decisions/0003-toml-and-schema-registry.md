# ADR 0003: TOML formats behind a span tree; one schema registry

- Status: accepted, 2026-11-24

## Context
Text files are the source of truth and must be diffable, comment-friendly, and validated with errors that
carry a file, line and column. Agents write TOML, JSON and YAML well; TOML has comments, no indentation
traps, and `[[array-of-tables]]` maps naturally onto lists of entities.

## Decision
- All formats are TOML. Parsing is rented (`toml_edit`), but its output is immediately converted into an
  owned `Node` tree with byte spans for every key and value (`slate_data::node`). Nothing else touches
  `toml_edit`, so the format could change by rewriting one file.
- One **schema registry** (`slate_data::schema`: `Spec`, `Field`, `Ty`) describes every table. It drives
  validation (`Checker`), defaults (`Fields`, defaults are TOML literals in the registry), JSON Schema
  export (`slate schema`), reference docs (`slate docs`) and `slate explain <component>`.
- Unknown keys are errors with did-you-mean suggestions, never ignored.
- Typed decoding is hand-written (no serde derive; see ADR 0004).

## Consequences
- Adding a key is one `Field` plus one line of decoding; docs and schemas update themselves. A test checks
  every default literal is valid for its own type.
- Validation and decoding are two steps; decoding assumes validated input and falls back to defaults.

## Revisit when
A format needs something TOML expresses badly (deep nesting, large generated data); a second format can be
added behind the same `Node` seam.
