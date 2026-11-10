# Black Ops III maps: visual parity audit (`render_t7` against the game)

Goal (owner): a Black Ops III map must look 1:1 with the real game, so
everything the game's renderer draws for a map must exist in `render_t7`.
This table lists every visual system the game's renderer has for map
rendering, what `render_t7` does about it today and the evidence for the
system (the game's shaders in the zones, its technique and code-shader
tables, and the static analysis of the owner's exe described in
[T7_NATIVE.md](T7_NATIVE.md); exe addresses are kept out of the repo).

Status words: **done** (drawn the way the game does, from its own
programs and data), **partial** (drawn, with named gaps), **missing**
(nothing drawn), **other agent** (being built in parallel on the
lighting/post side: lit fog/volumetrics, GTAO, bloom, lens flares,
colour matrix; not touched here).

Evidence key: *patch* = a code shader in `core_patch.ff` (the code
technique table names it), *common* = a material program in
`core_common.ff`, *map* = the map zone's own technique sets, *walk* = the
`fastfile_t7` walker reads the asset, *exe* = found in the exe's renderer
(T7_NATIVE.md has the details).

Updated 2026-10-26.

## Audit

| System | Status | What render_t7 does / lacks | Evidence |
|---|---|---|---|
| Depth prepass | done | G-buffer materials write their depth in their slot 3 technique; an opaque forward-only material (emissive road, `lit_emissive*_plus`: no blend, depth tested without write) has its slot 2 technique drawn in the G-buffer pass (`Material::forward_depth`, 2026-10-27; `T7_FORWARD_DEPTH=0` off) | map/common technique slot 2; the forward state maps |
| G-buffer (lit, plus, advanced, detail, micro-tile blend, vegetation, cloth, skin, backlit, fxanim) | done | `gbuffer`: the materials' own slot-3 programs, state maps, samplers, layouts | common `gbuffer_*`; T7_NATIVE "G-buffer pass" |
| GPU-skinned fxanim (vines, flags, banners) | partial | bound in rest pose (`gpu_skin`); their xanim playback not ported | `mcs/fxanim_vegetation`, `mcs/lit_flag` read `gpuSkinBase/Quat/Pos` |
| Deferred decals (G-buffer decals, reveal decals) | done | drawn in the G-buffer lists through `resolvedNormal` | common `gbuffer_decal*`, code texture 0x4f |
| Forward decals (`visibleDecals`, `forwardDecalConstants`) | missing | decal list bound empty (no decal packer, no `light_culling_decals`) | patch `light_culling_decals`; forward programs' RDEF |
| Effect decals / marks (FX element type 13, impacts on world) | missing | not drawn natively (the converted runtime draws BO2-style marks over the frame) | walk `FxElemMarkVisuals`; element type 13 |
| Deferred lighting (tile classify, 64 permutations) | done | `lighting`: classify, permute draw, `deferred_lighting_permuted(N)` | patch; T7_NATIVE "lighting pass" |
| Bilateral / SSR lighting permutations | missing | the plain permutation set runs; `_bilateral` and `_ssr` sets not | patch `deferred_lighting_permuted_ssr/_bilateral` |
| Sun light, sun constants | done | `lighting_globals` from the map's sun volume | exe sun constants builder; T7_NATIVE "map's sun" |
| Sun shadow cascades | partial | `sun_shadow`: fit and static tree fill, checked offline against the scene's triangles on Sector (`t7_sun_shadow_check`: 90..94% of pixels agree, cascade 0 99%; the lighting's lookup reads the maps the same way); dynamic casters (script models, players) not drawn; not seen on PC | patch `depth_fill_from_sst`, `floatz_writedepth`; the lighting permutations' lookup |
| Spot / omni shadow maps, cookies | missing | bound empty | lighting RDEF `gSpotShadowmapArray`, `gOmniShadowmapArray`, `gCookieArray` |
| Primary/local lights packing | done | `lights`: the exe's cull constants and pack | exe light packer; `LightingPackCBuffer` |
| Reflection probes (packing, blend volumes) | done | `probes`: packed per frame, cube array bound | exe probe packer; `RefProbePackCBuffer` |
| Probe volumes (diffuse SH grid) | done | `gProbeX/Y/ZArray` from the world | walk `GfxProbeVolumeTextures` |
| Screen-space reflections | missing | `_ssr` permutations not run | patch `_ssr` sets |
| Light culling (lights/probes clusters for forward) | done | `forward::Cull` | patch `light_culling_lights/_probes` |
| GTAO / HEMI AO | other agent | `ao`: GTAO + temporal denoiser | patch `ssao_gtao_*`, `ssao_denoiser_*` |
| Motion vectors (static) | done | `motion` | patch `motion_vector_static` |
| Fog (world fog, fog banks, sun fog) | done | fog constants from the map's banks and volumes | exe fog filler; T7_NATIVE "Fog" |
| Lit fog volumes / volumetric light | done (default on; local lights and 3-D volume unseen: no staged map has them) | `volumetric_pass`: pre-pass (depth, light/probe culling, 3-D volume), sun and local-light marches, blurs, apply; read back offline on Sector and Apartments (`T7_VOLUMETRIC_GPU=0` off; `T7_VOLUMETRIC_TEST=1` forces the local/3-D parts) | patch `volumetric_light*`, `downsample_depth_with_uv_4x_compute`, `volumetric_*_cull_compute` |
| Sky (lat-long HDR, sky box, skyMx/sunMx) | done | forward sky draws, vukko blend | common `sky_latlong_hdr`; exe vukko blend |
| Cloud shadows / sky-rotating sun shadows | partial (opt-in, unverified) | cookie array loaded (byte order checked), bound only with `T7_CLOUD_SHADOWS=1`; spot cookie layer = light word +0x230; no staged map's sun has a cloud layer (Mountain checked too), and offline the map lights light nothing, so neither is seen drawn | `GfxWorld` +0x260 `gCookieArray`; sun constants 24..35 |
| Emissive surfaces (scroll, layered, masks) | done | forward list 7/8 | common `lit_emissive*` |
| Transparent / blended surfaces | done | forward lists 9/10, back to front where the exe reverses | common `lit_transparent*` |
| Glass (refraction through `resolvedPostSun`) | done | scene copied before lists 9/10/20/21 | common `glass*` |
| Order-independent transparency (OIT) | missing | OIT targets and `oit_apply*` not built (its programs use texture atomics WGSL lacks) | patch `oit_apply*`; T7_NATIVE "Done vs not yet" |
| Water (shore flow, river sim boxes, `floatZDownscaled`) | done (Metro checked offline; Warhawk unverified) | the exe's own water programs in the forward lists (refraction of `resolvedPostSun`, depth fade, fresnel, sky reflection, normals scrolled by game time); `floatZ` handed to them in the exe's encoding (`63/(64·d)`, `forward::FloatZ`: read raw it put the scene ~4x nearer and the depth fade saw no water); brush-model water (`wave_water` / `wave_clamp`) drawn at its entity (`fastfile_t7::map::brush_places`), brush models no drawn entity names left out; one `t7 water:` log line per map | common `water_*`; patch `downsample_depth_4x_compute` |
| Ocean / large water simulation | missing | none of the staged maps uses an ocean technique; not looked for | (to check on a map that has one) |
| Weather grime (wet, snow volumes) | done | `weather`: volume numbers and images; no shipped program reads them on the staged maps | exe weather filler |
| Rain / snow particles | partial | they are map-placed effects (`weather/fx_*`): sprites now drawable natively (below) | map entity strings |
| Wind (vegetation sway) | partial | vegetation programs' wind term with default constants; the exe's wind volumes not read | common `vegetation*` read wind constants |
| **Particles / FX sprites (billboard, oriented, rotated, cloud)** | **partial** | `fx` + `forward::fx_draw`: placed effects evaluated each frame and drawn with their own `ei/effect_*` techniques, list 18 position; **on by default again** (`BO2MP_T7_NATIVE_FX=0` off; 2026-10-27 after the particle rules below and `fx_check` on all six staged maps); GPU draw run offline (`t7_fx_check`) | common/mp `ei/effect_lit_blend`, `_lit_emissive_blend`, `_distortion` (layout group 8, 64-byte sprite record read off the vertex program) |
| Sprite lighting (`fx_compute_sprite_light`, `lmapTex`) | partial (new, off) | `BO2MP_T7_FX_LIGHT=1`: `fx_compute_sprite_light_shadow` run per frame, one 8x8 `lmapTex` layer per sprite (the nearest 2047), `spriteExtra` built from the quad as the vertex program builds it; off: `lmapTex` 1.0. Offline: runs without validation errors, finite light (below); not compared with the game | patch `fx_compute_sprite_light*`; PS reads `lmapTex` |
| FX tails / lines (3, 4) | partial (on) | on by default (`BO2MP_T7_FX_TAILS=0` off); velocity-aligned quads (IW4's tail: size0 wide, 2 x size1 long, behind the particle; a line ahead of it) through the sprite record's own-axes flag, wound as the billboards (they were back-facing and culled: the "changed no pixel"); offline Evac's `fx_water_stream_sm_apt` draws a thin falling stream; geometry not read off the exe | element types; `fx_iw4::tail_*` |
| FX trails (5) | missing | not drawn natively (strips from `FxTrailDef` need per-particle history) | walk `FxTrailDef` |
| FX model elements (7) | partial (new, off) | `BO2MP_T7_FX_MODELS=1`: each model element's XModel built once as a pool of 32 G-buffer model instances; every frame the nearest particles of that model take the bones (particle place and orientation, scale 1), the rest are zero. Unverified on the GPU (`t7_fx_check` has no G-buffer); Evac has 1 model element | element type 7 visuals are XModels |
| FX light elements (8) | done | effect lights added to the scene lights as the exe does | `fastfile_t7::scene_light`; T7_NATIVE "Effect lights" |
| FX runners (14, child effects), death effects | partial (new) | a runner plays one of its effects where and when each particle spawns (its orientation), a death effect where each particle dies; evaluated from time alone, nested to 3, 16 children per element at a time | element type 14 visuals; `FxElemDef` +0x1a0 |
| FX emitted / impact effects | missing | emitted (per distance travelled) and impact (collision) effects not played natively | `FxElemDef` +0x1a8, +0x198 |
| FX sounds | n/a | audio, not the renderer | element sound names |
| Distortion (heat haze, water rings) | partial | technique pipelines made (slot 6); drawn only if its bindings resolve | common `ei/effect_distortion` |
| Exploder / scripted / impact / explosion effects | partial | `BO2MP_T7_FX_EXPLODERS=1` plays effect entities that wait for an exploder as if it fired at the map's start (the map script that fires them is not run); none of the staged maps' effect entities waits for one (`t7 fx: exploders` line); the converted runtime still plays impacts and explosions | entity `exploder_count`, ImpactFx tables |
| Lens flares (`klf`, occlusion query) | missing (findings) | not drawn; the per-element 64-byte dynamic record writer is now read (below); the source list builder and `gEmitterStatic` writer are not | `techsetdef_lensflare*`, `lensflare_query` |
| Exposure (eye adaptation) | done | `post_exposure` + `exposure_compute` | patch `exposure_compute` |
| Colour grade LUT (vision blend, LUT volumes) | done | `post_lut`, `create_lut2dv` | patch `create_lut2dv` |
| Colour matrix | other agent | settled as not needed (no program reads it) | scan of every staged zone |
| Bloom | other agent | `post_bloom`: downsample/blur chain, per-view settings | patch `bloom/*` |
| Lens distortion (`bloom_apply_lut` variants) | missing | plain `bloom_apply_lut` | patch lens distortion variants |
| Depth of field | missing | not built (its programs use `sample_pos`, not translated) | patch `dof/*` |
| Motion blur | missing | `motion_vector_static_with_mb` and blur not run | patch |
| SMAA / FXAA (exe default: Filmic SMAA T2x) | missing | no AA pass | patch `smaa_*`, `fxaa_hemi_aa_*` |
| Final resample (dither, sRGB) | done | `resample_final_bm_null` | patch `resample_final*` |
| Screen effects (damage, flash, shellshock overlays, ADS blur) | missing | player-state driven; ADS blur bloom variants not run | patch `bloom_downsample_*_adsblur*`; HUD overlays from the game side |
| Sky box models streamed from base packages | partial | missing when the package is not beside the map (Nuk3town staged without it) | `t7 sky` log lines |

## The effect sprite record (from the effect vertex program)

`ei/effect_lit_blend`, `ei/effect_lit_emissive_blend` and
`ei/effect_distortion` all draw with the same vertex declaration in
layout group 8: one 64-byte record per instance (POSITION f32x4 @0,
COLOR0 rgba8 @16, TEXCOORD0 u32x3 @20, NORMAL unorm10_10_10_2 @32,
TEXCOORD1 unorm10_10_10_2 @36, TEXCOORD2 f32x4 @40, TEXCOORD3 u16x4 @56)
and the quad corner from `SV_VertexID & 3`. What each field does was read
off the translated vertex and pixel programs (`t7_fx_techs <map.ff>` with
`T7_DUMP=<dir>` writes them; `dxbc2wgsl` translates) and is written up in
`crates/render_t7/src/fx.rs`. Read from the programs: the atlas word
(cell, next cell, blend, column and row bits), the facing flags (own axes,
z-up, eye-facing), the rotation in 1/65536 turns, the half sizes and
pivot, the alpha mode (256 = plain alpha; 0 makes the sprite vanish), the
soft-particle depth (`floatZ`), the near fade, the view-angle fade, and
the emissive scale (half float) multiplying the emissive mask.

Not read from the exe (the CPU packer of the sprite records is not
decompiled): how the colour bytes are encoded, what feeds the emissive
scale (taken from visual-sample word 6, clamped at 0) and the sprite light
map's contents. The colours are now written **as authored** (no CPU
sRGB decode): neither the vertex nor the pixel program converts them, and
only so do Nuk3town's effects take the colours they are named for (offline,
below: `fx_de_rez_tree_green` green, `_tree_pink` pink, `_fireplace`
orange; decoded, `_wall_orange` and `_ground_orange` came out red). Byte
order is red first (`_ground_blue` is blue).

