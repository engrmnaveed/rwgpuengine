//! Signals: queued while a step runs, dispatched at its end through the
//! `[[connect]]` wiring, each connection after its `when` conditions.

use rwgpuengine_core::json::Json;
use rwgpuengine_data::CondTest;

use std::rc::Rc;

use rwgpuengine_core::limits::{ACTIONS_PER_STEP, SIGNALS_PER_STEP};

use super::actions::ActCtx;
use super::counting::Stale;
use super::wiring::{Condition, Connection};
use super::{Entity, EntityId, SIGNAL_LIMIT, World, fmt_cell, scan};
use crate::events::EventKind;

/// A queued signal: who emitted it (`None` = the ground) and the cell it
/// happened on (the tapped cell, or the emitter's cell).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Pending {
    pub(super) emitter: Option<EntityId>,
    pub(super) signal: String,
    pub(super) cell: [i32; 2],
    /// For `refused`: why the spawn could not be placed.
    pub(super) reason: Option<&'static str>,
}

impl World {
    /// Queue a signal; it is dispatched at the end of the current step.
    pub fn emit(&mut self, from: EntityId, signal: &str) {
        let cell = self.get(from).map_or([0, 0], Entity::cell);
        self.pending.push_back(Pending { emitter: Some(from), signal: signal.to_string(), cell, reason: None });
    }

    /// Is any connection listening to `signal` from `emitter` (`None` = the ground)?
    pub fn is_listened(&self, emitter: Option<EntityId>, signal: &str) -> bool {
        let prefab = emitter.and_then(|e| self.get(e)).and_then(|e| e.prefab.as_deref());
        let found = self.routes.any(emitter, prefab, signal);
        if self.checking() {
            let scanned = !scan::listeners(&self.connections, &self.entities, emitter, signal).is_empty();
            assert_eq!(found, scanned, "the routes and the scan disagree on whether '{signal}' is listened to");
        }
        found
    }

    /// The connections that listen to `signal` from `emitter` (`None` = the
    /// ground), by their place in the file, in file order.
    fn listeners(&self, emitter: Option<EntityId>, signal: &str) -> Vec<u32> {
        let prefab = emitter.and_then(|e| self.get(e)).and_then(|e| e.prefab.as_deref());
        let found = self.routes.listeners(emitter, prefab, signal);
        if self.checking() {
            let scanned = scan::listeners(&self.connections, &self.entities, emitter, signal);
            assert_eq!(found, scanned, "the routes and the scan disagree on who listens to '{signal}'");
        }
        found
    }

    /// Evaluate a condition now: whether it holds, and what was seen.
    /// `reason` is the refusal reason when the signal is `refused`.
    pub fn check_condition(&self, c: &Condition, reason: Option<&str>) -> (bool, String) {
        if let CondTest::Reason(want) = &c.def.test {
            return match reason {
                Some(r) => (r == want, format!("the reason is '{r}'")),
                None => (false, "the signal is not a refusal".into()),
            };
        }
        let Some((id, e)) = c.entity.and_then(|id| Some((id, self.get(id)?))) else {
            return (false, "entity missing".into());
        };
        match &c.def.test {
            CondTest::Reason(_) => (false, String::new()),
            CondTest::Occupied(want) => {
                let occ = self.occupants_of(id);
                let seen = match occ.first() {
                    Some(&o) => format!("{} is on {}", self.describe(o), fmt_cell(e.cell())),
                    None => format!("nobody is on {}", fmt_cell(e.cell())),
                };
                (occ.is_empty() != *want, seen)
            }
            CondTest::Enabled(want) => {
                (e.enabled == *want, if e.enabled { "it is enabled".into() } else { "it is disabled".into() })
            }
            CondTest::Prop { prop, equals, at_least, at_most } => {
                let Some(v) = e.props.get(prop) else {
                    return (false, format!("{prop} is not set"));
                };
                let n = v.as_num();
                let held = equals.as_ref().is_none_or(|x| v.same(x))
                    && at_least.is_none_or(|lo| n.is_some_and(|n| n >= lo))
                    && at_most.is_none_or(|hi| n.is_some_and(|n| n <= hi));
                (held, format!("{prop} is {}", v.literal()))
            }
        }
    }

    pub(super) fn source_name(&self, emitter: Option<EntityId>) -> String {
        match emitter {
            Some(id) => self.describe(id),
            None => "the ground".into(),
        }
    }

