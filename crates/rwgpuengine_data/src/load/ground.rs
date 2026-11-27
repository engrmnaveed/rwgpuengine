//! `[ground]` with an optional terrain `map`, and its `[[terrain]]` tables.
//!
//! Part of stage 2. A map's cells are its terrains' characters, so the
//! ground is decoded and checked in one pass, in the order of its parts:
//! the terrains, then `size` or the rows of `map`, then the keys that do not
//! go with a map.

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::{Diagnostic, SourceFile, Span};

use super::order::Sink;
use crate::defs::{self, GroundDef};
use crate::node::Table;
use crate::schema::{self, Fields};

/// The ground. Problems are reported; the ground is still returned when it
/// has a usable size.
pub(crate) fn ground(root: &Table, src: &SourceFile, out: &mut Sink) -> Option<GroundDef> {
    let g = root.table("ground")?;
    let gspan = root.value_span("ground", Span::default());
    let mut terrains: Vec<defs::TerrainDef> = Vec::new();
    for node in root.array("terrain").unwrap_or(&[]) {
        let Some(t) = node.as_table() else { continue };
        let raw = t.str("char").unwrap_or_default();
        let at = t.value_span("char", node.span);
        if raw.chars().count() != 1 || raw.chars().all(char::is_whitespace) {
            out.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!("[[terrain]] `char` must be one visible character, found {raw:?}"),
                )
                .at(src, at)
                .hint("for example `char = \"g\"` for grass or `char = \"~\"` for water"),
            );
            continue;
        }
        let def = defs::decode_terrain(t);
        if let Some(first) = terrains.iter().find(|x| x.ch == def.ch) {
            out.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!("terrain char {raw:?} is declared twice (`{}` and `{}`)", first.name, def.name),
                )
                .at(src, at)
                .hint("give each terrain its own character"),
            );
            continue;
        }
        terrains.push(def);
    }
    let Some(map_node) = g.get("map") else {
        if !g.has("size") {
            out.push(
                Diagnostic::new(&codes::MISSING_KEY, "[ground] needs `size` or `map`")
                    .at(src, gspan)
                    .hint("`size = [8, 8]` for plain ground, or `map = [\"...\", ...]` with [[terrain]] tables"),
            );
            return None;
        }
        return Some(defs::decode_plain_ground(g));
    };
    let rows: Vec<(String, Span)> =
        map_node.as_array().unwrap_or(&[]).iter().filter_map(|n| Some((n.as_str()?.to_string(), n.span))).collect();
    let width = rows.first().map_or(0, |(r, _)| r.chars().count());
    let mut ok = width > 0;
    if width == 0 {
        out.push(Diagnostic::new(&codes::INVALID_VALUE, "`map` rows must not be empty").at(src, map_node.span));
    }
    let declared = || {
        if terrains.is_empty() {
            "no terrain is declared yet".to_string()
        } else {
            let list: Vec<String> = terrains.iter().map(|t| format!("{:?} ({})", t.ch.to_string(), t.name)).collect();
            format!("declared: {}", list.join(", "))
        }
    };
    let mut cells = Vec::with_capacity(width * rows.len());
    for (y, (row, at)) in rows.iter().enumerate() {
        let n = row.chars().count();
        if n != width {
            ok = false;
            out.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!("`map` row {y} has {n} characters, but row 0 has {width}"),
                )
                .at(src, *at)
                .hint("every row must have the same length: the map is a rectangle"),
            );
            continue;
        }
        for (x, ch) in row.chars().enumerate() {
            match terrains.iter().position(|t| t.ch == ch) {
                Some(i) => cells.push(i as u8),
                None => {
                    ok = false;
                    let col = Span::new(at.start + 1 + row.char_indices().nth(x).map_or(0, |(b, _)| b), 0);
                    let col = Span::new(col.start, col.start + ch.len_utf8());
                    out.push(
                        Diagnostic::new(
                            &codes::UNKNOWN_TERRAIN,
                            format!("`map` cell [{x}, {y}] is {:?}, which no [[terrain]] declares", ch.to_string()),
                        )
                        .at(src, col)
                        .note(declared())
                        .hint(format!(
                            "add [[terrain]] with `char = {:?}`, or use a declared character",
                            ch.to_string()
                        )),
                    );
                    break;
                }
            }
        }
    }
    let size = [width as i32, rows.len() as i32];
    if g.has("size") {
        let given = Fields::new(g, &schema::GROUND).cell("size");
        if given != size {
            out.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!(
                        "`size = [{}, {}]` does not match the map, which is {} x {}",
                        given[0], given[1], size[0], size[1]
                    ),
                )
                .at(src, g.value_span("size", gspan))
                .hint("remove `size`: a map sets the ground's size"),
            );
        }
    }
    for key in ["color", "alt_color"] {
        if g.has(key) {
            out.push(
                Diagnostic::new(&codes::INVALID_VALUE, format!("`{key}` does not apply with `map`"))
                    .at(src, g.key_span(key, gspan))
                    .hint("set colors on the [[terrain]] tables"),
            );
        }
    }
    if !ok || terrains.len() > 255 {
        return None;
    }
    Some(GroundDef { size, terrains, cells, has_map: true })
}
