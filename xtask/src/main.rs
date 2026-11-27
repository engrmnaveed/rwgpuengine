//! Dev automation for the rwgpuengine workspace. Run through the cargo alias:
//!
//! ```text
//! cargo xtask ci        everything CI runs, in order (fails fast)
//! cargo xtask repo-check  what must never be committed: home paths, build
//!                       output, oversized files (the first step of `ci`)
//! cargo xtask dist      the release artifacts, with no path of this machine
//!                       (`--agent`: only the rwgpu command and the web module)
//! cargo xtask web       build rwgpuengine_web.wasm (web profile): the CPU player
//! cargo xtask web-player  build the WebGPU web player into target/web-player
//!                       (the players' half; installs the pinned wasm-bindgen
//!                       tool into target/tools the first time)
//! cargo xtask smoke     the first-milestone command chain on a fresh project
//! cargo xtask budgets   measure and enforce budgets.toml
//! cargo xtask docs      regenerate docs/reference (`--check` to verify only)
//! cargo xtask wasm-check  the wasm module against the native command (needs Node)
//! cargo xtask export-check  `rwgpu export web` for every example, played from a
//!                       static server (needs Node)
//! cargo xtask pages     the folder the GitHub Pages workflow publishes: Tiny
//!                       Town exported and proved under /rwgpuengine/ (needs Node)
//! cargo xtask gpu       the players' half: the wgpu renderer against the CPU
//!                       reference, the native player against `rwgpu test`,
//!                       the web player in a headless browser, and the
//!                       players' budgets (needs a GPU or a software adapter;
//!                       not part of `ci`). `--window` also plays in a real
//!                       window.
//! ```
//!
//! `render-bench` is internal: `budgets` runs it as a release build.

// xtask measures wall-clock time on purpose (budgets); the determinism ban
// on Instant::now applies to the engine, not to dev tooling.
#![allow(clippy::disallowed_methods)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

mod dist;
mod repo;
mod town;
mod web;

use rwgpuengine_core::diag::SourceFile;
use rwgpuengine_core::json::Json;
use rwgpuengine_data::node::{Table, parse_toml};

