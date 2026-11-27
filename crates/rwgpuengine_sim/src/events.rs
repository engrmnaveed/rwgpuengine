//! The event log: a structured record of everything that happened and,
//! just as important, everything that was attempted and refused.
//!
//! "Why didn't the gate open?" is answered by reading events, not code:
//! the plate's `trigger` event, the `signal` event with its connection
//! count, the `action` event on the gate, or the `blocked` event that kept
//! the player from reaching the plate.
//!
//! At scale (ADR 0027) a sentence for every move is not affordable and not
//! readable. Events of the kinds there is one of per actor in a step (moves,
//! refused moves, plates, game code's) are written for the first
//! [`EVENTS_PER_KIND_PER_STEP`] of each kind in a step; the rest are counted,
//! and one `omitted` event per kind says how many. A [`Watch`] names what is
//! written in full whatever the count: entities, kinds, or everything. Every
//! other kind is bounded by the step's limits on work and always written.
//! Which events are written depends only on the order they happen in, never
//! on anything outside the world; and events never change the world.

use rwgpuengine_core::json::Json;
use rwgpuengine_core::limits::{EVENT_LOG, EVENTS_PER_KIND_PER_STEP};

use crate::world::EntityId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventKind {
    /// Actions pressed this frame.
    Input,
    /// A tap hit an entity (which then emits `tapped`).
    Tap,
    /// A tap hit nothing enabled.
    TapMiss,
    Moved,
    /// A move was refused: solid, actor, edge, area or cooldown.
    Blocked,
    /// A trigger's cell became occupied or empty.
    Trigger,
    /// A signal was dispatched (with its connection count).
    Signal,
    /// A connection's action was applied to its target.
    Action,
    /// A connection's `when` conditions did not all hold; nothing was done.
    Skipped,
    /// A `spawn` action placed a new entity.
    Spawned,
    /// A `counts` prop changed (copies were spawned, disabled or enabled).
    Counted,
    Error,
    /// Logged by Rust game code.
    Custom,
    /// Events of one kind in one step that were counted, not written.
    Omitted,
}

/// The number of [`EventKind`]s.
pub const KINDS: usize = 14;

impl EventKind {
    pub const ALL: [EventKind; KINDS] = [
        EventKind::Input,
        EventKind::Tap,
        EventKind::TapMiss,
        EventKind::Moved,
        EventKind::Blocked,
        EventKind::Trigger,
        EventKind::Signal,
        EventKind::Action,
        EventKind::Skipped,
        EventKind::Spawned,
        EventKind::Counted,
        EventKind::Error,
        EventKind::Custom,
        EventKind::Omitted,
    ];

    pub fn name(self) -> &'static str {
        match self {
            EventKind::Input => "input",
            EventKind::Tap => "tap",
            EventKind::TapMiss => "tap_miss",
            EventKind::Moved => "moved",
            EventKind::Blocked => "blocked",
            EventKind::Trigger => "trigger",
            EventKind::Signal => "signal",
            EventKind::Action => "action",
            EventKind::Skipped => "skipped",
            EventKind::Spawned => "spawned",
            EventKind::Counted => "counted",
            EventKind::Error => "error",
            EventKind::Custom => "custom",
            EventKind::Omitted => "omitted",
        }
    }

    /// Its place in [`EventKind::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn parse(s: &str) -> Option<EventKind> {
        EventKind::ALL.into_iter().find(|k| k.name() == s)
    }
}

/// Reference for one event kind: who the subject (`entity`) and the other
/// party (`other`) are, and which extra fields the JSON carries.
#[derive(Debug)]
pub struct EventDoc {
    pub kind: EventKind,
    pub subject: &'static str,
    pub other: &'static str,
    pub fields: &'static str,
    pub meaning: &'static str,
}

