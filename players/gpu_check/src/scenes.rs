//! The reference scenes: the render lists both renderers draw.
//!
//! - The example games at every frame their playtests shoot (River Town's
//!   catalog: its ten buildings of shapes at every quarter turn, in both views).
//! - The generated town of 5,000 boxes at 1280 × 720 in perspective.
//! - Small scenes, one per rule of the reference renderer, each under iso
//!   and perspective cameras.
//! - 200 random scenes, each under a random camera, the same on every run:
//!   they look for what the hand-made scenes miss.
//! - 100 random scenes of shapes (`[[shape]]` lists), made the same way.
//!
//! Two things are kept out of the random scenes on purpose, because there
//! two things have the same depth, no rule says which shows, and the
//! reference's own picture is decided by rounding (ADR 0021, "what stays
//! undefined").
//!
//! 1. **Two faces that are not level.** The reference takes the depth of
//!    such a face from its corners, which are snapped to 1/256 of a pixel,
//!    so that snapping decides. It happens in two ways:
//!    - two boxes in one cell: their sides share a plane (kept out by never
//!      putting two boxes in one cell);
//!    - two boxes side by side, in the iso view: their sides meet in an
//!      upright edge, and when the reference puts that edge exactly on a
//!      column of pixel centers, the whole column is such a tie (kept out by
//!      turning the camera a tenth of a degree further, see [`upright_tie`]).
//!
//!    Where a slanted edge passes through a single pixel center the same tie
//!    is one pixel on an edge, and is inside the limit.
//! 2. **Two cards of different entities on one upright line, at different
//!    heights**: a label on a box and the label of something standing in the
//!    box's cell. Both are upright in one plane, one above the other (kept
//!    out by giving a box of one cell a label only where nothing with a
//!    sprite or a label stands, and the other way round). At one height the
//!    two are the same arithmetic, and the first in the file shows.

use std::path::Path;

use rwgpuengine_data::{CameraMove, CameraView, Vfs};
use rwgpuengine_render::camera::to_fixed;
use rwgpuengine_render::{Mesh, Projector, RenderList, bake};
use rwgpuengine_runtime::{DataOnly, Session, SessionConfig};

pub struct Scene {
    pub name: String,
    pub list: RenderList,
    /// The scene file and the camera, for a scene made here: written next to
    /// the pictures of a failing scene, so it can be looked at with `rwgpu`.
    pub source: Option<String>,
}

fn session(vfs: &Vfs, script: Option<&str>) -> Result<Session, String> {
    let cfg = SessionConfig { script: script.map(str::to_string), ..SessionConfig::default() };
    Session::load(vfs, &cfg, Box::new(DataOnly))
        .map_err(|d| d.iter().map(|d| d.render_human()).collect::<Vec<_>>().join("\n"))
}

/// The generated town (`xtask/src/town.rs`), as its scene's camera sees it
/// (in perspective), or through the iso view at the same zoom.
pub fn town_list(boxes: usize, iso: bool) -> Result<RenderList, String> {
    town_seen(boxes, iso.then_some((ISO, 0.0, 1.0, [0.0, 0.0])))
}

/// The generated town after one move of its camera.
fn town_seen(boxes: usize, cam: Option<Cam>) -> Result<RenderList, String> {
    let mut s = session(&crate::town::town(boxes), None)?;
    if let Some(cam) = cam {
        s.move_camera(&CameraMove { view: Some(cam.0), turn: cam.1, zoom: cam.2, pan: cam.3 });
    }
    Ok(s.render_list(false))
}

/// How many random scenes the gate draws after the fixed ones.
pub const RANDOM_SCENES: u64 = 200;
/// And how many random scenes of shapes after those.
pub const RANDOM_SHAPES: u64 = 100;

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

