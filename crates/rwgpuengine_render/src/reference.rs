//! The CPU reference renderer.
//!
//! It draws a [`RenderList`] with nothing but integers and exact float
//! operations, so the same list gives the same bytes on every platform and
//! screenshots can be compared with zero tolerance. It is the source of truth
//! for what a frame looks like: `rwgpu shot`, `rwgpu test`, the frame report
//! and picking all use it, and a GPU renderer is held to it.
//!
//! Two stages:
//!
//! 1. [`project`] takes the list to the screen: every instance becomes
//!    triangles with corners on a 1/256 pixel grid and a depth at each
//!    corner, every card becomes flat shapes at one depth.
//! 2. [`render`] fills the pixels. Solid things and cards go through a depth
//!    buffer, so order does not matter for them; see-through ones follow, far
//!    to near; the 2D layer is painted last. Besides colors it records which
//!    item owns each pixel, which is what the frame report measures
//!    visibility with.
//!
//! One rule goes beyond "the nearest wins". Flat things lie *in* the plane of
//! what they lie on: a tile on the ground, a mat on a table. Depth cannot
//! order two faces in one plane, so level faces carry a rank, and where two
//! of them are at the same depth the higher rank shows ([`nearer`]). For
//! that to hold whatever the faces' corners are, a level face takes its
//! depth from its plane's equation ([`LevelPlanes`]), not from its corners.
//! The part of a card below its anchor lies in such a plane too, and shows
//! over a floor there.
//!
//! [`Painter`] is the same drawing without the owners, for players that only
//! show the picture.

use std::collections::BTreeMap;

use rwgpuengine_core::color::Rgba;
use rwgpuengine_core::hash::fnv1a64;
use rwgpuengine_sim::EntityId;

use crate::camera::{CardPlace, Hom, LevelPlanes, Projector, dot4, to_fixed};
use crate::draw::{self, Part, Rect, Shape};
use crate::list::{Card, CardBody, Instance, Light, Mesh, RenderList};
use crate::paint::{ceil_div, fill, solid, spans};
use crate::view::round;

/// Sub-pixel steps per pixel, and the offset of a pixel's center.
const SUB: i64 = 256;
const HALF: i64 = SUB / 2;

/// One triangle on the screen: corners in sub-pixel steps, clockwise (y
/// down), each with its depth.
#[derive(Clone, Copy, Debug)]
struct Tri {
    p: [[i64; 2]; 3],
    z: [f64; 3],
    /// A level face: its plane's depth over the pixels
    /// ([`LevelPlanes::over_pixels`]), used in place of the corners' depths.
    /// Corners are snapped to the sub-pixel grid, so two triangles of one
    /// plane would give depths that differ by more than rounding.
    plane: Option<[f64; 3]>,
    color: Rgba,
    /// 0 for a face that is not level; else 1 + the instance's layer.
    rank: u8,
}

/// The rank of the part of a card that lies on a floor: above every level
/// face, so in one plane with a floor the card shows.
pub(crate) const RANK_LYING: u8 = u8::MAX;

#[derive(Clone, Debug)]
enum Kind {
    /// Triangles `tris.0..tris.1` of the projection.
    Solid { tris: (u32, u32) },
    /// Shapes `shapes.0..shapes.1`, with the card's depth row by row.
    /// `lying` is the level plane through the anchor of a card that is not
    /// lifted off it: the depth of the rows at and below the anchor.
    Card { shapes: (u32, u32), place: CardPlace, color: Rgba, lying: Option<[f64; 3]> },
    /// A shape of the 2D layer: in front of the world, in list order.
    Overlay { shape: Shape, color: Rgba },
}

/// One thing the frame draws, in screen space.
#[derive(Clone, Debug)]
pub struct Item {
    /// `None` for ground cells.
    pub entity: Option<EntityId>,
    pub part: Part,
    /// Grid cell of a ground cell or a world entity ([0, 0] in the 2D layer).
    pub cell: [i32; 2],
    pub enabled: bool,
    /// Pixel rectangle around everything the item draws; it may reach
    /// outside the frame. Empty if the item faces away or is out of range.
    pub bounds: Rect,
    /// Nothing of the item is within the camera's range (behind it, too far,
    /// or far off to a side).
    pub out_of_range: bool,
    /// See-through items are drawn after the solid ones, far to near.
    translucent: bool,
    sort: f32,
    kind: Kind,
}

/// A render list seen from its camera.
#[derive(Clone, Debug)]
pub struct Projected {
    pub width: u32,
    pub height: u32,
    pub background: Rgba,
    /// The ground, then each world entity's parts, then the 2D layer.
    pub items: Vec<Item>,
    tris: Vec<Tri>,
    shapes: Vec<Shape>,
    /// Enabled items in the order they are drawn.
    order: Vec<u32>,
    /// Items before this index are in the world; the rest is the 2D layer.
    world: usize,
}