/// One entry per [`EventKind`], in the same order (a test keeps them aligned).
pub const EVENT_DOCS: &[EventDoc] = &[
    EventDoc {
        kind: EventKind::Input,
        subject: "—",
        other: "—",
        fields: "pressed",
        meaning: "Actions that went down this step.",
    },
    EventDoc {
        kind: EventKind::Tap,
        subject: "the entity hit (none for a ground tap)",
        other: "—",
        fields: "x, y, signal; cell (ground taps)",
        meaning: "A tap hit the topmost enabled entity under the point; it emits `tapped`. A tap that hits no \
                  entity but lands on a ground cell is a ground tap when a [[connect]] listens to \
                  `from = \"ground\"`: the ground emits `tapped` with that `cell`.",
    },
    EventDoc {
        kind: EventKind::TapMiss,
        subject: "topmost disabled entity under the point, if any",
        other: "—",
        fields: "x, y, under_disabled; cell (when over the ground)",
        meaning: "A tap hit nothing enabled (and, over the ground, nothing listens to the ground's `tapped`).",
    },
    EventDoc {
        kind: EventKind::Moved,
        subject: "the entity that moved",
        other: "the pusher (when `cause` is `pushed`)",
        fields: "cause, from, to, by",
        meaning: "One cell of movement. `cause` is the direction, `patrol`, `wander` or `pushed` (a pushable \
                  moved by a mover; logged just before the mover's own move).",
    },
    EventDoc {
        kind: EventKind::Blocked,
        subject: "the mover (none for a spawn)",
        other: "the blocker (for `solid` and `actor`), the pushable that would not move (for `push`), the entity \
                in the way (for `occupied`), the payer (for `funds`)",
        fields: "cause, from, to, reason, frames_left; beyond, push_blocked, push_blocked_by (for `push`); \
                 prefab, at, cell, terrain, cost (for a spawn)",
        meaning: "A move or a placement was refused. For moves, `reason`: solid, actor, edge, area, cooldown, or \
                  push (the pushable in the way could not move: `push_blocked` says why). For a `spawn` \
                  (`cause = \"spawn\"`): funds (the payer of its `cost` has too little; checked first, `other` = \
                  the payer, `cost` = what it needed), edge (a cell outside the ground), terrain (a cell whose \
                  terrain has `build = false`) or occupied (an enabled entity covers a cell); `cell` is the cell \
                  that failed. \
                  When a [[connect]] listens, the source of the spawn's signal (the ground, for ground taps) then \
                  emits `refused` on `at` (`signal = \"refused\"` here).",
    },
    EventDoc {
        kind: EventKind::Trigger,
        subject: "the trigger",
        other: "the actor that entered or left",
        fields: "signal, by",
        meaning: "The trigger's cell became occupied or empty; it emits `signal`.",
    },
    EventDoc {
        kind: EventKind::Signal,
        subject: "the emitter",
        other: "—",
        fields: "signal, connections; reason (for `refused`)",
        meaning: "A signal was dispatched to this many `[[connect]]` tables (0 = nothing listens). A `refused` \
                  signal carries the spawn's `reason` (edge, terrain or occupied), which `when = [{ reason = ... }]` \
                  tests.",
    },
    EventDoc {
        kind: EventKind::Action,
        subject: "the target",
        other: "the emitter",
        fields: "signal, action, before, after, changed; prop, value (add/set); when (each condition, held, seen)",
        meaning: "A connection's action ran on its target: enable, disable, toggle, or add/set a prop \
                  (`before`/`after` are the enabled state or the prop's values). It is logged even when nothing \
                  changes (`enable` on an enabled entity: \"stays enabled\", `changed = false`), so \
                  `[[expect]] event = \"action\"` counts every time the connection fired; add `changed = true` to \
                  count only real changes.",
    },
    EventDoc {
        kind: EventKind::Skipped,
        subject: "the target (none for `spawn`)",
        other: "the emitter",
        fields: "signal, action, prop, prefab, cell, when (each condition, held, seen); stopped_by",
        meaning: "A connection's signal arrived, but a `when` condition did not hold, so nothing was done. The \
                  message names the first failing condition and what was seen. One event per action of an \
                  `actions` list; an action after a `spawn` that could not be placed is skipped too \
                  (`stopped_by = \"refused spawn\"`).",
    },
    EventDoc {
        kind: EventKind::Spawned,
        subject: "the new entity",
        other: "the emitter of the signal (none for a ground tap)",
        fields: "prefab, cell, cells; paid (entity, prop, amount, before, after) with a `cost`",
        meaning: "A `spawn` action placed a new copy of `prefab` with its back corner on `cell`, covering `cells`. \
                  It gets the next free id (`#N (prefab)` in messages) and emits `spawned` if a [[connect]] \
                  listens (`from_prefab`).",
    },
    EventDoc {
        kind: EventKind::Counted,
        subject: "the entity whose `counts` prop changed",
        other: "—",
        fields: "prop, prefab, before, after",
        meaning: "A `counts` prop now has a new number of enabled `prefab` copies, because a copy was spawned, \
                  disabled or enabled. The message says which; the entity then emits `changed` if a [[connect]] listens (once when several \
                  of its counts change together).",
    },
    EventDoc {
        kind: EventKind::Error,
        subject: "the emitter",
        other: "—",
        fields: "code, signal",
        meaning: "A step reached one of the engine's limits and stopped there: more than 1000 signals (E0301) or \
                  10000 actions (E0302) in one step, 10000 count changes (E0303), a spawn beyond 1000000 entities \
                  (E0304), more than 256 taps (E0305); or a spawn of a prefab the world does not have (E0201). The \
                  message says what was cut off; `rwgpu run` reports the code as a diagnostic.",
    },
    EventDoc {
        kind: EventKind::Custom,
        subject: "set by game code",
        other: "set by game code",
        fields: "set by game code",
        meaning: "Logged by Rust game code.",
    },
    EventDoc {
        kind: EventKind::Omitted,
        subject: "—",
        other: "—",
        fields: "of, count, written",
        meaning: "More than 256 `moved`, `blocked` (for a move), `trigger` or `custom` events happened in one step: \
                  the first 256 of that kind (`of`) were written and `count` more were counted, not written. Logged \
                  at the end of the step, once per kind. The entities a run watches (`--watch NAME` on `rwgpu run` \
                  and `rwgpu inspect`, or `inspect --entity NAME --events`) have all of their events written.",
    },
];

