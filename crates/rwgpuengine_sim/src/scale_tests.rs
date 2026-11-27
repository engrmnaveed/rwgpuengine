//! Events at scale and the limits on work in a step (ADR 0027): the budget of
//! the per-actor kinds, watching, and one hostile small scene per limit that
//! reaches it.

use rwgpuengine_core::limits::{
    ACTIONS_PER_STEP, COUNT_CHANGES_PER_STEP, ENTITIES, EVENTS_PER_KIND_PER_STEP, SIGNALS_PER_STEP, TAPS_PER_STEP,
};
use rwgpuengine_data::{SceneLoader, Vfs};

use crate::events::{EventKind, Watch};
use crate::input::{InputFrame, Tap};
use crate::world::{NoSystems, World};

fn world(scene: &str, prefabs: &[(&str, &str)]) -> World {
    let mut vfs = Vfs::new();
    vfs.insert("scenes/main.toml", scene);
    for (name, text) in prefabs {
        vfs.insert(format!("prefabs/{name}.toml"), *text);
    }
    let mut loader = SceneLoader::new(&vfs);
    let def = loader.load_scene("scenes/main.toml").unwrap_or_else(|| panic!("{:#?}", loader.diags));
    World::new(&def, 7)
}

fn idle(w: &mut World, n: usize) {
    for _ in 0..n {
        w.tick(&InputFrame::default(), &mut NoSystems);
    }
}

/// `n` wanderers on every other cell of a square ground, the first ones named.
fn town(n: usize, named: usize) -> String {
    let mut side = 2;
    while (side / 2) * (side / 2) < n {
        side += 2;
    }
    let mut s = format!("[ground]\nsize = [{side}, {side}]\n");
    for i in 0..n {
        let (x, y) = (2 * (i % (side / 2)), 2 * (i / (side / 2)));
        let name = if i < named { format!("name = \"w{i}\"\n") } else { String::new() };
        s.push_str(&format!("\n[[entity]]\n{name}pos = [{x}, {y}]\nwander = {{ step_frames = 1 }}\n"));
    }
    s
}

fn written(w: &World, frame: u64, kind: EventKind) -> usize {
    w.events.all().iter().filter(|e| e.frame == frame && e.kind == kind).count()
}

#[test]
fn past_the_budget_moves_are_counted_and_one_line_says_how_many() {
    let mut w = world(&town(3_000, 0), &[]);
    idle(&mut w, 3);
    let budget = EVENTS_PER_KIND_PER_STEP.get();
    for f in 0..3 {
        assert_eq!(written(&w, f, EventKind::Moved), budget, "frame {f}");
        // At the end of the step, one per kind past the budget, in kind order.
        let of: Vec<&str> = w
            .events
            .all()
            .iter()
            .filter(|e| e.frame == f)
            .skip_while(|e| e.kind != EventKind::Omitted)
            .map(|e| e.data.get("of").and_then(|v| v.as_str()).unwrap_or("not an omitted event"))
            .collect();
        // In the first step the actors stand apart: few are blocked.
        let want: &[&str] = if f == 0 { &["moved"] } else { &["moved", "blocked"] };
        assert_eq!(of, want, "frame {f}");
        let line = w.events.all().iter().find(|e| e.frame == f && e.kind == EventKind::Omitted).unwrap();
        assert!(
            line.message.contains("more 'moved' events in this step were counted, not written"),
            "{}",
            line.message
        );
        assert!(line.message.contains("--watch NAME"));
        let count = line.data.get("count").and_then(|v| v.as_i64()).unwrap() as usize;
        assert!(count + budget <= 3_000, "one try per actor at most");
    }
    // Nothing is lost from the counts: written and omitted add up to what happened.
    let counts = w.events.counts();
    let omitted = w.events.omitted();
    let happened = |k| counts.iter().find(|(x, _)| *x == k).map_or(0, |c| c.1);
    let left_out = |k| omitted.iter().find(|(x, _)| *x == k).map_or(0, |c| c.1);
    for k in [EventKind::Moved, EventKind::Blocked] {
        let kept = w.events.all().iter().filter(|e| e.kind == k).count() as u64;
        assert_eq!(kept + left_out(k), happened(k), "{k:?}");
        assert!(left_out(k) > 0);
    }
    assert_eq!(happened(EventKind::Moved) + happened(EventKind::Blocked), 3 * 3_000 - roll_zero(&w));
}

