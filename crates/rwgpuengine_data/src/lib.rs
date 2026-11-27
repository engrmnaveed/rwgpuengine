//! `rwgpuengine_data` — text files are the source of truth.
//!
//! Scenes, prefabs, playtests and the project file are TOML with a published
//! schema. This crate turns text into typed definitions, or into diagnostics
//! with stable codes, file, line and fix hints, without running anything.
//!
//! - [`node`]: span-preserving document tree (TOML parsed by `toml_edit`).
//! - [`schema`]: the format registry: validation, defaults, JSON Schema, docs.
//! - [`defs`]: typed definitions the simulation is built from.
//! - [`load`]: the virtual file set, loaders and `check_project`.

pub mod defs;
pub mod load;
pub mod node;
pub mod schema;

pub use defs::*;
pub use load::{CheckReport, PROJECT_FILE, SceneLoader, Vfs, check_project, load_playtest, load_project};

#[cfg(test)]
mod corpus;
#[cfg(test)]
mod tests;
