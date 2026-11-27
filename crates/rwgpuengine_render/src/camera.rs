//! The camera, and the projection from the world to the screen.
//!
//! A [`Camera`] is part of a session's state: a player turns, zooms and pans
//! it, playtests replay those moves, and what a tap hits depends on it. A
//! [`Projector`] is the camera's math for one moment: a 4 × 4 matrix that
//! takes a world point to a pixel and a depth.
//!
//! Two views share the same center, turn and zoom:
//!
//! - **iso**: the isometric view of `[iso]`. With no turn and zoom 1 it gives
//!   exactly the pixels of the formula in [`view`](crate::view), so scenes
//!   and taps written for the 2.5D engine stay valid.
//! - **perspective**: a camera 30° above the horizon with a 45° vertical
//!   field of view, placed so that the center is drawn at the iso view's size.
//!
//! All of it is exact float operations plus the owned sine and cosine of
//! [`rwgpuengine_core::math`], so every platform projects to the same bits.

use rwgpuengine_core::math::{COS_30, SIN_30, SQRT_2, TAN_22_5, sin_cos_deg64};
use rwgpuengine_data::{CameraMove, CameraView, SceneDef};
use rwgpuengine_sim::World;

use crate::view::{View, start_center};

pub const ZOOM_MIN: f32 = 0.125;
pub const ZOOM_MAX: f32 = 16.0;

/// Sub-pixel steps per pixel: projected corners snap to this grid.
pub(crate) const SUB: f64 = 256.0;
/// The iso view draws what lies within this many depth units of its center,
/// in front and behind (a world unit is between 1.4 and 1.8 of them).
const ISO_RANGE: f64 = 4096.0;
/// How far outside the frame, in pixels, geometry is kept before it is cut.
const GUARD: f64 = 4096.0;
/// One step of a card's [`lift`](crate::Card::lift), in world units toward
/// the camera.
pub(crate) const LIFT: f64 = 1.0 / 1024.0;

/// Where the picture is taken from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub view: CameraView,
    /// Ground point [x, y] drawn at the view's pivot (the middle of the frame).
    pub center: [f32; 2],
    /// Degrees turned about the vertical line through `center`, in 0..360.
    pub turn: f32,
    pub zoom: f32,
}

impl Camera {
    /// The camera a scene starts with.
    pub fn new(scene: &SceneDef) -> Camera {
        let c = &scene.camera;
        Camera { view: c.view, center: start_center(scene), turn: wrap_degrees(c.turn), zoom: clamp_zoom(c.zoom) }
    }

    /// Apply one move: view, then turn, zoom and pan.
    pub fn apply(&mut self, m: &CameraMove) {
        if let Some(v) = m.view {
            self.view = v;
        }
        self.turn = wrap_degrees(self.turn + m.turn);
        self.zoom = clamp_zoom(self.zoom * m.zoom);
        self.center = [self.center[0] + m.pan[0], self.center[1] + m.pan[1]];
    }

    /// Apply one move, then keep the center inside `bound`: a pan that would
    /// take it past the edge stops at the edge (ADR 0030).
    pub fn apply_within(&mut self, m: &CameraMove, bound: &Bound) {
        self.apply(m);
        self.center = bound.clamp(self.center);
    }
}

