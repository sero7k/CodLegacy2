// The material shader (docs/JUP.md, "Native renderer"): the terms the game's
// layered surface programs were decoded to use, written from the decoded data
// because the zones store no material-to-program link.
//
//  - Up to four layers. Layer l has a colour picture (record slot l), a packed
//    normal map (slot 4 + l) and, for l >= 1, a blend mask (slot 12 + l) the
//    program compares with 128/255.
//  - Colours blend in a gamma-2 space: the sample is square-rooted, mixed, and
//    squared back.
//  - The packed normal (usage 0x37, green g and alpha a): p = 2g - 1,
//    q = 2a - 1, x = (p + q) / 2, y = (p - q) / 2, z = 1 - |x| - |y|, then
//    normalised. Red is the gloss, blue the occlusion.
//
// The G-buffer gets albedo and occlusion, the world normal and the gloss.

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) a0: vec4<f32>,
    @location(4) a1: vec4<f32>,
    @location(5) a2: vec4<f32>,
    @location(6) origin: vec4<f32>,
    @location(7) blend: vec4<f32>,
    @location(8) uv2: vec2<f32>,
}

fn world_of(v: VIn) -> vec3<f32> {
    return v.origin.xyz + (v.pos.x * v.a0.xyz + v.pos.y * v.a1.xyz + v.pos.z * v.a2.xyz) * v.a0.w;
}

fn normal_of(v: VIn) -> vec3<f32> {
    return normalize(v.normal.x * v.a0.xyz + v.normal.y * v.a1.xyz + v.normal.z * v.a2.xyz);
}

@group(0) @binding(0) var<uniform> g: G;

// The sun's depth pass.
@vertex
fn vs_shadow(v: VIn) -> @builtin(position) vec4<f32> {
    return g.sun_vp * vec4<f32>(world_of(v), 1.0);
}

struct VOut {
    @builtin(position) p: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    // The vertex's layer blend weights (x is 1 where it has them, else all 0).
    @location(3) blend: vec4<f32>,
    // The second texture coordinate set.
    @location(4) uv2: vec2<f32>,
}

@vertex
fn vs_main(v: VIn) -> VOut {
    var o: VOut;
    let rel = world_of(v) - g.eye.xyz;
    o.p = g.vp * vec4<f32>(rel, 1.0);
    o.rel = rel;
    o.normal = normal_of(v);
    o.uv = v.uv;
    o.blend = v.blend;
    o.uv2 = v.uv2;
    return o;
}

struct Flags {
    // x: layers, y: cut-out, z: has colour (layer 0), w: bit l - 1 set where upper layer l has a
    // mask, bit l + 3 where it is sampled with the second texture coordinate set
    a: vec4<f32>,
    // 1 where the layer has a packed normal map
    n: vec4<f32>,
    // The albedo a layer with no colour picture shows (rgb); with w = 1, the paint
    // the colour picture is multiplied by instead.
    t: vec4<f32>,
    // Each layer's colour multiplier (rgb).
    tint: array<vec4<f32>, 4>,
}

@group(1) @binding(0) var colour0: texture_2d<f32>;
@group(1) @binding(1) var colour1: texture_2d<f32>;
@group(1) @binding(2) var colour2: texture_2d<f32>;
@group(1) @binding(3) var colour3: texture_2d<f32>;
@group(1) @binding(4) var normal0: texture_2d<f32>;
@group(1) @binding(5) var normal1: texture_2d<f32>;
@group(1) @binding(6) var normal2: texture_2d<f32>;
@group(1) @binding(7) var normal3: texture_2d<f32>;
@group(1) @binding(8) var mask1: texture_2d<f32>;
@group(1) @binding(9) var mask2: texture_2d<f32>;
@group(1) @binding(10) var mask3: texture_2d<f32>;
@group(1) @binding(11) var surf_samp: sampler;
@group(1) @binding(12) var<uniform> flags: Flags;

struct FOut {
    @location(0) albedo: vec4<f32>,
    @location(1) normal: vec4<f32>,
}

// The tangent-space normal of a packed sample, with its gloss and occlusion.
fn unpack(s: vec4<f32>) -> vec3<f32> {
    let p = s.g * 2.0 - 1.0;
    let q = s.a * 2.0 - 1.0;
    let x = (p + q) * 0.5;
    let y = (p - q) * 0.5;
    let z = 1.0 - abs(x) - abs(y);
    return normalize(vec3<f32>(x, y, max(z, 0.05)));
}

