//! The rules on one entity's merged definition, and on its `counts`.

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::{Diagnostic, SourceFile, Span};
use rwgpuengine_core::text::did_you_mean;

use super::super::order::Sink;
use super::super::scene::{prefab_exists, prefab_names};
use super::super::vfs::Vfs;
use super::shape;
use crate::defs::{self, EntityDef, LabelPart};
use crate::node::Table;
use crate::schema;

/// Checks on one entity that need its merged definition: reserved names,
/// `counts` beside `props`, footprints, labels, pushables and screen
/// entities. `t` is the entity's own table (for locations), `span` its place.
pub(crate) fn rules(def: &EntityDef, t: &Table, span: Span, src: &SourceFile, out: &mut Sink) {
    let who = def.name.as_deref().map_or("this entity".to_string(), |n| format!("`{n}`"));
    if let Some(name) = def.name.as_deref()
        && schema::RESERVED_NAMES.contains(&name)
    {
        let means = if name == "ground" {
            "[[connect]] `from = \"ground\"` means taps on the ground"
        } else {
            "[[connect]] `to = \"self\"` means the entity that emitted"
        };
        out.push(
            Diagnostic::new(&codes::INVALID_VALUE, format!("`{name}` is a reserved name: {means}"))
                .at(src, t.value_span("name", span))
                .hint("choose another name"),
        );
    }
    for (k, prefab) in &def.counts {
        if def.props.iter().filter(|(p, _)| p == k).count() > 1 {
            out.push(
                Diagnostic::new(&codes::INVALID_VALUE, format!("{who} declares `{k}` in both `props` and `counts`"))
                    .at(src, t.key_span("counts", span))
                    .hint(format!(
                        "remove `{k}` from `props`: `counts` makes it an integer prop (enabled `{prefab}` copies)"
                    )),
            );
        }
    }
    if def.footprint != [1, 1] {
        let fixed: Vec<&str> = [
            ("mover", def.mover.is_some()),
            ("patrol", def.patrol.is_some()),
            ("wander", def.wander.is_some()),
            ("pushable", def.pushable),
            ("screen_pos", def.screen_pos.is_some()),
        ]
        .into_iter()
        .filter(|(_, has)| *has)
        .map(|(k, _)| k)
        .collect();
        if !fixed.is_empty() {
            out.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!(
                        "{who} has `footprint = [{}, {}]`; only things that stay put can cover several cells, so it \
                         cannot have `{}`",
                        def.footprint[0],
                        def.footprint[1],
                        fixed.join("`, `")
                    ),
                )
                .at(src, t.key_span("footprint", span))
                .hint("remove `footprint`, or the behavior"),
            );
        }
    }
    // Its own shape list, or its prefab's against its own footprint (a
    // prefab's list is checked against the prefab's, in the prefab's file).
    if let Some(shapes) = &def.shapes {
        let mut push = |d: Diagnostic| out.push(d);
        if let Some(list) = t.get("shape") {
            let w = shape::Written { src, list: Some(list), span, allow: &def.allow };
            shape::list(shapes, def.footprint, &w, &mut push);
        } else if t.has("footprint") {
            let w = shape::Written { src, list: None, span: t.key_span("footprint", span), allow: &def.allow };
            shape::inside(shapes, def.footprint, &w, &mut push);
        }
    }
    if let Some(label) = &def.label {
        let at = t.table("label").map_or(span, |l| l.value_span("text", t.value_span("label", span)));
        match defs::parse_label(&label.text) {
            Err(why) => out.push(
                Diagnostic::new(&codes::INVALID_VALUE, format!("label text {:?}: {why}", label.text)).at(src, at),
            ),
            Ok(parts) => {
                for part in parts {
                    if let LabelPart::Prop(name) = part
                        && def.prop(&name).is_none()
                    {
                        out.push(unknown_prop(def, &name, &format!("label placeholder `{{{name}}}`"), src, at));
                    }
                }
            }
        }
        let odd: Vec<String> = {
            let mut v: Vec<char> = label.text.chars().filter(|c| !defs::font::supports(*c)).collect();
            v.sort_unstable();
            v.dedup();
            v.into_iter().map(|c| format!("{c:?}")).collect()
        };
        if !odd.is_empty() && !def.allow.iter().any(|a| a.eq_ignore_ascii_case(codes::UNSUPPORTED_CHAR.code)) {
            out.push(
                Diagnostic::new(
                    &codes::UNSUPPORTED_CHAR,
                    format!("label of {who} has characters the font cannot draw: {}", odd.join(", ")),
                )
                .at(src, at)
                .hint("they are drawn as a hollow box; the font has printable ASCII only"),
            );
        }
    }
    if def.pushable {
        let moving: Vec<&str> =
            [("mover", def.mover.is_some()), ("patrol", def.patrol.is_some()), ("wander", def.wander.is_some())]
                .into_iter()
                .filter(|(_, has)| *has)
                .map(|(k, _)| k)
                .collect();
        if !moving.is_empty() {
            out.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!(
                        "{who} is `pushable`, so it only moves when pushed; it cannot have `{}`",
                        moving.join("`, `")
                    ),
                )
                .at(src, t.key_span("pushable", span))
                .hint("remove the behavior, or remove `pushable`"),
            );
        }
    }
    if def.screen_pos.is_some() {
        let world_only: Vec<&str> = [
            ("shape", def.shapes.is_some()),
            ("turn", def.turn != 0),
            ("tile", def.tile.is_some()),
            ("block", def.block.is_some()),
            ("mover", def.mover.is_some()),
            ("patrol", def.patrol.is_some()),
            ("wander", def.wander.is_some()),
            ("trigger", def.trigger.is_some()),
            ("solid", def.solid),
            ("pushable", def.pushable),
        ]
        .into_iter()
        .filter(|(_, has)| *has)
        .map(|(k, _)| k)
        .collect();
        if !world_only.is_empty() {
            out.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!(
                        "{who} has `screen_pos`, so it is drawn on the screen, not in the world; it cannot have `{}`",
                        world_only.join("`, `")
                    ),
                )
                .at(src, t.key_span("screen_pos", span))
                .hint("screen entities can have `sprite` and `label`; put world parts on a separate entity"),
            );
        }
    }
}

