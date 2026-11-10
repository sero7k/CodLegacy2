# IW7 world collision (Havok) and the mp_riot walk gap: byte offsets

Moved from `docs/IW7_ENGINE_PLAN.md` (offsets are data offsets inside zones and the Havok packfile, kept here because `make publish-check` flags long hex numbers elsewhere).

## Findings with real data (2026-11-03, mp_riot and mp_geneva staged from the owner's install)

**World collision is a Havok packfile.** The clip map's `havokWorldShapeData` on mp_geneva is 9,875,376 bytes (`iw7_havok <zone.ff> out.bin` writes it):
- Standard Havok packfile, magic `57e0e057 10c0c010`, version string `hk_2014.2.5-r1`, 64-bit little endian, 3 sections (`__classnames__`, `__types__` empty, `__data__`), section headers 0x40 apart from file offset 0x40, section fields in the order data start, local, global, virtual, exports, imports, end (offsets relative to the section).
- Classes: `HavokPhysicsShapeList` at data 0x0, `hknpCompressedMeshShape` at 0x1840, `hknpCompressedMeshShapeData` at 0x12850. That is Havok's new-physics compressed mesh, not the old MOPP tree.
- The data object holds 6 size-tagged arrays (sizes 1448, 100820, 166248, 78, 285238, 40135, 50380) after an AABB (-640,-640,-64 to 640,640,800 on mp_geneva) and a 5-byte-entry array (2895 entries at 0x12950). Array 1 is 1448 sections of 0x60 bytes at 0x161e0; each starts with an array `{ptr, size, capacity|0x80000000}` (its tree nodes), then its AABB (min/max as float4), 3 + 3 sentinel words (0x7f7fffee / 0xff800000), then packed offsets/counts.
- Not decoded: the section's packed fields, the packed vertex format and the triangle primitive format. The mesh tree is Havok 2014 `hkcdStaticMeshTree`; the visual world's triangles on the same map are the oracle to check a decode against (every decoded triangle should sit on or near a drawn surface or an invisible clip surface).
- Next step: decode one section (its vertices and triangles) and compare with the drawn soup; then build `asset_world::world_iw7` collision from it. Players would then collide with invisible clip walls and not with decorative props that have no collision.

**Havok compressed mesh: array map (mp_geneva, data-section offsets; checked by exact byte counts).** The `hknpCompressedMeshShapeData` object (0x12850) holds pointers at +0x10, +0x50, +0x60, +0x70, +0x80, +0x90, +0xa0, +0xb8 (sizes beside them):

| Array | Offset | Count | Element | Guess |
|---|---|---|---|---|
| A (+0x10) | 0x12950 | 2895 | 5 bytes | per-node data |
| sections (+0x50) | 0x161e0 | 1448 | 0x60 bytes | mesh sections |
| V1 (+0x60) | 0xfe670 | 100820 | u32 | packed vertices (likely 11/11/10 bits) |
| I2 (+0x70) | 0x160dc0 | 166248 | u16 | per-section primitive / run stream |
| (+0x80) | 0x1b2090 | 78 | u32 | |
| A6 (+0x90) | 0x1b21d0 | 285238 | 8 bytes | primitives? |
| A7 (+0xa0) | 0x3df380 | 40135 | u32 | per-section list, at most 255 each |
| (+0xb8) | 0x4066a0 | 50380 | 112 bytes | runs to the end of the data |

Section record (0x60 bytes): `{ptr, size, capacity}` of its tree nodes (16 bytes per node, blobs placed after the section array), bounds as two float4 (min then max), 3 words 0x7f7fffee and 3 words 0xff800000, then word 18 (0..78, rising), word 19 = offset into I2 (rising), word 20 = `{first:24, count:8}` into V1 (**contiguous across all 1448 sections**), word 21 = `{first:24, count:8}` into A7 (nearly contiguous), word 22 (not monotone: packed, unknown), word 23 (0..4, rising).
Vertex dequantisation did not fit a plain per-axis scale to the section bounds in a quick test (x and y spread to nearly the full 11 bits, z only part of 10 bits). Primitives and their vertex source are still unknown.

