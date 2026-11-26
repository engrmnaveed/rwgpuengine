//! The reference scenes: the render lists both renderers draw.
//!
//! - The three example games at every frame their playtests shoot.
//! - The generated town of 5,000 boxes at 1280 × 720 in perspective.
//! - Small scenes, one per rule of the reference renderer, each under iso
//!   and perspective cameras.
//! - 200 random scenes, each under a random camera, the same on every run:
//!   they look for what the hand-made scenes miss.
//!
//! One thing is kept out of these scenes on purpose, because there the
//! reference's own picture is decided by rounding (ADR 0021, "what stays
//! undefined"): two faces that are not level, in one plane and over each
//! other, as when two boxes stand in one cell and their sides coincide.

use std::path::Path;

use rwgpuengine_data::{CameraMove, CameraView, Vfs};
use rwgpuengine_render::RenderList;
use rwgpuengine_runtime::{DataOnly, Session, SessionConfig};

pub struct Scene {
    pub name: String,
    pub list: RenderList,
}

fn session(vfs: &Vfs, script: Option<&str>) -> Result<Session, String> {
    let cfg = SessionConfig { script: script.map(str::to_string), ..SessionConfig::default() };
    Session::load(vfs, &cfg, Box::new(DataOnly))
        .map_err(|d| d.iter().map(|d| d.render_human()).collect::<Vec<_>>().join("\n"))
}

/// The generated town (`xtask/src/town.rs`), as its scene's camera sees it
/// (in perspective), or through the iso view at the same zoom.
pub fn town_list(boxes: usize, iso: bool) -> Result<RenderList, String> {
    let mut s = session(&crate::town::town(boxes), None)?;
    if iso {
        s.move_camera(&CameraMove { view: Some(ISO), turn: 0.0, zoom: 1.0, pan: [0.0, 0.0] });
    }
    Ok(s.render_list(false))
}

/// How many random scenes the gate draws after the fixed ones.
pub const RANDOM_SCENES: u64 = 200;

type Cam = (CameraView, f32, f32, [f32; 2]);

const ISO: CameraView = CameraView::Iso;
const PER: CameraView = CameraView::Perspective;

/// The cameras of the reference renderer's own tests, and two that are
/// panned: the starting view, quarter and odd turns, zooms, perspective.
const CAMERAS: [Cam; 9] = [
    (ISO, 0.0, 1.0, [0.0, 0.0]),
    (ISO, 90.0, 1.0, [0.0, 0.0]),
    (ISO, 217.0, 1.75, [0.0, 0.0]),
    (ISO, 33.0, 0.5, [0.0, 0.0]),
    (ISO, 180.0, 1.25, [1.5, -0.75]),
    (PER, 0.0, 1.0, [0.0, 0.0]),
    (PER, 200.0, 0.8, [0.0, 0.0]),
    (PER, 295.0, 3.0, [0.0, 0.0]),
    (PER, 60.0, 1.5, [-1.0, 0.5]),
];

/// Instances: boxes and flat rectangles under the light, level faces in one
/// plane by rank (a tile on the ground and on a box, a block of height 0 on
/// a tile), a tile under the foot of a box, boxes on boxes, a box in the air.
/// Some flat things are larger or smaller than what they lie on (a terrace
/// of four cells, a skylight on one cell of a roof, a raft over the water),
/// and of two tiles in one plane the first one stays.
const SOLIDS: &str = r##"
[ground]
map = ["ggggwwgg", "ggggwwgg", "ggppppgg", "ggpggpgg", "ggpggpgg", "ggppppgg", "gggggggg", "gggggggg"]

[[terrain]]
char = "g"
color = "#3f8a3f"
alt_color = "#3a8039"

[[terrain]]
char = "w"
color = "#2a5d8f"

[[terrain]]
char = "p"
color = "#b9a26a"
alt_color = "#b09a62"

[[entity]]
name = "house"
pos = [0, 0]
footprint = [2, 2]
block = { height = 1.5, color = "#b5651d", top = "#8b2f2f" }

[[entity]]
name = "tower"
pos = [7, 0]
block = { height = 3, color = "#c8c8c8", top = "#5a6ea0" }

