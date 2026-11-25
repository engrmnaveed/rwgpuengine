//! Input for one simulation step, already resolved to entities.

use slate_data::Action;

use crate::world::EntityId;

/// A set of [`Action`]s as a bit mask.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActionSet(u32);

impl ActionSet {
    pub const EMPTY: ActionSet = ActionSet(0);

    pub fn from_bits(bits: u32) -> ActionSet {
        ActionSet(bits & 0x3f)
    }

    pub fn bits(self) -> u32 {
        self.0
    }

    pub fn insert(&mut self, a: Action) {
        self.0 |= a.bit();
    }

    pub fn contains(self, a: Action) -> bool {
        self.0 & a.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn union(self, other: ActionSet) -> ActionSet {
        ActionSet(self.0 | other.0)
    }

    pub fn iter(self) -> impl Iterator<Item = Action> {
        Action::ALL.into_iter().filter(move |a| self.contains(*a))
    }

    pub fn names(self) -> Vec<&'static str> {
        self.iter().map(Action::name).collect()
    }
}

/// A screen tap, resolved against what was drawn (picking happens in the
/// runtime, which owns both the world and the renderer).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tap {
    pub x: f32,
    pub y: f32,
    /// Topmost enabled entity drawn under the point.
    pub hit: Option<EntityId>,
    /// Disabled entities under the point, topmost first. Reported when a tap
    /// misses, to answer "why didn't my button fire?".
    pub under_disabled: Vec<EntityId>,
    /// The ground cell under the point, when no enabled entity was hit.
    pub ground: Option<[i32; 2]>,
}

/// Everything the simulation sees as input for one step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputFrame {
    /// Actions that went down this step.
    pub pressed: ActionSet,
    /// Actions that are down this step (pressed now or held).
    pub down: ActionSet,
    pub taps: Vec<Tap>,
}

impl InputFrame {
    pub fn is_empty(&self) -> bool {
        self.pressed.is_empty() && self.down.is_empty() && self.taps.is_empty()
    }
}
