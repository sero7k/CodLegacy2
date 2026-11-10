# Black Ops III, natively: what the game itself does (renderer, lighting, collision, map logic)

Goal (owner, 22 Oct): read and draw every BO3-built map the way BO3's own
code does it — same passes in the same order, same buffers and layouts,
same constants computed by the same formulas, same shaders from the zones,
same map/lighting/collision/logic reads. No look-alikes: what isn't done
yet is left out and listed at the end.

Reference: the owner's unprotected `BlackOps3.exe` (Nov 2024 build, PDB path
`d:\workspace\outrun\t7\game\pc\BlackOps3.pdb`, no PDB). Analysed with Ghidra
11.4.2 headless in the cloud box; addresses and decompiles are kept out of
the repo (private notes). Shaders are read from the installed game's zones.

## Tooling

* Ghidra headless project of the exe, plus small scripts: decompile a
  function, list cross-references, find functions by the displacements they
  store to (`FindStores`), by index patterns into vec4 arrays
  (`FindPattern`), and calls taking a given constant argument (`ArgCalls`).
* `fastfile_t7` examples: `t7_dxbc <zone> <out> <name>` dumps a zone's shader
  programs; **`t7_rdef <file.dxbc>...`** (new) prints each program's constant
  buffers with every variable's offset/size and whether the shader reads it,
  and its texture slots.

## Where the shaders are

| zone | holds |
|---|---|
| `core_patch.ff` | **all code shaders**: `deferred_lighting_permuted(0..63)` (+ `_bilateral`, `_ssr`), `deferred_permute_classify/clear/draw/uber_resolve(+gtao, volumetrics)`, `exposure_compute`, `create_lut2dv`, `bloom/*` (downsample_ev, l0/l1/lx, blur mip x/y l1–l5, apply_lut + lens distortion variants), `resample_final(_bm, _bm_1886, _bm_null)`, `ssao_gtao`/denoiser, `volumetric_light*`, `smaa_*`, `fxaa_hemi_aa_*`, `dof/*`, `oit_apply*`, `reflectionprobe_compute`, `diffuseprobe_compute`, `light_culling_*`, `motion_vector_static`, `fx_compute_sprite_*` (4170 programs) |
| `core_common.ff` | material programs: `gbuffer_lit`, `gbuffer_micro_tile_blend`, `gbuffer_decal*`, `gbuffer_vegetation`, `gbuffer_cloth`, `gbuffer_weapon_camo`, `lit*` (forward: glass, hair, eye, skin, emissive), effects |
| `core_gfx.ff` | `techsetdef_*` (2d, resolve, depth_clear, shadowoverlay, prepass, buildshadowmap, cinematic, lensflare), UI effects |
| map zone (`mp_*.ff`) | the map's own technique sets (its materials' G-buffer programs) |

The exe keeps the code techniques in a table of `{name, global slot}` (244
entries; e.g. `bloom_apply_lut`, `resample_final`, `bloom_downsample_ev`),
each slot filled when `core_patch` loads; the draw code reads the slot.

## Engine constant names and indices

A 240-entry table `{name, index}` names every engine ("code") constant. The
ones the lighting and post passes read (index in brackets): `fogConsts`..
`fogConsts9` [23–31], `fogColor` [32], `sunFog` [33], `sunFogDir` [34],
`sunFogColor` [35], `colorMatrixR/G/B` [17–19], `colorBias/TintBase/
TintDelta` [36–38], `skyMxR/G/B` [45–47], `sunMxR/G/B` [48–50],
`postFxControl0..F` [51–66], `postFxConst00..63` [67–130], `postFxBloom00..25`
[131–156], `hdrControl0/1` [169–170], `relHDRExposure` [7].

## Constant buffers (layouts from the shaders' RDEF; the engine fills them)

**`PerSceneConsts` (1488 bytes):** projection/view/viewProjection and their
inverses (0x000–0x17f), eyeOffset 0x180, adsZScale 0x190, **hdrControl0
0x1a0, hdrControl1 0x1b0**, fogColor 0x1c0, fogConsts..fogConsts9
0x1d0–0x250, sunFogDir 0x260, sunFogColor 0x270, sunFog 0x280, zNear 0x290,
cloth tints 0x2a0/0x2b0, renderTargetSize 0x2c0, upscaledTargetSize 0x2d0,
materialColor 0x2e0, camera up/look/side/velocity 0x2f0–0x320, **skyMxR/G/B
0x330–0x350, sunMxR/G/B 0x360–0x380**, skyRotationTransition 0x390, debug
overrides 0x3a0–0x400, emblemLUTSelector 0x410, colorMatrixR/G/B
0x420–0x440, gameTime 0x450, gameTick 0x460, subpixelOffset 0x470,
viewportDimensions 0x480, viewSpaceScaleBias 0x490, ui3dUVSetup0–5
0x4a0–0x4f0, clipSpaceLookupScale/Offset 0x500/0x510, computeSpriteControl
0x520, invBc/MaskTexSizes 0x530/0x540, **relHDRExposure 0x550**,
triDensity 0x560/0x570, voldecalRevealTextureInfo 0x580, extraClipPlane0/1
0x590/0x5a0, shaderDebug 0x5b0, isDepthHack 0x5c0.

**`PostFxCBuffer` (2016 bytes):** postFxControl0..F 0x000–0x0f0,
postFxConst00..63 0x100–0x4f0, postFxBloom00..25 0x500–0x690, filterTap
0x6a0 (128), postfx view/proj/viewProj matrices 0x720–0x7d0. Readers:
`create_lut2dv` reads postFxControlE and postFxConst00.. (the colour grade
is baked into a LUT each frame); `bloom_apply_lut` reads postFxControl0–2;
`bloom_downsample_ev` reads postFxBloom25; `exposure_compute` reads
postFxControl0/1 and writes `exposureAdaptionBuffer` (eye adaptation runs
on the GPU).

**`LightingGlobals` (944 bytes), read by `deferred_lighting_permuted`:**
numLights 0x000, numRefProbes 0x004, (0x008: a count the filler writes),
**fogConstants 0x030 (224)**, **sunConstants 0x110 (256)**,
**globalProbeConstants 0x230 (48)**, 0x330/0x334 counts, viewID 0x340,
debug fields 0x344–0x37c, permuteStride 0x390, numOverrideProbes 0x3a0,
enableDitheredShadow 0x3a4 (+0x3a8 a shadow quality tap count, 8/9/16 by
setting). Also: `LightingPackCBuffer` (65520: the visible lights, packed
224 bytes each by the light packer), `RefProbePackCBuffer` (61152),
`RefProbeBlendPackCBuffer` (65472), `gPermutedTilingBuffer`,
`gDeferredCullBits`, `gSunShadowTree`.

Lighting shader textures: t0 GBufferDiffuseColor, t1 GBufferNormalAndGloss,
t2 GBufferSpecularColorAndOcclusion, t4 GBufferDepth, t5 gEnvBRDFGeneric,
t8 gReflectionProbeArray, t9 gSpotShadowmapArray, t10 gOmniShadowmapArray,
t13 gSunShadowmapArray, **t17/t18/t19 gProbeX/Y/ZArray** (the sun volume's
three probe-volume images), t20 gCookieArray, t27 gSSAOTexture, t31
gTransShadowmapArray.

## The LightingGlobals filler (translated from the exe)

One function builds the whole buffer each view:

* **lights:** each visible light is packed (224 bytes) into the light pack,
  a bit per light into the cull bitfield every 32 lights; counts to 0x004,
  0x008, 0x330, 0x334; probes over the override count to 0x3a0.
* **fogConstants (0x030–0x10f)** are copied from the view's fog config
  (fields 0x170–0x23c of that struct) with a fixed shuffle: 0x200/0x208 →
  0x30/0x38, 0x230–0x23c → 0x40–0x4c, 0x170..0x180 → 0x50..0x68, 0x220–0x228
  → 0x74–0x7c, 0x210/0x214 → 0x80/0x84, 0x190–0x1bc → 0xa0–0xa8, 0x8c,
  0xb0–0xb8, 0x98, 0xc0–0xc8, 0x9c, 0x1c0–0x1c8 → 0xd0–0xd8; 0x1cc..0x1d8 and
  0x1ec/0x1f0 each written twice (x and y) into 0xe8..0x100, 0xe0, 0x90;
  0x1dc → 0x88, 0x1e0 → 0xcc, 0x1e4 → 0x54, 0x1e8 → 0x64, 0x1f4 → 0x70,
  0x1f8 → 0xac, 0x1fc → 0xbc; 0xdc = 0.
