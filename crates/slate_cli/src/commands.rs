//! The commands. Each returns an [`Outcome`]; `lib.rs` prints it.

use std::path::{Path, PathBuf};

use slate_core::codes;
use slate_core::diag::Diagnostic;
use slate_core::hash;
use slate_core::json::Json;
use slate_core::text::did_you_mean;
use slate_data::{check_project, schema};
use slate_render::image::{self, Image};
use slate_runtime::{InspectOptions, Session, SessionConfig, run_playtest};
use slate_sim::dump;

use crate::GameFactory;
use crate::args::{self, COMMANDS, Parsed};
use crate::docs;
use crate::out::{Outcome, display_path};
use crate::project::{Located, io_err, locate, write_file};

macro_rules! tri {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(d) => return Outcome::failed(vec![d]),
        }
    };
}

// ---------------------------------------------------------------------------
// new

const TEMPLATE: &[(&str, &str)] = &[
    ("slate.toml", include_str!("../template/slate.toml")),
    ("scenes/main.toml", include_str!("../template/scenes/main.toml")),
    ("prefabs/player.toml", include_str!("../template/prefabs/player.toml")),
    ("prefabs/wall.toml", include_str!("../template/prefabs/wall.toml")),
    ("prefabs/crate.toml", include_str!("../template/prefabs/crate.toml")),
    ("prefabs/door.toml", include_str!("../template/prefabs/door.toml")),
    ("prefabs/plate.toml", include_str!("../template/prefabs/plate.toml")),
    ("tests/smoke.toml", include_str!("../template/tests/smoke.toml")),
    (".gitignore", include_str!("../template/gitignore")),
    ("CLAUDE.md", include_str!("../template/CLAUDE.md")),
];

/// The starter project's files, rendered for `name`.
pub fn template_files(name: &str) -> Vec<(String, String)> {
    let engine = slate_data::load::major_minor(slate_core::ENGINE_VERSION);
    let mut files: Vec<(String, String)> = TEMPLATE
        .iter()
        .map(|(p, t)| (p.to_string(), t.replace("{{name}}", name).replace("{{engine}}", &engine)))
        .collect();
    files.push(("AGENTS.md".into(), docs::project_agents_md(name)));
    files
}

pub fn new(p: &Parsed) -> Outcome {
    let Some(target) = p.positional(0) else {
        return Outcome::usage(args::bad_arg(
            "`slate new` needs a folder name".into(),
            Some("slate new my-game".into()),
        ));
    };
    let dir = PathBuf::from(target);
    if dir.exists() && std::fs::read_dir(&dir).map(|mut d| d.next().is_some()).unwrap_or(true) {
        return Outcome::failed(vec![
            Diagnostic::new(&codes::ALREADY_EXISTS, format!("`{target}` already exists and is not empty"))
                .hint("choose a new folder name"),
        ]);
    }
    let folder = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "game".into());
    let name = p.value("--name").map(str::to_string).unwrap_or(folder).replace('"', "'");
    let files = template_files(&name);
    for (rel, text) in &files {
        tri!(write_file(&dir.join(rel), text.as_bytes()));
    }
    // Self-check: a fresh project must be valid.
    let vfs = tri!(slate_data::Vfs::from_dir(&dir).map_err(|e| io_err(target, &e)));
    let report = check_project(&vfs);
    let mut o = Outcome::new(report.ok()).with_root(&dir);
    o.diagnostics = report.diagnostics;
    let list: Vec<Json> = files.iter().map(|(f, _)| Json::from(f.as_str())).collect();
    o.data = Json::object().with("name", name.as_str()).with("files", list);
    o.line(format!("created {} `{name}` with {} files", display_path(&dir), files.len()));
    for (f, _) in &files {
        o.line(format!("  {f}"));
    }
    o.line(format!("\nnext: cd {} && slate check && slate test   (then read AGENTS.md)", display_path(&dir)));
    o.settle()
}

// ---------------------------------------------------------------------------
// check

pub fn check(p: &Parsed) -> Outcome {
    let loc = tri!(locate(p.positional(0)));
    let report = check_project(&loc.vfs);
    let mut o = Outcome::new(report.ok()).with_root(&loc.root);
    let (errors, warnings) = slate_core::diag::count(&report.diagnostics);
    let list = |v: &[String]| Json::Arr(v.iter().map(Json::from).collect());
    o.data = Json::object()
        .with("project", report.project.as_ref().map(|p| p.name.clone()))
        .with("scenes", list(&report.scenes))
        .with("prefabs", list(&report.prefabs))
        .with("playtests", list(&report.playtests));
    let verdict = if report.ok() { "ok" } else { "failed" };
    o.line(format!(
        "check {verdict}: {} scene(s), {} prefab(s), {} playtest(s) — {errors} error(s), {warnings} warning(s)",
        report.scenes.len(),
        report.prefabs.len(),
        report.playtests.len()
    ));
    o.diagnostics = report.diagnostics;
    o.settle()
}

