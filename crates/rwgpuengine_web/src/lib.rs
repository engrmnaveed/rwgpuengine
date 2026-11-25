//! `rwgpuengine_web` — rwgpu in the browser.
//!
//! Compiled to `wasm32-unknown-unknown` (`cargo xtask web`) and driven by
//! `web/player.js` through the small C ABI below. No wasm-bindgen: the
//! boundary is a handful of functions passing integers, floats and
//! (pointer, length) pairs into linear memory (ADR 0008).
//!
//! The same runtime runs natively and here, so a scripted run in the
//! browser reaches the same state hash and the same pixels as
//! `rwgpu run` / `rwgpu shot`.
//!
//! Text results (errors, JSON, hashes) are written to an output buffer read
//! with `rwgpu_out_ptr` / `rwgpu_out_len`.

// The C ABI boundary needs raw pointers into linear memory.
#![allow(unsafe_code)]

use std::cell::RefCell;

use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_core::json::Json;
use rwgpuengine_data::Vfs;
use rwgpuengine_runtime::{DataOnly, InspectOptions, LiveInput, Presenter, Session, SessionConfig};
use rwgpuengine_sim::{ActionSet, dump};

#[derive(Default)]
struct State {
    vfs: Option<Vfs>,
    session: Option<Session>,
    out: Vec<u8>,
    /// The picture the page shows (`rwgpu_present`, `rwgpu_render`).
    presenter: Presenter,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

fn set_out(s: &mut State, text: String) -> u32 {
    s.out = text.into_bytes();
    s.out.len() as u32
}

fn diags_json(diags: &[Diagnostic]) -> String {
    Json::object()
        .with("ok", false)
        .with("diagnostics", diags.iter().map(Diagnostic::to_json).collect::<Vec<_>>())
        .to_compact()
}

/// Read a UTF-8 string the caller placed in linear memory.
///
/// # Safety
/// `ptr..ptr+len` must be a valid, initialized region (allocated with
/// [`rwgpu_alloc`] and filled by the caller).
unsafe fn read_str(ptr: *const u8, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    // SAFETY: guaranteed by the caller (see above).
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// Allocate `len` bytes for the caller to fill (e.g. with a bundle).
#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let ptr = v.as_mut_ptr();
    std::mem::forget(v);
    ptr
}

/// Free memory from [`rwgpu_alloc`].
///
/// # Safety
/// `ptr` and `len` must come from one `rwgpu_alloc(len)` call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rwgpu_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        // SAFETY: allocated by rwgpu_alloc with capacity len.max(1).
        drop(unsafe { Vec::from_raw_parts(ptr, 0, len.max(1)) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_out_ptr() -> *const u8 {
    with(|s| s.out.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_out_len() -> u32 {
    with(|s| s.out.len() as u32)
}

/// Writes the engine version to the output buffer.
#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_version() -> u32 {
    with(|s| set_out(s, rwgpuengine_core::ENGINE_VERSION.to_string()))
}

/// Load a project bundle (see `Vfs::to_bundle`). Returns 0 on success, 1 on
/// error (message in the output buffer).
///
/// # Safety
/// See [`read_str`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rwgpu_set_bundle(ptr: *const u8, len: usize) -> u32 {
    // SAFETY: forwarded caller contract.
    let text = unsafe { read_str(ptr, len) };
    with(|s| match Vfs::from_bundle(&text) {
        Ok(vfs) => {
            s.vfs = Some(vfs);
            s.session = None;
            s.presenter.reset();
            0
        }
        Err(e) => {
            set_out(s, Json::object().with("ok", false).with("error", e).to_compact());
            1
        }
    })
}

/// Start a session. Empty strings mean "default" (main scene, no script).
/// Returns 0 on success; on failure the output buffer holds
/// `{"ok": false, "diagnostics": [...]}`.
///
/// # Safety
/// See [`read_str`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rwgpu_start(
    scene_ptr: *const u8,
    scene_len: usize,
    input_ptr: *const u8,
    input_len: usize,
) -> u32 {
    // SAFETY: forwarded caller contract.
    let (scene, input) = unsafe { (read_str(scene_ptr, scene_len), read_str(input_ptr, input_len)) };
    with(|s| {
        let Some(vfs) = &s.vfs else {
            set_out(s, Json::object().with("ok", false).with("error", "no bundle loaded").to_compact());
            return 1;
        };
        let cfg = SessionConfig {
            scene: Some(scene).filter(|x| !x.is_empty()),
            script: Some(input).filter(|x| !x.is_empty()),
            seed: None,
        };
        s.presenter.reset();
        match Session::load(vfs, &cfg, Box::new(DataOnly)) {
            Ok(session) => {
                let warnings = diags_json(&session.warnings);
                s.session = Some(session);
                set_out(s, warnings);
                0
            }
            Err(diags) => {
                let text = diags_json(&diags);
                set_out(s, text);
                s.session = None;
                1
            }
        }
    })
}

/// One fixed step. `pressed`/`down` are action bit masks (up=1, down=2,
/// left=4, right=8, confirm=16, cancel=32); `tap` != 0 adds a tap at
/// (tap_x, tap_y) in frame pixels.
#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_step(pressed: u32, down: u32, tap: u32, tap_x: f32, tap_y: f32) {
    with(|s| {
        if let Some(session) = s.session.as_mut() {
            let live = LiveInput {
                pressed: ActionSet::from_bits(pressed),
                down: ActionSet::from_bits(down),
                taps: if tap != 0 { vec![(tap_x, tap_y)] } else { vec![] },
            };
            session.step(&live);
        }
    })
}

/// Step (with scripted input only) until `frame`.
#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_run_to(frame: u32) {
    with(|s| {
        if let Some(session) = s.session.as_mut() {
            session.run_to(frame as u64);
        }
    })
}

