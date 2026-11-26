//! From a render list to the bytes the GPU reads. No GPU is needed here.
//!
//! Everything that decides *where* something is drawn to the pixel, or in
//! which order, comes from the reference renderer's own code: the
//! [`Projector`], a card's place and shapes, the far-to-near order of
//! see-through things, a face's color under the light. The GPU projects the
//! corners of boxes and fills pixels.

use std::ops::Range;
use std::sync::OnceLock;

use rwgpuengine_core::color::Rgba;
use rwgpuengine_render::camera::to_fixed;
use rwgpuengine_render::font::lit;
use rwgpuengine_render::reference::{BOX_FACES, card_shapes, middle_depth, shade};
use rwgpuengine_render::{Instance, Mesh, Projector, Rect, RenderList, Shape};

use crate::{FaceColor, Options, Origin};

/// Bytes of one instance: position, size, two colors, the rank.
pub(crate) const INSTANCE_BYTES: usize = 36;
/// Bytes of one flat shape.
pub(crate) const SHAPE_BYTES: usize = 88;
pub(crate) const GLOBALS_BYTES: usize = 1744;

/// The iso view's depth scale (`held` in the shader): 60 halvings from the
/// nearest depth to the middle of the range and 60 more to the farthest.
const ISO_DEPTH_SCALE: f32 = 60.0;

/// Bytes of one instance's corner columns (the iso view).
pub(crate) const COLUMNS_BYTES: usize = 16;

/// Shader flags (`shader.wgsl`).
const SHADE_IN_SHADER: u32 = 1;
const CARD_ONE_DEPTH: u32 = 4;
const ISO_COLUMNS: u32 = 8;

/// The highest rank a level face can have: what lies in one plane is told
/// apart by a few steps of depth, and a card that lies there is above them.
const MAX_RANK: u8 = 6;

fn f32s(out: &mut Vec<u8>, v: &[f32]) {
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
}

fn i32s(out: &mut Vec<u8>, v: &[i32]) {
    for i in v {
        out.extend_from_slice(&i.to_le_bytes());
    }
}

fn rgba(out: &mut Vec<u8>, c: Rgba) {
    out.extend_from_slice(&[c.r, c.g, c.b, c.a]);
}

/// The corners of the unit box, face by face, two triangles each: x, y, z
/// and the face. `faces` picks from the reference's own table.
pub(crate) fn mesh(faces: Range<usize>) -> Vec<u8> {
    let mut out = Vec::new();
    for (face, (_, quad)) in BOX_FACES.iter().enumerate().take(faces.end).skip(faces.start) {
        for k in [0, 1, 2, 0, 2, 3] {
            let i = quad[k] as u8;
            out.extend_from_slice(&[i & 1, (i >> 1) & 1, (i >> 2) & 1, face as u8]);
        }
    }
    out
}

/// The solid instances of a list, ready to upload. They do not depend on
/// the camera.
#[derive(Debug, Default)]
pub(crate) struct Instances {
    pub bytes: Vec<u8>,
    /// Opaque flat rectangles, then opaque boxes, each in list order.
    pub flats: Range<u32>,
    pub boxes: Range<u32>,
    /// See-through instances: (index in the list, index in `bytes`).
    pub glass: Vec<(usize, u32)>,
    /// The index in the list of each instance in `bytes`, in that order.
    pub order: Vec<u32>,
}

pub(crate) fn see_through(i: &Instance) -> bool {
    i.color.a < 255 || i.top.a < 255
}

pub(crate) fn instances(list: &[Instance]) -> Instances {
    let mut out = Instances::default();
    let push = |bytes: &mut Vec<u8>, i: &Instance| {
        f32s(bytes, &i.pos);
        f32s(bytes, &i.size);
        rgba(bytes, i.color);
        rgba(bytes, i.top);
        bytes.extend_from_slice(&[i.layer.saturating_add(1).min(MAX_RANK), 0, 0, 0]);
    };
    let drawn = |(_, i): &(usize, &Instance)| i.enabled;
    let mut n = 0;
    for (k, i) in list.iter().enumerate().filter(drawn).filter(|(_, i)| !see_through(i) && i.mesh == Mesh::Flat) {
        push(&mut out.bytes, i);
        out.order.push(k as u32);
        n += 1;
    }
    out.flats = 0..n;
    for (k, i) in list.iter().enumerate().filter(drawn).filter(|(_, i)| !see_through(i) && i.mesh == Mesh::Box) {
        push(&mut out.bytes, i);
        out.order.push(k as u32);
        n += 1;
    }
    out.boxes = out.flats.end..n;
    for (k, i) in list.iter().enumerate().filter(|(_, i)| i.enabled && see_through(i)) {
        push(&mut out.bytes, i);
        out.order.push(k as u32);
        out.glass.push((k, n));
        n += 1;
    }
    out
}

