//! The world: entities, connections, the RNG and the event log, advanced one
//! fixed step at a time.
//!
//! Entities are "fat": one struct with optional built-in components plus a
//! free-form `props` map (ADR 0006). That keeps the world easy to query and
//! to dump as JSON: what `slate inspect` prints is this struct.

use std::collections::{BTreeMap, VecDeque};

use slate_core::hash::Hasher64;
use slate_core::json::Json;
use slate_core::rng::Rng;
use slate_data::{
    Action, BlockDef, CondTest, ConditionDef, ConnectAction, ConnectFrom, EntityDef, GroundDef, LabelDef, LabelPart,
    PatrolDef, PatrolMode, PropValue, SceneDef, SpriteDef, TileDef, TriggerDef, WanderDef, footprint_cells,
    parse_label, round_cell,
};

use crate::dump::prop_json;
use crate::events::{Event, EventKind, EventLog};
use crate::input::InputFrame;

/// Entities are numbered in scene order, starting at 0. Ids are stable for a
/// given scene file, so they can be used in scripts and tests.
pub type EntityId = u32;

/// Maximum signals dispatched in one step before it is treated as a loop.
pub const SIGNAL_LIMIT: usize = 1000;

#[derive(Clone, Debug, PartialEq)]
pub struct Mover {
    pub step_frames: u32,
    /// Frames until the next move is allowed.
    pub cooldown: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Patrol {
    pub def: PatrolDef,
    /// Index into `def.path` of the cell being walked to.
    pub target: usize,
    /// +1 or -1 (pingpong direction).
    pub dir: i32,
    pub timer: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Wander {
    pub def: WanderDef,
    pub timer: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Trigger {
    pub def: TriggerDef,
    /// Actors on the trigger's cell after the last step.
    pub occupants: Vec<EntityId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    pub id: EntityId,
    pub name: Option<String>,
    pub prefab: Option<String>,
    pub enabled: bool,
    pub pos: [f32; 3],
    /// Cells covered along +x and +y from `pos` ([1, 1] for most entities).
    pub footprint: [i32; 2],
    pub solid: bool,
    /// Movers push it one cell; triggers count it; it blocks everything else.
    pub pushable: bool,
    pub tile: Option<TileDef>,
    pub block: Option<BlockDef>,
    pub sprite: Option<SpriteDef>,
    pub mover: Option<Mover>,
    pub patrol: Option<Patrol>,
    pub wander: Option<Wander>,
    pub trigger: Option<Trigger>,
    pub label: Option<LabelDef>,
    /// Drawn on the screen at this pixel (top-left) instead of in the world.
    pub screen_pos: Option<[f32; 2]>,
    pub props: BTreeMap<String, PropValue>,
    /// Warning codes intended for this entity (not reported).
    pub allow: Vec<String>,
}

impl Entity {
    /// The label's text as shown: placeholders replaced by prop values.
    pub fn label_text(&self) -> Option<String> {
        let label = self.label.as_ref()?;
        let Ok(parts) = parse_label(&label.text) else {
            return Some(label.text.clone());
        };
        let mut out = String::new();
        for p in parts {
            match p {
                LabelPart::Text(t) => out.push_str(&t),
                LabelPart::Prop(name) => match self.props.get(&name) {
                    Some(v) => out.push_str(&v.display()),
                    None => out.push_str(&format!("{{{name}}}")),
                },
            }
        }
        Some(out)
    }

    pub fn cell(&self) -> [i32; 2] {
        [round_cell(self.pos[0]), round_cell(self.pos[1])]
    }

    /// Every cell the entity covers (its footprint), x first then y.
    pub fn cells(&self) -> Vec<[i32; 2]> {
        footprint_cells(self.cell(), self.footprint)
    }

    /// Does the entity cover `c`?
    pub fn covers(&self, c: [i32; 2]) -> bool {
        let [x, y] = self.cell();
        c[0] >= x && c[1] >= y && c[0] < x + self.footprint[0] && c[1] < y + self.footprint[1]
    }

    /// Is the entity in the world (not a screen/HUD entity)?
    pub fn in_world(&self) -> bool {
        self.screen_pos.is_none()
    }

    /// Does it take up its cells for placement? Enabled world entities that
    /// are drawn there (tile, block, sprite, label), solid, pushable, actors
    /// or triggers. A data-only entity (just props) takes up nothing.
    pub fn takes_space(&self) -> bool {
        self.enabled
            && self.in_world()
            && (self.tile.is_some()
                || self.block.is_some()
                || self.sprite.is_some()
                || self.label.is_some()
                || self.trigger.is_some()
                || self.solid
                || self.pushable
                || self.is_actor())
    }

    pub fn is_actor(&self) -> bool {
        self.mover.is_some() || self.patrol.is_some() || self.wander.is_some()
    }

    /// What triggers count: actors and pushables.
    pub fn holds_plates(&self) -> bool {
        self.is_actor() || self.pushable
    }

    /// `player` or `#12` for unnamed entities.
    pub fn label(&self) -> String {
        match &self.name {
            Some(n) => n.clone(),
            None => format!("#{}", self.id),
        }
    }

    /// Is this warning code allowed (silenced) for this entity?
    pub fn allows(&self, code: &str) -> bool {
        self.allow.iter().any(|a| a.eq_ignore_ascii_case(code))
    }

    /// `'player' (#3)` for messages.
    pub fn describe(&self) -> String {
        match &self.name {
            Some(n) => format!("'{n}' (#{})", self.id),
            None => match &self.prefab {
                Some(p) => format!("#{} ({p})", self.id),
                None => format!("#{}", self.id),
            },
        }
    }
}

/// What a connection listens to.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Entity(EntityId),
    /// Every entity made from this prefab (placed or spawned).
    Prefab(String),
    /// Taps on ground cells that hit no entity.
    Ground,
}

/// What a connection acts on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Entity(EntityId),
    /// The entity that emitted the signal (`to = "self"`).
    Emitter,
    /// `spawn` has no target.
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    pub from: Source,
    pub signal: String,
    pub to: Target,
    pub action: ConnectAction,
    /// With add/set.
    pub prop: Option<String>,
    pub value: Option<PropValue>,
    /// With spawn.
    pub prefab: Option<String>,
    /// All must hold when the signal arrives.
    pub when: Vec<Condition>,
}

impl Connection {
    /// Does this connection listen to `signal` from `emitter` (`None` = the ground)?
    fn listens(&self, world: &World, emitter: Option<EntityId>, signal: &str) -> bool {
        self.signal == signal
            && match (&self.from, emitter) {
                (Source::Entity(id), Some(e)) => *id == e,
                (Source::Prefab(p), Some(e)) => world.get(e).and_then(|x| x.prefab.as_deref()) == Some(p.as_str()),
                (Source::Ground, None) => true,
                _ => false,
            }
    }
}

/// A queued signal: who emitted it (`None` = the ground) and the cell it
/// happened on (the tapped cell, or the emitter's cell).
#[derive(Clone, Debug, PartialEq)]
struct Pending {
    emitter: Option<EntityId>,
    signal: String,
    cell: [i32; 2],
}

/// Why a spawn could not be placed.
#[derive(Clone, Debug, PartialEq)]
pub struct Refusal {
    /// `edge`, `terrain` or `occupied`.
    pub reason: &'static str,
    /// The first cell of the footprint that failed.
    pub cell: [i32; 2],
    /// The entity covering that cell (for `occupied`).
    pub by: Option<EntityId>,
    /// The terrain there (for `terrain`).
    pub terrain: Option<String>,
    /// "cell (4, 4) is water (build = false)".
    pub why: String,
}

/// A `when` condition with its entity resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Condition {
    pub entity: EntityId,
    pub def: ConditionDef,
}

