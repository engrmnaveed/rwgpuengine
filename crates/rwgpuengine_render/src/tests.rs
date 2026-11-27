use rwgpuengine_data::{CameraMove, CameraView, SceneLoader, Vfs, load_project};
use rwgpuengine_sim::World;

use crate::camera::{Camera, Projector};
use crate::list::build;
use crate::pick::{ground_tap_point, pick, tap_point};
use crate::reference::{Frame, Painter, Projected, project, render};
use crate::report::{FrameReport, build_report};
use crate::view::{View, round};

/// A world with the view and camera its scene starts with.
struct Stage {
    world: World,
    view: View,
    camera: Camera,
}

impl Stage {
    fn seen(&self, include_disabled: bool) -> Projected {
        project(&build(&self.world, &self.view, &self.camera, include_disabled))
    }

    fn render(&self) -> (Projected, Frame, FrameReport) {
        let seen = self.seen(false);
        let frame = render(&seen);
        let report = build_report(&self.world, &seen, &frame);
        (seen, frame, report)
    }

    fn image(&self) -> u64 {
        render(&self.seen(false)).image_hash()
    }

    /// Where a world point is drawn, exactly.
    fn at(&self, x: f32, y: f32, z: f32) -> [f32; 2] {
        Projector::new(&self.view, &self.camera).screen([x, y, z]).expect("in front of the camera")
    }

    /// The pixel a world point is drawn in.
    fn px(&self, x: f32, y: f32, z: f32) -> [i32; 2] {
        self.at(x, y, z).map(round)
    }

    fn with(mut self, view: CameraView, turn: f32, zoom: f32) -> Stage {
        self.camera.apply(&CameraMove { view: Some(view), turn, zoom, pan: [0.0, 0.0] });
        self
    }
}

fn setup(scene: &str) -> Stage {
    let mut vfs = Vfs::new();
    vfs.insert("rwgpu.toml", "[project]\nname = \"t\"\n[display]\nwidth = 320\nheight = 240\n");
    vfs.insert("scenes/main.toml", scene);
    let (project, d) = load_project(&vfs);
    let project = project.unwrap_or_else(|| panic!("{d:#?}"));
    let mut loader = SceneLoader::new(&vfs);
    let def = loader.load_scene("scenes/main.toml").unwrap_or_else(|| panic!("{:#?}", loader.diags));
    Stage { world: World::new(&def, 1), view: View::new(&project, &def), camera: Camera::new(&def) }
}

/// The cameras the 3D tests look through: the starting view, quarter and odd
/// turns, zooms, and perspective.
fn cameras() -> Vec<(CameraView, f32, f32)> {
    use CameraView::{Iso, Perspective};
    vec![
        (Iso, 0.0, 1.0),
        (Iso, 90.0, 1.0),
        (Iso, 217.0, 1.75),
        (Iso, 33.0, 0.5),
        (Perspective, 0.0, 1.0),
        (Perspective, 200.0, 0.8),
        (Perspective, 295.0, 3.0),
    ]
}

const SCENE: &str = r##"
[ground]
size = [4, 4]

[[entity]]
name = "hero"
pos = [1, 1]
sprite = { color = "#ff0000", size = [16, 24] }

[[entity]]
name = "wall"
pos = [3, 0]
block = { height = 2 }

[[entity]]
name = "ghost"
pos = [40, 40]
sprite = {}
"##;

#[test]
fn camera_centers_the_ground() {
    let stage = setup(SCENE);
    // Ground center (1.5, 1.5) lands in the middle of the 320x240 frame.
    assert_eq!(stage.px(1.5, 1.5, 0.0), [160, 120]);
    // Cell corners land on whole pixels: a cell is 64 x 32 of them.
    assert_eq!(stage.at(0.5, 0.5, 0.0), [160.0, 88.0]);
    assert_eq!(stage.at(1.5, 0.5, 0.0), [192.0, 104.0]);
    assert_eq!(stage.at(0.5, 0.5, 1.0), [160.0, 56.0]);
}

#[test]
fn renders_deterministically_and_reports_what_is_where() {
    let stage = setup(SCENE);
    let (_, frame, report) = stage.render();
    assert_eq!(frame.image_hash(), stage.image());

    // The hero's sprite is drawn in red, bottom edge on its cell center.
    let [cx, cy] = stage.px(1.0, 1.0, 0.0);
    assert_eq!(frame.pixel(cx as u32, (cy - 10) as u32).to_hex(), "#ff0000");

    assert_eq!(report.ground_cells, 16);
    let hero = report.entities.iter().find(|r| r.label == "hero").unwrap();
    assert_eq!(hero.parts, vec!["sprite"]);
    assert_eq!(hero.visible_px, hero.drawn_px);
    let ghost = report.entities.iter().find(|r| r.label == "ghost").unwrap();
    assert!(ghost.offscreen);
    let codes: Vec<&str> = report.issues.iter().map(|d| d.code).collect();
    assert_eq!(codes, vec!["W0301"]);
}

#[test]
fn sprites_behind_blocks_are_reported_occluded() {
    let scene = r##"
[ground]
size = [4, 4]

[[entity]]
name = "hero"
pos = [1, 1]
sprite = { size = [16, 24] }

[[entity]]
name = "tower"
pos = [2, 2]
block = { height = 3 }
"##;
    let (_, _, report) = setup(scene).render();
    let hero = report.entities.iter().find(|r| r.label == "hero").unwrap();
    assert_eq!(hero.covered_by.first().map(|c| c.0), Some(1));
    assert!(report.issues.iter().any(|d| d.code == "W0302"), "{:#?}", report.issues);
}