// A layer's weight from its mask: on where the mask passes 128/255. Where the
// vertex carries the layer's painted weight `vw` (world surfaces), the mask is a
// height the weight reveals: none of the layer where the weight is 0, all of it
// where it is 1, and in between the mask's high parts first.
fn weight(m: f32, vw: f32, painted: bool) -> f32 {
    if (painted) {
        return smoothstep(-0.12, 0.12, vw + m - 1.0);
    }
    return smoothstep(0.47, 0.53, m);
}

fn gamma2(c: vec3<f32>) -> vec3<f32> {
    return sqrt(max(c, vec3<f32>(0.0)));
}

// A hash and value noise on the position, for surfaces with no picture: a stand-in
// albedo that is one flat colour over a whole roof reads as a flat grey.
fn hash3(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.3183099 + vec3<f32>(0.1, 0.2, 0.3));
    q = q * 17.0;
    return fract(q.x * q.y * q.z * (q.x + q.y + q.z));
}

fn vnoise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(mix(hash3(i), hash3(i + vec3<f32>(1.0, 0.0, 0.0)), u.x), mix(hash3(i + vec3<f32>(0.0, 1.0, 0.0)), hash3(i + vec3<f32>(1.0, 1.0, 0.0)), u.x), u.y);
    let b = mix(mix(hash3(i + vec3<f32>(0.0, 0.0, 1.0)), hash3(i + vec3<f32>(1.0, 0.0, 1.0)), u.x), mix(hash3(i + vec3<f32>(0.0, 1.0, 1.0)), hash3(i + vec3<f32>(1.0, 1.0, 1.0)), u.x), u.y);
    return mix(a, b, u.z);
}

// The stand-in albedo of a material with no colour picture, at a world position.
fn stand_in(world: vec3<f32>) -> vec3<f32> {
    var tint = flags.t.xyz;
    if (tint.x + tint.y + tint.z <= 0.0) {
        tint = vec3<f32>(0.22);
    }
    let coarse = vnoise(world * 0.004);
    let fine = vnoise(world * 0.05);
    let patch_ = vnoise(world * 0.0007 + vec3<f32>(7.0, 3.0, 1.0));
    return tint * (0.72 + 0.5 * coarse + 0.22 * fine) * (0.9 + 0.2 * patch_);
}

// What a surface looks like at a pixel, before any lighting.
struct Surf {
    base: vec3<f32>,
    alpha: f32,
    n: vec3<f32>,
    gloss: f32,
    occlusion: f32,
}