/// Paint the current frame; returns a pointer to width*height*4 RGBA bytes,
/// valid until the next `rwgpu_render`, `rwgpu_present` or `rwgpu_start`.
/// It always paints. A page that draws every frame calls `rwgpu_present`.
#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_render() -> *const u8 {
    with(|s| {
        s.presenter.reset();
        if let Some(session) = &s.session {
            s.presenter.present(session);
        }
        s.presenter.pixels().as_ptr()
    })
}

/// Bring the pixel buffer up to date, repainting only if the picture
/// changed. Returns 1 when the pixels at `rwgpu_pixels` are new and the
/// page should show them, 0 when they are the ones it already has. The first
/// call after `rwgpu_start` returns 1.
#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_present() -> u32 {
    with(|s| match &s.session {
        Some(session) => s.presenter.present(session) as u32,
        None => 0,
    })
}

/// Pointer to the pixel buffer `rwgpu_present` keeps: width*height*4 RGBA
/// bytes, valid until the next `rwgpu_render`, `rwgpu_present` or `rwgpu_start`.
#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_pixels() -> *const u8 {
    with(|s| s.presenter.pixels().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_width() -> u32 {
    with(|s| s.session.as_ref().map_or(0, |x| x.view.width))
}

#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_height() -> u32 {
    with(|s| s.session.as_ref().map_or(0, |x| x.view.height))
}

#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_frame() -> u32 {
    with(|s| s.session.as_ref().map_or(0, |x| x.frame() as u32))
}

#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_tick_rate() -> u32 {
    with(|s| s.session.as_ref().map_or(60, |x| x.project.sim.tick_rate))
}

/// Frames the loaded input script asks for (0 if none).
#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_script_frames() -> u32 {
    with(|s| s.session.as_ref().and_then(|x| x.script_frames()).unwrap_or(0) as u32)
}

/// Writes the state hash (16 hex digits) to the output buffer.
#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_hash() -> u32 {
    with(|s| {
        let h = s.session.as_ref().map(Session::hash_hex).unwrap_or_default();
        set_out(s, h)
    })
}

/// Writes the image hash of the current frame to the output buffer. It is
/// the hash of the painted pixels, which equal the reference rasterizer's
/// (`rwgpu shot`); painting keeps that rasterizer and the frame report out
/// of the web module (ADR 0018).
#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_image_hash() -> u32 {
    with(|s| {
        let h = match &s.session {
            Some(x) => {
                let mut rgba = Vec::new();
                x.paint(&mut rgba);
                rwgpuengine_core::hash::hex(rwgpuengine_core::hash::fnv1a64(&rgba))
            }
            None => String::new(),
        };
        set_out(s, h)
    })
}

/// Writes the world JSON (like `rwgpu inspect --json`) to the output buffer.
#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_inspect() -> u32 {
    with(|s| {
        let j = s.session.as_ref().map(|x| x.inspect(&InspectOptions::default())).unwrap_or(Json::Null);
        set_out(s, j.to_compact())
    })
}

