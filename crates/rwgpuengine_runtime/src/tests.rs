use std::path::PathBuf;

use rwgpuengine_data::{Action, CameraMove, CameraView, InputDef, InputKind, Vfs};

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
    let [x, y] = s.screen_point([1.0, 1.0, 0.0]).unwrap();
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
    // 11 playtests, 1,721 frames; a frame in ten changes the picture.
    assert_eq!(frames, 1721);
    assert!(repaints >= 9 && repaints * 8 < frames, "{repaints} repaints in {frames} frames");
}

/// A 6 x 6 floor where a tap on the ground builds a post on that cell.
fn builder(playtest: &str) -> Vfs {
    let mut vfs = Vfs::new();
    vfs.insert(
        "rwgpu.toml",
        "[project]\nname = \"t\"\nmain_scene = \"scenes/main.toml\"\n[display]\nwidth = 320\nheight = 240\n",
    );
    vfs.insert(
        "scenes/main.toml",
        "[ground]\nsize = [6, 6]\n\n[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\nactions = [{ action = \"spawn\", prefab = \"post\" }]\n",
    );
    vfs.insert("prefabs/post.toml", "block = { color = \"#c08040\", height = 1 }\nsolid = true\n");
    vfs.insert("tests/t.toml", playtest);
    vfs
}

fn posts(s: &Session) -> Vec<[i32; 2]> {
    s.world.entities.iter().filter(|e| e.prefab.as_deref() == Some("post")).map(|e| e.cell()).collect()
}

#[test]
fn the_camera_is_replayed_and_decides_what_a_tap_hits() {
    // The same pixel is tapped three times: before a quarter turn, after it,
    // and after switching to perspective. Each time it shows another cell.
    let script = "[test]\nframes = 30\n\n[[input]]\nframe = 2\ntap = [224, 136]\n\n[[input]]\nframe = 10\ncamera = { turn = 90 }\n\n[[input]]\nframe = 10\ntap = [224, 136]\n\n[[input]]\nframe = 20\ncamera = { view = \"perspective\", zoom = 0.5, pan = [1, 0] }\n\n[[input]]\nframe = 21\ntap = [224, 136]\n";
    let vfs = builder(script);
    let check = rwgpuengine_data::check_project(&vfs);
    assert!(check.diagnostics.is_empty(), "{:#?}", check.diagnostics);
    let cfg = SessionConfig { script: Some("tests/t.toml".into()), ..Default::default() };
    let run = || {
        let mut s = Session::load(&vfs, &cfg, Box::new(DataOnly)).unwrap();
        let mut seen = Vec::new();
        for frame in [2, 3, 10, 11, 22, 30] {
            s.run_to(frame);
            seen.push((s.camera, posts(&s), s.state_hash(), s.render().0.image_hash()));
        }
        (seen, s)
    };
    let (seen, s) = run();
    // The pixel is 64 right of and 16 below the middle of the frame, where
    // (2.5, 2.5) is drawn: two cells across and half a cell toward the
    // viewer, which is cell (4, 2). The camera move of frame 10 happens
    // before that frame's tap; after a quarter turn the same offset is
    // (2.5 - 0.5, 2.5 - 1.5): cell (2, 1).
    assert_eq!(seen[0].1, Vec::<[i32; 2]>::new());
    assert_eq!(seen[1].1, vec![[4, 2]]);
    assert_eq!(seen[2].0.turn, 0.0, "frame 10 has not been stepped yet");
    assert_eq!((seen[3].0.turn, seen[3].1.clone()), (90.0, vec![[4, 2], [2, 1]]));
    let last = &seen[5];
    assert_eq!(
        (last.0.view, last.0.turn, last.0.zoom, last.0.center),
        (CameraView::Perspective, 90.0, 0.5, [3.5, 2.5])
    );
    assert_eq!(last.1.len(), 3, "the third tap found a third cell: {:?}", last.1);
    // Replaying gives the same cameras, worlds and pictures.
    assert_eq!(seen, run().0);
    // The camera is reported next to the world's state.
    let j = s.inspect(&InspectOptions::default());
    assert_eq!(j.pointer("camera/view").and_then(|v| v.as_str()), Some("perspective"));
    assert_eq!(j.pointer("camera/turn").and_then(|v| v.as_f64()), Some(90.0));
    assert_eq!(s.summary().pointer("camera/zoom").and_then(|v| v.as_f64()), Some(0.5));
}

