//! The paint path: colors only, for players.
//!
//! [`paint`] fills a pixel buffer from a draw list with exactly the pixels
//! [`rasterize`](crate::raster::rasterize) gives, and nothing else: no
//! per-pixel owners, no statistics, no report. It solves each shape's
//! coverage rule for whole runs of pixels instead of testing every pixel of
//! the bounding box, which makes it tens of times faster (ADR 0018).
//!
//! `rasterize` stays the reference: `slate shot`, `slate test` and the frame
//! report use it, and tests hold the two pixel-identical.

use slate_core::color::Rgba;
use slate_data::font;

use crate::draw::{DrawList, Rect, Shape};
use crate::font::lit;

/// Quad vertices up to this size keep every product of the span solver
/// inside an i64. Larger shapes use the per-pixel test.
const QUAD_LIMIT: i64 = 1 << 27;
/// Ellipse sizes up to this keep `w² · h²` inside an i64.
const ELLIPSE_LIMIT: i64 = 1 << 15;

/// Paint the list's enabled items, back to front, into `rgba` (RGBA8,
/// row-major, top row first). The buffer is resized to the view and fully
/// overwritten, so a player can reuse one buffer for every frame.
pub fn paint(list: &DrawList, rgba: &mut Vec<u8>) {
    let (w, h) = (list.view.width as usize, list.view.height as usize);
    let bg = list.view.background;
    rgba.resize(w * h * 4, 0);
    solid(rgba, [bg.r, bg.g, bg.b, 255]);
    let screen = Rect { x0: 0, y0: 0, x1: w as i32, y1: h as i32 };
    for item in list.items.iter().filter(|i| i.enabled) {
        let area = item.shape.bbox().intersect(&screen);
        let color = item.color;
        spans(&item.shape, area, |y, x0, x1| fill(rgba, w, y, x0, x1, color));
    }
}

/// Source-over `color` onto pixels `x0..x1` of row `y`.
fn fill(rgba: &mut [u8], width: usize, y: i32, x0: i32, x1: i32, color: Rgba) {
    let row = y as usize * width;
    let run = &mut rgba[(row + x0 as usize) * 4..(row + x1 as usize) * 4];
    if color.a == 255 {
        solid(run, [color.r, color.g, color.b, 255]);
    } else {
        for px in run.as_chunks_mut::<4>().0 {
            let c = color.over(Rgba { r: px[0], g: px[1], b: px[2], a: 255 });
            *px = [c.r, c.g, c.b, 255];
        }
    }
}

/// Set every pixel of `run` (a whole number of RGBA pixels) to `px`.
///
/// Short runs are written pixel by pixel. Long ones write a few pixels and
/// then copy what is already written onto the rest, doubling each time: a
/// block copy is far faster than a pixel loop where the target has no
/// vector instructions (wasm), and the bytes are the same either way.
fn solid(run: &mut [u8], px: [u8; 4]) {
    const SEED: usize = 16 * 4;
    let head = run.len().min(SEED);
    for p in run[..head].as_chunks_mut::<4>().0 {
        *p = px;
    }
    let mut done = head;
    while done < run.len() {
        let n = done.min(run.len() - done);
        run.copy_within(..n, done);
        done += n;
    }
}

