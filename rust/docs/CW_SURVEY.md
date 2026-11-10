# Black Ops Cold War install: storage survey (read-only)

Source: the owner's own Battle.net install, `C:\Program Files (x86)\Call of Duty Black Ops Cold War`,
read through the device bridge (staging) only. Nothing was written to the PC.
Survey date 2026-10-28. Status: **CASC layer fully read (all 6,415 paths, no encryption at the CASC
level). The game's own containers (.ff / .xsub / .xpak) are not yet read: no zone bytes have been
staged.** The owner cut the first 1 MB of `data.000` (`cw_data000_head.bin`), which holds the
encoding and vfs-root manifests.

## Format

Blizzard **CASC** local storage (NGDP/TACT), product `zeus`, branch `eu`,
version `1.35.0.27007542`, build name `2026_05_13__TU35__v27007542_Bnet_r2_signed_r_ship`.

- `.build.info`: Build Key `3107edb8e994e5adc88144e792c303bd`, CDN Key `87b4b66b3808794ea521b6253c82803e`,
  tags `mp? zm? EU? enUS speech/text`.
- `.product.db`: install packages checked: `zeus-multiplayer`, `zeus-zombies` (base present);
  **not installed: campaign, arcade, hd**. So the install is MP + Zombies, standard-res textures.
- `Data/config/31/07/3107...` (build config) is plain text. Notable keys:
  - `encoding = 83ad5d0a... 3724aa3ed6a0884d1f09455fc752668f` (size 412959 / 413123 encoded)
  - `root = a4bb8a78...` plus **`vfs-root = 7eebe987... e7c5428895e9567c9cbe1617343c70e5`** (403675 / 266708)
  - `install`, `download`, `size`, `patch-index` manifests.
  The `vfs-root` key means the file tree is **TVFS** (the namespace used by BO4 / MW2019-era
  Battle.net CoD titles), not a WoW-style root.
- `Data/config/87/b4/87b4...` (CDN config): 24 archives + archive-group, 2 patch archives.
- `Data/indices/*.index` (30): CDN archive indexes (for patching; not needed to read local data).
- `Data/data/`: `data.000`..`data.097` (98 archives, ~1 GB each, ~100.4 GB total) and
  **1,022 `.idx` files** = 16 buckets x many generations (the launcher left stale ones;
  only the highest version per bucket counts: `0000000074, 010000006f, 0200000074, 0300000072,
  0400000074, 0500000083, 0600000072, 070000006b, 0800000079, 0900000083, 0a0000006f,
  0b0000006f, 0c00000069, 0d0000008a, 0e0000006f, 0f00000072`).
- `Data/ecache/`: a 212 KB CASC side store holding only 30-byte entry headers and BLTE
  **headers** (chunk tables, no chunk data) - an encoded-header cache, no content.
- `Data/zeus/`: shmem + `.residency` only.
- The 58 `.pak` files are `blizzardbrowser/*.pak` (Chromium/CEF launcher UI), not game data.

### Local index (`.idx`) layout, verified

Version 7: spec `size 4 / offset 5 / key 9 / offset-bits 30`, archive total size 1,098,437,885,952.
Entries: 9-byte EKey prefix, 5-byte big-endian (archive << 30 | offset), 4-byte LE size.
Parsed with a ~40-line Python reader (`idx.py`, kept in the session scratchpad; trivially
reimplementable in Rust).

Result of the 16 current indexes: **3,002 encoded files**, 1,434 with data (100.42 GB), 1,568 that
are header-only 30-byte entries (empty / non-resident). 182 files are larger than 100 MB, largest
945 MB. The few-thousand-big-files profile matches CoD's TVFS layout (large `.xpak`/`.xsub`-style
packages plus fastfiles), not per-asset files.

The core manifests are all at the very start of `data.000`:

