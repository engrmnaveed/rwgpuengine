//! The schema registry: one description of every file format, used for
//! validation (`slate check`), defaults (decoding), JSON Schema output
//! (`slate schema`) and reference docs (`slate docs`, `slate explain`).
//!
//! Adding a key to a format means adding one [`Field`] here and reading it in
//! `defs.rs`; validation, docs and schemas follow automatically.

use slate_core::codes;
use slate_core::color::Rgba;
use slate_core::diag::{Diagnostic, SourceFile, Span};
use slate_core::json::Json;
use slate_core::text::did_you_mean;

use crate::node::{Node, NodeKind, Table, parse_toml};

/// The type of a value in a slate text file.
#[derive(Debug)]
pub enum Ty {
    Bool,
    Int {
        min: i64,
        max: i64,
    },
    Num {
        min: f64,
        max: f64,
    },
    Str,
    /// Letters, digits, `_`, `-` and `/` (prefab subfolders).
    Name,
    /// Project-relative file path with `/` separators.
    Path,
    /// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`.
    Color,
    Enum(&'static [&'static str]),
    /// `[x, y]` numbers.
    Vec2,
    /// `[x, y]` or `[x, y, z]` numbers.
    Pos,
    /// `[x, y]` integers.
    Cell,
    /// Non-empty array of cells.
    Cells,
    /// Table of scalar values (free-form game data).
    Props,
    /// List of warning codes, e.g. `["W0201"]`.
    Codes,
    Table(&'static Spec),
    /// Array of tables (`[[name]]` sections).
    Tables(&'static Spec),
}

/// One key of a table.
#[derive(Debug)]
pub struct Field {
    pub name: &'static str,
    pub ty: Ty,
    pub required: bool,
    /// Default as a TOML literal (`1.0`, `"#ffffff"`, `[0, 0]`). `None` when
    /// there is no default or it is computed (the doc then says how).
    pub default: Option<&'static str>,
    pub doc: &'static str,
}

/// A table format: a file root, a section, or a component.
#[derive(Debug)]
pub struct Spec {
    /// Stable identifier used by `slate explain` and schema file names.
    pub id: &'static str,
    /// How the table appears in messages: `entity`, `[ground]`, `sprite`.
    pub title: &'static str,
    pub doc: &'static str,
    pub fields: &'static [Field],
}

impl Spec {
    pub fn field(&self, name: &str) -> Option<&'static Field> {
        self.fields.iter().find(|f| f.name == name)
    }
}

const fn opt(name: &'static str, ty: Ty, default: Option<&'static str>, doc: &'static str) -> Field {
    Field { name, ty, required: false, default, doc }
}

const fn req(name: &'static str, ty: Ty, doc: &'static str) -> Field {
    Field { name, ty, required: true, default: None, doc }
}

const U16: Ty = Ty::Int { min: 1, max: 4096 };
const FRAMES: Ty = Ty::Int { min: 1, max: 1_000_000 };
const FRAME: Ty = Ty::Int { min: 0, max: 1_000_000 };
const SEED: Ty = Ty::Int { min: 0, max: i64::MAX };

/// Abstract input actions. Arrow keys / WASD map to the directions on the web.
pub const ACTIONS: &[&str] = &["up", "down", "left", "right", "confirm", "cancel"];
/// What a `[[connect]]` can do to its target.
pub const CONNECT_ACTIONS: &[&str] = &["enable", "disable", "toggle"];
/// Kinds of entries in the event log.
pub const EVENT_KINDS: &[&str] =
    &["input", "tap", "tap_miss", "moved", "blocked", "trigger", "signal", "action", "error", "custom"];
pub const SPRITE_SHAPES: &[&str] = &["circle", "rect", "diamond"];
pub const PATROL_MODES: &[&str] = &["loop", "pingpong"];
/// Why a move was refused (the `reason` of `blocked` events).
pub const BLOCK_REASONS: &[&str] = &["solid", "actor", "edge", "area", "cooldown"];

// ---------------------------------------------------------------------------
// Project file: slate.toml

pub static PROJECT_FILE: Spec = Spec {
    id: "project",
    title: "slate.toml",
    doc: "Project settings. Its folder is the project root; every path in the project is relative to it.",
    fields: &[
        req("project", Ty::Table(&PROJECT), "Name and entry scene."),
        opt("display", Ty::Table(&DISPLAY), None, "Output size and background."),
        opt("iso", Ty::Table(&ISO), None, "Isometric grid geometry."),
        opt("sim", Ty::Table(&SIM), None, "Simulation rate and seed."),
    ],
};

pub static PROJECT: Spec = Spec {
    id: "project.project",
    title: "[project]",
    doc: "Project identity.",
    fields: &[
        req("name", Ty::Str, "Project name."),
        opt(
            "engine",
            Ty::Str,
            None,
            "slate version the project targets, e.g. \"0.1\". `check` warns (W0205) if the running slate differs.",
        ),
        opt("main_scene", Ty::Path, Some("\"scenes/main.toml\""), "Scene used when a command is given no scene."),
    ],
};

pub static DISPLAY: Spec = Spec {
    id: "project.display",
    title: "[display]",
    doc: "The rendered frame. Screenshots and the web player use exactly this size.",
    fields: &[
        opt("width", U16, Some("640"), "Frame width in pixels."),
        opt("height", U16, Some("480"), "Frame height in pixels."),
        opt("background", Ty::Color, Some("\"#1d2330\""), "Color behind everything."),
    ],
};

