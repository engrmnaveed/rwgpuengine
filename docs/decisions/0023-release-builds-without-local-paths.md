# ADR 0023: Release builds without local paths, and a repository check

- Status: accepted and built, 2026-11-26
- Decided by: the supervisor session's brief for step 4, piece 1; the mechanism by measurement (the KPI rule)

## Context

Two things. First, a debt since step 1: release artifacts held paths of the machine that built them
([the audit, 3.6](../audits/2026-11-25-architecture-audit.md#36-other-facts-found), and three entries of
2026-11-25 in the [progress log](../progress-log.md)). A home path contains a user name, and the owner's rule
is that their name and local paths never leave their machine. Second,
on 2026-11-26 a session committed and pushed a local cross-build folder (`players/target-musl`, 1,608
files) whose files held the home path. Review caught it. Nothing in the gate would have.

## Measurements

Apple M5 Pro, the pinned toolchain (1.100.0), on the step branch with the native player. A "path" is one
place in the artifact's bytes where the home folder starts; the columns split them by Cargo's home and the
repository, which lie inside it here.

| artifact | a plain Cargo build: paths (in Cargo's home / in the repository) | `cargo xtask dist` | bytes, plain → dist |
|---|---|---|---|
| `rwgpu` | 45 (45 / 0) | 0 | 2,031,296 → 2,014,784 |
| web module | 21 (21 / 0) | 0 | 628,075 → 626,627 |
| `gpu_check` | 289 (270 / 19), and 1 more before its `env!` was removed | 0 | 5,571,616 → 5,555,104 |
| `rwgpu-player` | 390 (367 / 23) | 0 | 5,988,512 → 5,955,488 |

Where they came from:

- **Panic locations of rented crates** (`unwrap`, index and overflow messages): Cargo hands rustc each
  registry crate by its full path, `<cargo home>/registry/src/index.crates.io-…/png-0.18.1/src/common.rs`.
- **The agent's crates in the players' workspace** (19): they are path dependencies outside that workspace,
  so Cargo passes their absolute path. Inside the agent's workspace they were already relative
  (`crates/rwgpuengine_sim/src/world.rs`), because Cargo passes members relative to the workspace root.
- **One `env!("CARGO_MANIFEST_DIR")`** in `gpu_check`, which found the repository that way (1). No flag
  rewrites an `env!` value; the check now looks upward from where it runs.
- Nothing else: no debug info (`debug = false`, `strip = true`), no toolchain path, and the standard
  library's paths are `/rustc/<commit>/…` already. On Windows `strip = true` makes rustc pass the linker
  `/DEBUG:NONE`, so no PDB path either; the Windows runner's scan checks that.

What step 1 did: `rwgpu serve` knows the source tree's wasm path only in debug builds. Nothing else.

The two mechanisms, each a clean release build three times (median), measured before the native player
landed; the players' column is its whole workspace:

| | paths left | `rwgpu` | web module | players | web bytes | a rented crate is written | an agent's crate in `players/` |
|---|---|---|---|---|---|---|---|
| nothing | 45 / 21 / 289 | 3.59 s | 3.48 s | 14.30 s | 623,739 | `<cargo home>/registry/src/…/png-0.18.1/src/common.rs` | `<repository>/crates/rwgpuengine_core/src/json.rs` |
| **`--remap-path-prefix` from xtask** | 0 / 0 / 1 | 3.49 s | 3.48 s | 14.38 s | **622,291** | **`png-0.18.1/src/common.rs`** | **`crates/rwgpuengine_core/src/json.rs`** |
| Cargo's `trim-paths = "all"` | 0 / 0 / 1 | 3.53 s | 3.46 s | 14.45 s | 624,317 | `/cargo/registry/25cdd57fae9f0462/png-0.18.1/…` | `/cargo/deps/rwgpuengine_core-0.1.0/src/json.rs` |

The one path left in each is the `env!` above. `trim-paths` is not stable in Cargo 1.100: the manifest is
refused ("feature `trim-paths` is required … not stabilized in this version of Cargo"). It was measured with
`RUSTC_BOOTSTRAP=1`, to compare only; a gate cannot depend on that.

The edit loop does not change. Dev builds get no flags: the budget's rebuild after an edit measures
0.44–0.52 s, as before. A release rebuild after touching `rwgpuengine_sim` takes 1.72–1.76 s remapped and
1.72–1.77 s plain; the players' after touching `rwgpuengine_gpu`, 1.14–1.79 s and 1.10–1.94 s. A plain
`cargo build --release` beside the remapped one rebuilds nothing when switching (0.03 s each way): Cargo
keeps both, because rustflags are part of its file names.

## Decision

1. **`--remap-path-prefix`, passed by xtask** (`xtask/src/dist.rs`). Every release build xtask makes goes
   through one function: the `rwgpu` command, the web module, the players' programs, the render bench of the
   budgets, and `cargo xtask gpu --fast`. It sets `CARGO_ENCODED_RUSTFLAGS` to the environment's rustflags
   plus one flag per folder:

   | folder | becomes | so a location reads |
   |---|---|---|
   | the repository | nothing | `crates/rwgpuengine_sim/src/world.rs:120:5` |
   | each folder in `<cargo home>/registry/src/` | nothing | `png-0.18.1/src/common.rs:42:9` |
   | `<cargo home>/git/checkouts/` | nothing | `<repository>-<hash>/<commit>/src/lib.rs` |
   | the home folder (anything else in it) | `~` | `~/…`, with no user name |

   rustc applies the last flag that matches, so the longest folder goes last and the most specific rule
   wins. The flags depend only on where things are on this machine, so Cargo never rebuilds for them.
   It beat `trim-paths` on every count but build time, where they tie: it is stable on the pinned
   toolchain, it writes shorter names (1,448 bytes off the web module, `trim-paths` adds 578), and it names
   the agent's crates by their place in the repository.
2. **`cargo xtask dist`** is the one way to build release artifacts, for the gates and for people. It builds
   `rwgpu`, the web module and the players' programs (`--agent` leaves those out) and checks each.
   `cargo xtask web` and the budgets use the same code.
3. **The artifact check.** After each release build, xtask reads the artifact's bytes for the home folder
   (`HOME`, and `USERPROFILE`), the repository and Cargo's home: as written, with `/` for `\`, and with
   `\\` doubled; ignoring ASCII case; in UTF-8 and in UTF-16. A hit fails, and names the artifact and the
   first five strings found, with the folder shown as `<home>`, `<repository>` or `<cargo home>`.
   `cargo xtask ci` checks `rwgpu` and the web module, `cargo xtask gpu` the players' programs (a list:
   `gpu_check` and `rwgpu-player`).
4. **The repository check**, `cargo xtask repo-check`, the first step of `cargo xtask ci` (0.7 s with
   Cargo's start-up). It reads the files git tracks and fails on:
   - an absolute home path of any user: `/home/<name>`, the same under `Users` (macOS), and
     `<drive>:\Users\<name>` with `\`, `/` or `\\`. Not inside a web address (`https://docs.rs/home/…`), and
     not a placeholder (`<name>`, `$USER`, or the one made-up name on the exception list, `someone`, which
     examples use). The docs write no macOS home path even with `<name>`, so that a plain search of
     outgoing commits for that folder finds nothing;
   - build output: a `target` or `target-*` folder anywhere, a `.dSYM` folder, compiler and linker output by
     name (`.d`, `.rlib`, `.rmeta`, `.o`, `.obj`, `.a`, `.lib`, `.so`, `.dylib`, `.dll`, `.exe`, `.pdb`,
     `.ilk`, `.exp`, `.wasm`, `.profraw`), or a program by its first bytes (ELF, Mach-O, PE, wasm, `ar`);
   - a file over `[repo] file_bytes` (150,000). The largest tracked file is
     `crates/rwgpuengine_data/src/load.rs` at 75,685 bytes; the limit is twice that, rounded. No kind of
     file needs more today.

   Untracked files that `.gitignore` does not cover are checked for build output too: the folder of
   2026-11-26 would have failed the gate before anyone staged it. Each problem names the file, the line (or
   the byte, in a binary file) and what to do.

## Consequences and limits

- A plain `cargo build --release` or `cargo install` still writes paths. The documented way to install the
  player, `cargo install --locked --path players/rwgpuengine_player` (the README and the hint of E0006), is
  such a build: 390 paths. It stays the way for one's own machine, where those paths are the owner's own;
  the README says that a program for someone else comes from `cargo xtask dist`.
- xtask's release builds do not use rustflags from Cargo's configuration files: `CARGO_ENCODED_RUSTFLAGS`
  replaces them. `RUSTFLAGS` from the environment is kept.
- The players' own crates stay relative to their workspace (`rwgpuengine_gpu/src/pack.rs`); rustc writes some
  as `players/gpu_check/src/main.rs`. Both name the file.
- The artifact check knows this machine's folders, not another's. The repository check knows any user's
  home path, but reads the working tree of the tracked files: a staged version that differs, the history,
  and the inside of compressed files are not read. Checking every outgoing commit before a push stays a
  step of its own ([conventions](../conventions.md#privacy)).
- Tested without those systems: the Windows forms by unit tests with planted paths (each form, case,
  UTF-16), and by the Windows runner in CI. Linux in a container (arm64 Alpine, home `/home/someone`,
  Cargo's home `/usr/local/cargo` outside it): the xtask tests pass; a plain build holds 44 paths under
  Cargo's home, the remapped one 0, and the smoke chain ends in the Mac's state `94fd42c60d7a4c92`.
- Not done: byte-for-byte reproducible builds, installers, packaging, signing, git hooks.

## When to revisit

- When Cargo's `trim-paths` is stable on the pinned toolchain: measure it again.
- When a check finds a path the remapping does not cover, or a tracked file that needs more than the limit.
