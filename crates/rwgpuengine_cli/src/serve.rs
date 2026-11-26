//! `rwgpu serve`: a tiny single-threaded HTTP server (std only) for the web
//! player. It serves the page, the wasm module and the project as a bundle
//! that is rebuilt from disk on every request, so data edits show up in
//! the browser without any build step.
//!
//! Two players (ADR 0024). The WebGPU player is a folder of built files
//! (`cargo xtask web-player`) that `rwgpu` finds and serves as they are; it
//! draws with WebGPU where the browser has it and with the CPU reference
//! renderer where not. Without it, the CPU player of ADR 0008 is served: the
//! page below and `rwgpuengine_web.wasm`. `rwgpu` links nothing of the
//! players' half either way.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_core::hash;
use rwgpuengine_core::json::Json;
use rwgpuengine_data::{Vfs, load_project};

use crate::args::Parsed;
use crate::out::{Outcome, display_path};
use crate::project::locate;

const INDEX_HTML: &str = include_str!("../../../web/index.html");
const PLAYER_JS: &str = include_str!("../../../web/player.js");

/// Where `cargo xtask web` puts the module, for running from source
/// (`cargo rwgpu serve ...`). Debug builds only: a release binary must not
/// carry a path of the machine that built it.
#[cfg(debug_assertions)]
const SOURCE_WASM: Option<&str> =
    Some(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/wasm32-unknown-unknown/web/rwgpuengine_web.wasm"));
#[cfg(not(debug_assertions))]
const SOURCE_WASM: Option<&str> = None;

/// The WebGPU player's files, as `cargo xtask web-player` writes them. The
/// page is served at `/`; nothing else in the folder is served.
pub const WEB_PLAYER_FILES: [&str; 4] = ["index.html", "player.js", "rwgpu_web.js", "rwgpu_web_bg.wasm"];

/// The folder a package puts beside the `rwgpu` program.
pub const WEB_PLAYER_DIR: &str = "web-player";

/// Where `cargo xtask web-player` builds it in the workspace, for running from
/// source. Debug builds only, like [`SOURCE_WASM`].
#[cfg(debug_assertions)]
const SOURCE_WEB_PLAYER: Option<&str> = Some(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/web-player"));
#[cfg(not(debug_assertions))]
const SOURCE_WEB_PLAYER: Option<&str> = None;

/// Which player `serve` serves.
#[derive(Debug, Clone, PartialEq)]
pub enum Player {
    /// The WebGPU player's folder.
    Gpu(PathBuf),
    /// The CPU player's module.
    Cpu(PathBuf),
}

/// The folders that may hold the WebGPU player, in the order they are tried:
/// the flag alone when given; else `RWGPU_WEB_PLAYER`, then `web-player` next
/// to the `rwgpu` program (a package that ships both never picks up another
/// version), then the workspace build.
fn web_player_candidates(flag: Option<&str>) -> Vec<PathBuf> {
    match flag {
        Some(f) => vec![PathBuf::from(f)],
        None => std::env::var_os("RWGPU_WEB_PLAYER")
            .map(PathBuf::from)
            .into_iter()
            .chain(std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(WEB_PLAYER_DIR))))
            .chain(SOURCE_WEB_PLAYER.map(PathBuf::from))
            .collect(),
    }
}

/// Does `dir` hold every file of the WebGPU player?
pub fn is_web_player(dir: &Path) -> bool {
    WEB_PLAYER_FILES.iter().all(|f| dir.join(f).is_file())
}

/// The player to serve: the WebGPU player where one is found, else the CPU
/// player. A missing WebGPU player is not an error; a `--web-player` folder
/// that does not hold one is, and so is having neither player.
pub fn choose(web_player: Option<&str>, wasm: Option<&str>) -> Result<(Player, Vec<PathBuf>), Diagnostic> {
    let candidates = web_player_candidates(web_player);
    if let Some(dir) = candidates.iter().find(|d| is_web_player(d)) {
        return Ok((Player::Gpu(dir.clone()), candidates));
    }
    if let Some(flag) = web_player {
        return Err(Diagnostic::new(&codes::FILE_NOT_FOUND, format!("`{flag}` does not hold the WebGPU player"))
            .hint("build it with `cargo xtask web-player`, which writes target/web-player")
            .note(format!("the folder needs: {}", WEB_PLAYER_FILES.join(", "))));
    }
    let wasm = wasm_path(wasm).map_err(|d| {
        d.note(format!(
            "and no WebGPU player was found in: {}",
            candidates.iter().map(|p| display_path(p)).collect::<Vec<_>>().join(", ")
        ))
    })?;
    Ok((Player::Cpu(wasm), candidates))
}

