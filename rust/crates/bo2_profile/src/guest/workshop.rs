//! bo2mp: guns from Black Ops III Steam Workshop weapon packs, each pack a
//! game of its own in Create-a-Class (its tab, its statstable rows).
//!
//! A pack's guns are Black Ops III weapons in the pack's zone (`zm_mod.ff`
//! or `core_mod.ff` in its Workshop folder), read by the same lane as Black
//! Ops III's own (`assets::lane::t7`), under the Black Ops III namespace:
//! a gun's catalog name is the pack's tag and its name there (`cw_ak47` for
//! Cold War's `t9_ak47`), so its script name is `bo3_cw_ak47_mp` and never
//! one of Black Ops III's own (CoD Online 2 reuses BO3's stock names:
//! `ar_standard_zm` is its G37H, `ol2_ar_standard` here). The rosters are
//! written out (not read from the zones): the statstable numbers must not
//! move when a pack updates, and the zones hold upgraded, dual-wield,
//! starting-pistol and skin copies a roster leaves out. A listed gun the
//! zone lacks is not loaded and so not listed in a match.

use super::{AR, CQB, GuestGun, LAUNCHER, LMG, P, PISTOL, S, SMG, SNIPER, SPECIAL};

/// One Workshop weapon pack.
#[derive(Clone, Copy, Debug)]
pub struct WorkshopPack {
    /// Its game in the rosters (`t7cw`).
    pub game: &'static str,
    /// Its Steam Workshop item id (its folder's name).
    pub workshop_id: &'static str,
    /// The zone its guns are in (`zm_mod`).
    pub zone: &'static str,
    /// Its tab in Create-a-Class's weapon picker.
    pub tab: &'static str,
    /// The game its guns come from, in full.
    pub title: &'static str,
    pub guns: &'static [GuestGun],
    pub first_index: i32,
}

impl WorkshopPack {
    /// The name the pack's zone gives a roster gun (`cw_ak47` -> `t9_ak47`).
    #[must_use]
    pub fn zone_weapon(&self, gun: &str) -> String {
        let rest = gun.split_once('_').map_or(gun, |(_, r)| r);
        match self.game {
            "t7cw" => format!("t9_{rest}"),
            "t7vg" => format!("s4_{rest}"),
            "t7mw" => format!("iw8_{rest}"),
            "t7cod2" => format!("{rest}_cod2_zm"),
            "t7ol" => format!("codol_{rest}"),
            "t7ol2" => format!("{rest}_zm"),
            _ => gun.to_owned(),
        }
    }

    /// The picture a roster gun's Create-a-Class tile tries: the pack's own
    /// kill icon where it has one per gun (CoD Online's `i_codol_<gun>`,
    /// CoD 2's `iw2_hud_<gun>`); Cold War's, Vanguard's, Modern Warfare's
    /// and CoD Online 2's guns name Black Ops III's stock icons (an AP9's
    /// for every gun), so theirs is left blank.
    #[must_use]
    pub fn picture(&self, gun: &str) -> String {
        let rest = gun.split_once('_').map_or(gun, |(_, r)| r);
        match self.game {
            "t7ol" => format!("{}:i_codol_{rest}", self.game),
            "t7cod2" => match rest.strip_suffix("_sniper") {
                Some(base) => format!("{}:iw2_hud_{base}_scoped|iw2_hud_{base}", self.game),
                None => format!("{}:iw2_hud_{rest}", self.game),
            },
            _ => String::new(),
        }
    }
}

