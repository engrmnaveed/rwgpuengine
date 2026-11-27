//! The loader's oracle (ADR 0028): what the loader says about a corpus of
//! projects, pinned.
//!
//! Agents work from the diagnostics: code, severity, file, position, width,
//! message, hint, notes and the source line, in order. This module records
//! all of it for every case of the corpus, and a hash of what each file that
//! loads decodes to (its definitions, every field). [`PINS`] holds a hash of
//! each record. They were recorded from the loader as it was before it
//! became a pipeline and before it was made linear. A change to the loader
//! must keep every one; a pin is not updated to make a test pass. A pin that
//! cannot be kept is a finding for the owner.
//!
//! Each case is recorded the ways the engine calls the loader:
//! - `check_project` (`rwgpu check`, `rwgpu export web`): sorted and
//!   deduplicated, with the lists of scenes, prefabs and playtests;
//! - `load_project`;
//! - one `SceneLoader` per scene, as a session loads one (`rwgpu run`, `shot`,
//!   `inspect`, the players): in the order the loader writes them;
//! - `load_playtest` per playtest.
//!
//! The corpus: the three example games, every input of the loader tests in
//! `tests.rs`, inputs made to reach every place the loader reports a problem
//! (alone, several in one file, and spread over the files of one project),
//! and generated worlds of a thousand entities. The cases under `limits/`
//! are sizes over the limits of `rwgpuengine_core::limits` (E0209); they are
//! new with the limits, and the only cases whose records the limits changed.
//!
//! `cargo test -p rwgpuengine_data corpus::print -- --ignored --nocapture`
//! prints the lines of [`PINS`]. With `CORPUS_OUT=<file>` it also writes every
//! record to that file, so that two versions of the loader can be diffed.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_core::hash::{Hasher64, hex};

use crate::load::{SceneLoader, Vfs, check_project, load_playtest, load_project};
use crate::tests as t;

/// One file set of the corpus.
pub(crate) struct Case {
    pub name: String,
    pub vfs: Vfs,
    /// Playtests loaded by path that are not under `tests/` (or do not exist).
    pub extra_playtests: Vec<String>,
}

fn case(name: impl Into<String>, vfs: Vfs) -> Case {
    Case { name: name.into(), vfs, extra_playtests: Vec::new() }
}

// ---------------------------------------------------------------------------
// Records

fn diag_line(out: &mut String, d: &Diagnostic) {
    let place = match (&d.file, d.line, d.column) {
        (Some(f), Some(l), Some(c)) => format!("{f}:{l}:{c}+{}", d.width),
        (Some(f), _, _) => f.clone(),
        _ => "-".to_string(),
    };
    let _ = write!(out, "  {} {} {place} {:?}", d.code, d.severity.as_str(), d.message);
    if let Some(h) = &d.hint {
        let _ = write!(out, " hint={h:?}");
    }
    for n in &d.notes {
        let _ = write!(out, " note={n:?}");
    }
    if let Some(s) = &d.source_line {
        let _ = write!(out, " src={s:?}");
    }
    out.push('\n');
}

fn diag_lines(out: &mut String, diags: &[Diagnostic]) {
    for d in diags {
        diag_line(out, d);
    }
}

/// A hash of everything a definition holds, or `-` when nothing loaded.
fn defs_hash<T: std::fmt::Debug>(def: Option<&T>) -> String {
    def.map_or_else(|| "-".to_string(), |d| hex(Hasher64::new().str(&format!("{d:?}")).finish()))
}

/// Everything the loader says about `case`, as text.
pub(crate) fn record(case: &Case) -> String {
    let vfs = &case.vfs;
    let mut out = String::new();
    let report = check_project(vfs);
    let _ = writeln!(
        out,
        "check ok={} project={} scenes={:?} prefabs={:?} playtests={:?}",
        report.ok(),
        report.project.as_ref().map_or("-", |p| p.name.as_str()),
        report.scenes,
        report.prefabs,
        report.playtests
    );
    diag_lines(&mut out, &report.diagnostics);

    let (project, diags) = load_project(vfs);
    let _ = writeln!(out, "project {}", defs_hash(project.as_ref()));
    diag_lines(&mut out, &diags);

    // Every scene a session could be asked to load: scenes/, the main scene
    // and the scene of every playtest.
    let mut playtests: Vec<String> = vfs.paths_under("tests", ".toml").map(str::to_string).collect();
    playtests.extend(case.extra_playtests.iter().cloned());
    let mut scenes: BTreeSet<String> = vfs.paths_under("scenes", ".toml").map(str::to_string).collect();
    if let Some(p) = &project {
        scenes.insert(p.main_scene.clone());
    }
    let mut tests = Vec::new();
    for path in &playtests {
        let (def, diags) = load_playtest(vfs, path);
        if let Some(scene) = def.as_ref().and_then(|d| d.scene.clone()) {
            scenes.insert(scene);
        }
        tests.push((path, def, diags));
    }
    for path in scenes.iter().filter(|p| vfs.contains(p)) {
        let mut loader = SceneLoader::new(vfs);
        let scene = loader.load_scene(path);
        let _ = writeln!(out, "scene {path} {}", defs_hash(scene.as_ref()));
        diag_lines(&mut out, &loader.diags);
    }
    for (path, def, diags) in &tests {
        let _ = writeln!(out, "playtest {path} {}", defs_hash(def.as_ref()));
        diag_lines(&mut out, diags);
    }
    out
}

fn record_hash(record: &str) -> String {
    hex(Hasher64::new().str(record).finish())
}

// ---------------------------------------------------------------------------
// The base project: a scene that uses most of the formats and checks clean.

const PROJECT: &str = "[project]\nname = \"corpus\"\n";

const GROUND: &str = r##"[scene]
name = "corpus"
doc = "Every format the loader checks, with no problem in it."
seed = 4

[camera]
center = [4, 3]
turn = 30
zoom = 1.5

[ground]
map = ["gggggggg", "gggggggg", "ggggwwgg", "gggggggg", "ssgggggg", "gggggggg"]

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
color = "#d8c890"
alt_color = "#c8b880"
"##;

const ENTITIES: &str = r##"
[[entity]]
name = "player"
pos = [0, 0]
sprite = { color = "#fc0", shape = "diamond", size = [20, 28], offset = [0, -2] }
mover = { step_frames = 4 }

[[entity]]
name = "plate"
pos = [1, 1]
tile = { color = "#c9b26b" }
trigger = {}

[[entity]]
name = "pad"
pos = [3, 1]
tile = {}
trigger = { on_enter = "on", on_exit = "off", only = "player" }

[[entity]]
name = "door"
prefab = "wall"
pos = [5, 1]
block = { color = "#a33", top = "#c55" }

[[entity]]
prefab = "wall"
cells = [[7, 0], [7, 1]]

[[entity]]
name = "crate"
pos = [2, 3, 0.5]
block = { color = "#a63", height = 0.5 }
pushable = true

[[entity]]
name = "guard"
pos = [6, 5]
sprite = {}
patrol = { path = [[6, 5], [7, 5]], mode = "pingpong", step_frames = 10 }

[[entity]]
name = "chick"
pos = [1, 5]
sprite = { shape = "circle" }
wander = { step_frames = 20, area = [[2, 5], [0, 4]] }

[[entity]]
name = "hud"
screen_pos = [8, 8]
label = { text = "Coins: {coins} {mode} {{ok}}", scale = 2, color = "#fff", offset = [1, 1] }
props = { coins = 0, mode = "play", rate = 0.5, on = false }

[[entity]]
name = "town"
props = { coins = 10 }

[[entity]]
name = "job"
counts = { houses = "house" }

[[entity]]
name = "hint"
props = { msg = "" }

[[entity]]
prefab = "house"
pos = [4, 3]

[[entity]]
name = "spare"
prefab = "coin"
pos = [6, 2]
enabled = false

[[connect]]
from = "plate"
signal = "pressed"
to = "door"
action = "disable"
when = [{ entity = "pad", occupied = false }]

[[connect]]
from = "pad"
signal = "on"
to = "hud"
action = "add"
prop = "coins"

[[connect]]
from = "ground"
signal = "tapped"
action = "spawn"
prefab = "house"
cost = { entity = "town", prop = "coins", amount = 3 }

[[connect]]
from_prefab = "house"
signal = "tapped"
to = "self"
action = "disable"

[[connect]]
from = "hud"
signal = "changed"
to = "hint"
action = "set"
prop = "msg"
value = "rich"
when = [{ entity = "hud", prop = "coins", at_least = 3 }, { entity = "door", enabled = false }]

[[connect]]
from = "ground"
signal = "refused"
actions = [{ action = "set", to = "hint", prop = "msg", value = "no room" }, { action = "add", to = "hud", prop = "rate", value = 1 }]
when = [{ reason = "occupied" }]
"##;

const BASE_TEST: &str = r#"[test]
doc = "The player takes one step."
frames = 60
seed = 9

[[input]]
frame = 1
press = "right"

[[input]]
frame = 5
hold = "down"
for = 8

[[input]]
frame = 20
tap = [320, 200]

[[input]]
frame = 21
camera = { turn = 90, zoom = 2, pan = [1, 0], view = "perspective" }

[[expect]]
entity = "player"
pos = [1, 0]
frame = 2

[[expect]]
event = "moved"
entity = "player"
at_least = 1

[[expect]]
prefab = "house"
count = 1

[[shot]]
frame = 30
reference = "tests/refs/base.png"
max_diff_pixels = 4
"#;

const WALL: &str = "doc = \"A wall.\"\nblock = { color = \"#888\" }\nsolid = true\n";
const HOUSE: &str = "footprint = [2, 2]\nblock = { color = \"#eee\", top = \"#c33\" }\nsolid = true\n";
const COIN: &str =
    "sprite = { color = \"#fd0\", size = [12, 12] }\ntrigger = { on_enter = \"collected\", only = \"player\" }\n";
const HUT: &str = "block = { color = \"#a85\", height = 0.8 }\nsolid = true\n";

fn base_scene() -> String {
    format!("{GROUND}{ENTITIES}")
}

/// The base project with `files` added or replaced.
fn with(files: &[(&str, &str)]) -> Vfs {
    let mut vfs = Vfs::new();
    vfs.insert("rwgpu.toml", PROJECT);
    vfs.insert("scenes/main.toml", base_scene());
    vfs.insert("tests/base.toml", BASE_TEST);
    for (path, text) in [
        ("prefabs/wall.toml", WALL),
        ("prefabs/house.toml", HOUSE),
        ("prefabs/coin.toml", COIN),
        ("prefabs/hut.toml", HUT),
    ] {
        vfs.insert(path, text);
    }
    for (path, text) in files {
        vfs.insert(*path, *text);
    }
    vfs
}

/// The base project with `extra` at the end of its scene.
fn scene_with(extra: &str) -> Vfs {
    let scene = format!("{}\n{extra}", base_scene());
    with(&[("scenes/main.toml", &scene)])
}

/// Only these files.
fn bare(files: &[(&str, &str)]) -> Vfs {
    let mut vfs = Vfs::new();
    for (path, text) in files {
        vfs.insert(*path, *text);
    }
    vfs
}

/// A project of one scene with nothing else.
fn bare_scene(text: &str) -> Vfs {
    bare(&[("rwgpu.toml", PROJECT), ("scenes/main.toml", text)])
}

// ---------------------------------------------------------------------------
// Inputs that reach each place the loader reports a problem

/// The project file: (name, rwgpu.toml).
const PROJECT_FILES: &[(&str, &str)] = &[
    ("syntax", "[project\nname = \"t\"\n"),
    ("empty", ""),
    ("unknown-key", "[project]\nname = \"t\"\nnmae = \"x\"\n"),
    ("unknown-section", "[project]\nname = \"t\"\n\n[render]\nfast = true\n"),
    ("missing-name", "[project]\nmain_scene = \"scenes/main.toml\"\n"),
    ("wrong-type", "[project]\nname = 3\n"),
    ("display-range", "[project]\nname = \"t\"\n\n[display]\nwidth = 0\nheight = 99999\nbackground = \"blue\"\n"),
    ("sim-range", "[project]\nname = \"t\"\n\n[sim]\ntick_rate = 5000\nseed = -1\n"),
    ("iso-range", "[project]\nname = \"t\"\n\n[iso]\ntile_width = 2\ntile_height = 2.5\n"),
    ("main-missing-hint", "[project]\nname = \"t\"\nmain_scene = \"scenes/mian.toml\"\n"),
    ("main-missing-no-hint", "[project]\nname = \"t\"\nmain_scene = \"levels/zzz.toml\"\n"),
    ("main-path-invalid", "[project]\nname = \"t\"\nmain_scene = \"/abs/main.toml\"\n"),
    ("main-path-backslash", "[project]\nname = \"t\"\nmain_scene = \"scenes\\\\main.toml\"\n"),
    ("engine-mismatch", "[project]\nname = \"t\"\nengine = \"9.9\"\n"),
    ("engine-match", "[project]\nname = \"t\"\nengine = \"0.1\"\n"),
    ("engine-patch", "[project]\nname = \"t\"\nengine = \"0.1.0\"\n"),
    ("engine-and-main-missing", "[project]\nname = \"t\"\nengine = \"0.2\"\nmain_scene = \"scenes/nope.toml\"\n"),
    ("display-ok", "[project]\nname = \"t\"\n\n[display]\nwidth = 320\nheight = 240\nbackground = \"#000\"\n"),
    ("project-not-a-table", "project = \"t\"\n"),
];

/// Entities, appended to the base scene: (name, text).
const ENTITY_SNIPS: &[(&str, &str)] = &[
    ("unknown-prefab-hint", "[[entity]]\nprefab = \"wal\"\npos = [1, 2]\n"),
    ("unknown-prefab-no-hint", "[[entity]]\nprefab = \"zeppelin\"\npos = [1, 2]\n"),
    ("unknown-prefab-subfolder", "[[entity]]\nprefab = \"props/wall\"\npos = [1, 2]\n"),
    ("name-with-cells", "[[entity]]\nname = \"rocks\"\nprefab = \"wall\"\ncells = [[0, 2], [1, 2]]\n"),
    ("duplicate-name", "[[entity]]\nname = \"plate\"\npos = [2, 2]\ntile = {}\n"),
    (
        "duplicate-name-twice",
        "[[entity]]\nname = \"hint\"\npos = [2, 2]\n\n[[entity]]\nname = \"hint\"\npos = [3, 2]\n",
    ),
    ("reserved-ground", "[[entity]]\nname = \"ground\"\npos = [2, 2]\ntile = {}\n"),
    ("reserved-self", "[[entity]]\nname = \"self\"\npos = [2, 2]\n"),
    ("props-and-counts", "[[entity]]\nname = \"tally\"\nprops = { huts = 1 }\ncounts = { huts = \"hut\" }\n"),
    (
        "footprint-and-behaviors",
        "[[entity]]\nname = \"big\"\npos = [0, 2]\nfootprint = [2, 1]\nmover = {}\nwander = {}\nsprite = {}\n",
    ),
    ("footprint-pushable", "[[entity]]\npos = [0, 2]\nfootprint = [1, 2]\npushable = true\nblock = {}\n"),
    (
        "footprint-screen",
        "[[entity]]\nname = \"panel\"\nscreen_pos = [0, 0]\nfootprint = [3, 1]\nlabel = { text = \"x\" }\n",
    ),
    ("footprint-zero", "[[entity]]\nname = \"flat\"\npos = [0, 2]\nfootprint = [0, -3]\ntile = {}\n"),
    (
        "label-unclosed",
        "[[entity]]\nname = \"sign\"\npos = [2, 2]\nlabel = { text = \"Score {points\" }\nprops = { points = 0 }\n",
    ),
    ("label-single-brace", "[[entity]]\nname = \"sign\"\npos = [2, 2]\nlabel = { text = \"a } b\" }\n"),
    ("label-bad-placeholder", "[[entity]]\nname = \"sign\"\npos = [2, 2]\nlabel = { text = \"{a b}\" }\n"),
    ("label-empty-placeholder", "[[entity]]\nname = \"sign\"\npos = [2, 2]\nlabel = { text = \"{}\" }\n"),
    (
        "label-unknown-prop-hint",
        "[[entity]]\nname = \"sign\"\npos = [2, 2]\nlabel = { text = \"Score {point}\" }\nprops = { points = 0 }\n",
    ),
    ("label-unknown-props-no-hint", "[[entity]]\npos = [2, 2]\nlabel = { text = \"{zzz} and {qqq}\" }\n"),
    (
        "label-unsupported-chars",
        "[[entity]]\nname = \"sign\"\npos = [2, 2]\nlabel = { text = \"Caf\u{e9} \u{f1} \u{e9}\\t\" }\n",
    ),
    (
        "label-unsupported-allowed",
        "[[entity]]\nname = \"sign\"\npos = [2, 2]\nlabel = { text = \"Caf\u{e9}\" }\nallow = [\"w0207\"]\n",
    ),
    ("label-escaped-braces", "[[entity]]\nname = \"sign\"\npos = [2, 2]\nlabel = { text = \"{{x}} }}\" }\n"),
    (
        "label-on-prefab",
        "[[entity]]\nname = \"sign\"\nprefab = \"coin\"\npos = [2, 2]\nlabel = { text = \"{n}\" }\nprops = { n = 1 }\n",
    ),
    (
        "pushable-moving",
        "[[entity]]\nname = \"box\"\npos = [2, 2]\nblock = {}\npushable = true\nmover = {}\npatrol = { path = [[2, 2]] }\n",
    ),
    (
        "screen-world-parts",
        "[[entity]]\nname = \"badge\"\nscreen_pos = [4, 4]\nblock = {}\ntrigger = {}\nsolid = true\npushable = true\ntile = {}\n",
    ),
    ("screen-movers", "[[entity]]\nscreen_pos = [4, 4]\nmover = {}\npatrol = { path = [[0, 0]] }\nwander = {}\n"),
    ("counts-unknown-prefab-hint", "[[entity]]\nname = \"tally\"\ncounts = { huts = \"hutt\" }\n"),
    ("counts-unknown-prefab-no-hint", "[[entity]]\nname = \"tally\"\ncounts = { a = \"zeppelin\", b = \"airship\" }\n"),
    (
        "cells-checked-once",
        "[[entity]]\npos = [0, 0, 2]\ncells = [[0, 2], [1, 2], [2, 2]]\nlabel = { text = \"{nothing}\" }\ncounts = { n = \"nope\" }\n",
    ),
    ("cells-one-checked", "[[entity]]\ncells = [[0, 2]]\nlabel = { text = \"{zz}\" }\ncounts = { n = \"nope\" }\n"),
    (
        "rules-and-counts",
        "[[entity]]\nname = \"x\"\npos = [0, 2]\nfootprint = [2, 1]\nmover = {}\ncounts = { n = \"nope\" }\nlabel = { text = \"{zz}\" }\n",
    ),
    ("cells-duplicates", "[[entity]]\nprefab = \"coin\"\ncells = [[0, 2], [0, 2]]\n"),
    ("cells-bad-item", "[[entity]]\nprefab = \"coin\"\ncells = [[0, 2], [1], \"x\", [1.5, 2]]\n"),
    ("cells-empty", "[[entity]]\nprefab = \"coin\"\ncells = []\n"),
    (
        "pos-wrong",
        "[[entity]]\nname = \"p1\"\npos = \"here\"\n\n[[entity]]\nname = \"p2\"\npos = [1]\n\n[[entity]]\nname = \"p3\"\npos = [1, 2, 3, 4]\n",
    ),
    ("pos-not-finite", "[[entity]]\nname = \"p1\"\npos = [nan, 0]\n\n[[entity]]\nname = \"p2\"\npos = [inf, 1]\n"),
    (
        "pos-rounding",
        "[[entity]]\nname = \"half\"\npos = [4.5, 1.49]\ntile = {}\n\n[[entity]]\nname = \"edge\"\npos = [7.6, -0.5]\ntile = {}\n",
    ),
    ("name-invalid", "[[entity]]\nname = \"a b\"\npos = [2, 2]\n\n[[entity]]\nname = \"\"\npos = [2, 2]\n"),
    ("name-wrong-type", "[[entity]]\nname = 7\npos = [2, 2]\n"),
    ("unknown-key-hint", "[[entity]]\nname = \"e\"\nsprit = { color = \"#fff\" }\n"),
    ("unknown-key-no-hint", "[[entity]]\nname = \"e\"\nzzzzzzzz = 1\n"),
    ("unknown-component-key", "[[entity]]\nname = \"e\"\nsprite = { colour = \"#fff\", sise = [1, 1] }\n"),
    ("component-wrong-type", "[[entity]]\nname = \"e\"\nsprite = 3\nblock = \"tall\"\nmover = []\n"),
    ("color-invalid", "[[entity]]\nname = \"e\"\nsprite = { color = \"orange\" }\ntile = { color = \"#12345\" }\n"),
    (
        "enum-invalid",
        "[[entity]]\nname = \"e\"\nsprite = { shape = \"circel\" }\npatrol = { path = [[0, 0]], mode = \"bounce\" }\n",
    ),
    ("int-range", "[[entity]]\nname = \"e\"\nlabel = { text = \"x\", scale = 9 }\nmover = { step_frames = 0 }\n"),
    ("num-range", "[[entity]]\nname = \"e\"\nblock = { height = 100.0 }\n"),
    ("props-nested", "[[entity]]\nname = \"e\"\nprops = { a = [1], b = { c = 1 }, d = 1, e = nan }\n"),
    ("props-wrong-type", "[[entity]]\nname = \"e\"\nprops = 3\ncounts = [\"x\"]\n"),
    ("counts-not-names", "[[entity]]\nname = \"e\"\ncounts = { a = 3, b = \"x y\" }\n"),
    ("allow-invalid", "[[entity]]\nname = \"e\"\nallow = [\"E0101\", 3, \"W0210\", \"nonsense\", \"W0201\"]\n"),
    ("allow-wrong-type", "[[entity]]\nname = \"e\"\nallow = \"W0201\"\n"),
    ("missing-required", "[[entity]]\nname = \"e\"\nlabel = {}\npatrol = {}\n"),
    ("bool-wrong-type", "[[entity]]\nname = \"e\"\nsolid = \"yes\"\nenabled = 1\npushable = 0\n"),
];