// (Every sample is taken here, in uniform control flow.)
fn evaluate(i: VOut) -> Surf {
    let dp1 = dpdx(i.rel);
    let dp2 = dpdy(i.rel);
    let duv1 = dpdx(i.uv);
    let duv2 = dpdy(i.uv);
    let c0 = textureSample(colour0, surf_samp, i.uv);
    let sets = u32(flags.a.w + 0.5);
    let uv1 = select(i.uv, i.uv2, (sets & 16u) != 0u);
    let uv2 = select(i.uv, i.uv2, (sets & 32u) != 0u);
    let uv3 = select(i.uv, i.uv2, (sets & 64u) != 0u);
    let c1 = textureSample(colour1, surf_samp, uv1);
    let c2 = textureSample(colour2, surf_samp, uv2);
    let c3 = textureSample(colour3, surf_samp, uv3);
    let n0 = textureSample(normal0, surf_samp, i.uv);
    let n1 = textureSample(normal1, surf_samp, uv1);
    let n2 = textureSample(normal2, surf_samp, uv2);
    let n3 = textureSample(normal3, surf_samp, uv3);
    let m1 = textureSample(mask1, surf_samp, uv1).r;
    let m2 = textureSample(mask2, surf_samp, uv2).r;
    let m3 = textureSample(mask3, surf_samp, uv3).r;
    let layers = flags.a.x;
    let painted = i.blend.x > 0.5;
    // (An upper layer without a mask of its own: a flat 0.5 the painted weight
    // reveals past half; with no painted weight it is not drawn.)
    let bits = u32(flags.a.w + 0.5);
    let k1 = select(select(0.0, 0.5, painted), m1, (bits & 1u) != 0u);
    let k2 = select(select(0.0, 0.5, painted), m2, (bits & 2u) != 0u);
    let k3 = select(select(0.0, 0.5, painted), m3, (bits & 4u) != 0u);
    let w1 = select(0.0, weight(k1, i.blend.y, painted), layers > 1.5);
    let w2 = select(0.0, weight(k2, i.blend.z, painted), layers > 2.5);
    let w3 = select(0.0, weight(k3, i.blend.w, painted), layers > 3.5);

    // (No colour picture: the material's stand-in albedo, varied over the surface.
    // A material with only upper layers blends them over it.)
    let world = i.rel + g.eye.xyz;
    var base = select(stand_in(world), vec3<f32>(0.22), flags.t.w > 0.5);
    if (flags.a.z > 0.5 || layers > 1.5) {
        var c = gamma2(select(base, c0.rgb * flags.tint[0].xyz, flags.a.z > 0.5));
        c = mix(c, gamma2(c1.rgb * flags.tint[1].xyz), w1);
        c = mix(c, gamma2(c2.rgb * flags.tint[2].xyz), w2);
        c = mix(c, gamma2(c3.rgb * flags.tint[3].xyz), w3);
        base = c * c;
        if (flags.t.w > 0.5) {
            base = base * flags.t.xyz;
        }
    }
    // (Albedo past 0.5 is a picture's white, which no weathered rooftop surface is
    // (white paint is about 0.7 new, less dirty): rolled off at a quarter so a white
    // layer is not a blown-out panel in the low sun. Was 0.65 and an eighth.)
    base = min(base, vec3<f32>(0.5)) + max(base - vec3<f32>(0.5), vec3<f32>(0.0)) * 0.25;

    var n = normalize(i.normal);
    if (dot(n, -i.rel) < 0.0) {
        n = -n;
    }
    var gloss = 0.35;
    var occlusion = 1.0;
    if (flags.n.x + flags.n.y + flags.n.z + flags.n.w > 0.5) {
        // Each layer's own normal, gloss and occlusion, blended by the same weights.
        var tn = select(vec3<f32>(0.0, 0.0, 1.0), unpack(n0), flags.n.x > 0.5);
        var gl = select(0.35, n0.r, flags.n.x > 0.5);
        var ao = select(1.0, n0.b, flags.n.x > 0.5);
        if (flags.n.y > 0.5) {
            tn = mix(tn, unpack(n1), w1);
            gl = mix(gl, n1.r, w1);
            ao = mix(ao, n1.b, w1);
        }
        if (flags.n.z > 0.5) {
            tn = mix(tn, unpack(n2), w2);
            gl = mix(gl, n2.r, w2);
            ao = mix(ao, n2.b, w2);
        }
        if (flags.n.w > 0.5) {
            tn = mix(tn, unpack(n3), w3);
            gl = mix(gl, n3.r, w3);
            ao = mix(ao, n3.b, w3);
        }
        tn = normalize(tn);
        // The tangent frame from the screen-space derivatives.
        let dp2perp = cross(dp2, n);
        let dp1perp = cross(n, dp1);
        let t = dp2perp * duv1.x + dp1perp * duv2.x;
        let b = dp2perp * duv1.y + dp1perp * duv2.y;
        let inv = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-20));
        // (A frame whose two axes differ greatly in length, or lie along each other,
        // comes from texture coordinates on a line: keep the surface's own normal.)
        let lt = dot(t, t);
        let lb = dot(b, b);
        let lo = min(lt, lb);
        let hi = max(lt, lb);
        let skew = abs(dot(t, b)) / max(sqrt(lt * lb), 1e-20);
        let sound = select(0.0, 1.0, lo > 0.01 * hi && skew < 0.9);
        n = normalize(mix(n, t * inv * tn.x + b * inv * tn.y + n * tn.z, sound));
        gloss = gl;
        occlusion = ao;
    }
    var s: Surf;
    s.base = base;
    s.alpha = c0.a;
    s.n = n;
    s.gloss = gloss;
    s.occlusion = occlusion;
    return s;
}

@fragment
fn fs_main(i: VOut) -> FOut {
    let s = evaluate(i);
    if (flags.a.y > 0.5 && s.alpha < 0.5) {
        discard;
    }
    var o: FOut;
    o.albedo = vec4<f32>(s.base, s.occlusion);
    o.normal = vec4<f32>(s.n, s.gloss);
    return o;
}
