//! An owned, span-preserving document tree.
//!
//! TOML is parsed by the rented `toml_edit` crate and immediately converted
//! into this tree, so the rest of the engine never touches `toml_edit` types.
//! Swapping the text format later (ADR 0003) only touches this file.

use slate_core::codes;
use slate_core::diag::{Diagnostic, SourceFile, Span};
use toml_edit::{Item, TableLike, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Node>),
    Table(Table),
}

/// A table that keeps key order and key spans.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub key: String,
    pub key_span: Span,
    pub value: Node,
}

impl Node {
    /// The TOML type name used in messages: "string", "integer", ...
    pub fn type_name(&self) -> &'static str {
        match self.kind {
            NodeKind::Bool(_) => "boolean",
            NodeKind::Int(_) => "integer",
            NodeKind::Float(_) => "float",
            NodeKind::Str(_) => "string",
            NodeKind::Array(_) => "array",
            NodeKind::Table(_) => "table",
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self.kind {
            NodeKind::Bool(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self.kind {
            NodeKind::Int(i) => Some(i),
            _ => None,
        }
    }

    /// Integers and floats both read as numbers.
    pub fn as_num(&self) -> Option<f64> {
        match self.kind {
            NodeKind::Int(i) => Some(i as f64),
            NodeKind::Float(f) => Some(f),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Node]> {
        match &self.kind {
            NodeKind::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_table(&self) -> Option<&Table> {
        match &self.kind {
            NodeKind::Table(t) => Some(t),
            _ => None,
        }
    }

    /// A short rendering of the value for messages, e.g. `"orange"` or `[1, 2]`.
    pub fn preview(&self) -> String {
        match &self.kind {
            NodeKind::Bool(b) => b.to_string(),
            NodeKind::Int(i) => i.to_string(),
            NodeKind::Float(f) => f.to_string(),
            NodeKind::Str(s) => format!("{s:?}"),
            NodeKind::Array(items) => {
                let inner: Vec<String> = items.iter().take(4).map(Node::preview).collect();
                let more = if items.len() > 4 { ", ..." } else { "" };
                format!("[{}{more}]", inner.join(", "))
            }
            NodeKind::Table(_) => "{ ... }".to_string(),
        }
    }
}

impl Table {
    pub fn entry(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.key == key)
    }

    pub fn get(&self, key: &str) -> Option<&Node> {
        self.entry(key).map(|e| &e.value)
    }

    pub fn has(&self, key: &str) -> bool {
        self.entry(key).is_some()
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Node::as_str)
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(Node::as_int)
    }

    pub fn num(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Node::as_num)
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(Node::as_bool)
    }

    pub fn table(&self, key: &str) -> Option<&Table> {
        self.get(key).and_then(Node::as_table)
    }

    pub fn array(&self, key: &str) -> Option<&[Node]> {
        self.get(key).and_then(Node::as_array)
    }

    /// Span of the key, falling back to `fallback`.
    pub fn key_span(&self, key: &str, fallback: Span) -> Span {
        self.entry(key).map_or(fallback, |e| e.key_span)
    }

    /// Span of the value, falling back to `fallback`.
    pub fn value_span(&self, key: &str, fallback: Span) -> Span {
        self.entry(key).map_or(fallback, |e| e.value.span)
    }

    /// Merge `over` onto `self`: tables merge recursively, everything else is
    /// replaced. Used to apply entity overrides on top of a prefab.
    pub fn merged_with(&self, over: &Table) -> Table {
        let mut out = self.clone();
        for e in &over.entries {
            match (out.entries.iter_mut().find(|x| x.key == e.key), &e.value.kind) {
                (Some(existing), NodeKind::Table(over_t)) => {
                    if let NodeKind::Table(base_t) = &existing.value.kind {
                        let merged = base_t.merged_with(over_t);
                        existing.value = Node { kind: NodeKind::Table(merged), span: e.value.span };
                        existing.key_span = e.key_span;
                    } else {
                        *existing = e.clone();
                    }
                }
                (Some(existing), _) => *existing = e.clone(),
                (None, _) => out.entries.push(e.clone()),
            }
        }
        out
    }
}

/// Parse a TOML source file into a [`Table`], or an `E0101` diagnostic.
pub fn parse_toml(src: &SourceFile) -> Result<Table, Diagnostic> {
    let doc = toml_edit::Document::parse(src.text.as_str()).map_err(|e| {
        let span = e.span().map(Span::from).unwrap_or_default();
        let message = e.message().trim().trim_end_matches('.').to_string();
        Diagnostic::new(&codes::TOML_SYNTAX, format!("invalid TOML: {message}"))
            .at(src, span)
            .hint("check quotes, brackets and `key = value` syntax; strings need double quotes")
    })?;
    Ok(convert_table(doc.as_table(), Span::new(0, 0)))
}

fn convert_table(t: &dyn TableLike, fallback: Span) -> Table {
    let mut entries = Vec::new();
    for (key, item) in t.iter() {
        let key_span = t.get_key_value(key).and_then(|(k, _)| k.span()).map(Span::from).unwrap_or(fallback);
        if let Some(value) = convert_item(item, key_span) {
            entries.push(Entry { key: key.to_string(), key_span, value });
        }
    }
    Table { entries }
}

fn convert_item(item: &Item, key_span: Span) -> Option<Node> {
    match item {
        Item::None => None,
        Item::Value(v) => Some(convert_value(v, key_span)),
        Item::Table(t) => {
            let span = t.span().map(Span::from).unwrap_or(key_span);
            Some(Node { kind: NodeKind::Table(convert_table(t, span)), span })
        }
        Item::ArrayOfTables(a) => {
            let span = a.span().map(Span::from).unwrap_or(key_span);
            let items = a
                .iter()
                .map(|t| {
                    let tspan = t.span().map(Span::from).unwrap_or(span);
                    Node { kind: NodeKind::Table(convert_table(t, tspan)), span: tspan }
                })
                .collect();
            Some(Node { kind: NodeKind::Array(items), span })
        }
    }
}

fn convert_value(v: &Value, fallback: Span) -> Node {
    let span = v.span().map(Span::from).unwrap_or(fallback);
    let kind = match v {
        Value::String(s) => NodeKind::Str(s.value().clone()),
        Value::Integer(i) => NodeKind::Int(*i.value()),
        Value::Float(f) => NodeKind::Float(*f.value()),
        Value::Boolean(b) => NodeKind::Bool(*b.value()),
        // slate has no date fields; keep the text so type errors can show it.
        Value::Datetime(d) => NodeKind::Str(d.value().to_string()),
        Value::Array(a) => NodeKind::Array(a.iter().map(|x| convert_value(x, span)).collect()),
        Value::InlineTable(t) => NodeKind::Table(convert_table(t, span)),
    };
    Node { kind, span }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_spans_for_keys_and_values() {
        let src = SourceFile::new("t.toml", "[[entity]]\nname = \"a\"\npos = [1, 2]\n");
        let root = parse_toml(&src).unwrap();
        let ents = root.array("entity").unwrap();
        let e = ents[0].as_table().unwrap();
        let name = e.entry("name").unwrap();
        assert_eq!(&src.text[name.key_span.start..name.key_span.end], "name");
        assert_eq!(&src.text[name.value.span.start..name.value.span.end], "\"a\"");
        assert_eq!(src.locate(e.get("pos").unwrap().span.start), (3, 7));
    }

    #[test]
    fn syntax_errors_have_a_location() {
        let src = SourceFile::new("t.toml", "a = 1\nb = red\n");
        let d = parse_toml(&src).unwrap_err();
        assert_eq!(d.code, "E0101");
        assert_eq!(d.line, Some(2));
    }

    #[test]
    fn merge_overrides_fields_inside_components() {
        let base = parse_toml(&SourceFile::new("a", "solid = true\nblock = { color = \"#111\", height = 2 }")).unwrap();
        let over = parse_toml(&SourceFile::new("b", "block = { color = \"#222\" }")).unwrap();
        let m = base.merged_with(&over);
        assert_eq!(m.bool("solid"), Some(true));
        let block = m.table("block").unwrap();
        assert_eq!(block.str("color"), Some("#222"));
        assert_eq!(block.int("height"), Some(2));
    }
}
