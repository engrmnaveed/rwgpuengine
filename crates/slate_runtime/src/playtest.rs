//! Running playtests: step the scripted session, check `[[expect]]`s at
//! their frames and capture `[[shot]]` frames for comparison.
//!
//! Image comparison against reference files happens in the CLI (it owns the
//! disk); everything else lives here so the web player could run tests too.

use slate_core::codes;
use slate_core::diag::Diagnostic;
use slate_data::{ExpectDef, PlaytestDef, ShotDef, Vfs};
use slate_render::{Frame, FrameReport};
use slate_sim::EventKind;

use crate::session::{Game, Session, SessionConfig};

/// A frame captured for a `[[shot]]`.
#[derive(Debug)]
pub struct ShotCapture {
    pub def: ShotDef,
    pub frame_no: u64,
    pub frame: Frame,
    pub report: FrameReport,
}

#[derive(Debug)]
pub struct PlaytestRun {
    pub name: String,
    pub path: String,
    pub scene: String,
    pub frames: u64,
    pub seed: u64,
    pub state_hash: String,
    pub expects_checked: usize,
    /// One `E0401` per failed expectation.
    pub failures: Vec<Diagnostic>,
    pub shots: Vec<ShotCapture>,
    pub warnings: Vec<Diagnostic>,
}

/// Run a playtest to its last frame. `Err` if it cannot be loaded.
pub fn run_playtest(vfs: &Vfs, path: &str, game: Box<dyn Game>) -> Result<PlaytestRun, Vec<Diagnostic>> {
    let cfg = SessionConfig { script: Some(path.to_string()), ..SessionConfig::default() };
    let mut session = Session::load(vfs, &cfg, game)?;
    let def = session.playtest.clone().expect("playtest session");
    let frames = def.frames;
    let mut failures = Vec::new();
    let mut shots = Vec::new();
    let mut checked = 0;
    loop {
        let f = session.frame();
        for exp in def.expects.iter().filter(|e| e.frame.unwrap_or(frames) == f) {
            checked += 1;
            if let Some(d) = check_expect(&session, &def, exp, f) {
                failures.push(d);
            }
        }
        for shot in def.shots.iter().filter(|s| s.frame.unwrap_or(frames) == f) {
            let (frame, report) = session.render();
            shots.push(ShotCapture { def: shot.clone(), frame_no: f, frame, report });
        }
        if f >= frames {
            break;
        }
        session.step(&Default::default());
    }
    Ok(PlaytestRun {
        name: def.name.clone(),
        path: def.path.clone(),
        scene: session.scene.path.clone(),
        frames,
        seed: session.world.seed,
        state_hash: session.hash_hex(),
        expects_checked: checked,
        failures,
        shots,
        warnings: session.warnings.clone(),
    })
}

fn fmt_pos(p: [f32; 3]) -> String {
    let n = |v: f32| {
        if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{v}") }
    };
    format!("[{}, {}, {}]", n(p[0]), n(p[1]), n(p[2]))
}