// ---------------------------------------------------------------------------
// run / shot / inspect share session loading

struct Loaded {
    loc: Located,
    session: Session,
}

fn load_session(p: &Parsed, games: &GameFactory) -> Result<Loaded, Outcome> {
    let mut loc = locate(p.positional(0)).map_err(|d| Outcome::failed(vec![d]))?;
    let seed = p.number("--seed").map_err(Outcome::usage)?;
    let script = match p.value("--input") {
        Some(i) => Some(input_script(&mut loc, i).map_err(|d| Outcome::failed(vec![d]))?),
        None => loc.playtest_arg(),
    };
    let cfg = SessionConfig { scene: loc.scene_arg(), script, seed };
    match Session::load(&loc.vfs, &cfg, games()) {
        Ok(session) => Ok(Loaded { loc, session }),
        Err(diags) => {
            let root = loc.root.clone();
            Err(Outcome::failed(diags).with_root(&root).settle())
        }
    }
}

/// Resolve `--input`: a path relative to the project root or the current
/// folder. A file outside the project's scanned files (another folder,
/// `out/`) is read from disk and added under the path as given; scene paths
/// inside it are still relative to the project root.
fn input_script(loc: &mut Located, path: &str) -> Result<String, Diagnostic> {
    let rel = to_project_rel(loc, path);
    if loc.vfs.contains(&rel) {
        return Ok(rel);
    }
    let on_disk = std::env::current_dir().unwrap_or_default().join(path);
    if on_disk.is_file() {
        let text = std::fs::read_to_string(&on_disk).map_err(|e| io_err(path, &e))?;
        loc.vfs.insert(path, text);
        return Ok(path.to_string());
    }
    Ok(rel) // the session reports E0001 with suggestions
}

/// `--input` may be given relative to the cwd or to the project root.
fn to_project_rel(loc: &Located, path: &str) -> String {
    if loc.vfs.contains(path) {
        return path.to_string();
    }
    let abs = crate::out::normalize(&std::env::current_dir().unwrap_or_default().join(path));
    match abs.strip_prefix(&loc.root) {
        Ok(rel) => rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"),
        Err(_) => path.to_string(),
    }
}

fn frame_arg(p: &Parsed, name: &str, session: &Session, default: u64) -> Result<u64, Outcome> {
    Ok(p.number(name).map_err(Outcome::usage)?.or(session.script_frames()).unwrap_or(default))
}

