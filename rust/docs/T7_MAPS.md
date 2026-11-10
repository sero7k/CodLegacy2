# Black Ops III (T7) maps

MP maps play by Black Ops II's rules; Zombies maps as Black Ops II Zombies
through the Infinite Warfare Zombies pipeline (`lane::iw7_zombies`).

| Part | Where |
|---|---|
| Walk of the map assets | `fastfile_t7/src/walk/{world,gfxworld,world_misc}.rs` |
| Map read into plain data | `fastfile_t7/src/map.rs` (`T7Map::load`) |
| Probe | `t7_map <zone.ff>` (`T7_MAP_LOAD=1` reads it whole, `T7_MAP_DUMP=ents` prints entities) |
| Collision | `asset_world/src/world_t7.rs` |
| Match world, Zombies layout | `assets/src/lane/t7_map.rs` (`load_t7_world`, `zm::read`) |
| Discovery | `asset_transport::discover` (`T7_ZONE_VERSION` 0x251, `TAff0000`, `is_t7_map`) |
| Hooks | `match_walk.rs`, `map_check.rs` (`CodLegacy check-maps`), UI list "Black Ops III", `CodLegacy map t7:zm_<map>` |

## What walks (retail, 14 Oct 2026)

Every MP map zone walks to its exact end, `mp_veiled_heyday` included.
Zombies: `zm_factory`, `zm_castle`, `zm_island`, `zm_zod`, `zm_genesis`
and `zm_stalingrad` walk to their exact end and read whole
(`T7_MAP_LOAD=1 t7_map`: world, static models, collision, entities).
`zm_common` and `zm_levelcommon` still stop (player sounds, a weapon).
`T7_TRACE` prints the walker line of each allocation; `t7_scan` finds
pointer runs where a list really starts.

What the Zombies maps needed: XCam's list at 0x68 (16 each, count at 0x70)
and its frame-list count in the u32 at 0x18 when bytes 0x14/0x15 are 0;
ComWorld's second tome list (0x68/0x70, a 24-byte record at a tome's 0x20)
and umbra gates (0x78/0x80, 16 each, a name first), a tome's u32 list
(0x40/0x48); GfxWorld's second tome count at 0x56c (0x568 is a flag);
SAnim's first list holds both u16 counts (0x64 + 0x66) when the first is
under three; LightDescription's 0x210..0x230 are runtime words; a decal
effect's second visuals may load a material rather than an image.

Retail vs x64-zt: GfxWorld exposure volumes 108; siege anim count at 0x578;
`unk` (0x1e8) 48 a record; second umbra list and auto-play list (20) load;
static model draws load posed bones (64) and a siege anim; interactive
water 192 (cells 40, outlines 32, wave clamps 48, a list of 72 at 0x90),
grid memory runtime; post-effect lists at 0x1610.. (vukko boxes 144), 0x1b40..
(bloom mods 384, boxes 464), 0x1b80.. (LUT boxes 144), 0x1ff0.. (colour mods
336, boxes 688), 0x2020/0x2030 (underwater boxes 164). ClipMap static models
load posed bones (32). SAnim: a list at 0x20 (28, count at 0x6c).
StreamerHint 0x28; ZBarrier 0x300 (name only); XCam lists at 0x78 and 0x88.

## What converts

World (IW3 builder, BO3 materials, its own light below), placed models (BO2
model builder), materials hidden by their words (sky, tools, clip,
shadow), entities (plain text, BO2 class names: read as a Treyarch map).
Collision is the zone's own: brushes under a k-d tree of their boxes (BO3's
BSP has a handful of nodes), the triangle mesh with each partition's
vertices in its own segment and all AABB roots under one root, plus the
coarsest LOD of each static model the clip map lists.

Zombies: `zm::read` gives `ZmLayout` (player_volume zones with a trigger
model each, spawners, exterior_goal windows, doors/debris, perks, box
spots, Pack-a-Punch, power switch, wall buys, dog spots, traversals) to
`prepare_layout`; `attach_t6_zm_rules` writes Buried-kit Zombies. Not
carried: teleporters, traps, Gobblegum, Dead Shot. `zm_factory` is also
Der Riese's name: registered as a foreign map, `bo1_maps` leaves it alone.

