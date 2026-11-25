# Tools behind the 2026-11-25 audit

Scratch scripts, kept so every number in the [audit](../../2026-11-25-architecture-audit.md) can be measured
again. They are not part of the engine or the gate. Run them from the repository root after
`cargo xtask web` and `cargo build --release`.

| file | what it measures | how to run |
|---|---|---|
| `wasm_check.mjs` | State and image hashes of every playtest of a project in `rwgpuengine_web.wasm` under Node, with no browser. `--native <rwgpu>` compares them with `rwgpu shot` and exits 1 on a difference. `--bench` adds render time. | `node docs/audits/2026-11-25/tools/wasm_check.mjs target/wasm32-unknown-unknown/web/rwgpuengine_web.wasm examples/tiny_town --native target/release/rwgpu` |
| `scale_town.py` | Copies a project with every pixel value multiplied by `k` (display, iso tiles, HUD, sprites, labels, taps). | `python3 docs/audits/2026-11-25/tools/scale_town.py examples/tiny_town /tmp/town_x2 2` |
| `fuzz.py` | Breaks the example projects at random (seeded) and runs five commands on each; reports panics, hangs and output that is not JSON. | `python3 docs/audits/2026-11-25/tools/fuzz.py target/release/rwgpu . /tmp/fuzzwork 400` |
| `raster_spans_bench.rs` | Splits a frame into tick, draw-list build, rasterize and report; compares a span rasterizer prototype with the reference rasterizer, frame by frame. It is a record of the prototype and still uses the crates' old names (`slate_*`). | Put it in a scratch crate as `src/main.rs` with path dependencies on `rwgpuengine_core`, `rwgpuengine_data`, `rwgpuengine_sim`, `rwgpuengine_render` and `rwgpuengine_runtime`; then `cargo run --release -- examples/tiny_town`. |

The other targets in the audit's determinism table were run like this:

```bash
# x86-64 macOS, under Rosetta 2
cargo build --release --bin rwgpu --target x86_64-apple-darwin
arch -x86_64 target/x86_64-apple-darwin/release/rwgpu test examples/tiny_town

# x86-64 Linux, static, run in any Linux container
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
  cargo build --release --bin rwgpu --target x86_64-unknown-linux-musl
```

Linux arm64 was built inside a `rust:1.100.0-alpine` container with `--network none`, from a copy of the
repository and `cargo vendor`ed dependencies.