[[entity]]
name = "low"
pos = [3, 0]
block = { height = 0.4, color = "#80868f" }

[[entity]]
name = "odd"
pos = [6, 3]
block = { height = 1.2, color = "#404040", top = "#ffffff" }

[[entity]]
name = "plate"
pos = [1, 3]
tile = { color = "#d9c27a" }

[[entity]]
name = "table"
pos = [3, 3]
block = { color = "#808080", height = 1 }

[[entity]]
name = "mat"
pos = [3, 3, 1]
tile = { color = "#00c060" }

[[entity]]
name = "rug"
pos = [1, 5]
tile = { color = "#0000ff" }
block = { color = "#e6c850", height = 0 }

[[entity]]
name = "crate"
pos = [4, 6]
tile = { color = "#ff00ff" }
block = { color = "#404040", height = 1 }

[[entity]]
name = "stack"
pos = [6, 6]
block = { height = 0.5, color = "#a05a2c" }

[[entity]]
name = "upper"
pos = [6, 6, 0.5]
block = { height = 0.75, color = "#2c5aa0", top = "#f0f0f0" }

[[entity]]
name = "float"
pos = [0, 6, 3]
block = { height = 0.5, color = "#c04040", top = "#40c040" }

[[entity]]
name = "wide"
pos = [3, 7]
footprint = [3, 1]
block = { height = 0.25, color = "#707070" }

[[entity]]
cells = [[7, 3], [7, 4], [7, 5]]
block = { height = 0.8, color = "#3f6f3f" }

[[entity]]
name = "terrace"
pos = [4, 4]
footprint = [2, 2]
tile = { color = "#c96f4a" }

[[entity]]
name = "inlay"
pos = [5, 5]
tile = { color = "#ffffff" }

[[entity]]
name = "skylight"
pos = [1, 0, 1.5]
tile = { color = "#9fd8ff" }

[[entity]]
name = "raft"
pos = [4, 0]
footprint = [2, 2]
block = { height = 0, color = "#8a6a3a" }
"##;

/// Cards: every sprite shape and text, standing on their anchor. A tall
/// sprite in front of a wall, a sprite behind a box, a card on top of its
/// entity's block, a label in front of its own sprite, a sprite wider than
/// its cell. Sprites grow with the camera; text keeps its size.
const CARDS: &str = r##"
[ground]
size = [6, 6]

[[entity]]
name = "wall"
pos = [0, 0]
footprint = [6, 1]
block = { height = 2.5, color = "#606878" }

[[entity]]
name = "tall"
pos = [2, 1]
sprite = { shape = "rect", color = "#ff8800", size = [14, 70] }

[[entity]]
name = "ball"
pos = [1, 3]
sprite = { shape = "circle", color = "#ff3030", size = [30, 22], offset = [0, -3] }

[[entity]]
name = "gem"
pos = [3, 2]
sprite = { shape = "diamond", color = "#4fc3f7", size = [21, 27] }

[[entity]]
name = "hero"
pos = [2, 4]
sprite = { shape = "rect", color = "#2060c0", size = [18, 30] }
label = { text = "Hero", color = "#ffffff", offset = [0, -12] }

[[entity]]
name = "pillar"
pos = [4, 3]
block = { height = 1.5, color = "#907040" }
sprite = { shape = "circle", color = "#ffe060", size = [20, 20] }
label = { text = "Top {n}", scale = 2 }
props = { n = 3 }

[[entity]]
name = "hidden"
pos = [4, 2]
sprite = { shape = "rect", color = "#ff00ff", size = [16, 24] }

[[entity]]
name = "sign"
pos = [0, 5]
label = { text = "Sign\nq~j|", color = "#202020", scale = 3 }

[[entity]]
name = "wide"
pos = [5, 5]
sprite = { shape = "rect", color = "#30a060", size = [60, 16], offset = [0, 3] }

[[entity]]
name = "post"
pos = [5, 4]
block = { height = 1, color = "#a03030" }

[[entity]]
name = "big"
pos = [3, 5]
sprite = { shape = "diamond", color = "#c040c0", size = [44, 44], offset = [0, -4] }
"##;

