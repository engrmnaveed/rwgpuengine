//! The web player: the functions the page calls (ADR 0024).
//!
//! The page (`web/player.js` in this crate's folder) owns the canvas, the
//! events and the animation frames. It hands each of them to these
//! functions and puts on the screen what they give back. Everything that
//! decides something is here, in the code the native window uses too: the
//! engine's session and its fixed step ([`crate::clock::Steps`]), the
//! bindings ([`crate::input`]), where the frame goes ([`crate::fit`]), and
//! drawing only what changed ([`Player`]).
//!
//! The frame is shown one of two ways, and the page says which it has:
//!
//! - **WebGPU.** [`gpu_open`] finds an adapter and a device for the canvas.
//!   `rwgpuengine_gpu` draws the frame and scales it onto the canvas, nearest
//!   pixel, with bars, as in the native window.
//! - **The CPU reference renderer**, where the browser has no WebGPU, gives
//!   no adapter, or loses the device. The reference's painter paints the
//!   frame and the page puts the pixels on a 2D canvas of the frame's size
//!   ([`pixels`]). The game goes on where it was: the session is the same.
//!
//! Text comes back as strings; a problem as the JSON `rwgpu` prints,
//! `{"ok": false, "diagnostics": [...]}`.

use std::cell::RefCell;

use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_core::hash;
use rwgpuengine_core::json::Json;
use rwgpuengine_data::Vfs;
use rwgpuengine_gpu::{FrameCopy, Options, Renderer, wgpu};
use rwgpuengine_runtime::{DataOnly, LiveInput, Presenter, Session, SessionConfig};
use rwgpuengine_sim::dump;
use wasm_bindgen::prelude::*;

use crate::clock::{Steps, Timings, Watch};
use crate::fit;
use crate::gpu::{self, Gpu, failed};
use crate::input::{self, Button, Gesture, Input};
use crate::play::{self, Player};

/// [`tick`] and the other calls that step or draw say what the page has to
/// do with these bits.
///
/// The CPU path painted a new picture: put [`pixels`] on the canvas.
pub const PIXELS: u32 = 1;
/// The session reached the frame it was asked to stop at ([`set_end`]).
pub const FINISHED: u32 = 2;
/// The GPU stopped working ([`fallback`] says why). The game goes on with
/// the CPU reference renderer: the page gives it a 2D canvas.
pub const LOST: u32 = 4;
/// A frame was drawn (by either path).
pub const DREW: u32 = 8;

/// The GPU and the canvas it shows the frame on.
struct Canvas {
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// Lost on purpose, by [`lose_device`].
    lost: bool,
}

/// The CPU path: the session, painted by the reference's painter.
struct Cpu {
    session: Session,
    end: Option<u64>,
    presenter: Presenter,
    draws: u64,
    paint_us: Timings,
}

enum Play {
    Gpu(Box<Player>),
    Cpu(Box<Cpu>),
}

impl Play {
    fn session(&self) -> &Session {
        match self {
            Play::Gpu(p) => &p.session,
            Play::Cpu(c) => &c.session,
        }
    }

    fn session_mut(&mut self) -> &mut Session {
        match self {
            Play::Gpu(p) => &mut p.session,
            Play::Cpu(c) => &mut c.session,
        }
    }

    fn finished(&self) -> bool {
        match self {
            Play::Gpu(p) => p.finished(),
            Play::Cpu(c) => c.end.is_some_and(|e| c.session.frame() >= e),
        }
    }

    fn step(&mut self, live: &LiveInput) {
        if !self.finished() {
            self.session_mut().step(live);
        }
    }
}

struct Web {
    vfs: Option<Vfs>,
    canvas: Option<Canvas>,
    /// Why the CPU reference renderer shows the game, once it does.
    fallback: Option<String>,
    play: Option<Play>,
    /// The renderer of the last session: a restart keeps its pipelines.
    spare: Option<Renderer>,
    input: Input,
    steps: Steps,
    /// Started by the first animation frame after a start or a pause.
    watch: Option<Watch>,
    /// The canvas's size in its own pixels: the device pixels it covers on
    /// the WebGPU path, the frame's size on the CPU path.
    size: [u32; 2],
    /// Where the frame goes on the canvas.
    area: [u32; 4],
    /// The state may have changed outside a step (a start, a camera move).
    dirty: bool,
    /// The canvas lost its picture (resized) and needs the frame again.
    show: bool,
    present_us: Timings,
    copy: Option<FrameCopy>,
    out: String,
}

