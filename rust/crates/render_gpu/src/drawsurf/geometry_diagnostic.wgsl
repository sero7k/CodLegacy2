#import bevy_render::view::View

@group(0) @binding(0) var<uniform> view: View;

// bo2mp: motion vectors for a temporal upscaler (FSR 3.1, DLSS), written to a
// second colour target by the opaque draws when the MOTION def is on (no
// other pipeline touches binding 1). Same convention as Bevy's motion vector
// prepass: the current minus the previous frame's unjittered position, as a
// share of the target (UV units, +x right, +y down). `flags.x` 1 draws no
// motion (the first-person gun and arms ride the camera).
#ifdef MOTION
struct MotionParams {
    prev_clip_from_world: mat4x4<f32>,
    flags: vec4<f32>,
}
@group(0) @binding(1) var<uniform> motion_params: MotionParams;

fn motion_from_clip(cur: vec4<f32>, prev: vec4<f32>) -> vec2<f32> {
    // (A point behind either camera has no sensible reprojection: still.)
    if (motion_params.flags.x > 0.5 || cur.w <= 0.0 || prev.w <= 0.0) {
        return vec2(0.0);
    }
    return (cur.xy / cur.w - prev.xy / prev.w) * vec2(0.5, -0.5);
}

// A static point: only the camera moved. (Players, grenades and effects have
// no previous transform in the extract, so they take this too.)
fn motion_world(world: vec3<f32>) -> vec2<f32> {
    return motion_from_clip(
        view.unjittered_clip_from_world * vec4(world, 1.0),
        motion_params.prev_clip_from_world * vec4(world, 1.0),
    );
}

// The sky: the camera's turn only (w = 0 drops both translations).
fn motion_sky(dir: vec3<f32>) -> vec2<f32> {
    return motion_from_clip(
        view.unjittered_clip_from_world * vec4(dir, 0.0),
        motion_params.prev_clip_from_world * vec4(dir, 0.0),
    );
}
#endif

// bo2zm: a world surface's colour map (only the textured world pipeline
// binds group 1).
@group(1) @binding(0) var colour_map: texture_2d<f32>;
@group(1) @binding(1) var colour_sampler: sampler;
// bo2zm: a layered surface's layer 1 and 2 colour maps.
@group(1) @binding(6) var colour_map1: texture_2d<f32>;
@group(1) @binding(7) var colour_map2: texture_2d<f32>;

// bo2zm: a T6 lightmap page and the world lighting. Lightmapped world
// surfaces bind all four; every other Black Ops II surface (unlit world
// surfaces, props) binds the lighting and lights alone (bindings 2, 3).
struct T6Lighting {
    // xyz: direction to the sun; w: exposure scale.
    sun_dir_exposure: vec4<f32>,
    // rgb: sun colour.
    sun_color: vec4<f32>,
    // The sky's rotation (x, y, z of skyBoxRotation) and brightness (w,
    // negative when z flips).
    sky: vec4<f32>,
    // bo2zm: the sky's skyColorParm x (above 0: the cube is rgb / a, else
    // rgb^2) and w (the share of the fog it takes). bo2mp: z is 1 on a
    // Black Ops III world lit by its own light (`t7_shade`).
    sky_parms: vec4<f32>,
    // bo2zm: the vision's colour grade (see bo2_final): tone band slopes and
    // offsets, the red, green and blue rows of the shadow, mid and highlight
    // matrices, then gamma, saturation (luma weights, amount) and blend.
    // Identity on a multiplayer map (its grade is bo2_bloom's final pass).
    grade: array<vec4<f32>, 14>,
    // bo2zm: the world's colour tables drawn (see world_lut): the first rows
    // of tables A and B, A's weight, and 1 when the map has them.
    lut: vec4<f32>,
}
// The map's primary lights, four vec4 each: origin and type (1 sun, 2 spot,
// 5 omni), colour and radius, direction and the cone's outer cosine, then
// dAttenuation and the inner cosine. Rows 256 and 257 (vec4 1024 on) are
// the fog's.
const T6_MAX_LIGHTS: u32 = 256u;
struct T6Lights {
    v: array<vec4<f32>, 1032>,
}
@group(2) @binding(0) var lightmap: texture_2d<f32>;
@group(2) @binding(1) var lightmap_sampler: sampler;
@group(2) @binding(2) var<uniform> t6: T6Lighting;
@group(2) @binding(3) var<uniform> t6_lights: T6Lights;
// bo2zm: the lights effects throw (muzzle flashes, explosions, impact
// flashes): a count, then per light its origin and radius, its colour.
struct T6DynLights {
    count: vec4<f32>,
    v: array<vec4<f32>, 32>,
}
@group(2) @binding(4) var<uniform> t6_dyn: T6DynLights;
// bo2mp: Black Ops II's sun shadow partitions (near, far), read with a
// compare sampler: each one's clip-from-world, then (on, near texel, far
// texel, depth range), sizes in world units.
struct SunShadow {
    near_from_world: mat4x4<f32>,
    far_from_world: mat4x4<f32>,
    info: vec4<f32>,
    // bo2mp: the shared look's middle cascade (`render_t7::bo2_shared`,
    // layer 2) and (x: three cascades, y: its texel in world units).
    mid_from_world: mat4x4<f32>,
    cascade: vec4<f32>,
}
@group(2) @binding(5) var sun_shadow_map: texture_depth_2d_array;
@group(2) @binding(6) var sun_shadow_sampler: sampler_comparison;
@group(2) @binding(7) var<uniform> sun_shadow: SunShadow;
// bo2zm: the world's colour tables (`GfxWorld::lutMaterial`; a 1x1 white
// texel without them).
@group(2) @binding(8) var t6_lut: texture_2d<f32>;
@group(2) @binding(9) var t6_lut_sampler: sampler;

// bo2zm: Black Ops II's lit shaders add such lights ("glights", the lmap
// pixel shaders' combined_glight path) as colour * saturate(1 - d /
// radius)^2 * N.L, diffuse only.
fn dyn_lights(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var sum = vec3(0.0);
    let count = min(u32(t6_dyn.count.x + 0.5), 16u);
    for (var i = 0u; i < count; i++) {
        let a = t6_dyn.v[i * 2u];
        let c = t6_dyn.v[i * 2u + 1u];
        let d = a.xyz - p;
        let dist = max(length(d), 0.0001);
        let fall = saturate(1.0 - dist / max(a.w, 1.0));
        sum += c.rgb * (fall * fall) * saturate(dot(d / dist, n));
    }
    return sum;
}

// bo2mp: the wind over the ground, a unit vector (the dynamic lights' uniform's
// z and w, set each frame; a fixed breeze when unset).
fn wind_dir() -> vec2<f32> {
    let w = t6_dyn.count.zw;
    return select(vec2(0.8192, 0.5736), normalize(w), dot(w, w) > 0.000001);
}

#ifdef ANIM_UV
// bo2mp: flowing water, whatever the game (`asset_core::T6Anim::Flow` and
// `Fall`: streams, rivers, waterfalls, scrolling techniques). The colour
// picture slides along the surface: down the slope when it is steep (the
// texture axis the world's "down" runs along, by the derivatives of the
// position and the texcoord) from 1.7 degrees of slope, else along v (a
// level pool has no downhill; a material's own scroll constant, the sign of
// `uvAnimParms`, has no bits in the draw code to ride in). The pace is a fixed number of
// texture repeats a second (an integer number an hour, so the hourly wrap of
// the game time is seamless) and the same over the whole surface, so the
// picture never shears however long it runs. Textures repeat, so the offset
// is kept in 0..1. Needs uniform control flow (derivatives): the fragment
// entry points call it first.
#ifdef ANIM_FALL
const ANIM_PACE: f32 = 0.75;
#else
const ANIM_PACE: f32 = 0.05;
#endif
fn anim_uv(uv: vec2<f32>, p: vec3<f32>, normal: vec3<f32>) -> vec2<f32> {
    let offset = fract(ANIM_PACE * t6_dyn.count.y);
    let n = normalize(normal);
    // The world's down, less its part along the normal.
    let g = vec3(0.0, 0.0, -1.0) + n * n.z;
    var axis = vec2(0.0, 1.0);
    // (Down the slope from about 1.7 degrees: a stream bed is gentle, and
    // along v alone it ran either way, up the stream as often as down.)
    if (dot(g, g) > 0.001) {
        let dpx = dpdx(p);
        let dpy = dpdy(p);
        let dux = dpdx(uv);
        let duy = dpdy(uv);
        let det = dux.x * duy.y - duy.x * dux.y;
        if (abs(det) > 0.00000000000001) {
            // The world step of one texcoord unit in u and in v.
            let pu = (dpx * duy.y - dpy * dux.y) / det;
            let pv = (dpy * dux.x - dpx * duy.x) / det;
            let au = dot(normalize(pu), g);
            let av = dot(normalize(pv), g);
            axis = select(vec2(0.0, sign(av)), vec2(sign(au), 0.0), abs(au) > abs(av));
        }
    }
    return uv - axis * offset;
}
#endif

#ifdef CLOTH
// bo2mp: cloth and foliage in the wind (`asset_core::T6Anim::Cloth`: a
// flag, a banner, a tarp, ivy, ferns; Black Ops II's `flag`, Ghosts' and
// Modern Warfare Remastered's `flag` and `flagext` techniques). A
// travelling wave along the wind, a faster flutter across it and a slow
// gust, pushing the vertex along its normal (the cloth flaps) and a little
// along the wind. How far a vertex moves grows with its distance from the
// model's origin (a flagpole, a plant's root, a tent's centre), to 2.5 units
// at 72 and beyond, times `weight`: the vertex colour's alpha, as Black Ops
// II's `mc_lit_sm_flag` vertex shader scales its wave (fxc disassembly: the
// sum of three sines times COLOR.w, along the normal). Where the artist
// painted it 0 the cloth is pinned: mp_downhill's table cloth keeps its top
// on the table instead of flapping down through it. The rates are whole
// cycles an hour: the game time wraps hourly without a jump.
const CLOTH_TAU: f32 = 6.2831853;
fn cloth_offset(local: vec3<f32>, world: vec3<f32>, normal: vec3<f32>, weight: f32) -> vec3<f32> {
    let t = t6_dyn.count.y;
    let wind = wind_dir();
    let reach = smoothstep(0.0, 1.0, length(local) / 72.0);
    let travel = dot(world.xy, wind) * 0.05;
    let gust = 0.65 + 0.35 * sin(t * CLOTH_TAU * 212.0 / 3600.0 + dot(world.xy, vec2(0.0021, 0.0034)));
    let flap = sin(travel - t * CLOTH_TAU * 1500.0 / 3600.0)
        + 0.5 * sin(travel * 2.3 - t * CLOTH_TAU * 2400.0 / 3600.0 + 1.7)
        + 0.25 * sin(world.z * 0.09 + world.x * 0.03 - t * CLOTH_TAU * 3300.0 / 3600.0);
    let amplitude = 2.5 * reach * gust * saturate(weight);
    let drift = 0.35 * amplitude * (0.5 + 0.5 * sin(travel - t * CLOTH_TAU * 1100.0 / 3600.0));
    return normalize(normal) * (flap * amplitude) + vec3(wind * drift, 0.0);
}
#endif

#ifdef SOFT
// bo2zm: the scene depth, for soft effect edges.
@group(3) @binding(0) var soft_depth: texture_depth_2d;

// BO2's zfeather pixel shaders: how far the scene behind this pixel lies
// past it, times `inv` (1 / the fade distance), clamped to 0..1.
fn soft_fade(clip: vec4<f32>, world: vec3<f32>, inv: f32) -> f32 {
    let scene_d = textureLoad(soft_depth, vec2<i32>(clip.xy), 0);
    let uv = (clip.xy - view.viewport.xy) / view.viewport.zw;
    let ndc = vec4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, scene_d, 1.0);
    let scene_view = view.view_from_clip * ndc;
    let scene_z = select(1e9, -scene_view.z / scene_view.w, scene_d > 0.0);
    let sprite_z = -(view.view_from_world * vec4(world, 1.0)).z;
    return saturate((scene_z - sprite_z) * inv);
}
#endif

#ifdef SHINE
// bo2zm: a shine surface's normal and specular maps, and the reflection
// probes (their cube array and, per probe, its lightingSH: three vec4).
@group(1) @binding(8) var normal_map: texture_2d<f32>;
@group(1) @binding(9) var specular_map: texture_2d<f32>;
// bo2mp: an emissive flow surface's colour remap (rows above) and constants
// (the last row); a 1x1 stand-in for every other shine surface.
@group(1) @binding(10) var aux_map: texture_2d<f32>;
#ifdef LAYER1_SPEC
// bo2mp: a one-blend-layer surface's layer 1 specular map (`specularMap1`).
@group(1) @binding(11) var specular_map1: texture_2d<f32>;
#endif
struct T6Probes {
    v: array<vec4<f32>, 96>,
}
@group(3) @binding(0) var probe_cubes: texture_cube_array<f32>;
@group(3) @binding(1) var probe_sampler: sampler;
@group(3) @binding(2) var<uniform> probes: T6Probes;
#endif

struct VertexIn {
    @builtin(instance_index) instance: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // bo2zm: xyz the tangent, w the binormal's sign.
    @location(2) tangent: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) uv: vec2<f32>,
    @location(5) lightmap_uv: vec2<f32>,
#ifdef LAYERS
    // bo2zm: layer 1 and 2 texcoords (a second buffer).
    @location(6) layer_uv: vec4<f32>,
#endif
}

struct SmodelVertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(6) world_from_local_0: vec4<f32>,
    @location(7) world_from_local_1: vec4<f32>,
    @location(8) world_from_local_2: vec4<f32>,
    @location(9) world_from_local_3: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) lightmap_uv: vec2<f32>,
    @location(4) world_position: vec3<f32>,
    // bo2zm: the surface's primary light (the world draw's instance index).
    @location(5) @interpolate(flat) light_index: u32,
    @location(6) layer_uv: vec4<f32>,
    // bo2zm: the tangent frame and reflection probe (shine).
    @location(7) tangent: vec3<f32>,
    @location(8) binormal: vec3<f32>,
    @location(9) @interpolate(flat) probe: u32,
}

@vertex
fn vertex(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_position = view.clip_from_world * vec4(in.position, 1.0);
    out.normal = in.normal;
    out.color = in.color;
    out.uv = in.uv;
    out.lightmap_uv = in.lightmap_uv;
    out.world_position = in.position;
    out.tangent = in.tangent.xyz;
    // As BO2's world vertex shaders: binormal = cross(normal, tangent)
    // times the sign of the vertex's binormal sign.
    out.binormal = cross(in.normal, in.tangent.xyz) * sign(in.tangent.w);
#ifdef SHINE
    // A shine surface's instance index: primary light, then its probe.
    out.light_index = in.instance & 63u;
    out.probe = in.instance >> 6u;
#else
    out.light_index = in.instance;
    out.probe = 0u;
#endif
#ifdef LAYERS
    out.layer_uv = in.layer_uv;
#else
    out.layer_uv = vec4(0.0);
#endif
    return out;
}

@vertex
fn vertex_smodel(in: SmodelVertexIn) -> VertexOut {
    let world_from_local = mat4x4<f32>(
        in.world_from_local_0,
        in.world_from_local_1,
        in.world_from_local_2,
        in.world_from_local_3,
    );
    var out: VertexOut;
    let world = world_from_local * vec4(in.position, 1.0);
    out.clip_position = view.clip_from_world * world;
    out.normal = (world_from_local * vec4(in.normal, 0.0)).xyz;
    out.color = in.color;
    out.uv = in.uv;
    out.lightmap_uv = vec2(0.0);
    out.world_position = world.xyz;
    out.light_index = 0u;
    out.layer_uv = vec4(0.0);
    out.tangent = vec3(1.0, 0.0, 0.0);
    out.binormal = vec3(0.0, 1.0, 0.0);
    out.probe = 0u;
    return out;
}

// The untextured fallback: a surface whose material the exact pass refused
// (most often an image that did not decode). Neutral grey with a little
// up-facing shade so the shape still reads; the load report names the
// material ("unresolved material images"). CODLEGACY_DIAG_CHECKER=1 restores the
// developer checker (normal colour x UV cells).
fn shade_fallback(in: VertexOut) -> vec4<f32> {
#ifdef DIAG_CHECKER
    let normal_colour = abs(normalize(in.normal)) * 0.72 + vec3(0.08, 0.02, 0.10);
    let cell = i32(floor(in.uv.x * 16.0) + floor(in.uv.y * 16.0)) & 1;
    let diagnostic_tint = select(vec3(0.72, 0.22, 0.78), vec3(0.22, 0.72, 0.78), cell == 0);
    return vec4(mix(normal_colour, diagnostic_tint, 0.28) * max(in.color.rgb, vec3(0.35)), 1.0);
#else
    let n = normalize(in.normal);
    let shade = 0.85 + 0.15 * n.z;
    return vec4(vec3(0.38) * shade, 1.0);
#endif
}

// bo2zm: the colour map's stored texel. The material images decode as sRGB;
// Black Ops II's shaders read the raw texel and square it themselves.
fn raw_texel(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3(0.0));
    let lo = x * 12.92;
    let hi = 1.055 * pow(x, vec3(1.0 / 2.4)) - 0.055;
    return select(hi, lo, x <= vec3(0.0031308));
}

// bo2zm: Black Ops II's final image, after its surface shaders: the game's
// resolve rolls highlights off above 0.75 (hdr_bloom_apply: 0.75 + 0.25 *
// (1 - 2^(-5.77 * (x - 0.75)))), here, so the 8-bit frame keeps them. The
// final pass (bo2_bloom) undoes it, adds bloom, rolls off again and, on a
// multiplayer map, looks the frame up in the map's colour table (its vision
// file and lut image). bo2zm: on a Zombies map the world's colour tables and
// the vision's grade (scripts switch visions) are applied here instead.
fn bo2_final(encoded: vec3<f32>) -> vec3<f32> {
#ifdef IW6_HDR
    // (Into Ghosts' HDR scene: the display value back to light in its units.)
    return max(encoded, vec3(0.0)) * max(encoded, vec3(0.0)) * iw6_effect_scale();
#else
    return bo2_finish(bo2_rolloff(encoded), t6, t6_lut, t6_lut_sampler);
#endif
}

// bo2mp: a Ghosts map drawn by its own renderer, a frame drawn in stages
// (`render_gpu::drawsurf::iw6_native`, `BO2MP_IW6_CHAR_GRADE`): these
// draws go into Ghosts' HDR scene before its fog, colorization, bloom, iris
// and final pass, so they write linear light in Ghosts' units (no fog, no
// tone, no grade of their own). Light row 251 (`IW6_ROW`; its v0.w, the
// carrier look's switch, stays 0 on a Ghosts map) holds the numbers the
// lane wrote (`assets::lane::iw6_map`): v1 the sun's diffuse light (rgb)
// and the factor from this pass's light units to Ghosts' (w), v2 the sun's
// specular light (rgb) and the factor a display value is taken back by (w).
const IW6_ROW: u32 = 251u;

fn iw6_light_scale() -> f32 {
    return t6_lights.v[IW6_ROW * 4u + 1u].w;
}

fn iw6_effect_scale() -> f32 {
    return t6_lights.v[IW6_ROW * 4u + 2u].w;
}

// bo2zm: the highlights' roll-off above 0.75 (gamma-encoded).
fn bo2_rolloff(encoded: vec3<f32>) -> vec3<f32> {
    let rolled = 0.75 + 0.25 * (1.0 - exp2(-5.77078 * (encoded - 0.75)));
    return max(select(encoded, rolled, encoded > vec3(0.75)), vec3(0.0));
}

// bo2zm: a rolled-off colour through the world's colour table and the
// vision's grade (both identity on a multiplayer map).
fn bo2_finish(rolled: vec3<f32>, l: T6Lighting, lut: texture_2d<f32>, s: sampler) -> vec3<f32> {
    return bo2_grade(world_lut(lut, s, rolled, l.lut), l);
}

// bo2zm: the world's colour table, looked up before the vision grade (the
// game bakes both into one table each frame): a strip of 32 slices of 32 x
// 32, red across a slice, green down it, blue picking the slice (blended
// with the next one), one table per 32 rows. Tables A and B blend by A's
// weight as the camera moves between colour table volumes.
fn world_lut(lut: texture_2d<f32>, s: sampler, x: vec3<f32>, pick: vec4<f32>) -> vec3<f32> {
    if pick.w < 0.5 {
        return x;
    }
    let size = vec2<f32>(textureDimensions(lut));
    let c = saturate(x) * 31.0;
    let slice = floor(c.z);
    let next = min(slice + 1.0, 31.0);
    let f = c.z - slice;
    let u0 = (slice * 32.0 + c.x + 0.5) / size.x;
    let u1 = (next * 32.0 + c.x + 0.5) / size.x;
    let va = (pick.x + c.y + 0.5) / size.y;
    let vb = (pick.y + c.y + 0.5) / size.y;
    let a = mix(
        textureSampleLevel(lut, s, vec2(u0, va), 0.0).rgb,
        textureSampleLevel(lut, s, vec2(u1, va), 0.0).rgb,
        f,
    );
    let b = mix(
        textureSampleLevel(lut, s, vec2(u0, vb), 0.0).rgb,
        textureSampleLevel(lut, s, vec2(u1, vb), 0.0).rgb,
        f,
    );
    return mix(b, a, pick.z);
}

