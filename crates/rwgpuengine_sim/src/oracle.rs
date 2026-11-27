//! The oracle of the simulation at scale (ADR 0026): generated worlds, and
//! what the engine reached in each of them when every question about space
//! was answered by scanning all entities.
//!
//! The pins in [`PINS`] were recorded with that engine, before the cell index
//! existed. A change to how the world stores or finds entities must reach
//! every one of them: the state hash at the last frame, a hash over the state
//! hash of every frame, and a hash over every event written. They are not
//! updated to make a test pass; a pin that cannot be kept is a finding for
//! the owner (ADR 0005).
//!
//! Each world is run twice. Once with every event written (`Watch::everything`):
//! that run reaches the three numbers of the scanning engine, so every sentence
//! is still the one it wrote. And once as a game runs by default, with the
//! per-step budget of the per-actor kinds (ADR 0027): the same two state
//! numbers, and the fourth number, the events then written. It differs from
//! the third only in the worlds where more than 256 events of one of those
//! kinds happen in a step, and it was recorded with the budget.
//!
//! The worlds are made to ask every question often: blocked moves, pushes,
//! plates and pads, spawns with costs and refusals, entities disabled and
//! enabled again, counts, signals with several actions, taps on entities and
//! on the ground. They are scene files like any other and go through the
//! loader, so they are worlds a game can have.

use std::collections::BTreeSet;

use rwgpuengine_core::hash::Hasher64;
use rwgpuengine_core::json::Json;
use rwgpuengine_core::rng::Rng;
use rwgpuengine_data::{Action, SceneLoader, Vfs};

use crate::events::{EventKind, Watch};
use crate::input::{InputFrame, Tap};
use crate::world::{NoSystems, Systems, World};

/// What a run reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reached {
    /// `World::state_hash` after the last step.
    pub state: u64,
    /// A hash over the state hash after every step.
    pub trace: u64,
    /// A hash over every event, in the order written.
    pub events: u64,
}

/// A generated world and the input that drives it.
pub struct Case {
    pub name: &'static str,
    pub vfs: Vfs,
    pub seed: u64,
    pub frames: u64,
    pub driver: Driver,
    /// Rust game code that changes the world between the rules, if any.
    pub meddler: Option<Meddler>,
}

impl Case {
    pub fn world(&self) -> World {
        let mut loader = SceneLoader::new(&self.vfs);
        let def = loader.load_scene("scenes/main.toml").unwrap_or_else(|| {
            let errors: Vec<_> = loader.diags.iter().filter(|d| d.is_error()).take(5).collect();
            panic!("{}: the generated scene does not load: {errors:#?}", self.name)
        });
        World::new(&def, self.seed)
    }

    /// Step the world to the last frame and hash what happened.
    pub fn run(&self, world: &mut World) -> Reached {
        let mut driver = self.driver.clone();
        let mut meddler = self.meddler.clone();
        let mut trace = Hasher64::new();
        let mut events = Hasher64::new();
        // Frame 0 too: the plates held and the copies counted when the world is built.
        trace.u64(world.state_hash());
        for _ in 0..self.frames {
            let before = world.events.len() as u64 + world.events.dropped();
            let input = driver.next(world);
            match meddler.as_mut() {
                Some(m) => world.tick(&input, m),
                None => world.tick(&input, &mut NoSystems),
            }
            trace.u64(world.state_hash());
            let new = (world.events.len() as u64 + world.events.dropped() - before) as usize;
            let all = world.events.all();
            assert!(new <= all.len(), "{}: one step wrote more events than the log keeps", self.name);
            for e in &all[all.len() - new..] {
                events.u64(e.frame).str(e.kind.name());
                events.u64(e.entity.map_or(u64::MAX, u64::from)).u64(e.other.map_or(u64::MAX, u64::from));
                events.str(e.signal.as_deref().unwrap_or("-")).str(&e.message).str(&e.data.to_compact());
            }
        }
        Reached { state: world.state_hash(), trace: trace.finish(), events: events.finish() }
    }
}

/// Input made with the engine's own random numbers. Tap targets depend on
/// the world as it is, so two engines that agree get the same input.
#[derive(Clone, Debug)]
pub struct Driver {
    rng: Rng,
    hold: Option<(Action, u32)>,
    /// Percent of free frames that start a direction (held 1 to 6 frames).
    pub moves: u32,
    /// Percent of frames with taps, and the most taps in one frame.
    pub taps: u32,
    pub most_taps: u32,
    /// The ground's size, for taps on ground cells.
    pub ground: Option<[i32; 2]>,
    /// Names that are tapped often (buttons and the like).
    pub favorites: Vec<String>,
}

impl Driver {
    pub fn new(seed: u64) -> Driver {
        Driver { rng: Rng::new(seed), hold: None, moves: 0, taps: 0, most_taps: 1, ground: None, favorites: Vec::new() }
    }

    pub fn next(&mut self, world: &World) -> InputFrame {
        let mut input = InputFrame::default();
        match self.hold {
            Some((a, left)) if left > 0 => {
                input.down.insert(a);
                self.hold = Some((a, left - 1));
            }
            _ => {
                self.hold = None;
                if self.rng.below(100) < self.moves {
                    let a = Action::DIRECTIONS[self.rng.below(4) as usize];
                    input.pressed.insert(a);
                    input.down.insert(a);
                    self.hold = Some((a, self.rng.below(6)));
                }
            }
        }
        if self.rng.below(100) < self.taps {
            for _ in 0..1 + self.rng.below(self.most_taps) {
                let tap = self.tap(world);
                input.taps.push(tap);
            }
        }
        input
    }

