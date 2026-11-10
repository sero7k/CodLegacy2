# BO3 map completeness

The owner's report (2026-10-27): "some of the BO3 maps are still missing
sections". This page is the repeatable test for that and what it found.

## The test

`cargo run -p render_t7 --example t7_map_complete [map.ff ...]`

With no arguments it finds every BO3 map zone staged under
`/mnt/user-data/uploads` (`T7_MAPS_ROOT`): an `mp_*.ff` / `zm_*.ff` beside
BO3's shared zones, or in a Workshop item folder. `T7_GAME_ZONE=<dir>` names
BO3's `zone` folder for Workshop maps. Per map it prints:

- **The tally** (`render_t7::completeness::tally`): every world surface and
  every static model surface per placement, drawn by the G-buffer (slot 3
  technique), by the forward lists (slot 6) or not at all. The undrawn are
  grouped by reason (the material's technique error, a constant buffer the
  material does not hold, shadow-only, no material, mesh not built) and by
  technique set family, the materials named. The same count is logged when a
  map loads, one line per map: `t7 native: scene: completeness <map>: ...`.
- **The hole test** (`render_t7::completeness::find_holes`): rays from up to
  `T7_HOLE_EYES` (32) spawn points in `T7_HOLE_DIRS` (384) directions (not
  steeper than 60 degrees up), out to 4000 units, against the solid collision
  (brushes with contents 1, the clip map's triangles, the collision triangles
  of the static models it lists) and against the scene's triangles. A ray
  whose first solid collision hit has no drawn surface at or within 24 units
  past it is a hole. It is put down to the undrawn draw it meets there (its
  reason), else to the collision: a model the scene does not have (its mesh
  not built), collision outside a drawn model's mesh, a solid brush or clip
  triangles with nothing on them. Nodraw brushes (side surface flag 0x80) are
  invisible by design and counted apart. A visible surface with no collision
  is not looked at (the collision work).
- `T7_MODEL_CHECK=<model>`: a model's drawn placements next to its collision
  placements.

Offline limits: of the staged maps only Sector and Skyjacked have their
`.xpak` packages, and none has the base packages, so many static model meshes
are not built here (`static model placement whose mesh could not be built`);
on the owner's PC they are (his logs: every model built on Nuk3town). The
hole test's "model not in the scene" holes are mostly that.

## The owner's runs (his logs, 2026-10-26)

From `t7 surfaces: <map>: drawn N of M draws` in his nine logs (the last run
of each map; draws, not surfaces: one draw per model per surface, all its
placements). Sorted by what is missing.

| zone | draws | drawn | skipped | % | why skipped |
|---|---|---|---|---|---|
| mp_crucible (Gauntlet) | 13489 | 3209 | 10280 | 76.2% | no G-buffer or forward technique x10271 |
| mp_rome | 9669 | 2926 | 6743 | 69.7% | constant buffer not in the material x6743 |
| zm_factory | 13436 | 4862 | 8574 | 63.8% | no G-buffer or forward technique x8572 |
| mp_shoothouse18 (Workshop) | 3064 | 1127 | 1937 | 63.2% | no technique x1370; constant buffer x555 |
| mp_metro (Metro) | 8472 | 3345 | 5127 | 60.5% | no G-buffer or forward technique x5125 |
| zm_genesis | 15230 | 6026 | 9204 | 60.4% | constant buffer not in the material x9202 |
| mp_warhawk (Workshop) | 4512 | 2146 | 2366 | 52.4% | constant buffer not in the material x2366 |
| zm_stalingrad | 20818 | 10514 | 10304 | 49.5% | constant buffer not in the material x10290 |
| mp_atomic (Workshop) | 1678 | 938 | 740 | 44.1% | constant buffer x437; no technique x300 |
| mp_veiled_heyday | 6818 | 4152 | 2666 | 39.1% | constant buffer not in the material x2653 |
| mp_redwood_ice | 2484 | 1559 | 925 | 37.2% | constant buffer not in the material x895 |
| mp_shrine | 2640 | 1663 | 977 | 37.0% | constant buffer not in the material x973 |
| mp_western | 5389 | 3469 | 1920 | 35.6% | constant buffer not in the material x1919 |
| mp_slums (Workshop) | 10739 | 7292 | 3447 | 32.1% | no G-buffer or forward technique x3444 |
| mp_mountain (Workshop) | 11616 | 8781 | 2835 | 24.4% | no technique x2625; pipeline failed x193 |
| mp_firingrange (Workshop) | 6840 | 5600 | 1240 | 18.1% | no G-buffer or forward technique x1240 |
| mp_biodome (Aquarium) | 11378 | 9719 | 1659 | 14.6% | pipeline failed x1076; texture not named in the zone x380; no technique x198 |
| mp_jungle (Workshop) | 4370 | 3795 | 575 | 13.2% | no G-buffer or forward technique x575 |
| mp_waterpark (Splash) | 10951 | 10480 | 471 | 4.3% | texture not named in the zone x403 |
| mp_redwood (Redwood) | 2542 | 2455 | 87 | 3.4% | texture not named x52; no technique x35 |
| mp_skyjacked (Skyjacked) | 8108 | 7959 | 149 | 1.8% | no technique x149 (shared-zone references) |
| mp_veiled (Fringe) | 7040 | 6964 | 76 | 1.1% | no technique x76 |
| mp_infection (Infection) | 3356 | 3325 | 31 | 0.9% | no technique x30 |
| mp_ethiopia (Hunted) | 3096 | 3074 | 22 | 0.7% | no technique x21 |
| mp_spire | 12210 | 12170 | 40 | 0.3% | no technique x21; code resource x19 |
| zm_island | 16308 | 16282 | 26 | 0.2% | no technique x26 |
| mp_stronghold (Stronghold) | 5321 | 5319 | 2 | 0.0% | code resource not made yet x2 |
| mp_conduit | 9693 | 9691 | 2 | 0.0% | code resource not made yet x2 |
| mp_rise (Rise) | 7287 | 7286 | 1 | 0.0% | code resource x1 |
| zm_castle | 14857 | 14856 | 1 | 0.0% | code resource x1 |
| zm_zod | 18270 | 18269 | 1 | 0.0% | no technique x1 |
| mp_nuketown_x (Nuk3town) | 2640 | 2640 | 0 | 0.0% | none |

