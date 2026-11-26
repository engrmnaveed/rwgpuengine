# Contributing to rwgpuengine

Thank you for looking. The project is young and its direction has just changed to 3D
([vision](docs/vision.md), [3D architecture proposal](docs/proposals/3d-architecture.md)), so the most useful
contributions right now are small and well-aimed: a bug with a failing playtest, a diagnostic that could
say more, a platform where the gate fails.

## Before you write code

- Read [AGENTS.md](AGENTS.md). It is the short version of the rules for changing the engine, for people and
  for coding agents alike: determinism is a contract, few dependencies and no proc macros in the agent's half,
  layers point down only, every problem is a diagnostic with a stable code.
- For anything larger than a fix, open an issue first. Features enter the engine when a reference game needs
  them ([roadmap](docs/roadmap.md)), and decisions are recorded as [ADRs](docs/decisions/README.md) with the
  options and the measurements.

## The gate

```bash
cargo xtask ci
```

It must pass before a pull request is reviewed. It starts with the repository check (no home path, no build
output, no oversized file: see [privacy](docs/conventions.md#privacy)), then runs formatting, clippy for
native and wasm, the tests, the generated-docs check, the example playtests, the budgets in
[budgets.toml](budgets.toml), and the check that the wasm module reaches the native command's state hashes
and pixels (that one needs Node.js). If your change alters a state hash or a reference image, say so in the
pull request and show why.

After changing commands, file formats or diagnostic codes, run `cargo xtask docs` and commit the regenerated
reference.

## Commits and pull requests

- Conventional commit subjects (`fix(sim): …`), with a body that says what was measured and verified, and
  what is not done ([conventions](docs/conventions.md#commits)).
- One logical change per pull request.
- You are responsible for every line you submit, however it was written. If a coding agent wrote part of it,
  say so in the pull request, and make sure you understand and have tested the change yourself.

## Licence

By contributing you agree that your contribution is licensed under the
[Apache License, Version 2.0](LICENSE), the licence of the project.
