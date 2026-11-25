//! A session: one project + one scene + an optional input script, stepped
//! frame by frame. Every command is a pure function of (files, script, seed,
//! frame): `shot --frame 300` simply replays 300 steps.

use slate_core::codes;
use slate_core::diag::Diagnostic;
use slate_core::hash;
use slate_core::json::Json;
use slate_core::text::did_you_mean;
use slate_data::{
    InputDef, InputKind, PlaytestDef, ProjectDef, SceneDef, SceneLoader, Vfs, load_playtest, load_project,
};
use slate_render::{DrawList, Frame, FrameReport, View, build_report, draw, paint, pick, rasterize};
use slate_sim::{ActionSet, InputFrame, Systems, World, dump};

/// Rust game code plugged into a session (ADR 0006: data first, code
/// where data runs out). Systems run every step after the built-ins.
pub trait Game: Systems {
    fn name(&self) -> &str {
        "data-only"
    }
    /// Called once after the world is built, before frame 0 is stepped.
    fn on_start(&mut self, _world: &mut World) {}
    /// Extra state for `slate inspect` (shown under "game").
    fn inspect(&self, _world: &World) -> Json {
        Json::Null
    }
}

/// The default: no Rust code, everything comes from the files.
#[derive(Debug, Default)]
pub struct DataOnly;
impl Systems for DataOnly {}
impl Game for DataOnly {}

/// What to load.
#[derive(Clone, Debug, Default)]
pub struct SessionConfig {
    /// Scene path; defaults to the playtest's scene, then main_scene.
    pub scene: Option<String>,
    /// Playtest whose `[[input]]` drives the session.
    pub script: Option<String>,
    /// Seed override (beats playtest, scene and project seeds).
    pub seed: Option<u64>,
}

/// Input from a live player (web, future windowed player), merged with the script.
#[derive(Clone, Debug, Default)]
pub struct LiveInput {
    pub pressed: ActionSet,
    pub down: ActionSet,
    pub taps: Vec<(f32, f32)>,
}

/// Scripted input, indexed by frame.
#[derive(Clone, Debug, Default)]
pub struct Script {
    inputs: Vec<InputDef>,
}

impl Script {
    pub fn new(inputs: Vec<InputDef>) -> Script {
        Script { inputs }
    }

    /// (pressed, down, taps) for the step taken at `frame`.
    pub fn at(&self, frame: u64) -> (ActionSet, ActionSet, Vec<(f32, f32)>) {
        let (mut pressed, mut down, mut taps) = (ActionSet::EMPTY, ActionSet::EMPTY, Vec::new());
        for i in &self.inputs {
            match i.kind {
                InputKind::Press(a) if i.frame == frame => {
                    pressed.insert(a);
                    down.insert(a);
                }
                InputKind::Hold(a, n) if frame >= i.frame && frame < i.frame + n => {
                    if frame == i.frame {
                        pressed.insert(a);
                    }
                    down.insert(a);
                }
                InputKind::Tap(x, y) if i.frame == frame => taps.push((x, y)),
                _ => {}
            }
        }
        (pressed, down, taps)
    }
}

pub struct Session {
    pub project: ProjectDef,
    pub scene: SceneDef,
    pub playtest: Option<PlaytestDef>,
    pub world: World,
    pub view: View,
    /// Warnings found while loading (the session still runs).
    pub warnings: Vec<Diagnostic>,
    script: Script,
    game: Box<dyn Game>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("scene", &self.scene.path).field("frame", &self.world.frame).finish()
    }
}

impl Session {
    /// Load project, optional playtest and scene. On failure, returns every
    /// diagnostic (errors and warnings).
    pub fn load(vfs: &Vfs, cfg: &SessionConfig, mut game: Box<dyn Game>) -> Result<Session, Vec<Diagnostic>> {
        let (project, mut diags) = load_project(vfs);
        let Some(project) = project else {
            return Err(diags);
        };

        let playtest = match &cfg.script {
            None => None,
            Some(path) if !vfs.contains(path) => {
                diags.push(
                    Diagnostic::new(&codes::FILE_NOT_FOUND, format!("input script `{path}` not found"))
                        .maybe_hint(did_you_mean(path, vfs.paths_under("tests", ".toml"))),
                );
                return Err(diags);
            }
            Some(path) => {
                let (def, d) = load_playtest(vfs, path);
                diags.extend(d);
                match def {
                    Some(def) => Some(def),
                    None => return Err(diags),
                }
            }
        };

        let scene_path = cfg
            .scene
            .clone()
            .or_else(|| playtest.as_ref().and_then(|p| p.scene.clone()))
            .unwrap_or_else(|| project.main_scene.clone());
        if !vfs.contains(&scene_path) {
            let mut d = Diagnostic::new(&codes::UNKNOWN_SCENE, format!("scene `{scene_path}` not found"))
                .maybe_hint(did_you_mean(&scene_path, vfs.paths_under("scenes", ".toml")));
            if let Some(p) = playtest.as_ref().filter(|p| p.scene.as_deref() == Some(scene_path.as_str())) {
                d = d.at(&p.src, p.scene_span);
            }
            diags.push(d);
            return Err(diags);
        }
        let mut loader = SceneLoader::new(vfs);
        let scene = loader.load_scene(&scene_path);
        diags.extend(loader.diags);
        let Some(scene) = scene else {
            return Err(diags);
        };

        let seed =
            cfg.seed.or_else(|| playtest.as_ref().and_then(|p| p.seed)).or(scene.seed).unwrap_or(project.sim.seed);
        let mut world = World::new(&scene, seed);
        game.on_start(&mut world);
        let view = View::new(&project, &scene);
        let script = Script::new(playtest.as_ref().map(|p| p.inputs.clone()).unwrap_or_default());
        Ok(Session { project, scene, playtest, world, view, warnings: diags, script, game })
    }

