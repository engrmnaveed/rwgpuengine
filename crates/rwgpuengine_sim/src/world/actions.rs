//! What a connection does to its target: enable, disable, toggle, add, set
//! (a `spawn` is placement's).

use rwgpuengine_core::json::Json;
use rwgpuengine_data::{ConnectAction, PropValue};

use super::counting::Stale;
use super::wiring::{Act, Target};
use super::{EntityId, World, fmt_cell};
use crate::dump::prop_json;
use crate::events::EventKind;

/// What every action of one connection run shares: the signal, where it
/// happened, how messages say where it came from, and the `when` results.
pub(super) struct ActCtx<'a> {
    pub(super) emitter: Option<EntityId>,
    pub(super) signal: &'a str,
    pub(super) cell: [i32; 2],
    pub(super) via: &'a str,
    pub(super) when: &'a [Json],
    pub(super) held: &'a [&'a str],
    /// 1-based position in an `actions` list of two or more.
    pub(super) item: Option<usize>,
}

impl World {
    /// The action's target (`None` for spawn) and how messages name it.
    pub(super) fn act_target(&self, a: &Act, emitter: Option<EntityId>) -> (Option<EntityId>, String) {
        let to = match a.to {
            Target::Entity(id) => Some(id),
            Target::Emitter => emitter,
            Target::None => None,
        };
        let label = match (to, &a.prefab) {
            (Some(id), _) => self.describe(id),
            (None, Some(p)) => format!("spawn '{p}'"),
            (None, None) => "nothing".into(),
        };
        (to, label)
    }

    fn act_data(a: &Act, ctx: &ActCtx) -> Json {
        let mut data = Json::object().with("signal", ctx.signal).with("action", a.action.name());
        if let Some(p) = &a.prop {
            data.set("prop", p.as_str());
        }
        if let Some(p) = &a.prefab {
            data.set("prefab", p.as_str());
            data.set("cell", ctx.cell);
        }
        if !ctx.when.is_empty() {
            data.set("when", ctx.when.to_vec());
        }
        if let Some(i) = ctx.item {
            data.set("item", i);
        }
        data
    }

    /// Log a `skipped` event for an action that did not run.
    pub(super) fn skip_action(&mut self, a: &Act, ctx: &ActCtx, why: &str, after_refusal: bool) {
        let (to, target) = self.act_target(a, ctx.emitter);
        let mut data = Self::act_data(a, ctx);
        if after_refusal {
            data.set("stopped_by", "refused spawn");
        }
        let msg = if a.action == ConnectAction::Spawn {
            format!("{target} at {} skipped ({}): {why}", fmt_cell(ctx.cell), ctx.via)
        } else {
            format!("{target} {} skipped ({}): {why}", a.action.name(), ctx.via)
        };
        // For spawn the subject is nothing yet; the emitter is the other party.
        self.write(EventKind::Skipped, to, ctx.emitter, msg, data);
    }

    /// Apply one action. Returns false only for a `spawn` that could not be placed.
    pub(super) fn run_action(&mut self, a: &Act, ctx: &ActCtx) -> bool {
        if a.action == ConnectAction::Spawn {
            let prefab = a.prefab.as_deref().unwrap_or_default();
            let turn = self.spawn_turn(prefab, a.turn.as_ref());
            return self.spawn(prefab, a.cost.as_ref(), turn, ctx.cell, ctx.emitter, ctx.via, ctx.held);
        }
        let (to, target) = self.act_target(a, ctx.emitter);
        let Some(to) = to else { return true };
        let mut data = Self::act_data(a, ctx);
        let mut enabled_changed = false;
        let (change, prop_changed) = match a.action {
            ConnectAction::Spawn => return true,
            ConnectAction::Add | ConnectAction::Set => match self.change_prop(a, to) {
                Some((change, before, value, after)) => {
                    data = data
                        .with("value", prop_json(&value))
                        .with("before", prop_json(&before))
                        .with("after", prop_json(&after))
                        .with("changed", after != before);
                    (change, after != before)
                }
                None => return true,
            },
            ConnectAction::Enable | ConnectAction::Disable | ConnectAction::Toggle => {
                let Some(e) = self.entities.0.get_mut(to as usize) else { return true };
                let before = e.enabled;
                e.enabled = match a.action {
                    ConnectAction::Enable => true,
                    ConnectAction::Disable => false,
                    _ => !before,
                };
                let after = e.enabled;
                enabled_changed = before != after;
                // On or off its cells, in or out of the copies counted.
                self.resync(to);
                data = data.with("before", before).with("after", after).with("changed", before != after);
                let state = |b: bool| if b { "enabled" } else { "disabled" };
                let change = if before == after {
                    format!("{}: stays {}", a.action.name(), state(after))
                } else {
                    format!("{}: {} -> {}", a.action.name(), state(before), state(after))
                };
                (change, false)
            }
        };
        let mut msg = format!("{target} {change} ({})", ctx.via);
        if !ctx.held.is_empty() {
            msg.push_str(&format!("; when {} held", ctx.held.join(", ")));
        }
        self.write(EventKind::Action, Some(to), ctx.emitter, msg, data);
        // A prop change makes its owner emit `changed`, if anything listens.
        if prop_changed && self.is_listened(Some(to), "changed") {
            self.emit(to, "changed");
        }
        if enabled_changed {
            let now = if self.get(to).is_some_and(|e| e.enabled) { "enabled" } else { "disabled" };
            let why = format!("{} was {now}", self.describe(to));
            let prefab = self.get(to).and_then(|e| e.prefab.clone());
            self.refresh_counts(&why, Stale::Prefab(prefab.as_deref(), None));
        }
        true
    }

    /// Apply `add`/`set`. Returns the message part, before, value and after.
    /// Props keep their declared type (validated by `rwgpu check`).
    fn change_prop(&mut self, c: &Act, to: EntityId) -> Option<(String, PropValue, PropValue, PropValue)> {
        let prop = c.prop.as_deref()?;
        let value = c.value.clone().unwrap_or(PropValue::Int(1));
        let current = self.entities.0.get_mut(to as usize)?.props.get_mut(prop)?;
        let before = current.clone();
        let after = match (c.action, &before) {
            (ConnectAction::Add, PropValue::Int(a)) => match value {
                PropValue::Int(b) => PropValue::Int(a.saturating_add(b)),
                _ => before.clone(),
            },
            (ConnectAction::Add, PropValue::Float(a)) => PropValue::Float(a + value.as_num().unwrap_or(0.0)),
            (ConnectAction::Set, _) if before.accepts(&value) => before.coerce(&value),
            _ => before.clone(),
        };
        *current = after.clone();
        let what = match c.action {
            ConnectAction::Add => format!("add {prop} {}", value.display()),
            _ => format!("set {prop} = {}", value.literal()),
        };
        let change = if after == before {
            format!("{what}: stays {}", after.literal())
        } else {
            format!("{what}: {} -> {}", before.literal(), after.literal())
        };
        Some((change, before, value, after))
    }
}
