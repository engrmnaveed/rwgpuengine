//! The window: winit's event loop around a [`Player`].
//!
//! The loop sleeps until the next fixed step is due (`ControlFlow::WaitUntil`)
//! or an event arrives. When it wakes it takes the steps that are due (at
//! most [`MAX_CATCH_UP`](crate::clock::MAX_CATCH_UP)), builds the render list
//! once, and draws and shows it only if it changed. A still scene costs a
//! step and a list comparison 60 times a second, and no drawing.

use std::sync::Arc;

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_gpu::wgpu;
use rwgpuengine_runtime::Session;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::clock::{Steps, Timings, Watch};
use crate::fit;
use crate::gpu::{self, Gpu, failed};
use crate::input::{Button, Gesture, Input};
use crate::play::{self, Player};

/// A picture the window showed, read back for `--screenshot`.
#[derive(Debug)]
pub struct Shot {
    pub width: u32,
    pub height: u32,
    /// RGBA8, top row first.
    pub rgba: Vec<u8>,
    /// `window`: read back from the window's own surface after it was shown.
    /// `offscreen`: drawn the same way into a texture of the window's size,
    /// where the surface cannot be read.
    pub from: &'static str,
}

/// How a run in a window went.
#[derive(Debug)]
pub struct Ran {
    pub player: Player,
    pub gpu: Gpu,
    /// The person closed the window before the end frame.
    pub closed_early: bool,
    /// Window size in pixels at the end, and its scale factor.
    pub window: [u32; 2],
    pub scale_factor: f64,
    pub format: wgpu::TextureFormat,
    pub steps: Steps,
    /// Showing a frame on the window: getting the surface's texture, scaling
    /// the frame onto it, submitting (µs).
    pub present_us: Timings,
    pub shot: Option<Shot>,
}

/// Open a window and play `session` in it until `end` (or until it is closed).
pub fn play(session: Session, end: Option<u64>, screenshot: bool) -> Result<Ran, Diagnostic> {
    let event_loop = EventLoop::new().map_err(|e| no_window(e.to_string()))?;
    let mut app = App {
        title: session.project.name.clone(),
        session: Some(session),
        end,
        screenshot,
        open: None,
        input: Input::default(),
        watch: Watch::start(),
        steps: Steps::new(60),
        error: None,
        closed_early: false,
        camera_moved: false,
        cursor: None,
        present_us: Timings::default(),
        shot: None,
    };
    event_loop.run_app(&mut app).map_err(|e| no_window(e.to_string()))?;
    if let Some(d) = app.error {
        return Err(d);
    }
    let Some(open) = app.open else {
        return Err(no_window("the window system never gave the player a window".into()));
    };
    let size = open.window.inner_size();
    Ok(Ran {
        player: open.player,
        gpu: open.gpu,
        closed_early: app.closed_early,
        window: [size.width, size.height],
        scale_factor: open.window.scale_factor(),
        format: open.config.format,
        steps: app.steps,
        present_us: app.present_us,
        shot: app.shot,
    })
}

/// E0502. winit's errors start with the place in its own source
/// (`os error at <path>/winit-0.30.13/src/...rs:765: `): only the sentence
/// after it is for the reader.
fn no_window(why: String) -> Diagnostic {
    let why = match why.strip_prefix("os error at ") {
        Some(rest) => rest.split_once(".rs:").and_then(|(_, r)| r.split_once(": ")).map_or(why.as_str(), |(_, s)| s),
        None => why.as_str(),
    };
    Diagnostic::new(&codes::NO_WINDOW_SYSTEM, format!("could not open a window: {why}")).hint(
        "run it on a desktop, or add --headless to play without a window; on a Linux server `xvfb-run -a` gives it \
         a virtual display",
    )
}

struct App {
    title: String,
    /// Until the window is open.
    session: Option<Session>,
    end: Option<u64>,
    screenshot: bool,
    open: Option<Open>,
    input: Input,
    watch: Watch,
    steps: Steps,
    error: Option<Diagnostic>,
    closed_early: bool,
    camera_moved: bool,
    /// The mouse pointer, in window pixels.
    cursor: Option<[f64; 2]>,
    present_us: Timings,
    shot: Option<Shot>,
}

