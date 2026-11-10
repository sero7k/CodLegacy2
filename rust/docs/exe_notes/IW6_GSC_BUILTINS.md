# Ghosts (IW6 MP) script builtins

Names for the numeric builtin ids that compiled Ghosts GSC calls. Scripts carry only ids: functions are below 0x8000, methods are 0x8000 plus the slot. Nothing in the exe maps id to name, so every name here comes from the handler function behind the id (arguments read, fields written, helpers called, errors raised) and from how scripts call it. Full table: `iw6_builtins.tsv` (columns: kind, id, name, argspec, returns, what, confidence, evidence, handler, static_calls, mainrun_calls). Rust: `gsc_iw6::builtins::{name, lookup, field_name, field_id}`; `NameTable::known()` also loads `crates/gsc_iw6/data/builtins_iw6.names` and `fields_iw6.names`, so `gsc_dis` prints names by default.

Confidence: VERIFIED = handler behaviour and call sites agree, and the behaviour leaves no other reading. LIKELY = handler fits one name well but a sibling is possible. GUESS = shape only; the name carries a `?` or an `unk_` prefix.

Counts: 254 rows, 74 GUESS, 60 LIKELY, 120 VERIFIED.

## Conventions seen in the handlers
- The VM pushes arguments last to first; argument 0 is the top of the stack.
- Methods take the entity from the entity array (stride 0x2f0 bytes; client pointer at +0x168, agent pointer at +0x178). Origin is at +0x140, angles at +0x14c, classname at +0x1a8, flags at +0x1c0, svFlags at +8.
- Mover methods set the trajectory type at +0xc: 5 gravity move, 9 physics launch, 10 animated script model.
- svFlags 0x20 with a client mask of -1 means hidden from every client (`hide`).
- Functions that read a string from the script and look it up in a config-string table are the precache family (`precachemodel`, `precacheshader`, ...); they differ only by table.

## Important entries by area

### Core, dvars, types

| id | name | args | what | conf |
|---|---|---|---|---|
| 0x2d | isdefined | v:any | false for undefined and for freed/non-value objects; true for any other value | V |
| 0x37 | getdvar | name:string,[default:string] | dvar value as string | V |
| 0x39 | getdvarfloat | name:string,[default] | dvar as float (atof of the string) | V |
| 0x32 | setdvar | name:string,value:string/any | sets a dvar from script; vector args are concatenated; double quotes are replaced by single quotes | V |
| 0x38 | getdvarint | name:string,[default] | dvar as int (atoi of the string) | V |
| 0x2f | isstring | v:any | type == string | V |
| 0x63 | spawn | classname:string,origin:vec,[spawnflags:int],[ignoreSpawnflag:int] | creates an entity of a classname (script_model, script_origin, trigger_radius ...) at origin | V |
| 0x3b | gettime | - | level time in ms | L |
| 0x5b | getweaponattachments | weapon:string | attachment names of a weapon string (up to 4) | L |
| 0x34 | setdvarifuninitialized | name:string,value:any | only sets the dvar when it has no value yet | V |

### Math, vectors, strings