type Result<T = ()> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let task = args.first().map(String::as_str).unwrap_or("help");
    let rest = &args[args.len().min(1)..];
    let result = match task {
        "ci" => ci(),
        "repo-check" => repo_check(),
        "dist" => dist_all(rest),
        "web" => web().map(|_| ()),
        "web-player" => dist::Machine::here(&root()).and_then(|m| web::build(&m)).map(|_| ()),
        "smoke" => smoke().map(|_| ()),
        "budgets" => budgets(),
        "docs" => docs(rest.iter().any(|a| a == "--check")),
        "wasm-check" => wasm_check(),
        "export-check" => export_check(),
        "pages" => pages(rest),
        "render-bench" => render_bench(),
        "gpu" => gpu(rest),
        _ => {
            eprintln!(
                "usage: cargo xtask <ci | repo-check | dist [--agent] | web | web-player | smoke | budgets | \
                 docs [--check] | wasm-check | export-check | pages [--out DIR] [--base /PATH/] | \
                 gpu [--fast | --study | --window] [-- <options of players/gpu_check>]>"
            );
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask {task}: FAILED: {e}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace root").to_path_buf()
}

fn step(name: &str) {
    eprintln!("\n=== {name}");
}

/// Run a command in the workspace root; error if it fails.
fn run(program: &str, args: &[&str]) -> Result {
    eprintln!("$ {program} {}", args.join(" "));
    let status =
        Command::new(program).args(args).current_dir(root()).status().map_err(|e| format!("{program}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("`{program} {}` failed ({status})", args.join(" "))) }
}

/// Run a command in `dir`; error if it fails.
fn run_in(dir: &Path, program: &str, args: &[&str]) -> Result {
    eprintln!("$ {program} {}", args.join(" "));
    let status = Command::new(program).args(args).current_dir(dir).status().map_err(|e| format!("{program}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("`{program} {}` failed ({status})", args.join(" "))) }
}

/// Run a command and capture stdout.
fn capture(program: &str, args: &[&str], dir: &Path) -> Result<(bool, String)> {
    let out = Command::new(program).args(args).current_dir(dir).output().map_err(|e| format!("{program}: {e}"))?;
    Ok((out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned()))
}

fn ci() -> Result {
    repo_check()?;
    step("format");
    run("cargo", &["fmt", "--all", "--check"])?;
    step("clippy (native)");
    run("cargo", &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])?;
    step("clippy (wasm)");
    run("cargo", &["clippy", "-p", "rwgpuengine_web", "--target", "wasm32-unknown-unknown", "--", "-D", "warnings"])?;
    step("tests");
    run("cargo", &["test", "--workspace"])?;
    step("generated docs are current");
    docs(true)?;
    step("example playtests");
    for example in EXAMPLES {
        run("cargo", &["run", "--quiet", "--bin", "rwgpu", "--", "check", example])?;
        run("cargo", &["run", "--quiet", "--bin", "rwgpu", "--", "test", example])?;
    }
    step("budgets (builds web + release, runs smoke)");
    budgets()?;
    wasm_check()?;
    export_check()?;
    eprintln!("\nci: all green");
    Ok(())
}

const EXAMPLES: [&str; 3] = ["examples/iso_garden", "examples/coin_vault", "examples/tiny_town"];

/// Native equals web: for every example playtest, the wasm module (run under
/// Node by `web/check.mjs`) must reach the native command's state hash and
/// paint pixels that hash to its `shot` image. Without Node the check is
/// skipped on a developer's machine and fails in CI, which always has it.
fn wasm_check() -> Result {
    step("native equals web (the wasm module under Node)");
    if !node("the wasm check")? {
        return Ok(());
    }
    let wasm = web()?.to_string_lossy().into_owned();
    let cli = release_cli()?.to_string_lossy().into_owned();
    for example in EXAMPLES {
        run("node", &["web/check.mjs", &wasm, example, "--native", &cli])?;
    }
    Ok(())
}

/// Whether Node is here. Without it a check is skipped on a developer's
/// machine and fails in CI, which always has it.
fn node(check: &str) -> Result<bool> {
    if Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success()) {
        return Ok(true);
    }
    if std::env::var_os("CI").is_some() {
        return Err(format!("`node` was not found; CI must run {check}"));
    }
    eprintln!("skipped: `node` was not found. Install Node.js to run {check}; CI always runs it.");
    Ok(false)
}

/// `rwgpu export web` gives a folder that plays the game from any static
/// server (ADR 0025): every example, with the CPU player (no GPU and no
/// browser here), proved by [`prove_export`] at the site's root and under a
/// path. The WebGPU player's folders are proved in a browser by `cargo xtask
/// gpu` ([`export_folders`]).
fn export_check() -> Result {
    step("export web: the exported folder (the CPU player) plays the game from a static server");
    if !node("the export check")? {
        return Ok(());
    }
    let dir = root().join("target/export-check");
    let _ = std::fs::remove_dir_all(&dir);
    let built = Built::here()?;
    let start = Instant::now();
    for example in EXAMPLES {
        let name = Path::new(example).file_name().and_then(|n| n.to_str()).ok_or("example name")?;
        prove_export(&built, example, &dir.join(name), &["/", &format!("/games/{name}/")])?;
    }
    eprintln!("export check: {} ms for {} examples", start.elapsed().as_millis(), EXAMPLES.len());
    Ok(())
}

/// The game the Pages workflow publishes, and the path GitHub Pages serves a
/// project site under (`https://<owner>.github.io/<repository>/`).
const PAGES_GAME: &str = "examples/tiny_town";
const PAGES_BASE: &str = "/rwgpuengine/";

/// `cargo xtask pages [--out DIR] [--base /PATH/]`: the folder the Pages
/// workflow publishes (ADR 0025). Everything built through `cargo xtask dist`,
/// then Tiny Town exported by the release `rwgpu` with the WebGPU player next
/// to it, and proved on that very folder: a second export naming the player
/// gives the same bytes, no file holds a path of this machine, and in a
/// headless browser, from a plain static host under the path Pages serves it
/// at and then at a site's root, every playtest reaches the engine's state on
/// WebGPU and on the CPU path, with the picture rule where the browser draws
/// WebGPU. Node and a browser are needed here even off CI: nothing unproved is
/// published.
fn pages(args: &[String]) -> Result {
    let mut out = root().join("target/pages");
    let mut base = PAGES_BASE.to_string();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--out", Some(v)) => out = root().join(v),
            ("--base", Some(v)) if v.starts_with('/') && v.ends_with('/') => base = v.clone(),
            _ => return Err(format!("pages takes --out DIR and --base /PATH/ (got `{a}`)")),
        }
    }
    if Command::new("node").arg("--version").output().is_err() {
        return Err("`node` was not found: the folder is proved in a browser before it is published".into());
    }
    step("pages: everything built through `cargo xtask dist`");
    dist_all(&[])?;
    step("pages: the folder GitHub Pages publishes, exported and proved");
    let release = root().join("target/release");
    let cli = release.join(format!("rwgpu{}", std::env::consts::EXE_SUFFIX));
    let beside = release.join(rwgpuengine_cli::web::WEB_PLAYER_DIR);
    // A folder of an earlier run of this command is ours to replace; any other
    // folder must be new or empty (`rwgpu export web` refuses otherwise).
    if out == root().join("target/pages") {
        let _ = std::fs::remove_dir_all(&out);
    }
    let json = export_web(&cli, PAGES_GAME, &out, None)?;
    expect_player(&json, "webgpu", &beside)?;
    let named = root().join("target/pages-named");
    let _ = std::fs::remove_dir_all(&named);
    export_web(&cli, PAGES_GAME, &named, Some(("--web-player", &web::out_dir())))?;
    let files = same_folders(&out, &named)?;
    dist::check_artifacts(&dist::Machine::here(&root())?, &root(), &files)?;
    let expected = web::expected(&cli, &[PAGES_GAME])?;
    let gpu_check =
        root().join("players/target-release/release").join(format!("gpu_check{}", std::env::consts::EXE_SUFFIX));
    let pixels_apart = Budgets::load()?.limit("players", "pixels_apart");
    let mut summary = Vec::new();
    for at in [base.as_str(), "/"] {
        let served = web::Served::Export { folders: &out, base: at };
        let m = web::check(&beside, &cli, &gpu_check, &expected, pixels_apart, &served)?;
        if !m.ran {
            return Err("no Chromium browser was found (set RWGPU_BROWSER): the folder is proved in a browser before \
                        it is published"
                .into());
        }
        summary.extend(m.summary);
    }
    eprintln!("\nTiny Town's folder in a browser:");
    for line in &summary {
        eprintln!("  {line}");
    }
    eprintln!("\npages: {} is proved under {base} and at /, ready to publish as it is", display(&out));
    Ok(())
}

/// The proof of one exported folder (ADR 0025). `example` is exported into
/// `out` by the release `rwgpu`, which takes the module next to itself as an
/// installed one would; exported again with `--wasm` into another folder: the
/// same bytes; every file of `out` read for paths of this machine (ADR 0023);
/// then `out` itself is played by `web/export-check.mjs` from a plain static
/// server under each of `bases`, and opened once more under the first with the
/// module sent as `application/octet-stream`.
fn prove_export(built: &Built, example: &str, out: &Path, bases: &[&str]) -> Result {
    let Built { wasm, cli, beside, machine } = built;
    let name = Path::new(example).file_name().and_then(|n| n.to_str()).ok_or("example name")?;
    let scratch = root().join("target/export-check");
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let again = scratch.join(format!("{name}-with-wasm-flag"));
    let _ = std::fs::remove_dir_all(&again);
    let json = export_web(cli, example, out, None)?;
    // An installed rwgpu has no source tree: it took the module next to itself.
    expect_player(&json, "cpu", beside)?;
    export_web(cli, example, &again, Some(("--wasm", wasm)))?;
    let files = same_folders(out, &again)?;
    dist::check_artifacts(machine, &root(), &files)?;
    let manifest = scratch.join(format!("{name}.json"));
    std::fs::write(&manifest, &json).map_err(|e| e.to_string())?;
    let (out_s, cli_s, manifest_s) = (out.to_string_lossy(), cli.to_string_lossy(), manifest.to_string_lossy());
    let check = ["web/export-check.mjs", &out_s, example, "--native", &cli_s];
    for (i, base) in bases.iter().enumerate() {
        let sizes: &[&str] = if i == 0 { &["--export-json", &manifest_s] } else { &[] };
        run("node", &[&check[..], &["--base", base], sizes].concat())?;
    }
    let first = bases.first().ok_or("no base path")?;
    run("node", &[&check[..], &["--base", first, "--wasm-type", "application/octet-stream", "--open-only"]].concat())
}

/// The release `rwgpu` and web module, built as `cargo xtask dist` builds
/// them, and a copy of that `rwgpu` in a folder of its own with only the CPU
/// module next to it: an installed `cargo xtask dist --agent`. Whatever else
/// target/release holds (the WebGPU player, after `cargo xtask dist`) is not
/// seen, so the CPU player is what the lookup finds.
struct Built {
    wasm: PathBuf,
    cli: PathBuf,
    beside: PathBuf,
    machine: dist::Machine,
}

impl Built {
    fn here() -> Result<Built> {
        let wasm = web()?;
        let release = release_cli()?;
        let bin = root().join("target/export-check/bin");
        let _ = std::fs::remove_dir_all(&bin);
        std::fs::create_dir_all(&bin).map_err(|e| format!("{}: {e}", bin.display()))?;
        let cli = bin.join(release.file_name().ok_or("rwgpu")?);
        std::fs::copy(&release, &cli).map_err(|e| format!("{}: {e}", cli.display()))?;
        let beside = module_beside(&wasm, &cli)?;
        Ok(Built { wasm, cli, beside, machine: dist::Machine::here(&root())? })
    }
}

/// A path for messages: relative to the repository when it is inside it.
fn display(p: &Path) -> String {
    p.strip_prefix(root()).unwrap_or(p).display().to_string()
}

/// The module next to the release `rwgpu`, as `cargo xtask dist` leaves it and
/// as `rwgpu serve` and `rwgpu export web` look for it.
fn module_beside(wasm: &Path, cli: &Path) -> Result<PathBuf> {
    let to = cli.with_file_name("rwgpuengine_web.wasm");
    std::fs::copy(wasm, &to).map_err(|e| format!("{}: {e}", to.display()))?;
    Ok(to)
}

/// `rwgpu export web` into `out`, with the player found by the lookup (none
/// named, and the variables removed) or named by a flag (`--wasm FILE`,
/// `--web-player DIR`). Returns its JSON.
fn export_web(cli: &Path, example: &str, out: &Path, player: Option<(&str, &Path)>) -> Result<String> {
    let out_s = out.to_string_lossy().into_owned();
    let mut args = vec!["export".to_string(), "web".into(), example.into(), "--out".into(), out_s, "--json".into()];
    if let Some((flag, place)) = player {
        args.extend([flag.to_string(), place.to_string_lossy().into_owned()]);
    }
    eprintln!("$ rwgpu {}", args.join(" "));
    let o = Command::new(cli)
        .args(&args)
        .current_dir(root())
        .env_remove("RWGPU_WEB_WASM")
        .env_remove("RWGPU_WEB_PLAYER")
        .output()
        .map_err(|e| format!("rwgpu: {e}"))?;
    let json = String::from_utf8_lossy(&o.stdout).into_owned();
    if !o.status.success() {
        return Err(format!("rwgpu export web {example} failed: {json}"));
    }
    Ok(json)
}

/// `rwgpu export web --json` took this player from this place.
fn expect_player(json: &str, kind: &str, place: &Path) -> Result {
    let j = Json::parse(json).map_err(|e| format!("rwgpu export web: bad JSON ({e})"))?;
    let got = j.get("player").and_then(Json::as_str).unwrap_or("");
    let from = j.get("player_from").and_then(Json::as_str).unwrap_or("");
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if got != kind || canon(&root().join(from)) != canon(place) {
        return Err(format!(
            "rwgpu export web took the {got} player from {from}, not the {kind} player in {}",
            display(place)
        ));
    }
    Ok(())
}

/// The two folders hold the same files with the same bytes. Returns the first's files.
fn same_folders(a: &Path, b: &Path) -> Result<Vec<PathBuf>> {
    let list = |d: &Path| -> Result<Vec<(String, Vec<u8>)>> {
        let mut v = Vec::new();
        for e in std::fs::read_dir(d).map_err(|e| format!("{}: {e}", d.display()))? {
            let e = e.map_err(|e| e.to_string())?;
            v.push((e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).map_err(|e| e.to_string())?));
        }
        v.sort();
        Ok(v)
    };
    let (la, lb) = (list(a)?, list(b)?);
    let names = |l: &[(String, Vec<u8>)]| l.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
    if names(&la) != names(&lb) {
        return Err(format!("two exports hold different files: {:?} and {:?}", names(&la), names(&lb)));
    }
    for ((name, x), (_, y)) in la.iter().zip(&lb) {
        if x != y {
            return Err(format!("two exports of one project differ in {name}"));
        }
    }
    eprintln!("ok: two exports, the same {} files byte for byte", la.len());
    Ok(la.into_iter().map(|(n, _)| a.join(n)).collect())
}

