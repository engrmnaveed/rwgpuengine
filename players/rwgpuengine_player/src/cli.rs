//! The player's command line: `rwgpu play`'s arguments, the engine's own
//! project loading, and results printed the way `rwgpu` prints them.
//!
//! Nothing here is a second copy of the agent's half: the flags are the `play`
//! entry of `rwgpuengine_cli::args::COMMANDS`, the project is found and loaded
//! by `rwgpuengine_cli::commands::load_session`, and the result goes through
//! `rwgpuengine_cli::out::emit`, `--json` included.

use std::path::{Path, PathBuf};

use rwgpuengine_cli::args::{self, Parsed};
use rwgpuengine_cli::commands::{Loaded, load_session};
use rwgpuengine_cli::out::{self, Outcome, display_path};
use rwgpuengine_cli::project::write_file;
use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_core::json::Json;
use rwgpuengine_render::image::encode_png;
use rwgpuengine_runtime::{DataOnly, Game};

use crate::clock::{Timings, Watch};
use crate::gpu::{self, Gpu};
use crate::play::Player;
use crate::window;

/// Run the player with `args` (without the program name). Returns the exit code.
pub fn main_with(args: Vec<String>) -> i32 {
    let mut full = vec!["play".to_string()];
    full.extend(args);
    let json = full.iter().any(|a| a == "--json");
    let parsed = match args::parse(&full) {
        Ok(p) => p,
        Err(d) => return out::emit("play", Outcome::usage(d), json),
    };
    if parsed.help() {
        let mut o = Outcome::new(true);
        let play = parsed.cmd;
        o.text = format!(
            "rwgpu-player: the native player that `rwgpu play` starts. It takes the same arguments.\n\n{}",
            args::command_text(play)
        );
        o.data = args::command_json(play);
        return out::emit("help", o, parsed.json());
    }
    out::emit("play", play(&parsed), parsed.json())
}

fn play(p: &Parsed) -> Outcome {
    let Loaded { loc, session } = match load_session(p, &|| Box::new(DataOnly) as Box<dyn Game>) {
        Ok(l) => l,
        Err(o) => return o,
    };
    let end = match p.number("--frames") {
        Ok(n) => n.or(session.script_frames()),
        Err(d) => return Outcome::usage(d),
    };
    let headless = p.has("--headless");
    if headless && end.is_none() {
        return Outcome::usage(args::bad_arg(
            "`--headless` needs an end: `--frames N`, or `--input` with a playtest".into(),
            Some("for example `rwgpu play --headless --frames 300`".into()),
        ));
    }
    let screenshot = p.value("--screenshot").map(|s| std::env::current_dir().unwrap_or_default().join(s));
    let warnings = session.warnings.clone();
    let mut o = if headless {
        headless_run(session, end, screenshot.as_deref())
    } else {
        windowed(session, end, screenshot.as_deref())
    }
    .unwrap_or_else(|d| Outcome::failed(vec![d]));
    o.diagnostics.splice(0..0, warnings);
    o.with_root(&loc.root).settle()
}

fn windowed(
    session: rwgpuengine_runtime::Session,
    end: Option<u64>,
    shot: Option<&Path>,
) -> Result<Outcome, Diagnostic> {
    let ran = window::play(session, end, shot.is_some())?;
    let mut o = result(&ran.player, &ran.gpu, true, ran.closed_early);
    let [w, h] = ran.window;
    let area = crate::fit::letterbox(ran.player.frame_size(), ran.window);
    o.data.set(
        "window",
        Json::object()
            .with("size", [w, h])
            .with("scale_factor", ran.scale_factor)
            .with("frame_area", area)
            .with("format", format!("{:?}", ran.format)),
    );
    o.data.set("presents", timing(&ran.present_us));
    o.data.set("behind", Json::object().with("wakeups", ran.steps.behind).with("steps_let_go", ran.steps.dropped));
    o.line(format!(
        "  window      {w} x {h} pixels (scale {}), the frame at {} x {}{}",
        ran.scale_factor,
        area[2],
        area[3],
        if [area[2], area[3]] == [w, h] { "" } else { " with bars around it" }
    ));
    o.line(format!("  showing     {}", line(&ran.present_us)));
    if ran.steps.behind > 0 {
        o.line(format!(
            "  behind      {} time(s): the game slowed down for {} step(s) rather than hurry",
            ran.steps.behind, ran.steps.dropped
        ));
    }
    if let Some(path) = shot {
        let s =
            ran.shot.ok_or_else(|| gpu::failed("the window closed before its last picture was read back".into()))?;
        save(&mut o, path, s.width, s.height, &s.rgba, s.from)?;
    }
    Ok(o)
}

