# Modern Warfare Remastered, natively: what the game itself does (tone map, exposure, light set, collision)

Goal (owner, 30 Oct): do for MWR what `docs/T7_NATIVE.md` did for Black Ops III:
read the game's own maths from its executable and put it in our renderer in
place of fitted numbers. This file holds function-level descriptions and
formulas; the exe addresses behind them are in `docs/exe_notes/`.

Reference: the owner's `h1_mp64_zt.exe` (the Horizon MW build of the h1
lineage, 21 MB), analysed statically with Ghidra 11.4.2 headless in the cloud
box (the exe was never run). It is an ordinary unprotected PE: a normal CRT
entry point, readable `.text` (entropy 6.6), a plain `.interpr` section
that holds the script interpreter, and `.rdata` strings. Nothing needed
was encrypted or filled in by a loader at run time; no protection was
touched or looked at.

Method: Ghidra decompile of the functions that fill the exposure and tone
map state; a table scan for the engine constant names; the dvar registration
calls (defaults and ranges are immediate arguments); the vision and light set
key tables. Dvar names are **hashed** in this exe (32-bit), so a dvar's name
comes from `docs/mwr_engine/dvars_rust.md` (read live from HorizonMW's
console) and is matched to the exe's registration by default value, range
and by where the value is used; this matching is called *matched* below.

What is **read** below is code or tables the decompile shows; **inferred**
is stated where it is.

## Engine constants (CONST_SRC_CODE)

A table of 501 `{name}` pointers in order, the position being the constant's
index; 0..421 are the `CONST_SRC_CODE_*` constants, then
`TEXTURE_SRC_CODE_*` (422...). The ones this work cares about:

| index | name | index | name |
|---|---|---|---|
| 120 | VEIL_PARAMS | 205 | INV_SCENE_PROJECTION |
| 196..202 | SHADOWMAP_* (switch partition, distance bias, scale, uv offset, cascade mask, blend) | 213..221 | FOG ... FOG_HEIGHT_FOG |
| 216..220 | FOG_SUN_CONSTS, FOG_SUN_COLOR_LINEAR/GAMMA, FOG_SUN_DIR, FOG_SKY_FOG | 222..228 | ATMOS_FOG_PARMS_0..6 |
| 231..233 | GLOW_SETUP, GLOW_APPLY, GLOW_SETUP_ALT_COLOR_SCALE | 234..237 | COLOR_BIAS, COLOR_TINT_BASE/DELTA/QUADRATIC_DELTA |
| 238 | PERCEPTUAL_PARMS | 239..243 | FILMIC_PARMS, FILMIC_SHOULDER_PARMS, FILMIC_TOE_PARMS, TONEMAP_PARMS, HDR_STAGE_PARMS |
| 269..273 | SUN_SHADOWMAP_PIXEL_ADJUST_ARRAY_0..2, CASCADE_V_CLAMP, NEAR_FAR_PLANE | 313 | LIGHT_PROBE_AMBIENT |
| 267, 268 | OUTDOOR_FEATHER_PARMS, ENVMAP_PARMS | 147..154 | DOF_* |

The whole list (index name), for the shader-constant work:

```
0 LIGHT_POSITION | 1 LIGHT_DIFFUSE | 2 LIGHT_PHYSICALSIZE | 3 LIGHT_FADEOFFSET | 4 LIGHT_SPECULAR
5 LIGHT_SPOTDIR | 6 LIGHT_SPOTFACTORS | 7 LIGHT_FALLOFF_PLACEMENT | 8 LIGHT_CUCOLORIS_ANIM
9 LIGHT_POSITION1 | 10 LIGHT_DIFFUSE1 | 11 LIGHT_PHYSICALSIZE1 | 12 LIGHT_FADEOFFSET1 | 13 LIGHT_SPECULAR1
14 LIGHT_SPOTDIR1 | 15 LIGHT_SPOTFACTORS1 | 16 LIGHT_FALLOFF_PLACEMENT1 | 17 LIGHT_CUCOLORIS_ANIM1
18 LIGHT_POSITION2 | 19 LIGHT_DIFFUSE2 | 20 LIGHT_PHYSICALSIZE2 | 21 LIGHT_FADEOFFSET2 | 22 LIGHT_SPECULAR2
23 LIGHT_SPOTDIR2 | 24 LIGHT_SPOTFACTORS2 | 25 LIGHT_FALLOFF_PLACEMENT2 | 26 LIGHT_CUCOLORIS_ANIM2
27 LIGHT_POSITION3 | 28 LIGHT_DIFFUSE3 | 29 LIGHT_PHYSICALSIZE3 | 30 LIGHT_FADEOFFSET3 | 31 LIGHT_SPECULAR3
32 LIGHT_SPOTDIR3 | 33 LIGHT_SPOTFACTORS3 | 34 LIGHT_FALLOFF_PLACEMENT3 | 35 LIGHT_CUCOLORIS_ANIM3
36 LIGHT_POSITION_DB_ARRAY_0 | 37 LIGHT_POSITION_DB_ARRAY_1 | 38 LIGHT_POSITION_DB_ARRAY_2
39 LIGHT_POSITION_DB_ARRAY_3 | 40 LIGHT_DIFFUSE_DB_ARRAY_0 | 41 LIGHT_DIFFUSE_DB_ARRAY_1
42 LIGHT_DIFFUSE_DB_ARRAY_2 | 43 LIGHT_DIFFUSE_DB_ARRAY_3 | 44 LIGHT_PHYSICALSIZE_DB_ARRAY_0
45 LIGHT_PHYSICALSIZE_DB_ARRAY_1 | 46 LIGHT_PHYSICALSIZE_DB_ARRAY_2 | 47 LIGHT_PHYSICALSIZE_DB_ARRAY_3
48 LIGHT_FADEOFFSET_DB_ARRAY_0 | 49 LIGHT_FADEOFFSET_DB_ARRAY_1 | 50 LIGHT_FADEOFFSET_DB_ARRAY_2
51 LIGHT_FADEOFFSET_DB_ARRAY_3 | 52 LIGHT_SPECULAR_DB_ARRAY_0 | 53 LIGHT_SPECULAR_DB_ARRAY_1
54 LIGHT_SPECULAR_DB_ARRAY_2 | 55 LIGHT_SPECULAR_DB_ARRAY_3 | 56 LIGHT_SPOTDIR_DB_ARRAY_0
57 LIGHT_SPOTDIR_DB_ARRAY_1 | 58 LIGHT_SPOTDIR_DB_ARRAY_2 | 59 LIGHT_SPOTDIR_DB_ARRAY_3
60 LIGHT_SPOTFACTORS_DB_ARRAY_0 | 61 LIGHT_SPOTFACTORS_DB_ARRAY_1 | 62 LIGHT_SPOTFACTORS_DB_ARRAY_2
63 LIGHT_SPOTFACTORS_DB_ARRAY_3 | 64 LIGHT_CUCOLORIS_ANIM_DB_ARRAY_0 | 65 LIGHT_CUCOLORIS_ANIM_DB_ARRAY_1
66 LIGHT_CUCOLORIS_ANIM_DB_ARRAY_2 | 67 LIGHT_CUCOLORIS_ANIM_DB_ARRAY_3
68 LIGHT_FALLOFF_PLACEMENT_DB_ARRAY_0 | 69 LIGHT_FALLOFF_PLACEMENT_DB_ARRAY_1
70 LIGHT_FALLOFF_PLACEMENT_DB_ARRAY_2 | 71 LIGHT_FALLOFF_PLACEMENT_DB_ARRAY_3 | 72 LIGHT_DYN_COUNT
73 LIGHT_DYN_TYPES | 74 LIGHT_DYN_SHADOW_TYPES | 75 PARTICLE_CLOUD_COLOR | 76 GAMETIME | 77 SYSTEMTIME
78 GENERIC_MATERIAL_DATA | 79 EYEOFFSET | 80 WORLD_MATRIX_EYE_OFFSET | 81 EMBLEM_LUT_SELECTOR
82 MODEL_VELOCITY_PARMS | 83 MODEL_VELOCITY_SKINNED_PARMS | 84 POSTFX_CONTROL0 | 85 POSTFX_CONTROL1
86 POSTFX_CONTROL2 | 87 POSTFX_CONTROL3 | 88 POSTFX_CONTROL4 | 89 POSTFX_CONTROL5 | 90 POSTFX_CONTROL6
91 POSTFX_CONTROL7 | 92 POSTFX_CONTROL8 | 93 POSTFX_CONTROL9 | 94 POSTFX_CONTROLA | 95 POSTFX_CONTROLB
96 POSTFX_CONTROLC | 97 POSTFX_CONTROLD | 98 POSTFX_CONTROLE | 99 POSTFX_CONTROLF | 100 SCRIPT_PARMS
101 EFFECT_MODEL_COLOR | 102 EFFECT_MODEL_COLOR_EMISSIVE | 103 FILTER_TAP_0 | 104 FILTER_TAP_1
105 FILTER_TAP_2 | 106 FILTER_TAP_3 | 107 FILTER_TAP_4 | 108 FILTER_TAP_5 | 109 FILTER_TAP_6
110 FILTER_TAP_7 | 111 FILTER_TAP_8 | 112 FILTER_TAP_9 | 113 FILTER_TAP_10 | 114 FILTER_TAP_11
115 COLOR_MATRIX_R | 116 COLOR_MATRIX_G | 117 COLOR_MATRIX_B | 118 RENDER_TARGET_SIZE
119 RENDER_SOURCE_SIZE | 120 VEIL_PARAMS | 121 FXAA_RCPFRAMEOPT | 122 VOLUME_LIGHT_SCATTER_1
123 VOLUME_LIGHT_SCATTER_2 | 124 VOLUME_LIGHT_SCATTER_3 | 125 MODEL_LIGHTMAP_PARAMS | 126 NEARPLANE_ORG
127 NEARPLANE_DX | 128 NEARPLANE_DY | 129 SSAO_POWER_BB_SHARPNESS_STEP | 130 SSAO_COLOR_COEFF
131 SSAO_FALLOFF_DEPTH_SCALE | 132 SSAO_UV_TO_ROT_SCALE_AND_FADE | 133 SSAO_SAMPLE_MAT_SCALE
134 SSAO_DEPTH_OFFSET_REJECT | 135 LIT2D_LIGHTDIRX2 | 136 LIT2D_SPECHALFANGLEDIRX2 | 137 LIT2D_AMBIENTCOLOR
138 LIT2D_DIFFUSECOLOR | 139 LIT2D_SPECCOLOR_SPECEXPONENT | 140 LIT2D_ADDITIVECOLOR
141 LIT2D_BLOODSPATTER_AMBIENTCOLOR | 142 LIT2D_BLOODSPATTER_DIFFUSECOLOR
143 LIT2D_BLOODSPATTER_SPECCOLOR_SPECEXPONENT | 144 LIT2D_BLOODSPATTER_ADDITIVECOLOR
145 LIT2D_HUDBLOOD_COLOR | 146 LIT2D_HUDBLOOD_COLOR_THIN | 147 DOF_EQUATION_VIEWMODEL_AND_FAR_BLUR
148 DOF_EQUATION_SCENE | 149 DOF_LERP_SCALE | 150 DOF_LERP_BIAS | 151 DOF_ROW_DELTA | 152 DOF_EQUATION
153 DOF_PARAMS | 154 DOF_TILE_PARAMS | 155 FRAME_COUNT_PARAMS | 156 MOTION_MATRIX_X | 157 MOTION_MATRIX_Y
158 MOTION_MATRIX_W | 159 SSR_PREV_FRAME_VIEWPROJECTION_MATRIX_R0
160 SSR_PREV_FRAME_VIEWPROJECTION_MATRIX_R1 | 161 SSR_PREV_FRAME_VIEWPROJECTION_MATRIX_R2
162 SSR_PREV_FRAME_VIEWPROJECTION_MATRIX_R3 | 163 PREV_EYEPOSITION_TRANSFORM
164 SCREEN_SPACE_REFLECTION_PARAMETERS | 165 CLIP_SPACE_LOOKUP_SCALE_AND_OFFSET
166 SSR_CLIP_TO_FADE_SCALE_OFFSET_PS | 167 SSS_MRT_OFF_SHADER_OFF | 168 MDAO_VIEWTOOCCLUDER_MATRIX_R0
169 MDAO_VIEWTOOCCLUDER_MATRIX_R1 | 170 MDAO_VIEWTOOCCLUDER_MATRIX_R2 | 171 MDAO_VIEWTOOCCLUDER_MATRIX_R3
172 MDAO_WORLDTOOCCLUDERFRAME_MATRIX_R0 | 173 MDAO_WORLDTOOCCLUDERFRAME_MATRIX_R1
174 MDAO_WORLDTOOCCLUDERFRAME_MATRIX_R2 | 175 MDAO_WORLDTOOCCLUDERFRAME_MATRIX_R3 | 176 MDAO_SCREEN_PARAMS
177 MDAO_CAMERA_PARAMS | 178 MDAO_LOOKUP_PARAMS | 179 MDAO_VOLUME_PARAMS | 180 DISTORTION_SAMPLE_LIMITS_PS
181 SCOPE_SAMPLE_LIMITS_PS | 182 UV_TO_PREV_SCALED_CLIP_MAT_C0_PS | 183 UV_TO_PREV_SCALED_CLIP_MAT_C1_PS
184 UV_TO_PREV_SCALED_CLIP_MAT_C3_PS | 185 PREV_FRAME_VIEWPROJECTION_MATRIX_R0
186 PREV_FRAME_VIEWPROJECTION_MATRIX_R1 | 187 PREV_FRAME_VIEWPROJECTION_MATRIX_R2
188 PREV_FRAME_VIEWPROJECTION_MATRIX_R3 | 189 PREV_FRAME_WORLD_MATRIX_EYE_OFFSET
190 STATIC_VELOCITY_PARMS | 191 MOTION_BLUR_HQ_PARAMS | 192 MOTION_BLUR_HQ_TILE_MAX_PARAMS
193 MB_UV_TO_PREV_SCALED_CLIP_MAT_C0_PS | 194 MB_UV_TO_PREV_SCALED_CLIP_MAT_C1_PS
195 MB_UV_TO_PREV_SCALED_CLIP_MAT_C3_PS | 196 SHADOWMAP_SWITCH_PARTITION_ARRAY_0
197 SHADOWMAP_SWITCH_PARTITION_ARRAY_1 | 198 SHADOWMAP_DISTANCE_BIAS | 199 SHADOWMAP_SCALE
200 SHADOWMAP_PARTITION_UV_OFFSET | 201 SHADOWMAP_CASCADE_MASK | 202 SHADOWMAP_DISTANCE_BLEND | 203 ZNEAR
204 LIGHTING_LOOKUP_SCALE | 205 INV_SCENE_PROJECTION | 206 RIM_LIGHT_0_DIR | 207 RIM_LIGHT_0_COLOR
208 RIM_LIGHT_1_DIR | 209 RIM_LIGHT_1_COLOR | 210 RIM_LIGHT_TECHNIQUE | 211 DEBUG_BUMPMAP
212 MATERIAL_COLOR | 213 FOG | 214 FOG_COLOR_LINEAR | 215 FOG_COLOR_GAMMA | 216 FOG_SUN_CONSTS
217 FOG_SUN_COLOR_LINEAR | 218 FOG_SUN_COLOR_GAMMA | 219 FOG_SUN_DIR | 220 FOG_SKY_FOG | 221 FOG_HEIGHT_FOG
222 ATMOS_FOG_PARMS_0 | 223 ATMOS_FOG_PARMS_1 | 224 ATMOS_FOG_PARMS_2 | 225 ATMOS_FOG_PARMS_3
226 ATMOS_FOG_PARMS_4 | 227 ATMOS_FOG_PARMS_5 | 228 ATMOS_FOG_PARMS_6 | 229 FOG_SKY_DIR | 230 FOG_DEPTHHACK
231 GLOW_SETUP | 232 GLOW_APPLY | 233 GLOW_SETUP_ALT_COLOR_SCALE | 234 COLOR_BIAS | 235 COLOR_TINT_BASE
236 COLOR_TINT_DELTA | 237 COLOR_TINT_QUADRATIC_DELTA | 238 PERCEPTUAL_PARMS | 239 FILMIC_PARMS
240 FILMIC_SHOULDER_PARMS | 241 FILMIC_TOE_PARMS | 242 TONEMAP_PARMS | 243 HDR_STAGE_PARMS
244 UI3D_UV_SETUP_0 | 245 UI3D_UV_SETUP_1 | 246 UI3D_UV_SETUP_2 | 247 UI3D_UV_SETUP_3 | 248 UI3D_UV_SETUP_4
249 UI3D_UV_SETUP_5 | 250 HUDFX_PARMS | 251 HUDFX_PARMS2 | 252 SSSS_PARAMS | 253 SSSS_SCALE
254 SSSS_KERNEL_0 | 255 SSSS_KERNEL_1 | 256 SSSS_KERNEL_2 | 257 SSSS_KERNEL_3 | 258 SSSS_KERNEL_4
259 SSSS_KERNEL_5 | 260 SSSS_KERNEL_6 | 261 SSSS_KERNEL_7 | 262 SSSS_KERNEL_8 | 263 SSSS_KERNEL_9
264 SSSS_KERNEL_10 | 265 SSSS_KERNEL_11 | 266 SSSS_KERNEL_12 | 267 OUTDOOR_FEATHER_PARMS | 268 ENVMAP_PARMS
269 SUN_SHADOWMAP_PIXEL_ADJUST_ARRAY_0 | 270 SUN_SHADOWMAP_PIXEL_ADJUST_ARRAY_1
271 SUN_SHADOWMAP_PIXEL_ADJUST_ARRAY_2 | 272 SUN_SHADOWMAP_CASCADE_V_CLAMP
273 SUN_SHADOWMAP_NEAR_FAR_PLANE | 274 SPOT_SHADOWMAP_PIXEL_ADJUST | 275 SPOT_SHADOWMAP_PIXEL_ADJUST_1
276 SPOT_SHADOWMAP_PIXEL_ADJUST_2 | 277 SPOT_SHADOWMAP_PIXEL_ADJUST_3
278 SPOT_SHADOWMAP_PIXEL_ADJUST_ARRAY_0 | 279 SPOT_SHADOWMAP_PIXEL_ADJUST_ARRAY_1
280 SPOT_SHADOWMAP_PIXEL_ADJUST_ARRAY_2 | 281 SPOT_SHADOWMAP_PIXEL_ADJUST_ARRAY_3
282 SPOT_SHADOWMAP_ZTRANSFORM | 283 SPOT_SHADOWMAP_ZTRANSFORM_1 | 284 SPOT_SHADOWMAP_ZTRANSFORM_2
285 SPOT_SHADOWMAP_ZTRANSFORM_3 | 286 SPOT_SHADOWMAP_ZTRANSFORM_ARRAY_0
287 SPOT_SHADOWMAP_ZTRANSFORM_ARRAY_1 | 288 SPOT_SHADOWMAP_ZTRANSFORM_ARRAY_2
289 SPOT_SHADOWMAP_ZTRANSFORM_ARRAY_3 | 290 COMPOSITE_FX_DISTORTION | 291 POSTFX_FADE_EFFECT
292 SCENE_UV_MULT | 293 SCENE_VIEW_MULT | 294 TEXCOORD_CLAMP_0 | 295 TEXCOORD_CLAMP_1
296 VIEWPORT_DIMENSIONS | 297 FRAMEBUFFER_READ | 298 THERMAL_COLOR_OFFSET | 299 THERMAL_FADE_CONTROL
300 THERMAL_FADE_COLOR | 301 PLAYLIST_POPULATION_PARAMS | 302 TESSELLATION_PARMS | 303 TESSELLATION_PARMS2
304 EYE_PARAMETER_DVAR | 305 MP_RIM_PARAMETER_DVAR1 | 306 MP_RIM_PARAMETER_DVAR2 | 307 HEAD_ROT_MAT_R0
308 HEAD_ROT_MAT_R1 | 309 HEAD_ROT_MAT_R2 | 310 GUN_SIGHT_COLOR | 311 GUN_RETICLE_COLOR
312 BASE_LIGHTING_COORDS | 313 LIGHT_PROBE_AMBIENT | 314 CLIP_SPACE_LOOKUP_SCALE
315 CLIP_SPACE_LOOKUP_OFFSET | 316 PARTICLE_CLOUD_TEXTURE_ATLAS_SETTINGS | 317 PARTICLE_CLOUD_VEL_WORLD
318 PARTICLE_CLOUD_MATRIX0 | 319 PARTICLE_CLOUD_MATRIX1 | 320 PARTICLE_CLOUD_MATRIX2
321 PARTICLE_CLOUD_SPARK_COLOR0 | 322 PARTICLE_CLOUD_SPARK_COLOR1 | 323 PARTICLE_CLOUD_SPARK_COLOR2
324 PARTICLE_FOUNTAIN_PARM0 | 325 PARTICLE_FOUNTAIN_PARM1 | 326 LIT2D_BLOODSPATTER_FADESHARPNESS
327 BLUR_SCENE_PIXEL_POS_TO_UV | 328 REACTIVEMOTION_CENTERS | 329 REACTIVEMOTION_CENTERS
330 REACTIVEMOTION_CENTERS | 331 REACTIVEMOTION_CENTERS | 332 REACTIVEMOTION_CENTERS
333 REACTIVEMOTION_CENTERS | 334 REACTIVEMOTION_CENTERS | 335 REACTIVEMOTION_CENTERS
336 REACTIVEMOTION_CENTERS | 337 REACTIVEMOTION_CENTERS | 338 REACTIVEMOTION_CENTERS
339 REACTIVEMOTION_CENTERS | 340 REACTIVEMOTION_CENTERS | 341 REACTIVEMOTION_CENTERS
342 REACTIVEMOTION_CENTERS | 343 REACTIVEMOTION_CENTERS | 344 DEPTH_FROM_CLIP | 345 CODE_MESH_ARG_0
346 CODE_MESH_ARG_1 | 347 CODE_MESH_ARG_3 | 348 CODE_MESH_ARG_4 | 349 SCRIPT_CONST0 | 350 VIEW_MATRIX
351 INVERSE_VIEW_MATRIX | 352 TRANSPOSE_VIEW_MATRIX | 353 INVERSE_TRANSPOSE_VIEW_MATRIX
354 PROJECTION_MATRIX | 355 INVERSE_PROJECTION_MATRIX | 356 TRANSPOSE_PROJECTION_MATRIX
357 INVERSE_TRANSPOSE_PROJECTION_MATRIX | 358 VIEW_PROJECTION_MATRIX | 359 INVERSE_VIEW_PROJECTION_MATRIX
360 TRANSPOSE_VIEW_PROJECTION_MATRIX | 361 INVERSE_TRANSPOSE_VIEW_PROJECTION_MATRIX
362 SHADOW_LOOKUP_MATRIX | 363 INVERSE_SHADOW_LOOKUP_MATRIX | 364 TRANSPOSE_SHADOW_LOOKUP_MATRIX
365 INVERSE_TRANSPOSE_SHADOW_LOOKUP_MATRIX | 366 SHADOW_LOOKUP_MATRIX1 | 367 INVERSE_SHADOW_LOOKUP_MATRIX1
368 TRANSPOSE_SHADOW_LOOKUP_MATRIX1 | 369 INVERSE_TRANSPOSE_SHADOW_LOOKUP_MATRIX1
370 SHADOW_LOOKUP_MATRIX2 | 371 INVERSE_SHADOW_LOOKUP_MATRIX2 | 372 TRANSPOSE_SHADOW_LOOKUP_MATRIX2
373 INVERSE_TRANSPOSE_SHADOW_LOOKUP_MATRIX2 | 374 SHADOW_LOOKUP_MATRIX3 | 375 INVERSE_SHADOW_LOOKUP_MATRIX3
376 TRANSPOSE_SHADOW_LOOKUP_MATRIX3 | 377 INVERSE_TRANSPOSE_SHADOW_LOOKUP_MATRIX3
378 WORLD_OUTDOOR_LOOKUP_MATRIX | 379 INVERSE_WORLD_OUTDOOR_LOOKUP_MATRIX
380 TRANSPOSE_WORLD_OUTDOOR_LOOKUP_MATRIX | 381 INVERSE_TRANSPOSE_WORLD_OUTDOOR_LOOKUP_MATRIX
382 PREV_FRAME_WORLD_MATRIX | 383 PREV_FRAME_INVERSE_WORLD_MATRIX | 384 PREV_FRAME_TRANSPOSE_WORLD_MATRIX
385 PREV_FRAME_INVERSE_TRANSPOSE_WORLD_MATRIX | 386 WORLD_MATRIX0 | 387 INVERSE_WORLD_MATRIX0
388 TRANSPOSE_WORLD_MATRIX0 | 389 INVERSE_TRANSPOSE_WORLD_MATRIX0 | 390 WORLD_VIEW_MATRIX0
391 INVERSE_WORLD_VIEW_MATRIX0 | 392 TRANSPOSE_WORLD_VIEW_MATRIX0
393 INVERSE_TRANSPOSE_WORLD_VIEW_MATRIX0 | 394 WORLD_VIEW_PROJECTION_MATRIX0
395 INVERSE_WORLD_VIEW_PROJECTION_MATRIX0 | 396 TRANSPOSE_WORLD_VIEW_PROJECTION_MATRIX0
397 INVERSE_TRANSPOSE_WORLD_VIEW_PROJECTION_MATRIX0 | 398 WORLD_MATRIX1 | 399 INVERSE_WORLD_MATRIX1
400 TRANSPOSE_WORLD_MATRIX1 | 401 INVERSE_TRANSPOSE_WORLD_MATRIX1 | 402 WORLD_VIEW_MATRIX1
403 INVERSE_WORLD_VIEW_MATRIX1 | 404 TRANSPOSE_WORLD_VIEW_MATRIX1
405 INVERSE_TRANSPOSE_WORLD_VIEW_MATRIX1 | 406 WORLD_VIEW_PROJECTION_MATRIX1
407 INVERSE_WORLD_VIEW_PROJECTION_MATRIX1 | 408 TRANSPOSE_WORLD_VIEW_PROJECTION_MATRIX1
409 INVERSE_TRANSPOSE_WORLD_VIEW_PROJECTION_MATRIX1 | 410 WORLD_MATRIX2 | 411 INVERSE_WORLD_MATRIX2
412 TRANSPOSE_WORLD_MATRIX2 | 413 INVERSE_TRANSPOSE_WORLD_MATRIX2 | 414 WORLD_VIEW_MATRIX2
415 INVERSE_WORLD_VIEW_MATRIX2 | 416 TRANSPOSE_WORLD_VIEW_MATRIX2
417 INVERSE_TRANSPOSE_WORLD_VIEW_MATRIX2 | 418 WORLD_VIEW_PROJECTION_MATRIX2
419 INVERSE_WORLD_VIEW_PROJECTION_MATRIX2 | 420 TRANSPOSE_WORLD_VIEW_PROJECTION_MATRIX2
421 INVERSE_TRANSPOSE_WORLD_VIEW_PROJECTION_MATRIX2
```

