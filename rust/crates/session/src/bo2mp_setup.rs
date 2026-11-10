//! bo2mp lane C: the match Black Ops II's front end set up. Its private
//! match lobby (CUSTOM GAMES) writes it when START MATCH is pressed, then
//! asks for the swap to `t6:<map>`; the match reads it as it starts.

use bevy::prelude::*;

/// The lobby's choices, as BO2's own menus hold them: the map's load name
/// (`ui_mapname`, e.g. `mp_nuketown_2020`), the game type (`ui_gametype`,
/// e.g. `tdm`) and SETUP BOTS (game type settings `bot_friends`,
/// `bot_enemies`: how many on his team / the other; `bot_difficulty`:
/// 0 recruit, 1 regular, 2 hardened, 3 veteran, as the menu lists them).
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct Bo2mpMatchSetup {
    pub map: String,
    pub gametype: String,
    pub bot_friends: u32,
    pub bot_enemies: u32,
    pub bot_difficulty: u32,
}

impl Bo2mpMatchSetup {
    /// The zone the match loads (`t6:mp_nuketown_2020`; a map from another
    /// game by its own key, `iw4:mp_rust`).
    pub fn zone(&self) -> String {
        match foreign_map(&self.map) {
            Some((game, stem)) => format!("{game}:{stem}"),
            None => format!("t6:{}", self.map),
        }
    }

    /// The map's zone name (`mp_rust`): what the loaded match is called.
    pub fn zone_stem(&self) -> String {
        foreign_map(&self.map).map_or_else(|| self.map.clone(), |(_, stem)| stem)
    }
}

/// bo2mp: maps from other games the front end lists, by the name its menus
/// know them by (`ui_mapname`): the game's zone prefix and the map's zone
/// name. A name two games share is listed under a suffixed name.
static FOREIGN_MAPS: std::sync::RwLock<Vec<(String, String, String)>> = std::sync::RwLock::new(Vec::new());

pub fn register_foreign_maps(maps: Vec<(String, String, String)>) {
    if let Ok(mut m) = FOREIGN_MAPS.write() {
        *m = maps;
    }
}

/// (game prefix, zone name) of a map the menus list under `ui_name`.
/// A name the list does not hold (the list not read yet, a name kept from
/// an earlier run) that ends in a game's suffix (`mp_warhawk_t7`,
/// `mp_warhawk_iw6`: the name two games share) is that game's map.
pub fn foreign_map(ui_name: &str) -> Option<(String, String)> {
    let m = FOREIGN_MAPS.read().ok()?;
    m.iter()
        .find(|(name, ..)| name.eq_ignore_ascii_case(ui_name))
        .map(|(_, game, stem)| (game.clone(), stem.clone()))
        .or_else(|| suffixed_map(ui_name))
}

/// The games whose maps the menus list with their prefix as a suffix.
const SUFFIXED_GAMES: &[&str] = &["iw3", "t4", "iw4", "iw5", "t5", "iw6", "s1", "h1", "iw7", "t7", "jup", "bf1"];

/// `mp_warhawk_t7` -> (`t7`, `mp_warhawk`).
fn suffixed_map(ui_name: &str) -> Option<(String, String)> {
    let lower = ui_name.to_ascii_lowercase();
    let (stem, game) = lower.rsplit_once('_')?;
    (stem.starts_with("mp_") && SUFFIXED_GAMES.contains(&game)).then(|| (game.to_owned(), stem.to_owned()))
}

/// bo2mp: the line the front end logs as a match starts: the menu's name,
/// the zone it loads and where that came from.
pub fn describe_start(ui_name: &str) -> String {
    let listed = FOREIGN_MAPS
        .read()
        .ok()
        .is_some_and(|m| m.iter().any(|(name, ..)| name.eq_ignore_ascii_case(ui_name)));
    let setup = Bo2mpMatchSetup { map: ui_name.to_owned(), ..Default::default() };
    let how = if listed {
        "from the map list"
    } else if suffixed_map(ui_name).is_some() {
        "not in the map list: by its game suffix"
    } else {
        "not in the map list of other games' maps: loaded as Black Ops II's own"
    };
    format!("map `{ui_name}` -> zone `{}` ({how})", setup.zone())
}

