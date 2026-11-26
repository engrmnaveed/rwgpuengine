//! `rwgpuengine_render` — the render list and the CPU reference renderer.
//!
//! world → [`list::build`] → [`RenderList`] → [`reference::project`] →
//! [`Projected`] → [`reference::render`] → pixels and owners →
//! [`report::build_report`] / [`image::encode_png`].
//!
//! The render list is what every renderer reads: a camera, a light, solid
//! instances, cards and a 2D layer. The reference renderer here draws it on
//! the CPU, with no GPU and no window, to identical bytes on every platform;
//! picking ([`pick`]) and the frame report read the same projection, so what
//! is shown, what is reported and what a tap hits cannot disagree.
//!
//! Players that only show the picture use a [`Painter`]: the same pixels
//! without owners, statistics or a report.

pub mod camera;
pub mod draw;
pub mod font;
pub mod image;
pub mod list;
pub mod paint;
pub mod pick;
pub mod reference;
pub mod report;
pub mod view;

pub use camera::{Camera, CardPlace, LevelPlanes, Projector};
pub use draw::{DrawItem, Part, Rect, Shape};
pub use list::{Card, CardBody, Instance, Light, Mesh, RenderList};
pub use reference::{Coverage, Frame, Item, ItemStats, Painter, Projected, project, render};
pub use report::{EntityReport, FrameReport, build_report};
pub use view::View;

#[cfg(test)]
mod tests;