#[test]
fn picking_hits_what_is_drawn_and_names_disabled_entities() {
    let scene = r##"
[ground]
size = [4, 4]

[[entity]]
name = "button"
pos = [1, 1]
sprite = { size = [20, 20] }

[[entity]]
name = "lever"
pos = [2, 1]
sprite = { size = [20, 20] }
enabled = false
"##;
    for (view, turn, zoom) in cameras() {
        let stage = setup(scene).with(view, turn, zoom);
        let world = &stage.world;
        let list = stage.seen(true);
        let up = if view == CameraView::Iso { 10.0 * zoom } else { 4.0 };
        let [bx, by] = stage.at(1.0, 1.0, 0.0);
        assert_eq!(pick(&list, bx, by - up).hit, world.find("button"), "{view:?} {turn} {zoom}");
        let [lx, ly] = stage.at(2.0, 1.0, 0.0);
        let miss = pick(&list, lx, ly - up);
        assert_eq!(miss.hit, None);
        assert_eq!(miss.under_disabled, vec![world.find("lever").unwrap()], "{view:?} {turn} {zoom}");
        // The ground stops the search: a tap on bare floor hits nothing and names the cell.
        let [gx, gy] = stage.at(0.0, 3.0, 0.0);
        if (0.0..320.0).contains(&gx) && (0.0..240.0).contains(&gy) {
            let floor = pick(&list, gx, gy);
            assert_eq!((floor.hit, floor.ground), (None, Some([0, 3])), "{view:?} {turn} {zoom}");
        }
    }
    // A tap outside the frame hits nothing.
    let list = setup(scene).seen(true);
    assert_eq!(pick(&list, -3.0, 50.0), rwgpuengine_sim::Tap { x: -3.0, y: 50.0, ..Default::default() });
}

#[test]
fn hidden_trigger_tiles_are_reported() {
    // A goal tile right behind a tall tree (the cold-agent test found this).
    let scene = r##"
[ground]
size = [4, 4]

[[entity]]
name = "goal"
pos = [1, 1]
tile = {}
trigger = {}

[[entity]]
name = "tree"
pos = [2, 2]
block = { height = 1.6 }
"##;
    let (_, _, report) = setup(scene).render();
    let d = report.issues.iter().find(|d| d.code == "W0304").expect("W0304");
    assert!(d.message.contains("trigger tile of goal at cell (1, 1)"), "{}", d.message);
    assert!(d.message.contains("covered by tree"), "{}", d.message);
}

#[test]
fn tap_points_hit_their_entity_or_are_none_when_hidden() {
    let scene = r##"
[ground]
size = [4, 4]

[[entity]]
name = "lever"
pos = [0, 0]
tile = {}
sprite = { shape = "rect", size = [4, 24] }

[[entity]]
name = "buried"
pos = [1, 1]
tile = {}

[[entity]]
name = "tower"
pos = [2, 2]
block = { height = 2 }
"##;
    let stage = setup(scene);
    let (seen, frame, _) = stage.render();
    let lever = stage.world.find("lever").unwrap();
    let [x, y] = tap_point(&seen, &frame, lever).expect("lever is tappable");
    assert_eq!(pick(&stage.seen(true), x as f32 + 0.5, y as f32 + 0.5).hit, Some(lever));
    // A floor tile completely behind a tall block cannot be tapped.
    assert_eq!(tap_point(&seen, &frame, stage.world.find("buried").unwrap()), None);
    // Seen from the other side, it can.
    let stage = setup(scene).with(CameraView::Iso, 180.0, 1.0);
    let (seen, frame, _) = stage.render();
    let buried = stage.world.find("buried").unwrap();
    let [x, y] = tap_point(&seen, &frame, buried).expect("nothing covers it from here");
    assert_eq!(pick(&stage.seen(true), x as f32 + 0.5, y as f32 + 0.5).hit, Some(buried));
}

#[test]
fn allowed_warnings_are_not_reported() {
    let scene = |allow: &str| {
        format!(
            "[ground]\nsize = [4, 4]\n\n[[entity]]\nname = \"hero\"\npos = [1, 1]\nsprite = {{ size = [16, 24] }}\n{allow}\n\n[[entity]]\nname = \"tower\"\npos = [2, 2]\nblock = {{ height = 3 }}\n"
        )
    };
    let issues = |text: &str| setup(text).render().2.issues.iter().map(|d| d.code).collect::<Vec<_>>();
    assert!(issues(&scene("")).contains(&"W0302"));
    assert!(!issues(&scene("allow = [\"W0302\"]")).contains(&"W0302"));
}

// ---------------------------------------------------------------------------
// Labels and screen entities (ADR 0011)

fn render_scene(text: &str) -> (World, Projected, Frame, FrameReport) {
    let stage = setup(text);
    let (seen, frame, report) = stage.render();
    (stage.world, seen, frame, report)
}

const HUD: &str = r##"
[ground]
size = [4, 4]

[[entity]]
name = "tower"
pos = [3, 0]
block = { height = 2 }

[[entity]]
name = "hud"
screen_pos = [10, 6]
sprite = { shape = "rect", color = "#000000", size = [60, 14] }
label = { text = "Coins {n}", color = "#ffff00", offset = [3, 3] }
props = { n = 7 }

[[entity]]
name = "tag"
pos = [1, 1]
label = { text = "Hi", scale = 2 }
"##;

