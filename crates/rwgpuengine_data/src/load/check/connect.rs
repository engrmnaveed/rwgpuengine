//! The rules on one [[connect]]: its source, its signal, the form of its
//! actions, and each action against what it acts on.

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::{Diagnostic, SourceFile, Span};
use rwgpuengine_core::text::did_you_mean;

use super::super::order::{ActionStep, At, ConnectStep, Diags, Sink};
use super::super::resolve::{ACTION_KEYS, Action, Connection, Resolved};
use super::super::vfs::Vfs;
use super::entity::{self, unknown_prop};
use super::when;
use crate::defs::{self, ConnectAction, ConnectDef, ConnectFrom, EntityDef};
use crate::node::Table;

pub(crate) fn connection(vfs: &Vfs, c: &Connection, r: &Resolved, src: &SourceFile, out: &mut Diags) {
    let (t, span, item) = (c.table, c.node.span, c.item);
    let at = |step| At::Connect(item, step);
    source(t, span, src, &mut out.at(at(ConnectStep::Source)));
    signal(c, r, src, &mut out.at(at(ConnectStep::Signal)));
    let ok = form(t, span, &c.def, src, &mut out.at(at(ConnectStep::Form)));
    debug_assert_eq!(ok, c.well_formed, "resolve and check read the form of a [[connect]] alike");
    if c.well_formed {
        for (k, (a, ra)) in c.def.actions.iter().zip(&c.actions).enumerate() {
            action(vfs, a, &c.def.from, ra, r, src, out, item, k);
            let step = At::Connect(item, ConnectStep::Action(k, ActionStep::Logic));
            logic(a, &c.def.from, ra.table, ra.span, r.target(c, ra), src, &mut out.at(step));
        }
    }
    when::when(&c.def, t, r, src, &mut out.at(at(ConnectStep::When)));
}

/// Exactly one of `from` and `from_prefab`.
fn source(t: &Table, span: Span, src: &SourceFile, out: &mut Sink) {
    match (t.has("from"), t.has("from_prefab")) {
        (true, true) => out.push(
            Diagnostic::new(&codes::INVALID_VALUE, "[[connect]] has both `from` and `from_prefab`")
                .at(src, t.key_span("from_prefab", span))
                .hint("use `from` for one entity, `from_prefab` for every copy of a prefab"),
        ),
        (false, false) => out.push(
            Diagnostic::new(&codes::MISSING_KEY, "[[connect]] needs `from` or `from_prefab`").at(src, span).hint(
                "`from = \"<entity name>\"`, `from = \"ground\"` (taps on the ground) or `from_prefab = \"<prefab>\"`",
            ),
        ),
        _ => {}
    }
}

/// The source can emit the signal (W0203).
fn signal(c: &Connection, r: &Resolved, src: &SourceFile, out: &mut Sink) {
    let s = &r.signals;
    let (t, span) = (c.table, c.node.span);
    let emits: Option<Vec<String>> = match &c.def.from {
        ConnectFrom::Ground => {
            let mut v = vec!["tapped".to_string()];
            if s.refuses(&c.def.from) {
                v.push("refused".into());
            }
            Some(v)
        }
        ConnectFrom::Entity(n) => r.source(c).map(|e| {
            let mut v = vec!["tapped".to_string()];
            v.extend(e.trigger.iter().flat_map(|tr| [tr.on_enter.clone(), tr.on_exit.clone()]));
            if s.changed_names.contains(n) || !e.counts.is_empty() {
                v.push("changed".into());
            }
            if s.refuses(&c.def.from) {
                v.push("refused".into());
            }
            v
        }),
        ConnectFrom::Prefab(p) => r.source(c).map(|e| {
            let mut v = vec!["tapped".to_string()];
            v.extend(e.trigger.iter().flat_map(|tr| [tr.on_enter.clone(), tr.on_exit.clone()]));
            if s.changed_prefabs.contains(p) || s.changed_copies.contains(p) || !e.counts.is_empty() {
                v.push("changed".into());
            }
            if s.spawned.contains(p) {
                v.push("spawned".into());
            }
            if s.refuses(&c.def.from) || s.spawning_copies.contains(p) {
                v.push("refused".into());
            }
            v
        }),
    };
    let Some(emits) = emits else { return };
    let signal = &c.def.signal;
    if emits.contains(signal) {
        return;
    }
    let who = match &c.def.from {
        ConnectFrom::Entity(n) => format!("`{n}`"),
        ConnectFrom::Prefab(p) => format!("prefab `{p}`"),
        ConnectFrom::Ground => "the ground".to_string(),
    };
    let hint = did_you_mean(signal, emits.iter().map(String::as_str)).or_else(|| match signal.as_str() {
        "changed" => Some(format!(
            "{who} emits `changed` only when a [[connect]] `add`s to or `set`s one of its props, \
             or one of its `counts` changes"
        )),
        "spawned" => Some(format!("{who} emits `spawned` only for copies made by a `spawn` action")),
        "refused" => {
            Some(format!("{who} emits `refused` only when a `spawn` that one of its signals starts cannot be placed"))
        }
        _ => None,
    });
    out.push(
        Diagnostic::new(
            &codes::SIGNAL_NEVER_EMITTED,
            format!("{who} never emits `{signal}` (it emits: {})", emits.join(", ")),
        )
        .at(src, t.value_span("signal", span))
        .maybe_hint(hint),
    );
}