pub fn run(p: &Parsed, games: &GameFactory) -> Outcome {
    let Loaded { loc, mut session } = match load_session(p, games) {
        Ok(l) => l,
        Err(o) => return o,
    };
    let frames = match frame_arg(p, "--frames", &session, 300) {
        Ok(f) => f,
        Err(o) => return o,
    };
    let every = match p.number("--hash-every") {
        Ok(v) => v.filter(|n| *n > 0),
        Err(d) => return Outcome::usage(d),
    };
    let mut trace = Vec::new();
    let tracked = match p.value("--track") {
        None => None,
        Some(name) => match session.world.find(name) {
            Some(id) => Some((name.to_string(), id)),
            None => {
                let names: Vec<String> = session.world.entities.iter().filter_map(|e| e.name.clone()).collect();
                let hint = did_you_mean(name, names.iter().map(String::as_str))
                    .unwrap_or_else(|| format!("named entities: {}", names.join(", ")));
                return Outcome::failed(vec![
                    Diagnostic::new(&codes::UNKNOWN_ENTITY, format!("no entity named `{name}` to track")).hint(hint),
                ])
                .with_root(&loc.root);
            }
        },
    };
    // (cell, first frame, last frame): consecutive frames on the same cell.
    let mut spans: Vec<([i32; 2], u64, u64)> = Vec::new();
    let mut record = |session: &Session| {
        if let Some((_, id)) = &tracked
            && let Some(e) = session.world.get(*id)
        {
            let (cell, f) = (e.cell(), session.frame());
            match spans.last_mut() {
                Some(last) if last.0 == cell && last.2 + 1 == f => last.2 = f,
                _ => spans.push((cell, f, f)),
            }
        }
    };
    record(&session);
    while session.frame() < frames {
        session.step(&Default::default());
        record(&session);
        if let Some(n) = every
            && session.frame() % n == 0
        {
            trace.push(Json::object().with("frame", session.frame()).with("state_hash", session.hash_hex()));
        }
    }
    let mut o = Outcome::new(true).with_root(&loc.root);
    o.diagnostics = session.warnings.clone();
    o.data = session.summary();
    if every.is_some() {
        o.data.set("hash_trace", trace.clone());
    }
    let w = &session.world;
    o.line(format!(
        "ran {} for {} frames (seed {}{})",
        session.scene.path,
        w.frame,
        w.seed,
        session.playtest.as_ref().map(|t| format!(", input {}", t.path)).unwrap_or_default()
    ));
    o.line(format!("  state hash  {}", session.hash_hex()));
    o.line(format!("  entities    {} ({} enabled)", w.entities.len(), w.entities.iter().filter(|e| e.enabled).count()));
    let counts: Vec<String> = w.events.counts().iter().map(|(k, n)| format!("{} {n}", k.name())).collect();
    o.line(format!("  events      {}", if counts.is_empty() { "none".into() } else { counts.join(", ") }));
    for t in &trace {
        o.line(format!(
            "  frame {:>6}  {}",
            t.get("frame").and_then(Json::as_i64).unwrap_or(0),
            t.get("state_hash").and_then(Json::as_str).unwrap_or("")
        ));
    }
    if let Some((name, _)) = &tracked {
        let list: Vec<Json> = spans
            .iter()
            .map(|(c, a, b)| Json::object().with("cell", *c).with("from_frame", *a).with("to_frame", *b))
            .collect();
        o.data.set("track", Json::object().with("entity", name.as_str()).with("spans", list));
        o.line(format!("  track {name} (cell: frames, inclusive)"));
        for (c, a, b) in &spans {
            o.line(format!("    ({}, {}): {a}-{b}", c[0], c[1]));
        }
    }
    if p.has("--events") {
        let events: Vec<Json> = w.events.all().iter().map(|e| dump::event_json(w, e)).collect();
        o.data.set("events", events);
        for e in w.events.all() {
            o.line(format!("  [{:>5}] {:<9} {}", e.frame, e.kind.name(), e.message));
        }
    }
    o.settle()
}

pub fn shot(p: &Parsed, games: &GameFactory) -> Outcome {
    let Loaded { loc, mut session } = match load_session(p, games) {
        Ok(l) => l,
        Err(o) => return o,
    };
    let frame = match frame_arg(p, "--frame", &session, 0) {
        Ok(f) => f,
        Err(o) => return o,
    };
    let max_diff = match p.number("--max-diff") {
        Ok(v) => v.unwrap_or(0),
        Err(d) => return Outcome::usage(d),
    };
    session.run_to(frame);
    let (fb, report) = session.render();
    let stem = session.scene.path.rsplit('/').next().unwrap_or("scene").trim_end_matches(".toml").to_string();
    let out_path = match p.value("--out") {
        Some(o) => PathBuf::from(o),
        None => loc.root.join("out").join(format!("{stem}_f{frame}.png")),
    };
    let png = match image::encode_png(fb.width, fb.height, &fb.rgba) {
        Ok(b) => b,
        Err(e) => {
            return Outcome::failed(vec![Diagnostic::new(&codes::IO_ERROR, format!("PNG encoding failed: {e}"))]);
        }
    };
    tri!(write_file(&out_path, &png));

    let mut o = Outcome::new(true).with_root(&loc.root);
    o.diagnostics = session.warnings.clone();
    o.diagnostics.extend(report.issues.iter().cloned());
    o.data = Json::object()
        .with("path", display_path(&out_path))
        .with("frame", frame)
        .with("width", fb.width)
        .with("height", fb.height)
        .with("image_hash", hash::hex(fb.image_hash()))
        .with("state_hash", session.hash_hex())
        .with("scene", session.scene.path.as_str())
        .with("input", session.playtest.as_ref().map(|t| t.path.clone()))
        .with("report", report.to_json(&session.world));
    o.line(format!(
        "wrote {} ({}x{}, frame {frame}, image {})",
        display_path(&out_path),
        fb.width,
        fb.height,
        hash::hex(fb.image_hash())
    ));
    if p.has("--report") {
        o.text.push_str(&report.to_text());
    }
    if let Some(reference) = p.value("--compare") {
        let actual = Image { width: fb.width, height: fb.height, rgba: fb.rgba.clone() };
        let mut again: Vec<String> = std::env::args().skip(1).filter(|a| a != "--json").collect();
        if !again.iter().any(|a| a == "--report") {
            again.push("--report".into());
        }
        let hints = CompareHints {
            create: format!("cp {} {reference}", display_path(&out_path)),
            update: format!("cp {} {reference}", display_path(&out_path)),
            investigate: format!("slate {}", again.join(" ")),
        };
        let (cmp, diags) = compare_with(&actual, Path::new(reference), &diff_path(&out_path), max_diff, &hints);
        o.data.set("compare", cmp);
        o.diagnostics.extend(diags);
    }
    o.settle()
}

