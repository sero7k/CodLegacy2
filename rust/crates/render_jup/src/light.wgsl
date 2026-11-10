// Lighting: the G-buffer, the sun (with its shadow map), the ambient light of
// the nearest reflection probe (the map carries no lightmaps), the vision
// set's fog toward and away from the sun, and a sky where nothing was
// drawn. Linear HDR out.

@group(0) @binding(0) var<uniform> g: G;

@group(1) @binding(0) var albedo_tex: texture_2d<f32>;
@group(1) @binding(1) var normal_tex: texture_2d<f32>;
@group(1) @binding(2) var depth_tex: texture_depth_2d;
@group(1) @binding(3) var shadow_tex: texture_depth_2d;
@group(1) @binding(4) var shadow_samp: sampler_comparison;
// Four vec4 a probe: origin, up, side, down.
@group(1) @binding(5) var<storage, read> probes: array<vec4<f32>>;
// The probes' pictures, one layer each (rgba16float, a few mip levels).
@group(1) @binding(6) var probe_maps: texture_2d_array<f32>;
@group(1) @binding(7) var probe_samp: sampler;
// The map's sky (a 1x1 black cube when it has none; `g.sky_zenith.w` says).
@group(1) @binding(8) var sky_cube: texture_cube<f32>;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn shadow_at(world: vec3<f32>, n: vec3<f32>, ndl: f32) -> f32 {
    if (g.misc.x < 0.5) {
        return 1.0;
    }
    // Offset along the normal by a few texels, more at a grazing angle.
    let off = n * g.misc.z * (1.5 + 3.0 * (1.0 - ndl));
    let c = g.sun_vp * vec4<f32>(world + off, 1.0);
    let uv = vec2<f32>(c.x * 0.5 + 0.5, 0.5 - c.y * 0.5);
    if (c.z > 1.0 || c.z < 0.0 || uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return 1.0;
    }
    let texel = 1.0 / f32(textureDimensions(shadow_tex).x);
    var s = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            s += textureSampleCompareLevel(shadow_tex, shadow_samp, uv + vec2<f32>(f32(x), f32(y)) * texel, c.z - 0.0008);
        }
    }
    return s / 9.0;
}

// Screen-space ambient occlusion: twelve points in the hemisphere over the
// normal (up to `SSAO_RADIUS` units, more of them near), each projected into
// the depth buffer; one the drawn scene stands in front of (and within the
// radius of it, so a far background does not darken a near edge) is
// occluded. The pattern turns per pixel. The map has no lightmaps and only
// a few probes, so nothing else darkens the corners, the feet of walls and
// the undersides of props. Returns the share of ambient light kept.
const SSAO_RADIUS: f32 = 40.0;

fn ssao(rel: vec3<f32>, n: vec3<f32>, px: vec2<i32>) -> f32 {
    if (g.amb_misc.y <= 0.0) {
        return 1.0;
    }
    let kernel = array<vec3<f32>, 12>(
        vec3<f32>(0.53, 0.18, 0.30), vec3<f32>(-0.42, 0.37, 0.21), vec3<f32>(0.08, -0.61, 0.35),
        vec3<f32>(-0.25, -0.28, 0.62), vec3<f32>(0.71, -0.33, 0.42), vec3<f32>(-0.66, -0.12, 0.55),
        vec3<f32>(0.19, 0.74, 0.47), vec3<f32>(-0.10, 0.12, 0.93), vec3<f32>(0.36, 0.05, 0.12),
        vec3<f32>(-0.17, 0.21, 0.09), vec3<f32>(0.04, -0.22, 0.15), vec3<f32>(-0.31, -0.06, 0.24),
    );
    // A turn of the pattern per pixel (a 4 x 4 interleave).
    let k = f32((px.x & 3) * 4 + (px.y & 3)) * 0.3926991;
    let up = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.z) > 0.9);
    let t0 = normalize(cross(up, n));
    let b0 = cross(n, t0);
    let t = t0 * cos(k) + b0 * sin(k);
    let b = cross(n, t);
    let dist = length(rel);
    // (Far off, a pixel covers more than the radius: the term fades out.)
    let radius = SSAO_RADIUS * clamp(dist / 400.0, 1.0, 3.0);
    var occluded = 0.0;
    for (var i = 0; i < 12; i++) {
        let o = kernel[i];
        let p = rel + (t * o.x + b * o.y + n * (o.z + 0.05)) * radius;
        let c = g.vp * vec4<f32>(p, 1.0);
        if (c.w <= 0.0) {
            continue;
        }
        let ndc = c.xyz / c.w;
        let sp = vec2<i32>(vec2<f32>((ndc.x * 0.5 + 0.5) * g.size.x, (0.5 - ndc.y * 0.5) * g.size.y));
        if (sp.x < 0 || sp.y < 0 || sp.x >= i32(g.size.x) || sp.y >= i32(g.size.y)) {
            continue;
        }
        let d = textureLoad(depth_tex, sp, 0);
        // (Reversed depth: nearer is larger.)
        if (d > ndc.z * 1.0005) {
            let h = g.inv_vp * vec4<f32>(ndc.xy, d, 1.0);
            let scene = h.xyz / h.w;
            let gap = length(p) - length(scene);
            occluded += 1.0 - smoothstep(radius * 0.5, radius * 1.5, gap);
        }
    }
    let fade = 1.0 - smoothstep(2500.0, 5000.0, dist);
    return 1.0 - clamp(0.8 * g.amb_misc.y, 0.0, 1.0) * fade * occluded / 12.0;
}