thread_local! {
    static WEB: RefCell<Web> = RefCell::new(Web::new());
}

fn with<R>(f: impl FnOnce(&mut Web) -> R) -> R {
    WEB.with(|w| f(&mut w.borrow_mut()))
}

fn problems(diags: &[Diagnostic]) -> String {
    Json::object()
        .with("ok", diags.is_empty())
        .with("diagnostics", diags.iter().map(Diagnostic::to_json).collect::<Vec<_>>())
        .to_compact()
}

/// "Apple M5 Pro (WebGPU)". Browsers keep the adapter's name to themselves
/// unless asked otherwise, and wgpu reads only that name; the page names the
/// adapter from `GPUAdapter.info` then.
fn describe(info: &wgpu::AdapterInfo) -> String {
    if info.name.is_empty() {
        return format!("WebGPU{}", if info.device_type == wgpu::DeviceType::Cpu { ", software" } else { "" });
    }
    gpu::describe(info)
}

impl Web {
    fn new() -> Web {
        Web {
            vfs: None,
            canvas: None,
            fallback: None,
            play: None,
            spare: None,
            input: Input::default(),
            steps: Steps::new(60),
            watch: None,
            size: [1, 1],
            area: [0, 0, 1, 1],
            dirty: false,
            show: false,
            present_us: Timings::default(),
            copy: None,
            out: String::new(),
        }
    }

    fn frame_size(&self) -> [u32; 2] {
        self.play.as_ref().map_or([1, 1], |p| [p.session().view.width, p.session().view.height])
    }

    /// Fit the frame to the canvas again (a new size, a new session).
    fn refit(&mut self) {
        if self.canvas.is_none() {
            self.size = self.frame_size();
        }
        self.area = fit::letterbox(self.frame_size(), self.size);
    }

    fn begin(&mut self, session: Session) {
        self.steps = Steps::new(session.project.sim.tick_rate);
        self.watch = None;
        self.input = Input::default();
        if let Some(Play::Gpu(old)) = self.play.take() {
            self.spare = Some(old.renderer);
        }
        self.play = Some(match &self.canvas {
            Some(c) => {
                let renderer = self.spare.take().unwrap_or_else(|| Renderer::new(&c.gpu.device, Options::default()));
                Play::Gpu(Box::new(Player::with_renderer(session, renderer, None)))
            }
            None => Play::Cpu(Box::new(Cpu::new(session, None))),
        });
        self.dirty = true;
        self.refit();
    }

    /// From here on the CPU reference renderer shows the game, which goes on
    /// where it was.
    fn use_cpu(&mut self, why: String) {
        self.fallback = Some(why);
        self.canvas = None;
        self.spare = None;
        self.copy = None;
        if let Some(Play::Gpu(p)) = self.play.take() {
            let Player { session, end, .. } = *p;
            self.play = Some(Play::Cpu(Box::new(Cpu::new(session, end))));
        }
        self.dirty = true;
        self.refit();
    }

    fn gesture(&mut self, g: Option<Gesture>) {
        let (Some(g), Some(play)) = (g, self.play.as_mut()) else { return };
        let session = play.session_mut();
        let m = match g {
            Gesture::Move(m) => Some(m),
            Gesture::Pan(d) => play::pan_move(session, d),
            Gesture::Key(does) => Some(play::key_move(session, does)),
        };
        if let Some(m) = m {
            session.move_camera(&m);
            self.dirty = true;
        }
    }

    /// The frame pixel under a canvas pixel, and canvas pixels per frame pixel.
    fn to_frame(&self, at: [f64; 2]) -> (Option<[f32; 2]>, f64) {
        let frame = self.frame_size();
        (fit::to_frame(self.area, frame, at), fit::scale(self.area, frame))
    }

    /// Build the list, and draw and show it if it changed.
    fn update(&mut self, watch: &Watch) -> u32 {
        let Some(play) = self.play.as_mut() else { return 0 };
        match play {
            Play::Gpu(player) => {
                let Some(canvas) = &self.canvas else { return 0 };
                if player.update(watch) && player.draw(&canvas.gpu, watch) {
                    self.show = true;
                    return DREW;
                }
                0
            }
            Play::Cpu(cpu) => {
                let t = watch.micros();
                if cpu.presenter.present(&cpu.session) {
                    cpu.paint_us.add(watch.micros() - t);
                    cpu.draws += 1;
                    return PIXELS | DREW;
                }
                0
            }
        }
    }

