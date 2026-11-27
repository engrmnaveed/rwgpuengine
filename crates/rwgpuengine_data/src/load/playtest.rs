//! Playtests: parsed (stage 1), then each table decoded with its own rules;
//! and the checks that need the playtest's scene, which `rwgpu check` runs.

use std::collections::BTreeMap;

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::{Diagnostic, Span};
use rwgpuengine_core::text::did_you_mean;

use super::check::unknown_prop;
use super::parse::{Parsed, parse_checked};
use super::scene::{prefab_exists, prefab_names};
use super::vfs::Vfs;
use crate::defs::{self, PlaytestDef, SceneDef};
use crate::node::Table;
use crate::schema::{self, Fields};

/// Load a playtest. Returns `None` if it has errors. Scene references are
/// checked by [`check_project`].
pub fn load_playtest(vfs: &Vfs, path: &str) -> (Option<PlaytestDef>, Vec<Diagnostic>) {
    let (def, diags) = load_playtest_lenient(vfs, path);
    let failed = diags.iter().any(Diagnostic::is_error);
    (if failed { None } else { def }, diags)
}

/// Like [`load_playtest`], but returns whatever could be decoded even when
/// there are errors, so `check` can report cross-file problems in one pass.
/// Parsed (stage 1), then each table decoded with its own rules.
pub(crate) fn load_playtest_lenient(vfs: &Vfs, path: &str) -> (Option<PlaytestDef>, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let Some(Parsed { src, root, .. }) = parse_checked(vfs, path, &schema::PLAYTEST_FILE, &mut diags) else {
        if !vfs.contains(path) {
            diags.push(Diagnostic::new(&codes::FILE_NOT_FOUND, format!("playtest `{path}` not found")));
        }
        return (None, diags);
    };
    let empty = Table::default();
    let test_t = root.table("test").unwrap_or(&empty);
    let test = Fields::new(test_t, &schema::TEST);
    let frames = test.int("frames") as u64;

    let mut inputs = Vec::new();
    for node in root.array("input").unwrap_or(&[]) {
        let Some(t) = node.as_table() else { continue };
        let kinds: Vec<&str> = ["press", "hold", "tap", "camera"].into_iter().filter(|k| t.has(k)).collect();
        if kinds.len() != 1 {
            let found = if kinds.is_empty() { "none".to_string() } else { kinds.join(" and ") };
            diags.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!("[[input]] needs exactly one of `press`, `hold`, `tap` or `camera` (found {found})"),
                )
                .at(&src, node.span)
                .hint("split it into separate [[input]] tables"),
            );
            continue;
        }
        if t.has("for") && !t.has("hold") {
            diags.push(
                Diagnostic::new(&codes::INVALID_VALUE, "`for` only applies to `hold`")
                    .at(&src, t.key_span("for", node.span))
                    .hint("use `hold = \"...\"` with `for`, or remove `for`"),
            );
        }
        if let Some(input) = defs::decode_input(t) {
            if input.frame >= frames {
                diags.push(
                    Diagnostic::new(
                        &codes::INPUT_AFTER_END,
                        format!("input at frame {} never happens: the playtest runs frames 0..{frames}", input.frame),
                    )
                    .at(&src, t.value_span("frame", node.span)),
                );
            }
            inputs.push(input);
        }
    }

    let mut expects = Vec::new();
    for node in root.array("expect").unwrap_or(&[]) {
        let Some(t) = node.as_table() else { continue };
        let line = src.locate(node.span.start).0;
        if t.has("count") && !t.has("prefab") {
            diags.push(
                Diagnostic::new(&codes::INVALID_VALUE, "`count` only applies with `prefab`")
                    .at(&src, t.key_span("count", node.span))
                    .hint("add `prefab = \"...\"` to count its copies; count events with `event` and `at_least`/`at_most`"),
            );
            continue;
        }
        if t.has("prefab") && t.has("event") {
            // Events about a prefab: `count`/`pos` belong to copy counts.
            let wrong: Vec<&str> =
                ["count", "pos", "enabled", "solid", "props", "text"].into_iter().filter(|k| t.has(k)).collect();
            if !wrong.is_empty() {
                diags.push(
                    Diagnostic::new(
                        &codes::INVALID_VALUE,
                        format!(
                            "[[expect]] with `event` and `prefab` counts events about the prefab; it cannot use `{}`",
                            wrong.join("`, `")
                        ),
                    )
                    .at(&src, node.span)
                    .hint(
                        "count events with `at_least`/`at_most`; count copies in a separate [[expect]] without `event`",
                    ),
                );
                continue;
            }
        } else if t.has("prefab") {
            let extra: Vec<&str> =
                ["entity", "enabled", "solid", "props", "text"].into_iter().filter(|k| t.has(k)).collect();
            if !extra.is_empty() {
                diags.push(
                    Diagnostic::new(
                        &codes::INVALID_VALUE,
                        format!("[[expect]] with `prefab` counts copies; it cannot use `{}`", extra.join("`, `")),
                    )
                    .at(&src, node.span)
                    .hint("a copy count takes `count` and optionally `pos` (add `event` to count events about the prefab instead)"),
                );
                continue;
            }
            for key in ["other", "at_most", "at_least", "signal", "reason"] {
                if t.has(key) {
                    diags.push(
                        Diagnostic::new(&codes::INVALID_VALUE, format!("`{key}` only applies to event expectations"))
                            .at(&src, t.key_span(key, node.span))
                            .hint("use `count` to check how many copies exist"),
                    );
                }
            }
            expects.push(defs::decode_expect(t, node.span, line));
            continue;
        }
        let state_keys: Vec<&str> =
            ["pos", "enabled", "solid", "props", "text"].into_iter().filter(|k| t.has(k)).collect();
        if !t.has("event") && (!t.has("entity") || state_keys.is_empty()) {
            diags.push(Diagnostic::new(&codes::INVALID_VALUE, "[[expect]] checks nothing").at(&src, node.span).hint(
                "use `entity` with `pos`/`enabled`/`solid`/`props`/`text`, `prefab` with `count`, or `event` (optionally \
                 with `entity`/`signal`)",
            ));
            continue;
        }
        if t.has("event") && !state_keys.is_empty() {
            diags.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!("[[expect]] mixes `event` with `{}`", state_keys.join("`, `")),
                )
                .at(&src, node.span)
                .hint("split it into two [[expect]] tables"),
            );
            continue;
        }
        for key in ["other", "at_most", "at_least", "signal"] {
            if t.has(key) && !t.has("event") {
                diags.push(
                    Diagnostic::new(&codes::INVALID_VALUE, format!("`{key}` only applies to event expectations"))
                        .at(&src, t.key_span(key, node.span))
                        .hint("add `event = \"...\"`, or remove it"),
                );
            }
        }
        if let (Some(lo), Some(hi)) = (t.int("at_least"), t.int("at_most"))
            && lo > hi
        {
            diags.push(
                Diagnostic::new(&codes::INVALID_VALUE, format!("`at_least = {lo}` is more than `at_most = {hi}`"))
                    .at(&src, t.value_span("at_least", node.span)),
            );
        }
        if t.has("changed") && t.str("event") != Some("action") {
            diags.push(
                Diagnostic::new(&codes::INVALID_VALUE, "`changed` only applies to `event = \"action\"`")
                    .at(&src, t.key_span("changed", node.span))
                    .hint("add `event = \"action\"`, or remove `changed`"),
            );
        }
        if t.has("reason") && t.str("event") != Some("blocked") {
            diags.push(
                Diagnostic::new(&codes::INVALID_VALUE, "`reason` only applies to `event = \"blocked\"`")
                    .at(&src, t.key_span("reason", node.span))
                    .hint("add `event = \"blocked\"`, or remove `reason`"),
            );
        }
        if let Some(f) = t.int("frame")
            && f as u64 > frames
        {
            diags.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!("expect at frame {f} is after the last frame ({frames})"),
                )
                .at(&src, t.value_span("frame", node.span)),
            );
        }
        expects.push(defs::decode_expect(t, node.span, line));
    }

    let mut shots = Vec::new();
    for node in root.array("shot").unwrap_or(&[]) {
        let Some(t) = node.as_table() else { continue };
        shots.push(defs::decode_shot(t, src.locate(node.span.start).0));
    }

    let name = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".toml").to_string();
    let def = PlaytestDef {
        path: path.to_string(),
        name,
        doc: test.opt_str("doc"),
        scene: test.opt_str("scene"),
        scene_span: test_t.value_span("scene", Span::default()),
        frames,
        seed: test.opt_int("seed").map(|s| s as u64),
        inputs,
        expects,
        shots,
        src,
    };
    (Some(def), diags)
}