// bo2zm: the vision's colour grade of a rolled-off, gamma-encoded colour.
fn bo2_grade(x: vec3<f32>, l: T6Lighting) -> vec3<f32> {
    let lin = x * x;
    let g = l.grade;
    let r = saturate(dot(x, vec3(0.212585, 0.715195, 0.07222)) * g[0] + g[1]);
    let s = vec3(r.x, r.z * r.w, r.y);
    let band = s * s * (3.0 - 2.0 * s);
    let w = band / max(band.x + band.y + band.z, 0.000001);
    let c = vec4(lin, 1.0);
    let red = saturate(dot(vec3(dot(g[2], c), dot(g[3], c), dot(g[4], c)), w));
    let green = saturate(dot(vec3(dot(g[5], c), dot(g[6], c), dot(g[7], c)), w));
    let blue = saturate(dot(vec3(dot(g[8], c), dot(g[9], c), dot(g[10], c)), w));
    let graded = exp2(log2(max(vec3(red, green, blue), vec3(0.0000001))) * g[11].xyz);
    let luma = dot(graded, g[12].xyz);
    let toned = vec3(luma) + g[12].w * (graded - vec3(luma));
    return sqrt(saturate(lin + g[13].xyz * (toned - lin)));
}

// bo2zm: what every lit and unlit Black Ops II shader writes: the colour
// times the exposure (`hdrControl0.x`), square-rooted; the colour target
// holds gamma-encoded values. Then the game's final image steps.
fn bo2_out(colour: vec3<f32>) -> vec3<f32> {
#ifdef IW6_HDR
    return max(colour, vec3(0.0)) * iw6_light_scale();
#else
    return bo2_final(sqrt(max(colour * t6.sun_dir_exposure.w, vec3(0.0))));
#endif
}

// bo2zm A7: what an additive draw adds: the final image of its colour less
// that of black. The game grades the frame once, after the blend; graded on
// its own, each additive quad added the grade's black (Nuketown's shadow
// matrix biases, about 3/8/16 of 255) over its whole square: pale-blue
// rectangles round the fires and the light shafts.
fn bo2_added(encoded: vec3<f32>) -> vec3<f32> {
    return max(bo2_final(encoded) - bo2_final(vec3(0.0)), vec3(0.0));
}

// bo2zm: an unlit surface's output (additive ones add no black, above).
fn unlit_out(colour: vec3<f32>) -> vec3<f32> {
#ifdef IW6_HDR
    // (Emissive: its displayed value, as the effects'.)
    return max(colour * t6.sun_dir_exposure.w, vec3(0.0)) * iw6_effect_scale();
#endif
#ifdef ADD
    return bo2_added(sqrt(max(colour * t6.sun_dir_exposure.w, vec3(0.0))));
#else
    return bo2_out(colour);
#endif
}

// bo2zm: the map's fog as the game's shaders apply it (vertex shaders of
// the lit, unlit and sky techniques). Density halves every half height
// above the base height and doubles every half height below it, up to 100x
// (BO2's fog constants: x carries ln(density / max density) = ln(0.01), max
// density 100x; Die Rise's cloud deck under the rooftops). Along the ray
// from the eye its mean is 100 (F(x1) - F(x0)) / (x1 - x0) with x = -ln2 *
// (z - base) / half height + ln(0.01) and F(x) = e^x below zero, x + 1
// above. Fog = 1 - min(1, 2^-((ln2 * mean * d - start) / half distance)):
// with height fog on, BO2 scales the distance term by ln2 and the shader's
// last step is a base-2 exp. A half height under 1 is no height
// fog (the world fog's rows already raise theirs to 1, as BO2 does). Its
// colour and opacity blend toward the
// sun fog by t = saturate(y * dot(sun fog dir, view dir) + x); the colour
// moves toward the fog colour (HDR, before the exposure) by fog * opacity.
// `a`..`e` are light-table slot 257 (four vec4) and slot 256's first vec4.
fn fog_f(x: f32) -> f32 {
    return select(x + 1.0, exp(x), x < 0.0);
}

// BO2's ln2 on the distance term, only with height fog on.
fn fog_ln2(half_height: f32) -> f32 {
    return select(1.0, 0.6931472, half_height >= 1.0);
}

// The mean fog density (in units of the base density) from the eye to `pz`.
fn fog_mean(eye_z: f32, pz: f32, base: f32, half_height: f32) -> f32 {
    let k = select(0.0, -0.6931472 / half_height, half_height >= 1.0);
    let x0 = k * (eye_z - base) - 4.6051702;
    let x1 = k * (pz - base) - 4.6051702;
    let dx = x1 - x0;
    let flat_mean = select(1.0, exp(x0), x0 < 0.0);
    return 100.0 * select((fog_f(x1) - fog_f(x0)) / dx, flat_mean, abs(dx) < 0.0001);
}

fn fog_apply(
    colour: vec3<f32>,
    p: vec3<f32>,
    a: vec4<f32>,
    c: vec4<f32>,
    s: vec4<f32>,
    e: vec4<f32>,
    q: vec4<f32>,
) -> vec3<f32> {
    if (a.w < 0.5) {
        return colour;
    }
    let f = fog_at(p, a, c, s, e, q);
    return mix(colour, f.tint, fog_amount(f.fog, f.opacity, a.w));
}

// bo2mp: how much of the fog colour a point takes. Black Ops II scales the
// fog by its opacity; a Ghosts map drawn by its engine's law (the fog row's
// w at 2, `BO2MP_IW6_FOG_LAW`, on by default) holds the transmittance at its floor
// instead: max(1 - max opacity, e^(-k (d - start))).
fn fog_amount(fog: f32, opacity: f32, mode: f32) -> f32 {
    return select(fog * opacity, min(fog, opacity), mode > 1.5);
}

// The fog toward `p`: its colour, amount and opacity.
struct FogAt {
    tint: vec3<f32>,
    fog: f32,
    opacity: f32,
}

fn fog_at(
    p: vec3<f32>,
    a: vec4<f32>,
    c: vec4<f32>,
    s: vec4<f32>,
    e: vec4<f32>,
    q: vec4<f32>,
) -> FogAt {
    let eye = view.world_position;
    let d = distance(p, eye);
    let mean = fog_mean(eye.z, p.z, c.w, e.w) * fog_ln2(e.w);
    let fog = 1.0 - min(exp2(-(mean * d - a.x) / max(a.y, 1.0)), 1.0);
    let t = saturate(q.y * dot(e.xyz, (p - eye) / max(d, 0.0001)) + q.x);
    return FogAt(mix(c.rgb, s.rgb, t), fog, mix(a.z, s.w, t));
}

// bo2zm: the fog as Black Ops II's effect vertex shaders hand it on: the
// fog colour times the exposure, square-rooted (display space), and how
// much of the effect's own colour stays, (1 - fog * opacity)^2.
fn effect_fog(p: vec3<f32>) -> vec4<f32> {
#ifdef IW6_HDR
    // (Ghosts' own fog pass fogs the scene.)
    return vec4(0.0, 0.0, 0.0, 1.0);
#endif
    let a = t6_lights.v[1028u];
    let c = t6_lights.v[1029u];
    let s = t6_lights.v[1030u];
    let e = t6_lights.v[1031u];
    let q = t6_lights.v[1024u];
    if (a.w < 0.5) {
        return vec4(0.0, 0.0, 0.0, 1.0);
    }
    let eye = view.world_position;
    let d = distance(p, eye);
    let mean = fog_mean(eye.z, p.z, c.w, e.w) * fog_ln2(e.w);
    let fog = 1.0 - min(exp2(-(mean * d - a.x) / max(a.y, 1.0)), 1.0);
    let t = saturate(q.y * dot(e.xyz, (p - eye) / max(d, 0.0001)) + q.x);
    let tint = mix(c.rgb, s.rgb, t);
    let opacity = mix(a.z, s.w, t);
    let keep = 1.0 - fog_amount(fog, opacity, a.w);
    return vec4(sqrt(saturate(tint * t6.sun_dir_exposure.w)), keep * keep);
}

fn bo2_fog(colour: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
#ifdef IW6_HDR
    return colour;
#endif
    return fog_apply(
        colour,
        p,
        t6_lights.v[1028u],
        t6_lights.v[1029u],
        t6_lights.v[1030u],
        t6_lights.v[1031u],
        t6_lights.v[1024u],
    );
}

// bo2zm: an unlit surface's fog. Additive ones fade toward black by the
// fog's keep factor (going toward the fog colour would add it over the
// whole surface: Tranzit's 4000-unit fire glow cards lit the town's skyline
// and the power station's window in bright rectangles); the rest go toward
// the fog colour.
fn unlit_fog(colour: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
#ifdef ADD
    return colour * sqrt(effect_fog(p).w);
#else
    return bo2_fog(colour, p);
#endif
}

// bo2zm: the direct light of primary light `index` at `p` with normal `n`,
// before its baked visibility. The sun uses the world's sun; spot and omni
// lights fall off with dAttenuation / d^2 (full strength inside) times a
// smoothstep to zero at their radius, and spots fade between their outer
// and inner cone cosines (the beam runs along -direction). Structure from
// the game's lmap shaders; the parameter mapping is inferred.
// bo2zm: a primary light at a point: the direction to it and its colour
// there (attenuated, before N.L).
struct LightRay {
    dir: vec3<f32>,
    colour: vec3<f32>,
}

// bo2mp: one sun partition at a point (its clip position): four compare
// taps half a texel apart, the point moved toward the sun by a texel and a
// half plus half a unit so a lit side does not shadow itself.
fn sun_shadow_partition(layer: i32, clip: vec4<f32>, texel: f32) -> f32 {
    let uv = clip.xy * vec2(0.5, -0.5) + vec2(0.5);
    let depth = clip.z - (texel * 1.5 + 0.5) / sun_shadow.info.w;
    let o = 0.5 / 2048.0;
    var lit = textureSampleCompareLevel(sun_shadow_map, sun_shadow_sampler, uv + vec2(-o, -o), layer, depth);
    lit += textureSampleCompareLevel(sun_shadow_map, sun_shadow_sampler, uv + vec2(o, -o), layer, depth);
    lit += textureSampleCompareLevel(sun_shadow_map, sun_shadow_sampler, uv + vec2(-o, o), layer, depth);
    lit += textureSampleCompareLevel(sun_shadow_map, sun_shadow_sampler, uv + vec2(o, o), layer, depth);
    return lit * 0.25;
}

// bo2mp: how much sun reaches `p` past the things that move (1 = all): the
// near partition where it covers `p`, else the far one, fading out at its
// edge; past both, all of it.
fn sun_shadow_at(p: vec3<f32>) -> f32 {
    if (sun_shadow.info.x < 0.5) {
        return 1.0;
    }
    let near = sun_shadow.near_from_world * vec4(p, 1.0);
    if (all(abs(near.xy) < vec2(0.96)) && near.z > 0.0 && near.z < 1.0) {
        return sun_shadow_partition(0, near, sun_shadow.info.y);
    }
    // bo2mp: the shared look's three cascades (Black Ops III's fit): the
    // middle one between the near and the far.
    if (sun_shadow.cascade.x > 0.5) {
        let mid = sun_shadow.mid_from_world * vec4(p, 1.0);
        if (all(abs(mid.xy) < vec2(0.96)) && mid.z > 0.0 && mid.z < 1.0) {
            return sun_shadow_partition(2, mid, sun_shadow.cascade.y);
        }
    }
    let far = sun_shadow.far_from_world * vec4(p, 1.0);
    let edge = max(abs(far.x), abs(far.y));
    if (edge >= 1.0 || far.z <= 0.0 || far.z >= 1.0) {
        return 1.0;
    }
    return mix(sun_shadow_partition(1, far, sun_shadow.info.z), 1.0, smoothstep(0.8, 1.0, edge));
}

fn primary_light_ray(index: u32, p: vec3<f32>) -> LightRay {
    return primary_light_ray_with(index, p, true);
}

// `shadowed` false leaves the live sun shadow out of a sun's colour (the sea
// applies it itself, at the game's own share: `wsun`).
fn primary_light_ray_with(index: u32, p: vec3<f32>, shadowed: bool) -> LightRay {
    var ray: LightRay;
    ray.dir = vec3(0.0, 0.0, 1.0);
    ray.colour = vec3(0.0);
    if (index >= T6_MAX_LIGHTS) {
        return ray;
    }
    let a = t6_lights.v[index * 4u];
    let b = t6_lights.v[index * 4u + 1u];
    let c = t6_lights.v[index * 4u + 2u];
    let d = t6_lights.v[index * 4u + 3u];
    let kind = u32(a.w + 0.5);
    if (kind == 1u) {
        ray.dir = t6.sun_dir_exposure.xyz;
        ray.colour = t6.sun_color.rgb;
        if (shadowed) {
            ray.colour *= sun_shadow_at(p);
        }
        return ray;
    }
    if (kind == 0u) {
        return ray;
    }
    let to_light = a.xyz - p;
    let d2 = max(dot(to_light, to_light), 1.0);
    let dist = sqrt(d2);
    let l = to_light / dist;
    var atten = saturate(d.x / d2);
    let fall = saturate(1.0 - dist / max(b.w, 1.0));
    atten *= fall * fall * (3.0 - 2.0 * fall);
    if (kind >= 2u && kind <= 4u) {
        atten *= smoothstep(c.w, max(d.y, c.w + 0.0001), dot(l, c.xyz));
    }
    ray.dir = l;
    ray.colour = b.rgb * atten;
    return ray;
}

// bo2mp: a model's light plus a primary light's `lit * N.L`. On a Call of
// Duty 4 / World at War map (the sun colour's alpha 3 or more) as their lit
// model shaders add them (`lp_sun_*`: 2 * grid + visibility * N.L * sun),
// in gamma space: `light` is the grid's (2 * colour)^2, the sun colour a
// gamma value. Black Ops II adds them linear.
fn add_light(light: vec3<f32>, lit: vec3<f32>, ndotl: f32) -> vec3<f32> {
    // (bo2mp: an Infinite Warfare map's, alpha 5, adds them linear.)
    if (t6.sun_color.a > 2.5 && t6.sun_color.a < 4.5) {
        let g = sqrt(max(light, vec3(0.0))) + lit * ndotl;
        return g * g;
    }
    return light + lit * ndotl;
}

fn primary_light(index: u32, p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let ray = primary_light_ray(index, p);
    return ray.colour * saturate(dot(n, ray.dir));
}

// bo2mp: Black Ops III's own light (`assets::lane::t7_light`), on a world
// that has it (`t7_on`). Its numbers sit in light rows 254 and 255: the
// exposure and the sun's brightness over the sun colour drawn; the baked
// sun map's axes (u, v: (axis, origin), toward the sun: (axis, 1 / texel))
// and its height packing (base, range, size). The sun map is the colour
// table's slot (`t6_lut`): per texel the height toward the sun of the
// nearest surface in 24 bits (rgb), alpha 0 where nothing is.
const T7_ROW: u32 = 254u;

// bo2mp: the flags a viewmodel's instance rows carry in sixteenths in the
// fraction of primary.w (`extract`): 1 glow, 2 first-person gun or arms
// (their maps are Black Ops II's or III's), 4 another game's gun.
fn gun_flags(w: f32) -> u32 {
    return u32(fract(w) * 16.0 + 0.5) & 7u;
}

fn t7_on() -> bool {
    return t6.sky_parms.z > 0.5;
}

fn t7_row(i: u32) -> vec4<f32> {
    return t6_lights.v[T7_ROW * 4u + i];
}

// bo2mp: Black Ops III's look on a Black Ops II map (`assets::lane::
// bo3_look`, `BO2MP_BO3_LOOK=1`): the same branch, but the surface keeps
// Black Ops II's own albedo (vertex tint, layers) and takes its ambient
// light from Black Ops II's lightmap (ambient and directional pages: the
// light without the sun), its probes as Black Ops II stores them (rgb / a)
// and its fog as Black Ops II's.
fn t7_bo2() -> bool {
    return t6_lights.v[(T7_ROW + 1u) * 4u + 1u].z > 0.5;
}

// bo2mp: the shared look on a Black Ops II map (`BO2MP_BO2_SHARED_LOOK=1`,
// `assets::lane::bo3_look::SHARED_FLAG`, light row 255 element 14): the
// sun's visibility is Black Ops II's own baked one times the live
// cascades, and none where Black Ops II's primary light is not the sun
// (`t7_shared_sun_vis`).
fn t7_shared() -> bool {
    return t7_bo2() && t6_lights.v[(T7_ROW + 1u) * 4u + 3u].z > 0.5;
}

// The sun term's visibility under the shared look: the surface's primary
// light is the sun (kind 1) or it gets no sun at all, as Black Ops II's
// own lit shaders (`primary_light_ray`); then that light's baked
// visibility (lightmap page 2 alpha, or the light grid's for a model)
// times the live shadow. Only ever a product of numbers in 0..1 on Black
// Ops II's own sun: it darkens, it never adds light the lightmap has.
fn t7_shared_sun_vis(index: u32, baked: f32, live: f32) -> f32 {
    if (index >= T6_MAX_LIGHTS || index >= RT_ROW / 4u) {
        return 0.0;
    }
    let kind = u32(t6_lights.v[index * 4u].w + 0.5);
    return select(0.0, saturate(baked) * saturate(live), kind == 1u);
}

// bo2mp: on such a map, lightmap pages in Call of Duty 4's form, linear
// (light row 255 element 9: Modern Warfare Remastered's maps,
// `assets::lane::h1_map`): each page the square root of its light over 4,
// the dominant light's tangent direction in the alphas.
fn t7_iw_pages() -> bool {
    return t6_lights.v[(T7_ROW + 1u) * 4u + 2u].y > 0.5;
}

// bo2mp: Modern Warfare Remastered's own lit world shader (`mwr_shade`)
// instead of Black Ops III's, on such pages (light row 1, third vector z).
fn t7_mwr() -> bool {
    return t6_lights.v[(T7_ROW + 1u) * 4u + 2u].z > 0.5;
}

// The variants Black Ops III's light does not draw on a Black Ops II map
// (their own shaders: water, lava flow, tiles, flags).
#ifdef WATER
const T7_SPECIAL: bool = true;
#else
#ifdef FLOW
const T7_SPECIAL: bool = true;
#else
#ifdef TILE
const T7_SPECIAL: bool = true;
#else
#ifdef TILE_BLEND
const T7_SPECIAL: bool = true;
#else
#ifdef FLAG
const T7_SPECIAL: bool = true;
#else
const T7_SPECIAL: bool = false;
#endif
#endif
#endif
#endif
#endif

fn t7_drawn() -> bool {
    return t7_on() && !(T7_SPECIAL && t7_bo2());
}

fn t7_sun_height(x: i32, y: i32, size: i32) -> f32 {
    if (x < 0 || y < 0 || x >= size || y >= size) {
        return -1e30;
    }
    let t = textureLoad(t6_lut, vec2<i32>(x, y), 0);
    if (t.a < 0.5) {
        return -1e30;
    }
    let q = round(t.r * 255.0) * 65536.0 + round(t.g * 255.0) * 256.0 + round(t.b * 255.0);
    let b = t6_lights.v[(T7_ROW + 1u) * 4u];
    return b.x + b.y * q / 16777215.0;
}

// How much of the baked sun reaches `p` (surface normal `n`): the sun map
// read 4 x 4 texels round it, bilinear weights (a 3-texel tent), the point
// moved off the surface by a texel and a half and up toward the sun by two.
fn t7_sun_visibility(p: vec3<f32>, n: vec3<f32>) -> f32 {
    let ru = t7_row(1u);
    let rv = t7_row(2u);
    let rd = t7_row(3u);
    let size = i32(t6_lights.v[(T7_ROW + 1u) * 4u].z + 0.5);
    if (size < 2) {
        return 1.0;
    }
    let texel = 1.0 / max(rd.w, 0.000001);
    let q = p + n * (texel * 1.5);
    let fx = (dot(q, ru.xyz) - ru.w) * rd.w - 0.5;
    let fy = (dot(q, rv.xyz) - rv.w) * rd.w - 0.5;
    let h = dot(q, rd.xyz) + texel * 2.0 + 1.0;
    let x0 = i32(floor(fx));
    let y0 = i32(floor(fy));
    let f = vec2(fx - floor(fx), fy - floor(fy));
    var lit = 0.0;
    for (var j = -1; j <= 2; j++) {
        let wy = select(select(1.0, f.y, j == 2), 1.0 - f.y, j == -1);
        for (var i = -1; i <= 2; i++) {
            let wx = select(select(1.0, f.x, i == 2), 1.0 - f.x, i == -1);
            lit += wx * wy * select(0.0, 1.0, h >= t7_sun_height(x0 + i, y0 + j, size));
        }
    }
    return lit / 9.0;
}

// A reveal decal's coverage (Black Ops III's `gbuffer_decal` programs):
// its reveal map's value against a band the vertex alpha x sets, from
// saturate((1 - x) - soft x^0.5) to saturate((1 - x) + soft (1 - x)^0.5)
// (ramp 0.5: every map's). The vertex alpha arrives packed with the soft
// edge (`t7_light::reveal_alpha`); any other alpha is the texel's own.
fn t7_reveal(texel_a: f32, vertex_a: f32) -> f32 {
    if (vertex_a > -1.5) {
        return texel_a;
    }
    let t = -vertex_a - 2.0;
    let q = floor(t * 0.5);
    let x = saturate(saturate((t - 2.0 * q) / 0.999) * 0.998 + 0.001);
    let soft = q / 64.0;
    let lo = saturate((1.0 - x) - soft * sqrt(x));
    let hi = saturate((1.0 - x) + soft * sqrt(1.0 - x));
    return saturate((texel_a - lo) / max(hi - lo, 0.0001));
}

