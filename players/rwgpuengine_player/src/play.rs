//! What every player does, with a window or without: step the engine's
//! session, build its render list, and draw it with the GPU only when it
//! changed.
//!
//! There is one simulation and one picture: `Session::step` with the live
//! input, `Session::render_list`, `Renderer::draw`. The player adds no rule
//! of its own; what it adds is time (the window's clock) and showing the
//! frame (scaled, with bars).

use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_data::{CameraMove, CameraView};
use rwgpuengine_gpu::{Options, Renderer, wgpu};
use rwgpuengine_render::{Camera, Projector, RenderList, View};
use rwgpuengine_runtime::{LiveInput, Session};

use crate::clock::{Timings, Watch};
use crate::gpu::{Gpu, failed};
use crate::input::{Does, Gesture, STILL};

/// A session drawn by the GPU renderer.
pub struct Player {
    pub session: Session,
    pub renderer: Renderer,
    /// The list in the renderer's frame now.
    drawn: Option<RenderList>,
    /// The list of the current state, when it differs from `drawn`.
    pending: Option<RenderList>,
    /// Close at this frame (`--frames`, or the playtest's).
    pub end: Option<u64>,
    /// Frames drawn into the renderer's frame.
    pub draws: u64,
    /// Building the list and comparing it with the one drawn, each time the
    /// state may have changed (µs).
    pub list_us: Timings,
    /// `Renderer::draw` (prepare and submit), each time it drew (µs).
    pub draw_us: Timings,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player").field("session", &self.session).field("draws", &self.draws).finish()
    }
}

impl Player {
    pub fn new(session: Session, device: &wgpu::Device, end: Option<u64>) -> Player {
        Player::with_renderer(session, Renderer::new(device, Options::default()), end)
    }

    /// A player that draws with a renderer made before, so its pipelines are
    /// not built again: for a page that restarts the game or loads new files.
    pub fn with_renderer(session: Session, renderer: Renderer, end: Option<u64>) -> Player {
        Player {
            session,
            renderer,
            drawn: None,
            pending: None,
            end,
            draws: 0,
            list_us: Timings::default(),
            draw_us: Timings::default(),
        }
    }

    /// Has the session reached the frame to close at?
    pub fn finished(&self) -> bool {
        self.end.is_some_and(|e| self.session.frame() >= e)
    }

    /// One fixed step with the live input, unless the session is finished.
    pub fn step(&mut self, live: &LiveInput) {
        if !self.finished() {
            self.session.step(live);
        }
    }

    /// Build the list of the current state. Returns true when it differs from
    /// the one drawn, so there is something new to draw.
    pub fn update(&mut self, watch: &Watch) -> bool {
        let t = watch.micros();
        let list = self.session.render_list(false);
        let changed = self.drawn.as_ref() != Some(&list);
        self.pending = changed.then_some(list);
        self.list_us.add(watch.micros() - t);
        changed
    }

    /// Draw the new list into the renderer's frame, if there is one.
    /// Returns true when it drew.
    pub fn draw(&mut self, gpu: &Gpu, watch: &Watch) -> bool {
        let Some(list) = self.pending.take() else { return false };
        let t = watch.micros();
        self.renderer.draw(&gpu.device, &gpu.queue, &list);
        self.draw_us.add(watch.micros() - t);
        self.drawn = Some(list);
        self.draws += 1;
        true
    }

    /// The frame size (the list's view).
    pub fn frame_size(&self) -> [u32; 2] {
        [self.session.view.width, self.session.view.height]
    }

    /// Move the camera, as a gesture asks.
    pub fn camera(&mut self, m: &CameraMove) {
        self.session.move_camera(m);
    }

    /// The move for [`Does::View`] and [`Does::Home`] ([`key_move`]).
    pub fn key_move(&self, does: Does) -> CameraMove {
        key_move(&self.session, does)
    }

    /// The pan of a drag by `d` frame pixels ([`pan_move`]).
    pub fn pan_move(&self, d: [f32; 2]) -> Option<CameraMove> {
        pan_move(&self.session, d)
    }

    /// Make the move a gesture asks for ([`gesture_move`]). Returns it.
    pub fn gesture(&mut self, g: Gesture) -> Option<CameraMove> {
        let m = gesture_move(&self.session, g)?;
        self.session.move_camera(&m);
        Some(m)
    }