    /// Show the frame on the WebGPU canvas.
    fn present(&mut self, watch: &Watch) {
        self.show = false;
        let (Some(Play::Gpu(player)), Some(canvas)) = (&mut self.play, &mut self.canvas) else { return };
        let t = watch.micros();
        let texture = match canvas.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => {
                // Shown again at the next frame.
                canvas.surface.configure(&canvas.gpu.device, &canvas.config);
                self.show = true;
                return;
            }
        };
        let view = texture.texture.create_view(&Default::default());
        let g = &canvas.gpu;
        player.renderer.present(&g.device, &g.queue, &view, canvas.config.format, Some(self.area), false);
        g.queue.present(texture);
        self.present_us.add(watch.micros() - t);
    }

    /// After a step or a move: draw what changed, show it, and fall back to
    /// the CPU if the GPU stopped working.
    fn after(&mut self, watch: &Watch) -> u32 {
        let mut flags = 0;
        if self.dirty {
            self.dirty = false;
            flags |= self.update(watch);
        }
        if self.show {
            self.present(watch);
        }
        let lost = self.canvas.as_ref().and_then(|c| match c.gpu.check() {
            Err(d) => Some(format!("{} [{}]", d.message, d.code)),
            Ok(()) if c.lost => Some("the device was lost".to_string()),
            Ok(()) => None,
        });
        if let Some(why) = lost {
            self.use_cpu(why);
            flags = LOST | self.update(watch);
        }
        if self.play.as_ref().is_some_and(Play::finished) {
            flags |= FINISHED;
        }
        flags
    }

    fn tick(&mut self, now: f64) -> u32 {
        if self.play.is_none() {
            return 0;
        }
        let watch = *self.watch.get_or_insert_with(|| Watch::started_at(now));
        let due = self.steps.due(watch.since(now));
        for _ in 0..due {
            let live = self.input.take_step();
            if let Some(p) = self.play.as_mut() {
                p.step(&live);
            }
        }
        self.dirty |= due > 0;
        self.after(&watch)
    }

    fn stats(&self) -> Json {
        let t = |t: &Timings| {
            let (median, max) = t.summary();
            Json::object().with("median_us", median).with("max_us", max).with("count", t.len()).with("sum_us", t.sum())
        };
        let mut j = Json::object()
            .with("path", if self.canvas.is_some() { "webgpu" } else { "cpu" })
            .with("fallback", self.fallback.clone())
            .with("canvas", Json::Arr(self.size.iter().map(|&v| Json::from(v)).collect()))
            .with("area", Json::Arr(self.area.iter().map(|&v| Json::from(v)).collect()))
            .with("present", t(&self.present_us))
            .with("steps_behind", self.steps.behind)
            .with("steps_dropped", self.steps.dropped);
        if let Some(c) = &self.canvas {
            j = j.with("adapter", describe(&c.gpu.info)).with("format", format!("{:?}", c.config.format));
        }
        match &self.play {
            Some(Play::Gpu(p)) => {
                j = j
                    .with("frame", p.session.frame())
                    .with("draws", p.draws)
                    .with("list", t(&p.list_us))
                    .with("draw", t(&p.draw_us))
            }
            Some(Play::Cpu(c)) => {
                j = j.with("frame", c.session.frame()).with("draws", c.draws).with("paint", t(&c.paint_us))
            }
            None => {}
        }
        j
    }
}

impl Cpu {
    fn new(session: Session, end: Option<u64>) -> Cpu {
        Cpu { session, end, presenter: Presenter::new(), draws: 0, paint_us: Timings::default() }
    }
}

/// The engine version.
#[wasm_bindgen]
pub fn version() -> String {
    rwgpuengine_core::ENGINE_VERSION.to_string()
}

