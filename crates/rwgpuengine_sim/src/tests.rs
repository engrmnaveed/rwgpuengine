use rwgpuengine_data::{Action, PropValue, SceneLoader, Vfs};

use crate::events::EventKind;
use crate::input::{ActionSet, InputFrame, Tap};
use crate::world::{NoSystems, World};

const SCENE: &str = r##"
[ground]
size = [6, 6]

[[entity]]
name = "player"
pos = [0, 0]
sprite = {}
mover = { step_frames = 4 }

[[entity]]
name = "wall"
pos = [2, 0]
block = {}
solid = true

[[entity]]
name = "plate"
pos = [0, 2]
tile = {}
trigger = {}

[[entity]]
name = "gate"
pos = [1, 2]
block = {}
solid = true

[[entity]]
name = "bell"
pos = [5, 5]
sprite = {}

[[connect]]
from = "plate"
signal = "pressed"
to = "gate"
action = "disable"
"##;

fn world(scene: &str, seed: u64) -> World {
    let mut vfs = Vfs::new();
    vfs.insert("scenes/main.toml", scene);
    let mut loader = SceneLoader::new(&vfs);
    let def = loader.load_scene("scenes/main.toml").unwrap_or_else(|| panic!("{:#?}", loader.diags));
    World::new(&def, seed)
}

fn press(a: Action) -> InputFrame {
    let mut s = ActionSet::EMPTY;
    s.insert(a);
    InputFrame { pressed: s, down: s, taps: vec![] }
}

fn step(w: &mut World, input: InputFrame) {
    w.tick(&input, &mut NoSystems);
}

fn idle(w: &mut World, n: usize) {
    for _ in 0..n {
        step(w, InputFrame::default());
    }
}

fn kinds(w: &World) -> Vec<&'static str> {
    w.events.all().iter().map(|e| e.kind.name()).collect()
}

#[test]
fn every_event_kind_is_documented_in_order() {
    let documented: Vec<EventKind> = crate::events::EVENT_DOCS.iter().map(|d| d.kind).collect();
    assert_eq!(documented, EventKind::ALL);
}

#[test]
fn event_kind_names_match_the_schema() {
    let names: Vec<&str> = EventKind::ALL.iter().map(|k| k.name()).collect();
    assert_eq!(names, rwgpuengine_data::schema::EVENT_KINDS);
}

#[test]
fn mover_moves_and_reports_why_it_cannot() {
    let mut w = world(SCENE, 1);
    step(&mut w, press(Action::Right));
    assert_eq!(w.by_name("player").unwrap().cell(), [1, 0]);

    // Pressed again during the cooldown: ignored, with the reason logged.
    step(&mut w, press(Action::Right));
    let last = w.events.all().last().unwrap();
    assert_eq!(last.kind, EventKind::Blocked);
    assert!(last.message.contains("still waiting"), "{}", last.message);

    idle(&mut w, 4);
    step(&mut w, press(Action::Right));
    let last = w.events.all().last().unwrap();
    assert_eq!(last.kind, EventKind::Blocked);
    assert!(last.message.contains("blocked by solid 'wall' (#1)"), "{}", last.message);
    assert_eq!(w.by_name("player").unwrap().cell(), [1, 0]);

    idle(&mut w, 4);
    step(&mut w, press(Action::Up));
    assert!(w.events.all().last().unwrap().message.contains("outside the 6x6 ground"));
}

#[test]
fn plate_opens_gate_through_a_connection() {
    let mut w = world(SCENE, 1);
    assert!(w.by_name("gate").unwrap().enabled);
    step(&mut w, press(Action::Down));
    idle(&mut w, 4);
    step(&mut w, press(Action::Down));
    assert_eq!(w.by_name("player").unwrap().cell(), [0, 2]);
    assert!(!w.by_name("gate").unwrap().enabled);
    let k = kinds(&w);
    assert!(k.ends_with(&["moved", "trigger", "signal", "action"]), "{k:?}");

    // The disabled gate no longer blocks.
    idle(&mut w, 4);
    step(&mut w, press(Action::Right));
    assert_eq!(w.by_name("player").unwrap().cell(), [1, 2]);

    // Leaving the plate emits `released`, which nothing listens to.
    let released =
        w.events.all().iter().find(|e| e.signal.as_deref() == Some("released") && e.kind == EventKind::Signal);
    assert!(released.unwrap().message.contains("nothing is connected"));
}

#[test]
fn taps_emit_tapped_and_misses_explain_themselves() {
    let mut w = world(SCENE, 1);
    let bell = w.find("bell").unwrap();
    let gate = w.find("gate").unwrap();
    step(
        &mut w,
        InputFrame {
            taps: vec![Tap { x: 1.0, y: 2.0, hit: Some(bell), under_disabled: vec![], ground: None }],
            ..Default::default()
        },
    );
    assert_eq!(kinds(&w), vec!["tap", "signal"]);
    step(
        &mut w,
        InputFrame {
            taps: vec![Tap { x: 3.0, y: 4.0, hit: None, under_disabled: vec![gate], ground: None }],
            ..Default::default()
        },
    );
    let miss = w.events.all().last().unwrap();
    assert_eq!(miss.kind, EventKind::TapMiss);
    assert!(miss.message.contains("disabled 'gate' (#3) is under the pointer"), "{}", miss.message);
}

#[test]
fn patrols_loop_and_pingpong() {
    let scene = |mode: &str| {
        format!(
            "[ground]\nsize = [5, 5]\n[[entity]]\nname = \"g\"\npos = [0, 0]\npatrol = {{ path = [[0, 0], [2, 0]], step_frames = 1, mode = \"{mode}\" }}\n"
        )
    };
    for mode in ["loop", "pingpong"] {
        let mut w = world(&scene(mode), 1);
        let mut cells = Vec::new();
        for _ in 0..6 {
            idle(&mut w, 1);
            cells.push(w.by_name("g").unwrap().cell()[0]);
        }
        // Two points: loop and pingpong walk the same way.
        assert_eq!(cells, vec![1, 2, 1, 0, 1, 2], "{mode}");
    }
}

#[test]
fn same_seed_same_world_different_seed_different_world() {
    let scene = "[ground]\nsize = [8, 8]\n[[entity]]\nname = \"c\"\npos = [4, 4]\nwander = { step_frames = 1 }\n";
    let run = |seed| {
        let mut w = world(scene, seed);
        idle(&mut w, 200);
        (w.state_hash(), w.by_name("c").unwrap().cell())
    };
    assert_eq!(run(7), run(7));
    assert_ne!(run(7).0, run(8).0);
}

#[test]
fn wander_respects_its_area() {
    let scene = "[ground]\nsize = [8, 8]\n[[entity]]\nname = \"c\"\npos = [1, 1]\nwander = { step_frames = 1, area = [[0, 0], [1, 1]] }\n";
    let mut w = world(scene, 3);
    for _ in 0..300 {
        idle(&mut w, 1);
        let [x, y] = w.by_name("c").unwrap().cell();
        assert!((0..=1).contains(&x) && (0..=1).contains(&y));
    }
}

