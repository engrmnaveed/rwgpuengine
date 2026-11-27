//! A session: one project + one scene + an optional input script, stepped
//! frame by frame. Every command is a pure function of (files, script, seed,
//! frame): `shot --frame 300` simply replays 300 steps.

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_core::hash;
use rwgpuengine_core::json::Json;
use rwgpuengine_core::limits::TAPS_PER_STEP;
use rwgpuengine_core::text::did_you_mean;
use rwgpuengine_data::{
    CameraMove, InputDef, InputKind, PlaytestDef, ProjectDef, SceneDef, SceneLoader, Vfs, load_playtest, load_project,
};
use rwgpuengine_render::camera::Bound;
use rwgpuengine_render::{
    Camera, Frame, FrameReport, Painter, Projected, Projector, RenderList, View, build_report, list, pick, project,
    render,
};
use rwgpuengine_sim::{ActionSet, EventKind, InputFrame, Systems, Tap, World, dump};

/// Rust game code plugged into a session (ADR 0006: data first, code
/// where data runs out). Systems run every step after the built-ins.
pub trait Game: Systems {
    fn name(&self) -> &str {
        "data-only"
    }
    /// Called once after the world is built, before frame 0 is stepped.
    fn on_start(&mut self, _world: &mut World) {}
    /// Extra state for `rwgpu inspect` (shown under "game").
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

    /// Camera moves for the step taken at `frame`, in file order.
    pub fn camera_at(&self, frame: u64) -> impl Iterator<Item = &CameraMove> {
        self.inputs.iter().filter(move |i| i.frame == frame).filter_map(|i| match &i.kind {
            InputKind::Camera(m) => Some(m),
            _ => None,
        })
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
    /// Where the picture is taken from. It is part of the session's state:
    /// moves arrive as input ([`Session::move_camera`], a playtest's
    /// `[[input]] camera`) and are replayed with it, because what a tap hits
    /// depends on the camera. It is not part of the world, so not of the
    /// state hash either; `inspect` and `run` report it next to that.
    pub camera: Camera,
    /// Where the camera's center may go, fixed when the session starts: the
    /// ground's cells (ADR 0030). Every move keeps the center inside it.
    pub camera_bound: Bound,
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
        let camera = Camera::new(&scene);
        let camera_bound = Bound::new(&world, camera.center);
        let script = Script::new(playtest.as_ref().map(|p| p.inputs.clone()).unwrap_or_default());
        Ok(Session { project, scene, playtest, world, view, camera, camera_bound, warnings: diags, script, game })
    }

    pub fn frame(&self) -> u64 {
        self.world.frame
    }

    /// Frames the script wants to run (playtest `frames`), if any.
    pub fn script_frames(&self) -> Option<u64> {
        self.playtest.as_ref().map(|p| p.frames)
    }

    /// Take one fixed step with scripted plus live input. The script's
    /// camera moves for this frame happen first, so its taps see the moved
    /// camera.
    pub fn step(&mut self, live: &LiveInput) {
        let frame = self.world.frame;
        let moves: Vec<CameraMove> = self.script.camera_at(frame).copied().collect();
        for m in &moves {
            self.move_camera(m);
        }
        let (pressed, down, mut taps) = self.script.at(frame);
        taps.extend(live.taps.iter().copied());
        let resolved = if taps.is_empty() {
            Vec::new()
        } else {
            // The world applies the first taps up to its limit and says how
            // many more came (E0305); those are not picked.
            let seen = self.projected(true);
            let most = TAPS_PER_STEP.get();
            taps.iter()
                .enumerate()
                .map(|(i, &(x, y))| if i < most { pick::pick(&seen, x, y) } else { Tap { x, y, ..Tap::default() } })
                .collect()
        };
        let input = InputFrame {
            pressed: pressed.union(live.pressed),
            down: down.union(live.down).union(live.pressed),
            taps: resolved,
        };
        self.world.tick(&input, self.game.as_mut());
    }