## Light (`assets::lane::t7_light`, `fastfile_t7::light`)

On by default; `BO2MP_T7_LIGHT=0` gives the old flat light. Only Black Ops
III maps take it. Read from the GfxWorld's first sun volume:

- Sky: the sky box model's material (`mc/sky_latlong_hdr`) names a BC6H
  lat-long picture (8192 x 4096) whose pixels are the sun volume's sky
  buffer (an xpak entry); `$Globals.skyScale` scales it. Lookup (sky shader):
  `u = 0.5 - atan2(d.x, d.y) / 2pi`, `v = acos(d.z) / pi`. Drawn as a 512px
  RGBA16F cube in the probes' unit.
- Probes: one BC6H cube array (256px, 7 levels), stored mip-major (all
  probes' faces at level 0, then level 1, ...), faces world +X -X +Y -Y +Z
  -Z. Configs 328 bytes: word 0 the probe's scale (texels times it are the
  game's light; probe 0's is the map's unit), 1..3 mean colour, 11..13
  origin, 14..22 rotation rows, 23..25 half size, 26..31 parallax box. The
  32 most used round the spawns are drawn (probe 0 first), each surface and
  model taking the smallest box holding it.
- Sun: the sun volume's pitch and yaw aim the light; toward the sun is
  `(-cos p cos y, -cos p sin y, sin p)` (matches the probes' brightest
  texel on Redwood, Nuketown, Apartments). Brightness: intensity over
  probe 0's scale.
- Baked at load (~1 s): a 4096px sun height map over the spawn points'
  square from the world and placed models' opaque triangles (cut-outs
  dithered), and per world vertex the ambient (probe ambient cube by
  normal, times each probe's scale over the unit) times a short-ray
  occlusion; placed models take the mean ambient at their origin.
- Shader (`t7_shade` in `geometry_diagnostic.wgsl`): albedo x (ambient +
  sun x N.L x sun map x moving shadows), plus with normal/specular maps the
  ambient bent by the probe's level 6, a GGX sun highlight and the probe's
  reflection at level `5 (1 - gloss)` (BO3's `lit` shader), split-sum
  weighted, the reflection corrected against the probe's parallax box
  (config words 29..31 centre, 26..28 half size, in its axes; BO3 uses six
  planes, words 46..70; `BO2MP_T7_PARALLAX=0` off).
- Fog (`t7_fog_core`): BO3's atmospheric fog, the maths of its forward
  `lit` and sky shaders (`CoreFogConstants`): optical length (distance past
  the start x density at the eye x the height falloff's (1 - e^-h) / h),
  transmittance `2^(length x total density)` eased by the extinction
  strength, plus Rayleigh (phase blended by the sun strength) and Mie
  (Schlick, faded in past the haze distance) in-scatter x (1 - T). Settings
  from the fog volume (`GfxWorld` 0x148, 624 bytes: planes, priority at
  0x20, a bank per lighting state at 0x40/0xd0/0x160) holding most spawns.
  Bank words as read (names ours; BO3 builds its constants from them in
  code not read): 1 start, 2 half distance, 3 base height, 4 half height,
  16..18 fog colour, 19 extinction, 20..22 haze colour, 23 haze start, 24
  haze spread, 25 Schlick k, 26 haze density, 28 in-scatter, 30 sun
  strength, 31 sky distance. The fog's in-scatter is the sky's horizon
  colour (words 16..18 as its colour were far too blue), the haze's its colour at the sun's light x 2 pi x
  its density. Added: the haze toward the sun is
  shadowed by the baked sun map along the first 2500 units of the ray (16
  steps), which draws the sun's shafts. `BO2MP_T7_FOG` (density scale, 0
  off), `BO2MP_T7_HAZE`, `BO2MP_T7_SHAFTS`.
- Tone curve: BO3's own (`bloom_apply_lut` in core_patch, fxc
  disassembly): `t = saturate(log2(x + 0.00873) x 0.072703 + 0.598206)`,
  then the polynomial `((((7.712947 t - 19.311527) t + 14.275167) t -
  2.490045) t + 0.878083) t - 0.06691`, x the light times the exposure
  (`BO2MP_T7_EXPOSURE`, default 1.6: BO3's auto-exposure is not drawn).
  Written without BO2's roll-off.
- Final pass: the map's colour table (the `GfxWorld` header's LUT material,
  a `create_lut2dv` source: 1024 x 1024 R10G10B10A2, 32 tables of 32 x 32
  per 32 rows; band 0) and a highlight bloom (ours; `BO2MP_T7_GRADE=0`,
  `BO2MP_T7_BLOOM`).
- The sun's light is its intensity over probe 0's scale times 1.5 (against
  pictures of the game; `BO2MP_T7_SUN` sets the factor). `BO2MP_T7_AMBIENT`
  scales the ambient light (a test aid).

- Probe volume (`gProbeX/Y/ZArray`, three BC6H 3D textures, 248 x 280 x
  608 on Redwood): an atlas of each probe's light volume. A probe's region
  is its config words 4..6 (uvw size) at 7..9 (uvw place), its box (half
  size 23..25, in its axes) mapped onto it; the depth's first half holds
  the positive directions, the second the negative (`t7_pvol` checks the
  regions are lit inside, dark round). As BO3's lit shader: the ambient is
  the probe's level 6 at the normal times the three axes' texels weighted
  by the normal's squares (here per vertex, trilinear, with a light contact
  occlusion; `BO2MP_T7_VOLUME=0` off). Words 34, 36, 37: the region's size
  in texels.
