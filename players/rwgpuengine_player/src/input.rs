//! Keys, clicks, touches and gestures, turned into the engine's input.
//!
//! The game's input is the CPU web player's (`web/player.js`), so a player
//! at the keyboard, a finger on a tablet and a playtest all give the same
//! [`LiveInput`]: arrows or W A S D, Enter or Space, Escape, a tap, a swipe.
//! The camera gets keys and buttons the game does not use, and every
//! gesture is a [`CameraMove`], the move a playtest's `camera = { ... }`
//! makes.
//!
//! The window (winit) and the browser page both feed this one table and
//! this one state, so their bindings cannot drift apart: a key is named by
//! its W3C `code` (`KeyboardEvent.code` in a browser, the same name as
//! winit's `KeyCode`), a button by [`Button`].

use std::collections::VecDeque;

use rwgpuengine_data::{Action, CameraMove};
use rwgpuengine_runtime::LiveInput;
use rwgpuengine_sim::ActionSet;

/// What a key does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Does {
    /// A game action.
    Act(Action),
    /// Turn the camera by these degrees.
    Turn(f32),
    /// Multiply the zoom by this.
    Zoom(f32),
    /// Switch between `iso` and `perspective`.
    View,
    /// Back to the scene's own camera.
    Home,
}

/// Every key a player knows: its W3C `code`, the word `rwgpu help play`
/// uses for it, and what it does.
pub const KEYS: &[(&str, &str, Does)] = &[
    ("ArrowUp", "arrows", Does::Act(Action::Up)),
    ("ArrowDown", "arrows", Does::Act(Action::Down)),
    ("ArrowLeft", "arrows", Does::Act(Action::Left)),
    ("ArrowRight", "arrows", Does::Act(Action::Right)),
    ("KeyW", "W", Does::Act(Action::Up)),
    ("KeyS", "S", Does::Act(Action::Down)),
    ("KeyA", "A", Does::Act(Action::Left)),
    ("KeyD", "D", Does::Act(Action::Right)),
    ("Enter", "Enter", Does::Act(Action::Confirm)),
    ("NumpadEnter", "Enter", Does::Act(Action::Confirm)),
    ("Space", "Space", Does::Act(Action::Confirm)),
    ("Escape", "Escape", Does::Act(Action::Cancel)),
    ("KeyQ", "Q", Does::Turn(-90.0)),
    ("KeyE", "E", Does::Turn(90.0)),
    ("Minus", "-", Does::Zoom(1.0 / ZOOM_KEY)),
    ("NumpadSubtract", "-", Does::Zoom(1.0 / ZOOM_KEY)),
    ("Equal", "=", Does::Zoom(ZOOM_KEY)),
    ("NumpadAdd", "=", Does::Zoom(ZOOM_KEY)),
    ("KeyV", "V", Does::View),
    ("Home", "Home", Does::Home),
];

/// Is this key one the player binds (so a page should not scroll for it)?
pub fn bound(code: &str) -> bool {
    KEYS.iter().any(|(k, _, _)| *k == code)
}

/// The zoom of one press of - or =.
pub const ZOOM_KEY: f32 = 1.25;
/// The zoom of one notch of a mouse wheel.
pub const ZOOM_WHEEL: f32 = 1.1;
/// Scrolled pixels (a trackpad's smooth scrolling) that make one notch.
pub const WHEEL_NOTCH_PX: f64 = 40.0;
/// Degrees the camera turns for a right-drag of one frame pixel.
pub const TURN_PER_PX: f32 = 0.5;
/// A press and release this far apart, in frame pixels, is a swipe, not a
/// tap (the web player's rule).
pub const SWIPE_PX: f32 = 12.0;

/// A camera move that changes nothing, to build moves from.
pub const STILL: CameraMove = CameraMove { view: None, turn: 0.0, zoom: 1.0, pan: [0.0, 0.0] };

/// A mouse button (or a pen's tip and barrel button).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Button {
    Left,
    Right,
    Middle,
    Other,
}