/// Game-specific systems written in Rust. They run every step after the
/// built-in behaviors and before triggers and signal dispatch.
pub trait Systems {
    fn tick(&mut self, _world: &mut World, _input: &InputFrame) {}
}

/// No custom systems (data-only projects).
#[derive(Debug, Default)]
pub struct NoSystems;
impl Systems for NoSystems {}

#[derive(Clone, Debug)]
pub struct World {
    /// Number of steps taken. Frame N is the state after N steps.
    pub frame: u64,
    pub seed: u64,
    pub rng: Rng,
    pub entities: Vec<Entity>,
    /// Ground size: movement is limited to 0..size on each axis.
    pub bounds: Option<[i32; 2]>,
    /// The ground with its terrain (placement rules read it).
    pub ground: Option<GroundDef>,
    pub connections: Vec<Connection>,
    pub events: EventLog,
    /// Prefabs that `spawn` actions place (their `prefab` is the name).
    pub templates: Vec<EntityDef>,
    pending: VecDeque<Pending>,
    names: BTreeMap<String, EntityId>,
}

/// A runtime entity from its definition.
fn make_entity(id: EntityId, def: &EntityDef) -> Entity {
    Entity {
        id,
        name: def.name.clone(),
        prefab: def.prefab.clone(),
        enabled: def.enabled,
        pos: def.pos,
        footprint: def.footprint,
        solid: def.solid,
        pushable: def.pushable,
        tile: def.tile,
        block: def.block,
        sprite: def.sprite,
        mover: def.mover.map(|m| Mover { step_frames: m.step_frames, cooldown: 0 }),
        patrol: def.patrol.clone().map(|p| Patrol { def: p, target: 0, dir: 1, timer: 0 }),
        wander: def.wander.map(|w| Wander { def: w, timer: 0 }),
        trigger: def.trigger.clone().map(|t| Trigger { def: t, occupants: Vec::new() }),
        label: def.label.clone(),
        screen_pos: def.screen_pos,
        props: def.props.iter().cloned().collect(),
        allow: def.allow.clone(),
    }
}