/// Markdown table of [`EVENT_DOCS`].
pub fn events_markdown() -> String {
    let mut out = String::from(
        "Every event has `frame` (the step that produced it: `[N]` happened in step N, so it is part of the state \
         from frame N + 1), `kind`, `entity` (the subject), `other` (if any) and `message`.\n\n\
         | kind | `entity` (subject) | `other` | extra fields | meaning |\n|---|---|---|---|---|\n",
    );
    for d in EVENT_DOCS {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            d.kind.name(),
            d.subject,
            d.other,
            d.fields,
            d.meaning
        ));
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// The frame whose step produced the event.
    pub frame: u64,
    pub kind: EventKind,
    /// The entity the event is about.
    pub entity: Option<EntityId>,
    /// A second entity involved (blocker, signal source, ...).
    pub other: Option<EntityId>,
    /// Signal name for trigger/signal/action events.
    pub signal: Option<String>,
    /// One self-contained English sentence.
    pub message: String,
    /// Kind-specific structured fields.
    pub data: Json,
}

/// What a run writes in full, beyond the per-step budget of
/// [`EVENTS_PER_KIND_PER_STEP`]: the events about some entities (as subject
/// or other party), the events of some kinds, or everything. Watching
/// changes which events are written, never what happens.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Watch {
    /// Every event is written, as if there were no budget.
    pub all: bool,
    /// In id order, each once.
    entities: Vec<EntityId>,
    kinds: Vec<EventKind>,
}

impl Watch {
    /// Write every event.
    pub fn everything() -> Watch {
        Watch { all: true, ..Watch::default() }
    }

    pub fn entity(&mut self, id: EntityId) {
        if let Err(at) = self.entities.binary_search(&id) {
            self.entities.insert(at, id);
        }
    }

    pub fn kind(&mut self, kind: EventKind) {
        if !self.kinds.contains(&kind) {
            self.kinds.push(kind);
        }
    }

    pub fn entities(&self) -> &[EntityId] {
        &self.entities
    }

    pub fn kinds(&self) -> &[EventKind] {
        &self.kinds
    }

    pub fn is_empty(&self) -> bool {
        !self.all && self.entities.is_empty() && self.kinds.is_empty()
    }

    fn covers(&self, kind: EventKind, entity: Option<EntityId>, other: Option<EntityId>) -> bool {
        let watched = |id: Option<EntityId>| id.is_some_and(|id| self.entities.binary_search(&id).is_ok());
        self.all || self.kinds.contains(&kind) || watched(entity) || watched(other)
    }
}

/// The event log: what was written, in order, and a count of everything that
/// happened. Bounded twice: per step, a budget for the per-actor kinds
/// ([`EventLog::admit`]); and in all, [`EVENT_LOG`] events, beyond which the
/// oldest half is dropped. Both are counted per kind.
#[derive(Clone, Debug)]
pub struct EventLog {
    events: Vec<Event>,
    dropped: u64,
    cap: usize,
    budget: u32,
    watch: Watch,
    /// Every event so far, written or not, per kind.
    happened: [u64; KINDS],
    /// Counted, not written, per kind.
    omitted: [u64; KINDS],
    /// Written, then dropped because the log was full, per kind.
    dropped_kinds: [u64; KINDS],
    /// The step the two counters below are for.
    step: u64,
    /// Written under the budget in this step, per kind.
    step_written: [u32; KINDS],
    /// Not written in this step, per kind.
    step_omitted: [u64; KINDS],
}

