# Black Ops II maps in the shared (BO3-style) look

Off by default. `BO2MP_BO2_SHARED_LOOK=1` (also `on`/`true`) before starting the
game turns it on for every Black Ops II map (checked on none yet: no GPU in the
cloud). Not `BO2MP_BO2_LOOK`: that name is already taken by the World at War
material swap (on by default, `=0` off), so it could not mean this as well.

**Route.** `render_t7` itself cannot draw a BO2 map: its G-buffer is each
material's own BO3 technique and BO2 materials have none. So the map stays in
the fallback pass and takes the parts of `render_t7` that do not need BO3
materials. No second renderer.

| what | where | from |
|---|---|---|
| lighting: GGX sun, probe reflections, rebaked probe grid on world, props, players and gun, local and effect lights, BO3 tone curve, sun shafts over BO2's fog | `t7_shade` (`geometry_diagnostic.wgsl`) | the `BO2MP_BO3_LOOK` branch, switched on by the same flag |
| sun term | `t7_shared_sun_vis` | BO2's own: sun colour × BO2's baked visibility (lightmap page 2 alpha; the light grid's for models) × live shadow, and only where BO2's primary light is the sun |
| sun shadows | `geometry_diagnostic/sun_shadow.rs`, `render_t7::bo2_shared::cascades` | BO3's three-cascade fit (`render_t7::sun_shadow::fit`) at 2048², placed models cast as well as moving things |
| SMAA 1x, camera motion blur | `drawsurf/shared_look.rs`, `render_t7::bo2_shared::SharedPost` | BO3's own programs from its `core_patch.ff` (`post_aa`, `post_mblur`), after BO2's bloom and colour table |
| colour grade | unchanged | BO2's own colour table after the BO3 tone curve |

**Double lighting.** BO2 lightmaps leave the primary light out (BO2's lit shader
adds sun × page 2 alpha itself). Under the shared look the sun is added once, at
BO2's own visibility, and the live cascades only multiply it, so a surface can
only get darker than BO2's own sun term, never brighter. Surfaces whose primary
light is a spot or omni get no sun (they get their own light). Unit tested:
`render_t7 bo2_shared::the_sun_only_modulates`. Not on Carrier: its look turns
the sun away from the one its lightmaps were baked for.

**Switches.** `BO2MP_BO2_SHARED_AA=0` no SMAA (BO2's FXAA comes back);
`BO2MP_BO2_SHARED_MBLUR=0` no blur, or a number for its scale (default 1);
`BO2MP_BO2_SHARED_MBLUR_QUALITY=0|1|2`; `BO2MP_BO2_SHADOW_DISTANCE` (default
4000, about 900 / 3500 / 7000 units across the cascades). The BO3 branch's own
`BO2MP_BO3_EXPOSURE`, `BO2MP_RT_*`, `BO2MP_T7_HAZE`/`_SHAFTS` apply. SMAA and the
blur need BO3 installed (`game_paths.cfg` / the games root); without it the log
says `bo2 shared look: no SMAA or motion blur` and the rest stays on.

**Check (headless, numbers only).**
`cargo run --release -p assets --example bo2_shared_look_check -- <BO3 zone dir> <BO2 zone>/mp_hijacked.ff <BO2 zone>/mp_raid.ff <BO2 zone>/mp_slums.ff <BO2 zone>/mp_express.ff`
prints, per map and spawn view: NaN texels, black %, mean, world in live shadow
%, and ms for cascades, lit pass and SMAA + blur. `FAIL` on NaN, black or a
validation error. `synthetic` in place of a zone runs a ground and a block
(that is what ran in the cloud, on lavapipe: OK, no NaN, 0 % black). The check's
shading is reduced (grey surface, sun by N.L × cascades, flat ambient); it tests
the passes, not the picture.

**Log lines to look for.** `bo2 shared look: on`, `bo2 shared post: ...`,
`bo2 shared look: first frame through SMAA / motion blur`.

**Known limits.** Not seen on a GPU. The sky is BO2's own sky cube. The blur
is the camera's turn only (no depth read: steps, moving players and the gun do
not blur) and runs after the grade (BO3 blurs before its bloom); SMAA runs before
the blur. World geometry does not cast in real time (its baked shadows stay);
props cast within about 5,000 units. No SSAO, no BO3 bloom or BO3 LUT, no
temporal AA. Rendering changes need the owner's screenshots.