fn diff_path(out: &Path) -> PathBuf {
    let s = out.to_string_lossy();
    PathBuf::from(format!("{}.diff.png", s.trim_end_matches(".png")))
}

/// Exact commands to suggest in image diagnostics.
struct CompareHints {
    /// Creates the missing reference.
    create: String,
    /// Accepts the new image as the reference.
    update: String,
    /// Shows what is drawn where in the same frame.
    investigate: String,
}

/// Compare with a reference file. Returns JSON describing the result and
/// E0402/E0403 diagnostics on mismatch or missing reference.
fn compare_with(
    actual: &Image,
    reference: &Path,
    diff_out: &Path,
    max_diff: u64,
    hints: &CompareHints,
) -> (Json, Vec<Diagnostic>) {
    let shown = display_path(reference);
    let base = Json::object().with("reference", shown.as_str()).with("max_diff_pixels", max_diff);
    let bytes = match std::fs::read(reference) {
        Ok(b) => b,
        Err(_) => {
            let d = Diagnostic::new(&codes::REFERENCE_MISSING, format!("reference image `{shown}` not found"))
                .hint(format!("create it with `{}`, then look at the image before committing it", hints.create));
            return (base.with("status", "missing"), vec![d]);
        }
    };
    let reference_img = match image::decode_png(&bytes) {
        Ok(i) => i,
        Err(e) => {
            let d = Diagnostic::new(&codes::IO_ERROR, format!("cannot decode `{shown}`: {e}"));
            return (base.with("status", "unreadable"), vec![d]);
        }
    };
    let diff = image::diff(actual, &reference_img);
    let mut j = base
        .with("diff_pixels", diff.diff_pixels)
        .with("total_pixels", diff.total_pixels)
        .with("max_delta", diff.max_delta as u32)
        .with("same_size", diff.same_size);
    if diff.same_size && diff.diff_pixels <= max_diff {
        return (j.with("status", "match"), vec![]);
    }
    let mut d = if diff.same_size {
        Diagnostic::new(
            &codes::IMAGE_MISMATCH,
            format!("{} of {} pixels differ from `{shown}` (allowed: {max_diff})", diff.diff_pixels, diff.total_pixels),
        )
    } else {
        Diagnostic::new(
            &codes::IMAGE_MISMATCH,
            format!(
                "image is {}x{} but `{shown}` is {}x{}",
                actual.width, actual.height, reference_img.width, reference_img.height
            ),
        )
    };
    if let Some(vis) = &diff.image
        && let Ok(png) = image::encode_png(vis.width, vis.height, &vis.rgba)
        && write_file(diff_out, &png).is_ok()
    {
        j.set("diff_image", display_path(diff_out));
        d = d.note(format!("changed pixels are red in {}", display_path(diff_out)));
    }
    let d = d.hint(format!(
        "to see what is drawn where in this frame: `{}`; if the change is intended: `{}`",
        hints.investigate, hints.update
    ));
    (j.with("status", "mismatch"), vec![d])
}