| manifest | EKey (9 B) | archive | offset | size (incl. 30 B header) |
|---|---|---|---|---|
| encoding | 3724aa3ed6a0884d1f | 0 | 480 | 413,153 |
| vfs-root | e7c5428895e9567c9c | 0 | 418,319 | 266,738 |
| install | 9752ad32edbeb2cb06 | 0 | 413,633 | 4,686 |
| download | ed89efaa96d6a06ae5 | 0 | 685,057 | 141,324 |
| patch-index | 399d3cd4d538a58e11 | 0 | 826,381 | 4,514 |

So **the first ~831 KB of `data.000` is enough to get the full file-name tree** (vfs-root, TVFS
names) and the CKey->EKey map (encoding).

## Results from the manifests (first 1 MB of `data.000`)

Each manifest decodes from BLTE to exactly the size in the build config (encoding 412,959,
vfs-root 403,675, install 5,090, download 166,751), so the reader is right.

### Encryption at the CASC level: none

The encoding file lists the encoding specs used by all 6,376 encoded keys. There are five, and
none has an `e:` (encrypted) part:

| ESpec | keys |
|---|---|
| `b:{64K*=n}` (64 KB chunks, stored raw) | 6,371 |
| `b:256K*=z`, `b:{683=z,4407=n}`, `b:{11=n,140162=n,26578=z}`, `b:{15=n,26578=z,64K*=n}` | 5 (the manifests) |

So every game file is stored plain (`N` chunks) inside the archives. A file is its CASC entry
minus the 30-byte header and the BLTE chunk table. There are no BLTE `E` blocks and no key names.
That says nothing about encryption *inside* the game's own formats. CoD fastfiles have their own
compression and, in some titles, encryption. MWII/MWIII `IWffs100` titles were encrypted (see
`JUP.md`), and this is not checked yet for Cold War.

### TVFS file tree: 6,415 paths (full list: `docs/cw_paths.tsv`)

The TVFS layout is v1, header 0x2e, EKey 9 bytes, flags 0x0f, no nested VFS, single-span
files. `docs/cw_paths.tsv` lists every path with its size, EKey, archive, offset and CASC size.
Archive, offset and size are `-` when the entry is not resident. Use them to cut one file with
`dd` (skip the 30-byte header plus the BLTE header).

| top folder | files | size | resident |
|---|---|---|---|
| `zone/` | 5,909 | 231.8 GB | 1,090 |
| `video/` (.bk2) | 413 | 13.7 GB | 336 |
| `blizzardbrowser/` (the 58 `.pak` files are Chromium/CEF UI paks) | 69 | 0.2 GB | (launcher) |
| root (exe/dll) | 24 | 0.4 GB | loose on disk |

Zone file types: `.ff` 2,302 fastfiles, `.fd` 2,302 (one per .ff, probably fastfile delta/patch
data), `.xsub` 1,138 stream-package parts, `.xpak` 167 package indexes. This is the same
KAPI `.xpak`/`.xsub` family as MWII (`JUP.md`). There are no `.xcache` files.
Language-prefixed zones exist for 22 languages (`en_`, `fr_` ...). The resolution variants are
`1080_`, `4k_` and `techset_`.

**Map zones (all resident unless marked):**

- MP (36): mp_amerika, mp_apocalypse, mp_black_sea, mp_cartel, mp_cliffhanger, mp_drivein_rm,
  mp_dune, mp_echelon, mp_express_rm, mp_firebase, mp_hijacked_rm, mp_jungle_rm, mp_kgb, mp_mall,
  mp_miami, mp_miami_strike, mp_moscow, mp_nuketown6, mp_paintball_rm, mp_raid_rm,
  mp_russianbase_rm, mp_satellite, mp_slums_rm, mp_sm_amsterdam, mp_sm_berlin_tunnel,
  mp_sm_central, mp_sm_deptstore, mp_sm_finance, mp_sm_game_show, mp_sm_gas_station,
  mp_sm_market, mp_sm_vault, mp_tank, mp_tundra, mp_village_rm, mp_zoo_rm (+ mp_common).
  `_rm` are the BO1/BO2 remakes: Hijacked, Express, Raid, Slums, Drive-In, Jungle, Paintball,
  Russian Base, Village, Zoo.
