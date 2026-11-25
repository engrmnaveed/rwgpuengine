//! A minimal JSON value with deterministic output.
//!
//! Objects keep insertion order, so the same data always prints the same
//! bytes: diffs of `--json` output are meaningful and golden tests are stable.
//! No serde, no proc macros (ADR 0004).

use std::fmt::Write as _;

/// A JSON value. Objects are ordered `(key, value)` lists.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// An empty object.
    pub fn object() -> Json {
        Json::Obj(Vec::new())
    }

    /// An empty array.
    pub fn array() -> Json {
        Json::Arr(Vec::new())
    }

    /// Builder-style insert. Panics if `self` is not an object.
    pub fn with(mut self, key: &str, value: impl Into<Json>) -> Json {
        self.set(key, value);
        self
    }

    /// Insert or replace `key`. Panics if `self` is not an object.
    pub fn set(&mut self, key: &str, value: impl Into<Json>) {
        let value = value.into();
        match self {
            Json::Obj(entries) => {
                if let Some(entry) = entries.iter_mut().find(|(k, _)| k == key) {
                    entry.1 = value;
                } else {
                    entries.push((key.to_string(), value));
                }
            }
            _ => panic!("Json::set called on a non-object"),
        }
    }

    /// Append to an array. Panics if `self` is not an array.
    pub fn push(&mut self, value: impl Into<Json>) {
        match self {
            Json::Arr(items) => items.push(value.into()),
            _ => panic!("Json::push called on a non-array"),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Follow a `/`-separated path of object keys and array indices.
    pub fn pointer(&self, path: &str) -> Option<&Json> {
        let mut cur = self;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            cur = match cur {
                Json::Obj(_) => cur.get(part)?,
                Json::Arr(items) => items.get(part.parse::<usize>().ok()?)?,
                _ => return None,
            };
        }
        Some(cur)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Int(i) => Some(*i as f64),
            Json::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    fn is_scalar(&self) -> bool {
        !matches!(self, Json::Arr(_) | Json::Obj(_))
    }

    /// Single-line output.
    pub fn to_compact(&self) -> String {
        let mut out = String::new();
        self.write_compact(&mut out);
        out
    }

    /// Two-space indented output. Short arrays of scalars stay on one line
    /// (`"pos": [3, 4, 0]`) so world dumps stay readable and diff well.
    pub fn to_pretty(&self) -> String {
        let mut out = String::new();
        self.write_pretty(&mut out, 0);
        out
    }

    fn write_compact(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Int(i) => {
                let _ = write!(out, "{i}");
            }
            Json::Float(f) => write_float(out, *f),
            Json::Str(s) => write_string(out, s),
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write_compact(out);
                }
                out.push(']');
            }
            Json::Obj(entries) => {
                out.push('{');
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_string(out, k);
                    out.push(':');
                    v.write_compact(out);
                }
                out.push('}');
            }
        }
    }

    fn write_pretty(&self, out: &mut String, indent: usize) {
        match self {
            Json::Arr(items) if items.is_empty() => out.push_str("[]"),
            Json::Arr(items) if items.len() <= 16 && items.iter().all(Json::is_scalar) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.write_compact(out);
                }
                out.push(']');
            }
            Json::Arr(items) => {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    push_indent(out, indent + 1);
                    item.write_pretty(out, indent + 1);
                    if i + 1 < items.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                push_indent(out, indent);
                out.push(']');
            }
            Json::Obj(entries) if entries.is_empty() => out.push_str("{}"),
            Json::Obj(entries) => {
                out.push_str("{\n");
                for (i, (k, v)) in entries.iter().enumerate() {
                    push_indent(out, indent + 1);
                    write_string(out, k);
                    out.push_str(": ");
                    v.write_pretty(out, indent + 1);
                    if i + 1 < entries.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                push_indent(out, indent);
                out.push('}');
            }
            scalar => scalar.write_compact(out),
        }
    }

    /// Strict JSON parser (RFC 8259). Used by tests and tooling that read
    /// `--json` output; the engine's own inputs are TOML.
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut p = Parser { s: text.as_bytes(), i: 0 };
        p.ws();
        let v = p.value()?;
        p.ws();
        if p.i != p.s.len() {
            return Err(format!("trailing characters at byte {}", p.i));
        }
        Ok(v)
    }
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn write_float(out: &mut String, f: f64) {
    if !f.is_finite() {
        out.push_str("null");
        return;
    }
    // Rust's float Display is the shortest string that round-trips, and is
    // identical on every platform.
    let s = format!("{f}");
    out.push_str(&s);
    if !s.contains('.') && !s.contains('e') && !s.contains("inf") {
        out.push_str(".0");
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

macro_rules! from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for Json {
            fn from(v: $t) -> Json { Json::Int(v as i64) }
        }
    )*};
}
from_int!(i8, i16, i32, i64, u8, u16, u32, usize);

impl From<u64> for Json {
    /// u64 values above i64::MAX become strings; hashes should use
    /// [`crate::hash::hex`] instead anyway.
    fn from(v: u64) -> Json {
        if v <= i64::MAX as u64 { Json::Int(v as i64) } else { Json::Str(v.to_string()) }
    }
}

impl From<f32> for Json {
    /// Converts through the shortest f32 representation so `0.1f32` prints
    /// as `0.1`, not `0.10000000149011612`.
    fn from(v: f32) -> Json {
        if !v.is_finite() {
            return Json::Null;
        }
        Json::Float(format!("{v}").parse::<f64>().unwrap_or(0.0))
    }
}