/// Cards that reach below their anchor, where no floor is at the anchor's
/// height: the lower part lies in the level plane through the anchor, so a
/// box in front covers it and a lower box does not.
const CARDS_BELOW: &str = r##"
[ground]
size = [6, 6]

[[entity]]
name = "float"
pos = [2, 2, 1.5]
sprite = { shape = "rect", color = "#ff8800", size = [24, 40], offset = [0, 20] }

[[entity]]
name = "tag"
pos = [4, 2, 1]
label = { text = "Low", scale = 2, offset = [0, 10], color = "#ffffff" }

[[entity]]
name = "front"
pos = [3, 3]
block = { height = 2, color = "#4060a0" }

[[entity]]
name = "back"
pos = [1, 1]
block = { height = 3, color = "#a06040" }

[[entity]]
name = "ball"
pos = [4, 4, 2]
sprite = { shape = "circle", color = "#30c0c0", size = [30, 30], offset = [0, 15] }

[[entity]]
name = "low"
pos = [2, 3]
block = { height = 1, color = "#508050" }
"##;

/// Cards that reach below their anchor onto the floor they stand on: the
/// ground, a tile, a tile of four cells, the top of their own box. The floor
/// does not cut them; a box in front does.
const CARDS_ON_A_FLOOR: &str = r##"
[ground]
size = [5, 5]

[[entity]]
name = "nudged"
pos = [1, 1]
sprite = { shape = "rect", color = "#ff0000", size = [16, 24], offset = [0, 10] }

[[entity]]
name = "tag"
pos = [3, 1]
label = { text = "Tag", scale = 2, offset = [0, 8] }

[[entity]]
name = "both"
pos = [1, 3]
sprite = { shape = "circle", color = "#00a0ff", size = [28, 28], offset = [0, 12] }
label = { text = "ab", offset = [0, 10], color = "#ffff00" }

[[entity]]
name = "table"
pos = [3, 3]
block = { height = 1, color = "#808080" }
sprite = { shape = "diamond", color = "#ff00ff", size = [30, 30], offset = [0, 14] }

[[entity]]
name = "plate"
pos = [2, 2]
tile = { color = "#d9c27a" }
sprite = { shape = "rect", color = "#208020", size = [20, 20], offset = [0, 8] }

[[entity]]
name = "early"
pos = [0, 3]
sprite = { shape = "circle", color = "#ff8800", size = [24, 24], offset = [0, 12] }

[[entity]]
name = "lawn"
pos = [0, 3]
footprint = [2, 2]
tile = { color = "#5fae5f" }

[[entity]]
name = "front"
pos = [2, 4]
block = { height = 1.5, color = "#4060a0" }

[[entity]]
name = "shadow"
pos = [3, 2]
sprite = { shape = "rect", color = "#00000060", size = [40, 30], offset = [0, 22] }
"##;

/// See-through things: tiles, boxes, sprites and text, over solid things and
/// over each other, far to near; a solid thing drawn later behind them; a
/// see-through tile under its own solid block of height 0.
const SEE_THROUGH: &str = r##"
[ground]
size = [6, 6]

[[entity]]
name = "wall"
pos = [0, 0]
footprint = [2, 1]
block = { color = "#101010", top = "#202020", height = 2 }

[[entity]]
name = "pane"
pos = [1, 1]
sprite = { shape = "rect", color = "#ffffff80", size = [40, 80] }

[[entity]]
name = "far"
pos = [-1, -1]
sprite = { shape = "rect", color = "#ff0000", size = [20, 120] }

[[entity]]
name = "plate"
pos = [3, 1]
tile = { color = "#ffcc0080" }

[[entity]]
name = "glassbox"
pos = [3, 3]
block = { color = "#40a0ff80", top = "#ffffff60", height = 1.5 }

[[entity]]
name = "inside"
pos = [3, 3]
sprite = { shape = "circle", color = "#ff2020", size = [20, 20], offset = [0, -6] }

[[entity]]
name = "halfbox"
pos = [5, 1]
block = { color = "#208040", top = "#ffffff80", height = 1 }

[[entity]]
name = "smoke1"
pos = [1, 4]
sprite = { shape = "circle", color = "#ff000099", size = [50, 40] }

