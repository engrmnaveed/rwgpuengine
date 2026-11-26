//! `rwgpuengine_player` — the native player: a project in a window, drawn
//! with the GPU. `rwgpu play` runs its program, `rwgpu-player`.
//!
//! It is a thin shell. The simulation is the engine's `Session`, stepped at
//! the project's fixed rate with the same input the web player and playtests
//! give; the picture is the session's render list drawn by `rwgpuengine_gpu`,
//! which `cargo xtask gpu` holds to the CPU reference. The player adds the
//! clock, the window, and the mapping from keys and clicks to that input
//! (ADR 0022).
//!
//! - [`play::Player`]: step, build the list, draw it only when it changed.
//! - [`window`]: winit's event loop around a player; [`fit`]: the frame in a
//!   window, with bars; [`input`]: keys, taps, swipes and camera gestures;
//!   [`clock`]: fixed steps and the wall clock.
//! - [`cli`]: `rwgpu play`'s arguments in, `rwgpu`'s output out.

pub mod cli;
pub mod clock;
pub mod fit;
pub mod gpu;
pub mod input;
pub mod play;
pub mod window;

pub use cli::main_with;
pub use gpu::Gpu;
pub use play::Player;