pub static ISO: Spec = Spec {
    id: "project.iso",
    title: "[iso]",
    doc: "Isometric projection. A cell's diamond is tile_width × tile_height pixels; one unit of height is z_height pixels.",
    fields: &[
        opt(
            "tile_width",
            Ty::Int { min: 4, max: 1024 },
            Some("64"),
            "Diamond width in pixels. Usually 2 × tile_height.",
        ),
        opt("tile_height", Ty::Int { min: 2, max: 512 }, Some("32"), "Diamond height in pixels."),
        opt("z_height", Ty::Int { min: 1, max: 1024 }, Some("32"), "Pixels per unit of height (z, block height)."),
    ],
};

pub static SIM: Spec = Spec {
    id: "project.sim",
    title: "[sim]",
    doc: "The fixed-step simulation. A frame is one simulation step.",
    fields: &[
        opt("tick_rate", Ty::Int { min: 1, max: 1000 }, Some("60"), "Steps per second."),
        opt("seed", SEED, Some("1"), "Seed for all randomness. Playtests and `--seed` can override it."),
    ],
};

// ---------------------------------------------------------------------------
// Scene file: scenes/*.toml

pub static SCENE_FILE: Spec = Spec {
    id: "scene",
    title: "scene file",
    doc: "A level: ground, entities and the connections between their signals.",
    fields: &[
        opt("scene", Ty::Table(&SCENE), None, "Scene metadata."),
        opt("camera", Ty::Table(&CAMERA), None, "What part of the world is on screen."),
        opt("ground", Ty::Table(&GROUND), None, "A rectangle of floor cells; also the bounds movers stay inside."),
        opt("entity", Ty::Tables(&ENTITY), None, "Things in the world, one `[[entity]]` table each."),
        opt("connect", Ty::Tables(&CONNECT), None, "Signal wiring, one `[[connect]]` table each."),
    ],
};

pub static SCENE: Spec = Spec {
    id: "scene.scene",
    title: "[scene]",
    doc: "Scene metadata.",
    fields: &[
        opt("name", Ty::Str, None, "Display name. Default: the file name."),
        opt("doc", Ty::Str, None, "What the scene is for."),
        opt("seed", SEED, None, "Overrides the project seed for this scene."),
    ],
};

pub static CAMERA: Spec = Spec {
    id: "scene.camera",
    title: "[camera]",
    doc: "The camera looks straight down the isometric axis; it only pans.",
    fields: &[
        opt(
            "center",
            Ty::Vec2,
            None,
            "Grid point [x, y] at the middle of the frame. Default: the middle of the ground.",
        ),
        opt("offset", Ty::Vec2, Some("[0, 0]"), "Extra shift in screen pixels [right, down]."),
    ],
};

pub static GROUND: Spec = Spec {
    id: "scene.ground",
    title: "[ground]",
    doc: "Floor cells from [0, 0] to [size_x - 1, size_y - 1], drawn as a checkerboard.",
    fields: &[
        req("size", Ty::Cell, "Number of cells [x, y]."),
        opt("color", Ty::Color, Some("\"#6fa86a\""), "Floor color."),
        opt("alt_color", Ty::Color, None, "Color of every other cell. Default: `color`, slightly darker."),
    ],
};

pub static ENTITY: Spec = Spec {
    id: "entity",
    title: "entity",
    doc: "A thing in the world. Starts from an optional prefab; keys here override it (component tables merge key by key).",
    fields: &[
        opt(
            "name",
            Ty::Name,
            None,
            "Unique name, needed to reference it from [[connect]], playtests and `inspect --entity`.",
        ),
        opt("prefab", Ty::Name, None, "Prefab to start from: `wall` loads prefabs/wall.toml."),
        opt(
            "pos",
            Ty::Pos,
            Some("[0, 0, 0]"),
            "Grid position [x, y] or [x, y, z]. +x is down-right on screen, +y is down-left, z is up.",
        ),
        opt(
            "cells",
            Ty::Cells,
            None,
            "Place one copy on each listed cell instead of `pos`. Cannot be used with `name`.",
        ),
        F_ENABLED,
        F_SOLID,
        F_TILE,
        F_BLOCK,
        F_SPRITE,
        F_MOVER,
        F_PATROL,
        F_WANDER,
        F_TRIGGER,
        F_PROPS,
        F_ALLOW,
    ],
};

pub static PREFAB_FILE: Spec = Spec {
    id: "prefab",
    title: "prefab file",
    doc: "A reusable entity template: prefabs/<name>.toml. Same keys as an entity, minus name/prefab/pos/cells.",
    fields: &[
        opt("doc", Ty::Str, None, "What the prefab is."),
        F_ENABLED,
        F_SOLID,
        F_TILE,
        F_BLOCK,
        F_SPRITE,
        F_MOVER,
        F_PATROL,
        F_WANDER,
        F_TRIGGER,
        F_PROPS,
        F_ALLOW,
    ],
};

const F_ENABLED: Field = opt(
    "enabled",
    Ty::Bool,
    Some("true"),
    "Disabled entities are not drawn, not solid, cannot be tapped and run no behaviors.",
);
const F_SOLID: Field = opt("solid", Ty::Bool, Some("false"), "Nothing can move into its cell.");
const F_TILE: Field = opt("tile", Ty::Table(&TILE), None, "Flat diamond on the floor (marks, pressure plates).");
const F_BLOCK: Field = opt("block", Ty::Table(&BLOCK), None, "Isometric box (walls, crates, buildings).");
const F_SPRITE: Field =
    opt("sprite", Ty::Table(&SPRITE), None, "Upright shape standing on the cell (characters, items).");
