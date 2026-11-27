//! `counts` props: kept equal to the enabled copies of a prefab (ADR 0014).

use rwgpuengine_core::json::Json;
use rwgpuengine_data::PropValue;

use super::{Entity, EntityId, World, scan};
use crate::events::EventKind;

impl World {
    /// Store a count (the prop exists: loading declares every `counts` prop).
    pub(super) fn set_count(&mut self, id: EntityId, prop: &str, n: i64) {
        if let Some(v) = self.entities.0[id as usize].props.get_mut(prop) {
            *v = PropValue::Int(n);
        }
    }

    /// Count props whose value is out of date: (owner, prop, prefab, before, now).
    pub(super) fn count_changes(&self) -> Vec<(EntityId, String, String, Option<i64>, i64)> {
        let mut out = Vec::new();
        for &id in &self.counters {
            let e = &self.entities[id as usize];
            for (prop, prefab) in &e.counts {
                let n = self.index.copies(prefab);
                if self.checking() {
                    let scanned = scan::copies(&self.entities, prefab);
                    assert_eq!(n, scanned, "the index and the scan disagree on the enabled copies of '{prefab}'");
                }
                let before = match e.props.get(prop) {
                    Some(PropValue::Int(v)) => Some(*v),
                    _ => None,
                };
                if before != Some(n as i64) {
                    out.push((id, prop.clone(), prefab.clone(), before, n as i64));
                }
            }
        }
        out
    }

    /// Bring `counts` props up to date: log `counted` for each change and
    /// make its owner emit `changed` (once), if anything listens.
    pub(super) fn refresh_counts(&mut self, why: &str) {
        if self.counters.is_empty() {
            return;
        }
        let mut emitted: Vec<EntityId> = Vec::new();
        for (id, prop, prefab, before, n) in self.count_changes() {
            self.set_count(id, &prop, n);
            let who = self.get(id).map_or_else(|| format!("#{id}"), Entity::label);
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
            self.log(EventKind::Counted, Some(id), None, msg, data);
            if listened {
                emitted.push(id);
                self.emit(id, "changed");
            }
        }
    }
}