// bo2mp: a Black Ops II blend layer's coverage. Plain (`bKcK`): its alpha
// times its weight. Alpha-revealed (`bKcK..vK`, packed is negative:
// `T6Layers::reveal_color`): the alpha is a height its weight reveals,
// saturate((alpha - lo) / (hi - lo)), lo = saturate((1 - w) - x w^y), hi =
// saturate((1 - w) + x (1 - w)^y), x and y the material's reveal parms
// (`lit_sm_*_b1c1n1v1_b2c2n2v2` pixel shader, fxc disassembly).
fn t6_layer_cover(alpha: f32, w: f32, packed: f32) -> f32 {
    if (packed > -0.5) {
        return alpha * w;
    }
    let u = floor((-packed - 1.0) * 0.5 + 0.5);
    let xq = floor(u / 1024.0);
    let x = xq / 256.0;
    let y = (u - xq * 1024.0) / 256.0;
    let lo = saturate((1.0 - w) - x * pow(max(w, 0.000001), y));
    let hi = saturate((1.0 - w) + x * pow(max(1.0 - w, 0.000001), y));
    return saturate((alpha - lo) / max(hi - lo, 0.0001));
}

// Black Ops III's tone curve (its final pass, `bloom_apply_lut`, fxc
// disassembly of core_patch's programs): the light times the exposure plus
// 0.00873, its log2 mapped to 0..1 (times 0.072703 plus 0.598206: 0.0033
// to 46), through a fifth-order polynomial: display values, which its
// colour table then grades. Written to the frame without Black Ops II's
// roll-off: the final pass's unroll and roll-off give it back as it is.
fn t7_tone(c: vec3<f32>, exposure: f32) -> vec3<f32> {
    let x = max(c * exposure, vec3(0.0)) + vec3(0.00873);
    let t = saturate(log2(x) * 0.072703 + 0.598206);
    let p = (((7.712947 * t - 19.311527) * t + 14.275167) * t - 2.490045) * t + 0.878083;
    return saturate(p * t - 0.06691);
}

// bo2mp: Modern Warfare Remastered's tone map on its maps (`r_tonemap`
// "Linear", read from the game; `tonemap_linear`): the exposed colour
// clipped at one, in this pipeline's encoding (the square root, as the
// grade table reads it).
fn mwr_tone(c: vec3<f32>, exposure: f32) -> vec3<f32> {
    return sqrt(saturate(c * exposure));
}

fn t7_out(c: vec3<f32>) -> vec3<f32> {
#ifdef IW6_HDR
    return max(c, vec3(0.0)) * iw6_light_scale();
#endif
    let toned = select(t7_tone(c, t7_exposure()), mwr_tone(c, t7_exposure()), t7_mwr());
    return bo2_finish(toned, t6, t6_lut, t6_lut_sampler);
}

// The tone curve's exposure: Black Ops III's map's (row 254 x); on a Black
// Ops II map (`t7_bo2`) that times Black Ops II's own, which moves between
// its exposure volumes.
fn t7_exposure() -> f32 {
    // (On a Modern Warfare Remastered map, times its automatic exposure's
    // factor: row 3's w, `asset_world::mwr_auto_exposure_factor`.)
    let auto_x = select(1.0, t6.sky_parms.w, t7_mwr() && t6.sky_parms.w > 0.0);
    return t7_row(0u).x * select(1.0, t6.sun_dir_exposure.w, t7_bo2()) * auto_x;
}

// bo2mp: Black Ops III's atmospheric fog (its forward `lit` and sky
// shaders' `CoreFogConstants` path, disassembled): along the ray from the
// eye, an optical length (distance past the start, times the density at
// the eye, times the height falloff's integral (1 - e^-h) / h), the
// transmittance 2^(length * total density) eased by the extinction
// strength, and the light the fog scatters in: a Rayleigh term (its phase
// blended in by the sun strength) and a Mie term (Schlick phase, faded in
// past the haze distance), times the in-scatter strength and (1 -
// transmittance). The settings come from the map's fog volume
// (`t7_light::Fog`); BO3 turns them into its constants in code we cannot
// read, so the mapping is ours. Added: the haze toward the sun is shadowed
// by the baked sun map along the ray (marched), which draws the sun's
// shafts through the trees (BO3's volumetric lighting).
// Light rows 252 and 253 (vec4 1008..1015): `t7_light::Fog::rows`.
const T7_FOG: u32 = 1008u;

struct T7Fog {
    f: array<vec4<f32>, 8>,
    // The sun map's rows (u, v, toward the sun, packing): `t7_sun_visibility`.
    m: array<vec4<f32>, 4>,
}

// (The world's light table; the sky reads its own copy, `fragment_sky`.)
fn t7_fog_world() -> T7Fog {
    var o: T7Fog;
    for (var i = 0u; i < 8u; i++) {
        o.f[i] = t6_lights.v[T7_FOG + i];
    }
    o.m[0] = t6_lights.v[T7_ROW * 4u + 1u];
    o.m[1] = t6_lights.v[T7_ROW * 4u + 2u];
    o.m[2] = t6_lights.v[T7_ROW * 4u + 3u];
    o.m[3] = t6_lights.v[(T7_ROW + 1u) * 4u];
    return o;
}

// Whether the sun reaches `q` past the baked sun map (one texel).
fn t7_lit_at(q: vec3<f32>, g: T7Fog, map: texture_2d<f32>) -> f32 {
    let size = i32(g.m[3].z + 0.5);
    if (size < 2) {
        return 1.0;
    }
    let x = i32(floor((dot(q, g.m[0].xyz) - g.m[0].w) * g.m[2].w));
    let y = i32(floor((dot(q, g.m[1].xyz) - g.m[1].w) * g.m[2].w));
    if (x < 0 || y < 0 || x >= size || y >= size) {
        return 1.0;
    }
    let t = textureLoad(map, vec2<i32>(x, y), 0);
    if (t.a < 0.5) {
        return 1.0;
    }
    let packed = round(t.r * 255.0) * 65536.0 + round(t.g * 255.0) * 256.0 + round(t.b * 255.0);
    let top = g.m[3].x + g.m[3].y * packed / 16777215.0;
    return select(0.0, 1.0, dot(q, g.m[2].xyz) + 8.0 >= top);
}

// The fog over `c` seen at `p` from `eye` (`sky`: a sky pixel, `p` the ray's
// end at the sky's distance). `jitter` (0..1) offsets the shaft march.
fn t7_fog_core(c: vec3<f32>, p: vec3<f32>, eye: vec3<f32>, sky: bool, jitter: f32, g: T7Fog, map: texture_2d<f32>) -> vec3<f32> {
    let f4 = g.f[4];
    let f7 = g.f[7];
    let atmos = f4.w > 0.5;
    if (!atmos && f7.z < 0.5) {
        return c;
    }
    let v = p - eye;
    let d = max(length(v), 0.001);
    let dir = v / d;
    // Height falloff: density 2^-((z - base) / half height), its mean along
    // the ray (sky rays: the sky's own height settings).
    let hs = select(g.f[5], g.f[6], sky);
    let start = hs.x;
    let k = hs.z * 0.6931472;
    let at_eye = min(exp2(-(eye.z - hs.y) * hs.z), 16.0);
    var optical = max(d - start, 0.0) * at_eye;
    let h = v.z * k;
    if (abs(h) > 0.0001 && hs.z > 0.0) {
        optical *= (1.0 - exp(-h)) / h;
    }
    let ext = f4.x;
    var tr = select(vec3(1.0), saturate(exp2(optical * g.f[2].xyz) * ext + vec3(1.0 - ext)), atmos);
    // bo2mp: Modern Warfare Remastered's sky falloff: the sky's fog fades
    // out from rows 5 w degrees above the horizon over row 6 w more
    // (`atmosFogSkyAngularFalloff*`).
    if (sky && g.f[6].w > 0.0) {
        let up = degrees(asin(clamp(dir.z, -1.0, 1.0)));
        tr = mix(vec3(1.0), tr, 1.0 - saturate((up - g.f[5].w) / g.f[6].w));
    }
    // Phases: Schlick's Mie toward the sun, faded in past the haze
    // distance; Rayleigh (normalised) blended in by the sun strength.
    let mu = dot(g.f[3].xyz, dir);
    let kk = g.f[3].w;
    let q = 1.0 - kk * mu;
    let haze = saturate((d - g.f[0].w) * g.f[1].w);
    let mie = (1.0 - kk * kk) / (12.566371 * q * q) * haze;
    let ms = saturate(mu);
    let ray = mix(1.0, 0.75 * (1.0 + ms * ms), f4.z);
    // The shafts: the share of the ray's first stretch the sun reaches,
    // on the Mie term (and, by the shaft density, an in-scatter of its own
    // nearer the eye, where the fog is thin).
    var lit = 1.0;
    var shaft = 0.0;
    if (f7.z > 0.5) {
        let reach = min(d, f7.y);
        let n = i32(f7.z);
        let step = reach / f32(n);
        var sum = 0.0;
        for (var i = 0; i < n; i++) {
            let s = (f32(i) + jitter) * step;
            sum += t7_lit_at(eye + dir * s, g, map);
        }
        let frac = sum / f32(n);
        // Past the march the ray counts as lit.
        lit = mix(1.0, frac, reach / d);
        shaft = frac * (1.0 - exp2(-reach * f7.x));
    }
    let ins = (g.f[0].xyz * ray + g.f[1].xyz * (mie * lit)) * f4.y * (vec3(1.0) - tr);
    let vol = g.f[1].xyz * mie * f7.w * shaft;
    return c * tr + ins + vol;
}

fn t7_fog(c: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
#ifdef IW6_HDR
    return c;
#endif
    let g = t7_fog_world();
    let j = fract(52.982918 * fract(dot(floor(p * 0.37), vec3(0.06711056, 0.00583715, 0.0137))));
    return t7_fog_core(c, p, view.world_position, false, j, g, t6_lut);
}

// A surface lit as Black Ops III's forward `lit` shader does, reduced:
// albedo (linear) times the ambient light (the vertex's or the model's,
// baked from the probes) and the sun (Lambert, its baked visibility and
// the moving things' shadows); on a surface with its normal and specular
// maps (SHINE) the ambient bent by the normal map (the probe's smallest
// level), the sun's GGX highlight, and the probe's reflection at a level
// by gloss (5 (1 - gloss)), weighted by the split-sum fit and darkened
// where the ambient is. Gloss: Black Ops II's specular alpha back to Black
// Ops III's (power 2^(17 g)).
#ifdef SHINE
// A probe's light toward `d` (Black Ops II's: rgb / a).
fn t7_probe(d: vec3<f32>, pi: u32, mip: f32) -> vec3<f32> {
    // (A probe index from another game's picker may name a layer this map's
    // array does not have: clamped, never read past the array.)
    let last = i32(textureNumLayers(probe_cubes)) - 1;
    let t = textureSampleLevel(probe_cubes, probe_sampler, d, min(i32(pi), last), mip);
    return select(t.rgb, t.rgb / (t.a + 0.000001), t7_bo2());
}
#endif

// bo2mp: Modern Warfare Remastered's lit world shader (`lmo_sm_lmpb_l0d_
// l0sd_r0c0n0s0_nofog`, decompiled; docs/mwr_engine/world_lit.md): the
// baked light A + B * N.dir (its direction in the tangent frame, the normal
// map's normal), the sun by N.L times its shadow; the sun's highlight GGX
// with the gloss as alpha^2 = 2 / (2^(16 g) + 2), the visibility term and
// Schlick's fresnel; the probe re-lit by the baked light toward the
// reflection, (A + B * R.dir) * (2 - R.N), times the environment's split
// sum; the diffuse times (1 - specular colour). Fog, exposure and the tone
// curve follow (the game's post passes).
fn mwr_shade(base: vec3<f32>, a: vec3<f32>, b: vec3<f32>, dirw: vec3<f32>, vn: vec3<f32>, n: vec3<f32>, st: vec4<f32>, probe: u32, p: vec3<f32>, baked_vis: f32) -> vec3<f32> {
    let sun_dir = t6.sun_dir_exposure.xyz;
    let sun = t6.sun_color.rgb * t7_row(0u).y;
    let ndl = saturate(dot(n, sun_dir));
    // The sun's shadow as the game's: its baked visibility (the lightmap's
    // primary page) under the live shadow map near the eye.
    var vis = 0.0;
    if (dot(vn, sun_dir) > -0.05) {
        // (Light row 1's fourth vector's x set: the sun height map's shadows
        // of the placed models over it too, `BO2MP_H1_HEIGHT_SHADOW`.)
        let hm = select(1.0, t7_sun_visibility(p, vn), t6_lights.v[(T7_ROW + 1u) * 4u + 3u].x > 0.5);
        vis = baked_vis * hm * sun_shadow_at(p);
    }
    let irr = a + b * saturate(dot(n, dirw));
    var f0 = vec3(0.0);
    var spec = vec3(0.0);
#ifdef SHINE
    // (Specular pictures are linear here: the game squares them as stored.)
    f0 = st.rgb * st.rgb;
    let gl = st.a;
    let a2 = 2.0 / (exp2(16.0 * gl) + 2.0);
    let v = normalize(view.world_position - p);
    let ndv = saturate(dot(n, v));
    let h = normalize(sun_dir + v);
    let ndh = saturate(dot(n, h));
    let vdh = abs(dot(v, h));
    let dd = ndh * ndh * (a2 - 1.0) + 1.0;
    let d = a2 / max(dd * dd, 1e-6);
    let g = 1.0 / max((ndl + sqrt(ndl * ndl * (1.0 - a2) + a2)) * (ndv + sqrt(ndv * ndv * (1.0 - a2) + a2)), 1e-4);
    let fq = 1.0 - vdh;
    let fr = f0 + (vec3(1.0) - f0) * (fq * fq * fq * fq * fq);
    spec = sun * fr * (vis * ndl * d * g * 1.0390625);
    let r = reflect(-v, n);
    let relight = (a + b * saturate(dot(r, dirw))) * (2.0 - dot(r, vn));
    let pi = min(probe, 31u);
    let env = t7_probe(r, pi, 6.0 - 6.0 * gl) * probes.v[pi * 3u].x;
    let rough = sqrt(sqrt(a2));
    let rr = rough * vec4(-1.0, -0.0275, -0.572, 0.022) + vec4(1.0, 0.0425, 1.04, -0.04);
    let a004 = min(rr.x * rr.x, exp2(-9.28 * ndv)) * rr.x + rr.y;
    let ab = vec2(-1.04, 1.04) * a004 + rr.zw;
    spec += relight * env * (f0 * ab.x + ab.y);
#endif
    let test = u32(t6_lights.v[(T7_ROW + 1u) * 4u + 1u].x + 0.5);
    if (test == 1u) {
        return vec3(vis) / max(t7_exposure(), 0.0001);
    } else if (test == 2u) {
        return irr;
    } else if (test == 3u) {
        return base / max(t7_exposure(), 0.0001);
    }
    return base * (vec3(1.0) - f0) * (irr + sun * (vis * ndl)) + spec;
}

// bo2mp: Carrier's dusk look (`assets::lane::carrier_look`): its numbers sit
// in light row 251 (vec4s 1004..1007): v0 (wetness, puddle scale, puddle
// threshold, 1 = on), v1 (ambient tint rgb, ambient scale), v2 (sky grade
// rgb, amount), v3 (sun glow rgb, glow sharpness). Off (v0.w 0) on every
// other map, where none of it runs.
fn look_row(i: u32) -> vec4<f32> {
    return t6_lights.v[1004u + i];
}

fn look_on() -> bool {
    return t6_lights.v[1004u].w > 0.5;
}

fn look_hash(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2(0.1031, 0.1030));
    q += dot(q, q.yx + 33.33);
    return fract((q.x + q.y) * q.x);
}

// Value noise, 0..1.
fn look_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(look_hash(i), look_hash(i + vec2(1.0, 0.0)), u.x),
        mix(look_hash(i + vec2(0.0, 1.0)), look_hash(i + vec2(1.0, 1.0)), u.x),
        u.y,
    );
}

fn look_fbm(p: vec2<f32>) -> f32 {
    return look_noise(p) * 0.6 + look_noise(p * 2.13 + vec2(17.3, 5.1)) * 0.4;
}

// bo2mp: the real-time ambient (`assets::lane::rt_ambient`), light row 250
// (vec4s 1000..1003): v0 the first probe's centre and the cell; v1 the grid's
// dims and (w) the share against the lightmap's ambient (0: no grid); v2 the
// picture's first row, tiles per row, scale and width; v3 x: local lights on,
// y: effect lights on. The probes are ambient cubes in the sun map's picture
// below the sun map: a tile of dims x by dims y per (z slice, face).
const RT_ROW: u32 = 1000u;

fn rt_on() -> bool {
    return t6_lights.v[RT_ROW + 1u].w > 0.0;
}

// One face of one z slice at the cell coordinates `xy` (probe centres at +0.5).
fn rt_face(face: u32, slice: u32, xy: vec2<f32>, dims: vec3<f32>, r2: vec4<f32>) -> vec3<f32> {
    let t = slice * 6u + face;
    let per_row = u32(r2.y + 0.5);
    let tile = vec2(f32(t % per_row) * dims.x, f32(t / per_row) * dims.y);
    let size = vec2<f32>(textureDimensions(t6_lut));
    let uv = (vec2(0.0, r2.x) + tile + xy) / size;
    let c = textureSampleLevel(t6_lut, t6_lut_sampler, uv, 0.0).rgb;
    return c * c * r2.z;
}

// The ambient light at `p` for a surface facing `n`: the probes' cubes,
// trilinear, the three faces `n` points at weighted by n squared. The second
// element is how far inside the grid `p` is (1 inside, fading to 0 two cells
// out).
fn rt_ambient(p: vec3<f32>, n: vec3<f32>) -> vec4<f32> {
    let r0 = t6_lights.v[RT_ROW];
    let r1 = t6_lights.v[RT_ROW + 1u];
    let r2 = t6_lights.v[RT_ROW + 2u];
    let dims = r1.xyz;
    let g = (p - r0.xyz) / r0.w;
    let edge = max(max(max(-g.x, g.x - (dims.x - 1.0)), max(-g.y, g.y - (dims.y - 1.0))), max(-g.z, g.z - (dims.z - 1.0)));
    let gc = clamp(g, vec3(0.0), dims - 1.0);
    let z0 = floor(gc.z);
    let fz = gc.z - z0;
    let s0 = u32(z0);
    let s1 = u32(min(z0 + 1.0, dims.z - 1.0));
    let xy = gc.xy + 0.5;
    let nn = n * n;
    let fx = select(0u, 1u, n.x < 0.0);
    let fy = select(2u, 3u, n.y < 0.0);
    let fzf = select(4u, 5u, n.z < 0.0);
    var c = vec3(0.0);
    c += nn.x * mix(rt_face(fx, s0, xy, dims, r2), rt_face(fx, s1, xy, dims, r2), fz);
    c += nn.y * mix(rt_face(fy, s0, xy, dims, r2), rt_face(fy, s1, xy, dims, r2), fz);
    c += nn.z * mix(rt_face(fzf, s0, xy, dims, r2), rt_face(fzf, s1, xy, dims, r2), fz);
    return vec4(c, 1.0 - saturate(edge / 2.0));
}

// bo2mp: a surface's primary light that is not the sun (the sun is drawn
// live): a spot or omni light's `lit * N.L`, `vis` its baked visibility.
// Rows 250 and up are not lights.
fn t7_local_light(index: u32, vis: f32, p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    if (t6_lights.v[RT_ROW + 3u].x < 0.5 || index >= RT_ROW / 4u) {
        return vec3(0.0);
    }
    let kind = u32(t6_lights.v[index * 4u].w + 0.5);
    if (kind < 2u) {
        return vec3(0.0);
    }
    let ray = primary_light_ray(index, p);
    return ray.colour * (vis * saturate(dot(n, ray.dir)));
}