const F_MOVER: Field = opt("mover", Ty::Table(&MOVER), None, "Moves one cell per direction input.");
const F_PATROL: Field = opt("patrol", Ty::Table(&PATROL), None, "Walks a fixed path of cells.");
const F_WANDER: Field = opt("wander", Ty::Table(&WANDER), None, "Walks randomly (seeded).");
const F_TRIGGER: Field =
    opt("trigger", Ty::Table(&TRIGGER), None, "Emits signals when actors step on or off its cell.");
const F_PROPS: Field = opt(
    "props",
    Ty::Props,
    None,
    "Free-form game data shown by `inspect` and checked by `[[expect]] props`. Nothing in data files changes \
     props at runtime (Rust game code can).",
);
const F_ALLOW: Field = opt(
    "allow",
    Ty::Codes,
    None,
    "Warning codes that are intended for this entity and should not be reported, e.g. `[\"W0201\"]` for \
     deliberately stacked solids. Errors cannot be allowed.",
);

pub static TILE: Spec = Spec {
    id: "tile",
    title: "tile",
    doc: "A flat diamond drawn on the floor at the entity's cell, above the ground.",
    fields: &[opt("color", Ty::Color, Some("\"#c9b26b\""), "Fill color.")],
};

pub static BLOCK: Spec = Spec {
    id: "block",
    title: "block",
    doc: "An isometric box filling the entity's cell. Left and right faces are shaded automatically.",
    fields: &[
        opt("color", Ty::Color, Some("\"#9aa0a6\""), "Top face color."),
        opt("height", Ty::Num { min: 0.0, max: 64.0 }, Some("1.0"), "Height in z units."),
    ],
};

pub static SPRITE: Spec = Spec {
    id: "sprite",
    title: "sprite",
    doc: "An upright shape whose bottom edge stands on the middle of the entity's cell.",
    fields: &[
        opt("shape", Ty::Enum(SPRITE_SHAPES), Some("\"circle\""), "circle (ellipse filling size), rect or diamond."),
        opt("color", Ty::Color, Some("\"#ffffff\""), "Fill color."),
        opt("size", Ty::Vec2, Some("[24, 32]"), "Width and height in pixels."),
        opt("offset", Ty::Vec2, Some("[0, 0]"), "Shift in pixels [right, down] from the standing point."),
    ],
};

pub static MOVER: Spec = Spec {
    id: "mover",
    title: "mover",
    doc: "Moves one cell per `up`/`down`/`left`/`right` input: up = -y, down = +y, left = -x, right = +x. \
          Every enabled mover responds to the same input. \
          A press or hold moves on its first frame if the mover is ready, then every `step_frames` while held. \
          Input during the wait is not queued: a press is dropped (logged as `blocked`, reason `cooldown`); a hold \
          moves as soon as the wait ends. A refused move also starts the wait, so a held direction retries every \
          `step_frames`. Solids, other actors and the ground's edge also block, each logged as \
          a `blocked` event with its reason.",
    fields: &[opt(
        "step_frames",
        Ty::Int { min: 1, max: 600 },
        Some("8"),
        "Frames from one move to the next. `hold` for `step_frames × N` frames gives exactly N steps.",
    )],
};

pub static PATROL: Spec = Spec {
    id: "patrol",
    title: "patrol",
    doc: "Walks `path` one cell at a time (x first, then y), starting from its `pos` toward `path[0]` (or \
          `path[1]` if it starts on `path[0]`). The k-th step attempt happens in step k × step_frames − 1, so it is \
          visible from frame k × step_frames. A blocked attempt is retried at the next attempt time.",
    fields: &[
        req("path", Ty::Cells, "Cells to visit, in order."),
        opt("step_frames", Ty::Int { min: 1, max: 6000 }, Some("30"), "Frames between step attempts."),
        opt(
            "mode",
            Ty::Enum(PATROL_MODES),
            Some("\"loop\""),
            "loop: back to the first cell; pingpong: walk the path backwards.",
        ),
    ],
};

pub static WANDER: Spec = Spec {
    id: "wander",
    title: "wander",
    doc: "Every `step_frames` (attempt k in step k × step_frames − 1, like patrol), draws one number from the \
          world's seeded RNG: stay, or try one step up/down/left/right. Runs with the same seed repeat exactly.",
    fields: &[
        opt("step_frames", Ty::Int { min: 1, max: 6000 }, Some("45"), "Frames between steps."),
        opt(
            "area",
            Ty::Cells,
            None,
            "Two corner cells [[x0, y0], [x1, y1]] to stay inside. Default: the whole ground.",
        ),
    ],
};

pub static TRIGGER: Spec = Spec {
    id: "trigger",
    title: "trigger",
    doc: "Watches its own cell. Emits `on_enter` when the first actor (an entity with mover, patrol or wander) arrives \
          and `on_exit` when the last one leaves.",
    fields: &[
        opt("on_enter", Ty::Name, Some("\"pressed\""), "Signal emitted when the cell becomes occupied."),
        opt("on_exit", Ty::Name, Some("\"released\""), "Signal emitted when the cell becomes empty."),
        opt("only", Ty::Name, None, "Only this entity (by name) counts. Default: any actor."),
    ],
};

