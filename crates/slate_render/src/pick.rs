//! Picking: which entity is drawn at a screen point.
//!
//! Walks the draw list front to back using the same coverage test the
//! rasterizer uses, so a tap hits exactly what is visible there. Disabled
//! entities in front of the hit are collected so a missed tap can say
//! "the button under your finger is disabled".

use slate_sim::{EntityId, Tap};

use crate::draw::DrawList;

/// Resolve a tap at screen pixel (x, y). The list must be built with
/// `include_disabled = true` for misses to name disabled entities.
pub fn pick(list: &DrawList, x: f32, y: f32) -> Tap {
    let (px, py) = (x.floor() as i32, y.floor() as i32);
    let mut tap = Tap { x, y, hit: None, under_disabled: Vec::new() };
    for item in list.items.iter().rev() {
        if !item.shape.covers(px, py) {
            continue;
        }
        let Some(id) = item.entity else {
            break; // the ground stops the search
        };
        if item.enabled {
            tap.hit = Some(id);
            break;
        }
        if !tap.under_disabled.contains(&id) {
            tap.under_disabled.push(id);
        }
    }
    tap
}

/// Convenience: all entities whose shapes cover the point, front to back.
pub fn entities_at(list: &DrawList, x: f32, y: f32) -> Vec<EntityId> {
    let (px, py) = (x.floor() as i32, y.floor() as i32);
    let mut out = Vec::new();
    for item in list.items.iter().rev() {
        if let Some(id) = item.entity {
            if item.shape.covers(px, py) && !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}
