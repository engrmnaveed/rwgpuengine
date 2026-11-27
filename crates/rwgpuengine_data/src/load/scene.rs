//! A scene through the four stages, and the prefabs it uses.

use std::collections::BTreeMap;
use std::sync::Arc;

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::{Diagnostic, SourceFile, Span};
use rwgpuengine_core::text::did_you_mean;

use super::check;
use super::order::{At, Diags};
use super::parse::{self, Parsed};
use super::resolve::{self, Resolved};
use super::vfs::Vfs;
use crate::defs::{self, EntityDef, Origin, SceneDef, ShapeDef};
use crate::node::Table;
use crate::schema::{self, Fields};

/// A prefab file that loaded: its table, and its shape list, decoded once
/// and shared by every entity made from it.
#[derive(Clone)]
pub(crate) struct Prefab {
    pub table: Table,
    pub shapes: Option<Arc<[ShapeDef]>>,
}

/// Loads scenes, resolving prefabs (each prefab file is parsed and reported
/// once, however many entities use it).
pub struct SceneLoader<'a> {
    vfs: &'a Vfs,
    /// Each prefab asked for so far, by its place in `loaded` (a map of a
    /// kind the loader has already: a new kind of map is 4 KB of the web
    /// module).
    prefabs: BTreeMap<String, usize>,
    /// `None` for a file that is missing or has errors.
    loaded: Vec<Option<Prefab>>,
    pub diags: Vec<Diagnostic>,
}

