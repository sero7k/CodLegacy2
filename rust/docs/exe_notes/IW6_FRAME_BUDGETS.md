# Ghosts: how the exe keeps things from costing frame time (read 2026-11-05, branch iw6-fps5)

Static reading of the decompile text (`iw6mp64_ship_all.c`). [C] = read in code, [I] = inferred.

## Effect marks (decals) [C]
- `fx_marks_nearlimit` (default 5, 0..1000, 0 = unlimited). Adding a mark (add function 0x140352xxx, branches per mark kind) first calls the near-limit pruner 0x140356570 with (system, list head, origin, radius, target id, limit-1). It walks the target's mark list, counts marks of the SAME target (surface / entity key, +0x30) whose centre is closer than the NEW mark's radius, remembers the one with the smallest creation tick (+0xc), and when the count >= limit-1 frees that oldest one (0x140353e90). So at most `limit` marks stand within a radius on one surface; the oldest goes.
- The mark pool is a fixed array of 0x8c00 / 0x46 ints stride = 512 marks of 0x118 bytes; the world-surface kind (2) scans for a free entry in it. [C] (512 at once, the rest are not drawn or made.)
- Ours before: 48 slots per ring, 32 rings (1536 marks), no near limit. Now: `BO2MP_IW6_DECAL_NEARLIMIT` (5; 0 off), `BO2MP_IW6_DECAL_POOL` (512; 0 off) in `render_iw6::decal`.
- `fx_marks_smodels`, `fx_marks_ents` (true): marks on static models and on entities; the Ghosts native renderer only marks world surfaces.

## Effect spawn and draw culls [C]
- `fx_cull_elem_spawn` (true): the element spawner 0x14035c150 calls 0x14035d100 before anything is made. It needs a stored camera position (system +0x20c, flag +0x218). When the element def has a spawn range amplitude (def +0x14) it spawns only while `0 <= dist(camera, spawn pos) - base <= amplitude`; with def flag 4 an additional visibility test (0x140367200, a trace between the camera and the spawn point) vetoes it. Shared spawn path in `fx::spawn` already does the range part (`cull_elem_for_spawn_allows`); the line-of-sight veto is NOT done.
- `fx_cull_elem_draw` (true): frustum sphere cull per element at draw, already translated (`BO2MP_IW6_FX_CULL`).
- Looping effects: cull distance test 0x14035bb50, squared distance of `(range + delta) * (lowest set bit index of the cull mask)` against TWO stored camera positions (current and the previous view for split screen); both must be far to cull. Not translated (we replay by script).
- `fx_visMinTraceDist` 80: a visibility trace is only made for pairs closer than... it compares `dist >= 80` before the loop that would trace (`0x1403xxxx`), i.e. no trace work for near points. [I]
- No distance cull and no screen-size cull for drawing exist (confirmed again). `fx_drawClouds` toggles clouds. `fx_deferelem` = element (not effect) granularity of deferred processing (worker threads).
- `fx_maxNumApexFx` 2, `glass_max_shatter_fx_per_frame` 6, `glass_fx_chance` 0.25, `glass_physics_chance` 0.15: per-frame spawn caps on glass.

## LOD [C]
- `r_highLodDist`, `r_mediumLodDist`, `r_lowLodDist`, `r_lowestLodDist`, `r_lod4Dist`, `r_lod5Dist`: all default -1 (= use the model's own per-LOD distances), flags 4. The setter (0x140602870) pushes them to the DPVS; with `r_forceLod` (enum, default 6 = none) it sets the picked level's distance to 0 and every other to 0.001 (0x3a83126f). There is no screen-size LOD and no `r_lodScale` dvar in this binary; the model distances scale only through those six dvars.
- `r_umbraDistanceReference` / `r_umbraUseDpvsCullDist`: object distance-range culling is done by Umbra from the camera position (or the DPVS cull distance when the dvar is on, default off), and the portal visibility is Umbra's query (`r_umbraQueryParts` 2 = async parts). Our software occlusion plays the Umbra role.

## Shadows [C]
`sm_enable`, `sm_sunEnable`, `sm_spotEnable` (script), `sm_spotLimit` 4, `sm_maxLightsWithShadows` 4, `sm_minSpotLightScore` 0.5, `sm_spotShadowFadeTime` 1.0 s, `sm_lightScore_eyeProjectDist` 64, `sm_lightScore_spotProjectFrac` 0.125, `sm_spotDistCull` true, `sm_strictCull` true (shadow casters culled strictly to the cascade), `sm_fastSunShadow` true, `sm_qualitySpotShadow` true. All already in `spot_shadow` / `sun_shadow` (see IW6_LIGHTING.md 3.2/3.3a).

## Threads [C]
`r_smp_backend` and `r_smp_worker` (+ `r_smp_worker_thread0..7`) split the front end over worker threads, defaults chosen from the core count at start; `fx_draw_simd` true draws effects with vector code. Our equivalents: record/finish on the render thread, instancing for merging.

## What we were missing before this pass
1. The mark near limit and the 512 pool (added).
2. The spawn-time line-of-sight veto for flagged elements (open; needs a world trace at spawn).
3. Looping-effect cull distance with two camera positions (open).
Everything else listed above was already translated or does not exist in the exe (no screen-size LOD, no effect distance cull, no `r_lodScale`).