#[test]
fn moving_the_camera_changes_the_picture_and_not_the_world() {
    let vfs = builder("[test]\nframes = 5\n");
    let mut s = Session::load(&vfs, &SessionConfig::default(), Box::new(DataOnly)).unwrap();
    s.run_to(3);
    let (state, picture) = (s.state_hash(), s.render().0.image_hash());
    let mut presenter = Presenter::new();
    assert!(presenter.present(&s) && !presenter.present(&s));
    // A player's gesture.
    s.move_camera(&CameraMove { view: None, turn: 30.0, zoom: 1.5, pan: [0.0, 0.0] });
    assert_eq!(s.state_hash(), state, "the camera is not part of the world");
    assert_ne!(s.render().0.image_hash(), picture);
    assert!(presenter.present(&s), "a moved camera is a new picture");
    assert!(presenter.pixels() == s.render().0.rgba);
    // A tap is resolved against the moved picture: where the middle of cell
    // (1, 4) is drawn now, that cell is hit.
    let [x, y] = s.screen_point([1.0, 4.0, 0.0]).unwrap();
    s.step(&LiveInput { taps: vec![(x, y)], ..Default::default() });
    assert_eq!(posts(&s), vec![[1, 4]]);
    // Moving back gives the first camera again, bit for bit.
    let before = s.camera;
    s.move_camera(&CameraMove { view: None, turn: 90.0, zoom: 1.0, pan: [2.0, -1.0] });
    s.move_camera(&CameraMove { view: None, turn: -90.0, zoom: 1.0, pan: [-2.0, 1.0] });
    assert_eq!(s.camera, before);
}

#[test]
fn a_scene_can_start_turned_zoomed_and_in_perspective() {
    let mut vfs = builder("[test]\nframes = 5\n");
    let scene = vfs.get("scenes/main.toml").unwrap().to_string();
    vfs.insert(
        "scenes/main.toml",
        format!("[camera]\nview = \"perspective\"\nturn = -90\nzoom = 2\ncenter = [1, 2]\n\n{scene}"),
    );
    assert!(rwgpuengine_data::check_project(&vfs).diagnostics.is_empty());
    let s = Session::load(&vfs, &SessionConfig::default(), Box::new(DataOnly)).unwrap();
    assert_eq!(
        (s.camera.view, s.camera.turn, s.camera.zoom, s.camera.center),
        (CameraView::Perspective, 270.0, 2.0, [1.0, 2.0])
    );
    // The center is drawn at the middle of the frame.
    assert_eq!(s.screen_point([1.0, 2.0, 0.0]).map(|p| p.map(|v| v.round())), Some([160.0, 120.0]));
    // Bad values are refused by `check`, with the key's location.
    vfs.insert("scenes/main.toml", format!("[camera]\nview = \"fisheye\"\nzoom = 100\n\n{scene}"));
    let d = rwgpuengine_data::check_project(&vfs).diagnostics;
    assert_eq!(d.len(), 2, "{d:#?}");
    assert!(d.iter().all(|d| d.file.as_deref() == Some("scenes/main.toml") && d.line.is_some()), "{d:#?}");
    // An input is one thing: a camera move and a tap need two tables.
    vfs.insert("scenes/main.toml", scene);
    vfs.insert("tests/t.toml", "[test]\nframes = 5\n\n[[input]]\nframe = 1\ncamera = { turn = 90 }\ntap = [1, 1]\n");
    let d = rwgpuengine_data::check_project(&vfs).diagnostics;
    assert!(d.len() == 1 && d[0].message.contains("exactly one of `press`, `hold`, `tap` or `camera`"), "{d:#?}");
}