pub static CONNECT: Spec = Spec {
    id: "connect",
    title: "[[connect]]",
    doc: "When entity `from` emits `signal`, apply `action` to entity `to`. Every entity emits `tapped` when tapped.",
    fields: &[
        req("from", Ty::Name, "Name of the emitting entity."),
        req("signal", Ty::Name, "Signal name, e.g. `pressed`, `released`, `tapped`."),
        req("to", Ty::Name, "Name of the target entity."),
        req("action", Ty::Enum(CONNECT_ACTIONS), "enable, disable or toggle the target."),
    ],
};

// ---------------------------------------------------------------------------
// Playtest file: tests/*.toml

pub static PLAYTEST_FILE: Spec = Spec {
    id: "playtest",
    title: "playtest file",
    doc: "Scripted input plus expectations, run by `slate test`. `run`, `shot` and `inspect` accept it as `--input`.",
    fields: &[
        opt("test", Ty::Table(&TEST), None, "Which scene, how long, which seed."),
        opt("input", Ty::Tables(&INPUT), None, "Scripted input, one `[[input]]` per event."),
        opt("expect", Ty::Tables(&EXPECT), None, "Checks on world state or the event log."),
        opt("shot", Ty::Tables(&SHOT), None, "Screenshots compared with reference images."),
    ],
};

pub static TEST: Spec = Spec {
    id: "playtest.test",
    title: "[test]",
    doc: "Playtest settings.",
    fields: &[
        opt("doc", Ty::Str, None, "What the playtest checks."),
        opt("scene", Ty::Path, None, "Scene to run. Default: the project's main_scene."),
        opt("frames", FRAMES, Some("300"), "Number of frames (steps) to run."),
        opt("seed", SEED, None, "Overrides the project and scene seed."),
    ],
};

pub static INPUT: Spec = Spec {
    id: "playtest.input",
    title: "[[input]]",
    doc: "One input event. Use exactly one of `press`, `hold` or `tap`. Input at frame N is applied by step N \
          (the step from frame N to N+1).",
    fields: &[
        req("frame", FRAME, "Frame at which the input happens."),
        opt("press", Ty::Enum(ACTIONS), None, "Press and release an action within the frame."),
        opt("hold", Ty::Enum(ACTIONS), None, "Hold an action down for `for` frames."),
        opt("for", Ty::Int { min: 1, max: 1_000_000 }, Some("1"), "With `hold`: number of frames to hold."),
        opt("tap", Ty::Vec2, None, "Tap the screen at pixel [x, y]; hits the topmost enabled entity drawn there."),
    ],
};

pub static EXPECT: Spec = Spec {
    id: "playtest.expect",
    title: "[[expect]]",
    doc: "A check at `frame`. Either entity state (`entity` with `pos`/`enabled`/`solid`/`props`) or the event log \
          (`event`, filtered by `entity`, `other`, `signal`, `reason`, counted with `at_least`/`at_most`). Events \
          are counted if they happened *before* `frame`: an event logged as `[N]` happened in step N and counts \
          from `frame = N + 1`. `entity` matches the event's subject and `other` its other party \
          (`slate explain events` lists both for every kind).",
    fields: &[
        opt("frame", FRAME, None, "When to check. Default: the last frame."),
        opt("entity", Ty::Name, None, "Entity to check, by name."),
        opt("pos", Ty::Pos, None, "Expected position."),
        opt("enabled", Ty::Bool, None, "Expected enabled state."),
        opt("solid", Ty::Bool, None, "Expected solid state."),
        opt("props", Ty::Props, None, "Expected values of these props (others are ignored)."),
        opt("event", Ty::Enum(EVENT_KINDS), None, "Count events of this kind that happened before `frame`."),
        opt(
            "other",
            Ty::Name,
            None,
            "With `event`: only events whose other party is this entity (the blocker of `blocked`, the actor of \
             `trigger`, the emitter of `action`).",
        ),
        opt("signal", Ty::Name, None, "With `event`: only count events about this signal."),
        opt(
            "reason",
            Ty::Enum(BLOCK_REASONS),
            None,
            "With `event = \"blocked\"`: only count refusals for this reason (e.g. `solid`, not `cooldown`).",
        ),
        opt(
            "at_least",
            Ty::Int { min: 0, max: 1_000_000 },
            Some("1"),
            "With `event`: minimum number of matching events (0 when only `at_most` is given).",
        ),
        opt(
            "at_most",
            Ty::Int { min: 0, max: 1_000_000 },
            None,
            "With `event`: maximum number of matching events. `at_most = 0` asserts it never happened.",
        ),
    ],
};

pub static SHOT: Spec = Spec {
    id: "playtest.shot",
    title: "[[shot]]",
    doc: "Render a frame and compare it with a reference PNG. Create or refresh references with `slate test --update-refs`.",
    fields: &[
        opt("frame", FRAME, None, "Frame to render. Default: the last frame."),
        req("reference", Ty::Path, "Reference PNG, relative to the project root (e.g. tests/refs/smoke.png)."),
        opt("max_diff_pixels", Ty::Int { min: 0, max: i64::MAX }, Some("0"), "Pixels allowed to differ."),
    ],
};

/// Every spec, for `slate explain`, docs and schema export.
pub static ALL_SPECS: &[&Spec] = &[
    &PROJECT_FILE,
    &PROJECT,
    &DISPLAY,
    &ISO,
    &SIM,
    &SCENE_FILE,
    &SCENE,
    &CAMERA,
    &GROUND,
    &ENTITY,
    &CONNECT,
    &PREFAB_FILE,
    &TILE,
    &BLOCK,
    &SPRITE,
    &MOVER,
    &PATROL,
    &WANDER,
    &TRIGGER,
    &PLAYTEST_FILE,
    &TEST,
    &INPUT,
    &EXPECT,
    &SHOT,
];

