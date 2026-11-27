//! `cargo xtask sim-bench`: what one simulation step costs as the world
//! grows (ADR 0026). Four generated worlds, each asking one kind of
//! question many times:
//!
//! - `town`: the 3D study's town (proposal 3.5), `n` actors on every other
//!   cell that each try one step per frame. "Is this cell free?"
//! - `plates`: `n / 2` plates and `n / 2` wanderers. "Who stands on me?",
//!   and a signal for every answer that changed.
//! - `wired`: the same plates, each with a name and a `[[connect]]` of its
//!   own instead of one `from_prefab` for all. "Who listens to this signal?"
//! - `build`: `n` trees, a job that counts them, and 64 taps on the ground
//!   every frame that each try to place a hut. "Is there room?" and "how
//!   many are there?"
//!
//! The worlds are built from definitions, not from scene text: the loader's
//! checks do not scale yet, and this measures the step.
//!
//! Events are written as a game runs by default (ADR 0027): the first 256 of
//! each per-actor kind in a step, the rest counted. `--all` writes every
//! event, as the engine did before. `events/step` is what happened,
//! `written/step` what the log wrote.

use std::time::Instant;

use rwgpuengine_core::hash;
use rwgpuengine_core::rng::Rng;
use rwgpuengine_data::{ConnectDef, ConnectFrom, EntityDef, SceneDef, SceneLoader, Vfs};
use rwgpuengine_sim::{InputFrame, NoSystems, Tap, Watch, World};

use crate::Result;

const KINDS: [&str; 4] = ["town", "plates", "wired", "build"];
const SIZES: [usize; 4] = [1_000, 5_000, 20_000, 50_000];
/// Ground taps per frame in the `build` world.
const TAPS: usize = 64;

/// The smallest even side whose every other cell holds `n` things.
fn side_for(n: usize) -> usize {
    let mut side = 2;
    while (side / 2) * (side / 2) < n {
        side += 2;
    }
    side
}

/// A small scene through the loader, for its ground, its connections and one
/// definition of each kind of entity. The entities are then copied.
fn load(scene: String, prefabs: &[(&str, &str)]) -> Result<SceneDef> {
    let mut vfs = Vfs::new();
    vfs.insert("scenes/main.toml", scene);
    for (name, text) in prefabs {
        vfs.insert(format!("prefabs/{name}.toml"), *text);
    }
    let mut loader = SceneLoader::new(&vfs);
    loader.load_scene("scenes/main.toml").ok_or_else(|| format!("the bench scene does not load: {:?}", loader.diags))
}

fn at(def: &EntityDef, x: usize, y: usize) -> EntityDef {
    EntityDef { pos: [x as f32, y as f32, 0.0], ..def.clone() }
}

/// `n` wanderers on every other cell. The world of the oracle's `town-*`
/// (`crates/rwgpuengine_sim/src/oracle.rs`): same cells, same seed.
pub fn town(n: usize) -> Result<(SceneDef, u64)> {
    let side = side_for(n);
    let mut scene = load(
        format!("[ground]\nsize = [{side}, {side}]\n\n[[entity]]\npos = [0, 0]\nwander = {{ step_frames = 1 }}\n"),
        &[],
    )?;
    let actor = scene.entities[0].clone();
    scene.entities = (0..n).map(|i| at(&actor, 2 * (i % (side / 2)), 2 * (i / (side / 2)))).collect();
    Ok((scene, 7))
}

/// `n / 2` plates on every other cell, each with a wanderer beside it.
pub fn plates(n: usize) -> Result<(SceneDef, u64)> {
    let pairs = n / 2;
    let side = side_for(pairs);
    let mut scene = load(
        format!(
            r#"[ground]
size = [{side}, {side}]

[[entity]]
name = "hud"
props = {{ on = 0, off = 0 }}

[[entity]]
prefab = "plate"
pos = [0, 0]

[[entity]]
pos = [1, 0]
wander = {{ step_frames = 1 }}

[[connect]]
from_prefab = "plate"
signal = "pressed"
to = "hud"
action = "add"
prop = "on"

[[connect]]
from_prefab = "plate"
signal = "released"
to = "hud"
action = "add"
prop = "off"
"#
        ),
        &[("plate", "tile = {}\ntrigger = {}\n")],
    )?;
    let (hud, plate, actor) = (scene.entities[0].clone(), scene.entities[1].clone(), scene.entities[2].clone());
    scene.entities = vec![hud];
    for i in 0..pairs {
        let (x, y) = (2 * (i % (side / 2)), 2 * (i / (side / 2)));
        scene.entities.push(at(&plate, x, y));
        scene.entities.push(at(&actor, x + 1, y));
    }
    Ok((scene, 21))
}