impl World {
    /// Build the world for frame 0. The scene must have passed validation.
    pub fn new(scene: &SceneDef, seed: u64) -> World {
        let mut entities = Vec::with_capacity(scene.entities.len());
        let mut names = BTreeMap::new();
        for (i, def) in scene.entities.iter().enumerate() {
            let id = i as EntityId;
            if let Some(n) = &def.name {
                names.insert(n.clone(), id);
            }
            entities.push(make_entity(id, def));
        }
        let connections = scene
            .connections
            .iter()
            .filter_map(|c| {
                let when = c
                    .when
                    .iter()
                    .map(|w| Some(Condition { entity: *names.get(&w.entity)?, def: w.clone() }))
                    .collect::<Option<Vec<_>>>()?;
                let from = match &c.from {
                    ConnectFrom::Entity(n) => Source::Entity(*names.get(n)?),
                    ConnectFrom::Prefab(p) => Source::Prefab(p.clone()),
                    ConnectFrom::Ground => Source::Ground,
                };
                let to = match (c.action, c.to.as_deref()) {
                    (ConnectAction::Spawn, _) => Target::None,
                    (_, Some("self")) => Target::Emitter,
                    (_, Some(n)) => Target::Entity(*names.get(n)?),
                    (_, None) => return None,
                };
                Some(Connection {
                    from,
                    signal: c.signal.clone(),
                    to,
                    action: c.action,
                    prop: c.prop.clone(),
                    value: c.value.clone(),
                    prefab: c.prefab.clone(),
                    when,
                })
            })
            .collect();
        let templates = scene.templates.clone();
        let mut world = World {
            frame: 0,
            seed,
            rng: Rng::new(seed),
            entities,
            bounds: scene.ground.as_ref().map(|g| g.size),
            ground: scene.ground.clone(),
            connections,
            events: EventLog::default(),
            templates,
            pending: VecDeque::new(),
            names,
        };
        // Initial trigger occupancy, without emitting: a plate that starts
        // occupied does not fire at frame 0.
        for id in world.ids_with(|e| e.trigger.is_some()) {
            let occ = world.occupants_of(id);
            if let Some(t) = world.entities[id as usize].trigger.as_mut() {
                t.occupants = occ;
            }
        }
        world
    }

    pub fn find(&self, name: &str) -> Option<EntityId> {
        self.names.get(name).copied()
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(id as usize)
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.entities.get_mut(id as usize)
    }

    pub fn by_name(&self, name: &str) -> Option<&Entity> {
        self.find(name).and_then(|id| self.get(id))
    }

    fn ids_with(&self, f: impl Fn(&Entity) -> bool) -> Vec<EntityId> {
        self.entities.iter().filter(|e| f(e)).map(|e| e.id).collect()
    }

    fn describe(&self, id: EntityId) -> String {
        self.get(id).map_or_else(|| format!("#{id}"), Entity::describe)
    }

    /// Append an event for the current frame.
    pub fn log(
        &mut self,
        kind: EventKind,
        entity: Option<EntityId>,
        other: Option<EntityId>,
        message: String,
        data: Json,
    ) {
        let signal = data.get("signal").and_then(Json::as_str).map(str::to_string);
        self.events.push(Event { frame: self.frame, kind, entity, other, signal, message, data });
    }

    /// Queue a signal; it is dispatched at the end of the current step.
    pub fn emit(&mut self, from: EntityId, signal: &str) {
        let cell = self.get(from).map_or([0, 0], Entity::cell);
        self.pending.push_back(Pending { emitter: Some(from), signal: signal.to_string(), cell });
    }

    /// Is any connection listening to `signal` from `emitter` (`None` = the ground)?
    pub fn is_listened(&self, emitter: Option<EntityId>, signal: &str) -> bool {
        self.connections.iter().any(|c| c.listens(self, emitter, signal))
    }

    /// Advance one fixed step: input, movers, patrols, wanderers, custom
    /// systems, triggers, then signal dispatch. The order is part of the
    /// determinism contract (docs/architecture.md#the-step).
    pub fn tick(&mut self, input: &InputFrame, systems: &mut dyn Systems) {
        self.apply_input(input);
        self.run_movers(input);
        self.run_patrols();
        self.run_wanders();
        systems.tick(self, input);
        self.run_triggers();
        self.dispatch_signals();
        self.frame += 1;
    }

    fn apply_input(&mut self, input: &InputFrame) {
        if !input.pressed.is_empty() {
            let names = input.pressed.names();
            self.log(
                EventKind::Input,
                None,
                None,
                format!("pressed {}", names.join(", ")),
                Json::object().with("pressed", names.iter().map(|n| Json::from(*n)).collect::<Vec<_>>()),
            );
        }
        for tap in &input.taps {
            let at = format!("({}, {})", tap.x, tap.y);
            let data = Json::object().with("x", tap.x).with("y", tap.y);
            match tap.hit {
                Some(id) => {
                    let msg = format!("tap at {at} hit {}; it emits 'tapped'", self.describe(id));
                    self.log(EventKind::Tap, Some(id), None, msg, data.with("signal", "tapped"));
                    self.emit(id, "tapped");
                }
                None if tap.ground.is_some() && self.is_listened(None, "tapped") => {
                    let cell = tap.ground.expect("ground cell");
                    let mut msg = format!("tap at {at} hit ground cell {}", fmt_cell(cell));
                    if let Some(t) = self.terrain_name(cell) {
                        msg.push_str(&format!(" ({t})"));
                    }
                    msg.push_str("; the ground emits 'tapped'");
                    self.log(EventKind::Tap, None, None, msg, data.with("cell", cell).with("signal", "tapped"));
                    self.pending.push_back(Pending { emitter: None, signal: "tapped".into(), cell });
                }
                None => {
                    let mut msg = format!("tap at {at} hit nothing enabled");
                    if let Some(cell) = tap.ground {
                        msg.push_str(&format!(
                            " (ground cell {}; nothing listens to the ground's 'tapped')",
                            fmt_cell(cell)
                        ));
                    }
                    if let Some(&d) = tap.under_disabled.first() {
                        msg.push_str(&format!("; disabled {} is under the pointer", self.describe(d)));
                    }
                    let under: Vec<Json> = tap.under_disabled.iter().map(|id| Json::from(*id)).collect();
                    let mut data = data.with("under_disabled", under);
                    if let Some(cell) = tap.ground {
                        data.set("cell", cell);
                    }
                    self.log(EventKind::TapMiss, tap.under_disabled.first().copied(), None, msg, data);
                }
            }
        }
    }

