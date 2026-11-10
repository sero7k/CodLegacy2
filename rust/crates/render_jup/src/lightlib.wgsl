// Shared by the lighting and forward passes: probe light, GGX terms, fog and sky.
// (The bindings `probes`, `probe_maps` and `probe_samp` are the including pass's.)

fn nearest_probe(world: vec3<f32>) -> u32 {
    let count = u32(g.ambient.w);
    var best = 0u;
    var best_d = 1e30;
    for (var k = 0u; k < count; k++) {
        let d = distance(probes[4u * k].xyz, world);
        if (d < best_d) {
            best_d = d;
            best = k;
        }
    }
    return best;
}

// How far the probe's light reaches `world`: 1 within the reach's begin, 0 past its end,
// scaled by the probe's share of the ambient light.
fn probe_share(probe: u32, world: vec3<f32>) -> f32 {
    if (u32(g.ambient.w) == 0u) {
        return 0.0;
    }
    let d = distance(probes[4u * probe].xyz, world);
    return g.amb_misc.x * (1.0 - smoothstep(g.grid2.z, g.grid2.w, d));
}

// The sky visibility grid: the highest geometry in a cell of the ground plan
// (after the probes in `probes`, four to a vec4).
fn grid_height(ix: i32, iy: i32) -> f32 {
    let cols = i32(g.grid.w);
    let rows = i32(g.grid2.x);
    if (ix < 0 || iy < 0 || ix >= cols || iy >= rows) {
        return -1.0e9;
    }
    let i = u32(iy * cols + ix);
    let v = probes[u32(g.grid2.y) + (i >> 2u)];
    return v[i & 3u];
}

// The share of the sky a point sees: eight directions over the ground plan, eight
// steps each, the highest horizon of the geometry along each (sin squared), the
// directions weighted toward where the normal faces. 1 in the open.
fn sky_vis(world: vec3<f32>, n: vec3<f32>) -> f32 {
    if (g.amb_misc.z < 0.5) {
        return 1.0;
    }
    let cell = g.grid.z;
    let z0 = world.z + 24.0;
    let side = sqrt(max(1.0 - n.z * n.z, 0.0));
    let nxy = n.xy / max(length(n.xy), 1e-4);
    var acc = 0.0;
    var wsum = 0.0;
    for (var a = 0; a < 8; a++) {
        let ang = f32(a) * 0.78539816;
        let dir = vec2<f32>(cos(ang), sin(ang));
        var worst = 0.0;
        var d = 36.0;
        for (var k = 0; k < 8; k++) {
            let p = world.xy + dir * d;
            let h = grid_height(i32(floor((p.x - g.grid.x) / cell)), i32(floor((p.y - g.grid.y) / cell)));
            let dz = h - z0;
            if (dz > 0.0) {
                worst = max(worst, dz * dz / (d * d + dz * dz));
            }
            d = d * 1.75;
        }
        let w = max(1.0 + side * dot(dir, nxy), 0.05);
        acc += w * worst;
        wsum += w;
    }
    // (Never fully dark: the facades around bounce light into a street, and a room
    // has its lights, which are not read; at most 55 per cent is taken, was 70: with
    // the stronger sun's lower ambient and the occlusion term, rooms went black.)
    return 1.0 - 0.55 * acc / wsum;
}

// The open sky's ambient light for a normal (up, side, down), the sky it sees cut by
// the geometry around.
fn sky_ambient(n: vec3<f32>, vis: f32) -> vec3<f32> {
    let up = max(n.z, 0.0);
    let down = max(-n.z, 0.0);
    let side = 1.0 - abs(n.z);
    return g.amb_up.xyz * (up * vis) + g.amb_down.xyz * down + g.amb_side.xyz * (side * (0.45 + 0.55 * vis));
}

// The ambient light at a point for a normal: the open sky's, seen through the
// geometry around, and near a probe that was read, the probe's up, side and down.
fn ambient_at(world: vec3<f32>, n: vec3<f32>, probe: u32, vis: f32) -> vec3<f32> {
    let sky_amb = sky_ambient(n, vis);
    let share = probe_share(probe, world);
    if (share <= 0.0) {
        return sky_amb;
    }
    let up = max(n.z, 0.0);
    let down = max(-n.z, 0.0);
    let side = 1.0 - abs(n.z);
    let pa = probes[4u * probe + 1u].xyz * up + probes[4u * probe + 3u].xyz * down + probes[4u * probe + 2u].xyz * side;
    return mix(sky_amb, pa, share);
}

// The probe's picture is octahedral (z up, the centre up, the corners down).
fn octahedral_uv(d: vec3<f32>) -> vec2<f32> {
    let p = d / (abs(d.x) + abs(d.y) + abs(d.z));
    var uv = p.xy;
    if (d.z < 0.0) {
        uv = vec2<f32>((1.0 - abs(p.y)) * sign(p.x), (1.0 - abs(p.x)) * sign(p.y));
    }
    return uv * 0.5 + vec2<f32>(0.5);
}

// The probe's light along a direction; rougher surfaces read a smaller level.
fn probe_radiance(probe: u32, d: vec3<f32>, roughness: f32) -> vec3<f32> {
    let levels = f32(textureNumLevels(probe_maps)) - 1.0;
    return textureSampleLevel(probe_maps, probe_samp, octahedral_uv(d), i32(probe), roughness * levels).rgb;
}

