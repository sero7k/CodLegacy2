# Call of Duty: Ghosts (IW6, PC x64) zones

Phase 1: open any Ghosts `.ff`, count its assets by type, read its sound
aliases, find their streams in `soundfile*.pak`, and play Ghosts' MP menu
music in our front end. Code: `crates/fastfile_iw6` (format),
`crates/audio/src/music/ghosts.rs` (music director hook), `CodLegacy check-ghosts`
(probe). Everything below was checked on the retail files
(`code_post_gfx_mp`, `patch_code_post_gfx_mp`, `code_pre_gfx_mp`, `common_mp`,
`patch_common_mp`, `common_core_mp`, `ui_mp`, `mp_character_room`,
`mp_prisonbreak`, `soundfile1.pak`) unless marked *unverified*.

Reference used for layouts: Joelrau's x64-zt (`src/zonetool/zonetool/iw6/`
`structs.hpp`, `zone.cpp`, `assets/*.cpp`: the IW6 linker/dumper, whose write
order and `align()` calls mirror the game's loader). Struct sizes were
measured by compiling that header with `clang++ --target=x86_64-pc-windows-msvc`
(MSVC x64 layout; `-S -emit-llvm` of a `sizeof` table), then every field the
code reads was confirmed against real bytes.

## Container (`container.rs`)

```
0x00 "IWff0100" (retail, signed) | "IWffu100" (unsigned, tools)
0x08 u32 version = 0x235
0x0c u8 compress=1, u8 compressType=0 (zlib), u8 sizeOfPointer=8, u8 sizeOfLong=4
0x10 u32 fileTimeHigh, u32 fileTimeLow
0x18 u32 imageCount
0x1c imageCount x XStreamFile (24 bytes): +2 u16 imagefile index, +4 u32 (1),
     +8 u64 offset, +16 u64 offsetEnd   -> bytes of zone/imagefile<N>.pak
     u64 baseFileLen (= .ff size), u64 totalFileLen (+ streamed images)
     signed: "IWffs100" auth header (0x2000) + first hash chunk (0x2000), then
             0x2000-byte data chunks with one 0x2000 hash chunk after every 256
     unsigned: the zlib stream directly
```
The data chunks join into ONE zlib stream (`78 DA`) of the whole zone image.
`common_mp`: 1616 streamed images (imagefile59.pak ...), 271,527,715 bytes
inflated; `mp_prisonbreak`: 7592 images; `code_post_gfx_mp`: none.

## Zone image (`xfile.rs`)

```
0x00 XFile { u64 size; u64 externalSize; u64 blockSize[7]; }          72
     blocks: 0 TEMP, 1 PHYSICAL, 2 RUNTIME, 3 VIRTUAL, 4 LARGE, 5 CALLBACK, 6 SCRIPT
0x48 XAssetList { ScriptStringList { i32 count; char** strings; };
                  i32 assetCount; XAsset* assets; XGlobals* globals; }   40
VIRTUAL: count x char*; the strings (string 0 is null)
VIRTUAL: XGlobals { XGfxGlobals* } (8), XGfxGlobals (104):
         u32 depthStencilStateCount, blendStateCount, ...;
         u64 depthStencilStateBits[] (align 4), u32 blendStateBits[] (align 4),
         u32 perPrim/perObj/stable constant-buffer sizes[] (align 4 each);
         the state/buffer tables are RUNTIME (not in the file)
VIRTUAL, align 4: assetCount x XAsset { u32 type; pad; void* header; }   16
asset data, in table order
```
The file has no padding: the loader reads the stream in order and aligns
its *block* positions. To know a VIRTUAL offset (what references point to)
the reader must count every VIRTUAL allocation with the loader's alignment
(`cursor.rs`). Asset headers, inline sub-asset headers and loaded-sound PCM
are TEMP; RUNTIME takes no file bytes.

### Pointers (`ptr.rs`)
8 bytes; top 28 bits `0xFDFDFDF`, bits 32..35 a block, low 32 a value.
* `FDFDFDFF_FFFFFFFF` follow (data inline, here);
* `FDFDFDFF_FFFFFFFE` insert: data inline **and** an 8-byte VIRTUAL slot,
  aligned 8, is taken (seen on `LoadedSound.data`);
* `FDFDFDF3_xxxxxxxx` reference to VIRTUAL offset `xxxxxxxx - 1` (strings,
  shared `SoundFile`s, whole assets via `XAsset[i].header` = table + 16*i + 8);
* a raw x64 pointer into `iw6mp64_ship.exe` (e.g. `0x1411b7cb0`) appears for
  built-in defaults (a retail `OcclusionShape`'s name): no string in the zone.

### Alignments confirmed
string table and strings unaligned; XGlobals/XGfxGlobals unaligned; globals
arrays and the asset table 4; NetConstStrings string table 8 (none when the
list is null/empty); `snd_alias_t[]`, `SoundFile`, `SpeakerMap`,
`OcclusionShape` 4; insert slots 8. With these, 4197 + 235 sound lists in
`common_mp` / `mp_prisonbreak` read with a carried VIRTUAL count land
exactly on their own name anchors (0 conflicts).

## Asset types (`asset_type.rs`)
0x00 physpreset .. 0x3E particlesimanimation (63 types, x64-zt order); 0x0E
sound, 0x12 loaded_sound, 0x22 attachment, 0x23 weapon, 0x34 netconststrings.
Top-level x64 header sizes are in `AssetType::header_size()` (e.g. material
936, techset 3312, xmodel 600, weapon 296, attachment 280, gfxworld 2608,
clipmap 640).

Counts seen:

| zone | assets | notable |
|---|---|---|
| code_post_gfx_mp | 1195 | 452 techset, 415 material, 108 rawfile, 59 sound |
| common_mp | 8777 | 6296 sound, 1418 xanim, 558 fx, 187 loaded_sound, 1 weapon |
| common_core_mp | 3034 | **132 weapon, 63 attachment**, 872 material, 739 sound, 235 xanim, 90 xmodel, 11 vehicle |
| mp_prisonbreak | 1741 | clipmap, comworld, glassworld, map_ents, fxworld, gfxworld, 879 material, 501 techset, 244 sound |
| ui_mp | 2030 | 1897 material, 80 techset, 46 weapon, 3 menulist, 0 sound |

## Sounds (`sound.rs`, `scan.rs`, `pak.rs`)

```
snd_alias_list_t { char* name; snd_alias_t* head; i32 count; }      24 (TEMP)
snd_alias_t 200: 0 name, 8 subtitle, 16 secondary, 24 chain, 32 mixerGroup,
  40 SoundFile*, 48 sequence, 52 volMin, 56 volMax, 60 volModIndex,
  64 pitchMin, 68 pitchMax, 72 distMin, 76 distMax, 80 velocityMin, 84 flags,
  88 masterPriority(u8), 92 masterPct, 96 slavePct, 100 probability,
  104 lfePct, 108 centerPct, 112 startDelay, 120/128/136 SndCurve*
  (volumeFalloff, lpf, reverbSend), 144..152 envelope, 160 SpeakerMap*,
  168 wetMixOverride, 176 OcclusionShape*, 184 allowDoppler, 192 DopplerPreset*
SoundFile 56 { u8 type (1 loaded, 2 streamed, 3 primed); u8 exists; pad; union @8 }
StreamFileName 24 { u16 isLocalized; u16 fileIndex; pad;
                    fileIndex ? {u64 offset; u64 length} : {char* dir; char* name} }
StreamedSound { StreamFileName; u32 totalMsec }   PrimedSound { StreamFileName;
  LoadedSound* loadedPart; i32 dataOffset; i32 totalSize; u32 primedCrc }
LoadedSound 56 (TEMP) { name; data; seekTable; RiffAudioInfo { rate, bytes,
  samples, avgBytesPerSec, u16 channels, bits, blockAlign, u8 format };
  i32 loadedSize; i32 seekTableSize }   name VIRTUAL, data TEMP, seek VIRTUAL
SndCurve 152, DopplerPreset 32 (TEMP + name); SpeakerMap 416, OcclusionShape 32 (VIRTUAL + name)
```
Per alias the stream order is: strings, sound file (+ loaded sound), the
three curves, speaker map, occlusion shape, doppler preset.

**Names are shared.** A list's `name` is usually a reference into the
zone's `ncs_snd_<zone>` `NetConstStrings` (read exactly at the zone's head);
every alias in a list repeats the list's name pointer. `SoundFile`s are
shared too (`music_challenge_factory` and `music_opening` point at the
silent `null` loaded sound of `AB_1mc_boost`).

**Finding them without reading every asset first**: `scan.rs` looks for the
list shape after the asset table, reads each candidate strictly, and places
runs of adjacent lists in VIRTUAL from inline-named lists (anchors) or, with
no anchor, from the zone's shared sound-file references (calibration, >= 2
agreeing votes). Results: every sound list found in every zone above
(59/59, 6296/6296, 739/739, 244/244, 8/8, 1/1), 0 conflicts; unresolved:
1 list name in common_mp (a reference outside NCS and anchored runs), 9
shared sound files in common_core_mp.

**Paks**: `fileIndex N` -> `zone/soundfile<N>.pak` (verified for N = 1; the
offsets of N = 4, 7, 13, 16, 22, 59 in common_mp fit their paks). Paks are
FLAC streams (`fLaC` + STREAMINFO), 0x800-aligned, no table. Localized ->
`zone/<language>/<lang>_soundfile<N>.pak` (*unverified*: no localized
stream in the MP zones read).

### Menu music (`music.rs`)

| alias | zone | soundfile1.pak offset | length | duration | format |
|---|---|---|---|---|---|
| music_mainmenu_mp | patch_code_post_gfx_mp (same stream as base) | 0x0bbad000 | 0x97e0dc | 131.095 s | FLAC 44.1k 2ch 16 |
| music_mainmenu | code_post_gfx_mp | 0x0b149800 | 0xa63044 | 142.845 s | 44.1k |
| music_mainmenu_extinct | code_post_gfx_mp | 0x0a541000 | 0xc08127 | 118.950 s | 48k |
| music_mainmenu_squad | code_post_gfx_mp | 0x0ce2a000 | 0xa21e7f | 121.647 s | 48k |
| music_mainmenu_rorke_files | code_post_gfx_mp | 0x0c52b800 | 0x8fe588 | 116.450 s | 48k |
| music_credits_entire_scroll | code_post_gfx_mp | 0x079ea000 | 0x2b56802 | 407.420 s | 44.1k |

STREAMINFO totals match `totalMsec` within a millisecond; symphonia decodes
all six to the end.

## In the game

* The install is found at `CODLEGACY_GHOSTS_DIR`, a `ghosts=` line in
  game_paths.cfg, `<CODLEGACY_GAMES>/ghosts` (build-and-run.ps1 links
  `games\ghosts` -> `Documents\ghosts_game_files`, and `games\aw` ->
  `aw_game_files`), or a folder under the games root named for Ghosts.
* With an install, `ghosts:music_mainmenu_mp` joins the default menu (and
  lobby) playlist; the others are listed in `audio_manifest.csv` and can be
  added in music.cfg (`menu = ghosts:music_mainmenu_squad`, `iw6:` works too).
  Tracks stream from the pak as FLAC through `StreamedMusic`.
* `AssetNamespace::Iw6` (phase 2) carries Ghosts' guns, animations,
  materials and sounds; Ghosts zones are still no `ZoneGame` (zone
  discovery keeps ignoring version 0x235).
* Zone discovery (`asset_transport::discover::files_under`) now drops plain
  non-`.ff` files before stat/canonicalize, so the paks are never touched;
  Ghosts' version 0x235 zones are ignored by every existing lane.
* `CodLegacy check-ghosts [zone]` prints a zone's counts and sound scan and checks
  every menu track (fLaC, STREAMINFO vs alias, full decode probe); report in
  `<artifacts>/ghosts-check.txt`. Release-speed: common_mp inflates and scans
  in ~2 s (debug ~12 s).

## Phase 2: the exact walk (`walk/`, `stream.rs`, `layout.rs`)

