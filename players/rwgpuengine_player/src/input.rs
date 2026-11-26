//! Keys, clicks and gestures, turned into the engine's input.
//!
//! The game's input is the web player's (`web/player.js`), so a player at
//! the keyboard, a finger on a tablet and a playtest all give the same
//! [`LiveInput`]: arrows or W A S D, Enter or Space, Escape, a tap, a swipe.
//! The camera gets keys and buttons the game does not use, and every
//! gesture is a [`CameraMove`], the move a playtest's `camera = { ... }`
//! makes.

use std::collections::VecDeque;

use rwgpuengine_data::{Action, CameraMove};
use rwgpuengine_runtime::LiveInput;
use rwgpuengine_sim::ActionSet;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

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

/// Every key the player knows, with the word `rwgpu help play` uses for it.
pub const KEYS: &[(KeyCode, &str, Does)] = &[
    (KeyCode::ArrowUp, "arrows", Does::Act(Action::Up)),
    (KeyCode::ArrowDown, "arrows", Does::Act(Action::Down)),
    (KeyCode::ArrowLeft, "arrows", Does::Act(Action::Left)),
    (KeyCode::ArrowRight, "arrows", Does::Act(Action::Right)),
    (KeyCode::KeyW, "W", Does::Act(Action::Up)),
    (KeyCode::KeyS, "S", Does::Act(Action::Down)),
    (KeyCode::KeyA, "A", Does::Act(Action::Left)),
    (KeyCode::KeyD, "D", Does::Act(Action::Right)),
    (KeyCode::Enter, "Enter", Does::Act(Action::Confirm)),
    (KeyCode::NumpadEnter, "Enter", Does::Act(Action::Confirm)),
    (KeyCode::Space, "Space", Does::Act(Action::Confirm)),
    (KeyCode::Escape, "Escape", Does::Act(Action::Cancel)),
    (KeyCode::KeyQ, "Q", Does::Turn(-90.0)),
    (KeyCode::KeyE, "E", Does::Turn(90.0)),
    (KeyCode::Minus, "-", Does::Zoom(1.0 / ZOOM_KEY)),
    (KeyCode::NumpadSubtract, "-", Does::Zoom(1.0 / ZOOM_KEY)),
    (KeyCode::Equal, "=", Does::Zoom(ZOOM_KEY)),
    (KeyCode::NumpadAdd, "=", Does::Zoom(ZOOM_KEY)),
    (KeyCode::KeyV, "V", Does::View),
    (KeyCode::Home, "Home", Does::Home),
];

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
    /// Where the left button went down, in frame pixels (`None` on a bar).
    left: Option<[f32; 2]>,
    /// A right or middle drag, and the last pointer position it was at.
    drag: Option<(Drag, [f64; 2])>,
    shift: bool,
    /// Smooth scrolling not yet a whole notch.
    scrolled: f64,
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

    /// A key went down (`down`) or up. Returns what it asks of the camera.
    pub fn key(&mut self, code: KeyCode, down: bool, repeat: bool) -> Option<Gesture> {
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
    pub fn button(&mut self, button: MouseButton, down: bool, frame_at: Option<[f32; 2]>) {
        let cursor = self.cursor.unwrap_or_default();
        match (button, down) {
            (MouseButton::Left, true) => self.left = frame_at,
            (MouseButton::Left, false) => {
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
            (MouseButton::Right, true) => {
                self.drag = Some((if self.shift { Drag::Pan } else { Drag::Turn }, cursor));
            }
            (MouseButton::Middle, true) => self.drag = Some((Drag::Pan, cursor)),
            (MouseButton::Right | MouseButton::Middle, false) => self.drag = None,
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
        assert_eq!(i.key(KeyCode::KeyW, true, false), None);
        assert_eq!(i.key(KeyCode::KeyW, true, true), None); // the key repeats: still one press
        let s = i.take_step();
        assert_eq!((bits(s.pressed), bits(s.down)), (Action::Up.bit(), Action::Up.bit()));
        // Held: down, not pressed again.
        let s = i.take_step();
        assert_eq!((bits(s.pressed), bits(s.down)), (0, Action::Up.bit()));
        // Pressed and let go between two steps: the step still sees it.
        i.key(KeyCode::KeyW, false, false);
        i.key(KeyCode::Space, true, false);
        i.key(KeyCode::Space, false, false);
        let s = i.take_step();
        assert_eq!((bits(s.pressed), bits(s.down)), (Action::Confirm.bit(), Action::Confirm.bit()));
        assert_eq!(bits(i.take_step().down), 0);
        // Escape is the game's: cancel.
        i.key(KeyCode::Escape, true, false);
        assert_eq!(bits(i.take_step().pressed), Action::Cancel.bit());
        // Holding a key and losing the window lets it go.
        i.key(KeyCode::ArrowLeft, true, false);
        i.lost_focus();
        i.take_step();
        assert_eq!(bits(i.take_step().down), 0);
    }

    #[test]
    fn a_click_is_a_tap_and_a_drag_is_a_swipe() {
        let mut i = Input::default();
        i.button(MouseButton::Left, true, Some([416.7, 272.2]));
        i.button(MouseButton::Left, false, Some([420.0, 275.0]));
        i.button(MouseButton::Left, true, Some([100.0, 100.0]));
        i.button(MouseButton::Left, false, Some([101.0, 101.0]));
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
            i.button(MouseButton::Left, true, Some([100.0, 100.0]));
            i.button(MouseButton::Left, false, Some(to));
            let s = i.take_step();
            assert_eq!(bits(s.pressed), want.bit(), "swipe to {to:?}");
            assert!(s.taps.is_empty());
        }
        // Pressed on a bar: nothing.
        i.button(MouseButton::Left, true, None);
        i.button(MouseButton::Left, false, Some([10.0, 10.0]));
        let s = i.take_step();
        assert!(s.taps.is_empty() && s.pressed.is_empty());
    }

    #[test]
    fn camera_gestures_are_camera_moves() {
        let mut i = Input::default();
        assert_eq!(i.key(KeyCode::KeyE, true, false), Some(Gesture::Move(CameraMove { turn: 90.0, ..STILL })));
        assert_eq!(i.key(KeyCode::KeyQ, true, true), None);
        assert_eq!(i.key(KeyCode::Equal, true, true), Some(Gesture::Move(CameraMove { zoom: ZOOM_KEY, ..STILL })));
        assert_eq!(i.key(KeyCode::KeyV, true, false), Some(Gesture::Key(Does::View)));
        assert_eq!(i.key(KeyCode::Home, true, false), Some(Gesture::Key(Does::Home)));
        // Camera keys are not game actions.
        assert!(i.take_step().pressed.is_empty());
        // Right-drag turns; at twice the size, 20 window pixels are 10 frame pixels.
        i.moved([100.0, 100.0], 2.0);
        i.button(MouseButton::Right, true, None);
        assert_eq!(i.moved([120.0, 130.0], 2.0), Some(Gesture::Move(CameraMove { turn: -5.0, ..STILL })));
        i.button(MouseButton::Right, false, None);
        assert_eq!(i.moved([140.0, 130.0], 2.0), None);
        // Middle-drag and Shift + right-drag pan.
        i.button(MouseButton::Middle, true, None);
        assert_eq!(i.moved([150.0, 110.0], 2.0), Some(Gesture::Pan([5.0, -10.0])));
        i.button(MouseButton::Middle, false, None);
        i.shift(true);
        i.button(MouseButton::Right, true, None);
        assert_eq!(i.moved([160.0, 110.0], 2.0), Some(Gesture::Pan([5.0, 0.0])));
        i.button(MouseButton::Right, false, None);
        // The wheel: a notch is a step of 1.1; smooth scrolling adds up to notches.
        assert_eq!(i.wheel(1.0, 0.0), Some(Gesture::Move(CameraMove { zoom: ZOOM_WHEEL, ..STILL })));
        assert_eq!(i.wheel(0.0, -25.0), None);
        assert_eq!(i.wheel(0.0, -25.0), Some(Gesture::Move(CameraMove { zoom: 1.0 / ZOOM_WHEEL, ..STILL })));
        assert_eq!(i.pinch(0.25), Some(Gesture::Move(CameraMove { zoom: 1.25, ..STILL })));
        assert_eq!(i.pinch(f64::NAN), None);
    }

    #[test]
    fn rwgpu_help_play_names_every_key() {
        let help = rwgpuengine_cli::args::PLAY_DETAILS;
        for (code, word, _) in KEYS {
            let spaced = format!(" {word} ");
            let found =
                help.contains(&spaced) || help.contains(&format!(" {word},")) || help.contains(&format!(" {word};"));
            assert!(found, "{code:?} is bound but `rwgpu help play` does not say `{word}`");
        }
        for gesture in ["right-drag", "middle-drag", "Shift + right-drag", "wheel", "pinch", "click", "drag"] {
            assert!(help.contains(gesture), "`rwgpu help play` does not say `{gesture}`");
        }
    }
}