#[test]
fn connection_loops_are_cut_and_reported() {
    let scene = r#"
[[entity]]
name = "a"
pos = [0, 0]
sprite = {}

[[entity]]
name = "b"
pos = [1, 0]
sprite = {}
"#;
    let mut w = world(scene, 1);
    // Wire a -> a manually: tapping `a` re-emits forever via custom code.
    struct Echo;
    impl crate::world::Systems for Echo {
        fn tick(&mut self, world: &mut World, _input: &InputFrame) {
            for _ in 0..1001 {
                world.emit(0, "ping");
            }
        }
    }
    w.tick(&InputFrame::default(), &mut Echo);
    let last = w.events.all().last().unwrap();
    assert_eq!(last.kind, EventKind::Error);
    assert!(last.message.contains("E0301"));
}

/// The behaviours the "stationary actor" and "counting in any order"
/// recipes in the project AGENTS.md rely on.
#[test]
fn stationary_actors_and_triggers_behave_as_documented() {
    let scene = r#"
[ground]
size = [4, 4]

[[entity]]
name = "plate"
pos = [0, 0]
tile = {}
trigger = {}

[[entity]]
name = "start_on"
pos = [0, 0]
patrol = { path = [[0, 0]] }

[[entity]]
name = "sense"
pos = [2, 2]
tile = {}
trigger = { on_enter = "bit_on", on_exit = "bit_off" }

[[entity]]
name = "bit"
pos = [2, 2]
patrol = { path = [[2, 2]], step_frames = 1 }
enabled = false
"#;
    let mut w = world(scene, 1);
    // Occupied at frame 0: no signal, ever. A stationary patrol never moves and logs nothing.
    idle(&mut w, 100);
    assert!(w.events.is_empty(), "{:?}", kinds(&w));
    assert_eq!(w.by_name("start_on").unwrap().cell(), [0, 0]);

    // Enabling an actor on a trigger's cell fires on_enter in the next step's trigger phase.
    let bit = w.find("bit").unwrap();
    w.get_mut(bit).unwrap().enabled = true;
    idle(&mut w, 1);
    let e = w.events.all().last().unwrap();
    assert_eq!((e.kind, e.signal.as_deref()), (EventKind::Signal, Some("bit_on")));
    w.get_mut(bit).unwrap().enabled = false;
    idle(&mut w, 1);
    assert_eq!(w.events.all().last().unwrap().signal.as_deref(), Some("bit_off"));
}

// ---------------------------------------------------------------------------
// Data logic v1 (ADR 0011)

/// Two plates held by stationary actors that can be switched on and off,
/// an AND door, coins counted into a HUD prop, and a status set at 2 coins.
const LOGIC: &str = r##"
[ground]
size = [8, 4]

[[entity]]
name = "player"
pos = [0, 0]
sprite = {}
mover = { step_frames = 4 }

[[entity]]
name = "plate_a"
pos = [1, 2]
tile = {}
trigger = {}

[[entity]]
name = "plate_b"
pos = [3, 2]
tile = {}
trigger = {}

[[entity]]
name = "weight_a"
pos = [1, 2]
enabled = false
patrol = { path = [[1, 2]] }

[[entity]]
name = "weight_b"
pos = [3, 2]
enabled = false
patrol = { path = [[3, 2]] }

[[entity]]
name = "door"
pos = [5, 2]
block = {}
solid = true

[[entity]]
name = "coin1"
pos = [1, 0]
sprite = {}
trigger = { on_enter = "collected", only = "player" }

[[entity]]
name = "coin2"
pos = [2, 0]
sprite = {}
trigger = { on_enter = "collected", only = "player" }

[[entity]]
name = "hud"
screen_pos = [4, 4]
label = { text = "Coins {coins} {status}" }
props = { coins = 0, status = "" }

[[connect]]
from = "plate_a"
signal = "pressed"
to = "door"
action = "disable"
when = [{ entity = "plate_b", occupied = true }]

[[connect]]
from = "plate_b"
signal = "pressed"
to = "door"
action = "disable"
when = [{ entity = "plate_a", occupied = true }]

[[connect]]
from = "plate_a"
signal = "released"
to = "door"
action = "enable"

[[connect]]
from = "plate_b"
signal = "released"
to = "door"
action = "enable"

[[connect]]
from = "coin1"
signal = "collected"
to = "hud"
action = "add"
prop = "coins"

[[connect]]
from = "coin2"
signal = "collected"
to = "hud"
action = "add"
prop = "coins"

[[connect]]
from = "coin1"
signal = "collected"
to = "coin1"
action = "disable"

[[connect]]
from = "coin2"
signal = "collected"
to = "coin2"
action = "disable"

[[connect]]
from = "hud"
signal = "changed"
to = "hud"
action = "set"
prop = "status"
value = "rich"
when = [{ entity = "hud", prop = "coins", at_least = 2 }]
"##;

fn set_enabled(w: &mut World, name: &str, on: bool) {
    let id = w.find(name).unwrap();
    w.get_mut(id).unwrap().enabled = on;
}

#[test]
fn and_door_opens_only_while_both_plates_are_held_in_either_order() {
    for order in [["weight_a", "weight_b"], ["weight_b", "weight_a"]] {
        let mut w = world(LOGIC, 1);
        set_enabled(&mut w, order[0], true);
        idle(&mut w, 2);
        assert!(w.by_name("door").unwrap().enabled, "one plate is not enough");
        let skipped: Vec<_> = w.events.all().iter().filter(|e| e.kind == EventKind::Skipped).collect();
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].message.contains("occupied does not hold (nobody is on"), "{}", skipped[0].message);

        set_enabled(&mut w, order[1], true);
        idle(&mut w, 2);
        assert!(!w.by_name("door").unwrap().enabled, "both held: open ({order:?})");
        let action = w.events.all().iter().rev().find(|e| e.kind == EventKind::Action).unwrap();
        assert!(action.message.contains("; when '"), "{}", action.message);
        assert_eq!(action.data.pointer("when/0/held").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(action.data.get("changed").and_then(|v| v.as_bool()), Some(true));

        // Stepping off either closes it again.
        set_enabled(&mut w, order[0], false);
        idle(&mut w, 2);
        assert!(w.by_name("door").unwrap().enabled, "released: closed again");
    }
}

