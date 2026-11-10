# Ghosts (IW6 MP, PC x64) Weapon Attachments: Behavioural Spec

Status tags: **CONFIRMED** = read directly from exe code or the repo's generated layouts. **INFERRED** = deduced from context. Addresses appear only in section 11 (Evidence).
All offsets are decimal unless prefixed 0x. All structs are little-endian x64.

## 1. Runtime model

- There is NO composed weapon object. Two global tables are indexed by `weaponIdx` (low 8 bits of the weapon word): the `WeaponCompleteDef` (CD) pointer table and the `WeaponDef` (WD) pointer table. CONFIRMED.
- Every stat is computed on demand by a getter `stat(weapon, isAlt [, playerstate])`. Each getter (a) builds a list of up to 4 attachment pointers, (b) reads the base value from WD/CD, (c) applies attachments by one of the rules in section 4. CONFIRMED.
- Attachments are separate assets (type 0x22, 280 bytes). Type 0x23 asset size is 2712 (WeaponDef); the 296-byte WeaponCompleteDef is the zone-level header that points to it. CONFIRMED.

## 2. Weapon word (32 bits) and name encoding

| Field | Bits | Notes |
|---|---|---|
| weaponIdx | 0-7 | index into CD/WD tables. CONFIRMED |
| weaponVariation (camo) | 8-13 | 0-63; 0 = none. CONFIRMED |
| weaponScopes | 14-16 | 1-based index into `CD.scopes`; 0 = none. CONFIRMED |
| weaponUnderBarrels | 17-18 | 1-based index into `CD.underBarrels`. CONFIRMED |
| weaponOthers | 19-25 | bitmask over `CD.others` (max 7). CONFIRMED |
| scopeVariation (reticle) | 26-31 | 0-63. CONFIRMED |

Name format (encode and parse), CONFIRMED:

1. Base internal name (e.g. `iw6_ak12_mp`).
2. Attachment internal names, at most 4, sorted ascending by `strcmp`.
3. Camo suffix `camoNN` (two digits, 1-63; value above 63 encodes as 0).
4. Scope suffix `scopeN` (1-2 digits), written only if a scope is present and reticle index != 0.
5. Separator: `_` when the name starts with `iw5_`..`iw9_`, otherwise `+`. Example: `iw6_ak12_mp_acog_silencer_camo07`.
6. `alt_` prefix selects the alt weapon (`CD.altWeapon`, int index at CD+200).
7. Tokenizer re-attaches trailing `_mp`/`_sp`/`_so` to the base token.
8. Only weapons with `WD.oldWeapon == 0` (WD+0xa88) accept attachments.
9. Parse resolves each token by: scopes (max 6 scanned), then underbarrels (max 3), then others (max 7). First scope and first underbarrel kept; others set bits; global cap 4.

## 3. Attachment list construction (ordering)

`GetAttachments(weapon, isAlt) -> out[4]`, CONFIRMED:

- Normal order: scope, then `others` in ascending bit order, then underbarrel last. A 5th entry would overwrite slot 3.
- Alt handling: if `isAlt` and an underbarrel with `weapClass != 0` exists (or `CD.altWeapon != 0`): out[0] = that underbarrel. If its `shareAmmoWithAlt` (att+277) is 0, return at once (only that underbarrel). Otherwise clear slot and continue the normal build.
- A plain underbarrel (`weapClass == 0`) is appended normally.
- A class-overriding underbarrel is appended only if it has `adsSettingsMain != NULL`, or `adsSettingsScaleMain` not in {0, 1}, or (`shareAmmoWithAlt` and alt state valid).
- Separate simple list (used for shareAmmo checks): underbarrel, scope, others (max 4).
- weapClass getter: if a class-overriding underbarrel (or alt weapon) exists and any listed attachment has `shareAmmoWithAlt`, base class is returned. Otherwise first non-zero attachment `weapClass` wins. CONFIRMED.
- Underbarrel with weapClass 4 (SPREAD) and weaponType 0 is treated as a primary shotgun and isAlt is forced false. CONFIRMED.

## 4. Combination rules

