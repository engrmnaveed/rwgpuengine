//! Typed definitions decoded from validated tables.
//!
//! These are plain data: what the files say, with defaults applied. The
//! simulation builds its world from them; nothing here changes at runtime.

use std::sync::Arc;

use rwgpuengine_core::color::Rgba;
use rwgpuengine_core::diag::{SourceFile, Span};

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
    pub const ALL: [Action; 6] =
        [Action::Up, Action::Down, Action::Left, Action::Right, Action::Confirm, Action::Cancel];
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
    /// Prefabs named by `spawn` actions, decoded at [0, 0, 0], in first-use order.
    pub templates: Vec<EntityDef>,
}

#[derive(Clone, Copy, Debug)]
pub struct CameraDef {
    pub center: Option<[f32; 2]>,
    pub offset: [f32; 2],
    pub view: CameraView,
    /// Degrees turned about the vertical axis through `center`.
    pub turn: f32,
    pub zoom: f32,
}

/// How the world is projected onto the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraView {
    /// The isometric view set by `[iso]`: parallel lines stay parallel.
    Iso,
    /// A camera 30 degrees above the horizon; far things are drawn smaller.
    Perspective,
}

impl CameraView {
    pub const NAMES: &'static [&'static str] = &["iso", "perspective"];

    pub fn name(self) -> &'static str {
        match self {
            CameraView::Iso => "iso",
            CameraView::Perspective => "perspective",
        }
    }

    pub fn parse(s: &str) -> Option<CameraView> {
        match s {
            "iso" => Some(CameraView::Iso),
            "perspective" => Some(CameraView::Perspective),
            _ => None,
        }
    }
}

/// One move of the camera: a playtest's `[[input]] camera`, or a player's
/// gesture. The parts are applied together: view, then turn, zoom and pan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraMove {
    pub view: Option<CameraView>,
    /// Degrees to turn by.
    pub turn: f32,
    /// Factor to multiply the zoom by.
    pub zoom: f32,
    /// Cells [x, y] to move the center by.
    pub pan: [f32; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct GroundDef {
    pub size: [i32; 2],
    /// At least one. Without `map`, a single buildable terrain from `color`.
    pub terrains: Vec<TerrainDef>,
    /// Terrain index of every cell, row by row: `cells[y * size_x + x]`.
    pub cells: Vec<u8>,
    /// The ground was written as a `map` (it is then part of the state hash
    /// and of `inspect`).
    pub has_map: bool,
}

/// A kind of ground cell (`[[terrain]]`).
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainDef {
    pub ch: char,
    pub name: String,
    pub color: Rgba,
    pub alt_color: Rgba,
    /// Spawned entities can be placed on it.
    pub build: bool,
}

impl GroundDef {
    /// Plain ground of one buildable terrain.
    pub fn plain(size: [i32; 2], color: Rgba, alt_color: Rgba) -> GroundDef {
        let terrain = TerrainDef { ch: '.', name: "ground".into(), color, alt_color, build: true };
        let n = (size[0].max(0) as usize) * (size[1].max(0) as usize);
        GroundDef { size, terrains: vec![terrain], cells: vec![0; n], has_map: false }
    }

    pub fn contains(&self, c: [i32; 2]) -> bool {
        c[0] >= 0 && c[1] >= 0 && c[0] < self.size[0] && c[1] < self.size[1]
    }

    /// The terrain of a cell, `None` outside the ground.
    pub fn terrain_at(&self, c: [i32; 2]) -> Option<&TerrainDef> {
        if !self.contains(c) {
            return None;
        }
        let i = c[1] as usize * self.size[0] as usize + c[0] as usize;
        self.terrains.get(*self.cells.get(i)? as usize)
    }

    /// The map rows as text (`inspect`).
    pub fn rows(&self) -> Vec<String> {
        (0..self.size[1])
            .map(|y| (0..self.size[0]).map(|x| self.terrain_at([x, y]).map_or('?', |t| t.ch)).collect())
            .collect()
    }
}

/// Where a definition came from, for messages.
#[derive(Clone, Debug, Default)]
pub struct Origin {
    pub file: String,
    pub line: usize,
    pub span: Span,
}

#[derive(Clone)]
pub struct EntityDef {
    pub name: Option<String>,
    pub prefab: Option<String>,
    pub pos: [f32; 3],
    /// Cells covered along +x and +y from `pos` (at least [1, 1]).
    pub footprint: [i32; 2],
    pub enabled: bool,
    pub solid: bool,
    /// Movers push it; triggers count it.
    pub pushable: bool,
    pub tile: Option<TileDef>,
    pub block: Option<BlockDef>,
    pub sprite: Option<SpriteDef>,
    pub mover: Option<MoverDef>,
    pub patrol: Option<PatrolDef>,
    pub wander: Option<WanderDef>,
    pub trigger: Option<TriggerDef>,
    pub label: Option<LabelDef>,
    /// Drawn on the screen at this pixel (top-left) instead of in the world.
    pub screen_pos: Option<[f32; 2]>,
    /// Declared props, then one integer prop per `counts` entry.
    pub props: Vec<(String, PropValue)>,
    /// `counts`: (prop, prefab). The engine keeps each prop equal to the
    /// number of enabled copies of the prefab.
    pub counts: Vec<(String, String)>,
    /// Warning codes intended for this entity (not reported).
    pub allow: Vec<String>,
    pub origin: Origin,
    /// Its `[[shape]]` list, if it has one with shapes in it. Every entity
    /// made from one prefab shares the prefab's list.
    pub shapes: Option<Arc<[ShapeDef]>>,
}

/// As `#[derive(Debug)]` writes it, with the keys that came after the
/// loader's oracle (`corpus.rs`) only where they are set: the definition of
/// a file that does not use them reads as it did, so its pin holds.
impl std::fmt::Debug for EntityDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("EntityDef");
        d.field("name", &self.name)
            .field("prefab", &self.prefab)
            .field("pos", &self.pos)
            .field("footprint", &self.footprint)
            .field("enabled", &self.enabled)
            .field("solid", &self.solid)
            .field("pushable", &self.pushable)
            .field("tile", &self.tile)
            .field("block", &self.block)
            .field("sprite", &self.sprite)
            .field("mover", &self.mover)
            .field("patrol", &self.patrol)
            .field("wander", &self.wander)
            .field("trigger", &self.trigger)
            .field("label", &self.label)
            .field("screen_pos", &self.screen_pos)
            .field("props", &self.props)
            .field("counts", &self.counts)
            .field("allow", &self.allow)
            .field("origin", &self.origin);
        if let Some(shapes) = &self.shapes {
            d.field("shapes", shapes);
        }
        d.finish()
    }
}

