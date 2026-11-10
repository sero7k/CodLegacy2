# MWR (H1) post chain, as decompiled (code_post_gfx_mp)

The order is: lit world (linear HDR, no fog) → `postopaquefx_fog*` → volume light scatter → `tonemap_*` (optionally with `veil` = bloom) → `postfx_color2*` (film) → AA.

## Fog: `postopaquefx_fog_hf` (atmospheric, height falloff)
- Depth comes from floatz (t2). Values above 0.984 are sky: a 64x remap, sky distance `0.1`, else `cb[26].x / z`.
- `p = (ndc.xy * cb0.xz + cb0.yw) * dist` gives the view-space ray.
- **Height falloff:** `h = dot(p, cb16.xyz) * cb15.z`, with optical depth × `(1 - e^-h) / h` when `|h| > 0.01`.
- **Optical length:** `len * cb10.w + cb13.w`, times `cb15.w` (sky: `cb14.w`).
- **Extinction:** `T = exp(-optical * cb12.rgb)`, then `T = sat(T * cb12.w_or_16.w + 1 - ...)`, i.e. a max-opacity blend.
- **Phases:**
  - Rayleigh: `(1 + mu²) * 3 / (16π)` × `cb10.rgb` (haze colour);
  - Mie (Henyey-Greenstein-like, `g = cb11.w`): `(1 - g²) / (4π (1 + g·mu)²)` × `cb11.rgb` (sun fog colour);
  - `mu = dot(viewdir, cb14.xyz)` (sun direction).
  - The sum is multiplied by `cb13.rgb` (in-scatter strength).
- **Output:** `scene * T + inscatter * (1 - T)`, all in linear HDR.
- The inputs map to `<map>_fog_hdr.gsc`'s `atmosFog*` fields: haze colour/strength/spread, sun fog colour, extinction strength, in-scatter strength, half-plane distance, start distance, sky distance and sun direction.

## Volume light scatter apply
`colour = scatter.x * (1 - sat(d² * cb35.w + d * cb34.w)) * cb35.rgb²`, where `d` is the screen distance to the sun at `cb34.xy`. The colour comes from the vision's `r_volumeLightScatterColor`.

## Tonemap: `tonemap_filmic`
- `x` is the scene (with exposure already in the curve).
- Below `cb4[0].x`, the toe: `(x * c2.x + c2.z) / (x * c2.y + c2.w)`.
- Above it, the shoulder, with `cb4[1]` in the same form.
- Then `sat`, then `pow(cb4[3].z)`.
- `veil`: `scene * cb28.y + veil_blur * cb28.x` first.
- **These coefficients are computed by the game's code from dvars and are not in the map files.** They need a frame capture or the exe's defaults.

## Film: `postfx_color2` (display space)
1. `c = srgb_encode(sat(x))`; `L = dot(c, rec601)`.
2. `c = mix(c, L, cb1.w * L + cb0.w)` (desaturation by luminance).
3. `c *= cb0.rgb + cb1.rgb * L + cb2.rgb * L²` (a tint curve through the dark, medium and light tints).
4. `c += cb3.rgb`.

`_clut` adds a colour LUT after this.