#[test]
fn add_counts_into_a_prop_and_changed_drives_conditions() {
    let mut w = world(LOGIC, 1);
    let hud = w.find("hud").unwrap();
    assert_eq!(w.get(hud).unwrap().label_text().as_deref(), Some("Coins 0 "));
    step(&mut w, press(Action::Right)); // onto coin1
    idle(&mut w, 1);
    assert_eq!(w.get(hud).unwrap().props.get("coins"), Some(&PropValue::Int(1)));
    assert!(!w.by_name("coin1").unwrap().enabled, "collected once");
    let add = w.events.all().iter().find(|e| e.kind == EventKind::Action && e.entity == Some(hud)).unwrap();
    assert!(add.message.contains("add coins 1: 0 -> 1"), "{}", add.message);
    assert_eq!(add.data.get("after"), Some(&rwgpuengine_core::json::Json::Int(1)));
    // changed -> set skipped at 1 coin.
    let skip = w.events.all().iter().find(|e| e.kind == EventKind::Skipped).unwrap();
    assert!(skip.message.contains("'hud'.coins >= 2 does not hold (coins is 1)"), "{}", skip.message);

    idle(&mut w, 4);
    step(&mut w, press(Action::Right)); // onto coin2
    idle(&mut w, 1);
    let h = w.get(hud).unwrap();
    assert_eq!(h.props.get("coins"), Some(&PropValue::Int(2)));
    assert_eq!(h.props.get("status"), Some(&PropValue::Str("rich".into())));
    assert_eq!(h.label_text().as_deref(), Some("Coins 2 rich"));

    // Walking back over the collected coins counts nothing more.
    idle(&mut w, 4);
    step(&mut w, press(Action::Left));
    idle(&mut w, 8);
    assert_eq!(w.get(hud).unwrap().props.get("coins"), Some(&PropValue::Int(2)));
}

#[test]
fn prop_changes_are_deterministic_and_hashed() {
    let run = || {
        let mut w = world(LOGIC, 3);
        step(&mut w, press(Action::Right));
        idle(&mut w, 3);
        w.state_hash()
    };
    assert_eq!(run(), run());
    let mut a = world(LOGIC, 3);
    let b = world(LOGIC, 3);
    let hud = a.find("hud").unwrap();
    a.get_mut(hud).unwrap().props.insert("coins".into(), PropValue::Int(5));
    assert_ne!(a.state_hash(), b.state_hash());
}

#[test]
fn a_changed_loop_is_cut_off_by_the_signal_limit() {
    let scene = r##"
[[entity]]
name = "n"
props = { v = 0 }

[[connect]]
from = "n"
signal = "tapped"
to = "n"
action = "add"
prop = "v"

[[connect]]
from = "n"
signal = "changed"
to = "n"
action = "add"
prop = "v"
"##;
    let mut w = world(scene, 1);
    let n = w.find("n").unwrap();
    let tap = Tap { x: 0.0, y: 0.0, hit: Some(n), under_disabled: vec![], ground: None };
    step(&mut w, InputFrame { taps: vec![tap.clone()], ..InputFrame::default() });
    assert!(w.events.all().iter().any(|e| e.kind == EventKind::Error), "{:?}", kinds(&w));
    // A `when` bound stops it instead.
    let bounded = scene.replace(
        "signal = \"changed\"\nto = \"n\"\naction = \"add\"\nprop = \"v\"",
        "signal = \"changed\"\nto = \"n\"\naction = \"add\"\nprop = \"v\"\nwhen = [{ entity = \"n\", prop = \"v\", at_most = 9 }]",
    );
    let mut w = world(&bounded, 1);
    step(&mut w, InputFrame { taps: vec![tap], ..InputFrame::default() });
    assert_eq!(w.get(n).unwrap().props.get("v"), Some(&PropValue::Int(10)), "adds while v <= 9");
    assert!(!w.events.all().iter().any(|e| e.kind == EventKind::Error));
}

#[test]
fn set_and_add_on_float_props() {
    let scene = r##"
[[entity]]
name = "n"
props = { speed = 1.5, flag = false }

[[connect]]
from = "n"
signal = "tapped"
to = "n"
action = "add"
prop = "speed"
value = -2

[[connect]]
from = "n"
signal = "tapped"
to = "n"
action = "set"
prop = "flag"
value = true
"##;
    let mut w = world(scene, 1);
    let n = w.find("n").unwrap();
    let tap = Tap { x: 0.0, y: 0.0, hit: Some(n), under_disabled: vec![], ground: None };
    step(&mut w, InputFrame { taps: vec![tap], ..InputFrame::default() });
    let e = w.get(n).unwrap();
    assert_eq!(e.props.get("speed"), Some(&PropValue::Float(-0.5)));
    assert_eq!(e.props.get("flag"), Some(&PropValue::Bool(true)));
}

// ---------------------------------------------------------------------------
// Pushables: crates that hold plates

const CRATES: &str = r##"
[ground]
size = [6, 4]

[[entity]]
name = "player"
pos = [0, 1]
sprite = {}
mover = { step_frames = 2 }

[[entity]]
name = "crate"
pos = [1, 1]
block = {}
pushable = true

[[entity]]
name = "plate"
pos = [2, 1]
tile = {}
trigger = {}

[[entity]]
name = "wall"
pos = [4, 1]
block = {}
solid = true

[[entity]]
name = "door"
pos = [5, 3]
block = {}
solid = true

[[entity]]
name = "crate2"
pos = [1, 3]
block = {}
pushable = true

[[entity]]
name = "crate3"
pos = [2, 3]
block = {}
pushable = true

[[entity]]
name = "guard"
pos = [5, 2]
sprite = {}
patrol = { path = [[5, 2], [5, 0]], step_frames = 1 }

[[connect]]
from = "plate"
signal = "pressed"
to = "door"
action = "disable"

[[connect]]
from = "plate"
signal = "released"
to = "door"
action = "enable"
"##;

fn walk(w: &mut World, a: Action, steps: usize) {
    for _ in 0..steps {
        step(w, press(a));
        idle(w, 2);
    }
}

#[test]
fn movers_push_crates_onto_plates_and_crates_hold_them() {
    let mut w = world(CRATES, 1);
    walk(&mut w, Action::Right, 1);
    assert_eq!(w.by_name("crate").unwrap().cell(), [2, 1], "pushed onto the plate");
    assert_eq!(w.by_name("player").unwrap().cell(), [1, 1]);
    let pushed =
        w.events.all().iter().find(|e| e.data.get("cause").and_then(|c| c.as_str()) == Some("pushed")).unwrap();
    assert!(
        pushed.message.contains("'crate' (#1) was pushed right from (1, 1) to (2, 1) by 'player'"),
        "{}",
        pushed.message
    );
    assert_eq!(pushed.other, w.find("player"));
    assert!(!w.by_name("door").unwrap().enabled, "the crate holds the plate");
    let trigger = w.events.all().iter().find(|e| e.kind == EventKind::Trigger).unwrap();
    assert!(trigger.message.contains("was entered by 'crate'"), "{}", trigger.message);

    // Push it off the plate: the player steps onto the plate in the same step,
    // so the plate never empties and nothing is released.
    walk(&mut w, Action::Right, 1);
    assert_eq!(w.by_name("crate").unwrap().cell(), [3, 1]);
    assert_eq!(w.by_name("player").unwrap().cell(), [2, 1]);
    assert!(!w.by_name("door").unwrap().enabled, "still held, by the player now");
    assert!(!w.events.all().iter().any(|e| e.signal.as_deref() == Some("released")));

    // Into the wall: refused with reason `push`, and nothing moves.
    walk(&mut w, Action::Right, 2);
    assert_eq!(w.by_name("crate").unwrap().cell(), [3, 1]);
    assert_eq!(w.by_name("player").unwrap().cell(), [2, 1]);
    let b = w.events.all().iter().rev().find(|e| e.kind == EventKind::Blocked).unwrap();
    assert_eq!(b.data.get("reason").and_then(|r| r.as_str()), Some("push"));
    assert_eq!(b.other, w.find("crate"));
    assert!(b.message.contains("cannot be pushed to (4, 1), blocked by solid 'wall'"), "{}", b.message);
}