// The environment's light along a direction, for reflections: the map's sky (without
// its sun), the ground's bounce below the horizon, blurred toward the ambient light
// as the surface gets rougher, dimmed where the sky is hidden.
fn env_reflection(world: vec3<f32>, probe: u32, d: vec3<f32>, roughness: f32, vis: f32) -> vec3<f32> {
    var c = min(sky_env(d), vec3<f32>(1.5));
    let amb = 0.5 * (g.amb_up.xyz + g.amb_side.xyz);
    c = mix(c, amb, clamp(roughness * 1.6, 0.0, 1.0));
    c = c * mix(0.2, 1.0, clamp(vis, 0.0, 1.0));
    c = mix(g.amb_down.xyz, c, smoothstep(-0.15, 0.2, d.z));
    let share = probe_share(probe, world);
    if (share > 0.0) {
        c = mix(c, min(probe_radiance(probe, d, roughness), vec3<f32>(3.0)), share);
    }
    return c;
}

fn ggx_d(nh: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = nh * nh * (a2 - 1.0) + 1.0;
    return a2 / (3.14159265 * d * d + 1e-7);
}

fn smith_g(nv: f32, nl: f32, a: f32) -> f32 {
    let k = a * 0.5;
    return (nv / (nv * (1.0 - k) + k)) * (nl / (nl * (1.0 - k) + k));
}

fn fog_over(colour: vec3<f32>, rel: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    if (g.fog_col.w < 0.5) {
        return colour;
    }
    let dist = length(rel);
    // (A fog that thins with height: base height and half height. Its density is
    // that of the air along the whole ray, the mean of 2^-(z - base)/half between
    // the eye and the surface, not the air at the surface: a street seen from a
    // rooftop was drawn through the thick fog at its own height, which made every
    // view from above milky.)
    var thin = 1.0;
    if (g.fog_misc.y > 1.0) {
        let a = max(g.eye.z - g.fog_misc.x, 0.0);
        let c = max(world.z - g.fog_misc.x, 0.0);
        let h = g.fog_misc.y;
        if (abs(c - a) < 1.0) {
            thin = exp2(-a / h);
        } else {
            thin = (exp2(-a / h) - exp2(-c / h)) * h / (0.693147 * (c - a));
        }
    }
    var f = g.fog.z * (1.0 - exp2(-max(dist - g.fog.x, 0.0) / g.fog.y * thin));
    // Toward the sun the fog takes its sun colour (inside `begin` degrees
    // of the sun fully, past `end` not at all).
    let c = clamp(dot(rel / max(dist, 1e-3), g.sun_dir.xyz), -1.0, 1.0);
    let angle = degrees(acos(c));
    let t = 1.0 - smoothstep(g.fog_misc.z, max(g.fog_misc.w, g.fog_misc.z + 0.01), angle);
    let fc = mix(g.fog_col.xyz, g.fog_sun.xyz, t);
    // (Toward the sun only `normal_scale` of the ordinary fog is left; the sun's colour takes the place.)
    f = f * mix(1.0, g.fog.w, t);
    return mix(colour, fc, clamp(f, 0.0, 1.0));
}

// The fog over the sky along `dir`: the same air thinning with height as
// `fog_over`, taken to infinity (toward the horizon the ray stays in the
// thick air and takes the fog's whole opacity; toward the zenith little).
// Without it a far vista building (Highrise's towers 100,000 units off, fogged
// to the fog's colour) stood as a tan square on the unfogged sky.
fn fog_sky(colour: vec3<f32>, dir: vec3<f32>) -> vec3<f32> {
    if (g.fog_col.w < 0.5 || g.fog_misc.y <= 1.0) {
        return colour;
    }
    let h = g.fog_misc.y;
    let a = max(g.eye.z - g.fog_misc.x, 0.0);
    let up = max(dir.z, 0.0);
    // Optical depth in half distances: 2^(-a/h) h / (ln 2 dz half).
    let tau = exp2(-a / h) * h / (0.693147 * max(up, 1e-4) * g.fog.y);
    var f = g.fog.z * (1.0 - exp2(-tau));
    let c = clamp(dot(dir, g.sun_dir.xyz), -1.0, 1.0);
    let t = 1.0 - smoothstep(g.fog_misc.z, max(g.fog_misc.w, g.fog_misc.z + 0.01), degrees(acos(c)));
    let fc = mix(g.fog_col.xyz, g.fog_sun.xyz, t);
    f = f * mix(1.0, g.fog.w, t);
    return mix(colour, fc, clamp(f, 0.0, 1.0));
}

fn sky_env(dir: vec3<f32>) -> vec3<f32> {
    if (g.sky_zenith.w > 0.5) {
        return textureSampleLevel(sky_cube, probe_samp, dir, 0.0).rgb;
    }
    let up = dir.z;
    var c = mix(g.sky_horizon.xyz, g.sky_zenith.xyz, pow(clamp(up, 0.0, 1.0), 0.45));
    if (up < 0.0) {
        c = g.sky_horizon.xyz * mix(1.0, 0.6, clamp(-up * 4.0, 0.0, 1.0));
    }
    return c;
}

fn sky(dir: vec3<f32>) -> vec3<f32> {
    let up = dir.z;
    var c = mix(g.sky_horizon.xyz, g.sky_zenith.xyz, pow(clamp(up, 0.0, 1.0), 0.45));
    if (up < 0.0) {
        c = g.sky_horizon.xyz * mix(1.0, 0.6, clamp(-up * 4.0, 0.0, 1.0));
    }
    let toward = max(dot(dir, g.sun_dir.xyz), 0.0);
    if (g.sky_zenith.w > 0.5) {
        // The map's own sky picture (the sun is in it).
        return textureSampleLevel(sky_cube, probe_samp, dir, 0.0).rgb;
    }
    if (g.misc.w < 0.5) {
        c += g.sun_col.xyz * (pow(toward, 24.0) * 0.12 + pow(toward, 2000.0) * 40.0);
    }
    return c;
}