impl Default for EventLog {
    fn default() -> Self {
        EventLog {
            events: Vec::new(),
            dropped: 0,
            cap: EVENT_LOG.get(),
            budget: EVENTS_PER_KIND_PER_STEP.value as u32,
            watch: Watch::default(),
            happened: [0; KINDS],
            omitted: [0; KINDS],
            dropped_kinds: [0; KINDS],
            step: 0,
            step_written: [0; KINDS],
            step_omitted: [0; KINDS],
        }
    }
}

impl EventLog {
    /// Write an event (and count it).
    pub fn push(&mut self, e: Event) {
        if self.events.len() >= self.cap {
            let half = self.cap / 2;
            for old in self.events.drain(..half) {
                self.dropped_kinds[old.kind.index()] += 1;
            }
            self.dropped += half as u64;
        }
        self.happened[e.kind.index()] += 1;
        self.events.push(e);
    }

    /// Should an event of a per-actor kind about `entity` (and `other`) be
    /// written? Asked before it is built: when the answer is no, it has been
    /// counted, and nothing else is to be done. When it is yes, build it and
    /// [`push`](EventLog::push) it. Yes for the first events of each kind in a
    /// step, up to the budget, and for whatever the watch covers.
    pub fn admit(&mut self, frame: u64, kind: EventKind, entity: Option<EntityId>, other: Option<EntityId>) -> bool {
        if frame != self.step {
            self.close_step();
            self.step = frame;
        }
        let k = kind.index();
        if self.watch.covers(kind, entity, other) {
            return true;
        }
        if self.step_written[k] < self.budget {
            self.step_written[k] += 1;
            return true;
        }
        self.step_omitted[k] += 1;
        self.omitted[k] += 1;
        self.happened[k] += 1;
        false
    }

    /// The end of a step: one `omitted` event for each kind that had events
    /// counted and not written in it.
    pub fn close_step(&mut self) {
        for kind in EventKind::ALL {
            let n = std::mem::take(&mut self.step_omitted[kind.index()]);
            if n == 0 {
                continue;
            }
            let written = self.step_written[kind.index()];
            let message = format!(
                "{n} more '{}' events in this step were counted, not written: the first {written} of a kind are \
                 written each step. Watch an entity to have all of its events written (`--watch NAME` on rwgpu run \
                 and inspect)",
                kind.name()
            );
            let data = Json::object().with("of", kind.name()).with("count", n).with("written", written);
            let frame = self.step;
            self.push(Event {
                frame,
                kind: EventKind::Omitted,
                entity: None,
                other: None,
                signal: None,
                message,
                data,
            });
        }
        self.step_written = [0; KINDS];
    }

    /// What is written in full from now on.
    pub fn set_watch(&mut self, watch: Watch) {
        self.watch = watch;
    }

    pub fn watch(&self) -> &Watch {
        &self.watch
    }

    pub fn all(&self) -> &[Event] {
        &self.events
    }

    /// Number of events dropped because the log was full.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Events involving `id` (as subject or other party).
    pub fn about(&self, id: EntityId) -> impl Iterator<Item = &Event> {
        self.events.iter().filter(move |e| e.entity == Some(id) || e.other == Some(id))
    }

    /// How many events of each kind happened, written or not, in kind order,
    /// omitting zeros.
    pub fn counts(&self) -> Vec<(EventKind, u64)> {
        Self::nonzero(&self.happened)
    }

    /// How many events of each kind happened in all, written or not.
    pub fn happened(&self) -> u64 {
        self.happened.iter().sum()
    }

    /// How many events of each kind were counted and not written, omitting zeros.
    pub fn omitted(&self) -> Vec<(EventKind, u64)> {
        Self::nonzero(&self.omitted)
    }

    /// How many events of each kind were written and then dropped because
    /// the log was full, omitting zeros.
    pub fn dropped_kinds(&self) -> Vec<(EventKind, u64)> {
        Self::nonzero(&self.dropped_kinds)
    }

    fn nonzero(counts: &[u64; KINDS]) -> Vec<(EventKind, u64)> {
        EventKind::ALL.into_iter().map(|k| (k, counts[k.index()])).filter(|(_, n)| *n > 0).collect()
    }
}