`walk::Walker` reads every asset in table order as the loader does: one
stream, a block stack (`stream.rs`: TEMP/VIRTUAL/RUNTIME... positions, each
`align(mask)`ed), sub-asset headers in TEMP and their contents in VIRTUAL,
"insert" pointers taking an 8-byte VIRTUAL slot. Struct offsets come from
`layout.rs`, generated from x64-zt's `structs.hpp` with an MSVC x64 record
layout (`tools/README.md`). References resolve to what was read: asset slots
(the table's and every inline-loaded pointer field), strings, shared sound
files and loaded sounds.

Checks (`cargo test -p fastfile_iw6 --release <name> -- --ignored
--nocapture` with `IW6_WALK=<zone.ff>`): `walk_retail` (stream end = image
end, final VIRTUAL offset = the XFile's VIRTUAL block size, unresolved
references, sound lists against the phase-1 scan, inline-named sound list
anchors), `surface_check`, `gun_*`, `image_dump`, `type_runs`.

| zone | walked | VIRTUAL at end = block size | notes |
|---|---|---|---|
| common_core_mp | 3034/3034 | yes (0xaaacf50) | 0 unresolved; 739 sound lists at the scan's offsets |
| common_mp | 8777/8777 | yes (0x22c505a) | 0 unresolved; 6296 lists; 972 name anchors ok, 0 off |
| patch_common_core_mp, patch_common_mp, ui_mp, code_pre_gfx_mp, mp_prisonbreak_load, patch_mp_prisonbreak | all | yes | |
| mp_prisonbreak | 1741/1741 | yes (0x7bf5160) | 0 unresolved; FxWorld, GfxWorld, MapEnts, GlassWorld, ClipMap read |
| the 15 base MP maps (below) | all | yes | 0 unresolved; `iw6_walk_check` |
| techsets_mp_frag, techsets_common_core_mp, techsets_common, techsets_code_post_gfx_mp | all | yes | 0 unresolved (2026-11-03: hull and domain programs are in VIRTUAL, not TEMP); techniques, passes, arguments and programs kept; shader maths in `exe_notes/IW6_SHADERS.md` |
| code_post_gfx_mp | 252/1195 | no | stops at a type without a layout (its technique sets are complete in techsets_code_post_gfx_mp) |

Layouts settled on retail data (beyond x64-zt):
* **Menus** (`walk/menu.rs`): x64-zt's order and `align(3)` hold, except an
  item's `enumDvarName` (type 13): the retail loader does not align it (one
  item in `ui_mp/ingame.txt` drifted common_mp by 4).
* **ImpactFx** (no x64-zt type): { name; table } with 15 x 38 effect
  pointers. **SurfaceFx**: { name; table } with 6 x 31.
* **EquipmentSndTable** (`soundaliases/equipment_snd.def`, no x64-zt
  type): an 88-byte header { name; 3 counts; 8 pointers }, its name, a
  list of 4 names (`vestlight`...), 4 { name, float } (`small` 5.0...),
  then float tables (pointer arrays and `float[6][3]` blocks, all whole
  multiples of 8). Read exactly up to the tables, which are taken as one
  run ending where the following raw files parse; the zone still ends on
  its exact VIRTUAL size.
* **Font**: x64-zt's.
* Weapon enums: `weapType` is IW4's + 1 (0 none; every gun reads 1, the
  Panzerfaust 3), `weapClass` and `fireType` are IW4's numbers.
  Animation slots: 0x10 reload, 0x11 reload empty, 0x18 raise, 0x1b drop,
  0x26/0x28/0x29 sprint, 0x30/0x31 ADS fire, 0x5a/0x5b ADS up/down.

### Models, pictures, animations

* `GfxPackedVertex` (32): position, binormal sign, colour, half2 UV (as
  BO2's), normal and tangent unsigned 10:10:10 over 1023 (BO2: signed over
  511). Blend vertices as IW4 in eight buckets (1..8 influences), bones as
  64-byte offsets, weights /65536. All 146 gun models: 1.26 M vertices
  inside their bounds, unit normals, indices in range, every bone valid.
* Streamed images: the four `XStreamFile`s of a streamed image are its
  parts in `imagefile<N>.pak` (one zlib stream each); part 0 has the
  smallest streamed size with all smaller levels, each later part one level
  more (`GfxImageStreamData` width/height/cumulative bytes). Decoded and
  checked by eye (`image_dump`). Gun pictures span 18 paks
  (imagefile1..59, ~520 MB of parts).
* 2-D pictures (`ui_image.rs`): Create-a-Class `weapon_<gun>_360`
  (512x256 BC3), `hud_icon_<gun>` kill icons, attachment pictures
  (`scope_acog_360`, `muzzle_break_360` ...), inline in ui_mp.
* Animations: IW-family part streams; all 828 gun animations decode with
  the engine's decoder (which refuses a stream it does not use up).

### Sounds
Gun sounds are in **common_mp** (not common_core_mp): loaded 16-bit PCM
(`ERiffAudioFormat` 1, 44.1/48 kHz). Shared loaded sounds and shared sound
files resolve through the walk; streamed aliases name `soundfile<N>.pak`
ranges (FLAC).

## In the game (phase 2: Ghosts guns as guest guns)

* `assets::lane::iw6` walks common_core_mp in every BO2-rules match with
  guest guns (`BO2MP_GUEST_GUNS=0` turns all guest guns off): models into
  BO2's captured form and its skinned-model builder (unit vectors repacked,
  blend buckets folded to four influences), animations under
  `AssetNamespace::Iw6`, materials on BO2's draw path by technique-set name
  (Ghosts' `m_`/`mc_`/`ms_` model techniques; colour, normal, specular maps
  from the zone or the paks; BC5 normals halved into BO2's
  `xy*4.0157-2.0157` convention), weapons through
  `WeaponCatalog::capture_iw6`. Arms are BO2's.
