# IW7 engine pass: plan (2026-11-03)

Goal: do for Infinite Warfare what was done for MWR and BO3: read everything the zones say about how guns, camos, scopes, effects, sound and collision work, and translate it into the BO2 engine. Written from the repo's own readers; nothing here was run against zone data (none is staged in the cloud), so every item below needs the owner's PC to verify.

## Source rule
Data comes from the zones (`fastfile_iw7`) and from the exe only as the owner's own, statically analysed install (see `CLAUDE.md`: no unpacking or bypassing of protected exes). An already-unpacked exe was offered in chat on 2026-11-03; it was not used for anything here.

## What already exists
- Weapon defs, attachments (`walk::weapon::attachment`: camo model and material arrays, reticles, alt reticles, `szAltModeName`, scope drift, `adsAltSwitch`) and the vfx reader are all read by `fastfile_iw7`.
- The IW7 gun lane (`assets::lane::iw7_gun`) reads none of the attachment data, and sets `alt_weapon_name` empty.
- The catalog already turns `alt_weapon_name` into `alternate_weapon` (`weapon_catalog_s1.rs`), and BO2 has an alt-weapon / hybrid-sight path (`t6_m2.rs`, `weapon_catalog_t6.rs`).
- Camos: `asset_core::weapon_camos` (swap, pattern, effect camos, per-game number ranges; IW7 has none yet). Every gun already wears every catalogued camo.

## Work items, in order
1. **Alt-mode (dual-mode) weapons.** Read `szAltWeaponName` (already in `rec.strings` as `WeaponField::Complete`) and the attachment's `szAltModeName`; build the alt def as its own guest row; set `alt_weapon_name` on the first. Needs: a probe listing which of the 41 guns have an alt def and what it is; alt ammo, alt reticle (`altReticles`), `stateTimers__altRaiseTime/DropTime` and `altSwitchSound` (already mapped in `iw7_gun_fields.rs`) check.
2. **IW7 camos for every gun.** Add an `IW7_FIRST` range to `weapon_camos`; read the camo table zone asset and the camo pictures (`camoColorMap`) into `PatternCamo` rows via a new `install_iw7_patterns`, as `install_t7_patterns` does. Model-swap camos (a per-camo model on the gun) need the attachment / weapon camo model arrays already walked. Check `camo_skips_material` against IW7 material names.
3. **Attachments and optics.** Read the attachment arrays in the lane; scope reticle tags, reticle scale by distance, parallax. Dual-view (second FOV) scopes are a render feature: start with the reticle, then the second pass.
4. **Weapon vfx.** Open items from `IW7_GUNS.md`: the aimed flash (`_v_ads`) has no BO2 slot; shell casings; explosion and trail slots; dropped modules (lights, beams, decals, trails).
5. **Lighting / viewmodel look, sound ambience, collisions.** Map side is covered by `iw7-look` (project doc). Remaining: viewmodel light tweaks, per-map ambience from the sound banks (`common_mp.sabl` is not staged), and collision from the world asset (`walk/world.rs`, `walk/physics.rs`).

## Verification needed from the PC
`CODLEGACY_IW7_ZONE=<dir> cargo test -p assets --lib iw7_gun -- --ignored --nocapture` before and after each item, plus screenshots for anything visual.

## Why the lighting, vfx, sound and collision items are not started (2026-11-03, overnight)
The cloud session has no zones or paks (searched the whole container), so nothing that depends on byte layouts or real values can be checked: spawn-shape and light modules in the vfx records, the clip map's brush data (Havok collision), sound-bank ambience, viewmodel light values. Writing those blind would be guessing, and the repo rule is to merge only tested work. `fastfile_iw7` builds (`cargo check -p fastfile_iw7`), so the first session with the zones staged can go straight to these probes:

| Item | Probe to run first (existing tools) |
|---|---|
| vfx dropped modules (spawn shapes, lights, beams) | `iw7_vfxdump <zone>` (module histogram), then `iw7_vfxdump <zone> <name>` on a flash with a spawn shape |
| collision beyond the triangle soup | `iw7_bmodels <map>`, `Iw7Map` clip map walk in `walk/world.rs` / `walk/physics.rs`: count brushes, planes, bounds against the soup |
| sound ambience | `iw7_snd_probe <dir> decode amb_` after staging `common_mp.sabl` |
| lighting | `iw7_report` with `IW7_VISION`, `IW7_PROBES`, `IW7_LMSTATS`; screenshots for the gaps listed in `iw7-look` |

