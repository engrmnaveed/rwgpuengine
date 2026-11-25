use slate_data::{SceneLoader, Vfs, load_project};
use slate_sim::World;

use crate::draw::build;
use crate::pick::pick;
use crate::raster::rasterize;
use crate::report::build_report;
use crate::view::View;

fn setup(scene: &str) -> (World, View, slate_data::SceneDef) {
    let mut vfs = Vfs::new();
    vfs.insert("slate.toml", "[project]\nname = \"t\"\n[display]\nwidth = 320\nheight = 240\n");
    vfs.insert("scenes/main.toml", scene);
    let (project, d) = load_project(&vfs);
    let project = project.unwrap_or_else(|| panic!("{d:#?}"));
    let mut loader = SceneLoader::new(&vfs);
    let def = loader.load_scene("scenes/main.toml").unwrap_or_else(|| panic!("{:#?}", loader.diags));
    let view = View::new(&project, &def);
    (World::new(&def, 1), view, def)
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
    let (_, view, _) = setup(SCENE);
    // Ground center (1.5, 1.5) lands in the middle of the 320x240 frame.
    assert_eq!(view.project_px(1.5, 1.5, 0.0), [160, 120]);
}

#[test]
fn renders_deterministically_and_reports_what_is_where() {
    let (world, view, def) = setup(SCENE);
    let list = build(&world, &view, def.ground.as_ref(), false);
    let (frame, stats) = rasterize(&list);
    let (frame2, _) = rasterize(&list);
    assert_eq!(frame.image_hash(), frame2.image_hash());

    // The hero's sprite is drawn in red, bottom edge on its cell center.
    let [cx, cy] = view.project_px(1.0, 1.0, 0.0);
    assert_eq!(frame.pixel(cx as u32, (cy - 10) as u32).to_hex(), "#ff0000");

    let report = build_report(&world, &list, &frame, &stats);
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
    let (world, view, def) = setup(scene);
    let list = build(&world, &view, def.ground.as_ref(), false);
    let (frame, stats) = rasterize(&list);
    let report = build_report(&world, &list, &frame, &stats);
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
    let (world, view, def) = setup(scene);
    let list = build(&world, &view, def.ground.as_ref(), true);
    let [bx, by] = view.project(1.0, 1.0, 0.0);
    assert_eq!(pick(&list, bx, by - 10.0).hit, world.find("button"));
    let [lx, ly] = view.project(2.0, 1.0, 0.0);
    let miss = pick(&list, lx, ly - 10.0);
    assert_eq!(miss.hit, None);
    assert_eq!(miss.under_disabled, vec![world.find("lever").unwrap()]);
    // Ground stops the search: a tap on bare floor hits nothing.
    let [gx, gy] = view.project(3.0, 3.0, 0.0);
    assert_eq!(pick(&list, gx, gy).hit, None);
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
    let (world, view, def) = setup(scene);
    let list = build(&world, &view, def.ground.as_ref(), false);
    let (frame, stats) = rasterize(&list);
    let report = build_report(&world, &list, &frame, &stats);
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
    let (world, view, def) = setup(scene);
    let list = build(&world, &view, def.ground.as_ref(), true);
    let lever = world.find("lever").unwrap();
    let [x, y] = crate::pick::tap_point(&list, lever).expect("lever is tappable");
    assert_eq!(pick(&list, x as f32 + 0.5, y as f32 + 0.5).hit, Some(lever));
    // A floor tile completely behind a tall block cannot be tapped.
    assert_eq!(crate::pick::tap_point(&list, world.find("buried").unwrap()), None);
}

#[test]
fn allowed_warnings_are_not_reported() {
    let scene = |allow: &str| {
        format!(
            "[ground]\nsize = [4, 4]\n\n[[entity]]\nname = \"hero\"\npos = [1, 1]\nsprite = {{ size = [16, 24] }}\n{allow}\n\n[[entity]]\nname = \"tower\"\npos = [2, 2]\nblock = {{ height = 3 }}\n"
        )
    };
    let issues = |text: &str| {
        let (world, view, def) = setup(text);
        let list = build(&world, &view, def.ground.as_ref(), false);
        let (frame, stats) = rasterize(&list);
        build_report(&world, &list, &frame, &stats).issues.iter().map(|d| d.code).collect::<Vec<_>>()
    };
    assert!(issues(&scene("")).contains(&"W0302"));
    assert!(!issues(&scene("allow = [\"W0302\"]")).contains(&"W0302"));
}