    /// Without a window: step to the end as fast as the steps go, drawing
    /// each frame whose list changed, as a window would.
    pub fn run_headless(&mut self, gpu: &Gpu, watch: &Watch) -> Result<(), Diagnostic> {
        let idle = LiveInput::default();
        self.update(watch);
        self.draw(gpu, watch);
        while !self.finished() {
            self.step(&idle);
            if self.update(watch) {
                self.draw(gpu, watch);
                // Keep the GPU's queue short; nothing waits here.
                let _ = gpu.device.poll(wgpu::PollType::Poll);
            }
        }
        gpu.check()
    }

    /// The renderer's frame as RGBA8 bytes (the layout of the reference's
    /// `Frame::rgba`).
    pub fn frame_bytes(&self, gpu: &Gpu) -> Result<Vec<u8>, Diagnostic> {
        let copy = self
            .renderer
            .copy_frame(&gpu.device, &gpu.queue)
            .ok_or_else(|| failed("nothing was drawn, so there is no frame to copy".into()))?;
        gpu.wait()?;
        match copy.take() {
            Some(Ok(bytes)) => Ok(bytes),
            Some(Err(e)) => Err(failed(e)),
            None => Err(failed("the frame did not arrive from the GPU".into())),
        }
    }
}

/// The camera move a gesture asks for, in this session: what both the
/// window and the web page do with every [`Gesture`]. `None` when it asks
/// for nothing (a pan of a ground seen edge-on).
pub fn gesture_move(session: &Session, g: Gesture) -> Option<CameraMove> {
    match g {
        Gesture::Move(m) => Some(m),
        Gesture::Pan(d) => pan_move(session, d),
        Gesture::Grab { from, to, zoom, turn } => grab_move(session, from, to, zoom, turn),
        Gesture::Settle { quarters, turned } => Some(settle_move(session, quarters, turned)),
        Gesture::Key(does) => Some(key_move(session, does)),
    }
}

/// Does a drag pan in this game, rather than swipe? It does when nothing in
/// the scene, and no prefab it spawns, has a `mover`: a swipe would move
/// nothing there (actions drive only movers), and a builder's map is moved
/// by dragging it (ADR 0030).
pub fn drag_pans(session: &Session) -> bool {
    let w = &session.world;
    !w.entities.iter().any(|e| e.mover.is_some()) && !w.templates.iter().any(|t| t.mover.is_some())
}

/// The move for [`Does::View`] (the other view) and [`Does::Home`] (back to
/// the scene's camera, as one move); [`STILL`] for anything else.
pub fn key_move(session: &Session, does: Does) -> CameraMove {
    let now = session.camera;
    match does {
        Does::View => CameraMove {
            view: Some(match now.view {
                CameraView::Iso => CameraView::Perspective,
                CameraView::Perspective => CameraView::Iso,
            }),
            ..STILL
        },
        Does::Home => {
            let home = Camera::new(&session.scene);
            CameraMove {
                view: Some(home.view),
                turn: home.turn - now.turn,
                zoom: home.zoom / now.zoom,
                pan: [home.center[0] - now.center[0], home.center[1] - now.center[1]],
            }
        }
        _ => STILL,
    }
}

/// The move of a grab: turn by `turn` and zoom by `zoom`, then pan so that
/// the ground drawn at frame pixel `from` now is drawn at `to`. Panning moves
/// the camera with its center, so the pan is the ground point under `from`
/// before the move less the one under `to` once turned and zoomed
/// ([`ground_at`]). `None` when the ground is seen edge-on there, or behind
/// the camera.
pub fn grab_move(session: &Session, from: [f32; 2], to: [f32; 2], zoom: f32, turn: f32) -> Option<CameraMove> {
    let now = session.camera;
    let mut then = now;
    then.apply(&CameraMove { zoom, turn, ..STILL });
    let p = ground_at(&session.view, &now, from)?;
    let q = ground_at(&session.view, &then, to)?;
    let pan = [p[0] - q[0], p[1] - q[1]];
    pan.iter().all(|v| v.is_finite()).then_some(CameraMove { view: None, turn, zoom, pan })
}

