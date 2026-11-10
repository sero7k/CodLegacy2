# Call of Duty: Ghosts (IW6 MP, PC x64) - lighting and final image: behavioural spec

Method: static reading of the exe image and the decompiled C only. Every statement is tagged
CONFIRMED (read directly from code or data), INFERRED (follows from code but a piece is
missing, or by analogy) or UNKNOWN. Addresses are only in the Evidence section (section 12).
Companion files: `const_src_code.tsv`, `texture_src_code.tsv` (same directory).

## 0. What is in the exe and what is not

In the exe (CONFIRMED): the CPU side of the renderer. Dvar defaults and ranges, the vision-set key
table, fog/colorization/tone-map/film constant preparation, light-grid decoding, the SH to
56-sample conversion, light-grid tweaks, shadow setup, the order of post passes (as names),
and the two name tables (const and texture sources).

NOT in the exe (UNKNOWN to this study, lives in zone `.ff` files): all shader bytecode (so the
exact per-pixel BRDF, lightmap decode, tone-map application order inside `postfx_color_grad_hdr`,
CLUT lookup maths, SSAO/SSR shaders), the `tonemapping`, `colorization`, `colorizationset`
assets (the Hable constants and grade values per vision set), the GfxWorld/ComWorld contents
(sun colour/direction/intensity, lightmaps, light-grid raw data, reflection probe images and
origins), vision files (`vision/<name>.vision` text) values per map. The exe tells you the
layout the CPU expects and the maths it does with them; the numbers per map must come from the zones.

Two correction notes against the task brief: the const table has 363 slots, not 347 (16 slots
carry the identical string `REACTIVEMOTION_CENTERS`, so 348 distinct strings; `HDR_SKY_INTENSITY`
is misspelt `CONST_SCR_CODE_...` in the exe). The texture table has 65 entries (0..64), not 68
(`TEXTURE_SRC_CODE_TEXTURE_0..3` are placeholder sampler-name strings assigned to the
`POSTFX_TEXTURE_0..3` slots, indices 46..49; they are not extra enum members).

## 1. CONST_SRC_CODE / TEXTURE_SRC_CODE enums

- Texture enum order: CONFIRMED. A runtime init function fills a 65-slot table of sampler-name
  strings; slot i (0-based) = `0x1480e8658 + 8*i`. Every assigned slot lines up with the static
  name table in `.data` (`TEXTURE_SRC_CODE_*`, 65 entries). 46 slots carry a shader-facing sampler
  name (e.g. 6 `shadowmapSamplerSun`, 14 `hdrSceneMipmapSampler`, 51 `clutMapSampler`, 40
  `macroclutSampler`, 56 `dynamicIrisExposure`, 54 `sceneLuminance`); the rest are bound by the engine only.
- Const enum order: the static name table at `0x1409f3b70` (363 slots). Evidence that table order
  equals the enum / vec4-slot order: the const array in the render structs is addressed as
  `base + 16*index`, and every probed writer matches the table (see anchors in the TSV):
  dynamic lights 7 slots each (indices 0..27), BLOOM_HQ_METERING_SENSITIVITY (99) written from its
  dvar, SSR matrix rows (133..136), rim lights (162..166), fog block (170..179), film block
  (183..186), colorization (193), tone map block (217..225). Two struct bases exist: the cmd-buffer
  input struct starts the array at +0 (fog written at +0xaa0 = 170*16), the per-view post struct
  starts it at +0x370 (film bias at +0xee0 = 0x370+183*16). Confidence: CONFIRMED for the anchored
  indices, HIGH (same table, same order) for the rest. Do not rely on indices of the matrix tail
  (289..362) beyond the names.

Lighting-related entries (0-based index, name; full list in TSV):

| idx | name | meaning |
|---|---|---|
| 0-6 / 7-13 / 14-20 / 21-27 | LIGHT_POSITION, DIFFUSE, SPECULAR, SPOTDIR, SPOTFACTORS, FALLOFF_PLACEMENT, CUCOLORIS_ANIM (light 0..3) | 7 vec4 per dynamic light, 4 lights max (`r_dlightLimit` 4) |
| 28-55 | LIGHT_*_DB_ARRAY_0..3 | same fields for the 4-light batch arrays |
| 56-58 | LIGHT_DYN_COUNT, LIGHT_DYN_TYPES, LIGHT_DYN_SHADOW_TYPES | |
| 98-102 | BLOOM_HQ_RADIUS, METERING_SENSITIVITY, IRIS_PARAMS, FALLOFF_1, FALLOFF_2 | HQ bloom + iris |
| 107 | MODEL_LIGHTMAP_PARAMS | |
| 108 | AVG_LUMINANCE_PARAMS | luminance chain |
| 109-114 | SSAO_* | |
| 133-139 | SSR_PREV_FRAME_VIEWPROJECTION_MATRIX_R0..R3, PREV_EYEPOSITION_TRANSFORM, SCREEN_SPACE_REFLECTION_FADE | |
| 157-160 | SHADOWMAP_SWITCH_PARTITION, SHADOWMAP_SCALE, ZNEAR, LIGHTING_LOOKUP_SCALE | |
| 162-166 | RIM_LIGHT_0_DIR/COLOR, RIM_LIGHT_1_DIR/COLOR, RIM_LIGHT_TECHNIQUE | |
| 167 | HDR_SKY_INTENSITY (`CONST_SCR_`) | |
| 170-179 | FOG, FOG_COLOR_LINEAR, FOG_COLOR_GAMMA, FOG_SUN_CONSTS, FOG_SUN_COLOR_LINEAR/GAMMA, FOG_SUN_DIR, FOG_HEIGHT (holds sky-fog angle params), FOG_SKY_DIR, FOG_DEPTHHACK | |
| 180-182 | GLOW_SETUP, GLOW_APPLY, GLOW_SETUP_ALT_COLOR_SCALE | |
| 183-186 | COLOR_BIAS, COLOR_TINT_BASE, COLOR_TINT_DELTA, COLOR_TINT_QUADRATIC_DELTA | film grade |
| 187-216 | MCOLOR_*, COLORIZATION_* (+ _ARRAY, _ALTERNATE, _LOW/MID/HIGH) | HDR colorization |
| 217-225 | TONE_MAPPING_* (9 vec4) | see section 2 |
| 226-232 | MSAA_RESOLVE_INV_TONE_MAP_C0..C6_PS | inverse tone map for MSAA custom resolve |
| 233-235 | OUTDOOR_FEATHER_PARMS, ENVMAP_PARMS, SUN_SHADOWMAP_PIXEL_ADJUST | |
| 236-242 | SPOT_SHADOWMAP_PIXEL_ADJUST (+_1,_2,_ARRAY_0..3) | |
| 254 | BLACK_LEVEL | |
| 255-256 | BASE_LIGHTING_COORDS, LIGHT_PROBE_AMBIENT | |

