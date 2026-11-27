//! Stage 1, parse: a file's text to nodes, checked against the schema.
//!
//! Every file of a project goes through it. A syntax error stops the file
//! (E0101); the schema (`schema::Checker`) reports every unknown key, wrong
//! type, missing key and invalid value, and the later stages still read
//! what it could.

use rwgpuengine_core::diag::{Diagnostic, SourceFile, Span};

use super::vfs::Vfs;
use crate::node::{Table, parse_toml};
use crate::schema::{self, Checker};

/// A parsed file.
pub(crate) struct Parsed {
    pub src: SourceFile,
    pub root: Table,
    /// The schema found no problem.
    pub ok: bool,
}

/// Parse and validate one file against a file-level spec. `None` if the
/// file is not there or is not TOML.
pub(crate) fn parse_checked(
    vfs: &Vfs,
    path: &str,
    spec: &'static schema::Spec,
    diags: &mut Vec<Diagnostic>,
) -> Option<Parsed> {
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
    Some(Parsed { src, root, ok })
}
