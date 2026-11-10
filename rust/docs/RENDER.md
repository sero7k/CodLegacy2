# Render: what lives where

Nine crates. `render_frontend` reads the presented snapshot, `render_gpu` writes
the pixels and solely owns `wgpu`. The island decides no game logic, never
writes `AuthorityWorld`, decodes no assets.

| crate | owns |
|---|---|
| `render_frontend` | `prepare/` scene, view, cull (`scene/{world,spawn,cull,camera}.rs`, `scene/gfx_scene.rs` anim-submission bridge, `worker_cmds.rs`); `adapters/` occupancy out of anim and FX; `assemble/` frame products (`drawsurf/{retained_list,frame_products,material_runtime}.rs`, `drawsurf/tess/`) |
| `render_scene` | occupancy storage: lighting request/result, frustum planes, camera pose. Static light lookup and BSP cells publish on `MatchInstalled` and clear on teardown; frame culling does not republish them. `render_anim` / `render_fx` produce the poses and FX |
| `render_frame` | the immutable frame-product snapshot at the frontend ↔ GPU border (`SourceRevisions`, `PackedSegments`, pure packing) |
| `render_material`, `render_backend` | the material tables; the list views |
| `render` | composition, `extract.rs`, `diag/` |

## The frame path

```text
zone → asset_iw4 IR → prepare/scene/world.rs (our own types) → spawn.rs (GPU)
     camera.rs (look/WASD), frustum.rs + cull.rs + dpvs_iw4
     → assemble/drawsurf/list.rs   ONE sorted GfxDrawSurf[]
       → setup.rs  techType → technique      → state.rs  GfxPassState / blend
       → tess/{world,smodel,xmodel,fx,mark,glass,particle_cloud}.rs
       → material_runtime.rs   N-pass execution or a typed refusal
       → sm3*.rs   SM3 tokens (`d3d9_sm3`) → WGSL → gpu_contract.rs → wgpu
```

**One** drawsurf machine; only the tess emitters fork per surface type. World,
static models and the viewmodel are three emitters into one list, not three
renderers. Lighting is `lighting_iw4` (light grid) with
`prepare/scene/smodel_lighting.rs` and `model_lighting_atlas.rs`, shadows
`assemble/drawsurf/sun_shadow.rs`. A local `StaticSunAndFx` schedule overlaps
static sun partition preparation on workers with FX on the main thread. It joins
before frame products consume either result; dynamic casters merge afterward.
Sun model draws reuse the colour geometry’s index buffers.

## GPU-side ownership: `render_gpu/src/drawsurf/colour_submit/`, three owners

* `geometry.rs` + `residency.rs` — residency. Capacity, live length, allocation
  generation and uploaded revision are four quantities: a shrunk frame keeps its
  buffer and a changed `PackedSegments` segment uploads alone;
* `prepare_camera.rs` — shared installation resets generations and publishes
  shadow views; sun, spot and camera then prepare in order, on pass-local
  executors. A second `Camera3d` is the typed refusal `MultipleCameraViews`;
* `record.rs` + `indirect.rs` — recording. Direct path `draw_indexed`; with
  `CODLEGACY_MULTI_DRAW` and the indirect downlevel flags, adjacent compatible runs
  merge into one `multi_draw_indexed_indirect`, order preserved.

`extract.rs` publishes `InstalledRenderWorld` when static geometry, ports or SMC
maps change, and `PublishedRenderFrame` every frame. `d3d9_decl` / `d3d9_sm3` /
`d3d9_state` carry D3D9 semantics with no word about IW4; the only D3D9 → `wgpu`
crossing is the adapter inside `render`, while `session` stands the match up and
`assets` owns the bytes. No live `r_*` dvars: env vars (`CODLEGACY_SINGLE_CELL`,
`CODLEGACY_SUN_SHADOW_*`, …) and Perfetto (`CODLEGACY_PERF=1`, [`PERF.md`](PERF.md)).

Film vision presets come out of the map and common fastfiles; `visionSetNaked` and
`visionReset` on the console drive `FilmVisionView` between them over a transition.

## Upscaling and frame generation