    pub fn frame(&self) -> u64 {
        self.world.frame
    }

    /// Frames the script wants to run (playtest `frames`), if any.
    pub fn script_frames(&self) -> Option<u64> {
        self.playtest.as_ref().map(|p| p.frames)
    }

    /// Take one fixed step with scripted plus live input.
    pub fn step(&mut self, live: &LiveInput) {
        let (pressed, down, mut taps) = self.script.at(self.world.frame);
        taps.extend(live.taps.iter().copied());
        let resolved = if taps.is_empty() {
            Vec::new()
        } else {
            let list = self.draw_list(true);
            taps.iter().map(|&(x, y)| pick::pick(&list, x, y)).collect()
        };
        let input = InputFrame {
            pressed: pressed.union(live.pressed),
            down: down.union(live.down).union(live.pressed),
            taps: resolved,
        };
        self.world.tick(&input, self.game.as_mut());
    }

    /// Step until `frame` (no-op if already there or past it).
    pub fn run_to(&mut self, frame: u64) {
        let idle = LiveInput::default();
        while self.world.frame < frame {
            self.step(&idle);
        }
    }

    pub fn draw_list(&self, include_disabled: bool) -> DrawList {
        draw::build(&self.world, &self.view, self.scene.ground.as_ref(), include_disabled)
    }

    /// Render the current frame and describe it: the reference pixels plus
    /// the frame report (`slate shot`, playtest `[[shot]]`s).
    pub fn render(&self) -> (Frame, FrameReport) {
        let list = self.draw_list(false);
        let (frame, stats) = rasterize(&list);
        let report = build_report(&self.world, &list, &frame, &stats);
        (frame, report)
    }

    /// Paint the current frame's colors into `rgba`: the same pixels as
    /// [`Session::render`], without the report, for showing the game. A
    /// player that paints every frame should use a
    /// [`Presenter`](crate::Presenter), which skips frames that did not change.
    pub fn paint(&self, rgba: &mut Vec<u8>) {
        paint(&self.draw_list(false), rgba);
    }

    pub fn state_hash(&self) -> u64 {
        self.world.state_hash()
    }

    /// The world as JSON: summary, entities (with screen rectangles) and,
    /// optionally, events.
    pub fn inspect(&self, opts: &InspectOptions) -> Json {
        let list = self.draw_list(false);
        let pick_list = self.draw_list(true);
        let ids: Option<Vec<u32>> =
            opts.entity.as_ref().map(|n| self.world.find(n).map(|id| vec![id]).unwrap_or_default());
        let mut entities = Vec::new();
        for e in &self.world.entities {
            if ids.as_ref().is_some_and(|ids| !ids.contains(&e.id)) {
                continue;
            }
            let mut j = dump::entity_json(&self.world, e);
            if opts.component.as_ref().is_some_and(|c| j.get(c).is_none()) {
                continue;
            }
            j.set("screen", screen_json(&list, e.id));
            // A pixel where a tap is guaranteed to hit this entity.
            j.set("tap", pick::tap_point(&pick_list, e.id).map(Json::from));
            entities.push(j);
        }
        let mut out = dump::world_summary(&self.world)
            .with("scene", self.scene.path.as_str())
            .with("input", self.playtest.as_ref().map(|p| p.path.clone()))
            .with("ground", dump::ground_json(&self.world))
            .with("entities", entities);
        if let Some(q) = &opts.place {
            out.set("place", self.place_json(q));
        }
        let game = self.game.inspect(&self.world);
        if !game.is_null() {
            out.set("game", game);
        }
        if opts.events {
            let filter_id = opts.entity.as_ref().and_then(|n| self.world.find(n));
            let events: Vec<Json> = self
                .world
                .events
                .all()
                .iter()
                .filter(|e| e.frame >= opts.since)
                .filter(|e| filter_id.is_none_or(|id| e.entity == Some(id) || e.other == Some(id)))
                .map(|e| dump::event_json(&self.world, e))
                .collect();
            out.set("events", events);
            if self.world.events.dropped() > 0 {
                out.set("events_dropped", self.world.events.dropped());
            }
        }
        out
    }

