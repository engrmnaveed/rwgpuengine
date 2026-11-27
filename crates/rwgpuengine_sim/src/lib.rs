//! `rwgpuengine_sim` — the deterministic core. Runs without a screen.
//!
//! The world advances in fixed steps ([`World::tick`]). Given the same scene,
//! seed and input, every step produces bit-identical state on every platform,
//! native or wasm, with or without a GPU (ADR 0005). [`World::state_hash`]
//! makes that checkable in one comparison.
//!
//! Built-in behaviors (components): `mover`, `patrol`, `wander`, `trigger`.
//! Signals flow through `[[connect]]` wiring; every step of that, including
//! refusals, is written to the [`events::EventLog`].

pub mod dump;
pub mod events;
pub mod input;
pub mod world;

pub use events::{EVENT_DOCS, Event, EventDoc, EventKind, EventLog, events_markdown};
pub use input::{ActionSet, InputFrame, Tap};
pub use world::{Condition, Connection, Entity, EntityId, NoSystems, Systems, World};

/// Rebuild probe for `cargo xtask budgets`: changing this env var makes
/// cargo recompile this crate (and its dependents) exactly as an edit would,
/// without touching any source file.
#[doc(hidden)]
pub const REBUILD_PROBE: Option<&str> = option_env!("RWGPU_REBUILD_PROBE");

#[cfg(test)]
mod equivalence;
#[cfg(test)]
mod oracle;
#[cfg(test)]
mod tests;
