//! The event log: a structured record of everything that happened and,
//! just as important, everything that was attempted and refused.
//!
//! "Why didn't the gate open?" is answered by reading events, not code:
//! the plate's `trigger` event, the `signal` event with its connection
//! count, the `action` event on the gate, or the `blocked` event that kept
//! the player from reaching the plate.

use slate_core::json::Json;

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
    Error,
    /// Logged by Rust game code.
    Custom,
}

impl EventKind {
    pub const ALL: [EventKind; 12] = [
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
        EventKind::Error,
        EventKind::Custom,
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
            EventKind::Error => "error",
            EventKind::Custom => "custom",
        }
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
                in the way (for `occupied`)",
        fields: "cause, from, to, reason, frames_left; beyond, push_blocked, push_blocked_by (for `push`); \
                 prefab, at, cell, terrain (for a spawn)",
        meaning: "A move or a placement was refused. For moves, `reason`: solid, actor, edge, area, cooldown, or \
                  push (the pushable in the way could not move: `push_blocked` says why). For a `spawn` \
                  (`cause = \"spawn\"`): edge (a cell outside the ground), terrain (a cell whose terrain has \
                  `build = false`) or occupied (an enabled entity covers a cell); `cell` is the cell that failed.",
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
        fields: "signal, connections",
        meaning: "A signal was dispatched to this many `[[connect]]` tables (0 = nothing listens).",
    },
    EventDoc {
        kind: EventKind::Action,
        subject: "the target",
        other: "the emitter",
        fields: "signal, action, before, after, changed; prop, value (add/set); when (each condition, held, seen)",
        meaning: "A connection's action ran on its target: enable, disable, toggle, or add/set a prop \
                  (`before`/`after` are the enabled state or the prop's values). It is logged even when nothing \
                  changes (`enable` on an enabled entity: \"stays enabled\", `changed = false`), so \
                  `[[expect]] event = \"action\"` counts every time the connection fired.",
    },
    EventDoc {
        kind: EventKind::Skipped,
        subject: "the target (none for `spawn`)",
        other: "the emitter",
        fields: "signal, action, prop, prefab, cell, when (each condition, held, seen)",
        meaning: "A connection's signal arrived, but a `when` condition did not hold, so nothing was done. The \
                  message names the first failing condition and what was seen.",
    },
    EventDoc {
        kind: EventKind::Spawned,
        subject: "the new entity",
        other: "the emitter of the signal (none for a ground tap)",
        fields: "prefab, cell, cells",
        meaning: "A `spawn` action placed a new copy of `prefab` with its back corner on `cell`, covering `cells`. \
                  It gets the next free id (`#N (prefab)` in messages) and emits `spawned` if a [[connect]] \
                  listens (`from_prefab`).",
    },
    EventDoc {
        kind: EventKind::Error,
        subject: "the emitter",
        other: "—",
        fields: "code, signal",
        meaning: "A runtime error, such as more than 1000 signals in one step (E0301).",
    },
    EventDoc {
        kind: EventKind::Custom,
        subject: "set by game code",
        other: "set by game code",
        fields: "set by game code",
        meaning: "Logged by Rust game code.",
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

/// Bounded log. When full, the oldest half is dropped and counted.
#[derive(Clone, Debug)]
pub struct EventLog {
    events: Vec<Event>,
    dropped: u64,
    cap: usize,
}

impl Default for EventLog {
    fn default() -> Self {
        EventLog { events: Vec::new(), dropped: 0, cap: 100_000 }
    }
}

impl EventLog {
    pub fn push(&mut self, e: Event) {
        if self.events.len() >= self.cap {
            let half = self.cap / 2;
            self.events.drain(..half);
            self.dropped += half as u64;
        }
        self.events.push(e);
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

    /// Count per kind, in kind order, omitting zeros.
    pub fn counts(&self) -> Vec<(EventKind, usize)> {
        EventKind::ALL
            .into_iter()
            .map(|k| (k, self.events.iter().filter(|e| e.kind == k).count()))
            .filter(|(_, n)| *n > 0)
            .collect()
    }
}
