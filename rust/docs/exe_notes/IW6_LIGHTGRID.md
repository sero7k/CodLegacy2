# Ghosts (IW6 MP, PC x64) light grid: sampling a world position to SH L2

Scope: the "new" GfxLightGrid format (tableVersion != 0: tree + palette, no legacy rows/colours), which
`mp_frag` uses. Complements `spec_lighting.md` section 3.1 (which covers cell size, SH basis, variable-precision
palette entry, 56-direction conversion). This document adds the tree lookup, leaf-table encoding, palette
addressing and how the two index fields are used. Reference decoder: `lightgrid_ref.py`; sample output:
`lightgrid_samples.txt`. All offsets/bit layouts are little-endian, bit streams are LSB-first.

Status tags: CONFIRMED = read in code AND validated on mp_frag data; INFERRED = read/deduced but not (or not fully)
verifiable on data; UNKNOWN.

## 1. Pipeline overview (CONFIRMED)

```
world pos (clamped to +-131072)
  -> 8 corner cells (cx+dx, cy+dy, cz+dz), trilinear weights                      [sec 2]
  -> per corner: tree descent -> leaf node -> octant block -> lane                 [sec 3, 4, 5]
        -> colour index (u32, 0 = no data), light-env index (u16), 2 lane flag bits
  -> drop corners with colour index 0 or weight < 0.001; renormalise; merge equal colour indices   [sec 8]
  -> per distinct colour index: palette address -> palette entry -> 9 x RGB SH coefficients       [sec 6, 7]
  -> weighted sum of SH coefficients (linear)  = the SH at the position
  -> (engine) 56-direction RGB table via the SH basis of spec_lighting 3.1
```
The colour index selects the SH payload (palette). The light-env index does NOT combine with it numerically; it is a
second, independent attribute (sec 9).

## 2. Header fields used (CONFIRMED)

GfxLightGrid (new format): tableVersion (!=0 selects the tree path), paletteVersion, rangeExponent{8,12,16}BitsEncoding
(signed bytes t8,t12,t16), paletteEntryCount N, paletteEntryAddress (u32[N], byte offsets into the bitstream),
paletteBitstreamSize, missingGridColorIndex, tree: maxDepth D, nodeCount, leafCount, coordMin/Max/HalfSize (3 x i32, grid
space), defaultColorIndexBitCount (CB), defaultLightIndexBitCount (LB), node table (u32[nodeCount]), leaf table (bytes).
mp_frag: D=8, 2884 nodes, 6942 leaves, min (3207,3072,1856), max (5255,5120,2368), half (1024,1024,256), CB=16, LB=10,
N=52294, missing=52293, t8=0, t12=4, t16=23.

Cell coordinates ("grid space", absolute):
```
cx = (floor(x) + 131072) >> 5      cy = (floor(y) + 131072) >> 5      cz = (floor(z) + 131072) >> 6
fx = (x+131072)/32 - cx            fy = (y+131072)/32 - cy            fz = (z+131072)/64 - cz
```
Corner c (0..7) has offsets dx = (c>>2)&1, dy = (c>>1)&1, dz = c&1 and weight
`w_c = (dx?fx:1-fx) * (dy?fy:1-fy) * (dz?fz:1-fz)`.

## 3. Tree descent (CONFIRMED)