/// Wanderers that rolled "stay" in the three steps of the test above
/// (neither a move nor a refusal): what the full log says.
fn roll_zero(_: &World) -> u64 {
    let mut all = world(&town(3_000, 0), &[]);
    all.events.set_watch(Watch::everything());
    idle(&mut all, 3);
    let tries = all.events.all().iter().filter(|e| matches!(e.kind, EventKind::Moved | EventKind::Blocked)).count();
    3 * 3_000 - tries as u64
}

#[test]
fn what_is_written_never_changes_the_world() {
    let scene = town(1_500, 3);
    let mut quiet = world(&scene, &[]);
    let mut all = world(&scene, &[]);
    all.events.set_watch(Watch::everything());
    let mut some = world(&scene, &[]);
    let mut watch = Watch::default();
    watch.entity(some.find("w1").unwrap());
    watch.kind(EventKind::Trigger);
    some.events.set_watch(watch);
    for _ in 0..20 {
        for w in [&mut quiet, &mut all, &mut some] {
            w.tick(&InputFrame::default(), &mut NoSystems);
        }
        assert_eq!(quiet.state_hash(), all.state_hash());
        assert_eq!(quiet.state_hash(), some.state_hash());
    }
    assert!(all.events.omitted().is_empty());
    assert!(!quiet.events.omitted().is_empty());
    let happened =
        |w: &World| w.events.counts().into_iter().filter(|(k, _)| *k != EventKind::Omitted).collect::<Vec<_>>();
    assert_eq!(happened(&quiet), happened(&all), "the same things happened");
}

#[test]
fn a_watched_entity_has_every_event_written_and_takes_no_place_in_the_budget() {
    let scene = town(2_000, 2_000);
    let mut plain = world(&scene, &[]);
    let mut watched = world(&scene, &[]);
    // The last actor: far past the budget in every step.
    let id = watched.find("w1999").unwrap();
    let mut watch = Watch::default();
    watch.entity(id);
    watched.events.set_watch(watch);
    let mut all = world(&scene, &[]);
    all.events.set_watch(Watch::everything());
    idle(&mut plain, 10);
    idle(&mut watched, 10);
    idle(&mut all, 10);
    let about = |w: &World| w.events.about(id).map(|e| (e.frame, e.message.clone())).collect::<Vec<_>>();
    assert!(about(&plain).is_empty(), "past the budget, nothing about it is written by default");
    assert_eq!(about(&watched), about(&all), "watched: every one of its events, word for word");
    assert!(!about(&watched).is_empty());
    // Everything else is what the unwatched run wrote.
    let others = |w: &World| {
        w.events.all().iter().filter(|e| e.entity != Some(id) && e.other != Some(id)).cloned().collect::<Vec<_>>()
    };
    let mut plain_others = others(&plain);
    let mut watched_others = others(&watched);
    // The omitted lines count one event fewer where the watched one was written.
    plain_others.retain(|e| e.kind != EventKind::Omitted);
    watched_others.retain(|e| e.kind != EventKind::Omitted);
    assert_eq!(plain_others, watched_others);
}

#[test]
fn a_small_world_writes_every_event() {
    let mut w = world(&town(200, 0), &[]);
    idle(&mut w, 50);
    assert!(w.events.omitted().is_empty());
    assert!(!w.events.all().iter().any(|e| e.kind == EventKind::Omitted));
}

#[test]
fn game_code_can_build_an_event_only_when_it_is_written() {
    let mut w = world(&town(10, 0), &[]);
    let mut built = 0;
    for i in 0..(EVENTS_PER_KIND_PER_STEP.get() + 10) {
        w.log_with(EventKind::Custom, None, None, |_| {
            built += 1;
            (format!("note {i}"), rwgpuengine_core::json::Json::object())
        });
    }
    assert_eq!(built, EVENTS_PER_KIND_PER_STEP.get());
    w.tick(&InputFrame::default(), &mut NoSystems);
    let last = w.events.all().last().unwrap();
    assert_eq!(last.kind, EventKind::Omitted);
    assert_eq!(last.data.get("count").and_then(|v| v.as_i64()), Some(10));
}

/// A patrol steps onto a plate in step 0: the kick for the scenes below.
const KICK: &str = r##"
[ground]
size = [4, 4]

