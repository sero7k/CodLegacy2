//! bo2mp: Modern Warfare 2's and 3's weapon camos. Their game draws a camo
//! as another model of the gun (WeaponDef gunXModel / worldModel slots
//! 1-15: `viewmodel_m4_woodland`), the same shape with the camo's
//! materials. The weapon capture notes each gun's camo models here; the
//! match load pairs them with the gun's own models into material swaps
//! (`asset_core::weapon_camos`).

use std::collections::HashMap;
use std::sync::Mutex;

use asset_core::AssetNamespace;

/// A gun's camos: (slot, view model, world model).
pub type CamoModels = Vec<(u8, Option<String>, Option<String>)>;

/// (game, gun's view model) -> (its world model, its camos).
static NOTED: Mutex<Option<HashMap<(AssetNamespace, String), (Option<String>, CamoModels)>>> = Mutex::new(None);

/// A gun's camo models as its weapon capture read them.
pub fn note(ns: AssetNamespace, gun: &str, world: Option<String>, camos: CamoModels) {
    if camos.is_empty() {
        return;
    }
    if let Ok(mut n) = NOTED.lock() {
        n.get_or_insert_with(HashMap::new).insert((ns, gun.to_owned()), (world, camos));
    }
}

/// Every gun noted: (game, view model, world model, camos).
#[must_use]
pub fn all() -> Vec<(AssetNamespace, String, Option<String>, CamoModels)> {
    NOTED
        .lock()
        .map(|n| n.iter().flatten().map(|((ns, gun), (world, camos))| (*ns, gun.clone(), world.clone(), camos.clone())).collect())
        .unwrap_or_default()
}

/// Modern Warfare 2's camos by slot (its WeaponDef model slots 1-8), and
/// the menu picture of each.
pub const MW2_CAMOS: [(u8, &str, &str); 8] = [
    (1, "Woodland", "weapon_camo_menu_woodland"),
    (2, "Desert", "weapon_camo_menu_desert"),
    (3, "Arctic", "weapon_camo_menu_arctic"),
    (4, "Digital", "weapon_camo_menu_digital"),
    (5, "Urban", "weapon_camo_menu_red_urban"),
    (6, "Red Tiger", "weapon_camo_menu_red_tiger"),
    (7, "Blue Tiger", "weapon_camo_menu_blue_tiger"),
    (8, "Fall", "weapon_camo_menu_orange_fall"),
];

/// Modern Warfare 3's camos by slot (its WeaponDef model slots 1-13).
pub const MW3_CAMOS: [(u8, &str, &str); 13] = [
    (1, "Classic", "weapon_camo_menu_classic"),
    (2, "Snow", "weapon_camo_menu_snow"),
    (3, "Multicam", "weapon_camo_menu_multi"),
    (4, "Digital Urban", "weapon_camo_menu_d_urban"),
    (5, "Hex", "weapon_camo_menu_hex"),
    (6, "Choco", "weapon_camo_menu_choco"),
    (7, "Snake", "weapon_camo_menu_snake"),
    (8, "Blue", "weapon_camo_menu_blue"),
    (9, "Red", "weapon_camo_menu_red"),
    (10, "Autumn", "weapon_camo_menu_autumn"),
    (11, "Gold", "weapon_camo_menu_gold"),
    (12, "Marine", "weapon_camo_menu_marine"),
    (13, "Winter", "weapon_camo_menu_winter"),
];