Not run in his logs (or not BO3): Combine (mp_sector) and Evac
(mp_apartments) are staged here and checked below; Exodus, Havoc
and the others named in the report did not appear as `t7 surfaces:` lines.
Stand-ins (`t7 stand-in:`) were all `flickerLookupMap <- i_generic_lookup`
on emissive scroll materials: a picture, not a missing section.

### By reason, all his maps

| reason | draws | maps | cause | status |
|---|---|---|---|---|
| no G-buffer or forward technique | ~33,900 | Gauntlet, The Giant, Metro, Slums, Shoothouse, Mountain, Firingrange, Jungle, Atomic... | the technique's vertex declaration (`no vertex declaration`) or state map (`no state map`) is a reference to data loaded earlier in the zone that the zone walk cannot follow (its target is in no segment the walk read); `wc/lit#882572b8`, `wc/lit_plus`, the decals | **fixed** (below) |
| constant buffer not in the material | ~36,000 | Rome, Stalingrad, Genesis, Heyday, Warhawk, Western, Shrine, Redwood (snow), Shoothouse, Atomic | the material's `$Globals` (material slot 22: technique 3) is a reference to an earlier material's buffer the walk cannot follow | **fallback added, not verified** (no staged map has it) |
| pipeline failed | ~1,290 | Aquarium 1076, Mountain 193, Splash 16 | translated programs that do not validate | open (translator) |
| texture not named in the zone | ~870 | Splash 403, Aquarium 380, Redwood 52 | open |
| code resource not made yet | ~60 | most | code textures/buffers not made | open (small) |
| shared-zone reference, no technique set | 149 | Skyjacked | `mc/t7_metal_bare_chrome_polished`, `mc/global_invisible`, weapons, robots: material records with an empty technique set name in every zone read | open |

## The fixes

Biggest count first. The two walk fixes (1, 2) are the cause; 3 and 4 are
fallbacks kept for zones the walk still misreads.

1. **Resident siege animations' per-part vec4s are 16-aligned**
   (`walk/world_misc.rs`, `sanim_resident`: the sixth array, mask 15, was
   4). The mod tools keep a Workshop map's fxanim animations in the zone;
   each one's last array starts on 16, which the walk counted as 4, so its
   VIRTUAL count fell behind by up to 12 bytes per animation (Summit: seven,
   0x38 in all by `vd/lit_detail_decal_reveal_plus`). Every reference to data
   read after them then missed: the vertex declarations and state maps of
   the decal and wall techniques that follow (`no vertex declaration`, `no
   state map`). Found with `T7_DRIFT=1 T7_DRIFT_EACH=1 t7_walk` (the shift
   that lands the misses: 0x30-0x38 from the sanims on) and `T7_SITE`
   (alignment by call site). Summit: unresolved references 22068 -> 20875.
