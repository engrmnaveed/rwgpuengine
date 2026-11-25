//! Isometric projection and the camera.
//!
//! Grid → screen (pixels, y down):
//!
//! ```text
//! sx = origin_x + (x - y) * tile_width  / 2
//! sy = origin_y + (x + y) * tile_height / 2 - z * z_height
//! ```
//!
//! So +x runs down-right, +y runs down-left and +z runs up. Integer cell
//! coordinates are cell centers; a cell's diamond spans ±0.5 around it.

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
    /// Screen position of grid point (0, 0, 0), whole pixels.
    pub origin: [f32; 2],
}

impl View {
    pub fn new(project: &ProjectDef, scene: &SceneDef) -> View {
        let d = project.display;
        let iso = project.iso;
        let center = scene.camera.center.unwrap_or_else(|| match &scene.ground {
            Some(g) => [(g.size[0] - 1) as f32 / 2.0, (g.size[1] - 1) as f32 / 2.0],
            None => [0.0, 0.0],
        });
        let half_w = iso.tile_width as f32 / 2.0;
        let half_h = iso.tile_height as f32 / 2.0;
        let ox = d.width as f32 / 2.0 - (center[0] - center[1]) * half_w + scene.camera.offset[0];
        let oy = d.height as f32 / 2.0 - (center[0] + center[1]) * half_h + scene.camera.offset[1];
        View {
            width: d.width,
            height: d.height,
            background: d.background,
            tile_w: iso.tile_width,
            tile_h: iso.tile_height,
            z_h: iso.z_height,
            origin: [round(ox) as f32, round(oy) as f32],
        }
    }

    /// Exact screen position (may be fractional).
    pub fn project(&self, x: f32, y: f32, z: f32) -> [f32; 2] {
        [
            self.origin[0] + (x - y) * (self.tile_w as f32 / 2.0),
            self.origin[1] + (x + y) * (self.tile_h as f32 / 2.0) - z * self.z_h as f32,
        ]
    }

    /// Screen position snapped to whole pixels (round half up).
    pub fn project_px(&self, x: f32, y: f32, z: f32) -> [i32; 2] {
        let [sx, sy] = self.project(x, y, z);
        [round(sx), round(sy)]
    }

    /// The four corners of a cell's diamond at height z: top, right, bottom, left.
    pub fn diamond(&self, x: f32, y: f32, z: f32) -> [[i32; 2]; 4] {
        [
            self.project_px(x - 0.5, y - 0.5, z),
            self.project_px(x + 0.5, y - 0.5, z),
            self.project_px(x + 0.5, y + 0.5, z),
            self.project_px(x - 0.5, y + 0.5, z),
        ]
    }
}

/// Round half up using exact float ops only (deterministic everywhere).
pub fn round(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}