/// What a pointer gesture asks of the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    Move(CameraMove),
    /// Pan so the ground follows the pointer by this many frame pixels.
    Pan([f32; 2]),
    /// Something that needs the session: [`Does::View`] or [`Does::Home`].
    Key(Does),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    Turn,
    Pan,
}

/// The input gathered between two steps.
#[derive(Clone, Debug, Default)]
pub struct Input {
    /// Actions pressed since the last step.
    pressed: ActionSet,
    /// Actions held down now.
    held: ActionSet,
    /// Taps waiting, in frame pixels: one per step.
    taps: VecDeque<(f32, f32)>,
    /// The pointer, in window pixels.
    cursor: Option<[f64; 2]>,
    /// Where the left button (or the one finger) went down, in frame pixels
    /// (`None` on a bar, or once a second finger came).
    left: Option<[f32; 2]>,
    /// A right or middle drag, and the last pointer position it was at.
    drag: Option<(Drag, [f64; 2])>,
    shift: bool,
    /// Smooth scrolling not yet a whole notch.
    scrolled: f64,
    /// Fingers on a touch screen, at most two, in window pixels.
    fingers: Vec<(u32, [f64; 2])>,
}

impl Input {
    /// The input for the next step: actions pressed since the last one, those
    /// held, and the next tap.
    pub fn take_step(&mut self) -> LiveInput {
        let live = LiveInput {
            pressed: self.pressed,
            down: self.held.union(self.pressed),
            taps: self.taps.pop_front().into_iter().collect(),
        };
        self.pressed = ActionSet::EMPTY;
        live
    }

    /// A key, by its W3C `code`, went down (`down`) or up. Returns what it
    /// asks of the camera.
    pub fn key(&mut self, code: &str, down: bool, repeat: bool) -> Option<Gesture> {
        let does = KEYS.iter().find(|(k, _, _)| *k == code)?.2;
        match does {
            Does::Act(a) => {
                if down {
                    if !repeat {
                        self.pressed.insert(a);
                    }
                    self.held.insert(a);
                } else {
                    self.held = ActionSet::from_bits(self.held.bits() & !a.bit());
                }
                None
            }
            // Holding - or = keeps zooming; a turn or a switch is one per press.
            Does::Zoom(z) if down => Some(Gesture::Move(CameraMove { zoom: z, ..STILL })),
            Does::Turn(t) if down && !repeat => Some(Gesture::Move(CameraMove { turn: t, ..STILL })),
            Does::View | Does::Home if down && !repeat => Some(Gesture::Key(does)),
            _ => None,
        }
    }

    /// Where the pointer is, in window pixels.
    pub fn cursor(&self) -> Option<[f64; 2]> {
        self.cursor
    }

    pub fn shift(&mut self, held: bool) {
        self.shift = held;
    }

    /// The window lost the keyboard: nothing is held any more.
    pub fn lost_focus(&mut self) {
        self.held = ActionSet::EMPTY;
        self.drag = None;
        self.left = None;
        self.shift = false;
        self.fingers.clear();
    }

    /// The pointer moved to `at` (window pixels); `scale` is window pixels
    /// per frame pixel.
    pub fn moved(&mut self, at: [f64; 2], scale: f64) -> Option<Gesture> {
        self.cursor = Some(at);
        let (drag, last) = self.drag?;
        self.drag = Some((drag, at));
        let d = [((at[0] - last[0]) / scale) as f32, ((at[1] - last[1]) / scale) as f32];
        match drag {
            // The side nearest the viewer follows the pointer.
            Drag::Turn => (d[0] != 0.0).then_some(Gesture::Move(CameraMove { turn: -d[0] * TURN_PER_PX, ..STILL })),
            Drag::Pan => (d != [0.0, 0.0]).then_some(Gesture::Pan(d)),
        }
    }

