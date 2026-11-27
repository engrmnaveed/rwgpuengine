//! The cell index against the scans (ADR 0026): random worlds, random steps,
//! and every answer of the index compared with the answer found by looking at
//! every entity.
//!
//! The worlds here are not scene files. Their entities are put together from
//! parts at random, so they hold what no loader lets through and Rust game
//! code can still make: an actor on the screen, a footprint of nothing or of
//! less than nothing, a pushable three cells wide, things far outside the
//! ground and off the middle of their cell. Between the rules of every step,
//! game code changes entities through `World::get_mut` in every way it can.
//!
//! With `check_against_scans` on, each question a rule asks is answered both
//! ways and the world panics where the answers differ. The tests also ask the
//! questions themselves on random cells, and after every step the index must
//! be what a rebuild from the entities gives.

use rwgpuengine_core::hash::Hasher64;
use rwgpuengine_core::rng::Rng;
use rwgpuengine_data::{Action, EntityDef, PatrolDef, PatrolMode, PropValue, SceneDef, SceneLoader, Vfs, WanderDef};

use crate::input::InputFrame;
use crate::oracle::Driver;
use crate::world::{Systems, World};

const PREFABS: [&str; 4] = ["hut", "crate", "walker", "pad"];
const NAMES: [&str; 5] = ["hud", "a", "b", "c", "d"];

/// A scene through the loader, for its parts: one entity with each component,
/// the spawn templates and the connections. `ground` is its `[ground]` table.
fn parts(ground: &str) -> SceneDef {
    let scene = format!(
        r#"{ground}
[[entity]]
name = "hud"
props = {{ n = 0, m = 0 }}
counts = {{ huts = "hut", crates = "crate", walkers = "walker" }}

[[entity]]
name = "seen"
tile = {{}}
block = {{}}
sprite = {{}}
label = {{ text = "x" }}
trigger = {{}}

[[entity]]
name = "a"
props = {{ n = 0 }}

[[entity]]
name = "b"

[[entity]]
name = "c"

[[entity]]
name = "d"

[[connect]]
from = "ground"
signal = "tapped"
actions = [{{ action = "spawn", prefab = "hut" }}, {{ action = "add", to = "hud", prop = "n" }}]
when = [{{ entity = "a", occupied = false }}]

[[connect]]
from = "ground"
signal = "tapped"
action = "spawn"
prefab = "crate"
when = [{{ entity = "a", occupied = true }}]

[[connect]]
from = "ground"
signal = "refused"
actions = [{{ action = "add", to = "hud", prop = "m" }}, {{ action = "toggle", to = "b" }}]

[[connect]]
from = "b"
signal = "tapped"
action = "spawn"
prefab = "walker"

[[connect]]
from = "a"
signal = "tapped"
actions = [{{ action = "spawn", prefab = "pad" }}, {{ action = "toggle", to = "b" }}]

[[connect]]
from = "a"
signal = "pressed"
actions = [{{ action = "toggle", to = "b" }}, {{ action = "disable", to = "c" }}, {{ action = "add", to = "hud", prop = "n" }}]

[[connect]]
from = "b"
signal = "pressed"
to = "c"
action = "enable"
when = [{{ entity = "d", occupied = true }}]

[[connect]]
from = "c"
signal = "released"
to = "a"
action = "toggle"

[[connect]]
from = "d"
signal = "tapped"
to = "self"
action = "toggle"

[[connect]]
from_prefab = "crate"
signal = "tapped"
to = "self"
action = "toggle"

[[connect]]
from_prefab = "pad"
signal = "pressed"
actions = [{{ action = "add", to = "hud", prop = "m" }}, {{ action = "toggle", to = "d" }}]

[[connect]]
from_prefab = "hut"
signal = "spawned"
to = "hud"
action = "add"
prop = "m"

[[connect]]
from_prefab = "walker"
signal = "tapped"
to = "self"
action = "disable"

# After the prefabs' `tapped`: a named copy is heard by both, in file order.
[[connect]]
from = "c"
signal = "tapped"
to = "hud"
action = "add"
prop = "n"

[[connect]]
from_prefab = "hut"
signal = "tapped"
to = "self"
action = "toggle"
"#
    );
    let mut vfs = Vfs::new();
    vfs.insert("scenes/main.toml", scene);
    vfs.insert("prefabs/hut.toml", "footprint = [2, 2]\nblock = {}\nsolid = true\n");
    vfs.insert("prefabs/crate.toml", "block = {}\npushable = true\n");
    vfs.insert("prefabs/walker.toml", "sprite = {}\nwander = { step_frames = 1 }\n");
    vfs.insert("prefabs/pad.toml", "footprint = [2, 1]\ntile = {}\ntrigger = {}\n");
    let mut loader = SceneLoader::new(&vfs);
    loader.load_scene("scenes/main.toml").unwrap_or_else(|| panic!("{:#?}", loader.diags))
}

