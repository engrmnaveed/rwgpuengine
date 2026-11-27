//! The web player on WebGPU ([ADR 0024](../../docs/decisions/0024-web-player-on-webgpu.md)):
//! its release build, the `wasm-bindgen` tool that writes its glue, and its
//! check in a real browser.
//!
//! The player is `rwgpuengine_player`'s library built for wasm32 (its `web`
//! module) and the page in `players/rwgpuengine_player/web/`. The build puts
//! the four files `rwgpu serve` serves into `target/web-player/`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use rwgpuengine_cli::web::WEB_PLAYER_FILES;
use rwgpuengine_core::codes;
use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_core::hash;
use rwgpuengine_core::json::Json;

use crate::{Result, dist, root, step};

/// Where the built player goes, and where `rwgpu serve` (a debug build run
/// from the source tree) finds it.
pub fn out_dir() -> PathBuf {
    root().join("target/web-player")
}

/// The version of the `wasm-bindgen` crate in the players' lockfile. The tool
/// that writes the glue must be exactly this version.
pub fn bindgen_version() -> Result<String> {
    let path = root().join("players/Cargo.lock");
    let lock = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line == "name = \"wasm-bindgen\"" {
            let v = lines.next().and_then(|l| l.strip_prefix("version = \"")).and_then(|l| l.strip_suffix('"'));
            return v.map(str::to_string).ok_or(format!("{}: no version after wasm-bindgen", path.display()));
        }
    }
    Err(format!("{}: no `wasm-bindgen` package", path.display()))
}

fn tool_missing(message: String, version: &str) -> String {
    Diagnostic::new(&codes::WEB_TOOL_MISSING, message)
        .hint(format!(
            "`cargo install wasm-bindgen-cli --version ={version} --locked`, then put it on PATH or name it in \
             WASM_BINDGEN"
        ))
        .render_human()
}