fn wasm_path(flag: Option<&str>) -> Result<PathBuf, Diagnostic> {
    let candidates: Vec<PathBuf> = match flag {
        Some(f) => vec![PathBuf::from(f)],
        None => std::env::var_os("RWGPU_WEB_WASM")
            .map(PathBuf::from)
            .into_iter()
            .chain(std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("rwgpuengine_web.wasm"))))
            .chain(SOURCE_WASM.map(PathBuf::from))
            .collect(),
    };
    candidates.iter().find(|p| p.is_file()).cloned().ok_or_else(|| {
        Diagnostic::new(&codes::FILE_NOT_FOUND, "the web player module rwgpuengine_web.wasm was not found")
            .hint("build it with `cargo xtask web`, or pass --wasm PATH")
            .note(format!("looked in: {}", candidates.iter().map(|p| display_path(p)).collect::<Vec<_>>().join(", ")))
    })
}

pub fn serve(p: &Parsed) -> Outcome {
    let loc = match locate(p.positional(0)) {
        Ok(l) => l,
        Err(d) => return Outcome::failed(vec![d]),
    };
    let port = match p.number("--port") {
        Ok(v) => v.unwrap_or(4410),
        Err(d) => return Outcome::usage(d),
    };
    let (player, looked) = match choose(p.value("--web-player"), p.value("--wasm")) {
        Ok(found) => found,
        Err(d) => return Outcome::failed(vec![d]).with_root(&loc.root),
    };
    let listener = match TcpListener::bind(("127.0.0.1", port as u16)) {
        Ok(l) => l,
        Err(e) => {
            return Outcome::failed(vec![
                Diagnostic::new(&codes::IO_ERROR, format!("cannot listen on 127.0.0.1:{port}: {e}"))
                    .hint("pick another port with --port"),
            ]);
        }
    };
    let which = match &player {
        Player::Gpu(dir) => format!(
            "the WebGPU player from {}; the CPU reference renderer where the browser has no WebGPU",
            display_path(dir)
        ),
        Player::Cpu(wasm) => format!(
            "the CPU player: {}; no WebGPU player in {} (build it with `cargo xtask web-player`)",
            display_path(wasm),
            looked.iter().map(|p| display_path(p)).collect::<Vec<_>>().join(", ")
        ),
    };
    // `--port 0` lets the system pick a free port: say which one it is.
    let port = listener.local_addr().map_or(port, |a| u64::from(a.port()));
    println!("serving {} at http://127.0.0.1:{port}/  ({which}; Ctrl+C to stop)", display_path(&loc.root));
    let _ = std::io::stdout().flush();
    for stream in listener.incoming().flatten() {
        if let Err(e) = handle(stream, &loc.root, &player) {
            eprintln!("serve: {e}");
        }
    }
    Outcome::new(true)
}

/// A file of the player being served: (content type, bytes), or `None`
/// for a path that is not one.
fn player_file(player: &Player, path: &str) -> Option<(&'static str, std::io::Result<Vec<u8>>)> {
    let name = match path {
        "/" => "index.html",
        p => p.strip_prefix('/')?,
    };
    let ctype = |n: &str| match n.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        _ => "application/wasm",
    };
    match player {
        Player::Gpu(dir) => {
            let file = WEB_PLAYER_FILES.iter().find(|f| **f == name)?;
            Some((ctype(file), std::fs::read(dir.join(file))))
        }
        Player::Cpu(wasm) => match name {
            "index.html" => Some((ctype(name), Ok(INDEX_HTML.as_bytes().to_vec()))),
            "player.js" => Some((ctype(name), Ok(PLAYER_JS.as_bytes().to_vec()))),
            "player.wasm" => Some(("application/wasm", std::fs::read(wasm))),
            _ => None,
        },
    }
}

