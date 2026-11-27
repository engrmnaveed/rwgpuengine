//! The render list: what is drawn, where, seen from which camera.
//!
//! The render list is the renderer's central artifact. The simulation's
//! world goes in; a camera, a light, solid instances, cards and a 2D layer
//! come out. Nothing in it depends on a GPU, and every consumer reads the
//! same list: the CPU reference renderer for `shot`, `test`, the frame report
//! and picking, and a GPU renderer for players. So "what you see", "what the
//! report says" and "what a tap hits" cannot disagree.
//!
//! - An [`Instance`] is a solid thing in the world: a box, a flat rectangle
//!   lying on a surface, or a [`Model`]. A `block`, a `tile` and the ground
//!   cells are boxes and rectangles; a `[[shape]]` list is a model, and every
//!   entity with that list is an instance of it.
//! - A [`Card`] is a flat picture that faces the camera and stands at a world
//!   point: a world entity's `sprite` or `label`.
//! - The overlay is the 2D layer of screen entities, drawn last.

use std::sync::Arc;

use rwgpuengine_core::color::Rgba;
use rwgpuengine_data::SpriteDef;
use rwgpuengine_sim::{EntityId, World};

use crate::camera::Camera;
use crate::draw::{self, DrawItem, Part};
use crate::model::Model;
use crate::view::View;

/// The shapes an [`Instance`] can have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mesh {
    /// A box from `pos` to `pos + size`.
    Box,
    /// A rectangle at height `pos[2]`, facing up; `size[2]` is 0.
    Flat,
    /// The model [`RenderList::models`]`[n]`, with the corner of its
    /// footprint at `pos`. `size` is the box around it; it has no colors of
    /// its own (they are its shapes').
    Model(u32),
}

/// One light for the whole scene. A face whose outward unit normal is `n` is
/// drawn at `base + n · toward` of its color, kept within 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub base: f32,
    pub toward: [f32; 3],
}

impl Light {
    /// The light of the 2.5D engine, extended to every direction: faces that
    /// look up keep their color, +y faces get 0.8 of it, +x faces 0.65, and
    /// the two sides that view never showed get 0.5 (−x) and 0.35 (−y).
    pub const DEFAULT: Light = Light { base: 0.575, toward: [0.075, 0.225, 0.425] };

    /// How bright a face with this outward unit normal is, 0..1.
    pub fn shade(&self, n: [f32; 3]) -> f32 {
        (self.base + n[0] * self.toward[0] + n[1] * self.toward[1] + n[2] * self.toward[2]).clamp(0.0, 1.0)
    }
}

/// A solid thing in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instance {
    /// `None` for ground cells.
    pub entity: Option<EntityId>,
    pub part: Part,
    /// Grid cell, for ground cells and reports.
    pub cell: [i32; 2],
    /// Disabled instances are only present in lists built for picking;
    /// nothing draws them.
    pub enabled: bool,
    pub mesh: Mesh,
    /// The corner with the smallest x, y and z.
    pub pos: [f32; 3],
    pub size: [f32; 3],
    /// Color of the sides.
    pub color: Rgba,
    /// Color of faces that look up or down (a [`Mesh::Flat`] is only this).
    pub top: Rgba,
    /// Flat things lie *on* a level surface, at exactly its height. Where
    /// two level faces are in one plane, the one with the higher layer
    /// shows: the ground and the tops of boxes have 0, a tile 1, a block of
    /// height 0 has 2. Between equal layers the earlier one in the list shows.
    pub layer: u8,
    /// Position in the drawing order: 0 for the ground, then one per entity.
    pub seq: u32,
}

/// What a [`Card`] shows.
#[derive(Clone, Debug, PartialEq)]
pub enum CardBody {
    /// The sprite's size is in pixels of the scene's starting view; it grows
    /// and shrinks with the camera like the world around it.
    Sprite(SpriteDef),
    /// Text in the built-in font: (x, y, glyph) from the text's top-left
    /// corner. Text keeps its pixel size whatever the camera does.
    Text { glyphs: Vec<(i32, i32, u8)>, scale: i32 },
}

/// A flat picture that faces the camera, standing at a world point.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub entity: EntityId,
    pub part: Part,
    pub cell: [i32; 2],
    pub enabled: bool,
    /// The world point the card stands at: above it the card is upright,
    /// below it the card lies on the level surface toward the viewer.
    pub anchor: [f32; 3],
    /// Top-left corner of the picture, in pixels from where `anchor` is
    /// drawn. For a sprite the offset scales with the camera.
    pub origin: [f32; 2],
    pub color: Rgba,
    pub body: CardBody,
    /// The card is drawn this many small steps (1/1024 of a world unit)
    /// nearer the camera than it stands: a sprite has 0 and a label 1, so a
    /// label is in front of its own sprite.
    pub lift: u8,
    pub seq: u32,
}

/// Everything one frame draws. Two equal lists give the same picture, so a
/// player can skip a frame whose list did not change.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderList {
    pub view: View,
    pub camera: Camera,
    pub light: Light,
    /// The ground, then every world entity's tile and block, in file order.
    pub instances: Vec<Instance>,
    /// Every world entity's sprite and label, in file order.
    pub cards: Vec<Card>,
    /// The 2D layer of screen entities, back to front.
    pub overlay: Vec<DrawItem>,
    /// The models the instances are of: each shape list in each footprint
    /// and turn it is drawn in, once, in the order first drawn.
    pub models: Vec<Model>,
}