/// The `wasm-bindgen` tool of exactly `version`: `WASM_BINDGEN`, then the one
/// xtask installed in `target/tools/`, then `PATH`. When none is that version
/// it is installed into `target/tools/` (once; about 25 s on an Apple M5 Pro,
/// longer on a slower machine), never anywhere else.
pub fn bindgen_tool(version: &str) -> Result<PathBuf> {
    let exe = format!("wasm-bindgen{}", std::env::consts::EXE_SUFFIX);
    let home = root().join("target/tools").join(format!("wasm-bindgen-{version}"));
    let mut candidates: Vec<PathBuf> = std::env::var_os("WASM_BINDGEN").map(PathBuf::from).into_iter().collect();
    candidates.push(home.join("bin").join(&exe));
    candidates.push(PathBuf::from(&exe));
    let want = format!("wasm-bindgen {version}");
    let version_of = |p: &Path| {
        let out = Command::new(p).arg("--version").stderr(Stdio::null()).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let mut seen = Vec::new();
    for c in &candidates {
        match version_of(c) {
            Some(v) if v == want => return Ok(c.clone()),
            Some(v) => seen.push(format!("{} is {v}", c.display())),
            None => {}
        }
    }
    if !seen.is_empty() {
        eprintln!("not the pinned version {version}: {}", seen.join("; "));
    }
    eprintln!("installing wasm-bindgen {version} into {} (once, with cargo install --locked)", home.display());
    let start = Instant::now();
    let home_s = home.to_string_lossy().into_owned();
    let pinned = format!("={version}");
    let args = ["install", "wasm-bindgen-cli", "--version", &pinned, "--locked", "--bin", "wasm-bindgen", "--root"];
    let status =
        Command::new("cargo").args(args).arg(&home_s).current_dir(root()).status().map_err(|e| {
            tool_missing(format!("could not run cargo to install wasm-bindgen {version}: {e}"), version)
        })?;
    let tool = home.join("bin").join(&exe);
    if !status.success() || version_of(&tool).as_deref() != Some(want.as_str()) {
        return Err(tool_missing(format!("could not install wasm-bindgen {version} into {}", home.display()), version));
    }
    eprintln!("installed wasm-bindgen {version} in {:.1} s", start.elapsed().as_secs_f64());
    Ok(tool)
}

/// `cargo xtask web-player`: the module as a release (this machine's folders
/// remapped, ADR 0023), its glue, and the page, in [`out_dir`]. Each file is
/// checked for paths of this machine. Returns the folder.
pub fn build(machine: &dist::Machine) -> Result<PathBuf> {
    step("web player: the module (wasm32, profile web) and its glue");
    let players = root().join("players");
    let target = players.join("target-web");
    let target_s = target.to_string_lossy().into_owned();
    let start = Instant::now();
    dist::cargo_release(
        machine,
        &players,
        &[
            "rustc",
            "-p",
            "rwgpuengine_player",
            "--lib",
            "--target",
            "wasm32-unknown-unknown",
            "--profile",
            "web",
            "--crate-type",
            "cdylib",
            "--target-dir",
            &target_s,
        ],
    )?;
    let built = start.elapsed().as_secs_f64();
    let wasm = target.join("wasm32-unknown-unknown/web/rwgpuengine_player.wasm");
    let version = bindgen_version()?;
    let tool = bindgen_tool(&version)?;
    let pkg = target.join("pkg");
    let _ = std::fs::remove_dir_all(&pkg);
    let start = Instant::now();
    let status = Command::new(&tool)
        .args(["--target", "web", "--no-typescript", "--remove-name-section", "--remove-producers-section"])
        .args(["--out-name", "rwgpu_web", "--out-dir"])
        .arg(&pkg)
        .arg(&wasm)
        .status()
        .map_err(|e| tool_missing(format!("{}: {e}", tool.display()), &version))?;
    if !status.success() {
        return Err(format!("`{} {}` failed ({status})", tool.display(), wasm.display()));
    }
    let glued = start.elapsed().as_secs_f64();
    let out = out_dir();
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let page = players.join("rwgpuengine_player/web");
    for name in WEB_PLAYER_FILES {
        let from = if name.starts_with("rwgpu_web") { pkg.join(name) } else { page.join(name) };
        std::fs::copy(&from, out.join(name)).map_err(|e| format!("{}: {e}", from.display()))?;
    }
    let files: Vec<PathBuf> = WEB_PLAYER_FILES.iter().map(|f| out.join(f)).collect();
    dist::check_artifacts(machine, &root(), &files)?;
    // The glue fetches the module relative to itself, so the player works under
    // any path of a site (ADR 0025).
    let glue = std::fs::read_to_string(out.join("rwgpu_web.js")).map_err(|e| format!("rwgpu_web.js: {e}"))?;
    for bad in ["fetch('/", "fetch(\"/", "from '/", "import('/", "new URL('/"] {
        if glue.contains(bad) {
            return Err(format!("rwgpu_web.js has `{bad}`: an address that would not work under a path of a site"));
        }
    }
    if !glue.contains("new URL('rwgpu_web_bg.wasm', import.meta.url)") {
        return Err("rwgpu_web.js no longer finds the module next to itself: look at how the glue loads it".into());
    }
    let shown = out.strip_prefix(root()).unwrap_or(&out).display().to_string();
    eprintln!("built {shown} in {built:.1} s, glue by wasm-bindgen {version} in {glued:.2} s:");
    for f in &files {
        eprintln!("  {} ({} bytes)", f.file_name().unwrap_or_default().to_string_lossy(), crate::file_size(f)?);
    }
    Ok(out)
}

/// The bytes a browser downloads for the player: the four files, as served
/// (`rwgpu serve` does not compress). A game's own files are not counted.
pub fn download_bytes(dir: &Path) -> Result<i64> {
    WEB_PLAYER_FILES.iter().map(|f| crate::file_size(&dir.join(f))).sum()
}

/// One playtest the engine has played: what the browser must reach.
#[derive(Clone, Debug)]
pub struct Expected {
    pub example: &'static str,
    pub test: String,
    pub frames: String,
    pub state: String,
}

/// What the browser check measured, for the report.
#[derive(Debug, Default)]
pub struct Measured {
    pub ran: bool,
    pub pixels_apart: Option<i64>,
    pub summary: Vec<String>,
}

/// What the browser check opens.
pub enum Served<'a> {
    /// `rwgpu serve` for each example, with the WebGPU player's folder; and,
    /// on Tiny Town, the measurements (real time, a lost device, idle, turning).
    Serve,
    /// Folders `rwgpu export web` wrote: `<folders>/<example's name>` (or
    /// `<folders>` itself when it holds an index.html), each from a plain
    /// static host under `base` (ADR 0025).
    Export { folders: &'a Path, base: &'a str },
}