impl EntityDef {
    pub fn prop(&self, name: &str) -> Option<&PropValue> {
        self.props.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    /// The prefab a `counts` prop counts, if `name` is one.
    pub fn counted(&self, name: &str) -> Option<&str> {
        self.counts.iter().find(|(k, _)| k == name).map(|(_, p)| p.as_str())
    }

    /// The grid cell the entity stands on.
    pub fn cell(&self) -> [i32; 2] {
        [round_cell(self.pos[0]), round_cell(self.pos[1])]
    }

    /// Actors are what triggers notice and what blocks other movers.
    pub fn is_actor(&self) -> bool {
        self.mover.is_some() || self.patrol.is_some() || self.wander.is_some()
    }

    /// Blocks movement like a solid (pushables do too, unless pushed).
    pub fn blocks(&self) -> bool {
        self.solid || self.pushable
    }

    /// Every cell the entity covers, x first then y.
    pub fn cells(&self) -> Vec<[i32; 2]> {
        footprint_cells(self.cell(), self.footprint)
    }
}

/// The cells of a `footprint` whose back corner (lowest x and y) is `at`.
pub fn footprint_cells(at: [i32; 2], footprint: [i32; 2]) -> Vec<[i32; 2]> {
    let mut out = Vec::with_capacity((footprint[0].max(1) * footprint[1].max(1)) as usize);
    for dy in 0..footprint[1].max(1) {
        for dx in 0..footprint[0].max(1) {
            out.push([at[0] + dx, at[1] + dy]);
        }
    }
    out
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
    /// Top face color (defaults to `color`).
    pub top: Rgba,
    pub height: f32,
}

/// The solids a `[[shape]]` can be. Each fills its box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ShapeKind {
    Box,
    /// A gable roof: two slopes that meet in a ridge along x.
    Roof,
    /// A ramp or a lean-to roof: one slope that rises toward +x.
    Wedge,
    /// Round and upright.
    Cylinder,
    /// Round, with its point up.
    Cone,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 5] =
        [ShapeKind::Box, ShapeKind::Roof, ShapeKind::Wedge, ShapeKind::Cylinder, ShapeKind::Cone];

    pub fn name(self) -> &'static str {
        match self {
            ShapeKind::Box => "box",
            ShapeKind::Roof => "roof",
            ShapeKind::Wedge => "wedge",
            ShapeKind::Cylinder => "cylinder",
            ShapeKind::Cone => "cone",
        }
    }

    pub fn parse(s: &str) -> Option<ShapeKind> {
        ShapeKind::ALL.into_iter().find(|k| k.name() == s)
    }
}

