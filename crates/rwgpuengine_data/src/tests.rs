//! Loader tests over small in-memory projects.

use crate::load::{Vfs, check_project};

pub(crate) fn project(files: &[(&str, &str)]) -> Vfs {
    let mut vfs = Vfs::new();
    vfs.insert("rwgpu.toml", "[project]\nname = \"t\"\n");
    for (p, t) in files {
        vfs.insert(*p, *t);
    }
    vfs
}

fn codes(vfs: &Vfs) -> Vec<&'static str> {
    check_project(vfs).diagnostics.iter().map(|d| d.code).collect()
}

pub(crate) const WALL: &str = "block = { color = \"#888\" }\nsolid = true\n";

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
    vfs.insert("rwgpu.toml", "[project]\nname = \"t\"\nengine = \"9.9\"\nmain_scene = \"scenes/mian.toml\"\n");
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

#[test]
fn actors_starting_inside_solids_are_warned_unless_allowed() {
    let scene = |allow: &str| {
        project(&[
            ("prefabs/wall.toml", WALL),
            (
                "scenes/main.toml",
                &format!(
                    "[[entity]]\nprefab = \"wall\"\npos = [1, 1]\n\n[[entity]]\nname = \"bit\"\npos = [1, 1]\npatrol = {{ path = [[1, 1]] }}\n{allow}\n"
                ),
            ),
        ])
    };
    assert_eq!(codes(&scene("")), vec!["W0206"]);
    assert!(codes(&scene("allow = [\"W0206\"]")).is_empty());
}

// ---------------------------------------------------------------------------
// Data logic v1 (ADR 0011): props, conditions, labels, screen entities

pub(crate) const LOGIC: &str = r##"
[ground]
size = [6, 4]

[[entity]]
name = "player"
pos = [0, 0]
sprite = {}
mover = {}

[[entity]]
name = "plate_a"
pos = [1, 1]
tile = {}
trigger = {}

[[entity]]
name = "plate_b"
pos = [3, 1]
tile = {}
trigger = {}

[[entity]]
name = "door"
pos = [5, 1]
block = {}
solid = true

[[entity]]
name = "coin"
pos = [2, 3]
sprite = {}
trigger = { on_enter = "collected", only = "player" }

[[entity]]
name = "hud"
screen_pos = [8, 8]
label = { text = "Coins: {coins}/3 {mode}" }
props = { coins = 0, mode = "play" }

[[connect]]
from = "plate_a"
signal = "pressed"
to = "door"
action = "disable"
when = [{ entity = "plate_b", occupied = true }]

[[connect]]
from = "coin"
signal = "collected"
to = "hud"
action = "add"
prop = "coins"

[[connect]]
from = "hud"
signal = "changed"
to = "hud"
action = "set"
prop = "mode"
value = "rich"
when = [{ entity = "hud", prop = "coins", at_least = 3 }, { entity = "door", enabled = false }]
"##;

fn logic_with(extra: &str) -> Vec<&'static str> {
    let scene = format!("{LOGIC}\n{extra}");
    let scene: &'static str = Box::leak(scene.into_boxed_str());
    codes(&project(&[("scenes/main.toml", scene)]))
}

#[test]
fn valid_logic_has_no_diagnostics() {
    let vfs = project(&[("scenes/main.toml", LOGIC)]);
    let report = check_project(&vfs);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let scene = crate::SceneLoader::new(&vfs).load_scene("scenes/main.toml").unwrap();
    let add = &scene.connections[1];
    assert_eq!(add.actions[0].value, Some(crate::PropValue::Int(1)), "add defaults to 1");
    assert_eq!(scene.connections[2].when.len(), 2);
}