| Rule | Behaviour | Where used |
|---|---|---|
| R1 override | First non-null attachment sub-block (in list order) replaces the base value wholesale | damage, ammunition, projectile, rumbles, ui, general, sight, addOns, locationDamage, adsOverlay, gunKick, viewKick, hipSpread, idle, ads blocks |
| R2 scale | Product over all attachments of a `*Scale` float; a value of 0 counts as 1.0 | scalars at att+224..268 |
| R1+R2 | Override then multiply by scale | clip/max/start ammo (ammunitionScale), gun kick (gunKickScale), view kick centre (viewCenterScale), hip spread (hipSpreadScale), idle (idleSettingsScale), ADS transition (adsSettingsScale) |
| Boolean | First attachment with non-zero value, else base | silenced, motionTracker |
| Enum | First non-zero attachment value | weaponType, weapClass |

Notes (CONFIRMED):
- Gun kick / view kick (hip and ADS, 5+5 floats each) are read from the first attachment with a gunKick/viewKick block, then every component is multiplied by the scale product; the interpolation between min and max uses the scaled endpoints. In ADS the sixth value in the block is the speed-clamp value; the hip-side clamp comes from the other group.
- Hip spread block field order: stand-min, stand-max, ducked-min, ducked-max, prone-min, prone-max (then fire-add and the rest). CONFIRMED.
- Idle sway: override then scale, times 0.75 when a playerstate flag is set.
- Damage ranges: override from `damage`, then `damageScale`/`damageScaleMin`, then ADS range scale (`adsDamageRangeScale`, 0 treated as 1).
- ADS block choice: `att.adsSettings` (+136) normally; `att.adsSettingsMain` (+144) when `shareAmmoWithAlt != 0` and not isAlt; for hybrid scopes (`adsOverlay.hybridToggle`) the player's toggle selects between them. ADS scale: `adsSettingsScale` (+248, <=0 means 1) normally; for the shared case 1.0 if `adsSettingsMain` exists else `adsSettingsScaleMain` (+252; 0 or 1 means 1). CONFIRMED.
- ADS transition rate = 1/(time*1000) per ms; defaults when time <= 0: 1/300 in, 1/500 out; multiplied by the ADS scale. CONFIRMED.
- Penetration: first attachment with `ammogeneral` gives penetrateType and multiplier; else WD+0x9c and CD+176. Result = multiplier * table[type*31 + surface]. CONFIRMED.
- Location damage: `att.locationDamage` (19 floats) replaces WD+0x928 multipliers; copy reorders file index to hit-location slot: dest 0-18 take file indices {0,1,2,3,4,5,6,9,7,10,8,11,12,15,13,16,14,17,18}; slots 19-21 from base. Reorder CONFIRMED, location names INFERRED.
- Impact FX: first attachment with `ammogeneral` overrides `CD.impactType` (CD+160); values 1-10 select impact tables. CONFIRMED.

## 5. WeaponAttachment layout (280 bytes, asset type 0x22)

| Off | Size | Field | Notes |
|---|---|---|---|
| 0 | 8 | szInternalName | ptr |
| 8 | 8 | szDisplayName | ptr, localized at load |
| 16 | 4 | type | INFERRED 0 scope, 1 underbarrel, 2 other |
| 20 | 4 | weaponType | first non-zero wins |
| 24 | 4 | weapClass | first non-zero wins |
| 32 | 8 | worldModels | ptr to 64-entry model array indexed by camo |
| 40 | 8 | viewModels | ptr to 64-entry array indexed by camo |
| 48 | 8 | reticleViewModels | ptr to 16-entry array |
| 56 | 8 | ammogeneral | AttAmmoGeneral (32) |
| 64 | 8 | sight | AttSight (9) |
| 72 | 8 | reload | AttReload (2) |
| 80 | 8 | addOns | AttAddOns (3) |
| 88 | 8 | general | AttGeneral (40) |
| 96 | 8 | aimAssist | AttAimAssist (12) |
| 104 | 8 | ammunition | AttAmmunition (24) |
| 112 | 8 | damage | AttDamage (28) |
| 120 | 8 | locationDamage | AttLocationDamage (76) |
| 128 | 8 | idleSettings | AttIdleSettings (24) |
| 136 | 8 | adsSettings | AttADSSettings (64) |
| 144 | 8 | adsSettingsMain | AttADSSettings (64) |
| 152 | 8 | scopeDriftSettings | AttScopeDriftSettings (24) |
| 160 | 8 | scopeDriftSettingsMain | AttScopeDriftSettings (24) |
| 168 | 8 | hipSpread | AttHipSpread (48) |
| 176 | 8 | gunKick | AttGunKick (88) |
| 184 | 8 | viewKick | AttViewKick (48) |
| 192 | 8 | adsOverlay | AttADSOverlay (64) |
| 200 | 8 | ui | AttUI (32) |
| 208 | 8 | rumbles | AttRumbles (16) |
| 216 | 8 | projectile | AttProjectile (144) |
| 224 | 4 | ammunitionScale | R2 |
| 228 | 4 | damageScale | R2 |
| 232 | 4 | damageScaleMin | R2 |
| 236 | 4 | stateTimersScale | R2, non-fire timers |
| 240 | 4 | fireTimersScale | R2, fire and melee timers |
| 244 | 4 | idleSettingsScale | R2 |
| 248 | 4 | adsSettingsScale | R2 |
| 252 | 4 | adsSettingsScaleMain | R2 |
| 256 | 4 | hipSpreadScale | R2 |
| 260 | 4 | gunKickScale | R2 (att+0x104) |
| 264 | 4 | viewKickScale | R2 (att+0x108) |
| 268 | 4 | viewCenterScale | R2 |
| 272 | 4 | loadIndex | assigned at load = enumeration index (max 96 assets) |
| 276 | 1 | hideIronSightsWithThisAttachment | underbarrel/class-override only |
| 277 | 1 | shareAmmoWithAlt | |

