//! `rwgpu export web`: a project as a folder that any static web host can
//! serve (ADR 0025). The folder holds the player's files and the game's, the
//! same files `rwgpu serve` serves, written once.
//!
//! Safe to run: the project must pass `rwgpu check`; the folder must be new
//! or empty unless `--force`; nothing is ever deleted; only the export's own
//! file names are written, never through a link; the folder may not be the
//! project or hold it. Deterministic: the same project and player give the
//! same bytes, with no time and no path of this machine in them.

use std::path::{Path, PathBuf};

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_core::json::Json;
use rwgpuengine_data::check_project;

use crate::args::{Parsed, bad_arg};
use crate::out::{Outcome, display_path, normalize};
use crate::project::{io_err, locate};
use crate::web::{self, Player};

/// The port `rwgpu serve` uses by default, for the "try it" line.
const SERVE_URL: &str = "http://127.0.0.1:4410/";

/// One file of the exported folder.
#[derive(Debug)]
pub struct OutFile {
    pub name: &'static str,
    pub bytes: Vec<u8>,
    /// `player` or `game`.
    pub part: &'static str,
    pub what: &'static str,
    /// Where a module was read from.
    pub from: Option<PathBuf>,
}

/// The files of the folder, in the order they are written: the player's
/// (`Player::files`), then the game's.
pub fn folder_files(player: &Player, game: &web::Game) -> std::io::Result<Vec<OutFile>> {
    let mut files = Vec::new();
    for f in player.files() {
        files.push(OutFile {
            name: f.name,
            bytes: f.bytes()?,
            part: "player",
            what: web::what(player.kind, f.name),
            from: f.path().map(Path::to_path_buf),
        });
    }
    files.push(OutFile {
        name: web::PROJECT_JSON,
        bytes: game.project_json.to_compact().into_bytes(),
        part: "game",
        what: "the game's name, main scene, scenes and playtests, for the page's lists",
        from: None,
    });
    files.push(OutFile {
        name: web::BUNDLE,
        bytes: game.bundle().into_bytes(),
        part: "game",
        what: "the game's files as one text bundle",
        from: None,
    });
    Ok(files)
}