Options > Advanced Video: UPSCALING (OFF / FSR 1 / FSR 3.1 / DLSS), RENDER QUALITY (67 / 58 / 50 / 33 %), SHARPNESS;
settings `upscaler`, `upscale_quality`, `upscale_sharpness`. `render_gpu/src/drawsurf/upscale.rs` retargets the
`FpvLens` camera to an image of the preset's size (order -10) and adds a `Camera2d` of ours on the window
(order -9) whose `Core2dSystems::PostProcess` system runs EASU then RCAS (`upscale.wgsl`, AMD's published FSR 1
algorithm). Off (the default) touches nothing. Menus, console and bevy_ui HUD sit on other window cameras.

`upscaler` accepts `off`, `fsr1`, `fsr3`, `dlss`. What this machine runs is decided once the adapter is known
(`temporal::refresh_support`, told to `frame::Upscaler::set_support`): FSR 3.1 needs the Vulkan backend, DLSS needs
Vulkan, an NVIDIA adapter, a build with the `dlss` cargo feature and the DLSS runtime. `Upscaler::effective()` walks a
choice down (DLSS, FSR 3.1, FSR 1) and the renderer logs "X is not available on this machine; drawing Y"; the OPTIONS
row labels such a choice ("NEEDS VULKAN: USES FSR 1", "NOT AVAILABLE: USES NEXT BEST"), so a settings file moved to
another PC still works. The same fall-back happens for a frame: `temporal::draw_temporal` runs before `draw_upscale` and
sets `TemporalOutcome::drawn`; when it could not draw (shaders still compiling, no motion target, an error) the FSR 1
passes draw that frame.

### Temporal upscaler inputs (jitter, motion vectors, depth): the foundation

Used by FSR 3.1 and DLSS (`temporal.rs`, `dlss.rs`, below). Active only while the effective upscaler is `fsr3` or `dlss` (`Upscaler::wants_temporal`),
or `BO2MP_MV_VIEW=1`; otherwise no component, texture, attachment or shader variant exists.