| id | name | args | what | conf |
|---|---|---|---|---|
| 0xa6 | randomint | max:int | [0,max) | V |
| 0xa7 | randomfloat | max:float | [0,max) | V |
| 0xb2 | abs | v:float | absolute value | V |
| 0xb0 | int | v:string/float/int | cast to int | V |
| 0xe9 | vectordot | a:vec,b:vec | dot product | V |
| 0xa9 | randomfloatrange | min:float,max:float | [min,max) | V |
| 0x107 | vectornormalize | v:vec | v divided by its length (length 0 -> unchanged) | V |
| 0xd5 | max | a:float,b:float | larger of two | V |
| 0xb3 | min | a:float,b:float | smaller of two | V |
| 0x116 | tolower | string | lowercase copy | V |
| 0x113 | issubstr | haystack:string,needle:string | strstr != 0 | V |
| 0x10c | anglestoright | angles:vec | right vector | V |
| 0xe1 | distance | a:vec,b:vec | euclidean distance | V |
| 0x108 | vectortoangles | v:vec | direction -> (pitch,yaw,0) | V |
| 0x10d | anglestoforward | angles:vec | forward vector | V |
| 0x66 | bullettrace | start:vec,end:vec,hitchars:int,[ignore:ent],[...] | trace; result array has position,fraction,entity,normal,surfacetype | V |
| 0xe3 | distancesquared | a:vec,b:vec | squared distance | V |
| 0xb7 | getnode | value:string,key:string | path node with node.<key> == value (error when several) | V |
| 0x10b | anglestoup | angles:vec | up vector | V |
| 0x89 | getnorthyaw | - | north yaw stored in configstring 0x499 (set by setminimap) | L |
| 0xab | cos | deg:float | cosine of an angle in degrees | V |
| 0xaa | sin | deg:float | sine of an angle in degrees | V |
| 0xec | visionsetthermal | name:string,[time:float=1] | sets configstring 0x49f "name" ms (default 1000 ms) | L |
| 0xed | visionsetpain | name:string,[time:float=1] | configstring 0x4a0 | L |
| 0x117 | strtok | string,delims:string | split on any delimiter char | V |
| 0xc3 | isai | ent | true for bots and agents | L |
| 0x115 | getsubstr | string,start:int,[end:int] | substring [start,end) (<=1023 chars) | V |
| 0xa8 | randomintrange | min:int,max:int | [min,max) | V |
| 0xdc | clamp | v:float,min:float,max:float | v clamped to [min,max] | V |
| 0x84 | physicstrace | start:vec,end:vec | end point of a physics (contents 0x820015) trace | L |
| 0xb1 | float | v:string/float/int | cast to float | V |
| 0xb8 | getnodearray | value:string,key:string | all path nodes matching | V |
| 0xd6 | floor | v:float | floorf | V |
| 0xd7 | ceil | v:float | ceilf | V |
| 0xda | sqrt | v:float | square root | V |
| 0xe4 | length | v:vec | vector length | V |
| 0xe7 | length2dsquared | v:vec | x*x+y*y | V |
| 0x114 | isendstr | string,suffix:string | string ends with suffix | V |

### Precache, vision, level config

| id | name | args | what | conf |
|---|---|---|---|---|
| 0x13a | precacherumble | name:string | init-only; non-empty name; registers a rumble config string | L |
| 0x13e | precachestring | s:istring | init-only; registers a localized string | V |
| 0x136 | visionsetnaked | name:string,[time:float=1] | configstring 0x49c "name" ms | V |
| 0x137 | visionsetnight | name:string,[time:float=1] | configstring 0x49d | V |
| 0x138 | visionsetmissilecam | name:string,[time:float=1] | configstring 0x49e | V |
| 0x13c | precacheitem | name:string | init-only; G_FindItem(pickup name) must exist | V |
| 0x13f | precachemenu | name:string | init-only; registers a menu | V |
| 0x130 | getmatchrulesdata | path... | reads the match-rules structured data (mp/recipes.def); 2..5 path args | V |
| 0x131 | isusingmatchrulesdata | - | match rules data present | L |
| 0x13d | precacheshader | name:string | init-only; registers a material | V |

### Effects, entities, tables, nodes, hudelems