impl<'a> SceneLoader<'a> {
    pub fn new(vfs: &'a Vfs) -> SceneLoader<'a> {
        SceneLoader { vfs, prefabs: BTreeMap::new(), loaded: Vec::new(), diags: Vec::new() }
    }

    /// Names of all prefab files, e.g. `wall`, `props/tree`.
    pub fn prefab_names(&self) -> Vec<String> {
        prefab_names(self.vfs)
    }

    /// Load (once) and validate a prefab. `None` if missing or invalid.
    pub fn prefab(&mut self, name: &str) -> Option<Table> {
        self.loaded(name).map(|p| p.table.clone())
    }

    /// The prefab, loaded and checked the first time it is asked for: its
    /// file against the schema, and its shape list by its rules.
    fn loaded(&mut self, name: &str) -> Option<&Prefab> {
        let at = match self.prefabs.get(name) {
            Some(at) => *at,
            None => {
                let prefab = self.load(name);
                self.loaded.push(prefab);
                self.prefabs.insert(name.to_string(), self.loaded.len() - 1);
                self.loaded.len() - 1
            }
        };
        self.loaded[at].as_ref()
    }

    fn load(&mut self, name: &str) -> Option<Prefab> {
        let path = format!("prefabs/{name}.toml");
        let Parsed { root, ok: true, src } =
            parse::parse_checked(self.vfs, &path, &schema::PREFAB_FILE, &mut self.diags)?
        else {
            return None;
        };
        let shapes = root.get("shape").and_then(defs::decode_shapes);
        let before = self.diags.len();
        if let Some(shapes) = &shapes {
            let f = Fields::new(&root, &schema::PREFAB_FILE);
            let allow = f.strs("allow");
            let w = check::shape::Written { src: &src, list: root.get("shape"), span: Span::default(), allow: &allow };
            let [fw, fh] = f.cell("footprint");
            check::shape::list(shapes, [fw.max(1), fh.max(1)], &w, &mut |d| self.diags.push(d));
        }
        let failed = self.diags[before..].iter().any(Diagnostic::is_error);
        (!failed).then_some(Prefab { table: root, shapes })
    }

    pub fn prefab_exists(&self, name: &str) -> bool {
        prefab_exists(self.vfs, name)
    }

    /// The size of a prefab's file, 0 if there is none.
    pub(crate) fn prefab_bytes(&self, name: &str) -> u64 {
        self.vfs.get(&format!("prefabs/{name}.toml")).map_or(0, |t| t.len() as u64)
    }

    /// Load a scene. Returns `None` if the file is missing or has errors;
    /// diagnostics accumulate in `self.diags`.
    ///
    /// The stages: parse (text to nodes, the schema), resolve (the ground,
    /// prefabs, names, what each [[connect]] refers to), check (each rule
    /// on its own), build. Their problems are reported in the order of the
    /// file (`order::At`).
    pub fn load_scene(&mut self, path: &str) -> Option<SceneDef> {
        let mut found = Vec::new();
        let parsed = parse::parse_checked(self.vfs, path, &schema::SCENE_FILE, &mut found);
        let Some(parsed) = parsed else {
            self.diags.extend(found);
            return None;
        };
        let mut out = Diags::default();
        out.extend(At::File, found);
        let resolved = resolve::scene(self, &parsed, &mut out);
        let mut checked = Diags::default();
        check::scene(self.vfs, &parsed, &resolved, &mut checked);
        let out = Diags::merge(out, checked);
        let failed = !parsed.ok || out.has_errors();
        self.diags.extend(out.into_diagnostics());
        if failed {
            return None;
        }
        Some(build(path, &parsed, resolved))
    }

    /// A prefab (see [`SceneLoader::prefab`]). The problems of its file are
    /// reported at `at`: where it is first used.
    pub(crate) fn prefab_at(&mut self, name: &str, out: &mut Diags, at: At) -> Option<&Prefab> {
        if !self.prefabs.contains_key(name) {
            let before = self.diags.len();
            self.loaded(name);
            out.extend(at, self.diags.drain(before..));
        }
        self.loaded(name)
    }

    /// A prefab decoded as an entity at [0, 0, 0], reporting E0201 at `span`
    /// when it does not exist. `None` if missing or invalid.
    pub(crate) fn prefab_def(
        &mut self,
        name: &str,
        src: &SourceFile,
        span: Span,
        out: &mut Diags,
        at: At,
    ) -> Option<EntityDef> {
        if !self.prefab_exists(name) {
            let names = self.prefab_names();
            let hint = did_you_mean(name, names.iter().map(String::as_str))
                .unwrap_or_else(|| format!("create prefabs/{name}.toml"));
            out.at(at).push(
                Diagnostic::new(&codes::UNKNOWN_PREFAB, format!("unknown prefab `{name}`")).at(src, span).hint(hint),
            );
            return None;
        }
        let Prefab { mut table, shapes } = self.prefab_at(name, out, at)?.clone();
        table.set_str("prefab", name);
        let origin = Origin { file: src.path.clone(), line: src.locate(span.start).0, span };
        Some(defs::decode_entity(&table, [0.0, 0.0, 0.0], origin, shapes))
    }
}

/// Names of all prefab files, e.g. `wall`, `props/tree`.
pub(crate) fn prefab_names(vfs: &Vfs) -> Vec<String> {
    vfs.paths_under("prefabs", ".toml").map(|p| p["prefabs/".len()..p.len() - ".toml".len()].to_string()).collect()
}

pub(crate) fn prefab_exists(vfs: &Vfs, name: &str) -> bool {
    vfs.contains(&format!("prefabs/{name}.toml"))
}

/// Stage 4, build: the scene's definitions.
fn build(path: &str, parsed: &Parsed, r: Resolved) -> SceneDef {
    let empty = Table::default();
    let meta = Fields::new(parsed.root.table("scene").unwrap_or(&empty), &schema::SCENE);
    let file_stem = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".toml").to_string();
    SceneDef {
        path: path.to_string(),
        name: meta.opt_str("name").unwrap_or(file_stem),
        doc: meta.opt_str("doc"),
        seed: meta.opt_int("seed").map(|s| s as u64),
        camera: defs::decode_camera(parsed.root.table("camera")),
        ground: r.ground,
        entities: r.entities,
        connections: r.connections.into_iter().map(|c| c.def).collect(),
        templates: r.templates,
    }
}