/// One `[[shape]]`: a solid in one flat color that fills the box from `at`
/// to `at + size`. `at` is counted from the corner of the entity's footprint
/// with the lowest x and y, at the entity's height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeDef {
    pub kind: ShapeKind,
    pub at: [f32; 3],
    pub size: [f32; 3],
    pub color: Rgba,
    /// Quarter turns of the solid inside its box, from +x toward +y (0 to 3).
    pub turn: u8,
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

/// Text drawn with the built-in pixel font.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelDef {
    /// The template, with `{prop}` placeholders.
    pub text: String,
    pub color: Rgba,
    pub scale: u32,
    pub offset: [f32; 2],
}

/// Metrics of the built-in pixel font, in font pixels (multiply by `scale`).
pub mod font {
    /// Glyph cell: 5 columns, 8 rows (row 7 holds descenders).
    pub const GLYPH_W: i32 = 5;
    pub const GLYPH_H: i32 = 8;
    /// Distance from one character to the next.
    pub const ADVANCE: i32 = 6;
    /// Distance from one line to the next.
    pub const LINE: i32 = 10;

    /// Characters the font can draw (others are drawn as a hollow box).
    pub fn supports(c: char) -> bool {
        c == '\n' || (' '..='~').contains(&c)
    }

    /// Size of the text box in font pixels: `[width, height]`.
    pub fn text_size(text: &str) -> [i32; 2] {
        let lines: Vec<&str> = text.split('\n').collect();
        let widest = lines.iter().map(|l| l.chars().count() as i32).max().unwrap_or(0);
        let w = if widest == 0 { 0 } else { widest * ADVANCE - (ADVANCE - GLYPH_W) };
        let h = lines.len() as i32 * LINE - (LINE - GLYPH_H);
        [w, h]
    }
}

/// One piece of a label template.
#[derive(Clone, Debug, PartialEq)]
pub enum LabelPart {
    Text(String),
    Prop(String),
}

/// Split a label template into literal text and `{prop}` placeholders.
/// `{{` and `}}` are literal braces. Errors describe the first problem.
pub fn parse_label(template: &str) -> Result<Vec<LabelPart>, String> {
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                lit.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                lit.push('}');
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => name.push(c),
                        None => return Err(format!("`{{{name}` is never closed; write `{{{{` for a literal brace")),
                    }
                }
                let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "_-".contains(c));
                if !valid {
                    return Err(format!("`{{{name}}}` is not a prop name; write `{{{{` for a literal brace"));
                }
                if !lit.is_empty() {
                    parts.push(LabelPart::Text(std::mem::take(&mut lit)));
                }
                parts.push(LabelPart::Prop(name));
            }
            '}' => return Err("a single `}` must be written `}}`".to_string()),
            c => lit.push(c),
        }
    }
    if !lit.is_empty() {
        parts.push(LabelPart::Text(lit));
    }
    Ok(parts)
}

