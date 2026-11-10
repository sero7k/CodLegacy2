# Optional M.O.A.B. (bo2mp)

An optional streak after Modern Warfare 3's M.O.A.B. It is **off by
default**, so native Black Ops II stays untouched. To turn it on, open the
lobby's SETTINGS tab and set **M.O.A.B.** to **25 / 20 / 15 / 30 KILLS**,
or set `BO2MP_MOAB=<kills>`.

Script: `crates/sim/src/t6/scripts/moab.gsc`. It is OUR CODE, compiled at
load like `_globallogic`.

## Where it comes from

| Part | Source |
|---|---|
| 10 s countdown with a ticking timer; incoming sound 3.3 s before the blast; blast sound and flash vision; deaths 1.5 s after the blast; aftermath vision | **Verified:** Modern Warfare 2's `maps/mp/killstreaks/_nuke.gsc` (plain text in MW2's common_mp) |
| Match goes on, enemies die, enemy team is EMP'd | Modern Warfare 3's MOAB. MW3's scripts ship compiled on PC, so these are its published rules, not its code. |
| EMP | Black Ops II's own `_emp::emp_use`. Its aftermath already restores `level.nukevisionset`. |
| Kill count (25), hold [Use] for 1 s to call it in | **Design.** It is earned per kill streak in one life, so it stays outside Black Ops II's three scorestreak slots. |
| Visions `mpnuke` and `mpnuke_aftermath` | **Design:** our values in the IW film form, modelled on MW2's look |
| Icon | Modern Warfare 2 Remastered's (Horizon MW) `dpad_killstreak_nuke`, from `hmw_killstreak.ff` / `.pak`; the sheet's steady lit frame (`fastfile_h1::ui_image::read_hmw_killstreak_icon`) |
| Voice | Modern Warfare 2 Remastered's announcer (`PC_1mc_use_tnuke`, `_enemy_tnuke`, `_achieve_nuke`, in `hmw_dlc_killstreaks.ff`); Ghosts' KEM voice is not used. The other factions' lines (US, UK, ...) are in the retail `common_mp` and are not read yet. |
| Sounds | Ghosts' KEM countdown alarm, launch, incoming and blast sounds, plus Black Ops II's own blast; Modern Warfare 2 (2009)'s sounds are no longer used (left out of the game) |

## Settings (dvars)

| Dvar | Default | Meaning |
|---|---|---|
| `scr_moab` | 0 | 1 allows the M.O.A.B. |
| `scr_moab_kills` | 25 | Kills in one life to earn it |
| `scr_moab_timer` | 10 | Countdown in seconds |
| `scr_moab_emp` | 1 | 1 EMPs the enemy team after the blast |
| `scr_moab_endsgame` | 0 | 1 ends the match, as MW2's tactical nuke does |

## Not done yet

- **Slow motion:** `setslowmotion` is a no-op in this engine.
- **Flash and aftermath effects:** MW2's are left out (`player_death_nuke_flash`, `nuke_aftermath_mp`).

## The blast and the map's events, the match's end (5 Nov 2026)

- The blast ends the match in the caller's victory by default (`scr_moab_endsgame 1`; `0` lets the match go on): the caller's team in a team game, the caller in a free-for-all.
- On a Ghosts map the map's own scripts are told: at the blast `moab.gsc` raises the match flag `bo2mp_moab` (bit 0x3e) for 0.2 s and the Ghosts script host (`sim::t6::iw6_script`) notifies the level `nuke_death`, once, as Ghosts' own nuke does at its blast (read from its nuke script). What waits on it is the map's own: Favela's `nuke_custom_visionset` (its death vision loop) is the one found in the staged zones (mp_frag and mp_snow have nothing waiting). The other maps' events could not be read (their zones are not staged); they run if they wait on `nuke_death`. Ghosts' other nuke notifies (`nuke_flash`, `nuke_aftermath`) are not sent. A map that is not Ghosts' has no script host, so only the match's end applies there.

## A Ghosts map's own nuke event, and everyone dies (6 Nov 2026)

- Calling the MOAB first asks the map. `moab.gsc` raises `bo2mp_moab_call` (bit 0x3d); the Ghosts script host calls the map's `level.nukeoverride` hook (field 10679) once and answers in `bo2mp_moab_handled` (bit 0x3c). Strikezone has one (its teleport event, "destroyed"); Favela and the other staged maps have none, so they get the normal countdown, blast and `nuke_death`.
- When the map handled it, the script still ends the match in the caller's victory, with everyone but the caller dead (`moab_map_nuke`, `moab_deaths`); otherwise the blast kills all players but the caller as before.
- To let the hook run, the host boots a few things Ghosts' own game type would: the nuke init (nuke script, token 1553), the level event flags, the participants array, the teleport zones with the "start" zone, the chopper-boss path arrays (empty), a do-nothing game-start hook that the teleport system wraps. The VM answers the agent utility's "all agents" with an empty array (that script is in no staged zone) and the path-node count/list builtins with nothing.
- Test: `cargo test -p sim --lib -- strikezone_has` (hook found, answers handled, 400 ticks of the event with no script error; Favela has no hook).