**mp_riot does not walk to its end on this install** (the other zones do: `global_core_mp`, `common_core_mp`, `patch_common_core_mp`, `techsets_mp_riot`, `mp_geneva`):
- `iw7_walk mp_riot.ff`: 4934/4934 assets, position 0xafc9504 of 0xc2556b8; 29 MB of the image is not consumed. The clip map header (asset 4931, at 0xafc920c) reads as noise (name empty, 2.5 billion static models), so the Havok data and everything after is not read on riot.
- The glass world (0x10 bytes) and path data (0x90 bytes, so no nodes) before it are tiny, so the walk may have lost step in or before them; the 29 MB tail holds many Havok packfiles (first at 0xb1a2fe2), so the map's physics/collision data is there but unwalked.
- Not found yet: where the walk goes wrong (compare `iw7_walk` positions per asset against a map that works; mp_geneva's glass world and path data are 0x258 and 514 KB).


### mp_riot: where the walk loses step (2026-11-03, traced)
- The map entities walk is sound up to the scriptables: the stream positions after the entity string, triggers, client triggers, splines, Havok entity shapes (342,352 bytes, a `hk_2014.2.5-r1` packfile at its start), cmodels (84), dynamic entities (673) and transient groups all land on structured bytes, as on mp_geneva.
- The scriptable instance array (1026 x 464 bytes) is read in step on both walk passes (same positions), and the part-runtime and reserved-dynent arrays after it add up exactly.
- But the bytes the instances' event-stream buffers are read from turn to noise (about 7.7 bits per byte, no zero bytes) from about 0xaef4b72, roughly instance 900, and stay noise to the walk's end. On mp_geneva the same region is mostly zeros.
- So riot's scriptables carry more than the walk reads before that point (a scriptable kind or field geneva does not use), or a buffer's pointer kind is misread. The 29 MB tail after the walk's end is high-entropy too, with many Havok packfiles in it. Next step: print each instance's context def name and the pointer word of `eventStreamBuffer` from instance 500 on and compare with geneva's.
- The zone image itself is complete: every IWC LZ4 block inflates to its stated size.

**Cause found and fixed (same day):** the scriptable `periph_ca_destroyer` walks its animation `riot_ca_destroyer` inline (event type 6). `XAnimParts::randomDataByteCount` (offset 92) is a u32, and the walk read it as a u16: 1,164,162 bytes read as 50,050, short by exactly 17 x 65,536. The "noise" was that animation's packed key data. With the u32 read, mp_riot walks to 0xc2556b8 of 0xc2556b8; mp_geneva and the common zones are unchanged.

## Compressed mesh decode: what is confirmed (2026-11-03, mp_riot and mp_geneva)
Checked against the drawn world (`map.surfaces` triangles sampled every 4 units; random points in the mesh box are a median 125 units from a drawn surface, 1.8% within 4):

- The data object's header: `+0x20` the mesh box (two float4: riot (-4030.5, -4020, -186) to (4066.5, 2101.5, 900), game units, z up); `+0x40` three u32 (riot 202811, 19, 393849; geneva 176168, 19, 370557), most likely primitive key count, bits per key, largest key.
- The arrays match Havok 2014's static mesh tree: `+0x50` sections (0x60 each), `+0x60` primitives (4 bytes: four vertex indices), `+0x70` shared-vertex indices (u16), `+0x80` data runs (u32), `+0x90` shared vertices (u64), `+0xa0` packed vertices (u32), `+0xb8` SIMD tree nodes (112 bytes: four boxes and four child words).
- **Shared vertices:** u64, x 21 bits, y 21, z 22 (low to high), scaled into the mesh box. Their quantised values span the full range on both maps. A median 2.9 units from a drawn surface, 64% within 4 units.
- **Packed vertices:** u32, 11/11/10 bits, scaled into the section's box (section words 4 to 11). A median 2.4 units, 75% within 4. The bit order inside a section's small box is not settled by this test.
- **Section words:** 20 = primitives `{first:24, count:8}` (contiguous over all sections); 21 = packed vertices `{first:24, count:8}`; 19 = offset into the shared-vertex indices (count in word 22, bits 8 to 15); 23 = **the 64K page of the shared indices**: shared vertex = `page << 16 | index`. With the page, 90% (riot) and 88% (geneva) of a section's shared vertices fall inside its box; without it, 15%. Words 12 to 17 hold 0x7f7fffee x3 and 0xff800000 x3 on every section (unset codec or bounds). Word 18 rises 0 to the data-run count.
- **Not settled: how primitive indices address the vertices.** Packed-first and shared-first orders both put triangle centres near drawn surfaces (69% / 66% within 4 units), but the triangles come out 1 to 3 units across. Many primitives read as (a, b, b, b), and 3 to 5% of sections have indices past packed + shared. Next: compare a decoded section against the drawn triangles in its box by eye (dump both as OBJ), and try the vertex list as packed, then shared, in data-run order.

### Primitive tests on mp_riot (2026-11-03, second pass)
- **Index topology is real.** Read as 4-byte primitives, 85% of triangle edges are shared by two triangles (45% with random indices). The triangle count (quads split) is 201,503, against the header's 202,811 primitive keys.
- **Vertex order: shared first, then packed.** With that order 65% of neighbouring triangles are coplanar; shuffling the section's vertices gives 32%, and packed-first scores near random (39%).
- **Still wrong somewhere.** The decoded triangles' total area is 3.8 million square units, against 202 million for the drawn world inside the same box, and the median section box is about 5 units across. Either the section box is not the packed vertices' range, or the primitive bytes are relative to something not yet found.
- Players collide with the drawn world meanwhile (`asset_world::world_iw7`), so nothing in play waits on this.
