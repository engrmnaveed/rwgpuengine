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
use rwgpuengine_render::font::lit;
use rwgpuengine_render::reference::{BOX_FACES, card_shapes, middle_depth, shade};
use rwgpuengine_render::{Instance, Mesh, Projector, Rect, RenderList, Shape};

use crate::{FaceColor, Options, Origin, Rank};

/// Bytes of one instance: position, size, two colors, the rank.
pub(crate) const INSTANCE_BYTES: usize = 36;
/// Bytes of one flat shape.
pub(crate) const SHAPE_BYTES: usize = 88;
pub(crate) const GLOBALS_BYTES: usize = 1728;

/// Shader flags (`shader.wgsl`).
const SHADE_IN_SHADER: u32 = 1;
const RANK_BY_BIAS: u32 = 2;
const CARD_ONE_DEPTH: u32 = 4;

/// The highest rank a level face can have: three bits of its depth hold it.
const MAX_RANK: u8 = 7;

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
    let drawn = |i: &&Instance| i.enabled;
    let mut n = 0;
    for i in list.iter().filter(drawn).filter(|i| !see_through(i) && i.mesh == Mesh::Flat) {
        push(&mut out.bytes, i);
        n += 1;
    }
    out.flats = 0..n;
    for i in list.iter().filter(drawn).filter(|i| !see_through(i) && i.mesh == Mesh::Box) {
        push(&mut out.bytes, i);
        n += 1;
    }
    out.boxes = out.flats.end..n;
    for (k, i) in list.iter().enumerate().filter(|(_, i)| i.enabled && see_through(i)) {
        push(&mut out.bytes, i);
        out.glass.push((k, n));
        n += 1;
    }
    out
}

/// The depth of level planes on the screen.
///
/// For a point of the plane at height `z0` that is drawn at pixel (x, y),
/// the reference's depth is `dot(a * z0 + b, (x, y, 1)) / (alpha * z0 + beta)`.
/// It follows from the projector's matrix alone, so it holds for both views.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LevelPlane {
    a: [f64; 3],
    b: [f64; 3],
    alpha: f64,
    beta: f64,
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl LevelPlane {
    pub fn new(proj: &Projector) -> LevelPlane {
        let m = proj.rows();
        // Columns of the matrix, as (x, y, w): what a step along world x, y
        // and z adds, and the constant.
        let col = |j: usize| [m[0][j], m[1][j], m[3][j]];
        let (c0, c1, cz, c3) = (col(0), col(1), col(2), col(3));
        let n = cross(c0, c1);
        let z = m[2];
        let mix = |c: [f64; 3], k: f64| {
            let (p, q) = (cross(c1, c), cross(c, c0));
            [
                z[0] * p[0] + z[1] * q[0] + k * n[0],
                z[0] * p[1] + z[1] * q[1] + k * n[1],
                z[0] * p[2] + z[1] * q[2] + k * n[2],
            ]
        };
        LevelPlane { a: mix(cz, z[2]), b: mix(c3, z[3]), alpha: dot(cz, n), beta: dot(c3, n) }
    }

    /// The reference's depth of the plane at height `z0`, at pixel (x, y).
    #[cfg(test)]
    pub fn depth(&self, x: f64, y: f64, z0: f64) -> f64 {
        let v = [self.a[0] * z0 + self.b[0], self.a[1] * z0 + self.b[1], self.a[2] * z0 + self.b[2]];
        dot(v, [x, y, 1.0]) / (self.alpha * z0 + self.beta)
    }
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

    /// The reference's depth as the GPU holds it: 1 nearest, 0 farthest.
    pub fn depth(&self, d: f64) -> f64 {
        (self.far - d) / (self.far + 1.0)
    }

    /// A change of the reference's depth, as the GPU holds it.
    pub fn slope(&self, d: f64) -> f64 {
        -d / (self.far + 1.0)
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
    let bias = match options.rank {
        Rank::DepthBits => 0.0,
        Rank::Bias(steps) => steps as f32 / 16_777_216.0,
    };
    f32s(&mut out, &[origin[0], origin[1], origin[2], bias]);

    let p = LevelPlane::new(&view.proj);
    let k = 1.0 / ((f + 1.0) * p.beta);
    f32s(&mut out, &[(p.a[0] * k) as f32, (p.a[1] * k) as f32, (p.a[2] * k) as f32, 0.0]);
    f32s(&mut out, &[(p.b[0] * k) as f32, (p.b[1] * k) as f32, (p.b[2] * k) as f32, (f / (f + 1.0)) as f32]);
    f32s(&mut out, &[(p.alpha / p.beta) as f32, 0.0, 0.0, 0.0]);
    f32s(&mut out, &[(2.0 / w) as f32, (2.0 / h) as f32, w as f32, h as f32]);

    // The light on each face, and each level's color under it from the
    // reference's own function.
    let light: [f32; 6] = std::array::from_fn(|face| list.light.shade(BOX_FACES[face].0));
    f32s(&mut out, &light);
    f32s(&mut out, &[0.0, 0.0]);
    let mut flags = 0;
    if options.color == FaceColor::Shader {
        flags |= SHADE_IN_SHADER;
    }
    if options.rank != Rank::DepthBits {
        flags |= RANK_BY_BIAS;
    }
    if options.cards == crate::CardDepth::Anchor {
        flags |= CARD_ONE_DEPTH;
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

/// How a card's depth runs over its rows, as the GPU holds depth.
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
            d0: [place.y as f32, view.depth(flat.at) as f32, view.slope(flat.up) as f32, card.anchor[2]],
            d1: [
                view.slope(place.at - flat.at) as f32,
                view.slope(place.up - flat.up) as f32,
                view.slope(place.down - flat.down) as f32,
                view.slope(flat.down) as f32,
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
    use rwgpuengine_data::CameraView;
    use rwgpuengine_render::{Camera, Projector};

    use super::*;

    fn view() -> rwgpuengine_render::View {
        rwgpuengine_render::View {
            width: 640,
            height: 480,
            background: Rgba::BLACK,
            tile_w: 64,
            tile_h: 32,
            z_h: 32,
            pivot: [320.0, 240.0],
        }
    }

    #[test]
    fn a_level_plane_has_the_depth_the_projector_gives_its_points() {
        for kind in [CameraView::Iso, CameraView::Perspective] {
            for (turn, zoom) in [(0.0, 1.0), (33.0, 0.5), (217.0, 1.75), (295.0, 3.0)] {
                let cam = Camera { view: kind, center: [4.5, 3.5], turn, zoom };
                let proj = Projector::new(&view(), &cam);
                let plane = LevelPlane::new(&proj);
                for z0 in [0.0, 0.5, 1.2, -2.0, 3.0] {
                    for (x, y) in [(4.5, 3.5), (0.0, 0.0), (9.25, -1.5), (-3.0, 8.0), (4.0, 3.0)] {
                        let h = proj.hom([x, y, z0], 0);
                        if h[3] <= 0.0 {
                            continue;
                        }
                        let want = h[2] / h[3];
                        let got = plane.depth(h[0] / h[3], h[1] / h[3], z0);
                        assert!((got - want).abs() < 1e-12, "{kind:?} {turn} {zoom} ({x}, {y}, {z0}): {got} {want}");
                    }
                }
            }
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
        assert_eq!([rank(0), rank(1), rank(2), rank(3)], [2, 7, 1, 1]);
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