/// `cargo xtask repo-check`: the first step of `ci`, before anything is built.
fn repo_check() -> Result {
    step("repository: no home path, no build output, no oversized file");
    let max = Budgets::load()?.limit("repo", "file_bytes");
    if max <= 0 {
        return Err("budgets.toml: `[repo] file_bytes` is missing".into());
    }
    repo::check(&root(), max as u64)
}

/// The web module, built as a release (this machine's folders remapped) and
/// checked for paths of this machine.
fn web() -> Result<PathBuf> {
    step("web build");
    let machine = dist::Machine::here(&root())?;
    let args = ["build", "-p", "rwgpuengine_web", "--target", "wasm32-unknown-unknown", "--profile", "web"];
    dist::cargo_release(&machine, &root(), &args)?;
    let wasm = root().join("target/wasm32-unknown-unknown/web/rwgpuengine_web.wasm");
    let size = std::fs::metadata(&wasm).map_err(|e| format!("{}: {e}", wasm.display()))?.len();
    eprintln!("built {} ({size} bytes)", wasm.display());
    dist::check_artifacts(&machine, &root(), std::slice::from_ref(&wasm))?;
    Ok(wasm)
}

/// The release `rwgpu` command, built and checked like the web module.
fn release_cli() -> Result<PathBuf> {
    let machine = dist::Machine::here(&root())?;
    dist::cargo_release(&machine, &root(), &["build", "--release", "--bin", "rwgpu"])?;
    let cli = root().join("target/release").join(format!("rwgpu{}", std::env::consts::EXE_SUFFIX));
    dist::check_artifacts(&machine, &root(), std::slice::from_ref(&cli))?;
    Ok(cli)
}

/// The players' programs, built into `built` as a release. A list: each is
/// checked for paths of this machine by the gate and by `cargo xtask dist`.
const PLAYER_PROGRAMS: [&str; 2] = ["gpu_check", "rwgpu-player"];

fn players_release(machine: &dist::Machine, built: &Path) -> Result<Vec<PathBuf>> {
    let built_s = built.to_string_lossy();
    dist::cargo_release(
        machine,
        &root().join("players"),
        &["build", "--release", "--workspace", "--target-dir", &built_s],
    )?;
    Ok(PLAYER_PROGRAMS
        .iter()
        .map(|p| built.join("release").join(format!("{p}{}", std::env::consts::EXE_SUFFIX)))
        .collect())
}