const fn cw(
    gun: &'static str,
    name: &'static str,
    group: &'static str,
    slot: &'static str,
) -> GuestGun {
    GuestGun {
        game: "t7cw",
        gun,
        name,
        group,
        slot,
        attachments: "",
        icon: "",
    }
}
const fn vg(
    gun: &'static str,
    name: &'static str,
    group: &'static str,
    slot: &'static str,
) -> GuestGun {
    GuestGun {
        game: "t7vg",
        gun,
        name,
        group,
        slot,
        attachments: "",
        icon: "",
    }
}
const fn mw(
    gun: &'static str,
    name: &'static str,
    group: &'static str,
    slot: &'static str,
) -> GuestGun {
    GuestGun {
        game: "t7mw",
        gun,
        name,
        group,
        slot,
        attachments: "",
        icon: "",
    }
}
const fn c2(
    gun: &'static str,
    name: &'static str,
    group: &'static str,
    slot: &'static str,
) -> GuestGun {
    GuestGun {
        game: "t7cod2",
        gun,
        name,
        group,
        slot,
        attachments: "",
        icon: "",
    }
}
const fn ol(
    gun: &'static str,
    name: &'static str,
    group: &'static str,
    slot: &'static str,
) -> GuestGun {
    GuestGun {
        game: "t7ol",
        gun,
        name,
        group,
        slot,
        attachments: "",
        icon: "",
    }
}
const fn o2(
    gun: &'static str,
    name: &'static str,
    group: &'static str,
    slot: &'static str,
) -> GuestGun {
    GuestGun {
        game: "t7ol2",
        gun,
        name,
        group,
        slot,
        attachments: "",
        icon: "",
    }
}

/// Black Ops Cold War's guns (Workshop pack 2753027294, `zm_mod`'s
/// `t9_<gun>`). Tactical rifles are assault rifles; the PPSh-41 is its drum
/// version (`t9_ppsh41_drum`), the EM2 and Swiss K31 their iron-sight ones.
pub const COLD_WAR: &[GuestGun] = &[
    cw("cw_ak47", "AK-47", AR, P),
    cw("cw_aug", "AUG", AR, P),
    cw("cw_c58", "C58", AR, P),
    cw("cw_carv2", "CARV.2", AR, P),
    cw("cw_em2_irons", "EM2", AR, P),
    cw("cw_fara83", "FARA 83", AR, P),
    cw("cw_ffar1", "FFAR 1", AR, P),
    cw("cw_grav", "Grav", AR, P),
    cw("cw_groza", "Groza", AR, P),
    cw("cw_krig6", "Krig 6", AR, P),
    cw("cw_m16", "M16", AR, P),
    cw("cw_qbz83", "QBZ-83", AR, P),
    cw("cw_xm4", "XM4", AR, P),
    cw("cw_dmr14", "DMR 14", AR, P),
    cw("cw_type63", "Type 63", AR, P),
    cw("cw_ak74u", "AK-74u", SMG, P),
    cw("cw_bullfrog", "Bullfrog", SMG, P),
    cw("cw_ksp45", "KSP 45", SMG, P),
    cw("cw_lapa", "LAPA", SMG, P),
    cw("cw_lc10", "LC10", SMG, P),
    cw("cw_mac10", "MAC-10", SMG, P),
    cw("cw_milano821", "Milano 821", SMG, P),
    cw("cw_mp5", "MP5", SMG, P),
    cw("cw_mp5k", "MP5K", SMG, P),
    cw("cw_ots9", "OTs 9", SMG, P),
    cw("cw_ppsh41_drum", "PPSh-41", SMG, P),
    cw("cw_tec9", "TEC-9", SMG, P),
    cw("cw_rpk", "RPK", LMG, P),
    cw("cw_aug_hbar", "AUG HBAR", LMG, P),
    cw("cw_hk21", "HK21", LMG, P),
    cw("cw_m60", "M60", LMG, P),
    cw("cw_mg82", "MG 82", LMG, P),
    cw("cw_rpd", "RPD", LMG, P),
    cw("cw_stoner63", "Stoner 63", LMG, P),
    cw("cw_galatz", "Galatz", SNIPER, P),
    cw("cw_lw3_tundra", "LW3 - Tundra", SNIPER, P),
    cw("cw_m82", "M82", SNIPER, P),
    cw("cw_pelington703", "Pelington 703", SNIPER, P),
    cw("cw_swiss_k31_irons", "Swiss K31", SNIPER, P),
    cw("cw_zrg20mm", "ZRG 20mm", SNIPER, P),
    cw("cw_410ironhide", ".410 Ironhide", CQB, P),
    cw("cw_gallo_sa12", "Gallo SA12", CQB, P),
    cw("cw_hauer77", "Hauer 77", CQB, P),
    cw("cw_streetsweeper", "Streetsweeper", CQB, P),
    cw("cw_1911", "1911", PISTOL, S),
    cw("cw_amp63", "AMP63", PISTOL, S),
    cw("cw_diamatti", "Diamatti", PISTOL, S),
    cw("cw_magnum", "Magnum", PISTOL, S),
    cw("cw_marshal", "Marshal", PISTOL, S),
    cw("cw_m79", "M79", LAUNCHER, S),
    cw("cw_rpg7", "RPG-7", LAUNCHER, S),
    cw("cw_nail_gun", "Nail Gun", SPECIAL, S),
];