/// What the engine reaches for every playtest of `examples`: `rwgpu test`.
pub fn expected(rwgpu: &Path, examples: &[&'static str]) -> Result<Vec<Expected>> {
    let mut all = Vec::new();
    for example in examples {
        let out = Command::new(rwgpu)
            .args(["test", example, "--json"])
            .current_dir(root())
            .output()
            .map_err(|e| format!("rwgpu test {example}: {e}"))?;
        let j = Json::parse(&String::from_utf8_lossy(&out.stdout)).map_err(|e| format!("rwgpu test {example}: {e}"))?;
        if !out.status.success() {
            return Err(format!("rwgpu test {example} failed: {}", j.to_compact()));
        }
        for t in j.get("tests").and_then(Json::as_array).unwrap_or(&[]) {
            let text = |k: &str| t.get(k).map(|v| v.as_str().map_or_else(|| v.to_compact(), str::to_string));
            all.push(Expected {
                example,
                test: text("path").unwrap_or_default(),
                frames: text("frames").unwrap_or_default(),
                state: text("state_hash").unwrap_or_default(),
            });
        }
    }
    Ok(all)
}

/// The web player in a real browser (`players/rwgpuengine_player/web/check.mjs`):
/// every example playtest on the WebGPU path and on the CPU path ends in the
/// engine's state; the WebGPU frames are within `pixels_apart` of the
/// reference, all on edges (`gpu_check --web-frames`); the CPU path's canvas
/// holds the reference's pixels exactly; a lost device hands the game to the
/// CPU path at the same state; a still scene draws nothing.
///
/// Without Node or a browser it is skipped on a developer's machine and fails
/// in CI. `RWGPU_BROWSER_RUNS` runs it several times.
pub fn check(
    dir: &Path,
    rwgpu: &Path,
    gpu_check: &Path,
    expected: &[Expected],
    pixels_apart: i64,
    served: &Served,
) -> Result<Measured> {
    match served {
        Served::Serve => step("web player: in a browser, headless"),
        Served::Export { base, .. } => step(&format!("rwgpu export web: the folders in a browser, under {base}")),
    }
    let in_ci = std::env::var_os("CI").is_some();
    let skip = |why: &str| -> Result<Measured> {
        if in_ci {
            return Err(format!("{why}; CI must run the browser check"));
        }
        eprintln!("skipped: {why}. CI always runs it.");
        Ok(Measured::default())
    };
    if !Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success()) {
        return skip("`node` was not found");
    }
    let runs = std::env::var("RWGPU_BROWSER_RUNS").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(1).max(1);
    let mut m = Measured::default();
    for run in 1..=runs {
        if runs > 1 {
            eprintln!("\nbrowser check, run {run} of {runs}");
        }
        match one_check(dir, rwgpu, gpu_check, expected, pixels_apart, served, &mut m)? {
            true => m.ran = true,
            false => return skip("no Chromium browser was found (set RWGPU_BROWSER to one)"),
        }
    }
    Ok(m)
}

