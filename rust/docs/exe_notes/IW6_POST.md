# Ghosts (IW6 MP): post effects and shadow sampling, from the exe (2026-11-05)

Source: `iw6mp64_ship_all.c` (the provided decompile, static only), `iw6/dvars_all.tsv`,
`iw6/const_src_code.tsv`, the zones' own post techniques (probe: `cargo run -p render_iw6 --example
iw6_post_probe <word> <zone.ff>...`, `PROBE_WGSL=<dir>` dumps the translated shaders). Rust code carries no
addresses; they live here. READ = taken from the exe; INFERRED = ours.

## 1. Far sun cascade texel

`sm_sunSampleSizeNear` 0.25 (READ earlier). The far partition texel is 4 times the near's (READ). The earlier
inferred ratio was 8 (still `BO2MP_IW6_SHADOW_DVARS=0`; `BO2MP_IW6_SHADOW_FAR_RATIO=1..16` overrides).
`sun_cache::FAR_HOLD_TEXELS` stays 64.

## 2. SSAO

- dvars: `r_ssao` (mode, default 1: `ssao_calc_fast`; 2: `ssao_calc_slow`; 0: none), `r_ssaoStrength` /
  `r_ssaoPower` from the vision (0.35 / 15.99 on mp_frag; strength 0 draws nothing), `r_ssaoWidth` 36 px at 720p,
  `DepthScale` 0.1, `FadeDepth` / `RejectDepth` 2500, `GapFalloff` 0.25, `GradientFalloff` 0.05, `BlurSharpness`
  50, `BlurStep` 1, `Diminish` 1.
- Passes: `ssao_calc_*` (reads `RANDOM_ROTATIONS`, `FLOATZ`) into an R8 target, `ssao_vblur_lq` (along y), then
  the post-opaque pass `postopaquefx_ssaolq_<fog>` (blur along x, AO applied; SSAO at t5). The `hq` forms are
  served by the same shaders here. `RENDER_TARGET_SIZE` = the target's (w, h, 1/w, 1/h).
- FLOATZ: the engine's equals ours when the camera near is 4 (`near * band / distance`), so no conversion.
- Code: `render_iw6::ssao`. INFERRED: the half-size target on HIGH (ULTRA is the render size).

## 3. Depth of field (the `r_dof_hq` 0 chain)

- The eight values (view model start / end, near start / end, far start / end, near blur, far blur) are the
  view parameter block's floats 0x220..0x23c, copied from the vision/script block (`r_dof_tweak` 0), from the
  dvars (`r_dof_tweak` 1, defaults: viewModelStart 2, End 8, nearBlur 6, farBlur 1.8, `r_dof_bias` 0.5), or
  zeroed (tweak 2 and others). Copy: `FUN_14060ce60`.
- Active when (`FUN_1406414c0`): no thermal/other override and vs+1 < ve, or ns+1 < ne, or (fs+1 < fe and farBlur > 0).
- Chain selector `FUN_14063c8f0`: hq 0 and active runs `FUN_140641070` (constants, below) then `FUN_14063ed60`
  (passes); hq 1/2 run `FUN_14063b8f0` (not driven). Passes of `FUN_14063ed60`: `dof_downsample` into a quarter-size
  target, a gaussian of radius `nearBlur * 0.25 * H / 480` target pixels (`FUN_140634490`, a generated kernel;
  ours is the bloom's 7-tap blur: INFERRED), `dof_near_coc` (rgb of the sharp downsample, alpha
  `2 * max(blurred alpha, alpha) - alpha`), then `postfx_dof` into the frame (scene, near-spread target, blurred
  target, FLOATZ). Which of the two targets is `POST_EFFECT_0` / `_1`: INFERRED from the shader (t4 carries the
  circle in alpha and is weighted by the mid weight; t5 is the blur).
- Constants (`FUN_140641070`; `n` = camera near, `H` = height, `p(x) = pow(x, r_dof_bias)` via `FUN_140732d00`):
  - `DOF_EQUATION_SCENE` = (ns', fs', ns'', fs'') with near: if `ne <= max(n, ns)` then ns = 0, ne = n/2;
    scale = `n * 63/64 / (ns - ne)`, bias = `ne / (ne - ns)`; far (only if `max(n, fs) < fe`): scale
    `n * 63/64 / (fe - fs)`, bias `fs / (fs - fe)`; packed (near scale, far scale, near bias, far bias).
    The weight is `scale / FLOATZ + bias`.
  - `DOF_EQUATION_VIEWMODEL_AND_FAR_BLUR` = (`0.0015625 / (vs - ve)`, 63/64, `ve / (ve - vs)`, `p(farBlur / nearBlur)`)
    with vs, ve = (0, 0.05) when `ve <= max(0.1, vs)`.
  - `p5 = p((672 / H) / nb)`, `p6 = p((1728 / H) / nb)`: `DOF_LERP_SCALE` = (-1/p5, -1/(p6-p5), -1/(1-p6), 1/(1-p6)),
    `DOF_LERP_BIAS` = (1, p6/(p6-p5), 1/(1-p6), -p6/(1-p6)); `DOF_ROW_DELTA` = (0, 1/H, 0, 0).
  INFERRED: H is the render height; the guards on p5 / p6 (the exe divides unguarded).
- The vision holds no DOF keys: in multiplayer the values come from aiming (the weapon's ADS) and scripts.
  `Host::set_dof` takes the eight; `IW6_SHOT_DOF="vs/ve/ns/ne/fs/fe/nb/fb"` in `iw6_shot`.

## 4. Sun blind, glare and flare

`GfxWorld.sun` (a `sunflare_t`, offset 1936 in the record; `fastfile_iw6::walk::SunFlareRec`): hasValidData,
spriteSize, flare min/max size, min/max dot, max alpha, fade in/out; blind min/max dot (cosines), max darken, fade
in/out; glare min/max dot, max lighten, fade in/out; `sunFxPosition` (the direction); sprite and flare materials.
State machine (READ): `target = ramp(dot(view forward, sun dir), min, max) * visibility`; the state moves toward
it by `dt / fadeIn` up or `dt / fadeOut` down (ms; under 1 ms a jump); the output is `state * maxDarken`
(alpha of `GLOW_APPLY.y`, darkens the scene) and `state * maxLighten` (`GLOW_APPLY.x`, added to every colour)
in `material_bloom_apply`. Visibility: the engine counts the sprite's occlusion query; ours counts depth samples
over the sprite's disc that see the sky (INFERRED: 8 x 8 grid, sky past 20000 units, read back late) and the
apply blend is (one, one minus source alpha) (INFERRED; identical to one / one at darken 0). The flare sprite
picture is not drawn (its material picture is not in the staged paks).

## 5. Motion blur (velocity chain driven, 2026-11-05)

Dvars (READ): `r_mbEnable` (enum, default 0; the option row turns it on) / `r_mbEnableA` (archive, default true),
`r_mbFastEnable` (default false) + `r_mbFastPreset` (the `postfx_mblurhq_fast_{low,med,high}[_depthmask]` forms),
`r_mbCameraRotationInfluence` 1, `r_mbCameraTranslationInfluence` 1, `r_mbModelVelocityScalar` 1,
`r_mbStaticVelocityScalar` 1, `r_mbViewModelVelocityScalar` 1, `r_mbViewModelEnable` true,
`r_mbPostfxMaxNumSamples` 15 (0..32).

Per frame (the view-parameter prep just before `FUN_14060db90`; the view-parameter block at +0x1ad0 / +0xca0..0xd3c): the previous frame's view matrices and
eye are kept; the camera rotation is interpolated by the rotation influence and the eye by the translation influence
(both 1 by default: the previous matrices as they were); the reprojection matrices land in the view block. Block values
the passes read: `MOTION_BLUR_HQ_PARAMS` = (0, `r_mbPostfxMaxNumSamples`, 1, 0); the static velocity scalar at +0xd30.

Chain of the default (`r_mbFastEnable` 0) path, `FUN_14063cc70` (render target ids in brackets are the exe's table):
1. Velocity picture `SCENE_VELOCITY` [64]: written while the scene draws (`velocity_static` for the world, the models'
   `*_velocity_*` techniques, `velocity_vm` for the view model; each from the previous frame's matrices / bones).
   The postfx shaders decode `v = (tex.xy * 2 - 1)`, `v = v * |v|` (a signed square: the picture stores sqrt(v) in
   two channels, 8 bits), in UV units; `postfx_mblurhq` multiplies by `RENDER_TARGET_SIZE.xy` for pixels.
2. `FUN_14063f7a0`: two draws of `postfx_mblurhq_velocity_pass0` (tile max, 20 taps at 1-texel steps, take the
   longest decoded vector): first along x into [65] (width / 20), params `MOTION_BLUR_HQ_TILE_MAX_PARAMS` =
   (0.5/w - 0.5/tileW, 0, 1/w, 0); then along y into [66] reading [65], params (0, 0.5/h - 0.5/tileH, 0, 1/h).
3. `postfx_mblurhq_velocity_pass1`: 3 x 3 neighbour maximum of [66] (texel step from `RENDER_TARGET_SIZE.zw`) into
   [67] = `SCENE_VELOCITY_PASS1`.
4. `postfx_mblurhq` over `RESOLVED_SCENE` + `SCENE_VELOCITY` (t4) + `SCENE_VELOCITY_PASS1` (t5) + `FLOATZ` (t6),
   constants `MOTION_BLUR_HQ_PARAMS`, `RENDER_TARGET_SIZE`, `ZNEAR`, up to 15 taps along the neighbourhood velocity,
   (from the shader: samples are weighted by their depth from `ZNEAR / floatz`, near over far).
   The fast path (`FUN_14063ca80`) runs the same two tile draws, then `postfx_mblurhq_fast_*` with
   `MOTION_BLUR_HQ_DECODE_SCALE_BIAS` = (-sqrt(w) ... ) i.e. (-sqrt(targetW), 2 sqrt(targetW), -sqrt(targetH),
   2 sqrt(targetH)) of the tile target (not driven).

Driven in `render_iw6::mblur` (default `BO2MP_IW6_MOTION_BLUR_MODE=velocity`; `old` / `camera` / `reprojection` keeps
`render_h1`'s camera-reprojection blur): passes 2 to 4 are the zones' own shaders; the velocity picture is ours.
- INFERRED: the world's velocity is made in one full-screen pass from `FLOATZ` and the previous camera (the same
  number `velocity_static` writes for static geometry: the pixel's clip position now against the same world point
  under last frame's matrices); sky pixels (FLOATZ 0) use the rotation only. Skinned models and the view model write
  no velocity of their own: pixels nearer than 24 units (the view model) are held still, so the gun does not smear
  with the camera. The tile target's rounding (ceil(size / 20)) is inferred (the exe's target sizes are not read).
- Not scaled by frame time (the exe has no such factor in the block above): at several hundred fps the per-frame
  displacement, hence the blur, is tiny, as it would be in the engine.
- An unmoved view draws nothing (byte-identical to the effect off, checked on lavapipe).

## 6. Depth of field, the other chains (read, not driven)

- `r_dof_hq` 1 / 2 (`FUN_14063b8f0`): quarter-size passes into targets [32..34] and [36]: `dof_hq_downsample` ([32]),
  `dof_hq_mask` ([33], `POSTFX_CONTROLD/E/F`), `dof_hq_mask_downsample` ([34]), `dof_hq_mask_blur` ([36], through
  the scaled draw helper). hq 2: one `dof_hq_coc_blur` draw into [40] then `dof_hq_final`; hq 1: three
  `dof_hq_coc_blur` draws ([40..42], each with one row of a 3 x float4 constant table in the exe's data) then
  `dof_hq_coc_blur_mip3` [39], `_mip2` [38], `_mip1` [37] (blur of the coarser result into the finer), then the final
  pass. Constants it sets: `DOF_EQUATION_*` as in 3, `POSTFX_CONTROL*` from the blur dvars scaled by `H / 480`
  (`blur = H/480 * view block 0x238 / 0x23c`, mask radius `H/720 * 12`), the near-COC and far-COC ramps.
  Not driven: the three-row constant table and the `POSTFX_CONTROL*` meanings are not read.
- Gaussian kernel (`FUN_140634490` -> `FUN_140634d60` -> `FUN_140634820` + `FUN_1406346e0`): sigma = `H * nearBlur / 480`
  over a scale, folded pair taps; translated as `render_iw6::dof::gaussian_pairs` and `normalise` (unit tested;
  weights and offsets of a pair, the half-texel shift by the parity of `round(size / factor)`, the 0.5 / sum
  normalisation, the 0.01 cut). Not wired: the engine's blur shaders take the taps as constants of a generated
  shader variant (`FUN_140633a50`), which the zones' `material_bloom_hq_blur` is not; the chain keeps the bloom's
  seven-tap blur (INFERRED) until the generated variants are read.
