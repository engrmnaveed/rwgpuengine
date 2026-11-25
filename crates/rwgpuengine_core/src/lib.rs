//! `rwgpuengine_core` — dependency-free foundations shared by every rwgpuengine crate.
//!
//! Everything here is owned code with zero external dependencies:
//!
//! - [`json`]: a small JSON value with deterministic (insertion-ordered) output
//!   and a strict parser. Used for every `--json` output.
//! - [`diag`]: diagnostics with stable codes, file/line/column and fix hints,
//!   rendered rustc-style for humans or as JSON for agents.
//! - [`codes`]: the registry of every stable diagnostic code (`rwgpu explain`).
//! - [`rng`]: seeded PCG32, identical on every platform.
//! - [`hash`]: FNV-1a 64 for state and image hashes.
//! - [`color`]: RGBA8 colors and hex parsing.
//! - [`math`]: sine and cosine built from exact operations, identical on every platform.
//! - [`text`]: edit distance and "did you mean" suggestions.

pub mod codes;
pub mod color;
pub mod diag;
pub mod hash;
pub mod json;
pub mod math;
pub mod rng;
pub mod text;

/// The engine version, shared by every crate and printed in all JSON output.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Version of the `--json` output envelope. Bump on breaking changes to the
/// shape of command output (see docs/conventions.md#json-output).
///
/// 2: the first key, which names the tool, is `rwgpu` (it was `slate`; ADR 0019).
pub const JSON_FORMAT_VERSION: u32 = 2;
