# Call of Duty: Ghosts (IW6 MP, PC x64): world lighting from the shaders

Companion to `IW6_LIGHTING.md` (the exe's CPU side). The exe holds no shader maths; the compiled
D3D11 programs (DXBC, SM 5.0, no RDEF: reflection stripped, only ISGN/OSGN/SHEX) are in the
technique-set zones. Everything below was read from those programs (static disassembly, nothing run),
2026-11-03. Tags: CONFIRMED (read from the bytecode / arguments), INFERRED, UNKNOWN.

## 0. Where the shaders are, and how they were read

- `mp_frag.ff` and `common_core_mp.ff` hold **no** programs: their technique sets are `,name` stubs
  (0 techniques each; 0 `DXBC` blobs in either image). The programs are in
  `techsets_mp_frag.ff` (381 assets, 4335 programs), `techsets_common_core_mp.ff` (584),
  `techsets_common.ff` (2831), `techsets_code_post_gfx_mp.ff` (452 techsets, 5329 programs; the
  common world/model sets such as `wc_l_sm_r0c0n0s0l0_nfwpf`, `mc_l_sm_r0c0n0s0l0_nfwpf`, `wc_sky_bl`
  and every post pass). These were staged from the owner's Ghosts `zone` folder (only the uploads'
  two zones were in the cloud).
- Walk fix (CONFIRMED): hull and domain shader programs are in the VIRTUAL block like vertex and
  pixel programs (they were read into TEMP). With that, all four techset zones walk exactly (stream
  end = zone end, VIRTUAL end = block size, 0 unresolved); before, VIRTUAL fell short by exactly the
  hull + domain program bytes (e.g. `techsets_common_core_mp` 0x6882c = 250112 + 177964) and 684..20671
  references went unresolved. `code_post_gfx_mp.ff` still stops at asset 252 (a type without a layout;
  its technique sets are duplicated in `techsets_code_post_gfx_mp.ff`, which is complete).
- The walk now keeps techniques, passes, arguments and program spans
  (`fastfile_iw6::walk::{TechniqueRec, PassRec, ShaderArgRec}`, `Capture::shader_programs`). Check:
  `IW6_TECHSETS=<techsets_*.ff> cargo test -p fastfile_iw6 --release techsets_retail -- --ignored`.
- Disassembly: a scratch tool over `dxbc_sm5::decode` (outside the repo). Opcode names as in
  `dxbc_sm5::decode::op`.

## 1. Shader arguments (MaterialShaderArgument, 16 bytes)

`type` u8 @0, `shader` u8 @1 (2 = vertex, 16 = pixel), `dest` u16 @2, union @8. CONFIRMED by matching
every argument against the program's declarations:

