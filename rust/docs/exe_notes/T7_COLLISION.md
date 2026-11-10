# Black Ops III (T7) collision: exe notes (2026-11-03)

Static reading only (Ghidra 11.4.2 headless on `BlackOps3.exe`, the project
in `/home/claude/bo3ref/gh`, `-noanalysis -readOnly`). Nothing was run.
The Rust side is `fastfile_t7::map::read_clip`, `assets::lane::t7_map::collision_of`
and `asset_world::world_t7`; see `docs/T7_NATIVE.md`, "Collision".

## The player's contents mask

Every instruction carrying one of the IW/T6 player masks as an immediate
(script `ScalarSites.java`, below): 0x2818011 at 24 sites, 0x2810011 at 3,
0x810011 at 9, 0x10011 at 25, 0x2800011 at 1.

* 0x1401d84f0 (client spawn, after the hull): when the client's spawn flag
  (bit 54 of the 64-bit flags at +0x10) is set it writes the player box
  (-15, -15, 0)..(15, 15, 70) at +0x1b4.. and **0x2818011** to the entity's
  clip mask (+0xa8); under two dvars (0x149ad1a80, then 0x149ad1f00) and a
  client bit (+0xb0c bit 7) the mask becomes 0xc00011 instead.
* 0x1401e9fb0: a box trace for a client with 0x2818011, or 0x2810011 when
  the client's session record (+0x1190) is set.
* 0x141b3ab80: 0x2810011 less every bit an entity chain removes (+0x214).

So Black Ops III's player mask is Black Ops II's 0x2810011 plus 0x8000.
No brush, triangle or static model on Nuk3town, Sector, Evac or Metro
carries 0x8000 (`t7_clip_census`), so leaving it out changes nothing on
these maps; it is not the cause of invisible walls (the mask stops more
in Black Ops III, not less).

## Static model placement (`cStaticModel_t`, 0x70)

+0x10 contents, +0x14 origin, +0x20 inverse scaled axes (3 x vec3), +0x60
posed bones, +0x68 bone count. The IW/T trace moves a point into the
model's space as a row vector times that matrix
(`clipmap_iw4::xmodel_trace::world_to_local`: `local[j] = sum_i d[i] *
inv[i][j]`), so the model's axes are the matrix's **columns**. The port had
read them as rows: every turned static model collided turned the other
way (yaw t as -t). Checked against the drawn placements (the `GfxWorld` static model list):
on Nuk3town 2185 of the 2260 placements matched to a drawn one had other
axes, every one the exact transpose; read by column, 0 differ. The exe's own
static-model trace was not found (its world trace is reached through a
table of function pointers, 0x1431129d0 + 0x90 x type); the numbers above
are the proof used.

## Brushes stored with sides missing (`cbrush_t`, 0x70)

Layout as read (Nuk3town brush 1248, raw words): +0 mins, +0xc contents,
+0x10 maxs, +0x1c side count, +0x20 sides (16 each: plane reference,
contents, surface flags), +0x28 axial contents [2][3], +0x40 axial surface
flags [2][3], +0x58 corner count, +0x60 corners (vec3), +0x68 8 bytes 0.
Brush 1248 is a box turned 18 degrees, 8 units thick, with 8 corners and
side count 3: the sides are its +x, +y and -y faces; the -x face (normal
(-0.95, -0.31, 0), distance -1792) is in no side, and the next side in the
array is the next brush's. Traced by its six bounds and three sides alone,
the bounding box's corner past the missing face stands solid up to 250
units from the brush. Counted over every player-solid brush (the solid of
the planes against the hull of the corners, more than 1 unit outside):
Nuk3town 931 of 1826, Sector 1514 of 5170, Evac 4290 of 14642, Metro 975 of
5753; some brushes keep no upright side at all (Evac 16812: six bounds,
eight turned corners). So the exe cannot be tracing these by their sides
alone; the corners are the brush as built. Its brush trace was not found
statically (searched for functions using +0x1c/+0x20/+0x58/+0x60 with 16-
and 12-byte strides: too many candidates, none confirmed), so how it uses
the corners is not known; the port adds the corners' missing faces as
sides (`fastfile_t7::map::sides_from_corners`).

## Not found / not done

* The static-model trace function itself (whether it tests the static
  model's own contents +0x10 against the mask before its surfaces').
* What 0x8000 names in Black Ops III.
* The brush trace, and how it uses the corners (+0x58/+0x60).

## Scripts

`docs/exe_notes/ghidra_scripts/ScalarSites.java` (every instruction whose
scalar operand is one of the given values, with its function: `-postScript
ScalarSites.java <out> 0x2818011 0x2810011 ...`) and `InsnRegex.java`
(functions whose instruction texts match at least N of the given regexes:
`-postScript InsnRegex.java <out> N <regex>...`).