/// Static placement, appended to the base scene.
const PLACEMENT_SNIPS: &[(&str, &str)] = &[
    ("solid-overlap", "[[entity]]\nprefab = \"wall\"\npos = [5, 1]\n"),
    ("solid-overlap-allowed-first", "[[entity]]\nprefab = \"wall\"\npos = [5, 1]\nallow = [\"W0201\"]\n"),
    ("solid-overlap-footprint", "[[entity]]\nname = \"shed\"\nprefab = \"house\"\npos = [6, 0]\n"),
    ("solid-overlap-three", "[[entity]]\nprefab = \"wall\"\ncells = [[1, 3], [1, 3], [1, 3]]\n"),
    ("solid-disabled-ignored", "[[entity]]\nprefab = \"wall\"\npos = [5, 1]\nenabled = false\n"),
    ("pushable-on-wall", "[[entity]]\nname = \"box\"\npos = [7, 1]\nblock = {}\npushable = true\n"),
    ("actor-in-solid", "[[entity]]\nname = \"ghost\"\npos = [5, 1]\nsprite = {}\nwander = {}\n"),
    ("actor-in-solid-unnamed", "[[entity]]\npos = [7, 0]\nsprite = {}\npatrol = { path = [[7, 0]] }\n"),
    (
        "actor-in-solid-allowed",
        "[[entity]]\nname = \"ghost\"\npos = [5, 1]\nsprite = {}\nwander = {}\nallow = [\"W0206\"]\n",
    ),
    ("actor-in-house", "[[entity]]\nname = \"ghost\"\npos = [5, 4]\nsprite = {}\nwander = {}\n"),
    (
        "actor-in-disabled-solid",
        "[[entity]]\nprefab = \"wall\"\npos = [2, 5]\nenabled = false\n\n[[entity]]\nname = \"ghost\"\npos = [2, 5]\nsprite = {}\nmover = {}\n",
    ),
    ("actor-on-pushable", "[[entity]]\nname = \"ghost\"\npos = [2, 3]\nsprite = {}\nwander = {}\n"),
    ("actor-on-actor", "[[entity]]\nname = \"twin\"\npos = [0, 0]\nsprite = {}\nmover = {}\n"),
    ("disabled-actor-in-solid", "[[entity]]\nname = \"ghost\"\npos = [5, 1]\nwander = {}\nenabled = false\n"),
    (
        "actor-in-solid-actor",
        "[[entity]]\nname = \"rock\"\npos = [3, 3]\nsolid = true\nwander = {}\n\n[[entity]]\nname = \"ghost\"\npos = [3, 3]\nwander = {}\n",
    ),
    ("outside-ground", "[[entity]]\nname = \"far\"\npos = [9, 9]\ntile = {}\n"),
    ("outside-ground-negative", "[[entity]]\npos = [-1, 0]\ntile = {}\n"),
    ("outside-ground-footprint", "[[entity]]\nname = \"barn\"\nprefab = \"house\"\npos = [7, 5]\n"),
    ("outside-ground-screen", "[[entity]]\nname = \"far\"\nscreen_pos = [900, 900]\nlabel = { text = \"x\" }\n"),
    ("outside-ground-allowed", "[[entity]]\npos = [9, 9]\ntile = {}\nallow = [\"W0202\"]\n"),
    ("outside-ground-cells", "[[entity]]\nprefab = \"coin\"\ncells = [[8, 0], [0, 8], [1, 1], [-2, -2]]\n"),
    (
        "three-warnings-one-entity",
        "[[entity]]\nprefab = \"wall\"\npos = [9, 0]\n\n[[entity]]\nname = \"lost\"\npos = [9, 0]\nsprite = {}\nmover = {}\nsolid = true\n",
    ),
    ("data-entity-anywhere", "[[entity]]\nname = \"data\"\npos = [99, 99]\nprops = { a = 1 }\n"),
];