impl From<f64> for Json {
    fn from(v: f64) -> Json {
        Json::Float(v)
    }
}

impl From<bool> for Json {
    fn from(v: bool) -> Json {
        Json::Bool(v)
    }
}

impl From<&str> for Json {
    fn from(v: &str) -> Json {
        Json::Str(v.to_string())
    }
}

impl From<String> for Json {
    fn from(v: String) -> Json {
        Json::Str(v)
    }
}

impl From<&String> for Json {
    fn from(v: &String) -> Json {
        Json::Str(v.clone())
    }
}

impl From<Vec<Json>> for Json {
    fn from(v: Vec<Json>) -> Json {
        Json::Arr(v)
    }
}

impl<T: Into<Json>> From<Option<T>> for Json {
    fn from(v: Option<T>) -> Json {
        v.map_or(Json::Null, Into::into)
    }
}

impl<T: Into<Json> + Copy, const N: usize> From<[T; N]> for Json {
    fn from(v: [T; N]) -> Json {
        Json::Arr(v.iter().map(|x| (*x).into()).collect())
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }

    fn err<T>(&self, msg: &str) -> Result<T, String> {
        Err(format!("{msg} at byte {}", self.i))
    }

    fn eat(&mut self, lit: &str) -> Result<(), String> {
        if self.s[self.i..].starts_with(lit.as_bytes()) {
            self.i += lit.len();
            Ok(())
        } else {
            self.err(&format!("expected `{lit}`"))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        match self.s.get(self.i) {
            None => self.err("unexpected end of input"),
            Some(b'n') => self.eat("null").map(|_| Json::Null),
            Some(b't') => self.eat("true").map(|_| Json::Bool(true)),
            Some(b'f') => self.eat("false").map(|_| Json::Bool(false)),
            Some(b'"') => self.string().map(Json::Str),
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                loop {
                    self.ws();
                    items.push(self.value()?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Json::Arr(items));
                        }
                        _ => return self.err("expected `,` or `]`"),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut entries = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Json::Obj(entries));
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        return self.err("expected string key");
                    }
                    let k = self.string()?;
                    self.ws();
                    self.eat(":")?;
                    self.ws();
                    let v = self.value()?;
                    entries.push((k, v));
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Json::Obj(entries));
                        }
                        _ => return self.err("expected `,` or `}`"),
                    }
                }
            }
            Some(c) if *c == b'-' || c.is_ascii_digit() => self.number(),
            Some(_) => self.err("unexpected character"),
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        let mut is_float = false;
        while let Some(&c) = self.s.get(self.i) {
            match c {
                b'0'..=b'9' | b'-' | b'+' => {}
                b'.' | b'e' | b'E' => is_float = true,
                _ => break,
            }
            self.i += 1;
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).map_err(|e| e.to_string())?;
        if is_float {
            text.parse::<f64>().map(Json::Float).map_err(|e| format!("bad number `{text}`: {e}"))
        } else {
            text.parse::<i64>()
                .map(Json::Int)
                .or_else(|_| text.parse::<f64>().map(Json::Float))
                .map_err(|e| format!("bad number `{text}`: {e}"))
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1; // opening quote
        let mut out = String::new();
        loop {
            let Some(&c) = self.s.get(self.i) else {
                return self.err("unterminated string");
            };
            self.i += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let Some(&e) = self.s.get(self.i) else {
                        return self.err("bad escape");
                    };
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = self.s.get(self.i..self.i + 4).ok_or("bad \\u escape")?;
                            let hex = std::str::from_utf8(hex).map_err(|e| e.to_string())?;
                            let code = u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
                            self.i += 4;
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        _ => return self.err("bad escape"),
                    }
                }
                _ => {
                    // Re-decode UTF-8 sequences byte by byte.
                    let start = self.i - 1;
                    let len = utf8_len(c);
                    let end = (start + len).min(self.s.len());
                    out.push_str(std::str::from_utf8(&self.s[start..end]).map_err(|e| e.to_string())?);
                    self.i = end;
                }
            }
        }
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_insertion_order() {
        let j = Json::object().with("b", 1).with("a", 2);
        assert_eq!(j.to_compact(), r#"{"b":1,"a":2}"#);
    }

    #[test]
    fn f32_prints_shortest() {
        assert_eq!(Json::from(0.1f32).to_compact(), "0.1");
        assert_eq!(Json::from(2.0f32).to_compact(), "2.0");
    }

    #[test]
    fn pretty_keeps_short_scalar_arrays_inline() {
        let j = Json::object().with("pos", [1, 2, 0]);
        assert_eq!(j.to_pretty(), "{\n  \"pos\": [1, 2, 0]\n}");
    }

    #[test]
    fn round_trips() {
        let j = Json::object()
            .with("s", "a\"b\\c\nd é")
            .with("n", Json::Null)
            .with("f", 1.5)
            .with("i", -3)
            .with("a", vec![Json::from(true), Json::object().with("x", 1)]);
        assert_eq!(Json::parse(&j.to_pretty()).unwrap(), j);
        assert_eq!(Json::parse(&j.to_compact()).unwrap(), j);
    }

    #[test]
    fn pointer_walks_objects_and_arrays() {
        let j = Json::parse(r#"{"a":[{"b":7}]}"#).unwrap();
        assert_eq!(j.pointer("a/0/b").and_then(Json::as_i64), Some(7));
    }
}