/// Vanguard's guns (Workshop pack 2906698009, `zm_mod`'s `s4_<gun>`).
/// Scoped bolt-actions and the Gorenko are sniper rifles, the marksman
/// rifles (Kar98k, G-43, M1 Garand, SVT-40) assault rifles.
pub const VANGUARD: &[GuestGun] = &[
    vg("vg_as44", "AS44", AR, P),
    vg("vg_automaton", "Automaton", AR, P),
    vg("vg_bar", "BAR", AR, P),
    vg("vg_cooper_carbine", "Cooper Carbine", AR, P),
    vg("vg_nz41", "NZ-41", AR, P),
    vg("vg_stg44", "STG44", AR, P),
    vg("vg_volkstg", "Volkssturmgewehr", AR, P),
    vg("vg_itra_burst", "ITRA Burst", AR, P),
    vg("vg_kar98k_irons", "Kar98k", AR, P),
    vg("vg_g43", "G-43", AR, P),
    vg("vg_m1garand", "M1 Garand", AR, P),
    vg("vg_svt40", "SVT-40", AR, P),
    vg("vg_m1928", "M1928", SMG, P),
    vg("vg_thompsonm1a1", "Thompson M1A1", SMG, P),
    vg("vg_mp40", "MP40", SMG, P),
    vg("vg_mp41", "MP-41", SMG, P),
    vg("vg_owen_gun", "Owen Gun", SMG, P),
    vg("vg_ppsh41_drum", "PPSh-41", SMG, P),
    vg("vg_pps43", "PPS-43", SMG, P),
    vg("vg_sten", "Sten", SMG, P),
    vg("vg_type100", "Type 100", SMG, P),
    vg("vg_welgun", "Welgun", SMG, P),
    vg("vg_bren", "Bren", LMG, P),
    vg("vg_dp27", "DP27", LMG, P),
    vg("vg_mg42", "MG42", LMG, P),
    vg("vg_type11", "Type 11", LMG, P),
    vg("vg_3line_scope", "3-Line Rifle", SNIPER, P),
    vg("vg_gorenko_rifle", "Gorenko Anti-Tank Rifle", SNIPER, P),
    vg("vg_lee_enfield_scope", "Lee Enfield", SNIPER, P),
    vg("vg_type99_scope", "Type 99", SNIPER, P),
    vg("vg_combat_shotgun", "Combat Shotgun", CQB, P),
    vg("vg_double_barrel", "Double Barrel", CQB, P),
    vg("vg_einhorn", "Einhorn Revolving", CQB, P),
    vg("vg_gracey_auto", "Gracey Auto", CQB, P),
    vg("vg_double_barrel_sawn", "Sawed-off Double Barrel", CQB, S),
    vg("vg_1911", "1911", PISTOL, S),
    vg("vg_klauser", "Klauser", PISTOL, S),
    vg("vg_machinepistol", "Machine Pistol", PISTOL, S),
    vg("vg_ratt", "RATT", PISTOL, S),
    vg("vg_topbreak", "Top Break", PISTOL, S),
    vg("vg_m1bazooka", "M1 Bazooka", LAUNCHER, S),
    vg("vg_mk11", "MK11 Launcher", LAUNCHER, S),
];

