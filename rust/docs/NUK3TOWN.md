# Nuk3town (`t7:mp_nuketown_x`) — 1:1 with Black Ops III

Reference shots from the retail game (the owner's PC, 20 Oct 2026, 1080p) are in
`docs/screenshots/nuk3town_ref/`:

| Shot | Where | What it shows |
| --- | --- | --- |
| 01_garden_robot_house | garden spawn, facing the robot house | hanging garden domes, wall planter, mannequin, cherry tree, sharp sun shadow on the grass, light beams in the sky |
| 02_orange_house_sign | orange house, facing the NUKETOWN sign bridge | glass panels with a grid, billboard screen (animated), data streams of gold voxels, warm lens haze at the screen's lower left |
| 03_bus_data_streams | bus stop, facing the bus | data stream columns (gold cubes falling in a light beam), white sparkle fountains in the distance, glowing blue lines on the road, spire with a blue hologram |
| 04_bus_side_spire | beside the bus, facing the spire | bus with an orange wireframe "materialise" overlay, blocky voxel shadows on the road, grass checker tiling, pink cherry trees, cypresses |

## What has to match

Each row is checked against the shots with the same camera (position, angles,
65 fov at 16:9 as BO3's default).

1. **Sun and shadows** — real-time sun shadows: sharp edges on grass and road, the falling voxels' blocky shadows (shot 04). Ours: a baked sun map, static only.
2. **Data streams** — the map's effects: light beams with gold voxel cubes falling, white sparkle fountains, the bus's materialise sparks. Ours: no BO3 map effects read yet (createfx / exploders in the map's script, FxEffectDef assets).
3. **Holographic materials** — the bus's orange wireframe overlay, blue road circuit lines, the spire's hologram, the hex grid across the sky. Ours: drawn with the plain lit branch, if at all.
4. **Screens** — the NUKETOWN billboard and the bus stop's chevron screen scroll/animate. Ours: static.
5. **Glass** — reflective glass with its grid (shot 02 windows, panels). Ours: static, check reflections.
6. **Foliage** — cherry blossom and leaf cards with alpha, cypresses, planters, hanging garden domes; wind sway. Check alpha test and two-sided lighting.
7. **Sky** — the clouds, horizon haze and sky brightness; the hex-grid dome overlay near the top of the view.
8. **Post** — BO3's bloom, warm lens haze at the screen edge, colour table (on), exposure.
9. **Props and entities** — mannequins in the houses, the bus, the sign, planters: all placed and posed.

## Status

- **Effects (row 2): in, not seen yet.** The map's 622 `"classname" "fx"` entities (63 effects with
  the ones they run, all `dlc0/nuketown/fx_de_rez_*` but the glass and car ones) are read from the zone
  and the common zones, their 17 sprite materials linked as `t7fx:<name>` and drawn as Black Ops II's
  effect techniques, and placed with their authored delay (`assets::lane::t7_fx`,
  `asset_game::fx_catalog_t7`, field notes there). `BO2MP_T7_FX=0` leaves them out. Check in game: the
  voxel flipbook (`fxt_de_rez_anim`, 8 x 4 played over life), the beams' height (1267-unit sprites,
  non-uniform size), their brightness (word 6 of a visual sample read as emissive stops), fade ranges.
- Everything else: not compared yet; the first step is our build's shots from the same four spots.
