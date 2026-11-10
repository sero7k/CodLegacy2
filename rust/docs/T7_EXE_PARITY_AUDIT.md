# Black Ops III exe vs `render_t7`: map-system parity audit

Owner's directive (2026-10-27): *anything related to maps within the BO3
exe — lighting, anything at all relevant to us — put it into our game.*
This is the systematic list: every map-relevant system the exe has, what
`render_t7` / the T7 map lane does about it, and the evidence. It is the
superset of [T7_VISUAL_PARITY.md](T7_VISUAL_PARITY.md) (renderer passes,
kept up to date there) and the "Done vs not yet" list in
[T7_NATIVE.md](T7_NATIVE.md); this file adds the world data, the dynamic
map content, the map-side sound and the per-map settings, and orders the
work.

Sources: the owner's `BlackOps3.exe` (static analysis, Ghidra headless;
addresses and decompiles stay in the private notes, not here), his zones
(`core_patch`, `core_common`, `core_gfx`, `mp_common`, `mp_sector`,
`mp_apartments`, `mp_nuketown_x`, `mp_skyjacked`), the shaders' own RDEF
(`t7_rdef`), the walker (`fastfile_t7::walk`).

Status: **done** (the exe's way, from its data/programs), **partial**
(named gaps), **missing**, **n/a** (no visual or map effect for us),
**other agent** (being built in parallel now: water, fog/volumetrics,
collision, map completeness, Outlaw white-out, Aquarium water/palms; not
touched by this audit's work).

Updated 2026-10-27.

## 1. `GfxWorld` lumps (what the exe streams for a map, `walk/gfxworld.rs`)

| Lump / section | Exe use | Status | Evidence / gap |
|---|---|---|---|
| Surfaces, vertices (pos 12 + attr 20), indices, sorted surface lists | world draw | done | `scene`, G-buffer + forward lists |
| Static model draw instances (152) | model draw, lighting per instance | done | `gbuffer` model instances |
| Brush models (64) | sub-model draw (doors, scripted brushes) | partial | drawn static; script movement not run (see 4) |
| Sun volumes (4536): sun state, `GfxGlobalLightSettings` | sun constants, shadow mode | done | `lighting_globals`, `native::view_sun` |
| Sun shadow trees (4 x `GfxStreamedShadowTreeBuffer`), min/max trees | cascades from the static tree, `gSunShadowMax` | partial | `sun_shadow`: static casters only |
| Reflection probe arrays, configs (328), blends (604) | `RefProbePack/BlendPack`, cube array | done | `probes` |
| Probe volumes (3 images per volume) | diffuse SH grid `gProbeX/Y/ZArray` | done | `lighting::probe_volume` |
| Sky boxes (`GfxSkyBoxImage`, rotation, model) | lat-long sky, `skyRotationTransition` | done | forward sky; model from base package partial |
| Exposure grids (probes 80, voxels, blends, faces) | metering | done | `post_exposure` |
| Exposure volumes (108) + planes | exposure per volume | done | `post_run` |
| Outdoor volumes (424) + 4 shadow trees | outdoor mask / SST | partial | read; `outdoorSSTConstants` left zero (no shipped mp program reads them on the staged maps) |
| Fog volumes (624), fog modifiers, planes | fog banks | other agent | `fastfile_t7::fog` |
| Lit fog volumes (360) + planes | volumetric light | other agent | `volumetric*` |
| LUT volumes (60) + planes, LUT material | colour grade | done | `post_lut` |
| Weather grime volumes (120, six images) + planes | `LightingGlobals.weather` (wetness, tint, **wind** speed/direction), weather code textures | partial → see 6 | `weather`; the G-buffer bound the material's own constants as `LightingGlobals` (fixed below) |
| Post-FX lists: vukko, bloom, LUT, colour vision, underwater (mods + boxes) | per-volume vision | done (underwater: missing) | `native::vision_slots`, `post_bloom::view_settings`, `sky_matrices`; underwater boxes (164) read, not applied (no staged mp map has them) |
| DPVS planes/nodes, cells (56) + AABB trees | visibility | n/a | everything drawn; frustum/umbra culling is a frame-time matter, not a look |
| Umbra tomes | occlusion culling of models, lights, probes | n/a (perf) | lights/probes culled by frustum only, which keeps every one the exe keeps |
| Cookie image array (+0x260) | `gCookieArray`: cloud layer, light cookies | partial | opt-in `T7_CLOUD_SHADOWS=1`; no staged map's sun has a cloud layer |
| Lens pack | sun/light lens flares | missing | `lensflare_query`, `techsetdef_lensflare*`; element writer read, source list not |
| Volume decals (184) + decal reveal image | projected world decals (`voldecalRevealTextureInfo`, `light_culling_decals`, `forwardDecalConstants`) | done (`BO2MP_T7_VOLUME_DECALS=0` off) | the exe's instanced path (`141de6db0`): each visible record packed into a 200-byte `volDecalInstanceBuffer` element, one box draw per material; seen drawn offline (section 6) |
| Material memory (16) | per-surface material constants | done | scene materials |
| Lightmap array (`GfxLightmapArray`) | baked indirect on world | done by the programs | the G-buffer programs bind what they read; no BO3 mp world program in the staged zones fails on a lightmap |

## 2. Render phases (code techniques in `core_patch`, the exe's order)

| Phase | Status | Gap |
|---|---|---|
| Depth prepass (slot 2) | done | |
| G-buffer (slot 3: lit, plus, detail, micro-tile blend, vegetation, cloth, skin, fxanim) | done | vegetation wind now from the frame's `LightingGlobals` (6) |
| Motion vectors (slot 4 / `motion_vector_static`) | done (static) | `_with_mb` while the view blurs |
| Deferred decals (`gbuffer_decal*`, `resolvedNormal`) | done | |
| Volume decals (G-buffer decal list) | done | section 6 |
| Forward decals (`light_culling_decals`) | missing | decal list bound empty |
| Light culling (lights, probes) | done | |
| Tile classify + 64 permutations | done | |
| SSR permutation set (`deferred_lighting_permuted_ssr(N)`) | partial (opt-in `BO2MP_T7_SSR=1`) | built from the exe's selection and inputs (6); not seen in a lit frame |
| Bilateral permutation set | missing | upscaled half-res lighting only; not used at native resolution |
| Spot / omni shadow maps | missing (findings, section 6) | bound empty (`gSpotShadowmapArray`, `gOmniShadowmapArray`) |
| GTAO + denoiser | done (other agent built) | |
| Forward lists (emissive, transparent, glass, water, sky) | done | water: other agent |
| Effects (sprites, tails, runners, lights) | partial | opt-in natively; trails, emitted/impact effects missing |
| OIT (`oit_apply*`) | missing | texture atomics have no WGSL form |
| Distortion | partial | |
| Volumetrics | other agent | |
| Exposure, LUT (vision blend), bloom, final resample | done | |
| Lens distortion (`bloom_apply_lut` variants) | missing | |
| Lens flares | missing | |
| ADS blur (`bloom_downsample_l1_adsblur`, `adsblur_*`) | done (`BO2MP_T7_ADS_BLUR=0` off) | section 6; the game's `fWeaponPosFrac` drives it; scope variant left out (Black Ops II's overlay) |
| Depth of field (`dof/*`) | missing (findings, section 6) | the exe runs the chain only for DoF modes 1/2 (refdef word `+0x38` bits 16..17), set for view type 0xb or a client state, never by aiming |
| Motion blur (`mblur_*`) | done (`BO2MP_T7_MOTION_BLUR`, the exe's Auto) | section 6; Auto blurs only while a blur amount is set (`BO2MP_T7_MOTION_BLUR_AMOUNT`; the exe's comes from a script command); the moving models' composite (stencil) not drawn |
| AA (Filmic SMAA T2x default; FXAA) | partial: SMAA 1x (`BO2MP_T7_AA`, on) | the exe's compute SMAA (section 6); T2x / Filmic need the projection jitter and history: run as the exe's non-temporal fallback (2); FXAA not built |
| Film grain / chromatic aberration (`resample_final_bm*`) | done | `resample_final_bm_null` (the mp default) |
| Screen overlays (damage, flash, shellshock, ADS blur) | missing natively | the BO2 HUD draws its own |

## 3. Material technique slots

Twelve slots per technique set (T7_NATIVE "Which technique draws what"):
2 prepass, 3 G-buffer, 4 G-buffer + motion, 5 unlit, 6 distortion /
effect, 7/8 emissive, 9/10 transparent, 8–11 debug. Done: 2, 3, 6 (effects),
7–10, sky. Not run: 4 (we write motion from `motion_vector_static`), 5,
debug slots (by design).

## 4. Dynamic map content

| System | Status | Gap |
|---|---|---|
| fxanim (GPU-skinned vines, flags, banners, cloth) | partial | rest pose; xanim playback not ported natively (the converted runtime animates flags/foliage) |
| Vegetation sway (`gbuffer_vegetation` wind) | done (2026-10-27) | the frame's `LightingGlobals.weather`, see 6 |
| Water ripples / shore flow | other agent | |
| Scripted brush models / movers | partial | drawn at rest; map scripts not run, game-mode brushes left out (spawn rules) |
| Destructibles (`destructibledef`) | missing | walked as an asset type, not used |
| Ambient effects (createfx / map-placed `fx_*`) | partial | native opt-in; converted runtime plays them |
| Exploders | partial | opt-in, fired at start |
| Ambient sound entities, reverb zones | partial (`BO2MP_T7_AMBIENCE=0` off) | rooms, room tones, emitters from the map's own bank and structs (section 6); the rooms' reverb (`SndRadverb`, 23 floats) not decoded, no echo |
| Killcam | n/a | Black Ops II's killcam (game side) |

## 5. Per-map settings (dvars, visions)

| System | Status | Gap |
|---|---|---|
| Vision files (`vision/*.vision`, 36-field table) | done | `native::VISION_FIELDS`; no mp map zone carries its own |
| Per-volume vision (vukko, bloom, LUT, colour boxes) | done | |
| Script vision overrides (killstreak/ability visions in `mp_common`) | missing | game-side trigger |
| `r_sunFlareTint`, `r_atmoBlur*` (vision words) | read, unused | no sun flare / atmospheric blur pass |
| Sun shadow / volumetric / exposure dvars | done | exe defaults translated (T7_NATIVE) |
| ADS blur / DoF dvars | ADS blur: folded path bool `0x6511eacd` (1), focus `0x6c655324` (500), f-stop `0x7b3e01dc` (108, 0.95..120), scale `0x53b683d5` (1), focus share `0x79589016` (0.5) | translated (section 6) |
| SSR / SMAA / DoF dvars | SMAA: AA mode (hash `0x99442788`, enum off / FXAA / SMAA 1x / Filmic SMAA 1x / SMAA T2x / Filmic SMAA T2x, default 5) | BO3 dvars are hashed (no names in the exe); names from the enum strings |

## 6. Work done from this audit (2026-10-27)

* **Vegetation wind** (`t7 wind:` line). The G-buffer bound any constant
  buffer without code-constant arguments from the material's own buffers,
  falling back to whichever material buffer sat in the slot: a vegetation
  vertex program's `LightingGlobals` (it reads `weather` at 0x260: the
  grime volume's wind speed and the sin/cos of its direction, times
  `gameTime`) got the material's `$Globals` bytes, i.e. zero wind. Sector's
  grime volume has `windSpeed 10`, `windDirection 90`. The G-buffer now
  binds its own `LightingGlobals` copy, refreshed every frame from the
  lighting stage's (the forward lists already took the lighting stage's).