/// Modern Warfare (2019)'s guns (Workshop pack 3829658749, `zm_mod`'s
/// `iw8_<gun>`), by Modern Warfare's own names. The pack's Cold War Gallo
/// SA12 (`t9_gallo_sa12`) is Cold War's, left out; the M16A4 and Stechkin
/// are the pack's additions.
pub const MW2019: &[GuestGun] = &[
    mw("mw19_m4a1", "M4A1", AR, P),
    mw("mw19_ak47", "AK-47", AR, P),
    mw("mw19_an94", "AN-94", AR, P),
    mw("mw19_asval", "AS VAL", AR, P),
    mw("mw19_cr56", "CR-56 AMAX", AR, P),
    mw("mw19_fal", "FAL", AR, P),
    mw("mw19_scar17s", "FN Scar 17", AR, P),
    mw("mw19_fr556", "FR 5.56", AR, P),
    mw("mw19_grau556", "Grau 5.56", AR, P),
    mw("mw19_m16a4", "M16A4", AR, P),
    mw("mw19_ram7", "RAM-7", AR, P),
    mw("mw19_ak74u", "AK-74u", SMG, P),
    mw("mw19_aug_lmg", "AUG", SMG, P),
    mw("mw19_cx9", "CX-9", SMG, P),
    mw("mw19_fennec", "Fennec", SMG, P),
    mw("mw19_mp5", "MP5", SMG, P),
    mw("mw19_mp7", "MP7", SMG, P),
    mw("mw19_p90", "P90", SMG, P),
    mw("mw19_bizon", "PP19 Bizon", SMG, P),
    mw("mw19_striker45", "Striker 45", SMG, P),
    mw("mw19_uzi", "Uzi", SMG, P),
    mw("mw19_bruenmk9", "Bruen Mk9", LMG, P),
    mw("mw19_holger26", "Holger-26", LMG, P),
    mw("mw19_pkm", "PKM", LMG, P),
    mw("mw19_sa87", "SA87", LMG, P),
    mw("mw19_ax50", "AX-50", SNIPER, P),
    mw("mw19_dragunov", "Dragunov", SNIPER, P),
    mw("mw19_ebr14", "EBR-14", SNIPER, P),
    mw("mw19_kar98k_scope", "Kar98k", SNIPER, P),
    mw("mw19_rytecamr", "Rytec AMR", SNIPER, P),
    mw("mw19_725", "725", CQB, P),
    mw("mw19_jak12", "JAK-12", CQB, P),
    mw("mw19_model680", "Model 680", CQB, P),
    mw("mw19_r90", "R9-0", CQB, P),
    mw("mw19_1911", "1911", PISTOL, S),
    mw("mw19_50gs", ".50 GS", PISTOL, S),
    mw("mw19_m19", "M19", PISTOL, S),
    mw("mw19_beretta93r", "Renetti", PISTOL, S),
    mw("mw19_stechkin_aps", "Stechkin APS", PISTOL, S),
    mw("mw19_rpg7", "RPG-7", LAUNCHER, S),
];

/// Call of Duty 2's guns (Workshop pack 804408209, `zm_mod`'s
/// `<gun>_cod2_zm`; its `core_mod` holds the multiplayer copies and the
/// streamed data). Bolt-actions are sniper rifles, as World at War's.
pub const COD2: &[GuestGun] = &[
    c2("cod2_m1garand", "M1 Garand", AR, P),
    c2("cod2_carbine", "M1 Carbine", AR, P),
    c2("cod2_g43", "Gewehr 43", AR, P),
    c2("cod2_svt", "Tokarev SVT-40", AR, P),
    c2("cod2_mp44", "MP-44", AR, P),
    c2("cod2_thompson", "Thompson", SMG, P),
    c2("cod2_m3", "M3 Grease Gun", SMG, P),
    c2("cod2_mp40", "MP-40", SMG, P),
    c2("cod2_ppsh", "PPSh-41", SMG, P),
    c2("cod2_pps42", "PPS-42", SMG, P),
    c2("cod2_sten", "Sten", SMG, P),
    c2("cod2_bar", "BAR", LMG, P),
    c2("cod2_bren", "Bren", LMG, P),
    c2("cod2_enfield", "Lee-Enfield", SNIPER, P),
    c2("cod2_enfield_sniper", "Lee-Enfield Scoped", SNIPER, P),
    c2("cod2_kar98", "Kar98k", SNIPER, P),
    c2("cod2_kar98_sniper", "Kar98k Scoped", SNIPER, P),
    c2("cod2_mosin", "Mosin-Nagant", SNIPER, P),
    c2("cod2_mosin_sniper", "Mosin-Nagant Scoped", SNIPER, P),
    c2("cod2_springfield", "Springfield", SNIPER, P),
    c2("cod2_g43_sniper", "Gewehr 43 Scoped", SNIPER, P),
    c2("cod2_trenchgun", "Trenchgun", CQB, P),
    c2("cod2_colt", "Colt .45", PISTOL, S),
    c2("cod2_luger", "Luger", PISTOL, S),
    c2("cod2_tt30", "TT-30", PISTOL, S),
    c2("cod2_webley", "Webley", PISTOL, S),
];