/// Shapes: every kind of `[[shape]]` at every turn of its own, alone and on
/// each other (a roof on walls with a chimney standing in it, a cone on a
/// cylinder, wedges as a lean-to and as ramps), flat boxes in rows and side
/// by side like a road, a tile on a cylinder's top, a sprite and a label
/// standing on shapes, shapes wall to wall with each other and with a
/// block, one in the air, one disabled; and whole entities turned a quarter,
/// a half and three quarters (a footprint of 3 by 2 standing 2 by 3).
const SHAPES: &str = r##"
[ground]
size = [9, 8]

[[entity]]
name = "house"
pos = [0, 0]
footprint = [2, 2]
shape = [{ kind = "box", at = [0.125, 0.125, 0], size = [1.75, 1.75, 1.25], color = "#e8d8b0" }, { kind = "roof", at = [0, 0, 1.25], size = [2, 2, 0.875], color = "#b5523b" }, { kind = "box", at = [1.25, 0.375, 1.25], size = [0.25, 0.25, 1], color = "#7a6a5a" }]

[[entity]]
name = "tree"
pos = [3, 0]
shape = [{ kind = "cylinder", at = [0.375, 0.375, 0], size = [0.25, 0.25, 0.5], color = "#7a5a3a" }, { kind = "cone", at = [0.125, 0.125, 0.5], size = [0.75, 0.75, 1.25], color = "#3f8a3f" }]

[[entity]]
name = "mill"
pos = [5, 0]
footprint = [2, 2]
shape = [{ kind = "box", at = [0, 0, 0], size = [1, 2, 1], color = "#c9b38a" }, { kind = "roof", at = [0, 0, 1], size = [1, 2, 0.5], color = "#8b5a3c", turn = 90 }, { kind = "cylinder", at = [1, 0.5, 0], size = [1, 1, 2], color = "#c8c8c8" }, { kind = "cone", at = [0.875, 0.375, 2], size = [1.25, 1.25, 0.75], color = "#5a6ea0" }]
allow = ["W0208"]

[[entity]]
name = "well"
pos = [8, 0]
shape = [{ kind = "cylinder", at = [0.125, 0.125, 0], size = [0.75, 0.75, 0.375], color = "#9a9a9a" }]

[[entity]]
name = "water"
pos = [8, 0, 0.375]
tile = { color = "#3a7ac0" }

[[entity]]
name = "ramps"
pos = [0, 3]
footprint = [2, 2]
shape = [{ kind = "wedge", size = [1, 1, 0.5], color = "#c0a060" }, { kind = "wedge", at = [1, 0, 0], size = [1, 1, 0.75], color = "#b89858", turn = 90 }, { kind = "wedge", at = [0, 1, 0], size = [1, 1, 1], color = "#a08040", turn = 180 }, { kind = "wedge", at = [1, 1, 0], size = [1, 1, 0.25], color = "#98783c", turn = 270 }]

[[entity]]
name = "workshop"
pos = [3, 2]
footprint = [3, 2]
turn = 90
shape = [{ kind = "box", size = [3, 2, 1], color = "#b07a4a" }, { kind = "wedge", at = [0, 0, 1], size = [3, 2, 0.75], color = "#70564a", turn = 180 }, { kind = "cylinder", at = [2.25, 0.25, 0.5], size = [0.375, 0.375, 1.75], color = "#5a5a5a" }]

[[entity]]
name = "roofs"
pos = [6, 3]
footprint = [3, 1]
shape = [{ kind = "roof", size = [1, 1, 0.5], color = "#b5523b" }, { kind = "roof", at = [1, 0, 0], size = [1, 1, 1], color = "#b5523b", turn = 90 }, { kind = "roof", at = [2, 0.25, 0], size = [1, 0.5, 0.25], color = "#8b2f2f", turn = 180 }]

