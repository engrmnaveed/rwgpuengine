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

use crate::defs::{
    self, CondTest, ConnectAction, ConnectFrom, EntityDef, GroundDef, LabelPart, Origin, PlaytestDef, ProjectDef,
    SceneDef,
};
use crate::node::{Node, Table, parse_toml};
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
        let ground = self.load_ground(&root, &src);
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
                    let def = defs::decode_entity(&merged, [c[0] as f32, c[1] as f32, z], origin);
                    if i == 0 {
                        check_entity(&def, t, node.span, &src, &mut self.diags);
                        self.check_counts(&def, t, node.span, &src);
                    }
                    entities.push(def);
                }
            } else {
                let pos = Fields::new(t, &schema::ENTITY).opt_pos("pos").unwrap_or([0.0, 0.0, 0.0]);
                let def = defs::decode_entity(&merged, pos, origin);
                check_entity(&def, t, node.span, &src, &mut self.diags);
                self.check_counts(&def, t, node.span, &src);
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
        let connect_nodes: Vec<(&Node, &Table)> =
            root.array("connect").unwrap_or(&[]).iter().filter_map(|n| Some((n, n.as_table()?))).collect();
        let decoded: Vec<defs::ConnectDef> = connect_nodes.iter().map(|(_, t)| defs::decode_connect(t)).collect();
        // Who emits `changed` (a connection changes their props) and which
        // prefabs get spawned (their copies emit `spawned`).
        let mut changed_names: BTreeSet<String> = BTreeSet::new();
        let mut changed_prefabs: BTreeSet<String> = BTreeSet::new();
        for (c, a) in decoded.iter().flat_map(|c| c.actions.iter().map(move |a| (c, a))) {
            // Paying a `cost` changes the payer's prop.
            if let Some(cost) = &a.cost {
                changed_names.insert(cost.entity.clone());
            }
            if !a.action.changes_prop() {
                continue;
            }
            match (a.to.as_deref(), &c.from) {
                (Some("self"), ConnectFrom::Entity(n)) => {
                    changed_names.insert(n.clone());
                }
                (Some("self"), ConnectFrom::Prefab(p)) => {
                    changed_prefabs.insert(p.clone());
                }
                (Some(n), _) => {
                    changed_names.insert(n.to_string());
                }
                _ => {}
            }
        }
        let is_spawn = |a: &&defs::ActionDef| a.action == ConnectAction::Spawn;
        let spawned: BTreeSet<String> =
            decoded.iter().flat_map(|c| c.actions.iter().filter(is_spawn)).filter_map(|a| a.prefab.clone()).collect();
        // Sources of spawns: they emit `refused` when a placement fails.
        let spawn_sources: Vec<ConnectFrom> =
            decoded.iter().filter(|c| c.actions.iter().any(|a| is_spawn(&a))).map(|c| c.from.clone()).collect();
        let refuses = |from: &ConnectFrom| spawn_sources.contains(from);
        // Prefabs that `spawn` places (one each, in first-use order).
        let mut templates: Vec<EntityDef> = Vec::new();
        let mut connections = Vec::new();
        for (&(node, t), c) in connect_nodes.iter().zip(decoded) {
            let span = node.span;
            let unknown_entity = |key: &str, name: &str| {
                let hint = did_you_mean(name, names.keys().map(String::as_str))
                    .unwrap_or_else(|| format!("give an entity `name = \"{name}\"`"));
                Diagnostic::new(&codes::UNKNOWN_ENTITY, format!("[[connect]] `{key}` names unknown entity `{name}`"))
                    .at(&src, t.value_span(key, span))
                    .hint(hint)
            };
            // The source: one entity, every copy of a prefab, or the ground.
            let source: Option<EntityDef> = match (t.has("from"), t.has("from_prefab")) {
                (true, true) => {
                    self.diags.push(
                        Diagnostic::new(&codes::INVALID_VALUE, "[[connect]] has both `from` and `from_prefab`")
                            .at(&src, t.key_span("from_prefab", span))
                            .hint("use `from` for one entity, `from_prefab` for every copy of a prefab"),
                    );
                    None
                }
                (false, false) => {
                    self.diags.push(
                        Diagnostic::new(&codes::MISSING_KEY, "[[connect]] needs `from` or `from_prefab`")
                            .at(&src, span)
                            .hint("`from = \"<entity name>\"`, `from = \"ground\"` (taps on the ground) or `from_prefab = \"<prefab>\"`"),
                    );
                    None
                }
                _ => match &c.from {
                    ConnectFrom::Ground => None,
                    ConnectFrom::Entity(name) => match entities.iter().find(|e| e.name.as_deref() == Some(name)) {
                        Some(e) => Some(e.clone()),
                        None => {
                            self.diags.push(unknown_entity("from", name));
                            None
                        }
                    },
                    ConnectFrom::Prefab(p) => self.prefab_def(p, &src, t.value_span("from_prefab", span)),
                },
            };
            // Signals the source can emit (W0203).
            let emits: Option<Vec<String>> = match &c.from {
                ConnectFrom::Ground => {
                    let mut v = vec!["tapped".to_string()];
                    if refuses(&c.from) {
                        v.push("refused".into());
                    }
                    Some(v)
                }
                ConnectFrom::Entity(n) => source.as_ref().map(|e| {
                    let mut v = vec!["tapped".to_string()];
                    v.extend(e.trigger.iter().flat_map(|tr| [tr.on_enter.clone(), tr.on_exit.clone()]));
                    if changed_names.contains(n) || !e.counts.is_empty() {
                        v.push("changed".into());
                    }
                    if refuses(&c.from) {
                        v.push("refused".into());
                    }
                    v
                }),
                ConnectFrom::Prefab(p) => source.as_ref().map(|e| {
                    let mut v = vec!["tapped".to_string()];
                    v.extend(e.trigger.iter().flat_map(|tr| [tr.on_enter.clone(), tr.on_exit.clone()]));
                    let named_copy_changes = entities.iter().any(|x| {
                        x.prefab.as_deref() == Some(p) && x.name.as_ref().is_some_and(|n| changed_names.contains(n))
                    });
                    if changed_prefabs.contains(p) || named_copy_changes || !e.counts.is_empty() {
                        v.push("changed".into());
                    }
                    if spawned.contains(p) {
                        v.push("spawned".into());
                    }
                    let named_copy_refuses = entities.iter().any(|x| {
                        x.prefab.as_deref() == Some(p)
                            && x.name.as_ref().is_some_and(|n| refuses(&ConnectFrom::Entity(n.clone())))
                    });
                    if refuses(&c.from) || named_copy_refuses {
                        v.push("refused".into());
                    }
                    v
                }),
            };
            if let Some(emits) = emits
                && !emits.contains(&c.signal)
            {
                let who = match &c.from {
                    ConnectFrom::Entity(n) => format!("`{n}`"),
                    ConnectFrom::Prefab(p) => format!("prefab `{p}`"),
                    ConnectFrom::Ground => "the ground".to_string(),
                };
                let hint =
                    did_you_mean(&c.signal, emits.iter().map(String::as_str)).or_else(|| match c.signal.as_str() {
                        "changed" => Some(format!(
                            "{who} emits `changed` only when a [[connect]] `add`s to or `set`s one of its props, \
                             or one of its `counts` changes"
                        )),
                        "spawned" => Some(format!("{who} emits `spawned` only for copies made by a `spawn` action")),
                        "refused" => Some(format!(
                            "{who} emits `refused` only when a `spawn` that one of its signals starts cannot be placed"
                        )),
                        _ => None,
                    });
                self.diags.push(
                    Diagnostic::new(
                        &codes::SIGNAL_NEVER_EMITTED,
                        format!("{who} never emits `{}` (it emits: {})", c.signal, emits.join(", ")),
                    )
                    .at(&src, t.value_span("signal", span))
                    .maybe_hint(hint),
                );
            }
            // One action (`action = ...`) or several (`actions = [...]`).
            let form_ok = match (t.has("action"), t.has("actions")) {
                (true, true) => {
                    self.diags.push(
                        Diagnostic::new(&codes::INVALID_VALUE, "[[connect]] has both `action` and `actions`")
                            .at(&src, t.key_span("actions", span))
                            .hint("use `action` for one action, or put every action in `actions = [{ ... }, ...]`"),
                    );
                    false
                }
                (false, false) => {
                    self.diags.push(
                        Diagnostic::new(&codes::MISSING_KEY, "[[connect]] needs `action` (or `actions` for several)")
                            .at(&src, span)
                            .hint(
                                "`action = \"disable\"` with `to = \"...\"`, or \
                                 `actions = [{ action = \"disable\", to = \"...\" }, ...]`",
                            ),
                    );
                    false
                }
                (false, true) => {
                    let mut ok = true;
                    for key in ["to", "prop", "value", "prefab", "cost"].into_iter().filter(|k| t.has(k)) {
                        ok = false;
                        self.diags.push(
                            Diagnostic::new(
                                &codes::INVALID_VALUE,
                                format!("with `actions`, `{key}` goes inside the action it belongs to"),
                            )
                            .at(&src, t.key_span(key, span))
                            .hint(format!("move `{key}` into one of the `actions` items")),
                        );
                    }
                    if c.actions.is_empty() {
                        ok = false;
                        self.diags.push(
                            Diagnostic::new(&codes::INVALID_VALUE, "`actions` is empty")
                                .at(&src, t.value_span("actions", span))
                                .hint("list at least one `{ action = ... }`, or use `action`"),
                        );
                    }
                    ok
                }
                (true, false) => true,
            };
            if form_ok {
                for ((at, aspan), a) in defs::action_tables(t, span).into_iter().zip(&c.actions) {
                    let target =
                        self.check_action(a, &c.from, at, aspan, source.as_ref(), &entities, &src, &mut templates);
                    check_action_logic(a, &c.from, at, aspan, target.as_ref(), &src, &mut self.diags);
                }
            }
            check_when(&c, t, &entities, &src, &mut self.diags);
            connections.push(c);
        }

        // Static checks on placement.
        let mut solid_cells: BTreeMap<[i32; 2], usize> = BTreeMap::new();
        let allows = |e: &defs::EntityDef, code: &str| e.allow.iter().any(|a| a.eq_ignore_ascii_case(code));
        for (i, e) in entities.iter().enumerate() {
            let cells = e.cells();
            if e.enabled && e.blocks() {
                for &cell in &cells {
                    if let Some(&j) = solid_cells.get(&cell) {
                        let other = &entities[j];
                        if allows(e, codes::SOLID_OVERLAP.code) || allows(other, codes::SOLID_OVERLAP.code) {
                            continue;
                        }
                        self.diags.push(
                            Diagnostic::new(
                                &codes::SOLID_OVERLAP,
                                format!("two solid entities on cell [{}, {}]", cell[0], cell[1]),
                            )
                            .at(&src, e.origin.span)
                            .note(format!("the other one is at {}:{}", other.origin.file, other.origin.line)),
                        );
                        break;
                    }
                    solid_cells.insert(cell, i);
                }
            }
            let cell = e.cell();
            if e.enabled
                && e.is_actor()
                && !allows(e, codes::ACTOR_IN_SOLID.code)
                && let Some(wall) =
                    entities.iter().find(|o| o.enabled && o.blocks() && !o.is_actor() && o.cells().contains(&cell))
            {
                let what =
                    |x: &defs::EntityDef| x.name.as_deref().map_or("an entity".to_string(), |n| format!("`{n}`"));
                self.diags.push(
                    Diagnostic::new(
                        &codes::ACTOR_IN_SOLID,
                        format!(
                            "actor {} starts inside solid {} on cell [{}, {}]",
                            what(e),
                            what(wall),
                            cell[0],
                            cell[1]
                        ),
                    )
                    .at(&src, e.origin.span)
                    .note(format!("the solid one is at {}:{}", wall.origin.file, wall.origin.line)),
                );
            }
            if let Some(g) = &ground
                && e.screen_pos.is_none()
                && let Some(out) = cells.iter().find(|c| !g.contains(**c))
                && !allows(e, codes::OUTSIDE_GROUND.code)
            {
                let whose = if out == &cell { String::new() } else { " (part of its footprint)".to_string() };
                self.diags.push(
                    Diagnostic::new(
                        &codes::OUTSIDE_GROUND,
                        format!(
                            "{} is on cell [{}, {}]{whose}, outside the {}x{} ground",
                            e.name.as_deref().map_or("entity".to_string(), |n| format!("`{n}`")),
                            out[0],
                            out[1],
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
            templates,
        })
    }

    /// Checks one action's target, or its prefab for `spawn` (recorded as a
    /// spawn template). Returns the target's definition.
    #[allow(clippy::too_many_arguments)]
    fn check_action(
        &mut self,
        a: &defs::ActionDef,
        from: &ConnectFrom,
        t: &Table,
        span: Span,
        source: Option<&EntityDef>,
        entities: &[EntityDef],
        src: &SourceFile,
        templates: &mut Vec<EntityDef>,
    ) -> Option<EntityDef> {
        let mut target: Option<EntityDef> = None;
        if a.action == ConnectAction::Spawn {
            if t.has("to") {
                self.diags.push(
                    Diagnostic::new(&codes::INVALID_VALUE, "`spawn` has no target: it places a new entity")
                        .at(src, t.key_span("to", span))
                        .hint("remove `to`; the new copy goes on the tapped cell or the emitter's cell"),
                );
            }
            match &a.prefab {
                None => self.diags.push(
                    Diagnostic::new(&codes::MISSING_KEY, "`action = \"spawn\"` needs `prefab`")
                        .at(src, t.value_span("action", span))
                        .hint("add `prefab = \"...\"`: the prefab file (prefabs/<name>.toml) to place"),
                ),
                Some(p) => {
                    let at = t.value_span("prefab", span);
                    if let Some(def) = self.prefab_def(p, src, at) {
                        check_entity(&def, &Table::default(), at, src, &mut self.diags);
                        self.check_counts(&def, &Table::default(), at, src);
                        if def.screen_pos.is_some() {
                            self.diags.push(
                                Diagnostic::new(
                                    &codes::INVALID_VALUE,
                                    format!("prefab `{p}` has `screen_pos`, so it cannot be spawned in the world"),
                                )
                                .at(src, at),
                            );
                        }
                        if !templates.iter().any(|t| t.prefab.as_deref() == Some(p.as_str())) {
                            templates.push(def);
                        }
                    }
                }
            }
            if let Some(cost) = &a.cost {
                check_cost(cost, t, span, entities, src, &mut self.diags);
            }
        } else {
            for key in ["prefab", "cost"].into_iter().filter(|k| t.has(k)) {
                self.diags.push(
                    Diagnostic::new(&codes::INVALID_VALUE, format!("`{key}` only applies to `action = \"spawn\"`"))
                        .at(src, t.key_span(key, span))
                        .hint(format!("remove `{key}`, or use `action = \"spawn\"`")),
                );
            }
            match a.to.as_deref() {
                None => self.diags.push(
                    Diagnostic::new(&codes::MISSING_KEY, format!("`action = \"{}\"` needs `to`", a.action.name()))
                        .at(src, span)
                        .hint("add `to = \"<entity name>\"`, or `to = \"self\"` for the entity that emitted"),
                ),
                Some("self") if *from == ConnectFrom::Ground => self.diags.push(
                    Diagnostic::new(
                        &codes::INVALID_VALUE,
                        "`to = \"self\"` with `from = \"ground\"`: the ground is not an entity",
                    )
                    .at(src, t.value_span("to", span))
                    .hint("name the entity to change, or use `action = \"spawn\"` to place one on the tapped cell"),
                ),
                Some("self") => target = source.cloned(),
                Some(name) => match entities.iter().find(|e| e.name.as_deref() == Some(name)) {
                    Some(e) => target = Some(e.clone()),
                    None => {
                        let names = entities.iter().filter_map(|e| e.name.as_deref());
                        let hint =
                            did_you_mean(name, names).unwrap_or_else(|| format!("give an entity `name = \"{name}\"`"));
                        self.diags.push(
                            Diagnostic::new(
                                &codes::UNKNOWN_ENTITY,
                                format!("[[connect]] `to` names unknown entity `{name}`"),
                            )
                            .at(src, t.value_span("to", span))
                            .hint(hint),
                        );
                    }
                },
            }
        }
        target
    }

    /// Every `counts` entry names an existing prefab (E0201).
    fn check_counts(&mut self, def: &EntityDef, t: &Table, span: Span, src: &SourceFile) {
        for (prop, prefab) in &def.counts {
            if self.prefab_exists(prefab) {
                continue;
            }
            let at = t.table("counts").map_or(span, |c| c.value_span(prop, t.value_span("counts", span)));
            let names = self.prefab_names();
            let hint = did_you_mean(prefab, names.iter().map(String::as_str))
                .unwrap_or_else(|| format!("create prefabs/{prefab}.toml"));
            self.diags.push(
                Diagnostic::new(&codes::UNKNOWN_PREFAB, format!("`counts.{prop}` counts unknown prefab `{prefab}`"))
                    .at(src, at)
                    .hint(hint),
            );
        }
    }

    /// A prefab decoded as an entity at [0, 0, 0], reporting E0201 at `at`
    /// when it does not exist. `None` if missing or invalid.
    fn prefab_def(&mut self, name: &str, src: &SourceFile, at: Span) -> Option<EntityDef> {
        if !self.prefab_exists(name) {
            let names = self.prefab_names();
            let hint = did_you_mean(name, names.iter().map(String::as_str))
                .unwrap_or_else(|| format!("create prefabs/{name}.toml"));
            self.diags.push(
                Diagnostic::new(&codes::UNKNOWN_PREFAB, format!("unknown prefab `{name}`")).at(src, at).hint(hint),
            );
            return None;
        }
        let mut table = self.prefab(name)?;
        table.set_str("prefab", name);
        let origin = Origin { file: src.path.clone(), line: src.locate(at.start).0, span: at };
        Some(defs::decode_entity(&table, [0.0, 0.0, 0.0], origin))
    }

    /// `[ground]` with an optional terrain `map`. Problems are reported; the
    /// ground is still returned when it has a usable size.
    fn load_ground(&mut self, root: &Table, src: &SourceFile) -> Option<GroundDef> {
        let g = root.table("ground")?;
        let gspan = root.value_span("ground", Span::default());
        let mut terrains: Vec<defs::TerrainDef> = Vec::new();
        for node in root.array("terrain").unwrap_or(&[]) {
            let Some(t) = node.as_table() else { continue };
            let raw = t.str("char").unwrap_or_default();
            let at = t.value_span("char", node.span);
            if raw.chars().count() != 1 || raw.chars().all(char::is_whitespace) {
                self.diags.push(
                    Diagnostic::new(
                        &codes::INVALID_VALUE,
                        format!("[[terrain]] `char` must be one visible character, found {raw:?}"),
                    )
                    .at(src, at)
                    .hint("for example `char = \"g\"` for grass or `char = \"~\"` for water"),
                );
                continue;
            }
            let def = defs::decode_terrain(t);
            if let Some(first) = terrains.iter().find(|x| x.ch == def.ch) {
                self.diags.push(
                    Diagnostic::new(
                        &codes::INVALID_VALUE,
                        format!("terrain char {raw:?} is declared twice (`{}` and `{}`)", first.name, def.name),
                    )
                    .at(src, at)
                    .hint("give each terrain its own character"),
                );
                continue;
            }
            terrains.push(def);
        }
        let Some(map_node) = g.get("map") else {
            if !g.has("size") {
                self.diags.push(
                    Diagnostic::new(&codes::MISSING_KEY, "[ground] needs `size` or `map`")
                        .at(src, gspan)
                        .hint("`size = [8, 8]` for plain ground, or `map = [\"...\", ...]` with [[terrain]] tables"),
                );
                return None;
            }
            return Some(defs::decode_plain_ground(g));
        };
        let rows: Vec<(String, Span)> =
            map_node.as_array().unwrap_or(&[]).iter().filter_map(|n| Some((n.as_str()?.to_string(), n.span))).collect();
        let width = rows.first().map_or(0, |(r, _)| r.chars().count());
        let mut ok = width > 0;
        if width == 0 {
            self.diags
                .push(Diagnostic::new(&codes::INVALID_VALUE, "`map` rows must not be empty").at(src, map_node.span));
        }
        let declared = || {
            if terrains.is_empty() {
                "no terrain is declared yet".to_string()
            } else {
                let list: Vec<String> =
                    terrains.iter().map(|t| format!("{:?} ({})", t.ch.to_string(), t.name)).collect();
                format!("declared: {}", list.join(", "))
            }
        };
        let mut cells = Vec::with_capacity(width * rows.len());
        for (y, (row, at)) in rows.iter().enumerate() {
            let n = row.chars().count();
            if n != width {
                ok = false;
                self.diags.push(
                    Diagnostic::new(
                        &codes::INVALID_VALUE,
                        format!("`map` row {y} has {n} characters, but row 0 has {width}"),
                    )
                    .at(src, *at)
                    .hint("every row must have the same length: the map is a rectangle"),
                );
                continue;
            }
            for (x, ch) in row.chars().enumerate() {
                match terrains.iter().position(|t| t.ch == ch) {
                    Some(i) => cells.push(i as u8),
                    None => {
                        ok = false;
                        let col = Span::new(at.start + 1 + row.char_indices().nth(x).map_or(0, |(b, _)| b), 0);
                        let col = Span::new(col.start, col.start + ch.len_utf8());
                        self.diags.push(
                            Diagnostic::new(
                                &codes::UNKNOWN_TERRAIN,
                                format!("`map` cell [{x}, {y}] is {:?}, which no [[terrain]] declares", ch.to_string()),
                            )
                            .at(src, col)
                            .note(declared())
                            .hint(format!(
                                "add [[terrain]] with `char = {:?}`, or use a declared character",
                                ch.to_string()
                            )),
                        );
                        break;
                    }
                }
            }
        }
        let size = [width as i32, rows.len() as i32];
        if g.has("size") {
            let given = Fields::new(g, &schema::GROUND).cell("size");
            if given != size {
                self.diags.push(
                    Diagnostic::new(
                        &codes::INVALID_VALUE,
                        format!(
                            "`size = [{}, {}]` does not match the map, which is {} x {}",
                            given[0], given[1], size[0], size[1]
                        ),
                    )
                    .at(src, g.value_span("size", gspan))
                    .hint("remove `size`: a map sets the ground's size"),
                );
            }
        }
        for key in ["color", "alt_color"] {
            if g.has(key) {
                self.diags.push(
                    Diagnostic::new(&codes::INVALID_VALUE, format!("`{key}` does not apply with `map`"))
                        .at(src, g.key_span(key, gspan))
                        .hint("set colors on the [[terrain]] tables"),
                );
            }
        }
        if !ok || terrains.len() > 255 {
            return None;
        }
        Some(GroundDef { size, terrains, cells, has_map: true })
    }
}

/// Checks on one entity that need its merged definition: label templates and
/// screen entities.
fn check_entity(def: &EntityDef, t: &Table, span: Span, src: &SourceFile, diags: &mut Vec<Diagnostic>) {
    let who = def.name.as_deref().map_or("this entity".to_string(), |n| format!("`{n}`"));
    if let Some(name) = def.name.as_deref()
        && schema::RESERVED_NAMES.contains(&name)
    {
        let means = if name == "ground" {
            "[[connect]] `from = \"ground\"` means taps on the ground"
        } else {
            "[[connect]] `to = \"self\"` means the entity that emitted"
        };
        diags.push(
            Diagnostic::new(&codes::INVALID_VALUE, format!("`{name}` is a reserved name: {means}"))
                .at(src, t.value_span("name", span))
                .hint("choose another name"),
        );
    }
    for (k, prefab) in &def.counts {
        if def.props.iter().filter(|(p, _)| p == k).count() > 1 {
            diags.push(
                Diagnostic::new(&codes::INVALID_VALUE, format!("{who} declares `{k}` in both `props` and `counts`"))
                    .at(src, t.key_span("counts", span))
                    .hint(format!(
                        "remove `{k}` from `props`: `counts` makes it an integer prop (enabled `{prefab}` copies)"
                    )),
            );
        }
    }
    if def.footprint != [1, 1] {
        let fixed: Vec<&str> = [
            ("mover", def.mover.is_some()),
            ("patrol", def.patrol.is_some()),
            ("wander", def.wander.is_some()),
            ("pushable", def.pushable),
            ("screen_pos", def.screen_pos.is_some()),
        ]
        .into_iter()
        .filter(|(_, has)| *has)
        .map(|(k, _)| k)
        .collect();
        if !fixed.is_empty() {
            diags.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!(
                        "{who} has `footprint = [{}, {}]`; only things that stay put can cover several cells, so it \
                         cannot have `{}`",
                        def.footprint[0],
                        def.footprint[1],
                        fixed.join("`, `")
                    ),
                )
                .at(src, t.key_span("footprint", span))
                .hint("remove `footprint`, or the behavior"),
            );
        }
    }
    if let Some(label) = &def.label {
        let at = t.table("label").map_or(span, |l| l.value_span("text", t.value_span("label", span)));
        match defs::parse_label(&label.text) {
            Err(why) => diags.push(
                Diagnostic::new(&codes::INVALID_VALUE, format!("label text {:?}: {why}", label.text)).at(src, at),
            ),
            Ok(parts) => {
                for part in parts {
                    if let LabelPart::Prop(name) = part
                        && def.prop(&name).is_none()
                    {
                        diags.push(unknown_prop(def, &name, &format!("label placeholder `{{{name}}}`"), src, at));
                    }
                }
            }
        }
        let odd: Vec<String> = {
            let mut v: Vec<char> = label.text.chars().filter(|c| !defs::font::supports(*c)).collect();
            v.sort_unstable();
            v.dedup();
            v.into_iter().map(|c| format!("{c:?}")).collect()
        };
        if !odd.is_empty() && !def.allow.iter().any(|a| a.eq_ignore_ascii_case(codes::UNSUPPORTED_CHAR.code)) {
            diags.push(
                Diagnostic::new(
                    &codes::UNSUPPORTED_CHAR,
                    format!("label of {who} has characters the font cannot draw: {}", odd.join(", ")),
                )
                .at(src, at)
                .hint("they are drawn as a hollow box; the font has printable ASCII only"),
            );
        }
    }
    if def.pushable {
        let moving: Vec<&str> =
            [("mover", def.mover.is_some()), ("patrol", def.patrol.is_some()), ("wander", def.wander.is_some())]
                .into_iter()
                .filter(|(_, has)| *has)
                .map(|(k, _)| k)
                .collect();
        if !moving.is_empty() {
            diags.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!(
                        "{who} is `pushable`, so it only moves when pushed; it cannot have `{}`",
                        moving.join("`, `")
                    ),
                )
                .at(src, t.key_span("pushable", span))
                .hint("remove the behavior, or remove `pushable`"),
            );
        }
    }
    if def.screen_pos.is_some() {
        let world_only: Vec<&str> = [
            ("tile", def.tile.is_some()),
            ("block", def.block.is_some()),
            ("mover", def.mover.is_some()),
            ("patrol", def.patrol.is_some()),
            ("wander", def.wander.is_some()),
            ("trigger", def.trigger.is_some()),
            ("solid", def.solid),
            ("pushable", def.pushable),
        ]
        .into_iter()
        .filter(|(_, has)| *has)
        .map(|(k, _)| k)
        .collect();
        if !world_only.is_empty() {
            diags.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!(
                        "{who} has `screen_pos`, so it is drawn on the screen, not in the world; it cannot have `{}`",
                        world_only.join("`, `")
                    ),
                )
                .at(src, t.key_span("screen_pos", span))
                .hint("screen entities can have `sprite` and `label`; put world parts on a separate entity"),
            );
        }
    }
}