Sub-structs (offsets within struct, CONFIRMED from layout.rs):
- AttAddOns: motionTracker 0, silenced 1, riotShield 2.
- AttSight: aimDownSight 0, adsFire 1, rechamberWhileAds 2, noAdsWhenMagEmpty 3, canHoldBreath 4, canVariableZoom 5, hideRailWithThisScope 6, useScopeDrift 7, useDualFOV 8.
- AttAmmunition: maxAmmo 0, startAmmo 4, clipSize 8, shotCount 12, reloadAmmoAdd 16, reloadStartAdd 20.
- AttDamage: damage 0, minDamage 4, meleeDamage 8, maxDamageRange 12, minDamageRange 16, playerDamage 20, minPlayerDamage 24.
- AttGeneral: boltAction 0, inheritsPerks 1, reticleSpin45 2, enemyCrosshairRange 4, reticleCenter 8, reticleSide 16, reticleCenterSize 24, reticleSideSize 28, moveSpeedScale 32, adsMoveSpeedScale 36.
- AttAmmoGeneral: penetrateType 0, penetrateMultiplier 4, impactType 8, fireType 12, tracerType 16, rifleBullet 24, armorPiercing 25.
- AttADSOverlay: overlay block 0-55 (mirrors WD+0x5b0), hybridToggle 56, thermalScope 57, thermalToggle 58, outlineEnemies 59.
- AttADSSettings (floats): adsSpread 0, adsAimPitch 4, adsTransInTime 8, adsTransOutTime 12, adsReloadTransTime 16, adsCrosshairInFrac 20, adsCrosshairOutFrac 24, adsZoomFov 28, adsZoomInFrac 32, adsZoomOutFrac 36, adsFovLerpTime 40, adsBobFactor 44, adsViewBobMult 48, adsFireRateScale 52, adsDamageRangeScale 56, adsFireAnimFrac 60.
- AttReload (2 bytes), AttRumbles (16), AttUI (32; contains ammoCounterClip), AttIdleSettings (24), AttScopeDriftSettings (24), AttHipSpread (48), AttGunKick (88), AttViewKick (48), AttLocationDamage (76), AttProjectile (144), AttAimAssist (12): see `crates/fastfile_iw6/src/layout.rs`.

Enums (CONFIRMED): weapType NONE 0, BULLET 1, GRENADE 2, PROJECTILE 3, RIOTSHIELD 4. weapClass RIFLE 0, SNIPER 1, MG 2, SMG 3, SPREAD 4, PISTOL 5, GRENADE 6, ROCKETLAUNCHER 7, TURRET 8, THROWINGKNIFE 9, NON_PLAYER 10, ITEM 11. PenetrateType NONE 0, SMALL 1, MEDIUM 2, LARGE 3.

## 6. WeaponCompleteDef (296 bytes) attachment-relevant fields