#[test]
fn props_must_be_declared_and_typed() {
    // Unknown prop on add, with a suggestion.
    let d = check_project(&project(&[(
        "scenes/main.toml",
        Box::leak(format!("{LOGIC}\n[[connect]]\nfrom = \"coin\"\nsignal = \"collected\"\nto = \"hud\"\naction = \"add\"\nprop = \"coin\"\n").into_boxed_str()),
    )]))
    .diagnostics;
    assert_eq!(d.iter().map(|d| d.code).collect::<Vec<_>>(), vec!["E0206"]);
    assert_eq!(d[0].hint.as_deref(), Some("did you mean `coins`?"));
    // add on a string prop; set with the wrong type; set without value; add without prop.
    let bad = [
        ("action = \"add\"\nprop = \"mode\"", "E0207"),
        ("action = \"set\"\nprop = \"coins\"\nvalue = \"many\"", "E0207"),
        ("action = \"add\"\nprop = \"coins\"\nvalue = 0.5", "E0207"),
        ("action = \"set\"\nprop = \"coins\"", "E0104"),
        ("action = \"add\"", "E0104"),
        ("action = \"disable\"\nprop = \"coins\"", "E0105"),
    ];
    for (body, code) in bad {
        let extra = format!("[[connect]]\nfrom = \"coin\"\nsignal = \"collected\"\nto = \"hud\"\n{body}\n");
        assert_eq!(logic_with(&extra), vec![code], "{body}");
    }
}

#[test]
fn when_conditions_are_checked() {
    let cases = [
        ("{ entity = \"plate_c\", occupied = true }", "E0202"),
        ("{ entity = \"plate_b\" }", "E0105"),
        ("{ entity = \"plate_b\", occupied = true, enabled = true }", "E0105"),
        ("{ entity = \"hud\", prop = \"coins\" }", "E0105"),
        ("{ entity = \"hud\", prop = \"coin\", at_least = 1 }", "E0206"),
        ("{ entity = \"hud\", prop = \"mode\", at_least = 1 }", "E0207"),
        ("{ entity = \"hud\", prop = \"coins\", equals = \"3\" }", "E0207"),
        ("{ entity = \"hud\", prop = \"coins\", at_least = 3, at_most = 2 }", "E0105"),
        ("{ entity = \"plate_b\", enabled = true, at_least = 2 }", "E0105"),
        ("{ entity = \"plate_b\", ocupied = true }", "E0103"),
    ];
    for (cond, code) in cases {
        let extra = format!(
            "[[connect]]\nfrom = \"plate_b\"\nsignal = \"pressed\"\nto = \"door\"\naction = \"disable\"\nwhen = [{cond}]\n"
        );
        let got = logic_with(&extra);
        assert!(got.contains(&code), "{cond}: expected {code}, got {got:?}");
    }
}

#[test]
fn changed_is_emitted_only_by_entities_whose_props_change() {
    let extra = "[[connect]]\nfrom = \"door\"\nsignal = \"changed\"\nto = \"door\"\naction = \"enable\"\n";
    let d = check_project(&project(&[("scenes/main.toml", Box::leak(format!("{LOGIC}\n{extra}").into_boxed_str()))]))
        .diagnostics;
    assert_eq!(d.iter().map(|d| d.code).collect::<Vec<_>>(), vec!["W0203"]);
    assert!(d[0].hint.as_deref().unwrap().contains("`add`s to or `set`s"), "{:?}", d[0].hint);
}

#[test]
fn labels_and_screen_entities_are_checked() {
    let cases = [
        ("label = { text = \"Score {score}\" }\nprops = { points = 0 }", "E0206"),
        ("label = { text = \"Score {points\" }\nprops = { points = 0 }", "E0105"),
        ("label = { text = \"a } b\" }", "E0105"),
        ("label = { text = \"Café\" }", "W0207"),
        ("screen_pos = [4, 4]\nblock = {}", "E0105"),
        ("screen_pos = [4, 4]\ntrigger = {}", "E0105"),
        ("label = { text = \"x\", scale = 9 }", "E0105"),
    ];
    for (body, code) in cases {
        let extra = format!("[[entity]]\nname = \"extra\"\npos = [4, 3]\n{body}\n");
        assert_eq!(logic_with(&extra), vec![code], "{body}");
    }
    // Braces can be escaped; W0207 can be allowed.
    assert!(logic_with("[[entity]]\nname = \"e\"\nlabel = { text = \"{{x}}\" }\n").is_empty());
    assert!(logic_with("[[entity]]\nname = \"e\"\nlabel = { text = \"Café\" }\nallow = [\"W0207\"]\n").is_empty());
}

