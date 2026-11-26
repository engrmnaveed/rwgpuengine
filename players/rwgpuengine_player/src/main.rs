//! `rwgpu-player`: the native player. `rwgpu play` runs it with the same
//! arguments; `rwgpu-player --help` lists them and the key bindings.

fn main() {
    std::process::exit(rwgpuengine_player::main_with(std::env::args().skip(1).collect()));
}