/// Black Ops' camos (counted from 1): name, its camo picture, its menu
/// swatch.
pub const BO1_CAMOS: [(u8, &str, &str, &str); 14] = [
    (1, "Dusty", "camo_dusty", "menu_mp_weapons_camo_dusty"),
    (2, "Ice", "camo_ice", "menu_mp_weapons_camo_icy"),
    (3, "Olive", "camo_od", "menu_mp_weapons_camo_olive"),
    (4, "Nevada", "camo_desert_nevada", "menu_mp_weapons_camo_nevada"),
    (5, "Sahara", "camo_desert_sahara", "menu_mp_weapons_camo_sahara"),
    (6, "ERDL", "camo_jungle_erdl", "menu_mp_weapons_camo_erdl"),
    (7, "Tiger", "camo_jungle_tiger", "menu_mp_weapons_camo_tiger"),
    (8, "Berlin", "camo_urban_german", "menu_mp_weapons_camo_berlin"),
    (9, "Warsaw", "camo_urban_warsaw", "menu_mp_weapons_camo_warsaw"),
    (10, "Siberia", "camo_winter_siberia", "menu_mp_weapons_camo_siberia"),
    (11, "Yukon", "camo_winter_yukon", "menu_mp_weapons_camo_yukon"),
    (12, "Woodland", "camo_woodland", "menu_mp_weapons_camo_wood"),
    (13, "Flora", "camo_woodland_flora", "menu_mp_weapons_camo_flora"),
    (14, "Gold", "camo_gold_c", "menu_mp_weapons_camo_gold"),
];
/// bo2mp: Modern Warfare III's (2023) camos, by their Create-a-Class
/// pictures in its `global_stream_mp` (`ui_jup_camo_<family><variant>`):
/// camo `n` (counted from 0) is `weapon_camos::JUP_FIRST + n`, each drawn
/// as its picture laid over a whole gun material, on every gun.
pub const JUP_CAMOS: [&str; 164] = [
    "ui_jup_camo_02a", "ui_jup_camo_02b", "ui_jup_camo_02c", "ui_jup_camo_02d",
    "ui_jup_camo_02e", "ui_jup_camo_02f", "ui_jup_camo_03a", "ui_jup_camo_03b",
    "ui_jup_camo_03c", "ui_jup_camo_03d", "ui_jup_camo_03e", "ui_jup_camo_03f",
    "ui_jup_camo_03g", "ui_jup_camo_03h", "ui_jup_camo_03p", "ui_jup_camo_06a",
    "ui_jup_camo_06b", "ui_jup_camo_06c", "ui_jup_camo_06d", "ui_jup_camo_06e",
    "ui_jup_camo_06f", "ui_jup_camo_06g", "ui_jup_camo_06h", "ui_jup_camo_06i",
    "ui_jup_camo_06j", "ui_jup_camo_07a", "ui_jup_camo_07b", "ui_jup_camo_07c",
    "ui_jup_camo_07d", "ui_jup_camo_07e", "ui_jup_camo_07f", "ui_jup_camo_07g",
    "ui_jup_camo_07h", "ui_jup_camo_07i", "ui_jup_camo_07j", "ui_jup_camo_08a",
    "ui_jup_camo_09a", "ui_jup_camo_09b", "ui_jup_camo_09c", "ui_jup_camo_09d",
    "ui_jup_camo_09e", "ui_jup_camo_09f", "ui_jup_camo_09g", "ui_jup_camo_09h",
    "ui_jup_camo_09i", "ui_jup_camo_10a", "ui_jup_camo_10b", "ui_jup_camo_10c",
    "ui_jup_camo_10d", "ui_jup_camo_10e", "ui_jup_camo_10f", "ui_jup_camo_10g",
    "ui_jup_camo_10h", "ui_jup_camo_10i", "ui_jup_camo_10j", "ui_jup_camo_10k",
    "ui_jup_camo_10l", "ui_jup_camo_11a", "ui_jup_camo_11b", "ui_jup_camo_11c",
    "ui_jup_camo_11d", "ui_jup_camo_11e", "ui_jup_camo_11f", "ui_jup_camo_11g",
    "ui_jup_camo_11h", "ui_jup_camo_11i", "ui_jup_camo_11j", "ui_jup_camo_11k",
    "ui_jup_camo_11l", "ui_jup_camo_11m", "ui_jup_camo_11n", "ui_jup_camo_13a",
    "ui_jup_camo_13b", "ui_jup_camo_13c", "ui_jup_camo_13d", "ui_jup_camo_13e",
    "ui_jup_camo_13f", "ui_jup_camo_13h", "ui_jup_camo_13i", "ui_jup_camo_13j",
    "ui_jup_camo_13k", "ui_jup_camo_13l", "ui_jup_camo_13m", "ui_jup_camo_14a",
    "ui_jup_camo_14b", "ui_jup_camo_14c", "ui_jup_camo_14d", "ui_jup_camo_14e",
    "ui_jup_camo_14f", "ui_jup_camo_14g", "ui_jup_camo_14h", "ui_jup_camo_14i",
    "ui_jup_camo_14j", "ui_jup_camo_14k", "ui_jup_camo_14l", "ui_jup_camo_15a",
    "ui_jup_camo_16a", "ui_jup_camo_16b", "ui_jup_camo_16c", "ui_jup_camo_16d",
    "ui_jup_camo_16e", "ui_jup_camo_16f", "ui_jup_camo_16g", "ui_jup_camo_16h",
    "ui_jup_camo_16i", "ui_jup_camo_16j", "ui_jup_camo_16k", "ui_jup_camo_16l",
    "ui_jup_camo_16m", "ui_jup_camo_17a", "ui_jup_camo_17b", "ui_jup_camo_17c",
    "ui_jup_camo_17d", "ui_jup_camo_17e", "ui_jup_camo_17f", "ui_jup_camo_17g",
    "ui_jup_camo_18a", "ui_jup_camo_18b", "ui_jup_camo_18c", "ui_jup_camo_18d",
    "ui_jup_camo_18e", "ui_jup_camo_19a", "ui_jup_camo_19b", "ui_jup_camo_19c",
    "ui_jup_camo_19d", "ui_jup_camo_19e", "ui_jup_camo_19f", "ui_jup_camo_19g",
    "ui_jup_camo_19h", "ui_jup_camo_19i", "ui_jup_camo_19j", "ui_jup_camo_19k",
    "ui_jup_camo_19l", "ui_jup_camo_19m", "ui_jup_camo_20b", "ui_jup_camo_20f",
    "ui_jup_camo_20g", "ui_jup_camo_20h", "ui_jup_camo_21a", "ui_jup_camo_22a",
    "ui_jup_camo_22b", "ui_jup_camo_22c", "ui_jup_camo_22d", "ui_jup_camo_22e",
    "ui_jup_camo_22f", "ui_jup_camo_22g", "ui_jup_camo_22h", "ui_jup_camo_23a",
    "ui_jup_camo_23d", "ui_jup_camo_23e", "ui_jup_camo_23f", "ui_jup_camo_24a",
    "ui_jup_camo_24b", "ui_jup_camo_24c", "ui_jup_camo_24d", "ui_jup_camo_24e",
    "ui_jup_camo_24f", "ui_jup_camo_24h", "ui_jup_camo_24j", "ui_jup_camo_25a",
    "ui_jup_camo_25b", "ui_jup_camo_25c", "ui_jup_camo_26a", "ui_jup_camo_26b",
];