#[test]
fn expectations_on_props_and_text_are_checked() {
    let test = |body: &str| {
        let t: &'static str = Box::leak(format!("[test]\nframes = 10\n[[expect]]\n{body}\n").into_boxed_str());
        codes(&project(&[("scenes/main.toml", LOGIC), ("tests/t.toml", t)]))
    };
    assert!(test("entity = \"hud\"\nprops = { coins = 0 }\ntext = \"Coins: 0/3 play\"").is_empty());
    assert_eq!(test("entity = \"hud\"\nprops = { coin = 0 }"), vec!["E0206"]);
    assert_eq!(test("entity = \"hud\"\nprops = { coins = \"0\" }"), vec!["E0207"]);
    assert_eq!(test("entity = \"door\"\ntext = \"x\""), vec!["E0105"]);
}

#[test]
fn label_templates_parse() {
    use crate::{LabelPart, parse_label};
    assert_eq!(
        parse_label("Coins: {coins}/3").unwrap(),
        vec![LabelPart::Text("Coins: ".into()), LabelPart::Prop("coins".into()), LabelPart::Text("/3".into())]
    );
    assert_eq!(parse_label("{{x}}").unwrap(), vec![LabelPart::Text("{x}".into())]);
    assert!(parse_label("{a b}").is_err() && parse_label("{").is_err() && parse_label("}").is_err());
    assert_eq!(crate::font::text_size("ab\nc"), [11, 18]);
    assert_eq!(crate::font::text_size(""), [0, 8]);
}

#[test]
fn pushables_are_checked() {
    let scene = |extra: &str| -> &'static str {
        Box::leak(format!("[ground]\nsize = [4, 4]\n\n[[entity]]\nname = \"c\"\npos = [1, 1]\nblock = {{}}\npushable = true\n{extra}\n").into_boxed_str())
    };
    assert!(codes(&project(&[("scenes/main.toml", scene(""))])).is_empty());
    assert_eq!(codes(&project(&[("scenes/main.toml", scene("mover = {}"))])), vec!["E0105"]);
    let on_wall = scene("\n[[entity]]\nname = \"w\"\npos = [1, 1]\nblock = {}\nsolid = true\n");
    assert_eq!(codes(&project(&[("scenes/main.toml", on_wall)])), vec!["W0201"]);
    assert_eq!(codes(&project(&[("scenes/main.toml", scene("screen_pos = [1, 1]"))])), vec!["E0105"]);
}

// ---------------------------------------------------------------------------
// Builder kit (ADR 0013): terrain maps, footprints, spawn, prefab sources

pub(crate) const HOUSE: &str = "footprint = [2, 2]\nblock = { color = \"#eee\", top = \"#c33\" }\nsolid = true\n";

/// The builder kit's scene with `extra` appended, and its house prefab.
pub(crate) fn builder_project(extra: &str) -> Vfs {
    let scene = format!(
        r##"
[ground]
map = ["ggw", "ggw", "sgg"]

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
props = {{ coins = 5 }}
{extra}
"##
    );
    project(&[("prefabs/house.toml", HOUSE), ("scenes/main.toml", &scene)])
}

fn builder_with(extra: &str) -> Vec<&'static str> {
    codes(&builder_project(extra))
}

#[test]
fn a_valid_builder_scene_has_no_diagnostics() {
    let extra = r##"
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
    assert_eq!(builder_with(extra), Vec::<&str>::new());
}