/// E0206 for `name` on `def`, suggesting a close prop or how to declare it.
fn unknown_prop(def: &EntityDef, name: &str, what: &str, src: &SourceFile, at: Span) -> Diagnostic {
    let who = def.name.as_deref().unwrap_or("the entity");
    let hint = did_you_mean(name, def.props.iter().map(|(k, _)| k.as_str()))
        .unwrap_or_else(|| format!("declare it with a starting value on `{who}`: `props = {{ {name} = 0 }}`"));
    Diagnostic::new(&codes::UNKNOWN_PROP, format!("{what} names prop `{name}`, but `{who}` has no such prop"))
        .at(src, at)
        .hint(hint)
}

/// A spawn's `cost`: an existing entity, a number prop it declares (not a
/// count), and an amount above 0 that fits the prop's type.
fn check_cost(
    cost: &defs::CostDef,
    t: &Table,
    span: Span,
    entities: &[EntityDef],
    src: &SourceFile,
    diags: &mut Vec<Diagnostic>,
) {
    let table = t.table("cost");
    let at = |key: &str| table.map_or(span, |c| c.value_span(key, t.value_span("cost", span)));
    let Some(payer) = entities.iter().find(|e| e.name.as_deref() == Some(cost.entity.as_str())) else {
        let names = entities.iter().filter_map(|e| e.name.as_deref());
        let hint = did_you_mean(&cost.entity, names)
            .unwrap_or_else(|| format!("give the paying entity `name = \"{}\"`", cost.entity));
        diags.push(
            Diagnostic::new(&codes::UNKNOWN_ENTITY, format!("`cost` names unknown entity `{}`", cost.entity))
                .at(src, at("entity"))
                .hint(hint),
        );
        return;
    };
    let prop = cost.prop.as_str();
    let Some(current) = payer.prop(prop) else {
        diags.push(unknown_prop(payer, prop, "`cost`", src, at("prop")));
        return;
    };
    if let Some(prefab) = payer.counted(prop) {
        diags.push(
            Diagnostic::new(
                &codes::INVALID_VALUE,
                format!("`cost` cannot pay from `{}`.{prop}: it counts `{prefab}` copies", cost.entity),
            )
            .at(src, at("prop"))
            .hint("pay from a prop declared in `props`, e.g. `props = { coins = 10 }`"),
        );
        return;
    }
    if current.as_num().is_none() {
        diags.push(
            Diagnostic::new(
                &codes::PROP_TYPE,
                format!("`cost` needs a number prop, but `{}`.{prop} is {}", cost.entity, current.type_name()),
            )
            .at(src, at("prop")),
        );
        return;
    }
    if !table.is_some_and(|c| c.has("amount")) {
        return; // already reported as missing
    }
    if cost.amount.as_num().is_none_or(|n| n <= 0.0) {
        diags.push(
            Diagnostic::new(
                &codes::INVALID_VALUE,
                format!("`cost` amount must be above 0, found {}", cost.amount.literal()),
            )
            .at(src, at("amount"))
            .hint("for something free, leave `cost` out"),
        );
    } else if !current.accepts(&cost.amount) {
        diags.push(
            Diagnostic::new(
                &codes::PROP_TYPE,
                format!(
                    "`cost` amount {} cannot be paid from `{}`.{prop}, which is {}",
                    cost.amount.literal(),
                    cost.entity,
                    current.type_name()
                ),
            )
            .at(src, at("amount"))
            .hint("integer props pay whole numbers; declare `0.0` for decimals"),
        );
    }
}

