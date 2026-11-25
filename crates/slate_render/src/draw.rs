//! Draw lists: what is drawn, where, in which order.
//!
//! The draw list is the renderer's central artifact (ADR 0007). The CPU
//! rasterizer consumes it, the frame report describes it, and picking walks
//! it backwards, so "what you see", "what the report says" and "what a tap
//! hits" can never disagree. A future GPU backend consumes the same list.

use slate_core::color::Rgba;
use slate_data::{GroundDef, SpriteShape, font};
use slate_sim::{Entity, EntityId, World};

use crate::font::{glyph_index, lit};
use crate::view::{View, round};

/// Pixel rectangle, `x1`/`y1` exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub const EMPTY: Rect = Rect { x0: 0, y0: 0, x1: 0, y1: 0 };

    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }

    pub fn intersect(&self, o: &Rect) -> Rect {
        let r = Rect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) };
        if r.is_empty() { Rect::EMPTY } else { r }
    }

    pub fn area(&self) -> i64 {
        if self.is_empty() { 0 } else { (self.x1 - self.x0) as i64 * (self.y1 - self.y0) as i64 }
    }

    /// `[x, y, w, h]`, the form used in JSON output.
    pub fn xywh(&self) -> [i32; 4] {
        [self.x0, self.y0, self.x1 - self.x0, self.y1 - self.y0]
    }
}

/// A filled shape. All coverage tests are exact integer math at pixel
/// centers, with a top-left fill rule, so shared edges are drawn once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Convex quad, vertices stored clockwise (screen space, y down).
    Quad([[i32; 2]; 4]),
    /// Ellipse inscribed in the rectangle.
    Ellipse(Rect),
    Rect(Rect),
    /// One character of the built-in font, top-left at (x0, y0), each font
    /// pixel `scale` × `scale` screen pixels.
    Glyph {
        x0: i32,
        y0: i32,
        scale: i32,
        index: u8,
    },
}

impl Shape {
    /// A convex quad from vertices in either winding order.
    pub fn quad(mut pts: [[i32; 2]; 4]) -> Shape {
        let mut area: i64 = 0;
        for i in 0..4 {
            let [ax, ay] = pts[i];
            let [bx, by] = pts[(i + 1) % 4];
            area += ax as i64 * by as i64 - bx as i64 * ay as i64;
        }
        if area < 0 {
            pts.reverse();
        }
        Shape::Quad(pts)
    }

    pub fn bbox(&self) -> Rect {
        match self {
            Shape::Quad(p) => Rect {
                x0: p.iter().map(|v| v[0]).min().unwrap_or(0),
                y0: p.iter().map(|v| v[1]).min().unwrap_or(0),
                x1: p.iter().map(|v| v[0]).max().unwrap_or(0),
                y1: p.iter().map(|v| v[1]).max().unwrap_or(0),
            },
            Shape::Ellipse(r) | Shape::Rect(r) => *r,
            Shape::Glyph { x0, y0, scale, .. } => {
                Rect { x0: *x0, y0: *y0, x1: x0 + font::GLYPH_W * scale, y1: y0 + font::GLYPH_H * scale }
            }
        }
    }

    /// Does the shape cover the center of pixel (px, py)?
    pub fn covers(&self, px: i32, py: i32) -> bool {
        // Doubled coordinates: pixel centers are odd, vertices even, so the
        // test is exact integer arithmetic.
        let (cx, cy) = (2 * px as i64 + 1, 2 * py as i64 + 1);
        match self {
            Shape::Quad(p) => {
                for i in 0..4 {
                    let (ax, ay) = (2 * p[i][0] as i64, 2 * p[i][1] as i64);
                    let (bx, by) = (2 * p[(i + 1) % 4][0] as i64, 2 * p[(i + 1) % 4][1] as i64);
                    let (dx, dy) = (bx - ax, by - ay);
                    let cross = dx * (cy - ay) - dy * (cx - ax);
                    let top_left = (dy == 0 && dx > 0) || dy < 0;
                    if cross < 0 || (cross == 0 && !top_left) {
                        return false;
                    }
                }
                true
            }
            Shape::Ellipse(r) => {
                let (w, h) = ((r.x1 - r.x0) as i64, (r.y1 - r.y0) as i64);
                if w <= 0 || h <= 0 {
                    return false;
                }
                let dx = cx - (2 * r.x0 as i64 + w);
                let dy = cy - (2 * r.y0 as i64 + h);
                dx * dx * h * h + dy * dy * w * w <= w * w * h * h
            }
            Shape::Rect(r) => px >= r.x0 && px < r.x1 && py >= r.y0 && py < r.y1,
            Shape::Glyph { x0, y0, scale, index } => {
                px >= *x0 && py >= *y0 && lit(*index, (px - x0) / scale, (py - y0) / scale)
            }
        }
    }
}