## Findings with real data (2026-11-03)
- **World collision is a Havok 2014.2.5 packfile** (`hknpCompressedMeshShape`, 9.9 MB on mp_geneva). Its outer layout is mapped (1448 sections, packed vertices, index streams); the triangles are not decoded yet. Offsets and tables: `docs/exe_notes/IW7_HAVOK.md`.
- **mp_riot walks to its end (fixed 2026-11-03).** An animation's `randomDataByteCount` is 32 bits; it was read as 16. mp_riot's `riot_ca_destroyer` (the destroyer set piece, 3600 frames) has 1,164,162, read as 50,050, so everything after it in the zone was read out of step: the glass world, the path data and the clip map came out as noise. Now riot reads 1929 path nodes (23,642 links; 0 before) and its clip map (10,465 static models, an 11 MB Havok mesh). Trace in `docs/exe_notes/IW7_HAVOK.md`.
- **Light probes**: the 27 halves are channel-major (9 terms of red, then green, then blue). IW7's `lprobe` vertex shader evaluates the first-order terms with the direction's (x, z, y) and the factors (-0.4886, +0.4886, -0.4886): term 2 is up, term 1 is -x, term 3 is -y. The probes' data agrees that term 2 points at the sky (both maps).
- **Players, guns and moving things now take the probes' direction** (`BO2MP_IW7_PROBE_DIR=0` gives the old "a quarter brighter from above"): `ProbeIndex::light_and_direction` returns the probes' first-order light as a fraction of their average (luminance, at most 0.9 long), and the lane's light grid stores each palette colour per direction as `1 + d . n` (d in quarters). On mp_riot: 749,670 of 913,140 grid points carry a direction, 31,374 palette colours (of 65,535), mean swing 0.39, mean up +0.07. Not seen on a screen.