    /// The entity that stops something entering `cell`, if any. Pushables
    /// count as solid here (only movers can push them, see `try_move`).
    pub fn blocker_at(&self, cell: [i32; 2], except: EntityId) -> Option<(EntityId, &'static str)> {
        self.blockers_at(cell, except).into_iter().next()
    }

    /// Every entity that stops something entering `cell`, in id order.
    fn blockers_at(&self, cell: [i32; 2], except: EntityId) -> Vec<(EntityId, &'static str)> {
        self.entities
            .iter()
            .filter(|e| e.id != except && e.enabled && e.in_world() && e.covers(cell))
            .filter_map(|e| {
                if e.solid || e.pushable {
                    Some((e.id, "solid"))
                } else if e.is_actor() {
                    Some((e.id, "actor"))
                } else {
                    None
                }
            })
            .collect()
    }

    fn outside(&self, cell: [i32; 2]) -> Option<[i32; 2]> {
        let [w, h] = self.bounds?;
        (cell[0] < 0 || cell[1] < 0 || cell[0] >= w || cell[1] >= h).then_some([w, h])
    }

    /// Move one cell. An entity with a mover pushes a pushable standing in
    /// the way, if the cell beyond it is free. Every refusal is logged with
    /// its reason.
    pub fn try_move(&mut self, id: EntityId, dx: i32, dy: i32, cause: &str) -> bool {
        let Some(e) = self.get(id) else { return false };
        let from = e.cell();
        let to = [from[0] + dx, from[1] + dy];
        let who = e.describe();
        let can_push = e.mover.is_some();
        let base = Json::object().with("cause", cause).with("from", from).with("to", to);
        if let Some([w, h]) = self.outside(to) {
            let msg = format!(
                "{who} could not move {cause} from {} to {}: outside the {w}x{h} ground",
                fmt_cell(from),
                fmt_cell(to)
            );
            self.log(EventKind::Blocked, Some(id), None, msg, base.with("reason", "edge"));
            return false;
        }
        let blockers = self.blockers_at(to, id);
        if let [(crate_id, _)] = blockers.as_slice()
            && can_push
            && self.entities[*crate_id as usize].pushable
        {
            let crate_id = *crate_id;
            let beyond = [to[0] + dx, to[1] + dy];
            let what = self.describe(crate_id);
            let stuck = match self.outside(beyond) {
                Some([w, h]) => Some((None, format!("outside the {w}x{h} ground"), "edge")),
                None => self.blocker_at(beyond, crate_id).map(|(b, reason)| {
                    let kind = if self.entities[b as usize].pushable { "another pushable" } else { reason };
                    (Some(b), format!("blocked by {kind} {}", self.describe(b)), reason)
                }),
            };
            if let Some((by, why, reason)) = stuck {
                let msg = format!(
                    "{who} could not move {cause} from {} to {}: {what} cannot be pushed to {}, {why}",
                    fmt_cell(from),
                    fmt_cell(to),
                    fmt_cell(beyond)
                );
                let data = base.with("reason", "push").with("beyond", beyond).with("push_blocked", reason);
                let data = match by {
                    Some(b) => data.with("push_blocked_by", self.get(b).map(Entity::label)),
                    None => data,
                };
                self.log(EventKind::Blocked, Some(id), Some(crate_id), msg, data);
                return false;
            }
            let c = &mut self.entities[crate_id as usize];
            c.pos[0] = beyond[0] as f32;
            c.pos[1] = beyond[1] as f32;
            let msg = format!("{what} was pushed {cause} from {} to {} by {who}", fmt_cell(to), fmt_cell(beyond));
            let data = Json::object().with("cause", "pushed").with("from", to).with("to", beyond).with("by", id);
            self.log(EventKind::Moved, Some(crate_id), Some(id), msg, data);
        } else if let Some(&(blocker, reason)) = blockers.first() {
            let pushable = self.entities[blocker as usize].pushable;
            let what = if pushable { "pushable" } else { reason };
            let note = if pushable && !can_push { " (only entities with a mover push)" } else { "" };
            let msg = format!(
                "{who} could not move {cause} from {} to {}: blocked by {what} {}{note}",
                fmt_cell(from),
                fmt_cell(to),
                self.describe(blocker)
            );
            self.log(EventKind::Blocked, Some(id), Some(blocker), msg, base.with("reason", reason));
            return false;
        }
        let e = &mut self.entities[id as usize];
        e.pos[0] = to[0] as f32;
        e.pos[1] = to[1] as f32;
        let msg = format!("{who} moved {cause} from {} to {}", fmt_cell(from), fmt_cell(to));
        self.log(EventKind::Moved, Some(id), None, msg, base);
        true
    }