/// Call of Duty Online's guns (Workshop pack 1798855811, `zm_mod`'s
/// `codol_<class>_<gun>`): each gun once, its plain copy (the skins,
/// `ak47cb`, `an94hw`, ..., and the `ss`/`altfire` specials left out); a gun
/// the pack has only in a skin is listed by it (`m240rare`, `pp2000cs`,
/// `as50rare`, `csg12tool`, `acrnuke`) under the gun's name.
pub const CODOL: &[GuestGun] = &[
    ol("ol_ar_ak47", "AK-47", AR, P),
    ol("ol_ar_ak104", "AK117", AR, P),
    ol("ol_ar_akbp", "AKBP", AR, P),
    ol("ol_ar_an94", "AN94", AR, P),
    ol("ol_ar_arx51", "X51", AR, P),
    ol("ol_ar_fn2000", "BPR2000", AR, P),
    ol("ol_ar_g36c", "G37H", AR, P),
    ol("ol_ar_m4a1techzb", "M4A1 Tech", AR, P),
    ol("ol_ar_mk14", "MK14 EBR", AR, P),
    ol("ol_ar_tar21", "Tavor-21", AR, P),
    ol("ol_ar_scar", "MK17-CQC", AR, P),
    ol("ol_ar_scarsapr", "MR23", AR, P),
    ol("ol_ar_aac", "AAC", AR, P),
    ol("ol_ar_acrnuke", "Remington ACR", AR, P),
    ol("ol_ar_m1garandmodern", "M1 Garand Modern", AR, P),
    ol("ol_smg_aks74u", "AKS74u", SMG, P),
    ol("ol_smg_mp5k", "SMG5", SMG, P),
    ol("ol_smg_miniuzi", "Mini-Uzi", SMG, P),
    ol("ol_smg_mp5sd", "SMG5SD", SMG, P),
    ol("ol_smg_p90", "PDW90", SMG, P),
    ol("ol_smg_pp2000cs", "PP2000", SMG, P),
    ol("ol_smg_tommygun", "Thompson", SMG, P),
    ol("ol_smg_ump45mw", "Grease Gun", SMG, P),
    ol("ol_smg_vectorh", "Vector", SMG, P),
    ol("ol_lmg_ariar", "M4LMG", LMG, P),
    ol("ol_lmg_aug", "Steyr AUG-H", LMG, P),
    ol("ol_lmg_mg4", "MAG43", LMG, P),
    ol("ol_lmg_rpd", "RPD", LMG, P),
    ol("ol_lmg_m240rare", "M260B", LMG, P),
    ol("ol_sniper_cheytac", "CheyTac M200", SNIPER, P),
    ol("ol_sniper_as50rare", "Arctic .50 BMG", SNIPER, P),
    ol("ol_sniper_rsrt3judge", "Type85 Evolution", SNIPER, P),
    ol("ol_sniper_tac300bluing", "Walther 2000", SNIPER, P),
    ol("ol_shotgun_aa12", "AA-12", CQB, P),
    ol("ol_shotgun_gpas12", "GPAS-12", CQB, P),
    ol("ol_shotgun_m1014", "Benelli M1014", CQB, P),
    ol("ol_shotgun_m1887", "Winchester M1887", CQB, P),
    ol("ol_shotgun_r870", "Remington 870 MCS", CQB, P),
    ol("ol_shotgun_spas12", "Franchi-12", CQB, P),
    ol("ol_shotgun_striker", "Striker", CQB, P),
    ol("ol_shotgun_csg12tool", "CSG-12", CQB, P),
    ol("ol_pistol_dbundle", "Desert Eagle", PISTOL, S),
    ol("ol_pistol_fa1911ncm", "FA1911", PISTOL, S),
    ol("ol_pistol_mk1911", "MK1911", PISTOL, S),
    ol("ol_launcher_at4", "AT-4", LAUNCHER, S),
    ol("ol_launcher_rpg7", "RPG-7", LAUNCHER, S),
    ol("ol_special_bow", "Bow", SPECIAL, S),
];

