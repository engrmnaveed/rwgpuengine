//! The world: entities, connections, the RNG and the event log, advanced one
//! fixed step at a time.
//!
//! The rules live in the modules below, one per responsibility; this file
//! holds the world itself, how it is built and the order of a step.
//!
//! No rule looks at every entity. Questions about space ("what stops a
//! move onto this cell?", "who stands on this plate?", "is there room?")
//! are answered from the cell index (`index.rs`, ADR 0026), which the world
//! keeps up to date as entities move, spawn and are switched on and off.
//! The index is derived: it can be rebuilt from the entities at any time and
//! is not part of the state hash. `scan.rs` holds the same questions
//! answered by looking at everything, as the definition the index is held to.

use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use rwgpuengine_core::json::Json;
use rwgpuengine_core::limits::{self, Limit};
use rwgpuengine_core::rng::Rng;
use rwgpuengine_data::{EntityDef, GroundDef, SceneDef, ShapeDef};

use crate::events::{Event, EventKind, EventLog};
use crate::input::InputFrame;

mod actions;
mod cells;
mod counting;
mod entity;
mod hashing;
mod index;
mod movement;
mod payment;
mod placement;
mod scan;
mod signals;
mod taps;
mod triggers;
mod wiring;

pub use entity::{Entities, Entity, EntityMut, Mover, Patrol, Trigger, Wander};
pub use index::IndexStats;
pub use placement::Refusal;
pub use wiring::{Act, Condition, Connection, Cost, Source, Target, Turn};

use counting::Stale;
use entity::make_entity;
use index::{Index, Kind};
use signals::Pending;
use wiring::Routes;

/// Entities are numbered in scene order, starting at 0. Ids are stable for a
/// given scene file, so they can be used in scripts and tests.
pub type EntityId = u32;

/// Maximum signals dispatched in one step before it is treated as a loop
/// ([`limits::SIGNALS_PER_STEP`]; the engine's limits are all there).
pub const SIGNAL_LIMIT: usize = limits::SIGNALS_PER_STEP.value as usize;

/// A limit of [`limits`] that a running world reached.
#[derive(Clone, Debug, PartialEq)]
pub struct LimitHit {
    pub limit: &'static Limit,
    /// The first step that reached it, and the last.
    pub first: u64,
    pub last: u64,
    /// How many steps reached it.
    pub steps: u64,
    /// What the first one cut off (the `error` event's message).
    pub message: String,
}

/// The work done so far in the current step, against the step's limits.
#[derive(Clone, Debug, Default)]
struct Work {
    actions: usize,
    count_changes: usize,
    /// A limit was reached: the step dispatches nothing more.
    cut: bool,
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
    /// Every entity, in id order. Read it like a slice; change an entity
    /// through [`World::get_mut`], so that the index follows.
    pub entities: Entities,
    /// Ground size: movement is limited to 0..size on each axis.
    pub bounds: Option<[i32; 2]>,
    /// The ground with its terrain (placement rules read it).
    pub ground: Option<GroundDef>,
    /// The scene's `[[connect]]` tables, in file order ([`World::connections`]).
    /// Shared, so that dispatching a signal does not copy them.
    connections: Rc<[Connection]>,
    pub events: EventLog,
    /// Prefabs that `spawn` actions place (their `prefab` is the name).
    pub templates: Vec<EntityDef>,
    /// The `[[shape]]` lists of the scene, each once, in the order first
    /// used: its entities' in file order, then the spawn templates'.
    /// [`Entity::shapes`] is a place in it. Part of the world's state.
    pub shapes: Vec<Arc<[ShapeDef]>>,
    /// The place in `shapes` of each template's list.
    template_shapes: Vec<Option<u32>>,
    pending: VecDeque<Pending>,
    names: BTreeMap<String, EntityId>,
    /// Entities with `counts` props (derived; not hashed).
    counters: Vec<EntityId>,
    /// For each prefab that `counts` props count, the entities with such a
    /// prop, in id order (derived; not hashed).
    counted_by: BTreeMap<String, Vec<EntityId>>,
    /// The work of this step so far.
    work: Work,
    /// The limits reached so far, in the order first reached.
    limits_hit: Vec<LimitHit>,
    /// [`limits::ENTITIES`]; lower in tests, which cannot step a million
    /// entities in a debug build.
    max_entities: usize,
    /// Where the entities are and what they are, for the rules (derived; not hashed).
    index: Index,
    /// Which connections listen to which signal (derived; not hashed).
    routes: Routes,
    /// Compare every answer of the index with a scan ([`World::check_against_scans`]).
    check: bool,
}

