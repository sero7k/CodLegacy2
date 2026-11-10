# Black Ops II mechanics: status

The live path, what is read from Black Ops II's own data, and what is still
open. Technique definitions and the cross-game matrix are in
[TRICKSHOTS.md](TRICKSHOTS.md).

## Live path (where to change things)

| System | Code | Black Ops II data |
|---|---|---|
| Movement | `sim/src/step.rs` `run_players_system` → `bo2_feel` / `pmove_context` → `movement_iw4::pmove` (`feel.on` branches in `feel.rs`, `dive.rs`, `friction.rs`, `walk.rs`, `stance.rs`) | ~10 dvars when scripts set them; the rest are literals (see Open) |
| Weapons | `sim/src/combat.rs` `advance_weapon_command` → `weapon_iw4::weapon_hands` (`tick.rs`, `weapon_change.rs`, `reload.rs`, `sprint.rs`, `offhand.rs`) | `asset_game/src/weapon_catalog_t6.rs` `t6_body_facts` (WeaponDef / WeaponVariantDef) |
| Prediction | `net/src/client/predict.rs` runs the same `sim::try_step` | `Bo2Feel` comes in every snapshot |

A multiplayer map started outside the lobby without `BO2MP_SCRIPTS=1` runs
MW2's rules; the log now warns when that happens.

## Status

| Item | Implemented | Tested | Verified vs native BO2 |
|---|---|---|---|
| Instaswap (offhand during drop finishes the switch) | yes | weapon_iw4 | no |
| BO2 switch rules on a joining PC's prediction (were host-only, and leaked into later non-BO2 matches) | yes (per step from `Bo2Feel`) | build | no |
| Select Fire: `<gun>+sf` and `<gun>+<sight>+sf` with BO2's `sf_<gun>_mp` alternates (fire type, fire / burst times, alternate raise / drop 0 on most); toggled by the alternate-weapon slot | yes | build; `t6_sf_probe` lists the 15 pairs | no |
| `iFirstRaiseTime` (spawn), `iEmptyRaiseTime`, `iEmptyDropTime`, `iLastFireTime` | yes | weapon_iw4 `bo2_timer_tests` | no |
| Dive get-up at the usual stance speed (stale test fixed) | yes | movement_iw4 feel_probe | no |
| NAC / reload NAC (returning to a dropping weapon readies it; a switch clears a pending rechamber) | yes (from the switch code) | weapon_iw4 | no |
| YY | follows the switch path; no code of its own | no | no |
| Non-sight attachment scales (rf, dualclip, fastads, extclip, stalker, ...) | **no** on Black Ops II's guns (base / sight variant stands in); **yes** on guest guns' variants (`Bo2AttachmentStats`: magazine, fire time, reload, damage range, ADS time and move speed, hip spread, ADS kick centring and idle, suppressed: off the minimap) | build | no |
| Anti-quickscope fields (`fAntiQuickScope*`) | no | — | — |

## Open (ranked)

1. **Non-sight attachments.** BO2 combines a gun's attachments at run time;
   here each catalog weapon is one combination. Needs either per-player
   attachment scales applied to the facts (and carried to prediction) or
   generated variants. Data: `WeaponAttachment` / `WeaponAttachmentUnique`
   in `common_mp.ff` (`t6_attachment_probe`).
2. **Movement numbers in one table**, defaults from Black Ops II's dvar
   registrations (sprint time, speed scales, jump height, friction, stop
   speed, accelerations, stance scales), seeded as VM dvars so scripts can
   override them. Today they are literals in `step.rs:2344-2403`,
   `friction.rs`, `walk.rs`, `stance.rs` (IW4's values, unchecked).
3. `perk_weapReloadMultiplier` read from the dvar (fixed at 0.5 in
   `combat.rs`).
4. A sim-level test that steps `run_players_system` with BO2's scripts
   loaded (the movement tests build their own context and bypass it).
5. In-game verification against native Black Ops II footage for every row
   above.

## His trickshot script (Tricky modes)

`sim/src/t6/scripts/trickshot/main.gsc`, kept as he wrote it for Plutonium
(`t6/trickshot.rs` binds Plutonium's additions). What its rules need from
the engine:

| Rule | Engine side |
|---|---|
| Lowered barriers (`manageBarriers` → `moveTrigger`) | Its test `trigger.origin[2] < level.origin[2]` fails (`level` has no origin); Plutonium runs the `if` anyway, so does `gsc_t6` now (`GSC-RUNTIME.md`). A moved `trigger_hurt` hurts where it now is (`hurt.rs` tests its brushes at the entity's origin). |
| Sky barriers (`removeskybarrier`) | `getentarray()` lists every map entity with its `classname`; triggers above z 180 go to (0, 0, 9999999). |
| Platforms under the teleport flags (`addcrateLocation`, `addPlatformLocation`) | A script model is solid as its model's collision map (`XModel::collmaps`, read by `asset_t6::read_collmaps`): `collision_clip_64x64x10` is a 64 x 64 x 10 slab about its origin (its drawn placeholder is a 64-unit cube, which is what stood before: the flag put him 30 units inside it and he fell through). Only pieces that stop a player count (`collision_physics_*` does not); care packages and other script models with a collision map are solid the same way. Every `collision_*` model is in the match's model library. A player teleported a few units into one is lifted onto it (`step.rs` `unstick_feet`). |
| `iprintln` / `iprintlnbold` | Reach his screen (`natives_mp::print` → `bo2mp_prints` → `ui/hud_prints.rs`): every toggle's message, the binds, the distance of the last kill. |
| `bg_prone_yawcap 360`, `jump_ladderPushVel 1024` (and the Ladder Mod toggle) | `Bo2Feel::prone_yaw_cap` / `ladder_push_vel`, sent to prediction (protocol 88). |
| `bulletrange 65536` | Every bullet but a shotgun pellet reaches that far (`combat.rs`). |
| Always Canswap (`seteverhadweaponall(0)`) | Each gun he switches to makes its first raise again. |

Not done: `sv_enablebounces` (Plutonium's bounces; no bounce in our
movement), `bg_ladder_yawcap` (no ladder yaw cap at all here), a brush
collision map that is not a box (`collision_clip_ramp`) stands as its
bounds. The script's own slips, left as he wrote them: `hkcmd` waits for
`hunterkillernotify` (the bind sends `hknotify`) and gives an RC-XD;
`endgamecmd` is never started; `player_meleeRange ` has a trailing space;
`setupCustomMap`'s `mp_downhill`, `mp_dig`, `mp_frostbite` and `mp_skate`
cases fall through into the next map's flags; the first toggle of Smooth
Anims and Drone Ele reports OFF (`undefined == 0` is false); Ladder Mod
Toggle chains `setDvar(...) && ...` (setDvar returns nothing), so it prints
nothing and always leaves the push at 1024.
