# Guest guns: cache and on-demand loading

Written 7 Nov 2026 from the owner's logs (RTX 3050 Laptop, 3.9 GB, Horizon MW,
Ghosts and World at War installed). Stage 1, the finished-gun cache, is in
for the lanes whose whole job is those guns. Stage 2, building a gun when
it is equipped, is not started. Nothing here has been timed on a match
start, and nothing has been seen on a GPU.

## What a match start costs now

Measured in `fps-2.log` (Horizon MW guns, before the stand-in change):

| step | time | state |
|---|---|---|
| stand-in picture lookup, 3 zones x (5 retail + 14 guest zones) | 3 x ~20 s | one lookup for all zones (`load_hmw_guests`), saved between runs (`h1_standins.cache`) |
| building the guns of a zone (models, materials, 199-307 pictures) | ~2 s a zone, 12-18 guns | unchanged: ~0.13 s a gun |
| first-use kill icons | 6.5 s | read on a thread beside the menu |
| the zones walked twice (the lookup's pass, then the build) | not measured | open |

So once the lookup is saved, a library of 128 guns costs about 128 x 0.13 s =
17 s of building at each match start, plus the memory of all of them
(`gun_texture_cap` now shrinks with the number of games read: 1024 px for two
games, 512 for three or more).

## Stage 1: a cache of finished guns

Implemented for the loaders that return finished guns and nothing else the
match still has to build from that same read.

| lane | where | file |
|---|---|---|
| Horizon MW (`h1`) | `load_hmw_guests` | `<bake>\h1\guns\finished_guns.pak` |
| Ghosts (`iw6`) | `load_iw6_guests` | `<bake>\iw6\<zone>\finished_guns.pak` |
| Advanced Warfare (`s1`) | `load_s1_guests` | `<bake>\s1\guns\finished_guns.pak` |
| Black Ops III and its workshop packs (`t7`) | `load_t7_guests`, one file per zone | `<bake>\t7\<zone>\finished_guns.pak` |
| World at War (`t4`) and Call of Duty 4 (`iw3`) | `build_legacy_guests` | `<bake>\<game>\common_mp\finished_guns.pak` |
| Infinite Warfare (`iw7` feature) | `load_iw7_guests` | `<bake>\iw7\guns\finished_guns.pak` |
| Black Ops II native (`t6`) | `load_t6_combat` | `<bake>\t6\<map>\finished_guns.pak` |
| Black Ops (`t5`), MW2 2009 (`iw4`), MW3 2011 (`iw5`) | `ZoneLane::load_common_mp` | `<bake>\<game>\<zone>\finished_guns.pak` |
| Modern Warfare III 2023 (`jup` feature) | `build_jup_guest_guns` | `<bake>\jup\guns\finished_guns.pak` |

`<bake>` is `CODLEGACY_BAKE_DIR`, else `D:\codlegacy-bake` when that drive
exists (`GUEST_BAKE.md`). `CODLEGACY_BAKE=0` does not read or write the
file. CLEAR CACHE deletes `finished_guns.pak`.

**Off by default since 7 Nov 2026** (`CODLEGACY_GUNS_CACHE=1` turns it on).
Measured on the owner's PC: a fresh bake folder's first loads wrote the file
on the load's own thread, 0.5 to 4.5 GB a lane (BO1 Nuked's first load
244 s, MW2's common set alone 4.5 GB, Black Ops II Stronghold 887 MB packed
in 1.7 s), while building the guns takes 2 to 6 s. Until the files are
small and written behind the load the cache costs more than it saves, so
nothing is read or written unless the switch is on. The parked work
(writing behind the load, baking the rule set ahead) is in a git stash on
the `lane/fable-load-cache` worktree, not on main.

The file holds the lane's `WeaponBuild` (mesh and animation edges filled
again on read), first-person and world meshes, projectile meshes, animations,
effects, effect models, tracers, the impact table when the lane has one, the
materials and decoded pictures the lane added, and its report lines. Horizon
MW also keeps `HmwGunReport`. The byte layout is version 1 (`BO2FGUN`); an
older or cut-short file is dropped and the guns are built again. The key is
every source file's path, size and time written, the picture caps the lane
used (`fp`, and `world` / `fx` / `camo` / `prop` where that lane has them),
and the text of the converting code, so a changed zone or a changed builder
does not reuse the old file.

A material that still carries a technique table, a draw route, or state-bit
rows is not written: those indexes belong to the catalog the walk was in,
and a wrong index would be a wrong gun. The lanes above link gun materials
as a Black Ops II draw plus decoded pictures, which is what the file stores.
If a future change puts a technique table on one of them, the log says
`not cached` and that start builds the guns as before.

A hit logs, for each zone name the lane recorded:

```
h1 guest guns: hmw_ar1 reads D:\codlegacy-bake\h1\guns\finished_guns.pak
```

The first start logs `kept in` that path. A file whose key is not this
build's logs `cache is stale, built again`. A file that does not read back
logs `cache unreadable, built again` and is deleted.