* **Jitter** (`upscale.rs`, `drive_jitter`): bevy's `TemporalJitter` on the `FpvLens` camera, offset from Halton(2,3),
  index `frame % phases + 1`, minus 0.5, in pixels (FSR's `GetJitterOffset`). Phase count is FSR's
  `8 * (display / render)^2` (display over render, whole: 8 at native, 32 at 50%), computed from the window and the
  preset. Bevy shifts the projection in `prepare_view_uniforms`, so `view.clip_from_world` (all passes that read
  the view uniform: the fallback pass, bloom, DoF, the sun shadow lookups) is jittered while
  `view.unjittered_clip_from_world` is not. Resource `UpscaleJitter { active, pixels, index, phases, render,
  display }` in both worlds. The jitter shows as a wobble while they still draw through FSR 1. Not jittered: the
  BO3 and MWR native passes (`t7_native`, `h1_native` build their own camera from `ExtractedView`).
* **Motion vectors** (`motion.rs`): component `ViewMotion` on the lens view, an `Rg16Float` target at the lens size,
  the second colour attachment (MRT) of the three fallback-pass passes (world / effects / viewmodel), cleared by one
  empty pass (`clear_motion`). Pipelines get the `MOTION` shader def and a second `ColorTargetState`; the entry
  points of `geometry_diagnostic.wgsl` are wrappers (`fragment*` call `shade_*`). Value = bevy's convention:
  `(current - previous)` unjittered NDC `* (0.5, -0.5)`, i.e. UV units, +x right, +y down; 0 where nothing wrote.
  Static world, props and models: previous frame's unjittered view-projection (kept per view in `prepare_motion`)
  applied to the same world position. Sky: the camera's turn only (`w = 0`). First-person gun and arms: 0 (their
  own bind group carries the flag; sway ghosts a little). Players, grenades, script models and FX: the extract keeps
  no previous transform (skinning is on the GPU from this frame's bones), so they take the camera-only
  reprojection: a moving player is wrong by his own movement. Blended draws (decals, glass, particles, effects) write
  no motion (write mask empty), leaving the motion of what is behind them. Not covered: the BO3 and MWR native
  passes (motion stays 0 under them), `Msaa` above 1 (the lens uses `Msaa::Off`).
* **Depth**: `SceneDepthTexture` on the same view (`scene_depth.rs`): `Depth24PlusStencil8`, reverse Z (1 near, 0
  far), at the lens size, `TEXTURE_BINDING`, `view()` is the depth-only view; valid after the main passes of the
  frame. It is the jittered depth. Neither FSR 3.1 (32-bit depth) nor DLSS can take it as is: a one-pass copy to
  `R32Float` is what `temporal.rs` does with it (`copy_depth`).
* **Debug**: `BO2MP_MV_VIEW=1` creates the motion target even with no upscaler and draws it over the finished frame
  (`motion_view.wgsl`: grey still, red/green x/y, a pixel of motion is 1/16 of the range).

### FSR 3.1 (`temporal.rs`, `temporal.wgsl`)

`wgpu-ffx` 0.1.0 (crates.io, MIT, AMD's FSR 3 upscaler as SPIR-V compute, `wgpu = "29"`, the version bevy 0.19 and
`render_h1` use; plain Rust, no native library or C++ build, so it is an ordinary dependency, no cargo feature). Vulkan
only by the platform's choice here (the shaders are SPIR-V). Per frame, on the `UpscaleCamera` view, `draw_temporal`:

1. copies the lens image (read as plain `Unorm`) to `Rgba16Float` (`copy_color`; the colour is gamma-encoded, not HDR:
   `HIGH_DYNAMIC_RANGE` off, `pre_exposure` 1, no exposure texture);
2. copies the lens depth (`Depth24PlusStencil8`, depth aspect) to `R32Float` (`copy_depth`), flags `DEPTH_INVERTED |
   DEPTH_INFINITE` (the lens projection is `perspective_infinite_reverse_rh`), `camera_near = f32::MAX`, `camera_far` =
   the lens near plane read back from the projection (`planes`), `camera_fov_y` from it, `view_space_to_meters_factor`
   0.0254 (inches);
3. dispatches `FsrContext::dispatch` with the `ViewMotion` target as the motion vectors, `dilated_depth` (`R32Float`),
   `dilated_motion_vectors` (`Rg16Float`) and `reconstructed_previous_depth` (a `render_w * render_h * 4` byte buffer)
   owned by us, `output` an `Rgba16Float` storage texture at the window's size, `enable_sharpening` and `sharpness`
   from the SHARPNESS slider (FSR's own RCAS inside; the slider is the library's 0..1, 1 = 0 stops of attenuation, as
   in `rcas_strength`), `frame_time_delta` the real frame time in ms (the library refuses under 1 ms, so clamped);
4. draws the output onto the window texture (`present`, sRGB-aware) in place of EASU and RCAS.

Signs, read from the sources (the first two are what to try on a PC first if the picture ghosts):

* Motion: ours is `current - previous` in UV units (+y down); FSR reads `previous = current + mv * scale / size`. So
  `motion_vector_scale` is `(-render_w, -render_h)`. `BO2MP_FSR3_MV_SCALE=x,y` replaces the `-1,-1` multipliers
  (each -1..1; the library rejects a scale over the render size or zero).
* Jitter: bevy's `TemporalJitter::jitter_projection` moves clip x by `-2 j.x / w` and y by `+2 j.y / h`
  (the test `jitter_is_negated_for_the_upscaler` checks this on a real projection); FidelityFX's sample moves them by
  `+2 j.x / w`, `-2 j.y / h`, and its `jitter_offset` is that `j`. So the offset handed over is the negated one (bevy's
  own DLSS node does the same). `BO2MP_FSR3_JITTER_SCALE=x,y`, default `-1,-1`. (An earlier note in this file said
  bevy's formula already is FSR's; it is the opposite sign.)
* Reset: `UpscaleJitter::resets` counts discontinuities found in the main world (`drive_jitter`): a new lens entity
  (a new map), the eye moving more than 150 units in one frame (respawn, teleport) or the view turning more than about
  110 degrees in one frame (`temporal::view_jumped`); the render world resets its history when the count differs from
  the one it last used. A change of window or preset size recreates the `FsrView`, which resets as well.
* Failure: a panic building the context or the textures, or an `FsrDispatchError`, sets `FSR3_BROKEN`; the menu then
  offers FSR 3.1 as "uses FSR 1" and FSR 1 draws. Known library limits (README of 0.1.0): no generated reactive
  mask; the Lanczos-LUT, half-precision and wave64 permutations are not chosen by device. The compute passes need
  `TEXTURE_FORMAT_16BIT_NORM` and read-write storage formats (`TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`); both are on
  in `bootstrap/src/plugins.rs` / bevy's defaults.

### DLSS (`dlss.rs`, cargo feature `dlss`)

Bevy 0.19's `dlss` feature (`bevy_anti_alias` over `dlss_wgpu` 4.0.0 = DLSS SDK 310.5.3, wgpu 29) is used for the Vulkan
instance and device extensions only (`DlssInitPlugin`, added by `DefaultPlugins`, needs the `DlssProjectId` resource,
which `bootstrap::plugins` inserts ahead of it). Bevy's own `Dlss` camera component wants bevy's depth and motion
prepasses, which this renderer does not run (it writes its own), so `bootstrap` disables bevy's `AntiAliasPlugin`
and `dlss.rs` drives `dlss_wgpu::super_resolution::DlssSuperResolution` itself with the same inputs as FSR 3.1: colour
(`Rgba16Float`), depth (`R32Float`), motion vectors (`Rg16Float`), `Automatic` exposure, flags `LowResolutionMotionVectors
| InvertedDepth | AutoExposure`, jitter = the negated offset and motion scale = `-render size` (exactly bevy's node),
`partial_texture_size` = our render size, the preset mapped to DLSS's mode (67 % Quality, 58 % Balanced, 50 %
Performance, 33 % Ultra Performance), output an `Rgba16Float` storage texture drawn by `present`. `READY` is set when
the instance offered it (`AdditionalVulkanFeatures`) and `DlssSdk::new` succeeded; any error later marks DLSS broken
(next best). Written against the 4.0.0 sources and bevy's node, **not compiled** (the SDK is not in this repository and
`dlss_wgpu`'s `mock` feature removes the module used): the first `cargo build --features dlss` on the owner's PC may
need a small fix.

Model: `dlss_wgpu` 4.0.0 exposes no preset or model hint (its `DlssSdk::parameters` is crate-private), so the game
cannot ask for DLSS 4's transformer model or DLSS 4.5's newer one by name; the SDK/driver pick the default. To get the
newest model: NVIDIA app, Graphics, this game (add `CodLegacy.exe` if it is not listed), DLSS Override - Model Preset:
"Latest"; or put a newer `nvngx_dlss.dll` beside the executable. If a later `dlss_wgpu` exposes
`NVSDK_NGX_Parameter_DLSS_Hint_Render_Preset_*`, set it in `dlss::record`.

#### Enabling DLSS (Windows, NVIDIA RTX)

1. Accept the licence of and get NVIDIA's DLSS SDK 310.5.3: `git clone --branch v310.5.3
   https://github.com/NVIDIA/DLSS C:\sdk\DLSS` (the SDK is not redistributable here).
2. Install the Vulkan SDK (LunarG) and LLVM/clang (`bindgen` needs `libclang`).
3. Set `DLSS_SDK=C:\sdk\DLSS` and `VULKAN_SDK=C:\VulkanSDK\<version>` (and `LIBCLANG_PATH` if clang is not on PATH).
4. `cargo build --release -p launcher --features dlss` (or `--profile perf`).
5. Copy `%DLSS_SDK%\lib\Windows_x86_64\rel\nvngx_dlss.dll` beside `CodLegacy.exe`; ship the licence text from section 9.5
   of `%DLSS_SDK%\doc\DLSS_Programming_Guide_Release.pdf` with any build given to others.
6. In the game set GRAPHICS API to VULKAN, then UPSCALING to DLSS. The log shows `DLSS Super Resolution is available`
   at start-up and `DLSS, drawing WxH for a WxH window` when it runs. Without the feature, the DLL, an RTX card or
   Vulkan the row shows "NOT AVAILABLE: USES NEXT BEST" and FSR 3.1 draws (FSR 1 off Vulkan).

### Not done

* Motion for what moves: players, grenades, script models, FX and the gun take the camera-only (or zero) vectors, so
  they ghost a little under FSR 3.1 / DLSS; a reactive mask (glass, particles, the gun) is not made. The BO3 and MWR
  native passes (`t7_native`, `h1_native`) draw without jitter or motion vectors: under them the temporal upscalers
  have nothing to work with and show a soft, ghosting picture; use FSR 1 there.
* FSR 4 (RDNA4 ML upscaler) has no Rust or wgpu binding; it is the FidelityFX SDK's DX12 / Vulkan DLL.
* Frame generation (DLSS FG, FSR 3 / 4 FG): needs motion vectors and depth, present control of the swap
  chain (FG paces and presents its own frames; wgpu owns the swap chain and exposes no hook), a native SDK
  and its own device creation. Bevy offers no interpolation or extrapolation. AMD Fluid Motion Frames
  (Radeon software, per game) works at driver level with no code.

## Real-time ambient for the Black Ops III look (`assets::lane::rt_ambient`)

A Black Ops II map drawn by the fallback pass's Black Ops III branch (`t7_shade`
in `geometry_diagnostic.wgsl`; Carrier always, other maps with
`BO2MP_BO3_LOOK=1`) has a live sun (GGX, the near sun shadow map and the sun map
baked at load) but took its ambient and bounce light from the lightmaps, baked
for the original time of day. The rebake replaces that at load, from our own
sun and sky:

* **Bake.** `bo3_look::apply` builds a `fastfile_t7::bake::Tracer` over the same
  solid casters the sun map uses (only triangles of 40 square units or more,
  `BO2MP_RT_MIN_AREA`: Carrier has 3M placed-model triangles and the tree's
  build is the cost), then `rt_ambient::bake` traces a probe grid (cell 64, at
  most 120,000 cells, grown until a timed pilot says the trace fits
  `BO2MP_RT_BUDGET` seconds, default 6) over the spawn box plus 384 units, from
  320 under the lowest spawn to 768 over the highest. Per probe, 32 rays on a
  sphere (turned per probe): a miss sees the sky (`Sky`: zenith, horizon, a
  dark ground), a hit sees `albedo * (sun * N.L * shadow + 0.35 * sky)` at the
  hit (`Tracer::nearest`, `Tracer::occluded` toward the sun). The radiance is
  folded into six cosine-weighted faces (ambient cube), the sun left out. Probes
  that are mostly buried are filled from their neighbours. `parallel_map`
  spreads it over the cores.
* **Delivery.** No new binding: the grid is appended below the sun map in the
  colour-table slot's texture (`T6Lut` taller than wide), one tile of
  dims x by dims y per (z slice, face), each texel the square root of the light
  over a scale. Light row 250 (vec4s 1000..1003; skipped, and said so, if a
  primary light holds it) carries origin, cell, dims, mix, the picture row,
  tiles per row, scale and width, and the local-light flags.