    fn run_movers(&mut self, input: &InputFrame) {
        for id in self.ids_with(|e| e.enabled && e.mover.is_some()) {
            let mover = self.entities[id as usize].mover.as_mut().expect("mover");
            mover.cooldown = mover.cooldown.saturating_sub(1);
            let (cooldown, step) = (mover.cooldown, mover.step_frames);
            let Some(dir) = Action::DIRECTIONS.into_iter().find(|a| input.down.contains(*a)) else {
                continue;
            };
            if cooldown == 0 {
                let (dx, dy) = dir.delta().expect("direction");
                self.try_move(id, dx, dy, dir.name());
                if let Some(m) = self.entities[id as usize].mover.as_mut() {
                    m.cooldown = step;
                }
            } else if input.pressed.contains(dir) {
                let who = self.describe(id);
                let msg = format!(
                    "{who} ignored '{}': still waiting {cooldown} frame(s) after its last move (mover.step_frames = {step})",
                    dir.name()
                );
                let data =
                    Json::object().with("cause", dir.name()).with("reason", "cooldown").with("frames_left", cooldown);
                self.log(EventKind::Blocked, Some(id), None, msg, data);
            }
        }
    }

    fn run_patrols(&mut self) {
        for id in self.ids_with(|e| e.enabled && e.patrol.is_some()) {
            let cell = self.entities[id as usize].cell();
            let patrol = self.entities[id as usize].patrol.as_mut().expect("patrol");
            patrol.timer += 1;
            if patrol.timer < patrol.def.step_frames {
                continue;
            }
            patrol.timer = 0;
            if patrol.def.path.len() < 2 && patrol.def.path.first() == Some(&cell) {
                continue;
            }
            if patrol.def.path.get(patrol.target) == Some(&cell) {
                advance(patrol);
            }
            let target = patrol.def.path[patrol.target];
            let (dx, dy) = step_toward(cell, target);
            if (dx, dy) != (0, 0) && self.try_move(id, dx, dy, "patrol") {
                let new_cell = self.entities[id as usize].cell();
                let patrol = self.entities[id as usize].patrol.as_mut().expect("patrol");
                if new_cell == target {
                    advance(patrol);
                }
            }
        }
    }

    fn run_wanders(&mut self) {
        for id in self.ids_with(|e| e.enabled && e.wander.is_some()) {
            let wander = self.entities[id as usize].wander.as_mut().expect("wander");
            wander.timer += 1;
            if wander.timer < wander.def.step_frames {
                continue;
            }
            wander.timer = 0;
            let area = wander.def.area;
            // 0 = stay, 1..=4 = up, down, left, right. One draw per step,
            // in entity order: part of the determinism contract.
            let roll = self.rng.below(5);
            if roll == 0 {
                continue;
            }
            let dir = Action::DIRECTIONS[(roll - 1) as usize];
            let (dx, dy) = dir.delta().expect("direction");
            let cell = self.entities[id as usize].cell();
            let to = [cell[0] + dx, cell[1] + dy];
            if let Some([lo, hi]) = area
                && (to[0] < lo[0] || to[1] < lo[1] || to[0] > hi[0] || to[1] > hi[1])
            {
                let msg = format!(
                    "{} stayed: {} is outside its wander area {}..{}",
                    self.describe(id),
                    fmt_cell(to),
                    fmt_cell(lo),
                    fmt_cell(hi)
                );
                let data =
                    Json::object().with("cause", "wander").with("reason", "area").with("from", cell).with("to", to);
                self.log(EventKind::Blocked, Some(id), None, msg, data);
                continue;
            }
            self.try_move(id, dx, dy, "wander");
        }
    }

    /// Enabled actors on an entity's cell, other than itself. For a trigger,
    /// only those its `only` accepts.
    pub fn occupants_of(&self, id: EntityId) -> Vec<EntityId> {
        let t = &self.entities[id as usize];
        let only = t.trigger.as_ref().and_then(|tr| tr.def.only.as_ref());
        self.entities
            .iter()
            .filter(|e| e.id != id && e.enabled && e.holds_plates() && t.covers(e.cell()))
            .filter(|e| only.is_none_or(|only| e.name.as_ref() == Some(only)))
            .map(|e| e.id)
            .collect()
    }

    /// Evaluate a condition now: whether it holds, and what was seen.
    pub fn check_condition(&self, c: &Condition) -> (bool, String) {
        let Some(e) = self.get(c.entity) else {
            return (false, "entity missing".into());
        };
        match &c.def.test {
            CondTest::Occupied(want) => {
                let occ = self.occupants_of(c.entity);
                let seen = match occ.first() {
                    Some(&o) => format!("{} is on {}", self.describe(o), fmt_cell(e.cell())),
                    None => format!("nobody is on {}", fmt_cell(e.cell())),
                };
                (occ.is_empty() != *want, seen)
            }
            CondTest::Enabled(want) => {
                (e.enabled == *want, if e.enabled { "it is enabled".into() } else { "it is disabled".into() })
            }
            CondTest::Prop { prop, equals, at_least, at_most } => {
                let Some(v) = e.props.get(prop) else {
                    return (false, format!("{prop} is not set"));
                };
                let n = v.as_num();
                let held = equals.as_ref().is_none_or(|x| v.same(x))
                    && at_least.is_none_or(|lo| n.is_some_and(|n| n >= lo))
                    && at_most.is_none_or(|hi| n.is_some_and(|n| n <= hi));
                (held, format!("{prop} is {}", v.literal()))
            }
        }
    }

