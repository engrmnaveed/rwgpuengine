# Proposal: the engine's name (open question Q1) — decided: rwgpuengine

> **Decided on 2026-11-25:** the owner chose **`rwgpuengine`**, and the rename is done
> ([ADR 0019](../decisions/0019-name-rwgpuengine.md)). The rest of this page is the shortlist as it was
> put to the owner.
>
> Status then: **second shortlist for the owner**, checked on 2026-11-25. The owner decided to rename. A first
> shortlist used the `<word>16x` family suffix (`game16x` and others); the owner then asked to **drop the
> `16x` suffix** and to name the engine after **the technology it is built on, Rust and wgpu**, in the form
> **`<alias>engine`**. The first shortlist is at the end, for the record.

## What was checked

| check | result |
|---|---|
| 33 names of the form `<alias>engine` on crates.io, each also as `<alias>_engine` | all free except `rustengine` |
| the same 33 on npm | all free |
| each alias alone on crates.io (a short command or crate may use it) | 21 free; 12 taken: `rwg`, `wgpu`, `wg`, `ferro`, `ferris`, `ferrous`, `oxgpu`, `oxide`, `oxi`, `patina`, `corten`, `hematite` |
| the eight strongest as GitHub repository names | one is in use: **`wgpuEngine`**, a C++ WebGPU 3D engine with 90 stars |
| the four short aliases as GitHub repository names and on the web | `rgpu` is an existing project (remote GPUs for PyTorch, 80 stars); `rwgpu` is two repositories with no stars; `ferrogpu` and `oxigpu`: nothing |
| may a product name say "Rust"? | Sources differ. The Rust Foundation's policy page allows "Rust" in crate and repository names that refer to use with the language; another policy text prefers `-rs` and does not generally permit "Rust" inside a product name. The shortlist therefore hints at Rust and does not spell it. |

## Shortlist

All four are free on crates.io (also with `_engine` and `_core`), on npm and as GitHub repository names. The
alias in each is also free as a crate, so it can be the short command that is typed in every step.

| name | says | command | for | against |
|---|---|---|---|---|
| **`rwgpuengine`** (recommended) | **R**ust + **wgpu** + engine | `rwgpu new town` · `rwgpu check` | The only one that names both technologies plainly, which is what the owner asked for. Eleven characters. | Hard to say aloud ("R-W-G-P-U engine"). It ties the name to one library: if the renderer ever moves off wgpu, the name is wrong. |
| `oxigpuengine` | **oxi**dation (how Rust tools hint at Rust: oxipng, oxc) + GPU + engine | `oxigpu new town` | Easy to say ("oxi-GPU"). Nothing else uses `oxigpu`. Survives a change of GPU library. | Says "GPU", not "wgpu". A reader must know the hint. |
| `ferrogpuengine` | **ferro**, iron (the Rust community's own word: ferrous, Ferrocene) + GPU + engine | `ferrogpu new town` | The most name-like of the four. Nothing else uses `ferrogpu`. Survives a change of GPU library. | Fourteen characters. Says "GPU", not "wgpu". |
| `rsgpuengine` | **rs**, the Rust file ending + GPU + engine | `rsgpu new town` | Plain and short. | Reads as a string of letters; the least memorable. |

Left off, on purpose:

| name | why |
|---|---|
| `wgpuengine` | Already the name of another engine (`wgpuEngine`, C++, 90 stars). It also reads as "the wgpu project's own engine". |
| `rgpuengine` | `rgpu` already means "remote GPU" in a known project. |
| `rustwgpuengine`, `rustgpuengine`, `rustwgengine` | They spell "Rust" inside a product name (see the last row of the checks). `rust-gpu` is also an existing project, for writing shaders in Rust. |
| `rustengine`, `rustyengine` | `rustengine` is taken on crates.io, and "Rusty Engine" (`rusty_engine`) is an existing Rust game engine for learners. |
| `rwgengine`, `ferroengine`, `oxideengine`, `oxiengine`, `ferrisengine` | The alias alone is taken on crates.io (`rwg`, `ferro`, `oxide`, `oxi`, `ferris`), so the short command and the crate would belong to different projects. |
| `wgpursengine` | `wgpu-rs` was the wgpu library's own repository name. |

## Recommendation

**`rwgpuengine`**, with `rwgpu` as the command. It is the direct answer to "name it after Rust and wgpu":
nothing has to be explained, and both the full name and the alias are free.

Its two costs are real and the owner should weigh them: it is awkward to say, and it names a dependency. If
either matters more than naming wgpu outright, **`oxigpuengine`** is the pick: it still says "Rust, on the
GPU", it can be spoken, and it does not depend on which GPU library is underneath.

## What the rename touches (after the owner picks)

| thing | today | then (with `rwgpuengine` as the example) |
|---|---|---|
| command | `slate` | `rwgpu` |
| crates | `slate_core`, `slate_data`, … | `rwgpuengine` (the command and the entry point), `rwgpuengine_core`, `rwgpuengine_data`, … |
| project file | `slate.toml` | to decide at the rename: `rwgpu.toml`, or the neutral `game.toml` |
| web ABI | `slate_step`, … | `rwgpu_step`, … |
| output envelope | `"slate": "0.1.0"` | `"rwgpu": "…"` (a format version bump) |
| repository | — | on the owner's own GitHub account, where the owner's other open-source projects are |
| authors and copyright | — | set at the open-source readiness step; the owner has said the 16xbrains developers co-author it |

It is one mechanical commit, proven by unchanged state and image hashes, and it is best done before the 3D
work adds new crates.

## For the record: the first shortlist (`<word>16x`)

Checked the same day: 42 names of the form `<word>16x` were all free on crates.io (also with `_core`) and
npm, and the ten strongest were unused on GitHub. `game16x` was recommended, then `forge16x`, `diorama16x`,
`sim16x`, `play16x` and `world16x`. One finding from that round still matters if the `16x` mark is used
elsewhere: another vendor ships "16x Prompt", a tool for AI coding assistants, at `16x.engineer`. The owner
dropped the suffix for this project.
