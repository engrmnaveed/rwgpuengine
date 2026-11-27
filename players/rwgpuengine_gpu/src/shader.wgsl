// The players' renderer: a render list drawn by the rules of the CPU
// reference renderer (rwgpuengine_render::reference). What each rule is and
// why it is done this way: docs/decisions/0021.
//
// Depth is reversed: 1 is the nearest the camera draws, the farthest is next
// to 0, and the larger value wins. A float is densest near 0. `held` below
// maps the reference's depth so that this precision is where the scene is.

struct Globals {
    // World point (taken from `origin`) to clip space.
    clip: mat4x4<f32>,
    // xyz: the point positions are taken from.
    origin: vec4<f32>,
    // The reference's depth of a level plane at height z0, at pixel center p:
    //   dot(la.xyz * z0 + lb.xyz, (p, 1)) / (1 + ld.x * z0)
    la: vec4<f32>,
    lb: vec4<f32>,
    // x: see above. y: 0 in perspective, else the iso view's depth scale.
    // z: the farthest depth drawn. w: 1 / (z + 1).
    ld: vec4<f32>,
    // 2 / width, 2 / height, width, height.
    frame: vec4<f32>,
    // Iso: the reference's depth of a world point (taken from `origin`).
    zrow: vec4<f32>,
    // Study only: the light on each of the six faces.
    k: array<vec4<f32>, 2>,
    flags: vec4<u32>,
    // The color of a face under the light, from the reference's own
    // function: byte `face * 256 + level`.
    shade: array<vec4<u32>, 96>,
}

@group(0) @binding(0) var<uniform> g: Globals;
// Iso: the pixel column of every corner of every instance of a model, in
// 1/256 of a pixel, computed and snapped as the reference does (`vs_model`).
@group(0) @binding(1) var<storage, read> columns: array<i32>;
// The picture so far, for see-through colors (the `_x` fragment shaders).
@group(1) @binding(0) var prev: texture_2d<f32>;

// Study options (ADR 0021); all off in the renderer players get.
const SHADE_IN_SHADER: u32 = 1u;
const CARD_ONE_DEPTH: u32 = 4u;
// The iso view: the CPU gives each corner's pixel column (`MeshIn.columns`).
const ISO_COLUMNS: u32 = 8u;

fn lut(face: u32, level: u32) -> u32 {
    let i = face * 256u + level;
    return (g.shade[i >> 4u][(i >> 2u) & 3u] >> ((i & 3u) * 8u)) & 255u;
}

fn shade(face: u32, c: vec4<u32>) -> vec4<u32> {
    if (g.flags.x & SHADE_IN_SHADER) != 0u {
        let k = g.k[face >> 2u][face & 3u];
        let v = clamp(floor(vec3<f32>(c.rgb) * k + 0.5), vec3<f32>(0.0), vec3<f32>(255.0));
        return vec4<u32>(vec3<u32>(v), c.a);
    }
    return vec4<u32>(lut(face, c.r), lut(face, c.g), lut(face, c.b), c.a);
}

// A whole 8-bit level per channel: nothing is left for the target to round.
fn rgba(c: vec4<u32>) -> vec4<f32> {
    return vec4<f32>(c) / 255.0;
}

// Source-over onto the picture so far, in the reference's integer arithmetic.
fn over(c: vec4<u32>, px: vec2<i32>) -> vec4<f32> {
    let d = vec3<u32>(textureLoad(prev, px, 0).rgb * 255.0 + 0.5);
    let m = (c.rgb * c.a + d * (255u - c.a) + 127u) / 255u;
    return vec4<f32>(vec3<f32>(m) / 255.0, 1.0);
}

// The reference's depth (-1 nearest, `ld.z` farthest) as the depth buffer
// holds it.
//
// In perspective the reference's depth already is dense where it matters, and
// a straight line to 1..0 keeps that. In the iso view the scene sits around
// depth 0, in the middle of the range, where a straight line would land at
// 0.5 and leave a float 100 times coarser than the reference's. So the iso
// range is laid along the exponent instead: every 1/60 of it halves the
// value, and within such a step the value falls in a straight line. That is
// monotonic, exact (the float is put together from its bits), and about 60
// times finer everywhere.
fn held(d: f32) -> f32 {
    if g.ld.y == 0.0 {
        return clamp((g.ld.z - d) * g.ld.w, 0.0, 1.0);
    }
    let u = g.ld.y * clamp(d, -1.0, 1.0);
    let n = floor(u);
    // 2 - (u - n) is in 1..2; lower its exponent by n + 61.
    return bitcast<f32>(bitcast<u32>(2.0 - (u - n)) - (u32(i32(n) + i32(g.ld.y) + 1) << 23u));
}