/// Open WebGPU for `canvas`: an adapter (`software` asks for the browser's
/// fallback adapter only), a device, and the canvas configured to show the
/// frame with no sRGB encoding. Resolves to the adapter's name. Rejects
/// with the problems' JSON (E0501, E0503); the page then shows the game with
/// [`use_cpu`] on a fresh canvas, since this one has a WebGPU context.
#[wasm_bindgen]
pub async fn gpu_open(canvas: web_sys::HtmlCanvasElement, software: bool) -> Result<String, JsValue> {
    let fail = |d: Diagnostic| JsValue::from_str(&problems(&[d]));
    let instance = gpu::instance();
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
        .map_err(|e| fail(failed(format!("the canvas gave no WebGPU context: {e}"))))?;
    let gpu = Gpu::request(&instance, Some(&surface), software).await.map_err(fail)?;
    let caps = surface.get_capabilities(&gpu.adapter);
    let mut config = surface
        .get_default_config(&gpu.adapter, 1, 1)
        .ok_or_else(|| fail(failed(format!("{} cannot show a picture on this canvas", describe(&gpu.info)))))?;
    // The frame's bytes are what the screen should show (ADR 0021).
    let plain = [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8Unorm];
    if let Some(f) = plain.into_iter().find(|f| caps.formats.contains(f)) {
        config.format = f;
    }
    config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
    surface.configure(&gpu.device, &config);
    let name = describe(&gpu.info);
    with(|w| {
        w.canvas = Some(Canvas { gpu, surface, config, lost: false });
        w.fallback = None;
        w.refit();
    });
    Ok(name)
}

/// Show the game with the CPU reference renderer from now on, saying `why`.
/// A game being played goes on where it was.
#[wasm_bindgen]
pub fn use_cpu(why: &str) {
    with(|w| w.use_cpu(why.to_string()));
}

/// Why the CPU reference renderer shows the game; empty on WebGPU.
#[wasm_bindgen]
pub fn fallback() -> String {
    with(|w| w.fallback.clone().unwrap_or_default())
}

/// Load a project bundle (`Vfs::to_bundle`). On a problem returns false and
/// [`out`] has it.
#[wasm_bindgen]
pub fn set_bundle(text: &str) -> bool {
    with(|w| match Vfs::from_bundle(text) {
        Ok(vfs) => {
            w.vfs = Some(vfs);
            true
        }
        Err(e) => {
            w.out = Json::object().with("ok", false).with("error", e).to_compact();
            false
        }
    })
}

/// Start a session: `scene` and `input` (a playtest) may be empty for the
/// project's main scene and live input only. [`out`] has the warnings, or
/// the problems when it returns false.
#[wasm_bindgen]
pub fn start(scene: &str, input: &str) -> bool {
    with(|w| {
        let Some(vfs) = &w.vfs else {
            w.out = Json::object().with("ok", false).with("error", "no bundle loaded").to_compact();
            return false;
        };
        let cfg = SessionConfig {
            scene: Some(scene.to_string()).filter(|x| !x.is_empty()),
            script: Some(input.to_string()).filter(|x| !x.is_empty()),
            seed: None,
        };
        match Session::load(vfs, &cfg, Box::new(DataOnly)) {
            Ok(session) => {
                w.out = problems(&session.warnings);
                w.begin(session);
                true
            }
            Err(diags) => {
                w.out = problems(&diags);
                false
            }
        }
    })
}

/// The text the last call left: problems, or a start's warnings.
#[wasm_bindgen]
pub fn out() -> String {
    with(|w| w.out.clone())
}

/// The canvas now covers `width` × `height` of its own pixels (on the WebGPU
/// path, device pixels). The frame is shown again at the next tick.
#[wasm_bindgen]
pub fn resize(width: u32, height: u32) {
    with(|w| {
        let Some(c) = w.canvas.as_mut() else { return };
        let max = c.gpu.device.limits().max_texture_dimension_2d;
        let size = [width.clamp(1, max), height.clamp(1, max)];
        if size != [c.config.width, c.config.height] {
            c.config.width = size[0];
            c.config.height = size[1];
            c.surface.configure(&c.gpu.device, &c.config);
        }
        w.size = size;
        w.show = true;
        w.refit();
    })
}

/// One animation frame at `now` (the page's clock, in ms): the steps that
/// are due at the project's rate (at most five), then the frame drawn and
/// shown if it changed. Returns [`PIXELS`], [`FINISHED`], [`LOST`] and
/// [`DREW`] bits.
#[wasm_bindgen]
pub fn tick(now: f64) -> u32 {
    with(|w| w.tick(now))
}

