# MWR veil (bloom) in h1_mp64_zt.exe, read 3 Nov 2026

Addresses (Ghidra image base 0x140000000):
- 1400831e0 downsample leg; 140083aa0 upsample leg; 140083a10 runs the upsample and, when view+0x1e78 is set, 14062ac80 (an extra blur of level 0x53 with radius view+0x1e7c), then copies the tone-map state (view+0x1e70/74/80/84) into PERCEPTUAL_PARMS.
- 140628da0 the post chain: 1400831e0, 1400855a0 (exposure), 140083a10, then the tone map/colour pass 140084a20.
- 1400836c0 first gaussian pass (writes VEIL_PARAMS); 140083530 second pass; 140083030 plain bilinear (+add); 140083110 first pass (material by kind).
- 1400838c0 builds the weight table (1415bc8a0, six floats, normalised) and step table (1415bc8b8, six floats) from the dvars.
- 14007f130 registers the veil dvars by hash: weights 8f9c12f9 (3) + 22f4d58a (3); radius c146ddf8; steps 4c20e779 (3) + df79aa0a (3); mode 3e613f80; gaussian size a386ef0d; first-pass kind 60e80aec; 5cf42335, 8d8b11d3 floats; 17c4711c, 15ce2988 bools.
- Material table at 140887d50 (14fe6f060 on): linear, log, karis, bilinear, bilinear_add, gaussian low/med/high, add low/med/high.
- 14062bcc0: step from radius: y = height*r/480; x = y / (height*pixelAspect/width).
- Enum strings: 140813298 Upscale, 1408132a0 Downscale, 1408132b0 Hybrid Fast, 1408132c0 Hybrid Reference; Low/Med/High at 140813238; Linear/Log/Karis at 1408132d4/dc/e0.

## Other post passes: inventory (read 3 Nov, strings only)

- **SSAO / hemispheric AO is a compute pipeline.** Targets `$ssao`, `$ssao_mip1..5`, `$ssao_blurred`, `$ssao_blurred_mip1..5`, `$ssao_floatz_mip1..5`, `$ssao_blurred_w1h0|w2h1|w3h2`. The shader names are `cs_hemi_ao_*` (105 of them: `blur_upsample`, `_lower`, `_lower_pre`, `_lower_mul`, each with `ssao`, `ssao_mdao` and the fog variants `_fog`, `_fog_hf`, `_fog_hf_fsf`, `_fog_fsf`, `_dfog`, `_dfog_hf`). The result is read by the `postopaquefx_ssaolq_*` / `postopaquefx_ssaohq_*` materials (already named in `look::post_opaque_technique`). `dxbc_sm5` has no compute stage, so SSAO cannot run until compute support is added to the translator (same blocker BO3 had). Light-set keys: `ssaomode`, `ssaostrength`, `ssaopower`, `ssaominstrengthdepth`, `ssaomaxstrengthdepth`, `ssaoblurradius`, `ssaowidth`, `ssaogapfalloff`, `ssaogradientfalloff`, `ssaofadedepth`, `ssaorejectdepth` (each with a `_time` blend key).
- **Depth of field** is ordinary full-screen pixel passes, so it can use `Post::record`: targets `$dof_floatz`, `$dof_half_prepass`, `$dof_half_color[_pingpong]`, `$dof_half_alpha[_pingpong]`, `$dof_tile0[_pingpong[_half]]`, `$dof_tile1`; materials `dof_depthhack`, `dof_downsample`, `dof_near_coc`, `dof_prepass`, `dof_median`, `dof_tile_downsample_0|1`, `dof_tile_downsample_halfres_0|1`, `dof_tile_neighbor`, `dof_upsample`, `dof_circular_low|med|high`. Constants `DOF_*` are slots 147..154. Not read yet: the order and the writers of the `DOF_*` constants.
- **Motion blur** needs `SCENE_VELOCITY` (+ `_TILE0/1`) and `MOTION_MATRIX_X/Y` (slots 156/157); the model draws have a "motionblur" surface path. Not read yet.