// The reference's depth of the level plane at height z0, at pixel center p.
fn level_depth(p: vec2<f32>, z0: f32) -> f32 {
    return dot(g.la.xyz * z0 + g.lb.xyz, vec3<f32>(p, 1.0)) / (1.0 + g.ld.x * z0);
}

// A level plane's held depth as bits. Every face in one plane computes the
// same bits here, whatever triangle it is part of. What lies in the plane
// adds a few steps to them, and the larger number is the nearer one:
//   a level face         2 x its rank (ranks 1 to 6)
//   a card lying there   14 (a lifted card at least 15)
// Two steps apart, so that a last-bit difference between two shaders cannot
// change the order. Nothing is rounded away: against a face that is not in
// the plane, these depths are within a few steps of the true one.
fn level_bits(p: vec2<f32>, z0: f32) -> u32 {
    return bitcast<u32>(held(level_depth(p, z0)));
}

// ---------------------------------------------------------------------------
// Instances: boxes and flat rectangles.

struct MeshIn {
    // A corner of the unit box (x, y, z) and the face it belongs to.
    @location(0) corner: vec4<u32>,
    @location(1) pos: vec3<f32>,
    @location(2) size: vec3<f32>,
    @location(3) color: vec4<u32>,
    @location(4) top: vec4<u32>,
    // x: the rank of the level faces.
    @location(5) extra: vec4<u32>,
    // Iso: the pixel column of each of the four corners (x, y), in 1/256 of
    // a pixel, computed and snapped as the reference does.
    @location(6) columns: vec4<i32>,
}

struct MeshOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat, either) color: vec4<u32>,
    // x: the height of a level face.
    @location(1) @interpolate(flat, either) plane: vec2<f32>,
    @location(2) @interpolate(flat, either) rank: u32,
    // Iso: the reference's depth, for sides.
    @location(3) depth: f32,
}

@vertex
fn vs_mesh(v: MeshIn) -> MeshOut {
    let c = vec3<f32>(v.corner.xyz);
    let world = (v.pos - g.origin.xyz) + c * v.size;
    let face = v.corner.w;
    var o: MeshOut;
    o.clip = g.clip * vec4<f32>(world, 1.0);
    if (g.flags.x & ISO_COLUMNS) != 0u {
        // A box's upright edges are one pixel column from top to bottom. In
        // single precision that column can land one step beside the
        // reference's, and then a whole edge is drawn a pixel off.
        o.clip.x = f32(v.columns[v.corner.x + 2u * v.corner.y]) * (g.frame.x / 256.0) - 1.0;
    }
    o.color = shade(face, select(v.color, v.top, face >= 4u));
    o.plane = vec2<f32>(v.pos.z + c.z * v.size.z, 0.0);
    o.rank = v.extra.x;
    o.depth = dot(g.zrow, vec4<f32>(world, 1.0));
    return o;
}

// Models: the solids of a `[[shape]]` list as one mesh, an instance for
// each entity. The CPU bakes the mesh with the reference's own code, so a
// corner is where the reference has it and a face has the color the
// reference gives it under the light; the fragment shaders are the boxes'.

struct ModelIn {
    // A corner, from the instance's place.
    @location(0) pos: vec3<f32>,
    // Its face's color under the light.
    @location(1) color: vec4<u32>,
    // x: the rank of a level face. y, z: the corner's column, low and high byte.
    @location(2) extra: vec4<u32>,
    // The instance: where it stands, and its first column in `columns`.
    @location(3) at: vec3<f32>,
    @location(4) first: u32,
}

@vertex
fn vs_model(v: ModelIn) -> MeshOut {
    let world = (v.at - g.origin.xyz) + v.pos;
    var o: MeshOut;
    o.clip = g.clip * vec4<f32>(world, 1.0);
    if (g.flags.x & ISO_COLUMNS) != 0u {
        o.clip.x = f32(columns[v.first + v.extra.y + 256u * v.extra.z]) * (g.frame.x / 256.0) - 1.0;
    }
    o.color = v.color;
    o.plane = vec2<f32>(v.at.z + v.pos.z, 0.0);
    o.rank = v.extra.x;
    o.depth = dot(g.zrow, vec4<f32>(world, 1.0));
    return o;
}

struct DepthOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

// Sides in perspective: the rasterizer's own depth.
@fragment
fn fs_side(i: MeshOut) -> @location(0) vec4<f32> {
    return rgba(i.color);
}

@fragment
fn fs_side_x(i: MeshOut) -> @location(0) vec4<f32> {
    return over(i.color, vec2<i32>(floor(i.clip.xy)));
}