#[test]
fn terrain_maps_are_checked() {
    let scene = |ground: &str| {
        let text = format!("{ground}\n[[terrain]]\nchar = \"g\"\n\n[[terrain]]\nchar = \"w\"\nbuild = false\n");
        codes(&project(&[("scenes/main.toml", &text)]))
    };
    assert_eq!(scene("[ground]\nmap = [\"gg\", \"gw\"]"), Vec::<&str>::new());
    // An undeclared character, a ragged row, a size that disagrees, colors with a map.
    assert_eq!(scene("[ground]\nmap = [\"gx\", \"gw\"]"), vec!["E0208"]);
    assert_eq!(scene("[ground]\nmap = [\"gg\", \"g\"]"), vec!["E0105"]);
    assert_eq!(scene("[ground]\nmap = [\"gg\", \"gw\"]\nsize = [3, 2]"), vec!["E0105"]);
    assert_eq!(scene("[ground]\nmap = [\"gg\", \"gw\"]\ncolor = \"#fff\""), vec!["E0105"]);
    // Neither size nor map.
    assert_eq!(scene("[ground]\ncolor = \"#fff\""), vec!["E0104"]);
    // Terrain chars must be single and unique.
    let vfs = project(&[(
        "scenes/main.toml",
        "[ground]\nmap = [\"g\"]\n[[terrain]]\nchar = \"g\"\n[[terrain]]\nchar = \"g\"\n[[terrain]]\nchar = \"ab\"\n",
    )]);
    assert_eq!(codes(&vfs), vec!["E0105", "E0105"]);
}

#[test]
fn spawn_and_prefab_sources_are_checked() {
    // spawn without a prefab, with a target, with an unknown prefab.
    let spawn = |keys: &str| {
        builder_with(&format!("[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\n{keys}"))
    };
    assert_eq!(spawn(""), vec!["E0104"]);
    assert_eq!(spawn("prefab = \"house\"\nto = \"town\""), vec!["E0105"]);
    assert_eq!(spawn("prefab = \"hosue\""), vec!["E0201"]);
    // The ground only emits `tapped`, and is not an entity.
    assert_eq!(
        builder_with("[[connect]]\nfrom = \"ground\"\nsignal = \"pressed\"\naction = \"spawn\"\nprefab = \"house\""),
        vec!["W0203"]
    );
    assert_eq!(
        builder_with("[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"disable\""),
        vec!["E0105"]
    );
    // from and from_prefab: exactly one.
    assert_eq!(builder_with("[[connect]]\nsignal = \"tapped\"\nto = \"town\"\naction = \"disable\""), vec!["E0104"]);
    assert_eq!(
        builder_with(
            "[[connect]]\nfrom = \"town\"\nfrom_prefab = \"house\"\nsignal = \"tapped\"\nto = \"town\"\naction = \"disable\""
        ),
        vec!["E0105"]
    );
    // A prefab that is never spawned never emits `spawned`.
    assert_eq!(
        builder_with("[[connect]]\nfrom_prefab = \"house\"\nsignal = \"spawned\"\nto = \"town\"\naction = \"disable\""),
        vec!["W0203"]
    );
    // `to = "self"` with add needs the prop on the prefab.
    assert_eq!(
        builder_with(
            "[[connect]]\nfrom_prefab = \"house\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"add\"\nprop = \"level\""
        ),
        vec!["E0206"]
    );
    // Non-spawn actions need `to`; `prefab` is only for spawn.
    assert_eq!(builder_with("[[connect]]\nfrom = \"town\"\nsignal = \"tapped\"\naction = \"disable\""), vec!["E0104"]);
    assert_eq!(
        builder_with(
            "[[connect]]\nfrom = \"town\"\nsignal = \"tapped\"\nto = \"town\"\naction = \"disable\"\nprefab = \"house\""
        ),
        vec!["E0105"]
    );
}

