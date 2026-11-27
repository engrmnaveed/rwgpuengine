//! Stage 2, resolve: what a scene's tables refer to.
//!
//! The ground and its terrains (`ground.rs`); every [[entity]] merged onto
//! its prefab and decoded, and the names they give; and for every
//! [[connect]], who emits, what each action acts on, which prefab a `spawn`
//! places and who pays for it. What cannot be found is reported here: an
//! unknown prefab (E0201) or entity (E0202), a name given twice (E0203), a
//! name with `cells` (E0205), and a prefab file's own problems, where the
//! prefab is first used. So are the scene's sizes (E0209): an [[entity]]
//! table that would take the scene over a limit is not decoded, and nothing
//! after it is resolved or checked. Everything else is checked in stage 3.

use std::collections::{BTreeMap, BTreeSet};

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::{Diagnostic, SourceFile, Span};
use rwgpuengine_core::limits::{self, Limit};
use rwgpuengine_core::text::did_you_mean;

use super::ground;
use super::order::{ActionStep, At, ConnectStep, Diags, EntityStep};
use super::parse::Parsed;
use super::scene::SceneLoader;
use crate::defs::{self, ActionDef, ConnectAction, ConnectDef, ConnectFrom, EntityDef, GroundDef, Origin};
use crate::node::{Node, Table};
use crate::schema::{self, Fields};

/// A scene with what its tables refer to.
pub(crate) struct Resolved<'p> {
    pub ground: Option<GroundDef>,
    /// Every entity, in file order: one per [[entity]] table, or one per
    /// cell of its `cells`.
    pub entities: Vec<EntityDef>,
    /// The [[entity]] tables that gave entities.
    pub tables: Vec<EntityTable<'p>>,
    /// Each name, and the first entity that gives it.
    pub index: BTreeMap<String, usize>,
    pub connections: Vec<Connection<'p>>,
    pub signals: Signals,
    /// Prefabs that `spawn` places, decoded at [0, 0, 0], in first-use order.
    pub templates: Vec<EntityDef>,
    /// An [[entity]] table would have taken the scene over a limit: the
    /// entities stop before it, and nothing more is resolved or checked.
    pub over_limit: bool,
}

/// An [[entity]] table that gave entities.
pub(crate) struct EntityTable<'p> {
    /// Its place in `entity`.
    pub item: usize,
    pub node: &'p Node,
    pub table: &'p Table,
    /// The entity its rules are checked on: its only one, or the first of
    /// its `cells` (`None` when `cells` lists none).
    pub checked: Option<usize>,
}

/// A [[connect]] table, decoded, with what it refers to.
pub(crate) struct Connection<'p> {
    /// Its place in `connect`.
    pub item: usize,
    pub node: &'p Node,
    pub table: &'p Table,
    pub def: ConnectDef,
    /// Who emits. `None` for the ground, and when it is not given or not
    /// found.
    pub source: Option<Source>,
    /// `action`, or a non-empty `actions` with nothing of an action beside
    /// it (`check::connect::form` reports the rest). Only then are its
    /// actions resolved and checked.
    pub well_formed: bool,
    /// Its actions, when `well_formed`.
    pub actions: Vec<Action<'p>>,
}

/// One action of a [[connect]], with what it refers to.
pub(crate) struct Action<'p> {
    /// The table that holds it: the [[connect]] itself, or an `actions` item.
    pub table: &'p Table,
    pub span: Span,
    /// What it changes (not for `spawn`): its `to`, or the source for `self`.
    pub target: Option<Target>,
    /// The prefab a `spawn` places, decoded.
    pub template: Option<EntityDef>,
    /// Who pays for a `spawn` with `cost` (an entity of the scene).
    pub payer: Option<usize>,
}

/// Who a [[connect]] listens to.
pub(crate) enum Source {
    /// An entity of the scene.
    Entity(usize),
    /// Every copy of a prefab: the prefab, decoded.
    Prefab(Box<EntityDef>),
}

/// What an action changes.
#[derive(Clone, Copy)]
pub(crate) enum Target {
    /// An entity of the scene.
    Entity(usize),
    /// The one that emitted (`to = "self"`).
    Source,
}

