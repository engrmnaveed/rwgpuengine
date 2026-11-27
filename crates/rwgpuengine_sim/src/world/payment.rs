//! A spawn's `cost`: whether the payer has enough, and taking it (ADR 0016).

use rwgpuengine_core::json::Json;
use rwgpuengine_data::PropValue;

use super::placement::Refusal;
use super::wiring::Cost;
use super::{Entity, World};
use crate::dump::prop_json;

impl World {
    /// Is there enough to pay `cost` (none: always) for a copy at `at`?
    pub(super) fn can_afford(&self, cost: Option<&Cost>, at: [i32; 2]) -> Result<(), Refusal> {
        let Some(k) = cost else { return Ok(()) };
        let have = self.get(k.entity).and_then(|e| e.props.get(&k.prop)).cloned();
        let need = k.amount.as_num().unwrap_or(0.0);
        if have.as_ref().and_then(PropValue::as_num).is_some_and(|h| h >= need) {
            return Ok(());
        }
        let owner = self.get(k.entity).map_or_else(|| format!("#{}", k.entity), Entity::label);
        let shown = have.map_or("unset".to_string(), |v| v.literal());
        let why = format!("it costs {} and '{owner}'.{} is {shown}", k.amount.display(), k.prop);
        Err(Refusal { reason: "funds", cell: at, by: Some(k.entity), terrain: None, why })
    }

    /// Take `cost` from the payer. Returns the prop before and after.
    pub(super) fn pay(&mut self, k: &Cost) -> Option<(PropValue, PropValue)> {
        let current = self.entities.0.get_mut(k.entity as usize)?.props.get_mut(&k.prop)?;
        let before = current.clone();
        let after = match (&before, &k.amount) {
            (PropValue::Int(a), PropValue::Int(b)) => PropValue::Int(a.saturating_sub(*b)),
            (PropValue::Float(a), b) => PropValue::Float(a - b.as_num().unwrap_or(0.0)),
            _ => before.clone(),
        };
        *current = after.clone();
        Some((before, after))
    }

    pub(super) fn cost_json(&self, k: &Cost) -> Json {
        let owner = self.get(k.entity).map_or_else(|| format!("#{}", k.entity), Entity::label);
        Json::object().with("entity", owner.as_str()).with("prop", k.prop.as_str()).with("amount", prop_json(&k.amount))
    }
}