#[test]
fn footprints_and_reserved_names_are_checked() {
    // A moving thing cannot be multi-cell; reserved names.
    assert_eq!(builder_with("[[entity]]\npos = [0, 0]\nfootprint = [2, 1]\nmover = {}\nsprite = {}"), vec!["E0105"]);
    assert_eq!(builder_with("[[entity]]\nname = \"ground\"\npos = [0, 0]\ntile = {}"), vec!["E0105"]);
    assert_eq!(builder_with("[[entity]]\nname = \"self\"\npos = [0, 0]\ntile = {}"), vec!["E0105"]);
    // Footprints count for overlaps and the ground's edge.
    assert_eq!(
        builder_with(
            "[[entity]]\nprefab = \"house\"\npos = [0, 0]\n\n[[entity]]\npos = [1, 1]\nblock = {}\nsolid = true"
        ),
        vec!["W0201"]
    );
    assert_eq!(builder_with("[[entity]]\nprefab = \"house\"\npos = [2, 2]"), vec!["W0202"]);
}

#[test]
fn prefab_expectations_are_checked() {
    let vfs = |expect: &str| {
        project(&[
            ("prefabs/house.toml", HOUSE),
            ("scenes/main.toml", "[ground]\nsize = [4, 4]\n"),
            ("tests/t.toml", &format!("[test]\nscene = \"scenes/main.toml\"\nframes = 10\n\n[[expect]]\n{expect}\n")),
        ])
    };
    assert_eq!(codes(&vfs("prefab = \"house\"\ncount = 0")), Vec::<&str>::new());
    assert_eq!(codes(&vfs("prefab = \"house\"\npos = [1, 1]")), Vec::<&str>::new());
    assert_eq!(codes(&vfs("prefab = \"hous\"\ncount = 1")), vec!["E0201"]);
    assert_eq!(codes(&vfs("count = 1")), vec!["E0105"]);
    assert_eq!(codes(&vfs("prefab = \"house\"\nenabled = true")), vec!["E0105"]);
}

#[test]
fn event_expectations_can_filter_by_prefab() {
    let vfs = |expect: &str| {
        project(&[
            ("prefabs/house.toml", HOUSE),
            ("scenes/main.toml", "[ground]\nsize = [4, 4]\n"),
            ("tests/t.toml", &format!("[test]\nscene = \"scenes/main.toml\"\nframes = 10\n\n[[expect]]\n{expect}\n")),
        ])
    };
    assert_eq!(codes(&vfs("event = \"skipped\"\nprefab = \"house\"\nat_least = 1")), Vec::<&str>::new());
    assert_eq!(codes(&vfs("event = \"blocked\"\nreason = \"occupied\"\nprefab = \"house\"")), Vec::<&str>::new());
    // count and pos belong to copy counts.
    assert_eq!(codes(&vfs("event = \"spawned\"\nprefab = \"house\"\ncount = 1")), vec!["E0105"]);
    assert_eq!(codes(&vfs("event = \"spawned\"\nprefab = \"house\"\npos = [1, 1]")), vec!["E0105"]);
}

// ---------------------------------------------------------------------------
// Builder kit v2 (ADR 0014): refusal signals and count props

pub(crate) const SPAWN_HOUSE: &str =
    "[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"house\"\n";

pub(crate) fn hint_on_refused(when: &str) -> String {
    format!(
        "{SPAWN_HOUSE}\n[[entity]]\nname = \"hint\"\nprops = {{ msg = \"\" }}\n\n[[connect]]\nfrom = \"ground\"\n\
         signal = \"refused\"\nto = \"hint\"\naction = \"set\"\nprop = \"msg\"\nvalue = \"no\"\n{when}"
    )
}