/// The plates of [`plates`], wired one by one: every plate has a name and a
/// `[[connect]]` that listens to it alone.
pub fn wired(n: usize) -> Result<(SceneDef, u64)> {
    let (mut scene, seed) = plates(n)?;
    let pressed: ConnectDef = scene.connections[0].clone();
    scene.connections.truncate(1);
    scene.connections[0].signal = "released".into();
    for (i, e) in scene.entities.iter_mut().filter(|e| e.trigger.is_some()).enumerate() {
        let name = format!("p_{i}");
        scene.connections.push(ConnectDef { from: ConnectFrom::Entity(name.clone()), ..pressed.clone() });
        e.name = Some(name);
    }
    Ok((scene, seed))
}

/// `n` trees on every other cell and a job that counts trees and huts. The
/// input taps the ground [`TAPS`] times a frame; each tap tries to place a hut.
pub fn build(n: usize) -> Result<(SceneDef, u64)> {
    let side = side_for(n);
    let mut scene = load(
        format!(
            r#"[ground]
size = [{side}, {side}]

[[entity]]
name = "bank"
props = {{ coins = 1000000000, tries = 0 }}

[[entity]]
name = "job"
counts = {{ huts = "hut", trees = "tree" }}

[[entity]]
prefab = "tree"
pos = [0, 0]

[[connect]]
from = "ground"
signal = "tapped"
action = "spawn"
prefab = "hut"
cost = {{ entity = "bank", prop = "coins", amount = 1 }}

[[connect]]
from = "ground"
signal = "refused"
to = "bank"
action = "add"
prop = "tries"
"#
        ),
        &[("tree", "sprite = {}\nsolid = true\n"), ("hut", "block = {}\nsolid = true\n")],
    )?;
    let tree = scene.entities[2].clone();
    scene.entities.truncate(2);
    scene.entities.extend((0..n).map(|i| at(&tree, 2 * (i % (side / 2)), 2 * (i / (side / 2)))));
    Ok((scene, 33))
}

fn scene(kind: &str, n: usize) -> Result<(SceneDef, u64)> {
    match kind {
        "town" => town(n),
        "plates" => plates(n),
        "wired" => wired(n),
        "build" => build(n),
        other => Err(format!("unknown world `{other}` (one of: {})", KINDS.join(", "))),
    }
}

/// One measured world.
struct Row {
    kind: String,
    n: usize,
    entities: usize,
    steps: u32,
    median_ms: f64,
    fastest_ms: f64,
    events_per_step: f64,
    written_per_step: f64,
    hash: u64,
    /// Bytes the world's index holds at the end.
    index_bytes: usize,
}

/// Step a world `steps` times (or, without a number, for about two seconds
/// and at least five steps) and time each step.
fn measure(kind: &str, n: usize, steps: Option<u32>, all: bool) -> Result<Row> {
    let (def, seed) = scene(kind, n)?;
    let size = def.ground.as_ref().map(|g| g.size).unwrap_or([1, 1]);
    let mut world = World::new(&def, seed);
    if all {
        world.events.set_watch(Watch::everything());
    }
    let mut taps = Rng::new(seed ^ 0xbead);
    let mut times: Vec<f64> = Vec::new();
    let mut total = 0.0;
    let written_before = world.events.len() as u64 + world.events.dropped();
    let events_before = world.events.happened();
    loop {
        let done = times.len() as u32;
        match steps {
            Some(s) if done >= s => break,
            None if done >= 400 || (done >= 5 && total >= 2000.0) => break,
            _ => {}
        }
        let mut input = InputFrame::default();
        if kind == "build" {
            input.taps = (0..TAPS)
                .map(|_| {
                    let cell = [taps.range_i32(0, size[0] - 1), taps.range_i32(0, size[1] - 1)];
                    Tap {
                        x: cell[0] as f32,
                        y: cell[1] as f32,
                        hit: None,
                        under_disabled: Vec::new(),
                        ground: Some(cell),
                    }
                })
                .collect();
        }
        let start = Instant::now();
        world.tick(&input, &mut NoSystems);
        let ms = start.elapsed().as_secs_f64() * 1e3;
        total += ms;
        times.push(ms);
    }
    let written = world.events.len() as u64 + world.events.dropped() - written_before;
    let events = world.events.happened() - events_before;
    let steps = times.len() as u32;
    times.sort_by(|a, b| a.total_cmp(b));
    Ok(Row {
        kind: kind.to_string(),
        n,
        entities: world.entities.len(),
        steps,
        median_ms: times[times.len() / 2],
        fastest_ms: times[0],
        events_per_step: events as f64 / steps.max(1) as f64,
        written_per_step: written as f64 / steps.max(1) as f64,
        hash: world.state_hash(),
        index_bytes: world.index_stats().bytes,
    })
}

