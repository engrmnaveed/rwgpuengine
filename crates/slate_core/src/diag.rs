//! Diagnostics: stable code, severity, location, message, hint.
//!
//! Modeled on rustc: every problem the engine reports carries a stable code
//! from [`crate::codes`], the file and 1-based line/column, the offending
//! source line, and (when we can tell) a hint that says how to fix it.
//! Humans get the rustc layout; agents get the same data as JSON.

use crate::codes::CodeInfo;
use crate::json::Json;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        }
    }
}

/// A byte range in a source file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Span {
        Span { start, end: end.max(start) }
    }
}

impl From<std::ops::Range<usize>> for Span {
    fn from(r: std::ops::Range<usize>) -> Span {
        Span::new(r.start, r.end)
    }
}

/// A text file with a line index, used to turn byte spans into line/column.
#[derive(Clone, Debug)]
pub struct SourceFile {
    /// Project-relative path with `/` separators, e.g. `scenes/main.toml`.
    pub path: String,
    pub text: String,
    line_starts: Vec<usize>,
}

impl SourceFile {
    pub fn new(path: impl Into<String>, text: impl Into<String>) -> SourceFile {
        let text = text.into();
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        SourceFile { path: path.into(), text, line_starts }
    }

    /// 1-based (line, column). Column counts characters, not bytes.
    pub fn locate(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.text.len());
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let start = self.line_starts[line_idx];
        let col = self.text.get(start..offset).map_or(offset - start, |s| s.chars().count());
        (line_idx + 1, col + 1)
    }

    /// The text of a 1-based line, without the newline.
    pub fn line_text(&self, line: usize) -> &str {
        let Some(&start) = self.line_starts.get(line.wrapping_sub(1)) else {
            return "";
        };
        let end = self.line_starts.get(line).copied().unwrap_or(self.text.len());
        self.text.get(start..end).unwrap_or("").trim_end_matches(['\n', '\r'])
    }
}

