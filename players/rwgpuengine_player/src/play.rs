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
use rwgpuengine_render::{Camera, RenderList};
use rwgpuengine_runtime::{LiveInput, Session};

use crate::clock::{Timings, Watch};
use crate::gpu::{Gpu, failed};
use crate::input::{Does, STILL};

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
        Player {
            session,
            renderer: Renderer::new(device, Options::default()),
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

    /// The move for [`Does::View`] and [`Does::Home`].
    pub fn key_move(&self, does: Does) -> CameraMove {
        let now = self.session.camera;
        match does {
            Does::View => CameraMove {
                view: Some(match now.view {
                    CameraView::Iso => CameraView::Perspective,
                    CameraView::Perspective => CameraView::Iso,
                }),
                ..STILL
            },
            Does::Home => {
                let home = Camera::new(&self.session.scene);
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

    /// The pan of a drag by `d` frame pixels ([`pan_move`]).
    pub fn pan_move(&self, d: [f32; 2]) -> Option<CameraMove> {
        pan_move(&self.session, d)
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

    fn town() -> Session {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/tiny_town");
        let vfs = Vfs::from_dir(&dir).expect("tiny_town");
        Session::load(&vfs, &SessionConfig::default(), Box::new(DataOnly)).expect("loads")
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
