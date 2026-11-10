use bevy::prelude::*;
use frame::{ClientSet, MatchTornDown, ReturnedToMenu, SessionSwapApplied};

use crate::{
    MatchType10SoundHints, PreparedBodies, PreparedBodyClips, PreparedDestructibleDeath,
    PreparedFpvMeshes, PreparedLocalizedStrings, PreparedProjectileMeshes, PreparedWeapons,
    PreparedWorldWeapons, PreparedXAnims, PreparedXModelWalkCensus, SessionCompass,
};
use asset_anim::PlayerAnimSources;
use asset_audio::SessionMapScriptSound;
use asset_game::SessionTeamSettings;
use asset_material::AssetRefDumpCensus;
use asset_world::MapXModelSceneCatalog;

pub(crate) fn drop_match_catalogs_on_teardown(
    mut torn: MessageReader<MatchTornDown>,
    mut returned: MessageReader<ReturnedToMenu>,
    mut commands: Commands,
) {
    let torn = torn.read().count();
    let returned = returned.read().count();
    if returned > 0 {
        // (At the menu nothing reads another game's map until the next one
        // loads, which makes its own.)
        crate::release_native_maps();
    }
    if torn == 0 && returned == 0 {
        return;
    }
    commands.queue(retire_match_catalogs);
}

fn retire_match_catalogs(world: &mut World) {
    frame::retire::retire_resources(world, |batch| {
        batch
            .resource::<PreparedWeapons>()
            .resource::<MatchType10SoundHints>()
            .resource::<PreparedFpvMeshes>()
            .resource::<PreparedBodies>()
            .resource::<PreparedWorldWeapons>()
            .resource::<PreparedProjectileMeshes>()
            .resource::<PreparedXModelWalkCensus>()
            .resource::<PreparedXAnims>()
            .resource::<PlayerAnimSources>()
            .resource::<SessionCompass>()
            .resource::<SessionMapScriptSound>()
            .resource::<SessionTeamSettings>()
            .resource::<PreparedLocalizedStrings>()
            .resource::<PreparedDestructibleDeath>()
            .resource::<PreparedBodyClips>()
            .resource::<MapXModelSceneCatalog>()
            .resource::<AssetRefDumpCensus>();
    });
}

pub(crate) fn register_match_teardown(app: &mut App) {
    app.add_message::<ReturnedToMenu>().add_systems(
        Update,
        drop_match_catalogs_on_teardown
            .after(SessionSwapApplied)
            .in_set(ClientSet::Load),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_message::<MatchTornDown>();
        app.add_message::<ReturnedToMenu>();
        app.add_systems(Update, drop_match_catalogs_on_teardown);
        app
    }

    /// Back at the menu, a Black Ops III map's renderer is asked to let go
    /// (a new request with no zone); a teardown for a map change leaves the
    /// next map's request alone.
    #[test]
    fn the_menu_lets_go_of_a_native_map() {
        let zone = std::path::PathBuf::from("zone/mp_skyjacked.ff");
        let mut app = app();
        asset_world::set_t7_native_zone(Some(zone.clone()));
        let (_, before) = asset_world::t7_native_request();
        app.world_mut().write_message(MatchTornDown {
            reason: frame::TeardownReason::Replaced,
            world_generation: Default::default(),
            match_key: Default::default(),
            match_epoch: 0,
        });
        app.update();
        assert_eq!(asset_world::t7_native_request(), (Some(zone), before), "a map change keeps the request");
        app.world_mut().write_message(ReturnedToMenu { swap_id: 1, had_world: true });
        app.update();
        let (now, after) = asset_world::t7_native_request();
        assert_eq!(now, None);
        assert!(after > before, "a new request, so the renderer drops its host");
    }
}
