# Vision

> Sources: *Agent-native Rust game engine: architecture direction* (Nov 24, 2026), the PDF at the repo root,
> and the owner's **direction update of 2026-11-25**, recorded in the next section. Where the two differ, the
> direction update wins. The rest of this page restates the PDF as text so agents and diffs can use it.

## Direction update — 2026-11-25 (owner)

After the first [audit](audits/2026-11-25-architecture-audit.md) the owner corrected the course. The earlier
documents read "keep it simple" as "2D and 2.5D on the CPU". That was not what the owner meant.

| | the direction now |
|---|---|
| **Dimension** | **3D.** The engine is a 3D-optimised engine. 2D and 2.5D games are still made with it, mostly with a 3D look. |
| **Renderer** | **The GPU is a must, and the renderer is written on wgpu.** |
| **What "simple" means** | **Assets only.** A complete 3D title needs many meshes and textures, and engines like Unreal invest in the best renderers and lighting. rwgpuengine does not invest there for now. In every other way it should be a capable engine. |
| **Platforms** | **Native and web from the start.** Some games will be distributed on phones and tablets, so the engine must be able to get there. It gets there by smart choices, not by over-engineering, and it does not bend to support older systems and hardware. |
| **Kinds of games** | Strategy and planning games for teenagers and adults; city builders (the reach is games like Cities: Skylines, not only like Clash of Clans); car-building and simulation games; children's games, both fun and STEM. Any of these can become a learning game, and the very simple mechanics learning games need stay supported. |
| **Templates** | The engine is open source, so it should cover several kinds of game and ship a template for each. |
| **Scale** | The first 3D reference game is a town, on an architecture sized (and measured) for a city. |
| **3D art** (the architect's call, delegated) | Shapes written as text first: buildings, terrain, vehicles and props described in text files, so a coding agent can author them. Importing simple meshes (glTF, flat colors, no textures) comes when a game needs a shape text cannot describe. |

**What does not change:** everything that makes the engine agent-native, the table under "What agent-native
means" below. Text files stay the source of truth, the command line with JSON stays the agent's interface,
the simulation stays deterministic and runs without a screen or a GPU, the world stays queryable, visual
checks stay built in, and limits stay enforced in CI. The GPU is for players. The agent's loop must never
need one.

**What this makes out of date.** A 3D direction study, the next step, rewrites these; until then they
describe the 2D engine that exists:
- "learning games on the web and tablets" in *The bet*, and the *Layers* table below (2D/2.5D render, touch
  input, the four kits);
- ADR 0004's dependency and size budgets as applied to GPU crates, ADR 0006 (entities at city scale), ADR
  0007 (the CPU rasterizer as what players see) and ADR 0008 (the web through a hand-written ABI only);
- [architecture.md](architecture.md), the [roadmap](roadmap.md) from M2 on, and
  [open questions](open-questions.md) Q5–Q8, Q11 and Q12.

## The bet

A game engine whose **first-class users are coding agents**, built in Rust, aimed at **learning games on
the web and tablets**. People still use it; they get familiar words (scenes, entities, prefabs, signals) and
fast feedback. Agents get what they are already good at: reading files and running commands.

## Why build from scratch

Bevy's fast API churn, general-purpose scope and human-first design all work against coding agents.
(Bevy's AI policy is *not* a reason: it governs contributions to Bevy itself, not projects built with it.)

Building from scratch means owning **the core, the data model and the agent interface** — not rewriting
wgpu, windowing, audio or text shaping. Those hold the platform pain and are rented.

## What agent-native means, from the ground up

| principle | what it means in rwgpuengine | where it lives |
|---|---|---|
| Text files are the source of truth | Scenes, prefabs, playtests, project settings are diffable TOML with a published schema. `check` validates without running. | `rwgpuengine_data`, `rwgpu check`, `rwgpu schema` |
| Command line first, JSON everywhere | `new check run test shot inspect explain` all take `--json`. Errors have stable codes, file, line and fix hints, like rustc. | `rwgpuengine_cli`, `rwgpuengine_core::codes` |
| Deterministic core, no screen needed | Fixed timestep, seeded RNG, integer rasterization. Same input → same state hash and same pixels, on every platform, with or without a GPU. | `rwgpuengine_sim`, `rwgpuengine_render`, ADR 0005 |
| A world the agent can query | At any frame: what exists, where, and *why* — an event log that records refusals ("blocked by solid 'gate'") as well as successes. | `rwgpu inspect --events` |
| Built-in visual checks | Offscreen PNG, reference-image comparison, and a text frame report of what is drawn where, hidden or off-screen. | `rwgpu shot --report`, `[[shot]]` |
| Docs tied to the installed version | `rwgpu new` writes an AGENTS.md generated by the running version; `rwgpu docs` prints the reference from the engine's own registries. | `rwgpuengine_cli::docs` |
| The edit-and-verify loop is the speed metric | Data edits need no compile (and hot-reload in the browser). Code edits: few deps, no proc macros, hot-patching later (subsecond). | budgets.toml, ADR 0004 |
| Hard limits enforced in CI | Download size, rebuild time, dependency count etc. fail the build when exceeded. | `cargo xtask budgets` |
| Familiar terms for humans | Scenes, entities, prefabs, signals. | everywhere |

## MCP's place

Files and the command line are the main interface. MCP is a **thin, optional adapter** on top, added later
(ADR 0002). Tool definitions loaded upfront cost context (Anthropic has seen 134K tokens of tool
definitions before optimization); Godot MCP ships 386 tools, while Godot MCP Toolkit starts small and adds
tools on demand. A CLI with `--help --json` is already a progressively disclosed tool surface.

## Three guardrails

1. **Own the architecture, rent the commodity parts.** wgpu, winit, audio, text shaping are rented.
2. **Reference games drive every feature.** Nothing goes into the engine until a game needs it.
   (Comfy was built for its author's games; when they left Rust gamedev in April 2024 it stopped.)
3. **Treat Arcane and nAIVE as prior art.** Borrow headless tests and state-query tools. Stand apart on
   Rust for game code, web and tablets first, and the learning-game kits.

## Layers

```text
Reference games        isometric builder · puzzle · tracing                     (drive features)
Kits (owned)           iso grid · tilemaps · tracing · quiz/lessons
Agent surface (owned)  CLI + JSON · world query · frame report · MCP (optional)  ← agents enter here
Core runtime (owned)   fixed-step sim · text + schema · 2D/2.5D render · touch input
Platform (rented)      wgpu · winit · audio · text shaping
```

## The test of the whole idea

The first milestone is one chain of commands working end to end, natively and in the browser:
`new` → write a small isometric scene as text → `check` → run headless 300 frames with scripted input →
`shot` saves a PNG → `inspect` dumps the world as JSON.

**A coding agent starting cold, with only the AGENTS.md, builds and verifies that scene without help.**
See [mvp-plan.md](mvp-plan.md#the-cold-agent-test) for the protocol.