szInternalName 0, weapDef 8, szDisplayName 16, hideTags 24 (null-terminated int list), scopes 32, underBarrels 40, others 48 (arrays of attachment pointers; capacities 7 / 3 / 7 as parsed), szXAnims 56 (124 ptrs), numAnimOverrides 64 / animOverrides 72, numSoundOverrides 80 / soundOverrides 88, numFXOverrides 96 / fxOverrides 104, numReloadStateTimerOverrides 112 / reloadOverrides 120, numNotetrackOverrides 128 / notetrackOverrides 136, fAdsZoomFov 144, iAdsTransInTime 148, iAdsTransOutTime 152, iClipSize 156, impactType 160, iFireTime 164, iFireTimeAkimbo 168, penetrateMultiplier 176, fAdsViewKickCenterSpeed 180, fHipViewKickCenterSpeed 184, szAltWeaponName 192, altWeapon 200 (int), iAltRaiseTime 204, killIcon 216, dpadIcon 224, fireAnimLength 232, iFirstRaiseTime 240, motionTracker 280, enhanced 281, szAdsrBaseSetting 288.
WeaponDef (2712 bytes) fields touched by attachments: gunXModel 0x8 (64 entries), camo* 0x18-0x38, szXAnimsRightHanded 0x40, szXAnimsLeftHanded 0x48, weapType 0x94, weapClass 0x98, penetrateType 0x9c, flash FX 0xb8/0xc0/0xc8, fire sounds 0x108/0x110/0x118/0x140/0x148, shell/eject FX 0x260-0x280, overlay 0x5b0, locationDamageMultipliers 0x928, fireRumble 0x930, meleeImpactRumble 0x938, tracerType 0x940, silenced 0xa6a, thermalScope 0xa7b, outlineEnemies 0xa7d, doNotAllowAttachmentsToOverrideSpread 0xa84, oldWeapon 0xa88, stowTag 0xa8c, stowOffsetModel 0xa90, worldModel 0x348, worldKnifeModel 0x368. All CONFIRMED against layout.rs.

## 7. Override tables (on WeaponCompleteDef)

### 7.1 Match code (CONFIRMED; resolves the earlier doubt in docs/IW6.md)
Each active attachment slot yields a u16 code: scope index 1-7 as is; underbarrel index * 8 (8, 16, 24); "other" bit i gives `0x20 << i`. Slot order: scope, underbarrel, then others ascending.
Entry fields `attachment1` (u16 @+0) and `attachment2` (u16 @+2). Score = number of entry fields equal to some slot code (0-2). Highest score wins, ties go to the first entry, score 0 never selects. Quirk: the anim resolver does not guard `attachment1 == 0`; sound/FX/anim-time paths do.

### 7.2 SoundOverrideEntry (32 bytes)
attachment1 +0, attachment2 +2, overrideSound +8, altmodeSound +0x10, soundType +0x18. Pointer chosen: `altmodeSound` when isAlt, else `overrideSound`. If no entry, use WD sound. soundType values used (CONFIRMED): 1 fireSound (WD 0x108), 2 fireSoundPlayer (0x110), 3 fireSoundPlayerAkimbo (0x118), 4 fireLastSound (0x140; player variant 0x148 follows type 4).

### 7.3 FXOverrideEntry (32 bytes)
Same header as sound; overrideFX +8, altmodeFX +0x10, fxType +0x18. fxType (CONFIRMED): 1 viewFlashEffect (WD 0xb8), 2 worldFlashEffect (0xc0), 3 viewShellEjectEffect (0x260), 4 worldShellEjectEffect (0x268), 5 viewLastShotEjectEffect (0x270), 6 worldLastShotEjectEffect (0x278), 7 viewMagEjectEffect (0x280), 8 viewFlashADSEffect (0xc8). Flash tag: `tag_flash_silenced` when dvar `useTagFlashSilenced` (default true) and weapon silenced, else `tag_flash`.

### 7.4 AnimOverrideEntry (40 bytes)
attachment1 +0, attachment2 +2, overrideAnim +8, altmodeAnim +0x10, animTreeType +0x18, animTime +0x1c, altTime +0x20. 124 anim types (NUM_WEAP_ANIMS 0x7c). The resolver builds two 124-entry arrays (normal, alt) starting from `CD.szXAnims`, then for each animTreeType applies the best-scoring entry. When the akimbo/dual-wield flag is set it copies WD right-handed and left-handed anim arrays whole. CONFIRMED.
`animTime`/`altTime` (alt when isAlt) of the best entry REPLACES the base state timer for that anim, then is multiplied by `fireTimersScale` (fire/melee timers) or `stateTimersScale` (all others), and by 0.65 when a playerstate flag (+0xe14 bit 0x17) is set. CONFIRMED.
Anim type to timer (CONFIRMED): 6 rechamber, 7 primeTime, 8 holdPrime, 9 melee, 0x10 reload, 0x11 reloadEmpty, 0x12 reloadStart, 0x13 reloadEnd, 0x18 raise, 0x19 firstRaise, 0x1a breachRaise, 0x1b drop, 0x1c altRaise, 0x1d altDrop, 0x22 quickRaise, 0x23 quickDrop, 0x24 emptyRaise, 0x25 emptyDrop, 0x26 sprintIn, 0x28 sprintLoop, 0x29 sprintOut, 0x2a stunBegin, 0x2b stunLoop, 0x2c stunEnd, 0x33 blastFront, 0x34 blastRight, 0x35 blastBack, 0x36 blastLeft, 0x37 slideIn, 0x38 slideLoop, 0x39 slideOut.