[[entity]]
name = "farm"
pos = [6, 5]
footprint = [3, 3]
shape = [{ kind = "box", at = [0, 0, 0], size = [1.5, 1, 0.75], color = "#a03c2c" }, { kind = "roof", at = [0, 0, 0.75], size = [1.5, 1, 0.5], color = "#5a3a2a" }, { kind = "box", at = [0.125, 1.25, 0], size = [2.75, 0.25, 0.125], color = "#7a9a3a" }, { kind = "box", at = [0.125, 1.75, 0], size = [2.75, 0.25, 0.125], color = "#c8b040" }, { kind = "box", at = [0.125, 2.25, 0], size = [2.75, 0.25, 0.125], color = "#7a9a3a" }, { kind = "box", at = [0.125, 2.625, 0], size = [2.75, 0.25, 0.0625], color = "#6a4a2a" }]

[[entity]]
cells = [[0, 6], [1, 6], [2, 6], [2, 7]]
shape = [{ kind = "box", size = [1, 1, 0.0625], color = "#8a8478" }]

[[entity]]
name = "bridge"
pos = [3, 6]
footprint = [2, 1]
turn = 180
shape = [{ kind = "wedge", size = [0.5, 1, 0.25], color = "#9a7a52" }, { kind = "box", at = [0.5, 0, 0], size = [1, 1, 0.25], color = "#9a7a52" }, { kind = "wedge", at = [1.5, 0, 0], size = [0.5, 1, 0.25], color = "#9a7a52", turn = 180 }, { kind = "box", at = [0.5, 0, 0.25], size = [1, 0.125, 0.25], color = "#6a4a2a" }, { kind = "box", at = [0.5, 0.875, 0.25], size = [1, 0.125, 0.25], color = "#6a4a2a" }]

[[entity]]
name = "crate"
pos = [5, 6]
block = { height = 0.25, color = "#a05a2c" }

[[entity]]
name = "hut"
pos = [4, 7]
turn = 270
shape = [{ kind = "box", size = [1, 1, 0.75], color = "#a05a2c" }, { kind = "cone", at = [0, 0, 0.75], size = [1, 1, 0.5], color = "#6a3a2a" }]
sprite = { shape = "circle", color = "#ffe060", size = [12, 12] }
label = { text = "Hut", offset = [0, -14] }

[[entity]]
name = "lamp"
pos = [0, 7, 0.5]
shape = [{ kind = "box", at = [0.25, 0.25, 0], size = [0.5, 0.5, 0.5], color = "#f0e080" }, { kind = "cone", at = [0.25, 0.25, 0.5], size = [0.5, 0.5, 0.25], color = "#404040" }]

[[entity]]
name = "gone"
pos = [1, 7]
enabled = false
shape = [{ kind = "cylinder", size = [1, 1, 3], color = "#ff00ff" }]
"##;

/// Found by random scenes, each with the camera it was found under. Small
/// copies of what went wrong, so that it stays right.
///
/// - An upright edge of a box that the reference puts on a column of pixel
///   centers, one sub-pixel step from where single precision puts it: the
///   whole edge was drawn one pixel off (random scene 2998).
/// - A sprite standing inside a see-through box, where it passes through the
///   box's top less than a hundredth of a pixel from a row's center: that
///   row went the wrong way (random scene 2757).
/// - A sprite whose anchor is a hair above a pixel center: the reference
///   left a gap of one row across it (random scene 9804).
const FOUND: [(&str, &str, Cam); 3] = [
    (
        "an upright edge on pixel centers",
        "[ground]\nsize = [7, 7]\n\n[[entity]]\npos = [5, 6]\nblock = { height = 2, color = \"#686aaa\", top = \"#cd0579\" }\n",
        (ISO, 155.9, 1.0, [1.25, 2.0]),
    ),
    (
        "a card through the top of a see-through box",
        "[ground]\nsize = [4, 4]\n\n[[entity]]\npos = [1, 0]\nsprite = { shape = \"diamond\", color = \"#535e94\", size = [43, 50], offset = [-2, 3] }\n\n[[entity]]\npos = [1, 0]\nblock = { height = 0.4, color = \"#9566fbbd\", top = \"#252b8e97\" }\n",
        (ISO, 93.6, 3.0, [1.25, -1.0]),
    ),
    (
        "an anchor a hair above a pixel center",
        "[ground]\nsize = [5, 5]\n\n[[entity]]\npos = [3, 0]\nsprite = { shape = \"rect\", color = \"#ef1912\", size = [14, 12], offset = [3, 7] }\n",
        (PER, 314.4, 2.0, [-0.5, 1.25]),
    ),
];

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
        let source = format!(
            "# {name}: {}; a 640 x 480 frame{}\n{scene}",
            camera_name(cam),
            if disabled { "; the list is built with disabled things in it" } else { "" }
        );
        let name = format!("{name}: {}", camera_name(cam));
        out.push(Scene { name, list: s.render_list(disabled), source: Some(source) });
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
            out.push(Scene { name: format!("{name}/{test} frame {frame}"), list: s.render_list(false), source: None });
        }
    }
    Ok(())
}