/// The iso view: the pixel column of each instance's four corners (x, y), in
/// the order of the instance buffer, as whole sub-pixel steps.
///
/// A box's upright edges are exactly upright in this view: one column from
/// top to bottom. The reference snaps that column to 1/256 of a pixel from a
/// double-precision sum. The GPU's single-precision sum can land one step
/// beside it, and where one of the two steps is a pixel center, a whole edge
/// is then drawn one pixel off. So these four numbers come from the
/// reference's own arithmetic.
pub(crate) fn columns(list: &[Instance], solid: &Instances, proj: &Projector) -> Vec<u8> {
    let row = proj.rows()[0];
    let mut out = Vec::with_capacity(solid.order.len() * COLUMNS_BYTES);
    for k in &solid.order {
        let inst = &list[*k as usize];
        let (p, s) = (inst.pos.map(f64::from), inst.size.map(f64::from));
        for corner in 0..4 {
            let x = p[0] + if corner & 1 != 0 { s[0] } else { 0.0 };
            let y = p[1] + if corner & 2 != 0 { s[1] } else { 0.0 };
            // The projector's own sum for a point's x (`Projector::hom`); in this view w is 1.
            let px = row[0] * x + row[1] * y + row[2] * p[2] + row[3];
            let fixed = to_fixed(px).clamp(-(1 << 23), 1 << 23) as i32;
            out.extend_from_slice(&fixed.to_le_bytes());
        }
    }
    out
}

/// The camera's numbers for one frame.
pub(crate) struct View {
    pub proj: Projector,
    /// The farthest depth that is drawn; the nearest is −1.
    far: f64,
}

impl View {
    pub fn new(list: &RenderList) -> View {
        let proj = Projector::new(&list.view, &list.camera);
        View { far: proj.far_depth(), proj }
    }

    /// Does every line of sight run the same way (the iso view)?
    pub fn iso(&self) -> bool {
        let w = self.proj.rows()[3];
        w[0] == 0.0 && w[1] == 0.0 && w[2] == 0.0
    }
}

/// The uniform block (`Globals` in the shader).
pub(crate) fn globals(list: &RenderList, view: &View, options: &Options) -> Vec<u8> {
    let m = view.proj.rows();
    let (w, h) = (list.view.width as f64, list.view.height as f64);
    let f = view.far;
    // Pixel space (x, y, depth, w) to clip space: x and y to −1..1 with y up,
    // depth to 1 (nearest) .. 0 (farthest), all still times w.
    let mut rows = [[0.0f64; 4]; 4];
    for j in 0..4 {
        rows[0][j] = 2.0 / w * m[0][j] - m[3][j];
        rows[1][j] = m[3][j] - 2.0 / h * m[1][j];
        rows[2][j] = (f * m[3][j] - m[2][j]) / (f + 1.0);
        rows[3][j] = m[3][j];
    }
    // Positions are taken from the camera's center, where the picture needs
    // its precision.
    let origin = match options.origin {
        Origin::Camera => [list.camera.center[0], list.camera.center[1], 0.0],
        Origin::World => [0.0; 3],
    };
    for row in &mut rows {
        row[3] += row[0] * origin[0] as f64 + row[1] * origin[1] as f64 + row[2] * origin[2] as f64;
    }
    let mut out = Vec::with_capacity(GLOBALS_BYTES);
    for j in 0..4 {
        f32s(&mut out, &[rows[0][j] as f32, rows[1][j] as f32, rows[2][j] as f32, rows[3][j] as f32]);
    }
    f32s(&mut out, &[origin[0], origin[1], origin[2], 0.0]);

    // The depth of a level plane over the screen: the reference's own equation.
    let p = view.proj.level_planes();
    f32s(&mut out, &[(p.a[0] / p.beta) as f32, (p.a[1] / p.beta) as f32, (p.a[2] / p.beta) as f32, 0.0]);
    f32s(&mut out, &[(p.b[0] / p.beta) as f32, (p.b[1] / p.beta) as f32, (p.b[2] / p.beta) as f32, 0.0]);
    let scale = if view.iso() && options.iso_depth == crate::IsoDepth::Exponent { ISO_DEPTH_SCALE } else { 0.0 };
    f32s(&mut out, &[(p.alpha / p.beta) as f32, scale, f as f32, (1.0 / (f + 1.0)) as f32]);
    f32s(&mut out, &[(2.0 / w) as f32, (2.0 / h) as f32, w as f32, h as f32]);
    // The reference's depth of a point taken from the origin (used in the iso view).
    let z = m[2];
    let at_origin = z[3] + z[0] * origin[0] as f64 + z[1] * origin[1] as f64 + z[2] * origin[2] as f64;
    f32s(&mut out, &[z[0] as f32, z[1] as f32, z[2] as f32, at_origin as f32]);

    // The light on each face, and each level's color under it from the
    // reference's own function.
    let light: [f32; 6] = std::array::from_fn(|face| list.light.shade(BOX_FACES[face].0));
    f32s(&mut out, &light);
    f32s(&mut out, &[0.0, 0.0]);
    let mut flags = 0;
    if options.color == FaceColor::Shader {
        flags |= SHADE_IN_SHADER;
    }
    if options.cards == crate::CardDepth::Anchor {
        flags |= CARD_ONE_DEPTH;
    }
    if view.iso() && options.columns == crate::Columns::Reference {
        flags |= ISO_COLUMNS;
    }
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&[0; 12]);
    for k in light {
        for level in 0..=255u8 {
            out.push(shade(Rgba::rgb(level, level, level), k).r);
        }
    }
    debug_assert_eq!(out.len(), GLOBALS_BYTES);
    out
}

