//! bo2mp: maps from other Call of Duty games in Black Ops II's map list.
//!
//! The installed games under `CODLEGACY_GAMES` are scanned once for their
//! multiplayer maps (Modern Warfare 2, Modern Warfare 3, Black Ops; World at
//! War and Call of Duty 4 once their zones load). Each game gets its own
//! categories in the Change Map popup, like BO2's DLC packs: its base maps,
//! then its DLC maps, a page of at most `PAGE` maps each so no list runs off
//! the screen. Their rows go into BO2's `mp/mapstable.csv` (the faction
//! pairing borrowed from Aftermath) with their names in the text tables,
//! and the match setup learns which game's zone each one loads.

use std::sync::OnceLock;

/// bo2mp: Modern Warfare II's (2022) table in `GAMES`: no zones are found
/// under this prefix; its maps come from the Modern Warfare III install.
const MWII_PREFIX: &str = "iw9";
const MWII_TITLE: &str = "Modern Warfare II (2022)";

/// The most maps a category lists before the next page starts.
const PAGE: usize = 14;

/// The first mapPackTypeIndex the extra categories use (BO2's own run to
/// its DLC5 and development indices).
const FIRST_PACK: u32 = 20;

/// The BO2 map whose mapstable row a foreign map's row copies.
const ROW_FROM: &str = "mp_la";

/// One game's maps: its zone prefix, its category key and title, its base
/// maps (in the order the game lists them) and its DLC maps, by zone name.
struct Game {
    prefix: &'static str,
    key: &'static str,
    title: &'static str,
    base: &'static [(&'static str, &'static str)],
    dlc: &'static [(&'static str, &'static str)],
}

/// bo2mp: Infinite Warfare's free re-releases of its DLC maps and the
/// originals they repeat: a variant is listed only when its original is not
/// installed (the zones are the same maps, a few vertices apart).
const IW7_VARIANTS: &[(&str, &str)] = &[
    ("mp_renaissance2", "mp_geneva"),
    ("mp_carnage2", "mp_rally"),
    ("mp_permafrost2", "mp_overflow"),
    ("mp_turista2", "mp_marsoasis"),
];

/// bo2mp: IW4x's own map pack in Modern Warfare 2 (`zone/iw4x/.../dlc`):
/// Call of Duty 4's and Black Ops' maps rebuilt for MW2, and its variants,
/// titled as IW4x lists them. They are MW2's maps here (MW2's section); a
/// Call of Duty 4 map with a version in this list is listed from Call of
/// Duty 4 itself as well, the MW2 one marked "(MW2)" (`COD4_IN_MW2`).
const IW4X: &[(&str, &str)] = &[
    ("mp_nuked", "Nuketown"),
    ("mp_cross_fire", "Crossfire"),
    ("mp_cargoship", "Wet Work"),
    ("mp_bloc_sh", "Forgotten City"),
    ("mp_killhouse", "Killhouse"),
    ("mp_bog_sh", "Bog"),
    ("mp_cargoship_sh", "Freighter"),
    ("mp_shipment", "Shipment"),
    ("mp_shipment_long", "Shipment: Long"),
    ("mp_rust_long", "Rust: Long"),
    ("mp_firingrange", "Firing Range"),
    ("mp_bloc", "Bloc"),
    ("mp_crash_tropical", "Crash: Tropical"),
    ("mp_estate_tropical", "Estate: Tropical"),
    ("mp_fav_tropical", "Favela: Tropical"),
    ("mp_storm_spring", "Chemical Plant"),
    ("mp_convoy", "Ambush"),
    ("mp_backlot", "Backlot"),
    ("mp_countdown", "Countdown"),
    ("mp_citystreets", "District"),
    ("mp_farm", "Downpour"),
    ("mp_pipeline", "Pipeline"),
    ("mp_showdown", "Showdown"),
    ("mp_crash_snow", "Winter Crash"),
    ("mp_broadcast", "Broadcast"),
    ("mp_carentan", "Chinatown"),
];

/// bo2mp: MW2's maps that are Call of Duty 4's (IW4x's rebuilds, its
/// variants of them, and MW2's own Stimulus remakes): listed in Call of
/// Duty 4's section as "<title> (MW2)", beside CoD4's own.
const COD4_MAPS_IN_MW2: &[&str] = &[
    "mp_convoy", "mp_backlot", "mp_bloc", "mp_bloc_sh", "mp_bog_sh", "mp_cargoship", "mp_cargoship_sh",
    "mp_carentan", "mp_citystreets", "mp_countdown", "mp_crash_snow", "mp_cross_fire", "mp_farm",
    "mp_killhouse", "mp_pipeline", "mp_shipment", "mp_shipment_long", "mp_showdown", "mp_broadcast",
    "mp_crash", "mp_overgrown", "mp_strike", "mp_vacant", "mp_creek",
];

/// bo2mp: Call of Duty 4 maps whose MW2 version has another zone name.
const COD4_IN_MW2: &[(&str, &str)] = &[("mp_bog", "mp_bog_sh"), ("mp_crossfire", "mp_cross_fire")];

/// The MW2 zone that is Call of Duty 4's `stem` (the same name, or
/// `COD4_IN_MW2`'s).
fn mw2_version_of(stem: &str) -> &str {
    COD4_IN_MW2.iter().find(|(c, _)| *c == stem).map_or(stem, |(_, m)| m)
}