2. **References to data a later asset loads, looked up after the walk**
   (`walk/material.rs`, `Capture::settle_technique_refs`). Workshop zones
   name shaders, vertex declarations, state maps and material constant
   buffers that are loaded later in the zone (retail zones never do: 0 on
   Metro, Aquarium, Nuk3town, Combine, Evac, Skyjacked). The walk kept such a
   reference as missing; it now keeps its VIRTUAL offset
   (`DrawMethodInfo::pending`, `TechniqueInfo::pending_state`,
   `TechsetRec::pending`, `MaterialRec::pending_cbuffers`) and resolves it
   once the walk is done. Summit: 1835 shaders, 1065 declarations, 194 state
   maps; 5828 references settled in all (logged at load: `zone references
   to shaders, vertex declarations, state maps and constant buffers read
   later, looked up after the walk: N`).
3. **Technique completed from a donor** (`scene.rs`,
   `Zones::technique_elsewhere` / `technique_donor`): when a technique's
   vertex declaration or state map is still an unresolved reference, the
   same technique set whole in another zone, else the missing part from a
   technique read with the same programs (or the same set kind in the same
   slot). With 1 and 2 it completes nothing on the staged maps; before them
   it drew Summit's decals. `CODLEGACY_T7_NO_DONOR=1` turns it off.
4. **Constant buffers made up** (`scene.rs`, `fill_missing_cbuffers`): a
   material constant buffer still missing is made from the program's own
   layout (RDEF), each variable from the material's numbers by name, else
   the buffer of a material of the same technique set. Not seen working:
   no staged map lacks one. `CODLEGACY_T7_NO_CBUF_FILL=1` turns it off.
5. The completeness line at load, the tally (now also counting a draw whose
   material lacks a constant buffer its programs read, `missing_cbuffer`),
   the hole test with its kinds (nodraw brushes and clip triangles counted
   apart as invisible by design), `T7_MODEL_CHECK`, and `T7_GPU=1|world|all`
   (the G-buffer pipelines and bindings made on a headless adapter: shaders
   that do not translate or validate).

## Before and after (offline, every staged BO3 map)

Staged: Nuk3town, Evac, Combine, Skyjacked (retail), Metro, Aquarium (retail,
staged 2026-10-27), Summit (Workshop `mp_mountain`). Warhawk is staged only
as its `en_` zone, the other Workshop items not at all. `T7_HOLE_EYES=16`,
`T7_GAME_ZONE` a folder with every staged shared zone. Before: the walk at
`fa09ef1` with `CODLEGACY_T7_NO_DONOR=1 CODLEGACY_T7_NO_CBUF_FILL=1`; after: fixes 1-4.
Surfaces, not draws.

| map | surfaces | not drawn before | not drawn after | world not drawn before / after | holes (rays) before / after |
|---|---|---|---|---|---|
| mp_mountain (Summit, Workshop) | 21254 | 6572 (30.9%) | 3814 (17.9%) | 2389 / 0 | 841 / 747 of 4415 |
| mp_nuketown_x | 6891 | 719 | 719 | 0 / 0 | 1578 / 1578 |
| mp_apartments (Evac) | 42259 | 3147 | 3147 | 0 / 0 | 120 / 120 |
| mp_sector (Combine) | 14614 | 1847 | 1847 | 0 / 0 | 510 / 510 |
| mp_skyjacked | 15009 | 2545 | 2545 | 0 / 0 | 808 / 808 |
| mp_metro | 29190 | 5032 | 5032 | 0 / 0 | 664 / 664 |
| mp_biodome (Aquarium) | 38737 | 7874 | 7874 | 0 / 0 | 1115 / 1115 |

Summit after: 0 world surfaces and 6 model surfaces (shadow-only) undrawn
other than meshes not built; the donor's left-overs before (23 model
surfaces of `mc/lit_backlit_plus` and `mc/lit_decal_micro_tile_mix_reveal_plus`,
the fxanim chain-link flag, barbed wire) are drawn now.

### Classification of what is still not drawn (offline)

| cause | surfaces | maps | note |
|---|---|---|---|
| static model mesh not built | 715..7874 per map | all | the maps' `.xpak` packages are not staged (built on the PC: his logs list 1-12 models per map) |
| shadow-only (shadowcaster / onlycast) | 2..9 | all | by design |
| missing material / technique set | 0 | - | (`mc/lit_emissive` is in `mp_patch`: stage it beside the map) |
| technique: no vertex declaration / state map | 0 (was 2389 world + 363 model on Summit) | - | fixes 1, 2 |
| constant buffer not in the material | 0 | - | none staged has it (PC: Rome, Stalingrad, Genesis, ...) |
| DXBC -> WGSL / pipeline failed | 0 | Aquarium (`T7_GPU=all`: 93 pipelines, none failed); Summit's `T7_GPU=world` run was killed for memory here, not checked | the PC's 1076 on Aquarium are not seen here |
| culled by flags / bounds / LOD, filters | 0 | - | only `CODLEGACY_T7_DEBUG_SURF` filters, off |
| brush models no drawn entity names | (water work) | Metro | `wave_water` / `wave_clamp` boxes, see the water notes |

