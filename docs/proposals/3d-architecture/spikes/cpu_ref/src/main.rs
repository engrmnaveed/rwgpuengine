//! Study spike (not engine code): a deterministic CPU reference renderer for
//! the same town the GPU spike draws. Flat-shaded triangles, a depth buffer
//! and an owner (instance id) per pixel, which is what a frame report needs.
//! Only exact float operations and integers, so every platform gives the
//! same bytes.
#[path = "../../shared/scene.rs"]
mod scene;

use std::time::Instant;

const SUB: i64 = 256; // sub-pixel steps per pixel

struct Frame {
    rgba: Vec<u8>,
    depth: Vec<f32>,
    owner: Vec<u32>,
}

fn render(town: &[scene::Inst], vp: &[[f32; 4]; 4], f: &mut Frame) -> (u32, u32) {
    let (w, h) = (scene::W as i64, scene::H as i64);
    for px in f.rgba.chunks_exact_mut(4) {
        px.copy_from_slice(&scene::CLEAR);
    }
    f.depth.fill(1.0);
    f.owner.fill(0);
    let (mut tris, mut drawn) = (0u32, 0u32);
    for (id, inst) in town.iter().enumerate() {
        for (normal, quad) in scene::FACES {
            // Project the four corners.
            let mut p = [[0i64; 2]; 4];
            let mut z = [0f32; 4];
            let mut behind = false;
            for (k, c) in quad.iter().enumerate() {
                let world = [c[0] * inst.size[0] + inst.pos[0], c[1] * inst.size[1] + inst.pos[1], c[2] * inst.size[2] + inst.pos[2]];
                let clip = |r: usize| vp[0][r] * world[0] + vp[1][r] * world[1] + vp[2][r] * world[2] + vp[3][r];
                let cw = clip(3);
                if cw <= 0.0 {
                    behind = true;
                    break;
                }
                let sx = (clip(0) / cw * 0.5 + 0.5) * w as f32;
                let sy = (1.0 - (clip(1) / cw * 0.5 + 0.5)) * h as f32;
                p[k] = [(sx * SUB as f32 + 0.5).floor() as i64, (sy * SUB as f32 + 0.5).floor() as i64];
                z[k] = clip(2) / cw;
            }
            if behind {
                continue;
            }
            let c = scene::shade(inst.color, normal);
            let color = [(c[0] * 255.0 + 0.5).floor() as u8, (c[1] * 255.0 + 0.5).floor() as u8, (c[2] * 255.0 + 0.5).floor() as u8, 255];
            for t in [[0usize, 1, 2], [0, 2, 3]] {
                let (a, b, cc) = (p[t[0]], p[t[1]], p[t[2]]);
                // Screen y points down, so a front face (counter-clockwise in
                // the world) has negative area here.
                let area = (b[0] - a[0]) * (cc[1] - a[1]) - (b[1] - a[1]) * (cc[0] - a[0]);
                if area >= 0 {
                    continue;
                }
                tris += 1;
                let (minx, maxx) = (a[0].min(b[0]).min(cc[0]), a[0].max(b[0]).max(cc[0]));
                let (miny, maxy) = (a[1].min(b[1]).min(cc[1]), a[1].max(b[1]).max(cc[1]));
                let (x0, x1) = (((minx - SUB / 2 + SUB - 1).div_euclid(SUB)).max(0), ((maxx - SUB / 2).div_euclid(SUB)).min(w - 1));
                let (y0, y1) = (((miny - SUB / 2 + SUB - 1).div_euclid(SUB)).max(0), ((maxy - SUB / 2).div_euclid(SUB)).min(h - 1));
                if x0 > x1 || y0 > y1 {
                    continue;
                }
                // Edge functions, oriented so inside is >= 0; a top or left
                // edge keeps its boundary pixels, the others do not.
                let edge = |p0: [i64; 2], p1: [i64; 2]| {
                    let (dx, dy) = (p1[0] - p0[0], p1[1] - p0[1]);
                    // value = dy*(x - p0x) - dx*(y - p0y): >= 0 inside a triangle that is
                    // counter-clockwise on screen. Going round it that way, a left edge
                    // runs down (dy > 0) and a top edge runs left (dx < 0).
                    let top_left = (dy == 0 && dx < 0) || dy > 0;
                    (dy, -dx, dx * p0[1] - dy * p0[0], if top_left { 0 } else { -1 })
                };
                let e = [edge(b, cc), edge(cc, a), edge(a, b)];
                let zs = [z[t[0]], z[t[1]], z[t[2]]];
                let inv_area = 1.0 / (-area) as f32;
                for py in y0..=y1 {
                    let cy = py * SUB + SUB / 2;
                    for px in x0..=x1 {
                        let cx = px * SUB + SUB / 2;
                        let w0 = e[0].0 * cx + e[0].1 * cy + e[0].2;
                        let w1 = e[1].0 * cx + e[1].1 * cy + e[1].2;
                        let w2 = e[2].0 * cx + e[2].1 * cy + e[2].2;
                        if w0 + e[0].3 < 0 || w1 + e[1].3 < 0 || w2 + e[2].3 < 0 {
                            continue;
                        }
                        let depth = (w0 as f32 * zs[0] + w1 as f32 * zs[1] + w2 as f32 * zs[2]) * inv_area;
                        let i = (py * w + px) as usize;
                        if depth < f.depth[i] {
                            f.depth[i] = depth;
                            f.owner[i] = id as u32 + 1;
                            f.rgba[i * 4..i * 4 + 4].copy_from_slice(&color);
                            drawn += 1;
                        }
                    }
                }
            }
        }
    }
    (tris, drawn)
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let n_px = (scene::W * scene::H) as usize;
    let mut f = Frame { rgba: vec![0; n_px * 4], depth: vec![1.0; n_px], owner: vec![0; n_px] };
    for n in [5_000usize, 50_000, 500_000] {
        let town = scene::town(n);
        let vp = scene::view_proj(n);
        let reps = if n > 100_000 { 3 } else { 9 };
        let mut times = Vec::new();
        let mut stats = (0, 0);
        for _ in 0..reps {
            let t = Instant::now();
            stats = render(&town, &vp, &mut f);
            times.push(t.elapsed().as_secs_f64() * 1e3);
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let ms = times[times.len() / 2];
        let visible: std::collections::BTreeSet<u32> = f.owner.iter().copied().filter(|o| *o != 0).collect();
        std::fs::write(format!("{dir}/cpu_{n}.rgba"), &f.rgba).ok();
        let mut line = format!(
            "n={n:>7}: {ms:.1} ms; {} front triangles; {} of {n} boxes own a pixel; image {:016x}",
            stats.0,
            visible.len(),
            fnv(&f.rgba)
        );
        if let Ok(gpu) = std::fs::read(format!("{dir}/gpu_{n}.rgba")) {
            let (mut diff, mut max) = (0usize, 0u8);
            for (a, b) in f.rgba.chunks_exact(4).zip(gpu.chunks_exact(4)) {
                if a != b {
                    diff += 1;
                    for k in 0..3 {
                        max = max.max(a[k].abs_diff(b[k]));
                    }
                }
            }
            line += &format!("; against the GPU frame: {diff} of {n_px} pixels differ ({:.3} %)", diff as f64 * 100.0 / n_px as f64);
            let _ = max;
        }
        println!("{line}");
    }
}
