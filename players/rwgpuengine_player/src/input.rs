//! Keys, clicks, touches and gestures, turned into the engine's input.
//!
//! The game's input is the CPU web player's (`web/player.js`), so a player
//! at the keyboard, a finger on a tablet and a playtest all give the same
//! [`LiveInput`]: arrows or W A S D, Enter or Space, Escape, a tap, a swipe.
//! The camera gets keys, buttons and gestures the game does not use, and
//! every one of them ends in a [`CameraMove`], the move a playtest's
//! `camera = { ... }` makes (ADR 0022, ADR 0030).
//!
//! Every camera move can be made by touch alone:
//!
//! | touch | does |
//! |---|---|
//! | one finger, still | a tap (moved less than [`SWIPE_PX`]) |
//! | one finger, dragged | a pan, the ground following the finger; a swipe in a game with a mover |
//! | two fingers | the ground under them follows them: moved together they pan, apart or together they zoom, twisted past [`Tuning::twist_deg`] they turn |
//! | letting go of a twist | the view settles on a quarter turn: the next one the way it was twisted |
//! | two fingers, still | switch between iso and perspective |
//! | three fingers, still | back to the scene's camera |
//!
//! The window (winit) and the browser page both feed this one table and
//! this one state, so their bindings cannot drift apart: a key is named by
//! its W3C `code` (`KeyboardEvent.code` in a browser, the same name as
//! winit's `KeyCode`), a button by [`Button`], a point by its frame pixel.

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
/// A press and release this far apart, in frame pixels, is a drag (a swipe
/// or a pan), not a tap (the web player's rule). Fingers that move less are
/// still: two or three still fingers are a tap of their own.
pub const SWIPE_PX: f32 = 12.0;

/// A camera move that changes nothing, to build moves from.
pub const STILL: CameraMove = CameraMove { view: None, turn: 0.0, zoom: 1.0, pan: [0.0, 0.0] };

/// How two fingers are told apart as a pan, a pinch and a twist: chosen by
/// measurement (ADR 0030); the check of that measurement makes other values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// The fingers turn the view once they have twisted this far (degrees)
    /// since the second one came down; before that a twist turns nothing.
    /// Letting go of a twist that turned the view settles it on a quarter turn.
    pub twist_deg: f64,
    /// The fingers zoom once their spread has changed by this factor since
    /// the second one came down (1.0: at once).
    pub zoom_after: f64,
    /// When zooming starts after a change of spread, it first catches up
    /// with all of it.
    pub zoom_catches_up: bool,
    /// Whichever of turning and zooming starts first is the only one for the
    /// rest of the touch.
    pub exclusive: bool,
}

impl Default for Tuning {
    fn default() -> Tuning {
        Tuning { twist_deg: TWIST_DEG, zoom_after: ZOOM_AFTER, zoom_catches_up: false, exclusive: false }
    }
}

/// [`Tuning::twist_deg`]: measured in ADR 0030.
pub const TWIST_DEG: f64 = 20.0;
/// [`Tuning::zoom_after`]: measured in ADR 0030.
pub const ZOOM_AFTER: f64 = 1.0;

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
    /// Pan so the ground under the camera's center follows the pointer by
    /// this many frame pixels (a middle-drag).
    Pan([f32; 2]),
    /// Turn by `turn` degrees and zoom by `zoom`, then pan so that the
    /// ground drawn at frame pixel `from` is drawn at `to`: a finger's drag,
    /// two fingers, and a left-drag in a game with no mover.
    Grab {
        from: [f32; 2],
        to: [f32; 2],
        zoom: f32,
        turn: f32,
    },
    /// Two fingers that turned the view let go: settle `quarters` quarter
    /// turns (clockwise on the screen is positive) from the quarter turn
    /// nearest where the view was before it turned by `turned` degrees.
    Settle {
        quarters: i32,
        turned: f32,
    },
    /// Something that needs the session: [`Does::View`] or [`Does::Home`].
    Key(Does),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    Turn,
    Pan,
}

/// The left button, or the one finger, from where it went down.
#[derive(Clone, Copy, Debug)]
struct Left {
    start: [f32; 2],
    /// It went down on the frame, not on a bar: only then can it be a tap.
    inside: bool,
    /// It pans, and has taken the ground this far.
    pan: Option<[f32; 2]>,
}