/// Where the camera's center may go: a rectangle on the ground, fixed when a
/// session starts (ADR 0030). However a move arrives (a playtest's
/// `[[input]] camera`, a player's gesture), the center stays inside it, so
/// the middle of the frame always shows the scene and what a tap hits after
/// a pan replays the same everywhere.
///
/// - With a ground: its cells, edge to edge, from (−0.5, −0.5) to
///   (size_x − 0.5, size_y − 0.5).
/// - Without one: the cells of the world entities that take up space
///   ([`Entity::takes_space`](rwgpuengine_sim::Entity::takes_space)) when the
///   session starts.
/// - Either way it holds the point the scene's camera starts at, so the
///   scene's own camera, and the way back to it, are always inside.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bound {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl Bound {
    /// The bound of `world` as it is now, for a camera that starts at `start`.
    pub fn new(world: &World, start: [f32; 2]) -> Bound {
        let mut b = Bound { min: start, max: start };
        let mut take = |lo: [f32; 2], hi: [f32; 2]| {
            for i in 0..2 {
                b.min[i] = b.min[i].min(lo[i]);
                b.max[i] = b.max[i].max(hi[i]);
            }
        };
        match world.bounds {
            Some([w, h]) if w > 0 && h > 0 => take([-0.5, -0.5], [w as f32 - 0.5, h as f32 - 0.5]),
            _ => {
                for e in world.entities.iter().filter(|e| e.takes_space()) {
                    let ([x, y], [fw, fh]) = (e.cell(), e.footprint);
                    let lo = [x as f32 - 0.5, y as f32 - 0.5];
                    take(lo, [lo[0] + fw.max(1) as f32, lo[1] + fh.max(1) as f32]);
                }
            }
        }
        b
    }

    /// The point of the bound nearest `p` (`p` itself when it is inside). A
    /// coordinate that is not a number goes to the low edge.
    pub fn clamp(&self, p: [f32; 2]) -> [f32; 2] {
        [0, 1].map(|i| p[i].max(self.min[i]).min(self.max[i]))
    }

    pub fn contains(&self, p: [f32; 2]) -> bool {
        (0..2).all(|i| p[i] >= self.min[i] && p[i] <= self.max[i])
    }
}

fn wrap_degrees(d: f32) -> f32 {
    if !d.is_finite() {
        return 0.0;
    }
    let w = d - (d / 360.0).floor() * 360.0;
    // Rounding can land exactly on 360.
    if (0.0..360.0).contains(&w) { w } else { 0.0 }
}

fn clamp_zoom(z: f32) -> f32 {
    if z.is_finite() { z.clamp(ZOOM_MIN, ZOOM_MAX) } else { 1.0 }
}

/// Which side of things the camera sees.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Eye {
    /// Every line of sight runs along this world direction (toward the camera).
    Direction([f64; 3]),
    Point([f64; 3]),
}

/// A homogeneous point in pixel space: the pixel is (x / w, y / w) and the
/// depth is z / w, from −1 (nearest) upward; smaller is nearer. The iso view
/// uses −1..1 with its center at 0. The perspective view uses −near / w,
/// which runs from −1 toward 0: floats are densest near 0, so far things
/// keep their precision.
pub type Hom = [f64; 4];

/// The camera's math for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projector {
    /// Rows x, y, z, w: each is dotted with (wx, wy, wz, 1).
    m: [[f64; 4]; 4],
    eye: Eye,
    /// Perspective: the near distance (depth = −near / w).
    near: Option<f64>,
    /// Iso: how much depth one world unit toward the camera takes away.
    iso_depth_per_unit: f64,
    /// A sprite's size factor is this over w.
    card_k: f64,
    /// The level direction that points at the viewer, as a unit vector.
    toward: [f64; 2],
    /// Half-planes that bound what is drawn: `dot(plane, hom) >= 0` is inside.
    planes: [[f64; 4]; 6],
}

