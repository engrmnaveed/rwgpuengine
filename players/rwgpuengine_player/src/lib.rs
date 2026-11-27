//! `rwgpuengine_player` — the players: a project in a native window or on a
//! browser canvas, drawn with the GPU. `rwgpu play` runs the native program,
//! `rwgpu-player`; `rwgpu serve` serves the web player.
//!
//! It is a thin shell. The simulation is the engine's `Session`, stepped at
//! the project's fixed rate with the same input the CPU web player and
//! playtests give; the picture is the session's render list drawn by
//! `rwgpuengine_gpu`, which `cargo xtask gpu` holds to the CPU reference. The
//! player adds the clock, the window or canvas, and the mapping from keys,
//! clicks and touches to that input (ADR 0022, ADR 0024).
//!
//! - [`play::Player`]: step, build the list, draw it only when it changed.
//! - [`fit`]: the frame in a window or canvas, with bars; [`input`]: keys,
//!   taps, swipes and camera gestures; [`clock`]: fixed steps and the wall
//!   clock. These are the same in both players.
//! - Native: [`window`], winit's event loop around a player, and [`cli`],
//!   `rwgpu play`'s arguments in and `rwgpu`'s output out.
//! - Browser (wasm32): [`web`], the functions the page calls.

pub mod clock;
pub mod fit;
pub mod gpu;
pub mod input;
pub mod play;

#[cfg(test)]
mod gestures;

#[cfg(not(target_arch = "wasm32"))]
pub mod cli;
#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(not(target_arch = "wasm32"))]
pub mod window;

#[cfg(not(target_arch = "wasm32"))]
pub use cli::main_with;
pub use gpu::Gpu;
pub use play::Player;