    fn run_triggers(&mut self) {
        for id in self.ids_with(|e| e.enabled && e.trigger.is_some()) {
            let now = self.occupants_of(id);
            let trigger = self.entities[id as usize].trigger.as_mut().expect("trigger");
            let was = std::mem::replace(&mut trigger.occupants, now.clone());
            let (on_enter, on_exit) = (trigger.def.on_enter.clone(), trigger.def.on_exit.clone());
            let cell = self.entities[id as usize].cell();
            if was.is_empty() && !now.is_empty() {
                let msg = format!(
                    "{} at {} was entered by {}; it emits '{on_enter}'",
                    self.describe(id),
                    fmt_cell(cell),
                    self.describe(now[0])
                );
                let data = Json::object().with("signal", on_enter.as_str()).with("by", now[0]);
                self.log(EventKind::Trigger, Some(id), Some(now[0]), msg, data);
                self.emit(id, &on_enter);
            } else if !was.is_empty() && now.is_empty() {
                let msg = format!(
                    "{} at {} was left by {}; it emits '{on_exit}'",
                    self.describe(id),
                    fmt_cell(cell),
                    self.describe(was[0])
                );
                let data = Json::object().with("signal", on_exit.as_str()).with("by", was[0]);
                self.log(EventKind::Trigger, Some(id), Some(was[0]), msg, data);
                self.emit(id, &on_exit);
            }
        }
    }

    fn source_name(&self, emitter: Option<EntityId>) -> String {
        match emitter {
            Some(id) => self.describe(id),
            None => "the ground".into(),
        }
    }

    fn dispatch_signals(&mut self) {
        let mut dispatched = 0;
        while let Some(Pending { emitter, signal, cell }) = self.pending.pop_front() {
            dispatched += 1;
            if dispatched > SIGNAL_LIMIT {
                let msg = format!(
                    "more than {SIGNAL_LIMIT} signals in one frame (last: '{signal}' from {}); dropping the rest. \
                     This is usually a loop of connections (E0301)",
                    self.source_name(emitter)
                );
                self.log(
                    EventKind::Error,
                    emitter,
                    None,
                    msg,
                    Json::object().with("code", "E0301").with("signal", signal.as_str()),
                );
                self.pending.clear();
                break;
            }
            let targets: Vec<Connection> =
                self.connections.iter().filter(|c| c.listens(self, emitter, &signal)).cloned().collect();
            let source = self.source_name(emitter);
            let msg = if targets.is_empty() {
                let hint = match emitter.and_then(|id| self.get(id)) {
                    Some(e) => match (&e.name, &e.prefab) {
                        (None, Some(p)) => format!("from_prefab = \"{p}\""),
                        _ => format!("from = \"{}\"", e.label()),
                    },
                    None => "from = \"ground\"".to_string(),
                };
                format!(
                    "{source} emitted '{signal}' but nothing is connected to it (add [[connect]] {hint} signal = \"{signal}\")"
                )
            } else {
                format!("{source} emitted '{signal}' to {} connection(s)", targets.len())
            };
            let data = Json::object().with("signal", signal.as_str()).with("connections", targets.len());
            self.log(EventKind::Signal, emitter, None, msg, data);
            for c in targets {
                self.run_connection(&c, emitter, &signal, cell);
            }
        }
    }

    /// Check a connection's conditions, then apply its action or log why not.
    fn run_connection(&mut self, c: &Connection, emitter: Option<EntityId>, signal: &str, cell: [i32; 2]) {
        let to = match c.to {
            Target::Entity(id) => Some(id),
            Target::Emitter => emitter,
            Target::None => None,
        };
        let results: Vec<(String, bool, String)> = c
            .when
            .iter()
            .map(|w| {
                let (held, seen) = self.check_condition(w);
                (w.def.describe(), held, seen)
            })
            .collect();
        let when_json: Vec<Json> = results
            .iter()
            .map(|(cond, held, seen)| {
                Json::object().with("condition", cond.as_str()).with("held", *held).with("seen", seen.as_str())
            })
            .collect();
        let target = match (to, &c.prefab) {
            (Some(id), _) => self.describe(id),
            (None, Some(p)) => format!("spawn '{p}'"),
            (None, None) => "nothing".into(),
        };
        let via = match emitter.and_then(|id| self.get(id)) {
            Some(e) => format!("via {}.'{signal}'", e.label()),
            None => format!("via the ground's '{signal}' at {}", fmt_cell(cell)),
        };
        let mut data = Json::object().with("signal", signal).with("action", c.action.name());
        if let Some(p) = &c.prop {
            data.set("prop", p.as_str());
        }
        if let Some(p) = &c.prefab {
            data.set("prefab", p.as_str());
            data.set("cell", cell);
        }
        if !results.is_empty() {
            data.set("when", when_json);
        }
        // For spawn the subject is nothing yet; the emitter is the other party.
        let (subject, other) = match to {
            Some(id) => (Some(id), emitter),
            None => (None, emitter),
        };
        if let Some((cond, _, seen)) = results.iter().find(|(_, held, _)| !held) {
            let msg = if c.action == ConnectAction::Spawn {
                format!("{target} at {} skipped ({via}): {cond} does not hold ({seen})", fmt_cell(cell))
            } else {
                format!("{target} {} skipped ({via}): {cond} does not hold ({seen})", c.action.name())
            };
            self.log(EventKind::Skipped, subject, other, msg, data);
            return;
        }
        if c.action == ConnectAction::Spawn {
            let held: Vec<&str> = results.iter().map(|(cond, _, _)| cond.as_str()).collect();
            self.spawn(c.prefab.as_deref().unwrap_or_default(), cell, emitter, &via, &held);
            return;
        }
        let Some(to) = to else { return };
        let (change, prop_changed) = match c.action {
            ConnectAction::Spawn => return,
            ConnectAction::Add | ConnectAction::Set => match self.change_prop(c, to) {
                Some((change, before, value, after)) => {
                    data = data
                        .with("value", prop_json(&value))
                        .with("before", prop_json(&before))
                        .with("after", prop_json(&after))
                        .with("changed", after != before);
                    (change, after != before)
                }
                None => return,
            },
            ConnectAction::Enable | ConnectAction::Disable | ConnectAction::Toggle => {
                let Some(e) = self.entities.get_mut(to as usize) else { return };
                let before = e.enabled;
                e.enabled = match c.action {
                    ConnectAction::Enable => true,
                    ConnectAction::Disable => false,
                    _ => !before,
                };
                let after = e.enabled;
                data = data.with("before", before).with("after", after).with("changed", before != after);
                let state = |b: bool| if b { "enabled" } else { "disabled" };
                let change = if before == after {
                    format!("{}: stays {}", c.action.name(), state(after))
                } else {
                    format!("{}: {} -> {}", c.action.name(), state(before), state(after))
                };
                (change, false)
            }
        };
        let mut msg = format!("{target} {change} ({via})");
        if !results.is_empty() {
            let held: Vec<&str> = results.iter().map(|(cond, _, _)| cond.as_str()).collect();
            msg.push_str(&format!("; when {} held", held.join(", ")));
        }
        self.log(EventKind::Action, Some(to), emitter, msg, data);
        // A prop change makes its owner emit `changed`, if anything listens.
        if prop_changed && self.is_listened(Some(to), "changed") {
            self.emit(to, "changed");
        }
    }