impl Projector {
    pub fn new(view: &View, cam: &Camera) -> Projector {
        let (sn, cs) = sin_cos_deg64(cam.turn as f64);
        let (cx, cy) = (cam.center[0] as f64, cam.center[1] as f64);
        let (px, py) = (view.pivot[0] as f64, view.pivot[1] as f64);
        let zoom = cam.zoom as f64;
        let half_w = view.tile_w as f64 / 2.0;
        let half_h = view.tile_h as f64 / 2.0;
        // The turned grid, written as rows over (wx, wy):
        // along = qx + qy runs toward the viewer, across = qx − qy to the right.
        let across = [cs - sn, -(sn + cs)];
        let along = [cs + sn, cs - sn];
        let dot2 = |r: [f64; 2]| r[0] * cx + r[1] * cy;

        let (w, h) = (view.width as f64, view.height as f64);
        // Nearest and farthest depth, then the guard band around the frame.
        let mut planes = [
            [0.0, 0.0, 1.0, 1.0],
            [0.0, 0.0, -1.0, 1.0],
            [1.0, 0.0, 0.0, GUARD],
            [-1.0, 0.0, 0.0, w + GUARD],
            [0.0, 1.0, 0.0, GUARD],
            [0.0, -1.0, 0.0, h + GUARD],
        ];

        match cam.view {
            CameraView::Iso => {
                let (hw, hh, zh) = (half_w * zoom, half_h * zoom, view.z_h as f64 * zoom);
                let x_row = [across[0] * hw, across[1] * hw, 0.0];
                let y_row = [along[0] * hh, along[1] * hh, -zh];
                // The pivot is snapped to the sub-pixel grid, so that in the
                // scene's starting view the offsets are the whole pixels the
                // 2.5D formula uses, whatever rounding the center brought in.
                let x0 = snap(px - (x_row[0] * cx + x_row[1] * cy));
                let y0 = snap(py - (y_row[0] * cx + y_row[1] * cy));
                // Lines of sight run along (1, 1, k) in the turned grid.
                let k = 2.0 * half_h / view.z_h as f64;
                let s = 1.0 / ISO_RANGE;
                let m = [
                    [x_row[0], x_row[1], x_row[2], x0],
                    [y_row[0], y_row[1], y_row[2], y0],
                    [-along[0] * s, -along[1] * s, -k * s, dot2(along) * s],
                    [0.0, 0.0, 0.0, 1.0],
                ];
                Projector {
                    m,
                    eye: Eye::Direction([along[0], along[1], k]),
                    near: None,
                    iso_depth_per_unit: (2.0 + k * k).sqrt() * s,
                    card_k: zoom,
                    toward: [along[0] / SQRT_2, along[1] / SQRT_2],
                    planes,
                }
            }
            CameraView::Perspective => {
                let focal = h / 2.0 / TAN_22_5;
                let dist = focal / (SQRT_2 * half_w * zoom);
                let (near, far) = (dist / 16.0, dist * 64.0);
                // View axes over (wx, wy, wz, 1): right, up, and w = distance in front of the eye.
                let r = [across[0] / SQRT_2, across[1] / SQRT_2];
                let t = [along[0] / SQRT_2, along[1] / SQRT_2];
                let right = [r[0], r[1], 0.0, -dot2(r)];
                let up = [-SIN_30 * t[0], -SIN_30 * t[1], COS_30, SIN_30 * dot2(t)];
                let w_row = [-COS_30 * t[0], -COS_30 * t[1], -SIN_30, dist + COS_30 * dot2(t)];
                let mut m = [[0.0; 4]; 4];
                for i in 0..4 {
                    m[0][i] = px * w_row[i] + focal * right[i];
                    m[1][i] = py * w_row[i] - focal * up[i];
                    m[3][i] = w_row[i];
                }
                m[2][3] = -near;
                planes[1] = [0.0, 0.0, -1.0, -near / far];
                let reach = dist * COS_30 / SQRT_2;
                Projector {
                    m,
                    eye: Eye::Point([cx + reach * along[0], cy + reach * along[1], dist * SIN_30]),
                    near: Some(near),
                    iso_depth_per_unit: 0.0,
                    card_k: focal / (SQRT_2 * half_w),
                    toward: t,
                    planes,
                }
            }
        }
    }

    /// The matrix: rows x, y, z and w, each dotted with (wx, wy, wz, 1). A
    /// GPU renderer loads it, so both renderers share one projection.
    pub fn rows(&self) -> &[[f64; 4]; 4] {
        &self.m
    }

    /// The depths that are drawn: from −1 (nearest) to this one (farthest).
    pub fn far_depth(&self) -> f64 {
        self.planes[1][3]
    }