/// `cargo xtask dist`: every release artifact, built the one way the gates
/// build them, and checked for paths of this machine (ADR 0023). `--agent`
/// leaves out the players' programs (which compile wgpu, but need no GPU to
/// build).
fn dist_all(args: &[String]) -> Result {
    if let Some(bad) = args.iter().find(|a| *a != "--agent") {
        return Err(format!("unknown option `{bad}`: dist takes --agent"));
    }
    step("dist: the rwgpu command");
    let cli = release_cli()?;
    let wasm = web()?;
    // Next to rwgpu, where `rwgpu serve` and `rwgpu export web` look for it.
    let beside = module_beside(&wasm, &cli)?;
    let mut artifacts = vec![cli, wasm, beside];
    if args.is_empty() {
        step("dist: the players' programs");
        let machine = dist::Machine::here(&root())?;
        let programs = players_release(&machine, &root().join("players/target-release"))?;
        dist::check_artifacts(&machine, &root(), &programs)?;
        artifacts.extend(programs);
        step("dist: the web player");
        let dir = web::build(&machine)?;
        // Beside the release `rwgpu`, where `rwgpu serve` looks first.
        let beside = root().join("target/release").join(rwgpuengine_cli::web::WEB_PLAYER_DIR);
        let _ = std::fs::remove_dir_all(&beside);
        std::fs::create_dir_all(&beside).map_err(|e| format!("{}: {e}", beside.display()))?;
        for f in rwgpuengine_cli::web::WEB_PLAYER_FILES {
            std::fs::copy(dir.join(f), beside.join(f)).map_err(|e| format!("{f}: {e}"))?;
            artifacts.push(beside.join(f));
        }
    }
    eprintln!("\nrelease artifacts, with no path of this machine:");
    for a in &artifacts {
        eprintln!("  {} ({} bytes)", a.strip_prefix(root()).unwrap_or(a).display(), file_size(a)?);
    }
    Ok(())
}

/// The first milestone, end to end, with JSON assertions at each step.
fn smoke() -> Result<u128> {
    step("smoke: new -> check -> run -> shot -> inspect -> test");
    let rwgpu = release_cli()?;
    let dir = root().join("target/xtask-smoke");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let rwgpu_s = rwgpu.to_string_lossy().to_string();

    let start = Instant::now();
    let call = |args: &[&str], cwd: &Path| -> Result<Json> {
        let mut full: Vec<&str> = args.to_vec();
        full.push("--json");
        let (ok, out) = capture(&rwgpu_s, &full, cwd)?;
        let j = Json::parse(&out).map_err(|e| format!("rwgpu {}: bad JSON ({e}): {out}", args.join(" ")))?;
        if !ok || j.get("ok").and_then(Json::as_bool) != Some(true) {
            return Err(format!("rwgpu {} failed: {}", args.join(" "), j.to_pretty()));
        }
        Ok(j)
    };

    call(&["new", "demo"], &dir)?;
    let project = dir.join("demo");
    // Step 2 of the milestone: write a small isometric scene as a text file.
    let scene = r##"
[scene]
name = "Smoke"

[ground]
size = [5, 5]

[[entity]]
name = "player"
prefab = "player"
pos = [0, 0]

[[entity]]
prefab = "wall"
cells = [[2, 0], [2, 1], [2, 3]]

[[entity]]
name = "beacon"
pos = [4, 4]
sprite = { shape = "diamond", color = "#4fc3f7", size = [20, 28] }
"##;
    std::fs::write(project.join("scenes/smoke.toml"), scene).map_err(|e| e.to_string())?;
    let input = "[test]\nscene = \"scenes/smoke.toml\"\nframes = 300\n\n[[input]]\nframe = 0\nhold = \"down\"\nfor = 16\n\n[[input]]\nframe = 40\nhold = \"right\"\nfor = 32\n\n[[expect]]\nentity = \"player\"\npos = [4, 2]\n";
    std::fs::write(project.join("tests/smoke_scene.toml"), input).map_err(|e| e.to_string())?;

    let check = call(&["check"], &project)?;
    expect_eq(&check, "errors", 0)?;
    let run_j = call(&["run", "--frames", "300", "--input", "tests/smoke_scene.toml"], &project)?;
    expect_eq(&run_j, "frame", 300)?;
    let shot = call(&["shot", "--input", "tests/smoke_scene.toml"], &project)?;
    let png = shot.get("path").and_then(Json::as_str).ok_or("shot: no path")?;
    if !project.join(png).is_file() {
        return Err(format!("shot: {png} was not written"));
    }
    let inspect = call(&["inspect", "--input", "tests/smoke_scene.toml", "--entity", "player"], &project)?;
    let cell = inspect.pointer("entities/0/cell").map(Json::to_compact).unwrap_or_default();
    if cell != "[4,2]" {
        return Err(format!("inspect: player cell is {cell}, expected [4,2]"));
    }
    let test = call(&["test"], &project)?;
    expect_eq(&test, "failed", 0)?;
    let ms = start.elapsed().as_millis();
    // Not timed: a playtest file outside tests/ runs as itself, never as
    // "run every test" (a broken copy under out/ must fail).
    std::fs::create_dir_all(project.join("out")).map_err(|e| e.to_string())?;
    std::fs::write(project.join("out/broken.toml"), input.replace("pos = [4, 2]", "pos = [0, 0]"))
        .map_err(|e| e.to_string())?;
    let (ok, out) = capture(&rwgpu_s, &["test", "out/broken.toml", "--json"], &project)?;
    let j = Json::parse(&out).map_err(|e| format!("rwgpu test out/broken.toml: bad JSON ({e})"))?;
    if ok || j.get("failed").and_then(Json::as_i64) != Some(1) || j.get("passed").and_then(Json::as_i64) != Some(0) {
        return Err(format!("rwgpu test out/broken.toml should run only that (failing) file: {}", j.to_pretty()));
    }
    eprintln!("smoke chain ok in {ms} ms (state {})", run_j.get("state_hash").and_then(Json::as_str).unwrap_or("?"));
    Ok(ms)
}

fn expect_eq(j: &Json, key: &str, want: i64) -> Result {
    match j.get(key).and_then(Json::as_i64) {
        Some(v) if v == want => Ok(()),
        other => Err(format!("expected {key} = {want}, got {other:?} in {}", j.to_pretty())),
    }
}

fn docs(check_only: bool) -> Result {
    let dir = root().join("docs/reference");
    let mut stale = Vec::new();
    for (rel, text) in rwgpuengine_cli::commands::reference_files() {
        let path = dir.join(&rel);
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        if current != text {
            if check_only {
                stale.push(rel);
            } else {
                if let Some(p) = path.parent() {
                    std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
                }
                std::fs::write(&path, text).map_err(|e| e.to_string())?;
                eprintln!("wrote docs/reference/{rel}");
            }
        }
    }
    if stale.is_empty() {
        eprintln!("docs/reference {}", if check_only { "is up to date" } else { "regenerated" });
        Ok(())
    } else {
        Err(format!("docs/reference is stale ({}); run `cargo xtask docs`", stale.join(", ")))
    }
}

// ---------------------------------------------------------------------------
// Budgets

struct Budgets(Table);

