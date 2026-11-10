# MWR static-model lighting constants (read 3 Nov 2026, private notes)

Two near-identical per-model draw loops (`FUN_1405c79e0`, size 436, and `FUN_1405c89f0`, size 598; the second is the instanced/lod-split one) walk a list of placed models (u16 model index at +0x50, u8 lod at +0x58, count at +0x48) and, per model, with the model record (0x50 bytes, flags u16 at +0x42) and its lighting record (0x18 bytes, `GfxStaticModelLighting`):

- world matrix constants via `FUN_1405ca9c0` (3 rows of the model matrix, abs'd rotation for bounds; not lighting);
- flag 0x40 and colour index (u16 at +0x44) != 1: `FUN_1405eed50` writes the light-grid coordinates (slot 312, state + 0x2580: x = ((idx-1)&0xff)*4 + bias, y = (idx-1 >> 6 ...), z 0.5, w 1.0);
- flag 0x20: `FUN_1405eec70` writes LIGHT_PROBE_AMBIENT (slot 313, state + 0x2590) from the four half-floats of the model's ground lighting;
- flag 0x100 and lightmap index (+0x10 of the lighting record) != -1: MODEL_LIGHTMAP_PARAMS (slot 125, state + 0x19d0) = (record+8, record+0xc, record+0, record+4) = (scale.x, scale.y, offset.x, offset.y). The VS applies `params.xy * uv + params.zw`. The lightmap index itself is not used here (it picks the page elsewhere).

Our `render_h1::scene` writes exactly this (scale then offset), so the constants are not the cause of the dark stacked containers (checked against these functions).