/// [[connect]] tables, appended to the base scene.
const CONNECT_SNIPS: &[(&str, &str)] = &[
    (
        "from-both",
        "[[connect]]\nfrom = \"plate\"\nfrom_prefab = \"house\"\nsignal = \"pressed\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    ("from-neither", "[[connect]]\nsignal = \"pressed\"\nto = \"door\"\naction = \"enable\"\n"),
    ("from-unknown-hint", "[[connect]]\nfrom = \"plat\"\nsignal = \"pressed\"\nto = \"door\"\naction = \"enable\"\n"),
    (
        "from-unknown-no-hint",
        "[[connect]]\nfrom = \"zeppelin\"\nsignal = \"pressed\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    ("from-cells-entity", "[[connect]]\nfrom = \"wall\"\nsignal = \"tapped\"\nto = \"door\"\naction = \"enable\"\n"),
    (
        "from-prefab-unknown",
        "[[connect]]\nfrom_prefab = \"hous\"\nsignal = \"tapped\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    (
        "from-prefab-unknown-no-hint",
        "[[connect]]\nfrom_prefab = \"zeppelin\"\nsignal = \"tapped\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    ("signal-typo", "[[connect]]\nfrom = \"plate\"\nsignal = \"presed\"\nto = \"door\"\naction = \"enable\"\n"),
    (
        "signal-changed-never",
        "[[connect]]\nfrom = \"door\"\nsignal = \"changed\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    (
        "signal-spawned-never",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"spawned\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    (
        "signal-refused-never",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"refused\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    ("signal-other-never", "[[connect]]\nfrom = \"plate\"\nsignal = \"zzz\"\nto = \"door\"\naction = \"enable\"\n"),
    ("signal-custom-trigger", "[[connect]]\nfrom = \"pad\"\nsignal = \"off\"\nto = \"door\"\naction = \"enable\"\n"),
    (
        "signal-custom-trigger-typo",
        "[[connect]]\nfrom = \"pad\"\nsignal = \"of\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    (
        "signal-prefab-pressed",
        "[[connect]]\nfrom_prefab = \"house\"\nsignal = \"pressed\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    (
        "signal-prefab-spawned",
        "[[connect]]\nfrom_prefab = \"house\"\nsignal = \"spawned\"\nto = \"town\"\naction = \"add\"\nprop = \"coins\"\nvalue = -1\n",
    ),
    (
        "signal-prefab-changed-never",
        "[[connect]]\nfrom_prefab = \"wall\"\nsignal = \"changed\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    (
        "signal-prefab-refused-never",
        "[[connect]]\nfrom_prefab = \"wall\"\nsignal = \"refused\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    (
        "signal-prefab-spawned-never",
        "[[connect]]\nfrom_prefab = \"coin\"\nsignal = \"spawned\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    (
        "signal-prefab-collected",
        "[[connect]]\nfrom_prefab = \"coin\"\nsignal = \"collected\"\nto = \"hud\"\naction = \"add\"\nprop = \"coins\"\n",
    ),
    (
        "signal-prefab-changed-named-copy",
        "[[entity]]\nname = \"bank\"\nprefab = \"coin\"\npos = [2, 2]\nprops = { n = 0 }\n\n[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"bank\"\naction = \"add\"\nprop = \"n\"\n\n[[connect]]\nfrom_prefab = \"coin\"\nsignal = \"changed\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    (
        "signal-prefab-refused-named-copy",
        "[[entity]]\nname = \"shop\"\nprefab = \"hut\"\npos = [3, 5]\n\n[[connect]]\nfrom = \"shop\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"hut\"\n\n[[connect]]\nfrom_prefab = \"hut\"\nsignal = \"refused\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    (
        "signal-prefab-refused-self",
        "[[connect]]\nfrom_prefab = \"hut\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"hut\"\n\n[[connect]]\nfrom_prefab = \"hut\"\nsignal = \"refused\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    (
        "signal-ground-pressed",
        "[[connect]]\nfrom = \"ground\"\nsignal = \"pressed\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    ("signal-ground-taped", "[[connect]]\nfrom = \"ground\"\nsignal = \"taped\"\nto = \"door\"\naction = \"enable\"\n"),
    (
        "signal-ground-changed",
        "[[connect]]\nfrom = \"ground\"\nsignal = \"changed\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    (
        "signal-counts-changed",
        "[[connect]]\nfrom = \"job\"\nsignal = \"changed\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    (
        "signal-cost-payer-changed",
        "[[connect]]\nfrom = \"town\"\nsignal = \"changed\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    (
        "signal-self-changed",
        "[[connect]]\nfrom = \"hint\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"set\"\nprop = \"msg\"\nvalue = \"x\"\n\n[[connect]]\nfrom = \"hint\"\nsignal = \"changed\"\nto = \"door\"\naction = \"enable\"\n",
    ),
    (
        "signal-and-form",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"zzz\"\nto = \"door\"\naction = \"enable\"\nactions = [{ action = \"disable\", to = \"door\" }]\n",
    ),
    (
        "spawn-to-and-prefab",
        "[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\nto = \"town\"\n\n[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\nto = \"town\"\nprefab = \"hutt\"\n",
    ),
    (
        "action-both",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"door\"\naction = \"enable\"\nactions = [{ action = \"disable\", to = \"door\" }]\n",
    ),
    ("action-neither", "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"door\"\n"),
    (
        "actions-top-keys",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"door\"\nprop = \"x\"\nvalue = 1\nprefab = \"hut\"\ncost = { entity = \"town\", prop = \"coins\", amount = 1 }\nactions = [{ action = \"disable\", to = \"door\" }]\n",
    ),
    ("actions-empty", "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nactions = []\n"),
    ("actions-not-tables", "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nactions = [1, \"x\"]\n"),
    (
        "actions-each-checked",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nactions = [{ to = \"door\" }, { action = \"disable\", to = \"dor\" }, { action = \"add\", to = \"hud\", prop = \"gold\" }, { action = \"spawn\" }, { action = \"spawn\", prefab = \"hutt\" }, { action = \"set\", to = \"hud\", prop = \"coins\" }, { action = \"enable\", to = \"door\", value = 3 }]\n",
    ),
    (
        "spawn-with-to",
        "[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"hut\"\nto = \"town\"\n",
    ),
    ("spawn-no-prefab", "[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\n"),
    (
        "spawn-unknown-prefab",
        "[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"hutt\"\n",
    ),
    (
        "spawn-unknown-prefab-no-hint",
        "[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"zeppelin\"\n",
    ),
    (
        "spawn-twice",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\naction = \"spawn\"\nprefab = \"hut\"\n\n[[connect]]\nfrom = \"pad\"\nsignal = \"on\"\nactions = [{ action = \"spawn\", prefab = \"hut\" }, { action = \"spawn\", prefab = \"coin\" }]\n",
    ),
    (
        "spawn-with-prop",
        "[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"hut\"\nprop = \"x\"\nvalue = 2\n",
    ),
    (
        "cost-unknown-entity-hint",
        "[[connect]]\nfrom = \"pad\"\nsignal = \"on\"\naction = \"spawn\"\nprefab = \"hut\"\ncost = { entity = \"twon\", prop = \"coins\", amount = 3 }\n",
    ),
    (
        "cost-unknown-entity-no-hint",
        "[[connect]]\nfrom = \"pad\"\nsignal = \"on\"\naction = \"spawn\"\nprefab = \"hut\"\ncost = { entity = \"zeppelin\", prop = \"coins\", amount = 3 }\n",
    ),
    (
        "cost-unknown-prop",
        "[[connect]]\nfrom = \"pad\"\nsignal = \"on\"\naction = \"spawn\"\nprefab = \"hut\"\ncost = { entity = \"town\", prop = \"gold\", amount = 3 }\n",
    ),
    (
        "cost-counts-prop",
        "[[connect]]\nfrom = \"pad\"\nsignal = \"on\"\naction = \"spawn\"\nprefab = \"hut\"\ncost = { entity = \"job\", prop = \"houses\", amount = 1 }\n",
    ),
    (
        "cost-string-prop",
        "[[connect]]\nfrom = \"pad\"\nsignal = \"on\"\naction = \"spawn\"\nprefab = \"hut\"\ncost = { entity = \"hint\", prop = \"msg\", amount = 1 }\n",
    ),
    (
        "cost-zero",
        "[[connect]]\nfrom = \"pad\"\nsignal = \"on\"\naction = \"spawn\"\nprefab = \"hut\"\ncost = { entity = \"town\", prop = \"coins\", amount = 0 }\n",
    ),
    (
        "cost-negative",
        "[[connect]]\nfrom = \"pad\"\nsignal = \"on\"\naction = \"spawn\"\nprefab = \"hut\"\ncost = { entity = \"town\", prop = \"coins\", amount = -2.5 }\n",
    ),
    (
        "cost-float-on-int",
        "[[connect]]\nfrom = \"pad\"\nsignal = \"on\"\naction = \"spawn\"\nprefab = \"hut\"\ncost = { entity = \"town\", prop = \"coins\", amount = 1.5 }\n",
    ),
    (
        "cost-missing-amount",
        "[[connect]]\nfrom = \"pad\"\nsignal = \"on\"\naction = \"spawn\"\nprefab = \"hut\"\ncost = { entity = \"town\", prop = \"coins\" }\n",
    ),
    (
        "cost-float-on-float",
        "[[connect]]\nfrom = \"pad\"\nsignal = \"on\"\naction = \"spawn\"\nprefab = \"hut\"\ncost = { entity = \"hud\", prop = \"rate\", amount = 0.25 }\n",
    ),
    (
        "cost-unknown-key",
        "[[connect]]\nfrom = \"pad\"\nsignal = \"on\"\naction = \"spawn\"\nprefab = \"hut\"\ncost = { who = \"town\", prop = \"coins\", amount = 1 }\n",
    ),
    (
        "cost-on-non-spawn",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"door\"\naction = \"disable\"\ncost = { entity = \"town\", prop = \"coins\", amount = 1 }\n",
    ),
    (
        "prefab-on-non-spawn",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"door\"\naction = \"disable\"\nprefab = \"hut\"\n",
    ),
    ("non-spawn-no-to", "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\naction = \"toggle\"\n"),
    ("self-with-ground", "[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"disable\"\n"),
    ("to-unknown-hint", "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"dor\"\naction = \"enable\"\n"),
    (
        "to-unknown-no-hint",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"zeppelin\"\naction = \"enable\"\n",
    ),
    (
        "to-unknown-with-prop",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"hd\"\naction = \"add\"\nprop = \"coins\"\n",
    ),
    (
        "self-entity-add",
        "[[connect]]\nfrom = \"hud\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"add\"\nprop = \"coins\"\n",
    ),
    (
        "self-entity-unknown-prop",
        "[[connect]]\nfrom = \"hud\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"add\"\nprop = \"coin\"\n",
    ),
    (
        "self-unknown-source",
        "[[connect]]\nfrom = \"zeppelin\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"set\"\nprop = \"coins\"\nvalue = 1\n",
    ),
    (
        "self-prefab-unknown-prop",
        "[[connect]]\nfrom_prefab = \"house\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"add\"\nprop = \"level\"\n",
    ),
    ("add-no-prop", "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"hud\"\naction = \"add\"\n"),
    (
        "add-counts-prop",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"job\"\naction = \"add\"\nprop = \"houses\"\n",
    ),
    (
        "set-counts-prop",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"job\"\naction = \"set\"\nprop = \"houses\"\nvalue = 1\n",
    ),
    (
        "add-string-prop",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"hud\"\naction = \"add\"\nprop = \"mode\"\n",
    ),
    (
        "add-float-on-int",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"hud\"\naction = \"add\"\nprop = \"coins\"\nvalue = 0.5\n",
    ),
    (
        "add-int-on-float",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"hud\"\naction = \"add\"\nprop = \"rate\"\nvalue = 1\n",
    ),
    (
        "add-string-value",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"hud\"\naction = \"add\"\nprop = \"rate\"\nvalue = \"1\"\n",
    ),
    (
        "set-no-value",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"hud\"\naction = \"set\"\nprop = \"coins\"\n",
    ),
    (
        "set-wrong-type",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nactions = [{ action = \"set\", to = \"hud\", prop = \"coins\", value = \"many\" }, { action = \"set\", to = \"hud\", prop = \"on\", value = \"yes\" }, { action = \"set\", to = \"hud\", prop = \"mode\", value = 2 }]\n",
    ),
    (
        "set-value-not-finite",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"hud\"\naction = \"set\"\nprop = \"rate\"\nvalue = nan\n",
    ),
    (
        "set-value-table",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"hud\"\naction = \"set\"\nprop = \"rate\"\nvalue = [1]\n",
    ),
    (
        "prop-on-enable",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"door\"\naction = \"enable\"\nprop = \"x\"\nvalue = 1\n",
    ),
    ("action-unknown", "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"door\"\naction = \"disabel\"\n"),
    ("signal-missing", "[[connect]]\nfrom = \"plate\"\nto = \"door\"\naction = \"enable\"\n"),
    (
        "connect-unknown-key",
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"door\"\naction = \"enable\"\nwhen_all = []\n",
    ),
];

/// `when` lists, each in a [[connect]] appended to the base scene.
const WHEN_SNIPS: &[(&str, &str)] = &[
    ("reason-with-others", "{ reason = \"edge\", entity = \"town\", enabled = true }"),
    ("reason-unknown", "{ reason = \"water\" }"),
    ("no-entity", "{ enabled = true }"),
    ("unknown-entity-hint", "{ entity = \"pa\", occupied = true }"),
    ("unknown-entity-no-hint", "{ entity = \"zeppelin\", occupied = true }"),
    ("kinds-none", "{ entity = \"pad\" }"),
    ("kinds-two", "{ entity = \"pad\", occupied = true, enabled = true }"),
    ("kinds-three", "{ entity = \"hud\", occupied = true, enabled = true, prop = \"coins\" }"),
    ("compare-without-prop", "{ entity = \"pad\", enabled = true, at_least = 2, equals = 1 }"),
    ("compares-nothing", "{ entity = \"hud\", prop = \"coins\" }"),
    ("unknown-prop-hint", "{ entity = \"hud\", prop = \"coin\", at_least = 1 }"),
    ("unknown-prop-no-hint", "{ entity = \"pad\", prop = \"zzz\", equals = 1 }"),
    ("at-least-on-string", "{ entity = \"hud\", prop = \"mode\", at_least = 1 }"),
    ("at-most-on-bool", "{ entity = \"hud\", prop = \"on\", at_most = 1 }"),
    ("equals-type", "{ entity = \"hud\", prop = \"coins\", equals = \"3\" }"),
    ("equals-int-on-float", "{ entity = \"hud\", prop = \"rate\", equals = 1 }"),
    ("range-empty", "{ entity = \"hud\", prop = \"coins\", at_least = 3, at_most = 2 }"),
    ("type-and-range", "{ entity = \"hud\", prop = \"mode\", at_least = 3, at_most = 2, equals = 1 }"),
    ("counts-prop-equals", "{ entity = \"job\", prop = \"houses\", equals = \"two\" }"),
    (
        "several-items",
        "{ entity = \"zeppelin\", occupied = true }, { entity = \"pad\" }, { entity = \"hud\", prop = \"coin\", at_least = 1 }",
    ),
    ("item-not-table", "1, { entity = \"pad\", occupied = true }"),
    ("unknown-key", "{ entity = \"pad\", ocupied = true }"),
    ("not-finite", "{ entity = \"hud\", prop = \"rate\", at_least = inf }"),
    ("cells-entity", "{ entity = \"wall\", occupied = true }"),
];

fn when_connect(cond: &str) -> String {
    format!(
        "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"door\"\naction = \"disable\"\nwhen = [{cond}]\n"
    )
}

/// Grounds, each a scene of its own: (name, scene).
const GROUND_SCENES: &[(&str, &str)] = &[
    ("terrain-char-empty", "[ground]\nmap = [\"g\"]\n\n[[terrain]]\nchar = \"\"\n\n[[terrain]]\nchar = \"g\"\n"),
    ("terrain-char-long", "[ground]\nmap = [\"g\"]\n\n[[terrain]]\nchar = \"ab\"\n\n[[terrain]]\nchar = \"g\"\n"),
    ("terrain-char-space", "[ground]\nmap = [\"g\"]\n\n[[terrain]]\nchar = \" \"\n\n[[terrain]]\nchar = \"g\"\n"),
    (
        "terrain-char-twice",
        "[ground]\nmap = [\"g\"]\n\n[[terrain]]\nchar = \"g\"\nname = \"grass\"\n\n[[terrain]]\nchar = \"g\"\nname = \"moss\"\n\n[[terrain]]\nchar = \"g\"\n",
    ),
    ("terrain-missing-char", "[ground]\nmap = [\"g\"]\n\n[[terrain]]\nname = \"x\"\n\n[[terrain]]\nchar = \"g\"\n"),
    ("terrain-char-not-string", "[ground]\nmap = [\"g\"]\n\n[[terrain]]\nchar = 7\n\n[[terrain]]\nchar = \"g\"\n"),
    (
        "terrain-bad-keys",
        "[ground]\nmap = [\"g\"]\n\n[[terrain]]\nchar = \"g\"\nname = \"a b\"\nbuild = \"no\"\ncolor = \"red\"\n",
    ),
    ("no-size-no-map", "[ground]\ncolor = \"#fff\"\n"),
    ("empty-ground-table", "[ground]\n"),
    ("ground-not-table", "ground = [8, 8]\n"),
    ("map-empty-row", "[ground]\nmap = [\"\"]\n"),
    ("map-empty-list", "[ground]\nmap = []\n"),
    ("map-ragged", "[ground]\nmap = [\"gg\", \"g\", \"ggg\"]\n\n[[terrain]]\nchar = \"g\"\n"),
    (
        "map-unknown-char",
        "[ground]\nmap = [\"gx\", \"gw\"]\n\n[[terrain]]\nchar = \"g\"\n\n[[terrain]]\nchar = \"w\"\n",
    ),
    ("map-unknown-none-declared", "[ground]\nmap = [\"ab\"]\n"),
    (
        "map-unknown-several-rows",
        "[ground]\nmap = [\"xg\", \"gy\", \"zz\", \"gg\"]\n\n[[terrain]]\nchar = \"g\"\nname = \"grass\"\n",
    ),
    ("map-unknown-unicode", "[ground]\nmap = [\"g\u{e9}\", \"g\u{1F600}\"]\n\n[[terrain]]\nchar = \"g\"\n"),
    ("map-ragged-and-unknown", "[ground]\nmap = [\"gx\", \"g\", \"xg\"]\n\n[[terrain]]\nchar = \"g\"\n"),
    ("map-size-mismatch", "[ground]\nmap = [\"gg\", \"gg\"]\nsize = [3, 2]\n\n[[terrain]]\nchar = \"g\"\n"),
    ("map-size-match", "[ground]\nmap = [\"gg\", \"gg\"]\nsize = [2, 2]\n\n[[terrain]]\nchar = \"g\"\n"),
    ("map-colors", "[ground]\nmap = [\"g\"]\ncolor = \"#fff\"\nalt_color = \"#eee\"\n\n[[terrain]]\nchar = \"g\"\n"),
    ("map-not-strings", "[ground]\nmap = [1, \"g\"]\n\n[[terrain]]\nchar = \"g\"\n"),
    (
        "map-unicode-terrain",
        "[ground]\nmap = [\"\u{e9}\u{e9}\", \"\u{e9}g\"]\n\n[[terrain]]\nchar = \"\u{e9}\"\n\n[[terrain]]\nchar = \"g\"\n",
    ),
    ("plain", "[ground]\nsize = [3, 2]\ncolor = \"#123\"\n"),
    ("plain-alt", "[ground]\nsize = [3, 2]\ncolor = \"#123\"\nalt_color = \"#456\"\n"),
    ("plain-size-wrong", "[ground]\nsize = [3]\n"),
    ("plain-size-float", "[ground]\nsize = [3.5, 2]\n"),
    ("terrain-without-map", "[ground]\nsize = [3, 2]\n\n[[terrain]]\nchar = \"g\"\n"),
    ("negative-size", "[ground]\nsize = [-3, 2]\n\n[[entity]]\npos = [0, 0]\ntile = {}\n"),
    ("zero-size", "[ground]\nsize = [0, 0]\n\n[[entity]]\npos = [0, 0]\ntile = {}\n"),
    ("not-tables", "entity = 3\nconnect = \"everything\"\nterrain = { char = \"g\" }\n"),
    ("no-ground", "[[entity]]\npos = [50, -50]\ntile = {}\nsolid = true\n\n[[entity]]\npos = [50, -50]\nwander = {}\n"),
];

/// Playtests, as tests/t.toml in the base project: (name, text).
const PLAYTESTS: &[(&str, &str)] = &[
    ("syntax", "[test\nframes = 1\n"),
    ("empty", ""),
    ("input-kinds-none", "[[input]]\nframe = 1\n"),
    ("input-kinds-two", "[[input]]\nframe = 1\npress = \"up\"\ntap = [1, 1]\n"),
    ("input-kinds-camera-tap", "[[input]]\nframe = 1\ncamera = { turn = 90 }\ntap = [1, 1]\n"),
    ("input-kinds-all", "[[input]]\nframe = 1\npress = \"up\"\nhold = \"up\"\ntap = [1, 1]\ncamera = {}\nfor = 2\n"),
    ("for-without-hold", "[[input]]\nframe = 1\npress = \"up\"\nfor = 3\n"),
    (
        "input-after-end",
        "[test]\nframes = 10\n\n[[input]]\nframe = 10\npress = \"up\"\n\n[[input]]\nframe = 99\nhold = \"left\"\nfor = 2\n\n[[input]]\nframe = 9\ntap = [3, 3]\n",
    ),
    ("input-after-default-end", "[[input]]\nframe = 400\npress = \"up\"\n"),
    ("input-unknown-action", "[[input]]\nframe = 1\npress = \"jump\"\n"),
    ("input-missing-frame", "[[input]]\npress = \"up\"\n"),
    ("input-camera-bad", "[[input]]\nframe = 1\ncamera = { turn = 999, zoom = 0, view = \"fish\", spin = 1 }\n"),
    ("expect-count-without-prefab", "[[expect]]\ncount = 1\n"),
    (
        "expect-event-prefab-wrong",
        "[[expect]]\nevent = \"spawned\"\nprefab = \"house\"\ncount = 1\npos = [1, 1]\nenabled = true\n",
    ),
    ("expect-prefab-extra", "[[expect]]\nprefab = \"house\"\nentity = \"player\"\nsolid = true\n"),
    (
        "expect-prefab-event-keys",
        "[[expect]]\nprefab = \"house\"\nother = \"player\"\nat_most = 1\nat_least = 0\nsignal = \"x\"\nreason = \"solid\"\n",
    ),
    ("expect-checks-nothing", "[[expect]]\nframe = 3\n\n[[expect]]\nentity = \"player\"\n"),
    ("expect-mixes-event", "[[expect]]\nevent = \"moved\"\nentity = \"player\"\npos = [1, 0]\nprops = { a = 1 }\n"),
    (
        "expect-event-keys-without-event",
        "[[expect]]\nentity = \"player\"\npos = [1, 0]\nother = \"door\"\nat_most = 3\nat_least = 1\nsignal = \"s\"\n",
    ),
    ("expect-range", "[[expect]]\nevent = \"moved\"\nat_least = 3\nat_most = 1\n"),
    ("expect-changed-not-action", "[[expect]]\nevent = \"moved\"\nchanged = true\n"),
    ("expect-reason-not-blocked", "[[expect]]\nevent = \"moved\"\nreason = \"solid\"\n"),
    (
        "expect-frame-after-end",
        "[test]\nframes = 10\n\n[[expect]]\nframe = 11\nentity = \"player\"\npos = [0, 0]\n\n[[expect]]\nframe = 10\nentity = \"player\"\npos = [0, 0]\n\n[[expect]]\nframe = 12\nevent = \"moved\"\n",
    ),
    ("scene-missing-hint", "[test]\nscene = \"scenes/mian.toml\"\n"),
    ("scene-missing-no-hint", "[test]\nscene = \"levels/nothing.toml\"\n"),
    ("expect-prefab-unknown", "[[expect]]\nprefab = \"hous\"\ncount = 1\n\n[[expect]]\nprefab = \"zeppelin\"\n"),
    (
        "expect-other-unknown",
        "[[expect]]\nevent = \"blocked\"\nother = \"dor\"\n\n[[expect]]\nevent = \"blocked\"\nother = \"zeppelin\"\n",
    ),
    (
        "expect-entity-unknown",
        "[[expect]]\nentity = \"playr\"\npos = [0, 0]\n\n[[expect]]\nentity = \"zeppelin\"\nenabled = true\n",
    ),
    ("expect-event-entity-unknown", "[[expect]]\nevent = \"moved\"\nentity = \"zeppelin\"\nother = \"playr\"\n"),
    ("expect-props-unknown", "[[expect]]\nentity = \"hud\"\nprops = { coin = 0, mood = \"x\" }\n"),
    ("expect-props-type", "[[expect]]\nentity = \"hud\"\nprops = { coins = \"0\", rate = 1, on = 1, mode = 2.5 }\n"),
    ("expect-props-float-for-int", "[[expect]]\nentity = \"hud\"\nprops = { coins = 1.0 }\n"),
    ("expect-props-counts", "[[expect]]\nentity = \"job\"\nprops = { houses = 2 }\n"),
    ("expect-text-no-label", "[[expect]]\nentity = \"door\"\ntext = \"x\"\n"),
    ("expect-text-ok", "[[expect]]\nentity = \"hud\"\ntext = \"Coins: 0 play {ok}\"\n"),
    ("expect-cells-entity", "[[expect]]\nentity = \"wall\"\nenabled = true\n"),
    ("expect-unknown-event", "[[expect]]\nevent = \"exploded\"\n"),
    ("shot-missing-reference", "[[shot]]\nframe = 1\n"),
    ("shot-ok", "[[shot]]\nreference = \"tests/refs/a.png\"\n"),
    ("test-unknown-key", "[test]\nframez = 10\n"),
    ("test-frames-zero", "[test]\nframes = 0\n"),
    ("test-frames-huge", "[test]\nframes = 2000000\n\n[[input]]\nframe = 1500000\npress = \"up\"\n"),
    (
        "expect-everything",
        "[test]\nframes = 30\n\n[[expect]]\nentity = \"playr\"\npos = [0, 0]\n\n[[expect]]\nentity = \"hud\"\nprops = { coin = 1, coins = \"x\" }\ntext = \"t\"\n\n[[expect]]\nevent = \"blocked\"\nother = \"zeppelin\"\nreason = \"push\"\n\n[[expect]]\nprefab = \"hous\"\ncount = 2\n\n[[expect]]\ncount = 1\n\n[[expect]]\nframe = 31\nevent = \"moved\"\n",
    ),
];

/// Two names equally close to `gate_c`, written `gate_b` first: a hint that
/// takes names in file order says `gate_b`, one in sorted order `gate_a`.
const TIES: &str =
    "[[entity]]\nname = \"gate_b\"\nprops = { n = 0 }\n\n[[entity]]\nname = \"gate_a\"\nprops = { n = 0 }\n";

/// Projects of several files.
fn file_cases() -> Vec<Case> {
    let mut v = Vec::new();
    let ties = |connect: &str| scene_with(&format!("{TIES}\n{connect}"));
    v.push(case(
        "ties/from",
        ties("[[connect]]\nfrom = \"gate_c\"\nsignal = \"tapped\"\nto = \"door\"\naction = \"enable\"\n"),
    ));
    v.push(case(
        "ties/to",
        ties("[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"gate_c\"\naction = \"enable\"\n"),
    ));
    v.push(case(
        "ties/cost",
        ties("[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\naction = \"spawn\"\nprefab = \"hut\"\ncost = { entity = \"gate_c\", prop = \"n\", amount = 1 }\n"),
    ));
    v.push(case(
        "ties/when",
        ties("[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"door\"\naction = \"enable\"\nwhen = [{ entity = \"gate_c\", enabled = true }]\n"),
    ));
    v.push(case(
        "ties/expect",
        with(&[
            ("scenes/main.toml", &format!("{}\n{TIES}", base_scene())),
            ("tests/t.toml", "[[expect]]\nentity = \"gate_c\"\nenabled = true\n\n[[expect]]\nevent = \"moved\"\nother = \"gate_c\"\n"),
        ]),
    ));
    v.push(case(
        "ties/props",
        scene_with(
            "[[entity]]\nname = \"meter\"\nprops = { level_b = 0, level_a = 0 }\nlabel = { text = \"{level_c}\" }\n",
        ),
    ));
    v.push(case(
        "ties/prefabs",
        scene_with_files(
            "[[entity]]\nprefab = \"tree_c\"\n\n[[entity]]\nname = \"count\"\ncounts = { n = \"tree_c\" }\n",
            &[("prefabs/tree_b.toml", "tile = {}\n"), ("prefabs/tree_a.toml", "tile = {}\n")],
        ),
    ));
    v.push(case("files/prefab-syntax", with(&[("prefabs/wall.toml", "block = {\n")])));
    v.push(case(
        "files/prefab-unknown-key-used-twice",
        with(&[("prefabs/wall.toml", "block = {}\nsolid = true\nsolidd = 1\n")]),
    ));
    v.push(case("files/prefab-unused-invalid", with(&[("prefabs/junk.toml", "zzz = 1\nname = \"x\"\n")])));
    v.push(case("files/prefab-empty", with(&[("prefabs/blank.toml", "")])));
    v.push(case(
        "files/prefab-subfolder",
        scene_with_files(
            "[[entity]]\nprefab = \"props/tree\"\npos = [0, 2]\n\n[[entity]]\nprefab = \"props/tre\"\npos = [1, 2]\n",
            &[("prefabs/props/tree.toml", "sprite = { color = \"#2a2\" }\nsolid = true\n")],
        ),
    ));
    v.push(case(
        "files/prefab-invalid-from-prefab",
        scene_with_files(
            "[[connect]]\nfrom_prefab = \"junk\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"add\"\nprop = \"x\"\n\n[[entity]]\nprefab = \"junk\"\npos = [0, 2]\n\n[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"junk\"\n",
            &[("prefabs/junk.toml", "zzz = 1\n")],
        ),
    ));
    v.push(case(
        "files/spawn-screen-prefab",
        scene_with_files(
            "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\naction = \"spawn\"\nprefab = \"badge\"\n",
            &[("prefabs/badge.toml", "screen_pos = [0, 0]\nlabel = { text = \"x\" }\n")],
        ),
    ));
    v.push(case(
        "files/spawn-screen-prefab-cost",
        scene_with_files(
            "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\naction = \"spawn\"\nprefab = \"badge\"\ncost = { entity = \"town\", prop = \"coins\", amount = 0 }\n",
            &[("prefabs/badge.toml", "screen_pos = [0, 0]\nlabel = { text = \"x\" }\n")],
        ),
    ));
    v.push(case(
        "files/spawn-prefab-checks",
        scene_with_files(
            "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nactions = [{ action = \"spawn\", prefab = \"rover\" }, { action = \"spawn\", prefab = \"rover\" }]\n\n[[connect]]\nfrom_prefab = \"rover\"\nsignal = \"spawned\"\nto = \"self\"\naction = \"add\"\nprop = \"fuel\"\n",
            &[(
                "prefabs/rover.toml",
                "footprint = [2, 2]\nmover = {}\nlabel = { text = \"{fuel} Caf\u{e9}\" }\ncounts = { n = \"zeppelin\" }\nprops = { n = 1 }\n",
            )],
        ),
    ));
    v.push(case(
        "files/self-prefab-label",
        scene_with_files(
            "[[connect]]\nfrom_prefab = \"meter\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"set\"\nprop = \"level\"\nvalue = \"high\"\n\n[[connect]]\nfrom_prefab = \"meter\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"add\"\nprop = \"tag\"\n\n[[connect]]\nfrom_prefab = \"meter\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"add\"\nprop = \"count\"\n",
            &[("prefabs/meter.toml", "props = { level = 1, tag = \"a\" }\ncounts = { count = \"hut\" }\ntile = {}\n")],
        ),
    ));
    v.push(case(
        "files/main-scene-elsewhere",
        with(&[
            ("rwgpu.toml", "[project]\nname = \"t\"\nmain_scene = \"levels/one.toml\"\n"),
            (
                "levels/one.toml",
                "[[entity]]\nprefab = \"wal\"\n\n[[entity]]\nname = \"a\"\n\n[[entity]]\nname = \"a\"\n",
            ),
        ]),
    ));
    v.push(case(
        "files/two-scenes-one-bad-prefab",
        with(&[
            ("prefabs/wall.toml", "block = {}\nsolid = \"yes\"\n"),
            (
                "scenes/b.toml",
                "[[entity]]\nprefab = \"wall\"\npos = [1, 1]\n\n[[entity]]\nprefab = \"wall\"\npos = [2, 1]\n",
            ),
        ]),
    ));
    v.push(case("files/scene-empty", with(&[("scenes/main.toml", "")])));
    v.push(case(
        "files/not-toml-ignored",
        with(&[
            ("scenes/readme.md", "# notes"),
            ("prefabs/wall.txt", "x"),
            ("tests/notes.txt", "y"),
            ("other/x.toml", "zzz"),
        ]),
    ));
    v.push(case("files/project-only", bare(&[("rwgpu.toml", PROJECT)])));
    v.push(case("files/no-project", {
        let mut vfs = with(&[]);
        let mut out = Vfs::new();
        for p in vfs.paths().map(str::to_string).collect::<Vec<_>>() {
            if p != "rwgpu.toml" {
                out.insert(p.clone(), vfs.get(&p).unwrap_or_default().to_string());
            }
        }
        vfs = out;
        vfs
    }));
    v.push(case("files/empty", Vfs::new()));
    v.push(case(
        "files/no-project-broken-files",
        bare(&[("scenes/main.toml", "[[entity]]\nprefab = \"nope\"\n"), ("tests/t.toml", "[[input]]\nframe = 1\n")]),
    ));
    v.push(case(
        "files/test-scene-elsewhere",
        with(&[
            (
                "tests/t.toml",
                "[test]\nscene = \"levels/alt.toml\"\nframes = 5\n\n[[expect]]\nentity = \"knight\"\npos = [0, 0]\n\n[[expect]]\nentity = \"kngiht\"\nenabled = true\n",
            ),
            ("levels/alt.toml", "[ground]\nsize = [2, 2]\n\n[[entity]]\nname = \"knight\"\nsprite = {}\n"),
        ]),
    ));
    v.push(case(
        "files/test-scene-elsewhere-fails",
        with(&[
            (
                "tests/t.toml",
                "[test]\nscene = \"levels/alt.toml\"\n\n[[expect]]\nentity = \"zeppelin\"\npos = [0, 0]\n",
            ),
            ("tests/u.toml", "[test]\nscene = \"levels/alt.toml\"\n"),
            ("levels/alt.toml", "[[entity]]\nprefab = \"nope\"\n"),
        ]),
    ));
    v.push(case(
        "files/project-broken-playtests",
        with(&[
            ("rwgpu.toml", "[project]\nname = 3\n"),
            ("tests/t.toml", "[[expect]]\nentity = \"zeppelin\"\npos = [0, 0]\ncount = 3\n"),
        ]),
    ));
    v.push(case(
        "files/playtest-subfolder",
        with(&[("tests/more/deep.toml", "[[expect]]\nentity = \"zeppelin\"\npos = [0, 0]\n")]),
    ));
    v.push(Case {
        name: "files/playtest-not-found".into(),
        vfs: with(&[]),
        extra_playtests: vec!["tests/nothing.toml".into(), "levels/test.toml".into()],
    });
    v.push(case("files/scene-syntax", with(&[("scenes/main.toml", "[[entity]\nname = \"x\"\n")])));
    v.push(case(
        "files/scene-duplicate-key",
        with(&[("scenes/main.toml", "[[entity]]\nname = \"x\"\nname = \"y\"\n")]),
    ));
    v.push(case(
        "files/scene-unknown-top",
        with(&[("scenes/main.toml", "[[entiti]]\npos = [0, 0]\n\n[scen]\nname = \"x\"\n")]),
    ));
    v.push(case("files/scene-meta-bad", with(&[("scenes/main.toml", "[scene]\nname = 1\nseed = -4\ndoc = []\n")])));
    v.push(case(
        "files/camera-bad",
        with(&[(
            "scenes/main.toml",
            "[camera]\nview = \"fisheye\"\nzoom = 100\nturn = 720\ncenter = [1]\noffset = \"x\"\n",
        )]),
    ));
    v.push(case("files/terrains-255", bare_scene(&many_terrains(255))));
    v.push(case("files/terrains-256", bare_scene(&many_terrains(256))));
    v
}

fn scene_with_files(extra: &str, files: &[(&str, &str)]) -> Vfs {
    let mut vfs = scene_with(extra);
    for (path, text) in files {
        vfs.insert(*path, *text);
    }
    vfs
}

/// A map that uses `n` terrains, one row of each character.
fn many_terrains(n: u32) -> String {
    let chars: Vec<char> = (0..n).filter_map(|i| char::from_u32(0x4e00 + i)).collect();
    let row: String = chars.iter().collect();
    let mut s = format!("[ground]\nmap = [\"{row}\"]\n");
    for c in &chars {
        let _ = write!(s, "\n[[terrain]]\nchar = \"{c}\"\n");
    }
    s
}

// ---------------------------------------------------------------------------
// The inputs of the loader tests (tests.rs), one case each

fn test_inputs() -> Vec<Case> {
    let mut v = Vec::new();
    let p = t::project;
    v.push(case(
        "tests/a-valid-project",
        p(&[
            ("prefabs/wall.toml", t::WALL),
            (
                "scenes/main.toml",
                "\n[ground]\nsize = [4, 4]\n\n[[entity]]\nname = \"player\"\npos = [0, 0]\nsprite = { color = \"#fc0\" }\nmover = {}\n\n[[entity]]\nprefab = \"wall\"\ncells = [[2, 0], [2, 1]]\n\n[[entity]]\nname = \"plate\"\npos = [1, 1]\ntile = {}\ntrigger = {}\n\n[[entity]]\nname = \"gate\"\nprefab = \"wall\"\npos = [2, 2]\nblock = { color = \"#a33\" }\n\n[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"gate\"\naction = \"disable\"\n",
            ),
            ("tests/smoke.toml", "\n[test]\nframes = 60\n\n[[input]]\nframe = 1\npress = \"right\"\n\n[[expect]]\nentity = \"player\"\npos = [1, 0]\n"),
        ]),
    ));
    v.push(case(
        "tests/missing-prefab",
        p(&[("prefabs/wall.toml", t::WALL), ("scenes/main.toml", "[[entity]]\nprefab = \"wal\"\npos = [0, 0]\n")]),
    ));
    v.push(case(
        "tests/reference-errors",
        p(&[(
            "scenes/main.toml",
            "\n[[entity]]\nname = \"a\"\npos = [0, 0]\n\n[[entity]]\nname = \"a\"\npos = [1, 0]\n\n[[entity]]\nname = \"b\"\ncells = [[0, 1]]\n\n[[connect]]\nfrom = \"a\"\nsignal = \"pressed\"\nto = \"gaet\"\naction = \"toggle\"\n",
        )]),
    ));
    v.push(case(
        "tests/static-placement",
        p(&[
            ("prefabs/wall.toml", t::WALL),
            (
                "scenes/main.toml",
                "[ground]\nsize = [2, 2]\n[[entity]]\nprefab = \"wall\"\ncells = [[0, 0], [0, 0], [5, 5]]\n",
            ),
        ]),
    ));
    v.push(case(
        "tests/playtest-checks",
        p(&[
            ("scenes/main.toml", "[[entity]]\nname = \"p\"\n"),
            (
                "tests/t.toml",
                "\n[test]\nframes = 10\n\n[[input]]\nframe = 20\npress = \"up\"\n\n[[input]]\nframe = 1\npress = \"up\"\ntap = [1, 1]\n\n[[expect]]\nentity = \"q\"\nenabled = true\n",
            ),
        ]),
    ));
    v.push(case(
        "tests/main-scene-and-engine",
        bare(&[
            ("rwgpu.toml", "[project]\nname = \"t\"\nengine = \"9.9\"\nmain_scene = \"scenes/mian.toml\"\n"),
            ("scenes/main.toml", ""),
        ]),
    ));
    v.push(case(
        "tests/reason-needs-blocked",
        p(&[
            ("scenes/main.toml", "[[entity]]\nname = \"p\"\n"),
            ("tests/t.toml", "[[expect]]\nevent = \"moved\"\nreason = \"solid\"\n"),
        ]),
    ));
    v.push(case(
        "tests/event-only-keys",
        p(&[
            ("scenes/main.toml", "[[entity]]\nname = \"p\"\n"),
            (
                "tests/t.toml",
                "[[expect]]\nentity = \"p\"\nenabled = true\nother = \"p\"\n\n[[expect]]\nevent = \"moved\"\nat_least = 3\nat_most = 1\n\n[[expect]]\nevent = \"blocked\"\nother = \"ghost\"\n",
            ),
        ]),
    ));
    for (i, allow) in ["", "allow = [\"W0201\"]", "allow = [\"E0101\"]", "allow = [\"W0210\"]"].iter().enumerate() {
        let scene = format!(
            "[[entity]]\nname = \"a\"\nprefab = \"wall\"\n\n[[entity]]\nname = \"b\"\nprefab = \"wall\"\n{allow}\n"
        );
        v.push(case(format!("tests/allow-{i}"), p(&[("prefabs/wall.toml", t::WALL), ("scenes/main.toml", &scene)])));
    }
    for (i, allow) in ["", "allow = [\"W0206\"]"].iter().enumerate() {
        let scene = format!(
            "[[entity]]\nprefab = \"wall\"\npos = [1, 1]\n\n[[entity]]\nname = \"bit\"\npos = [1, 1]\npatrol = {{ path = [[1, 1]] }}\n{allow}\n"
        );
        v.push(case(
            format!("tests/actor-in-solid-{i}"),
            p(&[("prefabs/wall.toml", t::WALL), ("scenes/main.toml", &scene)]),
        ));
    }
    let logic = |extra: &str| {
        let scene = format!("{}\n{extra}", t::LOGIC);
        p(&[("scenes/main.toml", &scene)])
    };
    v.push(case("tests/logic-valid", p(&[("scenes/main.toml", t::LOGIC)])));
    v.push(case(
        "tests/logic-unknown-prop",
        logic(
            "[[connect]]\nfrom = \"coin\"\nsignal = \"collected\"\nto = \"hud\"\naction = \"add\"\nprop = \"coin\"\n",
        ),
    ));
    let bad = [
        "action = \"add\"\nprop = \"mode\"",
        "action = \"set\"\nprop = \"coins\"\nvalue = \"many\"",
        "action = \"add\"\nprop = \"coins\"\nvalue = 0.5",
        "action = \"set\"\nprop = \"coins\"",
        "action = \"add\"",
        "action = \"disable\"\nprop = \"coins\"",
    ];
    for (i, body) in bad.iter().enumerate() {
        v.push(case(
            format!("tests/logic-props-{i}"),
            logic(&format!("[[connect]]\nfrom = \"coin\"\nsignal = \"collected\"\nto = \"hud\"\n{body}\n")),
        ));
    }
    let conds = [
        "{ entity = \"plate_c\", occupied = true }",
        "{ entity = \"plate_b\" }",
        "{ entity = \"plate_b\", occupied = true, enabled = true }",
        "{ entity = \"hud\", prop = \"coins\" }",
        "{ entity = \"hud\", prop = \"coin\", at_least = 1 }",
        "{ entity = \"hud\", prop = \"mode\", at_least = 1 }",
        "{ entity = \"hud\", prop = \"coins\", equals = \"3\" }",
        "{ entity = \"hud\", prop = \"coins\", at_least = 3, at_most = 2 }",
        "{ entity = \"plate_b\", enabled = true, at_least = 2 }",
        "{ entity = \"plate_b\", ocupied = true }",
    ];
    for (i, cond) in conds.iter().enumerate() {
        v.push(case(
            format!("tests/logic-when-{i}"),
            logic(&format!(
                "[[connect]]\nfrom = \"plate_b\"\nsignal = \"pressed\"\nto = \"door\"\naction = \"disable\"\nwhen = [{cond}]\n"
            )),
        ));
    }
    v.push(case(
        "tests/logic-changed",
        logic("[[connect]]\nfrom = \"door\"\nsignal = \"changed\"\nto = \"door\"\naction = \"enable\"\n"),
    ));
    let labels = [
        "label = { text = \"Score {score}\" }\nprops = { points = 0 }",
        "label = { text = \"Score {points\" }\nprops = { points = 0 }",
        "label = { text = \"a } b\" }",
        "label = { text = \"Caf\u{e9}\" }",
        "screen_pos = [4, 4]\nblock = {}",
        "screen_pos = [4, 4]\ntrigger = {}",
        "label = { text = \"x\", scale = 9 }",
    ];
    for (i, body) in labels.iter().enumerate() {
        v.push(case(
            format!("tests/logic-label-{i}"),
            logic(&format!("[[entity]]\nname = \"extra\"\npos = [4, 3]\n{body}\n")),
        ));
    }
    v.push(case("tests/logic-label-escaped", logic("[[entity]]\nname = \"e\"\nlabel = { text = \"{{x}}\" }\n")));
    v.push(case(
        "tests/logic-label-allowed",
        logic("[[entity]]\nname = \"e\"\nlabel = { text = \"Caf\u{e9}\" }\nallow = [\"W0207\"]\n"),
    ));
    let expects = [
        "entity = \"hud\"\nprops = { coins = 0 }\ntext = \"Coins: 0/3 play\"",
        "entity = \"hud\"\nprops = { coin = 0 }",
        "entity = \"hud\"\nprops = { coins = \"0\" }",
        "entity = \"door\"\ntext = \"x\"",
    ];
    for (i, body) in expects.iter().enumerate() {
        let test = format!("[test]\nframes = 10\n[[expect]]\n{body}\n");
        v.push(case(format!("tests/logic-expect-{i}"), p(&[("scenes/main.toml", t::LOGIC), ("tests/t.toml", &test)])));
    }
    let crate_scene = |extra: &str| {
        format!(
            "[ground]\nsize = [4, 4]\n\n[[entity]]\nname = \"c\"\npos = [1, 1]\nblock = {{}}\npushable = true\n{extra}\n"
        )
    };
    for (i, extra) in [
        "",
        "mover = {}",
        "\n[[entity]]\nname = \"w\"\npos = [1, 1]\nblock = {}\nsolid = true\n",
        "screen_pos = [1, 1]",
    ]
    .iter()
    .enumerate()
    {
        v.push(case(format!("tests/pushable-{i}"), p(&[("scenes/main.toml", &crate_scene(extra))])));
    }
    let b = t::builder_project;
    v.push(case(
        "tests/builder-valid",
        b("\n[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"house\"\nwhen = [{ entity = \"town\", prop = \"coins\", at_least = 3 }]\n\n[[connect]]\nfrom_prefab = \"house\"\nsignal = \"spawned\"\nto = \"town\"\naction = \"add\"\nprop = \"coins\"\nvalue = -3\n\n[[connect]]\nfrom_prefab = \"house\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"disable\"\n"),
    ));
    let terrain = |ground: &str| {
        let text = format!("{ground}\n[[terrain]]\nchar = \"g\"\n\n[[terrain]]\nchar = \"w\"\nbuild = false\n");
        p(&[("scenes/main.toml", &text)])
    };
    for (i, ground) in [
        "[ground]\nmap = [\"gg\", \"gw\"]",
        "[ground]\nmap = [\"gx\", \"gw\"]",
        "[ground]\nmap = [\"gg\", \"g\"]",
        "[ground]\nmap = [\"gg\", \"gw\"]\nsize = [3, 2]",
        "[ground]\nmap = [\"gg\", \"gw\"]\ncolor = \"#fff\"",
        "[ground]\ncolor = \"#fff\"",
    ]
    .iter()
    .enumerate()
    {
        v.push(case(format!("tests/terrain-{i}"), terrain(ground)));
    }
    v.push(case(
        "tests/terrain-chars",
        p(&[(
            "scenes/main.toml",
            "[ground]\nmap = [\"g\"]\n[[terrain]]\nchar = \"g\"\n[[terrain]]\nchar = \"g\"\n[[terrain]]\nchar = \"ab\"\n",
        )]),
    ));
    let spawn =
        |keys: &str| b(&format!("[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\n{keys}"));
    for (i, keys) in ["", "prefab = \"house\"\nto = \"town\"", "prefab = \"hosue\""].iter().enumerate() {
        v.push(case(format!("tests/spawn-{i}"), spawn(keys)));
    }
    let builder_snips = [
        "[[connect]]\nfrom = \"ground\"\nsignal = \"pressed\"\naction = \"spawn\"\nprefab = \"house\"",
        "[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"disable\"",
        "[[connect]]\nsignal = \"tapped\"\nto = \"town\"\naction = \"disable\"",
        "[[connect]]\nfrom = \"town\"\nfrom_prefab = \"house\"\nsignal = \"tapped\"\nto = \"town\"\naction = \"disable\"",
        "[[connect]]\nfrom_prefab = \"house\"\nsignal = \"spawned\"\nto = \"town\"\naction = \"disable\"",
        "[[connect]]\nfrom_prefab = \"house\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"add\"\nprop = \"level\"",
        "[[connect]]\nfrom = \"town\"\nsignal = \"tapped\"\naction = \"disable\"",
        "[[connect]]\nfrom = \"town\"\nsignal = \"tapped\"\nto = \"town\"\naction = \"disable\"\nprefab = \"house\"",
        "[[entity]]\npos = [0, 0]\nfootprint = [2, 1]\nmover = {}\nsprite = {}",
        "[[entity]]\nname = \"ground\"\npos = [0, 0]\ntile = {}",
        "[[entity]]\nname = \"self\"\npos = [0, 0]\ntile = {}",
        "[[entity]]\nprefab = \"house\"\npos = [0, 0]\n\n[[entity]]\npos = [1, 1]\nblock = {}\nsolid = true",
        "[[entity]]\nprefab = \"house\"\npos = [2, 2]",
    ];
    for (i, extra) in builder_snips.iter().enumerate() {
        v.push(case(format!("tests/builder-{i}"), b(extra)));
    }
    let house_test = |expect: &str| {
        let test = format!("[test]\nscene = \"scenes/main.toml\"\nframes = 10\n\n[[expect]]\n{expect}\n");
        p(&[
            ("prefabs/house.toml", t::HOUSE),
            ("scenes/main.toml", "[ground]\nsize = [4, 4]\n"),
            ("tests/t.toml", &test),
        ])
    };
    for (i, expect) in [
        "prefab = \"house\"\ncount = 0",
        "prefab = \"house\"\npos = [1, 1]",
        "prefab = \"hous\"\ncount = 1",
        "count = 1",
        "prefab = \"house\"\nenabled = true",
        "event = \"skipped\"\nprefab = \"house\"\nat_least = 1",
        "event = \"blocked\"\nreason = \"occupied\"\nprefab = \"house\"",
        "event = \"spawned\"\nprefab = \"house\"\ncount = 1",
        "event = \"spawned\"\nprefab = \"house\"\npos = [1, 1]",
    ]
    .iter()
    .enumerate()
    {
        v.push(case(format!("tests/prefab-expect-{i}"), house_test(expect)));
    }
    let refused = |when: &str| t::hint_on_refused(when);
    v.push(case("tests/refused-0", b(&refused(""))));
    v.push(case("tests/refused-1", b(&refused("when = [{ reason = \"terrain\" }]"))));
    v.push(case("tests/refused-2", b(&refused("").replace(t::SPAWN_HOUSE, ""))));
    v.push(case("tests/refused-3", b(&refused("when = [{ reason = \"water\" }]"))));
    v.push(case("tests/refused-4", b(&refused("when = [{ reason = \"edge\", entity = \"town\", enabled = true }]"))));
    v.push(case(
        "tests/refused-5",
        b(&refused("when = [{ reason = \"edge\" }]").replace("signal = \"refused\"", "signal = \"tapped\"")),
    ));
    v.push(case("tests/refused-6", b(&refused("when = [{ enabled = true }]"))));
    for (i, spawns) in [true, false].iter().enumerate() {
        let spawn = if *spawns {
            "[[connect]]\nfrom = \"town\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"house\"\n"
        } else {
            ""
        };
        v.push(case(
            format!("tests/refused-town-{i}"),
            b(&format!(
                "{spawn}\n[[connect]]\nfrom = \"town\"\nsignal = \"refused\"\nto = \"town\"\naction = \"set\"\nprop = \"coins\"\nvalue = 0"
            )),
        ));
    }
    let job = |counts: &str, extra: &str| b(&format!("[[entity]]\nname = \"job\"\ncounts = {counts}\n{extra}"));
    let reads = "label = { text = \"Houses {houses}\" }\nscreen_pos = [0, 0]\n\n[[connect]]\nfrom = \"job\"\nsignal = \"changed\"\nto = \"town\"\naction = \"set\"\nprop = \"coins\"\nvalue = 9\nwhen = [{ entity = \"job\", prop = \"houses\", at_least = 2 }]";
    v.push(case("tests/counts-0", job("{ houses = \"house\" }", reads)));
    v.push(case("tests/counts-1", job("{ houses = \"hous\" }", "")));
    v.push(case("tests/counts-2", job("{ houses = 3 }", "")));
    v.push(case("tests/counts-3", job("{ houses = \"house\" }", "props = { houses = 0 }")));
    v.push(case(
        "tests/counts-4",
        job(
            "{ houses = \"house\" }",
            "\n[[connect]]\nfrom = \"town\"\nsignal = \"tapped\"\nto = \"job\"\naction = \"add\"\nprop = \"houses\"",
        ),
    ));
    v.push(case(
        "tests/counts-5",
        job(
            "{ houses = \"house\" }",
            "\n[[connect]]\nfrom = \"job\"\nsignal = \"changed\"\nto = \"town\"\naction = \"disable\"\nwhen = [{ entity = \"job\", prop = \"houses\", equals = \"two\" }]",
        ),
    ));
    v.push(case(
        "tests/counts-6",
        b("[[entity]]\nname = \"job\"\nprops = { n = 0 }\n\n[[connect]]\nfrom = \"job\"\nsignal = \"changed\"\nto = \"town\"\naction = \"disable\""),
    ));
    let grouped = |connect: &str| {
        b(&format!(
            "[[entity]]\nname = \"hint\"\nprops = {{ msg = \"\" }}\n\n[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\n{connect}"
        ))
    };
    let ok = "actions = [\n  { action = \"spawn\", prefab = \"house\" },\n  { action = \"add\", to = \"town\", prop = \"coins\", value = -3 },\n  { action = \"set\", to = \"hint\", prop = \"msg\", value = \"\" },\n]";
    let groups = [
        ok.to_string(),
        format!("action = \"spawn\"\nprefab = \"house\"\n{ok}"),
        "when = []".to_string(),
        "to = \"town\"\nactions = [{ action = \"disable\", to = \"town\" }]".to_string(),
        "actions = []".to_string(),
        "actions = [{ to = \"town\" }]".to_string(),
        "actions = [{ action = \"disable\", to = \"twon\" }]".to_string(),
        "actions = [{ action = \"add\", to = \"town\", prop = \"gold\" }]".to_string(),
        "actions = [{ action = \"spawn\" }]".to_string(),
        "actions = [{ action = \"spawn\", prefab = \"hous\" }]".to_string(),
        "actions = [{ action = \"set\", to = \"hint\", prop = \"msg\", value = \"x\" }]\n\n[[connect]]\nfrom = \"hint\"\nsignal = \"changed\"\nto = \"town\"\naction = \"disable\"".to_string(),
    ];
    for (i, g) in groups.iter().enumerate() {
        v.push(case(format!("tests/actions-{i}"), grouped(g)));
    }
    for (i, expect) in
        ["event = \"action\"\nchanged = true\nat_least = 0", "event = \"skipped\"\nchanged = true\nat_least = 0"]
            .iter()
            .enumerate()
    {
        let test = format!("[test]\nscene = \"scenes/main.toml\"\nframes = 10\n\n[[expect]]\n{expect}\n");
        v.push(case(
            format!("tests/changed-{i}"),
            p(&[("scenes/main.toml", "[ground]\nsize = [4, 4]\n"), ("tests/t.toml", &test)]),
        ));
    }
    let costed = |cost: &str, extra: &str| {
        b(&format!(
            "[[entity]]\nname = \"sign\"\nprops = {{ msg = \"\" }}\ncounts = {{ houses = \"house\" }}\n\n[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"house\"\ncost = {cost}\n{extra}"
        ))
    };
    let ok_cost = "{ entity = \"town\", prop = \"coins\", amount = 3 }";
    for (i, cost) in [
        ok_cost,
        "{ entity = \"twon\", prop = \"coins\", amount = 3 }",
        "{ entity = \"town\", prop = \"gold\", amount = 3 }",
        "{ entity = \"sign\", prop = \"msg\", amount = 3 }",
        "{ entity = \"sign\", prop = \"houses\", amount = 1 }",
        "{ entity = \"town\", prop = \"coins\", amount = 0 }",
        "{ entity = \"town\", prop = \"coins\", amount = -2 }",
        "{ entity = \"town\", prop = \"coins\", amount = 1.5 }",
        "{ entity = \"town\", prop = \"coins\" }",
    ]
    .iter()
    .enumerate()
    {
        v.push(case(format!("tests/cost-{i}"), costed(cost, "")));
    }
    v.push(case(
        "tests/cost-non-spawn",
        b(&format!(
            "[[connect]]\nfrom = \"town\"\nsignal = \"tapped\"\nto = \"town\"\naction = \"disable\"\ncost = {ok_cost}"
        )),
    ));
    v.push(case(
        "tests/cost-in-list",
        b(&format!(
            "[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\ncost = {ok_cost}\nactions = [{{ action = \"spawn\", prefab = \"house\" }}]"
        )),
    ));
    v.push(case(
        "tests/cost-funds",
        costed(
            ok_cost,
            "\n[[connect]]\nfrom = \"ground\"\nsignal = \"refused\"\nto = \"sign\"\naction = \"set\"\nprop = \"msg\"\nvalue = \"too dear\"\nwhen = [{ reason = \"funds\" }]\n\n[[connect]]\nfrom = \"town\"\nsignal = \"changed\"\nto = \"sign\"\naction = \"set\"\nprop = \"msg\"\nvalue = \"paid\"",
        ),
    ));
    v
}

// ---------------------------------------------------------------------------
// Generated worlds: the shapes the benchmarks load, at a thousand entities

/// The smallest even side whose every other cell holds `n` things.
fn side_for(n: usize) -> usize {
    let mut side = 2;
    while (side / 2) * (side / 2) < n {
        side += 2;
    }
    side
}

/// The 3D study's town: `n` wanderers on every other cell.
fn town(n: usize) -> String {
    let side = side_for(n);
    let mut s = format!("[ground]\nsize = [{side}, {side}]\n");
    for i in 0..n {
        let (x, y) = (2 * (i % (side / 2)), 2 * (i / (side / 2)));
        let _ = write!(s, "\n[[entity]]\npos = [{x}, {y}]\nwander = {{ step_frames = 1 }}\n");
    }
    s
}

/// Named plates, each with a wanderer beside it and a [[connect]] of its own.
fn wired(n: usize) -> String {
    let pairs = n / 2;
    let side = side_for(pairs);
    let mut s =
        format!("[ground]\nsize = [{side}, {side}]\n\n[[entity]]\nname = \"hud\"\nprops = {{ on = 0, off = 0 }}\n");
    for i in 0..pairs {
        let (x, y) = (2 * (i % (side / 2)), 2 * (i / (side / 2)));
        let _ = write!(
            s,
            "\n[[entity]]\nname = \"p_{i}\"\nprefab = \"plate\"\npos = [{x}, {y}]\n\n[[entity]]\npos = [{}, {y}]\nwander = {{ step_frames = 1 }}\n",
            x + 1
        );
    }
    s.push_str("\n[[connect]]\nfrom_prefab = \"plate\"\nsignal = \"released\"\nto = \"hud\"\naction = \"add\"\nprop = \"off\"\n");
    for i in 0..pairs {
        let _ = write!(
            s,
            "\n[[connect]]\nfrom = \"p_{i}\"\nsignal = \"pressed\"\nto = \"hud\"\naction = \"add\"\nprop = \"on\"\n"
        );
    }
    s
}

/// Named entities that refer to each other: every [[connect]] names a
/// source, a target, a payer and a condition by name.
fn linked(n: usize) -> String {
    let side = side_for(n);
    let mut s = format!("[ground]\nsize = [{side}, {side}]\n");
    for i in 0..n {
        let (x, y) = (2 * (i % (side / 2)), 2 * (i / (side / 2)));
        let _ = write!(
            s,
            "\n[[entity]]\nname = \"a_{i}\"\npos = [{x}, {y}]\ntile = {{}}\nprops = {{ n = 0, coins = 5 }}\nlabel = {{ text = \"{{n}}\" }}\n"
        );
    }
    for i in 0..n {
        let (to, cond) = ((i + 1) % n, (i + 2) % n);
        let _ = write!(
            s,
            "\n[[connect]]\nfrom = \"a_{i}\"\nsignal = \"tapped\"\nactions = [{{ action = \"add\", to = \"a_{to}\", prop = \"n\" }}, {{ action = \"spawn\", prefab = \"hut\", cost = {{ entity = \"a_{i}\", prop = \"coins\", amount = 1 }} }}]\nwhen = [{{ entity = \"a_{cond}\", prop = \"n\", at_least = 0 }}]\n"
        );
    }
    s
}

/// A playtest that names `n` entities of [`linked`].
fn linked_test(n: usize) -> String {
    let mut s = "[test]\nframes = 10\n".to_string();
    for i in 0..n {
        let _ = write!(s, "\n[[expect]]\nentity = \"a_{i}\"\nprops = {{ n = 0 }}\n");
    }
    s
}

/// `n` trees, a job that counts them and spawns on the ground with a cost.
fn build(n: usize) -> String {
    let side = side_for(n);
    let mut s = format!(
        "[ground]\nsize = [{side}, {side}]\n\n[[entity]]\nname = \"bank\"\nprops = {{ coins = 1000000000, tries = 0 }}\n\n[[entity]]\nname = \"job\"\ncounts = {{ huts = \"hut\", trees = \"tree\" }}\n"
    );
    for i in 0..n {
        let (x, y) = (2 * (i % (side / 2)), 2 * (i / (side / 2)));
        let _ = write!(s, "\n[[entity]]\nprefab = \"tree\"\npos = [{x}, {y}]\n");
    }
    s.push_str("\n[[connect]]\nfrom = \"ground\"\nsignal = \"tapped\"\naction = \"spawn\"\nprefab = \"hut\"\ncost = { entity = \"bank\", prop = \"coins\", amount = 1 }\n\n[[connect]]\nfrom = \"ground\"\nsignal = \"refused\"\nto = \"bank\"\naction = \"add\"\nprop = \"tries\"\n");
    s
}

/// Every actor starts in a wall, and every [[connect]] names an entity that
/// does not exist: the slow paths of errors and warnings.
fn crowded(n: usize) -> String {
    let side = side_for(n);
    let mut s = format!("[ground]\nsize = [{side}, {side}]\n");
    for i in 0..n {
        let (x, y) = (2 * (i % (side / 2)), 2 * (i / (side / 2)));
        let _ = write!(
            s,
            "\n[[entity]]\nname = \"w_{i}\"\nprefab = \"wall\"\npos = [{x}, {y}]\n\n[[entity]]\npos = [{x}, {y}]\nwander = {{}}\nsolid = true\n"
        );
    }
    for i in 0..n {
        let _ =
            write!(s, "\n[[connect]]\nfrom = \"w_{i}\"\nsignal = \"tapped\"\nto = \"x_{i}\"\naction = \"disable\"\n");
    }
    s
}

fn generated() -> Vec<Case> {
    let n = 1000;
    vec![
        case("generated/town-1000", bare_scene(&town(n))),
        case(
            "generated/wired-1000",
            bare(&[
                ("rwgpu.toml", PROJECT),
                ("scenes/main.toml", &wired(n)),
                ("prefabs/plate.toml", "tile = {}\ntrigger = {}\n"),
            ]),
        ),
        case(
            "generated/linked-1000",
            bare(&[
                ("rwgpu.toml", PROJECT),
                ("scenes/main.toml", &linked(n)),
                ("prefabs/hut.toml", HUT),
                ("tests/t.toml", &linked_test(n)),
            ]),
        ),
        case(
            "generated/build-1000",
            bare(&[
                ("rwgpu.toml", PROJECT),
                ("scenes/main.toml", &build(n)),
                ("prefabs/tree.toml", "sprite = {}\nsolid = true\n"),
                ("prefabs/hut.toml", "block = {}\nsolid = true\n"),
            ]),
        ),
        case(
            "generated/crowded-300",
            bare(&[("rwgpu.toml", PROJECT), ("scenes/main.toml", &crowded(300)), ("prefabs/wall.toml", WALL)]),
        ),
    ]
}

// ---------------------------------------------------------------------------
// The corpus

fn examples() -> Vec<Case> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    ["coin_vault", "iso_garden", "tiny_town"]
        .iter()
        .map(|name| {
            let vfs = Vfs::from_dir(&root.join(name)).unwrap_or_else(|e| panic!("examples/{name}: {e}"));
            case(format!("examples/{name}"), vfs)
        })
        .collect()
}

/// Every case, in a fixed order.
pub(crate) fn cases() -> Vec<Case> {
    let mut v = examples();
    v.push(case("base", with(&[])));
    for (name, text) in PROJECT_FILES {
        v.push(case(format!("project/{name}"), with(&[("rwgpu.toml", text)])));
    }
    let groups: [(&str, &[(&str, &str)]); 3] =
        [("entity", ENTITY_SNIPS), ("placement", PLACEMENT_SNIPS), ("connect", CONNECT_SNIPS)];
    for (group, snips) in groups {
        for (name, text) in snips {
            v.push(case(format!("{group}/{name}"), scene_with(text)));
        }
        // Several at a time: every snippet of the group in one scene.
        let all: Vec<&str> = snips.iter().map(|(_, t)| *t).collect();
        v.push(case(format!("{group}/all"), scene_with(&all.join("\n"))));
    }
    for (name, cond) in WHEN_SNIPS {
        v.push(case(format!("when/{name}"), scene_with(&when_connect(cond))));
    }
    let twins = "[[entity]]\nname = \"twin\"\n\n[[entity]]\nname = \"twin\"\n";
    let on_twin = when_connect("{ entity = \"twin\", enabled = true }");
    v.push(case("when/duplicate-name-entity", scene_with(&format!("{twins}\n{on_twin}"))));
    let all_when: Vec<&str> = WHEN_SNIPS.iter().map(|(_, c)| *c).collect();
    v.push(case("when/all", scene_with(&when_connect(&all_when.join(", ")))));
    for (name, scene) in GROUND_SCENES {
        v.push(case(format!("ground/{name}"), bare_scene(scene)));
    }
    // Several at a time, in files of their own.
    let mut grounds = bare(&[("rwgpu.toml", PROJECT)]);
    for (i, (_, scene)) in GROUND_SCENES.iter().enumerate() {
        grounds.insert(format!("scenes/g{i:02}.toml"), *scene);
    }
    v.push(case("ground/all", grounds));
    for (name, text) in PLAYTESTS {
        v.push(case(format!("playtest/{name}"), with(&[("tests/t.toml", text)])));
    }
    let mut tests = with(&[]);
    for (i, (_, text)) in PLAYTESTS.iter().enumerate() {
        tests.insert(format!("tests/t{i:02}.toml"), *text);
    }
    v.push(case("playtest/all", tests));
    v.extend(file_cases());
    v.extend(shape_cases());
    v.push(case("everything", everything()));
    v.extend(test_inputs());
    v.extend(generated());
    v.extend(limit_cases());
    v
}
// ---------------------------------------------------------------------------
// Shape lists (ADR 0029). A group of its own: the groups above are in
// `everything`, whose pin is older than shapes.

/// A house of three shapes with no problem in it.
const HOUSE_SHAPES: &str = "footprint = [2, 2]\n\n[[shape]]\nkind = \"box\"\nsize = [2, 2, 1.5]\ncolor = \"#b5651d\"\n\n[[shape]]\nkind = \"roof\"\nat = [0, 0, 1.5]\nsize = [2, 2, 0.8]\ncolor = \"#8b2f2f\"\n\n[[shape]]\nkind = \"box\"\nat = [0.25, 0.25, 1.5]\nsize = [0.25, 0.25, 1]\ncolor = \"#555555\"\n";

/// Entities with shape lists, appended to the base scene: (name, text).
const SHAPE_SNIPS: &[(&str, &str)] = &[
    (
        "house",
        "[[entity]]\nname = \"home\"\npos = [0, 2]\nfootprint = [2, 2]\n\n[[entity.shape]]\nkind = \"box\"\nsize = [2, 2, 1.5]\n\n[[entity.shape]]\nkind = \"roof\"\nat = [0, 0, 1.5]\nsize = [2, 2, 0.8]\ncolor = \"#8b2f2f\"\nturn = 90\n",
    ),
    (
        "every-kind",
        "[[entity]]\nname = \"kit\"\npos = [0, 2]\nfootprint = [3, 2]\nshape = [{ kind = \"box\", size = [1, 1, 1] }, { kind = \"roof\", at = [1, 0], size = [1, 1, 0.5], turn = 0 }, { kind = \"wedge\", at = [2, 0], size = [1, 1, 0.5], turn = 270 }, { kind = \"cylinder\", at = [0, 1, 0], size = [1, 1, 2], color = \"#ccc\" }, { kind = \"cone\", at = [1.25, 1.25, 0.5], size = [0.5, 0.5, 1], turn = 180 }]\n",
    ),
    ("empty-list", "[[entity]]\nname = \"bare\"\npos = [0, 2]\nshape = []\n"),
    ("kind-hint", "[[entity]]\npos = [0, 2]\nshape = [{ kind = \"cilinder\", size = [1, 1, 1] }]\n"),
    (
        "kind-no-hint",
        "[[entity]]\npos = [0, 2]\nshape = [{ kind = \"prism\", size = [1, 1, 1] }, { kind = 3, size = [1, 1, 1] }]\n",
    ),
    ("required-missing", "[[entity]]\npos = [0, 2]\nshape = [{ size = [1, 1, 1] }, { kind = \"box\" }, {}]\n"),
    (
        "size-wrong",
        "[[entity]]\npos = [0, 2]\nshape = [{ kind = \"box\", size = [1, 1] }, { kind = \"box\", size = [1, 0, 1] }, { kind = \"box\", size = [1, 1, 64.5] }, { kind = \"box\", size = \"big\" }, { kind = \"box\", size = [1, -1, 1] }, { kind = \"box\", size = [1, \"a\", 1] }, { kind = \"box\", size = [1, 1, 64] }]\n",
    ),
    (
        "at-wrong",
        "[[entity]]\npos = [0, 2]\nshape = [{ kind = \"box\", at = [1], size = [1, 1, 1] }, { kind = \"box\", at = [0, 0, 0, 0], size = [1, 1, 1] }, { kind = \"box\", at = [nan, 0], size = [1, 1, 1] }]\n",
    ),
    (
        "turn-wrong",
        "[[entity]]\npos = [0, 2]\nshape = [{ kind = \"roof\", size = [1, 1, 1], turn = 45 }, { kind = \"roof\", size = [1, 1, 1], turn = \"90\" }, { kind = \"roof\", size = [1, 1, 1], turn = 360 }, { kind = \"roof\", size = [1, 1, 1], turn = -90 }, { kind = \"roof\", size = [1, 1, 1], turn = 90.0 }]\n",
    ),
    (
        "color-wrong",
        "[[entity]]\nname = \"glass\"\npos = [0, 2]\nshape = [{ kind = \"box\", size = [1, 1, 1], color = \"#ff000080\" }, { kind = \"box\", at = [0, 0, 1], size = [1, 1, 1], color = \"red\" }, { kind = \"cone\", at = [0, 0, 2], size = [1, 1, 1], color = \"#abc8\" }]\n",
    ),
    (
        "unknown-key-hint",
        "[[entity]]\npos = [0, 2]\nshape = [{ kind = \"box\", size = [1, 1, 1], colour = \"#fff\" }]\n",
    ),
    ("unknown-key-no-hint", "[[entity]]\npos = [0, 2]\nshape = [{ kind = \"box\", size = [1, 1, 1], zzzzzzzz = 1 }]\n"),
    (
        "not-tables",
        "[[entity]]\nname = \"a\"\npos = [0, 2]\nshape = [1, \"box\"]\n\n[[entity]]\nname = \"b\"\npos = [1, 2]\nshape = \"box\"\n",
    ),
    (
        "outside",
        "[[entity]]\nname = \"wide\"\npos = [0, 2]\nshape = [{ kind = \"box\", size = [1.5, 1, 1] }, { kind = \"box\", at = [0, -0.25, 1], size = [1, 1, 1] }, { kind = \"cone\", at = [0.25, 0.25, 2], size = [0.5, 0.5, 1] }, { kind = \"box\", at = [0.1, 0.3, 3], size = [0.9, 0.7, 1] }]\n",
    ),
    (
        "outside-allowed",
        "[[entity]]\nname = \"eaves\"\npos = [0, 2]\nallow = [\"W0208\"]\nshape = [{ kind = \"box\", size = [1, 1, 1] }, { kind = \"roof\", at = [-0.125, -0.125, 1], size = [1.25, 1.25, 0.5] }]\n",
    ),
    (
        "one-plane",
        "[[entity]]\nname = \"flush\"\npos = [0, 2]\nfootprint = [2, 2]\nshape = [{ kind = \"box\", size = [2, 2, 1] }, { kind = \"box\", at = [1.5, 0.5, 0.5], size = [0.5, 0.5, 2] }, { kind = \"wedge\", at = [0.5, 1.5, 0], size = [1, 0.5, 0.5], turn = 90 }, { kind = \"roof\", at = [0, 0.5, 0.25], size = [1, 1, 0.5] }, { kind = \"wedge\", at = [0.5, 0, 0.5], size = [1, 0.5, 1], turn = 90 }]\n",
    ),
    (
        "one-plane-allowed",
        "[[entity]]\nname = \"flush2\"\npos = [0, 2]\nallow = [\"w0209\"]\nshape = [{ kind = \"box\", size = [1, 1, 1] }, { kind = \"box\", at = [0.5, 0.25, 0.5], size = [0.5, 0.5, 2] }]\n",
    ),
    (
        "planes-apart",
        "[[entity]]\nname = \"fine\"\npos = [0, 2]\nfootprint = [2, 2]\nshape = [{ kind = \"box\", size = [1, 2, 1] }, { kind = \"roof\", at = [0, 0, 1], size = [1, 2, 0.5], turn = 90 }, { kind = \"box\", at = [1, 0, 0], size = [1, 1, 2] }, { kind = \"box\", at = [1, 1, 0], size = [1, 1, 0.5] }, { kind = \"cylinder\", at = [1, 1, 0.5], size = [1, 1, 1] }, { kind = \"wedge\", at = [1, 0, 2], size = [1, 1, 0.5], turn = 180 }]\n",
    ),
    (
        "both-warnings",
        "[[entity]]\nname = \"odd\"\npos = [0, 2]\nshape = [{ kind = \"box\", size = [1, 1, 1] }, { kind = \"box\", at = [0.5, 0, 0.5], size = [1, 0.5, 1] }]\n",
    ),
    (
        "on-screen",
        "[[entity]]\nname = \"icon\"\nscreen_pos = [4, 4]\nshape = [{ kind = \"box\", size = [1, 1, 1] }]\nlabel = { text = \"x\" }\n",
    ),
    (
        "cells-checked-once",
        "[[entity]]\ncells = [[0, 2], [1, 2], [0, 3]]\nshape = [{ kind = \"box\", size = [2, 1, 1] }]\n",
    ),
    (
        "with-block",
        "[[entity]]\nname = \"post\"\npos = [0, 2]\nblock = { height = 0.5 }\nshape = [{ kind = \"cylinder\", at = [0.25, 0.25, 0.5], size = [0.5, 0.5, 1] }]\n",
    ),
];

/// Shape lists in prefab files, and what uses them.
fn shape_cases() -> Vec<Case> {
    let mut v = Vec::new();
    for (name, text) in SHAPE_SNIPS {
        v.push(case(format!("shape/{name}"), scene_with(text)));
    }
    let all: Vec<&str> = SHAPE_SNIPS.iter().map(|(_, t)| *t).collect();
    v.push(case("shape/all", scene_with(&all.join("\n"))));
    let homes = "[[entity]]\nprefab = \"home\"\npos = [0, 2]\n\n[[entity]]\nname = \"second\"\nprefab = \"home\"\npos = [6, 3]\n";
    let scene = |extra: &str| format!("{}\n{extra}", base_scene());
    v.push(case("shape/prefab", with(&[("prefabs/home.toml", HOUSE_SHAPES), ("scenes/main.toml", &scene(homes))])));
    // A prefab's list is checked in the prefab's file, once, however many use it (or none).
    let odd = "footprint = [2, 1]\n\n[[shape]]\nkind = \"box\"\nsize = [2, 1, 1]\n\n[[shape]]\nkind = \"box\"\nat = [1.5, 0.25, 0.5]\nsize = [0.5, 0.5, 1]\n\n[[shape]]\nkind = \"cone\"\nat = [0, 0.5, 1]\nsize = [1, 1, 1]\n";
    v.push(case("shape/prefab-warnings", with(&[("prefabs/home.toml", odd), ("scenes/main.toml", &scene(homes))])));
    v.push(case("shape/prefab-warnings-unused", with(&[("prefabs/home.toml", odd)])));
    let allowed = format!("allow = [\"W0208\", \"W0209\"]\n{odd}");
    v.push(case(
        "shape/prefab-warnings-allowed",
        with(&[("prefabs/home.toml", &allowed), ("scenes/main.toml", &scene(homes))]),
    ));
    // An entity's `allow` does not reach into its prefab's file.
    let entity_allows = "[[entity]]\nprefab = \"home\"\npos = [0, 2]\nallow = [\"W0208\", \"W0209\"]\n";
    v.push(case(
        "shape/prefab-warnings-entity-allow",
        with(&[("prefabs/home.toml", odd), ("scenes/main.toml", &scene(entity_allows))]),
    ));
    let glass = "[[shape]]\nkind = \"box\"\nsize = [1, 1, 1]\ncolor = \"#ffffff80\"\n";
    v.push(case(
        "shape/prefab-see-through",
        with(&[("prefabs/home.toml", glass), ("scenes/main.toml", &scene(homes))]),
    ));
    let invalid = "[[shape]]\nkind = \"sphere\"\nsize = [1, 1]\nsides = 8\n";
    v.push(case("shape/prefab-invalid", with(&[("prefabs/home.toml", invalid), ("scenes/main.toml", &scene(homes))])));
    // Its prefab's list against its own footprint, and a list of its own in place of the prefab's.
    let smaller = "[[entity]]\nname = \"squeezed\"\nprefab = \"home\"\npos = [0, 2]\nfootprint = [1, 2]\n\n[[entity]]\nname = \"roomy\"\nprefab = \"home\"\npos = [6, 3]\nfootprint = [2, 3]\n";
    v.push(case(
        "shape/prefab-own-footprint",
        with(&[("prefabs/home.toml", HOUSE_SHAPES), ("scenes/main.toml", &scene(smaller))]),
    ));
    let own = "[[entity]]\nname = \"shed\"\nprefab = \"home\"\npos = [0, 2]\nshape = [{ kind = \"box\", size = [2, 2.5, 1] }]\n\n[[entity]]\nname = \"lot\"\nprefab = \"home\"\npos = [6, 3]\nshape = []\n";
    v.push(case(
        "shape/prefab-own-list",
        with(&[("prefabs/home.toml", HOUSE_SHAPES), ("scenes/main.toml", &scene(own))]),
    ));
    // A shaped prefab that is spawned, and one that connections listen to.
    let spawned = "[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\naction = \"spawn\"\nprefab = \"home\"\n\n[[connect]]\nfrom_prefab = \"home\"\nsignal = \"tapped\"\nto = \"self\"\naction = \"disable\"\n";
    v.push(case(
        "shape/prefab-spawned",
        with(&[("prefabs/home.toml", HOUSE_SHAPES), ("scenes/main.toml", &scene(spawned))]),
    ));
    v.push(case(
        "shape/prefab-spawned-warnings",
        with(&[("prefabs/home.toml", odd), ("scenes/main.toml", &scene(spawned))]),
    ));
    v
}

// ---------------------------------------------------------------------------
// Sizes over the limits (E0209)

/// `n` entities made from `prefab`, each on a cell of their own.
fn copies_of(prefab: &str, n: usize) -> String {
    (0..n).map(|i| format!("[[entity]]\nprefab = \"{prefab}\"\npos = [{}, {}]\n\n", i % 64, i / 64)).collect()
}

/// A prefab of one mebibyte (most of it a comment).
fn mebibyte_prefab() -> String {
    let body = "tile = {}\n";
    format!("{body}#{}\n", "x".repeat((1 << 20) - body.len() - 2))
}

/// A playtest of `n` [[expect]] tables.
fn expects(n: usize) -> String {
    (0..n).map(|_| "[[expect]]\nevent = \"moved\"\n").collect()
}

fn limit_cases() -> Vec<Case> {
    let long = "k".repeat(129);
    let exactly = "k".repeat(128);
    let max_file = rwgpuengine_core::limits::FILE_BYTES.value as usize;
    let mut v = vec![
        case("limits/file", with(&[("scenes/main.toml", &format!("#{}\n", "x".repeat(max_file - 1)))])),
        case("limits/file-exactly", with(&[("tests/t.toml", &format!("#{}\n", "x".repeat(max_file - 2)))])),
        case("limits/key", scene_with(&format!("[[entity]]\nname = \"e\"\n{long} = 1\n"))),
        case("limits/key-exactly", scene_with(&format!("[[entity]]\nname = \"e\"\nprops = {{ {exactly} = 1 }}\n"))),
        case("limits/prop-key", scene_with(&format!("[[entity]]\nname = \"e\"\nprops = {{ {long} = 1, b = 2 }}\n"))),
        case("limits/counts-key", scene_with(&format!("[[entity]]\nname = \"e\"\ncounts = {{ {long} = \"hut\" }}\n"))),
        case(
            "limits/name",
            scene_with(&format!(
                "[[entity]]\nname = \"{long}\"\n\n[[connect]]\nfrom = \"plate\"\nsignal = \"pressed\"\nto = \"{long}x\"\naction = \"enable\"\n"
            )),
        ),
        case("limits/name-exactly", scene_with(&format!("[[entity]]\nname = \"{exactly}\"\n"))),
        case("limits/ground", bare_scene("[ground]\nsize = [4097, 4096]\n\n[[entity]]\nprefab = \"nope\"\n")),
        case("limits/ground-exactly", bare_scene("[ground]\nsize = [4096, 4096]\n")),
        case("limits/ground-huge", bare_scene("[ground]\nsize = [2147483647, 2147483647]\n")),
        case(
            "limits/map",
            bare_scene(&format!("[ground]\nmap = [\"{}\"]\n\n[[terrain]]\nchar = \"g\"\n", "g".repeat(16_777_217))),
        ),
        case(
            "limits/covered",
            scene_with(
                "[[entity]]\nname = \"field\"\npos = [0, 9]\nfootprint = [2048, 2049]\ntile = {}\n\n[[entity]]\nprefab = \"nope\"\n",
            ),
        ),
        case(
            "limits/covered-adds-up",
            scene_with(
                "[[entity]]\npos = [0, 9]\nfootprint = [2048, 1024]\ntile = {}\n\n[[entity]]\ncells = [[0, 2000], [1, 2000]]\nfootprint = [1024, 1024]\ntile = {}\n",
            ),
        ),
        case(
            "limits/footprint-huge",
            scene_with("[[entity]]\npos = [0, 9]\nfootprint = [2147483647, 2147483647]\nblock = {}\nsolid = true\n"),
        ),
        case(
            "limits/prefab-copies",
            with(&[
                ("prefabs/big.toml", &mebibyte_prefab()),
                (
                    "scenes/main.toml",
                    &format!("{}\n[[entity]]\nprefab = \"wal\"\n\n{}", base_scene(), copies_of("big", 129)),
                ),
            ]),
        ),
        case(
            "limits/prefab-copies-exactly",
            bare(&[
                ("rwgpu.toml", PROJECT),
                ("prefabs/big.toml", &mebibyte_prefab()),
                ("scenes/main.toml", &copies_of("big", 128)),
            ]),
        ),
        case("limits/playtest", with(&[("tests/t.toml", &expects(100_001))])),
        case(
            "limits/playtest-exactly",
            with(&[("tests/t.toml", &format!("[test]\nscene = \"scenes/main.toml\"\n{}", expects(100_000)))]),
        ),
    ];
    // Every cell of `cells` is an entity.
    let cells: Vec<String> = (0..1_000_001).map(|i| format!("[{}, {}]", i % 1000, i / 1000)).collect();
    v.push(case(
        "limits/entities",
        bare_scene(&format!(
            "[[entity]]\nprefab = \"nope\"\n\n[[entity]]\ncells = [{}]\ntile = {{}}\n",
            cells.join(", ")
        )),
    ));
    // Several limits, and other problems, in the files of one project.
    v.push(case(
        "limits/everything",
        with(&[
            ("rwgpu.toml", &format!("[project]\nname = \"all\"\nengine = \"0.0\"\n{long} = 1\n")),
            ("prefabs/big.toml", &mebibyte_prefab()),
            ("prefabs/odd.toml", &format!("{long} = 1\n")),
            ("scenes/a.toml", "[ground]\nsize = [5000, 5000]\n\n[[entity]]\nprefab = \"odd\"\n"),
            ("scenes/b.toml", &format!("[[entity]]\nprefab = \"wal\"\n\n{}", copies_of("big", 130))),
            (
                "scenes/c.toml",
                "[[entity]]\nfootprint = [3000, 3000]\nblock = {}\n\n[[entity]]\nname = \"x\"\nsprit = {}\n",
            ),
            ("scenes/d.toml", &format!("[[entity]]\nname = \"{long}\"\n")),
            ("tests/t.toml", &format!("[test]\nscene = \"scenes/c.toml\"\n{}", expects(100_001))),
            ("tests/u.toml", "[[expect]]\nentity = \"zeppelin\"\npos = [0, 0]\n"),
        ]),
    ));
    // Shape lists (ADR 0029): one over the limit in a scene and in a prefab, and one of exactly the limit.
    let most = rwgpuengine_core::limits::SHAPES_IN_A_LIST.get();
    let shapes = |n: usize| -> String {
        (0..n)
            .map(|i| format!("[[shape]]\nkind = \"box\"\nat = [0, 0, {}]\nsize = [1, 1, 0.5]\n\n", i as f32 * 0.5))
            .collect()
    };
    let in_scene = |n: usize| {
        format!("[[entity]]\nname = \"tower\"\npos = [0, 2]\n\n{}", shapes(n).replace("[[shape]]", "[[entity.shape]]"))
    };
    let towers = "[[entity]]\nprefab = \"tower\"\npos = [0, 2]\n\n[[entity]]\nprefab = \"tower\"\npos = [1, 2]\n";
    v.push(case("limits/shapes", scene_with(&format!("{}\n[[entity]]\nprefab = \"nope\"\n", in_scene(most + 1)))));
    v.push(case("limits/shapes-exactly", scene_with(&in_scene(most))));
    v.push(case(
        "limits/shapes-prefab",
        with(&[
            ("prefabs/tower.toml", &shapes(most + 2)),
            ("scenes/main.toml", &format!("{}\n{towers}", base_scene())),
        ]),
    ));
    v
}

/// One project with a problem of every kind, in many files.
fn everything() -> Vfs {
    let mut vfs = with(&[
        ("rwgpu.toml", "[project]\nname = \"all\"\nengine = \"0.0\"\n\n[display]\nwidth = 0\n"),
        ("prefabs/wall.toml", "block = {}\nsolid = true\nsolidd = 1\n"),
        ("prefabs/junk.toml", "[x\n"),
    ]);
    let joined = |snips: &[(&str, &str)]| snips.iter().map(|(_, t)| *t).collect::<Vec<_>>().join("\n");
    vfs.insert("scenes/main.toml", format!("{}\n{}", base_scene(), joined(ENTITY_SNIPS)));
    vfs.insert("scenes/b.toml", format!("{}\n{}", base_scene(), joined(CONNECT_SNIPS)));
    vfs.insert("scenes/c.toml", format!("{}\n{}", base_scene(), joined(PLACEMENT_SNIPS)));
    let all_when: Vec<&str> = WHEN_SNIPS.iter().map(|(_, c)| *c).collect();
    vfs.insert("scenes/d.toml", format!("{}\n{}", base_scene(), when_connect(&all_when.join(", "))));
    for (i, (_, scene)) in GROUND_SCENES.iter().enumerate().step_by(3) {
        vfs.insert(format!("scenes/g{i:02}.toml"), *scene);
    }
    for (i, (_, text)) in PLAYTESTS.iter().enumerate().step_by(2) {
        vfs.insert(format!("tests/t{i:02}.toml"), *text);
    }
    vfs.insert(
        "tests/zz.toml",
        "[test]\nscene = \"scenes/b.toml\"\n\n[[expect]]\nentity = \"hud\"\nprops = { coin = 1 }\n",
    );
    vfs
}

// ---------------------------------------------------------------------------
// Pins

/// The hash of each case's record, recorded from the loader before ADR 0028
/// (the cases under `limits/`: with the limits, which they are made to reach).
/// `examples/tiny_town` was recorded again when the example got a fifth
/// playtest (ADR 0030): its record differs from the one before only by the
/// lines that list and decode that file, and every other case's is the same.
///
/// Five pins are newer, by one word each (ADR 0029): where an unknown key of
/// an entity or of a prefab file has no near name, the hint lists the keys
/// there are, and `shape` is one now (`entity/unknown-key-no-hint`,
/// `entity/all`, `files/prefab-unused-invalid`,
/// `files/prefab-invalid-from-prefab`, `everything`). Every other line of
/// every record of before is the same byte for byte. The cases under
/// `shape/` and `limits/shapes*` came with shapes.
const PINS: &[(&str, &str)] = &[
    ("examples/coin_vault", "b7e48e8a696ae553"),
    ("examples/iso_garden", "c84862e5a560d2ec"),
    ("examples/tiny_town", "bd8547dbcbc2557a"),
    ("base", "156b1dceb27c7367"),
    ("project/syntax", "2e77e8d7e6435ae3"),
    ("project/empty", "4658d7bd83cf6967"),
    ("project/unknown-key", "fa1dea3bfe2183e2"),
    ("project/unknown-section", "cb1d57afb04b0b80"),
    ("project/missing-name", "4a1ced61c35312ca"),
    ("project/wrong-type", "afd30e99aa9e3e4a"),
    ("project/display-range", "0a5ca04b5f831104"),
    ("project/sim-range", "7e43310c23b6dac3"),
    ("project/iso-range", "42d6f7b46d43c695"),
    ("project/main-missing-hint", "700dbee4c8248a5f"),
    ("project/main-missing-no-hint", "4e9688bb93031d1e"),
    ("project/main-path-invalid", "8371c032f9296eb1"),
    ("project/main-path-backslash", "120d9ef7d79f42f7"),
    ("project/engine-mismatch", "19f38d9c89b49e9b"),
    ("project/engine-match", "a5b833854c7c1e00"),
    ("project/engine-patch", "a58c34d8b2982b23"),
    ("project/engine-and-main-missing", "79958ee18a551bdc"),
    ("project/display-ok", "c9dda961f4796634"),
    ("project/project-not-a-table", "708ec39cbe6ad9b2"),
    ("entity/unknown-prefab-hint", "0d3e1c855b6b9536"),
    ("entity/unknown-prefab-no-hint", "012b3f5208eee157"),
    ("entity/unknown-prefab-subfolder", "6f6a8457fe445eb4"),
    ("entity/name-with-cells", "be8bf5746cb1decb"),
    ("entity/duplicate-name", "1b2856ffebdb0bd5"),
    ("entity/duplicate-name-twice", "ca10f4c06c37c52b"),
    ("entity/reserved-ground", "5aaa101ad02a8bdd"),
    ("entity/reserved-self", "8a1d554b557054eb"),
    ("entity/props-and-counts", "7af14c8d38c02289"),
    ("entity/footprint-and-behaviors", "3523c5663d37b1d9"),
    ("entity/footprint-pushable", "9a3b052e73ccda3b"),
    ("entity/footprint-screen", "631d0b5de7664cf9"),
    ("entity/footprint-zero", "f72117cccdfcded4"),
    ("entity/label-unclosed", "1848d53593ceb6fd"),
    ("entity/label-single-brace", "12d55a091f2ede9e"),
    ("entity/label-bad-placeholder", "298dae9fb69189d9"),
    ("entity/label-empty-placeholder", "d0bda36eff39237d"),
    ("entity/label-unknown-prop-hint", "1edaa41134e2895d"),
    ("entity/label-unknown-props-no-hint", "7bcae561f2de91a3"),
    ("entity/label-unsupported-chars", "b5eb8a2d59d8559a"),
    ("entity/label-unsupported-allowed", "0df97ae5e3f23c1c"),
    ("entity/label-escaped-braces", "5541bb054050f345"),
    ("entity/label-on-prefab", "baf5fa122564c2ec"),
    ("entity/pushable-moving", "cae04d2bcfdcedbd"),
    ("entity/screen-world-parts", "db59ffca2607222f"),
    ("entity/screen-movers", "8d3597124c1e75b8"),
    ("entity/counts-unknown-prefab-hint", "c6c923331ae9e371"),
    ("entity/counts-unknown-prefab-no-hint", "4e5a6b3ece8e391a"),
    ("entity/cells-checked-once", "9b804e13fe7ff064"),
    ("entity/cells-one-checked", "638b021dcce6dfc0"),
    ("entity/rules-and-counts", "bf65b6aeb978909e"),
    ("entity/cells-duplicates", "b90ab35f878a1a27"),
    ("entity/cells-bad-item", "e3b5d587ff85a7cb"),
    ("entity/cells-empty", "5860dfe29aa7b812"),
    ("entity/pos-wrong", "09d8509276adb928"),
    ("entity/pos-not-finite", "1f044fee5b29ac2e"),
    ("entity/pos-rounding", "4adc243c50151f39"),
    ("entity/name-invalid", "8439275b913f98c1"),
    ("entity/name-wrong-type", "829f302ac8eae24a"),
    ("entity/unknown-key-hint", "b5c82855140cd9e6"),
    ("entity/unknown-key-no-hint", "60882b18f6865f74"),
    ("entity/unknown-component-key", "166b582c65949910"),
    ("entity/component-wrong-type", "48d97e00bfb983dc"),
    ("entity/color-invalid", "dd7ca6235dcb4093"),
    ("entity/enum-invalid", "02d41179e759d20c"),
    ("entity/int-range", "e2ce69cecfe6ca91"),
    ("entity/num-range", "a8f3ad49b925de44"),
    ("entity/props-nested", "8d77aea437cd5070"),
    ("entity/props-wrong-type", "bd9895328d683b0c"),
    ("entity/counts-not-names", "733cf7e765a62847"),
    ("entity/allow-invalid", "3d1f8bb05db13f3a"),
    ("entity/allow-wrong-type", "1a73634911bc1125"),
    ("entity/missing-required", "df6e472e0ecbc578"),
    ("entity/bool-wrong-type", "ef392472d4c1c046"),
    ("entity/all", "597cf038d383fb0d"),
    ("placement/solid-overlap", "5445844e40504593"),
    ("placement/solid-overlap-allowed-first", "a7bc67f6acfacf26"),
    ("placement/solid-overlap-footprint", "0215b0a7bc1b465f"),
    ("placement/solid-overlap-three", "b8ab38c4568d0062"),
    ("placement/solid-disabled-ignored", "a34f7baa2537826c"),
    ("placement/pushable-on-wall", "4ac763cf8b4ebb56"),
    ("placement/actor-in-solid", "3bef4430a9fa8bb0"),
    ("placement/actor-in-solid-unnamed", "03aeb024d6d26817"),
    ("placement/actor-in-solid-allowed", "51411f710a2c52ed"),
    ("placement/actor-in-house", "32f8fe8af0f58fad"),
    ("placement/actor-in-disabled-solid", "cfb9861a14451d1a"),
    ("placement/actor-on-pushable", "9d840a23c787b377"),
    ("placement/actor-on-actor", "df8511daa8fba506"),
    ("placement/disabled-actor-in-solid", "2b7da6819d841fc2"),
    ("placement/actor-in-solid-actor", "0fbac237ac62d117"),
    ("placement/outside-ground", "b3270c214e662f5f"),
    ("placement/outside-ground-negative", "e4e4e4d3332b83f6"),
    ("placement/outside-ground-footprint", "9faf84f9213c4a82"),
    ("placement/outside-ground-screen", "cb45d5db11c1121d"),
    ("placement/outside-ground-allowed", "63933e8ad3f72516"),
    ("placement/outside-ground-cells", "832f9d51aebf35ef"),
    ("placement/three-warnings-one-entity", "f30e159c65d61b4a"),
    ("placement/data-entity-anywhere", "a2d9cee2b5174fbd"),
    ("placement/all", "cb40c5844f7f2d8f"),
    ("connect/from-both", "c7f129c097e3cee3"),
    ("connect/from-neither", "8b20f49508f2af0d"),
    ("connect/from-unknown-hint", "ca1b065506bc9da0"),
    ("connect/from-unknown-no-hint", "7b94f36730acb0f1"),
    ("connect/from-cells-entity", "939b50d3120d45d5"),
    ("connect/from-prefab-unknown", "a36fcf5e9f4a548e"),
    ("connect/from-prefab-unknown-no-hint", "eebf93a4d021deaa"),
    ("connect/signal-typo", "303016bf0eb90d77"),
    ("connect/signal-changed-never", "08ee2c22a1fca6ce"),
    ("connect/signal-spawned-never", "4add5ccb3e910d10"),
    ("connect/signal-refused-never", "379cf4a46baa69b0"),
    ("connect/signal-other-never", "248383fd41f5e250"),
    ("connect/signal-custom-trigger", "096d790ba194375d"),
    ("connect/signal-custom-trigger-typo", "9e2493c25d62e3a5"),
    ("connect/signal-prefab-pressed", "dff40692a36d59e2"),
    ("connect/signal-prefab-spawned", "097a4a05412e87a2"),
    ("connect/signal-prefab-changed-never", "49d065236f60e145"),
    ("connect/signal-prefab-refused-never", "9c4a83b3838e53ba"),
    ("connect/signal-prefab-spawned-never", "b985e2023565e71d"),
    ("connect/signal-prefab-collected", "1d50f5637eda6e0e"),
    ("connect/signal-prefab-changed-named-copy", "eb994a0c3cf09db7"),
    ("connect/signal-prefab-refused-named-copy", "df2120433b12c385"),
    ("connect/signal-prefab-refused-self", "2b8fadc335688288"),
    ("connect/signal-ground-pressed", "34eb1be33458cc1a"),
    ("connect/signal-ground-taped", "a315c8925f84335d"),
    ("connect/signal-ground-changed", "61264c7e9d7fa971"),
    ("connect/signal-counts-changed", "af87cc027e1491d1"),
    ("connect/signal-cost-payer-changed", "71a12fb23f3e9a1c"),
    ("connect/signal-self-changed", "476dae6f9bd9fee7"),
    ("connect/signal-and-form", "37c34e5f121d5a08"),
    ("connect/spawn-to-and-prefab", "2ea0df2a23930e37"),
    ("connect/action-both", "96ea76bfe2120c09"),
    ("connect/action-neither", "4ab8904c3bef707d"),
    ("connect/actions-top-keys", "74facaef5c162042"),
    ("connect/actions-empty", "728b7eb165aeac1e"),
    ("connect/actions-not-tables", "45e77a2c0ac27a07"),
    ("connect/actions-each-checked", "9b73f634b9aaf7cb"),
    ("connect/spawn-with-to", "ce1f734c13259225"),
    ("connect/spawn-no-prefab", "3bbb8eacd9e911a5"),
    ("connect/spawn-unknown-prefab", "cfb25fa17e5460d6"),
    ("connect/spawn-unknown-prefab-no-hint", "1032600332392dc4"),
    ("connect/spawn-twice", "76737e9c5b560d52"),
    ("connect/spawn-with-prop", "b9c05db6a89fb6ec"),
    ("connect/cost-unknown-entity-hint", "95f2aa73954cdaf1"),
    ("connect/cost-unknown-entity-no-hint", "2705853e61567787"),
    ("connect/cost-unknown-prop", "dde7ce4f26589179"),
    ("connect/cost-counts-prop", "746fed6e3928c731"),
    ("connect/cost-string-prop", "e42c978a1738dea9"),
    ("connect/cost-zero", "c82a57f6acbadc59"),
    ("connect/cost-negative", "ccf1f43d3053f619"),
    ("connect/cost-float-on-int", "f5aaa8ae543b8bfd"),
    ("connect/cost-missing-amount", "8539b037d7215d49"),
    ("connect/cost-float-on-float", "d915150279c0c44c"),
    ("connect/cost-unknown-key", "e24c7aaea61b6e76"),
    ("connect/cost-on-non-spawn", "a466606143538959"),
    ("connect/prefab-on-non-spawn", "ad482876e5d0094d"),
    ("connect/non-spawn-no-to", "e6895737a210ede7"),
    ("connect/self-with-ground", "dbd67f70c8c4f3a1"),
    ("connect/to-unknown-hint", "101c640efd322196"),
    ("connect/to-unknown-no-hint", "721ccef3efd65c71"),
    ("connect/to-unknown-with-prop", "115ab042996fe3c2"),
    ("connect/self-entity-add", "c1af285e8c6ba13d"),
    ("connect/self-entity-unknown-prop", "75ef41748511e4c2"),
    ("connect/self-unknown-source", "7b94f36730acb0f1"),
    ("connect/self-prefab-unknown-prop", "8a7e90cf426a523f"),
    ("connect/add-no-prop", "91cd9970cec5e44d"),
    ("connect/add-counts-prop", "73bfae838aabcd58"),
    ("connect/set-counts-prop", "a824e4bb0def9d24"),
    ("connect/add-string-prop", "b3bec9ad76e75813"),
    ("connect/add-float-on-int", "f3a9201b3f827b77"),
    ("connect/add-int-on-float", "7ee675485dee5b47"),
    ("connect/add-string-value", "c5051fc97fff3bb9"),
    ("connect/set-no-value", "84afb2cdb48ee5e6"),
    ("connect/set-wrong-type", "2cdeea0685a3fbe3"),
    ("connect/set-value-not-finite", "01a56e92c9eb444e"),
    ("connect/set-value-table", "c47950365929c495"),
    ("connect/prop-on-enable", "ae2eb296ca8bdefe"),
    ("connect/action-unknown", "f4b821bd5dc0b131"),
    ("connect/signal-missing", "31f2fa70ed44f705"),
    ("connect/connect-unknown-key", "be63ad4836ef5936"),
    ("connect/all", "b2415b43f8c82e27"),
    ("when/reason-with-others", "204201bf83b40864"),
    ("when/reason-unknown", "5d38bd74f81d9c97"),
    ("when/no-entity", "66c75fd4924a7ee1"),
    ("when/unknown-entity-hint", "936cef52574c2871"),
    ("when/unknown-entity-no-hint", "b5a4da79b68b4bed"),
    ("when/kinds-none", "85fa8bd696bc2a25"),
    ("when/kinds-two", "6a9e72dfdd13d65c"),
    ("when/kinds-three", "40d69f60397ca07c"),
    ("when/compare-without-prop", "462051f710979e70"),
    ("when/compares-nothing", "7c4fbfb79126e929"),
    ("when/unknown-prop-hint", "dcb234da1afc5ed7"),
    ("when/unknown-prop-no-hint", "4a09c05483821fe1"),
    ("when/at-least-on-string", "e450971df35033a3"),
    ("when/at-most-on-bool", "401fbba65ccd4545"),
    ("when/equals-type", "95318a9bd9902cfd"),
    ("when/equals-int-on-float", "90ae8dfe7c4718c1"),
    ("when/range-empty", "0fc5c8a04c850281"),
    ("when/type-and-range", "6ba352375832416c"),
    ("when/counts-prop-equals", "ab9853d5547912fd"),
    ("when/several-items", "129b066d35df56f4"),
    ("when/item-not-table", "7f2a16ed2c4736ed"),
    ("when/unknown-key", "ed79d733b5a26183"),
    ("when/not-finite", "46cf16310b226901"),
    ("when/cells-entity", "a29533b4c81b9b2f"),
    ("when/duplicate-name-entity", "90797b12d111f905"),
    ("when/all", "50adeb6cd4638d2b"),
    ("ground/terrain-char-empty", "823c3c2c1163e78b"),
    ("ground/terrain-char-long", "1432f71a5b2a63bb"),
    ("ground/terrain-char-space", "cfee72ae3e73b69f"),
    ("ground/terrain-char-twice", "9137ac852aae1f0f"),
    ("ground/terrain-missing-char", "d2f0cabfb29ff77a"),
    ("ground/terrain-char-not-string", "e573ad0334bda243"),
    ("ground/terrain-bad-keys", "8f0a40160809819e"),
    ("ground/no-size-no-map", "73a6b83bce50c096"),
    ("ground/empty-ground-table", "73a6b83bce50c096"),
    ("ground/ground-not-table", "c8f17e70e5fb3a42"),
    ("ground/map-empty-row", "5ecea8c3f354b624"),
    ("ground/map-empty-list", "4c891ebbe9df4fb7"),
    ("ground/map-ragged", "7856f60ad240ec30"),
    ("ground/map-unknown-char", "b0ec94d48b4acba5"),
    ("ground/map-unknown-none-declared", "6fa250b02f695c2f"),
    ("ground/map-unknown-several-rows", "90ea1cb3f8c2e4ef"),
    ("ground/map-unknown-unicode", "d81150ba6a887109"),
    ("ground/map-ragged-and-unknown", "35298c9a689bf18c"),
    ("ground/map-size-mismatch", "b37e92845911cb02"),
    ("ground/map-size-match", "cab77c4b4f700bda"),
    ("ground/map-colors", "b39b75e61739e417"),
    ("ground/map-not-strings", "2be758f0ffba265a"),
    ("ground/map-unicode-terrain", "0c2c253ab1bd30fb"),
    ("ground/plain", "18ae39045131d56e"),
    ("ground/plain-alt", "05fee48f3c1df8b5"),
    ("ground/plain-size-wrong", "83f0d002d1c5668a"),
    ("ground/plain-size-float", "fc35c014560321ae"),
    ("ground/terrain-without-map", "a5856dc24254a961"),
    ("ground/negative-size", "328f376bd27977e7"),
    ("ground/zero-size", "2c1e505ee1cf65df"),
    ("ground/not-tables", "bf31f66a2a451280"),
    ("ground/no-ground", "8d9c09b27f41ad90"),
    ("ground/all", "6c4527d81e361ea9"),
    ("playtest/syntax", "d5750717bbdcfc7a"),
    ("playtest/empty", "ea8beb861c8b7379"),
    ("playtest/input-kinds-none", "db6d96aacbaf3086"),
    ("playtest/input-kinds-two", "814182c53845452a"),
    ("playtest/input-kinds-camera-tap", "6e27772a051401f2"),
    ("playtest/input-kinds-all", "a260b24cbf40857e"),
    ("playtest/for-without-hold", "44540b75b57ac070"),
    ("playtest/input-after-end", "93b8b60140c379b1"),
    ("playtest/input-after-default-end", "633b63398ca5ef0a"),
    ("playtest/input-unknown-action", "b72924d5d9735cac"),
    ("playtest/input-missing-frame", "0afd253c9e383a52"),
    ("playtest/input-camera-bad", "cc8c37bcb1a5c2ea"),
    ("playtest/expect-count-without-prefab", "4242e151262f8b68"),
    ("playtest/expect-event-prefab-wrong", "615e155fc2ba9b0d"),
    ("playtest/expect-prefab-extra", "7f114f7eb43ea4da"),
    ("playtest/expect-prefab-event-keys", "b23faa374a0e05e2"),
    ("playtest/expect-checks-nothing", "f45c106b23113b96"),
    ("playtest/expect-mixes-event", "a7240b4437112912"),
    ("playtest/expect-event-keys-without-event", "7a0003374d60834b"),
    ("playtest/expect-range", "301b5f7c81335793"),
    ("playtest/expect-changed-not-action", "d70d69dbb34cbf76"),
    ("playtest/expect-reason-not-blocked", "3fa118ca9db092b2"),
    ("playtest/expect-frame-after-end", "fabe0d85d3c5ff9e"),
    ("playtest/scene-missing-hint", "e3a70ab7c28d5852"),
    ("playtest/scene-missing-no-hint", "d989e50845e34a7a"),
    ("playtest/expect-prefab-unknown", "41ef3099000cd736"),
    ("playtest/expect-other-unknown", "8e1546069dbe7b12"),
    ("playtest/expect-entity-unknown", "a173b33ddaa4b4a6"),
    ("playtest/expect-event-entity-unknown", "d498652a4a9ab920"),
    ("playtest/expect-props-unknown", "c35fc1000b4d8d04"),
    ("playtest/expect-props-type", "a68211a1a9ecf0b5"),
    ("playtest/expect-props-float-for-int", "c49ffe43ec32b814"),
    ("playtest/expect-props-counts", "657dc9af4ecb69c1"),
    ("playtest/expect-text-no-label", "5234ab21239b4740"),
    ("playtest/expect-text-ok", "35c3a157fb3dbfed"),
    ("playtest/expect-cells-entity", "c7eef7ecaa3164ff"),
    ("playtest/expect-unknown-event", "87f6762f18a4c035"),
    ("playtest/shot-missing-reference", "0d12effa156d1499"),
    ("playtest/shot-ok", "d3b9209c5ab5e4d5"),
    ("playtest/test-unknown-key", "d1b300fa29f48c43"),
    ("playtest/test-frames-zero", "a6dfd0fadc98390b"),
    ("playtest/test-frames-huge", "a68750cf0872aaae"),
    ("playtest/expect-everything", "fbc2d12ed72aa270"),
    ("playtest/all", "93461b546e0a4d51"),
    ("ties/from", "d9612d8d46ee3c46"),
    ("ties/to", "209f65595295e0e2"),
    ("ties/cost", "5b9be1e9c8923829"),
    ("ties/when", "9dda23db1b401ea9"),
    ("ties/expect", "9318f5c66db6a65e"),
    ("ties/props", "e9334297599a1375"),
    ("ties/prefabs", "19aa1200e1ac2d77"),
    ("files/prefab-syntax", "451102ab2f2a041a"),
    ("files/prefab-unknown-key-used-twice", "d391fd06071e35cd"),
    ("files/prefab-unused-invalid", "e1b962775f859dcd"),
    ("files/prefab-empty", "c377855d158e3968"),
    ("files/prefab-subfolder", "aa6caac048b0e61d"),
    ("files/prefab-invalid-from-prefab", "7c89f80caa02fa2d"),
    ("files/spawn-screen-prefab", "ab4f0b7d6dd7dea0"),
    ("files/spawn-screen-prefab-cost", "4254e5348cfa14fa"),
    ("files/spawn-prefab-checks", "afb27f5e3917cf60"),
    ("files/self-prefab-label", "cab2a3f023eb5278"),
    ("files/main-scene-elsewhere", "f76d96d244042e06"),
    ("files/two-scenes-one-bad-prefab", "b0d2d014798d9b3e"),
    ("files/scene-empty", "af2bd80d5a277979"),
    ("files/not-toml-ignored", "156b1dceb27c7367"),
    ("files/project-only", "206f15598290710a"),
    ("files/no-project", "075f7dfdcc02b3d3"),
    ("files/empty", "e87d59f1da3f8fb6"),
    ("files/no-project-broken-files", "60d9768d76300cfe"),
    ("files/test-scene-elsewhere", "4c2bd65a44b126a8"),
    ("files/test-scene-elsewhere-fails", "2f39fbc13efd2402"),
    ("files/project-broken-playtests", "336fa5a4c1f8471b"),
    ("files/playtest-subfolder", "8e8292a3632fe077"),
    ("files/playtest-not-found", "c8faac30ac93d1d6"),
    ("files/scene-syntax", "549b2d97fefb3575"),
    ("files/scene-duplicate-key", "8fdf5bcae81faf61"),
    ("files/scene-unknown-top", "60cddbbc05a7ac8b"),
    ("files/scene-meta-bad", "1cc9e6766a3e1f3c"),
    ("files/camera-bad", "888a9213d3b229d2"),
    ("files/terrains-255", "48a2e42d10397921"),
    ("files/terrains-256", "6dbe71d9849c8d1c"),
    ("shape/house", "9ebc5342405f8016"),
    ("shape/every-kind", "b149217cef8c9aaa"),
    ("shape/empty-list", "8e05c39ccbfe292d"),
    ("shape/kind-hint", "0090848de675101d"),
    ("shape/kind-no-hint", "ae8f636354e966c7"),
    ("shape/required-missing", "d8080a2dc122e355"),
    ("shape/size-wrong", "61878fa5c5e82d89"),
    ("shape/at-wrong", "5812c74bc339287e"),
    ("shape/turn-wrong", "f3121882131ff2fa"),
    ("shape/color-wrong", "a983e47cbe84694e"),
    ("shape/unknown-key-hint", "0891ed3dca5ab577"),
    ("shape/unknown-key-no-hint", "e1adf7ed95da9991"),
    ("shape/not-tables", "e23979a947dd498c"),
    ("shape/outside", "833d0de3732471a8"),
    ("shape/outside-allowed", "f77d9472b94fb73c"),
    ("shape/one-plane", "361dde9e5799b6a4"),
    ("shape/one-plane-allowed", "4d72904fcdcd32e9"),
    ("shape/planes-apart", "50b14e33b2ed9c12"),
    ("shape/both-warnings", "02e2f95dd48fde60"),
    ("shape/on-screen", "5ef2dcf4875f77a3"),
    ("shape/cells-checked-once", "da9a8dfa33d75531"),
    ("shape/with-block", "d3466d828e326f5a"),
    ("shape/all", "e02a57e15029c4e8"),
    ("shape/prefab", "e9417fb3a483aeb9"),
    ("shape/prefab-warnings", "f077c037b70ade21"),
    ("shape/prefab-warnings-unused", "8c834f1da8e6db39"),
    ("shape/prefab-warnings-allowed", "dbfa0a1a55c7dcd4"),
    ("shape/prefab-warnings-entity-allow", "258f96be6e5dfb5a"),
    ("shape/prefab-see-through", "4340bf59524b5098"),
    ("shape/prefab-invalid", "c8207b676fd498de"),
    ("shape/prefab-own-footprint", "425535b73d750eab"),
    ("shape/prefab-own-list", "f7eaf743cef6ee70"),
    ("shape/prefab-spawned", "ede6c88cb451020d"),
    ("shape/prefab-spawned-warnings", "108e0105677140a2"),
    ("everything", "41e16c1ca2df4163"),
    ("tests/a-valid-project", "ee60916e978d9580"),
    ("tests/missing-prefab", "63bab07b03a580ed"),
    ("tests/reference-errors", "828b83965df45d91"),
    ("tests/static-placement", "4f9cfcd2ab2c042d"),
    ("tests/playtest-checks", "2628d545a2b4ed28"),
    ("tests/main-scene-and-engine", "e59a218f36b4a89d"),
    ("tests/reason-needs-blocked", "e72a34bedfe6d2ac"),
    ("tests/event-only-keys", "177633901403a539"),
    ("tests/allow-0", "2300c8befffdad96"),
    ("tests/allow-1", "4270d12217be2c45"),
    ("tests/allow-2", "a2e20b7b5f1947ed"),
    ("tests/allow-3", "691c4cd6bc4e8f5e"),
    ("tests/actor-in-solid-0", "dffd1246893ff15d"),
    ("tests/actor-in-solid-1", "177f2d7c454a36b9"),
    ("tests/logic-valid", "172445b4583b480d"),
    ("tests/logic-unknown-prop", "5ff9c1614bab19af"),
    ("tests/logic-props-0", "d488e3c40020ce53"),
    ("tests/logic-props-1", "860628c26984db2f"),
    ("tests/logic-props-2", "a431e12769123cd4"),
    ("tests/logic-props-3", "c8e6f5acdbaa8003"),
    ("tests/logic-props-4", "41e8ea4a7746770b"),
    ("tests/logic-props-5", "7cc67d8811941643"),
    ("tests/logic-when-0", "5dc3d5b2049c0fab"),
    ("tests/logic-when-1", "7dba697087842504"),
    ("tests/logic-when-2", "847435b151ed7658"),
    ("tests/logic-when-3", "f10c41be3b7ed82c"),
    ("tests/logic-when-4", "d98feda6db98bf4c"),
    ("tests/logic-when-5", "a6407dded19b4ba8"),
    ("tests/logic-when-6", "339a92d94664bc11"),
    ("tests/logic-when-7", "c1decc7e71a5e8ac"),
    ("tests/logic-when-8", "482fc0a5b1dd516b"),
    ("tests/logic-when-9", "98eff7ed36b569c3"),
    ("tests/logic-changed", "c9580ce1cfd53431"),
    ("tests/logic-label-0", "975b259c141c885c"),
    ("tests/logic-label-1", "30b7a788b2c3e70b"),
    ("tests/logic-label-2", "b67f65042a12f00f"),
    ("tests/logic-label-3", "d1fe87d490c3caa3"),
    ("tests/logic-label-4", "ca78dfb13f7eb2aa"),
    ("tests/logic-label-5", "a4dcc7f3ce25f99c"),
    ("tests/logic-label-6", "3cc288e7d4c2ada9"),
    ("tests/logic-label-escaped", "4ef168b32720fbf7"),
    ("tests/logic-label-allowed", "566c50cf5578cced"),
    ("tests/logic-expect-0", "4878db004fc7c49d"),
    ("tests/logic-expect-1", "183764c3d7ea6940"),
    ("tests/logic-expect-2", "865de919064e66c7"),
    ("tests/logic-expect-3", "02fcec8a10f2790a"),
    ("tests/pushable-0", "b896a799ae7dc319"),
    ("tests/pushable-1", "fbc1c1b030076f23"),
    ("tests/pushable-2", "06717346d5826a6f"),
    ("tests/pushable-3", "a80454b7c36241d8"),
    ("tests/builder-valid", "f8f26abfa1cee17e"),
    ("tests/terrain-0", "430bc6a76eba2cac"),
    ("tests/terrain-1", "6b641e77f8895c0e"),
    ("tests/terrain-2", "8890d2d792c0b56d"),
    ("tests/terrain-3", "c67b9c0f38979c6f"),
    ("tests/terrain-4", "d6afda6b2c96d2df"),
    ("tests/terrain-5", "acc8c5688119b04f"),
    ("tests/terrain-chars", "0d2b764673349c39"),
    ("tests/spawn-0", "52c2c36e42780830"),
    ("tests/spawn-1", "f0631c8bfe9331b8"),
    ("tests/spawn-2", "96435b074d4f02dc"),
    ("tests/builder-0", "7792531361aab377"),
    ("tests/builder-1", "71cbc973dc3ba799"),
    ("tests/builder-2", "acc456fd71bedf57"),
    ("tests/builder-3", "43f748e20d3dc179"),
    ("tests/builder-4", "c7c40622ddfd372f"),
    ("tests/builder-5", "674ff5734e550635"),
    ("tests/builder-6", "7e15cb2e491f0672"),
    ("tests/builder-7", "a3e5265227dad604"),
    ("tests/builder-8", "1c092c5cebeab061"),
    ("tests/builder-9", "502602d50b7b929e"),
    ("tests/builder-10", "4a85f8f6fa856560"),
    ("tests/builder-11", "a0dbcc8130104f75"),
    ("tests/builder-12", "fefd6eb29a20a2e9"),
    ("tests/prefab-expect-0", "fb45bb44e434a184"),
    ("tests/prefab-expect-1", "79fc09d368f8121f"),
    ("tests/prefab-expect-2", "28c1413399600293"),
    ("tests/prefab-expect-3", "44fab8f5b6e6d0dd"),
    ("tests/prefab-expect-4", "ba68bdaee0760e71"),
    ("tests/prefab-expect-5", "a62992e93f74754e"),
    ("tests/prefab-expect-6", "6b880b9a00cf6ab2"),
    ("tests/prefab-expect-7", "0ac1e3e22ab455cd"),
    ("tests/prefab-expect-8", "a58bd0ff4ec191c1"),
    ("tests/refused-0", "d0b1e6072d4a9f0c"),
    ("tests/refused-1", "740d131858ac4b80"),
    ("tests/refused-2", "a8a7ed621a47d192"),
    ("tests/refused-3", "c1ba2bc2d9d81c48"),
    ("tests/refused-4", "3d4d49ff70120da5"),
    ("tests/refused-5", "a1eafa718880b05f"),
    ("tests/refused-6", "c6b94294e682e3f1"),
    ("tests/refused-town-0", "07729682635dbf35"),
    ("tests/refused-town-1", "c07c0b3abcbbbda8"),
    ("tests/counts-0", "fb9352052b75511a"),
    ("tests/counts-1", "870ad55fc568bb98"),
    ("tests/counts-2", "17e5ed00e822bd58"),
    ("tests/counts-3", "e42c4374fa5376f5"),
    ("tests/counts-4", "6c5c3651caa7cb3b"),
    ("tests/counts-5", "6faae78a35241d62"),
    ("tests/counts-6", "896ca14274945734"),
    ("tests/actions-0", "b57d025b3f5657e9"),
    ("tests/actions-1", "68c21193dcbdc0f0"),
    ("tests/actions-2", "cc61d352ef00d8c9"),
    ("tests/actions-3", "77a81fdfb8249448"),
    ("tests/actions-4", "2c21c1c9eb0d6d24"),
    ("tests/actions-5", "ea18cf0803995e32"),
    ("tests/actions-6", "9990baaaaa69329c"),
    ("tests/actions-7", "4ba1cc704a54dc19"),
    ("tests/actions-8", "a1a8bd3ce50604e5"),
    ("tests/actions-9", "975395f0c407e2e8"),
    ("tests/actions-10", "af5a9f5be3bc5ade"),
    ("tests/changed-0", "6bded80c97e10d58"),
    ("tests/changed-1", "9945a78a4b6565f1"),
    ("tests/cost-0", "ce1604cfea47f290"),
    ("tests/cost-1", "51c54e9642d4dc16"),
    ("tests/cost-2", "1385ec8b0cbfecfd"),
    ("tests/cost-3", "aa6f2b7f2665f4ec"),
    ("tests/cost-4", "d8c00d83877bd73d"),
    ("tests/cost-5", "2a8c7142dffbcbff"),
    ("tests/cost-6", "13e8a470493f9215"),
    ("tests/cost-7", "5c8e5f745881e21b"),
    ("tests/cost-8", "05f6ad85d23a3e99"),
    ("tests/cost-non-spawn", "9e029fad0522438b"),
    ("tests/cost-in-list", "783a775c127ce069"),
    ("tests/cost-funds", "083def15f4e815eb"),
    ("generated/town-1000", "6ae8e79f147b95c6"),
    ("generated/wired-1000", "bcecca1773ab5d31"),
    ("generated/linked-1000", "451002d1099de0e4"),
    ("generated/build-1000", "dd5798c858091d22"),
    ("generated/crowded-300", "ed32465a227fb264"),
    ("limits/file", "3ffc4affb7df64a9"),
    ("limits/file-exactly", "0f69c91d687e73a4"),
    ("limits/key", "78397ee80fe45f26"),
    ("limits/key-exactly", "43b524fb7ab50752"),
    ("limits/prop-key", "e79ecfa1e9802530"),
    ("limits/counts-key", "c4aba6400c38bd10"),
    ("limits/name", "e3056cfa66ca25d8"),
    ("limits/name-exactly", "b828e9537418445e"),
    ("limits/ground", "b059eb8a5e5ce718"),
    ("limits/ground-exactly", "edcc2f98c620ec0a"),
    ("limits/ground-huge", "a2f5ebd052602889"),
    ("limits/map", "ef60709c614d90d1"),
    ("limits/covered", "7370d2d35c3d6f63"),
    ("limits/covered-adds-up", "6c04e38a1b5bed45"),
    ("limits/footprint-huge", "cac00b83c03b5b53"),
    ("limits/prefab-copies", "c47652eeff5fc160"),
    ("limits/prefab-copies-exactly", "82fa76c7155ce3ad"),
    ("limits/playtest", "28ff385dadef05ee"),
    ("limits/playtest-exactly", "91f4214313f9d5f4"),
    ("limits/entities", "5b251d46318bdd4c"),
    ("limits/everything", "17f91b2536fcec79"),
    ("limits/shapes", "2db1382040eda821"),
    ("limits/shapes-exactly", "0a53ccaba8860449"),
    ("limits/shapes-prefab", "3e63199cb3978abd"),
];

/// Every code the loader can report.
const LOADER_CODES: &[&str] = &[
    "E0001", "E0002", "E0101", "E0102", "E0103", "E0104", "E0105", "E0201", "E0202", "E0203", "E0204", "E0205",
    "E0206", "E0207", "E0208", "E0209", "W0201", "W0202", "W0203", "W0204", "W0205", "W0206", "W0207", "W0208",
    "W0209",
];

/// A case's record, and the codes of its check and of each playtest loaded
/// by path.
struct Recorded {
    name: String,
    record: String,
    codes: Vec<Vec<&'static str>>,
}

/// Every case recorded once, on several threads (some cases are as large as
/// the limits). Each record depends on its case alone.
fn recorded() -> &'static [Recorded] {
    static RECORDED: std::sync::OnceLock<Vec<Recorded>> = std::sync::OnceLock::new();
    RECORDED.get_or_init(|| {
        let cases = cases();
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let mut out: Vec<Option<Recorded>> = (0..cases.len()).map(|_| None).collect();
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..threads)
                .map(|t| {
                    let cases = &cases;
                    scope.spawn(move || {
                        (t..cases.len()).step_by(threads).map(|i| (i, record_one(&cases[i]))).collect::<Vec<_>>()
                    })
                })
                .collect();
            for w in workers {
                for (i, r) in w.join().expect("a case panicked") {
                    out[i] = Some(r);
                }
            }
        });
        out.into_iter().map(|r| r.expect("every case is recorded")).collect()
    })
}