[[entity]]
name = "walker"
pos = [0, 0]
patrol = { path = [[0, 0], [1, 0]], step_frames = 1 }

[[entity]]
name = "plate"
pos = [1, 0]
tile = {}
trigger = {}
"##;

fn errors(w: &World) -> Vec<String> {
    w.events
        .all()
        .iter()
        .filter(|e| e.kind == EventKind::Error)
        .filter_map(|e| e.data.get("code").and_then(|c| c.as_str()).map(str::to_string))
        .collect()
}

#[test]
fn a_loop_of_signals_reaches_the_signal_limit() {
    let scene = format!(
        "{KICK}\n[[entity]]\nname = \"n\"\nprops = {{ v = 0 }}\n\n[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"n\"\naction = \"add\"\nprop = \"v\"\n\n[[connect]]\nfrom = \"n\"\nsignal = \"changed\"\nto = \"n\"\naction = \"add\"\nprop = \"v\"\n"
    );
    let mut w = world(&scene, &[]);
    idle(&mut w, 1);
    assert_eq!(errors(&w), ["E0301"]);
    let hit = &w.limits_hit()[0];
    assert_eq!((hit.limit.name, hit.first, hit.steps), (SIGNALS_PER_STEP.name, 0, 1));
}

#[test]
fn connections_that_feed_each_other_reach_the_action_limit() {
    // Each `changed` of the hud reaches 10 connections of 10 adds, and each add changes it again.
    let mut scene = format!("{KICK}\n[[entity]]\nname = \"hud\"\nprops = {{ n = 0");
    for m in 0..10 {
        scene.push_str(&format!(", p{m} = 0"));
    }
    scene.push_str(
        " }\n\n[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"hud\"\naction = \"add\"\nprop = \"n\"\n",
    );
    for _ in 0..10 {
        let acts: Vec<String> =
            (0..10).map(|m| format!("{{ action = \"add\", to = \"hud\", prop = \"p{m}\" }}")).collect();
        scene.push_str(&format!(
            "\n[[connect]]\nfrom = \"hud\"\nsignal = \"changed\"\nactions = [{}]\n",
            acts.join(", ")
        ));
    }
    let mut w = world(&scene, &[]);
    idle(&mut w, 1);
    assert_eq!(errors(&w), ["E0302"]);
    let actions = w.events.all().iter().filter(|e| e.kind == EventKind::Action).count();
    assert_eq!(actions, ACTIONS_PER_STEP.get(), "every action up to the limit ran, and no more");
    let e = w.events.all().iter().find(|e| e.kind == EventKind::Error).unwrap();
    assert!(e.message.contains("more than 10000 actions in one frame (stopped at 'hud' (#2) add p"), "{}", e.message);
    // The next step starts again from nothing.
    idle(&mut w, 2);
    assert_eq!(w.limits_hit()[0].steps, 2, "the walker presses the plate again in step 2");
}

/// Eggs that spawn `per` more eggs where they stand (props only: they take
/// no space), started by the nest's `changed`.
fn eggs(per: usize, egg: &str) -> World {
    let acts: Vec<&str> = (0..per).map(|_| "{ action = \"spawn\", prefab = \"egg\" }").collect();
    let scene = format!(
        "{KICK}\n[[entity]]\nname = \"nest\"\npos = [3, 3]\nprops = {{ n = 0 }}\n\n[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"nest\"\naction = \"add\"\nprop = \"n\"\n\n[[connect]]\nfrom = \"nest\"\nsignal = \"changed\"\naction = \"spawn\"\nprefab = \"egg\"\n\n[[connect]]\nfrom_prefab = \"egg\"\nsignal = \"spawned\"\nactions = [{}]\n",
        acts.join(", ")
    );
    world(&scene, &[("egg", egg)])
}

#[test]
fn copies_that_count_their_own_kind_reach_the_count_limit() {
    let mut w = eggs(1, "counts = { eggs = \"egg\" }\n");
    idle(&mut w, 1);
    assert_eq!(errors(&w)[0], "E0303");
    let counted = w.events.all().iter().filter(|e| e.kind == EventKind::Counted).count();
    assert_eq!(counted, COUNT_CHANGES_PER_STEP.get());
    // The counts left behind are brought up to date by the next step.
    let behind = |w: &World| {
        let eggs = w.entities.iter().filter(|e| e.prefab.as_deref() == Some("egg")).count() as f64;
        let stale = |e: &&crate::world::Entity| e.props.get("eggs").and_then(|p| p.as_num()) != Some(eggs);
        w.entities.iter().filter(|e| e.prefab.is_some()).filter(stale).count()
    };
    assert!(behind(&w) > 0);
    idle(&mut w, 1);
    assert_eq!(behind(&w), 0);
    assert_eq!(w.limits_hit().len(), 1);
}