## The light set: keys

The tone map and post keys of a light set, in the engine's key table
(`_time` keys are the blend times): `sunshadowscale`, `spotlimit`,
`qualityspotshadow`, `usedsuncascadecount`, `sunsamplesizenear`,
`sunfilterradius`, `spotfilterradius`, `specularcolorscale`,
`diffusecolorscale`, `veil`, `veilstrength`, `veilbackgroundstrength`,
`tonemap`, `tonemapauto`, `tonemapblend`, `tonemaplockautoexposureadjust`,
`tonemapautoexposureadjust`, `tonemapexposure`, `tonemapexposureadjust`,
`tonemapmaxexposure`, `tonemapadaptspeed`, `tonemapdarkev`, `tonemapmidev`,
`tonemaplightev`, `tonemapdarkexposureadjust`, `tonemapmidexposureadjust`,
`tonemaplightexposureadjust`, `tonemapminexposureadjust`,
`tonemapmaxexposureadjust`, `tonemapwhite`, `tonemapshoulder`,
`tonemapcrossover`, `tonemaptoe`, `tonemapblack`, `aodiminish`, `ssao*`
(mode, strength, power, min/max strength depth, blur radius, width, gap and
gradient falloff, fade and reject depth), `hemiao*`, `mdao*`, `tessellation*`.

The vision (film and light tweaks) keys, with their offsets in the vision
struct: `r_filmEnable` 0, `r_filmBrightness` 4, `r_filmContrast` 8,
`r_filmDesaturation` 12, `r_filmDesaturationDark` 16, `r_filmInvert` 20,
`r_filmLightTint` 24, `r_filmMediumTint` 36, `r_filmDarkTint` 48,
`r_primaryLightUseTweaks` 60, `r_primaryLightTweakDiffuseStrength` 64,
`r_primaryLightTweakSpecularStrength` 68, `r_charLightAmbient` 72,
the view-model equivalents 84..96, `r_lightOverride` 108,
`r_volumeLightScatter*` 112..152, `r_rimLight*` 156..200,
`r_unlitSurfaceHDRScalar` 204, `r_litSurfaceHDRScalar` 208; after those the
glow keys (`r_glow*`, `r_glowBloomPinch`, `r_glowAltColorScale*`) and
`r_hdrSkyIntensity`, `r_hdrSkyColorTint`.

## Exposure (read)

State copied each frame from the light set (or, with `r_tonemapUseTweaks`,
from the dvars): mode, auto, blend, lock, auto adjust, max exposure EV,
fixed EV, exposure adjust, adapt speed, the dark/mid/light EV points and
adjusts, min/max adjust, then black, white, crossover, shoulder, toe
(order matched by defaults and ranges).

Two conversions, with `b` a per-view scalar that is 0.001 when it is unset
(zero or less; where it is set from was not traced):

* EV to exposure: `E(ev) = 0.18 * 2^(2.84 - ev) / b`. At EV 10, b 0.001:
  **1.2586**; one stop per EV.
* Scene luminance to EV: `EV = log2(0.18 / (E b)) + 2.84` (the inverse).

The frame's exposure:

1. Luminance `L`: (31 Oct, read: the scene is first reduced by halving until it fits 160x90, over the view rect clipped to the scene rect; then `cs_logaccumfromtex2d` / `continue` / `finish` in `code_post_gfx_mp`) the geometric mean (log average) of Rec. 709 luminance
   (0.2126, 0.7152, 0.0722) of a set of scene samples, each clamped to
   [6.1e-5, 65504], read back from the GPU.
2. Target grey `T`: with the light set locking the adjust
   (`tonemaplockautoexposureadjust`, default 1): `T = 0.18 * 2^autoadjust`
   (default adjust 0: **T = 0.18**); unlocked: the adjust curve below.
3. Automatic: `E = T / L`. Fixed (`tonemapauto` 0): `E = E(tonemapexposure)`.
4. If `tonemapblend` is on and `L > 6.1e-5`: the log of `E` moves toward
   the target by `tonemapadaptspeed` (0.02) per frame (state in ln E).
5. `E <- E(EV(E) - tonemapexposureadjust)` = `E * 2^adjust`, then
   `E <- min(E, E(tonemapmaxexposure))` (default max EV -10: no limit).

Adjust curve (unlocked auto): with `x = ln(L / b)`, axis points
`x_k = (ev_k - 2.84) ln 2` for dark, mid, light; `t = max(0, (x - x_d) /
(x_l - x_d))`, `t_m = (x_m - x_d) / (x_l - x_d)`; greys `g_k = (0.18
2^adj_k)^(1/2.2)`; exponent `p = ln((g_m - g_d) / (g_l - g_d)) / ln t_m`;
`T = ((g_l - g_d) t^p + g_d)^2.2`, clamped to `[0.18 2^minadj, 0.18
2^maxadj]`. Defaults (dvars): EVs 2.84 / 7.823 / 12.81, adjusts -3.17 /
-0.0651 / 1.47, min/max -3.17 / 2.3. With `b` 0.001 the EV points are
the scene luminances 0.001 / 0.0316 / 1.0.

## Tone curves

**Linear** (`tonemap` 3, what Rust uses; `r_tonemap` enum: 0 perceptual, 1
Reinhard, 2 filmic, 3 linear, 4 luminance): `out = saturate(E x)^gamma`
(inferred from `tonemap_linear`'s buffer and the shader dump in
`docs/mwr_engine/post.md`; gamma `r_tonemapGamma`, default 1).