fn record_one(c: &Case) -> Recorded {
    let mut codes = vec![check_project(&c.vfs).diagnostics.iter().map(|d| d.code).collect()];
    for p in &c.extra_playtests {
        codes.push(load_playtest(&c.vfs, p).1.iter().map(|d| d.code).collect());
    }
    Recorded { name: c.name.clone(), record: record(c), codes }
}

#[test]
fn every_pin_holds() {
    let all = recorded();
    let names: BTreeSet<&str> = all.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names.len(), all.len(), "two cases have the same name");
    let pinned: BTreeSet<&str> = PINS.iter().map(|(n, _)| *n).collect();
    assert_eq!(pinned.len(), PINS.len(), "a case is pinned twice");
    let mut wrong = Vec::new();
    for c in all {
        let got = record_hash(&c.record);
        match PINS.iter().find(|(n, _)| *n == c.name) {
            Some((_, want)) if *want == got => {}
            Some((_, want)) => wrong.push(format!("{}: pinned {want}, got {got}\n{}", c.name, c.record)),
            None => wrong.push(format!("{}: no pin (got {got})", c.name)),
        }
    }
    for name in &pinned {
        assert!(names.contains(name), "pin `{name}` names no case");
    }
    assert!(wrong.is_empty(), "{} of {} cases differ:\n{}", wrong.len(), all.len(), wrong.join("\n"));
}