| type | meaning | dest | union |
|---|---|---|---|
| 0 | code constant | vec4 register in the block's cbuffer | `index` u16 (CONST_SRC_CODE), `firstRow` u8, `rowCount` u8 |
| 1 | code texture | `t#` | u32 TEXTURE_SRC_CODE index |
| 2 | code sampler | `s#` | same index |
| 3 | material constant | `cb3` register | u32 name hash (the material's constant table hash, e.g. `envMapParms` 0x3d9994dc) |
| 4 | literal constant | register | pointer to 4 floats |
| 5 | material texture | `t#` | u32 name hash (a different hash from the material's texture table: colorMap 0xa0ab1041, normalMap 0x59d30d0f, specularMap 0x34ecccb3) |
| 6 | material sampler | `s#` | same hash as its texture |

Constant buffers (CONFIRMED from dest registers and the programs' `dcl_constant_buffer`):
`cb0` = per-primitive block (`perPrimArgSize`), `cb1` = per-object block, `cb2` = stable block,
`cb3` = the material's constants. One block is **shared by the vertex and pixel stages** (e.g. a
pass's cb1 holds the view-projection rows 0..3 for the VS and the light at rows 4..6 for the PS;
112 bytes = 7 vec4).

The CONST_SRC_CODE indices in `iw6/const_src_code.tsv` are confirmed again by use: FOG 170,
FOG_COLOR_LINEAR 171, FOG_SUN_CONSTS 173, FOG_SUN_COLOR_LINEAR 174, FOG_SUN_DIR 176, FOG_HEIGHT 177,
FOG_DEPTHHACK 179, ZNEAR 159, LIGHTING_LOOKUP_SCALE 160, INV_SCENE_PROJECTION 161, HDR_SKY_INTENSITY 167,
SHADOWMAP_SWITCH_PARTITION 157, SHADOWMAP_SCALE 158, COLORIZATION_* 193..195, TONE_MAPPING_* 217..225,
SUN_SHADOWMAP_PIXEL_ADJUST 235, BLACK_LEVEL 254, BASE_LIGHTING_COORDS 255, EYEOFFSET 62,
TRANSPOSE_SHADOW_LOOKUP_MATRIX 305, TRANSPOSE_WORLD_MATRIX0 329; textures MODEL_LIGHTING 3,
SHADOWMAP_SUN 6, FEEDBACK 11, RESOLVED_SCENE 13, HDR_SCENE_R11G11B10_MIPMAP 14, FLOATZ 26, MACROCLUT 40,
COLOR_GRADING_CLUT 51, SCENE_LUMINANCE 54, DYNAMIC_IRIS_EXPOSURE 56 — all as the tables say.

Not passed as arguments (bound by the engine at fixed slots, CONFIRMED by absence, slot use INFERRED
from the formats the repo reads, `IW6.md`): reflection probe cube `t1/s1`, lightmap primary `t2/s2`
(BC4: the sun's baked visibility), lightmap secondary `t3/s3` (two stacked R16G16B16A16_FLOAT pages).

## 2. Technique names (slot -> technique, world/model sets)

`wc_l_sm_r0c0n0s0l0_nfwpf` (world, colour + normal + specular, techsets_code_post_gfx_mp):
slot 0 `zprepass`, 1 `build_shadowmap_depth`, 7 `opl_lmap_r0c0n0s0_blml0_nofog_sm_sm5_ln0`
(PS `lmo_sm_l0n_r0c0n0s0l0_nofog_blm_sm5.hlsl`), 8 `..._ld0` (PS `lmo_sm_l0d_...`), 9 `..._ld1`
(PS `lmo_sm_l0d_l0sd_...`), 10..12 `ls0..2` (spot), 13/14 `lo0/1` (omni), 15..43 two-light
combinations, 44..47 `ldb[_sun][_cuc]` (`lmo_sm_ldb(_sun)sm_...`), 102/411 `debug_bumpmap`.
Light codes: `n` none, `d` directional (sun), `d..sd` directional with the sun shadow map, `s` spot,
`s..ss` shadowed spot, `s..sc` spot with cucoloris, `o` omni.
`mc_l_sm_r0c0n0s0l0_nfwpf` (models): the same slots with `opl_lprobe_...` (PS `lpo_sm_l0n_nofog_...`,
`lpo_sm_l0d_l0sd_nofog_...`), slot 2 `vcsh_nofog`. `wc_sky_bl`: `sky_blml` (VS `sky.hlsl`, PS
`sky_blml.hlsl`). Every lit world/model shader is `nofog`: fog is a post pass (section 7).

## 3. Common inputs (vertex shaders, CONFIRMED)

- Positions are camera-relative: the world matrix's translation minus `EYEOFFSET` (`cb2[24]` world,
  `cb2[30]` models). The PS's `v5` (world) / `v4` (model) is `p - eye`; `V = normalize(p - eye)`
  (points from the eye to the surface).
- Vertex normal and tangent are UNORM `2x - 1`, normalised, rotated by the world matrix. The binormal
  sign is the sign of the position's w; the PS forms `B = cross(N, T) * sign / |N_world|`.
- World lightmap UV in `TEXCOORD0.zw`; colour/normal/specular UV in `TEXCOORD0.xy`.

## 4. World surface: lit lightmap (`lmo_sm_l0n_r0c0n0s0l0_nofog_blm_sm5`)

Registers: `t0/s0` colorMap, `t1/s1` probe (cube), `t3/s3` lightmap secondary, `t4/s4` normalMap,
`t5/s5` specularMap; `cb3[0]` envMapParms, `cb3[2]` bloomParams. Pseudo-code (CONFIRMED):

    nm   = sample(normalMap, uv).rg               // BC5 SNORM
    s    = 2 * nm.gr                               // (tangent slope, binormal slope) = (2g, 2r)
    N    = normalize(Nv + s.x * T + s.y * B)
    V    = normalize(p - eye)
    ndv  = dot(V, N)
    fres = lerp(env.x, env.y, max(1 - |ndv|, 0) ^ env.z)       // envMapParms (min, max, exponent)
    R    = V - 2 ndv N
    sp   = sample(specularMap, uv); specCol = sp.rgb^2; gloss = sp.a
    refl = sampleLevel(probe, R, 4.5 - 5 gloss) * specCol * fres
    up   = sample(lmSecondary, (lm.x, lm.y * 0.5))            // upper page
    lo   = sample(lmSecondary, (lm.x, lm.y * 0.5 + 0.5))      // lower page
    d    = (up.a, lo.a, sqrt(max(1 - up.a^2 - lo.a^2, 0)))    // baked direction, tangent space
    nts  = normalize(s.x, s.y, 1)
    light= up.rgb * nts.z + lo.rgb * saturate(dot(nts, d))
    alb  = (sample(colorMap, uv).rgb * vertexColour.rgb)^2
    out.rgb = alb * light + refl
    out.a   = vertexColour.a * bloomParams.x                 // the bloom mask (section 8)

- Lightmap decode (CONFIRMED): no scale/bias, no exponent: the secondary's float texels are linear
  light. The upper page is a non-directional term weighted by the tangent normal's z; the lower page
  is the directional term on its baked direction (alphas x, y; z the rest of the unit length). On a
  surface without a normal map: `up.rgb + lo.rgb * d.z`. The primary (BC4) is not read here.
- Gamma: colour map times vertex colour, then squared (a gamma-2 decode); specular colour squared.
- No sun in `ln0`: surfaces without the sun as primary light take only the baked pages.

## 5. BRDF with a light (`..._l0d_...`, `..._l0d_l0sd_...`)

Per-object cbuffer: `cb1[4]` LIGHT_POSITION (directional: the unit direction towards the light),
`cb1[5]` LIGHT_DIFFUSE, `cb1[6]` LIGHT_SPECULAR (in `l0d_l0sd` rows 8..10, with
TRANSPOSE_SHADOW_LOOKUP_MATRIX at 4..7). Adds, to section 4 (CONFIRMED):

    vis   = sample(lmPrimary, lm).r                 // ld0: baked sun visibility (BC4)
          | sunShadow(p) (section 6)                // ld1: shadow map over the baked visibility
    diff  = saturate(dot(L, N)) * LIGHT_DIFFUSE
    n     = 2^(9.377518 gloss) + 7                  // = e^(6.5 gloss) + 7: 8 .. 672
    hl    = min(1, e^(n * (dot(R, L) - 0.99925)))   // Phong lobe on the reflection vector, exp form of (R.L)^n
    out.rgb = alb * (light + vis * diff) + specCol * fres * (probe(R) + vis * hl * LIGHT_SPECULAR)

- Diffuse: Lambert, no 1/pi. Specular: Phong on R (not Blinn), `pow` approximated by an exponential
  with a small offset (peaks clamp at 1), no normalisation, scaled by the same `envMapParms` fresnel
  ramp as the reflection and by the squared specular colour. No metalness/roughness.
- Gloss: specular map alpha. Highlight exponent `e^(6.5 g) + 7`; probe mip `4.5 - 5 g` (gloss 0.9 =
  mip 0).
- Fresnel: `lerp(envMapParms.x, envMapParms.y, (1 - |N.V|)^envMapParms.z)`, per material (mp_frag:
  e.g. (0.07, 1.3, 4.0), (0.2, 1.0, 2.5), (0.72, 10, 7.7); w is 2.0 in nearly all and unused here).
  The exe's `r_envMap*` dvars (min 0.2, max 0.5, exp 5) are not read by these shaders (ENVMAP_PARMS
  is not an argument of any of them).
- The specular light colour already includes `r_specularColorScale` (CPU side, `IW6_LIGHTING.md` 3.2).
- `reflectionRadius` (8096 in every material) is not read by these shaders: no parallax correction;
  the probe is sampled along R only.

## 6. Sun shadow (`..._l0d_l0sd_...`, CONFIRMED)

    sc    = (p - eye, 1) * TRANSPOSE_SHADOW_LOOKUP_MATRIX      // rows: u, v, depth
    nearUV= sc.xy; farUV = sc.xy * PART.w + PART.xy            // SHADOWMAP_SWITCH_PARTITION
    tap_i = sampleCmpLevelZero(shadowmapSun, uv + PIX.xy * O_i, sc.z)   // PIX = SUN_SHADOWMAP_PIXEL_ADJUST
    near  = sum(tap_i at nearUV) / 12; far = sum(tap_i at farUV) / 12
    e     = (nearUV, farUV) * SCALE.xxyy + SCALE.zzzw            // SHADOWMAP_SCALE
    fadeN = saturate(8 - max(|e_nearU|, |e_nearV|)); fadeF = saturate(8 - max(|e_farU|, |e_farV|))
    v     = lerp(baked, far, fadeF)                              // baked = lmPrimary.r (world) / grid.a (models)
    w     = (fadeF * fadeN != fadeN) ? 1 : fadeN
    vis   = lerp(v, near, w)

12 taps `O_i` (texels): (-2.453568, 2.469924), (2.58272, 1.97284), (0, 0), (-3.268776, -1.084384),
(3.9082, -0.43446), (0.314828, -2.861292), (2.897916, -2.323192), (1.949888, -0.252328),
(-0.336312, 3.593248), (-1.978208, -2.844204), (-3.786372, 0.90054), (0.61806, 1.861508). Hardware
compare (`sample_c_lz`, comparison sampler s4; filter mode of the sampler UNKNOWN, PCF of bilinear
compares if it is linear). Two cascades in one atlas (`IW6_LIGHTING.md` 3.3). Beyond the far cascade
the baked BC4 visibility (world) / light-grid alpha (models) takes over.

## 7. Models: light grid and probe (`lpo_sm_l0n_nofog_r0c0n0s0l0_blm_sm5`, CONFIRMED)

`t6/s6` MODEL_LIGHTING (a **3D texture**), `cb2[0]` LIGHTING_LOOKUP_SCALE, VS passes
BASE_LIGHTING_COORDS (`cb0[4]`, per primitive) through `TEXCOORD6`.

    m     = max(|N.x|, |N.y|, |N.z|)
    c     = BASE_LIGHTING_COORDS.xyz + N * LIGHTING_LOOKUP_SCALE.xyz / m     // the normal on the cube shell
    grid  = sample(modelLighting3D, c)            // trilinear
    out.rgb = alb * grid.rgb + specCol * fres * probe(R, 4.5 - 5 gloss)     (+ sun as section 5, vis base = grid.a)
    out.a   = vertexColour.a * bloomParams.x (cb3[4])

- So the 56-sample table of `IW6_LIGHTING.md` 3.1 is laid out as the shell of a 4x4x4 3D texture and
  read by trilinear filtering at the normal's cube-shell point (answers "how MODEL_LIGHTING is indexed").
  No SH is evaluated per pixel. Its alpha is the sun's visibility (used past the shadow map).
- The reflection probe is the same cube slot `t1` as the world's (per-draw selection is CPU side, UNKNOWN).
- Normal map read the same way as the world (`2 * nm.gr`).

## 8. Sky (`sky_blml.hlsl`, CONFIRMED)

    d     = normalize(p - eye)                        // z up
    f     = saturate(((1 - d.z) * pi/2 - FOG_HEIGHT.x) * FOG_HEIGHT.y) * FOG_HEIGHT.z
    s     = saturate((dot(FOG_SUN_DIR, d) - FOG_SUN_CONSTS.y) * FOG_SUN_CONSTS.z)
    fogC  = lerp(FOG_COLOR_LINEAR, FOG_SUN_COLOR_LINEAR, s)
    sky   = sample(skyCube, d).rgb * HDR_SKY_INTENSITY.rgb
    out.rgb = lerp(sky, fogC, f);  out.a = bloomParams.x

The sky-fog "angle" is `(1 - z) pi/2` (0 at the zenith, pi/2 at the horizon), not an arcsine.
`HDR_SKY_INTENSITY` is used as an rgb multiplier.

## 9. Fog (post-opaque, `postopaquefx_fog`, `_dfog`, `_fog_color`; CONFIRMED)

Full-screen, after the opaque pass, reading RESOLVED_SCENE (`t0`) and FLOATZ (`t4`):

    z = floatz.r;  if z == 0: T = 1 (nothing drawn; the sky fogs itself)
    if z > 63/64:  dist via near 0.1, d = 0.1 * (1/64) / (z - 63/64), constants FOG_DEPTHHACK (view model)
    else:          d = ZNEAR * (63/64) / z, constants FOG
    ray = ((2u - 1) * IP.x + IP.y, -((2v - 1) * IP.z) + IP.w, 1) * d    // IP = INV_SCENE_PROJECTION
    dist = |ray|
    fog:  T = saturate(max(F.y, e^(dist * F.z + F.w)))                 // F = FOG = (_, 1 - maxOpacity, -k, k start)
          out = lerp(FOG_COLOR_LINEAR, scene, T)
    dfog: s  = saturate((dot(FOG_SUN_DIR, ray / dist) - SC.y) * SC.z)  // SC = FOG_SUN_CONSTS
          T  = max(F.y, e^(dist * SC.w + F.w)); Ts = max(F.y, e^(dist * SC.x + F.w))
          T' = saturate(lerp(T, Ts, s)); col = lerp(FOG_COLOR_LINEAR, FOG_SUN_COLOR_LINEAR, s)
          out = lerp(col, scene, T')
    _color: then the HDR colorization of section 10 in the same pass.
    alpha passes through.

So: transmittance halves every half distance past the start, floored at `1 - maxOpacity`; the sun fog
uses `k * scale` for its own density and a linear cosine ramp between the end and begin angles.
OPEN: `FOG_SUN_DIR` is dotted with a camera-relative world direction in the sky shader and with the
view-space ray here; either the CPU writes it per pass or one of the two uses is in another space
(UNKNOWN which).

## 10. Post chain (tone, colour; CONFIRMED maths, order INFERRED from inputs)

1. Opaque lit pass: linear HDR, alpha = bloom mask (`bloomParams.x * vertex alpha`).
2. `postopaquefx_*`: fog (section 9), then optionally HDR colorization:
   `c = max(c, 0); c = lerp(c, dot(c, (0.299, 0.587, 0.114)), CPD.w); c = (c <= 0 ? 0 : c^CPD.xyz * SCALE) + BIAS`
   (CPD = COLORIZATION_POWER_AND_DESATURATION; `postfx_color_grad_hdr` is the same maths as a pass of its own).
3. Bloom HQ: `material_bloom_hq_setup_alpha_intensity`: 4 taps at `+-0.25 texel` (RENDER_TARGET_SIZE.zw),
   each colour weighted by its alpha / 4 (the bloom mask), desaturated toward Rec. 709 luma by
   GLOW_SETUP.z, clamped to 65504, raised to GLOW_SETUP.y (bloom gamma). Blur passes, then
   `material_bloom_apply_bloomhq`: three mip levels of HDR_SCENE_R11G11B10_MIPMAP at lods
   FALLOFF_2.xyz weighted FALLOFF_1.xyz, `out = sum * GLOW_APPLY.w + GLOW_APPLY.x`, alpha GLOW_APPLY.y
   (blended over the scene; blend state not read).
4. Luminance: `avg_luminance_setup` = 4-tap box of the source's **alpha** channel (texels outside the
   valid rect weighted 0, half-texel edges 0.5; AVG_LUMINANCE_PARAMS = (dx, dy, maxU, maxV)) / 4;
   `avg_luminance_accumulate` = 4-tap box * AVG_LUMINANCE_PARAMS.w / 4. Arithmetic mean, no log.
   Which pass writes the luminance into the alpha the setup reads: UNKNOWN.
5. `update_dynamic_iris_adaptation`: E = TONE_MAPPING_EXPOSURE (min, max, key, _), A =
   WHITE_ADAPTATION_RATES (down, up, dt, _):
   `target = key / max(lum, 1e-4); d = target - prev; step = d > 0 ? min(d, up dt) : max(d, -down dt);
   new = clamp(prev + step, min, max)` — a linear rate in exposure units per second, `up` (A.y) for
   brightening, `down` (A.x) for darkening. (This settles `IW6_LIGHTING.md` 2.2's open reading: E.xyz
   are min, max and the key.)
6. Final (`postfx_macroclut` or `postfx_color_grad_clut`): the VS fetches the exposure from
   DYNAMIC_IRIS_EXPOSURE at (E.x, 0.5) (a 1-texel target; the coordinate is incidental). Then

       x  = scene * exposure
       f  = (A x^2 + C B x + D E) / (A x^2 + B x + D F) - E/F      // Hable, per channel, consts 217..222
       t  = f * TONE_MAPPING_WHITE_POINT.rgb                       // 1 / f(W)
       macroclut:   g = srgb(saturate(sample(MACROCLUT, t * 15/16 + 1/32)))   // 16^3, linear in, linear out
       grade_clut:  g = sample(COLOR_GRADING_CLUT, srgb(saturate(t)) * 31/32 + 1/64)   // 32^3, sRGB in, sRGB out
       out = saturate((g - BLACK_LEVEL.x) * BLACK_LEVEL.y)

   `srgb` is the exact piecewise curve (12.92 below 0.0031308, else 1.055 x^(1/2.4) - 0.055); the
   output is written already encoded. In the macroclut path the tone-mapped value is not saturated
   before the lookup (the sampler clamps); film tint / colorization / CLUT are baked into the macro
   table on the CPU (`IW6_LIGHTING.md` 5). The tone map is applied once, after bloom and colorization.

## 11. What this means for the port (notes for the lane / tone workers)

- Lightmaps: upper page = non-directional, lower = directional with the direction in the two alphas,
  flat weight `d.z = sqrt(1 - x^2 - y^2)`. CoD4's packing (`fastfile_iw6::map::iw3_lightmap_look`) is
  drawn with CoD4's `rsqrt(1 + x^2 + y^2)` (0.74 instead of 0.44 at |d.xy| = 0.9): directional pages
  come out up to ~1.7x too bright on flat surfaces. `BO2MP_IW6_LM_DIR=1` rescales the lower page to fix
  it on flat surfaces (default off; needs a GPU screenshot). With a normal map the dot product with the
  tangent normal is exact only in a native shader.
- The sun is never in the lightmap: sunlit surfaces use `ld*` techniques that add `vis * N.L * sun`
  live (the lane's `sun_in_lightmap` heuristic should always come out "not baked" for Ghosts).
- Tone (owner of `iw6_tone.rs`): exposure multiplies before the curve; white scale after; macroclut
  table indexed linear (16), plain CLUT indexed sRGB (32); exact sRGB encode; black level last. The
  iris: linear-rate adaptation to `key / mean luminance` in [min, max], `up` = rates.y.
- Light grid (owner of the lightgrid wiring): sample the 56-value shell as a trilinear 4x4x4 volume at
  `base + N / max|N| * scale`; alpha = sun visibility for models past the shadow map.
- Pure Rust of all of the above: `fastfile_iw6::shading` (tests in the module).

## 12. Gaps

- Engine-bound slots t1..t3 are inferred from formats, not read (no argument names them).
- Shadow comparison sampler filter, blur kernels of the bloom HQ pyramid, the luminance source pass,
  `ldb` (deferred batch) light path, spot/omni attenuation and cucoloris (shaders dumped, not written
  up), SSAO/SSR/DOF/motion blur shaders, the water and layered (`b1c1`, `m1c1`) world variants.
- FOG_SUN_DIR space (section 9).

## 13. Tessellation and SSAO constants (exe decompile, 2026-11-03)

Read from `iw6mp64_ship_all.c` (Ghidra output of the unpacked exe; data tables read from the exe's
`.rdata` by file offset, nothing run).

- `TESSELLATION_PARMS` / `PARMS2` (FUN_14060e680, writing the code constant block at +0x1330):
  `(r_tessellationFactor 60 (min 1e-5), r_tessellationLodBias 1, r_tessellationHeightScale 1 x -1/256
  when r_tessellationHeightAuto (true), r_tessellationHybrid 1)` and `(r_subdomainLimit 3,
  r_tessellationCutoffDistance 960 - r_tessellationCutoffFalloff 320, 1 / falloff, r_tessellationEyeScale
  0.0625)`, the cutoff pair 0 unless `r_tessellation` is 1. In the `no_tess_world_disp` vertex programs
  (cb2 rows 29 and 30): `s = w0 / (PARMS.z >= 0 ? 1 : |pos.w|) * |PARMS.z|`, `disp = s * (k.x * h + k.y)`
  (k = the material's cb3 row 0), `h` the displacement map at mip `log2(depth / (PARMS.x * (1 + (N.V)
  (PARMS2.w - 1)))) + PARMS.y + log2|pos.w| + k.z`.
- SSAO (FUN_1406441c0 gathers, FUN_140644420 writes the six constants at 0x149832890.., in
  CONST_SRC_CODE order 109..114; w, h the target size):
  - `SSAO_POWER_BB_SHARPNESS_STEP` = (power, r_ssaoBlurSharpness, 0, step / h) for the first blur;
    FUN_140644370 sets the blur passes: step `max(1, floor(r_ssaoBlurStep * T[mode] + 0.5))` in x
    (`/ w`, y 0) or in y (`/ h`, x 0); T at 0x1408cc6c8 = (2, 2) and, for the first, 0x1408cc6c4 by
    mode = (0, 2, 2).
  - `SSAO_COLOR_COEFF` = (S D 0.299, S D 0.587, S D 0.114, S) with S the strength, D r_ssaoDiminish.
  - `SSAO_FALLOFF_DEPTH_SCALE` = (-1 / r_ssaoGapFalloff, r_ssaoGradientFalloff^2, r_ssaoDepthScale,
    r_ssaoDepthScaleViewModel x r_ssaoDepthScale).
  - `SSAO_UV_TO_ROT_SCALE_AND_FADE` = (w / 8, h / 8, r_ssaoFadeDepth, 0).
  - `SSAO_SAMPLE_MAT_SCALE` = (W h / w, W h / w, -W, W), W = r_ssaoWidth / 720.
  - `SSAO_DEPTH_OFFSET_REJECT` = (1 / w, 1 / h, 0, 1 / min(r_ssaoRejectDepth, h W r_ssaoFadeDepth /
    r_ssaoMinPixelWidth)).
  - Strength and power from the vision (`r_ssaoStrength`, `r_ssaoPower`) unless `r_ssaoUseTweaks`;
    times `r_ssaoScriptScale` when `r_ssaoUseScriptScale`; mode 0 when the strength is 0.
  - `RANDOM_ROTATIONS` (FUN_140643e20): 8 x 8, DXGI 34 (R16G16_FLOAT), pitch 32, data at 0x1408cc700
    (`.rdata`): 64 unit vectors (cos, sin), rows in order:
    (0.3188, 0.9478), (0.8145, -0.5801), (0.9653, 0.2615), (0.6572, 0.7534), (-0.9653, -0.2615), (-0.9917, 0.1278), (0.9746, -0.2244), (0.9863, 0.1655)
    (-0.8672, 0.4976), (0.7280, 0.6855), (-0.9478, 0.3188), (0.7534, -0.6572), (-0.3547, 0.9351), (0.4102, 0.9121), (-0.5801, -0.8145), (-0.9863, -0.1655)
    (-0.6855, 0.7280), (-0.9976, -0.0681), (0.7915, 0.6108), (-0.2615, 0.9653), (0.3547, -0.9351), (0.9478, -0.3188), (-0.9121, 0.4102), (-0.4976, -0.8672)
    (0.5801, 0.8145), (-0.5303, 0.8477), (0.0300, 0.9995), (0.1278, 0.9917), (0.4976, 0.8672), (-0.0300, -0.9995), (0.8672, -0.4976), (-0.7915, -0.6108)
    (0.8955, 0.4448), (0.0681, -0.9976), (-0.4102, -0.9121), (0.2244, 0.9746), (-0.9746, 0.2244), (-0.2244, -0.9746), (0.9976, 0.0681), (-0.6572, -0.7534)
    (0.5303, -0.8477), (-0.8477, -0.5303), (-0.1278, -0.9917), (0.9351, 0.3547), (-0.0681, 0.9976), (0.9121, -0.4102), (0.9917, -0.1278), (-0.9351, -0.3547)
    (-0.4448, 0.8955), (-0.8145, 0.5801), (0.4448, -0.8955), (0.6855, -0.7280), (0.6108, -0.7915), (0.1655, -0.9863), (-0.6108, 0.7915), (0.9995, -0.0300)
    (-0.1655, 0.9863), (-0.9995, 0.0300), (0.8477, 0.5303), (-0.3188, -0.9478), (0.2615, -0.9653), (-0.7534, 0.6572), (-0.8955, -0.4448), (-0.7280, -0.6855)
  - Not wired in `render_iw6` yet: `ssao_calc_fast` reads `FLOATZ` as 1 / distance (it rejects past
    `1 / r_ssaoRejectDepth`, fades by `FLOATZ * r_ssaoFadeDepth`, takes `FLOATZ > 63/64` as the view
    model), i.e. the engine's `ZNEAR * 63/64` is 1 there; `render_iw6`'s `FLOATZ` is `near * band /
    distance` (its `ZNEAR` makes the fog's distance right), so either a second depth copy at 1 / distance
    or every depth-unit constant rescaled is needed first.

## 14. The final pass's colour table (`MACROCLUT`, exe decompile, 2026-11-03)

FUN_14016c3b0 (per frame, when colour grading is on and `r_postfxMacroclutEnable`) finds or makes a
cached table for the post state's resolved colorization block (0x178 bytes copied from the vision at
+0x118 into the post state at +0x18f0 by FUN_14060cca0: the set's `lowAndHighEndPoints`,
`midEndPoints`, `alternateDepthScaleAndBias`, the alternate mask, then the main, alternate, low, mid and
high colorizations, 10 floats each: power xyz, desaturation, scale xyz, bias xyz; flags at +0xf8) and
fills it in FUN_14016c610 (directly or on the worker, `r_postfxMacroclutWorker`). The input grid
(FUN_14016d0f0): 4096 entries, `(i / 15)` per channel, red fastest, then green, then blue. Per entry:
`L = 0.299 r + 0.587 g + 0.114 b`; per band `x = c (1 - desat) + L desat`, `p = x > 0 ? x^power : 0`,
`band = p * scale + bias`; weights `low = 1 - S(sat((L - e0) e1))`, `mid = 1 - S(sat((|L - m0| - m1) m2)) +
0.0001`, `high = S(sat((L - e2) e3))` with `S(x) = x^2 (3 - 2x)`; `out = sat((mid*M + low*Lo + high*H) /
(low + mid + high))`, written as R8G8B8A8_UNORM (red low byte, alpha 0) into the 16^3 texture
(FUN_1405e4520 format 0x1c, named "macroclut"). Nothing in it, nor any technique, reads the vision's
`r_film*`: with the macroclut final pass the film is not applied. mp_frag (`mp_frag_colorset`): low
`mp_frag_low_color` (power 1, 1.03, 1, scale 0.851, 0.925, 1), mid identity, high (power 1.1, scale
0.945, 0.945, 0.980, bias 0.01), end points (0, 2.5641, 0.48, 3.125), (0.39, 0, 1.66667, 0). In
`render_iw6::look::Lmh` / `post::macroclut_lmh`.
