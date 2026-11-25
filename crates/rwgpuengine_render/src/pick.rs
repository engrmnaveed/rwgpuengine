//! Picking: which entity is drawn at a screen point.
//!
//! A tap is resolved against the same projection the reference renderer
//! draws, with the same coverage and depth rules, so it hits exactly what is
//! visible there. Disabled entities in front of the hit are collected so a
//! missed tap can say "the button under your finger is disabled".

use rwgpuengine_sim::{EntityId, Tap};

use crate::draw::Rect;
use crate::reference::{Frame, Projected, nearer};

/// Resolve a tap at screen pixel (x, y). The projection must come from a
/// list built with `include_disabled = true` for misses to name disabled
/// entities.
pub fn pick(p: &Projected, x: f32, y: f32) -> Tap {
    let (px, py) = (x.floor() as i32, y.floor() as i32);
    let mut tap = Tap { x, y, hit: None, under_disabled: Vec::new(), ground: None };
    if px < 0 || py < 0 || px >= p.width as i32 || py >= p.height as i32 {
        return tap;
    }
    let here = Rect { x0: px, y0: py, x1: px + 1, y1: py + 1 };
    // The depth and rank of item `idx` at this pixel, if it covers it.
    let covers = |idx: usize| {
        let mut found = None;
        p.cover(idx, here, |_, _, d, rank| found = Some((d, rank)));
        found
    };
    let note_disabled = |tap: &mut Tap, id: Option<EntityId>| {
        if let Some(id) = id
            && !tap.under_disabled.contains(&id)
        {
            tap.under_disabled.push(id);
        }
    };

    // The 2D layer is in front of everything, last drawn on top.
    for idx in (p.world_len()..p.items.len()).rev() {
        if covers(idx).is_none() {
            continue;
        }
        let item = &p.items[idx];
        if item.enabled {
            tap.hit = item.entity;
            return tap;
        }
        note_disabled(&mut tap, item.entity);
    }

    // The world: replay what the renderer does at this one pixel.
    let mut shown: Option<(usize, f32)> = None;
    let mut held = (f32::INFINITY, 0);
    for idx in p.world_order() {
        if let Some((d, rank)) = covers(idx)
            && nearer(d, rank, held.0, held.1)
        {
            if !p.translucent(idx) {
                held = (d, rank);
            }
            shown = Some((idx, d));
        }
    }
    // Disabled things nearer than what is shown, nearest first.
    let limit = shown.map_or(f32::INFINITY, |(_, d)| d);
    let mut hidden: Vec<(f32, usize)> = (0..p.world_len())
        .filter(|i| !p.items[*i].enabled)
        .filter_map(|i| covers(i).filter(|(d, _)| *d < limit).map(|(d, _)| (d, i)))
        .collect();
    hidden.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    for (_, idx) in hidden {
        note_disabled(&mut tap, p.items[idx].entity);
    }
    if let Some((idx, _)) = shown {
        match p.items[idx].entity {
            Some(id) => tap.hit = Some(id),
            None => tap.ground = Some(p.items[idx].cell),
        }
    }
    tap
}

/// A screen pixel that shows `id`, so a tap there hits it: as close as
/// possible to the middle of its front-most part that can be seen (a label
/// before a sprite before a block before a tile). `None` if the entity is
/// disabled, off-screen or completely covered. `frame` must be the rendering
/// of `p`.
pub fn tap_point(p: &Projected, frame: &Frame, id: EntityId) -> Option<[i32; 2]> {
    (0..p.items.len())
        .rev()
        .filter(|i| p.items[*i].entity == Some(id) && p.items[*i].enabled)
        .find_map(|i| owned_point(p, frame, i))
}

/// A screen pixel that shows ground cell `cell`, so a tap there is a ground
/// tap on it: as close as possible to the middle of the cell. `None` if the
/// cell is off-screen or completely covered. `frame` must be the rendering
/// of `p`.
pub fn ground_tap_point(p: &Projected, frame: &Frame, cell: [i32; 2]) -> Option<[i32; 2]> {
    let idx = p.items[..p.world_len()].iter().position(|i| i.entity.is_none() && i.cell == cell)?;
    owned_point(p, frame, idx)
}

/// The pixel owned by item `idx` nearest the middle of its bounds (ties: the
/// upper, then the left one).
fn owned_point(p: &Projected, frame: &Frame, idx: usize) -> Option<[i32; 2]> {
    let r = p.items[idx].bounds;
    let (cx, cy) = ((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2);
    let area = r.intersect(&Rect { x0: 0, y0: 0, x1: p.width as i32, y1: p.height as i32 });
    let tag = idx as u32 + 1;
    let mut best: Option<(i64, i32, i32)> = None;
    for py in area.y0..area.y1 {
        let row = py as usize * p.width as usize;
        for px in area.x0..area.x1 {
            if frame.owner[row + px as usize] == tag {
                let d = (px - cx) as i64 * (px - cx) as i64 + (py - cy) as i64 * (py - cy) as i64;
                if best.is_none_or(|b| (d, py, px) < b) {
                    best = Some((d, py, px));
                }
            }
        }
    }
    best.map(|(_, py, px)| [px, py])
}
