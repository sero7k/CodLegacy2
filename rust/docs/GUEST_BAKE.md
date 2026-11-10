# Guest map bake (BO1, MW2, MW3, CoD4, WaW)

A guest map's first load decodes its pictures and makes its shader programs.
The bake writes both to disk once, so every later load, the first one
included, reads one file per kind. The bake stays local and is never shipped.
Gameplay data (stats, hitboxes, collision) is never baked.

## Where it goes

`CODLEGACY_BAKE_DIR`, else `D:\codlegacy-bake`, then `<game>\<map>\`
(one game can have a root of its own, `CODLEGACY_BAKE_DIR_<GAME>` such as
`CODLEGACY_BAKE_DIR_T7=C:\codlegacy-bake\t7`, with its maps straight under
it; the owner allowed Black Ops III's files on the solid-state drive on
7 Nov 2026, nothing else):

| file | what |
|---|---|
| `mips.pak` + `mips.idx` | the map's own pictures (the common set is readied first, so it is not copied in) |
| `programs.pak` + `programs.idx` | the map's shader programs (CPU work, no card) |

Every entry in a pak is packed with lz4 (format 2, 7 Nov 2026; a pak from
format 1 is a miss and is baked again). The hard drive's read of the pak
was the whole wait of a warm load, and the entries pack well: mip chains
to about two thirds, programs to a fifteenth (MW2 Terminal: 230 MB to
16 MB). The loading threads unpack what they take; the one reading thread
stays on the disk. The `kept in` line says both sizes (`.. MB, .. MB
packed`). Black Ops II's own bundle (`t6pictures.pak`) is not packed: its
pictures are the game's own LZO blocks, which lz4 does not shrink
(base.ipak sampled at 97%).

Game folder codes: BO1 `t5`, MW2 `iw4`, MW3 `iw5`, CoD4 `iw3`, WaW `t4`.
BO3 (`t7`) has its own bake (`render_t7::bake_ahead`).

## Three ways a map gets baked

1. **Its first load in the game.** The loading screen shows the bake row
   (game, map, step, counts, why). Cancel throws the half-written pak away.
2. **Options > BAKE ALL.** Bakes every map of every game set in GAME FOLDERS,
   one after another, on its own thread.
3. **Offline, from the command line.** No game and no card:

```
cargo run -p render_frontend --release --example bake_all -- bo1 D:\COD\bo1_game_files\zone\Common\mp_nuked.ff
cargo run -p render_frontend --release --example bake_all -- mw3 D:\COD\mw3_game_files\zone\dlc\mp_terminal_cls.ff
cargo run -p render_frontend --release --example bake_all            # every game in GAME FOLDERS
```

`assets`' own `bake_all` example runs the same loop without the renderer, so
a guest map gets its pictures only (its line says "pictures only"). Use the
`render_frontend` one for a guest map.

A Black Ops III map bakes its pictures (`render_t7::bake_ahead`) and then
the files the Black Ops II rules write on a first load: the BO2 guns and
camos (`t6\<map>\finished_guns.pak`) and the sun map (a section of
`t7\<map>\map.pak`), by a bake-only load of the map on the game's stack
(`bake_guest::bake_rules_ahead`; the line says "<map> guns in 31.2s"). A map
whose guns file is there is "guns already baked": a stale one is rebuilt by
the next load, not by BAKE ALL.

Pass map paths exactly as they are in the install, with backslashes and the
real folder names: the bake's key is the path, plus the file's size and date.

## Off switches

`CODLEGACY_BAKE=0` turns all baking off, including the finished-gun files
(`GUN_STREAMING.md`, `finished_guns.pak` in the same folders). `CODLEGACY_BAKE_MIPS=0` turns off
the pictures only, and `CODLEGACY_BAKE_PROGRAMS=0` the programs only.
CoD4 and WaW maps make no shader programs, so they bake pictures only;
`CODLEGACY_BAKE_LEGACY_PROGRAMS=1` tries their programs too (the old way).

## Log lines

Offline, first run (one map):

```
1 maps
[1/1] mp_nuked in Ts (common set made|reused Cs, load Ls, P programs); bake mips: N entries (M MB, P MB packed) kept in D:\codlegacy-bake\t5\mp_nuked\mips.pak (Ws); bake programs: ...
Baked 1 maps. (Ts)
```

Offline, second run: `[1/1] mp_nuked already baked`.

In the game, a map with no good bake (`World` lines in the log):

```
bake mips: t5:mp_nuked done in ..s, 0 entries from the pak, N not, waited ..s; baking N entries: kept
bake mips: N entries (.. MB, .. MB packed) kept in D:\codlegacy-bake\t5\mp_nuked\mips.pak (..s)
```

In the game, a baked map (the check to look for):

```
bake mips: t5:mp_nuked reads D:\codlegacy-bake\t5\mp_nuked\mips.pak
bake mips: t5:mp_nuked done in ..s, N entries from the pak, 0 not, waited 0.00s; pak read .. MB in ..s
```

Each line also comes as a `bake programs:` line.

- **Pass:** a baked load says `reads ...pak`, a few entries at most are "not" from the pak, it waited near 0 s, and the bake row does not show.
- **Fail:**
  - `too many misses, it is baked again on the next load`: the entries changed.
  - `the disk fell behind, this load's bake is given up`: the bake was dropped.
  - `baking ... dropped`: the load was cancelled.