/// File-level specs, exported as JSON Schemas.
pub static FILE_SPECS: &[&Spec] = &[&PROJECT_FILE, &SCENE_FILE, &PREFAB_FILE, &PLAYTEST_FILE];

/// Component specs (tables an entity can carry).
pub static COMPONENT_SPECS: &[&Spec] = &[&TILE, &BLOCK, &SPRITE, &MOVER, &PATROL, &WANDER, &TRIGGER];

/// Find a spec by id or title (`sprite`, `entity`, `[ground]`, `ground`).
pub fn find_spec(name: &str) -> Option<&'static Spec> {
    let bare = name.trim_matches(['[', ']']);
    ALL_SPECS.iter().copied().find(|s| {
        s.id == name
            || s.title == name
            || s.title.trim_matches(['[', ']']) == bare
            || s.id.rsplit('.').next() == Some(bare)
    })
}

// ---------------------------------------------------------------------------
// Type descriptions

impl Ty {
    /// Plain-English description used in messages and docs.
    pub fn describe(&self) -> String {
        match self {
            Ty::Bool => "a boolean (true or false)".into(),
            Ty::Int { min, max } => describe_range("an integer", *min as f64, *max as f64, i64::MAX as f64),
            Ty::Num { min, max } => describe_range("a number", *min, *max, f64::MAX),
            Ty::Str => "a string".into(),
            Ty::Name => "a name (letters, digits, `_`, `-`, `/`)".into(),
            Ty::Path => "a project-relative file path like \"scenes/main.toml\"".into(),
            Ty::Color => "a hex color like \"#ff8800\"".into(),
            Ty::Enum(values) => {
                let list: Vec<String> = values.iter().map(|v| format!("\"{v}\"")).collect();
                format!("one of {}", list.join(", "))
            }
            Ty::Vec2 => "two numbers [x, y]".into(),
            Ty::Pos => "[x, y] or [x, y, z] numbers".into(),
            Ty::Cell => "two integers [x, y]".into(),
            Ty::Cells => "a list of cells like [[0, 0], [1, 0]]".into(),
            Ty::Props => "a table of simple values like { hp = 3 }".into(),
            Ty::Codes => "a list of warning codes like [\"W0201\"]".into(),
            Ty::Table(spec) => format!("a table ({})", spec.title),
            Ty::Tables(spec) => format!("an array of tables ({})", spec.title),
        }
    }

    /// Short type label for reference tables.
    pub fn label(&self) -> String {
        match self {
            Ty::Bool => "bool".into(),
            Ty::Int { .. } => "integer".into(),
            Ty::Num { .. } => "number".into(),
            Ty::Str => "string".into(),
            Ty::Name => "name".into(),
            Ty::Path => "path".into(),
            Ty::Color => "color".into(),
            Ty::Enum(values) => values.join(" \\| "),
            Ty::Vec2 => "[x, y]".into(),
            Ty::Pos => "[x, y, z?]".into(),
            Ty::Cell => "[x, y] int".into(),
            Ty::Cells => "[[x, y], ...]".into(),
            Ty::Props => "table".into(),
            Ty::Codes => "[\"W....\", ...]".into(),
            Ty::Table(spec) => format!("table: {}", spec.id),
            Ty::Tables(spec) => format!("[[{}]]", spec.title.trim_matches(['[', ']'])),
        }
    }
}

fn describe_range(base: &str, min: f64, max: f64, unbounded: f64) -> String {
    let fmt = |v: f64| {
        if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{v}") }
    };
    if max >= unbounded {
        format!("{base} ≥ {}", fmt(min))
    } else {
        format!("{base} from {} to {}", fmt(min), fmt(max))
    }
}

// ---------------------------------------------------------------------------
// Validation

/// Validates parsed tables against specs, collecting diagnostics.
pub struct Checker<'a> {
    pub src: &'a SourceFile,
    pub diags: &'a mut Vec<Diagnostic>,
}

