//! `slate_render` — 2D/2.5D isometric rendering, offscreen first.
//!
//! world → [`draw::build`] → [`draw::DrawList`] → [`raster::rasterize`] →
//! pixels + per-item stats → [`report::build_report`] / [`image::encode_png`].
//!
//! The rasterizer is a deterministic CPU implementation: no GPU, no window,
//! identical bytes on every platform (ADR 0007). Picking ([`pick`]) uses
//! the same draw list and coverage test.
//!
//! Players that only show the picture use [`paint::paint`] instead: the same
//! pixels from the same draw list, without owners, stats or a report, and
//! tens of times faster (ADR 0018).

pub mod draw;
pub mod font;
pub mod image;
pub mod paint;
pub mod pick;
pub mod raster;
pub mod report;
pub mod view;

pub use draw::{DrawItem, DrawList, Part, Rect, Shape};
pub use paint::paint;
pub use raster::{Frame, ItemStats, rasterize};
pub use report::{EntityReport, FrameReport, build_report};
pub use view::View;

#[cfg(test)]
mod tests;