### 7.5 Reload and notetrack overrides
ReloadStateTimerEntry (16 bytes): attachment +0, reloadAddTime, reloadEmptyAddTime, reloadStartAddTime. No exe reader found (may be unused, or consumed elsewhere). Notetrack overrides: no reader found. INFERRED unused in MP.

## 8. Models and tags

- Per-camo model arrays: `att.viewModels` (att+40), `att.worldModels` (att+32), both 64 entries indexed by camo; `att.reticleViewModels` (att+48) 16 entries. CONFIRMED.
- Viewmodel DObj build order: base `WD.gunXModel[camo]` on `tag_weapon`; then attachment models appended with NO parent tag in order scope `viewModels[camo]`, reticle model, underbarrel, others ascending; then `tag_clip` mag; then hands model (tag = `WD.knifeAttachTagOverride` if non-zero else `tag_weapon_left`/`tag_weapon_right` per hand). Reticle model only added if global flag 0x1418213a8 is 0; attachments skipped if flag 0x1418213aa is non-zero (meaning unknown). CONFIRMED order, INFERRED that null parent tag means bones are matched by name against the base skeleton.
- Camo fallback: if `gunXModel[camo]` is NULL, camo != 0 and `camoMaterialTarget[camo-1]` non-null, use model 0; `camoViewModel` replaces it if set; `camoViewModelMaterialOverride` plus the material target is applied. World uses `camoWorldModel` and `camoWorldModelMaterialOverride` with the same logic over `WD.worldModel[camo]` (0x348). CONFIRMED.
- World model: base WD.worldModel[camo], then attachments' `worldModels[camo]` in order scope, underbarrel, others.
- Player body: weapon on `tag_weapon_right` (or `tag_weapon_left` for the left-hand flag, `tag_inhand` for weapType 2), attachments follow with parent 0; stowed copy on `tag_stowed_back` (`tag_shield_back` for riot shields) using `WD.stowTag` (0xa8c), `stowOffsetModel` (0xa90) extra entry; `WD.worldKnifeModel` on `tag_inhand` when knife flag set. CONFIRMED.
- Reticle selection ("reticlehack"), CONFIRMED: reticle index = `weapon >> 26`. Index 0-15 uses `att.reticleViewModels[idx]` of the scope attachment. Index 16-31, 32-47, 48-63 use the standalone attachment assets named `reticlehack`, `reticlehack2`, `reticlehack3` (their `reticleViewModels[idx-16/-32/-48]`), cached on first lookup.
- Bone visibility on the viewmodel (CONFIRMED): bones `tag_sight_on`, `tag_sight_off`, `tag_rail`, `tag_foregrip`, `tag_bipods`, `tag_tip`. Hiding a bone hides children.
  - No scope: hide `tag_sight_off`, show `tag_sight_on`, hide `tag_rail`; if a class-overriding underbarrel has `hideIronSightsWithThisAttachment`, invert all three.
  - Scope: show `tag_sight_off`, hide `tag_sight_on`; show `tag_rail` unless `sight.hideRailWithThisScope`.
  - `tag_foregrip`/`tag_bipods` shown by default; hidden only when an alt-class underbarrel/alt weapon exists and no listed attachment has `shareAmmoWithAlt`.
  - `tag_tip` hidden when silenced (base `WD.silenced` or first attachment `addOns.silenced`).
  - Reticle bones hidden at build: `tag_reticle_acog`, `_red_dot`, `_tavor_scope`, `_thermal_scope`, `tag_eotech_reticle`, `tag_magnifier_eotech_reticle` (only the selected reticle model is shown).
  - `CD.hideTags` (int list) is applied via the same hide routine.
- Hidden-base-model rule: INFERRED that `hideTags` plus the bone toggles above are the only mechanism; no separate base-model-swap per attachment was found.