Probe tools added: `iw7_havok` (dump the world's Havok shape), `iw7_find_bytes` (offsets of a byte pattern in a decompressed zone).

## Map ambience, the BO3 way (2026-11-03)
As `asset_audio::bo3_map_sounds` reads a Black Ops III map, `asset_audio::iw7_map_sounds` reads an Infinite Warfare map's own bank (`<map>.all`; `BO2MP_IW7_AMBIENCE=0` turns it off):
- **Aliases** of the map's bank, with clips from `<map>.sabl` / `.sabs`. A clip not there is looked for in the patch and shared streamed banks (`patch_<map>`, `patch_code_post_gfx`, `code_post_gfx`, `patch_common_mp`, `common_mp`). Only the bank's index is read when opened, so the 8 GB `code_post_gfx.sabs` is cheap.
- **Rooms:** the bank's sound table (`SndBank::soundTable`, now kept by the walk as `SoundTableRec`) holds 73 sound zones (`ZoneDef`). The map's own zones (those looping `amb_<map>_...`) become the rooms. Each room carries its reverb's room type, dry and wet, and its loop (`ambientStream`) as its tone.
  - mp_riot: 23 rooms (`mp_riot`, `riot_ext_street`, ... `riot_sm_pipe`), all looping `amb_mp_riot_lr`.
  - mp_geneva: 23 rooms, all looping `amb_mp_geneva_lr`; the clip decodes from `mp_geneva.sabs` (36 kHz stereo, 149 s).
  - The other zones are the game's states (shell shock, killcam, killstreak cockpits) and are left out.
- **Playback** is the existing room tone player. Map aliases are now found in their own namespace (`SoundCatalog::map_alias_ns`), for IW7 and BO3 alike.

Not done, and listed:
- **Which zone a player is in** (done, same day). The walk keeps the map's client triggers (`ClientTriggersRec`). An audio trigger (types 2 and 66) names its sound zone by an offset into the triggers' string table, and its hulls (a box cut by slabs, each slab measured from its box's middle) are the volume.
  - riot: 51 room volumes; geneva: 49.
  - `ScriptedMapFx::room_volumes` / `room_at`: the room tone player picks the room holding the listener when the server sets none, else the default room.
  - Where volumes overlap the smaller wins, because the game's own order is not read.
  - On the maps read every room loops the same tone, so this is audible only once the echo exists.
- **Echo** (done, same day; an earlier note here was wrong: the values are in the zones). Each bank's `sendEffects` are its reverb presets (`SndSendEffectParams`, 136 bytes: name, id, 17 values). They become radverbs (`iw7_radverb`):
  - IW7 has a proximity late gain after the late gain, dropped;
  - and a rear level where BO2 has a return high-pass, left at 0.
  - riot and geneva: 25 presets each, and all 23 rooms find theirs (`gen_ext_street`: return 0.18, late size 4.1). The same table is in `code_post_gfx`.
  - The room echo (`audio::room_echo`) now follows the listener's room on IW7 maps, as the tone does.
  - Not heard yet.
- **Ambient one-shots.** `AmbientDef` / `AmbientEvent` / `AmbientElement` are read; the multiplayer maps read have none.
- **Riot's tone clip.** It is in neither `mp_riot.sabs` (empty here) nor `patch_code_post_gfx.sabs`. It is probably in `code_post_gfx.sabs`, which could not be staged to check.

## Lighting the BO3 way: stage 1 (2026-11-03)
The BO3 and MWR passes (`docs/T7_NATIVE.md`, `docs/H1_NATIVE.md`) read two halves:
- **The zones** give the shaders and data: G-buffer and forward programs, the lighting compute shaders, light grids, probes and visions.
- **The exe** gives which passes run, in what order, which engine constant (`CONST_SRC_CODE_*`) is which, and how the code fills them. For MWR that came from the unprotected `h1_mp64_zt.exe`; for BO3 from the owner's `BlackOps3.exe`.

For IW7 only the zone half is open. The retail `iw7_ship.exe` is protected, and the copy on the owner's install is an unpacked one (iw7-mod needs it), which the repo rules keep off limits. What the zones give, read on `techsets_mp_riot`:
- **1081 world programs** (`lmap_*`, `lprobe_*`, ...: 953 pixel, 128 vertex; `iw7_dxbc <zone> <dir> lmap` dumps them). They are forward+ programs (`fplusbt`, `cluster`): one pass lights a surface from lightmaps or probes, the sun and its shadow, and clustered local lights.
- **Translation:** all 953 pixel programs now translate through `dxbc_sm5` and pass naga validation; before, 14 did. The 939 others failed on one thing: `ld` from a typed `Buffer` SRV (t36 and t37, the cluster light lists), which the translator now reads as a word array (`Options::srv_buffer_words`; test `dxbc_sm5/tests/typed_buffer.rs`). 101 of 128 vertex programs translate; the other 27 write no position. Check with `dxbc_coverage <dir>`.
- **No reflection:** IW7's shaders ship without an RDEF chunk, so constant buffer layouts cannot be read the way `t7_rdef` read BO3's.
- **Which constant feeds which register** is in the techniques' argument tables (`MaterialShaderArgument`: type, stage, destination register, code-constant index, first row, row count; walked in `walk/material.rs`, not kept). The **names** of those indices are not in the zones: they are the exe's table.

**Argument tables, kept (2026-11-03):** `TechniqueSetRec::passes` holds every pass's programs and arguments (`PassRec`, `ShaderArgRec`); `iw7_tech_args <techsets.ff> [technique substring]` lists them. On `techsets_mp_riot` (1422 sets, 2017 passes):
- **Argument types.** Read from the data:
  - **0** = code constants: a small index, rows (first, count) (0, 1), or (0, 4) for matrices.
  - **1** and **2** = code textures and samplers.
  - **3** and **5** = material constants and textures, by name hash.
  - No literal constants (type 4) appear.
- **Stages** are a bit mask: 2 = vertex, 16 = pixel.
- **What every `lmap_specenv` pass (776) takes from the code:**
  - Pixel constants: 8, 9, 17, 109, 141, 144, 145, 147, 151, 192 and 210 to 214, one row each.
  - Vertex: 19 (one row), and 304 and 320, four rows each into register 0 (the transforms).
  - The 18 passes with clustered local lights add 134, 146, 174 to 196 and 308.
- The indices are not named: the names are the exe's table.

Next, in order:
1. Name the indices from the data where it is safe:
   - the sun direction and colour (match the ComWorld primary light);
   - fog (match the vision's fog);
   - the view matrices (the vertex programs' position transform).
   Leave the rest unnamed and listed.
2. A `render_iw7` crate on `render_h1`'s pattern: the world drawn with its own `lmap_*` programs, fed the lightmaps, probes and the named constants; behind a switch, off until the owner has seen it on a GPU.

## Bots walk the map's own path nodes (2026-11-03)
`lane::iw7` hands `Iw7Map::path_nodes` to BO2's bots (`ScriptSources::foreign_path_nodes`, as the BO3 lane does with a donor's); `BO2MP_IW7_NODES=0` gives the engine's walk graph as before.
- **IW7 node types,** read against the entity classes:
  - 1 = `node_pathnode` (riot 1458, geneva 801);
  - 16 / 17 = `node_negotiation_begin` / `_end` (riot 163 each; the begin carries its traversal animation, `boost_jump_across_300`, `jump_down_80`);
  - 0 = empty (at the origin, no links).
- **Renumbering:** BO2 numbers a traversal 17 / 18, so IW7's 16 and 17 are renumbered.
- **Not named:** geneva's 58 nodes of type 20 are linked like path nodes and walk as such.
- **Kept as they are:** indices, links (with their negotiation flag) and names.
- **Not seen with bots in a match yet.** IW7's traversals are its own animations (boost jumps); BO2's bots play what their scripts know for an `animscript`.

## World renderer: `render_iw7`, stage 1 (2026-11-03)
A new crate on `render_h1`'s pattern (it reuses `render_h1::gpu`): IW7's surfaces drawn with their own techniques, translated by `dxbc_sm5`.
- **Shader programs** are kept by the walk (`Capture::shader_programs`, by asset id), so each pass reaches its own DXBC.
- **Binding model** (`render_iw7::bind`), checked by `iw7_bind_check <techsets.ff> [technique]` against every pass's translated programs: zero mismatches on all 776 `lmap_specenv` passes of `techsets_mp_riot`.
  - Code constants fill `b{group}`: b0 per primitive, b1 per object, b2 stable.
  - Material constants fill `b3`.
  - Texture and sampler arguments name their `t#` / `s#` slot.
  - Argument types: 0 code const, 1 code texture, 2 code sampler, 3 material const, 5 material texture, 6 material sampler.
  - `PassRec::cbuffer_slots` reads 0, 0, 26 everywhere and is not the slots.
- **What the engine supplies itself** (no argument feeds it) on the lightmap passes:
  - t2, t3, t4: the lightmap pages;
  - t24, t25: likely the sun shadow;
  - t31, t32, t33: likely reflection probes / environment;
  - t35, t36, t37: the cluster light buffers.
  Each still has to be proven from the shader code.
- Across all 1144 passes of the zone, 98 vertex arguments of other families (models) overrun their buffer: to look at with models.
- **A software GPU in the cloud:** Mesa's lavapipe (`mesa-vulkan-drivers`) gives wgpu a Vulkan device here, so pipelines can be built and frames drawn offscreen and looked at without the owner's PC.

### Stage 2, step 1: the raw vertex stream (2026-11-03)
- `Iw7Map::raw_vertices` keeps every world vertex as the zone holds it: `GfxWorldVertex`, 44 bytes, the same record as MWR's.
  - 0 position (3 floats); 12 binormal sign; 16 colour (bytes); 20 uv; 28 lightmap uv; 36 normal (packed); 40 tangent (packed).
- `iw7_vs_inputs <techsets.ff> [technique]` lists the vertex programs' input signatures. Every `lmap_specenv` program takes `SV_POSITION0`, `TEXCOORD0`, `NORMAL0` and `TEXCOORD2`. Most add `COLOR0`.
  - Layered materials add `TEXCOORD3` to `TEXCOORD10` and `COLOR1`, which come from the layer streams (not read yet).
  - The simplest family (`w_l_sm_replace_i0c0s0n0p0`, 16 passes) takes only the four.
- **Mapping to try first** (MWR's `render_h1::world::world_attribute`, same record):
  - `SV_POSITION0` = 0 as four floats (position, binormal sign);
  - `COLOR0` = 16 as UNORM8x4;
  - `TEXCOORD0` = 20 as four floats (uv and lightmap uv);
  - `NORMAL0` = 36 and `TEXCOORD2` = 40 as UNORM 10:10:10:2.
  - Check it on the frame, then from the vertex programs' use of each input.
- **Next steps:**
  - **Technique slot:** find the lit technique by name (`lmap_*`) in each surface's set.
  - **Material arguments:** type 3 constants and type 5 textures name their material entry by a 32-bit hash (index, first row and row count read as one word). Match it to `MapMaterial` (textures by `texture::name_hash`; constants are kept by name, so their hash has to be found).
  - **Vertex constants:**
    - b0 per primitive (code 320) is the world matrix; the lprobe program adds the eye-relative translation to its rows' w.
    - b1 per object (code 304) is the view-projection.
    - Code 19 in b2's last register is the eye; the lprobe program subtracts it from the world translation.
    - All of this is to confirm from the lmap vertex program before relying on it.

### Stage 2, steps 2 to 6: the world drawn by its own techniques (2026-11-03)
- **Where:** `render_iw7::world` holds `Host` (the map prepared once, each frame records), `render` (offscreen) and the display pass. `iw7_world_shot <map.ff> <out.png> [w h] [x y z yaw pitch]` renders one frame.
- **Pipelines:**
  - Each surface draws its set's first `lmap_*` technique pass.
  - Both programs are translated by `dxbc_sm5` (the vertex outputs' interpolation is taken from the pixel program).
  - The vertex layout comes from `world_attribute`, over the raw 44-byte stream.
  - Front faces are clockwise and depth is reversed (1 near, 0 far, compare greater).
  - On riot: 7725 of 13559 surfaces are drawn, by 73 pipelines.
- **Constants, from the shader maths** (`techsets_mp_riot`, `lmap_g0[0]_c0_atest0...`):
  - Code 320 (vertex b0) is the world matrix: three rows with the translation in w. The vertex program subtracts the eye from it.
  - Code 19 (vertex b2 register 17) is the eye.
  - Code 304 (vertex b1) is the view-projection about the eye, as rows.
  - Code 8 is the direction toward the sun (w turns the sun on). Code 9 is the sun's colour.
  - Code 109 is (w, h, 1/w, 1/h): pixel times zw gives the screen uv of the screen maps, and times xy the light-cluster tile.
- **Left at zero** (`world::UNPROVEN`; the host logs each with its pass count):
  - 192: the sun visibility's softness;
  - 210 to 214: fog (zero leaves the colour as it is);
  - 144, 145, 147: volumetric fog;
  - 14, 17, 134, 141, 146, 151 to 159, 174 to 196, 308.
- **Light maps:**
  - t2 is `*lightmapN_primary` (BC4, the sun's visibility in red), t3 is `*lightmapN_secondary` (BC6H, two pages stacked: v times 0.5, and plus 0.5), and t4 is `*lightmapN_secondunorm` (RG8). They are uploaded as the zone holds them; riot's are inline, so no pak is needed.
  - The other code textures are white, except 51 (volumetric inscatter), which is black.
  - The cluster light buffers (t24, t25, t35, t37) are zero, so no local lights are drawn.
- **Lighting only:** material pictures are white and material constants are one.
- **Not drawn yet:**
  - **Layered surfaces:** 746 surfaces want TEXCOORD3 and 95 want COLOR1, which come from the layer streams. This includes riot's road.
  - **Failed translation:** 46 vertex programs fail with `NoPosition`.
  - **No lit technique:** 873 surfaces.
  - **Not read:** static models, sky, and IW7's tone map. The display pass uses exposure, Reinhard and sRGB; `BO2MP_IW7_EXPOSURE` defaults to 0.18, and the offline shot sets its own exposure.
- **In game:** on by default since later the same day (`render_gpu::drawsurf::iw7_native`, log lines `iw7 native:`).
  - The map load names the zone.
  - The map, its techsets and the host are made on a thread while the converted world keeps drawing (no loading hold).
  - Each frame then hands the image and the reversed depth into the frame with `render_t7::present`, and the converted world, props and sky stay out.
  - Never seen on a GPU: it needs the owner's screenshot.
- **Next:**
  - the layer streams (the road);
  - the material pictures and constants, matched by the arguments' 32-bit hash;
  - static models;
  - the sky;
  - the `NoPosition` vertex programs.

### Stage 2 continued: the whole map, native-only (2026-11-03)
- **Technique:** every surface and placed model draws its set's slot 8 (`render_iw7::world::DRAW_SLOT`): `lmap_*` on the lit world, `lprobe_*` on models, the unlit and sky sets' forward pass.
  - A tessellated `*_worldtess_*` technique gives way to its `*_world_*` twin, because its vertex program writes no position.
  - Distortion is skipped, because it reads the frame behind it.
  - Tools and shadow casters have no slot 8, so they don't draw.
  - Shared techniques (a set pointing at an earlier set's technique) are now resolved: `Capture::techniques_at`. They were the "no lit technique" surfaces.
- **Vertex streams** (`render_iw7::vertex`): `MaterialVertexDeclaration` routing is now captured (`Capture::vertex_decls`, `PassRec::vertex_decl`).
  - **Destinations:** 0 POSITION, 1 NORMAL, 2 COLOR0, 3 COLOR1, 8+n TEXCOORDn.
  - **Base sources (0-4):** the 44-byte world vertex or the 32-byte model vertex.
  - **World layer stream (sources 6 and up):** `Iw7Map::layer_data` and `WorldSurface::layer_offset`, laid out per world vertex format as MWR's is. This matches all 28 riot formats' strides (8 to 60 bytes).
  - **Model lighting stream (sources 24-26):** `StaticModel::vertex_lighting`.
  - **Integer inputs:** read as integers.
  - **Unrouted inputs:** read zeros (stride-0 stream).
- **Materials:**
  - **Textures:** a type-5 argument's 32-bit hash is index | first_row << 16 | row_count << 24, and it matches `MapMaterial::textures` (15544 of 15544 on riot). Pictures are uploaded at most `BO2MP_IW7_TEXTURE_MAX` (default 1024). Missing pictures are white, and missing normal maps are flat.
  - **Constants:** b3 holds the material's own `MaterialConstantBufferDef` vs/ps data for slot 8 (`MapMaterial::cbuffers`, `cb_index`).
  - **Sampler states:** from the texture def, read as MWR's.
- **Draw state** (`render_iw7::state`): `GfxStateBits` by `stateBitsEntry[8]` is read as MWR's load bits, except that the blend word is word 2.
  - Word 1 bit 0 is depth write, and bits 4-5 are the polygon offset.
  - The depth test bits are not taken, so every pass tests depth (greater-or-equal, reversed).
  - Opaque surfaces draw first, then blended ones by sort key.
- **Models:** the placed model's world matrix goes in code 320.
  - The vertex program reads the light grid probes (`Iw7Map::light_probes`, 64 bytes each) from raw buffer t20.
  - Code 250 = (0, first probe, loop count 1 as an integer, probe count 1), and code 22 = fade with w 0.9999 (full weight). Each model is lit by its probe nearest `lighting_origin`.
- **Sky:**
  - Code 328 is the sky's view-projection: only its rows' xyz meet the eye-relative position, which puts the sky at the far plane.
  - 205, the sky colour scale, is taken as one.
  - 217-219, the sky rotation, are taken as identity.
- **Programs:** they are read from the map's techsets zone, then by name from the shared technique zones beside it (`world::shared_technique_zones`: `techsets_global_core_mp`, `common_core_mp`, `common_mp`, `global_mp`, `common`, `global`; the `_cp` sets for Zombies). Technique sets are looked up in all of those zones.
- **Results** (riot, lavapipe, view culled):
  - Spawn view: 4182 surfaces drawn; 10 don't draw (5 pipelines whose programs aren't in the staged zones).
  - Wide view: 11516 surfaces drawn, by 2023 pipelines; 86 don't draw.
  - Pictures and most models need the install's `imagefile*.pak` / `assetfile*.pak`, which weren't in the cloud (only 7 models were read), so the owner's GPU run is the first with textures and props.
- **In game (on by default since the owner's go-ahead, 2026-11-03; `BO2MP_IW7_NATIVE=0` for the converted world):**
  - The lane builds no converted surfaces, materials, pictures, light map pages, placed models or sky (`load_iw7_world_for`). It still builds collision, spawns, entities, scripts, sounds and the light for players and the gun.
  - The loading screen is held until the first native frame (240 s at most).
  - `BO2MP_IW7_CONVERTED_WORLD=1` builds the converted world anyway.
- **Offline:** `iw7_world_shot` draws in distance bands (`IW7_SHOT_BAND`, `IW7_SHOT_RANGE`), since lavapipe holds about 600 pipelines in 6 GB.
- **Fog (later the same day):** codes 210-214 come from the map's vision (`world::Fog`, from the lit programs' fog maths: transmittance `max(exp(d * c212.w + c210.w), c210.y)`, the colour a mix of fog and sun fog by view angle). `BO2MP_IW7_FOG=0` turns it off. The colours' HDR scale against the sun's is not proven; on riot the fog reads heavy.
- **Done since (2026-11-03, evening; each read from the lit programs' own maths, with a switch to turn it off):**
  - **Tone map and exposure** (`post`): metered auto exposure (log luminance down to 1x1, adapting at `ADAPT_RATE`), the vision's black/white/crossover/shoulder/toe curve, sRGB, then the map's 32³ colour table. `BO2MP_IW7_EXPOSURE` scales the exposure.
  - **Sun shadows and SSAO** (`screen`): a depth pre-pass of the opaque surfaces and placed models, two sun cascades (1024 and 6144 units, snapped to texels), a 3x3 PCF mask on code texture 10 and half-size hemisphere AO on code texture 43. `BO2MP_IW7_SHADOWS=0`, `BO2MP_IW7_SSAO=0`.
  - **Local lights** (`lights`): the map's spot and omni primary lights binned per frame into the programs' clusters (8-pixel tiles, 32-unit depth slices; t24 records, t35 tile bits, t37 slice ranges, code 147). Positions are about the eye. `dxbc_sm5` gained `Options::d3d_position_w`, since WGSL's position w is 1/w. `BO2MP_IW7_LOCAL_LIGHTS=0`.
  - **Spot shadows** (`spot`): the spots the map lets cast shadows (32 on riot) get a 512² layer of code texture 15, drawn once from the shared occluders as linear depth over reach, and a t25 record (four rows about the eye, then the exponential test's `k, -1, 1 + e^(k bias), 1/reach`). `BO2MP_IW7_SPOT_SHADOWS=0`.
  - **Volumetric fog** (`volumetric`): a 160x90x64 froxel march (code textures 51 in-scatter, 52 transmittance) from the vision's `volumetric*` keys, lit through the sun cascades with a Henyey-Greenstein phase; codes 144/145 are four depth segments (256, 768, 2048, 5120). The density's unit (per 1000 units) is not proven. `BO2MP_IW7_VOLUMETRIC=0`.
  - **Distortion** (`behind`): surfaces reading code texture 18 (`distortion_*`) draw last, after a copy of the scene; codes 14 (time), 160 (uv clamp), 265/266 (clip to uv).
  - **Reflections** (`reflect`): the probe cube array (code texture 37) with the programs' own probe walk: t31 records (box, fade, parallax planes, layer), t32 grid cells, t33 per-probe L1 luminance measured from the cubes on the GPU. The map gives only origins, so each box is a cube sized by its nearest neighbour. Code 17 is the eye; codes 141 (blend by gloss, here always 1) and 151 (luminance match clamp, here 0 to 1) are not proven. `BO2MP_IW7_REFLECTIONS=0`. On lavapipe without the material pictures every surface reads as a mirror (white gloss); needs a look on the PC.
- **Specular environment lookup (code texture 64, `brdf`, 2026-11-03 evening):** the lit programs read a 64x64 table at `(n . v, smoothness)`, take `spec = f0 * x + y` and scale the diffuse light by `1 - max(spec)`. It was bound white, so every surface was a full mirror with no diffuse light: black shadows and blown-out sunlight in game (the owner's first video of Retaliation). Now the split-sum GGX table (Karis), made at load. The game's own table is made by its code (not read); code texture 65 is the specular occlusion lookup (`spec_occlusion`, see below).
- **Not done:** the game's own post-fx programs (`code_post_gfx` stops at its DDL asset), bloom, depth of field, motion blur, decals' exact order, spot shafts in the volumetric fog (`volumetricSpotBrightness` is 0 on riot).

## Performance (2026-11-03)

Measured with the offline shot (`IW7_SHOT_PROFILE=<frames>`, `--release`) on riot at fixed cameras, on the cloud box's software device (lavapipe, 2 cores): CPU times are the render thread's encode time; GPU and wall times there are a software rasteriser's and say little about a real card. In game, `BO2MP_IW7_PROFILE=1` prints the same line every 120 frames, with GPU times from timestamp queries where the adapter has them, and the first frame logs one `iw7 fps:` line (draws after culling, batches, pipelines, occluders).

| Camera | Draws (after culling) | CPU before | CPU after | Constants before → after |
|---|---|---|---|---|
| Spawn (`-592 3072 276`, yaw 270), cone build | 4189 → 3820 | 61.6 ms | 6.2 ms | 52.9 → 0.4 ms |
| Street (`-700 -300 120`, yaw -150, pitch 5), cone build | 2407 → 1998 | 32.4 ms | 3.5 ms | 26.6 → 0.2 ms |
| Street, everything within 1500 units (`IW7_SHOT_CONE=0`, as in game) | 4184 → 1268 | 68.2 ms | 7.6 ms | 59.0 → 0.6 ms |

Commits, biggest cost first:
1. **Profile** (`profile`): CPU per phase, counts, bytes uploaded, GPU timestamps read back without waiting.
2. **Constants in one arena:** every camera buffer is an offset in a few 1 MB uniform chunks with a CPU mirror; each code constant is worked out once a frame and each chunk uploaded with one write (was one `write_buffer` and several small allocations per buffer).
3. **Frustum culling** (`cull`, `BO2MP_IW7_CULL=0` off): draws and the depth pre-pass by the camera, each sun cascade's casters by its own box.
4. **Batching** (`BO2MP_IW7_BATCH=0` off): opaque draws sorted by pipeline, bind group and stream. On riot the switches were already one per distinct pipeline and bind group, so this guarantees it rather than cutting it.
5. **No allocation in the frame path:** light binning, spot records and the screen and volumetric uniforms reuse buffers; bind groups that depend on a view are made when it changes.

Already so before this pass: every pipeline is made at load (no first-sight hitches); spot shadows are drawn once; on IW7 maps no converted world is built and Black Ops II's grade and bloom stay out (`native_world`).

### Graphics settings (`native_quality`, the shared hook)

`Host::apply_quality` takes `native_quality::current()` when the host is made and again whenever `native_quality::revision()` moves (`render_gpu::drawsurf::iw7_native`), and logs one `iw7 quality:` line; the first frame logs `iw7 fps:` with the preset. ULTRA is the renderer's full quality, as before this pass.

| value | `render_iw7` | when |
|---|---|---|
| `sun_shadow` | the sun cascades and their mask (`BO2MP_IW7_SHADOWS=0` still wins); the volumetric fog then leaves the sun unshadowed | next frame |
| `sun_shadow_res` | each cascade's side, twice the shared value (ULTRA and HIGH 2048, MEDIUM and LOW 1024); the cascades and the mask's and fog's groups are made again | next frame |
| `spot_shadows` | 0: no spot shadows; more: every spot the map lets cast one (32 on riot), as at full quality | next frame |
| `spot_shadow_res` | 512 at every preset, as ours | |
| `local_lights` | the clustered spot and omni lights (off: the baked light only) | next frame |
| `ambient_occlusion` | the SSAO pass (`BO2MP_IW7_SSAO=0` still wins) | next frame |
| `draw_distance` | scale on placed models' own cull distances | next frame |
| `max_texture` | the pictures' size cap (`BO2MP_IW7_TEXTURE_MAX` wins) | next map |
| `anisotropy` | cap on the material samplers' anisotropy | next map |
| `lod_distance` | not applied: one LOD per model is drawn | |
| `motion_blur` (Graphics > MOTION BLUR; off at LOW and MEDIUM) | camera motion blur (`blur`): each pixel reprojected through the last frame's view, a 1/120 s shutter whatever the frame rate, at most 4% of the width, 10 samples, before the tone map. The game's own blur programs (`code_post_gfx`) are not read, so the shutter and cap are ours. `BO2MP_IW7_MOTION_BLUR=0` | next frame |
| `bloom`, `depth_of_field` | not drawn by `render_iw7` yet | |
| `render_scale`, `vsync`, `max_fps` | the view's size carries the scale; presentation owns the rest | |

Not scaled (the map's look): fog, volumetric fog, the tone map, exposure and the colour table.

Frame cost per preset, street view, cone build (lavapipe; CPU is the render thread's encode time):

| preset | CPU | occluders drawn (pre-pass, near, far cascade) | wall (software device) |
|---|---|---|---|
| ULTRA | 3.9 ms | 1880, 1166, 2268 | 1696 ms |
| HIGH | 4.0 ms | 1880, 1166, 2268 | 1897 ms |
| MEDIUM | 3.7 ms | 1880, 1166, 2268 (cascades 1024) | 1592 ms |
| LOW | 2.9 ms | none (no sun shadows, no AO) | 1453 ms |

ULTRA's image matches the shot before the settings were wired (only time-driven pixels differ).

Not done yet: instancing of repeated placed models; merging the shadow casters' draws; GPU numbers on a real card.

## The game's post programs, read (2026-11-03, night)

`code_post_gfx.ff`'s walk still stops at its DDL assets, but its 847 shader programs can be found by scanning for their DXBC containers; each is named by the source name stored before it (`iw7_post_programs`). Among them: `postfx_tonemap_*_macroclut` (the final display), the glare chain (`glare_brightpass1/2`, `glare_blur_x/y`, `glare_expand`), `dof_*`, `postfx_mblur*`, `cs_bcao` and `cs_mdao_*` (ambient occlusion), `cs_sunshadow*`, `cs_exponential_shadow_map_*` (spot shadows), `volume_light_scatter_*`, `cs_reflection_probe_*`, `cs_macroclut`. Their constants still need the techsets (behind the DDL) or the engine to know where each comes from.

Read so far:
- **Display** (`postfx_tonemap_veil_filmgrain_macroclut`): scene mixed with its veil (`veilStrength`), the piecewise rational curve per channel (two coefficient sets chosen by the crossover), a 3x3 colour matrix (identity on the visions read), `1.055 x^(1/2.4) - 0.055` clamped at 0, the 32^3 colour table at texel centres, then film grain (`tonemapGrainStrength`, the shader's own hash and weighting). `render_iw7::post` now does the same; the veil's source is our wide blur (luminance-weighted so lamps don't blot).
- **Glare** (bloom): bright pass = 4-tap average, luminance smoothstep between two thresholds, raised to a power, times an amount; blur = 5 taps 0.4921875 / 0.234375 / 0.01953125. The visions read set `r_glare_*Amount` to 0, so the maps use no glare.
- **Vision values now used:** `veilStrength`, `tonemapGrainStrength`, `tonemapAdaptSpeed` (a share each 1/60 s), `r_ssaoStrength` (mp_fallen's 0: no SSAO there).
- **Specular occlusion (code texture 65):** read at `(sqrt(1 - v), 2 / (2^(20 gloss) + 2), cos(bent axis, reflection))` and scaling the reflection; now the share of a Phong lobe's cone (where it falls to a tenth) inside the visibility cone, counted over evenly spread directions (`spec_occlusion`). White before: no reflection was ever hidden.
- **Exposure curve:** with `tonemapAutoExposureAdjustCurve` the automatic exposure is moved by the scene's EV along the vision's dark/mid/light points (`tonemapDarkEv`... `tonemapLightExposureAdjust`), clamped to `tonemapMin/MaxExposureAdjust` (`post::ExposureCurve`). The scene's EV is taken as the fixed exposure that would give the automatic one (`log2(EXPOSURE_K metered / AUTO_KEY)`), which is ours. Riot's street view comes out about half a stop brighter.

## Placed models, technique sets and alpha-tested shadows (2026-11-03, late)

From the owner's log (`mp_geneva`: 8423 "model surfaces with no forward technique", 554 of pipelines not made) and the zones in the cloud:
- **Technique sets kept by name.** 169 (geneva), 178 (fallen) and 173 (riot) of the sets the map's own technique set zone names are empty there; the whole set is in a later zone (the common technique sets). The first copy was kept, so every surface using one had "no forward technique". The whole copy now wins (`world::has_passes`).
- **Effect-style sets** (`mo_`, `mco_`, `mopw_`, `eq_`...: unlit glows, screens, holograms on placed models) fill only slots 0 to 4, no slot 8. They are drawn with their first colour technique (slot 0 or 1, not a depth-only, shadow, thermal or debug one), with that slot's state bits and constant buffer (`world::effect_pass`; `BO2MP_IW7_EFFECT_SETS=0`: not drawn). World `w_` sets (shadow casters, tools) never take this. Without slot 8 now: 24 sets on geneva (was 381), all depth-only or empty.
- **Lightmapped placed models** (`ml_`/`mlc_` sets; the vehicles and the distant city on fallen). Their vertex program makes the light map uv as `c117.xy * TEXCOORD4 + c117.zw`, TEXCOORD4 routed from source 13, and the pixel program reads the light map slots (2 to 4) there. Source 13 is the surface's `lmapUnwrap` (two floats a vertex, walked but never read), code constant 117 the placement's `GfxStaticModelLightmapInfo` scale and offset, and the light map its index. None of the three was fed: every such model read light map 0 at uv (0,0). Now: model vertices are 40 bytes here (`GfxPackedVertex` then the unwrap uv, `vertex::MODEL_STRIDE`), 117 is fed from the placement, and the group binds its light map (`BO2MP_IW7_MODEL_LIGHTMAPS=0`: as before). Checked on fallen's nine cloud-loadable models: unwraps in 0..1, places like offset (0.27, 0.09) scale (0.045, 0.09); the distant buildings now take their baked light.
- **Alpha-tested shadows.** Alpha-tested surfaces (technique or set `_atest`, or a pixel program that discards) now go into the depth pre-pass, sun cascades and spot maps through their alpha: the `_packed_a` picture's red where the material has one, else the colour map's alpha (not a `_packed_cs` colour map's), discarded below a half (`screen::CUTOUT`, `Screen::add_cutout`; `BO2MP_IW7_CUTOUT_SHADOWS=0`: none). Sets marked `_nocast` (fallen's tree canopy and treeline cards) and decals (polygon offset) cast nothing. Checked by `screen::tests` (a quad half-transparent casts only on its opaque half); the real pictures are not in the cloud.
- **The black road patches** were very likely the decals (`wc_l_sm_atest_*`: crosswalk paint, asphalt cracks) drawn as solid quads into the sun cascades, shadowing the road under each quad. They left the depth passes with the canopy fix and are now kept out by their polygon offset; not yet seen on the owner's screen.
- **Follow-up (2026-11-04): the owner's map went black** after this change. Most likely one pixel that was not finite (an infinity or NaN) from a newly drawn surface: the exposure metering averaged it in, the exposure went to 0, and the adaptation kept it for good. Post now zeroes such pixels before metering, the veil and the display, and the adaptation refuses a value that is not finite (`post::FULL_VS` `safe3`). The effect-style sets are off by default (`BO2MP_IW7_EFFECT_SETS=1`), and cutouts skip pictures that can't be sampled filtered.