fn handle(mut stream: TcpStream, root: &Path, player: &Player) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }
    let target = request.split_whitespace().nth(1).unwrap_or("/");
    let path = target.split('?').next().unwrap_or("/");
    let (status, ctype, body): (&str, &str, Vec<u8>) = match (player_file(player, path), path) {
        (Some((ctype, Ok(b))), _) => ("200 OK", ctype, b),
        (Some((_, Err(e))), _) => ("500 Internal Server Error", "text/plain", e.to_string().into_bytes()),
        (None, "/bundle" | "/bundle-hash" | "/project") => match Vfs::from_dir(root) {
            Err(e) => ("500 Internal Server Error", "text/plain", e.to_string().into_bytes()),
            Ok(vfs) => match path {
                "/bundle" => ("200 OK", "text/plain; charset=utf-8", vfs.to_bundle().into_bytes()),
                "/bundle-hash" => ("200 OK", "text/plain", hash::hex(vfs.content_hash()).into_bytes()),
                _ => ("200 OK", "application/json", project_json(&vfs).to_compact().into_bytes()),
            },
        },
        _ => ("404 Not Found", "text/plain", b"not found".to_vec()),
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    stream.flush()
}

fn project_json(vfs: &Vfs) -> Json {
    let (project, _) = load_project(vfs);
    let list = |dir: &str| Json::Arr(vfs.paths_under(dir, ".toml").map(Json::from).collect());
    Json::object()
        .with("name", project.as_ref().map(|p| p.name.clone()))
        .with("main_scene", project.as_ref().map(|p| p.main_scene.clone()))
        .with("scenes", list("scenes"))
        .with("tests", list("tests"))
        .with("version", rwgpuengine_core::ENGINE_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rwgpu-serve-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in files {
            std::fs::write(dir.join(f), format!("<{f}>")).unwrap();
        }
        dir
    }

    #[test]
    fn the_webgpu_player_is_served_when_its_folder_is_whole() {
        let whole = folder("whole", &WEB_PLAYER_FILES);
        let flag = whole.to_string_lossy().into_owned();
        let (player, looked) = choose(Some(&flag), None).unwrap();
        assert_eq!(player, Player::Gpu(whole.clone()));
        assert_eq!(looked, vec![whole.clone()]);

        // A folder the flag names that misses a file is an error, not the CPU player.
        let part = folder("part", &WEB_PLAYER_FILES[..3]);
        let flag = part.to_string_lossy().into_owned();
        let d = choose(Some(&flag), None).unwrap_err();
        assert_eq!(d.code, "E0001");
        assert!(!is_web_player(&part) && is_web_player(&whole));
        let _ = std::fs::remove_dir_all(whole);
        let _ = std::fs::remove_dir_all(part);
    }

    #[test]
    fn each_player_serves_its_own_files_and_nothing_else() {
        let dir = folder("files", &WEB_PLAYER_FILES);
        let gpu = Player::Gpu(dir.clone());
        let body = |p: &Player, path: &str| player_file(p, path).map(|(ctype, b)| (ctype, b.unwrap()));
        assert_eq!(body(&gpu, "/"), Some(("text/html; charset=utf-8", b"<index.html>".to_vec())));
        assert_eq!(body(&gpu, "/rwgpu_web_bg.wasm"), Some(("application/wasm", b"<rwgpu_web_bg.wasm>".to_vec())));
        assert_eq!(body(&gpu, "/rwgpu_web.js").map(|(c, _)| c), Some("text/javascript; charset=utf-8"));
        for other in ["/player.wasm", "/../Cargo.toml", "/bundle", "/x/index.html"] {
            assert_eq!(body(&gpu, other), None, "{other}");
        }
        let cpu = Player::Cpu(dir.join("rwgpu_web_bg.wasm"));
        assert_eq!(body(&cpu, "/").map(|(_, b)| b), Some(INDEX_HTML.as_bytes().to_vec()));
        assert_eq!(body(&cpu, "/player.js").map(|(_, b)| b), Some(PLAYER_JS.as_bytes().to_vec()));
        assert_eq!(body(&cpu, "/player.wasm").map(|(_, b)| b), Some(b"<rwgpu_web_bg.wasm>".to_vec()));
        assert_eq!(body(&cpu, "/rwgpu_web.js"), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