/// `None` if the expectation holds, else an E0401 with notes.
fn check_expect(session: &Session, def: &PlaytestDef, exp: &ExpectDef, frame: u64) -> Option<Diagnostic> {
    let world = &session.world;
    let inspect_hint = |entity: Option<&str>| {
        let mut cmd = format!("slate inspect --input {} --frame {frame} --events", def.path);
        if let Some(e) = entity {
            cmd.push_str(&format!(" --entity {e}"));
        }
        format!("see what happened: `{cmd}`")
    };

    if let Some(kind) = &exp.event {
        let kind = EventKind::parse(kind)?;
        let id = exp.entity.as_ref().and_then(|n| world.find(n));
        let other = exp.other.as_ref().and_then(|n| world.find(n));
        let matching: Vec<_> = world
            .events
            .all()
            .iter()
            .filter(|e| e.kind == kind && e.frame < frame)
            .filter(|e| id.is_none_or(|id| e.entity == Some(id)))
            .filter(|e| other.is_none_or(|o| e.other == Some(o)))
            .filter(|e| exp.signal.as_ref().is_none_or(|s| e.signal.as_ref() == Some(s)))
            .filter(|e| exp.reason.as_ref().is_none_or(|r| e.data.get("reason").and_then(|v| v.as_str()) == Some(r)))
            .collect();
        let n = matching.len() as u64;
        if n >= exp.at_least && exp.at_most.is_none_or(|hi| n <= hi) {
            return None;
        }
        let mut what = format!("`{}` event(s)", kind.name());
        if let Some(e) = &exp.entity {
            what.push_str(&format!(" whose subject is `{e}`"));
        }
        if let Some(o) = &exp.other {
            what.push_str(&format!(" with other party `{o}`"));
        }
        if let Some(s) = &exp.signal {
            what.push_str(&format!(" with signal '{s}'"));
        }
        if let Some(r) = &exp.reason {
            what.push_str(&format!(" with reason '{r}'"));
        }
        let bound = match exp.at_most {
            Some(0) => "no".to_string(),
            Some(hi) if exp.at_least == 0 => format!("at most {hi}"),
            Some(hi) => format!("{} to {hi}", exp.at_least),
            None => format!("at least {}", exp.at_least),
        };
        let mut d =
            Diagnostic::new(&codes::EXPECT_FAILED, format!("expected {bound} {what} before frame {frame}, found {n}"))
                .at(&def.src, exp.span)
                .hint(inspect_hint(exp.entity.as_deref()));
        for ev in matching.iter().take(3) {
            d = d.note(format!("[{}] {}", ev.frame, ev.message));
        }
        return Some(d);
    }

    let name = exp.entity.as_deref()?;
    let Some(e) = world.by_name(name) else {
        return Some(
            Diagnostic::new(&codes::UNKNOWN_ENTITY, format!("[[expect]] names unknown entity `{name}`"))
                .at(&def.src, exp.span),
        );
    };
    let mut problems = Vec::new();
    if let Some(p) = exp.pos
        && (0..3).any(|i| (p[i] - e.pos[i]).abs() > 1e-4)
    {
        problems.push(format!("pos = {} (found {})", fmt_pos(p), fmt_pos(e.pos)));
    }
    if let Some(v) = exp.enabled
        && v != e.enabled
    {
        problems.push(format!("enabled = {v} (found {})", e.enabled));
    }
    if let Some(v) = exp.solid
        && v != e.solid
    {
        problems.push(format!("solid = {v} (found {})", e.solid));
    }
    for (k, v) in &exp.props {
        match e.props.get(k) {
            Some(actual) if actual.same(v) => {}
            Some(actual) => problems.push(format!("props.{k} = {} (found {})", v.literal(), actual.literal())),
            None => problems.push(format!("props.{k} = {} (not set)", v.literal())),
        }
    }
    if let Some(want) = &exp.text {
        match e.label_text() {
            Some(shown) if &shown == want => {}
            Some(shown) => problems.push(format!("text = {want:?} (shown: {shown:?})")),
            None => problems.push(format!("text = {want:?} (it has no label)")),
        }
    }
    if problems.is_empty() {
        return None;
    }
    let mut d =
        Diagnostic::new(&codes::EXPECT_FAILED, format!("expected `{name}` {} at frame {frame}", problems.join(", ")))
            .at(&def.src, exp.span)
            .hint(inspect_hint(Some(name)));
    let recent: Vec<_> = world.events.about(e.id).filter(|ev| ev.frame < frame).collect();
    for ev in recent.iter().rev().take(3).rev() {
        d = d.note(format!("frame {}: {}", ev.frame, ev.message));
    }
    if recent.is_empty() {
        d = d.note(format!("no events involve `{name}` before frame {frame}"));
    }
    Some(d)
}