## Nuk3town's cyan plaza (fixed 2026-10-27)

Owner's screenshots: a flat, glowing cyan-blue slab where the plaza floor
between the buses should be, props standing on it. Found offline
(`t7_surf_at`, CPU, `t7_floor_check`, GPU):

* The plaza's floor is two static models, `p7_ntx_road` (materials
  `t7_ntx_plastic_road_gutter`, `_road_solar_tetris`, `_road_solar_tetris_dark`,
  `t7_ntx_decal_trim_line_tall_emissive_blue_lt`) and `p7_ntx_sidewalk`, both
  placed at (32, 136, -66); no world surface covers it (the map's collision
  there is those models'). Their meshes stream from the map's package (not
  staged here: offline they are not built; on the PC they are).
* Under the whole map, 110 units below the road, lies the vista water
  (`mtl_p7_ntx_vista_water`, `water_shore_flow_nonflat`, a forcefield-cyan
  colour map; forward, writes depth).
* The road's materials have no G-buffer technique. Their forward technique
  does not blend and tests depth greater-equal **without writing it**: it
  relies on the material's slot 2 (depth-only) technique having laid the
  depth first. `render_t7` never drew slot 2, so the road left no depth and
  the water, drawn later in the forward lists, passed the depth test and
  covered it: the cyan slab.

Fix: an opaque forward-only material's slot 2 technique is drawn in the
G-buffer pass (`Material::forward_depth`; sky sets left out). Offline
(`t7_floor_check`, Nuk3town's southern road with only road, sidewalk and
water draws, 320x180): depth on 4914 pixels before, 30702 after, the road's
25812 pixels covered; no validation error. The full frame (the road's colour
over its depth, the water culled behind it) was not rendered offline: the
software adapter runs out of memory beside the other agents' jobs. Not seen
on PC.

**Effects (for the effects agent; not changed here):** the same screenshots
show square brick-like sprites (white, orange, brown) floating everywhere in
the air, large and opaque-looking; these are the `fx_de_rez_*` effects now
drawn natively. They look far bigger and more solid than the game's de-rez
specks: worth checking their size graphs, orientation and blend against
the game (the owner's report, 2026-10-27).

## Effects on the GPU, run offline (2026-10-26, `t7_fx_check`)

`t7_fx_check <map.ff> [w h] [effects]` draws only the effect pass: one
small view (320x180) per named effect from 300 units, no G-buffer
pipelines, no forward world draws, no coverage, the lit scene cleared to
grey 0.18 and depth to far; pictures at `T7_IMAGE_MAX_DIM=256`. It fits the
box's 7 GB software adapter (the full offline frame did not). It reads the
pixels back: how many changed, their mean, their extent against where the
CPU puts the sprites (`T7_FX_SPRITES=n` keeps the nearest n).

Two bugs found and fixed by running it:

* **every lit and emissive effect material failed to bind** (8 of 10 on
  Nuk3town: `$Globals (b0) neither the material's nor made by the frame`,
  then `sampler ... not in the material's table`): a material whose
  constant buffers or sampler table are shared with one loaded earlier
  references them, and the walk kept nothing for a reference.
  `fastfile_t7::walk::material` now keeps each constant buffer by its
  record's place and gives referencing materials its name and contents,
  and reads a referenced sampler table where it was loaded. 10 of 10 now
  bind (this also gives any world material that shares constants its
  `$Globals`);
* colours were sRGB-decoded on the CPU (above).

Measured (Nuk3town, 320x180): one sprite of `fx_de_rez_fireplace` lands
on 153..169 x 79..95 where the CPU puts its centre at (161, 88) within
12 px; `_tree_green` changes 4288 pixels, mean (0.17, 0.33, 0.17);
`_tree_pink` (0.39, 0.30, 0.50); `_fireplace` (0.41, 0.32, 0.14), peaks
5.6 (emissive); `_butterfly` blue; Evac's `dirt/fx_dust_motes_300x300`
11 pixels of grey-white motes (half size 0.3). No validation errors, no
non-finite pixels. Ground and wall effects (`_ground_*`, `_wall_*`) are
oriented quads seen nearly edge-on from this view and cover few pixels.
Not compared with the game.

## Effects measured offline (2026-10-26, staged zones, CPU evaluation)

| map | placed | defs | elements | sprite elements drawable | sprites alive (t = 0 .. 60 s) | CPU per frame |
|---|---|---|---|---|---|---|
| mp_nuketown_x | 622 | 53 | 219 | 199 | 1398 .. 1878 | ~2 ms |
| mp_apartments | 971 | 50 (53 with child effects) | 156 (169) | 77 (82 with child effects; plus 77 tail/line elements now) | 980 .. 1033 | ~2 ms |
| mp_sector | 204 | 46 | 152 | 99 | 1268 .. 1288 | ~2 ms |

Every sprite visual on the three maps resolves to a material with a forward
technique. Looping spawns follow IW4's `FxSpawnDefLooping` (one element per
interval, `count` in all; every Nuk3town element has `i32::MAX`, no end).
Evac's elements by type: 67 billboards, 9 rotated, 43 tails, 34 lines,
6 clouds, 1 model, 2 lights, 5 lens flares, 2 runners; 21 elements have a
death effect. Nuk3town: 2 runners (`fx_de_rez_sm_sparkle_runner` plays
`fx_de_rez_sm_sparkle` every 4.5 s), 3 death effects. No effect entity on
Nuk3town or Evac waits for an exploder (their keys: `fxdef`, `origin`,
`angles`, `delay`, `fxstate1..4`, `timescale`, `guid`,
`primaryLightFraction`, `lightingOriginOffset`).

## Child effects, tails, the rest

* **Runners and death effects** (`fx::FxScene::children`): a runner
  element (type 14) plays one of its visuals' effects (picked by the
  particle's seed) at the particle's spawn place, in its orientation, when
  it spawns; an element with a death effect (+0x1a0) plays it where each
  particle is when its life ends. Children are evaluated from time alone
  like their parents (an effect's duration: its longest delay + life,
  plus its looping life; endless for a looping effect without one), nested
  to 3, 16 at a time per element. As IW4's runner; the exe's own spawn code
  is not ported.
* **Tails and lines** (3, 4): IW4's tail quad (`fx_iw4::tail_*`, as Black
  Ops II's runtime draws it): across the velocity and the view, size0
  wide, 2 x size1 long, its centre size1 behind the particle (a line:
  ahead), the velocity taken from the particle's position 8 ms either side.
  Packed with the sprite record's own-axes flag. The geometry is IW4's,
  not read off Black Ops III's exe. **Winding (fixed 2026-10-27):** the
  vertex program's quad normal is `U x V` with `U = B`, `V = -A` at
  rotation 0; a billboard's (`A` = view up, `B` = view right) points away
  from the eye, and the effect materials cull back faces. The tail's
  `across` was `velocity x to-eye`, which turned its normal toward the eye:
  every tail and line was culled (the offline view that "changed no
  pixel"). It is now `to-eye x velocity`. Offline, Evac's
  `fx_water_stream_sm_apt` from 300 units: 0 pixels before; 2568 with
  `T7_FX_DEBUG_SIZE=20` (every sprite 20 units and opaque) and 373 at the
  authored sizes after, a thin falling stream (x 157..162, y 85..172), no
  validation errors. On by default now (`BO2MP_T7_FX_TAILS=0` off).
* **Model elements** (7, `BO2MP_T7_FX_MODELS=1`, unverified on the GPU):
  `FxScene::models` evaluates their particles as the sprites' (spawn,
  life, place, orientation; scale 1, IW4's; the size graphs not applied),
  `Scene::read_effects` builds each named XModel once as a pool of 32
  model instances (bones all zero: nothing drawn), and
  `GBuffer::update_fx_models` gives each frame's nearest particles of a
  model that pool's bone matrices (`t7 fx: models:` lines: built, placed of
  alive). Staged maps: Evac's one model element (`p7_wires_metal_recessed`
  in `water/fx_water_drip_model_wires_metal_recessed`) is one-shot with a
  spawn count of 0, so it never shows; Sector has 8 (the haboob vista
  `p7_fxp_vista_haboob_01` and swirling newspapers
  `p7_fxp_debris_newspaper_01/_03` on 15 placed effects: about 100
  particles alive at 20 s, the nearest 32 of each model drawn); Nuk3town
  and Skyjacked none. `t7_fx_check` prints them (CPU).
