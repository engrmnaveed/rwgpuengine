//! `counts` props: kept equal to the enabled copies of a prefab (ADR 0014).
//!
//! A change to one entity changes the copies of its prefab only, so after
//! one the world looks only at the props that count that prefab
//! (`counted_by`); once a step (custom systems may have changed anything)
//! and when the world is built, it looks at all of them. The answers are the
//! same as looking at all of them every time, in the same order, because
//! every count is up to date before each change (ADR 0027).

use rwgpuengine_core::json::Json;
use rwgpuengine_core::limits::COUNT_CHANGES_PER_STEP;
use rwgpuengine_data::PropValue;

use super::{Entity, EntityId, World, scan};
use crate::events::EventKind;

/// Which `counts` props may be out of date.
#[derive(Clone, Copy, Debug)]
pub(super) enum Stale<'a> {
    /// Any of them.
    All,
    /// Those that count copies of this prefab, and every one of this entity
    /// (just spawned: its counts start at its prefab's numbers).
    Prefab(Option<&'a str>, Option<EntityId>),
}

/// A count to bring up to date: (owner, prop, prefab, before, now).
type Change = (EntityId, String, String, Option<i64>, i64);

impl World {
    /// Store a count (the prop exists: loading declares every `counts` prop).
    pub(super) fn set_count(&mut self, id: EntityId, prop: &str, n: i64) {
        if let Some(v) = self.entities.0[id as usize].props.get_mut(prop) {
            *v = PropValue::Int(n);
        }
    }

    /// Entity `id` has `counts` props: count it in, after every one before it.
    pub(super) fn add_counter(&mut self, id: EntityId) {
        self.counters.push(id);
        for (_, prefab) in &self.entities[id as usize].counts {
            let list = self.counted_by.entry(prefab.clone()).or_default();
            if list.last() != Some(&id) {
                list.push(id);
            }
        }
    }

    /// The props of entity `id` that are out of date, among those counting
    /// `prefab` (all of them for `None`).
    fn stale_props(&self, id: EntityId, prefab: Option<&str>, out: &mut Vec<Change>) {
        let e = &self.entities[id as usize];
        for (prop, counted) in &e.counts {
            if prefab.is_some_and(|p| p != counted) {
                continue;
            }
            let n = self.index.copies(counted);
            if self.checking() {
                let scanned = scan::copies(&self.entities, counted);
                assert_eq!(n, scanned, "the index and the scan disagree on the enabled copies of '{counted}'");
            }
            let before = match e.props.get(prop) {
                Some(PropValue::Int(v)) => Some(*v),
                _ => None,
            };
            if before != Some(n as i64) {
                out.push((id, prop.clone(), counted.clone(), before, n as i64));
            }
        }
    }

    /// Count props whose value is out of date, in id order and each entity's
    /// props in order.
    pub(super) fn count_changes(&self, stale: Stale) -> Vec<Change> {
        let mut out = Vec::new();
        match stale {
            Stale::All => {
                for &id in &self.counters {
                    self.stale_props(id, None, &mut out);
                }
            }
            Stale::Prefab(prefab, new) => {
                // The new entity has the highest id: it comes last, as in a
                // look at every counter.
                let counting = prefab.and_then(|p| self.counted_by.get(p)).map_or(&[][..], Vec::as_slice);
                for &id in counting.iter().filter(|id| Some(**id) != new) {
                    self.stale_props(id, prefab, &mut out);
                }
                if let Some(id) = new.filter(|id| !self.entities[*id as usize].counts.is_empty()) {
                    self.stale_props(id, None, &mut out);
                }
                if self.checking() {
                    assert_eq!(out, self.count_changes(Stale::All), "a scoped look at counts missed a change");
                }
            }
        }
        out
    }

    /// Bring `counts` props up to date: log `counted` for each change and
    /// make its owner emit `changed` (once), if anything listens. At
    /// [`COUNT_CHANGES_PER_STEP`] in a step the step stops (E0303); the counts
    /// left behind are brought up to date by the next step.
    pub(super) fn refresh_counts(&mut self, why: &str, stale: Stale) {
        if self.counters.is_empty() || self.work.cut {
            return;
        }
        let mut emitted: Vec<EntityId> = Vec::new();
        for (id, prop, prefab, before, n) in self.count_changes(stale) {
            let who = self.get(id).map_or_else(|| format!("#{id}"), Entity::label);
            if self.work.count_changes >= COUNT_CHANGES_PER_STEP.get() {
                let limit = COUNT_CHANGES_PER_STEP.value;
                let msg = format!(
                    "more than {limit} count changes in one frame (at {who}.{prop}, which counts '{prefab}'); \
                     dropping the rest of this frame's signals, and the counts not yet changed change next frame. \
                     Usually `counts` on many entities that count copies being spawned or switched (E0303)"
                );
                let data =
                    Json::object().with("code", "E0303").with("prop", prop.as_str()).with("prefab", prefab.as_str());
                self.cut_step(&COUNT_CHANGES_PER_STEP, Some(id), None, msg, data);
                return;
            }
            self.work.count_changes += 1;
            self.set_count(id, &prop, n);
            let was = before.map_or("unset".to_string(), |b| b.to_string());
            let mut msg = format!("{who}.{prop}: {was} -> {n} (enabled '{prefab}' copies");
            if !why.is_empty() {
                msg.push_str(&format!("; {why}"));
            }
            msg.push(')');
            let listened = !emitted.contains(&id) && self.is_listened(Some(id), "changed");
            if listened {
                msg.push_str("; it emits 'changed'");
            }
            let mut data = Json::object().with("prop", prop.as_str()).with("prefab", prefab.as_str()).with("after", n);
            if let Some(b) = before {
                data.set("before", b);
            }
            self.write(EventKind::Counted, Some(id), None, msg, data);
            if listened {
                emitted.push(id);
                self.emit(id, "changed");
            }
        }
    }
}
