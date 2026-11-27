//! `cargo xtask load-bench`: what loading and checking a project costs as it
//! grows (ADR 0028). Generated projects are written as real files under
//! `target/load-bench/` and timed through every stage a command goes through:
//!
//! - `read`: the files from disk (`Vfs::from_dir`);
//! - `parse`: the scene's text to nodes, checked against the schema;
//! - `load`: the scene as a session loads it (`SceneLoader::load_scene`);
//! - `check`: the whole project as `rwgpu check` checks it (`check_project`);
//! - `build`: the world built from the loaded scene (`World::new`).
//!
//! The worlds:
//!
//! - `town`: the 3D study's town (proposal 3.5), `n` wanderers on every other
//!   cell. The world of `sim-bench town`, as scene text.
//! - `plates`, `wired`, `build`: the other worlds of `sim-bench`, as text.
//! - `linked`: `n` named entities that name each other: every [[connect]]
//!   has a source, a target, a payer and a condition by name, and a playtest
//!   names every entity.
//! - `crowded`: `n` actors that each start inside a wall, and `n`
//!   [[connect]]s that each name an entity that does not exist: the paths of
//!   warnings and errors.
//!
//! And the sizes the engine's limits are about (`rwgpuengine_core::limits`):
//!
//! - `ground`: a plain ground of `n` cells and one wanderer;
//! - `footprint`: one solid that covers `n` cells, and one wanderer;
//! - `houses`: solids of 4 x 4 cells that cover `n` cells in all;
//! - `copies`: `n` entities made from a prefab of 4 KB;
//! - `inputs`: a playtest of `n` [[input]] tables.
//!
//! `step` is the first step of the world built from the scene. A project
//! over a limit is refused (E0209): its row says so, and has no world.

use std::path::{Path, PathBuf};
use std::time::Instant;

use rwgpuengine_core::diag::{Diagnostic, SourceFile, Span};
use rwgpuengine_data::node::parse_toml;
use rwgpuengine_data::schema::{self, Checker};
use rwgpuengine_data::{SceneLoader, Vfs, check_project};
use rwgpuengine_sim::{InputFrame, NoSystems, World};

use crate::Result;

pub const KINDS: [&str; 11] =
    ["town", "plates", "wired", "build", "linked", "crowded", "ground", "footprint", "houses", "copies", "inputs"];
const SIZES: [usize; 4] = [1_000, 5_000, 20_000, 50_000];

/// The smallest even side whose every other cell holds `n` things.
pub fn side_for(n: usize) -> usize {
    let mut side = 2;
    while (side / 2) * (side / 2) < n {
        side += 2;
    }
    side
}

/// The `i`-th of the cells two apart on a ground of side `side`.
fn spot(i: usize, side: usize) -> (usize, usize) {
    (2 * (i % (side / 2)), 2 * (i / (side / 2)))
}

/// A generated project: its files and the seed `sim-bench` runs it with.
pub struct Project {
    pub files: Vec<(String, String)>,
    pub seed: u64,
}

impl Project {
    fn new(scene: String, prefabs: &[(&str, &str)], seed: u64) -> Project {
        let mut files = vec![
            ("rwgpu.toml".to_string(), "[project]\nname = \"bench\"\n".to_string()),
            ("scenes/main.toml".to_string(), scene),
        ];
        files.extend(prefabs.iter().map(|(name, text)| (format!("prefabs/{name}.toml"), text.to_string())));
        Project { files, seed }
    }

    /// The files as a file set, without writing them.
    pub fn vfs(&self) -> Vfs {
        let mut vfs = Vfs::new();
        for (path, text) in &self.files {
            vfs.insert(path.as_str(), text.as_str());
        }
        vfs
    }
}

/// `n` wanderers on every other cell (`sim-bench town`).
pub fn town(n: usize) -> Project {
    let side = side_for(n);
    let mut s = format!("[ground]\nsize = [{side}, {side}]\n");
    for i in 0..n {
        let (x, y) = spot(i, side);
        s.push_str(&format!("\n[[entity]]\npos = [{x}, {y}]\nwander = {{ step_frames = 1 }}\n"));
    }
    Project::new(s, &[], 7)
}

const PLATE: &str = "tile = {}\ntrigger = {}\n";