pub fn all(root: &Path) -> Result<Vec<Scene>, String> {
    let mut out = Vec::new();
    for name in ["iso_garden", "coin_vault", "tiny_town", "river_town"] {
        example(&mut out, root, name)?;
    }
    out.push(Scene { name: "town of 5,000 boxes".into(), list: town_list(5_000, false)?, source: None });
    // The same town through the iso view, straight and turned, far and near: its boxes stand wall to wall.
    for cam in [
        (ISO, 0.0, 1.0, [0.0, 0.0]),
        (ISO, 33.0, 1.0, [0.0, 0.0]),
        (ISO, 217.0, 4.0, [3.0, -2.0]),
        (PER, 123.0, 3.0, [0.0, 0.0]),
    ] {
        let name = format!("town of 5,000 boxes: {}", camera_name(&cam));
        out.push(Scene { name, list: town_seen(5_000, Some(cam))?, source: None });
    }
    rule(&mut out, "solids", SOLIDS, &CAMERAS, false)?;
    rule(&mut out, "cards", CARDS, &CAMERAS, false)?;
    rule(&mut out, "cards below their anchor", CARDS_BELOW, &CAMERAS, false)?;
    rule(&mut out, "cards on a floor", CARDS_ON_A_FLOOR, &CAMERAS, false)?;
    rule(&mut out, "see-through", SEE_THROUGH, &CAMERAS, false)?;
    rule(&mut out, "shapes", SHAPES, &CAMERAS, false)?;
    rule(&mut out, "2D layer", OVERLAY, &[CAMERAS[0], CAMERAS[6]], false)?;
    rule(&mut out, "disabled", DISABLED, &[CAMERAS[0], CAMERAS[2], CAMERAS[5]], true)?;
    let near = [(PER, 0.0, 4.0, [0.0, 0.0]), (PER, 123.0, 8.0, [0.0, 0.0]), (PER, 0.0, 1.0, [0.0, 0.0])];
    rule(&mut out, "range, hall", HALL, &near, false)?;
    rule(&mut out, "range, deep floor", DEEP, &[(PER, 0.0, 16.0, [0.0, 0.0]), (PER, 40.0, 16.0, [0.0, 0.0])], false)?;
    rule(&mut out, "range, iso", FLOATERS, &[(ISO, 0.0, 1.0, [0.0, 0.0]), (ISO, 0.0, 0.5, [0.0, 0.0])], false)?;
    for (name, scene, cam) in FOUND {
        rule(&mut out, &format!("found, {name}"), scene, &[cam], false)?;
    }
    for seed in 1..=RANDOM_SCENES {
        out.push(random(seed)?);
    }
    for seed in 1..=RANDOM_SHAPES {
        out.push(random_shapes(seed)?);
    }
    Ok(out)
}

