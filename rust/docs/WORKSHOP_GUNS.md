# Workshop weapon packs (BO3 Steam Workshop), six Create-a-Class tabs

Guns from the owner's Black Ops III Steam Workshop weapon packs are guest guns. Each pack has its own tab in
Create-a-Class's weapon picker: **COLD WAR**, **VANGUARD**, **MW 2019**, **COD 2**, **CODOL** and **CODOL 2**. They come
after IW and before MOD/OTHER. The COLD WAR and VANGUARD "COMING SOON" placeholders are gone. Battlefield 1's is still
there. A tab shows only when its pack is installed and Black Ops III is installed.

| Tab | Pack (Workshop id) | Zone read | Game key | Rows | Guns listed | Complete offline |
|---|---|---|---|---|---|---|
| COLD WAR | Black Ops Cold War weapons (2753027294) | `zm_mod.ff` (`t9_<gun>`) | `t7cw` | 1500.. | 52 | 52 |
| VANGUARD | Vanguard weapons (2906698009) | `zm_mod.ff` (`s4_<gun>`) | `t7vg` | 1600.. | 42 | 42 |
| MW 2019 | Modern Warfare 2019 weapons (3829658749) | `zm_mod.ff` (`iw8_<gun>`) | `t7mw` | 1700.. | 40 | 40 |
| COD 2 | Call of Duty 2 weapons (804408209) | `zm_mod.ff` (`<gun>_cod2_zm`) | `t7cod2` | 1800.. | 26 | 26 |
| CODOL | CoD Online weapons (1798855811) | `zm_mod.ff` (`codol_<class>_<gun>`) | `t7ol` | 1900.. | 47 | 47 |
| CODOL 2 | CoD Online weapons, second pack (2014476142) | `core_mod.ff` (`<gun>_zm`) | `t7ol2` | 2000.. | 35 | 34 |

That is **242 guns**. "Complete" comes from `examples/t7_pack_guns`. A complete gun has its view and world models in
the zone, mesh data present (inline or in a package), every view colour map present, and an idle animation. The CODOL 2
gun that is not complete is the Mini UZI: one of its nine colour maps is a reference that does not resolve.

## Where things live

- **Discovery.** `asset_transport::workshop` keeps mods instead of discarding them (`Discovery::mods`,
  `Bo3WorkshopMod`). Each mod records its id, title, `FolderName`, folder, its `core_mod`/`zm_mod`/`mp_mod` zones and
  their `.xpak` files. Language copies are left out. `workshop::bo3_mod(id or FolderName)` looks a mod up.
