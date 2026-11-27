//! `rwgpu serve`: a tiny HTTP server (std only) for the web player. It
//! serves the player's files (`web::find_player`) and the game's, made from
//! disk on every request, so data edits show up in the browser without any
//! build step. With `--site` it serves a folder as it is, as a static web
//! host would: a folder made by `rwgpu export web`.
//!
//! Two players (ADR 0024). The WebGPU player is a folder of built files
//! (`cargo xtask web-player`) that `rwgpu` finds and serves as they are; it
//! draws with WebGPU where the browser has it and with the CPU reference
//! renderer where not. Without it, the CPU player of ADR 0008 is served: the
//! page in `web/` and `rwgpuengine_web.wasm`. `rwgpu` links nothing of the
//! players' half either way.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_data::Vfs;

use crate::args::{Parsed, bad_arg};
use crate::out::{Outcome, display_path};
use crate::project::locate;
use crate::web::{self, PlayerFile};

/// What a request is answered from.
#[derive(Debug, Clone)]
enum Site {
    /// The player's files and the project's, made fresh per request.
    Project { root: PathBuf, player: Vec<PlayerFile> },
    /// A folder's files as they are.
    Folder(PathBuf),
}

pub fn serve(p: &Parsed) -> Outcome {
    let port = match p.number("--port") {
        Ok(v) => v.unwrap_or(4410),
        Err(d) => return Outcome::usage(d),
    };
    let (site, root, which) = match p.value("--site") {
        Some(dir) => {
            if p.positional(0).is_some() || p.value(web::WASM_FLAG).is_some() || p.value(web::WEB_PLAYER_FLAG).is_some()
            {
                return Outcome::usage(bad_arg(
                    "`--site` serves a folder as it is: it takes no project and no player".into(),
                    Some(format!("rwgpu serve --site {dir}")),
                ));
            }
            let folder = PathBuf::from(dir);
            if !folder.join("index.html").is_file() {
                return Outcome::failed(vec![
                    Diagnostic::new(&codes::FILE_NOT_FOUND, format!("`{dir}` has no index.html"))
                        .hint("pass a folder made by `rwgpu export web`, or serve a project with `rwgpu serve PATH`"),
                ]);
            }
            let canon = folder.canonicalize().unwrap_or(folder);
            let which = format!("the folder {} as it is", display_path(&canon));
            (Site::Folder(canon), None, which)
        }
        None => {
            let loc = match locate(p.positional(0)) {
                Ok(l) => l,
                Err(d) => return Outcome::failed(vec![d]),
            };
            let player = match web::find_player(p) {
                Ok(player) => player,
                Err(d) if d.code == codes::BAD_ARGUMENT.code => return Outcome::usage(d),
                Err(d) => return Outcome::failed(vec![d]).with_root(&loc.root),
            };
            let which = format!("{}; {}", display_path(&loc.root), player.describe());
            (Site::Project { root: loc.root.clone(), player: player.files() }, Some(loc.root), which)
        }
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
    // `--port 0` lets the system pick a free port: say which one it is.
    let port = listener.local_addr().map_or(port, |a| u64::from(a.port()));
    println!("serving {which} at http://127.0.0.1:{port}/  (Ctrl+C to stop)");
    let _ = std::io::stdout().flush();
    accept(&listener, &site);
    let o = Outcome::new(true);
    match root {
        Some(r) => o.with_root(&r),
        None => o,
    }
}

/// How long a connection may stay silent before it is dropped.
const IDLE: std::time::Duration = std::time::Duration::from_secs(10);

/// Answer every connection on a thread of its own. A browser opens
/// connections ahead of time and may leave one idle; answered one at a time,
/// that one held every request behind it (a page that never started, on the
/// Windows runner: ADR 0024).
fn accept(listener: &TcpListener, site: &Site) {
    for stream in listener.incoming().flatten() {
        let site = site.clone();
        std::thread::spawn(move || {
            let _ = stream.set_read_timeout(Some(IDLE));
            if let Err(e) = handle(stream, &site)
                && e.kind() != std::io::ErrorKind::WouldBlock
                && e.kind() != std::io::ErrorKind::TimedOut
            {
                eprintln!("serve: {e}");
            }
        });
    }
}

type Reply = (&'static str, &'static str, Vec<u8>);

fn handle(mut stream: TcpStream, site: &Site) -> std::io::Result<()> {
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
    let (status, ctype, body) = match site {
        Site::Project { root, player } => project_reply(path, root, player),
        Site::Folder(dir) => folder_reply(path, dir),
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    stream.flush()
}

fn not_found() -> Reply {
    ("404 Not Found", "text/plain", b"not found".to_vec())
}

fn failed(e: impl ToString) -> Reply {
    ("500 Internal Server Error", "text/plain", e.to_string().into_bytes())
}

fn project_reply(path: &str, root: &Path, player: &[PlayerFile]) -> Reply {
    let name = match path.trim_start_matches('/') {
        "" => "index.html",
        n => n,
    };
    if let Some(f) = player.iter().find(|f| f.name == name) {
        return match f.bytes() {
            Ok(b) => ("200 OK", web::content_type(name), b),
            Err(e) => failed(e),
        };
    }
    if ![web::PROJECT_JSON, web::BUNDLE, web::BUNDLE_HASH].contains(&name) {
        return not_found();
    }
    let vfs = match Vfs::from_dir(root) {
        Ok(v) => v,
        Err(e) => return failed(e),
    };
    let game = web::game(&vfs);
    let body = match name {
        web::BUNDLE => game.bundle(),
        web::BUNDLE_HASH => game.bundle_hash(),
        // `reload`: the page polls the bundle's hash, which only `serve` has.
        _ => game.project_json.with("reload", true).to_compact(),
    };
    ("200 OK", web::content_type(name), body.into_bytes())
}

/// The file a request names in a served folder: `/` and folders are their
/// `index.html`; no part may be `.` or `..`, and links may not lead out.
pub fn folder_file(dir: &Path, url_path: &str) -> Option<PathBuf> {
    let mut file = dir.to_path_buf();
    for part in url_path.split('/').filter(|p| !p.is_empty()) {
        if part == "." || part == ".." || part.contains('\\') || part.contains(':') {
            return None;
        }
        file.push(part);
    }
    if file.is_dir() {
        file.push("index.html");
    }
    let real = file.canonicalize().ok()?;
    (real.starts_with(dir) && real.is_file()).then_some(real)
}

fn folder_reply(path: &str, dir: &Path) -> Reply {
    let Some(file) = folder_file(dir, path) else {
        return not_found();
    };
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match std::fs::read(&file) {
        Ok(b) => ("200 OK", web::content_type(&name), b),
        Err(e) => failed(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::{Kind, Player, WEB_PLAYER_FILES};

    fn folder(name: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rwgpu-serve-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in files {
            std::fs::write(dir.join(f), format!("<{f}>")).unwrap();
        }
        dir.canonicalize().unwrap()
    }

    fn coin_vault() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/coin_vault")
    }

    #[test]
    fn a_served_folder_gives_only_its_own_files() {
        let base = std::env::temp_dir().join(format!("rwgpu-serve-site-{}", std::process::id()));
        let dir = base.join("site");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("index.html"), "page").unwrap();
        std::fs::write(dir.join("sub/index.html"), "sub page").unwrap();
        std::fs::write(base.join("secret.txt"), "no").unwrap();
        let dir = dir.canonicalize().unwrap();
        assert_eq!(folder_file(&dir, "/"), Some(dir.join("index.html")));
        assert_eq!(folder_file(&dir, "/sub/"), Some(dir.join("sub/index.html")));
        assert_eq!(folder_file(&dir, "/../secret.txt"), None);
        assert_eq!(folder_file(&dir, "/sub/../../secret.txt"), None);
        assert_eq!(folder_file(&dir, "/missing.js"), None);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(base.join("secret.txt"), dir.join("link.txt")).unwrap();
            assert_eq!(folder_file(&dir, "/link.txt"), None, "a link out of the folder is not served");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A connection that sends nothing does not hold up the next one.
    #[test]
    fn an_idle_connection_does_not_hold_up_a_request() {
        use std::io::Read;
        let dir = folder("idle", &WEB_PLAYER_FILES);
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let player = Player { kind: Kind::Webgpu, place: dir.clone(), missed: Vec::new() };
        let site = Site::Project { root: coin_vault(), player: player.files() };
        std::thread::spawn(move || accept(&listener, &site));
        let _idle = TcpStream::connect(addr).unwrap();
        let mut asked = TcpStream::connect(addr).unwrap();
        asked.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        asked.write_all(b"GET /player.js HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut answer = String::new();
        asked.read_to_string(&mut answer).unwrap();
        assert!(answer.starts_with("HTTP/1.1 200 OK") && answer.ends_with("<player.js>"), "{answer}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn each_player_serves_its_own_files_and_the_games() {
        let dir = folder("files", &WEB_PLAYER_FILES);
        let root = coin_vault();
        let gpu = Player { kind: Kind::Webgpu, place: dir.clone(), missed: Vec::new() }.files();
        let reply = |player: &[PlayerFile], path: &str| {
            let (status, ctype, body) = project_reply(path, &root, player);
            (status, ctype, String::from_utf8_lossy(&body).into_owned())
        };
        assert_eq!(reply(&gpu, "/"), ("200 OK", "text/html; charset=utf-8", "<index.html>".into()));
        assert_eq!(reply(&gpu, "/rwgpu_web_bg.wasm").1, "application/wasm");
        assert_eq!(reply(&gpu, "/rwgpu_web.js").1, "text/javascript; charset=utf-8");
        let cpu = Player { kind: Kind::Cpu, place: dir.join("rwgpu_web_bg.wasm"), missed: Vec::new() }.files();
        assert_eq!(reply(&cpu, "/").2, web::INDEX_HTML);
        assert_eq!(reply(&cpu, "/player.js").2, web::PLAYER_JS);
        assert_eq!(reply(&cpu, "/player.wasm").2, "<rwgpu_web_bg.wasm>");
        for player in [&gpu, &cpu] {
            for name in ["/project.json", "/bundle.txt"] {
                assert_eq!(reply(player, name).0, "200 OK", "{name}");
            }
            let (_, ctype, hash) = reply(player, "/bundle-hash.txt");
            assert_eq!((ctype, hash.len()), ("text/plain; charset=utf-8", 16));
            assert!(reply(player, "/project.json").2.ends_with(",\"reload\":true}"));
            for other in ["/bundle", "/project", "/../Cargo.toml", "/x/index.html"] {
                assert_eq!(reply(player, other).0, "404 Not Found", "{other}");
            }
        }
        assert_eq!(reply(&gpu, "/player.wasm").0, "404 Not Found");
        assert_eq!(reply(&cpu, "/rwgpu_web.js").0, "404 Not Found");
        let _ = std::fs::remove_dir_all(dir);
    }
}