#[test]
fn screen_labels_draw_on_top_at_their_pixel() {
    let (world, list, frame, report) = render_scene(HUD);
    let hud = world.find("hud").unwrap();
    // Text box: "Coins 7" = 7 chars -> 41 x 8 px, top-left at (13, 9).
    let text_rect = list
        .items
        .iter()
        .filter(|i| i.entity == Some(hud) && i.part == crate::Part::Label)
        .fold(crate::Rect::EMPTY, |a, i| a.union(&i.bounds));
    assert_eq!(text_rect.xywh(), [13, 9, 41, 8], "glyph cells are 5 x 8, descender row included");
    // 'C' top-left pixel (col 1, row 0) is lit yellow over the black backdrop.
    assert_eq!(frame.pixel(14, 9).to_hex(), "#ffff00");
    assert_eq!(frame.pixel(13, 9).to_hex(), "#000000");
    // Screen items come after every world item, so nothing covers them.
    let first_hud = list.items.iter().position(|i| i.entity == Some(hud)).unwrap();
    assert!(list.items[first_hud..].iter().all(|i| i.entity == Some(hud)));
    let r = report.entities.iter().find(|r| r.id == hud).unwrap();
    assert_eq!(r.text.as_deref(), Some("Coins 7"));
    // Its label covers its own backdrop: that is not "hidden" (counted once).
    assert!(r.on_screen && r.visible_ratio() == 1.0, "{} of {}", r.visible_px, r.drawn_px);
    assert!(report.to_text().contains("hud            sprite+label  screen       rect [10, 6, 60, 14]  visible 100%"));
    assert!(report.issues.is_empty(), "{:#?}", report.issues);
    // Taps on the HUD hit it, even over the world.
    assert_eq!(pick(&list, 12.0, 8.0).hit, Some(hud));
}

#[test]
fn world_labels_stand_on_their_cell_and_scale() {
    let (world, list, _, _) = render_scene(HUD);
    let tag = world.find("tag").unwrap();
    let r = list.bounds_of(tag);
    // "Hi" at scale 2: (2 * 6 - 1) * 2 = 22 wide, 8 * 2 = 16 tall.
    assert_eq!((r.x1 - r.x0, r.y1 - r.y0), (22, 16));
    // Centered on the cell's standing point; the box bottom (row 8 * 2) is on it.
    let p = setup(HUD).px(1.0, 1.0, 0.0);
    assert_eq!(r.x0, p[0] - 11);
    assert_eq!(r.y0, p[1] - 16);
    // Text keeps its size when the camera zooms; it still stands on its cell.
    let zoomed = setup(HUD).with(CameraView::Iso, 0.0, 2.0);
    let r = zoomed.seen(false).bounds_of(tag);
    let p = zoomed.px(1.0, 1.0, 0.0);
    assert_eq!(r.xywh(), [p[0] - 11, p[1] - 16, 22, 16]);
}

#[test]
fn cut_off_or_covered_labels_are_reported() {
    // The backdrop fits (250..310); the text (290..331) does not: only W0305.
    let off =
        HUD.replace("screen_pos = [10, 6]", "screen_pos = [250, 6]").replace("offset = [3, 3]", "offset = [40, 3]");
    let (_, _, _, report) = render_scene(&off);
    let w: Vec<_> = report.issues.iter().filter(|d| d.code == "W0305").collect();
    assert_eq!(w.len(), 1, "{:#?}", w);
    assert!(w[0].message.contains("\"Coins 7\"") && w[0].message.contains("runs past the edge"), "{}", w[0].message);
    assert!(!report.issues.iter().any(|d| d.code == "W0303"), "{:#?}", report.issues);

    // A later screen entity drawn over the text.
    let covered = format!(
        "{HUD}\n[[entity]]\nname = \"blot\"\nscreen_pos = [12, 8]\nsprite = {{ shape = \"rect\", size = [20, 10] }}\n"
    );
    let (_, _, _, report) = render_scene(&covered);
    let w: Vec<_> = report.issues.iter().filter(|d| d.code == "W0305").collect();
    assert!(w.len() == 1 && w[0].message.contains("covered by blot"), "{:#?}", report.issues);
    let allowed = covered.replace("props = { n = 7 }", "props = { n = 7 }\nallow = [\"W0305\"]");
    assert!(!render_scene(&allowed).3.issues.iter().any(|d| d.code == "W0305"));
}

#[test]
fn labels_follow_their_props() {
    let mut stage = setup(HUD);
    let hud = stage.world.find("hud").unwrap();
    let before = stage.image();
    stage.world.get_mut(hud).unwrap().props.insert("n".into(), rwgpuengine_data::PropValue::Int(12));
    let (_, frame, report) = stage.render();
    assert_ne!(frame.image_hash(), before);
    assert_eq!(report.entities.iter().find(|r| r.id == hud).unwrap().text.as_deref(), Some("Coins 12"));
}

/// Writes every glyph to target/font-sheet.png (scale 4) for eyeballing.
/// Run with `cargo test -p rwgpuengine_render font_sheet -- --ignored`.
#[test]
#[ignore]
fn font_sheet() {
    let mut text = String::new();
    for (i, c) in (' '..='~').enumerate() {
        text.push(c);
        if i % 16 == 15 {
            text.push('\n');
        }
    }
    let escaped =
        text.replace('\\', "\\\\").replace('"', "\\\"").replace('{', "{{").replace('}', "}}").replace('\n', "\\n");
    let scene =
        format!("[[entity]]\nname = \"sheet\"\nscreen_pos = [8, 8]\nlabel = {{ text = \"{escaped}\", scale = 3 }}\n");
    let (_, _, frame, _) = render_scene(&scene);
    let png = crate::image::encode_png(frame.width, frame.height, &frame.rgba).unwrap();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/font-sheet.png");
    std::fs::write(&out, png).unwrap();
}

// ---------------------------------------------------------------------------
// Builder kit (ADR 0013): terrain, multi-cell boxes, ground picking

#[test]
fn a_multi_cell_block_looks_like_one_box() {
    // One 2 x 2 block draws the same pixels as four 1 x 1 blocks of the same
    // color and height: boxes that touch hide each other's inner faces, and
    // their tops meet without a gap. Checked where corners fall on whole
    // pixels; elsewhere a pixel exactly on a shared edge may go either way.
    let big = "[ground]\nsize = [4, 4]\n\n[[entity]]\npos = [1, 1]\nfootprint = [2, 2]\nblock = { color = \"#c08040\", height = 1.5 }\n";
    let four = "[ground]\nsize = [4, 4]\n\n[[entity]]\ncells = [[1, 1], [2, 1], [1, 2], [2, 2]]\nblock = { color = \"#c08040\", height = 1.5 }\n";
    for turn in [0.0, 90.0, 180.0, 270.0] {
        let img = |scene: &str| setup(scene).with(CameraView::Iso, turn, 1.0).image();
        assert_eq!(img(big), img(four), "turned {turn}");
    }
}

