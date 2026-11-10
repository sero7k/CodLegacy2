use bevy::prelude::*;

use crate::match_load::register_match_load_systems;
use crate::teardown::register_match_teardown;

pub struct AssetPlugin;

impl Plugin for AssetPlugin {
    fn build(&self, app: &mut App) {
        register_match_load_systems(app);
        register_match_teardown(app);
        app.add_systems(Last, crate::map_load_process::trim_main_heap_after_load);
        app.add_systems(Update, crate::map_load_process::mirror_bake_stage);
    }
}