impl Checker<'_> {
    fn error(&mut self, d: Diagnostic) {
        self.diags.push(d);
    }

    /// Check every key of `t` against `spec`. Returns false if any error was
    /// reported. `span` locates the table itself (for missing keys).
    pub fn table(&mut self, t: &Table, spec: &Spec, span: Span) -> bool {
        let mut ok = true;
        for e in &t.entries {
            match spec.field(&e.key) {
                None => {
                    ok = false;
                    let names = spec.fields.iter().map(|f| f.name);
                    let hint = did_you_mean(&e.key, names).unwrap_or_else(|| {
                        let all: Vec<&str> = spec.fields.iter().map(|f| f.name).collect();
                        format!("valid keys in {}: {}", spec.title, all.join(", "))
                    });
                    self.error(
                        Diagnostic::new(&codes::UNKNOWN_KEY, format!("unknown key `{}` in {}", e.key, spec.title))
                            .at(self.src, e.key_span)
                            .hint(hint),
                    );
                }
                Some(field) => ok &= self.value(&e.value, &field.ty, &e.key),
            }
        }
        for f in spec.fields.iter().filter(|f| f.required) {
            if !t.has(f.name) {
                ok = false;
                self.error(
                    Diagnostic::new(
                        &codes::MISSING_KEY,
                        format!("{} is missing required key `{}`", spec.title, f.name),
                    )
                    .at(self.src, span)
                    .hint(format!("add `{} = ...` ({}): {}", f.name, f.ty.describe(), f.doc)),
                );
            }
        }
        ok
    }

    /// Check one value. `label` names it in messages (e.g. `size`).
    pub fn value(&mut self, node: &Node, ty: &Ty, label: &str) -> bool {
        let wrong_type = |c: &mut Self| {
            c.error(
                Diagnostic::new(
                    &codes::WRONG_TYPE,
                    format!("`{label}` must be {}, found {} {}", ty.describe(), node.type_name(), node.preview()),
                )
                .at(c.src, node.span),
            );
            false
        };
        let invalid = |c: &mut Self, why: String, hint: Option<String>| {
            c.error(Diagnostic::new(&codes::INVALID_VALUE, why).at(c.src, node.span).maybe_hint(hint));
            false
        };
        match ty {
            Ty::Bool => node.as_bool().is_some() || wrong_type(self),
            Ty::Int { min, max } => match node.as_int() {
                None => wrong_type(self),
                Some(v) if v < *min || v > *max => {
                    invalid(self, format!("`{label}` must be {}, found {v}", ty.describe()), None)
                }
                Some(_) => true,
            },
            Ty::Num { min, max } => match node.as_num() {
                None => wrong_type(self),
                Some(v) if v < *min || v > *max || !v.is_finite() => {
                    invalid(self, format!("`{label}` must be {}, found {v}", ty.describe()), None)
                }
                Some(_) => true,
            },
            Ty::Str => node.as_str().is_some() || wrong_type(self),
            Ty::Name => match node.as_str() {
                None => wrong_type(self),
                Some(s) if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || "_-/".contains(c)) => {
                    invalid(self, format!("`{label}` must be {}, found {s:?}", ty.describe()), None)
                }
                Some(_) => true,
            },
            Ty::Path => match node.as_str() {
                None => wrong_type(self),
                Some(s) if s.is_empty() || s.starts_with('/') || s.contains('\\') || s.contains(':') => invalid(
                    self,
                    format!("`{label}` must be {}, found {s:?}", ty.describe()),
                    Some("paths are relative to the project root and use `/`".into()),
                ),
                Some(_) => true,
            },
            Ty::Color => match node.as_str() {
                None => wrong_type(self),
                Some(s) if Rgba::parse_hex(s).is_none() => invalid(
                    self,
                    format!("`{label}` must be {}, found {s:?}", ty.describe()),
                    Some("use #rgb, #rrggbb or #rrggbbaa, for example \"#e0a030\"".into()),
                ),
                Some(_) => true,
            },
            Ty::Enum(values) => match node.as_str() {
                None => wrong_type(self),
                Some(s) if !values.contains(&s) => invalid(
                    self,
                    format!("`{label}` must be {}, found {s:?}", ty.describe()),
                    did_you_mean(s, values.iter().copied()),
                ),
                Some(_) => true,
            },
            Ty::Vec2 | Ty::Pos => match node.as_array() {
                Some(items)
                    if (items.len() == 2 || (items.len() == 3 && matches!(ty, Ty::Pos)))
                        && items.iter().all(|n| n.as_num().is_some_and(f64::is_finite)) =>
                {
                    true
                }
                _ => wrong_type(self),
            },
            Ty::Cell => match node.as_array() {
                Some([a, b]) if a.as_int().is_some() && b.as_int().is_some() => true,
                _ => wrong_type(self),
            },
            Ty::Cells => match node.as_array() {
                Some(items) if !items.is_empty() => {
                    let mut ok = true;
                    for item in items {
                        ok &= self.value(item, &Ty::Cell, label);
                    }
                    ok
                }
                _ => wrong_type(self),
            },
            Ty::Codes => match node.as_array() {
                None => wrong_type(self),
                Some(items) => {
                    let mut ok = true;
                    for item in items {
                        let Some(s) = item.as_str() else {
                            ok &= self.value(item, &Ty::Str, label);
                            continue;
                        };
                        let known = codes::lookup(s).filter(|c| c.severity == slate_core::diag::Severity::Warning);
                        if known.is_none() {
                            ok = false;
                            let warnings = codes::ALL
                                .iter()
                                .filter(|c| c.severity == slate_core::diag::Severity::Warning)
                                .map(|c| c.code);
                            let hint = did_you_mean(s, warnings).unwrap_or_else(|| {
                                "only warning codes (W....) can be allowed; `slate explain` lists them".into()
                            });
                            self.error(
                                Diagnostic::new(
                                    &codes::INVALID_VALUE,
                                    format!("`{label}` can only list warning codes; {s:?} is not one"),
                                )
                                .at(self.src, item.span)
                                .hint(hint),
                            );
                        }
                    }
                    ok
                }
            },
            Ty::Props => match node.as_table() {
                None => wrong_type(self),
                Some(t) => {
                    let mut ok = true;
                    for e in &t.entries {
                        if matches!(e.value.kind, NodeKind::Array(_) | NodeKind::Table(_)) {
                            ok = false;
                            self.error(
                                Diagnostic::new(
                                    &codes::WRONG_TYPE,
                                    format!(
                                        "`{label}.{}` must be a boolean, number or string, found {}",
                                        e.key,
                                        e.value.type_name()
                                    ),
                                )
                                .at(self.src, e.value.span),
                            );
                        }
                    }
                    ok
                }
            },
            Ty::Table(spec) => match node.as_table() {
                None => wrong_type(self),
                Some(t) => self.table(t, spec, node.span),
            },
            Ty::Tables(spec) => match node.as_array() {
                None => wrong_type(self),
                Some(items) => {
                    let mut ok = true;
                    for item in items {
                        match item.as_table() {
                            Some(t) => ok &= self.table(t, spec, item.span),
                            None => ok &= self.value(item, &Ty::Table(spec), label),
                        }
                    }
                    ok
                }
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Typed access with defaults

/// Reads a validated table, filling in defaults from the spec.
pub struct Fields<'a> {
    pub table: &'a Table,
    pub spec: &'static Spec,
}

impl<'a> Fields<'a> {
    pub fn new(table: &'a Table, spec: &'static Spec) -> Fields<'a> {
        Fields { table, spec }
    }

    /// The value, or the spec default parsed from its TOML literal.
    pub fn node(&self, key: &str) -> Option<Node> {
        if let Some(n) = self.table.get(key) {
            return Some(n.clone());
        }
        let field = self.spec.field(key).unwrap_or_else(|| panic!("{} has no field `{key}`", self.spec.id));
        field.default.map(parse_default)
    }

    pub fn has(&self, key: &str) -> bool {
        self.table.has(key)
    }

    pub fn bool(&self, key: &str) -> bool {
        self.node(key).and_then(|n| n.as_bool()).unwrap_or(false)
    }

    pub fn int(&self, key: &str) -> i64 {
        self.opt_int(key).unwrap_or(0)
    }

    pub fn opt_int(&self, key: &str) -> Option<i64> {
        self.node(key).and_then(|n| n.as_int())
    }

    pub fn num(&self, key: &str) -> f64 {
        self.node(key).and_then(|n| n.as_num()).unwrap_or(0.0)
    }

    pub fn opt_str(&self, key: &str) -> Option<String> {
        self.node(key).and_then(|n| n.as_str().map(str::to_string))
    }

    pub fn str(&self, key: &str) -> String {
        self.opt_str(key).unwrap_or_default()
    }

    pub fn opt_color(&self, key: &str) -> Option<Rgba> {
        self.node(key).and_then(|n| n.as_str().and_then(Rgba::parse_hex))
    }

    pub fn color(&self, key: &str) -> Rgba {
        self.opt_color(key).unwrap_or(Rgba::WHITE)
    }

    pub fn opt_vec2(&self, key: &str) -> Option<[f32; 2]> {
        let n = self.node(key)?;
        let items = n.as_array()?;
        Some([items.first()?.as_num()? as f32, items.get(1)?.as_num()? as f32])
    }

    pub fn vec2(&self, key: &str) -> [f32; 2] {
        self.opt_vec2(key).unwrap_or([0.0, 0.0])
    }

    pub fn opt_pos(&self, key: &str) -> Option<[f32; 3]> {
        let n = self.node(key)?;
        node_pos(&n)
    }

    pub fn cell(&self, key: &str) -> [i32; 2] {
        self.node(key).and_then(|n| node_cell(&n)).unwrap_or([0, 0])
    }

    /// A list of strings (missing or malformed items are skipped).
    pub fn strs(&self, key: &str) -> Vec<String> {
        self.node(key)
            .and_then(|n| {
                n.as_array().map(|items| items.iter().filter_map(|i| i.as_str().map(str::to_string)).collect())
            })
            .unwrap_or_default()
    }

    pub fn cells(&self, key: &str) -> Vec<[i32; 2]> {
        self.node(key)
            .and_then(|n| n.as_array().map(|items| items.iter().filter_map(node_cell).collect()))
            .unwrap_or_default()
    }

    pub fn table(&self, key: &str) -> Option<&'a Table> {
        self.table.table(key)
    }
}

