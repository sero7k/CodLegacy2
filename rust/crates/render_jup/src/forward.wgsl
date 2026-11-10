// The blended surfaces: translucent materials and the sea, lit in one pass over
// the lit scene (after `fs_light`, depth tested against the scene's, not
// written). Included after the material shader, whose `evaluate` gives the
// surface; the sun is unshadowed here.

// Four vec4 a probe: origin, up, side, down.
@group(2) @binding(0) var<storage, read> probes: array<vec4<f32>>;
@group(2) @binding(1) var probe_maps: texture_2d_array<f32>;
@group(2) @binding(2) var probe_samp: sampler;
@group(2) @binding(3) var sky_cube: texture_cube<f32>;

@fragment
fn fs_forward(i: VOut) -> @location(0) vec4<f32> {
    let s = evaluate(i);
    let rel = i.rel;
    let world = rel + g.eye.xyz;
    let n = s.n;
    let v = normalize(-rel);
    let l = g.sun_dir.xyz;
    let ndl = max(dot(n, l), 0.0);
    let ndv = max(dot(n, v), 1e-3);
    let probe = nearest_probe(world);
    let vis = sky_vis(world, n);
    let rough = clamp((1.0 - s.gloss) * (1.0 - s.gloss), 0.1, 1.0);
    var lit = s.base * g.sun_col.xyz * ndl;
    let hv = normalize(l + v);
    let fresnel = 0.04 + 0.96 * pow(1.0 - max(dot(v, hv), 0.0), 5.0);
    lit += g.sun_col.xyz * (ggx_d(max(dot(n, hv), 0.0), rough) * smith_g(ndv, max(ndl, 1e-3), rough) * fresnel / (4.0 * ndv) * ndl);
    lit += s.base * ambient_at(world, n, probe, vis) * s.occlusion;
    {
        let fr = 0.04 + 0.96 * pow(1.0 - ndv, 5.0) * (1.0 - rough);
        lit += env_reflection(world, probe, reflect(-v, n), rough, vis) * fr * s.occlusion;
    }
    // A picture's alpha well above a decal's edge (0.1 to 0.3 and up) is glass or
    // a window pane; read as plain opacity it made whole facades and big panes
    // vanish (Elevated's towers), so half of what is missing from opaque is put back.
    let a = clamp(s.alpha, 0.0, 1.0);
    let opacity = a + (1.0 - a) * 0.5 * smoothstep(0.1, 0.3, a);
    return vec4<f32>(fog_over(lit, rel, world), opacity);
}

// The sea: the sky and sun mirrored by Fresnel over a deep tint (the fog's
// colour where the map has fog), ripples from a fixed pattern.
@fragment
fn fs_water(i: VOut) -> @location(0) vec4<f32> {
    let rel = i.rel;
    let world = rel + g.eye.xyz;
    let ripple = vec3<f32>(
        sin(world.x * 0.031 + world.y * 0.017) + 0.5 * sin(world.x * 0.083 - world.y * 0.059),
        cos(world.y * 0.027 - world.x * 0.013) + 0.5 * cos(world.y * 0.071 + world.x * 0.047),
        0.0,
    ) * 0.02;
    var n = normalize(vec3<f32>(0.0, 0.0, 1.0) + ripple);
    let v = normalize(-rel);
    if (dot(n, v) < 0.0) {
        n = vec3<f32>(n.x, n.y, -n.z);
    }
    let ndv = max(dot(n, v), 1e-3);
    let fresnel = 0.02 + 0.98 * pow(1.0 - ndv, 5.0);
    let deep = select(vec3<f32>(0.02, 0.07, 0.09), g.fog_col.xyz * 0.3, g.fog_col.w > 0.5);
    let l = g.sun_dir.xyz;
    let hv = normalize(l + v);
    let ndl = max(dot(n, l), 0.0);
    let spec = ggx_d(max(dot(n, hv), 0.0), 0.06) * smith_g(ndv, max(ndl, 1e-3), 0.06) * 0.02 / (4.0 * ndv);
    var lit = mix(deep * (0.4 + 0.6 * ndl), sky(reflect(-v, n)), fresnel);
    lit += g.sun_col.xyz * (spec * ndl);
    return vec4<f32>(fog_over(lit, rel, world), mix(0.9, 1.0, fresnel));
}
