# Call of Duty: Ghosts (IW6 MP, PC x64) - collision, traces, movement, bullets: behavioural spec

Static reading only (decompile of `iw6mp64_ship`, unpacked exe, string tables). Nothing was run or decrypted.
Tags: **[C]** = CONFIRMED from decompiled code/data; **[I]** = INFERRED (pattern, naming, or partial reading).
Addresses appear only in the Evidence section (section 9). Units are game units (inches). Z is up.

---------------------------------------------------------------------------------------------------

## 0. Headline findings

1. **Every player/AI/vehicle trace is a swept capsule, never a swept box.** A `Bounds{mid[3], half[3]}` is turned into radius `r = min(half.x, half.y)` and cylinder half-height `h = half.z - r`; the trace start/end are offset by `mid`. A point trace is the special case `half = 0`. [C]
2. **Static (placed) model collision is only used by point traces that ask for it** (bullets, sight/visibility style traces). It is a *ray vs triangle soup* test against the xmodel's collision surfaces. Capsule (player) traces never see static-model geometry; they only see the map's clip brushes/terrain. This is exactly the "known gap" in IW6.md. [C]
3. Trace order for a general trace: **world clipmap -> (point traces with the flag) static models -> entities (sector tree)**. Each stage only accepts hits with `fraction < current fraction`. [C]
4. The world is **BSP nodes -> leaves -> {leaf-brush kd-tree -> brushes} + {collision AABB trees -> terrain partitions -> triangles + vertical "borders"}**. Same family as the IW3/IW5 `clipMap_t` the project already parses. All epsilons are `0.125` (distance) and `4.7683716e-07` (parallel test). [C]
5. Surface type = `(surfaceFlags >> 20) & 0x1f`, 30 named types + default (list in section 8 and `surface_types.tsv`). `nopenetrate` (0x100) forces type 0, which gives penetration depth 0. [C]

---------------------------------------------------------------------------------------------------

## 1. Runtime structures the traces read

All of this is the in-memory form of `clipMap_t`; field order below is as the **code accesses it**. Offsets are bytes. The project's reader already produces these arrays; the table says which clipMap array each access corresponds to (names follow the IW5/IW6 asset layout, mapping marked [I] where only the access pattern proves it).

### 1.1 clipMap_t fields used (offset from the clipMap base) [C offsets, I names]

| off | content | element stride |
|---|---|---|
| +0x18 | materials (`ClipMaterial`: +0 name ptr, +8 surfaceFlags u32, +0xC contentFlags u32) | 0x10 |
| +0x48 | leafBrushNodes | 0x18 |
| +0x60 | numBrushes (u16/u32) | |
| +0x68 | brushes (`cbrush_t`) | 0x30 |
| +0x70 | brushBounds (`Bounds`: mid[3], half[3]) | 0x18 |
| +0x78 | brushContents (u32 per brush) | 4 |
| +0x88 | numStaticModels | |
| +0x90 | staticModelList | 0x50 |
| +0xA0 | nodes (`cNode_t`) | 0x10 |
| +0xB0 | leafs (`cLeaf_t`) | 0x28 |
| +0xC0 | verts (vec3, referenced in segments of 1024) | 12 |
| +0xD0 | triIndices (u16 triples) | 6 per tri |
| +0xD8 | triEdgeIsWalkable (bit array, 3 bits per tri) | bit |
| +0xF8 | partitions (`CollisionPartition`) | 0x10 |
| +0x108 | collAabbTrees (`CollisionAabbTree`) | 0x20 |
| +0x110 | numSubModels (cmodels count) | |
| +0x118 | cmodels | 0x50 |
| +0x170 | static-model sector tree cells | 0x1C |

Brush models (cmodels) point at the **same arrays** through a per-model "clip data" pointer (the world's clipMap or a model's own collision data); the trace code always reads brushes/materials/leafbrush-nodes through a `clipdata` pointer held in the trace work struct, so an entity brush model and the world run identical brush code. [C]

### 1.2 cNode_t (16 B) [C]
`{ cplane_t* plane @+0 ; i16 children[2] @+8 }`. `children[0]` = front (dist >= 0), `children[1]` = back. A negative child value `c` means leaf index `-1 - c`. Point-in-tree descent uses `side = (dot(p, n) - dist) >= 0 ? children[0] : children[1]` (axial planes use `p[axis]`).

### 1.3 cplane_t [C]
`{ float normal[3] @0 ; float dist @0xC ; u8 type @0x10 ; ... }`. `type < 3` = axial plane on axis `type` (only `p[type]` used); `type >= 3` = general plane.

### 1.4 cLeaf_t (0x28 B) [C offsets / I names]
```
+0x00 u16 firstCollAabbIndex      (index into collAabbTrees)
+0x02 u16 collAabbCount
+0x04 u32 brushContents           (OR of contents of all brushes under leafBrushNode)
+0x08 u32 terrainContents         (OR of contents of terrain AABB roots)
+0x0C Bounds bounds               (mid[3] @0xC, half[3] @0x18)
+0x24 i32 leafBrushNode           (index into leafBrushNodes; 0 = no brushes)
```
Brushes are tested **before** terrain; terrain is skipped when the brush walk already drove `fraction` to 0 (all-solid) and mask & brushContents matched.

### 1.5 cLeafBrushNode (0x18 B) [C]
```
+0x00 u8  axis
+0x02 i16 leafBrushCount     > 0  : leaf, brush index list (u16[]) at pointer +0x08
                             == 0 : pure interior node
                             < 0  : this entry's own brushes live in the *next* entry (+0x18); then continue with children
+0x04 u32 contents           (mask test: node skipped if (mask & contents) == 0)
+0x08 float dist  (interior)  | ptr u16* brushes (leaf)
+0x0C float range (interior)
+0x10 u16 child[0]  (offset in 0x18-units from this node)
+0x12 u16 child[1]
```
This is a loose kd-tree: children overlap by `range`. Traversal (box/capsule extent `e = extents[axis]`, `off = e + 0.125 - range`; `p1 = start[axis]-dist`, `p2 = end[axis]-dist`):
* if `min(p1,p2) >= off`: if `max(p1,p2) <= -off` -> nothing here; else go `child[0]`
* else if `max(p1,p2) <= -off`: go `child[1]`
* else split: if `|p2-p1| <= 4.7683716e-07` visit `child[0]` then `child[1]` with the full segment; otherwise the near side is `child[ (p2>=p1) ? 1 : 0 ]` visited first for the segment `[0, (|p1| ... )]`, then the far child for `[frac2, 1]` where
  `frac1 = (s + off)/|p2-p1|`, `frac2 = (s - off)/|p2-p1|` with `s = (p2>=p1) ? -p1 : p1`, `frac1` clamped to <= 1, `frac2` clamped to >= 0. The far child gets the **original end point**.
