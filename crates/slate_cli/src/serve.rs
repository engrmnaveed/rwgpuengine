//! `slate serve`: a tiny single-threaded HTTP server (std only) for the web
//! player. It serves the page, the wasm module and the project as a bundle
//! that is rebuilt from disk on every request, so data edits show up in
//! the browser without any build step.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

use slate_core::codes;
use slate_core::diag::Diagnostic;
use slate_core::hash;
use slate_core::json::Json;
use slate_data::{Vfs, load_project};

use crate::args::Parsed;
use crate::out::{Outcome, display_path};
use crate::project::locate;

const INDEX_HTML: &str = include_str!("../../../web/index.html");
const PLAYER_JS: &str = include_str!("../../../web/player.js");

/// Where `cargo xtask web` puts the module.
const DEFAULT_WASM: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/wasm32-unknown-unknown/web/slate_web.wasm");

fn wasm_path(flag: Option<&str>) -> Result<PathBuf, Diagnostic> {
    let candidates: Vec<PathBuf> = match flag {
        Some(f) => vec![PathBuf::from(f)],
        None => std::env::var_os("SLATE_WEB_WASM")
            .map(PathBuf::from)
            .into_iter()
            .chain(std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("slate_web.wasm"))))
            .chain(std::iter::once(PathBuf::from(DEFAULT_WASM)))
            .collect(),
    };
    candidates.iter().find(|p| p.is_file()).cloned().ok_or_else(|| {
        Diagnostic::new(&codes::FILE_NOT_FOUND, "the web player module slate_web.wasm was not found")
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
    let wasm = match wasm_path(p.value("--wasm")) {
        Ok(w) => w,
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
    println!(
        "serving {} at http://127.0.0.1:{port}/  (wasm: {}; Ctrl+C to stop)",
        display_path(&loc.root),
        display_path(&wasm)
    );
    let _ = std::io::stdout().flush();
    for stream in listener.incoming().flatten() {
        if let Err(e) = handle(stream, &loc.root, &wasm) {
            eprintln!("serve: {e}");
        }
    }
    Outcome::new(true)
}

fn handle(mut stream: TcpStream, root: &Path, wasm: &Path) -> std::io::Result<()> {
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
    let (status, ctype, body): (&str, &str, Vec<u8>) = match path {
        "/" | "/index.html" => ("200 OK", "text/html; charset=utf-8", INDEX_HTML.as_bytes().to_vec()),
        "/player.js" => ("200 OK", "text/javascript; charset=utf-8", PLAYER_JS.as_bytes().to_vec()),
        "/player.wasm" => match std::fs::read(wasm) {
            Ok(b) => ("200 OK", "application/wasm", b),
            Err(e) => ("500 Internal Server Error", "text/plain", e.to_string().into_bytes()),
        },
        "/bundle" | "/bundle-hash" | "/project" => match Vfs::from_dir(root) {
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
        .with("version", slate_core::ENGINE_VERSION)
}