/// Every `counts` entry names an existing prefab (E0201).
pub(crate) fn counts(vfs: &Vfs, def: &EntityDef, t: &Table, span: Span, src: &SourceFile, out: &mut Sink) {
    for (prop, prefab) in &def.counts {
        if prefab_exists(vfs, prefab) {
            continue;
        }
        let at = t.table("counts").map_or(span, |c| c.value_span(prop, t.value_span("counts", span)));
        let names = prefab_names(vfs);
        let hint = did_you_mean(prefab, names.iter().map(String::as_str))
            .unwrap_or_else(|| format!("create prefabs/{prefab}.toml"));
        out.push(
            Diagnostic::new(&codes::UNKNOWN_PREFAB, format!("`counts.{prop}` counts unknown prefab `{prefab}`"))
                .at(src, at)
                .hint(hint),
        );
    }
}

/// E0206 for `name` on `def`, suggesting a close prop or how to declare it.
pub(crate) fn unknown_prop(def: &EntityDef, name: &str, what: &str, src: &SourceFile, at: Span) -> Diagnostic {
    let who = def.name.as_deref().unwrap_or("the entity");
    let hint = did_you_mean(name, def.props.iter().map(|(k, _)| k.as_str()))
        .unwrap_or_else(|| format!("declare it with a starting value on `{who}`: `props = {{ {name} = 0 }}`"));
    Diagnostic::new(&codes::UNKNOWN_PROP, format!("{what} names prop `{name}`, but `{who}` has no such prop"))
        .at(src, at)
        .hint(hint)
}