/// One action (`action = ...`) or several (`actions = [...]`). Returns
/// whether the actions can be checked.
fn form(t: &Table, span: Span, c: &ConnectDef, src: &SourceFile, out: &mut Sink) -> bool {
    match (t.has("action"), t.has("actions")) {
        (true, true) => {
            out.push(
                Diagnostic::new(&codes::INVALID_VALUE, "[[connect]] has both `action` and `actions`")
                    .at(src, t.key_span("actions", span))
                    .hint("use `action` for one action, or put every action in `actions = [{ ... }, ...]`"),
            );
            false
        }
        (false, false) => {
            out.push(
                Diagnostic::new(&codes::MISSING_KEY, "[[connect]] needs `action` (or `actions` for several)")
                    .at(src, span)
                    .hint(
                        "`action = \"disable\"` with `to = \"...\"`, or \
                         `actions = [{ action = \"disable\", to = \"...\" }, ...]`",
                    ),
            );
            false
        }
        (false, true) => {
            let mut ok = true;
            for key in ACTION_KEYS.into_iter().filter(|k| t.has(k)) {
                ok = false;
                out.push(
                    Diagnostic::new(
                        &codes::INVALID_VALUE,
                        format!("with `actions`, `{key}` goes inside the action it belongs to"),
                    )
                    .at(src, t.key_span(key, span))
                    .hint(format!("move `{key}` into one of the `actions` items")),
                );
            }
            if c.actions.is_empty() {
                ok = false;
                out.push(
                    Diagnostic::new(&codes::INVALID_VALUE, "`actions` is empty")
                        .at(src, t.value_span("actions", span))
                        .hint("list at least one `{ action = ... }`, or use `action`"),
                );
            }
            ok
        }
        (true, false) => true,
    }
}

