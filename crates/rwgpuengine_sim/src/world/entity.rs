//! Entities and their built-in components.
//!
//! Entities are "fat": one struct with optional built-in components plus a
//! free-form `props` map (ADR 0006). That keeps the world easy to query and
//! to dump as JSON: what `rwgpu inspect` prints is this struct.

use std::collections::BTreeMap;

use rwgpuengine_data::{
    BlockDef, EntityDef, LabelDef, LabelPart, PatrolDef, PropValue, SpriteDef, TileDef, TriggerDef, WanderDef,
    footprint_cells, parse_label, round_cell,
};

use super::{EntityId, World};

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
    /// `counts`: (prop, prefab), kept equal to the enabled copies.
    pub counts: Vec<(String, String)>,
    /// Warning codes intended for this entity (not reported).
    pub allow: Vec<String>,
    /// Its `[[shape]]` list, as a place in [`World::shapes`]: the entities
    /// made from one prefab are drawn from one list.
    pub shapes: Option<u32>,
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
    /// are drawn there (tile, block, shapes, sprite, label), solid, pushable,
    /// actors or triggers. A data-only entity (just props) takes up nothing.
    pub fn takes_space(&self) -> bool {
        self.enabled
            && self.in_world()
            && (self.tile.is_some()
                || self.block.is_some()
                || self.shapes.is_some()
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

    /// `player`, `#12 (house)` for an unnamed prefab copy, or `#12`.
    pub fn label(&self) -> String {
        match (&self.name, &self.prefab) {
            (Some(n), _) => n.clone(),
            (None, Some(p)) => format!("#{} ({p})", self.id),
            (None, None) => format!("#{}", self.id),
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

/// Every entity, in id order: `entities[id]` is entity `id`. It reads like a
/// slice (`len`, `iter`, `entities[i]`, `for e in &world.entities`).
///
/// It cannot be changed from outside the world, because the world keeps an
/// index of where its entities are (ADR 0026): change an entity through
/// [`World::get_mut`], and the index follows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entities(pub(super) Vec<Entity>);

impl std::ops::Deref for Entities {
    type Target = [Entity];

    fn deref(&self) -> &[Entity] {
        &self.0
    }
}

impl<'a> IntoIterator for &'a Entities {
    type Item = &'a Entity;
    type IntoIter = std::slice::Iter<'a, Entity>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// An entity being changed by something other than the rules: Rust game
/// code, a test ([`World::get_mut`]). Use it like `&mut Entity`. When it is
/// let go, the world brings its index up to date with whatever changed:
/// where the entity is, whether it is enabled, what it is.
pub struct EntityMut<'a> {
    pub(super) world: &'a mut World,
    pub(super) id: EntityId,
}

impl std::ops::Deref for EntityMut<'_> {
    type Target = Entity;

    fn deref(&self) -> &Entity {
        &self.world.entities.0[self.id as usize]
    }
}

impl std::ops::DerefMut for EntityMut<'_> {
    fn deref_mut(&mut self) -> &mut Entity {
        &mut self.world.entities.0[self.id as usize]
    }
}

impl Drop for EntityMut<'_> {
    fn drop(&mut self) {
        self.world.resync(self.id);
    }
}

impl std::fmt::Debug for EntityMut<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (**self).fmt(f)
    }
}

/// A runtime entity from its definition. `shapes` is the place of its shape
/// list in the world's.
pub(super) fn make_entity(id: EntityId, def: &EntityDef, shapes: Option<u32>) -> Entity {
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
        counts: def.counts.clone(),
        allow: def.allow.clone(),
        shapes,
    }
}
