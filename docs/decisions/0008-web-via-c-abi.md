# ADR 0008: Web via a wasm32 cdylib with a hand-written C ABI

- Status: accepted, 2026-11-24

## Context
The first milestone must work in the browser; web and tablets come first. wasm-bindgen/wasm-pack add a
proc-macro dependency, an extra tool, generated JS and version-matching between the CLI tool and crate.

## Decision
- `slate_web` is a `cdylib` for `wasm32-unknown-unknown` exporting ~20 `extern "C"` functions (alloc/free,
  set_bundle, start, step, run_to, render, hash, inspect, events, ...). Strings cross as (ptr, len); text
  results come back through an output buffer.
- `web/player.js` is plain JavaScript (no framework, no bundler).
- The project travels as a text bundle (`slate-bundle 1` header, then `path\0text\0` pairs), built by
  `Vfs::to_bundle` — the same in-memory file set the native loader uses.
- `slate serve` (std-only HTTP) serves page, module and a fresh bundle per request, plus a content hash the
  page polls for instant data reload.

## Consequences
- Built with `cargo xtask web`: 467 KB, zero imports. No JS toolchain needed.
- The ABI is small and hand-maintained; a native test drives it and checks it matches the native runtime.

## Revisit when
The ABI grows past ~40 functions, or web APIs (audio, WebGPU) need rich bindings — then consider
wasm-bindgen for the platform layer only.