/// However a pan arrives, the camera's center stays over the ground (ADR
/// 0030): a gesture's move and a playtest's `[[input]] camera` stop at the
/// edge of its cells, and a tap after it hits what the clamped picture shows.
#[test]
fn a_pan_past_the_edge_stops_at_the_edge_of_the_ground() {
    let mut s = Session::load(&builder("[test]\nframes = 5\n"), &SessionConfig::default(), Box::new(DataOnly)).unwrap();
    let b = s.camera_bound;
    assert_eq!((b.min, b.max, s.camera.center), ([-0.5, -0.5], [5.5, 5.5], [2.5, 2.5]));
    s.move_camera(&CameraMove { view: None, turn: 0.0, zoom: 1.0, pan: [100.0, -100.0] });
    assert_eq!(s.camera.center, [5.5, -0.5]);
    // Along one axis it stops; along the other it still goes.
    s.move_camera(&CameraMove { view: None, turn: 90.0, zoom: 1.0, pan: [3.0, 2.0] });
    assert_eq!(s.camera.center, [5.5, 1.5]);
    s.move_camera(&CameraMove { view: None, turn: 0.0, zoom: 1.0, pan: [-7.25, 0.0] });
    assert_eq!(s.camera.center, [-0.5, 1.5]);
    let json = s.camera_json().to_compact();
    assert!(json.contains("\"center\":[-0.5,1.5]") && json.contains("\"bound\":[-0.5,-0.5,5.5,5.5]"), "{json}");

    // A playtest replays the same: the pan of 40 cells ends at the edge, so the
    // tap at the pixel where cell (5, 2) is drawn there builds on that cell.
    let mut edge =
        Session::load(&builder("[test]\nframes = 5\n"), &SessionConfig::default(), Box::new(DataOnly)).unwrap();
    edge.move_camera(&CameraMove { view: None, turn: 0.0, zoom: 1.0, pan: [3.0, 0.0] });
    let [x, y] = edge.screen_point([5.0, 2.0, 0.0]).unwrap();
    let script = format!(
        "[test]\nframes = 3\n\n[[input]]\nframe = 1\ncamera = {{ pan = [40, 0] }}\n\n[[input]]\nframe = 1\ntap = [{x}, {y}]\n"
    );
    let cfg = SessionConfig { script: Some("tests/t.toml".into()), ..SessionConfig::default() };
    let mut s = Session::load(&builder(&script), &cfg, Box::new(DataOnly)).unwrap();
    s.run_to(3);
    assert_eq!((s.camera.center, posts(&s)), ([5.5, 2.5], vec![[5, 2]]));
}