impl Resolved<'_> {
    /// The definition of a [[connect]]'s source.
    pub fn source<'a>(&'a self, c: &'a Connection) -> Option<&'a EntityDef> {
        match c.source.as_ref()? {
            Source::Entity(i) => Some(&self.entities[*i]),
            Source::Prefab(def) => Some(def),
        }
    }

    /// The definition of what an action changes.
    pub fn target<'a>(&'a self, c: &'a Connection, a: &Action) -> Option<&'a EntityDef> {
        match a.target? {
            Target::Entity(i) => Some(&self.entities[i]),
            Target::Source => self.source(c),
        }
    }

    /// The first entity named `name`.
    pub fn named(&self, name: &str) -> Option<&EntityDef> {
        self.index.get(name).map(|&i| &self.entities[i])
    }
}

/// What a scene's connections say about the signals entities emit (W0203).
pub(crate) struct Signals {
    /// Entities whose props a connection changes, by name (they emit `changed`).
    pub changed_names: BTreeSet<String>,
    /// Prefabs whose copies' props are changed through `to = "self"`.
    pub changed_prefabs: BTreeSet<String>,
    /// Prefabs that `spawn` places (their copies emit `spawned`).
    pub spawned: BTreeSet<String>,
    /// Sources of spawns: they emit `refused` when a placement fails. The
    /// ground, entities by name, and prefabs.
    pub ground_spawns: bool,
    pub spawning_names: BTreeSet<String>,
    pub spawning_prefabs: BTreeSet<String>,
    /// Prefabs with a copy, by name, in `changed_names`.
    pub changed_copies: BTreeSet<String>,
    /// Prefabs with a copy, by name, in `spawning_names`.
    pub spawning_copies: BTreeSet<String>,
}

impl Signals {
    fn of(decoded: &[ConnectDef], entities: &[EntityDef]) -> Signals {
        // Who emits `changed` (a connection changes their props) and which
        // prefabs get spawned (their copies emit `spawned`).
        let mut changed_names: BTreeSet<String> = BTreeSet::new();
        let mut changed_prefabs: BTreeSet<String> = BTreeSet::new();
        for (c, a) in decoded.iter().flat_map(|c| c.actions.iter().map(move |a| (c, a))) {
            // Paying a `cost` changes the payer's prop.
            if let Some(cost) = &a.cost {
                changed_names.insert(cost.entity.clone());
            }
            if !a.action.changes_prop() {
                continue;
            }
            match (a.to.as_deref(), &c.from) {
                (Some("self"), ConnectFrom::Entity(n)) => {
                    changed_names.insert(n.clone());
                }
                (Some("self"), ConnectFrom::Prefab(p)) => {
                    changed_prefabs.insert(p.clone());
                }
                (Some(n), _) => {
                    changed_names.insert(n.to_string());
                }
                _ => {}
            }
        }
        let is_spawn = |a: &&ActionDef| a.action == ConnectAction::Spawn;
        let spawned: BTreeSet<String> =
            decoded.iter().flat_map(|c| c.actions.iter().filter(is_spawn)).filter_map(|a| a.prefab.clone()).collect();
        let (mut ground_spawns, mut spawning_names, mut spawning_prefabs) = (false, BTreeSet::new(), BTreeSet::new());
        for c in decoded.iter().filter(|c| c.actions.iter().any(|a| is_spawn(&a))) {
            match &c.from {
                ConnectFrom::Ground => ground_spawns = true,
                ConnectFrom::Entity(n) => {
                    spawning_names.insert(n.clone());
                }
                ConnectFrom::Prefab(p) => {
                    spawning_prefabs.insert(p.clone());
                }
            }
        }
        let (mut changed_copies, mut spawning_copies) = (BTreeSet::new(), BTreeSet::new());
        for e in entities {
            if let (Some(p), Some(n)) = (&e.prefab, &e.name) {
                if changed_names.contains(n) {
                    changed_copies.insert(p.clone());
                }
                if spawning_names.contains(n) {
                    spawning_copies.insert(p.clone());
                }
            }
        }
        Signals {
            changed_names,
            changed_prefabs,
            spawned,
            ground_spawns,
            spawning_names,
            spawning_prefabs,
            changed_copies,
            spawning_copies,
        }
    }

    /// `from` starts a spawn, so it emits `refused`.
    pub fn refuses(&self, from: &ConnectFrom) -> bool {
        match from {
            ConnectFrom::Ground => self.ground_spawns,
            ConnectFrom::Entity(n) => self.spawning_names.contains(n),
            ConnectFrom::Prefab(p) => self.spawning_prefabs.contains(p),
        }
    }
}