/// bo2mp: Modern Warfare II's (2022) camos, as Modern Warfare III carries
/// their pictures (`ui_camo_<series>_<n>`): camo `n` is
/// `weapon_camos::IW9_FIRST + n`.
pub const IW9_CAMOS: [&str; 331] = [
    "ui_camo_a_01", "ui_camo_a_02", "ui_camo_a_03", "ui_camo_a_04",
    "ui_camo_a_05", "ui_camo_a_06", "ui_camo_a_07", "ui_camo_a_08",
    "ui_camo_a_09", "ui_camo_a_10", "ui_camo_a_11", "ui_camo_a_12",
    "ui_camo_a_13", "ui_camo_a_14", "ui_camo_a_15", "ui_camo_b_01",
    "ui_camo_b_02", "ui_camo_b_03", "ui_camo_b_04", "ui_camo_b_05",
    "ui_camo_b_06", "ui_camo_b_07", "ui_camo_b_08", "ui_camo_b_09",
    "ui_camo_b_10", "ui_camo_b_11", "ui_camo_b_12", "ui_camo_b_13",
    "ui_camo_b_14", "ui_camo_b_15", "ui_camo_c_01", "ui_camo_c_02",
    "ui_camo_c_03", "ui_camo_c_04", "ui_camo_c_05", "ui_camo_c_06",
    "ui_camo_c_07", "ui_camo_c_08", "ui_camo_c_09", "ui_camo_c_10",
    "ui_camo_c_11", "ui_camo_c_12", "ui_camo_c_13", "ui_camo_c_14",
    "ui_camo_c_15", "ui_camo_d_01", "ui_camo_d_02", "ui_camo_d_03",
    "ui_camo_d_04", "ui_camo_d_05", "ui_camo_d_06", "ui_camo_d_07",
    "ui_camo_d_08", "ui_camo_d_09", "ui_camo_d_10", "ui_camo_d_11",
    "ui_camo_d_12", "ui_camo_d_13", "ui_camo_d_14", "ui_camo_d_15",
    "ui_camo_e_01", "ui_camo_e_02", "ui_camo_e_03", "ui_camo_e_04",
    "ui_camo_e_05", "ui_camo_e_06", "ui_camo_e_07", "ui_camo_e_08",
    "ui_camo_e_09", "ui_camo_e_10", "ui_camo_e_11", "ui_camo_e_12",
    "ui_camo_e_13", "ui_camo_e_14", "ui_camo_e_15", "ui_camo_f_01",
    "ui_camo_f_02", "ui_camo_f_03", "ui_camo_f_04", "ui_camo_f_05",
    "ui_camo_f_06", "ui_camo_f_07", "ui_camo_f_08", "ui_camo_f_09",
    "ui_camo_f_10", "ui_camo_f_11", "ui_camo_f_12", "ui_camo_f_13",
    "ui_camo_f_14", "ui_camo_f_15", "ui_camo_g_01", "ui_camo_g_02",
    "ui_camo_g_03", "ui_camo_g_04", "ui_camo_g_05", "ui_camo_g_06",
    "ui_camo_g_07", "ui_camo_g_08", "ui_camo_g_09", "ui_camo_g_10",
    "ui_camo_g_11", "ui_camo_g_12", "ui_camo_g_13", "ui_camo_g_14",
    "ui_camo_g_15", "ui_camo_h_01", "ui_camo_h_02", "ui_camo_h_03",
    "ui_camo_h_04", "ui_camo_h_05", "ui_camo_h_06", "ui_camo_h_07",
    "ui_camo_h_08", "ui_camo_h_09", "ui_camo_h_10", "ui_camo_h_11",
    "ui_camo_h_12", "ui_camo_h_13", "ui_camo_h_14", "ui_camo_h_15",
    "ui_camo_i_01", "ui_camo_i_02", "ui_camo_i_03", "ui_camo_i_04",
    "ui_camo_i_05", "ui_camo_i_06", "ui_camo_i_07", "ui_camo_i_08",
    "ui_camo_i_09", "ui_camo_i_10", "ui_camo_i_11", "ui_camo_i_12",
    "ui_camo_i_13", "ui_camo_i_14", "ui_camo_i_15", "ui_camo_j_01",
    "ui_camo_j_02", "ui_camo_j_03", "ui_camo_j_04", "ui_camo_j_05",
    "ui_camo_j_06", "ui_camo_j_07", "ui_camo_j_08", "ui_camo_j_09",
    "ui_camo_j_10", "ui_camo_j_11", "ui_camo_j_12", "ui_camo_j_13",
    "ui_camo_j_14", "ui_camo_j_15", "ui_camo_k_01", "ui_camo_k_02",
    "ui_camo_k_03", "ui_camo_k_04", "ui_camo_k_05", "ui_camo_k_06",
    "ui_camo_k_07", "ui_camo_k_08", "ui_camo_k_09", "ui_camo_k_10",
    "ui_camo_k_11", "ui_camo_k_12", "ui_camo_k_13", "ui_camo_k_14",
    "ui_camo_k_15", "ui_camo_l_01", "ui_camo_l_02", "ui_camo_l_03",
    "ui_camo_l_04", "ui_camo_l_05", "ui_camo_l_06", "ui_camo_l_07",
    "ui_camo_l_08", "ui_camo_l_09", "ui_camo_l_10", "ui_camo_l_11",
    "ui_camo_l_12", "ui_camo_l_13", "ui_camo_l_14", "ui_camo_l_15",
    "ui_camo_m_01", "ui_camo_m_02", "ui_camo_m_03", "ui_camo_m_04",
    "ui_camo_m_05", "ui_camo_m_06", "ui_camo_m_07", "ui_camo_m_08",
    "ui_camo_m_09", "ui_camo_m_10", "ui_camo_m_11", "ui_camo_m_12",
    "ui_camo_m_13", "ui_camo_m_14", "ui_camo_m_15", "ui_camo_n_01",
    "ui_camo_n_02", "ui_camo_n_03", "ui_camo_n_04", "ui_camo_n_05",
    "ui_camo_n_06", "ui_camo_n_07", "ui_camo_n_08", "ui_camo_n_09",
    "ui_camo_n_10", "ui_camo_n_11", "ui_camo_n_12", "ui_camo_n_13",
    "ui_camo_n_14", "ui_camo_n_15", "ui_camo_o_01", "ui_camo_o_02",
    "ui_camo_o_03", "ui_camo_o_04", "ui_camo_o_05", "ui_camo_o_06",
    "ui_camo_o_07", "ui_camo_o_08", "ui_camo_o_09", "ui_camo_o_10",
    "ui_camo_o_11", "ui_camo_o_12", "ui_camo_o_13", "ui_camo_o_14",
    "ui_camo_o_15", "ui_camo_p_01", "ui_camo_p_02", "ui_camo_p_03",
    "ui_camo_p_04", "ui_camo_p_05", "ui_camo_p_06", "ui_camo_p_07",
    "ui_camo_p_08", "ui_camo_p_09", "ui_camo_p_10", "ui_camo_p_11",
    "ui_camo_p_12", "ui_camo_p_13", "ui_camo_p_14", "ui_camo_p_15",
    "ui_camo_q_01", "ui_camo_q_02", "ui_camo_q_03", "ui_camo_q_04",
    "ui_camo_q_05", "ui_camo_q_06", "ui_camo_q_07", "ui_camo_q_08",
    "ui_camo_q_09", "ui_camo_q_10", "ui_camo_q_11", "ui_camo_q_12",
    "ui_camo_q_13", "ui_camo_q_14", "ui_camo_q_15", "ui_camo_r_01",
    "ui_camo_r_02", "ui_camo_r_03", "ui_camo_r_04", "ui_camo_r_05",
    "ui_camo_r_06", "ui_camo_r_07", "ui_camo_r_08", "ui_camo_r_09",
    "ui_camo_r_10", "ui_camo_r_11", "ui_camo_r_12", "ui_camo_r_13",
    "ui_camo_r_14", "ui_camo_s_01", "ui_camo_s_02", "ui_camo_s_03",
    "ui_camo_s_04", "ui_camo_s_05", "ui_camo_s_06", "ui_camo_s_07",
    "ui_camo_s_08", "ui_camo_s_09", "ui_camo_s_10", "ui_camo_s_11",
    "ui_camo_s_12", "ui_camo_s_13", "ui_camo_s_14", "ui_camo_s_15",
    "ui_camo_t_01", "ui_camo_t_02", "ui_camo_t_03", "ui_camo_t_04",
    "ui_camo_t_05", "ui_camo_t_06", "ui_camo_t_07", "ui_camo_t_08",
    "ui_camo_t_09", "ui_camo_t_10", "ui_camo_t_11", "ui_camo_t_12",
    "ui_camo_t_13", "ui_camo_t_14", "ui_camo_t_15", "ui_camo_u_01",
    "ui_camo_u_02", "ui_camo_u_03", "ui_camo_u_04", "ui_camo_u_05",
    "ui_camo_u_06", "ui_camo_u_07", "ui_camo_u_08", "ui_camo_u_09",
    "ui_camo_u_10", "ui_camo_u_11", "ui_camo_u_12", "ui_camo_u_13",
    "ui_camo_u_14", "ui_camo_u_15", "ui_camo_v_01", "ui_camo_v_02",
    "ui_camo_v_03", "ui_camo_v_04", "ui_camo_v_05", "ui_camo_v_06",
    "ui_camo_v_07", "ui_camo_v_08", "ui_camo_v_09", "ui_camo_v_10",
    "ui_camo_v_11", "ui_camo_v_12", "ui_camo_v_13", "ui_camo_v_14",
    "ui_camo_v_15", "ui_camo_w_01", "ui_camo_w_02",
];