- Zombies: zm_silver (Die Maschine), zm_gold (Firebase Z), zm_platinum (Mauer der Toten),
  zm_tungsten (Forsaken), plus zm_common (codename mapping from community naming, unverified).
- Fireteam / WZ-era: wz_duga, wz_forest, wz_golova, wz_sanatorium, wz_ski_slopes, wz_zoo
  (resident). wz_doa is not resident.
- Campaign `cp_*`: listed but not installed.

**Stream packages (`.xsub` parts, resident / total):**

| package | parts resident / total | size |
|---|---|---|
| core | 74/74 | 37.6 GB |
| mp_initial | 18/18 | 4.0 GB |
| mp_postship | 43/43 | 17.2 GB |
| zm | 10/10 | 1.7 GB |
| zm_postship | 18/18 | 8.0 GB |
| zm_sink | 13/13 | 1.9 GB |
| wzmp | 37/37 | 13.8 GB |
| *_hd (all), doa, cp_* | 0 | not installed |

**Weapons:** no path names a weapon. As in every Treyarch title, weapon definitions and gun
xmodels are assets inside fastfiles (`core_common`, `mp_common`, `zm_common`, the map .ff files),
with their meshes and images streamed from the `core`/`mp_*` `.xsub` packages. They can only be
listed by name after a fastfile is decoded.

## Plain vs blocked (for porting weapons and maps as was done for BO3)

- **Plain:** the CASC storage, the full file tree, and the raw bytes of every resident file (no
  CASC encryption; an owner-side `dd` of the listed archive range gives the file).
- **Unknown (next check):** the T9 fastfile format itself, i.e. its header magic, compression
  (Oodle, with `oo2core_8_win64.dll` shipped) and whether zone data is encrypted at the game level.
  Also the KAPI `.xpak`/`.xsub` versions. If a fastfile turns out encrypted, stop there: no key work.
- **Blocked:** nothing confirmed blocked yet. Staging is limited to 400 MB per file, so big
  `.xsub` parts (up to 942 MB) need range cuts.

## Game-level containers (owner's four cuts, decoded from BLTE)

These four cuts were checked: `en_mp_sm_central.ff`, `mp_sm_central.ff`, `mp_sm_central.fd` and
`core.xpak`. All BLTE chunks are `N` and the decoded sizes equal the TVFS sizes.

### Fastfile `.ff`: `TAFF` v9, Oodle Kraken, **not encrypted**

```
0x000 "TAFF", u32 9, u32 0x39cb44f1, u32 4, u32 0x64, u32 0x2c2381cf, u32 6, ...
0x046 builder "TA-BM-SV-T9-12" (base build; the .fd brings it to T9-23)
0x17a .. 0x460  table of u64 sizes (0x182: total decoded size, e.g. 30,178,741)
0x46a zone name ("mp_sm_central"), 64-byte field
0x4aa 256-byte signature (RSA-2048 size; matches "_signed_r_ship"; not needed to read)
0x5ba block chain: u32 compressed, u32 decoded, u32 aligned size, u32 this block's own file offset,
      then an Oodle stream; next block at +16+aligned; ends with a zero header
```

The chain walks cleanly to the end. mp_sm_central has 29 blocks of up to 0xfffa0 bytes, and
their sum equals the header total. en_mp_sm_central has 1 block. **Every block starts `8C 06`,
an Oodle Kraken header.** Readable literals appear inside the streams (`en_mp_sm_central`,
`mp_postship`, `core_boot`, `en_core_ui`...), so the data is compressed plaintext. **There is no
game-level encryption.** The signature is only a signature, and nothing was done with it.

The zone content has not been listed yet, because the sandbox has no Kraken decoder. The repo
carries none by design (`fastfile_jup::oodle`), and building the open `ooz` here was not allowed
in this session. T9 asset names are mostly 64-bit hashes (only ~20 strings show in the literals),
so names also need a hash dictionary or the game's script/string tables.

### Fastfile patch `.fd`: one per `.ff`