fn one_check(
    dir: &Path,
    rwgpu: &Path,
    gpu_check: &Path,
    expected: &[Expected],
    pixels_apart: i64,
    served: &Served,
    m: &mut Measured,
) -> Result<bool> {
    let script = root().join("players/rwgpuengine_player/web/check.mjs");
    let mut cmd = Command::new("node");
    let out = match served {
        Served::Serve => {
            let out = root().join("target/web-check");
            cmd.arg(&script).arg("--rwgpu").arg(rwgpu).arg("--player").arg(dir).arg("--out").arg(&out);
            cmd.args(["--measure", "--touch"]);
            out
        }
        Served::Export { folders, base } => {
            let out = root().join("target/web-check-export").join(base.trim_matches('/').replace('/', "-") + "-check");
            cmd.arg(&script).arg("--rwgpu").arg(rwgpu).arg("--player").arg(dir).arg("--out").arg(&out);
            cmd.arg("--export").arg(folders).args(["--base", base]);
            out
        }
    };
    let mut examples: Vec<&str> = expected.iter().map(|e| e.example).collect();
    examples.dedup();
    cmd.args(&examples).current_dir(root()).stderr(Stdio::inherit());
    let start = Instant::now();
    let done = cmd.output().map_err(|e| format!("node {}: {e}", script.display()))?;
    let took = start.elapsed().as_secs_f64();
    if !done.status.success() {
        return Err(format!("the browser check could not run ({})", done.status));
    }
    let text = String::from_utf8_lossy(&done.stdout);
    let j = Json::parse(&text).map_err(|e| format!("the browser check printed no JSON ({e}): {text}"))?;
    let s = |j: &Json, k: &str| j.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    if j.get("browser").is_none_or(|b| b.as_str().is_none()) {
        return Ok(false);
    }
    let mut bad = Vec::new();
    m.summary.clear();
    // On a runner whose browser is known to lose the WebGPU device of a canvas (the Linux runner's
    // SwiftShader, ADR 0024), the page as a person gets it must hand every playtest to the CPU after the
    // browser's own loss, at the engine's state. A playtest that WebGPU still drew is held to everything.
    let loses = std::env::var("RWGPU_BROWSER_LOSES_DEVICE").is_ok_and(|v| v == "1");
    let lost_by_browser = |text: &str| text.contains("the browser lost the device");
    let (mut handed, mut kept) = (0, 0);
    let browser = format!("{} ({})", s(&j, "browser"), s(&j, "program"));
    eprintln!("browser: {browser}, in {took:.1} s");
    for path in ["webgpu", "cpu"] {
        let Some(p) = j.pointer(&format!("paths/{path}")) else {
            bad.push(format!("the {path} path did not run"));
            continue;
        };
        let shown = s(p, "path");
        let how = if shown == "webgpu" { format!("WebGPU, {}", s(p, "adapter")) } else { s(p, "fallback") };
        if shown != path {
            bad.push(format!("the page meant to draw with {path} drew with {shown}: {how}"));
        }
        let rows = p.get("playtests").and_then(Json::as_array).unwrap_or(&[]);
        let mut ok = 0;
        for want in expected {
            let row = rows.iter().find(|r| {
                s(r, "example") == want.example.rsplit('/').next().unwrap_or("") && s(r, "test") == want.test
            });
            let Some(row) = row else {
                bad.push(format!("{path}: {}/{} was not played", want.example, want.test));
                continue;
            };
            let frame = row.get("frame").map(Json::to_compact).unwrap_or_default();
            let mut fine = s(row, "state") == want.state && frame == want.frames;
            let drawn_by_cpu = s(row, "path") == "cpu" || s(row, "path_after") == "cpu";
            let handed_over = loses && path == "webgpu" && drawn_by_cpu && lost_by_browser(&s(row, "fallback"));
            // A frame copy that the browser's loss cut off is that loss, on such a runner.
            let cut_off = s(row, "error").contains("the frame did not come back from the GPU");
            if !s(row, "error").is_empty() && !(handed_over && cut_off) {
                fine = false;
                bad.push(format!("{path}: {}/{}: {}", want.example, want.test, s(row, "error")));
            }
            if path == "webgpu" {
                if handed_over {
                    handed += 1;
                } else if s(row, "path") == "webgpu" {
                    kept += 1;
                }
            }
            // Each playtest drawn by the path it was meant for: a device lost on the way would hand the
            // game to the CPU, reach the same state, and prove nothing about WebGPU.
            if s(row, "path") != path && !handed_over {
                fine = false;
                bad.push(format!(
                    "{path}: {}/{} ended drawn by {} ({})",
                    want.example,
                    want.test,
                    s(row, "path"),
                    s(row, "fallback")
                ));
            }
            if path == "cpu" {
                // The canvas holds the reference's pixels: `rwgpu shot`'s image hash.
                let native = shot_hash(rwgpu, want)?;
                let canvas = std::fs::read(out.join(s(row, "picture")))
                    .map(|b| hash::hex(hash::fnv1a64(&b)))
                    .unwrap_or_else(|_| "no picture".into());
                fine &= s(row, "image") == native && canvas == native;
                if canvas != native {
                    bad.push(format!(
                        "{path}: {}/{}: the canvas shows {canvas}, rwgpu shot {native}",
                        want.example, want.test
                    ));
                }
            }
            if s(row, "state") != want.state || frame != want.frames {
                bad.push(format!(
                    "{path}: {}/{} ended at frame {frame} in {}, the engine at {} in {}",
                    want.example,
                    want.test,
                    s(row, "state"),
                    want.frames,
                    want.state
                ));
            }
            if fine {
                ok += 1;
            }
        }
        let first: Vec<String> = p
            .get("first_frame_ms")
            .and_then(Json::as_array)
            .unwrap_or(&[])
            .iter()
            .map(|v| v.as_f64().map_or_else(|| v.to_compact(), |ms| format!("{ms:.0}")))
            .collect();
        let after = if path == "webgpu" && loses {
            format!(
                ", {handed} of them drawn by the CPU after the browser lost the device and {kept} by WebGPU \
                 (RWGPU_BROWSER_LOSES_DEVICE)"
            )
        } else {
            String::new()
        };
        let line = format!(
            "{path}: {how}: {ok} of {} playtests at the engine's state{}{after}; first frame after {} ms",
            expected.len(),
            if path == "cpu" { ", the canvas holding the reference's pixels" } else { "" },
            first.join(", ")
        );
        eprintln!("{}  {line}", if ok == expected.len() && shown == path { "ok  " } else { "FAIL" });
        if path == "webgpu" && loses && handed == 0 {
            eprintln!(
                "note  the browser kept every WebGPU device: RWGPU_BROWSER_LOSES_DEVICE is no longer needed here"
            );
        }
        m.summary.push(line);
    }
    // Every camera move by touch alone, replayed as a playtest by the engine (ADR 0030).
    if let Some(t) = j.get("touch") {
        match touch_replay(rwgpu, &out, t) {
            Ok(line) => {
                eprintln!("ok    {line}");
                m.summary.push(line);
            }
            Err(e) => {
                eprintln!("FAIL  {e}");
                bad.push(e);
            }
        }
    } else if matches!(served, Served::Serve) {
        bad.push("the touch check did not run".into());
    }
    // The device lost halfway through a playtest: the CPU path goes on.
    let lost = j.pointer("measure/lost");
    let turn = expected.iter().find(|e| e.test == "tests/turn_and_build.toml");
    if let (Some(l), Some(t)) = (lost, turn) {
        let first = loses && s(l, "before") == "cpu" && lost_by_browser(&s(l, "fallback"));
        let ok = (s(l, "before") == "webgpu" || first) && s(l, "path") == "cpu" && s(l, "state") == t.state;
        let line = if first {
            format!(
                "a device lost at frame 45 of turn_and_build: the browser had lost it already ({}); drawn by {} \
                 after, state {}",
                s(l, "fallback"),
                s(l, "path"),
                s(l, "state")
            )
        } else {
            format!(
                "a device lost at frame 45 of turn_and_build: drawn by {} before and by {} after, state {}",
                s(l, "before"),
                s(l, "path"),
                s(l, "state")
            )
        };
        eprintln!("{}  {line}", if ok { "ok  " } else { "FAIL" });
        if !ok {
            bad.push(line.clone());
        }
        m.summary.push(line);
    }
    if let Some(i) = j.pointer("measure/idle") {
        let draws = i.pointer("stats/draws").and_then(Json::as_i64).unwrap_or(-1);
        let line = format!(
            "Tiny Town left alone for {} ms: {} steps, {draws} frame drawn",
            i.get("ms").and_then(Json::as_f64).unwrap_or(0.0).round(),
            i.get("frames").map(Json::to_compact).unwrap_or_default()
        );
        eprintln!("{}  {line}", if draws == 1 { "ok  " } else { "FAIL" });
        if draws != 1 {
            bad.push(line.clone());
        }
        m.summary.push(line);
    }
    if let Some(t) = j.pointer("measure/turning/stats") {
        let mean = |k: &str| {
            let sum = t.pointer(&format!("{k}/sum_us")).and_then(Json::as_f64).unwrap_or(0.0);
            let n = t.pointer(&format!("{k}/count")).and_then(Json::as_f64).unwrap_or(1.0).max(1.0);
            sum / n
        };
        let line = format!(
            "Tiny Town with the camera turning: {} frames drawn; per drawn frame (mean) list {:.0} us, draw {:.0} us, \
             show {:.0} us",
            t.get("draws").map(Json::to_compact).unwrap_or_default(),
            mean("list"),
            mean("draw"),
            mean("present")
        );
        eprintln!("     {line}");
        m.summary.push(line);
    }
    if let Some(r) = j.pointer("measure/realtime") {
        let line = format!(
            "turn_and_build replayed in real time: frame {} in {:.0} ms, {} steps let go",
            r.get("frame").map(Json::to_compact).unwrap_or_default(),
            r.get("ms").and_then(Json::as_f64).unwrap_or(0.0),
            r.pointer("stats/steps_dropped").map(Json::to_compact).unwrap_or_default()
        );
        eprintln!("     {line}");
        m.summary.push(line);
    }
    // An exported folder: every request answered from it, every file of it asked for.
    if let Served::Export { base, .. } = served {
        let rows = j.get("served").and_then(Json::as_array).unwrap_or(&[]);
        for want in &examples {
            let name = want.rsplit('/').next().unwrap_or("");
            let Some(row) = rows.iter().find(|r| s(r, "example") == name) else {
                bad.push(format!("{name}: the exported folder was not served"));
                continue;
            };
            let list = |k: &str| {
                row.get(k).and_then(Json::as_array).unwrap_or(&[]).iter().map(Json::to_compact).collect::<Vec<_>>()
            };
            let (missed, unused) = (list("bad"), list("unused"));
            let line = format!(
                "{name}: {} requests under {base}, all answered from the folder; every file of it asked for",
                row.get("requests").map(Json::to_compact).unwrap_or_default()
            );
            let size =
                |part: &str, k: &str| row.pointer(&format!("sizes/{part}/{k}")).and_then(Json::as_i64).unwrap_or(0);
            let sizes = format!(
                "{name}: the folder {} bytes ({} gzipped): player {} ({}), game {} ({})",
                size("player", "bytes") + size("game", "bytes"),
                size("player", "gzip") + size("game", "gzip"),
                size("player", "bytes"),
                size("player", "gzip"),
                size("game", "bytes"),
                size("game", "gzip")
            );
            eprintln!("      {sizes}");
            if !m.summary.contains(&sizes) {
                m.summary.push(sizes);
            }
            if missed.is_empty() && unused.is_empty() {
                eprintln!("ok    {line}");
            } else {
                bad.push(format!("{name} under {base}: not answered {missed:?}, never asked for {unused:?}"));
            }
        }
        m.summary.push(format!("the exported folders under {base}: every request answered, every file used"));
    }
    let errors = j.get("errors").and_then(Json::as_array).unwrap_or(&[]);
    if !errors.is_empty() {
        bad.push(format!("the page reported: {}", errors.iter().map(Json::to_compact).collect::<Vec<_>>().join("; ")));
    }
    eprintln!(
        "download: {} bytes, {} gzipped",
        j.get("bytes").map(Json::to_compact).unwrap_or_default(),
        j.get("gzip").map(Json::to_compact).unwrap_or_default()
    );
    m.summary.push(format!(
        "{browser}; download {} bytes ({} gzipped)",
        j.get("bytes").map(Json::to_compact).unwrap_or_default(),
        j.get("gzip").map(Json::to_compact).unwrap_or_default()
    ));

    // The WebGPU frames against the reference, by gpu_check's own rule.
    let listed = std::fs::read_to_string(out.join("frames.txt")).unwrap_or_default();
    if loses && listed.trim().is_empty() {
        let line = "no WebGPU picture here: the browser lost the device before any frame came back; the macOS \
                    and Windows runners carry the picture proof (ADR 0024)";
        eprintln!("note  {line}");
        m.summary.push(line.into());
        return if bad.is_empty() {
            Ok(true)
        } else {
            Err(format!("the web player in {browser}:\n  {}", bad.join("\n  ")))
        };
    }
    let limit = pixels_apart.to_string();
    let frames = out.to_string_lossy().into_owned();
    let status = Command::new(gpu_check)
        .args(["--web-frames", &frames, "--max-pixels", &limit])
        .current_dir(root())
        .status()
        .map_err(|e| format!("{}: {e}", gpu_check.display()))?;
    let results = std::fs::read_to_string(root().join("target/gpu-check/web-results.txt")).unwrap_or_default();
    let px = results.lines().find_map(|l| l.strip_prefix("pixels_apart")?.trim().parse::<i64>().ok());
    m.pixels_apart = m.pixels_apart.max(px);
    if !status.success() {
        bad.push("the WebGPU frames are not within the pixel limit of the reference".into());
    }
    if bad.is_empty() { Ok(true) } else { Err(format!("the web player in {browser}:\n  {}", bad.join("\n  "))) }
}

