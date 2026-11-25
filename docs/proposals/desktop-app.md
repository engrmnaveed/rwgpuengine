# Proposal: the rwgpuengine desktop app (owner's idea, 2026-11-25) — not decided

> Status: **idea recorded, sequenced after the platform foundation** (see the roadmap). Nothing here is
> decided; the owner and the senior architect turn it into ADRs when its turn comes.

## The idea, in the owner's words (summarised)

A desktop app that makes the **game producer's** experience better and more human: see the game, steer it,
review it. **Under the hood it keeps doing things the rwgpuengine way**: files are the truth, and it uses the same
commands and the same deterministic checks.

It is **agentic-native through Claude Desktop**. The app builds a bridge to the main Claude app, so the
producer works with Claude there, and Claude drives the game through the bridge. Other agentic coding
platforms (Codex and others) come later.

## What this means against the vision

- **It fits** "familiar terms and fast feedback for people". The vision serves humans too; today they only
  get a CLI and a browser player.
- **It must not fork the truth.** The app has no hidden state. Everything it shows comes from the files and
  from `rwgpu … --json`. Everything it changes goes through the same paths an agent uses, so humans and
  agents always see the same project.
- **The bridge is the vision's "thin, optional MCP adapter"** (ADR 0002, Q9), which becomes concrete here:
  - **One protocol, many agents.** Claude Desktop, Claude Code, Codex and other coding agents can all
    connect to an MCP server. If the bridge is an MCP server the app hosts (or a `rwgpu mcp` command the app
    starts), "other platforms later" costs almost nothing.
  - **A small tool surface.** The vision's context-cost argument still holds: the few tools Q9 suggests
    (`check`, `run`, `shot`, `inspect`, `explain`, perhaps `test`), with the CLI's `--help --json` for the
    rest.

## Likely shape (options to measure, not choices)

| part | options | what decides it (KPIs) |
|---|---|---|
| App shell | Tauri (Rust + system webview), a native Rust GUI (egui/iced), Electron | bundle size, start time, reuse of the existing web player (wasm + canvas), Rust in-process, platforms |
| Live view | embed the existing web player; or a native window with the CPU frame | one player for web and app (determinism, maintenance) |
| Engine link | in-process crates; or a long-running `rwgpu` session server (JSON over a socket) | latency of scrub/step/inspect, and one protocol shared with MCP |
| Agent bridge | MCP server (stdio or local HTTP) that Claude Desktop connects to; later Codex etc. | setup steps for the producer, tool count (context), security (local only) |
| Panels | project and files, live player with frame scrubbing, entity inspector, event-log timeline, playtests with image diffs, diagnostics, an "agent activity" feed, approvals | the producer's time to understand and approve a change |

## Foundation it needs first (why it waits)

1. A **stable machine interface** for a long-running tool: the session protocol that the app and the MCP
   adapter would share. The CLI is per-invocation today.
2. The **MCP adapter** (Q9) and its security model (local only, no network, which files it may touch).
3. A **native or webview player** path, and packaging and signing for the target OSes.
4. **Consolidated formats and interfaces**, so the app is not rebuilt each time the core changes.

## Questions for the owner (when its turn comes)

1. **Who is the producer?** A teacher, a game designer, a parent? That sets the vocabulary and the panels.
2. **Target OSes** and distribution (macOS first? Windows?).
3. **The bridge:** MCP only, so the producer talks to Claude in Claude Desktop? Or should the app also run an
   agent itself (Claude Agent SDK) inside its own window?
4. **What a human may edit directly** in the app, and what is always done by asking the agent.
5. **Name and branding** (ties to Q1).
