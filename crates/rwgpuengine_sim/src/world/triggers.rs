//! Triggers: who stands on a plate, and the signals when that changes.

use rwgpuengine_core::json::Json;

use super::{EntityId, World, fmt_cell};
use crate::events::EventKind;

impl World {
    /// Enabled actors on an entity's cell, other than itself. For a trigger,
    /// only those its `only` accepts.
    pub fn occupants_of(&self, id: EntityId) -> Vec<EntityId> {
        let t = &self.entities[id as usize];
        let only = t.trigger.as_ref().and_then(|tr| tr.def.only.as_ref());
        self.entities
            .iter()
            .filter(|e| e.id != id && e.enabled && e.holds_plates() && t.covers(e.cell()))
            .filter(|e| only.is_none_or(|only| e.name.as_ref() == Some(only)))
            .map(|e| e.id)
            .collect()
    }

    pub(super) fn run_triggers(&mut self) {
        for id in self.ids_with(|e| e.enabled && e.trigger.is_some()) {
            let now = self.occupants_of(id);
            let trigger = self.entities[id as usize].trigger.as_mut().expect("trigger");
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