impl Budgets {
    fn load() -> Result<Budgets> {
        let path = root().join("budgets.toml");
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let table = parse_toml(&SourceFile::new("budgets.toml", text)).map_err(|d| d.render_human())?;
        Ok(Budgets(table))
    }

    fn limit(&self, section: &str, key: &str) -> i64 {
        self.0.table(section).and_then(|t| t.int(key)).unwrap_or(0)
    }

    /// A `[time]` limit. The limits are written for the reference machine
    /// (see budgets.toml); `RWGPU_TIME_BUDGET_SCALE=4` multiplies them for
    /// slower machines such as CI runners, so a regression of many times
    /// still fails there.
    fn time(&self, key: &str) -> i64 {
        self.scaled("time", key)
    }

    /// A time limit in any section, scaled the same way.
    fn scaled(&self, section: &str, key: &str) -> i64 {
        let scale = std::env::var("RWGPU_TIME_BUDGET_SCALE").ok().and_then(|v| v.parse::<i64>().ok()).unwrap_or(1);
        self.limit(section, key) * scale.max(1)
    }
}

struct Report {
    rows: Vec<(String, i64, i64)>,
}

impl Report {
    fn add(&mut self, name: &str, measured: i64, limit: i64) {
        self.rows.push((name.to_string(), measured, limit));
    }

    fn print_and_check(&self) -> Result {
        eprintln!("\n{:<32} {:>14} {:>14}  status", "budget", "measured", "limit");
        let mut over = Vec::new();
        for (name, m, l) in &self.rows {
            let status = if *l == 0 && name != "deps.proc_macro_crates" {
                "not enforced"
            } else if m <= l {
                "ok"
            } else {
                over.push(name.clone());
                "OVER"
            };
            eprintln!("{name:<32} {m:>14} {l:>14}  {status}");
        }
        if over.is_empty() { Ok(()) } else { Err(format!("over budget: {}", over.join(", "))) }
    }
}

fn budgets() -> Result {
    let b = Budgets::load()?;
    let mut r = Report { rows: Vec::new() };

    let wasm = web()?;
    r.add("web.wasm_bytes", file_size(&wasm)?, b.limit("web", "wasm_bytes"));

    let cli = release_cli()?;
    r.add("cli.binary_bytes", file_size(&cli)?, b.limit("cli", "binary_bytes"));

    let (external, proc_macros) = runtime_deps()?;
    eprintln!("runtime dependency tree: {}", external.join(", "));
    r.add("deps.runtime_crates", external.len() as i64, b.limit("deps", "runtime_crates"));
    if !proc_macros.is_empty() {
        eprintln!("proc-macro crates in the runtime tree: {}", proc_macros.join(", "));
    }
    r.add("deps.proc_macro_crates", proc_macros.len() as i64, b.limit("deps", "proc_macro_crates"));

    step("render time (release, native)");
    check_web_profile()?;
    // A release build like the others, so it shares their compiled crates.
    let machine = dist::Machine::here(&root())?;
    let out = dist::release_command(
        &machine,
        &root(),
        &["run", "--release", "--quiet", "--package", "xtask", "--", "render-bench"],
    )
    .output()
    .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("render-bench failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let us: Vec<i64> = text.split_whitespace().filter_map(|v| v.parse().ok()).collect();
    let &[render_us, paint_us, town_us] = us.as_slice() else {
        return Err(format!("render-bench: expected three numbers, got `{}`", text.trim()));
    };
    r.add("time.render_frame_us", render_us, b.time("render_frame_us"));
    r.add("time.paint_frame_us", paint_us, b.time("paint_frame_us"));
    r.add("time.reference_town_frame_us", town_us, b.time("reference_town_frame_us"));

    let smoke_ms = smoke()?;
    r.add("time.smoke_chain_ms", smoke_ms as i64, b.time("smoke_chain_ms"));

    step("incremental rebuild");
    run("cargo", &["build", "--bin", "rwgpu"])?;
    // A changed env var read by rwgpuengine_sim (option_env!) forces the same
    // recompile as a source edit, without modifying files.
    let probe = format!("{:?}", std::time::SystemTime::now());
    eprintln!("$ RWGPU_REBUILD_PROBE=... cargo build --bin rwgpu");
    let start = Instant::now();
    let status = Command::new("cargo")
        .args(["build", "--bin", "rwgpu"])
        .env("RWGPU_REBUILD_PROBE", probe)
        .current_dir(root())
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("rebuild failed".into());
    }
    r.add("time.incremental_rebuild_ms", start.elapsed().as_millis() as i64, b.time("incremental_rebuild_ms"));

    r.add("tablet.startup_ms", 0, b.limit("tablet", "startup_ms"));
    r.print_and_check()
}

/// The web profile keeps the renderer fast (ADR 0017): the size-first
/// `opt-level = "z"` made frames 22% slower, so rwgpuengine_render and rwgpuengine_core
/// are built at `opt-level = 3`. A native measurement cannot see this, so
/// check the setting itself.
fn check_web_profile() -> Result {
    let text = std::fs::read_to_string(root().join("Cargo.toml")).map_err(|e| e.to_string())?;
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    for pkg in ["rwgpuengine_render", "rwgpuengine_core"] {
        if !compact.contains(&format!("[profile.web.package.{pkg}]opt-level=3")) {
            return Err(format!(
                "Cargo.toml: `[profile.web.package.{pkg}] opt-level = 3` is missing; without it web frames are \
                 ~60% slower (ADR 0017)"
            ));
        }
    }
    Ok(())
}

/// Microseconds for one 640x480 frame of Tiny Town with its town built
/// (`build_town` at frame 120), two ways: `Session::render` (the reference
/// pixels plus the frame report, what `rwgpu shot` does) and a `Presenter`
/// made to repaint (what a player does for a frame that changed: build the
/// render list and paint it, with the buffers it keeps between frames). Each
/// is the median of 5 batches of 100. The third number is the CPU reference
/// renderer on a town of 5,000 boxes at 1280x720 ([`town::town`]): the render
/// list, its projection and the pixels with their owners, median of 5
/// batches of 10. Printed on stdout as `render paint town`; meant for a
/// release build (`budgets` runs it that way).
fn render_bench() -> Result {
    use rwgpuengine_runtime::{DataOnly, Presenter, Session, SessionConfig};
    let vfs = rwgpuengine_data::Vfs::from_dir(&root().join("examples/tiny_town")).map_err(|e| e.to_string())?;
    let cfg = SessionConfig { script: Some("tests/build_town.toml".into()), ..SessionConfig::default() };
    let mut session = Session::load(&vfs, &cfg, Box::new(DataOnly)).map_err(|d| format!("{d:?}"))?;
    while session.frame() < 120 {
        session.step(&Default::default());
    }
    let median = |n: u32, frame: &mut dyn FnMut()| {
        for _ in 0..n / 5 {
            frame();
        }
        let mut batches: Vec<u128> = (0..5)
            .map(|_| {
                let start = Instant::now();
                for _ in 0..n {
                    frame();
                }
                start.elapsed().as_micros() / n as u128
            })
            .collect();
        batches.sort_unstable();
        batches[2]
    };
    let render = median(100, &mut || {
        std::hint::black_box(session.render());
    });
    let mut presenter = Presenter::new();
    let paint = median(100, &mut || {
        presenter.reset();
        presenter.present(&session);
        std::hint::black_box(presenter.pixels());
    });
    let big = Session::load(&town::town(5_000), &SessionConfig::default(), Box::new(DataOnly))
        .map_err(|d| format!("{d:?}"))?;
    let town = median(10, &mut || {
        std::hint::black_box(rwgpuengine_render::render(&big.projected(false)));
    });
    println!("{render} {paint} {town}");
    Ok(())
}