/// The touch check's verdict (`check.mjs --touch`, ADR 0030): every one of
/// its checks held on the page, and what the page recorded (its camera moves
/// and taps), written as a playtest and run by `rwgpu run`, ends in the
/// page's state hash and camera, bit for bit.
fn touch_replay(rwgpu: &Path, out: &Path, t: &Json) -> Result<String> {
    let s = |j: &Json, k: &str| j.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let failed: Vec<String> = t
        .get("checks")
        .and_then(Json::as_array)
        .unwrap_or(&[])
        .iter()
        .filter(|c| c.get("ok").and_then(Json::as_bool) != Some(true))
        .map(|c| format!("{}: {}", s(c, "name"), s(c, "detail")))
        .collect();
    if !failed.is_empty() {
        return Err(format!("touch on the page: {}", failed.join("; ")));
    }
    let frame = t.get("frame").and_then(Json::as_i64).ok_or("the touch check gave no frame")?;
    let recorded = t.get("recorded").and_then(Json::as_array).unwrap_or(&[]);
    let toml = touch_playtest(recorded, frame);
    let file = out.join("touch.toml");
    std::fs::write(&file, &toml).map_err(|e| format!("{}: {e}", file.display()))?;
    let done = Command::new(rwgpu)
        .args(["run", "examples/tiny_town", "--json", "--input"])
        .arg(&file)
        .current_dir(root())
        .output()
        .map_err(|e| format!("rwgpu run: {e}"))?;
    let j = Json::parse(&String::from_utf8_lossy(&done.stdout)).map_err(|e| format!("rwgpu run: {e}"))?;
    let camera = |c: Option<&Json>| c.map(Json::to_compact).unwrap_or_default();
    let page_camera = Json::parse(&s(t, "camera_text")).ok();
    let (state, cam) = (s(&j, "state_hash"), camera(j.get("camera")));
    let moves = recorded.iter().filter(|r| r.get("camera").is_some()).count();
    let taps = recorded.iter().filter(|r| r.get("tap").is_some()).count();
    if state != s(t, "state") || cam != camera(page_camera.as_ref()) {
        return Err(format!(
            "touch: the page ended in state {} with camera {}, its record replayed by `rwgpu run` ({}) in {state} with {cam}",
            s(t, "state"),
            camera(page_camera.as_ref()),
            file.display()
        ));
    }
    let checks = t.get("checks").and_then(Json::as_array).map_or(0, <[Json]>::len);
    Ok(format!(
        "touch on a phone-sized page: pan, pinch, twist, a tap of two and of three fingers, a pan past the edge, \
         {checks} checks held; its {moves} camera moves and {taps} taps, replayed by `rwgpu run`, end in the same \
         state {state} and camera"
    ))
}

