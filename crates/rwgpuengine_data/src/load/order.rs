//! Where each problem of a scene stands in the order agents read them.
//!
//! A scene's problems are reported in the order of its file: the file as a
//! whole first (its syntax, then the schema), then the ground, then each
//! [[entity]] table, then each [[connect]] table, and last where the
//! entities stand. Within one item, in the order of the steps below. The
//! stages that find problems (resolve, then check) each run over the whole
//! scene; [`Diags::into_sorted`] puts what they found back in this order.
//! Problems with the same place keep the order they were found in.
//!
//! `rwgpu check` then sorts every file's problems by line and column
//! (`diag::sort`). `rwgpu run`, `shot`, `inspect` and the players show a
//! scene's problems in this order.

use rwgpuengine_core::diag::Diagnostic;

/// The place of a problem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum At {
    /// The file: its syntax and the schema.
    File,
    /// `[ground]` and the `[[terrain]]` tables.
    Ground,
    /// The `n`-th item of `entity` (counted from 0, tables or not).
    Entity(usize, EntityStep),
    /// The `n`-th item of `connect`.
    Connect(usize, ConnectStep),
    /// The `n`-th entity of the scene (a `cells` table gives several).
    Placement(usize, PlacementStep),
}

/// The steps of one [[entity]] table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum EntityStep {
    /// Its prefab: unknown, or the prefab file's own problems when this is
    /// the first use of it.
    Prefab,
    /// `name` with `cells`.
    Cells,
    /// The rules on its merged definition (`check::entity::rules`).
    Rules,
    /// Its `counts`.
    Counts,
    /// Its name, used before.
    Name,
}

/// The steps of one [[connect]] table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ConnectStep {
    /// `from` or `from_prefab`, and who that is.
    Source,
    /// Whether the source can emit `signal`.
    Signal,
    /// `action` or `actions`, and the keys beside `actions`.
    Form,
    /// The `n`-th action (the [[connect]] itself, or an item of `actions`).
    Action(usize, ActionStep),
    /// The `when` list.
    When,
}

/// The steps of one action. A `spawn` has the first six, every other
/// action the next two; both end with `Logic`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ActionStep {
    /// A `spawn` with `to`.
    SpawnTo,
    /// The `spawn`'s prefab: missing, unknown, or its file's problems.
    Prefab,
    /// The rules on the prefab, as a spawned entity.
    PrefabRules,
    /// The prefab's `counts`.
    PrefabCounts,
    /// The prefab is drawn on the screen.
    PrefabScreen,
    /// The `spawn`'s `cost`.
    Cost,
    /// `prefab` or `cost` on another action.
    Keys,
    /// `to`: missing, the ground's `self`, or unknown.
    Target,
    /// `prop` and `value` against the target (`check::connect::logic`).
    Logic,
}

/// The steps of where one entity stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PlacementStep {
    /// On a cell of another solid (W0201).
    Overlap,
    /// An actor inside a solid (W0206).
    InSolid,
    /// Outside the ground (W0202).
    Outside,
}

/// Problems found so far, each with its place.
#[derive(Debug, Default)]
pub(crate) struct Diags {
    items: Vec<(At, Diagnostic)>,
}

impl Diags {
    /// A sink that reports at `at`.
    pub fn at(&mut self, at: At) -> Sink<'_> {
        Sink { diags: self, at }
    }

    pub fn extend(&mut self, at: At, diags: impl IntoIterator<Item = Diagnostic>) {
        self.items.extend(diags.into_iter().map(|d| (at, d)));
    }

    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|(_, d)| d.is_error())
    }

    /// In the order agents read them.
    pub fn into_sorted(mut self) -> Vec<Diagnostic> {
        // Stable: problems with the same place keep the order they were found in.
        self.items.sort_by_key(|(at, _)| *at);
        self.items.into_iter().map(|(_, d)| d).collect()
    }
}

/// Reports problems at one place.
pub(crate) struct Sink<'a> {
    diags: &'a mut Diags,
    at: At,
}

impl Sink<'_> {
    pub fn push(&mut self, d: Diagnostic) {
        self.diags.items.push((self.at, d));
    }
}