    /// Name what the event log writes in full from now on (ADR 0027): each
    /// name is an entity's name, `#ID`, an event kind (`moved`, `blocked`,
    /// ...) or `all`. An entity of that name wins over a kind. Set it before
    /// stepping: what was not written is not written later. `Err` is an E0202
    /// for a name that is none of these.
    pub fn watch(&mut self, names: &[&str]) -> Result<(), Diagnostic> {
        let mut watch = self.world.events.watch().clone();
        for &name in names {
            if let Some(id) = self.world.find(name) {
                watch.entity(id);
            } else if let Some(id) = name.strip_prefix('#').and_then(|n| n.parse::<u32>().ok()) {
                if self.world.get(id).is_none() {
                    let n = self.world.entities.len();
                    return Err(Diagnostic::new(&codes::UNKNOWN_ENTITY, format!("no entity #{id} to watch"))
                        .hint(format!("ids are 0 to {} at frame 0", n.saturating_sub(1))));
                }
                watch.entity(id);
            } else if let Some(kind) = EventKind::parse(name) {
                watch.kind(kind);
            } else if name == "all" {
                watch.all = true;
            } else {
                let names: Vec<String> = self.world.entities.iter().filter_map(|e| e.name.clone()).collect();
                let known = names.iter().map(String::as_str).chain(EventKind::ALL.iter().map(|k| k.name()));
                let hint = did_you_mean(name, known).unwrap_or_else(|| {
                    "watch an entity by name or `#ID`, an event kind (moved, blocked, trigger), or `all`".into()
                });
                return Err(
                    Diagnostic::new(&codes::UNKNOWN_ENTITY, format!("no entity named `{name}` to watch")).hint(hint)
                );
            }
        }
        self.world.events.set_watch(watch);
        Ok(())
    }

    /// The names of what the log writes in full, as `watch` takes them.
    pub fn watched(&self) -> Vec<String> {
        let w = self.world.events.watch();
        let mut out: Vec<String> = if w.all { vec!["all".into()] } else { Vec::new() };
        for &id in w.entities() {
            out.push(self.world.get(id).and_then(|e| e.name.clone()).unwrap_or_else(|| format!("#{id}")));
        }
        out.extend(w.kinds().iter().map(|k| k.name().to_string()));
        out
    }

    /// One diagnostic for each limit on work the world reached while it ran
    /// (`rwgpuengine_core::limits`): what it cut off, and where to look.
    pub fn limit_diagnostics(&self) -> Vec<Diagnostic> {
        let input = self.playtest.as_ref().map(|p| format!(" --input {}", p.path)).unwrap_or_default();
        let mut out = Vec::new();
        for hit in self.world.limits_hit() {
            let Some(code) = hit.limit.code else { continue };
            // The event's sentence, without the code the diagnostic shows.
            let what = hit.message.trim_end_matches(&format!(" ({})", code.code));
            let mut d =
                Diagnostic::new(code, format!("step {}: {what}", hit.first)).in_file(&self.scene.path).hint(format!(
                    "see that step: `rwgpu inspect{input} --frame {} --since {} --events`; `rwgpu explain {}`",
                    hit.first + 1,
                    hit.first,
                    code.code
                ));
            d = d.note(format!("the limit is {} {} (`rwgpu explain limits`)", hit.limit.value, hit.limit.name));
            if hit.steps > 1 {
                d = d.note(format!("{} steps reached it, the last one step {}", hit.steps, hit.last));
            }
            out.push(d);
        }
        out
    }

    /// Step until `frame` (no-op if already there or past it).
    pub fn run_to(&mut self, frame: u64) {
        let idle = LiveInput::default();
        while self.world.frame < frame {
            self.step(&idle);
        }
    }

    /// Move the camera, as a player's gesture would. Taps from now on are
    /// resolved against the new picture. The center stays inside
    /// [`Session::camera_bound`]: a pan past the edge of the ground stops at
    /// the edge.
    pub fn move_camera(&mut self, m: &CameraMove) {
        self.camera.apply_within(m, &self.camera_bound);
    }

