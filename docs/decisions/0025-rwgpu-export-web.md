# ADR 0025: `rwgpu export web` — a project as a folder that any static web host can serve

- Status: accepted and built, 2026-11-26 (step 4 of the [3D proposal](../proposals/3d-architecture.md), piece 3
  of 3). First built with the player that paints with the CPU; the WebGPU player of piece 2
  ([ADR 0024](0024-web-player-on-webgpu.md)) went in when it landed, on 2026-11-27.
- Decided by: publishing, the owner, on 2026-11-26, answering "Publish the town demo with GitHub Pages from
  this repository when step 4 is done?" with "Yes, GitHub Pages". The command, the supervisor session's brief
  for step 4 ("done when: a link that opens the town"); the shape of the command and the folder were chosen
  here.
- Builds on: [ADR 0008](0008-web-via-c-abi.md) (the CPU module and the bundle),
  [ADR 0022](0022-native-player-winit-and-rwgpu-play.md) (how `rwgpu` finds a player program),
  [ADR 0023](0023-release-builds-without-local-paths.md) (no path of the machine in what is shipped) and
  [ADR 0024](0024-web-player-on-webgpu.md) (the WebGPU player and its browser check).

## Context

`rwgpu serve` plays a project in a browser, from this machine: it makes the page, finds the player and makes
the bundle on every request. Nothing could put a game on a web host. A link that opens the town needs a folder
that a host can serve as it is: no server code, no build step there, and nothing of the machine that made it.

## Decision

1. **The command.** `rwgpu export web [PATH] --out DIR [--force] [--web-player DIR | --wasm FILE] [--json]`.
   One command, `export`, with the target as its first argument, so a later target is a new value and not a
   new command; `web` is the only one. `--out` is needed: with a default folder, every second export would
   need `--force`.
2. **The folder.** The files `rwgpu serve` serves, under the same names, written once. The player's files
   depend on which player is found (point 5); the game's are the same for both:

   | file | part | what | comes from |
   |---|---|---|---|
   | `index.html`, `player.js`, `rwgpu_web.js`, `rwgpu_web_bg.wasm` | player | the WebGPU player: the page, its script, wasm-bindgen's glue, the module (WebGPU, with the CPU reference renderer as its fallback) | copied as they are from the player's folder |
   | or `index.html`, `player.js`, `player.wasm` | player | the CPU player: the page, its script, the module (`rwgpuengine_web`) | the page and script compiled into `rwgpu` from `web/`; the module found |
   | `project.json` | game | the game's name, main scene, scenes and playtests, for the page's lists | made from the project |
   | `bundle.txt` | game | the game's files as one text bundle (`Vfs::to_bundle`) | made from the project |

   Every address either page, its script and the glue use is relative (`project.json`, not `/project`), so
   the folder works at the root of a site and under any path. `serve` answers the same names, plus
   `bundle-hash.txt`, and only `serve` sets `"reload": true` in `project.json`: only then does a page poll for
   changes. Before, both pages asked for `bundle`, `project` and `bundle-hash` (the CPU page with a leading
   slash), and polled always.
3. **One lookup, one list.** `crates/rwgpuengine_cli/src/web.rs` says which files each player has
   (`WEB_PLAYER_FILES`, `CPU_PLAYER_FILES`), where each comes from (`Player::files`), how the player is found
   (`find_player`) and what the game's files are (`game`). `serve` and `export web` both read it; xtask's
   build of the WebGPU player and its checks use the same list.
4. **What goes in the bundle**: the files the engine reads, and the same set for `serve`.

   | file | why |
   |---|---|
   | `rwgpu.toml` | the project's settings: frame size, tick rate, seed, main scene |
   | `scenes/**.toml` | the scenes: the page lists them |
   | `prefabs/**.toml` | scenes and spawns build entities from them |
   | `tests/**.toml` | the playtests: the page lists them as input, and `?input=tests/x.toml&frames=N` replays one |
   | the main scene, and any scene a playtest names, outside `scenes/` | the engine loads them |

   Everything else stays behind, and `--json` lists each left-out file and why: reference pictures
   (`tests/refs/*.png`, only `rwgpu test` reads them), notes (`*.md`), other `.toml` files, anything outside the
   project folder. Hidden folders, `target/`, `out/` and the export folder are not walked. The playtests go in
   because they are a few kilobytes of text, already public in the repository, and they make a published link
   that replays a playtest possible.