#[test]
fn refused_signals_and_reason_conditions_are_checked() {
    // The ground emits `refused` once something spawns from its taps.
    assert_eq!(builder_with(&hint_on_refused("")), Vec::<&str>::new());
    assert_eq!(builder_with(&hint_on_refused("when = [{ reason = \"terrain\" }]")), Vec::<&str>::new());
    // ...and not before.
    let no_spawn = hint_on_refused("").replace(SPAWN_HOUSE, "");
    assert_eq!(builder_with(&no_spawn), vec!["W0203"]);
    // An unknown reason, a reason with an entity, a reason on another signal.
    assert_eq!(builder_with(&hint_on_refused("when = [{ reason = \"water\" }]")), vec!["E0105"]);
    assert_eq!(
        builder_with(&hint_on_refused("when = [{ reason = \"edge\", entity = \"town\", enabled = true }]")),
        vec!["E0105"]
    );
    let on_tapped =
        hint_on_refused("when = [{ reason = \"edge\" }]").replace("signal = \"refused\"", "signal = \"tapped\"");
    assert_eq!(builder_with(&on_tapped), vec!["E0105"]);
    // Other conditions still need an entity.
    assert_eq!(builder_with(&hint_on_refused("when = [{ enabled = true }]")), vec!["E0104"]);
    // An entity that starts spawns emits `refused` too; one that does not, never.
    let from_town = |spawns: bool| {
        let spawn = if spawns {
            "[[connect]]\nfrom = \"town\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"house\"\n"
        } else {
            ""
        };
        builder_with(&format!(
            "{spawn}\n[[connect]]\nfrom = \"town\"\nsignal = \"refused\"\nto = \"town\"\naction = \"set\"\n\
             prop = \"coins\"\nvalue = 0"
        ))
    };
    assert_eq!(from_town(true), Vec::<&str>::new());
    assert_eq!(from_town(false), vec!["W0203"]);
}

#[test]
fn count_props_are_checked() {
    let job =
        |counts: &str, extra: &str| builder_with(&format!("[[entity]]\nname = \"job\"\ncounts = {counts}\n{extra}"));
    // A count is an integer prop: labels, conditions and `changed` read it.
    let reads = "label = { text = \"Houses {houses}\" }\nscreen_pos = [0, 0]\n\n[[connect]]\nfrom = \"job\"\n\
                 signal = \"changed\"\nto = \"town\"\naction = \"set\"\nprop = \"coins\"\nvalue = 9\n\
                 when = [{ entity = \"job\", prop = \"houses\", at_least = 2 }]";
    assert_eq!(job("{ houses = \"house\" }", reads), Vec::<&str>::new());
    // Unknown prefab, a value that is not a name, a clash with props.
    assert_eq!(job("{ houses = \"hous\" }", ""), vec!["E0201"]);
    assert_eq!(job("{ houses = 3 }", ""), vec!["E0102"]);
    assert_eq!(job("{ houses = \"house\" }", "props = { houses = 0 }"), vec!["E0105"]);
    // The engine owns it: add/set are refused.
    let add = "\n[[connect]]\nfrom = \"town\"\nsignal = \"tapped\"\nto = \"job\"\naction = \"add\"\nprop = \"houses\"";
    assert_eq!(job("{ houses = \"house\" }", add), vec!["E0105"]);
    // It is an integer: equals must be a number.
    let text = "\n[[connect]]\nfrom = \"job\"\nsignal = \"changed\"\nto = \"town\"\naction = \"disable\"\n\
                when = [{ entity = \"job\", prop = \"houses\", equals = \"two\" }]";
    assert_eq!(job("{ houses = \"house\" }", text), vec!["E0207"]);
    // Without counts (or a connection changing its props), `job` never emits `changed`.
    let changed = "[[entity]]\nname = \"job\"\nprops = { n = 0 }\n\n[[connect]]\nfrom = \"job\"\nsignal = \"changed\"\n\
                   to = \"town\"\naction = \"disable\"";
    assert_eq!(builder_with(changed), vec!["W0203"]);
}

// ---------------------------------------------------------------------------
// Several actions per [[connect]] (ADR 0015)

fn grouped(connect: &str) -> Vec<&'static str> {
    builder_with(&format!(
        "[[entity]]\nname = \"hint\"\nprops = {{ msg = \"\" }}\n\n[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\n{connect}"
    ))
}

