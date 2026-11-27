//! The world: entities, connections, the RNG and the event log, advanced one
//! fixed step at a time.
//!
//! The rules live in the modules below, one per responsibility; this file
//! holds the world itself, how it is built and the order of a step.

use std::collections::{BTreeMap, VecDeque};

use rwgpuengine_core::json::Json;
use rwgpuengine_core::rng::Rng;
use rwgpuengine_data::{EntityDef, GroundDef, SceneDef};

use crate::events::{Event, EventKind, EventLog};
use crate::input::InputFrame;

mod actions;
mod counting;
mod entity;
mod hashing;
mod movement;
mod payment;
mod placement;
mod signals;
mod taps;
mod triggers;
mod wiring;

pub use entity::{Entity, Mover, Patrol, Trigger, Wander};
pub use placement::Refusal;
pub use wiring::{Act, Condition, Connection, Cost, Source, Target};

use entity::make_entity;
use signals::Pending;

/// Entities are numbered in scene order, starting at 0. Ids are stable for a
/// given scene file, so they can be used in scripts and tests.
pub type EntityId = u32;

/// Maximum signals dispatched in one step before it is treated as a loop.
pub const SIGNAL_LIMIT: usize = 1000;

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
    /// Entities with `counts` props (derived; not hashed).
    counters: Vec<EntityId>,
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
        let connections = wiring::resolve(scene, &names);
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
            counters: Vec::new(),
        };
        world.counters = world.ids_with(|e| !e.counts.is_empty());
        // Count props start at the scene's numbers, without events.
        for (id, prop, _, _, n) in world.count_changes() {
            world.set_count(id, &prop, n);
        }
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
}

pub fn fmt_cell(c: [i32; 2]) -> String {
    format!("({}, {})", c[0], c[1])
}