/// `n / 2` plates and `n / 2` wanderers (`sim-bench plates`); with `wired`,
/// every plate has a name and a [[connect]] of its own (`sim-bench wired`).
fn plates_or_wired(n: usize, wired: bool) -> Project {
    let pairs = n / 2;
    let side = side_for(pairs);
    let mut s =
        format!("[ground]\nsize = [{side}, {side}]\n\n[[entity]]\nname = \"hud\"\nprops = {{ on = 0, off = 0 }}\n");
    for i in 0..pairs {
        let (x, y) = spot(i, side);
        let name = if wired { format!("name = \"p_{i}\"\n") } else { String::new() };
        s.push_str(&format!(
            "\n[[entity]]\n{name}prefab = \"plate\"\npos = [{x}, {y}]\n\n[[entity]]\npos = [{}, {y}]\nwander = {{ step_frames = 1 }}\n",
            x + 1
        ));
    }
    if wired {
        // As `sim-bench wired` makes it: the prefab's connection listens to
        // `released` and still adds to `on`.
        s.push_str("\n[[connect]]\nfrom_prefab = \"plate\"\nsignal = \"released\"\nto = \"hud\"\naction = \"add\"\nprop = \"on\"\n");
        for i in 0..pairs {
            s.push_str(&format!(
                "\n[[connect]]\nfrom = \"p_{i}\"\nsignal = \"pressed\"\nto = \"hud\"\naction = \"add\"\nprop = \"on\"\n"
            ));
        }
    } else {
        s.push_str("\n[[connect]]\nfrom_prefab = \"plate\"\nsignal = \"pressed\"\nto = \"hud\"\naction = \"add\"\nprop = \"on\"\n");
        s.push_str("\n[[connect]]\nfrom_prefab = \"plate\"\nsignal = \"released\"\nto = \"hud\"\naction = \"add\"\nprop = \"off\"\n");
    }
    Project::new(s, &[("plate", PLATE)], 21)
}

/// `n` trees, a job that counts trees and huts, and spawns on ground taps
/// (`sim-bench build`).
pub fn build(n: usize) -> Project {
    let side = side_for(n);
    let mut s = format!(
        "[ground]\nsize = [{side}, {side}]\n\n[[entity]]\nname = \"bank\"\nprops = {{ coins = 1000000000, tries = 0 }}\n\n[[entity]]\nname = \"job\"\ncounts = {{ huts = \"hut\", trees = \"tree\" }}\n"
    );
    for i in 0..n {
        let (x, y) = spot(i, side);
        s.push_str(&format!("\n[[entity]]\nprefab = \"tree\"\npos = [{x}, {y}]\n"));
    }
    s.push_str(
        "\n[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"hut\"\ncost = { entity = \"bank\", prop = \"coins\", amount = 1 }\n\n[[connect]]\nfrom = \"ground\"\nsignal = \"refused\"\nto = \"bank\"\naction = \"add\"\nprop = \"tries\"\n",
    );
    Project::new(s, &[("tree", "sprite = {}\nsolid = true\n"), ("hut", "block = {}\nsolid = true\n")], 33)
}

/// `n` named entities that name each other, and a playtest that names them all.
fn linked(n: usize) -> Project {
    let side = side_for(n);
    let mut s = format!("[ground]\nsize = [{side}, {side}]\n");
    for i in 0..n {
        let (x, y) = spot(i, side);
        s.push_str(&format!(
            "\n[[entity]]\nname = \"a_{i}\"\npos = [{x}, {y}]\ntile = {{}}\nprops = {{ n = 0, coins = 5 }}\nlabel = {{ text = \"{{n}}\" }}\n"
        ));
    }
    for i in 0..n {
        let (to, cond) = ((i + 1) % n, (i + 2) % n);
        s.push_str(&format!(
            "\n[[connect]]\nfrom = \"a_{i}\"\nsignal = \"tapped\"\nactions = [{{ action = \"add\", to = \"a_{to}\", prop = \"n\" }}, {{ action = \"spawn\", prefab = \"hut\", cost = {{ entity = \"a_{i}\", prop = \"coins\", amount = 1 }} }}]\nwhen = [{{ entity = \"a_{cond}\", prop = \"n\", at_least = 0 }}]\n"
        ));
    }
    let mut test = "[test]\nframes = 10\n".to_string();
    for i in 0..n {
        test.push_str(&format!("\n[[expect]]\nentity = \"a_{i}\"\nprops = {{ n = 0 }}\n"));
    }
    let mut p = Project::new(s, &[("hut", "block = {}\nsolid = true\n")], 5);
    p.files.push(("tests/all.toml".to_string(), test));
    p
}

/// Every actor starts in a wall; every [[connect]] names a missing entity.
fn crowded(n: usize) -> Project {
    let side = side_for(n);
    let mut s = format!("[ground]\nsize = [{side}, {side}]\n");
    for i in 0..n {
        let (x, y) = spot(i, side);
        s.push_str(&format!(
            "\n[[entity]]\nname = \"w_{i}\"\nprefab = \"wall\"\npos = [{x}, {y}]\n\n[[entity]]\npos = [{x}, {y}]\nwander = {{}}\nsolid = true\n"
        ));
    }
    for i in 0..n {
        s.push_str(&format!(
            "\n[[connect]]\nfrom = \"w_{i}\"\nsignal = \"tapped\"\nto = \"x_{i}\"\naction = \"disable\"\n"
        ));
    }
    Project::new(s, &[("wall", "block = {}\nsolid = true\n")], 3)
}