#[test]
fn every_code_the_loader_reports_is_reached_alone_and_with_others() {
    let mut alone = BTreeSet::new();
    let mut with_others = BTreeSet::new();
    for c in recorded() {
        for codes in &c.codes {
            let distinct: BTreeSet<&str> = codes.iter().copied().collect();
            if distinct.len() == 1 {
                alone.extend(distinct.iter().copied());
            } else {
                with_others.extend(distinct.iter().copied());
            }
        }
    }
    for code in LOADER_CODES {
        assert!(alone.contains(code), "no case reports {code} alone");
        // `load_playtest` stops at a file that is not there: E0001 is alone.
        assert!(*code == "E0001" || with_others.contains(code), "no case reports {code} with other codes");
    }
    // Spread over the files of one project.
    let d = check_project(&everything()).diagnostics;
    let files: BTreeSet<&str> = d.iter().filter_map(|d| d.file.as_deref()).collect();
    let codes: BTreeSet<&str> = d.iter().map(|d| d.code).collect();
    assert!(files.len() >= 12 && codes.len() >= 18, "{} files, codes {codes:?}", files.len());
}

#[test]
fn the_base_project_checks_clean() {
    let r = check_project(&with(&[]));
    assert!(r.diagnostics.is_empty(), "{:#?}", r.diagnostics);
}

/// Prints the lines of [`PINS`]; with `CORPUS_OUT`, writes every record too.
#[test]
#[ignore]
fn print() {
    let mut all = String::new();
    for c in recorded() {
        println!("    ({:?}, {:?}),", c.name, record_hash(&c.record));
        let _ = writeln!(all, "== {}\n{}", c.name, c.record);
    }
    if let Some(path) = std::env::var_os("CORPUS_OUT") {
        std::fs::write(&path, all).expect("CORPUS_OUT is writable");
    }
}