```
0x00 u32 0xb08, u32 0, f32 1.0, u64 0x200000 (2 MiB window?), u64 30,178,579 (target size), u64 930,523
0x28 TAFF header of the patched zone (builder "TA-BM-SV-T9-23")
0x5e2 TAFF header of the base .ff (identical to the .ff's, "T9-12")
then delta data with Kraken streams inside (first `8C 06` at 0x915a); delta layout not decoded yet
```

So the shipped `.ff` is the launch-era base, and the current TU35 zone = `.ff` + `.fd` patch. The
loader must apply the patch, or accept base-build content. The base is probably fine for geometry
and guns, but this is unverified.

### `.xpak` (KAPI) v0x10

```
0x000 "KAPI", u16 4, u16 0x10 (version; MWIII is 0x17), u64 1, u32 2, u32 7,
      u64 file length (0x680000), u64 hash, 0x028 u64 part count (core: 74 = core-00000..73.xsub)
0x7a0 u64 row count (423,722), u64 table offset (0x8000), u64 table size (= rows * 16)
0x800 .. 0x8000  11-byte (u64 key, u24 index) rows, probably a bucket index into the table
0x8000 rows: u64 key (sorted), u64 packed: top bits part index (74 distinct values),
       the rest offset/size in the part (exact split to be pinned when an .xsub is read)
```

This is the same family as MWIII's KAPI (see `JUP.md`), so `fastfile_jup::xsub` is the starting
point. The streamed meshes and images of guns and maps are behind these keys.

## Plain vs blocked (updated)

- **Plain:** CASC, BLTE, TVFS, the `.ff` block chain (Kraken) and the `.xpak` tables. No
  encryption anywhere so far.
- **Needs a decoder:** Kraken. Use the game's `oo2core_8_win64.dll` on the PC through the existing
  `fastfile_jup::oodle_dll` backend, or an `ooz` library via `JUP_OODLE_LIB` as was done for MWIII.
  The `.fd` delta format and the T9 asset layouts (xmodel, weapon def, GfxWorld, lighting) are
  still to be decoded.
- **Blocked:** nothing.

## Decoding status (2026-10-28, with the game's Oodle DLL staged)

The game's `oo2core_8_win64.dll` is a Windows PE library. The repo's backend for it
(`fastfile_jup::oodle_dll`) is `#[cfg(windows)]`, so it only runs on Windows. On Linux the only
backend is `$JUP_OODLE_LIB`, a separately built decoder library, and none exists here. This
sandbox has no Wine and no Windows target, and per the brief no other decoder was downloaded or
built. **So the zone image cannot be decoded in the sandbox. Decoding runs on the owner's PC.**
The asset list (types, names, hashes; weapons, xmodels, GfxWorld, map ents, lighting) waits on
that run.

## Code

- `crates/casc` (pure Rust, read-only): `idx` (v7, newest generation per bucket), `blte`
  (`N`/`Z`; `E` reported as `Error::Encrypted`, never decoded), `config` (`.build.info` + build
  config), `tvfs`, `storage::Storage::open(install)` -> `read("zone/<name>.ff")` by ranged
  reads of `data.NNN`. Example `cw_casc list|extract`. The test `tests/cold_war_head.rs` checks
  the real manifests: encoding 412,959 B, vfs-root 403,675 B, 6,415 paths, 37 `zone/mp_*.ff`,
  and mp_sm_central's EKey.
- `crates/fastfile_t9`: `taff` (header, checked block chain, `decode(&dyn Decompressor)`), `fd`
  (patch header, `applies_to`), `kapi` (v0x10 header, sorted rows, `find`). Example
  `cw_zone <install> <out> <zone>...` reads the zone straight from CASC, decodes it with the
  game's DLL, and writes `<zone>.zone` and `<zone>.txt` (header, blocks, printable strings). Test
  `tests/cold_war_cuts.rs` checks the owner's cuts: 29 Kraken blocks summing to 30,178,741, the
  `.fd` base equal to the `.ff`, and `core.xpak` with 74 parts and 423,722 sorted rows.

Both tests skip when the staged files are absent.