fn file_size(p: &Path) -> Result<i64> {
    Ok(std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?.len() as i64)
}

/// (external crates, proc-macro crates) in rwgpuengine_runtime's normal dependency tree.
fn runtime_deps() -> Result<(Vec<String>, Vec<String>)> {
    deps(&root(), &["-p", "rwgpuengine_runtime"])
}

/// (external crates, proc-macro crates) in the normal dependency tree that
/// `cargo tree <which>` prints in the workspace at `dir`.
fn deps(dir: &Path, which: &[&str]) -> Result<(Vec<String>, Vec<String>)> {
    let mut args = vec!["tree"];
    args.extend_from_slice(which);
    args.extend_from_slice(&["-e", "normal", "--prefix", "none", "--format", "{p}"]);
    let (ok, tree) = capture("cargo", &args, dir)?;
    if !ok {
        return Err("cargo tree failed".into());
    }
    // `name vX.Y.Z ...`: a crate is counted once whatever its versions; a
    // proc macro once per version that is in this tree, since each version is
    // a compiler plugin of its own (and another system's version is not).
    let mut found: Vec<(String, String)> = tree
        .lines()
        .filter_map(|l| {
            let mut words = l.split_whitespace();
            Some((words.next()?.to_string(), words.next()?.trim_start_matches('v').to_string()))
        })
        .filter(|(n, _)| !n.starts_with("rwgpuengine_") && n != "gpu_check")
        .collect();
    found.sort();
    found.dedup();
    let mut names: Vec<String> = found.iter().map(|(n, _)| n.clone()).collect();
    names.dedup();

    let (ok, meta) = capture("cargo", &["metadata", "--format-version", "1"], dir)?;
    if !ok {
        return Err("cargo metadata failed".into());
    }
    let meta = Json::parse(&meta).map_err(|e| format!("cargo metadata: {e}"))?;
    let mut proc_macros = Vec::new();
    for pkg in meta.get("packages").and_then(Json::as_array).unwrap_or(&[]) {
        let name = pkg.get("name").and_then(Json::as_str).unwrap_or("");
        let version = pkg.get("version").and_then(Json::as_str).unwrap_or("");
        let is_pm = pkg.get("targets").and_then(Json::as_array).unwrap_or(&[]).iter().any(|t| {
            t.get("kind").and_then(Json::as_array).unwrap_or(&[]).iter().any(|k| k.as_str() == Some("proc-macro"))
        });
        if is_pm && found.iter().any(|(n, v)| n == name && v == version) {
            proc_macros.push(format!("{name} {version}"));
        }
    }
    proc_macros.sort();
    Ok((names, proc_macros))
}

// ---------------------------------------------------------------------------
// The players' half (players/): everything that needs a GPU

