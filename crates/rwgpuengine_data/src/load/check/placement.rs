//! Where the entities stand when the scene starts: solids on one cell
//! (W0201), actors inside solids (W0206), entities outside the ground
//! (W0202). Warnings an entity's `allow` names are not reported.

use std::collections::BTreeMap;

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::{Diagnostic, SourceFile};

use super::super::order::{At, Diags, PlacementStep};
use crate::defs::{EntityDef, GroundDef};

fn allows(e: &EntityDef, code: &str) -> bool {
    e.allow.iter().any(|a| a.eq_ignore_ascii_case(code))
}

pub(crate) fn placement(entities: &[EntityDef], ground: Option<&GroundDef>, src: &SourceFile, out: &mut Diags) {
    let at = |i, step| At::Placement(i, step);
    // The first enabled solid in the file on each cell, that is not an actor.
    let mut walls: BTreeMap<[i32; 2], usize> = BTreeMap::new();
    for (j, o) in entities.iter().enumerate() {
        if o.enabled && o.blocks() && !o.is_actor() {
            for cell in o.cells() {
                walls.entry(cell).or_insert(j);
            }
        }
    }
    // The solids seen so far, on each cell.
    let mut solid_cells: BTreeMap<[i32; 2], usize> = BTreeMap::new();
    for (i, e) in entities.iter().enumerate() {
        let cells = e.cells();
        // Two enabled solids on one cell: the later one is reported, once.
        if e.enabled && e.blocks() {
            for &cell in &cells {
                if let Some(&j) = solid_cells.get(&cell) {
                    let other = &entities[j];
                    if allows(e, codes::SOLID_OVERLAP.code) || allows(other, codes::SOLID_OVERLAP.code) {
                        continue;
                    }
                    out.at(at(i, PlacementStep::Overlap)).push(
                        Diagnostic::new(
                            &codes::SOLID_OVERLAP,
                            format!("two solid entities on cell [{}, {}]", cell[0], cell[1]),
                        )
                        .at(src, e.origin.span)
                        .note(format!("the other one is at {}:{}", other.origin.file, other.origin.line)),
                    );
                    break;
                }
                solid_cells.insert(cell, i);
            }
        }
        // An enabled actor on a cell of an enabled solid that is not an actor.
        let cell = e.cell();
        if e.enabled
            && e.is_actor()
            && !allows(e, codes::ACTOR_IN_SOLID.code)
            && let Some(&j) = walls.get(&cell)
        {
            let wall = &entities[j];
            let what = |x: &EntityDef| x.name.as_deref().map_or("an entity".to_string(), |n| format!("`{n}`"));
            out.at(at(i, PlacementStep::InSolid)).push(
                Diagnostic::new(
                    &codes::ACTOR_IN_SOLID,
                    format!("actor {} starts inside solid {} on cell [{}, {}]", what(e), what(wall), cell[0], cell[1]),
                )
                .at(src, e.origin.span)
                .note(format!("the solid one is at {}:{}", wall.origin.file, wall.origin.line)),
            );
        }
        // A cell of the entity (or of its footprint) outside the ground.
        if let Some(g) = ground
            && e.screen_pos.is_none()
            && let Some(out_cell) = cells.iter().find(|c| !g.contains(**c)).copied()
            && !allows(e, codes::OUTSIDE_GROUND.code)
        {
            let whose = if out_cell == cell { String::new() } else { " (part of its footprint)".to_string() };
            out.at(at(i, PlacementStep::Outside)).push(
                Diagnostic::new(
                    &codes::OUTSIDE_GROUND,
                    format!(
                        "{} is on cell [{}, {}]{whose}, outside the {}x{} ground",
                        e.name.as_deref().map_or("entity".to_string(), |n| format!("`{n}`")),
                        out_cell[0],
                        out_cell[1],
                        g.size[0],
                        g.size[1]
                    ),
                )
                .at(src, e.origin.span),
            );
        }
    }
}
