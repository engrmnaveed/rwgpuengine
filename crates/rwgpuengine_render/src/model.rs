//! Models: the solids a `[[shape]]` list is drawn as.
//!
//! A [`Model`] is a shape list in a footprint, turned a number of quarter
//! turns. [`bake`] makes it into flat faces: where their corners are from
//! the corner of the footprint, which way each face looks, and its color
//! under the light. Both renderers draw exactly these faces: the reference
//! renderer projects their corners, and the GPU renderer loads them as a
//! mesh and draws every entity of the model as an instance of it. Nothing
//! about a shape is decided anywhere else.
//!
//! Every number here comes from exact operations in a fixed order (ADR
//! 0005): corners are single-precision sums and products of what the file
//! says, a face's normal is the cross product of two of its edges divided by
//! its length (one square root), and the round shapes take their corners
//! from the engine's own sine and cosine. So a face has the same color on
//! every platform, to the bit.

use std::ops::Range;
use std::sync::Arc;

use rwgpuengine_core::color::Rgba;
use rwgpuengine_core::math::sin_cos_deg64;
use rwgpuengine_data::{ShapeDef, ShapeKind};

use crate::list::Light;
use crate::reference::shade;

/// The sides a cylinder and a cone are drawn with (ADR 0029 has the
/// measurement). A multiple of four, so that a quarter turn of a round
/// shape is the same shape: the engine's sine and cosine are exact at
/// quarter turns and mirror each other in between.
pub const ROUND_SIDES: usize = 16;

/// What a shape list is drawn as: its shapes, in the cells its entity
/// covers, turned `turn` quarter turns from +x toward +y about the middle of
/// those cells.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub shapes: Arc<[ShapeDef]>,
    /// The cells covered along x and y, as the entity stands (turned).
    pub footprint: [i32; 2],
    pub turn: u8,
}

impl Model {
    /// How high the model reaches above the entity's own height.
    pub fn height(&self) -> f32 {
        self.shapes.iter().map(|s| s.at[2] + s.size[2]).fold(0.0, f32::max)
    }
}

/// One flat face of a baked model.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    /// Its corners are `Baked::corners[corners]`, counter-clockwise seen from
    /// outside (clockwise on the screen, where y runs down).
    pub corners: Range<u32>,
    /// Outward, of length 1.
    pub normal: [f64; 3],
    /// Its color under the light.
    pub color: Rgba,
    /// For a face that looks straight up or down: its height above the
    /// instance's `pos`. Such a face is drawn by its plane and has a rank.
    pub level: Option<f32>,
}

/// The faces of one shape of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Solid {
    pub kind: ShapeKind,
    /// Its part of [`Baked::points`] and of [`Baked::faces`].
    pub points: Range<u32>,
    pub faces: Range<u32>,
}

/// A model as flat faces.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Baked {
    /// Corner positions, from the instance's `pos` (the corner of the
    /// footprint with the lowest x and y, at the entity's height).
    pub points: Vec<[f32; 3]>,
    /// The corners of every face, one face after the other, as places in
    /// `points`.
    pub corners: Vec<u32>,
    pub faces: Vec<Face>,
    /// One per shape, in the order of the list.
    pub solids: Vec<Solid>,
}

/// The corners of the shapes with flat sides, each in its own box (every
/// coordinate from 0 to 1): the eight of a box, numbered as the reference
/// renderer numbers them (x, then y, then z), and the two ends of a roof's
/// ridge, which runs along x above the middle.
const CORNERS: [[f32; 3]; 10] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [1.0, 0.0, 1.0],
    [0.0, 1.0, 1.0],
    [1.0, 1.0, 1.0],
    [0.0, 0.5, 1.0],
    [1.0, 0.5, 1.0],
];

/// The faces of a shape with flat sides at `turn = 0`, as corners of
/// [`CORNERS`], counter-clockwise seen from outside.
fn flat_faces(kind: ShapeKind) -> &'static [&'static [u8]] {
    match kind {
        // The six of the reference renderer's box, in its order.
        ShapeKind::Box => &[&[1, 3, 7, 5], &[0, 4, 6, 2], &[2, 6, 7, 3], &[0, 1, 5, 4], &[4, 5, 7, 6], &[0, 2, 3, 1]],
        // The slope that looks toward -y, the one toward +y, the gable ends at -x and +x, the floor.
        ShapeKind::Roof => &[&[0, 1, 9, 8], &[3, 2, 8, 9], &[0, 8, 2], &[1, 3, 9], &[0, 2, 3, 1]],
        // It rises toward +x: the slope, the high side, the flanks at -y and +y, the floor.
        _ => &[&[0, 5, 7, 2], &[1, 3, 7, 5], &[0, 1, 5], &[2, 7, 3], &[0, 2, 3, 1]],
    }
}

