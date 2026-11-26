//! Where the frame goes in a window, and where a click lands in the frame.
//!
//! The frame keeps its own size (`[view]` in rwgpu.toml). In a window it is
//! scaled to the largest rectangle with its aspect that fits, centered, with
//! black bars around it. A click is taken back from window pixels to frame
//! pixels, the pixels `rwgpu inspect` and playtest taps are written in.

/// The frame's rectangle in a window: `[x, y, width, height]` in window
/// pixels. `frame` and `window` are `[width, height]`.
pub fn letterbox(frame: [u32; 2], window: [u32; 2]) -> [u32; 4] {
    let [fw, fh] = frame.map(|v| u64::from(v.max(1)));
    let [ww, wh] = window.map(|v| u64::from(v.max(1)));
    // Width-bound when the window is relatively wider than the frame is tall.
    let (w, h) = if ww * fh <= wh * fw { (ww, (ww * fh + fw / 2) / fw) } else { ((wh * fw + fh / 2) / fh, wh) };
    let (w, h) = (w.clamp(1, ww), h.clamp(1, wh));
    [((ww - w) / 2) as u32, ((wh - h) / 2) as u32, w as u32, h as u32]
}

/// The frame pixel under a window pixel, or `None` when it is on a bar.
pub fn to_frame(area: [u32; 4], frame: [u32; 2], at: [f64; 2]) -> Option<[f32; 2]> {
    let [x, y, w, h] = area.map(f64::from);
    let fx = (at[0] - x) * f64::from(frame[0]) / w;
    let fy = (at[1] - y) * f64::from(frame[1]) / h;
    let inside = (0.0..f64::from(frame[0])).contains(&fx) && (0.0..f64::from(frame[1])).contains(&fy);
    inside.then_some([fx as f32, fy as f32])
}

/// Window pixels per frame pixel.
pub fn scale(area: [u32; 4], frame: [u32; 2]) -> f64 {
    f64::from(area[2]) / f64::from(frame[0].max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_keeps_its_aspect_with_bars_around_it() {
        // The window the player opens: the frame's size in logical pixels, twice that on a Retina display.
        assert_eq!(letterbox([640, 480], [640, 480]), [0, 0, 640, 480]);
        assert_eq!(letterbox([640, 480], [1280, 960]), [0, 0, 1280, 960]);
        // Wider: bars left and right. Taller: bars above and below.
        assert_eq!(letterbox([640, 480], [1000, 480]), [180, 0, 640, 480]);
        assert_eq!(letterbox([640, 480], [640, 900]), [0, 210, 640, 480]);
        // Any size: the aspect holds to the nearest pixel.
        assert_eq!(letterbox([640, 480], [1513, 911]), [149, 0, 1215, 911]);
        assert_eq!(letterbox([640, 480], [1, 1]), [0, 0, 1, 1]);
        assert_eq!(letterbox([640, 480], [0, 0]), [0, 0, 1, 1]);
    }

    #[test]
    fn a_click_lands_on_the_frame_pixel_drawn_under_it() {
        let frame = [640, 480];
        // At twice the size, window pixel (833, 545) shows frame pixel (416, 272).
        let area = letterbox(frame, [1280, 960]);
        assert_eq!(to_frame(area, frame, [833.0, 545.0]).map(|p| p.map(f32::floor)), Some([416.0, 272.0]));
        // With bars: the bars are not the frame.
        let area = letterbox(frame, [1000, 480]);
        assert_eq!(to_frame(area, frame, [179.0, 100.0]), None);
        assert_eq!(to_frame(area, frame, [180.0, 100.0]), Some([0.0, 100.0]));
        assert_eq!(to_frame(area, frame, [819.5, 479.5]), Some([639.5, 479.5]));
        assert_eq!(to_frame(area, frame, [820.0, 100.0]), None);
        assert_eq!(scale(area, frame), 1.0);
        // Every frame pixel's center maps back to that pixel at an odd size.
        let window = [1513, 911];
        let area = letterbox(frame, window);
        let s = scale(area, frame);
        for (px, py) in [(0u32, 0u32), (639, 479), (320, 240), (17, 401)] {
            let at = [f64::from(area[0]) + (f64::from(px) + 0.5) * s, f64::from(area[1]) + (f64::from(py) + 0.5) * s];
            let got = to_frame(area, frame, at).expect("inside");
            assert_eq!([got[0].floor() as u32, got[1].floor() as u32], [px, py]);
        }
    }
}