fn chance(rng: &mut Rng, percent: u32) -> bool {
    rng.below(100) < percent
}

/// A cell on or near the ground (or around zero without one); now and then far away.
fn some_cell(rng: &mut Rng, size: [i32; 2]) -> [i32; 2] {
    if chance(rng, 2) {
        return [rng.range_i32(-900_000, 900_000), rng.range_i32(-900_000, 900_000)];
    }
    [rng.range_i32(-2, size[0] + 1), rng.range_i32(-2, size[1] + 1)]
}

/// A footprint: one cell mostly, sometimes several, sometimes none or less.
fn some_footprint(rng: &mut Rng) -> [i32; 2] {
    if chance(rng, 80) { [1, 1] } else { [rng.range_i32(-1, 3), rng.range_i32(-1, 3)] }
}

/// The behaviors and flags of an entity, any mix of them.
fn dress(rng: &mut Rng, e: &mut EntityDef, seen: &EntityDef, size: [i32; 2]) {
    e.solid = chance(rng, 20);
    e.pushable = chance(rng, 15);
    e.tile = seen.tile.filter(|_| chance(rng, 15));
    e.block = seen.block.filter(|_| chance(rng, 15));
    e.sprite = seen.sprite.filter(|_| chance(rng, 15));
    e.label = seen.label.clone().filter(|_| chance(rng, 5));
    e.mover = chance(rng, 10).then(|| rwgpuengine_data::MoverDef { step_frames: 1 + rng.below(2) });
    e.patrol = chance(rng, 10).then(|| PatrolDef {
        path: (0..1 + rng.below(3)).map(|_| some_cell(rng, size)).collect(),
        step_frames: 1 + rng.below(2),
        mode: if chance(rng, 50) { PatrolMode::Loop } else { PatrolMode::PingPong },
    });
    e.wander = chance(rng, 25).then(|| WanderDef {
        step_frames: 1 + rng.below(2),
        area: chance(rng, 30).then(|| {
            let [x, y] = some_cell(rng, size);
            [[x - 2, y - 2], [x + 2, y + 2]]
        }),
    });
    e.trigger = seen.trigger.clone().filter(|_| chance(rng, 20)).map(|mut t| {
        t.only = chance(rng, 20).then(|| NAMES[rng.below(NAMES.len() as u32) as usize].to_string());
        t
    });
    e.screen_pos = chance(rng, 8).then_some([4.0, 4.0]);
    e.prefab = chance(rng, 30).then(|| PREFABS[rng.below(PREFABS.len() as u32) as usize].to_string());
}

fn some_pos(rng: &mut Rng, size: [i32; 2]) -> [f32; 3] {
    let [x, y] = some_cell(rng, size);
    // Off the middle of the cell, and on the line between two cells.
    let off = [0.0, 0.0, 0.0, 0.25, -0.25, 0.5][rng.below(6) as usize];
    [x as f32 + off, y as f32 - off, rng.below(3) as f32]
}

/// A world of `n` random entities on a ground of `size`, or on none.
fn random_world(seed: u64, size: Option<[i32; 2]>, n: usize) -> World {
    let mut rng = Rng::new(seed);
    let ground = size.map_or(String::new(), |[w, h]| format!("[ground]\nsize = [{w}, {h}]\n"));
    let mut scene = parts(&ground);
    let size = size.unwrap_or([8, 8]);
    let (hud, seen) = (scene.entities[0].clone(), scene.entities[1].clone());
    let bare = scene.entities[3].clone();
    scene.entities = (0..n)
        .map(|i| {
            let mut e = EntityDef { name: None, ..bare.clone() };
            dress(&mut rng, &mut e, &seen, size);
            e.pos = some_pos(&mut rng, size);
            e.footprint = some_footprint(&mut rng);
            e.enabled = chance(&mut rng, 80);
            if let Some(name) = NAMES.get(i) {
                e.name = Some(name.to_string());
                e.props = vec![("n".to_string(), PropValue::Int(0)), ("m".to_string(), PropValue::Int(0))];
            }
            if i == 0 {
                e.props = hud.props.clone();
                e.counts = hud.counts.clone();
            }
            e
        })
        .collect();
    World::new(&scene, seed)
}

/// Rust game code that changes entities in every way `get_mut` allows, and
/// asks the world's questions in between.
struct Meddler {
    rng: Rng,
    seen: EntityDef,
    size: [i32; 2],
    /// A hash over every answer it was given.
    told: Hasher64,
}

