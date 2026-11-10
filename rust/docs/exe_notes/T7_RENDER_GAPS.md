# BlackOps3.exe renderer: addresses behind the 2026-11-03 gap pass

Private notes for `docs/T7_NATIVE.md` "Graphical gaps (2026-11-03)". Exe:
the owner's unprotected `BlackOps3.exe` (Nov 2024 build), Ghidra 11.4.2
headless, project `/home/claude/bo3ref/gh` (copies `gh_d`, `gh_lit` when
`gh` is locked by another agent: `-readOnly` still takes the project lock).
Scripts: `/home/claude/bo3ref/scripts` (`Decomp.java addr|xref:addr`,
`Disasm.java out start end` from `docs/exe_notes/ghidra_scripts/`, and
`ReadF.java out addr...`, new: prints the u32/float/u64 at each address).

```sh
cd /home/claude/bo3ref && MAXMEM=2G ghidra_11.4.2_PUBLIC/support/analyzeHeadless gh_d bo3 \
  -process BlackOps3.exe -noanalysis -readOnly -max-cpu 1 -scriptPath scripts \
  -postScript Disasm.java /tmp/out.txt 0x14241b1b0 0x14241b340
```

## Renderer dvar names in the exe

BO3 registers dvars by hash, but these `r_` names are still in a name
table (0x1413e5290 for the LOD ones), so they name the features the
renderer has: `r_dof_*` (enable, near/far start/end/blur, view model
start/end, aperture override, tweak), `r_lodScaleRigid`, `r_lodBiasRigid`,
`r_modelLodBias`, `r_modelLodLimit`, `r_autoLodCullRadius`,
`r_umbraDistanceScale`, `r_texFilterQuality`, `r_textureMipLodBias`,
`r_spotShadowRes`, `r_omniShadowRes`, `r_actorShadowCount`,
`r_lightingShadowFiltering`, `r_lightingReflectionProbeMipDrop`,
`r_lightingSunShadowSSTMipDrop`, `r_lightingSunShadowDisableDynamicDraw`,
`r_volumetric_lighting_*`, `r_OIT`, `r_OIT_MaxEntries`, `r_sssblurEnable`,
`r_sunFlareTint`, `r_atmoBlurBA/HD/NS`, `r_filmLut`, `r_reviveFX_*`
(zombies), `r_waveWater*` (wave water simulation), `r_motionBlurMode`,
`r_motionBlurQuality`, `r_ssaoTechnique`, `r_aaTechnique`,
`r_sceneResolutionMultiplier`, `r_zfar`, `r_fog`.

## Model LOD selection (ported: `render_t7::lod`)

* Per-client LOD context, 0x30 bytes at `0x14a333ee0 + client*0x30`,
  filled by the frame function `0x141ddf580` (near `0x141ddf954`):
  +0x00 eye, +0x0c kind byte (view flags `& 0x38000000`: 0x8000000 -> 0,
  0x10000000 -> 1, 0x20000000 -> 2, else scene byte 0), +0x10 dvar
  `0x1486bf4b0` (hash 0x392adbe6, default 1, 1..4) times scene +0x1a6ad4,
  +0x14 dvar `0x1486bf500` (0xb0cb9cd7, default 0, -1000..0), +0x18 (not
  read by the pick), +0x1c scene +0x1a6bd4, +0x20 int dvar `0x1486bf060`
  (0xf7a1a581, default 0, 0..2), +0x24 dvar `0x1486bf4f8` (0x187bb1d0,
  default 1, 0..10000) or -1 (`0x142f88140`) when scene byte +0x1a6bd8 is
  set, +0x28 `sqrt(v)`, +0x2c `sqrt(v / dvar 0x1486bf1b0 + 1/pi)`
  (0x17de7f9, default 4, 1..100; 1/pi at `0x142f872f0`), with
  `v = float((h*0x38e38e390 >> 34) * (h >> 1)) * (1/scene+0x7c) * (1/scene+0x78)`
  (h: the u16 at render-target table +0x6cc2; `0x142f875ac` = 1.0).
  Registrations: `0x141d91af0` (float dvar register `0x1423a84f0`: ecx
  hash, xmm2 default, xmm3 min, [rsp+0x20] max; int `0x1423a7810`).
* Pick `0x14241b9b0(ctx, model, float r, float dist, float scale, int flags)`
  -> `0x14241b1b0(ctx, d', cull, r, sz, count, model+0x64)`; the rest of
  `0x14241b9b0` is the streaming fallback (nearest resident LOD, finer
  first) and residency counters.
* Callers: static models main view `0x141d72060` (records stride 0x98,
  flags 2, LOD byte stored per client), sun shadow `0x141d71b00` (flags
  4), spot shadow `0x141dafb90` / `0x141d71d90`, scene models
  `0x141d76350`, `0x141d76ef0`, `0x141d79080`, `0x141d79830`, `0x141d79d10`,
  `0x141e17000` (radius from model +0xf0 times scale).
