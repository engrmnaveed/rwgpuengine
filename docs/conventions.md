# Conventions

Rules for working on rwgpuengine itself. The short version lives in the root [AGENTS.md](../AGENTS.md).

## Privacy

The repository is public, and every push publishes at once. Never commit:

- an absolute home path of anyone, as macOS, Linux or Windows write it (`/home/<name>/`, `C:\Users\<name>\`
  and the like). Write paths relative to the repository; an example that needs a home folder uses the
  made-up name `someone`;
- a name or a personal email, in files, commit messages or pull requests. Commits use the repository's own
  git identity;
- build output: `target/` or `target-*/` folders, Cargo's `.d` files, objects, libraries, programs.

How to work:

- Stage by explicit path (`git add <path>`), never `git add -A` or `git add .`. Read `git status --short`,
  untracked files included, before each commit.
- Build scratch and cross builds outside the repository (`--target-dir` in a temporary folder), or in a
  folder `.gitignore` already covers.
- Before a push, check every outgoing commit, not only the tip: `git log -p <base>..HEAD` must hold no home
  path, and `git diff --stat <base>...HEAD` must list only the files you meant to change.
- Release artifacts come from `cargo xtask dist`, never from a plain `cargo build --release` or
  `cargo install`: only xtask remaps this machine's folders. A plain install is fine for one's own machine.

How the gate enforces it ([ADR 0023](decisions/0023-release-builds-without-local-paths.md)):

- `cargo xtask repo-check`, the first step of `cargo xtask ci`, reads every tracked file. It fails on a home
  path of any user, on build output (by folder, by name, or by a program's first bytes), and on a file over
  `[repo] file_bytes` in `budgets.toml`. Untracked build output that `.gitignore` misses fails too. The
  only exception is the placeholder name `someone`, in `xtask/src/repo.rs`.
- After every release build, xtask reads the artifact's bytes for this machine's home folder, the repository
  and Cargo's home. `cargo xtask ci` checks the `rwgpu` command and the web module; `cargo xtask gpu` the
  players' programs (`gpu_check` and `rwgpu-player`) and the web player's files.

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
- That holds for the agent's half (`crates/`, `xtask`). The players' half is the workspace in `players/`,
  with wgpu and its own limits ([ADR 0021](decisions/0021-wgpu-renderer-held-to-the-reference.md)). Its crates
  may depend on the agent's; never the other way round.

## Budgets

- Limits live in `budgets.toml`; `cargo xtask budgets` measures and fails when over. The `[players]` section
  is measured by `cargo xtask gpu`.
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