pub fn inspect(p: &Parsed, games: &GameFactory) -> Outcome {
    let Loaded { loc, mut session } = match load_session(p, games) {
        Ok(l) => l,
        Err(o) => return o,
    };
    let frame = match frame_arg(p, "--frame", &session, 0) {
        Ok(f) => f,
        Err(o) => return o,
    };
    let since = match p.number("--since") {
        Ok(v) => v.unwrap_or(0),
        Err(d) => return Outcome::usage(d),
    };
    session.run_to(frame);
    if let Some(name) = p.value("--entity")
        && session.world.find(name).is_none()
    {
        let names: Vec<String> = session.world.entities.iter().filter_map(|e| e.name.clone()).collect();
        let hint = did_you_mean(name, names.iter().map(String::as_str))
            .unwrap_or_else(|| format!("named entities: {}", names.join(", ")));
        return Outcome::failed(vec![
            Diagnostic::new(&codes::UNKNOWN_ENTITY, format!("no entity named `{name}` in {}", session.scene.path))
                .hint(hint),
        ])
        .with_root(&loc.root);
    }
    if let Some(c) = p.value("--component") {
        let known = schema::COMPONENT_SPECS.iter().map(|s| s.id).chain(["props"]);
        if !known.clone().any(|k| k == c) {
            let hint = did_you_mean(c, known)
                .unwrap_or_else(|| "components: tile, block, sprite, mover, patrol, wander, trigger, label".into());
            return Outcome::usage(args::bad_arg(format!("unknown component `{c}`"), Some(hint)));
        }
    }
    let opts = InspectOptions {
        entity: p.value("--entity").map(str::to_string),
        component: p.value("--component").map(str::to_string),
        events: p.has("--events"),
        since,
    };
    let data = session.inspect(&opts);
    let mut o = Outcome::new(true).with_root(&loc.root);
    o.diagnostics = session.warnings.clone();
    o.line(format!(
        "frame {} of {} (seed {}, state {})",
        session.frame(),
        session.scene.path,
        session.world.seed,
        session.hash_hex()
    ));
    for e in data.get("entities").and_then(Json::as_array).unwrap_or(&[]) {
        let name = e.get("name").and_then(Json::as_str).map(str::to_string);
        let id = e.get("id").and_then(Json::as_i64).unwrap_or(0);
        let label = name.unwrap_or_else(|| format!("#{id}"));
        let comps: Vec<&str> = ["tile", "block", "sprite", "label", "mover", "patrol", "wander", "trigger"]
            .into_iter()
            .filter(|c| e.get(c).is_some())
            .collect();
        let at = match e.get("screen_pos") {
            Some(_) => "screen".to_string(),
            None => format!("cell {}", e.get("cell").map(Json::to_compact).unwrap_or_default()),
        };
        let enabled = if e.get("enabled").and_then(Json::as_bool) == Some(true) { "" } else { "  (disabled)" };
        let solid = if e.get("solid").and_then(Json::as_bool) == Some(true) { " solid" } else { "" };
        let screen = e.get("screen").map(Json::to_compact).unwrap_or_default();
        let tap = e.get("tap").filter(|t| !t.is_null()).map(|t| format!(" tap {}", t.to_compact())).unwrap_or_default();
        let mut state = String::new();
        if let Some(cd) = e.pointer("mover/cooldown").and_then(Json::as_i64).filter(|c| *c > 0) {
            state.push_str(&format!(" cooldown {cd}"));
        }
        if let Some(next) = e.pointer("patrol/next").filter(|n| !n.is_null()) {
            state.push_str(&format!(" next {}", next.to_compact()));
        }
        o.line(format!(
            "  {id:>3} {label:<12} {at:<13} {:<20}{solid}{state} screen {screen}{tap}{enabled}",
            comps.join("+")
        ));
        if let Some(t) = e.get("trigger").and_then(|t| t.get("occupied_by")).and_then(Json::as_array)
            && !t.is_empty()
        {
            o.line(format!("      trigger occupied by {}", Json::Arr(t.to_vec()).to_compact()));
        }
        if let Some(shown) = e.pointer("label/shown").and_then(Json::as_str) {
            let at = e.get("screen_pos").map(|p| format!(" at screen {}", p.to_compact())).unwrap_or_default();
            o.line(format!("      text {shown:?}{at}"));
        }
        if let Some(Json::Obj(props)) = e.get("props")
            && !props.is_empty()
        {
            let list: Vec<String> = props.iter().map(|(k, v)| format!("{k} = {}", v.to_compact())).collect();
            o.line(format!("      props {{ {} }}", list.join(", ")));
        }
    }
    if let Some(events) = data.get("events").and_then(Json::as_array) {
        o.line(format!("events ({}):", events.len()));
        for e in events {
            o.line(format!(
                "  [{:>5}] {:<9} {}",
                e.get("frame").and_then(Json::as_i64).unwrap_or(0),
                e.get("kind").and_then(Json::as_str).unwrap_or(""),
                e.get("message").and_then(Json::as_str).unwrap_or("")
            ));
        }
    }
    o.data = data;
    o.settle()
}

// ---------------------------------------------------------------------------
// test

