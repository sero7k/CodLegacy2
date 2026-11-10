# Ghosts (IW6), natively: Ghosts' own renderer on Ghosts maps (`render_iw6`)

Since 2026-11-03 a Ghosts multiplayer map is drawn by Ghosts' own renderer and only by it, as Modern
Warfare Remastered's maps are by `render_h1` (`docs/H1_NATIVE.md`), Black Ops III's by `render_t7` and
MWII's by `render_jup`. Black Ops II's converted world, props, sky and grade are never built, loaded or
drawn on a Ghosts map. Players, the view gun and effects keep drawing through the existing character and
effects passes into the native frame's depth, as on MWR maps.

Switch: `BO2MP_IW6_NATIVE` (default on; `=0` returns to the converted world under Black Ops II's renderer,
for comparison). `BO2MP_IW6_CONVERTED_WORLD=1` builds the converted world anyway (it is then not drawn).
`BO2MP_IW6_MAX_TEX=n` caps the pictures (default 1024 in game, 2048 offline).

Everything below was checked headless on lavapipe (Mesa's software Vulkan) with the uploaded zones
(`mp_frag.ff`, `common_core_mp.ff`, the four `techsets_*.ff`, `code_post_gfx_mp.ff`) and five small image
paks staged from the owner's PC (`imagefile25/40/43/49/58.pak`). Nothing has been seen on a GPU in game.

## Where it is

| piece | what it does |
|---|---|
| `render_iw6::scene` | reads the map and the zones it draws on (`Scene::zones_for`: the map, `techsets_<map>`, `patch_<map>`, `techsets_code_post_gfx_mp`, `techsets_common_core_mp`, `techsets_common_mp`, `techsets_common`, then `fastfile_iw6::shared::SHARED_ZONES`); materials (a name the map only references is taken from the zone that holds it), the world mesh (raw `GfxWorldVertex`, 44 bytes) and its vertex layer stream, the placed models' first LODs (`GfxPackedVertex`, 32 bytes), draws per surface list (lit opaque, decal, translucent, emissive, sky, models), each placed model's light grid table, its baked vertex light and its ground light, lightmaps, probes, sun, fog, tone |
| `render_iw6::pool` | techniques, technique sets (slot → technique by the walk's own record), programs by name across zones, each set's world vertex format |
| `render_iw6::bind` | a pass's argument table: rates by the pass's counts (per primitive, per object, stable), `cb0..cb3`, one block shared by both stages; argument types 0..6; the engine's own pixel slots (`t1` probe, `t2` lightmap primary, `t3` lightmap secondary) |
| `render_iw6::state` | `GfxStateBits` (28 bytes) as read from mp_frag's materials: word 0 cull (bits 3..4), word 1 depth (IW layout: write, no-test, test 2..3, polygon offset 4..5), word 2 blend and colour write mask (24..27) |
| `render_iw6::world` | the renderer: pipelines (translated by `dxbc_sm5`), the constant arena and per-object blocks of `render_h1::world`, the depth pre-pass, technique picking, the no-displacement fallback |
| `render_iw6::layers` | the world's vertex layer stream: layouts from the vertex declarations' names (`pp.fcc0.3t0t0.fnn.ftt2.ft1t3.fx0t5.fw0td.f`) |
| `render_iw6::model_lighting` | `MODEL_LIGHTING`: each table as the shell of a 4 x 4 x 4 block of one 3-D texture |
| `render_iw6::post` | Ghosts' own post techniques run full screen; the luminance reduction and the iris step; the identity `MACROCLUT` |
| `render_iw6::host` | the frame: opaque, `FLOATZ`, fog, blended, iris, final |
| `render_iw6::sun_shadow` | the sun shadow map's two cascades in one atlas, the lookup, partition, fade scale and tap spacing the `ld1` shaders read |
| `render_iw6::look` | vision file, createart fog block (the map's HDR override block), tone mapping asset |
| `render_gpu::drawsurf::iw6_native` | the game hook (copied from `h1_native`): reads the scene on a thread, draws it first in the main pass into the scene depth band, holds the loading screen until the first native frame, logs one `iw6 native:` coverage line per map |
| `asset_world::{set_,}iw6_native_zones`, `assets::lane::iw6_map::{native_zones, load_iw6_world_for}` | the zones named as the map loads; the converted world's surfaces, materials, pictures, sky and placed model meshes not built (collision, spawns, entities, scripts, sounds, compass kept) |
| `examples/iw6_shot.rs` | `cargo run -p render_iw6 --release --example iw6_shot -- <map.ff> [--size WxH] [--cam x y z pitch yaw]... [--out pic.png]`: headless to PNG, with the coverage line and the HDR scene's luminance |
| `examples/iw6_probe.rs` | lists a map's technique sets, slots, state bits, pictures and paks, shader inputs and arguments (`PROBE_SET=`, `PROBE_TECH=`, `PROBE_RAW=1`) |

## How a frame is drawn (and what each part is read from)

0. **Sun shadow map** (`BO2MP_IW6_SUN_SHADOW=0` turns it off): the casters' `build_shadowmap_*`
   technique (slot 1) into two cascades of one 2R x 2R atlas (R 1024, `BO2MP_IW6_SHADOW_RES`). The
   `ld1` shaders' maths fixes the layout up to a choice (one fade scale and offset for a cascade's u and
   v, the same u offset for both cascades): each cascade a square quarter, the near one top-left, the far
   one under it. READ: the near cascade 0.25 units a texel (`sm_sunSampleSizeNear`; `BO2MP_IW6_SHADOW_DVARS=0`
   returns to the earlier inferred 0.5). INFERRED: centred on the eye, the far one 4 times as large (READ 2026-11-05: the far partition's texel is 4 times the near's; it was inferred as 8, `BO2MP_IW6_SHADOW_DVARS=0` returns to 8, `BO2MP_IW6_SHADOW_FAR_RATIO=1..16` overrides), depth
   from the sun over the world's bounds, bias about 2 units (`BO2MP_IW6_SHADOW_BIAS`, constant 4096, slope 3).
   The engine's own caster offset (`sm_polygonOffsetBias` 128, `sm_polygonOffsetScale` 2) is for its depth
   range: here it gave acne rings on mp_frag's snow (checked by eye), so it is used for the spot maps only.
   Not resolved: the exe's `SUN_SHADOWMAP_PIXEL_ADJUST` `(0.25/res, 0.5/(n res), 0.5/res, -0.25/(n res))`
   (`IW6_LIGHTING.md` 3.3) is anisotropic for any 2:1 atlas, and the `ld1` shaders use only its xy (taps at
   `uv + PIX.xy * O_i`, read), while their fade needs square cascades; the tap spacing stays one atlas texel.
   The split and the far cascade's size are not dvars (`sm_*` holds no such value).
1. **Depth pre-pass**: every opaque surface's `zprepass` technique (slot 0). Read from the state bits: the
   lit passes after it test less-equal without writing depth, so without it the picture is whatever was
   drawn last. A surface whose lit pass does not build gets no pre-pass (its depth would hide what is
   behind it).
2. **Opaque world and models**, each with its own technique: slot 9 (`..._ld1`: the sun over the sun
   shadow map, the baked visibility past it; slot 8 `ld0`, the baked visibility only, with the shadow map
   off) when the surface's primary light environment holds the sun (`ComPrimaryLightEnv`, light type 1),
   else slot 7 (`ln0`: the baked pages only; also an unlit set's technique). A technique
   with a hull shader (displacement) takes its form without (slot + 309: `no_tess_world_disp`), with
   neutral `TESSELLATION_PARMS` (INFERRED, as `render_h1`). The sky first, under everything.
   - World: lightmap secondary (two stacked RGBA16F pages) and primary (BC4 sun visibility) bound to
     `t3`/`t2`, the probe cube to `t1` by the surface's probe index.
      - Layered world surfaces (`vcl`/`vcw`, the generated `*100_132` blends): their second stream from the
     world's `vld` data at the surface's `vertexLayerData` offset. Each world vertex format's stride is READ
     from the stream (a surface's run to the next one over its vertex count; every run of a format agrees);
     the components' order and formats are inferred from the declarations (texture sets as two or four
     floats, `x`/`w`/`y` sources four bytes, in source order). On mp_frag 13 of 47 formats are longer than
     what any declaration reads (e.g. format 18, `..._z0z1z2`: 36 bytes, 32 read): the unread components
     are taken to follow the read ones (INFERRED), the read ones keep their offsets. With the declarations'
     stride those surfaces read every vertex but the first misaligned: their floats came out as garbage,
     the cause of the non-finite pixels (5,574 in the view `--cam -193 1650 500 30 90`, white blotches over
     the gravel; 0 now). `BO2MP_IW6_LAYER_STRIDE=0` returns to the declarations' stride.
   - Models: `m_`/`mc_` sets read `MODEL_LIGHTING` (the light grid's SH evaluated in the 56 lattice
     directions at the model's lighting origin, `fastfile_iw6::lightgrid`); `mv_` sets read the
     placement's baked vertex light (`GfxStaticModelVertexLighting`, 20 bytes a vertex: `TEXCOORD4`
     visibility, `TEXCOORD5` ambient, `TEXCOORD6` highlight); `m_ambient_` sets read `LIGHT_PROBE_AMBIENT`,
     which is the placement's `groundLighting` (non-zero on 1,707 of mp_frag's 8,779 placements, mostly these). Placements with zero ground
     light are the vertex-lit and grid-lit ones.
2a. **Models' sun visibility past the shadow map** (`BO2MP_IW6_GRID_SUN=0`: 1 everywhere, as before): the
   `MODEL_LIGHTING` volume's alpha, which the model `ld0`/`ld1` shaders read as their baked visibility (past
   the far cascade, or everywhere with the sun shadow off). The light grid's SH holds no sun; each cell has a
   primary light environment (`fastfile_iw6::lightgrid::LightGrid::environment`: that of the corner with data
   that weighs most, as `IW6_LIGHTGRID.md` 9 reads the engine). Alpha 1 when that environment holds the sun,
   0 when it holds other lights only. A cell with environment 0 or no data leaves it to the placement's own
   environment (INFERRED: on mp_frag 1,440 placements that draw with the sun sit in environment-0 cells;
   with the cell's 0 they would all go dark). The engine's flag-requested trace toward the light is not
   reproduced. mp_frag: 2,805 placements sunlit by their cell, 1,250 not, 4,724 by their own environment.
2b. **Spot and omni lights** (`BO2MP_IW6_LOCAL_LIGHTS=0` turns them off): a surface or model whose
   primary light environment (`ComPrimaryLightEnv`) has no sun and a spot (type 2) or omni (type 3) light
   draws with slot 11 (`ls1`, the shadowed spot technique; slot 10 `ls0` with `BO2MP_IW6_SPOT_SHADOW=0`)
   or 13 (`lo0`), its constants per light (`local_light`, as `render_h1::local_light`: eye-relative
   position with `1/radius`, colour times the sun's scales, the direction toward the light, spot factors
   `1/(cosInner - cosOuter)`, `-cosOuter/(...)`, exponent, shadow fade). Each light gets its own plan of
   the per-object constants. mp_frag: 57 primary lights (35 spot, 20 omni), 792 environments with one as
   primary; 1,808 draws take `ls1`, 1,137 `lo0`.
   - **Falloff** (`BO2MP_IW6_LIGHT_FALLOFF=0`: the old inferred `1 - d/r`): the shaders look the falloff up
     at `saturate(d / r) * LIGHT_FALLOFF_PLACEMENT.xy + .zw` in the lightmap secondary and square it. READ
     from mp_frag's pages: the light compiler writes each light def's attenuation picture into row 0 of the
     upper page at the def's `lmapLookupStart` (`GfxLightDef`, now captured with its pictures,
     `fastfile_iw6::walk::LightDefRec`; each primary light's `defName` is kept in `ComWorldRec`), its edge
     texels repeated on both sides: defs at texels 1, 35, 69 and 103, 32 texels each, falling from 0.99 to
     0.02 (row 1 on is ordinary lightmap; page 1 holds the same row). So the placement is
     `(width / W, 0, lmapLookupStart / W, 0)` with `W` the page width (1024): IW4's form, whose pages were
     512 wide. The ramp's length is the def's attenuation picture's width, 32 when the picture is not in the
     zones (the 34-texel spacing). All 55 spot/omni lights have their def on mp_frag; if any lacked one the
     whole map falls back to the inferred ramp (`iw6 light falloff:` note).
   - **Spot shadows** (`BO2MP_IW6_SPOT_SHADOW=0` turns them off; `BO2MP_IW6_SPOT_SHADOW_RES`,
     `BO2MP_IW6_SPOT_SHADOW_BIAS=c,s`): `ls1`'s maths READ from its translation (`spot_shadow`): four rows of
     `TRANSPOSE_SHADOW_LOOKUP_MATRIX` (projective), four compare taps at `+-w PIX.xy` and `+-w PIX.zw`
     (`SPOT_SHADOWMAP_PIXEL_ADJUST`) divided by w and averaged, then `vis = baked + LIGHT_SPOTFACTORS.w *
     (shadow - baked)` with `baked` the lightmap primary (as `ls0` reads it). So the spot factors' w is the
     shadow's fade: every spot light draws `ls1`; the lights with a map this frame get 1, the rest 0 (exactly
     `ls0`). Up to `sm_spotLimit` 4 lights, each a tile of one 2R x 2R atlas (R 512), drawn with the
     casters' `build_shadowmap_*` technique and the engine's shadow raster offset (`sm_polygonOffsetBias`
     128, `sm_polygonOffsetScale` 2, read from the dvars). INFERRED: which lights (the nearest to a point 64
     units ahead of the eye, measured to an eighth of the radius along the light, the points the
     `sm_lightScore_*` dvars name; the score is not read), R, the near plane (radius / 64, at least 4),
     the tap spacing (a quarter and a half texel on a rotated grid, after the sun's), the cone widened a
     little so edge taps stay in the tile. Omni lights have no shadow technique in use (`sm_spot*` only).
     The four tiles are four views of one caster list, each with its own constants, all in the frame's
     one encoder and submission (see "Performance").
3. **`FLOATZ`**: the reversed depth copied to R32F; `ZNEAR` set so Ghosts' fog reads distance
   `ZNEAR * 63/64 / floatz` in our depth band.
4. **Fog**: Ghosts' `postopaquefx_dfog` (sun fog on) or `postopaquefx_fog`; `FOG_SUN_DIR` in view space for
   this pass, world space for the sky shader.
5. **Blended surfaces** over the fogged scene (a blended surface whose colour map is missing is left out).
5b. **Material bloom** (`BO2MP_IW6_BLOOM=0` turns it off): Ghosts' `material_bloom_setup_alpha_intensity`
   (the scene weighted by its alpha, the bloom mask the lit shaders write as `bloomParams.x * vertex
   alpha`, through a soft threshold `1 + GLOW_SETUP.x / max(L, -GLOW_SETUP.x)` with `GLOW_SETUP.x = -
   r_materialBloomLuminanceCutoff`, desaturated by `GLOW_SETUP.z = r_materialBloomDesaturation`), into a
   half-size target; `material_bloom_hq_blur` along x then y (seven taps of 1/7 at +-1.46 times
   `BLOOM_HQ_RADIUS`, `r_materialBloomRadius` texels); `material_bloom_apply` (`bloom * GLOW_APPLY.w +
   GLOW_APPLY.x`, w `r_materialBloomIntensity`) added over the scene. The maths is the shaders' (read from
   their translation); the target size, the two blur passes and the additive blend are INFERRED. mp_frag
   has `r_materialBloomHQEnable 0`, so the HQ pyramid (`material_bloom_hq_setup`,
   `material_bloom_apply_bloomhq`) is not used; the `glow_*` passes (`r_glow`) are not run.
6. **Iris**: the mean Rec. 709 luminance of the scene (an arithmetic mean, as `avg_luminance_*`): the first
   level sums every scene pixel of its block exactly (`textureLoad`, sums and pixel counts in Rg32Float),
   then 2 x 2 sums of sums and counts down to one texel, so every pixel counts once whatever the sizes and
   the result is the same every run; a pixel that is not finite is left out. Then the iris step
   (`key / mean`, linear rates, `[min, max]`) into the one-texel `DYNAMIC_IRIS_EXPOSURE` (R16F,
   ping-pong). Offline the first step is long (settles), and the clock (`GAMETIME`) is held at 0 so
   animated materials draw the same every run. (Before 2026-11-03, third round, the first level took one
   bilinear tap per block: a sample, not the mean; view 0's exposure was 1.64 for a mean that gives 1.91.)
7. **Final**: Ghosts' `postfx_macroclut`: exposure, Hable's curve, the colour table, sRGB, black level,
   written into the display. The curve's constants as the shader combines them (read from its
   translation: numerator `LINEAR_ANGLE * (B x) + A x^2 + TOE_NUMERATOR`): `D E`, `D F`, `E/F`, `B`,
   `C`, `A`, white scale `1/f(W)` (`host::tone_consts`, unit-tested against `look::Tone::curve`).

### Exposure fix (2026-11-03, second round)

The first shots were washed out (snow, sky) and a near wall of the interior view drew fully white. Two
causes, both found by reading HDR values at pixels (`IW6_SHOT_PIXEL="x,y;..."` on `iw6_shot`) and the
final pass's own inputs (`IW6_DBG_PSOUT`):
- `TONE_MAPPING_LINEAR_ANGLE` was given `C B`; the shader multiplies it by `B x` itself, so the curve's
  linear part was B (3 on mp_frag) times too strong: a scene value of 0.27 at exposure 2.2 displayed 0.75
  instead of 0.5. Now `C`.
- A specular map that is not loaded was bound mid grey: under the materials' fresnel ramps (up to 10 at
  grazing angles) every untextured surface mirrored its probe. Now black (no reflection).
The iris itself was right (mean luminance against `key / mean` in [0.4, 2.2]).

## Props, brush models and movers (2026-11-03)

The static models were the only props `render_iw6` drew. Now every prop the map shows as it starts is drawn
by its own material's technique, as a placed model (model mesh, the light grid's table at its origin, the
nearest static placement's probe and light environment within 256 units, else probe 0 in the sun: INFERRED,
the engine lights them at run time as H1's dynamic entities) or, for brush models, as world surfaces at the
entity's pose (the world's techniques read `TRANSPOSE_WORLD_VIEW_PROJECTION_MATRIX0`, so the brush model's
own vertices, about its origin, land where the entity stands; their lightmaps move with them).