/// One reported problem.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    /// Stable code such as `E0103`; see [`crate::codes`].
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    pub file: Option<String>,
    /// 1-based.
    pub line: Option<usize>,
    /// 1-based, in characters.
    pub column: Option<usize>,
    /// Length of the highlighted range on `line`, in characters (>= 1).
    pub width: usize,
    pub source_line: Option<String>,
    pub hint: Option<String>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    /// A diagnostic with the code's default severity.
    pub fn new(code: &'static CodeInfo, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code: code.code,
            severity: code.severity,
            message: message.into(),
            file: None,
            line: None,
            column: None,
            width: 1,
            source_line: None,
            hint: None,
            notes: Vec::new(),
        }
    }

    /// Attach a file path without a position.
    pub fn in_file(mut self, path: &str) -> Diagnostic {
        self.file = Some(path.to_string());
        self
    }

    /// Attach the file, line/column and source line for `span` in `src`.
    pub fn at(mut self, src: &SourceFile, span: Span) -> Diagnostic {
        let (line, col) = src.locate(span.start);
        let line_text = src.line_text(line);
        let (_, end_col) = src.locate(span.end);
        let end_line = src.locate(span.end).0;
        let width = if end_line == line && end_col > col {
            end_col - col
        } else {
            line_text.chars().count().saturating_sub(col - 1).max(1)
        };
        self.file = Some(src.path.clone());
        self.line = Some(line);
        self.column = Some(col);
        self.width = width.max(1);
        self.source_line = Some(line_text.to_string());
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Diagnostic {
        self.hint = Some(hint.into());
        self
    }

    pub fn maybe_hint(self, hint: Option<String>) -> Diagnostic {
        match hint {
            Some(h) => self.hint(h),
            None => self,
        }
    }

    pub fn note(mut self, note: impl Into<String>) -> Diagnostic {
        self.notes.push(note.into());
        self
    }

    pub fn severity(mut self, severity: Severity) -> Diagnostic {
        self.severity = severity;
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// `file:line:column`, or as much of it as is known.
    pub fn location(&self) -> Option<String> {
        let file = self.file.as_ref()?;
        Some(match (self.line, self.column) {
            (Some(l), Some(c)) => format!("{file}:{l}:{c}"),
            _ => file.clone(),
        })
    }

    pub fn to_json(&self) -> Json {
        let mut j = Json::object()
            .with("code", self.code)
            .with("severity", self.severity.as_str())
            .with("message", self.message.as_str())
            .with("file", self.file.clone())
            .with("line", self.line)
            .with("column", self.column);
        j.set("hint", self.hint.clone());
        j.set("notes", Json::Arr(self.notes.iter().map(Json::from).collect()));
        j
    }

    /// rustc-style rendering:
    ///
    /// ```text
    /// error[E0103]: unknown key `sprit` in entity
    ///   --> scenes/main.toml:14:1
    ///    |
    /// 14 | sprit = { color = "#fff" }
    ///    | ^^^^^
    ///    = help: did you mean `sprite`?
    /// ```
    pub fn render_human(&self) -> String {
        let mut out = format!("{}[{}]: {}\n", self.severity.as_str(), self.code, self.message);
        let gutter = self.line.map_or(1, |l| l.to_string().len());
        let pad = " ".repeat(gutter);
        if let Some(loc) = self.location() {
            out.push_str(&format!("{pad}--> {loc}\n"));
        }
        if let (Some(line), Some(col), Some(text)) = (self.line, self.column, &self.source_line) {
            out.push_str(&format!("{pad} |\n"));
            out.push_str(&format!("{line} | {text}\n"));
            let lead: String = text
                .chars()
                .take(col - 1)
                .map(|c| if c == '\t' { '\t' } else { ' ' })
                .collect();
            out.push_str(&format!("{pad} | {lead}{}\n", "^".repeat(self.width)));
        }
        if let Some(hint) = &self.hint {
            out.push_str(&format!("{pad} = help: {hint}\n"));
        }
        for note in &self.notes {
            out.push_str(&format!("{pad} = note: {note}\n"));
        }
        out
    }
}

/// Counts of (errors, warnings).
pub fn count(diags: &[Diagnostic]) -> (usize, usize) {
    let errors = diags.iter().filter(|d| d.severity == Severity::Error).count();
    let warnings = diags.iter().filter(|d| d.severity == Severity::Warning).count();
    (errors, warnings)
}

/// Sort for stable output: by file, line, column, then code.
pub fn sort(diags: &mut [Diagnostic]) {
    diags.sort_by(|a, b| {
        (&a.file, a.line, a.column, a.code).cmp(&(&b.file, b.line, b.column, b.code))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes;

    #[test]
    fn locates_lines_and_columns() {
        let src = SourceFile::new("a.toml", "x = 1\ny = \"é\"\nz = 3");
        assert_eq!(src.locate(0), (1, 1));
        assert_eq!(src.locate(6), (2, 1));
        assert_eq!(src.locate(src.text.find('z').unwrap()), (3, 1));
        assert_eq!(src.line_text(2), "y = \"é\"");
    }

    #[test]
    fn renders_like_rustc() {
        let src = SourceFile::new("scenes/main.toml", "[[entity]]\nsprit = 1\n");
        let d = Diagnostic::new(&codes::UNKNOWN_KEY, "unknown key `sprit` in entity")
            .at(&src, Span::new(11, 16))
            .hint("did you mean `sprite`?");
        let text = d.render_human();
        assert!(text.contains("error[E0103]: unknown key `sprit` in entity"), "{text}");
        assert!(text.contains("--> scenes/main.toml:2:1"), "{text}");
        assert!(text.contains("2 | sprit = 1"), "{text}");
        assert!(text.contains("  | ^^^^^"), "{text}");
        assert!(text.contains("= help: did you mean `sprite`?"), "{text}");
    }
}