#[derive(Clone, Copy, Debug)]
struct Finger {
    id: u32,
    down: [f32; 2],
    at: [f32; 2],
}

/// Two fingers moving the camera.
#[derive(Clone, Copy, Debug)]
struct Two {
    ids: [u32; 2],
    /// Where they were when the camera last moved.
    last: [[f32; 2]; 2],
    /// Their spread when the second came down.
    spread0: f64,
    /// How far they twisted since then (degrees, clockwise on the screen).
    twist: f64,
    turning: bool,
    zooming: bool,
    /// How far this touch turned the view.
    turned: f32,
}

/// One touch on a screen: from the first finger down to the last one up.
#[derive(Clone, Debug, Default)]
struct Touch {
    fingers: Vec<Finger>,
    /// The most fingers down at once.
    most: usize,
    /// A finger went [`SWIPE_PX`] or more from where it came down.
    moved: bool,
    two: Option<Two>,
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
    /// The mouse pointer, in frame pixels (outside the frame on a bar).
    cursor: Option<[f32; 2]>,
    left: Option<Left>,
    /// A right or middle drag, and the last pointer position it was at.
    drag: Option<(Drag, [f32; 2])>,
    shift: bool,
    /// Smooth scrolling not yet a whole notch.
    scrolled: f64,
    /// A drag pans, rather than swiping: the game has no mover.
    drag_pans: bool,
    touch: Touch,
    pub tuning: Tuning,
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

