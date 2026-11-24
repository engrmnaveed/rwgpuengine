# ADR 0009: Stable diagnostic codes, rustc-style output, `explain`

- Status: accepted, 2026-11-24
- Source: direction doc, "Errors carry stable codes, file and line, and fix hints, like the Rust compiler's"

## Context
Agents fix errors fastest when the error says exactly where and how, and when the same problem always has
the same identifier they can search for or remember.

## Decision
- Every reported problem is a `Diagnostic` with a code from the registry `slate_core::codes`:
  `E`/`W` + 4 digits; ranges `x00xx` CLI/IO, `x01xx` format, `x02xx` references/static checks, `x03xx`
  runtime/frame report, `x04xx` playtests. Codes are **never reused or renumbered**.
- Each carries file, 1-based line/column, the source line, an optional hint (usually "did you mean `x`?",
  from an edit-distance suggester) and notes.
- Text output mimics rustc (`error[E0103]: … --> file:line:col`, caret, `= help:`); JSON output carries
  the same fields.
- `slate explain <CODE>` prints the long explanation; docs/reference/errors.md is generated from the
  registry.

## Consequences
- Tests assert codes, not message text, where possible; messages can improve freely.
- A test enforces unique codes and the E/W prefix matching severity.

## Revisit when
Never for the stability rule. The ranges can grow (x05xx…) as new subsystems arrive.
