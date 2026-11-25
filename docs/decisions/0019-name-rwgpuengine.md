# ADR 0019: The engine is named rwgpuengine

- Status: accepted and done, 2026-11-25 (resolves open question Q1)
- Decided by: the project owner. The owner decided to rename, asked for a name after the technology the
  engine is built on (Rust and wgpu) in the form `<alias>engine`, and chose `rwgpuengine` from the
  [shortlist](../proposals/name.md).

## Context

"slate" was a working codename from the first day. `slate` and `slate_core` are taken on crates.io, and the
word is shared with a well-known editor framework, a documentation generator and a magazine. On the same day
the owner set the direction to 3D on the GPU through wgpu
([vision](../vision.md#direction-update--2026-11-25-owner)), so the name was settled before the 3D work adds
new crates.

## Decision

**rwgpuengine**: **R**ust + **wgpu** + engine. The short form `rwgpu` is the command.

| thing | was | is |
|---|---|---|
| the engine | slate | **rwgpuengine** |
| the command | `slate` | **`rwgpu`** (`cargo rwgpu …` from source) |
| crates | `slate_core`, `slate_data`, `slate_sim`, `slate_render`, `slate_runtime`, `slate_cli`, `slate_web` | `rwgpuengine_core`, `rwgpuengine_data`, `rwgpuengine_sim`, `rwgpuengine_render`, `rwgpuengine_runtime`, `rwgpuengine_cli`, `rwgpuengine_web` |
| project file | `slate.toml` | `rwgpu.toml` |
| web ABI | `slate_step`, `slate_present`, … | `rwgpu_step`, `rwgpu_present`, … |
| web module | `slate_web.wasm` | `rwgpuengine_web.wasm` |
| project bundle header | `slate-bundle 1` | `rwgpu-bundle 1` |
| environment variables | `SLATE_WEB_WASM`, `SLATE_REBUILD_PROBE` | `RWGPU_WEB_WASM`, `RWGPU_REBUILD_PROBE` |
| first key of `--json` output | `"slate": "0.1.0"` | `"rwgpu": "0.1.0"`, and `format` goes from 1 to **2** |
| JSON Schema `$id` | `https://slate.dev/schema/<name>.json` | `urn:rwgpuengine:schema:<name>` |

Choices made while doing it:

| choice | why |
|---|---|
| The project file is `rwgpu.toml`, not a neutral `game.toml` | A name no other tool uses tells an agent which tool owns the folder and what to search the docs for. |
| `slate_cli` became `rwgpuengine_cli`; no crate is named plain `rwgpuengine` yet | The rename is mechanical. Which crate game code and `cargo install` use is part of the 3D crate layout (open question Q16), and that crate takes the plain name. |
| The schema `$id` is a `urn:` | `slate.dev` was never the project's domain, and there is no project domain yet. A `urn:` claims none. |
| `format` is 2 | The first key of every `--json` output changed, which is a breaking change to a public contract ([conventions](../conventions.md#json-output)). |
| No migration notes | Nothing was ever released under the old name. |

**Old records keep the old name.** ADRs 0001–0018, the experiment reports and their archived projects, the
progress log before this entry, the baseline, the audit and the handoff brief say "slate", `slate_core`,
`slate.toml` and so on, because that is what was true when they were written, and several quote commands and
prompts word for word. They are not rewritten. Read "slate" there as rwgpuengine, and `slate …` as `rwgpu …`.
The archived agent projects under `docs/experiments/` still have a `slate.toml`; to run one with today's
command, rename that file to `rwgpu.toml`.

## Evidence that only the name changed

Measured between the build before the rename and the build after it:

- **99 outputs and files, 0 different** once the name is substituted in the old ones: the text and JSON
  output of seven commands on each example; the PNG files of `shot`; sixteen project-independent commands,
  including the whole generated AGENTS.md and the four JSON Schemas; the project `new` writes, file by file;
  and three commands run on that project.
- **All nine example playtests end in the same state hashes** (`82c150f3d192812e` for Tiny Town's
  `build_town`), and the smoke chain in `94fd42c60d7a4c92`.
- **The web module** paints pixels that hash to native `shot`'s image hashes for all nine playtests.
- The generated AGENTS.md is 67,313 bytes before and after: "slate" and "rwgpu" are the same length.
- Sizes: the CLI is 1,912,336 bytes (it was 1,911,904) and the web module 585,789 (it was 585,725). The
  difference is longer crate names inside strings.
- `cargo xtask ci` is green: 135 tests, all budgets. The dependency budget caught the one place the script
  got wrong: the counter that skips the engine's own crates still looked for the short prefix, so it counted
  21 external crates instead of 16 until it was fixed.

## Consequences

- crates.io, npm and GitHub were checked on 2026-11-25: `rwgpuengine`, `rwgpuengine_core`, `rwgpuengine_engine`
  and the alias `rwgpu` are free. Nothing is published or reserved yet; that is part of the open-source
  readiness step.
- The name commits the engine to wgpu in public. That matches the owner's decision that the renderer is
  written on wgpu; if that ever changed, the name would be wrong.
- "rwgpu" is hard to say aloud. The owner chose it knowing that.
- Two unrelated repositories on GitHub are named `rwgpu` (no stars; one is a set of wrappers around wgpu
  types). Neither is a crate.

## Revisit when

Never for the name. The plain `rwgpuengine` crate is defined with the 3D crate layout.