    /// The screen pixel a world point is drawn at now, or `None` if it is
    /// behind the camera.
    pub fn screen_point(&self, p: [f32; 3]) -> Option<[f32; 2]> {
        Projector::new(&self.view, &self.camera).screen(p)
    }

    /// What the current frame draws: the list every renderer reads.
    pub fn render_list(&self, include_disabled: bool) -> RenderList {
        list::build(&self.world, &self.view, &self.camera, include_disabled)
    }

    /// The render list seen from the camera: what picking and the reference
    /// renderer work on.
    pub fn projected(&self, include_disabled: bool) -> Projected {
        project(&self.render_list(include_disabled))
    }

    /// Render the current frame and describe it: the reference pixels plus
    /// the frame report (`rwgpu shot`, playtest `[[shot]]`s).
    pub fn render(&self) -> (Frame, FrameReport) {
        let seen = self.projected(false);
        let frame = render(&seen);
        let report = build_report(&self.world, &seen, &frame);
        (frame, report)
    }

    /// Paint the current frame's colors into `rgba`: the same pixels as
    /// [`Session::render`], without the report, for showing the game. A
    /// player that paints every frame should use a
    /// [`Presenter`](crate::Presenter), which skips frames that did not change.
    pub fn paint(&self, rgba: &mut Vec<u8>) {
        Painter::new().paint(&self.render_list(false), rgba);
    }

    pub fn state_hash(&self) -> u64 {
        self.world.state_hash()
    }

    /// The world as JSON: summary, entities (with screen rectangles) and,
    /// optionally, events.
    pub fn inspect(&self, opts: &InspectOptions) -> Json {
        let seen = self.projected(false);
        let frame = render(&seen);
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
            j.set("screen", screen_json(&seen, e.id));
            // A pixel where a tap is guaranteed to hit this entity.
            j.set("tap", pick::tap_point(&seen, &frame, e.id).map(Json::from));
            entities.push(j);
        }
        let mut out = dump::world_summary(&self.world)
            .with("scene", self.scene.path.as_str())
            .with("input", self.playtest.as_ref().map(|p| p.path.clone()))
            .with("camera", self.camera_json())
            .with("ground", dump::ground_json(&self.world))
            .with("entities", entities);
        if let Some(q) = &opts.place {
            out.set("place", self.place_json(q, &seen, &frame));
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
            .with("camera", self.camera_json())
            .with("game", self.game.name())
    }

    /// The camera as it is now: `{ view, center, turn, zoom, bound }`, where
    /// `bound` is `[min_x, min_y, max_x, max_y]`, the rectangle the center
    /// stays in.
    pub fn camera_json(&self) -> Json {
        let (c, b) = (&self.camera, &self.camera_bound);
        Json::object()
            .with("view", c.view.name())
            .with("center", c.center)
            .with("turn", c.turn as f64)
            .with("zoom", c.zoom as f64)
            .with("bound", [b.min[0], b.min[1], b.max[0], b.max[1]])
    }

    pub fn hash_hex(&self) -> String {
        hash::hex(self.state_hash())
    }

    /// "Can I build here?": where `prefab` can be spawned right now, or
    /// whether it fits at one cell and why not. `seen` and `frame` are the
    /// current picture (`projected(false)` and its rendering).
    pub fn place_json(&self, q: &PlaceQuery, seen: &Projected, frame: &Frame) -> Json {
        let w = &self.world;
        let tap = |cell: [i32; 2]| pick::ground_tap_point(seen, frame, cell).map(Json::from);
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
                    .with("blocked_by", r.by.and_then(|id| w.get(id)).map(rwgpuengine_sim::Entity::label))
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

fn screen_json(list: &Projected, id: u32) -> Json {
    let r = list.bounds_of(id);
    if r.is_empty() { Json::Null } else { Json::from(r.xywh()) }
}