**Filmic** (`tonemap` 2). Read: the frame fills a 30-float constant block
for the exposure compute pass with the curve below (the pass folds in the
exposure and writes the tone map's buffer). With black `s0`, white `s1`,
crossover `s2`, shoulder `h`, toe `t`; `f = h^0.25`, `e2 = t^2`:

```
n  = (s2 - s0)(1 - e2)
q  = n / ((s1 - s2)(1 - f) + n)                      output level at the crossover
toe      (c <= s2):  y = (q(1-e2) c + q s0 (e2-1)) / (-e2 c + s2 - (1-e2) s0)
shoulder (c >  s2):  y = ((q f - q + 1) c + q s1 (1-f) - s2) / (f c + (1-f) s1 - s2)
```

y(s0) = 0, y(s1) = 1, y(s2) = q from both sides (unit tests). The block also
holds `s2`, `2^(range/2)` for `r_tonemapHighlightRange` (default 16, "dynamic
range, which determines white point luminance"; its use is in the pass's
shader, not read) and `1/gamma`. Defaults (dvars): black 0, white 512,
crossover 2, shoulder 0.95, toe 0.2; with them q = 0.228.

**Read (9 Nov 2026, `code_post_gfx_mp`'s `cs_logaccumfinish`, disassembled):**
the exposure enters as `c = x E / 0.18`: the pass writes the rows as
`[a E/0.18, cc E/0.18, b, d]` and the crossover as `s2 / (E/0.18)`;
`tonemap_filmic` picks the toe row for `x <= crossover`, takes
`saturate((x a + b) / (x cc + d))` and raises it to `1/gamma`.
`tonemap_linear` is `saturate(x E)^(1/gamma)` (no 1/0.18). The same pass
does, in order: `L = exp(sum ln(lum) / n)`; the target `T` (locked:
`0.18 2^autoadjust`; else the adjust curve on `L / b`); automatic
`E = T / L` or fixed `E = 0.18 / (2^(EV - 2.84) b)`; the blend
`lnE += (ln E_target - lnE) speed` when `L > 6.1e-5` (state in the buffer's
`[0].w`, before the adjust); then `E <- E 2^adjust` (through EV) and the cap.
So an automatic exposure shows the log-average at `c = 1`, not 0.18: our
`c = E x` had every filmic map 2.47 stops dark, which the fitted
`auto_key_boost` (3.5-8) made up for unevenly (dark Terminal, white Oil).
Now `tonemap::FILMIC_GAIN`; the boost and the fixed light sets' black-point
cap are off, and the metered exposure is no longer counted twice for fixed
light sets. `BO2MP_H1_FILMIC_EXPOSURE=old` brings all of that back.

## Dvar defaults read (tonemap block; names matched)

| setting | default | range |
|---|---|---|
| r_tonemapAuto / Blend / LockAutoExposureAdjust | 1 / 1 / 1 | bool |
| r_tonemapAutoExposureAdjust | 0 | -20..20 |
| r_tonemapExposure (EV) | 10 | -10..32 |
| r_tonemapExposureAdjust | 0 | -20..20 |
| r_tonemapMaxExposure (EV) | -10 | -10..32 |
| r_tonemapAdaptSpeed | 0.02 | 0.0001..1 |
| r_tonemapDark / Mid / LightEv | 2.84 / 7.823 / 12.81 | -10.16..28.81 |
| r_tonemapDark / Mid / LightExposureAdjust | -3.17 / -0.0651 / 1.47 | -7.5..7.5 |
| r_tonemapMin / MaxExposureAdjust | -3.17 / 2.3 | -7.5..7.5 |
| r_tonemapBlack / White | 0 / 512 | 0..4096 |
| r_tonemapCrossover / Shoulder / Toe | 2 / 0.95 / 0.2 | 0..4096 / 0..0.9999 / 0..0.9999 |
| r_tonemapHighlightRange | 16 | to 32 |
| r_tonemapGamma | 1 | to 8 |
| an unidentified float | 0.001 | 0..1000 (the number the view scalar `b` falls back to) |

## Compared with our MWR path

| | ours before | the exe |
|---|---|---|
| exposure per EV | one stop | one stop (read) |
| exposure at EV 10 | 0.70 (fitted; the user's note says 0.62) | 1.2586 (b 0.001) |
| auto key | 0.18 in `h1_native`; 0.15 default in the BO3-look path (per-map keys from `look_keys.txt`, empty) | 0.18 x 2^adjust, locked by default; a curve when unlocked |
| filmic shape | `e c / (1 + 0.10 e c)`, both segments | two rational segments from black/white/crossover/shoulder/toe |
| gamma | 1 | `1/r_tonemapGamma`, 1 |

Why the fitted 0.70 and the exe's 1.26 are not the same number: the fit was
made against HorizonMW's picture of Rust, a map without a light set, which
uses the *linear* tone map with automatic exposure (`E = 0.18 / L`). The
number then absorbs the scene's own level and is not the exposure of EV 10.
The structure agrees (one stop per EV; `0.18 / average`; `h1_native`'s
default key is already 0.18). The exact filmic curve has no picture to
check it against: the retail zones carry no light sets (`_lightsets_hdr.csv`
comes with HMW's user maps), and no staged zone had one.

`BO2MP_H1_TONEMAP=exact|fitted` (`render_h1::tonemap`): `exact` builds the
filmic buffer from the light set's own black, white, crossover, shoulder and
toe and the EV formula, and `h1_native` takes the game's key
(`tonemap::auto_key`) unless `--key` is given. **Default: exact since 30 Oct (owner: use the game's own data); before that it was fitted**, because
the fitted numbers do not agree with the exact ones on the one known map and
the exact curve is unchecked against a picture. `h1_native` prints a per-map
`h1 vision:` line (tone map mode, auto or fixed, EV, adjust, lock, the five
curve numbers and the crossover level, the exposure at the measured average,
film on or off); the BO3-look path (`BO2MP_BO3_LOOK_H1`) prints one too, but
it draws BO3's tone map, not MWR's, so only the key comparison applies there:
its auto key (0.15 default) is 0.26 stop below the game's 0.18 (nothing was
changed there: that value is fitted to HorizonMW's pictures).

## Film path and light grid tweaks (read, 30 Oct, second pass)

* **Vision parse.** A table of 39 vision keys `{name, offset, type}` drives
  the parser (type 0 bool, 1 float, 3 int, else vec3 read as `"%f %f %f"`).
  The film keys sit at struct offsets enable 0, brightness 4, contrast 8,
  desaturation 12, desaturation dark 16, invert 20 (bool), light tint 24,
  medium tint 36, dark tint 48. A vision struct is 212 bytes.
* **Vision blend.** Two visions blend per key with `w = (3 - 2t) t^2`
  (smoothstep of the blend fraction): floats and vec3s lerp by `w`, bools
  OR together, ints take the larger.
* **Into the frame.** Each frame the view's active vision (base, night
  vision, flash, and so on, picked by state) is copied into the scene
  definition in the order enable, brightness, contrast, desaturation,
  desaturation dark, invert, then the tints **dark, medium, light**.
* **Fade.** While a vision fraction `p` is below 1 the film fields move
  toward neutral: brightness `p b - (1 - p)`, and every other film value
  `p v` (the tints and contrast scale toward 0).
* **Light grid tweaks (models).** The model lighting sample is 9 colour
  coefficients (SH, 36 floats; the first is the average level). With the
  light grid tweak dvars on, each coefficient `c_i` becomes
  `(c_i I - c_0)(1 + C) + c_0`, with `I` the intensity and `C` the contrast,
  and `c_0` the untweaked first coefficient. Defaults (`I` 1, `C` 0) leave it
  unchanged. Our native MWR path uses Modern Warfare 2's 56-colour shell
  layout, not the 9-coefficient one, so this is recorded and not wired.

## Fog constants and the post chain (read, 30 Oct, third pass)

* **Where code constants live.** Some engine constants are written straight
  into a global vec4 array, one slot per `CONST_SRC_CODE` index, each with a
  per-constant version counter bumped on write. The slots were identified by
  their writers: the sun-flare / volume-light function fills
  `VOLUME_LIGHT_SCATTER_1..3`, the veil writer fills `VEIL_PARAMS`, the FXAA
  writer `FXAA_RCPFRAMEOPT`, and the fog writer the fog slots below. Not
  every constant is written this way: the film slots (`COLOR_BIAS`,
  `COLOR_TINT_*`) have no direct writer; they come from the per-view block
  (fourth pass, Lighting section).
* **How constants reach the array (read).** The renderer's front end
  records commands into a command stream and the back end replays them. One
  command type (the 26th handler of 27) sets one engine constant: it holds
  the slot index and four floats, and on replay it compares the four floats
  with the slot and, only if they differ, copies them in and bumps that
  slot's 16-bit version counter (the counters sit after the array, one per
  constant). So a constant that is never sent keeps its last value, and a
  shader sees a new value only when its version moves. The front-end code
  that emits the film (`COLOR_BIAS`, `COLOR_TINT_*`) set commands is still
  not found: no code names those slot numbers as fixed values, so they are
  sent through a table or a computed index.
* **Fog writer (read).** Each view, the fog writer takes the view's rotation
  rows and:
  * normal (atmospheric) fog: `ATMOS_FOG_PARMS_4.xyz` = the fog's sun
    direction rotated into **view space** (`.w` kept: the sky distance);
    `ATMOS_FOG_PARMS_6.xyz` = world up rotated into view space, `.w` = the
    light set's sky max opacity times a dvar scale (default 1, matched);
  * sun-fog mode: `FOG_SUN_DIR` = the sun-fog direction rotated into view
    space (all four rows), and `ATMOS_FOG_PARMS_6` as above with `.w` from
    the transform.

  This confirms `look::fog`'s P4 and P6 layout; nothing to change.
  (31 Oct, night, read: the sky max opacity `ATMOS_FOG_PARMS_6.w` is the
  view's `ATMOS_FOG_PARMS_2.w`, the fog's max opacity, times a float dvar
  with default 1, range 0..1; the fog shader uses P2.w as the opacity for
  surfaces and P6.w for the sky. `look::fog_exact` sets it.)
* **Post-opaque material pick (read, 31 Oct, night).** The fog pass is one
  material from a table of 19 (`postopaquefx_` + optional `ssaolq_` /
  `ssaohq_` + `fog` / `dfog` + optional `_hf` + optional `_fsf`), indexed
  by bits: SSAO quality (0..2), 4 for atmospheric fog, 8 for sun fog, 0x10
  for height fog, 0x20 for the sky's angular falloff. Atmospheric fog is
  drawn when its max opacity is above 0; height fog when ln 2 / height
  density (the height half-plane distance) is below 99000; sky falloff
  when the falloff angle range is below 8910 degrees (the fog string sends
  9000 when the falloff is off). The `_fsf` materials' technique is named
  `_flf` in `code_post_gfx_mp`. `look::post_opaque_technique`.
* **Legacy fog (read, 31 Oct, night).** When a fog script has
  `atmosFogEnabled 0` (AW/MW3-era usermaps: Big Ben, Highrise 2, Instinct,
  Lair) the fog builder takes its other branch and fills the legacy slots:
  `FOG = (0, 1 - maxOpacity, -d, d * start)`; `FOG_COLOR_LINEAR` = the fog
  colour (sRGB bytes to linear) times the intensity conversion used for
  the atmospheric in-scatter, `FOG_COLOR_GAMMA` the bytes; `FOG_SUN_CONSTS
  = (-d * scale, cos(end angle), 1 / (cos begin - cos end), -d)`, the sun
  colour likewise, `FOG_SUN_DIR` = the normalised sun direction (rotated
  into view space by the fog writer); `FOG_SKY_FOG = (min angle, 1 / (max -
  min), sky intensity * maxOpacity, 0)` in radians (max nudged above min);
  `FOG_HEIGHT_FOG = (-start, e^(...) at the eye, height falloff, 0)`; with a
  dvar on, `FOG_SKY_DIR` = up in view space and `FOG_DEPTHHACK` = FOG
  blended by the sky-opacity dvar (1: FOG). The post pass is then
  `postopaquefx_dfog[_hf]` (`_hf` when ln 2 / the height falloff is below
  99000). The shader (`postopaquefx_dfog_hf`, read): transmittance
  `max(1 - maxOpacity, e^(-d a))` with `a = (dist - start) x FOG_HEIGHT.y x`
  the height integral, blended toward the sun scale across the sun's fade;
  the sky takes `1 - FOG_SKY_FOG.z x sat(((1 - cos) pi/2 - min) / range)`.
  INFERRED: which script key feeds which fog-state field (start, max
  opacity, d = ln 2 / halfway distance, sun fade angles, `normalFogScale` as
  the sun scale, the sky fog angles and intensity, the height falloff, and
  that the eye term is `e^(-(eye z - base) falloff)` as for the atmospheric
  fog): taken from the fog string's order and the shader, the client parser
  was not read. `look::fog_legacy`; `BO2MP_H1_LEGACY_FOG=0` off (no fog, as
  before).
* **Where a map's fog comes from (zones, 31 Oct, night).** Every retail map's
  patch zone carries `maps/createart/<map>_fog_hdr.gsc` as a raw file (and
  compiled), with one `create_vision_set_fog("<vision set>")` block per
  vision set; the map's base vision set is the map name. Most maps looked
  fogless only because our zone walk stopped at the patch zone's
  `addon_map_ents` asset before reaching the script.
* **Post chain order (read).** Per view: set up the exposure state, the
  render-target and AA state, the auto-exposure measurement, then the
  perceptual tone-map parameters (`PERCEPTUAL_PARMS` = four values from the
  view's tone-map state), the exposure compute and the tone map, then the
  colour pass. The colour pass picks its material from a set of variants by
  flags: the film, CLUT and dark-vision variants, plus an extra pass when
  bloom or veil is on. It is drawn as a full-screen quad.

## Collision (read, 30 Oct; `fastfile_h1::map`, `assets::lane::h1_map`)

Same method as the light work above, following `docs/T7_NATIVE.md`'s
*Collision*: the map load to the clip map, the trace job, the world trace
down to brushes and terrain, and the model trace. **Read** means the
decompile shows it; **inferred** is said.

### Where the clip map lives (read)

* The map load takes the map name (it fails with "CM_LoadMap: NULL name"
  without one), finds the zone's clip map asset (type 0x18, named
  `col_map_mp` in the asset name table), and the engine keeps it as one
  global `clipMap_t`; the load then allocates, per partition, a "visited"
  stamp array for each of 11 trace contexts (so a partition is traced
  once per trace) and hands back the clip map's checksum.
* Every field the code reaches in that global sits where
  `fastfile_h1::layout::clipMap_t` puts it (640 bytes): `info` at 16
  (plane count, materials, leaf brush nodes, AABB trees at 88, the static
  model tree at 96/104, brushes 152, brush bounds 160, brush contents 168,
  partition count 232, static model list 256), nodes 280, leafs 296,
  sub-models 304/312, map entities 320, dynamic entities 392 on,
  scriptable entities 480 on, checksum 576. Record sizes the code steps
  by: `cbrush_t` 48, brush bounds 24, brush contents 4, `cbrushside_t` 8
  (plane index u32, material u16), `cplane_s` 20, `ClipMaterial` 16
  (surface flags at 8), `cLeaf_t` 44 (brush contents at 8, terrain
  contents at 12), `cNode_t` 16 (plane pointer, two i16 children, a
  negative child is `~leaf`), `cLeafBrushNode_s` 24, `CollisionAabbTree`
  32, `CollisionPartition` 16 (tri count, border count, first vertex
  segment, first triangle, borders), `CollisionBorder` 28, a vertex
  segment 0x3000 bytes (1024 vertices), `cmodel_t` 88 (its own
  `ClipInfo` pointer at 32, its leaf at 40), `cStaticModel_s` 88 (model,
  origin, inverse scaled axes at 20, bounds at 56), `SModelAabbNode` 28
  (bounds, first child u16 at 24, child count u8 at 26). **No record of
  our walker was misread**: every size and offset matches.
* The trace takes its `ClipInfo` through a pointer: the world's is the
  global's `info`, a brush model's is its `cmodel_t::info` (so a sub-model
  traces its own brushes and terrain).

### The trace (read)

* One trace job: a flag word picks what it meets: the world (brushes and
  terrain), the **static models**, entities, other entity kinds, and
  dynamic entities. The script, server and client trace wrappers set the
  static model flag only for a **point trace** (zero extents: the sum of
  the half sizes is 0) and only when their caller asks for it; a box
  trace goes to the entities instead. **A placed model is never met by a
  box or capsule trace: the player's movement collides with brushes,
  terrain, brush models and entities only; bullets and sight lines meet
  placed models.**
* World trace setup: start and end are moved to the box's middle; the
  trace is a capsule: radius = min(half x, half y), the segment along z of
  half length `half z - radius`; per axis a "no movement" flag when
  |delta| < 1e-6. A zero-length trace (or one asked as a position test)
  is a position test: the leaves the box (grown by 1 unit on each axis)
  touches, up to 1024, then their brushes and terrain.
* BSP walk: per node an axial plane's offset is the box's half size on
  that axis + **0.125**, a slanted plane's the half-size vector's length +
  0.125 (0.125 alone for a point trace); a node is split when |d1 - d2|
  > 4.77e-7 (2^-21), the near child first, as Quake III's.
* Leaf: brushes when the mask meets the leaf's brush contents (through
  its leaf brush node tree, each brush's swept box tested against its
  bounds first), terrain when it meets the leaf's terrain contents (its
  AABB trees, each partition traced once per trace, its triangles with
  the vertex segment `firstVertSegment * 1024`, and its borders when the
  trace moves horizontally).
* Brush: the IW4 capsule-through-brush: for a side plane the distance is
  `n.p - (dist + radius + |n . (0, 0, half z - radius)|)`; enter fraction
  `(d1 - 0.125) / (d1 - d2)` (**SURFACE_CLIP_EPSILON 0.125**), a plane
  with `d1 > 0` and `d2 >= min(d1, 0.125)` misses; leave fraction
  `d1 / (d1 - d2)`; the six bounds planes the same with the box's
  extents; start/all solid as Quake III's; the hit's surface flags from the
  side's (or axial side's) material; a glass brush asks the glass system.
  This is what `trace_iw4`/`clipmap_iw4` already do (an IW4 port), so the
  brush and terrain trace was **not changed**.