/// A scene with no ground bounds the center by the cells of what is in the
/// world when it starts; a scene's own camera may start outside the ground,
/// and the bound then reaches to it, so the way back is never cut off.
#[test]
fn without_a_ground_the_bound_is_what_the_world_holds_and_it_holds_the_start() {
    let scene = "[[entity]]\npos = [2, 3]\nfootprint = [2, 2]\nblock = { height = 1 }\n\n[[entity]]\npos = [-4, 1]\nsprite = {}\n\n\
                 [[entity]]\nname = \"hud\"\nscreen_pos = [0, 0]\nsprite = {}\n\n[[entity]]\nname = \"data\"\npos = [50, 50]\nprops = { n = 1 }\n";
    let vfs = project(scene, &[]);
    let s = Session::load(&vfs, &SessionConfig::default(), Box::new(DataOnly)).unwrap();
    // The block covers x 1.5..3.5, y 2.5..4.5; the sprite x -4.5..-3.5, y 0.5..1.5; the start is (0, 0).
    // The HUD and the entity with only props take up no cell.
    assert_eq!((s.camera_bound.min, s.camera_bound.max), ([-4.5, 0.0], [3.5, 4.5]));

    let mut vfs = builder("[test]\nframes = 5\n");
    let ground = vfs.get("scenes/main.toml").unwrap().to_string();
    vfs.insert("scenes/main.toml", format!("[camera]\ncenter = [20, 1]\n\n{ground}"));
    let mut s = Session::load(&vfs, &SessionConfig::default(), Box::new(DataOnly)).unwrap();
    assert_eq!((s.camera.center, s.camera_bound.min, s.camera_bound.max), ([20.0, 1.0], [-0.5, -0.5], [20.0, 5.5]));
    s.move_camera(&CameraMove { view: None, turn: 0.0, zoom: 1.0, pan: [-30.0, 0.0] });
    assert_eq!(s.camera.center, [-0.5, 1.0]);
    s.move_camera(&CameraMove { view: None, turn: 0.0, zoom: 1.0, pan: [20.5, 0.0] });
    assert_eq!(s.camera.center, [20.0, 1.0]);
}

/// A project in memory: rwgpu.toml, one scene, prefabs and playtests.
fn project(scene: &str, files: &[(&str, &str)]) -> Vfs {
    let mut vfs = Vfs::new();
    vfs.insert(
        "rwgpu.toml",
        "[project]\nname = \"t\"\nengine = \"0.1\"\nmain_scene = \"scenes/main.toml\"\n\n[sim]\ntick_rate = 60\nseed = 1\n",
    );
    vfs.insert("scenes/main.toml", scene);
    for (path, text) in files {
        vfs.insert(*path, *text);
    }
    vfs
}

/// `n` wanderers on every other cell, each named `w<i>`.
fn town(n: usize) -> String {
    let mut side = 2;
    while (side / 2) * (side / 2) < n {
        side += 2;
    }
    let mut s = format!("[ground]\nsize = [{side}, {side}]\n");
    for i in 0..n {
        let (x, y) = (2 * (i % (side / 2)), 2 * (i / (side / 2)));
        s.push_str(&format!("\n[[entity]]\nname = \"w{i}\"\npos = [{x}, {y}]\nwander = {{ step_frames = 1 }}\n"));
    }
    s
}

#[test]
fn a_playtest_counts_every_event_its_expectations_ask_about_at_any_scale() {
    // The last of 2,000 wanderers: past the step's budget, its moves are not
    // written by default. The playtest watches what it names.
    let scene = town(2_000);
    let vfs = project(&scene, &[]);
    let mut all = Session::load(&vfs, &SessionConfig::default(), Box::new(DataOnly)).unwrap();
    all.watch(&["all"]).unwrap();
    all.run_to(30);
    let id = all.world.find("w1999").unwrap();
    let moves = all.world.events.about(id).filter(|e| e.kind == rwgpuengine_sim::EventKind::Moved).count();
    let mut plain = Session::load(&vfs, &SessionConfig::default(), Box::new(DataOnly)).unwrap();
    plain.run_to(30);
    assert_eq!(plain.world.events.about(id).count(), 0, "not written by default");
    assert!(moves > 5);
    let test = format!(
        "[test]\nframes = 30\n\n[[expect]]\nevent = \"moved\"\nentity = \"w1999\"\nat_least = {moves}\nat_most = {moves}\n\n[[expect]]\nevent = \"blocked\"\nat_least = 1\n"
    );
    let vfs = project(&scene, &[("tests/t.toml", &test)]);
    let run = run_playtest(&vfs, "tests/t.toml", Box::new(DataOnly)).unwrap();
    assert!(run.failures.is_empty(), "{:#?}", run.failures);
    assert_eq!(run.expects_checked, 2);
}

