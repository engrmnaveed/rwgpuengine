//! `rwgpu-player`: the native player. `rwgpu play` runs it with the same
//! arguments; `rwgpu-player --help` lists them and the key bindings.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    std::process::exit(rwgpuengine_player::main_with(std::env::args().skip(1).collect()));
}

/// In a browser the page is the program: the library's `web` module.
#[cfg(target_arch = "wasm32")]
fn main() {}