/// The 40 pixels of each glyph as bits: bit `row * 5 + col`.
fn glyph_bits(index: u8) -> [i32; 2] {
    static BITS: OnceLock<Vec<[i32; 2]>> = OnceLock::new();
    let table = BITS.get_or_init(|| {
        (0..=255u8)
            .map(|index| {
                let mut bits = 0u64;
                for row in 0..8 {
                    for col in 0..5 {
                        if lit(index, col, row) {
                            bits |= 1 << (row * 5 + col);
                        }
                    }
                }
                [bits as u32 as i32, (bits >> 32) as u32 as i32]
            })
            .collect()
    });
    table[index as usize]
}

/// How a card's depth runs over its rows, in the reference's depth.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CardDepth {
    /// The anchor's row, the depth there, its change per row above the
    /// anchor, the anchor's height.
    pub d0: [f32; 4],
    /// What the lift adds at the anchor, per row above and per row below;
    /// and the change per row below the anchor.
    pub d1: [f32; 4],
    pub lift: u8,
}

fn push_shape(out: &mut Vec<u8>, shape: &Shape, color: Rgba, depth: &CardDepth) {
    let b = shape.bbox();
    i32s(out, &[b.x0, b.y0, b.x1, b.y1]);
    let kind = match shape {
        Shape::Rect(_) => {
            i32s(out, &[0; 8]);
            0
        }
        Shape::Ellipse(_) => {
            i32s(out, &[0; 8]);
            1
        }
        Shape::Quad(p) => {
            i32s(out, &[p[0][0], p[0][1], p[1][0], p[1][1], p[2][0], p[2][1], p[3][0], p[3][1]]);
            2
        }
        Shape::Glyph { scale, index, .. } => {
            let bits = glyph_bits(*index);
            i32s(out, &[*scale, bits[0], bits[1], 0, 0, 0, 0, 0]);
            3
        }
    };
    rgba(out, color);
    out.extend_from_slice(&[kind, depth.lift, 0, 0]);
    f32s(out, &depth.d0);
    f32s(out, &depth.d1);
}

/// One see-through thing of the world.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Glass {
    /// An instance: its index in the instance buffer.
    Flat(u32),
    Box(u32),
    /// A card: its shapes.
    Card(Range<u32>),
}

/// Everything of a frame that depends on the camera.
#[derive(Debug, Default)]
pub(crate) struct Shapes {
    pub bytes: Vec<u8>,
    /// The opaque cards' shapes, in list order.
    pub cards: Range<u32>,
    /// See-through things of the world, far to near, each with the pixels
    /// it may touch.
    pub glass: Vec<(Glass, Rect)>,
    /// The 2D layer in list order: a shape, its pixels, is it see-through.
    pub overlay: Vec<(u32, Rect, bool)>,
}