* **Exploders** (`BO2MP_T7_FX_EXPLODERS=1`): effect entities with an
  `exploder_count` are played from the map's start, as if their exploder
  fired then (the client script that fires them is not run). None of the
  staged maps places one, so this is untested on data.
* Left out, with their `t7 fx:` lines: trails (5: strips from
  `FxTrailDef`), lens flares (12, below), decals (13), emitted and impact
  effects.

## Oriented sprites' facing

The same culling hid oriented sprites (1) seen from the front: they were
packed with `A` = the particle's axis 1, `B` = axis 2, whose quad normal
`B x -A` = axis 0 points toward an eye in front of the effect. Nuk3town's
`fx_de_rez_ground_blue` seen from 300 units above (`T7_FX_FROM=z`): 0
pixels. Now `A` = axis 2, `B` = axis 1: the width runs along the
particle's left and the height along its up (as Black Ops II's runtime
draws them) and the normal is -axis 0, so an oriented sprite shows from
the side its forward axis points to, as a billboard shows to the eye.
After: 358 pixels, blue (mean 0.19, 0.22, 0.38), no validation errors.
Which side the game shows was not read off the exe (only that the
materials cull back faces and the program's winding).

## The sprite record's writer, read off the exe (2026-10-27)

The owner's Nuk3town (native effects on by default) showed square blocks
floating everywhere and a large cyan area round the hover pods. The blocks
themselves are Black Ops III's (`docs/screenshots/nuk3town_ref/03`: gold
voxel cubes in light beams; Nuk3town places 622 effects, all de-rez, all
in every lighting state); what was wrong is how they were drawn. The
exe's record writer (`FUN_141573090`, static analysis of the reference
exe) and its per-type draw table (0x142ec75b0: billboard, oriented,
rotated, tail, line, -, cloud, model, light, ...) show what `fx` wrote
wrongly; fixed in `fx::particle` (unit test
`the_record_takes_the_defs_draw_words_as_the_exe_writes_them`):