/// bo2mp: the Create-a-Class names of `JUP_CAMOS` and `IW9_CAMOS`, index
/// for index. Each camo's own name is a localized string Modern Warfare
/// III ships encrypted (`JUP_CAMOS/JUP_CAMO_02A`, `CAMOS/CAMO_A_01`: the
/// values of its `eng_*` localize entries are ciphered), so a camo is named
/// by its category, which its `global_stream_mp` bundle names in the clear
/// (`jup_camo_category_02_frogs`, `camo_category_spray` ->
/// `CAMOS/CATEGORY_SPRAY_PAINT`), and its place in it. Modern Warfare II's
/// eleven camos of the category `CAMOS/CATEGORY_O` (no name in the clear)
/// keep their picture's (`MWII K-04`). See docs/JUP.md, "Camos".
pub const JUP_CAMO_NAMES: [&str; 164] = [
    "FROGS 1", "FROGS 2", "FROGS 3", "FROGS 4", "FROGS 5", "FROGS 6", "GRAPHICAL 1",
    "GRAPHICAL 2", "GRAPHICAL 3", "GRAPHICAL 4", "GRAPHICAL 5", "GRAPHICAL 6", "GRAPHICAL 7",
    "GRAPHICAL 8", "GRAPHICAL 9", "SKULLS 1", "SKULLS 2", "SKULLS 3", "SKULLS 4", "SKULLS 5",
    "SKULLS 6", "SKULLS 7", "SKULLS 8", "SKULLS 9", "SKULLS 10", "TOPOGRAPHIC 1",
    "TOPOGRAPHIC 2", "TOPOGRAPHIC 3", "TOPOGRAPHIC 4", "TOPOGRAPHIC 5", "TOPOGRAPHIC 6",
    "TOPOGRAPHIC 7", "TOPOGRAPHIC 8", "TOPOGRAPHIC 9", "TOPOGRAPHIC 10", "LINES 1", "GLITCHY 1",
    "GLITCHY 2", "GLITCHY 3", "GLITCHY 4", "GLITCHY 5", "GLITCHY 6", "GLITCHY 7", "GLITCHY 8",
    "GLITCHY 9", "GLITTERY 1", "GLITTERY 2", "GLITTERY 3", "GLITTERY 4", "GLITTERY 5",
    "GLITTERY 6", "GLITTERY 7", "GLITTERY 8", "GLITTERY 9", "GLITTERY 10", "GLITTERY 11",
    "GLITTERY 12", "EIGHT BIT 1", "EIGHT BIT 2", "EIGHT BIT 3", "EIGHT BIT 4", "EIGHT BIT 5",
    "EIGHT BIT 6", "EIGHT BIT 7", "EIGHT BIT 8", "EIGHT BIT 9", "EIGHT BIT 10", "EIGHT BIT 11",
    "EIGHT BIT 12", "EIGHT BIT 13", "EIGHT BIT 14", "NEAPOLITAN 1", "NEAPOLITAN 2",
    "NEAPOLITAN 3", "NEAPOLITAN 4", "NEAPOLITAN 5", "NEAPOLITAN 6", "NEAPOLITAN 7",
    "NEAPOLITAN 8", "NEAPOLITAN 9", "NEAPOLITAN 10", "NEAPOLITAN 11", "NEAPOLITAN 12",
    "BACTERIA 1", "BACTERIA 2", "BACTERIA 3", "BACTERIA 4", "BACTERIA 5", "BACTERIA 6",
    "BACTERIA 7", "BACTERIA 8", "BACTERIA 9", "BACTERIA 10", "BACTERIA 11", "BACTERIA 12",
    "FORMATIONS 1", "CONTRAST 1", "CONTRAST 2", "CONTRAST 3", "CONTRAST 4", "CONTRAST 5",
    "CONTRAST 6", "CONTRAST 7", "CONTRAST 8", "CONTRAST 9", "CONTRAST 10", "CONTRAST 11",
    "CONTRAST 12", "CONTRAST 13", "EDM 1", "EDM 2", "EDM 3", "EDM 4", "EDM 5", "EDM 6", "EDM 7",
    "BRUSHSTROKES 1", "BRUSHSTROKES 2", "BRUSHSTROKES 3", "BRUSHSTROKES 4", "BRUSHSTROKES 5",
    "WATERCOLORS 1", "WATERCOLORS 2", "WATERCOLORS 3", "WATERCOLORS 4", "WATERCOLORS 5",
    "WATERCOLORS 6", "WATERCOLORS 7", "WATERCOLORS 8", "WATERCOLORS 9", "WATERCOLORS 10",
    "WATERCOLORS 11", "WATERCOLORS 12", "WATERCOLORS 13", "PSYCHEDELICS 1", "PSYCHEDELICS 2",
    "PSYCHEDELICS 3", "PSYCHEDELICS 4", "GRADIENTS 1", "BLUR 1", "BLUR 2", "BLUR 3", "BLUR 4",
    "BLUR 5", "BLUR 6", "BLUR 7", "BLUR 8", "BOKEH 1", "BOKEH 2", "BOKEH 3", "BOKEH 4",
    "BUTTERFLY 1", "BUTTERFLY 2", "BUTTERFLY 3", "BUTTERFLY 4", "BUTTERFLY 5", "BUTTERFLY 6",
    "BUTTERFLY 7", "BUTTERFLY 8", "WILD 1", "WILD 2", "WILD 3", "CRITTERS 1", "CRITTERS 2",
];
/// `IW9_CAMOS`' names (see `JUP_CAMO_NAMES`).
pub const IW9_CAMO_NAMES: [&str; 331] = [
    "SPRAY PAINT 1", "SPRAY PAINT 2", "SPRAY PAINT 3", "SPRAY PAINT 4", "SPRAY PAINT 5",
    "SPRAY PAINT 6", "SPRAY PAINT 7", "SPRAY PAINT 8", "SPRAY PAINT 9", "SPRAY PAINT 10",
    "SPRAY PAINT 11", "SPRAY PAINT 12", "SPRAY PAINT 13", "SPRAY PAINT 14", "SPRAY PAINT 15",
    "WOODLAND 1", "WOODLAND 2", "WOODLAND 3", "WOODLAND 4", "WOODLAND 5", "WOODLAND 6",
    "WOODLAND 7", "WOODLAND 8", "WOODLAND 9", "WOODLAND 10", "WOODLAND 11", "WOODLAND 12",
    "WOODLAND 13", "WOODLAND 14", "WOODLAND 15", "DIGITAL 1", "DIGITAL 2", "DIGITAL 3",
    "DIGITAL 4", "DIGITAL 5", "DIGITAL 6", "DIGITAL 7", "DIGITAL 8", "DIGITAL 9", "DIGITAL 10",
    "DIGITAL 11", "DIGITAL 12", "DIGITAL 13", "DIGITAL 14", "DIGITAL 15", "SPECIAL 1",
    "DRAGON 1", "DRAGON 2", "DRAGON 3", "DRAGON 4", "DRAGON 5", "DRAGON 6", "DRAGON 7",
    "DRAGON 8", "DRAGON 9", "DRAGON 10", "DRAGON 11", "DRAGON 12", "DRAGON 13", "DRAGON 14",
    "GEOMETRIC 1", "GEOMETRIC 2", "GEOMETRIC 3", "GEOMETRIC 4", "GEOMETRIC 5", "GEOMETRIC 6",
    "GEOMETRIC 7", "GEOMETRIC 8", "GEOMETRIC 9", "GEOMETRIC 10", "GEOMETRIC 11", "GEOMETRIC 12",
    "GEOMETRIC 13", "GEOMETRIC 14", "GEOMETRIC 15", "EVENTS 1", "DRAGON 15", "FUN 1",
    "GEOMETRIC 16", "FUN 2", "FUN 3", "FUN 4", "DRAGON 16", "WOODLAND 16", "FUN 5",
    "WOODLAND 17", "SPRAY PAINT 16", "FOLIAGE 1", "FOLIAGE 2", "SKULLS 1", "TIGER 1", "TIGER 2",
    "TIGER 3", "TIGER 4", "TIGER 5", "TIGER 6", "TIGER 7", "TIGER 8", "TIGER 9", "TIGER 10",
    "TIGER 11", "TIGER 12", "TIGER 13", "TIGER 14", "TIGER 15", "STRIPES 1", "STRIPES 2",
    "STRIPES 3", "STRIPES 4", "STRIPES 5", "STRIPES 6", "STRIPES 7", "STRIPES 8", "STRIPES 9",
    "STRIPES 10", "STRIPES 11", "STRIPES 12", "REPTILE 1", "STRIPES 13", "STRIPES 14",
    "REPTILE 2", "REPTILE 3", "REPTILE 4", "REPTILE 5", "REPTILE 6", "REPTILE 7", "REPTILE 8",
    "REPTILE 9", "REPTILE 10", "REPTILE 11", "REPTILE 12", "REPTILE 13", "REPTILE 14",
    "SPRAY PAINT 17", "STRIPES 15", "SKULLS 2", "SKULLS 3", "SKULLS 4", "SKULLS 5", "SOLID 1",
    "STRIPES 16", "FUN 6", "SPRAY PAINT 18", "FUN 7", "FUN 8", "FUN 9", "FUN 10", "DIGITAL 16",
    "FUN 11", "REPTILE 15", "SOLID 2", "SOLID 3", "SOLID 4", "SOLID 5", "SOLID 6", "SOLID 7",
    "SOLID 8", "SOLID 9", "SOLID 10", "SOLID 11", "SOLID 12", "SOLID 13", "SOLID 14",
    "SOLID 15", "SOLID 16", "CLASSIC 1", "CLASSIC 2", "CLASSIC 3", "CLASSIC 4", "CLASSIC 5",
    "CLASSIC 6", "DIGITAL 17", "DIGITAL 18", "DIGITAL 19", "DIGITAL 20", "DIGITAL 21",
    "DIGITAL 22", "DIGITAL 23", "DIGITAL 24", "DIGITAL 25", "REPTILE 16", "WOODLAND 18",
    "TIGER 16", "FOLIAGE 3", "DRAGON 17", "FUN 12", "FOLIAGE 4", "WOODLAND 19", "FUN 13",
    "FUN 14", "FUN 15", "WOODLAND 20", "REPTILE 17", "SPRAY PAINT 19", "FUN 16", "FOLIAGE 5",
    "FOLIAGE 6", "FOLIAGE 7", "FOLIAGE 8", "FOLIAGE 9", "FOLIAGE 10", "FOLIAGE 11", "EVENTS 2",
    "EVENTS 3", "TIGER 17", "FUN 17", "DIGITAL 26", "GEOMETRIC 17", "EVENTS 4", "EVENTS 5",
    "MWII O-01", "MWII O-02", "MWII O-03", "MWII O-04", "MWII O-05", "MWII O-06", "MWII O-07",
    "MWII O-08", "MWII O-09", "MWII O-10", "FUN 18", "FUN 19", "STRIPES 17", "SPRAY PAINT 20",
    "MWII O-15", "SPRAY PAINT 21", "GEOMETRIC 18", "SKULLS 6", "FUN 20", "SPRAY PAINT 22",
    "FUN 21", "FUN 22", "EVENTS 6", "FUN 23", "SPRAY PAINT 23", "TIGER 18", "FUN 24",
    "DIGITAL 27", "FUN 25", "DRAGON 18", "FUN 26", "EVENTS 7", "EVENTS 8", "EVENTS 9",
    "EVENTS 10", "SPECIAL 2", "EVENTS 11", "EVENTS 12", "EVENTS 13", "SPRAY PAINT 24",
    "DRAGON 19", "DIGITAL 28", "STRIPES 18", "SPRAY PAINT 25", "GEOMETRIC 19", "RANKED 1",
    "RANKED 2", "RANKED 3", "RANKED 4", "RANKED 5", "RANKED 6", "RANKED 7", "EVENTS 14",
    "EVENTS 15", "RANKED 8", "RANKED 9", "STRIPES 19", "EVENTS 16", "EVENTS 17", "FUN 27",
    "FUN 28", "FUN 29", "STRIPES 20", "DIGITAL 29", "WOODLAND 21", "GEOMETRIC 20",
    "WOODLAND 22", "WOODLAND 23", "FUN 30", "FUN 31", "EVENTS 18", "EVENTS 19", "EVENTS 20",
    "EVENTS 21", "FOLIAGE 12", "SPRAY PAINT 26", "DIGITAL 30", "FUN 32", "FOLIAGE 13",
    "GEOMETRIC 21", "FUN 33", "SKULLS 7", "SPRAY PAINT 27", "WOODLAND 24", "GEOMETRIC 22",
    "FUN 34", "SPRAY PAINT 28", "FUN 35", "TIGER 19", "FUN 36", "RANKED 10", "RANKED 11",
    "RANKED 12", "RANKED 13", "RANKED 14", "RANKED 15", "RANKED 16", "RANKED 17", "RANKED 18",
    "RANKED 19", "RANKED 20", "RANKED 21", "EVENTS 22", "WOODLAND 25", "SPRAY PAINT 29",
    "FUN 37", "FOLIAGE 14", "FUN 38", "DIGITAL 31", "GEOMETRIC 23", "FUN 39", "FUN 40",
    "SPECIAL 3", "GEOMETRIC 24", "FUN 41", "FUN 42", "GEOMETRIC 25", "DIGITAL 32",
    "WOODLAND 26", "FUN 43", "EVENTS 23",
];

