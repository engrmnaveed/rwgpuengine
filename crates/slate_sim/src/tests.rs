use slate_data::{Action, PropValue, SceneLoader, Vfs};

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
    assert_eq!(names, slate_data::schema::EVENT_KINDS);
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
            taps: vec![Tap { x: 1.0, y: 2.0, hit: Some(bell), under_disabled: vec![] }],
            ..Default::default()
        },
    );
    assert_eq!(kinds(&w), vec!["tap", "signal"]);
    step(
        &mut w,
        InputFrame { taps: vec![Tap { x: 3.0, y: 4.0, hit: None, under_disabled: vec![gate] }], ..Default::default() },
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
    assert_eq!(add.data.get("after"), Some(&slate_core::json::Json::Int(1)));
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
    let tap = Tap { x: 0.0, y: 0.0, hit: Some(n), under_disabled: vec![] };
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
    let tap = Tap { x: 0.0, y: 0.0, hit: Some(n), under_disabled: vec![] };
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
    let tap = Tap { x: 0.0, y: 0.0, hit: Some(bell), under_disabled: vec![] };
    step(&mut w, InputFrame { taps: vec![tap], ..InputFrame::default() });
    assert_eq!(w.get(bell).unwrap().props.get("rung"), Some(&PropValue::Int(1)));
    assert!(w.by_name("door").unwrap().enabled, "occupied at frame 0: no `pressed` (documented)");
}