/// The pixels a see-through instance may touch.
fn instance_bounds(proj: &Projector, inst: &Instance, frame: Rect) -> Rect {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for i in 0..8 {
        let corner =
            |axis: usize| inst.pos[axis] as f64 + if i >> axis & 1 != 0 { inst.size[axis] as f64 } else { 0.0 };
        let h = proj.hom([corner(0), corner(1), corner(2)], 0);
        if !proj.inside(&h) {
            // Cut by the camera's range: anywhere.
            return frame;
        }
        for (axis, v) in [h[0] / h[3], h[1] / h[3]].into_iter().enumerate() {
            lo[axis] = lo[axis].min(v);
            hi[axis] = hi[axis].max(v);
        }
    }
    let r = Rect {
        x0: lo[0].floor() as i32 - 1,
        y0: lo[1].floor() as i32 - 1,
        x1: hi[0].ceil() as i32 + 1,
        y1: hi[1].ceil() as i32 + 1,
    };
    r.intersect(&frame)
}

pub(crate) fn shapes(list: &RenderList, view: &View, solid: &Instances) -> Shapes {
    let proj = &view.proj;
    let frame = Rect { x0: 0, y0: 0, x1: list.view.width as i32, y1: list.view.height as i32 };
    let mut out = Shapes::default();
    let mut glass_bytes = Vec::new();
    // See-through things in the reference's order: instances and cards as
    // the list interleaves them, then far to near (a stable sort).
    let mut glass: Vec<(f32, Glass, Rect)> = Vec::new();
    let mut scratch = Vec::new();
    let mut next_glass = solid.glass.iter().peekable();
    let (mut i, mut c) = (0, 0);
    let mut cards = 0u32;
    let mut glass_shapes = 0u32;
    while i < list.instances.len() || c < list.cards.len() {
        let solid_next =
            c >= list.cards.len() || (i < list.instances.len() && list.instances[i].seq <= list.cards[c].seq);
        if solid_next {
            if let Some(&(_, at)) = next_glass.next_if(|(k, _)| *k == i) {
                let inst = &list.instances[i];
                let what = if inst.mesh == Mesh::Flat { Glass::Flat(at) } else { Glass::Box(at) };
                glass.push((middle_depth(proj, inst).1, what, instance_bounds(proj, inst, frame)));
            }
            i += 1;
            continue;
        }
        let card = &list.cards[c];
        c += 1;
        if !card.enabled {
            continue;
        }
        let anchor = card.anchor.map(f64::from);
        // Out of the camera's range: not drawn.
        let Some(place) = proj.card(anchor, card.lift) else { continue };
        let flat = if card.lift == 0 { place } else { proj.card(anchor, 0).unwrap_or(place) };
        let depth = CardDepth {
            d0: [place.y as f32, flat.at as f32, flat.up as f32, card.anchor[2]],
            d1: [
                (place.at - flat.at) as f32,
                (place.up - flat.up) as f32,
                (place.down - flat.down) as f32,
                flat.down as f32,
            ],
            lift: card.lift,
        };
        scratch.clear();
        card_shapes(card, &place, &mut scratch);
        let see_through = card.color.a < 255;
        let bytes = if see_through { &mut glass_bytes } else { &mut out.bytes };
        for s in &scratch {
            push_shape(bytes, s, card.color, &depth);
        }
        let n = scratch.len() as u32;
        if see_through {
            let bounds = scratch.iter().fold(Rect::EMPTY, |acc, s| acc.union(&s.bbox())).intersect(&frame);
            glass.push((place.at as f32, Glass::Card(glass_shapes..glass_shapes + n), bounds));
            glass_shapes += n;
        } else {
            cards += n;
        }
    }
    out.cards = 0..cards;
    out.bytes.append(&mut glass_bytes);
    glass.sort_by(|a, b| b.0.total_cmp(&a.0));
    out.glass = glass
        .into_iter()
        .map(|(_, what, bounds)| match what {
            Glass::Card(r) => (Glass::Card(r.start + cards..r.end + cards), bounds),
            other => (other, bounds),
        })
        .collect();
    for (at, item) in (cards + glass_shapes..).zip(list.overlay.iter().filter(|d| d.enabled)) {
        push_shape(&mut out.bytes, &item.shape, item.color, &CardDepth::default());
        out.overlay.push((at, item.shape.bbox().intersect(&frame), item.color.a < 255));
    }
    out
}

#[cfg(test)]
mod tests {
    use rwgpuengine_core::color::Rgba;

    use super::*;

    /// `held` of the shader for the iso view, operation for operation.
    fn held_iso(d: f32) -> f32 {
        let u = ISO_DEPTH_SCALE * d.clamp(-1.0, 1.0);
        let n = u.floor();
        f32::from_bits((2.0 - (u - n)).to_bits() - (((n as i32 + ISO_DEPTH_SCALE as i32 + 1) as u32) << 23))
    }

