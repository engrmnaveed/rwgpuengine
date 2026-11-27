//! The web players' files: which exist, where each comes from, and the
//! game's files made from a project. `rwgpu serve` and `rwgpu export web`
//! both read this module, so a static host serves the files `serve` serves,
//! under the same names (ADR 0025).
//!
//! There are two players, and [`find_player`] is the one lookup for both:
//!
//! | player | files | comes from |
//! |---|---|---|
//! | WebGPU (ADR 0024), with the CPU reference renderer as its fallback | `index.html`, `player.js`, `rwgpu_web.js`, `rwgpu_web_bg.wasm` | a folder `cargo xtask web-player` writes (`web-player` next to `rwgpu` in a package) |
//! | CPU (ADR 0008), when there is no WebGPU player | `index.html`, `player.js` | compiled into `rwgpu` from `web/` |
//! | | `player.wasm` | `rwgpuengine_web.wasm`, built on its own |
//!
//! The game's files are the same for both: `project.json` and `bundle.txt`,
//! made from the project by [`game`] (and `bundle-hash.txt`, which only `serve`
//! has). Every address either page uses is relative, so a folder works at the
//! root of a site and under any path.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_core::hash::{self, Hasher64};
use rwgpuengine_core::json::Json;
use rwgpuengine_data::{PROJECT_FILE, Vfs, load_playtest, load_project};

use crate::args::{Parsed, bad_arg};
use crate::out::display_path;

pub const INDEX_HTML: &str = include_str!("../../../web/index.html");
pub const PLAYER_JS: &str = include_str!("../../../web/player.js");

/// The WebGPU player's files, as `cargo xtask web-player` writes them into
/// one folder, under the names the page asks for.
pub const WEB_PLAYER_FILES: [&str; 4] = ["index.html", "player.js", "rwgpu_web.js", "rwgpu_web_bg.wasm"];

/// The CPU player's files, under the names its page asks for.
pub const CPU_PLAYER_FILES: [&str; 3] = ["index.html", "player.js", "player.wasm"];

/// The WebGPU player's folder in a package, next to the `rwgpu` program.
pub const WEB_PLAYER_DIR: &str = "web-player";
/// The CPU player's module, as built and next to the `rwgpu` program.
pub const CPU_MODULE: &str = "rwgpuengine_web.wasm";

/// The game's name, main scene, scenes and playtests, for the page's lists.
pub const PROJECT_JSON: &str = "project.json";
/// The game's files as one text bundle (`Vfs::to_bundle`).
pub const BUNDLE: &str = "bundle.txt";
/// The bundle's content hash, which the page polls to reload: `rwgpu serve` only.
pub const BUNDLE_HASH: &str = "bundle-hash.txt";

/// Where the workspace builds each player, relative to the repository.
const WEB_PLAYER_BUILT: &str = "target/web-player";
const CPU_MODULE_BUILT: &str = "target/wasm32-unknown-unknown/web/rwgpuengine_web.wasm";

/// The flags and variables that name a player.
pub const WEB_PLAYER_FLAG: &str = "--web-player";
pub const WASM_FLAG: &str = "--wasm";
pub const WEB_PLAYER_ENV: &str = "RWGPU_WEB_PLAYER";
pub const WASM_ENV: &str = "RWGPU_WEB_WASM";

/// What a player file is, for `rwgpu export web`'s list.
pub fn what(kind: Kind, name: &str) -> &'static str {
    match (kind, name) {
        (_, "index.html") => "the page",
        (Kind::Webgpu, "player.js") => {
            "the page's script: loads the module and the game, hands it the canvas, the input and the animation frames"
        }
        (Kind::Webgpu, "rwgpu_web.js") => "the module's JavaScript glue, written by wasm-bindgen",
        (Kind::Webgpu, _) => {
            "the engine and the player as a wasm module: draws with WebGPU, or with the CPU reference renderer where \
             the browser has no WebGPU"
        }
        (Kind::Cpu, "player.js") => "the page's script: loads the module and the game, steps, draws, takes input",
        (Kind::Cpu, _) => "the engine as a wasm module, painting with the CPU (rwgpuengine_web)",
    }
}