fn headless_run(
    session: rwgpuengine_runtime::Session,
    end: Option<u64>,
    shot: Option<&Path>,
) -> Result<Outcome, Diagnostic> {
    let gpu = Gpu::open(&gpu::instance(), None)?;
    let watch = Watch::start();
    let mut player = Player::new(session, &gpu.device, end);
    player.run_headless(&gpu, &watch)?;
    let mut o = result(&player, &gpu, false, false);
    if let Some(path) = shot {
        let rgba = player.frame_bytes(&gpu)?;
        let [w, h] = player.frame_size();
        save(&mut o, path, w, h, &rgba, "frame")?;
    }
    Ok(o)
}

/// What every run reports: the session's summary (frame, state hash, ...),
/// the adapter, and how many frames were drawn and how long that took.
fn result(player: &Player, gpu: &Gpu, in_window: bool, closed_early: bool) -> Outcome {
    let s = &player.session;
    let mut o = Outcome::new(true);
    o.data = s.summary();
    o.data.set("adapter", gpu.json());
    o.data.set("headless", !in_window);
    o.data.set("end", player.end);
    o.data.set("finished", player.finished());
    o.data.set("frames_drawn", player.draws);
    o.data.set("lists", timing(&player.list_us));
    o.data.set("draws", timing(&player.draw_us));
    let place = if in_window { "in a window" } else { "with no window" };
    o.line(format!(
        "played {} to frame {} {place} on {}{}",
        s.scene.path,
        s.frame(),
        gpu::describe(&gpu.info),
        s.playtest.as_ref().map(|t| format!(", input {}", t.path)).unwrap_or_default()
    ));
    if closed_early {
        o.line(format!("  closed      before frame {}: the window was closed", player.end.unwrap_or(0)));
    }
    o.line(format!("  state hash  {}", s.hash_hex()));
    o.line(format!("  drawn       {} frame(s) of {}: the others showed nothing new", player.draws, s.frame() + 1));
    o.line(format!("  lists       {}", line(&player.list_us)));
    o.line(format!("  draws       {}", line(&player.draw_us)));
    o
}

fn timing(t: &Timings) -> Json {
    let (median, max) = t.summary();
    Json::object().with("count", t.len()).with("median_us", median).with("max_us", max)
}

fn line(t: &Timings) -> String {
    let (median, max) = t.summary();
    format!("{} times, median {median} us, max {max} us", t.len())
}

fn save(o: &mut Outcome, path: &Path, w: u32, h: u32, rgba: &[u8], from: &str) -> Result<(), Diagnostic> {
    let png = encode_png(w, h, rgba).map_err(|e| gpu::failed(format!("the screenshot could not be encoded: {e}")))?;
    write_file(path, &png)?;
    let shown = display_path(&PathBuf::from(path));
    o.data.set("screenshot", Json::object().with("path", shown.as_str()).with("size", [w, h]).with("from", from));
    let source = match from {
        "window" => "read back from the window",
        "offscreen" => "drawn again off screen: this window cannot be read back",
        _ => "the frame itself",
    };
    o.line(format!("  screenshot  {shown} ({w} x {h}, {source})"));
    Ok(())
}