/// Write the town of [`town`] as a project folder, each wanderer named
/// `walker_<i>`: the world of ADR 0027's worked example, for `rwgpu run` and
/// `rwgpu inspect`. The same cells and seed as the town measured here.
fn write_town(n: usize, dir: &std::path::Path) -> Result {
    let side = side_for(n);
    let mut scene = format!("[scene]\nname = \"Town of {n}\"\nseed = 7\n\n[ground]\nsize = [{side}, {side}]\n");
    for i in 0..n {
        let (x, y) = (2 * (i % (side / 2)), 2 * (i / (side / 2)));
        scene.push_str(&format!(
            "\n[[entity]]\nname = \"walker_{i}\"\npos = [{x}, {y}]\nwander = {{ step_frames = 1 }}\n"
        ));
    }
    let project = format!(
        "[project]\nname = \"town-{n}\"\nengine = \"0.1\"\nmain_scene = \"scenes/main.toml\"\n\n[sim]\ntick_rate = 60\nseed = 7\n"
    );
    std::fs::create_dir_all(dir.join("scenes")).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("rwgpu.toml"), project).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("scenes/main.toml"), scene).map_err(|e| e.to_string())?;
    println!("wrote {} ({n} wanderers on a {side} x {side} ground)", dir.display());
    Ok(())
}

/// The `time.sim_step_us` budget: microseconds for one step of the town of
/// 50,000 wanderers, events as a game writes them by default; the median of
/// 200 steps after 20. Meant for a release build (`budgets` runs it so).
pub fn budget_step_us() -> Result<i64> {
    let (def, seed) = town(50_000)?;
    let mut world = World::new(&def, seed);
    let input = InputFrame::default();
    for _ in 0..20 {
        world.tick(&input, &mut NoSystems);
    }
    let mut times: Vec<u128> = (0..200)
        .map(|_| {
            let start = Instant::now();
            world.tick(&input, &mut NoSystems);
            start.elapsed().as_micros()
        })
        .collect();
    times.sort_unstable();
    Ok(times[times.len() / 2] as i64)
}

/// `cargo xtask sim-bench [town|plates|wired|build ...] [N ...] [--steps S] [--all]`,
/// or `sim-bench town N --write DIR` to write that town as a project.
/// Without names it measures all four worlds; without numbers, at 1,000,
/// 5,000, 20,000 and 50,000. `--steps` fixes the number of steps, so that
/// two engines can be compared at the same frame: the state hash printed
/// must then be equal.
pub fn run(args: &[String]) -> Result {
    if cfg!(debug_assertions) {
        eprintln!("sim-bench: this is a debug build; run `cargo run --release -p xtask -- sim-bench` for times");
    }
    let mut kinds: Vec<String> = Vec::new();
    let mut sizes: Vec<usize> = Vec::new();
    let mut steps = None;
    let mut all = false;
    let mut write = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--write" {
            write = Some(it.next().ok_or("--write needs a folder")?);
        } else if a == "--all" {
            all = true;
        } else if a == "--steps" {
            let v = it.next().ok_or("--steps needs a number")?;
            steps = Some(v.parse::<u32>().map_err(|e| format!("--steps {v}: {e}"))?);
        } else if let Ok(n) = a.replace('_', "").parse::<usize>() {
            sizes.push(n);
        } else {
            kinds.push(a.clone());
        }
    }
    if let Some(dir) = write {
        let (["town"], &[n]) = (kinds.iter().map(String::as_str).collect::<Vec<_>>().as_slice(), sizes.as_slice())
        else {
            return Err("--write writes one town: `sim-bench town N --write DIR`".into());
        };
        return write_town(n, std::path::Path::new(dir));
    }
    if kinds.is_empty() {
        kinds = KINDS.iter().map(|k| k.to_string()).collect();
    }
    if sizes.is_empty() {
        sizes = SIZES.to_vec();
    }
    println!("an entity is {} bytes", std::mem::size_of::<rwgpuengine_sim::Entity>());
    println!("events: {}", if all { "every one written (--all)" } else { "as a game writes them by default" });
    println!(
        "{:<7} {:>8} {:>9} {:>6} {:>12} {:>12} {:>10} {:>12} {:>13} {:>10}  state hash at the last frame",
        "world",
        "n",
        "entities",
        "steps",
        "median ms",
        "fastest ms",
        "ns per n",
        "events/step",
        "written/step",
        "index KB"
    );
    for kind in &kinds {
        for &n in &sizes {
            let r = measure(kind, n, steps, all)?;
            println!(
                "{:<7} {:>8} {:>9} {:>6} {:>12.4} {:>12.4} {:>10.1} {:>12.0} {:>13.0} {:>10.0}  {}",
                r.kind,
                r.n,
                r.entities,
                r.steps,
                r.median_ms,
                r.fastest_ms,
                r.median_ms * 1e6 / r.n as f64,
                r.events_per_step,
                r.written_per_step,
                r.index_bytes as f64 / 1024.0,
                hash::hex(r.hash)
            );
        }
    }
    Ok(())
}