pub fn export(p: &Parsed) -> Outcome {
    match p.positional(0) {
        Some("web") => {}
        Some(other) => {
            return Outcome::usage(bad_arg(
                format!("`rwgpu export` has no target `{other}`"),
                Some("the one target is `web`: rwgpu export web [PATH] --out DIR".into()),
            ));
        }
        None => {
            return Outcome::usage(bad_arg(
                "`rwgpu export` needs a target".into(),
                Some("rwgpu export web --out out/web".into()),
            ));
        }
    }
    let Some(out_arg) = p.value("--out") else {
        return Outcome::usage(bad_arg(
            "`rwgpu export web` needs --out DIR, the folder to write".into(),
            Some("for example `rwgpu export web --out out/web`".into()),
        ));
    };
    let loc = match locate(p.positional(1)) {
        Ok(l) => l,
        Err(d) => return Outcome::failed(vec![d]),
    };
    // Validated first, with the checker's own diagnostics.
    let report = check_project(&loc.vfs);
    if !report.ok() {
        let mut o = Outcome::failed(report.diagnostics).with_root(&loc.root);
        o.line("export refused: the project does not pass `rwgpu check`");
        return o;
    }
    let player = match web::find_player(p) {
        Ok(f) => f,
        Err(d) if d.code == codes::BAD_ARGUMENT.code => return Outcome::usage(d),
        Err(d) => return Outcome::failed(vec![d]).with_root(&loc.root),
    };
    let game = web::game(&loc.vfs);
    let files = match folder_files(&player, &game) {
        Ok(f) => f,
        Err(e) => return Outcome::failed(vec![io_err("the web player", &e)]).with_root(&loc.root),
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let out = normalize(&cwd.join(out_arg));
    let others = match prepare(&out, out_arg, &loc.root, &files, p.has("--force")) {
        Ok(o) => o,
        Err(o) => return o.with_root(&loc.root),
    };
    for f in &files {
        if let Err(e) = std::fs::write(out.join(f.name), &f.bytes) {
            return Outcome::failed(vec![io_err(&display_path(&out.join(f.name)), &e)]).with_root(&loc.root);
        }
    }
    let mut o = Outcome::new(true).with_root(&loc.root);
    o.diagnostics = report.diagnostics;
    if !others.is_empty() {
        let shown: Vec<&str> = others.iter().take(5).map(String::as_str).collect();
        let more = others.len() - shown.len();
        o.diagnostics.push(
            Diagnostic::new(
                &codes::EXPORT_EXTRA_FILES,
                format!(
                    "{} holds {} file(s) the export did not write: {}{}",
                    display_path(&out),
                    others.len(),
                    shown.join(", "),
                    if more > 0 { format!(" and {more} more") } else { String::new() }
                ),
            )
            .hint("a host that publishes this folder publishes them too; export into an empty folder to be sure"),
        );
    }
    let left_out = left_out(&loc.root, &game, &out);
    report_into(&mut o, &out, &player, &files, &game, &left_out, &others);
    o.settle()
}

/// Make sure the folder can take the export, and create it. Returns the
/// names already there that the export does not write.
fn prepare(out: &Path, out_arg: &str, root: &Path, files: &[OutFile], force: bool) -> Result<Vec<String>, Outcome> {
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let (out_real, root_real) = (real(out), real(root));
    if root_real.starts_with(&out_real) {
        return Err(Outcome::usage(bad_arg(
            format!("--out `{out_arg}` is the project folder or holds it: a host would publish the whole project"),
            Some("export into a folder of its own, such as `--out out/web`".into()),
        )));
    }
    if out.exists() && !out.is_dir() {
        return Err(Outcome::failed(vec![
            Diagnostic::new(&codes::ALREADY_EXISTS, format!("--out `{out_arg}` is a file, not a folder"))
                .hint("choose a new folder name"),
        ]));
    }
    let mut others = Vec::new();
    if out.is_dir() {
        let mut names: Vec<String> = std::fs::read_dir(out)
            .map_err(|e| Outcome::failed(vec![io_err(out_arg, &e)]))?
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        if !names.is_empty() && !force {
            return Err(Outcome::failed(vec![
                Diagnostic::new(
                    &codes::ALREADY_EXISTS,
                    format!("--out `{out_arg}` is not empty ({} entries)", names.len()),
                )
                .hint(
                    "choose a new or empty folder, or pass --force to write the export's files into it (nothing \
                         else there is changed or deleted)",
                ),
            ]));
        }
        for name in names {
            let path = out.join(&name);
            if !files.iter().any(|f| f.name == name) {
                others.push(name);
                continue;
            }
            let meta = std::fs::symlink_metadata(&path).map_err(|e| Outcome::failed(vec![io_err(&name, &e)]))?;
            if meta.file_type().is_symlink() || meta.is_dir() {
                let kind = if meta.is_dir() { "a folder" } else { "a link" };
                return Err(Outcome::failed(vec![
                    Diagnostic::new(&codes::ALREADY_EXISTS, format!("`{}` is {kind}", display_path(&path)))
                        .hint("rwgpu export writes only plain files, never through a link: remove it or choose an empty folder"),
                ]));
            }
        }
    }
    std::fs::create_dir_all(out).map_err(|e| Outcome::failed(vec![io_err(out_arg, &e)]))?;
    Ok(others)
}

/// The project's files that do not go to the web, and why, found by a walk
/// like `Vfs::from_dir`'s (hidden folders, `target/`, `out/` and the export
/// folder are not the game's).
pub fn left_out(root: &Path, game: &web::Game, out: &Path) -> Vec<(String, &'static str)> {
    let mut all = Vec::new();
    walk(root, root, out, &mut all);
    all.sort();
    all.into_iter()
        .filter(|p| !game.vfs.contains(p))
        .map(|p| {
            let why = if game.unread.contains(&p) {
                "a .toml file the engine does not read"
            } else if p.starts_with("tests/refs/") {
                "a playtest's reference picture: only `rwgpu test` reads it"
            } else if p.ends_with(".md") {
                "notes for people and agents"
            } else {
                "not a game file: the engine reads only rwgpu.toml, scenes/, prefabs/ and tests/"
            };
            (p, why)
        })
        .collect()
}

fn walk(root: &Path, dir: &Path, skip: &Path, all: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name.starts_with('.') || ["target", "out", "node_modules"].contains(&name.as_str()) || path == skip {
                continue;
            }
            walk(root, &path, skip, all);
        } else if !name.starts_with('.') {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            all.push(rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"));
        }
    }
}