pub fn test(p: &Parsed, games: &GameFactory) -> Outcome {
    let loc = tri!(locate(p.positional(0)));
    let filter = p.value("--filter");
    let mut paths: Vec<String> = match loc.playtest_arg() {
        Some(one) => vec![one],
        None => loc.vfs.paths_under("tests", ".toml").map(str::to_string).collect(),
    };
    if let Some(f) = filter {
        paths.retain(|path| path.contains(f));
    }
    let mut o = Outcome::new(true).with_root(&loc.root);
    if paths.is_empty() {
        o.diagnostics.push(Diagnostic::new(
            &codes::NO_TESTS,
            if filter.is_some() {
                "no playtests match the filter".to_string()
            } else {
                "no playtests in tests/".to_string()
            },
        ));
    }
    let update = p.has("--update-refs");
    let (mut passed, mut failed) = (0, 0);
    let mut results = Vec::new();
    for path in &paths {
        let run = match run_playtest(&loc.vfs, path, games()) {
            Ok(r) => r,
            Err(diags) => {
                failed += 1;
                o.line(format!("test {path} ... ERROR (could not load)"));
                results.push(Json::object().with("path", path.as_str()).with("ok", false).with("loaded", false));
                o.diagnostics.extend(diags);
                continue;
            }
        };
        let mut diags = run.failures.clone();
        let mut shots = Vec::new();
        let mut updated = Vec::new();
        for shot in &run.shots {
            // What the frame report noticed (hidden, off-screen...) is worth
            // a warning even when the pixels match the reference.
            for issue in &shot.report.issues {
                let mut d = issue.clone().in_file(&run.path);
                d.line = Some(shot.def.line);
                d.message = format!("at frame {}: {}", shot.frame_no, d.message);
                diags.push(d);
            }
            let reference = loc.abs(&shot.def.reference);
            let actual = Image { width: shot.frame.width, height: shot.frame.height, rgba: shot.frame.rgba.clone() };
            if update {
                let png = match image::encode_png(actual.width, actual.height, &actual.rgba) {
                    Ok(b) => b,
                    Err(e) => {
                        diags.push(Diagnostic::new(&codes::IO_ERROR, format!("PNG encoding failed: {e}")));
                        continue;
                    }
                };
                if let Err(d) = write_file(&reference, &png) {
                    diags.push(d);
                    continue;
                }
                updated.push(format!("  updated {} (frame {})", shot.def.reference, shot.frame_no));
                shots.push(
                    Json::object()
                        .with("reference", shot.def.reference.as_str())
                        .with("frame", shot.frame_no)
                        .with("status", "updated")
                        .with("image_hash", hash::hex(shot.frame.image_hash())),
                );
                continue;
            }
            let stem = format!("{}_f{}", run.name, shot.frame_no);
            let out_dir = loc.root.join("out").join("tests");
            let hints = CompareHints {
                create: format!("slate test --filter {} --update-refs", run.name),
                update: format!("slate test --filter {} --update-refs", run.name),
                investigate: format!("slate shot --input {} --frame {} --report", run.path, shot.frame_no),
            };
            let diff_out = out_dir.join(format!("{stem}.diff.png"));
            let (cmp, cdiags) = compare_with(&actual, &reference, &diff_out, shot.def.max_diff_pixels, &hints);
            let mut cmp = cmp.with("frame", shot.frame_no).with("image_hash", hash::hex(shot.frame.image_hash()));
            if !cdiags.is_empty() {
                let actual_path = out_dir.join(format!("{stem}.png"));
                if let Ok(png) = image::encode_png(actual.width, actual.height, &actual.rgba)
                    && write_file(&actual_path, &png).is_ok()
                {
                    cmp.set("actual_image", display_path(&actual_path));
                }
            }
            // Locate shot diagnostics in the playtest file.
            for d in cdiags {
                let mut d = d.in_file(&run.path);
                d.line = Some(shot.def.line);
                diags.push(d);
            }
            shots.push(cmp);
        }
        let ok = !diags.iter().any(Diagnostic::is_error);
        if ok {
            passed += 1;
        } else {
            failed += 1;
        }
        o.line(format!(
            "test {} ... {} ({} expect(s), {} shot(s), {} frames, final state {})",
            run.name,
            if ok { "ok" } else { "FAILED" },
            run.expects_checked,
            run.shots.len(),
            run.frames,
            run.state_hash
        ));
        for line in updated {
            o.line(line);
        }
        results.push(
            Json::object()
                .with("name", run.name.as_str())
                .with("path", run.path.as_str())
                .with("ok", ok)
                .with("scene", run.scene.as_str())
                .with("frames", run.frames)
                .with("seed", run.seed)
                .with("state_hash", run.state_hash.as_str())
                .with("expects", run.expects_checked)
                .with("shots", shots),
        );
        o.diagnostics.extend(run.warnings.iter().filter(|w| !w.is_error()).cloned());
        o.diagnostics.extend(diags);
    }
    o.diagnostics.dedup();
    o.line(format!(
        "\ntest result: {}. {passed} passed; {failed} failed{}",
        if failed == 0 { "ok" } else { "FAILED" },
        if update { "; reference images updated" } else { "" }
    ));
    o.data = Json::object().with("passed", passed).with("failed", failed).with("tests", results);
    o.ok = failed == 0;
    o.exit = if o.ok { 0 } else { 1 };
    o.settle()
}

