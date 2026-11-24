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