/// A quarter turn of a point of the rectangle `w` by `h`, from +x toward +y,
/// `turn` times: where it is in the rectangle as it then lies (`h` by `w`
/// after an odd number).
fn turned(p: [f32; 2], turn: u8, w: f32, h: f32) -> [f32; 2] {
    match turn % 4 {
        0 => p,
        1 => [h - p[1], p[0]],
        2 => [w - p[0], h - p[1]],
        _ => [p[1], w - p[0]],
    }
}

impl Baked {
    /// Add the face with these corners (places in `points`,
    /// counter-clockwise seen from outside), in `color` under `light`.
    #[inline(never)]
    fn face(&mut self, corners: &[u32], color: Rgba, light: &Light) {
        let at = |k: usize| self.points[corners[k] as usize].map(f64::from);
        let (a, b, c) = (at(0), at(1), at(2));
        let (e, f) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [e[1] * f[2] - e[2] * f[1], e[2] * f[0] - e[0] * f[2], e[0] * f[1] - e[1] * f[0]];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        // A face with no area (a size too small to hold next to its place) is not drawn.
        if length == 0.0 || !length.is_finite() {
            return;
        }
        let normal = n.map(|v| v / length);
        // Level: every corner at one height.
        let height = a[2] as f32;
        let level = corners.iter().all(|c| self.points[*c as usize][2] == height);
        let start = self.corners.len() as u32;
        self.corners.extend_from_slice(corners);
        self.faces.push(Face {
            corners: start..self.corners.len() as u32,
            normal,
            color: shade(color, light.shade(normal.map(|v| v as f32))),
            level: level.then_some(height),
        });
    }
}

