# Battlefield 1 (`bf1`): what is in, what is not

Reader only. `bf1:mp_verdun` is listed and its level can be opened and reported; nothing draws or plays a BF1 map yet. No game file ships; it reads **your own PC install** (`Data` + `Patch`). Readers are the vendored `crates/fb_vfs|fb_ebx|fb_res|fb_level|fb_terrain` (Unlicense, from AaronSparks/Battlefield-1-Rust-Rewrite, credits in `NOTICE`; only `Cargo.toml` changed). `bf1_adapter` is ours: config, catalog, report. Upstream's viewer (`bf1_game`, Bevy 0.19) is not vendored; its `load.rs` is the reference for the port.

## Use it
* Folder: `bf1=<folder with Data, Patch, bf1.exe>` in `game_paths.cfg` (Options > Game Folders > Battlefield 1), or `BF1_FILES` (`BF1_GAME_DIR` also works). Not set: nothing happens, no prompt on first launch. The scan finds a folder with `bf1.exe` + `Data\layout.toc` + `Patch`. Call of Claude never packs it (~80 GB).
* Without the game: `cargo run -p bf1_adapter --bin bf1_report -- "C:\...\Battlefield 1" mp_verdun` lists the install's `mp_*` levels and prints the world report (asset files, placed meshes, terrain tiles, bounds, centre height). In game: the Change Map popup has a **BATTLEFIELD 1** section (Verdun Heights `reader only`; other levels found in the install `not yet loadable`); picking one fails with the reason and logs the same report once (`bf1-report` thread, `Zone` log). Create-a-Class has a **BF1** tab with a "weapons coming soon" tile.
* Only Verdun Heights (`Win32/xpack1/levels/mp_verdun`) is known readable (upstream's run). Other ids come from your install, untried. Not tested here: no BF1 install on the dev machine.

## Remaining, maps (estimates: one dev, with the game to hand)
1. Run `bf1_report` on a real install, fix drift (other builds, EA app): 0.5 d.
2. Scale and axes: Frostbite metres, Y up, right-handed -> our units; upstream `convert.rs` is 40 lines: 0.5 d.
3. Static meshes into our scene through `LoadedWorld` (a `bf1` arm in `match_walk.rs`, a native-style handover like `t7`/`jup`); materials and textures (BC1-7, normal maps via `fb_res`): 5-8 d.
4. Terrain mesh and layer blending (`fb_terrain` heights/colour/masks, shader-db layer map), holes: 4-6 d. Foliage scatter, water, sky/environment, grading: 6-10 d.
5. Collision: Havok static compound (`fb_level::havok`) -> our clipmap/trace: 4-6 d.
6. Gameplay hooks: spawns (entity types are in the report), map bounds, Conquest zones/flags, vehicles as static props: 4-6 d. Human scale (BO2 player in BF1 metres), nav bake: 2-3 d.
7. Menu: previews and loadscreens; replace `NOT_PLAYABLE` with a real load: 1-2 d.

## Remaining, weapons (none decoded upstream)
Needs: weapon `EBX` partitions (`SoldierWeaponData`, `WeaponFiringData`, ammo/projectile data, customization/attachment unlocks); the 1P/3P `SkinnedMeshAsset` + rig + animations (`AnimationAsset`, state graphs) through `fb_res::mesh` skinning; material/texture sets; sound (`SoundWave` EBX + audio chunks, a new codec); and a mapping of the stats (damage, RPM, recoil, spread) onto BO2's weapon model. Order: weapon list + stats table (2 d), static 3P meshes (3 d), 1P rig and animations (8-12 d), sounds (5 d), then Create-a-Class entries under the **BF1** tab (`ui/shell/coming.rs`).