/// The case outside the gate (the top of this file), in its two forms.
/// Only the red box is pure red.
///
/// - Two boxes in one cell, the lower red one listed second or first, under
///   every camera: their sides share a plane up to the red one's height.
/// - Two boxes side by side, under an iso camera that puts their shared
///   upright edge on a column of pixel centers (it is random scene 596).
pub fn gaps() -> Result<Vec<(&'static str, Vec<Scene>)>, String> {
    let grey = |cell: &str| {
        format!("\n[[entity]]\npos = {cell}\nblock = {{ height = 2, color = \"#808080\", top = \"#a0a0a0\" }}\n")
    };
    let red = |cell: &str| {
        format!("\n[[entity]]\npos = {cell}\nblock = {{ height = 0.8, color = \"#ff0000\", top = \"#ff0000\" }}\n")
    };
    let mut out = Vec::new();
    for (name, scene) in [
        (
            "two boxes in one cell, the lower one second",
            format!("[ground]\nsize = [5, 5]\n{}{}", grey("[2, 2]"), red("[2, 2]")),
        ),
        (
            "two boxes in one cell, the lower one first",
            format!("[ground]\nsize = [5, 5]\n{}{}", red("[2, 2]"), grey("[2, 2]")),
        ),
    ] {
        let mut scenes = Vec::new();
        rule(&mut scenes, "", &scene, &CAMERAS, false)?;
        out.push((name, scenes));
    }
    let side_by_side = format!("[ground]\nsize = [8, 8]\n{}{}", grey("[2, 3]"), red("[2, 4]"));
    let mut scenes = Vec::new();
    rule(&mut scenes, "", &side_by_side, &[(ISO, 100.4, 2.0, [1.5, 1.25]), (ISO, 100.5, 2.0, [1.5, 1.25])], false)?;
    out.push(("two boxes side by side, an upright edge on pixel centers, then turned a tenth further", scenes));
    for (_, scenes) in &mut out {
        for s in scenes {
            s.name = s.name.trim_start_matches(": ").to_string();
        }
    }
    Ok(out)
}

/// A random scene under a random camera, the same for the same seed: a
/// ground, boxes, flat things of one to nine cells (tiles and blocks of
/// height 0, over each other, under boxes and on top of them), sprites and
/// text that stand on, above or below their anchor, some see-through, some
/// disabled, and a HUD. Boxes never share a cell, and the camera is never
/// where two of them tie along an upright edge (the top of this file).
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
    // Cells where a sprite or a label stands on the ground, and cells whose
    // box carries a label on its top: never the same cell (2, at the top of this file).
    let mut stands = vec![false; (size * size) as usize];
    let mut labelled = vec![false; (size * size) as usize];
    for k in 0..rng.range_i32(6, 14) {
        let (x, y) = (rng.range_i32(0, size - 1), rng.range_i32(0, size - 1));
        let glass = rng.below(4) == 0;
        let enabled = if rng.below(8) == 0 { "enabled = false\n" } else { "" };
        let kind = rng.below(7);
        let height = heights[rng.below(heights.len() as u32) as usize];
        let flat = kind == 0 || height == "0";
        let here = (y * size + x) as usize;
        if (1..=2).contains(&kind) && !flat && !free(&boxed, x, y) {
            continue;
        }
        if kind >= 3 && labelled[here] {
            continue;
        }
        stands[here] |= kind >= 3;
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
                    0 if wide || !stands[here] => {
                        labelled[here] = !wide;
                        scene.push_str(&format!("label = {{ text = \"B{k}\", color = {} }}\n", color(&mut rng, false)))
                    }
                    0 => {}
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
    let mut cam: Cam = (
        view,
        rng.below(3600) as f32 / 10.0,
        [0.5, 0.75, 1.0, 1.25, 1.6, 2.0, 3.0][rng.below(7) as usize],
        [rng.range_i32(-8, 8) as f32 / 4.0, rng.range_i32(-8, 8) as f32 / 4.0],
    );
    let disabled = rng.below(4) == 0;
    loop {
        let mut out = Vec::new();
        rule(&mut out, &format!("random {seed}"), &scene, &[cam], disabled)?;
        let scene = out.remove(0);
        if !upright_tie(&scene.list) {
            return Ok(scene);
        }
        // The one undefined thing (the top of this file): look from a tenth of a degree further round.
        cam.1 = (cam.1 * 10.0 + 1.0).round() % 3600.0 / 10.0;
    }
}

