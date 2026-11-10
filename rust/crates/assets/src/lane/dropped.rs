//! bo2mp: stand-ins for the games left out of the build (owner, 5 Nov
//! 2026): Infinite Warfare (`iw7` feature) and Modern Warfare III (2023)
//! (`jup` feature). Same names and types as the real lanes, so the callers
//! need no switches of their own: no guns, no maps, no operators. Building
//! with `--features iw7` or `--features jup` brings the real ones back.

#[cfg(not(feature = "iw7"))]
pub(crate) mod iw7 {
    use asset_anim::XAnimBuild;
    use asset_game::WeaponBuild;
    use asset_material::MaterialCatalog;
    use asset_model::{FpvMeshBuild, WorldWeaponBuild};
    use asset_transport::progress::LoadProgress;
    use std::collections::HashSet;
    use std::path::Path;

    use crate::lane::LoadedWorld;

    const LEFT_OUT: &str = "Infinite Warfare is not in this build";

    pub(crate) fn iw7_native_on() -> bool {
        false
    }

    pub(crate) fn load_iw7_world(path: &Path, progress: &LoadProgress, material_seed: MaterialCatalog) -> LoadedWorld {
        load_iw7_world_for(path, progress, material_seed, false)
    }

    pub(crate) fn load_iw7_world_for(path: &Path, _: &LoadProgress, _: MaterialCatalog, _: bool) -> LoadedWorld {
        diag::warn!(World, "bo2mp: {}: {LEFT_OUT}", path.display());
        LoadedWorld::with_gap(
            asset_world::WorldDrawPolicy::t5(),
            crate::lane_capability::PreparedCapability::PreparedWorld,
            LEFT_OUT.to_owned(),
            Some("assets::lane::dropped::iw7"),
        )
    }

    pub(crate) fn attach_t6_zm_rules(loaded: &mut LoadedWorld, map: &str) {
        loaded.report.push(format!("bo2 zombies rules gap: {map}: foreign Zombies maps are not in this build"));
    }

    pub(crate) fn is_iw7_zombies_map(_: &str) -> bool {
        false
    }

    #[allow(dead_code)]
    pub(crate) fn register_foreign_zm_map(_: &str) {}

    pub(crate) struct Iw7Prefetch;

    #[derive(Default)]
    pub(crate) struct Iw7Guests {
        pub weapons: WeaponBuild,
        pub fpv: FpvMeshBuild,
        pub world_guns: WorldWeaponBuild,
        pub xanims: XAnimBuild,
        pub material_seed: MaterialCatalog,
        pub report: Vec<String>,
        pub built: Vec<()>,
        pub aliases: HashSet<String>,
        pub fx: asset_game::FxCatalog,
        pub fx_models: asset_game::FxModelCatalog,
        pub tracers: asset_game::TracerCatalog,
    }

    pub(crate) fn prefetch_iw7_guest_guns(_: bool) -> Iw7Prefetch {
        Iw7Prefetch
    }

    pub(crate) fn build_iw7_guest_guns(_: Iw7Prefetch, material_seed: MaterialCatalog, _: &LoadProgress) -> Iw7Guests {
        Iw7Guests { material_seed, ..Default::default() }
    }

    pub(crate) fn walk_iw7_guest_guns(_: bool, material_seed: MaterialCatalog, _: &LoadProgress) -> Iw7Guests {
        Iw7Guests { material_seed, report: vec![format!("iw7 guest guns: {LEFT_OUT}")], ..Default::default() }
    }

    pub(crate) fn iw7_anim_check(_: &XAnimBuild) -> String {
        format!("iw7 animations: {LEFT_OUT}")
    }

    pub(crate) fn iw7_guest_sounds(_: &mut WeaponBuild, _: &mut Vec<String>) {}
}

#[cfg(not(feature = "jup"))]
pub(crate) mod jup {
    use asset_material::MaterialCatalog;
    use asset_model::{FpvMeshBuild, WorldWeaponBuild};
    use asset_transport::progress::LoadProgress;
    use std::path::Path;

    use crate::lane::LoadedWorld;

    const LEFT_OUT: &str = "Modern Warfare III (2023) is not in this build";

    pub(crate) fn jup_native_opted_in() -> bool {
        false
    }

    pub(crate) fn load_jup_world(path: &Path, _: &LoadProgress, _: MaterialCatalog, _: bool) -> LoadedWorld {
        diag::warn!(World, "bo2mp: {}: {LEFT_OUT}", path.display());
        LoadedWorld::with_gap(
            asset_world::WorldDrawPolicy::t5(),
            crate::lane_capability::PreparedCapability::PreparedWorld,
            LEFT_OUT.to_owned(),
            Some("assets::lane::dropped::jup"),
        )
    }

    pub(crate) struct JupPrefetch;

    #[derive(Default)]
    pub(crate) struct JupGuests {
        pub fpv: FpvMeshBuild,
        pub world_guns: WorldWeaponBuild,
        pub material_seed: MaterialCatalog,
        pub fx: asset_game::FxCatalog,
        pub fx_models: asset_game::FxModelCatalog,
        pub report: Vec<String>,
    }

    pub(crate) fn prefetch_jup_guest_guns(_: bool) -> JupPrefetch {
        JupPrefetch
    }

    pub(crate) fn build_jup_guest_guns(_: JupPrefetch, material_seed: MaterialCatalog) -> JupGuests {
        JupGuests { material_seed, ..Default::default() }
    }

    pub(crate) fn jup_check() -> Vec<String> {
        vec![format!("jup: {LEFT_OUT}")]
    }

    pub(crate) fn jup_guest_rows(_: &mut asset_game::WeaponBuild, _: &mut Vec<String>) {}

    /// The operators as the soldiers: none (Black Ops II's are drawn).
    pub(crate) mod jup_ops {
        pub fn operator_for(_: bool, _: u32) -> Option<String> {
            None
        }
        pub(crate) fn prefetch() {}
        pub(crate) fn add_operators(
            _: &mut asset_model::FpvMeshBuild,
            _: &mut asset_material::MaterialCatalog,
            _: Option<&asset_t6::XModelRef>,
            _: &mut Vec<String>,
        ) {
        }
    }
}