/// The smallest side whose square holds `n` cells.
fn square(n: usize) -> usize {
    let mut side = 1;
    while side * side < n {
        side += 1;
    }
    side
}

fn ground(n: usize) -> Project {
    let side = square(n);
    Project::new(format!("[ground]\nsize = [{side}, {side}]\n\n[[entity]]\npos = [0, 0]\nwander = {{}}\n"), &[], 1)
}

fn footprint(n: usize) -> Project {
    let side = square(n);
    Project::new(
        format!(
            "[[entity]]\npos = [0, 0]\nfootprint = [{side}, {side}]\nblock = {{}}\nsolid = true\n\n[[entity]]\npos = [-1, -1]\nwander = {{}}\n"
        ),
        &[],
        1,
    )
}

fn houses(n: usize) -> Project {
    let count = n.div_ceil(16);
    let per_row = square(count);
    let side = per_row * 4;
    let mut s = format!("[ground]\nsize = [{side}, {side}]\n");
    for i in 0..count {
        s.push_str(&format!(
            "\n[[entity]]\nprefab = \"house\"\npos = [{}, {}]\n",
            4 * (i % per_row),
            4 * (i / per_row)
        ));
    }
    Project::new(s, &[("house", "footprint = [4, 4]\nblock = {}\nsolid = true\n")], 1)
}

fn copies(n: usize) -> Project {
    let side = side_for(n);
    let mut prefab = String::from("tile = {}\nprops = { ");
    for i in 0..200 {
        prefab.push_str(&format!("prop_{i:03} = {i}, "));
    }
    prefab.push_str("last = 0 }\n");
    let mut s = format!("[ground]\nsize = [{side}, {side}]\n");
    for i in 0..n {
        let (x, y) = spot(i, side);
        s.push_str(&format!("\n[[entity]]\nprefab = \"big\"\npos = [{x}, {y}]\n"));
    }
    Project::new(s, &[("big", &prefab)], 1)
}

fn inputs(n: usize) -> Project {
    let mut test = "[test]\nframes = 1000000\n".to_string();
    for i in 0..n {
        test.push_str(&format!("\n[[input]]\nframe = {}\npress = \"up\"\n", i % 1_000_000));
    }
    let mut p = Project::new("[ground]\nsize = [4, 4]\n\n[[entity]]\nmover = {}\n".to_string(), &[], 1);
    p.files.push(("tests/long.toml".to_string(), test));
    p
}

pub fn project(kind: &str, n: usize) -> Result<Project> {
    Ok(match kind {
        "town" => town(n),
        "plates" => plates_or_wired(n, false),
        "wired" => plates_or_wired(n, true),
        "build" => build(n),
        "linked" => linked(n),
        "crowded" => crowded(n),
        "ground" => ground(n),
        "footprint" => footprint(n),
        "houses" => houses(n),
        "copies" => copies(n),
        "inputs" => inputs(n),
        other => return Err(format!("unknown world `{other}` (one of: {})", KINDS.join(", "))),
    })
}

/// Write the project's files under `dir` (emptied first).
fn write(p: &Project, dir: &Path) -> Result {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    for (path, text) in &p.files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("a file has a folder")).map_err(|e| e.to_string())?;
        std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
    }
    Ok(())
}

/// Milliseconds of `f`: the median of up to 7 runs, or one when a run takes
/// more than two seconds.
fn time<T>(mut f: impl FnMut() -> T) -> (f64, T) {
    let mut times = Vec::new();
    let mut total = 0.0;
    loop {
        let start = Instant::now();
        let out = std::hint::black_box(f());
        let ms = start.elapsed().as_secs_f64() * 1e3;
        times.push(ms);
        total += ms;
        if times.len() >= 7 || total > 2_000.0 {
            times.sort_by(|a, b| a.total_cmp(b));
            return (times[times.len() / 2], out);
        }
    }
}

/// One measured project.
pub struct Row {
    pub kind: String,
    pub n: usize,
    pub bytes: usize,
    pub entities: usize,
    pub diagnostics: usize,
    pub read: f64,
    pub parse: f64,
    pub load: f64,
    pub check: f64,
    pub build: f64,
    /// The first step of the built world.
    pub step: f64,
    /// Over a limit (E0209).
    pub refused: bool,
}

