# Spikes behind the 3D architecture proposal

Scratch programs that produced the numbers in [the proposal](../../3d-architecture.md). They are not engine
code and not part of the gate. Each is its own small workspace; build them one at a time. Three of them
download wgpu and its dependencies from crates.io on the first build.

| folder | what it measures | how to run |
|---|---|---|
| `shared/scene.rs` | The town, the camera and the light that both renderers use. Exact float operations only. | included by the others |
| `gpu_native` | N instanced boxes through wgpu 30 with no window: time until the GPU finished, and one frame written as raw RGBA. | `cargo run --release -- <output folder>` |
| `gpu_web` | The same through wgpu's WebGPU backend in a browser, with a hash of the pixels. | `cargo build --release --target wasm32-unknown-unknown`, then `wasm-bindgen --target web --out-dir web/pkg target/wasm32-unknown-unknown/release/gpu_web.wasm`, then serve `web/` and open it. Add `--features webgl` to the build to measure the size of the WebGL2 fallback. |
| `cpu_ref` | A deterministic CPU renderer for the same town, with depth and an owner per pixel; compares itself with `gpu_native`'s frames if they are in the output folder. | `cargo run --release -- <output folder>` |
| `scale_bench` | Moving N actors on a grid with today's storage, with a cell index, and with positions in a column. | `cargo run --release` |

The engine's own scale figures in the proposal came from `rwgpu run` on generated scenes of N wandering
actors (`wander = { step_frames = 1 }` on every other cell).

Results on an Apple M5 Pro, 2026-11-25, are in the proposal's section 3.