5. **Where the player comes from.** One lookup for `serve` and `export web`. A flag names the one place, and
   a flag beats a variable: `--web-player DIR`, `--wasm FILE`, then `RWGPU_WEB_PLAYER`, `RWGPU_WEB_WASM`. With
   none, a WebGPU player anywhere beats a CPU one: `web-player` next to the `rwgpu` program (links followed,
   as `rwgpu play` finds its player), the workspace's `target/web-player` (debug builds only), then
   `rwgpuengine_web.wasm` in the same two places. A WebGPU player is a folder that holds all four files.
   `cargo xtask dist` puts both players next to `rwgpu` in `target/release/` (`--agent`: the CPU player only,
   which needs no wasm-bindgen), so a built `rwgpu` exports with no flag and no source tree. None found, or a
   named place without a player, is **E0008** `web-player-not-found`, whose hint names `cargo xtask dist`.
   `--json` says which player (`player`, `player_from`); the text output says it in one line. Both flags at
   once is a usage error. Embedding a player in `rwgpu` stays with Q11: the WebGPU player is built in the
   players' half, which `rwgpu` may not link, and the CPU module would add 629 KB to a 2.1 MB program.
6. **Validated first.** A project that does not pass `rwgpu check` is not exported; the diagnostics are the
   checker's own. Warnings are passed on.
7. **Safe to run.** The folder must be new or empty, unless `--force` (E0005). With `--force` the export's
   own files are replaced and nothing else is changed; W0001 names the other files, because a host publishes
   them too. Nothing is ever deleted. Only the export's own names are written, and never through a link or
   over a folder that has one of those names (E0005). `--out` may not be the project folder or a folder that
   holds it (E0003): a host would publish the whole project.
8. **Deterministic.** The same project and player give the same bytes in every file: no time, paths sorted,
   `project.json` written with ordered keys, the player's files copied as they are. No path of this machine:
   the bundle holds project-relative paths and the files' text only, and the players are release builds that
   carry none (ADR 0023).
