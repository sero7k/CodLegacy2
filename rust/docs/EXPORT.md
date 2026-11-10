# Portable export

One folder that holds `CodLegacy.exe`, its settings and trimmed copies of every
game's files the rebuild reads, so the game runs with none of the original game
folders present. It is the owner's personal copy: it holds files from his own
installs, and its `README.txt` says it is not to be shared. Nothing of it is
ever committed or published (`CONTRIBUTING.md`, Game data).

```text
tools/export/Export BO2 Rebuild (dry run).bat   plan and sizes only, writes nothing
tools/export/Export BO2 Rebuild.bat             export, then verify
tools/export/export-portable.ps1 [-Destination X:\BO2_Rebuild] [-NoBuild] [-SkipVerify]
    [-VerifyMaps] [-VerifyOnly] [-Only bo2,mw2] [-Skip iw] [-IncludeCache] [-RedoJup]
    [-PlayDir ...] [-GamePaths ...\game_paths.cfg] [-RepoDir ...] [-BuildProfile play]
tools/export/rules.json                         what is kept, per game
```

## What it does

1. **Sources**: the `game_paths.cfg` and `.env` of the play folder
   (`%USERPROFILE%\bo2\play` unless `-PlayDir`). The mods are taken as the game
   resolves them today: `bo2mod` from its line, else `CODLEGACY_T6_MOD`, else
   `E:\mp_leetin_pub` when it exists; `bo2extra` from its line, else
   `CODLEGACY_T6_EXTRA`. A mod the game does not use now is not exported.
2. **Destination**: an earlier export (`BO2_Rebuild` at a drive's root or in
   Documents, with `CodLegacy.exe` in it) is updated in place; otherwise
   `BO2_Rebuild` on the local drive with the most free space, or
   `Documents\BO2_Rebuild` when that drive is the system drive. Refused when it
   is inside a game folder or holds one.
3. **Plan**: every file of each game folder is matched against `rules.json`;
   the table gives kept, skipped and still-to-copy sizes per game, and the free
   space. `-DryRun` stops here.
4. **The game**: `cargo build -p launcher --profile play` in the repo when it and
   `cargo` are there (`-NoBuild` skips it), then the newest `CodLegacy.exe` of
   `target\<profile>`, `target\play`, `target\release` and the play folder is
   copied, with `LICENSE`, `NOTICE` and the font licences.
5. **Copy**: `robocopy` per folder with the kept file names (`/J`, retries,
   timestamps kept), so a second run copies only what changed. Nothing is ever
   deleted, at the destination either: files a later rule no longer keeps are
   listed as a warning.
6. **Modern Warfare III (2023) and II (2022)**: `CodLegacy.exe export-mw3
   <export>\games\jup` (`ui::jup_pack`), the game's own small copy: every
   played gun read as a match reads it while the package entries used are
   noted, the gun sounds, Create-a-Class pictures, calling cards and emblems,
   then the zones and only those package entries written. No maps, Warzone or
   operators. `Documents\MW3_export` (`CodLegacy export-jup`) is copied too when
   present.