    pub(super) fn dispatch_signals(&mut self) {
        // Custom systems may have enabled or disabled copies.
        self.refresh_counts("", Stale::All);
        let mut dispatched = 0;
        while !self.work.cut
            && let Some(Pending { emitter, signal, cell, reason }) = self.pending.pop_front()
        {
            dispatched += 1;
            if dispatched > SIGNAL_LIMIT {
                let msg = format!(
                    "more than {SIGNAL_LIMIT} signals in one frame (last: '{signal}' from {}); dropping the rest. \
                     This is usually a loop of connections (E0301)",
                    self.source_name(emitter)
                );
                let data = Json::object().with("code", "E0301").with("signal", signal.as_str());
                self.cut_step(&SIGNALS_PER_STEP, emitter, None, msg, data);
                break;
            }
            // Shared, not copied: the connections never change.
            let connections = Rc::clone(&self.connections);
            let targets: Vec<&Connection> =
                self.listeners(emitter, &signal).iter().map(|i| &connections[*i as usize]).collect();
            let source = self.source_name(emitter);
            let msg = if targets.is_empty() {
                let hint = match emitter.and_then(|id| self.get(id)) {
                    Some(e) => match (&e.name, &e.prefab) {
                        (None, Some(p)) => format!("from_prefab = \"{p}\""),
                        _ => format!("from = \"{}\"", e.label()),
                    },
                    None => "from = \"ground\"".to_string(),
                };
                format!(
                    "{source} emitted '{signal}' but nothing is connected to it (add [[connect]] {hint} signal = \"{signal}\")"
                )
            } else {
                let why = reason.map_or(String::new(), |r| format!(" (reason '{r}')"));
                format!("{source} emitted '{signal}'{why} to {} connection(s)", targets.len())
            };
            let mut data = Json::object().with("signal", signal.as_str()).with("connections", targets.len());
            if let Some(r) = reason {
                data.set("reason", r);
            }
            self.write(EventKind::Signal, emitter, None, msg, data);
            for c in targets {
                if self.work.cut {
                    break;
                }
                self.run_connection(c, emitter, &signal, cell, reason);
            }
        }
    }

    /// Check a connection's conditions once, then apply its actions in order
    /// (or log why not). A refused `spawn` stops the actions after it.
    fn run_connection(
        &mut self,
        c: &Connection,
        emitter: Option<EntityId>,
        signal: &str,
        cell: [i32; 2],
        reason: Option<&'static str>,
    ) {
        let results: Vec<(String, bool, String)> = c
            .when
            .iter()
            .map(|w| {
                let (held, seen) = self.check_condition(w, reason);
                (w.def.describe(), held, seen)
            })
            .collect();
        let when_json: Vec<Json> = results
            .iter()
            .map(|(cond, held, seen)| {
                Json::object().with("condition", cond.as_str()).with("held", *held).with("seen", seen.as_str())
            })
            .collect();
        let via = match emitter.and_then(|id| self.get(id)) {
            Some(e) => format!("via {}.'{signal}'", e.label()),
            None => format!("via the ground's '{signal}' at {}", fmt_cell(cell)),
        };
        let failed =
            results.iter().find(|(_, held, _)| !held).map(|(cond, _, seen)| format!("{cond} does not hold ({seen})"));
        let held: Vec<&str> = results.iter().map(|(cond, _, _)| cond.as_str()).collect();
        let n = c.actions.len();
        let mut stopped: Option<String> = None;
        for (i, a) in c.actions.iter().enumerate() {
            if self.work.cut {
                return;
            }
            // In a list, each message says which item it is.
            let via = if n > 1 { format!("{via}; action {} of {n}", i + 1) } else { via.clone() };
            if self.work.actions >= ACTIONS_PER_STEP.get() {
                let (to, target) = self.act_target(a, emitter);
                let limit = ACTIONS_PER_STEP.value;
                let what = match &a.prop {
                    Some(prop) => format!("{} {prop}", a.action.name()),
                    None => a.action.name().to_string(),
                };
                let msg = format!(
                    "more than {limit} actions in one frame (stopped at {target} {what}, {via}); dropping the rest \
                     of this frame's signals. Usually connections that feed each other through 'changed' (E0302)"
                );
                let data = Json::object().with("code", "E0302").with("signal", signal).with("action", a.action.name());
                self.cut_step(&ACTIONS_PER_STEP, to, emitter, msg, data);
                return;
            }
            self.work.actions += 1;
            let item = (n > 1).then_some(i + 1);
            let ctx = ActCtx { emitter, signal, cell, via: &via, when: &when_json, held: &held, item };
            match failed.as_ref().or(stopped.as_ref()) {
                Some(why) => {
                    let why = why.clone();
                    self.skip_action(a, &ctx, &why, stopped.is_some() && failed.is_none());
                }
                None => {
                    if !self.run_action(a, &ctx) {
                        let p = a.prefab.as_deref().unwrap_or_default();
                        stopped = Some(format!("the '{p}' spawn before it in this [[connect]] was refused"));
                    }
                }
            }
        }
    }
}