* **Screen-space reflections** (`t7 ssr:` lines; opt-in
  `BO2MP_T7_SSR=1`). From the exe's lighting recorder: the permuted set is
  chosen per view as plain / `_bilateral` (+0x200 in the slot table) /
  `_ssr` (+0x400); `_ssr` when the SSR dvar (hashed, registered default 0,
  flags 0x10: a saved setting) is on and the view's motion-vector flag
  (S+0x1d30) is set. The SSR programs read, beyond the plain set:
  `viewProjectionMatrix`, `gDownsampledScene` (code texture 0x63: render
  target role view+0xb0e2e), `gMotionVectors` (0x64: role +0xb0e58 by
  frame parity), `gSSAODepthTexture` (0x56: with GTAO the depth pyramid
  ring image of the frame, `(frame + 2) % 3`) and sampler state 0x2a9 at
  s1 (`samplerPointClamp`, marching the pyramid by mip). With SSR on, the
  post chain's bloom downsample draws `bloom_downsample_l1_scene` in
  `_l1`'s place: the same bloom output plus `SV_Target2`, the quarter-res
  scene, which the next frame's permutations fetch through the motion
  vectors. Built: the SSR set, the three inputs and the sampler (the
  lighting stage's bind groups are now six per program: ring slot x
  motion parity), `gDownsampledScene` owned by the lighting stage and
  written by the post chain, code passes leaving out targets their pixel
  program does not write. Offline (`t7_ssr_check`, no map): permutations
  0/21/42/63 translate and make pipelines, the only new resources are the
  four above, and `bloom_downsample_l1_scene` draws with no validation
  error, its target 2 reading back the input colour (0.5, 0.25, 0.125).
  Not verified: a lit frame with SSR (the full-map host check is over this
  box's memory), so it stays opt-in.
* **Volume decals** (`t7 voldecals:` lines; on,
  `BO2MP_T7_VOLUME_DECALS=0` off). The exe's instanced path (`141de6db0`, when
  its dvar is on; the shipped `vd/*` techniques read the buffer it fills)
  writes per visible record (byte 0xb0 clear, cell visible) one element of
  50 floats: the local-to-world 4x4 with the axes scaled by the half
  extents (record words 0..12 and 24..27; the box is the cube -1..1), its
  inverse, the fade start (27..30), 0, the tint (1,1,1,1), the two UV
  rectangles (30..38), 1, 0. Decals are sorted by material, at most 0x800,
  one instanced box draw each. The pixel program rebuilds the position
  from `floatZ`, keeps |local| <= 1 and surfaces facing the local x axis
  (within 60 degrees), and samples the UV rectangle. Built: the walker
  keeps the records and their materials; `crate::volume_decals` packs
  them (unit-tested on Nuketown's first decal: centre, corner at the
  bounding radius, inverse, the record's own inverse); the scene adds a box
  (wound as the models) and one group-1 draw per material; the G-buffer
  binds code buffer 10, makes `floatZ` from the depth between the opaque
  and decal lists, and now takes the view's whole code-constant set.
  Offline (`t7_voldecal_check`, G-buffer only, Nuketown): 12 of 12 decals
  in 3 draws ready, no validation error; per decal a view back along its
  projection axis drawn with and without the decals: decals 3 and 11
  change 1156 and 639 of 16384 pixels, a square exactly where the box
  meets the wall (the logo pictures are not staged: the stand-in colour);
  the other ten sit on props whose models are not staged here, so nothing
  is under them.
* **Anti-aliasing** (`t7 aa:` line; `BO2MP_T7_AA=<0..5>` by the exe's
  mode numbers, default 5; `0` off). The AA dvar (hash `0x99442788`) is an
  enum, registered default 5: off, FXAA, SMAA 1x, Filmic SMAA 1x, SMAA T2x,
  Filmic SMAA T2x. `0x141d634d0` reads it and returns 2 for 3..5 when the
  frame cannot be temporal (a second view / split screen). The post frame
  (`0x141d66110`) runs it after the orchestrator (so on the graded image
  0x48 and its luma 0x4a) and before `resample_final`: mode 1
  `0x141d64900` (`fxaa_hemi_aa_*_compute`), modes 2..5 `0x141d636c0`, then
  `0x141d63d80` for 3..5. `0x141d636c0` is all compute (code shader table
  at `0x142ece4d0`, 0x18-byte `{name, slot}` entries):
  `smaa_pass1_compute_hq` over the frame (SRV 1 luma, SRV 2 colour, UAV 1
  the edges, UAVs 4/5 two append queues with initial count 0, a 12-byte
  element per edge pixel: the pixel, its neighbour's colour packed);
  `smaa_resolve_morphological_work_queue_compute` (1 group: the counts,
  through an `imm_atomic_alloc`, into six indirect words, the queues padded
  with zero elements to 64); `smaa_pass2v_compute` and
  `smaa_pass2h_compute`, `DispatchIndirect` at 0 and 12 (SRV 0
  `$smaa_tex`, the `$smaa_pass2` material's area/search picture, SRV 4 the
  edges, UAV 2 the colour, written in place at the queued pixels).
  `0x141d63d80`: mode 3 draws `$smaa_pass4` into the filmic history by
  frame parity (`postFxControl1 = (exp(-dt_ms/125), exp(-dt_ms/600), 0,
  0)`), modes 4/5 the temporal `smaa_pass3_work_queue(_hq)` over a ring of
  four frames plus `smaa_resolve_filmic_work_queue_compute` /
  `smaa_pass4_compute` (5), with `postFxControl0` the jitter (when its own
  dvar is on). Built: SMAA 1x exactly as above; `dxbc_sm5` now translates
  `imm_atomic_alloc` / `_consume` (D3D's hidden counter as the buffer's
  last word, reset by the host as the initial count). With AA on, 0x48 is
  RGBA16F (R11G11B10 is no storage format). Unit test (`post_aa`, a 64x64
  slanted edge): 148 + 22 queued edge pixels, the indirect words match,
  105 pixels blended, none more than 3 pixels from the edge. Not built: the
  temporal modes (this renderer draws no sub-pixel jitter, so 3..5 take
  the exe's own fallback, 2) and FXAA (1 runs as off). Not seen in a lit
  map frame (memory).
* **Camera motion blur** (`t7 motion blur:` line; `BO2MP_T7_MOTION_BLUR`
  = `off` / `auto` (default) / `on`, `..._QUALITY` 0..2 (default 1),
  `..._AMOUNT` Auto's amount). The view setup (`0x141ddc6c0`) sets the
  view's blur flag and scale (view state `S+0x1d30`: +1 the flag, +4 the
  scale, also `MotionVectorParams` 12.w). The blur dvar (hash `0x48c1287c`)
  is Off / Auto / On, default Auto; the scale dvar (`0x75eb6bca`) 1. On:
  blur at the scale. Auto: scale x the refdef's blur amount (refdef +0x74,
  cg `+0x131d64`: zeroed at map start, written only by a server script
  command handled in `140304960`, `atof` clamped 0..1), on above 1e-6; a
  client flag (bit 30 of `+0xd5f48`) forces amounts 1 / 0.125 / 0.625 /
  0.25 / 0.75 / 0.375 / 0.5 (bits 24..26), bit 31 zero. So the default
  blurs nothing in ordinary play. With the flag, `0x141d5c6b0` draws
  `motion_vector_static_with_mb` (target 1 the blur velocity, role
  `0xb0e5c`, R8G8B8A8: pixel velocity clamped to 20, `v/40 + 128/255`,
  depth in z) and `mblur_mv_depth_composite_dynamic` (moving models,
  stencil), and the post frame (`0x141d66110`, before the bloom) runs
  `0x141d64f30`: `mblur_tile_downsample_0` (20 taps across into `0xb0e5e`,
  a twentieth of the width) and `_1` (20 down into `0xb0e60`), each with
  `postFxControl0` = the shift from the tile's centre to its first pixel
  (`0.5/W - 0.5/tiles`) and the one-pixel step; `mblur_tile_neighbor` into
  `0xb0e62`; `mblur_low/medium/high` by the quality enum (`0x2853ac00`,
  default Medium) with code textures 7 the scene, 0 the velocity, 1 the
  neighbour tiles, into a new scene target the chain then reads. Built:
  all of it except the dynamic composite (this renderer's motion vectors
  are the camera's only). Unit tests (`post_mblur`): the exe's scale rule;
  a 160x80 frame with a 4-pixel bar: still, unchanged; moving 20 pixels
  across, the bar's light spread to 8 pixels either side in the 6-tap
  pattern of `mblur_medium`. Not seen in a map frame.
* **Spot / omni shadows (findings, not built).** The light packer
  (`141db6e10` -> `141db5a70` -> `141d40a40` -> `141d3fae0`) takes, for a
  spot or omni light (type 2 or 4) not flagged unshadowed, a per-light
  shadow state of 0x90 bytes from a per-client array of 3000
  (`0x14a33ba20 + client * 0x69780 + index * 0x90`; a dynamic light's from
  `141db4de0`): a dynamic slot at +0x70 (u16, 0xffff none) with its four
  params at +0x80..+0x8c, else a static slot at +0x50 with params at
  +0x60..+0x6c; the slot index goes to the record's shadow index (word 9,
  read by the lighting as `record[1].w & 0xffff`, the layer of
  `gSpotShadowmapArray`), the params to words 0x3c..0x3f, and +0x70/74/78
  divide the light's +0x94 (the shadow map size per type: 1024 / 2048 /
  2048 in our no-shadow packing). The slots are an LRU cache
  (`141db4ac0`: per light type, evicting the least recently used slot not
  used this frame, dynamic before static; `141db4790` / `141db45f0` free
  them). The lighting permutations sample the array with a comparison
  sampler through record vec4s 6..10 (projection, atlas scale/offset at
  8.yzw/9.x, depth scale/bias at 10.yz) with an 8-tap kernel when
  `LightingGlobals` word 58.y is set. Still to read: who fills the 0x90
  state (the slot's atlas rectangle and params), which surfaces each slot
  draws (`141d71d90`: static models into lists 0x53+i / 0x9b+i, skipping
  draw records with +0x60 bit 0 or +0x70 set) and with which technique,
  and the omni cube layout.
* **ADS blur** (`t7 ads blur:` lines; on, `BO2MP_T7_ADS_BLUR=0` off). What
  Black Ops III shows while aiming is not the `dof/*` chain: `141d5fd80`
  (the DoF frame, first in `141d66110`) runs only when the view's DoF mode
  (refdef `+0x38` word bits 16..17, view `+0xb2636 & 3`) is 1 (near CoC,
  combine) or 2 (far only); its table entry 0 (`dof_ads_*`) is never
  reached. The mode is set by `1405b4bf0` for view type 0xb (1 or 2 by a
  per-entry byte) or a client state 5 with a dvar; aiming leaves it 0.
  Aiming drives the ADS blur instead: the ADS amount (refdef `+0x68`) is
  `fWeaponPosFrac` (`14118e440`; zeroed for two pm flags), the start
  fraction / scope value (`+0x6c` / `+0x70`) the weapon's (`1427d0ac0`:
  def `+0x1260` / `+0x125c`, attachments `+0x110` / `+0x10c`). With
  mode 0 and an amount above 0, `141d61d40` draws `_l1` as
  `bloom_downsample_l1_adsblur` (`_scope` above 0.5, `_scene` with SSR)
  when the bool dvar `0x6511eacd` (default 1) is on: target 1 (role
  `0xb0e42`, quarter) is the quarter scene with the CoC in alpha from
  render target 0x25 (`downsampledDepth`, which the pre-pass `141d4c3f0`
  makes for it with `downsample_depth_with_uv_4x_compute` over `floatZ`):
  linear depth `0.984375 / floatZ`; viewmodel texels (negative) take
  `sat(d·c0.x + c0.y)` of their band depth, the world
  `sat(c0.z·z + c0.w)·c1.x`. `141d63100` then draws `adsblur_large_x`
  (→ `0xb0e44`), `_large_y` (→ `0xb0e46`), `_small` (`0xb0e46` +
  `0xb0e42` → `0xb0e44`) and `adsblur_update` (scene, depth, `0xb0e44` →
  the scene the grade reads: CoC = min(blurred CoC, viewmodel band term)
  · amount). Constants `141d60f10` (formulas in `crate::post_ads`) from
  the physical camera `1405b2e80` fills: focal length `7 / tan(atan(
  tan(fov/2)·0.5625))` mm, focus (dvar, 500), f-stop (dvar, 108; the
  weapon's per-stance clamps in `1405a02a0` not read: our weapons are
  Black Ops II's). With the defaults the world within about 62 units
  (zNear 4) blurs at full ADS, nothing beyond. Built: all of the folded
  path (`_scene` too); the game side writes `fWeaponPosFrac`
  (`frame::settings::BO3_ADS_AMOUNT`, 0 while a scope overlay is up: the
  exe's scope variant keeps the lens sharp by stencil 128, which our
  full-screen overlay stands in for). Unit tests (`post_ads`): the
  focal length, the constants (near term's zero at band depth 0.98441,
  the far term, the scope rescale), and the passes on a 128x64
  checkerboard with depth 10 / 1000: CoC 0.562 on the near half, 0 on the
  far; the near half's contrast 0.90 → 0.07, the far half kept; ADS 0
  leaves the scene. Not seen in a map frame (memory). The `dof/*` chain
  (16 passes, roles `0xb0de0..0xb0df8`) is not built: no multiplayer
  view sets its mode.
* **Ambient sound and rooms** (`t7 ambience:` lines; on,
  `BO2MP_T7_AMBIENCE=0` off). A map's sound lives in its own zone's
  `SndBank` (`mp_metro.all`: 102 aliases) and its banks
  (`zone/snd/all/mp_metro.all.sabl`, FLAC; the shared room tones in
  `mp_common.all.*`). The 0x184 records after the ducks are its ambient
  rooms: name, name hash (0x40), word 0x44 = 1 on the default room
  (`mp_metro_outdoor`), radverb hash / name (0x48 / 0x4c), dry / wet
  (0x114 / 0x118: 0.7 / 1.0 indoors, 1 / 1 outdoors), the room tone's
  alias id (0x11c: `amb_wind_exterior_2d` outdoors, `_interior_2d`
  indoors) and the snapshot (0x120, `cmn_swimming` underwater). The
  bank's inline area copies the map's `ambient_package` triggers (name,
  brush, priority, hash, then their planes at 0x409a); the entity string
  has the same triggers (`script_ambientroom`, `script_ambientpriority`)
  and Black Ops II's sound structs (`script_struct` + `script_sound`:
  `random` with waits, `looper`, `line_emitter`). Built
  (`asset_audio::bo3_map_sounds`, `audio::ambient::update_room_tone`):
  the map's aliases (all of them, with mp_common's banks for the tones),
  rooms with the default first, room tones swapped as the server's room
  (`bo2zm_room`, the triggers Black Ops II's maps already use) changes,
  loops and line emitters, now-and-then sounds. Offline (Metro, staged
  bank): 5 rooms, 4 tones, 50 loops, 268 now-and-then sounds, 9
  placements of 4 aliases without a clip; `amb_gust_cold` decodes (48
  kHz, 353548 frames). Not built: the reverb (BO3's `SndRadverb` is 23
  floats, two 8-float early/late groups, layout not decoded; nothing is
  echoed rather than a guessed mapping) and the snapshots (ducking).
  Not verified: in a game (the server's room choice on a BO3 map, the
  tones from mp_common's banks).
* **Other items, where they stand (not built).** Lens flares: no
  separate sun flare in the exe (no technique but `lensflare*`;
  `r_sunFlareTint` is unused); flares are `klf` effect elements. The draw
  (`141d65290`, in the post frame after the ADS blur, before the grade)
  walks the view's source list (view `+0x510`, 24-byte entries, count
  `+0x1000`) into role `0xb0e00` (the bloom's half-size level), one
  `lensflare` draw per klf element and instance (element +8 the count,
  `+0x1b8` its constants, `+0x1e0` its image; the def's `+0x68` / `+0x98`);
  the occlusion queries (`lensflare_query`, `141582210`) write the two
  32-byte-element UAV buffers `LensFlareManager` makes (`14157ef90`, a
  28-vertex fan). Still unread: the source list builder (from the effect
  player's lens-flare elements) and `gEmitterStatic`; a flare made up
  without them would not be the exe's, so none is drawn. Spot / omni
  shadows: the state array is reset by `141db5010` (3000 states per
  client), slots found per type over up to four size tiers (`141db4c90` /
  `141db4ac0`: evict a slot not used this frame, oldest first, larger
  tiers after); the passes that render a slot (`141dac790`, `141dad0c0`,
  `141dcc390`, `141dd1db0` call into it) are not read; not built.
  Destructibles (`destructibledef`) and scripted brush movers: they are
  run by the map's own scripts and BO3's destructible code, neither of
  which runs in a Black Ops II match on a BO3 map (the brush models stay
  where their entities are); not built. Motion blur, SMAA, the ADS blur
  and the ambience: above.

## 7. Priority (visual impact on mp maps, excluding other agents' items)

1. Vegetation wind (done above).
2. SSR permutation set (built, opt-in; next: a lit frame with it on the PC).
3. Lens flares (sun flare on outdoor maps; `r_sunFlareTint`).
4. Volume decals (done), then forward decals.
5. SMAA: 1x built (default on); T2x needs the projection jitter.
6. Spot shadow maps (indoor lights' shadows).
7. ADS blur done; the `dof/*` chain (scripted views only); motion blur done (Auto, as the exe).
8. fxanim playback, destructibles, ambient sound/reverb zones (game side).