## 9. ADS overlay and sight stats
Overlay: first attachment with `adsOverlay` supplies the overlay block, else WD+0x5b0. Hybrid and thermal queries consider scope attachments only (type 0). `hybridToggle` selects the ADS block via the player toggle. thermalScope/outlineEnemies override (WD 0xa7b/0xa7d). Reticle centre/side textures and sizes come from `general` (first non-null). CONFIRMED.

## 10. CSV tables read by code

- `mp/attachmenttable.csv` (CONFIRMED): row looked up where **column 4** equals the attachment name; **column 2** is the attachment class string. Class string is checked in `mp/recipes.def` map `attachmentClassRestricted`; the attachment name itself in `attachmentRestricted`. "none" always allowed. Six slots checked (3 primary, 3 secondary). Missing table is a fatal error. Other columns are not read by the exe (script/UI only; meanings UNKNOWN).
- `mp/statsTable.csv`: col 0 ref name, col 2 weapon group, col 3 challenge name, col 4 weapon name. Unlock passes when `challengeState >= 2`; names containing `knifeonly` bypass. CONFIRMED.
- `mp/camo_reticle_unlockTable.csv`: col 0 key (`<group>_<camo>` for camos, `<attachment>_<reticle>` for reticles), col 1 challenge suffix (camo stat = `ch_<weapon><col1>`; reticle uses col1 directly), col 2 required integer count; unlocked if progress >= count. CONFIRMED.
- `mp/reticleTable.csv`: col 1 reticle name, col 6 integer index into playerdata `unlockedReticles`; an unlock grant sets that bit. CONFIRMED.
- Loadout validation: `weaponSetups[i]` = {weapon, buff, camo, attachment[3], reticle}. Sights allowing a reticle are those named acog, reflex, eotech, hybrid. If such a sight is attached the reticle must be unlocked via camo_reticle_unlockTable (or "none"); otherwise the reticle must be "none". CONFIRMED.

## 11. Evidence (decompile addresses, iw6mp64_ship_all)

Name/pack: BG_GetWeaponNameComplete 0x140239370; G_GetWeaponForName 0x1403da060; tokenizer 0x1403da4e0; attachment name lookup 0x1403d9c60; base lookup 0x140238ed0.
Lists/selection: attachment list 0x140240bf0; simple list 0x14023dc30; ADS block pick 0x140247e60; ADS scale 0x140247fc0; weapClass 0x140240e20; shotgun check 0x1402415a0; ADS rates 0x14023d820.
Overrides: sound 0x14023cbb0; FX 0x14023c9f0; anims 0x14023b890; anim time 0x1402481c0.
Getters (selection): clip 0x14023e0d0; max ammo 0x140242fb0; start ammo 0x1402460d0; damage 0x14023e260; ranges 0x14023e310; hip spread 0x14023f230/0x14023f460; idle 0x14023f050; gun kick 0x14023ed50; kick blend 0x1402472a0; penetration 0x140238fd0; location damage 0x14023f930; silenced 0x140242570; motion tracker 0x140241340; overlay 0x140240540.
Models: viewmodel DObj 0x1402aede0; model streaming 0x1402a5d90; reticle 0x1402a98f0; reticlehack lookup 0x1402aa120; worldmodel 0x1402656f0; player body 0x14014bf60; bone visibility 0x1402456e0; reticle bone hide 0x1402adff0; flash tag 0x140241040; impact FX 0x1402a9350; attachment asset load 0x1402aaab0.
CSV: attachmenttable 0x140651c20; camo unlock 0x1403fcf90; reticle unlock 0x1403fe5f0; reticleTable 0x140599ca0; loadout validation 0x14040a430; reflex() 0x1403fc780; knifeonly 0x140408390.
Layouts: `/home/claude/codlegacyclient/crates/fastfile_iw6/src/layout.rs` and `walk/weapon.rs` in the same crate.

## 12. Unknowns

1. Names of 93 of the 124 anim types (only 31 mapped via timers).
2. attachmenttable.csv columns other than 2 and 4.
3. Attachment `type` enum names (0/1/2 inferred).
4. Whether reloadOverrides and notetrackOverrides have any reader.
5. DObj bone-merge semantics for null-parent attachment entries (inferred name matching).
6. Meaning of flags 0x1418213a8 / 0x1418213aa in the viewmodel builder.
7. Source of the hybrid-toggle state (playerstate +0x2e4 list / +0x41c) and exact meaning of isAlt flag (playerstate +0x420 bit 0x80000).
8. Hit-location names for the 19-float location damage table.
9. Sprint timers and the type-0 filter in the scope-only getter.
10. Remaining WeaponDef regions (turret, notetrack maps, stateTimers detail).