/// A scene's names, for the playtests that run it: in file order (for
/// hints), and each with the first entity that gives it.
pub(crate) struct Names {
    order: Vec<String>,
    first: BTreeMap<String, usize>,
}

impl Names {
    pub fn of(scene: &SceneDef) -> Names {
        let mut first = BTreeMap::new();
        for (i, e) in scene.entities.iter().enumerate() {
            if let Some(n) = &e.name {
                first.entry(n.clone()).or_insert(i);
            }
        }
        Names { order: scene.entities.iter().filter_map(|e| e.name.clone()).collect(), first }
    }

    fn contains(&self, name: &str) -> bool {
        self.first.contains_key(name)
    }

    fn iter(&self) -> impl Iterator<Item = &str> {
        self.order.iter().map(String::as_str)
    }
}

/// The checks of a playtest's `[[expect]]`s that need its scene: the prefabs
/// and entities they name, and the props and labels they read.
pub(crate) fn expectations(
    vfs: &Vfs,
    def: &PlaytestDef,
    scene: &SceneDef,
    names: &Names,
    scene_path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for exp in &def.expects {
        if let Some(p) = &exp.prefab
            && !prefab_exists(vfs, p)
        {
            let all = prefab_names(vfs);
            let hint = did_you_mean(p, all.iter().map(String::as_str))
                .unwrap_or_else(|| format!("there is no prefabs/{p}.toml"));
            diagnostics.push(
                Diagnostic::new(&codes::UNKNOWN_PREFAB, format!("[[expect]] counts unknown prefab `{p}`"))
                    .at(&def.src, exp.span)
                    .hint(hint),
            );
        }
        if let Some(name) = &exp.other
            && !names.contains(name)
        {
            let hint = did_you_mean(name, names.iter())
                .unwrap_or_else(|| format!("{scene_path} has no entity named `{name}`"));
            diagnostics.push(
                Diagnostic::new(&codes::UNKNOWN_ENTITY, format!("[[expect]] `other` names unknown entity `{name}`"))
                    .at(&def.src, exp.other_span)
                    .hint(hint),
            );
        }
        if let Some(name) = &exp.entity
            && !names.contains(name)
        {
            let hint = did_you_mean(name, names.iter())
                .unwrap_or_else(|| format!("{scene_path} has no entity named `{name}`"));
            diagnostics.push(
                Diagnostic::new(&codes::UNKNOWN_ENTITY, format!("[[expect]] names unknown entity `{name}`"))
                    .at(&def.src, exp.span)
                    .hint(hint),
            );
        }
        let Some(target) = exp.entity.as_deref().and_then(|n| names.first.get(n)).map(|&i| &scene.entities[i]) else {
            continue;
        };
        let at = Span::new(exp.span.start, exp.span.start);
        for (k, v) in &exp.props {
            match target.prop(k) {
                None => diagnostics.push(unknown_prop(target, k, "[[expect]] props", &def.src, exp.span)),
                Some(cur) if !cur.accepts(v) && !v.accepts(cur) => diagnostics.push(
                    Diagnostic::new(
                        &codes::PROP_TYPE,
                        format!(
                            "[[expect]] props.{k} = {} is {}, but `{}`.{k} is {}",
                            v.literal(),
                            v.type_name(),
                            target.name.as_deref().unwrap_or("?"),
                            cur.type_name()
                        ),
                    )
                    .at(&def.src, exp.span),
                ),
                Some(_) => {}
            }
        }
        if exp.text.is_some() && target.label.is_none() {
            diagnostics.push(
                Diagnostic::new(
                    &codes::INVALID_VALUE,
                    format!(
                        "[[expect]] `text` checks a label, but `{}` has no label",
                        target.name.as_deref().unwrap_or("?")
                    ),
                )
                .at(&def.src, at)
                .hint("check `props` instead, or add `label = { text = \"...\" }` to the entity"),
            );
        }
    }
}