/// Which web player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// WebGPU, with the CPU reference renderer as its fallback (ADR 0024).
    Webgpu,
    /// The CPU player of ADR 0008.
    Cpu,
}

impl Kind {
    /// The name `--json` gives it.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Webgpu => "webgpu",
            Kind::Cpu => "cpu",
        }
    }
}

/// Where a player file's bytes come from.
#[derive(Debug, Clone, PartialEq)]
pub enum From {
    /// Compiled into `rwgpu`.
    Text(&'static str),
    /// A file on disk, read each time, so `serve` picks up a rebuilt player
    /// without a restart.
    File(PathBuf),
}

/// One file of a web player, under the name the page asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerFile {
    pub name: &'static str,
    pub from: From,
}

impl PlayerFile {
    pub fn bytes(&self) -> std::io::Result<Vec<u8>> {
        match &self.from {
            From::Text(t) => Ok(t.as_bytes().to_vec()),
            From::File(p) => std::fs::read(p),
        }
    }

    /// Where it was read from, when it is a file.
    pub fn path(&self) -> Option<&Path> {
        match &self.from {
            From::Text(_) => None,
            From::File(p) => Some(p),
        }
    }
}

/// The player that was found.
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub kind: Kind,
    /// The WebGPU player's folder, or the CPU player's module.
    pub place: PathBuf,
    /// The places looked at for a WebGPU player before a CPU one was taken.
    pub missed: Vec<PathBuf>,
}

impl Player {
    /// Its files, in the order an export writes them.
    pub fn files(&self) -> Vec<PlayerFile> {
        match self.kind {
            Kind::Webgpu => WEB_PLAYER_FILES
                .iter()
                .map(|name| PlayerFile { name, from: From::File(self.place.join(name)) })
                .collect(),
            Kind::Cpu => vec![
                PlayerFile { name: "index.html", from: From::Text(INDEX_HTML) },
                PlayerFile { name: "player.js", from: From::Text(PLAYER_JS) },
                PlayerFile { name: "player.wasm", from: From::File(self.place.clone()) },
            ],
        }
    }

    /// One line for people: which player, and from where.
    pub fn describe(&self) -> String {
        match self.kind {
            Kind::Webgpu => format!(
                "the WebGPU player from {} (the CPU reference renderer where a browser has no WebGPU)",
                display_path(&self.place)
            ),
            Kind::Cpu if self.missed.is_empty() => format!("the CPU player: {}", display_path(&self.place)),
            Kind::Cpu => format!(
                "the CPU player: {}; no WebGPU player in {} (build it with `cargo xtask web-player`)",
                display_path(&self.place),
                self.missed.iter().map(|p| display_path(p)).collect::<Vec<_>>().join(", ")
            ),
        }
    }
}

/// The `Content-Type` a host should send for a file of the folder, by its
/// extension. A page compiles its module while it downloads only when it is
/// sent as `application/wasm`; otherwise both scripts compile it from its
/// bytes.
pub fn content_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("txt") => "text/plain; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

/// The source tree, for running from it (`cargo rwgpu serve ...`). Debug
/// builds only: a release program must not hold a path of the machine that
/// built it (ADR 0023).
#[cfg(debug_assertions)]
const SOURCE_TREE: Option<&str> = Some(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
#[cfg(not(debug_assertions))]
const SOURCE_TREE: Option<&str> = None;

/// The folder of the `rwgpu` program, links followed, as `rwgpu play` finds
/// its player.
fn program_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    exe.parent().map(Path::to_path_buf)
}

/// What names a player, if anything does.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Named {
    pub web_player: Option<String>,
    pub wasm: Option<String>,
    pub web_player_env: Option<OsString>,
    pub wasm_env: Option<OsString>,
}