fn report_into(
    o: &mut Outcome,
    out: &Path,
    player: &Player,
    files: &[OutFile],
    game: &web::Game,
    left_out: &[(String, &'static str)],
    others: &[String],
) {
    let bytes = |part: &str| files.iter().filter(|f| f.part == part).map(|f| f.bytes.len()).sum::<usize>();
    let (player_bytes, game_bytes) = (bytes("player"), bytes("game"));
    let shown = display_path(out);
    let try_it = format!("rwgpu serve --site {shown}");
    let name = game.project_json.get("name").and_then(Json::as_str).unwrap_or("?").to_string();
    o.data = Json::object()
        .with("target", "web")
        .with("out", shown.as_str())
        .with("player", player.kind.name())
        .with("player_from", display_path(&player.place))
        .with(
            "files",
            files
                .iter()
                .map(|f| {
                    Json::object()
                        .with("path", f.name)
                        .with("bytes", f.bytes.len())
                        .with("hash", web::file_hash(&f.bytes))
                        .with("part", f.part)
                        .with("what", f.what)
                        .with("from", f.from.as_deref().map(display_path))
                })
                .collect::<Vec<_>>(),
        )
        .with(
            "bundle",
            game.why
                .iter()
                .map(|(p, why)| {
                    let text = game.vfs.get(p).unwrap_or_default();
                    Json::object()
                        .with("path", p.as_str())
                        .with("bytes", text.len())
                        .with("hash", web::file_hash(text.as_bytes()))
                        .with("why", *why)
                })
                .collect::<Vec<_>>(),
        )
        .with(
            "left_out",
            left_out
                .iter()
                .map(|(p, why)| Json::object().with("path", p.as_str()).with("why", *why))
                .collect::<Vec<_>>(),
        )
        .with("other_files", others.iter().map(|s| Json::from(s.as_str())).collect::<Vec<_>>())
        .with("bytes", player_bytes + game_bytes)
        .with("player_bytes", player_bytes)
        .with("game_bytes", game_bytes)
        .with("try", try_it.as_str())
        .with("url", SERVE_URL)
        .with(
            "host",
            "any static web host: every address in the folder is relative, so it works at a site's root or under \
             a path. Serve .wasm as application/wasm for streaming compilation (without it the page compiles the \
             module from its bytes). Opened as a file (file://) it does not load: browsers refuse fetch there. \
             WebGPU needs a secure context: https, or http on localhost; elsewhere the page draws with the CPU.",
        );
    o.line(format!(
        "exported `{name}` to {shown}: {} files, {} bytes (player {player_bytes}, game {game_bytes})",
        files.len(),
        player_bytes + game_bytes
    ));
    o.line(format!("player: {}", player.describe()));
    for f in files {
        o.line(format!("  {:<17} {:>9}  {}", f.name, f.bytes.len(), f.what));
    }
    o.line(format!(
        "the bundle holds {} project file(s); {} left out (`--json` lists them and why)",
        game.why.len(),
        left_out.len()
    ));
    o.line(format!("try it: {try_it}   then open {SERVE_URL}"));
    o.line("publish: copy the folder to any static web host; every address in it is relative");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::{CPU_MODULE, CPU_PLAYER_FILES, WEB_PLAYER_FILES};

    /// A scratch folder of this test run.
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rwgpu-export-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn example(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(name).canonicalize().unwrap()
    }

    /// Run the command as `main_with` does, from a parsed line.
    fn run(line: &[&str]) -> Outcome {
        let args: Vec<String> = line.iter().map(|s| s.to_string()).collect();
        export(&crate::args::parse(&args).unwrap())
    }

    fn fake_module(dir: &Path) -> PathBuf {
        let m = dir.join(CPU_MODULE);
        std::fs::write(&m, b"\0asm\x01\0\0\0 a stand-in module").unwrap();
        m
    }

    fn folder(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut v: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap())
            .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap()))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn the_same_project_gives_the_same_bytes_and_no_path_of_this_machine() {
        let base = scratch("same");
        let module = fake_module(&base);
        let town = example("tiny_town");
        let (a, b) = (base.join("a"), base.join("deeper/b"));
        for out in [&a, &b] {
            let o = run(&[
                "export",
                "web",
                town.to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
                "--wasm",
                module.to_str().unwrap(),
            ]);
            assert!(o.ok, "{:?}", o.diagnostics);
        }
        let (fa, fb) = (folder(&a), folder(&b));
        assert_eq!(fa, fb);
        let names: Vec<&str> = fa.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["bundle.txt", "index.html", "player.js", "player.wasm", "project.json"]);
        let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
        for (name, bytes) in &fa {
            let text = String::from_utf8_lossy(bytes);
            for path in [town.to_string_lossy().into_owned(), base.to_string_lossy().into_owned(), home.clone()] {
                assert!(path.len() < 2 || !text.contains(&path), "{name} holds `{path}`");
            }
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_bundle_of_a_fixed_project_is_the_same_everywhere() {
        // Written in an order a file system may list differently; the bundle is
        // sorted by path and holds no time, so its hash is fixed.
        let base = scratch("fixed");
        let module = fake_module(&base);
        let project = base.join("game");
        for (rel, text) in [
            ("tests/t.toml", "[test]\nframes = 2\n"),
            ("scenes/main.toml", "[scene]\nname = \"M\"\n\n[ground]\nsize = [2, 2]\n"),
            ("rwgpu.toml", "[project]\nname = \"Fixed\"\nmain_scene = \"scenes/main.toml\"\n"),
            ("notes.md", "not published\n"),
        ] {
            crate::project::write_file(&project.join(rel), text.as_bytes()).unwrap();
        }
        let out = base.join("web");
        let o = run(&[
            "export",
            "web",
            project.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--wasm",
            module.to_str().unwrap(),
        ]);
        assert!(o.ok, "{:?}", o.diagnostics);
        let bundle = std::fs::read(out.join("bundle.txt")).unwrap();
        assert_eq!(web::file_hash(&bundle), "fd6cd2455a1b4482", "{}", String::from_utf8_lossy(&bundle));
        let project_json = std::fs::read_to_string(out.join("project.json")).unwrap();
        assert_eq!(
            project_json,
            format!(
                "{{\"name\":\"Fixed\",\"main_scene\":\"scenes/main.toml\",\"scenes\":[\"scenes/main.toml\"],\"tests\":[\"tests/t.toml\"],\"version\":\"{}\"}}",
                rwgpuengine_core::ENGINE_VERSION
            )
        );
        let left: Vec<&str> = o
            .data
            .get("left_out")
            .and_then(Json::as_array)
            .unwrap()
            .iter()
            .filter_map(|j| j.get("path")?.as_str())
            .collect();
        assert_eq!(left, ["notes.md"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn it_refuses_what_could_lose_or_publish_something() {
        let base = scratch("refuse");
        let module = fake_module(&base);
        let wasm = module.to_str().unwrap();
        let vault = example("coin_vault");
        let vault_s = vault.to_str().unwrap();
        let code = |o: &Outcome| o.diagnostics.first().map(|d| d.code).unwrap_or("none");

        // A folder that is not empty, without --force.
        let busy = base.join("busy");
        std::fs::create_dir_all(&busy).unwrap();
        std::fs::write(busy.join("keep.txt"), "mine").unwrap();
        let o = run(&["export", "web", vault_s, "--out", busy.to_str().unwrap(), "--wasm", wasm]);
        assert_eq!((o.ok, code(&o)), (false, "E0005"));
        assert_eq!(folder(&busy).len(), 1, "nothing was written");

        // With --force: written beside it, nothing deleted, a warning names it.
        let o = run(&["export", "web", vault_s, "--out", busy.to_str().unwrap(), "--wasm", wasm, "--force"]);
        assert!(o.ok);
        assert!(o.diagnostics.iter().any(|d| d.code == "W0001"), "{:?}", o.diagnostics);
        assert_eq!(std::fs::read_to_string(busy.join("keep.txt")).unwrap(), "mine");
        assert_eq!(folder(&busy).len(), 1 + CPU_PLAYER_FILES.len() + 2);

        // The project folder itself, or a folder holding it.
        for out in [vault.clone(), vault.parent().unwrap().to_path_buf()] {
            let o = run(&["export", "web", vault_s, "--out", out.to_str().unwrap(), "--wasm", wasm, "--force"]);
            assert_eq!((o.exit, code(&o)), (crate::out::EXIT_USAGE, "E0003"));
        }

        // A link where the export writes: never written through.
        #[cfg(unix)]
        {
            let linked = base.join("linked");
            std::fs::create_dir_all(&linked).unwrap();
            std::os::unix::fs::symlink(base.join("elsewhere.js"), linked.join("player.js")).unwrap();
            let o = run(&["export", "web", vault_s, "--out", linked.to_str().unwrap(), "--wasm", wasm, "--force"]);
            assert_eq!((o.ok, code(&o)), (false, "E0005"));
            assert!(!base.join("elsewhere.js").exists());
        }

        // A project that does not pass `rwgpu check`: the checker's diagnostics.
        let broken = base.join("broken");
        crate::project::write_file(&broken.join("rwgpu.toml"), b"[project]\nname = 3\n").unwrap();
        let o = run(&[
            "export",
            "web",
            broken.to_str().unwrap(),
            "--out",
            base.join("b").to_str().unwrap(),
            "--wasm",
            wasm,
        ]);
        assert!(!o.ok);
        let checked = check_project(&rwgpuengine_data::Vfs::from_dir(&broken).unwrap());
        assert_eq!(
            o.diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
            checked.diagnostics.iter().map(|d| d.code).collect::<Vec<_>>()
        );
        assert!(!base.join("b").exists());

        // No player module: E0008, with the way to get it.
        let o = run(&[
            "export",
            "web",
            vault_s,
            "--out",
            base.join("c").to_str().unwrap(),
            "--wasm",
            base.join("none.wasm").to_str().unwrap(),
        ]);
        assert_eq!((o.ok, code(&o)), (false, "E0008"));
        assert!(o.diagnostics[0].hint.as_deref().unwrap().contains("cargo xtask dist"));

        // Two players named at once.
        let o = run(&[
            "export",
            "web",
            vault_s,
            "--out",
            base.join("d").to_str().unwrap(),
            "--wasm",
            wasm,
            "--web-player",
            "x",
        ]);
        assert_eq!((o.exit, code(&o)), (crate::out::EXIT_USAGE, "E0003"));
        assert!(!base.join("d").exists());

        // No target, a wrong target, no --out.
        for line in [&["export"][..], &["export", "native", "--out", "x"], &["export", "web"]] {
            assert_eq!(run(line).exit, crate::out::EXIT_USAGE, "{line:?}");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_folder_lists_what_it_holds_and_why() {
        let base = scratch("list");
        let module = fake_module(&base);
        let out = base.join("web");
        let town = example("tiny_town");
        let o = run(&[
            "export",
            "web",
            town.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--wasm",
            module.to_str().unwrap(),
        ]);
        let files = o.data.get("files").and_then(Json::as_array).unwrap();
        let sum: i64 = files.iter().map(|f| f.get("bytes").and_then(Json::as_i64).unwrap()).sum();
        assert_eq!(o.data.get("bytes").and_then(Json::as_i64), Some(sum));
        for f in files {
            let name = f.get("path").and_then(Json::as_str).unwrap();
            let bytes = std::fs::read(out.join(name)).unwrap();
            assert_eq!(f.get("hash").and_then(Json::as_str), Some(web::file_hash(&bytes).as_str()));
        }
        let bundle: Vec<&str> = o
            .data
            .get("bundle")
            .and_then(Json::as_array)
            .unwrap()
            .iter()
            .filter_map(|j| j.get("path")?.as_str())
            .collect();
        assert!(bundle.contains(&"rwgpu.toml") && bundle.contains(&"tests/build_town.toml"));
        let left: Vec<&str> = o
            .data
            .get("left_out")
            .and_then(Json::as_array)
            .unwrap()
            .iter()
            .filter_map(|j| j.get("path")?.as_str())
            .collect();
        assert!(left.contains(&"tests/refs/build_town.png"), "{left:?}");
        assert!(left.iter().all(|p| !p.ends_with(".toml")), "{left:?}");
        let _ = std::fs::remove_dir_all(&base);
    }
    #[test]
    fn the_webgpu_player_is_exported_as_its_folder_holds_it() {
        let base = scratch("webgpu");
        let player = base.join("web-player");
        std::fs::create_dir_all(&player).unwrap();
        for f in WEB_PLAYER_FILES {
            std::fs::write(player.join(f), format!("<{f}>")).unwrap();
        }
        std::fs::write(player.join("check.mjs"), "not the player's").unwrap();
        let out = base.join("web");
        let vault = example("coin_vault");
        let o = run(&[
            "export",
            "web",
            vault.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--web-player",
            player.to_str().unwrap(),
            "--json",
        ]);
        assert!(o.ok, "{:?}", o.diagnostics);
        assert_eq!(o.data.get("player").and_then(Json::as_str), Some("webgpu"));
        let names: Vec<String> = folder(&out).into_iter().map(|(n, _)| n).collect();
        assert_eq!(
            names,
            ["bundle.txt", "index.html", "player.js", "project.json", "rwgpu_web.js", "rwgpu_web_bg.wasm"]
        );
        for f in WEB_PLAYER_FILES {
            assert_eq!(std::fs::read_to_string(out.join(f)).unwrap(), format!("<{f}>"), "copied as it is");
        }
        let parts: Vec<(&str, &str)> = o
            .data
            .get("files")
            .and_then(Json::as_array)
            .unwrap()
            .iter()
            .map(|f| (f.get("path").and_then(Json::as_str).unwrap(), f.get("part").and_then(Json::as_str).unwrap()))
            .collect();
        assert_eq!(&parts[..4], WEB_PLAYER_FILES.map(|f| (f, "player")));
        assert_eq!(&parts[4..], [("project.json", "game"), ("bundle.txt", "game")]);

        // A folder that misses a file is no player: E0008, and nothing is written.
        std::fs::remove_file(player.join("rwgpu_web.js")).unwrap();
        let o = run(&[
            "export",
            "web",
            vault.to_str().unwrap(),
            "--out",
            base.join("none").to_str().unwrap(),
            "--web-player",
            player.to_str().unwrap(),
        ]);
        assert_eq!((o.ok, o.diagnostics[0].code), (false, "E0008"));
        assert!(!base.join("none").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