/// Checks on one `add`/`set` action that need its target's definition.
/// `t` is the table holding the action (the [[connect]] or an `actions` item).
fn check_action_logic(
    c: &defs::ActionDef,
    from: &ConnectFrom,
    t: &Table,
    span: Span,
    target: Option<&EntityDef>,
    src: &SourceFile,
    diags: &mut Vec<Diagnostic>,
) {
    let action = c.action.name();
    // How the target is named in messages: `hud`, or the prefab for `self`.
    let to_label = match (c.to.as_deref(), from) {
        (Some("self"), ConnectFrom::Prefab(p)) => format!("{p} (self)"),
        (Some("self"), ConnectFrom::Entity(n)) => n.clone(),
        (Some(n), _) => n.to_string(),
        (None, _) => "the target".to_string(),
    };
    if c.action.changes_prop() {
        match &c.prop {
            None => diags.push(
                Diagnostic::new(&codes::MISSING_KEY, format!("`action = \"{action}\"` needs `prop`"))
                    .at(src, t.value_span("action", span))
                    .hint(format!("add `prop = \"...\"`: a prop declared in `{to_label}`'s props")),
            ),
            Some(prop) => {
                if let Some(target) = target
                    && let Some(prefab) = target.counted(prop)
                {
                    diags.push(
                        Diagnostic::new(
                            &codes::INVALID_VALUE,
                            format!(
                                "`{action}` cannot change `{to_label}`.{prop}: it is a `counts` prop (the number of \
                                     enabled `{prefab}` copies), kept up to date by the engine"
                            ),
                        )
                        .at(src, t.value_span("prop", span))
                        .hint(format!(
                            "remove this [[connect]]; spawning, disabling or enabling a `{prefab}` changes it"
                        )),
                    );
                } else if let Some(target) = target {
                    match target.prop(prop) {
                        None => diags.push(unknown_prop(
                            target,
                            prop,
                            &format!("`{action}`"),
                            src,
                            t.value_span("prop", span),
                        )),
                        Some(current) => {
                            let value_at = t.value_span("value", span);
                            match (c.action, &c.value) {
                                (ConnectAction::Add, _) if current.as_num().is_none() => diags.push(
                                    Diagnostic::new(
                                        &codes::PROP_TYPE,
                                        format!(
                                            "`add` needs a number prop, but `{}`.{prop} is {}",
                                            to_label,
                                            current.type_name()
                                        ),
                                    )
                                    .at(src, t.value_span("prop", span))
                                    .hint("use `set` for text and flags"),
                                ),
                                (ConnectAction::Add, Some(v)) if !current.accepts(v) => diags.push(
                                    Diagnostic::new(
                                        &codes::PROP_TYPE,
                                        format!(
                                            "`value = {}` cannot be added to `{}`.{prop}, which is {}",
                                            v.literal(),
                                            to_label,
                                            current.type_name()
                                        ),
                                    )
                                    .at(src, value_at)
                                    .hint("integer props count in whole numbers; declare `0.0` for decimals"),
                                ),
                                (ConnectAction::Set, None) => diags.push(
                                    Diagnostic::new(&codes::MISSING_KEY, "`action = \"set\"` needs `value`")
                                        .at(src, t.value_span("action", span))
                                        .hint(format!("add `value = ...` ({})", current.type_name())),
                                ),
                                (ConnectAction::Set, Some(v)) if !current.accepts(v) => diags.push(
                                    Diagnostic::new(
                                        &codes::PROP_TYPE,
                                        format!(
                                            "`value = {}` is {}, but `{}`.{prop} is {}",
                                            v.literal(),
                                            v.type_name(),
                                            to_label,
                                            current.type_name()
                                        ),
                                    )
                                    .at(src, value_at),
                                ),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    } else {
        for key in ["prop", "value"] {
            if t.has(key) {
                diags.push(
                    Diagnostic::new(&codes::INVALID_VALUE, format!("`{key}` only applies to `add` and `set`"))
                        .at(src, t.key_span(key, span))
                        .hint(format!("remove `{key}`, or use `action = \"add\"` / `\"set\"`")),
                );
            }
        }
    }
}

/// Checks on a [[connect]]'s `when` list (once per connection).
fn check_when(c: &defs::ConnectDef, t: &Table, entities: &[EntityDef], src: &SourceFile, diags: &mut Vec<Diagnostic>) {
    let by_name = |n: &str| entities.iter().find(|e| e.name.as_deref() == Some(n));
    for item in t.get("when").and_then(Node::as_array).unwrap_or(&[]) {
        let Some(w) = item.as_table() else { continue };
        let cond = defs::decode_condition(w);
        if w.has("reason") {
            let others: Vec<&str> = ["entity", "occupied", "enabled", "prop", "equals", "at_least", "at_most"]
                .into_iter()
                .filter(|k| w.has(k))
                .collect();
            if !others.is_empty() {
                diags.push(
                    Diagnostic::new(
                        &codes::INVALID_VALUE,
                        format!("a `reason` condition stands alone (found `{}` too)", others.join("`, `")),
                    )
                    .at(src, item.span)
                    .hint("write it as its own item: when = [{ reason = \"terrain\" }, { entity = ... }]"),
                );
            }
            if c.signal != "refused" {
                diags.push(
                    Diagnostic::new(
                        &codes::INVALID_VALUE,
                        format!(
                            "`reason` only applies with `signal = \"refused\"`; this [[connect]] listens to `{}`",
                            c.signal
                        ),
                    )
                    .at(src, w.key_span("reason", item.span))
                    .hint("listen to `refused` (emitted when a spawn cannot be placed), or remove `reason`"),
                );
            }
            continue;
        }
        if cond.entity.is_empty() {
            diags.push(
                Diagnostic::new(&codes::MISSING_KEY, "a `when` condition needs `entity`")
                    .at(src, item.span)
                    .hint("`{ entity = \"<name>\", ... }`; only a `reason` condition (on `refused`) has no entity"),
            );
            continue;
        }
        let Some(target) = by_name(&cond.entity) else {
            if !cond.entity.is_empty() {
                let names = entities.iter().filter_map(|e| e.name.as_deref());
                let hint = did_you_mean(&cond.entity, names)
                    .unwrap_or_else(|| format!("give an entity `name = \"{}\"`", cond.entity));
                diags.push(
                    Diagnostic::new(
                        &codes::UNKNOWN_ENTITY,
                        format!("`when` condition names unknown entity `{}`", cond.entity),
                    )
                    .at(src, w.value_span("entity", item.span))
                    .hint(hint),
                );
            }
            continue;
        };
        let kinds: Vec<&str> = ["occupied", "enabled", "prop"].into_iter().filter(|k| w.has(k)).collect();
        if kinds.len() != 1 {
            let found = if kinds.is_empty() { "none".to_string() } else { kinds.join(" and ") };
            diags.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!("a `when` condition needs exactly one of `occupied`, `enabled` or `prop` (found {found})"),
                )
                .at(src, item.span)
                .hint("write one condition per test: when = [{ ... }, { ... }]"),
            );
            continue;
        }
        let compares: Vec<&str> = ["equals", "at_least", "at_most"].into_iter().filter(|k| w.has(k)).collect();
        let CondTest::Prop { prop, equals, at_least, at_most } = &cond.test else {
            for k in compares {
                diags.push(
                    Diagnostic::new(&codes::INVALID_VALUE, format!("`{k}` only applies with `prop`"))
                        .at(src, w.key_span(k, item.span)),
                );
            }
            continue;
        };
        if compares.is_empty() {
            diags.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!("condition on `{}`.{prop} compares nothing", cond.entity),
                )
                .at(src, item.span)
                .hint("add `equals = ...`, `at_least = N` or `at_most = N`"),
            );
            continue;
        }
        let Some(current) = target.prop(prop) else {
            diags.push(unknown_prop(target, prop, "`when` condition", src, w.value_span("prop", item.span)));
            continue;
        };
        if (at_least.is_some() || at_most.is_some()) && current.as_num().is_none() {
            let k = if at_least.is_some() { "at_least" } else { "at_most" };
            diags.push(
                Diagnostic::new(
                    &codes::PROP_TYPE,
                    format!("`{k}` needs a number prop, but `{}`.{prop} is {}", cond.entity, current.type_name()),
                )
                .at(src, w.key_span(k, item.span))
                .hint("use `equals` for text and flags"),
            );
        }
        if let Some(v) = equals
            && !current.accepts(v)
        {
            diags.push(
                Diagnostic::new(
                    &codes::PROP_TYPE,
                    format!(
                        "`equals = {}` is {}, but `{}`.{prop} is {}",
                        v.literal(),
                        v.type_name(),
                        cond.entity,
                        current.type_name()
                    ),
                )
                .at(src, w.value_span("equals", item.span)),
            );
        }
        if let (Some(lo), Some(hi)) = (at_least, at_most)
            && lo > hi
        {
            diags.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!("`at_least = {lo}` is more than `at_most = {hi}`, so this condition never holds"),
                )
                .at(src, w.value_span("at_least", item.span)),
            );
        }
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
        if t.has("count") && !t.has("prefab") {
            diags.push(
                Diagnostic::new(&codes::INVALID_VALUE, "`count` only applies with `prefab`")
                    .at(&src, t.key_span("count", node.span))
                    .hint("add `prefab = \"...\"` to count its copies; count events with `event` and `at_least`/`at_most`"),
            );
            continue;
        }
        if t.has("prefab") && t.has("event") {
            // Events about a prefab: `count`/`pos` belong to copy counts.
            let wrong: Vec<&str> =
                ["count", "pos", "enabled", "solid", "props", "text"].into_iter().filter(|k| t.has(k)).collect();
            if !wrong.is_empty() {
                diags.push(
                    Diagnostic::new(
                        &codes::INVALID_VALUE,
                        format!(
                            "[[expect]] with `event` and `prefab` counts events about the prefab; it cannot use `{}`",
                            wrong.join("`, `")
                        ),
                    )
                    .at(&src, node.span)
                    .hint(
                        "count events with `at_least`/`at_most`; count copies in a separate [[expect]] without `event`",
                    ),
                );
                continue;
            }
        } else if t.has("prefab") {
            let extra: Vec<&str> =
                ["entity", "enabled", "solid", "props", "text"].into_iter().filter(|k| t.has(k)).collect();
            if !extra.is_empty() {
                diags.push(
                    Diagnostic::new(
                        &codes::INVALID_VALUE,
                        format!("[[expect]] with `prefab` counts copies; it cannot use `{}`", extra.join("`, `")),
                    )
                    .at(&src, node.span)
                    .hint("a copy count takes `count` and optionally `pos` (add `event` to count events about the prefab instead)"),
                );
                continue;
            }
            for key in ["other", "at_most", "at_least", "signal", "reason"] {
                if t.has(key) {
                    diags.push(
                        Diagnostic::new(&codes::INVALID_VALUE, format!("`{key}` only applies to event expectations"))
                            .at(&src, t.key_span(key, node.span))
                            .hint("use `count` to check how many copies exist"),
                    );
                }
            }
            expects.push(defs::decode_expect(t, node.span, line));
            continue;
        }
        let state_keys: Vec<&str> =
            ["pos", "enabled", "solid", "props", "text"].into_iter().filter(|k| t.has(k)).collect();
        if !t.has("event") && (!t.has("entity") || state_keys.is_empty()) {
            diags.push(Diagnostic::new(&codes::INVALID_VALUE, "[[expect]] checks nothing").at(&src, node.span).hint(
                "use `entity` with `pos`/`enabled`/`solid`/`props`/`text`, `prefab` with `count`, or `event` (optionally \
                 with `entity`/`signal`)",
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
        for key in ["other", "at_most", "at_least", "signal"] {
            if t.has(key) && !t.has("event") {
                diags.push(
                    Diagnostic::new(&codes::INVALID_VALUE, format!("`{key}` only applies to event expectations"))
                        .at(&src, t.key_span(key, node.span))
                        .hint("add `event = \"...\"`, or remove it"),
                );
            }
        }
        if let (Some(lo), Some(hi)) = (t.int("at_least"), t.int("at_most"))
            && lo > hi
        {
            diags.push(
                Diagnostic::new(&codes::INVALID_VALUE, format!("`at_least = {lo}` is more than `at_most = {hi}`"))
                    .at(&src, t.value_span("at_least", node.span)),
            );
        }
        if t.has("changed") && t.str("event") != Some("action") {
            diags.push(
                Diagnostic::new(&codes::INVALID_VALUE, "`changed` only applies to `event = \"action\"`")
                    .at(&src, t.key_span("changed", node.span))
                    .hint("add `event = \"action\"`, or remove `changed`"),
            );
        }
        if t.has("reason") && t.str("event") != Some("blocked") {
            diags.push(
                Diagnostic::new(&codes::INVALID_VALUE, "`reason` only applies to `event = \"blocked\"`")
                    .at(&src, t.key_span("reason", node.span))
                    .hint("add `event = \"blocked\"`, or remove `reason`"),
            );
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
            if let Some(p) = &exp.prefab
                && !loader.prefab_exists(p)
            {
                let all = loader.prefab_names();
                let hint = did_you_mean(p, all.iter().map(String::as_str))
                    .unwrap_or_else(|| format!("there is no prefabs/{p}.toml"));
                diagnostics.push(
                    Diagnostic::new(&codes::UNKNOWN_PREFAB, format!("[[expect]] counts unknown prefab `{p}`"))
                        .at(&def.src, exp.span)
                        .hint(hint),
                );
            }
            if let Some(name) = &exp.other
                && !names.contains(&name.as_str())
            {
                let hint = did_you_mean(name, names.iter().copied())
                    .unwrap_or_else(|| format!("{scene_path} has no entity named `{name}`"));
                diagnostics.push(
                    Diagnostic::new(
                        &codes::UNKNOWN_ENTITY,
                        format!("[[expect]] `other` names unknown entity `{name}`"),
                    )
                    .at(&def.src, exp.other_span)
                    .hint(hint),
                );
            }
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
            let Some(target) =
                exp.entity.as_deref().and_then(|n| scene.entities.iter().find(|e| e.name.as_deref() == Some(n)))
            else {
                continue;
            };
            let at = Span::new(exp.span.start, exp.span.start);
            for (k, v) in &exp.props {
                match target.prop(k) {
                    None => diagnostics.push(unknown_prop(target, k, "[[expect]] props", &def.src, exp.span)),
                    Some(cur) if !cur.accepts(v) && !v.accepts(cur) => diagnostics.push(
                        Diagnostic::new(
                            &codes::PROP_TYPE,
                            format!(
                                "[[expect]] props.{k} = {} is {}, but `{}`.{k} is {}",
                                v.literal(),
                                v.type_name(),
                                target.name.as_deref().unwrap_or("?"),
                                cur.type_name()
                            ),
                        )
                        .at(&def.src, exp.span),
                    ),
                    Some(_) => {}
                }
            }
            if exp.text.is_some() && target.label.is_none() {
                diagnostics.push(
                    Diagnostic::new(
                        &codes::INVALID_VALUE,
                        format!(
                            "[[expect]] `text` checks a label, but `{}` has no label",
                            target.name.as_deref().unwrap_or("?")
                        ),
                    )
                    .at(&def.src, at)
                    .hint("check `props` instead, or add `label = { text = \"...\" }` to the entity"),
                );
            }
        }
    }

    diagnostics.extend(std::mem::take(&mut loader.diags));
    diag::sort(&mut diagnostics);
    diagnostics.dedup();
    CheckReport { project, scenes: scene_paths.into_iter().collect(), prefabs, playtests, diagnostics }
}