    /// Does a drag (one finger, or the left button) pan the camera? It does
    /// in a game with no mover, where a swipe moves nothing
    /// ([`crate::play::drag_pans`]); otherwise a drag is a swipe and two
    /// fingers pan.
    pub fn set_drag_pans(&mut self, pans: bool) {
        self.drag_pans = pans;
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

    pub fn shift(&mut self, held: bool) {
        self.shift = held;
    }

    /// Is a finger on the screen? A window that also gets mouse events made
    /// from touches ignores them meanwhile.
    pub fn touching(&self) -> bool {
        !self.touch.fingers.is_empty()
    }

    /// The window lost the keyboard: nothing is held any more.
    pub fn lost_focus(&mut self) {
        self.held = ActionSet::EMPTY;
        self.drag = None;
        self.left = None;
        self.shift = false;
        self.touch = Touch::default();
    }

    /// The mouse pointer moved to frame pixel `at` (outside the frame on a
    /// bar).
    pub fn moved(&mut self, at: [f32; 2]) -> Option<Gesture> {
        self.cursor = Some(at);
        if self.left.is_some() {
            return self.left_moved(at, self.drag_pans);
        }
        let (drag, last) = self.drag?;
        self.drag = Some((drag, at));
        let d = [at[0] - last[0], at[1] - last[1]];
        match drag {
            // The side nearest the viewer follows the pointer.
            Drag::Turn => (d[0] != 0.0).then_some(Gesture::Move(CameraMove { turn: -d[0] * TURN_PER_PX, ..STILL })),
            Drag::Pan => (d != [0.0, 0.0]).then_some(Gesture::Pan(d)),
        }
    }

    /// A mouse button went down or up at frame pixel `at`; `inside` when that
    /// is on the frame, not on a bar.
    pub fn button(&mut self, button: Button, down: bool, at: [f32; 2], inside: bool) -> Option<Gesture> {
        self.cursor = Some(at);
        match (button, down) {
            (Button::Left, true) => self.left = Some(Left { start: at, inside, pan: None }),
            (Button::Left, false) => return self.left_up(at, inside, self.drag_pans),
            (Button::Right, true) => self.drag = Some((if self.shift { Drag::Pan } else { Drag::Turn }, at)),
            (Button::Middle, true) => self.drag = Some((Drag::Pan, at)),
            (Button::Right | Button::Middle, false) => self.drag = None,
            _ => {}
        }
        None
    }

    /// The left button or the one finger moved: a pan once it is a drag,
    /// when drags pan.
    fn left_moved(&mut self, at: [f32; 2], pans: bool) -> Option<Gesture> {
        let left = self.left.as_mut()?;
        let from = match left.pan {
            Some(from) => from,
            None if pans && far(left.start, at) => left.start,
            None => return None,
        };
        left.pan = Some(at);
        (from != at).then_some(Gesture::Grab { from, to: at, zoom: 1.0, turn: 0.0 })
    }

    /// The left button or the one finger went up: a tap where it went down
    /// if it hardly moved, else the end of a pan or a swipe.
    fn left_up(&mut self, at: [f32; 2], inside: bool, pans: bool) -> Option<Gesture> {
        let left = self.left.take()?;
        if let Some(from) = left.pan {
            return (from != at).then_some(Gesture::Grab { from, to: at, zoom: 1.0, turn: 0.0 });
        }
        if !far(left.start, at) {
            if left.inside && inside {
                self.taps.push_back((left.start[0].floor(), left.start[1].floor()));
            }
            return None;
        }
        if pans {
            return Some(Gesture::Grab { from: left.start, to: at, zoom: 1.0, turn: 0.0 });
        }
        if !left.inside || !inside {
            return None;
        }
        // Along the iso axes: up-right is up (-y), down-left down (+y),
        // up-left left (-x), down-right right (+x).
        let (dx, dy) = (at[0] - left.start[0], at[1] - left.start[1]);
        let a = match (dy < 0.0, dx > 0.0) {
            (true, true) => Action::Up,
            (true, false) => Action::Left,
            (false, true) => Action::Right,
            (false, false) => Action::Down,
        };
        self.pressed.insert(a);
        None
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

    /// A finger came down on a touch screen at frame pixel `at`; `inside`
    /// when that is on the frame. One finger is the left button; a second
    /// makes the touch a camera gesture; a third stops it, and is a tap of
    /// three fingers if none moves.
    pub fn finger_down(&mut self, id: u32, at: [f32; 2], inside: bool) -> Option<Gesture> {
        let t = &mut self.touch;
        if t.fingers.iter().any(|f| f.id == id) {
            return None;
        }
        if t.fingers.is_empty() {
            *t = Touch::default();
        }
        t.fingers.push(Finger { id, down: at, at });
        t.most = t.most.max(t.fingers.len());
        match t.fingers.len() {
            1 => {
                self.left = Some(Left { start: at, inside, pan: None });
                None
            }
            2 => {
                self.left = None;
                let [a, b] = [t.fingers[0], t.fingers[1]];
                let spread0 = spread(a.at, b.at);
                t.two = Some(Two {
                    ids: [a.id, b.id],
                    last: [a.at, b.at],
                    spread0,
                    twist: 0.0,
                    turning: false,
                    zooming: false,
                    turned: 0.0,
                });
                None
            }
            _ => {
                let tuning = self.tuning;
                self.touch.two.take().and_then(|two| settle(two, &tuning))
            }
        }
    }

    /// A finger moved to frame pixel `at`.
    pub fn finger_moved(&mut self, id: u32, at: [f32; 2]) -> Option<Gesture> {
        let t = &mut self.touch;
        let f = t.fingers.iter_mut().find(|f| f.id == id)?;
        f.at = at;
        t.moved |= far(f.down, at);
        match t.fingers.len() {
            // One finger after two keeps panning, in any game.
            1 => {
                let pans = self.drag_pans || self.touch.most > 1;
                self.left_moved(at, pans)
            }
            2 => self.two_moved(),
            _ => None,
        }
    }

    /// A finger went up at frame pixel `at` (`inside` the frame or not): the
    /// end of a tap, a swipe or a pan if it was the only one; a two- or
    /// three-finger tap if it was the last of fingers that did not move.
    pub fn finger_up(&mut self, id: u32, at: [f32; 2], inside: bool) -> Option<Gesture> {
        self.finger_gone(id, Some((at, inside)))
    }

    /// The browser or the window took a finger away: no tap, no swipe.
    pub fn finger_cancelled(&mut self, id: u32) -> Option<Gesture> {
        self.finger_gone(id, None)
    }

    fn finger_gone(&mut self, id: u32, up: Option<([f32; 2], bool)>) -> Option<Gesture> {
        let t = &mut self.touch;
        let i = t.fingers.iter().position(|f| f.id == id)?;
        let mut f = t.fingers.remove(i);
        if let Some((at, _)) = up {
            f.at = at;
            t.moved |= far(f.down, at);
        }
        match t.fingers.len() {
            0 => {
                let (most, moved) = (t.most, t.moved);
                *t = Touch::default();
                let Some((at, inside)) = up else {
                    self.left = None;
                    return None;
                };
                match most {
                    1 => {
                        let pans = self.drag_pans;
                        self.left_up(at, inside, pans)
                    }
                    2 if !moved => Some(Gesture::Key(Does::View)),
                    3 if !moved => Some(Gesture::Key(Does::Home)),
                    _ => {
                        self.left = None;
                        None
                    }
                }
            }
            // From two to one: the twist settles, and the finger left pans
            // once it moves (from where it is, with no slack if the touch
            // moved already).
            1 => {
                let rest = t.fingers[0];
                let pan = t.moved.then_some(rest.at);
                let two = t.two.take();
                if t.most == 2 {
                    self.left = Some(Left { start: rest.at, inside: false, pan });
                }
                two.and_then(|two| settle(two, &self.tuning))
            }
            _ => None,
        }
    }

    /// Two fingers moved: once one of them went [`SWIPE_PX`] from where it
    /// came down, the ground under them follows them.
    fn two_moved(&mut self) -> Option<Gesture> {
        let tuning = self.tuning;
        let t = &mut self.touch;
        if !t.moved {
            return None;
        }
        let two = t.two.as_mut()?;
        let at = |id: u32| t.fingers.iter().find(|f| f.id == id).map(|f| f.at);
        let (a, b) = (at(two.ids[0])?, at(two.ids[1])?);
        let [a0, b0] = two.last;
        let (u, v) = (sub(b0, a0), sub(b, a));
        let step = angle_deg(cross(u, v), dot(u, v));
        two.twist += step;
        let (s0, s1) = (spread(a0, b0), spread(a, b));
        let held = |other: bool| tuning.exclusive && other;
        if !two.turning && !held(two.zooming) && two.twist.abs() >= tuning.twist_deg {
            two.turning = true;
        }
        let mut base = s0;
        if !two.zooming && !held(two.turning) && two.spread0 > 0.0 {
            let r = s1 / two.spread0;
            two.zooming = r >= tuning.zoom_after || r * tuning.zoom_after <= 1.0;
            if two.zooming && tuning.zoom_catches_up {
                base = two.spread0;
            }
        }
        let zoom = if two.zooming && base > 0.0 && s1 > 0.0 { (s1 / base) as f32 } else { 1.0 };
        let turn = if two.turning { step as f32 } else { 0.0 };
        two.turned += turn;
        two.last = [a, b];
        let (from, to) = (mid(a0, b0), mid(a, b));
        (from != to || zoom != 1.0 || turn != 0.0).then_some(Gesture::Grab { from, to, zoom, turn })
    }
}

/// The settle of two fingers that turned the view: their twist in whole
/// quarter turns, and at least one the way they twisted while the twist is
/// still past the point where turning started.
fn settle(two: Two, tuning: &Tuning) -> Option<Gesture> {
    if !two.turning {
        return None;
    }
    let mut quarters = (two.twist / 90.0).round() as i32;
    if quarters == 0 && two.twist.abs() >= tuning.twist_deg {
        quarters = if two.twist > 0.0 { 1 } else { -1 };
    }
    Some(Gesture::Settle { quarters, turned: two.turned })
}

fn far(a: [f32; 2], b: [f32; 2]) -> bool {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    dx * dx + dy * dy >= SWIPE_PX * SWIPE_PX
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f64; 2] {
    [f64::from(a[0]) - f64::from(b[0]), f64::from(a[1]) - f64::from(b[1])]
}

fn cross(u: [f64; 2], v: [f64; 2]) -> f64 {
    u[0] * v[1] - u[1] * v[0]
}

fn dot(u: [f64; 2], v: [f64; 2]) -> f64 {
    u[0] * v[0] + u[1] * v[1]
}

fn spread(a: [f32; 2], b: [f32; 2]) -> f64 {
    let d = sub(a, b);
    dot(d, d).sqrt()
}

fn mid(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}

/// The angle in degrees whose sine and cosine are in the ratio `y` : `x`,
/// from −180 to 180: atan2 with plain arithmetic only (no platform libm,
/// ADR 0005), within 0.001 degree. On the screen, y down, it is clockwise.
pub fn angle_deg(y: f64, x: f64) -> f64 {
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    let (ax, ay) = (x.abs(), y.abs());
    let (t, swap) = if ay > ax { (ax / ay, true) } else { (ay / ax, false) };
    // atan on 0..1: a minimax polynomial (error under 2e-5 radians).
    let t2 = t * t;
    let r = t * (0.999_866_0 + t2 * (-0.330_299_5 + t2 * (0.180_141_0 + t2 * (-0.085_133_0 + t2 * 0.020_835_1))));
    let mut a = if swap { std::f64::consts::FRAC_PI_2 - r } else { r };
    if x < 0.0 {
        a = std::f64::consts::PI - a;
    }
    if y < 0.0 {
        a = -a;
    }
    a.to_degrees()
}

#[cfg(test)]
mod tests {
    use rwgpuengine_core::math::sin_cos_deg64;

    use super::*;

    fn bits(s: ActionSet) -> u32 {
        s.bits()
    }

    fn grab(from: [f32; 2], to: [f32; 2]) -> Option<Gesture> {
        Some(Gesture::Grab { from, to, zoom: 1.0, turn: 0.0 })
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
    fn a_click_is_a_tap_and_a_drag_is_a_swipe_in_a_game_with_a_mover() {
        let mut i = Input::default();
        i.button(Button::Left, true, [416.7, 272.2], true);
        i.moved([420.0, 275.0]);
        i.button(Button::Left, false, [420.0, 275.0], true);
        i.button(Button::Left, true, [100.0, 100.0], true);
        i.button(Button::Left, false, [101.0, 101.0], true);
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
            i.button(Button::Left, true, [100.0, 100.0], true);
            assert_eq!(i.moved(to), None, "a swipe moves no camera");
            assert_eq!(i.button(Button::Left, false, to, true), None);
            let s = i.take_step();
            assert_eq!(bits(s.pressed), want.bit(), "swipe to {to:?}");
            assert!(s.taps.is_empty());
        }
        // Pressed on a bar: nothing.
        i.button(Button::Left, true, [-5.0, 10.0], false);
        i.button(Button::Left, false, [10.0, 10.0], true);
        let s = i.take_step();
        assert!(s.taps.is_empty() && s.pressed.is_empty());
    }

    /// In a game with no mover a left-drag pans: the ground under the
    /// pointer follows it from where the button went down, and no tap or
    /// swipe is left behind. A click is still a tap.
    #[test]
    fn a_drag_pans_in_a_game_with_no_mover() {
        let mut i = Input::default();
        i.set_drag_pans(true);
        i.button(Button::Left, true, [100.0, 100.0], true);
        assert_eq!(i.moved([105.0, 104.0]), None, "under 12 frame pixels it may still be a tap");
        assert_eq!(i.moved([112.0, 104.0]), grab([100.0, 100.0], [112.0, 104.0]));
        assert_eq!(i.moved([150.0, 90.0]), grab([112.0, 104.0], [150.0, 90.0]));
        assert_eq!(i.button(Button::Left, false, [150.0, 90.0], true), None);
        let s = i.take_step();
        assert!(s.taps.is_empty() && s.pressed.is_empty());
        // Released far away with no move between: the pan still happens.
        i.button(Button::Left, true, [10.0, 10.0], true);
        assert_eq!(i.button(Button::Left, false, [70.0, 10.0], true), grab([10.0, 10.0], [70.0, 10.0]));
        i.button(Button::Left, true, [416.7, 272.2], true);
        i.button(Button::Left, false, [420.0, 275.0], true);
        assert_eq!(i.take_step().taps, vec![(416.0, 272.0)]);
    }

    #[test]
    fn camera_keys_and_mouse_gestures_are_camera_moves() {
        let mut i = Input::default();
        assert_eq!(i.key("KeyE", true, false), Some(Gesture::Move(CameraMove { turn: 90.0, ..STILL })));
        assert_eq!(i.key("KeyQ", true, true), None);
        assert_eq!(i.key("Equal", true, true), Some(Gesture::Move(CameraMove { zoom: ZOOM_KEY, ..STILL })));
        assert_eq!(i.key("KeyV", true, false), Some(Gesture::Key(Does::View)));
        assert_eq!(i.key("Home", true, false), Some(Gesture::Key(Does::Home)));
        // Camera keys are not game actions.
        assert!(i.take_step().pressed.is_empty());
        // Right-drag turns, half a degree per frame pixel.
        i.moved([50.0, 50.0]);
        i.button(Button::Right, true, [50.0, 50.0], true);
        assert_eq!(i.moved([60.0, 65.0]), Some(Gesture::Move(CameraMove { turn: -5.0, ..STILL })));
        i.button(Button::Right, false, [60.0, 65.0], true);
        assert_eq!(i.moved([70.0, 65.0]), None);
        // Middle-drag and Shift + right-drag pan.
        i.button(Button::Middle, true, [70.0, 65.0], true);
        assert_eq!(i.moved([75.0, 55.0]), Some(Gesture::Pan([5.0, -10.0])));
        i.button(Button::Middle, false, [75.0, 55.0], true);
        i.shift(true);
        i.button(Button::Right, true, [75.0, 55.0], true);
        assert_eq!(i.moved([80.0, 55.0]), Some(Gesture::Pan([5.0, 0.0])));
        i.button(Button::Right, false, [80.0, 55.0], true);
        // The wheel: a notch is a step of 1.1; smooth scrolling adds up to notches.
        assert_eq!(i.wheel(1.0, 0.0), Some(Gesture::Move(CameraMove { zoom: ZOOM_WHEEL, ..STILL })));
        assert_eq!(i.wheel(0.0, -25.0), None);
        assert_eq!(i.wheel(0.0, -25.0), Some(Gesture::Move(CameraMove { zoom: 1.0 / ZOOM_WHEEL, ..STILL })));
        assert_eq!(i.pinch(0.25), Some(Gesture::Move(CameraMove { zoom: 1.25, ..STILL })));
        assert_eq!(i.pinch(f64::NAN), None);
    }

    /// One finger is the left button: a tap where it came down, a swipe in a
    /// game with a mover, a pan in a game without.
    #[test]
    fn one_finger_taps_and_swipes_or_pans() {
        let mut i = Input::default();
        i.finger_down(7, [416.7, 272.2], true);
        i.finger_moved(7, [417.5, 272.7]);
        i.finger_up(7, [417.5, 272.7], true);
        assert_eq!(i.take_step().taps, vec![(416.0, 272.0)]);
        i.finger_down(7, [100.0, 100.0], true);
        assert_eq!(i.finger_moved(7, [130.0, 80.0]), None);
        i.finger_up(7, [130.0, 80.0], true);
        assert_eq!(bits(i.take_step().pressed), Action::Up.bit());
        // With no mover, the same drag pans and presses nothing.
        i.set_drag_pans(true);
        i.finger_down(7, [100.0, 100.0], true);
        assert_eq!(i.finger_moved(7, [130.0, 80.0]), grab([100.0, 100.0], [130.0, 80.0]));
        assert_eq!(i.finger_up(7, [130.0, 80.0], true), None);
        assert!(i.take_step().pressed.is_empty());
        // A cancelled touch is no tap.
        i.finger_down(4, [10.0, 10.0], true);
        assert_eq!(i.finger_cancelled(4), None);
        assert!(i.take_step().taps.is_empty());
    }

    /// Two fingers: nothing moves until one of them has gone 12 frame
    /// pixels; then the ground under their midpoint follows it, and their
    /// spread zooms. Each finger's move is one grab.
    #[test]
    fn two_fingers_pan_and_zoom_about_their_midpoint() {
        let mut i = Input::default();
        assert_eq!(i.finger_down(1, [100.0, 100.0], true), None);
        assert_eq!(i.finger_down(2, [200.0, 100.0], true), None);
        assert_eq!(i.finger_moved(2, [205.0, 100.0]), None, "still: may be a two-finger tap");
        // Moved together 20 pixels down, one finger after the other: the
        // first grab catches up from where they came down (the 5 pixels of
        // finger 2 too), the midpoint ends 20 lower, and the zooms undo each other.
        let g1 = i.finger_moved(1, [100.0, 120.0]);
        let g2 = i.finger_moved(2, [200.0, 120.0]);
        let (
            Some(Gesture::Grab { from: f1, to: t1, zoom: z1, turn: 0.0 }),
            Some(Gesture::Grab { from: f2, to: t2, zoom: z2, turn: 0.0 }),
        ) = (g1, g2)
        else {
            panic!("{g1:?} {g2:?}")
        };
        assert_eq!((f1, t1, f2, t2), ([150.0, 100.0], [152.5, 110.0], [152.5, 110.0], [150.0, 120.0]));
        assert!((z1 * z2 - 1.0).abs() < 1e-6 && z1 > 1.0, "{z1} {z2}");
        // Apart: the spread zooms, here 1.5 times, about the midpoint.
        let g = i.finger_moved(2, [250.0, 120.0]);
        assert_eq!(g, Some(Gesture::Grab { from: [150.0, 120.0], to: [175.0, 120.0], zoom: 1.5, turn: 0.0 }));
        // No tap, no swipe, no key at the end.
        assert_eq!(i.finger_up(1, [100.0, 120.0], true), None);
        assert_eq!(i.finger_up(2, [250.0, 120.0], true), None);
        let s = i.take_step();
        assert!(s.taps.is_empty() && s.pressed.is_empty());
    }

    /// A twist turns nothing until the fingers have twisted 20 degrees; then
    /// the view turns with them, and letting go settles it on a quarter turn.
    #[test]
    fn a_twist_turns_and_settles_on_a_quarter_turn() {
        let mut i = Input::default();
        // One finger goes round the other, 100 pixels away: the line between
        // them turns by as much as the finger goes round (clockwise is positive).
        let b = [400.0f32, 200.0];
        let round = |deg: f64| {
            let (sn, cs) = sin_cos_deg64(deg);
            [(400.0 - 100.0 * cs) as f32, (200.0 - 100.0 * sn) as f32]
        };
        let twist = |i: &mut Input, id: u32, degs: &[f64]| {
            let mut turned = Vec::new();
            for &deg in degs {
                match i.finger_moved(id, round(deg)) {
                    Some(Gesture::Grab { turn, zoom, .. }) => {
                        assert!((zoom - 1.0).abs() < 1e-5, "{deg}: {zoom}");
                        turned.push(turn);
                    }
                    None => turned.push(0.0),
                    g => panic!("{deg}: {g:?}"),
                }
            }
            turned
        };
        i.finger_down(1, round(0.0), true);
        i.finger_down(2, b, true);
        let turned = twist(&mut i, 1, &[5.0, 10.0, 15.0, 19.0, 25.0, 35.0]);
        // Nothing until 20 degrees; then the step that passed it (6) and the next (10).
        let near = |got: f32, want: f32| (got - want).abs() < 0.01;
        assert!(turned[..4].iter().all(|&t| t == 0.0) && near(turned[4], 6.0) && near(turned[5], 10.0), "{turned:?}");
        let sum: f32 = turned.iter().sum();
        assert_eq!(i.finger_up(1, round(35.0), true), Some(Gesture::Settle { quarters: 1, turned: sum }));
        assert_eq!(i.finger_up(2, b, true), None);
        // Anticlockwise is negative; a cancelled finger settles too.
        i.finger_down(1, round(0.0), true);
        i.finger_down(2, b, true);
        twist(&mut i, 1, &[-15.0, -30.0]);
        assert!(matches!(i.finger_cancelled(1), Some(Gesture::Settle { quarters: -1, .. })));
        i.finger_cancelled(2);
        // Twisted past 20 degrees and back: it settles where it was.
        i.finger_down(1, round(0.0), true);
        i.finger_down(2, b, true);
        twist(&mut i, 1, &[15.0, 25.0, 12.0]);
        assert!(matches!(i.finger_up(1, round(12.0), true), Some(Gesture::Settle { quarters: 0, .. })));
        i.finger_up(2, b, true);
        // A twist of 140 degrees is two quarter turns.
        i.finger_down(1, round(0.0), true);
        i.finger_down(2, b, true);
        twist(&mut i, 1, &(1..=14).map(|k| f64::from(k) * 10.0).collect::<Vec<_>>());
        assert!(matches!(i.finger_up(2, b, true), Some(Gesture::Settle { quarters: 2, .. })));
        // A pinch with no twist settles nothing.
        i.finger_up(1, round(140.0), true);
        i.finger_down(1, [100.0, 100.0], true);
        i.finger_down(2, [200.0, 100.0], true);
        i.finger_moved(2, [300.0, 100.0]);
        assert_eq!(i.finger_up(2, [300.0, 100.0], true), None);
    }

    /// Fingers that come down and go up without moving: two switch the view,
    /// three go back to the scene's camera, in whatever order they lift.
    #[test]
    fn still_fingers_switch_the_view_or_go_home() {
        let mut i = Input::default();
        i.finger_down(1, [100.0, 100.0], true);
        i.finger_down(2, [160.0, 110.0], true);
        i.finger_moved(1, [103.0, 104.0]);
        assert_eq!(i.finger_up(1, [103.0, 104.0], true), None);
        // The finger left does not pan or tap while it stays still.
        assert_eq!(i.finger_moved(2, [165.0, 110.0]), None);
        assert_eq!(i.finger_up(2, [165.0, 110.0], true), Some(Gesture::Key(Does::View)));
        for id in 1..=3 {
            i.finger_down(id, [100.0 * id as f32, 50.0], true);
        }
        i.finger_up(2, [200.0, 50.0], true);
        i.finger_up(3, [300.0, 50.0], true);
        assert_eq!(i.finger_up(1, [100.0, 50.0], true), Some(Gesture::Key(Does::Home)));
        // Moved: no tap of any kind.
        i.finger_down(1, [100.0, 100.0], true);
        i.finger_down(2, [200.0, 100.0], true);
        i.finger_moved(2, [230.0, 100.0]);
        i.finger_up(2, [230.0, 100.0], true);
        assert_eq!(i.finger_up(1, [100.0, 100.0], true), None);
        assert!(i.take_step().taps.is_empty());
    }

    /// After two fingers moved the camera, the one left pans on, in any game;
    /// after a twist the view settles first.
    #[test]
    fn the_finger_left_after_two_pans() {
        let mut i = Input::default();
        i.finger_down(1, [100.0, 100.0], true);
        i.finger_down(2, [200.0, 100.0], true);
        i.finger_moved(2, [200.0, 130.0]);
        i.finger_up(2, [200.0, 130.0], true);
        assert_eq!(i.finger_moved(1, [110.0, 100.0]), grab([100.0, 100.0], [110.0, 100.0]));
        assert_eq!(i.finger_up(1, [110.0, 100.0], true), None);
        assert!(i.take_step().pressed.is_empty());
    }

    #[test]
    fn the_angle_is_atan2_in_degrees() {
        for k in -179..=180 {
            let deg = f64::from(k);
            let (s, c) = sin_cos_deg64(deg);
            for r in [0.01, 1.0, 300.0] {
                let got = angle_deg(s * r, c * r);
                assert!((got - deg).abs() < 1e-3 || (got - deg).abs() > 359.999, "{deg}: {got}");
            }
        }
        assert_eq!(angle_deg(0.0, 0.0), 0.0);
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
        for gesture in GESTURE_WORDS {
            assert!(page.contains(gesture), "the web page's help does not say `{gesture}`");
        }
    }

    /// The words for the gestures that the page's help and `rwgpu help play`
    /// both say.
    const GESTURE_WORDS: [&str; 12] = [
        "right-drag",
        "middle-drag",
        "Shift + right-drag",
        "wheel",
        "pinch",
        "click",
        "swipe",
        "no mover",
        "two fingers",
        "twist",
        "quarter turn",
        "three fingers",
    ];

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
        for gesture in GESTURE_WORDS {
            assert!(help.contains(gesture), "`rwgpu help play` does not say `{gesture}`");
        }
    }
}
