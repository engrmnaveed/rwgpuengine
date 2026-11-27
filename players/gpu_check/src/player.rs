//! The native player against the engine and the reference.
//!
//! Every playtest of the example games is replayed by the player's own loop
//! (`rwgpuengine_player::Player`, with no window): the steps, the list built
//! after each, a GPU draw whenever it changed. At the end the session must be
//! in the state the engine reaches by itself, and the frame the player last
//! drew must be within the pixel limit of the reference's picture of the same
//! list, every differing pixel on an edge.
//!
//! `cargo xtask gpu` also runs the `rwgpu-player` program on the same
//! playtests and holds its state hashes to `rwgpu test`'s.

use std::path::Path;

use rwgpuengine_data::Vfs;
use rwgpuengine_player::clock::Watch;
use rwgpuengine_player::{Gpu, Player, gpu};
use rwgpuengine_render::{project, render};
use rwgpuengine_runtime::{DataOnly, Session, SessionConfig};

use crate::{Args, compare, write_pictures};

pub const EXAMPLES: [&str; 4] = ["iso_garden", "coin_vault", "tiny_town", "river_town"];

/// One replayed playtest.
pub struct Replayed {
    pub name: String,
    pub differing: usize,
    pub ok: bool,
}

/// Replay every example playtest with the player and compare. Prints a line
/// per playtest.
pub fn check(root: &Path, a: &Args) -> Result<Vec<Replayed>, String> {
    let gpu = Gpu::open(&gpu::instance(), None).map_err(|d| d.render_human())?;
    let watch = Watch::start();
    let mut out = Vec::new();
    for example in EXAMPLES {
        let dir = root.join("examples").join(example);
        let vfs = Vfs::from_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let tests: Vec<String> = vfs.paths_under("tests", ".toml").map(str::to_string).collect();
        for test in tests {
            let name = format!("{example}/{test}");
            let cfg = SessionConfig { script: Some(test.clone()), ..Default::default() };
            let load = || {
                Session::load(&vfs, &cfg, Box::new(DataOnly))
                    .map_err(|d| format!("{name}: {}", d.iter().map(|d| d.render_human()).collect::<String>()))
            };
            // The engine by itself: what `rwgpu run` and `rwgpu test` reach.
            let mut engine = load()?;
            let end = engine.script_frames().ok_or(format!("{name}: no frames"))?;
            engine.run_to(end);

            let mut player = Player::new(load()?, &gpu.device, Some(end));
            player.run_headless(&gpu, &watch).map_err(|d| format!("{name}: {}", d.render_human()))?;
            let got = player.frame_bytes(&gpu).map_err(|d| format!("{name}: {}", d.render_human()))?;
            let list = player.session.render_list(false);
            let seen = project(&list);
            let frame = render(&seen);
            let outcome = compare(&seen, &frame, &got);
            let same_state = player.session.hash_hex() == engine.hash_hex() && player.session.frame() == end;
            let ok = same_state && outcome.inside == 0 && outcome.differing <= a.max_pixels;
            println!(
                "{} {:<40} frame {:>4}  state {} {}  {:>3} apart, {} not on an edge   ({} of {} frames drawn)",
                if ok { "ok  " } else { "FAIL" },
                name,
                player.session.frame(),
                player.session.hash_hex(),
                if same_state { "=" } else { "!= the engine's" },
                outcome.differing,
                outcome.inside,
                player.draws,
                end + 1,
            );
            if !same_state {
                println!("       the engine reaches {} at frame {end}", engine.hash_hex());
            }
            if !ok || a.pictures {
                for line in &outcome.first {
                    println!("       {line}");
                }
                write_pictures(&a.out, &format!("player_{name}"), &frame, &got, &outcome.mask)?;
            }
            out.push(Replayed { name, differing: outcome.differing, ok });
        }
    }
    Ok(out)
}