const GAMES: &[Game] = &[
    Game {
        prefix: "iw4",
        key: "MW2",
        title: "Modern Warfare 2",
        base: &[
            ("mp_afghan", "Afghan"),
            ("mp_derail", "Derail"),
            ("mp_estate", "Estate"),
            ("mp_favela", "Favela"),
            ("mp_highrise", "Highrise"),
            ("mp_invasion", "Invasion"),
            ("mp_checkpoint", "Karachi"),
            ("mp_quarry", "Quarry"),
            ("mp_rundown", "Rundown"),
            ("mp_rust", "Rust"),
            ("mp_boneyard", "Scrapyard"),
            ("mp_nightshift", "Skidrow"),
            ("mp_subbase", "Sub Base"),
            ("mp_terminal", "Terminal"),
            ("mp_underpass", "Underpass"),
            ("mp_brecourt", "Wasteland"),
        ],
        dlc: &[
            ("mp_complex", "Bailout"),
            ("mp_crash", "Crash"),
            ("mp_overgrown", "Overgrown"),
            ("mp_compact", "Salvage"),
            ("mp_storm", "Storm"),
            ("mp_abandon", "Carnival"),
            ("mp_fuel2", "Fuel"),
            ("mp_strike", "Strike"),
            ("mp_trailerpark", "Trailer Park"),
            ("mp_vacant", "Vacant"),
        ],
    },
    Game {
        prefix: "iw5",
        key: "MW3",
        title: "Modern Warfare 3",
        base: &[
            ("mp_mogadishu", "Bakaara"),
            ("mp_alpha", "Lockdown"),
            ("mp_bootleg", "Bootleg"),
            ("mp_bravo", "Mission"),
            ("mp_carbon", "Carbon"),
            ("mp_dome", "Dome"),
            ("mp_exchange", "Downturn"),
            ("mp_hardhat", "Hardhat"),
            ("mp_interchange", "Interchange"),
            ("mp_lambeth", "Fallen"),
            ("mp_paris", "Resistance"),
            ("mp_plaza2", "Arkaden"),
            ("mp_radar", "Outpost"),
            ("mp_seatown", "Seatown"),
            ("mp_underground", "Underground"),
            ("mp_village", "Village"),
        ],
        dlc: &[
            ("mp_park", "Liberation"),
            ("mp_overwatch", "Overwatch"),
            ("mp_morningwood", "Black Box"),
            ("mp_italy", "Piazza"),
            ("mp_meteora", "Sanctuary"),
            ("mp_cement", "Foundation"),
            ("mp_qadeem", "Oasis"),
            ("mp_terminal_cls", "Terminal"),
            ("mp_hillside_ss", "Getaway"),
            ("mp_restrepo_ss", "Lookout"),
            ("mp_aground_ss", "Aground"),
            ("mp_courtyard_ss", "Erosion"),
            ("mp_crosswalk_ss", "Intersection"),
            ("mp_burn_ss", "U-Turn"),
            ("mp_six_ss", "Vortex"),
            ("mp_shipbreaker", "Decommission"),
            ("mp_roughneck", "Off Shore"),
            ("mp_moab", "Gulch"),
            ("mp_boardwalk", "Boardwalk"),
            ("mp_nola", "Parish"),
        ],
    },
    Game {
        prefix: "t5",
        key: "BO1",
        title: "Black Ops",
        base: &[
            ("mp_array", "Array"),
            ("mp_cracked", "Cracked"),
            ("mp_crisis", "Crisis"),
            ("mp_firingrange", "Firing Range"),
            ("mp_duga", "Grid"),
            ("mp_hanoi", "Hanoi"),
            ("mp_cairo", "Havana"),
            ("mp_havoc", "Jungle"),
            ("mp_cosmodrome", "Launch"),
            ("mp_nuked", "Nuketown"),
            ("mp_radiation", "Radiation"),
            ("mp_mountain", "Summit"),
            ("mp_villa", "Villa"),
            ("mp_russianbase", "WMD"),
        ],
        dlc: &[
            ("mp_berlinwall2", "Berlin Wall"),
            ("mp_discovery", "Discovery"),
            ("mp_kowloon", "Kowloon"),
            ("mp_stadium", "Stadium"),
            ("mp_gridlock", "Convoy"),
            ("mp_hotel", "Hotel"),
            ("mp_outskirts", "Stockpile"),
            ("mp_zoo", "Zoo"),
            ("mp_area51", "Hangar 18"),
            ("mp_silo", "Silo"),
            ("mp_drivein", "Drive-In"),
            ("mp_golfcourse", "Hazard"),
        ],
    },
    Game {
        prefix: "t4",
        key: "WAW",
        title: "World at War",
        base: &[
            ("mp_airfield", "Airfield"),
            ("mp_asylum", "Asylum"),
            ("mp_castle", "Castle"),
            ("mp_shrine", "Cliffside"),
            ("mp_courtyard", "Courtyard"),
            ("mp_dome", "Dome"),
            ("mp_downfall", "Downfall"),
            ("mp_hangar", "Hangar"),
            ("mp_makin", "Makin"),
            ("mp_outskirts", "Outskirts"),
            ("mp_roundhouse", "Roundhouse"),
            ("mp_seelow", "Seelow"),
            ("mp_suburban", "Upheaval"),
        ],
        dlc: &[
            ("mp_makin_day", "Makin Day"),
            ("mp_kneedeep", "Knee Deep"),
            ("mp_nachtfeuer", "Nightfire"),
            ("mp_subway", "Station"),
            ("mp_stalingrad", "Corrosion"),
            ("mp_docks", "Sub Pens"),
            ("mp_kwai", "Banzai"),
            ("mp_vodka", "Revolution"),
            ("mp_bgate", "Breach"),
            ("mp_drum", "Battery"),
        ],
    },
    Game {
        prefix: "iw6",
        key: "GHOSTS",
        title: "Ghosts",
        base: &[
            ("mp_chasm", "Chasm"),
            ("mp_flooded", "Flooded"),
            ("mp_frag", "Freight"),
            ("mp_descent_new", "Free Fall"),
            ("mp_dart", "Octane"),
            ("mp_zebra", "Overlord"),
            ("mp_prisonbreak", "Prison Break"),
            ("mp_hashima", "Siege"),
            ("mp_sovereign", "Sovereign"),
            ("mp_skeleton", "Stonehaven"),
            ("mp_fahrenheit", "Stormfront"),
            ("mp_strikezone", "Strikezone"),
            ("mp_lonestar", "Tremor"),
            ("mp_warhawk", "Warhawk"),
            ("mp_snow", "Whiteout"),
        ],
        dlc: &[
            ("mp_swamp", "Fog"),
            ("mp_ca_rumble", "Bayview"),
            ("mp_ca_red_river", "Containment"),
            ("mp_boneyard_ns", "Ignition"),
            ("mp_battery3", "Ruins"),
            ("mp_ca_impact", "Collision"),
            ("mp_ca_behemoth", "Behemoth"),
            ("mp_dome_ns", "Unearthed"),
            ("mp_dig", "Pharaoh"),
            ("mp_favela_iw6", "Favela"),
            ("mp_pirate", "Mutiny"),
            ("mp_zulu", "Departed"),
            ("mp_zerosub", "Subzero"),
            ("mp_conflict", "Dynasty"),
            ("mp_mine", "Goldrush"),
            ("mp_shipment_ns", "Showtime"),
        ],
    },
    // bo2mp: Call of Duty: Infinite Warfare (its maps through
    // `assets::lane::iw7`). Titles as the game's own English text names
    // them (`presence_<map>` in `english/eng_code_post_gfx.ff`, past the
    // zone's font); `mp_dome_iw` is the `mp_dome` there. The free re-releases
    // of DLC maps (`IW7_VARIANTS`) are not listed when their originals are
    // installed.
    Game {
        prefix: "iw7",
        key: "IW",
        title: "Infinite Warfare",
        base: &[
            ("mp_parkour", "Breakout"),
            ("mp_quarry", "Crusher"),
            ("mp_frontier", "Frontier"),
            ("mp_proto", "Frost"),
            ("mp_dome_iw", "Genesis"),
            ("mp_breakneck", "Mayday"),
            ("mp_metropolis", "Precinct"),
            ("mp_riot", "Retaliation"),
            ("mp_divide", "Scorch"),
            ("mp_rivet", "SkyDock"),
            ("mp_fallen", "Throwback"),
            ("mp_skyway", "Terminal"),
        ],
        dlc: &[
            ("mp_flip", "Excess"),
            ("mp_desert", "Grounded"),
            ("mp_afghan", "Dominion"),
            ("mp_prime", "Noir"),
            ("mp_geneva", "Renaissance"),
            ("mp_neon", "Neon"),
            ("mp_mansion", "Archive"),
            ("mp_paris", "Ember"),
            ("mp_junk", "Scrap"),
            ("mp_marsoasis", "Turista"),
            ("mp_nova", "Bermuda"),
            ("mp_pixel", "Fore"),
            ("mp_overflow", "Permafrost"),
            ("mp_codphish", "Altitude"),
            ("mp_rally", "Carnage"),
            ("mp_depot", "Depot 22"),
            ("mp_hawkwar", "Heartland"),
            ("mp_renaissance2", "Renaissance"),
            ("mp_carnage2", "Carnage"),
            ("mp_permafrost2", "Permafrost"),
            ("mp_turista2", "Turista"),
        ],
    },
    // bo2mp: Call of Duty: Black Ops III (its maps through
    // `assets::lane::t7_map`).
    Game {
        prefix: "t7",
        key: "BO3",
        title: "Black Ops III",
        base: &[
            ("mp_biodome", "Aquarium"),
            ("mp_spire", "Breach"),
            ("mp_sector", "Combine"),
            ("mp_apartments", "Evac"),
            ("mp_chinatown", "Exodus"),
            ("mp_veiled", "Fringe"),
            ("mp_havoc", "Havoc"),
            ("mp_ethiopia", "Hunted"),
            ("mp_infection", "Infection"),
            ("mp_metro", "Metro"),
            ("mp_nuketown_x", "Nuk3town"),
            ("mp_redwood", "Redwood"),
            ("mp_stronghold", "Stronghold"),
        ],
        dlc: &[
            ("mp_redwood_ice", "Redwood Snow"),
            ("mp_veiled_heyday", "Fringe Night"),
            ("mp_crucible", "Gauntlet"),
            ("mp_rise", "Rise"),
            ("mp_skyjacked", "Skyjacked"),
            ("mp_waterpark", "Splash"),
            ("mp_conduit", "Rift"),
            ("mp_kung_fu", "Knockout"),
            ("mp_aerospace", "Spire"),
            ("mp_banzai", "Verge"),
            ("mp_arena", "Rumble"),
            ("mp_cryogen", "Cryogen"),
            ("mp_rome", "Empire"),
            ("mp_ruins", "Citadel"),
            ("mp_shrine", "Berserk"),
            ("mp_miniature", "Micro"),
            ("mp_western", "Outlaw"),
            ("mp_city", "Rupture"),
        ],
    },
    // bo2mp: Call of Duty: Modern Warfare III (2023) (its maps through
    // `assets::lane::jup_map`; the zones played are
    // `asset_transport::JUP_MAPS`): Tokyo (Season 4, `mp_jup_elevated`),
    // Rebirth Island (Resurgence's map, under WARZONE) and its multiplayer
    // maps. Its English text is not readable from the zones, so the titles
    // are the maps' public names.
    Game {
        prefix: "jup",
        key: "MWIII",
        title: "Modern Warfare III (2023)",
        base: &[
            ("mp_jup_elevated", "Tokyo"),
            ("mp_jup_escape5", "Rebirth Island"),
            ("mp_jup_afghan_v2", "Afghan"),
            ("mp_jup_derail", "Derail"),
            ("mp_jup_estate", "Estate"),
            ("mp_jup_favela", "Favela"),
            ("mp_jup_highrise", "Highrise"),
            ("mp_jup_invasion", "Invasion"),
            ("mp_jup_karachi", "Karachi"),
            ("mp_jup_quarry", "Quarry"),
            ("mp_jup_rundown", "Rundown"),
            ("mp_jup_rust", "Rust"),
            ("mp_jup_scrapyard", "Scrapyard"),
            ("mp_jup_skidrow", "Skidrow"),
            ("mp_jup_subbase", "Sub Base"),
            ("mp_jup_terminal", "Terminal"),
            ("mp_jup_underpass", "Underpass"),
            ("mp_jup_wasteland", "Wasteland"),
            ("mp_jup_das_haus", "Das Haus"),
            ("mp_jup_growhouse", "Growhouse"),
        ],
        dlc: &[],
    },
    // bo2mp: Call of Duty: Modern Warfare II (2022) ("iw9"): no install of
    // its own is read (its zones are signed and encrypted); its maps are
    // the ones the Modern Warfare III install carries
    // (`asset_transport::MWII_MAPS`, each tied to MWII by its zone: its
    // world's source at the root of MWII's map tree, its assets under MWII's
    // name, its map-table row without MWIII's `MP_JUP_`), found under `jup`,
    // played by the same reader (`assets::lane::jup_map`) and listed on
    // their own tab, never on MWIII's. Titles: the map-table key where it
    // is the map's public name (DOME, FARM_18, SHIPMENT, SHOOTHOUSE2, the
    // Gunfight maps M_ALLEY, M_BLACKSITE, M_EXHIBIT), Crown Raceway for
    // GRANDPRIX (MWII's `mp_grandprix` is Crown Raceway), and the key itself
    // where no public name is tied to it (SWAP_MEET, LUXURY: unverified).
    Game {
        prefix: MWII_PREFIX,
        key: "MWII",
        title: MWII_TITLE,
        base: &[
            ("mp_jup_shipment_iw9", "Shipment"),
            ("mp_jup_shoothouse2", "Shoot House"),
            ("mp_jup_farm_18", "Farm 18"),
            ("mp_jup_observatory", "Dome"),
            ("mp_jup_grandprix_pm_2", "Crown Raceway"),
            ("mp_jup_swap_meet", "Swap Meet"),
            ("mp_jup_luxury", "Luxury"),
            ("mp_jup_m_alley", "Alley"),
            ("mp_jup_m_blacksite2", "Blacksite"),
            ("mp_jup_m_exhibit", "Exhibit"),
        ],
        dlc: &[],
    },
    // bo2mp: Call of Duty: Advanced Warfare. Its own map zones are not
    // read; the table names its maps rebuilt for Modern Warfare Remastered
    // (Horizon MW's usermaps, listed under this game as MWR's versions).
    // Titles from Horizon MW's English text (`MPUI_<map>`) where it has
    // them; the four it does not name keep their zone names.
    Game {
        prefix: "s1",
        key: "AW",
        title: "Advanced Warfare",
        base: &[
            ("mp_refraction", "Ascend"),
            ("mp_lab2", "Bio Lab"),
            ("mp_comeback", "Comeback"),
            ("mp_laser2", "Defender"),
            ("mp_detroit", "Detroit"),
            ("mp_greenband", "Greenband"),
            ("mp_levity", "Horizon"),
            ("mp_instinct", "Instinct"),
            ("mp_recovery", "Recovery"),
            ("mp_venus", "Retreat"),
            ("mp_prison", "Riot"),
            ("mp_solar", "Solar"),
            ("mp_terrace", "Terrace"),
            ("mp_dam", "Atlas Gorge"),
        ],
        dlc: &[
            ("mp_bigben2", "Parliament"),
            ("mp_blackbox", "Site 244"),
            ("mp_climate_3", "Climate"),
            ("mp_clowntown3", "Sideshow"),
            ("mp_fracture", "Fracture"),
            ("mp_highrise2", "Skyrise"),
            ("mp_kremlin", "Kremlin"),
            ("mp_lost", "Core"),
            ("mp_perplex_1", "Perplex"),
            ("mp_spark", "Chop Shop"),
            ("mp_torqued", "Drift"),
            ("mp_urban", "Urban"),
            ("mp_lair", "Lair"),
            ("mp_liberty", "Liberty"),
            ("mp_sector17", "Sector 17"),
            ("mp_seoul2", "Seoul"),
        ],
    },
    // bo2mp: Call of Duty: Modern Warfare Remastered (its maps through
    // `assets::lane::h1_map`): Call of Duty 4's maps as remastered, and
    // Horizon MW's usermaps (`hmw-usermaps/`: Modern Warfare 2's, Modern
    // Warfare 3's and Advanced Warfare's maps rebuilt for it). A usermap
    // another game's table names is listed under that game, marked as
    // MWR's version; Advanced Warfare's are named by the `s1` table.
    Game {
        prefix: "h1",
        key: "COD4",
        title: "Call of Duty 4",
        base: &[
            ("mp_convoy", "Ambush"),
            ("mp_backlot", "Backlot"),
            ("mp_bloc", "Bloc"),
            ("mp_bog", "Bog"),
            ("mp_countdown", "Countdown"),
            ("mp_crash", "Crash"),
            ("mp_crossfire", "Crossfire"),
            ("mp_citystreets", "District"),
            ("mp_farm", "Downpour"),
            ("mp_overgrown", "Overgrown"),
            ("mp_pipeline", "Pipeline"),
            ("mp_shipment", "Shipment"),
            ("mp_showdown", "Showdown"),
            ("mp_strike", "Strike"),
            ("mp_vacant", "Vacant"),
            ("mp_cargoship", "Wet Work"),
            ("mp_killhouse", "Killhouse"),
        ],
        dlc: &[
            ("mp_bog_summer", "Beach Bog"),
            ("mp_broadcast", "Broadcast"),
            ("mp_carentan", "Chinatown"),
            ("mp_creek", "Creek"),
            ("mp_farm_spring", "Day Break"),
            ("mp_crash_snow", "Winter Crash"),
        ],
    },
];

/// bo2mp: the end of a Steam Workshop map's origin (`Black Ops III
/// (Workshop)`): the map picker's cards wear a WORKSHOP badge.
pub(crate) const WORKSHOP_SUFFIX: &str = " (Workshop)";

/// Whether a map's origin is a Steam Workshop map's.
pub(crate) fn is_workshop_origin(origin: &str) -> bool {
    origin.ends_with(WORKSHOP_SUFFIX)
}

/// A map from another game, as the menus list it.
#[derive(Clone, Debug)]
pub(crate) struct ForeignMap {
    /// The name the menus know it by (`ui_mapname`): its zone name, with
    /// the game's prefix added when another game has a map by that name.
    pub ui_name: String,
    pub game: &'static str,
    pub stem: String,
    pub title: String,
    /// The game it comes from (`Modern Warfare 2 (DLC)`).
    pub origin: String,
    /// The map picker's section: the game it was made for (`MODERN
    /// WARFARE 3` for MW3's maps ported into MW2), or `CUSTOM MAPS`.
    pub section: String,
}

/// A category in the Change Map popup.
#[derive(Clone, Debug)]
pub(crate) struct Category {
    /// `MPUI_MAP_CATEGORY_<name>_CAPS`.
    pub name: String,
    pub title: String,
    pub description: String,
    pub pack: u32,
    pub maps: Vec<ForeignMap>,
}

static CATEGORIES: OnceLock<Vec<Category>> = OnceLock::new();

/// The categories, found once (an empty list when no other game is
/// installed).
pub(crate) fn categories() -> &'static [Category] {
    CATEGORIES.get_or_init(|| {
        let started = std::time::Instant::now();
        let categories = discover();
        diag::info!(Ui, "bo2mp maps: found in {:.2} s", started.elapsed().as_secs_f32());
        session::bo2mp_setup::register_foreign_maps(
            categories
                .iter()
                .flat_map(|c| c.maps.iter())
                .map(|m| (m.ui_name.clone(), m.game.to_owned(), m.stem.clone()))
                .collect(),
        );
        hks_t6::host::set_extra_map_categories(
            categories
                .iter()
                .map(|c| (c.name.clone(), c.pack, "menu_mp_map_category_dlc0".to_owned()))
                .collect(),
        );
        diag::info!(
            Ui,
            "bo2mp maps: {} more categories: {}",
            categories.len(),
            categories
                .iter()
                .map(|c| format!("{} ({})", c.title, c.maps.len()))
                .collect::<Vec<_>>()
                .join(", ")
        );
        categories
    })
}