#[test]
fn pushes_stop_at_edges_and_other_crates_and_patrols_cannot_push() {
    let mut w = world(CRATES, 1);
    // Down twice to (0, 3), then right into crate2, which has crate3 behind it.
    walk(&mut w, Action::Down, 1);
    walk(&mut w, Action::Down, 1);
    assert_eq!(w.by_name("player").unwrap().cell(), [0, 3]);
    walk(&mut w, Action::Right, 1);
    let b = w.events.all().iter().rev().find(|e| e.kind == EventKind::Blocked && e.entity == w.find("player")).unwrap();
    assert!(b.message.contains("blocked by another pushable 'crate3'"), "{}", b.message);
    assert_eq!(w.by_name("crate2").unwrap().cell(), [1, 3]);

    // A crate at the edge cannot leave the ground.
    let mut w = world(CRATES, 1);
    let c = w.find("crate").unwrap();
    w.get_mut(c).unwrap().pos = [1.0, 0.0, 0.0];
    walk(&mut w, Action::Right, 1); // (1, 1)
    walk(&mut w, Action::Up, 1);
    let b = w.events.all().iter().rev().find(|e| e.kind == EventKind::Blocked).unwrap();
    assert_eq!(b.data.get("push_blocked").and_then(|r| r.as_str()), Some("edge"), "{}", b.message);

    // The guard patrols (5, 2) -> (5, 0) through (5, 1): a crate there blocks it.
    let mut w = world(CRATES, 1);
    let c = w.find("crate").unwrap();
    w.get_mut(c).unwrap().pos = [5.0, 1.0, 0.0];
    idle(&mut w, 3);
    assert_eq!(w.by_name("crate").unwrap().cell(), [5, 1]);
    let b = w.events.all().iter().find(|e| e.kind == EventKind::Blocked && e.entity == w.find("guard")).unwrap();
    assert_eq!(b.data.get("reason").and_then(|r| r.as_str()), Some("solid"));
    assert!(b.message.contains("blocked by pushable 'crate' (#1) (only entities with a mover push)"), "{}", b.message);
}

#[test]
fn a_crate_on_a_plate_satisfies_occupied_conditions_from_frame_0() {
    let scene = format!(
        "{CRATES}\n[[entity]]\nname = \"bell\"\nprops = {{ rung = 0 }}\n\n[[connect]]\nfrom = \"bell\"\nsignal = \"tapped\"\nto = \"bell\"\naction = \"add\"\nprop = \"rung\"\nwhen = [{{ entity = \"plate\", occupied = true }}]\n"
    )
    .replace("name = \"crate\"\npos = [1, 1]", "name = \"crate\"\npos = [2, 1]");
    let mut w = world(&scene, 1);
    let bell = w.find("bell").unwrap();
    let tap = Tap { x: 0.0, y: 0.0, hit: Some(bell), under_disabled: vec![], ground: None };
    step(&mut w, InputFrame { taps: vec![tap], ..InputFrame::default() });
    assert_eq!(w.get(bell).unwrap().props.get("rung"), Some(&PropValue::Int(1)));
    assert!(w.by_name("door").unwrap().enabled, "occupied at frame 0: no `pressed` (documented)");
}

// ---------------------------------------------------------------------------
// Builder kit (ADR 0013)

const TOWN: &str = r##"
[ground]
map = ["gggw", "gggw", "gggs"]

[[terrain]]
char = "g"
name = "grass"

[[terrain]]
char = "w"
name = "water"
build = false

[[terrain]]
char = "s"
name = "sand"
build = false

[[entity]]
name = "town"
props = { coins = 7, built = 0 }

[[entity]]
name = "rock"
pos = [0, 2]
block = {}
solid = true

[[connect]]
from = "ground"
signal = "tapped"
action = "spawn"
prefab = "house"
when = [{ entity = "town", prop = "coins", at_least = 3 }]

[[connect]]
from_prefab = "house"
signal = "spawned"
to = "town"
action = "add"
prop = "coins"
value = -3

[[connect]]
from_prefab = "house"
signal = "tapped"
to = "self"
action = "disable"
"##;

fn town() -> World {
    let mut vfs = Vfs::new();
    vfs.insert("scenes/main.toml", TOWN);
    vfs.insert("prefabs/house.toml", "footprint = [2, 2]\nblock = {}\nsolid = true\n");
    let mut loader = SceneLoader::new(&vfs);
    let def = loader.load_scene("scenes/main.toml").unwrap_or_else(|| panic!("{:#?}", loader.diags));
    World::new(&def, 1)
}

fn ground_tap(cell: [i32; 2]) -> InputFrame {
    InputFrame {
        taps: vec![Tap { x: 0.0, y: 0.0, hit: None, under_disabled: vec![], ground: Some(cell) }],
        ..Default::default()
    }
}

fn last(w: &World, kind: EventKind) -> String {
    w.events.all().iter().rev().find(|e| e.kind == kind).map(|e| e.message.clone()).unwrap_or_default()
}

#[test]
fn placement_rules_edge_terrain_occupied() {
    let w = town();
    // Grass with room: the 2 x 2 footprint grows toward +x and +y.
    assert_eq!(w.can_place("house", [0, 0]), Ok(vec![[0, 0], [1, 0], [0, 1], [1, 1]]));
    // Reaches the water at (3, 0).
    let r = w.can_place("house", [2, 0]).unwrap_err();
    assert_eq!((r.reason, r.cell, r.terrain.as_deref()), ("terrain", [3, 0], Some("water")));
    // Covers the rock at (0, 2).
    let r = w.can_place("house", [0, 1]).unwrap_err();
    assert_eq!((r.reason, r.cell, r.by), ("occupied", [0, 2], Some(1)));
    // Off the bottom edge.
    let r = w.can_place("house", [1, 2]).unwrap_err();
    assert_eq!((r.reason, r.cell), ("edge", [1, 3]));
}