| id | name | args | what | conf |
|---|---|---|---|---|
| 0x1a6 | getnextarraykey | array,key | next key after key; undefined at the end | V |
| 0x1c1 | getfirstarraykey | array | first key; undefined for empty | V |
| 0x189 | getentarray | [value:string,key:string] | all matching entities; no args = every in-use entity | V |
| 0x188 | getent | value:string,key:string | the entity whose <key> field == value (error when several) | V |
| 0x143 | loadfx | path:string | fx index; prepends fx/ when the path lacks it | V |
| 0x18c | spawnstruct | - | new empty struct | V |
| 0x15e | precachemodel | name:string | init-only; registers a model | V |
| 0x1c0 | getarraykeys | array | keys of an array | L |
| 0x1a7 | sortbydistance | array,origin:vec | entities sorted by distance (max 0x800) | V |
| 0x148 | playloopedfx | fx:int,repeat:float,origin:vec,[cull:float],[forward],[up] | looped fx | V |
| 0x200 | connectnodepair | node1,node2,[dontbidirectional:int] | links two path nodes (both ways unless 3rd arg set) | V |
| 0x1be | setminimap | material:string,x1,y1,x2,y2 | configstring 0x49a with the map rectangle; checks north yaw | L |
| 0x1a8 | tablelookup | file:string,searchcol:int,value:string,returncol:int | first row whose column searchcol == value | V |
| 0x19a | isplayer | ent | entity has a player client | V |
| 0x196 | newhudelem | - | server-wide hud element | V |
| 0x1a9 | tablelookupbyrow | file:string,row:int,col:int | cell value | V |
| 0x144 | playfx | fx:int,origin:vec,[forward:vec],[up:vec] | one-shot effect at a position | V |
| 0x149 | spawnfx | fx:int,origin:vec,forward:vec,[up:vec] | creates an fx entity (use triggerfx) | V |
| 0x14a | triggerfx | fxent,[delay:float] | starts (or restarts) a spawnfx entity | V |
| 0x195 | playsoundatposition | origin:vec,alias:string | one-shot sound at a position | L |
| 0x1e9 | isagent | ent | entity is an agent | L |
| 0x18e | isalive | ent | ent.health > 0 | L |
| 0x1a5 | getbaseweaponname | weapon:string | weapon name without attachments/alt prefix | L |
| 0x1ac | tablelookupgetrownum | file:string,col:int,value:string | row number or -1 | V |
| 0x145 | playfxontag | fx:int,ent,tag:string | effect attached to a model tag | V |
| 0x1aa | tablelookupistring | file:string,searchcol:int,value:string,returncol:int | like tablelookup but returns a localized string | L |
| 0x1d0 | weaponclass | weapon:string | weapon class name (rifle, smg ...) | L |
| 0x1d8 | getvehiclenodearray | value,key | all matches | V |
| 0x1e8 | isbot | ent | entity is a bot | L |
| 0x179 | earthquake | scale:float,duration:float,source:vec,radius:float | screen shake event (g_earthquakeEnable) | V |
| 0x186 | iprintlnbold | string... | prints to every client (concatenated args) | L |
| 0x1d7 | getvehiclenode | value,key | first match; error on several | V |
| 0x201 | disconnectnodepair | node1,node2,[dontbidirectional:int] | unlinks two path nodes | V |
| 0x170 | physicsexplosionsphere | origin:vec,radius:float,inner:float,magnitude:float | pushes physics objects | L |
| 0x197 | newclienthudelem | player | hud element for one player | V |
| 0x212 | activateclientexploder | n:int,[players:ent or array],[delay:float] | asks clients to run client-side exploder n (optionally only for some players, after delay) | L |

### Methods: entities, movers, linking, sound