#[test]
fn things_in_front_of_a_big_box_cover_it_and_things_behind_are_covered() {
    let scene = r##"
[ground]
size = [5, 5]

[[entity]]
name = "house"
pos = [1, 1]
footprint = [2, 2]
block = { color = "#cccccc", top = "#aa2222", height = 1 }

[[entity]]
name = "front"
pos = [3, 2]
sprite = { color = "#00ff00", size = [10, 40] }

[[entity]]
name = "back"
pos = [0, 0]
sprite = { color = "#0000ff", size = [10, 60] }
"##;
    let stage = setup(scene);
    let (_, frame, report) = stage.render();
    let vis = |n: &str| report.entities.iter().find(|e| e.label == n).map(|e| e.visible_px * 100 / e.drawn_px.max(1));
    assert_eq!(vis("front"), Some(100), "the sprite at +x of the house is drawn over it");
    assert!(vis("back").unwrap() < 100, "the sprite behind the house is partly hidden");
    // The roof color is the top face.
    let [x, y] = stage.px(1.5, 1.5, 1.0);
    assert_eq!(frame.pixel(x as u32, y as u32).to_hex(), "#aa2222");
    // From the other side it is the other way round.
    let (_, _, report) = setup(scene).with(CameraView::Iso, 180.0, 1.0).render();
    let vis = |n: &str| report.entities.iter().find(|e| e.label == n).map(|e| e.visible_px * 100 / e.drawn_px.max(1));
    assert_eq!(vis("back"), Some(100));
    assert!(vis("front").unwrap() < 100);
}

#[test]
fn terrain_colors_and_ground_taps() {
    let scene = r##"
[ground]
map = ["gw", "gg"]

[[terrain]]
char = "g"
color = "#00aa00"
alt_color = "#00aa00"

[[terrain]]
char = "w"
color = "#0000aa"
alt_color = "#0000aa"
build = false

[[entity]]
name = "rock"
pos = [0, 1]
block = { height = 3 }
"##;
    let stage = setup(scene);
    let list = stage.seen(true);
    let (seen, frame, _) = stage.render();
    let [wx, wy] = stage.px(1.0, 0.0, 0.0);
    assert_eq!(frame.pixel(wx as u32, wy as u32).to_hex(), "#0000aa");
    // A tap on bare ground reports the cell; one on the rock hits the rock.
    let t = pick(&list, wx as f32 + 0.5, wy as f32 + 0.5);
    assert_eq!((t.hit, t.ground), (None, Some([1, 0])));
    let p = ground_tap_point(&seen, &frame, [1, 1]).expect("visible cell");
    assert_eq!(pick(&list, p[0] as f32 + 0.5, p[1] as f32 + 0.5).ground, Some([1, 1]));
    // The cell under the tall rock has no free pixel.
    assert_eq!(ground_tap_point(&seen, &frame, [0, 1]), None);
    let [rx, ry] = stage.px(0.0, 1.0, 3.0);
    assert_eq!(pick(&list, rx as f32 + 0.5, ry as f32 + 0.5).hit, stage.world.find("rock"));
}

/// A scene with every kind of shape: terrain, tiles, blocks (one multi-cell,
/// one with a top color), the three sprite shapes, see-through colors, world
/// and screen labels, things cut by the frame's edge, and a disabled entity.
const EVERYTHING: &str = r##"
[ground]
map = ["ggww", "gggw", "gggg", "ppgg"]

[[terrain]]
char = "g"
name = "grass"
color = "#3f8a3f"
alt_color = "#3a8039"

[[terrain]]
char = "w"
name = "water"
color = "#2a5d8f"
build = false

[[terrain]]
char = "p"
name = "path"
color = "#b9a26a"

[[entity]]
name = "plate"
pos = [1, 2]
tile = { color = "#ffcc0080" }

[[entity]]
name = "house"
pos = [2, 2]
footprint = [2, 2]
block = { height = 1.5, color = "#b5651d", top = "#8b2f2f" }

[[entity]]
name = "ball"
pos = [0, 0]
sprite = { shape = "circle", color = "#ff000099", size = [30, 22] }

[[entity]]
name = "gem"
pos = [1, 0]
sprite = { shape = "diamond", color = "#4fc3f7", size = [21, 27] }

[[entity]]
name = "ghost"
pos = [0, 3]
sprite = { shape = "rect", color = "#ffffff40", size = [400, 90], offset = [-150, 20] }

[[entity]]
name = "sign"
pos = [3, 0]
block = { height = 0.5 }
label = { text = "Hi {n}!", color = "#ffffffcc", scale = 2 }
props = { n = 7 }

[[entity]]
name = "hud"
screen_pos = [-6, 4]
sprite = { shape = "rect", color = "#00000088", size = [120, 30] }
label = { text = "Coins: {n}\nq~j", scale = 3, offset = [4, 3] }
props = { n = 12 }

[[entity]]
name = "hidden"
pos = [2, 0]
enabled = false
block = { height = 2 }
"##;

#[test]
fn the_painter_gives_the_reference_pixels() {
    let mut painter = Painter::new();
    for scene in [SCENE, EVERYTHING] {
        for (view, turn, zoom) in cameras() {
            let stage = setup(scene).with(view, turn, zoom);
            // With and without disabled items in the list: both skip them.
            for include_disabled in [false, true] {
                let list = build(&stage.world, &stage.view, &stage.camera, include_disabled);
                let frame = render(&project(&list));
                let mut rgba = Vec::new();
                painter.paint(&list, &mut rgba);
                assert_eq!(rgba.len(), frame.rgba.len());
                let first =
                    rgba.iter().zip(&frame.rgba).position(|(a, b)| a != b).map(|i| ((i / 4) % 320, i / 4 / 320));
                assert_eq!(first, None, "{view:?} {turn} {zoom}: the first different pixel (x, y)");
            }
        }
    }
}