fn t7_shade(base_in: vec3<f32>, amb_in: vec3<f32>, vn: vec3<f32>, n_in: vec3<f32>, st_in: vec4<f32>, probe: u32, p: vec3<f32>, pv: vec3<f32>, diffuse_share: bool, bo2_maps: bool, light_index: u32, light_vis: f32) -> vec3<f32> {
    var base = base_in;
    // bo2mp: on a Modern Warfare III map (light row 255 element 13,
    // `assets::lane::jup_map`) albedo past 0.5 rolls off at a quarter: MW3's
    // pictures are tint bases and layers the converted world draws one of,
    // and their near whites drew as bright as paper (as `render_jup`'s).
    if (t6_lights.v[(T7_ROW + 1u) * 4u + 3u].y > 0.5) {
        base = min(base, vec3(0.5) + max(base - vec3(0.5), vec3(0.0)) * 0.25);
    }
    var n = n_in;
    var st = st_in;
    // Carrier's wet deck: up-facing surfaces darken where wet and, in
    // puddles, go flat and near-mirror (the sun's streak and the probe's
    // reflection then show); a glossy surface gets a wet film's gloss.
    var wet = 0.0;
    var puddle = 0.0;
    var look_amb = vec3(1.0);
    if (look_on()) {
        let l0 = look_row(0u);
        let l1 = look_row(1u);
        look_amb = l1.rgb * l1.w;
        wet = saturate((vn.z - 0.6) / 0.35) * l0.x;
        puddle = saturate(wet) * smoothstep(l0.z, l0.z + 0.08, look_fbm(p.xy * l0.y));
        base = base * mix(1.0, 0.6, saturate(wet)) * mix(1.0, 0.75, puddle);
        n = normalize(mix(n, vn, puddle * 0.9));
#ifdef SHINE
        // f0 0.03 (water) is a texel of 0.0253 here (f0 = raw_texel^2).
        st = vec4(
            mix(max(st.rgb, vec3(0.0253 * 0.5 * saturate(wet))), vec3(0.0253), puddle),
            mix(max(st.a, mix(st.a, 0.8, saturate(wet))), 1.0, puddle),
        );
#endif
    }
    let sun_dir = t6.sun_dir_exposure.xyz;
    let sun = t6.sun_color.rgb * t7_row(0u).y;
    let ndl = saturate(dot(n, sun_dir));
    var vis = 0.0;
    if (dot(vn, sun_dir) > -0.05) {
        // (On a Modern Warfare Remastered map the live shadow map alone,
        // as its probe-lit models are drawn: the sun height map's blur
        // shadowed whole props.)
        vis = select(t7_sun_visibility(pv, vn), 1.0, t7_mwr() && t6_lights.v[(T7_ROW + 1u) * 4u + 3u].x < 0.5) * sun_shadow_at(pv);
        if (t7_shared()) {
            vis = t7_shared_sun_vis(light_index, light_vis, sun_shadow_at(pv));
        }
    }
    // The sky light's gain (light row 0's z; 0 or less: 1): the ambient
    // light and the probes' reflections against the sun
    // (`BO2MP_T7_SKY_GAIN`).
    let sky_gain = select(1.0, t7_row(0u).z, t7_row(0u).z > 0.0);
    var amb = amb_in * sky_gain * look_amb;
    // The rebake (`rt_ambient`) in place of the lightmap's ambient by its
    // share, where the surface is inside its grid.
    var amb_so = amb_in;
    if (t7_bo2() && rt_on()) {
        let rt = rt_ambient(p, n);
        let k = t6_lights.v[RT_ROW + 1u].w * rt.w;
        amb = mix(amb, rt.rgb * sky_gain, k);
        amb_so = mix(amb_in, rt.rgb, k);
    }
    var colour = vec3(0.0);
    // The diffuse's share (Modern Warfare Remastered's lit shaders: the
    // diffuse times 1 - the specular colour, so a metal or a glossy paint
    // is its reflection, not its colour and its reflection both).
    var kd = vec3(1.0);
#ifdef SHINE
    let lum = vec3(0.2126, 0.7152, 0.0722);
    // bo2mp: a guest gun with no probe near it (probe 0xffff, set by the
    // viewmodel's call) takes no reflection: it took probe 0's, a stand-in
    // or another game's, which on a map without Black Ops II's probes drew
    // BO3 and Workshop guns as a flat wash of colour.
    let no_probe = probe >= 0xffffu;
    let pi = select(min(probe, 31u), 0u, no_probe);
    let gain = select(select(probes.v[pi * 3u].x, 1.0, t7_bo2()), 0.0, no_probe);
    let l6v = t7_probe(vn, pi, 6.0);
    let l6n = t7_probe(n, pi, 6.0);
    amb *= clamp(dot(l6n, lum) / max(dot(l6v, lum), 0.0001), 0.5, 2.0);
    let sraw = raw_texel(st.rgb);
    // (On a Modern Warfare Remastered map, its own reading: the specular
    // squared as stored, alpha^2 = 2 / (2^(16 gloss) + 2).)
    // (bo2_maps: a first-person gun's or arms' maps are Black Ops II's or
    // III's encoding on any map; Modern Warfare Remastered's reading of the
    // specular colour and gloss is for its own models. Read that way, the
    // sun's lobe on a BO3 gun's tight gloss was a wash of the sun's colour.)
    let mwr = t7_mwr() && !bo2_maps;
    let f0 = select(sraw * sraw, st.rgb * st.rgb, mwr);
    kd = select(vec3(1.0), vec3(1.0) - f0, mwr && diffuse_share);
    let g = log2(max(exp2(13.0 * st.a) - 2.0, 1.0)) / 17.0;
    let a2 = clamp(select(2.0 / (exp2(17.0 * g) + 2.0), 2.0 / (exp2(16.0 * st.a) + 2.0), mwr), 0.0005, 1.0);
    let v = normalize(view.world_position - p);
    let ndv = max(dot(n, v), 0.0001);
    let h = normalize(sun_dir + v);
    let ndh = saturate(dot(n, h));
    let ldh = saturate(dot(sun_dir, h));
    let dd = ndh * ndh * (a2 - 1.0) + 1.0;
    let d = a2 / (3.14159265 * dd * dd);
    let fq = 1.0 - ldh;
    let fr = f0 + (1.0 - f0) * (fq * fq * fq * fq * fq);
    colour += sun * (vis * ndl * d / max(4.0 * ldh * ldh, 0.05)) * fr;
    var r = reflect(-v, n);
    // The reflection corrected against the probe's parallax box (Black
    // Ops III's lit shader against six planes round it; here its box, in
    // its axes turned about z): where the ray leaves the box, seen from
    // where the probe was taken (`t7_light::Lit::probe_rows`).
    let pb = probes.v[pi * 3u + 2u];
    if (!t7_bo2() && pb.x > 0.0) {
        let p0 = probes.v[pi * 3u];
        let p1 = probes.v[pi * 3u + 1u];
        let cs = vec2(p1.w, pb.w);
        let d0 = p - p1.xyz;
        let lp = vec3(cs.x * d0.x + cs.y * d0.y, -cs.y * d0.x + cs.x * d0.y, d0.z);
        let lr = vec3(cs.x * r.x + cs.y * r.y, -cs.y * r.x + cs.x * r.y, r.z);
        let inv = 1.0 / select(lr, vec3(1e-6), abs(lr) < vec3(1e-6));
        let far = max((pb.xyz - lp) * inv, (-pb.xyz - lp) * inv);
        let t = min(min(far.x, far.y), far.z);
        if (t > 0.0 && all(abs(lp) <= pb.xyz * 1.5)) {
            r = normalize(p + r * t - p0.yzw);
        }
    }
    let mip = clamp(5.0 * (1.0 - g) - 0.35, 0.0, 6.0);
    let env = t7_probe(r, pi, mip) * gain;
    let rough = sqrt(sqrt(a2));
    let rr = rough * vec4(-1.0, -0.0275, -0.572, 0.022) + vec4(1.0, 0.0425, 1.04, -0.04);
    let a004 = min(rr.x * rr.x, exp2(-9.28 * ndv)) * rr.x + rr.y;
    let ab = vec2(-1.04, 1.04) * a004 + rr.zw;
    let so = saturate(dot(amb_so, lum) / max(dot(l6v, lum) * gain, 0.0001));
    // (The reflections' own gain, light row 1's third vector's x
    // (`BO2MP_T7_REFLECT`); 0 or less: the sky light's. Nuk3town's dimmed
    // sky light left its metals, its white hover bus, black.)
    let rg = t6_lights.v[(T7_ROW + 1u) * 4u + 2u].x;
    colour += env * look_amb * (f0 * ab.x + ab.y) * so * select(sky_gain, rg, rg > 0.0);
#endif
    colour += base * kd * (amb + sun * (vis * ndl));
    // The map's local lights and the effects' (muzzle flashes, explosions),
    // diffuse, on a Black Ops II map (`t7_bo2`; light row 250's v3).
    if (t7_bo2()) {
        let local = t7_local_light(light_index, light_vis, p, n)
            + select(vec3(0.0), dyn_lights(p, n), t6_lights.v[RT_ROW + 3u].y > 0.5);
        colour += base * kd * local;
    }
    // A test view (`BO2MP_T7_VIEW`).
    let test = u32(t6_lights.v[(T7_ROW + 1u) * 4u + 1u].x + 0.5);
    if (test == 1u) {
        return vec3(vis) / max(t7_exposure(), 0.0001);
    } else if (test == 2u) {
        return amb;
    } else if (test == 3u) {
        return base / max(t7_exposure(), 0.0001);
    } else if (test == 4u) {
        return amb + sun * (vis * ndl);
    }
    return colour;
}

#ifdef IW6_HDR
const IW6_HDR_ON: bool = true;
#else
const IW6_HDR_ON: bool = false;
#endif

// bo2mp: a lit model drawn into Ghosts' HDR scene (`IW6_HDR`), lit as
// Ghosts' own model shaders light theirs (`docs/exe_notes/IW6_SHADERS.md`
// 5 and 7, the `lpo_sm_l0d_l0sd` programs): albedo times the light grid's
// light plus the sun by N.L times its visibility; on a surface with its
// specular map the squared specular colour times the fresnel ramp times
// the probe's reflection plus the sun's Phong lobe on the reflection
// vector, `min(1, e^(n (R.L - 0.99925)))` with `n = 2^(9.377518 gloss) + 7`.
// `amb` and `refl` are in this pass's light units (`iw6_light_scale` takes
// them to Ghosts'); the sun's light is Ghosts' own (light row 251). The
// grid's light here is its mean over the 56 directions (the moving things'
// grid holds no direction). INFERRED: the fresnel ramp (Ghosts' comes from
// each material's `envMapParms`, which Black Ops II's materials do not
// have): (0.2, 1.0, 2.5), a common one among mp_frag's materials.
fn iw6_lit(base: vec3<f32>, amb: vec3<f32>, vn: vec3<f32>, n: vec3<f32>, st: vec4<f32>, refl: vec3<f32>, p: vec3<f32>, pv: vec3<f32>, shine: bool) -> vec3<f32> {
    let l = t6.sun_dir_exposure.xyz;
    let sun_d = t6_lights.v[IW6_ROW * 4u + 1u].rgb;
    let sun_s = t6_lights.v[IW6_ROW * 4u + 2u].rgb;
    var vis = 0.0;
    if (dot(vn, l) > -0.05) {
        vis = select(1.0, t7_sun_visibility(pv, vn), t7_on()) * sun_shadow_at(pv);
    }
    let k = iw6_light_scale();
    var colour = base * (amb * k + sun_d * (vis * saturate(dot(n, l))));
    if (shine) {
        let sr = raw_texel(st.rgb);
        let spec = sr * sr;
        let v = normalize(p - view.world_position);
        let ndv = dot(v, n);
        let fres = mix(0.2, 1.0, pow(max(1.0 - abs(ndv), 0.0), 2.5));
        let r = v - 2.0 * ndv * n;
        let e = exp2(9.377518 * st.a) + 7.0;
        let hl = min(1.0, exp(e * (dot(r, l) - 0.99925)));
        colour += spec * fres * (refl * k + sun_s * (vis * hl));
    }
    return colour;
}

