# MWR (h1) to BO3 parity: progress tracker

The owner's goal: Modern Warfare Remastered maps run 1:1 like the real game, the way Black Ops III does here
(`docs/T7_NATIVE.md`). Method: read `h1_mp64_zt.exe` statically in Ghidra, translate the logic into our Rust
(`render_h1`, `fastfile_h1`, `assets::lane::h1_map`, `render_fx::h1_rules`), make it the default, keep an env
switch for the old behaviour, check it with offline GPU renders (`render_h1` example `h1_native`), push to `main`.

**Handover for the next agent.** Read this file, then `CLAUDE.md`, `docs/H1_NATIVE.md` (what was read and how),
`docs/exe_notes/` (exe addresses, Ghidra how-to and scripts). Update the checklist below with every push.

## Environment (owner's PC)

- Repo: `C:\Users\Gaming PC\bo2\bo2-multiplayer-rebuild`. Game build: `cargo build --profile play -p launcher`.
- Zones: `C:\Users\Gaming PC\Documents\mwr_game_files\zone\` (all `.ff` and `imagefile*.pak`). Exe:
  `C:\Users\Gaming PC\Documents\mwr_game_files\h1_mp64_zt.exe` (static analysis only).
- Ghidra 11.4.2: `C:\Users\Gaming PC\tools\ghidra_11.4.2_PUBLIC`, JDK `C:\Users\Gaming PC\tools\jdk-21.0.13+9`,
  project `C:\Users\Gaming PC\tools\ghidra_proj` (name `h1`, program `h1_mp64_zt.exe`), scripts
  `C:\Users\Gaming PC\tools\ghidra_scripts` (copy in `docs/exe_notes/ghidra_scripts`). Headless:
  `analyzeHeadless.bat <proj> h1 -process h1_mp64_zt.exe -noanalysis -readOnly -scriptPath <scripts> -postScript Dec.java <out> dec:ADDR ...`.
  On Windows pass range/bytes args with `_` (`'bytes:0x140890c40_16'`): cmd splits on commas. Hex needs `0x`.
- Offline render (Shipment needs its techset and patch zones):
  `target\release\examples\h1_native.exe mp_shipment.ff techsets_mp_shipment.ff patch_mp_shipment.ff techsets_common_mp.ff code_post_gfx_mp.ff common_mp.ff --cam X Y Z PITCH YAW --out pic.png`.
  Useful: `H1_FOG_OFF=1` (shows unlit/undrawn surfaces as black instead of fog grey), `H1_HIDE=<material words>`,
  `H1_DBG_MAT=<material words>` (slot, technique, state per draw), `H1_DBG_WHITE=<pixel shader words>`,
  `H1_DBG_LM=white`, `H1_DBG_CULL=none`, `H1_DBG_DEPTH=ge|always`, `H1_DBG_NOPREPASS=1`.
  Shipment views used: yard `--cam -500 -500 600 30 45`, eye in the yard `--cam 0 0 260 0 0`.

## Checklist

Status: [x] done and pushed, [~] in progress / partly, [ ] not started.

### 1. Lighting
- [x] Blended surfaces with the split-sum lookup draw (R16G16 pictures read). `4e41998`
- [x] Static models lit from the palette light grid (L2 SH entries, 56 shell colours). `ac8b5d0`
- [~] Black ground and containers on Shipment. Found and fixed so far:
      (a) displacement world sets (`*NNNn_NNNdn...`, floor patches) fill only the no-displacement slots
      193..195; `pick` falls back to them (`d91cf39`, `BO2MP_H1_DISP_FALLBACK=0` off).
      (b) their vertex programs read `TRANSPOSE_PROJECTION_MATRIX`, `INVERSE_TRANSPOSE_WORLD_VIEW_MATRIX0`
      (per object: inverse world-view, rows' w = eye in object space) and `TESSELLATION_PARMS(2)`; the first
      two are now filled (read from the shaders' use), the tessellation pair has neutral INFERRED values
      (1,0,1,0)/(0,0,0,1) (the engine builds them from the light set keys `tessellationfactor`,
      `tessellationcutoffdistance`, `tessellationcutofffalloff`, key indices 0x64/0x66/0x68; the formula is
      not read yet; dvars with defaults 960 and 320 are registered beside them).
      (c) model lightmaps, see below: the yard's containers are model-lightmapped.
      Still black after (a)-(c) (31 Oct): the yard floor and the container roofs. Checked and ruled out:
      depth test, culling, pre-pass, constant buffer contents (`H1_DBG_CB` dump is right), vertex layouts
      (`H1_DBG_ATTR`), vertex fetch (positions forced by `H1_DBG_VSPOS` draw for whole groups). Single-draw
      tests (`H1_ONLY_FIRST`) gave zero pixels even for known-good draws, so treat them as unreliable.
      Note: the owner's game (`CodLegacy.exe`) can hold ~11 GB of VRAM; offline renders then hit GPU
      out-of-memory and draw black. Use `H1_MAX_TEX=512` (h1_native now prints the first wgpu errors).
- [~] Model lightmaps: flag 0x100 placements (Shipment: 33 containers `ml/h1_shipping_container_03_*`, a
      few others) read `modelLightmapInfo` (offset float2, scale float2, page u32) and the surfaces'
      `lmapUnwrap` (half2 a vertex, values 0..1); the model vertex is now 36 bytes with the unwrap as
      `TEXCOORD4`, `MODEL_LIGHTMAP_PARAMS = (scale, offset)` per object (order INFERRED from names; the
      shader does `xy * uv + zw`), the draw takes the placement's lightmap page. Pipelines now build
      (the TEXCOORD4 skips are gone).
- [x] **FIXED (31 Oct): the black yard.** Cause: technique sets that reference a technique an earlier set
      loaded got an empty name for it (the reader followed the header's name pointer, not always a string
      it read), so shared `zprepass` / `build_shadowmap_model` were missing; with no depth pre-pass the lit
      pass (no depth write) left those pixels at sky depth and the fog pass painted over them. The walker
      now keeps every technique it reads by header offset (`Walker::technique_names`). Shipment's yard
      renders: `docs/screenshots/h1_shipment_yard_native.jpg`. Follow-ups: auto-exposure overexposes
      eye-level views in the yard; 6 draws want `TEXCOORD5`; check the model lightmap scale/offset order
      on the containers (they now draw, lit). The debug notes below are kept for the record.
- [x] (was) **Open bug: the yard's models produce no fragments at all.**
      Containers (`ml/mtl_h1_shipping_container_03_*`), their tarps (`mc/mtl_h1_cs_tarpaulin_*`), gravel and
      barrels: their draws are recorded (`H1_PROFILE=1` counts them), no wgpu error is raised (h1_native
      prints the first three), yet nothing lands in the scene target. Reproduced in isolation:
      `H1_ONLY=mc/mtl_h1_cs_tarpaulin` gives 0 pixels; `H1_DBG_MATSWAP=<container>,m/mtl_lightdrygrass_shp`
      (container geometry, grass material) draws; `H1_DBG_MATSWAP=lightdrygrass,<container material>`
      (grass geometry, container material) draws nothing. So it follows the MATERIAL. Ruled out: textures
      (`H1_DBG_TEXDEF=1` binds placeholders everywhere), shader bodies (`H1_DBG_VSVID=.hlsl` places
      vertices by index, `H1_DBG_PSWHITE=1` + `H1_DBG_WHITE=.hlsl` outputs white), render state (same as the
      grass: printed by `H1_DBG_ATTR`), depth (`H1_DBG_DEPTH=always`), culling (`H1_DBG_CULL=none`),
      pre-pass (`H1_DBG_NOPREPASS=1`), clip distances (none), backend (`WGPU_BACKEND=vulkan|dx12`), the
      per-object stride (`H1_DBG_STRIDE256=1`), the model lightmap rows (`H1_DBG_NOMLM=1`), the TEXCOORD4
      offset (`H1_DBG_TC4=1`). Left: the pipeline layout / bind group layout made from these shaders'
      reflection, and the frame constant buffer bindings (`make_groups` allocates a 16-byte binding when a
      plan has no frame buffer for a slot). Next step: a GPU capture (RenderDoc) of
      `h1_native ... H1_ONLY=mc/mtl_h1_cs_tarpaulin` to see whether the draw executes and what it binds.
      The world's yard floor (`*247n...` displacement patches, `*130n...` layered) behaves the same way.
- [ ] Light grid tree lookup for moving models (cell 32x32x64 from -131072, trilinear corners, tree walk).
      DEFERRED: `render_h1` draws only the map (world + static models); players, guns and dropped items
      are drawn by the BO2 renderer, so nothing would use it yet. Read so far (addresses in
      `docs/exe_notes/MWR_H1_LIGHTGRID.md`): the point lookup takes the 8 corners from the tree at
      lightGrid+0x348 (one call per corner), drops corners failing a leak trace, merges equal palette
      entries by weight and averages the sun visibility; the tree node decoder is a context-dependent
      bitstream (several helpers), not translated.
- [x] Automatic exposure in game (`render_h1::meter`): GPU log-average luminance, MWR's target / adapt /
      adjust / cap; default in game, `BO2MP_H1_EXPOSURE=fixed` for the old 1.0. Open: eye-level sunlit
      surfaces clip on Shipment; needs a real-game screenshot to settle sun / default tone map.
- [ ] Sun visibility from the light grid; optional contrast tweak (two dvars).
- [~] Spot and omni lights drawn natively: constants built (`render_h1::local_light`, 3 Nov); the lit pass slots are still unknown, so nothing draws yet.
- [ ] Reflection probes: confirm blending.

### 2. Shadows
- [x] Sun shadows ON by default (31 Oct; `BO2MP_H1_SUN_SHADOW=0` off). Checked on the GPU after the shared-technique fix:
      barrels/props shade and cast, grass clean (the earlier speckle/no-caster report predates `ba7ef55`).
- [ ] Spot/omni light shadows.
- [ ] Cached shadow tiles drawn, per-cascade scissor, small-map mode.

### 3. Post effects
- [x] Fog as the exe evaluates it: builder (`look::fog_exact`), every map's `_fog_hdr.gsc` found (zone walk
      fix), the map's own vision-set block, `ATMOS_FOG_PARMS_6.w`, the exe's post-opaque variant pick
      (`_hf`, `_flf` sky falloff). See Log 31 Oct (night).
- [ ] Bloom, glow, veil (`VEIL_PARAMS`).
- [ ] SSAO / MDAO.
- [ ] Depth of field.
- [ ] Motion blur.
- [ ] Anti-aliasing.
- [ ] Screen-space reflections (`SSR_*`, `SCREEN_SPACE_REFLECTION_PARAMETERS` are read by shaders, unfilled).
- [ ] Exposure pass and light-set colour grading confirmed.

### 4. Effects
- [ ] Sprite lighting on by default.
- [ ] Model elements.
- [ ] Trails, decals, lens flares.
- [ ] Exploders.
- [ ] Effects lit by the light grid (needs the tree lookup).

### 4b. Sound
- [~] Map createfx sounds (`createLoopSound`, `createIntervalSound`, and an effect's own `loopsound`) placed and played (3 Nov; written, not heard).
- [ ] `ambientPlay` bed, ambient rooms and reverb, sound triggers, the exe's default falloff curve.
- [ ] Sounds held in sound paks (primed heads only today).

### 5. Map logic
- [ ] Map scripts from the patch/common zones run (`fastfile_h1` `h1_scripts`).
- [ ] Movers (fans, windmill, hanging objects).
- [ ] Exploders fire on cue.
- [ ] Shipment out-of-bounds triggers.
- [~] Clutter and destructibles placed (Crash: 630 clutter, 15 destructibles): drawn in their starting pose via the static-model path (3 Nov; written, not seen). Not yet moved, broken or colliding.
- [ ] Vision/exposure volumes, sound/reverb areas.
- [ ] `trigger_hurt` START_OFF, helicopter height lock.

### 6. Collision
- [ ] Placed-model traces as the exe's code does them.
- [ ] Crash: floor near z 220 the player falls through (owner to confirm).

### 7. Checks
- [ ] Render sets per map (Shipment, Crash, Killhouse, ...) against real-game screenshots from the owner.
- [ ] Headless effects check like `t7_fx_check`.
- [ ] Full BO3-vs-MWR comparison in `docs/H1_NATIVE.md`.
- [ ] Remove the `H1_DBG_*` switches when done.

## Log (newest first)

- 31 Oct (night) **magenta foliage, black trees and the TEXCOORD5 skips fixed (retail and usermaps).**
  - Constant buffers per stage: D3D gives the vertex and pixel stages their own `b0..b3`; we filled one buffer per
    slot for both, so where a vertex and a pixel argument use the same register the later won. The wind techniques'
    vertex `flagWave0Parms` (6.28, 0, 12.57) landed in the pixel shader's `transmittance` (`b3[8]`): magenta
    grass/leaves on Bloc, Overgrown, Crash, Farm. Pixel-stage `b1..b3` are now their own buffers
    (`bind::buffer_key`; `b0` per-primitive stays shared; `BO2MP_H1_SHARED_CB=1`: old).
  - TEXCOORD4..6 of the `_vlc` techniques (the 6-21 "TEXCOORD5 not in a Model vertex" skips per map, item 4) are a
    vertex-lit placement's baked lighting (`GfxStaticModelVertexLighting`, draw flag 0x80, one per vertex of the model:
    visibility UNORM8 in .x, ambient colour and highlight colour as four halves, .w of each a direction term), bound
    as a second vertex stream of the model mesh (`StaticModel::vertex_lighting`, `VERTEX_LIT_WVF`). Read from the
    shaders (`lpo_sm_vlc_*`: diffuse = ambient + highlight x N.dir + visibility x sun) and the zone's struct layout;
    the visibility format UNORM8 from the data (0xff = lit). Bloc: 559 vertex-lit placements; its black pines,
    Broadcast's black overpass, Carentan's lamp post, Crash's car, Bog Summer's rubble now draw lit. No map skips a
    TEXCOORD5 draw now (Derail: 1 left).
  - Left: TEXCOORD1 on a few world/model draws (1-4 per map on Creek, Pipeline and some usermaps); one usermap
    picture missing from every zone (Seatown's `wood_split_log_brown_col`, 1,749 blended draws left out).

- 31 Oct (night) **legacy distance fog** (`look::fog_legacy`, `dfog` post pass): every usermap ships its own raw
  `maps/createart/<map>_fog_hdr.gsc` and vision (item 1 works for them unchanged); four (Big Ben 2, Highrise 2,
  Instinct, Lair) turn the atmospheric fog off and use the legacy fog, which drew no fog at all. Now the exe's legacy
  branch (read; key mapping inferred, `docs/H1_NATIVE.md` "Legacy fog"). Every map's fog mode is printed (`h1 fog:`
  note). 60 usermaps atmospheric, 4 legacy; all 24 retail maps atmospheric (unchanged).

- 31 Oct (night) **Horizon MW usermaps (MW2/MW3/AW/CoD4 maps ported to h1) in the offline check.** Script
  `allmaps_hmw.ps1` (owner's temp dir; copy of `allmaps.ps1`) renders all 64 `hmw-usermaps\<map>\<map>.ff` with the
  game's zone list (`assets::lane::h1::native_zones`: the map, `retail_zones` that exist, then `hmw-mod\zone\hmw_*.ff`
  sorted; `hmw_launcher.ff` is left out by `Scene::load`). All 64 load (15-20 s, nearly all of it the hmw zones) and draw.
  Fixed:
  - Streamed pictures a usermap names from the retail `imagefile<N>.pak`s were looked for beside the usermap: the paks
    are now taken from the install's zone folder (the one holding `common_mp.ff`), and a picture held by several zones
    falls back to the next when one does not read. Checkpoint 139 -> 0 missing pictures (14,895 blended draws were left
    out), Estate 115 -> 0, Invasion 91 -> 0, Fracture 54 -> 0, Rust 39 -> 0; no usermap misses a picture now.
  - Materials whose technique set is `2d` (Horizon's converted water, clear plastic, Estate's fences, ...) drew their
    model geometry through the screen-space menu shader: spikes across Estate's view. Left out of the 3-D scene
    (INFERRED, not read from the exe; `BO2MP_H1_DRAW_2D=1` draws them).
  - Debug: `h1_native` `H1_DBG_MESH=0|1`, `H1_DBG_LISTMATS=1`; `fastfile_h1` example `h1_model_dump`.
  Per-usermap status (480x270 default view, after these fixes): all 64 draw with sky and exposure; known: Paris' tree
  leaves draw white/lavender; many maps look dark at their intermission view (exposure/fog not yet checked per map);
  6-19 skip kinds per map are code constants we do not fill (SSR, `RENDER_TARGET_SIZE`, rim light) and TEXCOORD5.

- 31 Oct (night) **fog on every map, as the exe picks it.**
  - Why Crash (and most maps) had no fog: every `patch_mp_<map>.ff` holds an `addon_map_ents` asset early on,
    which the zone walk did not read, so the walk stopped there and lost every later asset, the raw
    `maps/createart/<map>_fog_hdr.gsc` (and the compiled createart scripts) among them. There is no "compiled
    only" fog: all 24 maps ship the raw script in their patch zone. `fastfile_h1::walk::world::addon_map_ents`
    reads it (x64-zt's writer order); all patch_mp zones now walk to the end (`h1_map` notes; only
    `patch_mp_vlobby_room` still stops, at an `animclass`). Tool: `fastfile_h1` example `h1_find_bytes`.
  - A `_fog_hdr.gsc` has one `create_vision_set_fog("<name>")` block per vision set (the map, interiors,
    `""`); we took every line, so the last block won. Now `Settings::parse_fog_script_for` takes the map's
    block (the base vision set is the map name; else `""`, else the first). `BO2MP_H1_FOG_BLOCK=last`: old.
  - `ATMOS_FOG_PARMS_6.w` (read, the per-view fog writer): `P2.w` (the max opacity: the shader uses P2.w for
    surfaces and P6.w for the sky in the same `T*op + 1 - op` form) times a float dvar, default 1 range 0..1.
    Was 1. `BO2MP_H1_FOG_SKY_OPACITY=one`: old.
  - The post-opaque pass (read): the exe indexes a material table (`postopaquefx_[ssaolq_|ssaohq_](fog|dfog)
    [_hf][_fsf]`) by `ssao | 4 atmos | 8 sun-fog | 0x10 height | 0x20 sky falloff`: fog drawn when P2.w > 0;
    `_hf` when ln2 / height density < 99000; `_fsf` when the sky falloff range < 8910 degrees (9000 = off).
    We always drew `_hf`; the sky falloff (`_fsf` material, technique named `_flf` in the zone) was never
    applied, so Crash's sky (falloff 0..140 degrees) was fully fogged. `look::post_opaque_technique`;
    `BO2MP_H1_POST_OPAQUE=hf`: old.
  - Checked: allmaps before/after montage (Countdown, Farm, Crash now fogged; nothing else moved), Crash at
    960x540 with/without fog, unit tests (`look::fog_tests`), game build.

- 31 Oct (evening): **all 24 MWR multiplayer maps render offline** (`allmaps` check: every map loads in 1-2.4 s, no
  wgpu errors, skies and exposure right). Fixes, each pushed:
  - MWR maps use only MWR's renderer like BO3 maps (loading screen held for the first native frame via
    `t7_native_load`, converted world never drawn, opt-out removed); retail maps read retail zones only
    (Horizon MW zones cost 14 s; load 21.6 s -> 1.1 s); a shared zone that does not open is skipped
    (`hmw_launcher.ff` broke the native path in game).
  - View-frustum culling + redundant-command removal: 1080p frame 32.7 -> 15 ms offline, pictures identical.
  - `dxbc_sm5`: `lod` instruction (dozens of world shaders were skipped on every map but Shipment).
  - Fog: the exe's builder (`look::fog_exact`, field layout from the client fog-string parser), P1.w clamp
    (black disc at the sun), P4.w never 0 (NaN skies on maps without a `_fog_hdr.gsc`, e.g. Crash, whose fog
    comes from compiled scripts: those maps currently run without atmospheric fog — open item).
  - Models' `is_front_face` matches D3D (magenta two-sided foliage).
  - Sun shadows on by default; exposure metered on the 160x90 reduction as the exe does.
  Open: fog for maps whose fog is set by compiled GSC (read the art script's call or the map's vision
  settings), spot/omni lights, bloom/SSAO/DOF/motion blur, effects, sound, map scripts; 6-15 TEXCOORD5
  draws per map still skipped.

- 31 Oct (latest): **MWR's own renderer is now the default in game** (`assets::lane::h1::native_zones`;
  `BO2MP_H1_NATIVE=0` for the converted world). Sun shadows default on (`0592ae0`).
- 31 Oct: **fog, exe builder found, translation pending.** `ATMOS_FOG_PARMS_0..3,5` are written into the
  per-view block (block offsets 0xde0..0xe3c = slots 222..227; slot 222 = P0) by `FUN_140629ce0` (atmos
  branch, when the fog state's byte at +0x540d0c is set; the other branch fills the legacy FOG_* slots).
  P4/P6 are rotated into view space later by `FUN_140084520` (third pass). The builder reads the fog state
  at `frontend(DAT_14feafb00) + 0x540cd0` (0x9c bytes; atmos fields from +0x540d10):
  - c1 = colour bytes +0x540d64..67 (/255, then `FUN_1406267f0`, likely sRGB->linear), c2 = bytes +0x540d68..6b;
    a = +0x540d10 (blend), s = +0x540d5c (scale);
  - P0 = (c1 (1-a) s, [+0x540d24]); P1 = (c2 a s, min(2 [+0x540d14] - 1, k)); P2 = (max(P0+P1, eps), [+0x540d1c]);
    P3 = (r / P2, -[+0x540d20]) with r = `FUN_1405d8160()`; P4 = (transform(+0x540d34), [+0x540d28]);
    P5 = (1/w, -(t/w), [+0x540d60], f) with w, t from the angles at +0x540d30 / +0x540d2c (degrees, floored).
  - The state is a blend (`FUN_1400ed430`) of fog states kept per index at `0x14fe6f514` (7 x 0x9c per client),
    stored by `FUN_1405d81c0` from the vision-volume code (`FUN_1400b16f0`, `FUN_1400b2580`, `FUN_1400c6b00`,
    `FUN_1400ccc50`, `FUN_1402346c0`). The `G_SetFog` string order (h1-mod `fog_editor.cpp`): start, ln2/half,
    transition, sunFogColour rgb, hazeColour rgb, hazeStrength, hazeSpread, extinction, inScatter,
    distanceScale, skyDistance, falloffStart (-90 off), falloffRange (9000 off), sunDir, heightBase,
    ln2/heightHalf. NEXT: map each string field to its +0x540dxx slot through `FUN_1400b16f0`'s packing
    (colours via `FUN_1400a00d0`, reciprocals `1/x`), then replace `look::fog` with the builder above.
    Our `look::fog` has a different structure (P0 = haze + density, P1 = sun colour x haze strength, P2 =
    extinction) and stays until then.

- 31 Oct (later): **meter now meters what the game meters.** The exposure dispatch (`FUN_140084d30`) runs
  `cs_logaccumfromtex2d` on the scene reduced by halving until it fits 160x90 (`FUN_140085fb0`'s loop), over
  the view rect clipped to the scene rect (`FUN_140085710`: view+0x170 ∩ view+0x180, the full scene).
  `render_h1::meter` box-filters to that size before the log (was every 4th raw pixel). Shipment: eye level
  luminance 0.051 → 0.090 (exposure x3.5 → x2.0), reference view 0.325 → 0.46. Sunlit walls at eye level
  still clip: next is the sun term (LIGHT_DIFFUSE path already exe-read: colour unscaled, scales 1).
  Also checked: `postfx_color2` does its own sRGB encode (output display-ready, both paths right); the
  container colour/spec maps are UNORM (BC1/BC3) and the shader squares them, as we bind; the container
  pixel shader's terms (diffuse = albedo² (1-spec²)(bounce + mask N.L sun), probe specular) all receive
  correct inputs (`H1_DBG_PSOUT` views).

- 31 Oct (late): verified against the exe and the zones, no change in behaviour yet:
  - `r_tonemap` default is Linear (enum index 3; registration with names array `0x140f72438`). Rust uses linear.
  - `tonemap_linear` pixel shader (zone): `saturate(E x)^gamma`, E = `cb4[3].x`, gamma `cb4[3].z`. Matches Rust.
  - The game's exposure is three compute shaders in `code_post_gfx_mp`: `cs_logaccumfromtex2d` (ln of Rec.709
    luminance clamped 6.1e-5..65504, every pixel in a rect), `cs_logaccumcontinue`, `cs_logaccumfinish`
    (disassembled with Microsoft's `d3dcompiler_47.dll` `D3DDisassemble`: our translator lacks buffer
    resources). Finish = locked target 0.18*2^adjust / L, ln-space adapt by speed when L > 6.1e-5, then
    adjust stops and the max-EV cap; row3 = (E, E*L*cb0[7].x, cb0[7].y, L). `render_h1::meter` matches it.
  - Model lightmaps: data is (scale, offset) as the VS uses (`xy*uv+zw`); pages 0..2 = `*lightmapN_*`.
    Primary page is BC4 (sun visibility in .x), secondary RGBA16F two halves (bounce ~0.5-2, max 19).
  - So the overexposure is not exposure/tone map: next is how the exe fills `LIGHT_DIFFUSE` /
    `LIGHT_SPECULAR` (sun colour 15 goes in as is) — code-constant writers around `0x1406213xx`.
  - Tools: `h1_dump_shader list-cs <word>`, `cs` stage, `H1_RAW=<file>` raw bytes; `h1_image_stats` RGBA16F
    percentiles; `H1_DBG_PSOUT=<word>|<wgsl expr>`; `H1_DBG_CB` lists writes; `H1_DBG_MLM_PRINT`.

- 31 Oct: model lightmaps wired (unwrap, info, params, page), projection / inverse world-view constants,
  neutral tessellation constants, debug tools (`H1_ONLY`, `H1_ONLY_FIRST`, `H1_DBG_CB`, `H1_DBG_ATTR`,
  `H1_DBG_VSPOS`, `H1_DBG_B0ID`, `H1_DBG_TESSZ`, `H1_DBG_NOLAYER`, `H1_DBG_DUMP`, `H1_MAX_TEX`,
  examples `h1_draw_probe`, `h1_draws_in` lists, `h1_smodel_census` flags and model lightmaps).

- 31 Oct: displacement fallback, debug switches (`d91cf39`); exe notes in repo (`eb1a480`); game build fix
  (`497a846`); palette light grid (`ac8b5d0`); diagnostics (`0724b60`); R16G16 pictures (`4e41998`).
