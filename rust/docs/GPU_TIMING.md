# Per-pass GPU timing and frame counters (all renderers)

Switch: `BO2MP_GPU_TIMING=1` (any game; `0` turns the BO3 table, which is on by default, off). Interval: `BO2MP_GPU_TIMING_EVERY=<seconds>` (default 5). Per-game switches still work (`H1_PROFILE`, `T7_NO_TIMING`, `T7_TIMING_EVERY`, `BO2MP_IW6_PROFILE`).

Format (one function, `render_h1::gpu_timer::pass_line`, tested):

    <game> gpu: pass ms <pass> <ms>, ..., total <ms> over <n> frames; draws <n>, tris <n>, fx quads <n>, lights <n>, shadow draws <n>

Passes are mean ms per frame from GPU timestamps (TIMESTAMP_QUERY and TIMESTAMP_QUERY_INSIDE_ENCODERS; without them the game says so and prints no table). Counters are per-frame means over the same frames. `n/a` means the renderer has no such stage or does not count it; it is never a made-up zero.

| game | where | passes | counters |
|---|---|---|---|
| `mwr` | `render_h1::gpu_timer`, `host.rs` | the host's marks | draws, tris (scene + local light passes), shadow draws, lights drawn; fx quads n/a |
| `t7` | `render_t7::timing` | the frame's marks | draws (profile counter minus shadow), shadow draws, fx quads (sprites after view cull), lights packed; tris n/a (G-buffer draws are GPU-culled indirect) |
| `bo2` | `render_gpu::diag::gpu_counters` | bevy render-diagnostic spans: opaque, sun, spot, colour, floatz, postfx | draws and tris (map, props, dynamic lists, IW4-port colour pass), shadow draws, lights = spot shadow slots; fx quads n/a |
| `iw6` | `render_iw6::profile` (`BO2MP_IW6_PROFILE=1` or the shared switch) | the host's marks (sun near/far, casters, spot, opaque, floatz, fx lights, ssao, fog, blended, host fx+chars, dof, mblur, bloom, iris, final) plus a table with CPU ms, draws, calls, culled, ktris per pass | scene draws and tris, shadow draws, local-lit entries, effect sprites on a quad count (estimated overdraw in the table) |
| iw7 | parked, untouched | | |

BO2 note: the spans come from bevy's render diagnostics, so `total` is the sum of every span of the batch, delivered a few frames late.

## Not measured in this pass

No MWR, BO3 or BO2 map is staged in the cloud box (only the Ghosts zones are), so the inward/outward audit of draws, triangles, light and shadow cost, transparent overdraw, foliage and effect budgets could not be run, and no optimisation was made for it (rule: measured counters only). Ask the owner to run the same view pair on his PC with `BO2MP_GPU_TIMING=1` and compare the lines; whatever grows inward then names the pass to fix.

## Glue changes (pixels unchanged)

- MWR and BO3 native draw: the scene depth view is made once per texture, not every frame (`BO2MP_NATIVE_VIEW_CACHE=0` restores); the MWR exposure switch is read once.
- Not touched: first-use pipeline hitches in render_gpu (needs a GPU run to see which ones show).

## Card memory (2026-11-06)

The `bo2` line ends with what wgpu's allocator holds: `; vram allocated <MB> in <n> allocations, reserved <MB>` (every live buffer and texture, and the memory blocks reserved for them; absent when the backend does not sub-allocate). At start `render: graphics card memory: <GB> (<adapter>) -> Low|Tight|Plenty` says what the renderers were lowered to (`native_quality::Vram`; `BO2MP_LOW_VRAM=0` keeps the preset, `BO2MP_VRAM_MB=<n>` stands in for the figure), and a match's `load ledger:` line gives `picture_cap`, `capped` and `capped_bytes_saved` for the material pictures held to the card's picture budget (`BO2MP_PICTURE_BUDGET_MB`).