| id | name | args | what | conf |
|---|---|---|---|---|
| 0x82ad | rotateroll | delta,time,[accel],[decel] | axis 2 | V |
| 0x81ac | getentitynumber | - | entity number | V |
| 0x80a9 | delete | - | removes a script entity (error on players, clients, or when linked children) | V |
| 0x804a | linkto | parent:ent,[tag:string],[offset:vec],[angles:vec] | attaches an entity to a parent (optionally a tag) | V |
| 0x82b2 | rotateto | angles:vec,time,[accel],[decel] | rotate to angles | V |
| 0x80aa | setmodel | model:string | sets the model | V |
| 0x826e | useanimtree | tree | binds the entity to an anim tree (entity must be animatable) | L |
| 0x80d1 | setcursorhint | hint:string | HINT_NOICON, HINT_ACTIVATE ... for use triggers | V |
| 0x8092 | getorigin | - | current origin | V |
| 0x81d9 | getlightintensity | - | primary light intensity | L |
| 0x8051 | hide | - | hides from every client | L |
| 0x8073 | playloopsound | alias:string | loop sound on the entity | V |
| 0x80b8 | makeusable | - | entity can be used (+contents) | L |
| 0x826d | clonebrushmodeltoscriptmodel | brush:ent | copies a brush model's collision model onto this script_model | V |
| 0x80d2 | sethintstring | istring... | use-prompt text | V |
| 0x81da | setlightintensity | v:float | primary light intensity (>=0) | L |
| 0x82b4 | notsolid | - | clears collision contents | V |
| 0x8057 | disconnectpaths | - | unlinks nodes inside this brush entity | V |
| 0x8056 | connectpaths | - | re-links the path nodes blocked by this brush entity | V |
| 0x82a3 | moveto | origin:vec,time:float,[accel:float],[decel:float] | linear move | V |
| 0x804c | unlink | - | detaches from the parent; resets the player link state | V |
| 0x8097 | playsound | alias:string,[notify:string] | sound at the entity | V |
| 0x817b | gettagorigin | tag:string | world origin of a model tag | V |
| 0x80a2 | istouching | ent | bounding boxes overlap | V |
| 0x8050 | show | - | clears the hidden-from-clients state | L |
| 0x82ea | playlocalsound | alias:string | sound for one player | V |
| 0x80a4 | stoploopsound | [alias] | stops the loop sound | V |
| 0x8211 | getpointinbounds | x:float,y:float,z:float | point inside the entity bounds by 0..1 fractions | L |
| 0x8242 | buttonpressed | name:string | always 0 on the server build | V |
| 0x817c | gettagangles | tag:string | world angles of a model tag | V |
| 0x82f2 | setclientdvar | name:string,value | sends a dvar value to one client | V |
| 0x8081 | fadeovertime | time:float(0..60] | hudelem fade | L |
| 0x8085 | destroy | - | frees the hud element | L |
| 0x82b3 | solid | - | sets collision contents 0x2080 on script_model/brush entities | V |
| 0x80c1 | settext | istring... | hudelem text (args concatenated) | L |
| 0x817d | shellshock | name:string,duration:float,[?] | shellshock (0..60 s) | L |
| 0x821c | getrankedplayerdata | path... | ranked player data | V |
| 0x826f | scriptmodelclearanim | - | clears the entity's script animation (LIKELY; from the handler beside 0x826e/0x8270) | L |
| 0x82f1 | setclientomnvar | name, value | typed HUD number (string name; bool / float / int by the value's type; LIKELY from the argument shape) | L |
| 0x8270 | scriptmodelplayanim | anim:string | plays a looping animation on a script_model | L |
| 0x8177 | getstance | - | stand,"crouch" or "prone" | L |
| 0x82ec | setweaponammoclip | weapon:string,count:int,[hand] | sets clip ammo | L |
| 0x8003 | playsoundtoteam | alias:string,team:string,[excludeplayer:ent] | plays a sound at the entity only for the players of a team | L |
| 0x8072 | playsoundasmaster | alias:string,[notify:string] | plays a sound at the entity as a master sound | V |
| 0x80a1 | geteye | - | eye position of a player/agent | L |
| 0x80c3 | setshader | material:string,[w:int,h:int] | hudelem material | L |
| 0x8181 | setdepthoffield | nearStart,nearEnd,farStart,farEnd,nearBlur,farBlur | dof (blur 4..10) | V |
| 0x8261 | suicide | - | kills the player | L |
| 0x829b | setperk | perk:string,[?],[?] | gives a perk | V |
| 0x82ae | devaddyaw | delta:float | editor helper (axis 1) | L |
| 0x82af | devaddroll | delta:float | editor helper (axis 2) | L |
| 0x82b0 | devaddpitch | delta:float | editor helper (axis 0) | L |
| 0x82b7 | physicslaunchserver | [velocity:vec,angvel:vec] | turns the entity into a physics mover with an initial velocity/spin | L |
| 0x82cd | setblurforplayer | blur:float,time:float | blur_target / blur_duration_ms client fields | V |
| 0x82ee | getweaponammoclip | weapon:string,[hand] | clip ammo | L |
| 0x80b9 | makeunusable | - | clears usability | L |
| 0x80c6 | settimer | seconds:float | hudelem count-down timer | L |
| 0x80ce | setvalue | v:float | hudelem numeric value | L |
| 0x821d | getprivateplayerdata | path... | private player data | V |
| 0x823b | setrankedplayerdata | path...,value | exe symbol | V |
| 0x823c | setprivateplayerdata | path...,value | exe symbol | V |
| 0x8245 | setspawnweapon | weapon:string | selects the weapon the player holds on spawn | L |
| 0x826a | getguid | - | player guid | V |
| 0x8292 | getspectatingplayer | - | the entity a spectator is following (undefined when none) | L |
| 0x829d | clearperks | - | clears all perks | V |
| 0x829e | unsetperk | perk:string,[?] | removes a perk | V |
| 0x82a7 | movegravity | velocity:vec,time:float | ballistic move under gravity | L |
| 0x82ed | setweaponammostock | weapon:string,count:int | sets stock ammo | L |
| 0x82ef | getweaponammostock | weapon:string | stock ammo | L |
| 0x82fc | setweaponhudiconoverrides | slot:int(1..7),kind:string,[weapon] | weapon,"altmode","nightvision","none" | V |
| 0x82a4 | movex | dist:float,time,[accel],[decel] | move along X by a distance | V |
| 0x82a5 | movey | dist,time,[accel],[decel] | axis 1 | V |
| 0x82a6 | movez | dist,time,[accel],[decel] | axis 2 | V |
| 0x82ab | rotatepitch | delta:float,time,[accel],[decel] | axis 0 | V |
| 0x82ac | rotateyaw | delta,time,[accel],[decel] | axis 1 | V |

### Methods: players, weapons, hud

| id | name | args | what | conf |
|---|---|---|---|---|
| 0x830c | setoffhandsecondaryclass | name:string | one of 6 class names | L |
| 0x8338 | setoffhandprimaryclass | name:string | offhand primary class | L |
| 0x8305 | getcurrentweapon | - | current weapon or "none" | V |
| 0x832f | getplayerangles | - | view angles | L |
| 0x8300 | getweaponslistall | - | all carried weapons (and alt modes) | V |
| 0x8302 | giveweapon | weapon:string,[camo:int],[akimbo:int],[?],[?] | gives a weapon | V |
| 0x8325 | givemaxammo | weapon:string | refills ammo of a carried weapon | L |
| 0x8404 | playsoundonmovingent | alias:string | sound that follows the entity (temp event 3); used by _movers | L |
| 0x8303 | takeweapon | weapon:string | removes a weapon | V |
| 0x8308 | hasweapon | weapon:string | true if carried | V |
| 0x830d | getoffhandsecondaryclass | - | reads the same | L |
| 0x8316 | openpopupmenu | menu:string | opens a menu on the client | L |
| 0x8318 | closepopupmenu | [menu] | closes the menu | L |

## Unresolved or only guessed (important ones)
- 0x82aa: rotateto_shortest (shape matches rotateto but wraps each axis).
- 0x82b5, 0x82b6: used by script_vehicle damage code; one stores a byte at +0x19b, the other toggles entity flag 0x10. Names unknown.
- 0x8018, 0x8019, 0x8026, 0x8027, 0x8028: object methods used by `_destructible` (part visibility / sound on tag). Names unknown.
- 0x833d: sets an entity subtype from nine names (helicopter, airstrike, explosive, ...): a killcam or damage-source type setter.
- 0x8375: brush entity helper, probably moving-platform related.
- 0x118 (string compare, 0 when equal), 0x175 (maps 19 engine constant strings to small ints), 0x176 (debug text, no-op on the server), 0x65 and 0x133 (dedicated-server stubs).
- The player flag set/clear pairs (0x8310/11, 0x8312/13, 0x8314/15, 0x82d6/d7, 0x830e/0f, 0x831c/1d/1e): each toggles one client flag; the flag bit is in the TSV `what`. They are perk or control restrictions (allow-X style) but which is which cannot be told from the handlers alone.

## Engine field ids
Script field reads (`self.origin`, `ent.targetname`) are compiled to token ids. Each class (entity, player client, agent, vehicle, hudelem, path node, vehicle node) registers its fields through one helper that stores the name and a key next to it; that key is the token id scripts use. 119 are in `crates/gsc_iw6/data/fields_iw6.names` (`token <id> <name>`), all VERIFIED from the exe tables, for example origin 430, angles 47, model 392, targetname 642, target 640, classname 97, code_classname 99, spawnflags 542, health 271, script_noteworthy 506, hudelem x/y/z 731/732/733. The string table behind non-code names (for example a script's own variable names, `#31490`) is built at runtime, so those cannot be named from the exe.

## Surprises
- The earlier notes in `IW6_GSC.md` guessed 0x8056/0x8057 = hide/show and 0x15e = precache anim. They are connectpaths/disconnectpaths and precachemodel. The real hide/show are 0x8051/0x8050 (LIKELY).
- 0x82ad is rotateroll, not movez; movex/y/z are 0x82a4..0x82a6.
- playsound (0x8097) and playsoundasmaster (0x8072) are two distinct handlers; the first is the normal one-shot.
- `setsaveddvar` has the same shape as `setdvar` but skips the existing-variable lookup (GUESS on the name).

## 0x175 (function): MOD name is explosive (read 2026-11-04)

Takes a MOD name string, looks it up in the 19-entry MOD name table (order: MOD_UNKNOWN, PISTOL_BULLET, RIFLE_BULLET, EXPLOSIVE_BULLET, GRENADE, GRENADE_SPLASH, PROJECTILE, PROJECTILE_SPLASH, MELEE, HEAD_SHOT, MELEE_DOG, MELEE_ALIEN, CRUSH, FALLING, SUICIDE, TRIGGER_HURT, EXPLOSIVE, IMPACT) and returns the byte at that index of the flag table at 0x1409e8630: set for indices 3 to 7 and 16, so EXPLOSIVE_BULLET, GRENADE, GRENADE_SPLASH, PROJECTILE, PROJECTILE_SPLASH, EXPLOSIVE. Table order of names is from the registration sequence (the pointer table's strings sit behind relocations in the unpacked file). The gate and warehouse damage handlers use it.

## Events pass (2026-11-04)

Read from the handlers (the handler table was parsed from the exe's two builtin tables, handler at +16 and id at +8 of each 24-byte row; 184 of 184 rows agree with the TSV):

- 0x81d0 `radiusdamage(origin, range, max, min, [attacker], [MOD], [weapon])`: blast from the entity as inflictor; range and damages must not be negative.
- 0x82b5 `setcandamage(bool)` sets the entity's takedamage byte; 0x82b6 `setcanradiusdamage(bool)` sets/clears the no-radius-damage flag (0x10 in the damage flags).
- 0x8018/0x8019 `attach(model, tag)`, 0x8026/0x8027 `detach(model, tag)` (two ids each, the second form without the tag check), 0x8028 `moveshieldmodel`, 0x833d `setscriptmoverkillcam(type)`, 0x8402 `getlinkedchildren()`.
- 0x8045/0x8048 `hidepart(tag, [model])` / `showpart(...)`: look the tag up in the model, set or clear its bit in the entity's part mask, tell the clients.
- 0x82a7 `movegravity(velocity, time)`: trajectory type 5 (gravity) from the current origin for time*1000 ms. 0x82b7 `physicslaunchserver(pos, vel)`: physics launch (not simulated).
- Engine damage (G_Damage on a non-client script entity): takedamage byte, damage >= 1, `health -= damage`, notify `damage` (amount, attacker, dir, point, MOD, model, tag, part, flags, weapon), `death`(attacker) when health < 1 (clamped at -999). Trigger damage gets the `damage` notify only.
- Read but not named: 0x80a6 (an event with an index looked up from a string, once per entity), 0x8176 / 0x81ae (set one flag on a script_model/vehicle class entity), 0x80c2.
- The census of every id the mp_frag scripts call, with the host's status: `IW6_SCRIPT_CENSUS.md`. Most UNIMPLEMENTED rows are player-only (rank data, dvars, weapons, perks, hud) and AI animscripts; none was hit when mp_frag ran with the door, gate, ladder, toy and sprinkler events.