Textures (0-based): 3 MODEL_LIGHTING, 4/5 LIGHTMAP_PRIMARY/SECONDARY, 6 SHADOWMAP_SUN, 7-10
SHADOWMAP_SPOT(_1..3), 14 HDR_SCENE_R11G11B10_MIPMAP, 17-24 LIGHT_ATTENUATION/CUCOLORIS x4, 25
OUTDOOR, 38 REFLECTION_PROBE, 39 DALTONIZE_CLUT, 40 MACROCLUT, 45 RANDOM_ROTATIONS, 46 SSAO,
51 COLOR_GRADING_CLUT, 54 SCENE_LUMINANCE, 56 DYNAMIC_IRIS_EXPOSURE, 57-59 SCENE_VELOCITY(+PASS0/1).
(Note the TSV index column is the authority; the table above uses the same 0-based indices.)

## 2. Tone map and exposure

### 2.1 Curve (CONFIRMED)
Hable / Uncharted 2 filmic, per colour channel c in {r,g,b}, with per-channel parameters
(A shoulder strength, B linear strength, C linear angle, D toe strength, E toe numerator,
F toe denominator):

    f(x) = (x*(A*x + C*B) + D*E) / (x*(A*x + B) + D*F) - E/F          (per channel)

The CPU-side parameter block is 18 floats, order: D[3], E[3], F[3], B[3], C[3], A[3] (r,g,b each),
followed by the white point W[3]. The CPU computes `whiteScale_c = 1 / f(W_c)` (the stored white
point vec is replaced by 1/f(W) per channel; w = max(W)). Output consts (all vec4, xyz = rgb):

| const | value |
|---|---|
| TONE_MAPPING_TOE_STRENGTH_TOE_NUMERATOR | D*E |
| TONE_MAPPING_TOE_STRENGTH_TOE_DENOMINATOR | D*F |
| TONE_MAPPING_TOE_ANGLE | E/F |
| TONE_MAPPING_LINEAR_STRENGTH | B |
| TONE_MAPPING_LINEAR_ANGLE | C |
| TONE_MAPPING_TOE_SHOULDER_STRENGTH | A |
| TONE_MAPPING_WHITE_POINT | 1/f(W) rgb, w = max(W) |
| TONE_MAPPING_EXPOSURE | (expA, expB, expC, 1) - the three floats at the end of the block |
| TONE_MAPPING_WHITE_ADAPTATION_RATES | see 2.3 |

The shader is expected to evaluate `f(exposure * hdr) * whiteScale` (INFERRED: the shader is not in
the exe, but the pre-multiplied D*E, D*F, E/F terms are exactly the shape of the Hable formula).
The values of A..F, W come from the vision set's `toneMapping "<name>"` asset (UNKNOWN numbers, zone data);
if a vision file names none the asset `default` is used. Classic Hable defaults (A.15 B.50 C.10 D.20 E.02 F.30, W 11.2)
are the obvious starting point for a reimplementation but are NOT confirmed from the exe.
The tone map is disabled/neutral when the vision set has no HDR colourisation flag (exposure and white scale are set to 1).

### 2.2 Dynamic iris (auto exposure) - CONFIRMED structure, INFERRED details
- Target is a 1x1-ish render target (`dynamicIrisExposure`, texture index 56) updated by pass
  `update_dynamic_iris_adaptation` (or `_hq` when HQ bloom is on).
- Luminance measurement: NOT a histogram. Passes `avg_luminance_setup` -> `avg_luminance_downsample`
  (repeated box-style downsample chain, 256x256-tile granularity on the CPU side) ->
  `avg_luminance_accumulate`; result texture `sceneLuminance` (index 54). CONFIRMED passes/chain shape, INFERRED weights.
- HQ metering weights: `r_materialBloomHQSensitivity` vec4 default (2.5, 2.5, 1.0, 2.0), range 0..10:
  weights of the (left, right, top, bottom) screen zones (dvar help text). The vec4 is uploaded
  unchanged as BLOOM_HQ_METERING_SENSITIVITY. CONFIRMED.
- BLOOM_HQ_IRIS_PARAMS = (1/W0, 1/H0, 1/bloomHQGamma, ...) with W0,H0 the luminance RT size. CONFIRMED.
- Exposure range comes from the vision's exposure block: floats (min, max, third) -> TONE_MAPPING_EXPOSURE.xyz.
  If min < max AND both adaptation rates > 0: adaptive (iris pass runs, clamps to [min,max]).
  Otherwise the iris RT is cleared to the constant (min+max)/2 (fixed exposure). CONFIRMED.
- Adaptation rates (vision floats rateA, rateB) feed TONE_MAPPING_WHITE_ADAPTATION_RATES:
  non-HQ: (rateA, rateB, dt, viewIndex-derived texel coord). HQ: (rateA*3, rateA*0.5, dt, ...).
  The first frame after a vision/level change uses rate = FLT_MAX (instant snap) and dt = 1.0;
  afterwards dt = frame time delta in seconds, clamped >= 0. CONFIRMED values; semantics of x = up / y = down INFERRED (3x faster brightening vs 0.5x darkening is the likely reading).
- Numeric defaults of min/max/rates: UNKNOWN (zone asset).

## 3. Lighting data

### 3.1 Light grid (CONFIRMED unless stated)
- World cell size: 32 units in X and Y, 64 units in Z. Grid origin -131072 on each axis; positions
  are clamped to +-131072 first. Cell index = floor(p + 131072) >> 5 (x, y) and >> 6 (z).
  Fractions fx = ((x+131072)/32) frac, fy likewise, fz = ((z+131072)/64) frac.
- 8 corner trilinear weights: standard trilinear of (fx, fy, fz). Corners with weight < 0.001 or whose
  cell is empty or flagged not visible from the query point's region are dropped; remaining corner
  weights are renormalised (sum of kept weights divides). Corner data is stored sparsely (a cell
  lookup structure returns an entry index, 0 = no data; entries are deduplicated across corners).