/// `[x, y]` or `[x, y, z]` as a position (z defaults to 0).
pub fn node_pos(n: &Node) -> Option<[f32; 3]> {
    let items = n.as_array()?;
    let x = items.first()?.as_num()? as f32;
    let y = items.get(1)?.as_num()? as f32;
    let z = items.get(2).and_then(Node::as_num).unwrap_or(0.0) as f32;
    Some([x, y, z])
}

pub fn node_cell(n: &Node) -> Option<[i32; 2]> {
    match n.as_array()? {
        [a, b] => Some([a.as_int()? as i32, b.as_int()? as i32]),
        _ => None,
    }
}

fn parse_default(literal: &str) -> Node {
    let src = SourceFile::new("<default>", format!("v = {literal}"));
    let t = parse_toml(&src).unwrap_or_else(|_| panic!("bad default literal {literal}"));
    t.get("v").cloned().expect("default value")
}

// ---------------------------------------------------------------------------
// JSON Schema export

/// JSON Schema (draft 2020-12) for a file-level spec. Editors such as VS Code
/// (Even Better TOML) use it for completion and inline errors.
pub fn json_schema(spec: &Spec) -> Json {
    let mut root = table_schema(spec);
    if let Json::Obj(entries) = &mut root {
        entries.insert(0, ("$schema".into(), "https://json-schema.org/draft/2020-12/schema".into()));
        entries.insert(1, ("$id".into(), format!("https://slate.dev/schema/{}.json", spec.id).into()));
        entries.insert(2, ("title".into(), format!("slate {}", spec.title).into()));
    }
    root
}

fn table_schema(spec: &Spec) -> Json {
    let mut props = Json::object();
    for f in spec.fields {
        let mut s = ty_schema(&f.ty);
        let mut desc = f.doc.to_string();
        if let Some(d) = f.default {
            desc.push_str(&format!(" Default: {d}."));
        }
        s.set("description", desc);
        props.set(f.name, s);
    }
    let required: Vec<Json> = spec.fields.iter().filter(|f| f.required).map(|f| Json::from(f.name)).collect();
    let mut j = Json::object().with("type", "object").with("description", spec.doc).with("properties", props);
    if !required.is_empty() {
        j.set("required", required);
    }
    j.with("additionalProperties", false)
}

