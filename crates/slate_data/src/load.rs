//! Loading and checking projects from an in-memory file set.
//!
//! Everything reads from a [`Vfs`] (project-relative path → text), so the
//! same code runs natively (files read from disk) and in the browser (files
//! shipped as one bundle). Nothing here runs the simulation: `slate check`
//! is pure static analysis.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use slate_core::codes;
use slate_core::diag::{self, Diagnostic, SourceFile, Span};
use slate_core::hash::Hasher64;
use slate_core::text::did_you_mean;

use crate::defs::{self, Origin, PlaytestDef, ProjectDef, SceneDef};
use crate::node::{Table, parse_toml};
use crate::schema::{self, Checker, Fields};

/// The project marker file, like Cargo.toml for cargo.
pub const PROJECT_FILE: &str = "slate.toml";

/// Project-relative text files. Paths use `/` and never start with `/`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vfs {
    files: BTreeMap<String, String>,
}

const BUNDLE_HEADER: &str = "slate-bundle 1\n";

impl Vfs {
    pub fn new() -> Vfs {
        Vfs::default()
    }

    pub fn insert(&mut self, path: impl Into<String>, text: impl Into<String>) {
        self.files.insert(path.into(), text.into());
    }

    pub fn get(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }

    pub fn contains(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    /// Paths under `dir/` ending with `ext`, sorted.
    pub fn paths_under<'a>(&'a self, dir: &'a str, ext: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.files
            .keys()
            .map(String::as_str)
            .filter(move |p| p.starts_with(dir) && p[dir.len()..].starts_with('/') && p.ends_with(ext))
    }

    /// Read every `.toml` file under `root` (skipping hidden folders,
    /// `target/` and `out/`).
    pub fn from_dir(root: &Path) -> std::io::Result<Vfs> {
        let mut vfs = Vfs::new();
        walk(root, root, &mut vfs)?;
        Ok(vfs)
    }

    /// Serialize for the web player: a header line, then `path\0text\0` pairs.
    pub fn to_bundle(&self) -> String {
        let mut out = String::from(BUNDLE_HEADER);
        for (path, text) in &self.files {
            out.push_str(path);
            out.push('\0');
            out.push_str(text);
            out.push('\0');
        }
        out
    }

    pub fn from_bundle(bundle: &str) -> Result<Vfs, String> {
        let body = bundle.strip_prefix(BUNDLE_HEADER).ok_or("not a slate bundle (bad header)")?;
        let mut vfs = Vfs::new();
        let mut parts = body.split('\0');
        while let Some(path) = parts.next() {
            if path.is_empty() {
                break;
            }
            let text = parts.next().ok_or_else(|| format!("bundle entry `{path}` has no content"))?;
            vfs.insert(path, text);
        }
        Ok(vfs)
    }

    /// Hash of all paths and contents; changes whenever any file changes.
    pub fn content_hash(&self) -> u64 {
        let mut h = Hasher64::new();
        for (path, text) in &self.files {
            h.str(path).str(text);
        }
        h.finish()
    }
}

fn walk(root: &Path, dir: &Path, vfs: &mut Vfs) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name.starts_with('.') || name == "target" || name == "out" || name == "node_modules" {
                continue;
            }
            walk(root, &path, vfs)?;
        } else if name.ends_with(".toml") {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let rel = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            vfs.insert(rel, std::fs::read_to_string(&path)?);
        }
    }
    Ok(())
}

/// Parse and validate one file against a file-level spec.
fn parse_checked(
    vfs: &Vfs,
    path: &str,
    spec: &'static schema::Spec,
    diags: &mut Vec<Diagnostic>,
) -> Option<(SourceFile, Table, bool)> {
    let text = vfs.get(path)?;
    let src = SourceFile::new(path, text);
    let root = match parse_toml(&src) {
        Ok(t) => t,
        Err(d) => {
            diags.push(d);
            return None;
        }
    };
    let ok = Checker { src: &src, diags }.table(&root, spec, Span::new(0, 0));
    Some((src, root, ok))
}