    /// The spawn template for `prefab`, if a `spawn` action uses it.
    pub fn template(&self, prefab: &str) -> Option<&EntityDef> {
        self.templates.iter().find(|t| t.prefab.as_deref() == Some(prefab))
    }

    fn terrain_name(&self, cell: [i32; 2]) -> Option<String> {
        let g = self.ground.as_ref().filter(|g| g.has_map)?;
        g.terrain_at(cell).map(|t| t.name.clone())
    }

    /// Can `prefab` be placed with its back corner on `at` right now? Returns
    /// the cells it would cover, or why not. Every covered cell must be inside
    /// the ground, on buildable terrain, and not covered by an enabled entity.
    pub fn can_place(&self, prefab: &str, at: [i32; 2]) -> Result<Vec<[i32; 2]>, Refusal> {
        let footprint = self.template(prefab).map_or([1, 1], |t| t.footprint);
        let cells = footprint_cells(at, footprint);
        for &c in &cells {
            match &self.ground {
                Some(g) if !g.contains(c) => {
                    let why = format!("cell {} is outside the {}x{} ground", fmt_cell(c), g.size[0], g.size[1]);
                    return Err(Refusal { reason: "edge", cell: c, by: None, terrain: None, why });
                }
                Some(g) => {
                    if let Some(t) = g.terrain_at(c).filter(|t| !t.build) {
                        let why =
                            format!("cell {} is {}, where nothing can be built (build = false)", fmt_cell(c), t.name);
                        return Err(Refusal {
                            reason: "terrain",
                            cell: c,
                            by: None,
                            terrain: Some(t.name.clone()),
                            why,
                        });
                    }
                }
                None => {}
            }
            if let Some(e) = self.entities.iter().find(|e| e.takes_space() && e.covers(c)) {
                let why = format!("cell {} is taken by {}", fmt_cell(c), e.describe());
                return Err(Refusal { reason: "occupied", cell: c, by: Some(e.id), terrain: None, why });
            }
        }
        Ok(cells)
    }

    /// Place a new copy of `prefab` at `at`, or log why not. The new entity
    /// emits `spawned` if a connection listens.
    fn spawn(&mut self, prefab: &str, at: [i32; 2], emitter: Option<EntityId>, via: &str, held: &[&str]) {
        let when = if held.is_empty() { String::new() } else { format!("; when {} held", held.join(", ")) };
        let Some(template) = self.template(prefab).cloned() else {
            let msg = format!("could not spawn '{prefab}' at {} ({via}): no such prefab", fmt_cell(at));
            let data = Json::object().with("code", "E0201").with("prefab", prefab);
            self.log(EventKind::Error, None, emitter, msg, data);
            return;
        };
        match self.can_place(prefab, at) {
            Err(r) => {
                let msg = format!("'{prefab}' could not be placed at {} ({via}): {}{when}", fmt_cell(at), r.why);
                let mut data = Json::object()
                    .with("cause", "spawn")
                    .with("prefab", prefab)
                    .with("at", at)
                    .with("reason", r.reason)
                    .with("cell", r.cell);
                if let Some(t) = &r.terrain {
                    data.set("terrain", t.as_str());
                }
                self.log(EventKind::Blocked, None, r.by, msg, data);
            }
            Ok(cells) => {
                let id = self.entities.len() as EntityId;
                let mut def = template;
                def.pos = [at[0] as f32, at[1] as f32, def.pos[2]];
                self.entities.push(make_entity(id, &def));
                let who = self.describe(id);
                let msg = format!("{who} was spawned at {} ({via}){when}", fmt_cell(at));
                let cells_json: Vec<Json> = cells.iter().map(|c| Json::from(*c)).collect();
                let data = Json::object().with("prefab", prefab).with("cell", at).with("cells", cells_json);
                self.log(EventKind::Spawned, Some(id), emitter, msg, data);
                if self.is_listened(Some(id), "spawned") {
                    self.emit(id, "spawned");
                }
            }
        }
    }