/// A value in an entity's `props` table. Its type is fixed by the declared
/// starting value.
#[derive(Clone, Debug, PartialEq)]
pub enum PropValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl PropValue {
    pub fn type_name(&self) -> &'static str {
        match self {
            PropValue::Bool(_) => "a boolean",
            PropValue::Int(_) => "an integer",
            PropValue::Float(_) => "a number",
            PropValue::Str(_) => "a string",
        }
    }

    pub fn as_num(&self) -> Option<f64> {
        match self {
            PropValue::Int(i) => Some(*i as f64),
            PropValue::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Can a prop of this type hold `v`? Integers fit number props.
    pub fn accepts(&self, v: &PropValue) -> bool {
        matches!(
            (self, v),
            (PropValue::Bool(_), PropValue::Bool(_))
                | (PropValue::Int(_), PropValue::Int(_))
                | (PropValue::Float(_), PropValue::Float(_) | PropValue::Int(_))
                | (PropValue::Str(_), PropValue::Str(_))
        )
    }

    /// Convert `v` to this prop's type (call after [`PropValue::accepts`]).
    pub fn coerce(&self, v: &PropValue) -> PropValue {
        match (self, v) {
            (PropValue::Float(_), PropValue::Int(i)) => PropValue::Float(*i as f64),
            _ => v.clone(),
        }
    }

    /// Equal values; an integer equals the same whole number.
    pub fn same(&self, v: &PropValue) -> bool {
        match (self, v) {
            (PropValue::Int(x), PropValue::Float(y)) | (PropValue::Float(y), PropValue::Int(x)) => (*x as f64) == *y,
            _ => self == v,
        }
    }

    /// As shown in labels: `3`, `0.5`, `true`, text without quotes.
    pub fn display(&self) -> String {
        match self {
            PropValue::Bool(b) => b.to_string(),
            PropValue::Int(i) => i.to_string(),
            PropValue::Float(f) => f.to_string(),
            PropValue::Str(s) => s.clone(),
        }
    }

    /// As written in TOML: strings quoted.
    pub fn literal(&self) -> String {
        match self {
            PropValue::Str(s) => format!("{s:?}"),
            v => v.display(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectAction {
    Enable,
    Disable,
    Toggle,
    /// Add `value` to the target's number prop.
    Add,
    /// Set the target's prop to `value`.
    Set,
    /// Place a new copy of a prefab where the signal happened.
    Spawn,
}

impl ConnectAction {
    pub fn name(self) -> &'static str {
        match self {
            ConnectAction::Enable => "enable",
            ConnectAction::Disable => "disable",
            ConnectAction::Toggle => "toggle",
            ConnectAction::Add => "add",
            ConnectAction::Set => "set",
            ConnectAction::Spawn => "spawn",
        }
    }

    pub fn changes_prop(self) -> bool {
        matches!(self, ConnectAction::Add | ConnectAction::Set)
    }
}

/// Who a `[[connect]]` listens to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectFrom {
    /// One entity, by name.
    Entity(String),
    /// Every entity made from this prefab.
    Prefab(String),
    /// Taps on ground cells that hit no entity.
    Ground,
}

impl ConnectFrom {
    /// `'plate'`, `every 'coin'`, `the ground`, for messages.
    pub fn describe(&self) -> String {
        match self {
            ConnectFrom::Entity(n) => format!("'{n}'"),
            ConnectFrom::Prefab(p) => format!("every '{p}'"),
            ConnectFrom::Ground => "the ground".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConnectDef {
    pub from: ConnectFrom,
    pub signal: String,
    /// One (`action = ...`) or several (`actions = [...]`), run in order.
    pub actions: Vec<ActionDef>,
    /// All must hold when the signal arrives (checked once, before the first action).
    pub when: Vec<ConditionDef>,
}

/// One action of a `[[connect]]`.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionDef {
    pub action: ConnectAction,
    /// `None` for `spawn`; `Some("self")` targets the emitter.
    pub to: Option<String>,
    /// With add/set: the target's prop.
    pub prop: Option<String>,
    /// With add/set: the value (add defaults to 1).
    pub value: Option<PropValue>,
    /// With spawn: the prefab to place.
    pub prefab: Option<String>,
    /// With spawn: the price, paid only if the copy is placed.
    pub cost: Option<CostDef>,
}

/// `cost = { entity = "town", prop = "coins", amount = 3 }` on a spawn.
#[derive(Clone, Debug, PartialEq)]
pub struct CostDef {
    pub entity: String,
    pub prop: String,
    /// Int or Float, as written.
    pub amount: PropValue,
}

impl ActionDef {
    /// The target entity's name, unless it is `self` or absent.
    pub fn to_name(&self) -> Option<&str> {
        self.to.as_deref().filter(|t| *t != "self")
    }
}

/// The tables that hold a `[[connect]]`'s actions, with their spans: the
/// items of `actions`, or the [[connect]] table itself.
pub fn action_tables(t: &Table, span: Span) -> Vec<(&Table, Span)> {
    match t.get("actions").and_then(Node::as_array) {
        Some(items) => items.iter().filter_map(|n| Some((n.as_table()?, n.span))).collect(),
        None => vec![(t, span)],
    }
}

/// One `when` item.
#[derive(Clone, Debug, PartialEq)]
pub struct ConditionDef {
    /// Empty for a `reason` condition.
    pub entity: String,
    pub test: CondTest,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CondTest {
    Occupied(bool),
    Enabled(bool),
    Prop {
        prop: String,
        equals: Option<PropValue>,
        at_least: Option<f64>,
        at_most: Option<f64>,
    },
    /// A `refused` signal's reason (no entity).
    Reason(String),
}

impl ConditionDef {
    /// `'plate_b' occupied`, `'hud'.coins >= 3`, for messages.
    pub fn describe(&self) -> String {
        let e = &self.entity;
        match &self.test {
            CondTest::Occupied(true) => format!("'{e}' occupied"),
            CondTest::Occupied(false) => format!("'{e}' not occupied"),
            CondTest::Enabled(true) => format!("'{e}' enabled"),
            CondTest::Enabled(false) => format!("'{e}' disabled"),
            CondTest::Reason(r) => format!("reason '{r}'"),
            CondTest::Prop { prop, equals, at_least, at_most } => {
                let mut tests = Vec::new();
                if let Some(v) = equals {
                    tests.push(format!("== {}", v.literal()));
                }
                if let Some(v) = at_least {
                    tests.push(format!(">= {v}"));
                }
                if let Some(v) = at_most {
                    tests.push(format!("<= {v}"));
                }
                format!("'{e}'.{prop} {}", tests.join(" and "))
            }
        }
    }
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
    Camera(CameraMove),
}

#[derive(Clone, Debug)]
pub struct ExpectDef {
    pub frame: Option<u64>,
    pub entity: Option<String>,
    pub pos: Option<[f32; 3]>,
    pub enabled: Option<bool>,
    pub solid: Option<bool>,
    pub props: Vec<(String, PropValue)>,
    /// Expected label text as shown.
    pub text: Option<String>,
    /// Count enabled entities of this prefab (at `pos`, if given).
    pub prefab: Option<String>,
    pub count: Option<u64>,
    pub event: Option<String>,
    pub signal: Option<String>,
    /// With `event = "blocked"`: solid, actor, edge, area or cooldown.
    pub reason: Option<String>,
    /// With `event`: the event's other party (blocker, actor, emitter).
    pub other: Option<String>,
    /// With `event = "action"`: only actions that changed something (or not).
    pub changed: Option<bool>,
    pub at_least: u64,
    pub at_most: Option<u64>,
    /// Span of the `other` value, for messages.
    pub other_span: Span,
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
        display: DisplayDef {
            width: d.int("width") as u32,
            height: d.int("height") as u32,
            background: d.color("background"),
        },
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
    CameraDef {
        center: c.opt_vec2("center"),
        offset: c.vec2("offset"),
        view: c.opt_str("view").and_then(|v| CameraView::parse(&v)).unwrap_or(CameraView::Iso),
        turn: c.num("turn") as f32,
        zoom: c.num("zoom") as f32,
    }
}

/// Plain ground from `size` and `color` (maps are built by the loader).
pub(crate) fn decode_plain_ground(t: &Table) -> GroundDef {
    let g = Fields::new(t, &schema::GROUND);
    let color = g.color("color");
    GroundDef::plain(g.cell("size"), color, g.opt_color("alt_color").unwrap_or(color.scale(9, 10)))
}

pub(crate) fn decode_terrain(t: &Table) -> TerrainDef {
    let f = Fields::new(t, &schema::TERRAIN);
    let ch = f.str("char").chars().next().unwrap_or('?');
    let color = f.color("color");
    TerrainDef {
        ch,
        name: f.opt_str("name").unwrap_or_else(|| ch.to_string()),
        color,
        alt_color: f.opt_color("alt_color").unwrap_or(color.scale(9, 10)),
        build: f.bool("build"),
    }
}

/// A `shape` list. `None` for a list with nothing in it (an entity can write
/// `shape = []` to have none of its prefab's), and for a list the schema
/// refused: its rules are then not checked on values it does not have.
pub(crate) fn decode_shapes(n: &Node) -> Option<Arc<[ShapeDef]>> {
    let shape = |item: &Node| {
        let t = item.as_table()?;
        let f = Fields::new(t, &schema::SHAPE);
        let three = t.get("size")?.as_array()?.len() == 3;
        let size = f.opt_pos("size").filter(|s| three && s.iter().all(|v| *v > 0.0))?;
        let turn = f.opt_int("turn").filter(|v| matches!(v, 0 | 90 | 180 | 270))?;
        Some(ShapeDef {
            kind: ShapeKind::parse(&f.str("kind"))?,
            at: f.opt_pos("at")?,
            size,
            color: f.opt_color("color")?,
            turn: (turn / 90) as u8,
        })
    };
    let shapes: Vec<ShapeDef> = n.as_array()?.iter().map(shape).collect::<Option<_>>()?;
    (!shapes.is_empty()).then(|| shapes.into())
}

/// Decode an entity from its merged (prefab + overrides) table. `shapes` is
/// its shape list, decoded once for every entity that shares it.
pub(crate) fn decode_entity(t: &Table, pos: [f32; 3], origin: Origin, shapes: Option<Arc<[ShapeDef]>>) -> EntityDef {
    let f = Fields::new(t, &schema::ENTITY);
    let comp = |key: &str| f.table(key);
    let counts: Vec<(String, String)> = f
        .table("counts")
        .map(|c| c.entries.iter().filter_map(|e| Some((e.key.clone(), e.value.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    EntityDef {
        name: f.opt_str("name"),
        prefab: f.opt_str("prefab"),
        pos,
        footprint: {
            let [w, h] = f.cell("footprint");
            [w.max(1), h.max(1)]
        },
        enabled: f.bool("enabled"),
        solid: f.bool("solid"),
        pushable: f.bool("pushable"),
        tile: comp("tile").map(|t| TileDef { color: Fields::new(t, &schema::TILE).color("color") }),
        block: comp("block").map(|t| {
            let b = Fields::new(t, &schema::BLOCK);
            let color = b.color("color");
            BlockDef { color, top: b.opt_color("top").unwrap_or(color), height: b.num("height") as f32 }
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
        mover: comp("mover")
            .map(|t| MoverDef { step_frames: Fields::new(t, &schema::MOVER).int("step_frames") as u32 }),
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
        label: comp("label").map(|t| {
            let l = Fields::new(t, &schema::LABEL);
            LabelDef {
                text: l.str("text"),
                color: l.color("color"),
                scale: l.int("scale") as u32,
                offset: l.vec2("offset"),
            }
        }),
        screen_pos: f.opt_vec2("screen_pos"),
        props: {
            let mut props = f.table("props").map(decode_props).unwrap_or_default();
            // A clash with a declared prop stays visible (two entries) for
            // `rwgpu check`; the declared one comes first.
            for (k, _) in &counts {
                props.push((k.clone(), PropValue::Int(0)));
            }
            props
        },
        counts,
        allow: f.strs("allow"),
        origin,
        shapes,
    }
}

pub(crate) fn decode_props(t: &Table) -> Vec<(String, PropValue)> {
    t.entries.iter().filter_map(|e| Some((e.key.clone(), scalar(&e.value)?))).collect()
}

/// A scalar node as a prop value.
pub(crate) fn scalar(n: &Node) -> Option<PropValue> {
    Some(match &n.kind {
        NodeKind::Bool(b) => PropValue::Bool(*b),
        NodeKind::Int(i) => PropValue::Int(*i),
        NodeKind::Float(f) => PropValue::Float(*f),
        NodeKind::Str(s) => PropValue::Str(s.clone()),
        _ => return None,
    })
}

pub(crate) fn decode_connect(t: &Table) -> ConnectDef {
    let c = Fields::new(t, &schema::CONNECT);
    let when = t
        .get("when")
        .and_then(Node::as_array)
        .map(|items| items.iter().filter_map(|n| n.as_table().map(decode_condition)).collect())
        .unwrap_or_default();
    let from = match (c.opt_str("from"), c.opt_str("from_prefab")) {
        (Some(f), _) if f == "ground" => ConnectFrom::Ground,
        (Some(f), _) => ConnectFrom::Entity(f),
        (None, Some(p)) => ConnectFrom::Prefab(p),
        (None, None) => ConnectFrom::Entity(String::new()),
    };
    let actions = action_tables(t, Span::default()).into_iter().map(|(a, _)| decode_action(a)).collect();
    ConnectDef { from, signal: c.str("signal"), actions, when }
}

/// One action from a [[connect]] table or an `actions` item (same keys).
pub(crate) fn decode_action(t: &Table) -> ActionDef {
    let c = Fields::new(t, &schema::ACTION);
    let action = match c.str("action").as_str() {
        "enable" => ConnectAction::Enable,
        "disable" => ConnectAction::Disable,
        "add" => ConnectAction::Add,
        "set" => ConnectAction::Set,
        "spawn" => ConnectAction::Spawn,
        _ => ConnectAction::Toggle,
    };
    let mut value = t.get("value").and_then(scalar);
    if action == ConnectAction::Add && value.is_none() {
        value = Some(PropValue::Int(1));
    }
    let cost = t.table("cost").map(|k| {
        let f = Fields::new(k, &schema::COST);
        CostDef {
            entity: f.str("entity"),
            prop: f.str("prop"),
            amount: k.get("amount").and_then(scalar).unwrap_or(PropValue::Int(0)),
        }
    });
    ActionDef { action, to: c.opt_str("to"), prop: c.opt_str("prop"), value, prefab: c.opt_str("prefab"), cost }
}

pub(crate) fn decode_condition(t: &Table) -> ConditionDef {
    let c = Fields::new(t, &schema::CONDITION);
    let test = if let Some(b) = t.bool("occupied") {
        CondTest::Occupied(b)
    } else if let Some(r) = c.opt_str("reason") {
        CondTest::Reason(r)
    } else if let Some(b) = t.bool("enabled") {
        CondTest::Enabled(b)
    } else {
        CondTest::Prop {
            prop: c.str("prop"),
            equals: t.get("equals").and_then(scalar),
            at_least: t.get("at_least").and_then(Node::as_num),
            at_most: t.get("at_most").and_then(Node::as_num),
        }
    };
    ConditionDef { entity: c.str("entity"), test }
}

pub(crate) fn decode_input(t: &Table) -> Option<InputDef> {
    let i = Fields::new(t, &schema::INPUT);
    let frame = i.int("frame") as u64;
    let kind = if let Some(a) = i.opt_str("press").and_then(|s| Action::parse(&s)) {
        InputKind::Press(a)
    } else if let Some(a) = i.opt_str("hold").and_then(|s| Action::parse(&s)) {
        InputKind::Hold(a, i.int("for") as u64)
    } else if let Some(c) = t.table("camera") {
        let c = Fields::new(c, &schema::INPUT_CAMERA);
        InputKind::Camera(CameraMove {
            view: c.opt_str("view").and_then(|v| CameraView::parse(&v)),
            turn: c.num("turn") as f32,
            zoom: c.num("zoom") as f32,
            pan: c.vec2("pan"),
        })
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
        text: e.opt_str("text"),
        prefab: e.opt_str("prefab"),
        count: e.opt_int("count").map(|v| v as u64),
        event: e.opt_str("event"),
        signal: e.opt_str("signal"),
        reason: e.opt_str("reason"),
        changed: e.table.bool("changed"),
        other: e.opt_str("other"),
        at_most: e.opt_int("at_most").map(|v| v as u64),
        // An explicit at_least wins; otherwise 1, or 0 when only at_most is given.
        at_least: match (t.int("at_least"), t.has("at_most")) {
            (Some(v), _) => v as u64,
            (None, true) => 0,
            (None, false) => 1,
        },
        other_span: t.value_span("other", span),
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
