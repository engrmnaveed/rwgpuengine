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
    Action, BlockDef, ConnectAction, PatrolDef, PatrolMode, PropValue, SceneDef, SpriteDef, TileDef, TriggerDef,
    WanderDef, round_cell,
};

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
    pub solid: bool,
    pub tile: Option<TileDef>,
    pub block: Option<BlockDef>,
    pub sprite: Option<SpriteDef>,
    pub mover: Option<Mover>,
    pub patrol: Option<Patrol>,
    pub wander: Option<Wander>,
    pub trigger: Option<Trigger>,
    pub props: BTreeMap<String, PropValue>,
    /// Warning codes intended for this entity (not reported).
    pub allow: Vec<String>,
}

impl Entity {
    pub fn cell(&self) -> [i32; 2] {
        [round_cell(self.pos[0]), round_cell(self.pos[1])]
    }

    pub fn is_actor(&self) -> bool {
        self.mover.is_some() || self.patrol.is_some() || self.wander.is_some()
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

#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    pub from: EntityId,
    pub signal: String,
    pub to: EntityId,
    pub action: ConnectAction,
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
    pub connections: Vec<Connection>,
    pub events: EventLog,
    pending: VecDeque<(EntityId, String)>,
    names: BTreeMap<String, EntityId>,
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
            entities.push(Entity {
                id,
                name: def.name.clone(),
                prefab: def.prefab.clone(),
                enabled: def.enabled,
                pos: def.pos,
                solid: def.solid,
                tile: def.tile,
                block: def.block,
                sprite: def.sprite,
                mover: def.mover.map(|m| Mover { step_frames: m.step_frames, cooldown: 0 }),
                patrol: def.patrol.clone().map(|p| Patrol { def: p, target: 0, dir: 1, timer: 0 }),
                wander: def.wander.map(|w| Wander { def: w, timer: 0 }),
                trigger: def.trigger.clone().map(|t| Trigger { def: t, occupants: Vec::new() }),
                props: def.props.iter().cloned().collect(),
                allow: def.allow.clone(),
            });
        }
        let connections = scene
            .connections
            .iter()
            .filter_map(|c| {
                Some(Connection {
                    from: *names.get(&c.from)?,
                    signal: c.signal.clone(),
                    to: *names.get(&c.to)?,
                    action: c.action,
                })
            })
            .collect();
        let mut world = World {
            frame: 0,
            seed,
            rng: Rng::new(seed),
            entities,
            bounds: scene.ground.map(|g| g.size),
            connections,
            events: EventLog::default(),
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
        self.pending.push_back((from, signal.to_string()));
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
                None => {
                    let mut msg = format!("tap at {at} hit nothing enabled");
                    if let Some(&d) = tap.under_disabled.first() {
                        msg.push_str(&format!("; disabled {} is under the pointer", self.describe(d)));
                    }
                    let under: Vec<Json> = tap.under_disabled.iter().map(|id| Json::from(*id)).collect();
                    self.log(
                        EventKind::TapMiss,
                        tap.under_disabled.first().copied(),
                        None,
                        msg,
                        data.with("under_disabled", under),
                    );
                }
            }
        }
    }

    /// The entity that stops something entering `cell`, if any.
    pub fn blocker_at(&self, cell: [i32; 2], except: EntityId) -> Option<(EntityId, &'static str)> {
        self.entities.iter().filter(|e| e.id != except && e.enabled && e.cell() == cell).find_map(|e| {
            if e.solid {
                Some((e.id, "solid"))
            } else if e.is_actor() {
                Some((e.id, "actor"))
            } else {
                None
            }
        })
    }

    /// Move one cell. Every refusal is logged with its reason.
    pub fn try_move(&mut self, id: EntityId, dx: i32, dy: i32, cause: &str) -> bool {
        let Some(e) = self.get(id) else { return false };
        let from = e.cell();
        let to = [from[0] + dx, from[1] + dy];
        let who = e.describe();
        let base = Json::object().with("cause", cause).with("from", from).with("to", to);
        if let Some([w, h]) = self.bounds
            && (to[0] < 0 || to[1] < 0 || to[0] >= w || to[1] >= h)
        {
            let msg = format!(
                "{who} could not move {cause} from {} to {}: outside the {w}x{h} ground",
                fmt_cell(from),
                fmt_cell(to)
            );
            self.log(EventKind::Blocked, Some(id), None, msg, base.with("reason", "edge"));
            return false;
        }
        if let Some((blocker, reason)) = self.blocker_at(to, id) {
            let what = if reason == "solid" { "solid" } else { "actor" };
            let msg = format!(
                "{who} could not move {cause} from {} to {}: blocked by {what} {}",
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

    /// Actors on a trigger's cell, honoring `only`.
    fn occupants_of(&self, trigger_id: EntityId) -> Vec<EntityId> {
        let t = &self.entities[trigger_id as usize];
        let Some(trigger) = &t.trigger else {
            return Vec::new();
        };
        let cell = t.cell();
        self.entities
            .iter()
            .filter(|e| e.id != trigger_id && e.enabled && e.is_actor() && e.cell() == cell)
            .filter(|e| trigger.def.only.as_ref().is_none_or(|only| e.name.as_ref() == Some(only)))
            .map(|e| e.id)
            .collect()
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

    fn dispatch_signals(&mut self) {
        let mut dispatched = 0;
        while let Some((from, signal)) = self.pending.pop_front() {
            dispatched += 1;
            if dispatched > SIGNAL_LIMIT {
                let msg = format!(
                    "more than {SIGNAL_LIMIT} signals in one frame (last: '{signal}' from {}); dropping the rest. \
                     This is usually a loop of connections (E0301)",
                    self.describe(from)
                );
                self.log(
                    EventKind::Error,
                    Some(from),
                    None,
                    msg,
                    Json::object().with("code", "E0301").with("signal", signal.as_str()),
                );
                self.pending.clear();
                break;
            }
            let targets: Vec<(EntityId, ConnectAction)> = self
                .connections
                .iter()
                .filter(|c| c.from == from && c.signal == signal)
                .map(|c| (c.to, c.action))
                .collect();
            let source = self.describe(from);
            let msg = if targets.is_empty() {
                let label = self.get(from).map_or_else(|| format!("#{from}"), Entity::label);
                format!(
                    "{source} emitted '{signal}' but nothing is connected to it \
                     (add [[connect]] from = \"{label}\" signal = \"{signal}\")"
                )
            } else {
                format!("{source} emitted '{signal}' to {} connection(s)", targets.len())
            };
            let data = Json::object().with("signal", signal.as_str()).with("connections", targets.len());
            self.log(EventKind::Signal, Some(from), None, msg, data);
            for (to, action) in targets {
                self.apply_action(from, &signal, to, action);
            }
        }
    }

    fn apply_action(&mut self, from: EntityId, signal: &str, to: EntityId, action: ConnectAction) {
        let Some(target) = self.entities.get_mut(to as usize) else {
            return;
        };
        let before = target.enabled;
        let after = match action {
            ConnectAction::Enable => true,
            ConnectAction::Disable => false,
            ConnectAction::Toggle => !before,
        };
        target.enabled = after;
        let state = |b: bool| if b { "enabled" } else { "disabled" };
        let change = if before == after {
            format!("stays {}", state(after))
        } else {
            format!("{} -> {}", state(before), state(after))
        };
        let msg = format!(
            "{} {}: {change} (via {}.'{signal}')",
            self.describe(to),
            action.name(),
            self.get(from).map_or_else(|| format!("#{from}"), Entity::label)
        );
        let data = Json::object()
            .with("signal", signal)
            .with("action", action.name())
            .with("before", before)
            .with("after", after);
        self.log(EventKind::Action, Some(to), Some(from), msg, data);
    }

    /// Hash of the full simulation state. Equal hashes mean equal worlds:
    /// the cheap way to compare runs across machines and builds.
    pub fn state_hash(&self) -> u64 {
        let mut h = Hasher64::new();
        h.u64(self.frame).u64(self.seed);
        let (s, inc) = self.rng.state();
        h.u64(s).u64(inc);
        for e in &self.entities {
            h.u32(e.id).bool(e.enabled).bool(e.solid);
            h.f32(e.pos[0]).f32(e.pos[1]).f32(e.pos[2]);
            if let Some(t) = &e.tile {
                h.u8(1).bytes(&[t.color.r, t.color.g, t.color.b, t.color.a]);
            }
            if let Some(b) = &e.block {
                h.u8(2).bytes(&[b.color.r, b.color.g, b.color.b, b.color.a]).f32(b.height);
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