* prune: a split visit returns immediately if `tr.fraction <= startFraction` (the "w" component of the interpolated point; see 2.3).

### 1.6 cbrush_t (0x30 B) and sides [C]
```
+0x00 u16 numsides            (number of NON-axial sides; the 6 axis-aligned faces are implicit)
+0x02 u16 glassPieceIndex     (0 = none; != 0 -> hit-filter callback, 1-based)
+0x08 ptr sides               (array of 16-byte cbrushside_t)
+0x18 u16 axialMaterial[2][3] ([0][axis] = min-face material, [1][axis] = max-face material)
+0x24 u8 firstAdjacentSideOffsets[2][3], edgeCount[2][3]   (bevels; not used by trace paths examined)
cbrushside_t (0x10): +0 cplane_t* plane, +8 u16 materialIndex
```
Brush bounds come from `brushBounds[i]` (`Bounds`), contents from `brushContents[i]`, per-face surface flags from `materials[materialIndex].surfaceFlags`. So a brush carries **one contents value, many surface-flag values**.

### 1.7 CollisionAabbTree (0x20 B) [C access / I name]
```
+0x00 float mid[3]
+0x0C u16  materialIndex       (-> materials[]: contents @+0xC, surfaceFlags @+8)
+0x0E u16  childCount          (0 => leaf, refers to a partition)
+0x10 float half[3]
+0x1C i32  firstChildIndex (childCount>0) | partitionIndex (childCount==0)
```
The **root** entries listed in a leaf (`firstCollAabbIndex..+count`) carry the material for everything beneath them: the trace tests `mask & materials[root.materialIndex].contents` once per root and reports that material's surfaceFlags/contents for any triangle hit below. Traversal is an explicit stack over children; each child is rejected by a separating-axis box test (3 face axes, plus 3 edge cross axes unless the segment is "short" relative to the box product test, see work struct 2.2).

### 1.8 CollisionPartition (0x10 B) [C]
```
+0x00 u8 triCount
+0x01 u8 borderCount
+0x02 u8 vertSegment             (vertex base = verts + vertSegment * 0x3000 bytes = 1024 verts)
+0x04 i32 firstTri               (index of first triangle in triIndices; tri = 3 x u16 into the segment)
+0x08 ptr borders                (CollisionBorder array, 0x1C B each)
CollisionBorder: float distEq[3]@0 (xy normal in [0],[1], plane dist in [2]); float zBase@0xC; zSlope@0x10; start@0x14; length@0x18
```
Each partition is visited **at most once per trace** (per-partition stamp array compared with a per-trace counter), because several AABB leaves can point at one partition.

### 1.9 Static models (collision) [C structure of the loop / I field names]
* `staticModelList[i]` (0x50 B): `+0 XModel*`, `+0x1C..` bounds-in-world (for the sector culling), `+0x14..+0x38` a 3x3 world->model axis (used both to bring the ray into model space and the normal back), `+8/+0x10` origin. Static model sector tree cells (0x1C B) hold `u16 first, u16 count` at +0x18/+0x1A; if `first < numStaticModels` the cell lists models directly, otherwise it is an interior cell and `first - numStaticModels` indexes child cells.
* XModel: `+0x200` u32 contents (mask test per model), `+0x1F2` i8 (<0 = no collision), `+0x1F4` int numCollSurfs, `+0x1F8` collSurfs array (0x24 B each: bounds 0x18 B, `+0x18` bone/part id, `+0x1C` contents, `+0x20` surfaceFlags). Collision geometry comes per LOD part (stride 0xE8 in a per-LOD table, `+0x74` count, `+0xA0` list): per part `triangles = u16 idx triples`, `vertices = 0x20-byte records (x,y,z,pad...)`.
* Hit effect on `trace_t`: `fraction`, normal (rotated back to world), `surfaceFlags = collSurf.+0x20`, `contents = collSurf.+0x1C`, `hitType=1`, `hitId=0x7FE` (world/"no entity"). [C]

### 1.10 Dynamic entities & brush models [C/I]
* Entities are `gentity_t` of stride **0x2F0**. Fields read by the entity-trace stage: `+0x124` contents (u32), `+0x128..+0x13F` local AABB (6 floats, added to the trace box for the broad phase), `+0x109` u8 collision kind, `+0x10A` flags (bit 1/2 = "has hitboxes", bit 2 = alt hitbox set, bit 3), `+0x10C` model/cmodel index, `+0x140` origin, `+0x14C` angles. [C offsets]
* Kind 0 and kind 4 route to a "transformed cmodel trace" (`kind 0`: box model index from `+0x10C`; `kind 4`: brush model at `+0x8C`): trace is rotated into model space (axis from angles), run against the cmodel's leaf (same brush/terrain code as the world, incl. terrain), then the normal is rotated back. Capsule against a **rotated** brushmodel that has terrain is flagged by the original developers as inexact. [C routing, I kind names]
* If the trace asked for hitboxes (bullets) and the entity has a DObj with hitbox geometry, the entity is traced bone-by-bone (result gives `partName`, `partGroup`) instead of with its AABB; broad-phase box for characters is about 64 (xy) by 52 (z half-height) with a +20 z shift when no alt hitbox. [C numbers, I meaning]
* Entity links are a spatial sector tree: 0x18-byte sector nodes `{u32 mask, u32 mask2(contents union), u16 firstEntity, ... , float dist@+0xC, u16 axis@+0x10, u16 child[2]@+0x14}`, entity chain next-pointer at `+0x2` of each 0x18 entry. Traversal is the same near/far split as 1.5 with `+0.0 extents`. [C pattern, I link semantics]
* Self/owner exclusion: the "ignore" argument is a small struct `{ignoreEntNum, ownerEntNum, flags 0x01010001}`; an entity is skipped if its number equals `ignoreEntNum` (when flag 0), equals the ignore entity's owner (flag 1), or its own owner equals the ignore entity/owner (flags 2/3). [C]