/// `cargo xtask gpu`: the gate of the players' half. It is separate from
/// `ci`, which never needs a GPU and never compiles wgpu.
///
/// - `--fast`: only the picture check, built incrementally. For the edit loop.
/// - `--study`: the options ADR 0021 chose between, measured on this adapter.
/// - `--window`: also replay a playtest in a real window (a desktop, or a
///   virtual display such as `xvfb-run`), and compare what the window showed.
/// - anything after `--` goes to the check itself (`players/gpu_check`).
fn gpu(args: &[String]) -> Result {
    let split = args.iter().position(|a| a == "--").unwrap_or(args.len());
    let (flags, pass) = (&args[..split], &args[(split + 1).min(args.len())..]);
    let pass: Vec<&str> = pass.iter().map(String::as_str).collect();
    if let Some(bad) = flags.iter().find(|a| !["--fast", "--study", "--window"].contains(&a.as_str())) {
        return Err(format!(
            "unknown option `{bad}`: gpu takes --fast, --study, --window, and `-- <options of the check>`"
        ));
    }
    let window = flags.iter().any(|a| a == "--window");
    let players = root().join("players");
    let b = Budgets::load()?;
    let pixels = b.limit("players", "pixels_apart").to_string();
    let frame_us = b.scaled("players", "gpu_frame_us").to_string();
    let limits = ["--max-pixels", pixels.as_str(), "--frame-budget-us", frame_us.as_str()];

    // Release builds of the players' half go to one folder of their own.
    // The gate empties it first (it times a clean build); --fast and --study
    // build on what is there, with the same remapping (ADR 0023), so the
    // edit loop and the gate share what they compiled.
    let machine = dist::Machine::here(&root())?;
    let built = players.join("target-release");
    let built_s = built.to_string_lossy().into_owned();
    let study = flags.iter().any(|a| a == "--study");
    if study || flags.iter().any(|a| a == "--fast") {
        let mut cmd = vec!["run", "--release", "--quiet", "-p", "gpu_check", "--target-dir", &built_s, "--"];
        cmd.extend_from_slice(&limits);
        if study {
            cmd.push("--study");
        }
        cmd.extend_from_slice(&pass);
        return dist::cargo_release(&machine, &players, &cmd);
    }

    step("the agent's half does not know wgpu");
    let lock = std::fs::read_to_string(root().join("Cargo.lock")).map_err(|e| format!("Cargo.lock: {e}"))?;
    if let Some(line) = lock.lines().find(|l| l.starts_with("name = \"wgpu") || *l == "name = \"winit\"") {
        return Err(format!("Cargo.lock of the agent's half has `{line}`; GPU and window crates belong in players/"));
    }
    eprintln!("ok: no GPU or window crate in Cargo.lock");

    step("players: format");
    run_in(
        &players,
        "cargo",
        &["fmt", "--check", "-p", "rwgpuengine_gpu", "-p", "rwgpuengine_player", "-p", "gpu_check"],
    )?;
    step("players: clippy (native)");
    run_in(&players, "cargo", &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])?;
    step("players: clippy (wasm): the renderer and the web player must build for the browser");
    let wasm = [
        "clippy",
        "-p",
        "rwgpuengine_gpu",
        "-p",
        "rwgpuengine_player",
        "--target",
        "wasm32-unknown-unknown",
        "--",
        "-D",
        "warnings",
    ];
    run_in(&players, "cargo", &wasm)?;
    step("players: tests");
    run_in(&players, "cargo", &["test", "--workspace"])?;

    let mut r = Report { rows: Vec::new() };
    step("players: dependencies");
    // Counted for each system; each budget is held on the system where its
    // number is the largest.
    let (mut crates, mut macros) = (0, 0);
    for target in ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc", "aarch64-apple-darwin"] {
        let (external, proc_macros) = deps(&players, &["--workspace", "--target", target])?;
        eprintln!(
            "{target}: {} rented crates; {} proc macros ({})",
            external.len(),
            proc_macros.len(),
            proc_macros.join(", ")
        );
        crates = crates.max(external.len());
        macros = macros.max(proc_macros.len());
    }
    // The web player: the players' workspace built for the browser.
    let (web, web_macros) = deps(&players, &["--workspace", "--target", "wasm32-unknown-unknown"])?;
    eprintln!(
        "wasm32-unknown-unknown (the web player): {} rented crates; {} proc macros ({})",
        web.len(),
        web_macros.len(),
        web_macros.join(", ")
    );
    crates = crates.max(web.len());
    macros = macros.max(web_macros.len());
    r.add("players.crates", crates as i64, b.limit("players", "crates"));
    r.add("players.proc_macro_crates", macros as i64, b.limit("players", "proc_macro_crates"));

    step("players: clean release build (the check and the player)");
    // Emptied first, so nothing is reused; the check and the player then run
    // from there, so nothing is built twice.
    let _ = std::fs::remove_dir_all(&built);
    let start = Instant::now();
    let programs = players_release(&machine, &built)?;
    r.add(
        "players.clean_release_build_ms",
        start.elapsed().as_millis() as i64,
        b.scaled("players", "clean_release_build_ms"),
    );
    step("players: no path of this machine in the programs");
    let paths = dist::check_artifacts(&machine, &root(), &programs);

    step("players: the wgpu renderer against the CPU reference");
    let exe = built.join("release").join(format!("gpu_check{}", std::env::consts::EXE_SUFFIX));
    let results = root().join("target/gpu-check/results.txt");
    let _ = std::fs::remove_file(&results);
    let mut cmd = limits.to_vec();
    cmd.extend_from_slice(&pass);
    let checked = run_in(&root(), &exe.to_string_lossy(), &cmd);
    // What the check measured: `key value` lines.
    let text = std::fs::read_to_string(&results).unwrap_or_default();
    let measured = |key: &str| text.lines().find_map(|l| l.strip_prefix(key)?.trim().parse::<i64>().ok());
    if let Some(px) = measured("pixels_apart") {
        r.add("players.pixels_apart", px, b.limit("players", "pixels_apart"));
    }
    if let Some(us) = measured("gpu_frame_us") {
        // Only a real GPU is held to the frame time; a software adapter's is printed.
        let limit = if measured("real_gpu") == Some(1) { b.scaled("players", "gpu_frame_us") } else { 0 };
        r.add("players.gpu_frame_us", us, limit);
    }
    let played = player_programs(&built, b.limit("players", "pixels_apart"), window);

    // The web player: built, measured, and played in a browser; then every
    // example exported with it and played from the folder (ADR 0025).
    let mut browser = Ok(web::Measured::default());
    let mut exported = Ok(web::Measured::default());
    let px_limit = b.limit("players", "pixels_apart");
    match web::build(&machine) {
        Ok(dir) => {
            r.add("players.web_download_bytes", web::download_bytes(&dir)?, b.limit("players", "web_download_bytes"));
            if let Ok(expected) = &played {
                let rwgpu = root().join("target/debug").join(format!("rwgpu{}", std::env::consts::EXE_SUFFIX));
                browser = web::check(&dir, &rwgpu, &exe, expected, px_limit, &web::Served::Serve);
                let start = Instant::now();
                exported = export_folders(&rwgpu, &dir, &machine).and_then(|folders| {
                    let mut all = web::Measured::default();
                    for base in ["/", PAGES_BASE] {
                        let served = web::Served::Export { folders: &folders, base };
                        let m = web::check(&dir, &rwgpu, &exe, expected, px_limit, &served)?;
                        all.ran |= m.ran;
                        all.pixels_apart = all.pixels_apart.max(m.pixels_apart);
                        all.summary.extend(m.summary);
                    }
                    Ok(all)
                });
                eprintln!(
                    "rwgpu export web: every example exported and played in a browser at / and under {PAGES_BASE} in \
                     {:.1} s",
                    start.elapsed().as_secs_f64()
                );
                let px = [&browser, &exported].iter().filter_map(|m| m.as_ref().ok()?.pixels_apart).max();
                if let Some(px) = px {
                    r.add("players.web_pixels_apart", px, px_limit);
                }
            }
        }
        Err(e) => browser = Err(e),
    }
    let budgets = r.print_and_check();
    for (title, m) in
        [("the web player in a browser", &browser), ("rwgpu export web's folders in a browser", &exported)]
    {
        if let Ok(m) = m
            && m.ran
        {
            eprintln!("\n{title}:");
            for line in &m.summary {
                eprintln!("  {line}");
            }
        }
    }
    checked?;
    played?;
    browser?;
    exported?;
    paths?;
    budgets?;
    eprintln!("\ngpu: all green");
    Ok(())
}

/// Every example exported by `rwgpu` (the debug build, which finds the
/// workspace's WebGPU player, `player`) into
/// `target/web-check-export/folders/<name>`, for the browser check; and proved
/// like any export: it took that player, a second export naming it gives the
/// same bytes, and no file holds a path of this machine (ADR 0025).
fn export_folders(rwgpu: &Path, player: &Path, machine: &dist::Machine) -> Result<PathBuf> {
    step("rwgpu export web: every example with the WebGPU player");
    let top = root().join("target/web-check-export");
    let _ = std::fs::remove_dir_all(&top);
    let (folders, named) = (top.join("folders"), top.join("named"));
    for example in EXAMPLES {
        let name = Path::new(example).file_name().and_then(|n| n.to_str()).ok_or("example name")?;
        let json = export_web(rwgpu, example, &folders.join(name), None)?;
        expect_player(&json, "webgpu", player)?;
        export_web(rwgpu, example, &named.join(name), Some(("--web-player", player)))?;
        let files = same_folders(&folders.join(name), &named.join(name))?;
        dist::check_artifacts(machine, &root(), &files)?;
    }
    Ok(folders)
}

