# Handover: Modern Warfare Remastered (h1) "native" work in the BO2 rebuild

For: the next coding agent (OpenCode, Opus). Written 30 Oct 2026, 17:00 UK time.
Repo: `sero7k/bo2-multiplayer-rebuild` on GitHub, branch `main`. Read this whole file before starting.

## 1. What the owner wants

The project is a Rust recreation of Black Ops II multiplayer that also loads maps, guns and effects from other Call of Duty games. For Modern Warfare Remastered (MWR, engine name **h1**) the owner wants the same treatment Black Ops III got (`docs/T7_NATIVE.md`):

> Read the game's own executable and make our game do exactly what the real game does (lighting, collision, map logic, effects and so on), so MWR maps run 1:1 like the real game.

Standing rules from the owner:
- **The exe's real behaviour becomes the default.** Keep an env switch only to bring back the old behaviour for comparison (e.g. `BO2MP_H1_LIGHT=fitted`).
- **Merge finished work into `main` without waiting for sign-off.** Push half-finished work too, with unfinished parts switched off by default so nothing breaks.
- He wants to see progress as it happens: post short updates after each step.
- Don't ask him permission questions while he is idle; take the most reasonable reading and say what you chose.

Repo rules (from `CLAUDE.md`, `AGENT.md` and the README):
- Every change gets a README "Recent changes" entry, tests where possible and a docs update.
- **No addresses, binary excerpts or decompiled listings go into the repo.** Translate the game's logic into our own Rust, and document it as formulas and function-level descriptions in `docs/H1_NATIVE.md`. Addresses belong only in private notes, like this file.
- Mark everything in the docs as **read** (seen in the exe) or **inferred** (a guess).
- Never pipe `cargo` output through `head`, because it kills the build. Use a separate `CARGO_TARGET_DIR` per agent, and set `CARGO_PROFILE_DEV_DEBUG=0`. Disk space is tight.

## 2. The executable and the game files

- **Exe:** `h1_mp64_zt.exe` (21 MB), the HorizonMW / h1-mod build. On the owner's PC it is at `C:\Users\Gaming PC\Documents\mwr_game_files\h1_mp64_zt.exe`.
  - It is **not protected**: `.text` entropy is 6.6, and nothing is encrypted or filled in by a loader.
  - Static analysis only. **Never run it.**
  - The other exes the owner sent (Ghosts `iw6mp64_ship.exe`, Advanced Warfare `s1_mp64_ship.exe`) **are** Arxan-encrypted. Their code is not to be unpacked or decrypted. For those games the lighting values come from each map's own zone data.
