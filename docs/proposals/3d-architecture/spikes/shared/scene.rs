//! Shared by the GPU spike and the CPU reference spike: one generated town,
//! one camera, one light. Only exact float operations (+ - * / sqrt), so both
//! sides compute the same numbers on every platform.
#![allow(dead_code)]

#[derive(Clone, Copy)]
pub struct Inst {
    /// Minimum corner of the box (y is up).
    pub pos: [f32; 3],
    pub size: [f32; 3],
    pub color: [u8; 4],
}

pub const W: u32 = 1280;
pub const H: u32 = 720;
/// Unit direction to the light (precomputed, normalized).
pub const LIGHT: [f32; 3] = [0.50709254, 0.8113481, 0.29073307];
pub const AMBIENT: f32 = 0.35;
pub const CLEAR: [u8; 4] = [42, 93, 143, 255];

/// A square town of `n` boxes: one per cell, deterministic heights and colors.
pub fn town(n: usize) -> Vec<Inst> {
    let g = grid(n);
    let mut seed: u32 = 0x1234_5678;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed >> 8
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (x, z) = ((i % g) as f32, (i / g) as f32);
        let r = next();
        let height = 0.4 + (r % 256) as f32 / 255.0 * 2.6;
        let palette = [[181, 101, 29], [139, 47, 47], [63, 138, 63], [200, 190, 160], [90, 110, 160], [230, 200, 80]];
        let c = palette[(next() % 6) as usize];
        out.push(Inst { pos: [x + 0.1, 0.0, z + 0.1], size: [0.8, height, 0.8], color: [c[0], c[1], c[2], 255] });
    }
    out
}

pub fn grid(n: usize) -> usize {
    let mut g = 1;
    while g * g < n {
        g += 1;
    }
    g
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

/// Column-major view-projection (right-handed, depth 0..1) framing the whole town.
pub fn view_proj(n: usize) -> [[f32; 4]; 4] {
    let g = grid(n) as f32;
    let center = [g / 2.0, 0.0, g / 2.0];
    let eye = [center[0] - g, g, center[2] - g];
    let f = norm(sub(center, eye));
    let s = norm(cross(f, [0.0, 1.0, 0.0]));
    let u = cross(s, f);
    let view = [
        [s[0], u[0], -f[0], 0.0],
        [s[1], u[1], -f[1], 0.0],
        [s[2], u[2], -f[2], 0.0],
        [-dot(s, eye), -dot(u, eye), dot(f, eye), 1.0],
    ];
    // tan(22.5 degrees): a 45 degree vertical field of view.
    let t = 0.414_213_57_f32;
    let (near, far) = (0.05 * g, 4.0 * g);
    let fl = 1.0 / t;
    let aspect = W as f32 / H as f32;
    let proj = [
        [fl / aspect, 0.0, 0.0, 0.0],
        [0.0, fl, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), -1.0],
        [0.0, 0.0, near * far / (near - far), 0.0],
    ];
    mul(proj, view)
}

/// a * b for column-major matrices.
pub fn mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut m = [[0.0; 4]; 4];
    for c in 0..4 {
        for r in 0..4 {
            m[c][r] = a[0][r] * b[c][0] + a[1][r] * b[c][1] + a[2][r] * b[c][2] + a[3][r] * b[c][3];
        }
    }
    m
}

/// The unit cube's six faces: outward normal, then four corners in
/// counter-clockwise order seen from outside.
pub const FACES: [([f32; 3], [[f32; 3]; 4]); 6] = [
    ([1.0, 0.0, 0.0], [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]]),
    ([-1.0, 0.0, 0.0], [[0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]]),
    ([0.0, 1.0, 0.0], [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]]),
    ([0.0, -1.0, 0.0], [[0.0, 0.0, 1.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0]]),
    ([0.0, 0.0, 1.0], [[1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0], [0.0, 0.0, 1.0]]),
    ([0.0, 0.0, -1.0], [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]]),
];

/// The color of a face under the light, as both renderers must compute it.
pub fn shade(color: [u8; 4], normal: [f32; 3]) -> [f32; 3] {
    let lambert = dot(normal, LIGHT).max(0.0);
    let k = AMBIENT + (1.0 - AMBIENT) * lambert;
    [color[0] as f32 / 255.0 * k, color[1] as f32 / 255.0 * k, color[2] as f32 / 255.0 * k]
}