/// The `rwgpu-player` program, run as a person or an agent runs it.
///
/// - Every example playtest, replayed with no window, ends in the state hash
///   `rwgpu test` reports for it.
/// - `rwgpu play` finds the program on PATH and passes its arguments on.
/// - With `window`, a playtest replayed in a real window reaches its hash
///   too, and the picture read back from the window is the reference's
///   picture, scaled.
fn player_programs(built: &Path, pixels_apart: i64, window: bool) -> Result<Vec<web::Expected>> {
    step("players: the rwgpu-player program against `rwgpu test`");
    let bin = built.join("release");
    let player = bin.join(format!("rwgpu-player{}", std::env::consts::EXE_SUFFIX)).to_string_lossy().into_owned();
    run("cargo", &["build", "--quiet", "--bin", "rwgpu"])?;
    let rwgpu = root().join("target/debug").join(format!("rwgpu{}", std::env::consts::EXE_SUFFIX));
    let rwgpu = rwgpu.to_string_lossy().into_owned();
    let json = |program: &str, args: &[&str], path: Option<&Path>| -> Result<Json> {
        let mut cmd = Command::new(program);
        cmd.args(args).current_dir(root());
        if let Some(dir) = path {
            let mut dirs = vec![dir.to_path_buf()];
            dirs.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
            cmd.env("PATH", std::env::join_paths(dirs).map_err(|e| e.to_string())?);
        }
        let out = finish_within(cmd, 180)?;
        let text = String::from_utf8_lossy(&out.stdout);
        let j = Json::parse(&text).map_err(|e| {
            format!("{program} {}: bad JSON ({e}): {text}{}", args.join(" "), String::from_utf8_lossy(&out.stderr))
        })?;
        if j.get("ok").and_then(Json::as_bool) != Some(true) {
            return Err(format!("{program} {} failed: {}", args.join(" "), j.to_pretty()));
        }
        Ok(j)
    };
    let field = |j: &Json, k: &str| j.get(k).map(|v| v.as_str().map_or_else(|| v.to_compact(), str::to_string));
    let mut expected = Vec::new();
    for example in EXAMPLES {
        let tests = json(&rwgpu, &["test", example, "--json"], None)?;
        for t in tests.get("tests").and_then(Json::as_array).unwrap_or(&[]) {
            let (path, hash) = (field(t, "path").unwrap_or_default(), field(t, "state_hash").unwrap_or_default());
            let got = json(&player, &[example, "--input", &path, "--headless", "--json"], None)?;
            let ok = field(&got, "state_hash").as_deref() == Some(hash.as_str())
                && field(&got, "frame") == field(t, "frames");
            eprintln!(
                "{} {example}/{path}: rwgpu test {hash}, rwgpu-player {} at frame {} ({} frames drawn)",
                if ok { "ok  " } else { "FAIL" },
                field(&got, "state_hash").unwrap_or_default(),
                field(&got, "frame").unwrap_or_default(),
                field(&got, "frames_drawn").unwrap_or_default(),
            );
            if !ok {
                return Err(format!("rwgpu-player does not reach `rwgpu test`'s state for {example}/{path}"));
            }
            let frames = field(t, "frames").unwrap_or_default();
            expected.push(web::Expected { example, test: path, frames, state: hash });
        }
    }
    let last = expected.last().cloned().ok_or("no example playtests")?;
    let (example, path, hash) = (last.example, last.test, last.state);
    let got = json(&rwgpu, &["play", example, "--input", &path, "--headless", "--json"], Some(&bin))?;
    if field(&got, "state_hash").as_deref() != Some(hash.as_str()) {
        return Err(format!("`rwgpu play` (the player found on PATH) did not reach {hash}: {}", got.to_pretty()));
    }
    eprintln!("ok   rwgpu play {example} --input {path} --headless: found rwgpu-player on PATH, state {hash}");

    if !window {
        return Ok(expected);
    }
    step("players: a replay in a real window");
    let shot = root().join("target/player/window.png");
    let reference = root().join("target/player/reference.png");
    let _ = std::fs::remove_file(&shot);
    let start = Instant::now();
    let got = json(&player, &[example, "--input", &path, "--screenshot", &shot.to_string_lossy(), "--json"], None)?;
    let took = start.elapsed().as_secs_f64();
    let window_json = got.get("window").map(Json::to_compact).unwrap_or_default();
    if field(&got, "state_hash").as_deref() != Some(hash.as_str())
        || got.get("headless").and_then(Json::as_bool) != Some(false)
    {
        return Err(format!("the replay in a window did not reach {hash}: {}", got.to_pretty()));
    }
    eprintln!("ok   {example}/{path} in a window ({took:.1} s): state {hash}; window {window_json}");
    let scene_test = format!("{example}/{path}");
    json(&rwgpu, &["shot", &scene_test, "--out", &reference.to_string_lossy(), "--json"], None)?;
    let read = |p: &Path| -> Result<rwgpuengine_render::image::Image> {
        let bytes = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
        rwgpuengine_render::image::decode_png(&bytes)
    };
    let (win, refr) = (read(&shot)?, read(&reference)?);
    let scale = win.width / refr.width.max(1);
    if scale == 0 || win.width != refr.width * scale || win.height != refr.height * scale {
        eprintln!(
            "note: the window is {} x {}, not a whole multiple of the {} x {} frame; its picture is not compared",
            win.width, win.height, refr.width, refr.height
        );
        return Ok(expected);
    }
    // Each frame pixel is a block of scale x scale window pixels.
    let at = |img: &rwgpuengine_render::image::Image, x: u32, y: u32| {
        let i = ((y * img.width + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    };
    let mut apart = 0i64;
    for y in 0..refr.height {
        for x in 0..refr.width {
            if (0..scale * scale).any(|k| at(&win, x * scale + k % scale, y * scale + k / scale) != at(&refr, x, y)) {
                apart += 1;
            }
        }
    }
    eprintln!(
        "{} the window's picture, read back, against the reference: {apart} of {} frame pixels apart (limit {pixels_apart})",
        if apart <= pixels_apart { "ok  " } else { "FAIL" },
        refr.width * refr.height
    );
    if apart > pixels_apart {
        return Err(format!("the window showed {apart} pixels apart from the reference (see {})", shot.display()));
    }
    Ok(expected)
}

/// Run `cmd` and collect its output, killing it after `seconds`: a window
/// that never opens must fail the gate, not hang it.
fn finish_within(mut cmd: Command, seconds: u64) -> Result<std::process::Output> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("{cmd:?}: {e}"))?;
    // Read both pipes while waiting, so a full pipe cannot stop the child.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let stdout = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if start.elapsed().as_secs() >= seconds {
            let _ = child.kill();
            let _ = child.wait();
            let err = stderr.join().unwrap_or_default();
            return Err(format!(
                "{cmd:?} did not finish in {seconds} s and was stopped: {}",
                String::from_utf8_lossy(&err)
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    Ok(std::process::Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}