#[test]
fn watch_takes_names_ids_kinds_and_all() {
    let vfs = project(&town(4), &[]);
    let mut s = Session::load(&vfs, &SessionConfig::default(), Box::new(DataOnly)).unwrap();
    s.watch(&["w1", "#3", "blocked"]).unwrap();
    assert_eq!(s.watched(), ["w1", "w3", "blocked"]);
    s.watch(&["all"]).unwrap();
    assert!(s.world.events.watch().all);
    let e = s.watch(&["w9"]).unwrap_err();
    assert_eq!(e.code, "E0202");
    assert_eq!(e.hint.as_deref(), Some("did you mean `w0`?"));
    assert_eq!(s.watch(&["#40"]).unwrap_err().code, "E0202");
}

#[test]
fn a_game_that_reaches_a_limit_fails_its_playtest_with_the_limit_code() {
    // A plate entered in step 0 starts a loop that runs 100 adds a signal.
    let mut scene = String::from(
        "[ground]\nsize = [4, 4]\n\n[[entity]]\nname = \"walker\"\npos = [0, 0]\npatrol = { path = [[0, 0], [1, 0]], step_frames = 1 }\n\n[[entity]]\nname = \"plate\"\npos = [1, 0]\ntile = {}\ntrigger = {}\n\n[[entity]]\nname = \"hud\"\nprops = { n = 0, p = 0 }\n\n[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"hud\"\naction = \"add\"\nprop = \"n\"\n",
    );
    let acts = vec!["{ action = \"add\", to = \"hud\", prop = \"p\" }"; 100].join(", ");
    scene.push_str(&format!("\n[[connect]]\nfrom = \"hud\"\nsignal = \"changed\"\nactions = [{acts}]\n"));
    let vfs = project(&scene, &[("tests/t.toml", "[test]\nframes = 4\n")]);
    let run = run_playtest(&vfs, "tests/t.toml", Box::new(DataOnly)).unwrap();
    let codes: Vec<&str> = run.failures.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["E0302"]);
    let d = &run.failures[0];
    assert!(d.message.starts_with("step 0: more than 10000 actions in one frame"), "{}", d.message);
    assert!(d.hint.as_deref().unwrap().contains("rwgpu inspect --input tests/t.toml --frame 1 --since 0 --events"));
    assert!(d.notes.iter().any(|n| n.contains("2 steps reached it, the last one step 2")), "{:?}", d.notes);
}

#[test]
fn more_taps_in_a_frame_than_the_limit_are_not_picked_and_say_so() {
    let mut test = String::from("[test]\nframes = 2\n");
    for i in 0..300 {
        test.push_str(&format!("\n[[input]]\nframe = 1\ntap = [{}, 100]\n", i));
    }
    let vfs = project(&town(4), &[("tests/t.toml", &test)]);
    let run = run_playtest(&vfs, "tests/t.toml", Box::new(DataOnly)).unwrap();
    assert_eq!(run.failures.iter().map(|d| d.code).collect::<Vec<_>>(), ["E0305"]);
}

#[test]
fn a_playtest_checks_each_frame_its_own_tables_in_file_order() {
    // Written out of frame order: two fail at frame 2, one at frame 5.
    let test = "[test]\nframes = 6\n\n[[expect]]\nframe = 5\nentity = \"w0\"\nenabled = false\n\n[[expect]]\nframe = 2\nentity = \"w1\"\nenabled = false\n\n[[expect]]\nframe = 2\nentity = \"w3\"\nenabled = false\n\n[[expect]]\nentity = \"w0\"\nenabled = true\n";
    let vfs = project(&town(4), &[("tests/t.toml", test)]);
    let run = run_playtest(&vfs, "tests/t.toml", Box::new(DataOnly)).unwrap();
    let failed: Vec<&str> = run.failures.iter().map(|d| d.message.split('`').nth(1).unwrap_or("")).collect();
    assert_eq!(failed, ["w1", "w3", "w0"]);
    assert_eq!(run.expects_checked, 4);
}
