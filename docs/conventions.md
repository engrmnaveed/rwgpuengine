# Conventions

Rules for working on rwgpuengine itself. The short version lives in the root [AGENTS.md](../AGENTS.md).

## Commits

Commit messages are the project's running progress log. Format:

```text
type(scope): what changed, in the imperative

Why it changed and what it enables. What was verified and how (tests,
commands, measurements). What is known not to work yet. What is next.
```

- `type`: `feat`, `fix`, `docs`, `build`, `refactor`, `test`, `perf`, `chore`.
- `scope`: crate or area without the `rwgpu_` prefix (`core`, `data`, `sim`, `render`, `runtime`, `cli`,
  `web`, `xtask`, `docs`), comma-separated when several.
- Bodies state measured facts (hashes, sizes, timings) rather than adjectives.
- One logical step per commit; `cargo xtask ci` passes on every commit.

## Determinism

The contract is [ADR 0005](decisions/0005-determinism-model.md). In engine crates:
- no libm transcendental functions, `HashMap`/`HashSet`, or wall-clock time (clippy enforces all three);
- round with `floor(x + 0.5)`; keep randomness on the world's RNG, drawn in entity order;
- anything that changes recorded hashes or reference images says so in the commit message.

## Diagnostic codes

- Add codes only in `rwgpuengine_core::codes`; take the next free number in the right range.
- Never reuse or renumber a released code; retire it instead.
- Every diagnostic should say *where* (file/line/column via `.at(src, span)`) and, when we can know it,
  *how to fix it* (`.hint(...)`). Prefer `did_you_mean` suggestions over lists.

## File formats

- Every key is declared once in `rwgpuengine_data::schema`; defaults are TOML literals there.
- Unknown keys are errors. Adding a key: `Field` in the registry → decode in `defs.rs` → behavior →
  `cargo xtask docs` (regenerates docs/reference) → a test.
- Format changes that break existing files need a migration note (from M2: machine-readable).

## JSON output

- One object per command with `--json`; envelope fields `rwgpu`, `format`, `command`, `ok`, `errors`,
  `warnings`, `diagnostics` are a public contract.
- Breaking changes to the envelope or a command's fields bump `rwgpuengine_core::JSON_FORMAT_VERSION`.
- Output is deterministic (ordered keys, no timestamps) so it can be diffed and golden-tested.

## Dependencies

- Owned crates only depend downward (see [architecture.md](architecture.md)).
- No proc macros in the runtime tree; new rented crates are justified in
  [ADR 0004](decisions/0004-few-dependencies-no-proc-macros.md).

## Budgets

- Limits live in `budgets.toml`; `cargo xtask budgets` measures and fails when over.
- Limits only go up in a commit that explains why and shows the measurement.

## Formatting and lints

- `cargo fmt` with `rustfmt.toml` (width 120, "Max" heuristics: dense code reads faster).
- `cargo clippy --workspace --all-targets -- -D warnings`, native and wasm32.
- `#[allow(...)]` needs a comment saying why.

## Tests

- Unit tests live next to the code (`mod tests` / `tests.rs` per crate).
- Assert diagnostic **codes** and structure; avoid asserting full message text unless the message is the
  feature.
- Examples are tested: `examples/iso_garden` playtests run in CI, and its reference images are reviewed
  by eye before they are committed.

## Toolchain

- Pinned in `rust-toolchain.toml` (1.100.0 + wasm32). Bump deliberately in its own commit, after
  `cargo xtask ci` passes and reference images still match.