/// A rendered frame.
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// RGBA8, row-major, top row first.
    pub rgba: Vec<u8>,
    /// Owner of each pixel: index into [`Projected::items`] + 1, or 0 for
    /// the background.
    pub owner: Vec<u32>,
}

impl Frame {
    /// FNV-1a of the pixels. Equal hashes mean identical images.
    pub fn image_hash(&self) -> u64 {
        fnv1a64(&self.rgba)
    }

    pub fn pixel(&self, x: u32, y: u32) -> Rgba {
        let i = ((y * self.width + x) * 4) as usize;
        Rgba { r: self.rgba[i], g: self.rgba[i + 1], b: self.rgba[i + 2], a: self.rgba[i + 3] }
    }
}

/// Per-item pixel accounting.
#[derive(Clone, Debug, Default)]
pub struct ItemStats {
    /// Pixels the item covers inside the frame, seen or not.
    pub drawn: u32,
    /// Pixels that show the item.
    pub visible: u32,
    /// Where the item is covered: the item that shows instead, by index.
    pub covered_by: BTreeMap<usize, u32>,
}

/// Who covers how many pixels of a frame.
#[derive(Clone, Debug, Default)]
pub struct Coverage {
    /// One entry per item of the projection (ground cells are not counted).
    pub items: Vec<ItemStats>,
    /// Pixels inside the frame covered by any part of an entity, each once.
    pub entity_px: BTreeMap<EntityId, u32>,
}

/// The six faces of a box: outward normal and corners. Corner `i` is at
/// `pos + size * (i & 1, i >> 1 & 1, i >> 2 & 1)`.
pub const BOX_FACES: [([f32; 3], [usize; 4]); 6] = [
    ([1.0, 0.0, 0.0], [1, 3, 7, 5]),
    ([-1.0, 0.0, 0.0], [0, 4, 6, 2]),
    ([0.0, 1.0, 0.0], [2, 6, 7, 3]),
    ([0.0, -1.0, 0.0], [0, 1, 5, 4]),
    ([0.0, 0.0, 1.0], [4, 5, 7, 6]),
    ([0.0, 0.0, -1.0], [0, 2, 3, 1]),
];

/// A face's color under the light: each channel rounded to the nearest level.
pub fn shade(color: Rgba, k: f32) -> Rgba {
    let s = |c: u8| (c as f32 * k + 0.5).floor().clamp(0.0, 255.0) as u8;
    Rgba { r: s(color.r), g: s(color.g), b: s(color.b), a: color.a }
}

/// Take a render list to the screen.
pub fn project(list: &RenderList) -> Projected {
    let proj = Projector::new(&list.view, &list.camera);
    let planes = proj.level_planes();
    let mut out = Projected {
        width: list.view.width,
        height: list.view.height,
        background: list.view.background,
        items: Vec::with_capacity(list.instances.len() + list.cards.len() + list.overlay.len()),
        tris: Vec::with_capacity(list.instances.len() * 6),
        shapes: Vec::new(),
        order: Vec::new(),
        world: 0,
    };
    // Both lists are in drawing order; within one entity, solid parts come first.
    let (mut i, mut c) = (0, 0);
    let mut clip = (Vec::new(), Vec::new());
    while i < list.instances.len() || c < list.cards.len() {
        let solid_next =
            c >= list.cards.len() || (i < list.instances.len() && list.instances[i].seq <= list.cards[c].seq);
        if solid_next {
            out.push_instance(&proj, &planes, &list.light, &list.instances[i], &mut clip);
            i += 1;
        } else {
            out.push_card(&proj, &planes, &list.cards[c]);
            c += 1;
        }
    }
    out.world = out.items.len();
    for d in &list.overlay {
        out.items.push(Item {
            entity: Some(d.entity),
            part: d.part,
            cell: [0, 0],
            enabled: d.enabled,
            bounds: d.shape.bbox(),
            out_of_range: false,
            translucent: false,
            sort: 0.0,
            kind: Kind::Overlay { shape: d.shape, color: d.color },
        });
    }
    // Solid world items in list order, see-through ones far to near, then the 2D layer.
    let enabled = |i: &usize| out.items[*i].enabled;
    let mut order: Vec<u32> =
        (0..out.world).filter(enabled).filter(|i| !out.items[*i].translucent).map(|i| i as u32).collect();
    let mut glass: Vec<u32> =
        (0..out.world).filter(enabled).filter(|i| out.items[*i].translucent).map(|i| i as u32).collect();
    glass.sort_by(|a, b| out.items[*b as usize].sort.total_cmp(&out.items[*a as usize].sort));
    order.extend(glass);
    order.extend((out.world..out.items.len()).filter(enabled).map(|i| i as u32));
    out.order = order;
    out
}

