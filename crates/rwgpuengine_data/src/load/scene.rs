//! A scene through the four stages, and the prefabs it uses.

use std::collections::BTreeMap;

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::{Diagnostic, SourceFile, Span};
use rwgpuengine_core::text::did_you_mean;

use super::check;
use super::order::{At, Diags};
use super::parse::{self, Parsed};
use super::resolve::{self, Resolved};
use super::vfs::Vfs;
use crate::defs::{self, EntityDef, Origin, SceneDef};
use crate::node::Table;
use crate::schema::{self, Fields};

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
        prefab_names(self.vfs)
    }

    /// Load (once) and validate a prefab. `None` if missing or invalid.
    pub fn prefab(&mut self, name: &str) -> Option<Table> {
        if let Some(cached) = self.prefabs.get(name) {
            return cached.clone();
        }
        let path = format!("prefabs/{name}.toml");
        let loaded = match parse::parse_checked(self.vfs, &path, &schema::PREFAB_FILE, &mut self.diags) {
            Some(Parsed { root, ok: true, .. }) => Some(root),
            _ => None,
        };
        self.prefabs.insert(name.to_string(), loaded.clone());
        loaded
    }

    pub fn prefab_exists(&self, name: &str) -> bool {
        prefab_exists(self.vfs, name)
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

    /// A prefab's table (see [`SceneLoader::prefab`]). The problems of its
    /// file are reported at `at`: where it is first used.
    pub(crate) fn prefab_at(&mut self, name: &str, out: &mut Diags, at: At) -> Option<&Table> {
        if !self.prefabs.contains_key(name) {
            let before = self.diags.len();
            self.prefab(name);
            out.extend(at, self.diags.drain(before..));
        }
        self.prefabs.get(name)?.as_ref()
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
        let mut table = self.prefab_at(name, out, at)?.clone();
        table.set_str("prefab", name);
        let origin = Origin { file: src.path.clone(), line: src.locate(span.start).0, span };
        Some(defs::decode_entity(&table, [0.0, 0.0, 0.0], origin))
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