---------------------------------------------------------------------------------------------------

## 2. Trace algorithm

### 2.1 `trace_t` (0x2C B zeroed, then fraction=1) [C]
```
+0x00 float fraction            (init 1.0; final clamped to <= 1.0)
+0x04 float normal[3]
+0x10 u32 surfaceFlags
+0x14 u32 contents
+0x18 int hitType               (0 none, 1 entity/world, 2 dynent model, 3 dynent brush, 4 glass)
+0x1C u16 hitId                 (0x7FE = world, 0x7FF = none; else entity number; for glass = glass piece+?)
+0x1E u16 modelIndex
+0x20 u32 partName
+0x24 u16 partGroup             (hit location group for hitbox hits)
+0x26 u8 allsolid
+0x27 u8 startsolid
+0x28 u8 walkable               (set in finish step, see 2.9)
+0x29 u8 getPenetration         (input flag: position test + push-out vector instead of sweep)
+0x2A u8 removePitchAndRoll     (input flag from player state)
```
The wrapper accepts a 3-byte option block `{getPenetration, removePitchRoll, skip}`; if `skip != 0` the trace returns with fraction 1.

### 2.2 Work struct built once per trace (what the brush/tri code reads) [C offsets]
Given start `S`, end `E`, bounds `B`:
* `S' = S + B.mid`, `E' = E + B.mid` (stored at +0x40 / +0x50); `delta = E'-S'` (+0x70); `len = |delta|` (+0x7C); `len2 = |delta|^2` (+0x80); `len_xy = |delta.xy|` (+0x84).
* `half = B.half` (+0x60..0x68); `r = min(half.x, half.y)` (+0xB8); cylinder half-height `h = half.z - r` (+0xC4; +0xBC,+0xC0 = 0). So the capsule axis is always vertical `(0,0,h)`. `boundRadius = |half|` (+0xD4).
* Axis-parallel flags (+0xF8..): `delta[axis] == 0`.
* Segment helper (first 0x40 bytes): `mid = (S'+E')/2`, `halfDelta = (E'-S')/2`, `|halfDelta|`, `1/|delta[axis]|` (0 if `|delta[axis]| < 1e-6`).
* Whole-move AABB (mid at +0x8C, half at +0x98): `half_move[axis] = |S'-mid|[axis] + r` (so +r on x/y/z) with z additionally `+ h`.
* `isPoint` (+0xA8) = `half.x+half.y+half.z == 0`.
* `longMove` (+0xA9) = `(2*hx_m)(2*hy_m)(2*hz_m) < r*r*... *16*len`-style heuristic that disables the 3 cross-product axes of the box-vs-box overlap test (cheap broad phase flag). [C formula, I purpose]
* A per-trace id (thread-local counter) is stamped on each visited partition.

### 2.3 Tree descent (world BSP nodes) [C]
Recursive near/far split; interpolated points carry a 4th component `w` = the fraction already achieved at that point (start w = 0, end w = tr.fraction at call time).
For node plane `n,d`: `t1 = side(S) - d`, `t2 = side(E) - d` (axial: `p[axis]-d`).
Offset `off`:
* axial plane: `extents[axis] + 0.125` (extents = `half` x,y,z; **not** the capsule radius)
* general plane: if point trace `0.125`, else `|half| + 0.125` (bounding sphere of the extents, conservative)

```
if min(t1,t2) >= off            -> child[0]  (front)
else if max(t1,t2) <= -off      -> child[1]  (back)
else (straddles):
    if tr.fraction <= w_start: return
    if |t2-t1| <= 4.7683716e-07: first=child[0], f1=1, f2=0
    else: first = child[(t2 >= t1) ? 1 : 0];  s = (t2>=t1) ? -t1 : t1
          f1 = (s + off)/|t2-t1|;  f2 = (s - off)/|t2-t1|;  if f1 > 1: f1 = 1
    recurse(first, S -> lerp(S,E,f1));  then continue iteratively with
    S := lerp(S,E,max(f2,0)) and second = the other child (E unchanged)
at a leaf: leaf trace (2.4)
```
(The same machinery exists twice more: a boolean "sight" version without result data, and a position-test version that gathers *all* leaves overlapping a box - `|dot| classification with -0.001` - for `getPenetration`.)

### 2.4 Leaf trace order [C]
```
if tr.fraction == 0: return
if (mask & leaf.brushContents) == 0  OR  !leaf_AABB_rejects_segment:   (cheap leaf-bounds + segment test; bounds expanded by r on x/y and r+h on z)
        walk leafBrushNode tree from leaf.leafBrushNode ... (1.5), testing each listed brush (2.5)
if (mask & leaf.terrainContents) != 0:
        for each collAabb root in [first, first+count): if tr.fraction==0 break;
              if (mask & materials[root.materialIndex].contents): walk the AABB tree (1.7),
              test partitions (2.6/2.7); on improvement copy that material's surfaceFlags/contents.
```
The leaf-bounds shortcut: the leaf AABB (inflated by the capsule extents) is tested against the segment with a slab + cross-axis test using the stored `1/|delta|`-style values; if the segment misses it the whole brush walk is skipped (`FUN_1403f1c90`-style early out also returns "tr.fraction==0" to cut off terrain).