impl Projected {
    fn push_instance(
        &mut self,
        proj: &Projector,
        planes: &LevelPlanes,
        light: &Light,
        inst: &Instance,
        clip: &mut (Vec<Hom>, Vec<Hom>),
    ) {
        let p = inst.pos.map(f64::from);
        let s = inst.size.map(f64::from);
        let corner = |i: usize| {
            [
                p[0] + if i & 1 != 0 { s[0] } else { 0.0 },
                p[1] + if i & 2 != 0 { s[1] } else { 0.0 },
                p[2] + if i & 4 != 0 { s[2] } else { 0.0 },
            ]
        };
        let start = self.tris.len();
        // Level faces: 1 + the layer, below a card that lies on them.
        let rank = inst.layer.min(RANK_LYING - 2) + 1;
        match inst.mesh {
            Mesh::Flat => {
                let up = [0.0, 0.0, 1.0];
                if proj.faces_camera(p, up.map(f64::from)) {
                    let quad = [0, 1, 3, 2].map(|i| proj.hom(corner(i), 0));
                    let face = (shade(inst.top, light.shade(up)), rank, planes.over_pixels(inst.pos[2]));
                    self.push_face(proj, &quad, face, clip);
                }
            }
            Mesh::Box => {
                // Each corner is projected once, so faces meet without gaps.
                let hom: [Hom; 8] = std::array::from_fn(|i| proj.hom(corner(i), 0));
                for (n, idx) in BOX_FACES {
                    if !proj.faces_camera(corner(idx[0]), n.map(f64::from)) {
                        continue;
                    }
                    let level = n[2] != 0.0;
                    let base = if level { inst.top } else { inst.color };
                    // The top is at the two numbers' sum as a scene file would hold it.
                    let height = if n[2] > 0.0 { inst.pos[2] + inst.size[2] } else { inst.pos[2] };
                    let plane = if level { planes.over_pixels(height) } else { None };
                    let face = (shade(base, light.shade(n)), if level { rank } else { 0 }, plane);
                    self.push_face(proj, &idx.map(|i| hom[i]), face, clip);
                }
            }
        }
        let tris = &self.tris[start..];
        let (middle, sort) = middle_depth(proj, inst);
        self.items.push(Item {
            entity: inst.entity,
            part: inst.part,
            cell: inst.cell,
            enabled: inst.enabled,
            bounds: tri_bounds(tris),
            out_of_range: !proj.inside(&middle) && tris.is_empty(),
            translucent: inst.color.a < 255 || inst.top.a < 255,
            sort,
            kind: Kind::Solid { tris: (start as u32, self.tris.len() as u32) },
        });
    }

    /// Add one flat face (its color, rank and plane), cut to the camera's
    /// range if it reaches outside.
    fn push_face(&mut self, proj: &Projector, quad: &[Hom; 4], face: Face, clip: &mut (Vec<Hom>, Vec<Hom>)) {
        if quad.iter().all(|h| proj.inside(h)) {
            self.push_fan(quad, face);
            return;
        }
        let (poly, next) = clip;
        poly.clear();
        poly.extend_from_slice(quad);
        for plane in proj.planes() {
            next.clear();
            for k in 0..poly.len() {
                let (cur, nxt) = (poly[k], poly[(k + 1) % poly.len()]);
                let (dc, dn) = (dot4(plane, &cur), dot4(plane, &nxt));
                if dc >= 0.0 {
                    next.push(cur);
                }
                if (dc >= 0.0) != (dn >= 0.0) {
                    // Always from the inside corner to the outside one, so two
                    // faces that share the edge cut it at the same point.
                    let (a, b, da, db) = if dc >= 0.0 { (cur, nxt, dc, dn) } else { (nxt, cur, dn, dc) };
                    let t = da / (da - db);
                    next.push(std::array::from_fn(|j| a[j] + t * (b[j] - a[j])));
                }
            }
            std::mem::swap(poly, next);
            if poly.len() < 3 {
                return;
            }
        }
        self.push_fan(poly, face);
    }