[[entity]]
name = "smoke2"
pos = [2, 5]
sprite = { shape = "circle", color = "#0000ff99", size = [50, 40], offset = [0, 6] }

[[entity]]
name = "smoke3"
pos = [2, 4]
sprite = { shape = "diamond", color = "#00ff0066", size = [50, 50] }

[[entity]]
name = "note"
pos = [4, 5]
block = { height = 0.5 }
label = { text = "Hi {n}!", color = "#ffffffcc", scale = 2 }
props = { n = 7 }

[[entity]]
name = "tint"
pos = [5, 4]
tile = { color = "#ff00ff40" }
block = { color = "#00ffff40", height = 0 }

[[entity]]
name = "rug"
pos = [4, 2]
tile = { color = "#ffcc0080" }
block = { color = "#804020", height = 0 }
"##;

/// The 2D layer: every shape and text at screen pixels, solid and
/// see-through, over each other and over the frame's edge, in list order.
const OVERLAY: &str = r##"
[ground]
size = [4, 4]

[[entity]]
name = "tower"
pos = [3, 0]
block = { height = 2 }

[[entity]]
name = "walker"
pos = [1, 1]
sprite = { shape = "rect", color = "#ff0000", size = [16, 24] }

[[entity]]
name = "hud"
screen_pos = [-6, 4]
sprite = { shape = "rect", color = "#00000088", size = [220, 44] }
label = { text = "Coins: {n}\nq~j", scale = 2, offset = [10, 3] }
props = { n = 12 }

[[entity]]
name = "badge"
screen_pos = [500, 20]
sprite = { shape = "circle", color = "#ffcc00", size = [60, 60] }
label = { text = "7", scale = 4, offset = [20, 14], color = "#000000" }

[[entity]]
name = "gem"
screen_pos = [560, 60]
sprite = { shape = "diamond", color = "#4fc3f7cc", size = [90, 70] }

[[entity]]
name = "veil"
screen_pos = [100, 300]
sprite = { shape = "rect", color = "#ffffff40", size = [600, 200] }

[[entity]]
name = "spot"
screen_pos = [300, 250]
sprite = { shape = "circle", color = "#ff000080", size = [200, 160] }
label = { text = "see-through text", color = "#ffffff99", offset = [20, 70] }

[[entity]]
name = "button"
screen_pos = [20, 400]
sprite = { shape = "rect", color = "#555555", size = [136, 44] }
label = { text = "Build", scale = 2, offset = [12, 14] }
"##;

/// Disabled things of every kind. The list is built the way picking builds
/// it, with them in it; nothing may draw them.
const DISABLED: &str = r##"
[ground]
size = [4, 4]

[[entity]]
name = "a"
pos = [1, 1]
block = { height = 1 }

[[entity]]
name = "b"
pos = [2, 1]
enabled = false
block = { height = 2, color = "#ff0000" }

[[entity]]
name = "c"
pos = [1, 2]
enabled = false
tile = { color = "#ff00ff" }
sprite = { color = "#00ff00", size = [20, 20] }
label = { text = "off" }

[[entity]]
name = "d"
pos = [2, 2]
sprite = { shape = "circle", color = "#ffff00", size = [20, 20] }

[[entity]]
name = "e"
screen_pos = [10, 10]
enabled = false
sprite = { shape = "rect", size = [100, 30] }
label = { text = "off" }

[[entity]]
name = "f"
screen_pos = [10, 50]
sprite = { shape = "rect", color = "#000000", size = [100, 30] }
label = { text = "on", offset = [4, 4] }

[[entity]]
name = "g"
pos = [3, 3]
enabled = false
sprite = { color = "#ff000080", size = [30, 30] }
"##;

/// The perspective camera's range, near: a low hall under a camera that
/// zooms in until part of the hall is behind it.
const HALL: &str = r##"
[ground]
size = [40, 40]

[[entity]]
name = "hall"
pos = [10, 10]
footprint = [20, 20]
block = { height = 0.25, color = "#8090a0", top = "#c0b090" }

