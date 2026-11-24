# ADR 0005: The determinism contract

- Status: accepted, 2026-11-24
- Source: direction doc, "A deterministic core that runs without a screen"

## Context
Scripted playtests and record/replay must give identical results every run, on every platform, with or
without a GPU. Reference-image tests need bit-identical pixels.

## Decision
Same project files + input script + seed ⇒ same state hash and same pixels on every platform. Achieved by:
- **Fixed timestep.** Frame N = state after N steps; input at frame N is applied by step N. Wall-clock
  time never enters the simulation (clippy bans `Instant::now`/`SystemTime::now`).
- **One seeded RNG** (PCG32) per world, drawn in a fixed order (entity id order). Known-value test pins it.
- **Exact float operations only**: IEEE `+ − × ÷` and `sqrt` are exact everywhere; libm `sin/cos/pow/exp/
  ln/atan2` are banned by clippy. Rounding is `floor(x + 0.5)`.
- **Ordered collections**: `HashMap`/`HashSet` are banned (random iteration order); use `BTreeMap`.
- **Fixed step order** (input, movers, patrols, wanderers, custom systems, triggers, signals).
- **Integer rasterization**: coverage, shading and blending are integer math (ADR 0007).
- `World::state_hash()` and `Frame::image_hash()` (FNV-1a 64) to compare runs in one line;
  `slate run --hash-every N` finds the first frame where two runs diverge.

Verified on v0.1: native (aarch64) and browser (wasm32) agree on state and image hashes.

## Consequences
- Game code must follow the same rules (documented; enforced by clippy for engine crates).
- Changing the RNG, step order or rasterizer changes recorded hashes and reference images: such changes
  must be deliberate and called out in the commit.

## Revisit when
A feature needs trigonometry (rotation, curves): add owned, exact-op implementations (polynomial/CORDIC) or
fixed-point helpers rather than calling libm.
