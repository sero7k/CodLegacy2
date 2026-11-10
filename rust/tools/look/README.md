# Look comparison against HorizonMW

- `docs/look_refs/<map>/hmw_intermission.jpg`: HorizonMW's own picture of each MWR/HMW map, HUD off, from the map's intermission camera (`devmap mp_X`).
- Our engine sits on the same camera when no class is picked (H1 maps carry their `mp_global_intermission` pose).
- `run9.cmd` / `run9.ps1` (copied to the PC runner folder): build origin/main, then for each line of `maps9.txt` (`tag MAP=h1:mp_x [ENV=val ...]`, see `maps_all.txt`) take one screenshot `iv_<tag>.png`. A tag `map__variant` is a variant run of that map.
- `python3 compare.py <shots dir> <out dir>`: side-by-side sheets (HMW left, ours right) and `scores.csv` (luma 10/50/90th percentiles, mean RGB, saturation, ours and HMW's).

## Baseline 2026-10-22 (before the MWR-shader path)

`docs/look_refs/baseline_2026-10-22_scores.csv`: 77 maps, our old shader path vs HMW.
- 20 maps within ±0.06 mid-grey, 43 too bright (>+0.10), 5 too dark.
- Our mid-grey sits near 0.50 on every map (auto-exposure to a single key); HMW ranges 0.10–0.63.
- Failures: mp_underground renders black (scene log-average 0, only 11 entities); mp_countdown's retail zone has no GfxWorld.
Re-run the same `run9` list and `compare.py` on the new path, then diff against this CSV.
