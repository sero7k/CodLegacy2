# Map converter: one finished file per map

Plan written 7 Nov 2026 (lane `lane/fable-load-cache`). The owner's call: stop
shaving seconds off every load and convert each map ONCE, offline, into a file
in our engine's own layout. A load then reads that file and nothing else: no
zone inflate, no walk, no collision build, no light bake, no picture decode.
This is how ZoneTool works for the real games (dump a zone to loose files,
list what a zone holds, build the zone), and the converter copies that
workflow, not its code (ZoneTool is GPL-3; nothing of it is pasted in here).

## Why: what a BO3 map load spends today (Skyjacked, 25.9 s first load)

| stage | first load | second load today | after conversion |
|---|---|---|---|
| zone inflate + asset walk | 3.3 s | 3.3 s | 0 (not opened) |
| collision + spawns | 0.4-0.6 s | same | read (~0.1 s) |
| light: blockers + sun map | 3.9 s | 0.3 s (`sunmap.pak`, this branch) | read |
| materials + pictures | 4.0 s | ~1 s (`pictures.pak`) | read |
| BO2 rules guns | 5.8 s | ~1.5 s (`finished_guns.pak`, camo gap) | read |
| world mesh + static models | ~2.1 s | same | read |
| fx fallback, props | 1.9 s | same | read |
| preflight (first map per launch) | 7.1 s | 0 on the 2nd map | unchanged: per launch, not per map |
| pipelines + uploads, admission | ~4 s | same | unchanged: the card's work |

Conversion removes the per-map work (about 11-12 s of the 25.9 s). The
per-launch work (preflight, pipelines, admission, ~11 s on the first map of a
launch) is a separate job and stays in the table so nobody expects a 3 s
Skyjacked from this alone: a converted Skyjacked lands near 10-13 s on the
first map of a launch and near 5-6 s on the second.

## The three commands (ZoneTool's names, our tool)

The tool is the `convert` example of `assets` (offline, no card) plus the same
loop behind Options > BAKE ALL in the game. ZoneTool's commands map one to one:

| ZoneTool | ours | what it does |
|---|---|---|
| `dumpzone <zone>` | `convert dump <game> <zone.ff>` | reads the zone with the engine's own reader and writes every asset we use to `<bake root>/<game>/<map>/dump/<type>/<name>.<ext>` (loose, inspectable, one file per asset) |
| `zone_source/<zone>.csv` | `<bake root>/<game>/<map>/map.csv` | one line per asset, `type,name`, written by `dump`; `build` reads it. `#` comments, `ignore,<other zone>` for assets that live in a shared zone |
| `buildzone <zone>` | `convert build <game> <map>` | reads the dump + csv and writes the ONE finished file `<bake root>/<game>/<map>/map.pak` |
| `loadzone <zone>` | the game's load | opens `map.pak`; when its key matches the installed zone, nothing else is read |
| `verifyzone <zone>` | `convert verify <game> <map>` | reads `map.pak` back, checks every section's key and size, prints the table |

`convert all` = dump + build for every map of every game in GAME FOLDERS (the
BAKE ALL loop, `map_bake::ahead`). A map that is already converted and whose
zone has not changed is skipped ("already converted").

### What ZoneTool actually does (read from the Joelrau x64 fork, 7 Nov)

That fork (D:\references\zonetool-joelrau, GPL-3, read for its workflow only)
is a dump-only DLL that runs inside the real game: `dumpzone <zone>` loads the
zone with the game's own loader, hooks every asset as it is added, and writes
one file per asset under `dump\<fastfile>\<type>\<name>.<ext>` (xmodel
`xmodel\<name>.xmb`, images `images\<name>.h1Image`, materials
`materials\<name>.json`, fx `effects\<name>.fxe` + `.json`, entities
`<map>.ents` + `.ents.spawnList.json`, collision `<map>.colmap`, the world
`<map>.gfxmap`, raw files under their own name) plus a manifest
`dump\<fastfile>\<fastfile>.csv` with one `type,name` line per asset in load
order. Cross-zone references (names starting with `,`) are listed in the CSV
and dumped after everything else. Building the zone back is a separate tool
(x64-zt; upstream source D:\references\zonetool-upstream). Its
`zone_source\<zone>.csv` syntax, which `map.csv` copies: `#` or `//` comment
lines, blank lines skipped, `require,<zone>` (that zone's assets are known and
referenced, not written again), `include,<csv>` (another listing pasted in),
`map,<name>` (every asset a map needs), `iterate,<type>,<folder>` (every file
in a folder added as that type), `reference,true|false` (following lines are
references), `localize,<key>,<text>`, and otherwise `type,name` for one asset. Our `convert dump` copies exactly that: the engine's own reader
instead of the game, one file per asset by type folder, and `map.csv` in load
order; our `convert build` is the x64-zt half, writing `map.pak`.