/// bo2mp: Black Ops III's camos: (its camo number in Create-a-Class, name,
/// menu swatch, camo material, the material's picture drawn). The number
/// and name are the "camo" rows of its
/// `gamedata/weapons/common/attachmenttable.csv` (core_patch: row order
/// from `camo_none` = 0; the `MPUI_CAMO_*` key's English text), the swatch
/// is in core_ui, the material is the guns' camo sets' (`camo_t7_<gun>` in
/// core_common, slot from weaponoptions.csv). The picture is the
/// material's colour map, else a metal camo's specular colour (Red Hex,
/// Etching; Gold's is in another zone: core_common's gold picture), else
/// Dark Matter's glow. Camo `n` is `weapon_camos::T7_FIRST + n`, drawn as
/// its picture over a whole gun material on every gun. Only the camos
/// whose material core_common holds (the base game's); the loot and DLC
/// camos' materials are in core_patch, which the walk does not read.
pub const T7_CAMOS: [(u8, &str, &str, &str, &str); 27] = [
    (1, "Jungle Tech", "menu_camo_jungletek_pattern", "mc/mtl_t7_camo_jungletek", "i_t7_camo_jungletek_c"),
    (2, "Ash", "menu_camo_ash_pattern", "mc/mtl_t7_camo_ashcraft", "i_t7_camo_ashcraft_c"),
    (3, "Flectarn", "menu_camo_flectarn_pattern", "mc/mtl_t7_camo_flecktarn", "i_t7_camo_flecktarn_c"),
    (4, "Heat Stroke", "menu_camo_heatstroke_pattern", "mc/mtl_t7_camo_heatstroke", "i_t7_camo_heatstroke_c"),
    (5, "Snow Job", "menu_camo_snowjob_pattern", "mc/mtl_t7_camo_snowblo", "i_t7_camo_snowblo_c"),
    (6, "Dante", "menu_camo_dante_pattern", "mc/mtl_t7_camo_dante", "i_t7_camo_dante_c"),
    (7, "Integer", "menu_camo_integer_pattern", "mc/mtl_t7_camo_integer", "i_t7_camo_integer_c"),
    (8, "6 Speed", "menu_camo_speed6_pattern", "mc/mtl_t7_camo_speed6", "i_t7_camo_speed6_c"),
    (9, "Policia", "menu_camo_interpol_pattern", "mc/mtl_t7_camo_sideways", "i_t7_camo_sideways_c"),
    (10, "Ardent", "menu_camo_ardent_pattern", "mc/mtl_wpn_t7_camo_ardent", "i_mtl_wpn_t7_camo_ardent_c"),
    (11, "Burnt", "menu_camo_burnt_pattern", "mc/mtl_t7_camo_burnt", "i_t7_camo_burnt_c"),
    (12, "Bliss", "menu_camo_bliss_pattern", "mc/mtl_t7_camo_bliss", "i_t7_camo_bliss_c"),
    (13, "Battle", "menu_camo_battle_pattern", "mc/mtl_t7_camo_artofwar_battle", "i_t7_camo_artofwar_c"),
    (14, "Chameleon", "menu_camo_chameleon_pattern", "mc/mtl_t7_camo_chameleon", "i_t7_camo_chameleon_c"),
    (15, "Gold", "menu_camo_gold_pattern", "mc/mtl_wpn_t7_camo_gold", "i_wpn_t7_camo_gold_alt_c"),
    (16, "Diamond", "menu_camo_diamond_pattern", "mc/mtl_t7_camo_diamond_inlay", "i_t7_camo_diamond_inlay_c"),
    (17, "Dark Matter", "menu_camo_darkmatter_pattern", "mc/mtl_wpn_t7_camo_darkmater_epic", "i_mtl_mtl_wpn_t7_camo_inifinity_e2"),
    (18, "Arctic", "menu_camo_arctic_pattern", "mc/mtl_t7_camo_arctic", "i_t7_camo_arctic_c"),
    (19, "Jungle", "menu_camo_jungle_pattern", "mc/mtl_t7_camo_jungle", "i_t7_camo_jungle_c"),
    (20, "Huntsman", "menu_camo_huntsman_pattern", "mc/mtl_t7_camo_huntsman", "i_t7_camo_huntsman_c"),
    (21, "Woodlums", "menu_camo_woodlums_pattern", "mc/mtl_t7_camo_woodlums", "i_t7_camo_woodlums_c"),
    (22, "Contagious", "menu_camo_contagious_pattern", "mc/mtl_t7_camo_contagious", "i_t7_camo_contagious_c"),
    (23, "Fear", "menu_camo_fear_pattern", "mc/mtl_t7_camo_fear", "i_t7_camo_fear_c"),
    (24, "WMD", "menu_camo_wmd_pattern", "mc/mtl_t7_camo_wmd", "i_t7_camo_wmd_c"),
    (25, "Red Hex", "menu_camo_redhex_pattern", "mc/mtl_wpn_t7_camo_red_anodized_hex", "i_wpn_t7_camo_red_anodized_s"),
    (26, "Ritual", "menu_camo_zod_pap_pattern", "mc/mtl_t7_camo_zod_pap", "i_t7_camo_zod_pap_c"),
    (42, "Etching", "menu_camo_etching_pattern", "mc/mtl_wpn_t7_camo_etching", "i_wpn_t7_camo_etching_s"),
];