struct Open {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// The surface can be read back (`--screenshot`).
    readable: bool,
    gpu: Gpu,
    player: Player,
    /// Where the frame goes in the window.
    area: [u32; 4],
}

impl App {
    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<Open, Diagnostic> {
        let session = self.session.take().ok_or_else(|| no_window("asked for a second window".into()))?;
        let (w, h) = (session.view.width, session.view.height);
        // The frame's own size in logical pixels: on a display that scales by
        // 2, every frame pixel is 2 x 2 window pixels.
        let attributes = Window::default_attributes()
            .with_title(self.title.as_str())
            .with_inner_size(LogicalSize::new(f64::from(w), f64::from(h)))
            .with_min_inner_size(LogicalSize::new(f64::from(w / 8).max(16.0), f64::from(h / 8).max(16.0)));
        let window = Arc::new(event_loop.create_window(attributes).map_err(|e| no_window(e.to_string()))?);
        let instance = gpu::instance();
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| failed(format!("could not make a surface to draw into the window: {e}")))?;
        let gpu = Gpu::open(&instance, Some(&surface))?;
        let size = window.inner_size();
        let caps = surface.get_capabilities(&gpu.adapter);
        let mut config =
            surface.get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1)).ok_or_else(|| {
                Diagnostic::new(
                    &codes::NO_GPU_ADAPTER,
                    format!("{} cannot show a picture in this window", gpu::describe(&gpu.info)),
                )
                .hint("install or update the graphics driver; `WGPU_BACKEND` picks another backend")
            })?;
        // The frame's bytes are what the screen should show: a format that
        // does not encode sRGB takes them as they are (ADR 0021).
        let plain = [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8Unorm];
        if let Some(f) = plain.into_iter().find(|f| caps.formats.contains(f)) {
            config.format = f;
        }
        let readable = self.screenshot && caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
        if readable {
            config.usage |= wgpu::TextureUsages::COPY_SRC;
        }
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&gpu.device, &config);
        self.steps = Steps::new(session.project.sim.tick_rate);
        self.input.set_drag_pans(play::drag_pans(&session));
        let player = Player::new(session, &gpu.device, self.end);
        let area = fit::letterbox(player.frame_size(), [config.width, config.height]);
        Ok(Open { window, surface, config, readable, gpu, player, area })
    }

    /// Show the frame on the window. With `capture`, read the shown picture
    /// back for `--screenshot`; when the window gives no texture to show it
    /// on (covered, minimized, being resized), draw it off screen instead.
    fn present(&mut self, capture: bool) {
        let Some(open) = self.open.as_mut() else { return };
        let t = self.watch.micros();
        let texture = match open.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                // Shown again after the next resize or wake-up.
                open.surface.configure(&open.gpu.device, &open.config);
                None
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => None,
            wgpu::CurrentSurfaceTexture::Validation => {
                self.error = Some(failed("the window's surface reported a validation error".into()));
                return;
            }
        };
        let mut readback = None;
        if let Some(texture) = texture {
            let view = texture.texture.create_view(&Default::default());
            let g = &open.gpu;
            open.player.renderer.present(&g.device, &g.queue, &view, open.config.format, Some(open.area), false);
            readback = (capture && open.readable).then(|| Readback::start(g, &texture.texture));
            g.queue.present(texture);
            self.present_us.add(self.watch.micros() - t);
        }
        if capture {
            let shot = match readback {
                Some(r) => r.finish(&open.gpu, "window"),
                None => offscreen(open),
            };
            match shot {
                Ok(s) => self.shot = Some(s),
                Err(d) => self.error = Some(d),
            }
        }
    }

    fn gesture(&mut self, g: Option<Gesture>) {
        let (Some(g), Some(open)) = (g, self.open.as_mut()) else { return };
        if open.player.gesture(g).is_some() {
            self.camera_moved = true;
        }
    }

    /// A window pixel as a frame pixel, and whether it is on the frame.
    fn frame_point(&self, at: [f64; 2]) -> ([f32; 2], bool) {
        self.open.as_ref().map_or(([0.0; 2], false), |o| fit::frame_point(o.area, o.player.frame_size(), at))
    }

    /// A finger on a touch screen: the bindings of `crate::input`.
    fn touch(&mut self, t: Touch) {
        let (at, inside) = self.frame_point([t.location.x, t.location.y]);
        // winit's ids are the platform's; two fingers down at once never share one.
        let id = t.id as u32;
        let g = match t.phase {
            TouchPhase::Started => self.input.finger_down(id, at, inside),
            TouchPhase::Moved => self.input.finger_moved(id, at),
            TouchPhase::Ended => self.input.finger_up(id, at, inside),
            TouchPhase::Cancelled => self.input.finger_cancelled(id),
        };
        self.gesture(g);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.open.is_some() || self.error.is_some() {
            return;
        }
        match self.open(event_loop) {
            Ok(open) => {
                self.open = Some(open);
                self.watch = Watch::start();
                if let Some(open) = self.open.as_mut() {
                    open.player.update(&self.watch);
                    open.player.draw(&open.gpu, &self.watch);
                }
                self.present(false);
                event_loop.set_control_flow(ControlFlow::WaitUntil(self.watch.at(self.steps.next())));
            }
            Err(d) => {
                self.error = Some(d);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if self.open.is_none() {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                self.closed_early = self.open.as_ref().is_some_and(|o| !o.player.finished());
                if self.screenshot && self.shot.is_none() {
                    self.present(true);
                }
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(open) = self.open.as_mut()
                    && size.width > 0
                    && size.height > 0
                {
                    open.config.width = size.width;
                    open.config.height = size.height;
                    open.surface.configure(&open.gpu.device, &open.config);
                    open.area = fit::letterbox(open.player.frame_size(), [size.width, size.height]);
                    open.window.request_redraw();
                }
            }
            // The window system wants the picture again (shown, uncovered,
            // resized): the frame is already drawn, so it is only shown.
            WindowEvent::RedrawRequested => self.present(false),
            WindowEvent::Focused(false) => self.input.lost_focus(),
            WindowEvent::ModifiersChanged(m) => self.input.shift(m.state().shift_key()),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let g = self.input.key(&code_name(code), event.state == ElementState::Pressed, event.repeat);
                    self.gesture(g);
                }
            }
            // A system that also makes mouse events from touches: those are ignored while a finger is down.
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some([position.x, position.y]);
                if !self.input.touching() {
                    let (at, _) = self.frame_point([position.x, position.y]);
                    let g = self.input.moved(at);
                    self.gesture(g);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let (Some(cursor), false) = (self.cursor, self.input.touching()) {
                    let (at, inside) = self.frame_point(cursor);
                    let g = self.input.button(button_of(button), state == ElementState::Pressed, at, inside);
                    self.gesture(g);
                }
            }
            WindowEvent::Touch(t) => self.touch(t),
            WindowEvent::MouseWheel { delta, .. } => {
                let g = match delta {
                    MouseScrollDelta::LineDelta(_, y) => self.input.wheel(f64::from(y), 0.0),
                    MouseScrollDelta::PixelDelta(p) => self.input.wheel(0.0, p.y),
                };
                self.gesture(g);
            }
            WindowEvent::PinchGesture { delta, .. } => {
                let g = self.input.pinch(delta);
                self.gesture(g);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.error.is_some() {
            event_loop.exit();
            return;
        }
        let Some(open) = self.open.as_mut() else { return };
        let due = self.steps.due(self.watch.elapsed());
        for _ in 0..due {
            let live = self.input.take_step();
            open.player.step(&live);
        }
        let mut show = false;
        if due > 0 || self.camera_moved {
            self.camera_moved = false;
            show = open.player.update(&self.watch) && open.player.draw(&open.gpu, &self.watch);
        }
        let finished = open.player.finished();
        if let Err(d) = open.gpu.check() {
            self.error = Some(d);
        }
        if show || finished {
            // At the end the last picture is shown once more, and read back
            // when a screenshot was asked for.
            self.present(finished && self.screenshot);
        }
        if finished || self.error.is_some() {
            event_loop.exit();
            return;
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.watch.at(self.steps.next())));
    }
}