- **MWR zones:** `C:\Users\Gaming PC\Documents\mwr_game_files\zone\` holds about 1,900 `.ff` files.
  - `code_post_gfx_mp.ff` holds the post-processing shaders.
  - `techsets_*.ff` hold the technique sets.
  - The maps are `mp_shipment`, `mp_killhouse`, `mp_crash` and so on.
  - `common_mp.ff` is 161 MB. The patch and common zones hold each map's own scripts (not fetched yet).
  - `hmw-usermaps` holds user maps, for example `mp_courtyard_ss`.
- **Tools in the repo:**
  - `cargo run -p fastfile_h1 --release --example h1_tech -- <zone.ff> [filter]` lists every technique, its shaders and its **argument table with code-constant slot names**, which tells you which shader reads which engine constant.
  - Other examples: `h1_map`, `h1_slots` (technique slots, caster states, light-set shadow keys), `h1_scripts`, `h1_fx_census`, `h1_fx_check`.
  - The native MWR renderer is `crates/render_h1`; it runs MWR's own shaders converted DXBC to WGSL. The map lane is `crates/assets/src/lane/h1_map.rs`.
- **Ghidra:** version 11.4.2, headless: `analyzeHeadless <projdir> h1 -import h1.exe` the first time (takes about 30 minutes), then `-process h1.exe -noanalysis -scriptPath <scripts> -postScript Dec.java <out> cmd...`.
  - The helper scripts are attached separately as `ghidra_scripts.zip`:
    - `Dec.java` takes the commands `dec:ADDR`, `xr:ADDR`, `xrr:START,END`, `str:SUBSTR`, `imm:VALUE`, `fconst:FLOAT`, `callers:ADDR` and `bytes:ADDR,N`.
    - `FindDisp.java` finds functions using given displacements, and `DispAll.java` lists all 16-byte-aligned displacements per function.
    - `DecRange.java` decompiles a range of functions and keeps those that match regexes; use `;;` to separate patterns that must all match.
    - `Disasm.java` disassembles a raw range, and `IncDisp.java` finds INC targets.
  - Only one Ghidra process can open a project at a time; copy the project for parallel work.

## 3. Facts about the exe (addresses are for your private notes only)

- **Floats in decompiles appear as `DAT_` symbols:** 1.0 is `DAT_140810c0c`, 0.5 is `DAT_140810aac`, the sign-flip mask is `DAT_140810ac0`, 2.0 is `DAT_140811d2c`, and -2.0 is `DAT_14081313c`.
- **Dvars:** names are **hashed**. A dvar's value is at `dvar + 0x10`. Match a dvar to its name by default value and range, using `docs/mwr_engine/dvars_rust.md`.
- **Engine constants (CONST_SRC_CODE):** there are 422; the names and indices are in `docs/H1_NATIVE.md`.
  - The back-end source state is at `0x1525c23d0`.
  - The **constant array is at `0x1525c35d0`** (state + 0x1200), with slot = (address − array) / 16.
  - The **per-slot u16 version counters are at `0x1525c5170`** (state + 0x2da0).
- **Constants reach the array three ways:**
  1. a **per-view block of 350 vec4**, filled by the front end and copied in when a view starts (film, `HDR_STAGE_PARMS`, rim lights, previous-frame matrices);
  2. **per-draw writes by computed index** in the back end (lights, model lighting);
  3. a **set-constant command** (dispatcher handler 26 of 27, at `0x140621350`; the command holds the slot at +4 and four floats at +8), used only for `SCRIPT_PARMS` and `LIT2D_HUDBLOOD_COLOR`.
- **Known writer functions:**
  - fog: `FUN_140084520` (ATMOS_FOG_PARMS_4/6, FOG_SUN_DIR, in view space)
  - perceptual tone map: `FUN_140083a10`
  - post chain per view: `FUN_140628da0`, which calls `1400831e0`, `1400855a0`, `140083a10` and then the colour pass `140084a20`
  - exposure compute / filmic 30-float block: `FUN_140084d30`
  - sun flare / volume light: `FUN_1406279a0`
- **Vision files:**
  - Parser: `FUN_14023fa50`, `14023e6f0`, key table at `0x140fe89c0` (39 keys, `{name, offset, type}`).
  - Blend: `FUN_14023e810`, smoothstep `(3 − 2t)t²`.
  - Copied into the frame by `FUN_14023ebc0` (the scene definition `DAT_1429c87f0` holds the film at +0x134).
- **Script builtin table:** `{index, function}` rows starting at `0x140fecdf8` (378 rows).
- **The material name to global table** for post materials is at `0x140887600` (`{name, global}` pairs).

## 4. Done and on `main` (newest first)

Details for every item are in `docs/H1_NATIVE.md`.

| Commit | What |
|---|---|
| 19d1614 (main) | **Effects:** MWR's particle rules read from the exe and used for MWR sprites by default: random table, seeds, per-frame visual state, fades, lit colour (`render_fx::h1_rules`), plus a headless check `h1_fx_check`. Check the README entry for exactly what is covered. |
| 8d27d85 | `render_h1` `h1_slots` example (technique slots, caster states, light-set shadow keys), from the sun-shadow agent (see section 5). |
| 63356eb | **Map logic:** worldspawn first; class handling (path nodes, weapon pickups, structs make nothing, unknown classes kept); trigger shapes (radius, disk); `wait` ≤ 0 fires once; hurt touch (`dmg` 5 when 0, 50 ms or 1000 ms with flag 16, flags 1 and 32, `MOD_TRIGGER_HURT`); numbered key names (`dmg` 293, `health` 486 …). **Kill volumes now kill (10,000)**; before, they did 5. CTF/DD use MWR's neutral spawns. CoD4 start-script deletions applied. Switch: `BO2MP_H1_MAP_LOGIC=0`. Test: `fastfile_h1 --test h1_map_logic`. |
| 5b9a5d7, 3b4e17d, f3f586c | **Sky, stages, probes, model atlas:** `r_hdrSkyIntensity` is ignored by MWR. The sky is scaled by `HDR_STAGE_PARMS = (1000·s, 16, 0, 0)`, where s is the radiometric scale of the clip-map stage the eye is in (`render_h1::stage`, switch `BO2MP_H1_STAGE=fitted`). Probes: one cube per draw, mip `6 − 6·gloss`, no blending; already matched. Model lighting atlas is 1,024 wide, `LIGHTING_LOOKUP_SCALE = (1.5/1024, 1.5/h, 0.375, 0)` (switch `BO2MP_H1_ATLAS=mw2`). Sun-shadow constant maths is translated in `render_h1::sun_shadow`, which is not drawn yet. |
| d2e8f82 | **Collision:** the record layouts all matched already. World trace = IW4 capsule trace (epsilon 0.125, walkable 0.7, step 18). **Placed models collide only with point traces** (bullets and sight), traced per model on their collision LOD, exact plane crossing; no longer merged into the world mesh, which removed the 1.5M triangle budget. Collision surfaces shared between placements (Crash 257 → 72 MiB). Switch `BO2MP_H1_COLLISION=soup`. Test: `assets drop_test_h1`. |
| 95cf272 | The game's sun and tone-map maths made default (`BO2MP_H1_LIGHT`, `BO2MP_H1_TONEMAP` = exact; `fitted` brings the old values back). |
| c51f8ca, e5944a4 | **Lighting:** the sun = the zone's primary-light colour **unscaled** × light-set diffuse/specular scales (no π); spot/omni fills recorded; static models' `LIGHT_PROBE_AMBIENT` set; **film constants found** (equal to our MW2 mapping at defaults; `COLOR_BIAS.w = 1/max(d, 2⁻¹²) − 1`; user brightness `h`); rim lights recorded. |
| a2cf308, 7aab2ec, 64887ab | Notes: vision parse, blend and fade; light-grid tweak `(cᵢ·I − c₀)(1 + C) + c₀`; fog writer; post chain order; command stream. |
| 942cd8d, d95b47d | Tone map and exposure: `E = 0.18·2^(2.84 − EV)/b` (1.2586 at EV 10), auto key 0.18, adaptation, filmic two-segment curve. |

None of this has been seen on screen: the cloud machine has no GPU. **The owner should check on his PC:**
- Do MWR maps look right now that the exact sun and tone map are the default (brighter or darker than before)?
- Can you walk through props on Shipment and shoot them?
- Do kill volumes kill?
- On Crash, is there a floor around height 220 that you fall through?

## 5. Half-finished, now on main

- **Sun shadow map pass** (main up to 1bff518; `render_h1::sun_shadow`, `host.rs`, `world.rs`, `state.rs`, `gpu.rs`, `scene.rs`, `fastfile_h1/src/map.rs`): **off by default**; `BO2MP_H1_SUN_SHADOW=1` turns it on. 43 `render_h1` tests pass. **Never seen on a GPU.**
  - Done and read from the exe: cascade split and fit (view corner rays projected on the sun axes, eye on a texel centre, 4x texel size and depth per cascade, texel-snapped centres), lookup matrix, switch partitions, scale/blend, `PIXEL_ADJUST_ARRAY_0..2`, `CASCADE_V_CLAMP`, `NEAR_FAR_PLANE`, caster bias (reversed depth, test greater-or-equal, bias -128, slope -4, clamp 0, comparison sampler GREATER with zero border), real caster bounds from `GfxWorld::shadowBounds`, per-cascade caster culling (`BO2MP_H1_SUN_SHADOW_CULL=0` off), the sun-lit-cell gate (`BO2MP_H1_SUN_SHADOW_GATE=auto|force|off`; our stand-in is one always-visible cell), map size by quality (`BO2MP_H1_SUN_SHADOW_QUALITY`; Auto = 1024, High 2048, Extra 3072), light-set defaults when a map has none (sample size 0.25, filter radius 4, 2 cascades).
  - Inferred: translucent casters (`TRANS_SHADOWMAP_SUN`, Rgba8Unorm cleared white, slot 7; Shipment has no slot-7 technique so nothing draws).
  - Not done: drawing the cached (static) cascade tiles (the tile maths and LRU pool exist, `BO2MP_H1_SUN_SHADOW_CACHE=1|force` only maintains the pool; the game has the cache off by default; drawing needs a shader to copy part of a depth texture), per-cascade scissor rects, the small-map mode (spot-shadow quality on and eye inside a spot volume).
  - **Next:** the owner runs it with `BO2MP_H1_SUN_SHADOW=1` on Shipment: shadows under props, no acne, no seams at cascade switches. If good, make it the default (change `enabled_from_env`, update docs/README).
- The parachute drop for Warzone/Resurgence is parked on its own branch (`bo2mp/warzone-drop`) **on purpose**. The owner said to leave it out for now.

## 6. Still to do (MWR), in order of value

1. **Verify the sun shadow pass on a GPU** (section 5), then turn it on by default.
2. **Map scripts → movers and exploders.** The fans, windmill and hanging objects stand still, and exploders never fire, because each map's own GSC is in the patch/common zones, which were not fetched.
   - Stage `common_mp.ff` and the patch zones from the PC.
   - Read the map scripts (`fastfile_h1` keeps compiled scripts; `h1_scripts` example).
   - Run them through our GSC path, or translate their mover and exploder logic.
   - Shipment's `out_of_bounds` triggers depend on these scripts.
3. **Dynamic entities:** clutter and destructibles are not placed (Crash has 630 clutter and 15 destructibles).
4. **Spot and omni lights drawn natively:** their constant fill is already read; see the "Lighting" section.
5. **Unread lighting pieces:** the meaning of the exposure scalar `b` (inferred to be the stage scale 0.001); `trigger_hurt` START_OFF in `sim::t6::hurt`.
6. **Other passes:** SSAO/MDAO, bloom/glow/veil (`VEIL_PARAMS` writer `FUN_1400836c0` / `140083530`), depth of field and motion blur. Read their constant writers and shaders and wire them.

## 7. How to work

- **Find a constant's writer:**
  - Scan the reference manager for writes into `0x1525c35d0 … +422·16`, or for INC on the version counters.
  - If there is no direct writer, look for it in the per-view block fill (`FUN_140085af0`, `FUN_14060bef0`, `FUN_14060b780`, which take the view and the source state `0x1525c23d0`).
- **Find which shader reads a slot:** use `h1_tech` on the zone.
- **Translate into Rust:** put it in `render_h1` / `fastfile_h1` / `assets::lane::h1_map`, add a unit test, make it the default, keep an env switch for the old behaviour, document it in `docs/H1_NATIVE.md`, add a README entry, then commit and push to `main`.
- **If something can't be read, say so plainly** and mark the fallback as inferred.

## 8. Sun shadow addresses (private notes, 30 Oct, sun-shadow finish)

- Setup per frame: FUN_140612560 -> FUN_140611fa0 (sun shadow struct = frontend global +0x542350; sunProj at +0xd87f4, params at +0xd886c; cascade count at global +0x61ab40).
- Axes FUN_140610280; frustum rays FUN_14060fd00; ray projection FUN_140612740; fit FUN_140610510; centres FUN_140611020; view/proj FUN_140611230 / FUN_1406113a0; switch partitions + scale/blend FUN_1406119d0; lookup FUN_140610390; cull planes 14060ddc0/dea0/dfb0/e8b0; cascade step table 0x140893f88.
- Per-frame globals FUN_1400f01f0: pixel adjust / V clamp / near-far at 0x14256eca8..ecdc (view parms block copied to slots 269..273 by FUN_14060bef0 from viewInfo+0x188+0x40); depth range FUN_14060f1e0 over rgp.world+0x610 (GfxWorld.shadowBounds).
- Raster states FUN_1405de7b0 (shadow offset dvars 0x14fd61260 int 128, 0x14fd61268 slope 4.0, 0x14fd61270 clamp 0; all negated). Comparison sampler FUN_140608b50 + FUN_14060a190: ComparisonFunc 5 (GREATER), border address, zero border.

## Fog pass pick and sky opacity (31 Oct, night)

- Post-opaque draw `FUN_140084230` (called from `FUN_140628240`): fog on = `FUN_1400848a0` (atmos: view+0x11dc =
  per-view block slot 224 .w = ATMOS_FOG_PARMS_2.w > 0; sun-fog mode view+0x2be0 != 0: FOG_SUN_DIR not (0,1,0,0)).
  Index `FUN_140084750`: ssao (view+0x1d88, else view+0x1d74 when flag byte view+0x1c77 & 4; 3 -> 2) | 4 atmos / 8
  sun-fog | 0x10 if ln2 (0x140813554) / [fog+0x540d60] < 99000 (0x14081355c) | 0x20 if [fog+0x540d30] < 8910
  (0x140813558, atmos only). Material = `*(0x14fe6f0c8 + 8*index)`; name table 0x140888020 (`{name, &global}`).
- Fog writer `FUN_140084520`: ATMOS_FOG_PARMS_6.w = view float[0x477] (block slot 224 .w = P2.w) x dvar
  `DAT_1415bc728` (registered in `FUN_14007fa90`, hash 0xcee1f7ec, float default 1, range 0..1).

## Code constants: render-command handlers (31 Oct)

- `0x140621350`: render-command handler 'set code constant': reads (index u32 at cmd+4, vec4 at cmd+8),
  writes `0x1525c23d0 + 0x1200 + 16*index` (= the constant array `0x1525c35d0`) only if changed, bumps the
  u16 counter `0x1525c23d0 + 0x2da0 + 2*index` (= `0x1525c5170`). Listed in the command table at `0x140896360`.
  So constants like LIGHT_DIFFUSE / MODEL_LIGHTMAP_PARAMS have no direct writer: find the frontend code that
  queues this command (sun builder / static model draw).
- `0x1406212a0` loop: bulk copy of 4-vec4 groups (matrices) with counter bumps.
- `0x140621330`: copies `rgp.world (0x14fd70160) + 0x6e8` to `0x1525c00d0`.
- `r_tonemap`: registered in `FUN_140080060` (hash 0x17cae392), enum names `0x140f72438`: Perceptual,
  Reinhard, Filmic, Linear, Luminance (via strings at 0x1408132d0..); default index 3 = Linear.