7. **Settings**: `game_paths.cfg` with `games\<key>` for every game exported and
   an empty line for the rest (so the first launch asks for nothing); `.env`
   with the play folder's settings minus every one that names a folder, plus
   `CODLEGACY_GAMES=games/bo2`, `CODLEGACY_NO_DETECT=1` (no scan of the PC for games),
   `CODLEGACY_BO3_WORKSHOP=0`, `CODLEGACY_T6_MOD` (the export's mod, or `off`),
   `BO2_JUP_EXPORT=MW3_export`; `music.cfg` and `music\`, `banners\` (Cold War's
   and Vanguard's `banner.png` become `banners\cw.png`/`vg.png`), `profile\`
   (from beside the play folder: `diag::profile_dir`), `codlegacy-artifacts\` without
   logs, screenshots, clips, dumps and the cache; `games\call_of_claude.txt`;
   `Play BO2 Rebuild.bat` (clears the folder settings of the environment, then
   `CodLegacy.exe frontend t6mp`); `README.txt`.
8. **Verify** (below) and `export-report.txt` (appended each run).

**Relative paths work.** `game_paths.cfg` and `.env` name `games\...` relative
to the export: `launcher::prepare_process_root` makes the exe's folder the
working directory on Windows before anything is read, `game_paths::config_path`
and `load_dotenv` look beside the exe, and every reader opens the path as given.
So the folder can be moved or copied whole. `dotenvy` never overrides a
variable already set, which is why `Play BO2 Rebuild.bat` clears them first.

## What is kept (rules.json)

The base is `call_of_claude::rules` (what each game's loaders open); the export
adds the owner's cuts. A file is kept when it matches `keep` and either matches
`force` or neither `drop`, `common_drop` (videos, installers, other languages)
nor a map zone. A map is a stem with a load zone (`map_marker`,
`zone/mp_*_load.ff`); `map_zones` are its zone, load, path, patch and language
zones.

| game | kept | left out |
|---|---|---|
| bo2 | `zone/all`, `zone/english` zones and image packs, `*.all`/`*.english` sound banks (multiplayer **and** BO2 Zombies) | videos, redist |
| bo1, mw2, mw3, waw, cod4 | multiplayer zones and maps, every IWD (WaW's gun sounds stream from them; the DLC5 pack borrows BO1's) | campaign, Zombies zones, videos |
| ghosts | multiplayer zones and maps, every image and sound pack | Extinction, videos |
| iw | menus, maps, Zombies (`cp_*`), shared zones, asset and image packs | all sound (never read), videos |
| aw | common zones, gun view/world zones (`mp_vm_*`, `mp_wm_*`), character parts, every pack | **every map** (zone, load, path), Exo Zombies |
| mwr | `zone/*.ff` but maps, every `imagefile*.pak`, Horizon MW's `hmw-mod/zone` (its guns and their sounds) | **every map**, `hmw-usermaps`, `usermaps` |
| bo3 | `core_*`, `mp_*` (maps, `mp_common`), `zm_common`/`zm_levelcommon` (shared zones the map loader looks in), their `en_` zones, every `.xpak` but Zombies' and campaign's, `mp_common` sound banks | **every Zombies map** (`zm_*` zones, packages, banks), campaign |
| jup | the game's small copy (`export-mw3`) | **every map**, Warzone, operators |
| cw, vg | `banner.png` only (they are banner-only: `game_paths::BANNER_ONLY_KEYS`) | the rest |
| bo2mod | `mod.ff`, `mod.json`, IWDs, sound banks | |
| bo2extra | everything | |

Not exported: `bf1` (no rules; not a game the export carries), keys the game no
longer has (`iw8`).

**Why AW and MWR shrink so little.** Their pictures stream from
`imagefile<N>.pak`, shared by maps and guns, and which pack a picture is in is
only known by reading the zone that names it. All packs are kept (about 43 GB
for AW and 44 GB for MWR); only the map zones go. Trimming the packs needs a
tool that walks every kept zone and lists the pack indices its images name
(`fastfile_iw6/examples/iw6_pak_need.rs` does it for Ghosts maps), then copies
only those packs: not done yet.

## Verify

Run after every export unless `-SkipVerify`; `-VerifyOnly` runs it alone on an
existing export. Every run the script starts has the game's settings
(`CODLEGACY_*`, `BO2_*`, `BO2MP_*`, `BF1_*`, `T6_*`, `T7_*`, `H1_*`, `S1_*`,
`IW6_*`, `JUP_*`) cleared from its environment.

1. `game_paths.cfg` and `.env` name no folder outside the export.
2. A copy of `CodLegacy.exe` in a new `verify\<time>\isolated` folder, with absolute
   settings into the export, an empty cache (so every gun is read from the
   export's files, not from a cache made from the originals), `USERPROFILE` and
   `HOME` pointed at an empty folder (nothing in Documents can be found) runs
   `CodLegacy.exe check-guns`: every guest game's guns read as a match reads them.
3. The export's own `CodLegacy.exe check-guns` (its relative settings).
4. With `-VerifyMaps`, one map per kept game (`verify_maps` in `rules.json`)
   from the isolated copy: `CodLegacy map <zone> --cmds quit`, window hidden.
5. Every drive path the runs printed or logged is checked: one outside the
   export (other than Windows, temp, ProgramData, GPU driver and cargo folders)
   fails the verify and is listed.
6. `check-guns` on the original folders (`verify\baseline`): each game's
   "guest guns OK" count in the export must be at least the originals'.

The original folders are never renamed or touched: they are unreachable to the
verify runs because nothing the runs read names them.

## Sizes measured on the owner's PC (2026-10-29)

From listings of `Documents\*_game_files` and `MW3\file_list.txt` with these
rules (the dry run gives the real numbers):

| game | install | export | note |
|---|---|---|---|
| bo2 | 14.6 GB | 14.1 GB | |
| bo1 | — | ~8.6 GB | |
| mw2 | 17.0 GB | 11.0 GB | |
| mw3 | 18.5 GB | 12.4 GB | |
| waw | — | ~5.6 GB | |
| cod4 | — | ~4.1 GB | |
| ghosts | 41.6 GB | 38.5 GB | |
| iw | 84.6 GB | 53.6 GB | sound left out (31 GB) |
| aw | ≥46 GB | ~44 GB | packs kept; maps' zones ~2 GB |
| mwr | ≥52 GB | ~50 GB | packs kept; maps' zones ≥5 GB, plus `hmw-usermaps` |
| jup | ~91 GB packages + zones | a few GB | small copy |
| bo3 | not measured | not measured | Steam folder not connected to the session |

About 245 GB without Black Ops III.