Node = u32: bits 31..24 = child mask M (8 bits, one per octant), bits 23..0 = base B.
Root = node 0. Octant index of a cell at a level with bit `b`:
`oct = (z_rel & b ? 4:0) | (y_rel & b ? 2:0) | (x_rel & b ? 1:0)`, where
`x_rel = (cx - minX) >> 2`, `y_rel = (cy - minY) >> 2`, `z_rel = cz - minZ` (z is NOT shifted: leaves are 4x4 cells in
x,y and 1 cell in z). Level l = 0..D uses bit `b = 1 << (D - l)` (D+1 = 9 levels for mp_frag; 2^9 = 512 = 2*half/4 in x,y
and 2*256 = 512 in z).
```
if not (minX<=cx<=maxX and minY<=cy<=maxY and minZ<=cz<=maxZ): return NO_DATA        # inclusive compare
idx = 0
for l in 0..D:
    node = nodes[idx]; M = node>>24; oct = ...
    if not (M>>oct)&1: return NO_DATA
    if l < D: idx = (node & 0xFFFFFF) + popcount(M & ((1<<oct)-1))     # children of a node are contiguous, in octant order
    else:     leafOffset = node & 0xFFFFFF; leafMask = M; leafOct = oct   # byte offset into the leaf table
```
So a depth-D node owns ONE leaf-table record ("leaf node") containing one block per set bit of its mask M; the block for
octant `oct` covers 4 (x) x 4 (y) x 1 (z) cells. Lane within the block (16 lanes), using ABSOLUTE cx, cy (note minX=3207 is
not a multiple of 4, so the lane pattern is rotated relative to the block origin):
`lane = LANE_OF[(cx & 3) + 4*(cy & 3)]`, `LANE_OF = [0,1,4,5, 2,3,6,7, 8,9,12,13, 10,11,14,15]`, i.e.
`lane = x0 | y0<<1 | x1<<2 | y1<<3` (x0 = cx&1, x1 = (cx>>1)&1, ...).
Notes: the max bound is compared inclusively but a cell with z_rel = 512 would alias z_rel = 0 (bit 9 never tested); real
data never uses it (INFERRED harmless). Evidence: BFS from the root reaches exactly 2884 nodes (= nodeCount) and the
depth-8 nodes' mask popcounts sum to exactly 6942 (= leafCount).

## 4. Leaf-table record (CONFIRMED)

A record starts at `leafTable + leafOffset`, n = popcount(leafMask) octant blocks. Bit reader: LSB-first, `read(k)` takes k
bits starting at the current bit position; `align()` rounds the position up to a byte boundary.

Node header (read from record start; ends byte-aligned):
```
f        = read(8)                                    # node flags
size[i]  = read(6)   for i in 0..n-1                  # byte size of octant block i (0 = block absent => all defaults)
# --- colour-index parameters
if f&1: cBase = read(CB); cW = read(bitlen(max(CB-1,1))) + 1     # bitlen(15)=4 for CB=16
else:   cBase = 0;        cW = 16
cVal  = (f&2) ? read(cW) : 0
if f&4: cN = read(4)+1; cList[0] = 0; cList[1..cN-1] = read(cW) each      # (cN-1 values)
# --- light-env-index parameters (same shape, LB = 10, default width 1)
if f&8: lBase = read(LB); lW = read(bitlen(max(LB-1,1))) + 1   else lBase = 0; lW = 1
lVal  = (f&0x10) ? read(lW) : 0
if f&0x20: lN = read(4)+1; lList[0] = 0; lList[1..lN-1] = read(lW) each
align()
```
Resolve ("absolute" form): `abs(v) = (v == 0) ? 0 : base - 1 + v`. If a list exists: `list_abs[i] = abs(list[i])`,
`default = list_abs[val]`, `listBits = bitlen(max(N-1,1))`; otherwise `default = abs(val)`, no list.
Value 0 always means "no entry". The blocks follow the header back to back: block i begins at
`recordStart + headerBytes + sum(size[0..i-1])`; block for the queried octant has rank k = popcount(leafMask & ((1<<oct)-1)).

Octant block (size s bytes; if s == 0: g = 0 and nothing is read; all lanes get the node defaults). Starts byte-aligned:
```
g = read(8)                                                  # block flags (only if s != 0)
# colour
if !(g&1): if cList absent: start = (cBase != 0) ? 1 : 0, w = cW          # relative-coding start
           else:             start = 1, w = listBits, listMode candidate
else:      start = read(cW); w = read(bitlen(max(cW-1,1))) + 1
def_ = g&2 ? read(w)
            : (list candidate ? cVal : (cDefaultAbs == 0 ? 0 : cDefaultAbs - startAbs + 1))      # startAbs = start==0 ? 0 : start-1+cBase
runs_c = (g&4) ? read_runs(w) : []
# light-env: identical, with (lBase, lW, lVal, lList, lDefaultAbs, g bits 8,0x10,0x20, runs width)
if g&0x40: flagMaskLo = read_mask_b()      if g&0x80: flagMaskHi = read_mask_b()      # per-lane flag bits 0 / 1
align()    # consumed bytes must equal size[i]
```
Lane values: start with `def_` in all 16 lanes, apply runs in order (below). Final value per lane:
`if listMode (node list present and listBits <= w): value = cList_abs[v]  else: v == 0 ? 0 : v - 1 + startAbs`,
where `startAbs = (start == 0) ? 0 : start - 1 + cBase`. Same for light-env with its own parameters.

