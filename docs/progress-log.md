# Progress log

Newest first. Commit messages carry the detail; this page is the narrative and the place to record
findings that are not code (experiment results, surprises, things to remember).

> Entries before "The engine is named rwgpuengine" were written when the working codename was "slate" and
> keep it: read `slate` as `rwgpu` and `slate_*` crates as `rwgpuengine_*`
> ([ADR 0019](decisions/0019-name-rwgpuengine.md)).

## 2026-11-27 — `rwgpu export web`: a game as a folder for any static web host, with the WebGPU player

- Step 4, piece 3 ([ADR 0025](decisions/0025-rwgpu-export-web.md)). `rwgpu export web [PATH] --out DIR` writes
  the files `rwgpu serve` serves, under the same names, once: the player's, then `project.json` and
  `bundle.txt`. Every address the pages, their scripts and wasm-bindgen's glue use is relative, so the folder
  works at a site's root and under any path. Built first with the CPU player on 2026-11-26; the WebGPU player
  of piece 2 went in when it landed.
- **One lookup for both commands.** `rwgpuengine_cli/src/web.rs` finds the player for `serve` and `export`:
  a flag or a variable names the one place (`--web-player`, `--wasm`, `RWGPU_WEB_PLAYER`, `RWGPU_WEB_WASM`);
  otherwise a WebGPU player next to `rwgpu` or in the workspace beats a CPU module in the same places. E0008
  when there is none (E0007 went to piece 2's `web-tool-missing`). `cargo xtask dist` puts both players next
  to `rwgpu`. `--json` says which player.
- **Only what the engine reads** goes in the bundle: `rwgpu.toml`, `scenes/`, `prefabs/`, `tests/`. Validated
  first; refuses a folder that is not empty without `--force`; never deletes; never writes through a link;
  the same bytes every time.
- **The proof, in both gates.** `cargo xtask ci` (no GPU, no browser) exports every example with the CPU
  player and plays the folder's own page under Node from a plain static host, at `/` and under a path: all ten
  playtests at the state and image hashes of `rwgpu test` and `rwgpu shot`. `cargo xtask gpu` exports every
  example with the WebGPU player and runs piece 2's browser check on the folders, from the same static host at
  `/` and under `/rwgpuengine/`: every playtest at the engine's state on WebGPU (0 pixels apart from the
  reference, on Metal) and on the CPU path (the reference's exact pixels); every request answered from the
  folder, every file used.
- **The link: GitHub Pages, decided by the owner** on 2026-11-26 ("Yes, GitHub Pages"). `cargo xtask pages`
  builds through `cargo xtask dist`, exports Tiny Town with the WebGPU player and proves that very folder in a
  headless Chrome under `/rwgpuengine/` and at `/`. `.github/workflows/pages.yml` runs it on `macos-latest`,
  where the runner's Chrome draws WebGPU, and publishes only that folder; manual trigger only, deploy only
  from `main`, least permissions. The supervisor switches Pages on and runs it after step 4 is in `main`.
- **Sizes.** Tiny Town's folder: 788,625 bytes (303,479 gzipped), of which the WebGPU player is 771,924
  (299,098) against its budget of 1,500,000, and the game 16,701 (4,381). With the CPU player: 662,530.
- **Found on the way.** A browser asks the host for `/favicon.ico`, outside a folder served under a path;
  both pages now name an empty icon, so nothing outside the folder is asked for.
- Not done: offline or installable pages, hashed file names against host caching, compression.

## 2026-11-26 — The web player on WebGPU, with the CPU reference renderer as its fallback

- Step 4, piece 2 ([ADR 0024](decisions/0024-web-player-on-webgpu.md)). `cargo xtask web-player` builds
  `rwgpuengine_player`'s library for wasm32 (its `web` module) with wasm-bindgen 0.2.130 and copies the page;
  `cargo rwgpu serve examples/tiny_town` shows the town on WebGPU, or with the CPU reference renderer where the
  browser has no WebGPU, no adapter, or loses the device. The page says which in one line.
- **Headless WebGPU, measured** (`web_probe`, Node only): this Mac's Chrome 158 gives the Metal adapter with no
  flags; GitHub's macOS runner its Paravirtual device; the Windows runner WARP only with
  `--enable-unsafe-webgpu --ignore-gpu-blocklist`; the Linux runner SwiftShader with `--enable-unsafe-webgpu`
  (Mesa's lavapipe never shows). So every runner can play the web player on WebGPU, with no new software.
- **The shape, measured.** One module holding the engine, the GPU renderer and the CPU painter: 767,824 bytes
  to download (297,481 gzipped) against the budget of 1,500,000. Two modules would save 12 KB on the WebGPU
  path and 125 KB on the fallback, but a lost device would end the game and the bindings would be written
  twice. winit's web backend would add 146 KB of wasm, 28 KB of glue, 8 crates and a proc macro, against a
  page script of 14 KB.
- **The glue tool** is installed by xtask into `target/tools/` with `cargo install --locked`, the crate's exact
  version: 24 to 25 s once; CI caches it.
- **Shared with the native player:** `Player`, `fit`, `clock::Steps`, the bindings (now keyed by W3C key codes,
  which are winit's names too) and the camera moves. Added: a two-finger pinch for touch screens.
- **The proof, in `cargo xtask gpu`:** a headless Chrome plays all ten example playtests on WebGPU and on the
  CPU path; every state hash is `rwgpu test`'s; the WebGPU frames are 0 pixels apart from the reference (Metal,
  and SwiftShader by hand); the CPU canvas holds the reference's pixels; a device lost halfway through a
  playtest hands the game to the CPU path at the same state; a still scene draws one frame in 3 s. CI runs
  it three times per job: green 3 of 3 on each runner of run 45884520090.
- **The Linux runner cannot show WebGPU on a canvas.** Chrome's only adapter there is SwiftShader, which loses
  the device a few frames after a WebGPU canvas first shows a picture ("A valid external Instance reference
  no longer exists"); a plain-WebGPU page in web_probe fails the same way under ten sets of flags, and it
  reproduces on a Mac with SwiftShader. There the gate proves the build, the CPU path in the browser and
  the handover after the browser's loss; the WebGPU picture is carried by the macOS and Windows runners and
  this Mac. The search found three real faults: the player ignored a loss Chrome reports as `destroyed`;
  the check let a WebGPU path that fell back pass; and `rwgpu serve` answered one connection at a time, so
  an idle one stalled a page on the Windows runner. All three are fixed.
- **Timings, this Mac:** navigation to the first frame 39 to 95 ms; with the camera turning, 0.25 to 0.4 ms of
  the page's time per drawn frame.
- Not done: `rwgpu export web` (piece 3), WebGL2, offline pages, sound, gamepads, phone shells; the CPU player
  of ADR 0008 has no camera bindings.

## 2026-11-26 — Release builds without local paths; the gate keeps paths and build output out of git

- Step 4, piece 1 ([ADR 0023](decisions/0023-release-builds-without-local-paths.md)). It pays the debt the
  entries of 2026-11-25 recorded ("a `dist` build that remaps paths is needed before any binary is
  published"), for the command, the web module and the players' programs.
- **What was in the artifacts**, on the reference machine: 45 paths in `rwgpu`, 21 in the web module, 289 in
  `gpu_check` and 390 in `rwgpu-player`. Panic locations of rented crates name Cargo's registry under the home folder; in the players'
  workspace the agent's crates are path dependencies and were named by their absolute path; and `gpu_check`
  found the repository through `env!("CARGO_MANIFEST_DIR")`. Now: 0 in each.
- **How.** xtask passes rustc `--remap-path-prefix` for the repository (→ `crates/…`), each registry folder
  (→ `png-0.18.1/…`), Cargo's git checkouts, and the rest of the home folder (→ `~`). Every release build goes
  through that one function, and `cargo xtask dist` builds all release artifacts with it. Cargo's
  `trim-paths` was measured too: same paths removed and the same build time, but it is not stable on 1.100,
  writes longer names and names the agent's crates `/cargo/deps/rwgpuengine_core-0.1.0/…`.
- **Measured.** Sizes, plain → `cargo xtask dist`: the command 2,031,296 → 2,014,784 bytes, the web module
  628,075 → 626,627, `gpu_check` 5,571,616 → 5,555,104, `rwgpu-player` 5,988,512 → 5,955,488. A clean build
  takes the same time (3.5 s, 3.5 s and 14.4 s, medians of three, before the player landed). The dev rebuild after an edit is untouched (0.44–0.52 s); a release one is 1.72–1.76 s either way.
- **The gates prove it.** After each release build, xtask reads the artifact's bytes for this machine's home
  folder, the repository and Cargo's home (with `\`, `/` and `\\`, any case, UTF-8 and UTF-16). `ci` covers
  the command and the web module, `gpu` the players' programs.
- **`cargo xtask repo-check`**, the first step of `ci`: no home path of any user in a tracked file, no build
  output (by folder, name or first bytes), no file over 150,000 bytes (the largest today, `load.rs`, has
  75,685). Untracked build output that `.gitignore` misses fails as well, so the `players/target-musl`
  folder pushed earlier today would have stopped the gate before it was staged. The tree passed as it was;
  the only exception is the placeholder name `someone`.
- `cargo install --locked --path players/rwgpuengine_player` stays the way to install the player on one's
  own machine; the README now says that a program for someone else comes from `cargo xtask dist`.
- Linux was checked in a container (home `/home/someone`, Cargo's home `/usr/local/cargo`): a plain build has
  44 paths, the remapped one 0. Windows only by unit tests until its CI runner runs it.
- Not done: reproducible builds byte for byte, installers, signing, git hooks. The outgoing-commit check
  before a push is still a step people and agents take by hand ([conventions](conventions.md#privacy)).

## 2026-11-26 — The native player: Tiny Town in a window, drawn with the GPU, replaying every playtest to its state hash

- Step 3, piece 3 ([ADR 0022](decisions/0022-native-player-winit-and-rwgpu-play.md)). `players/rwgpuengine_player`
  is the program `rwgpu-player`; `rwgpu play` finds it next to itself or on `PATH` and runs it (E0006 when it
  is not installed). The simulation is the engine's `Session`, stepped at the project's fixed rate with the
  web player's input; the picture is `rwgpuengine_gpu`'s, drawn only when the list changed. With this, step 3
  ("wgpu players") is done when it lands on `gpu-players`.
- **The window library, counted before building.** winit 0.30.13 with X11 and Wayland: Linux 105 rented
  crates (limit 130), Windows 87, macOS 84; 6, 6 and 5 proc macros (limit 6). The drawn Wayland title bar
  would cost 9 crates more; X11 only would save 19 and a proc macro on Linux. SDL3 and GLFW count few crates
  because their window system is a C library the count does not see.
- **A counting bug found on the way.** The gate matched proc macros by name across the lockfile. winit
  brings `thiserror` 1 on Linux only, and the gate then counted its `thiserror-impl` on Windows and macOS too
  (7 on Windows, over the limit). It now matches name and version.
- **Proof without a person.** `cargo xtask gpu` replays the ten example playtests three ways: the player's
  loop inside the check (same state hashes as the engine; the last frame 0 pixels apart from the reference
  on Metal), the `rwgpu-player` program with `--headless` (the hashes `rwgpu test` prints), and `rwgpu play`
  through `PATH`. With `--window` it replays `turn_and_build` in a real window and compares the picture read
  back from the window with the reference: on this Mac 0 of 307,200 frame pixels apart, each a 2 × 2 block.
- **Measured on an Apple M5 Pro**, Tiny Town in the window: per drawn frame on the CPU the list 38 µs,
  `Renderer::draw` 589 µs, showing it 214 µs; a still scene costs 0.8 % of one core. Clean release build of
  the players' workspace 13.6 to 17.8 s (limit 60).
- **The window on GitHub's runners.** Each runner opened a real window and replayed `turn_and_build` in it
  16 times out of 16 over two CI runs: lavapipe under `xvfb-run` on Linux, WARP on Windows, the virtual Metal
  device on macOS. Every replay reached the hash, and the picture read back from the window was 1 pixel apart
  from the reference on Linux and 0 on the others. So the `gpu` job now runs `cargo xtask gpu --window`
  everywhere. It takes 3 min 24 s, 4 min 59 s and 3 min 46 s warm on Linux, Windows and macOS (before:
  about 2, 3¾ and 2½).
- **A slip, and what it taught.** A commit staged with `git add -A players` carried a local cross-build folder,
  whose files hold the build machine's home path, to the public branch. It was taken out of the history with
  the owner's approval. Stage files by name, and check every outgoing commit, not only the tip.

## 2026-11-26 — The reference keeps its own rules: level faces by their plane, cards on their floor

- The defect the GPU check found in the reference renderer (Q17) is fixed, on the step branch
  ([note in ADR 0020](decisions/0020-render-list-camera-and-cpu-reference-renderer.md#note-2026-11-26-faces-in-one-plane-and-cards-on-a-floor),
  [ADR 0021](decisions/0021-wgpu-renderer-held-to-the-reference.md#what-the-check-found-in-the-reference-and-what-stays-undefined)).
  Decided by the supervisor session, as a fix that makes the reference keep rules the owner approved in
  ADR 0020; reported to the owner on 2026-11-26.
- **The fix.** A level face takes its depth from its plane's equation, not from its snapped corners, so two
  faces at one height have the same depth in every pixel and the rank decides. The part of a card at and
  below its anchor has that plane's depth and the highest rank. Five render tests fail on the old code.
  All ten playtests keep their state and image hashes, and painting is no slower.
- **Both cases are in the GPU gate now**, in hand-made scenes and in the random ones: 271 scenes.

  | adapter | pixels apart, the same 256 scenes: before → after the fix | all 271 scenes / worst scene | inside a face |
  |---|---|---|---|
  | Apple M5 Pro, Metal | 33 → 18 | 34 / 7 | 0 |
  | macOS runner, Metal | 33 → 18 | 34 / 7 | 0 |
  | Linux runner, Mesa lavapipe | 79 → 68 | 91 / 12 | 0 |
  | Windows runner, WARP | 63 → 52 | 61 / 5 | 0 |

  The worst scene is new: the town of 5,000 boxes in perspective, turned and zoomed in (12 pixels on
  lavapipe). `pixels_apart` stays at 24.

- **Longer random runs found five more things**, and each was traced to the rule that decides the pixels.
  - *The reference fix had a hole.* An anchor a hair above a pixel center left a gap of one row across the
    card. Fixed: the row just above the anchor lies on the floor too where it is not nearer than the floor.
  - *An upright edge one step off* (GPU). In the iso view a box's upright edge is one pixel column, and the
    GPU's single-precision corner could land one sub-pixel step beside the reference's: a whole edge one
    pixel off. Fixed: the CPU gives those columns, as the reference computes them.
  - *A card through the top of a box* (GPU). A card had a 16-step lead in depth over everything, to beat its
    own floor. Where it passes through the top of a box it stands in, the lead decided a row wrongly. Fixed:
    level faces keep their exact depth, and the card is held to its floor only in the row above its anchor.
  - *Two boxes whose sides have the same depth*, and *two labels on one upright line at different heights*:
    no rule decides these, and each renderer's rounding does. They stay undefined, are named at the top of
    `scenes.rs`, and random scenes keep them out by construction.
- **On the M5 Pro 20,000 random scenes pass**: 18,525 are identical to the reference, 1,280 differ in one
  pixel, 167 in two, 27 in three to five, one in 13.
- **Left for the owner.** Taking the depth of a box's sides from their plane as well settles the first
  undefined case in the reference. It changes one pixel of `tiny_town`'s `perspective.png`, so it is not
  done.
- A correction to this log's method: a watch on a CI run reported success because its exit status was a
  pipe's, not the run's. The run had been cancelled by the next push. Checks are now read from the pull
  request itself.
- `players/rwgpuengine_gpu/README.md` says how a player calls the renderer.

## 2026-11-26 — The players' renderer: wgpu draws every reference scene to within 6 edge pixels of the reference, on four adapters

- Step 3, piece 2 ([ADR 0021](decisions/0021-wgpu-renderer-held-to-the-reference.md)). `players/` is a second
  workspace with the crate `rwgpuengine_gpu` (wgpu 30.0.2, no window, compiles for wasm32) and the tool
  `gpu_check`. `cargo xtask gpu` is the players' gate; `cargo xtask ci` is unchanged, and the root
  `Cargo.lock` still has 33 packages and no wgpu.
- **The result.** 256 scenes are drawn by both renderers: the six frames the example playtests shoot, a
  town of 5,000 boxes, a scene per rule under nine cameras, and 200 random scenes. No per-channel
  tolerance; a differing pixel must be on an edge of the reference picture. All three CI runs of the pull
  request were green on all three systems (45766846084, 45768134899, 45769343354).

  | adapter | pixels apart, all 256 scenes / worst scene | inside a face | frame of 50,000 boxes | `gpu` job, warm / cold |
  |---|---|---|---|---|
  | Apple M5 Pro, Metal | 33 / 3 | 0 | 1.0–1.75 ms | — |
  | macOS runner, Metal | 33 / 3 | 0 | 6.3–7.0 ms | 2 min 30 s / 6 min 35 s |
  | Linux runner, Mesa lavapipe | 79 / 6 | 0 | 67–110 ms (software) | 2 min 8 s / 3 min 58 s |
  | Windows runner, WARP | 63 / 3 | 0 | 103–126 ms (software) | 3 min 59 s / 7 min 32 s |

  On Metal and WARP all six example frames are the reference's bytes. The limit is `pixels_apart = 24`.
- **What made it this close.** Whatever decides a pixel is computed by the reference's own code or in whole
  numbers. A face's color is selected from a table the reference's `shade` fills. Cards are placed on the CPU
  by `Projector::card`, and their shapes are tested per pixel with the reference's integer rule. See-through
  colors are mixed in the shader with the reference's integer formula. Level faces compute their depth from
  the plane's equation and carry their rank in its three lowest bits, so "two faces in one plane" is an
  exact comparison of bits on every adapter. What is left is the rasterizer's coverage where the GPU's
  32-bit corner lands on the other side of a 1/256 pixel step than the reference's 64-bit one.
- **The hand-made scenes were not enough.** They were green on all four adapters while a real fault was
  there. 300 random scenes found it on the first try: in the iso view a whole row of a sprite went the wrong
  way where the sprite crossed the top of a box close to a pixel center (46 and 53 pixels). The iso range
  was stored in a straight line, which puts the scene at 0.5 of the depth buffer, where a float is 100 times
  coarser than the reference's depth. The range is now laid along the float's exponent, 60 times finer:
  3,000 random scenes pass on the M5 Pro with at most 7 pixels apart, and 200 of them are in the gate.
- **Options, measured on all four adapters** (the table is in the ADR; `cargo xtask gpu --study` repeats it):
  - The GPU's own blender is exact on Metal and WARP and wrong on lavapipe in nearly every see-through
    pixel. So see-through colors are mixed in the shader.
  - The rank as a depth bias: 2 to 19 more pixels at 16 depth steps, 105 pixels inside faces at 256.
  - Rounding the color in the shader gave the same picture as the table on every adapter. The table stays
    because it has no arithmetic to get wrong.
  - Projecting 50,000 boxes on the CPU, as the reference does, takes 6.2 ms by itself. The GPU draws the
    whole frame in 1.0 to 1.75 ms.
- **Found in the reference renderer** (Q17 in [open questions](open-questions.md), the owner's call):
  - A tile larger than one cell loses 32 % to 76 % of its pixels to the ground under it when the camera is
    turned or in perspective. The rank rule's tolerance is smaller than what snapping corners to the 1/256
    grid does to the depth. The tests only had tiles of one cell, whose triangles are the ground's own.
  - The part of a card below its anchor is in the floor's plane and has no rank. In the starting view the
    floor always wins, which is the opposite of what ADR 0020 says; under other cameras it is a coin toss.
  - Two solid things that share a plane (two boxes in one cell) are decided the same way: 30 of 300
    unconstrained random scenes failed for that alone.
  - A fix was tried in a scratch copy (about 60 lines: a level face's depth from its plane; a card's lower
    part wins against a floor in its plane). All ten playtests kept their image hashes, the render tests
    passed, painting stayed at 151 µs. It is not in this change. The GPU check keeps these cases out of its
    scenes and prints the first two with `--study`.
- **Surprises.**
  - Windows already uses all 6 allowed proc-macro crates: two come with Direct3D 12. Linux and macOS use 4.
  - Without a window library Windows is the widest system (76 rented crates), not Linux (65).
  - The macOS runner draws the 50,000-box frame in 6.3 to 7.0 ms, where the study's spike took 2.8 ms. This
    frame has 50,176 ground cells as well, and level faces write their depth per pixel. On the M5 Pro the
    per-pixel depth made no measurable difference.
  - WARP takes 613 to 715 ms to draw 50,000 boxes when every instance is uploaded again. A player on a
    machine with no GPU would need the CPU painter, which is what the web's fallback is for.
- The agent's half, before and after: web module 619,742 → 619,866 bytes, command 1,979,088 → 1,979,136,
  clean release build 2.8 s → 2.8 s, clean dev build 2.2 s → 2.3 s, the same 16 rented crates and the same
  lockfile, paint 151 to 160 µs (limit 200). All ten playtests keep their state and image hashes.
- Not done: the window (piece 3), input, the web player, a GL backend, the fix to the reference.

## 2026-11-26 — wgpu runs on all three GitHub runners; the GPU picture can match the reference to a few edge pixels

- Step 3, piece 1: a measurement before the GPU check is designed. A new workflow, `gpu-probe.yml`, runs the
  study spikes `gpu_native` and `cpu_ref` on GitHub's hosted runners (runs 45756166643, 45757026172 and 45757795785, which gave the same picture hashes; tables
  and how to run it in [the spikes README](proposals/3d-architecture/spikes/README.md#on-githubs-runners)).
- **Every runner draws the town, at the first try.** Each offers one adapter, and wgpu picks it by itself:

  | runner | adapter | device type | install | ms per frame, 5,000 / 50,000 / 500,000 boxes |
  |---|---|---|---|---|
  | `ubuntu-latest` | Mesa lavapipe (Vulkan), Mesa 26.0.8 | `Cpu` | `mesa-vulkan-drivers`, 19 MB, 6 to 28 s | 7.5–9.1 / 49–69 / 364–479 |
  | `ubuntu-latest` | Mesa llvmpipe (GL 4.5) | `Cpu` | `libegl1 libegl-mesa0`, 164 kB, 4 to 19 s | 9.0 / 49–55 / 440–481 |
  | `windows-latest` | WARP, "Microsoft Basic Render Driver" (DX12) | `Cpu` | nothing; no `force_fallback_adapter` | 5.6–8.4 / 41–63 / 265–452 |
  | `macos-latest` | Apple Paravirtual device (Metal) | `IntegratedGpu` | nothing | 0.75–1.1 / 2.7–2.9 / 9.3–9.7 |

  On the software adapters the CPU reference is about as fast as the "GPU" at 5,000 boxes and faster above.
- **The macOS runner's GPU pictures are byte-identical to the M5 Pro's**, so they differ from the reference in
  the study's 1, 8 and 18 pixels.
- **Lavapipe, llvmpipe and WARP differed in about 36,000 pixels (3.9 %), nearly all by one level.** Looked at:
  all of those pixels are on the three face colors where color × 0.35 × 255 is an exact .5 tie (the
  ambient-lit sides). They come in irregular patches over about 70 % of each such face, always one level down.
  The software rasterizers interpolate the constant color with a tiny error. Two shader variants settled it.
  Not interpolating the color (`flat`) made it worse: whole tie faces one level down, 50,966 pixels, the same
  picture on all three software adapters. Also rounding to a whole level in the shader (`flat-rounded`) removed
  every one-level difference: **3 pixels at 5,000 boxes and 10 to 13 at 50,000 on all three**, against Metal's 1
  and 8. What remains, on every adapter, is coverage. Each is a lone pixel where two faces meet, painted in the
  other face's color, 16 to 180 levels off, and always on an edge of the reference. No pixel differed by 2 to 15
  levels anywhere.
- **No GPU picture can be a reference image across systems.** Apart from the macOS runner against the M5 Pro,
  every pair of GPU pictures differs: by 16,000 to 25,000 pixels with the study's shader, by 1 to 9 edge
  pixels with `flat-rounded`. The CPU reference gave the same three hashes on all four runners and the M5 Pro.
- **Recommendation for the step-3 check.**
  - Backends: Vulkan on lavapipe on Linux, because it is wgpu's first choice there and the path players with
    Vulkan drivers take. It is as fast as GL and gives the same `flat-rounded` picture at 5,000 boxes. GL on
    llvmpipe works too and installs less. DX12 on WARP on Windows and Metal on macOS need nothing installed.
  - The renderer writes whole 8-bit levels, rounded as the reference rounds them, with the color not
    interpolated (`flat-rounded`, measured). Safer, not measured: give the GPU each face's 8-bit color as
    computed by the reference's own function, so the GPU does no color arithmetic at all.
  - The limit: no per-channel tolerance, no differing pixel inside a face (the reference knows each pixel's
    owner, so "on an edge" can be checked), and a small count of edge pixels. The worst measured was 28 of
    921,600 at 500,000 boxes (that one with the study's shader: the variants ran up to 50,000). A limit of
    0.01 % (92 pixels) of a 1280 × 720 frame would leave three times that margin. The real check's scenes will
    have far fewer edges, so set the count on them. If the study's smooth shader were kept instead, the
    check would need both a 1-level tolerance over about 4 % of the pixels and a count.
  - Cost: building wgpu and its dependencies took 42 to 57 s cold on Linux and macOS and 121 s on Windows, and
    1 to 10 s with a warm cache. Linux adds 4 to 28 s of apt (the download is about 1 s; the rest is apt itself). Drawing a picture once takes under a second on
    every adapter, even at 500,000 boxes, once the device is ready (0.1 to 0.5 s). Inside the existing `ci`
    job that comes to an estimated 2 minutes more on Windows with a cold cache, and under 30 s with a warm one. A separate job, like the probe,
    took 35 to 91 s with a warm cache and 90 to 192 s cold.
- Not done or not tried: the variants at 500,000 boxes; the "8-bit color from the CPU" variant; Vulkan or GL on
  Windows (the runner lists neither) and MoltenVK on macOS; arm64 Linux and Windows runners; llvmpipe without
  installing `libegl1`; timing variance beyond three runs. The probe stays as a workflow that runs only when the
  spikes change. The engine has no GPU code yet.

## 2026-11-26 — The 3D core: Tiny Town plays in 3D from its unchanged files

- The owner said to start step 2 of the 3D order. Built, on the branch `3d-core`
  ([ADR 0020](decisions/0020-render-list-camera-and-cpu-reference-renderer.md) has the decisions and the
  measured options):
  - **The render list** replaces the 2D draw list for the world: solid instances (a box per `block`, a flat
    rectangle per `tile` and ground cell), cards (a world entity's `sprite` and `label`) and the 2D layer.
  - **The camera is session state**: `iso` or `perspective`, turn, zoom, pan. `[camera]` in a scene gained
    `view`, `turn` and `zoom`; playtests gained `[[input]] camera = { ... }`. Sine and cosine are owned
    (`rwgpuengine_core::math`).
  - **The CPU reference renderer** draws through a depth buffer; the frame report and picking read its
    projection.
- **Done when, and it is:** all nine existing playtests end in the state hashes they had, from files that
  did not change. A tenth, `turn_and_build`, builds a house, a tree and a well from three cameras.
- **The pictures changed, and were looked at.** Tiny Town's two differ from the old ones by one color level
  on box sides. The other two also differ along the tops of boxes 0.8 and 1.2 high, which are now drawn at
  25.6 and 38.4 pixels, not at a rounded whole pixel.
- **Findings on the way.** The first run through the new renderer passed every state check, and the three
  flaws that mattered were found by the new tests, not by the examples:
  - A flat thing drawn a little nearer the camera, the usual way to put a tile on the ground, left a line of
    tile color along the foot of a crate standing on a plate. Level faces now carry a rank instead.
  - A sprite drawn at one depth was cut by a wall *behind* it, because a camera looking down sees the top of
    that wall as nearer. A card now stands upright above its anchor and lies on the surface below it.
  - Painting for a player took 263 µs against a limit of 200. Solving each triangle row's run and writing
    through row slices brought it to 161. The limit was not changed.
- Measured (Apple M5 Pro): reference frame with report 1,587 µs (it was 2,225); paint 161 µs (59); a town of
  5,000 boxes at 1280 × 720 in 5.2 ms against the proposal's 20 ms, now a budget in the gate; web module
  619,742 bytes (585,789); command 1,979,088 bytes. Under Node the wasm module reaches the native state and
  image hashes for all ten playtests, turned and perspective frames included.
- Not done: no GPU, no window. The camera cannot tilt and entities cannot turn. The web player has no camera
  gestures yet, `shot` has no flag to move the camera, and a playtest cannot `[[expect]]` a camera. The frame
  report of a 5,000-box town takes about 14 ms on top of its frame and is not budgeted. The ground is one
  instance per cell.

## 2026-11-26 — The first CI run is green on Linux, Windows and macOS

- The first run on GitHub (run 45679193089, on the published tip, 2026-11-25 UTC) passed on all three
  systems at the first try: `cargo xtask ci` end to end, 135 tests on each, every budget, and the wasm check.
  **Windows had never run the gate before.** Nothing needed fixing.

  | runner | host | job time | CLI bytes | wasm bytes | render µs | paint µs | smoke ms | rebuild ms |
  |---|---|---|---|---|---|---|---|---|
  | `ubuntu-26.04` | `x86_64-unknown-linux-gnu` | 1 m 12 s | 2,178,568 | 585,725 | 6,042 | 174 | 21 | 1,452 |
  | `windows-2025` | `x86_64-pc-windows-msvc` | 2 m 14 s | 2,113,024 | 586,069 | 7,187 | 191 | 68 | 3,038 |
  | `macos-26-arm64` | `aarch64-apple-darwin` | 1 m 18 s | 1,912,320 | 585,749 | 3,752 | 106 | 32 | 1,335 |

- **Windows is a sixth target with the same results.** The smoke chain ends in state `94fd42c60d7a4c92` on
  all three runners, and all nine example playtests reach the same state and image hashes there as on the
  reference machine (`82c150f3d192812e` and `58bd8a0dbea14409` for Tiny Town's `build_town`). On each runner
  the wasm module, built on that runner and run under its Node (24 on Linux and macOS, 22 on Windows),
  reached the native command's hashes for all nine.
- **The time scale earns its place.** One 640 × 480 reference frame took 6,042 µs on the Linux runner and
  7,187 µs on Windows, against an unscaled limit of 6,000: without `RWGPU_TIME_BUDGET_SCALE` both would have
  failed with no regression. With the scale of 4 the worst measurement is at 30 % of its limit.
- **Finding: the wasm module is not the same bytes on every build machine** (585,725, 586,069 and 585,749
  on the runners, 585,789 on the reference machine), although it behaves identically. A build contains the
  build machine's paths: the reference machine's module holds 21 strings under the builder's home directory,
  next to rustc's and cargo's registry paths. Those paths differ in length per machine, which is the likely
  cause of the size difference; that was not proven with a remapped build. So the `dist` build that remaps
  paths, already owed before any binary release, covers the **web module** as well as the command: a web
  export built today would show the paths of whoever built it.
- CI change in this commit: `actions/checkout` v4 → v7. The run warned that v4 targets Node 20, which the
  runners no longer use.
- Not covered by CI: any GPU (the 3D spikes are not in the gate), Linux on arm64 and macOS on x86-64 (both
  measured by hand earlier), and a browser (Node runs the wasm module; Chromium was measured by hand). The
  runners' Node version is whatever the image ships.

## 2026-11-25 — Published: the first push to GitHub

- The owner approved the rewrite and the push. The repository is public at
  <https://github.com/engrmnaveed/rwgpuengine>; `main` holds everything up to here, and the tag
  `baseline-2026-11-25` marks the first baseline.
- **The history was rewritten once, before anything was public** (the audit's D3): every commit's author and
  committer is the owner's GitHub login with GitHub's no-reply address; the `Co-Authored-By` lines the owner
  asked to drop are gone from all messages; the direction PDF is in no commit ([vision.md](vision.md) restates
  it). Dates and content are unchanged: the tree at the tip is the same, file for file, apart from the PDF. A
  rehearsal in a scratch copy checked all of this first, and a backup of the original history is kept
  outside the repository.
- Commit hashes changed with the rewrite. Hashes quoted in earlier entries and in the experiment reports
  (`a028bea`, `ae52e90`, `ffcb792`, `0304e59` and others) are the old ones; the tag still points at the same
  baseline content.
- Not done: the first CI run on Linux, Windows and macOS had not finished when this was written.

## 2026-11-25 — Open-source readiness, prepared up to the push

- The owner said to continue after the 3D proposal, and created the public repository. First step of the
  proposal's order: get ready to publish. Done locally, in one commit:
  - **LICENSE** (Apache-2.0, the text the Rust project's crates ship) and `license` / `repository` in every
    crate's metadata.
  - **The wasm check is in the gate.** `cargo xtask wasm-check` runs `web/check.mjs` (moved from the audit's
    tools) for every example: the wasm module must reach the native command's state hashes and paint its
    `shot` pixels. Without Node it is skipped locally and fails in CI.
  - **CI for Linux, Windows and macOS** (`.github/workflows/ci.yml`), read-only permissions, with
    `RWGPU_TIME_BUDGET_SCALE=4` because the time budgets are written for the reference machine.
  - **Portability:** the command's path gets the system's suffix (`.exe`); `.gitattributes` fixes line
    endings to LF, because spans and bundles are bytes.
  - **No build-machine path in a release command from our own code:** `serve` only knows the source tree's
    wasm path in debug builds.
  - README rewritten for a first reader, with example output that is real again (its hashes had gone
    stale); a short CONTRIBUTING.md.
- **Not done, waiting for the owner's word because they cannot be undone:** rewriting the history (author
  and committer to the owner's GitHub no-reply address, co-author lines removed, the direction PDF left out)
  and the first push to the public repository.
- Not done at all: Windows has never run the gate, so the first CI run there may fail and need fixes.
  Release binaries still contain cargo's registry path of whoever builds them; a `dist` build that remaps
  it is needed before any binary is published, not before publishing source.

## 2026-11-25 — The 3D direction study: a proposed architecture, with spikes

- [The 3D architecture proposal](proposals/3d-architecture.md) is written and waits for the owner. Its core:
  **two halves joined by one render list.** The agent's loop stays free of the GPU and checks 3D scenes with
  an owned CPU reference renderer; players draw the same list with wgpu, natively and in the browser.
- Measured in four spikes ([code](proposals/3d-architecture/spikes/)), Apple M5 Pro:
  - **wgpu 30 to build:** 59 crates on macOS (80 with a window library, 113 on Linux), 4 proc macros, 10–12 s
    for a clean build, and **0.3 s to rebuild after an edit**, the same as today.
  - **wgpu to run:** 5,000 / 50,000 / 500,000 flat-shaded boxes at 1280 × 720 in 0.37 / 0.54 / 1.93 ms
    natively and 0.47 / 1.02 / 2.26 ms in Chromium, **with identical pixels** in both.
  - **wgpu on the web:** the WebGPU-only build of the spike is **116,791 bytes**; adding the WebGL2 fallback
    makes it 1,850,824.
  - **A CPU reference renderer** (150 lines, no dependencies) draws the same scenes in 4.9 / 11.8 / 63.9 ms
    and differs from the GPU frame by **1 / 8 / 18 pixels of 921,600**. Its images are the same on arm64 and
    x86-64, and its owner buffer gives the frame report.
  - **The simulation today is quadratic:** 1.3 ms per step at 1,000 moving actors, 26.6 ms at 5,000 and
    2.8 s at 50,000. With a cell index the same movement takes 0.15 ms at 50,000, and 1.5 ms at 500,000 with
    positions in a column.
- Found on the way: the new GitHub repository is public, so the first push publishes at once; wgpu 30's
  `get_mapped_range` returns a `Result`, and vertex buffer layouts are wrapped in `Option`.
- Not done: nothing from the spikes is in the engine. No window was opened, no phone, Windows or Linux GPU
  was measured, and physics, hot reloading, mesh import and fonts were not studied.

## 2026-11-25 — The engine is named rwgpuengine (ADR 0019)

- The owner chose **`rwgpuengine`** (Rust + wgpu + engine) from the second shortlist. The rename is done in
  one commit ([ADR 0019](decisions/0019-name-rwgpuengine.md)): the command is `rwgpu`, the crates are
  `rwgpuengine_*`, the project file is `rwgpu.toml`, the web ABI is `rwgpu_*`.
- Two things changed that are not only a name: the first key of every `--json` output is now `rwgpu`, so
  `format` is 2; and the JSON Schemas' `$id` is a `urn:`, because `slate.dev` was never the project's domain.
- Proof that nothing else changed: 99 command outputs and generated files compared between the build before
  and the build after, with the name substituted in the old ones: 0 differ, PNG files included. All nine
  playtest state hashes are the same, the web module still matches native, and the generated AGENTS.md is
  the same 67,313 bytes ("slate" and "rwgpu" are the same length).
- Old records (ADRs 0001–0018, experiment reports and their archived projects, earlier log entries, the
  baseline, the audit, the handoff brief) keep the old name on purpose. They quote commands and prompts word
  for word.
- Not done: nothing is published or reserved on crates.io; no crate is named plain `rwgpuengine` yet (that is
  decided with the 3D crate layout); the LICENSE file.

## 2026-11-25 — Name shortlist (Q1)

- The owner asked for the name before the 3D study, and said it may carry the 16xbrains family branding
  (`diskuse16x`, `macdisplay16x`). [Shortlist](proposals/name.md): **`game16x`** recommended, then `forge16x`,
  `diorama16x`, `sim16x`, `play16x`, `world16x`.
- Checked: 42 names of the form `<word>16x` are all free on crates.io (also with `_core`) and npm; the ten
  finalists are unused as GitHub repository and account names; no web pages use them.
- Finding: another vendor ships "16x Prompt", a tool for AI coding assistants, at `16x.engineer`. That rules
  out `engine16x` and is noted for the owner.
- Hosting (Q10) now reads: under the 16xbrains account, authors "The 16xbrains developers".
- Not done: the rename itself; it waits for the owner's pick.
- **Second round, same day.** The owner dropped the `16x` suffix for this project, asked for a name after the
  technology (Rust and wgpu) in the form `<alias>engine`, and set hosting to the owner's own GitHub account.
  New [shortlist](proposals/name.md): **`rwgpuengine`** recommended (command `rwgpu`), then `oxigpuengine`,
  `ferrogpuengine`, `rsgpuengine`.
  - Checked: 33 names of that form on crates.io (also as `<alias>_engine`) and npm; all free but `rustengine`.
  - Ruled out by what was found: `wgpuengine` (an existing C++ WebGPU engine has the name), `rgpuengine`
    (`rgpu` is a known remote-GPU project), every name that spells "Rust" (the trademark texts differ on
    product names), and aliases already taken as crates (`rwg`, `ferro`, `oxide`, `oxi`, `ferris`).

## 2026-11-25 — The owner's decisions and a course correction: 3D on the GPU

- The owner answered the audit's ten decisions and corrected the direction. The earlier documents had read
  "keep it simple" as "2D and 2.5D on the CPU"; the owner meant simple **assets**.
  - **The target is 3D, the GPU is a must, and the renderer is written on wgpu.**
  - **Native and web from the start**; phones and tablets must be reachable; no over-engineering, and no
    bending for older systems and hardware.
  - **Kinds of games:** strategy and planning, city builders (towards Cities: Skylines), car-building and
    simulation, children's games (fun and STEM); learning games stay possible. A template for each.
  - **Scale:** a town first, on an architecture sized for a city.
  - **Licence: Apache-2.0. Rename: yes** (shortlist to come).
- Delegated to the architect and decided: 3D art is shapes written as text first; identity in the public
  history; where data logic stops; audio as events; hosting. Reference games 2 and 3 and the resolution
  question are reopened by the 3D direction.
- Recorded in [vision.md](vision.md#direction-update--2026-11-25-owner) (the update wins over the PDF),
  [open-questions.md](open-questions.md) (new Q16: the 3D architecture) and the [roadmap](roadmap.md).
- What it changes for the plan: the audit's steps 5–9 are replaced by a **3D direction study**; the split of
  `load.rs` and `world.rs` waits for the 3D world model. What it does not change: the agent-native
  principles. The GPU is for players; the agent's loop must never need one.

## 2026-11-25 — A paint path for players (ADR 0018)

- The owner chose to consolidate before the public push. First consolidation step: the web player's frame
  ([ADR 0018](decisions/0018-paint-path-for-players.md)).
  - `slate_render::paint` gives the reference rasterizer's pixels by solving each shape's coverage rule for
    whole runs of pixels. `rasterize` is unchanged and stays the reference for `shot`, `test` and reports.
  - `slate_runtime::Presenter` repaints only when the draw list changed; the web ABI gains `slate_present`
    and `slate_pixels`, and `web/player.js` copies pixels to the canvas only then.
- Measured (Tiny Town, `build_town`, frame 120):
  - a changed frame, native: 2,256 µs → **55 µs** (budget `time.paint_frame_us = 200`);
  - a changed frame, wasm in Node: 2.81 ms → **0.09 ms** at 640 × 480; 42.2 ms → **0.91 ms** at 2560 × 1920;
  - repaints over the nine example playtests: 1,569 → **140**;
  - web wasm: 643,317 → **585,725 bytes** (the reference rasterizer and the report left the web module).
- Pixel identity: 9,000 random shapes against `covers`; a scene with every shape kind; every frame of all nine
  playtests; and in wasm against native `slate shot` for all nine. Five mutation checks fail the shape test.
  In Chromium the canvas's own pixels hash to the native image hash, and 120 live frames made 2 canvas copies.
- Found on the way:
  - The audit's wasm estimate was wrong: the first paint path was six times slower in wasm than natively at
    high resolution (no vector instructions). Filling long runs by block copy fixed it (3.9 ms → 0.91 ms).
  - Clippy 1.100 rejects `chunks_exact_mut` with a constant size; `as_chunks_mut` is the form to use.
- Not done: a presentation scale; anything on the GPU. Both now belong to the 3D direction (the entry above).

## 2026-11-25 — Architecture audit by the senior architect session

- The new supervisor session (Fable 5.2) audited the baseline against the vision:
  [audit report](audits/2026-11-25-architecture-audit.md). No engine code changed.
- **Verdict:** the base is sound; the direction has drifted. None of the vision's three differentiators is
  started (Rust game code has zero users, no tablet was measured, no learning kit exists), and a game still
  cannot be shipped, have levels, play sound or save progress.
- **Measured for the first time:**
  - Determinism on three more native targets (x86-64 macOS, x86-64 Linux, arm64 Linux with all 128 unit
    tests) and on wasm under Node with no browser: all 9 playtests give equal state hashes, and the wasm
    gives equal image hashes too. The native-equals-web check can be a CI gate.
  - The web player spends 97 % of a frame on work it can skip. A span rasterizer prototype is
    pixel-identical on 1,932 of 1,932 frames and 38–56 times faster than rasterize + report; only 8.4 % of
    frames change any pixel. On that evidence wgpu can wait (Q4).
  - A mutation fuzz of 2,000 runs: no panic, no malformed JSON, 7 hangs, all from `footprint = [4096, 4096]`
    (schema-valid, `check` passes, `shot` runs for more than 90 s).
  - 61 % of the generated AGENTS.md is the format reference that `slate explain` already serves.
  - Release binaries contain the build machine's home path (48 strings in the CLI, 21 in the wasm).
- Ten owner decisions (D1–D10) and a nine-step plan are in the report. The scripts are in
  [audits/2026-11-25/tools/](audits/2026-11-25/tools/).
- Not done: no tablet, Windows or Safari run; no cold-agent run; nothing merged from the prototype.

## 2026-11-25 — Size pass, baseline, pause for the senior architect

- **Size pass** ([ADR 0017](decisions/0017-web-profile-and-budgets.md)). I measured the web build in the
  browser at five optimisation settings, all giving identical hashes:
  - `z` alone was 43 KB smaller but 22 % slower per frame;
  - `3` alone was 37 % faster but 80 KB bigger;
  - the adopted mix (size-first, with the renderer and core at `3`) is **643,317 bytes (−31 KB) and 2.72
    ms/frame (−37 %)**, better on both.
- **Budgets:** wasm raised to 800,000 at the owner's request. New `time.render_frame_us` (native release,
  measured 2,253 µs, limit 6,000). `budgets` checks the web profile's renderer overrides.
- **The owner set the order from here:**
  1. pause core work at a baseline;
  2. the owner reviews;
  3. a senior architect session (Fable 5.2, Extra mode) audits against the vision, consolidates the core, lays
     the platform foundation and becomes the main supervisor;
  4. then demos with procedural build logs;
  5. then a desktop app for game producers, bridged to Claude Desktop (other agent platforms later).
- Written for that: [baseline](baselines/2026-11-25.md) (tag `baseline-2026-11-25`), the
  [architect brief](handoff/senior-architect-brief.md), the [desktop app proposal](proposals/desktop-app.md),
  Q15, and the roadmap's "Now" section.

## 2026-11-25 — Spawn costs and the `funds` refusal (M3.4)

- Built what runs #8 and #9 both asked for ([ADR 0016](decisions/0016-spawn-costs-and-funds-refusal.md)):
  `cost = { entity = "town", prop = "coins", amount = 3 }` on a spawn.
  - It is checked first; too little is `blocked` + `refused` with reason `funds`, so "not enough coins" uses
    the same path as water or a taken cell.
  - It is paid only if the copy is placed; the `spawned` event shows `paid`, and the payer emits `changed`.
- It replaces three hand-written parts per price: the coin condition, the payment, and the mirror "not
  enough coins" connection that had to come first in the file. That ordering trap is gone.
- Tiny Town: no condition mentions coins any more; 20 connections, 291 lines.
  - Same state hashes and images, native = web.
  - `refusals` proves the coin case as a `funds` refusal.
  - Mutation checks fail as expected (a wrong price: all three tests; a misrouted message: `refusals`).
- Wasm 674,719 bytes (+9.6 KB; 25 KB of headroom). A size pass is now a candidate before the next large
  feature.

## 2026-11-25 — Cold-agent test #9: run #8's park with `actions`, 41 → 24 connections

- Same task, word for word, as run #8, on the build with `actions` lists. Every requirement was supported
  with 0 warnings.
  - **24 connections instead of 41** (−41 %; ADR 0015 estimated 27); 369 scene lines instead of 470.
  - 5 min 54 s instead of 6 min 24 s; 21 tool calls; ~132K tokens.
  - It wrote `[spawn, pay, clear message]` lists and put each coin message before its spawn, as the recipe
    says.
- Re-verified, including two mutation checks. One is a lesson: moving a coin message after its spawn passed
  all 7 of its tests, because none spent the coins below a price. The recipe now says how to prove that
  order.
- Fixed:
  - `-o` was read as a path; short flags are now an error that names the long flag;
  - list messages say "action 3 of 3" (data `item`);
  - `[[expect]] event = "action"` takes `changed = true`, so real transitions can be counted (Tiny Town:
    the banner gets 11 `set`s, 1 real change);
  - recipes: clearing a message only on success; copies with overrides still count; tap pixels on shape
    edges;
  - limits: changing looks at runtime.
- Two runs now ask for the same two things: costs as data with a `funds` refusal, and a "not equal"
  condition. [Experiment report](experiments/2026-11-25-cold-agent-9.md).

## 2026-11-25 — Several actions per `[[connect]]` (M3.3)

- Built run #8's top request ([ADR 0015](decisions/0015-several-actions-per-connect.md)):
  `actions = [{ action = ..., to = ..., ... }, ...]` in one `[[connect]]`, sharing its source, signal and
  `when`.
  - **`when` is checked once.** Separate connections re-check after earlier ones acted: "pay, then say
    bought", both on `coins >= 3`, skipped the second when the last coins were spent.
  - **A refused spawn stops the rest of the list**, so `[spawn, pay]`, the first thing an agent writes, never
    charges for a house that was not placed.
  - **Each action still logs its own event**, so expectations keep their meaning.
- Tiny Town: 27 → 20 connections and 354 → 299 lines, ending in exactly the same state hashes and images.
  It is the same game, and native = web.
- A new `refusals` expect guards the one ordering rule this makes visible. A "not enough coins" check must
  come before a spawn that pays in its list. Two mutation checks (paying before spawning; hints after the
  spawn) fail it.
- Wasm 664,154 bytes (+6.6 KB). Old examples unchanged.

## 2026-11-25 — Cold-agent test #8: a park builder on kit v2, accepted

- The task asked for both v2 features directly:
  - three "why not" messages (water or path, taken, coins) that clear on the next success or tool choice;
  - a sign whose counts include two trees already in the scene;
  - a "park is open" goal that switches off again.
- Every requirement was supported, with 6 playtests (78 expects) and 0 warnings. 6 min 24 s, 19 tool calls,
  ~135K tokens: the fastest builder run, on the largest task. Re-verified independently, including two
  mutation checks.
- It used `counts` with no counting connections and `refused` with all three reasons. It handled `edge`,
  which the task did not mention.
- Fixed:
  - `inspect --place` prints only the answer;
  - unnamed copies are `#10 (house)` in every table and message;
  - the recipe says a tap on a building never reaches the ground, and that `when` is checked before
    placement;
  - tap pixels of screen labels move with their text;
  - "not equal" and per-terrain messages are listed as limits.
- The gaps have moved from capability to volume: the park needed 41 connections, 12 for buttons alone.
  Merging connections that share source, signal and `when` would make the three builder games 27 / 19 / 24
  instead of 41 / 30 / 27. That is the next proposal. [Experiment report](experiments/2026-11-25-cold-agent-8.md).

## 2026-11-25 — Builder kit v2: `refused` and `counts` (M3.2)

- Built the two gaps that Tiny Town and run #7 both showed
  ([ADR 0014](decisions/0014-refusal-signal-and-count-props.md)):
  - **`refused`**: a placement that fails makes the signal's source emit `refused` (the ground, for ground
    taps). `when = [{ reason = "terrain" }]` picks one reason (`edge`, `terrain`, `occupied`).
  - **`counts`**: `counts = { houses = "house" }` is an integer prop the engine keeps equal to the number of
    enabled copies. It updates in the step that changes it, logs `counted` and emits `changed`.
- Chose a count *prop* over the requested count *condition* on KPIs. A condition alone has nothing to
  re-check on after a removal, and file order would decide what it sees. It also cannot be shown in a label.
  A prop fixes both and adds no new condition kind.
- A `when` failure ("not enough coins") is still `skipped`, not `refused`. In a toolbar game every other
  tool's spawn is skipped on each tap, so a signal there would fire constantly.
- Tiny Town:
  - the job card is `counts` (6 counting connections → 0);
  - a hint line says "Build on grass!", "No room there!" or "A house needs 3 coins";
  - 23 expects in `refusals` (up from 14);
  - two mutation checks fail exactly the expectations that cover them.
- Old examples keep their exact hashes. Tiny Town native = web on all three playtests. Wasm 657,498 bytes
  (+15.7 KB). `get_mut` instead of `BTreeMap::insert` saved 3.2 KB of map code.

## 2026-11-25 — Cold-agent test #7: a farm builder, accepted

- A new builder task (pond and path terrain, a 3 × 2 barn, crops, fences, seeds on screen, remove with refund,
  a ready message, six required proofs) on builder kit v1. Every requirement supported, nothing approximated
  except the extra feedback the engine cannot give. 6 playtests with 59 expects, 0 warnings. 8 min 44 s,
  28 tool calls, ~156K tokens. Re-verified independently. ADR 0013's acceptance is met.
- Its most important finding was a **false pass**: `slate test out/x.toml` ran the project's tests instead of
  that file. Now it runs exactly that file, and a scene or prefab path is an error; the smoke chain guards it.
- Also fixed:
  - event expectations filter by `prefab` (Tiny Town's refusals test uses it);
  - `[[terrain]]` and `when` condition tables are in AGENTS.md;
  - the recipe explains why tapping a building never yields `occupied`;
  - removal wording is consistent;
  - `inspect` labels `screen_pos`.
- Next, with evidence from two games: a refusal signal and conditions that count copies.
  [Experiment report](experiments/2026-11-25-cold-agent-7.md).

## 2026-11-25 — Q3 decided: the isometric builder; builder kit v1 and Tiny Town (M3.1)

- The owner picked the isometric builder as reference game 1. Tiny Town ([spec](games/tiny-town.md)): tap a
  tool, tap the island, build 2 houses, 3 trees and a well within 12 coins, with Clear to take things back.
  Built entirely in data; 3 playtests (`build_town`, `refusals`, `clear`). Two mutation checks (an
  affordability rule, a refund value) fail exactly the playtests that cover them.
- Builder kit v1 ([ADR 0013](decisions/0013-first-reference-game-isometric-builder.md)):
  - terrain maps (`[ground] map` + `[[terrain]]` with `build`);
  - ground taps (`from = "ground"`);
  - `spawn` with placement rules, refusals logged as `blocked` (`edge`/`terrain`/`occupied`);
  - multi-cell `footprint`, drawn as per-cell slices;
  - `from_prefab` and `to = "self"`, `block.top`;
  - `[[expect]] prefab` + `count`;
  - `slate inspect --place house[@x,y]`, with a guaranteed tap pixel per cell.
- Found while building:
  - **Data-only entities blocked building at [0, 0].** A props-only `town` entity sits at the default
    position and counted as occupying it. Now only entities that are drawn, solid, pushable, actors or
    triggers take up cells (a sim test caught it).
  - **Hidden cells can't be tapped.** A cell hidden behind a house has no tap pixel, correctly: a player
    can't tap it either. `--place` says so instead of handing out a pixel that would hit the house.
  - **Agents needed tap pixels for ground cells.** They had them for entities only; `--place` now gives them.
- Determinism: `build_town` reaches state `537ae6f9300e9b67` and image `58bd8a0dbea14409` natively and in
  the browser. Older scenes keep their hashes and images. Live tapping in the web player builds as expected.
- wasm: 586,226 → 640,798 bytes (+9.3%, budget 700,000). Replacing a `BTreeMap<String, EntityDef>` with a
  `Vec` saved 15 KB of generic code. `opt-level = "z"` measures 600,909 but is not adopted until frame time is
  measured.
- The scene has 26 connections, 9 of them for clear/refund. That is the evidence to watch for grouped
  connections or per-prefab costs.

## 2026-11-25 — Cold-agent test #6: the vault task with no stand-ins

- On the pushable build, the agent built the full puzzle: three coins counted on a HUD, two crates pushed onto
  the plates, and the player walks into the vault. 0 warnings, 5 playtests with 49 expects plus its own
  mutation check. Re-verified independently.
- 7 min 41 s, 29 tool calls, ~136K tokens. That is the fastest of the four vault runs (18:42 → 14:06 →
  10:02 → 7:41), and the first file was written at 2:39.
- Its table kept one conservative "approximated" (no second character to hold a plate), prompted by our
  limits wording. That row is reworded.
- Fixes from its findings:
  - the starter `crate` prefab is now pushable, and a new `wall` prefab does the blocking;
  - crate-off-plate behaviour is documented;
  - the report says when a plate's coverer stands on it;
  - `slate test` prints "final state";
  - back-to-back holds are clarified.
- [Experiment report](experiments/2026-11-25-cold-agent-6.md).

## 2026-11-25 — Pushable crates hold plates (ADR 0012)

- `pushable = true`: a mover walking into it pushes it one cell if the cell beyond is free. Otherwise the
  mover is refused with the new reason `push`, which says what is in the way. Patrols and wanderers can't
  push. Triggers and `occupied` count pushables, so a crate holds a plate.
- coin_vault's statue workaround is now a crate the player pushes onto plate_a. The solve test asserts the
  crate's position and a refused second push at the ground's edge. Making the crate an immovable solid on a
  copy fails exactly the two playtests that should fail.
- Found while testing: a mover that pushes a crate off a plate steps onto that plate in the same step, so the
  plate never empties and never emits `released`. That is correct, and a test covers it.
- Hashes of scenes without pushables are unchanged (iso_garden `469ae7c92124d43d`).

## 2026-11-25 — Cold-agent test #5: logic supported, two targets missed

- Run #3's task on the data-logic build. AND door, re-closing and the on-screen counter were all supported
  with no approximation (`when`, `add`, `label`, `screen_pos`). The first scene checked clean, 0 warnings
  throughout, 3 playtests with 42 expects. Re-verified independently.
- Time: 10 min 2 s (run #4 14:06, run #3 18:42); 33 tool calls, ~149K tokens. The scene is a third the size
  of run #4's.
- ADR 0011's acceptance targets: "≤ 5 min" was missed. "No approximations" was miscalibrated: the task needs
  two plate holders, and objects can't hold plates (a separate limit ADR 0011 didn't cover). Every verdict was
  reachable at 0:26; most of the remaining time was designing around the second holder.
- Fixes from its findings:
  - the AND recipe says who can hold the plates;
  - the limits table carries the wording tasks use;
  - `action` events carry `changed` and the docs admit no-ops;
  - W0304's title is more precise;
  - reports print `screen` for HUD entities;
  - an entity's self-overlap no longer counts as hidden.
- Evidence for the next feature: runs #3–#5 all built a patrol stand-in to hold a plate.
  [Experiment report](experiments/2026-11-25-cold-agent-5.md).

## 2026-11-25 — Data logic v1 built (M1.6)

- Built ADR 0011's set: `[[connect]]` `add`/`set` on props (the owner emits `changed`), `when` conditions
  (`occupied`, `enabled`, prop `equals`/`at_least`/`at_most`; all must hold), a `label` component drawn
  with an owned 5×8 pixel font with `{prop}` placeholders, and `screen_pos` for HUD entities. New:
  `skipped` events, `[[expect]] text`, codes E0206 unknown-prop, E0207 prop-type, W0207 unsupported-char,
  W0305 label-cut.
- `examples/coin_vault` is the reference example: three coins counted on a HUD, a vault that opens at 3
  (any order), an AND lock (statue on one plate, player on the other), a status banner set by `set`. 4
  playtests, two of them negative; a mutation check (door at 2 coins, AND removed) fails exactly the
  playtests that should fail.
- Determinism: native and web agree on coin_vault (state `41f58b0904b83261`, image `f15c777308fdf361`);
  iso_garden's hashes and reference image are unchanged.
- Budgets: wasm 504,174 → 580,108 bytes (+15%, limit 700,000; mostly the static checks, which run in the
  browser too), CLI 1.63 → 1.71 MB, still 16 runtime crates and 0 proc macros, smoke 418 ms, rebuild 458 ms.
- Found while building: screen entities were ordered by part before entity, so every HUD label covered
  every later HUD sprite; fixed (file order). The recipe TOML snippets are now tested.
- Not done: the acceptance run (cold agent #5 on run #3's task: ≤ 5 min, nothing approximated, 0 warnings).

## 2026-11-25 — Q14 decided on KPIs: data logic first, Rust as the escape hatch

- Owner's rule: choose on KPIs only (performance and flexibility trade-off). Measured edit loops: data edit →
  `check` + `test` 0.01 s warm with no build; Rust edit → 0.39–0.49 s dev rebuild, 1.4 s release, 0.9–1.3 s
  browser module (and per-game web builds don't exist yet).
- Rust is more flexible and faster per operation, but loses every agent-loop KPI (static checks,
  queryability, guaranteed determinism, web delivery, authoring time). Its per-operation speed doesn't matter
  for event-driven logic at this scale. So: a small data-level logic set for the common cases, Rust for the
  rest. [ADR 0011](decisions/0011-game-logic-data-first-rust-escape-hatch.md) has the KPI table, the spec and
  the acceptance targets (run #3's task in ≤ 5 min with no approximations).

## 2026-11-25 — Cold-agent test #4: the limits table works

- Run #3's task again, same prompt, with the new limits table. Every verdict came at command 2
  (`slate explain limits`) instead of commands 7–21. 14 min 6 s vs 18 min 42 s, 37 vs 59 tool calls,
  0 warnings vs 40. Re-verified independently.
- It spent the saved time on a better approximation: a 2-bit counter kept in hidden stationary actors read by
  triggers, correct in any pickup order, with 6 thorough playtests.
- Remaining gaps were recipes that were vague or subtly wrong (collectibles could double-count, "a chain of
  toggles" can't count in any order). Fixed with a one-shot collectible recipe, a stationary-actor recipe, an
  any-order counting recipe, documented trigger semantics (enable/disable counts; occupied at frame 0 does not
  fire), value ranges in the reference, the projection formula, and a new W0206 actor-in-solid warning.
- Across four runs, each round removed one class of gap and the next found a subtler one.

## 2026-11-25 — Cold-agent test #3: PASS, honestly, but slow

- Task deliberately beyond the engine: a door that opens only while two plates are both held, plus an
  on-screen score counter. slate has no AND, no text, no counters.
- The agent built a real AND (two stacked solid door leaves, each opened by its own plate, held by patrolling
  sentries) and a seven-segment digit from 17 layered sprites that is correct for every pickup order, and said
  clearly what was approximated and why. Nothing faked. Re-verified independently.
- But it took **18 min 42 s**, 3× run #2: it found every limit by noticing absences in the schema. Fix: a
  "Limits in this version" table (with workarounds and their costs) in AGENTS.md and `slate explain limits`.
- Also: `allow = [...]` to silence intended warnings (its level: 40 → 0), documented draw-order ties and
  shared mover input, `--input` reads files outside the project. Its feature wishes went to the roadmap and
  to a new owner decision, Q14 (game logic in data or Rust?).

## 2026-11-25 — Cold-agent test #2: PASS (harder task)

- Task: tap a lever to open gate 1, slip past a patrolling guard, press a plate for gate 2, reach the goal;
  plus playtests proving the guard blocks bad timing and gate 2 stays shut without the plate.
- PASS in **6 min 7 s** (26 tool calls). Tap coordinates, guard timing and the solution were right on the
  first try. Re-verified independently. Record: [experiments/2026-11-25-cold-agent-2.md](experiments/2026-11-25-cold-agent-2.md).
- Gaps moved from "how do I write this" (run #1) to "how do I prove this precisely": it couldn't assert
  *who* blocked the player or that something *never* happened, and it had to measure patrol timing.
- Fixed: `[[expect]] other` and `at_most`; exact event-frame, patrol, wander and mover-retry semantics;
  `slate explain events`; `run --track NAME` (cell timeline with frame ranges); guaranteed `tap` points in
  `inspect`; runtime state in the text table; the template's smoke test names its scene.

## 2026-11-25 — Cold-agent test #1: PASS

- A fresh agent with only the binary and an empty folder ran `slate new`, read AGENTS.md and built an
  original walled-garden level with a plate, gate, goal and gem, plus a solving playtest and a negative
  one, in **3 min 58 s** (27 tool calls). `check` clean, `test` green, references viewed. Re-verified
  independently. Record: [experiments/2026-11-25-cold-agent-1.md](experiments/2026-11-25-cold-agent-1.md).
- Its first scene was valid on the first try; it hit no check errors and no failed expectations.
- Its 10 confusion points became fixes: exact commands in image diagnostics, a Recipes section (goal,
  doors, collectibles, negative tests, taps), `[[expect]] reason`, subject-only `entity` for event
  expects, precise mover timing, W0304 hidden-trigger + frame-report warnings in `slate test`,
  `--update-refs` lists files, `slate explain` lists topics.
- Lesson: the frame report already knew about the hidden goal tile, but only `shot --report` showed it.
  Information the engine has should reach the agent where it already looks (`slate test`).

## 2026-11-25 — M1 thin working version: native and browser

- The first-milestone chain works end to end: `new → (write scene) → check → run 300 frames with input →
  shot → inspect`, plus `test`, `explain`, `schema`, `docs`, `serve`.
- **Native and browser agree bit for bit**: `examples/iso_garden` walkthrough at frame 300 → state
  `469ae7c92124d43d`, image `bfca8c0b40af6828` in both.
- Browser data reload verified: a typo in a prefab showed `E0103 … did you mean step_frames?` in the page
  within a second; fixing it reloaded the scene. No build step involved.
- Budgets measured (M5 Pro): wasm 467 KB, release CLI 1.5 MB, 16 external runtime crates, 0 proc macros,
  6-command smoke chain 406 ms, slate_sim→cli recompile 0.38 s.
- Finding: cargo fingerprints file contents, so "touch a file" does not measure a rebuild. The budget
  probe now uses an env var read with `option_env!`.
- Finding: the frame report immediately surfaced a real (correct) occlusion — the player standing behind
  the 1.2-high gate is 56% visible. Good sign for the report's usefulness.
- Not done: the cold-agent test (needs a fresh agent); CI has not run on GitHub (no remote yet).

## 2026-11-24 — M0 foundation

- Read the direction doc; chose the codename "slate" (provisional, Q1).
- Pinned Rust 1.100.0 + wasm32; workspace with core/data/sim/render/runtime/cli/web layers.
- Determinism guardrails in clippy.toml; diagnostics with stable codes; schema registry.