9. **`rwgpu serve --site DIR`** serves a folder as it is, as a static host would (no `..`, no link out of the
   folder, a folder's `index.html`), so an exported folder can be tried with `rwgpu` alone. `--json` prints
   that line as `try`.

## What a host needs

- **Any static file server.** No server code, no rewrite rules, no headers beyond the usual.
- **`application/wasm` for `.wasm`** lets the browser compile the module while it downloads. A host that
  sends another type still works: the CPU player's script compiles the module from its bytes instead, and
  wasm-bindgen's glue does the same for the WebGPU player. The gate proves it for the CPU player with
  `application/octet-stream`.
- **A web server, not a file.** Opened from disk (`file://`), browsers refuse `fetch` and module scripts, so
  the page cannot load the player. Not tested here; it is how browsers behave.
- **HTTPS for WebGPU.** Browsers offer WebGPU only in a secure context (HTTPS, or `localhost`). Elsewhere the
  WebGPU player draws with the CPU reference renderer and says so. GitHub Pages is HTTPS.

## Evidence

Apple M5 Pro, Chrome 158, the pinned toolchain, release `rwgpu` and players built by xtask (ADR 0023).

- **The CPU player's folder, in `cargo xtask ci`** (no GPU, no browser). `cargo xtask export-check` exports
  each example with a copy of the release `rwgpu` that has only the CPU module next to it, and again with
  `--wasm`: the same files byte for byte. `web/export-check.mjs` serves the folder from a plain static host
  (`web/static-host.mjs`) under Node and opens the folder's own `index.html` and `player.js` in a small
  stand-in for a browser: elements by id, `fetch` limited to the folder, a canvas that records what is
  painted. It opens the page as linked (the main scene at frame 0), every playtest's last frame
  (`?input=tests/x.toml&frames=N`) and every `[[shot]]` frame. For all three examples, at `/` and under
  `/games/<name>/`, the frame, the state hash and the hash of the painted pixels are those of `rwgpu test` and
  `rwgpu shot`: the ten playtests keep their state and image hashes (`82c150f3d192812e` / `d8d261889fa64ce1`
  for `build_town`, `5775c67554808f1a` / `2338f77ae0eea945` for `turn_and_build`, and so on). Every request
  stays inside the folder and is answered 200; every file is asked for; the page polls nothing. Once more per
  example the module is sent as `application/octet-stream`, and the page still loads. A copy of the folder
  with one root-absolute address, one without the fallback for the module's type, and one with a damaged
  bundle each fail it (tried by hand).
- **The WebGPU player's folder, in `cargo xtask gpu`.** Every example is exported by `rwgpu` with the
  workspace's WebGPU player (the JSON must say `webgpu`, from `target/web-player`), and again naming it with
  `--web-player`: the same bytes; every file is read for this machine's paths. Then piece 2's browser check
  (`players/rwgpuengine_player/web/check.mjs --export`) serves each folder from the same plain static host, at
  `/` and under `/rwgpuengine/`, and opens it in a headless Chrome on both paths. Every playtest reaches
  `rwgpu test`'s state at its frame on WebGPU and on the CPU path (`?renderer=cpu`); each frame WebGPU drew is
  held to the reference by `gpu_check --web-frames`, the rule of ADR 0021 (0 pixels apart here, on Metal);
  the CPU path's canvas holds the reference's exact pixels (`rwgpu shot`'s image hash). Every request is
  answered from the folder and every file of it is asked for: 14 requests per example. On the Linux runner
  the browser loses the WebGPU device of a canvas (ADR 0024, "The limit on the Linux runner"); there the
  same check holds every playtest to being handed to the CPU at the engine's state, as for `rwgpu serve`.
- **The addresses.** A test in xtask holds the WebGPU page and script to relative addresses and to the game
  files' names; the build of the player fails if the glue ever loads its module by any address but
  `new URL('rwgpu_web_bg.wasm', import.meta.url)`. The browser check under `/rwgpuengine/` is the proof: a
  request outside the folder or one not answered fails it. Both pages carry `<link rel="icon" href="data:,">`,
  so a browser does not ask the host for `/favicon.ico`, outside the folder.
- **The folder that is published.** `cargo xtask pages` builds through `cargo xtask dist`, exports Tiny Town
  with the release `rwgpu` and the WebGPU player next to it, exports it again naming the player (the same
  bytes), reads every file for this machine's paths, and runs the browser check on that very folder under
  `/rwgpuengine/` and at `/`: four playtests on WebGPU (0 pixels apart) and on the CPU path, 14 requests, all
  answered. The whole command took 13.3 s here, the warm `cargo xtask dist` included.
- **Unit tests**: two exports of Tiny Town equal and free of the project's and home paths; a fixed project's
  bundle hashes to `fd6cd2455a1b4482` on every system; the refusals (not empty, `--force` beside other files,
  the project folder, a link, a project that fails check, no player, both flags, bad targets); the WebGPU
  player exported as its folder holds it, and a folder missing a file refused (E0008); the lookup's order; each
  player's files from `serve`, and the game's.
- **Time in the gates.** `cargo xtask ci`, warm: 15.05 s before this piece, 16.0 to 16.3 s after; its export check
  takes 1.5 s for the three examples. In `cargo xtask gpu` the exports and the two browser runs over
  the exported folders add 5.0 s (the whole gate: 37.5 s warm).

| game | WebGPU player | game | folder | gzip -9: player / game / folder |
|---|---|---|---|---|
| `iso_garden` | 771,924 | 4,688 | 776,612 | 299,098 / 1,866 / 300,964 |
| `coin_vault` | 771,924 | 9,457 | 781,381 | 299,098 / 2,652 / 301,750 |
| `tiny_town` | 771,924 | 16,701 | 788,625 | 299,098 / 4,381 / 303,479 |

| game | CPU player | game | folder | gzip -9: player / game / folder |
|---|---|---|---|---|
| `iso_garden` | 645,829 | 4,688 | 650,517 | 240,698 / 1,866 / 242,564 |
| `coin_vault` | 645,829 | 9,457 | 655,286 | 240,698 / 2,652 / 243,350 |
| `tiny_town` | 645,829 | 16,701 | 662,530 | 240,698 / 4,381 / 245,079 |

Bytes. The WebGPU player is the page (4,838), the script (15,086), the glue (79,045) and the module (672,955),
within its download budget of 1,500,000 (ADR 0024; 770,648 when it landed: the page and script grew by the
names and the reload switch, the module shrank by 103 bytes). The CPU player is the page (4,230), the script (9,068) and the module (632,531, limit 800,000);
the module is 5,904 bytes larger than before this piece (626,627) because it carries the explanations of
E0008 and W0001, as it carries every code's. The game part of Tiny Town is its
bundle (16,494) and its lists (207). The `rwgpu` command is 2,133,296 bytes (limit 3,000,000), against 2,014,784 on the commit before
pieces 2 and 3. Compressing or minifying further is not done: the game is
2.1 % of Tiny Town's folder (1.4 % gzipped), and the modules are already built for size (ADR 0017).

## Hosting: GitHub Pages from this repository

The owner decided it (above). The town is published from the repository's own Pages site by a workflow,
`.github/workflows/pages.yml`, once step 4 is merged into `main`. The supervisor session switches Pages on and
runs the workflow; no other session runs it or changes the repository's settings.

- **Manual only.** The workflow's one trigger is `workflow_dispatch`: merging it publishes nothing, and it runs
  only when someone presses "Run workflow" in the Actions tab. Only a run from `main` reaches the deploy job.
- **What it does.** On `macos-latest`, where the runner's Chrome draws WebGPU on a canvas (ADR 0024), it runs
  `cargo xtask pages`: everything built through `cargo xtask dist` (no path of the runner; the first run
  installs the pinned wasm-bindgen tool), Tiny Town exported into `target/pages` with the WebGPU player, and
  that folder proved before anything is published, as above, under `/<repository>/` (the path Pages serves it
  at) and at `/`. Only `target/pages` is uploaded (`actions/upload-pages-artifact@v5`, which archives with
  GNU tar on macOS) and deployed (`actions/deploy-pages@v5`).
- **Least permissions.** None at the top. The build job reads the repository (`contents: read`) and keeps no
  token in the checkout. The deploy job has only what Pages needs: `pages: write` and `id-token: write`, in the
  `github-pages` environment. Actions are pinned to their major versions, as in `ci.yml`.
- **What is switched on once:** Settings → Pages → Build and deployment → Source: "GitHub Actions".
- **The address:** `https://engrmnaveed.github.io/rwgpuengine/`. Pages sends `/rwgpuengine` to
  `/rwgpuengine/`, and every address in the folder is relative to that.
- **What becomes public:** the six files of the Tiny Town folder. The bundle holds Tiny Town's TOML files,
  which the repository already shows; the player is the engine, built from the public source. No path, name or
  email of a machine is in them (proved before upload). GitHub records who ran the workflow, as for any run.
- **Limit:** Pages caches files for about 10 minutes, so for a while after a new run a visitor can get the new
  page with the old module.

Alternatives, not taken:

- Pages from a branch: needs the module committed, which the repository check forbids (ADR 0023).
- A release asset: no link that opens the game; people download and serve it themselves.
- Cloudflare Pages or Netlify: an account and a token outside GitHub.
- itch.io: an account, a person uploads each version, and the game runs inside its frame.
- Building on `ubuntu-latest`: the runner's Chrome loses the WebGPU device of a canvas, so the published
  folder could not be proved on WebGPU there.

## Consequences and limits

- One folder is one game. A page with several games is several folders.
- Caching: a host that caches can give a visitor the new page with the old module for a while after a new
  export is published. File names carry no hash yet.
- `rwgpu` does not check that a player was built from its own version; `cargo xtask dist` builds them from
  one tree, and a player is checked only by playing it.
- A project with CRLF line endings gives a different bundle than the same project with LF: they are
  different files. The repository pins LF (`.gitattributes`).
- `serve` changed with the shared lookup: a missing player is E0008 (it was E0001); `RWGPU_WEB_PLAYER` and
  `RWGPU_WEB_WASM`, when set, are the only place looked at, as their flags are (before, the first was one
  place among several); `--wasm` now serves the CPU player even when a WebGPU player is found (before, it was
  then ignored); `--web-player` with `--wasm` is a usage error. Both pages ask for the new names, and poll
  only under `serve`.
- `cargo xtask dist --agent` after `cargo xtask dist` leaves the WebGPU player next to the release `rwgpu`,
  which then still finds it.
- Not done: pages that work offline or install, compression and minifying, hashed file names, other targets.
  The page is published only after step 4 is merged.

## Revisit when

A game needs more than one folder can give (several games on one page, files a host must rewrite), a host
needs headers the folder cannot set, caching causes a real mismatch, the WebGPU player's files change (the
lists in `web.rs`), or the owner decides Q11 (embedding a player in `rwgpu`).
