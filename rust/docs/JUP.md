# Modern Warfare III (2023, "jup", cod23) — containers

Code: `crates/fastfile_jup` (`container.rs`, `xsub.rs`, `oodle.rs`,
`oodle_dll.rs`), probe `cargo run -p fastfile_jup --example jup_inventory --
<install> [--list]`. Read on the retail build 1.21; everything little-endian.
Zone contents (asset list, layouts) are a separate layer, not described here.
(Modern Warfare (2019) support was removed at the owner's request.)

## Oodle

Every compressed run is an Oodle LZ stream; the zones' chunk headers say
compressor 6 (Kraken in the open decoders' numbering), first bytes `8C 06`
(compressed) or `CC 06` (stored). No pure-Rust decoder with a licence that
fits this Apache-2.0 tree exists: `oozle` is GPL-3.0, `oozextract` is labelled
MIT but is a structural port of GPL-3.0 `ooz`. So `oodle::Decompressor` is a
trait; on Windows `oodle::load(install)` loads the game's own
`oo2core_8_win64.dll` from the install root (or `$JUP_OODLE_DLL`) at runtime.
The DLL is never shipped or committed. `StoredOnly` (pure Rust) decodes stored
chunks only — enough for the tests. A pure decoder drops in behind the trait.

## `.ff` (`zone/*.ff`)

```
0x000 "IWffa100", u32 0x18, u32 1, u8[4] flags (01 00 04 xx | 01 00 05 00)
0x014 u32 file length - 0x120
0x038 u64 decompressed zone size      (0x040.. more u64s for the zone layer)
0x0e0 "TAFF" ...
0x120 auth region, 0x8000 bytes: u8 2 "IWC" "IWffs100", signature bytes,
      zone name at 0x250 (NUL padded), the first master block
0x8120 data header (12): u32 ?, u32 zone size (low 32 bits), u24 block size
      0x10000, u8 compressor 6
0x812c blocks: u32 compressed, u32 decompressed, u32 0, Oodle stream;
      next header at the next 4-byte boundary
```

Counting the data header as chunk 0, a 0x4000-byte master block sits before
every chunk whose index is a multiple of 512 (before blocks 511, 1023, ...).
The last block ends exactly at EOF; every block but the last decodes to
0x10000 bytes. No searching is needed. Swept on all 16,927 `.ff` of 1.21
(13.6 GB): every one walks strictly to EOF with decoded lengths summing to
0x038; 234 of them (random 200 + the 15 largest, the smallest, Rebirth tiles,
`mp_jup_rust`, `ingame_br`) fully decoded through the DLL, 6.5 GB of zone, all
exact. 563,269 blocks, 2,076 of them stored; 577 master blocks. Not encrypted.

## `KAPI` packages (`zone/*.xsub`, `zone/*.xpak`, `xpak_cache/`)

```
0x000 "KAPI", u16 flavor (1 = .xsub part / cache, 2 = zone .xpak index),
      u16 version 0x17, u64 1, u64 kind (3 in .xsub), u64 file length, u64 hash
0x028 u64 part count (.xpak), 0 in a part
0x788 u64 file count, data offset (0x8000), data size,
      hash count, hash table offset, hash table size (= count * 20)
hash row (20, sorted by key): u64 key; u64 packed: bit 0 compressed,
      bits 1..31 size in the file, bits 32..63 offset / 128; u32 decoded size
```

A stored entry is its bytes. A compressed entry is one or more segments of
at most 1 MiB, each: `u16 0xF01D`, u64 key (= the row's), u32 this segment's
offset in the entry, u32 the next one's (0 = last), u16 segment count, u16
index, u8 block count, then per block 21 bytes (u8 compressor, u32 compressed,
u32 decoded, u32 stream offset from the segment start, u32 offset in the
decoded entry, u32 checksum — not checked), then the streams. Decoded blocks
are 0x3ffe0 bytes, a segment's last shorter. Segments start exactly 1 MiB
apart. Swept on every entry of the 139 parts: 517,923 stored, 2,021,780
compressed (2,013,394 in one segment, the rest in 2 to 9+), all segment
headers and block tables consistent and summing to the row's decoded size;
2.4 M blocks, all compressor 6; a random 4,906 of them decoded through the
DLL, and `jup_inventory` decodes the first/middle/last 3 entries of each
part (1,231) and the first entry of every `.xpak` with parts.

The 139 `.xsub` parts (2,539,703 entries) carry data and their own table at
the end. The 39 zone `.xpak` are tables only, one row per entry of all their
parts (`<name>-00000.xsub` ...): offset = part × stride + offset in the part,
the stride being the smallest power of two that holds the largest part
(256 MiB for `boot`, 1 GiB for `codhq23a`). Checked on every row of the 31
that have parts (2,497,482 rows, all equal to the part's own row); the 8
`*_cdn.xpak` (kind 4) name no installed parts. `xpak_cache/cache_N_2.xpak`
(flavor 1, kind 1) index `cache_N_2.xpakdata` (entries start `1D F0`); not
read yet.

## Resurgence's sounds (`gamemode_iw4::resurgence::SOUNDS`)

Read at runtime from `ingame_br`, `eng_ingame_br`, `eng_mp_jup_escape5`,
`ingame_shared_mp` and `mp_jup_escape5` (`asset_audio::jup_resurgence_sounds`):
1.21 has 1,527 aliases in `ingame_br`, 893 in `mp_jup_escape5`, 1,615 English
voice lines in `eng_ingame_br` and 30 in `eng_mp_jup_escape5`; the battle
royale's pickups, cash, armor hit markers, purchase and end stings are in
`ingame_shared_mp` (16,628 aliases with `ingame_mp` and the base zones). Alias names are FNV-1a
hashes; a route names an alias where the name is known and `#<hash>` where
it is not.

- Names: the music (`mus_mx_mp_br_ri_*`, plain in `ingame_br`), and ~480
  sound names recovered by hashing: identifiers of the zones
  (`jup_alias_names`), and words of the zones joined under prefixes met from
  each hash's end (`jup_alias_brute --tails`: `br_splash_mission_accept`,
  `br_circle_closing`, `uin_iw9_br_victory_slam`, `br_pickup_cash_med_01`,
  `hit_marker_armor_break_plr`, `ui_select_purchase_confirm`, `emt_escape5_*` ...).
- The announcer: the voice lines of the two English zones were decoded from
  the install and run through a local speech-to-text model on the owner's PC
  (transcripts stay there, `C:\RE-Tools\jup\br\vo_all.tsv`; nothing of them
  is committed), then sorted into events by what they say. 304 of the 1,645
  hashed lines are Resurgence's (redeploys, the circle and safe zone,
  squads and players left, the countdown numbers, contracts, UAVs, loadout
  drops, buy station, victory and placings, infil and landing); 879 belong
  to the other modes these zones also carry (DMZ, Plunder, Lockdown, the
  Gulag, events) and 462 are flavour lines with no event here.
- Rebirth Island's ambience: the 37 stereo `wz23` streams of
  `mp_jup_escape5` of 90–97 s (beds), chained one after another.

- Alias links (`fastfile_jup::sound::alias_links`): an alias's record is
  its name hash, the hash of the alias played with it (a gun's fire is a
  chain of layers, each naming the next) and a block-15 pointer to its
  sndasset's asset-list entry, then a u32 of flags whose top two bits are
  clear: 0x3FFFFFFF on most interface and voice sounds, 0x1FFFC080,
  0x3FF04080 ... elsewhere. Read only with 0x3FFFFFFF at first, `ingame`
  showed 78,960 links and `ingame_shared_mp` 63,061; with any flags,
  135,078 and 141,056 (`eqp_armor_plates_insert_plr`, `iw9_mp_radiation_tick`
  and the guns' layers were among the missed).
- A bank's aliases also have headers (0x30 bytes: -2, the name hash, f32
  near and far distances, u16 count, 0xFFFF), in runs **sorted by name**:
  a hash between two named ones shares their prefix
  (`C:\RE-Tools\jup\sfx\tables.py` lists them; `br_circle_closing`,
  `br_circle_effect_lp`, then an unnamed one (`br_circle_nuke`, found so),
  then `br_circle_proximity_lp` ...). Names found this way and with
  `jup_alias_brute` under the neighbours' prefixes: `match_start_tick`,
  `uin_mp_splash_game_mode`, `iw9_mp_radiation_tick`,
  `eqp_armor_plates_{open,insert,close}_{plr,npc}`, `mp_slam_gulag_closed`,
  `mp_hq_respawn_disabled`, `mp_care_package_drop_lp`,
  `mp_care_package_med_impact(_metal, _swtn)`, `ui_mp_resurgence_timer(_tick)`,
  `uin_iw9_br_gulag_winner_redeploy`, `br_circle_effect_lp`.

Not named yet, so these events keep Black Ops II's stand-ins: the circle
closed, the final circle, gas entered, squads remaining and the top
squads (the announcer says the last three: `vo_final_circle`,
`vo_in_gas`, `vo_squads_left_*`).

## Emblems and calling cards (`cards.rs`, probe `jup_cards`)

All of them are defined in `zone/global.ff` (13.7 MB, 44 MB of zone), one
script bundle (type 115) each. A bundle's string cells hold its name in the
clear (`-2`, a 32-bit hash, 4 zero bytes, the name, NUL): `ui_emblem_005`,
`jup_emblem_n5934_1`, `ui_playercard_100`,
`jup_playercard_animated_opc_outbreak_mike4_tier1_atlas`, ... (calling cards
are "player cards"), and its title's localize key (`EMBLEMS/EMBLEM_005`;
the title is encrypted and not read). Its picture is the image asset of the
same name (`hash::name(name)`), stored just before the bundle; on 1.21 every
one has a single level (image header 0x2c) in a single streamed group,
found in `boot_shared` (the older ones), `codhq23b` / `codhq23a` (the
seasons') or `codhq_shared` (87). Image format byte 0x25 is BC1 (cards),
0x30 BC7.

1.21: 5,315 names, 5,272 with a picture in the zone: 2,889 emblems (2,182
flipbooks) and 2,383 calling cards (1,710 flipbooks); 2 pictures are in no
installed package (fetched on demand), 5,270 are there.

Flipbooks are frames side by side in rows, an emblem's 256x256, a card's
512x128: a 768x1024 emblem is 3 by 4 frames, a 1024x1024 one 4 by 4, a
1024x1024 card 2 by 8, a 1536x512 card 3 by 4. Neither the bundle's cells
nor the name tells one from a still picture (`ui_emblem_143` and
`ui_emblem_286`, both 1024x1024, have the same cells; some `_atlas` names
are 256x256 stills). The size decides where it can (a card of whole frames
not itself 4:1, an emblem of whole frames not square: a flipbook); a square
emblem of several frames is a flipbook when its neighbouring frames are
nearly the same picture (`detect_grid`).

The game shows them at runtime through `crates/ui/src/jup_cards.rs`
(`jup:<name>`, a frame `jup:<name>#<n>`), from `$BO2_JUP_INSTALL` or the
game folders file's `jup` line.

## The Warzone HUD's pictures (`crates/ui/src/jup_hud.rs`, probe `jup_hud_pics`)

The in-game HUD pictures are images (type 21) in `ingame`, `ingame_br`,
`ingame_shared_mp` and `ingame_base_mp`, named by hash only; the LUI scripts
that name and place them are compiled and not read. `jup_hud_pics <install>
<out> [max side] <zone>...` writes a zone's pictures as contact sheets (8 by
8 cells in asset order, `JUP_ONLY=0x30` and `JUP_LARGEST=512` keep the
UI-sized BC7 ones) with a TSV of cell to hash; the HUD's were picked from
those by eye. 1.21: `ingame` 85 images (map and objective icons), `ingame_br`
4,095 (733 BC7 of 512 or less: contracts, pings, loadout drops, buy station,
gas, plates, squad), `ingame_shared_mp` 2,201 (ammo, plates, cash, hit
markers, kill skulls), `ingame_base_mp` 2,429 (plate bar segments, rings,
armor break). UI pictures are white or grey glyphs (format 0x30, BC7, most
with a single streamed group in the zone `.xpak`s), drawn tinted.

`jup_hud::PICTURES` is the table (our name, zone, hash); the Warzone HUD
(`hud_warzone`, Resurgence only) draws them in `shell::wz_theme::HUD`'s
colours and a plain shape for any the install lacks. All 46 read in about 4 s
off the main thread on 1.21.

## Maps (`fastfile_jup::map`, `assets::lane::jup_map`)

Played: Rebirth Island (`mp_jup_escape5`, Resurgence), Tokyo
(`mp_jup_elevated`) and the multiplayer maps whose public names are certain:
the Modern Warfare 2 remakes (Afghan `mp_jup_afghan_v2`, Derail, Estate,
Favela, Highrise, Invasion, Karachi, Quarry, Rundown, Rust, Scrapyard,
Skidrow, Sub Base, Terminal, Underpass, Wasteland), Shoot House
(`mp_jup_shoothouse2`), Shipment (`mp_jup_shipment_iw9`), Das Haus, Farm 18
and Growhouse (`asset_transport::JUP_MAPS`). The install's other `mp_jup_*`
zones (variants, events, modes) are left out. Tokyo's zone is the one
dressed as Japan (`jup_appliance_vending_machine_food_japan_01`,
`jup_sign_arcade_japan_*`, `jup_sign_convenience_japan_*`, pachinko and
ramen props: 198 `japan` names, no other map zone more than one). Start one
with `map jup:<zone>`. A map is installed when its small `eng_<zone>.ff`
is (each of the above is in 1.21).

- World: the `gfx_map`'s 128-byte surface records over `gfx_map_trzone`
  vertex streams (`map/world.rs`). Each record names its world zone: 0 the
  map zone's own, k the k-th `<map>_*_0_tr` zone in name order (Rebirth: 24
  grid tiles `_<x>_<y>_0_0_tr` and 11 swap zones; checked by every zone's
  packed triangle bytes). Swap zones' other states (`_swap_dest_`,
  `_swap_t_`) are left out. Triangle tables run in the records' packed order
  but for a few surfaces placed later: matched by size, read in table order.
  Rebirth: 8297 surfaces, 1.40 M triangles, read in about 9 s.
- Materials (`map/materials.rs`): a material's colour is its first picture
  of usage 0x36 (BC1 or BC7; 0x32 is a gloss picture, flat grey where a
  material has none; 0x37 the normal map; 0x33/0x38 masks), else its first
  BC1. (First BC1 alone missed the BC7 colour maps: Rust 224 of 608
  materials coloured, 344 so; about 240 more have picture tables this
  reader does not follow yet.) The `gfx_map`'s material table (back-
  pointers to asset-list entries, all materials) and a 40-byte record per
  surface (+0 surface record, +4 table entry, +0x0e world zone). Material
  headers (0x78 bytes; +0x1b picture count, +0x30 the pictures) are found by
  shape as the run of exactly the zone's material count (Rebirth: 5841);
  pictures are asset-list entries of the image run. Format 0x25 (BC1) is
  the colour maps' form; 0x2a (BC1) holds masks and terrain blend maps, 0x2f
  BC7 (normal maps: red gloss, green and alpha the normal in diamond form, blue occlusion; see "Shaders, techniques and materials"), 0x30 BC7. Pixels: the largest streamed group within 1024 px from the
  zone `.xpak` indexes (levels placed from the top: BC1 streams to 2x2).
  A material whose picture table is another material's points into a block
  whose offsets drift from the zone image: the shift is voted from the
  materials' own tables (Rebirth: 655 tables, about 62 MB). Terrain (virtual
  textures, 256x256 tiles of formats 0x28/0x29) still draws without a
  picture (Rebirth's largest grey: 0x2a mask only, and tables of none).
- Placed models (`map/models.rs`): the `gfx_map`'s model table (16 bytes,
  back-pointer to the xmodel's entry), collections (16 bytes: first, count,
  model) and instances (24 bytes: u16 quaternion as v/65535*2-1, f16 scale,
  i32 origin in 1/4096 units). XModel headers (0xE8 bytes, name inline,
  `jhash(name)` at +0) stand in asset order; LODs (0x48 bytes) name their
  `xmodelsurfs`, whose meshes stream from the `.xpak` by the key at +0x10.
  Headers pair with surfs entries by aligning surface counts. XModel headers
  stand in asset order, a kind -1 entry's naming itself inline (name
  pointer -2), a kind -2 entry's by reference (a block pointer; found by
  the header's shape, +0x24 `c0 b6 73 2c` then 1.0); they pair with the
  entries run by run of a kind. Rust: 954 entries (19 of kind -2), 952
  headers; the two unrecognised stand together in one wide gap (0x342f
  against a median 0x33c) and are left out. Pairing kind -1 entries alone
  with inline headers (earlier) found the counts unequal and read no
  model: the MP maps drew without their props and fell through. Positions span
  `(2v-1)` times the half extents. Shadow casters, `tm7/` stand-ins and
  destructibles' other states are left out. Rebirth: 3580 models, 130,785
  placements, 4-6 s. LOD0 is drawn out to the last LOD's distance (at least
  1500); a coarse LOD collides inside the play area, not foliage, decals or
  effects' models.
- Sheets (flat, wider than 16384 units): the sea (Rebirth: 14 at z=64) is
  drawn cut to the play area and 16384 units around, not collided; the sea
  bed and vista sheets are not drawn (they stretched the bounds to ±144k).
- Terrain (`map/terrain.rs`, `lane/jup_terrain.rs`): the ground between
  the buildings is in no world surface (Rebirth's island showed the sea
  through it). An asset of the map's name holds the height field's header
  (+0x14 half side 16384, +0x20 1025 samples a side, +0x28 128 cells,
  +0x2c lowest height, +0x3c side 32768, +0x44 85 quadtree nodes) and at
  +0x100 a coarse 132 x 132 u16 grid (128 cells of 256 units and 2 more
  each side, sample centres from -16768, height `lowest + v / 8`); the
  1025-sample field is not in the zone. The grid is drawn and collided as
  16 x 16-cell chunks (chunks under the sea are drawn only), 20 units up
  (the path nodes on no world surface stand that far above it, 73% within
  32). The virtual texture: a table of 1365 tiles (a 6-level quadtree, each
  level row by row from -x -y) of 5 image back-pointers each, the BC1
  (0x25) one the colour; tiles are 256 px with a 4 px border (one BC1
  block), so a level is the tiles' inner 248 px side by side, and levels 0
  to 4 make one BC1 picture's mip chain (3968 px, about 9 units a pixel).
  Where it lies was fitted against the grid's coast: from (-16225,
  -16225), 34900 a side (the land masks agree on 95%).