Runs (`read_runs(w)`, up to 8): repeat { t = read(2);
 t=0: 16 values read(w), lanes 0..15 in order, stop.
 t=1: cnt = read(1)+2; cnt lane numbers read(4) each; one value read(w) applied to those lanes; more = read(1).
 t=2: mask = read_mask_a(); one value per set bit (ascending lane) read(w), stop.
 t=3: mask = read_mask_a(); one value read(w) applied to all masked lanes; more = read(1). } until stop/more==0.

Lane-mask codecs (pattern table PAT[16], each 4 lanes):
`PAT = [0,1,4,5] [2,3,6,7] [8,9,12,13] [10,11,14,15] | [0,2,8,10] [1,3,9,11] [4,6,12,14] [5,7,13,15] | [0,1,2,3] [1,4,3,6] [4,5,6,7] [2,3,8,9] [6,7,12,13] [8,9,10,11] [9,12,11,14] [12,13,14,15]`
(rows, columns, and 2x2 windows of the 4x4 block under the lane mapping above).
`read_mask_a`: b = read(1); b==0 -> read(16); else k = read(4); inv = read(1); bits = read(4); mask = OR of PAT[k][i] for set bit i;
return inv ? ~mask & 0xFFFF : mask.
`read_mask_b` (flag masks): s = read(2); s==0 -> read(16); s==1 -> 0xFFFF; else k = read(4); bits = read(4); mask as above;
s==3 -> ~mask & 0xFFFF.
Lane flags: bit0 = (flagMaskLo >> lane)&1, bit1 = (flagMaskHi >> lane)&1.

Validation (mp_frag): decoding every block of all 1741 leaf records consumes exactly `size[i]` bytes for every block, record
offsets are contiguous with no gaps, and the last record ends at byte 97517 = leafTableSize. 111072 cells (6942 x 16)
decode; 58015 have colour != 0; every non-zero colour index is in 2..52292 (< 52294). Max block size 33 bytes (6-bit
size field OK); all four run types occur (counts t0 3395, t1 1359, t2 4672, t3 955); node flags seen: 9, 1, 25, 41, 57, 11, 3, 5.

## 5. Lane mapping evidence (CONFIRMED)
Of all 24 assignments of the four coordinate bits (x0,x1,y0,y1) to lane bits, the engine's mapping gives the lowest SH
distance between adjacent cells, with isotropic x/y behaviour (median neighbour SH distance 0.52-0.56 for every
(axis, cx&3 / cy&3) phase vs 2.70 for random pairs); the first version of the enumerator that used block-relative instead of
absolute x&3 produced a spurious phase anomaly (0.54, 1.03, 1.37, 0.52), which disappeared with absolute coordinates.

## 6. Palette addressing (CONFIRMED)
`addr = paletteEntryAddress[index]` (u32, byte offset into the palette bitstream). Entry bytes at `bitstream + addr`.
Index must be < N; the engine substitutes `missingGridColorIndex` when the chosen index >= N or when no corner has data
(sec 8). Evidence: addresses strictly increasing; entry sizes computed from the headers equal consecutive address deltas for
52293 of 52294 entries; the last entry ends exactly at paletteBitstreamSize = 1961748 (entry 1, never referenced by any leaf,
is 34 bytes of SH followed by 22 more bytes: UNKNOWN content; entry 0 is also never referenced; probably reserved).

