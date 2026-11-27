//! The rules on one `[[shape]]` list: colors that are not see-through
//! (E0105), shapes inside the footprint (W0208), and no two upright sides in
//! one plane (W0209).
//!
//! A list is checked where it is written: a prefab's when its file is
//! loaded, an entity's own with the entity. An entity that takes its
//! prefab's list and sets its own `footprint` is checked against that.

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::{Diagnostic, SourceFile, Span};

use crate::defs::{ShapeDef, ShapeKind};
use crate::node::Node;

/// Where a list is written, for the places of its problems.
pub(crate) struct Written<'a> {
    pub src: &'a SourceFile,
    /// The `shape` value, if the list is in this file.
    pub list: Option<&'a Node>,
    /// Where to point when it is not (the entity, or its `footprint`).
    pub span: Span,
    /// Warning codes the list's owner allows.
    pub allow: &'a [String],
}

impl Written<'_> {
    /// The place of `key` in shape `i`, or the shape, or the list's owner.
    fn at(&self, i: usize, key: &str) -> Span {
        let Some(item) = self.list.and_then(Node::as_array).and_then(|a| a.get(i)) else { return self.span };
        item.as_table().map_or(item.span, |t| t.value_span(key, item.span))
    }

    fn allows(&self, code: &str) -> bool {
        self.allow.iter().any(|a| a.eq_ignore_ascii_case(code))
    }
}

/// Every rule, on a list where it is written.
pub(crate) fn list(shapes: &[ShapeDef], footprint: [i32; 2], w: &Written, out: &mut dyn FnMut(Diagnostic)) {
    for (i, s) in shapes.iter().enumerate() {
        if s.color.a < 255 {
            out(Diagnostic::new(
                &codes::INVALID_VALUE,
                format!("the `color` of a shape cannot be see-through, found \"{}\"", s.color.to_hex()),
            )
            .at(w.src, w.at(i, "color"))
            .hint("write it without the last two digits: \"#rrggbb\""));
        }
    }
    inside(shapes, footprint, w, out);
    if w.allows(codes::SHAPES_IN_ONE_PLANE.code) {
        return;
    }
    for (j, s) in shapes.iter().enumerate() {
        if let Some(i) = shapes[..j].iter().position(|o| shared_plane(o, s)) {
            out(Diagnostic::new(
                &codes::SHAPES_IN_ONE_PLANE,
                format!(
                    "this {} and the {} above it (shape {} of the list) each have an upright side in the same \
                     plane, facing the same way, and the two overlap",
                    s.kind.name(),
                    shapes[i].kind.name(),
                    i + 1
                ),
            )
            .at(w.src, w.at(j, "at"))
            .hint("no rule says which shows: move one of them a little with `at`, for example by 0.05"));
        }
    }
}

/// W0208: every shape stays over the cells of the footprint.
pub(crate) fn inside(shapes: &[ShapeDef], footprint: [i32; 2], w: &Written, out: &mut dyn FnMut(Diagnostic)) {
    if w.allows(codes::SHAPE_OUTSIDE_FOOTPRINT.code) {
        return;
    }
    // Sizes are written in decimals: a sum of two may be a hair past a whole cell.
    const SLACK: f64 = 1.0e-4;
    for (i, s) in shapes.iter().enumerate() {
        let outside =
            |k: usize| (s.at[k] as f64) < -SLACK || s.at[k] as f64 + s.size[k] as f64 > footprint[k] as f64 + SLACK;
        if outside(0) || outside(1) {
            out(Diagnostic::new(
                &codes::SHAPE_OUTSIDE_FOOTPRINT,
                format!(
                    "this {} reaches outside the {} x {} cells of its footprint",
                    s.kind.name(),
                    footprint[0],
                    footprint[1]
                ),
            )
            .at(w.src, w.at(i, "size"))
            .hint(
                "placement and taps go by the footprint: move the shape (`at`), make it smaller, or set \
                 `footprint`; `allow = [\"W0208\"]` if it is meant",
            ));
        }
    }
}

/// Which of its box's four upright sides (-x, +x, -y, +y) a shape fills with
/// a flat side of its own. A roof has its two gable ends, a wedge its high
/// side and its two flanks; round shapes have none. A triangle counts as the
/// rectangle around it.
fn flat_sides(s: &ShapeDef) -> [bool; 4] {
    // The axis a roof's ridge runs along and a wedge rises along, and which
    // end of it the wedge's high side is at.
    let run = (s.turn % 2) as usize;
    std::array::from_fn(|side| match s.kind {
        ShapeKind::Box => true,
        ShapeKind::Roof => side / 2 == run,
        ShapeKind::Wedge => side / 2 != run || (side % 2 == 1) == (s.turn < 2),
        ShapeKind::Cylinder | ShapeKind::Cone => false,
    })
}

/// Do two shapes each have a flat upright side in one plane, facing the same
/// way, where the two sides overlap?
fn shared_plane(a: &ShapeDef, b: &ShapeDef) -> bool {
    let overlap = |k: usize| a.at[k].max(b.at[k]) < (a.at[k] + a.size[k]).min(b.at[k] + b.size[k]);
    let (ours, theirs) = (flat_sides(a), flat_sides(b));
    (0..4).any(|side| {
        let axis = side / 2;
        // The low or the high end of the box along that axis.
        let end = |s: &ShapeDef| if side % 2 == 1 { s.at[axis] + s.size[axis] } else { s.at[axis] };
        ours[side] && theirs[side] && end(a) == end(b) && overlap(1 - axis) && overlap(2)
    })
}