fn ty_schema(ty: &Ty) -> Json {
    let num_array = |item: &str, min: usize, max: usize| {
        Json::object()
            .with("type", "array")
            .with("items", Json::object().with("type", item))
            .with("minItems", min)
            .with("maxItems", max)
    };
    match ty {
        Ty::Bool => Json::object().with("type", "boolean"),
        Ty::Int { min, max } => Json::object().with("type", "integer").with("minimum", *min).with("maximum", *max),
        Ty::Num { min, max } => Json::object().with("type", "number").with("minimum", *min).with("maximum", *max),
        Ty::Str | Ty::Path => Json::object().with("type", "string"),
        Ty::Name => Json::object().with("type", "string").with("pattern", "^[A-Za-z0-9_/-]+$"),
        Ty::Color => Json::object()
            .with("type", "string")
            .with("pattern", "^#([0-9a-fA-F]{3}|[0-9a-fA-F]{4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$"),
        Ty::Enum(values) => Json::object().with("enum", values.iter().map(|v| Json::from(*v)).collect::<Vec<_>>()),
        Ty::Vec2 => num_array("number", 2, 2),
        Ty::Pos => num_array("number", 2, 3),
        Ty::Cell => num_array("integer", 2, 2),
        Ty::Cells => Json::object().with("type", "array").with("items", num_array("integer", 2, 2)).with("minItems", 1),
        Ty::Codes => Json::object()
            .with("type", "array")
            .with("items", Json::object().with("type", "string").with("pattern", "^W[0-9]{4}$")),
        Ty::Props => Json::object().with("type", "object").with(
            "additionalProperties",
            Json::object()
                .with("type", vec![Json::from("boolean"), "integer".into(), "number".into(), "string".into()]),
        ),
        Ty::Table(spec) => table_schema(spec),
        Ty::Tables(spec) => Json::object().with("type", "array").with("items", table_schema(spec)),
    }
}

// ---------------------------------------------------------------------------
// Markdown reference

/// A markdown section documenting one spec: doc text and a key table.
pub fn spec_markdown(spec: &Spec, heading: &str) -> String {
    let mut out = format!("{heading} `{}`\n\n{}\n\n", spec.title, spec.doc);
    if spec.fields.is_empty() {
        return out;
    }
    out.push_str("| key | type | default | meaning |\n|---|---|---|---|\n");
    for f in spec.fields {
        let default = match (f.required, f.default) {
            (true, _) => "**required**".to_string(),
            (false, Some(d)) => format!("`{d}`"),
            (false, None) => "—".to_string(),
        };
        out.push_str(&format!("| `{}` | {} | {} | {} |\n", f.name, f.ty.label(), default, f.doc.replace('|', "\\|")));
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(spec: &Spec, text: &str) -> Vec<Diagnostic> {
        let src = SourceFile::new("t.toml", text);
        let t = parse_toml(&src).unwrap();
        let mut diags = Vec::new();
        Checker { src: &src, diags: &mut diags }.table(&t, spec, Span::default());
        diags
    }

    #[test]
    fn every_default_is_valid_for_its_type() {
        for spec in ALL_SPECS {
            for f in spec.fields {
                if let Some(d) = f.default {
                    let node = parse_default(d);
                    let src = SourceFile::new("<default>", "");
                    let mut diags = Vec::new();
                    let ok = Checker { src: &src, diags: &mut diags }.value(&node, &f.ty, f.name);
                    assert!(ok, "default for {}.{} is invalid: {:?}", spec.id, f.name, diags);
                }
            }
        }
    }

    #[test]
    fn unknown_keys_suggest_the_closest() {
        let d = check(&ENTITY, "sprit = { color = \"#fff\" }");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].code, "E0103");
        assert_eq!(d[0].hint.as_deref(), Some("did you mean `sprite`?"));
    }

    #[test]
    fn nested_component_errors_are_located() {
        let d = check(&ENTITY, "pos = [1, 2]\nsprite = { color = \"orange\" }\n");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].code, "E0105");
        assert_eq!(d[0].line, Some(2));
    }

    #[test]
    fn wrong_types_and_missing_keys() {
        let d = check(&CONNECT, "from = 3\nsignal = \"pressed\"\nto = \"gate\"\n");
        let codes: Vec<&str> = d.iter().map(|d| d.code).collect();
        assert_eq!(codes, vec!["E0102", "E0104"]);
    }

    #[test]
    fn enums_suggest_values() {
        let d = check(&CONNECT, "from = \"a\"\nsignal = \"s\"\nto = \"b\"\naction = \"disabel\"\n");
        assert_eq!(d[0].hint.as_deref(), Some("did you mean `disable`?"));
    }

    #[test]
    fn find_spec_by_name() {
        assert_eq!(find_spec("sprite").map(|s| s.id), Some("sprite"));
        assert_eq!(find_spec("[ground]").map(|s| s.id), Some("scene.ground"));
        assert_eq!(find_spec("ground").map(|s| s.id), Some("scene.ground"));
        assert_eq!(find_spec("connect").map(|s| s.id), Some("connect"));
    }

    #[test]
    fn json_schema_lists_required_keys() {
        let s = json_schema(&CONNECT);
        assert_eq!(s.pointer("required/0").and_then(Json::as_str), Some("from"));
        assert_eq!(s.pointer("properties/action/enum/2").and_then(Json::as_str), Some("toggle"));
    }
}