/// The page was paused (or hidden) and plays again: count steps from now,
/// rather than catch up.
#[wasm_bindgen]
pub fn resume() {
    with(|w| {
        w.watch = None;
        let rate = w.play.as_ref().map_or(60, |p| p.session().project.sim.tick_rate);
        w.steps = Steps::new(rate);
    })
}

/// One step with the input gathered, and the frame drawn if it changed (the
/// page's Step button).
#[wasm_bindgen]
pub fn step_once() -> u32 {
    with(|w| {
        let live = w.input.take_step();
        if let Some(p) = w.play.as_mut() {
            p.step(&live);
        }
        w.dirty = true;
        let watch = Watch::start();
        w.after(&watch)
    })
}

/// Stop stepping at `frame` (0: never).
#[wasm_bindgen]
pub fn set_end(frame: u32) {
    with(|w| set_end_in(w, frame));
}

/// Play to `frame` as fast as the steps go, with no live input, drawing
/// each frame whose list changed as the page would: what `rwgpu-player
/// --headless` does natively. The last frame is shown.
#[wasm_bindgen]
pub fn replay(frame: u32) -> u32 {
    with(|w| {
        set_end_in(w, frame);
        let watch = Watch::start();
        let idle = LiveInput::default();
        w.dirty = true;
        let mut flags = w.after(&watch);
        while w.play.as_ref().is_some_and(|p| !p.finished()) {
            if let Some(p) = w.play.as_mut() {
                p.step(&idle);
            }
            w.dirty = true;
            // A lost device does not end it: the CPU path takes over.
            flags |= w.after(&watch);
        }
        flags
    })
}

fn set_end_in(w: &mut Web, frame: u32) {
    let end = (frame > 0).then_some(u64::from(frame));
    match w.play.as_mut() {
        Some(Play::Gpu(p)) => p.end = end,
        Some(Play::Cpu(c)) => c.end = end,
        None => {}
    }
}

/// Where the CPU path's picture is in the module's memory: width × height ×
/// 4 bytes of RGBA, valid until the next call that steps or draws.
#[wasm_bindgen]
pub fn pixels() -> u32 {
    with(|w| match &w.play {
        Some(Play::Cpu(c)) => c.presenter.pixels().as_ptr() as u32,
        _ => 0,
    })
}

/// A key went down or up, by its `KeyboardEvent.code`. Returns true when
/// the player binds it (the page then keeps the browser from scrolling).
#[wasm_bindgen]
pub fn key(code: &str, down: bool, repeat: bool) -> bool {
    with(|w| {
        let g = w.input.key(code, down, repeat);
        w.gesture(g);
        input::bound(code)
    })
}

/// Shift is held or let go.
#[wasm_bindgen]
pub fn shift(held: bool) {
    with(|w| w.input.shift(held));
}

/// The page lost the keyboard: nothing is held any more.
#[wasm_bindgen]
pub fn blur() {
    with(|w| w.input.lost_focus());
}

fn button_of(b: i16) -> Button {
    match b {
        0 => Button::Left,
        1 => Button::Middle,
        2 => Button::Right,
        _ => Button::Other,
    }
}

/// A pointer went down at (x, y) in canvas pixels: a mouse `button`
/// (`PointerEvent.button`), or a finger when `touch`.
#[wasm_bindgen]
pub fn pointer_down(id: u32, touch: bool, button: i16, x: f64, y: f64) {
    with(|w| {
        let (at, scale) = w.to_frame([x, y]);
        if touch {
            w.input.finger_down(id, [x, y], at);
        } else {
            let g = w.input.moved([x, y], scale);
            w.gesture(g);
            w.input.button(button_of(button), true, at);
        }
    })
}

/// A pointer moved to (x, y) in canvas pixels.
#[wasm_bindgen]
pub fn pointer_move(id: u32, touch: bool, x: f64, y: f64) {
    with(|w| {
        let (_, scale) = w.to_frame([x, y]);
        let g = if touch { w.input.finger_moved(id, [x, y]) } else { w.input.moved([x, y], scale) };
        w.gesture(g);
    })
}

/// A pointer went up at (x, y), or was taken away by the browser
/// (`cancelled`: no tap).
#[wasm_bindgen]
pub fn pointer_up(id: u32, touch: bool, button: i16, x: f64, y: f64, cancelled: bool) {
    with(|w| {
        let (at, _) = w.to_frame([x, y]);
        let at = at.filter(|_| !cancelled);
        if touch {
            w.input.finger_up(id, at);
        } else {
            w.input.button(button_of(button), false, at);
        }
    })
}