| what | read from | switch (default) |
|---|---|---|
| clip map dynamic entities with a model (kinds 1 clutter, 2 destructible, 4 a scriptable's body, 5 its loose part) | `fastfile_iw6::map::extra_props` (`DynEntityDef` pose quaternion and origin) | `BO2MP_IW6_DYNENTS` (on) |
| dynamic entities built of brushes (glass, shelves) | `map::placed_brush_models` | `BO2MP_IW6_DYNENTS` (on) |
| scriptables (base model), `script_model` entities | `map::extra_props` (game objects, exploders, `light_off` twins left out as before) | `BO2MP_IW6_SCRIPT_PROPS` (on) |
| `script_brushmodel` entities with surfaces (doors, the hopper, pop-up targets) | `map::placed_brush_models` | `BO2MP_IW6_SCRIPT_PROPS` (on) |

A scriptable whose body is a dynamic entity of the same model where it stands (mp_frag's four fluorescent
lights and three fire barrels) is drawn once, as the entity (it was drawn twice by the converted lane too).

**Movers.** Every placed `script_brushmodel` (by its `*N`) and every `script_model` a door switch moves (by
model and starting origin) is a `scene::Mover`; `Host::set_mover_pose(key, origin, angles)` moves its objects
(`Renderer::set_object_world`), and their draws are never view-culled. In game `iw6_native` applies the
poses the sim publishes (`asset_world::iw6_mover_poses`, written by `sim::t6::iw6_doors`). Offline:
`IW6_SHOT_OPEN=1` puts every door piece at its far end, `IW6_SHOT_MOVERS=1` prints each mover's bounds.

mp_frag: `iw6 props: dynent 257, scriptable 29, script_model 29, script brush models 15, dynent brush models
16; movers 20` (each line also in the game's log as `iw6 native: mp_frag: iw6 props: ...`). The 16 brush
dynents and the 15 script brush models were never drawn natively before. Shots (scratchpad of the session
that made them): `door_open0_1.png` / `door_open1_1.png` (the west door closed, then open: the two
telescoping panels slid into the pocket), `door_open1_0.png` (the north door open, the bottles and cans on
the floor behind it are dynamic entities).

## Map scripts (2026-11-04)

`sim::t6::iw6_script` runs the map's real compiled GSC (zones common, common_mp, common_core_mp, their patches, the map, its patch; a later script of a name replaces an earlier one) in `gsc_iw6_vm`. Supply: `fastfile_iw6::scripts` (cached per zone in the temp dir; walking common.ff is the slow part), handed over as `asset_world::Iw6ScriptPack` together with the map's raw numeric-key entity blocks and the exploder placements (their effects converted by the assets lane). `BO2MP_IW6_SCRIPTS=0` turns it off.

- Tick: `pre` (before `movers::advance`) delivers presses, damage, players, timers, steps the VM and applies the host's `Effect`s; `post` sees finished tracks and sends `movedone`/`rotatedone`. VM time is the sim's level ms.
- Events in: use and touch triggers (`triggers.rs` fire list), damage (`damage_triggers_but` tap; `damage` with amount, attacker, direction, point, MOD, weapon), `connected`, `prematch_over`. `game_ended` (when the game script sets the match's `game_ended` flag, once).
- Out: movers via `zm.movers`, hide/show/delete/solid/candamage/usable/hint/hidepart on the sim `Ent`, sounds, loop sounds, effects, exploders (`activateclientexploder(id)` plays that id's placements after their delay, turned by the placement's forward AND up: the createfx rule, roll counts), poses to the renderer.
- Script-spawned entities (2026-11-04, events pass): `spawn(class, origin, flags, radius, height)` makes a host entity and a sim `Ent` with a T6 object (so the damage tap, `triggers::touches` and the use/touch dispatch see it). Scripts then `setmodel`/`moveto`/`rotateto`/`linkto`/`hide`/`delete` it like a map entity (a deleted script entity leaves the sim). `trigger_radius` / `trigger_multiple` / `trigger_once` spawned by a script say `trigger` with the player when he steps in (INFERRED: once per entry, the engine's wait time is not read). Spawned models are drawn when the map zone holds the model (see "Spawned and swapped models" below).
- Damage on script entities: the engine's rules (`iw6_script::events`, READ from the damage function): an entity takes damage only after `setcandamage(true)` (damage triggers always), `health -= max(1, damage)`, `damage` is notified with (amount, attacker, direction, point, MOD, model, tag, part, flags, weapon), and `death` (attacker) once health is under 1 (clamped at -999). Trigger damage gets only the `damage` notify. A script's `radiusdamage(origin, range, max, min, [attacker], [MOD], [weapon])` hurts host entities by the engine's linear fall-off measured to the entity box (skips the inflictor, entities with `setcanradiusdamage(false)`; the engine's line-of-sight trace is not made) and the players through the game's player damage. Model boxes for the damage tap come from the map zone's models (`Iw6ScriptPack::models` -> `Ent::bounds` -> `brushes::model_box`).
- Named in this pass, READ from the handlers: 0x81d0 `radiusdamage`, 0x82b5 `setcandamage`, 0x82b6 `setcanradiusdamage`, 0x8018/0x8019 `attach`, 0x8026/0x8027 `detach`, 0x8028 `moveshieldmodel`, 0x833d `setscriptmoverkillcam`, 0x8402 `getlinkedchildren`, 0x8045/0x8048 `hidepart`/`showpart` (set/clear the tag's bit in the part mask), 0x82a7 `movegravity` (trajectory type 5; the sim moves the entity straight to where the arc ends), 0x82b7 `physicslaunchserver` (no-op). 0x80d7 stays a no-op, and 0x80a6/0x8176/0x81ae/0x80c2 are read but not named (see `exe_notes/IW6_SCRIPT_CENSUS.md`).
- Effects (2026-11-04): the assets lane scans the scripts reachable from the map's `main` (and every function of the map's own scripts) for `fx/...` and `vfx/...` string literals (a destructible's definition hands them to `loadfx` through variables, so only the literals can be read) and converts every one found in the map, patch or common zones through the same path as the createfx effects (`Iw6ScriptPack::script_fx`, path -> catalog name). `playfx`, `playfxontag`, `triggerfx` (an effect entity from `spawnfx`), `playloopedfx`, `stopfxontag`, `killfxontag` and `exploder` then play by the id `loadfx` returned. Bounds: at most 24 plays a tick, a bolted effect plays at most once per 150 ms per (entity, effect, tag) (the toys' smoke script repeats every 400 ms), 256 bolted effects and 32 looped ones are remembered, `stopfxontag`/`killfxontag`/`delete` stop what was bolted (the stop event, 0x80, at the spot it played). A bolted effect plays at the tag's world position when it is played (the model's tag table from the zone; a tag it does not have stands at the origin): it does not follow the entity between plays. An effect in no zone read plays nothing and is counted in the load line (`iw6 script fx: N of M`).
- Unimplemented builtins: the VM counts them, pushes `undefined` and goes on; the sim names each one once in the log (`iw6 scripts: builtin ... is not implemented`) and prints the table with the tick statistics. `cargo run -p gsc_iw6_host --example iw6_script_census -- <zone dir> <map> [--md]` prints, for any map zone beside its common zones, every builtin id its scripts call (all of them, and those reachable from the map's `main`) with the host's status (implemented / approximate / no-op / UNIMPLEMENTED), the effect paths the scripts name, and what a 10-second boot hit that the host turned away (mp_frag: nothing). Generated table for mp_frag: `docs/exe_notes/IW6_SCRIPT_CENSUS.md`.
- Headless tests: the host sources are compiled twice from the same files, in `sim` (`cargo test -p sim --lib iw6_script`) and in `gsc_iw6_host` (`cargo test -p gsc_iw6_host`, seconds to build, no sim link). They boot every `mp_*` zone staged beside the common zones (only mp_frag here), run mp_frag's door switch, gate, warehouse door, ladder and underground sprinklers, and break a toy transformer with real damage events (`events::deliver_damage`): the script goes through the smoke, the sparks, the model swap to `..._dest`, the explosion effect and sound, the blast (`radiusdamage`), `setcandamage(false)` and deletes its helper entities, with no builtin turned away.
- `scriptmodelplayanim` / `useanimtree` play the animation (see "Script animations, doors and builtins" below).
- INFERRED: typed parsing of map keys, builtin 0x175 (explosive MOD flag; byte table read from the exe), the level fields the engine sets before the map script (`level.players`, console flag, bots).

### Spawned and swapped models (2026-11-04)

The renderer's draw lists are built when the scene loads, so a model a script spawns, or swaps in over a map model (`setmodel`: the toys' `..._dest` states), is drawn by a hidden instance made at load: the assets lane lists the map-zone models whose names appear in the reachable scripts (`Iw6ScriptPack::pool_models`), `render_iw6::scene` makes `BO2MP_IW6_SPAWN_POOL` (default 4, 0 off) zero-size instances of each (`MoverKey::Spawned { name, slot }`), and the sim publishes `Iw6MoverPose { model, slot, origin, angles, hidden }` for them; the map's own entity is published hidden. Limits: only models the map zone itself holds (a common-zone model is "left out", as for the static props); at most 4 live entities per model (more are not drawn); an attached model (`attach`) is kept on the sim entity but not drawn; the instance's light table is a flat placeholder (0.3), not the light grid at its place. Needs the owner's GPU to look at: nothing here has been seen on a screen.

## Other maps (2026-11-04)

The host is map-generic: it takes the scripts of the zones found beside the map (`common`, `common_mp`, `common_core_mp`, their patches, `<map>`, `patch_<map>`), the map's entity blocks and string tables, and starts `maps/mp/<map>::main`; no map name appears in the host, the lane or the renderer pool. Unknown builtins do not abort a map. What the install shows about the other Ghosts maps without their zones: the common zones carry one thing per map, the bot callout triggers (`trigger_multiple_bcs_<map>_<place>`) of mp_battery, behemoth, boneyard, chasm, conflict, dart, dig, dome, farenheit, favela, flooded, frag, hashima, impact, lonestar, mine, pirate, prisonbreak, red, rumble, shipment, skeleton, snow, sovereign, strikezone, swamp, warhawk, zebra, zerosub and zulu (the launcher's maps, counted from the dumps in `scratchpad/dis`). The map events themselves (Flooded's water, Tremor's quake, Stonehaven's, ...) live in each map's own zone script, which is not staged, so what they need cannot be read here; nothing is claimed for them. What any such script needs is exactly what the census lists for it: run `iw6_script_census` on the zone as soon as it is staged, implement the UNIMPLEMENTED rows that the "hit" line names (those are the ones the map's events call), and start with the movers, damage, spawn and effects paths above, which every event of mp_frag used.

## Map logic (2026-11-03)

`fastfile_iw6::maplogic::MapLogic` reads what the map's own data says (one `iw6 map logic:` line per map):

- **Door switches** (READ from the entities): a `trigger_use_touch` named `door_switch` targets a name; each
  `script_brushmodel` / `script_model` of that name that targets a `script_struct` goes to the struct's origin
  (and angles). `script_parameters` (numbered key 27766, named by its values) gives the sounds. mp_frag: 5
  groups, 15 pieces (`n_door`, `e_door`, `w_door`, `se_door`: two telescoping panels each; `hopper_and_door`:
  the hopper's brush, the vat, its four wheels and the hopper door).
- **Exploders** (READ from `maps/createfx/<map>_fx.gsc`, the createfx source, a raw file in the map zone):
  id, effect, origin, angles, delay, sounds. mp_frag: exploder 1, eight `vfx_sprinkler_system_frag`.
- **Not driven by the data** (counted): mp_frag 5 animated script models (the crane, the iron gate, its
  chain, the warehouse doors' lock), 4 destructible toys (transformers), 5 pop-up target brushes, 22 door
  switch lights, 13 door blockers (`prone_only`, `not_opening` triggers), 6 damage triggers of the map
  script (the gate's and warehouse's chains, the ladder, the underground sprinklers).

`sim::t6::iw6_doors` (on since 2026-11-04, `BO2MP_IW6_DOORS=0` off; timing INFERRED) makes the switches work: a press sends
every piece of the group to its far end (the next press back, a press while moving turns it round), the
collision follows (`brushes::sync`, the script movers' presence), the switch's `button_sound` and the
pieces' `start_sound` / `stop_sound` play, and the poses go to the renderer. INFERRED: 1.5 s a move
(`BO2MP_IW6_DOOR_SECS`), eased over a fifth at each end; nothing blocks a door. Ghosts' common door script
(`_movers`, the switch) is in common zones not staged here, the map's own script (`maps/mp/mp_frag`) is
bytecode: who fires exploder 1 (by the strings next to it, the underground damage triggers and the
`sprinkler_type_*` spray sounds), the gate, warehouse, ladder and hopper animations, the lights and blockers
are not translated.

## Coverage on mp_frag (headless, 2026-11-03)

`iw6 native: mp_frag: techniques made 620 (failed 0), draws 17017 of 46012 drawn (3627 skipped), slots
{7: 11551, 9: 7910, 10: 2, 11: 1808, 13: 1137, 316: 905, 318: 491, 319: 1, 320: 48, 322: 100}, pictures 148
loaded / 1012 missing (0 at a smaller size), post all Ghosts' own` (the draws count each pass, pre-pass
included; drawn is after view culling; the intermission view, 2026-11-03 third round).

- 17,798 world surfaces and 9,782 placed model surfaces (8,779 placements, 1,030 vertex-lit, 8,771 with a
  light grid table, 8 outside the grid).
- Skipped: 2,922 blended surfaces whose colour map is in a pak that is not there; 332 + 64
  `w_unlit_add_lin_bl_blmod_z1` (no lit/unlit slot: a set drawn by another pass), 292 shadow-caster-only
  sets (`*_shadowcaster_atest`, correct: they draw only into shadow maps), 12 effect sets.
- Pictures: 1,994 of mp_frag's 2,055 are streamed; the 33 inline ones are the lightmaps and probes. With the
  five staged paks 148 load; the others (paks 4, 7, 10, 13, 59 and the 3.4 GB pak 60, over the 400 MB
  staging limit) draw as Ghosts' material fallbacks (mid grey, alpha 0; normal maps flat).
- Sky: see "Sky audit" below.

- **Colour grading.** Two parts, as the engine splits it (`IW6_LIGHTING.md` 5):
  - *Colorization*, in HDR, in the post-opaque fog pass. The vision's `colorizationSet` (mp_frag:
    `mp_frag_colorset`, now captured by `fastfile_iw6` with its `Colorization` assets) picks Ghosts' own
    `{fog}_color_altd` technique (`_color` without an alternate). It sets
    `COLORIZATION_POWER_AND_DESATURATION{,_ALTERNATE}`, `_SCALE`, `_BIAS` and `_DEPTH_SCALE_AND_BIAS`. The
    translated shader does desaturate over luma (0.299, 0.587, 0.114), then `pow(c, power) * scale + bias`,
    then near/far by `dist * x + y`. `BO2MP_IW6_COLORIZATION=0` uses the plain fog technique.
  - *The final pass's colour table* (`MACROCLUT`, 16^3, since 2026-11-03 fourth round): the colorization
    set's low / mid / high grade, as the exe bakes it (READ, `IW6_SHADERS.md` 14; `look::Lmh`,
    `post::macroclut_lmh`): per entry each band desaturated, raised to its power, scaled and biased,
    blended by smoothstep weights on the luma from the set's end points, 8-bit. The vision's film
    (`r_film*`) is not in that bake and no technique reads it: it is not applied. Before, the film was
    baked into the table with MWR's curve (INFERRED); on mp_frag its dark tint (1.22, 1.32, 1.51) lifted
    and blued every shadow (the owner's "hazy, low contrast"). `BO2MP_IW6_LMH=0` (or a set without the
    three grades) returns to the film table, `BO2MP_IW6_FILM=0` then the identity. Intermission view,
    1280x720: mean 148 to 131, 5th percentile 47 to 36, the blue cast gone (`look/inter_film.png`,
    `look/inter_lmh.png`, `look/inter_cmp.png`).
  - `BLACK_LEVEL` is (0, 1). It is `r_blacklevel`'s default of 0, and the vision sets no black level, so
    this is the real value and not a placeholder.

PNGs (scratchpad of the session that made them): `mp_frag_native_0.png` (the intermission view),
`_1` (the yard behind it), `_2` (an interior whose pictures are in the staged paks).

Third round (2026-11-03), `iw6_shot` on lavapipe, 1280x720 (scratchpad
`/tmp/claude-0/-home-claude/84914088-0fb0-5515-8694-265294ae0942/scratchpad/r2/`). Cameras: view 0
`--cam -540 632 356 0 33`, view 1 `--cam -540 632 356 10 213`, view 2 `--cam 900 -300 150 0 160`, the spot
light floor `--cam 830 156 250 25 180`, the layered gravel `--cam -193 1650 500 30 90`.
- Spot/omni: `l1cmp_0.png` (lights off / inferred ramp / read ramp / read ramp with shadows),
  `sm_spotmap_0.png` (the spot shadow atlas). On mp_frag the spot lights' direct term is small next to the
  baked pages (the lightmap primary's spot visibility is 0 on many interior surfaces).
- Grid sun: `g2crop4.png` (a pole under a canopy loses its sun highlight; sun shadow off, where the models
  read the alpha everywhere).
- Non-finite pixels: `nan_cmp.png` (before / after on the gravel); `d3a_*.png` and `d3b_*.png` (two runs,
  byte-identical).
- Sun shadow: `s4cmp_*.png` (0.5 / 0.25 near texel), `s4crop.png` (0.5; 0.25 with the engine's offset: acne;
  0.25 with ours).
## Effects in the native frame (2026-11-03)

Players, the gun and effects draw after `render_iw6`'s final pass, into the display picture. A Ghosts
effect's colour is HDR (the sample's colour times its unlit HDR scale, `render_fx::iw6_rules`); it was
scaled down to 0..1 (`display_rgb`), so effects ignored the map's exposure, curve and film. Now, on a Ghosts
map drawn natively, each Ghosts sprite's colour goes through the frame's own grade on the CPU
(`fx_iw4::iw6::grade::Grade::apply`): times the iris exposure, Hable's curve per channel times its white
scale (the map's `ToneMapping`), sRGB, then the film baked into `MACROCLUT` (the vision's `r_film*`).
The exposure is the GPU's: `Host` copies the one-texel `DYNAMIC_IRIS_EXPOSURE` to a buffer and maps it the
frame after (`ExposureReadback`, one in flight, so a few frames late; before the first read, the iris's start
`(min + max) / 2`). `iw6_native` publishes `Host::fx_grade` every frame (`asset_world::set_iw6_fx_grade`);
`BO2MP_IW6_FX_GRADE=0` brings back the scale-down. One `iw6 fx grade:` line per map gives the exposure and
whether it was read back.

INFERRED: the grade is per sprite, then the sprite blends in display space; Ghosts blends its effects in HDR
before the tone map, so overlapping additive sprites differ (brighter cores roll off sooner here). The lit
types (3, 4) keep the light the effect pass samples (`BO2MP_IW6_FX_LIT`, off), not Ghosts' light grid.
Effects of Black Ops II's own guns and maps are not graded (their colours are display colours already).
Not seen on a GPU (`cargo test -p render_iw6 --lib grade` checks the CPU grade against `Tone::curve` and
`Film::apply`).

## Not done / falls back (in order of what to do next)

- **Props**: dynamic entities do not move or break (no physics, no health), scriptables stay in their
  first state, animated script models stand in their bind pose.
- **Sky picture**: read from pak 60 in game (not seen: the cloud has no pak 60); only its top level.
- **Layer stream layout**: inferred; checked only by every layered technique building and drawing.
- **Debug switches**: `IW6_DBG_PSOUT=<word>|<wgsl vec4>` (pixel programs whose name holds the word write
  the expression), `IW6_DBG_SKIPVS=<word>`, `IW6_DBG_CULL=none`, `IW6_DBG_NOCULL=1`, `IW6_DBG_SLOT=<n>`,
  `IW6_DUMP_WGSL=<dir>` with `IW6_DUMP_MATCH=<word>`.

## Post effects: SSAO, depth of field, motion blur, sun blind and glare (2026-11-05, driven)

Exe findings and addresses: `docs/exe_notes/IW6_POST.md`. All four are gated by the shared graphics options and
each has an env switch; none is touched by the REAL-TIME LIGHTING option. Nothing here has been seen on a real
GPU: checked headless (lavapipe, `iw6_shot` variants) on mp_frag only.

| effect | module | passes | gate | switch |
|---|---|---|---|---|
| SSAO | `render_iw6::ssao` | `ssao_calc_fast` (mode 2 `_slow`) over `FLOATZ` + `RANDOM_ROTATIONS` into R8, `ssao_vblur_lq`, then `postopaquefx_ssaolq_<fog>` (the fog pass we already run, with the AO) | AMBIENT OCCLUSION row (off on MEDIUM and LOW); strength from the vision (0 draws nothing); half size on HIGH (INFERRED), render size on ULTRA | `BO2MP_IW6_SSAO=0`, `BO2MP_IW6_SSAO_MODE=2` |
| depth of field | `render_iw6::dof` | `dof_downsample` (quarter size), blur, `dof_near_coc`, `postfx_dof` into the spare scene target | DEPTH OF FIELD row (off on LOW); draws only while a DOF is set (`Host::set_dof`, fed from the game's `DofFrame`: scripts and the sights) | `BO2MP_IW6_DOF=0` |
| motion blur | `render_iw6::mblur` | Ghosts' velocity-buffer chain (velocity from FLOATZ + last camera, tile max x then y, 3 x 3 neighbour max, `postfx_mblurhq`); `render_h1`'s reprojection blur with `old` | MOTION BLUR row (HIGH and ULTRA); nothing drawn while the view is still | `BO2MP_IW6_MOTION_BLUR=0`, `BO2MP_IW6_MOTION_BLUR_MODE=old` |
| sun blind / glare | `render_iw6::sun_fx` | a one-pixel visibility pass over FLOATZ (read back late), then the bloom's apply pass with `GLOW_APPLY` x = lighten, y = darken (also when bloom is off) | not on LOW; only when the map's `sunflare_t` has blind or glare values | `BO2MP_IW6_SUN_FX=0`, `BO2MP_IW6_SUN_FROM_DVARS=1` (the `r_sun*` dvar defaults instead of the level's) |

Order in `record_post`: sun state, DOF (reads the fogged target, writes the spare `scene` target, which the rest then
reads), motion blur, bloom (+ the sun's apply), iris, final pass. No allocation per frame (the targets and groups are
made on first use and on a resize); an effect that is off costs nothing (the byte-compare against the effect off
below is identical). Checked on mp_frag, 1280 x 720 / 640 x 360 lavapipe: SSAO darkens creases (pillars, tanks,
wall corners; HIGH half size and ULTRA full size both draw); DOF with near 0..400 and far 900..2500 blurs the far
alley and the nearest rail and keeps the middle sharp; with the sun in view the sun state darkens and lightens
(visibility held with `IW6_SHOT_SUN_VIS`), a view away from the sun is byte-identical to the effect off. mp_frag's own
level data holds no blind or glare (max darken 0), so there it is `BO2MP_IW6_SUN_FROM_DVARS=1` that shows it.
Not done: the sun flare sprite (its material picture is not in the staged paks), the engine's velocity-buffer motion
blur and its hq DOF, Ghosts' exact DOF gaussian (ours is the bloom's seven-tap blur), players and the gun as sun
shadow casters (`render_gpu` draws them, they do not write into the iw6 atlas). Found on the way, not from this
work: applying LOW after HIGH inside one process (the `iw6_shot` variants) hits a bind-group offset overrun in
`iw6_cb_inst` (`Host::apply_quality` list remake); the game's option change takes the same path.

## SSAO, depth of field, motion blur (inventory, 2026-11-03; now driven, see above)

All three are full-screen **pixel** techniques in `techsets_code_post_gfx_mp.ff` (and
`patch_code_post_gfx_mp.ff` for the motion blur's depth-masked forms); none is a compute shader, so
`dxbc_sm5` can translate them. They are not driven because their inputs are made by the exe and are not
read (wiring them would be inferred throughout, so it would ship off by default with nothing checked):
- **SSAO**: its six constants, the blur steps and the 8 x 8 `RANDOM_ROTATIONS` picture are now READ from
  the exe (`IW6_SHADERS.md` 13); still not drawn: its shaders read `FLOATZ` as 1 / distance, ours is
  scaled by the near plane and the depth band (see there). The inventory below stands.
- **SSAO** (`r_ssao` default mode 1, the vision's `r_ssaoStrength` 0.35 and `r_ssaoPower` 15.99 on
  mp_frag): `ssao_calc_fast`/`_slow` (reads `FLOATZ` and the engine's `RANDOM_ROTATIONS` picture, which is
  in no zone), `ssao_vblur_lq`/`_hq` (bilateral blur of `SSAO` over `FLOATZ`), then the post-opaque pass's
  `postopaquefx_ssao{lq,hq}_{fog,dfog}_color[_altd]` forms (the fog and colorization pass we run, plus the
  AO). Six constants come from the exe (`SSAO_FALLOFF_DEPTH_SCALE`, `SSAO_UV_TO_ROT_SCALE_AND_FADE`,
  `SSAO_SAMPLE_MAT_SCALE`, `SSAO_DEPTH_OFFSET_REJECT`, `SSAO_POWER_BB_SHARPNESS_STEP`, `SSAO_COLOR_COEFF`).
  Read from `ssao_calc_fast`'s translation so far: it works on `FLOATZ` as 1/depth, rejects pixels past
  `SSAO_DEPTH_OFFSET_REJECT.w`, fades by `depth * SSAO_UV_TO_ROT_SCALE_AND_FADE.z`, reads its rotation at
  `uv * SSAO_UV_TO_ROT_SCALE_AND_FADE.xy`, takes neighbour depths at `+-SSAO_DEPTH_OFFSET_REJECT.xz/.zy`,
  and picks the falloff by a depth threshold between `SSAO_FALLOFF_DEPTH_SCALE.z` and `.w`. The dvars that
  feed them (`r_ssaoWidth` 36 px at 720p, `DepthScale` 0.1, `FadeDepth`/`RejectDepth` 2500, `GapFalloff`
  0.25, `GradientFalloff` 0.05, `BlurSharpness` 50, `BlurStep` 1, `Diminish` 1) are known; how the exe
  combines them is not.
- **Depth of field**: `dof_near_coc`, `dof_downsample`, `dof_hq_*` (mask, CoC blur mips, blur, final),
  `postfx_dof*`. The vision has no DOF keys (the 57-key table holds none): in multiplayer DOF is set by
  aiming (the weapon's ADS values) and the `r_dof_*` dvars; the frame at rest has none to draw.
- **Motion blur**: `velocity_static`/`_vm` and the models' `*_velocity_*` techniques write
  `SCENE_VELOCITY`; `postfx_mblurhq` (+ `_velocity_pass0/1`, `_fast_{low,med,high}[_depthmask]`) reads it
  with `MOTION_BLUR_HQ_PARAMS` and `MOTION_BLUR_HQ_DECODE_SCALE_BIAS` (exe-made) and the previous frame's
  matrices. `r_mbEnable` is an option (enum); an offline still frame has no motion to blur.

## Lumber stacks and the gravel's lines (owner's second screenshot, 2026-11-03)

Reproduced offline at mp_frag's lumber yard (`--cam -40 2180 214 3 90`; shots `look/owner2.png` before,
`look/owner2_noshadow.png` with `BO2MP_IW6_SUN_SHADOW=0`, `look/owner2_fps4.png` after, scratchpad of the
session that made them).
- **Black stripes on the stacks: sun shadow acne of the stacks on themselves.** The plank stacks
  (`mv/mp_frag_woodstack_0*`, vertex-lit) carry static model flag 0x10 and the map places simple
  `*_shadowcaster_*` boxes with them to cast their shadow; the stacks' thin planks drawn into the cascades
  shadowed each other in stripes and zigzags. Fixed by frame cost round 4 (the map's own caster rules,
  merged first): the stripes are gone in the same view. Checked on the way, and right: the stacks'
  baked vertex light (`GfxStaticModelVertexLighting`) lines up with the vertices at every LOD (each
  surface's run rounded to an even count: the triangles' light spread is smallest there, 0.0089 against
  0.0125 one vertex off), and its light follows the faces (up faces about 0.5, sides 0.1 to 0.25, down
  0.08); the shader reads ambient `TEXCOORD5` plus the directional `TEXCOORD6` by the flat weight
  `sqrt(1 - a.w^2 - h.w^2)`. The visibility bytes (`TEXCOORD4`) are 0 on every vertex of the stacks.
  Offline the boxes cast nothing (their alpha-tested pictures are in paks not staged: alpha 0 discards
  every texel), so the stacks' shadow on the ground needs the owner's screenshot.
- **Lines on the gravel: cracks between displaced ground surfaces.** The yard's ground draws Ghosts'
  displacement techniques without tessellation (`*_no_tess_world_disp_*`, masked magenta with
  `IW6_DBG_PSOUT='disp|vec4<f32>(1.0,0.0,1.0,0.0)'`); their vertex programs move each vertex along its
  normal by `w0 * (height * k.x + k.y)` with `k` the material's displacement constants, a weight from
  the layer stream and a mip from distance (translation read). The lines are where neighbouring
  surfaces' edges moved apart, showing what is behind (light grey offline, the sky's stand-in; black in
  game). READ from the exe (`IW6_SHADERS.md` 13): `TESSELLATION_PARMS` = (60, 1, -1/256, 1) and
  `TESSELLATION_PARMS2` = (3, 0, 0, 0.0625) with tessellation off; with the height scale negative the
  shaders divide the height by the vertex's texture stretch `|position.w|` (mp_frag: 1/256 typical,
  down to 6e-5). Wired as `host::tessellation_consts`, **off by default** (`BO2MP_IW6_TESS_PARMS=1`):
  offline the displacement pictures are missing (drawn as the mid grey fallback), and with the read
  values parts of the yard by the stacks came apart (`look/owner2_tess.png`), so it cannot be judged
  here. Not resolved: the layer stream's `w0` offset in the longer formats is inferred
  (`layers::with_strides`). Needs: a screenshot of the same spot with `BO2MP_IW6_TESS_PARMS=1`.

## Players, the gun and effects in the native frame (graded by Ghosts' post, 2026-11-03)

The owner's first match on mp_frag (RX 9070 XT, ULTRA) showed the gun milky white. Two causes:
- **Grade**: the native frame wrote its tone-mapped picture first, then Black Ops II's passes drew players,
  the gun and effects over it in display values (the fallback pass's `t7_out`: Black Ops III's curve at the
  converted look's exposure, then its colour table). Neither Ghosts' exposure, Hable curve nor film
  reached them.
- **Light level**: their grid light was Ghosts' table times the converted world's look scale (`iw6_map`,
  `exposure * BO2MP_IW6_GRID_SCALE`), units of the converted look, not Ghosts'.

Now (`BO2MP_IW6_CHAR_GRADE`, on; `=0` the old order) a frame on a Ghosts map is drawn in stages:
1. `Host::record_scene` (in `iw6_native`, before the exact colour pass): shadow maps, the opaque world
   and models into Ghosts' HDR scene target.
2. The exact colour pass draws its opaque draws into that target (`colour_submit::record`), the fallback
   pass (`geometry_diagnostic`: the gun, arms, players, grenades, marks) its opaque draws too.
3. `Host::record_fog` (run by whichever pass reaches its blended draws first, `iw6_native::char_grade_mid`):
   `FLOATZ` from the depth with the characters in it, Ghosts' fog and colorization, the blended world.
4. Effects and the view model pass go into the fogged target.
5. `Host::record_post` (`draw_iw6_post`, after `FallbackDrawSet`): bloom, iris, `postfx_macroclut`.
The characters are in the iris's mean, as in Ghosts.

The fallback pass's pipelines for such a frame carry `IW6_HDR` (key `iw6_hdr`, target Ghosts' scene
format): lit models are lit as Ghosts' model shaders light theirs (`iw6_lit`: albedo times the grid
light plus the sun by N.L times its visibility; with a specular map the squared specular colour times
the fresnel ramp times the probe's reflection and the sun's Phong lobe on R, `IW6_SHADERS.md` 5 and 7)
in Ghosts' units: the grid light times `1 / look scale` (light row 251 v1.w, written by the lane:
`iw6_map::native_hdr_row`), so the grid is read at Ghosts' own level (`BO2MP_IW6_GRID_SCALE` still
multiplies), the sun is Ghosts' (the ComWorld sun times `r_diffuseColorScale` 1 / `r_specularColorScale`
2.5). No fog, curve or grade of their own; an opaque draw writes 0 into the scene's alpha (the bloom
mask). Unlit surfaces and effects, authored as display values, go in as `display^2 / E` with `E` the
exposure the map's iris settles at (INFERRED). INFERRED too: the fresnel ramp (0.2, 1.0, 2.5) (Black Ops
II's guns have no `envMapParms`), the grid's light as its mean over the 56 directions (the moving
things' grid keeps no direction). The 56-value volume itself is not read for them: `LIGHTING_LOOKUP_SCALE`
only places the lookup on the cube shell, so the mean of the same table at the same scale is the level
Ghosts' model shader gives a model facing every way.

Offline check (`iw6_shot`, `IW6_SHOT_PLACEHOLDER=old|graded`, `IW6_SHOT_PLACEHOLDER_AT=60,24,18`, view
`--cam -40 2180 214 3 90`, the lumber yard of the owner's second screenshot): a grey box (texel 0.5) lit
by the grid's mean at the box (2.25 / 2.23 / 1.99 there) and the sun, drawn over the finished picture
(old: the front face clips to white) and into the HDR scene (graded: the same light comes out light grey,
the top darker, as the world's props beside it). The tool draws no Black Ops II model, so the gun itself
needs the owner's screenshot. Shots: scratchpad of the session that made them, `look/gun_old.png`,
`look/gun_graded.png`, `look/gun_cmp.png`.

## Performance

### Graphics settings on the native renderers (shared hook, 2026-11-03)

Every native renderer (`render_iw6`, `render_h1`, `render_t7`, `render_jup`, `render_iw7`) reads the game's
graphics settings through one crate, `native_quality` (no dependencies; `frame::settings::native_quality`
re-exports it):

- **Preset**: Graphics > GRAPHICS QUALITY (`LOW`, `MEDIUM`, `HIGH`, `ULTRA`; default `ULTRA`), saved as
  `graphics_quality=` in the settings file. `BO2MP_QUALITY=low|medium|high|ultra` overrides it (offline
  tools have no settings and read this; with neither, `ULTRA`).
- **Option rows over it**: a row can only switch its part off or lower it (`native_quality::Quality::new`).
- **Publishing**: `console::user_settings` calls `GameSettings::publish_native_quality()` whenever the
  settings change; a renderer's hook (in `render_gpu::drawsurf::*_native`) reads `native_quality::current()`
  when `native_quality::revision()` differs from the revision it last applied.
- **Reload**: `Quality::needs_reload(old, new)` is true for the fields read as the scene is made (picture
  size cap, anisotropy); a renderer applies the rest on the next frame and says in its log line which
  changes wait for the next map.

| value (`Quality`) | LOW | MEDIUM | HIGH | ULTRA | option rows that lower it | when |
|---|---|---|---|---|---|---|
| `sun_shadow` | off | on | on | on | SHADOWS off | next frame |
| `sun_shadow_res` (one cascade's side) | 512 | 512 | 1024 | 1024 | | next frame |
| `spot_shadows` (lights with a map) | 0 | 0 | 2 | 4 | SHADOWS off, REAL-TIME LIGHTING off: 0 | next frame |
| `spot_shadow_res` (one tile's side) | 512 | 512 | 512 | 512 | | next frame |
| `local_lights` (spot / omni direct light) | off | on | on | on | REAL-TIME LIGHTING off | next frame |
| `max_texture` (largest picture side) | 512 | 1024 | 1024 | 2048 | TEXTURE QUALITY performance halves it; the file's `tex_quality` 3/2/1 caps 512/1024/2048 | map reload |
| `anisotropy` | 1x | 4x | 8x | 16x | the file's `tex_filter` 0/1 caps 1x/4x | map reload |
| `bloom` | off | on | on | on | BLOOM off | next frame |
| `depth_of_field` | off | on | on | on | DEPTH OF FIELD off | next frame |
| `ambient_occlusion` | off | off | on | on | | next frame |
| `motion_blur` | off | off | on | on | MOTION BLUR off | next frame |
| `draw_distance` (scale on models' cull distances) | 0.5 | 0.75 | 1 | 1 | | next frame |
| `lod_distance` (scale on where a model's next LOD starts) | 0.5 | 0.75 | 1 | 1 | | next frame |
| `render_scale` | from UPSCALING / RENDER QUALITY (the view's size already carries it) | | | | | next frame |
| `vsync`, `max_fps` | from V-SYNC and the frame cap (the presentation owns them; recorded) | | | | | |

Not settings (they are the map's look, not cost): fog, colorization, film, tone mapping and the iris stay as
the map sets them on every preset.

How `render_iw6` applies it (`Host::apply_quality`, read at load and whenever the revision moves):

| value | `render_iw6` | when |
|---|---|---|
| `sun_shadow`, `sun_shadow_res` | the sun atlas on/off (`ld1` / `ld0` techniques) and R (atlas 2R); `BO2MP_IW6_SUN_SHADOW=0`, `BO2MP_IW6_SHADOW_RES` still win | next frame (atlas re-made, lists made again; pipelines kept) |
| `spot_shadows`, `spot_shadow_res` | how many of the chosen spot lights get a tile (0: no atlas, spot lights draw `ls0`), tile size | next frame |
| `local_lights` | spot / omni lights with `ls*` / `lo0`, else `ln0` (baked only) | next frame (lists made again) |
| `max_texture` | the pictures' size cap (`BO2MP_IW6_MAX_TEX` wins) | next map |
| `anisotropy` | cap on the material samplers' anisotropy | next frame (groups made again) |
| `bloom` | Ghosts' material bloom passes | next frame |
| `draw_distance`, `lod_distance` | scale on the placements' LOD bands and cull distances | next frame |
| `depth_of_field`, `ambient_occlusion`, `motion_blur` | not drawn by `render_iw6` yet (see below) | |
| `render_scale` | the view's size (UPSCALING draws native frames with FSR 1) | |

How a native renderer follows it: keep a copy of the `Quality` it applied and the revision; each frame
compare `native_quality::revision()`, and on a change apply the next-frame fields (shadow targets
re-made at the new size, the light and post switches) and log one line with `Quality::summary()` and
which changes need the next map. `ULTRA` must be exactly the renderer's full quality (everything on, its
own maxima): the presets only take away.


### Frame cost (2026-11-03, `BO2MP_IW6_PROFILE=1`)

**Profile.** `BO2MP_IW6_PROFILE=1` keeps `render_iw6::profile`: per phase of a frame (`setup`, `sun near`,
`sun far`, `spot`, `opaque`, `fog`, `blended`, `bloom`, `iris`, `final`) the CPU time split into
prepare (first use), constants and encoding, the draws, draws culled, draw calls issued (runs merged),
pipeline / bind group / vertex and index buffer switches, the bytes uploaded and own submissions; GPU time
per phase from timestamp queries when the device has them inside encoders (read back a few frames later).
Offline `iw6_shot` prints a line per frame (with the record, submit and wait-for-GPU times;
`IW6_SHOT_FRAMES=n` frames per view, `IW6_SHOT_MOVE=deg` turns the view each frame); in game the line comes
every 5 s. One `iw6 fps:` line per map (at load in game, at the end offline) gives the preset applied and
each list's draws, pipelines, groups, constant plans and frame blocks.

**What changed** (each checked by pixel diff of the five standard 1080p shots against the shots before:
byte-identical unless said):
1. *One submission.* The sun's two cascades are two views of one caster list, the spot lights' four
   tiles four views of another: each view's frame blocks and per-object blocks sit side by side in the
   same buffers (dynamic offsets on the shadow pipelines' frame blocks too, `render_iw6::layout`), so every
   view is written before the frame is submitted and all go in the frame's encoder. Before, each spot tile
   was its own submission (CPU 145 ms of a 190 ms frame on lavapipe); a separate list per tile had run
   lavapipe out of memory because it copied every list, bind group and per-object buffer per tile.
2. *Constants written once.* Each object's per-object block is made once (and again when it moves); each
   frame only the rows that follow the view (an object's view-projection where its technique reads it,
   frame values in per-object blocks) are rewritten, and only bytes that changed are uploaded. Frame blocks
   are shared by every plan with the same layout and light (433 for the scene's 883 plans; 6 for the
   shadows' 58). The blended part reuses the opaque part's constants. Upload: 7.5 MB a frame before, 0 KB
   with a still view, 178 KB with a turning one.
3. *Sorted and merged.* The opaque list is sorted by pre-pass first, the sky, the material's sort key
   (as the engine sorts its draw lists), then pipeline, groups, mesh and index range (decals keep their
   order within a sort key; `IW6_DBG_NOSORT=1` keeps the scene's order); the shadow lists by pipeline,
   groups, mesh. Draws with the same state over adjacent index ranges go as one draw call.
4. *No per-frame GPU objects.* The post passes keep their bind groups and constant buffers per set of
   textures (constants written only when they change); the iris keeps its views and every level's group.
5. *Made at load.* Every list a frame draws (pipelines, groups, constant blocks) and the bloom passes are
   made as the host is made; in game the whole host is made on the loading thread (`iw6 native: lists
   made in ...`), so the render thread never compiles a pipeline and the first view of a spot light no
   longer hitches.
6. *LODs and cull distances* (`BO2MP_IW6_LOD=0`: the first LOD at any distance, as before). Each static
   placement draws every LOD of its model in its band (LOD i from the previous LOD's distance to its own,
   READ `XModelLodInfo.dist`), capped by the placement's `cullDist` (READ, `GfxStaticModelDrawInst`;
   5,535 of mp_frag's placements have one); INFERRED: measured from the placement's origin, unscaled by
   the zoom. A coarser LOD of a vertex-lit placement reads its own run of the baked vertex light: the
   placement's values hold every LOD's vertices in order, each surface's run rounded up to an even count
   (READ from the data: exact for all 96 vertex-lit models on mp_frag; the first LOD's later surfaces were
   read from the raw counts before, one vertex off after an odd surface). Pixel diff against before: LOD
   swaps on props at middle distance (view 4: 0.5% of pixels by more than 8 levels), and the iris a little
   brighter where the swaps change the mean (view 0 exposure 1.906 to 1.920).

**Numbers** (mp_frag, 1920 x 1080, lavapipe on 2 shared cores, steady frames, still view; CPU = `Host::record`;
noisy: another worker shared the machine for the later runs):

| view | CPU before (ms) | CPU after, LODs off | CPU after, ULTRA | draw calls before / after | pipeline switches before / after | bind group sets before / after | own submits |
|---|---|---|---|---|---|---|---|
| 0 intermission | 183 | 18 | 23 | 31,133 / 16,157 | 2,301 / 442 | 33,238 / 21,592 | 4 / 0 |
| 1 yard | 132 | 8 | 13 | 15,463 / 8,010 | 1,216 / 184 | 17,077 / 10,407 | 4 / 0 |
| 2 interior | 302 | 16 | 16 | 34,899 / 18,751 | 3,489 / 474 | 41,389 / 25,625 | 4 / 0 |
| 3 spot floor | 211 | 15 | 16 | 23,550 / 13,286 | 2,071 / 321 | 27,145 / 17,243 | 4 / 0 |
| 4 gravel | 154 | 11 | 12 | 20,149 / 11,417 | 1,389 / 355 | 21,841 / 14,750 | 4 / 0 |

Constants: 30-36 ms a frame before, 1.5-4 ms after. lavapipe's own work (rasterising, on the CPU) is most
of a software frame (1.1 to 3 s) and is not what a GPU spends; the GPU split per phase is in the profile
line (view 0 before: sun far 420 ms, opaque 1,050 ms of 1,670 on lavapipe).

**Presets** (view 0, `BO2MP_QUALITY=...`, same machine; draws after culling and calls are the steady
measure, the times are lavapipe's):

| preset | draws (view 0) | draw calls (view 0) | CPU record, view 0 (ms) | lavapipe frame, view 0 (ms) |
|---|---|---|---|---|
| LOW | 14,425 | 7,541 | 15.8 | 1,076 |
| MEDIUM | 17,906 | 9,751 | 20.2 | 1,834 |
| HIGH | 27,035 | 14,989 | 30.3 | 2,238 |
| ULTRA | 29,012 | 16,157 | 19.4 | 2,241 |

(The CPU times move with the other worker's load; the draws and calls do not. LOW draws no shadow map,
no spot or omni light and half the LOD and cull distances: 1,076 ms of lavapipe's against 2,241.)

**Round 2 (2026-11-03).** Every change here is checked two ways on the five standard 1080p shots:
against the shots before (byte-identical), and turning the view 0.5 degrees a frame and back
(`IW6_SHOT_MOVE=0.5`: the last frame is at the shot's angle; byte-identical to the still shot). The second
check found a bug in round 1 (fixed on main as its own commit): the blended part reused the opaque part's
constants when the code values' address matched, and that address was the same every frame, so with a
moving view the constants froze after the first frame (76-99% of the pixels wrong after a turn). The
reuse is now stamped by a frame counter (`Renderer::begin_frame`).
1. *Shadow atlases reused.* The sun's cascades and the spot tiles hold only the world's static casters
   (players and the gun go through Black Ops II's passes). When the atlas's projections, the view and the
   lights chosen are bit for bit last frame's and nothing moved (`Renderer::moved`, a door, a list made
   again), last frame's atlas is this frame's and nothing is drawn into it (`BO2MP_IW6_SHADOW_CACHE=0`:
   drawn every frame). The view counts because the displacement casters' techniques read the camera's
   rotation and projection (`INVERSE_TRANSPOSE_WORLD_VIEW_MATRIX0`, `TRANSPOSE_PROJECTION_MATRIX`), so a
   turn changes a few of their texels (24 pixels in the intermission view); keying on the eye alone failed
   the moving check. So a still view (standing, aiming) draws no shadow pass; any movement draws them.
2. *Texture and sampler groups shared.* They were made per pipeline, material, lightmap and probe; now
   one per set of layout entries and what they bind (layouts with equal entries are one in wgpu), and the
   list is sorted by them: bind group switches at the intermission view 21,592 to 13,156 while turning.
3. *Cells and portals* (`render_iw6::cells`; **off by default**, `BO2MP_IW6_CELLS=1`). `fastfile_iw6` now keeps
   what its walk already stepped through (`GfxWorldRec::{dpvs_planes, dpvs_nodes, cells}`: the plane tree,
   each cell's portals and AABB tree nodes; `walk_retail` on mp_frag still ends at the zone's last byte), and
   the portals are walked each frame from the camera's cell with `dpvs_iw4::visit_cells` (the walk the
   Black Ops II path runs), admitting a world surface or placement whose cells are seen, then the frustum.
   READ: the tree finds 31 of 32 cells' own centres (one centre lies in solid); portal planes face the
   neighbour, `closeDistance` is 0 on all 270 portals. Why it is off: mp_frag has 32 large cells, many not
   convex (cell 2 is the whole ground floor of a hall under two upper cells), so from the interior view
   the camera's cell holds the eye behind two of its own portals and the walk lost the room in front of it
   (39% of the picture); the per-cell clip planes cut too much (a door frame's edge seen through a doorway);
   with neither, the gain is 9% of the draws at the intermission view and 21% at the spot light view, and
   the eye-behind-a-portal fallback leaves most views uncut. It needs the engine's camera-cell and
   portal-clip rules read from the exe before it can be on.

| view (mp_frag, 1080p, lavapipe) | record before round 2 (with the constants fix), still (ms) | after, still (ms) | turning, before / after (ms) | bind group sets, turning (before / after) |
|---|---|---|---|---|
| 0 intermission | 16.0 | 7.4 | 14.5 / 16.9 | 21,597 / 13,156 |
| 1 yard | 10.2 | 4.3 | 13.5 / 11.8 | 10,345 / 6,144 |
| 2 interior | 15.6 | 6.8 | 15.5 / 15.4 | 25,392 / 15,682 |
| 3 spot floor | 15.5 | 5.4 | 15.0 / 14.1 | 17,225 / 10,940 |
| 4 gravel | 14.1 | 5.4 | 14.9 / 12.7 | 14,739 / 8,736 |

Constants with a turning view: 2.3-3.3 ms a frame, 170-180 KB uploaded (the round-1 numbers were taken
with a still camera and frozen constants after the first frame).

Not done, with why: *instancing* (the translated shaders read their per-object block as a uniform block at
a dynamic offset; instancing needs it as a storage array indexed by the instance, a change to every
translated vertex program); *render bundles for the shadow views* (each view's draws are culled per frame;
a bundle would replay the uncut list, more GPU work for less CPU); *casters smaller than a far cascade
texel* (not exact: changes the map); *shadow views with their own rotation and projection* (what the engine
gives the casters' displacement maths is not read; it would let a turn reuse the atlases).

**On the owner's GPU (to confirm):** the `iw6 native: lists made in` time; the profile line in game with
`BO2MP_IW6_PROFILE=1` (CPU per phase and the GPU's timestamps per phase on the RX 9070 XT) against the
budget (a GTX 1080 holding Ghosts on ultra: well under 8 ms GPU, 2-3 ms CPU); changing GRAPHICS QUALITY in a
match (shadows and lights at once, `iw6 fps: quality ...` in the log); LOD swaps at a distance that looks
like Ghosts'.

**Not done (next):** cells and portals (the walker reads `GfxCell`, portals and the AABB trees' surface
and placement lists but captures none: portal culling would cut the opaque list most indoors); instancing
of repeated placements (the per-object constants are uniform blocks at dynamic offsets; instancing needs
them in a storage buffer indexed by instance); culling casters too small for a far cascade texel; caching
static casters in the sun cascades (as `render_h1`'s tile cache); recording passes in parallel (wgpu
records a pass on one thread; the shadow views could go to bundles); depth of field, SSAO and motion blur
are not drawn, so their settings change nothing here yet.

**Round 3 (2026-11-03): the owner's first real numbers, and where the frame goes.** The owner's PC
(i9-14900KF, RX 9070 XT, Vulkan, AMD 26.10.1), mp_frag, ULTRA, about 1080p: 57-80 fps in the match (200 in
the menu, the frame cap), against 500+ fps for Ghosts itself on the same PC. His two logs:

| per frame, steady (bench run, AutoNoVsync, ~75 fps = 13.3 ms) | ms |
|---|---|
| main thread: PreUpdate + fixed | 0.6-0.8 |
| main thread: Update (Anim 0.5-1.0, Ui 1.4-2.0, the rest ~0.7) | 2.6-3.7 |
| main thread: PostUpdate | 0.5 |
| main thread: "after Last" (extract, then waiting for the render thread) | 8-12.7 |
| render thread: `render_iw6` record (`BO2MP_IW6_PROFILE=1`): sun near 0.42, sun far 0.85 (9,417 draws), opaque 1.81 (15,240 draws), blended 0.17, post 0.05 | 3.2-3.5 |
| render thread: wgpu encoding those commands at the encoder's `finish` (not in the record: wgpu 29 validates and records every command for the driver there); estimated from the noop backend's finish/record ratio here (~0.5) | ~1.6 |
| render thread: the driver's command recording, Bevy's extract / prepare / queue, Black Ops II's passes (players, gun, effects, UI), present | ~7-8, not measured yet |
| GPU | not measured (the timestamps read 0, below) |

So the frames are render-bound (the main thread waits ~9 ms for the render thread), `render_iw6`'s own
recording is about a quarter of it, and the rest has not been measured on his PC. In his play session (vsync
on) the 10 s windows run at 57-78 fps moving and up to 148-184 fps in stretches where the view was still and
the shadow atlases were reused (round 2): drawing the sun cascades (~9,700 draws) is a large share of a moving
frame.

What changed:
1. *The profile's GPU readback* (`render_iw6::profile`). It asked for a readback buffer's map in
   `profile_line`, right after `record` had copied the frame's timestamps into it and before the frame's
   encoder was submitted; a buffer waiting for its map may not be in a submission, so wgpu refused the
   frame's whole queue submit ("Buffer with 'iw6 profile read' label is still mapped", every frame with the
   profile on): those frames were not drawn and the timestamps read 0. Each readback slot now remembers the
   frame that wrote it and is only asked for once a later frame has begun; the device is polled without
   waiting; a failed map frees the slot; the ring is 8 deep, a frame with no free slot is counted. On
   lavapipe the GPU line is real again (view 0 turning: sun near 30, sun far 225, spot 107, opaque 676,
   fog 58, blended 2, bloom 50, iris 7, final 34, total 1,190 ms of a software GPU: opaque ~55%, sun far
   ~20%).
2. *Which option rows took parts away* (`native_quality::Quality::options_off`). The owner's `iw6 fps:
   quality ULTRA (... spot shadows 0, local lights off, bloom off, dof off ...)` is his own rows (REAL-TIME
   LIGHTING, BLOOM, DEPTH OF FIELD off), not the preset: ULTRA maps everything on. The line now ends with
   "; taken away by the options: BLOOM off, DEPTH OF FIELD off, REAL-TIME LIGHTING off (spot and omni
   lights, spot shadows)".
3. *Packed cull* (`world::Cull`). Every list entry (`Ready`: pipeline, three bind groups, two offset
   vectors) was walked by every view's cull (74,121 scene entries, 36,779 per sun cascade, the same per
   spot tile). Each prepared list now keeps a packed `Cull` per entry (bounds, LOD band, cells); the cull
   walks those alone into the entries seen, and only those are encoded. Byte-identical (still and turning).
   Encoding about 20% less on the noop backend (views 0-4, medians: before 11.7/8.5/11.7/9.9/9.7 ms, after
   11.6/4.5/8.4/6.5/6.4 ms on this noisy 2-core VM); the profile now splits it ("enc ... of which cull":
   3.7 of 10.1 ms at view 0, the rest is wgpu's command recording: 16,157 draw calls, 13,167 group sets).
4. *Measuring wgpu alone.* `iw6_shot` prints `finish` apart from the submit, and runs on wgpu's noop
   backend (`WGPU_BACKEND=noop WGPU_NOOP_BACKEND=1 T7_LIMITS=default`: wgpu-core's own CPU time, no driver
   and no software rasteriser).

Tried and dropped: *render bundles* per chunk of 96 sorted list entries, replayed while the chunk's
commands are unchanged (byte-identical, all chunks reused on a still view). `finish` did not get cheaper
(noop backend, turning, views 0-4: 6.3/3.0/8.2/4.6/4.2 ms against 6.8/2.8/7.2/4.9/4.2 without; lavapipe
the same within noise), and the record got no cheaper either; not kept. (Why not was not found: wgpu-core
still replays every bundle command and merges each bundle's resources into the pass.)

What the frame still needs to reach Ghosts' 2-3 ms (in order of size, as far as the numbers show):
- **Measure the rest of the render thread on the PC**: `CODLEGACY_RENDER_LOG=1` logs Bevy's render stages
  every 64 frames (extract, prepare, queue, render, `encoder_finish_ms`, `gpu_frame_ms`), `CODLEGACY_BENCH=1`'s
  report file (`codlegacy-artifacts/bench/<stamp>.txt`, section [2/3] and [3/3]) has the main thread's spans and
  the render stages; with `BO2MP_IW6_PROFILE=1` the GPU per phase is now real.
- **Fewer draws.** 25,991 draws and 14,613 calls a moving frame; Ghosts draws its static models instanced
  and portal-culled. Instancing needs the translated vertex programs to read their per-object block from a
  storage array by instance; cells and portals (round 2, off) need the engine's camera-cell and portal-clip
  rules read from the exe.
- **The sun cascades while moving**: 9,700 draws a frame. Caching per cascade (the far cascade moves
  rarely if its projection snaps), a separate view-independent atlas the few view-dependent casters draw
  over (depth only, so the result is the same minimum), casters under a far texel (not exact).
- **Black Ops II's per-frame work on a native map** (main thread Ui 1.4-2.0 ms and Anim 0.5-1.0 ms;
  render-thread extract and prepare of the converted world's resources that a Ghosts map does not draw):
  to be read from the bench report's span tree before cutting.

**Round 4 (2026-11-03): the sun cascades kept while moving, the map's own caster sets, instancing.**
Measured with `iw6_shot` on wgpu's noop backend (`WGPU_BACKEND=noop WGPU_NOOP_BACKEND=1 T7_LIMITS=default`:
wgpu-core's CPU time, no driver, no rasteriser) and on lavapipe for the pictures; mp_frag, the five standard
views, 1080p. New in `iw6_shot`: `IW6_SHOT_WALK="x,y,z"` steps the eye that far a frame and arrives at the
shot's position on the last frame (with `IW6_SHOT_FRAMES=n`). Exe addresses for everything READ here:
`docs/exe_notes/IW6_LIGHTING.md` 3.3a.

1. *Cascades on their own texel grid; the far one held* (`render_iw6::{sun_shadow, sun_cache}`, `Host::record`'s
   sun block). READ: Ghosts centres each partition on the middle of the texel the eye falls in along the light's
   two axes, `(floor(e / texel) + 0.5) * texel`; done here (`sun_shadow::setup_at`, `snapped`), so a cascade's
   texels stay where they are in the world and two centres give the same texels. The far cascade then keeps its
   centre while the eye stays within 64 far texels (128 units on ULTRA, its fade band) along those axes, and is
   drawn again only when it is re-centred, a mover moved, or the atlas was made again; the near one is drawn when
   its own centre or the view changes (its displacement casters read the camera). A cascade drawn alone clears
   only its quarter (a depth-1 triangle over the viewport, `sun_cache::Clear`). Ghosts has no sun cache (no
   `sm_cache*` dvar): holding the far cascade is ours. Noted, not applied: Ghosts' far partition texel is 4x the
   near's; ours stays 8x (INFERRED earlier). Switches: `BO2MP_IW6_SUN_CACHE=0` (both cascades every moving
   frame, round 2), `BO2MP_IW6_SUN_SNAP=0` (on the eye; byte-identical to before).
2. *The map's own sun caster sets* (`scene::{surface_casts, placement_casts, shadow_opt_bit}`,
   `world::slots_for`). READ: a world surface casts sun shadows when bit 0 of its light flags is set, and with the
   map's optimised caster set (iwRad; `r_useShadowGeomOpt`, on by default; mp_frag has one) only when bit 3 is set
   too; a placement casts unless flag 0x10 or its model's flag 2, and with the set only with bit 0 of its
   `sunShadowFlags`. The spot lights' shadow geometry leaves out flag 0x10 and model flag 2 (READ); their world
   surfaces are not filtered (their own per-light lists are not read). INFERRED: the set's index is 0 (the first
   sun light; every map here has one), and a per-view switch the placement cull also tests is taken as on. On
   mp_frag the sun's casters go from 17,510 surfaces and 8,779 placements to 3,824 and 1,577. Also: casters
   spanning less than one far texel along both light axes are left out of the far cascade (`world::min_caster`;
   77 draws at view 0, no pixel changed). Switches: `BO2MP_IW6_CASTER_FLAGS=0` (every caster),
   `BO2MP_IW6_SHADOW_OPT=0` (the flag rules without the set), `BO2MP_IW6_FAR_MIN=0`.
3. *Instancing* (`render_iw6::instancing`, `world::{make_pipeline, make_groups, record}`, `layout::pass_layout_with`;
   on by default, `BO2MP_IW6_INSTANCING=0` off). A run of list entries that differ only in their per-primitive
   block (`b0`), each block the next one in the plan's object buffer, goes as one `draw_indexed` with an instance
   per entry (up to 64). The pipeline's instanced form has its translated vertex program read `b0` from a
   read-only storage array bound at the first entry's block, indexed by `instance_index`: the uniform
   declaration becomes the array, and a private copy of the old block is filled at the top of `main`, so the
   body is untouched. Passes whose pixel program reads `b0`, or whose vertex program reads the instance, are not
   instanced; a variant that fails validation is dropped with a note. Object buffers get `STORAGE` usage and 64
   blocks of padding. The profile counts `instanced` draws and the `instanceable` repeats left.

| mp_frag, 1080p, noop backend, walking 3.6 units a frame, medians of 28 steady frames, views 0 / 1 / 2 / 3 / 4 | main (round 3) | + 1 cascades held | + 2 caster sets | + 3 instancing |
|---|---|---|---|---|
| `Host::record` (ms) | 11.7 / 7.3 / 15.7 / 10.2 / 8.4 | 8.1 / 4.7 / 8.3 / 6.4 / 6.0 | 7.0 / 4.4 / 7.7 / 6.0 / 5.2 | 7.6 / 4.2 / 7.6 / 5.2 / 5.2 |
| encoder `finish` (ms) | 6.8 / 2.6 / 10.8 / 5.2 / 4.0 | 5.1 / 1.0 / 5.3 / 2.7 / 2.4 | 4.7 / 1.1 / 5.1 / 2.7 / 2.1 | 4.6 / 0.9 / 4.4 / 1.9 / 1.8 |
| frames the far cascade is drawn | 28 of 28 | 0 of 28 | 0 of 28 | 0 of 28 |
| far cascade when drawn, view 0 (draws / calls) | 9,115 / 5,167 | 9,115 / 5,167 | 2,158 / 1,426 | 2,158 / 1,426 |
| draws a frame | 29,465 / 14,287 / 30,992 / 21,187 / 19,632 | 20,195 / 5,017 / 21,240 / 10,435 / 10,767 | 19,670 / 4,659 / 20,058 / 9,574 / 9,873 | the same |
| draw calls a frame | 16,420 / 7,803 / 18,353 / 13,162 / 11,417 | 11,155 / 2,538 / 12,273 / 6,445 / 6,302 | 10,650 / 2,201 / 11,150 / 5,700 / 5,622 | 8,138 / 1,974 / 8,459 / 4,370 / 4,494 |

Running (10.8 units a frame, 58 steady frames): the far cascade is drawn on 2 of 58 frames (main: 58 of 58);
draw calls at view 0 17,160 -> 8,998, record 9.3 -> 8.0 ms, `finish` 6.1 -> 5.3 ms. Turning 0.5 degrees a frame:
calls at view 0 16,002 -> 7,676 (the far cascade is not drawn; the near one is, ~220 draws). The record and
`finish` times move with the other workers on this 2-core VM by about +-1 ms; the draw and call counts do not.
Instancing raised pipeline switches (view 0 opaque 325 -> 872: the instanced and plain forms alternate).

Pictures (lavapipe; byte-identical unless said):
- *1*: `BO2MP_IW6_SUN_SNAP=0` byte-identical to main. With the snap the near cascade moves by under half a
  texel (0.125 units), which moves soft shadow edges: views 0 / 1 / 2 / 4 differ on 9.0 / 11.3 / 25.3 / 3.2% of
  the pixels, by more than 8 levels on 0.09 / 0.28 / 2.5 / 0.14% (max 62; view 2 is a wall just in front of
  the camera, the shift along one penumbra); view 3 identical.
- *2*: views 0 / 1 / 2 / 4 differ from 1 on 19.1 / 13.2 / 0.25 / 5.7% of the pixels, more than 8 levels on
  0.14 / 0.18 / 0.05 / 3.7%, view 3 identical. Most of view 4's change comes from flag 0x10 (the set alone
  differs from it by 0.05%): the plank stacks in the yard are 0x10 placements (sunShadowFlags 0), so they
  cast no dynamic sun shadow and the ground under them is sunlit (Ghosts' sun shader replaces the baked
  visibility with the map inside the cascades). That is what the zone's data says Ghosts draws; it needs a
  comparison with Ghosts itself at that spot (`BO2MP_IW6_CASTER_FLAGS=0` to compare).
- *3*: byte-identical to 2, and to `BO2MP_IW6_INSTANCING=0`, on all five shots.
- *Moving* (720p, the last frame of a sequence against the still shot): main is byte-identical walking and
  turning. With 1-3, turning: views 1-4 identical, view 0 133 pixels (max 6); walking 3.6 units a frame for 5
  frames: views 2 and 3 identical, views 0 / 1 / 4 differ on 0.16 / 0.35 / 0.07% of the pixels, 12 / 8 / 5
  pixels by more than 8 levels (max 33). That is the held far cascade, drawn from an eye up to 20 units back:
  its casters' LOD bands and the displacement casters' view-dependent texels are from that eye.

**What the owner should see** (RX 9070 XT): moving frames no longer redraw the far cascade (9,400 draws on
his log) except about every 1-2 s of walking (a frame with ~2,200 far draws, not 9,400); the near cascade is
~220 draws; the opaque list sends ~25% fewer calls. On the round 3 breakdown that takes `render_iw6`'s
record from ~3.3 ms to roughly 2 ms a moving frame and wgpu's `finish` with it; moving frames should get much
closer to the still-view rate (148-184 fps), the rest being Black Ops II's per-frame work (round 3's list). To
check: `BO2MP_IW6_PROFILE=1` while running (`sun far` should be absent from most lines, `sun far cached` in
its place; `instanced` counts in `opaque`), and the plank stacks' shadows against Ghosts.

Not done: shadow casters split into static and dynamic layers (players and the gun are not in Ghosts' atlas
here at all, they go through Black Ops II's passes; movers, the doors, still redraw both cascades for the
frames they move); instancing for passes whose pixel program reads `b0` (the instance would have to be passed
down as a flat varying); the far partition at Ghosts' 4x texel.

**Round 5 (2026-11-04): inward views (the owner's 57-80 fps looking into the map).**
The scene pass of a dense inner view drew 15-32k list entries and 1.4-3.0M triangles on mp_frag (the world is
295k opaque triangles in 17.5k surfaces plus 8.8k placements; the frustum cull keeps what lies in front, walls
included). MWR's methods (`render_h1`, `docs/H1_NATIVE.md` "Occlusion") ported to `render_iw6`:

1. *Occlusion* (`world::{find_occluders, Renderer::occlusion}`, `native_cull::{OccluderSet, Occlusion::from_set}`;
   on, `BO2MP_IW6_OCCLUSION=0` off). When the scene list is made, the large triangles of opaque, depth-writing
   world and placement draws that have a depth pre-pass, cannot discard (pixel program has no `discard`), have no
   distance band and have bounds are kept (`IW6_OCC_EXTENT` 48 units box extent, `IW6_OCC_AREA` 400 square units
   per triangle, `IW6_OCC_CAP` the largest 12,000 of 294,971 on mp_frag; the side the game draws comes from the
   state's cull mode and the vertex kind). Each frame the clusters (24 triangles, Morton order, own boxes) inside
   the view are rasterised into a 192x108 depth buffer (a triangle's far depth, so it never over-hides) and a draw
   whose box lies wholly behind it by 12 units is left out of the pre-pass, the lit pass, the fog and blended
   parts and the light passes of the scene view (not the sun or spot shadow views). The buffer is kept while the
   camera stands still. Cost: 0.02-0.06 ms a frame. `native_cull`'s fill is now scanline spans (was a box scan).
2. *Small-on-screen cull* (`world::small_angle`, `native_cull::too_small`; `BO2MP_IW6_SMALL_CULL=<radians>`, 0
   off). A draw whose box diagonal spans under the angle from the eye is left out of the scene view: LOW 0.010
   (about 5 px at 1080p), MEDIUM 0.007, HIGH 0.003, **ULTRA none**. The first try at 0.003 on ULTRA changed 89 thin
   bright pixels of the intermission view (max 119 levels), so ULTRA keeps the full picture and only the lower
   presets take away.
3. Profile line: `occluded`, `small` and `tris` counts per part (`BO2MP_IW6_PROFILE=1`); `IW6_DBG_OCC=1`,
   `IW6_DBG_CENSUS=1` (opaque list by kind, distance, triangles per draw, material), `IW6_DBG_SPAWNS=1` (spawn
   origins to pick cameras).

Measured with `iw6_shot` on wgpu's noop backend (CPU only; **no GPU time is claimed**), 1920x1080, ULTRA, 29 steady
frames, ten cameras: the five standard views (0-4) and five inward views from spawns (eye at spawn + 60:
5 `1190 770 212 0 180`, 6 `-898 2404 208 0 270`, 7 `76 -824 208 0 180`, 8 `222 3363 208 0 270`, 9
`731 -429 208 0 90`). Scene opaque list only (the still shot's sun cascades are cached); CPU ms vary by about +-1
with the other workers on the 2 cores, the draw and triangle counts do not.

| view | draws before / after | calls before / after | triangles before / after | occluded | record ms before / after |
|---|---|---|---|---|---|
| 0 | 15,130 / 13,364 | 5,966 / 5,953 | 1.50M / 1.36M | 1,382 | 14.0 / 9.3 |
| 1 | 2,247 / 2,145 | 959 / 945 | 228k / 224k | 80 | 9.2 / 2.6 |
| 2 | 15,317 / 11,586 | 6,072 / 5,696 | 1.39M / 1.17M | 3,466 | 9.9 / 6.8 |
| 3 (spot floor) | 6,990 / 3,668 | 3,103 / 2,132 | 838k / 552k | 3,306 | 3.9 / 4.2 |
| 4 (gravel) | 6,313 / 4,698 | 2,539 / 2,427 | 1.39M / 1.10M | 1,487 | 10.7 / 4.1 |
| 5 inward | 15,962 / 11,876 | 6,286 / 5,276 | 1.42M / 1.13M | 3,848 | 9.4 / 8.9 |
| 6 inward | 19,255 / 10,876 | 8,033 / 5,220 | 1.68M / 1.00M | 7,549 | 11.9 / 5.6 |
| 7 inward | 3,370 / 2,668 | 1,274 / 1,213 | 350k / 326k | 680 | 3.1 / 4.4 |
| 8 inward (north hall) | 31,907 / 10,530 | 12,413 / 5,759 | 3.05M / 1.29M | 19,799 | 12.8 / 7.6 |
| 9 inward | 23,595 / 16,823 | 9,020 / 7,780 | 2.29M / 1.70M | 5,749 | 11.8 / 8.0 |

(Before = `BO2MP_IW6_OCCLUSION=0 BO2MP_IW6_SMALL_CULL=0`, which is the code before this round. The record ms of
views 3 and 7 rose within the noise: their draws fell, the cost of the list was not the bound.) The saving is
largest where the owner's complaint is: the dense inner views lose 25-65% of the draws and 20-58% of the
triangles, the same ones the pre-pass and the lit pass both draw.
Larger occluder sets hide far more in the halls (`IW6_OCC_CAP=40000 IW6_OCC_AREA=150`: view 0 13,364 -> 11,129
draws, view 5 11,876 -> 9,748, view 8 10,530 -> 5,268, view 9 16,823 -> 9,807, triangles to 0.96-1.15M), at the
same rasterise cost (0.02-0.06 ms) and no more cull time. **They are not the default**: they showed false hides.
With extent 48 the intermission view at 720p lost 8 pixels (max 75 levels); with extent 96 that view was clean
but inward view 5 at 720p differed on 5.0% of the pixels (max 30, on 0.004% more than 8 levels) against
`BO2MP_IW6_OCCLUSION=0`, so a draw whose pass adds light or colour behind some of the smaller occluders is being
hidden (not found; the margin, the occluder side rule for two-sided surfaces or a pass that should not be
tested are the places to look). The defaults (12,000 / 400 / 48) are the set that was byte-identical on all ten
views; the env switches `IW6_OCC_CAP`, `IW6_OCC_AREA`, `IW6_OCC_EXTENT` are left for that investigation.

Pictures (lavapipe, byte for byte against `BO2MP_IW6_OCCLUSION=0 BO2MP_IW6_SMALL_CULL=0`): views 0-4 at 1080p and
views 5-9 at 1280x720 (1080p ran the 8 GB box out of memory): **identical on all ten**. Small cull alone at the
ULTRA value tried first: 89 pixels of view 0 (so it is off on ULTRA, above).

Not done, and why:
- *Cells and portals* stay off (`BO2MP_IW6_CELLS=1`): the occlusion test replaces them for the interior views
  (mp_frag's cell 2 is a non-convex ground floor; the exe's camera-cell and portal-clip rules are still unread).
- *GPU-driven culling and indirect draws*, *per-cascade caster culling beyond round 4*, *spot-light tile cost*,
  *screen-size LOD like the exe*: not started; with occlusion on, the scene list is 5-12k draws and 3-6k calls on
  the inward views, and the CPU cost is no longer the clear bound on the noop numbers (record 5-9 ms, a third
  of it the cull itself on the larger lists). What the card does with 1.0-1.7M triangles through two passes is
  unmeasured (no GPU here).

**What the owner should see** (RX 9070 XT, 1080p ULTRA): `BO2MP_IW6_PROFILE=1` line with `occluded` in the thousands on
inward cameras and `triangles` down by a third to a half; frame rate in the dense hall views up (the draw count at
the north hall camera is a third of before). To check: the same spot with `BO2MP_IW6_OCCLUSION=0`, and screenshots
for popping at doorways when turning (a draw is hidden only when its whole box is behind the occluders' far depth
by 12 units, so popping would mean an occluder rasterised too large; none seen in the ten shots).

## Props and effects audit (2026-11-04)

Owner reports from his PC (many maps; only mp_frag is staged in the cloud): effects missing or wrong on all maps, props invisible, giant white
translucent fans and black gaps on a snow map, a night map that looked like day. Everything below was measured headless on mp_frag
(`iw6_prop_audit`, `iw6_shot` software renders, `iw6_techset`); nothing was seen on a GPU. Cross-notes for the effect drawer: `exe_notes/IW6_FX_HANDOFF_PROPS.md`.

### Re-verification of the previous worker's claims

| claim | result |
|---|---|
| no ground displacement by default removes the black cracks | VERIFIED: `iw6_shot mp_frag --cam -193 1650 500 30 90`: with `BO2MP_IW6_TESS_PARMS=neutral` (old) black wedges open in the gravel; with the default none, and no holes appear |
| clouds (types 8, 9) off by default | kept (unit tested); the fans are not proven to be clouds, see the handoff note |
| LOD count from `numLods` | builds and renders; mp_frag: 7047 placements with LOD bands |
| broader model entity classes | builds; no effect on mp_frag (51 script_model, all placed or deliberately left out) |
| createfx from source text | mp_frag: text and compiled agree (245 placements, no note) |
| opaque grey fallback | works: `iw6 native` line now counts the groups; 1057 pictures missing here (5 of the paks staged) |
| scriptable first-state effects | works as built but mp_frag has none: its barrel and light scriptables hold event types 7 and 10 (light settings), the sparks of the toy boxes are in state 1 (destroyed) |

### What draws on mp_frag (props)

| source | in the zone | drawn | not drawn, why |
|---|---|---|---|
| static models | 8779 (387 models, all with geometry) | 8779 placed (23788 surfaces) | 292 surfaces of `m_shadowcaster_atest` / `mv_shadowcaster_atest`: shadow proxies, not drawn in the scene on purpose; 3477 draws (all blended) skipped only because their colour picture is not in the staged paks (they draw with the full pak set) |
| dynamic entities | 273 | 257 models + 16 brush models | none |
| scriptables | 52 | 37 (29 + 8 drawn as their dynent) | 15 are lights with no model (`light_flicker_fast`, `light_pulsing_*`) |
| `script_model` entities | 51 | 29 | 20 are game objects, exploders, crates, light_off twins (script's own); 2 are later states |
| script brush models | 15 | 15 | none |
| world surfaces, techset `w_unlit_add_lin_bl_blmod_z1_nfwpf` | 396 surfaces | 0 before | FIXED (below) |
| effect-set surfaces (`mc_effect_falloff_*`, `m_effect_zfeather_*`) | 56 draws | 0 before | FIXED (below, slot 2 fallback) |
| props whose model only another zone holds | 0 on mp_frag | | FIXED in code, untested here (below) |

Causes found and fixed:

1. **Displacement-only unlit effect sets were never drawn.** `w_unlit_add_lin_bl_blmod_z1_nfwpf` (ground glow cards, additive, vertex colour) fills slots 2, 7, 8, 9, 13 with one technique that has a hull shader and has no `no_tess` twin (slot + 309), so `pick` found nothing: 396 surfaces ("no technique for slots [7]", 308 + 64 + 24). `world::plain_twin` now maps such a set to `wc_unlit_add_lin_ndw_bl_blmod_nfwpf` (same blend and picture arguments, trivial vertex shader). INFERRED from the names; unit tested; on mp_frag they now reach the "colour map not loaded" check (3009 to 3477), i.e. they draw with the full pak set. The pipeline of the twin was not built here (the shot gets OOM-killed on this shared box when the debug switch `IW6_DBG_BLENDED=1` draws them with stand-ins): if a glow card looks wrong, `BO2MP_IW6_TWIN=0` is NOT provided; look at `pipeline not made` in the `skipped` lines.
2. **Effect techsets with no lit slot** (`mc_effect_falloff_add_..` fills 2 to 6 only): drawn through slot 2 (never a shadow caster set: the rule needs `_effect_` in the name).
3. **Props whose model sits in another zone.** `extra_props` finds models by name in the map zone only; a dynent, scriptable or `script_model` whose record is a stub (or absent) was "left out (model in another zone)". The scene now draws it from the first loaded zone that holds the model with geometry (its own materials), and the spawned-model pool also looks in the other zones. The `iw6 props:` line says how many were drawn from another zone and how many were in no zone read. Static models still read the map zone only (all 8779 are in it on mp_frag; the line `iw6 props` is the check on other maps).
4. Fallback picture: see the table above.

Not found / not fixable offline: the props the owner sees invisible on his maps are not reproducible here; the likely general causes (pictures of alpha-tested foliage missing, models only in `common`/`patch_common`, LOD last-distance culling) are covered above or in the previous worker's commits. Models that only `common.ff` holds are not loaded by the renderer (`Scene::zones_for`; walking 124 MB is slow): the effect and model LANES search it (`shared::lane_zones`).

### Effects sources

| source | read from | status |
|---|---|---|
| createfx placements | raw `maps/createfx/<map>_fx.gsc` (text), compiled bytecode as fallback | works (mp_frag 245) |
| effect id to path | raw `maps/mp/<map>_fx.gsc`, compiled strings | works (40 paths) |
| script `playfx`/`loadfx` literals, exploders | reachable scripts through the Ghosts script host (the map's `main` runs at boot; `clientSideEffects` is 1, so createfx placements are the lane's) | works; was skipped when the map had no readable createfx: FIXED (the lane now still converts script, exploder and scriptable effects) |
| scriptable first-state effects | `Capture::scriptable_fx` | works; none on mp_frag |
| ambient fx entities in MapEnts | `script_struct`/`script_model` with `fx` keys: not read | open |
| dynent / destructible effects | the destructible definitions name them through variables: only literals are read | open |
| weather and ambient by dvar and vision | not read | open |
| zone search order | map zone, then `SHARED_ZONES` (patch_common_mp, common_mp, patch_common_core_mp, common_core_mp, the two DLC zones), then `patch_common`, `common` | resolves 513 more effects (previous worker) |

New log line, ONE per map (send it): `iw6 fx NOT RESOLVED <script>: createfx ids N [...]; script paths N [...]; exploder ids N [...]; scriptable effects N [...]; child effects N [...]; with nothing drawable N [...]; sprite materials not linked N [...]`. "Sprite materials not linked" names the materials that are not found, have an unsupported technique (distortion, radial blur, dust on lens) or have no colour picture.

The night map that looked like day: not diagnosed (no map here); the first things in his log to check are the `NOT RESOLVED` line (glow sprites, beams, flares, fire missing), then `iw6 fog`, `iw6 tone` and `iw6 sun`.

Tools added: `cargo run -p render_iw6 --release --example iw6_techset -- <set>[,<set>] <zone.ff>...` (slots of a technique set and whether each technique and shader is present); `IW6_DBG_BLENDED=1` (draw blended surfaces whose picture is missing).

## VFX audit (2026-11-04)

Owner report: some Ghosts effects look wrong in game; no screenshots. Offline tools and a defect audit of mp_frag's
effects (245 createfx placements, 87 script paths, 42 distinct placed effects) against the exe-derived rules in
`docs/exe_notes/IW6_VFX_AUDIO.md`. Everything below was measured on the staged zones (paks 25/40/43/49/58 only), headless,
nothing seen on a GPU.

### Tools

| tool | use |
|---|---|
| `cargo run -p fastfile_iw6 --example iw6_fx_audit -- <zone dir> <map> [--paths file] [--census] [--quiet]` | text dump per effect: elements, types, counts, lifetimes, sizes, colours, lit/unlit, material, techset, blend state, which picture loaded or fell back; census of element types and of fallback pictures |
| `cargo run -p render_fx --example iw6_fx_shot -- <zone dir> <map> --effect <name>... [--paths file] [--placed] [--world --cam x y z pitch yaw] [--times 0.1,0.5,1,2,4] [--text-only] [--out dir]` | the game's own `FxSystemHost` and `build_fx_verts` (the same element rules as in game) run one effect isolated at its origin (or every createfx placement with `--placed`) and print per frame: sprite, trail, cloud, spark, light, model counts, the skipped counters, per element size, colour, flags, material and blend; PNG per time (software rasteriser; magenta soft disc where the picture is not in the staged paks, flagged FALLBACK in the dump; `--world` draws over the native frame's depth) |

`assets::iw6_fx_harness_views` and `asset_game::FxDefinitions::bind_materials_with` expose the lane's conversion to the tools.

### Defect table

| # | defect | evidence | status |
|---|---|---|---|
| 1 | Element types 8 and 9 (the exe's clouds: dust motes, snow, insects) were not mapped (`aw_elem_type` returned 255) so every cloud element became a silent sound element | census: 6 of the 42 placed effects hold a type 8 element; harness counted them under `unsupported`; the exe's draw order is 8 and 9 = clouds | fixed: 8 and 9 are kept (played as sprites / spark clouds); `BO2MP_IW6_FX_CLOUDS=0` for the old behaviour. Unit tests in `assets::lane::iw6_fx` and `asset_game::fx_catalog_s1` |
| 2 | Type 4 (flare) has no BO2 counterpart and plays silent | census: 1 placed effect (`gfx_flare_clr_*` family, 19+ materials with `eyeoffset` techsets) | opt-in: `BO2MP_FX_FLARE_SPRITE=1` plays it as a camera-facing sprite (no screen-space occlusion fade, which the exe does). Off by default: not verifiable without the pictures |
| 3 | Model elements (type 11) are dropped: the lane turns model visuals into `None` | 6 effects (`fx_metal_piece02_distant`, `fx_paper_sheet01_burn`, `fx_mail_03`, ...: debris and burning paper) | not fixed: needs the IW6 xmodel loader wired into the effect model catalog; debris only |
| 4 | Alpha materials use the `_abl` techniques, whose blend codes are 13/14 (`SRC1_ALPHA` / `INV_SRC1_ALPHA`, dual source). Codes 11..14 are not mapped by `render_iw6::state` (falls to One = additive) and are "unknown" in the shared draw-state path | 22 material states in the audited set (steam, smoke wisps, dust) show `blend op1 13/14` | not fixed: a dual-source blend needs shader outputs and a GPU to check. If smoke looks too bright or additive in game, this is the first suspect |
| 5 | Most effect pictures are in paks that are not staged | census: 84 pictures fall back across all effects, 44 for the placed set; the harness paints them as magenta discs | not fixable offline; with the full pak set they load |
| 6 | Effects the scripts name but no staged zone holds (`fx/fire/*`, `fx/misc/light_glow_*`, aircraft lights, `tv_explosion`) | census_all: 36 | not fixable offline (needs the other zones) |
| 7 | Types 6 and 7 are mapped to cloud and spark cloud by the lane's older table; the exe only says "cloud-like, identity unknown" | no mp_frag effect uses them | left as is |
| 8 | "dormant" counts in the harness (e.g. 150 of 152 elements at t = 0.1 s over all placements) | these are delayed or expired elements, not drops | not a defect |

Checked and found consistent: element type counts against the exe dispatch (0, 1, 2, 4, 8, 10, 11, 12, 14, 16 occur; 3, 5, 6, 7, 9, 13, 15, 17 do not on mp_frag), lifetimes, spawn delay and seed formulas (as in `fx_iw4::iw6`), sizes and fades of the isolated frames (steam wisp: 1 sprite at 0.1 s growing to 4 at 4 s, half size 18.5 to 42.5, alpha 12 to 86), additive blends of `effect_*_add_*` techsets (state `add`, 92 states), `eyeoffset` techsets (59 materials; handled by `render_iw6`).

### Switches

`BO2MP_IW6_FX_CLOUDS` (on), `BO2MP_FX_FLARE_SPRITE` (off).

### Needs the owner's GPU or screenshots

- Smoke and steam (defect 4): is it additive or too bright against Ghosts.
- Flares with `BO2MP_FX_FLARE_SPRITE=1`; dust motes and snow (clouds) now that they are drawn.
- The full pak set for the pictures (defect 5).

## Needs the owner's GPU

- A screenshot of mp_frag in game (`BO2MP_IW6_NATIVE` on, the default) against the same spot with
  `BO2MP_IW6_NATIVE=0`, and against Ghosts itself: exposure level, fog, the sky (with the real pak), the
  model light, layered ground, the sun shadows (acne, the cascade edges, `BO2MP_IW6_SUN_SHADOW=0` to
  compare).
- The log line `iw6 native: <map>: ...` and any `iw6 native: the frame was not drawn` warnings.
- Memory: the scene holds the map, the technique set zones and the shared zones' images (as MWR's).

## Graphics options and map events (2026-11-04)

- Ghosts has no real-time lighting. The REAL-TIME LIGHTING option belongs to the Black Ops III look and no longer
  reaches `render_iw6`: the map's spot and omni lights are part of its own lighting, switched only by the graphics
  preset (LOW draws without them). The option's text says Ghosts maps are not affected.
- Map events: only `mp_frag`'s zone is staged here, so only its events are read. Door switches work (default on);
  exploders (the sprinklers) are read but nothing fires them: that is the map's compiled script, and the other
  maps' zones (Flooded's water, Tremor, Stonehaven and the rest) must be staged before their events can be read.

## Bots (2026-11-04)

What the owner asked: bots use the real bot pathing and the real vischecks on Ghosts maps.

**What the install has, and has not (READ, checked on every staged zone).** No staged zone has a `PathData` asset (mp_frag, patch_mp_frag, mp_frag_load, common, common_mp, common_core_mp, common_bots_mp, the patches, ui, code, techsets): `iw6_asset_types` lists the types. `common_bots_mp.ff` is 80 assets (dog animations and an empty raw file), no bot scripts. The scripts the common patch zones call (`maps/mp/bots/_bots`, `_bots_util`, `_bots_strategy`, `_bots_gametype_*`) are named in their far calls but are not in any staged zone, so the real bot behaviour rules (what a bot decides, how it picks goals, its difficulty table `mp/botDifficultyTable.csv`) cannot be read here. The engine's own link and visibility tables (`PathData.pathVis`, `pathExposure`, `pathNoPeekVis`, `pathZones`) are therefore not in the install either; the layouts for them are in `fastfile_iw6::layout` and the walk does not read the asset (nothing to check it against).

**What the map zone has (READ, verified).** Every node of the map is a `node_*` entity in the MapEnts: origin, angles, class (the type), script keys, and for traversals `Begin` -> `End` by `target` / `targetname` plus the animation (`animscript`). `fastfile_iw6::pathnode::PathNodes` reads them. mp_frag: 1902 nodes: Path 1356, Cover Stand 9, Cover Crouch 11, Cover Crouch Window 19, Cover Prone 1, Cover Right 120, Cover Left 112, Begin 137, End 137; all 137 begins pair with an end. Traversal animations: step_up 38, jump_down 48, jump_down_80 18, jump_down_40 1, jump_up_40 14, jump_up_80 5, jump_across_72 2, ladder_up 3, wall_hop 1, window_over_40 7. Nearest-neighbour spacing: median 50 units, farthest 128. The node origin is the standing capsule's centre: the floor is 36 below it for most nodes (measured on mp_frag with the player hull; the exe's nearest-node test centres the 36-high capsule 36 above the node). The 31 type names are the exe's table (`Error`, `Path`, `Cover Stand` ... `Jump Attack`).

**Route graph (`bots::from_path_nodes`).** One graph node per path node (all types, so cover nodes are goals). Walk links: each node tests its 14 nearest nodes within 160 units (height difference at most 18 or three quarters of the distance) with the same supported-walk probe the grid bake uses, over the map's real collision (clip brushes and mesh); a window or a glass pane gives a `BreakGlass` link as in the grid. Traversals: each `Begin` -> `End` pair is a `Negotiate` link (cost: length plus 48); the reverse link exists for `window_over`, `jump_across`, `wall_hop`, `ladder` and `step` animations, not for `jump_up`/`jump_down`. A* is the existing resumable A* (same quota, same route steps). A bot running a `Negotiate` step runs at the far node and holds jump (`MoveMode::Jump`); ladders are climbed as the movement code allows (not checked in a running match). INFERRED: the link rule (radius, neighbour count, step limit), the traversal direction rule and the cost; the node set and the traversal pairs are the map's. `BO2MP_IW6_BOT_NAV=grid` bakes the walk grid instead. Without nodes (every other game) nothing changes.

mp_frag result (test `cargo test -p bots --test ghosts_nodes -- --nocapture`, real collision): 10168 links (9982 walks, 186 traversal links from the 137 pairs and their reverses), 236 components, the largest has 1447 of 1902 nodes (the rest are 207 single nodes, 77, 33, 30...: cover nodes inside props, rooms behind closed doors, upper levels reached by ladders and doors); walk links are mutual in 98 percent of cases; 200 of 206 spawns attach to the largest component; 2470 of 2600 routes between their nearest nodes are found (the rest start or end in a one-way `jump_down` pocket). Doors the scripts close and open are in the graph since 2026-11-04 ("Script animations, doors and builtins" below).

**Vischecks (READ).** The script builtin `sighttracepassed(start, end, hitCharacters, ignoreEnt)` is a point trace with contents `0x0080A831` when `hitCharacters` is 0 and `0x0280E831` (the bullet mask) when it is 1, ignoring the given entity, and it includes the placed models' collision (the same switch as a bullet). `0x0080A831` = solid 0x1, glass 0x10, water 0x20, sky 0x800, clipshot 0x2000, and the world bits 0x8000 and 0x800000. Against Black Ops II's bot sight (the shot mask without glass): Ghosts' sight is blocked by glass, passes through foliage (0x2), is not stopped by characters (a teammate in the way does not hide a target), and does not use `ai_nosight` (0x1000). On a Ghosts map the bots' sight rays use this mask (`SimWorld::iw6_sight_mask`; `BO2MP_IW6_SHOT_MASK=0` returns Black Ops II's). The per-bot rules (field of view, distance, smoke, perks, per-frame budgets) are in the bot scripts and are not in the install; the bots keep their own (90 degree field, 5906 units, quota-limited probes). The engine also has a capsule trace for movement checks that uses the player mask without the characters' bits (`& 0xFDFFBFFF`).

**Switches.** `BO2MP_IW6_BOT_NAV=grid`, `BO2MP_IW6_SHOT_MASK=0`. **Not done:** reading `PathData` (no zone to check it on), the scripted bot behaviour (scripts absent), movers and exploders feeding the graph, a ladder or jump check in a running match (nothing was run).

## Effects: exe study (2026-11-04)

See `docs/exe_notes/IW6_FX_EXE.md`. Fixed: effect material falloff/near ramp/eye offset (`BO2MP_IW6_FX_MATERIAL`), blend by technique tokens with `_abl` as alpha blend (`BO2MP_IW6_FX_DRAW`), oriented sprite axes 1 and 2 (`BO2MP_IW6_FX_FRAME`). Harness: `IW6_SHOT_PERF=1` prints draws, blend switches and overdraw. Open: trails, models, lights, flares, decals, distortion, soft particles, perf work.

Update 2026-11-05: the owner's white fans are most likely `screen`-blend lamp beams drawn as add (`BO2MP_IW6_FX_SCREEN`, default on: OneMinusDst/One); exe per-element draw cull (`BO2MP_IW6_FX_CULL`), tail length by flag (`BO2MP_IW6_FX_TAIL_SIZE`), diagnostic `BO2MP_IW6_FX_HIDE=tail,oriented,billboard,mat:<part>,fx:<part>`. See `docs/exe_notes/IW6_FX_EXE.md`.

**Switches.** `BO2MP_IW6_BOT_NAV=grid`, `BO2MP_IW6_SHOT_MASK=0`. **Not done:** reading `PathData` (no zone to check it on), the scripted bot behaviour (scripts absent), a ladder or jump check in a running match (nothing was run).

## Script animations, doors and builtins (2026-11-04)

What the owner asked: the scripted animations (mp_frag's crane, iron gate and chain, warehouse door lock, hopper wheels) play and draw, the rest of the common scripts' builtins do what the engine does, doors feed the bots' graph, and the paused scopes work is landed as far as it can be offline. Nothing here has been seen on a GPU; the pictures are lavapipe.

### Animations (READ the data, INFERRED the notifies)

- **Decode.** `fastfile_iw6::anim` reads every `XAnimParts` of the map zone with the IW-family decoder (`xmodel_runtime::AnimClip::from_parts`) into `iw6_pose::Clip` (frames, rate, looping, per-bone rotation and translation keys, notetracks) and every multi-bone `XModel` into `iw6_pose::Skeleton` (root bones first, parent = `i - parent_list[i - roots]`, parent-local bind pose and model-space bind). mp_frag: 18 animations (none refused), 26 skeletons. `iw6_pose` is a dependency-free crate (nlerp between keys, the compose/invert maths, `pose`, `skin`).
- **Play.** `sim::t6::iw6_script::anim::Animator` holds what plays on which entity. `scriptmodelplayanim(name)` / `useanimtree` start the clip at the level time (READ: the handler looks the name up and stores it with the start time; the clients play it); a name the zone does not hold changes nothing; `scriptmodelclearanim` (0x826f, named LIKELY from its handler: clears the entity's animation) stops it. Length is frames over rate; a looping clip repeats (its `looping` flag), a plain one holds its last frame. The notetracks the clip carries (`end` at 1.0 in every mp_frag clip) are delivered to the entity as notifies when the time passes them (INFERRED: the engine's own delivery was not read, the map scripts do not wait for any). `Iw6Host::anim` is in both the sim and `gsc_iw6_host` builds.
- **Bone links.** `linkto(parent, tag)` with no offsets keeps the child's current pose relative to the tag (READ: the handler stores the child's pose in the tag's frame); with offsets it uses them. A child linked to a bone of an animated model follows the bone's posed matrix (`BoneLink`; `movers::follow_links` asks `iw6_script::linked_pose` first). This is what carries the gate's two halves and the warehouse door's lock on the animated carrier.
- **Draw.** The model techniques have no skinning, so a skinned script model is drawn in parts: `render_iw6::skin` groups the triangles by bone (blend vertices' bone offsets and rigid lists' offsets are bone index times 64), one renderer object per bone, each placed by `entity * skin[bone]` (`Host::set_mover_skinned`; `Iw6MoverPose::skin` carries `pose * bind^-1` per bone from the sim through `asset_world`, applied in `render_gpu::iw6_native`). A triangle with several bones takes its heaviest one (the doors, gate and crane are rigid parts). `BO2MP_IW6_SKIN=0` draws the bind pose.
- **Verified (headless).** `cargo test -p gsc_iw6_host mp_frag_script_animations_play_to_their_end`: the iron gate's carrier plays `mp_frag_metal_door_open` (59 frames at 30, so 1.97 s) and its `end` notify arrives at the data's time; the pose at t=0 and t=end differ as the data says; the two gate halves linked to the carrier stand where they were while shut and move when it is open; the crane sways; a clip that does not exist changes nothing. `cargo test -p fastfile_iw6 mp_frags_animations_decode_and_say_what_the_data_says`, `cargo test -p render_iw6 mp_frags_skinned_models_split_into_their_bones`. A picture: `IW6_SHOT_ANIM="paris_catacombs_iron_gate_left,mp_frag_metal_door_open,0"` and `...,end` in `render_iw6/examples/iw6_shot` pose the placed model at those times (see the shots in the commit notes).
- **Not done.** A notify at a time other than the clip's authored notetracks; the delta's rotation parts (`quat`, `quat2`) are walked but not decoded; see 'Animation pass 2' for the rest.

### Builtin pass 3 (2026-11-05)

Every builtin id the mp_frag scripts call is now taken by the host (test `the_unimplemented_ids_of_the_mp_frag_census_are_all_taken`). Sources: `sim::t6::iw6_script::builtins` (`more_function` / `more_method`), `host::Stores` (the state they keep), `census::{NOOP,APPROX}`, names in `crates/gsc_iw6/data/builtins_iw6.names`.

- **Kept state.** The player-data banks (ranked, private, coop, common, player, cac, and the reserved-int forms) keep what a script sets; match data and client match data are one global bank each; team scores (axis/allies only), spectate permissions, `notifyonplayercommand` pairs, contents (the setter answers the old value), move-speed scale, stance, camera points (index under 4), client dvars (an odd count is an error).
- **Computed.** `positionwouldtelefrag` (box 15 across, 35 up and down around origin+35z), `pointonsegmentnearesttopoint`, `getspawnarray`, `isitemunlocked` (2 for a name with no row in the unlock table, else 1), `getxuid`.
- **Stand-ins.** `setplayerangles` writes the entity angles; `getspectatingplayer` answers undefined; `setblurforplayer` is a no-op; `getoffhandsecondaryclass` answers "flash". Ids whose handler gave no meaning are `unk_XXXX` no-ops. 0x8291 answers 1, 0x83fe and 0x0253 answer 0: meaning unknown.
- **Scopes.** `render_iw6::scope` gained `can_hold_breath` and `hold_breath_step`, INFERRED (the exe's timing code was not found; only the dvars `player_breath_snd_lerp` 2 / `player_breath_snd_delay` 1 and the perks `specialty_holdbreath`, `specialty_improvedholdbreath`, `specialty_holdbreathwhileads` are read). No scope-drift consumer was found.
- **Tools.** `gsc_iw6_host/examples/iw6_script_dump.rs` (disassembles every script of a map's zones into a folder), `fastfile_iw6/examples/iw6_stringtable_probe.rs` (prints a string table by name).

### Doors and the bots' graph

`connectpaths` / `disconnectpaths` (0x8056/0x8057) are methods on a brush entity: they relink / unlink the path nodes inside its volume (READ). The host now turns them into `Effect::Paths`; the sim takes the entity's brush-model bounds (`Iw6ScriptPack::brush_bounds`, from the map's `GfxBrushModel` bounds, about the model's origin) at the pose it has when called, turned by its angles, and publishes the gates (`asset_world::set_iw6_path_gates`). `bots::NavGraph::set_gate(key, mins, maxs, blocked)` holds every link whose walk (node centre to node centre, the hull's width and height added) crosses the volume while the gate is closed and gives it back when it opens; the graph keeps `Gates.full_adj` and recomputes components; its generation moves so walking bots re-plan. The plugin applies the list each tick (`sync_gates`); a restarted match boots the scripts again with no gates and the old ones open. Doors that never call `disconnectpaths` stay open to the graph, as in the engine.

mp_frag: at load the scripts shut 8 brush doors (and one model carrier) and give two back after the door switch runs. Tests: `cargo test -p bots --lib gate` (a corridor: a closed door cuts the route, an opened one restores it, two doors on one link both must open); `cargo test -p bots --test ghosts_nodes mp_frag_doors_close_and_open_routes` (the real graph: brushes `*51` and `*58` hold 12 links shut and give them all back); `cargo test -p gsc_iw6_host mp_frag_events` (the scripts really emit 8 or more disconnects and 2 or more connects).

### Builtins

Census of mp_frag (`docs/exe_notes/IW6_SCRIPT_CENSUS.md`, regenerated): 327 ids called; functions 50 implemented, 22 approximate, 22 no-op, 47 unimplemented; methods 38 implemented, 15 approximate, 40 no-op, 93 unimplemented (before: 48 / 128 unimplemented). New:

- Implemented: `connectpaths`, `disconnectpaths`, `useanimtree`, `scriptmodelplayanim`, `scriptmodelclearanim` (above); `game_ended` (the level notify when the game script sets the match flag).
- Approximate (a store or a sane value): `setrankedplayerdata` / `getrankedplayerdata`, `setprivateplayerdata` / `getprivateplayerdata` (a private store by player and path; an unset path reads 0), `buttonpressed` (false), `hasweapon` (false), `getstance` ("stand"), `getplayerangles` (the entity's angles), `getweaponslistall` (empty array), `getweaponammoclip` / `getweaponammostock` (0), `getguid` (the client number as 16 hex digits).
- No-ops (player-side, nothing for the sim): `setclientdvar`, `setclientomnvar` (0x82f1, named LIKELY from its handler's argument shape), `setdepthoffield`, `setoffhandprimaryclass`, `setoffhandsecondaryclass`, `setweaponammoclip`, `setweaponammostock`, `shellshock`, `givemaxammo`, `openpopupmenu`, `closepopupmenu`, `takeweapon`, `giveweapon`, `devaddpitch`, `devaddroll`, `devaddyaw`, `setperk`, `unsetperk`, `clearperks`, `iprintln`, `setweaponhudiconoverrides`, `setspawnweapon`, `suicide`, `playsoundtoplayers`.
- **Door hints are localized.** The English zones (`zone/english/eng_<map>.ff`, `eng_common_mp.ff`, `eng_common_core_mp.ff`, `eng_patch_common_mp.ff`) are walked for their `LocalizeEntry` assets (`Capture::localized`, 87 + 402 strings on mp_frag's set), and `sethintstring("MP_FRAG_OPERATE_DOOR")` shows "Hold [F] to Operate Door" (colour codes dropped, `{+activate}` as Black Ops II's use key). Probe: `cargo run -p fastfile_iw6 --example iw6_localize_probe -- <eng zone> [word]`.
- **Still unimplemented** (listed with call counts in the census; most are unnamed because the handler tables name no string): the 93 methods and 47 functions it shows, led by 0x823e (46 calls), 0x8269 (39; reads a client GUID, a `getxuid`-like), 0x0122 (37) and 0x0129 (29), which have no handler in the table and so cannot be read; 0x823f, 0x0253, 0x83fd/0x83fe (GUID / vector helpers), 0x821f, 0x821a (reads `mp/unlocktable.csv`: an unlock query). The VM pushes `undefined` and goes on for each; nothing in mp_frag's boot or its door, gate, ladder and toy scripts is turned away.

### Scopes (the paused `iw6-scopes` branch)

The branch held one commit: a split of the character grading (players, gun and effects drawn into the HDR scene between `record_scene` and `record_fog`, `BO2MP_IW6_CHAR_GRADE`) and a probe. Main has since carried that grading work in a later form (the staged frame, `Host::record_scene` / `record_fog` / `record_post`), so every conflicting hunk was resolved to main's version; what the branch added alone, `fastfile_iw6/examples/iw6_attach_probe.rs` (scope and sight attachments, reticle models, flags, zoom), is kept. The scope rules of `docs/exe_notes/IW6_SCOPES.md` that need no GPU are now code with tests (`render_iw6::scope`): `cg_fov` and its clamps, the ADS zoom fov (variable zoom 25/15/8), the ADS fraction (the zoom in the last `zoomInFrac` of the aim progress, attachment scales, a scale at or under 0 as 1), hybrid scope states 0..5 and their fovs, when the HUD overlay is active (picture, or reticle with no lens view, alpha over 0.01) and drawn (aim at 0.99, weapon not busy), the overlay's centre from the aim sway, the lens (picture-in-picture) activation (on above 0.4 going in, held to 0.7 coming out, blur 14t/3, zoom 8t+1), its 0.05 s blend ramp, the reticle index (the packed weapon id's top six bits, the three extension banks of 16) and the red-dot bones per state. **Not done, and why:** (the lens shader, `SCOPE_SAMPLE_LIMITS`' consumer and the reticle table's columns were read afterwards: 'Animation pass 2 and scope decode'); the second 512x512 scene and the viewmodel's scope parts (a second draw of the world and a second set of viewmodel objects in the frame) are renderer work that cannot be checked without the owner's GPU, and `render_iw6` has no gun or view-model path of its own to put them in; hold-breath and scope drift are not read. The pure rules are ready for that wiring.

## Effect lights (2026-11-05, branch iw6-fx2)

Ghosts' omni (12) and spot (13) light elements now reach the native frame. Chain: the lane keeps type 13 (`asset_game::s1_elem_type`, was dropped); the draw dispatch's light callback reads a Ghosts element by its own rule (`fx_iw4::iw6::light_state`: radius = first size channel, colour = rgb x the scale channel, a zero scale taking the colour's HDR scale: INFERRED; exe note: omni handler origin, radius size0, colour x scale, sphere cull); `render_fx::iw6_lights::publish` picks the nearest in radii under the preset's cap into a fixed list (`asset_world::set_iw6_fx_lights_with`, no allocation); `iw6_native` copies it to `Host::set_fx_lights`; `Host::record_fog` runs `render_iw6::fx_light::Pass` after the FLOATZ copy and before the fog: a full-screen pass that rebuilds position and normal from FLOATZ and multiplies the scene by `1 + gain x sum(colour x (1 - d/r)^2 x N.L x cone)`. Not recorded when there are no lights.
Gating: `Quality::fx_lights` (Ultra 8, High 6, Medium 3, Low 0; 0 when REAL-TIME LIGHTING is off); `BO2MP_IW6_FX_LIGHTS=0` (off; also drops type 13 again), `BO2MP_IW6_FX_LIGHT_GAIN` (default 0.25, a guess).
INFERRED / unverified: the multiply stands in for albedo; the falloff and spot cone (Black Ops II's default cone) are not Ghosts' own; depth band handling for the view-model band. Needs the owner's GPU and a fire/lamp scene.

### Not done, and why
- Worker-thread simulation: Ghosts effects run in the shared `FxSystemHost` (Bevy main schedule); the native frame is drawn from the render sub-app, so with pipelined rendering the render thread already only draws. A BO3-style `SimWorker` would mean moving the shared host (also BO2's gameplay) off the main thread; not attempted.
- Decals (15): need the world-mark path on Ghosts geometry (a projected quad against the BSP draw surfaces); not attempted. Distortion needs a scene-copy pass (a GPU change that cannot be checked here). Vector field (17) draws nothing by itself (it feeds GPU particle sims). Runner (16) maps to the catalog runner already, unverified.


## Flares and model elements (2026-11-05, phase 2, branch iw6-fx2)
The Ghosts exe's decompile exists as text (`/mnt/user-data/uploads/ghosts_game_files/iw6_decompile/complete/iw6mp64_ship_all.c`); no Ghidra is installed, so it was read from there (static only).
**Flares (type 4)**: the walker now keeps the flare def and its four gradient tables (`FxFlareRec`), carried as `Iw6ElemSource.flare` and parsed by `asset_game::Iw6FlareDef`. `render_fx::iw6_flare::place` translates the screen-space handler (centre from NDC x position, world-sized corners over depth x projection, gradient tables via the exe's `table()` lookup: count+1 floats, index clamp, results clamped 0..1) and draws it as a camera-facing billboard at the real depth. `Iw6FxView` (asset_world) is published each frame by the effect system (and by the `iw6_fx_shot` harness). Switch: `BO2MP_IW6_FX_FLARE=0`. Harness: mp_frag `vfx_sunflare_frag` draws 22 sprites. INFERRED: rotation angle arguments, screen radius, source-cos axis. Not done: the exe's occlusion fade.
**Models (11)**: Ghosts xmodels are linked into the effect model catalog (`assets::lane::iw6_fx`, `FxModelCatalog::insert_skel`, absorbed into `combat.fx_models`), so the existing model element path draws them. Switch: `BO2MP_IW6_FX_MODELS=0`. Not seen on a GPU.
`IW6_SHOT_ELEMS=1` makes the harness print each element's raw type, flare bytes and visuals.


## Animation pass 2 and scope decode (2026-11-05, branch iw6-script3)

### Animation
- **Root motion (READ data, INFERRED use).** The walker kept the delta translation's stream positions (`walk::DeltaTransAt`); `fastfile_iw6::anim::delta_trans` rebuilds the decoder's `RawDeltaTrans` (single frame of 3 floats when `size` is 0; else mins, step, `size+1` frame numbers (bytes under 256 frames, else u16) and packed frames, 3 bytes when small else 3 u16) and `clip()` samples `AnimClip::abs_delta_trans` at every frame into `iw6_pose::Clip::delta`. Real data: common_mp 583 of 1418 clips, common_core_mp 79 of 2441, mp_frag none (test `root_motion_in_the_installed_zones_decodes_to_finite_values`). The shift is `Clip::root_offset` (relative to frame 0; a looping clip adds its whole displacement per finished cycle; INFERRED, a plain one holds its end), applied in the entity's own frame (forward, left, up) by `Animator::root_shift` to the published pose origin and to the tags children link to. A new clip banks the old clip's offset so the model does not jump back. `BO2MP_IW6_ROOT_MOTION=0` turns it off. Not done: the delta's rotations.
- **Blending (INFERRED).** `scriptmodelplayanim` over a playing clip keeps the old clip running under the new one for `BLEND_MS_DEFAULT` (200 ms, `BO2MP_IW6_ANIM_BLEND_MS`, 0 = the old cut): `iw6_pose::pose_blend` blends each bone's parent-local pose (nlerp rotations, linear translations, normalised weights). The handlers read give the engine no blend time of their own, so the number is a stand-in. Restarting the same clip does not fade.
- **Any class (READ filter, INFERRED need).** `publishable_class`: `script_model` and `script_brushmodel` as before; any other class when an animation plays on it and it has a model. `BO2MP_IW6_ANIM_ANY_CLASS=0` restores the two-class filter.
- Tests: `cargo test -p iw6_pose`, `cargo test -p sim --lib iw6_script::anim`, `cargo test -p fastfile_iw6 --lib anim::`.

### Scopes
Details and the shader decode are in `docs/exe_notes/IW6_SCOPES.md` ("Lens shader", "Reticle table", "Thermal"). In `render_iw6::scope` (tested pure code): `sample_limits` (the half-texel inset packing), `lens_sample_uv` (the lens pixel shader's sampling), `thermal_constants`, `thermal_blur`, `parse_reticle_table` / `reticle_unlock_bit` / `reticle_unlocked`, and `set_idle_constants`, which `Host::code` calls so a pass that reads the scope, distortion or thermal constants finds a value (`BO2MP_IW6_SCOPE_CONSTS=0` leaves them unset). Tool: `render_iw6/examples/iw6_scope_shaders.rs` (lists techniques that read the scope / thermal / PiP inputs, `DUMP_PS=<shader>` prints a pixel shader as WGSL, `SLOT=<n>`, `SLOTS=1`, `NAMES=<word>`). Not done: the second 512x512 scene and the view-model scope parts (GPU work); the thermal technique shaders are not in the install, so the thermal draw cannot be decoded or checked.

## Sky audit (2026-11-05)

What each map's sky is, from the map's own data (`examples/iw6_sky.rs` prints it; `examples/iw6_sky_notes.rs`
loads the scene and prints the `iw6 sky:` notes). Only **mp_frag** is staged in `ghosts_game_files/zone`
(no other Ghosts map zone, no other map's `techsets_`/`patch_` zones), so it is the only row that is
measured; the other maps cannot be read until their zones are provided.

| Map | Sky picture (from the map) | Resolved? | Fallback when the pak is missing |
|---|---|---|---|
| mp_frag | `sky_mp_frag_hdr`, cube, BC6H (DXGI 95), 1024 x 1024, one part, `imagefile60.pak` (3.4 GB, not staged); sky material `wc/sky_mp_frag`, technique set `wc_sky_bl`, its only texture is that picture | no (pak 60 not staged) | procedural cube from the map's fog colour (mp_frag_hdr: horizon [0.60, 0.75, 1.0], a light-blue day haze), 0.7 overhead, a halo toward the sun at elevation 30 degrees; intensity 1 |
| every other Ghosts map | not readable here: needs the map's own zone. By the one example the name is probably `sky_<map>_hdr` (INFERRED from mp_frag; the real name is whatever the zone's `GfxSky` says) | unknown | same procedural stand-in from the map's own fog and sun, with the log line `iw6 sky: <picture> NOT RESOLVED (map <map> needs pak <imagefileN.pak>)` |

**What was wrong, and fixed**

1. *The wrong surface was the sky.* `GfxSky::skyStartSurfs` holds positions in the dpvs sorted surface index
   (`sortedSurfIndex`), not surface numbers. Read as surface numbers, mp_frag's sky list named surface 2647,
   a cargo container door (`wc/ag_cargocontainer_door_orange`), which was moved to the sky list (drawn first,
   without a depth test), while the real sky surface 15498 (`wc/sky_mp_frag`, bounds about +-30000 x 24500, in
   the lit opaque range) was drawn as ordinary opaque geometry. `fastfile_iw6::map::sky_surface_of` maps
   through the sorted index (sorted[2647] = 15498; tested on a synthetic index, checked on mp_frag's data).
2. *The stand-in was a white sky.* It was the map's reflection probe 0, which is a flat 1.0 (`docs/IW6.md`,
   "Reflection probes and the sky"), so any map whose sky pak is missing drew a white sky (a night map as
   well). It is now `render_iw6::sky::stand_in_cube`: 32 px RGBA16F faces whose horizon is the createart fog
   colour (far geometry fogs to it, so sky and world meet), darker (x0.7, INFERRED) overhead and below, and a
   halo toward the map's sun scaled by its brightness (a dim night sun adds none). A map without a fog block
   gets a neutral dim sky (`NEUTRAL_HORIZON`) and the note says `NO FOG BLOCK`. Intensity is 1: the fog colour is
   already in scene units, so the map's `r_hdrSkyIntensity` (3 on mp_frag) would triple it.
3. *The picture is the material's.* The sky is picked from the pictures the sky surfaces' materials sample
   (cube textures), falling back to the `GfxSky` picture; a disagreement between the two is logged. (On mp_frag
   both are `sky_mp_frag_hdr`.) The stand-in is registered under every name that cannot be read, not the
   `GfxSky` name alone, and a part that reads while another pak is missing is read at the reduced size and
   logged as such. Unused part slots no longer name `imagefile0.pak`.
4. *Honest logging.* `iw6 sky: <picture> NOT RESOLVED (map <map> needs pak imagefile<N>.pak)` then one
   `iw6 sky: stand-in ...` line with the horizon colour and where it came from.

**Checked and found consistent (mp_frag)**

- Sun and sky: the worldspawn `sundirection` is "-30 36 0" (pitch -30, yaw 36); the map's sun vector toward the
  sun is (0.70, 0.51, 0.50), 30 degrees up, and `LIGHT_POSITION`, the sun-fog direction and the stand-in's halo all
  use it. The real sky's own sun cannot be compared without pak 60.
- Orientation: the sky shader takes a camera-relative world direction (z up) and the cube is sampled like the
  probes (D3D face order, raw axes). The worldspawn holds no key that reads as a sky rotation (its keys are
  `sunlight`, `suncolor`, `sundirection`, `_color` and numbered `iw6_<n>` ones whose names are not known);
  nothing is rotated. `sky::face_dir` is the stand-in's face convention and is tested against the probe notes.
- State: the sky list draws first and tests no depth (sort), through the material's own technique (`wc_sky_bl`).
- Exposure: the HDR sky intensity constant is `r_hdrSkyIntensity` times `r_hdrSkyColorTint` (3 on mp_frag) for
  the real picture, 1 for the stand-in. Sky fog uses the createart block's `skyfogminangle` / `skyfogmaxangle` /
  `skyfogintensity` (`FogConsts::height`), unchanged.

**What the owner must provide to see the true sky**

- mp_frag: `imagefile60.pak` beside `mp_frag.ff` (it is over the 400 MB staging limit; on the owner's PC it is in
  the game's `zone` folder).
- Any other Ghosts map: that map's zones (`<map>.ff`, `techsets_<map>.ff`, `patch_<map>.ff`, `<map>_load.ff`)
  and the paks its sky picture names; the load log says which (`needs pak imagefileN.pak`). Until then the
  stand-in is built from that map's own fog and sun.
- Screenshots of mp_frag on a GPU with and without pak 60, to check the stand-in's horizon and zenith against the
  real sky (nothing here was seen on a GPU).


## Quality change (2026-11-05, branch iw6-fix3)

Bug: switching the graphics preset inside one process (`Host::apply_quality`, also the in-game option) panicked in wgpu at `finish` with a dynamic-offset overrun on `iw6_cb_inst` (offset 7104, buffer 12576, binding 6144). Root cause: the per-object constant buffer of a plan (`CbPlan::per_object`) is made `objects x stride x views` bytes plus `MAX_INSTANCES x stride` of padding (an instanced run binds up to 64 blocks from any object's offset). It was re-made only when `size < need`, with `need` lacking the padding, so a plan that gained fewer than 64 objects when the lists were made again (a new list with another shadow/light flag adds placements) kept the old buffer and its mirror: the new objects' blocks lay beyond the data written, and the instanced binding ran past the end. Fix: the check is `size < need + pad` (or a short mirror), and when a plan's buffer is re-made, the other prepared lists that hold its bind groups are dropped (they pointed at the old, no longer written buffer). Regression: `cargo test -p render_iw6 --test quality_cycle` loads mp_frag (`IW6_TEST_MAP` overrides, skipped when absent) on the software adapter and cycles low, medium, high, ultra, high, medium, low, ultra, low drawing two frames each (it failed before the fix, passes after).


## Decals (2026-11-05, branch iw6-fix3)

Ghosts decal elements (type 15; the impact-mark effects of bullets and explosions are the same element, so bullet holes take this path too: not checked in the exe) now draw on the Ghosts world. Chain: `fx::spawn` queues a `PendingDecalSpawn` as before; `render_fx::present::record_spawn_decal`, when there is no Black Ops II world and a native Ghosts frame is active (`asset_world::iw6_decals_active`), hands the spawn to `asset_world::push_iw6_decal` instead of the BO2 mark path (origin, axes from `fx_iw4::impact_mark_axis`, radius = first size channel, native colour, life = the element's life span, material = the visual's second material name, else the first: `OwnedFxVisual::mark_hint`, since a Ghosts material is not bound in the BO2 catalog). `iw6_native` takes the queue each frame into a stack array and calls `Host::add_decal`.
`render_iw6::decal::project` clips the mark's box (radius along the three axes, `marks_iw4::mark_fragment_clip_planes`) against the triangles of every world draw that reaches it (opaque and decal lists, no movers or models) with the engine's mark clipper (`mark_clip_world_triangle_points`, `mark_emit_brush_fragment`), taking lightmap coordinates and normals from the surface, and packs the vertices with `pack_mark_world_vertex` (the normal and tangent words rewritten in Ghosts' 10:10:10 form). Surfaces of a lighting other than the first reached (lightmap page, probe, sun / light flags) are left out.
Drawing: a ring (48 slots of 32 vertices and 24 triangles) is a dynamic world mesh and one `List::Decal` draw (`Renderer::add_dynamic_draw`) per (material, surface lighting), at most 32 rings; the material is found in any loaded zone (`Scene::material_named`). A mark takes the next slot (the oldest goes); unused triangles are degenerate. A ring's first mark remakes the draw lists once (a hitch for each new material / lighting pair; none after). A mark with a life fades over its last second in 8 alpha steps (the slot's vertices rewritten per step) and is then removed. Nothing is allocated per frame.
Tested: the clipper on a synthetic floor (winding, facing, reach), the fade steps, the vector packing; `tests/decal_ring.rs` makes a ring from the map's own decal material on mp_frag, remakes the lists and draws through the fade and removal on the software adapter. INFERRED: the IW4 mark clipper, the vertex layout and the facing test for Ghosts; the decal materials' techniques build as a world surface's do. Not seen on a GPU: needs the owner's screenshots (fire a weapon at a wall).

## Distortion (2026-11-05, branch iw6-fix3)

Ghosts' distortion effect elements (techsets `distortion_scale` and `distortion_shade`, slots 3 to 6) used to be dropped by the effect lane. Now `render_fx` builds a camera-facing (or oriented) quad per visible distortion element into a fixed list (`asset_world::iw6_distortion_into`, 32 at most) instead of a sprite; `iw6_native` hands it to `Host::set_distortion`, which draws the quads as a dynamic mesh with the effect's own material (`render_iw6::distort`), and `record_fog` copies the fogged scene colour into the `RESOLVED_POST_SUN` code texture just before the blended surfaces, so the pixel shader refracts the scene behind it. The copy only happens in frames with at least one quad, and the texture is created once. `world::pick` falls back to slot 3 for a distortion techset. Switch: `BO2MP_IW6_DISTORTION=0`.

INFERRED, not seen on a GPU: `RESOLVED_POST_SUN` being the fogged scene before the blended surfaces, the quad vertex contents (normal toward the viewer, tangent along the right axis) and the material being blended. The first quad of a new material remakes the draw lists once (a hitch).

## Players and gun in the sun shadow (2026-11-05, branch iw6-fix3)

The players and the view gun are drawn by the game's own character pass, which cannot write into Ghosts' sun shadow atlas, so they cast as proxies (`render_iw6::caster`): one capsule each (a segment and a radius), drawn depth-only into both cascades after the world's, with a small depth bias. The frontend lists the remote bodies (feet origin, 70 tall, radius 15) and the view gun (24 long along its forward axis, radius 3.5) in `merge_xmodel_draw_plan` (`asset_world::Iw6SunCasters`, 16 at most, fixed arrays); `iw6_native` adds the local player's capsule under the eye (eye height 60) and calls `Host::set_sun_casters`. The shadow is the silhouette of a capsule, not of the model, and stances (crouch, prone) are not told apart: an approximation, and the sizes and the gun's pose are INFERRED. Not seen on a GPU.

The world's cascades are cached across frames, so with proxies in the frame they are drawn into a second depth texture (the "static" one, made when the first proxy arrives) and each frame that texture is copied over the live atlas the shaders sample, then the capsules are drawn into the live one. That costs one atlas-sized depth copy per frame (2R squared texels, 4 bytes each) and a capsule mesh of a few hundred triangles per cascade; nothing is allocated per frame (the mesh, the instance buffer and the constants are made once). A change in whether proxies exist redraws both cascades once. It follows the preset: no sun shadow map, no proxies. Switch: `BO2MP_IW6_SUN_CASTERS=0`. Tests: capsule mesh and placement (lib), the pass on a software device (`tests/caster_pass.rs`), and `tests/decal_ring.rs` cycles proxies on and off on mp_frag.

## Performance round 6: the frame audit (2026-11-05, branch iw6-fps5)

Measured with the `play` profile (opt 3), the noop backend for CPU counts (`IW6_SHOT_ALLOC=1`, a counting global allocator in `examples/iw6_shot.rs`; `IW6_SHOT_ALLOC_TRACE=1` prints the last frame's allocation sites, `IW6_SHOT_ALLOC_DEPTH=n` frames per site) and lavapipe for pixels (byte compare of the standard 13 views at 1280x720). The VM has two noisy cores, so no ms are claimed for CPU steps below the noise (record 6-10 ms before and after); counts are exact.

**Per-frame allocations (record, view 0, 720p, steady state): 4027 -> 199** (finish 234 -> 225); picture byte-identical to the base. What went:
- code constants: name lookup is a hash map (`BO2MP_IW6_CODE_SCAN=1`: the old scan), `set_rows` reuses the allocation, `CodeValues` are pooled (`Host::code_pool`) and copied with `clone_from`/`overlay_from`, frustum planes are pooled, `missing_code` is rebuilt only when the set changes;
- the per-update light values are built once per light and overlaid (`lit_cache`, `BO2MP_IW6_LIGHT_CACHE=0` restores the rebuild);
- post passes (`post.rs`): pipelines' bindings resolved once, bind groups cached by view identity, constants written only when changed, texture inputs are slices; `FloatZ` and the native depth view cache their bind group / view;
- env switches are read once (`render_iw6::switch::{Switch,Number}`, `refresh()` for the harness);
- the software occlusion buffer is rebuilt in place (`native_cull::Occlusion::rebuild_from_set`), the `Arc` reused.
The 199 left are wgpu's own (pass creation; the pass command vector doubling, about 3.7 MB a frame inside wgpu-core), not ours.

**Skipped passes and cheaper passes**
- Effect lights: the full-screen pass is scissored to the union of the lights' projected sphere bounds (`fx_light::Uniforms::reach`, +-1 px; none in front of the eye = pass skipped; eye inside a sphere = full). `BO2MP_IW6_FX_LIGHT_SCISSOR=0` restores. Unit-tested (a sphere ahead, one behind); not pixel-compared (no lit effect in the standard shots).
- Motion blur: skipped when view-projection and eye equal the previous frame's (`ScreenFx::skip_still`; `BO2MP_IW6_MBLUR_STILL=1` draws it anyway). With no motion the pass is the identity.
- Players/gun sun shadow proxies: the atlas copy, the upload and both capsule passes are skipped when both cascades are held and the proxy list is bit-identical to last frame's (`BO2MP_IW6_CASTER_SKIP=0` restores). Measured with `IW6_SHOT_CASTER=1` (a proxy 150 units ahead): frame 1 "sun casters 75 ms" GPU on lavapipe, frame 2 4.4 ms (the previous frame's timestamp), frame 3 none; the picture is byte-identical with the skip on and off.
- Distortion: its scene copy already exists only in frames with a visible quad (checked, nothing to skip).
- The one-cascade clear pipeline is made at warm-up, not at the first frame that needs it.

**Decals**
- The first mark of a (material, surface lighting) pair remakes every draw list (a hitch). Rings cannot be made in advance because the key includes the surface's lightmap page, probe and light flags, which are only known when a bullet hits; making all combinations would be hundreds of rings. Left as it is (documented in `decal.rs`).
- New from the exe (docs/exe_notes/IW6_FRAME_BUDGETS.md): the near limit `fx_marks_nearlimit` (5: a new mark that finds 4 marks of the same surface within its radius removes the oldest) and the 512-mark pool. `BO2MP_IW6_DECAL_NEARLIMIT` (0 off), `BO2MP_IW6_DECAL_POOL` (0 off). Not seen on a GPU.

**Occlusion (item 3)**
The larger occluder set of round 5 does not reproduce its false hides on this tree: `IW6_OCC_CAP=40000 IW6_OCC_AREA=150 IW6_OCC_EXTENT=96`, and `100000/40/24`, against `BO2MP_IW6_OCCLUSION=0` at 720p on all 13 standard views (views 0-9 plus the interiors) are byte-identical, still and with `IW6_SHOT_MOVE=4` (turning four degrees a frame, ending on the shot's angle); the sole difference was 2 pixels of level 2 on view 4 with `100000/40/24`. The occluded draws on view 0: 1550 (old default) -> 3855 (new). The cause of the earlier 5% on view 5 was not found (it may have been fixed by the main changes merged since: draw order, mover handling); not found, not claimed. The default is now 40000 / 150 / 96 (`IW6_OCC_*` still override; the old set is `IW6_OCC_CAP=12000 IW6_OCC_AREA=400 IW6_OCC_EXTENT=48`). Doorway popping: the doors are movers, which are not occluders; a walking-through check needs a GPU and the owner.

**Not done**: the pipeline cache on disk (wgpu 29 `PipelineCache` is Vulkan-only and the harness backend here is lavapipe; needs the owner's GPU), the map-load profile and its derived-data cache (no time left in this pass), spot-tile world-space cache keys, hash-checked derived data. Exe findings and what the exe does that we lacked: docs/exe_notes/IW6_FRAME_BUDGETS.md.

## Performance round 7: inward versus outward (2026-11-05, branch iw6-fps5)

Owner report: on every Ghosts map the fps is awful looking toward the middle and smooth looking outward (RX 9070 XT; BO3 and MWR fine).

**Profile (`BO2MP_IW6_PROFILE=1`, or the shared `BO2MP_GPU_TIMING=1`; `=2` adds the old one-line form).** Every 5 s (`BO2MP_GPU_TIMING_EVERY`) the log holds the shared one-line `iw6 gpu: pass ms ... total ...; draws, tris, fx quads, lights, shadow draws` (`render_h1::gpu_timer::pass_line`) and a table, one row per pass: GPU ms from timestamp queries (when the device has `TIMESTAMP_QUERY` and `..._INSIDE_ENCODERS`; else `-` and a note), CPU ms, draws, calls, culled, ktris. Passes: setup, sun near / sun far (cascades), sun casters (players' proxies), spot (spot shadow tiles), occlusion, opaque (depth pre-pass and lit pass share one render pass, so one GPU row; the makeup line below splits the entries), floatz (depth copy), fx lights, ssao, fog, blended, host fx+chars (the time between the blended mark and `record_post`: everything the host draws in its own passes between the stages, i.e. characters and effect sprites, decals are in the opaque list), dof, mblur, bloom, iris, final, GPU total. Below it: `opaque list` (entries drawn, pre-pass entries and triangles, models, decals, sky, entries lit by a spot or omni light, occluded, too small, cut by portals), `figures` (fx lights, scene lights, spot tiles redrawn and kept) and `effect sprites` (built, in front of the eye and on screen, their summed screen coverage in screens as the overdraw estimate, the biggest quad's share of the screen; `asset_world::fx_stats_of`, tested). Nothing is invented: a pass the device cannot time shows `-`.

**What grows inward (measured offline, mp_frag, 1280x720, ULTRA, play profile, lavapipe; spawn eyes + 60, looking at the map's centre versus the opposite way, 11 views).** CPU counts and the GPU's software-rasterised pass times are exact for the work; they are not fps and not the RX 9070 XT's times.
- Scene opaque list: inward 11.2k-16.5k entries and 1.40-1.65M triangles per frame, outward 0.95-1.1k entries and 170-210k triangles (15x the entries, 8x the triangles). Of the 16.2k entries of view 0, 8.4k are distinct draws (a pre-pass entry and a lit entry each, 97% have a pre-pass); `IW6_DBG_CENSUS=1`: 4.8k are placed models (3.9k with a LOD band), 10k world surfaces; 5.8k entries are under 16 triangles, 7k under 64; most draws sit 1000-2000 units away.
- CPU of one frame (`record` + wgpu's `finish`, which encodes every command for the driver): inward 15-25 ms + 25-35 ms, outward 3 ms + 2.5 ms.
- GPU on lavapipe: the opaque pass inward 1250-2700 ms, outward 360-620 ms; every other pass is the same inward and outward (ssao about 110, fog about 95, bloom about 65, final about 43).
- The sun cascades do not grow: they are centred on the eye (view direction does not enter), cached while the eye stays on the same texel, and their casters are 245-320 draws (near) and 470-680 (far) either way. Spot shadow tiles: the chosen set depends on the view (`spot_shadow::choose`), a tile is redrawn only when its light changes (about 1000 draws and 100k triangles each).
- Effects are drawn by the host outside this crate: the table's `host fx+chars` row and the `effect sprites` line are what to read on the owner's PC. No effect-heavy view exists in the cloud box.
Conclusion: what separates the two views is the opaque scene pass (draw count, small draws, triangles), CPU-side first. The owner's one log (`iw6 gpu:` line plus table, inward then outward) says whether his frame is CPU-bound (CPU record ms and draws high, GPU total low) or GPU-bound.

**Ghosts' own rule.** `r_depthPrepass` (enum, default 0: the exe sets flags from it, `mode == 0` skips the optional pre-pass stages 0x3e-0x40 and `mode < 2` the twelve of mode 2; the 13 stages 0-12 are always drawn): the exe keeps a pre-pass for its main opaque stages unconditionally, as this renderer does. No change from it. Nothing else in the exe cuts distant small draws (no screen-size LOD, no distance cull; see `docs/exe_notes/IW6_FRAME_BUDGETS.md`).

**Done (each with a switch, picture byte-identical).**
- Occlusion buffer filled on threads (`BO2MP_IW6_OCC_THREADS`, default the cores up to 4, `1` = as before): each thread rasterises its share of the visible clusters into its own buffer, the buffers merge by the smallest depth per pixel, which is the buffer one thread makes (unit test `threads_make_the_same_buffer_as_one`; two views, 10 frames each turning 3 degrees a frame, 0 differing pixels). Occlusion CPU ms with the default occluder set while turning: 6.9 -> 4.2 on this box's 2 cores.
- The draw loop is one function over a render pass or a render bundle encoder (`BO2MP_IW6_BUNDLE_THREADS=n` records n runs as bundles on n threads and executes them in order): byte-identical (4 views), but on lavapipe `finish` did not get cheaper (wgpu replays a bundle's commands into the driver at `finish`), so it is off by default; kept for a GPU box to try.

**Tried and not shipped.**
- Larger occluder sets (`IW6_OCC_CAP=150000 IW6_OCC_AREA=20 IW6_OCC_EXTENT=12`; 100000/40/24): opaque entries on view 0 turning 3 degrees a frame 14.1k -> 4.7k (5.4k) and triangles 850k -> 360k, but the pictures are not identical to `BO2MP_IW6_OCCLUSION=0` (up to 189 pixels, 515 levels, on 5 of 8 inward views; 100000/40/24: 2 of 8): false hides, so the default stays 40000/150/96 and the false hides' cause (an occluder that is not solid from the side it is tested from) is open. The fill also costs 25-50 ms on 2 cores at 150000.
- Models without their own pre-pass entry (`BO2MP_IW6_MODEL_PREPASS=0`: the model's lit pass tests nearest-wins and writes depth; the world's pre-pass still hides what is behind a wall): about 15% of the entries (the models' pre-pass half), but 4,800 of 921,600 pixels differ on the first inward view (coplanar models resolve differently), so it is opt-in.

**Next (needs the owner's log).** If CPU-bound: fewer commands per entry (merging the pre-pass of models by instancing across LODs, a spatially grouped bundle cache that survives a still view), the occluder false hides. If GPU-bound: the opaque pass's cost per pixel (far cascade taps, probes) and the micro-draws.

**Map load and pipeline cache (items from this branch's earlier commit, unchanged here).** `BO2MP_IW6_LOAD_TIMES=1` prints the load stages (`loadtime.rs`), the map's zones are opened in parallel, shader modules are shared, and a pipeline cache is kept on disk where the device has Vulkan pipeline caches (`pipecache.rs`); lavapipe has none, so its saving is not measured here.


## Performance round 8: fewer commands per entry (2026-11-05, branch iw6-fps5)

Measured on mp_frag, 1280x720, `play` profile, wgpu noop backend for counts (`BO2MP_IW6_PROFILE=1`), lavapipe for pixels (one process per camera, 8 cameras: the 5 standard views plus the inward spawn views). CPU ms are noisy on this 2-core VM; counts are exact. `IW6_DBG_CENSUS=1` now also prints, for the pre-pass half and the lit half, the number of (pipeline, texture, sampler) states, distinct draw keys, repeats of a key, and the state changes along the draw order.

What the census found (inward view `731 -429 208 0 90`): the pre-pass half has only 47 (pipeline, texture, sampler) states but 4,200 distinct draw keys, so instancing (779 repeats) could never fold much; the cost was one `set_bind_group` with a dynamic offset per object (1,914 constant changes in the pre-pass order, 980 of them a different group id for the same buffers, 2,832 in the lit order). Meshes are two big buffers, so vertex/index binds were not the issue.

Done (each with a switch; all byte-identical on lavapipe on the 8 views):
- `BO2MP_IW6_G0_SHARE` (default on, 0 = as before): the constant bind group is shared by draws whose buffers are the same (pipeline, plan, and the material only when the pipeline reads material constants), not made per (material, lightmap, probe). Bind-group sets on the first inward view 5,304 -> 3,824.
- `BO2MP_IW6_BLOCKIDX` (default on): every entry with an instanced pipeline binds the whole object buffer once and takes its object block from the first instance (`instance_index`), so changing object is no bind-group set. Sets 3,824 -> 2,379, pipeline switches 804 -> 331.
- `BO2MP_IW6_MULTIDRAW` (default on; needs `INDIRECT_FIRST_INSTANCE`, else the same draws go direct with a first instance): runs of one state are written as indirect records (5 words, a per-frame buffer, region per pass, written with `queue.write_buffer` after the walk) and issued as one `multi_draw_indexed_indirect` (`MULTI_DRAW_INDIRECT` is not a wgpu 29 feature; plain multi-draw is always available, emulated where the API lacks it). Render bundles (`BO2MP_IW6_BUNDLE_THREADS`) loop `draw_indexed_indirect`.
- Bug found: `encode_draws` never issued the last pending run of a pass. Fixed (`BO2MP_IW6_LASTRUN=0` restores the old behaviour). It changes 68 pixels on the second standard view (`-540 632 356 10 213`), the one place a visible draw was lost; all other views are identical with it on or off.

| view | entries | calls before -> after | pipeline sets | bind-group sets |
|---|---|---|---|---|
| inward A | 11,750 | 6,624 -> 1,893 | 804 -> 331 | 5,304 -> 2,379 |
| inward B | 12,726 | 6,069 -> 2,166 | 821 -> 317 | 5,259 -> 2,714 |
| inward C | 10,218 | 5,107 -> 1,806 | 665 -> 307 | 4,190 -> 2,200 |
| inward D | 14,530 | 7,961 -> 2,677 | 1,021 -> 371 | 6,376 -> 3,110 |
| outward | 5,078 | 2,489 -> 537 | 357 -> 158 | 2,076 -> 728 |

CPU (min over steady frames, noisy): wgpu `finish` falls by about a third (4.3 -> 4.1, 4.6 -> 2.4, 6.2 -> 4.1 ms on three views); our `record` does not fall (6-10 ms), because the cull walk over about 70k entries (2.6 ms) and the per-entry loop dominate it. No GPU time is claimed.

Not done / checked and left:
- Depth pre-pass (item 2): re-read, the exe keeps the main opaque stages' pre-pass unconditionally (`r_depthPrepass` only gates the optional stages), so the 2 entries per draw stay; with indirect runs a pre-pass entry now costs a 20-byte record, not a call.
- Tiny-draw batching (item 2 of the coordinator): the 5.8k draws under 16 triangles are separate objects with their own block, so they cannot share one draw without a merged static mesh; with multi-draw they are records in a run. Not done.
- Instance-index indirection buffer: superseded by first-instance block indexing plus multi-draw (repeats of a key are separate records).
- LOD (item 4): `scene.rs` reads the model's own `XModelLodInfo.dist` per LOD for placed models (band lo..hi, the last LOD to the placement's `cullDist`, nothing past `cullDist`), scaled in `Renderer::record` by the quality LOD and draw distance; the exe has no `r_lodScale` and no screen-size LOD (the six `r_*LodDist` dvars default -1 = model's own), and the last-LOD rule stays INFERRED because the picking itself sits in the exe's Umbra/DPVS code that is not in the decompile. No change.
- Map load times and the pipeline cache: unchanged from the earlier commit (`BO2MP_IW6_LOAD_TIMES=1`, `pipecache.rs`); not measured further here.


## Post, scopes, runner, vector field, effect thread: the finish list (2026-11-05, branch iw6-render)

### Motion blur: Ghosts' velocity chain (done, not seen on a GPU)
`render_iw6::mblur` (`docs/exe_notes/IW6_POST.md` 5). The zones' own `postfx_mblurhq_velocity_pass0` (twice: x then y,
20-texel tiles), `_pass1` (3 x 3 neighbour maximum) and `postfx_mblurhq` run over a velocity picture we make: one
full-screen pass reads FLOATZ and last frame's camera (kept in `mblur::Hq`) and writes the engine's encoding (two 8-bit
channels, `sign(v) sqrt|v|`, UV units). Static geometry gets the same velocity the engine's `velocity_static` writes;
skinned models and the view model do not write velocity (the view model, nearer than 24 units, is held still). Four
passes plus the velocity pass cost five full-screen-class draws only on frames where the view moved; a still view draws
nothing. Targets are made once per size (velocity, two tile targets, neighbour, output), constants reuse the pooled
`CodeValues`, no per-frame allocation. Checked on lavapipe (mp_frag 640 x 360, HIGH, 4 degrees of yaw a frame): a still
view is byte-identical to the effect off; a turning view blurs along the motion and matches the old reprojection blur
in direction and extent. Counters, not fps: no measurement was possible without a
GPU. `BO2MP_IW6_MOTION_BLUR_MODE=old` restores the earlier blur; mp_snow at 720p did not complete on the 8 GB box (the
process is killed while drawing, as for other world renders there), so mp_frag was used.

### Depth of field (partly done)
Read: the other chains (`r_dof_hq` 1 / 2: ten quarter-size passes with mask, CoC blur mips and a final) and the generated
gaussian kernel (`IW6_POST.md` 6). Done: the kernel generator as pure code, `dof::gaussian_pairs` / `dof::normalise`,
with tests. Not done: wiring either, because (a) the hq chains need a three-row constant table and the `POSTFX_CONTROL*`
meanings that are in the exe's data, and (b) the engine's generated blur variants take the kernel as constants of a
generated shader, which the zones' blur techniques are not. The hq 0 chain (bloom's seven-tap blur, INFERRED) stays.

### Scope second scene and thermal (not done)
The pure code (`render_iw6::scope`: lens rect, activation, constants) is merged earlier; the 512 x 512 second scene needs a
second `record_scene` / `record_fog` of the world from the scope camera into its own targets (the host owns one set of
sized targets, the sun cascades and the light lists are per frame), the game side to supply the lens bone transform and
the dual-FOV state, and the view-model scope parts; none of that can be checked offline and the host change touches the
frame setup for every map, so it was left rather than shipped blind. Thermal: the thermal techniques are not in the
install (as before), so nothing can be drawn from data; constants are documented in `IW6_SCOPES.md`.

### Runner (type 16) and vector field (17): read (`IW6_FX_EXE.md`)
Runner: the exe's spawn handler was located and compared; our shared runner path (`FxChildEdge`, `pending_runners`)
already does the same selection, spin and spawn, so Ghosts runners play (the earlier "spawn only" note was wrong). Left:
a child bolted to its parent's tag, and the element-attached child effect (flag 0x80000000).
Vector field: the exe's design is a 256-slot instance table of trilinear 3-D vector grids sampled in the particle step
(velocity or acceleration fields), not what the task description assumed beyond that; the draw handler "Apex Turbulence"
belongs to NVIDIA APEX middleware. Not wired: the asset walker, a registry in the shared fx host and the force step in the
shared integrator are all outside `render_iw6`, change code BO2 effects run through, and cannot be verified without a
map that uses one (not checked which maps do). Documented precisely for the next pass.

### Effect simulation off the render thread (not done, why)
`render_t7::fx_draw::SimWorker` works because the T7 renderer owns its effect state (a private `Scene` handed to the
worker, results are sprite lists). Ghosts effects live in the shared `FxSystemHost`, a Bevy resource that gameplay
systems write in the same frame: spawns from sim events (impacts, shots, script `playfx`), `drain_spawn_runners`,
bolted effects reading entity transforms, collision against the sim's `SimWorld`, effect lights and sound hints, and the
draw pass reading the live slots for the same frame. Moving the tick to a worker needs either a double-buffered host (a
clone of every slot vector each tick, about the size of the 0x400-effect pool) or a message queue for every writer, plus
a deterministic hand-off that keeps BO2 gameplay (`clip_world`, `drain` order) unchanged. That is a gameplay-path change
that cannot be proven equal here, so it was not attempted; the measured cost to beat is the sim's ~0.9 ms per tick on
mp_frag (`IW6_FX_EXE.md`, debug build, inflated).
## Round 5 (iw6-fps5): load time, tiny draws, multi-map

- Load-time pipeline build: `make_pipeline` builds the instanced variant on a scoped thread beside the main one (`BO2MP_IW6_PIPE_THREADS`, `1` = serial, default `min(cores, 2)`). Measured on mp_frag with lavapipe at 640x360 (2 shared cores): renderer warm 15429 ms serial, 15122 ms with two threads (2%). Picture byte-identical. The time is lavapipe's shader compile (about 600 pipelines), which does not parallelise on two shared cores; on a real driver the Vulkan pipeline cache is the lever.
- `pipecache.rs`: now a pure `encode`/`decode` (FNV-1a checked), per-map cap 256 MB, directory cap 1 GB (`prune` removes the oldest `iw6_pipelines_*.bin` first), 2 unit tests. Lavapipe has no `PIPELINE_CACHE` feature, so no saving can be shown in the cloud.
- Load stages on mp_frag (lavapipe, 640x360): zone open 3.4-3.9 s (one zlib stream, not parallelisable without a format change), read_map 167 ms, scene assembled 287, scene built 831, Renderer::new 305, renderer warm about 15.1 s, post warm about 50.
- Tiny draws: NOT merged. Measured census, inward spawn view, 1280x720 (`IW6_DBG_CENSUS=1`): 11,364 opaque entries (5,472 pre-pass, 5,892 lit), 4,141 of them under 16 triangles, already issued as 2,547 calls (16,543 draws in 2,872 calls before culling) through run merging, block-indexed instancing and multi-draw-indirect. What is left are placements with their own world matrix and light probe/tile; merging them means baking transforms into a clustered static mesh, which changes per-object lighting inputs and cannot be shown pixel-identical without a GPU comparison of every standard view. Left open.
- Multi-map: nothing in render_iw6 / fastfile_iw6 / bots / sim host code names mp_frag outside tests (grep audit). `scene::tests::staged_maps_load_through_the_same_code` loads mp_frag and mp_snow through `Scene::load`, checks zones, entities, bounds, a vision per map and distinct looks; bots and fastfile_iw6 tests cover both maps' PathData, and the script boot test runs both.