// ---------------------------------------------------------------------------
// Labels and screen entities (ADR 0011)

fn render(text: &str) -> (World, crate::DrawList, crate::Frame, crate::FrameReport) {
    let (world, view, def) = setup(text);
    let list = build(&world, &view, def.ground.as_ref(), false);
    let (frame, stats) = rasterize(&list);
    let report = build_report(&world, &list, &frame, &stats);
    (world, list, frame, report)
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
    let (world, list, frame, report) = render(HUD);
    let hud = world.find("hud").unwrap();
    // Text box: "Coins 7" = 7 chars -> 41 x 8 px, top-left at (13, 9).
    let text_rect = list
        .items
        .iter()
        .filter(|i| i.entity == Some(hud) && i.part == crate::Part::Label)
        .fold(crate::Rect::EMPTY, |a, i| a.union(&i.shape.bbox()));
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
    let (world, list, _, _) = render(HUD);
    let tag = world.find("tag").unwrap();
    let r = list.bounds_of(tag);
    // "Hi" at scale 2: (2 * 6 - 1) * 2 = 22 wide, 8 * 2 = 16 tall.
    assert_eq!((r.x1 - r.x0, r.y1 - r.y0), (22, 16));
    // Centered on the cell's standing point; the box bottom (row 8 * 2) is on it.
    let p = setup(HUD).1.project_px(1.0, 1.0, 0.0);
    assert_eq!(r.x0, p[0] - 11);
    assert_eq!(r.y0, p[1] - 16);
}

#[test]
fn cut_off_or_covered_labels_are_reported() {
    // The backdrop fits (250..310); the text (290..331) does not: only W0305.
    let off =
        HUD.replace("screen_pos = [10, 6]", "screen_pos = [250, 6]").replace("offset = [3, 3]", "offset = [40, 3]");
    let (_, _, _, report) = render(&off);
    let w: Vec<_> = report.issues.iter().filter(|d| d.code == "W0305").collect();
    assert_eq!(w.len(), 1, "{:#?}", w);
    assert!(w[0].message.contains("\"Coins 7\"") && w[0].message.contains("runs past the edge"), "{}", w[0].message);
    assert!(!report.issues.iter().any(|d| d.code == "W0303"), "{:#?}", report.issues);

    // A later screen entity drawn over the text.
    let covered = format!(
        "{HUD}\n[[entity]]\nname = \"blot\"\nscreen_pos = [12, 8]\nsprite = {{ shape = \"rect\", size = [20, 10] }}\n"
    );
    let (_, _, _, report) = render(&covered);
    let w: Vec<_> = report.issues.iter().filter(|d| d.code == "W0305").collect();
    assert!(w.len() == 1 && w[0].message.contains("covered by blot"), "{:#?}", report.issues);
    let allowed = covered.replace("props = { n = 7 }", "props = { n = 7 }\nallow = [\"W0305\"]");
    assert!(!render(&allowed).3.issues.iter().any(|d| d.code == "W0305"));
}

#[test]
fn labels_follow_their_props() {
    let (mut world, view, def) = setup(HUD);
    let hud = world.find("hud").unwrap();
    let before = rasterize(&build(&world, &view, def.ground.as_ref(), false)).0.image_hash();
    world.get_mut(hud).unwrap().props.insert("n".into(), slate_data::PropValue::Int(12));
    let list = build(&world, &view, def.ground.as_ref(), false);
    let (frame, stats) = rasterize(&list);
    assert_ne!(frame.image_hash(), before);
    let report = build_report(&world, &list, &frame, &stats);
    assert_eq!(report.entities.iter().find(|r| r.id == hud).unwrap().text.as_deref(), Some("Coins 12"));
}

/// Writes every glyph to target/font-sheet.png (scale 4) for eyeballing.
/// Run with `cargo test -p slate_render font_sheet -- --ignored`.
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
    let (_, _, frame, _) = render(&scene);
    let png = crate::image::encode_png(frame.width, frame.height, &frame.rgba).unwrap();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/font-sheet.png");
    std::fs::write(&out, png).unwrap();
}
