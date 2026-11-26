// Showing the finished frame in a window or on a canvas.

struct BlitOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@group(0) @binding(0) var blit_frame: texture_2d<f32>;
@group(0) @binding(1) var blit_sampler: sampler;

@vertex
fn vs_blit(@builtin(vertex_index) vi: u32) -> BlitOut {
    // One triangle that covers the target.
    let p = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var o: BlitOut;
    o.clip = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

@fragment
fn fs_blit(i: BlitOut) -> @location(0) vec4<f32> {
    return textureSample(blit_frame, blit_sampler, i.uv);
}

// For a target that encodes what is written to it as sRGB: the frame's
// bytes already are what the screen should show, so take the encoding out.
@fragment
fn fs_blit_srgb(i: BlitOut) -> @location(0) vec4<f32> {
    let c = textureSample(blit_frame, blit_sampler, i.uv);
    let low = c.rgb / 12.92;
    let high = pow((c.rgb + 0.055) / 1.055, vec3<f32>(2.4));
    return vec4<f32>(select(high, low, c.rgb <= vec3<f32>(0.04045)), c.a);
}
