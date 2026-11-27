//! The cell index: what the world keeps about its entities so that no rule
//! has to look at all of them (ADR 0026).
//!
//! - which entities are on each cell ([`Cells`]),
//! - what a question about space needs to know of an entity, 24 bytes each
//!   ([`Fact`]), so that answering never reads the entity itself,
//! - which entities have each behavior, in id order,
//! - how many enabled copies each prefab has.
//!
//! An entity is entered once for every cell its body covers, so a footprint
//! of w x h costs w x h entries.
//!
//! All of it is derived: [`Index::build`] makes it from the entities alone,
//! it is not part of the state hash, and [`Index::differs_from`] says where
//! a kept index and a rebuilt one disagree. The rules tell it what they
//! change ([`Index::moved`], [`Index::resync`], [`Index::enter`]); anything
//! else that changes an entity goes through
//! [`World::get_mut`](super::World::get_mut), which calls `resync` when it
//! is done. `scan.rs` is the definition of every answer given here.

use std::collections::BTreeMap;

use super::cells::Cells;
use super::{Entity, EntityId};

const ENABLED: u16 = 1 << 0;
/// In the world and taking up its cells once enabled: drawn there, solid,
/// pushable, an actor or a trigger ([`Entity::takes_space`] without `enabled`).
const BODY: u16 = 1 << 1;
/// Solid or pushable: it blocks as "solid".
const SOLID: u16 = 1 << 2;
const ACTOR: u16 = 1 << 3;
const PUSHABLE: u16 = 1 << 4;
const MOVER: u16 = 1 << 5;
const PATROL: u16 = 1 << 6;
const WANDER: u16 = 1 << 7;
const TRIGGER: u16 = 1 << 8;
/// What holds a plate ([`Entity::holds_plates`]).
const WEIGHT: u16 = ACTOR | PUSHABLE;
/// The bits that decide, with the cell and the footprint, where an entity is entered.
const PLACED_BY: u16 = ENABLED | BODY | WEIGHT;

const NO_PREFAB: u32 = u32::MAX;

/// A behavior an entity can have. The index lists who has each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Mover,
    Patrol,
    Wander,
    Trigger,
}

const KINDS: [(Kind, u16); 4] =
    [(Kind::Mover, MOVER), (Kind::Patrol, PATROL), (Kind::Wander, WANDER), (Kind::Trigger, TRIGGER)];

/// What the index knows about one entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fact {
    cell: [i32; 2],
    footprint: [i32; 2],
    /// Its prefab's number in [`Index::prefabs`], or `NO_PREFAB`.
    prefab: u32,
    bits: u16,
}

impl Fact {
    /// An entity the index has not been told about: nowhere, nothing.
    const UNKNOWN: Fact = Fact { cell: [0, 0], footprint: [0, 0], prefab: NO_PREFAB, bits: 0 };

    fn of(e: &Entity, prefab: u32) -> Fact {
        let actor = e.mover.is_some() || e.patrol.is_some() || e.wander.is_some();
        let drawn = e.tile.is_some() || e.block.is_some() || e.sprite.is_some() || e.label.is_some();
        let body = e.screen_pos.is_none() && (drawn || e.trigger.is_some() || e.solid || e.pushable || actor);
        let bits = [
            (e.enabled, ENABLED),
            (body, BODY),
            (e.solid || e.pushable, SOLID),
            (actor, ACTOR),
            (e.pushable, PUSHABLE),
            (e.mover.is_some(), MOVER),
            (e.patrol.is_some(), PATROL),
            (e.wander.is_some(), WANDER),
            (e.trigger.is_some(), TRIGGER),
        ]
        .into_iter()
        .fold(0, |bits, (on, bit)| if on { bits | bit } else { bits });
        Fact { cell: e.cell(), footprint: e.footprint, prefab, bits }
    }

    fn has(self, bits: u16) -> bool {
        self.bits & bits != 0
    }

    /// [`Entity::covers`], from what the index knows.
    fn covers(self, c: [i32; 2]) -> bool {
        let [x, y] = self.cell;
        c[0] >= x && c[1] >= y && c[0] < x.wrapping_add(self.footprint[0]) && c[1] < y.wrapping_add(self.footprint[1])
    }