/// In the iso view: do two solids share an upright edge that the reference
/// puts exactly on a column of pixel centers? Along such a column two sides
/// of different solids have the same depth, and the reference's picture is
/// decided by how their corners were snapped (the top of this file). A solid
/// is a box, or one shape of a model: a shape has an upright edge wherever
/// two of its corners stand one above the other.
pub fn upright_tie(list: &RenderList) -> bool {
    let proj = Projector::new(&list.view, &list.camera);
    let w = proj.rows()[3];
    if w[0] != 0.0 || w[1] != 0.0 || w[2] != 0.0 {
        // In perspective an upright edge is not one column of the picture.
        return false;
    }
    // Each upright edge of each solid: where it is in the world, and the sub-pixel step it is drawn at.
    let mut edges = Vec::new();
    let mut edge = |x: f64, y: f64, z: f64| {
        let h = proj.hom([x, y, z], 0);
        edges.push((x.to_bits(), y.to_bits(), to_fixed(h[0] / h[3])));
    };
    let models: Vec<_> = list.models.iter().map(|m| bake(m, &list.light)).collect();
    for inst in list.instances.iter().filter(|i| i.enabled) {
        let (p, s) = (inst.pos.map(f64::from), inst.size.map(f64::from));
        match inst.mesh {
            Mesh::Box => {
                for corner in 0..4 {
                    let x = p[0] + if corner & 1 != 0 { s[0] } else { 0.0 };
                    let y = p[1] + if corner & 2 != 0 { s[1] } else { 0.0 };
                    edge(x, y, p[2]);
                }
            }
            Mesh::Model(n) => {
                let model = &models[n as usize];
                for solid in &model.solids {
                    let points = &model.points[solid.points.start as usize..solid.points.end as usize];
                    let mut columns: Vec<[u32; 2]> = points.iter().map(|q| [q[0].to_bits(), q[1].to_bits()]).collect();
                    columns.sort_unstable();
                    // The columns two or more of its corners are on, each once.
                    let mut k = 0;
                    while k < columns.len() {
                        let same = columns[k..].iter().take_while(|c| **c == columns[k]).count();
                        if same > 1 {
                            let [x, y] = columns[k].map(|bits| f32::from_bits(bits) as f64);
                            edge(p[0] + x, p[1] + y, p[2]);
                        }
                        k += same;
                    }
                }
            }
            Mesh::Flat => {}
        }
    }
    edges.sort_unstable();
    // A pixel center is half a pixel, 128 steps, past a whole pixel.
    edges.windows(2).any(|c| (c[0].0, c[0].1) == (c[1].0, c[1].1) && c[0].2.rem_euclid(256) == 128)
}