// Sides in the iso view: the reference's depth, as `held` stores it.
@fragment
fn fs_side_iso(i: MeshOut) -> DepthOut {
    var o: DepthOut;
    o.color = rgba(i.color);
    o.depth = held(i.depth);
    return o;
}

@fragment
fn fs_side_iso_x(i: MeshOut) -> DepthOut {
    var o: DepthOut;
    o.color = over(i.color, vec2<i32>(floor(i.clip.xy)));
    o.depth = held(i.depth);
    return o;
}

// Level faces: the plane's depth, two steps nearer per rank. Of two faces in
// one plane the higher rank is the nearer one, and equal ranks are equal
// depths (the first drawn stays).
@fragment
fn fs_level(i: MeshOut) -> DepthOut {
    var o: DepthOut;
    o.color = rgba(i.color);
    o.depth = bitcast<f32>(level_bits(i.clip.xy, i.plane.x) + 2u * i.rank);
    return o;
}

@fragment
fn fs_level_x(i: MeshOut) -> DepthOut {
    var o: DepthOut;
    o.color = over(i.color, vec2<i32>(floor(i.clip.xy)));
    o.depth = bitcast<f32>(level_bits(i.clip.xy, i.plane.x) + 2u * i.rank);
    return o;
}

// ---------------------------------------------------------------------------
// Flat shapes in whole pixels: cards (sprites and text in the world) and the
// 2D layer. Coverage is the reference's integer rule, tested per pixel.

struct ShapeIn {
    @builtin(vertex_index) vi: u32,
    // The shape's bounding box: x0, y0, x1, y1.
    @location(0) rect: vec4<i32>,
    // Quad: its four corners in a and b. Glyph: a = scale, bits 0..31, bits 32..39.
    @location(1) a: vec4<i32>,
    @location(2) b: vec4<i32>,
    @location(3) color: vec4<u32>,
    // x: 0 rectangle, 1 ellipse, 2 quad, 3 glyph. y: the card's lift.
    @location(4) kind: vec4<u32>,
    // Cards: the anchor's row, the depth there, its change per row above the
    // anchor, the anchor's height.
    @location(5) d0: vec4<f32>,
    // Cards: what the lift adds to the depth at the anchor, per row above and
    // per row below; and the change per row below the anchor.
    @location(6) d1: vec4<f32>,
}

struct ShapeOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat, either) rect: vec4<i32>,
    @location(1) @interpolate(flat, either) a: vec4<i32>,
    @location(2) @interpolate(flat, either) b: vec4<i32>,
    @location(3) @interpolate(flat, either) color: vec4<u32>,
    @location(4) @interpolate(flat, either) kind: vec4<u32>,
    @location(5) @interpolate(flat, either) d0: vec4<f32>,
    @location(6) @interpolate(flat, either) d1: vec4<f32>,
}

@vertex
fn vs_shape(v: ShapeIn) -> ShapeOut {
    // Two triangles over the bounding box, kept near the frame.
    let right = ((0x16u >> v.vi) & 1u) == 1u;
    let low = ((0x34u >> v.vi) & 1u) == 1u;
    let x = clamp(f32(select(v.rect.x, v.rect.z, right)), -1.0, g.frame.z + 1.0);
    let y = clamp(f32(select(v.rect.y, v.rect.w, low)), -1.0, g.frame.w + 1.0);
    var o: ShapeOut;
    o.clip = vec4<f32>(x * g.frame.x - 1.0, 1.0 - y * g.frame.y, 0.5, 1.0);
    o.rect = v.rect;
    o.a = v.a;
    o.b = v.b;
    o.color = v.color;
    o.kind = v.kind;
    o.d0 = v.d0;
    o.d1 = v.d1;
    return o;
}

// 32 x 32 -> 64 bits: (high, low).
fn mul64(a: u32, b: u32) -> vec2<u32> {
    let a0 = a & 0xffffu;
    let a1 = a >> 16u;
    let b0 = b & 0xffffu;
    let b1 = b >> 16u;
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let mid = (p00 >> 16u) + (p01 & 0xffffu) + (p10 & 0xffffu);
    return vec2<u32>(a1 * b1 + (p01 >> 16u) + (p10 >> 16u) + (mid >> 16u), (p00 & 0xffffu) | (mid << 16u));
}

fn add64(x: vec2<u32>, y: vec2<u32>) -> vec2<u32> {
    let low = x.y + y.y;
    return vec2<u32>(x.x + y.x + select(0u, 1u, low < x.y), low);
}