    /// Add a convex polygon as a fan of triangles.
    fn push_fan(&mut self, poly: &[Hom], (color, rank, plane): Face) {
        let at = |h: &Hom| ([to_fixed(h[0] / h[3]), to_fixed(h[1] / h[3])], h[2] / h[3]);
        let (p0, z0) = at(&poly[0]);
        for k in 1..poly.len() - 1 {
            let ((p1, z1), (p2, z2)) = (at(&poly[k]), at(&poly[k + 1]));
            let area = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]);
            match area {
                0 => {}
                a if a > 0 => self.tris.push(Tri { p: [p0, p1, p2], z: [z0, z1, z2], plane, color, rank }),
                _ => self.tris.push(Tri { p: [p0, p2, p1], z: [z0, z2, z1], plane, color, rank }),
            }
        }
    }

    fn push_card(&mut self, proj: &Projector, planes: &LevelPlanes, card: &Card) {
        let place = proj.card(card.anchor.map(f64::from), card.lift);
        let start = self.shapes.len();
        if let Some(at) = &place {
            card_shapes(card, at, &mut self.shapes);
        }
        let out_of_range = place.is_none();
        let place = place.unwrap_or_default();
        self.items.push(Item {
            entity: Some(card.entity),
            part: card.part,
            cell: card.cell,
            enabled: card.enabled,
            bounds: self.shapes[start..].iter().fold(Rect::EMPTY, |acc, s| acc.union(&s.bbox())),
            out_of_range,
            translucent: card.color.a < 255,
            sort: place.at as f32,
            kind: Kind::Card {
                shapes: (start as u32, self.shapes.len() as u32),
                place,
                color: card.color,
                // A lifted card (a label) is in front of the plane, by its own depth.
                lying: if card.lift == 0 { planes.over_pixels(card.anchor[2]) } else { None },
            },
        });
    }

    fn screen(&self) -> Rect {
        Rect { x0: 0, y0: 0, x1: self.width as i32, y1: self.height as i32 }
    }

    /// Call `f(x, y, depth, rank)` for every pixel of `clip` whose center
    /// item `idx` covers, each once. Items of the 2D layer report a depth
    /// nearer than anything in the world.
    pub(crate) fn cover(&self, idx: usize, clip: Rect, mut f: impl FnMut(i32, i32, f32, u8)) {
        match &self.items[idx].kind {
            Kind::Solid { tris } => {
                for t in &self.tris[tris.0 as usize..tris.1 as usize] {
                    scan(t, clip, |y, x0, x1, row| (x0..x1).for_each(|x| f(x, y, row.depth(x), t.rank)));
                }
            }
            Kind::Card { shapes, place, lying, .. } => {
                for s in &self.shapes[shapes.0 as usize..shapes.1 as usize] {
                    spans(s, s.bbox().intersect(&clip), |y, x0, x1| match lying_row(place, lying, y) {
                        Some(row) => (x0..x1).for_each(|x| f(x, y, row.depth(x), RANK_LYING)),
                        None => {
                            let d = place.depth(y);
                            (x0..x1).for_each(|x| f(x, y, d, 0));
                        }
                    });
                }
            }
            Kind::Overlay { shape, .. } => {
                spans(shape, shape.bbox().intersect(&clip), |y, x0, x1| {
                    (x0..x1).for_each(|x| f(x, y, f32::NEG_INFINITY, 0));
                });
            }
        }
    }

    /// Is the item see-through (drawn after the solid ones, without hiding
    /// what is drawn later behind it)?
    pub(crate) fn translucent(&self, idx: usize) -> bool {
        self.items[idx].translucent
    }

    /// Indices of the enabled world items, in the order they are drawn.
    pub(crate) fn world_order(&self) -> impl Iterator<Item = usize> + '_ {
        self.order.iter().map(|i| *i as usize).filter(|i| *i < self.world)
    }

    /// Number of world items; the 2D layer follows.
    pub(crate) fn world_len(&self) -> usize {
        self.world
    }

    /// Pixel rectangle around everything drawn for `id`.
    pub fn bounds_of(&self, id: EntityId) -> Rect {
        self.items.iter().filter(|i| i.entity == Some(id)).fold(Rect::EMPTY, |acc, i| acc.union(&i.bounds))
    }
}

/// The flat shapes a card is drawn with when its anchor is at `at`: one for
/// a sprite, one per character for text. They are added to `out`.
#[inline]
pub fn card_shapes(card: &Card, at: &CardPlace, out: &mut Vec<Shape>) {
    let (ax, ay) = (at.x as f32, at.y as f32);
    match &card.body {
        CardBody::Sprite(s) => {
            let scale = at.scale as f32;
            let (x0, y0) = (round(ax + card.origin[0] * scale), round(ay + card.origin[1] * scale));
            out.push(draw::sprite_shape(s, x0, y0, scale));
        }
        CardBody::Text { glyphs, scale } => {
            let (x0, y0) = (round(ax + card.origin[0]), round(ay + card.origin[1]));
            for &(gx, gy, index) in glyphs {
                out.push(Shape::Glyph { x0: x0 + gx, y0: y0 + gy, scale: *scale, index });
            }
        }
    }
}