* Names: `gh_<gun>_mp` (`AssetNamespace::Iw6`, guest prefix `gh_`); a
  variant `gh_ak12_mp+reflex` is a row of its own (`lane::iw6::
  guest_attachments`, `asset_game::GuestAttachment`): the attachment's view
  and world models on the gun's bone named as their root (`tag_red_dot`,
  `tag_silencer`), the aiming animations the gun's overrides give it, its
  suppressed fire sounds, Black Ops II's numbers and sight (reticle, zoom,
  a scope's overlay). An override's attachment is read as MW3's condition
  (scope number, under-barrel times 8, an other's bit from 5), else by its
  place in the gun's lists; neither is checked on Ghosts' data (not on
  this machine). Snipers' own scope (`<gun>scope`) goes on the base gun,
  with Black Ops II's sniper scope aimed.
* Create-a-Class: `bo2_profile::guest::GHOSTS` (39 guns, statstable rows
  from 800, attachments as Ghosts names them, new attachment rows for
  tracker, ironsight, barrelrange, barrelbored, flashsuppress, firetype*,
  ammoslug); pictures `iw6:weapon_<gun>_360|hud_icon_<gun>` read by
  `fastfile_iw6::ui_image` (front end and kill feed).
* Sounds: `asset_audio::sound_load_iw6` adds Ghosts' common_mp and
  common_core_mp weapon aliases (`weap_`, `wpn_`, `melee_`, ... and what they
  chain to) to the match bank under the Ghosts namespace
  (`CapturedSound::namespace`); loaded PCM plays as is, streamed FLAC is read
  from its pak range when played.
* `CodLegacy check-guns` (`BO2MP_CHECK_GUNS=iw6`): 39 of 39 Ghosts guns OK; fire
  sound playable for 49 of 52 Ghosts weapons (not: the Kastet's
  `weap_rgm_fire_plr`, the two riot shields), notetrack (reload) sounds 623
  of 637; 828 animations decode.

## Maps (phase 3: Ghosts' multiplayer maps)

All 15 base maps walk exactly (stream end = zone end, VIRTUAL end = block
size, 0 unresolved references): mp_chasm, mp_dart, mp_descent_new,
mp_fahrenheit, mp_flooded, mp_frag, mp_hashima, mp_lonestar, mp_prisonbreak,
mp_skeleton, mp_snow, mp_sovereign, mp_strikezone, mp_warhawk, mp_zebra
(`cargo run -p fastfile_iw6 --example iw6_walk_check -- <zone.ff>`). What
the other maps needed beyond Prison Break:

* `FxGlassDef`'s sub-assets load physics preset, material, shattered
  material (in their own fields, x64-zt's layout); Prison Break walked in
  field order only because all three load there.
* `ScriptableDef` (inline in `clipMap_t::scriptableMapEnts`): parts, states,
  on-enter events by kind (0 model asset, 1 play-effect asset, 2 stop-effect
  name slot as a sound alias's, 3 sound alias name, 4 animation name, 6
  physics model, 7 light settings' `noteworthy`, 9 shake rumble name; the
  others hold numbers), notetracks (0 effect asset, 1 sound name). An
  instance reads its def's counts from the def's header, kept by index
  (`Capture::scriptable_headers`) when an earlier instance loaded it.
* `ImpactFx` / `SurfaceFx` tables align to 4 (mp_snow's and mp_skeleton's
  own impact tables, named "", sit 4 past an 8-byte boundary).

`fastfile_iw6::map::read_map` turns the walk into plain data
(`examples/iw6_map_check.rs` checks it): the world (CoD4's vertex layout,
unit vectors unsigned 10:10:10), surfaces with bounds and the lit / decal /
emissive ranges, static models with their ground light, brush models;
collision (brushes with their side planes, terrain triangles, partitions
with their vertex segment — triangles index vertices from `segment * 1024`,
AABB forest, brush models, leaf brush nodes); the entity string (keys are
canonical script string numbers, `IW6_KEYS` names those read off retail
values: 97 classname, 430 origin, 47 angles, 642 targetname, 640 target,
392 model, ...); spawn points (`MapEnts::spawnList`, not in the entity
string) written back as entities; trigger models (`"model" "?N"`, as
IW4's/IW5's); the sun (ComWorld's first sun light); lightmaps as CoD4's
(`iw3_lightmap`: BC4 primary = sun visibility at twice the page, secondary
two stacked R16G16B16A16_FLOAT pages, linear light made CoD4's gamma bytes,
alpha direction -1..1 to CoD4's code).

In the game (`assets::lane::iw6_map`): a zone of version 0x235 is a legacy
map (`asset_transport::legacy_game_for_path` says `iw6`; zone key
`iw6:mp_prisonbreak`); the world goes through the IW3 world builder, the
collision through the IW3 clip builder (vertex segments and trigger models
set after), world materials by technique set name (`l_sm_r0c0..`,
`b1c1`/`m1c1` layers with `colorMap1`/`colorMap2`; sky, tools, shadow-only
and distortion surfaces not drawn), placed models through the guest-gun
model builder, colour maps at most 512 px (`BO2MP_GH_MAP_TEXTURE`), the
film vision and createart fog from the zone, spawn records over nothing
left out (Ghosts parks unplayed modes' spawns in an empty corner), and
Black Ops II's rules on top. The map list (`ui::foreign_maps`) lists a
Ghosts `mp_*` zone with a `<map>_load.ff` beside it (not Extinction's
`mp_alien_*`); its picture is ui_mp's `loadscreen_<map>`.

### Maps: textures and look (checked on all 15 maps)

`examples/iw6_material_census.rs` counts each map's world and placed-model
triangles by whether their colour map reads (`IW6_CENSUS_SHARED=1` as the
game now reads them; `_LIST`, `_TECH`, `_LIGHT`, `_Z`, `_BMODELS` list the
details); `examples/iw6_map_render.rs` software-renders a map (world, placed
models, colour maps, layers, lightmaps as the fallback draw lights them) to
PNG from the top or from spawn points, `IW6_RENDER_BEFORE=1` as the game drew
it before these fixes.

* **Vertex colour is R, G, B, A** in memory (a layered surface's layer-1
  weight in green, layer-2 in blue; byte 0 sits near 255). The IW3 builder
  reads D3DCOLOR (B, G, R, A), so the lane swaps red and blue
  (`iw6_map::iw3_colour`); read as it was, every two-layer surface (1-10 %
  of a map) showed its second layer everywhere.
* **Height-blended layers** are written `b1ha1c1` / `b1hb1c1` (bases
  `r0ha0c0`); BO2's reader wants the colour map right after the layer
  number, so they were not drawn at all (base layer only): 54 % of
  Skeleton's world triangles, 24 % of Snow's, 14 % of Chasm's, 12 % of
  Zebra's. `iw6::ghosts_layer_tokens` drops the `h?` part.
* **Placed models' vertex colour is no tint**: foliage keeps wind and
  reactive-motion weights there (ferns and grass drew red); the map lane
  whitens it.
* **Shared images and materials** (`shared.rs`): a map names images (and
  whole materials, `,m/mtl_sz_crate_federation`) another zone holds,
  `,name` with an empty record. They are read from `common_mp`,
  `common_core_mp` and their patches (once per process), the engine's own
  (`$white`, `$black`, `$identitynormalmap`) made. Strikezone's crates were
  20 % of its placed triangles, Zebra's 3 %, Chasm's broken floors 1.5 % of
  its world.
* **Exposure**: Ghosts tone-maps HDR light per map (baked light at the
  90th percentile from 0.07 on Sovereign to 1.7 on Skeleton; suns from 0.2
  to 10). `map::light_exposure` scales the lightmaps by `EXPOSURE_KEY` /
  that percentile before they become CoD4's gamma bytes; the sun takes the
  same exposure, at most `SUN_GAMMA_MAX` in gamma space; placed models'
  ground light too. Taken 1 : 1, sunlit surfaces summed past 2 (burnt
  white) and Sovereign's halls were dim.
* **Decals** (the world's decal range) take one polygon offset step
  (they are coplanar, without depth writes, and flickered).
* **Brush models** placed by their `script_brushmodel` entities are drawn
  where the entity stands (`map::placed_brush_models`): Freight's sliding
  doors, Fahrenheit's elevator, Flooded's running water, Prison Break's
  bomb-site wall. They were collision only (invisible walls). Later states
  (`targetname "exploder"`, `script_noteworthy "show"`) and brush models
  with no entity stay out.
* **Strikezone's second level**: the after-the-strike map is built into the
  same world 35,000 units up (half the world's triangles, 5,000 models and
  its own spawn points). `map::alternate_level_cut` finds it from the spawn
  points; its geometry, models and spawn records are left out.
* Exposure portals (`*_distfalloff_*`, colour map `$white`) are not drawn.

Coverage of drawn world triangles by readable colour maps went from
98.6..100 % to 100 % on every map; placed models from 79.7 % (Strikezone)
/ 97.1 % (Zebra) / 99+ % to 100 % (but a few lens-flare sprites). (The 15
base maps; the DLC maps are measured below, two of them.)

### Maps: DLC maps and props that are not static models (2026-10-27)

The owner's install has 16 DLC maps; two staged zones were measured (Zerosub
as a DLC map, Prison Break as a base map) with `iw6_material_census`
(`IW6_CENSUS_SHARED=1`; `IW6_CENSUS_BASE=1` reads only the four base shared
zones as before, `IW6_CENSUS_ZONES=a,b` an explicit list; pixel reads of
streamed images count as resolved when only the pak is missing).

* **Shared zones**: `fastfile_iw6::shared::SHARED_ZONES` is now patch_common_mp,
  common_mp, patch_common_core_mp, common_core_mp, common_core_dlc_updated_mp,
  common_core_dlc_mp (the lane and the census read the same list). The two DLC
  zones hold 625 and 81 assets (the DLC characters, e.g. `keegan_cornered_*`):
  2,831 -> 3,703 shared images. **No staged map needed them**: with the four
  base zones, 100 % of world triangles (Zerosub 308,205 of 308,259, the rest
  sky/tools) and 100 % of placed-model triangles (7.6 M, 10.0 M) have a
  resolved colour map, before and after. Prison Break's 3.4 % "hidden
  distortion" and 2 shared materials (1,262 tris) and Zerosub's `flare_a` (514
  tris) resolve in no zone. `patch_mp_<map>.ff` holds 1 (Zerosub) or 4 (Prison
  Break) assets and `techsets_mp_<map>.ff` only technique sets: neither
  carries images or models, so neither is read.
* **Placed models are complete**: 7,325 of 7,330 Zerosub instances and all
  8,139 of Prison Break's have LOD 0 geometry (the 5 left are flag cloth).
* **Missing props**: the game drew only `GfxWorld` static models. Three more
  lists show props at the start of a match: `clipMap_t` dynamic entities
  (`DynEntityDef`: pose quaternion + base model; clutter and destructibles),
  `ScriptableMapEnts` instances (origin, angles, `ScriptableDef::baseModel`;
  captured by the walker now, `Clip::scriptables`) and `script_model` entities
  (`origin`, `angles`, `model` by name). `map::extra_props` lists them;
  `iw6_map` places them with the static models (scale 1, never distance-culled,
  no ground light). Zerosub: 163 + 62 + 51; Prison Break: 30 + 0 + 8. All
  their models have geometry in the map zone. Left out as scripts' own:
  `script_gameobjectname` entities, `exploder`, `airdrop_crate`,
  `care_package`, `sd_bomb`, `light_off` twins, `*destroyed*`, `fullbody_*`
  (dogs), `prop_flag*`, `viewmodel*`.
* Not done: `modelscale`-like keys on script_models (unknown key numbers),
  scriptable states (a lit/unlit part, destroyed twin), props whose model a
  DLC map takes from a shared zone (models are not shared the way images are;
  none seen in the two staged maps), and the other 14 DLC maps.

### Maps: props audit (2026-10-27, second pass)

`iw6_prop_audit <zone.ff>...` (`IW6_AUDIT_TOP`, `IW6_AUDIT_GREP=car,truck`,
`IW6_AUDIT_ENTS=1`, `IW6_AUDIT_MODELS=1` lists a zone's models) counts, per
zone, the static models with/without geometry (and above the Strikezone cut),
the dynamic entities (brush models listed), the scriptables, the entities
that name a model, and what `map::extra_props_report` places and leaves out
with the reason. Run on 29 zones (every base map and the DLC maps staged
here); the last full-map numbers (before -> after):

* **Static models**: 100 % have geometry except 8 `mp_flooded_concrete_barrier_1`
  (Flooded), 2 `zerosub_flag_01` + 3 `prop_flag_seal` (flags), 1
  `ch_rubble_chunk01_alt` (Dart), 2 `,prop_flag_neutral` (Dome), 1
  `,prop_flag_iw6_ghost` (Pirate), and Sovereign's `,weapon_uav_control_unit_iw6`
  / `sov_uav_control_unit_iw6` (the first is common_mp's). Strikezone's cut
  drops 4,977 of 13,569 (the second level); no other map has a cut.
  Cull distances: median 2,000, p95 6,000, 0 (never) on 5-20 % of
  instances; the engine's own rule (`smodel_cull_dist_hides`) applies.
* **Cars** in the audited maps were drawn already: static models (Prison
  Break's trucks and bulldozers, Strikezone's FEMA trucks, Flooded's wrecks
  and Frag's crushed cars) or scriptables whose base model is the intact car
  (Flooded's vans and sedans, Zebra's `vehicle_iveco_lynx_iw6`; the states
  only hold the damaged model: `vehicle_iveco_lynx_destroyed_iw6`). What
  was dropped: Lonestar's two `script_model_mover` sedans
  (`ls_vehicle_civilian_sedan_gray_destroy_iw6`, started by a trigger, standing
  at their origin until then). `is_model_class` now takes `script_model`,
  `script_model_mover`, `misc_prefabl` (Favela's wall clock) and
  `midc_model` (Swamp's tree). Lonestar 34 -> 36 script models.
* **Brush-model dynents** (clutter built of brushes, `model` null and a brush
  index; vertices about the entity's origin): Lonestar 19, Frag 16, Dart 63
  (of 754) were never drawn (their surfaces sit outside the draw ranges).
  `placed_brush_models` now places them at their pose (Lonestar: 25 brush
  models placed, 6 + 19).
* **Skip rules** now reported with a reason. The `*destroyed*` rule only
  drops a model that stands (within 1 unit) beside a different model (the
  bomb-site missile and its wreck); a wreck on its own is scenery. Also
  dropped: `targetname *_after` (Pirate's `destruction_tower_after`).
  What is left out on every map is game-mode objects (`mp_missile_bombsite`
  + `_destroyed`, `com_plasticcase_*` crates, `prop_suitcase_bomb`, HQ
  laptops, `sab_bomb`), flags, and scriptables with no base model: lights,
  Sovereign's `sovereign_flat_tv_01` (23), `lonestar_static_glowstick` (7),
  `mp_hashima_sun`, `digital_clock_countdown`.
* **Shared-zone models.** The shared zones hold 746 models with geometry
  (194 `viewmodel_*`, 185 `weapon_*`, 126 `veh_*`, 75 `vfx_*`, characters), no
  map furniture. Only `weapon_uav_control_unit` is placed by a map (Hashima:
  5 dynents; Sovereign: 1 static `weapon_uav_control_unit_iw6`, also common_mp's;
  `sov_uav_control_unit_iw6` is in none staged). `iw6_map::shared_model_meshes` walks the shared
  zones beside the map when a placed model has no geometry and builds the
  models by name (their materials link into the map's catalog); the report
  line `iw6 shared models: N of M ...` says what was found. Models the
  zone names that no shared zone holds stay undrawn.
* Not done: `script_model` scale keys (none of the numbered keys seen is a
  scale: `iw6_21278` = a movement flag, `iw6_27562` = a prefab id,
  `iw6_15695` a light override); scriptable instance `preBrushModel` /
  `postBrushModel` (Sovereign's TVs); `mp_flooded_concrete_barrier_1` (in no
  shared zone staged here, maybe a DLC common zone); movers are placed at
  their start and do not move.

### Maps: what the game draws against what the lane places (2026-10-27, third pass)

The offline counts (placed, resolved) were ~100 %; the loss was after the
lane. `map_check_probe iw6 <zone.ff>` (the game's own `load_iw6_world`) now
prints a `props:` line: placements, those with a packed light, surfaces,
colour maps. The renderer's Black Ops II props pass
(`SmodelGpuPlan::t6_fallback_props`, `render::extract`, `geometry_diagnostic`)
**skips every placement whose `packed_lighting` is `None`** (the materials
have no technique set, so no draw item exists for them). The lane gave a
light only to models with a ground light of their own; the rest are lit by
Ghosts' light grid. Drawn before -> after: Prison Break 1,340 -> 8,177 of
8,177; Strikezone 1,626 -> 9,121 (of 14,098; 4,977 are the second level);
Zerosub 0 -> 7,601 (of 7,606). Models without their own light take the
middle ground light of the lit ones (the Remastered lane's rule). The extra
props (dynents, scriptables, script models) had none, so none was ever
drawn. Also checked and fine: the mesh builder drops 0 surfaces (0 unreadable
models but flags), per-instance cull distance and frustum sphere are the
instance's own, LOD is 0 only with the single LOD built (no distance
loss), the props pass binds a picture per material (the 8,192-slot texture
table is the technique path's), the server pack path draws nothing by design
(dedicated server only). Streamed pictures read from the paks (`iw6_pak_need`,
`IW6_PAK_READ=1`) have whole mip chains and 4x4 block sizes; not measured:
pictures in paks over 400 MB (4, 7, 10, 13, 60 = 69 % of Prison Break's
bytes; the staging tool's limit).

Walk regression fixed: the createfx commit read only two of a decal mark's
three material pointers (`FxElemMarkVisuals` is 24 bytes), desynchronising
Zerosub at `securitycamera_explosion2` (the map read as having no world).

### AnimClass (`walk/anim_class.rs`, 2026-10-27)

Type 0x20 (`AnimationClass`, 88 bytes) had no reader, so `mp_dome_ns` (the
Extinction-style alien animtree, asset 757 of 3043) stopped the walk. Order:
class name, state machine header, the states (40 bytes each), then per state
its anim entries (8 bytes each) and `animIndices` (RUNTIME: the pointer
follows but the loader fills the data, no file bytes; reading them as file
data overran by 489 bytes), the aim sets (112 bytes, all inline), the
scriptable (null in the one sample), three sound arrays (4 bytes each), effect
notes, effect pointers (loaded as `Fx`) and effect tags. IW6 states have no
alias lists (IW7's do). `mp_dome_ns` now walks 3043/3043 exactly (stream
end = zone end, VIRTUAL end = block size, 0 unresolved); `mp_prisonbreak`
and `mp_zerosub` still walk exactly. Unseen: non-null scriptable, aim sets,
sounds, effects.

### Createfx repeats (2026-10-27)

Placed effects played once at load. The kind (one-shot or loop) is not in the
compiled script's strings or record bytes (only the effect id, `fxid`, `delay`
and the entity key hash), so the rule is by effect: one with looping elements
(`elemDefCountLooping` > 0, header byte 20; `OwnedFx::loops`) plays once and
loops by itself; any other re-fires every `delay` seconds
(`asset_audio::ghosts_repeat_secs`: the delay when positive; unset, zero or
negative, the stored `-15`, takes the default 15 s, floor 0.1 s).
`CreateFxOneshot::repeat_secs` carries it (0 = once; every other lane sets 0),
`CreateFxRepeats` schedules (one play per due placement however long a stall,
on the placement's own grid, wrapping milliseconds), and the frontend's
createfx boot system plays the due ones. Exploders stay skipped. The 15 s
default is the magnitude of the stored sentinel, not read from the engine
script. Unverified in game.

`mp_lonestar` walks 1998/1998 exactly on this tree (stream end = zone end,
VIRTUAL end = block size, 0 unresolved, patch 15/15); no stop at asset 1161
reproduces. `mp_prisonbreak` (1741/1741) and `mp_zerosub` (2150/2150) too.

### Placed effects' axis (2026-10-28)

A createfx placement's angles give the effect's axis as `AnglesToAxis`
(forward, left, up), roll included (`render_fx::present::axis_from_angles_deg`;
it was forward-only with an arbitrary perpendicular up). Ghosts' effects author
their rise along the local forward (`vfx_crater_smk_large`: x +0.03 units/ms,
placed at pitch 283, forward up) or the local up (`vfx_crater_smk_wispy`: z
+0.0147, placed at pitch 359, roll -101); gravity is positive downward
(`vfx_crater_smk_large` +0.0025 x 800). `cargo run -p fastfile_iw6 --example
iw6_fx_motion -- <zone.ff> <name part>` dumps an effect's flags, gravity,
velocity samples and its placements' angles.

### Effect rules from the exe spec (2026-11-03)

Ghosts' particle rules (`docs/exe_notes/IW6_VFX_AUDIO.md`, part A) are
translated in `fx_iw4::iw6` and used through `render_fx::iw6_rules` (sprites),
`fx::spawn` (delay and life) and `assets::lane::iw6_fx` (createfx). The lane
keeps each element's own 312-byte record and 80-byte visual samples
(`asset_game::Iw6ElemSource`, `OwnedFxElemDef::iw6`) beside the AW conversion.

| Rule | Where | Switch (default) |
| --- | --- | --- |
| Element seed `(seq * 0x277f + effect seed + def seed + msec) mod 16361`; delay and life `base + ((rand16 * (amp + 1)) >> 16)`; a sound element more than 200 ms late dropped | `fx_iw4::iw6::{elem_seed, spawn_begin, life_msec, sound_fires}`, `fx::spawn` (`FxElemDefInfo::iw6_seed`) | `BO2MP_IW6_FX_RULES` (on) |
| Visual state: colour and the unlit HDR scale lerp base -> amplitude with the colour random; size, scale, rotation `base + r * amplitude`; rotation integrated over life; distance fades (min of far and near); alpha threshold 0 | `fx_iw4::iw6::visual_state`, `render_fx::iw6_rules::sprite_state` | `BO2MP_IW6_FX_RULES` (on) |
| Unlit colour `rgb * unlitHDRScale * scene` (the converted path set the scale to 1) | `iw6::unlit_rgb` | `BO2MP_IW6_FX_RULES` (on) |
| Lit types 3/4: `rgb * ((1 - b) * unlit * scene + b * light * litHDRScalar)` | `iw6::lit_rgb` | `BO2MP_IW6_FX_LIT` (off) |
| createfx: a placement plays once or loops by its placement; `playLoopedFx` repeat = round(s * 1000) ms, min 1 | `iw6::createfx`, `lane::iw6_fx::repeat_secs` | `BO2MP_IW6_CREATEFX_KIND` (on) |
| Weapon FXOverride: per fxType the entry with most attachments on the gun wins (first on a tie, 0 never) | `iw6::weapon::best_override`, `lane::iw6::attachment_fx_of`, `GuestAttachment::fx` | `BO2MP_IW6_FX_RULES` (on) |

Translated, with no Ghosts consumer yet: `keeps_bolt` (no Ghosts effect
is bolted: guest guns fire Black Ops II's effects), `flash_types` (ADS flash
first at ADS 1.0), `eject_type`, `flash_tag`, `run_frame`, `spawn_offset`,
`draw_list`, `approach_light` (lit type 3 light change rate).

Choices made against the spec or past it, from `mp_frag`'s data:

* **Sizes and scales add the random (`base + r * amp`); they do not lerp.**
  The spec says every channel lerps to the amplitude. On `mp_frag` 1747 of 2196
  visual samples have size amplitude 0 under a non-zero base, and every scale
  amplitude is 0 (bases up to 1000). Colour and unlit-scale amplitudes repeat
  their base. Only an additive range fits those numbers, and MWR's exe has the
  same split (`fx_iw4::h1`).
* **The random table is MWR's** (same seed range 16361). Ghosts' own table was
  not extracted; the spec says any uniform table works. Channels are IW4's
  numbering (inferred).
* **Placement kind comes from the stored delay.** The compiled script's call
  (createOneshotEffect or createLoopEffect) could not be read from the
  bytecode. A negative or zero delay (`-15`, the one-shot default) plays once;
  a positive delay loops on `playLoopedFx`'s repeat. All 245 `mp_frag`
  placements hold -15 and loop by themselves, so nothing changes there. Effects
  that loop by themselves still play once whatever the placement, because a
  replay would stack them (the loop-fx client handler is not determined).
* With FX rules on but the lit blend off, the converted lighting
  (`lightingFrac`) still lights the unlit colour. `mp_frag` has lit types 0, 1
  and 2 only (blend 0 on all but six elements), so the lit blend was not seen
  on a map.

Not translated (undetermined, or not in the spec): the loop-fx entity's
client replay and cull; gravity integration, bounce and collision; element
types 6/7, sprite corner maths, trails, spark fountains; FX sort and
soft-particle/distortion; the velocity graph's delta integration (the
converted IW4 velocity path stays); spawn offsets and origin frames keep the
converted path's IW4 randoms; one-shot counts, looping intervals and spawn
ranges (the spec gives no formula; IW4's draws stay); which unlit scale is
`unlitScale` (the sample's is used; the element's `unlitHDRScalar` at +300 is
not); the scene scale (1 is used); dynamic lights at 0.25 (flag 0x80000); the
extra lit-brightness scale (flag 0x10000); the premultiplied-RGB material flag;
the alpha line's `fadeIn * fadeOut` against the fade function's min (min is
used); exploder delay and loopsound; the random table's values; impacts,
tracers and hit culling for Ghosts guns (they fire Black Ops II's effects);
fxType 7 (mag eject) and 8 (ADS flash) have no catalog slot; a two-attachment
variant scores each attachment alone.
`iw6 fx rules:` in the map log gives the switches, the lit types of the
placed effects' elements and how many placements play once or repeat.

### Element types, physics and the gun's extra effect slots (2026-11-03)

Measured on the staged zones (scratch probe over every element of `mp_frag`, `common_mp`, `common_core_mp`):
Ghosts' records use types 0, 1, 2, 3, 4, 8, 9, 10, 11, 12, 14, 15, 16; **5, 6 and 7 are never used**. Types
8 and 9 are the clouds (the spec's third draw list), which AW numbers the same; the lane mapped Ghosts' 6/7
to AW's clouds and dropped 8/9 as unsupported, so `common_mp`'s glass and mud impacts (73 cloud elements),
`mp_frag`'s `snow_light_mp`, `vfx_dust_indoor_frag` and the sprinklers (`vfx_sprinkler_system_frag`, a
type 9) lost those parts. Now 8 and 9 keep their numbers (`iw6_fx::aw_elem_type`, `BO2MP_IW6_FX_CLOUDS`, on).

Gravity (offset 156), the reflection factor, collision bounds, spawn ranges, spawn offsets, the velocity
graphs, the spark fountain record (52 bytes) and the trail record's first 20 bytes sit where AW's do (the
layouts agree to byte 292), so the family's physics (gravity, bounce, collision traces against the world,
velocity graph integration, trails, spark fountains) already run on Ghosts' own numbers; only their random
draws are IW4's. `BO2MP_IW6_FX_DRAWS` (off: the channel and the seed's spawn time are inferred) draws the
one-shot count from Ghosts' element seed and table (`fx_iw4::iw6::oneshot_count`); spawn offsets, gravity
and reflection keep IW4's draws.

The gun's view flash while fully aimed (`fxType` 8, `viewFlashADSEffect`, 200) and its view magazine eject
(`fxType` 7, `viewMagEjectEffect`, 640) now have slots (`WeaponCombatFx::{view_flash_ads, view_mag_eject}`,
attachment overrides 7 and 8 included), their effects converted with what they run
(`BO2MP_IW6_ADS_FLASH`, on). A first-person shot at ADS fraction exactly 1 plays the aimed flash when
bound (`fx_iw4::iw6::weapon::flash_types`), else the view flash. Retail `common_core_mp`: 49 guns name an
aimed flash (`vfx_muz_ar_v_ads` ...), with 150 or so attachment overrides of type 8 (`_si_ads` silenced,
`_fla_ads` flash hider, `_muz_ads` muzzle brake); no gun or attachment names a magazine eject, and nothing
plays it (the trigger was not read).

### Ghosts guns' own impacts and tracers (2026-11-03)

`assets::lane::iw6_gun_fx`, with the table logic in `fx_iw4::iw6::weapon::{impact_rows, impact_column}`.
Black Ops II's guns are untouched.

| What | Read from | Switch (default) |
| --- | --- | --- |
| Bullet impacts: the cells of Ghosts' `ImpactFx` on the row of the gun's own impact type (going in; the alternate row coming out), each Black Ops II surface on its Ghosts column, the four flesh cells (body, body fatal, head, head fatal: columns 31..34) | `common_mp`'s impact table (`fastfile_iw6::walk::ImpactTableRec`, kept by `shared::SharedImages::impact_table`); effects and what they run from the shared zones, converted as a map's (`iw6_fx::gun_effects`), joined to the match's effects under the Ghosts namespace; `GuestImpacts` per gun | `BO2MP_IW6_OWN_IMPACTS` (on) |
| Tracers: the gun's `TracerDef` numbers (draw interval, speed, beam length and width, screw radius and distance, five colours) | `common_core_mp`'s tracers (`walk::TracerRec`; the patch zone's first) | `BO2MP_IW6_OWN_TRACERS` (on) |
| The tracer's material when no zone read holds it: Black Ops II's first tracer material (`OwnedTracerDef::material_standin`) | stand-in | `BO2MP_IW6_TRACER_STANDIN` (on) |

Measured on the staged zones (scratch probes, not kept): `common_mp` holds one impact table (15 rows of 38):
row 0 small, 1 small exit, 2 large, 3 large exit, 4 explosive, 5 and 6 shotgun, 7 explosive, 8 and 9 large
(armour piercing), 10 footstep dust, 11 small (fatal flesh), 12 grenade, 13 rocket, 14 small. Column 26 holds
cloth effects (rubber's slot; Black Ops II's cushion reads it), 28 `metal_decal` (painted metal). The 52 Ghosts
guns with a tracer read impact types 1 (pistols, SMGs), 2 (rifles, LMGs, snipers) and 5 (shotguns); 8 and 9
(grenade and rocket rows) are left to the projectile path. The effect trees are complete in the shared zones
(type 1: 56 cells, 120 effects; type 2: 122; type 5: 48), their 1,700 or so sprite materials all found;
their pictures are streamed (pak 4, 7 ... not staged in the cloud), so a cell whose tree shows nothing falls
back to the match's table (`iw6_gun_fx::drawable`). `common_core_mp` holds 11 tracers (`assaultrifle` 18000
u/s, 200 x 1, interval 3; `smg`, `pistol`, `lmg`, `sniper`, `shotgun`, `fmj_tracer` ...); their material
`gfx_tracer_new` is in neither `common_mp` nor `common_core_mp` (it is `code_post_gfx_mp`'s, whose walk stops
at asset 252 of 1195), hence the stand-in material. One line each in the match log: `iw6 guest guns: own
impacts ...`, `iw6 guest guns: own tracers ...`. Not seen in game.

## Next

1. Ghosts attachments: check the override condition reading, the
   models' roots and the sights in a match (written blind: no Ghosts zones
   here). Its own reticle models and stat changes are not read (Black Ops
   II's sights and numbers stand in).
2. **Maps** (in game, unchecked): the sky (drawn from the map's own
   `GfxSky` cube, see "Reflection probes and the sky"; not seen on screen), the minimap picture (in the map zone,
   which the HUD does not read for an `iw6` map), static model collision
   (Ghosts traces a placed model's collision LOD mesh; only the clip brushes
   around props are used), dynamic entities and scriptables (read, not
   placed), glass, `worldspawn` keys still numbered (`iw6_<n>`), DLC maps.
   Old notes: `mp_prisonbreak.ff` (+ `_load`, `_path`, `techsets_...`,
   `patch_...`): ReverbPreset now reads (72 bytes: name, 16 floats), and
   FxWorld follows x64-zt's writer but ends 0x40 bytes early on retail data.
   The first glass def's damaged/destroyed/destroyedQuiet sound slots hold
   all-ones values (not the `fd`-filled follow), and defs 1..3 reference
   them 0x5d8 bytes later. The walk then reaches GfxWorld (asset 1736)
   with 15 unresolved technique-set references, before MapEnts, GlassWorld
   and ClipMap. Technique sets with programs (code_post_gfx_mp,
   techsets_mp_prisonbreak) still drift: only their names are needed for
   BO2's draw path.
3. Streamed sounds' localized paks (`<language>/..._soundfile<N>.pak`) are
   unverified.

### Black Ops III's look (2026-10-27)

Ghosts maps are drawn through the fallback pass's Black Ops III branch
(`bo3_look::apply`, rt_ambient on, local lights off) by default;
`BO2MP_BO3_LOOK_IW6=0` returns to CoD4's lightmap draw under the map's own
exposure.

* **Exposure, once.** The look converts the secondary pages linearly
  (`iw3_lightmap_look`, sqrt(light / 4), `t7_iw_pages` in the shader), scaled so
  the shaded texels (sun visibility under 25) average `BO2MP_IW6_SHADE` (0.4)
  over up to six pages; the "Exposure" key above (`light_exposure`) and the sun's
  gamma clamp are not applied, and placed models' ground light takes the same
  scale (`BO2MP_IW6_MODEL`). The tone curve is the exposure
  (`BO2MP_IW6_EXPOSURE`, 1). The map's film vision is left out
  (`BO2MP_IW6_VISION=1` keeps it).
* Sun: the ComWorld's hue, light `BO2MP_IW6_SUN` (2.6) in those units, none if
  the bake holds it (sunlit/shaded >= 2.5; Prison Break reads 1.63). Fog: the
  `_fog.gsc` block as before (colours squared). The rebake's sky: the fog colour.
* Props, including the extra ones, are casters for the sun map and the probe
  trace and lit by the rebake where they have no ground light.
* **Gaps.** Reflections: Ghosts' own probes are read since 2026-11-03 but
  drawn only with `BO2MP_IW6_OWN_PROBES=1` (see "Reflection probes and the
  sky"); by default the sky probe stands in. Sun level and exposure are constants,
  not fitted against pictures. Volumetric scatter uses the defaults. Prison Break:
  load 65 s, 34 s of it the sun map and trace (9.7 M triangles).

### Black Ops III's materials, light and effects on a Ghosts map (2026-10-29)

Written from code, compiled and unit-tested; never seen on screen (no Ghosts
zone, no GPU on the box that built it). Everything is decided at map load from
the installs, with no hand-made table; with no Black Ops III install the
Ghosts map loads exactly as before.

**Materials** (`bo3_subst`, `t7::bo3_pool_run`, called from `iw6_map` under the
look; `BO2MP_BO3_MAT_SUBST=0` off). The pool is the install's multiplayer map
zones (`BO2MP_BO3_MAT_ZONES=mp_a,mp_b` or up to three of a favourites list,
`BO2MP_BO3_MAT_BUDGET` seconds, default 150, stops opening more zones). The
shared zones were measured instead and are NOT the pool: `mp_common` plus
`core_common` hold 4667 + 5677 materials of which 3 have a surface class (a
shotgun, a shoe, a sprite): props, weapons and characters; the world's
materials are in the maps' zones. Reading those two took 65 s on the build box
(2 cores): every zone read costs tens of seconds, and it is paid at every
Ghosts load (no cache yet). A Black Ops III material gets a class from its
collision surface type (the map zone's clip materials, bits 20..25, Black Ops
II's list) and its names, a Ghosts material from its name and its colour map's
name (`classify`); each colour map is measured at the first mip of 32 texels
or less (mean linear colour, brightness spread); a Ghosts material takes the
nearest-colour Black Ops III material of its class when
`sqrt(name confidence product) x colour fit` is at least
`BO2MP_BO3_MAT_CONF` (0.6) and the colour distance is at most
`BO2MP_BO3_MAT_DIST` (0.2). The swap replaces the colour, normal (BC5) and
specular (gloss in alpha) bindings of the Ghosts material (Builder::material of
the t7 lane makes them), keeps its draw. Kept as Ghosts' own: names with
decal, sign, poster, flag, logo, glass, window, sky, water, cloth, light,
etc.; materials that are layered, unlit, cut-out (leaf and grass cards carry
their atlas's UVs), blended, animated, decal, effect or water; and any whose
class has no Black Ops III colour within the distance. Log lines:
`iw6 material substitution: N materials: S swapped for Black Ops III's {class:
count} (pool P materials from <zones, per zone count, collision surfaces,
timings>), K kept (game's own..), E kept (cannot take another texture
{reason: count}), U unmatched, F kept (no Black Ops III colour within ..), L
link failed`, then `swapped, e.g. ghosts -> bo3 (class, confidence, colour
distance)` and `kept for want of a fit, e.g. ...`. Unknown until a PC log
comes back: how many Ghosts materials get a Black Ops III equivalent, and
whether the swapped textures tile at a scale that suits Ghosts' UVs (the
picture is swapped, the UVs are not).

**Material policy, tightened (2026-10-29, after a PC screenshot of mp_frag).**
The first policy swapped by class and nearest mean colour and made things
worse: a Ghosts container (surface type metal) took
`mc/mtl_p7_crate_ammo_metal_small` and drew as a maroon electrical panel, a
wall took `mc/mtl_p7_sin_vista_building_...`, `wc/zm_trim_metal_blk`,
`mc/mtl_p7_conduit_metal_pipes`, `wc/t7_concrete_tiles_4x4_...`: pictures laid
out for one prop or one tile grid, not tileable ground. The catalog is shared
by world and models, so the props took them too. Now: (1) only classes that
tile alike on anything swap: concrete, snow, dirt, rock, asphalt (`swappable`);
metal, wood, brick, grass and foliage keep Ghosts' own; (2) a Ghosts material's
class comes from its names (its material's and its colour map's), a collision
surface type only vetoes (`ghost_class`: `*a101n_155z` with a metal surface is
no longer "metal by evidence"); (3) a Black Ops III candidate must be a world
material (`wc/`, not a model's `mc/`), have a ground / concrete / rock /
snow / asphalt word and none of the prop, trim, tile, edge, panel, building,
crate, decal, brick... words of `POOL_DENY` or a `4x4`-style pattern size
(`generic_world`, unit-tested on the names from the log); (4) the pair's
contrast may differ by 0.08 at most (a flat painted surface does not take a
busy picture), its chromaticity by 0.05 (a mauve ground for a grey-brown one is
a pink patch; the tint only darkens, it cannot turn a hue), and the colour
distance default is 0.12 (was 0.2). Fewer swaps by design: with the pool names
seen in the log the expected ones are the mud, sand and concrete ground
materials. Unknown until a PC log: the count it comes to, and whether Ghosts'
UVs suit those pictures' tile size (not measurable here: the texel density of
a Ghosts surface is its mesh's UV span over the picture, not in the material).

**Light** (`bo3_look::{map_sun_ratio, solve_exposure}`, `iw6_map`;
`BO2MP_IW6_MAP_LIGHT=0` returns the old constants). The sun drawn is the
ComWorld's hue; its level, where the bake does not hold it, is the shaded
level (0.4) times `6.5 x ((sun HDR peak / baked shaded mean) / 75)^0.35`
clamped to 3..10 (the old 2.6 / 0.4 was set by eye on Prison Break, sun 9 over
a shaded mean of about 0.12: INFERRED anchor and exponent). The exposure is
solved from the traced light so a mid-grey in the sun draws 0.65 on Black Ops
III's tone curve (`solve_exposure`, unit-tested to 0.01); with no traced
ambient, or a baked sun, the old 0.8 stays. `BO2MP_IW6_SUN`, `_EXPOSURE` and
`_SHADE` still set each by hand. Log: `iw6 light: sun xS (the map's: ComWorld
sun peak P over the baked shaded mean M gives R x the shaded level), exposure
xE (solved ..)`, `exposure: xE solved from the map's light ..`, then the look
check lines as before (a WARNING there means the derived numbers still drew
flat or blown). Fog and the rebake's sky colour were already the map's.

**Overexposure of props, steel and the gun (2026-10-29).** Measured headless
on the real mp_frag (`ghosts_look_report`, the zones staged; the pictures'
`.pak` parts were not, so materials could not be decoded, the light numbers
are unaffected): baked shaded level 0.353 x scale 3.19 = 1.13; the sun live at
2.22; traced ambient up 0.41 / side 0.34; exposure solved 0.65; look check
passes (mid-grey 0.65 in sun, 0.28 shade, white 0.8 -> 0.79). The placed
models' own ground light had a median of 2.5 (p90 6.0, max 11.9): 6 times the
traced ambient the world gets, and the sun is added on top, so every prop was
over-lit while the world was in range. Cause (code, plus the numbers): the
grid's ground reading is the map's own scale, not the look's. Fix: with the sun
live the models' light is scaled to put its median at the world's shaded level
(`model_light_k`, never brightens, floor 0.15; mp_frag x0.34, models now
median 0.85 for those without their own), and inside the traced grid
`t7_shade` already takes 85 % of the ambient from the trace (the first-person
gun is lit there too). Separately, albedo past 0.5 now rolls off at a quarter
(`BO2MP_IW6_ROLLOFF=0` off; Modern Warfare III's rule in `t7_shade`) because
Ghosts' pictures are authored for its own tone mapping and paint, snow-covered
steel and bark carry albedo near 1. INFERRED, not seen: that these two bring
the crane, the gun and the bare tree into range; if the gun is still a flat
grey the cause is its own picture (a pale colour map), which `BO2MP_T7_VIEW=3`
shows. `iw6 look summary:` repeats the numbers in one line per map.

**Effects** (`bo3_vfx`, `iw6_fx::prepare`; `BO2MP_BO3_VFX_SUBST=0` off). A
createfx effect whose path names ember, spark, fire (small, large), steam,
smoke, dust, ripple or drip (and not waterfall, god rays, rain, birds...)
plays the shortest ambient Black Ops III effect (not impact, muzzle, shell,
blood, weapon, view, zombie...) among the install's effect names
(`T7Map::effect_names`) that holds the rule's words; the views are read with
`t7_map::shared_effects` and handed over through `t7_fx::set_pending`. Every
other effect plays as Ghosts'. The placement's angles are changed so the
Black Ops III effect rises where the Ghosts one did (`rise_of` reads the
local velocity samples; Black Ops III's rise along forward is INFERRED from
Black Ops II's createfx at pitch 270). Log: `iw6 vfx substitution: M of N
effects played as Black Ops III's {rule: count} (P placements; Q placements
keep the game's own, K effects: e.g. ..)`. Checked against the install's 915 shared-zone effect names: they are
character, killstreak, destruct and weapon effects (fire_ai_human_*), not
world ones, so `NOT_AMBIENT` rejects those and most rules keep Ghosts'. Not done: Ghosts guns' muzzle flashes and
explosion lights.

**The earlier screenshot defects** (white foliage with black speckle, black
vertical stripes on container faces, blown bloom, grey sky dome) have no root
cause found. Reading the lit branch (`t7_shade`) gives two things worth
testing first, neither verified: a Ghosts map has no reflection probes (see
Gaps above), so the probe term `env` and the ambient's probe ratio
(`amb *= clamp(l6n / l6v, 0.5, 2)`) are taken from a probe that is not there
(with both zero the ratio clamps to 0.5 on every shiny material); and sun
shadow acne on large vertical faces would make vertical stripes. To split the
causes on the PC without a code change: `BO2MP_T7_VIEW=1` (sun visibility),
`2` (ambient), `3` (albedo), `4` (ambient + sun) in the same spot.

## Map sound and Ghosts' alias rules (2026-11-03)

From `docs/exe_notes/IW6_VFX_AUDIO.md` part B, translated into
`asset_audio::iw6_rules` (pure functions with tests) and applied only to the
Ghosts namespace (`AssetNamespace::Iw6`: a Ghosts map's own sounds and the
Ghosts guns' sounds). Black Ops II's sounds keep Black Ops II's rules.

**Reader** (`fastfile_iw6::sound::AliasParams`, `walk/sound.rs`, `walk/misc.rs`):
every alias now keeps velocityMin, master priority and percentages, LFE and
centre percentages, envelope, wetMixOverride, allowDoppler, its three curves
(name and knots: `SndCurve` knot count is a u32 at 16, up to 16 (x, y) pairs at
20), speaker map name, occlusion shape (kind, inner, outer, occluded volume) and
Doppler preset; `Capture::snd_curves`, `doppler_presets`, `occlusion_shapes` and
`string_tables` (cells) hold them. Checked on the uploaded retail zones
(`IW6_ZONE_DIR=... cargo test -p fastfile_iw6 --release retail_alias_params --
--ignored`): both walks stay exact (mp_frag 1702/1702, common_core_mp
3034/3034); every alias has a volume curve; a zone's own curves carry knots
(`prisonbreak_radio` volume, `dog`/`battlechatter`/`reactive_emitters`
low-pass), curves another zone holds are `,`-named with no knots
(`,$default`, `,footsteps`, `,weapon2` ...: `common_mp`'s); every occlusion
shape is the default one (180/180/0); no Doppler preset, reverb preset or
string table is in either zone (`allowDoppler` is set on 21 common_core_mp
aliases, which take the dvar defaults). Flag bits agree with the spec: bit 0
on the `_lp` aliases (`ambient_mp_frag` 0x9539, `emt_frag_brewery_lp`), file
mode bits 7-8 (2 streamed for the ambient, 1 loaded for steps), channel bits
9-15 (steps 42, impacts 24, ambient 74), 0x10 with a wetMixOverride on every
alias that has one (steps 1.1554).

**Loader** (`asset_audio::sound_load_iw6`): rows carry `CapturedAlias::iw6`
(flags, wet override, Doppler, reverb-send curve, occlusion shape...; the IW4
`flags` stay unset), their volume curve by name `iw6/curve/<name>`; each zone's
own curves go into the catalog's curve table, so a common_core_mp or map alias
naming `,$default` takes `common_mp`'s knots when the banks are composed
(`resolve_curve_knots`), else the guest default as before.
`ghosts_map_catalog` keeps every list of a Ghosts map zone (from the map lane's
walk, no second read); `ghosts_ambient_alias` finds `ambient_<map>`.

**Map lane** (`assets::lane::iw6_map::attach_map_sound`): the map's catalog
becomes the match's map sound, `iw6:ambient_<map>` its ambient loop (played at
the alias's own volume, mp_frag 0.375), and one line per map:
`iw6 sound: mp_frag: N alias lists, N loaded clips, N curves of its own,
ambient ambient_mp_frag, 0 sound tables ...; map sounds on, curves on, ...`.

| Switch | Default | What |
|---|---|---|
| `BO2MP_IW6_MAP_SOUNDS` | on | the map zone's own aliases and ambient bed (before: none, a Ghosts map was silent but for the guns) |
| `BO2MP_IW6_CURVES` | on | each alias's own falloff curve (before: the guest default for all) |
| `BO2MP_IW6_GATING` | on | flag 0x40 drops a sound inside distMin, none starts past distMax, per-play volume clamped to [0, 1] |
| `BO2MP_IW6_PICK` | off | Ghosts' alias choice (MSVC `rand`, reservoir by probability, no repeat in lists of more than two) and its per-play numbers |
| `BO2MP_IW6_DOPPLER` | off | Doppler pitch from the listener's movement on aliases with allowDoppler (preset or the dvar defaults 900 / 0.1 / 0.1 / 4 / 6) |
| `BO2MP_IW6_REVERB` | off | the dry/wet rule (0x08 full dry, 0x10 own wet) against the map zone's reverb on the six-slot stack |
| `BO2MP_IW6_FOOTSTEPS` | off | a Ghosts map's own `step_*` / `land_*` aliases by surface, `_default` fallback, before Black Ops II's |

Pure logic in place with tests but not wired into play (no data or no
event to drive it): the sound tables' zone and reverb rows
(`soundtables/mp_common_zone.csv`, `..._reverb.csv`: neither uploaded zone
holds a string table; parsed, the start zone applied to the stack when found),
whizby bands, emitters and aliases (no bullet pass-by event on the client),
ambient elements (their tables are absent), occlusion shapes (all default;
one-shots carry no emitter axis), the listener speed for velocityMin (the gate
takes it when known; the client passes none). Not determined by the spec and
left out: low-pass curve use, speaker map tables, the LFE rule, mixer ducking,
ADSR and timescale rows, occlusion lerp use, world occlusion EQ (needs world
traces in the audio crate), the reverb preset's 14 DSP values (no mapping to
the echo; and no `ReverbPreset` in these zones), the row-typed
`soundtables/<map>.csv` layout (unverifiable without a table), weapon fire
sound selection. Not heard in game (no Ghosts install here; the zones are the
uploaded mp_frag and common_core_mp).

Footstep surfaces go by Black Ops II's list (`iw6_surface_flags_to_t6`), so a
Ghosts-only type (`metalgrate`) reaches its alias as `metal`.

### Ghosts' client sound rules in play (2026-11-03)

`audio::iw6_sound` (frame systems), `asset_audio::iw6_rules` (maths), the effects pass's shot segments
(`render_frontend` `iw6_bullet_sounds`). Ghosts guns only; Black Ops II's guns and sounds unchanged.

| Rule | Read from | Switch (default) |
| --- | --- | --- |
| Bullet pass-by: each shot segment of another player's Ghosts gun is an `Iw6BulletPass`; the closest point from 64 units along the shot, near < 40, medium < 80, far <= 160; the `whizby_<band>_NN_L/R` pair at the two emitters either side (width 400..600, 600..1000, 1000), the variation from a shuffled bag (no repeat until all played); `whizby` when no pair is in the bank | B7; `common_mp`'s aliases (near 00..07, medium 00..07, far 00..05, each with a `_space` twin; now kept by the gun sound loader, prefix `whizby`) | `BO2MP_IW6_WHIZBY` (on) |
| Bullet impact sounds: a Ghosts gun's hit plays `bullet_<size>[_exit]_<surface>`, `_default` after: size by the gun's impact type (Ghosts' numbering is IW4's: 1 small, 2 large, 3 armour piercing, 4 explosive; shotgun 5/6 small, INFERRED), the surface's Ghosts name (Black Ops II's `player` flesh, `tallgrass` grass). The gun's own impact effects (above) carry no sound element, so without this its hits were silent | B8; `common_mp`'s `bullet_small_*` (40 surfaces with `_exit_` twins), `bullet_large_*`, `bullet_ap_*`, `bullet_explode_*` (now kept, prefix `bullet_`) | `BO2MP_IW6_IMPACT_SOUNDS` (on) |
| Listener speed for `velocityMin`: the start gate drops an alias whose `velocityMin` is above the listener's speed (inches a second, from the listener's motion, smoothed; a step over 256 units in a frame is a teleport: no speed). Read again on 2026-11-03: the gate's value is the length of the velocity the client hands its listener update | B1; on the zones read only `mp_frag`'s one `emt_single...` emitter has a `velocityMin` (968) | `BO2MP_IW6_LISTENER_SPEED` (on) |
| World occlusion: a positioned Ghosts voice whose alias has a non-default occlusion shape is traced from its emitter (+10 z) to the listener against the world (solid contents) at most every 150 ms (`snd_occlusionDelay`), one voice a frame, the earliest due; while blocked its volume moves to the shape's occluded volume over 500 ms (`snd_occlusionLerpTime`; that use is INFERRED). The occlusion table's EQ (low/high-pass) is not applied: no sound table in the zones read | B3 | `BO2MP_IW6_OCCLUSION` (on); every occlusion shape on `mp_frag`, `common_mp` and `common_core_mp` is the default one (180/180/0), so nothing is traced on the retail data |
| Fire loop and fire stop: a Ghosts gun with a `fireLoopSound` (`fireLoopSoundPlayer` in the shooter's view) starts it on its first shot on the shooter's entity, keeps it while shots come and stops it with `fireStopSound` (`...Player`) once none came for 250 ms (INFERRED: the client's loop path was not read) | B10 fields 288..312 | `BO2MP_IW6_FIRE_LOOP` (on); no gun of `common_core_mp`'s 134 (patch zone included) names a loop or stop sound, so nothing loops on retail data |
| Left akimbo gun: in the shooter's own view the left gun's shot takes `fireSoundPlayerAkimbo` (with the attachment's type 3 override) in place of `fireSoundPlayer`; a last shot still takes the last-shot sound | B10 step 2 | `BO2MP_IW6_AKIMBO_SOUND` (on); 50 guns name one (`weap_ak12_fire_aki` ...), and the pistols' akimbo attachment overrides (`weap_p226_fire_aki`, silenced `weap_p226sup_fire_aki`) |
| Reverb presets: the decompile shows the preset's 14 floats feed Ghosts' own reverb DSP (not I3DL2): two of them scale its delay lengths (0.1..16), the rest are 0..1 / 0..4 gains whose roles were not traced; no `ReverbPreset` is in the zones read. Not mapped to the echo DSP | B5 (`docs/exe_notes/IW6_VFX_AUDIO.md`) | `BO2MP_IW6_REVERB` stays off |
| Sound tables: `common_mp` holds no string table (none of its 8,777 assets; nor `patch_common_mp`, `common_core_mp`, `common`, `ui_mp`, the code zones). Ghosts keeps them in the **localized** zones: `english/eng_common_mp.ff` (`soundtables/mp_common_zone.csv` 245 x 5, `_reverb` 211 x 6, `_filter` 20 x 7, `_occlusion` 45 x 6, `common_mp.csv` 592 x 8), `eng_patch_common_mp.ff` (a newer `common_mp.csv`, `mp_battery3.csv`), `eng_mp_frag.ff` (`mp_frag.csv` 29 x 12), `eng_common_core_mp.ff` (`common_core_mp.csv`). A Ghosts map now reads `eng_patch_<map>`, `eng_<map>`, `eng_patch_common_mp`, `eng_common_mp` (a table's first copy wins); its zones are the `zone_name` section of `soundtables/<map>.csv` (`parse_typed_zone_rows`) then `mp_common_zone.csv`, its reverbs `mp_common_reverb.csv`. mp_frag starts in zone `mp_frag`, reverb `mp_lonestar_ext` (preset `generic`, dry 0.9, wet 0.1). The tables have no `whizby_name` rows (the band defaults stand). Zone changes inside a map (the map's sound triggers), the filter, occlusion EQ, mix, ADSR and ambient rows are read by no rule yet; the reverb levels feed the dry/wet rule only with `BO2MP_IW6_REVERB=1` | B6; one `iw6 sound tables:` line per map | `BO2MP_IW6_SOUND_TABLES` (on) |

## Bullets: Ghosts' shot mask, its penetration table, props that stop bullets (2026-11-03)

- **Shot mask**: Black Ops II's bullet trace asks for `0x02806831`; Ghosts' for `0x0280E831` (adds `0x8000`, a
  body type; `docs/exe_notes/IW6_COLLISION.md` 4.1). `sim` now asks `World::bullet_mask_for(weapon)`: Ghosts'
  on a Ghosts map (every gun) and for a Ghosts gun anywhere, Black Ops II's otherwise (BO2 guns on BO2 maps
  unchanged). `BO2MP_IW6_SHOT_MASK=0` gives Black Ops II's everywhere. On mp_frag no collision material,
  brush or model carries `0x8000` (it is a run-time body type), so nothing on the map hits differently.
- **Penetration table**: `fastfile_iw6::penetration` reads `info/bullet_penetration_mp` as the game does
  (info string `BULLET_PEN_TABLE`, keys `small_|medium_|large_<surface>` over Ghosts' 31 surface names, with
  `metal_grate` at 14 and no `cushion`), found by name in a zone image (the raw file's header, name and data
  in a row). It is in none of the staged zones (mp_frag, common_core_mp, the code_post_gfx zones); the Ghosts
  gun lane looks in the install's `code_pre_gfx_mp`, `patch_code_post_gfx_mp`, `code_post_gfx_mp`,
  `patch_common_mp`, `common_mp` once a process. `asset_world::iw6_pen_depths_to_t6` puts the depths in
  Black Ops II's surface list (the map flags are moved to that list, `iw6_surface_flags_to_t6`): metal over
  metal grate, snow over slush, BO2's cushion takes cloth; BO2's `player` and `tallgrass`, which Ghosts has
  no column for, take flesh and grass (INFERRED). `World::penetration_table_for(weapon)` gives Ghosts' guns
  that table, every other gun the match's. `BO2MP_IW6_PEN_TABLE=0`: Black Ops II's for all. The match log
  says which: `iw6 bullets: shot mask ..., N Ghosts guns, their depth table ...` and `iw6 penetration table:
  ...` (found and from which zone, or not found). Not checked on the real table (not staged).
- **Props**: the dynamic entities, scriptables and script models (`map::extra_props`) are line-trace targets
  like the static models (`iw6_model_collision::prop_collision`: each model's collision LOD at the prop's
  starting pose; bullets and sight meet them, the player's capsule does not, as Ghosts' and Black Ops II's
  player never collides with clutter). `BO2MP_IW6_PROP_COLLISION=0` leaves them out. They do not move when
  shot (no dynamic entity physics) and doors moved by `iw6_doors` keep their props' collision where it was
  (only the brush models' collision moves).

## Surface types (impacts, marks, footsteps, penetration)

Ghosts' collision flags carry the surface type in bits 20..24 as CoD4's do, but
its list is not Infinity Ward's or Black Ops II's: a second metal,
`metalgrate`, sits at 14 and every type from `mud` up is one above BO2's.
Measured on `mp_prisonbreak`'s collision materials (`clip_grating_metal` 14,
`pbk_river_rock_wall_*` 18, `pbk_water_stream_rolling` 21, `pbk_wood_planks_01`
22, `pbk_asphalt_road_light_01` 23, `pbk_plastic_gray` 25, `clip_nosight_paintedmetal`
28) and on `common_mp`'s `ImpactFx` columns (`vfx_imp_sml_<type>` for column
0..30: 13 and 14 metal, 15 mud ... 25 plastic, 26 rubber, 27 fruit, 28 paintedmetal,
29 riotshield, 30 slush; 31..37 flesh). Modern Warfare Remastered's list is the
same (`mp_bloc`: grating 14, snow 20, water 21).

`asset_world::iw6_surface_flags_to_t6` (`SurfaceList::Iw6`) moves a Ghosts or
Remastered map's flags to BO2's list when the BO2 rules load (`t6_surface_list`):
before, only slush and riot shield were moved (Infinity Ward's rule), so a rock
wall read as BO2 `sand` (impact `fx_small_dirt`, mark `gfx_crater_dirt`: a faint
dust puff and a grey noisy crater patch), water as wood, wood as asphalt,
plastic as rubber. BO2's `rock` is `fx_small_rock` with `gfx_impact_plaster01/02`.
The same list drives footsteps and bullet penetration. Type 31 is no surface
(default); slush is snow, as for Infinity Ward. Infinite Warfare maps carry no
surface flags yet (their collision is a flat soup: every impact is `default`).
### Foliage and light of placed models (2026-10-28)

The owner's Prison Break screenshot (props present) showed tree canopies as
dark, jagged, solid sheets of leaf picture. `iw6_foliage_probe <zone.ff>`
(`PROBE_GREP=`, `PROBE_MODELS=1`) lists each placed model's materials: technique
set, placements, vertex colour mean, state bits, colour map format.

* **Cause.** Ghosts cut-outs on models come in two spellings: `..._t0c0..`
  (the lit ones, which the reader knew) and `m_unlit_atest_lin_[flagext_wv0_]
  nocast_bl_nfwpf` (`atest` = alpha test, `bl` = baked light): every tree
  canopy, twig card and vista tree of Prison Break (8 materials, 328 surface
  placements, 24,000 vertices). The reader took them for unlit, opaque cards:
  no cut-out, and Black Ops II's unlit path squares the picture with no light
  at all (the "greener-black"). Not the cause: the colour maps' alpha (BC2/BC3,
  the block data goes to the GPU as it is, mips included, 512 px is a level
  choice, not a resize), the vertex colour (255,255,255,255 on every foliage
  surface but the reactive grass's (255,13,0): whitening applies, and changes
  nothing here), and the cull (`iw6_model_draw` already draws every model
  two-sided). The `GfxStateBits` of these materials carry no alpha test:
  under D3D11 it is the pixel shader's, named by the technique set.
* **Fix.** `iw6_model_draw`: `atest` is alpha tested, drawn opaque (cut-out),
  lit as the other foliage (the BO3 look's sun, probes and shadows) instead of
  unlit; the tree's `shadowcaster_atest` cards stay shadow only; emissive
  `unlit_replace` panels stay unlit. World materials take `atest` too. Prison
  Break: 96 model materials alpha tested, all two-sided (88 before), 92 lit;
  6,784 surface placements drawn alpha tested (328 more).
* **Back faces.** The prop fragment shader turns a back face's normal to the
  eye for alpha-tested surfaces under Black Ops III's light
  (`@builtin(front_facing)`), so the underside of a canopy takes the sky
  and sun as a surface and not the far side's ambient.
* **Shadows.** `bo3_look` takes casters from the placed models and the extra
  props alike (inside the sun map's square), by the material's draw: opaque
  casts, alpha tested casts half (the sun map's checkerboard), unlit and blended
  cast nothing. The canopies were unlit, so cast nothing and did not block the
  probe trace; now lit they would cast. Ghosts' `nocast` (and `nocastp0`)
  techniques are left out (`bo3_look::apply_except`; 37 Prison Break materials:
  leaf cards, grass, some trunks; a tree with a `shadowcaster_atest` card
  casts by it, solid here since the sun map has no texture): the engine casts
  none from them. Prison Break's casters: 7.7 M triangles, 2.6 M of them partial.
* **Light of a model with none of its own.** The map's light grid (header
  `GfxWorld.lightGrid`: row/column axes, palette bitstream, range exponents) is
  not decoded; before, every such model (84 % of Prison Break's, every extra prop)
  took one median ground light. Now the inverse-distance mean of the four
  nearest ground-lit models (height counts double). Under the BO3 look the
  props pass also reads the real-time probes per pixel (85 % where the
  surface is inside their grid), so this light matters outside the probes' box
  and for the remaining 15 %. Prison Break, the 6,837 placements with none of
  their own (mean of rgb, the look's units): before all 1.268; now min 0.105,
  p10 0.605, median 1.204, p90 2.074, max 3.612 (`iw6 model light of the ...`
  report line; `ghosts_look_report`).
* **Exposure.** The look's tone curve is `BO2MP_IW6_EXPOSURE`, default 1 ->
  0.8 (sunlit sand is about 0.77 of white at 1.0 by hand, shade about 0.44; at 0.8
  about 0.73 and 0.39). A guess against one screenshot, not a fit: the
  shaded mean (`BO2MP_IW6_SHADE`, 0.4) and sun (`BO2MP_IW6_SUN`, 2.6) are unchanged.


## Light, probe and material pass, second round (2026-10-29)

What the Ghosts maps get under Black Ops III's look, and how each number was
measured (`iw6 look:`, `iw6 probe:`, `iw6 material ...` lines of the load
log; the `ghosts_look_report` ignored test prints them without a GPU).

- **Live sun.** `fastfile_iw6::map::sun_is_baked` calls the sun baked only if
  the sunlit/shaded ratio is >= 2.5 and the sunlit level is >= 0.6 of the
  sun's light in the same units. Ghosts' pages hold sky and bounce only: on
  mp_frag sunlit 1.65 / shaded 0.35 (4.6) against a sun of 7.9 is not baked.
  `BO2MP_IW6_SUN_BAKED=1|0` forces either.
- **Shade level.** The look scale is target / shaded total (A + 0.8 B);
  Prison Break x4.55, mp_frag x3.19. `BO2MP_IW6_SHADE_BASIS=a` returns to the
  upper page. `map_sun_ratio` is anchored in the same units.
- **Probe.** `iw6_look::link_sky_probe` links a cube array as the map's one
  probe (the lit branch binds normal and specular maps only with one);
  `BO2MP_IW6_PROBE=0` is the A/B.
- **Materials.** `bo3_subst::run` classes a material by name words (and
  compounds), by the collision surface type of the same name, and rejects a
  class its colour map does not look like (`plausible`); the Black Ops III
  pick is the nearest after a darkening-only tint (`tint_toward`, floor 0.55),
  the tint is multiplied into the swapped surfaces' vertex colours. Switches:
  `BO2MP_BO3_MAT_SUBST`, `_CONF`, `_DIST`, `_ZONES`, `_BUDGET`, `_SURFACE`,
  `_CACHE`. The pool's statistics are cached per zone (`bo3_pool_cache`).
- **Limits seen.** Most Ghosts world materials are generated blends
  (`*100_132`, layered) and stay as they are; the swap works on plain
  materials. A material with no readable colour map (missing image pack)
  cannot be measured and stays.

## The exe's lighting data and the map's own tone mapping (2026-10-30)

**What `iw6mp64_ship.exe` gives (read-only: PE headers, `.rdata`, strings; its
`.text` / `.interpr` are Arxan-encrypted and were not unpacked, dumped, debugged
or bypassed).** Names and descriptions only. The dvar names (`r_lightGridIntensity`
at RVA 0x8a2980, `r_lightGridContrast` 0x8a29d0, `r_filmTweak*` 0x8a8018.., `r_hdrSkyIntensity`
0x84fbe8, `r_sun*` 0x8c7b80..) sit in `.rdata` as strings with their description
strings; the `CONST_SRC_CODE_TONE_MAPPING_*` names (TOE_STRENGTH_TOE_NUMERATOR ..
WHITE_ADAPTATION_RATES, RVA 0x8c2347..0x8c24cf) are shader-constant names. No
default value of any of them is stored as data: the dvar defaults are immediates in
the encrypted code. A scan of `.rdata` / `.data` for the Hable coefficient sets
(0.15 0.50 0.10 0.20 0.02 0.30, 0.22 0.30 0.10 0.20 0.01 0.30, 0.28 0.29 ...), for
11.2 / 6.2 / 2.51 / 2.43 as f32 or f64 found none; the isolated hits (0.15 at
0x828b48 in a sound table, 0.01 at 0x8d10b0 in a texture-size table, 0.3 / 0.59 /
0.11 at 0x8cc12c, which are the luminance weights the repo already uses) are
unrelated. **Nothing numeric about lighting is recoverable from the exe.**

**What the zone gives (and the look now reads).** `vision/<map>.vision` (text) names
a `toneMapping` asset that is in the zone (`mp_frag_tone`), and carries
`r_hdrSkyIntensity`, `r_litSurfaceHDRScalar`, `r_unlitSurfaceHDRScalar`, film tints,
desaturation, glow / material bloom, `colorizationSet`. `fastfile_iw6::walk` now keeps
each `ToneMapping` asset's 26 floats (`Capture::tone_mappings`). Read values:

| asset | D | E | F | B | C | A | white | exposure (3 floats) | rates |
|---|---|---|---|---|---|---|---|---|---|
| mp_frag_tone | 0.8 | -2 | 10 | 3 | 1 | 1 | 16 | 0.4 2.2 1.1 | 1 1 |
| mp_prisonbreak_tone_01 | 2.2 | 0.2 | 2 | 1.6 1.6 2.6 | 1.6 | 5 | 11 | 0.21 0.34 0.13 | 3 3 |
| mp_prisonbreak_indoor_tone | 1.05 | 0.95 | 2.25 | -2.2 | 0.1 | 5.5 | 0.9 | 1.65 1.65 1.0 | 0 0 |
| mpintro_tone (common_mp) | 7.6 | 0.05 | 0.55 | -1.7 | 0.2 | 4.9 | 2 | 1.1 1.1 1.0 | 0 0 |

`iw6_tone.rs`: Hable's curve `f(xe)/f(W)` then gamma 2.2. INFERRED: the group order (the
header layout's names are Hable's A..F) and the gamma; the support is that mp_frag's
numbers (sun 8, baked shade 0.432) put a mid-grey in the sun at 0.65 and Prison
Break's (sun 9, 0.247) at 0.75, with nothing tuned. Only the exposure's first float is
used (the other two are read, meaning unknown). The branch's own curve (`t7_tone`) is
unchanged: the map's curve sets two targets (a mid-grey in the sun and in the shade)
and `iw6_tone::solve` finds the sun and exposure that give the branch the same two
values. `BO2MP_IW6_TONEMAP=hable|bo3` (default **hable** since 2026-10-30 (the user turned it on; `bo3` is the earlier rule).
Without a tone asset: Hable's published defaults (A .15 B .5 C .1
D .2 E .02 F .3, white 11.2) with the exposure solved for 0.65 (documented as defaults).
Headless numbers (unit-tested; NOT seen on screen):

| map | native grey sun / shade | bo3 (default) | hable |
|---|---|---|---|
| mp_frag | 0.65 / 0.16 | sun x2.22, exposure x0.66, look check 0.65 / 0.28 | sun x4.00 (clamped), exposure x0.37, 0.65 / 0.28 |
| mp_prisonbreak | 0.75 / 0.13 | sun x2.62, exposure x0.57, 0.65 / 0.29 | sun x4.00 (clamped), exposure x0.66, 0.75 / 0.38 |

Ghosts' real shade is darker than the branch's ambient allows (the traced ambient is
0.4..0.6), so `hable` runs the sun to the top of its 1.07..3.55 range; sun over shade is
unchanged (7.4..7.7), which is why it stays opt-in until seen on screen. Every map
logs one `iw6 vision: ...` line. Read and **not applied**: `r_hdrSkyIntensity` (3 mp_frag,
7 Prison Break), `r_litSurfaceHDRScalar` (0.2 / 0.85; if it multiplies lit light before
the curve it would darken mp_frag by 5x, which contradicts the 0.65 above, so its
meaning is unknown), `r_unlitSurfaceHDRScalar`, film tints and desaturation (graded on
top of the branch's curve this double-grades; `BO2MP_IW6_VISION=1` still keeps the
old film vision), bloom, colorization sets. `cargo test -p assets --lib ghosts_dump_rawfiles
-- --ignored --nocapture` (`GHOSTS_TEST_MAP`, `GHOSTS_DUMP=<name part>`) lists a zone's rawfiles and tone assets.

## Reflection probes and the sky (2026-11-03)

`fastfile_iw6::probe` reads the probes the walk captures
(`GfxWorldRec::reflection_probes`, `docs/exe_notes/IW6_LIGHTING.md` section 8)
and decodes their cubes; `iw6_look::link_map_probes` links them as the map's
probe cube array when `BO2MP_IW6_OWN_PROBES=1` (default **off**; the sky probe
of `iw6_look::link_sky_probe` stays otherwise). Checked on mp_frag with a
scratch decoder outside the repo (faces dumped to PNG and looked at).

- **Cubes.** Inline, never streamed; six faces one after another, each with its
  whole mip chain largest first. The smaller levels are prefiltered (17..42 %
  from a 2x2 mean), so they are used as stored. Probe 0 (DXGI 28, RGBA8, at the
  world origin) is a flat 1.0 (a neutral default); the others DXGI 10
  (RGBA16F, linear; their means 0.01..0.31). The half-float alpha is 1 on the
  top level and 0.43..0.57 below on every probe (meaning unknown, not used).
  Faces are D3D's +X -X +Y -Y +Z -Z sampled with the world direction (z up):
  +Z the sky, -Z the snow, a side face's top row looks up on -Y and down on +Y.
- **Linked.** The commonest size (64 px on mp_frag), its fewest levels (7); a
  larger probe (probe 21, 256 px) gives its levels from 64 px down; a smaller
  one is not kept and its surfaces take the nearest kept probe. At most 32.
  Each probe is scaled to a mean luminance of one (as the sky probe and
  Infinite Warfare's: the lit shader re-lights a reflection by the light where
  it is seen), half floats with alpha 1. Table rows: gain 1, the origin, no
  parallax box.
- **Selection.** World surfaces (`GfxSurface` lighting fields) and the world's
  placed models (`GfxStaticModelDrawInst::reflectionProbeIndex`, values 1..21
  on mp_frag) carry their probe in the zone: used as stored. The engine's rule
  for anything else was not found; the stand-in (`probe::NearestSamples`, also
  installed as the runtime's picker for players, guns and moving models through
  `asset_world::install_t6_probe_picker`) takes the stored probe of the nearest
  placed model or world surface centre (surfaces with probe 0 left out). On
  mp_frag the nearest other placed model's index equals the model's own for
  8105 of 8779 models (92 %); the tightest containing volume, else the nearest
  origin (`ProbeSet::pick`, the fallback) for 4361 (50 %); the nearest origin
  alone 3910; the nearest reference position's probe 1125 (12 %), so the
  references are kept as data and not used.
- **Volumes.** Six planes `(n, d)`, inside when `n.p + d >= 0` (probe 8's box
  holds its origin under this sign only). Several volumes do not hold their own
  probe's origin; they mark where the probe serves. Not used for parallax.
- **Log.** `iw6 probe: the map's own N of M reflection probes ...` (or the sky
  probe line, with the reason the map's own were not used) and
  `iw6 probe placement: ...` once per map.
- **Sky.** Already the map's own: the `GfxSky` image (`sky_mp_frag_hdr`, BC6H
  1024 px cube, streamed) is the only image of the sky surfaces' `wc_sky_bl`
  material (no constants beyond `bloomParams`), linked by `Builder::sky_material`
  and drawn by the sky pass. It could not be decoded here (the image paks are not
  on the build box). The vision's `r_hdrSkyIntensity` (3 on mp_frag) is still
  not applied.
- **Needs a GPU.** Whether reflections look right with the switch on (metal,
  glass, wet ground, guns), whether the per-probe normalisation leaves dark
  rooms too bright in reflections, and the cube orientation on screen.

## The engine's lighting rules on the lane (2026-11-03)

What of `docs/exe_notes/IW6_LIGHTING.md` and `IW6_LIGHTGRID.md` the lane now
applies, each behind its own switch. Since 2026-11-03 (the owner's call) the grid,
per-channel tone map, iris and both fog rules are **on by default**, each turned off
with `=0`; the props switch stays off (nothing here has been seen on a GPU). Every map logs one `iw6 engine rules: <map>: ...` line saying what
was applied and naming the switch of each part.

| switch | default | what it does |
|---|---|---|
| `BO2MP_IW6_GRID` | on (`=0` off) | players, the first-person gun, entities and effects' models lit by the map's own light grid (else the placed models' light round them, as before) |
| `BO2MP_IW6_GRID_PROPS=1` | off | a placed model with no stored light takes the grid's at its origin (+16), the mean of the 56 directions (else its neighbours', as before). Off is the engine's rule: a placed model whose `groundLighting` is all zero is lit per vertex (`fastfile_iw6::map`), so those props need their baked vertex light, not the grid |
| `BO2MP_IW6_GRID_SCALE=<x>` | 1 | factor on the grid's light (otherwise the map's light scale, as its baked pages). Kept at 1: the grid's values times the map's light scale are the engine's own units; the 2x measured below is against the lane's neighbour rule (which carries a lane factor x0.34), not against the engine, so it is no ground for 0.5 |
| `BO2MP_IW6_TONE_RGB` | on (`=0` off) | the map's tone curve per colour channel, through the final pass's colour table |
| `BO2MP_IW6_IRIS` | on (`=0` off) | the map curve's exposure is where its iris settles (`Hable::iris_exposure`), not the range's first float |
| `BO2MP_IW6_FOG_HDR` | on (`=0` off) | the fog's HDR block, colours linear by the sRGB curve times `HDRColorIntensity` / `HDRSunColorIntensity` and the map's light scale |
| `BO2MP_IW6_FOG_LAW` | on (`=0` off) | the fog's opacity by the engine's law: transmittance max(1 - max opacity, e^(-k (d - start))) (the fog row's w at 2, read by the world shader's `fog_amount`), where Black Ops II's scales the fog by the opacity |

**Light grid (`assets::lane::iw6_grid`).** Black Ops II's rules light moving things
from a grid in Modern Warfare 2's layout (`OwnedLightGrid`); that grid's cells are the
same 32 x 32 x 64 units from the same origin as Ghosts', so each of its points is one
Ghosts cell (`LightGrid::cell`), and each colour set is one Ghosts palette entry's
56-direction table (`lightgrid::light_table`: the SH evaluated at the shell lattice's
directions, negative light none, `sqrt(light) * 127.5`, as the engine bakes
`MODEL_LIGHTING`). The box is the spawn points' area (1st to 99th percentile, 1024
units round, 192 under to 704 over), inside the tree's box. A point with no data takes
the nearest point with data in its column (the one above on a tie), else a
neighbouring column's, else the map's middle palette entry: Ghosts drops such corners
and renormalises, which a lattice point cannot. Ghosts' model shaders read those 56 values as a 4 x 4 x 4 volume, trilinearly, at
the normal (`IW6_SHADERS.md`), so the table is the engine's own form. The entries name
no primary light (Ghosts keeps the model's sun visibility in the grid's alpha, which
is not carried here),
and `t6::foreign_t6_lighting` reads the grid as Ghosts' (`(b / 127.5)^2` times the
map's light scale; not CoD4's legacy branch, whose sun row would land in the fog
table), keeps the sun term as it was without a grid, and keeps the placed models'
light outside the box. Not done: the engine's per-corner visibility trace (the lane
flags), the light-environment index (which primary lights reach a cell), hero
lights, the `r_lightGridIntensity` / `Contrast` tweaks (off in the engine too).

Headless numbers, mp_frag (`ghosts_look_report`, `iw6_grid_on_a_zone`; NOT seen on
screen): the box is 152 x 230 x 17 = 594,320 points (55,401 from their own cell,
183,398 from their column, 355,521 from a neighbouring column), 50,186 colour sets, 15 s
to lay out in an unoptimised build; read back by Black Ops II's sampler at 200 spawn
cells it gives each cell's own Ghosts table to 0.0006 of full scale. The models' stored
light over the grid's reading at their origins: median 1.94 (1,567 models). With
`BO2MP_IW6_GRID_PROPS=1` the 7,395 placements without light of their own come out
min 0.023 / p10 0.25 / median 1.76 / p90 5.3 / max 15.3 (the look's units), against
0.010 / 0.20 / 0.85 / 2.0 / 4.0 by their neighbours: the grid at the map's light scale is
about twice as bright as the neighbour rule (which carries the models' factor x0.34);
`BO2MP_IW6_GRID_SCALE=0.5` brings the medians together if the screen agrees (the
default stays 1, the engine's units). The props switch stays off in any case: those
placements are lit per vertex in Ghosts. The
moving things take the same scale. `BO2MP_IW6_FOG_HDR` (on) there takes the
`mp_frag_HDR` block (HDR intensities unset, so 1): its colour linear x3.19, about three
times the squared, unscaled colour drawn before.

**Tone map (`iw6_tone`).** `Hable::from_asset_channel` keeps each channel's six
parameters and white point. `channel_lut` builds the final pass's 32^3 table: a
channel's display value is taken back to its scene light under the one
luminance-averaged curve the look is matched to, then drawn under that channel's own
curve (identity when every channel is alike, as on mp_frag; Prison Break's blue linear
strength 2.6 against 1.6 moves blue only). With `BO2MP_IW6_VISION=1` the film vision
follows it in the same table. Iris: Ghosts' own iris shader (`docs/exe_notes/IW6_SHADERS.md`, branch
`iw6-shaders`) moves the exposure toward key / mean luminance at a linear rate (the
second rate up, the first down), clamped to [min, max]; the exposure floats are (min,
max, key). `iris_exposure` is where those steps come to rest for a frame whose mean
luminance is a mid-grey in the map's sun: `settled_exposure(0.35 (sun + shade), key)`,
the middle of the range when the map does not adapt; a curve with no asset keeps its
solved exposure. mp_frag (key 1.1) and Prison Break (key 0.13) both settle at their
floor, the first float used before, so the switch changes nothing there. Not
reproduced: the per-frame steps (no runtime meter of the frame here).

**Fog.** The distance term already was the engine's (visibility halves every halfway
distance past the start: the shader's `2^-((d - start) / half)`, k = ln 2 / half; the
sun-fog blend linear between the cosines of its two angles). The opacity was not:
Ghosts holds the transmittance at 1 - max opacity (`max`), Black Ops II's shader
mixes by fog x opacity, so nearer fog drew thinner (opacity 0.5, e^-k(d-s) 0.8:
Ghosts keeps 0.8 of the scene, the branch 0.9); `BO2MP_IW6_FOG_LAW` (on; `=0` the old one) draws Ghosts'
law. `BO2MP_IW6_FOG_HDR` changes the colours (see table). Ghosts measures the
distance as depth, the branch along the ray (unchanged). Not done (shader
side): the sun fog's own density scale (`normalFogScale`, k * scale toward the sun),
sky fog, the view model's depth hack.

**Film.** Ghosts' film constants (bias `b + 0.5 - c/2`, contrast over `1 - blacklevel`,
light/medium/dark tints as a quadratic Bezier, desaturation from dark to full) are the
ones `asset_world::iw_film_colour` already draws (black level 0), reading the
Bezier's parameter as 1 - luminance (the shader's own parameter is zone data). New:
a key left at the engine's unset value (-1e8) takes the engine's value (medium tint
the mean of light and dark, dark desaturation the desaturation). The film stays out
under Black Ops III's look unless `BO2MP_IW6_VISION=1`.