- Entities: the zone's `map_ents` (`map/ents.rs`; pairs of key index,
  kind and value over hashed keys, GUIDs, vectors and pooled strings). MP
  spawn points are not among them: they are made over the AI nodes.
  Worldspawn has the sun (`sundirection`, `suncolor`, `sunlight`).
- Light (`lane/jup_light.rs`): the lightmaps are not read. The sun and
  the sky are baked into the world's vertex colours (a sun ray and 6 sky
  rays a vertex against the world and the larger placed models' coarse
  LODs, through a BVH) as the share of the light that reaches the vertex,
  drawn through the IW3 lightmapped path over a uniform lightmap of the
  sky's colour; each placed model takes one light sample above itself.
  World triangles wind clockwise seen from the front (every sea triangle
  does seen from above); normals follow that.
- Sky (`lane/jup_sky.rs`): the zone's BC6H (0x2d) pictures of use 0x3a,
  twice as wide as high, are two mirrored squares each the upper
  hemisphere seen from above; mapped concentrically with height
  `1 - r^k`, `k` fitted to put the picture's sun at worldspawn's height,
  turned to worldspawn's sun direction, exposed and written as the cube.
- Collision: the drawn world, the terrain and placed models inside the
  play area; surfaces wholly 48 units under the sea and models 192 under
  it do not collide (the fall kill plane then sits just under the island).
  This is the stand-in only where the zone's `col_map` cannot be read
  (`BO2MP_JUP_CLIP=drawn` forces it); see "Collision, water and ambience".
- World surfaces' extra stream (record +0x34, 4 bytes a vertex) holds blend
  weights, not colours.
- Not read yet: the full height field, and the things listed
  under "Lighting" below.
- **Lighting** (`fastfile_jup::map::lighting`, read offline on seven staged
  multiplayer maps; MW3 maps are lit the way MW3 lights them, never through
  the Black Ops III look; the BO3 native renderer is only a structural
  example):
  - **No lightmaps.** Every world surface has lightmap index 0 or 510 and no
    lightmap UVs (the earlier "indexes 0..29" reading was wrong). Light is
    probes plus volumes.
  - **Reflection probes** (image usage 0x2b, BC6H octahedral, z up): one
    52-byte record per probe, origin at +0x10, `0xfff00000 | k` at +0x30
    (only that word is reliable across maps); the probes' back-pointers sit
    just before the records. Probe counts match picture counts on all seven
    maps. Pixels: inline in the zone for 3 to 6 probes a map, the rest are in
    the streamed packages and may be CDN-only (not verifiable offline).
    Outdoor probes read up about 1.6 and bluish, indoor ones warm and dim.
  - **Volume lighting images** (usage 0x21 and 0x11): a BC6H 3-D probe
    volume, 4-byte index volumes, a six-slice half-float grid, a BC4 coverage
    mask and height maps, found by their header block (box min at +0x20, max
    at +0x30, dims as f32 at +0xF0). Located and decoded to pixels; HOW the
    shaders sample them is not worked out, so they are not used yet.
  - **Used:** each probe's picture becomes an up/side/down cosine-lobe
    ambient. In `jup_light::bake` a day map's sky light at a vertex (and at a
    placed model) is the uniform ambient scaled by the nearest of the four
    closest probes it can see, against the map's median open-sky probe,
    clamped 0.1 to 1.6 times. Night maps, maps with no readable probe, and
    `BO2MP_JUP_PROBES=0` keep the old ray bake exactly. The converted-map
    cache key includes `lighting.rs`, so old entries are not reused.
  - **Vision sets** (`fastfile_jup::map::atmosphere`, probe `jup_atmosphere`
    ; 27 Oct, six maps + the global zones): a map zone holds one or two
    small deflate-compressed text files among its raw files (each behind
    a `-2` pointer, `78 DA` zlib), lines `#x0<hex> "value"`: a 64-bit hash
    of the setting's name (not the asset-name hash; no name list known)
    and its value. The first names itself under `296239ab95968496`
    (`mp_jup_rust`, `mp_jup_derail_clut`); the global zones (`global`: 15)
    hold the thermal and other special sets with the same keys. About 80
    settings. Matched by value across maps to Infinity Ward's exponential
    fog (the fields of Ghosts' `create_vision_set_fog`): start
    (`f20b8d6f1754f879`: 1000 Rust, 4000 Derail, 350 Farm 18) and half
    (`0905f173a5ca3fef`: 2200, 2000, 12000) distance, colour
    (`56abb99d944feebb`) and sun colour (`d4128630ca211ea9`) as 8-bit
    values over 255, a unit sun direction (`909e0343c33f0296`), begin and
    end fade angles (`62c8f547e67a3753` < `fea33e4ef0b55969`: Rust 18.72 and
    113.04, Shipment 0 and 90), two opacity-like numbers
    (`89a94552cff79a58`, `90db71099e9df4a6`: Rust 0.8 and 0.5, Elevated 0
    and 0; taken as maximum opacity and normal-fog scale, the order is a
    guess) and base and half height (`2d725e6a9e05ce42`,
    `56e86cc101cec126`: Rust -1500 and 2300; guessed to be a height fog).
    The sun direction: a round elevation on the maps that use it (Rust 40,
    Das Haus 33, Elevated 9 degrees), `0 0 1` on Derail (whose worldspawn
    has the sun, -17.5 pitch) and downward on Shipment (night). Seven
    numbers around `49c68bc8a1894df8`, `88a6fb1edae5abaa`,
    `2301bd1bbf7bd9b0`, `8c03304fd2546cc7`, `e83a78dfd9da3a8d`,
    `66de1d1d8c817a25`, `1b4040b94566e453` (Rust 10, 9.6, 13.5, -2.6, 3.8,
    -5.25, 3.8) look like the exposure and curve numbers; `18a307bf53934ca`
    (-40.39 / -60 / 0 per map) is a three-vector; nothing else is named.
    The tonemap is another task and is not touched.
  - **Sun:** most maps have none on worldspawn (`lightstate`
    `mp_jup_rust`, `default_day`, `sky_tl_20230130_d_c`, `mp_farm18_am`
    name the lighting state that holds it, not read); Derail and Shipment
    have `suncolor`, `sundirection`, `sunlight` as before. The vision set's
    sun-fog direction now stands in for the sun where worldspawn has none
    (unverified; its colour and strength are not found, the old defaults
    stay).
  - **Placed lights:** the map's `light` entities (Shipment: ten, `pl#` 35
    to 44, `script_start_state off`, `lt_shipside_25`) have a 200-byte
    byte-packed record in the zone, found by origin (also a GUID 48 bytes
    after it): at the origin +12 radius (100 to 260), +20 1.0, +24 a number
    (2, 4, 6), +40 and +44 two numbers (0.342/0.5/0.537/0.707 and 0.643/
    0.67/0.737: cone cosines?), +64 u32 3, +96 50000, +100 50, +108 the
    colour as three bytes (all white). Records sit in an array by `pl#`
    (stride 200) and again 18178 bytes on. Direction comes from the
    entity's `angles` (the record holds a matrix of them). Rust has no
    `light` entity at all: baked lights are inside the volumes. Read into
    `JupMap::lights`; not drawn (they start off).
  - **Fog in use:** `jup_fog` on worldspawn, then `PreparedWorld::exp_fog`
    (`BO2MP_JUP_FOG=0` off). Whether the Black Ops III path the maps are
    drawn through shows it is not checked.
  - **Volume lighting, why it is not used:** `code_post_gfx` keeps the names
    `LookupRawLightGrid`, `GfxSHProbeFallback` and `LookupNeuralLightGrid`:
    the game has a raw grid, an SH probe fallback and a neural (network)
    light grid. On Rust the 3-D index image (100 x 92 x 16) is all zero and
    the 2-D index (69 x 66) all `0xffffffff`; the half-float six-slice grid
    (70 x 67, 6 slices x RGBA) has signed values (-2 to 2.3), so it is not
    colour; the BC6H volume (48 x 48 x 80) has dense ~0.5 slices
    alternating with sparse warm ones. None decodes to irradiance without
    the lookup's code (needs the DXIL of the grid shaders). The reflection
    probes stay the indirect light: the bake blends the four closest
    visible ones by inverse square distance (softened by 200 units).
  - **Indoor probe volume, second look (27 Oct):** `grid_2d` (Elevated 39 x 52,
    Derail 50 x 49 with 3 cells) is a sparse brick table (`Grids::brick_table`):
    a u32 a cell, `0xffffffff` empty, else high half = `column | row << 8` of
    the brick in the volume's atlas (16 columns), low half `0xffff`.
    Bricks are 9 x 9 texels (8 cells of 48 units plus a shared border texel;
    the border texels of neighbours agree, 0.002 against 0.018 for 8-texel
    bricks) and 16 layers deep (Elevated's volume 144 x 136 x 16 BC6H, 226
    bricks). Layers 0 to 7 of a brick are almost 0 with rare bright texels (up
    to 1.7), layers 8 to 15 are 0.5 plus or minus a deviation (0 to 1.14,
    mean 0.52 0.51 0.64): a biased signed encoding, not irradiance. The
    vertical mapping of the layers is not known. The six-slice grid (Derail
    51 x 50, Elevated 40 x 53, Farm 119 x 111) has large signed numbers with
    no clean layout (24 numbers a texel; PCA 53% in one component; no
    ambient-cube or L1 pattern), which fits `LookupNeuralLightGrid`. Rust's
    brick table is empty, Farm 18 and Afghan have no probe volume. Nothing
    of this lights anything yet.
  - **Lightstate:** not found. Worldspawn's `lightstate` names
    (`mp_jup_rust`, `default_day`, `sky_tl_20230130_d_c`, `mp_farm18_am`) are
    not stored under the asset-name hash in the map zones or in `global`,
    `ingame`, `ingame_shared_mp`, `global_stream_mp`; Derail's worldspawn sun
    (colour 1 0.9961 0.9804, strength 2.521, angles -17.5 -19) is not in its
    zone as floats. In Rust the second text file is the gunship vision
    (`vision/mp_jup_rust_killstreak_gunship.vision`), not a lighting state.
    The sky pictures that would show the sun's place are streamed and not in
    this install, so the sun-fog stand-in is unchecked.
  - **Exposure keys (candidates only, not wired):** pairs that look like
    auto-exposure limits in stops: `8c03304fd2546cc7` and `66de1d1d8c817a25`
    (negative: Rust -2.6 and -5.25, Elevated -2 and -2) against
    `e83a78dfd9da3a8d` and `1b4040b94566e453` (positive: 3.8 and 3.8,
    Elevated 1.5 and 1.5); `49c68bc8a1894df8`, `88a6fb1edae5abaa`,
    `2301bd1bbf7bd9b0` (10 / 9.6 / 13.5 on Rust) are three rising-ish
    numbers whose meaning is not clear. Nothing in Ghosts' vision-set fields
    settles which is which, so the look's exposure is left alone.
  - **Not found:** exposure/tonemap/colour grade names, lighting state asset
    (sun colour and strength, baked lights), lightmaps on Rebirth (not
    staged). The horizontal orientation of the probe pictures is a guess.
    Nothing was seen on screen (no GPU).
  - `cargo run -p fastfile_jup --example jup_probes -- <zone .ff> <out dir>`
    lists probes and writes their pictures as PNGs.
- A hidden test run spawns with `BO2MP_AUTOCLASS=class_smg` (the console's
  `spawn 0` waits for a class menu nobody answers):
  `CodLegacy map jup:mp_jup_escape5 --cmds "wait world; wait 50s; force_match_start &; …"`.

## Movement (`movement_iw4::modern`, probe `jup_movement`)

Resurgence plays Modern Warfare III (2023)'s movement; every other playlist
keeps Black Ops II's (`bo2mp_movement bo2` / `mw3` overrides it). Gun
handling stays Black Ops II's. The choice and the sea level ride in the
snapshot's movement rules (`Bo2Feel::modern`, `water_z`: the map lane writes
the sea's height to worldspawn as `sealevel`), the state in the player state
(`mw_*`), so host, prediction and bots agree.

- Sprint never runs out. Sprint pressed again while sprinting: tactical
  sprint (faster, gun down) until its time is spent; it refills over time
  and at once on a slide cancel.
- Crouch while sprinting on the ground: a slide (a burst that bleeds off,
  steered a little). Jump ends it in a jump; letting go of crouch ends it
  standing and sprinting.
- Jump moving into a wall climbs any walkable top in reach (no mantle
  brushes: these maps have none); a low top with open ground just past it
  is vaulted. The body plays Black Ops II's nearest climb.
- Deeper than chest height in the sea: swimming (floats at the surface, the
  view's pitch steers under it, jump rises, crouch sinks, sprint is faster
  on the surface, the gun down); knee-deep water slows.
- Its strafe, backpedal and stance speeds, no repeated-jump slowdown, falls
  hurt from higher. Plating while moving already works (the script's plate
  hold never stops the player).

No engine of it to port: every number is in `MW3_2023`, noted MW3 2023 (its
behaviour as played) or estimated. `cargo run -p fastfile_jup --example
jup_movement -- <install> [zone...]` prints, per zone, the plain-text
names with a movement word in them and the hashes of the likely setting
names, each with the numbers stored after it, to replace the estimates.

## Guns (`fastfile_jup::gun`, `assets::lane::jup_gun`)

Gun sounds: MWIII's `wpn_<class>_<gun>_fire_*` and MWII's `weap_<gun>_fire_*`
(no transient; the shot heads its layers), foley `wfoly_plr_<class>_<gun>_*`
with reloads numbered `reload_01`... Most are in `ingame_shared_mp`, not
`ingame`: both are read. Each name was checked by hashing (FNV-1a 64) against
the zones; the SVA 545's fire was not found, so it keeps its donor's.

Camos: Modern Warfare III's (`ui_jup_camo_<family><variant>`, 164) and II's
(`ui_camo_<series>_<n>`, 331) Create-a-Class pictures, all 256x256 in
global_stream_mp, are drawn as patterns over every gun's materials. Their
camo numbers start at 300 and 500 (`weapon_camos::JUP_FIRST`, `IW9_FIRST`),
past a byte: classes, `calcweaponoptions` and `giveweapon` carry camo
numbers up to 1023.

Camo names (20 Oct 2026): each camo is a script bundle in global_stream_mp
holding its picture, its reference and its localize key in the clear
(`ui_camo_a_06`, `camo_a_06`, `CAMOS/CAMO_A_06`; `ui_jup_camo_13h`,
`jup_camo_13h`, `JUP_CAMOS/JUP_CAMO_13H`; 495 bundles, one per picture). The
names themselves are not readable: the `eng_*` zones' localize entries are
(name hash, value), and every value shorter than about 256 bytes is
ciphered (first byte `0x80 | k`, k one of 64, a second byte, then
`length - 2`, then the bytes; only the long biographies are plain). So the
Create-a-Class names are the camos' categories: each bundle's cell after
the `0x0b` field is a 40-bit reference (`0x0f083fXXXX`) into the run of
category reference strings before the camos (`camo_category_spray`,
`jup_camo_category_02_frogs`, ...), one record per string of
`0x40 + align8(len + 1)` bytes (`jup_camo_category_pap` 0x30 more): every
reference lands on a record, and each category's bundle names its key
(`CAMOS/CATEGORY_SPRAY_PAINT`). Modern Warfare III's 22 categories follow
its family numbers (02 Frogs ... 26 Critters; 15a and 21a have no reference
and are Formations and Gradients by their family); Modern Warfare II's
series letters do not (series A is in several categories). Names:
`<CATEGORY> <n>`, n its place in the category (`weapon_camo_models::
JUP_CAMO_NAMES`, `IW9_CAMO_NAMES`): all 164 of Modern Warfare III's, 320 of
Modern Warfare II's; its 11 of `CAMOS/CATEGORY_O` (no name in the clear)
keep their picture's (`MWII K-04`).

Camo numbers, every game: Black Ops II's own weapon options below 100,
Modern Warfare 2's 101-108, 3's 121-133, Black Ops' 141-154, Modern Warfare
III's 300-463, II's 500-830, Black Ops III's 851-988 (`T7_FIRST`, docs/
T7_MAPS.md "Camos"), all under 1023 (`MAX_OPTION`, which the classes,
`calcweaponoptions` and `giveweapon` keep); 1039 is the guest guns'
Pack-a-Punch camo, past them. Fixed with Black Ops III's (20 Oct 2026):
the other games' patterns on Black Ops II's own guns now lie under its
camo mask (the colour map's alpha) at its own patterns' repeat, as its
own camos do, not over the whole material (metal, rubber and sights
too); a model with both kinds of surfaces draws each with its own row
(`render::extract`, rows 3 and 4); the camo in his hands in a killcam is
the killer's, not his own; BO2's menu scripts can no longer write over a
class's other-game camo (as they could not over a guest gun); each game's
camo tab lists all its camos (a Gold or Woodland of one game no longer
hides another's); the guest lanes' camos are kept apart per game, so one
lane's install never drops another's. Not done: a dropped gun on the
ground and a gun stowed on the back draw without their camo.

View placement: each gun's grip (its `tag_pistolgrip_attach`, plus a
1-unit slant) goes on its donor's grip, measured on `t6_wpn_*_view`, and
across the gun on the receiver mesh's centre line (not the tag's own y).
Its receiver top goes to the donor's sight height. Pistols are drawn 1.3x,
as Black Ops II's are, and sighted by their slides. `CodLegacy check-guns`
prints per gun the grip tag, the receiver's bounds and the offsets used
(`view grip tag ...`), and each built model's bounds.

Out of hand (21 Oct 2026, owner's screenshots of the KATT-AMR and the
Victus XMR, every MWIII/MWII gun): the gun drawn ahead of and to the right
of the hands, the stock large at the bottom right. Not found from the code:
the placement above puts the grip tag on the donor's grip if the tags and
meshes are read right, so the parts' bones and meshes need checking on the
files themselves (`cargo run -p fastfile_jup --example jup_gunfit --
[--save <dir>] <receiver> <part>=<tag>...`, which also writes the zones
decompressed for reading without the DLL).