### 2.5 Brush vs swept capsule (the core test) [C]
Inputs: segment `S',E'`, `delta`, `invLen[axis]`, extents `ext[axis] = r (+h on z)` (stored +0xC8..+0xD0 as `|axis component| + r`), running `enter = 0`, `leave = tr.fraction`, `clipSide = none`, `getout = false`, `startout = false`.
1. If `(mask & brushContents[b]) == 0` return.
2. **Axial slabs (3 axes)**. For axis with `delta[axis] != 0`:
   `sgn = delta[axis] < 0 ? -1 : 1`; `m = (segMid[axis]-brushMid[axis]) * sgn`; `hd = |halfDelta[axis]|`; `e = ext[axis] + brushHalf[axis]`;
   * `endPast = m + hd - e`; if `endPast > 0`: `leaveA = (2*hd - endPast) * invAbsDelta[axis]`; if `leaveA <= enter` -> **no hit, return**; `getout = true`; `leave = min(leave, leaveA)`.
   * `startBefore = hd - e - m` (distance start->near face); if `startBefore > 0`: `enterA = (startBefore - 0.125) * invAbsDelta[axis]`; if `leave <= enterA` return; if `enterA > enter` (or no clip side yet): `enter = enterA`, `clipSide = axial face (sgn<0 ? max-face : min-face)`, normal = `-sgn` on that axis, material = `axialMaterial[sgn<0][axis]`.
   For `delta[axis] == 0`: if `|segMid - brushMid| - (brushHalf + ext) >= 0` return (outside slab).