- Two storage generations, selected by a flag in the grid header:
  1. Legacy: each entry = 56 RGB8 samples (168 bytes). Blend: corner weights quantised to sum
     exactly 256 (remainder goes to the largest weight), blended in 8.8 fixed point (ushort, 255.0 = 0xFF00).
  2. SH ("current-gen"): each entry = 16-bit header + three channel blobs. Header: 3 x 5-bit
     values (R bits 14..10, G bits 9..5, B bits 4..0) minus 8 = per-channel exponent e_c. Two
     grid-level thresholds t0 <= t1 choose channel precision: e_c <= t0 -> 8-bit coefficients
     (9 bytes: DC byte + 8 signed bytes), e_c <= t1 -> 12-bit (DC u16 + eight 12-bit packed
     = 14 bytes, 2-byte aligned), else 16-bit (9 x u16 = 18 bytes, 2-byte aligned).
     Decode with s = per-channel scale (a power of two derived from e_c; INFERRED 2^e_c):
     L0 = s * raw / 2^bits ; Lk (k=1..8) = raw * (2s / 2^bits) - s  (signed range [-s, s)).
     9 coefficients = L0, then (x,y,z) L1 order, then L2 order below.
- SH basis evaluation (CONFIRMED, no normalisation constants: coefficients are pre-scaled by the baker):
  with unit direction (x,y,z):
  `v = c0 + x*c1 + y*c2 + z*c3 + z*x*c4 + z*y*c5 + y*x*c6 + (3*z*z - 1)*c7 + (x*x - y*y)*c8`
  (per colour channel). Band count dvar `r_lightGridSHBands` (default 0 = all bands; 1 = L0 only;
  2 = +L1; 3 = +L2; i.e. 0 behaves like 3).
- The SH is baked every time into a 56-direction table (so models sample the same representation
  as legacy grids): directions = every vertex of a 4x4x4 lattice that lies on the cube shell
  (not both inner coordinates): (i,j,k in 0..3), skip when i,j,k are all in {1,2}; 64-8 = 56 samples,
  loop order k (outer), j, i (inner); direction = normalize((i*2/3-1, j*2/3-1, k*2/3-1)).
  Each sample stored as byte = min(255, (sqrt(max(v,0)) + bias) * 127.5), bias unreadable from the decompile (assumed 0);
  decode: L = (byte/127.5)^2, range 0..4 (a gamma-2 encoding). The 8.8 fixed path represents the same data as
  ushort/0xFF00*2 squared (constant 3.0637257e-05 = 2/65280).
- This 56x RGB8 table is what TEXTURE_SRC_CODE_MODEL_LIGHTING (index 3) holds per model. How the
  pixel shader indexes it (nearest of 56, or trilinear over the 4x4x4 cube) is UNKNOWN (shader).
- Tweak dvars (applied only when r_lightGridEnableTweaks AND r_lightGridUseTweakedValues, plus hero
  lighting enabled): `r_lightGridEnableTweaks` false; `r_lightGridUseTweakedValues` true;
  `r_lightGridIntensity` default 1.0 range [0,2]; `r_lightGridContrast` default 0.0 range [-1,1].
  Operation on the 168 values v (8.8 fixed): v = min(v * intensity, 0xFF00); m = mean(v) over all
  168 values (all samples, all channels); c = contrast + 1; v' = clamp(v*c + m*(1-c), 0, 0xFF00).
- Hero lighting (`r_heroLighting` true): entity "hero" lights (0x44-byte records) are added into the
  56 table as `colour * falloff * max(0, dot(sampleDir, dirToLight))`. The code takes abs-masks that
  decompile as artefacts, so exact form is INFERRED.
- Other light-grid dvars: `r_cacheModelLighting`, `r_cacheSModelLighting` true; `r_altModelLightingUpdate` true;
  `r_lightCacheLessFrequentPeriod` 10 (1..100), `r_lightCacheLessFrequentMaxDistance` 64;
  `r_FXAverageColorFunc` 2 (0 = iwRad equation, 1 = legacy, 2 = SH L0 coefficient) for FX average colour;
  `r_showLightGrid` 0..3, `r_showMissingLightGrid` rainbow outside grid.

### 3.2 Sun, primary lights (CONFIRMED where stated)
- Light colours are stored sRGB-gamma and converted to linear with the exact sRGB curve
  (x<=0.03928 -> x/12.92 [coded as x*0.07739938]; else ((x+0.055)/1.055)^2.4) when the build flag
  `DAT_147e225a0 < 0x3a` is set (true for this PC build path); fog colours get the same conversion.
- Diffuse scale `r_diffuseColorScale` default 1.0 (0..10); specular scale `r_specularColorScale`
  default 2.5 (0..25). Both multiply the light colour; per-light tweak scales apply when flag bits 1/3 of the cmdbuf are set
  (the `r_primaryLightTweak{Diffuse,Specular}Strength[_NG]` and view-model variants, all default 1.0, range 0..10, off by default).
- Sun direction/colour/intensity themselves come from ComWorld primary light 0 / GfxWorld sun data in the zone: UNKNOWN numbers.
  The prior repo note (suns 0.2..10 in baked units) is consistent with HDR units where the lightgrid max is 4.0.
- Env-map sun: `r_envMapSunIntensity` default 2.0 (0..4): cap on sun specular for env-map materials;
  `r_envMapMinIntensity` 0.2, `r_envMapMaxIntensity` 0.5, `r_envMapExponent` 5.0, `r_envMapOverride` false.
  These describe a Fresnel-like ramp: reflection intensity = lerp(min, max, (1 - n.v)^exponent) (INFERRED from the
  help text "Min reflection intensity based on glancing angle", "Reflection exponent"); the consts go through ENVMAP_PARMS (index 234).
- Sun flare / blind / glare (CONFIRMED defaults): flare min size 0, max size 2500 px (at 640x480, 80 fov),
  min angle 45 deg, max angle 2 deg, max alpha 1, fade in 1 s, fade out 1 s; sprite size 16; sun blind: min angle 30,
  max angle 5, max darken 0.75, fade in 0.5 s, out 3 s; sun glare: min angle 30, max angle 5, max lighten 0.75, fade in 0.5, out 3.
  Angles are stored as cos(angle); times as ms. Materials `sun`, `sun_flare`. `r_drawSun` true.
  `r_sun_from_dvars` false (level values used).