/// Where a see-through instance is in the far-to-near order: the depth of
/// its middle.
#[inline]
pub fn middle_depth(proj: &Projector, inst: &Instance) -> (Hom, f32) {
    let p = inst.pos.map(f64::from);
    let s = inst.size.map(f64::from);
    let middle = proj.hom([p[0] + s[0] / 2.0, p[1] + s[1] / 2.0, p[2] + s[2] / 2.0], 0);
    (middle, (middle[2] / middle[3]) as f32)
}

/// A face's color, its rank and, for a level face, its plane over the pixels.
type Face = (Rgba, u8, Option<[f64; 3]>);

/// The depth along pixel row `y` of a card that lies there on its anchor's
/// level plane; `None` where the card stands upright (above the anchor) or
/// is lifted off the plane.
#[inline]
fn lying_row(place: &CardPlace, lying: &Option<[f64; 3]>, y: i32) -> Option<Row> {
    let plane = lying.as_ref().filter(|_| place.lies(y))?;
    Some(Row { dx: plane[0], at_zero: plane[1] * y as f64 + plane[2] })
}

fn tri_bounds(tris: &[Tri]) -> Rect {
    let mut b: Option<[i64; 4]> = None;
    for p in tris.iter().flat_map(|t| t.p) {
        let m = b.get_or_insert([p[0], p[1], p[0], p[1]]);
        *m = [m[0].min(p[0]), m[1].min(p[1]), m[2].max(p[0]), m[3].max(p[1])];
    }
    match b {
        None => Rect::EMPTY,
        Some([x0, y0, x1, y1]) => Rect {
            x0: x0.div_euclid(SUB) as i32,
            y0: y0.div_euclid(SUB) as i32,
            x1: (x1 + SUB - 1).div_euclid(SUB) as i32,
            y1: (y1 + SUB - 1).div_euclid(SUB) as i32,
        },
    }
}

/// One edge of a triangle as a function of the pixel: its value at pixel
/// (x, y) is `r0 + gx * x + gy * y`, and the pixel's center is on the inner
/// side when that is at least `min` (0 on a top or left edge, 1 on the
/// others).
#[derive(Clone, Copy)]
struct Edge {
    r0: i64,
    gx: i64,
    gy: i64,
    min: i64,
}

impl Edge {
    fn new(p: [i64; 2], q: [i64; 2]) -> Edge {
        let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
        let top_left = (dy == 0 && dx > 0) || dy < 0;
        Edge { r0: dx * (HALF - p[1]) - dy * (HALF - p[0]), gx: -dy * SUB, gy: dx * SUB, min: i64::from(!top_left) }
    }
}

/// A triangle's depth along one row of pixels: [`Row::depth`] of pixel `x`.
/// It depends only on the triangle and the pixel, never on a clip rectangle,
/// so picking one pixel sees exactly the depth the renderer saw.
#[derive(Clone, Copy)]
struct Row {
    dx: f64,
    at_zero: f64,
}

impl Row {
    #[inline]
    fn depth(&self, x: i32) -> f32 {
        (self.dx * x as f64 + self.at_zero) as f32
    }
}

/// Call `f(y, x0, x1, row)` for every row of `clip` in which the triangle
/// covers pixel centers: pixels `x0..x1` (`x1` exclusive, `x0 < x1`).
/// Coverage is exact integer math with a top-left rule (the same rule as
/// [`Shape::covers`]), so triangles that share an edge draw every pixel
/// along it exactly once. Each row's run is solved for, not searched.
#[inline]
fn scan(t: &Tri, clip: Rect, mut f: impl FnMut(i32, i32, i32, Row)) {
    let [a, b, c] = t.p;
    let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    if area <= 0 {
        return;
    }
    // Pixels whose centers lie inside the bounding box.
    let lo = |v: i64| (v - HALF + SUB - 1).div_euclid(SUB);
    let hi = |v: i64| (v - HALF).div_euclid(SUB);
    let x0 = lo(a[0].min(b[0]).min(c[0])).max(clip.x0 as i64);
    let x1 = hi(a[0].max(b[0]).max(c[0])).min(clip.x1 as i64 - 1);
    let y0 = lo(a[1].min(b[1]).min(c[1])).max(clip.y0 as i64);
    let y1 = hi(a[1].max(b[1]).max(c[1])).min(clip.y1 as i64 - 1);
    if x0 > x1 || y0 > y1 {
        return;
    }
    // The value of the edge opposite a corner, over the area, is that
    // corner's weight; so depth is a plane over the pixels:
    // depth = d0 + dx * x + dy * y.
    let edges = [Edge::new(b, c), Edge::new(c, a), Edge::new(a, b)];
    let [dx, dy, d0] = t.plane.unwrap_or_else(|| {
        let inv = 1.0 / area as f64;
        let fit = |v: [i64; 3]| (v[0] as f64 * t.z[0] + v[1] as f64 * t.z[1] + v[2] as f64 * t.z[2]) * inv;
        [fit(edges.map(|e| e.gx)), fit(edges.map(|e| e.gy)), fit(edges.map(|e| e.r0))]
    });
    for y in y0..=y1 {
        let (mut from, mut to) = (x0, x1);
        for e in &edges {
            let r = e.r0 + e.gy * y;
            if e.gx > 0 {
                from = from.max(ceil_div(e.min - r, e.gx));
            } else if e.gx < 0 {
                to = to.min((r - e.min).div_euclid(-e.gx));
            } else if r < e.min {
                to = from - 1;
            }
        }
        if from <= to {
            f(y as i32, from as i32, to as i32 + 1, Row { dx, at_zero: dy * y as f64 + d0 });
        }
    }
}