* **Shader.** `rt_ambient(p, n)` samples the three faces `n` points at,
  trilinear (the hardware filters in x, y; z by hand), weighted by n squared,
  fading to the lightmap over two cells outside the grid. `t7_shade` mixes it
  with the lightmap ambient by row 250's mix (`BO2MP_RT_AMBIENT_MIX`), for the
  world and the props alike (so props with no baked light are lit too).
* **Local lights.** `t7_shade` now adds, on a Black Ops II map, the surface's
  own spot or omni primary light (`t7_local_light`: the world's `light_index`
  with the lightmap page's visibility, a prop's `primary.x` and `.y`) and the
  effect lights (`dyn_lights`), diffuse only. No shadows for them: the spot
  shadow path in `shadowmap_spot_gpu.rs` belongs to the IW4 exact pipeline
  (its own targets and bind groups), not this one.
* **Ambient occlusion.** Not built. The fallback pass draws straight into the
  scene depth buffer (`scene_depth.rs`, read-only afterwards for soft
  particles) with no depth prepass and no separate ambient target; a screen-space
  term would need a pass over that depth after the opaque draw and a way to
  darken only the ambient light (a multiply would also dim the sun and the
  highlights), i.e. the ambient written to its own target. The probe grid gives
  the large-scale occlusion meanwhile.

