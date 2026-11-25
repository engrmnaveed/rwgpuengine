//! Finding the project and reading it from disk.

use std::path::{Path, PathBuf};

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_data::{PROJECT_FILE, Vfs};

use crate::out::normalize;

/// A project on disk plus what the PATH argument pointed at.
#[derive(Debug)]
pub struct Located {
    /// Absolute project root.
    pub root: PathBuf,
    pub vfs: Vfs,
    /// Project-relative path of the file argument, if it was a file.
    pub file: Option<String>,
}

impl Located {
    /// The file argument, if it is a scene (anything outside tests/).
    pub fn scene_arg(&self) -> Option<String> {
        self.file.clone().filter(|f| !f.starts_with("tests/"))
    }

    /// The file argument, if it is a playtest (under tests/).
    pub fn playtest_arg(&self) -> Option<String> {
        self.file.clone().filter(|f| f.starts_with("tests/"))
    }

    /// Absolute path of a project-relative path.
    pub fn abs(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}

/// Find the project containing `arg` (a folder or file; default `.`) by
/// walking up to the nearest rwgpu.toml, then read its text files.
pub fn locate(arg: Option<&str>) -> Result<Located, Diagnostic> {
    let cwd = std::env::current_dir().map_err(|e| io_err(".", &e))?;
    let given = arg.unwrap_or(".");
    let path = normalize(&cwd.join(given));
    if !path.exists() {
        return Err(Diagnostic::new(&codes::FILE_NOT_FOUND, format!("`{given}` does not exist"))
            .hint("pass a project folder, a scene file or a playtest file"));
    }
    let start = if path.is_dir() { path.clone() } else { path.parent().map(Path::to_path_buf).unwrap_or_default() };
    let mut dir = Some(start.as_path());
    let root = loop {
        let Some(d) = dir else {
            return Err(Diagnostic::new(
                &codes::PROJECT_NOT_FOUND,
                format!("no {PROJECT_FILE} in `{given}` or any parent folder"),
            )
            .hint("run inside a project, pass the project folder, or create one with `rwgpu new <path>`"));
        };
        if d.join(PROJECT_FILE).is_file() {
            break d.to_path_buf();
        }
        dir = d.parent();
    };
    let vfs = Vfs::from_dir(&root).map_err(|e| io_err(&root.display().to_string(), &e))?;
    let file = if path.is_file() {
        let rel = path.strip_prefix(&root).unwrap_or(&path);
        Some(rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
    } else {
        None
    };
    Ok(Located { root, vfs, file })
}

pub fn io_err(path: &str, e: &std::io::Error) -> Diagnostic {
    Diagnostic::new(&codes::IO_ERROR, format!("{path}: {e}"))
}

/// Write a file, creating parent folders.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), Diagnostic> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_err(&parent.display().to_string(), &e))?;
    }
    std::fs::write(path, bytes).map_err(|e| io_err(&path.display().to_string(), &e))
}