    /// The depth over the screen of every plane that is square to world axis
    /// `axis` (0, 1 or 2 for x, y and z), for this camera. Axis 2 gives the
    /// level planes.
    pub fn face_planes(&self, axis: usize) -> FacePlanes {
        // Columns of the matrix, as (x, y, w): what a step along each world
        // axis adds, and the constant. `u` and `v` are the two axes in the plane.
        let m = &self.m;
        let col = |j: usize| [m[0][j], m[1][j], m[3][j]];
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let (cu, cv, cw, c3) = (col(u), col(v), col(axis), col(3));
        let n = cross(cu, cv);
        // The two axes fall on one line of the screen: every such plane is seen edge-on.
        if dot3(n, n) <= 1.0e-6 * dot3(cu, cu) * dot3(cv, cv) {
            return FacePlanes { a: [0.0; 3], b: [0.0; 3], alpha: 0.0, beta: 0.0 };
        }
        let z = m[2];
        let mix = |c: [f64; 3], k: f64| {
            let (p, q) = (cross(cv, c), cross(c, cu));
            std::array::from_fn(|i| z[u] * p[i] + z[v] * q[i] + k * n[i])
        };
        FacePlanes { a: mix(cw, z[axis]), b: mix(c3, z[3]), alpha: dot3(cw, n), beta: dot3(c3, n) }
    }

    /// The depth of every level plane over the screen, for this camera.
    pub fn level_planes(&self) -> FacePlanes {
        self.face_planes(2)
    }

    /// Project a world point, drawn `lift` steps nearer the camera than it is
    /// (its pixel does not move; only its depth does).
    pub fn hom(&self, p: [f64; 3], lift: u8) -> Hom {
        let row = |r: &[f64; 4]| r[0] * p[0] + r[1] * p[1] + r[2] * p[2] + r[3];
        let (x, y, mut z, w) = (row(&self.m[0]), row(&self.m[1]), row(&self.m[2]), row(&self.m[3]));
        if lift > 0 {
            let d = lift as f64 * LIFT;
            match self.near {
                None => z -= d * self.iso_depth_per_unit,
                Some(near) if w - d > 0.0 => z = -near / (w - d) * w,
                Some(_) => {}
            }
        }
        [x, y, z, w]
    }

    /// Is `h` inside every bounding half-plane?
    pub fn inside(&self, h: &Hom) -> bool {
        self.planes.iter().all(|p| dot4(p, h) >= 0.0)
    }

    pub(crate) fn planes(&self) -> &[[f64; 4]; 6] {
        &self.planes
    }

    /// Does a flat face through `p` with outward normal `n` show its front?
    pub(crate) fn faces_camera(&self, p: [f64; 3], n: [f64; 3]) -> bool {
        let to_eye = match self.eye {
            Eye::Direction(d) => d,
            Eye::Point(e) => [e[0] - p[0], e[1] - p[1], e[2] - p[2]],
        };
        n[0] * to_eye[0] + n[1] * to_eye[1] + n[2] * to_eye[2] > 0.0
    }

