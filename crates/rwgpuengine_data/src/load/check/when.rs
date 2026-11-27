//! The rules on a [[connect]]'s `when` list.

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::{Diagnostic, SourceFile};
use rwgpuengine_core::text::did_you_mean;

use super::super::order::Sink;
use super::super::resolve::Resolved;
use super::entity::unknown_prop;
use crate::defs::{self, CondTest};
use crate::node::{Node, Table};

/// Each `when` item: one test of one named entity's state, or a `reason`
/// alone on `refused`.
pub(crate) fn when(c: &defs::ConnectDef, t: &Table, r: &Resolved, src: &SourceFile, out: &mut Sink) {
    for item in t.get("when").and_then(Node::as_array).unwrap_or(&[]) {
        let Some(w) = item.as_table() else { continue };
        let cond = defs::decode_condition(w);
        if w.has("reason") {
            let others: Vec<&str> = ["entity", "occupied", "enabled", "prop", "equals", "at_least", "at_most"]
                .into_iter()
                .filter(|k| w.has(k))
                .collect();
            if !others.is_empty() {
                out.push(
                    Diagnostic::new(
                        &codes::INVALID_VALUE,
                        format!("a `reason` condition stands alone (found `{}` too)", others.join("`, `")),
                    )
                    .at(src, item.span)
                    .hint("write it as its own item: when = [{ reason = \"terrain\" }, { entity = ... }]"),
                );
            }
            if c.signal != "refused" {
                out.push(
                    Diagnostic::new(
                        &codes::INVALID_VALUE,
                        format!(
                            "`reason` only applies with `signal = \"refused\"`; this [[connect]] listens to `{}`",
                            c.signal
                        ),
                    )
                    .at(src, w.key_span("reason", item.span))
                    .hint("listen to `refused` (emitted when a spawn cannot be placed), or remove `reason`"),
                );
            }
            continue;
        }
        if cond.entity.is_empty() {
            out.push(
                Diagnostic::new(&codes::MISSING_KEY, "a `when` condition needs `entity`")
                    .at(src, item.span)
                    .hint("`{ entity = \"<name>\", ... }`; only a `reason` condition (on `refused`) has no entity"),
            );
            continue;
        }
        let Some(target) = r.named(&cond.entity) else {
            if !cond.entity.is_empty() {
                let names = r.entities.iter().filter_map(|e| e.name.as_deref());
                let hint = did_you_mean(&cond.entity, names)
                    .unwrap_or_else(|| format!("give an entity `name = \"{}\"`", cond.entity));
                out.push(
                    Diagnostic::new(
                        &codes::UNKNOWN_ENTITY,
                        format!("`when` condition names unknown entity `{}`", cond.entity),
                    )
                    .at(src, w.value_span("entity", item.span))
                    .hint(hint),
                );
            }
            continue;
        };
        let kinds: Vec<&str> = ["occupied", "enabled", "prop"].into_iter().filter(|k| w.has(k)).collect();
        if kinds.len() != 1 {
            let found = if kinds.is_empty() { "none".to_string() } else { kinds.join(" and ") };
            out.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!("a `when` condition needs exactly one of `occupied`, `enabled` or `prop` (found {found})"),
                )
                .at(src, item.span)
                .hint("write one condition per test: when = [{ ... }, { ... }]"),
            );
            continue;
        }
        let compares: Vec<&str> = ["equals", "at_least", "at_most"].into_iter().filter(|k| w.has(k)).collect();
        let CondTest::Prop { prop, equals, at_least, at_most } = &cond.test else {
            for k in compares {
                out.push(
                    Diagnostic::new(&codes::INVALID_VALUE, format!("`{k}` only applies with `prop`"))
                        .at(src, w.key_span(k, item.span)),
                );
            }
            continue;
        };
        if compares.is_empty() {
            out.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!("condition on `{}`.{prop} compares nothing", cond.entity),
                )
                .at(src, item.span)
                .hint("add `equals = ...`, `at_least = N` or `at_most = N`"),
            );
            continue;
        }
        let Some(current) = target.prop(prop) else {
            out.push(unknown_prop(target, prop, "`when` condition", src, w.value_span("prop", item.span)));
            continue;
        };
        if (at_least.is_some() || at_most.is_some()) && current.as_num().is_none() {
            let k = if at_least.is_some() { "at_least" } else { "at_most" };
            out.push(
                Diagnostic::new(
                    &codes::PROP_TYPE,
                    format!("`{k}` needs a number prop, but `{}`.{prop} is {}", cond.entity, current.type_name()),
                )
                .at(src, w.key_span(k, item.span))
                .hint("use `equals` for text and flags"),
            );
        }
        if let Some(v) = equals
            && !current.accepts(v)
        {
            out.push(
                Diagnostic::new(
                    &codes::PROP_TYPE,
                    format!(
                        "`equals = {}` is {}, but `{}`.{prop} is {}",
                        v.literal(),
                        v.type_name(),
                        cond.entity,
                        current.type_name()
                    ),
                )
                .at(src, w.value_span("equals", item.span)),
            );
        }
        if let (Some(lo), Some(hi)) = (at_least, at_most)
            && lo > hi
        {
            out.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!("`at_least = {lo}` is more than `at_most = {hi}`, so this condition never holds"),
                )
                .at(src, w.value_span("at_least", item.span)),
            );
        }
    }
}