// bo2zm: world surfaces without a lightmap. Unlit ones as the game's unlit
// shaders draw them: (texel * vertex colour)^2 times the material's colour
// scale, then the BO2 output. A multiply decal scales what is behind by
// 1 + a * (texel * vertex colour - 1). Lit ones without a lightmap get a
// plain sun-and-sky light. Alpha feeds the blended variants.
fn shade_textured(in: VertexOut) -> vec4<f32> {
    let albedo = textureSample(colour_map, colour_sampler, in.uv);
#ifdef ALPHA_TEST
    // The game's alpha-tested shaders discard below 128/255.
    if (albedo.a < 0.501961) {
        discard;
    }
#endif
    let texel = raw_texel(albedo.rgb);
#ifdef UNLIT
    // (bo2mp: on a world lit by Black Ops III's own light the vertex
    // colour is its baked light, not a tint.)
    let tint = select(in.color.rgb, vec3(1.0), t7_on() && !t7_bo2());
#ifdef MULTIPLY
    let a = albedo.a * in.color.a;
    return vec4(mix(vec3(1.0), texel * tint, a), a);
#else
    let c = texel * tint;
    return vec4(
        unlit_out(unlit_fog(c * c * f32(#{UNLIT_SCALE}), in.world_position)),
        albedo.a * in.color.a,
    );
#endif
#else
    let n = normalize(in.normal);
    let sun = normalize(vec3(0.35, 0.25, 0.9));
    let light = 0.55 + 0.45 * max(dot(n, sun), 0.0);
    return vec4(texel * light, albedo.a);
#endif
}

#ifdef FLOW
// bo2mp: Black Ops II's emissive flow (`cod7emissiveflow`, the molten
// lava; its lit technique, fxc disassembly). The specular map holds its
// emissive masks and noise (hi.a, lo.a, noise.g, noise.a, packed by the
// assets lane), the auxiliary map its colour remap and, in the last
// row, its constants: diffuse uv control (scale, scroll), emissive uv
// scale and scroll, noise uv control, normal scale and bias, remap uv,
// diffuse filter (w: noise weight), emissive filter. The heat is the
// masks' product plus noise times the vertex blue; the remap's row is a
// smoothstep of noise plus the vertex red. The rock: the sun alone
// (Burley diffuse, a GGX lobe of alpha^2 0.2401, Schlick from 0.15), no
// lightmap; the glow: remap^2 times the emissive filter, plus remap^2
// times the diffuse filter by n.v. The normal map fades where the remap
// alpha does. No fog (the shader has none).
fn bo2_flow(
    uv: vec2<f32>,
    vcolour: vec4<f32>,
    vn: vec3<f32>,
    tangent: vec3<f32>,
    binormal: vec3<f32>,
    p: vec3<f32>,
) -> vec3<f32> {
    let crow = i32(textureDimensions(aux_map).y) - 1;
    let fc0 = textureLoad(aux_map, vec2<i32>(0, crow), 0);
    let fc1 = textureLoad(aux_map, vec2<i32>(1, crow), 0);
    let fc2 = textureLoad(aux_map, vec2<i32>(2, crow), 0);
    let fc3 = textureLoad(aux_map, vec2<i32>(3, crow), 0);
    let fc4 = textureLoad(aux_map, vec2<i32>(4, crow), 0);
    let fc5 = textureLoad(aux_map, vec2<i32>(5, crow), 0);
    let fc6 = textureLoad(aux_map, vec2<i32>(6, crow), 0);
    let fc7 = textureLoad(aux_map, vec2<i32>(7, crow), 0);
    let ft = t6_dyn.count.y;
    let fuv0 = uv * fc0.xy;
    let fuv = fuv0 + ft * fc0.zw;
    let fnoise = fuv.xyxy * fc3;
    let femit = fuv0.xyxy * fc1 + ft * fc2;
    let fhi = textureSample(specular_map, colour_sampler, femit.xy).r;
    let flo = textureSample(specular_map, colour_sampler, femit.zw).g;
    let fny = textureSample(specular_map, colour_sampler, fnoise.xy).b;
    let fna = textureSample(specular_map, colour_sampler, fnoise.zw).a;
    let fheat = fhi * flo + fny * vcolour.b * fc6.w;
    var frow = saturate((fna - 0.5) * 0.25 + vcolour.r);
    frow = frow * frow * (3.0 - 2.0 * frow);
    let fruv = vec2(fheat, frow) * fc5.xy + fc5.zw;
    let frows = f32(crow);
    let fremap = textureSample(aux_map, colour_sampler, vec2(fruv.x, fruv.y * frows / (frows + 1.0)));
    let falbedo = textureSample(colour_map, colour_sampler, fuv);
    let fnm = (textureSample(normal_map, colour_sampler, fuv).xy * fc4.x + fc4.y) * fremap.a;
    let fn_ = normalize(fnm.x * tangent + fnm.y * binormal + vn);
    let fv = normalize(p - view.world_position);
    let fl = t6.sun_dir_exposure.xyz;
    let fh = normalize(fl - fv);
    let fndoth = saturate(dot(fn_, fh));
    let fdd = fndoth * fndoth * -0.7599 + 1.0;
    let fd = 0.2401 / max(fdd * fdd, 0.000001);
    let fhv = saturate(dot(fh, -fv));
    let fhl = saturate(dot(fh, fl));
    let fnv = saturate(dot(fn_, -fv));
    let fnl = saturate(dot(fn_, fl));
    let fq = 1.0 - fhl;
    let fspec = fd * (fq * fq * fq * fq * fq * 0.85 + 0.15) / max(fhv * 4.0, 0.0001) * 0.3184;
    let f90 = fhl * fhl * 0.98 - 0.5;
    let fqv = 1.0 - fnv;
    let fql = 1.0 - fnl;
    let fburley = (1.0 + f90 * fqv * fqv * fqv * fqv * fqv) * (1.0 + f90 * fql * fql * fql * fql * fql);
    let fbase = raw_texel(falbedo.rgb);
    let fr2 = fremap * fremap;
    let flit = fnl * (fbase * fbase * fburley + vec3(fspec)) * t6.sun_color.rgb;
    let fglow = fr2.rgb * fc6.rgb * fnv + fr2.rgb * fc7.rgb;
    return flit + fglow;
}
#endif

// bo2zm: Black Ops II's lit world colour, as its lightmap shaders compute it
// (pimp_shader_lmap_*, read from the game's own shader code): the colour
// map times the vertex colour, squared into linear; the lightmap's three
// pages at (u, v / 3 + page / 3): base light and directional light (rgb / a),
// light direction (rgb * 2 - 1) with the baked visibility of the surface's
// primary light in alpha; light = base + directional * N.dir + primary *
// visibility; then the BO2 output. Normal maps, specular and fog are not
// drawn.
fn shade_lightmapped(in: VertexOut) -> vec4<f32> {
    let albedo = textureSample(colour_map, colour_sampler, in.uv);
#ifdef ALPHA_TEST
    // The game's alpha-tested shaders discard below 128/255.
    if (albedo.a < 0.501961) {
        discard;
    }
#endif
    let texel = raw_texel(albedo.rgb);
#ifdef UNTINTED
    // Layered materials: the vertex colour is the layer weights (green
    // layer 1, blue layer 2), not a tint. As the game's layer shaders: a
    // blend layer mixes in by its alpha times its weight (an alpha-revealed
    // one by its height against the weight, `t6_layer_cover`; red and
    // alpha carry its parms), a multiply layer
    // scales by 1 + weight * (layer - 1); all on raw texels, then squared.
    var c = texel;
#ifdef LAYERS
#ifdef LAYER1_BLEND
    let l1 = textureSample(colour_map1, colour_sampler, in.layer_uv.xy);
    c = mix(c, raw_texel(l1.rgb), t6_layer_cover(l1.a, in.color.g, in.color.r));
#endif
#ifdef LAYER1_MULTIPLY
    let m1 = raw_texel(textureSample(colour_map1, colour_sampler, in.layer_uv.xy).rgb);
    c = c * (1.0 + in.color.g * (m1 - 1.0));
#endif
#ifdef LAYER2_BLEND
    let l2 = textureSample(colour_map2, colour_sampler, in.layer_uv.zw);
    c = mix(c, raw_texel(l2.rgb), t6_layer_cover(l2.a, in.color.b, in.color.a));
#endif
#ifdef LAYER2_MULTIPLY
    let m2 = raw_texel(textureSample(colour_map2, colour_sampler, in.layer_uv.zw).rgb);
    c = c * (1.0 + in.color.b * (m2 - 1.0));
#endif
    // bo2mp: a test layer (`t1c1`, the `lit_sm_*_t1c1n1` pixel shader):
    // the layer replaces the colour where its alpha times its weight is at
    // least one half (its normal map is not drawn).
#ifdef LAYER1_TEST
    let t1 = textureSample(colour_map1, colour_sampler, in.layer_uv.xy);
    c = select(c, raw_texel(t1.rgb), t1.a * in.color.g >= 0.5);
#endif
    // bo2mp: an add layer (`aK`, the `lit_sm_*_a1c1` pixel shader): the
    // layer times its alpha times its weight, added before squaring.
#ifdef LAYER1_ADD
    let a1 = textureSample(colour_map1, colour_sampler, in.layer_uv.xy);
    c = c + raw_texel(a1.rgb) * a1.a * in.color.g;
#endif
#ifdef LAYER2_ADD
    let a2 = textureSample(colour_map2, colour_sampler, in.layer_uv.zw);
    c = c + raw_texel(a2.rgb) * a2.a * in.color.b;
#endif
#endif
    let base = c * c;
#else
    let tinted = texel * in.color.rgb;
    let base = tinted * tinted;
#endif
    // bo2mp: Black Ops III's own light (`t7_shade`): the vertex colour is
    // the baked ambient light, not a tint (on a Black Ops II map, `t7_bo2`:
    // the tint stays, the ambient is its lightmap's).
    if (t7_drawn()) {
        let vn7 = normalize(in.normal);
        var n7 = vn7;
        var st7 = vec4(0.0);
#ifdef SHINE
        // (Modern Warfare Remastered's normal maps are BC5, x and y over
        // 0..1: 2 t - 1; Black Ops III's its own wider code.)
        let nmt7 = textureSample(normal_map, colour_sampler, in.uv).xy;
        let nm7 = select(nmt7 * 4.015748 - 2.015748, nmt7 * 2.0 - 1.0, t7_mwr());
        n7 = normalize(nm7.x * in.tangent + nm7.y * in.binormal + vn7);
        st7 = textureSample(specular_map, colour_sampler, in.uv);
#endif
        var base7 = texel * texel;
        var amb7 = in.color.rgb;
        var alpha7 = t7_reveal(albedo.a, in.color.a);
        // (The surface's primary light and its baked visibility: a spot or
        // omni light the lightmap's ambient pages leave out.)
        var pl7 = 999u;
        var plv7 = 0.0;
        if (t7_bo2()) {
            let v7 = in.lightmap_uv.y / 3.0;
            let q0 = textureSample(lightmap, lightmap_sampler, vec2(in.lightmap_uv.x, v7));
            let q1 = textureSample(lightmap, lightmap_sampler, vec2(in.lightmap_uv.x, v7 + 1.0 / 3.0));
            let q2 = textureSample(lightmap, lightmap_sampler, vec2(in.lightmap_uv.x, v7 + 2.0 / 3.0));
            amb7 = q0.rgb / (q0.a + 0.000001) + q1.rgb / (q1.a + 0.000001) * saturate(dot(q2.rgb * 2.0 - 1.0, n7));
            if (t7_iw_pages()) {
                // (A + B f, f = saturate(rsqrt(1 + x^2 + y^2)) as CoD4's
                // lit shaders on a flat surface.)
                let xi = q0.a * 4.08 - 2.08;
                let yi = q1.a * 4.064516 - 2.064516;
                let fi = saturate(inverseSqrt(1.0 + xi * xi + yi * yi));
                amb7 = (q0.rgb * q0.rgb + q1.rgb * q1.rgb * fi) * 4.0;
                if (t7_mwr()) {
                    // Modern Warfare Remastered's: both pages' alphas the
                    // baked light's tangent direction (x, y; z the rest of
                    // the unit length), packed alike (`iw3_lightmap`).
                    let dx = q0.a * 4.08 - 2.08;
                    let dy = q1.a * 4.08 - 2.08;
                    let dz = sqrt(max(1.0 - dx * dx - dy * dy, 0.0));
                    let dirw = normalize(dx * in.tangent + dy * in.binormal + dz * vn7);
                    let a7 = q0.rgb * q0.rgb * 4.0;
                    let b7 = q1.rgb * q1.rgb * 4.0;
                    let lit_m = mwr_shade(base, a7, b7, dirw, vn7, n7, st7, in.probe, in.world_position, q2.a);
                    return vec4(t7_out(bo2_fog(lit_m, in.world_position)), select(albedo.a * in.color.a, t7_reveal(albedo.a, in.color.a), in.color.a < -1.5));
                }
            }
            base7 = base;
            pl7 = in.light_index;
            plv7 = q2.a;
            // (A reveal decal's packed vertex alpha: `t7_reveal`.)
            alpha7 = select(albedo.a * in.color.a, t7_reveal(albedo.a, in.color.a), in.color.a < -1.5);
        }
        let lit7 = t7_fog(t7_shade(base7, amb7, vn7, n7, st7, in.probe, in.world_position, in.world_position, true, false, pl7, plv7), in.world_position);
        return vec4(t7_out(bo2_fog(lit7, in.world_position)), alpha7);
    }
    let v = in.lightmap_uv.y / 3.0;
    let page0 = textureSample(lightmap, lightmap_sampler, vec2(in.lightmap_uv.x, v));
    let page1 = textureSample(lightmap, lightmap_sampler, vec2(in.lightmap_uv.x, v + 1.0 / 3.0));
    let page2 = textureSample(lightmap, lightmap_sampler, vec2(in.lightmap_uv.x, v + 2.0 / 3.0));
    // bo2mp: a Call of Duty 4 / World at War lightmap (the sun colour's
    // alpha is 2 or more: 2 Ghosts' and Infinite Warfare's, 3 CoD4's and
    // World at War's, 4 those with their own reflection probes;
    // `world_iw3::iw3_lightmap` packs its pages): CoD4's own lit world
    // shaders (`lm_sm_sun_*_sm3`, read from the zone's pixel shaders)
    // light in gamma space: A + B * f, x and y the dominant light's
    // tangent direction from the alphas, f = saturate(rsqrt(1 + x^2 +
    // y^2)) on a flat surface, plus the sun by N.L and its baked visibility
    // (the primary image, page 2 alpha), times the colour map and vertex
    // colour. Written as base * light^2 so the BO2 output's square root
    // gives CoD4's texel * light back.
    if (t6.sun_color.a > 1.5) {
        let x = page0.a * 4.08 - 2.08;
        let y = page1.a * 4.064516 - 2.064516;
        var f = saturate(inverseSqrt(1.0 + x * x + y * y));
        var n_iw3 = normalize(in.normal);
#ifdef WATER
        // bo2mp: CoD4 / World at War water (`wc_water`; the lane's
        // `legacy_water_table`). The games simulate their waves each frame;
        // here four scrolled samples of a ripple map make the normal (water
        // lies flat: no tangent frame), and, in CoD4's gamma space, the
        // water's colour under its baked light and the sun mixes toward the
        // reflection probe by the material's fresnel (envMapParms x to y by
        // (1 - N.V)^z), plus the sun's glint on the ripples.
        let wl0 = textureLoad(colour_map, vec2<i32>(0, 0), 0);
        let wl1 = textureLoad(colour_map, vec2<i32>(1, 0), 0);
        let wl2 = textureLoad(colour_map, vec2<i32>(2, 0), 0);
        let wl3 = textureLoad(colour_map, vec2<i32>(3, 0), 0);
        let wns = textureLoad(colour_map, vec2<i32>(4, 0), 0);
        let wlw = textureLoad(colour_map, vec2<i32>(5, 0), 0);
        let wc0 = textureLoad(colour_map, vec2<i32>(6, 0), 0);
        let wc1 = textureLoad(colour_map, vec2<i32>(7, 0), 0);
        let wca = textureLoad(colour_map, vec2<i32>(8, 0), 0);
        let wcb = textureLoad(colour_map, vec2<i32>(9, 0), 0);
        let wt = t6_dyn.count.y;
        let wp = in.world_position.xy;
        let na = textureSample(normal_map, colour_sampler, wp * wl0.xy + wt * wl0.zw).xy * wns.x + wns.y;
        let nb = textureSample(normal_map, colour_sampler, wp * wl1.xy + wt * wl1.zw).xy * wns.x + wns.y;
        let nc = textureSample(specular_map, colour_sampler, wp * wl2.xy + wt * wl2.zw).xy * wns.z + wns.w;
        let nd = textureSample(specular_map, colour_sampler, wp * wl3.xy + wt * wl3.zw).xy * wns.z + wns.w;
        let wn = normalize(vec3(na * wlw.x + nb * wlw.y + nc * wlw.z + nd * wlw.w, 1.0));
        let wv = normalize(in.world_position - view.world_position);
        let wndotv = saturate(dot(wn, -wv));
        let wfres = saturate(wc0.z + wc0.w * pow(1.0 - wndotv, max(wc0.y, 0.5)));
        let wvis = page2.a * sun_shadow_at(in.world_position);
        let wsun = t6.sun_color.rgb * wvis;
        // (The games light water by the sun and its reflection, not a
        // lightmap: where its baked light is dark it keeps a third of the
        // sun's colour, so a gutter is never a black trench.)
        let wlg = max(
            page0.rgb + page1.rgb * f + wsun * saturate(dot(wn, t6.sun_dir_exposure.xyz)),
            t6.sun_color.rgb * 0.35,
        );
        let wr = wv - 2.0 * dot(wv, wn) * wn;
        let we = textureSampleLevel(probe_cubes, probe_sampler, wr, i32(in.probe), 0.0);
        // (The map's own probes are gamma values, their alpha folded in; a
        // Black Ops II model map's are linear rgb / a for that map's light,
        // relit by the light here as the shine branch does. A black one,
        // the compiler's stand-in, reflects the light here instead.)
        let wlum = clamp(dot(wlg * wlg, vec3(0.299, 0.587, 0.114)), 0.0, 2.0);
        let wprobe = select(sqrt(max(we.rgb / (we.a + 0.000001) * wlum, vec3(0.0))), we.rgb, t6.sun_color.a > 3.5);
        let wenv = select(wprobe, wlg * 0.6, dot(wprobe, vec3(0.333)) < 0.02);
        let wbody = mix(wca, wcb, wndotv);
        var wg = mix(wbody.rgb * wlg, wenv, wfres);
        let wglint = exp2(log2(max(saturate(dot(wr, t6.sun_dir_exposure.xyz)), 0.000001)) * max(wc0.x, 1.0));
        wg += wsun * (wglint * wc1.x);
        let wa = saturate(mix(wbody.a, 1.0, wfres));
        return vec4(bo2_out(bo2_fog(wg * wg, in.world_position)), wa);
#endif
#ifdef SHINE
        // bo2mp: the normal map (Black Ops II's layout: a CoD4 map's xGxR
        // one is moved there when linked, `world_iw3::iw3_normal_to_bo2`)
        // bends the baked dominant light (tangent space (x, y, 1)) and the
        // sun: CoD4's `lm_*n0*` shaders take f = saturate(dot of the two,
        // normalised) and the world normal N + nx * T + ny * B.
        let nm3 = textureSample(normal_map, colour_sampler, in.uv).xy * 4.015748 - 2.015748;
        f = saturate(dot(normalize(vec3(nm3, 1.0)), normalize(vec3(x, y, 1.0))));
        n_iw3 = normalize(nm3.x * in.tangent + nm3.y * in.binormal + n_iw3);
#endif
        // bo2mp: an Infinite Warfare map's pages (the sun colour's alpha 5,
        // `fastfile_iw7::light`) hold linear light: each page is the square
        // root of its light over 4 (squared and added: A + B * f, IW7's
        // `lmap_specenv_*` shaders), the sun linear, its baked shadow a
        // signed distance (`smoothstep(saturate((2p - 1 + k) / 2k))`, k
        // `light::SUN_SHADOW_SOFTNESS`).
        let linear_iw3 = t6.sun_color.a > 4.5;
        var sun_vis = page2.a;
        if (linear_iw3) {
            let st = saturate((page2.a * 2.0 - 1.0 + 0.25) / 0.5);
            sun_vis = st * st * (3.0 - 2.0 * st);
        }
        sun_vis *= sun_shadow_at(in.world_position);
        let sun_iw3 = t6.sun_color.rgb * sun_vis * saturate(dot(n_iw3, t6.sun_dir_exposure.xyz));
        let lg = page0.rgb + page1.rgb * f + sun_iw3;
        var here_iw3 = lg * lg;
        if (linear_iw3) {
            here_iw3 = (page0.rgb * page0.rgb + page1.rgb * page1.rgb * f) * 4.0 + sun_iw3;
        }
        var lit_iw3 = base * (here_iw3 + dyn_lights(in.world_position, n_iw3));
#ifdef SHINE
        let st3 = textureSample(specular_map, colour_sampler, in.uv);
        let v3 = normalize(in.world_position - view.world_position);
        let r3 = v3 - 2.0 * dot(v3, n_iw3) * n_iw3;
        let gloss3 = st3.a;
        if (t6.sun_color.a > 3.5 && !linear_iw3) {
            // bo2mp: the map's own specular map and reflection probes, as
            // CoD4's `lm_sm_sun_r0c0n0s0` adds them, in gamma space: the
            // probe (rgb times its alpha, folded in when linked) at level
            // 6 - 8 * gloss, plus the sun's highlight e^((R.L - 0.99925) *
            // (2^(9.3775 * gloss) + 7)) times the sun's colour, its
            // visibility and 0.625 (envMapParms.w); both times the specular
            // colour and the fresnel lerp(0.8, 2, 1 - |N.V|) (envMapParms
            // 0.8 2 1: most of these maps' materials carry it).
            let spec_g = raw_texel(st3.rgb);
            let env_g = textureSampleLevel(probe_cubes, probe_sampler, r3, i32(in.probe), clamp(6.0 - 8.0 * gloss3, 0.0, 6.0)).rgb;
            let power = exp2(gloss3 * 9.377518) + 7.0;
            let lobe = saturate(exp((dot(r3, t6.sun_dir_exposure.xyz) - 0.99925) * power));
            let sun_spec = t6.sun_color.rgb * (sun_vis * lobe * 0.625);
            let fres = mix(0.8, 2.0, 1.0 - abs(dot(n_iw3, v3)));
            let g = sqrt(lit_iw3) + (env_g + sun_spec) * spec_g * fres;
            lit_iw3 = g * g;
        } else {
            let sr3 = raw_texel(st3.rgb);
            let spec3 = sr3 * sr3;
            let l3 = t6.sun_dir_exposure.xyz;
            let h3 = normalize(l3 - v3);
            let ndotl3 = saturate(dot(n_iw3, l3));
            let ndoth3 = saturate(dot(n_iw3, h3));
            let ldoth3 = saturate(dot(h3, l3));
            let power3 = exp2(gloss3 * 13.0);
            let lobe3 = exp2(log2(max(ndoth3, 0.000001)) * power3) * (power3 * 0.125 + 0.25);
            let k3 = saturate(gloss3 + 0.545);
            let vis3 = ndotl3 / (ldoth3 * ldoth3 * k3 - k3 + 1.01);
            let fres3 = spec3 + (1.0 - spec3) * exp2(-10.0 * ldoth3);
            let sun_lin = t6.sun_color.rgb * sun_vis;
            lit_iw3 += fres3 * lobe3 * vis3 * select(sun_lin * sun_lin, sun_lin, linear_iw3);
            // The reflection: the model map's probe (`bo2_look` picks one
            // for every surface), lit as the light here.
            let ndotv3 = saturate(dot(n_iw3, -v3));
            let fk3 = gloss3 * vec4(1.041667, 0.475, 0.018229, 0.25) + vec4(0.0, 0.0, -0.015625, 0.75);
            var ff3 = min(exp2(-9.28 * ndotv3), fk3.y);
            ff3 = fk3.x * ff3 + fk3.z;
            let ew3 = saturate(spec3 * (fk3.w - ff3) + ff3);
            let env3 = textureSampleLevel(probe_cubes, probe_sampler, r3, i32(in.probe), 4.0 - 4.0 * gloss3);
            let env3_rgb = env3.rgb / (env3.a + 0.000001);
            let here3 = here_iw3;
            let probe3 = max(dot(here3, vec3(0.299, 0.587, 0.114)), 0.0001);
            lit_iw3 += ew3 * env3_rgb * clamp(probe3, 0.0, 2.0);
        }
#endif
        return vec4(bo2_out(bo2_fog(lit_iw3, in.world_position)), albedo.a);
    }
    let ambient = page0.rgb / (page0.a + 0.000001);
    let directional = page1.rgb / (page1.a + 0.000001);
    let dir = page2.rgb * 2.0 - 1.0;
#ifdef SHINE
#ifdef FLOW
    // bo2mp: Black Ops II's emissive flow (`bo2_flow`).
    return vec4(
        bo2_out(bo2_flow(in.uv, in.color, normalize(in.normal), in.tangent, in.binormal, in.world_position)),
        1.0,
    );
#endif
#ifdef WATER
    // bo2mp: Black Ops II's water (`cod7water` pools, `cod7watershore` seas
    // and rivers; their sun-shadow techniques, fxc disassembly). The colour
    // map holds the material's constants (assets lane `water_table`), the
    // normal and specular maps its two normal maps. Four normal samples
    // scrolled over the world's xy by game time make the water's normal
    // (as the game's, no tangent frame: water lies flat); fresnel (bias and
    // scale on (1 - n.v)^5) mixes its colour with the reflection probe,
    // relit by the light here over the light the probe saw straight up;
    // the sun adds two highlight lobes. A sea also draws its whitewater and,
    // by half on open water, its shore picture (the aux map). The game refracts the scene behind
    // by the water's opacity; here the water blends over it instead.
    let wl0 = textureLoad(colour_map, vec2<i32>(0, 0), 0);
    let wl1 = textureLoad(colour_map, vec2<i32>(1, 0), 0);
    let wl2 = textureLoad(colour_map, vec2<i32>(2, 0), 0);
    let wl3 = textureLoad(colour_map, vec2<i32>(3, 0), 0);
    let wns = textureLoad(colour_map, vec2<i32>(4, 0), 0);
    let wlw = textureLoad(colour_map, vec2<i32>(5, 0), 0);
    let wc0 = textureLoad(colour_map, vec2<i32>(6, 0), 0);
    let wc1 = textureLoad(colour_map, vec2<i32>(7, 0), 0);
    let wca = textureLoad(colour_map, vec2<i32>(8, 0), 0);
    let wcb = textureLoad(colour_map, vec2<i32>(9, 0), 0);
    let wsh = textureLoad(colour_map, vec2<i32>(10, 0), 0);
    let wt = t6_dyn.count.y;
    let wp = in.world_position.xy;
    // bo2mp: the sea's filtering (assets lane `sea_filter`, rows 25 and 26 of
    // a sea's table; `BO2MP_SEA_FILTER=0` leaves them out): x specular
    // anti-aliasing strength, y the share of the live sun shadow that reaches
    // the sea, z how far the small ripples fade out with distance and w where
    // that starts; then x where it ends and y the switch.
    let wlast = i32(textureDimensions(colour_map).x) - 1;
    let wsea = wc1.w > 0.5;
    let wfx0 = select(vec4(0.0), textureLoad(colour_map, vec2<i32>(min(25, wlast), 0), 0), wsea);
    let wfx1 = select(vec4(0.0), textureLoad(colour_map, vec2<i32>(min(26, wlast), 0), 0), wsea);
    let wfon = wfx1.y > 0.5;
    let na = textureSample(normal_map, colour_sampler, wp * wl0.xy + wt * wl0.zw).xy * wns.x + wns.y;
    let nb = textureSample(normal_map, colour_sampler, wp * wl1.xy + wt * wl1.zw).xy * wns.x + wns.y;
    let nc = textureSample(specular_map, colour_sampler, wp * wl2.xy + wt * wl2.zw).xy * wns.z + wns.w;
    let nd = textureSample(specular_map, colour_sampler, wp * wl3.xy + wt * wl3.zw).xy * wns.z + wns.w;
    // The small ripples fade toward flat with distance (filtered, not
    // dropped: the layers' mips already average them, this finishes what a
    // 4-layer sum leaves at grazing angles).
    let wdist = length(in.world_position - view.world_position);
    let wflat = select(0.0, wfx0.z * smoothstep(wfx0.w, max(wfx1.x, wfx0.w + 1.0), wdist), wfon);
    let wn = normalize(vec3((na * wlw.x + nb * wlw.y + nc * wlw.z + nd * wlw.w) * (1.0 - wflat), 1.0));
    let wv = normalize(in.world_position - view.world_position);
    let wndotv = saturate(dot(wn, -wv));
    let wf = 1.0 - wndotv;
    let wfres = wc0.w * wf * wf * wf * wf * wf + wc0.z;
    let wray = primary_light_ray_with(in.light_index, in.world_position, !(wsea && wfon));
    let wlive = select(1.0, mix(1.0, sun_shadow_at(in.world_position), wfx0.y), wsea && wfon);
    let wndotl = saturate(dot(wn, wray.dir));
    let wamb = ambient + directional * 0.25;
    let wr = wv - 2.0 * dot(wv, wn) * wn;
    // The game's cube read takes its level from the ray's change across
    // the pixel: far ripples blur the sky instead of picking single dark
    // texels below its horizon (Takeoff's far grid).
    let wenv = textureSample(probe_cubes, probe_sampler, wr, i32(in.probe));
    let wpi = min(in.probe, 31u) * 3u;
    let ws0 = probes.v[wpi];
    let ws1 = probes.v[wpi + 1u];
    let wprobe = max((ws1.z + ws1.w + ws0.w) * ws0.xyz, vec3(0.1));
    // The pool relights the probe by the light here over the probe's own;
    // the sea reflects its own sky cube (its material's, a layer past the
    // world's probes: assets lane `sea_sky_cubes`).
    // The sea's shader (06.ps.wgsl:176-195) squares the cube and never
    // reads its alpha: rgb / a lit it about three times over (Takeoff).
    let wenv_rgb = wenv.rgb / (wenv.a + 0.000001);
    let wrefl = select(wenv_rgb * wamb / wprobe, wenv.rgb * wenv.rgb, wc1.w > 0.5);
    let wbody = mix(wca, wcb, wndotv);
    // The sun: the pool's by its baked visibility; the sea's at least
    // three quarters (its shadow term plus 0.75); a distant sea (one with a
    // horizon colour) has no shadow term and no lightmap of its own: full sun.
    // (Under the sea filter the live shadow of the carrier and the ships goes
    // into the baked term, so it can take the sea down to that floor and no
    // lower: a hard shadow edge on open water was a dark wedge.)
    let wfar = wsea && textureLoad(colour_map, vec2<i32>(min(22, i32(textureDimensions(colour_map).x) - 1), 0), 0).a > 0.5;
    let wsun = wray.colour * select(select(page2.a, saturate(page2.a * wlive + 0.75), wsea), 1.0, wfar);
    // bo2mp: the sea's whitewater and shore (assets lane `sea_layers`;
    // rows past the pool's table read its last row, whose flags are 0).
    // The aux map holds whiteWaterMask green and alpha, the shore map's
    // green and blue.
    let wm8s = textureLoad(colour_map, vec2<i32>(min(11, wlast), 0), 0);
    let wm8t = textureLoad(colour_map, vec2<i32>(min(12, wlast), 0), 0);
    let wm9s = textureLoad(colour_map, vec2<i32>(min(13, wlast), 0), 0);
    let wm9t = textureLoad(colour_map, vec2<i32>(min(14, wlast), 0), 0);
    let wwc = textureLoad(colour_map, vec2<i32>(min(15, wlast), 0), 0);
    let wsuv = textureLoad(colour_map, vec2<i32>(min(16, wlast), 0), 0);
    let wnw = textureLoad(colour_map, vec2<i32>(min(17, wlast), 0), 0);
    let wfl = textureLoad(colour_map, vec2<i32>(min(18, wlast), 0), 0);
    let wsx = textureLoad(colour_map, vec2<i32>(min(19, wlast), 0), 0);
    let wflags = u32(max(wfl.w, 0.0) + 0.5);
    let wshore_on = wsea && (wflags & 1u) != 0u;
    let wfoam_on = wsea && (wflags & 2u) != 0u;
    // The waves map (foam, depth, the sea's share over the shore picture):
    // under the table when the sea has a real one (flag 4; assets lane
    // `sea_table`), read by the mesh's uv nudged by the normal
    // (controlVar1.z), as the game's; else its open-water values. Its edges
    // are open water, so the sea meets its distant sea without a seam
    // (Yemen's square).
    var wwave = vec3(wfl.x, wfl.y, wsx.y);
    if (wsea && (wflags & 4u) != 0u) {
        let wtop = vec2<i32>(textureDimensions(colour_map)) - vec2(1, 2);
        let wst = clamp(in.uv + wn.xy * wsx.z, vec2(0.0), vec2(1.0)) * vec2<f32>(wtop);
        let wi = vec2<i32>(floor(wst));
        let wj = min(wi + 1, wtop);
        let wfr = wst - floor(wst);
        let w00 = textureLoad(colour_map, vec2(wi.x, wi.y + 1), 0);
        let w10 = textureLoad(colour_map, vec2(wj.x, wi.y + 1), 0);
        let w01 = textureLoad(colour_map, vec2(wi.x, wj.y + 1), 0);
        let w11 = textureLoad(colour_map, vec2(wj.x, wj.y + 1), 0);
        let wwm = mix(mix(w00, w10, wfr.x), mix(w01, w11, wfr.x), wfr.y);
        wwave = vec3(wwm.x, wwm.y, wwm.w);
    }
    // The deep colours by that depth (the table's are shallow + delta, at
    // depth 1: the game's shallow + delta * depth) and a distant sea's
    // horizon fade.
    let wdd = wwave.y - 1.0;
    let wsca = wca.rgb + textureLoad(colour_map, vec2<i32>(min(20, wlast), 0), 0).rgb * wdd;
    let wscb = wcb.rgb + textureLoad(colour_map, vec2<i32>(min(21, wlast), 0), 0).rgb * wdd;
    let whz = textureLoad(colour_map, vec2<i32>(min(22, wlast), 0), 0);
    let whf = textureLoad(colour_map, vec2<i32>(min(23, wlast), 0), 0);
    let wrs = textureLoad(colour_map, vec2<i32>(min(24, wlast), 0), 0);
    let wshare = select(1.0, wwave.z, wshore_on);
    let wm8 = wp.xyxy * wm8s + wt * wm8t;
    let wm9 = wp.xyxy * wm9s + wt * wm9t;
    let wma = vec4(
        textureSample(aux_map, colour_sampler, wm8.xy).y,
        textureSample(aux_map, colour_sampler, wm8.zw).x,
        textureSample(aux_map, colour_sampler, wm9.xy).y,
        textureSample(aux_map, colour_sampler, wm9.zw).x,
    );
    let wsuv2 = wp * wsuv.xy + wsuv.zw + wn.xy * wsx.x * sqrt(wshare);
    let wsa = textureSample(aux_map, colour_sampler, wsuv2);
    var wsolid: vec3<f32>;
    var wadd: vec3<f32>;
    var wspecw = 1.0;
    if (wsea) {
        let wlit = wsun * (wndotl / max(wndotv + wndotl, 0.0001)) + wsh.rgb;
        var wcol = mix(wsca, wscb, wfres);
        if (wfoam_on) {
            // Whitecaps: four mask samples, weighted by the noise; the
            // waves map's foam (wwave.x) at shores. Both terms also take the
            // last two noise weights, as the game's do.
            let ww = dot(wnw, wma);
            let wsq = sqrt(ww);
            let wx = saturate(vec2(wwave.x * 2.0, (saturate(ww * wsq * 0.65 + wwave.x) - 0.25) * 1.333333));
            let wfoam = wx * wx * (3.0 - 2.0 * wx) * wnw.zw * vec2(wsq, ww);
            wcol = mix(wcol, saturate(wcol * wwc.w), wfoam.x);
            wcol = mix(wcol, saturate(wwc.rgb * wfoam.y + wcol), wfoam.y);
        }
        // A distant sea's reflection fades to its horizon colour with
        // distance (the game's distant-sea shader, cod7watershore q74e9z9j
        // 06.ps: horizonColor by exp2(distance * slope + bias), at most 1),
        // and every sea's reflection is customReflectionScale times that
        // (06.ps:208, cb2 row 14: Yemen's 24). Without it the distant sea
        // came out about 35 times darker than the fogged sky at the horizon:
        // a hard line where the sea meets the sky.
        let wfade = min(exp2(length(in.world_position - view.world_position) * whf.x + whf.y), 1.0);
        let wsky = select(wrefl, mix(whz.rgb, wrefl, wfade), whz.a > 0.5) * wrs.rgb;
        wsolid = mix(wcol * wlit, wsky, wfres);
        if (wshore_on) {
            // The shore picture lit flat, tinted by the body colour, under
            // the sea by the waves map's alpha (half, open water).
            let wshc = vec3(wsa.z * wfl.z, wsa.z, wsa.w);
            let weye = -wv;
            let wslit = wsun * (wray.dir.z / max(wray.dir.z + weye.z, 0.0001)) + wsh.rgb;
            let wshore = wshc * wshc * wslit * mix(vec3(1.0), wscb, wshare);
            wsolid = mix(wshore, wsolid, wshare);
            wspecw = sqrt(wshare);
        }
        wadd = vec3(0.0);
    } else {
        let wlight = wamb + wsun * wndotl + dyn_lights(in.world_position, wn);
        wsolid = wbody.rgb * (1.0 - wfres) * wlight;
        wadd = wrefl * wfres;
    }
    let wh = normalize(wray.dir - wv);
    let wndoth = max(saturate(dot(wn, wh)), 0.000001);
    // bo2mp: specular anti-aliasing (the sea filter). The glint lobes are
    // sharp powers: a pixel that spans many ripples picks single bright ones
    // (salt and pepper). The lobe is widened by the half vector's change
    // across the pixel (slope variance, as Kaplanyan's geometric filter,
    // widening capped at 0.18), its power lowered to match and its height
    // scaled so the energy the lobe holds stays the same.
    let wdhx = dpdx(wh.xy);
    let wdhy = dpdy(wh.xy);
    let wsig = select(0.0, 0.25 * (dot(wdhx, wdhx) + dot(wdhy, wdhy)) * wfx0.x, wfon);
    let wwid = min(2.0 * wsig, 0.18);
    let wa0 = 2.0 / (max(wc0.x, 0.0) + 2.0);
    let wa1 = 2.0 / (max(wc0.y, 0.0) + 2.0);
    let wb0 = min(wa0 + wwid, 1.0);
    let wb1 = min(wa1 + wwid, 1.0);
    let wlobe = exp2(log2(wndoth) * (2.0 / wb0 - 2.0)) * wc1.x * (wa0 / wb0)
        + exp2(log2(wndoth) * (2.0 / wb1 - 2.0)) * wc1.y * (wa1 / wb1);
    let whl = 1.0 - saturate(dot(wh, wray.dir));
    let wfh = wc0.w * whl * whl * whl * whl * whl + wc0.z;
    let whv = max(saturate(dot(wh, -wv)) * 4.0, 0.0001);
    wadd += wsun * (wlobe * wfh * wndotl / whv * wc1.z) * wspecw;
    // The open sea (kind 2) never reads the vertex colour: its far quads
    // carry alpha 0 at their edges (Frostbite's distant sea).
    let wvalpha = select(in.color.a, 1.0, wc1.w > 1.5);
    let wa = saturate(select(wbody.a, wca.a, wsea) * wvalpha);
    let wout = wsolid + wadd / max(wa, 0.05);
    return vec4(bo2_out(bo2_fog(wout, in.world_position)), wa);
#endif
    // bo2zm: Black Ops II's lit world shader (`wpc_lit_sm_r0c0n0s0`, its
    // sun-shadow technique, from the fxc disassembly), with the lightmap's
    // baked sun visibility (page 2 alpha) for its shadow map.
    let vn = normalize(in.normal);
    var nm = textureSample(normal_map, colour_sampler, in.uv).xy * 4.015748 - 2.015748;
    var spec_tex = textureSample(specular_map, colour_sampler, in.uv);
#ifdef LAYER1_SPEC
    // bo2mp: where the blend layer covers (`lit_sm_*_b1c1n1s1`: Downhill's
    // snow over its ice), its own specular map, and the base normal map
    // fades out under it.
    let cover1 = t6_layer_cover(
        textureSample(colour_map1, colour_sampler, in.layer_uv.xy).a,
        in.color.g,
        in.color.r,
    );
    spec_tex = mix(spec_tex, textureSample(specular_map1, colour_sampler, in.layer_uv.xy), cover1);
    nm = nm * (1.0 - cover1);
#endif
    let n = normalize(nm.x * in.tangent + nm.y * in.binormal + vn);
    let spec_raw = raw_texel(spec_tex.rgb);
    let spec = spec_raw * spec_raw;
    let gloss = spec_tex.a;
    let vdir = normalize(in.world_position - view.world_position);
    var light = ambient + directional * saturate(dot(dir, n));
    // The reflection: the surface's probe by the reflected view, blurrier
    // the lower the gloss, weighted by a gloss-shaped fresnel and scaled by
    // the light here (vertex normal) over the light the probe saw (its SH
    // at the normal).
    let ndotv = saturate(dot(n, -vdir));
    let fk = gloss * vec4(1.041667, 0.475, 0.018229, 0.25) + vec4(0.0, 0.0, -0.015625, 0.75);
    var f = min(exp2(-9.28 * ndotv), fk.y);
    f = fk.x * f + fk.z;
    let env_weight = saturate(spec * (fk.w - f) + f);
    let r = vdir - 2.0 * dot(vdir, n) * n;
    let env = textureSampleLevel(probe_cubes, probe_sampler, r, i32(in.probe), 4.0 - 4.0 * gloss);
    let env_rgb = env.rgb / (env.a + 0.000001);
    let pi = min(in.probe, 31u) * 3u;
    let s0 = probes.v[pi];
    let s1 = probes.v[pi + 1u];
    let s2 = probes.v[pi + 2u];
    let sh = dot(s2, vec4(vn.z * vn.x, vn.y * vn.z, vn.x * vn.y, vn.x * vn.x - vn.y * vn.y))
        + dot(s1, vec4(vn, 1.0)) + s0.w * vn.z * vn.z;
    let probe_light = max(sh * s0.xyz, vec3(0.1));
    let here = ambient + directional * saturate(dot(dir, vn));
    let reflection = env_weight * env_rgb * here / probe_light;
    // The primary light: diffuse, and its highlight (normalised
    // Blinn-Phong by gloss, BO2's visibility and fresnel terms).
    let ray = primary_light_ray(in.light_index, in.world_position);
    let lit = ray.colour * page2.a;
    let ndotl = saturate(dot(n, ray.dir));
    light += lit * ndotl;
    light += dyn_lights(in.world_position, n);
    let h = normalize(ray.dir - vdir);
    let ndoth = saturate(dot(n, h));
    let ldoth = saturate(dot(h, ray.dir));
    let power = exp2(gloss * 13.0);
    let lobe = exp2(log2(max(ndoth, 0.000001)) * power) * (power * 0.125 + 0.25);
    let k = saturate(gloss + 0.545);
    let vis = ndotl / (ldoth * ldoth * k - k + 1.01);
    let fres = spec + (1.0 - spec) * exp2(-10.0 * ldoth);
    let highlight = fres * lobe * vis * lit;
    let colour = base * light + reflection + highlight;
    return vec4(bo2_out(bo2_fog(colour, in.world_position)), albedo.a);
#else
    let n = normalize(in.normal);
    var light = ambient + directional * saturate(dot(dir, n));
    light += primary_light(in.light_index, in.world_position, n) * page2.a;
    light += dyn_lights(in.world_position, n);
    return vec4(bo2_out(bo2_fog(base * light, in.world_position)), albedo.a);
#endif
}

// bo2zm: Black Ops II props. The instance carries its world-from-local
// columns, its light: rgb from the light grid (linear, already times the
// exposure, w = 0), or for an instance with baked vertex light w = the
// exposure, the vertex colour then holding that light ((c^2) * 32, as the
// game's mlv_* shaders read it); then its primary light and that light's
// visibility from the light grid.
struct PropVertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(6) world_from_local_0: vec4<f32>,
    @location(7) world_from_local_1: vec4<f32>,
    @location(8) world_from_local_2: vec4<f32>,
    @location(9) world_from_local_3: vec4<f32>,
    @location(10) light: vec4<f32>,
    @location(11) primary: vec4<f32>,
}

struct PropOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) light: vec4<f32>,
    @location(4) world_position: vec3<f32>,
    @location(5) primary: vec4<f32>,
#ifdef DECAL
    // A projected decal's box from the world (columns).
    @location(6) @interpolate(flat) box0: vec4<f32>,
    @location(7) @interpolate(flat) box1: vec4<f32>,
    @location(8) @interpolate(flat) box2: vec4<f32>,
    @location(9) @interpolate(flat) box3: vec4<f32>,
#endif
}

