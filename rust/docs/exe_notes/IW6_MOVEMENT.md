# Ghosts (IW6 MP, PC x64): player movement numbers

Method: static reading only of the already-unpacked dump
`iw6mp64_ship_unpacked` (sha256 771cce34...) and its Ghidra 11.4.2 export
`iw6mp64_ship_all.c`; float operands read from the image with a capstone
disassembly of the named functions (RIP-relative constants resolved). Nothing
was run, unpacked or decrypted. Image base 0x140000000.

Tags: **[C]** confirmed from code (constant and its use read), **[I]** inferred
(role or flag meaning reasoned, not traced end to end).

## 0. The main finding: these "dvars" are compiled constants

The earlier note (IW6_COLLISION.md section 8) assumed `g_gravity`, `g_speed`,
`player_sprint*`, `bg_fall*` etc. are registered by name hash. They are not
registered at all in this build:

* Every dvar goes through the named wrappers (`Dvar_RegisterBool/Float/Int/...`
  at 0x1404edd60..0x1404eea50), all of which call the one creator 0x1404ee360
  with a name string. No other caller of the creator exists (only the
  "External Dvar" and "ScriptClientData" paths). There is no hash register.
* None of those names exist as strings in the image (`strings` finds no
  `g_gravity`, `g_speed`, `player_sprintTime`, `jump_height`, `bg_fall*`).
  Only descriptions of perk dvars mention them ("Multiplier for
  player_sprinttime").
* The movement code reads literal floats/ints where IW3/IW4 read those dvars,
  and some compiled-out dvars survive as `0.0 * 1000` or `t * -0.0 / 750`
  (the default folded in). So the retail values are the literals below.

The only movement-adjacent dvars still registered are the `perk_*` ones
(specialty_parabolic registration at 0x14021bc20) and `bg_shock_movement`.
`jump_height` is replaced by an `alt_jump_height` game-setting lookup with a
39.0 fallback.

pm_flags bits used below (they match IW3's layout) [I for names, C for use]:
0x1 prone, 0x2 ducked, 0x40 walking/sight class, 0x80 hard-landing timer,
0x100 knockback timer, 0x400 respawned, 0x2000 jumping, 0x4000 sprinting,
0x8000 shellshocked, 0x20000 no-sprint, 0x40000 no-jump, 0x100000 sliding.

## 1. Values

| Value | Ghosts | Evidence | Conf |
|---|---|---|---|
| Gravity (ps->gravity, int, +0x5c) | **800** | spawn writes `ps[0x17] = 800` (0x140442300, also 0x140382890 for agents); jump `sqrt(gravity*h*2)` reads +0x5c | C |
| Run speed (ps->speed, +0x60) | **190** | ClientThink paths write `ps[0x18] = 0xbe` every frame (0x140441b40, 0x1403832e0) | C |
| Backpedal scale | **0.7** | CmdScale_Walk 0x14021fe10: `if forward<0: f *= 0.7` | C |
| Strafe scale | **0.8** | same function: `right * 0.8`; the larger of the two is used | C |
| Speed formula | `speed * max(|f|,|r|) / (127 * sqrt(f^2+r^2))` | 0x14021fe10 (`127.0` at 0x1408402d4) | C |
| Walk / sight-class scale | **0.4** | 0x14021fe10: pm_flags 0x40, or (prone and ps+0x424 > 0) -> x0.4; the same condition picks the weapon's adsMoveSpeedScale (+0x5a0) instead of moveSpeedScale (+0x59c) | C value, I meaning |
| Sprint speed scale | **1.4** | 0x14021fe10: pm_flags 0x4000 -> x1.4 (0x14084413c) | C |
| Sprint side-input scale | **0.667** | WalkMove 0x140225830: sprinting -> rightmove *= 0.667 | C |
| Noclip / ufo scale | x3 / x6 | 0x14021fe10 (pm_type 2 / 3) | C |
| Stance scale: stand / crouch / prone | **1.0 / 0.65 / 0.15** | 0x14021fc40; stance 3 (ps+0x190 = 0x16) -> 0.2; crouch while sprinting with a perk bit (ps+0xe18 bit 13) -> 1.0; prone<->crouch blends 0.15<->0.65 over 400 ms (200 ms for one path) | C |
| Shellshock scale | x0.4 when bg_shock_movement | 0x14021ffcc | C |
| Script scale | x ps->moveSpeedScale (+0x25c) | end of 0x14021fe10 | C |
| Weapon scale | x moveSpeedScale / adsMoveSpeedScale (WeaponDef +1436/+1440), attachment override at attach+0x58 -> +0x20/+0x24 | 0x140240310 | C |
| Sprint forward input | **> 105** (of 127) | 0x14021eb40 and 0x140224380: `'i' < forwardmove` | C |
| Sprint time (full meter) | **4000 ms x weapon sprintDurationScale** (WeaponDef +1444) | 0x140218c90: `+0x5a4 * 4000.0` | C |
| Marathon-class perk | x perk_sprintMultiplier (**2.0**), perk word +0xe14 bit 4 | 0x140218c90 | C |
| Sprint meter cap | **16383 ms** (0x3fff) | 0x140218c90 | C |
| Unlimited sprint perk | perk bit +0xe14 bit 14: meter always full | 0x140221370 / 0x140221400 | C |
| Sprint min to start | **> 1000 ms left** | 0x14021eb40 (`1000.0` at 0x140825434) | C |
| Sprint recharge pause | **0 ms** (compiled `0.0 * 1000`) | 0x140224538..0x140224556 | C |
| Sprint refill | 1 ms per ms rested: `left = max - (end - start) + (now - end)`, clamped 0..max | 0x140221370 (ps+0x248 lastStart, +0x24c lastEnd, +0x250 startMax) | C |
| Sprint end when meter empty | sets "button up required" (+0x244) | 0x140224380 | C |
| Sprint-out timer | weapon sprintOut state timer (WeaponDef +1128, akimbo +1332 via 0x140246030) x perk_sprintRecoveryMultiplierActual (**0.6**) with perk bit 17 | 0x14022e3a0 | C |
| Ground accel stand / crouch / prone | **9 / 12 / 19** (stance 3 also 19); x0.25 during hard landing | 0x140225830 | C |
| Air accel | **1.0** | AirMove 0x14021e760 -> accelerate with 1.0 | C |
| Friction / stop speed | **5.5 / 100** (x0.3 hard landing) | 0x140220fd0 (see IW6_COLLISION.md) | C |
| Jump height | **39.0** (`alt_jump_height` setting overrides when non-zero) | 0x140213140 | C |
| Jump velocity | `sqrt(2 * 800 * 39) = 249.8 u/s` | 0x140213540 | C |
| Jump interval | **500 ms** (`499 < now - lastJump`) | 0x140212f60 | C |
| Jump slowdown | after a landing with the jumping flag: height divided by `1 + 1.5 * t/1700` (t = pm_time), **2.5** from 1700 ms, cleared at 1800 ms | 0x140213540, 0x1402134d0 | C |
| Jump: no jump when | no-jump flag, respawned, prone/ladder states, pm_type >= 7 | 0x140212f60 | C |
| Step height ground / air | **18 / 10** | 0x140228990 (IW6_COLLISION.md) | C |
| Fall damage heights | **128 .. 300** (height from v^2 / 2g); damage `(h-128)/172 * 100` (10 with a cmd flag) | 0x140220000 | C |
| Hard landing | >12 units: velocity x0.67; damaging: pm_time `35*dmg + 500` ms (cap 2000), velocity x0.5 .. 0.2 | 0x140220000 | C |
| Slide start | horizontal velocity **x1.1** (x1.0 on one surface case) | 0x140227a10 | C |
| Slide length | **60 units x moveSpeedScale** along the facing; end time = length / speed | 0x140227a10 (`60.0` 0x1408270fc) | C |
| Slide needs | on ground, no hard landing, >= **200 ms** since sprint start, >= **1200 ms** since the last slide ended, forward speed >= **150**, weapon in a sprint state, not a riot shield class | 0x140227350, 0x1402276d0 | C values, I for "riot shield" |
| Slide exit | input steering only within 150 ms of the slide end; slide state left 350 ms after the end (180 ms while crouch is held and not prone) | 0x140227820, 0x140227d50 | C values, I for the phase names |
| Mantle probe | box half 0.1 x 0.1 x 35 (z mid 35), from origin - **14.9** x fwd to origin + **54.9** x fwd (**78.9** airborne), contents mantle 0x1000000 | 0x140214ef0 | C |
| Mantle angle | view within **60 deg** of the ledge normal | 0x140214ef0 (`57.2958`, `60.0`) | C |
| Mantle heights | anims 64, 56, 48, 40, 32, 24, 16 (nearest picked); table `{up, over, height, a, b}` = (1,8,64,0,30) (2,9,56,0,30) (3,10,48,0,30) (4,11,40,5,30) (5,12,32,5,30) (6,13,24,30,30) (7,14,16,30,30) | 0x140215390; table 0x1408400a0 | C values, I fields a/b |
| Mantle anim Z check | anims without a "ledge" notetrack must move -18 | 0x140214ad0 | C |
| Other walk factor | a second speed estimate: back x0.7, both inputs x0.925, back+side x0.85, then x0.4 / x1.4 | 0x1402211b0 | C values, I role (prediction / anim) |

## 2. What was not found
* Ladder speeds, prone/crouch eye heights and view heights, dive (none in MP),
  lean amounts, ADS transition.
* The meaning of pm_flags 0x40 and ps+0x424 (walk vs sight aiming) was not
  traced to their setters.
* Slide friction while sliding (the slide is a timed move; per-frame decay
  lives in 0x140227470 with 20, 270, 30, 0.5, 250, 0.7 constants not decoded).

## 3. In the Rust
`asset_game::iw6_movement::IW6_MOVEMENT` holds these numbers as data;
`iw6_movement_requested()` returns it only with `BO2MP_IW6_MOVEMENT_TABLE=1`.
Nothing calls it: Black Ops II movement stays authoritative.

## 4. Evidence index
| Role | Address |
|---|---|
| Dvar creator / wrappers | 0x1404ee360; 0x1404edd60, 0x1404ee070, 0x1404ee180, 0x1404ee270, 0x1404ee660 |
| Perk dvars | 0x14021bc20 |
| Spawn (gravity 800) | 0x140442300; agent 0x140382890 |
| ClientThink (speed 190) | 0x140441b40; 0x1403832e0 |
| CmdScale walk | 0x14021fe10 |
| Stance scale | 0x14021fc40; stance query 0x140221150 |
| Weapon move scale | 0x140240310 |
| Walk move | 0x140225830 |
| Air move | 0x14021e760 |
| Sprint max / left | 0x140218c90; 0x140221370, 0x140221400 |
| Sprint update / can start | 0x140224380; 0x14021eb40 |
| Sprint-out timer | 0x14022e3a0, 0x140246030 |
| Jump | 0x140212f60, 0x140213140, 0x140213540, 0x1402134d0, 0x1402130a0 |
| Landing / fall damage | 0x140220000 |
| Slide | 0x140227350, 0x1402276d0, 0x140227470, 0x140227820, 0x140227a10, 0x140227d50 |
| Mantle | 0x140214ad0, 0x140214ef0, 0x140215390, table 0x1408400a0 |
| Event names | table 0x1409e11a0 (EV_JUMP 127, EV_MANTLE 190, EV_SLIDE_START 191, EV_SLIDE_EASE_OUT 192) |