- Water: drawn as a clear turquoise sheet (sRGB 95, 170, 185, alpha 120)
  with its normal and specular maps; its colour map (a ramp for BO3's water
  shader) drew rainbow pools.

Not read: the sun shadow tree, the exposure grids and auto-exposure, the lit fog volumes
(360 bytes: local volumetric fog boxes; BO3's `volumetric_light_sun`
marches them with the sun's shadow map), BO3's bloom settings and lens
flares.

## Black Ops III's look on Black Ops II maps

`BO2MP_BO3_LOOK=1` (off by default; `assets::lane::bo3_look`): a Black Ops
II map is drawn by the same branch. Black Ops II supplies its textures
(colour, normal, specular with gloss in alpha), sun, sky cube, probes (`rgb
/ a`), fog and the light without the sun: the lightmap's ambient and
directional pages for the world (its vertex colours are tints or layer
weights, so not used for a baked light) and the light grid for models. A
sun map is baked at load from the world's and placed models' triangles
round the spawn points (the same bake), and the sun's shafts drawn through
it. Water, lava flow, tiles and flags keep their own shaders.
`BO2MP_BO3_EXPOSURE` (default 1.2) scales BO2's exposure into BO3's tone
curve.

## Guest guns' sounds

`asset_audio::sound_load_t7` reads `mp_common`'s aliases and plays their
FLAC clips from `snd/all/mp_common.all.{p.sabl,p.sabs,sabl,sabs}` (the
`.p` patch banks first: title-update replacements such as the ICR's
shot). An alias list's records are context alternatives, not random
variants: each has up to four (type, value) `SND_HashName` pairs at 0x74,
0x7c, 0x84, 0x8c (`water` over/under, `ringoff_plr`
outdoor/indoor/underwater, `movement` normal/quiet/loud, `plr_stance`,
`perspective`, `boot`). Played as variants, a gun's fire and decay picked
its underwater shot or tail about every other shot (the "water sound");
only the over-water, outdoor, normal-movement records are kept now.

## Camos (`asset_game::weapon_camo_models::T7_CAMOS`, `lane::t7::t7_camos`)

Black Ops III's camo list is `gamedata/weapons/common/attachmenttable.csv`
in core_patch: its "camo" rows, from `camo_none` (0) down, give the camo
number, the `MPUI_CAMO_*` key (en_core_patch: Jungle Tech, Ash, ...) and
the menu swatch (`menu_camo_<x>_pattern`, 168x168 in core_ui; 27 there).
`weaponoptions.csv` gives each camo's slot (counted from 1) in the guns'
camo sets (`WeaponCamo` `camo_t7_<gun>`: per slot its materials, which
replace the gun's base materials). core_common's sets hold 53 slots:
the base game's camos; the loot and DLC camos' materials are only in
core_patch, which the walk stops in (asset 112, read as an XCam), so those
are left out.

String tables: their cells are strings in the zone or references to
strings loaded earlier (`walk::simple::string_table` now keeps them,
`Capture::tables`, with the animations). core_patch's tables were read
with a fitted reference offset (virtual - file, about -4.6 MB there), the
attachment table's names being inline.

The 27 camos in core_common (camo numbers 1-26 and 42 Etching) are drawn
as patterns over every gun's materials, numbered `weapon_camos::T7_FIRST
+ n` (851-892): each its material's colour map (`_c`), else its specular
colour (Red Hex, Etching: their colour map is `$black_color`; Gold's
specular is in another zone, so core_common's gold picture
`i_wpn_t7_camo_gold_alt_c`), else its glow (Dark Matter, which is
animated in Black Ops III: its glow picture still). Create-a-Class's
BLACK OPS III tab (when Black Ops III is installed) shows the swatch, else
(Snow Job, Ritual, Etching: no swatch in core_ui) the picture drawn.
The pattern pictures are streamed (base `.xpak`s), so they were not
decoded here; the swatches were. `texture::image_levels_max` reads a
streamed picture whose smallest part holds fewer levels than the chain to
1x1 (Dark Matter's glow: one part of one level, 1024x1024), which was a
size error.

## Not done

Glass and doors static; map pictures (Zombies menu tiles show titles only:
BO3's menu art is in the front end's streamed packs).

## Zombies menu (22 Oct 2026)

ZOMBIES lists a BLACK OPS III page with the installed `zm_*` zones
(`asset_transport::list_t7_zombies_maps`): Shadows of Evil, The Giant, Der
Eisendrache, Zetsubou No Shima, Gorod Krovi, Revelations, keyed `t7:<map>`
(apart from Black Ops' Der Riese, also `zm_factory`) and started as `map
t7:<map>` with the foreign map script registered. Doors and debris join
their two zones from their flag's names (`connect_<a>_to_<b>`, `enter_`,
`activate_`), else the zones nearest the door and its brushes; no power
switch (Shadows of Evil, Zetsubou, Gorod Krovi, Revelations) means the power
is on from the start, as `zm_foreign_map.gsc` does.

Offline check (`BO2MP_CHECK_REPORT=zombies map_check_probe t7 <zone>`):

| Map | Zones (spawners) | Windows | Doors | Perks | Box | PaP | Walls |
|---|---|---|---|---|---|---|---|
| zm_zod | 59 (517) | 23 | 14 | 9 (no Widow's Wine) | 7 | yes | 19 |
| zm_factory | 12 (59) | 29 | 11 | 6 (no Deadshot) | 6 | yes | 10 |
| zm_castle | 34 (140) | 29 | 22 | 6 | 6 | yes | 17 |
| zm_island | 40 (473) | 36 | 20 | 6 | 5 | yes | 20 |
| zm_stalingrad | 41 (257) | 46 | 16 | 6 | 6 | yes | 13 |
| zm_genesis | 65 (219) | 48 | 19 | 6 (no Widow's Wine) | 10 | yes | 20 |

Not carried: Gobblegum, specialist weapons (Beast mode, the Wonder Weapons,
dragons, Apothicon), teleporters and trains, traps, the bosses and easter
eggs, Widow's Wine and Deadshot perks, the BO3 guns in the box (Black Ops
II's Buried kit stands in for weapons: `wall_weapon`). Nothing was played on
a GPU yet.