@vertex
fn vertex_prop(in: PropVertexIn) -> PropOut {
    let world_from_local = mat4x4<f32>(
        in.world_from_local_0,
        in.world_from_local_1,
        in.world_from_local_2,
        in.world_from_local_3,
    );
    var out: PropOut;
#ifdef CLOUD
    // bo2zm: Black Ops II particle clouds (`particlecloud_*` vertex shader):
    // the point of the cloud's box to the world by the cloud's placement,
    // then out to a quad facing the camera, the corner (texcoord less one
    // half) times the particle's width and height (instance primary.xy);
    // the colour is the cloud's (instance light), and nothing feathers it.
    let centre = world_from_local * vec4(in.position, 1.0);
    let corner = in.uv - vec2(0.5);
    let right = view.world_from_view[0].xyz;
    let up = view.world_from_view[1].xyz;
    let world = vec4(centre.xyz + right * (corner.x * in.primary.x) - up * (corner.y * in.primary.y), 1.0);
    out.clip_position = view.clip_from_world * world;
    out.normal = view.world_from_view[2].xyz;
    out.color = in.light;
    out.uv = in.uv;
    out.light = vec4(0.0);
    out.world_position = world.xyz;
    out.primary = vec4(0.0);
#else
    var world = world_from_local * vec4(in.position, 1.0);
#ifdef EFFECT
    // bo2zm M3: BO2's eye offset (an `_eo` effect material's
    // eyeOffsetParms.x, instance slot 23): the sprite is drawn that many
    // units nearer the eye along its line of sight (at least half way),
    // where it already shows on screen, so a flame on a wall or a pole is
    // not cut into by it.
    let eo = in.primary.w;
    if (eo > 0.0) {
        let to_sprite = world.xyz - view.world_position;
        let d = max(length(to_sprite), 0.0001);
        world = vec4(view.world_position + to_sprite * (max(d - eo, d * 0.5) / d), 1.0);
    }
#endif
#ifdef CLOTH
    // bo2mp: cloth and foliage in the wind (`cloth_offset`).
    let cloth_normal = (world_from_local * vec4(in.normal, 0.0)).xyz;
    world = vec4(world.xyz + cloth_offset(in.position, world.xyz, cloth_normal, in.color.a), 1.0);
#endif
#ifdef DECAL
    // bo2zm M4 retest 5: a projected decal's box is already in the world;
    // its instance columns are the box from the world, for the pixels.
    world = vec4(in.position, 1.0);
    out.box0 = in.world_from_local_0;
    out.box1 = in.world_from_local_1;
    out.box2 = in.world_from_local_2;
    out.box3 = in.world_from_local_3;
#endif
    out.clip_position = view.clip_from_world * world;
    out.normal = (world_from_local * vec4(in.normal, 0.0)).xyz;
    out.color = in.color;
    out.uv = in.uv;
    out.light = in.light;
    out.world_position = world.xyz;
    out.primary = in.primary;
    // bo2mp: a weapon camo's surfaces (gold, diamond, ...) tile the camo's
    // pictures across the gun: instance slot 22 holds 1000 + the tiling
    // (its effect feather and unlit dimming are below 1000).
    // (3000 + a pattern camo's repeat passes on to the pixels.)
    if (in.primary.z >= 1000.0 && in.primary.z < 3000.0) {
        out.uv = in.uv * (in.primary.z - 1000.0);
        out.primary.z = 0.0;
    }
#endif
    return out;
}

// bo2mp: a special camo's look over time (`weapon_camos::EffectCamo`),
// from its picture `pat` (display space): rgb the camo's colour, a the
// alpha the surface is drawn with (below 1 only for Ghost, whose gun is
// drawn blended). `t` runs in seconds, wrapping every hour (every period
// here divides 3600 / its pace, so the wrap is seamless to the eye).
fn camo_animate(style: u32, t: f32, uv: vec2<f32>, p: vec3<f32>, pat: vec3<f32>, mask: f32) -> vec4<f32> {
    let tau = 6.2831853;
    let lum = dot(pat, vec3(0.299, 0.587, 0.114));
    if (style == 16u) {
        // Reveal (a camo pack): the picture's alpha is a mask, 0-1; a
        // threshold sweeps through it (8 s) between 0.1 and 0.9 (never all
        // lit or all dim): the picture is brighter where the mask is above
        // it, a hot edge on the line between. The dim side is still the
        // picture (with a violet floor where it is black): the camo never
        // goes dark.
        let thr = 0.5 + 0.4 * sin(t * tau / 8.0);
        let lit = smoothstep(thr - 0.05, thr + 0.05, mask);
        let rim = exp(-pow((mask - thr) / 0.05, 2.0));
        let floor_violet = vec3(0.07, 0.02, 0.14);
        return vec4(mix(pat * 0.8, pat * 1.3, lit) + floor_violet + vec3(1.0, 0.35, 0.8) * rim * 0.35, 1.0);
    }
    if (style == 2u) {
        // Pulse: the picture breathes (4 s).
        return vec4(pat * (0.55 + 0.7 * (0.5 + 0.5 * sin(t * tau / 4.0))), 1.0);
    }
    if (style == 3u) {
        // Sweep: a bright band glides over it (5 s a pass).
        let d = fract((uv.x * 0.7 + uv.y * 0.45) * 1.5 - t / 5.0);
        let q = (d - 0.5) * 7.0;
        let band = exp(-q * q);
        return vec4(pat * (0.8 + 0.2 * band) + vec3(1.0, 0.95, 0.8) * band * (0.35 + 0.5 * lum), 1.0);
    }
    if (style == 4u) {
        // Plasma: swirling violet, cyan and magenta (Dark Matter's look),
        // the picture's light and dark modulating it.
        let a = sin(p.x * 0.11 + t * tau / 6.0) + sin(p.y * 0.13 - t * tau / 9.0) + sin((p.x + p.z) * 0.07 + t * tau / 12.0);
        let b = sin(p.z * 0.17 + a * 1.3 + t * tau / 8.0);
        let k = 0.5 + 0.5 * sin(a + b * 1.7);
        let swirl = mix(mix(vec3(0.18, 0.02, 0.45), vec3(0.05, 0.6, 0.95), k), vec3(0.95, 0.15, 0.75), saturate(b * 0.5 + 0.25));
        return vec4(swirl * (0.35 + 1.1 * lum) * (0.8 + 0.4 * k), 1.0);
    }
    if (style == 5u) {
        // Gold: brushed gold with a gliding highlight (6 s).
        let d = fract((uv.x * 0.55 + uv.y * 0.8) * 1.2 - t / 6.0);
        let q = (d - 0.5) * 8.0;
        let band = exp(-q * q);
        let gold = vec3(0.86, 0.62, 0.2) * (0.6 + 0.5 * lum);
        return vec4(gold + vec3(1.0, 0.9, 0.55) * band * 0.7, 1.0);
    }
    if (style == 6u) {
        // Ghost: white, see-through, a little more solid at the rim of
        // each surface's uv-flicker (3 s).
        return vec4(vec3(0.92, 0.95, 1.0), 0.42 + 0.08 * sin(t * tau / 3.0));
    }
    return vec4(pat, 1.0);
}