#[test]
fn a_chain_of_spawns_reaches_the_entity_limit() {
    // The limit is 1000000; a debug build cannot step that many in a test,
    // so this world's is 5000. `rwgpu run` on this scene reaches 1000000 in
    // step 200 (ADR 0027).
    let mut w = eggs(10, "props = { age = 0 }\n");
    w.limit_entities(5_000);
    let mut frames = 0;
    while w.limits_hit().iter().all(|h| h.limit != &ENTITIES) {
        idle(&mut w, 1);
        frames += 1;
        assert!(frames < 400, "the world never stopped growing: {} entities", w.entities.len());
    }
    assert_eq!(w.entities.len(), 5_000);
    let e = w.events.all().iter().rev().find(|e| e.kind == EventKind::Error).unwrap();
    assert_eq!(e.data.get("code").and_then(|c| c.as_str()), Some("E0304"));
    assert!(e.message.contains("the world holds 5000 entities, the most it may"), "{}", e.message);
    idle(&mut w, 4);
    assert_eq!(w.entities.len(), 5_000, "and it stays there");
    assert_eq!(ENTITIES.value, 1_000_000);
}

#[test]
fn too_many_taps_in_one_step_reach_the_tap_limit() {
    let scene = "[[entity]]\nname = \"n\"\nprops = { v = 0 }\n";
    let mut w = world(scene, &[]);
    let n = w.find("n").unwrap();
    let taps: Vec<Tap> = (0..300).map(|i| Tap { x: i as f32, hit: Some(n), ..Tap::default() }).collect();
    w.tick(&InputFrame { taps, ..InputFrame::default() }, &mut NoSystems);
    assert_eq!(w.events.all().iter().filter(|e| e.kind == EventKind::Tap).count(), TAPS_PER_STEP.get());
    assert_eq!(errors(&w), ["E0305"]);
    let e = w.events.all().iter().find(|e| e.kind == EventKind::Error).unwrap();
    assert!(
        e.message.starts_with("300 taps in one frame; the first 256 were applied and the other 44"),
        "{}",
        e.message
    );
}

#[test]
fn a_scoped_look_at_counts_finds_what_a_look_at_all_of_them_finds() {
    // Jobs that count huts and trees; huts that count trees; spawns of both.
    // With the check on, every scoped look is compared with a look at all.
    let scene = r##"
[ground]
size = [8, 8]

[[entity]]
name = "job"
counts = { huts = "hut", trees = "tree" }

[[entity]]
name = "job2"
counts = { trees = "tree", more = "tree" }

[[entity]]
prefab = "tree"
pos = [7, 7]

[[connect]]
from = "ground"
signal = "tapped"
actions = [{ action = "spawn", prefab = "hut" }, { action = "spawn", prefab = "tree" }]

[[connect]]
from_prefab = "tree"
signal = "tapped"
to = "self"
action = "toggle"
"##;
    let prefabs = [("hut", "block = {}\nsolid = true\ncounts = { trees = \"tree\" }\n"), ("tree", "sprite = {}\n")];
    let mut w = world(scene, &prefabs);
    assert!(w.check_against_scans(true) || !cfg!(debug_assertions));
    for i in 0..12 {
        let cell = [i % 6, i / 6 * 2];
        let ground = Tap { ground: Some(cell), ..Tap::default() };
        let tree = Tap {
            hit: w.entities.iter().rev().find(|e| e.prefab.as_deref() == Some("tree")).map(|e| e.id),
            ..Tap::default()
        };
        w.tick(&InputFrame { taps: vec![ground, tree], ..InputFrame::default() }, &mut NoSystems);
    }
    let huts = w.entities.iter().filter(|e| e.prefab.as_deref() == Some("hut") && e.enabled).count() as f64;
    assert_eq!(w.by_name("job").unwrap().props.get("huts").and_then(|p| p.as_num()), Some(huts));
    assert!(w.events.all().iter().filter(|e| e.kind == EventKind::Counted).count() > 20);
}
