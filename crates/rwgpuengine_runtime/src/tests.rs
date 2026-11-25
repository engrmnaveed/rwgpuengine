use std::path::PathBuf;

use rwgpuengine_data::{Action, InputDef, InputKind, Vfs};

use crate::playtest::run_playtest;
use crate::present::Presenter;
use crate::session::{DataOnly, InspectOptions, LiveInput, Script, Session, SessionConfig};

fn example(name: &str) -> Vfs {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(name);
    Vfs::from_dir(&root).unwrap_or_else(|e| panic!("examples/{name}: {e}"))
}

fn garden() -> Vfs {
    example("iso_garden")
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
fn coin_vault_playtests_pass_and_check_clean() {
    let vfs = example("coin_vault");
    let report = rwgpuengine_data::check_project(&vfs);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    for path in &report.playtests {
        let run = run_playtest(&vfs, path, Box::new(DataOnly)).unwrap_or_else(|d| panic!("{path}: {d:#?}"));
        assert!(run.failures.is_empty(), "{path}: {:#?}", run.failures);
    }
    assert_eq!(report.playtests.len(), 4);
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
    assert!(d.hint.as_deref().unwrap().contains("rwgpu inspect --input tests/wrong.toml --frame 20"));
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

#[test]
fn event_expects_match_the_subject_and_filter_by_reason() {
    let mut vfs = garden();
    let base = vfs.get("tests/closed_gate.toml").unwrap().split("[[expect]]").next().unwrap().to_string();
    let with = |expect: &str| format!("{base}[[expect]]\n{expect}\n");
    // The gate is only ever the *other* party of the player's blocked event.
    vfs.insert("tests/a.toml", with("event = \"blocked\"\nentity = \"gate\""));
    // The player was blocked by a solid at least once...
    vfs.insert("tests/b.toml", with("event = \"blocked\"\nentity = \"player\"\nreason = \"solid\""));
    // ...but never by the ground's edge.
    vfs.insert("tests/c.toml", with("event = \"blocked\"\nentity = \"player\"\nreason = \"edge\""));
    let failures = |p: &str| run_playtest(&vfs, p, Box::new(DataOnly)).unwrap().failures.len();
    assert_eq!(failures("tests/a.toml"), 1);
    assert_eq!(failures("tests/b.toml"), 0);
    assert_eq!(failures("tests/c.toml"), 1);
}

#[test]
fn event_expects_filter_by_other_party_and_bound_counts() {
    let mut vfs = garden();
    let base = vfs.get("tests/closed_gate.toml").unwrap().split("[[expect]]").next().unwrap().to_string();
    let with = |expect: &str| format!("{base}[[expect]]\n{expect}\n");
    vfs.insert("tests/by_gate.toml", with("event = \"blocked\"\nentity = \"player\"\nother = \"gate\""));
    vfs.insert("tests/by_hen.toml", with("event = \"blocked\"\nentity = \"player\"\nother = \"hen\""));
    vfs.insert("tests/never.toml", with("event = \"signal\"\nentity = \"goal\"\nsignal = \"reached\"\nat_most = 0"));
    vfs.insert("tests/not_never.toml", with("event = \"blocked\"\nentity = \"player\"\nat_most = 0"));
    let run = |p: &str| run_playtest(&vfs, p, Box::new(DataOnly)).unwrap();
    assert!(run("tests/by_gate.toml").failures.is_empty());
    assert_eq!(run("tests/by_hen.toml").failures.len(), 1);
    assert!(run("tests/never.toml").failures.is_empty());
    let d = &run("tests/not_never.toml").failures[0];
    assert!(d.message.starts_with("expected no `blocked` event(s) whose subject is `player`"), "{}", d.message);
    assert!(d.notes[0].contains("blocked by solid 'gate'"), "{:?}", d.notes);
}

#[test]
fn inspect_gives_tap_points_that_really_hit() {
    let mut vfs = garden();
    vfs.insert("scenes/garden.toml", vfs.get("scenes/garden.toml").unwrap().replace("enabled = false", ""));
    let mut s = Session::load(&vfs, &SessionConfig::default(), Box::new(DataOnly)).unwrap();
    let j = s.inspect(&InspectOptions { entity: Some("guard".into()), ..Default::default() });
    let tap = j.pointer("entities/0/tap").and_then(|t| t.as_array()).expect("tap point").to_vec();
    let (x, y) = (tap[0].as_i64().unwrap() as f32, tap[1].as_i64().unwrap() as f32);
    s.step(&LiveInput { taps: vec![(x, y)], ..Default::default() });
    assert_eq!(s.world.events.all()[0].entity, s.world.find("guard"));
}

#[test]
fn a_presenter_shows_the_reference_pixels_and_repaints_only_on_change() {
    // Every frame of every example playtest: whenever the presenter repaints,
    // its pixels are the reference rasterizer's; whenever it does not, the
    // reference picture has not changed either.
    let (mut frames, mut repaints) = (0, 0);
    for name in ["iso_garden", "coin_vault", "tiny_town"] {
        let vfs = example(name);
        for path in rwgpuengine_data::check_project(&vfs).playtests {
            let cfg = SessionConfig { script: Some(path.clone()), ..Default::default() };
            let mut session = Session::load(&vfs, &cfg, Box::new(DataOnly)).unwrap();
            let mut presenter = Presenter::new();
            assert!(presenter.pixels().is_empty());
            let mut last = Vec::new();
            for frame in 0..=session.script_frames().unwrap() {
                session.run_to(frame);
                frames += 1;
                if presenter.present(&session) {
                    repaints += 1;
                    last = session.render().0.rgba;
                    assert!(presenter.pixels() == last, "{name}/{path} frame {frame}: repainted pixels differ");
                } else if frame % 64 == 0 {
                    // Sampled, because the reference rasterizer is slow.
                    assert!(session.render().0.rgba == last, "{name}/{path} frame {frame}: a change was skipped");
                }
                assert!(!presenter.present(&session), "{name}/{path} frame {frame}: presenting twice repaints once");
            }
            // One-off painting gives the same picture, and a reset forces a repaint.
            let mut once = vec![1, 2, 3];
            session.paint(&mut once);
            assert!(once == last, "{name}/{path}");
            presenter.reset();
            assert!(presenter.present(&session) && presenter.pixels() == once);
        }
    }
    // 9 playtests, 1,569 frames; a frame in ten changes the picture.
    assert_eq!(frames, 1569);
    assert!(repaints >= 9 && repaints * 8 < frames, "{repaints} repaints in {frames} frames");
}
