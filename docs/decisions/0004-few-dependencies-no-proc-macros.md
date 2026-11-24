# ADR 0004: Few dependencies, zero proc macros in the runtime tree

- Status: accepted, 2026-11-24
- Source: direction doc, "The edit-and-verify loop is the key speed metric"

## Context
Code-edit latency is the agent's speed limit. LogLog Games found Rust compile times had improved, but not
with proc macros. Every dependency is also compile time, supply-chain risk and API churn.

## Decision
- The runtime tree (`slate_runtime` and below) has **zero proc-macro crates**; `cargo xtask budgets`
  enforces it (`deps.proc_macro_crates = 0`) and caps external crates (`deps.runtime_crates`).
- Owned replacements for common crates where they are small: JSON (`slate_core::json`, no serde), argument
  parsing (`slate_cli::args`, no clap), RNG (PCG32, no rand), hashing (FNV-1a), the dev HTTP server.
- New rented crates need a line in this ADR (below) saying why.

Rented today: `toml_edit` (TOML parsing with spans), `png` (PNG encode/decode). v0.1 tree: 16 external
crates, 0 proc macros.

## Consequences
- Measured: recompiling `slate_sim` and everything above it takes ~0.4 s (dev, M5 Pro); the release CLI
  is 1.5 MB; the wasm module is 467 KB.
- We maintain a little more code (≈600 lines for JSON + args + server).

## Revisit when
A rented crate would replace a large owned subsystem, or a proc macro would remove a whole class of bugs.