impl Named {
    /// The flags of a command, and this process's variables (an empty one
    /// counts as unset). Both flags at once is a usage error.
    pub fn from(p: &Parsed) -> Result<Named, Diagnostic> {
        let (web_player, wasm) = (p.value(WEB_PLAYER_FLAG), p.value(WASM_FLAG));
        if web_player.is_some() && wasm.is_some() {
            return Err(bad_arg(
                format!("{WEB_PLAYER_FLAG} and {WASM_FLAG} name two players: give one"),
                Some(format!("{WEB_PLAYER_FLAG} DIR for the WebGPU player, {WASM_FLAG} FILE for the CPU player")),
            ));
        }
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
        Ok(Named {
            web_player: web_player.map(str::to_string),
            wasm: wasm.map(str::to_string),
            web_player_env: var(WEB_PLAYER_ENV),
            wasm_env: var(WASM_ENV),
        })
    }
}

/// Where a player is looked for, in order, and whether one place was named.
/// A flag names the one place, and a flag beats a variable: `--web-player`,
/// `--wasm`, then `RWGPU_WEB_PLAYER`, `RWGPU_WEB_WASM`. Otherwise a WebGPU
/// player anywhere beats a CPU one: `web-player` next to `rwgpu`, the
/// workspace's build (debug builds only), then `rwgpuengine_web.wasm` in the
/// same two places.
pub fn places(named: &Named, program: Option<&Path>, tree: Option<&Path>) -> (Vec<(Kind, PathBuf)>, Option<String>) {
    let one = |kind: Kind, p: PathBuf, how: String| (vec![(kind, p)], Some(how));
    if let Some(f) = &named.web_player {
        return one(Kind::Webgpu, PathBuf::from(f), format!("{WEB_PLAYER_FLAG} {f}"));
    }
    if let Some(f) = &named.wasm {
        return one(Kind::Cpu, PathBuf::from(f), format!("{WASM_FLAG} {f}"));
    }
    if let Some(v) = &named.web_player_env {
        return one(Kind::Webgpu, PathBuf::from(v), format!("{WEB_PLAYER_ENV}={}", v.to_string_lossy()));
    }
    if let Some(v) = &named.wasm_env {
        return one(Kind::Cpu, PathBuf::from(v), format!("{WASM_ENV}={}", v.to_string_lossy()));
    }
    let mut all = Vec::new();
    for (kind, beside, built) in
        [(Kind::Webgpu, WEB_PLAYER_DIR, WEB_PLAYER_BUILT), (Kind::Cpu, CPU_MODULE, CPU_MODULE_BUILT)]
    {
        all.extend(program.map(|d| (kind, d.join(beside))));
        all.extend(tree.map(|t| (kind, crate::out::normalize(&t.join(built)))));
    }
    (all, None)
}

/// Does `place` hold a player of this kind: every file of the WebGPU player's
/// folder, or the CPU player's module?
pub fn holds(kind: Kind, place: &Path) -> bool {
    match kind {
        Kind::Webgpu => WEB_PLAYER_FILES.iter().all(|f| place.join(f).is_file()),
        Kind::Cpu => place.is_file(),
    }
}

/// The web player for `serve` and `export web`: the one place a flag or a
/// variable names, or the first player found (WebGPU first). None found is
/// E0008, with the way to get one.
pub fn find_player(p: &Parsed) -> Result<Player, Diagnostic> {
    let named = Named::from(p)?;
    let program = program_dir();
    let (places, how) = places(&named, program.as_deref(), SOURCE_TREE.map(Path::new));
    let found = places.iter().find(|(kind, place)| holds(*kind, place));
    match found {
        Some((kind, place)) => {
            let missed = places.iter().take_while(|(k, _)| *k == Kind::Webgpu).map(|(_, p)| p.clone());
            let missed = if *kind == Kind::Cpu { missed.collect() } else { Vec::new() };
            Ok(Player { kind: *kind, place: place.clone(), missed })
        }
        None => Err(player_missing(&places, how)),
    }
}