pub fn measure(kind: &str, n: usize, dir: &Path) -> Result<Row> {
    let p = project(kind, n)?;
    write(&p, dir)?;
    let bytes = p.files.iter().map(|(_, t)| t.len()).sum();
    let (read, vfs) = time(|| Vfs::from_dir(dir));
    let vfs = vfs.map_err(|e| format!("{}: {e}", dir.display()))?;
    let text = vfs.get("scenes/main.toml").ok_or("no scene")?;
    let (parse, parsed) = time(|| {
        let src = SourceFile::new("scenes/main.toml", text);
        let root = parse_toml(&src).map_err(|d| d.message)?;
        let mut diags: Vec<Diagnostic> = Vec::new();
        let ok = Checker { src: &src, diags: &mut diags }.table(&root, &schema::SCENE_FILE, Span::new(0, 0));
        Ok::<_, String>((ok, diags.len()))
    });
    let (ok, schema_diags) = parsed?;
    if !ok {
        return Err(format!("{kind} {n}: {schema_diags} schema problems"));
    }
    let (load, scene) = time(|| {
        let mut loader = SceneLoader::new(&vfs);
        (loader.load_scene("scenes/main.toml"), loader.diags.len())
    });
    let (check, report) = time(|| check_project(&vfs));
    // `crowded` is made of mistakes: it does not load, and has no world;
    // neither has a project over a limit.
    let refused = report.diagnostics.iter().any(|d| d.code == "E0209");
    let (build, step, entities) = match scene.0 {
        Some(scene) if report.ok() => {
            let (ms, mut world) = time(|| World::new(&scene, p.seed));
            let start = Instant::now();
            world.tick(&InputFrame::default(), &mut NoSystems);
            (ms, start.elapsed().as_secs_f64() * 1e3, world.entities.len())
        }
        _ if kind == "crowded" || refused => (0.0, 0.0, 0),
        _ => return Err(format!("{kind} {n}: {:?}", report.diagnostics.iter().take(3).collect::<Vec<_>>())),
    };
    Ok(Row {
        kind: kind.to_string(),
        n,
        bytes,
        entities,
        diagnostics: report.diagnostics.len(),
        read,
        parse,
        load,
        check,
        build,
        step,
        refused,
    })
}

/// The budget's measurement (`time.load_check_ms` in budgets.toml): the
/// study's town of 50,000 actors written as files and checked as
/// `rwgpu check` checks it, the median of seven runs, in milliseconds.
pub fn budget_check_ms(root: &Path) -> Result<u64> {
    let dir = root.join("target/load-bench/budget-town-50000");
    write(&town(50_000), &dir)?;
    let vfs = Vfs::from_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut times: Vec<f64> = (0..7)
        .map(|_| {
            let start = Instant::now();
            let report = std::hint::black_box(check_project(&vfs));
            assert!(report.ok(), "the town does not check: {:?}", report.diagnostics.first());
            start.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    times.sort_by(|a, b| a.total_cmp(b));
    Ok(times[3].round() as u64)
}

/// `cargo xtask load-bench [town | plates | wired | build | linked | crowded ...] [N ...]`.
/// Without names: `town`; without numbers: 1,000, 5,000, 20,000 and 50,000.
pub fn run(args: &[String], root: &Path) -> Result {
    if cfg!(debug_assertions) {
        eprintln!("load-bench: this is a debug build; run `cargo run --release -p xtask -- load-bench` for times");
    }
    let mut kinds: Vec<String> = Vec::new();
    let mut sizes: Vec<usize> = Vec::new();
    for a in args {
        match a.replace('_', "").parse::<usize>() {
            Ok(n) => sizes.push(n),
            Err(_) => kinds.push(a.clone()),
        }
    }
    if kinds.is_empty() {
        kinds.push("town".into());
    }
    if sizes.is_empty() {
        sizes = SIZES.to_vec();
    }
    println!(
        "{:<9} {:>8} {:>9} {:>11} {:>6} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>11}",
        "world",
        "n",
        "entities",
        "bytes",
        "diags",
        "read ms",
        "parse ms",
        "load ms",
        "check ms",
        "build ms",
        "step ms",
        "check us/n"
    );
    let base: PathBuf = root.join("target/load-bench");
    for kind in &kinds {
        for &n in &sizes {
            let r = measure(kind, n, &base.join(format!("{kind}-{n}")))?;
            println!(
                "{:<9} {:>8} {:>9} {:>11} {:>6} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>11.2}",
                r.kind,
                r.n,
                r.entities,
                r.bytes,
                r.diagnostics,
                r.read,
                r.parse,
                r.load,
                r.check,
                r.build,
                r.step,
                r.check * 1e3 / r.n as f64
            );
            if r.refused {
                println!("          (refused: over a limit, E0209)");
            }
        }
    }
    Ok(())
}