Material picture tables: a material's own table (its +0x30 following)
starts after the header **and** what its earlier pointers put there: 48
bytes when +0x20 is -3, a byte pair per +0x18 count when +0x28 follows
(`map::materials::picture_table`; 49,446 of 49,561 following tables in
global_stream_mp then hold only zone pointers, against about 46,000 when
read straight after the header). This was why Ghost's shirt and many map
materials had no pictures.

Operators: a material's colour is its largest own usage-0x36 picture (not
one many materials share, such as `3622f42cb25a4aa1`, a lookup). Without
one, its constants hold its colour as `ff ff ff 00 r g b _` followed by
half-float ones (Ghost's trousers `2b2b2d`), shaded by its 0x32 (BC1)
picture. That picture is also its specular map's gloss. Format 0x2a is
BC4 (8-byte blocks, one channel), not BC1.

Parts go on the bone named as their own root (`tag_scope`, a bolt's joint),
on the receiver or a part already placed, else on their listed tag: scopes,
bolts and irons hang off tags such as `tag_scope`, not `tag_scope_attach`.
The root goes onto that bone wherever it stands in the part's own model
(the part moved by the bone's pose times the inverse of its root's). Put
on as if the root were the part's origin, a scope whose `tag_scope` is its
eye line, above its mount, floats by that height: the likely cause of the
KATT-AMR's floating scope, not yet checked on its files. A part whose root
is its origin is unchanged.

Gun materials: usage 0x32 (BC7) is a part mask (green the metal, magenta
the polymer, blue trim; red equals blue and alpha is red's inverse). The
0x36 picture is a wear and pattern layer, not the gun's colour: the
KATT-AMR's is a smeared tan with none of the receiver's outlines, which its
other pictures all show. Format 0x2f (usage 0x37) is packed: gloss in red,
the normal's y in green, occlusion in blue, its x in alpha (alpha averages
0.5 on every one dumped, red does not). Read as x y z, as it was, the
normal maps were gloss for x, and the guns and operators shaded wrongly.
The guns are coloured from the mask under that occlusion, and their
specular maps take its gloss (`jup_gun::gun_colour`, `specular_levels`).

Reflectance (21 Oct 2026, every gun drew as mirror chrome): Black Ops II's
lit draw reads a specular byte `v` as `raw_texel(v)^2` (its sRGB encoding
squared), so the bytes written (0x70 metal, 0x38 polymer) were reflectances
of 0.48 and 0.26 over a near-black colour, at the packed map's full gloss.
Each finish (`jup_gun::Finish`) now has its own: polymer a dielectric's
0.04 (byte 8-9, as the other guest lanes), coated metal 0.10, machined trim
0.20, a material without a mask 0.06 (not metal); gloss from the packed
map's red held under 0.45 (polymer) to 0.7 (trim). Colours a little
lighter (metal `44464a`, polymer `2e2e30`, trim `5c5d60`).
A `jup_dump` folder beside the game (empty, or holding one note) gets every
gun material's pictures written to it once.

Default parts: a receiver's attach tag with no part of the gun on it means
a part left out. Each candidate part's root bone (`fill` probe) says where
it goes, so the MWII guns got their pistol grips (Lachmann Sub, -556,
-762, Kastov 762, EBR-14, SP-R 208's), handguards, bolts, triggers and the
P226's hammer. Where a gun has none of its own its platform's is used (the
M16 the M4's grip, the Kastov-74u, RPK, Bizon and Vityaz the Kastov 762's,
the HCR 56 the Lachmann-556's). The SO-14's rifle stock carries its grip:
its hand goes just ahead of `tag_stock_attach`.

Modern Warfare II (2022) guns: its own install's zones are signed and
encrypted (`IWffs100`) and are not read. MWIII carries the MWII guns as
`mp_av_att_vm_pNN_<class>_<gun>_<part>_v0_tr` (world `mp_aw_att_wm_...`).
44 are played (`iw9_<class>_<gun>` in `GUNS` and `bo2_profile::jup_names`),
with their default `_v0` receiver, barrel, magazine, stock and grip. Their
pictures are `icon_weapon_<class>_<gun>`.

Played (`bo2_profile::jup_names::JUP_PLAYED`, `assets::lane::jup_gun::GUNS`):
the Renetti, SVA 545, MCW, BAL-27, FR 5.56, DG-56, STG44, MCW 6.8, BAS-B,
KAR98K, Pulemyot 762, DG-58 LSW, Reclaimer 18, Static-HV, RAM-9, KATT-AMR
and XRK Stalker. The other named guns are not listed in Create-a-Class, for
bots or in the box.

- Which zones: `global_stream_mp` names every transient zone of the install
  (17,787 `*_tr` strings in 1.21), the guns' part zones among them
  (`mp_av_[vm_]jup_jpNN_<class>_<gun>_<part>_<variant>_lod0_tr`, `mp_aw_[wm_]…`
  the world model's). A gun's default build was taken as its `rec_000`
  receiver and the plain `_000` part of each kind: `bar` (else the first of
  `barrel`, `bar_classic`, `bar_bull`, `bar_light`, `bar_short`, `bar_long`),
  the magazine or belt of the gun's default capacity, `pgrip`, `stock`,
  `guard`, `bolt`, and the sight (`irons` on marksman rifles, `scope` on
  sniper rifles). The MCW and MCW 6.8 share the MCW's (`acharlie`)
  magazines and `stock_assault`. The `NN` of a part need not be the gun's
  (the Renetti's receiver is `jp19`). Not checked by eye: a part on a tag the
  receiver lacks is left off and named by `CodLegacy check-guns`.
- Guns with no `rec_000` (only blueprint receivers, `rec_v####` /
  `receiver_*`): Holger 556, Holger 26, Striker, WSP Swarm, Riveter,
  Lockwood 680, Sidewinder and others; not played yet.
- Handling: a Black Ops II donor per class (`an94_mp`, `hk416_mp`,
  `scar_mp`, `xm8_mp`, `type95_mp`, `tar21_mp`, `ballista_mp`, `mk48_mp`,
  `qbb95_mp`, `saiga12_mp`, `pdw57_mp`, `mp7_mp`, `svu_mp`, `dsr50_mp`) with
  the gun's fire mode, magazine and rate of fire (MW3 2023's, as played);
  bolt actions and semi-automatic shotguns and snipers keep the donor's
  cycling (`fire_time_ms` 0).
- Sounds: the other guns' aliases follow the Renetti's naming
  (`wpn_<class>_<gun>_fire_npc`, `_fire_plr_transient`, `_fire_plr_last_mech`,
  `wfoly_plr_<class>_<gun>_raise_01` ...); `ingame_shared_mp`'s sound
  packages hold the fire hashes of all but the SVA 545 (its package names
  them otherwise). A name the install lacks keeps the donor's sound
  (`asset_audio::jup_gun_has`).
- Parts: a gun is a receiver and the attachments it carries, each its own
  model in a `_tr` zone: `mp_av_*` the view model's, `mp_aw_*` the world
  model's (the Renetti: `jp19_pi_mike93_rec_000` receiver, `bar_000` slide
  and barrel, `mag_15_9mm_000`, `pgrip_000`). A zone is the mesh (world
  zones: one per LOD, then a detail collision), the model, its skin
  variants and a streaming info.
- Mesh positions are 21-bit fractions spanning the largest extent either
  side of the centre, in the model's space; the parts fit the receiver
  only so. Bones: per bone a 32-byte pose in the model (quaternion,
  position, 2.0), then the FNV-1a 32 hashes of their names (`j_gun`,
  `tag_barrel_attach`, `tag_mag_attach`, `tag_pistolgrip_attach`,
  `tag_flash`, `tag_brass`, ...). A part's root is at its origin and goes
  on the receiver's tag of its kind.
- Materials: one pointer per surface, -1/-2 for a 0x78-byte reference stub
  after the pointers (the material's hash), else a pointer back to a
  material loaded before (not read: drawn as dark gun metal). Gun
  materials are defined in `global_stream_mp` (one pass for the header
  word `c0 b6 73 2c`); their pictures are asset-list entries there, the
  list's place found from the pointers themselves. A gun material's
  colour is a tint in its constants (row 6, sRGB bytes), the game laying
  camo and wear over it; its pictures are a normal map (format 0x2f, BC7,
  x y z), an occlusion picture (0x30, usage 0x36, occlusion in alpha) and
  masks. The colour map built is the tint darkened by the occlusion.
- In the match the gun is a copy of a Black Ops II gun (the B23R) with its
  own models, name and handling (semi-automatic, 15 rounds): Black Ops
  II's view animations, arms, effects and HUD pictures
  (`WeaponRegistry::borrowed_namespace_of`). The models are one bone,
  `tag_weapon` at the receiver's grip tag, with `tag_flash` and
  `tag_brass`.
- Its sounds are MW3's (`asset_audio::JUP_GUN_SOUNDS`, read from `ingame`
  into every match's bank when the install is set). The gun's sound
  package (`jup_pi_mike93_sfxpackage`, type 93, `ingame_shared_mp`) lists
  alias hashes; those aliases are in `ingame` (the zone every mode loads),
  their names recovered by hashing: `wpn_pi_mike93_fire_npc` (mono, eight
  variants) -> `_fire_npc_mech`; `wpn_pi_mike93_fire_plr_transient` ->
  `_shot` -> `_mech` -> `_lfe` (each record names the next layer; the
  long tails `_fire_plr_atmo` / `_fire_npc_atmo` are added after the last);
  `_fire_plr_last_mech` (the last round) and `_first_*` chains; foley
  `wfoly_plr_pi_mike93_{raise,drop}(_quick)_01`, `_reload_{magout,magin,
  slide_release,hand}`, `_reload(_empty,_fast,_drum)_NN`, `_inspect_NN`.
  The copy keeps the B23R's animations: its notetrack sounds are replaced
  by word (`GuestCopy::notetrack_remap`: mag out / in, slide, hand), slots
  name `jup:#<hash>` (`asset_game::alias_namespace`). Each layer plays as
  the secondary of the one before.
- Not read: Modern Warfare III's view animations (animpkg, xanims), its
  weapon numbers, and two of its sounds: `wpn_pi_mike93_ads_up` /
  `_ads_down` (their records point at other aliases' headers, not a
  sndasset) and `weap_pickup_pistol_npc` (in `ingame_shared_mp`): the
  B23R's play there.
- `CodLegacy check-guns` (`BO2MP_CHECK_GUNS=jup`) reads and builds the models.
- Scopes: snipers and the marksman rifles built with one carry their
  `scope` part (`_scope_000` / MWII's `_scope_v0`, on `tag_scope`); aimed,
  their donor's Black Ops II scope. MWII's Victus XMR, MCPR-300, Carrack .300,
  SP-R 208 and LM-S had none (checked by assembling them, `gunobj`).
- Attachments (`bo2_profile::guest::JUP` words, Black Ops II's own): a
  sight is a copy of the donor's sight variant (`an94_mp+reflex`) with the
  gun's models and handling (the sight model at the donor's placement,
  over the receiver top the gun is lined up to), the others Black Ops II's
  numbers on the gun and its donor's suppressed shots. No suppressor or
  grip model is drawn on these guns yet.

## Operators (not played yet)

Their part zones: `mp_bd_body_c_jup_mp_<operator>[_<skin>]_lod0_tr` (691
bodies) and `mp_hd_head_c_jup_mp_..._lod0_tr` (1,183 heads), listed in
`global_stream_mp`'s transient zone list; e.g. Ghost (`ghost_10`) and
Makarov (`makarov`). A zone is several LODs' meshes, then the model (its
header as a gun part's: +0x10 every LOD's surfaces together, +0x12 the LOD
count; Ghost's head 23 over 6 LODs, 4 of them in this zone) with the
skeleton after the name: a u16 parent list, compressed rotations (rows of
`xx xx 82 5a`), then f32 positions. Read so far: the meshes
(`surfs::parse`) and the header. Not read: the skeleton's names and
poses, and the skin weights.

- Skinned surfaces have flag 0x0002 and a stream 11 (rigid ones a rigid
  range to one bone): per vertex a u32, its low 16 bits a bone index, its
  high 16 bits 0 (one bone) or a weight/count (`0x7d70`, `0x3d00`,
  `0xffff`); more (bone u16, weight u16) entries follow the per-vertex run
  for the vertices of more than one bone. Ghost's body, densest mesh,
  surface 1: 1,889 vertices, 524 on bone 5 alone.
- To play them: the skeleton by name (`j_` bones; their FNV-1a 32 hashes
  as the guns' tags), the weights, then the body built on Black Ops II's
  player skeleton by bone name so its animations drive it, lit as the
  guest guns are; and a setting for Black Ops II's soldiers instead.

## The small copy (Options > GAME FOLDERS, X on Modern Warfare III's row)

For keeping the guns without the whole game, the front end makes a copy of
the install holding only what they use (`ui::jup_pack`,
`fastfile_jup::pack`):

- every played gun (Modern Warfare III's and II's) is read and built as a
  match does while every zone read and package entry decoded is noted, and
  every level of a picture up to the largest asked for (so TEXTURE QUALITY
  still works on the copy);
- the gun sounds' package entries are taken from their list (a sound is
  only decoded when played), with the zones they are listed in;
- the zones noted are copied whole; each zone `.xpak` with entries noted is
  rewritten over one part holding those entries, byte for byte (the part
  is a package of its own too, as the sound reader looks in every `.xsub`);
  the Oodle DLL is copied beside them;
- the calling cards and emblems are saved decoded at the sizes shown
  (`looks/<name>.png`, the frames one under another, listed in
  `looks/looks.txt`), as their pictures in the game are far larger;
- `bo2mp_pack.txt` marks the copy, and Modern Warfare III's folder is set
  to it.

The readers open the copy as they open the install. The maps, Warzone
(Resurgence) and the operators are not in it: they need the full install.

## Shaders, techniques and materials (27 Oct 2026)

Target (owner, 27 Oct 2026): Modern Warfare III content must look the way
Modern Warfare III itself renders it, not in Black Ops III's style. The Black
Ops III native path (`docs/T7_NATIVE.md`, `render_t7`, `dxbc_sm5`) is only a
structural example (program, bindings, draw). Shading model, layer system and
tonemap are to come from MW3's own programs and constants.

Code: `fastfile_jup::shader` (`scan` finds the containers in a zone image and
reads stage, resource bindings and signatures), probe `cargo run -p
fastfile_jup --example jup_shaders -- <code_post_gfx.zone> [--all | --dump <n>
<out.dxbc>]`. Decode the zone first (`code_post_gfx.ff` is 54.6 MB, 115.9 MB
decoded; it is NOT `eng_code_post_gfx.ff`, which is localisation text).

Where: all of the game's programs are in `zone/code_post_gfx.ff`: 2,476
containers, 53.2 MB. None in `global`, `ingame`, `ingame_base_mp`, `ingame_br`,
`ingame_shared_mp`, `global_stream_mp`, `eng_code_post_gfx` or `code_pre_gfx`
(no `DXBC` at all); map zones were not scanned (their materials reference the
library, they do not carry programs). Format: DXIL, shader model 6 (the
install ships `dxcompiler.dll` and `dxil.dll`), in a plain `DXBC` container,
stripped: 2,450 hold `SFI0 ISG1 OSG1 PSV0 HASH DXIL`, 20 add `PSG1` (hull and
domain), 6 are `SFI0 RDAT HASH DXIL` (libraries). Stages (PSV0): 1,205
compute, 945 pixel, 300 vertex, 17 domain, 3 hull. Most compute programs are
preceded by an hlsl path in the clear (`cs_ssr_msaa.hlsl/cs_ssr_trace`,
`cs_post_opaque_processes.hlsl/...`, `cs_virtual_texturing.hlsl/...`); pixel
and vertex programs are unnamed. Resource and function names are stripped from
the bitcode (only about 50 programs keep a few, e.g. `LookupRawLightGrid`,
`GfxSHProbeFallback`, `LookupNeuralLightGrid`).

What the reflection says (all pixel/vertex programs share one root layout):
- Samplers `s0..s5` and a 4-array `s7..s10`; constant buffers `b2`, `b3`
  (and `b5`) in space 0: per-view and per-draw only.
- Engine textures are fixed slots in space 2 (`t0..t89`: 2D, 3D, 2D array and
  cube resources: shadow, light-grid, probe and screen-space inputs). Raw
  buffers (`ByteAddressBuffer`) in space 1 (`t0..t79`) carry the scene data.
- Material pictures are bindless: unbounded ranges `0..270335` of Texture2D
  in spaces 3 and 4, Texture2DArray in space 6, Texture3D in space 7. So no
  picture usage (0x32, 0x36, ...) is a binding slot; a program picks pictures
  by descriptor index read from a raw buffer (the material table, uploaded
  from the material's constants and picture table). Which usage feeds which
  term is therefore only knowable from the DXIL bitcode (where the index is
  loaded and how the sample is used), not from reflection.
- Vertex programs pull vertices (`SV_VertexID`/`SV_InstanceID`, raw buffers);
  they output `SV_Position`, `GPU_PIPELINE_ID`, `COLOR`, `TEXCOORDS`,
  `NORMAL`, `TANGENT`, `IRRADZ_COMPFCT`, `HIGHLIGHT_DIR_VIS`,
  `SECONDARY_TEXCOORDS`, `SCEONDARY_NORMAL` (sic): a second UV set and normal
  for layered/blended surfaces; vertex colour is a blend input.
- Pixel programs write 1 target (636 programs), 2 (232), 3 (40) or none (37),
  nearly all float4; a few write uint targets, `SV_Depth` (9) or
  `SV_Coverage` (27). Material pixel programs are 50-115 KB of DXIL each
  (ubershaders keyed by the pipeline id), so a material does not own a program
  in the zone: the library is small and the material picks by technique.

Materials (maps' and guns'): the 0x78-byte header's +0x20, +0x28, +0x38 and
+0x40 are, in most materials, back-pointers into block 0x0f a few bytes apart
(shared records, not asset-list entries); +0x30 is the picture table. They
were not decoded.

### The programs read (`dxil_sm6`, probes `dxil_dump`, `dxil_raw`)

`crates/dxil_sm6` reads the LLVM 3.7 bitcode in the `DXIL` part: the
bitstream (blocks, abbreviations, records), the module (types, globals,
constants, metadata, function bodies with relative operands) and prints it as
text. All 1,271 pixel, vertex, domain, hull and library programs of
`code_post_gfx` read without error. Shader models: pixel 6.1, vertex, domain,
hull and compute 6.0, libraries 6.5. Names survive where they matter: function
and struct names (`dx.op.*`, `%hostlayout.scene`, `%perObject`, `%tonemapping`,
`%struct.GfxWeatherData`, `%struct.WeatherMaterialsBuffer`), the entry name,
the hlsl file of compute programs. Stripped: resource names (`""`), cbuffer
member names. Every graphics program is one function `ps_main`/`vs_main` with
everything inlined, shader-model-6.0 operations only (`createHandle`,
`bufferLoad`, `rawBufferLoad`, `cbufferLoadLegacy`, `sample*`, `textureLoad`,
`dot2/3/4`, `legacyF16ToF32`, `discard`, `loadInput`/`storeOutput`, a few
wave/ray ones in libraries and compute) and ordinary LLVM instructions: the
translator's input is small.

The cbuffers are `scene` (b2, `GfxCodeSceneConsts` and friends: view, sun,
fog, weather, water, volumetric, glint), `perObject` (b3), `perPrim`,
`tonemapping`.

### The material record

A material pixel program's `GPU_PIPELINE_ID` input is the **byte offset of the
material's record** in a `ByteAddressBuffer` (`t39`, space 1); every program
reads its fields with `bufferLoad(buffer, id + constant)`. Seen so far:
- +44, +48, +172, +236, +240, +292: packed half pairs (`& 65535`, `>> 16`,
  `legacyF16ToF32`) and shifted fields: texture-coordinate scale/rotation,
  colour scales, blend ranges;
- +484 and one more dword: feature flag bits (`0x10`, `0x40`, `0x200`,
  `0x100000`, `0x20000`, `0x4000000`, `0x8000000`, ...) that switch layers
  on and off;
- +516 to +660 step 4: up to 37 texture indices; a value above 270335 means
  none (0); the index picks a `Texture2D` of the unbounded bindless range
  (`createHandle(class 0, range 0, index)`).
The record is built by the engine at load from the material's header, its
constant rows and its picture table; it is not on disk.

Layered surfaces (the 4-layer forward programs, #2116 and #2206-#2212 of the
library's order): texture slot `i` of layer `l` (0 to 3) is, by groups of
four: 0-3 colour (RGBA; alpha scaled and offset by two record bytes), 4-7
packed normal (below), 8-11 a 3-channel map times a record colour, 12-15 a
1-channel map compared with 0.5019608 (128/255), 16-19 a 1-channel map
(`1 - x`), 20-22 single maps on the base coordinates, 23-26 a 1-channel map
(1.0 when absent), 27-30 3 or 4 channels, 31-34 4 channels scaled by 1/255. A
second texture-coordinate set and normal (`SECONDARY_*`) and the vertex
colour blend the layers.

**Normal maps** (usage 0x37, format 0x2f), decoded the same way in every
program: green `g` and alpha `a`, strength `k` a record byte / 255:
`p = ((g - 0.5) k + 0.5) 2 - 1`, `q` the same from `a`; `x = (p + q) / 2`,
`y = (p - q) / 2`, `z = 1 - |x| - |y|`, then normalised. Red holds the gloss
(`red * byte + byte`, / 255) and blue the occlusion (1.0 when the layer has
no map). Measured on 6 decoded normal maps: with this decode `z` is never
negative and the mean `z` is 0.69-0.92; with alpha as x and green as y (the
earlier reading) 0.1-4% of the texels are not on the unit disc. The gun and
operator pipeline now decodes them so (`jup_gun::unpack_normal`).

Colours are blended in a gamma-2 space: the colour sample is square-rooted
(`x * rsqrt(x)` through the `0x5f3759df`-style bit trick), then
`(sqrt(c) - A) * B + C` per channel (`A`, `B`, `C` record bytes and halves),
saturated, then squared back.

The display stage (the `tonemapping` cbuffer: 16 floats and 4 ints; e.g.
program #1239, 742 lines): scene colour times an exposure, then by a mode
word either a 3D lookup table (`log2(x + 1/1024)` scaled into a 3D texture),
the sRGB transfer (`1.055 x^(1/2.4) - 0.055`, scale and offset from the
cbuffer), or a rational-polynomial fit of the PQ curve after a BT.709 to
BT.2020 matrix (0.6274 0.3293 0.0433 / 0.0691 0.9195 0.0114 / 0.0164 0.0880
0.8956). Not ported yet.

Material on disk (`jup_matdump`, `jup_slotstats` on `global_stream_mp`): a
material with a following picture table carries, before the table, a list of
`+0x18` (33 to 39) byte pairs `(slot id, picture index)`, and its picture
table holds shared default pictures (`f091436d9`, `f09148f41`, `f091436d1`:
white, flat normal, black) for the slots it does not set. The slot ids are
the same list for most materials (0 to 0x13, 0x1b to 0x21, 0x26 to 0x29,
0x32, 0x3d, sometimes 0x14, 0x22 to 0x25, 0x33, 0x34), but what a picture is
does not follow from its id: slot 0 holds a normal map (usage 0x37) in 527
materials and a colour map in 345; the id is a place in the material
type's own parameter table, so the role comes from the picture's usage (0x32
colour/mask, 0x36 colour/wear, 0x37 packed normal, 0x33 one-channel mask,
0x31 constants) and the material type (a shader family, not read yet).
Gun materials seen: usage 0x32 BC7 (part mask), 0x36 BC7 (wear), 0x37 (normal)
and one 0x31 2048x2048 format 0x28 picture.

Not done (blocked on the material type): which program a material uses (the shared records at +0x20/+0x28/+0x38/+0x40 hold float data, not a program id), the record's full layout, the gun and operator layer and camo terms. The guns, operators and maps keep their inferred colour inputs (`jup_gun.rs`: usage 0x32 part mask, 0x36 wear layer) and now the real normal decode.

Plan (1 is done: `dxil_sm6`):
1. LLVM bitstream reader for the DXIL part (wrapper header, then the module:
   abbreviations, blocks, records; types, constants, functions, metadata
   `dx.resources`/`dx.entryPoints`). Emit an instruction list per program.
2. Read one gun pixel program: find the raw-buffer loads that produce
   descriptor indices and constants, name each by its use (base colour, layer
   mask, normal, gloss/metal/AO, camo) and tie them to the material's
   picture usages and constant rows. Cross-check with decoded pictures.
3. Translate: a new `dxil_sm6` crate lowering the bitcode (DXIL ops are
   `call @dx.op.*` with LLVM 3.7 instructions) to naga IR, validated with
   naga offline, in the spirit of `dxbc_sm5`. Reimplemented, not vendored
   (`dxc` and SPIRV-Cross-derived code are not taken without a licence
   check). Bindless textures map to a texture array/binding array in wgpu.
4. Until then, render guns, operators and maps through the existing material
   path with MW3's own look (layer system, tonemap) as the target, not
   Black Ops III's.

### Which program a material uses: what the zones hold (27 Oct 2026)

Code: `fastfile_jup::shader` (`Program::key`, `groups`, `is_bindless`,
`pixel_outputs`), `fastfile_jup::map::materials::{header, slot_pairs}`,
`dxil_sm6::analysis::profile`. Probes: `jup_library <code_post_gfx.zone>
[--all]` (one line per program: key, outputs, samples, constant offsets read
from each buffer), `jup_zonetypes`, `jup_mathead <zone> [off:len,...]`,
`jup_slottype`, `dxil_res` (ranges with type names).

**There is no stored link from a material to a program.** Checked:
- No program, program record or tail hash of `code_post_gfx` (VS records end
  in an 8-byte hash, PS tails hold 8-byte hashes) occurs in any other zone
  (`gun1`, `global`, `global_stream_mp`, `ingame*`, a map zone): zero hits.
- Pointers inside `code_post_gfx` do not visibly lead to program records
  either: 4,000 of its 49,415 distinct back-pointer values were tried against
  every record start (and 32 and 64 bytes before) with every shift within
  +-200 MB; no shift is found by more than 6 pointers. The pointer every
  vertex record ends in, read as a file offset, falls inside a ray-tracing
  container, so block offsets are not file offsets in this zone.
- A material header (0x78 bytes) holds a hash, flags, counts and pointers to
  its own picture table and constants; nothing in it names a technique or a
  program. The `jup_slotstats` slot ids do not fix a picture's role (usage
  0x37, 0x32, 0x36, 0x33 all occur in every slot, also inside one header
  class), so the roles are the program's, chosen by the engine's own tables.
The engine computes the choice from the material's state at load (it also
computes the record in `t39`/`t47`, which is not on disk). Those tables are in
the packed executable, which is not analysed; a renderer must choose by the
keys below and by its own pass logic.

**What each program record says about itself.** The 64 bytes before a
container (unnamed programs; the post-process programs have their hlsl name
there instead) are eight u64: `[size flags, pass mask, family, feature mask,
0, 0, -4, container size]` (`ProgramKey`). On the 567 material pixel
programs (bindless):
- `family` 12: 341 programs (standard surfaces), reading the material record
  in raw buffer `t39` space 1 (266 of them; the rest are depth/coverage
  variants without material reads). `family` 44: 199 and `45`: 13, reading a
  larger record in `t47` space 1 (a second material kind with more layers; up
  to byte 944 of the record). `family` 0, 2, 8, 9, 13: 14 programs (tonemap
  and debug-like variants).
- `pass mask`: one of nine bits, `0x1` to `0x100`, rarely two. Bits `0x2` and
  `0x10`, `0x4` and `0x20`, `0x8` and `0x40` have identical program counts and
  output sets, so the nine are probably three groups of three. Outputs by
  bit on family 12: `0x1` 1 or 2 targets; `0x2`/`0x10` 2 targets (mostly);
  `0x4`/`0x20` and `0x8`/`0x40` 1 target; `0x80` none, 2 or 3 targets (no
  coverage); `0x100` the same with `SV_Coverage` (or 1 to 2 targets). The
  bit is the engine's pass id; its name is not stored.
- `feature mask`: always `2^k - 1` on family 12 (k = 0 to 6: 0, 1, 3, 7, 0xf,
  0x1f, 0x3f), `2^k - 1` (k 6 to 12) or `0x100 | (2^k - 1)` on 44. Samples
  per program run from 1 to 128. The meaning of a bit (a layer, a detail
  map) is not stored.
- The grouping is by position: 300 vertex programs, each followed by 0 to
  45 pixel programs (`groups`). Only 53 vertex programs output
  `GPU_PIPELINE_ID` (the surface vertex programs, 16 distinct output
  signatures: with `IRRADZ_COMPFCT`/`HIGHLIGHT_DIR_VIS` (lightmapped), with
  `SH_RED/GREEN/BLUE` (probe lit), `CLIP_POS`/`PREV_CLIP_POS` (velocity),
  `TRIID` (visibility)); 203 bindless pixel programs follow them. The other
  364 bindless pixel programs belong to effect, water and post vertex
  programs (`COLOR0,TEXCOORD0,TEXCOORD2..` and so on).

**The material record the family 12 programs read** (raw buffer `t39` space
1, byte offsets; the number of the 266 programs that read it): constants at
36, 40 ... 412 in steps of 4 (136 programs read each), flags at 484 (191)
and 500, then picture indices at 516 to 692 in steps of 4: 516 to 544 (112),
548 to 660 (136 for most), 664 to 676 (24), 680 to 692 (6). So the record has
45 picture-index entries, of which the first 37 are the common ones (a value
above 270335 is none). A material has 33 to 49 slot pairs (`slot_pairs`:
`(slot id, picture index)`, ids ascending): the record's picture entries are
very probably filled in the pairs' order (45 entries against up to 49 pairs;
the pair count class `0x21` is 33 pairs, `0x25` 37), but that is not shown:
which entry a program decodes as a normal map is not tied to a usage by the
data (needs data flow per program). The record format of the surface
vertex programs (`t23`, record stride 64 from byte 786464) and the pipeline
id are the engine's too.

**Material header classes (global_stream_mp: 51,329 materials, `jup_mathead`).**
`+0x18` is the slot-pair count (`0x21` = 33 in the great majority of the
common classes, `0x25` in some), `+0x19` is 30, 32 or 37 (`0x1e`, `0x20`,
`0x25`) on materials with pairs, `+0x1b` pictures (5 to 11), `+0x1c` constant
rows (10 to 18). `+0x0e` is a bit set from `{0x01, 0x08, 0x10, 0x20, 0x40,
0x80, 0x100}` (`0x68` with `+0x15/+0x16` = `02 83`: 17,679; `0x20` with `02
02`: 8,952, with `02 82`: 3,839; `0x158` with `02 03`: 2,188; `0xc8`,
`0x21`, `0x38`, ...); `+0x16` has bits `0x80` and `0x01`. These are the
candidates for what selects family and feature mask; there is no proof, so no
function maps one to the other here.

### Neural light grid: what the programs need (27 Oct 2026)

`LookupNeuralLightGrid` is inlined into 41 compute programs (none is a pixel
program): `cs_sample_neural_lightgrid` (#760: resamples the grid into two
`RWTexture3D`, float3 and float4, and a `RWStructuredBuffer<uint2>`),
`cs_del_sh_sample_neural_lightgrid` (#188, 190 to 233: SH probes, several
variants incl. reflection volumes),
`cs_vol_sh_sample_neural_lightgrid` (#1098 to 1108), `cs_light_sensor_neural`
(#591, 592), `cs_lgv_sampling_sample_neural_lightgrid` (#587), plus the
`LookupRawLightGrid` / `GfxSHProbeFallback` twins (#106 to 185, 586, 759,
1082 to 1094). They read these buffers (the same ranges in each; `dxil_res`):
- `t0` `StructuredBuffer<GfxNeuralLightGridTopLevelNode>` (16 bytes: 4 x
  i32) and `t2` `StructuredBuffer<GfxNeuralLightGridInternalNode>` (16 bytes:
  2 x uint2): a tree (a top level of nodes, internal nodes below) over the
  grid's cells.
- `t1`, `t3`, `t4`, `t5`, `t7` `Buffer<int>`; `t6` and `t8` `Buffer<float4>`
  (`t8` is read 29 float4 an entry: elements 1 to 9 and 24 to 28 of each
  block, probably the network's per-cell parameters); `t9` raw buffer (read at
  byte 16 of its records); in #587 raw `t12` (64-byte records, words at 16,
  32, 52, 56, 60) and `t13` (32-byte records, words at 12, 28);
  `Buffer<uint>` `t10`, `t11`, `t14` to `t19`.
- constant buffer `b4` `GfxNeuralLightGridSetupConsts` (80 bytes:
  `float3, i32, float3, i32, float3, i32, int3, i32, float, i32, i32, i32`:
  two float3 (box?), a third float3, grid dimensions as int3, a scale, three
  ints), `b0`/`b1` the sampling consts.
All of these are `Buffer`/structured resources the engine uploads from the
map's data; no zone-side layout of the arrays was found. The map zone holds
the raw light grid's header block (found by `lighting::read`, now
`Grids::block_at`, probe `jup_lightgrid`): four pointers (mask, 2-D bytes,
3-D bytes, six-way grid), the box (min at +0x20, max at +0x30), 3 x
(-0.074) and 3 x 1.158 (the six-way grid's value range), floats not decoded,
the texel counts at +0xF0 (552, 528, 69, 66, 100, 92 as floats on Rust) and
an 8-float vector (2.480, -0.040, 0.337, 0.151,
0.312, -0.243, -0.045, -0.028) twice. The neural arrays are not among the
images (all buffers above are buffers, not pictures), and no array of 16-byte
nodes was found by shape in the Rust zone. They may be in the streamed
packages, which were not searched. **Not implemented**: no evaluation of the
network exists; the lowering (`dxil_sm6::wgsl`) does not yet cover compute
programs (UAV stores, group-shared memory, barriers).

## Effects (vfx) — container read, numbers not read (27 Oct 2026)

Code: `fastfile_jup::vfx`, probe `cargo run -p fastfile_jup --example jup_vfx
-- <zone image or .ff> [--install <dir>] [--from n --to n] [--named]`.
Nothing here plays an effect yet; no `BO2MP_JUP_NATIVE_FX` switch exists
because there is nothing behind it.

**Where they are.** Effects are not KAPI entries (no effect path or name
hash among 59 tried, in several spellings, is a key of any of the 1,039,222
`.xpak` rows; the `.xsub` data itself was not available) and not entries
of the asset list under a type the crate names (`VFXPKG` 94 is something
else: in `ingame_shared_mp` the strings before a run of `-2` pointers are
effect paths and the run ends in vision-set names, a precache list). They
are in the zone stream, inline, as depth-first structs; `mp_jup_rust` holds
164 (its scriptables' `vfx\iw8\veh\scriptables\x.vfx` strings name 23 of
30 of them), `ingame_shared_mp` ~5,800, `global_stream_mp` ~9,500. An
effect's name hash is `hash::name(path without ".vfx")`, found stored
before each effect (checked: the path strings of Rust's scriptables hash to
effect headers in the zone).

**Layout read** (`vfx.rs` doc comment has the table): a 0x10-byte header
(hash, `-2`), a 0x78-byte root (flags, emitter count, two LOD distances,
bound 100000), `emitters * 0xC0` bytes of emitter records, then per emitter a
0x48-byte detail with two lists of 0x20-byte entries: **modules** (id 0..37)
and **parameter groups** (id 42..65). An entry is a pointer and an id. `-2`
or `-1` means its data follows the list; a pointer with block `0x0f` is a
back-reference to identical data stored earlier (the stream is
deduplicated: 5,300 of 5,900 effects reference module 1 and 5,900
reference module 29, each by offset into a pool). Check: an effect with one
emitter whose entries are all back-references is exactly
`0x10 + 0x78 + 0xC0 + 0x48 + 0x20 * entries` bytes; 260 of the 335 such
effects of `ingame_shared_mp`'s effect region (and 5 of 5 in Rust) end
where the next effect begins (the rest have other data between).

**Module payloads.** Parameter groups hold 16-byte values `{ptr, u32 mode,
f32 value}` after a 16-byte head (mode 2: a constant `value` with the shared
scalar object `0x06677570`; modes 5 and 6 with another reference are
curves); group 42 is 144 bytes, 45 is 176 bytes for the common case, module
17 is 64, 26 is 128. Their meaning (spawn rate, life, size, colour, flipbook,
lights, the material/image hash fields) is not decoded, and the
back-references cannot be followed without the sizes of every payload in
the stream (block `0x0f` offsets advance by payload size; the offset to
zone position delta changes between effects). Not found: where a map
places its ambient effects. Rust's entities carry no effect class (1,597
entities: path nodes, script structs, triggers, models), no effect hash
occurs elsewhere in the zone, so placements are probably created by the
map's script.

Stuck on: the module payload layouts (~60 ids), which need the full-stream
walk to resolve back-references; then the particle simulation and the
renderer, which should follow MW3's own modules, not BO3's effect model.

### Effects: what the second pass found (27 Oct 2026, same decoded zones)

- **Entry grammar.** After an emitter's detail, depth first: the module list
  (`count * 0x20`), then the data of its inline entries in list order, then
  the parameter list, then its inline entries' data. Checked on every
  one-emitter effect whose module entries are all back-references: the
  parameter list follows at `list1 + 0x20 * modules` and, when all its entries
  are back-references too, ends the effect (`rest = 0x20 * parameters` for
  every one of ~660 such effects, parameters 0..13).
- **Pointer `-1` marks data that is referenced again later** (the
  deduplicated pool): the distinct back-reference targets of a module id
  are its `-1` objects, `-2` objects are used once.
- **Block 0x0f is per zone and counts the zone's own objects, 16-aligned.**
  The offsets in `ingame_base_mp` run 13..20 M, `ingame_br` 25..35 M,
  `ingame_shared_mp` 107..141 M, `global_stream_mp` 182..213 M, `global`
  2..20 M; adjacent objects of one definition differ by their size rounded to
  16 (module 42: 0x90, module 45: 0xB0 in every adjacent pair seen). The
  delta to the zone position is 2.0 M at the start (the end of the asset
  list) and drifts to 24 M at the effects of `ingame_shared_mp`, so a
  back-reference cannot be turned into a position without the sizes of
  everything between (data of other blocks interleaves).
- **Small offsets are the zone's early assets.** Module 17's element is a
  back-reference to a small offset (`0x1c5c80`, `0x1c5de0`, `0x1c6220` ...
  rising with each effect): at `offset + 2,017,302` (+ a drift of
  16..46 bytes) lie asset headers (`hash`, `-2`), so module 17 is the
  pointer to the effect's sprite material or image.
- **The emitter record (0xC0 bytes, no pointers but +0xB8)** holds plain
  numbers; the values across 17,700 emitters (`ingame_shared_mp`):
  `+0x00` u32 1 (2 for 20); `+0x04`/`+0x08` u32 pairs (1, 5, 10, 100, 6, 20);
  `+0x0c`/`+0x10` f32 (0.04, 0.1, 0.3, 1, 3, 2); `+0x14`/`+0x18` f32 mostly 0
  (0.05, 0.02); `+0x1c`/`+0x20`/`+0x24` small u32 (1..6, 0xffffffff);
  `+0x28..+0x34` f32 (0.2, 0.05); `+0x3c` 10000, 2500, 40000, 225 (squares of
  100, 50, 200, 15); `+0x40` 250000, 4e6 (squares of 500, 2000); `+0x44`,
  `+0x58` 200, 1000, 100, 500, 50, 150; `+0x60` flags (2, 0x100002, 0x2002,
  0x802); `+0x78`/`+0x7c` f32 0.1 (0.75 / 1.25); `+0x80` a u32 tag of ~80
  values; `+0x8c` u32 (1, 19, 0, 4, 260); `+0x90` -2 for 1,183; `+0xa8` -1.
  Their meaning (spawn counts, life, cull distances) is inferred from the
  values only, not read from a running game: unverified.
- **What would unblock the rest:** the sizes of the module payloads (about
  60 ids with nested lists of the same 0x20-byte entries), which could only
  be learned from the retail loader (the executable is packed and is not
  to be analysed) or from many more worked examples; and a way to place
  the maps' ambient effects (not in the entities, not in any asset
  reached).
### Effect stand-ins (27 Oct 2026)

Until MW3's own effects play, each one asked for takes an effect of a game we
already play properly: Black Ops III (T7 lane) or Modern Warfare Remastered /
Horizon MW (H1 lane). Code: `assets::lane::jup_fx_standin`; data:
`crates/assets/data/jup_fx_standins.tsv` (columns explained in its header).

- **Table.** `override` rows (a whole MW3 path), `rule` rows (words that must
  all start a token of the path: `muzzle+sniper`, `impact+water`), then one
  `default` per category. First override, else first rule, else the default.
  The category is the caller's (a gun's muzzle flash) or
  the path's keywords (`Category::classify`). A source effect is `=name` (exact; the
  impact rows use Black Ops III's own `ImpactFx` table names such as
  `bullet_small_enter`) or `~a+b` (the source's shortest effect holding both
  words, picked when that game's effect list is at hand). `-` means no good
  match.
- **Source-agnostic.** Adding a game is a new `Source` and rows.
- **Asking.** `jup_fx_standin::request(site, name, category)` gives the
  stand-in or logs the effect once (`bo2mp jup fx: no stand-in for ...`) and
  returns `None`: the caller skips it. Hooked in the guest-gun rows
  (`jup_guest_rows`: muzzle flash, shell eject, bullet impact or a launcher's
  explosion per gun, named `weapon/<group>/<gun>/<slot>` because the guns' own
  effect paths are not read) and the map load (`entity_effects`: entity keys
  `fxid`, `fxdef`, `script_fxid`, `vfx`). The load report gets matched counts
  by category and the unmatched list.
- **Bound (gun slots).** `build_standin_fx` (called from `build_jup_guest_guns`)
  asks the table for every roster gun's flash (view and world), shell eject
  (view and world), bullet impacts and, for launchers, the explosion, then
  loads only the effects picked: H1 ones from the retail `common_mp`,
  `patch_common_mp`, `common_core_mp` zones (`hmw::standin_effects`, child
  effects included), T7 ones from the common BO3 fastfiles
  (`T7Map::shared_only`, `t7_map::shared_effects`; impact tables give 31
  surface + 4 flesh cells per bullet type 1..5). They land in `JupGuests.fx`,
  join the match catalog in the common cache (`t5_fx.absorb`, models joined),
  and `jup_guest_rows` sets each gun's `WeaponCombatFx.standins`
  (`GuestEffects`): `present_bound` returns the stand-in first, and
  `own_impact_for` serves the impact cells, so the BO2 donor's effects no
  longer play on a JUP gun when a stand-in was found. A slot with no stand-in
  (`-` rows) keeps the donor's.
- **Checked against the real lists.** Every `=name` in the table is an exact
  name in the H1 or T7 effect lists (the `~` selectors that matched nothing
  usable were replaced; `mud+splash` now precedes `splash`; `exp` rules need a
  token starting with `exp`, the launcher gets `explosion+launcher`; the group
  word for shotguns is `cqb`). rain, drip, steam, mist and fog have no good
  stand-in and stay `-`.
- **Run offline against the staged zones** (a temporary probe, removed): 74
  Black Ops III effects and 58 H1 ones captured with the effects they run
  (132 in all, 5 effect models; 37 of 44 T7 sprite materials drawn; all 138
  roster guns got a view flash, eject and bullet impact, none named an effect
  missing from the catalog). Takes about 150 s on a loaded machine, once per
  session in the common cache. `common_core_mp` is cut short in the staged
  copy; its walk is skipped (the walker's debug depth check is caught), the
  other two zones hold every picked effect.
- **Real MW3 effect names: not readable.** The guns' weapon assets (type 61)
  are in the decoded zones but their layout is not decoded and effects are
  referenced as inline hashed structs, so `weapon/<group>/<gun>/<slot>` stays.
- **Not done.** Placing a map's ambient effects (still unknown where a map
  names them). Nothing was run in game; the H1 effects' pictures need the
  game's `.pak` images, which are not staged on this machine, so those
  sprites fall back to the lane's retail stand-in until run on the PC.

### Where a map places its effects (2026-10-27, not found; what is missing)

Asked to give MW3 (2023) maps ambient effects as Ghosts and Modern Warfare
Remastered have (`BO2MP_MAP_FX_<game>`). Not reachable in this pass, for these
reasons, in order:

1. **No way to read a zone off Windows.** Every map zone is Oodle Kraken
   compressed (`oodle.rs`); the DLL loads on Windows only, and on Linux
   `oodle_so` needs `$JUP_OODLE_LIB` (a library exporting `ooz_kraken`),
   none on the build machine. The staging area held one compressed
   `global_stream_mp.ff` and the DLL, nothing decoded, and the PC shell is
   gone, so no zone could be opened and no number below is new data.
2. **The format notes say the placement is not in the entities** (Rust:
   path nodes, script structs, triggers, models; Derail's `script_struct` has
   only `delete_on_load` and prefab names) **and no effect hash occurs outside
   the effects' own headers in what was searched** (the ambience note: its
   emitters also stand in a compiled asset not identified). The same holds
   as for Ghosts and Remastered, whose placements are in the compiled
   `createfx` script of the zone or in raw script files; MW3's equivalent is
   most likely a compiled script or a scriptable/`fx` struct asset whose
   effect fields are 64-bit name hashes (`vfx::path_hash`, so they could be
   matched to a zone's effects without any names).
3. **The effects themselves are not playable**: `vfx` reads the container,
   module lists and ids but not the module payloads (spawn rate, life, size,
   colour, material hash), so even with placements there is nothing to
   convert; the stand-ins (`jup_fx_standin`) are asked for by entity keys
   (`fxid`, `fxdef`, `script_fxid`, `vfx`) that no staged map's entities
   carry.

Added to unblock the first two: `fastfile_jup::vfx::references` and
`jup_vfx <zone> --refs` list every mention of a zone's effect hashes outside
the effect headers, with any coordinate triple just before the hash or after
its pointer (unit-tested on hand-made bytes). **Run it on the PC** on a map
zone (`mp_jup_*.ff`, with `--install <MW3>`) and on `ingame_shared_mp`: if the
map's placements are data, the mentions with coordinates are them (their
offsets tell which asset holds them: compare with `jup_zonetypes`); if there
are none, the placements are in a compiled script and the Ghosts approach
(`fastfile_iw6::createfx`: a record per statement group, strings hashed here)
applies. Once placements exist, the conversion still needs the module
payloads, or a stand-in chosen per effect name from the table
(`jup_fx_standins.tsv`) spawned at the placement through
`iw6_fx::GuestMapFx` (the H1 and Black Ops III stand-ins are already
converted by `hmw::standin_effects` and `t7_map::shared_effects`).

### Black Ops III's look (2026-10-27)

`BO2MP_BO3_LOOK_JUP=0` turns off; on by default for day maps. Today's draw is the
CoD4 branch over vertex colours baked by `jup_light`; under the look
(`T7Map::bo3_look`, `jup_map`, `t7_map::build_t7_world`) the vertex colours go
white, the uniform page and the models' light are rescaled to the look's units
(`look_scale`: ambient to a 0.4 luminance), the sun's hue comes from worldspawn
with light 2.6, and `bo3_look::apply` adds the sun map, rt_ambient over the
spawn points (sky: the ambient's colour) and the tone curve. Gaps: night maps
(Shipment) keep the baked lamps, no look; no lightmaps, light grid or probes are
read (so no probe reflections and no baked local lights); materials bind colour
only (no normal or gloss maps: no GGX highlights); fog is not read (no fog rows
beyond the look's haze); no sky-driven ambient colour (the baked constant).
Not looked at on screen.

### Black Ops III's renderer on MW3 maps (2026-10-28, default)

The owner's decision: MW3 maps (Highrise first) drawn with Black Ops III's
renderer, MW3's own sky kept, and missing or poor MW3 pictures replaced by
Modern Warfare Remastered's (Horizon MW) and Modern Warfare 2's.

**Approach chosen: the converted world in Black Ops III's look**, not
`render_t7` fed a converted map. Why:

- `render_t7` executes a Black Ops III zone, not a map description. Its
  scene (`Scene::from_map`) needs the map zone's image and walk
  (`T7Map::zone_data`; a converted `T7Map` has none: "the map was read
  without its zone"). It takes the world's vertex streams and `GfxSurface`
  records (0x60 each, the dpvs ranges) straight from the zone's `GfxWorld`,
  every material from the zone's capture (technique set -> compiled DXBC
  programs, vertex declaration, state map, samplers, constant buffers), and
  the lighting and post chain's inputs from records of the same zone:
  `Lighting::new` and `Post::new` read the sun volume's
  `GfxGlobalLightSettings`, the exposure base, the bloom settings at the
  world header, the grade LUT material, the probe volume and reflection
  probe arrays, the lit fog volumes and lights.
- Feeding it MW3 would mean writing a synthetic Black Ops III zone (a
  `GfxWorld` record set, materials pointing at donor technique sets,
  a probe volume and reflection probes made from MW3's light, sun and
  exposure settings) and tuning it blind: world technique sets live in
  Black Ops III's map zones, not the shared ones, and no Black Ops III map
  zone is in the build machine's data to take one from or to check against
  (only `core_*`/`mp_common`). That is a renderer port of its own, not a
  switch.
- The converted lane already draws MW3 maps through the Black Ops III
  branch of the world shader (`t7_shade`: GGX sun, the baked sun map, the
  rebaked ambient, Black Ops III's tone curve), the branch Ghosts and
  Remastered maps use (`lane::bo3_look`, "Black Ops III's look" above). So
  that is now the default: `BO2MP_JUP_NATIVE=1` opts back in to
  `render_jup`, which is kept whole.

What changed in the converted lane (`lane::jup_map`, `lane::jup_texsub`):

- **Default**: `match_walk` builds the converted world for `jup` maps;
  `render_jup` only with `BO2MP_JUP_NATIVE=1` (`jup_native_opted_in`). The
  converted map's cache key tells the two apart (a conversion for
  `render_jup` keeps no pictures, sky or terrain picture).
- **Sky**: MW3's own sky cube (`jup_sky`, the wide BC6H pictures), as before.
- **Far vista clusters** (mesh reach over 12,000 units: Highrise's
  `us_periph_vista_cluster01_*`, `vista_highrise_skidrow_far`) are left out
  as `render_jup` leaves them out (79 placements of 4 models on Highrise;
  `BO2MP_JUP_FAR_VISTAS=1` keeps them). Placed where the zone places them
  they were the brown slabs burying the roofs.
- **Tool pictures** (Caulk Shadow and kin, recognised by picture as
  `render_jup` does) are shadow casters only (10 on Highrise).
- **Layer tints**: a world material's first colour row tints its picture
  (392 on Highrise, `BO2MP_JUP_TINT=0` off); BC1 pictures are tinted in
  place (end colours scaled, block mode kept), others redrawn as RGBA.
- **Paint**: a placed model's near-white tint base takes the colour its
  name states (`paint_of`, as `render_jup`; 122 materials, 404 model
  surfaces; `BO2MP_JUP_PAINT=0` off).
- **Albedo roll-off**: past 0.5 linear it rolls off at a quarter in
  `t7_shade` on an MW3 map (light row 255 element 13;
  `BO2MP_JUP_ROLLOFF=0` off).

**Stand-in pictures** (`lane::jup_texsub`, `BO2MP_JUP_SUB=0` off). A
drawn material wants one when it has no colour picture (grey), its picture
is not in the install, or it is read at 512 px or less while its full size
is larger (its top levels only in the download packages;
`BO2MP_JUP_SUB_LOW`). The other maps are found as
`<mwr>/hmw-usermaps/<map>/<map>.ff` (or `zone/`) and
`<mw2>/zone/english|dlc/<map>.ff` from the game folders, or
`BO2MP_JUP_SUB_MWR` / `BO2MP_JUP_SUB_MW2`; `mp_jup_highrise` is `mp_highrise`.

1. The two worlds are lined up: of the eight quarter turns and mirrors, the
   shift (searched round the spawn points' medians, then finer to 16 units)
   that lays most of the MW3 play area on Horizon MW's world triangles.
   Highrise: MW3 + (-1408, 6440, -1), no turn, 29% of the points within 32
   units (every other turn or mirror 17% or less).
2. **By place**: each wanting material's triangles are looked up among
   Horizon MW's at the same place, facing the same way; the Horizon MW
   material covering most of their area wins (40% of the area found, 25%
   found; placed models 60%). A small MW3 picture is replaced only by place
   (60% / 50%) and only when the stand-in's colour is within 0.6..1.7 of its
   own in every channel; the stand-in is then tinted to MW3's colour.
3. **By role and colour**: otherwise its colour (its own picture, else its
   tint row) against every candidate of the same role (concrete, metal,
   glass, wood, roofing, brick, tile, ground from the candidate's name; the
   MW3 material's from where it lies); without any colour the commonest
   candidate of its role by area.
4. A Horizon MW material whose picture is a retail one (not in the usermap's
   `.pak`) takes MW2's material or picture of the same name.

Each is logged `jup sub: <material> -> <game>:<picture> (<why>; <how>; its
material ...)`. Highrise offline: Horizon MW's `mp_highrise` (229,724
world triangles, 551 colour pictures read), 855 materials wanted one, **116
stood in**: 40 grey ones by place (office walls, concrete panels, tile and
wood floors, linoleum, carpet, brick, windows, fire-escape trim), 44 grey
ones as concrete by where they lie, 10 by colour, 22 small pictures by
place; 617 small pictures kept (no clear place match), 76 kept (stand-in's
colour too far), 46 grey ones with no match. MW2: its `mp_highrise` zone
walks (545 materials) but its colour pictures are in `main/iw_*.iwd`, not
staged here (4 GB), so none read offline; on the owner's PC they are read
from `main` beside the zone.

**Offline check**: `cargo run -p assets --example jup_bo3_shot -- --zone
<ff> --out <dir> [--cam name:x,y,z,pitch,yaw;...] [--no-sub]` converts the
map exactly as the load does (`assets::jup_preview`) and renders it
headless (lavapipe): albedo x (the look's ambient + the sun x 2.6 by N.L
and a shadow map), the Black Ops III tone curve, MW3's fog, the converted
sky. It is the look's formula reduced, not the game's pass (no rebaked
ambient occlusion, reflections, BO2 grade or bloom). Highrise, reference
view (1300, 900, 3300, pitch 14, yaw 210): mean colour 0.46 0.43 0.41 and
luma 0.25/0.41/0.68 against the reference screenshot's 0.42 0.33 0.24 and
0.16/0.30/0.58: brighter and greyer. In an office interior 13% of pixels
change with the stand-ins (grey walls now concrete, doors and trims).

Gaps: the converted world draws one picture per material (MW3's layers,
blend masks and normal maps are `render_jup`'s only), so painted and
layered surfaces read cleaner and whiter than in MW3; no reflection probes
or MW3 colour grade in the look; effects not drawn (stand-ins as before,
`jup_fx_standin`); the crane's loads hang as placed; 46 grey materials and
the placed models' materials have no place match (Horizon MW's props are
not read); MW2's pictures unverified offline. Not yet seen on the PC.

## Native renderer (`render_jup`, 27 Oct 2026)

Modern Warfare III (2023) and Modern Warfare II (2022, iw9) maps can be drawn by
`crates/render_jup`. **Since 2026-10-28 it is opt-in (`BO2MP_JUP_NATIVE=1`)**:
by default the converted world draws MW3 maps in Black Ops III's look (see
"Black Ops III's renderer on MW3 maps" below). Before that it was on by
default and `BO2MP_JUP_NATIVE=0` turned it off. Modelled on the BO3 native renderer (`docs/T7_NATIVE.md`): the glue is
`render_gpu::drawsurf::jup_native` (`active()`, the zone named at load by
`asset_world::set_jup_native_zone`, a scene read on a thread, the host made on
the render thread, the final image and scene depth handed to the frame through
`render_t7::present`, Black Ops II's grade and bloom skipped while it draws).
It shares `frame::settings::t7_native_load`'s loading hold. While it is active
the converted MW3 world, props and sky do not draw; with the option on they are
not even built (`load_jup_world(.., native)`; collision, entities, spawns and
the map's light for players are kept).

What is in it (log lines start `jup native:`):
- `scene`: the map zone and the zones beside it (`JupMap::load`), surfaces
  with smooth normals, the placed models (one mesh, an instance per
  placement), each material's colour picture and packed normal map with its
  mip chain from the `.xpak` packages (BC1/BC7 as stored), reflection probe
  ambient, the vision set's fog and sun, worldspawn sun.
- `library`: `code_post_gfx` found, programs lowered with
  `dxil_sm6::wgsl::translate`, a vertex and a pixel program linked by
  semantic (pixel locations moved onto the vertex outputs; a pixel binding
  whose register the vertex stage uses as another type, e.g. space 0 `t0`, moves
  up by 1024).
- `records`: the material record buffer (`GPU_PIPELINE_ID` = byte offset;
  picture indices at +516..+660, none = 0xFFFFFFFF; layer 0 colour in slot 0,
  packed normal in slot 4) for the day the programs can be driven.
- `mapping`: which programs a material draws with. **Not decoded in the
  install yet**; `mapping::choose` answers `None` and the API of the reader
  that is decoding it goes there. Until then every surface is drawn by
  `fallback.wgsl` (colour picture, the decoded packed-normal formula, gloss
  and occlusion from the normal map's red and blue), logged as such.
- Why the programs are not driven yet even where a pair is chosen: the vertex
  programs pull vertices from raw scene buffers (space 0 `t0`, space 1
  `t23..t26`, ...) and the pixel programs read engine textures and buffers
  (space 1 `t0..t79`, space 2 `t0..t89`) and `GfxCodeSceneConsts` /
  `perObject` constant buffers whose member layouts are stripped. Their
  bindless ranges are declared 270,336 wide (three of them), which needs
  `max_binding_array_elements_per_shader_stage` of that size and groups up to 7.
- `coverage`: a thread builds the pipeline list on the device at load
  (`BO2MP_JUP_PROGRAM_CHECK=0` skips): one pair per distinct binding family,
  lowered, linked, made into a render pipeline under a validation error scope,
  reported as `jup native: pair vs N ps M: ...`. `BO2MP_JUP_FORCE_PS=<n>`
  checks a chosen pixel program.
- `host`: a 4096^2 sun shadow map around the eye (`BO2MP_JUP_SHADOW=0`,
  `BO2MP_JUP_SHADOW_SIZE`), the G-buffer (albedo, normal and gloss, reversed
  depth), the lighting pass (sun with 3x3 PCF, nearest-probe ambient up/side/down,
  vision-set fog with the sun colour inside its begin angle and height thinning,
  sky gradient with a sun glow where nothing drew), and the display stage.

Compute / UAV passes the game has (SSR, bilateral grid, virtual texturing,
auto exposure) are not reproduced. The display stage implements the sRGB
branch of the tonemap (program 1239: exposure, then
`1.055 x^(1/2.4) - 0.055`) with a soft shoulder in place of its 3D lookup table
(the table is not located); program 1239 itself is a UI-style quad (its
vertex stage is the nearest earlier program writing `COLOR`+`TEXCOORD`) that
masks by a per-tile bit buffer and reads a 400-row scene constant buffer, so
it lowers and validates but is not bound. `BO2MP_JUP_EXPOSURE` scales exposure.

Not drawn natively yet: effects.

Probe: `cargo run -p render_jup --example jup_native_check -- <decoded
code_post_gfx.zone> [--all] [--map <zone.ff>]` validates the host's WGSL and
the pipeline list with naga and prints the coverage (map mode needs the game's
Oodle, so the PC).

Probe result (decoded `code_post_gfx`, pixel programs below 2100 plus the display pair): 223 material pixel
programs, 196 pair with a vertex program by semantic; the 9 sampled pairs all lower, link with every pixel
input written and validate in naga, as do the display pair (vertex 1238, pixel 1239) and the host's three
modules. The widest pair is 26 bindings in 4 groups with three bindless ranges of 270,336 descriptors.
### Game camera and tool materials

`render_jup::game_view::GameView` is the one place the host's `Camera` is made from the game's view: the game's lens is right-handed (x right, y up, looking down -z) over IW units with `perspective_infinite_reverse_rh` (depth `near / z`, 1 at the near plane, 0 at infinity, near 2, 65 degrees horizontal times the 0.75 tangent scale). The camera is rebuilt from the lens's own numbers (eye, normalised axes, tangents, near), so a jittered, finite-far or forward-depth clip matrix cannot reach the host's culling, depth or fog; `describe` / `convention` say in words which one the frame had. `jup_native` (the game) and `jup_shot` (offline, the default; `--legacy-camera` is the old 90 degree, near 4 view) both use it, and `jup_native` logs `jup native: first frame: camera eye ... depth ...; exposure ...` on the first frame, at frame 240 and when the convention changes. A spawn view is the spawn origin plus 60 (`EYE_HEIGHT`); `jup_shot` renders `ground*` views at spawn-like path nodes.

The "fog and brown floors" of the first in-game Highrise were not a convention fault (the game lens reproduced offline looks as the old renders). They were tool materials: a small (16 to 256 per side) BC1/BC7 opaque picture of one flat colour with a dark caption (Caulk Shadow, Shadow, No Collide, No Draw), used by one draw group with no normal or mask pictures. Material names are not in the zones, so `scene::tool_picture` recognises them by their picture (4 pictures, all true positives, on Highrise). Such materials are skipped in the host's G-buffer and forward passes and still drawn into the shadow pass (they cast shadows); `BO2MP_JUP_TOOLS=1` draws them. `jup_shot --dump-tools <dir>` writes the pictures and prints each tool material's draws, triangles and bounds. Offline packs hold few model meshes (Highrise 274 of 1365), so the effect is small offline and large in game.

### Offline screenshots (jup_shot)

`cargo run -p render_jup --example jup_shot -- --zone <ff> --out <dir> [--ents <spawnvars>] [--ooz libooz.so]` renders 1280x720 headless (lavapipe works). Views: spawns, perf cameras, overview, sky; the first view also writes `_albedo`, `_normal`, `_gloss`. Highrise findings: all 2268 world surfaces load after `plausible()` accepts `step <= 1.0`; the material table is extended past non-back-pointer entries (about 37 indices, 569..606, still have no asset and draw grey); water only when `<map>_water_01` exists. Not verifiable without the xsub data (over 400 MB each): pictures, sky cube, most probes. Shaded faces were dark and sunlit faces bright because only 6 probes load; the ambient light no longer depends on them (see "Look fixes" below). `--debug-all` writes albedo, normal and gloss for every view; `--pick view:x,y` needs `--pick-dump <dir>` to exist before it.

## Collision, water and ambience (`fastfile_jup::map::{havok, clip}`, `lane/jup_clip.rs`)

Read offline on Shipment, Das Haus, Derail, Elevated, Farm 18 and Afghan
(`jupiter` zones decoded with ooz; the lane itself cannot run here: no Oodle
DLL on Linux), so none of it has been seen in a running match.

- **Collision.** The zone's `col_map` is Havok 2021 `hknp` shapes, each in a
  `TAG0` tagfile of its own inside the zone (sections `SDKV`, `DATA`, `TYPE`,
  `INDX`; big-endian sizes, an item table giving each object's place in
  `DATA`). Per 256 m tile there is a mesh shape (quantised `Vertex16_3`
  sections plus `u8` primitives, a triangle or a quad) and a compound shape
  (64-byte instances: quaternion, translation, scale, a shape tag, and a
  relative pointer to a convex hull with corners and face lists). Units are
  metres, the game's axes; **game units are metres x 32** (a 256 m tile is
  8,192 units; the earlier "x 39.3701" was wrong, see "Collision units and
  winding" below). Counts: Shipment 4
  mesh + 4 compound shapes (3,196 hulls, 33,878 triangles), Derail 19 + 10
  (9,665, 226,464), Farm 18 90 + 18 (5,653, 175,438), Elevated 22 + 57
  (6,279, 178,050), Afghan 25 + 22 (14,411, 384,710); read in 0.2 to 0.5 s.
  The shapes sit in the drawn world's frame at 32 units to the metre, with no
  offset (checked on all seven maps by registering the shapes' flat floors
  against the drawn world's, 28 Oct 2026; the earlier percentile check at
  39.3701 was too coarse to show the 23 per cent error). The lane builds the hulls into brushes and
  the triangles into the mesh through the Black Ops III clip builder and
  caches it (`jup_cache` CLIP_KIND). The drawn world stands in only where a
  zone has no shapes.
- **Surface types: not decoded.** Every instance carries a u16 shape tag
  and a mesh keeps a `ShapeTagTableEntry` list (8 bytes: first shape key,
  tag; a tag of 0xffff ends it). The tags are per map (Derail uses 10 to 259
  with 107 and 116 the commonest, Farm 18 0 to 239 with 192 and 193 as large
  hulls), not a fixed list. The global physics materials are plain names in
  `code_post_gfx` (`pm_concrete`, `pm_metal_solid`, `pm_dynamicwater`,
  `pm_physicsvolume`, ... 46 of them, with 0x80-byte records beside them),
  but no map zone holds a table from tag to material, to contents (player
  clip, bullet clip) or to the footstep type, and none names a surface type
  in plain text. So every shape is solid and carries surface type `default`
  (footsteps and impacts are the default ones). To do: find the table that
  turns a tag into a physics material (not in the map zone nor in the
  staged `code_*`, `global` or `ingame` zones searched by name hash).
- **Water.** Only Shipment of the staged maps holds water: assets named
  `mp_shipment_iw9_water_01` (and `_nogameplay`), `watersim_*`, `watervis_*`,
  `water_shipment_periph_01..03`, and worldspawn's `water_config`
  (`mp_shipment_iw9_water_config.csv`, not in the zone). A water body is a
  run of wave records (0x30 bytes, floats), its flat mesh (x, y, z floats,
  then `u16` triangle indices) and its names; Shipment's sits at z = -424
  and 46,000 across, the same sheet the world already draws as the sea. The
  other five staged maps have no water asset (Derail, Farm 18, Afghan, Das
  Haus, Elevated: only props and the `veh9_civ_sea_water_platform` collision
  map name). Gameplay: the sea sheet still gives worldspawn `sealevel`
  (swimming from chest depth, wading slowed, `movement_iw4::modern`), and
  the real collision now has the sea floor, so a player wades on it. New:
  collision wholly under `sealevel` carries the `water` surface type
  (index 20, the numbering every game here shares), so footsteps and
  landings there are water's. Not done: pools inside a map (a local water
  rectangle needs the movement's single `water_z` to become a per-position
  test and the wire's movement rules to carry it; no staged map has one to
  check against), swim strokes and MW3's own water sounds. Crossing the surface now
  raises an event: swimming starts with the water landing (`0x84`, volume
  by the way-in speed) and ends with a water walk footstep, both through
  the existing audio events.
- **Ambience: the audio is not in the staged files.** Each map zone holds
  its sound assets (Derail 1,955, 2,091 alias links, 424 aliases; the 86
  of eight seconds or more include about 50 stereo beds of 50 to 150 s) but
  every stream is in the install's `.xsub` packages (none staged: the
  staged `zone` folders hold `.ff` files and five `.xpak` indexes; the PC's
  zone folder has 17,727 files, too many to list through the file tool) and
  no sound is preloaded in the zone (`inline` 0). Alias names are hashes
  and the zones hold no plain alias names for them (`amb_emt_dashaus_*`
  strings in Das Haus are effect names). The maps' entities carry no sound
  emitters (Derail: `script_struct` with only `delete_on_load` and prefab
  names, `script_origin` for helicopters and flags), so where an emitter
  stands is in a compiled asset not yet identified. Needed to finish: the
  `.xsub` packages with the map's streams (`BO2_JUP_INSTALL`'s `zone`
  folder; they decode with `fastfile_jup::xsub`), the alias-to-emitter
  table, and alias names (the `jup_alias_names` method on `amb_*` and
  `emt_*` words). Rebirth Island's ambience beds stay as they were.


### Collision units and winding (28 Oct 2026; spawn census on all seven maps)

Playtesting Highrise: a headless load reported `49 spawns: 3 on the ground, 4
fall more than 64 units, 40 over nothing, 2 in solid`
(`asset_world::world_iw3::spawn_ground_census`, a player box dropped onto the
collision; the spawns are made over the path nodes). The same census on the
other six maps was just as bad (the earlier "Shipment and Derail line up" came
from percentile extents only). Two faults in how the zone's shapes were read,
found by lining the shapes' exactly level, upward-facing floor triangles up
against the drawn world's (a height histogram of each, correlated over scale
and shift, then a 3-D occupancy correlation for the shift in x and y):

- **Scale.** Havok metres are 32 game units, not 39.3701. On every map the
  best scale is 0.8128 of the old one (32 / 39.3701) with shift zero in all
  three axes (Highrise: score 0.38 against 0.0007 as read; Derail 0.46,
  Das Haus 0.66, Elevated 0.52, Farm 18 0.44, Afghan 0.61, all about ten times
  the old scores). Read at 39.3701 the shapes were a fifth too large about the
  origin, so floors stood where the spawn points had none (on a tall map like
  Highrise, hundreds of units off in height). The tile size confirms it: 256 m
  is exactly 8,192 units.
- **Winding.** The mesh shapes' triangles are wound the other way from the
  game's: the player trace meets a triangle only from the side its winding
  faces, so with the havok winding no mesh floor was ever stood on (Derail:
  mesh alone 0 of 48 spawns on the ground, flipped 48 of 48; brushes alone 11
  of 48). `fastfile_jup::map::clip::read` now turns them round (a swap of two
  corners). A two-sided copy gave the same census; one-sided is the game's own.

Census (`on the ground / fall more than 64 / over nothing / in solid`), before
the change (shapes at 39.3701, havok winding) and after (this change):

| Map | Before | After |
| --- | --- | --- |
| Highrise (49) | 3 / 4 / 40 / 2 | 44 / 0 / 1 / 4 |
| Shipment (50) | 1 / 33 / 0 / 16 | 48 / 1 / 0 / 1 |
| Derail (48) | 6 / 0 / 35 / 7 | 44 / 0 / 0 / 4 |
| Das Haus (49) | 0 / 2 / 7 / 40 | 48 / 1 / 0 / 0 |
| Elevated (49) | 3 / 3 / 34 / 9 | 45 / 4 / 0 / 0 |
| Farm 18 (49) | 10 / 0 / 32 / 7 | 46 / 0 / 0 / 3 |
| Afghan (49) | 15 / 14 / 5 / 15 | 48 / 1 / 0 / 0 |

(Das Haus and Afghan also gained from the next point; before it they were
43 and 44 on the ground. Highrise's remaining over-nothing spawn is the map's
own `info_player_start`.) What is left is path nodes the generated spawns sit
on that are not floor: inside a prop's volume (in solid), on a ledge with a
drop (fall more than 64), and the `info_player_start` the zones carry.

- **Spawns only in the play area.** Das Haus and Afghan keep path nodes far
  outside the play area (one at the same far corner in both, over no ground at
  all); the generated spawns used them. Nodes outside the play area circle
  (`floor_points`, the one that already limits collision) are left out
  (Das Haus 2 of 474, Afghan 801 of 2,896), logged as
  `jup spawns: N of M path nodes lie outside the play area`.
- **Safety net.** `lane/jup_clip.rs best_collision` counts the census on the
  zone's shapes; when a tenth of the spawns or more have no floor (fall more
  than 64 units, or nothing below) it builds the collision again with the
  drawn world's collidable triangles (the converted map's surfaces, which exist
  in native mode too, where the world mesh drawn is empty) added to the mesh,
  and keeps it when that puts more spawns on the ground. Logged as
  `jup clip: N of M spawns have no floor of their own ...` with both counts,
  then `jup clip: final spawn census:`. After the two fixes above no map needed
  it (Das Haus and Afghan tried it and it gave no more ground); before them
  it took Highrise from 3 to 32 on the ground.
  `BO2MP_JUP_CLIP=drawn` still forces the drawn world's collision alone.
- How it was found (to repeat): a throwaway `#[ignore]` test calling
  `load_jup_world(<zone>, ..., native = true)` and printing the report lines
  `jup clip:` and `t7 spawns:`, with `JUP_OODLE_LIB` the ooz library and
  `JUP_OODLE_DLL` any file. The converted-map and clip caches are keyed on the
  source files, so editing the lane re-reads the zone (about 90 s a map).

### Pictures a material names without holding them (27 Oct 2026, Highrise)

The first Highrise pass bound pictures for a minority of materials. What was
wrong, and what the loader does now (`fastfile_jup::map::materials`, `foreign`):

- **Shared picture tables.** Of Highrise's 1422 materials 232 do not hold their
  own picture table: the zone stream is depth first, pointers are -2 (FOLLOW:
  data follows, used once) or -1 (INSERT: data follows and later back-pointers
  name it), and a back-pointer is `(block 0x0f << 32) | (offset + 1)` where the
  offset is in the block, not the file. The k-th distinct back-pointer value of a
  field (slot pairs `+0x28`, picture table `+0x30`, data `+0x38`, constants
  `+0x40`) is the k-th material that INSERTed that field ("rank matching";
  exact on Highrise: shared tables 232 of 232, slot-pair lists 127 of 127,
  entry cells 133 of 143). Anchors are monotone in (ordinal, order).
- **Stub pictures.** A 64-byte block whose first word is a picture hash with the
  top bit set and the rest zero says "the picture is an asset of another zone".
  `foreign::resolve` scans `global_stream_mp`, `ingame_shared_mp`,
  `ingame_base_mp`, `global`, `ingame`, `ingame_br`, `code_post_gfx` (188 of the
  233 hits), `code_pre_gfx` for the hash followed by eight zero bytes and reads
  the image header there. 25 stay unresolved on Highrise.
- **Pixels.** `JUP_PACK_DIR` takes several folders (path-list separator), each a
  `pack.bin` + `index.tsv` cut from the install's `.xsub` parts by
  `jup_map_coverage --plan`. Pictures whose only group is in a download index
  (`*_cdn.xpak`) draw at the lower sizes the install holds (1512 pictures on
  Highrise).
- **Normal maps that are not.** A packed normal map averages out to flat. 16
  pictures under usage `0x37` average far over (`mean_tilt` > 0.25: another kind
  of map under that usage byte); they are dropped, not drawn as normals.

Shading changes with it: world vertices are per corner with a 35 degree crease
split (`corner_normals`); triangles whose texture coordinates lie on a line
(the second set is not read) are mapped by their plane at the material's
median density; `material.wgsl` keeps the surface normal where the
derivative tangent frame is ill conditioned; the default exposure is 0.55
(`BO2MP_JUP_EXPOSURE`), since the sun is 1.2 unnormalised and washed any bright
albedo out to cream. Reflection probe groups are in none of the 40 staged
indexes: 6 of 56 probes load, the rest fall back to the flat ambient.

`jup_shot` flags: `--pack <dir>`, `--debug-all` (albedo, normal, gloss per view),
`--cam name:x,y,z,pitch,yaw[;...]`, `--pick view:x,y;x,y` (CPU ray cast over the
world draws, prints material, pictures, UVs, corners), `--pick-dump <dir>` (the
picked pictures as PNG; the directory must exist).

### Layers by usage, not by slot number (27 Oct 2026, Highrise)

The record's fixed slot numbers (0..3 colour, 4..7 normal, 12..15 mask) are not
what the zones hold: a material fills the slots it needs, so a colour picture
can sit in slot 4 or a mask in slot 0 (material 544: normal, colour, colour,
normal, colour at slots 0, 1, 2, 3, 8). `layers::classify` now takes the
material's distinct non-placeholder pictures in slot order, sorts them by usage
byte and builds layer k from the k-th colour (0x36; 0x32 if there is none), the
normal map of the same size (else the next one) and, above layer 0, the k-th
blend mask (0x33). A layer above the first with no mask is dropped. A material
with two masks and one colour that is its own colour picture (its other
colours are not in the install) is left grey: alone, a picture meant for
patches would cover the whole surface. Highrise: slots read as expected 3147,
of another usage 148 (was 2127 and 2141).

Triangles whose texture coordinates lie on a line are mapped by their plane
only where the ratio of the two principal stretches is under 0.25 and the
material is neither cut out nor blended (`BO2MP_JUP_PLANAR=0` turns the mapping
off); a cut-out BC1 picture is one with a block that is in three-colour mode
and uses index 3.

### Look fixes: blend masks, planar density, sky ambient (28 Oct 2026)

Headless renders (`highrise4`, `maps4`), all from data already staged.

- **Blend masks are BC4.** Highrise's 133 usage-0x33 pictures all have format byte
  `0x2a`, which `image::dxgi` calls BC1 (same eight bytes a block). A one-channel
  mask read as BC1 is noise (magenta static), and the layer weight
  `smoothstep(0.47, 0.53, mask)` then picked a random layer per texel: the
  stretched speckle and white flecks on the big roofs, and white layers over
  walls. `scene::mip_chain` reads format `0x2a` as BC4 (DXGI 80, `Bc4RUnorm` on
  the device; `jup_shot`'s picture dump decodes it). `image::dxgi` is left alone
  (other lanes use it).
- **Planar mapping density.** World triangles whose texture coordinates are on a
  line or sheared are mapped by their plane at the material's median density. That
  median came from every triangle including slivers, so a material could read
  1.3e-5 units per world unit (a whole roof on one texel: the flat white panel),
  or 0.13 (noise). Now only sound triangles (smaller stretch at least 0.4 of the
  larger, 100 units^2 or more) set it, three at least, held to 1/1024..1/16; else
  1/96. A triangle is planar below a ratio of 0.4 (was 0.25), which takes the
  wedge-shaped streaks off the large flat roofs.
- **Albedo.** Past 0.65 an albedo is rolled off (x0.12): a white layer in the sun
  was a blown-out panel. Materials with no colour picture (about 150 on Highrise,
  most of Farm 18) show a stand-in albedo: a muted palette entry chosen by the
  material hash (`skylight::fallback_tint`) varied over the surface by world-space
  noise, not one flat 0.22 grey. A material whose only colour picture is an upper
  layer (two blend masks, one colour, the base missing) keeps it as layer 1 over
  that stand-in by its mask (it was left wholly grey).
- **Grey materials of Highrise, counted** (`JUP_DBG=1` prints the signatures):
  of 155 without a colour picture, 6 had a picture that could not be read (no data
  in the install), about 35 were the lone-overlay case above, about 110 bind no
  colour picture at all (a normal map and masks, or nothing: their colour pictures
  are stubs in zones that are not staged or are not in the zone).
- **Night.** A worldspawn `sunlight` of 0 is a map that leaves the sun to its
  vision set, not a night. Only `0 < sunlight <= 0.05` is one. Shipment (sunlight
  0) was drawn with a 0.05 sun.
- **Ambient light** (`render_jup::skylight`). The probes that load differ by
  100 times (Shipment: up 7, 330 and 0.3), and the nearest one lit everything:
  Shipment white, Farm 18 flat grey. Now: (1) the map's open-sky ambient
  (up, side, down) is the cosine mean over the sphere of the sky cube, or the
  vision gradient desaturated to a third, with the sun's bounce off a 0.28 ground
  below the horizon, times 0.8; (2) the probes are brought to that level (median
  luminance, each held to 1/3..2x of it) and applied only near themselves (full
  within 400..1200 units, none past 1500..4000, at most 0.7 of the light); (3) a
  sky visibility term from a height grid (`HeightGrid`, the highest geometry per
  cell over the playable plan from world surfaces and placed models, cells of
  24 units or more, at most 1024 a side, kept after the probes in the storage
  buffer): the lighting pass marches eight directions by eight steps and takes
  `sin^2` of the highest horizon, weighted toward the normal's side, never taking
  more than 0.7 of the light. A street between towers is darker than the roofs
  with no probe at all (`BO2MP_JUP_SKYVIS=0` turns it off,
  `BO2MP_JUP_PROBE_RAW=1` keeps the probes' own level).
- **Reflections** no longer need a probe: the sky (without its sun) blurred
  toward the ambient with roughness, the ground's bounce below the horizon, the
  probe's radiance mixed in near it. The sun's GGX highlight uses roughness of at
  least 0.1 (a mirror-smooth normal map made white blots).
- Still off: pictures of the six other maps (not staged), their sky cubes (the
  gradient is used), Derail's magazine atlas on a wall (material 852 holds
  picture 283fe910, a prop atlas: a stub resolved to the wrong picture), the
  colour grade's warm cast on Highrise (the grade volume is applied as decoded),
  Highrise's remaining white panels that are white by their pictures.

### Reflection probes and the packed tangent frame (27 Oct 2026, findings)

- The 50 probe pictures not in the zone have two groups each (21824 and 87360
  bytes, the BC6H cube chain). Their group keys are in none of the 40 staged
  `.xpak` indexes, including the `_cdn` ones: the nearest entry of any index
  differs in 11 to 13 of 64 bits, i.e. is unrelated. They are in an index that
  is not staged. `file_list.txt` names no map package (`mp_jup_*` has no
  `.xpak`/`.xsub` of its own, 0 hits); the candidates are `map_shared.xpak`
  (staged, no hit) and indexes the listing does not show. The listing holds 178
  files and every `.xsub` it names belongs to `boot`, `codhq23a/b`,
  `codhq_shared`, `common23`, `map_shared`, `mp23`, `wz23` (+ language
  packs). The probe keys must be looked up in the `.xpak` files of those that
  are present in the listing but not in the install `zone` folder; none such is
  listed, so a missing index file is not the cause. The likely home is a
  streaming index of the install that is not in `zone\` at all (the game's
  `Data\` or the Battle.net cache).
- Packed tangent frame (4 bytes, `WorldSurface::frames`): on 224k vertices whose
  faces all agree, the top two bits and the low 30 bits behave as a
  smallest-three quaternion in no tested layout (all permutations of the three
  10-bit fields and of the tag, scale 1 and 1/sqrt 2: best 65% within
  0.98 of the vertex normal). The fields are bimodal (values 511/0/1023) for
  axis-aligned faces, so it is a quantised frame, but not a plain quaternion.
  Left as crease-split normals.

### Material shader (28 Oct 2026)

The zones store no material-to-program link (the engine chooses at run time), so the native renderer draws every material with `render_jup/src/material.wgsl`, an uber-shader written from the decoded family 12 pixel programs:

- Layers: colour l at record slot l, packed normal at 4 + l, blend mask of layer l at 12 + l (a one-channel picture the programs compare with 128/255). `layers::classify` reads those slots by the picture's usage byte (0x36/0x32 colour, 0x37 normal, 0x33 mask) and counts the slots that bear it out; a slot holding another usage is left empty, not guessed. Slots hold the material's slot-pair pictures in pair order (`MapMaterial::entries`).
- Colour: each layer's sample is square-rooted, mixed by the masks and squared back.
- Normal: p = 2g - 1, q = 2a - 1, x = (p + q) / 2, y = (p - q) / 2, z = 1 - |x| - |y|; red is gloss, blue occlusion. The tangent frame comes from screen-space derivatives (the vertex data carries none).
- Light: sun diffuse with the shadow map, GGX highlight (Smith visibility, Schlick Fresnel, F0 0.04, roughness from (1 - gloss)^2), probe ambient and the nearest probe's radiance for the reflection.
- Load: `fastfile_jup::map::handover` passes the converted lane's `JupMap` to the native scene.

Unverified: that the pair order equals the record order, the mask threshold softness, and the roughness mapping.

Ambience, what was found when staging was tried: of Derail's 1,955 sound
keys only 114 are in the staged zone `.xpak` indexes (107 in `codhq_shared`,
7 in `codhq23b`, none in `boot`, `boot_shared`, `codhq23a`); the rest are in
indexes of packages not staged (the map's own, names unknown: the install's
`zone` folder has 17,727 entries and the file tool refuses to list over
8,000; guessed `mp_jup_derail.xpak` does not exist). The `.xsub` parts that
do exist (`codhq_shared-00000.xsub`) are over the tool's 400 MB staging
limit, and splitting needs a shell on the PC (the device shell tool was
withdrawn). Unblock: stage `zone\*.xpak` (small) and the `-NNNNN.xsub`
parts that `sndkeys`-style key lookups point at, split to under 400 MB.

Also drawn now (28 Oct 2026):

- **Terrain.** The coarse height grid is a mesh in chunks of 16 cells (`render_jup::terrain`), its colour the virtual texture's finest level 4 read tile by tile (BC1, 62 inner blocks a tile, 3968 px a side with mips; a missing tile is sand) and laid over the terrain's square.
- **Frustum culling.** World surfaces are grouped per material and per 3072-unit cell, so a draw covers a small box; each draw carries its box (placed models: the union of their placements grown by the model's reach). The camera's side planes and near plane, and the sun's for the shadow pass, cull the draws (`BO2MP_JUP_CULL=0` submits everything).
- **Blended surfaces.** There is no flag in the zone that says a material blends (the header's `class_0e` is not decoded). A material whose colour picture is BC7 with alpha endpoints under 200 on a fifth of the blocks that can carry alpha (modes 4 to 6) is taken as translucent and drawn after the lit image (`forward.wgsl`: same surface as the G-buffer shader, unshadowed sun, probe ambient and reflection, fog), depth tested and not written, sorted far to near by box centre per draw. Log line `blended materials`. `BO2MP_JUP_BLEND=0` draws them opaque as before. UNVERIFIED: that BC7 alpha on a colour picture is opacity and not another term.
- **Water.** The flat sheets wider than 16384 (Shipment's sea, 46,000 across) are drawn by `fs_water`: sky and sun mirrored by Fresnel over a deep tint (the fog's colour), fixed ripples; the wave records of `mp_shipment_iw9_water_01` are not used.
- **Colour grade.** Searched with `jup_lut_search` over the staged zones: volumes of 32 x 32 x 32 texels, format 0xc (four 16-bit unorm channels, 262,144 bytes inline), usage 0x11 are the grades: one in each map zone (Rust: `104e72e8c42b2da8`), six in `ingame_shared_mp` (mean distance from the identity 0.01 to 0.23, so some are strong looks), two in `ingame_br`, one in `code_post_gfx` (`4ff89484c15f6a28`, far from the identity). Axes x, y, z are red, green, blue (identity fit 0.016 mean error against 0.23 for any other order); the diagonal runs about 0, 0.12, 0.26, 0.52 ... 1 at 0, 4, 8, 16 ... 31, so the volume is indexed by the log encoding the display program uses (`(log2(x + 1/1024) + 10) / 20`), and the display stage applies it in that encoding (`display.wgsl`, `graded`) before the shoulder and the sRGB transfer. The map's own volume is used; the shared ones belong to vision sets this lane does not select. `BO2MP_JUP_GRADE=0` skips it. UNVERIFIED: that the program indexes it this way for the whole image (its tile bit buffer and 483-row constant buffer are not decoded), and the tonemap curve itself, which stays the soft shoulder.
- **The map's sky.** The BC6H sky pictures (`render_jup::sky`, the converted lane's mapping) become a cube, the picture with the brightest sun turned to worldspawn's; sampled where nothing is drawn and for the sea's reflection (`BO2MP_JUP_SKY=0` for the analytic gradient).

### Highrise, second look pass (28 Oct 2026)

Offline (`jup_shot`, game camera, 1280x720, lavapipe) against a reference
screenshot of MW3 Highrise (warm late sun from low on the horizon, long hard
shadows, hazy warm sky, strong contrast). Renders: `highrise_v2/before`,
`step1`, ... in the outputs folder.

- **Model meshes offline.** The offline packs held the material pictures but
  not the placed models' meshes (they are streamed by key like pictures):
  274 of 1365 models built. `jup_map_coverage --meshes` adds the stream key of
  every LOD of every placed model to the plan (`coverage::mesh_keys`), and
  `--skip <index.tsv>` (repeatable) leaves out keys an earlier pack holds, so a
  new pack carries only what is missing. Highrise: 3301 streamed mesh keys,
  3281 in local parts, 20 in no index; with 6 picture groups the first packs
  lacked, 3287 ranges, 87 MiB (`MW3\highrise_out3\hr3_extract.bat`, plan
  `hr3_plan.tsv`). With it, 1345 of 1365 models and 30,388 of 30,745
  placements build. `JUP_PACK_DIR` takes the folder like the others; an
  `index.tsv` may carry a fourth column naming the file of the folder the
  bytes are in (`pack-001.bin`, for packs split under the staging tools'
  400 MB), else `pack.bin`. Probe pictures are still in no staged index.
- **Far vista clusters.** `us_periph_vista_cluster01_*` and
  `jup_us_vista_highrise_skidrow_far_01` reach 17,000 to 30,000 units from
  their origins and, placed where the zone places them, buried the rooftops
  under brown boxes. A model whose mesh reaches past 12,000 units is left out
  (logged; `BO2MP_JUP_FAR_VISTAS=1` draws them). How the game places them is
  not known.
- **Models made where they stand.** 184 model surfaces (e.g.
  `jup_construction_slat_highrise_01`, centred at (-1608, -741, 2924) with half
  sizes 160 x 93 x 68; `jup_highrise_office_rooftop_cable_02`) keep the place
  they were made at in their vertices; each placement added it again and they
  hung 3,000 units over the roofs. A mesh whose box centre is over 500 units
  and over four times its largest half size from its origin is moved onto it
  (`models::recentre_far`, also the collision's coarse LOD); recentred, their
  bottoms meet the roofs (the slats' 2,988 against roofs at about 2,990).
- **Layer blend weights are in the world's vertices.** The surfaces' extra
  stream (a word a vertex) is the painted weight of each layer: byte 0 is 255
  on every vertex, byte k layer k's weight; two-layer materials have byte 2 at
  0 (Highrise material 382: 320 of 320 vertices), three-layer ones spread it.
  The material shader now reveals layer k where `weight + mask - 1` passes 0
  (`smoothstep(-0.12, 0.12)`), the mask a height the painted weight uncovers;
  without painted weights (models) the 128/255 test stays.
  `BO2MP_JUP_VERTEX_BLEND=0` turns it off.
- **Layers by run of pictures.** A layer's pictures were written to the zone
  one after another, so they hold neighbouring image indices (material 543:
  365..367 concrete colour, normal and its own grunge mask; 643..645 normal,
  mask, colour; 152/154 colour, normal). `layers::classify_runs` makes a layer
  of each run, in the order the runs first appear among the material's
  entries (the run of the first entry is the base), each with its own mask
  where it has one; an upper layer without one is revealed by the painted
  weight alone (bit `l - 1` of the flags' mask word). Where the pictures make
  fewer than two runs with a colour each, the slot classification decides as
  before (`BO2MP_JUP_LAYER_RUNS=0`: always). 396 materials draw more than one
  layer (was 299).
- **Atlas layers.** The roof material's middle run is a boot (a prop atlas:
  a boot from the side and its soles) shared by 8 world materials: an upper
  layer the game samples with the second texture coordinates (not read here),
  so with the first it repeated the boot over the roofs (the brown scalloped
  roofing). An upper layer whose colour varies a lot at large scale (the
  deviation of its 8 x 8 block means over 0.045: the boot 0.062; concrete
  0.008, paper litter 0.012, plaster 0.022) shows its base instead (95 on
  Highrise).
- **Not an albedo.** A colour picture whose mean channels differ by more than
  0.45 is a packed mask under a colour usage (material 65's 0x32 picture, green
  with yellow flecks: the bright green ledges); the stand-in albedo shows
  instead (14 on Highrise).
- **Painted models.** Model pictures with a mean brightness over 0.70 are tint
  bases (paint over white: the crane `jup_us_machinery_crane_towerpeak_01_yellow`
  draws a light grey picture). The paint is in the material's constant rows
  (packed colours with a flag byte, e.g. `90 64 43 0a`, several per row; which
  row a program reads is not decoded, and the yellow crane's rows hold only
  white and grey), so the model's own name stands in for it: yellow, orange,
  red, blue, green, black, white, grey/gray, tan as a word of the name; else a
  painted-steel 0.62 (127 materials on Highrise). `BO2MP_JUP_PAINT=0` off.
  UNVERIFIED as the game's colours; a stand-in until the rows are read.
- **Light.** Worldspawn gives no sun intensity (hashed keys), so the stand-in
  was 1.2 against a sky ambient of about 0.43 up: shade and sun two to one, the
  flat, muddy look. Now 3.0 (`BO2MP_JUP_SUN`) with the ambient at 0.48 of the
  open sky (was 0.8; `BO2MP_JUP_AMBIENT` scales it), the sun's tint 60 per cent
  of the sky disc's colour (was 40; `BO2MP_JUP_SUN_TINT`), exposure 0.5.
  The sun's direction (elevation 12 degrees, toward +y) is the vision set's;
  the sky picture's brightest sun is turned to it (`sky:` line).
- **Display curve.** A filmic curve (the fitted ACES one) after the grade:
  a toe for the shadows, contrast in the mid tones, a long shoulder for the
  sun (`BO2MP_JUP_TONEMAP=shoulder`: the old soft shoulder). The game's own
  is a 3D table not located.
- **Fog over the sky.** The sky was drawn without fog, so a far vista tower
  (100,000 units off, fogged to the fog's colour) stood as a tan square on it.
  The sky now takes the height fog integrated to infinity along the view ray:
  the horizon the fog's whole opacity, the zenith little (a hazy warm horizon,
  as the game's).
- **Ground views.** Neither the zone's entities nor the exported spawnvars hold
  mode spawns (`mp_*_spawn`; the game makes them at run time), so `jup_shot`'s
  ground views stood on arbitrary path nodes, inside or against walls. They
  now stand on path nodes with an open view: horizontal rays from head height
  in 24 directions against the world and the placed models (a grid of 96-unit
  cells); a node meeting anything within 40 units is passed over; the view
  looks down the five neighbouring directions (75 degrees) whose nearest hit
  is at least 450 units off, scored by their mean reach (held to 2,500, open
  sky counting 1,200), four views 1,500 units apart. Mode spawn entities,
  where a map has them, are used with their angles. `--ents` with the exported
  spawnvars adds its 28 designer `perf_camera` views.
- `jup_shot --pick` now finds placed models too (prints the model's name and
  placement). Debug: `JUP_DBG_MAT=<i,..>` (a material's entries, layers, which
  materials share each picture, the extra stream's bytes),
  `JUP_DBG_MODELS=1` (largest models, placements over the roofs),
  `JUP_DBG_FIND=<name,..>` (placements by model name),
  `BO2MP_JUP_HIDE_MODELS=<name,..>`.

- **Placement checked** (`jup_shot --check-placement`). Every reading of the
  instance's four 16-bit words (24 component orders x 8 sign patterns) scored
  by the placements left upright (up axis within 10 degrees of +z) and, among
  those tilted as read, the ones resting on a world floor (a ray from the
  box's bottom centre, within 12 units). As read, (x, y, z, w): 18,609 of
  30,467 upright; every other order about 2,315; the as-read signs rest the
  most tilted ones (4,271 of 11,720; others 4,037 to 4,161). The words are of
  unit length across the table (a packed smallest-three, a matrix, Euler angles
  or an offset of four bytes would not be). The 39 per cent tilted as read are
  pipes, beams, conduits, wall vents, cables and crane parts; about 40 per cent
  rest on a world floor because the rest stand on other props, walls or
  ceilings. The Black Hawk (`veh8_mil_air_blima_mp`, a Z-up mesh: half sizes
  292 x 58 x 71) has up.z 0.41 under every reading that keeps the others
  upright, and its separately placed tail rotor (z 2976, 145 under the body)
  fits the roll: the zone places it rolled.
- **Ambient occlusion.** A screen-space term in the lighting pass: twelve
  points in the hemisphere over the normal, up to 40 units (growing to 120
  with distance, fading out from 2,500 to 5,000 units), projected into the
  depth buffer, a point behind the drawn scene and within the radius of it
  occluded; the pattern turns per pixel (4 x 4). Up to 80 per cent of the
  ambient and the reflection is taken, never the sun (`BO2MP_JUP_SSAO`
  scales it, 0 off).
- **Glass and wet.** Where the gloss (the packed normal map's red) is
  mirror-like, the diffuse colour gives way to the reflection: x0.35 at 0.95,
  from 0.72. The towers' window panes are only in their normal maps' gloss over
  a brick or concrete colour (`sp_tower_vista_building_t3_*`: brick colour,
  window grid in the normal map's red).
- **Rooms.** The sky visibility takes at most 55 per cent of the ambient (was
  70). Rooms are still dark: their light comes from the game's local lights
  and light grid, not drawn.

- **Two texture coordinate sets (the roofs' tiling).** A world surface record's
  `+0x1C` is its number of texture coordinate sets and `+0x44` the stream after
  them. Where it is 2, the coordinate stream is 16 bytes a vertex (set 1, then
  set 2), `+0x44` exactly `16 x vertices` past it (surfaces 1918: 0xC0 for 12
  vertices; 1921: 0x200 for 32). Read 8 bytes apart, every vertex after the
  first took half its own and half its neighbour's coordinates: Highrise's
  roof material 543 ranged from 177 down to 8 units per texture repeat across
  its surfaces (the big roof at 8, the concrete a smooth tan blur), while
  material 382 with the same concrete picture read 112. Read at the stride
  the record gives, both are 128 units a repeat (the median of the map's
  materials), and 440 materials have sound coordinates (was 314). 505 world
  surfaces have a second set (`WorldSurface::uvs2`). `BO2MP_JUP_UV2=1` draws
  atlas layers with it: the boot atlas lands as boot-sole prints, about ten
  times a print's size, so a scale is still missing; by default they show
  their base. The tile mosaic of material 155 (one surface, one set) is 32
  units a repeat, about 10 cm tiles: its scale is the data's.
- **Runs need a 0x36 colour.** A run of 0x32 pictures alone was taken as a
  layer; as the base it drew the far vista towers (material 1137) black. Runs
  are now used only when each has a usage-0x36 picture.
- **Wet layers.** On the roof material the near-white layer's normal map has
  gloss 0.63 to 0.76 (the concrete's 0.29): a wet or puddle layer whose colour
  the material's per-layer tint row (`ff ff ff 00 RR GG BB 0a`, e.g. `87 87 87`)
  darkens. Which tint row belongs to which layer is not decoded, so the tints
  are not applied yet.
- `jup_shot --pick` lists every hit along the ray with its material's flags;
  `JUP_DBG_DENSITY=1` prints texture repeat sizes per material and per surface.

- **Driving MW3's own programs instead of the uber-shader: assessed, not
  reliable from the data (28 Oct 2026).** `jup_library` on the decoded
  `code_post_gfx` (350 family-12 pixel programs), grouped by pass bit and
  feature mask:
  - Programs the material data cannot tell apart. In the G-buffer passes (two
    targets, pass `0x2` and its twin `0x10`) feature mask 15 has 6 programs
    that read the same material-record offsets and the same engine buffers,
    differing only in code length (64,216 to 68,044 instructions); feature 3
    has 22 programs over 9 record-read sets and 6 engine-buffer sets. 9 of
    the 40 (pass, feature) groups have several programs with one record-read
    set. Whatever separates them is engine state (vertex format, lightmap or
    probe lighting, velocity), not the material, so a scorer over a material's
    populated fields can at best choose a group, and nothing in the data
    confirms the choice (the zones hold no link, the exe is packed).
  - The record the programs read is not on disk. Constants at 36 to 412 (94
    words), flags at 484 and 500, 45 picture indices from 516: the engine
    builds it at load. On disk a material has 9 to 18 rows of 16 bytes of
    packed bytes and halves (per layer: a `0a 00 01 01 ...` row, a tint row
    `ff ff ff 00 RR GG BB 0a` with four halves of 1.0, a row with one half such
    as 0.307 / 0.945 / 0.100); which record word each becomes is the engine's.
  - Engine buffers with stripped layouts. Family-12 pixel programs read 30
    resources besides the record: `t79` (255 programs), `t48` (184), `t78`,
    `t11`, `t49`, `t77`, `t76`, `t47` (180 each), `t23` (150), `t4`, `t3`
    (132), `t19`, `t22` (124), cbuffers `b1` (125), `b2` (70), `b0` (65), ...
    Lighting, clusters, decals and visibility live there; each would have to
    be rebuilt from its use in DXIL.
  The uber-shader stays. What would move this: a run-time capture of the
  record and buffers from the game (not possible here: game exes are not run).
- **Placed lights.** Highrise's entities hold no `light` entities (the
  reader finds records by entity origin), and a scan of the decoded zone for
  the light record's shape (`+20` 1.0, `+64` 3, a radius) finds 6 matches, all
  junk (radius 1). No local lights to draw from this zone; rooms stay lit by
  the sky term.
- **Beams checked.** `jup_shot --check-model <name>` prints each placement's
  box and the floor under it. `un_metal_steel_support_tubing_beam_8x4x256`
  (local +y, 0 to 256): vertical posts on a grid (x -1424 and -2304, 160 to
  250 apart, pairs at z 3037 and 3064) and 45-degree braces rising from the
  roof (forward (0, -0.71, -0.71), the beam's +y going up 181 units): a
  scaffold frame, regular. The instance's u16 after the scale (+0x0a, now
  `Instance::flags`) is 0 or 1 and does not go with tilt (39 and 40 per cent).

- **Layer tints from the colour rows.** A material's constant rows include,
  per layer, a row `r g b f  r g b f` followed by four halves of 1.0 (`f`
  0x00 or 0x0a, read as off or on; `MapMaterial::colour_rows`, found by that
  shape between the header and the next). The second colour, where on,
  multiplies its layer's picture for world materials (linear, byte / 255;
  784 layers on Highrise). UNVERIFIED which colour of a row tints what: the
  roof concrete's `81 7c 72` is the same on every material that layers it, and
  with it the view matched to the reference screenshot (`ref_match`,
  `1300,900,3300,14,210`) measures mean 0.43 0.34 0.26 and luma percentiles
  0.09/0.20/0.30/0.52/0.72, the reference 0.42 0.33 0.24 and
  0.12/0.21/0.30/0.45/0.67 (without: 0.47 0.37 0.28, median 0.36).
  `BO2MP_JUP_TINT=0` off.
- **Whites.** Albedo past 0.5 (linear) rolls off at a quarter (was 0.65 and
  an eighth).

Still off after this step: the Black Hawk on the helipad is placed rolled
about 66 degrees (checked above; the reference shows a level one, perhaps a
scripted or dynamic helicopter, 14 `script_struct_heli` entities); the crane's hanging loads are
large and tilted (plywood stacks at scale 4.0, 1,000 units over the roof; as
placed); effects (steam plumes) are not drawn; no screen-space ambient
occlusion or reflections.

## The exported guns (27 Oct 2026)

`CodLegacy export-jup <folder>` (or `cargo run -p assets --example jup_export -- <folder>`; `-- --check <folder>` reads an export back) writes Modern Warfare III's (2023) guns, camos, calling cards, emblems and gun sounds into one folder in our own converted files, so the guns can be used without the game installed. It goes through the lane's own build path (`prefetch_jup_guest_guns`), so the folder holds exactly what the lane reads.

Layout (packs are `<prefix>_NNN.pack` of at most 24 MB with a `<prefix>.idx` of `name<TAB>file<TAB>offset<TAB>len`):

- `guns/<gun>.jgun`: `BO2JGUN1` and an LZ4 block of the view and world models and the view top.
- `materials/`: `BO2JMAT1` blobs named `<hash>_<cap>` (tint plus up to five PNGs: colour, occlusion, normal, mask, gloss).
- `camos/`: camo PNGs and `camos.tsv` (number, reference, name, game).
- `sounds/`: `JOPS1` Opus clips and `aliases.tsv`; `icons/`: Create-a-Class gun and camo PNGs; `looks/`: calling cards and emblems (`looks.txt`).
- `manifest.json` (written last, it marks the export) and `README.txt`.

Using it: set `BO2_JUP_EXPORT=<folder>`, or leave it as `MW3_export` beside the exe, in the working folder, in `Documents` (also OneDrive), in the home folder or beside the `jup` game folder. With no MW3 install the lane, Create-a-Class (`guest::set_jup_export_guns`), the sounds, cards and icons read it; a full install always wins.

Re-exporting needs the full install, with its `.xpak`/`.xsub` texture packages and sound paks (the pictures, sounds, cards and emblems come from there): set `BO2_JUP_INSTALL` to it. On Linux set `JUP_OODLE_LIB` to a library exporting `ooz_kraken(src, len, dst, dstlen) -> i32` and `JUP_OODLE_DLL` to any existing file. An export made from a trimmed install (only the `.ff` zones and the small `.xpak` indexes) has the models and material tables but no pictures, sounds, cards or emblems; `Documents\MW3_export` on the author's PC is such a copy until re-exported on the full install. Fixes in the build stage (alignment, material colours) need no re-export; fixes in the read stage do. If `MaterialPixels` gains a field, update `encode_material`/`decode_material` in `crates/assets/src/lane/jup_export.rs`.
(Done since: see the next section.)

### Ambience beds (decoded offline; played, not heard)

The sound streams were pulled from the install with a byte-range extract
(`snd_extract.ps1` over `snd_plan.tsv`: 3,993 entries, 332 MB, from 14
`.xsub` parts of `mp23`, `map_shared`, `common23`, `codhq_shared`; a stream's
part is its index offset divided by 1 GiB, the stride of every family with
parts near 940 MB). All 1,955 of Derail's streams resolve, and the six
staged maps' entries decode through `xsub::decode_entry` and Opus (the 86
streams of 8 s or more on Derail, 65 of 40 s or more on the others, no
failures).

What the long streams are: on every map a set of stereo streams of 40 to 150
s, very quiet (-65 to -30 dBFS rms), with a steady envelope (a level spread
of 2 to 3 dB at 20 Hz), a low spectral centre (400 to 1,700 Hz), chained as
pairs (an alias's `next` hash names the second). They look like beds, not
music (music would be louder, with a moving envelope), but that is a
judgement from the numbers, not from listening. They are map-specific (no
alias is in two maps; Derail has 52 such aliases, Elevated 39, Afghan 11,
Farm 18 10, Das Haus 6, Shipment 4). The alias names are unknown (no plain
names in the zones; the `amb_emt_dashaus_*` strings are scriptable and
effect names), and no emitter table was found: the maps' entities carry no
sound emitters, and the beds' alias records hold no position. So the beds
are not placed: they play together under the match.

What plays (`asset_audio::JUP_MAP_BEDS`): per map, up to three beds chosen by
the measure above (steady, no beat, -45 to -25 dBFS), as alias hashes:
Derail `f28a23bd791e4aff`, `32dbbdd2f22cccf5`, `789d609bb45cda5b`; Shipment
`04132c7f81e65993`, `cb3d8667bcaf029f`; Das Haus `c0499974fae01b22`;
Elevated `c058de46c80a633c`, `acb290cc0a1b33dc`; Farm 18 `0d499d94578688c9`;
Afghan `38bc3ddffa4f75c6`, `f9a97fe6179dc668`. A match on one of these maps
takes the map zone's sounds into its bank (`want_jup_map`, source "map zone
(MW3 2023 ambience)", cached like Resurgence's rows) and the ambient boot
starts each bed as a 2D loop, raised to an rms of -38 dBFS (at most 8 x) at
a volume of 0.6. Nothing was heard (no sound device here): which beds the
game really layers, and where, is open. A bed is 20 to 40 MB of samples.

MW3's own water sounds (the splash on entering, wading steps): not found.
The hashes of the five shared zones' 66,475 aliases were tried against
13,110 guessed names of the shapes `plr_step_walk_water`, `plr_swim_enter`,
`water_splash` (none match; the method does find `match_start_tick`). The
splash and wet step use Black Ops II's water sounds through the existing
events.

### Exporting without the whole game on this machine (plan, cut, expand)

The texture and sound packages (`.xsub`) are tens of GB, so an export can be made from only the ranges it reads. `jup_export --plan <plan.tsv> <scratch>` runs the export against an install whose `.xpak` indexes and zones are real and whose `.xsub` parts are empty files of the right lengths (sizes from the install's listing), notes every package entry read (`fastfile_jup::pack::record_start`, `note_range` for sound streams, whose rows name their own place; a part whose own table cannot be read gets its rows from its zone `.xpak`, `xsub::part_entries`) and writes `file<TAB>offset<TAB>size<TAB>key` per entry. `export_extract.bat` (`export_extract.ps1`, beside `zone\` and the plan) cuts those ranges out of the parts to `export_out\pack_NNN.bin` (300 MB each) and `placed.tsv`. `jup_export --expand <export_out> <install>` writes them back at their offsets into the empty parts, after which a normal export of that folder has the pictures and sounds. Keys in no installed package (streamed from the network only) are counted and skipped.

`--expand` makes every part the install's `file_list.txt` lists, not only those a range falls in (empty files of the listed length, sparse), and the zones the export opens must be there too (`global`, `global_stream_mp`, `ingame`, `ingame_shared_mp`, the gun zones; a `bo2mp_pack.txt` beside `zone` stands in for `ingame_br.ff`). The sound reader picks the packages it reads by the parts it finds, so a part left out of the folder takes its sounds with it: the first export from the 12,832 ranges (only the 23 parts they fall in were made) had 10 of 909 gun sound aliases and 8 pictures; with all the parts, 909 aliases (3,159 clips) and 579 Create-a-Class pictures. A debug build of the export takes about 40 minutes (the calling cards), 3 GB out.