#[test]
fn the_painter_overwrites_whatever_the_buffer_held() {
    let stage = setup(EVERYTHING);
    let list = build(&stage.world, &stage.view, &stage.camera, false);
    let frame = render(&project(&list));
    let mut painter = Painter::new();
    // Too long and full of another picture; then too short.
    let mut rgba = vec![0x5a; frame.rgba.len() + 64];
    painter.paint(&list, &mut rgba);
    assert!(rgba == frame.rgba);
    rgba.truncate(10);
    painter.paint(&list, &mut rgba);
    assert!(rgba == frame.rgba);
    // Equal lists compare equal (what a player's "repaint?" check relies on);
    // a moved entity or a moved camera makes them differ.
    assert_eq!(list, build(&stage.world, &stage.view, &stage.camera, false));
    let mut moved = stage.world.clone();
    moved.get_mut(2).unwrap().pos[0] += 1.0;
    assert_ne!(list, build(&moved, &stage.view, &stage.camera, false));
    let turned = setup(EVERYTHING).with(CameraView::Iso, 1.0, 1.0);
    assert_ne!(list, build(&turned.world, &turned.view, &turned.camera, false));
}

// ---------------------------------------------------------------------------
// The world in 3D: cameras, depth, light

#[test]
fn a_tap_hits_what_its_pixel_shows() {
    // For every camera, picking agrees with the picture pixel by pixel: the
    // entity or ground cell that owns a pixel is what a tap there gets.
    for scene in [SCENE, EVERYTHING] {
        for (view, turn, zoom) in cameras() {
            let stage = setup(scene).with(view, turn, zoom);
            let (seen, frame, _) = stage.render();
            let mut checked = 0;
            for y in (0..240).step_by(2) {
                for x in ((y / 2) % 2..320).step_by(2) {
                    let tap = pick(&seen, x as f32 + 0.5, y as f32 + 0.5);
                    let owner = frame.owner[y * 320 + x];
                    let want = match owner.checked_sub(1).map(|i| &seen.items[i as usize]) {
                        None => (None, None),
                        Some(i) if i.entity.is_some() => (i.entity, None),
                        Some(i) => (None, Some(i.cell)),
                    };
                    assert_eq!((tap.hit, tap.ground), want, "{view:?} {turn} {zoom} at ({x}, {y})");
                    checked += 1;
                }
            }
            assert_eq!(checked, 19_200);
        }
    }
}

#[test]
fn every_ground_cell_that_shows_can_be_tapped_from_every_camera() {
    let scene = "[ground]\nsize = [6, 5]\n\n[[entity]]\nname = \"tower\"\npos = [2, 2]\nblock = { height = 2 }\n";
    for (view, turn, zoom) in cameras() {
        let stage = setup(scene).with(view, turn, zoom);
        let (seen, frame, _) = stage.render();
        let mut shown = 0;
        for y in 0..5 {
            for x in 0..6 {
                if let Some(p) = ground_tap_point(&seen, &frame, [x, y]) {
                    shown += 1;
                    let tap = pick(&seen, p[0] as f32 + 0.5, p[1] as f32 + 0.5);
                    assert_eq!(tap.ground, Some([x, y]), "{view:?} {turn} {zoom}");
                }
            }
        }
        // The tower's own cell never shows; most others do, unless the
        // camera is zoomed in on a few.
        assert_eq!(ground_tap_point(&seen, &frame, [2, 2]), None);
        assert!(shown >= if zoom > 2.0 { 6 } else { 20 }, "{view:?} {turn} {zoom}: {shown} cells");
    }
}

#[test]
fn the_light_shades_each_side_of_a_box() {
    let scene =
        "[[entity]]\nname = \"box\"\npos = [0, 0]\nblock = { color = \"#c8c8c8\", top = \"#112233\", height = 1 }\n";
    // Which sides show at each quarter turn, and 200 times the light on them.
    let side = |turn: f32, right: bool| {
        let stage = setup(scene).with(CameraView::Iso, turn, 1.0);
        let (_, frame, _) = stage.render();
        // Half-way up, a quarter of a cell either side of the middle.
        let [mx, my] = stage.px(0.0, 0.0, 0.5);
        let x = if right { mx + 16 } else { mx - 16 };
        let p = frame.pixel(x as u32, (my + 12) as u32);
        assert!(p.r == p.g && p.g == p.b, "a grey side: {p:?}");
        p.r
    };
    // At the start: +y on the left at 0.8, +x on the right at 0.65.
    assert_eq!((side(0.0, false), side(0.0, true)), (160, 130));
    // Turned half round: the two sides the 2.5D engine never showed.
    assert_eq!((side(180.0, false), side(180.0, true)), (70, 100));
    assert_eq!((side(90.0, false), side(90.0, true)), (130, 70));
    assert_eq!((side(270.0, false), side(270.0, true)), (100, 160));
    // The top keeps its own color under every camera.
    for (view, turn, zoom) in cameras() {
        let stage = setup(scene).with(view, turn, zoom);
        let [x, y] = stage.px(0.0, 0.0, 1.0);
        // Zoomed far in, the perspective camera is level with the top.
        if (0..320).contains(&x) && (0..240).contains(&y) {
            assert_eq!(stage.render().1.pixel(x as u32, y as u32).to_hex(), "#112233", "{view:?} {turn} {zoom}");
        } else {
            assert_eq!((view, zoom), (CameraView::Perspective, 3.0));
        }
    }
}