/// Which part of an entity an item draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Part {
    Ground,
    Tile,
    BlockLeft,
    BlockRight,
    BlockTop,
    Sprite,
    Label,
}

impl Part {
    pub fn name(self) -> &'static str {
        match self {
            Part::Ground => "ground",
            Part::Tile => "tile",
            Part::BlockLeft | Part::BlockRight | Part::BlockTop => "block",
            Part::Sprite => "sprite",
            Part::Label => "label",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawItem {
    /// `None` for ground cells.
    pub entity: Option<EntityId>,
    pub part: Part,
    pub shape: Shape,
    pub color: Rgba,
    /// Disabled items are only present in lists built for picking; the
    /// rasterizer skips them.
    pub enabled: bool,
    /// Grid cell, for ground items and reports.
    pub cell: [i32; 2],
    key: (u8, i32, i32, Part, u32),
}

/// Two equal lists paint the same picture, so a player can skip repainting.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawList {
    pub view: View,
    /// Back to front.
    pub items: Vec<DrawItem>,
}

/// Painter's order: floor first, then everything standing, sorted by depth
/// (x + y), then height, then part (block faces, sprite, label), then id.
/// Screen entities (`screen_pos`) come last, in file order: a HUD is always
/// on top of the world.
pub fn build(world: &World, view: &View, ground: Option<&GroundDef>, include_disabled: bool) -> DrawList {
    let mut items = Vec::new();
    if let Some(g) = ground {
        for y in 0..g.size[1] {
            for x in 0..g.size[0] {
                let Some(t) = g.terrain_at([x, y]) else { continue };
                let color = if (x + y) % 2 == 0 { t.color } else { t.alt_color };
                let shape = Shape::quad(view.diamond(x as f32, y as f32, 0.0));
                items.push(DrawItem {
                    entity: None,
                    part: Part::Ground,
                    shape,
                    color,
                    enabled: true,
                    cell: [x, y],
                    key: (0, x + y, 0, Part::Ground, 0),
                });
            }
        }
    }
    for e in &world.entities {
        if e.enabled || include_disabled {
            push_entity(&mut items, e, view);
        }
    }
    items.sort_by_key(|i| i.key);
    DrawList { view: *view, items }
}

fn push_entity(items: &mut Vec<DrawItem>, e: &Entity, view: &View) {
    let [x, y, z] = e.pos;
    let cell = e.cell();
    let screen = e.screen_pos;
    let [fw, fh] = e.footprint;
    // The front cell (largest x + y): a multi-cell entity's sprite and label
    // sort with it, after every slice of its block.
    let front = [cell[0] + fw - 1, cell[1] + fh - 1];
    // Screen entities: one layer above the world, in file order (the id
    // takes the depth slot so it sorts before the part).
    let (layer, depth, zkey) = match screen {
        Some(_) => (3, e.id as i32, 0),
        None => (2, front[0] + front[1], round(z * 1000.0)),
    };
    let mut push_at = |part: Part, layer: u8, depth: i32, shape: Shape, color: Rgba| {
        items.push(DrawItem {
            entity: Some(e.id),
            part,
            shape,
            color,
            enabled: e.enabled,
            cell,
            key: (layer, depth, zkey, part, e.id),
        });
    };
    if screen.is_none() {
        // The footprint's corners: from (x - 0.5, y - 0.5) to (x + w - 0.5, y + h - 0.5).
        let (x1, y1) = (x + (fw - 1) as f32, y + (fh - 1) as f32);
        if let Some(t) = &e.tile {
            let d = [
                view.project_px(x - 0.5, y - 0.5, z),
                view.project_px(x1 + 0.5, y - 0.5, z),
                view.project_px(x1 + 0.5, y1 + 0.5, z),
                view.project_px(x - 0.5, y1 + 0.5, z),
            ];
            push_at(Part::Tile, 1, front[0] + front[1], Shape::quad(d), t.color);
        }
        if let Some(b) = &e.block {
            // One slice per covered cell, each sorted by its own cell, so a big
            // box interleaves correctly with smaller things around it; slices
            // drawn back to front hide each other's inner faces exactly.
            for dy in 0..fh {
                for dx in 0..fw {
                    let (cx, cy) = (x + dx as f32, y + dy as f32);
                    let depth = cell[0] + dx + cell[1] + dy;
                    let base = view.diamond(cx, cy, z);
                    let top = view.diamond(cx, cy, z + b.height);
                    let [_, base_r, base_b, base_l] = base;
                    let [_, top_r, top_b, top_l] = top;
                    if b.height > 0.0 {
                        push_at(
                            Part::BlockLeft,
                            2,
                            depth,
                            Shape::quad([top_l, top_b, base_b, base_l]),
                            b.color.scale(4, 5),
                        );
                        push_at(
                            Part::BlockRight,
                            2,
                            depth,
                            Shape::quad([top_b, top_r, base_r, base_b]),
                            b.color.scale(13, 20),
                        );
                    }
                    push_at(Part::BlockTop, 2, depth, Shape::quad(top), b.top);
                }
            }
        }
    }
    let mut push = |part: Part, layer: u8, shape: Shape, color: Rgba| push_at(part, layer, depth, shape, color);
    // A multi-cell entity's sprite and label stand at the middle of its footprint.
    let (x, y) = (x + (fw - 1) as f32 / 2.0, y + (fh - 1) as f32 / 2.0);
    // The standing point (world) or the top-left corner (screen).
    let anchor = match screen {
        Some(p) => p,
        None => view.project(x, y, z),
    };
    if let Some(s) = &e.sprite {
        let (w, h) = (round(s.size[0]).max(1), round(s.size[1]).max(1));
        let (x0, y0) = match screen {
            Some(_) => (round(anchor[0] + s.offset[0]), round(anchor[1] + s.offset[1])),
            None => (round(anchor[0] - s.size[0] / 2.0 + s.offset[0]), round(anchor[1] - s.size[1] + s.offset[1])),
        };
        let r = Rect { x0, y0, x1: x0 + w, y1: y0 + h };
        let shape = match s.shape {
            SpriteShape::Circle => Shape::Ellipse(r),
            SpriteShape::Rect => Shape::Rect(r),
            SpriteShape::Diamond => {
                let (mx, my) = (x0 + w / 2, y0 + h / 2);
                Shape::quad([[mx, y0], [r.x1, my], [mx, r.y1], [x0, my]])
            }
        };
        push(Part::Sprite, layer, shape, s.color);
    }
    if let (Some(l), Some(text)) = (&e.label, e.label_text()) {
        let scale = l.scale.max(1) as i32;
        let [w, h] = font::text_size(&text).map(|v| v * scale);
        let (x0, y0) = match screen {
            Some(_) => (round(anchor[0] + l.offset[0]), round(anchor[1] + l.offset[1])),
            None => (round(anchor[0] - w as f32 / 2.0 + l.offset[0]), round(anchor[1] - h as f32 + l.offset[1])),
        };
        for (row, line) in text.split('\n').enumerate() {
            for (col, c) in line.chars().enumerate() {
                if c == ' ' {
                    continue;
                }
                let shape = Shape::Glyph {
                    x0: x0 + col as i32 * font::ADVANCE * scale,
                    y0: y0 + row as i32 * font::LINE * scale,
                    scale,
                    index: glyph_index(c),
                };
                push(Part::Label, layer, shape, l.color);
            }
        }
    }
}

impl DrawList {
    /// Screen bounds of everything drawn for `id`.
    pub fn bounds_of(&self, id: EntityId) -> Rect {
        self.items.iter().filter(|i| i.entity == Some(id)).fold(Rect::EMPTY, |acc, i| acc.union(&i.shape.bbox()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_quads_share_edges_without_gaps_or_overlap() {
        // Two neighboring iso diamonds: every pixel in their union is covered
        // exactly once.
        let a = Shape::quad([[32, 0], [64, 16], [32, 32], [0, 16]]);
        let b = Shape::quad([[64, 16], [96, 32], [64, 48], [32, 32]]);
        for py in 0..48 {
            for px in 0..96 {
                assert!(!(a.covers(px, py) && b.covers(px, py)), "overlap at {px},{py}");
            }
        }
        // A 2x1 axis-aligned split has no gaps either.
        let l = Shape::quad([[0, 0], [5, 0], [5, 4], [0, 4]]);
        let r = Shape::quad([[5, 0], [10, 0], [10, 4], [5, 4]]);
        for py in 0..4 {
            for px in 0..10 {
                assert!(l.covers(px, py) ^ r.covers(px, py), "gap/overlap at {px},{py}");
            }
        }
    }

    #[test]
    fn winding_does_not_matter() {
        let cw = Shape::quad([[0, 0], [8, 0], [8, 8], [0, 8]]);
        let ccw = Shape::quad([[0, 8], [8, 8], [8, 0], [0, 0]]);
        for py in -1..9 {
            for px in -1..9 {
                assert_eq!(cw.covers(px, py), ccw.covers(px, py));
            }
        }
        assert!(cw.covers(0, 0) && !cw.covers(8, 8));
    }

    #[test]
    fn ellipse_is_symmetric() {
        let e = Shape::Ellipse(Rect { x0: 0, y0: 0, x1: 10, y1: 6 });
        for py in 0..6 {
            for px in 0..10 {
                assert_eq!(e.covers(px, py), e.covers(9 - px, py));
                assert_eq!(e.covers(px, py), e.covers(px, 5 - py));
            }
        }
        assert!(e.covers(5, 3) && !e.covers(0, 0));
    }
}
