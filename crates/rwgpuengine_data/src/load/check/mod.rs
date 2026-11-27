//! Stage 3, check: each rule on its own, on a resolved scene.
//!
//! - [`entity`]: the rules on one entity's definition, and its `counts`;
//! - [`connect`]: a [[connect]]'s source, signal, the form of its actions,
//!   and each action against what it acts on;
//! - [`when`]: a [[connect]]'s `when` list;
//! - [`placement`]: where the entities stand when the scene starts.
//!
//! Nothing here looks anything up in the files but prefab names: what the
//! tables refer to was found in stage 2.

mod connect;
mod entity;
mod placement;
mod when;

pub(crate) use entity::unknown_prop;

use super::order::{At, Diags, EntityStep};
use super::parse::Parsed;
use super::resolve::Resolved;
use super::vfs::Vfs;

pub(crate) fn scene(vfs: &Vfs, parsed: &Parsed, r: &Resolved, out: &mut Diags) {
    if r.over_limit {
        return;
    }
    let src = &parsed.src;
    for t in &r.tables {
        let Some(i) = t.checked else { continue };
        let (def, span) = (&r.entities[i], t.node.span);
        entity::rules(def, t.table, span, src, &mut out.at(At::Entity(t.item, EntityStep::Rules)));
        entity::counts(vfs, def, t.table, span, src, &mut out.at(At::Entity(t.item, EntityStep::Counts)));
    }
    for c in &r.connections {
        connect::connection(vfs, c, r, src, out);
    }
    placement::placement(&r.entities, r.ground.as_ref(), src, out);
}