/// Does a new piece of surface at depth `d` show in front of the one held
/// at depth `held`?
///
/// The nearer one shows. The exception is two level faces at the same depth
/// (within the rounding of an `f32`): they are in one plane, depth cannot
/// order them, and the higher rank shows; with equal ranks the one drawn
/// first stays. The part of a card that lies on a floor is in such a plane
/// too, with the highest rank ([`RANK_LYING`]): it shows over the floor.
/// Faces that are not level, and cards where they stand upright, have rank 0
/// and are ordered by depth alone, so a tile never shows through the foot of
/// a box standing on it.
#[inline]
pub(crate) fn nearer(d: f32, rank: u8, held: f32, held_rank: u8) -> bool {
    if rank != 0 && held_rank != 0 && (d - held).abs() <= 1.0e-6 * d.abs().max(held.abs()) + 1.0e-12 {
        return rank > held_rank;
    }
    d < held
}

/// What the depth buffer holds for a pixel: the depth and rank of the solid
/// surface drawn there, or (infinity, 0).
#[derive(Debug, Default)]
struct Depths {
    depth: Vec<f32>,
    rank: Vec<u8>,
}

impl Depths {
    fn clear(&mut self, n: usize) {
        self.depth.clear();
        self.depth.resize(n, f32::INFINITY);
        self.rank.clear();
        self.rank.resize(n, 0);
    }
}

/// The buffers a frame is drawn into.
struct Target<'a> {
    width: usize,
    rgba: &'a mut [u8],
    held: &'a mut Depths,
    owner: Option<&'a mut [u32]>,
}

impl Target<'_> {
    /// Draw pixels `x0..x1` of row `y` wherever the new surface shows in
    /// front of what is held. `keep` is false for see-through items, which
    /// do not hide what is drawn after them.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn run(
        &mut self,
        y: i32,
        x0: i32,
        x1: i32,
        depth: impl Fn(i32) -> f32,
        rank: u8,
        color: Rgba,
        tag: u32,
        keep: bool,
    ) {
        let row = y as usize * self.width;
        let (a, b) = (row + x0 as usize, row + x1 as usize);
        let depths = &mut self.held.depth[a..b];
        let ranks = &mut self.held.rank[a..b];
        let pixels = self.rgba[a * 4..b * 4].as_chunks_mut::<4>().0;
        let mut owner = self.owner.as_deref_mut().map(|o| &mut o[a..b]);
        for (k, ((held, held_rank), px)) in depths.iter_mut().zip(ranks).zip(pixels).enumerate() {
            let d = depth(x0 + k as i32);
            if nearer(d, rank, *held, *held_rank) {
                if keep {
                    *held = d;
                    *held_rank = rank;
                }
                let c = if color.a == 255 { color } else { color.over(Rgba { r: px[0], g: px[1], b: px[2], a: 255 }) };
                *px = [c.r, c.g, c.b, 255];
                if let Some(o) = owner.as_deref_mut() {
                    o[k] = tag;
                }
            }
        }
    }
}

/// Draw every enabled item. `rgba` must already hold the background and
/// `held` must be cleared.
fn draw(p: &Projected, rgba: &mut [u8], held: &mut Depths, owner: Option<&mut [u32]>) {
    let screen = p.screen();
    let mut out = Target { width: p.width as usize, rgba, held, owner };
    for &idx in &p.order {
        let item = &p.items[idx as usize];
        let tag = idx + 1;
        let keep = !item.translucent;
        match &item.kind {
            Kind::Solid { tris } => {
                for t in &p.tris[tris.0 as usize..tris.1 as usize] {
                    scan(t, screen, |y, x0, x1, row| out.run(y, x0, x1, |x| row.depth(x), t.rank, t.color, tag, keep));
                }
            }
            Kind::Card { shapes, place, color, lying } => {
                for s in &p.shapes[shapes.0 as usize..shapes.1 as usize] {
                    spans(s, s.bbox().intersect(&screen), |y, x0, x1| match lying_row(place, lying, y) {
                        Some(row) => out.run(y, x0, x1, |x| row.depth(x), RANK_LYING, *color, tag, keep),
                        None => {
                            let d = place.depth(y);
                            out.run(y, x0, x1, |_| d, 0, *color, tag, keep);
                        }
                    });
                }
            }
            Kind::Overlay { shape, color } => {
                spans(shape, shape.bbox().intersect(&screen), |y, x0, x1| {
                    fill(out.rgba, out.width, y, x0, x1, *color);
                    if let Some(o) = out.owner.as_deref_mut() {
                        let row = y as usize * out.width;
                        o[row + x0 as usize..row + x1 as usize].fill(tag);
                    }
                });
            }
        }
    }
}