/// The keys of one action: a `spawn`'s prefab (as a spawned entity) and
/// cost, or another action's `to`.
#[allow(clippy::too_many_arguments)]
fn action(
    vfs: &Vfs,
    a: &defs::ActionDef,
    from: &ConnectFrom,
    ra: &Action,
    r: &Resolved,
    src: &SourceFile,
    out: &mut Diags,
    item: usize,
    k: usize,
) {
    let (t, span) = (ra.table, ra.span);
    let at = |step| At::Connect(item, ConnectStep::Action(k, step));
    if a.action == ConnectAction::Spawn {
        if t.has("to") {
            out.at(at(ActionStep::SpawnTo)).push(
                Diagnostic::new(&codes::INVALID_VALUE, "`spawn` has no target: it places a new entity")
                    .at(src, t.key_span("to", span))
                    .hint("remove `to`; the new copy goes on the tapped cell or the emitter's cell"),
            );
        }
        match &a.prefab {
            None => out.at(at(ActionStep::Prefab)).push(
                Diagnostic::new(&codes::MISSING_KEY, "`action = \"spawn\"` needs `prefab`")
                    .at(src, t.value_span("action", span))
                    .hint("add `prefab = \"...\"`: the prefab file (prefabs/<name>.toml) to place"),
            ),
            Some(p) => {
                let pspan = t.value_span("prefab", span);
                if let Some(def) = &ra.template {
                    let none = Table::default();
                    entity::rules(def, &none, pspan, src, &mut out.at(at(ActionStep::PrefabRules)));
                    entity::counts(vfs, def, &none, pspan, src, &mut out.at(at(ActionStep::PrefabCounts)));
                    if def.screen_pos.is_some() {
                        out.at(at(ActionStep::PrefabScreen)).push(
                            Diagnostic::new(
                                &codes::INVALID_VALUE,
                                format!("prefab `{p}` has `screen_pos`, so it cannot be spawned in the world"),
                            )
                            .at(src, pspan),
                        );
                    }
                }
            }
        }
        if let (Some(cost), Some(payer)) = (&a.cost, ra.payer) {
            self::cost(cost, &r.entities[payer], t, span, src, &mut out.at(at(ActionStep::Cost)));
        }
    } else {
        for key in ["prefab", "cost"].into_iter().filter(|k| t.has(k)) {
            out.at(at(ActionStep::Keys)).push(
                Diagnostic::new(&codes::INVALID_VALUE, format!("`{key}` only applies to `action = \"spawn\"`"))
                    .at(src, t.key_span(key, span))
                    .hint(format!("remove `{key}`, or use `action = \"spawn\"`")),
            );
        }
        match a.to.as_deref() {
            None => out.at(at(ActionStep::Target)).push(
                Diagnostic::new(&codes::MISSING_KEY, format!("`action = \"{}\"` needs `to`", a.action.name()))
                    .at(src, span)
                    .hint("add `to = \"<entity name>\"`, or `to = \"self\"` for the entity that emitted"),
            ),
            Some("self") if *from == ConnectFrom::Ground => out.at(at(ActionStep::Target)).push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    "`to = \"self\"` with `from = \"ground\"`: the ground is not an entity",
                )
                .at(src, t.value_span("to", span))
                .hint("name the entity to change, or use `action = \"spawn\"` to place one on the tapped cell"),
            ),
            // Found, or reported unknown by resolve.
            Some(_) => {}
        }
    }
}

/// A spawn's `cost`: a number prop the payer declares (not a count), and an
/// amount above 0 that fits the prop's type.
fn cost(cost: &defs::CostDef, payer: &EntityDef, t: &Table, span: Span, src: &SourceFile, out: &mut Sink) {
    let table = t.table("cost");
    let at = |key: &str| table.map_or(span, |c| c.value_span(key, t.value_span("cost", span)));
    let prop = cost.prop.as_str();
    let Some(current) = payer.prop(prop) else {
        out.push(unknown_prop(payer, prop, "`cost`", src, at("prop")));
        return;
    };
    if let Some(prefab) = payer.counted(prop) {
        out.push(
            Diagnostic::new(
                &codes::INVALID_VALUE,
                format!("`cost` cannot pay from `{}`.{prop}: it counts `{prefab}` copies", cost.entity),
            )
            .at(src, at("prop"))
            .hint("pay from a prop declared in `props`, e.g. `props = { coins = 10 }`"),
        );
        return;
    }
    if current.as_num().is_none() {
        out.push(
            Diagnostic::new(
                &codes::PROP_TYPE,
                format!("`cost` needs a number prop, but `{}`.{prop} is {}", cost.entity, current.type_name()),
            )
            .at(src, at("prop")),
        );
        return;
    }
    if !table.is_some_and(|c| c.has("amount")) {
        return; // already reported as missing
    }
    if cost.amount.as_num().is_none_or(|n| n <= 0.0) {
        out.push(
            Diagnostic::new(
                &codes::INVALID_VALUE,
                format!("`cost` amount must be above 0, found {}", cost.amount.literal()),
            )
            .at(src, at("amount"))
            .hint("for something free, leave `cost` out"),
        );
    } else if !current.accepts(&cost.amount) {
        out.push(
            Diagnostic::new(
                &codes::PROP_TYPE,
                format!(
                    "`cost` amount {} cannot be paid from `{}`.{prop}, which is {}",
                    cost.amount.literal(),
                    cost.entity,
                    current.type_name()
                ),
            )
            .at(src, at("amount"))
            .hint("integer props pay whole numbers; declare `0.0` for decimals"),
        );
    }
}

