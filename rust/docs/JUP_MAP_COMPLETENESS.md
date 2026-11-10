# MW3 / MWII map completeness (native renderer lane)

Per map: textures, effects, lights, water, sky. Repeatable test for textures:

`cargo run -p fastfile_jup --example jup_map_coverage -- <install> --listing <install>/file_list.txt [--misses] [--plan out.tsv] [--max-side 2048] <zone name>...`

It reads the map's materials and pictures (no meshes) and resolves each picture
against the zone `.xpak` indexes only (no `.xsub` data): `inline` (the zone holds
the pixels), `xsub` (the largest group is in a streamed part of the install),
`xsub-partial` (only smaller groups are; the picture is drawn at a lower size),
`cdn` (only a `*_cdn.xpak` download index names it), `missing` (no index names it,
no pixels in the zone), `unsupported` (format byte not decoded: BC6H probes and
sky, masks of other formats). `--plan` writes `partfile, offset, size, key` rows
for a bounded extraction from the install's parts.

## Loader faults found by the probe (fixed 2026-10-27)

- Highrise did not load at all: its world surface list (128-byte records) was
  taken as the longest run of plausible records, and one surface with
  `step = 1.0` (a huge mesh, origin about -735000) broke the run at 384 of 2268.
  The step bound is now 64.
- Highrise, Das Haus and Derail had no material for any surface: the
  `gfx_map` material table can hold `-2` elements (the material header is stored
  inline after the array), which ended the run of back-pointers (Highrise: 607
  elements, 545 + 1 + 23 after the first inline one, 37 before it). The table is
  now the run grown over those elements; inline ones are matched to the next
  material header in asset order.
- Derail's placed models were not read (2913 xmodel headers for 2915 entries; the
  run-by-run pairing gave up). Pairing is now an alignment by kind that skips the
  entries beside the widest header gaps.
- `terrain::height_grid` multiplied an unchecked u32 (debug-build panic).

## Textures (offline; pictures bound through shared tables and stub hashes, 27 Oct 2026)

Counts are `jup_map_coverage` after the loader reads shared picture tables and
resolves stub pictures in the other zones (`docs/JUP.md`, "Pictures a material
names without holding them"). "No colour" is materials with no colour picture
bound at all; "cdn only" is pictures whose largest group is only in a download
index (drawn at the lower sizes the install holds); "missing" has no index
entry. The five indexes `boot`, `boot_shared`, `codhq23a`, `codhq23b`,
`codhq_shared` are still not staged.

| map | materials | no colour | pictures | inline | xsub full | cdn only (partial) | missing | unsupported |
|---|---|---|---|---|---|---|---|---|
| mp_jup_highrise | 1422 | 136 | 2616 | 115 | 907 | 1512 | 18 | 64 |
| mp_jup_das_haus | 1282 | 34 | 2292 | 103 | 664 | 1489 | 0 | 36 |
| mp_jup_derail | 2387 | 235 | 4120 | 160 | 1493 | 2362 | 0 | 105 |
| mp_jup_elevated | 2300 | 207 | 3842 | 143 | 1381 | 2222 | 0 | 96 |
| mp_jup_farm_18 | 2148 | 136 | 2870 | 110 | 1025 | 1652 | 0 | 83 |
| mp_jup_afghan_v2 | 1120 | 148 | 2335 | 112 | 845 | 1290 | 0 | 88 |
| mp_jup_shipment_iw9 | 386 | 35 | 731 | 28 | 261 | 430 | 0 | 12 |

Highrise in the native renderer (headless, `highrise2` renders): 1286 of 1422
materials have a colour picture; 1008 of those (137k triangles) have their
pixels in the extracted pack, 278 (26.8k triangles) do not (the delta pack
`hr_extract2.bat` cuts 2955 more ranges, 85.9 MiB). 25 stub hashes stay
unresolved; reflection probe groups are in no staged index (6 of 56 probes).

Effects, lights, water and decals per map are not probed yet: placed effects go
through `crates/assets/data/jup_fx_standins.tsv` (no per-map review done).

## Extraction packs per map (27 Oct 2026)

`jup_map_coverage --max-side 1024 --plan` per map, minus ranges already in the
Highrise packs. Each folder (`MW3\<map>_out\`) holds `<map>_plan.tsv`,
`<map>_extract.ps1`, `<map>_extract.bat` (writes `pack\pack.bin`, `index.tsv`,
`done.txt` there); point `JUP_PACK_DIR` at the `pack` folders (path-list).

| map | ranges | MiB |
|---|---|---|
| das_haus | 4049 | 136.3 |
| derail | 14961 | 525.1 |
| elevated | 11827 | 343.2 |
| farm_18 | 7187 | 254.7 |
| afghan_v2 | 7048 | 1002.9 |
| shipment_iw9 | 1643 | 59.6 |

## Effects, lights, decals per map (entities read, 27 Oct 2026)

No map's entity text carries an effect field (no `fx`/`effect`/`.vfx` value on
any entity of the seven maps): placed effects are not in `map_ents`, so the
stand-in table has nothing to place from entities. Placed `light` records
(`PlacedLight`): Shipment 10, Farm 18 4, the rest 0 (their lights are baked into
the volume lighting). Decals are surfaces with blended or cut-out materials
(drawn with their materials; nothing separate). Effects will need the zone's
effect-placement asset, not read yet.

`jup_shot` render check, all seven maps load and render (`maps3` folder): Das
Haus, Elevated and Afghan read as their material pictures arrive (Afghan,
Derail and Farm 18 are mostly grey until their packs are cut).

## Look fixes (28 Oct 2026)

`maps4` renders: Shipment, Farm 18, Das Haus, Derail, Elevated and Afghan now
read with balanced light with no pictures staged (stand-in albedo, sky ambient,
sky visibility), where `maps3` had Shipment washed out and Farm 18 flat grey.
Highrise `highrise4`: blend masks are BC4 (they were read as BC1 noise), the
planar mapping density is bounded. See `docs/JUP.md`, "Look fixes".
