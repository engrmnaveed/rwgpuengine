//! `[[connect]]` wiring with its names resolved: who listens to which signal,
//! on what conditions, and what it does.

use std::collections::BTreeMap;

use rwgpuengine_data::{CondTest, ConditionDef, ConnectAction, ConnectFrom, PropValue, SceneDef};

use super::{EntityId, World};

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
    /// Run in order; a refused `spawn` stops the rest.
    pub actions: Vec<Act>,
    /// All must hold when the signal arrives (checked once, before the first action).
    pub when: Vec<Condition>,
}

/// One action of a connection, with its target resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Act {
    pub to: Target,
    pub action: ConnectAction,
    /// With add/set.
    pub prop: Option<String>,
    pub value: Option<PropValue>,
    /// With spawn.
    pub prefab: Option<String>,
    /// With spawn: who pays, from which prop, how much.
    pub cost: Option<Cost>,
}

/// A spawn's price, with the payer resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct Cost {
    pub entity: EntityId,
    pub prop: String,
    pub amount: PropValue,
}

impl Connection {
    /// Does this connection listen to `signal` from `emitter` (`None` = the ground)?
    pub(super) fn listens(&self, world: &World, emitter: Option<EntityId>, signal: &str) -> bool {
        self.signal == signal
            && match (&self.from, emitter) {
                (Source::Entity(id), Some(e)) => *id == e,
                (Source::Prefab(p), Some(e)) => world.get(e).and_then(|x| x.prefab.as_deref()) == Some(p.as_str()),
                (Source::Ground, None) => true,
                _ => false,
            }
    }
}

/// A `when` condition with its entity resolved (`None` for `reason`).
#[derive(Clone, Debug, PartialEq)]
pub struct Condition {
    pub entity: Option<EntityId>,
    pub def: ConditionDef,
}

/// A scene's `[[connect]]` tables with their names resolved. A table that
/// names an entity the scene does not have is left out (the loader reports it).
pub(super) fn resolve(scene: &SceneDef, names: &BTreeMap<String, EntityId>) -> Vec<Connection> {
    scene
        .connections
        .iter()
        .filter_map(|c| {
            let when = c
                .when
                .iter()
                .map(|w| {
                    let entity = match w.test {
                        CondTest::Reason(_) => None,
                        _ => Some(*names.get(&w.entity)?),
                    };
                    Some(Condition { entity, def: w.clone() })
                })
                .collect::<Option<Vec<_>>>()?;
            let from = match &c.from {
                ConnectFrom::Entity(n) => Source::Entity(*names.get(n)?),
                ConnectFrom::Prefab(p) => Source::Prefab(p.clone()),
                ConnectFrom::Ground => Source::Ground,
            };
            let actions = c
                .actions
                .iter()
                .map(|a| {
                    let to = match (a.action, a.to.as_deref()) {
                        (ConnectAction::Spawn, _) => Target::None,
                        (_, Some("self")) => Target::Emitter,
                        (_, Some(n)) => Target::Entity(*names.get(n)?),
                        (_, None) => return None,
                    };
                    let cost = match &a.cost {
                        Some(k) => Some(Cost {
                            entity: *names.get(&k.entity)?,
                            prop: k.prop.clone(),
                            amount: k.amount.clone(),
                        }),
                        None => None,
                    };
                    Some(Act {
                        to,
                        action: a.action,
                        prop: a.prop.clone(),
                        value: a.value.clone(),
                        prefab: a.prefab.clone(),
                        cost,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(Connection { from, signal: c.signal.clone(), actions, when })
        })
        .collect()
}