    /// A mouse button went down or up. `frame_at` is the frame pixel under
    /// the pointer (`None` on a bar).
    pub fn button(&mut self, button: Button, down: bool, frame_at: Option<[f32; 2]>) {
        let cursor = self.cursor.unwrap_or_default();
        match (button, down) {
            (Button::Left, true) => self.left = frame_at,
            (Button::Left, false) => {
                let (Some(start), Some(end)) = (self.left.take(), frame_at) else { return };
                let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
                if dx * dx + dy * dy < SWIPE_PX * SWIPE_PX {
                    self.taps.push_back((start[0].floor(), start[1].floor()));
                } else {
                    // Along the iso axes: up-right is up (-y), down-left down
                    // (+y), up-left left (-x), down-right right (+x).
                    let a = match (dy < 0.0, dx > 0.0) {
                        (true, true) => Action::Up,
                        (true, false) => Action::Left,
                        (false, true) => Action::Right,
                        (false, false) => Action::Down,
                    };
                    self.pressed.insert(a);
                }
            }
            (Button::Right, true) => {
                self.drag = Some((if self.shift { Drag::Pan } else { Drag::Turn }, cursor));
            }
            (Button::Middle, true) => self.drag = Some((Drag::Pan, cursor)),
            (Button::Right | Button::Middle, false) => self.drag = None,
            _ => {}
        }
    }

    /// The wheel turned by `lines` notches (up is positive) or scrolled by
    /// `pixels` (a trackpad).
    pub fn wheel(&mut self, lines: f64, pixels: f64) -> Option<Gesture> {
        self.scrolled += lines * WHEEL_NOTCH_PX + pixels;
        let notches = (self.scrolled / WHEEL_NOTCH_PX).trunc();
        if notches == 0.0 {
            return None;
        }
        self.scrolled -= notches * WHEEL_NOTCH_PX;
        let mut zoom = 1.0f32;
        let step = if notches > 0.0 { ZOOM_WHEEL } else { 1.0 / ZOOM_WHEEL };
        for _ in 0..(notches.abs() as i64).min(10) {
            zoom *= step;
        }
        Some(Gesture::Move(CameraMove { zoom, ..STILL }))
    }

    /// A pinch on a trackpad: `delta` above 0 zooms in.
    pub fn pinch(&mut self, delta: f64) -> Option<Gesture> {
        let zoom = (1.0 + delta).clamp(0.5, 2.0) as f32;
        (delta.is_finite() && zoom != 1.0).then_some(Gesture::Move(CameraMove { zoom, ..STILL }))
    }

    /// A finger went down on a touch screen at `at` (window pixels) over
    /// `frame_at`. One finger taps and swipes as the left button does; a
    /// second one turns the touch into a pinch.
    pub fn finger_down(&mut self, id: u32, at: [f64; 2], frame_at: Option<[f32; 2]>) {
        if self.fingers.len() >= 2 || self.fingers.iter().any(|(f, _)| *f == id) {
            return;
        }
        self.fingers.push((id, at));
        if self.fingers.len() == 1 {
            self.button(Button::Left, true, frame_at);
        } else {
            self.left = None;
        }
    }

    /// A finger moved. Two fingers zoom by how much nearer or further apart
    /// they came: the pinch of the table, on a touch screen.
    pub fn finger_moved(&mut self, id: u32, at: [f64; 2]) -> Option<Gesture> {
        let before = self.spread();
        let finger = self.fingers.iter_mut().find(|(f, _)| *f == id)?;
        finger.1 = at;
        let (before, after) = (before?, self.spread()?);
        (before > 0.0).then(|| self.pinch(after / before - 1.0)).flatten()
    }

    /// A finger went up over `frame_at` (`None` on a bar, or when the touch
    /// was cancelled): the end of a tap or a swipe if it was the only one.
    pub fn finger_up(&mut self, id: u32, frame_at: Option<[f32; 2]>) {
        let Some(i) = self.fingers.iter().position(|(f, _)| *f == id) else { return };
        self.fingers.remove(i);
        if self.fingers.is_empty() {
            self.button(Button::Left, false, frame_at);
        }
    }

