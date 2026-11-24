//! The deterministic CPU rasterizer.
//!
//! Integer coverage tests, integer shading and integer blending: the same
//! draw list gives the same bytes on every platform, so screenshots can be
//! compared with zero tolerance against reference images (ADR 0007).
//! Besides colors it records which draw item owns each pixel; the frame
//! report uses that to measure visibility and occlusion.

use std::collections::BTreeMap;

use slate_core::color::Rgba;
use slate_core::hash::fnv1a64;

use crate::draw::{DrawList, Rect};

/// A rendered frame.
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// RGBA8, row-major, top row first.
    pub rgba: Vec<u8>,
    /// Owner of each pixel: draw item index + 1, or 0 for background.
    pub owner: Vec<u32>,
}

impl Frame {
    /// FNV-1a of the pixels. Equal hashes mean identical images.
    pub fn image_hash(&self) -> u64 {
        fnv1a64(&self.rgba)
    }

    pub fn pixel(&self, x: u32, y: u32) -> Rgba {
        let i = ((y * self.width + x) * 4) as usize;
        Rgba { r: self.rgba[i], g: self.rgba[i + 1], b: self.rgba[i + 2], a: self.rgba[i + 3] }
    }
}

/// Per-item pixel accounting.
#[derive(Clone, Debug, Default)]
pub struct ItemStats {
    /// Pixels the item covered inside the frame.
    pub drawn: u32,
    /// Pixels still showing the item at the end.
    pub visible: u32,
    /// Pixels of this item later covered by other items (by item index).
    pub covered_by: BTreeMap<usize, u32>,
}

pub fn rasterize(list: &DrawList) -> (Frame, Vec<ItemStats>) {
    let (w, h) = (list.view.width, list.view.height);
    let bg = list.view.background;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..w * h {
        rgba.extend_from_slice(&[bg.r, bg.g, bg.b, 255]);
    }
    let mut owner = vec![0u32; (w * h) as usize];
    let mut stats: Vec<ItemStats> = vec![ItemStats::default(); list.items.len()];
    let screen = Rect { x0: 0, y0: 0, x1: w as i32, y1: h as i32 };

    for (idx, item) in list.items.iter().enumerate() {
        if !item.enabled {
            continue;
        }
        let area = item.shape.bbox().intersect(&screen);
        for py in area.y0..area.y1 {
            for px in area.x0..area.x1 {
                if !item.shape.covers(px, py) {
                    continue;
                }
                let i = (py as u32 * w + px as u32) as usize;
                let dst = Rgba { r: rgba[i * 4], g: rgba[i * 4 + 1], b: rgba[i * 4 + 2], a: 255 };
                let c = item.color.over(dst);
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[c.r, c.g, c.b, 255]);
                let prev = owner[i];
                if prev != 0 {
                    *stats[prev as usize - 1].covered_by.entry(idx).or_insert(0) += 1;
                }
                owner[i] = idx as u32 + 1;
                stats[idx].drawn += 1;
            }
        }
    }
    for &o in &owner {
        if o != 0 {
            stats[o as usize - 1].visible += 1;
        }
    }
    (Frame { width: w, height: h, rgba, owner }, stats)
}
