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
    let mut tap = Tap { x, y, hit: None, under_disabled: Vec::new(), ground: None };
    for item in list.items.iter().rev() {
        if !item.shape.covers(px, py) {
            continue;
        }
        let Some(id) = item.entity else {
            tap.ground = Some(item.cell); // the ground stops the search
            break;
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
        if let Some(id) = item.entity
            && item.shape.covers(px, py)
            && !out.contains(&id)
        {
            out.push(id);
        }
    }
    out
}

/// A screen pixel where a tap hits `id` (resolved exactly as a real tap is),
/// as close as possible to the middle of its front-most visible part.
/// `None` if the entity is disabled, off-screen or completely covered.
/// The list must be built with `include_disabled = true`.
pub fn tap_point(list: &DrawList, id: EntityId) -> Option<[i32; 2]> {
    let (w, h) = (list.view.width as i32, list.view.height as i32);
    for item in list.items.iter().rev().filter(|i| i.entity == Some(id) && i.enabled) {
        let r = item.shape.bbox();
        let (cx, cy) = ((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2);
        let mut candidates: Vec<(i64, i32, i32)> = Vec::new();
        for py in r.y0.max(0)..r.y1.min(h) {
            for px in r.x0.max(0)..r.x1.min(w) {
                if item.shape.covers(px, py) {
                    let d = (px - cx) as i64 * (px - cx) as i64 + (py - cy) as i64 * (py - cy) as i64;
                    candidates.push((d, py, px));
                }
            }
        }
        candidates.sort_unstable();
        for (_, py, px) in candidates {
            if pick(list, px as f32 + 0.5, py as f32 + 0.5).hit == Some(id) {
                return Some([px, py]);
            }
        }
    }
    None
}

/// A screen pixel where a tap lands on ground cell `cell` with no enabled
/// entity in front of it (so it becomes a ground tap), as close as possible
/// to the middle of the cell's diamond. `None` if the cell is off-screen or
/// completely covered. The list must be built with `include_disabled = true`.
pub fn ground_tap_point(list: &DrawList, cell: [i32; 2]) -> Option<[i32; 2]> {
    let (w, h) = (list.view.width as i32, list.view.height as i32);
    let item = list.items.iter().find(|i| i.entity.is_none() && i.cell == cell)?;
    let r = item.shape.bbox();
    let (cx, cy) = ((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2);
    let mut candidates: Vec<(i64, i32, i32)> = Vec::new();
    for py in r.y0.max(0)..r.y1.min(h) {
        for px in r.x0.max(0)..r.x1.min(w) {
            if item.shape.covers(px, py) {
                let d = (px - cx) as i64 * (px - cx) as i64 + (py - cy) as i64 * (py - cy) as i64;
                candidates.push((d, py, px));
            }
        }
    }
    candidates.sort_unstable();
    candidates.into_iter().find_map(|(_, py, px)| {
        let t = pick(list, px as f32 + 0.5, py as f32 + 0.5);
        (t.hit.is_none() && t.ground == Some(cell)).then_some([px, py])
    })
}
