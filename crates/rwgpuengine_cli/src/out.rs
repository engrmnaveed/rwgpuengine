//! Command results and how they are printed.
//!
//! With `--json`, a command prints exactly one JSON object on stdout:
//!
//! ```json
//! { "rwgpu": "0.1.0", "format": 2, "command": "check", "ok": true, ..., "diagnostics": [] }
//! ```
//!
//! Without it, results go to stdout as text and diagnostics to stderr in
//! rustc style. Either way the exit code is 0 (ok), 1 (errors or failed
//! checks) or 2 (bad command line).

use std::path::{Path, PathBuf};

use rwgpuengine_core::diag::{self, Diagnostic};
use rwgpuengine_core::json::Json;

pub const EXIT_OK: i32 = 0;
pub const EXIT_FAILED: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

/// What a command produced.
#[derive(Debug)]
pub struct Outcome {
    pub ok: bool,
    /// Command-specific JSON fields (an object).
    pub data: Json,
    /// Text for humans (stdout).
    pub text: String,
    pub diagnostics: Vec<Diagnostic>,
    /// Project root as shown to the user; diagnostic paths are relative to it.
    pub root: Option<PathBuf>,
    pub exit: i32,
}

impl Outcome {
    pub fn new(ok: bool) -> Outcome {
        Outcome {
            ok,
            data: Json::object(),
            text: String::new(),
            diagnostics: Vec::new(),
            root: None,
            exit: if ok { EXIT_OK } else { EXIT_FAILED },
        }
    }

    /// A failure made only of diagnostics.
    pub fn failed(diagnostics: Vec<Diagnostic>) -> Outcome {
        Outcome { diagnostics, ..Outcome::new(false) }
    }

    pub fn usage(d: Diagnostic) -> Outcome {
        Outcome { exit: EXIT_USAGE, ..Outcome::failed(vec![d]) }
    }

    pub fn with_root(mut self, root: &Path) -> Outcome {
        self.root = Some(root.to_path_buf());
        self
    }

    pub fn line(&mut self, s: impl AsRef<str>) {
        self.text.push_str(s.as_ref());
        self.text.push('\n');
    }

    /// Recompute ok/exit after diagnostics were added.
    pub fn settle(mut self) -> Outcome {
        if self.diagnostics.iter().any(Diagnostic::is_error) && self.exit == EXIT_OK {
            self.ok = false;
            self.exit = EXIT_FAILED;
        }
        self
    }
}

/// Print an outcome and return the process exit code.
pub fn emit(command: &str, outcome: Outcome, json: bool) -> i32 {
    let root_display = outcome.root.as_ref().map(|r| display_path(r));
    if json {
        let mut j = Json::object()
            .with("rwgpu", rwgpuengine_core::ENGINE_VERSION)
            .with("format", rwgpuengine_core::JSON_FORMAT_VERSION)
            .with("command", command)
            .with("ok", outcome.ok);
        if let Some(root) = &root_display {
            j.set("root", root.as_str());
        }
        if let Json::Obj(entries) = outcome.data {
            for (k, v) in entries {
                j.set(&k, v);
            }
        }
        let (errors, warnings) = diag::count(&outcome.diagnostics);
        j.set("errors", errors);
        j.set("warnings", warnings);
        j.set("diagnostics", outcome.diagnostics.iter().map(Diagnostic::to_json).collect::<Vec<_>>());
        println!("{}", j.to_pretty());
    } else {
        print!("{}", outcome.text);
        for d in &outcome.diagnostics {
            let mut d = d.clone();
            // Show paths relative to where the user is, so they are clickable.
            if let (Some(root), Some(file)) = (&root_display, &d.file)
                && root != "."
            {
                d.file = Some(format!("{}/{file}", root.trim_end_matches('/')));
            }
            eprint!("{}", d.render_human());
        }
        let (errors, warnings) = diag::count(&outcome.diagnostics);
        if errors > 0 || warnings > 0 {
            let plural = |n: usize, w: &str| {
                if n == 1 { format!("{n} {w}") } else { format!("{n} {w}s") }
            };
            let verdict = if outcome.ok { "finished" } else { "failed" };
            eprintln!("{command} {verdict} with {} and {}", plural(errors, "error"), plural(warnings, "warning"));
        }
    }
    outcome.exit
}

/// `path` relative to the current directory when it is inside it.
pub fn display_path(path: &Path) -> String {
    let Ok(cwd) = std::env::current_dir() else {
        return path.display().to_string();
    };
    let abs = if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
    let abs = normalize(&abs);
    match abs.strip_prefix(&cwd) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
        Ok(rel) => rel.display().to_string(),
        Err(_) => abs.display().to_string(),
    }
}

/// Resolve `.` and `..` without touching the filesystem.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}
