# MWR (h1) scriptables and map scripts: exe notes (6 Nov 2026)

Addresses and decompiler names are allowed here only. Static analysis of `h1_mp64_zt.exe` (Ghidra 11.4.2 headless,
imported and analysed in about 15 minutes on 2 cores; project not kept). READ = seen in the decompile.

## Finding the code

The strings of the scriptable errors have no data references in Ghidra's database (the code reaches them with
RIP-relative operands it did not mark). `RipRef.java` (scan executable blocks for a rel32 that lands on an address)
found them: "Unknown scriptable notetrack type" is used by `FUN_14042dab0`, "Trying to link a reserved scriptable"
by `FUN_14042d610`, "Too many server-side scriptable replicated parts" by `FUN_1404354f0`. The server-side
scriptable system is `0x14042c000..0x140437000`.

## Instances (READ)

* Array `DAT_149811858`, stride 0x98 (= `ScriptableInstance`), `DAT_149811868` per snapshot slot, count
  `DAT_149811870`. Allocated and filled from the map's list in `FUN_14042c610` (event constants, part states and
  event stream are carved from one run-time buffer: part states 4 bytes a part, `eventStreamSize`, then 0x44 bytes
  a target for lights).
* An instance's entity number is at +0x96. Entity numbers 0x12..0x29 and 0x32..0x39 map to instance numbers
  (`n - 0x12` and `n - 0x1a`), anything else is 65000 (not a scriptable).
* `#snd_` in a notetrack name makes it a sound notetrack (`FUN_14042c800`); a notetrack's data type 0 is an
  effect, 1 a sound, else the error above (`FUN_14042dab0`).

## Damage (READ)

* `FUN_14042c440`: builds the damage context (instance, entity numbers of attacker and inflictor, point,
  direction, amount, means of death at +0x5c, flags) and calls `FUN_140432c70`.
* `FUN_140432c70`: means of death 3..7 or 0x10 (explosions) go to the radial path `FUN_1404321e0`. Else the hit
  part is found (`FUN_1404325e0`); when that part's current state has a non-zero parent transfer rate (byte at state
  +0x12, scaled by 1/255, constant at `DAT_140811418`) the whole (part 0) is hurt by that share first
  (`FUN_140432100`), then the hit part is hurt, and a hit on part 0 itself goes to `FUN_140432070` (not followed).
* `FUN_140432100`: damage = max(1, int(amount * share)). Kind mask: 1, or 4 when the means is 8; plus 8 when the
  attacker entity number is below 0x12 (a player), else 0x10 when the attacker's entity type is 1 or 0x12, else
  0x20. Calls `FUN_140431e70(context, kind, damage)`.
* `FUN_1404321e0` (radial): the same masks with `| 2`, one damage per part, scaled by the part's distance and
  visibility (`FUN_140431710`, 0x20 parts at most). The scale curve was not followed.
* `FUN_140431e70` (the state machine): part state is 4 bytes (u16 health; byte at +2 = current state in the high
  nibble, last executed state in the low nibble). The hit is ignored when health is 0 or when the current state's
  damage flags (byte +0x11) share no bit with the kind in the low three (mask 7) **and** none in the next three
  (mask 0x38). New health = health - damage. At 1 or more it is stored. Else, repeat: enter the next state (high
  nibble + 1; callbacks before and after), take the state's health (u16 at +0x14; 0 stays 0), add it to the signed
  remainder (the stored health is the remainder clamped at 0, the remainder itself carries on), stop when the new
  state is the last one, and when the new state does not take the kind give it its full health; loop while the
  remainder is below 1. Translated in `fastfile_h1::scriptable_run::Machine::damage_part`.
* Entering states: `FUN_14042df20` runs the events of the states between the low and the high nibble
  (`FUN_1404301e0`), then `FUN_1404313c0` for the timed events.
* Health drain tick `FUN_1404335c0` (event, instance, time): the stream slot holds the next time; when the time has
  come the slot grows by the interval (event +10) and `FUN_140431e70(instance, 0x3f, amount (event +8))` hurts the
  part itself with every kind. Burning cars use this.
* Not followed: `FUN_140432070` (a hit on the whole), the radial scale curve, the per-event executor's details
  (explosion damage curve, physics launch speeds, light/translate/rotate easing).

## Data on Crash (`h1_scriptables`, READ from the zone)

18 instances of 6 defs: `destpv_civ_80s_sedan1_h1_{brn,green,red}` (5 + 5 + 4 cars, 20 parts each), a bowl of fruit,
two fluorescent lights (`h1_lightfluo_flicker_mp_crash`) and a fire light. Dynamic entity kinds: 630 clutter (1),
15 destructible (2), 2 of kind 4 and 99 of kind 5 (a scriptable's body and its loose parts, as in Ghosts).

The car's whole (`root`): 250 intact (flags 0x3b), 200 white smoke, 100 black smoke, 300 burning (flags 0x0b: only
a player's hit hurts it, the drain hurts it with every kind), 400 burning, 0 destroyed. The burning states run the
fire effect, the burning sound and a drain of 12 every 200 ms; the destroyed state swaps the model, plays the
explosion effect and sound, an animation and an explosion (force 4000..5000, radius 375, damage 20..300). Parts
(hood, doors, bumpers, mirrors) have 1 or 40 health, flags 0x38 or 0x3f, and launch a physics piece when they die;
glass has 40/20 then 60 and breaks with its effect and sound; lights and tail lights 20.

## Map scripts (not done)

`maps/mp/<map>.gsc` is compiled bytecode in the map's patch zone (`patch_<map>.ff`; Crash: 291 bytes of code, 476
of stream, with `misc_rotate_ceilingfans`, `me_fanceil_spin`, `ceilingfan_rotate_custom`,
`knockbadpanoffwall`, `delayimpulsepot` among its strings). Its **stream** is the Ghosts layout (i32 body length,
a u16 name token or `00 00` and an inline name, then the strings and token pairs in instruction order), but its
**opcodes are not Ghosts'**: `gsc_iw6` stops at the first function ("unknown opcode 0x0b"). Running these scripts
needs the h1 opcode table (the VM loop), the builtin table (378 rows at `0x140fecdf8`) and the token names (the
engine string table) first; none of that was read. The shared libraries (`_load`, `_utility`, `_createfx`, the
game types) are in `common_mp.ff` / `common.ff` / `patch_common_mp.ff`. No map data was changed by this.