#[test]
fn ground_taps_spawn_pay_and_get_the_next_id() {
    let mut w = town();
    w.tick(&ground_tap([0, 0]), &mut NoSystems);
    assert_eq!(w.entities.len(), 3);
    let house = &w.entities[2];
    assert_eq!((house.prefab.as_deref(), house.cell(), house.footprint), (Some("house"), [0, 0], [2, 2]));
    assert_eq!(w.by_name("town").unwrap().props.get("coins"), Some(&PropValue::Int(4)));
    assert!(last(&w, EventKind::Spawned).contains("#2 (house) was spawned at (0, 0)"));
    // The house now blocks its four cells for movers and placements.
    assert!(w.blocker_at([1, 1], 99).is_some());
    // A refused placement costs nothing and says why.
    w.tick(&ground_tap([1, 1]), &mut NoSystems);
    assert_eq!(w.entities.len(), 3);
    assert!(
        last(&w, EventKind::Blocked).contains("cell (1, 1) is taken by #2 (house)"),
        "{}",
        last(&w, EventKind::Blocked)
    );
    assert_eq!(w.by_name("town").unwrap().props.get("coins"), Some(&PropValue::Int(4)));
    // Water.
    w.tick(&ground_tap([3, 1]), &mut NoSystems);
    assert!(last(&w, EventKind::Blocked).contains("is water"));
    // A second house: 1 coin left; a third is skipped by the `when`.
    w.tick(&ground_tap([1, 2]), &mut NoSystems); // edge
    assert!(last(&w, EventKind::Blocked).contains("outside the 4x3 ground"));
    w.tick(&ground_tap([2, 1]), &mut NoSystems); // (3, 1) is water
    w.tick(&ground_tap([2, 0]), &mut NoSystems); // water too
    assert_eq!(w.entities.len(), 3);
}

#[test]
fn from_prefab_and_self_target_the_emitter() {
    let mut w = town();
    w.tick(&ground_tap([0, 0]), &mut NoSystems);
    let tap_house = InputFrame {
        taps: vec![Tap { x: 0.0, y: 0.0, hit: Some(2), under_disabled: vec![], ground: None }],
        ..Default::default()
    };
    w.tick(&tap_house, &mut NoSystems);
    assert!(!w.entities[2].enabled, "the tapped house disables itself");
    // Its cells are free again.
    assert!(w.can_place("house", [0, 0]).is_ok());
}

#[test]
fn ground_taps_without_listeners_are_misses_with_the_cell() {
    let mut w = world(SCENE, 1);
    w.tick(&ground_tap([3, 3]), &mut NoSystems);
    let e = w.events.all().iter().find(|e| e.kind == EventKind::TapMiss).expect("tap_miss");
    assert!(e.message.contains("ground cell (3, 3)"), "{}", e.message);
    assert_eq!(e.data.get("cell").map(|c| c.to_compact()), Some("[3,3]".into()));
}

#[test]
fn multi_cell_triggers_watch_every_cell() {
    let scene = r##"
[ground]
size = [4, 4]

[[entity]]
name = "player"
pos = [0, 0]
sprite = {}
mover = { step_frames = 1 }

[[entity]]
name = "pad"
pos = [1, 0]
footprint = [2, 1]
tile = {}
trigger = {}

[[entity]]
name = "lamp"
pos = [3, 3]
sprite = {}
enabled = false

[[connect]]
from = "pad"
signal = "pressed"
to = "lamp"
action = "enable"
"##;
    let mut w = world(scene, 1);
    w.tick(&press(Action::Right), &mut NoSystems);
    assert!(w.by_name("lamp").unwrap().enabled, "entering the first cell presses it");
    w.tick(&press(Action::Right), &mut NoSystems);
    w.tick(&press(Action::Right), &mut NoSystems);
    assert_eq!(w.by_name("player").unwrap().cell(), [3, 0]);
    let released = w.events.all().iter().filter(|e| e.kind == EventKind::Trigger).count();
    assert_eq!(released, 2, "one press when entering (1, 0), one release when leaving (2, 0)");
}

#[test]
fn spawning_is_deterministic_and_hashed() {
    let run = || {
        let mut w = town();
        w.tick(&ground_tap([0, 0]), &mut NoSystems);
        w.tick(&ground_tap([2, 1]), &mut NoSystems);
        w.state_hash()
    };
    assert_eq!(run(), run());
    let mut w = town();
    let before = w.state_hash();
    w.tick(&InputFrame::default(), &mut NoSystems);
    let idle = w.state_hash();
    let mut w2 = town();
    w2.tick(&ground_tap([0, 0]), &mut NoSystems);
    assert_ne!(idle, w2.state_hash(), "a spawned entity changes the hash");
    assert_ne!(before, idle);
}

// ---------------------------------------------------------------------------
// Builder kit v2 (ADR 0014): refusal signals and count props

const FEEDBACK: &str = r##"
[ground]
map = ["ggggg", "ggggw", "ggggg"]

[[terrain]]
char = "g"
name = "grass"

[[terrain]]
char = "w"
name = "water"
build = false

[[entity]]
name = "hint"
props = { msg = "" }

[[entity]]
name = "job"
counts = { houses = "house", flowers = "flower" }
props = { ready = false }

[[entity]]
prefab = "flower"
cells = [[2, 2], [3, 2]]

[[connect]]
from = "ground"
signal = "tapped"
to = "hint"
action = "set"
prop = "msg"
value = ""

[[connect]]
from = "ground"
signal = "tapped"
action = "spawn"
prefab = "house"
when = [{ entity = "job", prop = "houses", at_most = 1 }]

[[connect]]
from = "ground"
signal = "refused"
to = "hint"
action = "set"
prop = "msg"
value = "no room"
when = [{ reason = "occupied" }]

[[connect]]
from = "ground"
signal = "refused"
to = "hint"
action = "set"
prop = "msg"
value = "not on water"
when = [{ reason = "terrain" }]

[[connect]]
from = "ground"
signal = "refused"
to = "hint"
action = "set"
prop = "msg"
value = "off the edge"
when = [{ reason = "edge" }]

[[connect]]
from = "job"
signal = "changed"
to = "job"
action = "set"
prop = "ready"
value = true
when = [{ entity = "job", prop = "houses", at_least = 2 }]

[[connect]]
from_prefab = "flower"
signal = "tapped"
to = "self"
action = "toggle"
"##;

fn feedback() -> World {
    let mut vfs = Vfs::new();
    vfs.insert("scenes/main.toml", FEEDBACK);
    vfs.insert("prefabs/house.toml", "footprint = [2, 2]\nblock = {}\nsolid = true\n");
    vfs.insert("prefabs/flower.toml", "sprite = {}\n");
    let mut loader = SceneLoader::new(&vfs);
    let def = loader.load_scene("scenes/main.toml").unwrap_or_else(|| panic!("{:#?}", loader.diags));
    World::new(&def, 1)
}