pub(crate) fn scene<'p>(loader: &mut SceneLoader, parsed: &'p Parsed, out: &mut Diags) -> Resolved<'p> {
    let (src, root) = (&parsed.src, &parsed.root);
    let ground = ground::ground(root, src, &mut out.at(At::Ground));
    let (entities, tables, names, over_limit) = entities(loader, parsed, out);
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    for (i, e) in entities.iter().enumerate() {
        if let Some(n) = &e.name {
            index.entry(n.clone()).or_insert(i);
        }
    }
    let connect_nodes: Vec<(usize, &Node, &Table)> = root
        .array("connect")
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .filter_map(|(i, n)| Some((i, n, n.as_table()?)))
        .collect();
    let decoded: Vec<ConnectDef> =
        if over_limit { Vec::new() } else { connect_nodes.iter().map(|(_, _, t)| defs::decode_connect(t)).collect() };
    let signals = Signals::of(&decoded, &entities);
    let mut templates = Templates::default();
    let mut connections = Vec::new();
    for ((item, node, t), def) in connect_nodes.into_iter().zip(decoded) {
        let mut c = Connection { item, node, table: t, def, source: None, well_formed: false, actions: Vec::new() };
        connection(loader, &mut c, &entities, &index, &names, src, out, &mut templates);
        connections.push(c);
    }
    Resolved { ground, entities, tables, index, connections, signals, templates: templates.defs, over_limit }
}

/// What a scene has taken so far of the limits on its size.
#[derive(Default)]
struct Sizes {
    entities: u64,
    covered: u64,
    copied: u64,
}

impl Sizes {
    /// Adds `count` entities that each cover `area` cells and copy a prefab
    /// of `prefab` bytes. The first limit that is then gone over, and the
    /// new total.
    fn add(&mut self, count: u64, area: u64, prefab: u64) -> Option<(&'static Limit, u64)> {
        self.entities = self.entities.saturating_add(count);
        self.covered = self.covered.saturating_add(count.saturating_mul(area));
        self.copied = self.copied.saturating_add(count.saturating_mul(prefab));
        [
            (&limits::SCENE_ENTITIES, self.entities),
            (&limits::COVERED_CELLS, self.covered),
            (&limits::PREFAB_BYTES_COPIED, self.copied),
        ]
        .into_iter()
        .find(|(l, n)| *n > l.value)
    }
}

/// E0209 for the [[entity]] table that takes the scene over `limit`.
fn too_large(limit: &Limit, total: u64, t: &Table, src: &SourceFile, span: Span) -> Diagnostic {
    let max = limit.value;
    let (what, hint) = if *limit == limits::SCENE_ENTITIES {
        ("entities".to_string(), "each cell of `cells` is one entity".to_string())
    } else if *limit == limits::COVERED_CELLS {
        (
            "cells covered by its entities".to_string(),
            "a `footprint` covers its width times its height in cells".to_string(),
        )
    } else {
        let p = t.str("prefab").unwrap_or_default();
        (
            "bytes of prefabs copied into its entities".to_string(),
            format!("every entity made from `{p}` copies all of prefabs/{p}.toml: give large prefabs to few entities"),
        )
    };
    Diagnostic::new(
        &codes::TOO_LARGE,
        format!("this [[entity]] brings the scene to {total} {what}, more than a scene can have ({max})"),
    )
    .at(src, span)
    .hint(hint)
}