* After the trace: `walkable = normal.z >= 0.7`.
* Movement (read in the step-slide routine and its callers): step 18
  units, walkable 0.7, clip-velocity overbounce `dot - |dot| * 0.001`. The
  one immediate mask of the movement setups is **0x2810011**
  (`MASK_PLAYERSOLID` as Black Ops II's), with the contents of entities
  linked to the mover taken out; the player's own mask comes from its
  entity's clip mask, whose value was not traced to a constant
  (inferred: the same 0x2810011, which the match keeps). No `MASK_SHOT`
  immediate (0x2806831) is in the code; the bullet mask was not found
  (the match keeps Black Ops II's).

### Placed models (read)

* A tree over the placements (`sCollisionTree`: `SModelAabbNode`s; a
  node whose first child is below the model count lists placements, else
  nodes). A placement is traced when its **model's** contents meet the
  mask, its byte at +80 is 0 or equals a per-map value set elsewhere (not
  followed), and the ray meets its bounds.
* The ray goes into model space by the inverse scaled axes (`local =
  (p - origin) * M`, M's rows the stored rows: what
  `clipmap_iw4::trace_static_model` does).
* The model trace (`collLod` < 0: nothing): every surface of the
  collision LOD, every rigid vertex list of each, in order, paired with
  the next `XModelCollSurf_s` (lists past the count of collision surfaces
  are not traced); the surface's contents must meet the mask and the ray
  its bounds; then the list's triangles (its run of `triIndices2`, which
  index the surface's vertices), through the list's
  `XSurfaceCollisionTree` when it has one, else all of them. A triangle
  is **one-sided**: its normal is `(c - a) x (b - a)`; it is hit when the
  start is in front and the ray goes in (`t = n.(a - start) / n.dir` in
  [0, fraction)) inside the triangle; the fraction is that crossing
  exactly (no 0.125 back-off), the hit's normal is turned back by the
  axes and normalised, surface flags and contents come from the collision
  surface.

### What changed here

* `fastfile_h1` keeps each model's `XModelCollSurf_s` list
  (`XModelRec::coll_surfs`), each surface's `triIndices2`
  (`SurfaceRec::tris2`) and the placement's byte at +80
  (`ClipStaticModel::layer`).
* The MWR lane no longer appends placed models' triangles to the world
  mesh (with its 1.5 M budget, the coarsest-LOD fallback Shipment fell
  to, the radius cut and the spawn-box clearing). Each placement goes to
  `ClipCollision::static_models` with its model's collision as the game
  reads it (one collision surface per rigid list, the triangles as
  `XModelCollTri` planes with the game's front), which the match's bullet
  trace already walks (`sim::bullet_collision`, the static model grid).
  So players walk round props on the map's own clip brushes, as in MWR,
  and bullets stop on the props' collision triangles with their
  surface flags.
* The model trace now takes the plane crossing exactly for MWR
  (`XModelColl::trace_offset` 0; every other game keeps IW4's 0.125), and
  a model's collision surfaces are shared between its placements (`Arc`:
  Crash places 13,327 models with 13.9 M triangles between them: its
  collision stage went from +257 MiB to +72 MiB resident).
* This is the default. `BO2MP_H1_COLLISION=soup` brings back the old
  collision for comparison (models' collision LOD triangles in the world
  mesh, met by the player's box, with the radius cut, the 1.5 M budget and
  the spawn-box clearing).
* Not matched: the placement layer byte is kept but not compared (0 on
  every placement of the four maps checked, so it changes nothing there);
  the per-list `XSurfaceCollisionTree` is not built (every triangle of a
  list is tested, as the game does for a list without a tree: same hits).

### Checks (`drop_test_h1`)

`H1_TEST_MAP=<zone.ff>[,...] cargo test -p assets --release --lib
drop_test_h1 -- --ignored --nocapture` loads each map through the lane,
checks every brush (unit planes; its planes enclose a solid that reaches
its bounds within 0.25 on each face) and runs `t7_map`'s drop test core
(`drop_test_world`): spawns and a grid of drawn world floors where a
player can get, the box dropped from 40 above, the match's pmove run 2 s
from up to 600 drops, every world player clip walked into.

Run 30 Oct on four maps (no GPU; retail zones staged from the owner's
PC, Courtyard a Horizon MW usermap):

| map | brushes checked (bad planes / empty / short) | placed models for the model trace | drops | pmove runs | player clips walked into | failures |
|---|---|---|---|---|---|---|
| mp_shipment | 10,176 (0 / 0 / 0) | 4,025, 4.43 M tris | 513 | 513 | 9 of 200 | 0 |
| mp_killhouse | 11,840 (0 / 0 / 0) | 3,249 (6 without a collision LOD), 3.03 M tris | 669 | 669 | 60 of 526 | 0 |
| mp_crash | 55,422 (0 / 0 / 0) | 13,327, 13.9 M tris | 1,184 | 1,184 | 149 of 1,857 | 4 fell through, 4 pmove did not land (the same 4 spawns), 2 walk holes, 14 spawns in solid |
| mp_courtyard_ss | 8,350 (0 / 0 / 0) | 2,393, 0.71 M tris | 728 | 728 | 80 of 532 | 26 spawns in solid |

* Every brush of the four maps encloses a solid reaching its bounds: the
  brush sides' plane indices and the planes are read right (no plane fix
  is needed, unlike Black Ops III's mp_apartments).
* Crash: the four spawns near (300, -1730, 220) stand over a drawn
  surface at 220 that the collision lacks; the box lands 130 to 165 lower
  on the next drawn floor (it is what the game's clip map holds; the
  surface's material was not identified). The two walk holes at
  (336..418, -800) are drawn floors 85 to 125 above the collision.
* "Spawn in solid" here counts a spawn put on the highest drawn surface
  within 128 under it: on Crash and Courtyard that surface can lie inside
  a brush (a drawn floor under a clip); the lane's own census, from the
  spawn record itself, has 0 in solid on Shipment, Killhouse and Crash and
  1 on Courtyard.
* Before (soup): on Shipment the models' triangles went over the 1.5 M
  budget and every model collided with its coarsest LOD; players walked
  on prop meshes the game lets them pass through only where clips stand.
## Lighting (read, 30 Oct, fourth pass)

Method: every instruction that touches the back end's constant array or
its version counters (by absolute reference, by displacement from the
source state, and by computed index) was listed and grouped by slot and
writer; then the writers of the lighting slots were decompiled.

### How constants get into the array (read; corrects the third pass)

There are **three** routes, not one:

1. **Per-view block.** The front end fills a block of 350 vec4s inside each
   view's info, laid out exactly like the constant array (slot `i` at
   `16 i`). A default block exists for views that set nothing. When the
   back end starts a view it clears its state, copies this block into the
   array whole, and marks per-draw constants (a per-constant
   update-frequency table) as stale. This is where the **film**,
   `HDR_STAGE_PARMS`, rim lights, previous-frame and motion-blur matrices,
   SSR and distortion constants come from; no code names their slots
   directly in the array, which is why the earlier scans missed them.
2. **Per-draw writes by computed index** in the back end: the primary,
   spot and omni light setters, the static-model lighting setters, two
   "set if changed" helpers, and the SSS kernel loop.
3. **The set-constant command** (type 26 by the dispatcher's 1-based
   count; header = u16 size, u8 type, u8 flags; then slot, four floats).
   Its one emitter is called from three places, for `SCRIPT_PARMS` and
   `LIT2D_HUDBLOOD_COLOR` only. No lighting constant uses it.

### Primary light (the sun): `LIGHT_POSITION/DIFFUSE/SPECULAR` (read)

* The scene's light list is the world's primary lights: at map load each
  `ComPrimaryLight` (the zone's) is copied into the renderer's light
  struct field by field; **the colour is copied unscaled**. Per frame the
  list is copied into the frame; script overrides (a sun colour override,
  or "colour normalised by its largest channel times an intensity") apply
  only when set; the sun direction is also kept for the frame.
* Per frame: the diffuse and specular colour scales are the light set's
  `diffusecolorscale` / `specularcolorscale`, or, when a tweak toggle dvar
  is on, two tweak dvars (defaults **1** and 1, ranges 0..10 / 0..25). With
  no light set, the light set state takes the dvars' defaults: **1, 1**.
* Per draw, for primary light `n` (0..3, constants `9 n + k`):
  `LIGHT_POSITION = (dir, 0)`, `LIGHT_DIFFUSE = (colour * diffuseScale, 1)`,
  `LIGHT_SPECULAR = (colour * specularScale, 1)`, the scales multiplied by
  the vision's `r_primaryLightTweakDiffuseStrength` /
  `...SpecularStrength` when the view's primary-light tweak flag is on
  (`r_primaryLightUseTweaks`), and by the view-model pair when drawing the
  view model.
* **Compared with ours:** the native path used colour x pi (fitted to
  HorizonMW's Rust). The exe has no pi. Shipment's sun is (15.0, 14.32,
  10.31) in the zone, and that is what the shader gets (light set absent).
  `render_h1::light` has both behind `BO2MP_H1_LIGHT=exact|fitted`,
  **default exact since 30 Oct (owner's decision; `fitted` keeps the old pi-scaled sun)**; earlier note: the world shader as decompiled (`mwr_engine/world_lit.md`)
  adds `NdotL * sunDiffuse` to the lightmap irradiance with no pi either,
  so the exact value should be right, but the fitted pi matched a picture
  and the exact one has not been looked at (no GPU here). Something else in
  our path (the lightmap decode or the exposure) may be absorbing the pi;
  flip the default once a picture agrees. `h1_native` notes print an
  `h1 light:` line with the mode, scales and colour.

### Spot and omni lights (read; constants built in `render_h1::local_light`, not drawn natively)

For light `n` the engine writes: `LIGHT_POSITION = (origin - eye, 1 /
radius)`; `LIGHT_DIFFUSE = (colour * diffuseScale, w from the light)`,
`LIGHT_SPECULAR = (colour * specularScale, w from the light)` (same scales
as the sun); `LIGHT_PHYSICALSIZE` = the bulb length and the two values after
it, then the bulb radius (the `ComPrimaryLight` fields, rotated by one);
`LIGHT_FADEOFFSET = (fade offset x, y, 0, 0)`; `LIGHT_SPOTDIR = (dir, shadow
slot + offset)`; `LIGHT_SPOTFACTORS = (1 / (cosInner - cosOuter), -cosOuter
/ (cosInner - cosOuter), exponent, a per-draw value)`; `LIGHT_CUCOLORIS_ANIM`
from the cookie's scroll and rotation at the frame time. A second setter
writes the same values to the `_DB_ARRAY` slots (`36 + n` upward).

Built (3 Nov): `local_light::LocalLight` fills these from the map's spot (type 2) and omni (type 3) lights. INFERRED: w of diffuse and specular is 1, the shadow slot is 0, the per-draw spot value is 0. Still open: which technique slots are the spot and omni lit passes (the exe holds no names for them; `slot` stops at `LIT_DIR_SHADOW` = 15), which lights reach which surface (the primary light environment's up to four), and the shadowed variants.

### Static models (read)

The static model draw, per instance, by its lighting flags:

* **Light grid (0x40):** `BASE_LIGHTING_COORDS = (((h - 1) mod 256) * 4 +
  2) / 1024, (floor((h - 1) / 256) * 4 + 2) / H, 0.5, 1)` with `h` the
  instance's lighting handle and `H` the atlas height (four texels per 256
  tiles, rounded to a power of two). MWR's model lighting atlas is 1024
  texels wide with 4x4 tiles; ours is our own 256-wide layout (Modern
  Warfare 2's), so this is recorded, not copied.
* **Ground lighting (0x20):** `LIGHT_PROBE_AMBIENT` = the four halves of the
  instance's ground lighting, as floats, unscaled. Only the `lprobe`
  techniques read it (7 techniques in Shipment's techset zone). Now set per
  object in `render_h1::scene` (was 0).
* **Model lightmap (0x100, lightmap index set):** `MODEL_LIGHTMAP_PARAMS =
  (scale.x, scale.y, offset.x, offset.y)` of the instance's lightmap info.
  Model-lightmapped statics are not drawn natively yet; recorded.

### Film constants (read; the follow-up from the earlier passes)

Written into the per-view block by the film step each frame:

* Inputs: the vision's film fields (or, with the film tweak toggle, the
  `r_film*` dvars): enable, brightness `b`, contrast `c`, desaturation `d`,
  desaturation dark `dd`, invert, tints dark `D`, medium `M`, light `L`.
  Then three global dvars: `c *= cmul` (default 1), `b += badd` (default
  0), `d = ((1 - d) k + d) d` (k default 1: unchanged). Film off: c 1, b 0,
  d 0, dd 0, no invert, all tints 1.
* With `s = max(d, 2^-12)`, the user brightness `h` (a profile setting,
  -0.99..0.99, default 0, applied by a toggle that is on by default) and
  `g = max(1 - h, 0.01)`:
  `k = c / g` (quadratic mode) or `c s / g` (linear mode);
  `bias = b + 0.5 - 0.5 c - h / g`; invert: `k = -k`, `bias += 1`.
* **Quadratic mode** (a mode dvar, **default on**): `COLOR_TINT_BASE = (k D,
  dd)`, `COLOR_TINT_DELTA = (2k M - 2k D, d - dd)`,
  `COLOR_TINT_QUADRATIC_DELTA = (k D - 2k M + k L, 0)`; linear mode:
  `BASE = (k D, 0)`, `DELTA = (k (L - D), 0)`, `QUAD = 0`.
* `COLOR_BIAS = (bias, bias, bias, 1 / s - 1)`.

This is exactly `look::film` (Modern Warfare 2's mapping) at the defaults,
except `COLOR_BIAS.w`, which was 0 and is now `1 / max(d, 2^-12) - 1`
(`postfx_color2` as decompiled does not read it). Unit test
`look::film_tests`.

### Other per-view lighting constants (read, recorded)

* `HDR_STAGE_PARMS = (v * 1000, 16, 0, 0)`, `v` a float taken per frame from
  a 32-byte table entry picked by the view position (inferred: an exposure
  volume). Ours stays (1, 1, 1, 1); its readers are the sky, effects and
  ocean techniques. Not changed: unverified meaning. (Fifth pass: `v` is
  the view's stage radiometric scale; now set exactly.)
* Rim lights (from the vision's `r_rimLight*`, or tweak dvars): with power
  `p`, the normalisation `(p + 8) / (8 pi)`; `RIM_LIGHT_0_COLOR = (colour *
  intensity0 * norm, max(p, 0.001))`, `RIM_LIGHT_1_COLOR = (colour *
  intensity1 * norm, a vision value)`, `RIM_LIGHT_0_DIR` = the direction from
  two angles (degrees) rotated into the view, `RIM_LIGHT_1_DIR = (a,
  max(b - a, 0), 1 - e, e)` from three vision values. Not used natively.
* `LIGHTING_LOOKUP_SCALE`, `ENVMAP_PARMS`, `OUTDOOR_FEATHER_PARMS` and the
  sky intensity (`r_hdrSkyIntensity`) were not found in this pass: no fixed
  writer in either the array or the per-view block's known fillers.
  (Fifth pass: `LIGHTING_LOOKUP_SCALE` found; the sky keys are dropped by
  the vision parser; the other two have no reader.)

## Sky, stages, probes, model lighting and sun shadows (read, 30 Oct, fifth pass)

Method as before, plus one finding that unlocked the rest: the per-view
block sits at a fixed offset in the view's info, and the functions that
fill it are handed either the view info or the block itself, so a
constant's writer is found by its offset from either base (slot `i` at
`16 i` in the block). What each shader does with a constant was read from
the technique dumps (`fastfile_h1` example `h1_tech` on
`techsets_mp_shipment.ff` and `code_post_gfx_mp.ff`) and the shaders
translated by `dxbc_sm5` (`dxbc2wgsl`).

### Who reads what (read, Shipment's techset zone, 11,758 techniques)

| constant | techniques reading it |
|---|---|
| `ENVMAP_PARMS`, `OUTDOOR_FEATHER_PARMS` | **none** (none in `code_post_gfx_mp` either) |
| `LIGHTING_LOOKUP_SCALE`, `MODEL_LIGHTING` | 1,168 (light-grid lit models) |
| `SHADOWMAP_*`, `SUN_SHADOWMAP_PIXEL_ADJUST_ARRAY_*`, `SUN_SHADOWMAP_CASCADE_V_CLAMP` | 1,797 (the `_sun` shadowed variants) |
| `TRANSPOSE_SHADOW_LOOKUP_MATRIX` | 6,735 |
| `SUN_SHADOWMAP_NEAR_FAR_PLANE` | 26 |
| `HDR_STAGE_PARMS` | 29 (sky, effects, ocean) |

So `ENVMAP_PARMS` and `OUTDOOR_FEATHER_PARMS` need no fill in MWR
multiplayer: nothing reads them (no writer for either was found).

### Stages and `HDR_STAGE_PARMS` (read; `render_h1::stage`)

* The clip map holds **stages** (`clipMap_t::stages`, 32 bytes each:
  name, origin, trigger index, sun primary light index, radiometric
  scale) and a trigger set for them (`stageTrigger`). Each view the engine
  picks the stage: stages 1.. in order, the first whose trigger model
  holds the eye **taken relative to the stage's origin**; a hull holds a
  point strictly inside its box on every axis and strictly inside each of
  its slabs (`|dir . p - mid| < half`). None found: stage 0 (inferred: the
  decompile loses the return value; stage 0 is the whole map).
* The view takes the stage's radiometric scale, and the view setup writes
  `HDR_STAGE_PARMS = (1000 x scale, 16, 0, 0)`.
* Every staged retail map has one stage with scale **0.001** (Shipment,
  Killhouse, Crash; `h1_map` prints them now), so `.x = 1`. The 0.001 is
  also the exposure's view scalar `b` (`E(ev) = 0.18 2^(2.84 - ev) / b`):
  the same number; that `b` is the stage scale is inferred (one reader
  returns `1 / stage 0's scale`, its callers not followed).
* Ours was (1, 1, 1, 1): `.x` is the same on retail maps, `.y` was 1 (one
  additive colour-tint effect shader scales by it a second time) and `.z`,
  `.w` were 1 (no reader uses them).

**Default now exact** (`BO2MP_H1_STAGE=fitted` brings back (1, 1, 1, 1));
the host sets it per frame from the eye's stage.

### The sky (read)

* The sky technique (`sky_cso_hss`, `sky.hlsl` / `sky_cso_hss.hlsl`) reads
  one code constant: the pixel shader is `min((cube(dir) x k.x + k.y) x
  HDR_STAGE_PARMS.x, 65504)` with `k` its material constant (alpha 1).
* **`r_hdrSkyIntensity` and `r_hdrSkyColorTint` do nothing in MWR**: the
  vision parser has its 39 keys, then a list of 13 names it recognises and
  drops (the `r_glow*` keys, `r_glowBloomPinch`, the two sky keys); a
  matched name in that list is skipped with its value. So the sky's
  brightness is its material constant times the stage scale, and nothing
  in the vision tints it.
* Ours: the native renderer draws the sky with this technique already, so
  with the exact `HDR_STAGE_PARMS` it is the game's sky (`stage::sky_color`
  is the formula, unit-tested).

### Reflection probes (read from the shaders and the data; binding inferred)

* Lit techniques sample one `texture_cube` (`t1`/`s1`, the custom sampler
  flag) at `lod = 6 - 6 x gloss`; no code constant enters (no
  `ENVMAP_PARMS`, no cube array, no probe blend). A drawn surface's probe
  is its `reflectionProbeIndex` (world surfaces and static models carry
  one; the draw-surface sort key has the field). Our renderer binds
  exactly that per draw (`scene.rs`, `world.rs`), so nothing changed. The
  engine's binder itself was not traced (inferred from the sort key and
  the shaders).

### Model lighting atlas (read; `render_h1::model_lighting`)

* `LIGHTING_LOOKUP_SCALE` is written into the per-view block as `(1.5 /
  1024, 1.5 x (1 / atlas height), 0.375, 0)` (immediates 1.5/1024 and
  0.375 = 1.5/4; the height factor from the atlas setup).
* The atlas is **1024 texels wide**, 4 deep; its height is a power of two
  from the engine's tile budget (the smallest power of two holding a
  dvar's page count of 4096 tiles plus one spare page). A tile's
  `BASE_LIGHTING_COORDS` is `((((h-1) mod 256) x 4 + 2) / 1024, (floor((h-1)
  / 256) x 4 + 2) / height, 0.5, 1)` (`h` the lighting handle; also the
  fourth pass).
* The upload of a tile writes its 64 texels x fastest, then y, then z
  (row pitch, then slice pitch), each texel three halves from the tile's
  56 colours and the record's visibility as alpha. The colour order is
  **exactly ours** (`SHELL_TO_TEXEL`, compared texel by texel). The 56
  colours come from the light grid at the model's lighting origin (or a
  fixed lookup when the grid has none); our source (the placement's grid
  colour, else a grid sample) is inferred to match.
* Changed: our atlas was 256 wide (Modern Warfare 2's); it is now MWR's
  1024 (`BO2MP_H1_ATLAS=mw2` brings back 256). The height stays "the power
  of two that holds the map's tiles": it enters the shaders only as `1 /
  height` in both constants, so any height that holds the tiles draws the
  same. Unit tests check the coordinates against the engine's integer
  maths and the lookup scale against its immediates.

### Sun shadows (read; `render_h1::sun_shadow`, pass off by default)

The native renderer draws `LIT_DIR` (the lightmaps' baked sun visibility),
as the engine does when a view has no sun shadow. `BO2MP_H1_SUN_SHADOW=1`
turns on the sun shadow map pass below (`host.rs`): the depth atlas drawn
with each caster's `build_shadowmap*` technique per cascade, bound as
`SHADOWMAP_SUN`, and the `LIT_DIR_SHADOW` slot first. Not seen on a GPU.

* **When (read; `sun_shadow::{Gate, view_has_sun_shadow, cell_bit}`):**
  only the main view, and only when the view's shadow flag allows it. The
  flag comes from the client's view definition: 0 auto, 1 forced on (kept
  only while the sun is on, else it becomes 0), 2 forced off. In auto the
  view draws it when the sun is on and some cell is both visible to the
  view and set in the world's per-cell "has sun-lit surfaces" bits
  (`GfxWorld::cellHasSunLitSurfsBits`, filled at map load, not in the
  zone; bit `i` is word `i / 32`, most significant bit first). Otherwise
  every cascade's shadow view (and its cached views) is cleared and the
  lit shaders fall back to the baked visibility. Ours: we keep no cells or
  portal visibility, so the host uses one stand-in cell, always visible,
  sun-lit when any surface of the map has a `LIT_DIR_SHADOW` technique;
  the sun is "on" when its colour is not black. `BO2MP_H1_SUN_SHADOW_GATE=
  auto|force|off` (default auto). Gated off, the frame draws `LIT_DIR`.
* **Light set keys** (read): the light set's key table names them by
  index: `sunshadowscale` (2), `usedsuncascadecount` (6),
  `sunsamplesizenear` (7, the first cascade's texel size in world units at
  full scale), `sunfilterradius` (9, the filter radius in texels), between
  them `spotlimit` (4) and `qualityspotshadow` (5), each float with a
  `_time` key. **Defaults (read):** every light set row is first filled
  from a block of override dvars (an override switch, off by default,
  makes the dvars win every frame) and the keys present then overwrite it,
  so a missing key keeps the dvar's default: `sunshadowscale` 1 (range
  0.25..1), `usedsuncascadecount` **2** (range 1..3), `sunsamplesizenear`
  **0.25** (range 1/64..32), `sunfilterradius` **4** (range 0..64; was
  1.5, inferred), `spotfilterradius` 4, `spotlimit` 4, `qualityspotshadow`
  on.
* **Map size (read; `sun_shadow::Quality`):** the shadow quality dvar
  (enum `Auto`, `Normal`, `High`, `Extra`, default `Auto`) sizes the maps
  when the renderer starts: `Auto` and `Normal` 1024 (the small map 512),
  `High` 2048 (1024), `Extra` 3072 (1536); with shadows off, 16 (8). The
  sun map (`$shadowmap_large`) is that wide and 3 bands high, the small one
  (`$shadowmap_small`) 4 bands; the translucent maps
  (`$trans_shadowmap_large/small`) are the same sizes in a colour format.
  Ours: 1024 by default (was 2048, inferred); `BO2MP_H1_SUN_SHADOW_QUALITY=
  normal|high|extra`, or `BO2MP_H1_SUN_SHADOW_RES` for any size. Nothing in
  the exe moves `Auto` up (the only other reader lowers the quality when
  the targets cannot be made); a player's config may set it.
* **Resolution (read):** `f = clamp(sunshadowscale, 0.25, 1)` (a dvar
  forces 1; off by default); used size `U = 2 ceil(res f / 2)`, border
  `(res - U) / 2` texels on each side; 3 rows. The 4-row case is not the
  cached cascades: with `qualityspotshadow` on and the eye inside one of
  the primary lights' volumes, the sun shadow moves to the small map, 4
  bands, `f` doubled (at most 1) (read; not done here).
* **Sun axes (read):** `s` the sun light's direction (the primary light's,
  toward the sun); `up` = world z when `|s.z| <= 0.9`, else world x;
  `x = normalize(up x s)`, `y = s x x` (`sun_shadow::axes`).
* **Texel sizes (read):** cascade 0 `t0 = sample size / f`, each next
  cascade x4 (`t_k = 4^k t0`).
* **Frustum fit (read; `sun_shadow::{fit, fit_axis}`):** the four corner
  rays of the view (unit vectors through `forward +- tan_x right +- tan_y
  up`) are projected on `x` and `y`; per axis the extent `[lo, hi]`
  includes 0 (the eye). Texels per unit of ray length `k = (U - 2) / (hi -
  lo)`; the eye's texel `a = ceil(0.5 - k lo) + 0.5`, and with `e =
  floor(U - 0.5 - k hi)`, when `a != e - 0.5` it is refitted: if `hi <=
  -lo`, `a -= 1` and `k = (1 - a) / lo`, else `a = e + 0.5` and `k = (U - 1
  - a) / hi`. So the eye is not centred: it sits on a texel centre placed
  so that the whole view wedge is inside the band. The ray length the
  first cascade holds is `min(k_x, k_y) t0`, and its **view depth (the
  split distance)** `d0 = min_i(forward . ray_i) x min(k_x, k_y) t0`;
  cascade `k` reaches `4^k d0` (the engine's culling planes for each
  cascade sit at those view depths).
* **Centres (read):** each cascade's centre in sun space is the eye's
  `(x . eye, y . eye)` snapped to its texels, `(floor(c / t_k) + 0.5) t_k`
  (two fixed-centre dvars, off by default, replace the eye with a point).
* **Projection (read; `Setup::world_projection`):** for cascade `k`,
  `x_clip = 2 (x . p - c_x) / (res t_k) + 2 (border + a_x) / res - 1`, the
  same for `y`, and `z = (s . p + far) / (far - near)`, w 1. The depth is
  **reversed: 1 on the sun side**. `(near, far)` are the min and max of
  `-s . p` over the world's **shadow bounds** (`GfxWorld::shadowBounds`,
  the mid point and half size; a debug dvar can replace them), the same
  for every cascade: these are the real caster bounds.
* **Caster states (read):** the `build_shadowmap*` states carry depth test
  3 (near wins: greater-equal in the reversed depth) and polygon offset 3.
  The engine's rasteriser state for offset 3 takes its depth bias and
  slope bias from two dvars (defaults 128 and 4.0) and a clamp dvar
  (default 0), **negated**: depth bias -128, slope -4, clamp 0 (decal
  offsets 1 and 2: `-mode x` dvars whose defaults are -1, i.e. +1, +1 per
  mode, which is how the main view's depth is known to be reversed too).
  The comparison sampler: compare **greater**, linear, border addressing
  with a zero border (`gpu::comparison`; clamp to edge when the device has
  no border support).
* **Shadow lookup matrix** (`TRANSPOSE_SHADOW_LOOKUP_MATRIX`; read): from
  the first cascade's view projection, `u = (x + w) / 2`, `v = (w - y) / (2
  rows)`, `z` as is, and the last column the **view-depth plane**: the
  view's forward axis over `d0` (minus the eye's distance along it), so
  `.w` is the view depth in units of the first cascade's
  (`sun_shadow::lookup_matrix`). The lit shaders compare `z` directly.
* **Cascade constants** (copied into the per-view block from the source
  the setup fills; read): `SHADOWMAP_SWITCH_PARTITION_ARRAY_0/1`, for
  cascade `i = 1, 2` with `r = 1 / 4^i` and `E = border + a`:
  `x = ((1 - r) E_x + (c0_x - ci_x) / t_i) / res`, `y = ((i + 1 - r) - ((1 -
  r) E_y + (c0_y - ci_y) / t_i) / res) / rows`, `z` "not cached" (1), `w =
  r`; `SHADOWMAP_DISTANCE_BIAS = (8, 8, 8)` for the used cascades,
  `SHADOWMAP_SCALE = (16 / f, 16 rows / f, -8 / f, -24 / f)`,
  `SHADOWMAP_PARTITION_UV_OFFSET = (0, 2 / rows, 0, 0)`,
  `SHADOWMAP_CASCADE_MASK = (1, 1, n >= 3, n >= 4)`,
  `SHADOWMAP_DISTANCE_BLEND = ((4^(n-1) d0)^2, 1 / (res t_(n-1) / 16)^2, 0,
  0)` (`n` the cascade count).
* **Per-view sun constants (read; `sun_shadow::{pixel_adjust, v_clamp,
  near_far_plane}`):** filled once a frame with the shadow setup and copied
  into code constants 269..273 with the view: with `R` the filter radius,
  `SUN_SHADOWMAP_PIXEL_ADJUST_ARRAY_0 = (R / res, R / (rows res), 0, 0)`,
  `_1` and `_2` the same times `t0 / t1` and `t0 / t2` (1/4, 1/16);
  `SUN_SHADOWMAP_CASCADE_V_CLAMP = (1 / rows - p0.v, 1 / rows + p1.v, 2 /
  rows - p1.v, 0)`; `SUN_SHADOWMAP_NEAR_FAR_PLANE = (near, far, far -
  near, 1 / (far - near))` in world units along the sun.
* **Per-cascade caster culling (read; `Setup::caster_planes`,
  `sun_shadow::{face_sun, side_planes, silhouette, box_inside}`):** each
  frame the engine builds up to 10 planes per cascade and culls the
  scene's casters (world surfaces and models) against them. With `r_a`,
  `r_b` adjacent corner rays (faces (3,0), (0,1), (1,2), (2,3)), the 2D
  cross of the rays projected on the shadow axes is `(r_a x r_b) . s`; a
  face counts as leaning toward the sun past a tolerance of 0.001 (the
  engine's sign is the opposite of ours, which orients the normal inward).
  The planes:
  1. When the faces lean both ways, the view wedge has a silhouette seen
     from the sun: the two corner rays where the lean switches. Through
     each, a plane containing the sun direction, its normal the ray's
     projected unit direction `d` turned a quarter into the wedge, pushed
     out by `(|d.x| + |d.y|) t0 / 2` for cascade 0 and `(1 + 4^k)` times
     that for cascade `k` (the cascade-0 plane copied, then the step added).
  2. When there is no silhouette and every face leans toward the sun (the
     view looks into the sun): the near plane at the previous cascade's
     view depth (`4^(k-1) d0`; cascade 0: the view's near plane).
  3. The cascade's band, its used part: `x . p` in `[c - e t, c + (U - e)
     t]` along both axes (`e` the eye's texel in the used part). The engine
     leaves out a side the silhouette planes already cover; ours keeps all
     four (the same casters pass).
  4. The view's side faces that lean toward the sun (through the eye),
     added to every cascade: a caster outside such a face throws its
     shadow away from the view.
  A caster is drawn when its bounds are inside every plane. Ours: each
  draw's world bounds from its vertices (`world::draw_bounds`, placed by
  its object), tested per cascade in `Renderer::record` (`Frame::cull`).
  On by default with the pass; `BO2MP_H1_SUN_SHADOW_CULL=0` draws every
  caster into every cascade. A non-default path (a debug switch) builds
  far/near planes by the view's angle to the sun, a silhouette of the
  wedge's 2D hull and per-cascade scissor rectangles; it is never taken
  (its switch is set every frame). The engine also narrows each cascade's
  drawn rectangle (a scissor) to the texels the view wedge reaches and,
  for the outer cascades, those the inner one does not cover (read, not
  done: it changes no texel the shaders sample).
* **Cached cascades (read; `sun_shadow::{CacheMode, cache_tiles,
  TileCache}`, `Setup::{world_texel_rect, cascade_tiles, tile_projection,
  tile_keys}`):** a dvar (enum "Disabled", "Enabled", "Enabled Force
  Cache"; **default "Disabled"**) turns them on, only with the small spot
  maps made and two other switches at their defaults. When on, each
  cascade's drawn rectangle is placed on a world-anchored texel grid (its
  first texel `floor(c / t + 1/2 - e)`), cut into tiles of half the sun
  map's width (`floor` division, the last index taken at the rectangle's
  end, so at most 3 x 3 tiles: the 9 "sun shadow cached" views a cascade
  has). Each tile has its own projection (`2 (a . p) / (T t) - 2 i - 1`
  along each axis, the cascades' reversed depth) and is kept in a pool
  keyed by the cascade, the sun direction, the tile index and its world
  size; a tile in the pool is reused (static casters drawn once), a new
  one takes the least recently used slot and is drawn; "Force" redraws
  every tile each frame. Ours: the tile maths and the pool are in
  `sun_shadow` and tested; with `BO2MP_H1_SUN_SHADOW_CACHE=1|force` the
  host keeps the pool's bookkeeping each frame (`H1_PROFILE` prints it),
  but still draws every caster into each cascade (the image the engine
  makes with the cache off, its default). Not done: drawing the tiles and
  copying them into the bands (a depth copy of part of a texture needs a
  shader blit in wgpu), and splitting static from dynamic casters.
* **Translucent casters (`TRANS_SHADOWMAP_SUN`):** read: the engine makes
  `$trans_shadowmap_large` as wide and as high as the sun depth map, in a
  colour format. Ours: an `Rgba8Unorm` map (format inferred) bound as
  `TRANS_SHADOWMAP_SUN`, cleared to white each frame (no tint; inferred),
  and per cascade the materials with a technique in slot 7 (inferred: the
  slot after the depth casters, as in IW's earlier engines) drawn into its
  band with their own blend, tested against the opaque casters' depth and
  not writing it (`world::Target::TransShadow`). Shipment's techsets fill
  slot 7 for 2 sets with no technique loaded, so nothing is drawn there
  yet and the map stays white (the same as the stand-in before).
* **Not read / not done:** the per-cascade scissor rectangles, the
  4-band small-map case, drawing the cached tiles, the translucent map's
  exact format and clear value, the dvar names (hashed).

### Checks

`cargo test -p render_h1 --lib` (43 tests; the sun shadow's: the light
set defaults and map sizes, every point of each cascade's view slab and
points above it toward the sun kept by its culling planes for views level,
down, into and away from the sun, far boxes culled, the silhouette's
normals into the wedge, the near plane when looking into the sun, the band
planes against the projection, the gate and the cell bit layout, the tile
ranges, the tile projection on the cascade's grid and the tile pool's
reuse, forcing and eviction). Headless on Shipment: `cargo run --release
-p render_h1 --example h1_slots -- --cull mp_shipment.ff
techsets_mp_shipment.ff` (1248 casters; from the intermission point the
two cascades keep 2 to 6 and 3 to 222 of them by heading). Before:
`cargo test -p render_h1 --lib` (36 tests): stage selection (relative
origin, strict hull and slab tests, stage 0 fallback), `HDR_STAGE_PARMS`
and the sky formula, the atlas coordinates against the engine's integer
maths, the lookup scale against its immediates, the fill order, the
shadow block masks and their slots 196..202, scale and offset, the lookup
matrix and the snap, the sun axes, the depth range over the shadow
bounds (reversed, 1 at the sun), the per-axis fit (eye on a texel
centre, rays inside the band), the frustum's corner rays inside every
cascade out to `4^k d0`, the switch partitions against each cascade's
own projection, pixel adjust, V clamp and blend. `h1_map` on Shipment, Killhouse and Crash prints one
stage each, scale 0.001. No GPU here: not seen on screen.

**First GPU check (31 Oct, owner's RX 9060 XT, `h1_native` offline, four
Shipment views with the pass off and on):** the pass runs with no
validation error and draws 91 to 437 casters per view (818 to 1,164
culled), but no prop's shadow shows on the ground (lamp posts, the tent)
and open grass goes darker and speckled (looks like self-shadowing). The
container yard's containers are not drawn at all in these renders, so
the main casters are missing. Not passed; the pass stays off by default.
The same renders found the split-sum lookup pictures unread (R16G16_FLOAT;
fixed, see README 31 Oct), which had left every blended surface out.

## Map logic (read, 30 Oct; `fastfile_h1::spawn`, `assets::lane::h1_map`)

What the exe does with a map's entities at map start, and what we do with
them now. Checked headless on the staged Shipment, Killhouse and Crash
(retail) and the usermap mp_courtyard_ss:
`H1_MAP_ZONES=a.ff:b.ff cargo test -p fastfile_h1 --release --test
h1_map_logic -- --ignored --nocapture`.

### The map-load spawn (read)

The level start reads the entity text block by block (`{`, quoted key/value
pairs, `}`; keys are the canonical script string numbers on a retail map).
The first block must be `worldspawn`, else the load stops with an error.
From it the game reads two music keys (written to two config strings as
`n\<name>\t\0\v\1000\e\1`), `northyaw` (kept as its sine and cosine,
degrees to radians; 1 and 0 when absent) and its `spawnflags`. Then each
block, by `classname`:

1. `node_*` (any case): a path node (no entity);
2. `info_vehicle_node`, `info_vehicle_node_*`: a vehicle path node;
3. `actor_*`: nothing (the multiplayer exe has no actors);
4. `weapon_<name>` where `<name>` is a loaded weapon: a weapon pickup;
5. the spawnable table (`info_notnull`, `trigger_radius`, `script_model`,
   `script_origin`, `script_vehicle_collmap`), then the map-only table
   (`trigger_use_touch`, `trigger_use`, `trigger_multiple`, `trigger_disk`,
   `trigger_hurt`, `trigger_once`, `trigger_damage`, `light`, `light_spot`,
   `light_omni`, `misc_mg42`, `misc_turret`, `script_brushmodel`,
   `script_struct`, `script_vehicle`). A class matches a table name
   case-insensitively when it is the name or the name followed by `_`
   (`script_struct_heli` is a `script_struct`, `light_spot` meets `light`
   first). `script_struct` makes no entity; the rest spawn an entity and run
   the class's spawn function. The entity keeps the map's class name, and a
   second class field takes the table's name;
6. **anything else is an entity too**, with no spawn function: not freed
   (Black Ops II and III free it). There is **no spawn point branch**: the
   spawn points are `MapEnts`' spawn list; a usermap's copies in the text
   are plain entities.

A reserved entity number is used when the map names one (a list kept beside
the map), else the next free entity.

Before the spawn function, every key goes through the field table:
`code_classname`, `classname`, `origin`, `model`, `spawnflags`, `target`,
`targetname`, `health`, `dmg`, `angles`, `script_linkname` and three more
(an integer, a vector and a fourth kind with its own parser; not named). `model` `*N` is a brush
model, `?N` a trigger model, any other name an `XModel` (set only for map
entities). Unknown keys stay on the entity for the scripts.

**Key numbers** read from the exe (the spawn functions and field table ask
for them by number): `dmg` 293, `health` 486, `northyaw` 714, `wait` 1293,
`cursorhint` 229, `hintstring` 497, `radius` 849 and `height` 488 (as
before). By values and counts against mp_courtyard_ss, which names them:
`script_delay` 914, `script_bombmode_original` 31072, `script_exploder`
31167. These are in `map::H1_KEYS`. **Before this, a retail map's kill
volume read `"h1_293" "10000"`: its `dmg` was unknown, and our rules hurt 5
a touch (100 a second) instead of 10000.**

### Spawn functions (read)

| Class | What it sets |
|---|---|
| `trigger_multiple` | touch handler; contents: player unless spawnflags 8, AI teams by spawnflags 1/2/4; `wait` ≤ 0 makes it fire once |
| `trigger_once` | as `trigger_multiple`, always once |
| `trigger_radius` | from the origin up `height`, `radius` round it (bounds: middle (0, 0, h/2), half (r, r, h/2)); error when `radius` or `height` is missing |
| `trigger_disk` | `radius` + 64 round the origin, from 100000 below to 100000 above |
| `trigger_hurt` | `dmg` 5 when 0; on unless spawnflags 1 (then only `use` toggles it on) |
| `trigger_damage` | `health` 32000, set again on each pain or death unless a key (number 9: `accumulate` by its place, inferred) is set; damage handler; `wait` ≤ 0 once |
| `trigger_use`, `trigger_use_touch` | use handler, `cursorhint` (default `HINT_INHERIT`), `hintstring` |
| `script_brushmodel` | its `*N` brush model's bounds and contents; spawnflags 1 adds two entity flags (not followed) |
| `script_model` | a model entity, solid by its model |
| `script_origin` | a point, no contents |
| `light*` | the primary light it names (key 1), its radius as bounds |
| `info_notnull` | its angles only |

Every brush/trigger-model trigger takes its bounds and contents from its
`?N` model. **The hurt touch** (read): when the toucher takes damage and the
trigger's next-hurt time has come, it queues the `trigger` notify, then
damages the toucher with the trigger as inflictor and attacker, `dmg`,
no damage flags (spawnflags 8 changes nothing here), `MOD_TRIGGER_HURT`;
if the toucher's health changed, the next hurt is 50 ms later (1000 ms with
spawnflags 16); spawnflags 32 turns it off after that hurt. `use` toggles a
hurt trigger on and off. **The touch of `trigger_multiple`** queues the
`trigger` notify with the toucher (up to 256 a frame, then sent at once).
The constants are in `fastfile_h1::spawn` (`hurt_damage`,
`hurt_interval_ms`, `radius_shape`, `disk_shape`, `fires_once`); Black Ops
II's rules (`sim::t6::hurt`) already hurt this way, with `dmg` now read.

### On the staged maps (`h1_map_logic`)

| | Shipment | Killhouse | Crash | courtyard_ss (usermap) |
|---|---|---|---|---|
| spawn list | 366 | 357 | 434 | 191 |
| TDM / FFA | 15 + 18 starts + 18 overflow / 15 | 17 + 35 starts / 17 | 19 + 22 + 14 / 19 | 13 + 20 / 19 |
| other types | dom, ctf, dd, hp, sab, ball, sd | dom, ctf, dd, hp, sab, sd | dom, ctf, dd, hp, sab, ball, sd | dom, ctf, dd, sab, sd |
| entities with a spawn function | 170 | 268 | 220 | 149 |
| plain entities (the spawn points in the text, ...) | 368 | 359 | 436 | 210 |
| structs / path nodes | 90 / 459 | 0 / 357 | 14 / 1126 | 22 (`script_struct_heli`) / 199 |
| kill volumes (`trigger_hurt`, `dmg` 10000) | 1: z 0..64 over the map's whole span (the ground is at 192) | none | 1: z 0..32 under the map | 3: one under the map (z -192..-128), two small pits |
| triggers, each with a volume a player box at its middle touches | 67 | 47 | 39 | 33 |
| brush model entities (all `*N` in range) | 55 | 121 | 66 | 20 |
| dynamic entities (clutter / destructible / other) | 2007 / 0 / 0 | 54 / 0 / 1 | 630 / 15 / 101 | 25 / 25 / 0 |

Each game type keeps its own classes (`mp_<type>_spawn`, `_allies_start` /
`_axis_start` or `_attacker_start` / `_defender_start`, and on Shipment and
Crash `_overflow` starts); CTF and Demolition have one team-neutral kind
(`mp_ctf_spawn`, `mp_dd_spawn`). There is no spawn selection in the
map-load code: picking a point is the game type scripts' (inferred: the
exe's only `_spawn` strings are loading screen pictures).

### The map's own scripts (not staged)

The map zones hold no map script: Shipment's two compiled scripts are the
shared `common_scripts` utility (vision/light set helpers, flags, autosave),
Crash adds `vehicle_scripts/_80s_sedan1`. `maps/mp/<map>.gsc`, `_load` and
the game type scripts are in zones not staged here (`common_mp`,
`patch_<map>`, ...). What they do at start is therefore **inferred** from
the entities' names and Call of Duty 4's multiplayer scripts, which MWR
remasters:

* `oldschool_pickup` (weapon pickups and perk models, with what they
  target): deleted unless the Old School mode runs;
* `delete_on_load` (Shipment: 15 triggers and 29 traversal-preview structs
  with dog and soldier models; Crash: 3): deleted;
* `exploder` script models (7 a map: the destroyed bomb objectives standing
  inside the whole ones, `script_exploder` ids): hidden and not solid until
  their exploder fires;
* `out_of_bounds` (1, seven hulls over the map's north edge, z 272..340) and
  `out_of_bounds_at_rest` (17, thin slabs over the container tops) on
  Shipment are `trigger_multiple`s: the exe only notifies `trigger`; what
  the scripts do with them (likely: equipment and killstreaks that come to
  rest there are destroyed) is not known. Left to our rules' scripts;
* `patchclip_*` brush models are templates parked under the map (z -320);
  `hp_zone_*` (Hardpoint outlines) and the bomb sites go with their mode
  (`script_gameobjectname`, Black Ops II's `_gameobjects` does this);
* movers named by the prefabs (`me_fanceil_spin`, `rotate`,
  `hanging_object` on Crash, `com_wall_fan_blade_rotate_custom`,
  `windmill_top`, `interactive_tv` on Killhouse) are driven by those
  scripts: they stand still here.

### What we do now (default; `BO2MP_H1_MAP_LOGIC=0` the old way)

The H1 lane (`lane::h1_map`) puts the entity text through
`fastfile_h1::spawn::as_spawned` before Black Ops II's rules get it: vehicle
nodes and actors left out, a class matched to a table class by the exe's
rule given that class's name (the map's kept as `h1_classname`), and the
script deletions above (Old School pickups and their targets,
`delete_on_load`, `exploder` models). The load report says what it did
(`h1 map logic: ...`). The keys are named whatever the switch says (a
reading fix): the kill volumes now kill at the first touch. Black Ops II's
rules (`asset_world::foreign_t6_entities`) now take MWR's team-neutral
`mp_ctf_spawn` for both CTF teams and `mp_dd_spawn` (renamed
`mp_dem_spawn`) for both Demolition sides before falling back on Team
Deathmatch's points.

Not done: the movers and the exploders' reveal (no scripts), the
`_overflow` starts (Black Ops II's start logic does not read them),
`trigger_hurt` spawnflags 1 and 32 (`sim::t6::hurt` does not honour START_OFF
on any map), dynamic entities (clutter and destructibles are `MapEnts`
records the lane does not place yet).

## Effects (read, 30 Oct; `fx_iw4::h1`, `render_fx::h1_rules`)

How MWR runs a particle, from the effect system's own functions (found
through the renderer's once-per-frame warning table: the "FX_ELEM_LIMIT"
warning is raised only by the element spawn, "FX_EFFECT_LIMIT" and the spot
light limit by the effect spawn; from there the spawn, the per-type draw
table and the channel evaluators). **Read** = the decompile shows it;
**inferred** is marked.

### Records (read)

Every field the runtime reaches matches `fastfile_h1`'s layout (x64-zt's,
the same as Advanced Warfare's but for the three decal materials): the zone
loader for the element array copies 320-byte elements with velocity samples
`(velIntervalCount + 1) x 96` at +0xc0 and visual samples
`(visStateIntervalCount + 1) x 112` at +0xc8; the runtime reads `elemType`
+0xb4, `elemLitType` +0xb5, `visualCount` +0xb6, the interval counts
+0xb7/+0xb8, the visuals +0xd0, the three child effects +0xf0/+0xf8/+0x100,
`lightingFrac` +0x121, **`randomSeed` +0x128** (used, see below),
`hdrLightingFrac` +0x130 and `shadowDensityScale` +0x134. Element types,
from the spawn routine and the 14-entry per-type draw table: 0 billboard, 1
oriented, 2 rotated (its own draw: pulled toward the eye, colour divided by
its light), 3 tail, 4 line, 5 trail (no draw entry; trails are their own
system), 6 flare-like quad, 7 particle simulation, 8 cloud, 9 spark cloud
(its own 0xa10 history block), 10 spark fountain, 11 model, 12 omni light,
13 spot light (two at most), 14 sound (only spawned within 200 ms of now),
15 decal, 16 runner, 17 vector field.

### Random numbers (read)

* One table of **16391 floats** in 0..1 (`crates/fx_iw4/data/h1_fx_random.bin`,
  as the Black Ops III table was kept); seeds are taken **mod 16361** and a
  channel is added (IW4: 508 floats, mod 479). Counts and times use the
  **low 16 bits of the float** as the random (`base + (r16 x (amp + 1)) >>
  16`), the visual channels the float.
* Channels: velocity 0..2, angular velocity 3..5, spawn origin 6..8, offset
  yaw / height / radius 9..11, spawn angles 12..14, gravity 15, reflection
  16, life 17, delay 18, one-shot count 19, emit distance 20, visual 21,
  atlas 22, colour 23, initial rotation 24, rotation 25, size 26 / 27,
  **scale 28, pivot 29 / 30** (new against IW4).
* Element seed: `(sequence x 10111 + effect seed + element def randomSeed +
  msec) mod 16361`, in unsigned arithmetic. The delay is drawn with the seed
  of `msec + delay base`, then the seed is taken again at the begin time.
  Effect seed: `rand() x 16361 >> 15` (IW4's shape, its 479 replaced).

### Spawning (read)

* One-shot elements: `count = base + (r16(effect seed, 19) x (amp + 1)) >>
  16` (the random only with an amplitude), all at the effect's start,
  sequence = index.
* Looping elements: the first at the start (sequence 0), then one each
  `interval` msec, `n = (from - start) / interval + 1`, while `msec <= to`
  and **`n < count`** (`spawn.looping.count`; `0x7fffffff` = no end:
  Shipment 36 of 54 looping elements, Crash 92 of 182, Killhouse all 72).
  Their frame is lerped between the last and this update's (origin and
  quaternion). When an update spans more than 128 ms, spawns that would
  already be dead are skipped.
* Life: `base + (r16(seed, 17) x (amp + 1)) >> 16`. An element with no child
  effects whose life has already ended is not spawned. Inherited velocity:
  `((flags2 >> 9) & 15) / 15` of the effect's own motion over the last update.
* Spawn origin, offsets (sphere / cylinder round the forward axis), run
  modes (`flags & 0x1c0`: 0 world, 0x40 relative to the spawn, 0x80 to the
  effect now, 0xc0 to the offset, 0x100 to the view) and the velocity graph
  (local `flags & 0x1000000`, world `0x2000000`, trapezoids within each
  interval) are IW4's (`fx_iw4`); gravity `(base + r x amp) x 800`
  integrated per update for elements that collide or inherit velocity
  (exact as `g t^2 / 2` for a constant g), analytic otherwise.

### Visual state (read)

* Interval: `i = min(floor(n t), n - 1)`, `frac = n t - i`; at or past the
  end of its life an element is drawn at `t = 1` and its age held at 0.99 of
  the life.
* **Colour and emissive** (rgba, rgb; floats): `lerp(base, second half,
  r23)` in each of the two samples, then `(1 - frac)` and `frac`. The same
  colour random drives both.
* Size (26, and 27 when `flags & 0x10000000`, else the same), scale (28),
  pivot (29, 30): `base + r x second half`.
* Rotation: `initial (24) + (delta_i (f - f^2 / 2) + total_i + delta_i+1 f^2
  / 2) x life msec` with r25 (IW4's).
* Distance fade: `fadeIn` (start, length) 1 -> 0, `fadeOut` 0 -> 1, a zero
  length leaves its side at 1, the smaller counts; alpha x fade (and rgb x
  fade for materials with the "fade colour" bit). A particle is skipped when
  its fade, or its colour alpha times the fade, is at or below a threshold
  dvar / 255 (**default 0**).
* Visual: `visuals[(r16(seed, 21) x count) >> 16]` when there are several.

### Lighting of sprites (read)

A renderer bool dvar (**default on**; matched as the HDR effects switch by
use) picks the rule:

* on (the game's default): lit type 0 -> `rgb x emissive`; lit types 1..3 ->
  `rgb x emissive + hdrLightingFrac x light x rgb` (the second term only
  when `hdrLightingFrac > 0`), where light is the particle's ambient (plus
  the sun when the element's flag 0x100000 is set) sampled from the light
  grid, and for type 3 its Rec. 709 luminance (0.2126, 0.7152, 0.0722);
  lit type 4 and up keep the sampled colour.
* off: lit types 1..3 `rgb x (1 + (2 light - 1) x lightingFrac / 255)`, type
  0 unchanged.

Staged zones (`h1_fx_census`): emissive scales reach 450 (Shipment), 238
(Crash), 250 (Killhouse); `hdrLightingFrac` 0.5..100. Crash: 148 of 349
elements lit (types 1..3), Shipment 55 of 80, Killhouse 60 of 97.

### What changed in ours

Before: an MWR element was converted to IW4 bytes (`fx_catalog_s1`): colour
times emissive squeezed to 0..1, IW4's random table and seed, BO2's
lighting (`lightingFrac` byte only; `hdrLightingFrac` and the lit types
ignored), so lit smoke drew unlit at its full colour and the per-definition
seed was unused. Now (default; **`BO2MP_H1_FX_EXE=0` brings back the old
path**) each MWR element keeps its record fields and float samples
(`OwnedFxElemDef::h1`), and the sprite pass (`render_fx::h1_rules`) takes
its seed (from the runtime's sequence, effect seed and begin time, the
element's own `randomSeed`), colour, emissive, sizes, rotation, distance
fade and threshold, and lit colour (HDR rule) from `fx_iw4::h1`.

Ours, not the game's (inferred / kept):

* The pool, spawn times, lifetimes and positions are still the converted
  runtime's (IW4 rules, the same formulas, but IW4's table: a particle's
  life and delay differ from the game's one by one, not in distribution).
  `fx_iw4::h1` has the game's begin / life / count / looping schedule for a
  stateless evaluator (used by the check below).
* Light: our effect's single light sample (the map's packed light at the
  effect, 0..1) stands for the game's per-particle HDR light grid sample;
  missing -> white.
* Display: our sprites are drawn after MWR's tone map, so the HDR colour is
  fitted to 0..1 keeping its hue (`h1::display_rgb`), as the conversion did;
  the game tone-maps it with the scene.
* Not done: pivots in the quad (the sprite pass has none), the rotated
  type's own draw (still drawn oriented), lines, flares, clouds, particle
  simulations, lights, decals and exploders keep the converted path; the
  vertex builder (orientation flags, the z-up billboard of `flags2 & 2`) was
  not decompiled in full.

### Checks

* `cargo test -p fx_iw4 --lib h1::` (10 tests: the table's first values and
  spread, the integer random, seeds, delay / life channels, the looping
  schedule and count, sample setup, colour / emissive lerp and size range,
  rotation, fades and threshold, the lit colour by type and mode, record
  offsets); `cargo test -p render_fx --lib h1_rules` (3: emissive hue,
  lit colour, fades).
* Headless, staged zones (`cargo run -p fastfile_h1 --release --example
  h1_fx_check -- <zones>`): every sprite-like element of every effect played
  with three effect seeds, looping ones over 10 s, five points of each life:
  seeds, begin and life in range, all channels finite and inside their
  samples' envelope, fades in 0..1. Results in the next subsection.
* Not seen on screen (no GPU here).

## The palette light grid (read, 31 Oct; `fastfile_h1::light_grid_palette`)

Retail maps hold no light grid colour table (Shipment and Crash: row
data, entries and colours all empty). The grid is a tree over a palette:
Shipment has one tree (659 nodes, 48,140 leaves), 50,465 palette entries
and a 2 MB bitstream; range exponents 0, 4, 23.

**Entry (read).** A palette entry is at `bitstream + paletteEntryAddress[i]`
(a byte offset). It starts with a u16 of three 5-bit values `e + 8`
(red at bit 10, green at bit 5, blue at bit 0). For each channel `s = 2^e`
and its nine coefficients follow in 8 bits when `e <= exp8`, 12 bits when
`e <= exp12`, else 16 bits; the 12 and 16-bit runs start on an even byte.
The first coefficient is `s * v / 2^bits`, the other eight
`v * 2s / 2^bits - s` (8-bit: 1/256, 12-bit: 1/4096 with a 16-bit first
coefficient, 16-bit: 1/65536). The 12-bit run is 14 bytes: the first
value a u16, the high bytes of the other eight at bytes 2 to 9 and their
low nibbles in bytes 9 to 13. The engine's own packing writes the first
value's low nibble into byte 9 and then overwrites byte 9 with the last
value's high bits, so the first value reads the last one's top nibble;
we decode it the same way. The encoder picks the smallest exponent that
holds the largest coefficient magnitude.

**Model light from an entry (read).** The nine coefficients a channel are
an L2 harmonic with the basis constants folded in. The engine turns them
into a lighting tile's 56 shell colours: for each shell point of a 4x4x4
cube (x fastest, then y, then z, the eight inner points left out;
coordinates -1, -1/3, 1/3, 1, normalised to `d`) the colour is
`c0 + c1 dx + c2 dy + c3 dz + c4 dz dx + c5 dz dy + c6 dy dx +
c7 (3 dz^2 - 1) + c8 (dx^2 - dy^2)`, clamped to 0..65504 and stored as
halves. That is the 56-colour order the model lighting atlas already
uses. With no palette entries the light is `c0 = 1`, the rest 0. A model
lit from one entry (a static model's colour index, flag 0x40) is exactly
this; the grid's `missingGridColorIndex` is the fallback entry.

**Points in the grid (read, not used yet).** A point is looked up in
cells of 32 x 32 x 64 units from -131072 (clamped to +-131072), with
trilinear weights over the eight corners; the corners' entries come from
the tree, their harmonics are summed by weight (normalised by the weight
total) and then turned into shell colours the same way. Moving models
and effects use this path; the tree's bit layout (2-bit node modes, a
16-bit mask or a 4-bit index into a mask table) is only partly read.
Not translated yet: the tree walk, the sun visibility it returns, and
the optional contrast tweak (two dvars, off unless set).

**What we do now.** Static models with a colour index and no colour table
take their tile from the palette (`BO2MP_H1_LIGHTGRID=off` leaves the
palette unread, as before, when they drew black). Shipment: 4,946 lit
tiles (was 0).

## Ambient sound: the map's createfx sounds (3 Nov; code written, not heard in game)

A retail map's `maps/createfx/<map>_fx.gsc` (in `patch_<map>.ff`) and a usermap's own copy place sounds beside the effects: `createLoopSound()` (plays for good) and `createIntervalSound()` (plays now and then), each with `ent.v[ "soundalias" ]`, an origin and, for an interval sound, `delay_min` / `delay_max`.

* `fastfile_h1::createfx::Placement` keeps `soundalias`, `interval`, `delay_min`, `delay_max`; `CreateFx::loop_sounds()` and `interval_sounds()` list them. The interval defaults (1 and 3 seconds) are **INFERRED** from Call of Duty 4's script helper; the retail helper is compiled.
* `assets::lane::h1_fx::prepare` builds an `asset_audio::H1MapAmbience` (the placements, and the zones that hold the alias lists: the map's patch zone, the map, then `common_mp` and the other common zones; `techsets_*` and `code_post_gfx_mp` hold none) and the T6 lane hands it to the match's sound catalog (`asset_audio::h1_ambience_catalog`): each zone is walked and only the alias lists the placements name are kept, with every alias they chain to. A later zone only adds lists the earlier ones lack, so a patch zone wins.
* The placements carry the `h1:` alias prefix; `audio::ambient` splits it (`asset_game::alias_namespace`) for loops (`MapEmitter` at the placement, the alias's own distances and falloff curve) and for interval sounds (the existing random point sound player, one place each).
* An H1 alias with no falloff curve fades linearly from its near to its far distance: **INFERRED**, not read.
* Only loaded sounds of format 16-bit PCM or FLAC inside the zone play (as the guns'). A sound that lives in a sound pak (primed heads) is silent and counted in the log (`sound bank: Horizon MW ...: N primed heads only`; `h1 ambience: ... found in N zones`).
* `BO2MP_MAP_SOUND_H1=0` leaves a map silent as before.

Not done: `ambientPlay` (the map's bed) and ambient rooms / reverb, sound triggers, the exe's own falloff default. Not heard in game: no zones were reachable when this was written.

## Dynamic entities: clutter and destructibles (3 Nov)

READ: the clip map lists dynamic entities (`DynEntityDef`: base model, brush model, health, pose quaternion and origin); static model placements do not include them (Shipment 2,007, Crash 630). `fastfile_h1::map::dyn_ent_models` turns kinds 1 (clutter) and 2 (destructible) that have a model into placed models in their starting pose, and `render_h1::scene` draws them with the static models. INFERRED: quaternion is (x, y, z, w) with the static models' handedness; light is the nearest lit static model's within 256 units (the engine lights them from the grid at run time, not translated); no lightmap or vertex light. Not done: movement, breaking (health), collision from the model. `BO2MP_H1_DYNENTS=0` turns it off.

## Scriptables (6 Nov; `fastfile_h1::scriptable`, `scriptable_run`; not wired into a match)

A map's barrels, cars, bowls and lights are scriptables: `ScriptableDef` assets (parts, each with states; entering a
state runs its events) placed by `clipMap_t::scriptableMapEnts`. No script runs them. The zone walker now keeps them
(`Capture::scriptable_defs`, `Clip::scriptables`; census: `cargo run -p fastfile_h1 --release --example h1_scriptables
-- <zone.ff> [--full]`). READ from Crash: 18 instances of 6 defs; the sedan's root part goes 250 intact, 200 white
smoke, 100 black smoke, 300 and 400 burning (drain 12 per 200 ms, sound, fire effect), then destroyed (model swap,
explosion effect, `car_explode`, animation, explosion force 4000..5000 radius 375 damage 20..300); hood, doors, bumpers
and mirrors launch a physics piece, the glass breaks with an effect and sound.

How a hit is handled (READ from the exe, `docs/exe_notes/MWR_H1_SCRIPTABLES.md`; translated in `Machine::damage_part`
and `hit`): a hit becomes a kind mask (direct 1 / radial 2 / melee 4, plus attacker player 8 / vehicle 0x10 / other
0x20); a state takes it only if its damage flags share a bit with the mask in both groups; health is carried into the
next state when it runs out; a part can pass a share of its damage to the whole; burning parts hurt themselves each
interval. INFERRED: the melee means-of-death number, the explosion fall-off (linear), the first drain tick at once,
state 0's events running at start. Not read: the radial per-part scale, the events' exact curves (physics launch speed,
translate/rotate easing, light changes), the share the whole gives its children.

Not done: placing instances in the sim (damage taps on their model boxes), drawing the model swaps and hidden parts
(`render_h1` has no per-entity poses yet), the dynamic entities of kinds 4 and 5 (the bodies and loose parts of
scriptables, 2 and 99 on Crash) and kinds 1/2 breaking. The map scripts (movers, exploders) are not run: the h1 script
opcodes differ from Ghosts' (see the exe notes).

## Not read (follow-ups)

* **Film:** read in the Lighting section (fourth pass).
* **Sky, probes, light grid atlas:** read in the fifth pass (stages,
  `HDR_STAGE_PARMS`, sky technique, `LIGHTING_LOOKUP_SCALE`, atlas layout
  and fill). Still open: the light grid sampling into a tile's 56 colours
  (`r_lightGrid*` tweaks are in the second pass), `ENVMAP_PARMS` and
  `OUTDOOR_FEATHER_PARMS` writers (nothing reads them).
* **Sun shadows:** read and wired (cascade fit, constants, caster bias,
  sampler, shadow bounds, map size and light set defaults, per-cascade
  caster culling, the sun-lit gate, translucent casters, the cached tiles'
  maths) but off by default (`BO2MP_H1_SUN_SHADOW=1`): check it on a GPU
  (shadows on the ground under props on Shipment, no acne, cascades
  switching without seams, no caster missing at a cascade's edge with
  `BO2MP_H1_SUN_SHADOW_CULL=0` as the reference), then make it the
  default. Left: drawing the cached tiles (off by default in the game
  too), the scissor rectangles, the small-map case.
* Check the exact sun (`BO2MP_H1_LIGHT=exact`, no pi) against a HorizonMW
  picture and find what the fitted pi absorbs.
* **Fog** (`G_SetFog` to `ATMOS_FOG_PARMS_*`): not read; `look::fog` stays.
* **Bloom, glow, veil** (`VEIL_PARAMS`, `GLOW_*`): the chain's shape is read (3 Nov, see "Bloom chain" below); the constants' values, weights table, materials and dvars are not.
* The exposure pass's own shader (in the zones): to confirm how the
  exposure enters the filmic rows; the source of the view scalar `b`;
  the lightset-to-state copy for blends between light sets (`_time` keys);
  the perceptual, Reinhard and luminance curves.
* Shadows, SSAO (`ssao*` keys), MDAO, DOF and motion blur are not in the
  native renderer; the keys are in the light set section above. Inventory (3 Nov): SSAO is a compute pipeline (`cs_hemi_ao_*`), which `dxbc_sm5` cannot run until it has a compute stage; DOF is full-screen pixel passes; see `docs/exe_notes/MWR_H1_VEIL.md`. Bloom is `render_h1::veil` ("Bloom chain").
* Compare the exact filmic curve with a HorizonMW picture of a map that has a
  light set (needs `<map>_lightsets_hdr.csv` and the picture), then flip the
  default.

## Bloom chain: the veil (read 3 Nov; `render_h1::veil`, behind `BO2MP_H1_VEIL=1`, off by default)

READ in the exe (addresses in `docs/exe_notes/MWR_H1_VEIL.md`):

* Names: the bloom is the **veil**. Targets `$veil_mip1..8`; materials `veil_downsample_linear|log|karis`, `veil_bilinear`, `veil_bilinear_add`, `veil_gaussian_low|med|high`, `veil_gaussian_add_low|med|high`; the colour pass has `tonemap_veil_{linear,reinhard,hist,filmic}` with `_blur`, `_dark` and `_blur_dark` variants. The `r_glow*` keys are vision-file keys (the vision reader's table), not read by the veil.
* Dvars (registered by hash, names unknown; defaults and ranges read): upsample weights, six floats, defaults 0.25 0.75 1.5 2 2.5 3, range 0..10, divided by their sum; blur radius 8 (0..100); blur step scale per level, six floats, default 1 (0..8); mode (Upscale default, Downscale, Hybrid Fast, Hybrid Reference); gaussian size (Low, Med default, High); first-pass kind (Linear, Log default, Karis); plus two bools and two floats (0.087, 0.913; -10..10) whose use is not read.
* Radius to step: `y = height * radius / 480`, `x = y / aspect`, `aspect = height * pixel aspect / width`.
* Chain: first pass scene into mip 1 (`Down(0)`) with the kind's material, one plain linear downsample into mip 2, then `min(8, levels) - 2` further downsamples (8 mips in all, `$veil_mip1..8`), each a plain `veil_downsample_linear` pass or a separable blur (a pass along y into a temporary of the source's size, then along x into the destination, `VEIL_PARAMS = (step x / width / 3, step y / height / 3, 1, 1)` sized by that pass's own target); then the same count of upsamples from the smallest level, the last of which (`Up(1)`, target 100) is what the colour pass reads as its second texture, each blending a level with the one below (`veil_gaussian_add_*` takes a second source and `VEIL_PARAMS.zw` = the two weights; the plain `veil_bilinear[_add]` writes (w, w, w, w2) to another constant). Downsample steps are scaled by 1/2^3, upsample by 1/2^2; the modes choose which levels blur (Upscale: up only; Downscale: down only; Hybrid Fast: alternate levels; Hybrid Reference: both, step x 1/sqrt 2).
* INFERRED: the engine's level count (8, the cap), the light-set level floor (0), a square root where the exe calls an unnamed math routine, Karis as the third kind's name (its material says so), the targets' sizes (scene halved per level), and that the plain pass's second constant is `MATERIAL_COLOR` (address arithmetic).
* Not read: the dvar names, the two bools and two floats, the colour pass's use of the veil (which texture name it samples, the strength constants from `veilstrength` / `veilbackgroundstrength`), the extra pass the exe runs when a view flag is set (a blur of the first level by a radius), and the `GLOW_*` writers. 

## Device limits in game

The game's wgpu device is made in `bootstrap::plugins` (bevy `WgpuSettings`). wgpu's defaults allow 16 sampled textures and 16 samplers per stage; MWR's world shaders bind up to about 26 textures. With the defaults those pipelines fail validation and the draws are skipped. The game raises both (64 and 32). `T7_LIMITS=default` in `h1_native` reproduces the game's device (with the same raise); the offline tool otherwise requests the adapter's maximum, which hid the problem.

### Veil: what runs (3 Nov)

`veil::Chain` makes the targets (Rgba16Float, the scene halved per mip) and records the plan through `Post::record`, each pass reading its source as `RESOLVED_SCENE` and its second source as `FEEDBACK` (the sizes `RENDER_TARGET_SIZE`/`RENDER_SOURCE_SIZE` = w,h,1/w,1/h, `TEXCOORD_CLAMP_1` = 0,0,1,1, `MATERIAL_COLOR` for the weighted add); `Host::record` then draws the colour pass as `tonemap_veil_filmic` or `tonemap_veil_linear` with the chain's `Up(1)` as `FEEDBACK` (READ from the exe's colour pass: second texture `FEEDBACK`, strengths `PERCEPTUAL_PARMS.xy` = veilstrength, veilbackgroundstrength; was INFERRED as `POST_EFFECT_0`; the colour pass binds the scene as its first texture and target 100 as its second; INFERRED: the names, from the order of the exe's texture slots, scene then `POST_EFFECT_0` then `POST_EFFECT_1`). Runs when the map's light set has `veil` = 1 (default since 3 Nov), `BO2MP_H1_VEIL=1`/`0` forces it. A missing material or texture in the zones logs `h1 veil: ...` once and falls back to the plain tone map for the rest of the run. Not done: the `_blur` and `_dark` colour variants (view flags not read), the dvar names. Strengths come from the light set, or `BO2MP_H1_VEIL_STRENGTH`/`_BG` for tests (also `_KIND`, `_MODE`, `_QUALITY`, `_STOP=n`). Open: on software-GPU Shipment renders any strength 0.002..1 whites out by pass 8 (HDR scene is ~1000x raw units); retail zones carry no light sets (see the tone map section; checked again 3 Nov across all staged zones, which hold no string tables), so a retail map's veil strength is the default 0 and the veil adds nothing; the wash at test strengths is the HDR level, not a bug found. Correction (3 Nov): HMW user maps do carry light sets. mp_rust (staged with its `.pak`) has `veil` 1, `veilstrength` 0.139, `veilbackgroundstrength` 0.9; rendered on the software GPU with the veil on, the picture shows a soft glow on the sky and bright metal, no wash, so the chain and colour pass behave. The earlier wash was the forced test strengths. The grey surfaces in earlier renders were the missing `<zone>.pak`; with it the map is textured. Still unchecked on a real GPU. Needs the owner's PC: run an MWR map with `BO2MP_H1_VEIL=1` and send the log and a screenshot.

## Model-lightmap containers: findings (3 Nov, software GPU, Shipment)

Reproduced offline: the stacked containers (`ml/` materials, static models with flag 0x100) draw black while `H1_DBG_LM=white` lights them. Ruled out, each by a render: the blended-surface skip (`H1_DBG_BLEND_DRAW=1` draws them anyway, no change), the scale/offset order (`H1_DBG_MLM_SWAP`), the unwrap stream (`H1_DBG_TC4`), the page index (`H1_DBG_MLM_PAGE=0|1|2`), and a missing `LIGHT_PROBE_AMBIENT` (`H1_DBG_PROBE_AMB=<v>`). Read: the 175 model surfaces with an unwrap hold sane 0..1 half-float uvs (`h1_unwrap`), 38 placements carry offset/scale/page (page 0..2, tile-like offsets 0/0.5), the VS computes `scale * uv + offset` from `MODEL_LIGHTMAP_PARAMS` (cb0 row 4) as the code writes it, the pass binds probe/primary/secondary at 1/2/3, and the secondary page holds non-zero data in the container region (0.08..0.44). The primary page is mostly 0 there (sun visibility). The cause is not found; next: read back the lightmap samples the PS gets for these draws, or compare against a real-GPU capture. New tools: `h1_img_paks` (which `imagefile<N>.pak` holds a zone's pictures; Shipment needs paks 1,3,5,7,9,11,13..., pak 1 is 5.2 GB and cannot be staged), `h1_unwrap`, `h1_lightmap_dump <zone> <outdir>` (raw pages).

Container follow-up (3 Nov): `H1_DBG_LM=primary|secondary|probe|white` now whites out one input at a time. Result on the stacked containers: the primary page and the reflection probe change nothing; the secondary page alone lights them (as `white` does). The secondary data in their region is real baked light (viewed as a picture: two stacked 1024x2048 layers, the container block is dark there: layer 0 about 0.08, layer 1 about 0.44), so the containers may simply be baked dark and the engine lights them with more than the lightmap (not read). Not resolved; needs a reference picture of the real game's Shipment containers.

Model-lightmap constants, READ from the exe (3 Nov): `MODEL_LIGHTMAP_PARAMS` = (scale.x, scale.y, offset.x, offset.y) from the model's lighting record when flag 0x100 is set and its lightmap index is not -1; `LIGHT_PROBE_AMBIENT` only with flag 0x20; light grid coordinates only with flag 0x40. Matches `render_h1::scene`. The dark stacked containers are therefore not a constants bug; their sun comes from the real-time shadowed sun (`sm` techniques), so check them with `BO2MP_H1_SUN_SHADOW=1` on a real GPU (on the software GPU the picture did not change with it on). Details and addresses: `docs/exe_notes/MWR_H1_MODELLIGHT.md`.

Stacked-container checks (3 Nov, later): the model surfaces of all `ml/` placements carry their unwrap (15 of 15 surfaces); `H1_DBG_MLM_FLIP=u|v|t` (flipped or transposed unwrap) and `--ev 3` do not light them while the ground does, so they receive far less light than the ground's shade; the data under their lightmap region is not black (secondary 0.08/0.44). The sun-lit ground patches are polygon-shaped and clip to white, which points at the world surfaces' lightmap coordinates or the primary page's content, not the model path. Next: sample the lightmap for a known world triangle on the CPU and compare with the rendered value.

Lightmap and shadow checks (3 Nov, Shipment, software GPU): world vertex lightmap coordinates are all inside 0..1 (`h1_lmap_uv`), so they are not the cause of the white patches. The primary lightmap pages are a hard 0/255 mask (85% 0, 11% 255, almost nothing between, decoded by hand): baked sun visibility, with no soft edges, so the polygon-shaped sun patches are that mask shown unfiltered by the real-time sun shadow. The sun shadow is on by default and changes about 20,000 pixels of a test view when switched off, so it does run here; what it does for models (the `ld1` model lit pass reads `SHADOWMAP_*` constants 196..202, `SUN_SHADOWMAP_*` 269..272 and `TRANSPOSE_SHADOW_LOOKUP_MATRIX` 364, all set) is the remaining suspect for the black stacked containers. `h1_ccname <index>...` prints code constant names.

## Performance (render_h1; 3 Nov, software GPU, so CPU numbers only)

Profile: `H1_PROFILE=1` prints per pass the CPU time of the constants update and of the encode, and now also draws, culled draws, pipeline sets, bind-group sets and triangles; `merged draws` lines show the load-time merge. Baseline on Shipment (camera 600 500 330, pitch 14, yaw 203, 960x540, lavapipe): scene pass update 2.65 ms, encode 2.52 ms, 11,133 draws, 849 pipeline sets, 11,267 bind-group sets, 6.2 M triangles; the two shadow cascades together 1,479 draws, 1.0 ms encode. GPU time is not measured here (the software GPU says nothing about a GTX 1080-class card); GPU timestamps are not added yet.

Change 1 (`BO2MP_H1_MERGE_DRAWS=0` turns it off): neighbouring draws with the same pipeline, bind groups, constants offsets and vertex streams, and touching index ranges, are joined at load, bounds kept as a union only when it stays within 1.5x the boxes' volume. Scene draws 11,133 -> 10,347 (-7%), encode 2.52 -> 2.11 ms; the picture below the sky is pixel-identical (the sky region differs run to run from `GAMETIME`). Most draws are not mergeable because each placed model has its own constants slot and material groups; the big costs left are the per-object constants update (about 2.7 ms) and one bind-group set per draw, to be attacked next with instancing and per-material grouping. Not done: GPU timestamps, ring-buffered per-frame constants, instancing, per-cascade light/shadow right-sizing, the settings table (waits for the shared hook in `docs/IW6_NATIVE.md`).

## Graphics settings and screen effects (render_h1; 3 Nov)

`render_h1` now reads the shared `native_quality` hook (`Host::apply_quality`, called when the map's host is made and whenever `native_quality::revision()` moves; one log line `mwr quality: ...`). One setting drives every game's renderer; this is MWR's side of the table in `docs/IW6_NATIVE.md` "Performance":

| value | `render_h1` | when |
|---|---|---|
| `sun_shadow` | the sun shadow pass (`LIT_DIR_SHADOW`) on or off (`BO2MP_H1_SUN_SHADOW=0` still wins) | next frame |
| `bloom` | the veil chain (still needs the light set's `veil`) | next frame |
| `ambient_occlusion` | `screenfx` AO pass (`BO2MP_H1_SSAO=0` off) | next frame |
| `depth_of_field` | `screenfx` DOF pass (`BO2MP_H1_DOF=0` off) | next frame |
| `motion_blur` | `screenfx` camera motion blur (`BO2MP_H1_MOTION_BLUR=0` off) | next frame |
| `max_texture` | picture size cap, at most 1024 (2048 ran the owner's card out of memory; `BO2MP_H1_MAX_TEX` wins) | next map |
| `local_lights` | the spot and omni light passes (below); at most 4 lights a frame (the game lights a surface by up to four), nearest first (`BO2MP_H1_LOCAL_LIGHTS=n`, 0 off) | next frame |
| `draw_distance` | draws whose bounds start beyond 14000 x the scale inches are left out (0.5 on LOW, 0.75 on MEDIUM; none on HIGH/ULTRA; INFERRED, not the game's per-model distances) | next frame |
| `anisotropy` | the cap on the material samplers (`BO2MP_H1_ANISO=n` wins) | next map |
| `lod_distance` | scales the distances at which a placed model's levels change (0.5 on LOW, 0.75 on MEDIUM) | next frame |

`render_h1::screenfx` (INFERRED look, not the game's passes; those are `cs_hemi_ao_*`, `dof_*` and the motion blur programs, not yet translated): full-screen passes over the fogged HDR scene and the scene depth, before the veil and the tone map. AO: 12 taps in a 48-inch radius, strength 0.65, faded out by 1500..3500 inches. DOF: far blur from 1000 to 7000 inches and near blur inside 60 inches, 16 taps, at most 0.6% of the frame height. Motion blur: the last frame's view, 1/120 s shutter, capped at 4% of the width (not drawn on a still first frame). Fog, film and the tone map are never touched. Checked offline on Shipment (lavapipe): AO darkens the container bases and creases; no validation errors; `cargo test -p render_h1` 61 pass. Not seen on a GPU.

### Spot and omni lights (render_h1; 3 Nov)

READ from the zones' technique names (`h1_slots --summary`): slot 37 is `l_spot_*`, 38 `l_omni_*`, 39 `l_sm_spot_*` (shadowed spot), 40 `l_sm_spot_cuc_*`, 43 to 46 the same four with `_dfog`. `Host::record` now draws, after the opaque scene and before the fog, one pass per picked light (`local_light::pick`: spheres reaching the view, nearest first, up to `frame_cap`): the draws whose bounds reach the light's sphere (`box_planes` as the culling planes; a draw with no bounds is left out) with slot 37 or 38 and the light in constants slot 0 (`LocalLight::write`), its own constants variant from 8. The techniques blend additively, so the draw list files them as blended (`Part::All`). INFERRED: the picture checks only on the software GPU: on Shipment (5 spots) each light draws 600 to 800 draws and lights up a region around it (about 0.9% of the pixels from one camera); the shadowed spot (39) and the cookie are not drawn, the light's shadow slot is 0, and `LIGHT_FALLOFF_PLACEMENT` and `LIGHT_CUCOLORIS_ANIM` are left at zero (the cone's look may differ from the game's). Cost: about 0.3 to 0.5 ms CPU per light.

### Per-preset cost (render_h1; 3 Nov, Shipment, 960x540, lavapipe, so CPU only)

`mwr fps:` is logged with the first frame of a map in game and printed by `h1_native` (`H1_WARM=n` adds n warm frames and prints the last as `warm: mwr fps: ...`). CPU time of one `Host::record` (encoding; a software GPU, so not a render-thread figure for a GTX 1080), local lights off (`BO2MP_H1_LOCAL_LIGHTS=0`) so the presets compare on equal ground:

| preset | CPU ms per frame | effects on |
|---|---|---|
| LOW | 10.3 | none; no lights; draw distance 7000 in |
| MEDIUM | 12.2 | depth of field |
| HIGH | 13.9 | AO, DOF, motion blur |
| ULTRA | 13.4 | AO, DOF, motion blur |

(Run to run the spread is about 1 to 2 ms; HIGH and ULTRA differ only by noise on the CPU side, their cost is on the GPU, which is not measured here.) Local lights add about 0.3 to 0.5 ms CPU each. Memory (measured later the same day, lavapipe, peak resident): 4.76 GB with no light, 5.58 GB with one, 5.67 GB with four: the first light costs about 0.8 GB once (the spot and omni techniques' pipelines are built on first use), each further light about 30 MB. (An earlier note here said 230 MB per light; that was wrong.) An 8 GB machine still ran out of memory when frames were repeated with the lights on. Not done: GPU timestamps, instancing and the per-frame constants ring (each light variant rebuilding the per-object state is the biggest cost left).

### Model levels of detail (render_h1; 3 Nov)

READ: each `XModel` lists up to six levels with a far-end distance (`dist`, inches; levels with no surfaces carry 1,000,000). Placed models now load every level that holds surfaces; the main view draws the level whose range holds the eye's distance to the model's origin (the previous level's far end up to its own; the last level goes on), the shadow views draw level 0 only. A vertex-lit placement keeps level 0 (its lighting values belong to it). `BO2MP_H1_LODS=0`: level 0 only (before). Shipment, same camera, 960x540, lavapipe: triangles 6,239,864 -> 3,219,686 (-48%), draws 10,347 -> 10,539 (+2%, more pipelines: 849 -> 901 sets), 0.19% of the pixels below the sky differ. INFERRED: the game's own LOD scale (`r_lodScale`, per-model bias) is not read; the distance is the model origin's, not the nearest surface's. Not seen on a GPU.


## Usermap shade (3 Nov, INFERRED)

Reported: the MW2, MW3 and AW usermaps are too dark. Offline on mp_favela (its own light set: `diffusecolorscale` 3.48, sun colour 1.1, so the sun is about 3.8 against Shipment's 15): `H1_DBG_LM=secondary` shows the shaded faces take their light from the secondary lightmap page alone (R16G16B16A16_FLOAT, decoded as on the retail maps, no format difference), and that page is dark beside the scaled sun. The engine's own treatment of the page is not read. Applied: the baked secondary page is multiplied by the light set's diffuse scale when it is above 1 (`light::secondary_scale`, once per picture at upload). Retail maps (scale 1) are untouched. `BO2MP_H1_LM_SCALE=<n>|off` overrides; `BO2MP_H1_LIGHT=fitted` leaves the page alone. The automatic exposure keeps the frame average at its key, so this changes contrast (the shade comes up, the lit parts come down a little), not the overall level. Not checked against the real game; AW and MW3 maps could not be staged whole (files over 400 MB), so only favela was rendered. New debug: `H1_DUMP_LIGHTSET=1` (h1_native prints the light set), `H1_DBG_LMINFO=1` (lightmap picture formats).

## GPU timestamps and the per-object constants (3 Nov)

`H1_PROFILE=1` (or `BO2MP_H1_PROFILE=1`) now also prints `mwr gpu ms/frame over N frames: shadow, opaque, lights, fog_blended, screenfx, tonemap_film, total` from timestamp queries (needs TIMESTAMP_QUERY and the in-encoder feature, read back four frames later; `H1_PROFILE_EVERY` sets the interval). The per-object constants fill no longer allocates per object per frame and only the changed byte range of each buffer is uploaded: Shipment `update` 2.65 ms to about 1.5-1.9 ms (lavapipe, CPU side).

## Where the light memory goes (3 Nov)

`H1_PROFILE=1` on h1_native prints `constants: N plans, M objects, K MB mirrors` (Shipment: 466 plans, 26,246 objects, 1 MB). The per-object constants and the per-light variants are therefore small; sharing them between lights would save next to nothing. The +0.8 GB the first local light adds on the lavapipe box (peak 4.76 to 5.58 GB, then about 0.03 GB per further light) is the new light-technique pipelines being compiled once (software Vulkan keeps the JIT code in RAM); a real GPU driver does not hold that in system memory. ULTRA with a light on a 6 GB-limit box is killed by the memory limit for that reason, offline only.

Follow-up (3 Nov, later): the model light grid takes the same factor (`h1 shade:` note; a tree and lamp post in shade were black beside the lifted lightmap). Rendered against real files: mp_favela (MW2, diffuse 3.48) and mp_alpha (MW3 Lockdown, diffuse 2.3, `.ff` 349 MB + `.pak` 33 MB staged whole): shade and models in shade now read. The AW maps checked (mp_highrise2, mp_seoul2) have `diffusecolorscale` 1, so the factor does nothing for them; their `.pak` files are 0.9-1.0 GB and cannot be staged (400 MB per file), so their lightmaps were not rendered and the AW darkness is NOT addressed. Next for AW: a screenshot, or their lightmap pages.

## Usermap exposure (4 Nov, INFERRED)

The owner's in-game screenshot of an AW usermap (median pixel 0.13 sRGB, mean 0.14) against Shipment's offline frame (median 0.35, mean 0.43) showed the cause behind "all usermaps too dark". Retail light sets fix the exposure (`tonemapauto 0`); the usermaps' light sets use the automatic exposure, whose key (`auto_key`, 0.18 or the adjust curve) goes into the filmic curve as the scene value. With the usermaps' curves (black 0.05, crossover 2.5-3, toe 0) the curve shows 0.18 as 0.0136, so the frame average landed near black. Now `tonemap::shown_key` lifts the exe's key by `auto_key_boost`: the factor that makes the curve show the log-average at `AUTO_DISPLAY_LEVEL` (0.07, fitted so usermaps average 0.28-0.43 against Shipment's 0.35-0.43; the dark-scene adjust of the light set's curve still applies on top). Only filmic light sets with automatic exposure change; fixed-exposure retail maps are untouched. `BO2MP_H1_AUTO_KEY=exe` restores the old key, `=<level>` sets the aim. The real engine's own handling of the key is not read, so this is a fit, not a translation. Offline means (960x540): mp_alpha 0.15 to 0.43, mp_seoul2 (no pak) 0.14 to 0.28, mp_favela 0.16 to 0.41. Together with the shade factor above.

## Instanced placements (4 Nov)

Repeated placements are drawn as one instanced call (`render_h1::instancing`, ported from the Ghosts renderer's): in a run of visible draws that repeat one another but for their per-object block (same pipeline, bind groups, mesh, index range, base vertex and LOD level) whose blocks follow one another in the plan's object buffer, up to 64 go as one `draw_indexed` with an instance each. The pipeline's instanced form has its vertex program read the block from a read-only storage array (bound at the first draw's block, indexed by `instance_index`; the translated WGSL's uniform declaration is rewritten, `instancing::patch_vertex`), so the body is unchanged. Only pipelines whose pixel program does not read that block, with no layer stream and one dynamic offset, are instanced; a form that fails validation is left out with a `note: h1 instancing:` line. Each plan's object buffer has STORAGE usage and 64 blocks of padding. `BO2MP_H1_INSTANCING=0` draws every draw alone.

Shipment (960x540, LOW, lavapipe, warm, `H1_GAMETIME=1`): the main scene pass 10,252 draws, 2,726 folded; scene record 5.32 to 3.71 ms; bind-group sets 11,018 to 8,302 (pipeline sets rise 850 to 1,433, the plain and instanced forms alternate); frame CPU 13.74 to 10.12 ms. Pictures byte-identical to instancing off (with the game time fixed; without it two runs of the same build differ by up to 10,000 pixels). `H1_PROFILE=1` prints the folded count in each `record` line.

`mwr look:` (4 Nov): after 240 frames the game logs one line per map with what drives its brightness: the light set's tone map mode, automatic or fixed exposure, the metered scene luminance, the exposure, the key boost, the shade factor, the diffuse scale and the sun colour. For maps that cannot be rendered offline (the AW usermaps, whose paks are over the 400 MB staging limit): a log from the owner's PC gives the numbers.

## Hidden and tiny draws are not drawn (4 Nov)

New shared crate `native_cull` (pure maths, the other renderers can take it): the owner's rule is that what cannot be seen, or is too small or far to show, is not drawn at full detail.

- **Occlusion** (`native_cull::Occlusion`, `BO2MP_H1_OCCLUSION=0` off): at load the map's opaque, non-discarding world and placement triangles of at least 400 square inches (the largest 12,000; placed models with LODs are left out) become occluders, each remembering which side the game draws. Each frame they are clipped to the near distance and the screen and rasterised into a 192x108 buffer that keeps, per pixel, the farthest depth of a triangle covering the pixel centre. A draw whose bounds start 12 inches behind that depth over every pixel they cover, plus a ring of one pixel, is skipped (in the depth pre-pass, the lit pass and the light passes alike, not the shadow views). One-sided surfaces only hide from their front: counting them from both sides hid a building's facade (found by comparing pictures).
- **Too small** (`BO2MP_H1_SMALL_CULL=<radians>`, 0 off): a draw whose bounds diagonal spans less than 0.010, 0.007, 0.005 or 0.003 radians (LOW to ULTRA, about 5 to 1.5 px at 1080p) is skipped. Spawn view of mp_highrise2: 1,430 draws at 0.006 for 74 pixels changed.
- `H1_PROFILE=1` prints `occlusion buffer N ms` (about 2 ms CPU in release) and, per pass, how many draws were hidden or too small. `H1_DBG_OCC=1` prints the occluder census.
- Checked on mp_highrise2 from inside the map: 4,222 draws to 167 in one view (before the facing fix: wrong), 665 to 599 and 209 to 209 in others, pictures identical (0 pixels over 32). The gain depends on the view; nothing was timed on a real GPU.
- Not taken from the gamedev cheat sheet on purpose: LOD hysteresis (the game's own LOD switch is hard), game-feel items (BO2 feel is not ours to change).

`mwr pictures:` (4 Nov): after 240 frames the game logs how many pictures the map asked for and could not load (first 24 with the reason) and how many draws were left out for it. A picture that is not loaded reads as grey with alpha 0, so an alpha-tested or blended surface using it draws nothing: the likely cause of see-through textures in game, which the offline viewer (which reads every picture) does not show.

## All-maps survey, 66 HMW usermaps (4 Nov, offline, LOW, from the owner's PC)

- `common_core_mp.ff` stops at asset 581 of 813, the first `vehicle` ("zone ends at ..." after about 1.2 MB: the inline turret weapon of the vehicle walks off the data). Lost with it: 142 pictures, 53 materials, 16 techsets, 8 weapons, 7 models, 2 vehicles. The pictures the 66 maps asked for were all found (no missing pictures but a few water reflections), so this is not shown as see-through offline; whether it matters in game is unknown (needs the `mwr pictures:` line).
- Draws left out for a missing picture (blended surfaces): mp_venus 38 (hdr_sky, pool and water reflection pictures not found), mp_refraction 11, mp_greenband 4, mp_recovery 2. Shaders reading a vertex input the surface lacks (TEXCOORD1 in a world vertex, TEXCOORD5 in a model vertex) drop one or two draws on urban, solar, terrace, detroit, plaza2, derail, courtyard_ss, venus.
- Brightness (offline mean, 0 to 1): darkest mp_nightshift 0.09 (night), mp_rundown 0.11 (dusk, whole shaded half black), mp_storm 0.16, mp_underpass 0.16, mp_brecourt 0.18; brightest mp_spark 0.69, mp_urban 0.66, mp_terrace 0.65. mp_terminal 0.23 (fixed exposure, EV 10; the owner's in-game frame was 0.17).
- Draw counts per map are 12,900 (rust) to 81,800 (spark); after frustum culling 150 to 6,000 are drawn.

### Guest zone walk and the red guns (4 Nov 2026)

* `hmw_patch_code_post_gfx_mp.ff` used to stop at asset 99 (VirtualLeaderboard) and then at the Ddl and StructuredDataDef assets; the walker now reads all three (layouts measured on that zone, no reference) and the zone walks 1001/1001, with its 136 pictures. None of the missing weapon pictures is among them.
* Still red on the HMW guns: `i_h2_*`, `i_iw4_*`, `h2_weapon_striker` and `camo_*_flow_surfparms_01` (3 in ar1, 8 in smg, 20 in shotgun). They are not in the five retail zones searched or in any `hmw_*` zone (all walk fully); the retail zone folder has about 2000 files and only those five are searched, so the next step is a name search over the rest.
* In game, `BO2MP_BO3_LOOK_H1=0` (the world drawn without the extra grade) shows the first-person gun black, not red, and the scene flatter and cooler (mean 0.20 against 0.175 with the grade). The `mwr look:` line is identical either way (exposure x1.259, luminance 0.166 on Terminal).
* Fixed-exposure light sets keep only a small black point (`BO2MP_H1_BLACK=exe` for the stored value): Rundown's 0.2 sat above the scene's own average and clipped the shade to black.

## Intact breakable glass (5 Nov 2026)

Before this, the map's glass pieces were not drawn at all (the walker read the
`FxWorld` glass system but kept only its name), so windows and skylights were
open. Now `fastfile_h1` captures the glass definitions (material, shattered
material, `texVecs`, half thickness) and the initial pieces with their geometry
words (`Capture::fx_glass`), and `render_h1::glass` turns each piece into a
two-sided pane in the model mesh (identity object): packed `i16` vertices x 1/32
along the frame's first two axes from the piece origin (the sibling engine's
scale; checked on Highrise, where a 384 x 1952 pane has the stored radius 31.09),
the stored triangulation when the piece has one, else a fan. Terminal: 215
panes, Highrise 124, all with materials. Seen offline on Terminal's office
window and skylight (pane pictures need the map's `.pak`; Highrise's is too large
to stage, so its panes are left out there by the existing rule for blended
surfaces with missing pictures). `BO2MP_H1_GLASS=0` leaves them out.

Not done: breaking (bullet hits, shatter effects, shards, the shattered material),
the glass brush contents in collision, and lighting of the panes (they take the
default model lighting). Next: shot detection against the panes on the server
side, then the shatter pieces from the sibling engine's `fx_iw4::glass_*`.

### Usermap brightness against the loading pictures (5 Nov 2026)

Reference: each usermap's own loading picture (`<map>_load.ff`, `loadscreen_<map>`; `h1_pics` writes it as a PNG). Measured on the 35 maps whose picture is in the load zone (the AW maps keep theirs in the 1 GB pak). Linear log-average, picture over our render (the owner's 4 Nov `all_look` renders):

* Every fixed-exposure usermap light set is an MW2 port with an old-style sun (power 1.0-1.6, EV 8.8-10): 2x to 23x too dark (Rundown 23, Brecourt 15, Nightshift 15, Afghan 14, Rust 10, Storm 9, Compact 8, Underpass 6, Quarry 6, Complex 5, Terminal 4).
* Automatic-exposure usermaps: 0.3x-3x, except Highrise, washed white.
* Sun power (brightest channel x diffuse scale) splits the maps: AW suns 8-60 (retail-like; Shipment 15), MW2/MW3/MWR ports 1-6, AW night maps 0.5.

Applied (all INFERRED fits, `render_h1::light`, `render_h1::tonemap`):

* `light::meters_old_fixed`: a fixed exposure with an old-style sun (0.9..7) is metered automatically instead (its EV assumes retail light levels). `BO2MP_H1_OLD_FIXED=exe` keeps the EV.
* `light::usermap_lift`: an old-style sun and the baked shade are lifted to 15 on those metered sets, to 3.8 on sets that were already automatic (their authors tuned the exposure for the content). `BO2MP_H1_SUN_FLOOR=<n>|0`.
* `tonemap::AUTO_TOTAL_CAP` 8: our automatic key boost times the light set's own `tonemapautoexposureadjust` lift never passes 8. Highrise (hand-made curve, crossover level 0.011, its own +3.26 stops) had 245 and a white frame.
* `light::KEEP_LOOK`: maps the owner approved as they look (mp_favela) are never lifted or re-metered.

Result on the staged maps (software GPU, game meter path `H1_METER=40`): Highrise x0.91, Alpha x1.10, Terminal x0.75 (picture is the interior), Rust x2.8 (picture has a heavy bright haze the render lacks), Favela unchanged. Still open: the bright haze/fog of the MW2 pictures (Rust, Rundown), and Terminal's sky (a dark storm picture where the real one is bright).

### Horizon MW's files surveyed for lighting and effects (5 Nov 2026)

What HMW's zones hold besides the map data already read, checked on the staged zones (HMW's `hmw_files`, `hmw_pre_gfx`, `hmw_patch_code_post_gfx_mp`, `hmw_patch_ui_mp`, `hmw_post_gfx`, and the Terminal, Rust, Favela, Highrise and Alpha map zones). Tools: `h1_grep`, `h1_rawfile`, `h1_assets`, `h1_tech`, `h1_lights`, and the new `h1_cluts` and `h1_tables` examples.

| Data | Where | Used |
|---|---|---|
| Map script lighting tweaks (`setdvar` in `maps/mp/<map>.gsc`: light grid intensity/contrast, colour scales, sun tweak strength, tone map) | every map zone; HMW's own copies for MW3/MWR maps in `hmw_files.ff` | yes, under their engine switches (`look::apply_script_dvars`, 5 Nov); light grid **contrast not applied** (its maths not read) |
| Light sets `maps/createart/<map>_lightsets_hdr.csv` (and a non-HDR `_lightsets.csv`) | map zone | HDR one, yes |
| Vision `vision/<map>.vision` (Rust's zone carries 203 vision files: killstreaks, end game, thermal...) | map zone | the map's own, yes; the event visions no |
| Fog `maps/createart/<map>_fog_hdr.gsc` | map zone | yes |
| Placed effects `maps/createfx/<map>_fx.gsc` | map zone | yes (`assets::lane::h1_fx`) |
| Spot/omni lights (Terminal 8, Highrise 10, Rust 3, physically based, shadowed) | ComWorld | yes (3 Nov; capped by quality) |
| Reflection probes ("environments", Rust 15) | GfxWorld | yes |
| Colour lookup tables (`clut/clut_*.clut`, Rust 6) | map zone | no, and not needed: all six are identity tables (HMW placeholders); Terminal, Favela, Highrise, Alpha have none |
| Sun sprite, lens flare, sun blind and glare (`sun/<name>.sun`; Rust's is CoD4's `bog_b.sun`) | map zone raw file | **no**: only the flare material is captured from GfxWorld; Ghosts' `render_iw6::sun_fx` is the model to port |
| HMW shaders (spot/omni lit variants, MW2 Remastered `_h2` lit and velocity shaders) | `hmw_patch_code_post_gfx_mp`, `hmw_post_gfx` | yes: no technique is skipped and no image missing on Terminal or Rust |
| HMW string tables (33) | `hmw_files`, `hmw_pre_gfx` | gameplay only (stats, camos, perks, ranks, challenges); no shared light set |

No other lighting data is stored. Open from this survey: the light grid contrast maths (exe), and the sun sprite/flare/blind/glare.