fn prop(w: &World, name: &str, prop: &str) -> PropValue {
    w.by_name(name).and_then(|e| e.props.get(prop)).cloned().unwrap_or(PropValue::Int(-1))
}

fn this_frame(w: &World, kind: EventKind) -> Vec<String> {
    let f = w.frame - 1;
    w.events.all().iter().filter(|e| e.frame == f && e.kind == kind).map(|e| e.message.clone()).collect()
}

#[test]
fn refused_spawns_emit_refused_with_their_reason() {
    let mut w = feedback();
    let msg = |w: &World| prop(w, "hint", "msg");
    // Reaches the water at (4, 1).
    w.tick(&ground_tap([3, 0]), &mut NoSystems);
    assert_eq!(msg(&w), PropValue::Str("not on water".into()));
    let blocked = last(&w, EventKind::Blocked);
    assert!(blocked.ends_with("; the ground emits 'refused'"), "{blocked}");
    let signal = this_frame(&w, EventKind::Signal);
    assert!(
        signal.iter().any(|m| m == "the ground emitted 'refused' (reason 'terrain') to 3 connection(s)"),
        "{signal:?}"
    );
    let skipped = this_frame(&w, EventKind::Skipped);
    assert_eq!(skipped.len(), 2, "{skipped:?}");
    assert!(skipped[0].contains("reason 'occupied' does not hold (the reason is 'terrain')"), "{skipped:?}");
    // Off the right edge at (5, 2).
    w.tick(&ground_tap([4, 2]), &mut NoSystems);
    assert_eq!(msg(&w), PropValue::Str("off the edge".into()));
    // A flower is in the way.
    w.tick(&ground_tap([1, 1]), &mut NoSystems);
    assert_eq!(msg(&w), PropValue::Str("no room".into()));
    // A house that fits clears the hint, and nothing is refused.
    w.tick(&ground_tap([0, 0]), &mut NoSystems);
    assert_eq!(msg(&w), PropValue::Str(String::new()));
    assert!(this_frame(&w, EventKind::Blocked).is_empty());
    // A `when` that stops the spawn is not a refusal: a third house is skipped.
    w.tick(&ground_tap([2, 0]), &mut NoSystems);
    w.tick(&ground_tap([0, 2]), &mut NoSystems);
    assert_eq!(prop(&w, "job", "houses"), PropValue::Int(2));
    let skipped = this_frame(&w, EventKind::Skipped);
    assert!(skipped.iter().any(|m| m.contains("spawn 'house' at (0, 2) skipped")), "{skipped:?}");
    assert!(!this_frame(&w, EventKind::Signal).iter().any(|m| m.contains("'refused'")));
}

#[test]
fn an_entity_that_starts_a_spawn_emits_refused() {
    let scene = r##"
[ground]
size = [3, 3]

[[entity]]
name = "planter"
pos = [1, 1]
sprite = {}
props = { tries = 0 }

[[connect]]
from = "planter"
signal = "tapped"
action = "spawn"
prefab = "tree"

[[connect]]
from = "planter"
signal = "refused"
to = "self"
action = "add"
prop = "tries"
"##;
    let mut vfs = Vfs::new();
    vfs.insert("scenes/main.toml", scene);
    vfs.insert("prefabs/tree.toml", "sprite = {}\n");
    let mut loader = SceneLoader::new(&vfs);
    let def = loader.load_scene("scenes/main.toml").unwrap_or_else(|| panic!("{:#?}", loader.diags));
    let mut w = World::new(&def, 1);
    let tap = Tap { x: 0.0, y: 0.0, hit: Some(0), under_disabled: vec![], ground: None };
    w.tick(&InputFrame { taps: vec![tap], ..Default::default() }, &mut NoSystems);
    // The planter stands on its own cell, so the tree has no room.
    assert_eq!(prop(&w, "planter", "tries"), PropValue::Int(1));
    assert!(
        last(&w, EventKind::Blocked).ends_with("; 'planter' (#0) emits 'refused'"),
        "{}",
        last(&w, EventKind::Blocked)
    );
}

#[test]
fn count_props_follow_spawns_and_enabling() {
    let mut w = feedback();
    // Scene copies are counted from frame 0, without events.
    assert_eq!((prop(&w, "job", "houses"), prop(&w, "job", "flowers")), (PropValue::Int(0), PropValue::Int(2)));
    w.tick(&InputFrame::default(), &mut NoSystems);
    assert!(w.events.all().iter().all(|e| e.kind != EventKind::Counted));
    // A spawn counts in the same step and the owner emits `changed`.
    w.tick(&ground_tap([0, 0]), &mut NoSystems);
    assert_eq!(prop(&w, "job", "houses"), PropValue::Int(1));
    assert_eq!(
        last(&w, EventKind::Counted),
        "job.houses: 0 -> 1 (enabled 'house' copies; #4 (house) was spawned); it emits 'changed'"
    );
    assert_eq!(prop(&w, "job", "ready"), PropValue::Bool(false));
    w.tick(&ground_tap([2, 0]), &mut NoSystems);
    assert_eq!(prop(&w, "job", "ready"), PropValue::Bool(true), "the condition on the count held");
    // Disabling and enabling a copy count down and up again.
    let flower = 2;
    let tap_flower = || InputFrame {
        taps: vec![Tap { x: 0.0, y: 0.0, hit: Some(flower), under_disabled: vec![], ground: None }],
        ..Default::default()
    };
    w.tick(&tap_flower(), &mut NoSystems);
    assert_eq!(prop(&w, "job", "flowers"), PropValue::Int(1));
    assert!(last(&w, EventKind::Counted).contains("#2 (flower) was disabled"), "{}", last(&w, EventKind::Counted));
    w.get_mut(flower).unwrap().enabled = true; // as Rust game code might
    w.tick(&InputFrame::default(), &mut NoSystems);
    assert_eq!(prop(&w, "job", "flowers"), PropValue::Int(2), "changes made outside connections are counted too");
}

#[test]
fn refusals_and_counts_are_deterministic() {
    let run = || {
        let mut w = feedback();
        for c in [[3, 0], [0, 0], [1, 1], [2, 0], [4, 2]] {
            w.tick(&ground_tap(c), &mut NoSystems);
        }
        (w.state_hash(), w.events.all().len())
    };
    assert_eq!(run(), run());
}

// ---------------------------------------------------------------------------
// Several actions per [[connect]] (ADR 0015)

const GROUPED: &str = r##"
[ground]
map = ["gggw", "gggw", "gggg"]

[[terrain]]
char = "g"
name = "grass"

[[terrain]]
char = "w"
name = "water"
build = false

[[entity]]
name = "town"
props = { coins = 3, note = "" }

[[entity]]
name = "lamp"
pos = [3, 2]
sprite = {}