/// The ground point drawn at frame pixel `px` under camera `cam`: the
/// directions of one cell along x and y at the camera's center give a first
/// answer, exact in the iso view, and corrections make it exact to a small
/// fraction of a pixel in perspective, where the ground point can be far
/// from the center. `None` when the ground is seen edge-on or the point is
/// behind the camera.
pub fn ground_at(view: &View, cam: &Camera, px: [f32; 2]) -> Option<[f32; 2]> {
    let projector = Projector::new(view, cam);
    let [cx, cy] = cam.center;
    let at = |v: [f32; 2]| projector.screen([cx + v[0], cy + v[1], 0.0]);
    let o = at([0.0, 0.0])?;
    let mut w = [0.0f32, 0.0];
    let mut s = o;
    for _ in 0..8 {
        // The ground's directions where the answer is now.
        let (x, y) = (at([w[0] + 1.0, w[1]])?, at([w[0], w[1] + 1.0])?);
        let (ax, ay) = ([x[0] - s[0], x[1] - s[1]], [y[0] - s[0], y[1] - s[1]]);
        let det = ax[0] * ay[1] - ay[0] * ax[1];
        if det.abs() < 1e-3 {
            return None;
        }
        let r = [px[0] - s[0], px[1] - s[1]];
        if r[0].abs() < 0.01 && r[1].abs() < 0.01 {
            break;
        }
        w = [w[0] + (r[0] * ay[1] - ay[0] * r[1]) / det, w[1] + (ax[0] * r[1] - r[0] * ax[1]) / det];
        s = at(w)?;
    }
    Some([cx + w[0], cy + w[1]])
}

/// The move that settles a twist on a quarter turn when the fingers let go:
/// `quarters` quarter turns from the quarter turn nearest the view's turn
/// before the twist (`turned` degrees ago). The move is the plain
/// difference, which lands on the quarter turn exactly (ADR 0030).
pub fn settle_move(session: &Session, quarters: i32, turned: f32) -> CameraMove {
    let now = session.camera.turn;
    let start = now - turned;
    let target = ((start / 90.0).round() + quarters as f32) * 90.0;
    let target = target - (target / 360.0).floor() * 360.0;
    CameraMove { turn: target - now, ..STILL }
}

