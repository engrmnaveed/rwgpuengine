//! `slate_runtime` — the composition root.
//!
//! Ties data, simulation and rendering together for both front ends (the
//! CLI and the web player): load a [`Session`], step it with scripted and
//! live input, render, inspect, and run playtests. Rust game code plugs in
//! through the [`Game`] trait. A player shows a session through a
//! [`Presenter`], which repaints only when the picture changes.

pub mod playtest;
pub mod present;
pub mod session;

pub use playtest::{PlaytestRun, ShotCapture, run_playtest};
pub use present::Presenter;
pub use session::{DataOnly, Game, InspectOptions, LiveInput, PlaceQuery, Script, Session, SessionConfig};

#[cfg(test)]
mod tests;
