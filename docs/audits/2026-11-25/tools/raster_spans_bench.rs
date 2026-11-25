//! Audit scratch bench (not part of the repo): where does a frame's time go,
//! and how fast is a present-only span rasterizer with identical pixels?
use std::time::Instant;

use slate_core::color::Rgba;
use slate_core::hash::fnv1a64;
use slate_render::font::lit;
use slate_render::{DrawList, Shape, build_report, rasterize};
use slate_runtime::{DataOnly, Session, SessionConfig};

fn floor_div(a: i64, b: i64) -> i64 {
    a.div_euclid(b)
}
fn ceil_div(a: i64, b: i64) -> i64 {
    -((-a).div_euclid(b))
}

#[inline]
fn fill(rgba: &mut [u8], w: usize, py: i32, x0: i32, x1: i32, c: Rgba) {
    if x1 <= x0 {
        return;
    }
    let row = py as usize * w;
    let s = &mut rgba[(row + x0 as usize) * 4..(row + x1 as usize) * 4];
    if c.a == 255 {
        for px in s.chunks_exact_mut(4) {
            px.copy_from_slice(&[c.r, c.g, c.b, 255]);
        }
    } else {
        for px in s.chunks_exact_mut(4) {
            let o = c.over(Rgba { r: px[0], g: px[1], b: px[2], a: 255 });
            px.copy_from_slice(&[o.r, o.g, o.b, 255]);
        }
    }
}

/// Present-only rasterizer: colors only, one span per scanline per shape,
/// the same exact pixel-center coverage rule as the reference rasterizer.
fn rasterize_spans(list: &DrawList) -> Vec<u8> {
    let (w, h) = (list.view.width as i32, list.view.height as i32);
    let bg = list.view.background;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for px in rgba.chunks_exact_mut(4) {
        px.copy_from_slice(&[bg.r, bg.g, bg.b, 255]);
    }
    let wu = w as usize;
    for item in &list.items {
        if !item.enabled {
            continue;
        }
        let c = item.color;
        match item.shape {
            Shape::Rect(r) => {
                let (x0, x1) = (r.x0.max(0), r.x1.min(w));
                for py in r.y0.max(0)..r.y1.min(h) {
                    fill(&mut rgba, wu, py, x0, x1, c);
                }
            }
            Shape::Quad(p) => {
                let (mut ymin, mut ymax) = (i32::MAX, i32::MIN);
                let (mut xmin, mut xmax) = (i32::MAX, i32::MIN);
                for v in &p {
                    ymin = ymin.min(v[1]);
                    ymax = ymax.max(v[1]);
                    xmin = xmin.min(v[0]);
                    xmax = xmax.max(v[0]);
                }
                let (bx0, bx1) = (xmin.max(0), xmax.min(w));
                for py in ymin.max(0)..ymax.min(h) {
                    let cy = 2 * py as i64 + 1;
                    // inclusive pixel range [lo, hi]
                    let (mut lo, mut hi) = (bx0 as i64, bx1 as i64 - 1);
                    let mut empty = false;
                    for i in 0..4 {
                        let (ax, ay) = (2 * p[i][0] as i64, 2 * p[i][1] as i64);
                        let (bx, by) = (2 * p[(i + 1) % 4][0] as i64, 2 * p[(i + 1) % 4][1] as i64);
                        let (dx, dy) = (bx - ax, by - ay);
                        let top_left = (dy == 0 && dx > 0) || dy < 0;
                        // cross(px) = B - 2*dy*px
                        let b = dx * (cy - ay) + dy * (ax - 1);
                        if dy == 0 {
                            if b < 0 || (b == 0 && !top_left) {
                                empty = true;
                                break;
                            }
                        } else if dy > 0 {
                            // 2*dy*px < B  (or <= when top_left; never for dy > 0)
                            let m = if top_left { floor_div(b, 2 * dy) } else { floor_div(b - 1, 2 * dy) };
                            hi = hi.min(m);
                        } else {
                            let e = -dy;
                            // B + 2e*px > 0 (>= when top_left, always for dy < 0)
                            let m = if top_left { ceil_div(-b, 2 * e) } else { floor_div(-b, 2 * e) + 1 };
                            lo = lo.max(m);
                        }
                    }
                    if !empty && lo <= hi {
                        fill(&mut rgba, wu, py, lo as i32, hi as i32 + 1, c);
                    }
                }
            }
            Shape::Ellipse(r) => {
                let (ew, eh) = ((r.x1 - r.x0) as i64, (r.y1 - r.y0) as i64);
                if ew <= 0 || eh <= 0 {
                    continue;
                }
                let c0 = 2 * r.x0 as i64 + ew;
                for py in r.y0.max(0)..r.y1.min(h) {
                    let dy = 2 * py as i64 + 1 - (2 * r.y0 as i64 + eh);
                    let rr = ew * ew * eh * eh - dy * dy * ew * ew;
                    if rr < 0 {
                        continue;
                    }
                    let m = ((rr / (eh * eh)) as u64).isqrt() as i64;
                    let lo = ceil_div(c0 - m - 1, 2).max(r.x0.max(0) as i64);
                    let hi = floor_div(c0 + m - 1, 2).min(r.x1.min(w) as i64 - 1);
                    if lo <= hi {
                        fill(&mut rgba, wu, py, lo as i32, hi as i32 + 1, c);
                    }
                }
            }
            Shape::Glyph { x0, y0, scale, index } => {
                for row in 0..8 {
                    for col in 0..5 {
                        if !lit(index, col, row) {
                            continue;
                        }
                        let (gx0, gx1) = ((x0 + col * scale).max(0), (x0 + (col + 1) * scale).min(w));
                        for py in (y0 + row * scale).max(0)..(y0 + (row + 1) * scale).min(h) {
                            fill(&mut rgba, wu, py, gx0, gx1, c);
                        }
                    }
                }
            }
        }
    }
    rgba
}