| record | the exe | was |
| --- | --- | --- |
| 0x18 view-angle fade | def +0x238 \| +0x23c << 16 (Nuk3town: 65, 85 degrees) | 0: oriented ground and wall sprites never faded edge-on |
| 0x28 pivot | def +0x228, +0x22c | 0 |
| 0x38 alpha mode | 256 / max(def +0x230, 1e-4), at most 0xffff (pod sprites: 0.39, 0.098 -> the pixel program's alpha threshold) | always 256 |
| 0x3a soft depth | 16 x def +0x234 (Nuk3town: 10 .. 100) | 0: hard edges where sprites cut the ground |
| 0x3c flags | billboards: def bit 0x100000 -> 1 (face the eye), 0x80000 -> 2 (z-up); oriented and rotated: 4 (own axes) | rotated sprites (type 2) drawn as z-up billboards |
| 0x20/0x24 axes (type 2) | the def's spawn angles alone (`FUN_141575410` -> `FUN_1415d56a0`) times the effect's axis | none: Nuk3town's `fx_de_rez_pod_bottom_glow_blue` (30 x 490 glow strips pitched 90 degrees, i.e. lying on the road under the pods) and `_pod_bottom_*` stood upright as tall cyan walls |
| 0x1c emissive | sample word 6 + random x the same word of its second half (base + amplitude) | lerp between the two |
| 0x14 atlas | `FUN_14156fce0`: behaviour bit 0x80 enables it; 0x10 blends to the next cell (byte 2 = fraction), 0x20 plays backwards, start mode 3 = a range (byte 6 cells); Nuk3town's flipbook `9c 0a 00 01 03 02`: cell 10 + 32 x life, held on 31 | IW4's index only: no blend, no enable bit, no range |

Still unverified: the drawn result (this box's software GPU runs out of
memory on Nuk3town with the other builds running; not seen on PC), the
colour (record 0x10: def word 4 bit 0x80000 tints it, not done; no
Nuk3town element sets it), the size and colour graphs' random columns.
Native effects stay opt-in (`BO2MP_T7_NATIVE_FX=1`) until they are seen
matching the reference shots.

## The particle rules read off the exe (2026-10-27, `fx_rand`, `fx_check`)

Read from the reference exe (static analysis): the dispatcher
`FUN_141578d80`, the draw state builder `FUN_14157cc80`, colour
`FUN_141578bd0`, spawn place `FUN_1415d4bd0` / offset `FUN_1415d47a0`,
physics step `FUN_1415bca50`, model draw `FUN_141575ca0`. Now in
`render_t7::fx_rand` / `fx`:

* **Random table**: Black Ops III's own, 844 floats at 0x142ec7630
  (`crates/render_t7/data/t7_fx_random.bin`), seeds mod **809** (IW4's
  table and 479 were used before); channels as IW4's (life 17, delay 18,
  count 19, visual 21, atlas 22, colour 23, rotation 24/25, size 26/27,
  model scale 28, **emissive 29** (was the colour's 23)).
* **Particle seed** `(sequence x 500 + effect seed + spawn msec) % 809`.
* **Age** = time since it began x the effect's time scale (entity
  `timescale`, now read).
* **One visual sample** (no intervals): the exe reads it with weight 0;
  such elements were dropped before (not drawn at all).
* **Fades**: fade out (+0x20/+0x24) and fade in (+0x28/+0x2c) only when
  their length is not 0, combined as the **smaller** (was a product, and
  a zero-length fade out hid everything past its start); a particle is
  drawn only when the fade is at least 4/255; its colour's alpha is not
  tested.
* **Camera offset**: def byte +0x1f0 pulls the sprite that many units
  toward the eye before the fades (Nuk3town's de-rez: 10).
* **Cylinder spawn offsets** go round the axis def byte +0x6c names (0
  forward, 1 left with the radius's cosine negated, 2 up).
* **Model elements**: scaled by the samples' scale word (+0x14, r28; was
  read at +0x24) times the effect's, not drawn at 0 or less.
* Gravity `(base + r15 x amp) x 800`, spawn origin and sphere offsets:
  as IW4's (confirmed).

Checks (`cargo test -p render_t7 --lib fx_check -- --ignored --nocapture`,
staged zones): `staged_maps_effects_follow_the_exes_rules` plays every
placed effect of Nuk3town, Evac, Sector, Skyjacked, Metro and Western (with
children) at 24 times over 29 s and holds each of ~860,000 sprite
records against its element: finite words, half sizes and colours inside
the def's graphs, the def's alpha mode / soft depth / view-angle fade /
pivot, facing flags by element type (a rotated sprite drawn as a z-up
billboard, the cyan pod walls, fails here), own axes unit and square,
rotated sprites not turning, atlas cells inside the sheet (an enabled
atlas drawing the whole sheet, the brick blocks, fails), each sprite
within reach of its effect: **0 failures on all six**.
`nuketown_overlay_from_its_spawns` builds every quad as the vertex
program does from all 773 spawn views at 3 times: 0 own-axes sprites
standing up taller than 200 units; the largest plain sprite covers 0.165
of a view (`fx_de_rez_flat_spawn`'s 44 x 106 animated strip at 165
units: its def's size1 graph reaches 78, fade in from 100; the reference
shots' data streams). GPU (`t7_fx_check`, Nuk3town, 320x180, the effect
pass alone): `pod_bottom_glow_blue`, `flat_spawn`, `tree_green`,
`wall_blue` draw without validation errors, finite, in their colours.
Not run: a full lit frame (the software adapter runs out of memory) and
the PC.

Still not 1:1: the exe's particles are stateful (pooled, stepped each
frame); `fx` evaluates them from time alone, so per-frame physics
(collision, bounce, emitted/impact effects), the spawn grid mode, the
life override (+0x1f6) and the exact seeds (the exe's absolute game time)
differ; trails (5), decals (13), lens flares (12, the manager's source
list builder not decompiled) are not drawn; sprite lighting and model
elements stay opt-in (`BO2MP_T7_FX_LIGHT=1`, `BO2MP_T7_FX_MODELS=1`), and
exploders (`BO2MP_T7_FX_EXPLODERS=1`) have no staged map placing one.

## Lens flares: the per-element record (findings, not built)

Read from the reference exe (static analysis): the lens flare draw's
setup walks the backend's source list (24-byte entries, count at +0x1000)
and, per source, calls one writer that fills a **64-byte record per klf
element** into a mapped buffer of 100 (6400 bytes, uploaded after the
loop; more than 100 elements in a frame and the source is skipped:
entry +8 = 0xff). For each element (klf elements: count u16 at +0x90,
array at +8, stride 0x1f0) it first computes an intensity from the
source's fade time `t` (+0x24 of the source's 0x34-byte state):
`min(t * e[0x150] + e[0x154], t * e[0x158] + e[0x15c])` clamped to 0..1,
`pow(x, e[0x168]) * e[0x160] + e[0x164]`; when no element is above 0 the
source is skipped. Then, with `k = scale * entry[+0x10] * alphaTable[colour
>> 24]`, the record is:

```text
0x00 f32  1.0 if element flags (+0x20) bit 2, else source x (+0x14)
0x04 u32  0                  0x08 u32  0
0x0c f32  1.0 if element flags (+0x34) bit 2, else source y (+0x18)
0x10 f32  k * the element's intensity
0x14 f32  0 if (+0x20) bit 2, else source +0x1c
0x18 f32  0 if (+0x34) bit 2, else source +0x20
0x1c f32x2 zero if element +0xa4 bit 2, else source +0x1c..+0x24
0x24 f32x2 source +0x1c..+0x24
0x2c f32  element +0xa8 == 1: source +0x30 x a constant, else 1.0
0x30 u32  the caller's word +0x40          0x34 i32, 0x38 i32: source +0x28, +0x2c truncated
0x3c u32  entry +0x0c & 0xc
```

and the entry's word +8 becomes its colour with the first element's index
in the top byte. This is the `gArtefactsDynamic` side; still missing for a
faithful draw: the per-frame source list builder (what the 0x34-byte
state's position, fade time and the entry's scale and colour hold), the
208-byte `gEmitterStatic` writer, the klf element fields beyond those
above, and the occlusion query pass. Lens flares stay off.

## Sprite lighting (built, opt-in `BO2MP_T7_FX_LIGHT=1`, 2026-10-27)

`forward::fx_draw` makes `fx_compute_sprite_light_shadow` (core_patch,
translated like the other code compute programs) and runs it each frame
before the effect pass: one 8x8 thread group per sprite, group z = the
sprite = the `lmapTex` layer, the count in `computeSpriteControl.x`
(`PerSceneConsts` 1312). The nearest 2047 sprites (the device's array
layers less one) are lit; each one's record gets its layer in the 0x1c
high half, the rest the last layer, kept at 1.0. Its `spriteExtra`
(48 bytes) is built on the CPU from the quad the vertex program builds:
words 0..2 the quad's centre (world, pivot applied), words 3..5 its two
full edges in view space as halves. The program also repacks
`spriteData` into `spriteDataW`, which nothing here reads (the vertex
program keeps reading the CPU's record); `spriteData` is bound to the
records only because the program declares it. Lights, probes, the probe
volume, cookies and the sun shadow come from the lighting stage's
bindings, the light culling masks from the forward pass.

Offline (`t7_fx_check`, Nuk3town `fx_de_rez_fireplace`, 22 sprites): no
validation errors; the first lit layers read back finite, red 0.24..2.79
(mean about 2.5, 1.7, 1.2: warm, by the fireplace). Not compared with the
game: the light's scale against the game's and whether `spriteExtra`'s
words 3..5 are the edges the exe writes (read off the program, not off
the CPU writer) are unverified, so it stays opt-in.

### What the program reads (findings)

`fx_compute_sprite_light` (core_patch; `_shadow` and `_lmap` variants;
translated with `t7_cs_bindings` `T7_CS_MAKE=1 T7_CS_WGSL=<dir>`) lights
one sprite per thread group (`z` = sprite, `cb1.d[82].x` = the count), 64
texels per sprite from an 8x8 table of texel positions, and stores them in
`lmapTex` (`RWTexture2DArray<float4>`, Rgba16Float, layer = sprite) at
the table's (x, y): one small light map layer per sprite, which the
effect pixel program samples (layer from the record's 0x1c high half). It
reads `spriteExtra` (48 bytes per sprite; words 0..2 a world position,
word 3 two halves, words 4..5 four halves (the quad's axes), word 7 flags
(bit 28: sun shadow)) and `spriteData` (64 bytes), and **writes
`spriteDataW`**: the record the vertex program reads is the shader's
repack of `spriteData` (words 0 and 6 and bytes 32..63 kept, the rest
repacked as halves), so the CPU's `spriteData` is not the vertex record
`fx` packs. Lighting: the map's lights and probes (`LightingPackCBuffer`,
`RefProbe*PackCBuffer`, `visibleLights`/`visibleProbes`), the probe
volume `gProbeX/Y/ZArray`, `gCookieArray`, the sun shadow map and tree,
transparent shadows. Porting it needs the CPU writer of `spriteExtra` and
`spriteData` (not decompiled); until then `lmapTex` is one layer of 1.0.

## Next for effects

1. Compare on the PC (on by default again since the particle rules above): Nuk3town's data streams and
   de-rez colours, Evac's wind tails/lines (now drawn), their brightness;
   then `BO2MP_T7_FX_LIGHT=1` (lit smoke) and `BO2MP_T7_FX_MODELS=1`
   against the game before turning them on.
2. Sprite lighting: confirm `spriteExtra` against the exe's CPU writer.
3. Trails, emitted/impact effects; lens flares (the source list builder
   and `gEmitterStatic`); exploders on a map that places them (and the
   script side that fires them).