    /// Where a card standing at `anchor` is drawn and how deep it is, or
    /// `None` if the anchor is out of the camera's range.
    ///
    /// A card is flat and faces the camera, but it has a place in the world:
    /// above its anchor it stands upright, so higher rows are nearer a camera
    /// that looks down; below its anchor it lies on the level surface it
    /// stands on, toward the viewer. So a tall sprite is not cut by a wall
    /// behind it, and a sprite nudged down a few pixels is not cut by the
    /// floor in front of it, while anything really in front still covers it.
    ///
    /// The part that lies is in the floor's own plane. The reference
    /// renderer gives it that plane's exact depth ([`FacePlanes`]) and lets it
    /// win against a floor there; `down` is the same plane as a slope, for a
    /// card that is lifted off it.
    pub fn card(&self, anchor: [f64; 3], lift: u8) -> Option<CardPlace> {
        let h = self.hom(anchor, lift);
        if !self.inside(&h) {
            return None;
        }
        let (y, at) = (h[1] / h[3], h[2] / h[3]);
        // Depth is a straight line in screen y along each of the two planes,
        // so one more point on each gives its slope. The step is short enough
        // to stay in front of a perspective camera.
        let step = h[3].min(1.0);
        let slope = |p: [f64; 3]| {
            let q = self.hom(p, lift);
            let dy = q[1] / q[3] - y;
            if dy == 0.0 { 0.0 } else { (q[2] / q[3] - at) / dy }
        };
        Some(CardPlace {
            x: h[0] / h[3],
            y,
            scale: self.card_k / h[3],
            at,
            up: slope([anchor[0], anchor[1], anchor[2] + step]),
            down: slope([anchor[0] + self.toward[0] * step, anchor[1] + self.toward[1] * step, anchor[2]]),
        })
    }

    /// The pixel a world point is drawn at, or `None` behind the camera.
    pub fn screen(&self, p: [f32; 3]) -> Option<[f32; 2]> {
        let h = self.hom([p[0] as f64, p[1] as f64, p[2] as f64], 0);
        (h[3] > 0.0).then(|| [(h[0] / h[3]) as f32, (h[1] / h[3]) as f32])
    }
}

/// The depth, over the screen, of the planes that are square to one world
/// axis: the level planes, or the planes the sides of boxes are in.
///
/// A point of the plane at `t` along that axis that is drawn at pixel-space
/// point (x, y) has the depth
/// `dot(a * t + b, (x, y, 1)) / (alpha * t + beta)`. It follows from the
/// projector's matrix alone, so it holds for both views.
///
/// Every level face takes its depth from here, not from its own corners.
/// Corners are snapped to the sub-pixel grid, and a depth fitted to snapped
/// corners is off by more than rounding. From the plane, two faces at one
/// height have the same depth in every pixel, bit for bit, whatever their
/// corners are, and their rank can decide between them.
///
/// The sides of boxes do not take theirs from here yet: that would change
/// one pixel of a reference image (ADR 0021, "what stays undefined").
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FacePlanes {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub alpha: f64,
    pub beta: f64,
}

impl FacePlanes {
    /// The depth of the plane at `t` at pixel-space point (x, y).
    pub fn depth(&self, x: f64, y: f64, t: f64) -> f64 {
        let v: [f64; 3] = std::array::from_fn(|i| self.a[i] * t + self.b[i]);
        dot3(v, [x, y, 1.0]) / (self.alpha * t + self.beta)
    }

