// bo2mp: BO2MP_MV_VIEW, the main view's motion vectors (see motion.rs) drawn
// over the frame. Grey is still; red is motion in x, green in y, a pixel of
// motion to a sixteenth of the range, saturating at eight; blue 0.5.

@group(0) @binding(0) var motion: texture_2d<f32>;

struct FullscreenOut {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> FullscreenOut {
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    var out: FullscreenOut;
    out.position = vec4(uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
    return out;
}

@fragment
fn fragment(in: FullscreenOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(motion));
    let at = clamp(vec2<i32>(in.position.xy), vec2(0), size - vec2(1));
    // (UV units to pixels of the target.)
    let pixels = textureLoad(motion, at, 0).xy * vec2<f32>(size);
    let c = clamp(vec2(0.5) + pixels / 16.0, vec2(0.0), vec2(1.0));
    return vec4(c.x, c.y, 0.5, 1.0);
}