Ghosts, Advanced Warfare and Black Ops III still inflate their zones in the
prefetch before the build looks at the file, so a hit skips the conversion,
not that inflate. Horizon MW's walk is the build, so a hit skips both.
Ghosts' penetration table is still taken on that prefetch.

The file is the whole product of the loader, not a list of weapon names.
A gun added to a zone the loader already reads (a workshop pack, a mod
zone, a new `*.ff` in the zone folder, a new row in `common_mp`) changes
that file's path, size or time, or the directory listing the key walks,
and the next start builds again. A gun added in the loader's source is
covered because the key includes that source: the loader enumerates the
guns it knows, and the cache stores every row it built.

Call of Duty 4's guns are left out of a default match (`LEFT_OUT`); the
loader caches them when a match does ask. Infinite Warfare and Modern
Warfare III (2023) are off in the default build (`dropped`). Those stubs
build no guns, so they do not write a file. `iw7` and `jup` turn the real
loaders on, and those loaders cache. `jup_guest_rows` still runs on every
start: each row is a copy of a Black Ops II donor, and that donor may
itself have come from the combat cache.

Black Ops II's `load_common_mp` is still the stub that does not read the
zone. It returns `caches_common_weapons() == false` and writes nothing.
The guns are `load_t6_combat`, which caches the guns together with the
scripts, HUD pictures, sounds, footsteps and visions that walk produces.
A hit registers the stored visions again. `PendingImages` on the common
walks is not stored: a hit builds a new image plan from the restored
materials, so the plan is still a task of this load. The sound claim is
filled from the catalog the walk deposited.

## Adding a game

A new `ZoneGame` has to implement `walk_common_mp`. `load_common_mp` is the
cache. `caches_common_weapons` defaults to true; return false only for a
stub that reads no guns, and say so next to the stub. A guest-gun loader
calls `finished_gun_cache::load_or_build` (or `read_body` when it must hand
back something else, as Black Ops III does with its pack set) and lists
every file it opens, including files that are not there yet
(`missing_ok`) when a later file should drop the cache. Do not register
weapon names with the cache. `gun_loaders_go_through_the_shared_cache` and
`every_zone_lane_declares_whether_common_weapons_are_cached` fail if a
current loader steps around this. A new file has to be added to that test.

Check, on the owner's PC, not done here: two match starts in a row with the
same game folders. The second logs `reads ... finished_guns.pak`. The guns
look and fire as built fresh (compare the `HmwGunReport` lines, and the
same report lines for the other lanes that were installed). No time and no
screenshot from that pair is in this change.

## Stage 2: guns built when equipped

At match start only the guns of the classes build (primary and secondary of
each class, plus the knife and the dropped-weapon shapes). Any other gun is
built when a class takes it in the game's editor (`cac::choose`), on a worker
thread, and its slot waits until it is ready (the editor shows the gun
greyed with a short note until then).

Needs: `build_zone` split into a per-gun step (today it builds a whole zone),
a way to add a gun to the running match's catalogs (`WeaponCatalog`,
`FpvMeshBuild`, `XAnimBuild` and the renderers' views of them are built once
at load), and the editor's `LIVE_GAMES` rule (`bo2_profile::guest::
begin_match`) replaced by a per-gun "ready" set.

The renderers are the hard part: a gun added after the first frame must reach
the GPU's model and picture tables without a restart.

## Order

1. Stage 1 for the lanes in the table above. Done in code; the two-match
   check is still the owner's.
2. Stage 2 behind `BO2MP_STREAM_GUNS=1` until it holds up on the owner's PC.
   Not started.

Stage 1 was checked with `cargo test --lib` for `assets` (169 passed, 22
ignored, including the cache tests for a new source file, the loader
contract and the zone-lane policy), `fastfile_h1` (44 passed, 6 ignored),
`ui` (154 passed, 2 ignored), `asset_game` (54), `asset_core` (16),
`asset_anim` (1 passed, 1 ignored), `asset_audio`, `asset_t6`,
`xmodel_runtime` (5) and `weapon_iw4` (36). `cargo check -p assets --features
jup` and `--features iw7` compiled those loaders. It still needs the owner's
log of two matches in a row. No match was started here, and nothing was
seen on a GPU.

## 9 Nov 2026

* "Load only the games the classes hold" already exists (`bo2_profile::guest::
  match_games`, set from the saved classes; `BO2MP_GUEST_GAMES` forces a set).
  A log that loads every game's lane means the classes hold guns of every game.
* The Horizon MW stand-in lookup walked every weapon zone once on its own and
  again for the build. The first pass now runs the zones side by side, one
  thread each (log line `zones walked for stand-ins side by side`). Untested:
  compare that line and the `h1_guest_weapons` stage time with the last log.
* Not done: Stage 2, and running the lanes side by side (they share one
  growing `MaterialCatalog`, so their material numbers cannot be made apart).