/// The page's record of its live input as a playtest: one `[[input]]` per
/// camera move, tap or press, at the frame it happened, its numbers written
/// as the exact values the page used.
pub fn touch_playtest(recorded: &[Json], frames: i64) -> String {
    let num = |j: Option<&Json>| j.and_then(Json::as_f64).map(|v| format!("{v}")).unwrap_or_else(|| "0".into());
    let pair = |j: Option<&Json>| {
        let a = j.and_then(Json::as_array).unwrap_or(&[]);
        format!("[{}, {}]", num(a.first()), num(a.get(1)))
    };
    let mut out = format!(
        "[test]\ndoc = \"What the web player's page recorded while touches moved its camera (cargo xtask gpu, ADR 0030).\"\nframes = {frames}\n"
    );
    for r in recorded {
        let frame = r.get("frame").and_then(Json::as_i64).unwrap_or(0);
        out.push_str(&format!("\n[[input]]\nframe = {frame}\n"));
        if let Some(c) = r.get("camera") {
            let view = c.get("view").and_then(Json::as_str).map(|v| format!("view = \"{v}\", ")).unwrap_or_default();
            out.push_str(&format!(
                "camera = {{ {view}turn = {}, zoom = {}, pan = {} }}\n",
                num(c.get("turn")),
                num(c.get("zoom")),
                pair(c.get("pan"))
            ));
        } else if let Some(p) = r.get("tap") {
            out.push_str(&format!("tap = {}\n", pair(Some(p))));
        } else if let Some(a) = r.get("press").and_then(Json::as_str) {
            out.push_str(&format!("press = \"{a}\"\n"));
        }
    }
    out
}