/// The pan that makes the ground under the camera's center follow the
/// pointer by `d` frame pixels. Panning moves the whole picture with the
/// center, so this asks where on the ground the point `d` away from the
/// center's pixel is, and moves the center there and back: the directions of
/// one cell along x and y give a first answer, exact in the iso view, and two
/// corrections make it exact to a small fraction of a pixel in perspective.
/// `None` when the ground is seen edge-on.
pub fn pan_move(session: &Session, d: [f32; 2]) -> Option<CameraMove> {
    let [cx, cy] = session.camera.center;
    let at = |v: [f32; 2]| session.screen_point([cx + v[0], cy + v[1], 0.0]);
    let (o, x, y) = (at([0.0, 0.0])?, at([1.0, 0.0])?, at([0.0, 1.0])?);
    let (ax, ay) = ([x[0] - o[0], x[1] - o[1]], [y[0] - o[0], y[1] - o[1]]);
    let det = ax[0] * ay[1] - ay[0] * ax[1];
    if det.abs() < 1e-3 {
        return None;
    }
    let solve = |r: [f32; 2]| [(r[0] * ay[1] - ay[0] * r[1]) / det, (ax[0] * r[1] - r[0] * ax[1]) / det];
    let mut w = solve(d);
    for _ in 0..2 {
        let s = at(w)?;
        let dw = solve([o[0] + d[0] - s[0], o[1] + d[1] - s[1]]);
        w = [w[0] + dw[0], w[1] + dw[1]];
    }
    Some(CameraMove { pan: [-w[0], -w[1]], ..STILL })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rwgpuengine_data::Vfs;
    use rwgpuengine_runtime::{DataOnly, SessionConfig};

    fn example(name: &str) -> Session {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(name);
        let vfs = Vfs::from_dir(&dir).expect("example");
        Session::load(&vfs, &SessionConfig::default(), Box::new(DataOnly)).expect("loads")
    }

    fn town() -> Session {
        example("tiny_town")
    }

    /// A grab, applied, draws the ground point that was under `from` at `to`,
    /// turned and zoomed about it: under every camera of the engine. A twist
    /// clockwise on the screen turns the ground clockwise about the fingers.
    #[test]
    fn a_grab_keeps_the_ground_under_the_fingers() {
        let moves = [
            STILL,
            CameraMove { turn: 90.0, ..STILL },
            CameraMove { turn: 37.0, zoom: 1.5, ..STILL },
            CameraMove { view: Some(CameraView::Perspective), ..STILL },
            CameraMove { turn: 200.0, zoom: 0.6, ..STILL },
        ];
        let grabs = [
            ([320.0, 240.0], [344.0, 230.0], 1.0, 0.0),
            ([150.0, 300.0], [140.0, 330.0], 1.25, 0.0),
            ([400.0, 200.0], [400.0, 200.0], 0.8, 12.0),
            ([250.0, 260.0], [270.0, 250.0], 1.1, -7.5),
        ];
        for m in moves {
            for (from, to, zoom, turn) in grabs {
                let mut s = town();
                s.move_camera(&m);
                let p = ground_at(&s.view, &s.camera, from).expect("ground under the finger");
                let right = ground_at(&s.view, &s.camera, [from[0] + 40.0, from[1]]).expect("ground");
                let g = grab_move(&s, from, to, zoom, turn).expect("a grab");
                s.move_camera(&g);
                let at = s.screen_point([p[0], p[1], 0.0]).expect("in front");
                assert!((at[0] - to[0]).abs() < 0.05 && (at[1] - to[1]).abs() < 0.05, "{m:?} {from:?}: at {at:?}");
                let r = s.screen_point([right[0], right[1], 0.0]).expect("in front");
                if turn > 0.0 {
                    assert!(r[1] > to[1] + 1.0, "{m:?}: clockwise moves the right side down: {r:?}");
                } else if turn < 0.0 {
                    assert!(r[1] < to[1] - 1.0, "{m:?}: anticlockwise moves the right side up: {r:?}");
                }
            }
        }
    }

    /// A settle lands on a quarter turn exactly, bit for bit, from wherever
    /// the view was: a quarter turn the next way, or the nearest one.
    #[test]
    fn a_settle_lands_on_a_quarter_turn_exactly() {
        let mut rng = rwgpuengine_core::rng::Rng::new(30);
        let mut s = town();
        for _ in 0..5000 {
            let before = rng.below(360_000) as f32 / 1000.0;
            s.move_camera(&CameraMove { turn: before - s.camera.turn, ..STILL });
            let turned = rng.range_i32(-170_000, 170_000) as f32 / 1000.0;
            s.move_camera(&CameraMove { turn: turned, ..STILL });
            let quarters = rng.range_i32(-2, 2);
            let m = settle_move(&s, quarters, turned);
            s.move_camera(&m);
            let base = (before / 90.0).round() as i32;
            let want = ((base + quarters).rem_euclid(4) * 90) as f32;
            assert_eq!(s.camera.turn, want, "from {before} turned {turned} by {quarters} quarters");
        }
    }

    /// A drag pans in a game where nothing moves by the player's directions
    /// (the towns), and swipes where something does (the games with a player).
    #[test]
    fn a_drag_pans_only_where_nothing_has_a_mover() {
        assert!(drag_pans(&town()));
        assert!(!drag_pans(&example("coin_vault")) && !drag_pans(&example("iso_garden")));
    }

    /// The pan of a drag, applied, puts the ground point that was under the
    /// center where the pointer went: under every camera of the engine.
    #[test]
    fn a_pan_drag_moves_the_ground_with_the_pointer() {
        let mut s = town();
        let moves = [
            STILL,
            CameraMove { turn: 90.0, ..STILL },
            CameraMove { turn: 37.0, zoom: 1.5, ..STILL },
            CameraMove { view: Some(CameraView::Perspective), ..STILL },
            CameraMove { turn: 200.0, zoom: 0.6, ..STILL },
        ];
        for m in moves {
            s.move_camera(&m);
            let [cx, cy] = s.camera.center;
            let before = s.screen_point([cx, cy, 0.0]).expect("in front");
            let d = [24.0, -10.0];
            let pan = pan_move(&s, d).expect("a pan");
            s.move_camera(&pan);
            let after = s.screen_point([cx, cy, 0.0]).expect("in front");
            let moved = [after[0] - before[0], after[1] - before[1]];
            assert!((moved[0] - d[0]).abs() < 0.05 && (moved[1] - d[1]).abs() < 0.05, "{m:?}: moved {moved:?}");
        }
    }
}