## Porting plan

1. `crates/casc` (pure Rust): `.idx` v7, BLTE (`N`/`Z`), encoding, TVFS. It opens a file by path
   as ranged reads from `data.NNN`, so no 1 GB staging is needed. This is a port of the scratchpad
   readers (~150 lines).
2. `crates/fastfile_t9`: TAFF header, block chain, Kraken through `fastfile_jup::oodle`
   backends, `.fd` patch apply, KAPI v0x10 + `.xsub` entry reader (adapt `fastfile_jup::xsub`),
   and a T9 hash -> name table.
3. Asset readers in the order BO3 used: xmodel/surfs, weapon/attachment defs, then GfxWorld,
   clipmap, map ents, lighting/fog/sky, FX. Then `render_t9` modelled on `render_t7`.
4. Run on the PC (Windows, with the game's Oodle DLL): a `cw_extract` tool that reads CASC in
   place and writes decoded zones and needed `.xsub` entries into a small export folder. Only that
   folder is staged.

## Earlier cut list


Cut each one with `dd` from `Data/data/data.<archive>`, bytes [offset, offset+size). These are
entries with header, and the BLTE is plain:

| file | archive | offset | CASC size |
|---|---|---|---|
| zone/en_mp_sm_central.ff | 007 | 1,073,276,633 | 1,859 |
| zone/mp_sm_central.ff | 006 | 734,390,240 | 5,964,877 |
| zone/mp_sm_central.fd | 009 | 1,069,417,827 | 3,774,519 |
| zone/core.xpak (KAPI index) | 008 | 877,942,019 | 6,818,386 |

The `.ff` header settles game-level encryption and compression. `core.xpak` gives the KAPI version
and the stream-key table.

Tools: the readers used here (`idx.py`, BLTE, TVFS, encoding, ~150 lines of Python) are small enough to
port into a Rust crate. CascLib, pycasc and the `casc*` crates are alternatives.

## First decoded zone: `mp_sm_central` (2026-11-03): stopped, the zone strings are encrypted

The owner ran `cw_zone` on his PC. `mp_sm_central.zone` (30,178,741 bytes, 29 blocks) is in
`Documents/cw_zones` with its strings list. The Cold War exe was **not** read: the uploaded copy is
a dump of the Arxan-protected binary with the protection stripped, and the project does not read
such dumps.

- The image starts with a `u64 0x22a`, then `0xff..ff` pointer markers (an asset-list header as in
  T7). The string table starts around `0x1000`.
- **Most zone strings are encrypted.** Each string is stored as `00 <tag> 01 <len> <len bytes>`,
  with the tag in `0x80..0xff`. About 20% are plain (`base_ger_kgb_cables_electrical_double_02_lod0`
  at `0x11b5`, `board_seam`, `cap02_grime`). The rest have the same length but random-looking
  bytes. A sample over the first 2 MB found 123 plain and 603 scrambled strings, in every tag
  range `0x8_..0xf_`. This is per-string encryption at the game level, and its keys and routines
  are in the exe. Per the brief, it is reported here and not worked around: no cipher analysis
  and no key search. Asset names stay unknown except for the plain ~20%, and T9 names are mostly
  64-bit hashes anyway.
- Not yet checked: whether the asset bodies (GfxWorld, materials, models, lights) are plain
  binary. The asset readers, `CW_ZONE_DATA.md` and the Cold War map lane wait for the owner to
  decide whether to go on without names.

`mp_common` failed with `bad block header at 0x5ba`, and `mp_nuketown6` was never reached. The
sandbox has no bytes of either zone (only the BO3 `mp_common.ff` is staged), so the cause is not
known. Both sizes are multiples of 4 KiB, unlike `mp_sm_central`, and `mp_common.fd` (17.3 MB) is
larger than its `.ff` (16.4 MB). `taff::first_block` now falls back to the first header after
`0x5ba` that names its own offset. If a zone still fails, `cw_zone` writes the raw `.ff` and
`.fd` of that zone to the output folder for staging and carries on with the next zone.