    #[test]
    fn the_iso_depth_is_stored_in_order_and_finely_around_the_scene() {
        // The whole range fits: 1 at the nearest depth, a small positive number at the farthest.
        assert_eq!(held_iso(-1.0), 1.0);
        assert!(held_iso(1.0) > 0.0 && held_iso(1.0) < 1e-35);
        // Nearer is larger, all the way through, also across the steps of the exponent.
        let mut last = held_iso(-1.0);
        for k in 1..=400_000 {
            let d = -1.0 + k as f32 / 200_000.0;
            let h = held_iso(d);
            assert!(h <= last, "{d}: {h} after {last}");
            last = h;
        }
        for step in [-2.0f32 / 60.0, -1.0 / 60.0, 0.0, 1.0 / 60.0, 2.0 / 60.0] {
            let (before, after) = (f32::from_bits(step.to_bits().wrapping_sub(1)), f32::from_bits(step.to_bits() + 1));
            let (lo, hi) = if before < after { (before, after) } else { (after, before) };
            assert!(held_iso(lo) >= held_iso(step) && held_iso(step) >= held_iso(hi), "around {step}");
        }
        // Around the scene, of 1,000 depths 5e-9 apart nearly all are told
        // apart. A straight line to 1..0 tells fewer than a tenth apart.
        for start in [0.0f32, 0.003, -0.003, 0.02, -0.02] {
            let depths = (0..1000).map(|k| start + k as f32 * 5e-9);
            let distinct = |held: &dyn Fn(f32) -> f32| {
                let mut v: Vec<u32> = depths.clone().map(|d| held(d).to_bits()).collect();
                v.dedup();
                v.len()
            };
            assert!(distinct(&held_iso) > 800, "{start}: {}", distinct(&held_iso));
            assert!(distinct(&|d| (1.0 - d) / 2.0) < 100, "{start}: the straight line");
        }
    }

    #[test]
    fn the_meshes_are_the_reference_faces() {
        // Four sides and two level faces, six corners each.
        assert_eq!(mesh(0..4).len(), 4 * 6 * 4);
        let level = mesh(4..6);
        assert_eq!(level.len(), 2 * 6 * 4);
        // The top comes first: a flat rectangle draws only those six.
        assert!(level.chunks(4).take(6).all(|v| v[2] == 1 && v[3] == 4));
        assert!(level.chunks(4).skip(6).all(|v| v[2] == 0 && v[3] == 5));
    }

    #[test]
    fn instances_are_packed_by_how_they_are_drawn() {
        use rwgpuengine_render::Part;
        let inst = |mesh: Mesh, alpha: u8, enabled: bool, layer: u8| Instance {
            entity: Some(1),
            part: Part::Block,
            cell: [0, 0],
            enabled,
            mesh,
            pos: [1.0, 2.0, 3.0],
            size: [1.0, 1.0, 0.5],
            color: Rgba { r: 10, g: 20, b: 30, a: alpha },
            top: Rgba::rgb(40, 50, 60),
            layer,
            seq: 1,
        };
        let list = [
            inst(Mesh::Box, 255, true, 0),
            inst(Mesh::Flat, 255, true, 1),
            inst(Mesh::Box, 128, true, 0),
            inst(Mesh::Flat, 255, false, 2),
            inst(Mesh::Flat, 255, true, 200),
        ];
        let packed = instances(&list);
        // Opaque flat rectangles, then opaque boxes, then the see-through one; the disabled one is left out.
        assert_eq!((packed.flats.clone(), packed.boxes.clone()), (0..2, 2..3));
        assert_eq!(packed.glass, vec![(2, 3)]);
        assert_eq!(packed.bytes.len(), 4 * INSTANCE_BYTES);
        // The rank is the layer plus one, and no more than the three depth bits hold.
        let rank = |k: usize| packed.bytes[k * INSTANCE_BYTES + 32];
        assert_eq!([rank(0), rank(1), rank(2), rank(3)], [2, 6, 1, 1]);
        assert_eq!(&packed.bytes[24..28], &[10, 20, 30, 255]);
    }

    #[test]
    fn glyph_bits_are_the_font() {
        for index in [0u8, 1, 33, 65, 94, 95, 200] {
            let bits = glyph_bits(index);
            for row in 0..8 {
                for col in 0..5 {
                    let bit = (row * 5 + col) as u32;
                    let word = if bit >= 32 { bits[1] } else { bits[0] } as u32;
                    assert_eq!((word >> (bit & 31)) & 1 == 1, lit(index, col, row), "glyph {index} ({col}, {row})");
                }
            }
        }
    }
}