* **sunConstants (0x110–0x20f, 64 floats)** come from a sun builder taking the
  sun config and the sun shadow setup: floats 0–2 the sun direction (config
  0x14–0x1c), 3 a shadow value, 4–6 the sun colour (config 0x00–0x08), 7 =
  colour scale × config 0x10, 8–11 the global probe's config words 0–3 (probe units, tint rgb; NOT shadow state, see "Debug views" below; left zero they blacked all indirect light),
  12–23 the three cascades' depth bands (`centre ± 32·size·k·range`,
  `1/(…)`, fade), 24/25/26 config 0x50/0x40/0x64, 27 a flag (shadows off),
  28–35 the 2×4 cloud-shadow transform (rotation `sin/cos(angle)·scale` from
  config 0x44.., offset `t·speed + origin`), 36 config 0x0c, 37 shadow 0x228,
  40–43 shadow 0x2f0–0x2fc, 44–59 the shadow lookup matrix, 61 the cascade's
  bias, 62 a split constant (three values by cascade count), 63 passed
  through. (Exact field meanings of the config are being mapped to the
  map's sun volume data.)
* **globalProbeConstants (0x210–0x22c):** the probe volume grid's
  `origin / size` (xy), sizes, `0.5/count` (xy), `range/(k·scale)`, a scale.
* **0x230–0x25c:** the probe volume's box relative to the view origin
  (`min − eye`), `1/extent`, `0.5/size` (yz) and its integer dimensions
  (from the sun volume's probe-volume record).
* 0x374/0x378: the render size in 64-pixel tiles (`(w+63)>>6`, `(h+63)>>6`).

## Where the engine keeps code constants, and what it puts there

The draw state holds the 240 code constants as vec4s at `+0x400 +
16·index`, each with a 16-bit change counter at `+0x20c0 + 2·index`
(bumped on every write; the cbuffer is rebuilt from them). Found so far:

* **relHDRExposure** (7): `(1, 1, 0, 0)` at the start of every frame; one
  draw-list mode switches it to `(1, 1, 0, 1)` for its draws. Nothing else
  writes it, so the deferred lighting runs with `(1, 1, 0, 0)`.
* **hdrControl0** (169): `(1, 0, 1, 1)`, reset at the start of every draw
  list.
* **exposure_compute** inputs (from the view's post-FX settings `S`):
  `postFxControl0 = (S[0x1d54], S[0x1d58], S[0x1d50], S[0x1d5c])`,
  `postFxControl1 = (S[0x1d44], S[0x1d4c], S[0x1d60] ? 1 : 0, view index + 1)`;
  it reads/writes two RGBA16F textures (the exposure history) and binds the
  scene's luminance target.
* **create_lut2dv** inputs: `postFxConst00..59` are four blocks of 15 vec4s
  copied straight from `S + 0x18cc + 0x110·k` (k = 0..3); `postFxConst60`
  = `S[0x1d0c..0x1d18]`; `postFxControlD..F` and further constants are
  computed from `S + 0x24..0xa8` (colour balance, contrast, the Rec.709
  luminance weights 0.2126/0.7152/0.0722 in one branch). Each 15-vec4 block
  is exactly a vision struct (0xf0 bytes): the colour grade is computed on
  the GPU from up to four visions' raw fields. Vision files are rawfiles
  `vision/<name>.vision` (`key "values"` lines; the exe's 36-field table:
  `vkTT` colour temperature, `vkTS`, `vkTC` tint, `vkTO`, five bands
  `vkRGB0..4`/`vkL0..4`/`vkM0..4` (no `vkM2`), `vkRM`, `r_filmLut`, revive-FX
  edge fields, `snd_*` filters, `r_sunFlareTint`, `r_atmoBlur*`), parsed by
  `fastfile_t7::native::parse_vision`; `vision/default.vision` is the
  fallback. 30 generic visions are in `core_common.ff` (`t7_vision`). The post-FX
  settings `S` are filled at view setup from the map's own post-effect
  data, one function per effect, in this order: vukko (`GfxWorld` 0x1610),
  bloom (0x1b40), LUT (0x1b70..0x1ba8), **colour vision (0x1bb0..0x2018)**,
  underwater (0x2020).

### The colour-vision blend (found; `fastfile_t7::native::vision_slots`)

The four slots and weights come from the map, not from vision files:
`GfxWorld + 0x1bb0` holds one base vision block per lighting state
(0x110 each: the 0xf0 vision struct + 0x20), followed by the modifier list
(count 0x1ff0, 336-byte records, pointer 0x1ff8) and two box lists (counts
0x2000 and 0x2010, 688-byte records). The mask is `1 << (4 + lighting
state)` | the client's extra bits. Modifiers whose flags contain the mask
weigh `smoothstep(clamp(dist·scale + bias))` (× a cone term, flag bit 8;
× a view-direction term, bit 9); their sum is capped to 1 and each slot
gets `w·strength` (vision at +0x40). Boxes (six planes, per-plane falloff
at 0x60, strength 0x78, mask 0x7c, vision at 0x90) weigh
`smoothstep(Π clamp(plane·eye × falloff))` and fill what is left of 1; the
remainder goes to the base vision of the current lighting state; the
weights are normalised. With the camera in no box and near no modifier the
grade is the map's base vision for its lighting state, alone, weight 1.
`vision/*.vision` rawfiles are not what MP maps grade with.

### Fog (sources found; the evaluation is in the exe, not yet ported)

The fog config (draw state 0x170..0x23c, which the LightingGlobals filler
shuffles into `fogConstants`) is written each view by one function from
the client's evaluated fog bank (0x1b0 bytes) at the camera position:
`fog = eval(bank, eye)` (an exponential height fog: density, height
falloff, start distance, colours, sun-scatter cone `1/(cos a2 − cos a1)`,
lit-fog terms). The bank comes from the map: with no fog volume selected,
`GfxWorld + 0x44` (and 0x40); else fog volume `i` (`GfxWorld + 0x150`,
624-byte records) bank `k` at `+0x30 + 0x90·k` (and +0x20), with the
client's transition blend. Ported field by field in
`fastfile_t7::fog` (with the bank transitions, below):

* `bank`: the first fog volume (in list order) whose lighting-state mask
  (word 10) has the state's bit, whose bank mask (word 11) meets the
  client's lit-fog bank mask (1 by default), and whose planes all have the
  eye behind them; its bank `k` (lowest set bit of the mask) is read with
  float 10 skipped (`GfxConfig_FogBank` 0x90 → `GfxConfig_Fog` 0x8c). No such
  volume: the world default `GfxConfig_Fog` at `GfxWorld + 0x44`.
* `modifier_scales`: the first fog modifier volume (`GfxWorldFogModifierVolume`
  0x30 — the walker read these as 44 bytes; fixed: the exe's size table and
  its code's stride say 0x30; flags word: bank mask in bits 0..3, plane
  count − 1 in 8..15, first plane in 16..31; planes at `GfxWorld + 0x180`)
  the eye is inside gives the distance scale (+0x18), +0x1c and the r/g/b/a
  colour scales (+0x20..0x2c); none: all 1.
* `client_fog`: colours `× intensity × colour scale`, `1/max(1, ·)` of the
  distance terms, sun-scatter direction from the bank's angles plus the
  current lighting state's sun pitch/yaw, the rest copied; word 9 = 1.
* `evaluate`: the exponential height fog (`2^`, `ln 2`, `0.333` grey,
  `0.99999`, `1e-4`/`1e-7` floors) exactly as the exe, both the plain and
  the lit-fog branch.
* `config` and `fog_constants`: the two fixed shuffles into the draw state
  (0x170..0x23c) and `LightingGlobals.fogConstants`.

**Transitions (ported: `fastfile_t7::fog::FogTransitions`).** The client
keeps, per local client, the selected fog volume and bank, the time the
blend started, the bank it blends from and last frame's blended bank. Each
frame: a *cut* is the client's own flag or the eye moving over 1024 units
since the last frame (squared distance > 1048576). On a cut the target bank
is taken as it is. Otherwise, when the selection changes (another volume,
another bank, or none = the world default), the blend restarts from
wherever it is (`from = current`, start = now); then `current = from +
(target − from) · s`, `s = smoothstep((now − start) · rate)` with the exe's
`(3 − 2t)t²` (0 before, 1 after), rate the volume's float at +0x20 (the
world's at +0x40 for the default; 0 = no blend). Modifier volumes do the
same with their own rate (+0x14) and the six scales (+0x18..0x2c), blending
to all ones when the eye leaves every modifier volume. A world with no fog
volumes runs the script fog instead: a linear blend between two banks over
a start/end time set by script (`script_fog`).
Vision: the renderer's vision blend has no time state; its transitions are
the box/modifier volumes' distance smoothsteps (`native::vision_slots`,
already exact). Time-based vision changes are script features (they set the
client's vision mask, which the blend reads).

`specularLighting` (code texture 0x34): no store anywhere in the exe binds
it directly (its record at draw state 0x1a10 is never written by constant
offset); the permuted lighting writes the scene target (0xa). Treated as
unbound.

### Lit fog and volumetric light (found 2026-10-26; `render_t7::volumetric`)

Black Ops III's "volumetrics" are its **lit fog volumes**, not the height
fog above: `GfxWorld` +0x5a8 count, +0x5b0 records of 0x168 (`GfxLitFogVolume`:
plane start and count, box min/max, three floats at 0x20..0x28, a flag word
at 0x2c, a blend time at 0x34, four banks of 19 floats at 0x38 + 0x4c·k),
planes at +0x5c0. Flag word: bits 0..3 the banks a volume has, 5 sun-lit
(0x20), 6 lit by local lights (0x40), 7 (0x80), 8 lit with no sun (0x100,
set at pack time for a sun-lit volume with words 8..10 non-zero when the
view has no sun), 9 off, 12+s lit in lighting state s; the high half a bank
index the packer clamps to 0..255 and then drops.

* **Client state** (0xf4 bytes per volume, 64 per client): at map load all
  three copies (from, to, current) are bank 0. A script call selects bank k
  for a mask of volumes over a time (negative: the volume's +0x34 seconds);
  each frame `s = (3 − 2t)t²` eases nine of the 19 floats (0..2, 7..10, 12,
  13) and the rest jump at the end. Nothing in our sim calls it, so maps
  run bank 0, as a match does until a script changes it.
* **Per view** (six ring buffers of 64 × 0xd0 bytes): a volume is packed
  when its flags (after the no-sun change; the sun bit cleared when the view
  has no sun: shadow mode 0 or intensity ≤ 0) have the lighting state's bit,
  not bit 9, one of 0x1e0, and the bit of the bank it shows; the frustum
  does not cull its box (farthest corner along each plane ≤ 0 culls); and
  the eye is within 8000 units of all six planes (its own six when each
  opposite pair cancels, sums of the components ≤ 0.001; else its box). At
  most 64. The record (0xd0): box min/max − eye, six planes as
  (clamped normal, `n·eye − d` clamped to ±65504) in the order 0, 1, 2, 4,
  3, 5, then `1/c8`, `1/c10` (1 below 1), `eye.z − c9` (±32767),
  `c7` (0..65535), `1.55k − 0.55k³` of `k = c12` (±0.99), `c13` (0..1),
  the flag word (sun 1, lights 2, 0x80 → 4, no-sun 0x10),
  `−log2(clamp(normalize(c0..2)/2, 1e-8, 1))·(1 − c13)`, words 8..10
  clamped to 6.1e-5..65504, `lerp(sun colour, c3..5 × sun intensity, c6)`,
  c14..18, one zero word. The list keeps the count and the OR of the flag
  words (count 0 when the OR is 0).
* **Dvars** (hashed names; defaults): the view flag 1 (copied into the scene
  each frame), the pass 1, mode 0 (`2DRayMarch`; 1 `3DVolume`, 2
  `2DRayMarchUnified`), samples 40 and steps 8 (`postFxControl4.w`), the
  blur 10 / 0.01 / 2000 (`postFxControlA`), half-resolution apply 0, the
  `_skip_samples` programs 1 and 1.
* **Passes** (after the transparent draw lists, before the effects lists):
  `downsample_depth_4x_compute` (floatZ → render target 0x26, quarter
  R32F; `postFxConst00 = (1/w, 1/h, w25, h25)`), then into render target
  0x28 (quarter RGBA16F) `volumetric_light_sun_skip_samples` when the flags
  have 1|4|0x10 and `volumetric_light_skip_samples` when they have 2
  (`_unified` in mode 2), a horizontal and vertical blur
  (`volumetric_light_hblur/vblur_compute` through 0x29), and
  `volumetric_light_apply` (or the upsample and half-resolution apply).
  The march reads the quarter depth (0x25), `volLightTexture` (the 3D
  `volumetricLightVolume`, R11G11B10, 64 slices), the OIT targets
  (0x2a/0x2b full RGBA8, 0x2c R8, 0x27 quarter R8_UINT), the sun shadow
  array, the cookie array and `gLitFogVolumes` (code buffer 0xc).
  Constants (`postFxControl0..7`, `A`): the inverse view matrix, `1/P00`,
  `1/P11`, count | flags << 16 and samples | steps << 16 as raw words, the
  eye and the scene-to-0x28 width ratio, the view matrix's third column,
  `(4, 0, 0, 0)`, the blur dvars. Render target sizes from the exe's size
  classes (class 2/9: `(w + 3) >> 2`).

Built: the bank states, the packer, the pass choice and the constants
(`cargo test -p render_t7 --lib volumetric`; the offline check
`T7_VOLUMETRIC_ZONE=<map.ff> ... -- --ignored` packs mp_sector's three and
mp_apartments' two volumes; Nuketown's one volume has no lighting-state bit,
so the exe draws no volumetrics there). Log lines: `t7 volumetrics: N lit
fog volumes in the map`, then per view `t7 volumetrics: K lit fog volumes
packed ... the exe's passes: ...`. The GPU passes are in
`render_t7::volumetric_pass` (below). Found for it: before the opaque draw lists a depth pre-pass (a code compute, run
when the list is not empty or the sun-shadow volumetric dvar is on) reads
the scene depth and writes render targets 0x25 (quarter R32F, the march's
`downsampledDepth`) and 0x27 (quarter R8_UINT, `downSampledDepthUV`) as
UAVs, with `postFxConst00 = (1/w, 1/h, w25, h25)`; with the local-light
flag it then culls lights and probes for the march (the `volumetric_*_cull`
programs). `volLightTexture` is the 3D `volumetricLightVolume` (quarter of
render target 0x29's size, 64 slices, R11G11B10), created when the dvars
are on and written only by the 3D mode, so in the default mode the march
samples a never-written (zero) volume. Still to read: the OIT targets'
contents when no OIT is drawn (0x2a/0x2b/0x2c) and the apply's bindings
and blend into the scene.

**The GPU passes (built 2026-10-26, `render_t7::volumetric_pass`; on by
default since 2026-10-27, `T7_VOLUMETRIC_GPU=0` turns them off; checked
offline, below).** Read
from the exe's recorders (march, blur driver, apply, pre-pass) and the
techniques' arguments (`t7_post_wgsl` now prints them). The code-texture
slots a recorder fills sit at a fixed stride, so each write names its code
texture; with the view flag on:

* **Pre-pass** (before list 14): `downsample_depth_with_uv_4x_compute`
  (the unnamed second compute shader beside `downsample_depth_4x_compute`):
  `floatZ` → UAV 0 render target 0x25 (R32F), UAV 1 0x27 (the chosen texel
  of each 4x4 block, `y << 2 | x`), `postFxControl7 = (4, 0, 0, 0)`,
  `postFxConst00 = (1/w, 1/h, w/4, h/4)`, dispatched in quarter pixels.
  0x27 is R8_UINT in the exe and R32_UINT here (no 8-bit storage format in
  wgpu; same values).
* **March** (after list 20 and the effects, before 21; `141d4ced0`): the
  4x depth downsample into 0x26; 0x28 cleared to zero; the full-screen code
  draw of `volumetric_light_sun_skip_samples` into 0x28 when the flags have
  1, 4 or 0x10 (`_sun` without the skip dvar). Its pixel program reads
  `downsampledDepth` = 0x25 (code texture 0xe), `volLightTexture` (0xb),
  `adaptiveTransparencyDepth/Trans` (0x1d/0x1e) = 0x2a/0x2b,
  `downSampledDepthUV` (0x1f) = 0x27, `oitPixelAlpha` (0x20) = the view's
  OIT alpha target, the sun shadow array (0x3, sampled through the
  renderer's `ShadowSamplerComparisonState` at s13, which no material
  argument binds), the cookie array (0x7), `gLitFogVolumes` (code buffer
  0xc, 52 words a record), `LightingGlobals` (b2), `PostFxCBuffer`
  (`postFxControl0..7`, `A`) and `PerSceneConsts` (`zNear`,
  `renderTargetSize` of the quarter target). The OIT inputs: when
  `oitPixelAlpha > 0.95` the program takes depth 2000 and transmittance 1
  whatever 0x2a/0x2b hold; with the view flag off the exe binds its own
  1x1 images there. With no OIT drawn here they are bound white (alpha 1,
  so transmittance 1): not read from the exe's frame, the program's own
  no-transparency path. `volLightTexture` is bound as one zero texel (the
  default mode never writes the volume, so it samples zero anywhere).
  Then, with flags 1|2|4, `volumetric_light_hblur_compute` (0x28 → 0x29)
  and `volumetric_light_vblur_compute` (0x29 → 0x28), both reading 0x26 as
  `codeTexture0` and `postFxControlA` = (10, 0.01, 2000, 0), dispatched in
  0x28's pixels.
* **Apply** (`141d4d3c0`): the scene target copied into `feedback` (code
  texture 8), then `volumetric_light_apply` drawn over the scene:
  `volumeLightTexture` = 0x28 (0xd), `downsampledAlphaDepth` = 0x26 (0x1a),
  `floatAlphaZSampler` = the scene depth (0x1b), `blendTexture` =
  `feedback`; the program writes `feedback + upsample(0x28) ·
  relHDRExposure.y` (a depth-weighted gather of the four quarter texels).
  Its two samplers are only gathered through, so they are made
  non-filtering (gathers do not filter; the R32F inputs cannot be).

* **Local lights** (list flag 2: a volume with flag 0x40; built
  2026-10-27). The light packer keeps, per 32 listed lights, a bit word of
  the volumetric spots and one of the volumetric omnis (a light whose byte
  +0x164 is set) in two structured buffers (`volumetricSpotLightBits`,
  `volumetricOmniLightBits`), and writes the first and last volumetric spot
  and omni into `LightingGlobals` words 200..203 (3000 and 0 when there is
  none). In the pre-pass (with flag 2) `volumetric_light_cull_compute`
  (threads: tiles x, tiles y, 16) ORs each tile's `visibleLights` words over
  the 16 depth slices and ANDs them with the bits into two 16-layer tile
  lists; the march then draws `volumetric_light_skip_samples`
  (`volumetric_light` without its dvar) into 0x28 after the sun's, its
  blend state adding, reading the lists (code textures 0x1a, 0x1b), the
  light records as a structured buffer (code buffer 1, 60 words a light,
  the `LightingPackCBuffer` records), their cull constants (code buffer 4),
  the spot shadow array (5), the cookie array and the range words (>> 5).
* **The 3-D volume** (list flag 4: a volume with flag 0x80; built
  2026-10-27). The probe packer likewise keeps a bit word per 32 listed
  probes of the volumetric ones (byte +0x121) and their first and last at
  `LightingGlobals` +0x330/+0x334 (250 and 0 when none). In the pre-pass
  `volumetric_probe_cull_compute` (forward `visibleProbes` AND the bits)
  and `volumetric_light_compute` (threads: the volume's width, height and
  64 >> 3: a group of 4 lights 32 slices, 4 at a time) light
  `volLightTexture` (`volumetricLightVolume`: one texel per 4x4 quarter
  depth texels, i.e. 16x16 pixels, 64 slices of `(1000 − near) / 64`;
  R11G11B10 in the exe, RGBA16F here since wgpu cannot store to it) from
  the probe volume, the reference probe packs and the volumetric probe list
  (bound where the program names `visibleProbes`, code texture 0x52, which
  the exe points at the volumetric list); the march samples it. Without the
  flag nothing writes it and the march samples zero, as in the exe.
* Both are made only for a map that has such a volume (no staged map has
  one, nor any volumetric light or probe); `T7_VOLUMETRIC_TEST=1` (offline
  checks only) gives every volume flags 0x40 and 0x80 and makes every
  packed light and probe volumetric.

**Checked offline (2026-10-27, `t7_host_check`, optimized build, lavapipe,
`T7_NO_PREWARM=1 T7_IMAGE_MAX_DIM=256 T7_NO_PROBE_ARRAY=1` to fit the
box's memory; final images read back and compared with the passes off):**

* mp_sector, 160x90, eye at volume 0's centre (`-1072 -651 490`, level):
  two volumes packed (flags 0x1, sun march only); 32.9% of the pixels
  brighter, none darker, mean +8.1/+6.7/+4.4 (of 255), up to +67: a warm
  sun-coloured haze over the far (sky) part of the view, cut by the
  buildings in front and zero in the sun-shadowed interior below (bottom
  third unchanged).
* mp_apartments, 320x180, eye in the map-wide volume 1 (`-1182 236 1334`,
  20° down, nothing but far depth in view): every pixel +7/+6/+4.
* mp_sector, eye in volume 1 (`-468 981 410`, 25° down at a dark,
  sun-shadowed interior): no change but effect sprites (the march adds
  nothing where the sun is shadowed).
* `T7_VOLUMETRIC_TEST=1` on mp_sector (320x180, volume 0): list flags 0x7,
  every pass recorded (pre-pass light/probe culling and the 3-D volume,
  20x12x64; sun and local-light marches), no validation error; it adds a
  bluish probe-lit haze over the whole view (mean +5.5/+6.6/+10.8), most
  in the near, shadowed part.
* Re-checked on the current main (2026-10-27, later build, also
  `T7_NO_LIGHTS=1 T7_NO_PROBES=1 BO2MP_T7_THREADS=1 LP_NUM_THREADS=1`
  to fit the shared 6 GB box): mp_sector 160x90 at `-1072 -651 490 0 45`,
  three volumes packed (flags 0x1): 40.0% of pixels brighter, none
  darker, mean +10.0/+8.2/+5.2, max +48; the haze is smooth (neighbouring
  pixels' added haze differs by at most 6 of 255: no banding) and cut by
  the skyline. HDR lit scene (`BO2MP_T7_NATIVE_DEBUG=lit`): mean
  0.0097/0.0065/0.0032 with the passes vs 0.0064/0.0039/0.0017 without,
  max 2.418/1.683/0.904 in both (finite: no NaN/Inf; the fog adds energy
  but never brightens past the scene's peak). mp_apartments 320x180 at
  `-1182 236 1334 20 45`: 68% of pixels +1..+5, none darker.
* mp_western (Outlaw, 2026-10-27; one map-wide volume with flag 0x40, so
  the local-light march runs, 4 volumetric spots): the owner's PC showed
  the whole view white, and the passes went back to opt-in (0758cad)
  until the cause was found. It was not the fog: the HDR lit scene's mean
  is 174.19/70.54/40.18 with the passes and 174.19/70.54/40.18 without,
  and the final image is identical. The world's grade LUT material names
  `create_lut2dv#da7372ff`, no image in any zone, so the post chain was
  not made and the raw HDR scene was shown (fixed in `post_run`:
  `identity_lut_win` is taken instead, 33d7a66). With that, Outlaw,
  Sector, Apartments, Nuk3town and Metro draw with no white pixels, and
  the passes are on by default again.
* The pass's dvar defaults read off BlackOps3_1.exe's registration
  (`141d978d0`.., static): samples `0x4fdd24df` 40 (1..256), steps
  `0xa90e937b` 8 (1..256), blur `0xfc7f08f7` 10 (0..40), `0x46d615f1` 0.01
  (0..1), `0xa08b16df` 2000 (0..10000), half-resolution apply
  `0xc61a70a7` off, both `_skip_samples` bools on, the pass `0x61444835`
  on, mode `0x2fbd3d93` 0: all as `volumetric::Dvars::default`.
* No validation error in any frame; maps with no lit fog volumes make
  nothing, and a view whose list is empty records nothing (Nuketown's one
  volume never packs).

Not built: the `2DRayMarchUnified` and `3DVolume` modes and the
half-resolution apply (dvars off by default). `T7_VOLUMETRIC=0` turns the
view flag off. Log: `t7 volumetrics: GPU passes made ... local-light march
made|not made ...; 3-D volume made|not made ...` once, then `t7
volumetrics: frame N: K lit fog volumes packed (...); drawn: pre-pass ...;
<passes>` whenever the passes change.

## The map's sun, as the view gets it (translated; `fastfile_t7::native`)

`GfxSunVolume` + 0x4c holds four `GfxGlobalLightSettings` (0xf4, one per
lighting state), which the exe copies whole. The view setup takes the
current state's settings and writes the sun config the builder above
reads: colour `settings[2..4] × settings[5]`, intensity `settings[5]`,
scale `settings[6]`, direction from pitch `settings[0]` and yaw
`settings[1]` (degrees): `(-cos p·cos y, -cos p·sin y, sin p)`, a second
direction from `settings[59..60]` added to them, and the shadow mode (0 when
the settings' byte 0x60 is clear, else 3 — 2 on secondary views). The
lighting state comes from the scene, which picks the first volume whose
planes contain the camera (sun volume list in the world). `t7_sun <map.ff>`
prints it: Nuketown's state 0 is colour (2896, 2094, 1509), direction
(-0.499, -0.349, 0.793), sun shadows on.

The per-view fog/sky state is a 0x1960-byte copy of a global lighting
state the client fills (vision/fog script calls, lit-fog banks). It is the
view's code-constant block itself: constant `i` at `+0x10·i`
(`relHDRExposure` 0x70, `skyRotationTransition` 0x290, `skyMx` 0x2d0,
`hdrControl0` 0xa90), so its 0x290–0x32c are code constants 0x29..0x32.

### `skyMx` / `sunMx`: the vukko blend (found 2026-10-26; `native::sky_matrices`)

The view setup's vukko step writes `skyRotationTransition` and
`skyMxR/G/B`, `sunMxR/G/B` (0x2d..0x32) each view, from the map:

* 32 vukko records, 0x80 bytes (32 floats `v`), inline at `GfxWorld +
  0x600` (to 0x1610); record 0 is the default;
* modifiers at `GfxWorld` 0x1610 (64-byte records: the exe's stride; the
  walker had this list's size unread) and boxes at 0x1620 / 0x1630
  (144-byte records), weighed by the same functions and mask as the
  colour vision's (record index: `u16` at 0xe resp. 0x7e, `& 0x1f`;
  strength 0x1c resp. 0x78); the weights accumulate the 32 floats
  (`sum += w·v`, no four-slot limit, no normalisation), the rest of 1 to
  record 0;
* rows: `skyMxR/G/B = (v0..2 · f24, D1.x)`, `(v4..6 · f25, D1.y)`,
  `(v8..10 · f26, D1.z)`; `sunMxR/G/B = (v12..14 · f28, v11)`,
  `(v16..18 · f29, v15)`, `(v20..22 · f30, 1)`, where
  `D1 = (cos(−v7)cos(−v3), sin(−v7)cos(−v3), sin(−v3))`,
  `fk = lerp(1, v[k], t)`, `t = smoothstep(clamp(clamp(D2·forward)·v27 +
  v31))`, `D2` from pitch `−v19` and yaw `−(v23 + a)` (`a` the sky
  rotation angle of `skyRotationTransition`);
* a script vision override (client weight / record) lerps the sum toward
  one record: not driven (weight 0).

Nuketown (no vukko volumes): record 0 gives identity sky and sun rows,
`D1 = +x`, `sunMxR.w = 4.613`, `sunMxG.w = −3.262` (`t7_sun` prints it).
These replace the identity stopgap of 138774a (which had the `w`s wrong).
`colorMatrixR/G/B` (0x11..0x13): **not needed** (checked 2026-10-26). No
writer was found by stores to the block, and no technique argument of any
program in the staged zones reads them (or `colorBias`/`colorTintBase`/
`colorTintDelta`, 0x24..0x26): `core_patch`, `core_common`, `core_gfx`,
`core_pre_gfx`, `core_post_gfx`, `mp_common`, `core_frontend_patch`,
`core_ui`, `en_mp_patch` and mp_nuketown_x, mp_sector, mp_apartments,
mp_skyjacked — 0 arguments, against 492 reading `postFxControl0` in
`core_patch` alone. They are Black Ops II's per-view colour matrix; Black
Ops III grades through the LUT that `create_lut2dv` builds each view from
the visions (`postFxConst00..59`), already ported. Left unset; the post
chain counts readers at start (`render_t7::code::readers`) and logs `t7
colorMatrix: N technique arguments ... read colorMatrixR/G/B` so a zone
that did read them would show.
`hdrControl0/1` and `relHDRExposure` were already the exe's
(`Post::begin_view` / `hdr_controls`, from the exposure EV).

## Which technique draws what (from the zones)

A material's technique set has 12 slots; on map materials slot 2 is the
depth prepass (no colour outputs), **slot 3 the G-buffer pass** (three
outputs: diffuse colour, normal + gloss, specular colour + occlusion; depth
from the depth buffer), slot 4 the same with a fourth output (motion
vectors), slot 5 an unlit simple pass, 8–11 debug views (wireframe,
overdraw, matrix mode, thermal). `t7_techslots <zone> [set]` lists them.
The G-buffer targets are the code textures `GBufferDiffuseColor` (0x2f),
`GBufferSpecularColorAndOcclusion` (0x30), `GBufferNormalAndGloss` (0x31),
`GBufferDepth` (0x32); the lighting writes `diffuseLighting` (0x33) and
`specularLighting` (0x34).

### Render targets (found: the exe's render-target table)

The renderer keeps its render targets in one pool (0xae0 bytes each,
indexed by render-target id). At start-up one function builds descriptor
arrays (0x2c bytes each: u16 id, u32 slot @4, u32 flags @8, width/height
size-class indices @0xc/0x10, u16 array size @0x14, colour DXGI format
@0x18, depth DXGI format @0x1c) and hands each to the pool's setup. A
descriptor with flags bit 12 aliases another target (its id in bits
17..25), so one target can carry several colour slots plus another
target's depth. Colour targets are created with bind flags RT|SRV|UAV
(0xa8), depth with DS|SRV (0x48); typed formats are created typeless and
viewed typed (R10G10B10A2 → TYPELESS, R8G8B8A8_SRGB → R8G8B8A8_TYPELESS,
D32_FLOAT_S8X24 → R32G8X24_TYPELESS with an R32_FLOAT_X8X24 shader view).

The view keeps a role → render-target table (u16 per role); each code
texture is bound as (render target, view), where view 0 is the depth and
view 2+s colour slot s. From the binding code and the descriptors:

| code texture | render target, view | DXGI format |
|---|---|---|
| GBufferDiffuseColor (0x2f) | G-buffer target 0x14, slot 0 | R8G8B8A8_UNORM_SRGB |
| GBufferNormalAndGloss (0x31) | 0x14, slot 1 | R10G10B10A2_UNORM |
| GBufferSpecularColorAndOcclusion (0x30) | 0x14, slot 2 | R10G10B10A2_UNORM |
| GBufferDepth (0x32) | 0x14's depth = scene target 0xa's | D32_FLOAT_S8X24_UINT (read as R32_FLOAT_X8X24) |
| motion vectors (G-buffer slot 3, technique slot 4) | 0x16/0x17 (per frame), slot 3 | R16G16_SNORM |
| diffuseLighting (0x33) | 0x20 or 0x21 (per frame), slot 0 | R11G11B10_FLOAT |
| scene colour (target 0xa) | slot 0 + depth | R11G11B10_FLOAT + D32_FLOAT_S8X24_UINT |

All of these are full resolution. `specularLighting`'s binding was not
traced; the permuted lighting writes one UAV (four floats), which fits
the scene target, but that is not confirmed. Other targets in the table:
the bloom chain 0x2e–0x34 and 0x43 (R11G11B10_FLOAT at successive size
classes), half-resolution R32_FLOAT depth copies (0x25, 0x26), R8_UNORM
SSAO-style targets, R16G16B16A16_FLOAT post-effect targets.

Visions in the shipped zones: `core_common.ff` (30, incl. `neutral`,
`default`, `mp_nuked`, `mp_array`, `mp_cracked`, `mp_havoc`, `mp_mountain`)
and `mp_common.ff` (18, vehicle/killstreak/ability visions); no MP map
zone carries its own vision, and `vision/default.vision` is empty.

## Map data the exe reads (checked against the exe's own struct sizes)

The exe carries a `{type name, size}` table for every streamed struct (862
entries). Our `GfxWorld` walker's record sizes agree with it everywhere we
read: `GfxSunVolume` 0x11b8 (4536), `GfxReflectionProbe` 0x148 (328),
`GfxExposureVolume` 0x6c (108), `GfxSurface` 0x60, `GfxStaticModelDrawInst`
0x98, `GfxBrushModel` 0x40, `GfxLitFogVolume` 0x168, `GfxOutdoorVolume`
0x1a8, `GfxLutVolume` 0x3c, `GfxProbeVolumeTextures` 0x20,
`GfxReflectionProbeVolumeData` 0x60, `GfxExposureProbe` 0x50,
`GfxLightmapArray` 0x10, `GfxLightingSH` 0x30, `GfxConfig_Sun` 0x40c,
`GfxConfig_Probe` 0x10c, `GfxConfig_ProbeBlend` 0x25c, `GfxConfig_Fog` 0x8c,
`GfxConfig_LitFogVolume` 0x168, `GfxGlobalLightSettings` 0xf4. No walker
fix was needed for sizes.

## Pass order (from the technique table and the draw code; being completed)

depth prepass → G-buffer (material `gbuffer_*` programs: diffuse, normal +
gloss, specular + occlusion, depth) → tile classification
(`deferred_permute_classify`) → per-tile lighting
(`deferred_lighting_permuted(N)`, N from the tile's features) / uber resolve
→ GTAO and volumetrics resolves → forward/transparent (`lit*`, OIT) →
exposure (`exposure_compute`) → bloom chain → LUT build (`create_lut2dv`) →
`bloom_apply_lut` (bloom + grade) → AA (SMAA/FXAA) → `resample_final`.

### The lighting pass as the exe records it (found 2026-10-22)

One function records a lit view: the depth/G-buffer draw lists (0, 2, 3),
the sun cascades when shadows are on (mode 3), spot/omni shadow maps, then
the deferred lighting. Code techniques are global slots
(`ComputeShader*`); a view uses variant `draw method byte` (view +0x2d4) of
each (`+0x10 + 0x38·method`); a dispatch's thread counts are divided by the
variant's thread group size (rounded up). Order:

1. **Light culling** (own function): `light_culling_probes`,
   `light_culling_lights`, `light_culling_decals` fill the per-tile cull
   bits (`gDeferredCullBits`) from the packed lights/probes/decals.
2. **`deferred_permute_clear`**, dispatched (1, 1, 1): clears the
   accumulate/extents buffers.
3. **`deferred_permute_classify`** (the `_swizzle` and `_no_sun` variants
   by settings/sun), dispatched (`(w+7)>>3`, `(h+7)>>3`, 1) threads (one per
   8×8 pixel tile; 64-thread groups): writes each tile's permutation into
   `gPermutedTilingBuffer` (8 bytes per tile) and counts per permutation.
   Its arguments: PerSceneConsts matrices (view, inverse projection,
   inverse view), renderTargetSize, subpixelOffset, and code texture 4
   (`gSunShadowMax`) at t14.
4. **`deferred_permute_draw`**, dispatched (1, 1, 1): turns the 64 counts
   into indirect dispatch arguments (12 bytes each).
5. **`deferred_lighting_permuted(N)`**, N = 0..63 (the `_bilateral` /
   `_ssr` set when those settings are on): one `DispatchIndirect` each, at
   offset `12·N` of the indirect argument buffer, with the G-buffer, depth,
   LightingGlobals, the light/probe packs, the probe volume and reflection
   probe arrays bound (`docs` above), writing the scene target.
   (A single full-screen `deferred_lighting` path exists in the exe but its
   programs are not shipped in `core_patch`: the retail game always runs
   the permuted path.)

Compute shaders carry their own argument records like materials'
(`walk::compute`: type 0x406 code constant, 0x506 code texture; `t7_code
<core_patch.ff> <filter>` with `T7_CODE_ARGS=1` prints them). The engine
binds the structured buffers and UAVs (tiling, accumulate, extents, cull
bits, indirect arguments, scene target) itself.

Post chain functions: exposure (`exposure_compute`), LUT
(`create_lut2dv(_compute)`), bloom (`bloom_downsample_ev`,
`bloom_apply_lut`), final resample (`resample_final`): each its own
function, not yet translated.

### Cloud shadows and the cookie array (2026-10-26)

`gCookieArray` (code texture 7) is bound by the lighting's code-texture
setup to the image at the world's draw (`GfxWorld` +0x258 is the draw
record; its +8, the world's +0x260, is the image; the walker kept only its
asset reference and now keeps its name, `GfxWorldRec::cookie_image`). It is
a 2-D array: the sun's cloud layer and the lights' cookies are layers of
it. The deferred lighting programs (and the volumetric march, and the
forward programs that light by the sun) test sun constant word 24 (sun
config 0x50): when non-zero they sample layer `word24 − 1` at
`fract(dot(sun[28..31], (p, 1)))`, `fract(dot(sun[32..35], (p, 1)))` at
the world position `p` (the cloud transform: rotation and scale from config 0x44.., scroll
from time) and scale the sun colour by `1 + config 0x40 · (cookie − 1)`.
`render_t7::lighting` loads the array as stored (the zone's pixels, else
the top streamed part). The byte order is level-major: each level holds
every layer (checked by `t7_volumetric_check`: each layer's mip 1 is its
box-filtered mip 0 to a mean error of 0.1-0.2 of 255 read level-major,
30-90 read layer-major; the staged maps' `*cookie_array` is 512x512 RGBA8,
10 levels, 5 layers on mp_sector, 7 on mp_nuketown_x, 3 on mp_skyjacked, 1
on mp_apartments and mp_mountain). **None of the staged maps' suns has a
cloud layer** (config 0x50 = 0 on all six), so the cloud path is not seen
drawn.

**The spot lights' cookie byte (fixed 2026-10-27).** The light packer's
record word 0x3b low byte (intermediate word 0x40) is the light's own word
at +0x230, which the packer passes down to the intermediate as the fourth
float of its parameters; it was taken as the light's place in the list.
The deferred program samples layer `byte − 1` of the array only when the
byte is not zero, and the staged maps bear it out: the largest value
equals the array's layer count on each (`t7_volumetric_check`: mp_sector
0..5 with 5 layers, 28 lights on 2; mp_nuketown_x 0..7 with 7; mp_skyjacked
0..3 with 3; mp_apartments 0..1 with 1; most spots 0, no cookie).

The array stays **opt-in** (`T7_CLOUD_SHADOWS=1` binds it; unset or `0`
binds one white layer and clears word 24), because nothing it draws has
been seen. Checked offline (2026-10-27, `t7_host_check` on mp_sector at
320x180, views behind cookie spots, final images and, with
`BO2MP_T7_NATIVE_DEBUG=lit`, the HDR lit scene read back): binding it
changed no pixel; but neither did `T7_NO_LIGHTS=1`: the map's lights
light nothing in the offline frame (lit scene mean 0.075/0.056/0.032
either way), so the cookies are not seen drawn, and no staged sun has a
cloud layer (mp_mountain's included, checked 2026-10-27). Turning it on
by default waits for a map whose sun has a cloud layer, or a frame where
a cookie spot lights something, read back. Log: `t7 cloud shadows: gCookieArray <image>: WxH, N
layers, ...`, `t7 cloud shadows: spot light cookie layers (...)` and `t7
cloud shadows: on|off|...`.

### Sun shadow cascades (`render_t7::sun_shadow`, merged 2026-10-26 from `wip/t7-sun-shadow`)

BO3 does not draw casters into its sun maps for the static world: the
cascades are filled from the map's streamed **sun shadow tree** (sun volume
+1056 info, its data in the map's xpak). Per view (`141e069a0` → fit
`141d42820`): three cascades over the frustum at `d/8, d/2, d` with
`d = 2·(|sun.z|·(0.8490505·p − c·p) + c·p)`, `c = √(0.4775909² + 0.7208867)`
(`141d420d0`, checked against the decompile), each an orthographic view of
the tree's sun space snapped to 32-texel tiles, with the tree level each
reads. On the GPU (`141d6fba0`): `depth_fill_from_sst` (DrawSST constants
`141e079b0`) per tile into the tile cache (render target 6), then
`floatz_writedepth` into the cascade's layer of `gSunShadowmapArray`
(render target 5, 1024² D16); `gSunShadowMax` is the min/max tree's image;
the lighting's sun constants' shadow fields (`141d43c20`) and
`minMaxConstants` (`141db8530`) come from the fit. Differences: every tile
is refilled each frame (the exe refills only tiles that scrolled in,
`141e06020`); the cache is R32F (exe R16_UNORM). Not done: dynamic casters
(models, players) drawn over the static fill, spot/omni shadows. Log lines:
`sun shadows: tree ...`, `t7 sun shadow: cascade N: ...`, or `sun shadows:
not drawn (...)` with the mode 2 fallback.

**Shadows following the player (fixed 2026-10-28).** Sun floats 44..59
(the shadow lookup matrix) are `A · S`, `S` the tree's world-to-sun matrix
and `A` the fourth argument of `141d43c20`, which `141db98e0` builds at
rsp+0x60 as rows (1,0,0,0) (0,1,0,0) (0,0,1,0) (eye, 1) (the view's
+0x240..+0x248; the same struct's +0x1a0 is the eye the global probe's
centre is made relative to). The lighting reconstructs eye-relative
positions, so the lookup must add the eye back; the port had `A` as the
identity, so every pixel read the cascades at `P − eye`: shadows slid with
the camera, came from the wrong casters and swam on everything in view.
`ShadowState::lookup` now writes `translate(eye) · S`; the unit test
`a_world_point_stays_put_in_the_cascades_as_the_camera_moves` checks a fixed
point's sun-space position and texel phase over sub-texel, multi-texel and
turning moves. The offline check below was not affected (it maps absolute
points through `S` itself), so its cascade-1 disagreements are a separate
question. Not yet seen on screen.

**Shadows still following the player: the lit positions were 4x too near
the eye (fixed 2026-10-28, `lighting::ExeDepth`).** The lighting-stage
programs (`deferred_permute_uber_resolve_gtao`, the classification, every
`deferred_lighting_permuted(N)`, the volumetric march) decode
`GBufferDepth` with constants in the DXBC itself: view `z = 63 / (64·d)`
below `63/64`, `1 / (64·d − 63)` above (the view model's band), then the
eye-relative position `z · (ndc.x · invP00, ndc.y · invP11, 1)` rotated by
`inverseViewMatrix` (PerSceneConsts registers 12..19). Our depth is `near /
z` (reversed, infinite), bound raw: every lit point came out `64·near/63`
(about 4 at near 4) times nearer the eye than it is, so the sun shadow
lookup (and the fog, the probe volume and the local lights) read the world
at `eye + (P − eye)/4`: a place that moves with the camera. That, not the
cascades, is what the owner saw after d1fa2b0. The forward lists already
converted their `floatZ` the same way (`crate::forward::FloatZ`); the
lighting stage now does too: `d · 63/(64·near)` into an R32Float target each
frame (the frame's near) before the sun shadow fill, bound as
`GBufferDepth` for every lighting program (GTAO's depth pyramid and the
volumetric march get the exe's encoding too). `T7_RAW_GBUFFER_DEPTH=1`: the
old raw binding.

**Checked against rays (2026-10-28, `t7_sun_shadow_check`, rewritten).**
Every seen point is read exactly as the permutations read it (eye-relative
point through floats 44..59, `1 − (z − f3)/16384`, the band test on floats
12..23, `uv = 0.5 ± 0.5·d/s`, layer `c + f37`, the comparison sampler's 2x2
PCF, `saturate(pcf + trans − 1)³`), and past the cascades as the tree read
(level 2, `lit ⇔ z ≤ h + 2·cell`, `h` from `depth_fill_from_sst` over a
one-cell-a-texel window; the window agrees with cascade 2 on 98.5 % where
both exist). Truth: a CPU ray toward the sun. Excluded: the penumbra (a
neighbour 1.5 texels away along the surface sees otherwise), the exe's own
contact zone (an occluder nearer than the fill's bias), and points whose
sun ray passes within 160 units of a static model the offline scene could
not build (the tree holds them, the rays do not). Sector, 14 views:
cascade 0 99.9 %, cascade 1 98.7 %, cascade 2 92-97 %, tree read 75-93 %
(the misses are lit by the rays but shadowed by casters hundreds of units
up the sun ray that the offline scene lacks: vista and unbuilt models).
`T7_MOVE_TEST=1`: the camera steps 0.4 to 200 units and turns; refitted and
refilled on the GPU, no static point's shadow changes (the fill and lookup
are stable). Shadowed points all keep probe-volume light (no black).
Tree axis check on every staged map's zone (no package needed): the tree's
depth axis is exactly the reverse of the sun direction the lighting uses
(Sector, Apartments, Aquarium, Metro, Outlaw, Summit, Warhawk). Not built:
spot/omni shadow maps, dynamic casters (players, view model) drawn over the
static fill. The full lit frame offline: the G-buffer without its pictures
(the box's memory) leaves the normal target zero, so the permutations skip
every pixel; not a check of the shadows.

**Checked offline (2026-10-27, `t7_sun_shadow_check`):** `mp_sector.xpak`
is staged now and holds Sector's tree (128 174 292 bytes, 421 x 215 tiles of
282 units). The example fills the three cascades on the software GPU (only
`depth_fill_from_sst` and `floatz_writedepth`: no G-buffer, no lighting),
reads them back, and for each pixel of a 64x36 grid casts a CPU ray from the
eye to the scene's triangles and one from the surface toward the sun
(opaque surfaces only). Read as the lighting permutations read it (sun-space
point from the info matrix, cascade = first with `max|xy - centre|/s < 1`,
`v = 0.5 - 0.5 y`, lit when `1 - (z - zBase)/16384 >= map`, the
`GreaterEqual` comparison sampler), the maps agree with the rays on 94.1 %
of the spawn view's pixels and 90.5 % of an overhead view's (cascade 0:
98.9 %; most disagreements in cascade 1: lit by the map, shadowed by the
ray: their occluders are 0.7 / 4.3 / 11 units away (10th / 50th / 90th
percentile) and the points lie within about 9 units above the map's depth,
i.e. contact shadows inside the fill's bias at that cascade's 1.1-unit
texels, not a misplaced map). The other readings (v up, or
the depth sense reversed) agree on 6..70 %. The sun direction points toward
the sun. Not checked: the shadow on screen (the lighting needs more memory
than the box has beside the other jobs), Nuketown and Skyjacked (their trees
are not in the staged packages), dynamic casters (not drawn).

**Everything out of the sun black (found 2026-10-27, Aquarium on the PC).**
The owner's mp_biodome log: `lighting: globals: ... shadow mode 3; probe
units/tint (sun[8..12]) 0.000 0.000 0.000 0.000` (offline, mode 2, the same
map gives 531.178 0.876 1.021 1.161). Sun constants 8..11 are the global
probe's config words 0..3 (see "Debug views": every deferred permutation
multiplies the probe volume's light by float 8). The sun builder
(`141d43c20`) takes them from the shadow state's +0x314 / +0x308..+0x310,
which `141d42240` copies from the view in every shadow mode; our fit leaves
that view record unread (`FitCamera::w68` / `v6c` zero), and
`SunShadows::patch_globals` wrote its whole sun block over the globals, so
with the map's sun shadow tree streamed (mode 3: every map whose package
holds it, i.e. on the PC) all indirect light was zero and whatever the sun
did not reach drew pure black: Aquarium's palms, planters, the dome, the
harbour water and the debris on it; the sunlit sides stayed lit (the green
palm beside the black ones). Fix: `keep_probe_words` keeps the global
probe's words the globals builder wrote when the state has none
(`CODLEGACY_T7_SHADOW_PROBE_WORDS=zero`: the old zeros). Unit test
`sun_shadow::tests::probe_words_survive_the_shadow_patch`.

### `render_t7::lighting` (stage 2, built 2026-10-22)

`Lighting::new` makes the programs from `core_patch` (68: resolve, clear,
classify, draw, 64 permuted) and uploads the map's light;
`prepare(targets)` sizes the buffers and binds; `update(camera, ms, cut)`
fills the constants; `record(enc)` records the stage after the G-buffer;
`scene_target()` is the lit scene (code texture 0xa). `Host` makes it on a
thread of its own and exposes `Host::scene_target()`. Offline check:
`t7_lighting_check <map.ff> [w h]` (`T7_LIGHTING_PNG=` writes the target).

As recorded (`141d5bf40`, `141d5ca10`):
1. `deferred_permute_uber_resolve_gtao` (the main view always takes the
   GTAO resolve: view `+0x3a1` is set for world views), threads to the next
   multiple of 8: per-8x8-tile depth extents (`gPermutedExtentsBuffer`; sky
   tiles `FLT_MAX`), the resolved normal and the GTAO depth pyramid
   (DS2x/4x/8x/8xCopy; the pyramid's reader, GTAO, is not run).
   `gtao_2` is not run.
2. clear, 3. classify (shadow mode 0 → `_swizzle_no_sun`, else
   `_swizzle`), 4. draw, 5. 64 × `DispatchIndirect` at `12·N`; each
   permuted program offsets its tiles by `permuteStride·N` itself.

Findings used:
* **Shadow mode** (`141ddd1b0`): 0 when the lighting state's byte 0x60 is
  clear, else `3 − (flag at view-setup +0x16499)`: 3 = cascaded sun shadow
  maps, **2 = sun lit without shadow maps** (`141d43c20` then sets the
  cascade bands, sun floats 12–23, to `FLT_MAX`, flag 27 stays 0, and
  `141d5ca10` skips the shadow passes). `_no_sun` classification never
  sets the sun bit, so mode 0 means no sun at all. Mode 3 now draws the
  cascades (`render_t7::sun_shadow`, below) when the map's sun shadow tree
  is read; else (or `T7_NO_SUN_SHADOW=1`) the mode 2 way. On
  Nuketown the classification then picks permutations 5 and 37.
* **Sun constants**: colour scale argument is 1.0 (`DAT_142f875ac`), so
  float 7 = config 0x10.
* **globalProbeConstants** (0x230–0x25c) come from the global probe's
  328-byte config (world probe data `+0x10`, record 0): box centre (words
  29–31) − eye, `0.5 / max(1e-4, (w23..25 + w26..28)·0.5)`, then words
  4–9 copied raw (the programs read them as floats: the probe volume's
  scale and offset). The global probe is layer 0 of
  `gReflectionProbeArray`.
* **minMaxConstants** (0x210–0x22c; RDEF says unused by the lighting, but
  classify `_swizzle` reads them with `gSunShadowMax`): from view
  `+0x358` (ints w, h, floats) and `+0x1730/+0x1738`; not filled (sun
  shadow max is not built).
* `permuteStride` (0x390): its writer is not found; any stride ≥ the tile
  count gives the same result (each permutation's tiles are a separate
  range); the tile count is used.
* Typed buffer UAVs (`RWBuffer<uint>`, the indirect arguments) are storage
  buffers of 32-bit words (`dxbc_sm5` option
  `typed_buffer_uavs_as_words`, on in `render_t7::compute`).
* Code samplers: `samplerLinear` (wrap), `samplerLinearClamp`, `samp0`
  (clamp; only the transparent shadow map uses it),
  `ShadowSamplerComparisonState` (greater-equal), `ShadowSamplerState`.
  Their exact states are not read from the exe yet.

Gaps (bound empty or zero, listed in the log as `lighting gap:`): no lights
(light packer not found), no local reflection probes (cull constants,
RefProbePack/Blend not packed), sun shadow maps / spot / omni / trans
shadows, cookies, cloud shadows (sun config 0x40–0x64 not mapped), SSAO
(enableAO 0), `viewmodelSunShadowRaw`, `enableDitheredShadow`, `gameTick`
(only turns shadow tap dither). The scene target is RGBA16F (wgpu has no
R11G11B10F storage).

### Lower half of the frame black on the PC (found 2026-10-23, from the video)

* **`gDeferredCullBits` was half the size the classify indexes.** The classify
  writes a tile's cull words at `(tile << 5) + k`, `k < 16` (`insertBits(16,
  tile, 5, 27)` is its limit), and the many-light permutations (every one with
  a cull-bit loop: 0-4, 8, 12, 16-20, 24, 28, 32-36, 40, 44, 48-52, 56, 60 and
  the `_bilateral` / `_ssr` sets) read them back the same way: 32 words a tile
  (16 used), not 16. The buffer held 16 words a tile, so every tile from the
  frame's middle row down wrote and read outside it: a software device drops
  the writes and reads zero (3507 pixels of Skyjacked 1080p differ, all in the
  lower half), the PC's driver gave a hard black band at exactly half the
  height on vertical surfaces and blocky wrong light. Now 32 words a tile.
* **Dispatch counts past the device limit.** One permuted group is one 8x8
  tile and `deferred_permute_draw` writes `(tiles, 1, 1)`; a permutation
  can pass 65535 tiles at 4K (480 x 270). Such a dispatch is now split
  (`Lighting::make_split`): a small program turns the 64 counts into one
  argument per piece (`min(piece, rest)` groups), and each piece's pipeline
  is the same program with the override `t7_group_offset` added to the group
  id (`dxbc_sm5::Options::group_offset_override`). One piece (nothing changes)
  up to 65535 tiles (1440p). `T7_MAX_GROUPS=<n>` forces a smaller limit to
  check it offline (lavapipe compiles every piece's pipeline: keep pieces few).
* The hand-off (`present`) now draws inside the view's viewport and reads
  the image relative to its origin, and logs the target size and format
  and the device limits; `lighting:` logs the tile counts, buffer sizes and
  pieces, and warns when a buffer passes the storage binding limit.
* `gSunShadowTree` was 16 bytes; it is zero and 64 KB (the permuted programs
  read it at an index computed from zeroed shadow constants).

### Lighting stage, later findings (2026-10-23)

* `permuteStride` is written by the packer driver 0x141db98e0:
  `((w+7)&~7)·((h+7)&~7) / 64` (the 8×8 tile count), which is what is set.
* **Map lights** (`render_t7::lights`, 0x141db6e10): per visible light a
  0x50 cull constant (lights first in `gCullConstants`, probes after at
  `+numLights`) and a 240-byte `LightingPackCBuffer` record (0x141db6bb0
  from the 0x53-word intermediate of 0x141d3fae0/0x141d40a40). The view's
  light list is the client's scene lights (scene +0x5da8, 0x238 each, count
  +0x1a5de8); the map's `ComPrimaryLight`s (564) are used, read at the same
  offsets. Visibility: type ≠ 0, layer mask (+0x158) & the lighting state's
  bit, not disabled (+0x15c), colour², distance fade (0x141db56e0; scale =
  refdef +0x7c, taken as tan(half vertical fov), clamped to 0.4775909), the
  frustum (no umbra). Classification puts up to two light indices in each
  tile entry (bits 24–31, 12–21) and picks permutation `base + lightClass`.
  Gaps: shadowed lights packed without a shadow slot (state 1, index
  0xffff), light flicker scale 1, cookies not loaded.
* **Sun shadow cascades** (to port): 0x141e069a0 → 0x141d42820 fits the
  cascades and fills the 800-byte shadow state at view+0x1440; the
  cascades are filled on the GPU from the streamed sun shadow tree by
  `depth_fill_from_sst` (DrawSST: 3 vec4 per cascade, 0x141e079b0;
  `gDispatchIDs`: packed 32×32 tile ids), cached and scrolled by
  0x141e06020, copied by `floatz_writedepth` into the depth array, then the
  dynamic casters drawn (0x141d6fba0). `gSunShadowMax` is the min/max
  tree's GPU image; minMaxConstants come from its record (view +0x358).
  The tree reads from the map's package (mp_sector: 128 MB, 3 mips).
  Read so far for the port: 0x141e069a0 copies the view's camera (its
  local `param_1` record: rows +0..+0x3c, origin +0x240, axes +0x250,
  fov +0x280/+0x284) and calls 0x141d42820(state, view +0x2d8 (sun
  shadow settings; +0x2c == 3 is the cascaded case, +0x14 split params,
  +0x30/+0x34/+0x38), view +0x350 (tree record), camera); the fit is
  per cascade (3): split distances from 0x141d420d0 (`d = 2·(|s|·(k·p −
  c·p) + c·p)`, texel snap `ceil`), a sun-space projection 0x141d44520
  (`1/x`, `1/y`, reversed-Z `n·f/(f−n)`), frustum planes 0x141d44950 and
  corners 0x141d44b80, texel-snapped bounds (floor/ceil, or fmod in the
  other branch), packed by 0x141d424b0 into the 800-byte state (3
  matrices at 0, 3 at 0xc0, per-cascade pairs at 0x180/0x198, vec4s at
  0x1b0, scalars 0x1e0..0x228, tile ranges 0x22c.., 0x2a0.. the first
  cascades' ids). The driver then copies the state to view +0x1440..
  +0x1758 and builds per-cascade culling planes (to `0x1486bcccc`, 0x120
  per cascade) for the dynamic casters. Ghidra loses 0x141d42240's
  arguments inside 0x141d42820; the disassembly is needed there.
* **Reflection probe packing** (to port): 0x141db8530: probes 1..n
  (0x148 records), visible by frustum/umbra (or the umbra bit when +0x144 ≠
  −1), key 1 (0 when +0x122), sorted, ≤ 0x111; cull constants via
  0x141d41c30/0x141d41d10 + 0x141db1670; blends (0x25c records, 0x60
  constants, 0x141d41160); probe constants 0x141d415b0 → 0x141db11f0
  (0xe0); globals +4 count, +8 blend count, +0x3a0 override count.

* **Local reflection probes** are packed (`render_t7::probes`, 0x141db8530;
  blends not yet). The cull constant is (centre − eye, AABB extents
  Σ|axis|·h, half sizes h, axis0..2); the record is 0x141d415b0's 57-word
  temporary repacked by 0x141db11f0 (word 30 = blend start / probe index /
  blend count bit-packed).
* **AO (GTAO)** (to port): 0x141d6de80 after the uber resolve. Quality from a
  dvar (`_DAT_1486bf5a0`); when `_DAT_1486f0ef0 == 1` it is the temporal path
  (`ssao_gtao_{low,med,high}` = dvar − 2, then
  `ssao_denoiser_spatial_temporal_1x_{1,2}` or `_t2x_{1,2}` by frame parity
  and temporal AA mode 4/5), else `ssao_gtao_ultra` + `ssao_denoiser_spatial`.
  Code constants 0x33/0x34 (state +0x730/+0x740): `(aspect/f, 1/f, rotation
  jitter (6 steps), offset jitter (4 steps))` with f = view +0x284, and
  `(exp(−k·dt), exp(…), 0, 0)` temporal weights. Inputs: the resolve's depth
  pyramid (DS2x..DS64x, so `gtao_2` runs first), the normal target; outputs
  through render targets 0x1e and 0x1b..0x1d (frame % 3 ring) to
  `gSSAOTexture` (code texture 0x55); then LightingGlobals `enableAO`.
* **AO as ported** (`render_t7::ao`): the uber resolve's depth pyramid is a
  three-image ring (exe `0x14a4fe868`, `(frame + 2) % 3` this frame's), each
  six mips DS2x..DS64x; the resolve's second dispatch `gtao_2` reads DS8x
  from `DS8xCopy` (render target 0x1f) and writes mips 3..5. GTAO
  (half resolution, R8) reads `GBufferNormalAndGloss` and the frame's ring
  image; the denoiser writes the frame's slot of the 0x1b..0x1d ring, bound
  as `gSSAOTexture` (`0x141d6dd20`; render target 0x19, cleared to 1, when
  the pass doesn't run). The quality dvar is an enum (Disabled, HEMIAO,
  GTAO Low/Medium/High/Ultra; default 3); `_DAT_1486f0ef0` is the GPU
  count (1 → temporal unless Ultra); the AA dvar `_DAT_1486bf4d0` (None,
  FXAA, SMAA 1x, Filmic SMAA 1x, SMAA T2x, Filmic SMAA T2x; default 5)
  picks the `t2x` denoisers. `postFxControl1` = `(exp(−dt/75),
  exp(−dt/215), 0, 0)` (dt ms); rotation jitter `{60, 300, 180, 240, 120,
  0}/360`, offsets `{0, .5, .25, .75}`; `viewSpaceScaleBias` (0x6) =
  `(2at, −2t, at, −t)` (view setup, t = tan(fov_y/2), a the aspect).
  `enableAO` is read by no lighting permutation. The temporal denoisers
  (the default) need the motion vectors (`motion_vector_static`,
  `0x141d5c6b0`, into render targets 0x16/0x17 with `MotionVectorParams`
  from `0x141ddc6c0`): `render_t7::motion` draws them after the resolve
  (floatZ rewritten in the exe's depth encoding first; no sub-pixel jitter,
  since no TAA runs), and the AO runs the exe's default (3, temporal, bound
  by frame mod 6: depth ring of 3, motion pair by parity). Without motion
  vectors it falls back to Ultra (5); `T7_SSAO_QUALITY` overrides. Rg16Float
  stands for R16G16_SNORM where the device lacks 16-bit norm targets. dxbc_sm5's opcode numbers
  for `lod`/`gather4`/`sample_pos`/`sample_info` were one low (107 is
  reserved); fixed.

## Map logic: which entity blocks the game spawns (`fastfile_t7::spawn`)

The exe's map-load spawn walks the entity string block by block (the
tokenizer is `{`, quoted key/value pairs, `}`) and decides per `classname`
(`spawnflags` read with `atoi`):

1. **Spawn points**, not entities: `info_player_start`, or in a
   multiplayer game an `mp_*` class containing `_spawn`, `_respawn` or
   `_intermission` (`cp_*` in a campaign game). (Kept for Black Ops II's
   scripts, which find them by class.)
2. `dyn_*` dropped; `node_*` path nodes; `actor_*` actors; `zbarrier_*`
   Zombies barriers; `scriptbundle_*` (case-insensitive) to the script
   bundle system; `info_vehicle_node[_rotate]` vehicle paths;
   `weapon_<valid weapon>` weapon pickups.
3. **Client-only, not server entities:** `script_model` without a
   `destructibledef` whose `client_server` is `ClientSide` or whose
   spawnflags have bit 2; `script_origin` with spawnflags bit 2 or
   `clientside` 1. (Kept in the text: our client draws map models from
   their entities.)
4. **Dropped:** `occlusion_override`, `rope*`, `umbra_volume`, `nav_volume`,
   `volume_fpstool`, `wave_water`, `glass*`, `fx`; `heli_height_lock` sets
   the helicopter ceiling (no entity).
5. **Entities:** the spawnable table (`info_notnull`, `info_notnull_big`,
   `info_volume`, `trigger_radius`, `trigger_box`, `trigger_box_use`,
   `trigger_radius_use`, `trigger_damage`, `trigger_radius_hurt`,
   `trigger_radius_out_of_bounds`, `script_model`, `script_origin`), then the
   map-only table (`trigger_use`, `trigger_use_touch`, `trigger_multiple`,
   `trigger_disk`, `trigger_hurt`, `trigger_once`, `trigger_lookat`,
   `trigger_out_of_bounds`, `trigger_ik_playerclip_terrain`,
   `trigger_navmesh`, `light`, `lensflare`, `reflection_probe`,
   `script_brushmodel`, `script_struct` (a struct), `script_vehicle`,
   `undertow_volume`). Class names compare case-sensitively.
6. **Anything else has no spawn function and is freed** (`volume_worldfog`,
   `volume_litfog`, `volume_sun`, `volume_weathergrime`,
   `nav_volume_passable`, `klf`, ...).

On Nuk3town: 776 spawn points, 279 entities, 164 structs, 132 path nodes,
120 client-only models; 622 `fx`, 17 `nav_volume_passable`, the fog/sun/
weather/fpstool/umbra/nav volumes, glass, heli lock and 9 scene bundles are
not spawned (they were entities here before, several with brush models).

Trigger shapes (the spawn functions):
* `trigger_box`: `width`, `length`, `height` (warning when missing), half of
  each kept as the box's extents;
* `trigger_disk`: `radius` + 64 in x/y, z from −100000 to +100000;
* `trigger_out_of_bounds`: contents class 7/9, flag 0x40 set on the entity;
* `trigger_use[_touch]`: `cursorhint` (default `HINT_INHERIT`),
  `hintstring`.

## Collision (`asset_world::world_t7`)

* The **world** is what the world trace reaches: the leaves the BSP reaches
  from node 0, their brushes, and only the AABB tree roots those leaves
  list (`firstCollAabbIndex`, `collAabbCount`). A sub-model keeps its own
  leaf (brushes and trees) and is traced only through its entity (Nuk3town:
  25 of 116 leaves are the world's; trees 0–21 are the world's, 11795+ the
  helicopter ceiling's `*13`).
* Brushes, planes, surface and content flags, partitions, triangles and
  walkable edges are taken as the zone has them; the world brushes sit
  under a k-d tree of their boxes, an index over the same brushes (Black
  Ops III's own BSP is 24 nodes over a leaf of 3297 brushes on Nuk3town), so
  a trace meets the same ones.
* Static models: every static model the clip map lists (`cStaticModel_t`,
  0x70) collides through its model's **collision triangles**: the
  `XModel`'s `XModelCollSurf` list (0xd8, count 0xe0; 0x30 each: triangles
  pointer, count, bounds as mins/maxs, bone 0x24, contents 0x28, surface
  flags 0x2c) and their `XModelCollTri`s (0x30: plane `n.p = d`, `svec`,
  `tvec`; a point's `s = svec.p - svec.w`, `t = tvec.p - tvec.w`, the
  triangle `s, t >= 0, s + t <= 1`; checked on Nuk3town: all 23696
  triangles sampled solve to corners inside their surface's bounds). These
  are held in the zone (not streamed), so a server traces them too. Each
  surface is in its bone's space, moved by the static model's posed bones
  (`cStaticModel_t` +96, count +104; `DObjAnimMat`), then by its origin,
  axes and scale. A surface's own contents and surface flags are kept.
  Every listed model collides, however small (the old radius cut-off is
  gone); a model with no collision surfaces collides with nothing (14 of
  2690 on Nuk3town, 0 of 6290 on Sector). Before, the models' collision or
  coarsest drawn LOD meshes stood in, which are streamed from the `.xpak`
  and missing when not built (430 of 2690 on Nuk3town offline), and a
  radius cut left small props out.
  Not checked against the exe's own trace function (the decompiler ran out
  of memory beside the other builds); the Havok shapes (`collmaps`,
  `physpreset`) are physics only and are not read for traces.
* Probe: `t7_spawn_models <map.ff> [model]` lists the static models whose
  collision a spawn's player box meets. Spawn probe (`spawns_on_t7`):
  Nuk3town 711 of 775 spawns on the ground, 63 meet solid (most against
  small props underfoot and door frames and walls within 15 units: the box
  probe, not a real trace); Sector 444 of 446 on the ground, 2 in solid.
* **Planes found where they are** (2026-10-27): the clip map's planes are
  a reference to the `GfxWorld`'s DPVS planes, and where that reference
  lands can sit a few bytes off them when the walk's VIRTUAL count drifted
  before the `GfxWorld` (mp_apartments: 4). There every non-axial brush
  side and BSP plane was read as (ny, nz, dist, type): 9369 of its 27196
  brushes cut by planes of nothing (floors, slopes, stairs and player clips
  with gaps or missing). `read_clip` now takes the start, within 16 bytes,
  whose planes have unit normals (`plane_start_shift`;
  `CODLEGACY_T7_NO_PLANE_FIX=1` the old way) and checks every brush against its
  own corners (the zone's `cbrush_t` vertices, +88/+96): the `collision:`
  report line gives how many brushes have a corner outside their planes
  and the shift. Offline: Apartments 9369 -> 0; Metro, Nuk3town, Sector,
  Skyjacked, Biodome, Summit 0 (the few brushes listed by `t7_contents`
  only carry redundant sides, inside every corner).
* **Invisible barriers (2026-11-03)**: two misreadings of the clip map,
  both in `fastfile_t7::map::read_clip` (exe notes:
  `docs/exe_notes/T7_COLLISION.md`).
  * *Static models turned the wrong way.* A `cStaticModel_t`'s inverse
    scaled axes are the matrix a point is multiplied by as a row vector
    (the IW/T traces' `world_to_local`), so the model's axes are its
    columns; they were read as rows, so every turned prop collided turned
    the other way (yaw t as -t): its collision stood beside the drawn
    prop, an invisible wall on one side and nothing on the other. Nuk3town:
    2185 of the 2260 placements matched to a drawn one had other axes, each
    the exact transpose; now 0 (Sector 3457 -> 9 of 4878, placements
    sharing an origin). Collision more
    than 8 units past the drawn model's box: 117 -> 2 placements
    (`t7_smodel_fit`). `BO2MP_T7_OLD_CLIP_AXES=1`: rows, as before.
  * *Brushes with sides missing.* Many brushes are stored with fewer
    sides than they have (a box turned 18 degrees keeps three of its four
    upright sides; some keep none), while their corners (+0x58/+0x60) are
    whole: traced by their planes alone, the bounding box's corner past a
    missing side stands solid, up to 250 units from anything built. The
    faces of the corners' hull that the planes leave out are added as
    sides (`sides_from_corners`, with the surface flags of the nearest
    axial side; a face never cuts a corner). Player-solid brushes bigger
    than their corners by more than a unit (`t7_clip_census`): Nuk3town
    931 -> 1 of 1826, Sector 1514 -> 0 of 5170, Evac 4290 -> 0 of 14642,
    Metro 975 -> 0 of 5753. `BO2MP_T7_OLD_BRUSH_SIDES=1`: as stored. The
    `collision:` report line counts the brushes given a side.
  * Not the cause: the contents mask. Black Ops III's player mask is
    Black Ops II's `MASK_PLAYERSOLID` plus one bit, 0x8000 (set on the
    player entity at spawn; exe notes); nothing on these four maps carries
    that bit.
* **Barrier check** (`t7_barriers`, headless): from every spawn and the
  ends of its open walks, the player box lifted a step sweeps 400 units in
  24 directions; a stop with nothing drawn within 12 units of the face it
  stands against is an invisible barrier, put down to a placed model (by
  name), a brush past its own corners, the map's own player clip (`*_clip`
  materials: Black Ops III's designers' clip, there in the game too) or
  the world mesh. Before -> now, invisible stops on placed models' collision
  (all invisible stops; sweeps):

  | map | lift | placed models | all invisible | sweeps |
  |---|---|---|---|---|
  | Nuk3town | 19 | 11122 -> 1370 | 19234 -> 15491 | 34512 -> 40488 |
  | Nuk3town | 60 | 9918 -> 1372 | 16586 -> 13998 | 34680 -> 41424 |
  | Sector | 19 | 1943 -> 310 | 4727 -> 3897 | 21696 -> 23088 |
  | Evac | 19 | 678 -> 561 | 2044 -> 1630 | 23832 -> 23640 |

  (More sweeps now: spawns and walks the props' misplaced collision had
  blocked.) No stop at a step's or a jump's height fell in a brush's
  missing-side corner, before or now: those corners lie under floors,
  inside walls and over the roofs, where a fall, a grenade or a bullet
  meets them rather than a walk. What is left is mostly the maps' own
  player clip (`*_clip` boxes over round tanks, planters and roof edges,
  full height over some props: Black Ops III's designers' clip) and props
  whose drawn mesh is streamed and missing offline (the check then sees
  nothing drawn). Metro's run did not finish on the build box (out of
  memory beside another job); its census is above. Static model axes on
  Sector: 3457 of 4878 transposed -> 9 (placements sharing an origin).
* **Contents** are Black Ops II's bits (the zone's clip materials name
  them): `clip_player` 0x10000, `clip` 0x430200 (player, AI 0x20000,
  vehicle 0x200, 0x400000), `nodraw` 0x30200, `clip_missile` 0x80,
  `clip_vehicle` 0x200, `clip_physics` 0x400, glass 0x10, water 0x20,
  foliage 0x2, `mantle_on` 0x1000000, sky 0x800. 0x2080 (missile and shot
  clip) is the drawn detail geometry players do not collide with (Metro
  8000 brushes and 74000 triangles of it): on the maps checked it floors
  only places no player reaches (Apartments' fake rooms behind the
  windows, Metro's train cars), so `MASK_PLAYERSOLID` (0x02810011) stands.
* **Drop test** (`drop_test_t7`, `T7_TEST_MAP=<zone.ff>[,...] cargo test
  -p assets --lib drop_test_t7 -- --ignored --nocapture`): where a player
  can get (8-direction 48-unit steps from every spawn, step 18, jump and
  mantle 60, falls of at most 300), then the player box dropped from 40
  over every spawn and every drawn floor on a 96-unit grid there: it must
  land within 48 of the floor (not out of the map, not through it), not end
  in solid; the match's own pmove dropped and run 2 s from 600 of them must
  not pass down through a drawn floor; every world player clip in the play
  space must stop the box walked through it. Per-map failure counts and the
  commonest causes by drawn material or model (`T7_STRICT=1` fails on any).
  `probe_t7` (`T7_PROBE="x y z;..."`) lists, at a point, the drawn floors,
  which contents bit stops the box where (brush or mesh), what holds it in
  solid where it lands, and the brushes round the column (world or a
  sub-model's, which zone leaf).

## Effects (status, 2026-10-23)

* **Placed effects** are the map's `"classname" "fx"` blocks: `fxdef`,
  `origin`, `angles`, `delay` (negative ms: how long it has run at map
  start), `fxstate1..4` (one per lighting state), `timescale`,
  `primaryLightFraction`, `guid` (Nuk3town 622; the server frees them, the
  client plays them). They are already converted by `assets::lane::t7_fx`
  into the engine's effect runtime (`asset_game::fx_catalog_t7`: the
  `FxEffectDef`/`FxElemDef` fields read off the zone against Black Ops II's
  layout); every placed effect is found on Sector (54 effects, 43 sprite
  materials drawable) and Nuk3town. `t7_map_fx` and `t7_fx_ents` list
  them.
* **Effect lights, as the exe adds them** (`fastfile_t7::scene_light`):
  a light element (type 8) holds a `LightDescription` (0x240: at 0x8 the
  renderer light it starts from; walked and kept by name,
  `walk::tables::light_description`, references found in the common
  zones). Each frame it draws (0x1415760a0) the exe copies that light,
  sets its colour (the element colour sRGB to linear times visual word
  0x18), size (0x6c, word 0x1c), cone (0x10c, cos of half of word 0x20, 90
  for a type 4 light), direction (0x9c, the element's forward negated) and
  the turned vector at 0x138, then sets it up (0x141d40280: a spot's cone
  terms 0x141d3ff20, the projection a type 3/4 light keeps at 0x00
  0x141d3fa00/0x141d44520, the radius 0x14c), drops it when its sphere is
  behind a cull plane and adds it to the frame's scene lights
  (0x141de5fd0: origin, 0x230, 0x234, 0x44, 0x208, bounds; 0x100 at most).
  The renderer copies them after the map's visible primary lights
  (0x141dddc20) and packs them with them (0x141db6e10): here
  `render_fx` publishes them each frame and `render_t7::lights` packs
  them after the map's. Nuk3town: 4 light elements (sparks, the car
  explosion), each with its description; the stored projection terms of a
  description match the port's. Not done: the per-client brightness for
  elements with flag 0x400 (taken as 1), the exclusion volumes of flag
  0x8000 (0x1415d54c0), a spot light's frustum matrix and corner bounds
  (its sphere's box is used; it holds the corners).
* **Not done** (not the exe's FX system yet):
  * the exe's own FX spawn/update/draw code is not ported: the runtime is
    Black Ops II/IW4's on Black Ops III's data (the field mapping is read
    off the data, not off the exe). The decompiler could not be run for
    it this session (it runs out of memory beside the renderer agents'
    builds);
  * lens flares (12) and decals (13) play silent;
  * distortion (heat haze, water rings) is left out; sprites are unlit
    (Black Ops III lights its smoke and sparks), so smoke and gas read flat;
  * placed effects now follow the client's radiant loader (0x14029a8e0,
    called from `CG_CallSpawn` 0x141106fc0 for `classname fx`): the record
    (0x40, array 0x1436df120) keeps origin, angles, the effect, `delay`
    (ms, added to the start time), `timescale` (+0x24, default 1.0),
    `guid` (+0x2c, hex), `outdoor` (+0x3a, effect flag 0x10), exploders
    (+0x38/0x39) and the `fxstate1..4` mask (+0x30, a missing key counts
    as 1). It starts at spawn (0x14029b060 -> 0x14159e180) unless it has
    exploders that wait; `script_gameobjectname` must name the mode. Here:
    `fxstate1` (the match's lighting state 1) gates, exploder effects and
    mode-bound ones are left out. `timescale` (1 on every Nuk3town effect)
    and `outdoor` are not carried into the runtime yet;
  * lens flares are their own system (`FxLensFlaresManager`: 0x141599870
    gives each type 12 element an instance in the effect's +0x138 table,
    0x14157fb10 spawns it, 0x141578200 moves and colours it each frame);
    not ported;
  * surface types for impacts and footsteps: Black Ops III keeps six bits
    at 20 (its impact code reads `(flags >> 20) & 0x3f`; its name table runs
    `bark` 1 .. `riotshield` 31 in Black Ops II's order, then `metalthin`,
    `metalhollow`, `metalcatwalk`, `metalcar`, `glasscar`,
    `glassbulletproof`, `watershallow`, `bodyarmor`, `opaqueglass` 32..40).
    The map's collision now hands the engine Black Ops II's five-bit type
    (`assets::lane::t7_map::t6_surface_flags`: the added nine as the metal,
    glass, water or flesh they are), so shallow water splashes instead of
    reading as dirt. Black Ops III's own impact tables are now
    used: an `ImpactFx` (`bullet_small_enter` ...) names a
    `SurfaceFxTable` (an effect per surface type 0..39 at 0x8 + 8 x type,
    the exe indexing it by `(flags >> 20) & 0x3f`) and an
    `EntityFxImpacts` (flesh: body/head, non-fatal/fatal from 0x38), kept
    by the walk (`Capture::impact_fields`, references resolved by slot) and
    read through `T7Map::impact_effects`. On a Black Ops III map those
    effects are converted with the map's and written over Black Ops II's
    impact rows (`assets::lane::t7_fx::IMPACT_ROWS`: small/large/xlarge
    bullets, shotgun, grenade bounce/explosion/dud, rocket, knife); mortar
    and tank shells and bolts keep Black Ops II's. Nuk3town: 38 of 40
    surface cells and all flesh cells named and found. Surface types
    32..39 index as their Black Ops II type (thin metal hits show the metal
    effect);
  * weather: Evac (`mp_apartments`) has no precipitation effect: its zone
    places only ground wind/wet effects (`weather/fx_wind_ground_wet_*`,
    82, already played) and its wet look is the world's weather grime
    volumes (`volume_weathergrime` surfaces, `GfxWeatherGrimeVolume`, the
    `gWeather*` maps the G-buffer programs blend). Now ported
    (`render_t7::weather`, from `141ddc190`): the view's grime volume is the
    first holding the eye (`141dd5bb0`/`141de4650`: no plane with
    `dot(n,eye)+d > 0`; a volume with no planes holds everything), and its
    numbers fill `LightingGlobals.weather` (0x260..0x2b0: +0xc, +0x14,
    sin/cos of +0x10 deg, +0x28, the directions of pitch/yaw +0x18/+0x1c and
    +0x20/+0x24, tints +0x2c and +0x38; no volume: zeros, cos 1, direction
    (0,-1,0), tints 1) every frame. Its six images (+0x48..+0x70) are what
    the exe binds as code textures 0x58..0x5d (`$white_gloss`, `$gray_color`,
    `$normal` without one). Evac: 6 volumes, the first with snow and grass
    images and weather/grime amounts 0. No program in `core_common`,
    `mp_common` or `mp_apartments` reads the weather block or those code
    textures (`weather` is marked unused in every RDEF), so it changes
    nothing on screen there and the G-buffer does not bind them yet. Lens
    flares, decals, distortion and lit particles remain.

### Effects on the GPU: lens flares and lit sprites (findings, not built)

**Lens flares** are drawn by the renderer from the `klf` assets
(`walk::fx::lens_flare`), not as effect sprites:

* init (`0x14157ef90`): a 28-vertex disc fan (27 segments) and its index
  buffer for the occlusion query; two 32-entry u32 UAVs,
  `lensFlarePixelCount` (`0x149f95a18`) and `lensFlareTotalAlpha`
  (`0x149f93128`), cleared by `lensflare_clear_buffers` (compute);
* per source, the query (`0x141582210`): binds the two UAVs beside the
  target (`OMSetRenderTargetsAndUAVs`, slot 6, 2 UAVs), draws the disc with
  code technique `lensflare_query` (`techsetdef_lensflare_query`: VS reads
  `modelInstanceBuffer`/`boneMatrixBuffer`, PS tests `floatZSampler` and
  adds to both UAVs at the source's slot, passed as a float at draw-state
  +0x830);
* the draw (`0x141d65290`): for each source in the backend list (+0x510,
  24-byte entries, count +0x1000; entry +8 = 0xff skips it) whose `klf` has
  elements (+0x92), binds the def's second buffer view (+0x68) and image
  (+0x98), then per element (stride 0x1f0) its buffer view (+0x1b8, the
  element's `LensFlareBuffer` at 0x1a8) and image (+0x1e0) and draws code
  technique `lensflare` element+8 times. `techsetdef_lensflare_effect`'s VS
  reads `gArtefactsStatic` (64 B), `gArtefactsDynamic` (64 B),
  `gEmitterStatic` (208 B, bound from backend +0xf9640) and the query's
  `lensFlarePixelCount`/`lensFlareTotalAlpha`, and builds the quads itself.
* **where the sources come from (found 2026-10-26):** lens flare sources
  are effect elements (the lens-flare element type, which our effect
  player keeps silent), not world lights. The element's update, behind a
  bool dvar, needs the element def's `klf` and its per-flare table: it
  evaluates the element's position from the def's origin ranges (the same
  `origin + t·velocity` the other elements use, the time eased toward
  `colour alpha·life·(1/255)` with a 300 ms ease: `t − d²/600` under 300,
  `that + 150` past it), puts it in the effect's frame (9 floats), and
  when the def's slot index is valid and the element's packed colour has
  RGB and alpha non-zero, appends the instance to the client's list
  (256 at most, an atomic counter per client) and writes the instance's
  colour word (+0x18), its 9-float frame and the element's three floats at
  +0x58..+0x60. So drawing flares needs the effect player to run
  lens-flare elements first (`render_fx`), then the manager's per-frame
  pass over that list.
* **missing:** the CPU writer of the 208-byte `gEmitterStatic` records and
  of the +0x510 source list (the manager's per-frame instance list,
  `0x14312d928`, 256 max, filled by `0x141578200`). Without them nothing
  can be drawn exactly, so lens flares stay off.

* **the source list (found 2026-11-03):** the frame function's
  `0x141ddf2a0` (bool dvar `0x1486beea8`, the one the element update
  checks) builds the view's list with `0x141581850` from the manager's
  per-client instance list (`0x14312d928`, counter `0x14312e928`, filled
  by the element update `0x141578200`): count at view +0x1000 / +0x1414,
  order at +0xe70, 0x34-byte sources at +0x1004. `0x141d4dca0` then writes
  each source's backend entry (+0x510 + i·0x18) and its emitter record with
  `0x141580490` (per flare element: the element's curve at the source's
  visibility, `pow`, scale and bias, the colour, the size words; up to 100
  elements) and uploads them (0x1900 bytes) to `gEmitterStatic`;
  `0x1415824a0` draws the queries (two per source, one per eye-state flag
  at +0x1014 / +0x1015). Still needed to draw one: our effect player
  running lens-flare elements (it keeps them silent), then all of the above
  (addresses in `docs/exe_notes/T7_RENDER_GAPS.md`).

**Lit sprites, checked 2026-11-03 (not promoted):** `BO2MP_T7_FX_LIGHT=1` on
Nuk3town (lavapipe, camera A, 640x360): no validation error, one compute
dispatch more, effects sim+upload +0.1 ms (the `spriteExtra` records); the
light maps read back finite (`t7_fx_check`: layers lit 2.5, 2.2, 1.4 red,
green, blue against 1.0 unlit, alpha 0). That is a large change of every lit
smoke and spark that nobody has seen on a GPU, so it stays opt-in until the
owner's screenshot (Nuk3town's chimneys and the Sector fires, with and
without the switch).

**Lit sprites** (smoke, sparks, `ei/effect_lit_*` technique sets): Black
Ops III lights them on the GPU. The CPU writes `spriteData` (64 B) and
`spriteExtra` (48 B) per sprite; `fx_compute_sprite_cull`
(+`_cull_down`/`_qtree`/`_clear`, reads `zBufferRough`, `spritePos` 32 B)
culls them; `fx_compute_sprite_light` (`_lmap`, `_light_shadow` variants)
lights each with the map's lights and probes (`LightingPackCBuffer`,
`RefProbePackCBuffer`, `RefProbeBlendPackCBuffer`, `visibleLights`,
`visibleProbes`, the probe volume `gProbeX/Y/ZArray`, `gCookieArray`, sun
and transparent shadow arrays) into `spriteDataW`; the effect technique
draws the result (slots 5/8 bind `lmapTex`, slot 6 only `colorMap` and
`floatZSampler`). Porting it means drawing effects through `render_t7` with
their own techniques instead of Black Ops II's sprite path, and finding the
CPU packer of `spriteData`/`spriteExtra`. Not built. Heat haze and water
rings are effect materials reading the refraction copies the forward pass
already makes (`resolvedPostSun`), so they come with that same effect path.

## Resuming the renderer from a RenderDoc capture of Nuk3town

Capture one frame on `mp_nuketown_x` (standing still, no transitions) and
read, from the deferred lighting dispatch and the passes after it:

1. **Targets:** the formats and sizes of the G-buffer render targets (code
   textures 0x2f–0x32), the depth buffer, the lighting outputs (0x33/0x34),
   the HDR scene target, the bloom chain, the 2D LUT and the final target.
2. **PerSceneConsts** (1488 bytes) as bound to `deferred_lighting_permuted`:
   in particular `hdrControl0/1`, `fogConsts..fogConsts9`, `sunFog*`,
   `skyMx*`, `sunMx*`, `colorMatrix*`.
3. **LightingGlobals** (944 bytes): the fog block (`fogConstants`), the sun
   block (`sunConstants`, to check `fastfile_t7::native`'s sun) and
   `globalProbeConstants`.
4. **PostFxCBuffer** (2016 bytes) at `exposure_compute`, `create_lut2dv`,
   `bloom_apply_lut` and `resample_final`: `postFxControl0/1`,
   `postFxConst00–60` (the four vision slots: compare each 15-vec4 block
   with the parsed `.vision` files to name the active visions) and the
   blend weights in `postFxConst60`.
5. The sampler states bound to the G-buffer pass for one material, to check
   the material sampler reading.

## `crates/render_t7`: the G-buffer pass as the exe records it (stage 1)

What the exe does per draw, and where `render_t7` does the same:

* **Draw method:** a technique has 8 draw methods; the draw takes the
  view's method byte (methods 0, 2, 3, 4 carry identical programs on every
  Nuketown technique; 1 a variant that also reads PerSceneConsts). Method
  0 is used. A method's byte at 0x4a picks the material's constant
  buffers: `technique * 6 + byte (+2 vertex, +4 pixel)`, bound at the
  buffer's device slot (= its RDEF bind point by name).
* **Arguments** (type high byte; low byte the stage, 0 vertex 1 pixel):
  0x200 material texture (texture table by name hash; the image's own SRV,
  its fallback view only while unstreamed), 0x300 material sampler
  (sampler table by hash, state `& 0x3fff`), 0x400 code constant (written at
  byte `location` of constant buffer `buffer`, `size` bytes; u[0] low byte
  the code constant index, table of 222 names in `render_t7::code`), 0x500
  code texture (u[0] index: 0x4f `resolvedNormal`, read by every decal),
  0x600 code buffer (6 bone matrices, 7 model instances).
* **State map** (`GfxStateMap` 112 bytes, per technique at +0x2c8): eight
  blend words, depth word, two stencil words, raster word, stencil ref;
  decoded with the exe's own tables (`render_t7::state` documents the
  bits). Ordinary draws use the plain variants (blend, depth, raster with
  depth clip); the exe also builds an inverted-depth/mirrored set.
* **Samplers:** the exe's sampler cache turns the 14-bit state into a
  `D3D11_SAMPLER_DESC` with its filter table, address table (wrap, clamp,
  border, mirror), max LOD 0 without mip bits, anisotropy from a table by
  texture-filter quality (`render_t7::sampler`).
* **Input layouts:** per vertex declaration (count, component ids) twelve
  layouts, one per layout group, from the exe's element table (id → slot,
  step, offset, DXGI format) plus `TEXCOORD15` (R32_UINT, slot 15, per
  instance). World surfaces are group 3 (positions 12 bytes; colour, UV,
  10:10:10:2 normal and tangent in 20 bytes), static models group 1
  (positions; 16-byte colour/half UV/normal/tangent; 12-byte skin weights).
* **Instances:** `modelInstanceBuffer` 24 bytes (+0 first bone of a skinned
  draw, 0 none; +12 the bone matrix placing it, 0 none = identity);
  `boneMatrixBuffer` 64 bytes, three rows `rotation | translation`. World
  surfaces take instance 0; each static model an instance whose matrix is
  its placement (axes × scale, origin).
* **Targets:** GBufferDiffuseColor R8G8B8A8_SRGB, NormalAndGloss and
  SpecularColorAndOcclusion R10G10B10A2, depth D32S8 cleared to 0 (reversed
  depth, tested greater-equal by the state maps). The G-buffer programs
  write three targets. Decals (programs reading `resolvedNormal`) draw after
  the normal target is copied to it.

Checked offline (`cargo run -p render_t7 --example t7_native_check --
<zone>/mp_nuketown_x.ff`, Vulkan software device), after every staged
zone walks to its end (`core_pre_gfx` 215/215, `core_gfx` 306/306,
`core_common` to its end; readers for the default one-off assets and for
`sound_patch`/`lightdef` taken from the exe's own load functions): all 271
G-buffer pipelines create and validate against their programs' RDEF; of
6122 draws, 5416 bind and a frame records and submits without validation
errors; the other 706 are materials whose technique sets have no G-buffer
technique (emissive, glass, skin, water, transparent: drawn by passes not
built yet), 7 of them in `mc/lit_emissive`, a technique set defined in no
staged zone. 397 images those 5416 draws bind are bound 1x1 for the check
only: 395 are streamed (`.xpak` only; 379 named by the map zone, 13 by
`mp_common`, 3 by `core_common`) and 2 (`$gloss`,
`i_t7_micro_metal_brushed_01_n`) have no record in any staged zone. The
list is in `docs/t7_native_missing_images.md`. On the PC the scene also
searches every other shared zone beside the map (`core_*`, `code_*`,
`*patch*`, `common*`) for a technique set the common zones lack.

Not yet in stage 1: the depth prepass (technique slot 2), model LOD
selection by distance (the finest LOD is drawn), `gameTime` (code constant
3, one scrolling material) — its value as the exe computes it is not traced.

## Post chain (found 2026-10-22; `render_t7::post`, `post_exposure`)

**Order (the post orchestrator):** colour-grade LUT build
(`create_lut2dv`, or `_alt`) → bloom downsample chain → bloom blur x
(`bloom_blur_mip_x_l5..l1`, five passes walking the bloom render-target
roles down) → bloom blur y → eye exposure (`exposure_compute`) →
`bloom_apply_lut` (bloom + grade) → (AA) → `resample_final`. Each pass is
a full-screen code technique drawn into a render-target role of the view.

**Bloom downsample chain:** l0 (`bloom_downsample_l0`) → l1
(`bloom_downsample_l1`, or `_scene` when a dvar and the view's flag
S+0x1d30 are set, or the `_adsblur`/`_adsblur_scope` variants when ADS
blur is on, the view isn't a 3D-UI view and its blur amount > 0; scope
when that amount exceeds a threshold) → `bloom_downsample_lx` for the
smaller levels → `bloom_downsample_ev` into the luminance target that
`exposure_compute` reads. Two extra `lx` levels run when the scene target
is taller than 1080 (and 2160) rows; the blur starts one or two levels
lower then.

**Eye exposure (ported, `post_exposure`):** the CPU side works out the
view's EV each view from the current lighting state's
`GfxGlobalLightSettings` (+0xb0 bias, +0xb4 lowest, +0xb8 highest EV),
the exposure volumes the eye is in (`GfxWorld` +0x128/+0x130 records of
0x6c, planes +0x138/+0x140; per state a u16 `v/1024 - 32` EV at +0x30
min, +0x38 max, +0x40 bias, 0xffff unset, and an adaptation scale at
+0x58; box falloffs +0x18..+0x2c by the plane's axis; weight 0 inside, 1
at the falloff's edge, sorted by record +0) and the luminance metered at
the eye (the sun volume's exposure grids: **not ported**, an input). The
metered luminance × 2^bias is clamped to 0.18·2^(min−3)..0.18·2^(max−3),
eased (`k = exp(−(1/60) / lerp(0.1, 0.4, 0.04/(L+0.04)))`) and averaged
over the last eight views; EV = log2(avg/0.18) + 3, scale 2^(EV−3).
`postFxControl0 = (min, max, bias, meter scale)`, `postFxControl1 = (EV,
scale, adapting ? 1 : 0, view index + 1)`. `exposure_compute` runs as one
8×8 group (`Dispatch(1,1,1)`) over two RGBA16F UAVs: u0 the luminance
target, u1 the view's exposure history. The zone's program translates and
runs offline (software GPU).

**Bloom settings per view (ported 2026-10-26, `post_bloom::view_settings`).**
The view setup zeroes the 80-float settings and adds records into their
first 72 floats (the last 8 stay zero): the bloom modifiers (`GfxWorld`
0x1b40 count / 0x1b48 records of 0x180: the colour vision's modifier weight,
mask at +0xc, strength +0x1c, settings +0x40), weights capped to a total of
1 and done when they reach it; then the box list at 0x1b50 and, if the total
is still under 1, the one at 0x1b60 (0x1d0 records: the colour vision's box
weight, falloffs +0x60, strength +0x78, mask +0x7c, settings +0x90), each
list's weights scaled to what is left (and by 1/sum when they sum past 1;
the running total takes every product, only positive ones are added); then
the base record of the lighting state (`+0x1640 + 0x140·state`) with what
is left. Mask: the lighting state's bit `1 << (4 + state)` and the
client's. Offline (`T7_BLOOM_ZONE=<map.ff> cargo test -p render_t7 --lib
post_bloom -- --ignored`): mp_apartments' three interior boxes halve the
bloom (`postFxBloom00` 0.5 → 0.25) inside them; Nuketown and Sector have no
bloom lists (base only). Log line on each change: `t7 bloom: view settings
from ...`.

**Not done yet:** the exposure-grid metering, the LUT build's constants
(postFxControlD..F from S+0x24..0xa8), the bloom passes' constants
(postFxBloom00..25) and target sizes, `bloom_apply_lut` and
`resample_final` bindings.

## `render_t7::forward`: the non-G-buffer surfaces (2026-10-23)

After the lighting the frame driver draws its draw lists in a fixed order
(14, 7, 8, [18], 9, 10, 20, 21, 22); each list names the technique slot its
draws use. The lists that carry the surfaces with no G-buffer technique
(emissive, glass, transparent, water, sky) use slot 255 -> the default
technique byte, which the exe sets to 6. Lists 16, 17 and 22 are sorted by
the draw's sort key and then walked back to front; the others by key.

Built: each material whose technique set has no slot 3 (G-buffer) technique
takes its slot 6 technique; those draws are recorded into the lit scene
(load, store; the scene depth tested) after the lighting and before the
post chain (`Host` records them there), sorted by the material's draw
sort key template (MaterialInfo +0x10, now read by the walk). The scene
caches one technique per (technique set, slot), so a technique makes one
pipeline. Bound: material textures, samplers and constant buffers as the
G-buffer pass does; the lighting stage's code textures, buffers and
samplers by name; `gEnvBRDFGlass` = core_gfx's `$env_brdf_glass`. The
light, probe and decal lists and their constants (`visibleLights`,
`visibleProbes`, `visibleDecals`, `lightConstants`, `refProbeConstants`,
`refProbeAttenuationConstants`, `forwardDecalConstants`) and the decal
material texture arrays (`materialTex*`): the light and probe packs are
the lighting stage's (`LightingPackCBuffer` / `RefProbePackCBuffer` /
`RefProbeBlendPackCBuffer`, read as structured buffers), the per-cluster
masks the light culling's (below); decals are not packed, so their list
and constants (and the `materialTex*` arrays read only through them) are
bound empty, as the exe's are with no decals (it skips their culling when
the count is 0).

Light culling (the exe dispatches it before the forward lists):
`light_culling_probes` then `light_culling_lights` (core_patch compute
shaders), each only when its count is non-zero, in threads (tiles x,
tiles y, 16 x ceil(count / 32)); a cluster is a 64-pixel tile and one of
16 view-depth slices of 128 units (the last to 131072); each writes a
R32Uint 2-D array mask (layer = slice x 16 + chunk of 32) to
`visibleLights` / `visibleProbes`, from `gCullConstants` (the lighting's
cull records: lights, then probes), `LightingGlobals` and its own
`PerSceneConsts` (filled by the culling programs' arguments: the inverse
view matrix and target size). `t7_forward_check` reads them back:
mp_sector at 320x180, 73 lights and 77 probes packed: 467 light bits in
162 words, 875 probe bits in 322 words.

The refraction inputs, as the frame makes them (`141d4e220`): before
view lists 20 and 21, when the list has draws, the lit scene is copied
into the resolve target and bound as `resolvedPostSun` (code texture 0x9;
`141d4b470` -> `141d4b2a0`), with `floatZ` (code texture 0x10) the scene
depth; between lists 20 and 21 the water pass (`141d4ced0` ->
`141d4c090`) runs `downsample_depth_4x_compute` (floatZ -> an R32_FLOAT
quarter-size target, `postFxControl7` = (4, 0, 0, 0), `postFxConst00` =
(1 / width, 1 / height, quarter width, quarter height), threads = the
target's pixels) for `floatZDownscaled` (code texture 0x11). Built so;
the draws that read `floatZ` while drawing with depth read a copy of the
depth taken before their pass (wgpu cannot sample the depth a pass draws
into). The exe runs the downsample only when the view has water (a dvar
and the view's water list); here it runs every frame. `dxbc_sm5`
translates `sample_pos` (zero for a resource that is not multisampled).
mp_sector: the water draws (`lit_transparent_water_flow`,
`lit_water_sim_flow_normal`) are ready; 361 of 396 forward draws.

The sky (found in its vertex program, `sky_latlong_hdr`): the vertex's
world position minus `eyeOffset`, normalised, scaled by
`|skyRotationTransition.z|`, through `viewProjectionMatrix` with clip z
= 0 (the far plane under the reversed depth): the sky follows the view
and sits behind everything whatever the model's placement, and
`skyRotationTransition.xy` turns its lat-long lookup. The exe writes
`skyRotationTransition` per view (a view-constants writer copies it
from the scene's sky state; the source not traced yet).
The sky: the sun volume's streamed lat-long picture is put in as the sky
material's colour map (mp_sector: 6144x3072 BC6H), and the sky box models
are added as draws (identity placement).

`t7_forward_check` (software GPU): mp_sector 25 pipelines, 0 failed, 358 of
396 forward draws ready, the frame records without validation errors;
mp_skyjacked 307 of 1046 before the decal arrays were bound empty.

Not yet (left out, not substituted):
* the view lists: the scene traversal adds draws to allocation lists,
  which the frame merges (by key) into the view's lists: view 7 <- 0, 1;
  8 <- 3, 4; 9 <- 0x17..0x19; 10 <- 0x1a, 0x1b; 14 <- 0x13; 22 <- 0x15;
  (G-buffer and others: 0 <- 5, 2 <- 7, 8, 3 <- 9, 4 <- 0xa, 5 <- 0xe,
  11 <- 0xb4, 12 <- 0x20, 13 <- 0x21, 15 <- 0x14, 23 <- 0x16). World
  surfaces go by their flags byte (+0x50): 0x20 -> 0x17, 0x10 -> 7, 2 ->
  0x1a, none -> 0; `scene::Draw::list` keeps the view list and the
  forward draws are recorded list by list in the frame's order (14, 7, 8,
  9, 10, 20, 21, 22), each by key, 16 / 17 / 22 reversed (back to front).
  Not decoded: which list a model surface goes in (static models and the
  sky are put in list 7; the model path's 0x2a / 0x2b lists are not among
  the main view's), so nothing lands in list 22 yet;
* the sky: drawn with the sun volume's first sky box (lighting state 0).
  Its model's mesh streams from the base packages (entry
  `0xe08bdfa374b1b242`, shared by the sky boxes), read at run time beside
  the map zone; where they are missing the log says "sky not drawn" with
  the entry (offline here: `T7_SYNTHETIC_SKY_MESH=1` gives the mesh zeroed
  data to check the path: mp_sector's sky draw makes its pipeline, binds
  and records cleanly). `skyRotationTransition` as the exe's view setup
  writes it: (sin a, cos a, the sky box's +0xc, a dvar defaulting to 0),
  a = the sky box's rotation (+8, degrees) plus a rotation dvar (default
  0) plus atan2 of a 2-D vector of the backend view (+0x2ec, +0x2f0),
  the client refdef's +0x2ec / +0x2f0, which only two script builtins
  write (`140412770`, `14040ac90`, one component each from their float
  argument): zero, so no turn, until a map's script calls them. Script
  sky rotation (those builtins) is not driven yet;
* refraction (done 2026-10-23): `resolvedPostSun` is the lit scene copied
  into the resolve target before lists 9, 10, 20 and 21 when a draw of the
  list reads it (`141d90700`, `141d4b2a0`); `shaderConstantSetBuffer` (code
  buffer 15: `wc/glass`, `glass_backlit`) is the draw state's +0x1cd8 from
  the backend's +0x96380 (`141df0cf0`): 384-byte constant sets uploaded
  each frame (`141d4e220`), added by effects and entities through the
  allocator (`141dea080`, `141575ca0`) whose count restarts at 1 every
  frame (`141dcb3c0`), so set 0, the one world and static model draws
  carry, is never written: bound as zero. Sets for effects and script
  entities are not filled (no effect or entity draws here use them).
  mp_sector: 396 of 396 forward draws ready;
* the depth prepass (technique slot 2): no list of the main view selects
  slot 2 (the list table's slots are 3/4, the default 6, and 11 for
  list 11, which the exe draws before the light culling); slot 2 belongs
  to another view (shadow maps are the likely one, not checked), so the
  main frame has no slot-2 pass. The material keeps its slot-2 technique
  (`Material::prepass`) for that;
* model LOD selection: the finest LOD is drawn. The model header keeps,
  after its LOD count (+0x40), two floats (+0x44, +0x48, e.g. 0.25 / 5)
  and from +0x64 one decreasing threshold per LOD (e.g. 36.3, 18.1, 9.5,
  ...: screen sizes rather than distances; `t7_lods` prints them); the
  exe's selection code is not found yet. Searched 2026-10-28 (not it):
  every scalar SSE load/compare of `[base + index*4 + 0x64]` in .text
  (only the effect sim's global table: 0x14157311a, 0x14157387b,
  0x141593ce0), compare chains over +0x64/+0x68 (bounds tests at
  0x1421f75aa, 0x1421f8eb5, 0x142771a39), `numLods` byte reads near them
  (portal queue 0x141d83a50); no compute program selects LODs; the
  `r_lodScaleRigid` / `r_lodBiasRigid` / `r_modelLodLimit` name strings are
  only in a name table (0x1413e5290), the dvars are reached by hash.
  Found and ported 2026-11-03: `render_t7::lod` ("Graphical gaps" below).
* draw merging (measured 2026-10-28, `t7_cull_check`): Sector's 4209
  G-buffer draws are 1171 runs of one material and source in draw order
  (923 distinct pairs), Western 3303 / 1168. Multi-draw would cut the draw
  packets ~3.6x but not the state changes, and wgpu has no multi-draw in
  render bundles (the pass would be recorded every frame, ~1 ms CPU);
  not done;
* decals not in the G-buffer, effects and particles.

## GPU resource audit (2026-10-23): poison mode, bounds audit, what it found

Offline tools (all off in the game; `crates/render_t7/src/audit.rs`, run with
`t7_host_check <zone.ff> <w> <h>`; `T7_STAND_IN=1` binds pictures missing
from the packages at hand as a grey 1x1 so every draw runs):

* `T7_POISON=all` (or `storage`, `uniform`, `tex`): every buffer the renderer
  makes without contents starts as the quiet NaN `0x7FC0DEAD` (not the
  indirect arguments; `gSunShadowTree`, the empty lists and the code
  instances are zero by design), every texture magenta (unsigned integer
  formats `0xDEADBEEF`). After the frames `scan` reads back every texture
  (all mips) and buffer and prints what nothing wrote, with the box of it.
* `T7_AUDIT=1` (set by `T7_POISON`): `dxbc_sm5` `Options::bounds_audit`: each
  translated compute shader counts, per buffer register and read/write/atomic,
  the accesses outside the bound buffer (`OOB <program>: u7 write: n words`).
  Proven by `T7_TEST_CULL_WORDS=16` (the old half-size `gDeferredCullBits`):
  `deferred_permute_classify_swizzle: u7 write: 9093 words` at 640x360.
* `T7_LIMITS=default`: the offline device gets wgpu's default limits
  (storage textures per stage 4, ...). The game's device is bevy's with
  `WgpuSettingsPriority::Functionality`: the adapter's own features and limits,
  so this mode only shows what a weak adapter would refuse (it flags
  `deferred_permute_uber_resolve_gtao`: 5 storage textures).

Results (Sector at 1280x720, 1920x1080, 2560x1440, 3840x2160, 1366x768,
1601x901, 641x363; Skyjacked and Evac 1920x1080, 1601x901, 3840x2160): no
out-of-bounds buffer access in any translated compute shader; every texture
in the chain written where it is read (the bloom targets the 1440p+ shift
leaves unused, the AO targets and light-cull masks off the covered tiles, the
luma target's white texels equal the poison byte); the cull-bits, tiling and
extents buffers sized right at every resolution; constant buffers carry no
field the shaders read and nothing set.

Fixed with it:
* the lit scene and the resolved normal were never cleared: the lighting
  writes only pixels with a surface, so a pixel with none (sky not drawn
  over it) kept an earlier frame's value (trails, stale sky). Cleared by a
  render pass each frame (no `clear_texture`: device feature).
* G-buffer draws skipped for resources the frame does not make, now bound:
  the weather grime maps (code textures 0x58..0x5d) as the exe's neutral
  defaults (`lit_micro_tile_grime_mix(_plus)`, the terrain/wall blend:
  Sector 64 draws, the black voids of a grime-blended ground);
  `shaderConstantSetBuffer` (code buffer 15, zero: `lit_fxanim_cracks`,
  Skyjacked 64 draws); `gpuSkinBase/Quat/Pos` (code textures 0x2c..0x2e, a
  black 2-D view: flags, cloth, wires, vegetation; Evac 64+ draws). The
  forward pass binds the weather maps the same way.
* the motion pass's `PostFxCBuffer` is zero by write, not by luck.
* present: a unit test places a viewport inside a larger odd-sized target
  pixel for pixel.

Still needs a PC log or a capture: the device limits line; `material ...:
technique set mc/lit_emissive#e6142445 in no zone read` (a stub in every zone
here; the emissive screens and lights are missing unless the PC's zones
hold it); a volume's own weather images are not rebound per view; the
explosion overlay (effects are not drawn by `render_t7`; the effect lights
pack as the exe's; nothing offline reproduces it); a full-screen frame at
4K on a real GPU (offline frames cover few tiles).

## Missing floors and extra surfaces (2026-10-26, `render_t7::surf_debug`)

Found in the exe (decompiled, Ghidra):

* World surfaces for the main view are collected by `141d70780` (job
  `141e16d30`, type 0 / 1) through `141d703b0`, which walks a surface index
  range, takes a surface only when its visibility byte (GfxWorld +0x370 array)
  is set, buckets it by the flags byte +0x50 (0x20, 0x10, 2, else 0 or, with
  the view's flag, 0x40) and merges consecutive surfaces of one material.
  Type 0 fills allocation lists 0, 0x17, 7, 0x1a from the ranges at dpvs +0xc
  / +0x1c, type 1 lists 0x1c, 0xa, 0xf from +0x14 / +0x24 (flags 0x20 and 2
  dropped there). The dpvs block is GfxWorld +0x2f8 (its +4 is the static
  model count at +0x2fc, its +0x138 the surfaces at +0x430), so the ranges
  are GfxWorld u32 pairs at +0x304 (A), +0x30c (B), +0x314 (C), +0x31c (D).
  The scene draws every surface (all ranges); which views type 1 serves is
  not traced, so the range filter is a switch (`main_ranges`), not the default.
* Static models: the sun shadow traversal `141d71b00` (three cascades, lists
  0x32..0x34, from `141d72710`) and the spot shadow one `141d71d90` (lists
  +0x53 / +0x9b, from `141dafde0`) skip a draw record with +0x60 bit 0 set or
  +0x70 non-zero, and pick a LOD with `14241b9b0`
  (model +0x58, +0x84, distance to +0x34, +0x30), -1 = not drawn. The main
  view's model walk was not found; the scene draws every placement at its
  finest LOD. `141d7d960` (map load) writes +0x71 (a material flag of any
  LOD surface).
* A draw with any one material picture missing was left out (G-buffer and
  forward). Now stood in for by binding (`surf_debug::stand_in`); colour maps
  only on surfaces that neither discard nor blend.

The log's `t7 surfaces:`, `t7 stand-in:` and `t7 big draw:` lines (once per
map) give every draw's outcome; `CODLEGACY_T7_DEBUG_SURF` (see the module) turns
the suspect filters on and off.

**Opaque forward surfaces' depth (2026-10-27).** A material with no slot 3
technique whose slot 6 technique neither blends nor writes depth (it tests
greater-equal) is drawn by the exe over the depth its slot 2 technique laid
first. `render_t7` drew no slot 2, so such a surface left no depth and a
forward surface behind it drew over it: Nuk3town's road (`p7_ntx_road`,
`lit_emissive_scroll_3layer_plus` / `lit_emissive_plus`) under the vista
water, the cyan plaza. Now the G-buffer pass draws those materials' slot 2
(`Material::forward_depth`; the `gbuffer:` log line counts them;
`T7_FORWARD_DEPTH=0` turns it off). Which surfaces the exe's depth list takes
was not read off its list builder; the rule above is the state maps'.
`t7_surf_at` (CPU: what lies under points of a map, model entities and
unbuilt placements there, draws' states) and `t7_floor_check` (the G-buffer
pass alone, its depth read back) found it.

## Debug views and the 2026-10-23 lit-scene findings

Findings (from the exe, `141ddd1b0` / `141d42240` / `141d43c20`, and offline `t7_lighting_check` on mp_sector):

* **Ambient was zero.** sunConstants 8..11 (LightingGlobals 0x130..0x13f) are the
  global probe's config words 0..3 (units, tint rgb), copied into the view by the
  view setup and from there by the sun builder. They were left at zero; every
  permutation multiplies the probe volume weights by float 8 and takes the specular
  occlusion bound from it, so all indirect diffuse and specular was black (43% of
  mp_sector's pixels exactly zero offline, independent of the GPU). Fixed in
  `lighting_globals::build`; the first-frame log prints `lighting: globals:` (sun
  floats 0..11), `fog constants at the eye` and `relHDRExposure` to check it.
  Shadow mode 2 uses FLT_MAX cascade bands, so no shadow map is sampled; empty shadow
  maps are not the cause of anything seen.
* **Sky black on a far eye.** The sky VS (`mc/sky_latlong_hdr`) ignores the instance
  placement matrix: it skins with the instance's word 0 as first bone (only when
  nonzero), then `normalize(pos - eyeOffset) * |z|` at clip z = 0. The exe draws the
  shared generic sky mesh skinned to a bone at the eye. We pushed word 0 = 0, so
  an unskinned mesh at the world origin collapsed to one direction once the eye was
  far from the origin (Skyjacked). Now `Scene::read_sky` pushes `[bone, 0, 0, bone, 0, 0]`,
  `GBuffer::update` writes the bone matrix (identity + eye translation) every frame, and
  a sky model with no skin data gets rigid weights. Offline: mp_sector with
  `T7_SYNTHETIC_SKY_MESH=1` and a far `T7_EYE` now shows the sky.
* **Missing images.** `T7Map::image` falls back to the other zones of the folder
  (`other_zones_image`, walked once on the first miss, logged as `t7 image: NAME is
  defined in zone Z`); `BO2MP_T7_NO_IMAGE_FALLBACK=1` turns it off, `T7_IMAGE_WHY=1`
  logs why each image fails. `i_generic_lookup` is core_common's (an xpak part in
  core_common's package); the others need their packages in the folder.

`BO2MP_T7_NATIVE_DEBUG=<mode>` (one at a time; `gbuffer` and `lit` as before):

| mode | what it does |
| --- | --- |
| `sky` | no world draws: only the sky and forward lists over the cleared depth |
| `ambient`, `probe` | sun colour zeroed: probe volume + reflection probes only |
| `sunonly`, `shadow` | probe volume diffuse weight (float 8) zeroed: sun + local lights + specular probes |
| `tiles` | readback at lit frame 30: tiles per permutation, tiles in no list, per-permutation count of wholly NaN/Inf and wholly black tiles, the first bad tiles with their depth extents |
| `cullbits` | as `tiles` plus the cull bits' set-bit statistics per tile and for the bad tiles |
| `nan` | `tiles` without the cull bits (NaN/Inf and exactly-black share of the lit scene) |
| `exposure` | same readback: mean/max luminance of the lit scene and what relHDRExposure makes of it |
| `blackpatch` | everything above |

`BO2MP_T7_DEBUG_PNG=<file>` also writes a picture of the lit scene classification
(permutation colour per tile; magenta NaN/Inf; cyan exactly black). Decoding of the
tile lists follows `deferred_permute_classify_swizzle` (word 0 & 0xfff = tile x, word 1 &
0xfff = tile y). Offline: `T7_FRAMES=34 BO2MP_T7_NATIVE_DEBUG=nan t7_host_check <zone> 640 360`.

### 2026-10-23: 8x8 stair-step blocks at silhouettes (cause: swapped probe blend words)

Symptom on the PC: whole 8x8 tiles along silhouettes and depth discontinuities lit differently
(brighter, warmer, or black) from the same surface in the next tile. NOT an empty-pixel problem:
`BO2MP_T7_NATIVE_DEBUG=empty` shows no pixel with depth 0 holds a lit colour (the permuted
programs reject a pixel whose `GBufferNormalAndGloss.z` is not above 0; the G-buffer and the lit
scene clear to 0; the uber resolve's tile extents ignore empty pixels), and with the sky drawn
(`BO2MP_T7_KEEP_FORWARD=1`) the empty pixels match across tile borders to 1e-3. The sky/silhouette
tile borders on the PC frame show no jump either (`none | 37` borders, 0.0005 median).

Cause: tiles that hold a local reflection probe in their cull bits take a permutation with the
probe loops (edge tiles hold more of them: their depth range is wide). `probes::record` packed
the probe's blend word with count and start swapped (`o[30]`: low 16 bits start, bits 25..30
count), so each local probe had zero blends, i.e. weight 1 over the whole tile instead of its
box falloff. Measured on the PC frame (permutation colours from `dbg_blackpatch.png` against the
final image): tiles of permutation 35 / 42 were 1.97x / 1.27x brighter and warmer than the
neighbouring 37 tiles; all other permutations agreed to 0.5%. Offline (Skyjacked, 640x360)
the same tiles were black, and `T7_NO_PROBES=1` removed every seam.

Fix: the two words un-swapped. Offline seam rate (same-surface pixel pairs across a tile border
whose lit luminance differs by over 1.25x) went from 8.87% (inside tiles 0.00%) to 0.23% (inside
tiles 0.26%), i.e. no tile signature left; Sector shows none either way.

Debug (all in `debug_views`, one readback at lit frame `BO2MP_T7_DEBUG_FRAME`, default 30):
`BO2MP_T7_NATIVE_DEBUG=empty` (or `BO2MP_T7_SEAMS=1` with any other mode, e.g. `sunonly`) logs
`debug: empty:` (lit pixels with no surface, black surface pixels by permutation, mean lit
luminance by permutation, empty-pixel seams) and `debug: seams:` (same-surface pixel pairs
across a tile border against inside tiles, and the per-permutation-pair jump table);
`BO2MP_T7_DEBUG_PNG=<file>` writes grey surface / blue black surface / red lit-empty.
`empty` skips the forward lists so the lit lists alone are read; `BO2MP_T7_KEEP_FORWARD=1`
keeps them (sky drawn). `T7_NO_PROBES=1` / `T7_NO_LIGHTS=1` drop the local probes / map lights.

## Load and frame time (2026-10-23)

The old converted world is not built when the native renderer is on (Options > BO3 NATIVE RENDERER, `frame::settings::T7_NATIVE_RENDER`; `BO2MP_T7_NATIVE=0` forces it off, and then the old path runs as before).

### Load path

- `match_walk` (t7 arm): `native` -> `fastfile_t7::shared::keep(true)`, `t7_native_load::set(PENDING)`, `map::expect_handed(zone)`, `set_t7_native_zone(Some(zone))`; `lane::t7_map::load_t7_world_for(.., native)` builds only what play needs (collision, spawns, entities, effects, audio, sky data, and a lighting-only draw with every surface hidden, so players still get the map's light), then `map::hand_over(zone, map)`. No looks, surface materials, prop meshes or world colours are made for drawing. A failed native load sets FAILED and cancels the hand-over.
- The loading screen holds (`world_gpu::poll`: `ready = live && !t7_native_load::pending()`) until the native renderer has drawn its first finished frame (`draw_t7_native` sets READY). `pending()` gives up after 15 minutes. There is no frame of the old world.
- `render_gpu::drawsurf::t7_native::load_t7_native` (view-less, Render/Prepare) starts `render_t7::prewarm::start` and a scene thread (`Scene::load` waits on `map::wait_handed`: the zone is read once). `Host::record` returns false until the load thread has made the pipelines, pictures, lighting stage and post chain.
- Shared zones (`SHARED_ZONES`, 10) are read side by side (`shared::preload`), once per process (`shared::zone` is a per-path `OnceLock` cache). `Scene::load` lets all but core_common/mp_common go, the load thread lets those go after the post chain; core_patch stays (programs are asked for again on a resize).

### What is made side by side

`render_t7::par::map` (workers: `BO2MP_T7_THREADS`, else cores - 1, 2..12). `render_t7::memo::get(device, key, build)` keeps compiled programs per device, so work made early (`prewarm`: the 64 permuted lighting programs, resolve/clear/classify/draw, post code passes, LUT, culling, depth downsample, all needing only core_patch) is found by the stage that needs it. `render_t7::warm::run` makes the G-buffer and forward pipelines (one per distinct technique and group) and decodes and uploads the pictures the draws bind, all after the scene is read and before the first frame. DXBC to WGSL is only about 0.4 s in all, so no WGSL or pipeline cache on disk was built (the pipeline cache is Vulkan-only; shader module and pipeline creation is the cost).

### Frame time

- Bind groups are made once per (material, group); code-constant buffers are shared per technique and binding, and written only when changed.
- G-buffer draws are recorded into render bundles, one per run of one stencil reference (`T7_NO_BUNDLES=1` records directly); model placements are one instanced draw per model surface.
- Per-pass timing (`render_t7::timing`): GPU timestamps inside the encoders, read back through a 4-slot ring. A line in the log (`t7 native: frame ms (GPU, mean of N frames over Xs): gbuffer .., clear+resolve .., motion .., ao .., lighting .., forward .., post lut .., bloom .., exposure .., apply+final .., present ..; total ..; CPU recording ms/frame (...); frames so far`) every 5 s, the first after 3 s (`T7_TIMING_EVERY=<s>`). `BO2MP_T7_TIMING=1` also prints the load's stage laps. Needs the device's `TIMESTAMP_QUERY` and `TIMESTAMP_QUERY_INSIDE_ENCODERS`; without them only the CPU part is logged.

### Measured (offline, lavapipe on 3 cores, 640x360, `T7_STAND_IN=1 T7_IMAGE_MAX_DIM=256`, 3 frames, wall seconds)

| zone | before | after | first finished frame (after) |
|---|---|---|---|
| mp_sector | 56.4 | 38.4 | 35.4 |
| mp_skyjacked | 54.4 | 34.4 | 32.5 |
| mp_nuketown_x | 54.4 | 38.4 | 35.8 |
| mp_apartments | (the old harness crashes, exit 139) | 34.3 | 31.4 |

After, on Sector: map zone read and walk 8.8 s (before, the shared zones were read again after it, and the converted world was built on top), pipelines (86 G-buffer + 25 forward) and pictures made side by side in 12 s wall (23 s of work on 2 threads), lighting stage 0.5 s, post chain 0.4 s (their programs were made while the zone was read). CPU recording per frame fell from about 150 ms (first frame) to about 4 ms. Peak memory offline is about 1 to 2 GB higher than before (parallel software pipeline compiles; the shared zones are held until the scene is made). The image is the same to the eye: about 8 % of pixels differ by a few levels along fine geometry (the instanced draws change the order of coplanar surfaces; the old path differs from itself in about 0.1 %).

### Offline measurement

`cargo run --release -p render_t7 --example t7_host_check -- <zone>.ff <w> <h>` (lavapipe; `T7_STAND_IN=1`, `T7_FRAMES=n`, `T7_HOST_PNG=<file>`, `T7_NO_PREWARM=1`, `T7_NO_BUNDLES=1`, `T7_IMAGE_MAX_DIM=<n>`). It prints the `timing:` lines. The offline numbers are on a software GPU: pipeline creation is far slower there than on a driver, so read the proportions, not the seconds.

## Performance (2026-11-03)

### Measuring

- `BO2MP_T7_PROFILE=1` (game or offline): a `t7 profile:` line every `T7_PROFILE_EVERY` seconds (default 5) with the mean per frame since the last line: CPU ms for the whole recording and per phase (frame begin, G-buffer constants, G-buffer record, exposure, lighting update, lighting record, forward constants, fog, effects sim+upload, forward record, post; `submit` in the offline tool, the game's engine submits), then draws (a G-buffer bundle counts the draws it holds), dispatches, render and compute passes, pipeline and bind group switches, bytes written through the queue, and the G-buffer draws and instances the GPU culling leaves for the view (counted on the CPU with the same spheres and planes). `render_t7::profile`.
- The GPU side per pass is the existing `frame ms (GPU ...)` line (`render_t7::timing`, timestamps inside encoders; on by default, every 5 s, `T7_TIMING_EVERY=<s>`).
- At load: `t7 fps: preset ... ; G-buffer N draws (M after culling this view, instances) in B bundles: pipeline and bind group switches, pipelines; forward draws, pipelines; every pipeline made at load`.
- Offline: `t7_host_check <zone>.ff <w> <h>` with `BO2MP_T7_PROFILE=1 T7_PROFILE_EVERY=0` (a line per frame), `T7_EYE="x y z pitch yaw"` for a fixed camera and `T7_FIXED_CLOCK_MS=<ms>` (one frame clock, so effects, wind and the AO and blur ring land the same in two runs and images can be compared pixel for pixel).

On the owner's PC (RX 9070 XT), from the game folder, any BO3 map, standing still for 15 s then turning around:

```powershell
$env:BO2MP_T7_PROFILE = "1"; .\CodLegacy.exe frontend t6mp
```

(or `BO2MP_T7_PROFILE=1` in the `.env` beside `CodLegacy.exe`), then send the log lines (in `codlegacy-artifacts\`) that start `t7 native: t7 fps`, `t7 native: t7 profile` and `t7 native: frame ms` (the GPU per pass, real timestamps on his card).

### Baseline and after (Nuketown, 1920x1080, ultra, two fixed cameras)

Measured in the cloud on lavapipe (Mesa's software Vulkan) on a 2-core box, `T7_STAND_IN=1 T7_IMAGE_MAX_DIM=256` (the map's package meshes and most pictures are not in the cloud, so stand-ins), cameras A `0 -600 60 5 90` and B `800 300 80 5 200`, frames 2 to 6. **The CPU numbers and the counts are real; the GPU milliseconds are a software rasteriser's** (seconds per frame) and only say which passes are heavy relative to each other. Run to run the CPU recording total moves by about 1 ms on this box (the phases much less).

| per frame | baseline A / B | after A / B |
|---|---|---|
| draws | 2319 / 2323 | 1377 / 1407 |
| pipeline switches | 1628 / 1632 | 638 / 659 |
| bind group switches | 5380 / 5392 | 2554 / 2644 |
| passes (render + compute) | 43 / 43 | 43 / 43 |
| bytes uploaded | 259 / 314 KB | 154 / 167 KB |
| effect sprites (draws) | 2581 (822) / 2581 (822) | 898 (285) / 1125 (304) |
| CPU forward record | 0.60 / 0.65 ms | 0.22 / 0.25 ms |
| CPU G-buffer constants | 0.054 / 0.139 ms | 0.048 / 0.044 ms |
| CPU effects sim + upload | 2.1 / 2.3 ms | 2.2 / 2.4 ms (unchanged: the simulation) |
| CPU recording, whole frame | 3.8 / 4.0 ms | 3.8 / 5.0 ms (noise; see above) |
| G-buffer after GPU culling | 398 of 991 draws, 1195 of 5480 instances / 435 of 991, 2881 of 5480 | same |

Images at ultra: identical pixel for pixel to the baseline at both cameras after every step, apart from two effect pixels that flip between two runs of the baseline itself.

What was done, biggest first (one commit each):

1. Forward draws (technique slot 6) culled to the view by the same spheres as the G-buffer's GPU culling (world surfaces by bounds, instanced model surfaces when no placement is in view; `T7_NO_FORWARD_CULL=1`), state the last draw set not set again (the exe's order kept), and a list with nothing drawn this frame records no pass, depth copy or scene resolve.
2. Effect sprites outside the view dropped after the simulation and the sprite lighting's pick (a sphere holding the quad at any rotation; `T7_FX_NO_VIEW_CULL=1`); the order of the rest is kept, so runs of one material merge into fewer draws.
3. No per-frame allocations in the code constants (one scratch), the sprite records (kept buffer) and env reads; the hand-off keeps its bind group and the scene depth view.

Already in place before (checked, not changed): the G-buffer is replayed from render bundles with GPU frustum culling into indirect draws (one instanced draw per model surface over its placements, instances culled one by one); bind groups are made per (material, group) at load and code constant buffers are written only when they change; every G-buffer, forward, effect, lighting and post pipeline is made at load (`warm`, `prewarm`, the load thread), so nothing is compiled when something first comes into view; the sun shadow cascades are filled from the map's baked shadow tree only when a cascade moves (no caster draws); the converted BO2 world is not built when the native renderer is on, and `render_gpu`'s `native_world()` keeps BO2's grade, bloom and world draws out.

The effects simulation (2 ms of the CPU frame here) runs on a worker thread a frame ahead (2026-11-03, `fx_draw::SimWorker`): each frame takes the sprites the worker made for it, at the time predicted from the last frame's step and from the last frame's eye, and asks for the next frame's; two sprite buffers go back and forth and the sort keeps its scratch (`fx::SortScratch`: the stable order by index, no allocation once grown). A result more than 50 ms off the frame's time or 256 units off its eye (a stall, a camera cut) is made again on the render thread; with `T7_FIXED_CLOCK_MS` and a still eye the sprites are exactly the inline ones. `BO2MP_T7_FX_INLINE=1`: on the render thread as before. Nuketown camera A, 640x360, frames 2 to 6: effects sim+upload 1.67-1.91 ms -> 0.14-0.27 ms, CPU recording 2.7-3.0 ms -> 1.1-1.6 ms, draws unchanged (1394), image identical (0 pixels). On the owner's PC the worker takes a core the game does not otherwise use for this.

Not done (the next steps, by cost): The forward lists' and effects' GPU cost is their visible pixels (the culling took off draws, not the pixels). Model LOD selection and the draw distance (the exe's small-on-screen cull) and the texture filtering are now driven by the settings ("Graphical gaps" below); spot shadows are (`spot_shadows`, `spot_shadow_res`).

### Graphics settings (`render_t7::quality`)

Read in order: `quality::set` (code), `BO2MP_T7_QUALITY=low|medium|high|ultra`, else the shared setting (`native_quality::current()`: Graphics > GRAPHICS QUALITY and its option rows, or `BO2MP_QUALITY`). Each switch's own variable still wins (`T7_SSAO_QUALITY`, `BO2MP_T7_AA`, `BO2MP_T7_MOTION_BLUR(_QUALITY)`, `T7_NO_SUN_SHADOW`, `T7_VOLUMETRIC_GPU`, `T7_NO_LIGHTS`, `T7_IMAGE_MAX_DIM`). A change of the shared setting logs `t7 quality: now ...`.

| shared value | BO3 renderer | LOW | MEDIUM | HIGH | ULTRA | applies |
|---|---|---|---|---|---|---|
| `ambient_occlusion` | GTAO (exe quality 3, temporal denoise) | off | off | on | on | next map |
| `motion_blur` | BO3 motion blur (Auto, medium) | off | off | on | on | amount next frame, passes next map |
| `sun_shadow` | sun shadow cascades from the baked tree (off: shadow mode 2) | off | on | on | on | next map |
| `local_lights` | the map's spot / omni lights in the lighting | off | on | on | on | next frame |
| `max_texture` | picture size cap, at most this renderer's 1024 (`T7_IMAGE_MAX_DIM=0` lifts it) | 512 | 1024 | 1024 | 1024 | next map |
| `render_scale` | the view's size (the game's upscaler, FSR 1 for native views; the view handed to this renderer already carries it, logged in `t7 fps:` as `view at N%`) | | | | | next frame |
| `spot_shadows` | local lights shadowed per view (`spot_shadow`; the exe's 4 at most) | 0 | 0 | 2 | 4 | next frame (the atlas is made at the map load when above 0) |
| `spot_shadow_res` | the atlas layer is 4 x this (512: the exe's 2048; spot tiles an eighth, omni a quarter) | | | 2048 | 2048 | next map |
| `sun_shadow_res` | not used: the cascades are 1024 texels as the exe's (ULTRA/HIGH's value); the exe's fill program (`depth_fill_from_sst`) writes 32 tiles of 32 texels from the baked tree, so a 512 map would need a fill the exe has none of (its own knob, the tree mip drop `r_lightingSunShadowSSTMipDrop`, is not traced); MEDIUM keeps 1024 (filled only when a cascade moves), LOW has no cascades | | | | | |
| `anisotropy` | the materials' texture-filter row (`sampler::material_quality`; 1 = the exe's x2..x16 by sampler level, 0 = x2 at most; `BO2MP_T7_TEX_FILTER=0\|1\|2`) | 0 | 1 | 1 | 1 | next map |
| `bloom` | the bloom's ten blur passes (off: skipped and the level `bloom_apply_lut` adds cleared to black, one pass; the downsamples that meter the exposure stay, so the exposure is the same; not an exe setting; `BO2MP_T7_BLOOM=0\|1`) | off | on | on | on | next frame |
| `depth_of_field` | not built | | | | | |
| `lod_distance`, `draw_distance` | static model LODs as the exe picks them and placements under a smallest radius on screen left out (`render_t7::lod`; LOD scale `1/lod_distance`, smallest radius `1/draw_distance` pixels; `BO2MP_T7_LOD=exe\|finest`) | exe (2, 2 px) | exe (1.33, 1.33 px) | finest | finest | next map |
| `vsync`, `max_fps` | the presentation's (not this renderer's) | | | | | |

AA stays SMAA 1x at every preset (no shared field). Never scaled: fog (the lit fog volumes too), colour grading, tone mapping, exposure. ULTRA is exactly what this renderer drew before the settings, apart from the spot shadows (ULTRA 4, HIGH 2; `BO2MP_T7_SPOT_SHADOWS=0`). Bloom off (LOW), Nuketown camera A, 640x360: passes 43 -> 34, draws 1394 -> 1384; the bloom-on image is unchanged.

## Graphical gaps (2026-11-03): what the exe's renderer has that `render_t7` lacks

Owner (BO3 session): "in the exe any graphical things we are missing, add
it into the game." Read off the exe's renderer dvar names (BO3 hashes its
dvars, but the `r_` names are kept in name tables), its draw code and the
earlier audits ([T7_EXE_PARITY_AUDIT.md](T7_EXE_PARITY_AUDIT.md),
[T7_VISUAL_PARITY.md](T7_VISUAL_PARITY.md)). Addresses:
`docs/exe_notes/T7_RENDER_GAPS.md`. Ranked by what the owner gets for the
work (seen on screen or frame time, every map), then by cost.

| # | Feature | What the exe does | What `render_t7` does | Status |
|---|---|---|---|---|
| 1 | Static model LODs and the small-on-screen cull (`r_lodScaleRigid`, `r_lodBiasRigid`, `r_modelLodLimit`, `r_autoLodCullRadius`) | Every frame, per placement: distance (scaled, biased) against the model's per-LOD distances times the view's pixel scale (height, field of view); a placement whose radius covers less than a pixel (or the model's own minimum) is not drawn | Was: the finest LOD of every placement, nothing left out for size | **landed** (`lod`, below); medium/low by default, `BO2MP_T7_LOD=exe` anywhere |
| 2 | Texture filtering (`r_texFilterQuality`) | Three anisotropy rows by setting | Was: row 1 always | **landed** (shared `anisotropy`; low takes row 0) |
| 3 | Spot / omni light shadows (`r_spotShadowRes`, `r_omniShadowRes`, `r_actorShadowCount`, `r_lightingShadowFiltering`) | Per view the four brightest shadowed lights get a tile of `gSpotShadowmapArray` and a shadow record (an omni light as a 90 degree spot looking at the view), 8-tap kernel; static cached slots for state-3 lights | Per-view shadows as the exe (`spot_shadow`, below); the static slots not drawn | **landed** (ultra 4, high 2, medium/low off; `BO2MP_T7_SPOT_SHADOWS=0`) |
| 4 | Lens flares (`klf`, `lensflare_query` / `lensflare_effect`) | Occlusion query disc per source into two UAVs, then each flare element's quads | Not drawn (effect type 12 silent) | missing, the chain now traced end to end (2026-11-03, below): the source list's fill and the 200-byte emitter records are found; building it needs the effect player to run lens-flare elements, the list fill, the emitter records, the query pass and the element draw (two translated techniques, one writing UAV counters): not started this pass |
| 5 | Screen-space reflections | `_ssr` lighting permutations when the setting is on | Built, opt-in `BO2MP_T7_SSR=1`, never seen lit on a GPU | partial: needs the owner's screenshot before it can be default |
| 6 | Depth of field (`r_dof_*`) | `dof/*` chain only for DoF modes 1/2 (cinematic views, a client state), never by aiming in mp | Not built | missing; low value in mp; 115 DoF programs need `sample_pos` in `dxbc_sm5` |
| 7 | Lit particles (`fx_compute_sprite_light`) | Smoke and sparks lit by the map's lights and probes on the GPU | Built, opt-in `BO2MP_T7_FX_LIGHT=1` | partial: kept opt-in (2026-11-03 check below: valid and cheap, but the light it gives a sprite is 2.5x the unlit one and nothing lit has been seen on a GPU) |
| 8 | Forward decals (`light_culling_decals`) | Decals culled into tiles, applied in the forward programs (water, glass, transparent surfaces) through `visibleDecals`, `forwardDecalConstants` and the decal material texture arrays (`materialTexBC3/BC7/Mask`) | Decal list bound empty | missing: the CPU packer of `forwardDecalConstants` and of the texture arrays is not found; the volume decals the G-buffer draws are done |
| 9 | Order-independent transparency (`r_OIT`, `oit_apply*`) | Per-pixel lists with texture atomics | Sorted forward only | skipped: no material of any staged mp map (Apartments, Metro, Nuk3town, Sector, Skyjacked, Western, mp_common) uses an OIT technique set, so the exe draws them sorted too; the 60 OIT programs use atomics on typed UAV textures, which WGSL lacks (it has atomics on storage buffers only: a translation would rewrite each texture into a buffer by pixel, in `dxbc_sm5`, not this renderer) |
| 10 | Wave water (`r_waveWater*`) | A simulated height grid around players | Static water programs | skipped: no staged mp map's water reads it (their `wc/lit_water_sim_flow*` and `water_shore_flow*` programs bind flow, foam, noise and colour maps only; no wave height texture) |
| 11 | Sun flare tint, atmospheric blur (`r_sunFlareTint`, `r_atmoBlur*`) | Vision words feeding a sun flare and an atmospheric blur | Read, unused | missing: the sun flare is a lens flare source (row 4) |
| 12 | Lens distortion (`bloom_apply_lut` variants) | Distortion variants of the apply pass | Plain variant | missing |
| 13 | Motion blur of moving models (stencil composite) | Moving models blurred with their own vectors | Static motion only | missing |
| 14 | AA: SMAA T2x / Filmic, FXAA (`r_aaTechnique`) | Temporal SMAA default | SMAA 1x (the exe's non-temporal fallback) | partial |
| 15 | Bilateral half-resolution lighting | Used when the scene is drawn below native size | Native-size lighting only | missing (only matters with the exe's upscaling) |
| 16 | Subsurface scattering blur (`r_sssblurEnable`) | Skin blur on characters | Characters are Black Ops II's | n/a here |
| 17 | Probe / sun-tree mip drop (`r_lightingReflectionProbeMipDrop`, `r_lightingSunShadowSSTMipDrop`), `r_textureMipLodBias` | Memory and speed knobs | Full size | not needed on the owner's card |
| — | Bloom on/off | No such switch in the exe: the bloom chain also measures the exposure | The shared `bloom` row skips the blurs and keeps the exposure's downsamples (LOW) | landed (`BO2MP_T7_BLOOM`) |
| — | Sun shadow resolution | The cascades are filled from the baked tree at 1024 | Same | n/a (`sun_shadow_res` not used; reason in the settings table) |
| — | Volumetric / lit fog, vegetation and cloth wind, siege animations, weather grime | | Done earlier (sections above) | done |
| — | Reverb | Audio, not graphics (rooms read, echo not decoded: audit section 4) | | not this renderer |

### Spot and omni shadows (`render_t7::spot_shadow`, 2026-11-03)

The exe's per-view local light shadows, translated (addresses in
`docs/exe_notes/T7_RENDER_GAPS.md`). The light packer sets aside every
listed spot or omni light whose shadow state is not 1, ranks them by the
light each throws into a box of the view (the highest of its brightness at
the box's 8 corners: colour, inverse-square falloff, the near and far
smoothsteps; ties by distance) and gives the first four (the shared
`spot_shadows` row: 4 ultra, 2 high) a tile. A spot keeps its own
projection; an omni light becomes a 90 degree spot at its origin looking
at the box's centre (the exe's conversion). The projection is fitted to the
box (uv scale and offset, z/w range), the tile allocated in an 8 x 8 grid
of cells per 2048-texel layer (spot tiles 256 px, omni 512 px; Morton
order; the least recently used tile not used this frame evicted) and the
casters drawn depth only with the fitted matrix, the exe's shadow depth
bias (-4, slope -4) into a 16-bit layer. A 15-vec4 shadow record per tile
goes after the light records and the light's flags word names it (bits
20..23); the lighting programs do the rest (8-tap kernel, `LightingGlobals`
58.y set as the exe's default, cubed).

Not the exe's: the exe's box is the bounds of the view's visible cells; the
ranking here takes the light's box clipped to the view frustum's, and the
fit the light's own box clipped to the light's square, so a tile is drawn
once while its light keeps it (the static world drawn here does not move;
the exe draws every tile every frame). Casters are the opaque G-buffer
draws (world surfaces by bounds, static model placements by sphere, finest
LOD) culled to each light's frustum; cut-out and blended materials do not
cast. All tiles of a frame are one render pass. Not drawn: the static
cached slots of state-3 lights (cube omni array) and the view model's set.

Measured (lavapipe, Nuk3town, `T7_STAND_IN=1`, 480x270 and 640x360): 32 of
the map's 88 lights can cast; the first view draws 4 tiles of 28 to 81
draws each, once; after that draws, pipeline and bind group switches per
frame are the same as without (1109 / 384 / 1750 at the test camera); the
lighting update is ~0.15 ms more (the ranking). Without the bias the lit
receivers shadowed themselves (264 of 129600 pixels darker by more than 8
at `-1442 1000 120 20 80`: the lookups land on the casters' own depth, so
the mapping is consistent); with the exe's bias 0 pixels move by more than
8 there. With `BO2MP_T7_SPOT_SHADOWS=0` the frame is identical to before.
The cloud has no packages (stand-in pictures, black probes), so what the
shadows look like needs the owner's screenshots: indoor lamps on Nuk3town
(the houses) and Sector, ultra, with and without `BO2MP_T7_SPOT_SHADOWS=0`;
`BO2MP_T7_SHADOW_FILTER=0` turns the 8-tap kernel off (sun cascades too).

### Static model LODs (`render_t7::lod`, 2026-11-03)

The exe's pick, translated: per view a LOD context (pixel scale
`px = sqrt(floor(h·8/9)·(h/2) / (tan_x·tan_y))`, LOD scale
`sqrt(px²/4 + 1/π)`, distance scale and bias, LOD limit, smallest radius
on screen); per placement the eye's distance to its point (record +0x34)
scaled and biased, less its radius (+0x84): inside the sphere the finest
LOD; a radius under `max(smallest radius, model's own) · d / px` not
drawn; else the highest LOD index (the finest) whose distance
`size · scale · dist[i] · lod²` still exceeds the squared distance, capped
at the model's finest for the main view (+0x85) and by the LOD limit. A LOD
whose mesh is not here falls back to the nearest finer, then coarser (the
exe's order while one streams in).

How it is drawn: with the setting on, every LOD of a model with more than
one is built at load; a model surface gets one indirect draw per LOD; the
GPU culling (`cull`) runs the pick per placement in the frustum and writes
each into its LOD's region of the instance ids (a range now has a region
per LOD and one for every placement in view). No CPU work per frame, no
extra bundle recording: the extra draws are in the bundles from load.
Forward-only surfaces (glass, emissive without a G-buffer technique) keep
the finest LOD over every placement, as the forward lists draw their own
instances; without the GPU culling (`T7_NO_GPU_CULL=1`, or no
`INDIRECT_FIRST_INSTANCE`) the finest LOD is drawn as before.

Checked offline (lavapipe, the staged zones with the core zones, 1920x1080
views at the exe's defaults, `BO2MP_T7_LOD=exe` against `finest`):

| | Nuk3town finest | Nuk3town exe | Sector finest | Sector exe |
|---|---|---|---|---|
| triangles per spawn view (mean of 9, `t7_cull_check`) | 0.23 M | 0.08 M | 2.43 M | 0.55 M |
| worst spawn view | 0.49 M | 0.11 M | 3.38 M | 0.63 M |
| placements drawn per view | 2417 | 1445 | 9224 | 7169 |
| GPU indirect arguments vs the CPU pick | 0 mismatches | 0 mismatches | 0 | 0 |

`t7_host_check` (Nuk3town, camera `800 300 80 5 200`, 640x360,
`BO2MP_T7_PROFILE=1`): draws 1401 -> 2081 (each LOD is its own indirect
draw in the bundles; those with no placement this frame draw nothing),
pipeline switches 650 -> 646, bind group switches 2629 -> 2608 (a model's
draws of one material are kept side by side across its LODs), CPU recording
unchanged, G-buffer pass on the software GPU 823 -> 619 ms (proportion
only). Image against the finest LOD: 5.9 % of pixels differ, 70 of 230400
by more than 8 levels (the far models). With `T7_NO_GPU_CULL=1` the exe
setting draws exactly the finest image, and the culled finest image equals
the unculled one (0 pixels), so ultra is unchanged. Not done: the exe's
scene factors (+0x1a6ad4, +0x1a6bd4, the kind byte for special views) are
taken as 1, 1, 0; scene models (effects' models, script entities) still
draw their finest LOD; the shadow pass has no casters to pick for; the
empty LOD draws could be dropped with a multi-draw count once wgpu has it
in bundles.

Needs the owner's PC: at medium (or `BO2MP_T7_LOD=exe` at ultra), walk
Nuk3town and Sector and watch for LOD pops at the distances the game has
them, and send the `t7 fps` / `t7 profile` lines (draws and instances
after culling).

## Done vs not yet

Done:
* reference exe analysed; code technique table, engine constant table,
  struct size table extracted;
* every code and material shader located in the zones; all constant
  buffer layouts read from the shaders (`t7_rdef`);
* LightingGlobals filler and the sun constants builder decompiled
  (formulas above); walker record sizes verified against the exe.
* map logic: the exe's map-load spawn rules (`fastfile_t7::spawn`; the sim
  gets only what the game spawns), map-placed `trigger_box` sizes and
  `trigger_disk`;
* collision: the world is the BSP's reachable leaves, their brushes and
  their AABB tree roots only (sub-model meshes no longer world collision).

* `dxbc_sm5` translates compute shaders (thread groups and ids,
  structured/raw SRVs and UAVs, typed UAV textures, group-shared memory,
  atomics incl. signed max/min by compare-exchange, barriers, `switch`,
  first-bit ops, `resinfo` on UAVs): every program dumped for the lighting
  and post passes (135: deferred lighting permutations, classify, uber
  resolve, exposure, LUT, bloom, resample) translates and passes naga
  validation; of 2688 `core_patch` programs 2513 do. Left: `sample_pos`
  (115 DOF programs) and atomics on typed UAV textures (60 objective/OIT
  programs; WGSL has no texture atomics), `imm_atomic_alloc/consume`.

Not yet (nothing below is drawn or substituted):
* **the frame on screen.** Status 2026-10-22 night: the lighting stage is
  built (`render_t7::lighting`, above; its gaps listed there); not built:
  light culling with the light and probe packers, sun shadow maps (since
  built, unverified), then
  exposure, LUT, bloom and resample_final. Until the
  chain reaches the screen the native path stays opt-in
  (`BO2MP_T7_NATIVE=1`) and the converted world is drawn.
* `crates/render_t7` past the G-buffer: tile classification + permuted
  lighting, compose, exposure, bloom, LUT and `resample_final` (the G-buffer
  pass is stage 1, above).
* the client side that fills the post-FX settings `S` and the fog state
  (vision/LUT/bloom boxes, exposure volumes, lit-fog volumes), the rest of
  the LUT constant maths, bloom constants, and the sun shadow setup (how the
  lit-fog volumes and vision/"box" states — `GfxBoxVBLOOM/VCOLOR/VLUT/VUKKO`,
  `GfxModV*` — become them; done since: colour vision, LUT, exposure,
  fog banks, `hdrControl0/1`, `relHDRExposure`, `skyMx`/`sunMx` from the
  vukko blend; `colorMatrix` is read by no shipped program, so not needed),
  script vision overrides;
* reflection probe packing (RefProbePack/Blend), light packing details;
* shadows (spot/omni, shadow trees), the volumetric pass on the GPU (lit
  fog volumes are packed, `render_t7::volumetric`), SSR, bloom/lens flares, particles and sprite lighting, post FX,
  DOF, motion blur, AA;
* collision still not exact: the static model trace function itself is
  not decompiled (the data and its convention are), sub-model meshes (a sub-model's own
  AABB trees: our sub-models carry brushes only), patches/terrain beyond
  the mesh, and the exe's own trace code (ours is Black Ops II's lineage on
  the same data);
* map logic still not done: destructible definitions as the exe reads
  them, nav mesh / path data beyond nodes, exposure/vision volumes,
  sound/reverb areas, `heli_height_lock` (the helicopter ceiling), the
  vehicle paths, weapon pickups' validity check, and the gametype-object
  filter (`script_gameobjectname`, done by Black Ops III's gametype scripts;
  here every game-mode `script_brushmodel` is left out).

**Water seen from below (2026-10-27, unverified on GPU).** The BO3 tour showed
mp_havoc's sky covered by a mirrored rock slab: the vista water sheet
(`t7_water_mp_havoc_flow_vista`, ~1e10 sq units) drawn with the eye under its
top, where refraction and reflection of the lit scene paint upside-down scene
over the sky. A flat water sheet (or any vista one) whose top is above the eye
is now left out of the forward lists (`Forward::water_hidden`).
`CODLEGACY_T7WATER=all` draws every water draw, `off` none (also the way to tell
whether mp_metro's flat plaza sheet is water: with `off` the floor either
shows or does not). `CODLEGACY_T7WATERLOG=1` logs each water draw's bounds, the eye
and the verdict when it changes (`t7 waterlog:` on stderr).

## Misc. screenshot gaps (heyday truck, cryogen, arena, ruins; 2026-10-27, from the PC shots and logs, no GPU)

* **Orphaned forward-depth prepass (fixed, `GBuffer::drop_orphan_forward_depth`).**
  `Material::forward_depth` lays the depth of an opaque forward-only
  material in the G-buffer pass; if its forward draw is then not ready
  (`forward: skipped` binding errors, a view list drawn before the lighting,
  a failed pipeline) the depth stays and the pixel is lit as black with
  nothing behind it drawn. That is the blocky black gaps (ruins' arches,
  cryogen under the walkway) if those surfaces are forward-only. Now the
  prepass draw is dropped when its forward draw is not ready; the forward
  log line `N depth prepass draws of opaque forward materials left out` names
  the materials (`T7_FORWARD_DEPTH_ORPHANS=keep`: the old way). Confidence
  medium: the shape matches, the logs of those two maps are not in hand.
  If the black remains, run with `BO2MP_T7_NATIVE_DEBUG=ambient` / `sunonly`
  and compare: black in both = no G-buffer data (this); black only without
  the sun = the probe volume (next item).
* **Probe volume cells with no data.** The logs of every PC map print
  `probe units/tint (sun[8..12]) 0.000 ...` in shadow mode 3, i.e. the PC
  build had no `keep_probe_words` (commit 7385bf78) or the probe's words are
  zero there. After a rebuild the line must show non-zero (offline Aquarium:
  531.178 0.876 1.021 1.161). Black regions with stair-stepped (grid cell)
  edges out of the sun are the signature of indirect light read as zero.
* **Heyday pickup truck.** Not found by reading. Its techsets are
  `mc/lit_vehicle*` and `mc/lit_micro_tile_mix_vehicle_plus` (41 draws), which
  the log shows with G-buffer techniques; there is no stand-in or skip line
  for them. See-through plus white means an alpha or tint constant read as
  zero/one: suspect `fill_missing_cbuffers` (variables the material names
  only partly stay zero, or a sibling's buffer is copied) or a vehicle paint
  mix map. Run with `CODLEGACY_T7_LOG_MODEL=truck,pickup,veh` (new): one
  `t7 model:` line per material (techset, passes, ready or not, pictures
  and whether they loaded, forward blend word, cbuffer count), then
  `CODLEGACY_T7_DEBUG_SURF=only=pickup` / `hide=glass` to isolate the layer.
* **Arena shot 3 (flat brown).** The orange stripes are textured, the large
  panels are not: a colour map stood in as mid grey
  (`surf_debug::stand_in`, "mid grey colour (untextured)") under orange light,
  or a decode failure like `i_generic_lookup: part 0 decodes to 0x8000 bytes,
  expected 0xaab8` in heyday's log. Grep the arena log for
  `t7 surfaces: stand-ins by kind` and `gbuffer: image missing`. The hatched
  ladder pattern on the floor under the glass is a reflection/shadow
  aliasing, separate.

* **Sky dome with mixed vertex distances (mp_city, 2026-10-27).** `skybox_dlc4_mp_city_override` is a 316-vertex ellipsoid (266..786 from its origin), not the generic unit sphere. The sky VS gives each vertex a direction; interpolating vectors of different lengths across a triangle points elsewhere than the pixel's ray, so big triangles looked up the lat-long in wrong directions (straight-edged black/stretched polygons). `Scene::build_model` (sky only) now puts every sky vertex on one sphere (the farthest vertex's distance), same directions. Not confirmed on PC (no BO3 files offline). `T7_SKY_RAW_POSITIONS=1`: as authored; the log line `sky mesh ...: N of M vertices moved onto one sphere` says how many moved. If the black polygons remain, the next suspect is the sky picture data itself (8192x4096 BC6H single level) or backface culling of a mixed-winding override dome (`T7_SURF_DEBUG` cull_none); `BO2MP_T7_NATIVE_DEBUG=sky` shows only the sky.

* **mp_city sky, black wedge + garbled fragments after the sphere fix (2026-10-27, second pass).** The sky VS normalises `pos - eyeOffset` itself, so the previous "one sphere" change did not alter what is drawn (directions only); the real suspects are the authored dome's triangles and the picture. Three changes, each with a switch: (1) sky techniques draw with no face culling (`T7_SKY_CULL=1`: the technique's own state) - a mixed-winding override dome culled to black holes; (2) a one-surface sky mesh is replaced by a generated 96x48 lat-long dome (poles on +z, attributes copied from the nearest authored vertex, winding like the authored majority) - `T7_SKY_AUTHORED_MESH=1` keeps the authored triangles; (3) new log lines: `sky mesh ...: N triangles: a wind one way, b the other, ... solid angle X of 4 pi (up/horizon/down), largest triangle, coverage by 30 deg azimuth sector` (a sector far below 1.00 = hole in the authored dome; both windings present = culling hole) and, on the `sky <image>:` line, `BC6H blocks with a reserved mode N of M ... by 16 columns` (reserved-mode blocks decode black; a cluster of them in some columns = the wedge is the picture). If the wedge persists with a generated dome and no cull, it is the picture/pixel program: read the BC6H line, then try `T7_SKY_RAW_POSITIONS`-independent checks (`BO2MP_T7_NATIVE_DEBUG=sky`, `CODLEGACY_T7_LOG_MODEL=skybox`).