#[test]
fn flat_things_lie_on_the_surface_under_them() {
    // A tile on the ground, a tile on top of a box, and a block of height 0
    // on a tile: each is in exactly the plane of what it lies on, and each
    // must show, whole, from every camera.
    let scene = r##"
[ground]
size = [5, 5]

[[entity]]
name = "plate"
pos = [1, 1]
tile = { color = "#ff0000" }

[[entity]]
name = "table"
pos = [3, 1]
block = { color = "#808080", height = 1 }

[[entity]]
name = "mat"
pos = [3, 1, 1]
tile = { color = "#00ff00" }

[[entity]]
name = "rug"
pos = [1, 3]
tile = { color = "#0000ff" }
block = { color = "#ffff00", height = 0 }
"##;
    for (view, turn, zoom) in cameras() {
        let stage = setup(scene).with(view, turn, zoom);
        let (seen, frame, report) = stage.render();
        let cov = crate::reference::coverage(&seen, &frame);
        let id = |name: &str| stage.world.find(name);
        // No pixel of a flat thing shows what it lies on instead.
        let lost_to = |flat: &str, under: Option<u32>| -> u32 {
            let item = seen.items.iter().position(|i| i.entity == id(flat) && i.part == crate::Part::Tile).unwrap();
            cov.items[item].covered_by.iter().filter(|(o, _)| seen.items[**o].entity == under).map(|(_, n)| *n).sum()
        };
        assert_eq!(lost_to("plate", None), 0, "{view:?} {turn} {zoom}: the ground shows through the plate");
        assert_eq!(lost_to("mat", id("table")), 0, "{view:?} {turn} {zoom}: the table shows through the mat");
        let shows = |name: &str| report.entities.iter().find(|e| e.label == name).map_or(0, |e| e.visible_px);
        if zoom <= 2.0 {
            assert!(shows("plate") > 0 && shows("mat") > 0, "{view:?} {turn} {zoom}");
        }
        // The rug's block of height 0 is over its own tile, which never shows.
        let blue = frame.rgba.as_chunks::<4>().0.iter().filter(|p| **p == [0, 0, 255, 255]).count();
        assert_eq!(blue, 0, "{view:?} {turn} {zoom}");
        let [x, y] = stage.px(1.0, 3.0, 0.0);
        if (0..320).contains(&x) && (0..240).contains(&y) {
            assert_eq!(frame.pixel(x as u32, y as u32).to_hex(), "#ffff00", "{view:?} {turn} {zoom}");
        }
    }
}

/// Pixels of `name`'s `part` that show something of `under` instead (the
/// ground if `under` is `None`).
fn lost_to(stage: &Stage, name: &str, part: crate::Part, under: Option<&str>) -> u32 {
    let (seen, frame, _) = stage.render();
    let cov = crate::reference::coverage(&seen, &frame);
    let id = stage.world.find(name);
    let under = under.map(|n| stage.world.find(n).expect("the entity under it"));
    let item = seen.items.iter().position(|i| i.entity == id && i.part == part).expect("the part is drawn");
    cov.items[item].covered_by.iter().filter(|(o, _)| seen.items[**o].entity == under).map(|(_, n)| *n).sum()
}

#[test]
fn flat_things_lie_on_the_surface_under_them_whatever_their_corners() {
    // A tile of 3 x 3 cells on the ground, a mat on one cell of a 2 x 2
    // table, a rug of two cells over a plate of one: none has the corners of
    // what it lies on, so their triangles are not the same triangles. Each
    // is in exactly that plane all the same, and shows whole from every camera.
    let scene = r##"
[ground]
size = [6, 6]

[[entity]]
name = "plaza"
pos = [1, 1]
footprint = [3, 3]
tile = { color = "#ff0000" }

[[entity]]
name = "table"
pos = [4, 4]
footprint = [2, 2]
block = { color = "#808080", height = 1 }

[[entity]]
name = "mat"
pos = [5, 4, 1]
tile = { color = "#00ff00" }

[[entity]]
name = "plate"
pos = [4, 0]
tile = { color = "#0000ff" }

[[entity]]
name = "rug"
pos = [4, 0]
footprint = [2, 1]
block = { color = "#ffff00", height = 0 }
"##;
    let mut seen_in = [0u32; 2];
    for (view, turn, zoom) in cameras() {
        let stage = setup(scene).with(view, turn, zoom);
        let cam = format!("{view:?} {turn} {zoom}");
        assert_eq!(lost_to(&stage, "plaza", crate::Part::Tile, None), 0, "{cam}: the ground shows through the plaza");
        assert_eq!(
            lost_to(&stage, "mat", crate::Part::Tile, Some("table")),
            0,
            "{cam}: the table shows through the mat"
        );
        assert_eq!(lost_to(&stage, "rug", crate::Part::Block, None), 0, "{cam}: the ground shows through the rug");
        assert_eq!(
            lost_to(&stage, "rug", crate::Part::Block, Some("plate")),
            0,
            "{cam}: the plate shows through the rug"
        );
        // The plate is under the rug everywhere: it never shows.
        let (_, frame, report) = stage.render();
        let blue = frame.rgba.as_chunks::<4>().0.iter().filter(|p| **p == [0, 0, 255, 255]).count();
        assert_eq!(blue, 0, "{cam}");
        // The plaza is in the middle of every picture; the mat and the rug
        // are in most of them.
        let shows = |name: &str| report.entities.iter().find(|e| e.label == name).map_or(0, |e| e.visible_px);
        assert!(shows("plaza") > 0, "{cam}");
        seen_in[0] += u32::from(shows("mat") > 0);
        seen_in[1] += u32::from(shows("rug") > 0);
    }
    assert!(seen_in[0] >= 4 && seen_in[1] >= 4, "{seen_in:?}");
}