// ---------------------------------------------------------------------------
// explain / schema / docs / help / version

pub fn explain(p: &Parsed) -> Outcome {
    let Some(topic) = p.positional(0) else {
        return explain_topics();
    };
    let mut o = Outcome::new(true);
    if topic == "limits" || topic == "limit" {
        o.data = Json::object().with("topic", "limits").with("kind", "limits").with("text", docs::LIMITS_MD);
        o.text = format!("# Limits in slate {}\n\n{}", slate_core::ENGINE_VERSION, docs::LIMITS_MD);
        return o;
    }
    if topic == "events" || topic == "event" {
        let text = slate_sim::events_markdown();
        let kinds: Vec<Json> = slate_sim::EVENT_DOCS
            .iter()
            .map(|d| {
                Json::object()
                    .with("kind", d.kind.name())
                    .with("subject", d.subject)
                    .with("other", d.other)
                    .with("fields", d.fields)
                    .with("meaning", d.meaning)
            })
            .collect();
        o.data = Json::object().with("topic", "events").with("kind", "events").with("events", kinds);
        o.text = format!("# Events\n\n{text}");
        return o;
    }
    if let Some(c) = codes::lookup(topic) {
        o.data = Json::object()
            .with("topic", c.code)
            .with("kind", "code")
            .with("slug", c.slug)
            .with("severity", c.severity.as_str())
            .with("title", c.title)
            .with("text", c.explain);
        o.line(format!("{} ({}, {}): {}\n\n{}", c.code, c.slug, c.severity.as_str(), c.title, c.explain));
        return o;
    }
    if let Some(spec) = schema::find_spec(topic) {
        let text = schema::spec_markdown(spec, "#");
        let fields: Vec<Json> = spec
            .fields
            .iter()
            .map(|f| {
                Json::object()
                    .with("key", f.name)
                    .with("type", f.ty.describe())
                    .with("required", f.required)
                    .with("default", f.default)
                    .with("doc", f.doc)
            })
            .collect();
        o.data = Json::object()
            .with("topic", spec.id)
            .with("kind", "format")
            .with("title", spec.title)
            .with("doc", spec.doc)
            .with("fields", fields)
            .with("text", text.as_str());
        o.text = text;
        return o;
    }
    if let Some(c) = args::find(topic) {
        o.data = Json::object().with("topic", c.name).with("kind", "command").with("command", args::command_json(c));
        o.text = args::command_text(c);
        return o;
    }
    let candidates: Vec<&str> = codes::ALL
        .iter()
        .map(|c| c.slug)
        .chain(schema::ALL_SPECS.iter().map(|s| s.id))
        .chain(COMMANDS.iter().map(|c| c.name))
        .collect();
    let hint = did_you_mean(topic, candidates)
        .unwrap_or_else(|| "try a code like E0201, a component like sprite, or a command like shot".into());
    Outcome::usage(args::bad_arg(format!("nothing to explain for `{topic}`"), Some(hint)))
}

/// `slate explain` with no topic: everything that can be explained.
fn explain_topics() -> Outcome {
    let mut o = Outcome::new(true);
    o.line("slate explain <TOPIC> — topics:\n\n  limits             what this version cannot do, and workarounds\n  events             event kinds: subject, other party, fields\n\nFile formats and components:");
    for s in schema::ALL_SPECS {
        o.line(format!("  {:<18} {}", s.id, s.title));
    }
    o.line("\nDiagnostic codes:");
    for c in codes::ALL {
        o.line(format!("  {}  {:<22} {}", c.code, c.slug, c.title));
    }
    o.line("\nCommands:");
    for c in COMMANDS {
        o.line(format!("  {:<18} {}", c.name, c.summary));
    }
    o.data = Json::object()
        .with(
            "formats",
            schema::ALL_SPECS
                .iter()
                .map(|s| Json::object().with("topic", s.id).with("title", s.title))
                .collect::<Vec<_>>(),
        )
        .with(
            "codes",
            codes::ALL
                .iter()
                .map(|c| Json::object().with("topic", c.code).with("slug", c.slug).with("title", c.title))
                .collect::<Vec<_>>(),
        )
        .with("commands", COMMANDS.iter().map(|c| Json::from(c.name)).collect::<Vec<_>>());
    o
}