/// Render the projection: the reference pixels, and who owns each.
pub fn render(p: &Projected) -> Frame {
    let n = (p.width * p.height) as usize;
    let mut rgba = vec![0; n * 4];
    solid(&mut rgba, [p.background.r, p.background.g, p.background.b, 255]);
    let mut held = Depths::default();
    held.clear(n);
    let mut owner = vec![0u32; n];
    draw(p, &mut rgba, &mut held, Some(&mut owner));
    Frame { width: p.width, height: p.height, rgba, owner }
}

/// Count, for every item, the pixels it covers, the ones that show it, and
/// who shows instead where it is covered.
pub fn coverage(p: &Projected, frame: &Frame) -> Coverage {
    let w = p.width as usize;
    let screen = p.screen();
    let mut out = Coverage { items: vec![ItemStats::default(); p.items.len()], entity_px: BTreeMap::new() };
    // The entity (+ 1) that last counted each pixel. An entity's items follow
    // each other in the list, so this counts each of its pixels once.
    let mut counted = vec![0u32; w * p.height as usize];
    for (idx, item) in p.items.iter().enumerate() {
        // Ground cells are not accounted for: nothing reports on them.
        let Some(entity) = item.entity.filter(|_| item.enabled) else { continue };
        let tag = idx as u32 + 1;
        let stats = &mut out.items[idx];
        let mut union = 0;
        // Covered pixels come in runs with one owner; count a run, then file it.
        let mut run = (0u32, 0u32);
        let file = |stats: &mut ItemStats, (owner, n): (u32, u32)| {
            if n > 0 {
                *stats.covered_by.entry(owner as usize - 1).or_insert(0) += n;
            }
        };
        p.cover(idx, screen, |x, y, _, _| {
            let i = y as usize * w + x as usize;
            stats.drawn += 1;
            match frame.owner[i] {
                o if o == tag => stats.visible += 1,
                0 => {}
                o if o == run.0 => run.1 += 1,
                o => {
                    file(stats, run);
                    run = (o, 1);
                }
            }
            if counted[i] != entity + 1 {
                counted[i] = entity + 1;
                union += 1;
            }
        });
        file(stats, run);
        *out.entity_px.entry(entity).or_insert(0) += union;
    }
    out
}

/// A player's painter: the reference pixels without owners or statistics.
/// It keeps its depth buffer between frames.
#[derive(Debug, Default)]
pub struct Painter {
    held: Depths,
}

impl Painter {
    pub fn new() -> Painter {
        Painter::default()
    }

    /// Paint the list into `rgba` (RGBA8, row-major, top row first). The
    /// buffer is resized to the view and fully overwritten, so a player can
    /// reuse one buffer for every frame.
    pub fn paint(&mut self, list: &RenderList, rgba: &mut Vec<u8>) {
        let p = project(list);
        let n = (p.width * p.height) as usize;
        rgba.resize(n * 4, 0);
        solid(rgba, [p.background.r, p.background.g, p.background.b, 255]);
        self.held.clear(n);
        draw(&p, rgba, &mut self.held, None);
    }
}

#[cfg(test)]
mod tests {
    use rwgpuengine_core::rng::Rng;

    use super::*;

    /// The rule `scan` must match, tested pixel by pixel.
    fn covers(t: &Tri, x: i64, y: i64) -> bool {
        let (cx, cy) = (x * SUB + HALF, y * SUB + HALF);
        (0..3).all(|i| {
            let (p, q) = (t.p[i], t.p[(i + 1) % 3]);
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            let cross = dx * (cy - p[1]) - dy * (cx - p[0]);
            cross > 0 || (cross == 0 && ((dy == 0 && dx > 0) || dy < 0))
        })
    }