/// Every [[entity]] merged onto its prefab and decoded, the tables that gave
/// them, the names they give, and whether one took the scene over a limit.
fn entities<'p>(
    loader: &mut SceneLoader,
    parsed: &'p Parsed,
    out: &mut Diags,
) -> (Vec<EntityDef>, Vec<EntityTable<'p>>, BTreeMap<String, usize>, bool) {
    let (src, root) = (&parsed.src, &parsed.root);
    let path = src.path.as_str();
    let mut entities = Vec::new();
    let mut tables = Vec::new();
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    let mut sizes = Sizes::default();
    for (item, node) in root.array("entity").unwrap_or(&[]).iter().enumerate() {
        let Some(t) = node.as_table() else { continue };
        let at = |step| At::Entity(item, step);
        let merged = match t.str("prefab") {
            Some(name) if loader.prefab_exists(name) => match loader.prefab_at(name, out, at(EntityStep::Prefab)) {
                Some(p) => p.merged_with(t),
                None => continue, // prefab file has errors, already reported
            },
            Some(name) => {
                let names = loader.prefab_names();
                let hint = did_you_mean(name, names.iter().map(String::as_str))
                    .unwrap_or_else(|| format!("create prefabs/{name}.toml, or remove `prefab`"));
                out.at(at(EntityStep::Prefab)).push(
                    Diagnostic::new(&codes::UNKNOWN_PREFAB, format!("unknown prefab `{name}`"))
                        .at(src, t.value_span("prefab", node.span))
                        .hint(hint),
                );
                continue;
            }
            None => Table::default().merged_with(t),
        };
        let origin = Origin { file: path.to_string(), line: src.locate(node.span.start).0, span: node.span };
        let z = t.get("pos").and_then(schema::node_pos).map_or(0.0, |p| p[2]);
        let cells = match t.get("cells") {
            Some(_) if let Some(name) = t.str("name") => {
                out.at(at(EntityStep::Cells)).push(
                    Diagnostic::new(&codes::NAME_WITH_CELLS, format!("entity `{name}` has both `name` and `cells`"))
                        .at(src, t.key_span("name", node.span))
                        .hint("remove `name`, or use one [[entity]] with `pos` per named entity"),
                );
                continue;
            }
            Some(cells_node) => Some((cells_node, defs::cells_of(cells_node))),
            None => None,
        };
        let count = cells.as_ref().map_or(1, |(_, c)| c.len() as u64);
        let [w, h] = Fields::new(&merged, &schema::ENTITY).cell("footprint");
        let area = w.max(1) as u64 * h.max(1) as u64;
        let prefab = t.str("prefab").map_or(0, |p| loader.prefab_bytes(p));
        if let Some((limit, total)) = sizes.add(count, area, prefab) {
            out.at(at(EntityStep::Size)).push(too_large(limit, total, t, src, node.span));
            return (entities, tables, names, true);
        }
        if let Some((cells_node, cells)) = cells {
            let checked = (!cells.is_empty()).then_some(entities.len());
            for (i, c) in cells.into_iter().enumerate() {
                let span = cells_node.as_array().and_then(|a| a.get(i)).map_or(cells_node.span, |n| n.span);
                let origin = Origin { span, line: src.locate(span.start).0, ..origin.clone() };
                entities.push(defs::decode_entity(&merged, [c[0] as f32, c[1] as f32, z], origin));
            }
            tables.push(EntityTable { item, node, table: t, checked });
        } else {
            let pos = Fields::new(t, &schema::ENTITY).opt_pos("pos").unwrap_or([0.0, 0.0, 0.0]);
            let def = defs::decode_entity(&merged, pos, origin);
            if let Some(name) = &def.name {
                if let Some(&first_line) = names.get(name) {
                    out.at(at(EntityStep::Name)).push(
                        Diagnostic::new(&codes::DUPLICATE_NAME, format!("entity name `{name}` is used twice"))
                            .at(src, t.value_span("name", node.span))
                            .note(format!("first defined at {path}:{first_line}"))
                            .hint("rename one of them; names must be unique within a scene"),
                    );
                } else {
                    names.insert(name.clone(), src.locate(t.value_span("name", node.span).start).0);
                }
            }
            tables.push(EntityTable { item, node, table: t, checked: Some(entities.len()) });
            entities.push(def);
        }
    }
    (entities, tables, names, false)
}

/// `action`, or a non-empty `actions` with no key of an action beside it.
/// The same rule `check::connect::form` reports.
fn well_formed(t: &Table, c: &ConnectDef) -> bool {
    match (t.has("action"), t.has("actions")) {
        (true, false) => true,
        (false, true) => ACTION_KEYS.iter().all(|k| !t.has(k)) && !c.actions.is_empty(),
        _ => false,
    }
}

/// Keys of an action, which go inside the items of `actions`.
pub(crate) const ACTION_KEYS: [&str; 5] = ["to", "prop", "value", "prefab", "cost"];

/// The prefabs `spawn` places, in first-use order.
#[derive(Default)]
struct Templates {
    defs: Vec<EntityDef>,
    seen: BTreeSet<String>,
}

