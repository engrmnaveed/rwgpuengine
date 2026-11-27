//! Triggers: who stands on a plate, and the signals when that changes.

use rwgpuengine_core::json::Json;

use super::index::Kind;
use super::{EntityId, World, fmt_cell, scan};
use crate::events::EventKind;

impl World {
    /// Enabled actors on an entity's cell, other than itself. For a trigger,
    /// only those its `only` accepts.
    pub fn occupants_of(&self, id: EntityId) -> Vec<EntityId> {
        let t = &self.entities[id as usize];
        let only = t.trigger.as_ref().and_then(|tr| tr.def.only.as_ref());
        // Every cell `t` covers, as `Entity::covers` has it: none if a side
        // of its footprint is below 1 or would pass the largest cell.
        let [x, y] = t.cell();
        let mut found = Vec::new();
        if let (Some(x1), Some(y1)) = (x.checked_add(t.footprint[0]), y.checked_add(t.footprint[1])) {
            for cy in y..y1 {
                for cx in x..x1 {
                    self.index.weights_on([cx, cy], id, &mut found);
                }
            }
        }
        // Each cell's are in id order; several cells' together are not.
        if t.footprint != [1, 1] {
            found.sort_unstable();
        }
        if let Some(only) = only {
            found.retain(|e| self.entities[*e as usize].name.as_ref() == Some(only));
        }
        if self.checking() {
            let scanned = scan::occupants_of(&self.entities, id);
            assert_eq!(found, scanned, "the index and the scan disagree on who stands on #{id}");
        }
        found
    }

    pub(super) fn run_triggers(&mut self) {
        for id in self.enabled_with(Kind::Trigger) {
            let now = self.occupants_of(id);
            let trigger = self.entities.0[id as usize].trigger.as_mut().expect("trigger");
            let was = std::mem::replace(&mut trigger.occupants, now.clone());
            let (on_enter, on_exit) = (trigger.def.on_enter.clone(), trigger.def.on_exit.clone());
            let cell = self.entities[id as usize].cell();
            if was.is_empty() && !now.is_empty() {
                let msg = format!(
                    "{} at {} was entered by {}; it emits '{on_enter}'",
                    self.describe(id),
                    fmt_cell(cell),
                    self.describe(now[0])
                );
                let data = Json::object().with("signal", on_enter.as_str()).with("by", now[0]);
                self.log(EventKind::Trigger, Some(id), Some(now[0]), msg, data);
                self.emit(id, &on_enter);
            } else if !was.is_empty() && now.is_empty() {
                let msg = format!(
                    "{} at {} was left by {}; it emits '{on_exit}'",
                    self.describe(id),
                    fmt_cell(cell),
                    self.describe(was[0])
                );
                let data = Json::object().with("signal", on_exit.as_str()).with("by", was[0]);
                self.log(EventKind::Trigger, Some(id), Some(was[0]), msg, data);
                self.emit(id, &on_exit);
            }
        }
    }
}