    /// Short summary used by `run`.
    pub fn summary(&self) -> Json {
        dump::world_summary(&self.world)
            .with("scene", self.scene.path.as_str())
            .with("input", self.playtest.as_ref().map(|p| p.path.clone()))
            .with("game", self.game.name())
    }

    pub fn hash_hex(&self) -> String {
        hash::hex(self.state_hash())
    }

    /// "Can I build here?": where `prefab` can be spawned right now, or
    /// whether it fits at one cell and why not.
    pub fn place_json(&self, q: &PlaceQuery) -> Json {
        let w = &self.world;
        let list = self.draw_list(true);
        let tap = |cell: [i32; 2]| pick::ground_tap_point(&list, cell).map(Json::from);
        if w.template(&q.prefab).is_none() {
            let mut known: Vec<&str> = w.templates.iter().filter_map(|t| t.prefab.as_deref()).collect();
            known.sort_unstable();
            return Json::object().with("prefab", q.prefab.as_str()).with("ok", false).with(
                "message",
                format!(
                    "no [[connect]] in this scene spawns `{}` (spawned prefabs: {})",
                    q.prefab,
                    if known.is_empty() { "none".to_string() } else { known.join(", ") }
                ),
            );
        }
        match q.at {
            Some(cell) => match w.can_place(&q.prefab, cell) {
                Ok(cells) => Json::object()
                    .with("prefab", q.prefab.as_str())
                    .with("at", cell)
                    .with("ok", true)
                    .with("cells", Json::Arr(cells.into_iter().map(Json::from).collect()))
                    .with("tap", tap(cell))
                    .with("message", format!("`{}` fits at ({}, {})", q.prefab, cell[0], cell[1])),
                Err(r) => Json::object()
                    .with("prefab", q.prefab.as_str())
                    .with("at", cell)
                    .with("ok", false)
                    .with("reason", r.reason)
                    .with("cell", r.cell)
                    .with("blocked_by", r.by.and_then(|id| w.get(id)).map(slate_sim::Entity::label))
                    .with("tap", tap(cell))
                    .with("message", format!("`{}` does not fit at ({}, {}): {}", q.prefab, cell[0], cell[1], r.why)),
            },
            None => {
                let size = w.bounds.unwrap_or([0, 0]);
                let mut free = Vec::new();
                for y in 0..size[1] {
                    for x in 0..size[0] {
                        if w.can_place(&q.prefab, [x, y]).is_ok() {
                            free.push(Json::object().with("cell", [x, y]).with("tap", tap([x, y])));
                        }
                    }
                }
                Json::object()
                    .with("prefab", q.prefab.as_str())
                    .with("ok", !free.is_empty())
                    .with("count", free.len())
                    .with("free", free)
                    .with(
                        "message",
                        "cells where the prefab's back corner can go right now; `tap` is a screen pixel that taps \
                         that ground cell (a playtest `[[input]] tap`)",
                    )
            }
        }
    }
}

/// `inspect --place <prefab>[@x,y]`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaceQuery {
    pub prefab: String,
    pub at: Option<[i32; 2]>,
}

impl PlaceQuery {
    /// `house` or `house@3,4`.
    pub fn parse(s: &str) -> Result<PlaceQuery, String> {
        let (prefab, at) = match s.split_once('@') {
            None => (s, None),
            Some((p, c)) => {
                let nums: Vec<Option<i32>> = c.split(',').map(|v| v.trim().parse().ok()).collect();
                match nums.as_slice() {
                    [Some(x), Some(y)] => (p, Some([*x, *y])),
                    _ => return Err(format!("`{c}` is not a cell; write `{p}@3,4`")),
                }
            }
        };
        if prefab.is_empty() {
            return Err("missing prefab name; write `house` or `house@3,4`".into());
        }
        Ok(PlaceQuery { prefab: prefab.to_string(), at })
    }
}

/// Filters for [`Session::inspect`].
#[derive(Clone, Debug, Default)]
pub struct InspectOptions {
    /// Only this entity (by name); also filters events.
    pub entity: Option<String>,
    /// Only entities with this component (`sprite`, `trigger`, ...).
    pub component: Option<String>,
    pub events: bool,
    /// Only events from this frame on.
    pub since: u64,
    /// Answer "can I build here?" for a spawned prefab.
    pub place: Option<PlaceQuery>,
}

fn screen_json(list: &DrawList, id: u32) -> Json {
    let r = list.bounds_of(id);
    if r.is_empty() { Json::Null } else { Json::from(r.xywh()) }
}