    /// Apply `add`/`set`. Returns the message part, before, value and after.
    /// Props keep their declared type (validated by `slate check`).
    fn change_prop(&mut self, c: &Connection, to: EntityId) -> Option<(String, PropValue, PropValue, PropValue)> {
        let prop = c.prop.as_deref()?;
        let value = c.value.clone().unwrap_or(PropValue::Int(1));
        let current = self.entities.get_mut(to as usize)?.props.get_mut(prop)?;
        let before = current.clone();
        let after = match (c.action, &before) {
            (ConnectAction::Add, PropValue::Int(a)) => match value {
                PropValue::Int(b) => PropValue::Int(a.saturating_add(b)),
                _ => before.clone(),
            },
            (ConnectAction::Add, PropValue::Float(a)) => PropValue::Float(a + value.as_num().unwrap_or(0.0)),
            (ConnectAction::Set, _) if before.accepts(&value) => before.coerce(&value),
            _ => before.clone(),
        };
        *current = after.clone();
        let what = match c.action {
            ConnectAction::Add => format!("add {prop} {}", value.display()),
            _ => format!("set {prop} = {}", value.literal()),
        };
        let change = if after == before {
            format!("{what}: stays {}", after.literal())
        } else {
            format!("{what}: {} -> {}", before.literal(), after.literal())
        };
        Some((change, before, value, after))
    }

    /// Hash of the full simulation state. Equal hashes mean equal worlds:
    /// the cheap way to compare runs across machines and builds.
    pub fn state_hash(&self) -> u64 {
        let mut h = Hasher64::new();
        h.u64(self.frame).u64(self.seed);
        let (s, inc) = self.rng.state();
        h.u64(s).u64(inc);
        // A terrain map is part of the world (plain ground is not, so older
        // scenes keep their hashes).
        if let Some(g) = self.ground.as_ref().filter(|g| g.has_map) {
            h.u8(0xA0).i64(g.size[0] as i64).i64(g.size[1] as i64).bytes(&g.cells);
            for t in &g.terrains {
                h.str(&t.name).bool(t.build);
            }
        }
        for e in &self.entities {
            h.u32(e.id).bool(e.enabled).bool(e.solid);
            if e.pushable {
                h.u8(10);
            }
            if e.footprint != [1, 1] {
                h.u8(11).i64(e.footprint[0] as i64).i64(e.footprint[1] as i64);
            }
            h.f32(e.pos[0]).f32(e.pos[1]).f32(e.pos[2]);
            if let Some(t) = &e.tile {
                h.u8(1).bytes(&[t.color.r, t.color.g, t.color.b, t.color.a]);
            }
            if let Some(b) = &e.block {
                h.u8(2).bytes(&[b.color.r, b.color.g, b.color.b, b.color.a]).f32(b.height);
                if b.top != b.color {
                    h.u8(12).bytes(&[b.top.r, b.top.g, b.top.b, b.top.a]);
                }
            }
            if let Some(s) = &e.sprite {
                h.u8(3).bytes(&[s.color.r, s.color.g, s.color.b, s.color.a]).f32(s.size[0]).f32(s.size[1]);
            }
            if let Some(m) = &e.mover {
                h.u8(4).u32(m.cooldown);
            }
            if let Some(p) = &e.patrol {
                h.u8(5).u64(p.target as u64).i64(p.dir as i64).u32(p.timer);
            }
            if let Some(w) = &e.wander {
                h.u8(6).u32(w.timer);
            }
            if let Some(t) = &e.trigger {
                h.u8(7).u64(t.occupants.len() as u64);
                for o in &t.occupants {
                    h.u32(*o);
                }
            }
            if let Some(l) = &e.label {
                h.u8(8).str(&l.text).bytes(&[l.color.r, l.color.g, l.color.b, l.color.a]).u32(l.scale);
            }
            if let Some([x, y]) = e.screen_pos {
                h.u8(9).f32(x).f32(y);
            }
            for (k, v) in &e.props {
                h.str(k);
                match v {
                    PropValue::Bool(b) => h.u8(0).bool(*b),
                    PropValue::Int(i) => h.u8(1).i64(*i),
                    PropValue::Float(f) => h.u8(2).f64(*f),
                    PropValue::Str(s) => h.u8(3).str(s),
                };
            }
        }
        h.finish()
    }
}

fn advance(p: &mut Patrol) {
    let n = p.def.path.len();
    if n < 2 {
        return;
    }
    match p.def.mode {
        PatrolMode::Loop => p.target = (p.target + 1) % n,
        PatrolMode::PingPong => {
            let next = p.target as i64 + p.dir as i64;
            if next < 0 || next >= n as i64 {
                p.dir = -p.dir;
            }
            p.target = (p.target as i64 + p.dir as i64) as usize;
        }
    }
}

/// One axis at a time, x first.
fn step_toward(from: [i32; 2], to: [i32; 2]) -> (i32, i32) {
    if from[0] != to[0] { ((to[0] - from[0]).signum(), 0) } else { (0, (to[1] - from[1]).signum()) }
}

pub fn fmt_cell(c: [i32; 2]) -> String {
    format!("({}, {})", c[0], c[1])
}
