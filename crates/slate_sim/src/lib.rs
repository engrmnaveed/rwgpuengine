//! `slate_sim` — the deterministic core. Runs without a screen.
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

pub use events::{Event, EventKind, EventLog};
pub use input::{ActionSet, InputFrame, Tap};
pub use world::{Connection, Entity, EntityId, NoSystems, Systems, World};

#[cfg(test)]
mod tests;
