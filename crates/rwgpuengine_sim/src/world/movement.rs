//! Movement on the grid: what blocks a cell, one step with pushing, and the
//! three behaviors that take steps (movers, patrols, wanderers).

use rwgpuengine_core::json::Json;
use rwgpuengine_data::{Action, PatrolMode};

use super::entity::Patrol;
use super::index::Kind;
use super::{Entity, EntityId, World, fmt_cell, scan};
use crate::events::EventKind;
use crate::input::InputFrame;

impl World {
    /// The entity that stops something entering `cell`, if any. Pushables
    /// count as solid here (only movers can push them, see `try_move`).
    pub fn blocker_at(&self, cell: [i32; 2], except: EntityId) -> Option<(EntityId, &'static str)> {
        self.blockers(cell, except).0
    }

    /// The first entity, in id order, that stops something entering `cell`,
    /// and whether a second one does too.
    fn blockers(&self, cell: [i32; 2], except: EntityId) -> (Option<(EntityId, &'static str)>, bool) {
        let found = self.index.blockers(cell, except);
        if self.checking() {
            let scanned = scan::blockers_at(&self.entities, cell, except);
            assert_eq!(
                found,
                (scanned.first().copied(), scanned.len() > 1),
                "the index and the scan disagree on what blocks {cell:?} for #{except}: the scan found {scanned:?}"
            );
        }
        found
    }

    /// Stand an entity on a cell; its height stays.
    fn put(&mut self, id: EntityId, cell: [i32; 2]) {
        let e = &mut self.entities.0[id as usize];
        e.pos[0] = cell[0] as f32;
        e.pos[1] = cell[1] as f32;
        let now = e.cell();
        self.index.moved(id, now);
    }

    fn outside(&self, cell: [i32; 2]) -> Option<[i32; 2]> {
        let [w, h] = self.bounds?;
        (cell[0] < 0 || cell[1] < 0 || cell[0] >= w || cell[1] >= h).then_some([w, h])
    }

    /// Move one cell. An entity with a mover pushes a pushable standing in
    /// the way, if the cell beyond it is free. Every refusal is logged with
    /// its reason.
    pub fn try_move(&mut self, id: EntityId, dx: i32, dy: i32, cause: &str) -> bool {
        let Some(e) = self.get(id) else { return false };
        let from = e.cell();
        let to = [from[0] + dx, from[1] + dy];
        let who = e.describe();
        let can_push = e.mover.is_some();
        let base = Json::object().with("cause", cause).with("from", from).with("to", to);
        if let Some([w, h]) = self.outside(to) {
            let msg = format!(
                "{who} could not move {cause} from {} to {}: outside the {w}x{h} ground",
                fmt_cell(from),
                fmt_cell(to)
            );
            self.log(EventKind::Blocked, Some(id), None, msg, base.with("reason", "edge"));
            return false;
        }
        let (blocker, more) = self.blockers(to, id);
        if let Some((crate_id, _)) = blocker
            && !more
            && can_push
            && self.entities[crate_id as usize].pushable
        {
            let beyond = [to[0] + dx, to[1] + dy];
            let what = self.describe(crate_id);
            let stuck = match self.outside(beyond) {
                Some([w, h]) => Some((None, format!("outside the {w}x{h} ground"), "edge")),
                None => self.blocker_at(beyond, crate_id).map(|(b, reason)| {
                    let kind = if self.entities[b as usize].pushable { "another pushable" } else { reason };
                    (Some(b), format!("blocked by {kind} {}", self.describe(b)), reason)
                }),
            };
            if let Some((by, why, reason)) = stuck {
                let msg = format!(
                    "{who} could not move {cause} from {} to {}: {what} cannot be pushed to {}, {why}",
                    fmt_cell(from),
                    fmt_cell(to),
                    fmt_cell(beyond)
                );
                let data = base.with("reason", "push").with("beyond", beyond).with("push_blocked", reason);
                let data = match by {
                    Some(b) => data.with("push_blocked_by", self.get(b).map(Entity::label)),
                    None => data,
                };
                self.log(EventKind::Blocked, Some(id), Some(crate_id), msg, data);
                return false;
            }
            self.put(crate_id, beyond);
            let msg = format!("{what} was pushed {cause} from {} to {} by {who}", fmt_cell(to), fmt_cell(beyond));
            let data = Json::object().with("cause", "pushed").with("from", to).with("to", beyond).with("by", id);
            self.log(EventKind::Moved, Some(crate_id), Some(id), msg, data);
        } else if let Some((blocker, reason)) = blocker {
            let pushable = self.entities[blocker as usize].pushable;
            let what = if pushable { "pushable" } else { reason };
            let note = if pushable && !can_push { " (only entities with a mover push)" } else { "" };
            let msg = format!(
                "{who} could not move {cause} from {} to {}: blocked by {what} {}{note}",
                fmt_cell(from),
                fmt_cell(to),
                self.describe(blocker)
            );
            self.log(EventKind::Blocked, Some(id), Some(blocker), msg, base.with("reason", reason));
            return false;
        }
        self.put(id, to);
        let msg = format!("{who} moved {cause} from {} to {}", fmt_cell(from), fmt_cell(to));
        self.log(EventKind::Moved, Some(id), None, msg, base);
        true
    }

    pub(super) fn run_movers(&mut self, input: &InputFrame) {
        for id in self.enabled_with(Kind::Mover) {
            let mover = self.entities.0[id as usize].mover.as_mut().expect("mover");
            mover.cooldown = mover.cooldown.saturating_sub(1);
            let (cooldown, step) = (mover.cooldown, mover.step_frames);
            let Some(dir) = Action::DIRECTIONS.into_iter().find(|a| input.down.contains(*a)) else {
                continue;
            };
            if cooldown == 0 {
                let (dx, dy) = dir.delta().expect("direction");
                self.try_move(id, dx, dy, dir.name());
                if let Some(m) = self.entities.0[id as usize].mover.as_mut() {
                    m.cooldown = step;
                }
            } else if input.pressed.contains(dir) {
                let who = self.describe(id);
                let msg = format!(
                    "{who} ignored '{}': still waiting {cooldown} frame(s) after its last move (mover.step_frames = {step})",
                    dir.name()
                );
                let data =
                    Json::object().with("cause", dir.name()).with("reason", "cooldown").with("frames_left", cooldown);
                self.log(EventKind::Blocked, Some(id), None, msg, data);
            }
        }
    }

    pub(super) fn run_patrols(&mut self) {
        for id in self.enabled_with(Kind::Patrol) {
            let cell = self.entities[id as usize].cell();
            let patrol = self.entities.0[id as usize].patrol.as_mut().expect("patrol");
            patrol.timer += 1;
            if patrol.timer < patrol.def.step_frames {
                continue;
            }
            patrol.timer = 0;
            if patrol.def.path.len() < 2 && patrol.def.path.first() == Some(&cell) {
                continue;
            }
            if patrol.def.path.get(patrol.target) == Some(&cell) {
                advance(patrol);
            }
            let target = patrol.def.path[patrol.target];
            let (dx, dy) = step_toward(cell, target);
            if (dx, dy) != (0, 0) && self.try_move(id, dx, dy, "patrol") {
                let new_cell = self.entities[id as usize].cell();
                let patrol = self.entities.0[id as usize].patrol.as_mut().expect("patrol");
                if new_cell == target {
                    advance(patrol);
                }
            }
        }
    }

    pub(super) fn run_wanders(&mut self) {
        for id in self.enabled_with(Kind::Wander) {
            let wander = self.entities.0[id as usize].wander.as_mut().expect("wander");
            wander.timer += 1;
            if wander.timer < wander.def.step_frames {
                continue;
            }
            wander.timer = 0;
            let area = wander.def.area;
            // 0 = stay, 1..=4 = up, down, left, right. One draw per step,
            // in entity order: part of the determinism contract.
            let roll = self.rng.below(5);
            if roll == 0 {
                continue;
            }
            let dir = Action::DIRECTIONS[(roll - 1) as usize];
            let (dx, dy) = dir.delta().expect("direction");
            let cell = self.entities[id as usize].cell();
            let to = [cell[0] + dx, cell[1] + dy];
            if let Some([lo, hi]) = area
                && (to[0] < lo[0] || to[1] < lo[1] || to[0] > hi[0] || to[1] > hi[1])
            {
                let msg = format!(
                    "{} stayed: {} is outside its wander area {}..{}",
                    self.describe(id),
                    fmt_cell(to),
                    fmt_cell(lo),
                    fmt_cell(hi)
                );
                let data =
                    Json::object().with("cause", "wander").with("reason", "area").with("from", cell).with("to", to);
                self.log(EventKind::Blocked, Some(id), None, msg, data);
                continue;
            }
            self.try_move(id, dx, dy, "wander");
        }
    }
}

fn advance(p: &mut Patrol) {
    let n = p.def.path.len();
    if n < 2 {
        return;
    }
    match p.def.mode {
        PatrolMode::Loop => p.target = (p.target + 1) % n,
        PatrolMode::PingPong => {
            let next = p.target as i64 + p.dir as i64;
            if next < 0 || next >= n as i64 {
                p.dir = -p.dir;
            }
            p.target = (p.target as i64 + p.dir as i64) as usize;
        }
    }
}

/// One axis at a time, x first.
fn step_toward(from: [i32; 2], to: [i32; 2]) -> (i32, i32) {
    if from[0] != to[0] { ((to[0] - from[0]).signum(), 0) } else { (0, (to[1] - from[1]).signum()) }
}