/// Call of Duty Online's second pack (Workshop pack 2014476142,
/// `core_mod`'s `<gun>_zm`): Black Ops III's Zombies guns reskinned with
/// CoD Online's models, and its additions (AK-47, FAMAS, CAR-T, M16A4,
/// AK117, Thompson, SMG5SD, CSG 12, Glock 18, ...). Its dual-wield Ranger,
/// alternate-mode specials and wonder weapons are left out, and so are the
/// guns whose animations the zone does not hold under their own names (the
/// M1 Garand, Type 95, Remington ACR, MK17-CQC, CAR-T, SPBC, M16A4 and M21
/// EBR: their tables name Black Ops III's stock animations by references
/// the walk misplaces) and the G37H Custom (its model's mesh is not read).
pub const CODOL2: &[GuestGun] = &[
    o2("ol2_ar_standard", "G37H", AR, P),
    o2("ol2_ar_cqb", "M4A1 Tech", AR, P),
    o2("ol2_ar_m14", "MR23", AR, P),
    o2("ol2_ar_stg44", "AK117", AR, P),
    o2("ol2_ar_garand", "BPR2000 Tech", AR, P),
    o2("ol2_ar_famas", "FAMAS", AR, P),
    o2("ol2_ar_ak47", "AK-47", AR, P),
    o2("ol2_smg_standard", "UMG", SMG, P),
    o2("ol2_smg_fastfire", "Vector", SMG, P),
    o2("ol2_smg_versatile", "SMG5", SMG, P),
    o2("ol2_smg_capacity", "PDW90", SMG, P),
    o2("ol2_smg_burst", "Skorpion", SMG, P),
    o2("ol2_smg_thompson", "Thompson", SMG, P),
    o2("ol2_smg_ak74u", "AKS-74U", SMG, P),
    o2("ol2_smg_mp40_1940", "AKBP", SMG, P),
    o2("ol2_smg_longrange", "MP7", SMG, P),
    o2("ol2_smg_sten", "SMG5SD", SMG, P),
    o2("ol2_lmg_light", "MAG43", LMG, P),
    o2("ol2_lmg_heavy", "M260B", LMG, P),
    o2("ol2_lmg_cqb", "RPD", LMG, P),
    o2("ol2_lmg_slowfire", "Steyr AUG", LMG, P),
    o2("ol2_lmg_rpk", "SA80-LSW", LMG, P),
    o2("ol2_sniper_fastbolt", "CheyTac M200", SNIPER, P),
    o2("ol2_sniper_powerbolt", "North Pole .50 BMG", SNIPER, P),
    o2("ol2_shotgun_pump", "Franchi 12", CQB, P),
    o2("ol2_shotgun_fullauto", "Atchisson AA-12", CQB, P),
    o2("ol2_shotgun_precision", "Winchester M1887", CQB, P),
    o2("ol2_shotgun_semiauto", "Benelli M1014", CQB, P),
    o2("ol2_shotgun_energy", "Striker", CQB, P),
    o2("ol2_shotgun_csg12", "CSG 12", CQB, P),
    o2("ol2_pistol_standard", "M1911.45", PISTOL, S),
    o2("ol2_pistol_fullauto", "Mini UZI", PISTOL, S),
    o2("ol2_pistol_burst", "Beretta M93R", PISTOL, S),
    o2("ol2_pistol_energy", "Glock 18", PISTOL, S),
    o2("ol2_launcher_standard", "RPG", LAUNCHER, S),
];