/// A key's W3C `code`, the name the bindings use ([`crate::input::KEYS`]).
/// winit names its `KeyCode`s after those codes, so the name is the
/// variant's.
fn code_name(code: KeyCode) -> String {
    format!("{code:?}")
}

fn button_of(b: MouseButton) -> Button {
    match b {
        MouseButton::Left => Button::Left,
        MouseButton::Right => Button::Right,
        MouseButton::Middle => Button::Middle,
        _ => Button::Other,
    }
}

/// A copy of a texture on its way off the GPU.
struct Readback {
    buffer: wgpu::Buffer,
    row: u32,
    width: u32,
    height: u32,
    bgra: bool,
}

impl Readback {
    fn start(g: &Gpu, texture: &wgpu::Texture) -> Readback {
        let (width, height) = (texture.width(), texture.height());
        let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = g.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenshot"),
            size: u64::from(row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = g.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        g.queue.submit([encoder.finish()]);
        let bgra = matches!(texture.format(), wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
        Readback { buffer, row, width, height, bgra }
    }

    fn finish(self, g: &Gpu, from: &'static str) -> Result<Shot, Diagnostic> {
        let slice = self.buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        g.wait()?;
        let mapped = slice.get_mapped_range().map_err(|e| failed(format!("reading the window back: {e}")))?;
        let line = self.width as usize * 4;
        let mut rgba = Vec::with_capacity(line * self.height as usize);
        for row in mapped.chunks(self.row as usize).take(self.height as usize) {
            rgba.extend_from_slice(&row[..line]);
        }
        drop(mapped);
        if self.bgra {
            for px in rgba.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
        }
        Ok(Shot { width: self.width, height: self.height, rgba, from })
    }
}

/// The window's picture drawn again into a texture of its size, for a
/// surface that cannot be read.
fn offscreen(open: &mut Open) -> Result<Shot, Diagnostic> {
    let g = &open.gpu;
    let texture = g.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("screenshot"),
        size: wgpu::Extent3d { width: open.config.width, height: open.config.height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: open.config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    open.player.renderer.present(&g.device, &g.queue, &view, open.config.format, Some(open.area), false);
    Readback::start(g, &texture).finish(g, "offscreen")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_display_is_e0502_in_winits_own_words_without_its_source_path() {
        let d = no_window(
            "os error at /home/someone/.cargo/registry/src/x/winit-0.30.13/src/platform_impl/linux/mod.rs:765: \
             neither WAYLAND_DISPLAY nor WAYLAND_SOCKET nor DISPLAY is set."
                .into(),
        );
        assert_eq!(d.code, "E0502");
        assert_eq!(
            d.message,
            "could not open a window: neither WAYLAND_DISPLAY nor WAYLAND_SOCKET nor DISPLAY is set."
        );
        assert_eq!(no_window("no luck".into()).message, "could not open a window: no luck");
    }

    /// The window names winit's keys as a browser does, so both players use
    /// one table of bindings.
    #[test]
    fn winits_key_names_are_the_codes_the_bindings_use() {
        let bound = [
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::KeyW,
            KeyCode::KeyS,
            KeyCode::KeyA,
            KeyCode::KeyD,
            KeyCode::Enter,
            KeyCode::NumpadEnter,
            KeyCode::Space,
            KeyCode::Escape,
            KeyCode::KeyQ,
            KeyCode::KeyE,
            KeyCode::Minus,
            KeyCode::NumpadSubtract,
            KeyCode::Equal,
            KeyCode::NumpadAdd,
            KeyCode::KeyV,
            KeyCode::Home,
        ];
        let names: Vec<String> = bound.iter().map(|k| code_name(*k)).collect();
        let table: Vec<&str> = crate::input::KEYS.iter().map(|(k, _, _)| *k).collect();
        assert_eq!(names, table);
        assert_eq!(button_of(MouseButton::Back), Button::Other);
    }
}