    /// The cells the entity is entered on: every cell its body covers, and
    /// the cell it stands on if it can hold a plate (the same cell, for
    /// anything a scene file can describe). None while it is disabled.
    fn each_cell(self, mut f: impl FnMut([i32; 2])) {
        if !self.has(ENABLED) {
            return;
        }
        let [x, y] = self.cell;
        let mut on_its_cell = false;
        // A footprint that would pass the largest cell covers nothing, as in `covers`.
        if self.has(BODY)
            && let (Some(x1), Some(y1)) = (x.checked_add(self.footprint[0]), y.checked_add(self.footprint[1]))
        {
            for cy in y..y1 {
                for cx in x..x1 {
                    f([cx, cy]);
                }
            }
            on_its_cell = x1 > x && y1 > y;
        }
        if self.has(WEIGHT) && !on_its_cell {
            f(self.cell);
        }
    }

    fn placed_like(self, other: Fact) -> bool {
        self.cell == other.cell && self.footprint == other.footprint && self.bits & PLACED_BY == other.bits & PLACED_BY
    }
}

#[derive(Clone, Debug)]
pub(super) struct Index {
    cells: Cells,
    /// One per entity, by id.
    facts: Vec<Fact>,
    /// Who has each behavior, enabled or not, in id order (by [`KINDS`]).
    with: [Vec<EntityId>; 4],
    /// The number of each prefab name seen so far.
    prefabs: BTreeMap<String, u32>,
    /// Enabled copies per prefab number.
    copies: Vec<u32>,
}

/// How large the index is (`World::index_stats`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexStats {
    /// Cells of the ground that are an array.
    pub ground_cells: usize,
    /// Entries: an entity on a cell.
    pub entries: usize,
    /// Bytes held, lists and facts included.
    pub bytes: usize,
}

impl Index {
    /// The index of these entities. `ground` is the ground's size, if the
    /// world has one: its cells are an array, all others a sorted map.
    pub fn build(entities: &[Entity], ground: Option<[i32; 2]>) -> Index {
        let mut index = Index {
            cells: Cells::new(ground),
            facts: Vec::with_capacity(entities.len()),
            with: Default::default(),
            prefabs: BTreeMap::new(),
            copies: Vec::new(),
        };
        for e in entities {
            index.enter(e);
        }
        index
    }

    /// A new entity, the next id.
    pub fn enter(&mut self, e: &Entity) {
        let id = self.facts.len() as EntityId;
        self.facts.push(Fact::UNKNOWN);
        self.resync(id, e);
    }

    fn prefab_number(&mut self, e: &Entity) -> u32 {
        let Some(name) = e.prefab.as_deref() else { return NO_PREFAB };
        if let Some(n) = self.prefabs.get(name) {
            return *n;
        }
        let n = self.copies.len() as u32;
        self.prefabs.insert(name.to_string(), n);
        self.copies.push(0);
        n
    }

    /// Bring everything kept about entity `id` up to date with `e`, whatever
    /// changed: where it is, whether it is enabled, what it is.
    pub fn resync(&mut self, id: EntityId, e: &Entity) {
        let prefab = self.prefab_number(e);
        let new = Fact::of(e, prefab);
        let old = self.facts[id as usize];
        if new == old {
            return;
        }
        if !new.placed_like(old) {
            self.replace(id, old, new);
        }
        for (i, (_, bit)) in KINDS.iter().enumerate() {
            if new.has(*bit) != old.has(*bit) {
                let list = &mut self.with[i];
                match (list.binary_search(&id), new.has(*bit)) {
                    (Err(at), true) => list.insert(at, id),
                    (Ok(at), false) => {
                        list.remove(at);
                    }
                    _ => {}
                }
            }
        }
        if old.has(ENABLED) && old.prefab != NO_PREFAB {
            self.copies[old.prefab as usize] -= 1;
        }
        if new.has(ENABLED) && new.prefab != NO_PREFAB {
            self.copies[new.prefab as usize] += 1;
        }
        self.facts[id as usize] = new;
    }

    /// Entity `id` now stands on `cell` and nothing else about it changed:
    /// what a step of movement does, without looking at the entity.
    pub fn moved(&mut self, id: EntityId, cell: [i32; 2]) {
        let old = self.facts[id as usize];
        let new = Fact { cell, ..old };
        if new == old {
            return;
        }
        self.replace(id, old, new);
        self.facts[id as usize] = new;
    }

    /// Take entity `id` off the cells `old` says it is on and put it on `new`'s.
    fn replace(&mut self, id: EntityId, old: Fact, new: Fact) {
        old.each_cell(|c| self.cells.remove(c, id));
        new.each_cell(|c| self.cells.add(c, id));
    }

