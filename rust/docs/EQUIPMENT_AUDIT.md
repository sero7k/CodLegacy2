# Black Ops II multiplayer equipment: sounds and effects audit

Scope: only Black Ops II's own assets. The weapon file (`WeaponDef`) names what
the engine plays; the engine's client scripts add a few more. Both are checked
by `asset_game::equipment_audit`.

## How to get the data-level table

The cloud box that wrote this had no Black Ops II zones staged, so the
per-weapon alias names are not in this file. Run the game with
`BO2MP_EQUIP_AUDIT=1` (and `CODLEGACY_AUDIO_EQUIPLOG=1` to see every
beep/bounce/explosion as it plays). Once weapons, sound bank and effects are
loaded it logs `equip audit: weapon | slot | asset | status` for every
sound slot, bounce surface, notetrack sound and effect of the 19 listed pieces
of equipment (`BO2_MP_EQUIPMENT`), the list of weapons the data lacks, and the
effects whose names look like each piece's lights. `MISSING` means the weapon
file names it and the bank/catalog does not have it: list those for the owner,
do not substitute another game's.

## Slots: is each one played?

| Slot (weapon file / script) | Event or code | Played? | Fixed here? |
|---|---|---|---|
| pullbackSound / Player (pin pull) | `EV_PREP_OFFHAND` (`audio::entity_events`) | yes | no change |
| fireSound / Player (throw) | `EV_USE_OFFHAND`; own view stays silent unless the file names a Player alias (first person is notetracks) | yes | no change |
| first-person notetracks (pin, throw, plant) | the note names its sound (`sndnt#alias`) or the weapon file's `notetrackSoundMap` does; unmapped notes are logged once | yes | **yes**: Black Ops II's map was ignored (its sounds are inline), and a map value may name the player's alias then the others' (`a_plr a_npc`), which no bank has as one name. Both fixed (`notetrack_value_aliases`); the audit lists every note of every equipment animation and says which carry no sound |
| bounceSound[surface] | `EV_GRENADE_BOUNCE` (`grenade_contact`) | yes | no change |
| stick (semtex, ballistic knife, axe in a wall or player) | `EV_GRENADE_STICK`, same observer | played, but always the default surface's sound | **yes**: the event now carries the surface (flesh for players) |
| projExplosionSound | `EV_GRENADE_EXPLODE` / `EV_FLASHBANG_EXPLODE` (`fx::system::explosion`); stock fallback for frag, semtex, crossbow bolt and shock charge | yes | consts moved to `equipment_audit` |
| detonateSound / Player (C4 detonator key) | `EV_DETONATE` | **no**: reached the audio observer and was dropped as "no handler" | **yes**: slots read, prepared, played |
| projIgnitionSound | missile state tick | yes | no change |
| projectileSound (flight loop) | `audio::projectile_loops` | **yes** (second batch): a looping emitter rides each in-flight projectile whose weapon file names the alias, moves with it, and stops when the shot lands, sticks or goes off; lists reused, no per-frame allocation | **yes** |
| deploySound / Player, finishDeploySound / Player | `EV_DEPLOY_EQUIPMENT`, `EV_FINISH_DEPLOY_EQUIPMENT` (`sim::equipment::settle_equipment`, `think_projectile`) | **yes**: deploy when a thrown item comes to rest (stick or settle), finish when its `deployTime` is up (at once if 0); the owner hears the `...Player` alias when the file names one, others the world alias at the item | **yes** |
| breakdownSound / Player, finishBreakdownSound / Player | `EV_BREAKDOWN_EQUIPMENT`, `EV_FINISH_BREAKDOWN_EQUIPMENT` (`equipment::note_breakdown`, from `script::host::weapons::settle_projectiles`) | **yes, approximated**: raised when a script retires a placed item that lay at rest (pickup by its owner, or the script's own removal); both at once because the script removes it in one step | **yes** |
| projDudSound | `EV_PROJ_DUD` (`think_projectile`, the `Dud` geometry: a shot that hit before its arming distance) | **yes** | **yes** |
| fireStart/KillcamSound, rotate/spin sounds (turrets), nightvision sounds | none | not read: not equipment slots | open, out of scope here |
| projTrailEffect / projIgnitionEffect | `tick_missile_present_state` (bolted to the projectile) | yes if bound (the audit now lists both) | no change |
| projExplosionEffect (+ impact table) | `explosion` | yes | no change |
| projExplosionEffect2 to 5 | `explosion` (`WeaponCombatFx::explosion_extra_present`) | **yes**: play with the first, at the same place and orientation | **yes** |
| projDudEffect | `dud_effect` (`EV_PROJ_DUD`) | **yes** | **yes** |
| shellshock tinnitus loop / end / abort (flash, concussion) | `audio::shellshock` | played from the MW2 bank | **yes**: BO2 bank first |

## Script-supplied sounds and lights (client scripts, run as Rust rules)

| Item | What | Source | Hooked? |
|---|---|---|---|
| Semtex, crossbow bolt | beep `wpn_semtex_alert`, 0.3 s shrinking /1.2 to 0.08 s | `_sticky_grenade.csc` | yes (`equipment_lights`) |
| Semtex | blink light, green own/team, red enemy | same | yes |
| C4 | steady light green/red | `_satchel_charge.csc` | yes |
| Bouncing Betty | steady light green/red | `_bouncingbetty.csc` | yes |
| Shock Charge | scan light; mask crackle `wpn_taser_mine_tacmask` on the shocked player | `_proximity_grenade.csc` | yes |
| Shock Charge | electrocution effect on the victim | `_proximity_grenade.csc` | **no** (only the sound); candidate effect names come from the audit log |
| Claymore, Trophy System, Sensor grenade, Camera Spike, Motion Sensor, Scrambler, Tactical Insertion, Nightingale | client-script lights and effects | their `.csc` | **not run**; the audit log lists effects whose names look like them. Server scripts' `playsound` / `playloopsound` / `playfx` natives are bound, so their scripted sounds play when the aliases exist |

### What is missing, for the owner (second batch)

A search of the repo and docs found **no Black Ops II client script source or
decompile** (grep for `_claymore`, `_trophy`, `_shockcharge`,
`_proximity_grenade`, `_bouncingbetty`, `_acoustic`, `_camera_spike`,
`_scrambler`, `_nightingale`, `fx` tables): the only client-script knowledge
here is what the first batch wrote into `equipment_lights.rs`, and the repo
links server scripts only (`gsc_t6::program`: "client `.csc` objects are
skipped"; BO3's `.csc` translator is `gsc_t7`, not the Black Ops II one). So
nothing was implemented from guesses. What the audit now prints, in the
`equip audit:` lines at load (`BO2MP_EQUIP_AUDIT=1`):

- `<weapon> | client script (<what it adds>) | hooked in Rust` or `NOT RUN` |
  `in the loaded zones: <the compiled client scripts whose path mentions the
  item>` or `no script found` (`equipment_audit::client_script_report`; the
  zones keep compiled scripts as `script:<path>` raw files). Send the lines
  for the `NOT RUN` items, and the decompiled `.csc` of each, and the lights
  and effects can be added as data rules in `equipment_lights.rs`.
- the effects in the catalog whose names look like each item (existing).
- per weapon `Anim note <name>` rows: every notetrack of the equipment's
  animations, with its sound resolved, `MISSING`, or `note carries no sound`.
  A pin pull or throw note in the last group is a gap.

Still not done, and not guessable: the Shock Charge victim's electrocution
effect (`_proximity_grenade.csc`), and the lights/effects of the Claymore,
Trophy System, Sensor grenade, Camera Spike, Motion Sensor, Scrambler,
Tactical Insertion and Nightingale. The deploy / breakdown sounds of a
*placed turret* (a killstreak, not a thrown item) are not raised: the turret
scripts own those states.

## Items

All 19 pieces use the same generic rows above: Frag `frag_grenade_mp`,
Semtex `sticky_grenade_mp`, Combat Axe `hatchet_mp`, C4 `satchel_charge_mp`,
Claymore `claymore_mp`, Bouncing Betty `bouncingbetty_mp`, Concussion
`concussion_grenade_mp`, Flash `flash_grenade_mp`, EMP `emp_grenade_mp`, Sensor
`sensor_grenade_mp`, Shock Charge `proximity_grenade_mp`, Smoke
`willy_pete_mp`, Nightingale `nightingale_mp`, Trophy `trophy_system_mp`,
Tactical Insertion `tactical_insertion_mp`, Motion Sensor `acoustic_sensor_mp`,
Camera Spike `camera_spike_mp`, Scrambler `scrambler_mp`, Ballistic Knife
`ballistic_knife_mp`. The weapon names are as recalled; a name the data lacks
is reported as absent, not guessed.

## Victim effects

Flash and concussion apply the real `shock/*.shock` files (screen, look,
movement, view kick, tinnitus). The tinnitus alias is now looked up in the BO2
bank. EMP and sensor effects are server scripts. Grenade warning audio was
restored earlier (`T6 - grenade warning back`).

## Events added in the second batch

`EV_DEPLOY_EQUIPMENT` 0x06, `EV_FINISH_DEPLOY_EQUIPMENT` 0x07,
`EV_BREAKDOWN_EQUIPMENT` 0x08, `EV_FINISH_BREAKDOWN_EQUIPMENT` 0x09 and
`EV_PROJ_DUD` 0x0d (numbers free in the table; all `GrenadeContact` events,
heard from the payload's origin so a projectile whose slot is gone still
plays). Each carries the weapon, the owner (`attacker_entity_num`) and the
item's origin. `deployTime` is read into `EquipmentRuntimeFacts`.

## Tests

`cargo test -p asset_game --lib equipment_audit` (list, every slot audited and
prepared, bounce/notetrack/effect checks, extra effects, notetrack
classification, client script report), `cargo test -p audio --lib` (every
equipment event has a slot; the placing events; the owner's alias choice; the
flight-loop reconcile) and `cargo test -p sim --lib equipment` (stick surface,
flesh = 7 against `weapon_iw4::SURFACE_TYPE_NAMES`, finish-deploy edge).
Nothing here was run against Black Ops II data or heard.
