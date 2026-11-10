# Black Ops III Zombies: AI, pathing and map scripts (plan)

Goal (owner, 2026-10-27): the BO3 Zombies maps play like the real game:
zombie AI and pathing, rounds, and every scripted map event (Shadows of
Evil's Beast mode, rituals, Shadow Man, pods, train, Margwa, parasites...).
First map: `zm_zod` (Shadows of Evil). Everything below was measured on the
owner's BO3 Steam zones (staged 2026-10-27) and `BlackOps3.exe`
(`/home/claude/bo3ref`).

## 1. What runs today

A BO3 `zm_*` map loads through the T7 lane (`assets::lane::t7_map`,
`fastfile_t7`) and is drawn natively (`render_t7`). Its Zombies side is not
BO3's at all:

1. `t7_map::zm::read` reads the map entities into the Infinite Warfare
   pipeline's `ZmLayout`: zones (`info_volume` `player_volume` + their
   spawn structs), windows (`exterior_goal` + zbarrier), doors/debris
   (`zombie_door`/`zombie_debris`, the zones joined guessed from their
   `script_flag`), perk machines (only the six BO2 has), box spots,
   Pack-a-Punch, power switch, wall buys (remapped to IW7 guns), dog spots,
   `node_negotiation_begin` traversals.
2. `iw7_zombies::prepare_layout` lays a walk graph over the world's
   triangles (`asset_world::nav_soup`) as BO2 path nodes.
3. `iw7_zombies::attach_t6_zm_rules` loads **Black Ops II's** Zombies zones
   beside the map (Buried as the kit donor: its zombies, boards, box,
   machines, guns), writes the layout as BO2 entities, and links our map
   script `sim/src/t6/scripts/zm_foreign_map.gsc` (`sim::t6::foreign_zm`).
4. The match is BO2 Zombies: BO2's `_zm` GSC on the T6 VM (`gsc_t6`, natives
   in `sim::t6::*`, AI in `sim::t6::actors` / `natives_ai` / `nav`).

So on `zm_zod` today: BO2 (Buried) zombies with BO2 animations walk a
generated graph; rounds/points/doors/box/perks are BO2's scripts. Not
present: BO3's zombie behaviour, Beast mode, rituals, pods, Apothicon
Servant, Margwa, parasites, keepers, train, Shadow Man, GobbleGum, traps,
teleports, the Easter egg.

## 2. What BO3 itself ships (measured)

### Scripts (bytecode)

* The compiled scripts are only in the **patch** zones; `zm_zod.ff`,
  `zm_common.ff` hold none (no `scriptparsetree`). For `zm_zod`:
  `core_patch.ff` 365 objects, `zm_patch.ff` 267, `zm_zod_patch.ff` 123:
  **456 server `.gsc`** + the `.csc` client scripts. They include all of
  `scripts/shared/ai/*` (zombie, zombie_utility, archetype_*, margwa, mechz,
  skeleton, systems/behavior_tree_utility, animation_state_machine_*,
  blackboard, ...), `scripts/zm/_zm*.gsc` (zm, spawner, zonemgr, blockers,
  magicbox, perks, power, weapons, powerups, bgb, altbody, laststand...),
  `gametypes/zclassic`, and the 65 `zm_zod*` scripts (quest, ee, pods,
  portals, train, robot, shadowman, sword_quest, idgun_quest, beastcode,
  margwa, smashables, traps, transformer...).
* Format: magic `80 47 53 43 0D 0A 00`, version byte **0x1C**. Header:
  crc, include/animtree/cseg/string-fixup/dev-string/export/import/fixup/
  profile offsets, cseg size, name offset (u32s at 0x08..0x34), then u16
  counts (strings, exports, imports, fixups, profile, dev strings) and u8
  include/animtree counts. Exports 0x14 bytes (crc, bytecode offset, name,
  namespace, param count, flags). Imports: name, namespace, u16 ref count,
  u8 params, u8 flags (low nibble: call kind), then the ref offsets.
