//! Dev automation for the rwgpuengine workspace. Run through the cargo alias:
//!
//! ```text
//! cargo xtask ci        everything CI runs, in order (fails fast)
//! cargo xtask web       build rwgpuengine_web.wasm (web profile)
//! cargo xtask smoke     the first-milestone command chain on a fresh project
//! cargo xtask budgets   measure and enforce budgets.toml
//! cargo xtask docs      regenerate docs/reference (`--check` to verify only)
//! cargo xtask wasm-check  the wasm module against the native command (needs Node)
//! cargo xtask gpu       the players' half: the wgpu renderer against the CPU
//!                       reference, and the players' budgets (needs a GPU or a
//!                       software adapter; not part of `ci`)
//! ```
//!
//! `render-bench` is internal: `budgets` runs it as a release build.

// xtask measures wall-clock time on purpose (budgets); the determinism ban
// on Instant::now applies to the engine, not to dev tooling.
#![allow(clippy::disallowed_methods)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

mod town;

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
        "web" => web().map(|_| ()),
        "smoke" => smoke().map(|_| ()),
        "budgets" => budgets(),
        "docs" => docs(rest.iter().any(|a| a == "--check")),
        "wasm-check" => wasm_check(),
        "render-bench" => render_bench(),
        "gpu" => gpu(rest),
        _ => {
            eprintln!(
                "usage: cargo xtask <ci | web | smoke | budgets | docs [--check] | wasm-check | gpu [--fast | --study] \
                 [-- <options of players/gpu_check>]>"
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
    let has_node = Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success());
    if !has_node {
        if std::env::var_os("CI").is_some() {
            return Err("`node` was not found; CI must run the wasm check".into());
        }
        eprintln!("skipped: `node` was not found. Install Node.js to run this check; CI always runs it.");
        return Ok(());
    }
    let wasm = web()?.to_string_lossy().into_owned();
    let cli = release_cli()?.to_string_lossy().into_owned();
    for example in EXAMPLES {
        run("node", &["web/check.mjs", &wasm, example, "--native", &cli])?;
    }
    Ok(())
}

fn web() -> Result<PathBuf> {
    step("web build");
    run("cargo", &["build", "-p", "rwgpuengine_web", "--target", "wasm32-unknown-unknown", "--profile", "web"])?;
    let wasm = root().join("target/wasm32-unknown-unknown/web/rwgpuengine_web.wasm");
    let size = std::fs::metadata(&wasm).map_err(|e| format!("{}: {e}", wasm.display()))?.len();
    eprintln!("built {} ({size} bytes)", wasm.display());
    Ok(wasm)
}

fn release_cli() -> Result<PathBuf> {
    run("cargo", &["build", "--release", "--bin", "rwgpu"])?;
    Ok(root().join("target/release").join(format!("rwgpu{}", std::env::consts::EXE_SUFFIX)))
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
    let out = Command::new("cargo")
        .args(["run", "--release", "--quiet", "--package", "xtask", "--", "render-bench"])
        .current_dir(root())
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
    let mut names: Vec<String> = tree
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|n| !n.starts_with("rwgpuengine_") && *n != "gpu_check")
        .map(str::to_string)
        .collect();
    names.sort();
    names.dedup();

    let (ok, meta) = capture("cargo", &["metadata", "--format-version", "1"], dir)?;
    if !ok {
        return Err("cargo metadata failed".into());
    }
    let meta = Json::parse(&meta).map_err(|e| format!("cargo metadata: {e}"))?;
    let mut proc_macros = Vec::new();
    for pkg in meta.get("packages").and_then(Json::as_array).unwrap_or(&[]) {
        let name = pkg.get("name").and_then(Json::as_str).unwrap_or("");
        let is_pm = pkg.get("targets").and_then(Json::as_array).unwrap_or(&[]).iter().any(|t| {
            t.get("kind").and_then(Json::as_array).unwrap_or(&[]).iter().any(|k| k.as_str() == Some("proc-macro"))
        });
        if is_pm && names.iter().any(|n| n == name) {
            proc_macros.push(name.to_string());
        }
    }
    Ok((names, proc_macros))
}

// ---------------------------------------------------------------------------
// The players' half (players/): everything that needs a GPU

/// `cargo xtask gpu`: the gate of the players' half. It is separate from
/// `ci`, which never needs a GPU and never compiles wgpu.
///
/// - `--fast`: only the picture check, built incrementally. For the edit loop.
/// - `--study`: the options ADR 0021 chose between, measured on this adapter.
/// - anything after `--` goes to the check itself (`players/gpu_check`).
fn gpu(args: &[String]) -> Result {
    let split = args.iter().position(|a| a == "--").unwrap_or(args.len());
    let (flags, pass) = (&args[..split], &args[(split + 1).min(args.len())..]);
    let pass: Vec<&str> = pass.iter().map(String::as_str).collect();
    if let Some(bad) = flags.iter().find(|a| !["--fast", "--study"].contains(&a.as_str())) {
        return Err(format!("unknown option `{bad}`: gpu takes --fast, --study, and `-- <options of the check>`"));
    }
    let players = root().join("players");
    let b = Budgets::load()?;
    let pixels = b.limit("players", "pixels_apart").to_string();
    let frame_us = b.scaled("players", "gpu_frame_us").to_string();
    let limits = ["--max-pixels", pixels.as_str(), "--frame-budget-us", frame_us.as_str()];

    // Release builds of the players' half go to one folder of their own.
    // The gate empties it first (it times a clean build); --fast and --study
    // build on what is there.
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
        return run_in(&players, "cargo", &cmd);
    }

    step("the agent's half does not know wgpu");
    let lock = std::fs::read_to_string(root().join("Cargo.lock")).map_err(|e| format!("Cargo.lock: {e}"))?;
    if let Some(line) = lock.lines().find(|l| l.starts_with("name = \"wgpu") || *l == "name = \"winit\"") {
        return Err(format!("Cargo.lock of the agent's half has `{line}`; GPU and window crates belong in players/"));
    }
    eprintln!("ok: no GPU or window crate in Cargo.lock");

    step("players: format");
    run_in(&players, "cargo", &["fmt", "--check", "-p", "rwgpuengine_gpu", "-p", "gpu_check"])?;
    step("players: clippy (native)");
    run_in(&players, "cargo", &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])?;
    step("players: clippy (wasm): the renderer must build for the browser");
    let wasm = ["clippy", "-p", "rwgpuengine_gpu", "--target", "wasm32-unknown-unknown", "--", "-D", "warnings"];
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
    let (web, _) = deps(&players, &["-p", "rwgpuengine_gpu", "--target", "wasm32-unknown-unknown"])?;
    eprintln!("wasm32-unknown-unknown (the renderer alone): {} rented crates", web.len());
    r.add("players.crates", crates as i64, b.limit("players", "crates"));
    r.add("players.proc_macro_crates", macros as i64, b.limit("players", "proc_macro_crates"));

    step("players: clean release build");
    // Emptied first, so nothing is reused; the check then runs from there,
    // so nothing is built twice.
    let _ = std::fs::remove_dir_all(&built);
    let start = Instant::now();
    run_in(&players, "cargo", &["build", "--release", "-p", "gpu_check", "--target-dir", &built_s])?;
    r.add(
        "players.clean_release_build_ms",
        start.elapsed().as_millis() as i64,
        b.scaled("players", "clean_release_build_ms"),
    );

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
    let budgets = r.print_and_check();
    checked?;
    budgets?;
    eprintln!("\ngpu: all green");
    Ok(())
}