/// The key two CoD Online guns' display names are compared by: letters and
/// digits only, the two packs' spellings folded together (`Franchi-12` and
/// `Franchi 12`, `Atchisson AA-12` and `AA-12`, `RPG-7` and `RPG`).
fn codol_key(name: &str) -> String {
    let k: String = name.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect();
    match k.as_str() {
        "atchissonaa12" => "aa12".to_owned(),
        "steyraugh" => "steyraug".to_owned(),
        "rpg7" => "rpg".to_owned(),
        _ => k,
    }
}

/// The two CoD Online packs as one: (first-pack gun, second-pack gun) for
/// every gun both have, by name. Create-a-Class lists the first pack's copy
/// (`ol_...`, the original CoD Online models) and hides the second's
/// (`ol2_...`, Black Ops III's guns reskinned). The rosters, and so every
/// saved class's statstable rows, are unchanged.
#[must_use]
pub fn codol_pairs() -> Vec<(&'static str, &'static str)> {
    CODOL
        .iter()
        .filter_map(|a| {
            let k = codol_key(a.name);
            CODOL2.iter().find(|b| codol_key(b.name) == k).map(|b| (a.gun, b.gun))
        })
        .collect()
}

/// Whether a CoD Online roster gun is hidden because the first pack has the
/// same gun (see [`codol_pairs`]).
#[must_use]
pub fn codol_hidden(game: &str, gun: &str) -> bool {
    game == "t7ol2" && super::available("t7ol") && codol_pairs().iter().any(|(_, b)| *b == gun)
}

/// The packs, in their tabs' order.
pub const WORKSHOP_PACKS: [WorkshopPack; 6] = [
    WorkshopPack {
        game: "t7cw",
        workshop_id: "2753027294",
        zone: "zm_mod",
        tab: "COLD WAR",
        title: "Black Ops Cold War",
        guns: COLD_WAR,
        first_index: 1500,
    },
    WorkshopPack {
        game: "t7vg",
        workshop_id: "2906698009",
        zone: "zm_mod",
        tab: "VANGUARD",
        title: "Vanguard",
        guns: VANGUARD,
        first_index: 1600,
    },
    WorkshopPack {
        game: "t7mw",
        workshop_id: "3829658749",
        zone: "zm_mod",
        tab: "MW 2019",
        title: "Modern Warfare (2019)",
        guns: MW2019,
        first_index: 1700,
    },
    WorkshopPack {
        game: "t7cod2",
        workshop_id: "804408209",
        zone: "zm_mod",
        tab: "COD 2",
        title: "Call of Duty 2",
        guns: COD2,
        first_index: 1800,
    },
    WorkshopPack {
        game: "t7ol",
        workshop_id: "1798855811",
        zone: "zm_mod",
        tab: "CODOL",
        title: "Call of Duty Online",
        guns: CODOL,
        first_index: 1900,
    },
    WorkshopPack {
        game: "t7ol2",
        workshop_id: "2014476142",
        zone: "core_mod",
        tab: "CODOL 2",
        title: "Call of Duty Online (second pack)",
        guns: CODOL2,
        first_index: 2000,
    },
];

/// The pack whose game this is (`t7cw`).
#[must_use]
pub fn workshop_pack(game: &str) -> Option<&'static WorkshopPack> {
    WORKSHOP_PACKS.iter().find(|p| p.game == game)
}

