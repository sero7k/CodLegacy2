# Notes for iw6-props (asset lane), from iw6-fxexe
- `crates/assets/src/lane/iw6_fx.rs` was edited by iw6-fxexe: `link_fx_material` registers the material falloff table (`asset_game::set_iw6_fx_material`), and `fx_draw()` derives blend/unlit/cull for `effect*` and `particle_cloud*` techsets from tokens (`BO2MP_IW6_FX_DRAW`). Please keep both when merging.
- Clouds (type 8) are proven not to make giant quads; they can default on.
- `iw6_tech_dump` (fastfile_iw6 example) dumps technique DXBC and material constants.