fn shade_prop(in: PropOut) -> vec4<f32> {
#ifdef GHOST
    // bo2zm: Black Ops II's ghost shader (`sw4_3d_char_ghost2`, from the
    // fxc disassembly; Mob's Afterlife ghost hands), with the material's
    // own numbers: two edge noises drifting over the surface (the second
    // turned, here the colour map's again), their product shaping a rim
    // (facing the eye) and a hollow (edge-on) term; the alpha is the
    // smaller of the two times Overall_Intensity, square-rooted. The colour
    // is (Inside_Color + Inside_Add) times the vertex colour, squared,
    // times the light, times Overall_Intensity: a bright cyan glow.
    let t = t6_dyn.count.y;
    let to_eye = normalize(view.world_position - in.world_position);
    let ndv = saturate(dot(normalize(in.normal), to_eye));
    let e2 = raw_texel(textureSample(colour_map, colour_sampler, in.uv * 5.0 + fract(t * vec2(-0.03, 0.01) * 5.0)).rgb).x;
    let d = in.uv - vec2(0.5);
    let turned = vec2(dot(d, vec2(0.982287, -0.187382)), dot(d, vec2(0.187382, 0.982287))) + vec2(0.5);
    let e3 = raw_texel(textureSample(colour_map, colour_sampler, turned * 0.5 + fract(t * vec2(0.05, 0.05) * 0.5)).rgb).x;
    let e = e2 * e3;
    let rim_t = saturate((ndv + 0.02) / 0.73);
    let hollow_t = saturate((ndv - 0.89) / -1.21);
    let rim = rim_t * rim_t * (3.0 - 2.0 * rim_t) * saturate(e * 0.3);
    let hollow = hollow_t * hollow_t * (3.0 - 2.0 * hollow_t) * (e * 1.09);
    let ghost_a = sqrt(max(min(rim, hollow) * 3.99, 0.0));
    let ghost_exposure = max(t6.sun_dir_exposure.w, 0.000001);
    var ghost_light = in.light.rgb / ghost_exposure;
    var ghost_tint = in.color.rgb;
    if (in.light.w > 0.0) {
        ghost_light = in.color.rgb * in.color.rgb * 32.0;
        ghost_tint = vec3(1.0);
    }
    let inside = vec3(0.0, 3.75, 5.0) + vec3(0.5);
    let ghost_c = inside * ghost_tint;
    return vec4(bo2_out(ghost_c * ghost_c * ghost_light * 3.99), saturate(ghost_a));
#else
#ifdef OBJECTIVE
    // bo2zm M3: Black Ops II's objective shader (`mc_objective`, from the
    // fxc disassembly; the magic box's question marks): the colour pulses
    // between colorObjMin and colorObjMax once a second (gameTime),
    // shifted across the surface by how it faces the eye, times one half
    // (its colour map is black), added on (its blend is one, one). The
    // colour map here holds the two colours (texels 0 and 1).
    let lo = textureSampleLevel(colour_map, colour_sampler, vec2(0.25, 0.5), 0.0).rgb;
    let hi = textureSampleLevel(colour_map, colour_sampler, vec2(0.75, 0.5), 0.0).rgb;
    let to_eye = normalize(view.world_position - in.world_position);
    let nz = -dot(normalize(in.normal), to_eye);
    let pulse = 0.5 - 0.5 * sin((nz * -0.5 + t6_dyn.count.y) * 6.28318);
    return vec4(bo2_out(mix(lo, hi, pulse) * 0.5), 1.0);
#else
#ifdef DECAL
    // bo2zm M4 retest 5: Black Ops II's projected decal (`mc_projecteddecal`,
    // from the fxc disassembly; the blood a hit paints on a zombie): the
    // scene behind the box's pixel, from the depth, into the box (-1..1);
    // outside it nothing is drawn. Inside, the texture projected along the
    // box's x axis (at y, z), its alpha fading to the box's x faces, its
    // colour the texture squared times the light grid's light, no fog.
    let scene_d = textureLoad(soft_depth, vec2<i32>(in.clip_position.xy), 0);
    if (scene_d <= 0.0) {
        discard;
    }
    let suv = (in.clip_position.xy - view.viewport.xy) / view.viewport.zw;
    let scene_clip = vec4(suv.x * 2.0 - 1.0, 1.0 - suv.y * 2.0, scene_d, 1.0);
    let scene_h = view.world_from_clip * scene_clip;
    let box_from_world = mat4x4<f32>(in.box0, in.box1, in.box2, in.box3);
    let b = (box_from_world * vec4(scene_h.xyz / scene_h.w, 1.0)).xyz;
    if (any(abs(b) > vec3(1.0))) {
        discard;
    }
    let decal = textureSampleLevel(colour_map, colour_sampler, b.yz * 0.5 + 0.5, 0.0);
    let dtex = raw_texel(decal.rgb);
    let decal_exposure = max(t6.sun_dir_exposure.w, 0.000001);
    return vec4(bo2_out(dtex * dtex * in.light.rgb / decal_exposure), (1.0 - abs(b.x)) * decal.a);
#else
#ifdef SCROLL
    // bo2zm 27c: BO2's scrolling cloud sheets (`mc_sw4_3d_scrolling_zfeather`,
    // from the fxc disassembly): the texcoord times ColorScale.x (instance
    // light.w), drifting by the game time times ColorSpeed (light.yz) times
    // ColorScale.x.
    var scroll_uv = in.uv * in.light.w + fract(t6_dyn.count.y * in.light.yz * in.light.w);
    // bo2mp: a Black Ops III flipbook screen (light.w negative: its rows;
    // light.y its columns, light.z its frames a second): the frame the
    // time is on, of the grid read left to right, top to bottom.
    if (in.light.w < 0.0) {
        let rows = max(-in.light.w, 1.0);
        let cols = max(in.light.y, 1.0);
        let frames = rows * cols;
        let f = floor(t6_dyn.count.y * in.light.z);
        let k = f - frames * floor(f / frames);
        let col = k - cols * floor(k / cols);
        let row = floor(k / cols);
        scroll_uv = (fract(in.uv) + vec2(col, row)) / vec2(cols, rows);
    }
    let albedo = textureSample(colour_map, colour_sampler, scroll_uv);
#else
    let albedo = textureSample(colour_map, colour_sampler, in.uv);
#endif
#ifdef ALPHA_TEST
    // The game's alpha-tested shaders discard below 128/255.
    if (albedo.a < 0.501961) {
        discard;
    }
#endif
    let texel = raw_texel(albedo.rgb);
#ifdef EFFECT
    // bo2zm: Black Ops II effect shaders (`effect_*`): vertex colour times
    // texture, written as is. Their vertex shaders fade the alpha by the
    // view depth times the material's `featherParms.x` (instance slot 22),
    // so a sprite fades out as it nears the camera.
    let feather = in.primary.z;
    let near = select(
        1.0,
        saturate(distance(in.world_position, view.world_position) * feather),
        feather > 0.0,
    );
    // bo2zm M4 retest 5: the `_ds` (dissolve) ones, from the fxc
    // disassembly: alpha = saturate(alphaDissolveParms.x * (texture alpha +
    // vertex alpha - 1)) (minus instance slot 20, which is otherwise a
    // primary light, 0 or more), so the texture's faint floor never shows
    // and the sprite dissolves as it fades.
    let dissolve = -min(in.primary.x, 0.0);
    var a = albedo.a * in.color.a * near;
    if (dissolve > 0.0) {
        a = saturate(dissolve * (albedo.a + in.color.a * near - 1.0));
    }
    // Nothing to blend where the sprite is clear (most of a puff's corners,
    // a near-faded sprite): stop before the depth read and the blend.
    if (a < 0.002) {
        discard;
    }
#ifdef SOFT
    // BO2's zfeather pixel shaders: the alpha fades as the sprite nears the
    // scene behind it, (scene depth - sprite depth) * featherParms.x.
    if (feather > 0.0) {
        a *= soft_fade(in.clip_position, in.world_position, feather);
        if (a < 0.002) {
            discard;
        }
    }
#endif
    // bo2zm M3: BO2's falloff effect shaders (`falloffParms`, from the fxc
    // disassembly): the colour times f * lerp(end, begin, f), f =
    // saturate(dot(eye ray, normal)^2 * parms.z + parms.w): a sprite (a
    // flame sheet, a zombie's eye beam) fades as it turns edge-on to the eye.
    // Instance slots 16..19 (see the extraction).
    var tint = in.color.rgb;
    if (in.light.w > 0.5) {
        let ray = normalize(in.world_position - view.world_position);
        let d = dot(ray, normalize(in.normal));
        let f = saturate(d * d * in.light.x + in.light.y);
        tint = tint * f * mix(vec3(in.light.z), vec3(1.0), f);
    }
#ifdef EFFECT_ADD
#ifdef EFFECT_SCREEN
    // A screen-blended one (INV_DEST_COLOR / ONE) takes the colour already
    // times its alpha: it lights only what is dark behind it.
    return vec4(bo2_added(texel * tint) * a, a);
#else
    // Additive ones take no fog (it would add the fog colour over the whole
    // quad); the blend scales the colour by the alpha.
    return vec4(bo2_added(texel * tint), a);
#endif
#else
#ifdef EFFECT_MULTIPLY
    return vec4(mix(vec3(1.0), texel * tint, a), a);
#else
    // Blended ones go toward the effect fog by its keep factor.
    let fx_fog = effect_fog(in.world_position);
    let fx = mix(fx_fog.rgb, texel * tint, fx_fog.w);
    return vec4(bo2_final(fx), a);
#endif
#endif
#else
#ifdef UNLIT
#ifdef MULTIPLY
    let a = albedo.a * in.color.a;
    return vec4(mix(vec3(1.0), texel * in.color.rgb, a), a);
#else
#ifdef SCROLL
    // A cloud sheet's colour is the texture times hdrAmount, squared
    // (instance primary.x holds hdrAmount squared), no vertex colour; the
    // vertex alpha fades it, and so does how near the scene behind it lies
    // (1 / FadeFactor, instance light.x).
    var sheet_a = albedo.a * in.color.a;
#ifdef SOFT
    sheet_a *= soft_fade(in.clip_position, in.world_position, in.light.x);
#endif
    if (sheet_a < 0.002) {
        discard;
    }
    return vec4(
        unlit_out(unlit_fog(texel * texel * in.primary.x, in.world_position)),
        sheet_a,
    );
#else
    // bo2zm round 1: a zombie's eyes (`unlit_cheap_zombie_eyes`): BO2's
    // shader scales the texture by the scriptVector2.y its client script set
    // (2 alive, Origins 3, 0.25 once the eyes go out) before squaring it;
    // it rides negated in instance slot 22, which no other unlit row makes
    // negative.
    var c = texel * in.color.rgb;
    if (in.primary.z < 0.0) {
        c *= -in.primary.z;
    }
    return vec4(
        unlit_out(unlit_fog(c * c * f32(#{UNLIT_SCALE}), in.world_position)),
        albedo.a * in.color.a,
    );
#endif
#endif
#else
    // The grid light is already times the exposure; the BO2 output adds it
    // again, so divide it out here.
    let exposure = max(t6.sun_dir_exposure.w, 0.000001);
    var light = in.light.rgb / exposure;
#ifdef FOLIAGE
    // bo2mp: Black Ops II's foliage (`mc_treecanopy`, fxc disassembly): the
    // vertex colour's rgb picks the wind variant and its alpha scales the
    // grid light; the leaves are their colour map alone.
    light *= in.color.a;
    let tinted = texel;
#else
#ifdef CLOTH
    // bo2mp: a flag in the wind (`mc_lit_sm_flag`): the vertex colour is its
    // sway weight (0 at the pole), not a tint; tinting by it drew mp_downhill's
    // teardrop flags black from the pole out.
    let tinted = texel;
#else
    let tinted = texel * in.color.rgb;
#endif
#endif
    var base = tinted * tinted;
    if (in.light.w > 0.0) {
        light = in.color.rgb * in.color.rgb * 32.0;
        base = texel * texel;
    }
#ifdef SHINE
    // bo2mp: a pattern camo (instance slot 22 = 3000 + its repeat), as Black
    // Ops II's gun shader lays it (fxc disassembly of `mc_lit_sm_r0c0n0s0o0`'s
    // light-probe pass): the pattern at (uv - 0.5) * repeat + 0.5, under the
    // gun's colour by the colour map's alpha (alpha 0: all pattern). The
    // pattern rides in the specular map's slot here; the camo areas shine as
    // painted metal. (Sampled either way: texture reads stay in uniform
    // control flow.)
    // (5000 + its repeat: the pattern over the whole surface, another game's
    // gun, shaded by the gun's own colour.)
    // (A special camo adds 10000 * its style code: `weapon_camos::
    // EffectCamo::code`, a style 1-31 plus 32 * its speed step; 0 for the
    // plain camos.)
    let camo_code = max(floor(in.primary.z / 10000.0), 0.0);
    let camo_z = in.primary.z - camo_code * 10000.0;
    let camo_style = u32(camo_code) % 32u;
    let camo_pace = exp2(f32(u32(camo_code) / 32u) - 1.0);
    let camo_time = t6_dyn.count.y * camo_pace;
    let camo_on = camo_z >= 3000.0;
    let camo_full = camo_z >= 5000.0;
    let camo_repeat = camo_z - select(3000.0, 5000.0, camo_full);
    // (Scroll: the picture slides one whole repeat every 12 s: seamless.)
    // (A camo pack's scroll, styles 8-15: toward compass step style - 8,
    // counter-clockwise from the right in 45 degree steps, uv's v running
    // down; the picture slides one whole repeat per 12 s at pace 1.)
    let camo_slide = fract(camo_time / 12.0);
    let camo_ang = (f32(camo_style) - 8.0) * 0.7853982;
    let camo_dir = vec2(round(cos(camo_ang)), -round(sin(camo_ang)));
    let camo_drift = select(
        select(vec2(0.0), -camo_dir * camo_slide, camo_style >= 8u && camo_style < 16u),
        vec2(1.0, -1.0) * camo_slide,
        camo_style == 1u,
    );
    let camo_uv = (in.uv - vec2(0.5)) * max(camo_repeat, 0.0001) + vec2(0.5) + camo_drift;
    // (A camo pack's frames, style 7: the picture is a filmstrip of square
    // frames stacked top to bottom (`camo_pack::filmstrip`), so the frame
    // count is the picture's height over its width. One frame shows at a
    // time, 6 * 2^speed per second, wrapped inside its own cell; the
    // gradients come from the unwrapped uv so the mip level has no seam.)
    let strip_dim = vec2<f32>(textureDimensions(specular_map));
    let strip_n = max(floor(strip_dim.y / max(strip_dim.x, 1.0) + 0.5), 1.0);
    let strip_on = camo_style == 7u && strip_n > 1.5;
    let strip_frame = floor(fract(camo_time * 12.0 / strip_n) * strip_n);
    let strip_wrap = fract(camo_uv);
    let strip_half = 0.5 / max(strip_dim.y, 1.0);
    let strip_uv = vec2(
        strip_wrap.x,
        clamp((strip_wrap.y + strip_frame) / strip_n, strip_frame / strip_n + strip_half, (strip_frame + 1.0) / strip_n - strip_half),
    );
    let strip_scale = vec2(1.0, 1.0 / strip_n);
    let camo_flat = textureSample(specular_map, colour_sampler, camo_uv);
    let camo_cell = textureSampleGrad(specular_map, colour_sampler, strip_uv, dpdx(camo_uv) * strip_scale, dpdy(camo_uv) * strip_scale);
    let camo_pat = select(camo_flat, camo_cell, strip_on);
    // (Layers, style 17: Black Ops III's `lit_emissive_scroll_3layer_plus`
    // as Dark Matter sets it, read from core_common: the emissive picture
    // three times, scaled (1, 0.8), (1, 2), (1, 1) and sliding (0.02, 0.02),
    // (0.06, -0.07), (0, -0.03) a second, tinted red, blue and violet, the
    // glow flickering at 0.228 Hz. Gradient reads, so they may sit in a
    // branch.)
    let lay_dx = dpdx(camo_uv);
    let lay_dy = dpdy(camo_uv);
    var lay_glow = vec3(0.0);
    if (camo_style == 17u) {
        let l1 = textureSampleGrad(specular_map, colour_sampler, camo_uv * vec2(1.0, 0.8) + camo_time * vec2(0.02, 0.02), lay_dx, lay_dy).rgb;
        let l2 = textureSampleGrad(specular_map, colour_sampler, camo_uv * vec2(1.0, 2.0) + camo_time * vec2(0.06, -0.07), lay_dx, lay_dy).rgb;
        let l3 = textureSampleGrad(specular_map, colour_sampler, camo_uv + camo_time * vec2(0.0, -0.03), lay_dx, lay_dy).rgb;
        let w1 = dot(raw_texel(l1), vec3(0.299, 0.587, 0.114));
        let w2 = dot(raw_texel(l2), vec3(0.299, 0.587, 0.114));
        let w3 = dot(raw_texel(l3), vec3(0.299, 0.587, 0.114));
        // (The game's tints, red 1 / 0.006 / 0.004, blue 0.002 / 0.007 /
        // 0.06, violet 0.04 / 0.003 / 0.16, square-rooted: its HDR scale
        // lifts the dim ones.)
        let flick = 0.8 + 0.2 * sin(camo_time * 6.2831853 * 0.228);
        lay_glow = (w1 * vec3(1.0, 0.08, 0.063) + w2 * 1.4 * vec3(0.043, 0.087, 0.244) + w3 * 1.4 * vec3(0.207, 0.058, 0.402)) * flick;
    }
    if (camo_style == 18u) {
        // Ritual (the zod Pack-a-Punch camo; the same shader, read from
        // core_common): two layers of the glow picture in its own colour,
        // sliding (0.02, 0.01) and (0.01, -0.01) a second, the second
        // zoomed (0.2, 0.4), tinted a dim grey (0.41), flickering at 1 Hz.
        let r1 = raw_texel(textureSampleGrad(specular_map, colour_sampler, camo_uv + camo_time * vec2(0.02, 0.01), lay_dx, lay_dy).rgb);
        let r2 = raw_texel(textureSampleGrad(specular_map, colour_sampler, camo_uv * vec2(1.0, 1.0) * 0.7 + camo_time * vec2(0.01, -0.01), lay_dx, lay_dy).rgb);
        let flick = 0.85 + 0.15 * sin(camo_time * 6.2831853);
        lay_glow = (r1 + r2 * 0.8) * 0.9 * flick;
    }
    if (camo_style == 19u) {
        // Diamond (read from core_common): the glow picture is a 4 x 4
        // flipbook, a frame every 2 s, laid 6 x 6 times over the gun and
        // sliding (0.02, -0.7) a second, flickering at 0.3 Hz.
        let cell = floor(fract(camo_time / 32.0) * 16.0);
        let corner = vec2(cell % 4.0, floor(cell / 4.0));
        let tiled = in.uv * 6.0 + camo_time * vec2(0.02, -0.7);
        let cell_uv = (corner + fract(tiled)) / 4.0;
        let sp = raw_texel(textureSampleGrad(specular_map, colour_sampler, cell_uv, dpdx(in.uv) * 1.5, dpdy(in.uv) * 1.5).rgb);
        lay_glow = sp * (0.85 + 0.15 * sin(camo_time * 6.2831853 * 0.3));
    }
    // bo2mp: a guest gun's own light (Infinite Warfare's glowing sights and
    // tritium vials: its emissive map times `emissiveParams.x`, linear light
    // in the auxiliary map's place, in this draw's light units): added after
    // the lighting and the fog, before the exposure and tone. The gun's
    // extract marks its rows with a sixteenth in the fraction of
    // primary.w (a quarter is a foreign gun's): the tile and flow props'
    // auxiliary maps hold other things. (Sampled either way, as above.)
    let glow_tex = textureSample(aux_map, colour_sampler, in.uv).rgb;
    let glow_on = (gun_flags(in.primary.w) & 1u) != 0u;
    var gun_glow = select(vec3(0.0), max(glow_tex, vec3(0.0)), glow_on);
    var camo_alpha = 1.0;
    if (camo_on) {
        let shade = clamp(dot(texel, vec3(0.299, 0.587, 0.114)) * 2.2, 0.35, 1.25);
        var pat = raw_texel(camo_pat.rgb);
        let live = camo_animate(camo_style, camo_time, in.uv, in.world_position, pat, camo_pat.a);
        pat = live.rgb;
        camo_alpha = live.a;
        // (A reveal camo glows by itself, as Dark Matter does: its own light,
        // linear, added after the lighting like a gun's emissive map, so a
        // dark picture is not washed out by a bright sky or the sun.)
        if (camo_style == 16u) {
            gun_glow = gun_glow + live.rgb * live.rgb * 0.7;
        }
        // (Layers: the gun body stays dark, the three layers are its own light.)
        if (camo_style == 17u) {
            pat = vec3(0.02, 0.01, 0.03);
            gun_glow = gun_glow + lay_glow * 1.2;
        }
        if (camo_style == 18u) {
            pat = vec3(0.04, 0.05, 0.04);
            gun_glow = gun_glow + lay_glow * 0.9;
        }
        if (camo_style == 19u) {
            // (Diamond: a silver-blue body, the flipbook glittering on it.)
            pat = vec3(0.48, 0.53, 0.62) + lay_glow * 0.8;
            gun_glow = gun_glow + lay_glow * 0.7;
        }
        let camo_mixed = select(mix(pat, texel, albedo.a), pat * shade, camo_full) * in.color.rgb;
        base = camo_mixed * camo_mixed;
    }
    // bo2zm: Black Ops II's lit model shader (`mc_lit_sm_r0c0n0s0`, its
    // light-probe technique, from the fxc disassembly): as the world's, the
    // light here from the light grid. A model's vertices carry no tangent
    // frame here, so it comes from the screen-space derivatives of position
    // and texcoord (a cotangent frame).
    let vn = normalize(in.normal);
    let dp1 = dpdx(in.world_position);
    let dp2 = dpdy(in.world_position);
    let duv1 = dpdx(in.uv);
    let duv2 = dpdy(in.uv);
    let dp2perp = cross(dp2, vn);
    let dp1perp = cross(vn, dp1);
    let tg = dp2perp * duv1.x + dp1perp * duv2.x;
    let bt = dp2perp * duv1.y + dp1perp * duv2.y;
    let frame = inverseSqrt(max(max(dot(tg, tg), dot(bt, bt)), 1e-20));
    // bo2mp: Black Ops III's own light (`t7_shade`).
    if (t7_drawn()) {
        let nm7 = textureSample(normal_map, colour_sampler, in.uv).xy * 4.015748 - 2.015748;
        let n7 = normalize(nm7.x * tg * frame + nm7.y * bt * frame + vn);
        // (A camo's pattern rides in the specular map's place.)
        let st7 = select(textureSample(specular_map, colour_sampler, in.uv), vec4(0.24, 0.24, 0.24, 0.45), camo_on);
        let pr7 = select(u32(max(in.primary.w - 0.5, 0.0)), 0xffffu, (gun_flags(in.primary.w) & 4u) != 0u && in.primary.w < 0.99);
        // (A guest gun's viewmodel rides a quarter in primary.w's fraction:
        // `extract`'s instance row; its diffuse is not cut by 1 - specular.)
        let foreign_gun = (gun_flags(in.primary.w) & 4u) != 0u;
        let vm_gun = (gun_flags(in.primary.w) & 2u) != 0u;
        // The first-person gun and arms (within arm's reach of the eye) take
        // the sun's visibility at the eye: a hand that reaches into a baked
        // shadow's casters, or a wall's, would otherwise go black while the
        // world round it is lit.
        let vis_at = select(in.world_position, view.world_position, distance(in.world_position, view.world_position) < 40.0);
        if (IW6_HDR_ON) {
            // (Below, after the probe's reflection.)
        } else {
        let lit7 = t7_fog(t7_shade(base, light, vn, n7, st7, pr7, in.world_position, vis_at, !foreign_gun, vm_gun, u32(max(in.primary.x + 0.5, 0.0)), in.primary.y), in.world_position);
        return vec4(t7_out(bo2_fog(lit7, in.world_position) + gun_glow), select(albedo.a * in.color.a, camo_alpha, camo_on && camo_style == 6u));
        }
    }
#ifdef FLOW
    // bo2mp: a lava prop (`mc_cod7emissiveflow`), as the world's.
    return vec4(bo2_out(bo2_flow(in.uv, in.color, vn, tg * frame, bt * frame, in.world_position)), 1.0);
#endif
#ifdef TILE
    // bo2mp: Black Ops II's emissive tile (`sw4_3d_cod7_emissive_tile`,
    // Magma's lava rocks; its lit technique, fxc disassembly): the colour
    // map tiled by Micro_Scale times the vertex colour, squared, lit (here
    // the grid light, the primary light by the macro normal map times
    // Macro_Height; the micro normal map is not drawn), plus the glow
    // squared: (emissive A + emissive B) * A's alpha * hdrAmount, combined
    // by the assets lane in the specular map's place. Auxiliary texel 0:
    // Micro_Scale, Micro_Height, Macro_Height.
    let tc = textureLoad(aux_map, vec2<i32>(0, 0), 0);
    let tcol = textureSample(colour_map, colour_sampler, in.uv * tc.xy);
    let ttint = select(in.color.rgb, vec3(1.0), in.light.w > 0.0);
    let tbase = raw_texel(tcol.rgb) * ttint;
    let tnm = (textureSample(normal_map, colour_sampler, in.uv).xy * 4.015748 - 2.015748) * tc.w;
    let tn = normalize(tnm.x * tg * frame + tnm.y * bt * frame + vn);
    let tray = primary_light_ray(u32(in.primary.x + 0.5), in.world_position);
    var tlight = light + tray.colour * in.primary.y * saturate(dot(tn, tray.dir));
    tlight += dyn_lights(in.world_position, tn);
    let tglow = textureSample(specular_map, colour_sampler, in.uv).rgb;
    return vec4(
        bo2_out(bo2_fog(tbase * tbase * tlight + tglow * tglow, in.world_position)),
        1.0,
    );
#endif
#ifdef TILE_BLEND
    // bo2mp: Black Ops II's tile blend (`sw4_3d_cod7_tile_blend`, `_edge`,
    // `_spec`; cliffs, rocks, vista mountains; lit technique, fxc
    // disassembly): Micro_1 (colour map, at uv * Micro_1_Scale) and Micro_2
    // (specular map's place, at uv * Micro_2_Scale) mixed by the macro
    // map's green squared (normal map's place), times 1 + blue^2 *
    // (EdgeHighlight - 1) (the edge variant; 1 elsewhere), times the vertex
    // colour, squared; the ambient light times saturate(red^2 +
    // AO_Diffuse_Adj); the primary light by the vertex normal (the macro
    // and micro normal maps and the spec variant's specular are not drawn).
    let bc0 = textureLoad(aux_map, vec2<i32>(0, 0), 0);
    let bc1 = textureLoad(aux_map, vec2<i32>(1, 0), 0);
    let bm = raw_texel(textureSample(normal_map, colour_sampler, in.uv).rgb);
    let b1 = raw_texel(textureSample(colour_map, colour_sampler, in.uv * bc0.xy).rgb);
    let b2 = raw_texel(textureSample(specular_map, colour_sampler, in.uv * bc0.zw).rgb);
    let bedge = 1.0 + bm.z * bm.z * (bc1.y - 1.0);
    let btint = select(in.color.rgb, vec3(1.0), in.light.w > 0.0);
    let bcol = saturate(mix(b1, b2, bm.y * bm.y) * bedge) * btint;
    let bao = saturate(bm.x * bm.x + bc1.x);
    let bray = primary_light_ray(u32(in.primary.x + 0.5), in.world_position);
    var blight = light * bao + bray.colour * in.primary.y * saturate(dot(vn, bray.dir));
    blight += dyn_lights(in.world_position, vn);
    return vec4(bo2_out(bo2_fog(bcol * bcol * blight, in.world_position)), 1.0);
#endif
#ifdef FLAG
    // bo2mp: Black Ops II's tattered flag (`sw4_3d_phong_simple_flag_tatters`,
    // Dig's tarps; its lit technique, fxc disassembly): the frayed edge
    // texture (specular map's place, at uv * EdgeScale, auxiliary texel 0)
    // mixed into the diffuse by the vertex colour's red, squared; alpha
    // mixes the edge's alpha toward 1 by saturate(2 * red), tested at
    // 128/255. Lit by the grid and the primary light (its gloss, detail
    // normal and wind are not drawn).
    let gc = textureLoad(aux_map, vec2<i32>(0, 0), 0);
    let gedge = textureSample(specular_map, colour_sampler, in.uv * gc.xy);
    let gw = select(in.color.r, 1.0, in.light.w > 0.0);
    let ga = mix(gedge.a, 1.0, saturate(2.0 * gw));
    if (ga < 0.501961) {
        discard;
    }
    let gcol = mix(raw_texel(gedge.rgb), texel, gw);
    let gray = primary_light_ray(u32(in.primary.x + 0.5), in.world_position);
    var glight = light + gray.colour * in.primary.y * saturate(dot(vn, gray.dir));
    glight += dyn_lights(in.world_position, vn);
    return vec4(bo2_out(bo2_fog(gcol * gcol * glight, in.world_position)), 1.0);
#endif
    let nm = textureSample(normal_map, colour_sampler, in.uv).xy * 4.015748 - 2.015748;
    let n = normalize(nm.x * tg * frame + nm.y * bt * frame + vn);
    let spec_tex = select(
        textureSample(specular_map, colour_sampler, in.uv),
        vec4(0.24, 0.24, 0.24, 0.45),
        camo_on,
    );
    let spec_raw = raw_texel(spec_tex.rgb);
    let spec = spec_raw * spec_raw;
    let gloss = spec_tex.a;
    let vdir = normalize(in.world_position - view.world_position);
    // The reflection, when the model has a probe (instance slot 23, plus
    // one): scaled by the light here over the light the probe saw.
    let ndotv = saturate(dot(n, -vdir));
    let fk = gloss * vec4(1.041667, 0.475, 0.018229, 0.25) + vec4(0.0, 0.0, -0.015625, 0.75);
    var f = min(exp2(-9.28 * ndotv), fk.y);
    f = fk.x * f + fk.z;
    let env_weight = saturate(spec * (fk.w - f) + f);
    let r = vdir - 2.0 * dot(vdir, n) * n;
    let probe_plus = u32(in.primary.w + 0.5);
    let probe = select(0u, probe_plus - 1u, probe_plus > 0u);
    let env = textureSampleLevel(probe_cubes, probe_sampler, r, min(i32(probe), i32(textureNumLayers(probe_cubes)) - 1), 4.0 - 4.0 * gloss);
    // bo2mp: a CoD4 / World at War map's own probes (sun colour alpha 4)
    // are lit already (gamma, no lighting table): squared, not relit.
    let own_probes = t6.sun_color.a > 3.5 && t6.sun_color.a < 4.5;
    let env_rgb = select(env.rgb / (env.a + 0.000001), env.rgb * env.rgb, own_probes);
    let pi = min(probe, 31u) * 3u;
    let s0 = probes.v[pi];
    let s1 = probes.v[pi + 1u];
    let s2 = probes.v[pi + 2u];
    let sh = dot(s2, vec4(vn.z * vn.x, vn.y * vn.z, vn.x * vn.y, vn.x * vn.x - vn.y * vn.y))
        + dot(s1, vec4(vn, 1.0)) + s0.w * vn.z * vn.z;
    let probe_light = max(sh * s0.xyz, vec3(0.1));
    let relit = select(light / probe_light, vec3(1.0), own_probes);
    let reflection = select(vec3(0.0), env_weight * env_rgb * relit, probe_plus > 0u);
#ifdef IW6_HDR
    {
        // (The first-person gun and arms take the sun's visibility at the
        // eye, as above.)
        let pv6 = select(in.world_position, view.world_position, distance(in.world_position, view.world_position) < 40.0);
        let refl6 = select(vec3(0.0), env_rgb * relit, probe_plus > 0u);
        let amb6 = light + dyn_lights(in.world_position, n);
        let c6 = iw6_lit(base, amb6, vn, n, spec_tex, refl6, in.world_position, pv6, true);
        return vec4(c6 + gun_glow * iw6_light_scale(), select(albedo.a * in.color.a, camo_alpha, camo_on && camo_style == 6u));
    }
#endif
    // The primary light: diffuse and its highlight.
    let ray = primary_light_ray(u32(in.primary.x + 0.5), in.world_position);
    // (bo2mp: a CoD4 / World at War sun is a gamma colour: its highlight
    // takes it squared.)
    let lit_g = ray.colour * in.primary.y;
    let lit = select(lit_g, lit_g * lit_g, t6.sun_color.a > 2.5 && t6.sun_color.a < 4.5);
    let ndotl = saturate(dot(n, ray.dir));
    light = add_light(light, lit_g, ndotl);
    light += dyn_lights(in.world_position, n);
    let h = normalize(ray.dir - vdir);
    let ndoth = saturate(dot(n, h));
    let ldoth = saturate(dot(h, ray.dir));
    let power = exp2(gloss * 13.0);
    let lobe = exp2(log2(max(ndoth, 0.000001)) * power) * (power * 0.125 + 0.25);
    let k = saturate(gloss + 0.545);
    let vis = ndotl / (ldoth * ldoth * k - k + 1.01);
    let fres = spec + (1.0 - spec) * exp2(-10.0 * ldoth);
    let highlight = fres * lobe * vis * lit;
    // The vertex alpha fades a blended model surface (a decal's painted
    // patches); opaque surfaces ignore the alpha.
    return vec4(
        bo2_out(bo2_fog(base * light + reflection + highlight, in.world_position) + gun_glow),
        select(albedo.a * in.color.a, camo_alpha, camo_on && camo_style == 6u),
    );
#else
#ifdef IW6_HDR
    {
        let vn6 = normalize(in.normal);
        let pv6 = select(in.world_position, view.world_position, distance(in.world_position, view.world_position) < 40.0);
        let c6 = iw6_lit(base, light + dyn_lights(in.world_position, vn6), vn6, vn6, vec4(0.0), vec3(0.0), in.world_position, pv6, false);
        return vec4(c6, albedo.a * in.color.a);
    }
#endif
    // bo2mp: Black Ops III's own light (`t7_shade`).
    if (t7_drawn()) {
        let vn7 = normalize(in.normal);
        let lit7 = t7_fog(t7_shade(base, light, vn7, vn7, vec4(0.0), 0u, in.world_position, in.world_position, true, false, u32(max(in.primary.x + 0.5, 0.0)), in.primary.y), in.world_position);
        return vec4(t7_out(bo2_fog(lit7, in.world_position)), albedo.a * in.color.a);
    }
    let n = normalize(in.normal);
    let pray = primary_light_ray(u32(in.primary.x + 0.5), in.world_position);
    light = add_light(light, pray.colour * in.primary.y, saturate(dot(n, pray.dir)));
    light += dyn_lights(in.world_position, n);
    return vec4(bo2_out(bo2_fog(base * light, in.world_position)), albedo.a * in.color.a);
#endif
#endif
#endif
#endif
#endif
#endif
}

// bo2zm: the Black Ops II sky: a full-screen triangle at the far plane
// (reverse-Z: depth 0, drawn only where nothing else is), sampling the
// sky's cube map by view direction as the game's skycubemaphdr shader does
// (world x, y, z; rgb / a), times its brightness, then the BO2 output (times
// the exposure, square-rooted). bo2mp: its fog as that shader applies it:
// the pixel shader works the fog out again and multiplies it by the vertex
// shader's fog times opacity, so the sky fogs by fog^2 * opacity. Its
// skyColorMultiplier (GfxWorld::skyDynIntensity, factors 1 on both
// Nuketowns) and colour matrix (identity) change nothing.
@group(1) @binding(3) var sky_map: texture_cube<f32>;
@group(1) @binding(4) var sky_sampler: sampler;
@group(1) @binding(5) var<uniform> sky_t6: T6Lighting;
// bo2zm: the light table again, for the sky fog (slots 256, 257).
@group(1) @binding(6) var<uniform> sky_lights: T6Lights;
// bo2zm: the world's colour tables, for the sky's final colour.
@group(1) @binding(12) var sky_lut: texture_2d<f32>;
@group(1) @binding(13) var sky_lut_sampler: sampler;

struct SkyOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vertex_sky(@builtin(vertex_index) index: u32) -> SkyOut {
    let x = f32((index & 1u) * 4u) - 1.0;
    let y = f32((index >> 1u) * 4u) - 1.0;
    var out: SkyOut;
    out.clip_position = vec4(x, y, 0.0, 1.0);
    out.ndc = vec2(x, y);
    return out;
}

fn shade_sky(in: SkyOut) -> vec4<f32> {
    let near = view.world_from_clip * vec4(in.ndc, 1.0, 1.0);
    let dir = normalize(near.xyz / near.w - view.world_position);
    // The game's sky vertex shader turns the lookup about z:
    // x' = x * r.y + y * r.z, y' = x * r.x + y * r.y.
    let r = sky_t6.sky;
    let z = select(dir.z, -dir.z, r.w < 0.0);
    let lookup = vec3(dir.x * r.y + dir.y * r.z, dir.x * r.x + dir.y * r.y, z);
    let texel = textureSample(sky_map, sky_sampler, lookup);
    // bo2zm: its skyColorParm x picks how the cube is stored (above 0: rgb /
    // a, else rgb^2).
    let parms = sky_t6.sky_parms;
    let decoded = select(texel.rgb * texel.rgb, texel.rgb / (texel.a + 0.000001), parms.x > 0.0);
    let sky = decoded * abs(r.w);
    // The game puts the sky 2e7 units out and fogs it there, taking only
    // its skyColorParm w of the fog (1 on most maps, Buried 0.16).
    var fogged = sky;
    if (sky_lights.v[1028u].w >= 0.5) {
        let f = fog_at(
            view.world_position + dir * 20000000.0,
            sky_lights.v[1028u],
            sky_lights.v[1029u],
            sky_lights.v[1030u],
            sky_lights.v[1031u],
            sky_lights.v[1024u],
        );
        fogged = mix(sky, f.tint, f.fog * f.fog * f.opacity * parms.y);
    }
    // bo2mp: a Black Ops III sky (its picture in its probes' unit) through
    // the world's tone curve (`t7_tone`).
    if (sky_t6.sky_parms.z > 0.5) {
        // Its fog (`t7_fog_core`) at the sky's distance.
        var g7: T7Fog;
        for (var i = 0u; i < 8u; i++) {
            g7.f[i] = sky_lights.v[T7_FOG + i];
        }
        g7.m[0] = sky_lights.v[T7_ROW * 4u + 1u];
        g7.m[1] = sky_lights.v[T7_ROW * 4u + 2u];
        g7.m[2] = sky_lights.v[T7_ROW * 4u + 3u];
        g7.m[3] = sky_lights.v[(T7_ROW + 1u) * 4u];
        let j7 = fract(52.982918 * fract(dot(in.clip_position.xy, vec2(0.06711056, 0.00583715))));
        let far7 = view.world_position + dir * max(g7.f[2].w, 1.0);
        var sky7 = t7_fog_core(fogged, far7, view.world_position, true, j7, g7, sky_lut);
        // bo2mp: Carrier's stormy dusk sky (`carrier_look`): the clear sky
        // greyed and darkened overhead, lighter to the horizon, broken
        // clouds that thin where the sun is, and a warm glow round the sun
        // (stronger near the horizon).
        if (sky_lights.v[1004u].w > 0.5) {
            let k2 = sky_lights.v[1006u];
            let k3 = sky_lights.v[1007u];
            let sun_d = sky_t6.sun_dir_exposure.xyz;
            let above = saturate(dir.z);
            let sl = dot(sky7, vec3(0.2126, 0.7152, 0.0722));
            let sd = max(dot(dir, sun_d), 0.0);
            let cp = dir.xy / (above + 0.2) * 1.4;
            var cl = smoothstep(0.38, 0.72, look_fbm(cp) * 0.7 + look_fbm(cp * 3.1 + vec2(9.2, 3.4)) * 0.3);
            cl *= 1.0 - 0.8 * pow(sd, 12.0);
            cl *= smoothstep(-0.02, 0.12, dir.z);
            let storm = k2.w * mix(0.4, 1.0, sqrt(above));
            let grey = k2.rgb * sl * mix(1.0, 0.45, above * k2.w);
            sky7 = mix(sky7, grey, saturate(storm));
            let cloud = k2.rgb * sl * 0.5;
            sky7 = mix(sky7, cloud, saturate(cl * k2.w));
            // The clouds' lit edges toward the sun.
            sky7 += k3.rgb * (cl * (1.0 - cl) * 4.0) * pow(sd, 3.0) * 0.8;
            let low = mix(2.2, 0.6, saturate(dir.z * 2.5));
            sky7 += k3.rgb * pow(sd, max(k3.w, 1.0)) * low * mix(1.0, 0.45, cl);
        }
        let bo2_7 = sky_lights.v[(T7_ROW + 1u) * 4u + 1u].z > 0.5;
        let mwr_auto = select(1.0, sky_t6.sky_parms.w, sky_lights.v[(T7_ROW + 1u) * 4u + 2u].z > 0.5 && sky_t6.sky_parms.w > 0.0);
        // The sky's own exposure gain (row 1's y; 0 or less: 1,
        // `BO2MP_T7_SKY_EXPOSURE`): the world's exposure set against
        // pictures of the game leaves the sky darker than the game's.
        let sky_e = sky_lights.v[(T7_ROW + 1u) * 4u + 1u].y;
        let e7 = sky_lights.v[T7_ROW * 4u].x * select(1.0, sky_t6.sun_dir_exposure.w, bo2_7) * select(1.0, sky_e, sky_e > 0.0) * mwr_auto;
        let mwr7 = sky_lights.v[(T7_ROW + 1u) * 4u + 2u].z > 0.5;
        let t7_x = select(t7_tone(sky7, e7), mwr_tone(sky7, e7), mwr7);
        return vec4(bo2_finish(t7_x, sky_t6, sky_lut, sky_lut_sampler), 1.0);
    }
    let sky_x = bo2_rolloff(sqrt(max(fogged * sky_t6.sun_dir_exposure.w, vec3(0.0))));
    return vec4(bo2_finish(sky_x, sky_t6, sky_lut, sky_lut_sampler), 1.0);
}

// bo2mp: the fragment entry points. Each wraps the shading above and, with
// MOTION, adds the motion vector as a second target.
struct ShadeOut {
    @location(0) colour: vec4<f32>,
#ifdef MOTION
    @location(1) motion: vec2<f32>,
#endif
}

@fragment
fn fragment(in: VertexOut) -> ShadeOut {
    var out: ShadeOut;
    out.colour = shade_fallback(in);
#ifdef MOTION
    out.motion = motion_world(in.world_position);
#endif
    return out;
}

@fragment
fn fragment_textured(in: VertexOut) -> ShadeOut {
    var out: ShadeOut;
#ifdef ANIM_UV
    var moved = in;
    moved.uv = anim_uv(in.uv, in.world_position, in.normal);
    out.colour = shade_textured(moved);
#else
    out.colour = shade_textured(in);
#endif
#ifdef MOTION
    out.motion = motion_world(in.world_position);
#endif
    return out;
}

@fragment
fn fragment_lightmapped(in: VertexOut) -> ShadeOut {
    var out: ShadeOut;
#ifdef ANIM_UV
    var moved = in;
    moved.uv = anim_uv(in.uv, in.world_position, in.normal);
    out.colour = shade_lightmapped(moved);
#else
    out.colour = shade_lightmapped(in);
#endif
#ifdef MOTION
    out.motion = motion_world(in.world_position);
#endif
    return out;
}

@fragment
fn fragment_prop(in: PropOut, @builtin(front_facing) front: bool) -> ShadeOut {
    var out: ShadeOut;
    var facing = in;
#ifdef ALPHA_TEST
    // bo2mp: a two-sided cut-out card (a leaf sheet, a fence) lit by Black
    // Ops III's light: its back face's normal turns to the eye, so the
    // underside of a canopy is lit as a surface and not as its far side
    // (dark: the ambient from behind it, no sun).
    if (!front && t7_drawn()) {
        facing.normal = -in.normal;
    }
#endif
#ifdef ANIM_UV
    var moved = facing;
    moved.uv = anim_uv(in.uv, in.world_position, in.normal);
    out.colour = shade_prop(moved);
#else
    out.colour = shade_prop(facing);
#endif
#ifdef IW6_HDR_OPAQUE
    // (Ghosts' scene alpha is its bloom mask: an opaque model writes none.)
    out.colour.a = 0.0;
#endif
#ifdef MOTION
    out.motion = motion_world(in.world_position);
#endif
    return out;
}

@fragment
fn fragment_sky(in: SkyOut) -> ShadeOut {
    var out: ShadeOut;
    out.colour = shade_sky(in);
#ifdef MOTION
    let near = view.world_from_clip * vec4(in.ndc, 1.0, 1.0);
    out.motion = motion_sky(normalize(near.xyz / near.w - view.world_position));
#endif
    return out;
}
