//! Stage 1, parse: a file's text to nodes, checked against the schema.
//!
//! Every file of a project goes through it. A file over the size limit
//! (E0209) is not parsed; a syntax error stops the file (E0101); the schema
//! (`schema::Checker`) reports every unknown key, wrong type, missing key and
//! invalid value, and the later stages still read what it could, unless a
//! name or a key is over the limit (E0209).

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::{Diagnostic, SourceFile, Span};
use rwgpuengine_core::limits;

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
    let max = limits::FILE_BYTES.value;
    if text.len() as u64 > max {
        diags.push(
            Diagnostic::new(
                &codes::TOO_LARGE,
                format!("`{path}` is {} bytes, more than a file can have ({max})", text.len()),
            )
            .in_file(path)
            .hint("this version cannot read a larger file; `rwgpu docs` lists the limits (Limits on sizes)"),
        );
        return None;
    }
    let src = SourceFile::new(path, text);
    let root = match parse_toml(&src) {
        Ok(t) => t,
        Err(d) => {
            diags.push(d);
            return None;
        }
    };
    let before = diags.len();
    let ok = Checker { src: &src, diags }.table(&root, spec, Span::new(0, 0));
    // A name over the limit would make every later look-up of it long.
    if diags[before..].iter().any(|d| d.code == codes::TOO_LARGE.code) {
        return None;
    }
    Some(Parsed { src, root, ok })
}