/// The wheel: `WheelEvent.deltaY` in its `deltaMode` (0 pixels, 1 lines, 2
/// pages). With Control held it is a trackpad's pinch, as browsers send it.
#[wasm_bindgen]
pub fn wheel(mode: u32, dy: f64, ctrl: bool) {
    with(|w| {
        let g = match (ctrl, mode) {
            (true, _) => w.input.pinch(-dy / 100.0),
            (false, 0) => w.input.wheel(0.0, -dy),
            (false, _) => w.input.wheel(-dy, 0.0),
        };
        w.gesture(g);
    })
}

/// The frame's width (the project's `[view]`), 0 before a start.
#[wasm_bindgen]
pub fn width() -> u32 {
    with(|w| w.play.as_ref().map_or(0, |p| p.session().view.width))
}

#[wasm_bindgen]
pub fn height() -> u32 {
    with(|w| w.play.as_ref().map_or(0, |p| p.session().view.height))
}

#[wasm_bindgen]
pub fn frame() -> u32 {
    with(|w| w.play.as_ref().map_or(0, |p| p.session().frame() as u32))
}

#[wasm_bindgen]
pub fn tick_rate() -> u32 {
    with(|w| w.play.as_ref().map_or(60, |p| p.session().project.sim.tick_rate))
}

/// Frames the playtest being played asks for (0 without one).
#[wasm_bindgen]
pub fn script_frames() -> u32 {
    with(|w| w.play.as_ref().and_then(|p| p.session().script_frames()).unwrap_or(0) as u32)
}

/// The state hash, as `rwgpu run` prints it.
#[wasm_bindgen]
pub fn state_hash() -> String {
    with(|w| w.play.as_ref().map(|p| p.session().hash_hex()).unwrap_or_default())
}

/// The hash of the reference's picture of the current state: `rwgpu shot`'s
/// image hash.
#[wasm_bindgen]
pub fn image_hash() -> String {
    with(|w| match &w.play {
        Some(p) => {
            let mut rgba = Vec::new();
            p.session().paint(&mut rgba);
            hash::hex(hash::fnv1a64(&rgba))
        }
        None => String::new(),
    })
}

/// The camera now, as `rwgpu run --json` reports it.
#[wasm_bindgen]
pub fn camera() -> String {
    with(|w| w.play.as_ref().map(|p| p.session().camera_json().to_compact()).unwrap_or_default())
}

/// Events from frame `since` on, as a JSON array.
#[wasm_bindgen]
pub fn events(since: u32) -> String {
    with(|w| {
        let Some(p) = &w.play else { return "[]".into() };
        let s = p.session();
        let list = s.world.events.all().iter().filter(|e| e.frame >= u64::from(since));
        Json::Arr(list.map(|e| dump::event_json(&s.world, e)).collect()).to_compact()
    })
}

/// What the player did: the path, the adapter, frames drawn and what they
/// took, as JSON.
#[wasm_bindgen]
pub fn stats() -> String {
    with(|w| w.stats().to_compact())
}

/// Start copying the frame the GPU drew to memory, for a check. False on the
/// CPU path or before anything was drawn.
#[wasm_bindgen]
pub fn copy_frame() -> bool {
    with(|w| {
        let (Some(Play::Gpu(p)), Some(c)) = (&w.play, &w.canvas) else { return false };
        w.copy = p.renderer.copy_frame(&c.gpu.device, &c.gpu.queue);
        w.copy.is_some()
    })
}

/// The frame's RGBA bytes once the copy arrived (the page returns to the
/// browser between asks). Throws if the copy failed.
#[wasm_bindgen]
pub fn copied() -> Result<Option<Vec<u8>>, JsValue> {
    with(|w| {
        let Some(copy) = &w.copy else { return Ok(None) };
        match copy.take() {
            None => Ok(None),
            Some(r) => {
                w.copy = None;
                r.map(Some).map_err(|e| JsValue::from_str(&e))
            }
        }
    })
}

/// Destroy the GPU device, as a browser does when it loses it: for the
/// check that the game goes on with the CPU reference renderer.
#[wasm_bindgen]
pub fn lose_device() {
    with(|w| {
        if let Some(c) = w.canvas.as_mut() {
            c.lost = true;
            c.gpu.device.destroy();
        }
    })
}