3. **Brush side planes** (`numsides`, non-axial): for plane `(n, d)`:
   `D = |dot(n,(0,0,h))| + r + d` (capsule support offset; when the capsule axis is vertical this is `|n.z|*h + r + d`);
   `d1 = dot(n,S') - D`, `d2 = dot(n,E') - D`.
   * `d1 <= 0` (start inside this plane): if `d2 > 0`: `getout = true`; if `(d1-d2)*leave < d1` then `leave = d1/(d1-d2)` and if `leave <= enter` return.
   * `d1 > 0` (start outside): `if min(d1,0.125) <= d2` return (ends outside / hasn't pierced by more than 0.125); if `d2 > 0` getout = true; candidate `c = (d1-0.125)/(d1-d2)`; if `c > enter`: `enter = c; if leave <= enter return`; clipSide = this side (normal = plane normal, material = side material). If `c <= enter` and no clipSide yet, still record this side.
4. If `glassPieceIndex != 0`: call the owner's hit-filter with `glassPieceIndex-1`; if it returns false the brush is ignored.
5. Result: `tr.contents = brushContents[b]` in every case. If `clipSide == none`: **start is inside the brush**: `startsolid = 1`; if additionally `!getout` (end also inside every plane): `allsolid = 1`, `fraction = 0`, `surfaceFlags = 0`. Else: `fraction = enter` (only reached when `enter < previous fraction` because `leave` was seeded with it), normal, `surfaceFlags = materials[side.material].surfaceFlags`, `walkable = 0` (set later), `hitType = 1` (or 4 with `hitId = glassPieceIndex` for glass brushes), `hitId = 0x7FE` otherwise.

Constants that matter for fidelity: backoff `0.125` on **entering** hits only; leaving uses exact fractions.

### 2.6 Terrain triangle vs swept capsule [C]
Triangles are **one-sided** (back-face culled: skipped when `dot(delta, N_unnormalised) >= 0`, where `N = (v0-v1) x (v0-v2)`... i.e. the face must oppose the motion).
Let `n = normalise(N)`, `P` = capsule centre at start shifted along z by `-h * sign(n.z)` toward the tri (i.e. the lower sphere centre if the face points up, the upper if down), `r`.
* `dist = dot(P - r*n - v0, n)`.
  * If `dist >= 0`: `t = (dist - 0.125) / (-dot(delta,n))`; reject if `tr.fraction <= t`. Not start-solid.
  * Else (start is behind the plane): `cd = dot(P - v0, n)`; if `r*r <= cd*cd` reject; else project the centre onto the plane, `t = 0`, mark start-solid candidate.
* Point (projected hit point) is tested against the three triangle edges with cross-product signs. Inside -> face hit: `normal = n`, `fraction = max(t,0)`, `walkable = 0` for a face hit (walkable resolved later from normal.z), `startsolid` if the projection case was used, `hitType=1`, `hitId=0x7FE`.
* If outside an edge: sphere-vs-edge test against an **infinite cylinder of radius `r + 0.125`** around the edge line (solve the quadratic, take the near root; reject if root >= tr.fraction; root < 0 -> start inside -> `fraction = 0`, `startsolid` if inside `r`), restricted to the segment by the parametric `0..|edge|^2` test; if the nearest feature is a vertex, **sphere vs vertex**: radius `r + 0.125`, quadratic with `startsolid` shortcut when `distSq < r*r`, normal = `(hit - vertex)/(r+0.125)` re-normalised by one Newton step `((3 - |n|^2)/2)`.
* Edge/vertex hits take `walkable` from `triEdgeIsWalkable` bit for the edge (`tri*3 + k`), so a ledge edge can be walkable even when its normal.z < 0.7.
* Point traces use a plain ray/triangle test (Moller-Trumbore style, one-sided, hit when `0 < t < tr.fraction`), normal = face normal, `walkable = 0`.

### 2.7 Borders (vertical edge cylinders under terrain) [C]
Used only when the move has horizontal motion (`delta.x != 0 || delta.y != 0`) and the capsule has a cylinder (`h != 0`). For each border: plane in xy `(nx, ny, d)`; reject if moving away (`dot(delta.xy, n) >= 0`); entry time to the plane at offset `r + 0.125`; z of the contact is compared with the border's sloped z-range `[zBase + zSlope*s]` (s along the border within `[start, start+length]`) extended by `h`; hits near the ends fall back to a vertical-cylinder (radius `r+0.125`) test on the end-posts. A hit sets `normal = (nx, ny, 0)`, `walkable = 0`, and may set `startsolid`.

### 2.8 Position test / start-solid / all-solid rules [C]
* **startsolid**: start inside a brush's expanded planes (`clipSide == none`), or inside a triangle's `r`-sphere/cylinder.
* **allsolid**: start AND end inside every plane of a brush (`getout == false`); sets `fraction = 0`, `surfaceFlags = 0`, keeps `contents`. Terrain "position" hit sets `fraction = 0`, `allsolid = startsolid = 1`.
* After a trace that did not start solid, `walkable = (normal.z >= 0.7)` unless a terrain edge/vertex already provided it.
* `getPenetration` mode: every leaf overlapping the box is visited; for each brush whose planes (offset by the capsule support + `0.125` handled as above) contain the centre, the code finds the **minimum penetration** over the 6 axial faces and all sides and writes a push-out vector `normal-or-axis * (depth + 0.125)` into `tr.normal`, with `allsolid = startsolid = 1`, `fraction = 0`. This vector is what the player "stuck" correction uses (3.6).
* Point contents (`CM_PointContents`-style): descend nodes by `dot(p,n) - d >= 0 -> front`, then in the leaf test `p` strictly inside the leaf bounds, then walk the leafBrushNode tree (kd split `p[axis] <= dist -> child[1]`), and for each brush OR in `brushContents` if `|p - brushMid| < brushHalf` on all axes and `dot(n,p) < d` for every side. (Strict inequalities.)
* A boolean **sight trace** exists that reuses the same descent with `fraction` only (no normal/material): it may be seeded with a previous hit brush index (+1) to test that brush first.

### 2.9 Results, rounding [C]
* Fractions are plain `float`; no quantisation. The world stage returns `min(fraction, 1.0)`.
* The caller recomputes impact points as `S + (E-S)*fraction`.
* Contents mask tests are `(mask & contents) != 0`; there is no surface-flag mask on the sweep (surface flags are only reported).

---------------------------------------------------------------------------------------------------

## 3. Player movement (Pmove) collision [C unless marked]

### 3.1 Trace wrapper
`PM_playerTrace(pm, tr, S, E, bounds, ignoreEnt, mask)` = world+entity capsule trace with the player's bounds, plus: if the hit entity is one of the "pushable" classes (entity number window 0x12..0x29 with a stored pass id equal to `ignoreEnt`) and not start-solid the hit is accepted as is; otherwise if the hit contents include `0x2004000` the entity is added to `pm.touchents` (max 32, deduped) and the trace is **re-run with those two contents bits removed from the mask** (so players slide through triggers/"touch" volumes but still register them). `PM_trace` is the same without the re-run. The `removePitchRoll` option bit is `ps.flags2 >> 25 & 1`.

### 3.2 Bounds (half sizes; all 30 wide) [C]
| state | mid z | half x,y | half z | height |
|---|---|---|---|---|
| stand | 35 | 15 | 35 | 70 |
| crouch | 25 | 15 | 25 | 50 |
| prone | 15 | 15 | 15 | 30 |
| prone-ish state 0x16 | 25 | 15 | 25 | 50 (sets prone flag) [I] |
| dead (pm_type 5) | 4 | 8 | 12 | - |
Resulting capsules: stand r=15, h=20; crouch r=15, h=10; prone r=15, h=0 (sphere). Stance changes use test traces with bounds `mask & ~0x2004000` (standing check; if it is all-solid the player stays crouched/prone).
A second probe box used by the airborne "stuck above/below" test is `mid (0,0,25) half (15,15,25)`.

### 3.3 Ground detection
`PM_GroundTrace`: start `z + 0.25`, end `z - 0.25` (stand/crouch), or `z -> z - 1.0` when in ladder/mantle-class flags; same bounds. If the first trace starts solid or isn't walkable, a second trace is made along a surface-aligned direction (helper at 0x140221BE0, [I] detail). `walkable = normal.z >= 0.7` (or triEdge flag). If the trace is `allsolid`, run the stuck handling (3.6). Lift-off ("kickoff") rule: if not in a special state and `vel.z > 0` and `dot(vel, normal) > 10` and the surface is walkable-by-slope the player is detached from the ground (`groundEntity = none`, `walking = false`). Standing on top of entities whose contents have bit `0x800000` and a specific entity class is treated as ground.
Slide angle thresholds: `0.7` walkable, `0.3` minimum for accepting a step landing, `0.9` for "almost flat" test inside stair stepping.

### 3.4 Slide move (bumps) [C]
* Up to **4 bumps**, `maxPlanes = 8` (more than 7 collected planes zeroes the velocity).
* Gravity application when requested (not in the 0x800 flag): `endVel.z = vel.z - gravity*dt`; `vel.z = (vel.z + endVel.z) * 0.5`; if on a ground plane its normal is pre-seeded as plane 0; clip velocity to it.
* Each bump: trace from origin to `origin + vel*timeLeft`; if all-solid -> try stuck correction, on failure zero z velocity and stop; move to `fraction`; `timeLeft *= (1 - fraction)`; if `fraction==1` stop.
* A plane within `dot > 0.999` of an already-collected plane is *nudged*: `vel = clip(vel,n) + n`, then the bump is skipped.
* Otherwise the planes are insertion-sorted by `dot(vel_clipped, plane)` ascending; the first plane with `dot < 0.1` triggers: clip velocity (and primal velocity) against it and against every other plane with `dot < 0.1`; if the clipped velocity still points into the first plane the code builds the crease direction `cross(planeA, planeB)` (normalised), sets `vel = dir * dot(dir, vel)`, and aborts (zero velocity) if a third plane also blocks (`dot < 0.1`).
* If `pm_time != 0` (knockback/timer) the pre-slide velocity is restored.
* `PM_ClipVelocity(v, n)`: `d = dot(v,n); k = -(d - |d|*0.001); out = v + n*k` i.e. **overbounce 1.001 when moving into the plane, 0.999 when moving away**.
* Vertical-only walk smoothing: `PM_ProjectOnPlane`: if `|n.z| < 0.001` or horizontal speed 0 keep v; else re-project the horizontal velocity onto the plane so speed along the slope is preserved (`scale = sqrt((vz^2+vxy^2)/(newz^2+vxy^2))`, applied if it is a slow-down, or the new z is negative, or velocity z > 0).

### 3.5 Step slide [C for constants and order; I for the exact accept/reject conditions]
`stepSize = 18.0` (10.0 while the prone flag bit0 is set); step-down probe depth `9.0`.
1. Run slide move from the original position (`start_o`, `start_v`).
2. If the slide moved and the player was on the ground (or on a ground plane with `normal.z <= 0.9`): trace **up** from `start_o` by `stepSize + 1`; use `fraction` to compute the real up distance `up = (stepSize+1)*frac - 1`. If `up < 1` (blocked) abort the step. A second up-trace nudged by `normal * 0.1` is tried if the first returns `fraction < 0.001`.
3. Move to `start_o + up`, restore `start_v`, slide again, then trace **down** by `up` (+9.0 more if the player was grounded at the start). Reject the step if the down trace hits a non-walkable surface with `normal.z < 0.3`, if it lands higher than `stepSize*2*normal.z` above the best of start/end heights, or if the stepped path covers *less* horizontal distance than the plain slide (`dot` test with `+0.001`).
4. After a successful step, view smoothing: `viewChange += zDelta` (decayed), speed is scaled by `(1 - |zDelta|/stepSize)*0.8 + 0.2`; a footstep event is raised for `|zDelta| > 3` units.
5. Airborne jump/mantle assist reuses the same step with a jump-height cap `jump_height + origin_at_jump`.

### 3.6 Stuck handling (`PM_CorrectAllSolid`) [C]
1. Ask for the **penetration push-out vector** (trace with `getPenetration=1`). Accept only if each |component| <= `halfSize - 0.125`. Move by `(|v| + 0.001) * normalised v`, re-test at the new spot; if it is not all-solid keep it.
2. Else jitter: try 26 offsets (the 3x3x3 neighbourhood minus the origin) scaled by `1.0`; each candidate must be non-start-solid, then a down trace of `1.25` (`1.0 + 0.25`) must also be non-start-solid; the first success moves the player there.
3. Else `groundEntity = none`, `walking = false`, clear jump state.

### 3.7 Movement constants (exact where read) [C unless noted]
| item | value |
|---|---|
| friction | `5.5 * max(speed, stopspeed=100) * dt` per frame, subtracted from speed; speed < 1.0 -> velocity 0 |
| friction multipliers | x0.3 when pm_flags bit 7; dead (pm_type 5): extra `5.0 * speed * dt` |
| accelerate (ground) | `accelspeed = accel * dt * max(wishspeed, 100)`, clamped to `addspeed = wishspeed - current` |
| accel values | stand 9.0, crouch 12.0, prone 19.0 (pm "stance" classes), x0.25 when pm_flags bit 7 |
| air accelerate | 1.0 (same formula) |
| noclip (pm_type 2) | friction `8.25 * max(speed,100) * dt`, accel 9.0 (`0x41100000`) |
| jump velocity | `sqrt(2 * gravity * jumpHeight)`, jumpHeight default **39.0** (looked up as a named dvar with 39.0 fallback); minimum 500 ms between jumps |
| jump apex cap | ceiling clamp `sqrt(gravity*2*(39 + jumpOrigin.z - z))` |
| stand view smoothing | step eye smoothing factor 0.8 |
| back/side cmd scale | backward x0.7, strafe x0.8 applied in the cmd-scale (`FUN_14021fe10`) |
| step height | 18.0 (prone 10.0) |
| walkable slope | normal.z >= 0.7 |
| falling kickoff | `vel . n > 10` |
| `g_gravity`, `g_speed`, player_sprint*, bg_fall*, stairs scale | compiled to constants in this build, not registered at all: gravity 800, speed 190 and the rest are read in docs/exe_notes/IW6_MOVEMENT.md [C] |

---------------------------------------------------------------------------------------------------

## 4. Bullet traces and damage

### 4.1 Trace
`Bullet_Trace`: a **point trace** (zero Bounds) from muzzle/start to `start + dir*range`, ignoring the shooter (and shooter's owner/vehicle), **contents mask `0x0280E831`** (bits 0, 4, 5, 11, 13, 14, 15, 23, 25), hitbox flag = 1 (entities are traced against bone hitboxes, giving `partName`/`partGroup`), static-model flag = 1 (placed model collision is included). [C mask value; bit names: bit0 solid, bit4 glass, bit5 water, bit11 sky, bit13 clipshot match the material-keyword table; bits 14/15/23/25 are body/actor-type contents, names I]

After the trace `hitSurfaceType = (surfaceFlags >> 20) & 0x1f`; if surface flag `0x100` (`nopenetrate`) is set the type becomes 0; if the type is 0 and no default was passed, 0. [C]

### 4.2 Penetration (up to 5 surfaces) [C]
Per surface:
1. `depth = table[penetrationClass][surfaceType] * clientMultiplier`, where `penetrationClass` = weapon penetrate type (0 none,1 small,2 medium,3 large; weapon attachments can override class and multiplier), table from rawfile `info/bullet_penetration_mp` (columns `small_<surf>`, `medium_<surf>`, `large_<surf>`, one row per surface name in the enum). Type 0 or class 0 -> depth 0. If the shooter has `specialty_bulletpenetration`, `depth *= perk_bulletPenetrationMultiplier` (**default 2.0**, range 0..30). `depth <= 0` ends the bullet.
2. Advance the start through the surface: from the hit point move along the direction by `0.135 / max(dot(dir,-normal),...)` (if the surface is "world" 0x7FE and `dot(dir, normal)` magnitude >= 0.125; otherwise by `0.135 * 8`), then trace onward.
3. Reverse trace from the far side back along `-dir` (starting `0.01` beyond) to find the exit; `thickness = distance(entry, exit)` clamped to >= 1.0. If exit surface is also penetrable use `depth = min(entryDepth, exitDepth)`.
4. `bullet.power -= thickness / depth`; stop when `power <= 0`.
5. Exit effects are spawned when the exit surface is flesh (type 7) or thickness > 30 (`900` units^2).
6. Repeat; the loop ends after the 5th surface.
Hit location group `armor` (20) stops the bullet; `shield` (19) is handled by the shield path.

### 4.3 Damage [C/I]
* Base damage from the weapon: linear falloff between `maxDamageRange` and `minDamageRange` (from muzzle origin to **hit point** distance): `d <= maxRange -> maxDamage`, `d >= minRange -> minDamage`, between: `lerp(maxDamage, minDamage, (d-maxRange)/(minRange-maxRange))`, truncated to int, then `* bullet.power` (the remaining penetration power, 1.0 for the first surface) and the hit-location multiplier from `info/mp_lochit_dmgtable` (rawfile, float per location, default 1.0; values not decodable here). Headshot flag when `partGroup` in {helmet(1), head(2)}.
* Hit location order (index = `partGroup`): 0 none, 1 helmet, 2 head, 3 neck, 4 torso_upper, 5 torso_lower, 6 right_arm_upper, 7 left_arm_upper, 8 right_arm_lower, 9 left_arm_lower, 10 right_hand, 11 left_hand, 12 right_leg_upper, 13 left_leg_upper, 14 right_leg_lower, 15 left_leg_lower, 16 right_foot, 17 left_foot, 18 gun, 19 shield, 20 armor, 21 soft. [C]
* Radius (explosive) damage: for entities whose AABB is within `radius` (min box distance), `scale = 1 - dist/radius`, `damage = lerp(minDamage, maxDamage, scale)`, then multiplied by the cover/visibility trace factor; the damage origin is lifted `+24` in z for the knockback vector; radius squared is capped to 65536 for non-vehicle entities. [C]
* **Ricochet**: `EV_FIRE_RICOCHET` exists as an event name only; no ricochet trace/bounce code was located (bullets end at `nopenetrate` or when power is spent). [I]
* Dynamic entity hit: `hitType`, `hitId`, `partName`, `partGroup` come from the hitbox trace; when the hitbox trace is not available the entity's AABB (`+0x128`) result is used with `partGroup 0`.

---------------------------------------------------------------------------------------------------

## 5. Contents / surface flag bit names (from the material-keyword table in `.data`) [C]

Surface flags (`tr.surfaceFlags`, `materials[].surfaceFlags`): `0x1 nodamage`, `0x2 slick`, `0x4 sky`, `0x8 ladder`, `0x10 noimpact`, `0x20 nomarks`, `0x80 nodraw`, `0x100 nopenetrate`, `0x200 stairs`, `0x400 nolightmap`, `0x1000 soft`, `0x2000 nosteps`, `0x4000 nonsolid`, `0x20000 nodlight`, `0x40000 nocastshadow`, `0x02000000 mantleOn`, `0x04000000 mantleOver`, `0x80000000 portal`, bits 20..24 = surface type.
Contents (`tr.contents`): `0x4 aiclipallowdeath`, `0x40 canshootclip`, `0x80 clipmissile`, `0x200 vehicleclip`, `0x400 itemclip`, `0x800 sky`, `0x1000 ai_nosight`, `0x2000 clipshot`, `0x10000 playerclip`, `0x20000 monsterclip`, `0x1000000 mantle (on/over)`, `0x8000000 detail`, `0x10000000 structural`, `0x80000000 nodrop`; type-contents: foliage `0x2`, glass `0x10`, water `0x20`; solid is `0x1`. Extra (compile-only) bits: origin, physicsGeom, lightPortal, outdoorBounds.
(Which bit is "player body" etc. - `0x4000`, `0x8000`, `0x800000`, `0x2000000` - is not named in the table.)

---------------------------------------------------------------------------------------------------

## 6. Surface type enum
Written to `surface_types.tsv` (index 0..30 plus three aliases). Order: default, bark, brick, carpet, cloth, concrete, dirt, flesh, foliage, glass, grass, gravel, ice, metal, metal_grate, mud, paper, plaster, rock, sand, snow, water, wood, asphalt, ceramic, plastic, rubber, fruit, paintedmetal, riotshield, slush. Aliases accepted in material names: `cushion` -> cloth, `opaqueglass` -> glass (without glass contents), `clipmissile` -> contents 0x80. The penetration table has exactly 31 columns per class (type 0..30).
Note the project IW6.md lists 26 as `cloth`; the executable's table has `rubber` at 26 and `cloth` at 4; 25 plastic / 28 paintedmetal / 29 riotshield / 30 slush agree.

---------------------------------------------------------------------------------------------------

## 7. What could NOT be determined
* Values of `info/bullet_penetration_mp` and `info/mp_lochit_dmgtable` (rawfiles inside compressed zone files; not decoded).
* Gravity, `g_speed`, sprint, crouch/prone speed scales, fall damage: now read as compiled constants, see IW6_MOVEMENT.md.
* Exact `cplane_t`, `ClipMaterial` strides beyond the accessed fields; exact XModel collision struct (PhysGeomList part layout) and its ray/tri maths (it is a SIMD 4-wide Moller-Trumbore-like test with `rcp` approximations).
* Names of contents bits 14, 15, 23, 25 in the bullet mask.
* Mantle/ladder acquisition code (large helper with 15/30/50 probes not decoded); player eye heights; sprint/slide/dive states.
* Rotated-capsule vs brushmodel-with-terrain exact algorithm (flagged inexact by the original code).
* Bullet ricochet (no code found).

---------------------------------------------------------------------------------------------------

## 8. Porting notes for the Rust re-implementation
* Implement the trace over the arrays the reader already has: nodes, leafs (with brushContents/terrainContents/leafBrushNode/bounds), leafBrushNodes, brushes + brushBounds + brushContents, collAabbTrees, partitions, verts (segmented), triIndices, triEdgeIsWalkable, borders.
* Use `r = min(half.x, half.y)`, `h = half.z - r` for **all** movers; use `0.125` consistently exactly as in 2.3/2.5/2.6.
* Do not trace a placed model's collision LOD for capsules. For **bullets** add a ray test against each placed model's collision surfaces (contents/surfaceFlags per collision surface); order: world, static models, entities.
* Store per-trace stamp for partitions; seed `leave` with the current `fraction` (that is what makes later hits only win when closer).

---------------------------------------------------------------------------------------------------

## 9. Evidence (function addresses; renamed by role)

| Role | Address | Key lines / constants |
|---|---|---|
| Trace dispatch table (handler 0/1) | 0x1409E1C40 stride 0x48 | slot0 trace fn, slot3 (+0x30) etc.; handlers: 0 = cgame, 1 = game |
| Game-side trace wrapper | 0x1403A1390 -> 0x14048BAA0 | builds ignore struct (0x14048B520), point vs capsule branch (`halfSize` all zero), `half+1.0` for entity sector broad phase |
| `SV_Trace`-like (world, then statics, then entities) | 0x14048BAA0 | calls 0x1403F31C0 first, then static models 0x1403F6000, then sector walk 0x1403F6330 / 0x1403F5330 |
| Trace init / finish | 0x1403F31C0 | memset 0x2C, fraction=1, option bytes, clamp fraction <= 1.0 |
| Work-struct builder | 0x1403ED880 | `+0x40 start+mid`, `+0x60 half`, `+0xB8 min(hx,hy)`, `+0xD4 sqrt(hx^2+hy^2+hz^2)`, `+0xC4 = halfZ - r` set in 0x1403F31C0 |
| Segment helper | 0x1403F3340 | mid, halfDelta, abs, `1.0/x if x >= 1e-06` |
| World entry (after setup) | 0x1403EE0C0 | `+0xA8` isPoint, walkable `0.7 <= normal.z` at the end |
| World BSP descent | 0x1403F2160 | `+ 0.125`, `4.7683716e-07`, leaf = `-1 - child`, node stride 0x10, leaf stride 0x28 |
| Leaf trace | 0x1403F1BE0 | `leaf+4` brushContents, `leaf+8` terrainContents, AABB tree stride 0x20 |
| Leaf bounds early out | 0x1403F1C90, 0x1403F34C0 | segment-vs-AABB with abs mask |
| LeafBrushNode traversal | 0x1403F1DE0 | node stride 0x18, `+0.125`, children at +0x10/+0x12 |
| Brush list | 0x1403F1A00 | per brush SIMD AABB reject then 0x1403F1500 |
| **Brush vs capsule** | 0x1403F1500 | `fVar - 0.125`, getout/startsolid/allsolid, glass filter, `1.4013e-45` hitType, `0x7fe` hitId, `5.6e-45` glass |
| Position test brush | 0x1403EFF90 | penetration vector `(depth + 0.125) * n` |
| Swept brush sight test | 0x1403EF110 | same maths, returns brush index+1 |
| Sight trace | 0x1403EDB30 / 0x1403EFC30 | world descent for boolean test |
| Point contents | 0x1403ED4C0 / 0x1403ED670 | strict `<` tests |
| Leaf collection for position test | 0x1403ED360, 0x1403EE720 | `-0.001` classification |
| AABB tree walk | 0x1403ECCD0 | explicit stack of 16 per level, SIMD SAT |
| Partition visit | 0x1403ECB70, 0x1403EB220 | stamp array at `work+0xE8`, counter `work+0xE0`, vert segment `*0x3000` |
| Tri vs capsule | 0x1403EB940 | one-sided `0.0 <= dot` reject, `-r*n`, `0.125` |
| Edge cylinder / vertex sphere | 0x1403EC410, 0x1403EC880 | `r + 0.125`, normal Newton step |
| Tri vs point | 0x1403EBFF0 | Moller-Trumbore-like, `fraction = t/det` |
| Border test | 0x1403EB490, 0x1403EC2A0, 0x1403ECE70 | `r + 0.125`, z-range |
| Triangle startsolid test | 0x1403EB150, 0x1403EA690, 0x1403EA800 | closest point squared distance vs `r*r` |
| Transformed (brush model) trace | 0x1403EE880 (transform), 0x1403F3050, 0x1403F0450, 0x1403F08C0, 0x1403F0F20, 0x1403F1220 | capsule against rotated model |
| Entity sector tree | 0x1403F5330/0x1403F53C0 (capsule), 0x1403F6330/0x1403F63B0 (point) | sector node stride 0x18 at 0x144512... table `0x1474A3334`; linked list `ushort` |
| Entity clip (box/hitbox) | 0x14048B080 (capsule), 0x14048A2F0 (point) | entity stride 0x2F0, contents +0x124, kind +0x109, hitbox call 0x140507440 |
| Static model ray | 0x1403F6000, 0x1403F6210, 0x1403ECF40, 0x140519270, 0x140519660 | model contents `+0x200`, collSurf stride 0x24, tri SIMD |
| Pmove main | 0x140226190 | frame clamp 1..200 ms (`local_160`), calls order: duck 0x14021EC20, ground 0x1402214A0, walk 0x140225830 / air 0x14021E760 |
| PM trace wrappers | 0x140225C20 (PM_playerTrace), 0x140225DB0 (PM_trace) | contents `0x2004000`, mask `& 0xFDFFBFFF`, touchents max 0x20 |
| Duck / bounds | 0x14021EC20 | 35/35, 25/25, 15/15, 4/12, widths 15/8 |
| Ground trace | 0x1402214A0 | `+/-0.25`, `-1.0`, `0.7`, `10.0` |
| Slide move | 0x140228150 | 4 bumps, 8 planes, `0.999`, `0.1` |
| Step slide | 0x140228990 | `18.0`, `10.0`, `9.0`, `0.9`, `0.3`, `0.1`, `0.001` |
| ClipVelocity | 0x14021FAF0 | `0.001` overbounce |
| Slope projection | 0x1402236B0 | `0.001` |
| Friction | 0x140220FD0 | `5.5`, `100.0`, `5.0`, `0.3` |
| Accelerate | 0x14021E580 | `100.0` floor |
| Walk accel select | 0x140225830 | `9.0 / 12.0 / 19.0`, `0.25` |
| Jump | 0x140212F60, 0x140213540, 0x140213140 | `sqrt(g*h*2)`, `39.0`, `499 <` ms |
| Stuck handling | 0x14021C3D0, 0x14021C650, 0x140221ED0, jitter table 0x140843F18 | `0.125`, `0.001`, `1.0 + 0.25` |
| Bullet loop | 0x140376740 | up to 5 iterations, `0.135`, `0.01`, `900.0`, surface type 7 |
| Bullet trace | 0x140378370, 0x14039F8A0 | mask `0x280E831` |
| Penetration depth | 0x140238FD0 | table `0x1417357E0 + (class*0x1f + type)*4`, perk multiplier |
| Penetration table load | 0x140239860, 0x140239920 | rawfile `info/bullet_penetration_mp`, names `small_/medium_/large_<surface>` |
| Perk dvar | 0x14021BC20 region | `perk_bulletPenetrationMultiplier` default 2.0 |
| Damage range | 0x140377420 | linear lerp |
| Impact handler | 0x140377F00 | hit-loc 19 shield, 20 armor, 1/2 head flags |
| Radius damage | 0x140395D30 | `65536.0`, `+24.0` |
| Hitloc table | 0x140395BA0 region, names at 0x1409E62B0 | `info/mp_lochit_dmgtable`, 22 entries |
| Surface type names | 0x1409EEFA8 (stride 0x18), accessor 0x1404F9350 | `flags >> 20 & 0x1f`, 30 types |
| Material-keyword/contents table | 0x1409EF2D8 - 0x18*3 (stride 0x18) | surface/contents bits |