/// The model as flat faces, each with its color under `light`.
#[inline(never)]
pub fn bake(model: &Model, light: &Light) -> Baked {
    let mut out = Baked::default();
    // The footprint before the model was turned.
    let [fw, fh] = model.footprint.map(|v| v as f32);
    let (w, h) = if model.turn.is_multiple_of(2) { (fw, fh) } else { (fh, fw) };
    const N: u32 = ROUND_SIDES as u32;
    // The corners of one shape in its own box (every coordinate from 0 to 1).
    let mut unit: Vec<[f32; 3]> = Vec::new();
    for s in model.shapes.iter() {
        let first = out.points.len() as u32;
        let first_face = out.faces.len() as u32;
        let round = matches!(s.kind, ShapeKind::Cylinder | ShapeKind::Cone);
        let top = s.kind == ShapeKind::Cylinder;
        // Where each of `CORNERS` is among the shape's corners.
        let mut places = [u32::MAX; CORNERS.len()];
        unit.clear();
        if round {
            // The base: N corners on the circle that touches the four sides of the box,
            // counter-clockwise from the middle of the +x side. Then the top, or the point.
            for i in 0..if top { 2 * N } else { N } {
                let (sin, cos) = sin_cos_deg64((i % N) as f64 * 360.0 / N as f64);
                unit.push([(0.5 + 0.5 * cos) as f32, (0.5 + 0.5 * sin) as f32, (i / N) as f32]);
            }
            if !top {
                unit.push([0.5, 0.5, 1.0]);
            }
        } else {
            // Only the corners its faces use, in the order first used.
            for c in flat_faces(s.kind).iter().flat_map(|f| f.iter()) {
                if places[*c as usize] == u32::MAX {
                    places[*c as usize] = first + unit.len() as u32;
                    unit.push(CORNERS[*c as usize]);
                }
            }
        }
        // In its box as the list places it (a round shape is the same at
        // every turn), then with the whole model.
        out.points.extend(unit.iter().map(|u| {
            let [u0, u1] = turned([u[0], u[1]], if round { 0 } else { s.turn }, 1.0, 1.0);
            let [x, y] = turned([s.at[0] + s.size[0] * u0, s.at[1] + s.size[1] * u1], model.turn, w, h);
            [x, y, s.at[2] + s.size[2] * u[2]]
        }));
        if round {
            for i in 0..N {
                let next = (i + 1) % N;
                let side = if top { [i, next, N + next, N + i] } else { [i, next, N, N] };
                out.face(&side.map(|c| first + c)[..if top { 4 } else { 3 }], s.color, light);
            }
            // The level faces: a cylinder's top looks up, the base looks down.
            for up in [true, false] {
                if up && !top {
                    continue;
                }
                let ring: [u32; ROUND_SIDES] =
                    std::array::from_fn(|i| first + if up { N + i as u32 } else { N - 1 - i as u32 });
                out.face(&ring, s.color, light);
            }
        } else {
            for face in flat_faces(s.kind) {
                let mut corners = [0; 4];
                for (k, c) in face.iter().enumerate() {
                    corners[k] = places[*c as usize];
                }
                out.face(&corners[..face.len()], s.color, light);
            }
        }
        out.solids.push(Solid {
            kind: s.kind,
            points: first..out.points.len() as u32,
            faces: first_face..out.faces.len() as u32,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn shape(kind: ShapeKind, at: [f32; 3], size: [f32; 3], turn: u8) -> ShapeDef {
        ShapeDef { kind, at, size, color: Rgba::rgb(200, 100, 50), turn }
    }

    fn model(shapes: Vec<ShapeDef>, footprint: [i32; 2], turn: u8) -> Model {
        Model { shapes: shapes.into(), footprint, turn }
    }

    fn corners<'a>(b: &'a Baked, f: &Face) -> impl Iterator<Item = [f32; 3]> + 'a {
        b.corners[f.corners.start as usize..f.corners.end as usize].iter().map(|c| b.points[*c as usize])
    }

    #[test]
    fn every_shape_is_closed_and_its_faces_look_outward() {
        for kind in ShapeKind::ALL {
            for own in 0..4 {
                for turn in 0..4 {
                    // A box of its own in a footprint of 3 by 2 cells (2 by 3 when turned).
                    let footprint = if turn % 2 == 0 { [3, 2] } else { [2, 3] };
                    let s = shape(kind, [0.5, 0.25, 0.5], [2.0, 1.5, 1.25], own);
                    let b = bake(&model(vec![s], footprint, turn), &Light::DEFAULT);
                    let what = format!("{kind:?} turned {own}, model turned {turn}");
                    // Every edge is walked once each way: the solid has no hole.
                    let mut edges: BTreeMap<(u32, u32), i32> = BTreeMap::new();
                    let middle: [f64; 3] = {
                        let n = b.points.len() as f64;
                        std::array::from_fn(|k| b.points.iter().map(|p| p[k] as f64).sum::<f64>() / n)
                    };
                    for f in &b.faces {
                        let ids = &b.corners[f.corners.start as usize..f.corners.end as usize];
                        assert!(ids.len() >= 3, "{what}");
                        for (i, a) in ids.iter().enumerate() {
                            let z = ids[(i + 1) % ids.len()];
                            *edges.entry((*a.min(&z), *a.max(&z))).or_insert(0) += if *a < z { 1 } else { -1 };
                        }
                        // The normal has length 1 and points away from the middle of the solid.
                        let length: f64 = f.normal.iter().map(|v| v * v).sum::<f64>().sqrt();
                        assert!((length - 1.0).abs() < 1e-12, "{what}: {:?}", f.normal);
                        for p in corners(&b, f) {
                            let out: f64 = (0..3).map(|k| (p[k] as f64 - middle[k]) * f.normal[k]).sum();
                            assert!(out > 0.0, "{what}: a face looks inward");
                        }
                        // Every corner of a face is in the face's plane.
                        let first = corners(&b, f).next().expect("a corner");
                        for p in corners(&b, f) {
                            let off: f64 = (0..3).map(|k| (p[k] as f64 - first[k] as f64) * f.normal[k]).sum();
                            assert!(off.abs() < 1e-6, "{what}: a face is not flat ({off})");
                        }
                        // A level face is exactly level, at the height it says.
                        if let Some(h) = f.level {
                            assert!(f.normal[0] == 0.0 && f.normal[1] == 0.0 && f.normal[2].abs() == 1.0, "{what}");
                            assert!(corners(&b, f).all(|p| p[2] == h), "{what}");
                        } else {
                            assert!(f.normal[2].abs() < 1.0, "{what}: a level face is not marked");
                        }
                    }
                    assert!(edges.values().all(|n| *n == 0), "{what}: an edge is open");
                    // It stays in its footprint, and in its box's height.
                    for p in &b.points {
                        assert!((0.0..=footprint[0] as f32).contains(&p[0]), "{what}: {p:?}");
                        assert!((0.0..=footprint[1] as f32).contains(&p[1]), "{what}: {p:?}");
                        assert!((0.5..=1.75).contains(&p[2]), "{what}: {p:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_box_has_the_faces_and_colors_of_a_block() {
        let light = Light::DEFAULT;
        let s = shape(ShapeKind::Box, [0.25, 0.5, 0.0], [1.5, 1.0, 2.0], 0);
        let b = bake(&model(vec![s], [2, 2], 0), &light);
        assert_eq!((b.faces.len(), b.points.len(), b.solids.len()), (6, 8, 1));
        for (f, (normal, _)) in b.faces.iter().zip(crate::reference::BOX_FACES) {
            assert_eq!(f.normal, normal.map(f64::from));
            assert_eq!(f.color, shade(s.color, light.shade(normal)));
            assert_eq!(f.level.is_some(), normal[2] != 0.0);
        }
        assert_eq!(b.faces[4].level, Some(2.0));
        assert_eq!(b.faces[5].level, Some(0.0));
    }

    #[test]
    fn turns_put_ridges_and_slopes_where_the_format_says() {
        let light = Light::DEFAULT;
        // The highest points of a shape of 2 by 1 cells, 1 high.
        let top = |kind: ShapeKind, own: u8| {
            let b = bake(&model(vec![shape(kind, [0.0; 3], [2.0, 1.0, 1.0], own)], [2, 1], 0), &light);
            let mut high: Vec<[f32; 2]> = b.points.iter().filter(|p| p[2] == 1.0).map(|p| [p[0], p[1]]).collect();
            high.sort_by(|a, b| a.partial_cmp(b).expect("numbers"));
            high
        };
        // A roof's ridge runs along x, and along y after a quarter turn.
        assert_eq!(top(ShapeKind::Roof, 0), [[0.0, 0.5], [2.0, 0.5]]);
        assert_eq!(top(ShapeKind::Roof, 1), [[1.0, 0.0], [1.0, 1.0]]);
        assert_eq!(top(ShapeKind::Roof, 2), top(ShapeKind::Roof, 0));
        // A wedge rises toward +x, then +y, -x and -y.
        assert_eq!(top(ShapeKind::Wedge, 0), [[2.0, 0.0], [2.0, 1.0]]);
        assert_eq!(top(ShapeKind::Wedge, 1), [[0.0, 1.0], [2.0, 1.0]]);
        assert_eq!(top(ShapeKind::Wedge, 2), [[0.0, 0.0], [0.0, 1.0]]);
        assert_eq!(top(ShapeKind::Wedge, 3), [[0.0, 0.0], [2.0, 0.0]]);

        // The whole model turns about the middle of its footprint, from +x toward +y: a
        // tower in the +x, low-y corner of a 3 by 2 footprint goes round its four corners.
        let tower = shape(ShapeKind::Box, [2.0, 0.0, 0.0], [1.0, 1.0, 3.0], 0);
        let corner = |turn: u8, footprint: [i32; 2]| {
            let b = bake(&model(vec![tower], footprint, turn), &light);
            let lo = |k: usize| b.points.iter().map(|p| p[k]).fold(f32::INFINITY, f32::min);
            [lo(0), lo(1)]
        };
        assert_eq!(corner(0, [3, 2]), [2.0, 0.0]);
        assert_eq!(corner(1, [2, 3]), [1.0, 2.0]);
        assert_eq!(corner(2, [3, 2]), [0.0, 1.0]);
        assert_eq!(corner(3, [2, 3]), [0.0, 0.0]);
    }

    #[test]
    fn a_face_is_lit_by_its_own_normal_and_its_tone_is_pinned() {
        let light = Light::DEFAULT;
        let white = |kind: ShapeKind, size: [f32; 3], own: u8| {
            let s = ShapeDef { kind, at: [0.0; 3], size, color: Rgba::WHITE, turn: own };
            bake(&model(vec![s], [2, 2], 0), &light)
        };
        // A roof 2 wide and 1 high has slopes at 45 degrees. The one that looks toward +y
        // (the lit side) is as bright as a top, the other is between a top and a -y side.
        let roof = white(ShapeKind::Roof, [2.0, 2.0, 1.0], 0);
        let tones: Vec<u8> = roof.faces.iter().map(|f| f.color.r).collect();
        assert_eq!(tones, [183, 255, 128, 166, 38], "-y slope, +y slope, -x gable, +x gable, bottom");
        // Turned a quarter, its slopes look toward +x and -x.
        let tones: Vec<u8> = white(ShapeKind::Roof, [2.0, 2.0, 1.0], 1).faces.iter().map(|f| f.color.r).collect();
        assert_eq!(tones, [237, 210, 89, 204, 38], "+x slope, -x slope, -y gable, +y gable, bottom");
        // A steeper roof is darker on its far side, a flatter one lighter: the tone follows the slope.
        let far = |height: f32| white(ShapeKind::Roof, [2.0, 2.0, height], 0).faces[0].color.r;
        assert!(far(2.0) < far(1.0) && far(1.0) < far(0.5), "{} {} {}", far(2.0), far(1.0), far(0.5));
        // A cylinder's sides go round from the tone of a +x side through +y, -x and -y.
        let side = |i: usize| white(ShapeKind::Cylinder, [1.0, 1.0, 1.0], 0).faces[i].color.r;
        let n = ROUND_SIDES;
        assert!(side(n / 4 - 1) > side(0) && side(n / 2) < side(0) && side(3 * n / 4) < side(n / 2));
        assert_eq!((side(n), side(n + 1)), (255, 38), "its top and its bottom");
        // The same list gives the same faces, to the bit.
        assert_eq!(white(ShapeKind::Cone, [1.0, 1.0, 2.0], 0), white(ShapeKind::Cone, [1.0, 1.0, 2.0], 3));
    }
}