- **Rosters.** `bo2_profile::guest::WORKSHOP_PACKS` (`guest/workshop.rs`) holds one hand-written roster per pack, plus the
  pack's id, zone, tab, title and first statstable row. `WorkshopPack::zone_weapon` maps a roster name to the zone's
  name (`cw_ak47` -> `t9_ak47`). `WorkshopPack::picture` gives the tile picture.
  - **Why hand-written:** statstable rows (saved classes) must not move when a pack updates. The zones also hold
    upgraded (`_up`, `_upgraded`), dual-wield (`_ldw`/`_rdw`), starting-pistol (`_start`), alternate-mode and skin copies
    (CoD Online's `ak47cb`, `an94hw`, ...). Those need judgement, and the existing architecture is `&'static`
    rosters. A roster gun the zone lacks does not load, so it is not listed in a match.
  - **Classes:** tactical and marksman rifles are assault rifles (Cold War, Vanguard), as BO3 does. Bolt-actions are
    snipers and the BAR and Bren are LMGs (CoD 2, as World at War). Pistols and launchers are secondaries.
- **Names and namespace.** The pack guns are Black Ops III weapons and use the T7 namespace. A gun's catalog name is
  its pack tag plus its name (`cw_ak47`, `ol2_ar_standard`). Its script and statstable name is therefore
  `bo3_cw_ak47(_mp)`, and its text key is `WEAPON_BO3_CW_AK47`. CoD Online 2 reuses BO3's stock names
  (`ar_standard_zm` is its G37H), but its guns are `bo3_ol2_*` and never meet BO3's own `bo3_ar_standard`.
  `prefix()` is `bo3_` for every pack game. The availability bits are now a `u32`: 17 games.
- **Listing.** Every pack gun is listed on its tab, as for the games in `COMPLETE`. Packs come last in `PRIORITY`, so
  they never remove another game's copy, and BO2's own AK-47 does not hide Cold War's. Pack guns are not in the
  Mystery Box: there are hundreds of them, and they are meant for Create-a-Class.
- **Front end.** `ui::foreign_maps::guest_guns_available` marks a pack available when BO3 is installed and the Workshop
  item has the pack's zone (logged per pack). `read_game_image` reads a pack's tile pictures from its zone
  (`workshop_pack_icons`). These are CoD Online's `i_codol_<gun>` and CoD 2's `iw2_hud_<gun>[_scoped]` kill icons. Cold
  War, Vanguard, MW 2019 and CoD Online 2 guns all name BO3's stock icons (the AP9's), so their tiles have no picture.
- **Match.** `assets::lane::t7` (`prefetch_t7_pack_guns`, `build_t7_pack_guns`) runs after BO3's own guns in
  `session_load/common_cache.rs`. Each pack's builds are absorbed into the weapon registry as `t7 workshop guns
  absorbed`. `match_walk` sets each pack's availability once its guns are loaded.

## The lane: `load_t7_guests` generalised

core_common's gun build (`load_t7_guests`) now takes a `T7Zone` with an optional pack. A pack differs in these ways:

- **Guns:** `pack_chosen` (roster -> zone weapon) instead of `BO3_GUNS`.
- **Packages:** one shared `XpakSet` for every pack. It holds the packs' own `.xpak` files, then BO3's `zone` folder's.
  It is passed from build to build. Stock attachments (the AP9 parts every Cold War, Vanguard and MW gun names) stream
  from BO3's packages. Only each entry's hash table is read when a package opens. An entry's bytes are read when a mesh
  or picture needs them. The 1-3 GB packs are never read whole.
- **Textures:** capped at 512 by default (`BO2MP_WORKSHOP_TEXTURE=<size>`; `BO2MP_GUN_TEXTURE` also applies). At 512, a
  view model shares its world model's pictures. All six packs come to about 4300 pictures, with a peak of 3.6 GB to
  build them (at 1024 it was 8400 pictures and 8.8 GB). Converted pictures go to the existing on-disk cache
  (`t7_gun_cache::key_for_files`: the pack zone and its `.xpak`).
- **Not done for packs:** the arms skeleton (core_common's build inserts BO3's arms, which pack guns pose too), BO3's
  camos (installed once by core_common's build), and attachments. Pack guns name BO3's stock AP9 attachment uniques,
  so none are offered and the roster's attachment lists are empty.
- **Snipers:** they keep the same treatment as BO3's (a `_scope` attach model is removed, and BO2's scope is used when
  aiming).
- **`BO2MP_WORKSHOP_GUNS`:** `0` turns the packs off. `cw,ol2` (tags, games or ids) reads only those packs.

### core_common and `,`/`<ref` names

The brief said to resolve other-zone references from core_common. Measured offline, that is not needed for these
packs: every chosen gun's view and world models, materials and animations are inside its own pack zone. The only
outside data is the stock attachments' streamed data, and the shared package set covers that from BO3's folder. A
second core_common walk (core_common is the biggest BO3 zone) would add load time and memory and gain nothing here,
so it is not done. (The arms come from core_common's own build.)

### Zombies numbers made multiplayer's (`normalise_zombies_numbers`)

The packs ship their Zombies copies only, with zombie-sized numbers. Examples: Cold War's AK-47 does 230, falling to
100. The M82 does 1000. The RPG-7 does 5000 on a direct hit plus a 1500 explosion. CoD Online 2's AK-47 claims a
960-round magazine. In each pack gun's BO2 `WeaponDef`/`WeaponVariantDef`, after the field copy (`T7_TO_T6`):

- **Bullet damage:** if the first (closest) step is above the class figure, every step is scaled by the same factor.
  The fall-off keeps its shape, and no step goes below 1. `minPlayerDamage` and `playerDamage` are scaled too. The
  class figures are about BO3 multiplayer's: AR 42, SMG 36, LMG 45, sniper 100, shotgun 50 (per pellet), pistol 38. A
  launcher's direct hit is capped at 300.
- **Explosion:** inner damage at most 200, outer scaled by the same factor.
- **Magazine** 1..200. **Start ammo** >= 0. **Reserve** 0..999. **Melee** at most 135.

Every change is logged (`Zombies numbers made multiplayer's on N guns: ...`). Known simplification: semi-automatic
rifles in the AR group (the M1 Garand, SVT, G43) are capped like automatic rifles (42 per shot). They are weaker than
BO2's DMR feel.

### CoD Online 2's animation tables

The `core_mod.ff` walk steps over a stretch of the zone (`stepped over assets 1238..1240: layout not read`). Names
referenced after that stretch are placed by fitted deltas that land wrong. As a result, most of its weapons'
animation-table names read as garbage (`"andard"`, binary). Its guns' own animations are in the zone under
`vm_<gun>_<ending>` (`vm_g37h_idle`, `vm_g37h_runing`).

When a pack gun's idle does not resolve, `repaired_anims` takes a stem from the gun's view model name and fills the
slots from `ANIM_ENDINGS`. The stem is a word of `g37h_vmgun`, `vmgun_famas`, `ak47_ol_vmgun` -> `vm_47`, or
`csg12recluse_vmgun` -> `vm_csg12`. The slots are idle, fire, reload, raise, drop, sprint, prone, slides and ADS.

Eight CoD Online 2 guns have no animations of their own under any name, so they are **left out of the roster**: the M1
Garand, Type 95, Remington ACR, MK17-CQC, CAR-T, SPBC, M16A4 and M21 EBR. Their tables name BO3's stock animations
through the misplaced references. The G37H Custom (`g37h_vmgun_camo`, its mesh not read) is left out as well.

## Sounds

`asset_audio::bo3_pack_sound_zones` lists each installed pack's zone that holds its sound bank. That is the zone whose
`snd/all/<zone>.all.sabl` sits beside it, otherwise every zone the pack has. These zones join the match's sound
sources under the T7 namespace. `sound_load_t7::banks` falls back to every bank in a mod's `snd/all` when none is named
for the zone. MW 2019's bank is `zm_weapon2019.all`; this fallback never applies in BO3's own folder. `weap_` aliases are
now kept (CoD Online 2's naming).

Measured (`t7_pack_guns --build`), guns whose own fire sound is playable from their pack's bank:

| Pack | Own fire sound |
|---|---|
| Cold War | 52/52 |
| Vanguard | 42/42 |
| MW 2019 | 40/40 |
| CoD Online | 47/47 |
| CoD Online 2 | 16/35 |
| CoD 2 | 0/26 |

CoD 2's bank is read from its `core_mod.ff`, but the walk stops there at material 356 (a bad data pointer) before
reaching the bank. CoD 2's guns, and the CoD Online 2 guns whose fire alias reads as garbage (`SAR21`, `F2000`), play
BO2's sounds for their class (`WeaponRegistry::borrow_class_sounds_if` in `match_walk`).

## Offline check

```text
cargo run --release -p assets --example t7_pack_guns -- [--build] [cw,vg,mw19,cod2,ol,ol2]
```

It finds the packs as the game does (`CODLEGACY_GAME_PATHS=<play>\game_paths.cfg` for BO3's folder). For every roster gun it
reports: its group, whether the zone has it, its view and world models with mesh data and colour maps, its animations,
and how many were found by name. `--build` also builds the guns as a match does and adds the lane's report, every
animation decoded by the engine's decoder, each built gun's model and animation edges, and the fire sounds.

Results on the owner's install (2026-10-31):

- **Walk:** all six pack zones walk to their end (0.1-0.4 s each).
- **Build:** 242 weapons, none without a view model, world model or animations. Every animation decodes. A few
  inspect and reload clips with more than 140 bones do not fit BO2's byte counts and are left out (Cold War M60/MG 82
  inspect, Vanguard MG42 reload-empty, MW PKM reload, CoD Online M240/MG4/RPD reload).
- **Pictures:** 3 of about 4300 cannot be paired as specular+gloss because of mixed formats. The guns still draw with
  colour and normal maps.
- **Time and memory:** about 7 s for all six packs from scratch, with a 3.6 GB peak.

## Not tested in game

Nothing here has been run in the game. The CaC tabs and tiles, first-person fit on BO2's arms (the pack guns' view
animations are BO3-rig animations, as BO3's own), the numbers in play, sounds in a match and load-time memory with all
packs are checked only by the offline example and unit tests.

## Gaps

- CoD Online 2 ships its zones as `zone.7z` too; that archive is not extracted or read, only the loose `core_mod.ff`.
  Eight of its guns are left out (above).
- CoD 2's sound bank (above). Its multiplayer copies (`<gun>_cod2_mp` in `core_mod`) are not used. That walk
  stops before any weapon (0 captured), so the `zm_mod` copies are read and normalised instead.
- No pack has per-gun menu pictures for Cold War, Vanguard, MW 2019 or CoD Online 2 (they name BO3's AP9 icons).
- No attachments for pack guns, no Pack-a-Punch of their own (the packs' `_up` copies are not read), and no Mystery
  Box.

## Open: blob / missing gun / red gun (5 Nov 2026)

Owner's screenshots: CoD Online CheyTac M200 a huge flat-shaded octagonal blob, MW 2019 RAM-7 only the glove, Model 680 solid red on an MWR map; worse on MWR and Ghosts maps.
`cargo run --release -p assets --example t7_pack_lods -- <zm_mod.ff> [gun]` prints each gun's models (bones, LODs, vertices, triangles, inline or streamed, materials with picture counts). On the owner's zones: one LOD per model, every mesh streamed (`.xpak`), the densest-LOD pick is right, the colour maps are all defined. `model_ref` ignores rigid vertex lists (unskinned vertices go to bone 0), and the mesh bytes need the 1.5-2.4 GB `.xpak`, which could not be staged. Still to check on the owner's PC: the lane report's `models: ... not [...]` line and `t7 gun: material ... has no colour picture` lines for the pack, and whether it differs on a Black Ops II map versus an MWR/Ghosts map (those maps apply the BO3 look to viewmodels too).
Findings (second pass, same day; the probe takes `BONES=1 SURFS=1 MATS=1 HIDESIM=1` and `IDLE=1 <gun,gun>`):
- CheyTac: CONFIRMED from the zone. `codol_sniper_cheytac` hangs `codol_sco_sniper_cheytac` (scope, 4358 vertices, no tag named) and `codol_sco_sniper_cheytac_ads` (the aiming overlay: 866 vertices, a 17-vertex reticle disc, a 17-vertex stencil disc, an 832-vertex cover, tag `tag_scope`) on the gun; the sniper filter knew `_scope` / `_ads_view` only. Fixed: `is_ads_overlay`, `is_own_scope_model`, `own_root_tag`.
- Part bits: CONFIRMED in code and by a stand-in on the zone (`HIDESIM=1`): `model_ref` set `1 << bone`, the engine reads from the top bit, so a hide tag hid the mirror bone's surfaces (M16A4, M4A1, AUG LMG, FAL, R90, UZI lose body surfaces). Fixed (`part_bit`), and Black Ops III guns' first-person hide now drops a surface only when all its bones are hidden.
- RAM-7: NOT confirmed. All 30571 + attachment vertices reach the draw (log rows 43785 = 7656 arms + 36129 gun, 12 draws), its idle clip leaves the gun bones at bind, its materials are the AX-50's, its stream layout adds up. What is left: weights (mesh stream in the 1.5 GB `.xpak`, not stageable). The `bo2mp t7 pack gun:` line prints its box, centre, weights on missing bones, unweighted and non-finite vertices, per-surface bones and the surfaces its hide tag touches.
Pack attachments are off on purpose (`pack.is_none()` in `build_t7_pack_guns`; rosters' `attachments` empty; `guest_attachments` matches `au_<gun>_` names, which pack zones do not use).

Third pass (6 Nov 2026; `t7_pack_lods` takes `FIELDS=1` for every field and animation slot of a weapon):
- **Attach tags (CONFIRMED from the zone, fixed):** 28 MW2019 weapons (25 roster guns) had no `attachViewModelTag` fields: their list is one array the linker shares among most of the pack's guns (`[-, -, tag_stock_attach, tag_barrel_attach, tag_mag_attach]`, block 5 offset 12889088 in his `zm_mod.ff`), and `walk::weapon::list` read a shared list's entries only where they were references, never the first owner's inline strings. `field_str`'s fallback (another variant of the same `base_name`) found nothing since the `_up` variants have their own base names, so the stock, barrel and magazine hung on `tag_weapon`. The walk now reads the inline strings after the list in slot order (`BO2MP_T7_SHARED_TAGS=0` = old); CoD Online 2's `core_mod` places such a list wrong (the stepped-over stretch) and its bytes are refused as tag names (`is_tag_name`), so its six such guns are as before. The CoD Online and Vanguard packs were not affected.
- **Camo pictures (CONFIRMED in his 5 Nov log, fixed):** no material binds a pattern camo's picture, and the world's upload hands the GPU only bound pictures, so BO3's camos (and Black Ops', and BO2's own patterns) never reached it: `bo2mp shine maps not ready: ... specular 0x1612 [not uploaded]` across many gun materials at once, each then drawn plain (`bo2mp dynamic draw without its shine group`). `weapon_camos::pattern_images` + `world_images` fix it (`BO2MP_CAMO_PICTURES=0` = old); BO3 camo pictures are 512 a side now (`BO2MP_T7_CAMO_TEXTURE`).
- **RAM-7 (still NOT confirmed):** the zone says nothing hides: 33 bones all under `tag_sling`, body rigid on `tag_sling` (surfaces 0 and 1), sights 2735 + 2735 vertices on `tag_sight_off`/`_on`, idle keys only `tag_weapon_right` at a place like the AX-50's, pictures all defined, stream layout exact to the byte. By the rigid lists neither the old (mirrored) nor the new part bits hid anything of it, so if it is still invisible the cause is in the skin weights or outside the mesh. The `bo2mp t7 pack gun:` line now adds `collapse N vertices (P%) with [bones]` and `vertices per bone [...]` from the real weights, and a gun whose hide tags would collapse over half its vertices keeps them off (`bo2mp t7 gun ...: left off`; `BO2MP_T7_HIDE_GUARD=0`). Send that line.
