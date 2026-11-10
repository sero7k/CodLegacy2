# Infinite Warfare guns (`iw7_*`), the IW tab

41 IW7 multiplayer guns are guest guns on their own Create-a-Class tab, **IW**
(after HMW and before MOD/OTHER). They fire with BO2 gunplay: the IW7 weapon def
is mapped onto S1 fields, then `capture_s1` builds a BO2 row, as the HMW and
`jup_gun` lanes do.

- **Roster**: `bo2_profile::guest::IW7` (stats from 1400). AR: NV4, R3K, KBAR-32, Type-2, Volk, R-VN, X-Eon, G-Rail, OSA, M1.
  SMG: Erad, FHR-40, Karma-45, RPR Evo, VPR, Trencher, Raijin-EMX, MacTav-45. LMG: R.A.W., Mauler, Titan, Auger, Atlas.
  Sniper: KBS Longbow, EBR-800, Trek-50, TF-141. Close range: Reaver, DCM-8, Banshee, M.2187, Proteus, S-Ravage.
  Secondaries: EMC, Oni, Hailstorm, UDM, Stallion .44, Hornet, Spartan SA3, P-LAW.
- **Left out**: melee, killstreak and mode-only weapons (Venom-X, Howitzer). HVR, Widowmaker, Rack-9, Kendall 44 and DMR-1
  are not in the staged zones at all (see [Missing guns](#missing-guns)).
- **Dedupe**: BO2's own guns come first, and MW2 guns are not touched. IW7 rows are listed *beside* other games' guns
  (`BESIDE = "iw7"`), so classics such as the M1 or MacTav-45 stay on the IW tab. A mod gun with the same name gives way.
- **Names** are the retail display names. No localize strings were staged, so they come from imfdb, the CoD wiki, Prima and the
  Activision blog.

## Where it reads from

`asset_audio::iw7_zone_dir()`: `CODLEGACY_IW7_ZONE`, then game path `iw` (or its `zone/`), then the games root.
`fastfile_iw7::zone_path` accepts `x.ff` or `iw7_x.ff`.
- Weapon defs come from `global_core_mp`. Anim and sfx packages, anims and sound banks come from `patch_common_core_mp`,
  `common_core_mp` and `common_mp`. Models come from `mp_weapon_{view,world}_iw7_<gun>_tr`. Materials come from the
  `techsets_*` zones. IW7's first-person rig (`viewmodel_base_viewhands_iw7`) comes from `common_core_mp`.
- **Lane**: `crates/assets/src/lane/iw7_gun.rs`, with field tables in `iw7_gun_fields.rs`. It runs after HMW in
  `session_load/common_cache.rs`.
  - Anim packages and the sfx package are found **by name**. Each anim slot is checked against IW7's naming endings,
    because the walker's fitted references can land on a neighbour.
  - IW7 blend verts are BO2's (`b0`, then `(bone, weight)` pairs) plus a zero pad word per vertex, with plain bone indices;
    BO2 uses no pad word and bone indices ×64 (`s1_blend`).

## View models

What was wrong, and what the lane does now:

- **Geometry read 16 bytes late.** A surface sharing another surface's mesh (two copies of the M.2187's shell, its akimbo
  models' parts) names it by a VIRTUAL reference, which the walk places by deltas fitted from string references. On the
  M.2187 view model those landed 16 bytes past the mesh: 18 surfaces' vertices were garbage and 19 index buffers shifted
  by eight indices (every triangle on the wrong vertices; the "out-of-range indices" were the tail running into the next
  rigid list). `Walker::geometry_blob` takes a reference to the start of geometry an earlier surface read in place, of the
  same length, within 64 bytes. A surface whose vertices are still not positions is left out (`vertices_sane`).
- **Surfaces not drawn.** The first-person pass draws no surface without a colour map. A material whose picture is in an
  `imagefile` pak that is not in the folder (a staged copy without the paks: 684 of 692 gun materials on the check machine)
  now draws with a stand-in finish (`link_stand_in`: gun-metal grey, a dull dielectric specular), not as chrome and not
  missing. On the owner's install the pictures are there (paks 17, 18, 37, 53, 69, 85, 101, 117, 132, 133, 165) and nothing
  draws with the stand-in.
- **Colour slot.** IW7's colour map is `colorMap`, a packed colour+smoothness picture (`..._packed_cs`); normal and gloss
  are `normalMap` (`_packed_ng`/`_nog`). The check `picture_kind` flags a colour slot holding another kind: none do.
- **Hands off the gun.** IW7's view animations key IW7's full-body rig: the shoulders hang on clavicles on a spine under
  `tag_torso`, `j_mainroot` turned a third of a turn. Played straight on BO2's arms (shoulders on `tag_torso`), the wrists
  land 2 to 19 units (right) and 14 to 32 (left) from the gun's hand tags. The lane puts the rig in the first-person catalog
  (`asset_game::IW7_REFERENCE_ARMS`); `FpvArmsRetarget::build_iw7` poses it with the clips and gives BO2's arm bones their
  counterparts' places (palms `j_metaring_*_1`, wrist twist `j_wristfronttwist1_*`, fingers one joint on) and the gun's
  root `tag_weapon_right`'s, where the clips put the gun (its `tag_ik_loc_ri`/`_le` on the wrists, 0.0 units).
- **Skinning.** IW7's blend repack checks out on every gun: pad words zero, no bone index past the skeleton, no vertex
  whose weights add past one. The shared S1/HMW attachment code (`s1.rs`, `hmw.rs`: `k % 2 == 0`) remaps positions 0, 2,
  4 of a multi-bone vertex (its first bone and its *weights*) instead of 0, 1, 3 (its bones): their multi-bone attachment
  vertices are garbled. Not IW7's code; left to that lane.
- **Parts.** IW7 gun view models are single models (no composite parts); sights and magazines are surfaces rigid on their
  own bones (`tag_sight_on`, `j_mag1`). IW7 attachments are not read.

## Sounds

`asset_audio::sound_load_iw7` reads the zones' sound banks (`fastfile_iw7::sab`): each alias record names its clip by
`assetId`, an entry of the zone's `.sabl` (loaded) or `.sabs` (streamed) file: FLAC frames without a stream header, given
one (`sab::flac_header`) and decoded under the IW7 namespace (`audio::clip_store`). The killcam's slow-motion records
(`atmosphere`/`killcam`) are left out. A gun plays its own alias where a clip is here, else its donor's
(`WeaponBuild::borrow_sounds_and_effects` keeps the gun's own slots); its animations' sound notes (`weap_m4_clipout_plr`)
play themselves where their clips are here, else the donor's reload sound of the same kind (magazine out/in, chamber,
shell, lift, end). ADS enter/leave (`enterAdsSoundPlayer`, `leaveAdsSoundPlayer`) map to BO2's ADS slots.

Measured (`iw7_snd_probe <dir> decode weap_`): 452 weapon clips decode, none fail, every frame count matches its entry;
44.1 or 48 kHz (a few 36 kHz), mostly stereo; RMS median −15 dBFS, peaks at 0 dBFS; the high-frequency ratio is low only on
the `_lfe` layers. Not muffled.

Coverage on the staged files: 27 of 371 package aliases and 178 of 465 reload notes play IW7's own; the rest need
`common_mp.sabl` (3125 records of `common_mp`'s bank name it), not staged. Own fire sounds today: R-VN, X-Eon, G-Rail
(player), VPR, Trencher, Auger, Atlas, Trek-50, M.2187, Proteus, UDM, Stallion .44.

## Glow

A gun material with an `emissiveMap` (and `emissiveParams.x` > 0, `fastfile_iw7::glow`) that is lit and opaque gets a second
picture, `TS_EMISSIVE_MAP` (`0xf1`, the auxiliary map's place): the emissive map (at most 512) as linear light times
`emissiveParams.x` times `BO2MP_IW7_GLOW` (default 1; 0 turns it off), its hue kept under 2.5 (`iw7_glow::link_gun_glow`).
The viewmodel draw rows carry its slot (`T6DynamicDraw::draws`, now 8 words) and the gun's instance row a sixteenth in
`primary.w`'s fraction. `geometry_diagnostic.wgsl` (`gun_glow`) adds it after the lighting and the fog and before the exposure
and tone, in both the Black Ops III light path (`t7_shade` case) and Black Ops II's shine path. Other props' auxiliary maps
(tile, flow) are not read as glow. No new shader def; the naga_oil test (`geometry_shader_compiles_with_motion_through_naga_oil`:
none, SHINE, SHINE+WATER, ALPHA_TEST and every def alone, with and without MOTION) passes. Additive materials (Banshee `_trans`,
Hornet `_alpha`) are not drawn this way. The gain is a guess in the draw's light units (a lit white surface under a unit light
is 1); not seen on a screen yet.

## Effects

Every IW7 gun names its effects in a `WeaponVFXPackage` (`iw7_<gun>_mp_vfxpackage`, `common_core_mp`: 136 packages, 458 effect
slots, `iw7_vfxpkg <zone> <name>` lists them) and **all of them are `vfx` particle systems** (`vfx/core/muzflash/ar/vfx_muz_ar_v` /
`_w` / `_v_ads`, `vfx/iw7/core/shell/ar/vfx_se_ar_view` / `_world`; none is a classic `fx`).

**Own flash, eject and tracer (2026-10-27, `BO2MP_IW7_OWN_FX=0` gives the donor's back).**
- *Reader*: `fastfile_iw7::walk::vfx` keeps each system's emitters (the 144-byte record: spawn rate, life, delay, burst, emitter
  life), states (element type 0 sprite, 1 beam, 2/3 cloud, 4 decal, 5 flare, 6 geometry trail, 7/8 light, 9 model, 10 oriented
  sprite, 11 runner, 12 tail), the three module groups and each module's type, 144 data bytes, curves and linked assets. The
  module numbering is checked by data over common_core_mp's 1189 systems (`iw7_vfxdump <zone> [name]`; with no name a histogram
  of module types). The walk reads the same bytes as before: stream end = zone end on `common_core_mp`, `global_core_mp`,
  `patch_common_core_mp` and the gun zones tried (`mp_weapon_view/world_iw7_m4/ar57_tr`). Tracer defs keep their numbers too.
- *Conversion* (`lane/iw7_fx.rs` into `asset_game::ParticleFxView`, `fx_catalog_iw7.rs` builds the IW4-style elements): a muzzle flash
  is a runner that picks one child system (kept as a runner element); each child's sprite emitters become billboard / oriented /
  tail elements: material(s) (picked at random), life, delay, a burst (`particleBurstCount`) or a loop for an emitter that lives,
  colour and alpha from the colour graph times the intensity graph (additive materials keep the gain, the others cap it at 1) and
  the attribute multipliers, size from the size graph, velocity from the velocity graph, gravity module times 800 (plus a force
  pulling down), first rotation, and the flipbook from the material's `textureAtlas` grid. Graphs are sampled at 2 or 8 even
  steps. The flash of a first-person slot draws with the viewmodel. Models (shell casings) become model elements when the
  model is in the zones.
- *Spawn boxes* (2026-11-03, `BO2MP_IW7_FX_SPAWN_BOX=0` spawns at the origin as before): a `spawnBox` module's volume (two float4,
  minimum then maximum, data words 12 and 16; minimum <= maximum in all 3917 of `common_core_mp`'s) becomes the element's spawn
  box. Its word 3 (0 to 2, probably the spawn mode) is not read: every box is filled. Not seen on a screen.
- *Dropped, counted per gun in the probe*: lights (omni/spot), beams, flares, decals, geometry trails, clouds, volumetrics, spawn
  spheres and cylinders (the spawn is at the origin), relative velocity, mirror texture, colour lerp, size lerp, rotation
  graphs, 3D rotation, physics ray casts, events (impact), positions graphs, and extra states.
- *Materials*: the sprite's technique-set blend (add / blend) with `colorTint` (a 200x exposure multiplier) left out, drawn
  unlit, both sides, named `iw7:<material>#fx`. A sprite whose picture is not in the folder (the `imagefile` pak) is not drawn;
  a slot with nothing drawable keeps the donor's effect. There is no stand-in picture (a grey card is worse than BO2's flash).
- *Wiring*: `GuestEffects` slots view/world flash, view/world eject, view/world last eject (`FX_SLOT_*`) and the tracer
  (`WeaponCombatFx` override in `stamp_combat_fx`); `borrow_sounds_and_effects` keeps a guest gun's own stand-ins. The aimed flash
  (`vfx_muz_*_v_ads`) and the magazine eject have no BO2 slot and are not used. Probe per gun: `CODLEGACY_IW7_ZONE=<dir> cargo test -p
  assets --lib iw7_gun -- --ignored --nocapture` prints `own effects` lines (slot -> own / donor's, elements read, dropped
  modules). `BO2MP_IW7_FX_KEEP_UNPICTURED=1` keeps sprites without a picture (a probe, for a folder without the paks).
- *Seen offline with the picture check off (m4, ar57 zones staged; the paks are not)*: 166 systems converted, 140 build an effect,
  82 slots and 16 tracers for 41 guns. **Not seen on a screen**: sizes are halved (IW7's is a width), curve values are times the
  curve's scale, velocity is the local frame, the flipbook plays over life when the atlas module names a rate, the gravity
  unit and the made-up eject velocity (a shell with none of its own is thrown right, up and back) are guesses.
- *Left*: the shell casing model (`vfx_rifle_shell`, `fx_rifle_shell`) is in no staged zone, so the shell ejects keep the
  donor's (92 model emitters wait for it); the explosion and projectile trail slots are still the donor's.

The launchers (Spartan SA3, P-LAW) fire their donor's projectile model (`t6:<model>`; FHJ-18, SMAW) as their own is in an
absent assetfile pak.

## Missing guns

HVR, Widowmaker, Rack-9, Kendall 44 and DMR-1 have no weapon def, localize key, anim or sfx package or model in any staged
zone. The bases without a plain `_mp` def (`iw7_g18_mpr`, `iw7_spas_mpl`, `iw7_ump45_mpr`, `iw7_cheytac_mpr`) and `iw7_m1_mp`
are the classics' rare and legendary variants (`weapon_g18_rare_vm_camo`, `weapon_m200_legendary_vm_camo`, ...), not those
guns. They came with title updates; their defs and models are in zones not staged.

## Check it

`CODLEGACY_IW7_ZONE=<dir> cargo test -p assets --lib iw7_gun -- --ignored --nocapture` prints a table per gun: models,
materials, animations, sounds (own and the donor's), and the view model checks: where the idle holds the gun in IW7's
hands per tag, its bounds in `tag_view`'s frame, the wrists' distance from the grips on BO2-make arms (IW7's own
`viewmodel_base_viewhands`) straight and retargeted, colour slots holding another kind of map, stand-in surfaces,
weights past one and garbage surfaces. It asserts the last four are clean and the retargeted hands within 2.5 units.

Last run (2026-10-21, about 23 s in debug): 41 of 41 guns built and captured, 636 animations decode, none refused.
Retargeted, every gun's wrists are on its grips (0.0 units; Erad 0.7, Spartan SA3 1.7, OSA 0.8, whose hand tags differ).

## Audit (2026-10-27, owner: "some are missing effects and are just not right")

Offline over the 41 guns (`cargo test -p assets --lib iw7_gun -- --ignored --nocapture`, with the gun, techset and core zones
staged; the `imagefile` paks are 1 to 4 GB each and were not):

- **Models**: all 41 view and world models build (surfaces 14k to 68k vertices, none garbled, no weights past one). Three had
  stray Maya `lambert1` faces (ARX-160 world 5 vertices, Karma-45 view 25, S-Ravage world 14) that drew as grey lumps; they
  are left out now.
- **Pictures**: the pictures the guns name are in `imagefile` paks 17, 18, 37, 53, 69, 85, 101, 117, 132, 133 and 165, all on
  the owner's install, so in a match nothing draws with the stand-in finish (that is only for a folder without the paks).
  Native sizes of the 512 distinct colour and normal pictures: 163 at 2048, 219 at 1024, 101 at 512, 29 smaller. They were read
  at most 1024 (specular maps 512): the 2048 bodies (NV4 `weapon_kb_m4_c_..._packed_cs`, 2048 BC7, normal 2048) were soft.
  First person is now read at 2048 (`BO2MP_IW7_TEXTURE`, `BO2MP_GUN_TEXTURE`), specular at half that.
- **Effects**: see [Effects](#effects). Flash, eject and tracer are the gun's own where their pictures and models are here (`BO2MP_IW7_OWN_FX=0` for the donor's); the rest are the donor's.
- **Glowing parts** (drawn since 2026-10-27, see [Glow](#glow)): 18 guns have materials with an `emissiveMap` that glows: sights
  and red dots (NV4, Volk, RPR Evo, Reaver, Banshee incl. its additive `_trans` sights, M.2187), tritium vials (R3K, Atlas, Oni,
  Hailstorm: `*_tritvial_*`, `emissivePara` 3 to 4), the Type-2, R-VN, Raijin-EMX, EMC (its whole receiver: `emc_e`) and Hornet
  (`accessories_g18_x`, an additive `_alpha` too). IW7 adds `emissive * emissiveParams.x` after lighting.
- **Animations**: IW7 gun packages carry no sprint (`sprint_in/loop/out`, slots 197, 199, 200), stun, detonate or night-vision
  clips; sprint is the additive `vm_<gun>_offset`/`_sprint_offset` (slot 40), not a whole pose, so the gun stays on its idle
  while sprinting. `idle_empty`, `hold_fire`, `toggle_*`, `raise_breach` and the empty raise/drop are empty on most guns;
  Erad, Titan, R.A.W., Oni, Spartan SA3 and P-LAW also lack `reload_empty`/`fast` (they play `reload`). A package reference
  that lands on a neighbour (`vm_sonic_reload_fast_rare` in 14 slots of the S-Ravage's) is refused by the name check.
  Set `IW7_DUMP_ANIMS=<gun>` to list a gun's package.
- **Sprint** (2026-10-27): the weapon def has no sprint offset field, and `_offset` is an additive clip (not played), so a gun with no sprint clips gets a procedural pose: `WeaponCatalog::give_last_guest_sprint_pose` sets `bob.sprint_ofs`/`sprint_rot` (`weapon_catalog_s1.rs`, forward/right/up 1.5/1.0/-3.0 in, pitch/yaw/roll 14/24/-10 deg), which `fpv_present` eases over the sprint-in/out times. Selection: own clips, else a BO2 donor clip (never: its skeleton does not fit), else the pose. Values are a guess at BO2's look; tune on screen. The same applies to stun and night vision (no pose; they stay on idle).
- **Left**: the additive glowing sights (Banshee `_trans`, Hornet `_alpha`); IW7's own explosion and trail effects, shell casing models; reading `_offset` itself.

## Gaps

- Pictures in `imagefile` paks that are not in the folder draw with the stand-in finish (a staged copy without them). There are no IW7 attachments.
- Most of the guns' own sounds need `common_mp.sabl`. IW7 flashes and tracers are its own when their pictures are here; ejects, explosions and trails are BO2's (see Effects).
- Not yet tried in a live match: the retarget, the stand-in finish and the sounds are checked offline only.