pub const LAYER_TILE: u8 = 1;
pub const LAYER_FLAT_BLOCK: u8 = 2;
pub const LIFT_SPRITE: u8 = 0;
pub const LIFT_LABEL: u8 = 1;

/// Build the list for the world as it is now. With `include_disabled`,
/// disabled entities are in the list too, marked: picking uses that to say
/// "the button under your finger is disabled".
pub fn build(world: &World, view: &View, camera: &Camera, include_disabled: bool) -> RenderList {
    let mut instances = Vec::new();
    let mut cards = Vec::new();
    if let Some(g) = &world.ground {
        for y in 0..g.size[1] {
            for x in 0..g.size[0] {
                let Some(t) = g.terrain_at([x, y]) else { continue };
                let color = if (x + y) % 2 == 0 { t.color } else { t.alt_color };
                instances.push(Instance {
                    entity: None,
                    part: Part::Ground,
                    cell: [x, y],
                    enabled: true,
                    mesh: Mesh::Flat,
                    pos: [x as f32 - 0.5, y as f32 - 0.5, 0.0],
                    size: [1.0, 1.0, 0.0],
                    color,
                    top: color,
                    layer: 0,
                    seq: 0,
                });
            }
        }
    }
    let mut models: Vec<Model> = Vec::new();
    // The model of each shape list at each turn, once it is in the list.
    let mut model_of = vec![[u32::MAX; 4]; world.shapes.len()];
    let mut seq = 0;
    for e in &world.entities {
        if !draw::in_world(e) || !(e.enabled || include_disabled) {
            continue;
        }
        seq += 1;
        let [x, y, z] = e.pos;
        let cell = e.cell();
        let [fw, fh] = e.footprint;
        // The footprint runs from (x − 0.5, y − 0.5) to (x + w − 0.5, y + h − 0.5).
        let pos = [x - 0.5, y - 0.5, z];
        let mut solid = |part: Part, mesh: Mesh, height: f32, color: Rgba, top: Rgba, layer: u8| {
            instances.push(Instance {
                entity: Some(e.id),
                part,
                cell,
                enabled: e.enabled,
                mesh,
                pos,
                size: [fw as f32, fh as f32, height],
                color,
                top,
                layer,
                seq,
            });
        };
        if let Some(t) = &e.tile {
            solid(Part::Tile, Mesh::Flat, 0.0, t.color, t.color, LAYER_TILE);
        }
        let mut top = z;
        if let Some(b) = &e.block {
            if b.height > 0.0 {
                solid(Part::Block, Mesh::Box, b.height, b.color, b.top, 0);
                top += b.height;
            } else {
                solid(Part::Block, Mesh::Flat, 0.0, b.top, b.top, LAYER_FLAT_BLOCK);
            }
        }
        if let Some(shapes) = e.shapes.and_then(|n| world.shapes.get(n as usize)) {
            let turn = e.turn % 4;
            let slot = &mut model_of[e.shapes.unwrap_or(0) as usize][turn as usize];
            // The same list in another footprint is another model (rare: one more is looked for).
            let fits = |m: &Model| Arc::ptr_eq(&m.shapes, shapes) && m.footprint == e.footprint && m.turn == turn;
            if !models.get(*slot as usize).is_some_and(fits) {
                *slot = models.iter().position(fits).unwrap_or_else(|| {
                    models.push(Model { shapes: shapes.clone(), footprint: e.footprint, turn });
                    models.len() - 1
                }) as u32;
            }
            let height = models[*slot as usize].height();
            solid(Part::Shape(shapes[0].kind), Mesh::Model(*slot), height, Rgba::WHITE, Rgba::WHITE, 0);
            top = top.max(z + height);
        }
        // A sprite and a label stand at the middle of the entity's footprint,
        // on top of its block and its shapes (inside them they could not be seen).
        let anchor = [x + (fw - 1) as f32 / 2.0, y + (fh - 1) as f32 / 2.0, top];
        if let Some(s) = &e.sprite {
            // The sprite stands on the anchor: bottom edge there, centered.
            cards.push(Card {
                entity: e.id,
                part: Part::Sprite,
                cell,
                enabled: e.enabled,
                anchor,
                origin: [-s.size[0] / 2.0 + s.offset[0], -s.size[1] + s.offset[1]],
                color: s.color,
                body: CardBody::Sprite(*s),
                lift: LIFT_SPRITE,
                seq,
            });
        }
        if let (Some(l), Some(text)) = (&e.label, e.label_text()) {
            let scale = l.scale.max(1) as i32;
            let [w, h] = rwgpuengine_data::font::text_size(&text).map(|v| v * scale);
            cards.push(Card {
                entity: e.id,
                part: Part::Label,
                cell,
                enabled: e.enabled,
                anchor,
                origin: [-(w as f32) / 2.0 + l.offset[0], -(h as f32) + l.offset[1]],
                color: l.color,
                body: CardBody::Text { glyphs: draw::glyphs(&text, scale), scale },
                lift: LIFT_LABEL,
                seq,
            });
        }
    }
    RenderList {
        view: *view,
        camera: *camera,
        light: Light::DEFAULT,
        instances,
        cards,
        overlay: draw::overlay(world, include_disabled),
        models,
    }
}