    /// The plane at `t` as a function of the pixel: the depth at the center
    /// of pixel (px, py) is `c[0] * px + (c[1] * py + c[2])`.
    ///
    /// `t` is a single-precision number on purpose: it is what scene files
    /// hold, so a tile at 1.2 and the top of a box from 0.4 to 1.2 are one
    /// plane. `None` if the camera sees the plane edge-on, or within a
    /// thousandth of it: such a plane covers a row of pixels at most, and
    /// its equation is steeper than the pixel grid can place.
    pub fn over_pixels(&self, t: f32) -> Option<[f64; 3]> {
        let t = t as f64;
        let den = self.alpha * t + self.beta;
        if den.abs() <= 1.0e-3 * (self.alpha * t).abs().max(self.beta.abs()) {
            return None;
        }
        let v: [f64; 3] = std::array::from_fn(|i| (self.a[i] * t + self.b[i]) / den);
        Some([v[0], v[1], v[0] * 0.5 + v[1] * 0.5 + v[2]])
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Where a card is on the screen, and its depth row by row.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CardPlace {
    /// The pixel its anchor is drawn at.
    pub x: f64,
    pub y: f64,
    /// Size factor of a sprite standing there.
    pub scale: f64,
    /// Depth at the anchor's row, and its change per pixel above and below.
    pub at: f64,
    pub up: f64,
    pub down: f64,
}

impl CardPlace {
    /// Is pixel row `py` at or below the anchor, where the card lies on the
    /// level surface it stands on?
    pub fn lies(&self, py: i32) -> bool {
        py as f64 + 0.5 - self.y >= 0.0
    }

    /// The card's depth in pixel row `py`.
    pub fn depth(&self, py: i32) -> f32 {
        let dy = py as f64 + 0.5 - self.y;
        (self.at + dy * if dy < 0.0 { self.up } else { self.down }) as f32
    }
}

pub(crate) fn dot4(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}

/// Round to the sub-pixel grid.
fn snap(v: f64) -> f64 {
    (v * SUB + 0.5).floor() / SUB
}

/// A projected coordinate as a whole number of sub-pixel steps (1/256 of a
/// pixel): where the reference renderer puts a corner.
pub fn to_fixed(v: f64) -> i64 {
    (v * SUB + 0.5).floor() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use rwgpuengine_core::color::Rgba;

    fn view() -> View {
        View {
            width: 640,
            height: 480,
            background: Rgba::BLACK,
            tile_w: 64,
            tile_h: 32,
            z_h: 32,
            pivot: [320.0, 240.0],
        }
    }

    fn cam(view: CameraView, turn: f32, zoom: f32) -> Camera {
        Camera { view, center: [4.5, 3.5], turn, zoom }
    }

    #[test]
    fn the_starting_iso_view_is_the_two_and_a_half_d_formula() {
        let v = view();
        let p = Projector::new(&v, &cam(CameraView::Iso, 0.0, 1.0));
        // origin = pivot - A * center = (288, 112).
        for (x, y, z) in
            [(0.0f32, 0.0f32, 0.0f32), (4.5, 3.5, 0.0), (9.5, -0.5, 1.0), (2.0, 7.5, 0.5), (-3.0, 12.0, 2.25)]
        {
            let want = [288.0 + (x - y) * 32.0, 112.0 + (x + y) * 16.0 - z * 32.0];
            assert_eq!(p.screen([x, y, z]), Some(want), "({x}, {y}, {z})");
        }
    }

    #[test]
    fn a_center_with_rounding_error_still_gives_whole_pixels() {
        // The 2.5D engine rounded the origin to a whole pixel; so must this.
        let mut v = view();
        let c = [3.3f32, 2.1f32];
        let origin = [(320.0 - (c[0] - c[1]) * 32.0 + 0.5).floor(), (240.0 - (c[0] + c[1]) * 16.0 + 0.5).floor()];
        v.pivot = [origin[0] + (c[0] - c[1]) * 32.0, origin[1] + (c[0] + c[1]) * 16.0];
        let p = Projector::new(&v, &Camera { view: CameraView::Iso, center: c, turn: 0.0, zoom: 1.0 });
        assert_eq!(p.screen([0.0, 0.0, 0.0]), Some(origin));
        assert_eq!(p.screen([5.0, 2.0, 1.0]), Some([origin[0] + 96.0, origin[1] + 112.0 - 32.0]));
    }

    #[test]
    fn turning_keeps_the_center_in_place_and_quarter_turns_are_exact() {
        let v = view();
        for view_kind in [CameraView::Iso, CameraView::Perspective] {
            for turn in [0.0, 37.0, 90.0, 180.0, 270.0] {
                let p = Projector::new(&v, &cam(view_kind, turn, 1.0));
                let [sx, sy] = p.screen([4.5, 3.5, 0.0]).unwrap();
                // The pivot is snapped to 1/256 of a pixel.
                assert!((sx - 320.0).abs() < 3e-3 && (sy - 240.0).abs() < 3e-3, "{view_kind:?} {turn}: {sx} {sy}");
            }
        }
        // A quarter turn: +x, which ran down-right, runs down-left.
        let p = Projector::new(&v, &cam(CameraView::Iso, 90.0, 1.0));
        assert_eq!(p.screen([5.5, 3.5, 0.0]), Some([288.0, 256.0]));
        assert_eq!(p.screen([4.5, 4.5, 0.0]), Some([288.0, 224.0]));
    }

    #[test]
    fn perspective_draws_the_center_at_the_iso_size_and_far_things_smaller() {
        let v = view();
        let iso = Projector::new(&v, &cam(CameraView::Iso, 0.0, 1.0));
        let per = Projector::new(&v, &cam(CameraView::Perspective, 0.0, 1.0));
        // Two points either side of the center, across the view.
        let width = |p: &Projector, at: [f32; 2]| {
            let a = p.screen([at[0] + 0.25, at[1] - 0.25, 0.0]).unwrap();
            let b = p.screen([at[0] - 0.25, at[1] + 0.25, 0.0]).unwrap();
            a[0] - b[0]
        };
        assert!((width(&iso, [4.5, 3.5]) - 32.0).abs() < 1e-3);
        assert!((width(&per, [4.5, 3.5]) - 32.0).abs() < 1e-2, "{}", width(&per, [4.5, 3.5]));
        // Away from the viewer is smaller, toward the viewer larger.
        assert!(width(&per, [0.5, -0.5]) < 28.0 && width(&per, [8.5, 7.5]) > 36.0);
        // Zoom scales both views about the center.
        let per2 = Projector::new(&v, &cam(CameraView::Perspective, 0.0, 2.0));
        assert!((width(&per2, [4.5, 3.5]) - 64.0).abs() < 2e-2);
    }

    #[test]
    fn depth_grows_away_from_the_camera_and_lift_brings_things_nearer() {
        let v = view();
        for view_kind in [CameraView::Iso, CameraView::Perspective] {
            for turn in [0.0, 45.0, 200.0] {
                let p = Projector::new(&v, &cam(view_kind, turn, 1.0));
                let depth = |h: Hom| h[2] / h[3];
                let ground = depth(p.hom([4.5, 3.5, 0.0], 0));
                let above = depth(p.hom([4.5, 3.5, 1.0], 0));
                assert!(above < ground, "{view_kind:?} {turn}: up is nearer");
                assert!((-1.0..1.0).contains(&ground) && (-1.0..1.0).contains(&above));
                // A lift is a small step toward the camera: less than a rise of 0.1.
                let lifted = depth(p.hom([4.5, 3.5, 0.0], 1));
                let raised = depth(p.hom([4.5, 3.5, 0.1], 0));
                assert!(lifted < ground && lifted > raised, "{view_kind:?} {turn}: {ground} {lifted} {raised}");
                assert!(depth(p.hom([4.5, 3.5, 0.0], 2)) < lifted);
                // A card: nearer going up its upright part, and nearer going
                // down the part that lies on the floor toward the viewer.
                let card = p.card([4.5, 3.5, 0.0], 0).unwrap();
                let row = card.y.floor() as i32;
                assert!(card.depth(row - 20) < card.depth(row - 2), "{view_kind:?} {turn}: up");
                assert!(card.depth(row + 20) < card.depth(row + 2), "{view_kind:?} {turn}: down");
                assert!((card.depth(row) as f64 - ground).abs() < 1e-4);
                assert!(p.card([4.5, 3.5, 0.0], 1).unwrap().depth(row - 7) < card.depth(row - 7));
                assert!(p.faces_camera([4.5, 3.5, 0.0], [0.0, 0.0, 1.0]));
                assert!(!p.faces_camera([4.5, 3.5, 0.0], [0.0, 0.0, -1.0]));
            }
        }
    }

    #[test]
    fn a_plane_has_the_depth_the_projector_gives_its_points() {
        // Planes square to each axis: at `t` along it, with two free coordinates.
        for kind in [CameraView::Iso, CameraView::Perspective] {
            for (turn, zoom) in [(0.0, 1.0), (33.0, 0.5), (217.0, 1.75), (295.0, 3.0)] {
                let proj = Projector::new(&view(), &cam(kind, turn, zoom));
                for axis in 0..3 {
                    let planes = proj.face_planes(axis);
                    for t in [0.0f32, 0.5, 1.2, -2.0, 3.0] {
                        let over = planes.over_pixels(t).expect("not edge-on");
                        for (p, q) in [(4.5, 3.5), (0.0, 0.0), (9.25, -1.5), (-3.0, 2.0), (4.0, 0.25)] {
                            let mut point = [0.0; 3];
                            point[axis] = t as f64;
                            point[(axis + 1) % 3] = p;
                            point[(axis + 2) % 3] = q;
                            let h = proj.hom(point, 0);
                            if h[3] <= 0.0 {
                                continue;
                            }
                            let (sx, sy, want) = (h[0] / h[3], h[1] / h[3], h[2] / h[3]);
                            let got = planes.depth(sx, sy, t as f64);
                            let what = format!("{kind:?} {turn} {zoom} axis {axis} at {t}: ({p}, {q})");
                            assert!((got - want).abs() < 1e-11, "{what}: {got} {want}");
                            // The same through the pixel whose center is that point.
                            let by_pixel = over[0] * (sx - 0.5) + (over[1] * (sy - 0.5) + over[2]);
                            assert!((by_pixel - want).abs() < 1e-11, "{what}");
                        }
                    }
                }
            }
        }
        let v = view();
        // A level plane at the eye's height is edge-on: no equation for it.
        let per = Projector::new(&v, &cam(CameraView::Perspective, 0.0, 1.0));
        let Eye::Point(eye) = per.eye else { panic!("perspective has an eye point") };
        assert_eq!(per.level_planes().over_pixels(eye[2] as f32), None);
        assert!(per.level_planes().over_pixels(eye[2] as f32 + 0.5).is_some());
        // Turned an eighth, the iso view looks along the sides that face y.
        let iso = Projector::new(&v, &cam(CameraView::Iso, 45.0, 1.0));
        assert_eq!(iso.face_planes(1).over_pixels(2.5), None);
        assert!(iso.face_planes(0).over_pixels(2.5).is_some() && iso.face_planes(2).over_pixels(2.5).is_some());
    }

    #[test]
    fn moves_wrap_and_clamp() {
        let mut c = cam(CameraView::Iso, 350.0, 1.0);
        c.apply(&CameraMove { view: None, turn: 20.0, zoom: 2.0, pan: [1.0, -2.0] });
        assert_eq!((c.turn, c.zoom, c.center), (10.0, 2.0, [5.5, 1.5]));
        c.apply(&CameraMove { view: Some(CameraView::Perspective), turn: -30.0, zoom: 100.0, pan: [0.0, 0.0] });
        assert_eq!((c.view, c.turn, c.zoom), (CameraView::Perspective, 340.0, ZOOM_MAX));
        c.apply(&CameraMove { view: None, turn: 0.0, zoom: 0.0001, pan: [0.0, 0.0] });
        assert_eq!(c.zoom, ZOOM_MIN);
    }

    #[test]
    fn a_bound_keeps_the_center_and_lets_through_what_is_inside() {
        let b = Bound { min: [-0.5, -0.5], max: [9.5, 7.5] };
        assert_eq!(b.clamp([4.5, 3.5]), [4.5, 3.5]);
        assert_eq!(b.clamp([30.0, -2.0]), [9.5, -0.5]);
        assert_eq!(b.clamp([f32::NAN, f32::INFINITY]), [-0.5, 7.5]);
        assert!(b.contains([9.5, -0.5]) && !b.contains([9.6, 0.0]));
        let mut c = cam(CameraView::Iso, 0.0, 1.0);
        c.apply_within(&CameraMove { view: None, turn: 90.0, zoom: 2.0, pan: [20.0, 1.0] }, &b);
        assert_eq!((c.turn, c.zoom, c.center), (90.0, 2.0, [9.5, 4.5]));
    }
}