### 3.3 Sun shadow (CONFIRMED)
- Cascades: 2 by default; 4 in the "high" mode (selected when the draw method/params and a
  quality predicate allow). They share ONE shadowmap atlas laid out horizontally:
  atlas size = (n * res) x res, n = 2 or 4. SUN_SHADOWMAP_PIXEL_ADJUST xyzw = (0.25/res, 0.5/(n*res), 0.5/res, -0.25/(n*res)).
- Effective resolution = 2 * ceil(res * sm_sunShadowScale * 0.5) (even); `sm_sunShadowScale` default 1.0
  (0.25..1; high mode doubles the scale capped at 1). `sm_sunShadowScaleLocked` true forces 1.
- Near-cascade texel size in world units: `sm_sunSampleSizeNear` default 0.25 (range 1/64..32);
  `sm_sunShadowCenterMode`, `sm_fastSunShadow` true, `sm_strictCull` true, `sm_cameraOffset` 0.
  Base resolution values (res) are UNKNOWN (runtime globals, set from quality settings).
- Split scheme (distances per cascade), blend widths, per-cascade bias: UNKNOWN (per-map data / shader constants SHADOWMAP_SWITCH_PARTITION idx 157, SHADOWMAP_SCALE idx 158).
- Depth bias for shadow pass is a raster state: DepthBias = -`sm_polygonOffsetBias` (default 128, 0..8192),
  SlopeScaledDepthBias = -`sm_polygonOffsetScale` (default 2.0, 0..8192), DepthBiasClamp = -`sm_polygonOffsetClamp` (default 0).
  The negated signs hint at a reversed-Z or inverted offset convention (INFERRED). Decals use `r_polygonOffset{Scale -1, Bias -1, Clamp 0}`
  multiplied by the decal layer count (1..3).
- Spot shadows: up to 4 (`sm_spotLimit` 4, `sm_maxLightsWithShadows` 4), score threshold `sm_minSpotLightScore` 0.5,
  fade `sm_spotShadowFadeTime` 1.0 s, distance cull on, entity shadows on, `r_spotLightShadows` true.
  PCF width / filter kernel: UNKNOWN (shader).