fn player_missing(places: &[(Kind, PathBuf)], how: Option<String>) -> Diagnostic {
    let build = "from a clone of the rwgpuengine repository, `cargo xtask dist` builds `rwgpu` with both players next \
                 to it (target/release/; `--agent`: the CPU player only)";
    let d = match (how, places) {
        (Some(how), [(Kind::Webgpu, place)]) => {
            Diagnostic::new(&codes::WEB_PLAYER_NOT_FOUND, format!("`{how}` does not hold the WebGPU player"))
                .hint(format!(
                    "{build}; or build it with `cargo xtask web-player` ({WEB_PLAYER_BUILT}) and name that folder"
                ))
                .note(format!("{} needs: {}", display_path(place), WEB_PLAYER_FILES.join(", ")))
        }
        (Some(how), _) => {
            Diagnostic::new(&codes::WEB_PLAYER_NOT_FOUND, format!("`{how}`: the CPU player's module was not found"))
                .hint(format!("{build}; or build it with `cargo xtask web` ({CPU_MODULE_BUILT}) and name that file"))
        }
        (None, _) => Diagnostic::new(&codes::WEB_PLAYER_NOT_FOUND, "no web player was found").hint(format!(
            "{build}; or name one with {WEB_PLAYER_FLAG} DIR (`cargo xtask web-player`) or {WASM_FLAG} FILE (`cargo \
             xtask web`)"
        )),
    };
    d.note(format!("looked in: {}", places.iter().map(|(_, p)| display_path(p)).collect::<Vec<_>>().join(", ")))
}

/// Why a project file goes to the web.
pub const WHY_PROJECT: &str = "the project's settings: frame size, tick rate, seed, main scene";
pub const WHY_SCENE: &str = "a scene: the page lists it";
pub const WHY_PREFAB: &str = "a prefab: scenes and spawns build entities from it";
pub const WHY_PLAYTEST: &str = "a playtest: the page lists it as input and can replay it (`?input=`)";
pub const WHY_MAIN_SCENE: &str = "the main scene (outside scenes/)";
pub const WHY_PLAYTEST_SCENE: &str = "a scene a playtest plays (outside scenes/)";

/// The game's files for the web, made from a project's files.
#[derive(Debug)]
pub struct Game {
    /// The files the engine reads: `rwgpu.toml`, `scenes/`, `prefabs/`, `tests/`,
    /// and the main scene and playtests' scenes wherever they are.
    pub vfs: Vfs,
    /// Each bundled file and why it is there, in path order.
    pub why: Vec<(String, &'static str)>,
    /// The project's other `.toml` files: the engine never reads them.
    pub unread: Vec<String>,
    pub project_json: Json,
}

impl Game {
    pub fn bundle(&self) -> String {
        self.vfs.to_bundle()
    }

