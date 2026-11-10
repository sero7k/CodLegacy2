// The display stage: the scene colour times an exposure, then the sRGB
// transfer (1.055 x^(1/2.4) - 0.055; the game's own display program has this
// branch next to its 3D lookup table and its PQ fit, docs/JUP.md). A soft
// shoulder keeps a bright sun from clipping where the game's lookup table
// would roll it off. The image is display-encoded, as the host's hand-off
// expects.

@group(0) @binding(0) var<uniform> g: G;
@group(0) @binding(1) var hdr_tex: texture_2d<f32>;
// The map's colour grade (a 1x1x1 volume when it has none; `g.sky_horizon.w` says).
@group(0) @binding(2) var grade_tex: texture_3d<f32>;
@group(0) @binding(3) var grade_samp: sampler;

// The grade is indexed by the scene colour as (log2(x + 1/1024) + 10) / 20 and
// answers in the same encoding.
fn graded(c: vec3<f32>) -> vec3<f32> {
    let enc = clamp((log2(max(c, vec3<f32>(0.0)) + vec3<f32>(1.0 / 1024.0)) + vec3<f32>(10.0)) * 0.05, vec3<f32>(0.0), vec3<f32>(1.0));
    let uv = enc * (31.0 / 32.0) + vec3<f32>(0.5 / 32.0);
    let out = textureSampleLevel(grade_tex, grade_samp, uv, 0.0).rgb;
    return max(exp2(out * 20.0 - vec3<f32>(10.0)) - vec3<f32>(1.0 / 1024.0), vec3<f32>(0.0));
}

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn shoulder(x: vec3<f32>) -> vec3<f32> {
    // Linear to 0.6, then a rational roll-off to 1.
    let k = 0.6;
    let over = max(x - vec3<f32>(k), vec3<f32>(0.0));
    let r = (1.0 - k) * over / (over + (1.0 - k));
    return min(x, vec3<f32>(k)) + r;
}

// A filmic curve (the fitted ACES one): a toe that deepens the shadows, a mid
// slope over 1 for contrast, a long shoulder for the sun. The game's own curve is
// a 3D table not located; this stands in for its contrast, not its exact shape.
fn filmic(x: vec3<f32>) -> vec3<f32> {
    let a = x * (2.51 * x + vec3<f32>(0.03));
    let b = x * (2.43 * x + vec3<f32>(0.59)) + vec3<f32>(0.14);
    return clamp(a / b, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3<f32>(0.0031308));
}

@fragment
fn fs_display(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    var c = textureLoad(hdr_tex, vec2<i32>(p.xy), 0).rgb * g.misc.y;
    if (g.sky_horizon.w > 0.5) {
        c = graded(c);
    }
    if (g.amb_misc.w > 0.5) {
        return vec4<f32>(srgb_encode(filmic(max(c, vec3<f32>(0.0)))), 1.0);
    }
    return vec4<f32>(srgb_encode(shoulder(max(c, vec3<f32>(0.0)))), 1.0);
}