// Does the shape cover the center of pixel `px`? `px` is inside its box.
fn covers(i: ShapeOut, px: vec2<i32>) -> bool {
    let kind = i.kind.x;
    let l = px - i.rect.xy;
    if kind == 3u {
        // A glyph: 5 x 8 font pixels of `scale` pixels each, one bit apiece.
        let cell = l / max(i.a.x, 1);
        let bit = u32(cell.y * 5 + cell.x);
        let word = bitcast<u32>(select(i.a.y, i.a.z, bit >= 32u));
        return ((word >> (bit & 31u)) & 1u) == 1u;
    }
    if kind == 2u {
        // A convex quad, clockwise: doubled coordinates, so that pixel
        // centers are odd and corners even, and a top-left rule on its edges.
        let c = 2 * l + vec2<i32>(1, 1);
        var p = array<vec2<i32>, 4>(i.a.xy, i.a.zw, i.b.xy, i.b.zw);
        for (var k = 0; k < 4; k++) {
            let p0 = 2 * (p[k] - i.rect.xy);
            let d = 2 * (p[(k + 1) & 3] - i.rect.xy) - p0;
            let cross = d.x * (c.y - p0.y) - d.y * (c.x - p0.x);
            let top_left = (d.y == 0 && d.x > 0) || d.y < 0;
            if cross < 0 || (cross == 0 && !top_left) {
                return false;
            }
        }
        return true;
    }
    if kind == 1u {
        // An ellipse inscribed in the box: dx² h² + dy² w² <= w² h², in 64 bits.
        let w = u32(i.rect.z - i.rect.x);
        let h = u32(i.rect.w - i.rect.y);
        let dx = u32(abs(2 * l.x + 1 - i32(w)));
        let dy = u32(abs(2 * l.y + 1 - i32(h)));
        let lhs = add64(mul64(dx * dx, h * h), mul64(dy * dy, w * w));
        let rhs = mul64(w * w, h * h);
        return lhs.x < rhs.x || (lhs.x == rhs.x && lhs.y <= rhs.y);
    }
    return true;
}

// A card's held depth in pixel row `p.y`. Above its anchor the card stands
// upright; at and below it the card lies in the level plane through the
// anchor, in front of any floor there.
fn card_depth(i: ShapeOut, p: vec2<f32>) -> f32 {
    let dy = p.y - i.d0.x;
    let lift = i.kind.y;
    if (g.flags.x & CARD_ONE_DEPTH) != 0u {
        return held(i.d0.y + i.d1.x);
    }
    let plane = level_bits(p, i.d0.w);
    if dy < 0.0 {
        // The same bits for every card at this anchor, then the lift: a
        // label is in front of its own sprite.
        let upright = bitcast<u32>(held(i.d0.y + dy * i.d0.z + (i.d1.x + dy * i.d1.y)));
        // A card stands on its anchor's plane, never behind it. In the row
        // just above the anchor it can be upright by less than a depth can
        // hold: there it is at least where a card lying on the plane is.
        // Everything else the card meets, the top of a box it stands in for
        // one, it meets at its own depth.
        if dy > -1.0 {
            return bitcast<f32>(max(upright, plane + 14u + lift));
        }
        return bitcast<f32>(upright);
    }
    if lift == 0u {
        return bitcast<f32>(plane + 14u);
    }
    // Lifted off the plane: its own depth, and in front of what lies there.
    let lifted = bitcast<u32>(held(level_depth(p, i.d0.w) + (i.d1.x + dy * i.d1.z)));
    return bitcast<f32>(max(lifted, plane + 14u + lift));
}

@fragment
fn fs_card(i: ShapeOut) -> DepthOut {
    if !covers(i, vec2<i32>(floor(i.clip.xy))) {
        discard;
    }
    var o: DepthOut;
    o.color = rgba(i.color);
    o.depth = card_depth(i, i.clip.xy);
    return o;
}

@fragment
fn fs_card_x(i: ShapeOut) -> DepthOut {
    let px = vec2<i32>(floor(i.clip.xy));
    if !covers(i, px) {
        discard;
    }
    var o: DepthOut;
    o.color = over(i.color, px);
    o.depth = card_depth(i, i.clip.xy);
    return o;
}

// The 2D layer: in front of the world, in list order.
@fragment
fn fs_overlay(i: ShapeOut) -> @location(0) vec4<f32> {
    if !covers(i, vec2<i32>(floor(i.clip.xy))) {
        discard;
    }
    return rgba(i.color);
}

@fragment
fn fs_overlay_x(i: ShapeOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(floor(i.clip.xy));
    if !covers(i, px) {
        discard;
    }
    return over(i.color, px);
}