@fragment
fn fs_light(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(p.xy);
    let d = textureLoad(depth_tex, px, 0);
    let ndc = vec2<f32>(p.x * g.size.z * 2.0 - 1.0, 1.0 - p.y * g.size.w * 2.0);
    if (d <= 0.0) {
        // Nothing drawn: the sky, along the ray through this pixel.
        let far = g.inv_vp * vec4<f32>(ndc, 0.0001, 1.0);
        let dir = normalize(far.xyz / far.w);
        return vec4<f32>(fog_sky(sky(dir), dir), 1.0);
    }
    let h = g.inv_vp * vec4<f32>(ndc, d, 1.0);
    let rel = h.xyz / h.w;
    let world = rel + g.eye.xyz;
    var a = textureLoad(albedo_tex, px, 0);
    let nm = textureLoad(normal_tex, px, 0);
    let n = normalize(nm.xyz);
    let gloss = clamp(nm.w, 0.0, 1.0);
    // (Glass and wet surfaces: where the gloss is mirror-like the surface's own
    // colour gives way to what it reflects. The window panes of the towers are
    // only in their normal maps' gloss over a brick or concrete colour; drawn at
    // that colour they read as cardboard.)
    a = vec4<f32>(a.rgb * mix(1.0, 0.35, smoothstep(0.72, 0.95, gloss)), a.a);
    let v = normalize(-rel);
    let l = g.sun_dir.xyz;
    let ndl = max(dot(n, l), 0.0);
    let ndv = max(dot(n, v), 1e-3);
    let sh = select(1.0, shadow_at(world, n, ndl), ndl > 0.0);
    let probe = nearest_probe(world);
    let vis = sky_vis(world, n);
    // Gloss to GGX roughness (not under 0.1: the sun is a disc, and a mirror finish
    // answers it with a white blot); dielectrics (F0 0.04).
    let rough = clamp((1.0 - gloss) * (1.0 - gloss), 0.1, 1.0);
    let f0 = 0.04;
    var lit = a.rgb * g.sun_col.xyz * (ndl * sh);
    // The sun's highlight: GGX with Smith visibility and Schlick Fresnel.
    let hv = normalize(l + v);
    let vh = max(dot(v, hv), 0.0);
    let fresnel = f0 + (1.0 - f0) * pow(1.0 - vh, 5.0);
    let spec = ggx_d(max(dot(n, hv), 0.0), rough) * smith_g(ndv, max(ndl, 1e-3), rough) * fresnel / (4.0 * ndv * max(ndl, 1e-3));
    lit += g.sun_col.xyz * (spec * ndl * sh);
    let ao = ssao(rel, n, px);
    lit += a.rgb * ambient_at(world, n, probe, vis) * (a.a * ao);
    // The environment's reflection (sky, ground, a probe near it) with the
    // split-sum Fresnel approximation.
    {
        let r = reflect(-v, n);
        let fr = f0 + (max(1.0 - rough, f0) - f0) * pow(1.0 - ndv, 5.0);
        lit += env_reflection(world, probe, r, rough, vis) * fr * (a.a * ao);
    }
    return vec4<f32>(fog_over(lit, rel, world), 1.0);
}