    /// How far apart two fingers are, in window pixels.
    fn spread(&self) -> Option<f64> {
        let [(_, a), (_, b)] = self.fingers.as_slice() else { return None };
        Some(((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(s: ActionSet) -> u32 {
        s.bits()
    }

    #[test]
    fn keys_give_the_web_players_actions() {
        let mut i = Input::default();
        assert_eq!(i.key("KeyW", true, false), None);
        assert_eq!(i.key("KeyW", true, true), None); // the key repeats: still one press
        let s = i.take_step();
        assert_eq!((bits(s.pressed), bits(s.down)), (Action::Up.bit(), Action::Up.bit()));
        // Held: down, not pressed again.
        let s = i.take_step();
        assert_eq!((bits(s.pressed), bits(s.down)), (0, Action::Up.bit()));
        // Pressed and let go between two steps: the step still sees it.
        i.key("KeyW", false, false);
        i.key("Space", true, false);
        i.key("Space", false, false);
        let s = i.take_step();
        assert_eq!((bits(s.pressed), bits(s.down)), (Action::Confirm.bit(), Action::Confirm.bit()));
        assert_eq!(bits(i.take_step().down), 0);
        // Escape is the game's: cancel.
        i.key("Escape", true, false);
        assert_eq!(bits(i.take_step().pressed), Action::Cancel.bit());
        // Holding a key and losing the window lets it go.
        i.key("ArrowLeft", true, false);
        i.lost_focus();
        i.take_step();
        assert_eq!(bits(i.take_step().down), 0);
        // A key nobody binds does nothing, and a page may let it scroll.
        assert_eq!(i.key("KeyZ", true, false), None);
        assert!(i.take_step().pressed.is_empty());
        assert!(bound("ArrowUp") && bound("Home") && !bound("KeyZ") && !bound("Tab"));
    }

    #[test]
    fn a_click_is_a_tap_and_a_drag_is_a_swipe() {
        let mut i = Input::default();
        i.button(Button::Left, true, Some([416.7, 272.2]));
        i.button(Button::Left, false, Some([420.0, 275.0]));
        i.button(Button::Left, true, Some([100.0, 100.0]));
        i.button(Button::Left, false, Some([101.0, 101.0]));
        // One tap per step, at the pixel where the button went down.
        assert_eq!(i.take_step().taps, vec![(416.0, 272.0)]);
        assert_eq!(i.take_step().taps, vec![(100.0, 100.0)]);
        assert!(i.take_step().taps.is_empty());
        for (to, want) in [
            ([130.0, 80.0], Action::Up),
            ([80.0, 80.0], Action::Left),
            ([130.0, 120.0], Action::Right),
            ([80.0, 120.0], Action::Down),
        ] {
            i.button(Button::Left, true, Some([100.0, 100.0]));
            i.button(Button::Left, false, Some(to));
            let s = i.take_step();
            assert_eq!(bits(s.pressed), want.bit(), "swipe to {to:?}");
            assert!(s.taps.is_empty());
        }
        // Pressed on a bar: nothing.
        i.button(Button::Left, true, None);
        i.button(Button::Left, false, Some([10.0, 10.0]));
        let s = i.take_step();
        assert!(s.taps.is_empty() && s.pressed.is_empty());
    }

    #[test]
    fn camera_gestures_are_camera_moves() {
        let mut i = Input::default();
        assert_eq!(i.key("KeyE", true, false), Some(Gesture::Move(CameraMove { turn: 90.0, ..STILL })));
        assert_eq!(i.key("KeyQ", true, true), None);
        assert_eq!(i.key("Equal", true, true), Some(Gesture::Move(CameraMove { zoom: ZOOM_KEY, ..STILL })));
        assert_eq!(i.key("KeyV", true, false), Some(Gesture::Key(Does::View)));
        assert_eq!(i.key("Home", true, false), Some(Gesture::Key(Does::Home)));
        // Camera keys are not game actions.
        assert!(i.take_step().pressed.is_empty());
        // Right-drag turns; at twice the size, 20 window pixels are 10 frame pixels.
        i.moved([100.0, 100.0], 2.0);
        i.button(Button::Right, true, None);
        assert_eq!(i.moved([120.0, 130.0], 2.0), Some(Gesture::Move(CameraMove { turn: -5.0, ..STILL })));
        i.button(Button::Right, false, None);
        assert_eq!(i.moved([140.0, 130.0], 2.0), None);
        // Middle-drag and Shift + right-drag pan.
        i.button(Button::Middle, true, None);
        assert_eq!(i.moved([150.0, 110.0], 2.0), Some(Gesture::Pan([5.0, -10.0])));
        i.button(Button::Middle, false, None);
        i.shift(true);
        i.button(Button::Right, true, None);
        assert_eq!(i.moved([160.0, 110.0], 2.0), Some(Gesture::Pan([5.0, 0.0])));
        i.button(Button::Right, false, None);
        // The wheel: a notch is a step of 1.1; smooth scrolling adds up to notches.
        assert_eq!(i.wheel(1.0, 0.0), Some(Gesture::Move(CameraMove { zoom: ZOOM_WHEEL, ..STILL })));
        assert_eq!(i.wheel(0.0, -25.0), None);
        assert_eq!(i.wheel(0.0, -25.0), Some(Gesture::Move(CameraMove { zoom: 1.0 / ZOOM_WHEEL, ..STILL })));
        assert_eq!(i.pinch(0.25), Some(Gesture::Move(CameraMove { zoom: 1.25, ..STILL })));
        assert_eq!(i.pinch(f64::NAN), None);
    }

    #[test]
    fn one_finger_taps_and_swipes_and_two_fingers_pinch() {
        let mut i = Input::default();
        // One finger, as the left button.
        i.finger_down(7, [833.0, 545.0], Some([416.7, 272.2]));
        i.finger_moved(7, [835.0, 546.0]);
        i.finger_up(7, Some([417.5, 272.7]));
        assert_eq!(i.take_step().taps, vec![(416.0, 272.0)]);
        i.finger_down(7, [200.0, 200.0], Some([100.0, 100.0]));
        i.finger_up(7, Some([130.0, 80.0]));
        assert_eq!(bits(i.take_step().pressed), Action::Up.bit());
        // Two fingers: no tap, and their spread is the zoom.
        i.finger_down(1, [100.0, 100.0], Some([50.0, 50.0]));
        i.finger_down(2, [200.0, 100.0], Some([100.0, 50.0]));
        assert_eq!(i.finger_moved(2, [250.0, 100.0]), Some(Gesture::Move(CameraMove { zoom: 1.5, ..STILL })));
        assert_eq!(i.finger_moved(1, [100.0, 100.0]), None);
        assert_eq!(i.finger_moved(9, [0.0, 0.0]), None);
        // A third finger is ignored; lifting them leaves no tap behind.
        i.finger_down(3, [0.0, 0.0], Some([0.0, 0.0]));
        i.finger_up(2, Some([125.0, 50.0]));
        i.finger_up(1, Some([50.0, 50.0]));
        let s = i.take_step();
        assert!(s.taps.is_empty() && s.pressed.is_empty());
        // A cancelled touch (no frame point) is no tap either.
        i.finger_down(4, [10.0, 10.0], Some([5.0, 5.0]));
        i.finger_up(4, None);
        assert!(i.take_step().taps.is_empty());
    }

    /// The web player's page says what every binding does, in the words of
    /// `rwgpu help play`.
    #[test]
    fn the_web_page_names_every_key() {
        let page = include_str!("../web/index.html");
        for (code, word, _) in KEYS {
            let found = [" ", ",", ";", "."].iter().any(|end| page.contains(&format!(" {word}{end}")));
            assert!(found, "{code} is bound but the web page's help does not say `{word}`");
        }
        for gesture in ["right-drag", "middle-drag", "Shift + right-drag", "wheel", "pinch", "click", "swipe"] {
            assert!(page.contains(gesture), "the web page's help does not say `{gesture}`");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rwgpu_help_play_names_every_key() {
        let help = rwgpuengine_cli::args::PLAY_DETAILS;
        for (code, word, _) in KEYS {
            let spaced = format!(" {word} ");
            let found =
                help.contains(&spaced) || help.contains(&format!(" {word},")) || help.contains(&format!(" {word};"));
            assert!(found, "{code} is bound but `rwgpu help play` does not say `{word}`");
        }
        for gesture in ["right-drag", "middle-drag", "Shift + right-drag", "wheel", "pinch", "click", "drag"] {
            assert!(help.contains(gesture), "`rwgpu help play` does not say `{gesture}`");
        }
    }
}
