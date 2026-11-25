//! The frame and the isometric grid a scene is drawn with.
//!
//! These are fixed for a session. What moves is the [`Camera`](crate::Camera).
//!
//! With the camera where the scene puts it (`view = "iso"`, no turn, zoom 1),
//! grid → screen in pixels, y down, is:
//!
//! ```text
//! sx = origin_x + (x - y) * tile_width  / 2
//! sy = origin_y + (x + y) * tile_height / 2 - z * z_height
//! ```
//!
//! So +x runs down-right, +y runs down-left and +z runs up. Integer cell
//! coordinates are cell centers; a cell spans ±0.5 around it.

use rwgpuengine_core::color::Rgba;
use rwgpuengine_data::{ProjectDef, SceneDef};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub width: u32,
    pub height: u32,
    pub background: Rgba,
    pub tile_w: i32,
    pub tile_h: i32,
    pub z_h: i32,
    /// Screen pixel where the camera's center point is drawn. It is the
    /// middle of the frame plus the scene's `[camera] offset`, moved by less
    /// than a pixel so that whole cells land on whole pixels in the scene's
    /// starting view.
    pub pivot: [f32; 2],
}

impl View {
    pub fn new(project: &ProjectDef, scene: &SceneDef) -> View {
        let d = project.display;
        let iso = project.iso;
        let center = start_center(scene);
        let half_w = iso.tile_width as f32 / 2.0;
        let half_h = iso.tile_height as f32 / 2.0;
        // Where grid point (0, 0, 0) is drawn at the start, in whole pixels.
        let ox = round(d.width as f32 / 2.0 - (center[0] - center[1]) * half_w + scene.camera.offset[0]) as f32;
        let oy = round(d.height as f32 / 2.0 - (center[0] + center[1]) * half_h + scene.camera.offset[1]) as f32;
        View {
            width: d.width,
            height: d.height,
            background: d.background,
            tile_w: iso.tile_width,
            tile_h: iso.tile_height,
            z_h: iso.z_height,
            pivot: [ox + (center[0] - center[1]) * half_w, oy + (center[0] + center[1]) * half_h],
        }
    }
}

/// The ground point a scene's camera looks at when it starts: `[camera]
/// center`, or the middle of the ground.
pub fn start_center(scene: &SceneDef) -> [f32; 2] {
    scene.camera.center.unwrap_or_else(|| match &scene.ground {
        Some(g) => [(g.size[0] - 1) as f32 / 2.0, (g.size[1] - 1) as f32 / 2.0],
        None => [0.0, 0.0],
    })
}

/// Round half up using exact float ops only (deterministic everywhere).
pub fn round(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}
