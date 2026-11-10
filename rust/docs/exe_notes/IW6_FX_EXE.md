# Ghosts (IW6) effects: exe study (2026-11-04)

Partial study (the session was cut short); what is verified is marked. Addresses are from the Ghosts MP exe decompile (static only).

## Draw handlers (verified)
- Oriented sprite handler FUN_140336f80: the quad spans frame axes 1 and 2 (axis 0 is the card normal), no random spawn angles are applied. The earlier note (IW6_VFX_AUDIO C7) said axes 0 and 2; that was wrong. Fixed in `render_fx::present` (`BO2MP_IW6_FX_FRAME`).
- Tail handler FUN_140337210, quad builder FUN_140339030, cloud FUN_1403350e0 / 140335580 / 140335210, effect create FUN_14035ae70, element spawn FUN_14035c150.
- Clouds: same box-template design as IW4/BO2 (cull sphere radius = max(size0,size1)+scale). Ghosts type 8 is still drawn as one billboard.
- Budgets seen: 0x400 effects, slot stride 0x60, 256 cloud records, one spot-light effect.

## Effect technique decode (verified from DXBC via `iw6_tech_dump` + `dxbc2wgsl`)
- falloff: Add: rgb = k * lerp(begin,end,x) with x from the normal/view cosine; Blend: rgb = lerp. Constants falloffParms .z/.w, falloffBegin, falloffEndCo.
- zfeather (soft particles): depth feather in the pixel shader (featherParms.x). Not done on CPU; needs the GPU pass.
- eyeoffset: origin moved toward the camera by eyeOffsetPar.x (clamped to half distance).
- `_abl` dual-source blend is, for the colour, a plain alpha blend (decoded), so it maps to Blend, not Add.
- `particle_cloud_*replace*` is not opaque.

## Deviations fixed (switches default on)
| Deviation | Switch |
|---|---|
| Effect techniques ignored falloff, near ramp, eye offset | `BO2MP_IW6_FX_MATERIAL` |
| Blend taken from guessed state bits; now from technique tokens, `_abl` = Blend, unlit/no cull/no alpha test | `BO2MP_IW6_FX_DRAW` |
| Oriented sprite axes (0,2) instead of (1,2) | `BO2MP_IW6_FX_FRAME` |

## White fans (owner screenshot): evidence
mp_frag only (snow map not staged). Placed `vfx_smk_puffy_white_01` oriented sprites have half size 2000x2000 (a 4000 unit card), spawnAngles/angVel all zero, frame axis 2 = up, so with createfx angles of yaw only they stand as vertical white walls at z about 2000 (5 placements; widest 15% of a 480x270 default frame at 5685 units). Before the fix they used axes (0,2), after (1,2): both are vertical planes, so the axis error alone turns the wall by 90 degrees but does not explain flat fans. The other candidate is the falloff-less godray/beam sheets (`gfx_spotlight_beam_flashlight`, window godrays), whose alpha now drops 255 to 120-188 with the falloff table. Not proven without the owner's map; needs his screenshot after this branch and log lines `IW6_SHOT_AXES`-style output of the placed oriented sprites on that map.
Clouds (type 8) are ruled out: no oversized quads with clouds on.

## Correction (later the same day): the hard-edged 4000 unit sheets were a harness bug
`vfx_smk_puffy_white_01` carries three colour-class pictures: a vector field (`vfx_vf_perif_smoke_l2r`, hash 06592a6e, opaque 64x64 RGBA8), a gradient and the real `colorMap` (hash a0ab1041, `vfx_smk_plume_puffy_01`). `iw6_fx_shot` took the first one, so the alpha was opaque and the cards looked like hard sheets. The game lane (`s1::material_in`) picks `colorMap` by hash and is right; the harness now does too. With the right picture the 4000 unit cards are soft puffs. So these cards do not explain the owner's sheets.
The exe has no quad extent clamp: the draw path only does a frustum-sphere cull (radius max(size0,size1), `fx_cull_elem_draw`) and drops elements at or below `fx_alphaThreshold` (colour/fade alpha), so no clamp is added.
Remaining suspects for sheets radiating from apex points: tail (type 2) beams (`gfx_spotlight_beam_flashlight`, `gfx_godray_beam_win`, cones) and spark fountains; need the owner's map and screenshot.

