//! Flat shapes in screen pixels, and the 2D layer drawn over the world.
//!
//! [`Shape`] is what sprites, labels and the HUD are made of once they are
//! on the screen: exact integer coverage at pixel centers. [`overlay`] builds
//! the layer of screen entities (`screen_pos`), which ignores the camera and
//! is always on top.

use rwgpuengine_core::color::Rgba;
use rwgpuengine_data::{SpriteDef, SpriteShape, font};
use rwgpuengine_sim::{Entity, EntityId, World};

use crate::font::{glyph_index, lit};
use crate::view::round;

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
    Block,
    Sprite,
    Label,
}

impl Part {
    pub fn name(self) -> &'static str {
        match self {
            Part::Ground => "ground",
            Part::Tile => "tile",
            Part::Block => "block",
            Part::Sprite => "sprite",
            Part::Label => "label",
        }
    }
}

/// One shape of the 2D layer.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawItem {
    pub entity: EntityId,
    pub part: Part,
    pub shape: Shape,
    pub color: Rgba,
    /// Disabled items are only present in lists built for picking; nothing
    /// draws them.
    pub enabled: bool,
}

/// The 2D layer: every screen entity's sprite and label, back to front (file
/// order, a sprite before its own label). A HUD is always on top of the world.
pub fn overlay(world: &World, include_disabled: bool) -> Vec<DrawItem> {
    let mut items = Vec::new();
    for e in &world.entities {
        let Some(at) = e.screen_pos else { continue };
        if !(e.enabled || include_disabled) {
            continue;
        }
        let mut push = |part: Part, shape: Shape, color: Rgba| {
            items.push(DrawItem { entity: e.id, part, shape, color, enabled: e.enabled });
        };
        if let Some(s) = &e.sprite {
            // `screen_pos` is the top-left corner.
            let (x0, y0) = (round(at[0] + s.offset[0]), round(at[1] + s.offset[1]));
            push(Part::Sprite, sprite_shape(s, x0, y0, 1.0), s.color);
        }
        if let (Some(l), Some(text)) = (&e.label, e.label_text()) {
            let scale = l.scale.max(1) as i32;
            let (x0, y0) = (round(at[0] + l.offset[0]), round(at[1] + l.offset[1]));
            for (gx, gy, index) in glyphs(&text, scale) {
                push(Part::Label, Shape::Glyph { x0: x0 + gx, y0: y0 + gy, scale, index }, l.color);
            }
        }
    }
    items
}

/// A sprite's shape with its top-left corner at (x0, y0), `scale` times its size.
pub(crate) fn sprite_shape(s: &SpriteDef, x0: i32, y0: i32, scale: f32) -> Shape {
    let (w, h) = (round(s.size[0] * scale).max(1), round(s.size[1] * scale).max(1));
    let r = Rect { x0, y0, x1: x0 + w, y1: y0 + h };
    match s.shape {
        SpriteShape::Circle => Shape::Ellipse(r),
        SpriteShape::Rect => Shape::Rect(r),
        SpriteShape::Diamond => {
            let (mx, my) = (x0 + w / 2, y0 + h / 2);
            Shape::quad([[mx, y0], [r.x1, my], [mx, r.y1], [x0, my]])
        }
    }
}

/// Where each character of `text` goes, relative to the text's top-left
/// corner: (x, y, glyph). Spaces draw nothing.
pub(crate) fn glyphs(text: &str, scale: i32) -> Vec<(i32, i32, u8)> {
    let mut out = Vec::new();
    for (row, line) in text.split('\n').enumerate() {
        for (col, c) in line.chars().enumerate() {
            if c != ' ' {
                out.push((col as i32 * font::ADVANCE * scale, row as i32 * font::LINE * scale, glyph_index(c)));
            }
        }
    }
    out
}

/// Does this entity stand in the world (as opposed to on the screen)?
pub(crate) fn in_world(e: &Entity) -> bool {
    e.screen_pos.is_none()
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