/// Load `slate.toml`.
pub fn load_project(vfs: &Vfs) -> (Option<ProjectDef>, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    if !vfs.contains(PROJECT_FILE) {
        diags.push(
            Diagnostic::new(&codes::PROJECT_NOT_FOUND, "no slate.toml in this folder or any parent")
                .hint("run inside a project, pass the project folder, or create one with `slate new <path>`"),
        );
        return (None, diags);
    }
    let Some((src, root, ok)) = parse_checked(vfs, PROJECT_FILE, &schema::PROJECT_FILE, &mut diags) else {
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
        let running = slate_core::ENGINE_VERSION;
        if !(running == engine || running.starts_with(&format!("{engine}."))) {
            diags.push(
                Diagnostic::new(
                    &codes::ENGINE_MISMATCH,
                    format!("project targets slate {engine}, but this is slate {running}"),
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

/// Loads scenes, resolving prefabs (each prefab file is parsed and reported
/// once, however many entities use it).
pub struct SceneLoader<'a> {
    vfs: &'a Vfs,
    prefabs: BTreeMap<String, Option<Table>>,
    pub diags: Vec<Diagnostic>,
}

impl<'a> SceneLoader<'a> {
    pub fn new(vfs: &'a Vfs) -> SceneLoader<'a> {
        SceneLoader { vfs, prefabs: BTreeMap::new(), diags: Vec::new() }
    }

    /// Names of all prefab files, e.g. `wall`, `props/tree`.
    pub fn prefab_names(&self) -> Vec<String> {
        self.vfs
            .paths_under("prefabs", ".toml")
            .map(|p| p["prefabs/".len()..p.len() - ".toml".len()].to_string())
            .collect()
    }

    /// Load (once) and validate a prefab. `None` if missing or invalid.
    pub fn prefab(&mut self, name: &str) -> Option<Table> {
        if let Some(cached) = self.prefabs.get(name) {
            return cached.clone();
        }
        let path = format!("prefabs/{name}.toml");
        let loaded = match parse_checked(self.vfs, &path, &schema::PREFAB_FILE, &mut self.diags) {
            Some((_, table, true)) => Some(table),
            _ => None,
        };
        self.prefabs.insert(name.to_string(), loaded.clone());
        loaded
    }

    pub fn prefab_exists(&self, name: &str) -> bool {
        self.vfs.contains(&format!("prefabs/{name}.toml"))
    }

    /// Load a scene. Returns `None` if the file is missing or has errors;
    /// diagnostics accumulate in `self.diags`.
    pub fn load_scene(&mut self, path: &str) -> Option<SceneDef> {
        let start = self.diags.len();
        let (src, root, ok) = parse_checked(self.vfs, path, &schema::SCENE_FILE, &mut self.diags)?;
        let empty = Table::default();
        let meta = Fields::new(root.table("scene").unwrap_or(&empty), &schema::SCENE);
        let ground = root.table("ground").filter(|g| g.has("size")).map(defs::decode_ground);
        let camera = defs::decode_camera(root.table("camera"));

        // Entities.
        let mut entities = Vec::new();
        let mut names: BTreeMap<String, usize> = BTreeMap::new();
        for node in root.array("entity").unwrap_or(&[]) {
            let Some(t) = node.as_table() else { continue };
            let base = match t.str("prefab") {
                Some(name) if self.prefab_exists(name) => match self.prefab(name) {
                    Some(p) => p,
                    None => continue, // prefab file has errors, already reported
                },
                Some(name) => {
                    let names = self.prefab_names();
                    let hint = did_you_mean(name, names.iter().map(String::as_str))
                        .unwrap_or_else(|| format!("create prefabs/{name}.toml, or remove `prefab`"));
                    self.diags.push(
                        Diagnostic::new(&codes::UNKNOWN_PREFAB, format!("unknown prefab `{name}`"))
                            .at(&src, t.value_span("prefab", node.span))
                            .hint(hint),
                    );
                    continue;
                }
                None => Table::default(),
            };
            let merged = base.merged_with(t);
            let origin = Origin { file: path.to_string(), line: src.locate(node.span.start).0, span: node.span };
            let z = t.get("pos").and_then(schema::node_pos).map_or(0.0, |p| p[2]);
            if let Some(cells_node) = t.get("cells") {
                if let Some(name) = t.str("name") {
                    self.diags.push(
                        Diagnostic::new(
                            &codes::NAME_WITH_CELLS,
                            format!("entity `{name}` has both `name` and `cells`"),
                        )
                        .at(&src, t.key_span("name", node.span))
                        .hint("remove `name`, or use one [[entity]] with `pos` per named entity"),
                    );
                    continue;
                }
                for (i, c) in defs::cells_of(cells_node).into_iter().enumerate() {
                    let span = cells_node.as_array().and_then(|a| a.get(i)).map_or(cells_node.span, |n| n.span);
                    let origin = Origin { span, line: src.locate(span.start).0, ..origin.clone() };
                    entities.push(defs::decode_entity(&merged, [c[0] as f32, c[1] as f32, z], origin));
                }
            } else {
                let pos = Fields::new(t, &schema::ENTITY).opt_pos("pos").unwrap_or([0.0, 0.0, 0.0]);
                let def = defs::decode_entity(&merged, pos, origin);
                if let Some(name) = &def.name {
                    if let Some(&first_line) = names.get(name) {
                        self.diags.push(
                            Diagnostic::new(&codes::DUPLICATE_NAME, format!("entity name `{name}` is used twice"))
                                .at(&src, t.value_span("name", node.span))
                                .note(format!("first defined at {path}:{first_line}"))
                                .hint("rename one of them; names must be unique within a scene"),
                        );
                    } else {
                        names.insert(name.clone(), src.locate(t.value_span("name", node.span).start).0);
                    }
                }
                entities.push(def);
            }
        }

        // Connections.
        let mut connections = Vec::new();
        for node in root.array("connect").unwrap_or(&[]) {
            let Some(t) = node.as_table() else { continue };
            let c = defs::decode_connect(t);
            for (key, name) in [("from", &c.from), ("to", &c.to)] {
                if !name.is_empty() && !names.contains_key(name) {
                    let hint = did_you_mean(name, names.keys().map(String::as_str))
                        .unwrap_or_else(|| format!("give an entity `name = \"{name}\"`"));
                    self.diags.push(
                        Diagnostic::new(
                            &codes::UNKNOWN_ENTITY,
                            format!("[[connect]] `{key}` names unknown entity `{name}`"),
                        )
                        .at(&src, t.value_span(key, node.span))
                        .hint(hint),
                    );
                }
            }
            if let Some(from) = entities.iter().find(|e| e.name.as_deref() == Some(c.from.as_str())) {
                let emits: Vec<String> = std::iter::once("tapped".to_string())
                    .chain(from.trigger.iter().flat_map(|tr| [tr.on_enter.clone(), tr.on_exit.clone()]))
                    .collect();
                if !emits.contains(&c.signal) {
                    self.diags.push(
                        Diagnostic::new(
                            &codes::SIGNAL_NEVER_EMITTED,
                            format!("`{}` never emits `{}` (it emits: {})", c.from, c.signal, emits.join(", ")),
                        )
                        .at(&src, t.value_span("signal", node.span))
                        .maybe_hint(did_you_mean(&c.signal, emits.iter().map(String::as_str))),
                    );
                }
            }
            connections.push(c);
        }

        // Static checks on placement.
        let mut solid_cells: BTreeMap<[i32; 2], usize> = BTreeMap::new();
        for (i, e) in entities.iter().enumerate() {
            let cell = e.cell();
            if e.enabled && e.solid {
                if let Some(&j) = solid_cells.get(&cell) {
                    let other = &entities[j];
                    self.diags.push(
                        Diagnostic::new(
                            &codes::SOLID_OVERLAP,
                            format!("two solid entities on cell [{}, {}]", cell[0], cell[1]),
                        )
                        .at(&src, e.origin.span)
                        .note(format!("the other one is at {}:{}", other.origin.file, other.origin.line)),
                    );
                } else {
                    solid_cells.insert(cell, i);
                }
            }
            if let Some(g) = &ground
                && (cell[0] < 0 || cell[1] < 0 || cell[0] >= g.size[0] || cell[1] >= g.size[1])
            {
                self.diags.push(
                    Diagnostic::new(
                        &codes::OUTSIDE_GROUND,
                        format!(
                            "{} is on cell [{}, {}], outside the {}x{} ground",
                            e.name.as_deref().map_or("entity".to_string(), |n| format!("`{n}`")),
                            cell[0],
                            cell[1],
                            g.size[0],
                            g.size[1]
                        ),
                    )
                    .at(&src, e.origin.span),
                );
            }
        }

        let failed = !ok || self.diags[start..].iter().any(Diagnostic::is_error);
        if failed {
            return None;
        }
        let file_stem = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".toml").to_string();
        Some(SceneDef {
            path: path.to_string(),
            name: meta.opt_str("name").unwrap_or(file_stem),
            doc: meta.opt_str("doc"),
            seed: meta.opt_int("seed").map(|s| s as u64),
            camera,
            ground,
            entities,
            connections,
        })
    }
}

/// Load a playtest. Returns `None` if it has errors. Scene references are
/// checked by [`check_project`].
pub fn load_playtest(vfs: &Vfs, path: &str) -> (Option<PlaytestDef>, Vec<Diagnostic>) {
    let (def, diags) = load_playtest_lenient(vfs, path);
    let failed = diags.iter().any(Diagnostic::is_error);
    (if failed { None } else { def }, diags)
}

/// Like [`load_playtest`], but returns whatever could be decoded even when
/// there are errors, so `check` can report cross-file problems in one pass.
fn load_playtest_lenient(vfs: &Vfs, path: &str) -> (Option<PlaytestDef>, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let Some((src, root, _ok)) = parse_checked(vfs, path, &schema::PLAYTEST_FILE, &mut diags) else {
        if !vfs.contains(path) {
            diags.push(Diagnostic::new(&codes::FILE_NOT_FOUND, format!("playtest `{path}` not found")));
        }
        return (None, diags);
    };
    let empty = Table::default();
    let test_t = root.table("test").unwrap_or(&empty);
    let test = Fields::new(test_t, &schema::TEST);
    let frames = test.int("frames") as u64;

    let mut inputs = Vec::new();
    for node in root.array("input").unwrap_or(&[]) {
        let Some(t) = node.as_table() else { continue };
        let kinds: Vec<&str> = ["press", "hold", "tap"].into_iter().filter(|k| t.has(k)).collect();
        if kinds.len() != 1 {
            let found = if kinds.is_empty() { "none".to_string() } else { kinds.join(" and ") };
            diags.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!("[[input]] needs exactly one of `press`, `hold` or `tap` (found {found})"),
                )
                .at(&src, node.span)
                .hint("split it into separate [[input]] tables"),
            );
            continue;
        }
        if t.has("for") && !t.has("hold") {
            diags.push(
                Diagnostic::new(&codes::INVALID_VALUE, "`for` only applies to `hold`")
                    .at(&src, t.key_span("for", node.span))
                    .hint("use `hold = \"...\"` with `for`, or remove `for`"),
            );
        }
        if let Some(input) = defs::decode_input(t) {
            if input.frame >= frames {
                diags.push(
                    Diagnostic::new(
                        &codes::INPUT_AFTER_END,
                        format!("input at frame {} never happens: the playtest runs frames 0..{frames}", input.frame),
                    )
                    .at(&src, t.value_span("frame", node.span)),
                );
            }
            inputs.push(input);
        }
    }

    let mut expects = Vec::new();
    for node in root.array("expect").unwrap_or(&[]) {
        let Some(t) = node.as_table() else { continue };
        let line = src.locate(node.span.start).0;
        let state_keys: Vec<&str> = ["pos", "enabled", "solid", "props"].into_iter().filter(|k| t.has(k)).collect();
        if !t.has("event") && (!t.has("entity") || state_keys.is_empty()) {
            diags.push(Diagnostic::new(&codes::INVALID_VALUE, "[[expect]] checks nothing").at(&src, node.span).hint(
                "use `entity` with `pos`/`enabled`/`solid`/`props`, or `event` (optionally with `entity`/`signal`)",
            ));
            continue;
        }
        if t.has("event") && !state_keys.is_empty() {
            diags.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!("[[expect]] mixes `event` with `{}`", state_keys.join("`, `")),
                )
                .at(&src, node.span)
                .hint("split it into two [[expect]] tables"),
            );
            continue;
        }
        if let Some(f) = t.int("frame")
            && f as u64 > frames
        {
            diags.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!("expect at frame {f} is after the last frame ({frames})"),
                )
                .at(&src, t.value_span("frame", node.span)),
            );
        }
        expects.push(defs::decode_expect(t, node.span, line));
    }

    let mut shots = Vec::new();
    for node in root.array("shot").unwrap_or(&[]) {
        let Some(t) = node.as_table() else { continue };
        shots.push(defs::decode_shot(t, src.locate(node.span.start).0));
    }

    let name = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".toml").to_string();
    let def = PlaytestDef {
        path: path.to_string(),
        name,
        doc: test.opt_str("doc"),
        scene: test.opt_str("scene"),
        scene_span: test_t.value_span("scene", Span::default()),
        frames,
        seed: test.opt_int("seed").map(|s| s as u64),
        inputs,
        expects,
        shots,
        src,
    };
    (Some(def), diags)
}

/// Result of `slate check`.
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

/// Validate every file of a project without running anything.
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
        let names: Vec<&str> = scene.entities.iter().filter_map(|e| e.name.as_deref()).collect();
        for exp in &def.expects {
            if let Some(name) = &exp.entity
                && !names.contains(&name.as_str())
            {
                let hint = did_you_mean(name, names.iter().copied())
                    .unwrap_or_else(|| format!("{scene_path} has no entity named `{name}`"));
                diagnostics.push(
                    Diagnostic::new(&codes::UNKNOWN_ENTITY, format!("[[expect]] names unknown entity `{name}`"))
                        .at(&def.src, exp.span)
                        .hint(hint),
                );
            }
        }
    }

    diagnostics.extend(std::mem::take(&mut loader.diags));
    diag::sort(&mut diagnostics);
    diagnostics.dedup();
    CheckReport { project, scenes: scene_paths.into_iter().collect(), prefabs, playtests, diagnostics }
}
