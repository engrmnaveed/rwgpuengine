//! Input: pressed actions and taps become events and `tapped` signals.

use rwgpuengine_core::json::Json;

use super::signals::Pending;
use super::{World, fmt_cell};
use crate::events::EventKind;
use crate::input::InputFrame;

impl World {
    pub(super) fn apply_input(&mut self, input: &InputFrame) {
        if !input.pressed.is_empty() {
            let names = input.pressed.names();
            self.log(
                EventKind::Input,
                None,
                None,
                format!("pressed {}", names.join(", ")),
                Json::object().with("pressed", names.iter().map(|n| Json::from(*n)).collect::<Vec<_>>()),
            );
        }
        for tap in &input.taps {
            let at = format!("({}, {})", tap.x, tap.y);
            let data = Json::object().with("x", tap.x).with("y", tap.y);
            match tap.hit {
                Some(id) => {
                    let msg = format!("tap at {at} hit {}; it emits 'tapped'", self.describe(id));
                    self.log(EventKind::Tap, Some(id), None, msg, data.with("signal", "tapped"));
                    self.emit(id, "tapped");
                }
                None if tap.ground.is_some() && self.is_listened(None, "tapped") => {
                    let cell = tap.ground.expect("ground cell");
                    let mut msg = format!("tap at {at} hit ground cell {}", fmt_cell(cell));
                    if let Some(t) = self.terrain_name(cell) {
                        msg.push_str(&format!(" ({t})"));
                    }
                    msg.push_str("; the ground emits 'tapped'");
                    self.log(EventKind::Tap, None, None, msg, data.with("cell", cell).with("signal", "tapped"));
                    self.pending.push_back(Pending { emitter: None, signal: "tapped".into(), cell, reason: None });
                }
                None => {
                    let mut msg = format!("tap at {at} hit nothing enabled");
                    if let Some(cell) = tap.ground {
                        msg.push_str(&format!(
                            " (ground cell {}; nothing listens to the ground's 'tapped')",
                            fmt_cell(cell)
                        ));
                    }
                    if let Some(&d) = tap.under_disabled.first() {
                        msg.push_str(&format!("; disabled {} is under the pointer", self.describe(d)));
                    }
                    let under: Vec<Json> = tap.under_disabled.iter().map(|id| Json::from(*id)).collect();
                    let mut data = data.with("under_disabled", under);
                    if let Some(cell) = tap.ground {
                        data.set("cell", cell);
                    }
                    self.log(EventKind::TapMiss, tap.under_disabled.first().copied(), None, msg, data);
                }
            }
        }
    }
}
