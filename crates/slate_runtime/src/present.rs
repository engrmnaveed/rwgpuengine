//! Presenting a session's picture in a player.

use slate_render::{DrawList, paint};

use crate::session::Session;

/// A player's pixel buffer, repainted only when the picture changes.
///
/// A step that moves nothing gives the same draw list, and equal lists paint
/// equal pixels. In the example games fewer than one frame in ten changes
/// anything, so a player that asks [`Presenter::present`] every frame mostly
/// gets "nothing to show" for the price of building and comparing the list
/// (ADR 0018).
#[derive(Debug, Default)]
pub struct Presenter {
    pixels: Vec<u8>,
    shown: Option<DrawList>,
}

impl Presenter {
    pub fn new() -> Presenter {
        Presenter::default()
    }

    /// Bring the buffer up to date with `session`. Returns false when the
    /// picture is the one already in the buffer, and true when it was
    /// repainted (always on the first call and after [`Presenter::reset`]).
    pub fn present(&mut self, session: &Session) -> bool {
        let list = session.draw_list(false);
        if self.shown.as_ref() == Some(&list) {
            return false;
        }
        paint(&list, &mut self.pixels);
        self.shown = Some(list);
        true
    }

    /// RGBA8, row-major, top row first: the picture last presented. Empty
    /// before the first `present`.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Forget the picture, so the next `present` repaints: for a new session,
    /// or when whatever showed the pixels lost them.
    pub fn reset(&mut self) {
        self.shown = None;
    }
}
