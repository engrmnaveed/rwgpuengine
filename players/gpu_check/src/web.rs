//! The web player's pictures against the reference.
//!
//! The browser check (`players/rwgpuengine_player/web/check.mjs`, run by
//! `cargo xtask gpu`) plays every example playtest in a real browser and
//! writes the frame WebGPU drew at the end of each as RGBA bytes, with a line
//! in `frames.txt`: `<example> <playtest> <frame> <width> <height> <file>`.
//! Here each playtest is replayed by the engine to that frame, its render list
//! drawn by the CPU reference renderer, and the two pictures compared by the
//! rule the native renderer is held to: every differing pixel on an edge of
//! the reference picture, at most `--max-pixels` in one picture. No GPU is
//! needed for this part.

use std::path::Path;

use rwgpuengine_data::Vfs;
use rwgpuengine_render::{project, render};
use rwgpuengine_runtime::{DataOnly, Session, SessionConfig};

use crate::{Args, compare, write_pictures};

/// Compare every picture listed in `dir/frames.txt`. Prints a line per
/// picture; returns (pictures, the most pixels apart in one, all green).
pub fn check(root: &Path, a: &Args, dir: &Path) -> Result<(usize, usize, bool), String> {
    let list = dir.join("frames.txt");
    let text = std::fs::read_to_string(&list).map_err(|e| format!("{}: {e}", list.display()))?;
    let (mut count, mut worst, mut ok) = (0, 0, true);
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let words: Vec<&str> = line.split_whitespace().collect();
        let &[example, test, frame, width, height, file] = words.as_slice() else {
            return Err(format!("{}: a line needs six words: `{line}`", list.display()));
        };
        let number = |s: &str| s.parse::<u64>().map_err(|e| format!("{}: `{s}`: {e}", list.display()));
        let (frame, width, height) = (number(frame)?, number(width)?, number(height)?);
        let name = format!("{example}/{test}");
        let vfs = Vfs::from_dir(&root.join("examples").join(example)).map_err(|e| format!("{name}: {e}"))?;
        let cfg = SessionConfig { script: Some(test.to_string()), ..Default::default() };
        let mut session = Session::load(&vfs, &cfg, Box::new(DataOnly))
            .map_err(|d| format!("{name}: {}", d.iter().map(|d| d.render_human()).collect::<String>()))?;
        session.run_to(frame);
        let seen = project(&session.render_list(false));
        let reference = render(&seen);
        let got = std::fs::read(dir.join(file)).map_err(|e| format!("{}: {e}", dir.join(file).display()))?;
        if (u64::from(reference.width), u64::from(reference.height)) != (width, height)
            || got.len() != reference.rgba.len()
        {
            return Err(format!(
                "{name}: the browser's frame is {width} x {height} ({} bytes), the reference's {} x {}",
                got.len(),
                reference.width,
                reference.height
            ));
        }
        let outcome = compare(&seen, &reference, &got);
        let good = outcome.inside == 0 && outcome.differing <= a.max_pixels;
        println!(
            "{} {:<40} frame {:>4}  {:>3} apart, {} not on an edge",
            if good { "ok  " } else { "FAIL" },
            name,
            frame,
            outcome.differing,
            outcome.inside
        );
        if !good || a.pictures {
            for l in &outcome.first {
                println!("       {l}");
            }
            write_pictures(&a.out, &format!("web_{name}"), &reference, &got, &outcome.mask)?;
        }
        count += 1;
        worst = worst.max(outcome.differing);
        ok &= good;
    }
    if count == 0 {
        return Err(format!("{}: no pictures to compare", list.display()));
    }
    Ok((count, worst, ok))
}