#[test]
fn of_two_flat_things_of_one_kind_in_one_plane_the_first_stays() {
    // Two tiles with the same rank, one inside the other: the first in the
    // file shows wherever they overlap, under every camera.
    let scene = |first: &str, second: &str| {
        format!(
            "[ground]\nsize = [6, 6]\n\n[[entity]]\nname = \"big\"\npos = [1, 1]\nfootprint = [3, 3]\ntile = {{ color = \"{first}\" }}\n\n[[entity]]\nname = \"small\"\npos = [2, 2]\ntile = {{ color = \"{second}\" }}\n"
        )
    };
    for (view, turn, zoom) in cameras() {
        let stage = setup(&scene("#ff0000", "#0000ff")).with(view, turn, zoom);
        let (_, frame, _) = stage.render();
        let blue = frame.rgba.as_chunks::<4>().0.iter().filter(|p| **p == [0, 0, 255, 255]).count();
        assert_eq!(blue, 0, "{view:?} {turn} {zoom}: the second tile shows through the first");
    }
}

#[test]
fn the_part_of_a_card_below_its_anchor_shows_on_the_floor_it_lies_on() {
    // Sprites nudged ten pixels down: one on the ground, one on a tile that
    // comes later in the file, one on top of its own box. Below its anchor
    // each lies in the plane of the floor it stands on, and the floor does
    // not cut it, from any camera.
    let scene = r##"
[ground]
size = [6, 6]

[[entity]]
name = "walker"
pos = [1, 1]
sprite = { shape = "rect", color = "#ff0000", size = [16, 24], offset = [0, 10] }

[[entity]]
name = "guest"
pos = [3, 1]
sprite = { shape = "circle", color = "#00ffff", size = [20, 24], offset = [0, 10] }

[[entity]]
name = "plate"
pos = [3, 1]
tile = { color = "#d9c27a" }

[[entity]]
name = "statue"
pos = [1, 3]
block = { color = "#808080", height = 1 }
sprite = { shape = "diamond", color = "#ff00ff", size = [24, 24], offset = [0, 10] }
"##;
    for (view, turn, zoom) in cameras() {
        let stage = setup(scene).with(view, turn, zoom);
        let cam = format!("{view:?} {turn} {zoom}");
        let sprite = crate::Part::Sprite;
        assert_eq!(lost_to(&stage, "walker", sprite, None), 0, "{cam}: the ground cuts the walker");
        assert_eq!(lost_to(&stage, "guest", sprite, None), 0, "{cam}: the ground cuts the guest");
        assert_eq!(lost_to(&stage, "guest", sprite, Some("plate")), 0, "{cam}: the plate cuts the guest");
        assert_eq!(lost_to(&stage, "statue", sprite, Some("statue")), 0, "{cam}: its own box cuts the statue's sprite");
        // Nothing else is near the walker: all of it shows.
        let (_, _, report) = stage.render();
        let walker = report.entities.iter().find(|e| e.label == "walker").unwrap();
        if walker.on_screen && !walker.clipped {
            assert_eq!(walker.visible_px, walker.drawn_px, "{cam}");
        }
    }
    // What is really in front still covers it: a box toward the viewer.
    let behind = format!("{scene}\n[[entity]]\nname = \"wall\"\npos = [2, 2]\nblock = {{ height = 1 }}\n");
    let stage = setup(&behind);
    assert!(lost_to(&stage, "walker", crate::Part::Sprite, Some("wall")) > 0);
}

#[test]
fn a_card_keeps_the_row_just_above_its_anchor() {
    // Boxes 16.5 pixels high, less the last bit of that number, each with a
    // sprite on top: the anchor is a hair above a pixel center. The row there
    // is upright by less than a depth can hold. It must not fall behind the
    // top the sprite stands on, wherever on the ground that is.
    let mut scene = String::from("[ground]\nsize = [6, 6]\n");
    for y in 0..6 {
        for x in 0..6 {
            scene.push_str(&format!(
                "\n[[entity]]\nname = \"s{x}{y}\"\npos = [{x}, {y}]\nblock = {{ color = \"#808080\", height = 0.51562494 }}\nsprite = {{ shape = \"rect\", color = \"#ff0000\", size = [8, 14], offset = [0, 4] }}\n"
            ));
        }
    }
    let stage = setup(&scene);
    let height = stage.world.entities[0].block.as_ref().map(|b| b.height.to_bits());
    assert_eq!(height, Some((33.0f32 / 64.0).to_bits() - 1));
    let (_, frame, _) = stage.render();
    let mut checked = 0;
    for y in 0..6 {
        for x in 0..6 {
            // The sprite's middle column, from its top row to the row above
            // the box's top edge in front of it: all red, no gap at the anchor.
            let [sx, sy] = stage.px(x as f32, y as f32, 0.515_625);
            let rows = sy - 10..sy + 3;
            if !(4..316).contains(&sx) || rows.start < 0 || rows.end > 240 {
                continue;
            }
            let column: Vec<bool> = rows.map(|r| frame.pixel(sx as u32, r as u32).to_hex() == "#ff0000").collect();
            assert!(column.iter().all(|red| *red), "the sprite at ({x}, {y}): {column:?}");
            checked += 1;
        }
    }
    assert!(checked >= 20, "{checked}");
}

#[test]
fn a_sprite_keeps_its_last_row_at_any_size() {
    // Seen at another size than its own, a sprite's last row can fall just
    // below its anchor. The floor it stands on does not take that row.
    let mut scene = String::from("[ground]\nsize = [6, 6]\n");
    for y in 0..6 {
        for x in (y % 2..6).step_by(2) {
            scene.push_str(&format!(
                "\n[[entity]]\nname = \"s{x}{y}\"\npos = [{x}, {y}]\nsprite = {{ shape = \"rect\", size = [8, 12] }}\n"
            ));
        }
    }
    for (view, turn, zoom) in cameras() {
        let stage = setup(&scene).with(view, turn, zoom);
        for y in 0..6 {
            for x in (y % 2..6).step_by(2) {
                let lost = lost_to(&stage, &format!("s{x}{y}"), crate::Part::Sprite, None);
                assert_eq!(lost, 0, "{view:?} {turn} {zoom}: the ground cuts the sprite at ({x}, {y})");
            }
        }
    }
}