Holes left are almost all `model not in the scene` (meshes not built here)
and `collision outside its drawn model's mesh` (Summit's
`p7_mou_granite_cliff_*_opti` rocks; Nuk3town's 225).

## Shared constant data (fix 6, 2026-10-27) and the maps staged from his PC

6. **Material constant buffers whose initial data is shared with an earlier
   one** (`stream.rs` `Stream::pmap`, `walk/material.rs`). A zone keeps one
   copy of equal material constants: a buffer's `initData` is then a
   reference into PHYSICAL to an earlier buffer's. The walk had no file map
   for PHYSICAL and dropped such buffers, so the material had no `$Globals`
   and its draws were skipped. This was the PC's `constant buffer not in the
   material` (~36,000 draws: Rome, Stalingrad, Genesis, Heyday, Warhawk,
   Western, Shrine, Ruins, Redwood snow, Shoothouse, Atomic), retail and
   Workshop alike. Found on Warhawk: its walls' eight `$Globals` slots all
   follow in the stream, their `initData` refs to PHYSICAL.

Map zones staged one at a time from his install (the device bridge), the
made-up-buffer and donor fallbacks off, surfaces:

| map | surfaces | not drawn before (walk at 8423383) | not drawn after | world not drawn before / after | what is left |
|---|---|---|---|---|---|
| mp_rome | 44934 | 31292 (24304 constant buffer) | 6988 | 6245 / 0 | model meshes not built (packages) |
| mp_shrine | 27165 | 10189 (6060 constant buffer) | 4129 | 491 / 0 | meshes |
| mp_warhawk (Workshop) | 19064 | 10063 (2093 constant buffer) | 7970 | 813 / 0 | meshes |
| mp_crucible (Gauntlet) | 26244 | 3324 | 3324 | 0 / 0 | meshes (its PC `no technique` was the ComWorld walk fault) |
| zm_factory (The Giant) | 46136 | - | 1358 | - / 0 | meshes, 15 shadow-only |
| mp_veiled_heyday | 29409 | - | 4789 | - / 0 | meshes |
| mp_redwood_ice | 15219 | - | 3963 | - / 0 | meshes |
| mp_western | 39995 | - | 12734 | - / 0 | meshes |
| mp_shoothouse18 (Workshop) | 11328 | - | 3855 | - / 0 | meshes |
| mp_atomic (Workshop) | 6588 | - | 3669 | - / 0 | meshes |
| mp_slums (Workshop) | 31333 | - | 10363 | - / 0 | meshes, 6 brush-model surfaces no entity draws |
| mp_jungle (Workshop) | 37222 | - | 9796 | - / 0 | meshes |

Every map checked now draws every world surface and every model surface
whose mesh is built. "Mesh not built" is the offline limit (the meshes are
in `base.xpak`, 9 GB, not stageable here; his logs show 1-12 such models
per map on the PC). Not staged before the bridge dropped: zm_stalingrad,
zm_genesis, mp_ruins, mp_waterpark, mp_firingrange, and the other retail
maps without PC skips.

## What his PC runs mean now

- **No technique** (~34,000 draws: Gauntlet, The Giant, Metro, Slums,
  Shoothouse, Summit, Firing Range, Jungle, Atomic). Metro's 5012 were the
  ComWorld walk fault fixed in `540c251` (his runs predate it; offline Metro
  draws every world surface). The Workshop maps' are fixes 1 and 2 (Summit
  verified). Gauntlet and The Giant (retail, not staged) need a run.
- **Constant buffer not in the material** (~36,000: Rome, Stalingrad,
  Genesis, Heyday, Warhawk, Western, Shrine, Ruins, Redwood snow). The
  Workshop ones (Warhawk, Shoothouse, Atomic) are likely forward references
  (fix 2 covers material constant buffers); the retail ones are more likely
  a VIRTUAL miscount earlier in those zones, like the sanims. Look on his
  next log for `looked up after the walk` and the `t7 surfaces:` skip
  counts; if retail ones remain, `T7_DRIFT=1 t7_walk <zone>` on that zone
  shows where the count parts.

## Open

- Summit's walk still drifts after its `GfxWorld` (0x110, then 0x120 by the
  end): references into data read after the world (glass technique sets,
  the character images) miss. No draw is lost to it offline.
- Pipeline failures (Aquarium 1076 draws on the PC, Summit 193): not
  reproduced on the headless adapter here; needs his log's `pipeline
  failed` lines.
- Textures not named in the zone (Splash 403, Aquarium 380, Redwood 52) and
  Skyjacked's 149 empty-technique-set references.
- Drawn black (Metro's ground, Aquarium's patches): undrawn world surfaces
  show as the cleared background, so Metro's black ground was its 5012
  undrawn draws; the rest needs his frames with the fixes in.