[[connect]]
from = "ground"
signal = "tapped"
when = [{ entity = "town", prop = "coins", at_least = 3 }]
actions = [
  { action = "spawn", prefab = "house" },
  { action = "add", to = "town", prop = "coins", value = -3 },
  { action = "set", to = "town", prop = "note", value = "bought" },
]

[[connect]]
from = "lamp"
signal = "tapped"
actions = [{ action = "toggle", to = "self" }, { action = "set", to = "town", prop = "note", value = "lamp" }]
"##;

fn grouped() -> World {
    let mut vfs = Vfs::new();
    vfs.insert("scenes/main.toml", GROUPED);
    vfs.insert("prefabs/house.toml", "footprint = [2, 2]\nblock = {}\nsolid = true\n");
    let mut loader = SceneLoader::new(&vfs);
    let def = loader.load_scene("scenes/main.toml").unwrap_or_else(|| panic!("{:#?}", loader.diags));
    World::new(&def, 1)
}

#[test]
fn a_refused_spawn_stops_the_rest_of_its_actions() {
    let mut w = grouped();
    // (2, 0) reaches the water at (3, 0): nothing placed, nothing paid.
    w.tick(&ground_tap([2, 0]), &mut NoSystems);
    assert_eq!(prop(&w, "town", "coins"), PropValue::Int(3));
    assert_eq!(prop(&w, "town", "note"), PropValue::Str(String::new()));
    let skipped = this_frame(&w, EventKind::Skipped);
    assert_eq!(skipped.len(), 2, "{skipped:?}");
    assert!(
        skipped[0].contains("add skipped")
            && skipped[0].ends_with("the 'house' spawn before it in this [[connect]] was refused"),
        "{skipped:?}"
    );
    assert!(skipped[1].contains("; action 3 of 3)"), "{skipped:?}");
    let e = w.events.all().iter().rev().find(|e| e.kind == EventKind::Skipped).unwrap();
    assert_eq!(e.data.get("stopped_by").and_then(|v| v.as_str()), Some("refused spawn"));
    assert_eq!(e.data.get("item").and_then(|v| v.as_i64()), Some(3));
    // On grass: placed, then paid and noted, in order.
    w.tick(&ground_tap([0, 0]), &mut NoSystems);
    assert_eq!(prop(&w, "town", "coins"), PropValue::Int(0));
    assert_eq!(prop(&w, "town", "note"), PropValue::Str("bought".into()));
    let kinds: Vec<&str> = w.events.all().iter().filter(|e| e.frame == 1).map(|e| e.kind.name()).collect();
    assert_eq!(kinds, ["tap", "signal", "spawned", "action", "action"]);
}

#[test]
fn an_action_list_checks_its_when_once() {
    let mut w = grouped();
    // Paying the 3 coins makes `coins >= 3` false, but the note is still set:
    // the condition held when the signal arrived.
    w.tick(&ground_tap([0, 0]), &mut NoSystems);
    assert_eq!(prop(&w, "town", "note"), PropValue::Str("bought".into()));
    // Now it fails: every action of the list is skipped, each with its own event.
    w.tick(&ground_tap([0, 2]), &mut NoSystems);
    let skipped = this_frame(&w, EventKind::Skipped);
    assert_eq!(skipped.len(), 3, "{skipped:?}");
    assert!(skipped.iter().all(|m| m.contains("'town'.coins >= 3 does not hold (coins is 0)")), "{skipped:?}");
    // Lists work for any source and target, `self` included.
    let lamp = w.find("lamp").unwrap();
    let tap = Tap { x: 0.0, y: 0.0, hit: Some(lamp), under_disabled: vec![], ground: None };
    w.tick(&InputFrame { taps: vec![tap], ..Default::default() }, &mut NoSystems);
    assert!(!w.get(lamp).unwrap().enabled);
    assert_eq!(prop(&w, "town", "note"), PropValue::Str("lamp".into()));
}

// ---------------------------------------------------------------------------
// Spawn costs (ADR 0016)

fn shop(coins: &str) -> World {
    let scene = format!(
        r##"
[ground]
map = ["gggw", "gggw", "gggg"]

[[terrain]]
char = "g"
name = "grass"

[[terrain]]
char = "w"
name = "water"
build = false

[[entity]]
name = "town"
props = {{ coins = {coins}, why = "", paid = 0 }}

[[connect]]
from = "ground"
signal = "tapped"
action = "spawn"
prefab = "hut"
cost = {{ entity = "town", prop = "coins", amount = 3 }}

[[connect]]
from = "ground"
signal = "refused"
to = "town"
action = "set"
prop = "why"
value = "funds"
when = [{{ reason = "funds" }}]

[[connect]]
from = "ground"
signal = "refused"
to = "town"
action = "set"
prop = "why"
value = "terrain"
when = [{{ reason = "terrain" }}]

[[connect]]
from = "town"
signal = "changed"
to = "town"
action = "set"
prop = "paid"
value = 1
"##
    );
    let mut vfs = Vfs::new();
    vfs.insert("scenes/main.toml", scene.as_str());
    vfs.insert("prefabs/hut.toml", "block = {}\nsolid = true\n");
    let mut loader = SceneLoader::new(&vfs);
    let def = loader.load_scene("scenes/main.toml").unwrap_or_else(|| panic!("{:#?}", loader.diags));
    World::new(&def, 1)
}

#[test]
fn a_spawn_pays_its_cost_only_when_placed() {
    let mut w = shop("7");
    // Water: refused for terrain, nothing paid.
    w.tick(&ground_tap([3, 0]), &mut NoSystems);
    assert_eq!(
        (prop(&w, "town", "coins"), prop(&w, "town", "why")),
        (PropValue::Int(7), PropValue::Str("terrain".into()))
    );
    // Grass: placed and paid in the same step; the payer emits `changed`.
    w.tick(&ground_tap([0, 0]), &mut NoSystems);
    assert_eq!(prop(&w, "town", "coins"), PropValue::Int(4));
    assert_eq!(prop(&w, "town", "paid"), PropValue::Int(1));
    let spawned = last(&w, EventKind::Spawned);
    assert!(spawned.contains("; paid 3 from 'town'.coins (7 -> 4)"), "{spawned}");
    let e = w.events.all().iter().rev().find(|e| e.kind == EventKind::Spawned).unwrap();
    assert_eq!(e.data.pointer("paid/after").map(|v| v.to_compact()), Some("4".into()));
}