#[test]
fn action_lists_are_checked() {
    let ok = "actions = [\n  { action = \"spawn\", prefab = \"house\" },\n  \
              { action = \"add\", to = \"town\", prop = \"coins\", value = -3 },\n  \
              { action = \"set\", to = \"hint\", prop = \"msg\", value = \"\" },\n]";
    assert_eq!(grouped(ok), Vec::<&str>::new());
    // Exactly one of `action` and `actions`; nothing else at the top.
    assert_eq!(grouped(&format!("action = \"spawn\"\nprefab = \"house\"\n{ok}")), vec!["E0105"]);
    assert_eq!(grouped("when = []"), vec!["E0104"]);
    assert_eq!(grouped("to = \"town\"\nactions = [{ action = \"disable\", to = \"town\" }]"), vec!["E0105"]);
    assert_eq!(grouped("actions = []"), vec!["E0105"]);
    // Each item is checked like a single action, at its own place.
    assert_eq!(grouped("actions = [{ to = \"town\" }]"), vec!["E0104"]);
    assert_eq!(grouped("actions = [{ action = \"disable\", to = \"twon\" }]"), vec!["E0202"]);
    assert_eq!(grouped("actions = [{ action = \"add\", to = \"town\", prop = \"gold\" }]"), vec!["E0206"]);
    assert_eq!(grouped("actions = [{ action = \"spawn\" }]"), vec!["E0104"]);
    assert_eq!(grouped("actions = [{ action = \"spawn\", prefab = \"hous\" }]"), vec!["E0201"]);
    // An item that sets a prop makes its target emit `changed` (no W0203).
    let changed = "actions = [{ action = \"set\", to = \"hint\", prop = \"msg\", value = \"x\" }]\n\n[[connect]]\n\
                   from = \"hint\"\nsignal = \"changed\"\nto = \"town\"\naction = \"disable\"";
    assert_eq!(grouped(changed), Vec::<&str>::new());
}

#[test]
fn changed_filters_only_action_events() {
    let vfs = |expect: &str| {
        project(&[
            ("scenes/main.toml", "[ground]\nsize = [4, 4]\n"),
            ("tests/t.toml", &format!("[test]\nscene = \"scenes/main.toml\"\nframes = 10\n\n[[expect]]\n{expect}\n")),
        ])
    };
    assert_eq!(codes(&vfs("event = \"action\"\nchanged = true\nat_least = 0")), Vec::<&str>::new());
    assert_eq!(codes(&vfs("event = \"skipped\"\nchanged = true\nat_least = 0")), vec!["E0105"]);
}

// ---------------------------------------------------------------------------
// Spawn costs (ADR 0016)

fn costed(cost: &str, extra: &str) -> Vec<&'static str> {
    builder_with(&format!(
        "[[entity]]\nname = \"sign\"\nprops = {{ msg = \"\" }}\ncounts = {{ houses = \"house\" }}\n\n[[connect]]\n\
         from = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"house\"\ncost = {cost}\n{extra}"
    ))
}