impl Meddler {
    fn new(seed: u64, size: Option<[i32; 2]>) -> Meddler {
        let seen = parts("").entities[1].clone();
        Meddler { rng: Rng::new(seed), seen, size: size.unwrap_or([8, 8]), told: Hasher64::new() }
    }

    fn change(&mut self, world: &mut World) {
        let id = self.rng.below(world.entities.len() as u32);
        let mut dressed = EntityDef { ..self.seen.clone() };
        dress(&mut self.rng, &mut dressed, &self.seen.clone(), self.size);
        let mut e = world.get_mut(id).expect("entity");
        match self.rng.below(12) {
            0 | 1 => e.enabled = !e.enabled,
            2 | 3 => e.pos = some_pos(&mut self.rng, self.size),
            4 => e.footprint = some_footprint(&mut self.rng),
            5 => (e.solid, e.pushable) = (dressed.solid, dressed.pushable),
            6 => (e.tile, e.block, e.sprite) = (dressed.tile, dressed.block, dressed.sprite),
            7 => e.screen_pos = dressed.screen_pos,
            8 => e.prefab = dressed.prefab,
            9 => {
                // Take a behavior away, or its timers back to the start.
                e.mover = e.mover.take().filter(|_| chance(&mut self.rng, 50));
                e.patrol = e.patrol.take().filter(|_| chance(&mut self.rng, 50));
                e.wander = e.wander.take().filter(|_| chance(&mut self.rng, 50));
            }
            10 => e.trigger = e.trigger.take().filter(|_| chance(&mut self.rng, 50)),
            _ => {
                // Several things at once, while the entity is held.
                e.enabled = true;
                e.pos = some_pos(&mut self.rng, self.size);
                e.solid = !e.solid;
            }
        }
    }

    /// Ask what a rule would ask, on a random cell. The world compares the
    /// index's answer with the scan's; the answer goes into `told`.
    fn ask(&mut self, world: &mut World) {
        let id = self.rng.below(world.entities.len() as u32);
        let cell = some_cell(&mut self.rng, self.size);
        match self.rng.below(6) {
            0 => {
                let b = world.blocker_at(cell, id);
                self.told.u64(b.map_or(u64::MAX, |(id, _)| u64::from(id))).str(b.map_or("", |(_, why)| why));
            }
            1 => {
                let who = world.occupants_of(id);
                self.told.u64(who.len() as u64);
                who.iter().for_each(|o| {
                    self.told.u32(*o);
                });
            }
            2 => {
                let prefab = PREFABS[self.rng.below(PREFABS.len() as u32) as usize];
                match world.can_place(prefab, cell) {
                    Ok(cells) => self.told.u64(cells.len() as u64),
                    Err(r) => self.told.str(r.reason).u64(r.by.map_or(u64::MAX, u64::from)),
                };
            }
            3 => {
                let signal = ["tapped", "pressed", "released", "spawned", "refused"][self.rng.below(5) as usize];
                let from = chance(&mut self.rng, 80).then_some(id);
                self.told.bool(world.is_listened(from, signal));
            }
            4 => {
                let (dx, dy) = Action::DIRECTIONS[self.rng.below(4) as usize].delta().expect("direction");
                self.told.bool(world.try_move(id, dx, dy, "shove"));
            }
            _ => world.emit(id, ["tapped", "pressed", "released"][self.rng.below(3) as usize]),
        }
    }
}

impl Systems for Meddler {
    fn tick(&mut self, world: &mut World, _input: &InputFrame) {
        for _ in 0..3 {
            self.change(world);
            self.ask(world);
        }
    }
}

fn driver(seed: u64, size: Option<[i32; 2]>) -> Driver {
    let mut d = Driver::new(seed);
    // Taps on the ground need a ground with a cell.
    (d.moves, d.taps, d.most_taps, d.ground) = (80, 60, 3, size.filter(|[w, h]| *w > 0 && *h > 0));
    d.favorites = NAMES.iter().map(|n| n.to_string()).collect();
    d
}

/// Step a random world with the scans checking every answer. Returns the
/// state hash, a hash of every event and of every answer given to game code.
fn run_checked(seed: u64, size: Option<[i32; 2]>, n: usize, steps: usize, check: bool) -> (u64, u64, u64) {
    let mut world = random_world(seed, size, n);
    assert_eq!(world.check_against_scans(check), check, "the check is compiled into the tests");
    world.verify_index().unwrap_or_else(|why| panic!("seed {seed}, a new world: {why}"));
    let mut meddler = Meddler::new(seed ^ 0xfeed, size);
    let mut driver = driver(seed ^ 0xd00d, size);
    for step in 0..steps {
        // Game code between steps too, as `on_start` or a test would.
        meddler.change(&mut world);
        meddler.ask(&mut world);
        let input = driver.next(&world);
        world.tick(&input, &mut meddler);
        world.verify_index().unwrap_or_else(|why| panic!("seed {seed}, after step {step}: {why}"));
    }
    let mut events = Hasher64::new();
    for e in world.events.all() {
        events.u64(e.frame).str(e.kind.name()).str(&e.message).str(&e.data.to_compact());
    }
    (world.state_hash(), events.finish(), meddler.told.finish())
}