* Model header: +0x40 LOD count, +0x44 size, +0x48 smallest radius, +0x4c
  size per kind, +0x58 radius per kind, +0x64 distances (largest first),
  +0x84 / +0x85 finest LOD for shadow / main view, +0x88 LOD pointers
  (coarsest first: checked against the meshes' triangle counts).

## Texture filtering (ported: `render_t7::sampler::material_quality`)

Sampler create `0x141d4a500`: the anisotropy table `[quality][level]`
(rows in `sampler.rs`), quality from the texture-filter dvar.

## Spot / omni shadows per view (ported: `render_t7::spot_shadow`)

* Light packer `0x141db6e10`: a light whose shadow state (`0x141db5a70`:
  light +0x84 unless the state struct's +0x7c) is not 1 is set aside in a
  0x60-word record (`+0x5a0 + i*0x180` of the packer's frame): words
  0..0x52 the intermediate (`0x141db8370` copies it), 0x53 its place in
  the list, 0x57 |origin - eye|^2, 0x5b the light at the eye
  (`0x141db6220`), 0x5c the light at the view box's corners
  (`0x141db6450`: |colour| x clamp(+0x54^2/d^2) x smoothstep near/far from
  intermediate words 0x11..0x14 along the axis, max of 8 corners), 0x5e
  the shadow state pointer.
* `0x141dbbae0` (called from the packer after the list): view box at view
  +0xb0d80..+0xb0d94 (min, max; filled in the frame function `0x141ddf580`
  from the visible cell list at `0x14b19c1f0`, entries 0xa0 bytes, bounds
  at +0x34, kept when the 12-byte entry at world +0x8a374 has bit 0x400),
  corners and centre `0x141db6ac0`. Candidates: priority (+0x170) > 0,
  type 2 or 4, not (+0x98 == 2 and +0x89); sorted `0x141dbdbd0`
  (priority descending, then +0x15c ascending); at most 4 (8 with the
  view's +0x400 +0x6230 flag). Spot: matrix from the record (+0x68).
  Omni: a copy (`0x141d7e0b0`) made type 2, +0x84 3, +0x94 1, back
  direction normalize(origin - box centre), +0x10c/+0x110 cos(pi/4)
  (`0x142f874a0`), +0x114 1, +0x118 0.1, +0x11c 1, +0x120 0.1, +0x124 1,
  +0x128 0, then `0x141d40070` (light matrix: `0x141d3ff20` pull-back,
  `0x141d3f7b0` view at origin + back * +0x12c, `0x142389660` rotation,
  `0x141d44520` perspective: m0 1/tx, m5 1/ty, m10 -n/(f-n), m11 1,
  m14 nf/(f-n), reversed depth).
* Fit and tile `0x141dbc4f0`: at most 16 shadow views (+0x6ae70); an
  optional view cull behind bool dvar `0x1486bf238` (not ported); the box
  corners through the matrix (spot: z <= w, divided), uv = (x/2+1/2,
  1/2-y/2): scale 1/max(range, 1.526e-5) (`0x142f87048`), offset
  centre - 0.5/scale, depth min/max of z/w (max 1 when fewer than 8 corners
  in front). Slot `0x141db41e0` (per layer a u64 of 8x8 cells; tier tables
  `0x142f29b10` masks {all, 0xffff, 0xf, 1}, `0x142f29b30` positions
  {1, 4, 16, 64}, `0x142f29b38` scale {1, .5, .25, .125}, `0x142f29b48`
  step {64, 16, 4, 1}; position to cell `0x141db3f40`, Morton), eviction
  `0x141db4c90` / `0x141db4ac0`. Tiers: enum dvars `0x1486bf2b8` (spot,
  default 3) and `0x1486bf280` (omni, default 2), registered in
  `0x141d91640`; view +0xb0d9c / +0xb0d98 override when not 4. The shadow
  view `0x141dec320`: frustum from the light (`0x141debbb0`) and the
  casters' matrix L x C, C rows (sx k, 0,0,0), (0, sy k, 0,0), (0,0,1,0),
  (((1-2ox) sx - 1) k, ((2oy-1) sy + 1) k, -min, k), k = max - min.
  Record: intermediate + word 9 slot, 0x3c..0x3f tile (scale, x, scale, y),
  0x49/0x4a scale, 0x4b/0x4c -offset, 0x4d -min, 0x4e max - min; packed
  by `0x141db0c90` (spot) / `0x141db0890` (type 1) after the light records
  (index = light count + j); the light's own record gets j+1 in intermediate
  word 5 (flags word bits 20..23, read for world pixels; bits 16..19 are
  the view model pixels' set, depth >= 0.99219).
* Lighting programs: 8-tap kernel when `LightingGlobals` +0x3a4 is set
  (bool dvar `0x1486bef70`, default 1, written in `0x141db8530`), offsets
  x word 13, cubed. Shadow raster state: the technique state's bias mode 3.

## Not ported (where to start)

* Static shadow slots (state 3: the 3000-state LRU per client at
  `0x14a33ba20`, cube omni `gOmniShadowmapArray`); their passes
  `0x141dac790`, `0x141dad0c0`, `0x141dcc390`, `0x141dd1db0`.
* Spot shadow caster traversal `0x141d71d90` (lists +0x53 / +0x9b, from
  `0x141dafde0`), LOD per caster `0x141dafb90` (here: the opaque
  G-buffer draws at the finest LOD).
* Lens flares: `0x14157ef90` init, `0x141582210` query, `0x141d65290`
  draw, element update `0x141578200` (list `0x14312d928`, counter
  `0x14312e928`, 256 a client). View list: `0x141ddf2a0` -> `0x141581850`
  (count view +0x1000 / +0x1414, order +0xe70, 0x34-byte sources at
  +0x1004). Backend entries (+0x510, 0x18) and emitter records
  (`gEmitterStatic`, uploaded 0x1900 bytes from +0xf9648 to +0xf9640) in
  `0x141d4dca0` via `0x141580490`; queries `0x1415824a0` (UAV slots 6 and 7:
  `0x149f93130`, `0x149f95a20`; the source's slot in draw state +0x830).
* DoF: chain only for DoF modes 1/2 (refdef +0x38 bits 16..17).