[[entity]]
name = "here"
pos = [19, 19, 0.25]
sprite = { color = "#ff0000", size = [8, 8] }

[[entity]]
name = "post"
pos = [17, 17, 0.25]
block = { height = 1.5, color = "#a04040" }
"##;

/// The perspective camera's range, far: a wide floor deep under the scene,
/// seen past the ground's edge and cut where it is too far away.
const DEEP: &str = r##"
[ground]
size = [4, 4]

[[entity]]
name = "deep"
pos = [-60, -60, -12]
footprint = [120, 120]
block = { height = 0, color = "#6040a0" }
"##;

/// The iso camera's range: one box so near the camera and one so far from
/// it that the range cuts through them.
const FLOATERS: &str = r##"
[ground]
size = [4, 4]

[[entity]]
name = "near"
pos = [1363, 1369, 1340]
footprint = [2, 2]
block = { height = 64, color = "#c06030", top = "#f0d080" }

[[entity]]
name = "far"
pos = [-1361, -1367, -1404]
footprint = [2, 2]
block = { height = 64, color = "#3060c0", top = "#80d0f0" }
"##;

fn project_files(scene: &str, width: u32, height: u32) -> Vfs {
    let mut vfs = Vfs::new();
    vfs.insert("rwgpu.toml", format!("[project]\nname = \"check\"\n[display]\nwidth = {width}\nheight = {height}\n"));
    vfs.insert("scenes/main.toml", scene);
    vfs
}

fn camera_name(c: &Cam) -> String {
    let view = if c.0 == ISO { "iso" } else { "perspective" };
    let pan = if c.3 == [0.0, 0.0] { String::new() } else { format!(" pan {:?}", c.3) };
    format!("{view} turn {} zoom {}{pan}", c.1, c.2)
}

fn rule(out: &mut Vec<Scene>, name: &str, scene: &str, cameras: &[Cam], disabled: bool) -> Result<(), String> {
    let vfs = project_files(scene, 640, 480);
    for cam in cameras {
        let mut s = session(&vfs, None).map_err(|e| format!("{name}: {e}"))?;
        s.move_camera(&CameraMove { view: Some(cam.0), turn: cam.1, zoom: cam.2, pan: cam.3 });
        out.push(Scene { name: format!("{name}: {}", camera_name(cam)), list: s.render_list(disabled) });
    }
    Ok(())
}

/// Every frame a playtest of this example shoots.
fn example(out: &mut Vec<Scene>, root: &Path, name: &str) -> Result<(), String> {
    let dir = root.join("examples").join(name);
    let vfs = Vfs::from_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tests: Vec<String> = vfs.paths_under("tests", "toml").map(str::to_string).collect();
    for test in tests {
        let mut s = session(&vfs, Some(&test)).map_err(|e| format!("{name}/{test}: {e}"))?;
        let Some(def) = s.playtest.clone() else { continue };
        let mut frames: Vec<u64> = def.shots.iter().map(|shot| shot.frame.unwrap_or(def.frames)).collect();
        frames.sort_unstable();
        frames.dedup();
        for frame in frames {
            s.run_to(frame);
            out.push(Scene { name: format!("{name}/{test} frame {frame}"), list: s.render_list(false) });
        }
    }
    Ok(())
}

pub fn all(root: &Path) -> Result<Vec<Scene>, String> {
    let mut out = Vec::new();
    for name in ["iso_garden", "coin_vault", "tiny_town"] {
        example(&mut out, root, name)?;
    }
    out.push(Scene { name: "town of 5,000 boxes".into(), list: town_list(5_000, false)? });
    rule(&mut out, "solids", SOLIDS, &CAMERAS, false)?;
    rule(&mut out, "cards", CARDS, &CAMERAS, false)?;
    rule(&mut out, "cards below their anchor", CARDS_BELOW, &CAMERAS, false)?;
    rule(&mut out, "cards on a floor", CARDS_ON_A_FLOOR, &CAMERAS, false)?;
    rule(&mut out, "see-through", SEE_THROUGH, &CAMERAS, false)?;
    rule(&mut out, "2D layer", OVERLAY, &[CAMERAS[0], CAMERAS[6]], false)?;
    rule(&mut out, "disabled", DISABLED, &[CAMERAS[0], CAMERAS[2], CAMERAS[5]], true)?;
    let near = [(PER, 0.0, 4.0, [0.0, 0.0]), (PER, 123.0, 8.0, [0.0, 0.0]), (PER, 0.0, 1.0, [0.0, 0.0])];
    rule(&mut out, "range, hall", HALL, &near, false)?;
    rule(&mut out, "range, deep floor", DEEP, &[(PER, 0.0, 16.0, [0.0, 0.0]), (PER, 40.0, 16.0, [0.0, 0.0])], false)?;
    rule(&mut out, "range, iso", FLOATERS, &[(ISO, 0.0, 1.0, [0.0, 0.0]), (ISO, 0.0, 0.5, [0.0, 0.0])], false)?;
    for seed in 1..=RANDOM_SCENES {
        out.push(random(seed)?);
    }
    Ok(out)
}

