//! The engine's limits, in one table (ADR 0027).
//!
//! A game file must not be able to make one step take unbounded work or
//! the world grow without end. Each limit here bounds one way it could:
//! when a running game reaches a limit, the step stops there with a
//! diagnostic of the limit's code and an `error` event that says what was
//! cut off. The event log's own bounds are here too; reaching them is not an
//! error, and what they leave out is counted.
//!
//! `rwgpu explain limits`, the generated reference and the AGENTS.md of a new
//! project print this table.

use crate::codes::{self, CodeInfo};

/// When a limit is checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applies {
    /// While a scene is loaded and checked.
    Load,
    /// In every step of a running game.
    Step,
    /// What the event log writes and keeps.
    Events,
}

impl Applies {
    pub fn name(self) -> &'static str {
        match self {
            Applies::Load => "load",
            Applies::Step => "step",
            Applies::Events => "events",
        }
    }
}

/// One limit: what it bounds, its number, and what happens at it.
#[derive(Debug)]
pub struct Limit {
    /// Short name, as tables and messages say it ("actions per step").
    pub name: &'static str,
    pub value: u64,
    pub applies: Applies,
    /// The diagnostic when a game reaches it; `None` when reaching it is not
    /// an error.
    pub code: Option<&'static CodeInfo>,
    /// What it bounds and what happens at it, in one or two sentences.
    pub doc: &'static str,
}

/// Limits are the same when their names are: a name is given once.
impl PartialEq for Limit {
    fn eq(&self, other: &Limit) -> bool {
        self.name == other.name
    }
}

impl Limit {
    pub fn get(&self) -> usize {
        self.value as usize
    }
}

/// Signals dispatched in one step: a loop of connections is cut here.
pub const SIGNALS_PER_STEP: Limit = Limit {
    name: "signals per step",
    value: 1_000,
    applies: Applies::Step,
    code: Some(&codes::SIGNAL_LIMIT),
    doc: "Signals dispatched in one step. The rest of the step's signals are dropped: usually a loop of \
          connections.",
};

/// Actions run or skipped in one step.
pub const ACTIONS_PER_STEP: Limit = Limit {
    name: "actions per step",
    value: 10_000,
    applies: Applies::Step,
    code: Some(&codes::ACTION_LIMIT),
    doc: "`[[connect]]` actions run or skipped in one step. The step stops at the next one and drops the signals \
          still queued: one signal can reach many connections with many actions each.",
};

/// `counts` props changed in one step.
pub const COUNT_CHANGES_PER_STEP: Limit = Limit {
    name: "count changes per step",
    value: 10_000,
    applies: Applies::Step,
    code: Some(&codes::COUNT_LIMIT),
    doc: "`counts` props changed in one step. The step stops there and drops the signals still queued; the next \
          step brings the remaining counts up to date.",
};

/// Entities a world may hold: a spawn beyond it is not placed.
pub const ENTITIES: Limit = Limit {
    name: "entities in a world",
    value: 1_000_000,
    applies: Applies::Step,
    code: Some(&codes::ENTITY_LIMIT),
    doc: "Entities a world may hold, spawned ones included (no entity is ever removed). A spawn beyond it is not \
          placed; the step stops there and drops the signals still queued.",
};

/// Taps applied in one step.
pub const TAPS_PER_STEP: Limit = Limit {
    name: "taps per step",
    value: 256,
    applies: Applies::Step,
    code: Some(&codes::TAP_LIMIT),
    doc: "Taps applied in one step, from a playtest or a player. The first ones are applied in order; the rest \
          are not looked at.",
};

/// Events of one per-actor kind written in one step; the rest are counted.
pub const EVENTS_PER_KIND_PER_STEP: Limit = Limit {
    name: "events of one kind per step",
    value: 256,
    applies: Applies::Events,
    code: None,
    doc: "`moved`, `blocked` (for a move), `trigger` and `custom` events written in one step, per kind. The rest \
          are counted, one `omitted` event per kind says how many, and the entities a run watches \
          (`--watch NAME`) have all of theirs written. Every other kind is bounded by the step's limits and \
          always written.",
};

/// Events the log keeps.
pub const EVENT_LOG: Limit = Limit {
    name: "events kept",
    value: 100_000,
    applies: Applies::Events,
    code: None,
    doc: "Events the log keeps. When it is full, the oldest half is dropped and counted (`events_dropped`); ask \
          for an earlier frame (`--frame`, `--since`) to see them.",
};

/// Every limit, in the order tables show them.
pub const ALL: &[&Limit] = &[
    &SIGNALS_PER_STEP,
    &ACTIONS_PER_STEP,
    &COUNT_CHANGES_PER_STEP,
    &ENTITIES,
    &TAPS_PER_STEP,
    &EVENTS_PER_KIND_PER_STEP,
    &EVENT_LOG,
];

/// The limits as a Markdown table.
pub fn markdown() -> String {
    let mut out = String::from("| limit | number | checked | code | what happens |\n|---|---|---|---|---|\n");
    for l in ALL {
        let code = l.code.map_or("—", |c| c.code);
        out.push_str(&format!("| {} | {} | {} | {} | {} |\n", l.name, l.value, l.applies.name(), code, l.doc));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_limit_is_named_once_and_its_code_is_registered() {
        let mut names = std::collections::BTreeSet::new();
        for l in ALL {
            assert!(names.insert(l.name), "two limits are named {}", l.name);
            assert!(l.value > 0, "{}", l.name);
            if let Some(c) = l.code {
                assert_eq!(codes::lookup(c.code).map(|x| x.code), Some(c.code), "{}", l.name);
                // The explanation says the number the table has.
                assert!(c.explain.contains(&l.value.to_string()), "{} does not say {}", c.code, l.value);
            }
        }
        assert!(markdown().lines().count() == ALL.len() + 2);
    }
}
