//! Dev automation for the slate workspace. Run through the cargo alias:
//!
//! ```text
//! cargo xtask ci        everything CI runs, in order (fails fast)
//! cargo xtask web       build slate_web.wasm (web profile)
//! cargo xtask smoke     the first-milestone command chain on a fresh project
//! cargo xtask budgets   measure and enforce budgets.toml
//! cargo xtask docs      regenerate docs/reference (`--check` to verify only)
//! ```

// xtask measures wall-clock time on purpose (budgets); the determinism ban
// on Instant::now applies to the engine, not to dev tooling.
#![allow(clippy::disallowed_methods)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

use slate_core::diag::SourceFile;
use slate_core::json::Json;
use slate_data::node::{Table, parse_toml};

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
        _ => {
            eprintln!("usage: cargo xtask <ci | web | smoke | budgets | docs [--check]>");
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
    run("cargo", &["clippy", "-p", "slate_web", "--target", "wasm32-unknown-unknown", "--", "-D", "warnings"])?;
    step("tests");
    run("cargo", &["test", "--workspace"])?;
    step("generated docs are current");
    docs(true)?;
    step("example playtests");
    for example in ["examples/iso_garden", "examples/coin_vault", "examples/tiny_town"] {
        run("cargo", &["run", "--quiet", "--bin", "slate", "--", "check", example])?;
        run("cargo", &["run", "--quiet", "--bin", "slate", "--", "test", example])?;
    }
    step("budgets (builds web + release, runs smoke)");
    budgets()?;
    eprintln!("\nci: all green");
    Ok(())
}

fn web() -> Result<PathBuf> {
    step("web build");
    run("cargo", &["build", "-p", "slate_web", "--target", "wasm32-unknown-unknown", "--profile", "web"])?;
    let wasm = root().join("target/wasm32-unknown-unknown/web/slate_web.wasm");
    let size = std::fs::metadata(&wasm).map_err(|e| format!("{}: {e}", wasm.display()))?.len();
    eprintln!("built {} ({size} bytes)", wasm.display());
    Ok(wasm)
}

fn release_cli() -> Result<PathBuf> {
    run("cargo", &["build", "--release", "--bin", "slate"])?;
    Ok(root().join("target/release/slate"))
}

/// The first milestone, end to end, with JSON assertions at each step.
fn smoke() -> Result<u128> {
    step("smoke: new -> check -> run -> shot -> inspect -> test");
    let slate = release_cli()?;
    let dir = root().join("target/xtask-smoke");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let slate_s = slate.to_string_lossy().to_string();

    let start = Instant::now();
    let call = |args: &[&str], cwd: &Path| -> Result<Json> {
        let mut full: Vec<&str> = args.to_vec();
        full.push("--json");
        let (ok, out) = capture(&slate_s, &full, cwd)?;
        let j = Json::parse(&out).map_err(|e| format!("slate {}: bad JSON ({e}): {out}", args.join(" ")))?;
        if !ok || j.get("ok").and_then(Json::as_bool) != Some(true) {
            return Err(format!("slate {} failed: {}", args.join(" "), j.to_pretty()));
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
    for (rel, text) in slate_cli::commands::reference_files() {
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

    let smoke_ms = smoke()?;
    r.add("time.smoke_chain_ms", smoke_ms as i64, b.limit("time", "smoke_chain_ms"));

    step("incremental rebuild");
    run("cargo", &["build", "--bin", "slate"])?;
    // A changed env var read by slate_sim (option_env!) forces the same
    // recompile as a source edit, without modifying files.
    let probe = format!("{:?}", std::time::SystemTime::now());
    eprintln!("$ SLATE_REBUILD_PROBE=... cargo build --bin slate");
    let start = Instant::now();
    let status = Command::new("cargo")
        .args(["build", "--bin", "slate"])
        .env("SLATE_REBUILD_PROBE", probe)
        .current_dir(root())
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("rebuild failed".into());
    }
    r.add("time.incremental_rebuild_ms", start.elapsed().as_millis() as i64, b.limit("time", "incremental_rebuild_ms"));

    r.add("tablet.startup_ms", 0, b.limit("tablet", "startup_ms"));
    r.print_and_check()
}

fn file_size(p: &Path) -> Result<i64> {
    Ok(std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?.len() as i64)
}

/// (external crates, proc-macro crates) in slate_runtime's normal dependency tree.
fn runtime_deps() -> Result<(Vec<String>, Vec<String>)> {
    let (ok, tree) = capture(
        "cargo",
        &["tree", "-p", "slate_runtime", "-e", "normal", "--prefix", "none", "--format", "{p}"],
        &root(),
    )?;
    if !ok {
        return Err("cargo tree failed".into());
    }
    let mut names: Vec<String> = tree
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|n| !n.starts_with("slate_"))
        .map(str::to_string)
        .collect();
    names.sort();
    names.dedup();

    let (ok, meta) = capture("cargo", &["metadata", "--format-version", "1"], &root())?;
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