    pub fn bundle_hash(&self) -> String {
        hash::hex(self.vfs.content_hash())
    }
}

/// The game's files of a project. Never fails: a project with errors (while
/// `serve` reloads it) still gives every file under the usual folders.
pub fn game(vfs: &Vfs) -> Game {
    let (project, _) = load_project(vfs);
    let mut extra: Vec<(String, &'static str)> = Vec::new();
    if let Some(p) = &project {
        extra.push((p.main_scene.clone(), WHY_MAIN_SCENE));
    }
    for test in vfs.paths_under("tests", ".toml") {
        if let (Some(def), _) = load_playtest(vfs, test)
            && let Some(scene) = def.scene
        {
            extra.push((scene, WHY_PLAYTEST_SCENE));
        }
    }
    let mut out = Vfs::new();
    let mut why = Vec::new();
    let mut unread = Vec::new();
    for path in vfs.paths() {
        let reason = if path == PROJECT_FILE {
            Some(WHY_PROJECT)
        } else if path.starts_with("scenes/") {
            Some(WHY_SCENE)
        } else if path.starts_with("prefabs/") {
            Some(WHY_PREFAB)
        } else if path.starts_with("tests/") {
            Some(WHY_PLAYTEST)
        } else {
            extra.iter().find(|(p, _)| p == path).map(|(_, w)| *w)
        };
        match reason {
            Some(w) => {
                out.insert(path, vfs.get(path).unwrap_or_default());
                why.push((path.to_string(), w));
            }
            None => unread.push(path.to_string()),
        }
    }
    let list = |dir: &str| Json::Arr(out.paths_under(dir, ".toml").map(Json::from).collect());
    let project_json = Json::object()
        .with("name", project.as_ref().map(|p| p.name.clone()))
        .with("main_scene", project.as_ref().map(|p| p.main_scene.clone()))
        .with("scenes", list("scenes"))
        .with("tests", list("tests"))
        .with("version", rwgpuengine_core::ENGINE_VERSION);
    Game { vfs: out, why, unread, project_json }
}

/// FNV-1a 64 of a file's bytes, as `rwgpu` writes state and image hashes.
pub fn file_hash(bytes: &[u8]) -> String {
    hash::hex(Hasher64::new().bytes(bytes).finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vfs(files: &[(&str, &str)]) -> Vfs {
        let mut v = Vfs::new();
        for (p, t) in files {
            v.insert(*p, *t);
        }
        v
    }

    #[test]
    fn the_bundle_holds_what_the_engine_reads_and_nothing_else() {
        let v = vfs(&[
            ("rwgpu.toml", "[project]\nname = \"G\"\nmain_scene = \"levels/start.toml\"\n"),
            ("levels/start.toml", "[scene]\n"),
            ("levels/other.toml", "[scene]\n"),
            ("extra/side.toml", "[scene]\n"),
            ("scenes/a.toml", "[scene]\n"),
            ("prefabs/p.toml", ""),
            ("tests/t.toml", "[test]\nscene = \"extra/side.toml\"\nframes = 1\n"),
            ("notes.toml", "x = 1\n"),
        ]);
        let g = game(&v);
        let paths: Vec<&str> = g.vfs.paths().collect();
        assert_eq!(
            paths,
            ["extra/side.toml", "levels/start.toml", "prefabs/p.toml", "rwgpu.toml", "scenes/a.toml", "tests/t.toml"]
        );
        assert_eq!(g.unread, ["levels/other.toml", "notes.toml"]);
        assert!(g.why.contains(&("levels/start.toml".to_string(), WHY_MAIN_SCENE)));
        assert!(g.why.contains(&("extra/side.toml".to_string(), WHY_PLAYTEST_SCENE)));
        assert_eq!(g.project_json.get("scenes").and_then(Json::as_array).map(<[Json]>::len), Some(1));
        assert!(g.bundle().starts_with("rwgpu-bundle 1\n"));
    }

    #[test]
    fn a_project_with_errors_still_gives_its_usual_folders() {
        let g = game(&vfs(&[("rwgpu.toml", "[project"), ("scenes/a.toml", "x"), ("tests/t.toml", "[[")]));
        assert_eq!(g.vfs.paths().count(), 3);
        assert!(g.project_json.get("name").is_some_and(Json::is_null));
    }

    #[test]
    fn every_address_the_page_uses_is_relative() {
        // The CPU player's page and script name files without a leading slash or
        // a host, so the folder works under any path of a site. (The WebGPU
        // player's are held to the same by xtask's tests and by the browser
        // check under a path.)
        for (name, text) in [("index.html", INDEX_HTML), ("player.js", PLAYER_JS)] {
            for bad in ["src=\"/", "href=\"/", "fetch('/", "fetch(\"/", "http://", "https://", "//cdn"] {
                assert!(!text.contains(bad), "{name} has `{bad}`");
            }
        }
        for name in CPU_PLAYER_FILES.iter().chain(&WEB_PLAYER_FILES) {
            assert!(!name.contains('/') && !name.contains('\\'), "{name}");
        }
        for name in CPU_PLAYER_FILES.iter().skip(1) {
            assert!(PLAYER_JS.contains(name) || INDEX_HTML.contains(name), "{name}");
        }
        for name in [PROJECT_JSON, BUNDLE, BUNDLE_HASH] {
            assert!(PLAYER_JS.contains(&format!("'{name}'")), "player.js does not ask for {name}");
        }
        let cpu = Player { kind: Kind::Cpu, place: PathBuf::from("m.wasm"), missed: Vec::new() };
        let names: Vec<&str> = cpu.files().iter().map(|f| f.name).collect();
        assert_eq!(names, CPU_PLAYER_FILES);
    }

    #[test]
    fn a_flag_or_a_variable_names_the_one_place() {
        let (program, tree) = (Path::new("bin"), Path::new("src"));
        let at = |named: &Named| places(named, Some(program), Some(tree));
        let flag = Named { web_player: Some("w".into()), wasm_env: Some("e.wasm".into()), ..Named::default() };
        assert_eq!(at(&flag), (vec![(Kind::Webgpu, PathBuf::from("w"))], Some("--web-player w".into())));
        let flag = Named { wasm: Some("m.wasm".into()), web_player_env: Some("v".into()), ..Named::default() };
        assert_eq!(at(&flag).0, [(Kind::Cpu, PathBuf::from("m.wasm"))], "a flag beats a variable");
        let var = Named { web_player_env: Some("v".into()), wasm_env: Some("e.wasm".into()), ..Named::default() };
        assert_eq!(at(&var), (vec![(Kind::Webgpu, PathBuf::from("v"))], Some("RWGPU_WEB_PLAYER=v".into())));
        let var = Named { wasm_env: Some("e.wasm".into()), ..Named::default() };
        assert_eq!(at(&var).0, [(Kind::Cpu, PathBuf::from("e.wasm"))]);
        // Nothing named: a WebGPU player anywhere first, next to the program first.
        let (all, how) = at(&Named::default());
        assert_eq!(how, None);
        assert_eq!(
            all,
            [
                (Kind::Webgpu, program.join("web-player")),
                (Kind::Webgpu, tree.join("target/web-player")),
                (Kind::Cpu, program.join("rwgpuengine_web.wasm")),
                (Kind::Cpu, tree.join("target/wasm32-unknown-unknown/web/rwgpuengine_web.wasm")),
            ]
        );
        // A release program has no source tree.
        assert_eq!(places(&Named::default(), Some(program), None).0.len(), 2);
    }

    #[test]
    fn a_player_is_whole_or_not_found() {
        let dir = std::env::temp_dir().join(format!("rwgpu-web-holds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in &WEB_PLAYER_FILES[..3] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        assert!(!holds(Kind::Webgpu, &dir), "a folder without the module is no player");
        std::fs::write(dir.join(WEB_PLAYER_FILES[3]), "x").unwrap();
        assert!(holds(Kind::Webgpu, &dir) && !holds(Kind::Cpu, &dir));
        assert!(holds(Kind::Cpu, &dir.join(WEB_PLAYER_FILES[3])));
        let gpu = Player { kind: Kind::Webgpu, place: dir.clone(), missed: Vec::new() };
        let files = gpu.files();
        assert_eq!(files.iter().map(|f| f.name).collect::<Vec<_>>(), WEB_PLAYER_FILES);
        assert_eq!(files[3].path(), Some(dir.join("rwgpu_web_bg.wasm").as_path()));
        // Named and missing: E0008, naming what the folder needs.
        let d = player_missing(&[(Kind::Webgpu, dir.join("no"))], Some("--web-player no".into()));
        assert_eq!(d.code, "E0008");
        assert!(d.render_human().contains("rwgpu_web_bg.wasm"), "{}", d.render_human());
        let d = player_missing(&places(&Named::default(), Some(&dir), None).0, None);
        assert!(d.render_human().contains("cargo xtask dist"), "{}", d.render_human());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn content_types_are_the_ones_a_browser_needs() {
        assert_eq!(content_type("player.wasm"), "application/wasm");
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(content_type(BUNDLE), "text/plain; charset=utf-8");
        assert_eq!(content_type("README"), "application/octet-stream");
    }
}