pub fn schema_cmd(p: &Parsed) -> Outcome {
    let kinds: Vec<&str> = schema::FILE_SPECS.iter().map(|s| s.id).collect();
    if let Some(dir) = p.value("--out") {
        let mut o = Outcome::new(true);
        let mut written = Vec::new();
        for s in schema::FILE_SPECS {
            let path = Path::new(dir).join(format!("{}.schema.json", s.id));
            tri!(write_file(&path, (schema::json_schema(s).to_pretty() + "\n").as_bytes()));
            o.line(format!("wrote {}", display_path(&path)));
            written.push(Json::from(display_path(&path)));
        }
        o.data = Json::object().with("written", written);
        return o;
    }
    let Some(kind) = p.positional(0) else {
        let mut o = Outcome::new(true);
        o.data = Json::object().with("kinds", kinds.iter().map(|k| Json::from(*k)).collect::<Vec<_>>());
        o.line(format!("schemas: {}  (slate schema <kind>, or --out DIR for all)", kinds.join(", ")));
        return o;
    };
    match schema::FILE_SPECS.iter().find(|s| s.id == kind) {
        Some(s) => {
            let mut o = Outcome::new(true);
            let j = schema::json_schema(s);
            o.text = j.to_pretty() + "\n";
            o.data = Json::object().with("kind", kind).with("schema", j);
            o
        }
        None => Outcome::usage(args::bad_arg(
            format!("unknown schema `{kind}`"),
            Some(did_you_mean(kind, kinds.iter().copied()).unwrap_or_else(|| format!("kinds: {}", kinds.join(", ")))),
        )),
    }
}

/// Files of the generated reference: (relative path, contents).
pub fn reference_files() -> Vec<(String, String)> {
    let mut files = vec![
        ("cli.md".to_string(), docs::cli_markdown()),
        ("formats.md".to_string(), docs::formats_markdown()),
        ("errors.md".to_string(), docs::errors_markdown()),
    ];
    for s in schema::FILE_SPECS {
        files.push((format!("schemas/{}.schema.json", s.id), schema::json_schema(s).to_pretty() + "\n"));
    }
    files
}

pub fn docs_cmd(p: &Parsed) -> Outcome {
    let mut o = Outcome::new(true);
    if p.has("--agents") {
        let text = docs::project_agents_md("my-game");
        o.data = Json::object().with("agents_md", text.as_str());
        o.text = text;
        return o;
    }
    if let Some(dir) = p.value("--out") {
        let mut written = Vec::new();
        for (rel, text) in reference_files() {
            let path = Path::new(dir).join(&rel);
            tri!(write_file(&path, text.as_bytes()));
            written.push(Json::from(display_path(&path)));
            o.line(format!("wrote {}", display_path(&path)));
        }
        o.data = Json::object().with("written", written);
        return o;
    }
    let text = format!("{}\n{}\n{}", docs::cli_markdown(), docs::formats_markdown(), docs::errors_markdown());
    o.data = Json::object().with("markdown", text.as_str());
    o.text = text;
    o
}

pub fn help(p: &Parsed) -> Outcome {
    let mut o = Outcome::new(true);
    match p.positional(0) {
        None => {
            o.text = args::overview_text();
            o.data = Json::object()
                .with("version", slate_core::ENGINE_VERSION)
                .with("commands", COMMANDS.iter().map(args::command_json).collect::<Vec<_>>());
        }
        Some(name) => match args::find(name) {
            Some(c) => {
                o.text = args::command_text(c);
                o.data = Json::object().with("command", args::command_json(c));
            }
            None => {
                let hint = did_you_mean(name, COMMANDS.iter().map(|c| c.name));
                return Outcome::usage(args::bad_arg(format!("unknown command `{name}`"), hint));
            }
        },
    }
    o
}

pub fn version() -> Outcome {
    let mut o = Outcome::new(true);
    o.line(format!("slate {}", slate_core::ENGINE_VERSION));
    o.data = Json::object()
        .with("version", slate_core::ENGINE_VERSION)
        .with("json_format", slate_core::JSON_FORMAT_VERSION)
        .with("diagnostic_codes", codes::ALL.len());
    o
}
