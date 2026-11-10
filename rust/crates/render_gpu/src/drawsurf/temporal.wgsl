// bo2mp: the passes around the temporal upscalers (FSR 3.1, DLSS; temporal.rs).
// They take the lens's frame, its depth and the finished upscale and put them
// in the formats the upscalers ask for. One full-screen triangle each; every
// pass reads texel for texel (the source and the target have the same size).

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

// The colour in (the lens's frame read as plain Unorm, so the gamma-encoded
// values, or FSR's / DLSS's finished frame in Rgba16Float).
@group(0) @binding(0) var color_in: texture_2d<f32>;
// The lens's depth (the depth aspect of Depth24PlusStencil8).
@group(0) @binding(1) var depth_in: texture_depth_2d;

// The lens's frame as Rgba16Float, the format FSR 3.1 reads. Values as they are.
@fragment
fn copy_color(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(textureLoad(color_in, vec2<i32>(frag.xy), 0).rgb, 1.0);
}

// The lens's depth as R32Float (FSR 3.1 and DLSS read a 32-bit float image).
@fragment
fn copy_depth(@builtin(position) frag: vec4<f32>) -> @location(0) f32 {
    return textureLoad(depth_in, vec2<i32>(frag.xy), 0);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3(0.055)) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

// The upscaled frame onto the window's texture. FSR 3.1 and DLSS hand back the
// gamma-encoded colour they were given; an sRGB target wants it linear.
@fragment
fn present(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    var pix = saturate(textureLoad(color_in, vec2<i32>(frag.xy), 0).rgb);
#ifdef SRGB_OUT
    pix = srgb_to_linear(pix);
#endif
    return vec4(pix, 1.0);
}