fn median_us(mut f: impl FnMut(), n: u32) -> f64 {
    for _ in 0..n / 4 {
        f();
    }
    let mut b: Vec<f64> = (0..5)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..n {
                f();
            }
            t.elapsed().as_secs_f64() * 1e6 / n as f64
        })
        .collect();
    b.sort_by(|a, b| a.partial_cmp(b).unwrap());
    b[2]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = std::path::Path::new(&args[0]);
    let vfs = slate_data::Vfs::from_dir(dir).unwrap();
    let tests: Vec<String> = vfs.paths_under("tests", ".toml").map(str::to_string).collect();
    for t in tests {
        let cfg = SessionConfig { script: Some(t.clone()), ..Default::default() };
        let mut s = Session::load(&vfs, &cfg, Box::new(DataOnly)).unwrap();
        let frames = s.script_frames().unwrap_or(0);
        // image identity on every frame of the playtest
        let mut same = 0u64;
        let mut changed_frames = 0u64;
        let mut last = 0u64;
        for f in 0..=frames {
            s.run_to(f);
            let list = s.draw_list(false);
            let reference = rasterize(&list).0.image_hash();
            let fast = fnv1a64(&rasterize_spans(&list));
            if reference == fast {
                same += 1;
            }
            if f > 0 && reference != last {
                changed_frames += 1;
            }
            last = reference;
        }
        let list = s.draw_list(false);
        let n = if list.view.width > 1000 { 40 } else { 200 };
        let t_build = median_us(|| { std::hint::black_box(s.draw_list(false)); }, n);
        let t_rast = median_us(|| { std::hint::black_box(rasterize(&list)); }, n);
        let (frame, stats) = rasterize(&list);
        let t_report = median_us(|| { std::hint::black_box(build_report(&s.world, &list, &frame, &stats)); }, n);
        let t_fast = median_us(|| { std::hint::black_box(rasterize_spans(&list)); }, n);
        let t_tick = {
            let mut s2 = Session::load(&vfs, &cfg, Box::new(DataOnly)).unwrap();
            let t = Instant::now();
            s2.run_to(frames);
            t.elapsed().as_secs_f64() * 1e6 / frames.max(1) as f64
        };
        println!(
            "{:<22} {}x{} items={:<4} identical={}/{} changed_frames={}/{} | us: tick={:.1} build={:.0} raster={:.0} report={:.0} spans={:.0} (x{:.1})",
            t.trim_start_matches("tests/").trim_end_matches(".toml"),
            list.view.width, list.view.height, list.items.len(),
            same, frames + 1, changed_frames, frames,
            t_tick, t_build, t_rast, t_report, t_fast, t_rast / t_fast
        );
    }
}