#[test]
fn too_little_to_pay_is_a_funds_refusal_checked_before_the_cell() {
    let mut w = shop("2");
    // Even over water the reason is funds: nothing could be built anywhere.
    w.tick(&ground_tap([3, 0]), &mut NoSystems);
    assert_eq!(prop(&w, "town", "why"), PropValue::Str("funds".into()));
    let blocked = last(&w, EventKind::Blocked);
    assert!(blocked.contains("it costs 3 and 'town'.coins is 2"), "{blocked}");
    let e = w.events.all().iter().rev().find(|e| e.kind == EventKind::Blocked).unwrap();
    assert_eq!(e.data.get("reason").and_then(|v| v.as_str()), Some("funds"));
    assert_eq!(e.other, w.find("town"), "the payer is the other party");
    assert_eq!((w.entities.len(), prop(&w, "town", "coins")), (1, PropValue::Int(2)));
    // A decimal purse pays decimals.
    let mut w = shop("3.5");
    w.tick(&ground_tap([0, 0]), &mut NoSystems);
    assert_eq!(prop(&w, "town", "coins"), PropValue::Float(0.5));
}

// ---------------------------------------------------------------------------
// Shape lists (ADR 0029)

const SHAPED: &str = r##"
[ground]
size = [8, 6]

[[entity]]
name = "town"
props = { coins = 10 }

[[entity]]
name = "first"
prefab = "house"
pos = [0, 0]

[[entity]]
name = "tree"
pos = [3, 0]
shape = [{ kind = "cylinder", at = [0.375, 0.375, 0], size = [0.25, 0.25, 0.5] }, { kind = "cone", at = [0.125, 0.125, 0.5], size = [0.75, 0.75, 1] }]

[[entity]]
prefab = "house"
cells = [[4, 0], [6, 0]]

[[entity]]
name = "lot"
prefab = "house"
pos = [0, 4]
shape = []

[[connect]]
from = "ground"
signal = "tapped"
action = "spawn"
prefab = "house"
"##;

const SHAPED_HOUSE: &str = "footprint = [2, 2]\n\n[[shape]]\nkind = \"box\"\nsize = [2, 2, 1.5]\ncolor = \"#b5651d\"\n\n[[shape]]\nkind = \"roof\"\nat = [0, 0, 1.5]\nsize = [2, 2, 0.75]\ncolor = \"#8b2f2f\"\nturn = 90\n";

fn shaped(house: &str, scene: &str) -> World {
    let mut vfs = Vfs::new();
    vfs.insert("scenes/main.toml", scene);
    vfs.insert("prefabs/house.toml", house);
    let mut loader = SceneLoader::new(&vfs);
    let def = loader.load_scene("scenes/main.toml").unwrap_or_else(|| panic!("{:#?}", loader.diags));
    World::new(&def, 1)
}

#[test]
fn the_world_holds_each_shape_list_once_and_spawned_copies_share_it() {
    let mut w = shaped(SHAPED_HOUSE, SHAPED);
    // The prefab's list, then the tree's own, in the order first used.
    assert_eq!(w.shapes.len(), 2);
    let lists: Vec<Option<u32>> = w.entities.iter().map(|e| e.shapes).collect();
    assert_eq!(lists, [None, Some(0), Some(1), Some(0), Some(0), None]);
    assert_eq!((w.shapes[0].len(), w.shapes[0][1].kind.name(), w.shapes[1][1].kind.name()), (2, "roof", "cone"));
    // A copy spawned later is drawn from the same list, and the table does not grow.
    w.tick(&ground_tap([2, 2]), &mut NoSystems);
    assert_eq!((w.entities.len(), w.entities[6].shapes, w.shapes.len()), (7, Some(0), 2));
    assert!(last(&w, EventKind::Spawned).contains("was spawned at (2, 2)"));
    // What `inspect` prints names the shapes, with turns in degrees.
    let j = crate::dump::entity_json(&w, &w.entities[6]);
    assert_eq!(j.pointer("shape/1/kind").and_then(|k| k.as_str()), Some("roof"));
    assert_eq!(j.pointer("shape/1/turn").and_then(|k| k.as_i64()), Some(90));
    assert_eq!(j.pointer("shape/0/size").map(|k| k.to_compact()), Some("[2,2,1.5]".into()));
    assert_eq!(crate::dump::entity_json(&w, &w.entities[5]).get("shape"), None);
}

#[test]
fn an_entity_of_shapes_takes_up_its_cells() {
    // Shapes are drawn there, so nothing can be placed on them: the tree's
    // cell and every cell of a house refuse a spawn, the empty lot does not.
    let mut w = shaped(SHAPED_HOUSE, SHAPED);
    assert!(w.entities[2].takes_space() && w.entities[1].takes_space() && !w.entities[5].takes_space());
    for (cell, by) in [([3, 0], 2), ([2, 0], 2), ([1, 1], 1), ([5, 1], 3)] {
        let refusal = w.can_place("house", cell).expect_err("taken");
        assert_eq!((refusal.reason, refusal.by), ("occupied", Some(by)), "{cell:?}");
    }
    assert!(w.can_place("house", [0, 4]).is_ok(), "an entity with `shape = []` is only data");
    w.check_against_scans(true);
    w.tick(&ground_tap([0, 4]), &mut NoSystems);
    w.tick(&ground_tap([1, 4]), &mut NoSystems);
    assert_eq!(w.entities.len(), 7, "the second tap is on the first one's house");
    assert!(w.verify_index().is_ok());
}

#[test]
fn shapes_are_part_of_the_state_and_a_world_without_them_hashes_as_before() {
    let hash = |house: &str| shaped(house, SHAPED).state_hash();
    let base = hash(SHAPED_HOUSE);
    assert_eq!(base, hash(SHAPED_HOUSE));
    // Every number, the kind, the color and the turn of a shape count.
    for (from, to) in [
        ("size = [2, 2, 1.5]", "size = [2, 2, 1.25]"),
        ("at = [0, 0, 1.5]", "at = [0, 0.5, 1.5]"),
        ("kind = \"roof\"", "kind = \"wedge\""),
        ("color = \"#8b2f2f\"", "color = \"#8b2f30\""),
        ("turn = 90", "turn = 270"),
    ] {
        assert!(SHAPED_HOUSE.contains(from));
        assert_ne!(base, hash(&SHAPED_HOUSE.replace(from, to)), "{from} -> {to}");
    }
    // Which list an entity has counts too: the lot with the prefab's list is another world.
    assert_ne!(base, shaped(SHAPED_HOUSE, &SHAPED.replace("shape = []\n", "")).state_hash());
    // A world with no shape list is hashed as it was before there were any: the pinned
    // towns of the oracle and of the examples say so; here, that the table adds nothing.
    let plain = "footprint = [2, 2]\nblock = {}\n";
    let without = SHAPED.replace("shape = [{ kind = \"cylinder\", at = [0.375, 0.375, 0], size = [0.25, 0.25, 0.5] }, { kind = \"cone\", at = [0.125, 0.125, 0.5], size = [0.75, 0.75, 1] }]", "sprite = {}");
    let w = shaped(plain, &without);
    assert!(w.shapes.is_empty() && w.entities.iter().all(|e| e.shapes.is_none()));
}