/// Who a [[connect]] listens to, and what its actions refer to.
#[allow(clippy::too_many_arguments)]
fn connection(
    loader: &mut SceneLoader,
    c: &mut Connection,
    entities: &[EntityDef],
    index: &BTreeMap<String, usize>,
    names: &BTreeMap<String, usize>,
    src: &SourceFile,
    out: &mut Diags,
    templates: &mut Templates,
) {
    let (t, span, item) = (c.table, c.node.span, c.item);
    let at = |step| At::Connect(item, step);
    // The source: one entity, every copy of a prefab, or the ground. With
    // both `from` and `from_prefab`, or neither, there is none
    // (`check::connect::source` reports it).
    c.source = match (t.has("from"), t.has("from_prefab")) {
        (true, true) | (false, false) => None,
        _ => match &c.def.from {
            ConnectFrom::Ground => None,
            ConnectFrom::Entity(name) => match index.get(name) {
                Some(&i) => Some(Source::Entity(i)),
                None => {
                    let hint = did_you_mean(name, names.keys().map(String::as_str))
                        .unwrap_or_else(|| format!("give an entity `name = \"{name}\"`"));
                    out.at(at(ConnectStep::Source)).push(
                        Diagnostic::new(
                            &codes::UNKNOWN_ENTITY,
                            format!("[[connect]] `from` names unknown entity `{name}`"),
                        )
                        .at(src, t.value_span("from", span))
                        .hint(hint),
                    );
                    None
                }
            },
            ConnectFrom::Prefab(p) => loader
                .prefab_def(p, src, t.value_span("from_prefab", span), out, at(ConnectStep::Source))
                .map(|def| Source::Prefab(Box::new(def))),
        },
    };
    c.well_formed = well_formed(t, &c.def);
    if !c.well_formed {
        return;
    }
    for (k, ((table, aspan), a)) in defs::action_tables(t, span).into_iter().zip(&c.def.actions).enumerate() {
        let step = |s| At::Connect(item, ConnectStep::Action(k, s));
        let mut action = Action { table, span: aspan, target: None, template: None, payer: None };
        if a.action == ConnectAction::Spawn {
            if let Some(p) = &a.prefab
                && let Some(def) =
                    loader.prefab_def(p, src, table.value_span("prefab", aspan), out, step(ActionStep::Prefab))
            {
                if templates.seen.insert(p.clone()) {
                    templates.defs.push(def.clone());
                }
                action.template = Some(def);
            }
            if let Some(cost) = &a.cost {
                action.payer = payer(cost, table, aspan, entities, index, src, &mut out.at(step(ActionStep::Cost)));
            }
        } else {
            match a.to.as_deref() {
                // No `to`, or the ground's `self`: reported by `check::connect::action`.
                None => {}
                Some("self") if c.def.from == ConnectFrom::Ground => {}
                Some("self") => action.target = Some(Target::Source),
                Some(name) => match index.get(name) {
                    Some(&i) => action.target = Some(Target::Entity(i)),
                    None => {
                        let names = entities.iter().filter_map(|e| e.name.as_deref());
                        let hint =
                            did_you_mean(name, names).unwrap_or_else(|| format!("give an entity `name = \"{name}\"`"));
                        out.at(step(ActionStep::Target)).push(
                            Diagnostic::new(
                                &codes::UNKNOWN_ENTITY,
                                format!("[[connect]] `to` names unknown entity `{name}`"),
                            )
                            .at(src, table.value_span("to", aspan))
                            .hint(hint),
                        );
                    }
                },
            }
        }
        c.actions.push(action);
    }
}

/// The entity a spawn's `cost` names, or E0202.
fn payer(
    cost: &defs::CostDef,
    t: &Table,
    span: Span,
    entities: &[EntityDef],
    index: &BTreeMap<String, usize>,
    src: &SourceFile,
    out: &mut super::order::Sink,
) -> Option<usize> {
    if let Some(&i) = index.get(cost.entity.as_str()) {
        return Some(i);
    }
    let at = t.table("cost").map_or(span, |c| c.value_span("entity", t.value_span("cost", span)));
    let names = entities.iter().filter_map(|e| e.name.as_deref());
    let hint = did_you_mean(&cost.entity, names)
        .unwrap_or_else(|| format!("give the paying entity `name = \"{}\"`", cost.entity));
    out.push(
        Diagnostic::new(&codes::UNKNOWN_ENTITY, format!("`cost` names unknown entity `{}`", cost.entity))
            .at(src, at)
            .hint(hint),
    );
    None
}