## Perf (offline proxy, mp_frag, all 245 placements, default camera, 480x270)
t=0.5 s: 204 sprites, 35 on screen, 20 draws / 13 materials / 11 blend switches, overdraw 0.70x. t=4 s: 451 sprites, 81 on screen, 22 draws, overdraw 1.03x, no quad over a quarter of the screen. CPU: sim about 0.9 ms/tick, build 8 ms in a debug build (inflated). So the CPU side is small; the cost is GPU fill/draws, not measurable offline. No before/after change made (no perf code changed).

## Not done
Trails, spark fountains, model elements (6 effects on mp_frag), effect lights, flares (type 4, opt-in `BO2MP_FX_FLARE_SPRITE`), decals, runner, vector field, distortion, soft particle depth feather, per-vertex falloff (approximated per sprite), cloud volumes, worker-thread simulation, placement distance cull. Types used on mp_frag: 0,1,2,4,8,10,11,12,14,16.


## Update (2026-11-05): the white fans, most likely cause found; cull, tail size

### White fans: screen blend drawn as add (fixed, `BO2MP_IW6_FX_SCREEN`, default on)
The owner's pyramids have the apex at the top and a wide base on the ground, white and translucent over snow. `fx/lights/vfx_light_util_square` e0 is a tail (type 2, 100 x 244 units, colour 1.0/0.97/0.91, alpha 0.33) of `gfx_spotlight_beam_flashlight`, technique `effect_zfeather_falloff_screen_lin_nofog_ndw_bl`. Its state bits are blend op 1, src 10 (INV_DEST_COLOR), dst 2 (ONE): a **screen** blend. The lane drew `screen` as add (`T6Blend::Add`, SrcAlpha/One), which over bright snow saturates to white, while a screen blend adds nothing where the frame is already bright. A lamp beam with a cone picture therefore became a hard white pyramid on every bright map. Now `T6Draw::screen` (draw-code bit 0x40 of an effect code) selects a pipeline blend of OneMinusDst/One with the colour premultiplied by alpha in the shader (`EFFECT_SCREEN`). Not seen on a GPU (the cone pictures are in paks that are not staged): needs the owner's screenshot. Cloud-free, so it holds for every map; mp_frag has 2 screen materials.
Alternative suspects, ruled out by the exe: tail geometry (below), spark fountains and flares (none placed on mp_frag; type 4 is a screen-space handler, below).

### Tail (handler 140337210, quad builder 140339030), line by line
- Anchor = origin - size[1] * v; the quad spans +-size[1] along v and +-size[0] along the tangent. This equals the lane (`tail_anchor_origin`, 2*size0 by 2*size1). The exe tangent is delta x v (ours v x delta): only the winding differs; effects do not cull.
- Rotation turns the rectangle in the (tangent, v) plane, as the lane's spin.
- Deviation fixed: size[1] is read from its own channel only when the element has flag 0x10000000 (non-uniform size); otherwise it equals size[0], for tails too (`BO2MP_IW6_FX_TAIL_SIZE=0` gives the old always-read).
- Velocity: base + random * amplitude per axis, local (flag 0x1000000) in the element's frame, world (0x2000000), times 1000 (per ms to per s); zero velocity gives no quad.

### Draw cull (exe `fx_cull_elem_draw`, default on, `BO2MP_IW6_FX_CULL=0` off)
Sprites and oriented sprites: sphere at the origin, radius max(size0,size1), against the view planes (5 with flag 0x400). Tail (`cull_tail`): per plane, with s = n.v, f = |s|*size1 + signed distance of the tail middle; dropped when f < -size0*sqrt(1-s^2). No distance cull exists in the exe. Counted in `gaps.iw6_culled`; harness `IW6_SHOT_CULL=0` disables, planes come from the camera.

### Per-type table (draw handlers seen in the decompile)
| Type | Exe handler | State here |
|---|---|---|
| 0 billboard | 140336bb0 | drawn |
| 1 oriented | 140336f80 (axes 1,2) | drawn |
| 2 tail | 140337210 | drawn (above) |
| 3 trail | not located | converted trail path |
| 4 flare | 140337c40: screen-space NDC quad, position scaled by a def factor, curves by the view angle | not drawn by default (`BO2MP_FX_FLARE_SPRITE=1` draws a world sprite, unverified) |
| 5 particle sim | 140337490 (ParticleSimAnimation) | not drawn |
| 8, 9 clouds | 1403350e0 / 140335580 / 140335210 | one billboard (type 8), spark cloud (9) |
| 10 spark fountain | not located (cloud lists, "Particle Spark Cloud Indices") | converted BO2 fountain, not compared |
| 11 model | not located | not drawn (6 mp_frag effects) |
| 12 omni | 140337960: origin, radius size0, colour * scale (0xac), culled by the sphere | not drawn |
| 13 spot | 140337aa0 | not drawn |
| 14 sound | n/a | played |
| 16 runner | not located | spawn only |
| 17 vector field | 140338540 ("Apex Turbulence") | not drawn |
Also: handler 1403377c0 (colour by distance, submits a draw with 0xd00/0xd01) was not identified.

