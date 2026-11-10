# `docs/` — the one-page map

Short files (each ≤50 lines) on what lives where and how to poke the live
game. Keep them this short: nobody opens a long file twice.

| file | about | when to read |
|---|---|---|
| [`BUILD.md`](BUILD.md) | system packages per distro (Fedora / Debian / Arch), macOS, what the Windows cross build needs | before your first build |
| [`RUN.md`](RUN.md) | running (`make map`, `--cmds`), controls frozen until `Playing`, `force_match_start`, sync-by-default, the verb list and the traps | before your first live run |
| [`WINDOWS.md`](WINDOWS.md) | portable `iw4launcher.exe`: `.env`, shortcuts into CoD, writable `codlegacy-artifacts/` | building and running on Windows |
| [`EXPORT.md`](EXPORT.md) | the portable export: one folder with `CodLegacy.exe`, its settings and trimmed copies of the games (`tools/export`), what is kept per game, the verify, sizes | moving the game to a folder that needs no game installed |
| [`DEPLOY.md`](DEPLOY.md) | `make release` / `publish` / `deploy`: the play profile, hashed `.zst`, master by SHA, provision kept separate | shipping a release, "why is the player on an old version" |
| [`DUO.md`](DUO.md) | `make duo`: two windows, fresh lobby ID, per-client console commands | reproducing multiplayer bugs locally |
| [`MASTER.md`](MASTER.md) | your own master over ssh from the machine with the clone: `cargo xtask master install`, a self-signed certificate with no domain, what to hand players | standing up a relay for yourself or your friends |
| [`SERVER.md`](SERVER.md) | a small dedicated server: `CodLegacy pack` server packs (collision, spawns, entities), what a headless match reads, `CODLEGACY_SERVER_PACKS` | hosting maps from games the server does not have |
| [`PERF.md`](PERF.md) | native `.pftrace` — the only runtime truth; picking a UUID, the manifest, `CODLEGACY_PERF`, SQL | traces, scenario SQL, why a frame took 80 ms |
| [`BENCH.md`](BENCH.md) | `make bench`: the map-load waterfall and stage `exclusive` time, frame time as a span tree, the render/GPU/work counters, and the run package (`manifest.json`, `summary.json`); in-process, no trace needed | "where did this run spend its time" |
| [`HUD_AUDIT.md`](HUD_AUDIT.md) | every in-game text/UI element: which face draws it, what moved to the project's HUD font, what is left and why | before touching HUD text or fonts |
| [`RENDER.md`](RENDER.md) | the nine crates of the island, the frame path `IR → cull → one drawsurf list → tess → material → SM3 → wgpu`, GPU-side ownership, the `d3d9_*` border | touching the picture, techsets, lighting |
| [`ANIM.md`](ANIM.md) | three floors: `anim_iw4` (facts and curves), `xmodel_runtime` (tree and pose), who picks the clip (`sim` / `render_frontend/adapters/anim/`) | viewmodel, skeleton, bone hits |
| [`MAP-LOAD.md`](MAP-LOAD.md) | map load: the `session` → `assets` → install transaction, the `load_prepared_match` walk, the lane by `ZoneGame`, the artifact cache | a zone won't load, an asset went missing, "why didn't the match come up" |
| [`T7_MAPS.md`](T7_MAPS.md) | Black Ops III maps: what walks, what converts, the Zombies layout, what is missing | a BO3 map won't load or looks wrong |
| [`BF1.md`](BF1.md) | Battlefield 1 (`bf1:`): the vendored `fb_*` readers, `bf1_adapter`, `BF1_FILES`, `bf1_report`, what is and is not built, rendering/weapons task list | adding or testing BF1 maps, weapons |
| [`BO2_SHARED_LOOK.md`](BO2_SHARED_LOOK.md) | Black Ops II maps in the shared BO3-style look (`BO2MP_BO2_SHARED_LOOK=1`, off by default): what comes from `render_t7`, the double-lighting rule, switches, the headless check, limits | turning the look on, or a BO2 map too bright or dark under it |
| [`H1.md`](H1.md) | Modern Warfare Remastered zones and Horizon MW usermaps: container, the exact walk, H1 vs S1, `<zone>.pak` (index 96), world data, retail vs tools; the map lane (what loads, what was checked, gaps) | reading or playing an MWR map or usermap |
| [`H1_NATIVE.md`](H1_NATIVE.md) | Modern Warfare Remastered's own tone map, exposure and light set maths read from its executable (function level), engine constant names, tonemap dvar defaults, comparison with the fitted values, follow-ups | the MWR look, exposure or tone map |
| [`JUP.md`](JUP.md) | Modern Warfare III (2023) containers: `.ff` Oodle blocks and master blocks, `KAPI` `.xsub`/`.xpak`, the Oodle backend | reading MW3 files |
| [`ENTITIES.md`](ENTITIES.md) | the `TickInput → sim::step → Snapshot` funnel, the `entity_iw4` taxonomy (`EntityState` / `Centity` / `ET_*` / trajectories), what sits where in `sim` | gameplay, networking, replay |
| [`SIM-STEP.md`](SIM-STEP.md) | `sim::step`: one `TickInput` → `Snapshot` funnel for authority, prediction and replay; `StepReason`; what makes a step deterministic | touching the step, prediction or replay |
| [`GSC-RUNTIME.md`](GSC-RUNTIME.md) | GSC → executable IR → Bevy runtime; args, arrays and tables still share one `Runtime` | implementing gameplay or script execution |
| [`GSC-POSTFX.md`](GSC-POSTFX.md) | script vision, color correction, blur, DoF and bloom | authoring or debugging GSC post effects |
| [`SCOPES.md`](SCOPES.md) | aimed scopes: zoom and hidden gun, where the scope picture comes from (BO2's `overlay:<material>`, other games' images, the BO2 sniper-scope fallback), guest snipers' borrowed scopes, thermal (Dual Band) and the Millimeter Scanner | a scope, thermal or MMS shows nothing |
| [`BOTS.md`](BOTS.md) | host AI: the per-tick pipeline, what a probe that never ran may not claim, the shared query budget, resumable routes, fighting from a position | bot decisions, bot movement, "why is it standing there" |
| [`GUEST_AUDIO.md`](GUEST_AUDIO.md) | guest guns' sounds and effects: which alias and records play, context records, layers, falloff, multichannel clips, BO2 fallbacks per slot by class, the census | a guest gun sounds muffled or wrong, or fires without a flash or eject |
| [`IW7_GUNS.md`](IW7_GUNS.md) | Infinite Warfare guest guns on the IW Create-a-Class tab: the roster, dedupe, where the files are read from, the S1 capture lane, donor sounds, the opt-in check and gaps | an IW7 gun is missing, grey or silent |
| [`CAMOS.md`](CAMOS.md) | camo numbers, how a camo reaches a gun (swap / pattern, which materials), the special (animated, Ghost) camos and how to add one | a camo does not show on a gun, adding a camo |