## 7. Palette entry -> SH (CONFIRMED structure; see caveats)
```
hdr = u16 @0:   eR = ((hdr>>10)&31) - 8,  eG = ((hdr>>5)&31) - 8,  eB = (hdr&31) - 8        # per-channel exponent
pos = 2
for ch in R,G,B:  s = 2^e_ch
   e <= t8        -> 8-bit : 9 bytes at pos (no alignment):   c0 = s/256 * u8[0];  ck = u8[k]*(2s/256) - s  (k=1..8)
   e <= t12       -> 12-bit: pos += pos&1; 14 bytes:          c0 = s/65536 * u16[0]; ck = v12[k]*(2s/4096) - s
                       v12[k] = (byte[1+k] << 4) | lo[k]  for k=1..8   (high bytes b2..b9)
                       lo[1]=b9>>4, lo[2]=b10&15, lo[3]=b10>>4, lo[4]=b11&15, lo[5]=b11>>4, lo[6]=b12&15, lo[7]=b12>>4, lo[8]=b13&15
   else           -> 16-bit: pos += pos&1; 18 bytes:          c0 = s/65536 * u16[0]; ck = u16[k]*(2s/65536) - s
   pos += size
entry size = pos rounded up to even.
```
Output index [k][channel], k = 0..8 in the order of spec_lighting 3.1 (L0; then x,y,z; then zx, zy, yx, 3z^2-1, x^2-y^2).
mp_frag exponent histogram (channels over all 52294 entries): -7:841 -6:2431 -5:2286 -4:3309 -3:7034 -2:12069 -1:15324 0:29308
1:58533 2:23454 3:2293; so 8-bit channels (e<=0) 72602, 12-bit (1..3) 84280, 16-bit 0 (16-bit layout therefore verified
by code reading only: INFERRED on data).
12-bit caveat: the engine reads k=1's low nibble from the high nibble of byte 9, which is also the top nibble of k=8's high
byte; byte 13's high nibble is 0 in all 84277 blobs checked, so the encoder's exact low-nibble order is UNKNOWN; any
alternative order changes a coefficient by at most 15/4096*2s (0.73% of its scale). The decoder follows the engine.
Value ranges (used entries): L0 luma 0.006..6.5 (median 1.06, mean 1.19), per-coefficient max |c| = [6.55, 3.83, 2.93, 4.61,
2.52, 2.13, 2.88, 0.79, 1.39] for k = 0..8; no non-finite values.

## 8. Blending, fallbacks (CONFIRMED code; blending numerically unvalidated against the game)
```
for c in 0..7: (ci, li, flags) = cell_lookup(corner c); if ci == 0 or w_c < 0.001 (w >= 0.001 kept): drop
W = sum(kept w);  if no kept corner: use the fallback entry (below)
merge equal ci (add weights); SH = sum_ci (w_ci / W) * palette_SH[ci]          # a single distinct ci is used directly
```
Optional engine step (needs collision, not reproduced): corners whose lane flags request it (INFERRED: bit0 consulted when
fz < 0.5, bit1 when fz >= 0.5, for the lower/upper cell of a column) are tested by a short trace from the cell corner
toward the query point and dropped if blocked; the weights are then renormalised as above.
Fallback (no data): `missingGridColorIndex` (52293) palette entry is used (a debug colour: DC grey 0.235, L1 = (0.234, -0.234, 0));
no leaf of mp_frag references it. Out-of-bounds / no-data queries in the sample file confirm all 8 corners return index 0.
SH -> radiance / 56 table: unchanged from spec_lighting 3.1. The engine clamps negative lobes to 0 (`max(v,0)`).

## 9. Light-env index (CONFIRMED range; usage INFERRED)
Per cell u16 (<= 10 bits: LB = 10), 0 = none. Used values on mp_frag: 174 distinct, max 786 < ComWorld.primaryLightEnvCount
795. The engine takes it from the corner with the largest weight (ties/priority rules INFERRED) and uses it to index a table
of 10-byte records (up to 4 primary-light indices + count): the set of primary lights (sun etc.) affecting that cell. It does
not alter the SH, i.e. the grid SH is baked without direct primary lights (INFERRED: high-altitude cells show strong
downward L1 from sunlit ground, and the L1 direction of bright cells is anti-aligned with the sun, so the direct sun is not
in the SH).

