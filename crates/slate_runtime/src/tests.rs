use std::path::PathBuf;

use slate_data::{Action, InputDef, InputKind, Vfs};

use crate::playtest::run_playtest;
use crate::session::{DataOnly, InspectOptions, LiveInput, Script, Session, SessionConfig};

fn garden() -> Vfs {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/iso_garden");
    Vfs::from_dir(&root).expect("examples/iso_garden")
}

#[test]
fn script_press_and_hold() {
    let s = Script::new(vec![
        InputDef { frame: 2, kind: InputKind::Press(Action::Up) },
        InputDef { frame: 5, kind: InputKind::Hold(Action::Left, 3) },
        InputDef { frame: 6, kind: InputKind::Tap(1.0, 2.0) },
    ]);
    let (p, d, t) = s.at(2);
    assert!(p.contains(Action::Up) && d.contains(Action::Up) && t.is_empty());
    let (p, d, _) = s.at(5);
    assert!(p.contains(Action::Left) && d.contains(Action::Left));
    let (p, d, t) = s.at(6);
    assert!(!p.contains(Action::Left) && d.contains(Action::Left) && t == vec![(1.0, 2.0)]);
    assert!(s.at(8).1.is_empty());
}

#[test]
fn example_playtests_pass() {
    let vfs = garden();
    for path in ["tests/walkthrough.toml", "tests/closed_gate.toml"] {
        let run = run_playtest(&vfs, path, Box::new(DataOnly)).unwrap_or_else(|d| panic!("{path}: {d:#?}"));
        assert!(run.failures.is_empty(), "{path}: {:#?}", run.failures);
        assert!(run.expects_checked >= 3, "{path}");
    }
}

#[test]
fn sessions_are_deterministic_and_replayable() {
    let vfs = garden();
    let cfg = SessionConfig { script: Some("tests/walkthrough.toml".into()), ..Default::default() };
    let mut a = Session::load(&vfs, &cfg, Box::new(DataOnly)).unwrap();
    let mut b = Session::load(&vfs, &cfg, Box::new(DataOnly)).unwrap();
    a.run_to(300);
    // Stepping in uneven chunks must not matter.
    for f in [1, 7, 150, 151, 299, 300] {
        b.run_to(f);
    }
    assert_eq!(a.hash_hex(), b.hash_hex());
    assert_eq!(a.render().0.image_hash(), b.render().0.image_hash());

    let other_seed = SessionConfig { seed: Some(99), ..cfg };
    let mut c = Session::load(&vfs, &other_seed, Box::new(DataOnly)).unwrap();
    c.run_to(300);
    assert_ne!(a.hash_hex(), c.hash_hex(), "the hen wanders differently");
}

#[test]
fn failing_expectations_explain_themselves() {
    let mut vfs = garden();
    vfs.insert(
        "tests/wrong.toml",
        "[test]\nframes = 20\n[[input]]\nframe = 1\npress = \"right\"\n[[expect]]\nentity = \"player\"\npos = [5, 5]\n",
    );
    let run = run_playtest(&vfs, "tests/wrong.toml", Box::new(DataOnly)).unwrap();
    let d = &run.failures[0];
    assert_eq!(d.code, "E0401");
    assert_eq!(d.message, "expected `player` pos = [5, 5, 0] (found [2, 1, 0]) at frame 20");
    assert_eq!(d.line, Some(7), "points at the entity being checked");
    assert!(d.notes[0].contains("moved right from (1, 1) to (2, 1)"), "{:?}", d.notes);
    assert!(d.hint.as_deref().unwrap().contains("slate inspect --input tests/wrong.toml --frame 20"));
}

#[test]
fn live_taps_resolve_against_the_drawn_frame() {
    let mut vfs = garden();
    vfs.insert("scenes/garden.toml", vfs.get("scenes/garden.toml").unwrap().replace("enabled = false", ""));
    let mut s = Session::load(&vfs, &SessionConfig::default(), Box::new(DataOnly)).unwrap();
    let player = s.world.find("player").unwrap();
    let [x, y] = s.view.project(1.0, 1.0, 0.0);
    s.step(&LiveInput { taps: vec![(x, y - 10.0)], ..Default::default() });
    let tap = &s.world.events.all()[0];
    assert_eq!(tap.entity, Some(player), "{}", tap.message);
    let j = s.inspect(&InspectOptions { entity: Some("player".into()), events: true, ..Default::default() });
    assert_eq!(j.pointer("entities/0/name").and_then(|v| v.as_str()), Some("player"));
    assert_eq!(j.pointer("events/0/kind").and_then(|v| v.as_str()), Some("tap"));
}
