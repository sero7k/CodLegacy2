# MWR (H1) world lit shader, as decompiled

Source: `ps lmo_sm_lmpb_l0d_l0sd_r0c0n0s0_nofog.hlsl_h2` (technique `opl_lmap_lmpb_nofog_r0c0n0s0_sm_ld1_h2`,
techset `wc_l_sm_lmpb_r0c0n0sd0_nfwpf_h2`, Rust). The chain was DXBC → vkd3d-compiler (SPIR-V) → spirv-cross (GLSL).
The dump tool is `bench/h1probe`, and it uses `Capture::shader_programs` and `Capture::techniques`.

Technique naming:
- `ln0`: no live light (the sun is in the bake).
- `ld0`: sun without a shadow map.
- `ld1`: sun with its shadow map.
- `ls*`: spot light. `lo*`: omni light.
- `nofog`: fog is applied in post.

## Inputs
- **t4, colour map.** `albedo = (tex.rgb * vcolor.rgb)^2` (linear, gamma 2).
- **t7, normal map.** Two channels; z is rebuilt as `sqrt(1 - x² - y²)`. Tangent space comes from v3 (normal), v4 (tangent, w = sign) and the cross product.
- **t8, specular map.** `spec = tex.rgb^2`; `gloss = tex.a`.
- **t3, lightmap.** Two halves: the top half (v × 0.5) gives `A.rgb`, `dir.x = A.a`; the bottom half (v × 0.5 + 0.5) gives `B.rgb`, `dir.y = B.a`. `dir.z = sqrt(1 - x² - y²)` (tangent space).
  - `irr = A + B * sat(dot(n_ts_norm, dir))`, where `n_ts_norm = normalize(nmap.y, nmap.x, 1)` (note the swapped xy).
  - **The values are used raw (linear HDR); there is no scale.**
- **t2, baked sun visibility** (at the lightmap uv, `.x`).
- **t5, sun shadow map** (cascades, cb2[10..21], PCF with a random rotation); t6 is an extra shadow term. Its weight fades with distance: `w = sat((cb2[21].x - dist) * cb2[21].y)`. `shadow = mix(baked_vis, shadowmap, w)`.
- **cb1[8].xyz** is the sun direction (toward the light), **cb1[9].xyz** the sun's diffuse colour (HDR), **cb1[10].xyz** the sun's specular colour.
- **cb3[0].y** is the fresnel max; **cb3[0].z** is the fresnel power.
- **t1, reflection probe cube.** LOD is `6 - 6 * gloss`.
- **t0, environment BRDF LUT.** It is looked up at (NdotV, gloss), mapped as `uv = (x * 0.984375 + 0.0078125, ...)`; `envspec = spec * lut.x + lut.y`.

## Sun specular
- `a = 2 / (2^(16 * gloss) + 2)`
- `D = a / (NdotH² * (a - 1) + 1)²`
- `Vis = 1 / ((NdotL + sqrt(NdotL² * (1 - a) + a)) * (NdotV + sqrt(NdotV² * (1 - a) + a)))`
- `sunspec = D * Vis * 1.0390625 * sunSpecColour * NdotL`
- `F = spec + (cb3.y - spec) * (1 - |V·H|)^cb3.z`

## Reflection
- `R = reflect(-V, N)`
- `relight = (A + B * sat(R_ts · dir)) * (2 - dot(R, vertexNormal))` (greyscale if `cb2[37].w`)
- `refl = relight * cube(R, lod) * envspec`

**The probe is relit by the lightmap in the reflection direction.**

## Output (linear HDR, before post)
`o = albedo * (1 - spec) * (irr + shadow * NdotL * sunDiffuse) + refl + shadow * F * sunspec`