### 3.3a Sun shadow casters and cascade centres (2026-11-03, read for `render_iw6` frame cost round 4)
- Cascade centre snap (`FUN_14061d1f0`): per partition, centre along the light's two axes =
  `(floor(viewOrigin . axis / texel) + 0.5) * texel` (texel = the partition's world units per texel,
  `param_1[0x18 + partition]`; made in `FUN_14060eba0` (`res * sm_sunSampleSizeNear / ...`) and `FUN_14061c9a0` (`sampleSize / sm_sunShadowScale`, x4 for
  the far partition). With `sm_sunShadowCenter` set (non-zero) the far partition (or both with
  `sm_sunShadowCenterMode`) is centred on it instead, snapped the same way; `sm_cameraOffset` > 0 moves the
  origin along the view. No sun shadow cache dvar exists (`dvars_all.tsv` has no `sm_cache*`).
  (`render_iw6` keeps its own far ratio 8, INFERRED earlier; the exe's x4 is noted, not applied.)
- World surface caster bits, built at load (`FUN_1405d9190`): for each surface `i` below the world
  model's surface count (`models[0].surfaceCount`, GfxWorld+0x760 -> +0x38) whose material's draw-surf
  word has bits 0x3e000000 and whose `GfxSurface.laf.fields.flags & 1`: set bit `i` of
  `dpvs.surfaceCastsSunShadow` (+0x988); then for each bit `k` of `flags >> 3` (`k < sunShadowOptCount`)
  set bit `i` of `surfaceCastsSunShadowOpt[k]` (+0x998, stride `sunSurfVisDataCount` words).
- Which bits the sun partitions draw (`FUN_1405b0bf0`): `surfaceCastsSunShadowOpt[min(light,
  optCount) - 1]` when `r_useShadowGeomOpt` (default true) and the opt arrays exist and the light index
  (`viewParms+0x1839`: the first visible sun primary light, `FUN_140601880`, 1..`lastSunPrimaryLightIndex`)
  is <= `lastSunPrimaryLightIndex`; else `surfaceCastsSunShadow`. Consecutive sorted surfaces with the same
  material and adjacent indices are merged into one draw (`FUN_1405af430`).
- Sun partition placement cull (`FUN_1405b2170`, writes the partition's static model list): skip
  `GfxStaticModelDrawInst.flags & 0x10`; with the opt data and `viewParms+0x1838` set, skip when
  `(sunShadowFlags & (1 << (min(light, optCount) - 1))) == 0`; skip models with `XModel.flags & 2`
  (`+499`); cull distance as the scene's. When the per-partition sub-lists are active (`+0x1d24`, counts at
  `0x151ab0`) this list keeps only `flags & 0x800` placements and `FUN_1405b5ea0` fills the sub-lists with
  the rest (`flags & 0x810 == 0`, same sun-flag mask).
- Primary light shadow geometry (`FUN_1405b2810` and siblings over `shadowGeom` / `shadowGeomOptimized`,
  GfxWorld+0x888 / +0x890): placements with `flags & 0x10` or model flag 2 are skipped.
- mp_frag data: sunShadowOptCount 1, lastSunPrimaryLightIndex 1, world model 17,819 surfaces; surface flags
  {0: 275, 1: 13,686, 8: 216, 9: 3,824}; placements with 0x10: 3,706 (all with sunShadowFlags 0), without:
  1,577 with sunShadowFlags bit 0, 3,496 without; models with flag 2: 29.

### 3.4 Static models vs world, lightmaps (mostly UNKNOWN)
- World surfaces: lightmap textures `LIGHTMAP_PRIMARY` (4) and `LIGHTMAP_SECONDARY` (5) only. The enum has no third
  or SH-lightmap slot, so any directional/SH decomposition lives inside these two textures' channels
  (INFERRED from the enum; consistent with the repo notes that the secondary pages carry sun-visibility).
  `r_lightMap` debug: Black/White(/other) replaces lightmaps by constants. Decode maths: UNKNOWN (shader).
- Models (dynamic and static): lit by the 56-sample table from the light grid (3.1) bound as MODEL_LIGHTING, plus
  dynamic lights and the sun when shadowed. Static models can additionally carry per-vertex baked light or probe data (UNKNOWN, zone).
- A per-model light-grid sampling budget exists ("Exceeded max lightgrid sampling count for FX per frame", "Lightgrid patch list overflow, model lighting will be incorrect").
- Reflection probes: texture slot REFLECTION_PROBE (38) plus `r_texFilterProbeBilinear` (false) and the debug dvar
  `r_showReflectionProbeSelection` (enum). Selection and parallax code was not located in the exe by dvar
  or string; UNKNOWN. LIGHT_PROBE_AMBIENT (const 256) exists as a per-draw ambient colour.

## 4. Fog (CONFIRMED)

Script call `setexpfog(startDist, halfwayDist, r, g, b, [hdrIntensity], maxOpacity, transitionTime,
[sun: r, g, b, [intensity], dirVec3, beginFadeAngle, endFadeAngle, scale], [sky: intensity, minAngle, maxAngle])`.
Constraints: startDist >= 0, halfwayDist > 0, maxOpacity in [0,1], sky angles in [-90,180].
Readback dvars: g_fogStartDistReadOnly 0, g_fogHalfDistReadOnly 0.1, g_fogMaxOpacityReadOnly 1, g_fogColorIntensityReadOnly 1,
g_sunFogBeginFadeAngleReadOnly 0, EndFade 180, g_sunFogScaleReadOnly 1, g_sunFogColorIntensityReadOnly 1.
Derived: k = ln 2 / halfDist. Fog state is interpolated over transitionTime between two states.

Constants produced each frame (r_fog true, else FOG = (0,1,0,0)):
- FOG = (0, 1 - maxOpacity, -k, k * start). Expected use (INFERRED): transmittance T = exp(-k*(d - start)) clamped, floor at 1-maxOpacity;
  i.e. visibility halves every halfDist beyond startDist.
- FOG_COLOR_LINEAR = srgbToLinear(colour) * hdrIntensity, w = alpha byte/255. FOG_COLOR_GAMMA = colour bytes / 255.
- Sun fog (when enabled): FOG_SUN_CONSTS = (-k * scale, cos(endAngle), 1/(cos(beginAngle) - cos(endAngle)) [100 if begin<=end... ], -k).
  FOG_SUN_COLOR_LINEAR = srgbToLinear(sunColour) * sunIntensity; FOG_SUN_COLOR_GAMMA = raw; FOG_SUN_DIR = normalised direction.
  Blend between fog colour and sun-fog colour by smoothstep-like ramp of cos(angle to sun dir) between the two cos values. (INFERRED)
- Sky fog (FOG_HEIGHT slot): (minAngle_rad, 1/(maxAngle_rad - minAngle_rad) [maxAngle bumped +0.01 if <= min], intensity * maxOpacity, 0).
  Dvar overrides: `r_skyFogUseTweaks`, `r_sky_fog_min_angle` 0, `r_sky_fog_max_angle` 90, `r_sky_fog_intensity` 0.
- FOG_SKY_DIR: a fixed vector rotated into view space (used for the combined postopaque pass). FOG_DEPTHHACK: viewmodel fog scale
  (`r_fog_depthhack_scale`, -1 = disabled).
- Fog is applied in the combined "postopaquefx" pass (variants `_fog`, `_dfog` (depth fog), `_color`, `_ssaolq`, `_ssaohq`, `_msaa_*`), i.e.
  fog + colorization + SSAO are applied after the opaque pass in one fullscreen shader (`r_combinePostOpaqueFx` true), not per material.
  `r_zfar` (0 = off) overrides the 100%-opacity distance.
- HDR sky: `r_hdrSkyIntensity` default 1.0 (0..10), tint `r_hdrSkyColorTint` (vision key), const CONST_SCR_CODE_HDR_SKY_INTENSITY (167);
  `r_hdrSkyIntensityUseTweaks` false. Lit/unlit surface scalars `r_litSurfaceHDRScalar`, `r_unlitSurfaceHDRScalar` default 1.0 (0..10).

## 5. Colour grading and film (CONFIRMED)

Vision-set text keys and byte offsets in the resolved vision struct (table of 57 keys; vec3 keys are 3 floats):

| offset | key | type |
|---|---|---|
| 0x00 | r_filmEnable | bool |
| 0x04 | r_filmBrightness | float |
| 0x08 | r_filmContrast | float |
| 0x0c | r_filmDesaturation | float |
| 0x10 | r_filmDesaturationDark | float (default sentinel -1e8 -> equals r_filmDesaturation) |
| 0x14 | r_filmInvert | bool |
| 0x18 / 0x24 / 0x30 | r_filmLightTint / r_filmMediumTint / r_filmDarkTint | vec3 (medium sentinel -1e8 -> average of light and dark) |
| 0x3c.. | r_primaryLightUseTweaks_NG, TweakDiffuseStrength_NG 0x40, TweakSpecularStrength_NG 0x44, r_charLightAmbient_NG 0x48 (vec3), r_viewModelPrimaryLight*_NG 0x54/58/5c, r_viewModelLightAmbient_NG 0x60 (vec3) | |
| 0x6c | r_lightOverride | float |
| 0x70-0x90 | r_materialBloomHQEnable 0x70, HQHaziness 0x74, HQGamma 0x78, HQDesaturation 0x7c, Intensity 0x80, Radius 0x84, Pinch 0x88, LuminanceCutoff 0x8c, Desaturation 0x90 | |
| 0x94-0xb0 | r_volumeLightScatter (bool), LinearAtten, QuadraticAtten, AngularAtten, DepthAttenNear, DepthAttenFar, BackgroundDistance, Color (vec3 0xb0) | |
| 0xbc,0xc0 | r_ssaoStrength, r_ssaoPower | |
| 0xc8-0xe8 | r_glare_enable, normal/mirror Start,End,Power,Amount | |
| 0xec-0x104 | r_rimLight0Pitch_NG, Heading_NG, DiffuseIntensity_NG, SpecIntensity_NG, Bias_NG, Power_NG, Color_NG (vec3) | |
| 0x114,0x118,0x11c,0x120 | r_unlitSurfaceHDRScalar, r_litSurfaceHDRScalar, r_hdrSkyIntensity, r_hdrSkyColorTint (vec3) | |
| (special) | r_glow, colorizationSet "<name>", toneMapping "<name>", a material name (CLUT/mat) | colorizationSet and toneMapping resolve to zone assets (`ASSET_TYPE_COLORIZATIONSET` 0x39, `ASSET_TYPE_TONEMAPPING` 0x3A) |

Film grade formula (alt shader, `r_filmAltShader` true; the CPU stage computes constants):
- contrast c = vision.contrast * `r_contrast`(1.0); brightness b = vision.brightness + `r_brightness`(0.0);
  desat d' = ((1 - d)*`r_desaturation`(1.0) + d) * d where d = vision.desaturation;
  `r_blacklevel` default 0 (-0.99..0.99). If film or colour grading is disabled everything resets to identity.
- Let den = max(0.01, 1 - blacklevel); s = c / den; bias = (b + 0.5) - c*0.5 - blacklevel/den; invert flips s = -s, bias += 1.
- COLOR_BIAS = (bias, bias, bias, 1/max(d', 2^-12) - 1)  [w term 2^-12 = 0.000244].
- Mode 0 (legacy 2-tint): TINT_BASE = light*s, TINT_DELTA = (dark - light)*s, QUADRATIC = 0.
- Mode 1 (3-tint, default): TINT_BASE = light*s; TINT_DELTA = 2*s*(medium - light); TINT_QUADRATIC = s*(light - 2*medium + dark)
  i.e. quadratic Bezier light -> dark with medium as the control point. Alpha components: BASE.w = darkDesat,
  DELTA.w = d' - darkDesat (desaturation also interpolates dark -> light). (Mode 2 is a script override, treated like mode 0 except tinted differently; INFERRED.)
- Tweak dvars (`r_filmUseTweaks`, `r_filmTweakEnable`, `r_filmTweakContrast` 1.4, `r_filmTweakBrightness` 0, `r_filmTweakDesaturation` 0.2,
  `r_filmTweakDesaturationDark` 0.2, `r_filmTweakInvert` false, tint vec3 dvars) replace the vision values.
- Multi-colour-correction mode (`r_cc_mode` 0 = off; dvars `r_cc_desaturation1/2` 0, `r_cc_lerp1` 0, `r_cc_lerp2` 5000 (world units: distance-based
  blend between two grades), `r_cc_*` colour vec3 dvars) writes MCOLOR_* consts (idx 187..192) at post +0xf20..+0xf7c: near/far grade, blended over
  [lerp1, lerp2] by depth (`1/max(lerp2-lerp1, 0.1)`).
- Colorization (HDR "colorization set", per-vision): 0x178-byte block copied into the post struct and expanded into COLORIZATION_* consts
  (power&desaturation, scale, bias, low/mid/high end points, alternate mask set; `r_hdrColorizationBeforeEmissive` true). Field values: zone asset (UNKNOWN).
- CLUT: textures `COLOR_GRADING_CLUT` (51, sampler clutMapSampler), `MACROCLUT` (40), `DALTONIZE_CLUT` (39). The "macroclut" optimisation
  (`r_postfxMacroclutEnable` true, built on a worker thread `r_postfxMacroclutWorker` true) bakes film + colorization + CLUT steps into one CLUT
  applied by passes `postfx_macroclut(_watersheeting/_digital)`; otherwise `postfx_color_grad_hdr(_altd/_altm/_altdm)`, `_clut`, `_lmh` variants are used.
  CLUT dimensions and lookup maths: UNKNOWN (zone texture + shader).

## 6. Bloom, glare, flare, SSAO, SSR, DOF, volumetric

- Material bloom (legacy, `r_materialBloom*`): Radius 2.0 (0..32), Pinch 0 (-1..1), Intensity 0 (0..16), LuminanceCutoff 0.8, Desaturation 0.
  Vision overrides unless `r_materialBloomUseTweaks`.
- Material bloom HQ (`r_materialBloomHQEnable`, vision-driven, `r_materialBloomHQScriptMasterEnable` true): Gaussian pyramid of 6 levels.
  Passes `material_bloom_hq_setup_alpha_intensity`, `material_bloom_hq_blur` (x then y per level), `material_bloom_apply_bloomhq`
  (`_bloomglare` when glare merges). Level n radius = `HQRadius`(0.75, 0..1.5) * 0.01 * 2^n * scale_n; scale vec3 dvars:
  `FalloffScale1` default (0.7, 0.8, 0.9) for levels 0-2 (range 0..2), `FalloffScale2` default (1,1,1) for 3-5.
  Level weights: `FalloffWeight1` (0.25, 0.5, 0.75) levels 0-2, `FalloffWeight2` (1.0, 1.25, 1.5) levels 3-5 (range 0..10); the CPU normalises the
  three/six weights to sum to 1 (w_i / sum). Haziness (linear scale) default 0.2 (0..10), Gamma (threshold-like) 1.5 (0..4), Desaturation 0 (0..1).
  Bloom is zeroed in views where the scope/PiP flags are set.
- Glare/lens glare (`r_glare_*`): disabled by default; masks use luminance start/end (0.125/0.75 default), power 1, amount 1; blend merges into
  the bloom pass (`r_glare_mergeWithBloom` true). Passes glare_brightpass(1,2), downsample, blur_x/y, expand.
- Volumetric light scatter (`r_volumeLightScatter` false; linear atten 1.0, quadratic 0, angular 0.25, depth near/far 0, background dist 100000):
  passes volume_light_scatter_blur0..2, apply, apply_depth_atten. Constants VOLUME_LIGHT_SCATTER_1..3.
- SSAO: modes `r_ssao` (enum, default index 1), `r_ssaoStrength` 3.0, Power 1.0, Diminish 1.0, Width 36 (px at 720p), DepthScale 0.1 (viewmodel 0.0003),
  BlurSharpness 50, BlurStep 1, GapFalloff 0.25, GradientFalloff 0.05, FadeDepth 2500, RejectDepth 2500, MinPixelWidth 1, ScriptScale 1.
  Vision supplies strength/power; `r_ssaoUseTweaks` switches to dvars. HBAO alternative (`r_hbao*`: Radius 1.1, Bias 0.2, DetailAO 0.5, CoarseAO 1.0,
  PowerExponent 1.1, Blur sharpness 8, SceneScale 16, StrengthScale 1). SSAO is composed in the postopaquefx pass (variants ssaolq/ssaohq). Bilateral blur + random-rotation texture (index 45).
- SSR: consts SSR_PREV_FRAME_VIEWPROJECTION_MATRIX_R0..R3 = previous frame view-projection rows, PREV_EYEPOSITION_TRANSFORM = (eye - prevEye) *
  `r_ssrPositionCorrection` (0.5, -2..2) with w = correction, SCREEN_SPACE_REFLECTION_FADE. CONFIRMED. The source buffer is the previous frame (reprojection).
  `r_ssrFadeInDuration` 0.05 s: fade = min(1, fade + dt/0.05) each frame; the fade is reset to 0 when the reflection source buffer is invalidated by a view change, specifically
  dual-view scope (PiP) transitions, and set to 0 entirely while a secondary view is rendered. CONFIRMED.
- DOF: `r_dof_enable` true, near blur 6.0 (4..10), far blur 1.8, viewmodel start/end 2/8 in, near start/end 10/60, far start/end 1000/7000, bias 0.5 (0.1..3), HQ mode enum; passes dof_hq_*.
- Motion blur HQ: `r_mbEnable` per-object velocity RTs (scene velocity textures 57-59), camera rotation/translation influence 1.0, samples 15 max (`r_mbPostfxMaxNumSamples`), view-model enable true.
- Anti-aliasing: SMAA (threshold 0.1), FXAA (subpixel 0.33), MSAA with custom resolve (`msaa_resolve_pre/post/custom2/custom4`) that applies the tone map before the hardware resolve and its inverse after (consts MSAA_RESOLVE_INV_TONE_MAP_*).

## 7. Per-pixel lighting equations (largely UNKNOWN; what the exe constrains)

- Shader bytecode is in zone files, so the BRDF is NOT directly readable. Evidence that constrains it:
  - Specular boost `r_specularColorScale` 2.5 (default) on top of a 'specular colour' multiplication (spec colour * spec colour scale) means the engine keeps the classic
    Blinn-Phong-style `specular colour * gloss-derived exponent` workflow (the light's SPECULAR const is a separate colour from DIFFUSE; spec is an additive term).
    There is no metalness/roughness (PBR) input anywhere: no such const or texture enum slot, and the debug dvar is `r_specularMap` (Black = off, White = "super shiny"). CONFIRMED absence; Blinn-Phong form INFERRED.
  - Ambient/specular from probe: TEXTURE_SRC_CODE_REFLECTION_PROBE (cube) sampled with the env-map ramp (min 0.2, max 0.5, exponent 5, sun cap 2.0 -> section 3.2). Probe selection: UNKNOWN.
  - Rim light (character/viewmodel only): direction from pitch/heading (`r_rimLight0Pitch` -44.5, heading -39 defaults; _NG vision keys), diffuse and spec intensity
    (1.0), `r_rimLightBias` 0, `r_rimLightPower` 1 (0.5..64): n.l computed as pow(clamp(n.l + bias), power) (INFERRED from dvar texts "How much to bias the n.l calculation", "Power to raise the n.l calculation").
    Colour constants: RIM_LIGHT_0_COLOR = rimColor * ((ambientRadius + 8) / 25.13) * diffuseIntensity and RIM_LIGHT_1_COLOR with specIntensity, w of dir = specular exponent-like term. CONFIRMED CPU math.
  - Gloss to exponent mapping, Fresnel, energy conservation: UNKNOWN (shader). Alpha of the specular map carries gloss in the repo's current understanding; unchanged.
- Dynamic light constant packing (CONFIRMED): per light slot 0 position (xyz relative to eye... position minus camera origin; w = 1/radius), 1 diffuse colour (xyz) ,
  2 specular colour, 3 spot dir (xyz) + w, 4 spot factors, 5 falloff placement, 6 cucoloris animation. Colours are sRGB->linear converted (3.2).
- HUD 2D lighting (`cg_hudLighting_*`): basic spec exponent 33, blood spec exponent 100; ambient/diffuse/spec/additive lum scale+offset dvars.

## 8. Summary of reimplementation recipe (what is safe to copy now)

1. Fog: use setexpfog maths above (k = ln2/half, FOG consts, sun fog cone via cos angles, sky fog angles). Colours: sRGB -> linear, times HDR intensity.
2. Tone map: Hable per channel with parameters from vision (tonemapping asset) and 1/f(W) normalisation; exposure vec3 and adaptive iris with min/max and rates (snap on first frame).
3. Film grade: bias/tint Bezier above, then CLUT.
4. Light grid: 32x32x64 cells, origin -131072, trilinear, 56-sample cube-shell table, sqrt(2.0) encoding L=(b/127.5)^2.
5. Bloom HQ: 6-level pyramid, weights and scales above.

## 9. List of what could NOT be determined

- Per-map numbers: sun colour/direction/intensity, Hable parameters, exposure min/max, rates, colorization sets, vision-file values, shadow split distances, sun shadow base resolution.
- Shader maths: BRDF (gloss->exponent, Fresnel), lightmap decode (primary/secondary), directional-lightmap/SH bands in lightmaps, how MODEL_LIGHTING is indexed per pixel, CLUT lookup, SSAO/SSR shader, fog application order.
- GfxWorld/ComWorld struct layouts (header fields for the light grid ranges/sample counts besides what the sampling code implies, sun data, probe tables).
- Reflection probe selection/parallax correction.
- The small unreadable constant added before the *127.5 in the SH->byte encoder (decompile shows a runtime global).
- Whether the `3 * dvar rates` HQ iris variant is up or down speed.

## 10. Reusable existing project values

The repo notes (suns 0.2..10, light percentile exposure) are consistent with the 0..4 lightgrid range: a proper reimplementation
can drop per-map exposure fitting once the vision `toneMapping` asset (Hable A..F, W, exposure min/max) is read from the zones.

## 11. Dvar default table (excerpt, CONFIRMED from registration sites)

See `const_src_code.tsv` for indices. Key dvars: r_specularColorScale 2.5 [0,25]; r_diffuseColorScale 1.0 [0,10]; r_lightGridIntensity 1.0 [0,2]; r_lightGridContrast 0 [-1,1];
r_lightGridSHBands 0 [0,3]; r_envMap{Min 0.2, Max 0.5, Exponent 5.0, SunIntensity 2.0}; sm_sunShadowScale 1.0 [.25,1]; sm_sunSampleSizeNear 0.25; sm_polygonOffsetScale 2.0 / Bias 0x80 / Clamp 0;
r_materialBloomHQ{Haziness .2, Gamma 1.5, Desat 0, Radius .75, Sensitivity (2.5,2.5,1,2)}; r_ssrPositionCorrection 0.5; r_ssrFadeInDuration 0.05; r_blacklevel 0;
r_filmTweakContrast 1.4; r_dlightLimit 4; r_znear 4.0; r_fxaaSubpixel 0.33; r_smaaThreshold 0.1.

## 12. Evidence (addresses)

- Texture runtime name table init: `TEXTURE_SRC_CODE_DALTONIZE_CLUT` @ 0x14062a840 (fills `0x1480e8658 + 8*i`).
- Static name tables (.data): textures 0x1409f3960 (65), consts 0x1409f3b70 (363).
- Vision key table: 0x1409e22d0 (57 entries x 32 bytes: name, desc, offset, type); parser `use_existing_hdr_colorization` @ 0x1402a48f0, loader `default_5` @ 0x1402a4700.
- Film consts: FUN_140610af0 @ 0x140610af0; film inputs FUN_14060cfa0 @ 0x14060cfa0; bloom inputs FUN_14060d580; glare inputs FUN_14060d3b0; cc grade FUN_1406118f0 @ 0x1406118f0.
- Tone curve: FUN_140609910 @ 0x140609910 (Hable); constants block FUN_1406102f0 @ 0x1406102f0 (writes 0x1100..0x116c and colorization 0xf80..); caller FUN_14060cca0 @ 0x14060cca0;
  exposure/rates FUN_14016d680 @ 0x14016d680; iris pass driver FUN_14016d360 @ 0x14016d360; luminance chain FUN_14063d260 @ 0x14063d260.
- Fog: FUN_1406333b0 @ 0x1406333b0 (consts), fog state load FUN_140611440 neighbour at 0x140610... (state copy near line 869518), setexpfog builtin FUN_1403c5140 @ 0x1403c5140.
- Light grid: weights FUN_140638a40 @ 0x140638a40, caller FUN_1406356d0 @ 0x1406356d0, blend FUN_1406362d0 @ 0x1406362d0, SH decode FUN_1405fb010 @ 0x1405fb010
  (+ FUN_1405f9a00 / FUN_1405f9760 / FUN_1405f98e0), SH->56 samples FUN_1405fcc20 @ 0x1405fcc20, direction table builder FUN_1405fa200 @ 0x1405fa200 (table at 0x148110cb0),
  hero lights FUN_1405fa310 @ 0x1405fa310, tweaks FUN_140635ae0 @ 0x140635ae0, application FUN_140636080 @ 0x140636080.
- Light consts: FUN_1405cb870 @ 0x1405cb870, FUN_1405cba10 @ 0x1405cba10, sRGB->linear FUN_1405cb7a0 @ 0x1405cb7a0.
- Sun shadow setup: FUN_14060eba0 @ 0x14060eba0; raster offsets FUN_1405e6480 @ 0x1405e6480.
- SSR: FUN_14060e280 @ 0x14060e280 (consts), FUN_140261f40 @ 0x140261f40 (fade-in), fade reset near 0x1402... (line 218884).
- Rim light: FUN_140611b40 @ 0x140611b40.
- Dvar registration: developer_2 @ 0x1405d9a70 (render dvars), r_specularColorScale @ 0x1405de310, r_sunsprite_shader @ 0x140613fc0 (sun flare/blind/glare); parsed table dumped to `dvars_all.tsv` (same scratchpad).
- Post pass names (strings 0x1408bb...): avg_luminance_*, update_dynamic_iris_adaptation(_hq), material_bloom_*, postfx_color_grad_hdr*, postfx_macroclut*, postopaquefx_*.

## 8. Reflection probes in the GfxWorld (zone data, read from `mp_frag.ff`, 2026-11-03)

Captured by the world walk (`fastfile_iw6::walk::GfxWorldRec::reflection_probes`, `ReflectionProbeRec`); the walk's
stream order is unchanged. Field offsets come from `layout.rs` (GfxWorld `draw__reflectionProbe*`, 136..184).
- `reflectionProbeCount` (u32 at draw+136), then in stream order: `reflectionProbes` (one image pointer per probe,
  each an inline Image asset), `reflectionProbeOrigins` (`GfxReflectionProbe[count]`, 32 bytes each: 16 bytes of
  origin at +0, `probeVolumes` pointer at +16, `probeVolumeCount` u32 at +24; the volume arrays follow each record,
  96 bytes per `GfxReflectionProbeVolumeData`), `reflectionProbeTextures` (runtime GfxTexture slots), then
  `reflectionProbeReferenceCount` (u32 at draw+168), `reflectionProbeReferenceOrigins` (`vec3[count]`) and
  `reflectionProbeReferences` (`u8[count]`, the probe index for each reference position).
- mp_frag: 22 probes, images `*reflection_probe0` .. `*reflection_probe21` (map_type 5 = cube). Probe 0 is at (0,0,0)
  with a different format (DXGI 28, RGBA8) than the rest (DXGI 10, RGBA16F): it reads as the default/global probe.
  Origins are whole or near-whole units (e.g. (-40,368,176), (320,-352,200)); the fourth float of the origin was
  0.0 in every probe. Volume counts: probes 8 (1), 13 (2), 14 (4), 20 (3), 21 (3); the rest 0. 20 references, e.g.
  (1424,-560,200)->probe 11, (-512,1984,472)->probe 8, (-16,-176,184)->probe 0.
- Per-cell lists (`GfxCell::reflectionProbes` / `reflectionProbeReferences`, u8 index lists) are stepped over, not
  captured.
- UNKNOWN: the meaning of the fourth origin float (0 everywhere seen), the plane layout inside a volume record
  (96 bytes = 6 planes of vec4 assumed, not verified), how the exe picks a probe from the references and cell lists,
  and the parallax-correction maths (see 3.4). The cube images are not yet decoded here.
- Update (2026-11-03, `fastfile_iw6::probe`): the volume record is 6 planes `(n, d)`, inside when n.p + d >= 0
  (verified on mp_frag probe 8, whose box holds its origin under that sign only). The cubes decode: six faces, each
  with its prefiltered mip chain; probe 0 a flat 1.0. Surfaces and static models store their own probe index; the
  stand-in for everything else and its measured agreement are in `docs/IW6.md` "Reflection probes and the sky".
