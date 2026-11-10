//! bo2mp (bake all): the programs of a guest map baked ahead
//! (`assets::bake_guest`), made as the world spawn makes them: CPU only, no
//! card. The plugin hands this to `assets::set_program_maker`.

/// Makes every program of the prepared map: `Ok(how many)`.
pub fn make_programs(prepared: assets::PreparedMatch) -> Result<usize, String> {
    let scene = crate::world_scene_from_draw(prepared.world, prepared.materials, &[]).map_err(|e| format!("{e:?}"))?;
    let mut compile = crate::MaterialProgramCompile::default();
    compile.arm(&scene.runtime_material_catalog, None);
    // (With no deadline the compile runs to its end in this call.)
    while !compile.until(&scene.runtime_material_catalog, None) {}
    Ok(compile.total() as usize)
}