#[test]
fn a_tile_does_not_show_through_the_foot_of_a_box_standing_on_it() {
    // A box side is ordered against the tile by depth alone, and it is in
    // front of it everywhere above its foot.
    let scene = "[ground]\nsize = [4, 4]\n\n[[entity]]\nname = \"crate\"\npos = [1, 1]\ntile = { color = \"#ff00ff\" }\nblock = { color = \"#404040\", height = 1 }\n";
    for (view, turn, zoom) in cameras() {
        let (_, frame, _) = setup(scene).with(view, turn, zoom).render();
        let pink = frame.rgba.as_chunks::<4>().0.iter().filter(|p| **p == [255, 0, 255, 255]).count();
        assert_eq!(pink, 0, "{view:?} {turn} {zoom}");
    }
}

#[test]
fn see_through_things_blend_over_what_is_behind_them() {
    let scene = r##"
[[entity]]
name = "wall"
pos = [0, 0]
block = { color = "#000000", top = "#000000", height = 2 }

[[entity]]
name = "pane"
pos = [1, 1]
sprite = { shape = "rect", color = "#ffffff80", size = [40, 80] }

[[entity]]
name = "far"
pos = [-1, -1]
sprite = { shape = "rect", color = "#ff0000", size = [20, 120] }
"##;
    let stage = setup(scene);
    let (seen, frame, report) = stage.render();
    // The pane is in front of the wall: half white over black.
    let [x, y] = stage.px(1.0, 1.0, 0.0);
    assert_eq!(frame.pixel(x as u32, (y - 30) as u32).to_hex(), "#808080");
    // It owns those pixels, and a tap there hits it.
    let pane = stage.world.find("pane");
    assert_eq!(pick(&seen, x as f32 + 0.5, y as f32 - 29.5).hit, pane);
    let vis = |n: &str| report.entities.iter().find(|e| e.label == n).map(|e| (e.visible_px, e.drawn_px)).unwrap();
    assert_eq!(vis("pane").0, vis("pane").1);
    // The far sprite is drawn after the pane in the file but is behind the
    // wall and the pane: it shows only above the wall, and its red does not
    // reach the pane's pixels.
    assert!(vis("far").0 < vis("far").1);
    assert_eq!(frame.pixel(x as u32, (y - 60) as u32).to_hex(), "#808080");
}

#[test]
fn perspective_cuts_what_is_too_near_and_reports_what_is_out_of_range() {
    // The camera hangs over a wide, low hall: part of its roof and of the
    // ground is behind the camera, and more of it the further it zooms in.
    // Nothing may wrap round or panic, and the picture stays a function of
    // the list.
    let scene = "[ground]\nsize = [40, 40]\n\n[[entity]]\nname = \"hall\"\npos = [10, 10]\nfootprint = [20, 20]\nblock = { height = 0.25 }\n\n[[entity]]\nname = \"lost\"\npos = [39, 39]\nsprite = {}\n\n[[entity]]\nname = \"here\"\npos = [19, 19, 0.25]\nsprite = { color = \"#ff0000\", size = [8, 8] }\n";
    for zoom in [1.0, 4.0, 8.0] {
        for turn in [0.0, 123.0] {
            let stage = setup(scene).with(CameraView::Perspective, turn, zoom);
            let (seen, frame, report) = stage.render();
            assert_eq!(frame.image_hash(), stage.image());
            // The middle of the frame shows the hall's roof, which reaches
            // behind the camera: cut there, not dropped.
            let tap = pick(&seen, 160.5, 120.5);
            assert_eq!(tap.hit, stage.world.find("hall"), "zoom {zoom} turn {turn}: {tap:?}");
            let hall = report.entities.iter().find(|e| e.label == "hall").unwrap();
            assert!(hall.visible_px > 10_000 && hall.clipped, "zoom {zoom} turn {turn}: {hall:?}");
            if zoom == 1.0 {
                let here = report.entities.iter().find(|e| e.label == "here").unwrap();
                assert!(!here.offscreen && here.visible_px > 0, "turn {turn}: {here:?}");
            }
            if turn == 0.0 {
                // (39, 39) is toward the viewer, behind the camera.
                let lost = report.entities.iter().find(|e| e.label == "lost").unwrap();
                assert!(lost.offscreen && lost.drawn_px == 0, "zoom {zoom}: {lost:?}");
                let w = report.issues.iter().find(|d| d.code == "W0301").expect("W0301");
                assert_eq!(w.message, "lost is out of the camera's sight (behind it, or too far away)");
            }
        }
    }
}

#[test]
fn boxes_whose_height_is_not_a_whole_pixel_are_drawn_at_their_height() {
    // 1.2 units of height are 38.4 pixels: the top's corners are on the
    // 1/256 pixel grid, not rounded to 38.
    let scene =
        "[[entity]]\nname = \"box\"\npos = [0, 0]\nblock = { color = \"#404040\", top = \"#ffffff\", height = 1.2 }\n";
    let stage = setup(scene);
    let (seen, frame, _) = stage.render();
    let [fx, fy] = stage.at(0.5, 0.5, 1.2);
    assert!((fy.fract() - 0.6).abs() < 1e-4, "the front corner of the top is at y = ...6, not a whole pixel: {fy}");
    // Straight above the front corner: white from 0.6 up, so the pixel whose
    // center is at .5 is side, and the one above it is top.
    let (x, y) = (fx as u32, fy.floor() as u32);
    assert_eq!(frame.pixel(x, y - 1).to_hex(), "#ffffff");
    assert_ne!(frame.pixel(x, y).to_hex(), "#ffffff");
    assert_eq!(seen.bounds_of(stage.world.find("box").unwrap()).xywh()[3], 32 + 39);
}