## The finished file (`map.pak`)

One file, sections one after another, each the layout `sunmap.pak` already has
on this branch, so `sunmap.pak` becomes its first section unchanged:

```
file   = MAGIC "BO2MAP\0\0" + format u32 + key u64 + section count u32 + directory + sections
dir    = per section: tag [u8;8], offset u64, packed len u64, raw len u64, key u64
section= lz4 body (lz4_flex::compress_prepend_size)
```

- The file key = zone file name + size + time, the shared zones it draws from
  (mp_common on BO3), the converter's format, the source files whose layout the
  sections depend on. A wrong key = the file is ignored and the map converts again.
- Each section has its own key so one stale section (a changed light rule) is
  rebuilt alone, not the whole file.
- Sections are read on demand (the directory is small; a section is one
  `read_exact` + lz4). No section is a `serde` dump of a Rust struct: each is a
  written layout with a reader, like `sunmap.pak`, so a struct change never
  silently changes the file.

Section tags, in build order (first ones first):

| tag | holds | replaces |
|---|---|---|
| `SUNMAP` | the baked sun shadow map | light bake (done: `t7_light_cache.rs`) |
| `CLIP` | collision brushes/slabs, trigger slabs, mins/maxs | collision build |
| `ENTS` | entity string, spawns, brush models + places, trigger models | entity parse |
| `WORLD` | vertices, indices, surfaces, material indices | zone walk of the world |
| `MODELS` | static model meshes + placements, sky models | model walk |
| `MATS` | materials: names, light rule, picture refs, bo3 look | material walk |
| `PICS` | the map's pictures (the `pictures.pak` body) | picture decode |
| `FX` | fx fallback table | fx walk |
| `PROPS` | scene props | prop walk |
| `GUNS` | BO2 rules guns for this map (the `finished_guns.pak` body) | gun build |
| `NAV` | nav mesh / compass | nav build |

Gameplay rule: `CLIP` and `ENTS` are written from the same code that builds
them at load and are checked back against a fresh build in `verify` (a stale
collision file means falling through the map, so these two sections also carry
the zone's size and time inside their own key).

## Where it lives

- `crates/assets/src/lane/map_file.rs`: the container (write, read, directory,
  keys), game-agnostic. Tests: round trip, wrong key, damaged section, a
  section added later still reads the old ones.
- `crates/assets/src/lane/t7_convert.rs`: BO3 dump + build + verify from a
  loaded `T7Map` (the sections above), and the load-side `open` that fills a
  `T7Map` from the file with no zone open.
- `crates/assets/examples/convert.rs`: the command line (`dump`, `build`,
  `verify`, `all`), the same loop BAKE ALL runs.
- Other games (BO1 `t5`, MW2 `iw4`, MW3 `iw5`, Ghosts `iw6`, MWR `h1`, MW2019
  `jup`) each get a `<game>_convert.rs` writing the same tags from their own
  loaded map.

Off switch: `CODLEGACY_CONVERT=0` (load from the zone the old way);
`CODLEGACY_BAKE=0` turns everything off as before.

## Plan and ETA (one engineer, dates Pacific)

| when | step | done means |
|---|---|---|
| Sat 7 Nov | plan, container (`map_file.rs`) + tests, `SUNMAP` moved in | `cargo test -p assets` green; `map.pak` written on a Skyjacked load |
| Sun 8 Nov | `CLIP`, `ENTS`, `WORLD`, `MODELS` written and read back; `convert dump/build/verify` | Skyjacked loads with the zone never opened; measured load ~12-14 s first map of a launch |
| Mon 9 Nov | `MATS`, `PICS`, `FX`, `PROPS`, `GUNS` in; `convert all`; stale keys; BAKE ALL = convert | Skyjacked ~10-13 s; Nuketown X, Stronghold, City converted and measured |
| Tue 10 Nov | the per-launch work: preflight (7.1 s) and pipeline warm from the file | first map of a launch ~5-7 s |
| Wed 11 - Fri 13 Nov | BO1, MW2, MW3, then Ghosts/MWR/MW2019 through the same container, one a day | every game's maps convert once and load from `map.pak` |

So: BO3 maps loading from one file by Monday night 9 Nov; every game by
Friday 13 Nov. Biggest risk is the collision/entity sections (correctness, so
`verify` and a playtest per map), and BO3's shared zone `mp_common`
(184 MiB, 20837 assets) which every map still needs until it is converted too.