/// A random scene of shapes under a random camera, the same for the same
/// seed: a ground, and on cells of their own (a footprint of one to nine)
/// things built of `[[shape]]` lists: one shape somewhere in the footprint;
/// walls with a roof, a wedge, a cone, a cylinder or a box on them, some with
/// a chimney standing in the roof; rows of flat boxes; a cylinder with a cone
/// on it; one shape in each cell. Of every kind, at every turn, sizes in
/// eighths of a cell, some in the air, some disabled, some with a tile on a
/// level top or with a sprite or a label standing on them, and some blocks
/// between. Half of the entities are turned a quarter, a half or three
/// quarters, footprint and all. No two shapes of a list have upright sides in one plane that
/// overlap (W0209), and the camera is never where two solids tie along an
/// upright edge (the top of this file).
pub fn random_shapes(seed: u64) -> Result<Scene, String> {
    use rwgpuengine_core::rng::Rng;
    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5348_4150_4553);
    let size = rng.range_i32(5, 9);
    let mut scene = format!("[ground]\nsize = [{size}, {size}]\n");
    let color = |rng: &mut Rng| format!("\"#{:06x}\"", rng.next_u32() & 0xff_ffff);
    // Eighths of a cell, as a file writes them.
    let e = |n: i32| format!("{}", n as f32 / 8.0);
    let kinds = ["box", "roof", "wedge", "cylinder", "cone"];
    let shape = |rng: &mut Rng, kind: &str, at: [i32; 3], sz: [i32; 3]| {
        format!(
            "{{ kind = \"{kind}\", at = [{}, {}, {}], size = [{}, {}, {}], color = {}, turn = {} }}",
            e(at[0]),
            e(at[1]),
            e(at[2]),
            e(sz[0]),
            e(sz[1]),
            e(sz[2]),
            color(rng),
            rng.below(4) * 90
        )
    };
    let mut taken = vec![false; (size * size) as usize];
    for k in 0..rng.range_i32(5, 12) {
        let (x, y) = (rng.range_i32(0, size - 1), rng.range_i32(0, size - 1));
        let (w, h) = (rng.range_i32(1, 3).min(size - x), rng.range_i32(1, 3).min(size - y));
        let cells = |f: &mut dyn FnMut(usize)| {
            for cy in y..y + h {
                for cx in x..x + w {
                    f((cy * size + cx) as usize);
                }
            }
        };
        let mut free = true;
        cells(&mut |c| free &= !taken[c]);
        if !free {
            continue;
        }
        cells(&mut |c| taken[c] = true);
        let z = [0, 0, 0, 0, 2, 4][rng.below(6) as usize];
        let enabled = if rng.below(8) == 0 { "enabled = false\n" } else { "" };
        // Half of them are turned. `w` by `h` are the cells as covered: the
        // file has the footprint, and the shapes, as they are before the turn.
        let turn = if rng.below(2) == 0 { 0 } else { rng.below(4) };
        let (w, h) = if turn % 2 == 0 { (w, h) } else { (h, w) };
        let turned = if turn == 0 { String::new() } else { format!("turn = {}\n", turn * 90) };
        let footprint = if w * h == 1 { String::new() } else { format!("footprint = [{w}, {h}]\n") };
        scene.push_str(&format!(
            "\n[[entity]]\nname = \"e{k}\"\npos = [{x}, {y}, {}]\n{enabled}{footprint}{turned}",
            e(z)
        ));
        let (w8, h8) = (8 * w, 8 * h);
        let mut list: Vec<String> = Vec::new();
        // The height of a level top a tile can lie on, if the thing has one.
        let mut level_top = None;
        let mut height = 0;
        match rng.below(8) {
            0 => {
                let high = rng.range_i32(2, 16);
                scene.push_str(&format!("block = {{ height = {}, color = {} }}\n", e(high), color(&mut rng)));
                level_top = Some(high);
                height = high;
            }
            1 | 2 => {
                // One shape, somewhere in the footprint.
                let kind = kinds[rng.below(5) as usize];
                let (sx, sy) = (rng.range_i32(2, w8), rng.range_i32(2, h8));
                let at = [rng.range_i32(0, w8 - sx), rng.range_i32(0, h8 - sy), 0];
                height = rng.range_i32(1, 20);
                list.push(shape(&mut rng, kind, at, [sx, sy, height]));
                level_top = matches!(kind, "box" | "cylinder").then_some(height);
            }
            3 | 4 => {
                // Walls with something on them, each set in from the footprint or not.
                let (i, j) = (rng.range_i32(0, 1), rng.range_i32(0, 2));
                let walls = rng.range_i32(2, 16);
                list.push(shape(&mut rng, "box", [i, i, 0], [w8 - 2 * i, h8 - 2 * i, walls]));
                let kind = kinds[rng.below(5) as usize];
                let top = rng.range_i32(1, 12);
                list.push(shape(&mut rng, kind, [j, j, walls], [w8 - 2 * j, h8 - 2 * j, top]));
                height = walls + top;
                level_top = matches!(kind, "box" | "cylinder").then_some(height);
                // A chimney standing in a roof, a step in from every wall.
                let room = i.max(j) + 1;
                if kind == "roof" && rng.below(2) == 0 {
                    let at = [rng.range_i32(room, w8 - room - 2), rng.range_i32(room, h8 - room - 2), walls];
                    height += 3;
                    list.push(shape(&mut rng, "box", at, [2, 2, top + 3]));
                }
            }
            5 => {
                // Rows of flat boxes, like a field.
                let thick = rng.range_i32(1, 2);
                let mut row = 1;
                while row + 2 < h8 {
                    list.push(shape(&mut rng, "box", [1, row, 0], [w8 - 2, 2, thick]));
                    row += 4;
                }
                height = thick;
            }
            6 => {
                // A round tower with a cone on it, as wide or wider.
                let i = rng.range_i32(0, 2);
                let across = w8.min(h8) - 2 * i;
                let tower = rng.range_i32(4, 20);
                list.push(shape(&mut rng, "cylinder", [i, i, 0], [across, across, tower]));
                let over = rng.range_i32(0, 1).min(i);
                let cap = rng.range_i32(2, 12);
                list.push(shape(
                    &mut rng,
                    "cone",
                    [i - over, i - over, tower],
                    [across + 2 * over, across + 2 * over, cap],
                ));
                height = tower + cap;
            }
            _ => {
                // A shape of its own in each cell of the footprint, or none.
                for cy in 0..h {
                    for cx in 0..w {
                        if rng.below(3) == 0 {
                            continue;
                        }
                        let (a, b) = (rng.range_i32(0, 4), rng.range_i32(0, 4));
                        let sz = [rng.range_i32(2, 8 - a), rng.range_i32(2, 8 - b), rng.range_i32(1, 16)];
                        let kind = kinds[rng.below(5) as usize];
                        list.push(shape(&mut rng, kind, [8 * cx + a, 8 * cy + b, 0], sz));
                        height = height.max(sz[2]);
                    }
                }
            }
        }
        if !list.is_empty() {
            scene.push_str(&format!("shape = [{}]\n", list.join(", ")));
        }
        match rng.below(6) {
            0 => scene.push_str(&format!("label = {{ text = \"S{k}\", color = {} }}\n", color(&mut rng))),
            1 => scene.push_str(&format!(
                "sprite = {{ shape = \"circle\", color = {}, size = [{}, {}] }}\n",
                color(&mut rng),
                rng.range_i32(8, 30),
                rng.range_i32(8, 40)
            )),
            _ => {}
        }
        // Something flat on its level top, as large as the footprint, turned with it.
        if let Some(top) = level_top.filter(|_| rng.below(3) == 0) {
            scene.push_str(&format!(
                "\n[[entity]]\nname = \"t{k}\"\npos = [{x}, {y}, {}]\n{footprint}{turned}tile = {{ color = {} }}\n",
                e(z + top),
                color(&mut rng)
            ));
        }
        let _ = height;
    }
    let view = if rng.below(2) == 0 { ISO } else { PER };
    let mut cam: Cam = (
        view,
        rng.below(3600) as f32 / 10.0,
        [0.5, 0.75, 1.0, 1.25, 1.6, 2.0, 3.0][rng.below(7) as usize],
        [rng.range_i32(-8, 8) as f32 / 4.0, rng.range_i32(-8, 8) as f32 / 4.0],
    );
    let disabled = rng.below(4) == 0;
    loop {
        let mut out = Vec::new();
        rule(&mut out, &format!("random shapes {seed}"), &scene, &[cam], disabled)?;
        let scene = out.remove(0);
        if !upright_tie(&scene.list) {
            return Ok(scene);
        }
        // The one undefined thing (the top of this file): look from a tenth of a degree further round.
        cam.1 = (cam.1 * 10.0 + 1.0).round() % 3600.0 / 10.0;
    }
}