* **Load time (cache and cold path).** What `bo3_look::apply` leaves in the world
  (the colour-table picture with the sun map and the grid below it, and light
  rows 250 and 252..255) is kept in `codlegacy-artifacts/cache/rt_look` by
  `lane::rt_cache`, so only the first load of a map pays for the bake; the
  next one reads a ~60 MiB lz4 file (0.17 s on Ghosts' Prison Break, the same
  picture and rows to the byte). The key (`look<map tag>-<hash>`) covers the world's
  vertices and indices, every placement and mesh used, what each material does
  to the sun, the sun and sky given, every `BO2MP_RT_*` / `BO2MP_BO3_*` /
  `BO2MP_T7_*` variable and the text of the baking code, so a change to any is a
  new entry, never a stale one; a file that does not read back whole is a miss.
  `BO2MP_RT_CACHE=0` leaves it out. The cold path: placed models' triangles are
  gathered on every core from per-mesh index lists read once (the tree's solid
  list is filtered in the same pass); `Tracer::new` reads the triangles and
  builds the tree's upper levels on every core (the same tree as before) and
  each node keeps its split axis so rays visit the near child first (same
  hits, fewer nodes); `SunDepth::render` projects on every core and draws the
  map in cache-sized strips from per-strip lists, cutting each triangle's rows
  to the span its edges leave in (the edge test then decides as before: the
  heights are those of a plain box raster, a test compares them), and
  `packed` is parallel too. Measured on Prison Break (9.7 M casters, 570 k in
  the tree) with a fixed 166-unit grid: the probe values equal the old
  bake's exactly (max and mean difference 0). `BO2MP_RT_DUMP=<file>` writes the
  grid (dims, cell, f32 cubes) to compare two bakes (it also skips the cache).

Settings: `BO2MP_RT_AMBIENT` (0 off), `_MIX`, `_SKY`, `_BOUNCE`, `_CELL`,
`_RAYS`, `_BUDGET`, `_REACH`, `_MIN_AREA`, `_CACHE`, `_DUMP`, `BO2MP_RT_LOCAL`.
Options > Advanced Video > REAL-TIME LIGHTING (`rt_lighting` in settings.cfg, on by
default; `frame::settings::RT_LIGHTING`) switches the rebake and the local lights
together, read at map load; `BO2MP_RT_AMBIENT` / `BO2MP_RT_LOCAL`, when set, win. The load report's `rt ambient:` line gives the grid,
the sky used and the seconds.

## Animated materials (water, falls, flags, foliage)

The fallback pass (`drawsurf/geometry_diagnostic.{rs,wgsl}`) animates any
material a lane marks, for every game, off one clock and one wind.

- Marking: `asset_core::T6Anim` (`None`, `Flow`, `Fall`, `Cloth`) rides in bits
  22-23 of the draw code (the sort key is clamped to 6 bits); the shader style
  carries it in bits 18-19. Unmarked materials take no animated path (no shader
  defs, no cost).
- Defs: `ANIM_UV` (UV scroll at a fixed pace, 0.05 a second for Flow, 0.75 with
  `ANIM_FALL`; axis and sign chosen from the UV derivatives: down the slope from
  1.7 degrees of it (2026-10-28; was 17, so gentle streams ran along v, up or
  down by the mesh's UV order), along v on a level surface), `CLOTH` (props only:
  vertex wave weighted by distance from the model origin, along the wind).
- Clock and wind: game time is `t6_dyn.count.y`; wind direction is `count.zw`
  (`BO2MP_WIND_DEG`, default 35 degrees). Binding 4 is visible to the vertex stage.
- Readers: `asset_core::t6_anim_kind` (names: `flag`/`flagext`/cloth techniques,
  `cod7waterflowreflect`, waterfall/cascade/rapids/falls, scroll techniques) for
  BO2 (`t6_materials`), BO1, CoD4 (`cod4`), BO3 fallback (`t7_map`);
  `iw6::iw6_world_anim` adds `uvAnimParms` for Ghosts, MWR (`h1`) and IW7.
  Effects, clouds, shadow-only and sky stay still.
- Flat water of BO1 and CoD4 (no reflection probes) carries a 32x32 rippled tile
  that Flow slides along it.
- MWR's native pass (`render_h1`) had `GAMETIME` fixed at zero; `Host::code` now
  sets it from the host's clock (`code::set_game_time`).
- Not done: the sign of `uvAnimParms` / BO2 scroll constants for level water (the draw code has no free bit), flow maps, BO2 `Layer*Scroll` constants, BO1 native wind-spring
  constants (zero), MW3 2023 (hash-named, no constants), per-material rates beyond
  Flow/Fall. IW4/IW5 run the D3D9 path with real game time already; BO3 native
  already has `gameTime`.