    fn tap(&mut self, world: &World) -> Tap {
        let n = world.entities.len() as u32;
        let kind = self.rng.below(10);
        let ground = self.ground.map(|[w, h]| [self.rng.range_i32(0, w - 1), self.rng.range_i32(0, h - 1)]);
        // 0..=4 the ground (when there is one), 5..=6 a favorite, else any entity.
        let id = match kind {
            0..=4 if ground.is_some() => None,
            5..=6 if !self.favorites.is_empty() => {
                let name = &self.favorites[self.rng.below(self.favorites.len() as u32) as usize];
                world.find(name)
            }
            _ if n > 0 => {
                // Prefer a copy of a prefab: those are the ones connections listen to.
                let mut pick = self.rng.below(n);
                for _ in 0..4 {
                    if world.entities[pick as usize].prefab.is_some() {
                        break;
                    }
                    pick = self.rng.below(n);
                }
                Some(pick)
            }
            _ => None,
        };
        match id {
            Some(id) if world.entities[id as usize].enabled => {
                Tap { x: id as f32, y: 1.0, hit: Some(id), under_disabled: Vec::new(), ground: None }
            }
            // A disabled entity cannot be hit: the tap goes through to the ground.
            Some(id) => Tap { x: id as f32, y: 2.0, hit: None, under_disabled: vec![id], ground },
            None => {
                let [x, y] = ground.unwrap_or([0, 0]);
                Tap { x: x as f32, y: y as f32, hit: None, under_disabled: Vec::new(), ground }
            }
        }
    }
}

/// Rust game code as a game might write it (`Systems`): each step it flips an
/// entity on or off, puts one somewhere else, shoves one, makes one emit, and
/// asks the world's own questions about space. What it is told goes into the
/// event log, so the answers are part of what a run reached.
#[derive(Clone, Debug)]
pub struct Meddler {
    rng: Rng,
    /// Prefabs it asks `can_place` about.
    prefabs: Vec<&'static str>,
}

