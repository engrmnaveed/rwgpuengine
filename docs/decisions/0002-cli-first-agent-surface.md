# ADR 0002: Files and the CLI are the agent interface; MCP is a thin adapter later

- Status: accepted, 2026-11-24
- Source: direction doc, "Agents' main interface"

## Context
Agents are already best at reading files and running commands. MCP clients typically load every tool
definition into context upfront; tool definitions alone have reached 134K tokens before optimization.
Godot MCP ships 386 tools; Godot MCP Toolkit starts small and adds tools on demand.

## Decision
- Every capability is a `slate` command over text files. All commands accept `--json` and print exactly
  one JSON object with a fixed envelope (`slate`, `format`, `command`, `ok`, `errors`, `warnings`,
  `diagnostics` + command fields). Exit codes: 0 ok, 1 failures, 2 usage.
- The command table is data (`slate_cli::args::COMMANDS`): `help`, `help --json` and docs come from it, so
  the CLI is self-describing and progressively disclosed (`slate help` → `slate help shot`).
- MCP, if added (M6), is a thin adapter mapping a handful of tools 1:1 onto commands.

## Consequences
- Any agent that can run a shell can use slate; no client integration needed.
- The JSON envelope is a public contract, versioned by `format` (`JSON_FORMAT_VERSION`).

## Revisit when
A target agent environment cannot run commands, or MCP clients reliably load tools lazily.
