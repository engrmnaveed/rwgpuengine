use slate_data::{Action, SceneLoader, Vfs};

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
