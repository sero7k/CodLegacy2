# Map layout audit: Ghosts, Modern Warfare Remastered (+ Horizon MW usermaps), Modern Warfare III

Question: which maps "don't work or don't remotely look like a map" for the
readers, as opposed to the in-game lanes. Method: the existing probes on the
owner's zones, one map staged at a time, no image paks, nothing
rendered in a live match.

* Ghosts: `iw6_map_check` (walk, world, collision, spawns) and
  `iw6_map_render <zone> <prefix> top` (software top-down render; magenta in
  the PNGs is a colour map whose picture sits in an image pak that was not
  staged, not a layout fault).
* MWR / Horizon MW: `h1_map_top <zone.ff> <out.png>` (new, in
  `crates/fastfile_h1/examples/`): walk, one colour a world surface seen from
  above, spawns as white dots on the view their 2nd..98th percentile frames;
  prints bounds, how many spawns stand over a drawn surface, how much of the
  frame is covered.
* MW3 2023: **not runnable here.** `fastfile_jup::oodle::load` has no
  non-Windows backend (`oodle.rs`: "the game's Oodle DLL loads only on
  Windows"), so `jup_map` panics on Linux. Run on the PC (below).

`iw6_map_render` now frames the view on the 2nd..98th percentile of the spawns
(a parked spawn in a far corner stretched the view to nothing).

## Ghosts (IW6): 30 of 31 MP maps read and look like their map

Staged zone + `patch_` + `techsets_` of each, 29 maps. `walk` is exact, 0 bad
triangles, 0 vertices outside bounds, 0 aabb nodes out of range, every plane
unit-normal, spawns 120-300 a map. Top-down renders show recognisable layouts
(Strikezone's ring, Fahrenheit's dome, Hashima's island and causeway,
Pirate's quay, Zerosub's sub pen, Shipment NS's yard).

| game | map | status | symptom | likely cause (location) | fix size |
|---|---|---|---|---|---|
| IW6 | mp_dome_ns | FIXED 2026-10-27 | was: walk stopped at asset 757 of 3043 (`AnimClass`) | `walk/anim_class.rs` added; walks 3043/3043 exactly, 287k verts, 202k tris (0 bad), 4784 static models | done |
| IW6 | the other 14 base maps (chasm dart descent_new fahrenheit flooded frag hashima lonestar prisonbreak skeleton snow sovereign strikezone warhawk zebra) | OK | none in layout; colour is magenta only because no paks staged | | |
| IW6 DLC | zerosub favela_iw6 pirate conflict mine battery3 boneyard_ns shipment_ns ca_rumble | OK | as above (battery3 walks 3057 assets, shipment_ns 1947) | | |
| IW6 DLC | ca_behemoth ca_impact ca_red_river dig swamp zulu | OK (walk + world + clip only) | not rendered | | |
| IW6 | alien_* (Extinction) | not tested | the map list excludes them | | |

Minor, not layout: spawn records with no floor below them (hashima 19/148,
zulu 19/195, strikezone 14/314, snow 13/181, warhawk 12/176, behemoth 12/147,
prisonbreak 13/148, zebra 8/139) are the authors' air spawns, as in H1
(`placespawnpoint` drops them); worth confirming the lane drops them. Sovereign's
top view is dominated by distant high vista buildings (highest z wins in the
render), the play area is the bright block lower right.

## MWR retail (H1): 19 of 21 sampled read; Countdown and Creek do not

| game | map | status | symptom | likely cause (location) | fix size |
|---|---|---|---|---|---|
| H1 | mp_countdown | FIXED 2026-10-27 (walks 3841/3841; 383/383 spawns over a drawn surface, 98 % frame) | was: "walk stopped at asset 2510 (Image): zone ends at 0x9dbbbb0"; no GfxWorld, ClipMap or MapEnts, 0 spawns | walk desync before image 2510, or an image whose `dataLen1` overruns; `walk/material.rs:212` `image()` is where it throws | M (needs a debug session on the zone) |
| H1 | mp_creek | FIXED 2026-10-27 (walks 4290/4290; 380/380 spawns over a drawn surface, 100 % frame) | cause: `walk/world.rs` FxWorld glass defs read the physics preset after the materials; the file has it first (maps that define their own inline). was: same, asset 2496 (Image), zone ends at 0xbae46c9. Asset 2495 is the FxWorld, 2492-93 images | same; the FxWorld reader (`walk/world.rs:142`, glass system arrays) just before is the first suspect, unconfirmed | M |
| H1 | bloc broadcast backlot bog bog_summer cargoship carentan citystreets convoy crash crossfire farm farm_spring killhouse overgrown pipeline showdown strike vacant | OK | all spawns inside the frame and over a drawn surface (showdown 372/379, bloc 549/574), frame coverage 82-100 % (cargoship 24 %: a ship in open sea, as expected); 1.0-1.6 M drawn tris | | |
| H1 | crash_snow, shipment | OK per `docs/H1.md` (not re-run) | crash_snow draws grey without its techsets/patch zones | | |

Not sampled: mp_dome-like variants `mp_*_snow`/other retail zones past
`mp_farm` that were not listed (the directory listing was capped at 2000
entries).

## Horizon MW usermaps: 7 of 34 sampled, all read

| game | map | status | symptom | likely cause | fix size |
|---|---|---|---|---|---|
| HMW | mp_rust, courtyard_ss, highrise, afghan, favela, derail, underpass | OK | layouts read (underpass covers 59 % of its frame: it stands on a tall vertical stack, z span 110k; highrise spans z -86k..80k) | | |
| HMW | mp_terminal, mp_estate (and any other zone over 400 MB) | not tested | the staging tool refuses files over 400 MB | | |

Note: the readers are fine for all of these; the owner's "doesn't look like
a map" for MWR is therefore most likely the in-game lane: without the retail
`imagefile<N>.pak`s and `techsets_<map>`/`patch_<map>` zones a retail map
draws grey (docs/H1.md "Retail maps", "Known gaps"; Shipment: 748 of 762
colour maps in paks not installed), no light grid, one LOD, no reflection
probes. That is a look problem, not a layout problem.

## Modern Warfare III 2023 (JUP): not run

Cannot be decoded here (Oodle on Windows only). From `docs/JUP.md` the known
layout risks, in order:

1. Only Rebirth (`mp_jup_escape5`), Tokyo (`mp_jup_elevated`) and Rust are
   recorded as read. The other 20 `JUP_MAPS` (`asset_transport/src/discover.rs:266`)
   are listed by public name; none is recorded as checked.
2. Terrain (`map/terrain.rs`, `lane/jup_terrain.rs`): the ground between
   buildings is in no world surface; the height-field origin/size were fitted
   against Rebirth's coast only (from (-16225,-16225), 34900 a side). A map
   with terrain and different extents draws floating buildings.
3. Placed models (`map/models.rs`): header/surfs pairing by shape; Rust left 2
   of 954 headers unrecognised. A failure here once made MP maps draw
   without props and fall through.
4. Materials (`map/materials.rs`): 240 materials have picture tables not
   followed; terrain virtual textures draw without a picture; lightmaps unread.
5. Collision: the drawn world + terrain + coarse LODs; `col_map` not read, so
   thin geometry and props are wrong. Spawns are made over AI nodes.
6. MWII maps (`MWII_MAPS`) are guessed zone names.

Run on the PC to close this (PowerShell, from the repo):

    foreach ($m in 'mp_jup_afghan_v2','mp_jup_derail','mp_jup_estate','mp_jup_favela','mp_jup_highrise','mp_jup_invasion','mp_jup_karachi','mp_jup_quarry','mp_jup_rundown','mp_jup_rust','mp_jup_scrapyard','mp_jup_skidrow','mp_jup_subbase','mp_jup_terminal','mp_jup_underpass','mp_jup_wasteland','mp_jup_shoothouse2','mp_jup_shipment_iw9','mp_jup_das_haus','mp_jup_farm_18','mp_jup_growhouse','mp_jup_elevated') { cargo run -p fastfile_jup --release --example jup_map -- "C:\Users\Gaming PC\Documents\MW3" $m 2>&1 | Select-String 'world:|bounds|surface list|tile|terrain|no ' }

Look for: "no surface list", "no world zone", "tile k ...: packed triangle bytes",
"no world surface read", bounds far larger than the play area.

## Recommended fix order

1. Run the JUP loop above on the PC (no code; tells which of 22 maps fail).
2. ~~IW6 `AnimClass` reader (dome_ns)~~ done 2026-10-27.
3. ~~H1 Countdown/Creek walk desync~~ done 2026-10-27 (FxWorld glass def order).
4. Lane work for MWR/Ghosts look (paks, techsets) if "looks like a map"
   complaints are about untextured/grey maps rather than layout.