/// Whether a game is a Workshop pack's.
#[must_use]
pub fn is_workshop_game(game: &str) -> bool {
    workshop_pack(game).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_name_their_zone_weapons() {
        let p = |g| workshop_pack(g).unwrap();
        assert_eq!(p("t7cw").zone_weapon("cw_ak47"), "t9_ak47");
        assert_eq!(
            p("t7cw").zone_weapon("cw_swiss_k31_irons"),
            "t9_swiss_k31_irons"
        );
        assert_eq!(p("t7vg").zone_weapon("vg_3line_scope"), "s4_3line_scope");
        assert_eq!(p("t7mw").zone_weapon("mw19_50gs"), "iw8_50gs");
        assert_eq!(
            p("t7cod2").zone_weapon("cod2_kar98_sniper"),
            "kar98_sniper_cod2_zm"
        );
        assert_eq!(p("t7ol").zone_weapon("ol_ar_ak47"), "codol_ar_ak47");
        assert_eq!(
            p("t7ol2").zone_weapon("ol2_smg_mp40_1940"),
            "smg_mp40_1940_zm"
        );
        assert_eq!(
            p("t7cod2").picture("cod2_kar98_sniper"),
            "t7cod2:iw2_hud_kar98_scoped|iw2_hud_kar98"
        );
        assert_eq!(p("t7ol").picture("ol_smg_p90"), "t7ol:i_codol_smg_p90");
        assert!(p("t7cw").picture("cw_ak47").is_empty());
    }

    #[test]
    fn a_pack_lists_every_gun_on_its_tab_once_installed() {
        use crate::guest::{
            GAMES, MOD_GAME, TEST_GAMES, available, extend_stats_rows, game_for_index,
            reference_for_index, set_available, tab_game_for_index,
        };
        let _games = TEST_GAMES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let rows_with = |games: &[&str]| {
            for game in GAMES.iter().chain(std::iter::once(&MOD_GAME)) {
                set_available(game, games.contains(game));
            }
            // (Black Ops II's own AK-47 and MP7 do not leave the packs' out.)
            let mut rows: Vec<Vec<String>> = ["ak47", "mp7"]
                .iter()
                .map(|r| {
                    let mut row = vec![String::new(); 19];
                    row[0] = "10".to_owned();
                    row[2] = "weapon_assault".to_owned();
                    row[4] = (*r).to_owned();
                    row
                })
                .collect();
            extend_stats_rows(&mut rows);
            rows
        };
        assert_eq!(rows_with(&[]).len(), 2);
        let rows = rows_with(&["t7", "t7ol2", "t7cw"]);
        assert!(available("t7ol2") && available("t7cw") && !available("t7vg"));
        let row = |r: &str| rows.iter().find(|x| x[4] == r).cloned();
        let ak = row("bo3_cw_ak47").unwrap();
        assert_eq!(
            (ak[0].as_str(), ak[2].as_str(), ak[3].as_str()),
            ("1500", "weapon_assault", "WEAPON_BO3_CW_AK47")
        );
        assert_eq!(
            rows.iter().filter(|r| r[4].starts_with("bo3_cw_")).count(),
            COLD_WAR.len()
        );
        assert_eq!(
            rows.iter().filter(|r| r[4].starts_with("bo3_ol2_")).count(),
            CODOL2.len()
        );
        assert!(row("bo3_vg_stg44").is_none());
        // (CoD Online 2's G37H beside Black Ops III's own KN-44.)
        assert!(row("bo3_ar_standard").is_some() && row("bo3_ol2_ar_standard").is_some());
        assert_eq!(game_for_index(2000), Some("t7ol2"));
        assert_eq!(tab_game_for_index(1500), Some("t7cw"));
        assert_eq!(reference_for_index(1500).as_deref(), Some("bo3_cw_ak47"));
        for game in GAMES.iter().chain(std::iter::once(&MOD_GAME)) {
            set_available(game, false);
        }
    }

    #[test]
    fn rosters_are_their_packs_and_fit_their_rows() {
        let mut all: Vec<&str> = Vec::new();
        for (k, p) in WORKSHOP_PACKS.iter().enumerate() {
            assert!(
                !p.guns.is_empty() && p.guns.len() < 100,
                "{} has {} guns",
                p.game,
                p.guns.len()
            );
            if let Some(next) = WORKSHOP_PACKS.get(k + 1) {
                assert!(p.first_index + p.guns.len() as i32 <= next.first_index);
            }
            let tag = p.guns[0].gun.split('_').next().unwrap();
            for g in p.guns {
                assert_eq!(g.game, p.game);
                assert!(
                    g.gun.starts_with(&format!("{tag}_")),
                    "{} not {tag}_",
                    g.gun
                );
                assert!(!all.contains(&g.gun), "{} twice", g.gun);
                all.push(g.gun);
            }
        }
    }
}