* Names are **canonical hashes**, not strings. From the exe
  (`0x14136ccc0`): FNV-1a over the lower-cased name, basis `0x4B9ACE2F`,
  prime `0x1000193`, then one more multiply by the prime
  (`canon("zm_zod") = 0xBCFA45B0`, `canon("main") = 0xD290EBFA`, both
  found in `zm_zod.gsc`'s exports). No names are needed to link: imports
  match exports by (namespace, name) hash.
* **Opcodes**: the VM (`0x14138e5b0`) reads a u16 (2-aligned); values
  `< 0x2000` index a **static** 8192-entry handler table in `.rdata`
  (`0x142eb38b0`; `0x2000` set: a debug twin table). 8192 slots map to
  **126 distinct handlers** (plus the invalid-opcode one): every operation
  has ~30-70 opcode values. All 126 handlers were decompiled
  (`bo3ref/decomp/<addr>.c`) and identified (section 4); the slot-to-
  handler table is read from the exe, so no opcode guessing.
* **Builtins**: per-type tables of `{hash, min args, max args, fn, type}`
  (0x20 bytes), partly static in `.data`, partly written by init code.
  2485 entries found (server + client). The 456 zod server scripts call
  **1147 distinct builtins** (20435 call sites); 768 are named by hashing
  candidate names (our BO2/IW catalogs, every identifier in the exe's
  strings, the scripts' own strings). The rest are identified on demand by
  decompiling the table's handler. Not in any table: `assert`/`assertmsg`
  (dev), `println`, `array(...)` (to check: compiled specially).

### Data the AI runs on (in `zm_zod.ff`)

`behaviortree` 3, `animstatemachine` 5, `animselectortableset` 5,
`animmappingtable` 6, `aitype` 16, `character` 59, `scriptbundle` 87,
`zbarrier` 7, `navmesh` 1, `navvolume` 1, xanim 4290. The nav mesh is a
**Havok AI 2014.2.0-r1 packfile** (`hkaiNavMesh`, generation settings and
streaming sets inside the zone image at ~0x1d36fece..). Traversals are
map `node_negotiation_*` pairs; the zone walk (`fastfile_t7::walk`) does not
yet keep any of these assets.

### How BO3 zombie AI works (from the script list and assets)

The engine runs a **behaviour tree** asset per archetype (`zombie`,
`zombie_dog`, `margwa`, `parasite`/`wasp`, `raps`, `zod_companion`), whose
conditions/actions are script functions registered by name
(`behavior_tree_utility`, `archetype_zombie_interface`, `zombie.gsc`,
`_zm_behavior.gsc`). Actions drive the **animation state machine** asset
(ASM states, transitions, `animselectortable` picks the clip by
blackboard: locomotion speed walk/run/sprint/super-sprint, crawler,
melee variants, traversal type, barrier tear/attack), and **mocomps**
(motion modes: animation-driven vs path-driven). Pathing is engine-side
over the Havok nav mesh (`setgoal`, `pathenabled`, `findpath`...).
Spawning, rounds, zones, doors, power, perks, box, wall buys, powerups and
every map event are script (`_zm.gsc` `round_think`, `_zm_spawner`,
`_zm_zonemgr`, `_zm_blockers`, ...).

## 3. Decision: run the real scripts on a T7 VM

**Chosen: the VM route**, with BO3's own scripts driving everything above
the engine layer; native Rust for what BO3's engine does natively (nav
mesh pathing, BT/ASM interpreters, entity/physics/animation).

Why: "all scripted events work like the normal game" means Shadows of
Evil's ~65 map scripts plus ~390 shared ones; re-writing those by hand
would be a re-interpretation and would never be complete. The bytecode is
all present, unencrypted, linkable by hash, and the opcode set is small
and now fully known from the exe.

Effort and risk, honestly:

| piece | size | risk |
| ----- | ---- | ---- |
| loader, linker, disassembler (`crates/gsc_t7`) | small | low: format verified on all 456 objects |
| interpreter: values, structs/arrays, entities, threads, wait/notify/endon/waittill, function pointers, classes, switch | ~3-4k lines | medium (reference: `gsc_t6`'s VM) |
| builtins: 1147 used; most are thin (math, arrays, strings, flags, dvars, entity fields, sounds/fx as events) | large, incremental | medium: ~380 unnamed; client-visual ones can be recorded as events |
| Havok nav mesh reader + A* + funnel + traversal links | medium | medium (packfile layout, fixups) |
| behaviour tree + ASM + anim selector + mocomps | large | high: asset layouts unknown yet, engine semantics read from the exe |
| entity model: T7 entities/actors/players on the sim | large | high: the sim is BO2's; zm scripts touch players heavily |
| CSC client scripts (visual events) | partly done (2026-11-04) | a second VM runs the `.csc` objects and delivers the server's client fields to their callbacks (`sim::t6::t7_client`); effects/sounds reach the renderer, lighting/fog/post-effects do not yet |

The BO2-kit path stays as the fallback until the T7 path plays a full
round loop on `zm_zod`; the switch is per map.

## 4. The T7 opcode set (from the decompiled handlers)

Operand reads align to their size (u16 at 2, u32 at 4, u64 at 8); call
ops have a u8 parameter count before the aligned target. Stack values are
16 bytes `{value, type}`; types: 0 undefined, 1 object, 2 string,
3 istring, 4 vector, 5 hash, 6 float, 7 int, 9 (u64 constant), 0xb call
frame, 0xc pre-call marker, 0xd builtin pointer, 0xe script function
pointer, 0x10 animation.

Fixed slots: 0x00..0x0b debugger ops; 0x0c ClearParams (pop to marker),
0x0d CheckClearParams (pre-call marker becomes the frame), 0x0e
PreScriptCall (push marker), 0x0f CallBuiltin (u8 n, u64 fn), 0x10 End.
Scrambled (per handler): Return, GetUndefined, GetZero, GetByte, GetNegByte,
GetUnsignedShort, GetNegUnsignedShort, GetInteger, GetFloat, GetString,
GetIString, GetVector (12 inline bytes), GetHash, GetFunction (0xe),
GetAPIFunction (0xd), GetAnimation (0x10), push-u64 (type 9), Vector,
VectorConstant (packed byte), VectorScale, GetSelf, GetLevel, GetAnim,
GetGame, GetWorld/Classes (4 object registers, `level` identified),
Get*Object (set the field-object register), GetSelfObject, CastFieldObject,
EvalFieldVariable(Ref), EvalSelfFieldVariable(Ref),
EvalLevelFieldVariable(Ref), ClearFieldVariable, EvalLocalVariableCached
(u8), EvalLocalVariableRefCached, SafeCreateLocalVariables (u8 count, per
var: u32 hash + u8 flags: 1 by-ref array, 2 varargs),
SafeSetVariableFieldCached, SafeSetWaittillVariableFieldCached,
SetVariableField, ClearArray, EvalArray, EvalArrayRef, GetEmptyArray,
SizeOf, IsDefined, FirstArrayKey, NextArrayKey, CallBuiltinMethod,
ScriptFunctionCall, ScriptMethodCall, ScriptThreadCall,
ScriptMethodThreadCall and their `Pointer` forms, ClassFunctionCall /
ClassFunctionThreadCall (u32 method hash), New (class instance),
DecTop, SafeDecTop, CastBool, BoolNot, BoolComplement, JumpOnFalse,
JumpOnTrue, JumpOnFalseExpr, JumpOnTrueExpr, Jump (i16), DevblockBegin,
Switch (i32 to a table of {value, offset}), EndSwitch, Inc, Dec, Bit_Or,
Bit_Xor, Bit_And, Equal, NotEqual, SuperEqual, SuperNotEqual, LessThan,
GreaterThan, LessThanOrEqualTo, GreaterThanOrEqualTo, ShiftLeft,
ShiftRight, Plus, Minus, Multiply, Divide, Modulus, Wait (server: 20
frames/s), RealWait (ms), WaitTillFrameEnd, WaitTill, WaitTillMatch (the same
handler code; u8 operand), EndOn, Notify, GetTime.

## 5. Milestones (each pushed when verified)

1. **gsc_t7 loader + disassembler** — every function of the 456 zod
   objects decodes to its end, every jump lands on an instruction, every
   import resolves to a script export or a builtin table entry (test over
   the staged zones; skipped when absent). The opcode slot table is
   generated from the exe into the crate.
2. **Interpreter core** — unit tests for each op family; then the real
   `scripts/shared/system_shared.gsc` autoexecs run headless with stub
   builtins; a report of the builtins hit and not implemented.
3. **Level start** — map entities as T7 script entities/structs, `level`,
   flags, clientfield registration (recorded), dvars, `zm_zod::main`
   through `zm::init` to the round loop's start; the unimplemented-builtin
   report drives the work.
4. **Nav mesh** — Havok packfile reader for `hkaiNavMesh` (faces, edges,
   vertices, user edges), A* over faces + funnel, traversal links from the
   negotiation nodes; test: paths on `zm_zod` from spawners to a player
   around obstacles.
5. **Zombie AI core** — actors from `aitype` spawners, the BT asset and its
   script-registered functions, ASM/selector tables (locomotion speeds,
   melee, barrier attack/vault, rise from ground, crawlers), damage/death;
   rounds from `_zm.gsc`. Test: headless, zombies spawn, reach a player,
   rounds advance when killed.
6. **Economy** — doors/zones, power, perks, box, wall buys, powerups via
   their scripts (use triggers, hint strings, zbarriers, models).
7. **Shadows of Evil events** — Beast mode, rituals, pods, train, Margwa,
   parasites, keepers, Shadow Man, sword/idgun quests; then the other maps
   (`zm_castle`, `zm_island`, `zm_stalingrad`, `zm_genesis`, `zm_theater`
   ... each needs its `<map>_patch.ff`).

## 6. Status

### The PC's four symptoms of 29 Oct (root causes, 2026-11-03)

Reverted on main in b80ecb8b. Each cause read from the zones (and the
scripts' own bytecode); none of the fixes has been seen on the PC. Checked
here: unit tests, real-zone ignored tests and the headless harness. Not
checked here: the real game start (no Black Ops II Zombies kit zones, no
`zm_zod` xpaks or sound banks and no GPU in the cloud).

1. **T-posed zombies.** A Shadows of Evil zombie is a 105-bone body plus a
   211-bone head (316 bones in one DObj; the Margwa's body alone has 253).
   `xmodel_runtime::DObj::build` refused more than 192 bones (Black Ops
   II's part-bit capacity, 6 words), so the client's
   `pose_t6_composition` built no DObj and drew every zombie's models in
   their bind pose, while the sim (which only times clips) walked them to
   the player. The clips were never at fault: the walk clip's 104 tracks
   all bind by name on the body. Fix: a DObj holds up to 1024 bones; bones
   past the part bits are always animated and never masked or hidden.
   Also: a character's extra model that is only a reference in the zone
   (`c_zom_zod_zombie_fem_body3_intestine`) is left off its character, or
   that composition lost its pose the same way. Check:
   `T7_ZM_SKELETONS=1 T7_ZM_ZONE=<zm_zod.ff> T7_ZM_ENTS=<entities> cargo
   test -p assets --lib zombie_actors_pose -- --ignored` (every character
   of the 16 AI types posed: 312 of 316 bones move at 30% of the walk);
   `t7_characters zm_zod.ff --cast --bind`.
2. **Mystery boxes in front of gates and doors.** The engine builds a
   zbarrier from its classname's `ZBarrierDef`; the sim took the boards
   from the map entity's `zbarrierboardmodel<i>` keys. Shadows of Evil's 15
   `zbarrier_zm_zod_gate_scissor` entities carry a magic box's five boards
   in those keys (a prefab's leftovers: `p6_anim_zm_magic_box_fake`,
   `p6_anim_zm_magic_box`), so each gate stood as a box. The def
   `zm_zod_gate_scissor` has one board: `p7_fxanim_zm_zod_gate_scissor_mod`,
   tear `p7_fxanim_zm_zod_gate_scissor_open_anim`. Layout read from the
   zone (`walk::world_misc::zbarrier`): board count u32 at 0x38, six boards
   of 0x70 from 0x40, each its model at +0x20, upgraded model +0x30, tear
   and board animations at +0x38 / +0x40 (pointers to the animations'
   names). `lane::t7_map::zbarriers_from_defs` rewrites every zbarrier
   entity's board keys from its def before the scripts or the kit read
   them. Check: `cargo test -p assets --lib zbarrier_boards_come_from_the_def`;
   `t7_characters zm_zod.ff zm_` prints the defs.
3. **Sky missing.** Pushed earlier (152d977f): the sky material's forward
   technique was not read. Checked with `t7_sky_check` (the game's
   `Scene::load`), not drawn on a GPU through the game start.
4. **No round-start voice lines.** Three gaps on one path. (a) The lines
   are played (headless, round 1: `vox_shad_round1_start_0` to all,
   `vox_plr_1_round1_start_0` to the player) but every script sound goes
   to the client tagged `t6:`, and the map bank keeps Black Ops III's
   aliases under T7: the lookup missed. Now a `t6:` alias only Black Ops
   III's aliases hold plays from those (`audio::ui_sounds::bo3_script_alias`).
   (b) The bank read only the map zone and mp_common: the round-start
   stingers are in `zm_zod_patch` (`mus_zod_roundstart_<n>_intro`), the
   voice lines in the language zones (`en_zm_zod`, banks
   `<zone>.en.sabl/.sabs`). `asset_audio::zm_sound_zones` adds `<map>_patch`,
   `zm_patch`, `zm_common` and their `en_` zones with their banks.
   (c) The stinger is a music state: `zm_audio` picks `zod_roundstart_1..5`,
   `music::setmusicstate` sets client system "musicCmd", whose
   `music_shared.csc` calls the engine's `soundsetmusicstate(tolower(state))`
   (builtin hash 0x2913b292); the sim knew only Nuketown's WAVE. Now a
   Black Ops III state plays `mus_<state>_intro` to the players (the alias
   `_zm_audio` times the state by). Headless: `mus_zod_roundstart_1_intro`
   at round 1, `_5_` at round 2, `mus_zod_roundend_3_intro` between
   (`BO2MP_SNDLOG=1`). Not checked: the `en_zm_zod` zone and banks (not
   staged here), and playback on the PC.

* 2026-10-27: plan; formats, hash, opcode table and builtin tables
  measured.
* 2026-10-27, milestone 1 done: `crates/gsc_t7` reads and decodes every
  object of `core_patch`, `zm_patch` and `zm_zod_patch` (755, 456 server
  scripts) by control flow; every call links to a loaded export or a
  builtin record except `shellshock`/`obituary` (records the table reader
  does not see) and 7 namespaced calls into multiplayer-only functions
  (`cargo test --release -p gsc_t7`, `T7_ZONES=<zone dir>`). 1240 builtins
  called, 840 named. Found on the way: the VM aligns the pc to 2 before
  each opcode; developer strings have their own fixup table; animation
  refs are 16 bytes; the first waittill handler is `waittillmatch` with a
  u8 operand; builtin records with any argument count say max 0x7fffffff.
  Regenerate the tables with `python3 crates/gsc_t7/gen/t7gen.py
  BlackOps3.exe crates/gsc_t6/src/t7 <extracted scripts dir>
  crates/gsc_t7/gen/catalog_names.txt` (scripts dir: `t7_gsc_scan <zone>
  <dir>` output).
* 2026-10-27, milestone 2 (VM core) done: rather than a second VM, the
  T7 objects are translated to `gsc_t6`'s instructions
  (`gsc_t6::t7::translate`; the T7 code lives in `gsc_t6` so the sim gains no dependency, `crates/gsc_t7` keeps the generator, tests and tools) and run by the Black Ops II VM, which the sim
  already hosts (entities, players, actors, triggers, natives). Added to
  `gsc_t6` for Black Ops III: `===`/`!==`, classes (`new`, method calls
  through `classes.<class>[0].__vtable[hash]`), the `classes` and `world`
  globals, `...` parameters. Hash-linking is resolved at translation; a
  hash with no known name becomes `#xxxxxxxx`. Autoexecs run a script
  after its includes (class registrations before their users).
  `cargo test --release -p gsc_t7 --test run`: all 456 scripts link with no
  unrunnable operation, the 343 autoexecs run headless (all
  `system::register` calls, 21 classes); the only errors are in
  `zm_zod_zombie`, which needs the engine (entities); called but unbound
  with only the engine-free builtins: `registerclientfield`,
  `getentarray` and three unnamed ones.
* 2026-10-27, milestone 4 (nav mesh) done ahead of 3: `crates/nav_t7`
  reads the Havok 2014.2 packfiles in the map zone (`hkx`: sections,
  local/virtual fixups) and the `hkaiNavMesh` (`mesh`: faces, edges with
  twins, vertices in metres, scaled to inches; layout in the module doc),
  with A* over faces and the funnel, corners kept 15 units off the walls,
  traversal links (`node_negotiation_begin` -> `_end`, and each window:
  `exterior_goal` through its zbarrier) and blocked faces (doors). On
  `zm_zod` (`cargo test --release -p nav_t7 --test zod`): 3582 faces; 171
  of 185 traversals and 22 of 23 windows link; 12 of 13 start-zone spawn
  structs path to the players' start (8 round corners, 3 through a window
  or traversal), every stretch over the mesh; map-wide 477 of 525 spawn
  structs (doors open). `nav_query <packfile> x y z x y z` prints a path;
  `t7_havok_dump <zone> <dir>` writes a zone's packfiles out. The nav
  volume (flying AI) is found but not read.
* 2026-10-27, milestone 3 (level start) running headless: with
  `BO2MP_T7_SCRIPTS=1` a BO3 Zombies map's lane (`lane::t7_zm_scripts`)
  hands the match its patch zones' 755 objects and its own entity string
  (`T6ScriptSet::t7_objects`; the BO2 kit stays for models, anims,
  weapons). `sim::t6::t7` translates them at install, runs the autoexecs,
  makes each map struct with its keys and runs `struct::init` on it, then
  `zclassic::main`, `zm_zod::main`, `callbacks_shared`'s start callbacks;
  the engine callbacks are `callbacks_shared`'s. Engine-side differences
  so far: event names key by canonical hash (`Vm::t7_events`), weapons are
  objects (`getweapon`; converted to names at BO2 builtins and back from
  `getcurrentweapon` & co., `Hooks::native_args/native_return`),
  `tablelookuprow`, `getdvarstring`; both waittill assign ops are BO2's
  waittill form. Headless (`cargo test -p sim --lib t7_test` with
  `T7_SCRIPTS_DIR`, `T7_ENTS`, `T7_TABLES`, see the test's doc): 343
  autoexecs, 1714 entities, 1877 structs, `zm_zod::main` and the start
  callbacks run, 159 threads wait; 11 builtins called still unbound
  (`spawncollision`, `getaiteamarray`, `hide/showmiscmodels`, unnamed
  ones), script errors left in `visionset_mgr`, `_zm_unitrigger`,
  `_zm_pack_a_punch`. Not yet: players connecting, rounds, zombies.
* 2026-10-27, milestone 3: the player spawns and round 1 starts
  (headless). Fixes on the way: jump and switch targets are aligned to 2
  (an odd target is the next instruction: `globallogic_spawn::mayspawn`
  fell off its end and returned undefined, so nobody spawned); by-ref
  `&array` parameters (`array::add`, every callback registration);
  `ClearArray`; start order `finalize` before `startgametype`. Engine
  side: BO3's `getdvarint/getdvarfloat(name, default)`, the zclassic
  settings that differ (`disableCAC` 1: the team pick spawns the player;
  `playerNumLives` 1), the string builtins `table_shared::load` reads
  through (`strstrip`, `strisint`, `strisfloat`), `hashstring`, spawner
  arrays (`actor_spawner_*`, vehicle spawners by their `SPAWNER` key),
  `spawncollision`, and a few no-ops; 9 more builtin names found by hash
  (catalog). The test loads the map's brush model bounds (`T7_CMODELS`,
  `t7_cmodels zm_zod.ff` output) so the out-of-playable-area check sees
  the zone volumes (without them the player was killed at once and the
  game ended). Now: player `playing`, `initial_blackscreen_passed`,
  `start_zombie_round_logic`, `begin_spawning` set, round 1, 0 AI:
  `spawnfromspawner`, `getclosestpointonnavmesh`,
  `getaiarchetypearray` and the UI model builtins are the next ones.
* 2026-10-27, milestones 4-5 (AI and rounds), headless: zombies spawn,
  walk the nav mesh to the windows, tear the boards, climb in, find the
  player and kill him; shot as they come, the rounds advance (round 4 by
  150 s with `T7_KILL=12`). `sim::t6::t7_ai` is the engine half:
  `spawnfromspawner` (the spawner's fields, archetype by class, then
  `callbacks_shared::codecallback_actorspawned`), the nav mesh in the sim
  (`nav_t7`'s files included by path; `T6Install::t7_nav`, the map zone's
  packfiles) with traversals and windows linked, and a behaviour tree
  written from the registered names: services (`zombieFindFleshService`,
  `zombieEnteredPlayableService`, ...; `findNodesService` and
  `updateAttackSpotService` outside), then enter (mantle through the
  window link), move to the entrance and the attack spot, tear a board
  (`getChunkService`; grab, hold, pull; the `destroy_piece` notetrack
  through `animationstatenetwork::runnotetrackhandler`), attack through
  boards, taunt; inside: melee (the `zombie_melee` notetrack) or walk the
  goal the scripts set (`setgoal`, snapped onto the mesh). Script
  functions are called with the zombie as argument and `self`
  (`Vm::call_value_now` returns their value). Builtins: nav
  (`getclosestpointonnavmesh`, `ispointonnavmesh`, `isatgoal`,
  `calcapproximatepathtoposition`, `positionquery_source_navigation`),
  blackboard, `pathmode`/`getpathmode`, `getaiarchetypearray`,
  `arraysortclosest`, `kill`; the actor damage callback has Black Ops
  III's 15 arguments and weapon objects. The test reads the brush models'
  planes (`t7_cmodels --planes`): the zone volumes are their brushes, not
  boxes (a box let zombies count as inside at the window).
  Not yet: the zone's behaviour tree and animation state machine assets
  (actions last fixed times, walking 60/130/200/250 units/s by
  `zombie_move_speed`), rise animations (riser zombies stay under the
  floor until they walk), the margwa, parasites and other archetypes'
  trees, and the game path (`BO2MP_T7_SCRIPTS=1`) has not been run with
  the nav mesh (the lane does not pass it yet).
* 2026-10-27, doors (headless): `T7_USE="x y z@seconds"` puts player 0
  next to the use trigger nearest that point and uses it (its "trigger"
  notify with use held, as the engine does; 5000 points first): the start
  zone's door to the junction (750) opens with the scripts' own
  `zm_blockers` (flag `connect_start_to_junction`, points taken). Wallbuys,
  the box and perks run on `_zm_unitrigger` stubs and need guns: not
  verified headless (no weapons there); in the game Black Ops III's guns
  are the match's guests under their own names, and the Zombies-only ones
  the scripts give (knife, frag grenade, ...) map to the kit's
  (`t7::KIT_NAMES`). A dead Black Ops III actor no longer runs Black Ops
  II's death animscript.
* 2026-10-27, the map's own data the scripts read: script bundles (the
  zones' `scriptbundle` assets, 267 for zm_zod: scenes, characters,
  vehicles' settings) are made into structs and handed to
  `struct::createstruct(bundle, type, name)` after the autoexecs, as the
  engine does (`t7::create_bundles`; it files them in
  `level.scriptbundles`; the scene system readies its scenes). The lane
  reads them with the tables; the test from `T7_BUNDLES` (`t7_bundle <zone>
  "" --dump`). Path nodes are the map's `node_*` blocks made into objects
  (`getnode`, `getnodearray`: the train's stations), vehicle nodes come
  from `getallvehiclenodes`; `getstructfield`, `arraygetclosest/farthest`,
  `linktraversal/unlinktraversal` (a nav link out of every path),
  `setenablenode`. The train, the scenes and the vehicles start without
  script errors now (the bot script's are left: no bots in Zombies).
- **Vehicles (parasites, wasps).** `script_vehicle` spawners spawn through
  `spawnfromspawner` as vehicles (`t7_ai::spawn_vehicle`): fields copied,
  targetname the one asked for (`zombie_wasp`, which the round counts),
  `codecallback_vehiclespawned`; bullet damage goes to
  `codecallback_vehicledamage` (15 args), `finishvehicledamage` lowers
  health and at 0 runs `codecallback_vehiclekilled` and notifies `death`;
  `isalive` is false for a killed one's wreck. Headless: from round 6
  (`CODLEGACY_T6_START_ROUND=6`, `T7_KILL=6`) round 7 is reached by 80 s
  (before, the parasite round never ended). Not done: the parasites'
  flight (they stand where they spawn), margwa's tree, beast mode,
  rituals.
- **Weapons, wall buys, the box, power and perks.** Weapon objects carry
  the engine's fields the scripts read (`clipsize`, `maxammo`,
  `altweapon`, `dualwieldweapon`, `weapclass`, `inventorytype`, flags;
  hashed names through `field_name`, e.g. `rootweapon`). Build-kit
  builtins (`getbuildkitweapon` named in `catalog_names.txt`), and the
  builtins bound here keep weapon objects (`KEEPS_WEAPONS`). Weapons the
  Black Ops II kit lacks get a kit stand-in of their class (`STAND_INS`),
  and the engine's answers (`getcurrentweapon`, weapon notifies, damage
  notifies) name the Black Ops III weapon asked for. `isinmovemode`.
  Headless (`t7_test`): a catalog of the zombies weapons tables' weapons;
  scripted switches made at once with `weapon_change(_complete)`;
  `T7_USE` takes `box`; `T7_ZONES=1` enables every zone; `T7_SHOOT`,
  `T7_SETLEVEL`. Verified: start pistol (Bloodhound), wall buy (RK5, 500),
  power switch 1 by its trigger (as the beast's shock `useby`s it) then
  Quick Revive (500, `setperk`), the box (950, a weapon taken, swapping
  at the 2-gun limit). Not done: the beast's lightning itself, Pack-a-Punch.
- **Parasites fly and attack.** A vehicle gets its settings bundle
  (`scriptbundlesettings`, the `vehiclecustomsettings` bundle whose vehicle
  type its own names: `parasitesettings`), so `setspeed` has its speed
  and the existing flight model (`flight.rs`) carries it to
  `setvehgoalpos` goals. Its attack substate (`fire@stationary`) sends
  "pre_fire", spits at its turret target (`setturrettargetent`, 40) and
  ends with `asm_terminated` (`t7_ai::vehicle_tick`); melee is the
  script's own `dodamage`. Builtins named and bound: `mapfloat`,
  `isairborne`, `strisnumber`, vehicle occupants/immunity/damage
  multiplier, `getanimframecount`, exploders (client). VM fix: a Black
  Ops III `...` parameter is any flagged local, not only the last
  (`waittill_any_timeout` and every variadic script function). Headless:
  a forced parasite round (`T7_SETLEVEL='#ef0c0bfd=2'`) comes, its
  parasites close on the player and bite him, die to his shots and the
  round ends (round 3 by 90 s).
- **The Margwa.** Its own tree (`t7_ai::think_margwa`, from `margwa.gsc`'s
  registered functions): `margwaTargetService`, then spawn, teleport in /
  out, wait / freeze, pain, the stun / Servant / sword reactions, smash
  (its `margwa_smash_attack` notetrack: the radius hit) and swipe
  (`margwaSwipeAttackAction`, `margwa_melee_fire`), each its start, timed
  length and terminate, else it walks to the goal the service set (facing
  its enemy when near). Actors' `enemy` is the engine's perception (the
  favorite enemy, else the nearest player). Its heads' tags are guessed
  where the skeleton is not loaded (`tag_guess`: `j_chunk_head_bone`,
  `_le`, `_ri` above its shoulders) and a hit names the head it struck by
  bone index (`bone_for_point`, `getpartname`), so bullets into an open
  head kill it, as the game's damage override decides. Level start: the
  map's entities now spawn before the autoexecs (`_zm_ai_margwa` reads its
  spawners there), its structs and nodes after. Headless: a forced Margwa
  round (`T7_SETLEVEL='#bf361dc0=2'`, the subway door's flag set) spawns
  one, it walks to the player, smashes him (166) and dies to head shots;
  the rounds go on.
- **Beast mode and the power boxes.** `useby` (a trigger's "trigger" by
  that player) bound: the beast's lightning on a power box (the
  transformer's `script_model`, `zm_altbody_beast` watching its damage)
  uses its `use_elec_switch` trigger, `zm_power` turns that district's
  power on. The beast's lightning stands in as a gun in the Black Ops II
  kit (ray gun, else the M1911), so his shots in the game reach the boxes
  through the bullet path's damage notify (which now hands the weapon
  object). Headless: the start room's kiosk makes him the beast (his
  grapple and three lightning hands), his lightning on the box sets
  `power_on1`, the beast ends and his guns come back, Quick Revive sells.
- **The train runs.** Three causes: (1) a scene's objects carried the
  editor's `player` key whatever their type, so `isdefined(_s.player)`
  made the train a "player" object and its door scene deleted the train
  when it ended: only `type player` objects keep the key now
  (`create_bundles`); (2) `animscripted` on any entity now sets its
  origin/angles and sends its notify "end" when the clip has played (its
  length, else a second): `animation::play` waits on it (the doors);
  (3) `attachpath` sets a Black Ops III vehicle on its node, and
  `setswitchnode(from, to)` routes it at the junction (`Vehicles::switches`).
  Headless (`T7_STUBUSE`: a unitrigger stub's "trigger", the train's
  button): it leaves its station, runs the rails through the junction
  and stops at the next station 18 s later.
- **Riding the train; the levers call it.** The map key
  `moving_platform_enabled` sets the entity's platform flag (riders.rs);
  a platform with no loaded model takes the box of the brush models
  linked to it (the train's floor and walls), so `riders::advance`
  carries a player standing in it. Headless (`T7_USE='model:veh_t7_zmb_zod_train@18'`,
  `T7_STUBUSE` the same `@20`, `T7_TRACK=player,model:veh_t7_zmb_zod_train`):
  the player stays on its floor from the station through the junction.
  A station lever's stub (`T7_STUBUSE='1605.5 -9303.2 556.75@20'`, the
  train elsewhere) brings it there in 20 s. Left unbound on the way, with
  no visible effect: train methods `#c6021eb7(node, node)` and
  `#0f03481a()`.
- **The rituals (first one end to end).** Engine fixes: placed script
  bundles (`scriptbundle_*` classnames) are structs, not entities
  (`zm_zod_smashables` looks up each crate's / door's scene with
  `struct::get(target)` and its `__init__` ran `scene::init` on an
  undefined list); `?N` trigger models bound `getmins/getmaxs`
  (`local_bounds`: the union of their hulls about the origin; the craft
  tables size their use box from it) and `istouching` (defend volumes);
  a dead T7 actor keeps `damageweapon`/`damagemod` from its death args
  (`kill()`'s weapon "none"; `_zm_spawner`'s death function errored).
  Headless test data: `t7_cmodels zm_zod.ff --planes` now also prints
  `T ?<n>` trigger hull lines, read into the clip map's triggers; brush
  models' own brushes are loaded with no contents (they lie in model
  space; a nav volume's box blocked every trace near the map's middle,
  so `bullettracepassed` failed at the altars).
  Flow verified (`T7_SETLEVEL='#c303b23b=1'` keeps the out-of-area kill
  off; `T7_USE='1118 -5234 150@18,1577.57 -5441.85 184@80,1471.74 -3490.2
  127@86,1756 -1531 560~1756 -1561 560@94+6,1756 -1531 560~1756 -1561
  560@104+4' T7_TRIG='1581 -5441 198@24,1502 -3492 140@30'`): kiosk ->
  beast; smash the key crate (`level.#c913a45f` 1) and the Detective's
  crate; human again, take the key (`quest_key` clientfield), take the
  badge (`memento_detective_found`, keepers spawn), place it (craftable
  `ritual_detective`, `#469080d7` -> `ritual_detective_ready`), initiate
  (`zm_zod_quest::#79c39bdd`), stand in `defend_area_volume_detective`:
  progress 0..1 in 25 s, `ritual_detective_complete`. The Magician's
  memento drops 9.5 s after flag `power_on20` (the junction crane's
  district; `#0661aecc`). All four rituals then complete in one run
  (`T7_SETFLAGS=power_on20`, beast smashes at 22/25/28/31 s of the key,
  Detective, Femme and Boxer crates; altars stood at 30 units, e.g.
  Femme `7158 -4802 172~7136 -4781 172`, Boxer `2478 -9312 -216~2478
  -9282 -216`, Magician `1592 -5932 432~1592 -5962 432`; the key is taken
  again at its stand after each ritual): `ritual_<name>_complete` for
  all four by 300 s, each relic picked up off its altar. Left: the
  Pack-a-Punch ritual (relics in the rift's basins), gateworms.
- **Other maps: script loading.** All maps but `zm_zod` load `core_patch`, `zm_patch`, `zm_levelcommon`, then `<map>_patch` (a later zone's object of the same name wins; a map zone's own scripts are an older 0x1b build the patch supersedes). Headless `T7_MAP=zm_factory` (data from `t7_ents`/`t7_tables`/`t7_bundle`/`t7_havok_dump`/`t7_cmodels` on `zm_factory.ff`): rounds 1..3 with zombies. Unbound there: `isspawner`, `isplayerunderwater`, `getitemindexfromref`, `registerxp`, UI model builtins.
- **Power-ups.** `zombie_spawn_init` sets `no_powerups`; the tree's `enablePowerups` (called once the zombie is inside, `t7_ai`) clears it, as BO2's emerge into the playable area did. Straight-down traces with no brush hit fall back to the nav mesh floor (T7 maps only): `zombie_death_points` ground-traces the drop point and `powerup_drop` throws away drops outside `player_volume`. Headless: Max Ammo drops at 173 s (`T7_KILL=12`), grabbed with `T7_GRAB=1`.
- **Traps (chain traps).** `zm_zod_traps` runs unchanged: use trigger (1000), `trap_chain_damage` touch trigger (AI flags) hurts players (25) and kills zombies (`dodamage` 50% then 100% of max health, gib). Engine fixes: actors run `scripts/shared/ai/systems/init::main` before `codecallback_actorspawned` with their weapons set to "none" (`self.a`); `kill()` attacker defaults to the world; player damage/laststand/killed callbacks take weapon objects (13-arg damage callback). Powerups time out after ~26 s when not taken.
- **Parasite thinking.** Query point fields by canonical hash (`distToOrigin2D`, `inClaimedLocation`, `score`, `_scoreDebug`, query `centerOnNav`), `positionquery_filter_distancetogoal`, vehicle `setgoal` anchor + `goalpos`/`goalradius`/`goalheight`, `vehcansee`. Parasites hover at their engagement distance and spit, as the settings say. Headless caveat: `zm_zod::#35c958af` ground-traces under each parasite every ~1 s and re-queues it (`zombie_total++`, kill) after 5 misses outside the playable volumes; without world collision in the test (only brush models) parasites near the map edge miss, so forced parasite rounds can run long headless (seed-dependent: `CODLEGACY_T6_SEED`). Test: `T7_PEEK` reads level fields through the engine's getters.
- **Pods.** `zm_zod_pods`: one sprayer per `pod_sprayer_location` group (random), picked up through its unitrigger (UI-model field `pod_sprayer_held`); pods (`fungus_pod` structs) grow a level each round end and are harvested with the sprayer (`#7e428fa9`: weighted reward by level: craftable part, grenade, parasite, power-up, shield charge, weapon, zombie). Client fields: player-state/UI-model values stored per entity in `ClientFx::values` (`ps:`/`ui:` keys).
- **Other maps headless (2026-10-28).** `zm_castle`, `zm_island`, `zm_stalingrad`, `zm_genesis` (and `zm_factory`): rounds 1..3 by 80 s with `T7_KILL=6 T7_GOD=1`. Test data per map: `t7_ents <map>.ff`, `t7_tables zm_common zm_patch zm_levelcommon <map> <map>_patch --dump`, `t7_bundle` on the map and patch zones, `t7_havok_dump`, `t7_cmodels --planes`; run with `T7_MAP=<map>` and those files as `T7_ENTS`/`T7_TABLES`/`T7_BUNDLES`/`T7_NAV`/`T7_CMODELS`. `zm_theater` was blocked at its GfxWorld (asset 12234) until 2026-10-28 (see below). Not yet staged: `zm_asylum`, `zm_sumpf`, `zm_cosmodrome`, `zm_temple`, `zm_moon`, `zm_tomb`, `zm_prototype`. Script errors seen (not blocking rounds): `_zm_audio` vox tables, `_bb`, `gameskill_shared` difficulty settings, `vehicle_shared` on Zetsubou.
- **Pack-a-Punch ritual.** Chain: four character rituals (`#96a27419(name)` success: the relic piece moves to `quest_ritual_item_placed_<name>`), relics picked one at a time (one piece carried), `pap_door_open` (stand in `pap_door_trigger`), each relic crafted at `quest_ritual_usetrigger_pap` (2624 -3538 -484; stand at floor ~-480, |stub z - (player z + 35)| <= 42 and inside the 110x80 box), `ritual_pap_ready` starts the `pap` defend area (stub at `defend_area_pap`, started twice: two stubs); initiate requires `pap_basin_1..4` (`#7dde76aa`) and the player inside `defend_area_volume_pap`; defend then success (`#96a27419(pap)` -> `#8ff18033`, `ritual_pap_complete`). The basins are reached by wall runs (left/right friezes) and the middle islands: not driven headless (BO3 wall running is not in the sim); verified with the map's skip `#c0a29676`.
- **Main quest (zm_zod_ee).** Chain `#189ed812`: `ritual_pap_complete` -> `ee_begin` -> book/keepers (`#2e77f7bf`) -> Shadow Man (`#db49b939`: four `ee_keeper_<name>_armed`, boss stub at `defend_area_pap`, players in `defend_area_volume_pap` with swords) -> Apothicon (`#db8d1f6e`) -> ending (`#e186ed49`, `ee_complete`). Devgui skips usable headless: `#b44200f1` (begin), `#7a0bffae` (keepers), `#54098545` (to the Apothicon), `#d2f61efa` (flags only). VM: T7 thread calls return a finished thread's value (`Vm::t7_events`). Not driven headless: the sword quest (eggs, upgrades), the book/rail steps, making the Shadow Man vulnerable (keeper beams), damaging the Apothicon. No spawn closets or spiders on this map (those are Zetsubou/Revelations).
- **Zombie look and animation (2026-10-28).** The walk keeps `aitype` and `character` fields (`Capture::impact_fields`; `t7_characters <zone> [--cast]` prints them): an AI type's eight character slots (0x4b8..0x4f0), a character's body 0x60, head 0x70 or head alias 0x78 (an `xmodelalias` list), hat 0x88 and an extra 0x98 (Shadows of Evil: 11 zombie AI types, 55 characters). `lane::t7_map` passes a Zombies map's actor spawner AI types (`actor_spawner_X` -> `spawner_X`) to `lane::t7::map_props`, which builds those models with the map props and loads the base archetype's clips (`ai_zombie_base_*`, `bo3_ai_zombie_*`: walk/run/sprint/super-sprint, crawl, idles, attacks, board grab/hold/pull, window attack/traversal, deaths, taunts; numbered variants only, 561 names on `zm_zod`); the cast goes to the match as `T6Install::t7_cast`, the clips' timing as script anims (the Zombies path now takes the pending props too). `t7_ai`: a spawned actor wears one of its AI type's characters (body as its model, a random head and hat attached, `self.head`/`self.hatmodel`); clips by behaviour (`CLIP_KINDS`), a zombie keeping its own variant: the locomotion loop for `zombie_move_speed` (idle when it stood, stride rate from the ground covered), the melee clip (the swing lasts it, the hit at its `zombie_melee` note), board and window clips for the barrier actions (lasting their clip), the death clip. Headless with `T7_CAST` and `T7_ANIMS=1`. Not yet: the ASM/selector tables themselves, root motion, rise clips (not in `zm_zod.ff`), gib models.
- **Kino der Toten (2026-10-28).** The Chronicles remaster's GfxWorld has later sun volumes: fields from the shadow infos (1056) to the exposure grids lie 32 later, and 32 bytes follow the volume array (`walk::gfxworld::sun_volume_shift`, `GfxWorldRec::later`). `zm_theater.ff` now walks 30803 of 30804 assets; headless with its own scripts: rounds 1..3, power, doors (its start doors are electric: they open with power), wall buys, the box, perks, the foyer electric trap. `zm_prototype` stops earlier, at a material (not looked at).
- **Other maps' economy (2026-10-28).** Headless per map (`T7_ZONES=1`; power switch, nearest paid door, nearest wall buy, box bought and its gun taken, Quick Revive, Juggernog): all work on Kino, Der Eisendrache and Gorod Krovi; Zetsubou power/door/wall buy/box (its perk machines are placed at random: not found at the map's spots); Revelations door/perks/box (no power switch: generators). Fixes: string-table references past a walk drift are read at the fitted shift (Zetsubou's GfxWorld walk is 224 bytes short in its cells' tree lists, so its weapon table read as fragments and wall buys cost 0); bone quaternions 4-aligned; map keys missing from the name dictionary also set under their hash (Zetsubou's skull altars, `script_special`); weapon names trimmed (`dragon_gauntlet `); `updateattackspotservice` only with a `first_node`. Left per map: Zetsubou's GfxWorld miscount, `riotshield` wall-buy structs (unregistered weapon), the quests (Zetsubou skull/spiders/thrashers, Gorod dragons and gauntlet, Eisendrache bows and dragons, Revelations' generators and Apothicons, Kino's teleporter), `_zm_audio` vox table errors.
- **Boss fights damageable (2026-10-28).** Shadows of Evil's Shadow Man takes damage on `c_zom_zod_shadowman_tentacles_fb` once the keepers are armed in the defend volume with an upgraded sword (`ee_boss_vulnerable`); 500 a hit teleports him, eight captures end the fight (`ee_boss_defeated`). The Apothicon: rails, keepers shot with the beast's lightning, the beam, the train lever, a second beam: `ee_final_boss_defeated`, then the ending: `ee_complete` headless by 235 s (`scratchpad` scripts `boss.sh`/`apo.sh`).
- **Per-map completeness (2026-10-28).** Zetsubou's labs power needs a full bucket and the scene notetrack `water_drain_complete` (animscripted notetracks now reach the scripts as notifies); its perks are bought where the round's shuffle puts them (Double Tap, Speed Cola); Juggernog waits for the bunker's deferred power. Revelations' four corruption engines (stand within 80 facing the front, hold use, 500, kills charge it): `all_power_on`. Gorod Krovi's store trap kills. VM: a builtin reached through a function pointer whose form is unbound runs its other form (`hide`, `notsolid`, `attackbuttonpressed`). `spawnactor` from another actor spawner when the type has none (the Furies).
- **Quests headless (2026-10-28).** Kino: power, pad, mainframe link (`teleporter_linked`), the pad again: projector room, a bonus room, back to the theater; a film reel into the projector starts its film. Der Eisendrache: eight kills in each dragon head's volume (`dragon_position`'s target) feed it; all three: `soul_catchers_charged`, the broken bow appears and its pickup gives `elemental_bow` (blocker fixed: `weaponcyclingenabled`/`offhandweaponsenabled`, unnamed builtins `#d7cc60fb`/`#17893c41`, returned undefined). Revelations: after power, the Pack-a-Punch sac's three cords shot: `apotho_pack_freed`. Not reached: Gorod Krovi's Pack-a-Punch lockdown waits for `dragon_pavlov_first_time` (the first dragon ride), which needs the Dragon Network Controller crafted, whose parts' structs read at a prefab's local origin (0 0 -273): the prefab offset is not applied in the extracted entities. Zetsubou's skull rituals need the skull room's seal opened first (the use there bought the nearest door instead). Gorod's main quest, Der Eisendrache's bow upgrades, Zetsubou's KT-4 and Revelations' main quest not tried. Test aids: `T7_KILL_AT="x y z r [pull]@a-b;…"`, `T7_DUMP_AT`, `T7_SHOOT` at a named entity itself, player position in the frame line.
- **Round 15+ and the game path (2026-10-28).** Fixes: a map zone's own scripts load before its patch (Gorod Krovi's `vehicles/_sentinel_drone`, `vehicles/_dragon_whelp`, `ai/raz`, `_zm_elemental_zombies` were missing; Der Eisendrache, Zetsubou and Revelations have none of their own); `getspawnerarray` returns vehicle spawners; spawned AI vehicles are sentient, so their spawner's spawn functions run and a sentinel round ends (`last_ai_down`). Gorod Krovi's Dragon Network parts are parked at 0 0 -273 by design: a code cylinder (kill drop) at its terminal calls a drop pod whose end moves a part out (two of three headless with `CODLEGACY_T6_SEED=7`). Zetsubou: holding use on the penstock web (-720 3679 -516) tears it, the bunker defend ends (`defend_over`), the bunker switch (-1554 3465 -375) turns `power_on`; Juggernog (351 2809 -384) then bought. Long runs (`scratchpad/longrun.sh <map> 10 7000`, god mode healed every frame, kills 2 s after spawn): Shadows of Evil, Kino, Zetsubou, Gorod Krovi and Revelations from round 10 to 14-15 and Der Eisendrache from 12 to 16 with no hang and 70-130 script warnings each (one-off, recovering); round health `level.zombie_health` 1263/1389/1527/1679 at rounds 12-15, Black Ops III's 150 + 100 a round to 9 then x1.1. Not checked: zombie counts per round against BO3's table, each map's forced special rounds but Gorod's sentinels.
- **Game path gaps (from the code, not run here: it needs the owner's PC).** (1) The menu starts `map t7:<zone>` with gametype `zclassic`, but Black Ops III's own scripts run only with `BO2MP_T7_SCRIPTS=1`; without it the BO2 kit's scripts run on the BO3 map. (2) Kino der Toten's menu entry is Black Ops' (`zm_theater`, BO1 assets); the Chronicles map (`t7:zm_theater`) that runs headless has no entry. (3) The headless run builds its own Black Ops III weapon catalog (zombies tables plus start pistols, glaives, bows, gauntlet, KT-4); the game path has the BO2 kit's weapons and the multiplayer guest guns (`BO3_GUNS`), so `pistol_m1911`, wonder and quest weapons resolve to nothing there. (4) HUD: round (`level.round_number` -> `bo2zm_round`) and points (`score`) are the shared sim's; perks now map from `hudItems.perks.*` to BO2's `perk_*` icons (Widow's Wine has no BO2 icon); power-up, ammo and BO3-only HUD models (`hudItems.showDpad*`, the shield, the bow charge) are not drawn. (5) The game path has never been run with the BO3 scripts on any map.
- **Game path closed (2026-10-28, untested on the PC).** (1) BO3's scripts are on by default for BO3 Zombies maps (`BO2MP_T7_SCRIPTS=0` turns them off). (2) The picker's BLACK OPS III page lists, when installed: Shadows of Evil, The Giant, Der Eisendrache, Zetsubou, Gorod Krovi, Revelations, Verrückt, Kino der Toten (Chronicles), Shangri-La, Moon, Origins; left out until their zone walks finish: Nacht der Untoten (a material, asset 11901), Shi No Numa (GfxWorld 8361), Ascension (a GfxWorld image, 10313); BO3 tiles have no picture yet (front-end streamed art). (3) Weapons: BO3's multiplayer guns and their `_upgraded_zm` copies are real BO3 guns (core_common guests); others go to BO2 stand-ins, now including the wonder and quest weapons. Missing as BO3's own: everything defined in map zones and `zm_levelcommon` (Thundergun, Wunderwaffe, Ray Gun Mk III, KT-4, bows, gauntlet, glaives, Annihilator, Chronicles guns such as `pistol_m1911`, `ar_m14`, `smg_mp40`). (4) HUD: use hints now read BO3's English strings (`en_core_patch`, `en_zm_patch`, `en_zm_levelcommon`, `en_<map>[_patch]`); round, points, ammo and perk icons come through Black Ops II's HUD; not drawn: BO3's objective/quest widgets (`hudItems.*` beyond perks), Widow's Wine's icon, the BO3 weapon names in the HUD's weapon slot (the LUI's strings are the kit's).
- **Steam Workshop Zombies maps (2026-11-04).** Mod-tools scripts use opcodes with bit 13 (0x2000) set: the VM clears that bit and runs `SLOTS_HI[op & 0x1fff]`, a second full handler table (`gen_slots.rs`, from the exe by `t7gen.py`). Community maps have no patch zone: `script_zones` takes the map zone itself. Headless test data as for the retail maps, with the map zone's own objects as `T7_SCRIPTS_DIR/<map>_patch`. 80 s with `T7_KILL=6 T7_GOD=1`: `zm_town_hd`, `zm_town`, `zm_farm_hd`, `zm_giant`, `zm_halloween`, `zm_waterparkfinale` round 3; `zm_prison` (begin_spawning never set, the player goes down), `zm_die` and `zm_pentagon` (no AI by 80 s, `T7_ZONES=1` no different) not yet; `zm_nuked` stops at its GfxWorld (sun volume records read at the wrong place; the Kino later-layout shift does not fit). `zm_alcatraz_island` and `zm_coast` zones are over the bridge's 400 MB staging limit.

### Headless audit, 2026-11-04 (every Zombies zone available in the cloud)

Run with the `t7_test` harness (`shadows_of_evil_level_starts_with_its_own_scripts`
with `T7_MAP`, `T7_SCRIPTS_DIR`, ... ; god mode, one player, 1200-2600
frames). "ok" = the map's own scripts start the round loop, spawn zombies
through the behaviour tree and the round advances.

| Map | Headless result |
|---|---|
| zm_zod (Shadows of Evil) | ok; rounds 1/2/3 spawn 6/8/13 zombies solo (BO3's own counts) |
| zm_factory (The Giant, official) | ok |
| Workshop zm_giant, zm_farm_hd, zm_town, zm_halloween, zm_waterparkfinale | ok, rounds 1-2 |
| Workshop zm_prison (Mob of the Dead) | ok once the afterlife intro (~70 s) is over |
| Workshop zm_die (Die Rise) | ok once the Dobby intro movie wait ends (79.5 s timeout; no movie plays headless); round 2 reached |
| Workshop zm_pentagon (Five) | partial: spawns one zombie, then a script thread spins and the round stalls (not yet traced) |
| zm_town_hd, zm_nuked | not walkable by the fastfile reader yet |
| castle, island, stalingrad, genesis, ... | the map zones are not in the cloud; untestable here |

What was wrong and is fixed:

- **Scripts were off by default in `t7_zm_scripts::enabled()`**: they are
  now on unless `BO2MP_T7_SCRIPTS=0|off|false` (test
  `black_ops_iii_scripts_are_on_unless_switched_off`).
- **Workshop scripts did not even load** (fixed by the parallel Workshop work as `SLOTS_HI`): the Mod Tools build their
  bytecode with opcode values that have bit 13 set (`0x2000 | n`), which
  select the VM's second handler table (the same 127 handlers in another
  order). `gsc_t6::t7::ops::SLOTS_HI` is that table (generator
  `t7gen.py`); `op_of` honours bit 13.
- **A Workshop map has no `<map>_patch` zone**; it fell back to the Black Ops II
  kit. It now runs its own scripts alone (core_patch, zm_patch, the map).
- **Custom front-end menus blocked the round loop**: Dobby maps open the LUI
  menu `PreGameMenu` and wait for `start_game`; Five opens `ModeSelect`
  and waits for a mode. The front end cannot draw them, so the sim answers
  them (`PreGameMenu` -> `start_game`, `ModeSelect` -> `classic`), a second
  later, as the menu's reply. Override with
  `BO2MP_T7_MENU_REPLIES=<menu>=<response>,...` (`none` leaves one unanswered).

Still different from BO3 (not fixed, listed so nobody assumes otherwise):

- (Fixed 2026-11-04, see "Fidelity pass" below: zombie speeds from the clips'
  root motion, Five's stall, the cam-anim builtins, the client scripts.)
- GobbleGums are not implemented on purpose (out of scope); the
  `gameskill_shared` difficulty setup and the VOX/bot tables read script
  bundles and string tables of `core_common` (not in the cloud zones) and
  report script errors in a headless run only.

### Fidelity pass, 2026-11-04 (root motion, Five, cam anims, client scripts)

Status of the six items (headless unless said; the owner's PC still has to
look at all of it):

| # | Item | State |
|---|---|---|
| 1 | Zombie speeds from root motion | done. `fastfile_t7::walk::anim` decodes the delta translation (`mins + size * value`; the stored `size` is already the span / 255 or / 65535); `T6AnimFacts::delta_trans` carries it; `t7_ai::ground_speed` is the ground the zombie's own locomotion clip (its variant) covers a second. Measured on Shadows of Evil: walk 23-53, run 57-132, sprint 140-160, super sprint 176, crawl 14-89 u/s (was 60/130/200/250). The round ramp is the scripts': walk at round 1, run by 8, sprint by 15. `BO2MP_T7_SPEEDS=standin` keeps the old numbers. Test: `T7_ZONES=<dir> cargo test --release -p fastfile_t7 --test anim_root_motion`. |
| 2 | Five stalls after one zombie | fixed at the root, headless rounds 1-4. The spinning thread was Five's own `_zm_developer_commands` polling the Mod Tools devgui variable `add_bots`: `modvar(name, value)` was unbound, so the poll's reset never ran and it called `addtestclient()` every frame (and `getdvarint("add_bots", 1)` kept answering 1). `modvar` now sets the dvar; `addtestclient` is a no-op. Also: the headless out-of-playable-area kill needs `T7_SETLEVEL='#c303b23b=1' T7_SETLEVEL_AT=10` on Five (new `T7_SETLEVEL_AT`, frames; its kill comes at 7 s). |
| 3 | Missing builtins / script errors | cam anims bound (`camanimscripted`, `endcamanimscripted`, `extracamanimscripted`, `endextracamanimscripted`: no camera, the scene goes on; `#5f3e774c` = `getcamanimtime`: the clip's length if known, else 0), `modvar`, `addtestclient`, `getitemindexfromref`, `getbaseweaponitemindex`, `weaponhasattachment`, `currentsessionmode`, presentation no-ops (`sethighdetail`, `setlightingstate`, character styles, ...). Remaining headless errors on every map are data the cloud zones lack, not engine gaps: `gameskill_shared` (script bundles `gamedifficulty_*`), `_bot` (`bot_default`), `_zm_audio` (VOX string tables), scene bundles of `core_common`. GobbleGum calls (`_zm_bgb`) are harmless. `BO2MP_T7_GAMESKILL=1` runs the difficulty setup when its bundles are loaded. Not fixed: unnamed builtins (`.#b15ba9ed`, `.#67f028cc`, `#27c72c1b`, ...) still return undefined. |
| 4 | Client scripts | foundation done, `sim::t6::t7_client`: the zone's `.csc` objects (297 on Shadows of Evil, 2688 functions) are translated (`gsc_t6::t7::translate_ext`) and run by a second VM on the same world: autoexecs, the map's `main`, `codecallback_*`, then each client field the server sets (`codesetclientfield`, `codesetworldclientfield`, `codesetplayerstateclientfield`) runs its registered callback `(localClientNum, old, new, newEnt, snapshot, name, timeJump)` on a stand-in entity. Headless on Shadows of Evil: 359 fields registered, ~380 callbacks run (bminteract, eye_candy, perk bottle/box/power-up fx, fungus pods, train lights, trap state, shadowman fx...). `playfx`/`playfxontag`/`playsound` go out as the same events the server natives raise. Not drawn yet: lighting states, fog/sun banks, post-effects, vision sets, light and exploder setters (accepted, counted by `T7Client::unbound`), scene objects (the client VM has no script bundles), the unnamed per-frame builtin `.#f72343a0`. `BO2MP_T7_CLIENT=0` turns it off. |
| 5 | Zone readers (`zm_nuked`, `zm_town_hd`) | not fixed: both zones are not in the cloud, and the GfxWorld sun volume layout cannot be derived blind. Their scripts run headless from the earlier extracted data (`zm_town_hd` rounds 1-4; `zm_nuked` has no entity data and stays at round 1). |
| 6 | Round / economy / AI | the values are the scripts' own (counts 6/8/13, health 150 + 100 a round to 9 then x1.1, box and perk tables from the zone's string tables); the speed ramp above was checked at rounds 1/8/15/25. Not compared one by one against the retail game; no stand-ins were changed. |

Headless result per map (3000 frames, one god-mode player, `T7_KILL=6`, the
frame count includes the intro waits):

| Map | Result |
|---|---|
| zm_zod, zm_factory (The Giant) | rounds 1-4, client scripts run (zod) |
| zm_giant, zm_town, zm_town_hd, zm_farm_hd, zm_halloween, zm_waterparkfinale, zm_pentagon | rounds 1-4 |
| zm_prison, zm_die | round 3 (afterlife / Dobby intro waits first) |
| zm_nuked | round 1 (its entity data is missing: the zone does not walk) |
