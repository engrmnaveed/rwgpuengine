//! Typed definitions decoded from validated tables.
//!
//! These are plain data: what the files say, with defaults applied. The
//! simulation builds its world from them; nothing here changes at runtime.

use slate_core::color::Rgba;
use slate_core::diag::{SourceFile, Span};

use crate::node::{Node, NodeKind, Table};
use crate::schema::{self, Fields};

/// Abstract input actions. Keyboards, touch and scripts all map onto these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Cancel,
}

impl Action {
    pub const ALL: [Action; 6] = [Action::Up, Action::Down, Action::Left, Action::Right, Action::Confirm, Action::Cancel];
    /// Directions in the order movers check them when several are held.
    pub const DIRECTIONS: [Action; 4] = [Action::Up, Action::Down, Action::Left, Action::Right];

    pub fn name(self) -> &'static str {
        match self {
            Action::Up => "up",
            Action::Down => "down",
            Action::Left => "left",
            Action::Right => "right",
            Action::Confirm => "confirm",
            Action::Cancel => "cancel",
        }
    }

    pub fn parse(s: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.name() == s)
    }

    /// Bit in an action mask (used by the web player's C ABI).
    pub fn bit(self) -> u32 {
        1 << (self as u32)
    }

    /// Grid step for direction actions: up = -y, down = +y, left = -x, right = +x.
    pub fn delta(self) -> Option<(i32, i32)> {
        match self {
            Action::Up => Some((0, -1)),
            Action::Down => Some((0, 1)),
            Action::Left => Some((-1, 0)),
            Action::Right => Some((1, 0)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProjectDef {
    pub name: String,
    pub engine: Option<String>,
    pub main_scene: String,
    pub display: DisplayDef,
    pub iso: IsoDef,
    pub sim: SimDef,
}

#[derive(Clone, Copy, Debug)]
pub struct DisplayDef {
    pub width: u32,
    pub height: u32,
    pub background: Rgba,
}

#[derive(Clone, Copy, Debug)]
pub struct IsoDef {
    pub tile_width: i32,
    pub tile_height: i32,
    pub z_height: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct SimDef {
    pub tick_rate: u32,
    pub seed: u64,
}

#[derive(Clone, Debug)]
pub struct SceneDef {
    pub path: String,
    pub name: String,
    pub doc: Option<String>,
    pub seed: Option<u64>,
    pub camera: CameraDef,
    pub ground: Option<GroundDef>,
    pub entities: Vec<EntityDef>,
    pub connections: Vec<ConnectDef>,
}

#[derive(Clone, Copy, Debug)]
pub struct CameraDef {
    pub center: Option<[f32; 2]>,
    pub offset: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
pub struct GroundDef {
    pub size: [i32; 2],
    pub color: Rgba,
    pub alt_color: Rgba,
}

/// Where a definition came from, for messages.
#[derive(Clone, Debug, Default)]
pub struct Origin {
    pub file: String,
    pub line: usize,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct EntityDef {
    pub name: Option<String>,
    pub prefab: Option<String>,
    pub pos: [f32; 3],
    pub enabled: bool,
    pub solid: bool,
    pub tile: Option<TileDef>,
    pub block: Option<BlockDef>,
    pub sprite: Option<SpriteDef>,
    pub mover: Option<MoverDef>,
    pub patrol: Option<PatrolDef>,
    pub wander: Option<WanderDef>,
    pub trigger: Option<TriggerDef>,
    pub props: Vec<(String, PropValue)>,
    pub origin: Origin,
}

impl EntityDef {
    /// The grid cell the entity stands on.
    pub fn cell(&self) -> [i32; 2] {
        [round_cell(self.pos[0]), round_cell(self.pos[1])]
    }

    /// Actors are what triggers notice and what blocks other movers.
    pub fn is_actor(&self) -> bool {
        self.mover.is_some() || self.patrol.is_some() || self.wander.is_some()
    }
}

/// Round half up with exact float ops (deterministic on every platform).
pub fn round_cell(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileDef {
    pub color: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockDef {
    pub color: Rgba,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteShape {
    Circle,
    Rect,
    Diamond,
}

impl SpriteShape {
    pub fn name(self) -> &'static str {
        match self {
            SpriteShape::Circle => "circle",
            SpriteShape::Rect => "rect",
            SpriteShape::Diamond => "diamond",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteDef {
    pub shape: SpriteShape,
    pub color: Rgba,
    pub size: [f32; 2],
    pub offset: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoverDef {
    pub step_frames: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatrolMode {
    Loop,
    PingPong,
}

impl PatrolMode {
    pub fn name(self) -> &'static str {
        match self {
            PatrolMode::Loop => "loop",
            PatrolMode::PingPong => "pingpong",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatrolDef {
    pub path: Vec<[i32; 2]>,
    pub step_frames: u32,
    pub mode: PatrolMode,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WanderDef {
    pub step_frames: u32,
    /// Inclusive corners, normalized so min <= max.
    pub area: Option<[[i32; 2]; 2]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TriggerDef {
    pub on_enter: String,
    pub on_exit: String,
    pub only: Option<String>,
}

/// A value in an entity's free-form `props` table.
#[derive(Clone, Debug, PartialEq)]
pub enum PropValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectAction {
    Enable,
    Disable,
    Toggle,
}

impl ConnectAction {
    pub fn name(self) -> &'static str {
        match self {
            ConnectAction::Enable => "enable",
            ConnectAction::Disable => "disable",
            ConnectAction::Toggle => "toggle",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConnectDef {
    pub from: String,
    pub signal: String,
    pub to: String,
    pub action: ConnectAction,
}

#[derive(Clone, Debug)]
pub struct PlaytestDef {
    pub path: String,
    /// File stem, e.g. `smoke` for tests/smoke.toml.
    pub name: String,
    pub doc: Option<String>,
    pub scene: Option<String>,
    /// Span of the `scene` value, for messages.
    pub scene_span: Span,
    pub frames: u64,
    pub seed: Option<u64>,
    pub inputs: Vec<InputDef>,
    pub expects: Vec<ExpectDef>,
    pub shots: Vec<ShotDef>,
    /// The parsed file, kept so later checks can point into it.
    pub src: SourceFile,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InputDef {
    pub frame: u64,
    pub kind: InputKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InputKind {
    Press(Action),
    Hold(Action, u64),
    Tap(f32, f32),
}

#[derive(Clone, Debug)]
pub struct ExpectDef {
    pub frame: Option<u64>,
    pub entity: Option<String>,
    pub pos: Option<[f32; 3]>,
    pub enabled: Option<bool>,
    pub solid: Option<bool>,
    pub props: Vec<(String, PropValue)>,
    pub event: Option<String>,
    pub signal: Option<String>,
    pub at_least: u64,
    /// Span of the `entity` value (or the table), for messages.
    pub span: Span,
    pub line: usize,
}

#[derive(Clone, Debug)]
pub struct ShotDef {
    pub frame: Option<u64>,
    pub reference: String,
    pub max_diff_pixels: u64,
    pub line: usize,
}

// ---------------------------------------------------------------------------
// Decoding (tables are already validated; Fields fills in defaults)

pub(crate) fn decode_project(root: &Table) -> ProjectDef {
    let empty = Table::default();
    let p = Fields::new(root.table("project").unwrap_or(&empty), &schema::PROJECT);
    let d = Fields::new(root.table("display").unwrap_or(&empty), &schema::DISPLAY);
    let i = Fields::new(root.table("iso").unwrap_or(&empty), &schema::ISO);
    let s = Fields::new(root.table("sim").unwrap_or(&empty), &schema::SIM);
    ProjectDef {
        name: p.str("name"),
        engine: p.opt_str("engine"),
        main_scene: p.str("main_scene"),
        display: DisplayDef { width: d.int("width") as u32, height: d.int("height") as u32, background: d.color("background") },
        iso: IsoDef {
            tile_width: i.int("tile_width") as i32,
            tile_height: i.int("tile_height") as i32,
            z_height: i.int("z_height") as i32,
        },
        sim: SimDef { tick_rate: s.int("tick_rate") as u32, seed: s.int("seed") as u64 },
    }
}

pub(crate) fn decode_camera(t: Option<&Table>) -> CameraDef {
    let empty = Table::default();
    let c = Fields::new(t.unwrap_or(&empty), &schema::CAMERA);
    CameraDef { center: c.opt_vec2("center"), offset: c.vec2("offset") }
}

pub(crate) fn decode_ground(t: &Table) -> GroundDef {
    let g = Fields::new(t, &schema::GROUND);
    let color = g.color("color");
    GroundDef { size: g.cell("size"), color, alt_color: g.opt_color("alt_color").unwrap_or(color.scale(9, 10)) }
}

/// Decode an entity from its merged (prefab + overrides) table.
pub(crate) fn decode_entity(t: &Table, pos: [f32; 3], origin: Origin) -> EntityDef {
    let f = Fields::new(t, &schema::ENTITY);
    let comp = |key: &str| f.table(key);
    EntityDef {
        name: f.opt_str("name"),
        prefab: f.opt_str("prefab"),
        pos,
        enabled: f.bool("enabled"),
        solid: f.bool("solid"),
        tile: comp("tile").map(|t| TileDef { color: Fields::new(t, &schema::TILE).color("color") }),
        block: comp("block").map(|t| {
            let b = Fields::new(t, &schema::BLOCK);
            BlockDef { color: b.color("color"), height: b.num("height") as f32 }
        }),
        sprite: comp("sprite").map(|t| {
            let s = Fields::new(t, &schema::SPRITE);
            let shape = match s.str("shape").as_str() {
                "rect" => SpriteShape::Rect,
                "diamond" => SpriteShape::Diamond,
                _ => SpriteShape::Circle,
            };
            SpriteDef { shape, color: s.color("color"), size: s.vec2("size"), offset: s.vec2("offset") }
        }),
        mover: comp("mover").map(|t| MoverDef { step_frames: Fields::new(t, &schema::MOVER).int("step_frames") as u32 }),
        patrol: comp("patrol").map(|t| {
            let p = Fields::new(t, &schema::PATROL);
            let mode = if p.str("mode") == "pingpong" { PatrolMode::PingPong } else { PatrolMode::Loop };
            PatrolDef { path: p.cells("path"), step_frames: p.int("step_frames") as u32, mode }
        }),
        wander: comp("wander").map(|t| {
            let w = Fields::new(t, &schema::WANDER);
            let cells = w.cells("area");
            let area = match cells.as_slice() {
                [a, b] => Some([[a[0].min(b[0]), a[1].min(b[1])], [a[0].max(b[0]), a[1].max(b[1])]]),
                _ => None,
            };
            WanderDef { step_frames: w.int("step_frames") as u32, area }
        }),
        trigger: comp("trigger").map(|t| {
            let tr = Fields::new(t, &schema::TRIGGER);
            TriggerDef { on_enter: tr.str("on_enter"), on_exit: tr.str("on_exit"), only: tr.opt_str("only") }
        }),
        props: f.table("props").map(decode_props).unwrap_or_default(),
        origin,
    }
}

pub(crate) fn decode_props(t: &Table) -> Vec<(String, PropValue)> {
    t.entries
        .iter()
        .filter_map(|e| {
            let v = match &e.value.kind {
                NodeKind::Bool(b) => PropValue::Bool(*b),
                NodeKind::Int(i) => PropValue::Int(*i),
                NodeKind::Float(f) => PropValue::Float(*f),
                NodeKind::Str(s) => PropValue::Str(s.clone()),
                _ => return None,
            };
            Some((e.key.clone(), v))
        })
        .collect()
}

pub(crate) fn decode_connect(t: &Table) -> ConnectDef {
    let c = Fields::new(t, &schema::CONNECT);
    let action = match c.str("action").as_str() {
        "enable" => ConnectAction::Enable,
        "disable" => ConnectAction::Disable,
        _ => ConnectAction::Toggle,
    };
    ConnectDef { from: c.str("from"), signal: c.str("signal"), to: c.str("to"), action }
}

pub(crate) fn decode_input(t: &Table) -> Option<InputDef> {
    let i = Fields::new(t, &schema::INPUT);
    let frame = i.int("frame") as u64;
    let kind = if let Some(a) = i.opt_str("press").and_then(|s| Action::parse(&s)) {
        InputKind::Press(a)
    } else if let Some(a) = i.opt_str("hold").and_then(|s| Action::parse(&s)) {
        InputKind::Hold(a, i.int("for") as u64)
    } else {
        let [x, y] = i.opt_vec2("tap")?;
        InputKind::Tap(x, y)
    };
    Some(InputDef { frame, kind })
}

pub(crate) fn decode_expect(t: &Table, span: Span, line: usize) -> ExpectDef {
    let e = Fields::new(t, &schema::EXPECT);
    ExpectDef {
        frame: e.opt_int("frame").map(|v| v as u64),
        entity: e.opt_str("entity"),
        pos: e.opt_pos("pos"),
        enabled: e.table.bool("enabled"),
        solid: e.table.bool("solid"),
        props: e.table("props").map(decode_props).unwrap_or_default(),
        event: e.opt_str("event"),
        signal: e.opt_str("signal"),
        at_least: e.int("at_least") as u64,
        span: t.value_span("entity", span),
        line,
    }
}

pub(crate) fn decode_shot(t: &Table, line: usize) -> ShotDef {
    let s = Fields::new(t, &schema::SHOT);
    ShotDef {
        frame: s.opt_int("frame").map(|v| v as u64),
        reference: s.str("reference"),
        max_diff_pixels: s.int("max_diff_pixels") as u64,
        line,
    }
}

/// Cells listed in a `cells` key, with an optional z from `pos`.
pub(crate) fn cells_of(n: &Node) -> Vec<[i32; 2]> {
    n.as_array().map(|items| items.iter().filter_map(schema::node_cell).collect()).unwrap_or_default()
}