/// Checks on one `add`/`set` action that need its target's definition.
/// `t` is the table holding the action (the [[connect]] or an `actions` item).
fn logic(
    c: &defs::ActionDef,
    from: &ConnectFrom,
    t: &Table,
    span: Span,
    target: Option<&EntityDef>,
    src: &SourceFile,
    out: &mut Sink,
) {
    let action = c.action.name();
    // How the target is named in messages: `hud`, or the prefab for `self`.
    let to_label = match (c.to.as_deref(), from) {
        (Some("self"), ConnectFrom::Prefab(p)) => format!("{p} (self)"),
        (Some("self"), ConnectFrom::Entity(n)) => n.clone(),
        (Some(n), _) => n.to_string(),
        (None, _) => "the target".to_string(),
    };
    if c.action.changes_prop() {
        match &c.prop {
            None => out.push(
                Diagnostic::new(&codes::MISSING_KEY, format!("`action = \"{action}\"` needs `prop`"))
                    .at(src, t.value_span("action", span))
                    .hint(format!("add `prop = \"...\"`: a prop declared in `{to_label}`'s props")),
            ),
            Some(prop) => {
                if let Some(target) = target
                    && let Some(prefab) = target.counted(prop)
                {
                    out.push(
                        Diagnostic::new(
                            &codes::INVALID_VALUE,
                            format!(
                                "`{action}` cannot change `{to_label}`.{prop}: it is a `counts` prop (the number of \
                                     enabled `{prefab}` copies), kept up to date by the engine"
                            ),
                        )
                        .at(src, t.value_span("prop", span))
                        .hint(format!(
                            "remove this [[connect]]; spawning, disabling or enabling a `{prefab}` changes it"
                        )),
                    );
                } else if let Some(target) = target {
                    match target.prop(prop) {
                        None => out.push(unknown_prop(
                            target,
                            prop,
                            &format!("`{action}`"),
                            src,
                            t.value_span("prop", span),
                        )),
                        Some(current) => {
                            let value_at = t.value_span("value", span);
                            match (c.action, &c.value) {
                                (ConnectAction::Add, _) if current.as_num().is_none() => out.push(
                                    Diagnostic::new(
                                        &codes::PROP_TYPE,
                                        format!(
                                            "`add` needs a number prop, but `{}`.{prop} is {}",
                                            to_label,
                                            current.type_name()
                                        ),
                                    )
                                    .at(src, t.value_span("prop", span))
                                    .hint("use `set` for text and flags"),
                                ),
                                (ConnectAction::Add, Some(v)) if !current.accepts(v) => out.push(
                                    Diagnostic::new(
                                        &codes::PROP_TYPE,
                                        format!(
                                            "`value = {}` cannot be added to `{}`.{prop}, which is {}",
                                            v.literal(),
                                            to_label,
                                            current.type_name()
                                        ),
                                    )
                                    .at(src, value_at)
                                    .hint("integer props count in whole numbers; declare `0.0` for decimals"),
                                ),
                                (ConnectAction::Set, None) => out.push(
                                    Diagnostic::new(&codes::MISSING_KEY, "`action = \"set\"` needs `value`")
                                        .at(src, t.value_span("action", span))
                                        .hint(format!("add `value = ...` ({})", current.type_name())),
                                ),
                                (ConnectAction::Set, Some(v)) if !current.accepts(v) => out.push(
                                    Diagnostic::new(
                                        &codes::PROP_TYPE,
                                        format!(
                                            "`value = {}` is {}, but `{}`.{prop} is {}",
                                            v.literal(),
                                            v.type_name(),
                                            to_label,
                                            current.type_name()
                                        ),
                                    )
                                    .at(src, value_at),
                                ),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    } else {
        for key in ["prop", "value"] {
            if t.has(key) {
                out.push(
                    Diagnostic::new(&codes::INVALID_VALUE, format!("`{key}` only applies to `add` and `set`"))
                        .at(src, t.key_span(key, span))
                        .hint(format!("remove `{key}`, or use `action = \"add\"` / `\"set\"`")),
                );
            }
        }
    }
}