/// Finds the categories in the background (once a run), so the match HUD,
/// built when play starts, finds them ready: found then, from the HDD, they
/// froze the first frame of a match started without the front end for up
/// to 20 s.
pub(crate) fn warm_categories() {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    // (Counted as a picture read: a menu asking for a map's picture keeps
    // asking while the list is found.)
    READS_IN_FLIGHT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let spawned = std::thread::Builder::new().name("ui-map-list".into()).spawn(|| {
        categories();
        READS_IN_FLIGHT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    });
    if spawned.is_err() {
        READS_IN_FLIGHT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// bo2mp: what of Modern Warfare II (2022) the Modern Warfare III install
/// has: MWII's maps (`asset_transport::MWII_MAPS`, played by the MWIII
/// reader) and MWIII's, each confirmed by its `eng_<zone>.ff`, and the other
/// `mp_*` map zones there that neither list names.
fn mwii_2022_report(installed: &[(String, String)]) {
    let listed: Vec<&str> = installed.iter().filter(|(g, s)| g == "jup" && asset_transport::is_mwii_map(s)).map(|(_, s)| s.as_str()).collect();
    let Some(install) = crate::jup_cards::install() else {
        diag::info!(Ui, "bo2mp maps: Modern Warfare II (2022): not listed (its maps are read from the Modern Warfare III install, and none is set)");
        return;
    };
    let names: Vec<String> = std::fs::read_dir(install.join("zone"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_ascii_lowercase))
        .collect();
    let mut eng: Vec<String> = names
        .iter()
        .filter_map(|n| n.strip_prefix("eng_").and_then(|n| n.strip_suffix(".ff")).map(str::to_owned))
        .filter(|n| n.starts_with("mp_") && !asset_transport::is_jup_map(n))
        .collect();
    eng.sort();
    // (Each listed zone's `eng_<zone>.ff` is the install's word that the
    // map is there.)
    let absent = |list: &[&'static str]| -> Vec<&'static str> { list.iter().copied().filter(|m| !names.iter().any(|n| n == &format!("eng_{m}.ff"))).collect() };
    let jup_absent = absent(&asset_transport::JUP_MAPS);
    let mwii_absent = absent(&asset_transport::MWII_MAPS);
    diag::info!(
        Ui,
        "bo2mp maps: Modern Warfare III (2023): {} of {} listed map zones confirmed by their eng_<zone>.ff{}; map titles are public names (unverified: the install's localized titles are encrypted)",
        asset_transport::JUP_MAPS.len() - jup_absent.len(),
        asset_transport::JUP_MAPS.len(),
        if jup_absent.is_empty() { String::new() } else { format!(" (no eng_ zone: {})", jup_absent.join(", ")) }
    );
    diag::info!(
        Ui,
        "bo2mp maps: Modern Warfare II (2022): {} of {} MWII map zones confirmed by their eng_<zone>.ff{}, {} listed from {} ({}); other map zones there (neither game's listed maps: modes, Zombies, variants): {} {}",
        asset_transport::MWII_MAPS.len() - mwii_absent.len(),
        asset_transport::MWII_MAPS.len(),
        if mwii_absent.is_empty() { String::new() } else { format!(" (no eng_ zone: {})", mwii_absent.join(", ")) },
        listed.len(),
        install.display(),
        if listed.is_empty() { "none".to_owned() } else { listed.join(", ") },
        eng.len(),
        if eng.is_empty() { "none".to_owned() } else { eng.iter().take(400).cloned().collect::<Vec<_>>().join(", ") }
    );
}

/// bo2mp: the Modern Warfare II (2022) guns the Modern Warfare III install
/// carries (their view-model part zones,
/// `mp_av_att_vm_pNN_<class>_<gun>_<part>_..._tr`, MWII's parts ending
/// `_v0`), against the ones played (`bo2_profile::jup_names::JUP_PLAYED`):
/// logged once, the ones not played named (a gun needs its part list in
/// `assets::lane::jup_gun::GUNS` before it can be played).
fn mwii_guns_report() {
    static DONE: OnceLock<()> = OnceLock::new();
    if DONE.set(()).is_err() {
        return;
    }
    let Some(install) = crate::jup_cards::install() else { return };
    let mut found: Vec<String> = std::fs::read_dir(install.join("zone"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_ascii_lowercase))
        .filter_map(|n| {
            let rest = n.strip_prefix("mp_av_att_vm_p")?;
            let rest = rest.get(2..)?.strip_prefix('_')?;
            // (MWIII's own guns' parts carry `jup_jpNN_`; MWII's do not.)
            if rest.starts_with("jup_") || !n.contains("_v0") {
                return None;
            }
            let mut parts = rest.split('_');
            let (class, gun) = (parts.next()?, parts.next()?);
            (class.len() == 2 && !gun.is_empty()).then(|| format!("iw9_{class}_{gun}"))
        })
        .collect();
    found.sort();
    found.dedup();
    let played = bo2_profile::jup_names::JUP_PLAYED;
    let not_played: Vec<&str> = found.iter().map(String::as_str).filter(|g| !played.contains(g)).collect();
    let absent: Vec<&str> = played.iter().copied().filter(|g| g.starts_with("iw9_") && !found.iter().any(|f| f == g)).collect();
    diag::info!(
        Ui,
        "bo2mp guest guns: Modern Warfare II (2022) guns in the Modern Warfare III install: {} found, {} played; not played (no part list yet): {}; played but no `_v0` part zone seen: {}",
        found.len(),
        found.len() - not_played.len(),
        if not_played.is_empty() { "none".to_owned() } else { not_played.join(", ") },
        if absent.is_empty() { "none".to_owned() } else { absent.join(", ") }
    );
}

fn title_of(stem: &str) -> String {
    stem.trim_start_matches("mp_")
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// bo2mp: a usermap's title from its zone name, a source game tag moved to
/// the end (`mp_waw_castle` -> `Castle (WaW)`).
fn usermap_title(stem: &str) -> String {
    const TAGS: &[(&str, &str)] = &[
        ("waw", "WaW"),
        ("t4", "WaW"),
        ("cod4", "CoD4"),
        ("iw3", "CoD4"),
        ("cod2", "CoD2"),
        ("cod1", "CoD"),
        ("uo", "UO"),
        ("bo1", "BO1"),
        ("bo2", "BO2"),
        ("bo3", "BO3"),
        ("t5", "BO1"),
        ("mw1", "MW1"),
        ("mw2", "MW2"),
        ("mw3", "MW3"),
        ("mwr", "MWR"),
        ("codo", "CoDO"),
        ("ghosts", "Ghosts"),
    ];
    let words: Vec<&str> = stem.trim_start_matches("mp_").split('_').filter(|w| !w.is_empty()).collect();
    let tag_of = |w: &str| TAGS.iter().find(|(t, _)| w.eq_ignore_ascii_case(t)).map(|(_, n)| *n);
    let (tag, rest) = match (words.first().copied().and_then(tag_of), words.last().copied().and_then(tag_of)) {
        (Some(tag), _) if words.len() > 1 => (Some(tag), &words[1..]),
        (_, Some(tag)) if words.len() > 1 => (Some(tag), &words[..words.len() - 1]),
        _ => (None, &words[..]),
    };
    let title = title_of(&rest.join("_"));
    match tag {
        Some(tag) => format!("{title} ({tag})"),
        None => title,
    }
}

fn discover() -> Vec<Category> {
    let mut categories = discover_cod();
    // bo2mp: Battlefield 1's own section, after the classic pages (only with a
    // Battlefield 1 install set; nothing is read otherwise).
    #[cfg(feature = "bf1")]
    categories.extend(bf1_category(FIRST_PACK + categories.len() as u32));
    categories
}

#[cfg(feature = "bf1")]
/// bo2mp: the Battlefield 1 section of the Change Map popup: its levels
/// (`bf1_adapter::catalog`), each tagged by what it can do, `reader only`
/// (Verdun Heights) or `not yet loadable`. Picking one fails the load with
/// the reason (`asset_transport::discover`) and logs the level report. None
/// without a usable install (`BF1_FILES` or the `bf1` line of
/// game_paths.cfg).
fn bf1_category(pack: u32) -> Option<Category> {
    let configured = asset_transport::game_paths::get(asset_transport::game_paths::BF1_KEY);
    let root = match bf1_adapter::config::resolve_from_env(configured.as_deref()) {
        Ok(root) => root,
        Err(bf1_adapter::config::Bf1Error::NotConfigured) => return None,
        Err(e) => {
            diag::warn!(Ui, "bo2mp maps: Battlefield 1 not listed: {e}");
            return None;
        }
    };
    let listed = bf1_adapter::catalog::catalog(Some(&root));
    if listed.is_empty() {
        diag::warn!(Ui, "bo2mp maps: Battlefield 1 install {} has no multiplayer levels under Data/Win32", root.display());
        return None;
    }
    Some(bf1_category_of(&listed, pack))
}

#[cfg(feature = "bf1")]
/// The section for a map list (split from [`bf1_category`] to be testable).
fn bf1_category_of(listed: &[bf1_adapter::catalog::ListedMap], pack: u32) -> Category {
    let maps: Vec<ForeignMap> = listed
        .iter()
        .map(|m| ForeignMap {
            // (Always suffixed: the session finds the game by `_bf1`.)
            ui_name: format!("{}_{}", m.id, bf1_adapter::FAMILY),
            game: bf1_adapter::FAMILY,
            stem: m.id.clone(),
            title: m.label(),
            origin: "Battlefield 1".to_owned(),
            section: bf1_adapter::SECTION_TITLE.to_owned(),
        })
        .collect();
    Category {
        name: "BO2RB_BF1".to_owned(),
        title: bf1_adapter::SECTION_TITLE.to_owned(),
        description: format!("Battlefield 1: {} maps listed, none playable yet (see docs/BF1.md).", maps.len()),
        pack,
        maps,
    }
}

/// Infinite Warfare is parked (the owner, 11-05): its maps and guns are not
/// listed or played until the work on it resumes (the renderer's speed,
/// lighting and collision are not right yet). The code stays;
/// `BO2MP_IW7=1` lists them again.
#[must_use]
pub fn iw7_parked() -> bool {
    !std::env::var("BO2MP_IW7").is_ok_and(|v| v.trim() == "1")
}

fn discover_cod() -> Vec<Category> {
    let Ok(root) = asset_transport::games_root_from_env() else {
        return Vec::new();
    };
    let packs = asset_transport::list_mp_map_packs(&root);
    // bo2mp: usermaps (`<game>/usermaps/<map>/<map>.ff`), by `game:map`.
    let custom: Vec<String> = packs
        .iter()
        .filter(|p| p.label.ends_with(asset_transport::USERMAPS_PACK))
        .flat_map(|p| p.maps.iter().cloned())
        .collect();
    if !custom.is_empty() {
        diag::info!(Ui, "bo2mp maps: usermaps {}", custom.join(", "));
    }
    // Every map zone of every installed game but Black Ops II's own.
    let mut installed: Vec<(String, String)> = packs
        .into_iter()
        .flat_map(|p| p.maps)
        .filter_map(|key| {
            let (game, stem) = key.split_once(':')?;
            // (Modern Warfare 2 (2009) and Call of Duty 4 are not played any
            // more, installed or not: the owner, 11-04. Their code stays.)
            (!matches!(game, "t6" | "iw3" | "iw4")).then(|| (game.to_owned(), stem.to_owned()))
        })
        .collect();
    // Call of Duty 4's and World at War's maps (read by their own readers:
    // `iw3`, and `t4` through the CoD4 lane), and Ghosts' (`iw6`, read by
    // `fastfile_iw6`).
    installed.extend(
        asset_transport::list_legacy_mp_maps(&root)
            .into_iter()
            // (Modern Warfare III (2023)'s maps are left out of the game: the
            // owner, 11-05. Its emblems and calling cards stay.)
            .filter(|(game, _)| matches!(game.as_str(), "t4" | "iw6" | "t7" | "h1") || (game == "iw7" && !iw7_parked())),
    );
    installed.sort();
    installed.dedup();
    mwii_2022_report(&installed);
    // bo2mp: a Modern Warfare 3 (2011) map that MWR or Horizon MW has a
    // version of is played as that version only: the 2011 one is left out
    // (the owner, 11-05: the same map twice is just extra space). MW3's
    // other maps stay.
    let mwr: std::collections::HashSet<String> = installed.iter().filter(|(g, _)| g == "h1").map(|(_, s)| s.clone()).collect();
    let remade = |g: &str, s: &str| g == "iw5" && (mwr.contains(s) || mwr.contains(mw2_version_of(s)) || mwr.iter().any(|m| mw2_version_of(m) == s));
    let replaced: Vec<String> = installed.iter().filter(|(g, s)| remade(g, s)).map(|(_, s)| s.clone()).collect();
    if !replaced.is_empty() {
        diag::info!(Ui, "bo2mp maps: {} Modern Warfare 3 maps played as MWR's version only: {}", replaced.len(), replaced.join(" "));
        installed.retain(|(g, s)| !remade(g, s));
    }
    // bo2mp: a Black Ops III Workshop map that recreates one of Black Ops'
    // maps as it was (Summit, Firing Range, Jungle) is played instead of
    // the original, which is left out (the owner, 11-05). Reimaginings
    // (Nuk3town, Dome) stay beside the originals.
    let recreated: Vec<&'static str> = BO1_RECREATIONS
        .iter()
        .filter(|(ws, _)| installed.iter().any(|(g, s)| g == "t7" && s == ws) && asset_transport::workshop::bo3_map(ws).is_some())
        .map(|(_, bo1)| *bo1)
        .collect();
    if !recreated.is_empty() {
        diag::info!(Ui, "bo2mp maps: Black Ops maps played as their Black Ops III Workshop recreation: {}", recreated.join(" "));
        installed.retain(|(g, s)| !(g == "t5" && recreated.contains(&s.as_str())));
    }
    // (A stem two games use, or one of Black Ops II's own maps' names: BO2's
    // Standoff and MW3's Village are both `mp_village`, BO2's and BO1's
    // Studio both `mp_studio`. Those carry their game in the name.)
    let shared = |stem: &str| BO2_MAPS.contains(&stem) || installed.iter().filter(|(_, s)| s == stem).count() > 1;
    // Every game's maps in one list (base maps, then DLC, game by game),
    // split into sections of at most `PAGE` so no list runs off the screen.
    let mut all: Vec<ForeignMap> = Vec::new();
    for game in GAMES {
        let is_custom = |stem: &str| custom.iter().any(|c| c.split_once(':') == Some((game.prefix, stem)));
        let all_mine: Vec<&String> = installed.iter().filter(|(g, _)| g == game.prefix).map(|(_, s)| s).collect();
        if all_mine.is_empty() {
            continue;
        }
        let (custom_mine, mut mine): (Vec<&String>, Vec<&String>) = all_mine.into_iter().partition(|s| is_custom(s));
        // bo2mp: Black Ops III's community maps (Steam Workshop items and
        // its `usermaps`: `asset_transport::workshop`), titled as their
        // `workshop.json` says, after its own maps.
        let workshop: Vec<(String, String)> = if game.prefix == "t7" {
            let (ws, rest): (Vec<&String>, Vec<&String>) = mine.into_iter().partition(|s| asset_transport::workshop::bo3_map(s).is_some());
            mine = rest;
            let ws: Vec<&asset_transport::workshop::Bo3CustomMap> =
                ws.iter().filter_map(|s| asset_transport::workshop::bo3_map(s)).filter(|m| m.mode == asset_transport::workshop::MapMode::Multiplayer).collect();
            // bo2mp: a remake of one of Black Ops II's own maps (Slums) is
            // not listed: BO2's original is played (the owner, 11-04).
            let (bo2_own, ws): (Vec<_>, Vec<_>) = ws.into_iter().partition(|m| is_bo2_remake(&m.stem));
            if !bo2_own.is_empty() {
                diag::info!(Ui, "bo2mp maps: Black Ops III Workshop remakes of Black Ops II's own maps not listed (BO2's original is): {}", bo2_own.iter().map(|m| format!("{} '{}'", m.stem, m.title)).collect::<Vec<_>>().join(", "));
            }
            ws.into_iter().map(|m| (m.stem.clone(), asset_transport::workshop::display_title(&m.title))).collect()
        } else {
            Vec::new()
        };
        if !workshop.is_empty() {
            diag::info!(Ui, "bo2mp maps: Black Ops III Workshop maps listed: {}", workshop.iter().map(|(s, t)| format!("{s} '{t}'")).collect::<Vec<_>>().join(", "));
        }
        if game.prefix == "iw7" {
            let has = |stem: &str| mine.iter().any(|m| m.as_str() == stem);
            let hidden: Vec<&str> = IW7_VARIANTS.iter().filter(|(v, original)| has(v) && has(original)).map(|(v, _)| *v).collect();
            mine.retain(|m| !hidden.contains(&m.as_str()));
        }
        let known = |list: &[(&str, &str)]| -> Vec<(String, String)> {
            list.iter()
                .filter(|(s, _)| mine.iter().any(|m| m.as_str() == *s))
                .map(|(s, t)| ((*s).to_owned(), (*t).to_owned()))
                .collect()
        };
        let base = known(game.base);
        let mut dlc = known(game.dlc);
        // IW4x's pack: MW2's own maps, with IW4x's titles.
        let iw4x: &[(&str, &str)] = if game.prefix == "iw4" { IW4X } else { &[] };
        dlc.extend(known(iw4x));
        // Another game's map ported into this one (IW4x's CoD4 and MW3
        // packs in MW2): its own title, listed as that game's map.
        let mut ports: Vec<(String, String, &'static str)> = Vec::new();
        // Maps the tables do not name (a remaster's extra, a mod map) go
        // with the DLC, by their zone names.
        for stem in &mine {
            if game.base.iter().chain(game.dlc).chain(iw4x).any(|(s, _)| *s == stem.as_str()) {
                continue;
            }
            let from = GAMES
                .iter()
                .filter(|g| g.prefix != game.prefix)
                .find_map(|g| g.base.iter().chain(g.dlc).find(|(s, _)| *s == stem.as_str()).map(|(_, t)| ((*t).to_owned(), g.title)));
            match from {
                Some((title, origin)) => ports.push(((*stem).clone(), title, origin)),
                None => dlc.push(((*stem).clone(), title_of(stem))),
            }
        }
        // Usermaps last, titled from their zone names.
        let custom_maps: Vec<(String, String)> =
            custom_mine.iter().map(|s| ((*s).clone(), usermap_title(s))).collect();
        for (stem, title, origin) in ports {
            // MWR's version of another game's map is marked only when the
            // original is listed too (the owner, 11-04: no MWR text on the
            // remastered maps of a game whose own install is gone).
            let original_listed = installed.iter().any(|(g, s)| g != game.prefix && s == &stem && GAMES.iter().any(|x| x.prefix == g.as_str() && x.title == origin));
            let title = if game.prefix == "h1" && original_listed { format!("{title} (MWR)") } else { title };
            all.push(ForeignMap {
                ui_name: if shared(&stem) { format!("{stem}_{}", game.prefix) } else { stem.clone() },
                game: game.prefix,
                origin: if original_listed { format!("{origin} (in {})", game.title) } else { origin.to_owned() },
                stem,
                title,
                section: origin.to_uppercase(),
            });
        }
        for (maps, suffix) in [(base, ""), (dlc, " (DLC)"), (workshop, WORKSHOP_SUFFIX), (custom_maps, " (Custom)")] {
            for (stem, title) in maps {
                let cod4 = game.prefix == "iw4" && suffix != " (Custom)" && COD4_MAPS_IN_MW2.contains(&stem.as_str());
                all.push(ForeignMap {
                    ui_name: if shared(&stem) { format!("{stem}_{}", game.prefix) } else { stem.clone() },
                    game: game.prefix,
                    title: if cod4 { format!("{title} (MW2)") } else { title },
                    origin: if cod4 { "Call of Duty 4 (MW2 version)".to_owned() } else { format!("{}{suffix}", game.title) },
                    section: if suffix == " (Custom)" {
                        "CUSTOM MAPS".to_owned()
                    } else if cod4 {
                        "CALL OF DUTY 4".to_owned()
                    } else {
                        game.title.to_uppercase()
                    },
                    stem,
                });
            }
        }
    }
    let pages: Vec<&[ForeignMap]> = all.chunks(PAGE).collect();
    let numerals = ["", " II", " III", " IV", " V", " VI", " VII", " VIII", " IX", " X"];
    pages
        .iter()
        .enumerate()
        .map(|(i, chunk)| {
            let mut games: Vec<&str> = Vec::new();
            for m in chunk.iter() {
                let g = GAMES.iter().find(|g| g.prefix == m.game).map_or("", |g| g.title);
                if !games.contains(&g) {
                    games.push(g);
                }
            }
            Category {
                name: format!("BO2RB_CLASSIC{}", i + 1),
                title: format!("CLASSIC MAPS{}", numerals.get(i).copied().unwrap_or("")),
                description: format!("{}: {} maps.", games.join(", "), chunk.len()),
                pack: FIRST_PACK + i as u32,
                maps: chunk.to_vec(),
            }
        })
        .collect()
}

#[cfg(test)]
#[cfg(feature = "bf1")]
mod bf1_section_tests {
    use bf1_adapter::catalog::{ListedMap, MapStatus};

    #[test]
    fn the_section_lists_loadable_and_not_loadable_maps() {
        let listed = vec![
            ListedMap { id: "mp_verdun".into(), title: "Verdun Heights".into(), status: MapStatus::Readable },
            ListedMap { id: "mp_fields".into(), title: "Fields".into(), status: MapStatus::NotYetLoadable },
        ];
        let c = super::bf1_category_of(&listed, 25);
        assert_eq!((c.title.as_str(), c.pack, c.maps.len()), ("BATTLEFIELD 1", 25, 2));
        assert_eq!(c.maps[0].ui_name, "mp_verdun_bf1");
        assert_eq!(c.maps[0].title, "Verdun Heights (reader only)");
        assert_eq!(c.maps[1].title, "Fields (not yet loadable)");
        assert!(c.maps.iter().all(|m| m.section == "BATTLEFIELD 1" && m.game == "bf1"));
    }
}

#[cfg(test)]
mod usermap_title_tests {
    #[test]
    fn titles() {
        assert_eq!(super::usermap_title("mp_waw_castle"), "Castle (WaW)");
        assert_eq!(super::usermap_title("mp_castle_waw"), "Castle (WaW)");
        assert_eq!(super::usermap_title("mp_cod4_crash_snow"), "Crash Snow (CoD4)");
        assert_eq!(super::usermap_title("mp_nuketown"), "Nuketown");
        assert_eq!(super::usermap_title("mp_waw"), "Waw");
    }

    #[test]
    fn cod4_maps_have_their_mw2_versions() {
        assert_eq!(super::mw2_version_of("mp_bog"), "mp_bog_sh");
        assert_eq!(super::mw2_version_of("mp_backlot"), "mp_backlot");
        // No title twice in IW4x's list.
        let mut t: Vec<&str> = super::IW4X.iter().map(|(s, _)| *s).collect();
        t.sort();
        t.dedup();
        assert_eq!(t.len(), super::IW4X.len());
    }
}

/// The text key of a foreign map's name and description.
fn name_key(map: &ForeignMap) -> String {
    format!("BO2RB_MAP_{}", map.ui_name.to_ascii_uppercase())
}

/// Puts the foreign maps into BO2's tables and text: a mapstable row each
/// (Aftermath's, renamed), and each map's and category's text.
pub(crate) fn inject(values: &mut hks_t6::host::EngineValues) {
    let categories = categories();
    if categories.is_empty() {
        return;
    }
    for c in categories {
        values
            .localize
            .insert(format!("MPUI_MAP_CATEGORY_{}_CAPS", c.name), c.title.clone());
        values
            .localize
            .insert(format!("MPUI_MAP_CATEGORY_{}_DESC", c.name), c.description.clone());
        for m in &c.maps {
            values.localize.insert(name_key(m), m.title.to_uppercase());
            values.localize.insert(format!("{}_DESC", name_key(m)), format!("{} from {}.", m.title, m.origin));
        }
    }
    let Some(rows) = values.tables.get_mut("mp/mapstable.csv") else {
        return;
    };
    let Some(template) = rows.iter().find(|r| r.first().is_some_and(|c| c == ROW_FROM)).cloned() else {
        return;
    };
    let mut sort = 100;
    for c in categories {
        for m in &c.maps {
            if rows.iter().any(|r| r.first() == Some(&m.ui_name)) {
                continue;
            }
            let mut row = template.clone();
            row.resize(row.len().max(12), String::new());
            row[0] = m.ui_name.clone();
            row[3] = name_key(m);
            row[5] = sort.to_string();
            row[6] = format!("{}_DESC", name_key(m));
            row[11] = c.pack.to_string();
            sort += 1;
            rows.push(row);
        }
    }
}

type PictureMap = std::collections::HashMap<String, Option<std::sync::Arc<bevy::prelude::Image>>>;
static PICTURES: std::sync::Mutex<Option<PictureMap>> = std::sync::Mutex::new(None);

/// Marks `key` as being read; false when it is read or being read already.
fn claim_picture(key: &str) -> bool {
    let Ok(mut guard) = PICTURES.lock() else { return false };
    let done = guard.get_or_insert_with(Default::default);
    if done.contains_key(key) {
        return false;
    }
    done.insert(key.to_owned(), None);
    true
}

fn store_picture(key: String, image: Option<std::sync::Arc<bevy::prelude::Image>>) {
    if let Ok(mut guard) = PICTURES.lock() {
        guard.get_or_insert_with(Default::default).insert(key, image);
    }
}

fn picture_image((w, h, rgba): (u32, u32, Vec<u8>), srgb: bool) -> std::sync::Arc<bevy::prelude::Image> {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    std::sync::Arc::new(bevy::prelude::Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        rgba,
        if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm },
        bevy::asset::RenderAssetUsages::default(),
    ))
}

/// A picture read off the main thread once and kept by `key`: `None` while
/// it is being read (and for good when nothing was found).
pub(crate) fn picture_async(
    key: &str,
    srgb: bool,
    read: impl FnOnce() -> Option<(u32, u32, Vec<u8>)> + Send + 'static,
) -> Option<std::sync::Arc<bevy::prelude::Image>> {
    {
        let guard = PICTURES.lock().ok()?;
        if let Some(found) = guard.as_ref().and_then(|d| d.get(key)) {
            return found.clone();
        }
    }
    if !claim_picture(key) {
        return None;
    }
    let key = key.to_owned();
    READS_IN_FLIGHT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let spawned = std::thread::Builder::new()
        .name("ui-picture".into())
        .spawn(move || {
            struct Done;
            impl Drop for Done {
                fn drop(&mut self) {
                    READS_IN_FLIGHT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
            let _done = Done;
            let started = std::time::Instant::now();
            let (found, cached) = crate::picture_cache::read_through(&key, read);
            let image = found.map(|p| picture_image(p, srgb));
            diag::info!(
                Ui,
                "picture `{key}`: {} ({:.0} ms)",
                match (image.is_some(), cached) {
                    (true, true) => "from the cache",
                    (true, false) => "read from its game",
                    (false, true) => "not found (kept miss)",
                    (false, false) => "not found",
                },
                started.elapsed().as_secs_f64() * 1000.0
            );
            store_picture(key, image);
        });
    if spawned.is_err() {
        READS_IN_FLIGHT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
    None
}

/// Pictures being read from their games right now (a menu does not give
/// up on a picture while one is: the first read of a zone can be slow).
static READS_IN_FLIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub(crate) fn picture_reads_in_flight() -> usize {
    READS_IN_FLIGHT.load(std::sync::atomic::Ordering::SeqCst)
}

/// bo2mp: which other games' guns Create-a-Class lists (an installed game
/// with its multiplayer common zone; off with `BO2MP_GUEST_GUNS=0`).
pub(crate) fn guest_guns_available() {
    let started = std::time::Instant::now();
    let on = std::env::var("BO2MP_GUEST_GUNS").map_or(true, |v| v.trim() != "0");
    let root = asset_transport::games_root_from_env().ok();
    // (Black Ops' zone version, as its common_mp carries it.)
    let t5 = on && root.as_ref().is_some_and(|r| asset_transport::find_common_mp_for_envelope(r, 0x1D9).is_ok());
    // (Modern Warfare 2's zone version.)
    let iw4 = on && root.as_ref().is_some_and(|r| asset_transport::find_common_mp_for_envelope(r, 0x114).is_ok());
    bo2_profile::guest::set_available("t5", t5);
    bo2_profile::guest::set_available("iw4", iw4);
    let say = |listed: bool| if listed { "guns listed" } else if on { "not installed" } else { "off (BO2MP_GUEST_GUNS=0)" };
    diag::info!(Ui, "bo2mp guest guns: Black Ops {}, Modern Warfare 2 {}", say(t5), say(iw4));
    // (Modern Warfare 3's zone version is 1; World at War's and CoD4's are
    // `legacy_version`'s.)
    let legacy = |game| asset_transport::legacy_version(game).unwrap_or(u32::MAX);
    for (game, version, title) in
        [("iw5", 1, "Modern Warfare 3"), ("t4", legacy("t4"), "World at War"), ("iw3", legacy("iw3"), "Call of Duty 4")]
    {
        let listed = on && root.as_ref().is_some_and(|r| asset_transport::find_common_mp_for_envelope(r, version).is_ok());
        bo2_profile::guest::set_available(game, listed);
        diag::info!(Ui, "bo2mp guest guns: {title} {}", say(listed));
    }
    // (Ghosts: its zones are read by `fastfile_iw6`; its guns are in
    // common_core_mp.)
    let ghosts = on && ghosts_install(root.as_ref()).is_some_and(|i| i.join("zone").join("common_core_mp.ff").is_file());
    bo2_profile::guest::set_available("iw6", ghosts);
    diag::info!(Ui, "bo2mp guest guns: Ghosts {}", say(ghosts));
    // (Advanced Warfare: read by `fastfile_s1`; its guns are in common_mp.)
    let extra: Vec<std::path::PathBuf> = asset_transport::game_paths::get("aw").into_iter().collect();
    let aw = on && fastfile_s1::install::find_install(root.as_ref().map(|r| r.0.as_path()), &extra).is_some();
    bo2_profile::guest::set_available("s1", aw);
    diag::info!(Ui, "bo2mp guest guns: Advanced Warfare {}", say(aw));
    // (Horizon MW: its weapon zones under Modern Warfare Remastered's
    // `hmw-mod/zone`, read by `fastfile_h1`.)
    let hmw = on && asset_audio::hmw_zone_dir().is_some();
    bo2_profile::guest::set_available("h1", hmw);
    diag::info!(Ui, "bo2mp guest guns: Horizon MW {}", say(hmw));
    // (Infinite Warfare: its global_core_mp, common_core_mp and the guns'
    // transient zones, read by `fastfile_iw7`.)
    let iw7 = on && !iw7_parked() && asset_audio::iw7_zone_dir().is_some();
    bo2_profile::guest::set_available("iw7", iw7);
    diag::info!(Ui, "bo2mp guest guns: Infinite Warfare {}", say(iw7));
    // (Black Ops III: read by `fastfile_t7`; its guns are in core_common.)
    let bo3 = on
        && root.as_ref().is_some_and(|r| {
            asset_transport::find_zone_file_version(r, "core_common", asset_transport::T7_ZONE_VERSION).is_ok()
        });
    bo2_profile::guest::set_available("t7", bo3);
    diag::info!(Ui, "bo2mp guest guns: Black Ops III {}", say(bo3));
    // (Black Ops III Workshop weapon packs: their zone in their Workshop
    // folder, read with Black Ops III's, which they need.)
    for pack in &bo2_profile::guest::WORKSHOP_PACKS {
        let found = asset_transport::workshop::bo3_mod(pack.workshop_id).and_then(|m| m.zone(pack.zone).map(|_| m));
        let listed = bo3 && found.is_some();
        bo2_profile::guest::set_available(pack.game, listed);
        let why = match found {
            Some(m) if !bo3 => format!("not listed: Black Ops III is not installed ({})", m.dir.display()),
            Some(m) => format!("guns listed ({})", m.dir.display()),
            None if on => format!("not installed (Workshop item {} with {}.ff)", pack.workshop_id, pack.zone),
            None => say(false).to_owned(),
        };
        diag::info!(Ui, "bo2mp guest guns: {} pack {why}", pack.title);
    }
    // (Modern Warfare III (2023): read by `fastfile_jup` through the
    // install's Oodle DLL; the match builds its guns from their part zones
    // and `global_stream_mp`, as `asset_audio::jup_install` finds it.)
    let jup = on
        && crate::jup_cards::install().is_some_and(|i| {
            let zone = i.join("zone");
            // (A small copy of the install, `jup_pack`, has the guns'
            // zones but not Warzone's `ingame_br`.)
            zone.join("global_stream_mp.ff").is_file()
                && (zone.join("ingame_br.ff").is_file() || i.join(asset_audio::PACK_MARK).is_file())
                && fastfile_jup::oodle::dll_path(&i).is_file()
        });
    // (Without the game, the guns an exported folder holds: `iw4l
    // export-jup`, `BO2_JUP_EXPORT` or `MW3_export` in the usual places.
    // Create-a-Class lists those only.)
    let exported = if on && !jup { asset_transport::jup_export::dir() } else { None };
    bo2_profile::guest::set_jup_export_guns(exported.as_ref().map(|d| asset_transport::jup_export::guns(d)));
    let jup = jup || exported.as_ref().is_some_and(|d| !asset_transport::jup_export::guns(d).is_empty());
    if let Some(d) = &exported {
        diag::info!(Ui, "bo2mp guest guns: Modern Warfare III (2023) from the export {}", d.display());
    }
    bo2_profile::guest::set_available("jup", jup);
    diag::info!(Ui, "bo2mp guest guns: Modern Warfare III (2023) {}", say(jup));
    if jup {
        mwii_guns_report();
    }
    // (The mod's guns: its folder has its zone, `CODLEGACY_T6_MOD`. Its guns
    // leave out the other games' copies of the same guns.)
    let modded = asset_transport::t6_mod::zone().is_some();
    bo2_profile::guest::set_available(bo2_profile::guest::MOD_GAME, modded);
    match asset_transport::t6_mod::info().filter(|_| modded) {
        Some(m) => diag::info!(Ui, "bo2mp mod guns: {} {} by {} listed ({})", m.name, m.version, m.author, asset_transport::t6_mod::root().map_or_else(String::new, |r| r.display().to_string())),
        None => diag::info!(Ui, "bo2mp mod guns: no mod ({} unset and no {})", asset_transport::t6_mod::ENV, asset_transport::t6_mod::DEFAULT_FOLDER),
    }
    diag::info!(Ui, "bo2mp guest guns: found in {:.2} s", started.elapsed().as_secs_f32());
}

/// Whether a name is the picture a Black Ops III camo draws on a gun.
fn is_bo3_camo_picture(name: &str) -> bool {
    asset_game::weapon_camo_models::T7_CAMOS.iter().any(|c| c.4 == name)
}

/// bo2mp: Black Ops III's camo swatches (`menu_camo_*` in core_ui), read
/// once a run.
fn bo3_camo_swatches() -> Option<&'static std::collections::HashMap<String, (u32, u32, Vec<u8>)>> {
    static SWATCHES: std::sync::OnceLock<Option<std::collections::HashMap<String, (u32, u32, Vec<u8>)>>> =
        std::sync::OnceLock::new();
    SWATCHES
        .get_or_init(|| {
            let root = asset_transport::games_root_from_env().ok()?;
            let zone = asset_transport::find_zone_file_version(&root, "core_ui", asset_transport::T7_ZONE_VERSION).ok()?;
            match fastfile_t7::ui_image::read_images(&zone.path, |n| n.starts_with("menu_camo_"), 256) {
                Ok(list) => {
                    diag::info!(Ui, "bo2mp: {} Black Ops III camo swatches read", list.len());
                    Some(list.into_iter().map(|(n, w, h, px)| (n, (w, h, px))).collect())
                }
                Err(e) => {
                    diag::warn!(Ui, "bo2mp: Black Ops III camo swatches: {e}");
                    None
                }
            }
        })
        .as_ref()
}

/// bo2mp: Black Ops III's weapon icons (`t7_icon_weapon_*` in core_common)
/// and its camos' pictures, read once a run (the zone is big: it is walked
/// once, its icons kept).
fn bo3_icons() -> Option<&'static std::collections::HashMap<String, (u32, u32, Vec<u8>)>> {
    static ICONS: std::sync::OnceLock<Option<std::collections::HashMap<String, (u32, u32, Vec<u8>)>>> =
        std::sync::OnceLock::new();
    ICONS
        .get_or_init(|| {
            let root = asset_transport::games_root_from_env().ok()?;
            let zone = asset_transport::find_zone_file_version(&root, "core_common", asset_transport::T7_ZONE_VERSION).ok()?;
            match fastfile_t7::ui_image::read_images(&zone.path, |n| n.starts_with("t7_icon_weapon_") || is_bo3_camo_picture(n), 512) {
                Ok(list) => {
                    diag::info!(Ui, "bo2mp: {} Black Ops III weapon icons read", list.len());
                    Some(list.into_iter().map(|(n, w, h, px)| (n, (w, h, px))).collect())
                }
                Err(e) => {
                    diag::warn!(Ui, "bo2mp: Black Ops III weapon icons: {e}");
                    None
                }
            }
        })
        .as_ref()
}

/// bo2mp: a Black Ops III Workshop pack's gun pictures (the names its
/// roster's pictures try, in its zone), read once a run per pack.
fn workshop_pack_icons(pack: &'static bo2_profile::guest::WorkshopPack) -> Option<&'static std::collections::HashMap<String, (u32, u32, Vec<u8>)>> {
    type Icons = std::collections::HashMap<String, (u32, u32, Vec<u8>)>;
    static ICONS: std::sync::Mutex<Vec<(&'static str, Option<&'static Icons>)>> = std::sync::Mutex::new(Vec::new());
    let mut held = ICONS.lock().ok()?;
    if let Some((_, icons)) = held.iter().find(|(g, _)| *g == pack.game) {
        return *icons;
    }
    let wanted: std::collections::HashSet<String> = pack
        .guns
        .iter()
        .flat_map(|g| pack.picture(g.gun).split_once(':').map(|(_, n)| n.split('|').map(str::to_owned).collect::<Vec<_>>()).unwrap_or_default())
        .collect();
    let zone = asset_transport::workshop::bo3_mod(pack.workshop_id).and_then(|m| m.zone(pack.zone).map(std::path::Path::to_path_buf)).filter(|_| !wanted.is_empty());
    let icons = match zone.map(|z| fastfile_t7::ui_image::read_images(&z, |n| wanted.contains(n), 256)) {
        Some(Ok(list)) => {
            diag::info!(Ui, "bo2mp: {} {} gun pictures read", list.len(), pack.title);
            Some(&*Box::leak(Box::new(list.into_iter().map(|(n, w, h, px)| (n, (w, h, px))).collect::<Icons>())))
        }
        Some(Err(e)) => {
            diag::warn!(Ui, "bo2mp: {} gun pictures: {e}", pack.title);
            None
        }
        None => None,
    };
    held.push((pack.game, icons));
    icons
}

/// bo2mp: Black Ops III's map previews (`img_t7_menu_mp_preview_*` in
/// mp_common), read once a run.
fn bo3_map_previews() -> Option<&'static std::collections::HashMap<String, (u32, u32, Vec<u8>)>> {
    static PREVIEWS: std::sync::OnceLock<Option<std::collections::HashMap<String, (u32, u32, Vec<u8>)>>> =
        std::sync::OnceLock::new();
    PREVIEWS
        .get_or_init(|| {
            let root = asset_transport::games_root_from_env().ok()?;
            let zone = asset_transport::find_zone_file_version(&root, "mp_common", asset_transport::T7_ZONE_VERSION).ok()?;
            match fastfile_t7::ui_image::read_images(&zone.path, |n| n.starts_with("img_t7_menu_mp_preview_"), 1024) {
                Ok(list) => {
                    diag::info!(Ui, "bo2mp: {} Black Ops III map previews read", list.len());
                    Some(list.into_iter().map(|(n, w, h, px)| (n, (w, h, px))).collect())
                }
                Err(e) => {
                    diag::warn!(Ui, "bo2mp: Black Ops III map previews: {e}");
                    None
                }
            }
        })
        .as_ref()
}

/// bo2mp: the short name Black Ops III's menus give a map's pictures
/// (`img_t7_menu_mp_preview_<short>`, `img_t7_menu_mp_loadscreen_<short>`):
/// its zone name less `mp_`, else the code name its maptable (core_patch's
/// `maptable_mp_dlc<N>`) gives it.
fn bo3_picture_short(stem: &str) -> &str {
    let short = stem.strip_prefix("mp_").unwrap_or(stem);
    match short {
        "redwood" => "redwoods",
        "crucible" => "gauntlet",
        "waterpark" => "splash",
        "arena" => "rally",
        "ruins" => "citadel",
        "miniature" => "mirco",
        "western" => "outlaw",
        "city" => "rupture",
        other => other,
    }
}

/// bo2mp: Black Ops III's loading screens (`img_t7_menu_mp_loadscreen_*`,
/// 1920x1080, streamed from the `.xpak` packages) for its maps whose
/// previews mp_common lacks, read once a run from core_patch and made
/// 1600 wide; with why each one wanted did not decode.
struct Bo3Loadscreens {
    found: std::collections::HashMap<String, (u32, u32, Vec<u8>)>,
    failed: std::collections::HashMap<String, String>,
    zone: Result<(), String>,
}

fn bo3_loadscreens() -> &'static Bo3Loadscreens {
    static SHOTS: OnceLock<Bo3Loadscreens> = OnceLock::new();
    SHOTS.get_or_init(|| {
        let previews = bo3_map_previews();
        let wanted: std::collections::HashSet<String> = categories()
            .iter()
            .flat_map(|c| c.maps.iter())
            .filter(|m| m.game == "t7")
            .map(|m| bo3_picture_short(&m.stem).to_owned())
            .filter(|s| !previews.is_some_and(|p| p.contains_key(format!("img_t7_menu_mp_preview_{s}").as_str())))
            .map(|s| format!("img_t7_menu_mp_loadscreen_{s}"))
            .collect();
        let mut out = Bo3Loadscreens { found: Default::default(), failed: Default::default(), zone: Ok(()) };
        let zone = asset_transport::games_root_from_env()
            .map_err(|e| e.to_string())
            .and_then(|root| asset_transport::find_zone_file_version(&root, "core_patch", asset_transport::T7_ZONE_VERSION).map_err(|e| e.to_string()));
        let zone = match zone {
            Ok(z) => z,
            Err(e) => {
                out.zone = Err(format!("core_patch not found: {e}"));
                return out;
            }
        };
        match fastfile_t7::ui_image::read_images_report(&zone.path, |n| wanted.contains(n), 2048) {
            Ok((found, failed)) => {
                for (n, w, h, px) in found {
                    out.found.insert(n, fit_width((w, h, px), 1600));
                }
                out.failed.extend(failed);
            }
            Err(e) => out.zone = Err(e),
        }
        diag::info!(Ui, "bo2mp: {} Black Ops III loading screens read of {} wanted", out.found.len(), wanted.len());
        out
    })
}

/// A picture made `width` wide (Lanczos) when it is wider.
fn fit_width((w, h, rgba): (u32, u32, Vec<u8>), width: u32) -> (u32, u32, Vec<u8>) {
    if w <= width {
        return (w, h, rgba);
    }
    let Some(buf) = image::RgbaImage::from_raw(w, h, rgba.clone()) else {
        return (w, h, rgba);
    };
    let nh = (h as u64 * width as u64 / w as u64).max(1) as u32;
    (width, nh, image::imageops::resize(&buf, width, nh, image::imageops::FilterType::Lanczos3).into_raw())
}

/// bo2mp: a Black Ops III map's picture: its menu preview
/// (`img_t7_menu_mp_preview_<short>`, mp_common: the launch maps'), else
/// its loading screen (`img_t7_menu_mp_loadscreen_<short>`, core_patch:
/// Nuk3town's and the DLC maps', whose previews are only in the `.fd`
/// patch deltas), else its minimap from its own zone on a 16:9 card. One
/// line logged per map: what was tried and why it failed.
fn bo3_map_picture(stem: &str) -> Option<(u32, u32, Vec<u8>)> {
    let short = bo3_picture_short(stem);
    let preview = format!("img_t7_menu_mp_preview_{short}");
    let previews = bo3_map_previews();
    if let Some(found) = previews.and_then(|p| p.get(preview.as_str())) {
        diag::info!(Ui, "bo3 map picture: {stem}: `{preview}` (mp_common)");
        return Some(found.clone());
    }
    let why_preview = if previews.is_some() { "not in mp_common" } else { "mp_common unreadable" };
    let shot = format!("img_t7_menu_mp_loadscreen_{short}");
    let shots = bo3_loadscreens();
    if let Some(found) = shots.found.get(shot.as_str()) {
        diag::info!(Ui, "bo3 map picture: {stem}: `{preview}` {why_preview}; `{shot}` (core_patch)");
        return Some(found.clone());
    }
    let why_shot = match &shots.zone {
        Err(e) => e.clone(),
        Ok(()) => shots.failed.get(shot.as_str()).cloned().unwrap_or_else(|| "not in core_patch".into()),
    };
    match bo3_minimap_card(stem) {
        Ok((name, found)) => {
            diag::warn!(Ui, "bo3 map picture: {stem}: `{preview}` {why_preview}; `{shot}`: {why_shot}; its minimap `{name}` instead");
            Some(found)
        }
        Err(e) => {
            diag::warn!(Ui, "bo3 map picture: {stem}: `{preview}` {why_preview}; `{shot}`: {why_shot}; minimap: {e}; no picture");
            None
        }
    }
}

/// bo2mp: a Black Ops III map's minimap (`i_compass_map_<map>_t7`, else
/// `_full`, in the map's own zone) centred on a dark 1600x900 card.
fn bo3_minimap_card(stem: &str) -> Result<(String, (u32, u32, Vec<u8>)), String> {
    let root = asset_transport::games_root_from_env().map_err(|e| e.to_string())?;
    let zone = asset_transport::find_zone_file_version(&root, stem, asset_transport::T7_ZONE_VERSION).map_err(|e| e.to_string())?;
    let names = [format!("i_compass_map_{stem}_t7"), format!("i_compass_map_{stem}_full")];
    let (found, failed) = fastfile_t7::ui_image::read_images_report(&zone.path, |n| names.iter().any(|w| w == n), 2048)?;
    let Some((name, w, h, px)) = names.iter().find_map(|n| found.iter().find(|f| &f.0 == n)).cloned() else {
        let why: Vec<String> = failed.iter().map(|(n, e)| format!("{n}: {e}")).collect();
        return Err(if why.is_empty() { format!("none of {} in {stem}", names.join(", ")) } else { why.join("; ") });
    };
    if image::RgbaImage::from_raw(w, h, px.clone()).is_none() {
        return Err("bad minimap size".into());
    }
    Ok((name, minimap_card((w, h, px))))
}

/// bo2mp: a minimap centred on a dark 1600x900 card (the map picker's
/// fallback when a map has no preview or loading screen).
pub(crate) fn minimap_card((w, h, px): (u32, u32, Vec<u8>)) -> (u32, u32, Vec<u8>) {
    let Some(src) = image::RgbaImage::from_raw(w, h, px.clone()) else {
        return (w, h, px);
    };
    let (cw, ch) = (1600u32, 900u32);
    let side = ch - 60;
    let scale = side as f32 / w.max(h).max(1) as f32;
    let (sw, sh) = (((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1));
    let small = image::imageops::resize(&src, sw, sh, image::imageops::FilterType::Lanczos3);
    let mut card = image::RgbaImage::from_pixel(cw, ch, image::Rgba([16, 18, 22, 255]));
    image::imageops::overlay(&mut card, &small, ((cw - sw) / 2) as i64, ((ch - sh) / 2) as i64);
    (cw, ch, card.into_raw())
}

/// bo2mp: a Ghosts map's picture: a menu picture naming it (ui_mp and the
/// other menu zones; the old lookup kept only gun pictures, so every
/// Ghosts tile was blank), else its `<map>_load` zone's loading screen,
/// else its minimap (`compass_map_<map>`) on a card. One line per map.
fn ghosts_map_picture(stem: &str) -> Option<(u32, u32, Vec<u8>)> {
    let Some(install) = ghosts_install(asset_transport::games_root_from_env().ok().as_ref()) else {
        diag::warn!(Ui, "ghosts map picture: {stem}: no Ghosts install found (game_paths.cfg `ghosts=`); no picture");
        return None;
    };
    match fastfile_iw6::ui_image::read_map_picture(&install, stem) {
        Ok((name, minimap, p)) => {
            if minimap {
                diag::warn!(Ui, "ghosts map picture: {stem}: no menu picture or loading screen; {name} instead");
                Some(minimap_card(p))
            } else {
                diag::info!(Ui, "ghosts map picture: {stem}: {name}");
                Some(p)
            }
        }
        Err(e) => {
            diag::warn!(Ui, "ghosts map picture: {stem}: {e}; no picture");
            None
        }
    }
}

/// bo2mp: where Ghosts is installed (`fastfile_iw6::install`'s search, with
/// game_paths.cfg's `ghosts=`).
fn ghosts_install(root: Option<&asset_transport::GamesRoot>) -> Option<std::path::PathBuf> {
    let extra: Vec<std::path::PathBuf> = asset_transport::game_paths::get("ghosts").into_iter().collect();
    fastfile_iw6::install::find_install(root.map(|r| r.0.as_path()), &extra)
}

/// The namespace of a game prefix (`iw4` -> MW2).
fn namespace_of(prefix: &str) -> Option<asset_core::AssetNamespace> {
    Some(match prefix {
        "iw4" => asset_core::AssetNamespace::Iw4,
        "iw5" => asset_core::AssetNamespace::Iw5,
        "t5" => asset_core::AssetNamespace::T5,
        _ => return None,
    })
}

/// An image from a game's own IWDs (the first of `names` it has).
pub(crate) fn read_game_image(prefix: &str, names: &[String]) -> Option<(u32, u32, Vec<u8>)> {
    // bo2mp: the mod's Create-a-Class pictures (`CODLEGACY_T6_MOD`): its
    // archive's, else Black Ops II's packs by name.
    if prefix == bo2_profile::guest::MOD_GAME {
        let zone_all = asset_transport::games_root_from_env()
            .ok()
            .and_then(|root| asset_transport::find_zone_file(&root, "t6:patch_mp").ok())
            .and_then(|z| z.path.parent().map(std::path::Path::to_path_buf));
        return names.iter().find_map(|n| assets::t6_mod_picture_rgba(n, zone_all.as_deref()));
    }
    // bo2mp: Modern Warfare III's (2023) emblems and calling cards: its
    // `global` zone's names, their pictures in its packages.
    if prefix == crate::jup_cards::PREFIX {
        // (A map's pictures: its loading screen's, else the menus' key art.)
        let mwii = |n: &str| n.strip_prefix("loadscreen_").or_else(|| n.strip_prefix("preview_")).is_some_and(|m| asset_transport::is_mwii_map(m.strip_suffix("_jup").unwrap_or(m)));
        if names.iter().any(|n| n.starts_with("loadscreen_mp_jup_") || n.starts_with("preview_mp_jup_") || mwii(n)) {
            return crate::jup_hud::map_picture(names);
        }
        // (A gun's: its Create-a-Class picture, in global_stream_mp.)
        if names.iter().any(|n| n.starts_with("icon_weapon_") || n.starts_with("hud_icon_weapon_") || n.starts_with("ui_jup_camo_") || n.starts_with("ui_camo_")) {
            return crate::jup_hud::gun_picture(names);
        }
        return crate::jup_cards::read_picture(names);
    }
    // bo2mp: the owner's own calling cards and emblem, built in.
    if prefix == crate::custom_cards::PREFIX {
        return crate::custom_cards::read_picture(names);
    }
    // bo2mp: Black Ops II's own calling cards and emblems, from its packs.
    if prefix == crate::bo_cards::BO2 {
        return names.iter().find_map(|n| crate::bo_cards::read_bo2(n));
    }
    // bo2mp: Ghosts' pictures are in its zones (ui_mp), not in IWDs.
    if prefix == "iw6" {
        let install = ghosts_install(asset_transport::games_root_from_env().ok().as_ref())?;
        return fastfile_iw6::ui_image::read_ui_image(&install, names);
    }
    // bo2mp: Infinite Warfare's are in its ui.ff (and its image paks): the
    // map previews `image_mp_map_<map>`, the Zombies posters.
    #[cfg(feature = "iw7")]
    if prefix == "iw7" {
        // (Its guns' Create-a-Class pictures: their HUD icons in its
        // global_core_mp.)
        if names.iter().any(|n| n.starts_with("hud_icon_") || n.starts_with("icon_obit_")) {
            return fastfile_iw7::ui_image::read_gun_icon(&asset_audio::iw7_zone_dir()?, names);
        }
        let root = asset_transport::games_root_from_env().ok()?;
        let ui = asset_transport::find_zone_file_version(&root, "ui", asset_transport::IW7_ZONE_VERSION).ok()?;
        let dir = ui.path.parent()?.to_path_buf();
        let wanted: Vec<String> = names.iter().flat_map(|n| iw7_picture_names(n)).collect();
        return fastfile_iw7::ui_image::read_ui_image(&dir, &wanted);
    }
    // bo2mp: Black Ops III's weapon icons are in its core_common (and its
    // `.xpak` packages).
    if prefix == "t7" {
        // (A map's: its menu preview in mp_common, else its loading
        // screen in core_patch, else its minimap: `bo3_map_picture`.)
        if let Some(stem) = names.iter().find_map(|n| n.strip_prefix("loadscreen_").or_else(|| n.strip_prefix("preview_"))) {
            return bo3_map_picture(stem);
        }
        // (A camo's: its menu swatch in core_ui, else the picture it draws
        // on a gun, in core_common: `weapon_camo_models::T7_CAMOS`.)
        if let Some(n) = names.iter().find(|n| n.starts_with("menu_camo_")) {
            return bo3_camo_swatches()?.get(n.as_str()).cloned();
        }
        if let Some(n) = names.iter().find(|n| is_bo3_camo_picture(n)) {
            return bo3_icons()?.get(n.as_str()).cloned();
        }
        if !names.iter().any(|n| n.starts_with("t7_icon_weapon_")) {
            return None;
        }
        let icons = bo3_icons()?;
        return names.iter().find_map(|n| icons.get(n.as_str()).cloned());
    }
    // bo2mp: a Black Ops III Workshop pack's are its kill icons in its zone.
    if let Some(pack) = bo2_profile::guest::workshop_pack(prefix) {
        let icons = workshop_pack_icons(pack)?;
        return names.iter().find_map(|n| icons.get(n.as_str()).cloned());
    }
    // bo2mp: Horizon MW's are its guns' HUD icons in its weapon zones.
    if prefix == "h1" {
        // (A map's: its loading picture, in its loading zone.)
        if let Some(stem) = names.iter().find_map(|n| n.strip_prefix("loadscreen_").or_else(|| n.strip_prefix("preview_"))) {
            let zone = h1_map_zone(stem)?;
            let retail = h1_retail_zone_dir(&zone);
            let mut wanted = vec![format!("loadscreen_{stem}")];
            wanted.extend(names.iter().cloned());
            return fastfile_h1::ui_image::read_map_picture(&zone, retail.as_deref(), &wanted);
        }
        let dir = asset_audio::hmw_zone_dir()?;
        // (A killstreak's: its D-pad icon, in the killstreak zones.)
        if names.iter().any(|n| n.trim_start_matches(',').contains("killstreak")) {
            return fastfile_h1::ui_image::read_hmw_killstreak_icon(&dir, names);
        }
        return fastfile_h1::ui_image::read_hmw_icon(&dir, names);
    }
    // bo2mp: Advanced Warfare's are in its common_mp (and its image paks).
    if prefix == "s1" {
        let root = asset_transport::games_root_from_env().ok();
        let extra: Vec<std::path::PathBuf> = asset_transport::game_paths::get("aw").into_iter().collect();
        let install = fastfile_s1::install::find_install(root.as_ref().map(|r| r.0.as_path()), &extra)?;
        return fastfile_s1::ui_image::read_ui_image(&install, names);
    }
    let main = game_main(prefix)?;
    names
        .iter()
        .find_map(|n| asset_material::decode_ui_image_from_main(&main, n).ok().flatten())
}

/// bo2mp: a Modern Warfare Remastered / Horizon MW map's zone (the retail
/// `zone/<map>.ff` or a usermap's `hmw-usermaps/<map>/<map>.ff`), the games'
/// folders listed once a run.
fn h1_map_zone(stem: &str) -> Option<std::path::PathBuf> {
    static ZONES: OnceLock<Vec<(String, std::path::PathBuf)>> = OnceLock::new();
    ZONES
        .get_or_init(|| {
            let Ok(root) = asset_transport::games_root_from_env() else {
                return Vec::new();
            };
            asset_transport::list_legacy_mp_map_files(&root)
                .into_iter()
                .filter(|(g, _, _)| g == "h1")
                .map(|(_, s, p)| (s, p))
                .collect()
        })
        .iter()
        .find(|(s, _)| s == stem)
        .map(|(_, p)| p.clone())
}

/// The retail `zone` folder (its `imagefile<N>.pak`) for an h1 map zone:
/// the zone's own folder, else the install's `zone` above a usermap's.
fn h1_retail_zone_dir(zone: &std::path::Path) -> Option<std::path::PathBuf> {
    zone.ancestors().skip(1).take(4).find_map(|d| {
        if d.join("common_mp.ff").is_file() {
            Some(d.to_path_buf())
        } else {
            Some(d.join("zone")).filter(|z| z.join("common_mp.ff").is_file())
        }
    })
}

/// A game's `main` folder (its IWDs), found once and kept: finding it walks
/// the games' folders (every game's zones), which made each of Modern
/// Warfare 2's and 3's calling cards and emblems slow to come in (a picker's
/// page asks for dozens at once, each walking them again).
fn game_main(prefix: &str) -> Option<std::path::PathBuf> {
    static MAINS: std::sync::Mutex<Option<std::collections::HashMap<String, Option<std::path::PathBuf>>>> = std::sync::Mutex::new(None);
    // (Held while it is found: pictures asked for together wait for the one
    // walk rather than each walking.)
    let mut mains = MAINS.lock().ok()?;
    let mains = mains.get_or_insert_with(Default::default);
    if let Some(found) = mains.get(prefix) {
        return found.clone();
    }
    let root = asset_transport::games_root_from_env().ok()?;
    let main = match asset_transport::legacy_version(prefix) {
        // Call of Duty 4 / World at War: the `main` beside their zones.
        Some(version) => asset_transport::find_zone_file_version(&root, "common_mp", version)
            .ok()
            .and_then(|common| asset_transport::game_root_for_zone(&common.path).ok())
            .map(|r| r.join("main")),
        None => {
            let trees = asset_transport::NamespaceTrees::discover(&root);
            namespace_of(prefix).and_then(|ns| trees.main_for(ns)).map(std::path::Path::to_path_buf)
        }
    };
    diag::info!(Ui, "picture: {prefix}'s main is {main:?}");
    mains.insert(prefix.to_owned(), main.clone());
    main
}

/// bo2mp shell: the names of every image a game's own IWDs hold that start
/// with `starts` (lower case, sorted; empty when the game is not installed).
/// Slow the first time (the archives are indexed): call it off the main
/// thread.
pub(crate) fn game_image_names(prefix: &str, starts: &str) -> Vec<String> {
    let Some(main) = game_main(prefix) else {
        return Vec::new();
    };
    let Ok(index) = asset_transport::IwdIndex::open(&main) else { return Vec::new() };
    let mut out: Vec<String> = index.image_names().filter(|n| n.starts_with(starts)).map(str::to_owned).collect();
    out.sort();
    out
}

/// bo2mp: Infinite Warfare's pictures for a name the menus ask for
/// (`loadscreen_<map>`, `preview_<map>`): its map preview (named for the
/// map's code name, mostly the zone name) or its Zombies poster.
fn iw7_picture_names(name: &str) -> Vec<String> {
    let map = name
        .strip_prefix("loadscreen_")
        .or_else(|| name.strip_prefix("preview_"))
        .or_else(|| name.strip_suffix("_load"))
        .unwrap_or(name);
    if let Some(cp) = map.strip_prefix("cp_") {
        return vec![format!("zm_theater_poster_{cp}")];
    }
    let code = map.strip_prefix("mp_").unwrap_or(map);
    // (ui.ff names the pictures by the maps' titles where the zone names
    // differ: Mayday is mp_breakneck, Breakout mp_parkour.)
    let code = match code {
        "breakneck" => "mayday",
        "parkour" => "breakout",
        "codphish" => "altitude",
        "rally" | "carnage2" => "carnage",
        "dome_iw" => "dome",
        "desert" => "grounded",
        "hawkwar" => "heartland",
        "divide" => "scorch",
        "rivet" => "skydock",
        "skyway" => "terminus",
        "renaissance2" => "geneva",
        "permafrost2" => "overflow",
        "turista2" => "marsoasis",
        other => other,
    };
    vec![format!("image_mp_map_{code}"), format!("image_mp_map_{code}_vote")]
}

/// A prestige icon from another game where BO2's menus ask for one: the
/// combined prestiges' `<game>:<material>` (bo2_profile's progression),
/// read from that game's own files, the first of its candidate names it
/// has (logged, as the names are not all known for certain).
pub(crate) fn prestige_picture(name: &str) -> Option<std::sync::Arc<bevy::prelude::Image>> {
    let (game, names) = bo2_profile::progression::foreign_picture(name)?;
    let key = name.to_owned();
    picture_async(name, true, move || {
        // (A name may be another game's: Horizon MW's guns try the original
        // game's picture first, `iw4:weapon_cheytac`.)
        let found = names.iter().find_map(|n| {
            let (from, name) = bo2_profile::progression::picture_source(game, n);
            read_game_image(from, &[name.to_owned()]).map(|img| (n, img))
        });
        // (Black Ops' and Black Ops II's emblems are shapes.)
        let found = found.map(|(n, img)| (n, if game == crate::bo_cards::BO1 || game == crate::bo_cards::BO2 { crate::bo_cards::shape_to_cover(img) } else { img }));
        match &found {
            Some((n, _)) => diag::info!(Ui, "bo2mp progression: `{key}` is {game}'s `{n}`"),
            None => diag::warn!(Ui, "bo2mp progression: `{key}`: {game} has none of {}", names.join(", ")),
        }
        found.map(|(_, img)| img)
    })
}

/// A foreign map's picture where BO2's menus ask for one: its map-select
/// picture (`menu_<map>_map_select_final`), its loading picture
/// (`loadscreen_<map>`) or its preview, read from the map's own game (its
/// loading screen, else its preview). All of a map's names share one read,
/// which `warm_map_pictures` has usually done already.
pub(crate) fn map_picture(material: &str) -> Option<std::sync::Arc<bevy::prelude::Image>> {
    let ui_name = material
        .strip_prefix("menu_")
        .and_then(|m| m.strip_suffix("_map_select_final"))
        .or_else(|| material.strip_prefix("loadscreen_"))
        .or_else(|| material.strip_prefix("menu_mp_map_select_").and_then(|m| m.strip_suffix("_big")))?;
    // (Not waited for on a frame: `None` while the list is found, from the
    // HDD, in the background; the picture is asked for again.)
    let Some(found) = CATEGORIES.get() else {
        warm_categories();
        return None;
    };
    let map = found.iter().flat_map(|c| c.maps.iter()).find(|m| m.ui_name == ui_name)?.clone();
    picture_async(&map_key(&map), true, move || read_map_picture_gently(&map))
}

/// A map's picture read from its game one at a time, at background disk
/// priority: another game's zone is a long read off the HDD, and a menu
/// full of tiles asked for dozens at once (92 MB/s through his Black Ops II
/// matches). The cache's are quick either way.
fn read_map_picture_gently(map: &ForeignMap) -> Option<(u32, u32, Vec<u8>)> {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    asset_audio::background_io(true);
    let found = read_map_picture(map);
    asset_audio::background_io(false);
    found
}

/// Black Ops II's own multiplayer maps (its mapsTable), whose names another
/// game's map must not take.
const BO2_MAPS: &[&str] = &[
    "mp_la", "mp_dockside", "mp_carrier", "mp_drone", "mp_express", "mp_hijacked", "mp_meltdown", "mp_overflow",
    "mp_nightclub", "mp_raid", "mp_slums", "mp_village", "mp_turbine", "mp_socotra", "mp_nuketown_2020",
    "mp_downhill", "mp_mirage", "mp_hydro", "mp_skate", "mp_concert", "mp_magma", "mp_vertigo", "mp_studio",
    "mp_uplink", "mp_bridge", "mp_castaway", "mp_paintball", "mp_dig", "mp_frostbite", "mp_pod", "mp_takeoff",
];

/// bo2mp: whether a community map's zone name is one of Black Ops II's own
/// maps, alone or with a remaster's ending (`mp_slums`, `mp_raid_hd`).
/// bo2mp: Black Ops III Workshop maps that recreate a Black Ops map as it
/// was: (Workshop zone, Black Ops zone). They replace the original in the
/// map list (the owner, 11-05; Summit is his example). Reimaginings are not
/// here.
const BO1_RECREATIONS: &[(&str, &str)] = &[("mp_mountain", "mp_mountain"), ("mp_firingrange", "mp_firingrange"), ("mp_jungle", "mp_havoc")];

fn is_bo2_remake(stem: &str) -> bool {
    let stem = stem.to_ascii_lowercase();
    let base = ["_remastered", "_remaster", "_hd", "_rm", "_bo2", "_t6"].iter().find_map(|end| stem.strip_suffix(end)).unwrap_or(&stem);
    BO2_MAPS.contains(&base)
}

fn map_key(map: &ForeignMap) -> String {
    format!("map:{}", map.ui_name)
}

/// Where a map's picture is kept once read (the games' archives are slow to
/// search; this is read in a few milliseconds).
fn map_picture_cache(map: &ForeignMap) -> std::path::PathBuf {
    std::path::Path::new("codlegacy-artifacts/cache/map-pictures").join(format!("{}_{}.png", map.game, map.stem))
}

/// A map's picture: from the cache, else from its game (then cached), made
/// bigger off the main thread.
fn read_map_picture(map: &ForeignMap) -> Option<(u32, u32, Vec<u8>)> {
    // bo2mp: a Black Ops III Workshop map's own picture (its loading image,
    // else its preview or Workshop picture, in its item folder).
    if map.game == "t7"
        && let Some(custom) = asset_transport::workshop::bo3_map(&map.stem)
    {
        let found = custom.pictures.iter().find_map(|p| match read_picture_file(p) {
            Ok(found) => Some(found),
            Err(e) => {
                diag::warn!(Ui, "bo3 workshop: {} picture {}: {e}", custom.stem, p.display());
                None
            }
        });
        if let Some(found) = found {
            return Some(upscale_map_shot(found));
        }
    }
    let cache = map_picture_cache(map);
    let small = image::open(&cache).ok().map(|i| i.to_rgba8()).map(|i| (i.width(), i.height(), i.into_raw())).or_else(|| {
        let found = if map.game == "iw6" {
            ghosts_map_picture(&map.stem)?
        } else {
            read_game_image(
                map.game,
                &[format!("loadscreen_{}", map.stem), format!("preview_{}", map.stem), format!("{}_load", map.stem)],
            )?
        };
        if let Some(img) = image::RgbaImage::from_raw(found.0, found.1, found.2.clone()) {
            let _ = std::fs::create_dir_all(cache.parent()?);
            if let Err(e) = img.save(&cache) {
                diag::warn!(Ui, "map picture cache: {}: {e}", cache.display());
            }
        }
        Some(found)
    })?;
    Some(upscale_map_shot(small))
}

/// A PNG or JPEG file's pixels (RGBA).
pub(crate) fn read_picture_file(path: &std::path::Path) -> Result<(u32, u32, Vec<u8>), String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if ext == "png" {
        let img = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
        return Ok((img.width(), img.height(), img.into_raw()));
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let img = bevy::prelude::Image::from_buffer(
        &bytes,
        bevy::image::ImageType::Extension(&ext),
        bevy::image::CompressedImageFormats::NONE,
        true,
        bevy::image::ImageSampler::Default,
        bevy::asset::RenderAssetUsages::default(),
    )
    .map_err(|e| e.to_string())?;
    let (w, h) = (img.width(), img.height());
    let data = img.data.ok_or("no pixels")?;
    if data.len() == (w * h * 4) as usize { Ok((w, h, data)) } else { Err("not 8-bit RGBA".into()) }
}

/// A small map shot (256-1023 px wide, wider than tall) made about 1600 px
/// wide with a Lanczos filter and a light unsharp mask, so it stays clean
/// full-screen and in the big map card.
pub(crate) fn upscale_map_shot((w, h, rgba): (u32, u32, Vec<u8>)) -> (u32, u32, Vec<u8>) {
    if !(128..1024).contains(&w) || h >= 1024 || (w as f32) < h as f32 * 1.3 {
        return (w, h, rgba);
    }
    let Some(buf) = image::RgbaImage::from_raw(w, h, rgba.clone()) else {
        return (w, h, rgba);
    };
    let scale = (1600.0 / w as f32).clamp(1.0, 4.0);
    let (bw, bh) = ((w as f32 * scale) as u32, (h as f32 * scale) as u32);
    let big = image::imageops::resize(&buf, bw, bh, image::imageops::FilterType::Lanczos3);
    let sharp = image::imageops::unsharpen(&big, 1.2, 4);
    (bw, bh, sharp.into_raw())
}

/// Reads every foreign map's picture already in the picture cache in the
/// background, one after another, so the map picker's tiles are in by the
/// time it opens (once a run). One not cached yet is read from its game
/// when a menu shows it (`map_picture`): reading them all from the other
/// games here kept the HDD busy for 135 s, through his matches.
pub(crate) fn warm_map_pictures() {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("ui-map-pictures".into())
        .spawn(|| {
            let started = std::time::Instant::now();
            let maps: Vec<ForeignMap> = categories().iter().flat_map(|c| c.maps.iter().cloned()).collect();
            let (mut found, mut cached) = (0, 0);
            for map in &maps {
                let key = map_key(map);
                if !map_picture_cache(map).is_file() {
                    continue;
                }
                cached += 1;
                if !claim_picture(&key) {
                    continue;
                }
                let image = read_map_picture(map).map(|p| picture_image(p, true));
                found += usize::from(image.is_some());
                store_picture(key, image);
            }
            diag::info!(
                Ui,
                "bo2mp shell: map pictures warmed: {found} read of {} maps ({cached} from the cache) in {:.1}s",
                maps.len(),
                started.elapsed().as_secs_f32()
            );
        })
        .ok();
}

#[cfg(test)]
mod h1_map_picture_probe {
    /// Every Modern Warfare Remastered / Horizon MW map under `CODLEGACY_GAMES`
    /// has its loading picture: `CODLEGACY_GAMES=<dir> cargo test -p ui --lib
    /// h1_maps_have_pictures -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs the games under CODLEGACY_GAMES"]
    fn h1_maps_have_pictures() {
        let root = asset_transport::games_root_from_env().expect("CODLEGACY_GAMES");
        let maps: Vec<String> = asset_transport::list_legacy_mp_map_files(&root)
            .into_iter()
            .filter(|(g, _, _)| g == "h1")
            .map(|(_, s, _)| s)
            .collect();
        assert!(!maps.is_empty(), "no h1 maps");
        let mut missing = Vec::new();
        for m in &maps {
            match super::read_game_image("h1", &[format!("loadscreen_{m}"), format!("preview_{m}")]) {
                Some((w, h, _)) => println!("{m}: {w}x{h}"),
                None => missing.push(m.clone()),
            }
        }
        assert!(missing.is_empty(), "no picture: {missing:?}");
    }
}

#[cfg(test)]
mod bo3_map_pictures {
    /// Black Ops III's DLC maps' pictures go by their maptable code names
    /// (core_patch's `maptable_mp_dlc<N>`), not their zone names.
    #[test]
    fn dlc_maps_use_their_maptable_picture_names() {
        for (stem, short) in [
            ("mp_redwood", "redwoods"),
            ("mp_crucible", "gauntlet"),
            ("mp_waterpark", "splash"),
            ("mp_arena", "rally"),
            ("mp_ruins", "citadel"),
            ("mp_shrine", "shrine"),
            ("mp_miniature", "mirco"),
            ("mp_western", "outlaw"),
            ("mp_city", "rupture"),
            ("mp_nuketown_x", "nuketown_x"),
            ("mp_skyjacked", "skyjacked"),
        ] {
            assert_eq!(super::bo3_picture_short(stem), short, "{stem}");
        }
    }

    /// Every Black Ops III map the picker lists has a picture under
    /// `CODLEGACY_GAMES` (preview, loading screen or minimap card):
    /// `CODLEGACY_GAMES=<dir> cargo test -p ui --lib bo3_maps_have_pictures --
    /// --ignored --nocapture`.
    #[test]
    #[ignore = "needs Black Ops III under CODLEGACY_GAMES"]
    fn bo3_maps_have_pictures() {
        let only: Option<Vec<String>> = std::env::var("BO3_MAPS").ok().map(|v| v.split(',').map(str::to_owned).collect());
        let mut missing = Vec::new();
        for (stem, _) in super::GAMES.iter().filter(|g| g.prefix == "t7").flat_map(|g| g.base.iter().chain(g.dlc.iter())) {
            if only.as_ref().is_some_and(|o| !o.iter().any(|m| m == stem)) {
                continue;
            }
            match super::bo3_map_picture(stem) {
                Some((w, h, _)) => println!("{stem}: {w}x{h}"),
                None => missing.push(*stem),
            }
        }
        assert!(missing.is_empty(), "no picture: {missing:?}");
    }
}

#[cfg(test)]
mod bo2_names {
    #[test]
    fn other_games_maps_never_take_a_bo2_maps_name() {
        // BO2's Standoff and MW3's Village are both `mp_village`; BO2's and
        // BO1's Studio both `mp_studio`.
        for stem in ["mp_village", "mp_studio", "mp_nuketown_2020"] {
            assert!(super::BO2_MAPS.contains(&stem), "{stem}");
        }
        let mut sorted = super::BO2_MAPS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), super::BO2_MAPS.len());
    }

    #[test]
    fn workshop_remakes_of_bo2_maps_are_known() {
        // His Workshop's Slums (item 3802778645) is BO2's own map: not listed.
        assert!(super::is_bo2_remake("mp_slums"));
        assert!(super::is_bo2_remake("MP_RAID_HD"));
        // BO1's, MW3's and others' remakes are.
        for stem in ["mp_firingrange", "mp_jungle", "mp_mountain", "mp_dome", "mp_dome_night", "mp_shoothouse18", "mp_nuketown_x"] {
            assert!(!super::is_bo2_remake(stem), "{stem}");
        }
    }
}

#[cfg(test)]
mod map_intro_music {
    /// Every map the menus list has its own line in the map intro music
    /// table (`audio/src/music/map_intro.cfg`); a map added to a list
    /// without one would play its game's default.
    #[test]
    fn every_listed_map_has_an_intro_track_line() {
        let mut missing = Vec::new();
        for stem in super::BO2_MAPS {
            if !audio::music::map_intro::builtin_has("t6", stem) {
                missing.push(format!("t6:{stem}"));
            }
        }
        for game in super::GAMES {
            let maps = game.base.iter().chain(game.dlc.iter());
            // (Modern Warfare II's maps play as Modern Warfare III's.)
            let prefix = if game.prefix == super::MWII_PREFIX { "jup" } else { game.prefix };
            for (stem, _) in maps {
                if !audio::music::map_intro::builtin_has(prefix, stem) {
                    missing.push(format!("{prefix}:{stem}"));
                }
            }
        }
        for (stem, _) in super::IW4X {
            if !audio::music::map_intro::builtin_has("iw4", stem) {
                missing.push(format!("iw4:{stem}"));
            }
        }
        assert!(missing.is_empty(), "no map_intro.cfg line: {missing:?}");
    }
}

