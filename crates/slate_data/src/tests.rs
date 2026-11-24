//! Loader tests over small in-memory projects.

use crate::load::{Vfs, check_project};

fn project(files: &[(&str, &str)]) -> Vfs {
    let mut vfs = Vfs::new();
    vfs.insert("slate.toml", "[project]\nname = \"t\"\n");
    for (p, t) in files {
        vfs.insert(*p, *t);
    }
    vfs
}

fn codes(vfs: &Vfs) -> Vec<&'static str> {
    check_project(vfs).diagnostics.iter().map(|d| d.code).collect()
}

const WALL: &str = "block = { color = \"#888\" }\nsolid = true\n";

#[test]
fn a_valid_project_has_no_diagnostics() {
    let vfs = project(&[
        ("prefabs/wall.toml", WALL),
        (
            "scenes/main.toml",
            r##"
[ground]
size = [4, 4]

[[entity]]
name = "player"
pos = [0, 0]
sprite = { color = "#fc0" }
mover = {}

[[entity]]
prefab = "wall"
cells = [[2, 0], [2, 1]]

[[entity]]
name = "plate"
pos = [1, 1]
tile = {}
trigger = {}

[[entity]]
name = "gate"
prefab = "wall"
pos = [2, 2]
block = { color = "#a33" }

[[connect]]
from = "plate"
signal = "pressed"
to = "gate"
action = "disable"
"##,
        ),
        (
            "tests/smoke.toml",
            r#"
[test]
frames = 60

[[input]]
frame = 1
press = "right"

[[expect]]
entity = "player"
pos = [1, 0]
"#,
        ),
    ]);
    let report = check_project(&vfs);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    assert_eq!(report.scenes, vec!["scenes/main.toml"]);
    assert_eq!(report.prefabs, vec!["wall"]);

    let mut loader = crate::SceneLoader::new(&vfs);
    let scene = loader.load_scene("scenes/main.toml").unwrap();
    assert_eq!(scene.entities.len(), 5);
    let gate = scene.entities.iter().find(|e| e.name.as_deref() == Some("gate")).unwrap();
    assert!(gate.solid, "prefab key kept");
    assert_eq!(gate.block.unwrap().color.to_hex(), "#aa3333", "override merged into the prefab's block");
    assert_eq!(gate.block.unwrap().height, 1.0, "default applied");
}

#[test]
fn missing_prefab_suggests_a_name() {
    let vfs =
        project(&[("prefabs/wall.toml", WALL), ("scenes/main.toml", "[[entity]]\nprefab = \"wal\"\npos = [0, 0]\n")]);
    let r = check_project(&vfs);
    assert_eq!(r.diagnostics[0].code, "E0201");
    assert_eq!(r.diagnostics[0].hint.as_deref(), Some("did you mean `wall`?"));
    assert_eq!(r.diagnostics[0].line, Some(2));
}

#[test]
fn reference_errors() {
    let vfs = project(&[(
        "scenes/main.toml",
        r#"
[[entity]]
name = "a"
pos = [0, 0]

[[entity]]
name = "a"
pos = [1, 0]

[[entity]]
name = "b"
cells = [[0, 1]]

[[connect]]
from = "a"
signal = "pressed"
to = "gaet"
action = "toggle"
"#,
    )]);
    assert_eq!(codes(&vfs), vec!["E0203", "E0205", "W0203", "E0202"]);
}

#[test]
fn static_placement_warnings() {
    let vfs = project(&[
        ("prefabs/wall.toml", WALL),
        (
            "scenes/main.toml",
            "[ground]\nsize = [2, 2]\n[[entity]]\nprefab = \"wall\"\ncells = [[0, 0], [0, 0], [5, 5]]\n",
        ),
    ]);
    assert_eq!(codes(&vfs), vec!["W0201", "W0202"]);
}

#[test]
fn playtest_checks() {
    let vfs = project(&[
        ("scenes/main.toml", "[[entity]]\nname = \"p\"\n"),
        (
            "tests/t.toml",
            r#"
[test]
frames = 10

[[input]]
frame = 20
press = "up"

[[input]]
frame = 1
press = "up"
tap = [1, 1]

[[expect]]
entity = "q"
enabled = true
"#,
        ),
    ]);
    assert_eq!(codes(&vfs), vec!["W0204", "E0105", "E0202"]);
}

#[test]
fn missing_main_scene_and_engine_mismatch() {
    let mut vfs = Vfs::new();
    vfs.insert("slate.toml", "[project]\nname = \"t\"\nengine = \"9.9\"\nmain_scene = \"scenes/mian.toml\"\n");
    vfs.insert("scenes/main.toml", "");
    let r = check_project(&vfs);
    let c: Vec<_> = r.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(c, vec!["W0205", "E0204"]);
    assert_eq!(r.diagnostics[1].hint.as_deref(), Some("did you mean `scenes/main.toml`?"));
}

#[test]
fn bundle_round_trip() {
    let vfs = project(&[("scenes/main.toml", "a = \"x\"\n")]);
    let back = Vfs::from_bundle(&vfs.to_bundle()).unwrap();
    assert_eq!(back, vfs);
    assert_eq!(back.content_hash(), vfs.content_hash());
}

#[test]
fn reason_needs_a_blocked_event() {
    let vfs = project(&[
        ("scenes/main.toml", "[[entity]]\nname = \"p\"\n"),
        ("tests/t.toml", "[[expect]]\nevent = \"moved\"\nreason = \"solid\"\n"),
    ]);
    assert_eq!(codes(&vfs), vec!["E0105"]);
}

#[test]
fn event_only_keys_and_bounds_are_checked() {
    let vfs = project(&[
        ("scenes/main.toml", "[[entity]]\nname = \"p\"\n"),
        (
            "tests/t.toml",
            "[[expect]]\nentity = \"p\"\nenabled = true\nother = \"p\"\n\n[[expect]]\nevent = \"moved\"\nat_least = 3\nat_most = 1\n\n[[expect]]\nevent = \"blocked\"\nother = \"ghost\"\n",
        ),
    ]);
    assert_eq!(codes(&vfs), vec!["E0105", "E0105", "E0202"]);
}

#[test]
fn allow_silences_intended_warnings_and_only_warnings() {
    let stacked = |allow: &str| {
        project(&[
            ("prefabs/wall.toml", WALL),
            (
                "scenes/main.toml",
                &format!(
                    "[[entity]]\nname = \"a\"\nprefab = \"wall\"\n\n[[entity]]\nname = \"b\"\nprefab = \"wall\"\n{allow}\n"
                ),
            ),
        ])
    };
    assert_eq!(codes(&stacked("")), vec!["W0201"]);
    assert!(codes(&stacked("allow = [\"W0201\"]")).is_empty());
    // An invalid entry is an error, and silences nothing.
    assert_eq!(codes(&stacked("allow = [\"E0101\"]")), vec!["W0201", "E0105"]);
    let r = check_project(&stacked("allow = [\"W0210\"]"));
    let bad = r.diagnostics.iter().find(|d| d.code == "E0105").expect("E0105");
    assert_eq!(bad.hint.as_deref(), Some("did you mean `W0201`?"));
}