impl Meddler {
    pub fn new(seed: u64, prefabs: &[&'static str]) -> Meddler {
        Meddler { rng: Rng::new(seed), prefabs: prefabs.to_vec() }
    }
}

impl Systems for Meddler {
    fn tick(&mut self, world: &mut World, _input: &InputFrame) {
        for _ in 0..3 {
            let n = world.entities.len() as u32;
            let id = self.rng.below(n);
            let cell = world.entities[id as usize].cell();
            let near = [cell[0] + self.rng.range_i32(-2, 2), cell[1] + self.rng.range_i32(-2, 2)];
            match self.rng.below(9) {
                0 => {
                    let on = world.entities[id as usize].enabled;
                    world.get_mut(id).expect("entity").enabled = !on;
                }
                1 => {
                    let z = world.entities[id as usize].pos[2];
                    world.get_mut(id).expect("entity").pos = [near[0] as f32, near[1] as f32, z];
                }
                2 => {
                    let (dx, dy) = Action::DIRECTIONS[self.rng.below(4) as usize].delta().expect("direction");
                    let moved = world.try_move(id, dx, dy, "shove");
                    world.log(EventKind::Custom, Some(id), None, format!("shoved: {moved}"), Json::object());
                }
                3 => world.emit(id, "tapped"),
                4 => {
                    let told = format!("{:?}", world.blocker_at(near, id));
                    world.log(EventKind::Custom, Some(id), None, told, Json::object().with("cell", near));
                }
                5 => {
                    let told = format!("{:?}", world.occupants_of(id));
                    world.log(EventKind::Custom, Some(id), None, told, Json::object());
                }
                6 => {
                    let prefab = self.prefabs[self.rng.below(self.prefabs.len() as u32) as usize];
                    let told = match world.can_place(prefab, near) {
                        Ok(cells) => format!("{prefab} fits on {cells:?}"),
                        Err(r) => format!("{prefab}: {} at {:?} by {:?}: {}", r.reason, r.cell, r.by, r.why),
                    };
                    world.log(EventKind::Custom, None, None, told, Json::object().with("cell", near));
                }
                7 => {
                    // Half a cell: where it stands is decided by rounding.
                    let p = world.entities[id as usize].pos;
                    world.get_mut(id).expect("entity").pos = [p[0] + 0.5, p[1] - 0.5, p[2]];
                }
                _ => {
                    let told = world.is_listened(Some(id), "tapped");
                    world.log(EventKind::Custom, Some(id), None, format!("listened: {told}"), Json::object());
                }
            }
        }
    }
}

fn project(scene: String, prefabs: &[(&str, &str)]) -> Vfs {
    let mut vfs = Vfs::new();
    vfs.insert("scenes/main.toml", scene);
    for (name, text) in prefabs {
        vfs.insert(format!("prefabs/{name}.toml"), *text);
    }
    vfs
}

/// The 3D study's town (proposal 3.5): `n` actors on every other cell of a
/// square ground, each trying one step per frame.
pub fn town(name: &'static str, n: usize, frames: u64) -> Case {
    let mut side = 2;
    while (side / 2) * (side / 2) < n {
        side += 2;
    }
    let mut scene = format!("[ground]\nsize = [{side}, {side}]\n");
    for i in 0..n {
        let (x, y) = (2 * (i % (side / 2)), 2 * (i / (side / 2)));
        scene.push_str(&format!("\n[[entity]]\npos = [{x}, {y}]\nwander = {{ step_frames = 1 }}\n"));
    }
    Case { name, vfs: project(scene, &[]), seed: 7, frames, driver: Driver::new(1), meddler: None }
}

/// A plate under every other cell and as many wanderers: every step, each
/// plate asks who stands on it, and many signals are sent.
pub fn plates(name: &'static str, n: usize, frames: u64) -> Case {
    let mut side = 2;
    while (side / 2) * (side / 2) < n {
        side += 2;
    }
    let mut scene = format!(
        "[ground]\nsize = [{side}, {side}]\n\n[[entity]]\nname = \"hud\"\nprops = {{ on = 0, off = 0, big = false, spin = 0 }}\n"
    );
    for i in 0..n {
        let (x, y) = (2 * (i % (side / 2)), 2 * (i / (side / 2)));
        scene.push_str(&format!("\n[[entity]]\nprefab = \"plate\"\npos = [{x}, {y}]\n"));
        scene.push_str(&format!("\n[[entity]]\npos = [{}, {y}]\nwander = {{ step_frames = 1 }}\n", x + 1));
    }
    scene.push_str(
        r#"
[[connect]]
from_prefab = "plate"
signal = "pressed"
to = "hud"
action = "add"
prop = "on"

[[connect]]
from_prefab = "plate"
signal = "released"
actions = [{ action = "add", to = "hud", prop = "off" }, { action = "toggle", to = "self" }, { action = "toggle", to = "self" }]

[[connect]]
from = "hud"
signal = "changed"
to = "hud"
action = "set"
prop = "big"
value = true
when = [{ entity = "hud", prop = "on", at_least = 50 }]

# While `on` is 40 to 140 this feeds itself: the signal limit cuts it (E0301).
[[connect]]
from = "hud"
signal = "changed"
to = "hud"
action = "add"
prop = "spin"
when = [{ entity = "hud", prop = "on", at_least = 40 }, { entity = "hud", prop = "on", at_most = 140 }]
"#,
    );
    let vfs = project(scene, &[("plate", "tile = {}\ntrigger = {}\n")]);
    Case { name, vfs, seed: 21, frames, driver: Driver::new(2), meddler: None }
}

/// How many of each thing a [`crowd`] has, per 1,000 cells of ground.
#[derive(Clone, Copy, Debug)]
pub struct Mix {
    pub walls: u32,
    pub halls: u32,
    pub crates: u32,
    pub movers: u32,
    pub patrols: u32,
    pub weights: u32,
    pub wanderers: u32,
    pub plates: u32,
    pub pads: u32,
    pub coins: u32,
    pub trees: u32,
    pub flowers: u32,
    /// Percent of things placed without asking whether the cell is free.
    pub overlap: u32,
    pub coins_at_start: i64,
}

impl Mix {
    /// Four cells in ten are taken: moves meet something most of the time.
    pub const CROWD: Mix = Mix {
        walls: 60,
        halls: 8,
        crates: 60,
        movers: 16,
        patrols: 30,
        weights: 12,
        wanderers: 80,
        plates: 50,
        pads: 12,
        coins: 20,
        trees: 20,
        flowers: 20,
        overlap: 3,
        coins_at_start: 40,
    };
    /// The same, with a third of the things put on cells that are taken.
    pub const PACKED: Mix = Mix { overlap: 35, wanderers: 120, crates: 90, ..Mix::CROWD };
    /// Little to start with and a lot of money: the taps build the world.
    pub const BUILDER: Mix = Mix {
        walls: 10,
        halls: 2,
        crates: 6,
        movers: 4,
        patrols: 4,
        weights: 2,
        wanderers: 10,
        plates: 8,
        pads: 2,
        coins: 4,
        trees: 6,
        flowers: 6,
        overlap: 0,
        coins_at_start: 100_000,
    };
}

const CROWD_PREFABS: &[(&str, &str)] = &[
    ("wall", "block = {}\nsolid = true\n"),
    ("hall", "footprint = [2, 2]\nblock = { height = 1.5 }\nsolid = true\n"),
    ("tree", "sprite = {}\nsolid = true\n"),
    ("flower", "sprite = { shape = \"diamond\" }\n"),
    ("coin", "sprite = {}\ntrigger = { on_enter = \"collected\" }\n"),
    ("hut", "footprint = [2, 2]\nblock = {}\nsolid = true\n"),
    ("crate", "block = { height = 0.5 }\npushable = true\n"),
    ("walker", "sprite = {}\nwander = { step_frames = 2 }\n"),
    ("pad", "footprint = [2, 1]\ntile = {}\ntrigger = { on_enter = \"padded\", on_exit = \"unpadded\" }\n"),
    ("button", "sprite = { shape = \"rect\" }\nlabel = { text = \"?\" }\n"),
];

const TOOLS: [(&str, i64); 5] = [("hut", 3), ("tree", 1), ("crate", 1), ("walker", 2), ("pad", 1)];

/// A ground of terrains with everything the rules know on it, wired so that
/// plates open gates, switch actors on and off, pay and count, and taps
/// build, clear and toggle.
pub fn crowd(name: &'static str, seed: u64, size: [i32; 2], mix: Mix, frames: u64, driver: Driver) -> Case {
    let [w, h] = size;
    let mut rng = Rng::new(seed);
    let per = |n: u32| ((w * h) as u32 * n).div_ceil(1000) as usize;
    let mut s = String::new();

    // The ground: grass, with water and sand where nothing can be built.
    s.push_str("[ground]\nmap = [\n");
    for _ in 0..h {
        let row: String = (0..w)
            .map(|_| match rng.below(100) {
                0..=5 => 'w',
                6..=11 => 's',
                _ => 'g',
            })
            .collect();
        s.push_str(&format!("  \"{row}\",\n"));
    }
    s.push_str("]\n\n[[terrain]]\nchar = \"g\"\nname = \"grass\"\n\n[[terrain]]\nchar = \"w\"\nname = \"water\"\n");
    s.push_str("build = false\n\n[[terrain]]\nchar = \"s\"\nname = \"sand\"\nbuild = false\n");

    // Where things go. A cell is handed out once, except for `overlap`.
    let mut taken: BTreeSet<[i32; 2]> = BTreeSet::new();
    let mut place = |rng: &mut Rng, fw: i32, fh: i32| -> [i32; 2] {
        let any = rng.below(100) < mix.overlap;
        let mut at = [0, 0];
        for _ in 0..200 {
            at = [rng.range_i32(0, w - fw), rng.range_i32(0, h - fh)];
            let cells = (0..fh).flat_map(|dy| (0..fw).map(move |dx| (dx, dy)));
            if any || cells.clone().all(|(dx, dy)| !taken.contains(&[at[0] + dx, at[1] + dy])) {
                break;
            }
        }
        for dy in 0..fh {
            for dx in 0..fw {
                taken.insert([at[0] + dx, at[1] + dy]);
            }
        }
        at
    };
    // Warnings a crowded scene is meant to have: stacked solids, an actor in a solid.
    let allow = "allow = [\"W0201\", \"W0202\", \"W0206\"]\n";
    let dirs = ["pressed", "hit", "stepped"];

    // The HUD and what holds the game's numbers.
    s.push_str(&format!(
        "\n[[entity]]\nname = \"bank\"\nscreen_pos = [8, 8]\nlabel = {{ text = \"{{coins}} {{tool}} {{note}}\" }}\n\
         props = {{ coins = {}, tool = \"hut\", note = \"\", tries = 0, built = 0, on = 0, off = 0, speed = 1.5, \
         flag = false }}\n",
        mix.coins_at_start
    ));
    s.push_str(
        "\n[[entity]]\nname = \"job\"\nscreen_pos = [8, 40]\nlabel = { text = \"{huts} {trees} {crates}\" }\n\
         counts = { huts = \"hut\", trees = \"tree\", crates = \"crate\", walkers = \"walker\", pads = \"pad\", \
         flowers = \"flower\", coins = \"coin\" }\nprops = { ready = false }\n",
    );
    s.push_str("\n[[entity]]\nname = \"hint\"\nprops = { msg = \"\" }\n");
    for (i, (tool, _)) in TOOLS.iter().enumerate() {
        s.push_str(&format!(
            "\n[[entity]]\nname = \"btn_{tool}\"\nprefab = \"button\"\nscreen_pos = [{}, 440]\n",
            8 + 120 * i
        ));
    }

    // Things that stay put.
    for i in 0..per(mix.walls) {
        let [x, y] = place(&mut rng, 1, 1);
        let z = [0, 0, 0, 1, 2][rng.below(5) as usize];
        s.push_str(&format!("\n[[entity]]\nprefab = \"wall\"\npos = [{x}, {y}, {z}]\n{allow}"));
        if i % 7 == 0 {
            s.push_str("enabled = false\n");
        }
    }
    for _ in 0..per(mix.halls) {
        let [x, y] = place(&mut rng, 2, 2);
        s.push_str(&format!("\n[[entity]]\nprefab = \"hall\"\npos = [{x}, {y}]\n{allow}"));
    }
    let gates = per(mix.walls).div_ceil(6).max(3);
    for i in 0..gates {
        let [x, y] = place(&mut rng, 1, 1);
        s.push_str(&format!("\n[[entity]]\nname = \"gate_{i}\"\nprefab = \"wall\"\npos = [{x}, {y}]\n{allow}"));
        if i % 3 == 1 {
            s.push_str("enabled = false\n");
        }
    }
    for kind in ["tree", "flower", "coin"] {
        let n = match kind {
            "tree" => per(mix.trees),
            "flower" => per(mix.flowers),
            _ => per(mix.coins),
        };
        let cells: Vec<String> = (0..n)
            .map(|_| {
                let [x, y] = place(&mut rng, 1, 1);
                format!("[{x}, {y}]")
            })
            .collect();
        if !cells.is_empty() {
            s.push_str(&format!("\n[[entity]]\nprefab = \"{kind}\"\ncells = [{}]\n{allow}", cells.join(", ")));
        }
    }
    // Outside the ground: nothing walks there, but things can stand there.
    s.push_str(&format!("\n[[entity]]\nprefab = \"wall\"\npos = [-2, 3]\n{allow}"));
    s.push_str(&format!("\n[[entity]]\nprefab = \"wall\"\npos = [{}, 2]\n{allow}", w + 1));
    s.push_str(&format!("\n[[entity]]\nname = \"far_plate\"\npos = [-1, 1]\ntile = {{}}\ntrigger = {{}}\n{allow}"));
    s.push_str(&format!("\n[[entity]]\nname = \"stray\"\npos = [-1, 2]\nwander = {{ step_frames = 1 }}\n{allow}"));
    // Takes no space: a spawn on its cell is placed the first time.
    let [x, y] = place(&mut rng, 1, 1);
    s.push_str(&format!("\n[[entity]]\nname = \"seeder\"\npos = [{x}, {y}]\nprops = {{ sown = 0 }}\n"));

    // Things that move, or are moved.
    let crates = per(mix.crates);
    let named_crates = crates.div_ceil(4);
    for i in 0..crates {
        let [x, y] = place(&mut rng, 1, 1);
        let name = if i < named_crates { format!("name = \"crate_{i}\"\n") } else { String::new() };
        s.push_str(&format!("\n[[entity]]\n{name}prefab = \"crate\"\npos = [{x}, {y}]\n{allow}"));
    }
    let movers = per(mix.movers).max(2);
    for i in 0..movers {
        let [x, y] = place(&mut rng, 1, 1);
        let step = 1 + rng.below(3);
        s.push_str(&format!(
            "\n[[entity]]\nname = \"m_{i}\"\npos = [{x}, {y}]\nsprite = {{}}\nmover = {{ step_frames = {step} }}\n{allow}"
        ));
    }
    for _ in 0..per(mix.patrols) {
        let [x, y] = place(&mut rng, 1, 1);
        let stops = 2 + rng.below(3);
        let path: Vec<String> = (0..stops)
            .map(|_| {
                let px = (x + rng.range_i32(-5, 5)).clamp(0, w - 1);
                let py = (y + rng.range_i32(-5, 5)).clamp(0, h - 1);
                format!("[{px}, {py}]")
            })
            .collect();
        let mode = if rng.below(2) == 0 { "loop" } else { "pingpong" };
        s.push_str(&format!(
            "\n[[entity]]\npos = [{x}, {y}]\nsprite = {{}}\npatrol = {{ path = [{}], step_frames = {}, mode = \"{mode}\" }}\n{allow}",
            path.join(", "),
            1 + rng.below(4)
        ));
    }
    for _ in 0..per(mix.wanderers) {
        let [x, y] = place(&mut rng, 1, 1);
        let step = 1 + rng.below(3);
        let area = if rng.below(10) < 3 {
            let (a, b) = (rng.range_i32(1, 4), rng.range_i32(1, 4));
            format!(
                ", area = [[{}, {}], [{}, {}]]",
                (x - a).max(0),
                (y - b).max(0),
                (x + a).min(w - 1),
                (y + b).min(h - 1)
            )
        } else {
            String::new()
        };
        // Some stand off the middle of their cell, and some up in the air.
        let pos = match rng.below(10) {
            0 => format!("[{}.25, {}.75, 0]", x, (y - 1).max(0)),
            1 => format!("[{x}, {y}, 2]"),
            _ => format!("[{x}, {y}]"),
        };
        s.push_str(&format!(
            "\n[[entity]]\npos = {pos}\nsprite = {{}}\nwander = {{ step_frames = {step}{area} }}\n{allow}"
        ));
    }

    // Plates, pads and the weights that hold some of them from frame 0.
    let plates = per(mix.plates).max(3);
    let weights = per(mix.weights).max(2);
    let mut plate_cells = Vec::new();
    for i in 0..plates {
        let [x, y] = place(&mut rng, 1, 1);
        plate_cells.push([x, y]);
        let enter = dirs[i % dirs.len()];
        let exit = if enter == "pressed" { "released".to_string() } else { format!("un{enter}") };
        let only = match rng.below(10) {
            0 => format!(", only = \"crate_{}\"", rng.below(named_crates.max(1) as u32)),
            1 => format!(", only = \"m_{}\"", rng.below(movers as u32)),
            _ => String::new(),
        };
        s.push_str(&format!(
            "\n[[entity]]\nname = \"p_{i}\"\npos = [{x}, {y}]\ntile = {{}}\n\
             trigger = {{ on_enter = \"{enter}\", on_exit = \"{exit}\"{only} }}\n{allow}"
        ));
        if i % 11 == 5 {
            s.push_str("enabled = false\n");
        }
        // Now and then a second plate under the first.
        if i % 9 == 4 {
            s.push_str(&format!("\n[[entity]]\nname = \"under_{i}\"\npos = [{x}, {y}]\ntrigger = {{}}\n{allow}"));
        }
    }
    let pads = per(mix.pads).max(2);
    for i in 0..pads {
        let (fw, fh) = [(2, 1), (2, 2), (1, 3)][i % 3];
        let [x, y] = place(&mut rng, fw, fh);
        s.push_str(&format!(
            "\n[[entity]]\nname = \"pad_{i}\"\npos = [{x}, {y}]\nfootprint = [{fw}, {fh}]\ntile = {{}}\n\
             trigger = {{ on_enter = \"padded\", on_exit = \"unpadded\" }}\n{allow}"
        ));
    }
    for i in 0..weights {
        // On a plate: it is held when the weight is switched on.
        let [x, y] = plate_cells[rng.below(plates as u32) as usize];
        let off = if i % 2 == 0 { "enabled = false\n" } else { "" };
        s.push_str(&format!(
            "\n[[entity]]\nname = \"wt_{i}\"\npos = [{x}, {y}]\npatrol = {{ path = [[{x}, {y}]] }}\n{off}{allow}"
        ));
    }

    // The toolbar and the ground: choose what a tap on the ground builds.
    for (tool, cost) in TOOLS {
        s.push_str(&format!(
            "\n[[connect]]\nfrom = \"btn_{tool}\"\nsignal = \"tapped\"\nactions = [\n  \
             {{ action = \"set\", to = \"bank\", prop = \"tool\", value = \"{tool}\" }},\n  \
             {{ action = \"set\", to = \"hint\", prop = \"msg\", value = \"\" }},\n]\n"
        ));
        s.push_str(&format!(
            "\n[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"{tool}\"\n\
             cost = {{ entity = \"bank\", prop = \"coins\", amount = {cost} }}\n\
             when = [{{ entity = \"bank\", prop = \"tool\", equals = \"{tool}\" }}]\n"
        ));
    }
    for reason in ["edge", "terrain", "occupied", "funds"] {
        s.push_str(&format!(
            "\n[[connect]]\nfrom = \"ground\"\nsignal = \"refused\"\nto = \"hint\"\naction = \"set\"\nprop = \"msg\"\n\
             value = \"{reason}\"\nwhen = [{{ reason = \"{reason}\" }}]\n"
        ));
    }
    s.push_str(
        r#"
[[connect]]
from = "ground"
signal = "refused"
to = "bank"
action = "add"
prop = "tries"

[[connect]]
from_prefab = "hut"
signal = "spawned"
to = "bank"
action = "add"
prop = "built"

[[connect]]
from_prefab = "hut"
signal = "tapped"
actions = [{ action = "disable", to = "self" }, { action = "add", to = "bank", prop = "coins", value = 3 }]

[[connect]]
from_prefab = "tree"
signal = "tapped"
actions = [{ action = "disable", to = "self" }, { action = "add", to = "bank", prop = "coins", value = 1 }]

[[connect]]
from_prefab = "flower"
signal = "tapped"
to = "self"
action = "toggle"

[[connect]]
from_prefab = "walker"
signal = "tapped"
to = "self"
action = "toggle"

[[connect]]
from_prefab = "crate"
signal = "tapped"
to = "self"
action = "toggle"

[[connect]]
from_prefab = "coin"
signal = "collected"
actions = [{ action = "add", to = "bank", prop = "coins", value = 2 }, { action = "disable", to = "self" }]

[[connect]]
from_prefab = "pad"
signal = "padded"
to = "bank"
action = "add"
prop = "on"

[[connect]]
from_prefab = "pad"
signal = "unpadded"
to = "bank"
action = "add"
prop = "off"

[[connect]]
from = "bank"
signal = "changed"
to = "bank"
action = "set"
prop = "flag"
value = true
when = [{ entity = "bank", prop = "coins", at_most = 10 }]

[[connect]]
from = "bank"
signal = "changed"
to = "bank"
action = "set"
prop = "flag"
value = false
when = [{ entity = "bank", prop = "coins", at_least = 11 }]

[[connect]]
from = "job"
signal = "changed"
to = "job"
action = "set"
prop = "ready"
value = true
when = [{ entity = "job", prop = "huts", at_least = 2 }]

[[connect]]
from = "m_0"
signal = "tapped"
action = "spawn"
prefab = "tree"

[[connect]]
from = "m_0"
signal = "refused"
to = "bank"
action = "add"
prop = "tries"

[[connect]]
from = "seeder"
signal = "tapped"
actions = [{ action = "spawn", prefab = "crate" }, { action = "add", to = "self", prop = "sown" }]
"#,
    );

    // Every plate and pad does something, most of it on a condition.
    let action = |rng: &mut Rng, from: &str| -> String {
        let switch = ["enable", "disable", "toggle"][rng.below(3) as usize];
        match rng.below(12) {
            0..=2 => format!("{{ action = \"{switch}\", to = \"gate_{}\" }}", rng.below(gates as u32)),
            3..=4 => format!("{{ action = \"{switch}\", to = \"wt_{}\" }}", rng.below(weights as u32)),
            5 => format!("{{ action = \"toggle\", to = \"crate_{}\" }}", rng.below(named_crates.max(1) as u32)),
            6 => format!("{{ action = \"toggle\", to = \"m_{}\" }}", rng.below(movers as u32)),
            7 => format!("{{ action = \"add\", to = \"bank\", prop = \"coins\", value = {} }}", rng.range_i32(-2, 3)),
            8 => "{ action = \"add\", to = \"bank\", prop = \"speed\", value = 0.25 }".to_string(),
            9 => format!("{{ action = \"set\", to = \"bank\", prop = \"note\", value = \"{from}\" }}"),
            10 => format!("{{ action = \"{switch}\", to = \"p_{}\" }}", rng.below(plates as u32)),
            _ => format!("{{ action = \"spawn\", prefab = \"{}\" }}", ["crate", "tree"][rng.below(2) as usize]),
        }
    };
    let condition = |rng: &mut Rng| -> String {
        let yes = rng.below(2) == 0;
        match rng.below(7) {
            0 => format!("{{ entity = \"p_{}\", occupied = {yes} }}", rng.below(plates as u32)),
            1 => format!("{{ entity = \"pad_{}\", occupied = {yes} }}", rng.below(pads as u32)),
            2 => format!("{{ entity = \"gate_{}\", enabled = {yes} }}", rng.below(gates as u32)),
            3 => format!("{{ entity = \"bank\", prop = \"coins\", at_least = {} }}", rng.range_i32(0, 30)),
            4 => format!("{{ entity = \"bank\", prop = \"coins\", at_most = {} }}", rng.range_i32(20, 60)),
            5 => format!("{{ entity = \"crate_{}\", occupied = {yes} }}", rng.below(named_crates.max(1) as u32)),
            _ => "{ entity = \"bank\", prop = \"tool\", equals = \"hut\" }".to_string(),
        }
    };
    let wire = |rng: &mut Rng, s: &mut String, from: &str, signal: &str| {
        let actions: Vec<String> = (0..1 + rng.below(3)).map(|_| action(rng, from)).collect();
        let when: Vec<String> = (0..rng.below(3)).map(|_| condition(rng)).collect();
        s.push_str(&format!(
            "\n[[connect]]\nfrom = \"{from}\"\nsignal = \"{signal}\"\nactions = [{}]\n",
            actions.join(", ")
        ));
        if !when.is_empty() {
            s.push_str(&format!("when = [{}]\n", when.join(", ")));
        }
    };
    for i in 0..plates {
        let from = format!("p_{i}");
        let enter = dirs[i % dirs.len()];
        let exit = if enter == "pressed" { "released".to_string() } else { format!("un{enter}") };
        wire(&mut rng, &mut s, &from, enter);
        if rng.below(2) == 0 {
            wire(&mut rng, &mut s, &from, enter);
        }
        if rng.below(3) > 0 {
            wire(&mut rng, &mut s, &from, &exit);
        }
        if i % 5 == 0 {
            s.push_str(&format!(
                "\n[[connect]]\nfrom = \"{from}\"\nsignal = \"refused\"\nto = \"bank\"\naction = \"add\"\nprop = \"tries\"\n"
            ));
        }
    }
    for i in 0..pads {
        let from = format!("pad_{i}");
        wire(&mut rng, &mut s, &from, "padded");
        wire(&mut rng, &mut s, &from, "unpadded");
    }

    let mut driver = driver;
    driver.ground = Some(size);
    driver.favorites =
        TOOLS.iter().map(|(t, _)| format!("btn_{t}")).chain(["m_0".to_string(), "seeder".to_string()]).collect();
    Case { name, vfs: project(s, CROWD_PREFABS), seed, frames, driver, meddler: None }
}

/// No ground: nothing has an edge, cells go below zero and far away.
pub fn open(name: &'static str, seed: u64, n: usize, frames: u64) -> Case {
    let mut rng = Rng::new(seed);
    let mut s = String::from(
        "[[entity]]\nname = \"tally\"\nprops = { on = 0, off = 0, sown = 0, tries = 0 }\n\
         counts = { crates = \"crate\" }\n",
    );
    let span = ((n as f32).sqrt() as i32).max(4);
    let at = |rng: &mut Rng| [rng.range_i32(-span, span), rng.range_i32(-span, span)];
    for i in 0..n {
        let [x, y] = at(&mut rng);
        match i % 8 {
            0 => s.push_str(&format!(
                "\n[[entity]]\nname = \"m_{i}\"\npos = [{x}, {y}]\nsprite = {{}}\nmover = {{ step_frames = 1 }}\n\
                 allow = [\"W0206\"]\n"
            )),
            1 | 2 => s.push_str(&format!("\n[[entity]]\nprefab = \"crate\"\npos = [{x}, {y}]\nallow = [\"W0201\"]\n")),
            3 => s.push_str(&format!("\n[[entity]]\nprefab = \"plate\"\npos = [{x}, {y}]\n")),
            4 => s.push_str(&format!(
                "\n[[entity]]\npos = [{x}, {y}]\nblock = {{}}\nsolid = true\nallow = [\"W0201\", \"W0206\"]\n"
            )),
            5 => s.push_str(&format!(
                "\n[[entity]]\npos = [{x}, {y}]\nwander = {{ step_frames = 1, area = [[{}, {}], [{}, {}]] }}\n\
                 allow = [\"W0206\"]\n",
                x - 3,
                y - 3,
                x + 3,
                y + 3
            )),
            _ => s.push_str(&format!(
                "\n[[entity]]\npos = [{x}, {y}]\nwander = {{ step_frames = 1 }}\nallow = [\"W0206\"]\n"
            )),
        }
    }
    // Far from everything, and off the middle of a cell.
    s.push_str("\n[[entity]]\nname = \"hermit\"\npos = [-1000.5, 2000.25, 3]\nwander = { step_frames = 1 }\n");
    s.push_str("\n[[entity]]\nname = \"far_plate\"\nprefab = \"plate\"\npos = [-1001, 2000]\n");
    s.push_str("\n[[entity]]\nname = \"seeder\"\npos = [0, 0]\nprops = { sown = 0 }\n");
    s.push_str(
        r#"
[[connect]]
from_prefab = "plate"
signal = "pressed"
to = "tally"
action = "add"
prop = "on"

[[connect]]
from_prefab = "plate"
signal = "released"
to = "tally"
action = "add"
prop = "off"

[[connect]]
from = "seeder"
signal = "tapped"
actions = [{ action = "spawn", prefab = "crate" }, { action = "add", to = "tally", prop = "sown" }]

[[connect]]
from = "seeder"
signal = "refused"
to = "tally"
action = "add"
prop = "tries"

[[connect]]
from_prefab = "crate"
signal = "tapped"
to = "self"
action = "toggle"
"#,
    );
    let vfs =
        project(s, &[("crate", "block = { height = 0.5 }\npushable = true\n"), ("plate", "tile = {}\ntrigger = {}\n")]);
    let mut driver = Driver::new(seed ^ 0x55);
    driver.moves = 80;
    driver.taps = 40;
    driver.most_taps = 2;
    driver.favorites = vec!["seeder".to_string()];
    Case { name, vfs, seed, frames, driver, meddler: None }
}

/// The same world with Rust game code changing it every step.
fn meddled(case: Case, seed: u64) -> Case {
    Case { meddler: Some(Meddler::new(seed, &["hut", "crate", "tree", "pad", "nothing"])), ..case }
}

fn driver(seed: u64, moves: u32, taps: u32, most_taps: u32) -> Driver {
    Driver { moves, taps, most_taps, ..Driver::new(seed) }
}

/// Every world of the oracle. The first group is small enough for the gate;
/// the second is run by hand in a release build
/// (`cargo test --release -p rwgpuengine_sim oracle -- --ignored`).
pub fn cases(large: bool) -> Vec<Case> {
    if !large {
        vec![
            town("town-200", 200, 120),
            plates("plates-150", 150, 120),
            crowd("crowd-20x16", 11, [20, 16], Mix::CROWD, 300, driver(101, 70, 40, 3)),
            crowd("packed-16x16", 12, [16, 16], Mix::PACKED, 300, driver(102, 70, 40, 3)),
            crowd("builder-24x24", 13, [24, 24], Mix::BUILDER, 200, driver(103, 30, 100, 4)),
            open("open-160", 14, 160, 300),
            meddled(crowd("meddled-20x16", 15, [20, 16], Mix::CROWD, 300, driver(105, 70, 40, 3)), 305),
            meddled(open("meddled-open-120", 16, 120, 300), 306),
        ]
    } else {
        vec![
            town("town-1000", 1_000, 200),
            town("town-5000", 5_000, 100),
            plates("plates-2000", 2_000, 100),
            crowd("crowd-64x48", 21, [64, 48], Mix::CROWD, 300, driver(201, 70, 40, 3)),
            crowd("crowd-100x100", 22, [100, 100], Mix::CROWD, 150, driver(202, 70, 40, 3)),
            crowd("packed-48x48", 23, [48, 48], Mix::PACKED, 300, driver(203, 70, 40, 3)),
            crowd("builder-96x96", 24, [96, 96], Mix::BUILDER, 400, driver(204, 30, 100, 6)),
            open("open-3000", 25, 3_000, 200),
            meddled(crowd("meddled-64x48", 26, [64, 48], Mix::PACKED, 300, driver(206, 70, 40, 3)), 406),
        ]
    }
}

/// What the scanning engine reached: (world, state, trace, events), and the
/// events written by default, with the per-step budget (ADR 0027).
pub const PINS: &[(&str, u64, u64, u64, u64)] = &[
    ("town-200", 0xbcb126c911ec1d60, 0x8d652fd907881576, 0x7605128bf2fddbd9, 0x7605128bf2fddbd9),
    ("plates-150", 0x033840459082a5dc, 0x90874438d6209ce2, 0xfb6de082c3b231e6, 0xfb6de082c3b231e6),
    ("crowd-20x16", 0x4bd97066a674eac8, 0x223d0d5c779240c6, 0x6a3d0aee7a6159a4, 0x6a3d0aee7a6159a4),
    ("packed-16x16", 0x24483a11f3272468, 0xd5f71786a3bad9c7, 0xdd6cddf453a8fa1b, 0xdd6cddf453a8fa1b),
    ("builder-24x24", 0xdb54e65991485080, 0xa25a275ca8d124c9, 0xb209d5b3c794ac0c, 0xb209d5b3c794ac0c),
    ("open-160", 0x8364a5353f6c9ac3, 0x791716c5ec6277d9, 0x4a3bf61b113c5409, 0x4a3bf61b113c5409),
    ("meddled-20x16", 0x2587b976e351e3fb, 0x6d60e6bce3571e66, 0xacf5968ef5ad207a, 0xacf5968ef5ad207a),
    ("meddled-open-120", 0x4f8c9fea183e405b, 0x225c0721e91695ec, 0xec7cf70bedc3b01b, 0xec7cf70bedc3b01b),
    ("town-1000", 0x6401405746d706dd, 0x0a01a8ce660b000c, 0xef1b96f2d3965445, 0xf6c61d739509aa1f),
    ("town-5000", 0xb691b266267e088e, 0xe521215e038f6cdc, 0x08c19a6be2e71201, 0xc9f87916f33740eb),
    ("plates-2000", 0x14fcf0235f82b130, 0xa131cee1a94e3029, 0x77f63e2bc2b7f453, 0x091ec32267266d09),
    ("crowd-64x48", 0x62f53a447330d2fa, 0x01422db5b36663a0, 0x89ed56267a82db17, 0x89ed56267a82db17),
    ("crowd-100x100", 0xab50569fd41ae6da, 0xccd9d2b83f033dbc, 0x1c8f6b13ab4048a3, 0x116ecfb67635d345),
    ("packed-48x48", 0x047b563e868f1544, 0x2722758a61bcd388, 0xbe5471d4af222841, 0xbe5471d4af222841),
    ("builder-96x96", 0xd51702604af84a36, 0xeaab256787678238, 0x74acc7bdd0f71447, 0x74acc7bdd0f71447),
    ("open-3000", 0xe293c0bab798f7a1, 0x4b32556fa9593828, 0xf8b4f5d3d93bdd88, 0xeee8c4eadb5ca88f),
    ("meddled-64x48", 0x82e88958898d2a30, 0xc2901092676e3c56, 0xf48d5d5970d75e28, 0xf48d5d5970d75e28),
];

fn check(cases: Vec<Case>) {
    for case in cases {
        let pin = PINS.iter().find(|p| p.0 == case.name).unwrap_or_else(|| panic!("{}: no pin", case.name));
        let mut world = case.world();
        world.events.set_watch(Watch::everything());
        let reached = case.run(&mut world);
        let want = Reached { state: pin.1, trace: pin.2, events: pin.3 };
        assert_eq!(reached, want, "{}: not what the scanning engine reached, every event written", case.name);
        let mut world = case.world();
        let reached = case.run(&mut world);
        let want = Reached { state: pin.1, trace: pin.2, events: pin.4 };
        assert_eq!(reached, want, "{}: not what the engine reaches by default", case.name);
    }
}

#[test]
fn small_worlds_reach_what_the_scanning_engine_reached() {
    check(cases(false));
}

#[test]
#[ignore = "large worlds: run in a release build"]
fn large_worlds_reach_what_the_scanning_engine_reached() {
    check(cases(true));
}

/// Prints the pins and what each world exercises. Only for recording them
/// with an engine that is known to be right; see the module's first lines.
#[test]
#[ignore = "prints the pins; it checks nothing"]
fn print_pins() {
    let large = std::env::var("ORACLE_LARGE").is_ok();
    for case in cases(large) {
        let mut all = case.world();
        all.events.set_watch(Watch::everything());
        let every = case.run(&mut all);
        let mut world = case.world();
        let start = world.entities.len();
        let r = case.run(&mut world);
        assert_eq!((every.state, every.trace), (r.state, r.trace), "{}: what is written changed the world", case.name);
        println!(
            "    (\"{}\", 0x{:016x}, 0x{:016x}, 0x{:016x}, 0x{:016x}),",
            case.name, r.state, r.trace, every.events, r.events
        );
        let mut reasons: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for e in world.events.all() {
            let reason = e.data.get("reason").and_then(|r| r.as_str()).unwrap_or("");
            let cause = if e.data.get("cause").and_then(|c| c.as_str()) == Some("pushed") { ":pushed" } else { "" };
            let prefab = match e.kind {
                EventKind::Spawned => e.data.get("prefab").and_then(|p| p.as_str()).unwrap_or(""),
                _ => "",
            };
            let sep = if reason.is_empty() && prefab.is_empty() { "" } else { ":" };
            *reasons.entry(format!("{}{sep}{reason}{prefab}{cause}", e.kind.name())).or_default() += 1;
        }
        let kept: Vec<String> = reasons.iter().map(|(k, n)| format!("{k} {n}")).collect();
        eprintln!(
            "// {}: {} entities at the start, {} at the end, {} frames; events kept: {} (dropped {}): {}",
            case.name,
            start,
            world.entities.len(),
            case.frames,
            world.events.len(),
            world.events.dropped(),
            kept.join(", ")
        );
    }
}
