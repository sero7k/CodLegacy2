# MWR (H1) post chain, as decompiled from code_post_gfx_mp

Order: lit scene (HDR, no fog) → `postopaquefx_fog*` → volume light scatter → `tonemap_*` → `postfx_color*` (film, LDR) → AA.

## Fog: `postopaquefx_fog_hf` (cb2)
1. **Distance and density.** The distance comes from floatz (t2). Sky pixels (z > 0.984) use the sky settings (cb16.w, 0.1).
   - `optical = (dist * cb10.w + cb13.w) * cb15.w`
   - Height falloff along the ray: `h = dot(ray, cb16.xyz) * cb15.z`; `optical *= (1 - e^-h) / h`.
2. **Transmittance.** `tr = clamp(1 + maxOp * (exp(-optical * cb12.xyz) - 1), 0, 1)`, with `maxOp = cb12.w`.
3. **In-scatter.** With `mu = dot(sunDir cb14.xyz, viewDir)` and `g = cb11.w`:
   - `ins = (1 - tr) * cb13.xyz * (Rayleigh(mu) * cb10.xyz + Mie_HG(mu, g) * cb11.xyz)`
   - `Rayleigh = 0.0597 * (1 + mu²)`
   - `Mie = (1 - g²) / (4π * (1 + g*mu)²)` (as written; Schlick-like)
4. **Output.** `out = scene * tr + ins`.

The constants come from the map's `_fog_hdr.gsc` `atmosFog*` fields; the mapping is done in code. To be confirmed against a frame capture.

## Tone map: `tonemap_filmic` (cb4)
- `y = x <= cb4[0].x ? (x*T.x + T.z) / (x*T.y + T.w) : (x*S.x + S.z) / (x*S.y + S.w)`, where `T = cb4[2]` (toe) and `S = cb4[1]` (shoulder).
- Then `y = saturate(y)^cb4[3].z`.
- Exposure is folded into the coefficients by code (not in the data).
- Veil (bloom): `in = veil * cb2[28].x + scene * cb2[28].y`.

## Film: `postfx_color2` (cb2[0..3])
1. `c` = sRGB encode of the tone-mapped linear colour.
2. `L = dot(c, (0.299, 0.587, 0.114))`.
3. `c = mix(c, L, cb1.w * L + cb0.w)` (desaturation by brightness).
4. `c = c * (cb0.xyz + cb1.xyz * L + cb2.xyz * L²) + cb3.xyz` (tint: a quadratic through the dark, medium and light tints; offset).

The output is sRGB. The constants come from the vision `r_film*` fields; the mapping is done in code.

## Volume light scatter apply
`out = scatter_tex.x * cb35.xyz * (1 - sat(|d| * cb34.w + |d|² * cb35.w)) * cb35.xyz`, where `d` is the screen distance to the sun, `cb34.xy`.

## Unknowns
The code that turns the vision and fog settings into these constants (exposure, curve, tints) has not been read yet. Plan: capture the constants from the real game, then fit and verify the mapping.