/// The case outside the gate (see the top of this file), under every
/// camera: two boxes in one cell, the red one listed second or first. Their
/// sides coincide up to the lower one's height. Only the red box is pure red.
pub fn gaps() -> Result<Vec<(&'static str, Vec<Scene>)>, String> {
    let grey = "\n[[entity]]\npos = [2, 2]\nblock = { height = 2, color = \"#808080\", top = \"#a0a0a0\" }\n";
    let red = "\n[[entity]]\npos = [2, 2]\nblock = { height = 1, color = \"#ff0000\", top = \"#ff0000\" }\n";
    let mut out = Vec::new();
    for (name, scene) in [
        ("two boxes in one cell, the lower one second", format!("[ground]\nsize = [5, 5]\n{grey}{red}")),
        ("two boxes in one cell, the lower one first", format!("[ground]\nsize = [5, 5]\n{red}{grey}")),
    ] {
        let mut scenes = Vec::new();
        rule(&mut scenes, "", &scene, &CAMERAS, false)?;
        for s in &mut scenes {
            s.name = s.name.trim_start_matches(": ").to_string();
        }
        out.push((name, scenes));
    }
    Ok(out)
}

/// A random scene under a random camera, the same for the same seed: a
/// ground, boxes, flat things of one to nine cells (tiles and blocks of
/// height 0, over each other, under boxes and on top of them), sprites and
/// text that stand on, above or below their anchor, some see-through, some
/// disabled, and a HUD. Boxes never share a cell (the top of this file).
/// The gate draws the first [`RANDOM_SCENES`];
/// `cargo xtask gpu --fast -- --random N` draws the first N alone.
pub fn random(seed: u64) -> Result<Scene, String> {
    use rwgpuengine_core::rng::Rng;
    let mut rng = Rng::new(seed);
    let size = rng.range_i32(4, 8);
    let mut scene = format!("[ground]\nsize = [{size}, {size}]\n");
    let color = |rng: &mut Rng, see_through: bool| {
        let c = rng.next_u32();
        let alpha = if see_through { format!("{:02x}", 0x30 + rng.below(0xa0)) } else { String::new() };
        format!("\"#{:06x}{alpha}\"", c & 0xff_ffff)
    };
    let shapes = ["rect", "circle", "diamond"];
    let heights = ["0", "0.25", "0.4", "0.5", "0.8", "1", "1.2", "1.5", "2", "2.75"];
    // A footprint of up to 3 x 3 cells that stays on the ground.
    let footprint = |rng: &mut Rng, x: i32, y: i32| {
        let (w, h) = (rng.range_i32(1, 3).min(size - x), rng.range_i32(1, 3).min(size - y));
        if w * h == 1 { String::new() } else { format!("footprint = [{w}, {h}]\n") }
    };
    let mut boxed = vec![false; (size * size) as usize];
    let free = |boxed: &[bool], x: i32, y: i32| x < size && y < size && !boxed[(y * size + x) as usize];
    for k in 0..rng.range_i32(6, 14) {
        let (x, y) = (rng.range_i32(0, size - 1), rng.range_i32(0, size - 1));
        let glass = rng.below(4) == 0;
        let enabled = if rng.below(8) == 0 { "enabled = false\n" } else { "" };
        let kind = rng.below(7);
        let height = heights[rng.below(heights.len() as u32) as usize];
        let flat = kind == 0 || height == "0";
        if (1..=2).contains(&kind) && !flat && !free(&boxed, x, y) {
            continue;
        }
        scene.push_str(&format!("\n[[entity]]\nname = \"e{k}\"\npos = [{x}, {y}]\n{enabled}"));
        match kind {
            0 => {
                let cells = footprint(&mut rng, x, y);
                scene.push_str(&format!("{cells}tile = {{ color = {} }}\n", color(&mut rng, glass)));
            }
            1 | 2 if flat => {
                let cells = footprint(&mut rng, x, y);
                scene.push_str(&format!("{cells}block = {{ height = 0, color = {} }}\n", color(&mut rng, glass)));
            }
            1 | 2 => {
                let wide =
                    rng.below(4) == 0 && free(&boxed, x + 1, y) && free(&boxed, x, y + 1) && free(&boxed, x + 1, y + 1);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().take(if wide { 4 } else { 1 }) {
                    boxed[((y + dy) * size + x + dx) as usize] = true;
                }
                let two = if wide { "footprint = [2, 2]\n" } else { "" };
                let glass_top = glass && rng.below(2) == 0;
                scene.push_str(&format!(
                    "{two}block = {{ height = {height}, color = {}, top = {} }}\n",
                    color(&mut rng, glass),
                    color(&mut rng, glass_top)
                ));
                match rng.below(4) {
                    0 => {
                        scene.push_str(&format!("label = {{ text = \"B{k}\", color = {} }}\n", color(&mut rng, false)))
                    }
                    // Something flat on one cell of its top.
                    1 => scene.push_str(&format!(
                        "\n[[entity]]\nname = \"t{k}\"\npos = [{x}, {y}, {height}]\ntile = {{ color = {} }}\n",
                        color(&mut rng, false)
                    )),
                    _ => {}
                }
            }
            3..=5 => {
                let (w, h) = (rng.range_i32(8, 44), rng.range_i32(8, 60));
                scene.push_str(&format!(
                    "sprite = {{ shape = \"{}\", color = {}, size = [{w}, {h}], offset = [{}, {}] }}\n",
                    shapes[rng.below(3) as usize],
                    color(&mut rng, glass),
                    rng.range_i32(-6, 6),
                    rng.range_i32(-8, 12)
                ));
                if rng.below(3) == 0 {
                    scene
                        .push_str(&format!("label = {{ text = \"s{k}\", offset = [0, {}] }}\n", rng.range_i32(-20, 8)));
                }
            }
            _ => scene.push_str(&format!(
                "label = {{ text = \"L{k} q\", scale = {}, color = {}, offset = [0, {}] }}\n",
                rng.range_i32(1, 3),
                color(&mut rng, glass),
                rng.range_i32(-4, 6)
            )),
        }
    }
    for k in 0..rng.range_i32(0, 3) {
        let glass = rng.below(2) == 0;
        scene.push_str(&format!(
            "\n[[entity]]\nname = \"hud{k}\"\nscreen_pos = [{}, {}]\nsprite = {{ shape = \"{}\", color = {}, size = [{}, {}] }}\nlabel = {{ text = \"H{k}\", scale = 2, offset = [6, 6] }}\n",
            rng.range_i32(-20, 560),
            rng.range_i32(-10, 420),
            shapes[rng.below(3) as usize],
            color(&mut rng, glass),
            rng.range_i32(30, 240),
            rng.range_i32(20, 120)
        ));
    }
    let view = if rng.below(2) == 0 { ISO } else { PER };
    let cam: Cam = (
        view,
        rng.below(3600) as f32 / 10.0,
        [0.5, 0.75, 1.0, 1.25, 1.6, 2.0, 3.0][rng.below(7) as usize],
        [rng.range_i32(-8, 8) as f32 / 4.0, rng.range_i32(-8, 8) as f32 / 4.0],
    );
    let mut out = Vec::new();
    rule(&mut out, &format!("random {seed}"), &scene, &[cam], rng.below(4) == 0)?;
    Ok(out.remove(0))
}