/// `rwgpu shot`'s image hash at the end of a playtest.
fn shot_hash(rwgpu: &Path, want: &Expected) -> Result<String> {
    let png = root().join("target/web-check/shot.png");
    let out = Command::new(rwgpu)
        .args(["shot", "--input", &want.test, "--frame", &want.frames, "--out"])
        .arg(&png)
        .arg("--json")
        .current_dir(root().join(want.example))
        .output()
        .map_err(|e| format!("rwgpu shot: {e}"))?;
    let j = Json::parse(&String::from_utf8_lossy(&out.stdout)).map_err(|e| format!("rwgpu shot: {e}"))?;
    j.get("image_hash").and_then(Json::as_str).map(str::to_string).ok_or(format!("rwgpu shot: {}", j.to_compact()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page's record, written as a playtest, reads back to the very
    /// numbers the page's camera moves had: f32 values that a short decimal
    /// would round, tiny and negative ones, a view, taps and a press.
    #[test]
    fn the_touch_record_reads_back_as_the_same_moves() {
        use rwgpuengine_data::{CameraView, InputKind, Vfs, load_playtest};
        let moves = [
            (0.1f32, 1.0f32 / 3.0, [1.0e-7f32, -2.5e-9]),
            (90.0, 1.5000004, [-4.399_999, 7.25]),
            (-12.345_679, 0.6666667, [0.0, 0.0]),
        ];
        let n = |v: f32| Json::from(f64::from(v));
        let mut recorded: Vec<Json> = moves
            .iter()
            .enumerate()
            .map(|(i, (turn, zoom, pan))| {
                let mut c = Json::object()
                    .with("turn", n(*turn))
                    .with("zoom", n(*zoom))
                    .with("pan", vec![n(pan[0]), n(pan[1])]);
                if i == 1 {
                    c.set("view", "perspective");
                }
                Json::object().with("camera", c).with("frame", i as u64)
            })
            .collect();
        recorded.push(Json::object().with("tap", [304.0f32, 232.0]).with("frame", 3u64));
        recorded.push(Json::object().with("press", "up").with("frame", 4u64));
        let text = touch_playtest(&recorded, 9);
        let mut vfs = Vfs::new();
        vfs.insert("tests/touch.toml", text.as_str());
        let (def, diags) = load_playtest(&vfs, "tests/touch.toml");
        assert!(diags.is_empty(), "{diags:?}\n{text}");
        let def = def.expect("a playtest");
        assert_eq!(def.frames, 9);
        for (i, (turn, zoom, pan)) in moves.iter().enumerate() {
            let InputKind::Camera(m) = &def.inputs[i].kind else { panic!("{:?}", def.inputs[i]) };
            assert_eq!((m.turn.to_bits(), m.zoom.to_bits()), (turn.to_bits(), zoom.to_bits()), "{text}");
            assert_eq!(m.pan.map(f32::to_bits), pan.map(f32::to_bits), "{text}");
            assert_eq!(m.view, (i == 1).then_some(CameraView::Perspective));
        }
        assert!(matches!(def.inputs[3].kind, InputKind::Tap(304.0, 232.0)) && def.inputs[3].frame == 3);
        assert!(matches!(def.inputs[4].kind, InputKind::Press(_)) && def.inputs[4].frame == 4);
    }

    /// The tool must be the crate's exact version: the players' manifest pins
    /// the crate with `=`, and the lockfile agrees.
    #[test]
    fn the_wasm_bindgen_crate_is_pinned_to_the_tools_version() {
        let version = bindgen_version().unwrap();
        let manifest = std::fs::read_to_string(root().join("players/Cargo.toml")).unwrap();
        let pin = format!("wasm-bindgen = {{ version = \"={version}\"");
        assert!(manifest.contains(&pin), "players/Cargo.toml should pin `{pin}`");
    }

    #[test]
    fn the_player_files_are_the_ones_serve_serves() {
        assert_eq!(WEB_PLAYER_FILES, ["index.html", "player.js", "rwgpu_web.js", "rwgpu_web_bg.wasm"]);
        let page = root().join("players/rwgpuengine_player/web");
        for f in ["index.html", "player.js", "check.mjs"] {
            assert!(page.join(f).is_file(), "{f}");
        }
        let script = std::fs::read_to_string(page.join("player.js")).unwrap();
        assert!(script.contains("from './rwgpu_web.js'") && script.contains("'rwgpu_web_bg.wasm'"));
    }

    /// The page and its script name files relative to themselves, so a folder
    /// `rwgpu export web` writes works under any path of a site (ADR 0025); the
    /// glue is checked where it is written, and the browser check serves the
    /// folders under a path. The game's files have the names `rwgpu serve` and
    /// `rwgpu export web` give them.
    #[test]
    fn the_web_player_asks_for_everything_by_a_relative_address() {
        use rwgpuengine_cli::web::{BUNDLE, BUNDLE_HASH, PROJECT_JSON};
        let page = root().join("players/rwgpuengine_player/web");
        for name in ["index.html", "player.js"] {
            let text = std::fs::read_to_string(page.join(name)).unwrap();
            for bad in ["src=\"/", "href=\"/", "fetch('/", "fetch(\"/", "from '/", "import('/", "http://", "https://"] {
                assert!(!text.contains(bad), "{name} has `{bad}`");
            }
        }
        let script = std::fs::read_to_string(page.join("player.js")).unwrap();
        for name in [PROJECT_JSON, BUNDLE, BUNDLE_HASH] {
            assert!(script.contains(&format!("fetch('{name}'")), "player.js does not ask for {name}");
        }
        assert!(script.contains("if (project.reload)"), "only `rwgpu serve` has the bundle's hash to poll");
    }
}