    #[test]
    fn a_triangle_covers_exactly_the_pixels_of_the_edge_rule() {
        let mut rng = Rng::new(7);
        let clip = Rect { x0: -3, y0: -2, x1: 40, y1: 30 };
        let mut filled = 0;
        for n in 0..4000 {
            // Corners on the sub-pixel grid; one triangle in four is far
            // larger than the clip, one in four has whole-pixel corners.
            let reach = if n % 4 == 0 { 400 } else { 50 };
            let mut corner = || {
                let v = [rng.range_i32(-reach, reach) as i64 * SUB, rng.range_i32(-reach, reach) as i64 * SUB];
                if n % 4 == 1 { v } else { [v[0] / 37, v[1] / 37] }
            };
            let (a, b, c) = (corner(), corner(), corner());
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if area == 0 {
                continue;
            }
            let p = if area > 0 { [a, b, c] } else { [a, c, b] };
            let t = Tri { p, z: [0.25, 0.5, 0.75], plane: None, color: Rgba::WHITE, rank: 0 };
            let mut got = vec![0u8; (43 * 32) as usize];
            scan(&t, clip, |y, x0, x1, row| {
                assert!(x0 < x1 && clip.x0 <= x0 && x1 <= clip.x1 && (clip.y0..clip.y1).contains(&y));
                for x in x0..x1 {
                    let d = row.depth(x);
                    assert!((0.25..=0.75).contains(&d), "depth is between the corners': {d}");
                    got[((y + 2) * 43 + x + 3) as usize] += 1;
                }
            });
            for y in clip.y0..clip.y1 {
                for x in clip.x0..clip.x1 {
                    let want = u8::from(covers(&t, x as i64, y as i64));
                    assert_eq!(got[((y + 2) * 43 + x + 3) as usize], want, "triangle {n} {p:?} at ({x}, {y})");
                    filled += want as u32;
                }
            }
        }
        assert!(filled > 100_000, "{filled}");
    }

    #[test]
    fn a_pixel_has_one_depth_whatever_the_clip() {
        let t = Tri {
            p: [[10, 7], [9000, 1300], [2100, 7700]],
            z: [-0.25, 0.125, 0.5],
            plane: None,
            color: Rgba::WHITE,
            rank: 0,
        };
        let whole = Rect { x0: 0, y0: 0, x1: 40, y1: 32 };
        let mut n = 0;
        scan(&t, whole, |y, x0, x1, row| {
            for x in x0..x1 {
                n += 1;
                let mut alone = None;
                scan(&t, Rect { x0: x, y0: y, x1: x + 1, y1: y + 1 }, |_, _, _, r| alone = Some(r.depth(x)));
                assert_eq!(alone.map(f32::to_bits), Some(row.depth(x).to_bits()), "({x}, {y})");
            }
        });
        assert!(n > 300);
    }

    #[test]
    fn two_triangles_with_a_common_edge_share_its_pixels_out() {
        // A quad cut along either diagonal covers the same pixels, each once.
        let q = [[13i64, 40], [5200, 911], [6001, 4700], [-300, 3900]];
        let clip = Rect { x0: -4, y0: -2, x1: 28, y1: 22 };
        let count = |tris: [[usize; 3]; 2]| {
            let mut got = vec![0u8; 32 * 24];
            for i in tris {
                let t = Tri { p: i.map(|k| q[k]), z: [0.0; 3], plane: None, color: Rgba::WHITE, rank: 0 };
                scan(&t, clip, |y, x0, x1, _| (x0..x1).for_each(|x| got[((y + 2) * 32 + x + 4) as usize] += 1));
            }
            got
        };
        let one = count([[0, 1, 2], [0, 2, 3]]);
        assert!(one.iter().all(|n| *n <= 1) && one.iter().filter(|n| **n == 1).count() > 250);
        assert_eq!(one, count([[0, 1, 3], [1, 2, 3]]));
    }

    #[test]
    fn level_faces_in_one_plane_are_ordered_by_rank_and_the_rest_by_depth() {
        // Nearer wins.
        assert!(nearer(0.1, 0, 0.2, 0) && !nearer(0.2, 0, 0.1, 0));
        assert!(nearer(0.5, 0, f32::INFINITY, 0) && nearer(-0.5, 2, f32::INFINITY, 0));
        // Two level faces at one depth, give or take rounding: the rank decides, both ways round.
        let d = 0.123_456_7_f32;
        let next = f32::from_bits(d.to_bits() + 3);
        assert!(nearer(next, 2, d, 1) && !nearer(d, 1, next, 2));
        assert!(
            !nearer(d, 1, d, 1) && !nearer(next, 1, d, 1) && !nearer(d, 1, next, 1),
            "equal ranks: the first stays"
        );
        // A face that is not level is never "in the same plane".
        assert!(nearer(d, 0, next, 2) && !nearer(next, 0, d, 2));
        // Level faces a real step apart are ordered by depth, whatever their rank.
        assert!(nearer(0.1, 1, 0.1001, 2) && !nearer(0.1001, 2, 0.1, 1));
    }
}