#[test]
fn spawn_costs_are_checked() {
    let ok = "{ entity = \"town\", prop = \"coins\", amount = 3 }";
    assert_eq!(costed(ok, ""), Vec::<&str>::new());
    // The payer and its prop must exist, be a number and not a count.
    assert_eq!(costed("{ entity = \"twon\", prop = \"coins\", amount = 3 }", ""), vec!["E0202"]);
    assert_eq!(costed("{ entity = \"town\", prop = \"gold\", amount = 3 }", ""), vec!["E0206"]);
    assert_eq!(costed("{ entity = \"sign\", prop = \"msg\", amount = 3 }", ""), vec!["E0207"]);
    assert_eq!(costed("{ entity = \"sign\", prop = \"houses\", amount = 1 }", ""), vec!["E0105"]);
    // The amount is above 0 and fits the prop.
    assert_eq!(costed("{ entity = \"town\", prop = \"coins\", amount = 0 }", ""), vec!["E0105"]);
    assert_eq!(costed("{ entity = \"town\", prop = \"coins\", amount = -2 }", ""), vec!["E0105"]);
    assert_eq!(costed("{ entity = \"town\", prop = \"coins\", amount = 1.5 }", ""), vec!["E0207"]);
    assert_eq!(costed("{ entity = \"town\", prop = \"coins\" }", ""), vec!["E0104"]);
    // Only spawns cost something; in a list, `cost` goes in the spawn item.
    assert_eq!(
        builder_with(&format!(
            "[[connect]]\nfrom = \"town\"\nsignal = \"tapped\"\nto = \"town\"\naction = \"disable\"\ncost = {ok}"
        )),
        vec!["E0105"]
    );
    assert_eq!(
        builder_with(&format!(
            "[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\ncost = {ok}\nactions = [{{ action = \"spawn\", prefab = \"house\" }}]"
        )),
        vec!["E0105"]
    );
    // `funds` is a refusal reason; paying makes the payer emit `changed`.
    let funds = "\n[[connect]]\nfrom = \"ground\"\nsignal = \"refused\"\nto = \"sign\"\naction = \"set\"\nprop = \"msg\"\n\
                 value = \"too dear\"\nwhen = [{ reason = \"funds\" }]\n\n[[connect]]\nfrom = \"town\"\n\
                 signal = \"changed\"\nto = \"sign\"\naction = \"set\"\nprop = \"msg\"\nvalue = \"paid\"";
    assert_eq!(costed(ok, funds), Vec::<&str>::new());
}

#[test]
fn every_copy_of_a_prefab_shares_its_shape_list() {
    let house = "footprint = [2, 2]\n\n[[shape]]\nkind = \"box\"\nsize = [2, 2, 1.5]\n\n[[shape]]\nkind = \"roof\"\nat = [0, 0, 1.5]\nsize = [2, 2, 0.75]\ncolor = \"#8b2f2f\"\nturn = 90\n";
    let scene = "[[entity]]\nprefab = \"house\"\ncells = [[0, 0], [2, 0]]\n\n[[entity]]\nname = \"third\"\nprefab = \"house\"\npos = [4, 0]\n\n[[entity]]\nname = \"shed\"\nprefab = \"house\"\npos = [0, 2]\nshape = [{ kind = \"wedge\", size = [2, 2, 1], turn = 270 }]\n\n[[entity]]\nname = \"lot\"\nprefab = \"house\"\npos = [2, 2]\nshape = []\n\n[[connect]]\nfrom = \"third\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"house\"\n";
    let vfs = project(&[("prefabs/house.toml", house), ("scenes/main.toml", scene)]);
    assert_eq!(codes(&vfs), Vec::<&str>::new());
    let mut loader = crate::SceneLoader::new(&vfs);
    let def = loader.load_scene("scenes/main.toml").expect("the scene loads");
    let shapes = |i: usize| def.entities[i].shapes.as_ref();
    let list = shapes(0).expect("the prefab's list");
    assert_eq!(list.len(), 2);
    assert_eq!(
        (list[1].kind, list[1].at, list[1].size, list[1].turn),
        (crate::ShapeKind::Roof, [0.0, 0.0, 1.5], [2.0, 2.0, 0.75], 1)
    );
    assert_eq!(list[0].color, rwgpuengine_core::color::Rgba::parse_hex("#9aa0a6").unwrap());
    // One list for every copy, placed or spawned: not one each.
    for other in [shapes(1), shapes(2), def.templates[0].shapes.as_ref()] {
        assert!(std::sync::Arc::ptr_eq(list, other.expect("a copy has the list")));
    }
    // A list of its own replaces the prefab's; an empty one leaves none.
    let own = shapes(3).expect("its own list");
    assert_eq!((own.len(), own[0].kind, own[0].turn), (1, crate::ShapeKind::Wedge, 3));
    assert!(shapes(4).is_none());
    // A file that has no shapes reads as it did before there were any.
    let plain = format!("{:?}", def.entities[4]);
    assert!(!plain.contains("shapes") && plain.ends_with("} }"), "{plain}");
    assert!(format!("{:?}", def.entities[0]).contains("shapes: [ShapeDef { kind: Box"));
}