### Not done (honest list)
Worker-thread simulation, placement distance cull (the exe has none), models, effect lights, flares, decals, vector field, distortion, GPU depth feather for Ghosts (the GPU pass already feathers `zfeather` effects through the BO2 path), a spark-fountain comparison. World renders could not be run (the box OOM-kills `--world`).


### Flare handler (type 4), phase 2
Read from the text decompile (`iw6_decompile/complete/iw6mp64_ship_all.c`, no Ghidra on the box). Flare def: tables' counts at def offsets 64, 68, 72, 76; pos 0, rotcount 4, flags 8, depthScaleRange 12, value 20, radialRot 28, scaleX 36, scaleY 44, dir 52. Logic is translated in `render_fx::iw6_flare`; rotation args, screen radius and source-cos axis are inferred; occlusion is not done.


## Runner (type 16) and vector field (17), read 2026-11-05

Spawn dispatch is `FUN_14035c150` (elem def stride 0x138, type byte at +0xb4).

### Runner: `FUN_14035bc40`
Spawn-time only; no element slot is taken. It computes the element's origin in the effect's frame (emitter + local
offset, `FUN_1403676e0` / `FUN_140367a40`), picks the child effect from the visuals array (`visuals[(rnd16 * count) >> 16]`
when the count is not 1: the same selection as `runner_child_edge(random_seed)`), builds the child's axes, and with element
flag 8 (random spin) rotates them about the axis by `rnd * 360` degrees, then queues a child-effect request
(`FUN_14035a450`: slot record 0x80 bytes: effect def, origin, axes, seed, spawn time, and the parent's bolt entity / tag
when the element carries a tag index). The queue is drained by the effect system into real effects with the child's own
elements. Our shared engine does the same through `FxChildEdge` / `pending_runners` (`render_fx::present::drain_spawn_runners`);
the Ghosts types map to it (`s1_elem_type` RUNNER -> RUNNER), so Ghosts runners were not "spawn only" after all: the
note above ("not located") is superseded. What is not matched: the exe passes the parent entity / tag so a child follows a
bolted parent (our request carries the origin only), and elements with flag 0x80000000 and a +0x40 effect pointer (a child
effect spawned by a normal element when it is created, `FUN_14035a210`) are not played.

### Vector field (type 17): two parts
1. Spawn `FUN_14035c010`: loads the asset `vectorfields/<name>` (`DB_FindXAssetHeader`, type VECTORFIELD) and registers an
   instance in a 256-slot table (`FUN_1401494f0`): asset, a 3 x 4 transform (position + axes), a scale / weight. No element
   is allocated for the vector field itself.
2. Consumers (`FUN_140365330` the particle step; `FUN_1401495c0` collects up to 8 instances whose world box (the asset's
   bounds, longest half extent as a radius) holds the particle, once per effect update when the effect def bit 8 is set;
   `FUN_140149c20` sums the sampled vectors, each times the instance weight, over the instances of one `type` (1 =
   acceleration-like: summed into the particle's velocity after gravity; 2 = velocity: replaces the particle's velocity
   when any instance holds the point); `FUN_140149840` is the sampler: trilinear over the asset's 3-D grid of float4
   (`linearData`, cell size `ds`, `numEntries` x / y dims, `worldBounds` mins / maxs; instances are axis-aligned boxes
   at the instance position; flag bit 0 = uniform field: the first entry for any point inside), `cg_vectorFieldsForceUniform`
   forces that path).
Draw handler `FUN_140338540` ("Apex Turbulence") only hands the instance's position / orientation and an LOD fraction
(`r_apexTurbulence_LOD_*`) to NVIDIA APEX's turbulence volume (a GPU fluid grid): that simulation is middleware code in the
exe and is not translated.
Status: the fastfile walker reads only the visual's name (`FxVisualRec::Other`); the `VectorField` asset record (80 bytes:
name, linearData, worldBounds, ds, numEntries, flags, type) is laid out (`fastfile_iw6::layout::VectorField`) but not
walked; the catalog drops type 17 (`s1_elem_type` has no arm). Wiring needs: walk the asset's grid, a registry in the fx
host, and the force step in the particle integrator behind a Ghosts-only flag. Not done here (see IW6_NATIVE.md).