    /// The first entity that stops something entering `cell`, and whether a
    /// second one does too. Pushables count as solid.
    pub fn blockers(&self, cell: [i32; 2], except: EntityId) -> (Option<(EntityId, &'static str)>, bool) {
        let mut found = self.cells.at(cell).filter_map(|id| {
            let f = self.facts[id as usize];
            // Entered here as a body (not only as a weight on its own cell).
            if id == except || !f.has(BODY) || !f.covers(cell) {
                None
            } else if f.has(SOLID) {
                Some((id, "solid"))
            } else if f.has(ACTOR) {
                Some((id, "actor"))
            } else {
                None
            }
        });
        let first = found.next();
        (first, first.is_some() && found.next().is_some())
    }

    /// The first entity that takes up `cell` for placement.
    pub fn taking(&self, cell: [i32; 2]) -> Option<EntityId> {
        self.cells.at(cell).find(|id| {
            let f = self.facts[*id as usize];
            f.has(BODY) && f.covers(cell)
        })
    }

    /// Adds to `out`, in id order, the actors and pushables that stand on
    /// `cell`, other than `except`.
    pub fn weights_on(&self, cell: [i32; 2], except: EntityId, out: &mut Vec<EntityId>) {
        out.extend(self.cells.at(cell).filter(|id| {
            let f = self.facts[*id as usize];
            *id != except && f.has(WEIGHT) && f.cell == cell
        }));
    }

    /// The enabled entities with a behavior, in id order: who takes a turn
    /// in that part of the step.
    pub fn enabled_with(&self, kind: Kind) -> Vec<EntityId> {
        self.all_with(kind).iter().copied().filter(|id| self.facts[*id as usize].has(ENABLED)).collect()
    }

    /// The entities with a behavior, enabled or not, in id order.
    pub fn all_with(&self, kind: Kind) -> &[EntityId] {
        &self.with[KINDS.iter().position(|(k, _)| *k == kind).expect("a kind")]
    }

    /// The enabled copies of `prefab`.
    pub fn copies(&self, prefab: &str) -> usize {
        self.prefabs.get(prefab).map_or(0, |n| self.copies[*n as usize] as usize)
    }

    pub fn stats(&self) -> IndexStats {
        let (ground_cells, entries, cells_bytes) = self.cells.stats();
        let lists: usize = self.with.iter().map(|l| l.capacity() * 4).sum();
        let bytes =
            cells_bytes + self.facts.capacity() * std::mem::size_of::<Fact>() + lists + self.copies.capacity() * 4;
        IndexStats { ground_cells, entries, bytes }
    }

    /// Where this index and `other` disagree, if anywhere. Two indexes of
    /// the same entities hold the same things, whatever happened on the way:
    /// prefabs may be numbered in another order and nodes lie elsewhere, so
    /// they are compared by what they say.
    pub fn differs_from(&self, other: &Index) -> Option<String> {
        if self.facts.len() != other.facts.len() {
            return Some(format!("{} entities are known, {} expected", self.facts.len(), other.facts.len()));
        }
        let names = |index: &Index| -> Vec<String> {
            let mut names = vec![String::new(); index.copies.len()];
            for (name, n) in &index.prefabs {
                names[*n as usize].clone_from(name);
            }
            names
        };
        let (mine, theirs) = (names(self), names(other));
        for (id, (a, b)) in self.facts.iter().zip(&other.facts).enumerate() {
            let same = (a.cell, a.footprint, a.bits) == (b.cell, b.footprint, b.bits)
                && mine.get(a.prefab as usize) == theirs.get(b.prefab as usize);
            if !same {
                return Some(format!("entity #{id} is known as {a:?}, expected {b:?}"));
            }
        }
        for (i, (kind, _)) in KINDS.iter().enumerate() {
            if self.with[i] != other.with[i] {
                return Some(format!("the {kind:?} list is {:?}, expected {:?}", self.with[i], other.with[i]));
            }
        }
        let counted = |index: &Index| -> BTreeMap<String, u32> {
            let all = index.prefabs.iter().map(|(k, n)| (k.clone(), index.copies[*n as usize]));
            all.filter(|(_, n)| *n > 0).collect()
        };
        if counted(self) != counted(other) {
            return Some(format!("enabled copies are {:?}, expected {:?}", counted(self), counted(other)));
        }
        let (mine, theirs) = (self.cells.listing(), other.cells.listing());
        if mine != theirs {
            let odd = mine.iter().zip(&theirs).find(|(a, b)| a != b);
            return Some(match odd {
                Some((a, b)) => format!("cell {:?} holds {:?}, expected cell {:?} to hold {:?}", a.0, a.1, b.0, b.1),
                None => format!("{} cells hold someone, {} expected", mine.len(), theirs.len()),
            });
        }
        None
    }
}