/// The sizes the random worlds take: crowded grounds, a ground of one row, a
/// ground of nothing, and no ground.
const GROUNDS: [Option<[i32; 2]>; 6] = [Some([8, 6]), Some([5, 5]), Some([12, 1]), Some([0, 0]), None, Some([20, 16])];

/// 16,000 random steps on 400 random worlds: every answer of the index is
/// the scan's, and the index is what a rebuild gives after every step.
#[test]
fn the_index_answers_as_the_scans_do_on_random_worlds() {
    for seed in 0..400u64 {
        let size = GROUNDS[seed as usize % GROUNDS.len()];
        let n = 20 + (seed as usize * 7) % 90;
        run_checked(1000 + seed, size, n, 40, true);
    }
}

/// The check changes nothing: a world reaches the same state, writes the same
/// events and gives game code the same answers with it on or off.
#[test]
fn checking_against_the_scans_changes_nothing() {
    for seed in 0..40u64 {
        let size = GROUNDS[seed as usize % GROUNDS.len()];
        assert_eq!(run_checked(5000 + seed, size, 60, 40, true), run_checked(5000 + seed, size, 60, 40, false));
    }
}

/// A world stepped, and the same world with its index thrown away and built
/// again every few steps, are the same world.
#[test]
fn a_world_stepped_and_a_world_rebuilt_agree() {
    for seed in 0..60u64 {
        let size = GROUNDS[seed as usize % GROUNDS.len()];
        let mut kept = random_world(9000 + seed, size, 70);
        let mut rebuilt = kept.clone();
        let (mut meddler_a, mut meddler_b) = (Meddler::new(seed, size), Meddler::new(seed, size));
        let (mut driver_a, mut driver_b) = (driver(seed, size), driver(seed, size));
        for step in 0..40 {
            if step % 3 == 0 {
                rebuilt.rebuild_index();
            }
            let (a, b) = (driver_a.next(&kept), driver_b.next(&rebuilt));
            assert_eq!(a, b, "seed {seed}, step {step}: the worlds are tapped alike");
            kept.tick(&a, &mut meddler_a);
            rebuilt.tick(&b, &mut meddler_b);
            assert_eq!(kept.state_hash(), rebuilt.state_hash(), "seed {seed}, step {step}");
            assert_eq!(kept.events.len(), rebuilt.events.len(), "seed {seed}, step {step}");
        }
        assert_eq!(meddler_a.told.finish(), meddler_b.told.finish(), "seed {seed}: the same answers");
        kept.verify_index().unwrap_or_else(|why| panic!("seed {seed}: {why}"));
    }
}

/// The oracle's small worlds once more, with every answer compared with a
/// scan on the way: the same pins, and no answer differs.
#[test]
fn the_oracle_s_small_worlds_pass_with_every_answer_checked() {
    for case in crate::oracle::cases(false) {
        let mut world = case.world();
        assert!(world.check_against_scans(true));
        let reached = case.run(&mut world);
        let pin = crate::oracle::PINS.iter().find(|p| p.0 == case.name).expect("a pin");
        assert_eq!((reached.state, reached.trace, reached.events), (pin.1, pin.2, pin.4), "{}", case.name);
        world.verify_index().unwrap_or_else(|why| panic!("{}: {why}", case.name));
    }
}

/// `verify_index` does notice: an index that was not told of a change is not
/// what a rebuild gives.
#[test]
fn a_stale_index_is_found() {
    let mut world = random_world(77, Some([8, 6]), 40);
    assert_eq!(world.verify_index(), Ok(()));
    let id = (0..world.entities.len() as u32).find(|id| world.entities[*id as usize].enabled).expect("one enabled");
    // Let go of the entity without telling the world: what `get_mut` exists to prevent.
    let mut held = world.get_mut(id).expect("entity");
    held.enabled = false;
    held.solid = true;
    held.sprite = None;
    std::mem::forget(held);
    let stale = world.verify_index().expect_err("the index was not told");
    assert!(stale.contains(&format!("entity #{id}")), "{stale}");
    world.rebuild_index();
    assert_eq!(world.verify_index(), Ok(()));
}
