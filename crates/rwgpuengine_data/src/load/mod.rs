//! Loading and checking projects from an in-memory file set.
//!
//! Everything reads from a [`Vfs`] (project-relative path → text), so the
//! same code runs natively (files read from disk) and in the browser (files
//! shipped as one bundle). Nothing here runs the simulation: `rwgpu check`
//! is pure static analysis.
//!
//! A file goes through stages (ADR 0028):
//!
//! 1. **parse** ([`parse`]): text to nodes, checked against the schema.
//! 2. **resolve** ([`resolve`], [`ground`]): for a scene, the ground and its
//!    terrains, prefabs merged into entities, names, and what each
//!    [[connect]] refers to. What cannot be found is reported here.
//! 3. **check** ([`check`]): each rule on its own, on what was resolved.
//! 4. **build**: the definitions the simulation is made from.
//!
//! A scene's problems are reported in the order of its file, whichever
//! stage found them ([`order`]). The project file and playtests
//! ([`playtest`]) have no references of their own to resolve: they are
//! parsed and decoded with their rules, and `check_project` checks a
//! playtest against its scene.

mod check;
mod ground;
mod order;
mod parse;
mod playtest;
mod resolve;
mod scene;
mod vfs;

use std::collections::{BTreeMap, BTreeSet};

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::{self, Diagnostic, Span};
use rwgpuengine_core::text::did_you_mean;

use crate::defs::{self, ProjectDef, SceneDef};
use crate::schema;
use parse::{Parsed, parse_checked};
use playtest::load_playtest_lenient;

pub use playtest::load_playtest;
pub use scene::SceneLoader;
pub use vfs::Vfs;

/// The project marker file, like Cargo.toml for cargo.
pub const PROJECT_FILE: &str = "rwgpu.toml";

/// Load `rwgpu.toml`.
pub fn load_project(vfs: &Vfs) -> (Option<ProjectDef>, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    if !vfs.contains(PROJECT_FILE) {
        diags.push(
            Diagnostic::new(&codes::PROJECT_NOT_FOUND, "no rwgpu.toml in this folder or any parent")
                .hint("run inside a project, pass the project folder, or create one with `rwgpu new <path>`"),
        );
        return (None, diags);
    }
    let Some(Parsed { src, root, ok }) = parse_checked(vfs, PROJECT_FILE, &schema::PROJECT_FILE, &mut diags) else {
        return (None, diags);
    };
    let project = defs::decode_project(&root);
    let span_of =
        |section: &str, key: &str| root.table(section).map_or(Span::default(), |t| t.value_span(key, Span::default()));
    if ok && !vfs.contains(&project.main_scene) {
        diags.push(
            Diagnostic::new(&codes::UNKNOWN_SCENE, format!("main_scene `{}` does not exist", project.main_scene))
                .at(&src, span_of("project", "main_scene"))
                .maybe_hint(did_you_mean(&project.main_scene, vfs.paths_under("scenes", ".toml"))),
        );
    }
    if let Some(engine) = &project.engine {
        let running = rwgpuengine_core::ENGINE_VERSION;
        if !(running == engine || running.starts_with(&format!("{engine}."))) {
            diags.push(
                Diagnostic::new(
                    &codes::ENGINE_MISMATCH,
                    format!("project targets rwgpu {engine}, but this is rwgpu {running}"),
                )
                .at(&src, span_of("project", "engine"))
                .hint(format!("set `engine = \"{}\"` once the project works with this version", major_minor(running))),
            );
        }
    }
    let has_errors = diags.iter().any(Diagnostic::is_error);
    (if ok && !has_errors { Some(project) } else { None }, diags)
}

/// "0.1.0" -> "0.1"
pub fn major_minor(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// Result of `rwgpu check`.
#[derive(Debug)]
pub struct CheckReport {
    pub project: Option<ProjectDef>,
    pub scenes: Vec<String>,
    pub prefabs: Vec<String>,
    pub playtests: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
}

impl CheckReport {
    pub fn ok(&self) -> bool {
        !self.diagnostics.iter().any(Diagnostic::is_error)
    }
}

/// Validate every file of a project without running anything: the project
/// file, every scene, every prefab and every playtest, each playtest
/// against its scene. Sorted by file and position.
pub fn check_project(vfs: &Vfs) -> CheckReport {
    let (project, mut diagnostics) = load_project(vfs);
    let mut loader = SceneLoader::new(vfs);

    let mut scene_paths: BTreeSet<String> = vfs.paths_under("scenes", ".toml").map(str::to_string).collect();
    if let Some(p) = &project
        && vfs.contains(&p.main_scene)
    {
        scene_paths.insert(p.main_scene.clone());
    }
    let mut scenes: BTreeMap<String, Option<SceneDef>> = BTreeMap::new();
    for path in &scene_paths {
        let scene = loader.load_scene(path);
        scenes.insert(path.clone(), scene);
    }
    let prefabs = loader.prefab_names();
    for name in &prefabs {
        loader.prefab(name);
    }

    let playtests: Vec<String> = vfs.paths_under("tests", ".toml").map(str::to_string).collect();
    for path in &playtests {
        let (def, diags) = load_playtest_lenient(vfs, path);
        diagnostics.extend(diags);
        let (Some(def), Some(project)) = (def, &project) else {
            continue;
        };
        let scene_path = def.scene.clone().unwrap_or_else(|| project.main_scene.clone());
        if !vfs.contains(&scene_path) {
            diagnostics.push(
                Diagnostic::new(&codes::UNKNOWN_SCENE, format!("playtest scene `{scene_path}` does not exist"))
                    .at(&def.src, def.scene_span)
                    .maybe_hint(did_you_mean(&scene_path, vfs.paths_under("scenes", ".toml"))),
            );
            continue;
        }
        let scene = match scenes.get(&scene_path) {
            Some(s) => s.clone(),
            None => {
                let s = loader.load_scene(&scene_path);
                scenes.insert(scene_path.clone(), s.clone());
                s
            }
        };
        let Some(scene) = scene else { continue };
        playtest::expectations(vfs, &def, &scene, &scene_path, &mut diagnostics);
    }

    diagnostics.extend(std::mem::take(&mut loader.diags));
    diag::sort(&mut diagnostics);
    diagnostics.dedup();
    CheckReport { project, scenes: scene_paths.into_iter().collect(), prefabs, playtests, diagnostics }
}