/// Call `f(y, x0, x1)` for every run of pixels of `shape` inside `area`
/// (`x1` exclusive, `x0 < x1`). `area` must lie inside the shape's bounding
/// box. The runs cover exactly the pixels [`Shape::covers`] accepts, each
/// once.
pub(crate) fn spans(shape: &Shape, area: Rect, mut f: impl FnMut(i32, i32, i32)) {
    if area.is_empty() {
        return;
    }
    match *shape {
        Shape::Rect(_) => {
            for py in area.y0..area.y1 {
                f(py, area.x0, area.x1);
            }
        }
        Shape::Quad(p) if p.iter().flatten().all(|v| (*v as i64).abs() <= QUAD_LIMIT) => {
            // `covers` keeps a pixel when, for every edge a -> b,
            //   cross = dx·(cy - ay) - dy·(cx - ax)
            // is positive, or zero on a top or left edge. With cx = 2·px + 1
            // that is linear in px:  cross = dx·cy + k - 2·dy·px,  where
            // k = dy·(ax - 1) - dx·ay.  So each edge bounds px from one side.
            let mut edges = [(0i64, 0i64, 0i64); 4];
            for (i, edge) in edges.iter_mut().enumerate() {
                let (ax, ay) = (2 * p[i][0] as i64, 2 * p[i][1] as i64);
                let (bx, by) = (2 * p[(i + 1) % 4][0] as i64, 2 * p[(i + 1) % 4][1] as i64);
                let (dx, dy) = (bx - ax, by - ay);
                *edge = (dx, dy, dy * (ax - 1) - dx * ay);
            }
            for py in area.y0..area.y1 {
                let cy = 2 * py as i64 + 1;
                // Inclusive range of covered pixels on this row.
                let (mut lo, mut hi) = (area.x0 as i64, area.x1 as i64 - 1);
                for &(dx, dy, k) in &edges {
                    let b = dx * cy + k;
                    if dy > 0 {
                        // Never a top or left edge: cross > 0, so 2·dy·px < b.
                        hi = hi.min((b - 1).div_euclid(2 * dy));
                    } else if dy < 0 {
                        // Always a left edge: cross >= 0, so 2·|dy|·px >= -b.
                        lo = lo.max(ceil_div(-b, -2 * dy));
                    } else if b < 0 || (b == 0 && dx <= 0) {
                        // Horizontal: the whole row is out (a top edge has dx > 0).
                        hi = lo - 1;
                        break;
                    }
                }
                if lo <= hi {
                    f(py, lo as i32, hi as i32 + 1);
                }
            }
        }
        Shape::Ellipse(r)
            if (r.x1 as i64 - r.x0 as i64) <= ELLIPSE_LIMIT && (r.y1 as i64 - r.y0 as i64) <= ELLIPSE_LIMIT =>
        {
            // `covers`: dx²·h² + dy²·w² <= w²·h², with dx = cx - (2·x0 + w).
            let (w, h) = ((r.x1 - r.x0) as i64, (r.y1 - r.y0) as i64);
            let center = 2 * r.x0 as i64 + w;
            for py in area.y0..area.y1 {
                let dy = 2 * py as i64 + 1 - (2 * r.y0 as i64 + h);
                let rest = w * w * h * h - dy * dy * w * w;
                if rest < 0 {
                    continue;
                }
                // The largest |dx| with dx²·h² <= rest, then the pixels with
                // |2·px + 1 - center| <= that.
                let reach = (rest / (h * h)).isqrt();
                let lo = ceil_div(center - reach - 1, 2).max(area.x0 as i64);
                let hi = (center + reach - 1).div_euclid(2).min(area.x1 as i64 - 1);
                if lo <= hi {
                    f(py, lo as i32, hi as i32 + 1);
                }
            }
        }
        Shape::Glyph { x0, y0, scale, index } if scale > 0 => {
            // Each lit font pixel is a `scale` × `scale` block; neighbors on
            // a row join into one run.
            for py in area.y0..area.y1 {
                let row = (py - y0) / scale;
                let mut col = 0;
                while col < font::GLYPH_W {
                    if !lit(index, col, row) {
                        col += 1;
                        continue;
                    }
                    let first = col;
                    while col < font::GLYPH_W && lit(index, col, row) {
                        col += 1;
                    }
                    let (a, b) = ((x0 + first * scale).max(area.x0), (x0 + col * scale).min(area.x1));
                    if a < b {
                        f(py, a, b);
                    }
                }
            }
        }
        // Out of the solver's range: test each pixel, as the reference does.
        _ => {
            for py in area.y0..area.y1 {
                let mut px = area.x0;
                while px < area.x1 {
                    if !shape.covers(px, py) {
                        px += 1;
                        continue;
                    }
                    let first = px;
                    while px < area.x1 && shape.covers(px, py) {
                        px += 1;
                    }
                    f(py, first, px);
                }
            }
        }
    }
}

/// `a / b` rounded up, for `b > 0`.
fn ceil_div(a: i64, b: i64) -> i64 {
    -(-a).div_euclid(b)
}

#[cfg(test)]
mod tests {
    use slate_core::rng::Rng;

    use super::*;

    const SCREEN: Rect = Rect { x0: 0, y0: 0, x1: 64, y1: 48 };