impl World {
    /// Build the world for frame 0. The scene must have passed validation.
    pub fn new(scene: &SceneDef, seed: u64) -> World {
        let mut entities = Vec::with_capacity(scene.entities.len());
        let mut names = BTreeMap::new();
        let mut lists = ShapeLists::new(scene.entities.iter().chain(&scene.templates));
        for (i, def) in scene.entities.iter().enumerate() {
            let id = i as EntityId;
            if let Some(n) = &def.name {
                names.insert(n.clone(), id);
            }
            entities.push(make_entity(id, def, lists.place(def)));
        }
        let template_shapes = scene.templates.iter().map(|def| lists.place(def)).collect();
        let connections = wiring::resolve(scene, &names);
        let templates = scene.templates.clone();
        let bounds = scene.ground.as_ref().map(|g| g.size);
        let mut world = World {
            frame: 0,
            seed,
            rng: Rng::new(seed),
            index: Index::build(&entities, bounds),
            routes: Routes::build(&connections),
            entities: Entities(entities),
            bounds,
            ground: scene.ground.clone(),
            connections: connections.into(),
            events: EventLog::default(),
            templates,
            shapes: lists.lists,
            template_shapes,
            pending: VecDeque::new(),
            names,
            counters: Vec::new(),
            counted_by: BTreeMap::new(),
            work: Work::default(),
            limits_hit: Vec::new(),
            max_entities: limits::ENTITIES.get(),
            check: false,
        };
        for id in scan::ids_with(&world.entities, |e| !e.counts.is_empty()) {
            world.add_counter(id);
        }
        // Count props start at the scene's numbers, without events.
        for (id, prop, _, _, n) in world.count_changes(Stale::All) {
            world.set_count(id, &prop, n);
        }
        // Initial trigger occupancy, without emitting: a plate that starts
        // occupied does not fire at frame 0.
        for id in world.index.all_with(Kind::Trigger).to_vec() {
            let occ = world.occupants_of(id);
            if let Some(t) = world.entities.0[id as usize].trigger.as_mut() {
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

    /// An entity to change, for Rust game code and tests: use the result
    /// like `&mut Entity`. The world's index is brought up to date when the
    /// result is let go, at the end of the statement in
    /// `world.get_mut(id)?.enabled = false`.
    pub fn get_mut(&mut self, id: EntityId) -> Option<EntityMut<'_>> {
        ((id as usize) < self.entities.len()).then(|| EntityMut { world: self, id })
    }

    pub fn by_name(&self, name: &str) -> Option<&Entity> {
        self.find(name).and_then(|id| self.get(id))
    }

    /// The scene's `[[connect]]` tables with their names resolved, in file order.
    pub fn connections(&self) -> &[Connection] {
        &self.connections
    }

    /// Lower the entity limit, for a test.
    #[cfg(test)]
    pub(crate) fn limit_entities(&mut self, n: usize) {
        self.max_entities = n;
    }

    /// The limits this world has reached, in the order first reached: each
    /// stopped a step there (`rwgpu run` reports them as diagnostics).
    pub fn limits_hit(&self) -> &[LimitHit] {
        &self.limits_hit
    }

    /// A step reached `limit`: write the `error` event that says what was
    /// cut off, drop the signals still queued, and dispatch nothing more in
    /// this step.
    pub(super) fn cut_step(
        &mut self,
        limit: &'static Limit,
        entity: Option<EntityId>,
        other: Option<EntityId>,
        message: String,
        data: Json,
    ) {
        self.reached(limit, &message);
        self.write(EventKind::Error, entity, other, message, data);
        self.pending.clear();
        self.work.cut = true;
    }

    /// Remember that this step reached `limit`.
    pub(super) fn reached(&mut self, limit: &'static Limit, message: &str) {
        let frame = self.frame;
        match self.limits_hit.iter_mut().find(|h| h.limit == limit) {
            Some(h) => {
                if h.last != frame {
                    h.last = frame;
                    h.steps += 1;
                }
            }
            None => self.limits_hit.push(LimitHit {
                limit,
                first: frame,
                last: frame,
                steps: 1,
                message: message.to_string(),
            }),
        }
    }

    /// Tell the index that entity `id` changed in any way.
    fn resync(&mut self, id: EntityId) {
        if let Some(e) = self.entities.0.get(id as usize) {
            self.index.resync(id, e);
        }
    }

    /// The enabled entities with a behavior, in id order: who takes a turn
    /// in that part of the step.
    fn enabled_with(&self, kind: Kind) -> Vec<EntityId> {
        let ids = self.index.enabled_with(kind);
        if self.checking() {
            let scanned = scan::ids_with(&self.entities, |e| {
                e.enabled
                    && match kind {
                        Kind::Mover => e.mover.is_some(),
                        Kind::Patrol => e.patrol.is_some(),
                        Kind::Wander => e.wander.is_some(),
                        Kind::Trigger => e.trigger.is_some(),
                    }
            });
            assert_eq!(ids, scanned, "the index and the scan disagree on the enabled entities with a {kind:?}");
        }
        ids
    }

    /// Hold the index to the scans. While on, every answer the index gives
    /// (what blocks a cell, who stands on a plate, what takes a cell, how
    /// many copies, who listens, whose turn it is) is also worked out by
    /// looking at every entity, and the world panics where the two differ.
    /// As slow as the engine was before the index; for tests, and for
    /// finding a fault.
    ///
    /// Returns whether the check is on now. A release build leaves the
    /// check out (it would be 8 KB of the web module) and answers `false`.
    pub fn check_against_scans(&mut self, on: bool) -> bool {
        self.check = on;
        self.checking()
    }

    /// Is every answer of the index to be compared with a scan? Never in a
    /// release build: the comparisons are then not compiled.
    fn checking(&self) -> bool {
        cfg!(any(test, debug_assertions)) && self.check
    }

    /// Is the index what a rebuild from the entities gives? `Err` says where
    /// it is not. Debug builds ask this at the end of every step.
    pub fn verify_index(&self) -> Result<(), String> {
        match self.index.differs_from(&Index::build(&self.entities, self.bounds)) {
            Some(why) => Err(why),
            None => Ok(()),
        }
    }

    /// Throw the index away and build it again from the entities. A world
    /// answers the same before and after.
    pub fn rebuild_index(&mut self) {
        self.index = Index::build(&self.entities, self.bounds);
    }

    /// How large the index is.
    pub fn index_stats(&self) -> IndexStats {
        self.index.stats()
    }

    fn describe(&self, id: EntityId) -> String {
        self.get(id).map_or_else(|| format!("#{id}"), Entity::describe)
    }

    /// Append an event for the current frame, for Rust game code. Events of
    /// the per-actor kinds (`moved`, `blocked`, `trigger`, `custom`) count
    /// against the step's budget for their kind: past it they are counted,
    /// not written, unless the log's [`Watch`](crate::Watch) covers them.
    /// [`World::log_with`] builds the message only when it is written.
    pub fn log(
        &mut self,
        kind: EventKind,
        entity: Option<EntityId>,
        other: Option<EntityId>,
        message: String,
        data: Json,
    ) {
        if Self::per_actor(kind) && !self.wants(kind, entity, other) {
            return;
        }
        self.write(kind, entity, other, message, data);
    }

    /// [`World::log`], with the message and fields built by `build` only if
    /// the event is written.
    pub fn log_with(
        &mut self,
        kind: EventKind,
        entity: Option<EntityId>,
        other: Option<EntityId>,
        build: impl FnOnce(&World) -> (String, Json),
    ) {
        if Self::per_actor(kind) && !self.wants(kind, entity, other) {
            return;
        }
        let (message, data) = build(self);
        self.write(kind, entity, other, message, data);
    }

    fn per_actor(kind: EventKind) -> bool {
        matches!(kind, EventKind::Moved | EventKind::Blocked | EventKind::Trigger | EventKind::Custom)
    }

    /// Will an event of a per-actor kind about `entity` (and `other`) be
    /// written in this step? If not, it has been counted: build nothing. If
    /// so, build it and [`World::write`] it. Ask once per event: each yes
    /// takes a place in the step's budget.
    pub(super) fn wants(&mut self, kind: EventKind, entity: Option<EntityId>, other: Option<EntityId>) -> bool {
        self.events.admit(self.frame, kind, entity, other)
    }

    /// Append an event that has been admitted, or of a kind that is always
    /// written.
    pub(super) fn write(
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

    /// Advance one fixed step: input, movers, patrols, wanderers, custom
    /// systems, triggers, then signal dispatch. The order is part of the
    /// determinism contract (docs/architecture.md#the-step).
    pub fn tick(&mut self, input: &InputFrame, systems: &mut dyn Systems) {
        self.work = Work::default();
        self.apply_input(input);
        self.run_movers(input);
        self.run_patrols();
        self.run_wanders();
        systems.tick(self, input);
        self.run_triggers();
        self.dispatch_signals();
        self.events.close_step();
        #[cfg(debug_assertions)]
        if let Err(why) = self.verify_index() {
            panic!("the cell index is not what the entities say after step {}: {why}", self.frame);
        }
        self.frame += 1;
    }
}

/// The shape lists of a scene, each once. Entities made from one prefab hold
/// the same list (the loader decodes a prefab's once), so a list is known by
/// where it is: a table from that to its place, with no order of its own.
struct ShapeLists {
    lists: Vec<Arc<[ShapeDef]>>,
    /// (where a list is, its place in `lists` + 1), or (0, 0); open
    /// addressing, at most half full.
    seen: Vec<(usize, u32)>,
}

impl ShapeLists {
    fn new<'a>(defs: impl Iterator<Item = &'a EntityDef>) -> ShapeLists {
        let with_shapes = defs.filter(|d| d.shapes.is_some()).count();
        let size = if with_shapes == 0 { 0 } else { (with_shapes * 2).next_power_of_two() };
        ShapeLists { lists: Vec::new(), seen: vec![(0, 0); size] }
    }

    /// The place of `def`'s list, which is added if it is new.
    fn place(&mut self, def: &EntityDef) -> Option<u32> {
        let list = def.shapes.as_ref()?;
        let at = Arc::as_ptr(list) as *const ShapeDef as usize;
        let mask = self.seen.len() - 1;
        // Lists are at least 8 bytes apart; the low bits say nothing.
        let mut slot = (at >> 3).wrapping_mul(0x9E37_79B9) & mask;
        loop {
            match self.seen[slot] {
                (0, _) => {
                    self.lists.push(list.clone());
                    self.seen[slot] = (at, self.lists.len() as u32);
                    return Some(self.lists.len() as u32 - 1);
                }
                (found, place) if found == at => return Some(place - 1),
                _ => slot = (slot + 1) & mask,
            }
        }
    }
}

pub fn fmt_cell(c: [i32; 2]) -> String {
    format!("({}, {})", c[0], c[1])
}