## 10. Validation evidence (mp_frag, numbers)
* Tree: 2884/2884 nodes, 6942 leaf octants, 1741 leaf records, leaf table consumed exactly (97517 B), 0 gaps.
* Indices: colour 2..52292 (52291 distinct non-zero + 0), all < 52294; 47.8% of lanes (53057/111072) are 0 (no data); light-env <= 786.
* Palette: monotonic addresses; bitstream consumed exactly; 1 irregular entry (index 1, unreferenced).
* Smoothness: median relative |dL0| between adjacent populated cells 0.098 (x), 0.093 (y), 0.256 (z) vs 1.127 for random pairs;
  median SH distance 0.52-0.56 (x,y) vs 2.70 random.
* Spawn points (205): L0 luma min 0.118, p10 0.675, median 1.789, p90 2.275, max 3.129; dark indoor-type `mp_dom_spawn`
  points 0.12-0.28 (up-radiance 0.09-0.33), open team-start spawns 1.8-3.0; smooth sweep along +x at y=-1520, z=192:
  L0 2.26 -> 1.48 over 256 units, no jumps. Upward-facing surface points (665): radiance toward +z 0.83 vs toward -z 0.50.
* No-data queries (outside bounds, high above the map) return 0 for all corners.

## 11. Unknowns
UNKNOWN: contents of palette entries 0/1 and entry 1's 22 trailing bytes; encoder-side 12-bit low-nibble order; exact
meaning of the two lane flag bits (INFERRED trace-request bits) and the trace itself; light-env tie-break; 16-bit entries
(none in mp_frag); legacy-format maps (tableVersion 0) not covered; z_rel==512 aliasing never exercised; whether the sun
is added at runtime on top of the grid SH (INFERRED yes, via light env).

## Evidence (addresses; image base 0x140000000)
* Corner weights/lookup loop, trilinear weights, clamp +-131072, 0.001 threshold, 8 corner offsets, visibility flags: 0x140638a40.
* Tree descent + 16-entry lane table + hashed lookup cache: 0x1405fb6f0 (called per corner with `tree = world+0x5d0`).
* Leaf record decode: header 0x1405fc880, block 0x1405fc210, run list 0x1405fc4f0, lane masks 0x1405faf30 and 0x1405fb160,
  absolute-list resolve 0x1405fb260, block lane resolve 0x1405fb620 / 0x1405fab80, lane scatter/fill 0x1405fc040 / 0x1405fc140,
  orchestration 0x1405fac40; pattern table init 0x1405fbb80 (table at 0x1480ffaa0, stride 0x1c).
* Palette entry decode 0x1405fb010 (8-bit 0x1405f9a00, 12-bit 0x1405f9760, 16-bit 0x1405f98e0); palette address use in
  0x140636510 (blend of SH by normalised weights); corner collection/weights 0x1406356d0; fallback to missing index 0x1406367d0.
* SH -> 56 table 0x1405fcb40; direction table init 0x1405fa200.
* Zone positions (inflated image of mp_frag.ff): palette_entries 171912254, palette_bitstream 172121430, tree_nodes 174083178,
  tree_leaf_table 174094714; spawn/sun/vertex dump: `spawn_dump` example in the scratch workspace; image dump: `img_dump`.

## Calibration against the props' stored ground light (2026-11-03, mp_frag)

`StaticModel::ground_lighting` is stored on 1,707 of mp_frag's 8,779 placed models; 7,072 have none and borrow
their neighbours' light (`iw6_map::neighbour_light`). The decoded grid was tried as a replacement for that
borrowing. Each model that carries its own value was held out and predicted two ways, as the log of the
ratio (RMS over 1,567 models with grid data):

| predictor | log-RMSE |
|---|---|
| mean of the four nearest other models' own values (the current rule) | 0.49 |
| grid, light facing up at the origin + 16, one median scale | 0.94 |
| best grid variant tried (DC term, sampled 64 units above the origin) | 0.75 |

Variants: sample height -64..128, and the up reading, the DC term, the maximum and the mean over the 56
directions. None beats the neighbour mean, and the ratio of stored value to grid reading spans 0.43..3.8
(p10..p90). So `ground_lighting` is not the grid's reading at the model's origin in any scale tried, and the
grid was **not** wired into the props' light. Open: what position and measure the stored value is taken at
(it may include the sun, or be sampled on the ground below by a trace); the models that carry one may also
be a biased set (trees and outdoor props), so the test says little about the 7,072 that do not.
