//! The definition of every answer the index gives: the same questions,
//! answered by looking at every entity or every connection, as the engine
//! did before the cell index (ADR 0026).
//!
//! No step calls these. They are what the index is held to: with
//! [`World::check_against_scans`](super::World::check_against_scans) on,
//! every answer of the index is compared with the scan's, and the tests do
//! that on thousands of random steps.

use super::wiring::Connection;
use super::{Entity, EntityId};

/// The entities `f` accepts, in id order.
pub(super) fn ids_with(entities: &[Entity], f: impl Fn(&Entity) -> bool) -> Vec<EntityId> {
    entities.iter().filter(|e| f(e)).map(|e| e.id).collect()
}

/// Every entity that stops something entering `cell`, in id order.
pub(super) fn blockers_at(entities: &[Entity], cell: [i32; 2], except: EntityId) -> Vec<(EntityId, &'static str)> {
    entities
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

/// Enabled actors and pushables on entity `id`'s cells, other than itself,
/// in id order. For a trigger, only those its `only` accepts.
pub(super) fn occupants_of(entities: &[Entity], id: EntityId) -> Vec<EntityId> {
    let t = &entities[id as usize];
    let only = t.trigger.as_ref().and_then(|tr| tr.def.only.as_ref());
    entities
        .iter()
        .filter(|e| e.id != id && e.enabled && e.holds_plates() && t.covers(e.cell()))
        .filter(|e| only.is_none_or(|only| e.name.as_ref() == Some(only)))
        .map(|e| e.id)
        .collect()
}

/// The first entity, in id order, that takes up `cell` for placement.
pub(super) fn taking(entities: &[Entity], cell: [i32; 2]) -> Option<EntityId> {
    entities.iter().find(|e| e.takes_space() && e.covers(cell)).map(|e| e.id)
}

/// The enabled copies of `prefab`.
pub(super) fn copies(entities: &[Entity], prefab: &str) -> usize {
    entities.iter().filter(|x| x.enabled && x.prefab.as_deref() == Some(prefab)).count()
}

/// The connections that listen to `signal` from `emitter` (`None` = the
/// ground), by their place in the file, in file order.
pub(super) fn listeners(
    connections: &[Connection],
    entities: &[Entity],
    emitter: Option<EntityId>,
    signal: &str,
) -> Vec<u32> {
    (0..connections.len() as u32).filter(|i| connections[*i as usize].listens(entities, emitter, signal)).collect()
}