/// bo2mp online: the menus' name (`ui_mapname`) of the map a zone key
/// loads (`t6:mp_la` -> `mp_la`; `iw4:mp_rust` -> its listed name, which
/// is `mp_rust_iw4` when two games share the stem).
pub fn ui_name_for_zone(zone: &str) -> String {
    let (game, stem) = zone.split_once(':').unwrap_or(("t6", zone));
    if game == "t6" {
        return stem.to_owned();
    }
    FOREIGN_MAPS
        .read()
        .ok()
        .and_then(|m| {
            m.iter()
                .find(|(_, g, s)| g.eq_ignore_ascii_case(game) && s.eq_ignore_ascii_case(stem))
                .map(|(name, ..)| name.clone())
        })
        .unwrap_or_else(|| stem.to_owned())
}

#[cfg(test)]
mod online_tests {
    use super::*;

    #[test]
    fn zones_map_back_to_menu_names() {
        register_foreign_maps(vec![("mp_dome_t4".to_owned(), "t4".to_owned(), "mp_dome".to_owned())]);
        assert_eq!(ui_name_for_zone("t6:mp_la"), "mp_la");
        assert_eq!(ui_name_for_zone("t4:mp_dome"), "mp_dome_t4");
        assert_eq!(ui_name_for_zone("iw3:mp_bog"), "mp_bog");
        let s = Bo2mpMatchSetup { map: ui_name_for_zone("t4:mp_dome"), ..Default::default() };
        assert_eq!(s.zone(), "t4:mp_dome");
        // (One test: the list is one static the tests would race on.)
        bo3_and_ghosts_maps_find_their_zones();
    }

    fn bo3_and_ghosts_maps_find_their_zones() {
        register_foreign_maps(vec![
            ("mp_crucible".to_owned(), "t7".to_owned(), "mp_crucible".to_owned()),
            ("mp_warhawk_t7".to_owned(), "t7".to_owned(), "mp_warhawk".to_owned()),
            ("mp_warhawk_iw6".to_owned(), "iw6".to_owned(), "mp_warhawk".to_owned()),
            ("mp_favela_iw6".to_owned(), "iw6".to_owned(), "mp_favela_iw6".to_owned()),
        ]);
        let zone = |m: &str| Bo2mpMatchSetup { map: m.to_owned(), ..Default::default() }.zone();
        // Gauntlet is `mp_crucible` (its DLC code name is only its pictures').
        assert_eq!(zone("mp_crucible"), "t7:mp_crucible");
        assert_eq!(zone("mp_warhawk_t7"), "t7:mp_warhawk");
        assert_eq!(zone("mp_warhawk_iw6"), "iw6:mp_warhawk");
        // A listed name wins over its look (Ghosts' Favela keeps its suffix).
        assert_eq!(zone("mp_favela_iw6"), "iw6:mp_favela_iw6");
        // Not listed (an empty or stale list): by the suffix.
        assert_eq!(zone("mp_redwood_t7"), "t7:mp_redwood");
        assert_eq!(zone("mp_nuketown_2020"), "t6:mp_nuketown_2020");
        assert_eq!(zone("mp_la"), "t6:mp_la");
        // Battlefield 1's maps are always listed with the suffix.
        assert_eq!(zone("mp_verdun_bf1"), "bf1:mp_verdun");
        assert!(describe_start("mp_warhawk_t7").contains("zone `t7:mp_warhawk` (from the map list)"));
        assert!(describe_start("mp_bogus").contains("loaded as Black Ops II's own"));
    }
}
