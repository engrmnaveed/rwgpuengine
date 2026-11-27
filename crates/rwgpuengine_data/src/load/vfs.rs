//! The virtual file set: project-relative path → text.

use std::collections::BTreeMap;
use std::path::Path;

use rwgpuengine_core::hash::Hasher64;

/// Project-relative text files. Paths use `/` and never start with `/`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vfs {
    files: BTreeMap<String, String>,
}

const BUNDLE_HEADER: &str = "rwgpu-bundle 1\n";

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
        let body = bundle.strip_prefix(BUNDLE_HEADER).ok_or("not a rwgpu bundle (bad header)")?;
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