    /// The pixels `spans` reports, checking each run is inside the area,
    /// non-empty, and that no pixel is reported twice.
    fn covered_by_spans(shape: &Shape) -> Vec<bool> {
        let area = shape.bbox().intersect(&SCREEN);
        let mut hit = vec![false; (SCREEN.x1 * SCREEN.y1) as usize];
        spans(shape, area, |y, x0, x1| {
            assert!(
                x0 < x1 && x0 >= area.x0 && x1 <= area.x1 && y >= area.y0 && y < area.y1,
                "{shape:?}: {y} {x0}..{x1}"
            );
            for x in x0..x1 {
                let i = (y * SCREEN.x1 + x) as usize;
                assert!(!hit[i], "{shape:?}: pixel {x},{y} reported twice");
                hit[i] = true;
            }
        });
        hit
    }

    fn assert_same_as_covers(shape: &Shape) {
        let hit = covered_by_spans(shape);
        let area = shape.bbox().intersect(&SCREEN);
        for py in 0..SCREEN.y1 {
            for px in 0..SCREEN.x1 {
                let inside = px >= area.x0 && px < area.x1 && py >= area.y0 && py < area.y1;
                let want = inside && shape.covers(px, py);
                assert_eq!(hit[(py * SCREEN.x1 + px) as usize], want, "{shape:?} at {px},{py}");
            }
        }
    }

    fn rect(rng: &mut Rng) -> Rect {
        let (x0, y0) = (rng.range_i32(-20, 70), rng.range_i32(-20, 55));
        Rect { x0, y0, x1: x0 + rng.range_i32(0, 40), y1: y0 + rng.range_i32(0, 40) }
    }

    fn point(rng: &mut Rng) -> [i32; 2] {
        [rng.range_i32(-20, 84), rng.range_i32(-20, 68)]
    }

    #[test]
    fn spans_cover_exactly_the_pixels_covers_accepts() {
        let mut rng = Rng::new(2026);
        for _ in 0..1500 {
            // Any four points: convex, concave, crossed, flat or repeated.
            let quad = Shape::quad([point(&mut rng), point(&mut rng), point(&mut rng), point(&mut rng)]);
            assert_same_as_covers(&quad);
            // An axis-aligned box and an iso diamond, as the draw list makes them.
            let r = rect(&mut rng);
            assert_same_as_covers(&Shape::quad([[r.x0, r.y0], [r.x1, r.y0], [r.x1, r.y1], [r.x0, r.y1]]));
            let (mx, my) = ((r.x0 + r.x1) / 2, (r.y0 + r.y1) / 2);
            assert_same_as_covers(&Shape::quad([[mx, r.y0], [r.x1, my], [mx, r.y1], [r.x0, my]]));
            assert_same_as_covers(&Shape::Ellipse(r));
            assert_same_as_covers(&Shape::Rect(r));
            let glyph = Shape::Glyph {
                x0: rng.range_i32(-12, 66),
                y0: rng.range_i32(-12, 50),
                scale: rng.range_i32(1, 5),
                index: rng.below(100) as u8,
            };
            assert_same_as_covers(&glyph);
        }
    }

    #[test]
    fn shapes_beyond_the_solver_range_use_the_pixel_test() {
        let far = 1 << 28;
        let all = Shape::quad([[-far, -far], [far, -far], [far, far], [-far, far]]);
        assert!(covered_by_spans(&all).iter().all(|&hit| hit));
        assert_same_as_covers(&all);
        let half = Shape::quad([[-far, -far], [32, -far], [32, far], [-far, far]]);
        assert_same_as_covers(&half);
        assert_eq!(covered_by_spans(&half).iter().filter(|&&hit| hit).count(), 32 * 48);
        // 40,000 wide: past the solver's limit, still exact in the reference's i64 math.
        let big = Shape::Ellipse(Rect { x0: -20_000, y0: -20_000, x1: 20_000, y1: 20_000 });
        assert_same_as_covers(&big);
        assert!(covered_by_spans(&big).iter().all(|&hit| hit));
    }

    #[test]
    fn ceil_div_rounds_up() {
        assert_eq!([ceil_div(7, 2), ceil_div(6, 2), ceil_div(-7, 2), ceil_div(0, 3)], [4, 3, -3, 0]);
    }
}