/// Writes events from frame `since` on, as a JSON array.
#[unsafe(no_mangle)]
pub extern "C" fn rwgpu_events(since: u32) -> u32 {
    with(|s| {
        let j = match &s.session {
            Some(x) => Json::Arr(
                x.world
                    .events
                    .all()
                    .iter()
                    .filter(|e| e.frame >= since as u64)
                    .map(|e| dump::event_json(&x.world, e))
                    .collect(),
            ),
            None => Json::array(),
        };
        set_out(s, j.to_compact())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out() -> String {
        with(|s| String::from_utf8(s.out.clone()).unwrap())
    }

    #[test]
    fn abi_round_trip_matches_the_native_runtime() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/iso_garden");
        let vfs = Vfs::from_dir(&root).unwrap();
        let bundle = vfs.to_bundle();
        let input = "tests/walkthrough.toml";
        // SAFETY: pointers come from live Rust strings.
        unsafe {
            assert_eq!(rwgpu_set_bundle(bundle.as_ptr(), bundle.len()), 0);
            assert_eq!(rwgpu_start(std::ptr::null(), 0, input.as_ptr(), input.len()), 0, "{}", out());
        }
        assert_eq!(rwgpu_script_frames(), 300);
        rwgpu_run_to(300);
        rwgpu_hash();
        let web_hash = out();

        let cfg = SessionConfig { script: Some(input.into()), ..Default::default() };
        let mut native = Session::load(&vfs, &cfg, Box::new(DataOnly)).unwrap();
        native.run_to(300);
        assert_eq!(web_hash, native.hash_hex());
        assert_eq!(rwgpu_width(), 640);
        assert!(!rwgpu_render().is_null());
    }

    #[test]
    fn present_repaints_only_when_the_picture_changes() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/iso_garden");
        let vfs = Vfs::from_dir(&root).unwrap();
        let bundle = vfs.to_bundle();
        let input = "tests/walkthrough.toml";
        let start = || {
            // SAFETY: pointers come from live Rust strings.
            unsafe {
                assert_eq!(rwgpu_set_bundle(bundle.as_ptr(), bundle.len()), 0);
                assert_eq!(rwgpu_start(std::ptr::null(), 0, input.as_ptr(), input.len()), 0, "{}", out());
            }
        };
        let shown = || with(|s| s.presenter.pixels().to_vec());
        let reference = || with(|s| s.session.as_ref().unwrap().render().0.rgba);
        assert_eq!(rwgpu_present(), 0, "no session yet");

        start();
        assert_eq!((rwgpu_present(), rwgpu_present()), (1, 0), "first frame once");
        assert!(shown() == reference());
        assert_eq!(rwgpu_pixels(), with(|s| s.presenter.pixels().as_ptr()));

        // The walkthrough's first input is at frame 10. Over the first 40
        // frames some change the picture and most do not; a frame may be
        // skipped only if the reference picture is the same.
        let first = shown();
        let (mut repaints, mut skips) = (0, 0);
        for frame in 1..=40 {
            let before = reference();
            rwgpu_run_to(frame);
            if rwgpu_present() == 1 {
                repaints += 1;
                assert!(shown() == reference(), "frame {frame}");
            } else {
                skips += 1;
                assert!(reference() == before, "frame {frame}: a change was skipped");
            }
        }
        assert!(repaints >= 1 && skips > repaints, "{repaints} repaints, {skips} skips");

        // rwgpu_render always paints, into the same buffer.
        let moved = shown();
        assert!(moved != first);
        assert_eq!(rwgpu_render(), rwgpu_pixels());
        assert!(shown() == moved);
        assert_eq!(rwgpu_present(), 0);

        // A restart shows its first frame again.
        start();
        assert_eq!(rwgpu_present(), 1);
        assert!(shown() != moved && shown() == reference());
    }

    #[test]
    fn start_errors_come_back_as_json() {
        let mut vfs = Vfs::new();
        vfs.insert("rwgpu.toml", "[project]\nname = \"x\"\n");
        let bundle = vfs.to_bundle();
        // SAFETY: pointers come from live Rust strings.
        unsafe {
            assert_eq!(rwgpu_set_bundle(bundle.as_ptr(), bundle.len()), 0);
            assert_eq!(rwgpu_start(std::ptr::null(), 0, std::ptr::null(), 0), 1);
        }
        let j = Json::parse(&out()).unwrap();
        assert_eq!(j.pointer("diagnostics/0/code").and_then(Json::as_str), Some("E0204"));
    }
}
