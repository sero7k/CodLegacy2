//! bo2mp: other games' guns as extra Black Ops II weapons ("guest guns").
//!
//! Each one gets a Create-a-Class row in mp/statsTable.csv (index from
//! `FIRST_INDEX`, reference `bo1_ak74u`) and the attachments BO2 lacks get
//! rows in mp/attachmentTable.csv; the game gives `bo1_ak74u_mp+reflex`,
//! which the weapon catalog names that game's own variant
//! (`ak74u_reflex_mp`). A game's guns are listed only once
//! `set_available` says its guns are there (the front end: the game is
//! installed; the match: its guns were loaded).

use std::sync::atomic::{AtomicU32, Ordering};

mod workshop;
pub use workshop::{CODOL, CODOL2, COD2, COLD_WAR, MW2019, VANGUARD, WORKSHOP_PACKS, WorkshopPack, codol_hidden, codol_pairs, is_workshop_game, workshop_pack};

/// The first statstable index a guest gun takes (BO2's own end at 255).
pub const FIRST_INDEX: i32 = 300;

/// A guest gun: its game (`t5`), its own name there (`ak74u`), its name,
/// BO2's item group and slot, its attachments (as its game names them,
/// in the order Create-a-Class lists them) and its kill icon material.
#[derive(Clone, Copy, Debug)]
pub struct GuestGun {
    pub game: &'static str,
    pub gun: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    pub slot: &'static str,
    pub attachments: &'static str,
    pub icon: &'static str,
}

const fn g(
    gun: &'static str,
    name: &'static str,
    group: &'static str,
    slot: &'static str,
    attachments: &'static str,
    icon: &'static str,
) -> GuestGun {
    GuestGun { game: "t5", gun, name, group, slot, attachments, icon }
}

const SMG: &str = "weapon_smg";
const AR: &str = "weapon_assault";
const LMG: &str = "weapon_lmg";
const SNIPER: &str = "weapon_sniper";
const CQB: &str = "weapon_cqb";
const PISTOL: &str = "weapon_pistol";
const LAUNCHER: &str = "weapon_launcher";
const SPECIAL: &str = "weapon_special";
const P: &str = "primary";
const S: &str = "secondary";

/// Black Ops' multiplayer guns (attachments as its variants in common_mp
/// carry them).
pub const BO1: &[GuestGun] = &[
    g("mp5k", "MP5K", SMG, P, "acog elbit reflex extclip rf silencer", "hud_icon_mp5"),
    g("skorpion", "Skorpion", SMG, P, "extclip grip rf silencer", "hud_icon_skorpion"),
    g("mac11", "MAC11", SMG, P, "elbit reflex extclip grip rf silencer", "hud_icon_mac11"),
    g("ak74u", "AK74u", SMG, P, "acog elbit reflex extclip dualclip gl grip rf silencer", "hud_icon_ak74u"),
    g("uzi", "Uzi", SMG, P, "acog elbit reflex extclip grip rf silencer", "hud_icon_uzi"),
    g("pm63", "PM63", SMG, P, "extclip grip rf", "hud_icon_pm63"),
    g("mpl", "MPL", SMG, P, "acog elbit reflex dualclip grip rf silencer", "hud_icon_mpl"),
    g("spectre", "Spectre", SMG, P, "acog elbit reflex extclip grip rf silencer", "hud_icon_spectre"),
    g("kiparis", "Kiparis", SMG, P, "acog elbit reflex extclip grip rf silencer", "hud_icon_kiparis"),
    g("m16", "M16", AR, P, "acog elbit reflex ir extclip dualclip gl mk ft silencer", "hud_icon_m16a4"),
    g("enfield", "Enfield", AR, P, "acog elbit reflex ir extclip dualclip gl mk ft silencer", ""),
    g("m14", "M14", AR, P, "acog elbit reflex ir extclip gl mk ft grip silencer", "hud_icon_m14"),
    g("famas", "FAMAS", AR, P, "acog elbit reflex ir extclip dualclip gl mk ft silencer", "hud_icon_famas"),
    g("galil", "Galil", AR, P, "acog elbit reflex ir extclip dualclip gl mk ft silencer", "hud_icon_galil"),
    g("aug", "AUG", AR, P, "acog elbit reflex ir extclip dualclip gl mk ft silencer", "hud_icon_aug"),
    g("fnfal", "FN FAL", AR, P, "acog elbit reflex ir extclip dualclip gl mk ft silencer", "hud_icon_fnfal"),
    g("ak47", "AK47", AR, P, "acog elbit reflex ir extclip dualclip gl mk ft silencer", "hud_icon_ak47"),
    g("commando", "Commando", AR, P, "acog elbit reflex ir extclip dualclip gl mk ft silencer", ""),
    g("g11", "G11", AR, P, "lps vzoom", "hud_icon_g11"),
    g("rottweil72", "Olympia", CQB, P, "", ""),
    g("ithaca", "Stakeout", CQB, P, "grip", ""),
    g("spas", "SPAS-12", CQB, P, "silencer", "hud_icon_spas12"),
    g("hs10", "HS10", CQB, P, "", ""),
    g("hk21", "HK21", LMG, P, "acog elbit reflex ir extclip", ""),
    g("rpk", "RPK", LMG, P, "acog elbit reflex ir extclip dualclip", ""),
    g("m60", "M60", LMG, P, "acog elbit reflex ir extclip grip", "hud_icon_m60e4"),
    g("stoner63", "Stoner63", LMG, P, "acog elbit reflex ir extclip", "hud_icon_stoner63a"),
    g("dragunov", "Dragunov", SNIPER, P, "acog ir vzoom extclip silencer", "hud_icon_dragunov"),
    g("wa2000", "WA2000", SNIPER, P, "acog ir vzoom extclip silencer", "hud_icon_waw2000"),
    g("l96a1", "L96A1", SNIPER, P, "acog ir vzoom extclip silencer", ""),
    g("psg1", "PSG1", SNIPER, P, "acog ir vzoom extclip silencer", ""),
    g("asp", "ASP", PISTOL, S, "", ""),
    g("m1911", "M1911", PISTOL, S, "upgradesight extclip silencer", ""),
    g("makarov", "Makarov", PISTOL, S, "upgradesight extclip silencer", "hud_icon_makarov"),
    g("python", "Python", PISTOL, S, "acog snub speed", "hud_icon_colt"),
    g("cz75", "CZ75", PISTOL, S, "upgradesight auto extclip silencer", "hud_icon_cz75"),
    g("m72_law", "M72 LAW", LAUNCHER, S, "", ""),
    g("rpg", "RPG", LAUNCHER, S, "", "hud_icon_rpg"),
    g("strela", "Strela-3", LAUNCHER, S, "", ""),
    g("china_lake", "China Lake", LAUNCHER, S, "", ""),
    g("crossbow_explosive", "Crossbow", SPECIAL, S, "", "hud_icon_crossbow"),
    g("knife_ballistic", "Ballistic Knife", SPECIAL, S, "", ""),
];

const fn m(
    gun: &'static str,
    name: &'static str,
    group: &'static str,
    slot: &'static str,
    attachments: &'static str,
) -> GuestGun {
    GuestGun { game: "iw4", gun, name, group, slot, attachments, icon: "" }
}

const MW2_AR: &str = "reflex eotech acog thermal heartbeat gl shotgun silencer fmj xmags";
const MW2_SMG: &str = "reflex eotech acog thermal rof akimbo silencer fmj xmags";
const MW2_LMG: &str = "reflex eotech acog thermal heartbeat grip silencer fmj xmags";
const MW2_SNIPER: &str = "acog thermal heartbeat silencer fmj xmags";
const MW2_MP: &str = "reflex eotech akimbo silencer fmj xmags";
const MW2_SHOTGUN: &str = "reflex eotech grip silencer fmj xmags";
const MW2_PISTOL: &str = "tactical akimbo silencer fmj xmags";

/// Modern Warfare 2's multiplayer guns (attachments as its variants in
/// common_mp carry them).
pub const MW2: &[GuestGun] = &[
    m("m4", "M4A1", AR, P, MW2_AR),
    m("famas", "FAMAS", AR, P, MW2_AR),
    m("scar", "SCAR-H", AR, P, MW2_AR),
    m("tavor", "TAR-21", AR, P, MW2_AR),
    m("fal", "FAL", AR, P, MW2_AR),
    m("m16", "M16A4", AR, P, MW2_AR),
    m("masada", "ACR", AR, P, MW2_AR),
    m("fn2000", "F2000", AR, P, MW2_AR),
    m("ak47", "AK-47", AR, P, MW2_AR),
    m("mp5k", "MP5K", SMG, P, MW2_SMG),
    m("ump45", "UMP45", SMG, P, MW2_SMG),
    m("kriss", "Vector", SMG, P, MW2_SMG),
    m("p90", "P90", SMG, P, MW2_SMG),
    m("uzi", "Mini-Uzi", SMG, P, MW2_SMG),
    m("sa80", "L86 LSW", LMG, P, MW2_LMG),
    m("rpd", "RPD", LMG, P, MW2_LMG),
    m("mg4", "MG4", LMG, P, MW2_LMG),
    m("aug", "AUG HBAR", LMG, P, MW2_LMG),
    m("m240", "M240", LMG, P, MW2_LMG),
    m("cheytac", "Intervention", SNIPER, P, MW2_SNIPER),
    m("barrett", "Barrett .50cal", SNIPER, P, MW2_SNIPER),
    m("wa2000", "WA2000", SNIPER, P, MW2_SNIPER),
    m("m21", "M21 EBR", SNIPER, P, MW2_SNIPER),
    m("spas12", "SPAS-12", CQB, P, MW2_SHOTGUN),
    m("aa12", "AA-12", CQB, P, MW2_SHOTGUN),
    m("striker", "Striker", CQB, P, MW2_SHOTGUN),
    m("m1014", "M1014", CQB, P, MW2_SHOTGUN),
    m("ranger", "Ranger", CQB, S, "akimbo fmj"),
    m("model1887", "Model 1887", CQB, S, "akimbo fmj"),
    m("pp2000", "PP2000", PISTOL, S, MW2_MP),
    m("glock", "G18", PISTOL, S, MW2_MP),
    m("beretta393", "M93 Raffica", PISTOL, S, MW2_MP),
    m("tmp", "TMP", PISTOL, S, MW2_MP),
    m("usp", "USP .45", PISTOL, S, MW2_PISTOL),
    m("coltanaconda", ".44 Magnum", PISTOL, S, "tactical akimbo fmj"),
    m("beretta", "M9", PISTOL, S, MW2_PISTOL),
    m("deserteagle", "Desert Eagle", PISTOL, S, "tactical akimbo fmj"),
    m("at4", "AT4-HS", LAUNCHER, S, ""),
    m("rpg", "RPG-7", LAUNCHER, S, ""),
    m("stinger", "Stinger", LAUNCHER, S, ""),
    m("javelin", "Javelin", LAUNCHER, S, ""),
    m("m79", "Thumper", LAUNCHER, S, ""),
];

const fn gh(
    gun: &'static str,
    name: &'static str,
    group: &'static str,
    slot: &'static str,
    attachments: &'static str,
    icon: &'static str,
) -> GuestGun {
    GuestGun { game: "iw6", gun, name, group, slot, attachments, icon }
}

// Ghosts' attachments as its weapon definitions list them (scopes,
// under-barrels, others), one word per attachment: the per-class copies
// (`reflexsmg`, `eotechlmg`, `silencer03`, `griphide`, `firetypeburstlight`)
// are the one Create-a-Class picks. Guns whose definitions list no sights
// take their class's (the assault rifles' and SMGs' are the AK-12's).
const GH_AR: &str = "reflex eotech acog hybrid thermal tracker barrelrange flashsuppress silencer xmags fmj";
const GH_AR_UB: &str =
    "reflex eotech acog hybrid thermal tracker gl grip shotgun barrelrange flashsuppress silencer xmags fmj";
const GH_SMG: &str = "reflex eotech acog hybrid thermal tracker grip barrelrange flashsuppress silencer xmags fmj";
const GH_LMG: &str = "acog hybrid thermal tracker barrelrange flashsuppress silencer xmags fmj";
const GH_DMR: &str = "reflex eotech acog thermal tracker silencer xmags fmj";
const GH_SNIPER: &str = "acog thermal vzscope xmags fmj";

/// bo2mp: Call of Duty: Ghosts' multiplayer guns (`iw6_<gun>_mp` in its
/// common_core_mp; Create-a-Class and the scripts call it `gh_<gun>`). A
/// gun's attachments are variants of it (`gh_ak12_mp+reflex`,
/// `asset_game::GuestAttachment`): Ghosts' attachment models and aiming
/// animations, Black Ops II's numbers and sights. Pictures
/// are its ui_mp's `weapon_<gun>_360` (the `icon` names other ones). Its
/// statstable rows are numbered from `GHOSTS_FIRST_INDEX`.
pub const GHOSTS: &[GuestGun] = &[
    gh("ak12", "AK-12", AR, P, "reflex eotech acog hybrid thermal tracker gl grip shotgun barrelrange flashsuppress silencer xmags fmj firetypeburst firetypesingle", ""),
    gh("sc2010", "SC-2010", AR, P, "reflex eotech acog hybrid thermal tracker barrelrange flashsuppress silencer xmags fmj firetypeburst firetypesingle", ""),
    gh("honeybadger", "Honey Badger", AR, P, GH_AR, ""),
    gh("bren", "Bren", AR, P, GH_AR, ""),
    gh("msbs", "MSBS", AR, P, "reflex eotech acog hybrid thermal tracker gl grip shotgun barrelrange flashsuppress silencer xmags fmj firetypeauto firetypesingle", ""),
    gh("arx160", "ARX-160", AR, P, GH_AR_UB, ""),
    gh("fads", "FAD", AR, P, "reflex eotech acog hybrid thermal tracker gl shotgun barrelrange flashsuppress silencer xmags fmj", ""),
    gh("r5rgp", "Remington R5", AR, P, GH_AR, ""),
    gh("kriss", "Vector CRB", SMG, P, "reflex eotech acog hybrid thermal tracker grip barrelrange flashsuppress rof silencer xmags fmj", "weapon_kriss_360|hud_icon_kriss_iw6"),
    gh("pp19", "Bizon", SMG, P, GH_SMG, ""),
    gh("vepr", "Vepr", SMG, P, GH_SMG, ""),
    gh("microtar", "MTAR-X", SMG, P, GH_SMG, ""),
    gh("cbjms", "CBJ-MS", SMG, P, GH_SMG, ""),
    gh("k7", "K7", SMG, P, GH_SMG, "weapon_k7_ui_360|hud_icon_k7_ui"),
    gh("m27", "M27-IAR", LMG, P, "reflex eotech acog hybrid thermal tracker gl shotgun barrelrange flashsuppress silencer xmags fmj", ""),
    gh("ameli", "Ameli", LMG, P, GH_LMG, ""),
    gh("lsat", "LSAT", LMG, P, GH_LMG, ""),
    gh("kac", "Chain SAW", LMG, P, GH_LMG, ""),
    gh("vks", "VKS", SNIPER, P, "acog thermal vzscope barrelbored silencer xmags fmj", ""),
    gh("gm6", "Lynx", SNIPER, P, GH_SNIPER, ""),
    gh("usr", "USR", SNIPER, P, GH_SNIPER, ""),
    gh("l115a3", "L115", SNIPER, P, "acog thermal vzscope barrelbored silencer xmags fmj", ""),
    gh("imbel", "IA-2", SNIPER, P, "reflex eotech acog thermal tracker barrelrange flashsuppress silencer xmags fmj firetypeburst ironsight", ""),
    gh("svu", "SVU", SNIPER, P, GH_DMR, ""),
    gh("g28", "MR-28", SNIPER, P, GH_DMR, ""),
    gh("mk14", "MK14 EBR", SNIPER, P, GH_DMR, "weapon_mk14_360|hud_icon_mk14_iw6"),
    gh("mts255", "MTS-255", CQB, P, "reflex eotech grip ammoslug barrelrange silencer xmags", ""),
    gh("fp6", "FP6", CQB, P, "reflex eotech grip ammoslug barrelrange silencer xmags", ""),
    gh("maul", "Maul", CQB, P, "reflex eotech grip ammoslug barrelrange silencer xmags", ""),
    gh("uts15", "Bulldog", CQB, P, "reflex eotech grip barrelrange silencer xmags", ""),
    gh("p226", "P226", PISTOL, S, "reflex eotech akimbo tactical barrelrange flashsuppress silencer xmags fmj", ""),
    gh("m9a1", "M9A1", PISTOL, S, "reflex eotech xmags fmj", ""),
    gh("mp443", "MP-443 Grach", PISTOL, S, "reflex eotech xmags fmj", "weapon_mp443_ui_360|hud_icon_mp443_ui"),
    gh("magnum", ".44 Magnum", PISTOL, S, "reflex eotech acog barrelrange flashsuppress silencer xmags fmj", ""),
    gh("pdw", "PDW", PISTOL, S, "reflex eotech acog tactical xmags fmj", ""),
    gh("panzerfaust3", "Panzerfaust", LAUNCHER, S, "", ""),
    gh("mk32", "MK32", LAUNCHER, S, "", ""),
    gh("maaws", "MAAWS", LAUNCHER, S, "", "hud_icon_maaws"),
    gh("rgm", "Kastet", LAUNCHER, S, "", ""),
];

/// The first statstable index of Ghosts' guns (fixed, as MW3's).
pub const GHOSTS_FIRST_INDEX: i32 = 800;

const fn aw(
    gun: &'static str,
    name: &'static str,
    group: &'static str,
    slot: &'static str,
    attachments: &'static str,
) -> GuestGun {
    GuestGun { game: "s1", gun, name, group, slot, attachments, icon: "" }
}

/// bo2mp: Call of Duty: Advanced Warfare's multiplayer guns (`iw5_<gun>_mp`
/// in its common_mp; Create-a-Class and the scripts call it `aw_<gun>`),
/// grouped by their weapon definitions' class. Its attachments (as AW's
/// weapon definitions list them, in the words below) are variants of the
/// gun: AW's attachment models, aiming animations and suppressed shots,
/// Black Ops II's numbers and sights. Its statstable rows are numbered
/// from `AW_FIRST_INDEX`.
pub const AW: &[GuestGun] = &[
    aw("bal27", "Bal-27", AR, P, "reflex eotech acog hybrid thermal tracker grip steadyaim silencer xmags extbarrel fastads stalker"),
    aw("ak12", "AK12", AR, P, "reflex eotech acog hybrid thermal tracker grip steadyaim silencer xmags dualclip extbarrel fastads stalker"),
    aw("arx160", "ARX-160", AR, P, "reflex eotech acog hybrid thermal tracker grip steadyaim silencer xmags dualclip extbarrel fastads stalker"),
    aw("hbra3", "HBRa3", AR, P, "reflex eotech acog hybrid thermal tracker grip steadyaim silencer xmags dualclip extbarrel fastads stalker"),
    aw("himar", "IMR", AR, P, "reflex eotech acog hybrid thermal tracker grip steadyaim silencer xmags extbarrel fastads stalker"),
    aw("m182spr", "MK14", AR, P, "reflex eotech acog hybrid thermal tracker grip steadyaim silencer xmags dualclip extbarrel fastads stalker"),
    aw("epm3", "EPM3", AR, P, "reflex eotech acog hybrid thermal tracker grip fastads stalker"),
    aw("kf5", "KF5", SMG, P, "reflex eotech acog hybrid thermal tracker grip steadyaim silencer xmags dualclip extbarrel fastads stalker"),
    aw("mp11", "MP11", SMG, P, "reflex eotech acog hybrid thermal tracker grip steadyaim silencer xmags dualclip extbarrel fastads stalker"),
    aw("asm1", "ASM1", SMG, P, "reflex eotech acog hybrid thermal tracker grip steadyaim silencer xmags dualclip extbarrel fastads stalker"),
    aw("sn6", "SN6", SMG, P, "reflex eotech acog hybrid thermal tracker grip steadyaim silencer xmags dualclip extbarrel fastads stalker"),
    aw("hmr9", "AMR9", SMG, P, "reflex eotech acog hybrid thermal tracker grip steadyaim silencer xmags dualclip extbarrel fastads stalker"),
    aw("sac3", "SAC3", SMG, P, "steadyaim silencer xmags extbarrel"),
    aw("lsat", "Pytaek", LMG, P, "reflex eotech acog hybrid thermal tracker grip steadyaim silencer xmags extbarrel fastads stalker"),
    aw("exoxmg", "XMG", LMG, P, "steadyaim xmags"),
    aw("asaw", "Ameli", LMG, P, "reflex eotech acog hybrid thermal tracker grip steadyaim silencer xmags extbarrel fastads stalker"),
    aw("em1", "EM1", LMG, P, "reflex eotech acog hybrid thermal tracker grip fastads stalker"),
    aw("mors", "MORS", SNIPER, P, "acog thermal vzscope fastads stalker"),
    aw("thor", "Atlas 20mm", SNIPER, P, "vzscope xmags stalker"),
    aw("gm6", "Lynx", SNIPER, P, "acog thermal vzscope silencer xmags fastads stalker"),
    aw("m990", "NA-45", SNIPER, P, "acog thermal vzscope stalker"),
    aw("uts19", "Tac-19", CQB, P, "reflex tracker grip steadyaim silencer xmags extbarrel fastads stalker"),
    aw("rhino", "S-12", CQB, P, "reflex tracker grip steadyaim silencer xmags extbarrel fastads stalker"),
    aw("maul", "Bulldog", CQB, P, "reflex tracker grip steadyaim silencer xmags extbarrel fastads stalker"),
    aw("titan45", "Atlas 45", PISTOL, S, "reflex eotech acog tracker steadyaim silencer xmags extbarrel fastads"),
    aw("pbw", "PDW", PISTOL, S, "reflex eotech acog tracker steadyaim silencer xmags fastads"),
    aw("rw1", "RW1", PISTOL, S, "reflex eotech acog tracker extbarrel"),
    aw("vbr", "VBR", PISTOL, S, "reflex eotech acog tracker steadyaim silencer xmags fastads"),
    aw("mahem", "MAHEM", LAUNCHER, S, ""),
    aw("stingerm7", "Stinger M7", LAUNCHER, S, ""),
    aw("maaws", "MAAWS", LAUNCHER, S, ""),
    aw("exocrossbow", "Crossbow", SPECIAL, S, "reflex eotech acog hybrid thermal tracker"),
    aw("microdronelauncher", "MDL", SPECIAL, S, ""),
];

/// The first statstable index of Advanced Warfare's guns.
pub const AW_FIRST_INDEX: i32 = 900;

// bo2mp: Black Ops III's attachments by class (its attachment uniques for
// each gun), in Black Ops II's words (`elbit` is its ELO).
const BO3_AR: &str = "reflex elbit holo acog dualoptic ir silencer extclip dualclip extbarrel fmj grip fastads rf stalker steadyaim";
const BO3_SMG: &str = "reflex elbit holo acog dualoptic silencer extclip dualclip extbarrel fmj grip fastads rf stalker steadyaim";
const BO3_LMG: &str = "reflex elbit holo acog dualoptic ir silencer extclip dualclip fmj grip fastads rf stalker steadyaim";
const BO3_SNIPER: &str = "elbit acog dualoptic ir silencer extclip dualclip fmj rf stalker swayreduc";
const BO3_SHOTGUN: &str = "reflex elbit holo silencer extclip dualclip extbarrel fastads rf stalker steadyaim";
const BO3_PISTOL: &str = "reflex elbit silencer extclip dualclip extbarrel fmj fastads steadyaim";

const fn t7(gun: &'static str, name: &'static str, group: &'static str, slot: &'static str) -> GuestGun {
    let attachments = if same(group, AR) {
        BO3_AR
    } else if same(group, SMG) {
        BO3_SMG
    } else if same(group, LMG) {
        BO3_LMG
    } else if same(group, SNIPER) {
        BO3_SNIPER
    } else if same(group, CQB) {
        BO3_SHOTGUN
    } else if same(group, PISTOL) {
        BO3_PISTOL
    } else {
        ""
    };
    GuestGun { game: "t7", gun, name, group, slot, attachments, icon: "" }
}

/// bo2mp: Call of Duty: Black Ops III's multiplayer guns (`<gun>_mp` in its
/// core_common; Create-a-Class and the scripts call it `bo3_<gun>`),
/// grouped by their class. Its attachments (its attachment uniques) are
/// variants of the gun: their models at their placements and their
/// animations, Black Ops II's numbers and sights. Its statstable rows are
/// numbered from `BO3_FIRST_INDEX`.
pub const BO3: &[GuestGun] = &[
    t7("ar_standard", "KN-44", AR, P),
    t7("ar_accurate", "ICR-1", AR, P),
    t7("ar_fastburst", "XR-2", AR, P),
    t7("ar_cqb", "HVK-30", AR, P),
    t7("ar_damage", "Man-O-War", AR, P),
    t7("ar_marksman", "Sheiva", AR, P),
    t7("ar_longburst", "M8A7", AR, P),
    t7("smg_standard", "Kuda", SMG, P),
    t7("smg_versatile", "VMP", SMG, P),
    t7("smg_capacity", "Weevil", SMG, P),
    t7("smg_fastfire", "Vesper", SMG, P),
    t7("smg_burst", "Pharo", SMG, P),
    t7("smg_longrange", "Razorback", SMG, P),
    t7("lmg_light", "BRM", LMG, P),
    t7("lmg_cqb", "Dingo", LMG, P),
    t7("lmg_slowfire", "Gorgon", LMG, P),
    t7("lmg_heavy", "48 Dredge", LMG, P),
    t7("sniper_fastbolt", "Locus", SNIPER, P),
    t7("sniper_fastsemi", "Drakon", SNIPER, P),
    t7("sniper_powerbolt", "SVG-100", SNIPER, P),
    t7("sniper_chargeshot", "P-06", SNIPER, P),
    t7("shotgun_pump", "KRM-262", CQB, P),
    t7("shotgun_semiauto", "205 Brecci", CQB, P),
    t7("shotgun_fullauto", "Haymaker 12", CQB, P),
    t7("shotgun_precision", "Argus", CQB, P),
    t7("pistol_standard", "MR6", PISTOL, S),
    t7("pistol_burst", "RK5", PISTOL, S),
    t7("pistol_fullauto", "L-CAR 9", PISTOL, S),
    t7("launcher_standard", "XM-53", LAUNCHER, S),
    t7("launcher_lockonly", "BlackCell", LAUNCHER, S),
];

/// The first statstable index of Black Ops III's guns.
pub const BO3_FIRST_INDEX: i32 = 1000;

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

// bo2mp: Modern Warfare III's guns' attachments, Black Ops II's own: a sight
// is a copy of the gun's Black Ops II donor with that sight, the others
// Black Ops II's numbers on the gun. A gun with its own scope takes no
// other sight.
const JUP_AR: &str = "reflex holo acog dualoptic ir mms rangefinder silencer extclip dualclip fastads grip stalker steadyaim fmj";
const JUP_SMG: &str = "reflex holo acog mms rangefinder silencer extclip dualclip fastads grip stalker steadyaim fmj rf";
const JUP_LMG: &str = "reflex holo acog dualoptic ir vzoom silencer extclip fastads grip stalker steadyaim fmj rf";
const JUP_SCOPED: &str = "silencer extclip dualclip fastads stalker steadyaim fmj";
const JUP_MARKSMAN: &str = "acog ir vzoom silencer extclip dualclip fastads stalker steadyaim fmj";
const JUP_SHOTGUN: &str = "reflex holo silencer extclip fastads grip stalker steadyaim fmj";
const JUP_PISTOL: &str = "reflex silencer extclip dualclip fastads stalker steadyaim fmj";

/// The Modern Warfare III guns built with their own scope (sniper rifles,
/// and marksman rifles that carry one).
pub const JUP_SCOPED_GUNS: &[&str] = &[
    "jp10_sn_cdelta50",
    "jp17_sn_hsierra",
    "iw9_sn_alpha50",
    "iw9_sn_mromeo",
    "iw9_sn_walpha",
    "iw9_dm_mike24",
    "iw9_dm_pgolf1",
];

const fn jup_attachments(gun: &str, group: &str) -> &'static str {
    let mut k = 0;
    while k < JUP_SCOPED_GUNS.len() {
        if same(gun, JUP_SCOPED_GUNS[k]) {
            return JUP_SCOPED;
        }
        k += 1;
    }
    if same(group, SMG) {
        JUP_SMG
    } else if same(group, LMG) {
        JUP_LMG
    } else if same(group, SNIPER) {
        JUP_MARKSMAN
    } else if same(group, CQB) {
        JUP_SHOTGUN
    } else if same(group, PISTOL) {
        JUP_PISTOL
    } else {
        JUP_AR
    }
}

const fn jup_roster<const N: usize>(
    names: &[(&'static str, &'static str, &'static str, &'static str)],
    played: &[&'static str],
) -> [GuestGun; N] {
    let mut out = [GuestGun { game: "jup", gun: "", name: "", group: "", slot: "", attachments: "", icon: "" }; N];
    let mut i = 0;
    while i < N {
        let mut k = 0;
        while k < names.len() {
            let (gun, name, group, slot) = names[k];
            if same(gun, played[i]) {
                let attachments = jup_attachments(gun, group);
                out[i] = GuestGun { game: "jup", gun, name, group, slot, attachments, icon: "" };
            }
            k += 1;
        }
        i += 1;
    }
    out
}

/// bo2mp: Call of Duty: Modern Warfare III (2023)'s guns (`jup_<gun>_mp`;
/// Create-a-Class and the scripts call it `jup_<gun>` too,
/// `jup_jp12_pi_mike93`): the ones played (`crate::jup_names::JUP_PLAYED`,
/// whose models the match builds), named by `crate::jup_names`. A guest
/// gun is the base gun with its default parts (a sniper's scope among
/// them); a sight is its donor's sight variant on it. Its statstable rows
/// are numbered from `JUP_FIRST_INDEX`.
pub const JUP: &[GuestGun] =
    &jup_roster::<{ crate::jup_names::JUP_PLAYED.len() }>(crate::jup_names::JUP_NAMES, crate::jup_names::JUP_PLAYED);

/// The first statstable index of Modern Warfare III (2023)'s guns.
pub const JUP_FIRST_INDEX: i32 = 1200;

const fn hm(
    gun: &'static str,
    name: &'static str,
    group: &'static str,
    slot: &'static str,
    icon: &'static str,
) -> GuestGun {
    let attachments = if same(group, SNIPER) {
        HMW_SNIPER
    } else if same(group, CQB) {
        HMW_SHOTGUN
    } else if same(group, PISTOL) {
        HMW_PISTOL
    } else if same(group, LAUNCHER) {
        ""
    } else {
        HMW_RIFLE
    };
    GuestGun { game: "h1", gun, name, group, slot, attachments, icon }
}

// bo2mp: Horizon MW's attachments (its zones' `*mwr` ones and each gun's
// own), by class.
const HMW_RIFLE: &str = "reflex eotech acog thermal grip silencer xmags fmj";
const HMW_SNIPER: &str = "acog thermal silencer xmags fmj";
const HMW_SHOTGUN: &str = "reflex eotech grip silencer xmags fmj";
const HMW_PISTOL: &str = "silencer xmags fmj";

/// bo2mp: Horizon MW's guns: Modern Warfare 2's guns remastered for Modern
/// Warfare Remastered (`h2_<gun>_mp` in its `hmw-mod/zone/hmw_*.ff`; the
/// AT4 is `at4_mp`). Create-a-Class and the scripts call it `h2_<gun>`. Its
/// attachments its zones hold are variants of it, as Advanced Warfare's
/// guns'. `icon` is the gun's HUD icon material in its
/// zone. They replace Modern Warfare 2's, 3's and Call of Duty 4's copies
/// of a gun, on those games' tabs (`PRIORITY`, `hmw_home`).
/// Its statstable rows are numbered from `HMW_FIRST_INDEX`. Its melee
/// weapons (hatchet, shovel, sickle, Diabolical, Cliffhanger), Semtex, the
/// throwing knife and the Infected mode's weapon are not listed.
pub const HMW: &[GuestGun] = &[
    hm("ak47", "AK-47", AR, P, "hud_icon_ak472"),
    hm("fal", "FAL", AR, P, "hud_icon_fnfal"),
    hm("famas", "FAMAS", AR, P, "hud_icon_famas"),
    hm("fn2000", "F2000", AR, P, "hud_icon_fn2000"),
    hm("m16", "M16A4", AR, P, "hud_icon_m16a4"),
    hm("m4", "M4A1", AR, P, "hud_icon_m4_grunt"),
    hm("masada", "ACR", AR, P, "hud_icon_masada"),
    hm("scar", "SCAR-H", AR, P, "hud_icon_scar_h"),
    hm("tavor", "TAR-21", AR, P, "hud_icon_tavor"),
    hm("ak74u", "AK-74u", SMG, P, "hud_icon_ak74u"),
    hm("kriss", "Vector", SMG, P, "hud_icon_kriss"),
    hm("mp5k", "MP5K", SMG, P, "hud_icon_mp5k"),
    hm("p90", "P90", SMG, P, "hud_icon_p90"),
    hm("ump45", "UMP45", SMG, P, "hud_icon_ump45"),
    hm("uzi", "Mini-Uzi", SMG, P, "hud_icon_mini_uzi"),
    hm("aug", "AUG HBAR", LMG, P, "hud_icon_steyr"),
    hm("m240", "M240", LMG, P, "hud_icon_m240"),
    hm("mg4", "MG4", LMG, P, "hud_icon_mg4"),
    hm("rpd", "RPD", LMG, P, "hud_icon_rpd"),
    hm("sa80", "L86 LSW", LMG, P, "hud_icon_sa80_lmg"),
    hm("barrett", "Barrett .50cal", SNIPER, P, "hud_icon_barrett50cal"),
    hm("cheytac", "Intervention", SNIPER, P, "hud_icon_cheytac"),
    hm("m21", "M21 EBR", SNIPER, P, "hud_icon_m14ebr_scope"),
    hm("m40a3", "M40A3", SNIPER, P, "hud_icon_m40a3"),
    hm("wa2000", "WA2000", SNIPER, P, "hud_icon_wa2000"),
    hm("mors", "MORS", SNIPER, P, "hud_icon_mors"),
    hm("aa12", "AA-12", CQB, P, "hud_icon_aa12"),
    hm("m1014", "M1014", CQB, P, "hud_icon_benelli_m4"),
    hm("spas12", "SPAS-12", CQB, P, "hud_icon_spas12"),
    hm("striker", "Striker", CQB, P, "hud_icon_striker"),
    hm("winchester1200", "W1200", CQB, P, "hud_icon_winchester_1200"),
    hm("ranger", "Ranger", CQB, S, "hud_icon_sawed_off"),
    hm("model1887", "Model 1887", CQB, S, "hud_icon_model1887"),
    hm("pp2000", "PP2000", PISTOL, S, "hud_icon_pp2000"),
    hm("glock", "G18", PISTOL, S, "hud_icon_glock"),
    hm("beretta393", "M93 Raffica", PISTOL, S, "hud_icon_beretta393"),
    hm("tmp", "TMP", PISTOL, S, "hud_icon_mp9"),
    hm("usp", "USP .45", PISTOL, S, "hud_icon_usp_45"),
    hm("coltanaconda", ".44 Magnum", PISTOL, S, "hud_icon_colt_anaconda"),
    hm("m9", "M9", PISTOL, S, "hud_icon_m9beretta"),
    hm("deserteagle", "Desert Eagle", PISTOL, S, "hud_icon_desert_eagle"),
    hm("colt45", "M1911", PISTOL, S, "hud_icon_colt_45"),
    hm("boomhilda", "Boomhilda", PISTOL, S, "hud_icon_usp_45"),
    hm("at4", "AT4-HS", LAUNCHER, S, "h2hud_icon_at4"),
    hm("rpg", "RPG-7", LAUNCHER, S, "hud_icon_rpg"),
    hm("m79", "Thumper", LAUNCHER, S, "hud_icon_m79"),
];

/// The first statstable index of Horizon MW's guns.
pub const HMW_FIRST_INDEX: i32 = 1300;

const fn iw(gun: &'static str, name: &'static str, group: &'static str, slot: &'static str, icon: &'static str) -> GuestGun {
    GuestGun { game: "iw7", gun, name, group, slot, attachments: "", icon }
}

/// bo2mp: Call of Duty: Infinite Warfare's multiplayer guns (`iw7_<gun>_mp`
/// in its `global_core_mp`; Create-a-Class and the scripts call it
/// `iw7_<gun>` too). The classic guns are its own versions (`ump45c`, the
/// MacTav-45; `m1c`, the M1), with its own models: they are listed on its
/// tab beside the other games' guns of the same name (`BESIDE`). A guest
/// gun is its base gun (no attachments). `icon` is its HUD icon material in
/// `global_core_mp`. Its statstable rows are numbered from
/// `IW7_FIRST_INDEX`. Its melee weapons (axe, katana, nunchucks), the
/// Zombies Venom-X, the Howitzer and the killstreak and mode weapons are
/// not listed, nor the guns its title updates added (HVR, Widowmaker,
/// Rack-9, Kendall 44, DMR-1), which the zones do not hold (`iw7_g18_mpr`,
/// `iw7_spas_mpl`, `iw7_ump45_mpr`, `iw7_cheytac_mpr`, `iw7_m1_mp` are the
/// classics' rare and legendary variants).
pub const IW7: &[GuestGun] = &[
    iw("m4", "NV4", AR, P, "hud_icon_weapon_m4"),
    iw("sdfar", "R3K", AR, P, "hud_icon_weapon_sdfar"),
    iw("ar57", "KBAR-32", AR, P, "hud_icon_weapon_ar57"),
    iw("fmg", "Type-2", AR, P, "hud_icon_weapon_fmg_mp"),
    iw("ake", "Volk", AR, P, "hud_icon_weapon_ake"),
    iw("rvn", "R-VN", AR, P, "hud_icon_weapon_rvn"),
    iw("vr", "X-Eon", AR, P, "hud_icon_weapon_vr"),
    iw("gauss", "G-Rail", AR, P, "hud_icon_weapon_gauss"),
    iw("arclassic", "OSA", AR, P, "hud_icon_weapon_arx160"),
    iw("m1c", "M1", AR, P, "hud_icon_weapon_m1_c"),
    iw("erad", "Erad", SMG, P, "hud_icon_weapon_erad"),
    iw("fhr", "FHR-40", SMG, P, "hud_icon_weapon_fhr40"),
    iw("crb", "Karma-45", SMG, P, "hud_icon_weapon_crb"),
    iw("ripper", "RPR Evo", SMG, P, "hud_icon_weapon_ripper"),
    iw("crdb", "VPR", SMG, P, "hud_icon_weapon_crdb"),
    iw("mp28", "Trencher", SMG, P, "hud_icon_weapon_mp28"),
    iw("tacburst", "Raijin-EMX", SMG, P, "hud_icon_weapon_tacburst"),
    iw("ump45c", "MacTav-45", SMG, P, "hud_icon_weapon_ump"),
    iw("sdflmg", "R.A.W.", LMG, P, "hud_icon_weapon_sdflmg"),
    iw("mauler", "Mauler", LMG, P, "hud_icon_weapon_mauler"),
    iw("lmg03", "Titan", LMG, P, "hud_icon_weapon_lmg03"),
    iw("minilmg", "Auger", LMG, P, "hud_icon_weapon_minilmg"),
    iw("unsalmg", "Atlas", LMG, P, "hud_icon_weapon_unsalmg"),
    iw("kbs", "KBS Longbow", SNIPER, P, "hud_icon_weapon_kbs"),
    iw("m8", "EBR-800", SNIPER, P, "hud_icon_weapon_m8"),
    iw("ba50cal", "Trek-50", SNIPER, P, "hud_icon_weapon_ba50cal"),
    iw("cheytacc", "TF-141", SNIPER, P, "hud_icon_weapon_m200"),
    iw("devastator", "Reaver", CQB, P, "hud_icon_weapon_devastator"),
    iw("sdfshotty", "DCM-8", CQB, P, "hud_icon_weapon_sdfshotgun"),
    iw("sonic", "Banshee", CQB, P, "hud_icon_weapon_sonic_shotgun"),
    iw("mod2187", "M.2187", CQB, P, "hud_icon_weapon_mod2187"),
    iw("longshot", "Proteus", CQB, P, "hud_icon_weapon_longshot"),
    iw("spasc", "S-Ravage", CQB, P, "hud_icon_weapon_spas_c"),
    iw("emc", "EMC", PISTOL, S, "hud_icon_weapon_emc"),
    iw("nrg", "Oni", PISTOL, S, "hud_icon_weapon_nrg"),
    iw("revolver", "Hailstorm", PISTOL, S, "hud_icon_weapon_revolver"),
    iw("udm45", "UDM", PISTOL, S, "hud_icon_weapon_udm45"),
    iw("mag", "Stallion .44", PISTOL, S, "hud_icon_weapon_mag"),
    iw("g18c", "Hornet", PISTOL, S, "hud_icon_weapon_g18_c"),
    iw("lockon", "Spartan SA3", LAUNCHER, S, "hud_icon_weapon_launcher"),
    iw("chargeshot", "P-LAW", LAUNCHER, S, "hud_icon_weapon_chargeshot"),
];

/// The first statstable index of Infinite Warfare's guns.
pub const IW7_FIRST_INDEX: i32 = 1400;

const fn w3(
    gun: &'static str,
    name: &'static str,
    group: &'static str,
    slot: &'static str,
    attachments: &'static str,
    icon: &'static str,
) -> GuestGun {
    GuestGun { game: "iw5", gun, name, group, slot, attachments, icon }
}

const MW3_AR: &str = "reflex eotech acog hybrid thermal heartbeat gl shotgun silencer xmags";
const MW3_AR_ROF: &str = "reflex eotech acog hybrid thermal heartbeat gl shotgun silencer rof xmags";
const MW3_SMG: &str = "reflex eotech acog hamrhybrid thermal rof silencer xmags";
const MW3_LMG: &str = "reflex eotech acog thermal grip rof silencer xmags";
const MW3_LMG_HB: &str = "reflex eotech acog thermal heartbeat grip rof silencer xmags";
const MW3_SNIPER: &str = "acog vzscope thermal heartbeat silencer xmags";
const MW3_SHOTGUN: &str = "reflex eotech grip silencer xmags";
const MW3_MP: &str = "reflex eotech akimbo silencer xmags";
const MW3_PISTOL: &str = "tactical akimbo silencer xmags";

/// bo2mp: Modern Warfare 3's multiplayer guns, by their names in its
/// common_mp (`iw5_acr`; Create-a-Class and the scripts call it `mw3_acr`).
/// MW3 builds a variant from attachment configurations; the attachments
/// are its statstable's words for the gun (per-class sights and
/// suppressors are one word). Its statstable rows are numbered from
/// `MW3_FIRST_INDEX`, apart from the rosters listed before it.
pub const MW3: &[GuestGun] = &[
    w3("iw5_m4", "M4A1", AR, P, MW3_AR, "weapon_m4_short"),
    w3("iw5_m16", "M16A4", AR, P, MW3_AR_ROF, "weapon_m16a4"),
    w3("iw5_scar", "SCAR-L", AR, P, MW3_AR, "weapon_scar_h"),
    w3("iw5_cm901", "CM901", AR, P, MW3_AR, "weapon_cm901"),
    w3("iw5_type95", "Type 95", AR, P, MW3_AR_ROF, "weapon_type95"),
    w3("iw5_g36c", "G36C", AR, P, MW3_AR, "weapon_g36"),
    w3("iw5_acr", "ACR 6.8", AR, P, MW3_AR, "weapon_acr"),
    w3("iw5_mk14", "MK14", AR, P, MW3_AR_ROF, "weapon_mk14"),
    w3("iw5_ak47", "AK-47", AR, P, MW3_AR, "weapon_ak47"),
    w3("iw5_fad", "FAD", AR, P, MW3_AR, "weapon_fad"),
    w3("iw5_mp5", "MP5", SMG, P, MW3_SMG, "weapon_mp5k"),
    w3("iw5_ump45", "UMP45", SMG, P, MW3_SMG, "weapon_ump45"),
    w3("iw5_pp90m1", "PP90M1", SMG, P, MW3_SMG, "weapon_pp90m1"),
    w3("iw5_p90", "P90", SMG, P, MW3_SMG, "weapon_p90"),
    w3("iw5_m9", "PM-9", SMG, P, MW3_SMG, "weapon_mini_uzi"),
    w3("iw5_mp7", "MP7", SMG, P, MW3_SMG, "weapon_mp7"),
    w3("iw5_sa80", "L86 LSW", LMG, P, MW3_LMG_HB, "weapon_sa80"),
    w3("iw5_mg36", "MG36", LMG, P, MW3_LMG_HB, "weapon_mg36"),
    w3("iw5_pecheneg", "PKP Pecheneg", LMG, P, MW3_LMG, "weapon_pecheneg"),
    w3("iw5_mk46", "MK46", LMG, P, MW3_LMG_HB, "weapon_mk46"),
    w3("iw5_m60", "M60E4", LMG, P, MW3_LMG, "weapon_m60e4"),
    w3("iw5_barrett", "Barrett .50cal", SNIPER, P, MW3_SNIPER, "weapon_barrett"),
    w3("iw5_l96a1", "L118A", SNIPER, P, MW3_SNIPER, "weapon_l96a1"),
    w3("iw5_dragunov", "Dragunov", SNIPER, P, MW3_SNIPER, "weapon_dragunov"),
    w3("iw5_as50", "AS50", SNIPER, P, MW3_SNIPER, "weapon_as50"),
    w3("iw5_rsass", "RSASS", SNIPER, P, MW3_SNIPER, "weapon_rsass"),
    w3("iw5_msr", "MSR", SNIPER, P, MW3_SNIPER, "weapon_msr"),
    w3("iw5_usas12", "USAS 12", CQB, P, MW3_SHOTGUN, "weapon_usas12"),
    w3("iw5_ksg", "KSG 12", CQB, P, MW3_SHOTGUN, "weapon_ksg"),
    w3("iw5_spas12", "SPAS-12", CQB, P, MW3_SHOTGUN, "weapon_spas12"),
    w3("iw5_aa12", "AA-12", CQB, P, MW3_SHOTGUN, "weapon_aa12"),
    w3("iw5_striker", "Striker", CQB, P, MW3_SHOTGUN, "weapon_striker"),
    w3("iw5_1887", "Model 1887", CQB, P, "", "weapon_model1887"),
    w3("iw5_fmg9", "FMG9", PISTOL, S, MW3_MP, "weapon_fmg9"),
    w3("iw5_g18", "G18", PISTOL, S, MW3_MP, "weapon_glock"),
    w3("iw5_mp9", "MP9", PISTOL, S, MW3_MP, "weapon_mp9"),
    w3("iw5_skorpion", "Skorpion", PISTOL, S, MW3_MP, "weapon_skorpion"),
    w3("iw5_usp45", "USP .45", PISTOL, S, MW3_PISTOL, "weapon_usp_45"),
    w3("iw5_p99", "P99", PISTOL, S, MW3_PISTOL, "weapon_p99"),
    w3("iw5_mp412", "MP412", PISTOL, S, "tactical akimbo", "weapon_mp412"),
    w3("iw5_44magnum", ".44 Magnum", PISTOL, S, "tactical akimbo", "weapon_magnum"),
    w3("iw5_fnfiveseven", "Five Seven", PISTOL, S, MW3_PISTOL, "weapon_fnfiveseven"),
    w3("iw5_deserteagle", "Desert Eagle", PISTOL, S, "tactical akimbo", "weapon_desert_eagle"),
    w3("iw5_smaw", "SMAW", LAUNCHER, S, "", "weapon_smaw"),
    w3("stinger", "Stinger", LAUNCHER, S, "", "weapon_stinger"),
    w3("xm25", "XM25", LAUNCHER, S, "", "weapon_xm25"),
    w3("m320", "M320 GLM", LAUNCHER, S, "", "weapon_m320"),
    w3("rpg", "RPG-7", LAUNCHER, S, "", "weapon_rpg7"),
];

/// The first statstable index of MW3's guns: a fixed number, not their
/// place in `all()`, so rosters listed before them can grow without moving
/// a saved class's MW3 gun.
pub const MW3_FIRST_INDEX: i32 = 500;

const fn t4(
    gun: &'static str,
    name: &'static str,
    group: &'static str,
    slot: &'static str,
    attachments: &'static str,
    icon: &'static str,
) -> GuestGun {
    GuestGun { game: "t4", gun, name, group, slot, attachments, icon }
}

const WAW_SMG: &str = "aperture extclip silencer";
const WAW_BOLT: &str = "scoped bayonet gl";

/// bo2mp: World at War's multiplayer guns (attachments as its variants in
/// common_mp carry them, `silenced` and `bigammo` as BO2's `silencer` and
/// `extclip`; the LMGs' bipods are deployed modes, not listed). Its
/// statstable rows are numbered from `WAW_FIRST_INDEX`.
pub const WAW: &[GuestGun] = &[
    t4("stg44", "STG-44", AR, P, "aperture telescopic flash", "hud_icon_stg44"),
    t4("svt40", "SVT-40", AR, P, "aperture telescopic flash", "hud_icon_svt40"),
    t4("gewehr43", "Gewehr 43", AR, P, "aperture telescopic gl silencer", "hud_icon_gewehr43"),
    t4("m1garand", "M1 Garand", AR, P, "scoped flash bayonet gl", "hud_icon_m1garand"),
    t4("m1carbine", "M1A1 Carbine", AR, P, "aperture flash bayonet extclip", "hud_icon_m1carbine"),
    t4("thompson", "Thompson", SMG, P, WAW_SMG, "hud_icon_thompson"),
    t4("mp40", "MP40", SMG, P, WAW_SMG, "hud_icon_mp40"),
    t4("type100smg", "Type 100", SMG, P, WAW_SMG, "hud_icon_type_100_smg"),
    t4("ppsh", "PPSh-41", SMG, P, "aperture extclip", "hud_icon_ppsh"),
    t4("bar", "BAR", LMG, P, "", "hud_icon_bar"),
    t4("fg42", "FG42", LMG, P, "telescopic", "hud_icon_fg42"),
    t4("type99lmg", "Type 99 LMG", LMG, P, "bayonet", "hud_icon_type_99_lmg"),
    t4("dp28", "DP-28", LMG, P, "", "hud_icon_dp28"),
    t4("mg42", "MG42", LMG, P, "", "hud_icon_mg42"),
    t4("30cal", "Browning M1919", LMG, P, "", "hud_icon_30cal"),
    t4("springfield", "Springfield", SNIPER, P, WAW_BOLT, "hud_icon_springfield_scoped"),
    t4("kar98k", "Kar98k", SNIPER, P, WAW_BOLT, "hud_icon_kar98k"),
    t4("mosinrifle", "Mosin-Nagant", SNIPER, P, WAW_BOLT, "hud_icon_mosin_rifle"),
    t4("type99rifle", "Arisaka", SNIPER, P, WAW_BOLT, "hud_icon_type_99_rifle"),
    t4("ptrs41", "PTRS-41", SNIPER, P, "", "hud_icon_ptrs41"),
    t4("shotgun", "M1897 Trench Gun", CQB, P, "bayonet grip", "hud_icon_shotgun"),
    t4("doublebarreledshotgun", "Double-Barreled Shotgun", CQB, P, "sawoff grip", "hud_icon_double_barrel"),
    t4("colt", "M1911 .45", PISTOL, S, "", "hud_icon_colt"),
    t4("nambu", "Nambu", PISTOL, S, "", "hud_icon_nambu"),
    t4("tokarev", "Tokarev TT-33", PISTOL, S, "", "hud_icon_tokarev"),
    t4("walther", "Walther P38", PISTOL, S, "", "hud_icon_walther"),
    t4("357magnum", ".357 Magnum", PISTOL, S, "", "hud_icon_357"),
    t4("bazooka", "M9A1 Bazooka", LAUNCHER, S, "", "hud_icon_bazooka"),
];

/// The first statstable index of World at War's guns (see
/// `MW3_FIRST_INDEX`).
pub const WAW_FIRST_INDEX: i32 = 600;

const fn c4(
    gun: &'static str,
    name: &'static str,
    group: &'static str,
    slot: &'static str,
    attachments: &'static str,
    icon: &'static str,
) -> GuestGun {
    GuestGun { game: "iw3", gun, name, group, slot, attachments, icon }
}

const COD4_AR: &str = "reflex acog gl silencer";
const COD4_SMG: &str = "reflex acog silencer";
const COD4_LMG: &str = "reflex acog grip";

/// bo2mp: Call of Duty 4's multiplayer guns (attachments as its variants
/// in common_mp carry them). Its statstable rows are numbered from
/// `COD4_FIRST_INDEX`.
pub const COD4: &[GuestGun] = &[
    c4("m16", "M16A4", AR, P, COD4_AR, "hud_icon_m16a4_grenade"),
    c4("ak47", "AK-47", AR, P, COD4_AR, "hud_icon_ak47"),
    c4("m4", "M4 Carbine", AR, P, COD4_AR, "hud_icon_m4carbine"),
    c4("g3", "G3", AR, P, COD4_AR, "hud_icon_g3"),
    c4("g36c", "G36C", AR, P, COD4_AR, "hud_icon_g36c_mp"),
    c4("m14", "M14", AR, P, COD4_AR, "hud_icon_m14"),
    c4("mp44", "MP44", AR, P, "", "hud_icon_mp44"),
    c4("mp5", "MP5", SMG, P, COD4_SMG, "hud_icon_mp5"),
    c4("skorpion", "Skorpion", SMG, P, COD4_SMG, "hud_icon_skorpian"),
    c4("uzi", "Mini-Uzi", SMG, P, COD4_SMG, "hud_icon_mini_uzi"),
    c4("ak74u", "AK-74u", SMG, P, COD4_SMG, "hud_icon_ak74u"),
    c4("p90", "P90", SMG, P, COD4_SMG, "hud_icon_p90"),
    c4("saw", "M249 SAW", LMG, P, COD4_LMG, "hud_icon_m249saw"),
    c4("rpd", "RPD", LMG, P, COD4_LMG, "hud_icon_rpd"),
    c4("m60e4", "M60E4", LMG, P, COD4_LMG, "hud_icon_m60e4"),
    c4("m40a3", "M40A3", SNIPER, P, "acog", "hud_icon_m40a3"),
    c4("m21", "M21", SNIPER, P, "acog", "hud_icon_m14_scoped"),
    c4("dragunov", "Dragunov", SNIPER, P, "acog", "hud_icon_dragunov"),
    c4("remington700", "R700", SNIPER, P, "acog", "hud_icon_remington700"),
    c4("barrett", "Barrett .50cal", SNIPER, P, "acog", "hud_icon_barrett50cal"),
    c4("winchester1200", "W1200", CQB, P, "reflex grip", "hud_icon_winchester_1200"),
    c4("m1014", "M1014", CQB, P, "reflex grip", "hud_icon_benelli_m4"),
    c4("beretta", "M9", PISTOL, S, "silencer", "hud_icon_m9beretta"),
    c4("usp", "USP .45", PISTOL, S, "silencer", "hud_icon_usp_45"),
    c4("colt45", "M1911 .45", PISTOL, S, "silencer", "hud_icon_colt_45"),
    c4("deserteagle", "Desert Eagle", PISTOL, S, "", "hud_icon_desert_eagle"),
    c4("rpg", "RPG-7", LAUNCHER, S, "", "hud_icon_rpg"),
];

/// The first statstable index of Call of Duty 4's guns (see
/// `MW3_FIRST_INDEX`).
pub const COD4_FIRST_INDEX: i32 = 700;

/// bo2mp: the game name of the mod's guns (`CODLEGACY_T6_MOD`). They are Black
/// Ops II weapons from the mod's own zone, so their references have no
/// prefix (`cheytac`, given as `cheytac_mp`).
pub const MOD_GAME: &str = "t6mod";

const fn md(
    gun: &'static str,
    name: &'static str,
    group: &'static str,
    slot: &'static str,
    attachments: &'static str,
    icon: &'static str,
) -> GuestGun {
    GuestGun { game: MOD_GAME, gun, name, group, slot, attachments, icon }
}

const GRENADE: &str = "weapon_grenade";

/// bo2mp: the @leetinservers mod's guns (`mod.ff`, `CODLEGACY_T6_MOD`): its
/// statstable rows for the weapons Black Ops II lacks, with its names,
/// groups, slots, attachments (Black Ops II's own words) and
/// Create-a-Class pictures (`icon`, in its archive). Its rows are numbered
/// from `MOD_FIRST_INDEX`. One is listed only where no gun ported from the
/// games is the same gun, and never one of `MOD_HIDDEN`.
pub const MOD: &[GuestGun] = &[
    md("deagle", "Gold Desert Eagle", PISTOL, S, "", "menu_mp_weapons_deagle"),
    md("mauser", "Mauser C96", PISTOL, S, "", "menu_mp_weapons_mc96"),
    md("boomhilda", "Boomhilda", PISTOL, S, "", "menu_mp_weapons_bh"),
    md("glock", "G18", PISTOL, S, "", "menu_mp_weapons_glock"),
    md("magnum", ".44 Magnum", PISTOL, S, "tacknife", "menu_mp_weapons_magnum"),
    md("mp5k", "MP5K", SMG, P, "reflex silencer_smg fastads_smg extbarrel extclip dualclip", "menu_mp_weapons_mp5"),
    md("ak47", "AK-47", AR, P, "acog extclip dualclip reflex gl sf", "menu_mp_weapons_ak47"),
    md("galil", "Galil", AR, P, "acog extclip dualclip reflex gl sf", "menu_mp_weapons_galil"),
    md("m16", "M16", AR, P, "acog extclip dualclip reflex gl sf", "menu_mp_weapons_m16"),
    md("fn_scar", "SCAR", AR, P, "", "menu_mp_weapons_fnscar"),
    md("koshka", "Koshka", SNIPER, P, "", "menu_mp_weapons_koshka"),
    md("svg", "SVG-100", SNIPER, P, "", "menu_mp_weapons_svg"),
    md("katt", "KATT-AMR", SNIPER, P, "", "menu_mp_weapons_katt"),
    md("k98", "KAR98K", SNIPER, P, "fmj extclip_sniper", "menu_mp_weapons_k98"),
    md("halo_sniper", "SRS99-AM", SNIPER, P, "", "menu_mp_weapons_halo_sniper"),
    md("scav", "Scavenger", SNIPER, P, "", "menu_mp_weapons_scav"),
    md("iw5_as50", "AS50", SNIPER, P, "dualclip steadyaim_smg fmj", "menu_mp_weapons_iw5_as50"),
    md("usr", "USR", SNIPER, P, "dualclip steadyaim_smg fmj", "menu_mp_weapons_usr"),
    md("ebr", "M21 EBR", SNIPER, P, "dualclip steadyaim_smg fmj", "menu_mp_weapons_ebr"),
    md("m40a3", "M40A3", SNIPER, P, "steadyaim_smg fmj", "menu_mp_weapons_m40a3"),
    md("awp", "AWP | Dragon Lore", SNIPER, P, "fmj", "menu_mp_weapons_awp"),
    md("psg", "PSG1", SNIPER, P, "dualclip steadyaim_smg fmj", "menu_mp_weapons_psg"),
    md("barret", "Barrett .50cal", SNIPER, P, "dualclip steadyaim_smg fmj", "menu_mp_weapons_m82a"),
    md("drag", "SVO", SNIPER, P, "dualclip steadyaim_smg fmj", "menu_mp_weapons_drag"),
    md("cheytac", "Intervention", SNIPER, P, "dualclip steadyaim_smg acog_sniper ir fmj", "menu_mp_weapons_cheytac"),
    md("msr", "MSR", SNIPER, P, "dualclip steadyaim_smg acog_sniper fmj", "menu_mp_weapons_msr"),
    md("l96a", "L96A1", SNIPER, P, "dualclip steadyaim_smg fmj", "menu_mp_weapons_l96a"),
    md("locus", "Locus", SNIPER, P, "dualclip steadyaim_smg fmj", "menu_mp_weapons_locus"),
    md("lynx", "Lynx", SNIPER, P, "dualclip steadyaim_smg fmj", "menu_mp_weapons_lynx"),
    md("kbs", "KBS Longbow", SNIPER, P, "dualclip steadyaim_smg fmj", "menu_mp_weapons_kbs"),
    md("wa2000", "CNG", SNIPER, P, "fmj", "menu_mp_weapons_w2k"),
    md("mors", "Mors", SNIPER, P, "is ir fmj", "menu_mp_weapons_mors"),
    md("ptrs", "PTRS-41", SNIPER, P, "steadyaim_smg fmj", "menu_mp_weapons_ptrs"),
    md("spas", "SPAS 12", CQB, P, "reflex mms dualclip_segmented steadyaim fastads", "menu_mp_weapons_spas"),
    md("stake", "Stakeout", CQB, P, "dualclip_segmented steadyaim fastads", "menu_mp_weapons_stake"),
    md("m1887", "Model 1887", CQB, P, "dualclip_segmented steadyaim fastads", "menu_mp_weapons_1887"),
    md("freedom_qcb", "Freedom", CQB, P, "", "menu_mp_weapons_freedom"),
    md("fn_tacshotty", "Tactical Shotgun", CQB, P, "", "menu_mp_weapons_fntac"),
    md("mgl", "MGL", LAUNCHER, S, "", "menu_mp_weapons_mm1"),
    md("china_lake", "China Lake", LAUNCHER, S, "", "menu_mp_weapons_china"),
    md("slipgun", "Sliquifier", SPECIAL, P, "", "menu_mp_weapons_slip"),
    md("slowgun", "Paralyzer", SPECIAL, P, "", "menu_mp_weapons_slow"),
    md("raygun_mark2", "Ray Gun Mark II", SPECIAL, P, "", "menu_mp_weapons_rgunmk2"),
    md("lightstaff", "Staff of Lightning", SPECIAL, S, "", "menu_mp_lightstaff"),
    md("icestaff", "Staff of Ice", SPECIAL, S, "", "menu_mp_icestaff"),
    md("windstaff", "Staff of Wind", SPECIAL, S, "", "menu_mp_windstaff"),
    md("firestaff", "Staff of Fire", SPECIAL, S, "", "menu_mp_firestaff"),
    md("bk", "Butterfly Knife", SPECIAL, S, "", "menu_mp_weapons_bk"),
    md("karambit", "Karambit | Lore", SPECIAL, S, "", "menu_mp_weapons_karambit"),
    md("barber", "Barber", SPECIAL, S, "", "menu_mp_weapons_barber"),
    md("rpgdud", "Danger Close", SPECIAL, S, "", "menu_mp_weapons_rpgd"),
    md("m9", "M9 Bayonet | Marble Fade", SPECIAL, S, "", "menu_mp_weapons_m9"),
    md("diamond_pickaxe", "Diamond Pickaxe", SPECIAL, S, "", "menu_mp_weapons_pickaxe"),
    md("exo_launcher", "Exo Launcher", GRENADE, "primarygrenade", "", "hud_s1_semtex"),
    md("iw5_jammer", "Scrambler", GRENADE, "specialgrenade", "", "hud_iw5_jammer"),
    md("iw5_trophy", "Trophy System IW5", GRENADE, "specialgrenade", "", "hud_iw5_trophy"),
];

/// bo2mp: what each gun is, in a line, by its statstable reference (its
/// Create-a-Class description, with the game it comes from after it).
const GUN_LINES: &[(&str, &str)] = &[
    ("bo1_mp5k", "Fully automatic. High rate of fire, deadly up close."),
    ("bo1_skorpion", "Fully automatic. Extremely fast firing, small magazine."),
    ("bo1_mac11", "Fully automatic. Very fast firing, high recoil."),
    ("bo1_ak74u", "Fully automatic. Hard-hitting compact rifle."),
    ("bo1_uzi", "Fully automatic. Steady and fast firing."),
    ("bo1_pm63", "Fully automatic. Very fast firing, short range."),
    ("bo1_mpl", "Fully automatic. Accurate, fast firing."),
    ("bo1_spectre", "Fully automatic. Fast firing with a long magazine."),
    ("bo1_kiparis", "Fully automatic. Very high rate of fire."),
    ("bo1_m16", "Three-round burst. Accurate and powerful at range."),
    ("bo1_enfield", "Fully automatic. Low recoil, high rate of fire."),
    ("bo1_m14", "Semi-automatic. High damage marksman rifle."),
    ("bo1_famas", "Fully automatic. Very high rate of fire, low recoil."),
    ("bo1_galil", "Fully automatic. Stable with a large magazine."),
    ("bo1_aug", "Fully automatic. Very accurate with little recoil."),
    ("bo1_fnfal", "Semi-automatic. High damage per shot at any range."),
    ("bo1_ak47", "Fully automatic. High damage with heavy recoil."),
    ("bo1_commando", "Fully automatic. Fast firing carbine with low recoil."),
    ("bo1_g11", "Three-round burst. Extremely fast, accurate burst."),
    ("bo1_rottweil72", "Double-barrel. Two devastating shots."),
    ("bo1_ithaca", "Pump action. Powerful at close range."),
    ("bo1_spas", "Pump action. Devastating at close range."),
    ("bo1_hs10", "Semi-automatic. Fast firing, dual-wieldable."),
    ("bo1_hk21", "Fully automatic. High damage, large belt."),
    ("bo1_rpk", "Fully automatic. Accurate with a large magazine."),
    ("bo1_m60", "Fully automatic. Powerful with a large belt."),
    ("bo1_stoner63", "Fully automatic. Fast firing and accurate."),
    ("bo1_dragunov", "Semi-automatic. Powerful, accurate marksman rifle."),
    ("bo1_wa2000", "Semi-automatic. Fast follow-up shots."),
    ("bo1_l96a1", "Bolt action. One shot kill to the upper body."),
    ("bo1_psg1", "Semi-automatic. Low recoil, fast follow-ups."),
    ("bo1_asp", "Semi-automatic. Compact pistol, dual-wieldable."),
    ("bo1_m1911", "Semi-automatic. Reliable .45 pistol."),
    ("bo1_makarov", "Semi-automatic. Fast firing pistol."),
    ("bo1_python", "Revolver. High damage, six rounds."),
    ("bo1_cz75", "Semi-automatic. Fast firing, can go full auto."),
    ("bo1_m72_law", "Rocket launcher. Free-fire, fast to aim."),
    ("bo1_rpg", "Rocket launcher. Free-fire, wide blast."),
    ("bo1_strela", "Lock-on launcher. Anti-air only."),
    ("bo1_china_lake", "Pump-action grenade launcher."),
    ("bo1_crossbow_explosive", "Fires explosive bolts that stick to targets."),
    ("bo1_knife_ballistic", "Fires the blade. One shot kill."),
    ("mw2_m4", "Fully automatic. Balanced, steady carbine."),
    ("mw2_famas", "Three-round burst. Fast burst at close to mid range."),
    ("mw2_scar", "Fully automatic. Hard-hitting 7.62mm battle rifle."),
    ("mw2_tavor", "Fully automatic. High rate of fire, moderate recoil."),
    ("mw2_fal", "Semi-automatic. High damage per shot at any range."),
    ("mw2_m16", "Three-round burst. Accurate and powerful at range."),
    ("mw2_masada", "Fully automatic. Very low recoil and highly accurate."),
    ("mw2_fn2000", "Fully automatic. Very high rate of fire."),
    ("mw2_ak47", "Fully automatic. High damage with heavy recoil."),
    ("mw2_mp5k", "Fully automatic. High rate of fire, deadly up close."),
    ("mw2_ump45", "Fully automatic. High damage, low rate of fire."),
    ("mw2_kriss", "Fully automatic. Extremely fast firing, low recoil."),
    ("mw2_p90", "Fully automatic. Very large magazine, fast firing."),
    ("mw2_uzi", "Fully automatic. Very high rate of fire."),
    ("mw2_sa80", "Fully automatic. Very accurate light machine gun."),
    ("mw2_rpd", "Fully automatic. Fast firing with a large drum."),
    ("mw2_mg4", "Fully automatic. Accurate with steady recoil."),
    ("mw2_aug", "Fully automatic. Very accurate heavy-barrel rifle."),
    ("mw2_m240", "Fully automatic. Hard-hitting with a large belt."),
    ("mw2_cheytac", "Bolt action. One shot to the upper body."),
    ("mw2_barrett", "Semi-automatic .50 calibre. Devastating at long range."),
    ("mw2_wa2000", "Semi-automatic. Fast follow-up shots."),
    ("mw2_m21", "Semi-automatic. High damage marksman rifle."),
    ("mw2_spas12", "Pump action. Devastating at close range."),
    ("mw2_aa12", "Fully automatic. Rapid fire shotgun."),
    ("mw2_striker", "Semi-automatic. Large drum, fast follow-ups."),
    ("mw2_m1014", "Semi-automatic. Fast firing combat shotgun."),
    ("mw2_ranger", "Double-barrel. Both barrels at once, one-shot kill."),
    ("mw2_model1887", "Lever action. Powerful, long-reaching shotgun."),
    ("mw2_pp2000", "Fully automatic machine pistol."),
    ("mw2_glock", "Fully automatic machine pistol."),
    ("mw2_beretta393", "Three-round burst machine pistol."),
    ("mw2_tmp", "Fully automatic machine pistol."),
    ("mw2_usp", "Semi-automatic. Reliable .45 pistol."),
    ("mw2_coltanaconda", "Revolver. High damage, slow firing."),
    ("mw2_beretta", "Semi-automatic. Fast firing sidearm."),
    ("mw2_deserteagle", "Semi-automatic. High damage .50 calibre pistol."),
    ("mw2_at4", "Rocket launcher. Locks on to vehicles."),
    ("mw2_rpg", "Rocket launcher. Free-fire, wide blast."),
    ("mw2_stinger", "Lock-on launcher. Anti-air only."),
    ("mw2_javelin", "Lock-on launcher. Top-attack missile."),
    ("mw2_m79", "Grenade launcher. Explosive rounds."),
    ("gh_ak12", "Fully automatic. Versatile rifle with steady recoil."),
    ("gh_sc2010", "Fully automatic. Low recoil, fast handling."),
    ("gh_honeybadger", "Fully automatic. Integrally suppressed, deadly up close."),
    ("gh_bren", "Fully automatic. High damage, large magazine."),
    ("gh_msbs", "Two-round burst. Fast, precise burst."),
    ("gh_arx160", "Fully automatic. Fast firing with low recoil."),
    ("gh_fads", "Fully automatic. Very high rate of fire, large magazine."),
    ("gh_r5rgp", "Fully automatic. Low recoil and consistent at range."),
    ("gh_kriss", "Fully automatic. Extremely fast firing, low recoil."),
    ("gh_pp19", "Fully automatic. Huge helical magazine."),
    ("gh_vepr", "Fully automatic. Slow firing but hard-hitting."),
    ("gh_microtar", "Fully automatic. Fast firing, good accuracy."),
    ("gh_cbjms", "Fully automatic. Armour-piercing rounds, long range."),
    ("gh_k7", "Fully automatic. Integrally suppressed, fast firing."),
    ("gh_m27", "Fully automatic. Accurate and quick to aim."),
    ("gh_ameli", "Fully automatic. Very high rate of fire."),
    ("gh_lsat", "Fully automatic. Steady fire, huge magazine."),
    ("gh_kac", "Fully automatic. Huge magazine, fired from the hip."),
    ("gh_vks", "Bolt action. Heavy-calibre, one shot kill."),
    ("gh_gm6", "Semi-automatic heavy rifle. Devastating power."),
    ("gh_usr", "Bolt action. Fast-handling, one shot to the upper body."),
    ("gh_l115a3", "Bolt action. Long range, one shot kill."),
    ("gh_imbel", "Semi-automatic. Fast firing marksman rifle."),
    ("gh_svu", "Semi-automatic. Bullpup marksman rifle."),
    ("gh_g28", "Semi-automatic. Accurate marksman rifle."),
    ("gh_mk14", "Semi-automatic. High damage marksman rifle."),
    ("gh_mts255", "Revolving. Powerful semi-automatic shotgun."),
    ("gh_fp6", "Pump action. Powerful with a long range."),
    ("gh_maul", "Semi-automatic. Fast firing with tight spread."),
    ("gh_uts15", "Semi-automatic. Bullpup shotgun with tight spread."),
    ("gh_p226", "Semi-automatic. Accurate, fast firing."),
    ("gh_m9a1", "Semi-automatic. Fast firing sidearm."),
    ("gh_mp443", "Semi-automatic. Fast firing pistol."),
    ("gh_magnum", "Revolver. High damage, slow firing."),
    ("gh_pdw", "Fully automatic. Compact, fast firing."),
    ("gh_panzerfaust3", "Rocket launcher. Free-fire, powerful blast."),
    ("gh_mk32", "Semi-automatic grenade launcher."),
    ("gh_maaws", "Rocket launcher. Free-fire or lock-on."),
    ("gh_rgm", "Grenade launcher. Fast firing."),
    ("mw3_m4", "Fully automatic. Balanced, steady carbine."),
    ("mw3_m16", "Three-round burst. Accurate and powerful at range."),
    ("mw3_scar", "Fully automatic. Accurate 5.56mm rifle."),
    ("mw3_cm901", "Fully automatic. Powerful with moderate recoil."),
    ("mw3_type95", "Three-round burst. Two-burst kill at most ranges."),
    ("mw3_g36c", "Fully automatic. Fast firing, accurate carbine."),
    ("mw3_acr", "Fully automatic. Very low recoil and highly accurate."),
    ("mw3_mk14", "Semi-automatic. High damage marksman rifle."),
    ("mw3_ak47", "Fully automatic. High damage with heavy recoil."),
    ("mw3_fad", "Fully automatic. Very high rate of fire, large magazine."),
    ("mw3_mp5", "Fully automatic. Accurate and controllable."),
    ("mw3_ump45", "Fully automatic. High damage, low rate of fire."),
    ("mw3_pp90m1", "Fully automatic. Large magazine, fast firing."),
    ("mw3_p90", "Fully automatic. Very large magazine, fast firing."),
    ("mw3_m9", "Fully automatic. Extremely fast firing."),
    ("mw3_mp7", "Fully automatic. Fast firing with low recoil."),
    ("mw3_sa80", "Fully automatic. Very accurate light machine gun."),
    ("mw3_mg36", "Fully automatic. Accurate with a large drum."),
    ("mw3_pecheneg", "Fully automatic. Hard-hitting with a large belt."),
    ("mw3_mk46", "Fully automatic. Fast firing light machine gun."),
    ("mw3_m60", "Fully automatic. Powerful with a large belt."),
    ("mw3_barrett", "Semi-automatic .50 calibre. Devastating at long range."),
    ("mw3_l96a1", "Bolt action. One shot kill to the upper body."),
    ("mw3_dragunov", "Semi-automatic. Powerful, accurate marksman rifle."),
    ("mw3_as50", "Semi-automatic .50 calibre. Fast and powerful."),
    ("mw3_rsass", "Semi-automatic. Accurate with low recoil."),
    ("mw3_msr", "Bolt action. One shot kill, fast handling."),
    ("mw3_usas12", "Fully automatic. Rapid fire shotgun."),
    ("mw3_ksg", "Pump action. Fires a single tight slug-like spread."),
    ("mw3_spas12", "Pump action. Devastating at close range."),
    ("mw3_aa12", "Fully automatic. Rapid fire shotgun."),
    ("mw3_striker", "Semi-automatic. Large drum, fast follow-ups."),
    ("mw3_1887", "Lever action. Powerful, long-reaching shotgun."),
    ("mw3_fmg9", "Fully automatic. Folding machine pistol."),
    ("mw3_g18", "Fully automatic machine pistol."),
    ("mw3_mp9", "Fully automatic machine pistol."),
    ("mw3_skorpion", "Fully automatic. Extremely fast firing, small magazine."),
    ("mw3_usp45", "Semi-automatic. Reliable .45 pistol."),
    ("mw3_p99", "Semi-automatic. Accurate pistol."),
    ("mw3_mp412", "Revolver. High damage, slow firing."),
    ("mw3_44magnum", "Revolver. High damage, slow firing."),
    ("mw3_fnfiveseven", "Semi-automatic. Large magazine, fast firing."),
    ("mw3_deserteagle", "Semi-automatic. High damage .50 calibre pistol."),
    ("mw3_smaw", "Rocket launcher. Free-fire or lock-on."),
    ("mw3_stinger", "Lock-on launcher. Anti-air only."),
    ("mw3_xm25", "Airburst grenade launcher."),
    ("mw3_m320", "Grenade launcher. Explosive rounds."),
    ("mw3_rpg", "Rocket launcher. Free-fire, wide blast."),
    ("waw_stg44", "Fully automatic. The first assault rifle; slow, steady fire."),
    ("waw_svt40", "Semi-automatic. Accurate self-loading rifle."),
    ("waw_gewehr43", "Semi-automatic. Accurate self-loading rifle."),
    ("waw_m1garand", "Semi-automatic. Powerful eight-round rifle."),
    ("waw_m1carbine", "Semi-automatic. Light and fast firing."),
    ("waw_thompson", "Fully automatic. Reliable and hard-hitting."),
    ("waw_mp40", "Fully automatic. Steady, accurate fire."),
    ("waw_type100smg", "Fully automatic. Fast firing, low damage."),
    ("waw_ppsh", "Fully automatic. Extremely fast firing, drum magazine."),
    ("waw_bar", "Fully automatic. Powerful with a small magazine."),
    ("waw_fg42", "Fully automatic. Light and fast firing."),
    ("waw_type99lmg", "Fully automatic. Accurate light machine gun."),
    ("waw_dp28", "Fully automatic. Drum-fed, steady fire."),
    ("waw_mg42", "Fully automatic. Extremely fast firing."),
    ("waw_30cal", "Fully automatic. Heavy, hard-hitting machine gun."),
    ("waw_springfield", "Bolt action. Accurate, one shot to the upper body."),
    ("waw_kar98k", "Bolt action. Accurate, one shot to the upper body."),
    ("waw_mosinrifle", "Bolt action. Accurate, one shot to the upper body."),
    ("waw_type99rifle", "Bolt action. Accurate, one shot to the upper body."),
    ("waw_ptrs41", "Semi-automatic anti-tank rifle. One shot kill anywhere."),
    ("waw_shotgun", "Pump action. Devastating at close range."),
    ("waw_doublebarreledshotgun", "Double-barrel. Two powerful shots."),
    ("waw_colt", "Semi-automatic. Reliable .45 pistol."),
    ("waw_nambu", "Semi-automatic. Light pistol."),
    ("waw_tokarev", "Semi-automatic. Fast firing pistol."),
    ("waw_walther", "Semi-automatic. Accurate pistol."),
    ("waw_357magnum", "Revolver. High damage, slow firing."),
    ("waw_bazooka", "Rocket launcher. Free-fire, slow rockets."),
    ("cod4_m16", "Three-round burst. Accurate and powerful at range."),
    ("cod4_ak47", "Fully automatic. High damage with heavy recoil."),
    ("cod4_m4", "Fully automatic. Balanced, steady carbine."),
    ("cod4_g3", "Semi-automatic. Hard-hitting battle rifle."),
    ("cod4_g36c", "Fully automatic. Fast firing, accurate carbine."),
    ("cod4_m14", "Semi-automatic. High damage marksman rifle."),
    ("cod4_mp44", "Fully automatic. Slow firing but very powerful."),
    ("cod4_mp5", "Fully automatic. Accurate and controllable."),
    ("cod4_skorpion", "Fully automatic. Extremely fast firing, small magazine."),
    ("cod4_uzi", "Fully automatic. Very high rate of fire."),
    ("cod4_ak74u", "Fully automatic. Hard-hitting compact rifle."),
    ("cod4_p90", "Fully automatic. Very large magazine, fast firing."),
    ("cod4_saw", "Fully automatic. Large belt, steady fire."),
    ("cod4_rpd", "Fully automatic. Fast firing with a large drum."),
    ("cod4_m60e4", "Fully automatic. Powerful with a large belt."),
    ("cod4_m40a3", "Bolt action. One shot kill to the upper body."),
    ("cod4_m21", "Semi-automatic. High damage marksman rifle."),
    ("cod4_dragunov", "Semi-automatic. Powerful, accurate marksman rifle."),
    ("cod4_remington700", "Bolt action. Accurate, one shot to the upper body."),
    ("cod4_barrett", "Semi-automatic .50 calibre. Devastating at long range."),
    ("cod4_winchester1200", "Pump action. Powerful at close range."),
    ("cod4_m1014", "Semi-automatic. Fast firing combat shotgun."),
    ("cod4_beretta", "Semi-automatic. Fast firing sidearm."),
    ("cod4_usp", "Semi-automatic. Reliable .45 pistol."),
    ("cod4_colt45", "Semi-automatic. Reliable .45 pistol."),
    ("cod4_deserteagle", "Semi-automatic. High damage .50 calibre pistol."),
    ("cod4_rpg", "Rocket launcher. Free-fire, wide blast."),
    ("aw_bal27", "Fully automatic. Balanced, versatile rifle."),
    ("aw_ak12", "Fully automatic. Versatile rifle with steady recoil."),
    ("aw_arx160", "Fully automatic. Fast firing with low recoil."),
    ("aw_hbra3", "Fully automatic. Low recoil, accurate at range."),
    ("aw_himar", "Three-round burst. Fast burst rifle."),
    ("aw_m182spr", "Semi-automatic. High damage marksman rifle."),
    ("aw_epm3", "Semi-automatic. Powerful energy rifle."),
    ("aw_kf5", "Fully automatic. Fast firing, accurate."),
    ("aw_mp11", "Fully automatic. Very fast firing."),
    ("aw_asm1", "Fully automatic. Hard-hitting, large magazine."),
    ("aw_sn6", "Fully automatic. Balanced and steady."),
    ("aw_hmr9", "Fully automatic. Fast firing with tight spread."),
    ("aw_sac3", "Fully automatic. Dual-wielded, very fast firing."),
    ("aw_lsat", "Fully automatic. Fast firing, large magazine."),
    ("aw_exoxmg", "Fully automatic. Dual-wielded, devastating fire."),
    ("aw_asaw", "Fully automatic. Very high rate of fire."),
    ("aw_em1", "Continuous energy beam. Never needs a reload."),
    ("aw_mors", "Bolt action railgun. One shot kill anywhere."),
    ("aw_thor", "Semi-automatic. Huge damage, slow fire."),
    ("aw_gm6", "Semi-automatic heavy rifle. Devastating power."),
    ("aw_m990", "Two-shot explosive rifle. Mark, then detonate."),
    ("aw_uts19", "Pump action. Tight spread, long reach."),
    ("aw_rhino", "Semi-automatic. Large magazine, fast firing."),
    ("aw_maul", "Semi-automatic. Bullpup shotgun with tight spread."),
    ("aw_titan45", "Semi-automatic. Hard-hitting pistol."),
    ("aw_pbw", "Fully automatic. Compact, fast firing."),
    ("aw_rw1", "Single shot. Devastating one-round pistol."),
    ("aw_vbr", "Fully automatic. Fast firing burst pistol."),
    ("aw_mahem", "Energy launcher. Free-fire or lock-on."),
    ("aw_stingerm7", "Lock-on launcher. Anti-air and anti-vehicle."),
    ("aw_maaws", "Rocket launcher. Free-fire or lock-on."),
    ("aw_exocrossbow", "Fires explosive bolts that stick to targets."),
    ("aw_microdronelauncher", "Fires micro-drones that seek targets."),
    ("bo3_ar_standard", "Fully automatic. Balanced all-round rifle."),
    ("bo3_ar_accurate", "Fully automatic. Very low recoil and accurate."),
    ("bo3_ar_fastburst", "Three-round burst. Fast, deadly burst."),
    ("bo3_ar_cqb", "Fully automatic. Very high rate of fire."),
    ("bo3_ar_damage", "Fully automatic. High damage, slower fire."),
    ("bo3_ar_marksman", "Semi-automatic. Powerful, accurate shots."),
    ("bo3_ar_longburst", "Four-round burst. Precise long-range burst."),
    ("bo3_smg_standard", "Fully automatic. Balanced, versatile SMG."),
    ("bo3_smg_versatile", "Fully automatic. Very high rate of fire."),
    ("bo3_smg_capacity", "Fully automatic. Large magazine, steady fire."),
    ("bo3_smg_fastfire", "Fully automatic. Extremely fast firing."),
    ("bo3_smg_burst", "Burst fire. Fast, accurate burst."),
    ("bo3_smg_longrange", "Fully automatic. Effective at longer range."),
    ("bo3_lmg_light", "Fully automatic. Balanced light machine gun."),
    ("bo3_lmg_cqb", "Fully automatic. Fast firing, good mobility."),
    ("bo3_lmg_slowfire", "Fully automatic. Slow firing, very high damage."),
    ("bo3_lmg_heavy", "Fully automatic. High damage, long range."),
    ("bo3_sniper_fastbolt", "Bolt action. Fast handling, one shot to the upper body."),
    ("bo3_sniper_fastsemi", "Semi-automatic. Fast follow-up shots."),
    ("bo3_sniper_powerbolt", "Bolt action. Heavy hitting, one shot kill."),
    ("bo3_sniper_chargeshot", "Charge-shot rifle. Hold to charge up to three shots."),
    ("bo3_shotgun_pump", "Pump action. Powerful at close range."),
    ("bo3_shotgun_semiauto", "Semi-automatic. Fast follow-up shots."),
    ("bo3_shotgun_fullauto", "Fully automatic. Rapid fire shotgun."),
    ("bo3_shotgun_precision", "Pump action. Tight spread, long reach."),
    ("bo3_pistol_standard", "Semi-automatic. Balanced sidearm."),
    ("bo3_pistol_burst", "Three-round burst pistol."),
    ("bo3_pistol_fullauto", "Fully automatic machine pistol."),
    ("bo3_launcher_standard", "Rocket launcher. Free-fire or lock-on."),
    ("bo3_launcher_lockonly", "Lock-on launcher. Anti-scorestreak."),
    ("jup_jp12_pi_mike93", "Semi-automatic. Fast firing, can burst."),
    ("jup_jp36_ar_anov94", "Fully automatic. Fast opening pair, then steady."),
    ("jup_jp19_ar_acharlie", "Fully automatic. Balanced all-rounder."),
    ("jup_jp34_ar_balpha27", "Fully automatic. Quick firing, light recoil."),
    ("jup_jp05_ar_foxtrot2000", "Three-round burst. Fast bursts, bullpup."),
    ("jup_jp08_ar_qbravo97", "Three-round burst. Accurate bursts at range."),
    ("jup_jp44_ar_stango44", "Fully automatic. Slow firing, hard hitting."),
    ("jup_jp19_br_acharlie450", "Fully automatic. Heavy calibre, hard hitting."),
    ("jup_jp03_br_aromeo200", "Fully automatic. Heavy calibre, steady."),
    ("jup_jp37_dm_kappa98", "Bolt action. Lethal with a headshot."),
    ("jup_jp06_lm_pkilop", "Fully automatic. Belt-fed, sustained fire."),
    ("jup_jp08_lm_qbravo95lsw", "Fully automatic. Bullpup, high mobility."),
    ("jup_jp38_sh_spapa12", "Semi-automatic. Fast follow-up shots."),
    ("jup_jp41_sm_kpappa50", "Fully automatic. Large top-loaded magazine."),
    ("jup_jp25_sm_talpham", "Fully automatic. Very fast firing, bullpup."),
    ("jup_jp10_sn_cdelta50", "Semi-automatic. Anti-materiel .50 calibre."),
    ("jup_jp17_sn_hsierra", "Bolt action. Powerful .50 calibre."),
    ("jup_iw9_ar_mike4", "Fully automatic. Reliable all-rounder."),
    ("jup_iw9_ar_mike16", "Three-round burst. Accurate at range."),
    ("jup_iw9_ar_akilo", "Fully automatic. Hard hitting, heavy recoil."),
    ("jup_iw9_ar_akilo105", "Fully automatic. Fast firing, light recoil."),
    ("jup_iw9_ar_akilo74", "Fully automatic. Compact, quick handling."),
    ("jup_iw9_ar_kilo53", "Fully automatic. Steady and accurate."),
    ("jup_iw9_ar_golf3", "Fully automatic. Battle rifle, hard hitting."),
    ("jup_iw9_ar_scharlie", "Fully automatic. Balanced and controllable."),
    ("jup_iw9_ar_mcbravo", "Fully automatic. Very fast firing."),
    ("jup_iw9_ar_mcharlie", "Fully automatic. Compact with low recoil."),
    ("jup_iw9_ar_falpha", "Fully automatic. Bullpup, very high rate of fire."),
    ("jup_iw9_dm_soscar14", "Fully automatic. Heavy battle rifle."),
    ("jup_iw9_dm_scromeo", "Semi-automatic. Accurate marksman rifle."),
    ("jup_iw9_dm_mike14", "Semi-automatic. Fast firing marksman rifle."),
    ("jup_iw9_dm_stango25", "Semi-automatic. Quick, controllable marksman rifle."),
    ("jup_iw9_dm_pgolf1", "Semi-automatic. Precise at long range."),
    ("jup_iw9_dm_mike24", "Bolt action. Lethal with a headshot."),
    ("jup_iw9_sm_mpapa5", "Fully automatic. Accurate and steady."),
    ("jup_iw9_sm_mpapa7", "Fully automatic. Very fast firing."),
    ("jup_iw9_sm_papa90", "Fully automatic. Large top-loaded magazine."),
    ("jup_iw9_sm_victor", "Fully automatic. Extreme rate of fire."),
    ("jup_iw9_sm_beta", "Fully automatic. Huge helical magazine."),
    ("jup_iw9_sm_alpha57", "Fully automatic. Large magazine, steady fire."),
    ("jup_iw9_sm_mpapax", "Fully automatic. Fast firing and mobile."),
    ("jup_iw9_sm_acharlie45", "Fully automatic. Hard hitting up close."),
    ("jup_iw9_sm_acharlie9", "Fully automatic. Fast and compact."),
    ("jup_iw9_sm_aviktor", "Fully automatic. Agile with steady recoil."),
    ("jup_iw9_sm_apapa", "Fully automatic. Bullpup, low recoil."),
    ("jup_iw9_lm_rkilo", "Fully automatic. Drum fed, hard hitting."),
    ("jup_iw9_lm_kilo21", "Fully automatic. Belt-fed, sustained fire."),
    ("jup_iw9_lm_ngolf7", "Fully automatic. Belt-fed, fast firing."),
    ("jup_iw9_sh_vecho", "Semi-automatic. Magazine-fed shotgun."),
    ("jup_iw9_sh_mike1014", "Semi-automatic. Fast follow-up shots."),
    ("jup_iw9_sh_charlie725", "Double barrel. Devastating at close range."),
    ("jup_iw9_sh_tsierra12", "Semi-automatic. Bullpup shotgun."),
    ("jup_iw9_sn_alpha50", "Bolt action. Powerful .50 calibre."),
    ("jup_iw9_sn_mromeo", "Bolt action. Fast handling sniper rifle."),
    ("jup_iw9_sn_walpha", "Semi-automatic. Bullpup sniper rifle."),
    ("jup_iw9_pi_golf17", "Semi-automatic. Reliable sidearm."),
    ("jup_iw9_pi_decho", "Semi-automatic. Hand cannon."),
    ("jup_iw9_pi_swhiskey", "Revolver. Massive damage per shot."),
    ("jup_iw9_pi_papa220", "Semi-automatic. Hard hitting sidearm."),
    ("jup_iw9_pi_mike2011", "Semi-automatic. Fast firing sidearm."),
    ("jup_iw9_pi_tango9", "Fully automatic. Machine pistol."),
    ("h2_ak47", "Fully automatic. High damage with heavy recoil."),
    ("h2_fal", "Semi-automatic. High damage per shot at any range."),
    ("h2_famas", "Three-round burst. Fast burst at close to mid range."),
    ("h2_fn2000", "Fully automatic. Very high rate of fire."),
    ("h2_m16", "Three-round burst. Accurate and powerful at range."),
    ("h2_m4", "Fully automatic. Balanced, steady carbine."),
    ("h2_masada", "Fully automatic. Very low recoil and highly accurate."),
    ("h2_scar", "Fully automatic. Hard-hitting 7.62mm battle rifle."),
    ("h2_tavor", "Fully automatic. High rate of fire, moderate recoil."),
    ("h2_ak74u", "Fully automatic. Hard-hitting compact rifle."),
    ("h2_kriss", "Fully automatic. Extremely fast firing, low recoil."),
    ("h2_mp5k", "Fully automatic. High rate of fire, deadly up close."),
    ("h2_p90", "Fully automatic. Very large magazine, fast firing."),
    ("h2_ump45", "Fully automatic. High damage, low rate of fire."),
    ("h2_uzi", "Fully automatic. Very high rate of fire."),
    ("h2_aug", "Fully automatic. Very accurate heavy-barrel rifle."),
    ("h2_m240", "Fully automatic. Hard-hitting with a large belt."),
    ("h2_mg4", "Fully automatic. Accurate with steady recoil."),
    ("h2_rpd", "Fully automatic. Fast firing with a large drum."),
    ("h2_sa80", "Fully automatic. Very accurate light machine gun."),
    ("h2_barrett", "Semi-automatic .50 calibre. Devastating at long range."),
    ("h2_cheytac", "Bolt action. One shot to the upper body."),
    ("h2_m21", "Semi-automatic. High damage marksman rifle."),
    ("h2_m40a3", "Bolt action. One shot kill to the upper body."),
    ("h2_wa2000", "Semi-automatic. Fast follow-up shots."),
    ("h2_mors", "Bolt action. Heavy hitting, one shot kill."),
    ("h2_aa12", "Fully automatic. Rapid fire shotgun."),
    ("h2_m1014", "Semi-automatic. Fast firing combat shotgun."),
    ("h2_spas12", "Pump action. Devastating at close range."),
    ("h2_striker", "Semi-automatic. Large drum, fast follow-ups."),
    ("h2_winchester1200", "Pump action. Powerful at close range."),
    ("h2_ranger", "Double-barrel. Both barrels at once, one-shot kill."),
    ("h2_model1887", "Lever action. Powerful, long-reaching shotgun."),
    ("h2_pp2000", "Fully automatic machine pistol."),
    ("h2_glock", "Fully automatic machine pistol."),
    ("h2_beretta393", "Three-round burst machine pistol."),
    ("h2_tmp", "Fully automatic machine pistol."),
    ("h2_usp", "Semi-automatic. Reliable .45 pistol."),
    ("h2_coltanaconda", "Revolver. High damage, slow firing."),
    ("h2_m9", "Semi-automatic. Fast firing sidearm."),
    ("h2_deserteagle", "Semi-automatic. High damage .50 calibre pistol."),
    ("h2_colt45", "Semi-automatic. Reliable .45 pistol."),
    ("h2_boomhilda", "Semi-automatic. Hard-hitting scoped hand cannon."),
    ("h2_at4", "Rocket launcher. Locks on to vehicles."),
    ("h2_rpg", "Rocket launcher. Free-fire, wide blast."),
    ("h2_m79", "Grenade launcher. Explosive rounds."),
    ("iw7_m4", "Fully automatic. Accurate ballistic rifle for mid to long range."),
    ("iw7_sdfar", "Three-round burst. Hard-hitting energy rifle."),
    ("iw7_ar57", "Fully automatic. Light and steady from the hip."),
    ("iw7_fmg", "Fully automatic. Very high rate of fire energy rifle."),
    ("iw7_ake", "Fully automatic. Highest damage, slow rate of fire."),
    ("iw7_rvn", "Two-round burst. Fast, accurate bursts at range."),
    ("iw7_vr", "Fully automatic. Accurate energy rifle."),
    ("iw7_gauss", "Semi-automatic. Rail rifle, precise at long range."),
    ("iw7_arclassic", "Fully automatic. Classic rifle with a high rate of fire."),
    ("iw7_m1c", "Semi-automatic. Classic battle rifle, hard-hitting."),
    ("iw7_erad", "Fully automatic. Low recoil energy submachine gun."),
    ("iw7_fhr", "Fully automatic. Very high rate of fire, deadly up close."),
    ("iw7_crb", "Fully automatic. Double magazine for fast reloads."),
    ("iw7_ripper", "Fully automatic. Fast firing, steady at mid range."),
    ("iw7_crdb", "Fully automatic. Dual barrels and dual magazines."),
    ("iw7_mp28", "Fully automatic. High damage, steady rate of fire."),
    ("iw7_tacburst", "Three-round burst. Fast bursts up close."),
    ("iw7_ump45c", "Fully automatic. Classic submachine gun, high damage."),
    ("iw7_sdflmg", "Fully automatic. High-capacity energy machine gun."),
    ("iw7_mauler", "Fully automatic. High rate of fire, steady from the hip."),
    ("iw7_lmg03", "Fully automatic. Accurate energy machine gun, large magazine."),
    ("iw7_minilmg", "Fully automatic. Rotary machine gun."),
    ("iw7_unsalmg", "Fully automatic. Light machine gun with a short belt."),
    ("iw7_kbs", "Bolt action. One shot kill above the waist."),
    ("iw7_m8", "Semi-automatic. Energy sniper rifle."),
    ("iw7_ba50cal", "Semi-automatic .50 calibre. Devastating at long range."),
    ("iw7_cheytacc", "Bolt action. Classic rifle, high damage and precision."),
    ("iw7_devastator", "Semi-automatic. Drum fed, quick follow-up shots."),
    ("iw7_sdfshotty", "Fully automatic. Fastest firing energy shotgun."),
    ("iw7_sonic", "Pump action. Concussive sonic blasts."),
    ("iw7_mod2187", "Lever action. Powerful at close range."),
    ("iw7_longshot", "Semi-automatic. Long reaching for a shotgun."),
    ("iw7_spasc", "Pump action. Classic shotgun, long range for its kind."),
    ("iw7_emc", "Semi-automatic. Hard-hitting energy sidearm."),
    ("iw7_nrg", "Fully automatic. Energy sidearm that fires faster as it fires."),
    ("iw7_revolver", "Three-round burst. Burst-fire revolver."),
    ("iw7_udm45", "Fully automatic. Bullpup machine pistol."),
    ("iw7_mag", "Revolver. High damage, slow firing."),
    ("iw7_g18c", "Fully automatic machine pistol. Classic, fast firing."),
    ("iw7_lockon", "Rocket launcher. Locks on to killstreaks."),
    ("iw7_chargeshot", "Energy launcher. Plasma rounds with a large blast."),
    ("deagle", "Semi-automatic. High damage .50 calibre pistol."),
    ("mauser", "Semi-automatic. Classic broomhandle pistol."),
    ("boomhilda", "Upgraded Mauser. Fires explosive rounds."),
    ("glock", "Fully automatic machine pistol."),
    ("magnum", "Revolver. High damage, slow firing."),
    ("mp5k", "Fully automatic. High rate of fire, deadly up close."),
    ("ak47", "Fully automatic. High damage with heavy recoil."),
    ("galil", "Fully automatic. Stable with a large magazine."),
    ("m16", "Three-round burst. Accurate and powerful at range."),
    ("koshka", "Bolt action. Powerful, one shot to the upper body."),
    ("svg", "Bolt action. Heavy hitting, one shot kill."),
    ("katt", "Bolt action anti-materiel rifle. One shot kill."),
    ("k98", "Bolt action. Accurate, one shot to the upper body."),
    ("halo_sniper", "Semi-automatic anti-materiel rifle. One shot kill."),
    ("scav", "Explosive bolts that detonate after impact."),
    ("iw5_as50", "Semi-automatic .50 calibre. Fast and powerful."),
    ("usr", "Bolt action. Fast-handling, one shot to the upper body."),
    ("ebr", "Semi-automatic. High damage marksman rifle."),
    ("m40a3", "Bolt action. One shot kill to the upper body."),
    ("awp", "Bolt action. One shot kill to the upper body."),
    ("psg", "Semi-automatic. Low recoil, fast follow-ups."),
    ("barret", "Semi-automatic .50 calibre. Devastating at long range."),
    ("drag", "Bolt action. Powerful, one shot to the upper body."),
    ("cheytac", "Bolt action. One shot to the upper body."),
    ("msr", "Bolt action. One shot kill, fast handling."),
    ("l96a", "Bolt action. One shot kill to the upper body."),
    ("locus", "Bolt action. Fast handling, one shot to the upper body."),
    ("lynx", "Semi-automatic heavy rifle. Devastating power."),
    ("kbs", "Bolt action. One shot kill to the upper body."),
    ("wa2000", "Semi-automatic. Fast follow-up shots."),
    ("mors", "Bolt action railgun. One shot kill anywhere."),
    ("ptrs", "Semi-automatic anti-tank rifle. One shot kill anywhere."),
    ("spas", "Pump action. Devastating at close range."),
    ("stake", "Pump action. Powerful at close range."),
    ("m1887", "Lever action. Powerful, long-reaching shotgun."),
    ("freedom_qcb", "Semi-automatic. Fast firing shotgun."),
    ("fn_tacshotty", "Semi-automatic. Fast firing shotgun."),
    ("mgl", "Six-shot grenade launcher."),
    ("china_lake", "Pump-action grenade launcher."),
    ("slipgun", "Wonder weapon. Leaves a slick pool behind."),
    ("slowgun", "Wonder weapon. Slows targets down."),
    ("raygun_mark2", "Wonder weapon. Three-round burst of energy."),
    ("lightstaff", "Elemental staff. Chains lightning between targets."),
    ("icestaff", "Elemental staff. Freezes targets."),
    ("windstaff", "Elemental staff. Blasts targets away."),
    ("firestaff", "Elemental staff. Sets targets ablaze."),
    ("bk", "Knife. One hit kill up close."),
    ("karambit", "Knife. One hit kill up close."),
    ("barber", "Straight razor. One hit kill up close."),
    ("rpgdud", "Dud rocket. Hits without exploding."),
    ("m9", "Knife. One hit kill up close."),
    ("diamond_pickaxe", "Melee weapon. One hit kill up close."),
    ("exo_launcher", "Lethal. Explosive launched grenade."),
    ("iw5_jammer", "Tactical. Jams enemy radar nearby."),
    ("iw5_trophy", "Tactical. Destroys incoming explosives."),
];

/// The first statstable index of the mod's guns.
pub const MOD_FIRST_INDEX: i32 = 1100;

/// bo2mp: the mod's Fortnite guns (its SCAR and tactical shotgun): kept in
/// its numbering so saved classes stay put, never listed, boxed or given
/// to bots.
pub const MOD_HIDDEN: &[&str] = &["fn_scar", "fn_tacshotty"];

/// Whether a mod gun is one of `MOD_HIDDEN`.
fn mod_hidden(gun: &GuestGun) -> bool {
    gun.game == MOD_GAME && MOD_HIDDEN.contains(&gun.gun)
}

/// bo2mp: the mod's Zombies weapons (wonder weapons, Origins' pistol and
/// its upgrade, the staffs): Custom Games only in multiplayer, never in a
/// public match (Create-a-Class hides them there and the server refuses to
/// give them). By weapon name without `_mp` (`lightstaff_pap` has no row).
pub const MOD_CUSTOM_ONLY: &[&str] = &[
    "raygun_mark2",
    "slipgun",
    "slowgun",
    "lightstaff",
    "lightstaff_pap",
    "icestaff",
    "windstaff",
    "firestaff",
    "mauser",
    "boomhilda",
    "scav",
];

/// Whether a weapon (`raygun_mark2_mp+...`) or a statstable reference
/// (`raygun_mark2`) is one of the mod's Zombies weapons (`MOD_CUSTOM_ONLY`).
#[must_use]
pub fn is_custom_only(name: &str) -> bool {
    let name = name.split('+').next().unwrap_or(name).to_ascii_lowercase();
    let name = name.strip_suffix("_mp").unwrap_or(&name);
    MOD_CUSTOM_ONLY.contains(&name)
}

/// Whether the statstable item numbered `index` is one of the mod's
/// Zombies weapons.
#[must_use]
pub fn is_custom_only_index(index: i32) -> bool {
    reference_for_index(index).is_some_and(|r| game_for_index(index) == Some(MOD_GAME) && is_custom_only(&r))
}

/// bo2mp: now and then a bot carries one of the mod's guns instead of the
/// gun its class script picked (Black Ops II's bot scripts list only items
/// numbered below 256): a third of its weapons, by `seed` (bot, class and
/// slot), one of the loaded mod guns of the same group and slot; none of
/// the Zombies weapons in a public match.
pub fn bot_mod_gun(
    group: &str,
    slot: &str,
    seed: u32,
    public: bool,
    loaded: impl Fn(&GuestGun) -> bool,
) -> Option<&'static GuestGun> {
    if !available(MOD_GAME) || seed % 3 != 0 || group == GRENADE || group == SPECIAL {
        return None;
    }
    let pick: Vec<&'static GuestGun> = MOD
        .iter()
        .filter(|g| !mod_hidden(g) && g.group == group && g.slot == slot && !(public && is_custom_only(g.gun)) && loaded(g))
        .collect();
    (!pick.is_empty()).then(|| pick[(seed / 3) as usize % pick.len()])
}

/// Attachments the guest guns have and BO2's table lacks: reference, slot,
/// name, picture.
const EXTRA_ATTACHMENTS: &[(&str, &str, &str, &str)] = &[
    ("elbit", "top", "Red Dot Sight", "cac_mods_red_dot"),
    ("lps", "top", "Low Power Scope", "cac_mods_var_zoom"),
    ("upgradesight", "top", "Upgraded Iron Sights", "cac_mods_ironsight"),
    ("ft", "bottom", "Flamethrower", "t5:menu_mp_weapons_attach_ft_hirez"),
    ("auto", "trigger", "Full Auto Upgrade", "cac_mods_trigger_group"),
    ("snub", "muzzle", "Snub Nose", "cac_mods_barrel_extend"),
    ("speed", "trigger", "Speed Reloader", "cac_mods_dual_clip"),
    ("eotech", "top", "Holographic Sight", "cac_mods_holographic"),
    ("thermal", "top", "Thermal Scope", "cac_mods_dual_band"),
    ("heartbeat", "bottom", "Heartbeat Sensor", "cac_mods_rangefinder"),
    ("shotgun", "bottom", "Shotgun", "cac_mods_trigger_group"),
    ("xmags", "trigger", "Extended Mags", "cac_mods_extended_mag"),
    ("rof", "trigger", "Rapid Fire", "cac_mods_rapid_fire"),
    ("akimbo", "bottom", "Akimbo", "cac_mods_dual_wield"),
    ("tactical", "bottom", "Tactical Knife", "cac_mods_tact_knife"),
    // (Modern Warfare 3's.)
    ("hybrid", "top", "Hybrid Sight", "cac_mods_holographic"),
    ("hamrhybrid", "top", "HAMR Scope", "cac_mods_holographic"),
    ("vzscope", "top", "Variable Zoom", "cac_mods_var_zoom"),
    // (World at War's.)
    ("aperture", "top", "Aperture Sight", "cac_mods_ironsight"),
    ("telescopic", "top", "Telescopic Sight", "cac_mods_var_zoom"),
    ("scoped", "top", "Sniper Scope", "cac_mods_var_zoom"),
    ("bayonet", "bottom", "Bayonet", "cac_mods_tact_knife"),
    ("flash", "muzzle", "Flash Hider", "cac_mods_barrel_extend"),
    ("sawoff", "muzzle", "Sawed-Off", "cac_mods_barrel_extend"),
    // (Ghosts'.)
    ("tracker", "top", "Tracker Sight", "iw6:scope_targetfinder_360"),
    ("ironsight", "top", "Iron Sights", "iw6:iron_sight_360"),
    ("barrelrange", "muzzle", "Long Barrel", "cac_mods_barrel_extend"),
    ("barrelbored", "muzzle", "Bored Barrel", "cac_mods_barrel_extend"),
    ("flashsuppress", "muzzle", "Muzzle Brake", "iw6:muzzle_break_360"),
    ("firetypeburst", "trigger", "Burst Fire", "iw6:burst_fire_360"),
    ("firetypesingle", "trigger", "Single Fire", "iw6:single_fire_360"),
    ("firetypeauto", "trigger", "Full Auto", "iw6:full_auto_360"),
    ("ammoslug", "trigger", "Slug Rounds", "iw6:slug_round_360"),
];

/// The prefix a game's guns carry (`t5` -> `bo1_`).
#[must_use]
pub fn prefix(game: &str) -> &'static str {
    match game {
        "t5" => "bo1_",
        "iw4" => "mw2_",
        "iw5" => "mw3_",
        "iw6" => "gh_",
        "t4" => "waw_",
        "iw3" => "cod4_",
        "s1" => "aw_",
        "t7" => "bo3_",
        "jup" => "jup_",
        "h1" => "h2_",
        "iw7" => "iw7_",
        // (The Workshop packs' guns are Black Ops III weapons, named apart
        // by their pack's tag: `bo3_cw_ak47`.)
        g if is_workshop_game(g) => "bo3_",
        _ => "",
    }
}

/// Whether a weapon name is a guest gun's (`bo1_ak74u_mp+reflex`,
/// `gh_ak12_mp`): it starts with a game's prefix.
#[must_use]
pub fn is_guest_name(name: &str) -> bool {
    GAMES.iter().any(|g| name.starts_with(prefix(g)))
}

const GAMES: [&str; 17] = ["t5", "iw4", "iw5", "iw6", "t4", "iw3", "s1", "t7", "jup", "h1", "iw7", "t7cw", "t7vg", "t7mw", "t7cod2", "t7ol", "t7ol2"];
static AVAILABLE: AtomicU32 = AtomicU32::new(0);

fn bit(game: &str) -> u32 {
    if is_left_out(game) {
        return 0;
    }
    GAMES.iter().position(|g| *g == game).map_or(0, |i| 1 << i)
}

/// bo2mp: the games whose guns are left out of the game (owner, 5 Nov
/// 2026): Modern Warfare 2 (2009), Modern Warfare 3 (2011), Call of Duty 4,
/// Infinite Warfare and Modern Warfare III (2023). Their rows stay in the
/// tables, so every other gun keeps its statstable index (saved classes
/// stay right), but they are never listed, never loaded and no match walks
/// their zones (their bit is 0). Modern Warfare Remastered's guns (Horizon
/// MW, `h1`) stay. `CODLEGACY_ALL_GUEST_GUNS=1` brings them all back.
pub const LEFT_OUT: &[&str] = &["iw4", "iw5", "iw3", "iw7", "jup"];

/// Whether `game`'s guns are left out ([`LEFT_OUT`]).
#[must_use]
pub fn is_left_out(game: &str) -> bool {
    // (Under test, only on a thread that asks for it: the roster tests check
    // every game's rows.)
    #[cfg(test)]
    if !TEST_LEFT_OUT.with(std::cell::Cell::get) {
        return false;
    }
    LEFT_OUT.contains(&game) && !std::env::var("CODLEGACY_ALL_GUEST_GUNS").is_ok_and(|v| v.trim() == "1")
}

#[cfg(test)]
thread_local! {
    static TEST_LEFT_OUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

static MOD_AVAILABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether `game`'s guns are listed (`t5`, `iw4`, `iw5`, `iw6`; the mod's,
/// `t6mod`).
pub fn set_available(game: &str, on: bool) {
    if game == MOD_GAME {
        MOD_AVAILABLE.store(on, Ordering::Relaxed);
        return;
    }
    if on {
        AVAILABLE.fetch_or(bit(game), Ordering::Relaxed);
    } else {
        AVAILABLE.fetch_and(!bit(game), Ordering::Relaxed);
    }
}

/// bo2mp: the games whose guns the running match read (a bit each), and
/// whether a match is running. A match reads only the guns of the games his
/// classes held as it started, so a class edited in the game can not take a
/// gun of another game: it was never read (it drew as nothing, or as a
/// blob). While a match runs those guns are not listed in the game's class
/// editor.
static LIVE_GAMES: AtomicU32 = AtomicU32::new(0);
static LIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A match starts reading the guns of the games in `mask`.
pub fn begin_match(mask: u32) {
    LIVE_GAMES.store(mask, Ordering::Relaxed);
    LIVE.store(true, Ordering::Relaxed);
}

/// The match is over: every available game's guns are listed again.
pub fn end_match() {
    LIVE.store(false, Ordering::Relaxed);
}

/// Whether `game`'s guns can be used in the match that runs now (always,
/// when none runs or Black Ops II's own).
#[must_use]
pub fn read_by_match(game: &str) -> bool {
    let bit = bit(game);
    bit == 0 || !LIVE.load(Ordering::Relaxed) || LIVE_GAMES.load(Ordering::Relaxed) & bit != 0
}

/// Whether a guest gun is listed: its game's guns are there (and, while a
/// match runs, that match read them).
#[must_use]
pub fn gun_available(g: &GuestGun) -> bool {
    if !read_by_match(g.game) {
        return false;
    }
    // (Modern Warfare III's guns played from an exported folder, not the
    // game: the ones the folder holds.)
    if g.game == "jup"
        && let Ok(held) = JUP_EXPORT_GUNS.lock()
        && let Some(held) = held.as_ref()
    {
        return available("jup") && held.iter().any(|h| h == g.gun);
    }
    available(g.game)
}

/// bo2mp: the Modern Warfare III (2023) guns an exported folder holds
/// (`asset_transport::jup_export::guns`), when the game is played from it
/// because the install is not there (`None`: from the install). Create-a-Class
/// then lists those guns only.
static JUP_EXPORT_GUNS: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

/// Sets the guns of the export Modern Warfare III's guns are played from
/// (`None`: the install's), see [`gun_available`].
pub fn set_jup_export_guns(guns: Option<Vec<String>>) {
    if let Ok(mut held) = JUP_EXPORT_GUNS.lock() {
        *held = guns;
    }
}

#[must_use]
pub fn available(game: &str) -> bool {
    if is_left_out(game) {
        return false;
    }
    if game == MOD_GAME {
        return MOD_AVAILABLE.load(Ordering::Relaxed);
    }
    AVAILABLE.load(Ordering::Relaxed) & bit(game) != 0
}

/// bo2mp: the games whose guns a match loads, a bit each ([`game_bit`]):
/// the ones his custom classes hold, set from his saved classes and again as
/// an offline match starts. Every game until then (a match started some
/// other way). Each game's guns are a long read off the disk (Advanced
/// Warfare's 73 s on an HDD), so a match reads only the ones he can use.
static MATCH_GAMES: AtomicU32 = AtomicU32::new(u32::MAX);

/// The bit of `game` in [`match_games`] (0 for a game not listed).
#[must_use]
pub fn game_bit(game: &str) -> u32 {
    bit(game)
}

static MATCH_GAMES_SET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_match_games(mask: u32) {
    MATCH_GAMES.store(mask, Ordering::Relaxed);
    MATCH_GAMES_SET.store(true, Ordering::Relaxed);
}

/// Whether anything set [`match_games`] yet (the front end, or a match
/// started without it reading his classes).
#[must_use]
pub fn match_games_set() -> bool {
    MATCH_GAMES_SET.load(Ordering::Relaxed)
}

#[must_use]
pub fn match_games() -> u32 {
    forced_games().unwrap_or_else(|| MATCH_GAMES.load(Ordering::Relaxed))
}

/// How many other games' guns the next match reads (those installed and in
/// [`match_games`]): the more there are, the less memory each can have.
#[must_use]
pub fn match_game_count() -> u32 {
    let m = match_games();
    GAMES.iter().filter(|g| available(g) && m & bit(g) != 0).count() as u32
}

/// For measuring what each game's guns cost a match:
/// `BO2MP_GUEST_GAMES=t5,s1` (or `all`, or `none`) reads those games' guns
/// whatever his classes hold. Unset, his classes decide.
fn forced_games() -> Option<u32> {
    static FORCED: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    *FORCED.get_or_init(|| {
        let v = std::env::var("BO2MP_GUEST_GAMES").ok()?;
        let v = v.trim();
        Some(match v {
            "all" => GAMES.iter().fold(0, |mask, g| mask | bit(g)),
            "none" | "" => 0,
            _ => v.split(',').fold(0, |mask, g| mask | bit(g.trim())),
        })
    })
}

/// The games of the guest guns numbered `indices` in the statstable (a
/// class's primary and secondary), as [`match_games`] bits.
pub fn games_of_indices(indices: impl IntoIterator<Item = i32>) -> u32 {
    indices.into_iter().filter_map(game_for_index).fold(0, |mask, game| mask | bit(game))
}

/// The games of the guest guns his custom classes hold, read from his
/// stats (the custom game's classes and the public ones).
pub fn games_of_classes(stats: &std::collections::HashMap<String, crate::StatValue>) -> u32 {
    let keys = ["custommatchcacloadouts", "cacloadouts"].into_iter().flat_map(|set| {
        (0..10).flat_map(move |n| ["primary", "secondary"].map(|slot| format!("{set}.customclass.{n}.{slot}")))
    });
    games_of_indices(keys.filter_map(|k| stats.get(&k).and_then(crate::StatValue::as_num)).map(|v| v as i32))
}

/// Every guest gun, available or not, in table order.
pub fn all() -> impl Iterator<Item = &'static GuestGun> {
    BO1.iter().chain(MW2.iter()).chain(GHOSTS.iter()).chain(MW3.iter()).chain(WAW.iter()).chain(COD4.iter()).chain(AW.iter()).chain(BO3.iter()).chain(JUP.iter()).chain(HMW.iter()).chain(IW7.iter()).chain(MOD.iter()).chain(WORKSHOP_PACKS.iter().flat_map(|p| p.guns.iter()))
}

/// The statstable index of the guest gun at `position` in `all()`: a
/// roster with its own first index (MW3's) numbers from it by its place in
/// that roster; the others by their place in `all()` from `FIRST_INDEX`.
fn stats_index(position: usize, gun: &GuestGun) -> i32 {
    let own = [
        (MW3, MW3_FIRST_INDEX),
        (WAW, WAW_FIRST_INDEX),
        (COD4, COD4_FIRST_INDEX),
        (GHOSTS, GHOSTS_FIRST_INDEX),
        (AW, AW_FIRST_INDEX),
        (BO3, BO3_FIRST_INDEX),
        (JUP, JUP_FIRST_INDEX),
        (HMW, HMW_FIRST_INDEX),
        (IW7, IW7_FIRST_INDEX),
        (MOD, MOD_FIRST_INDEX),
    ];
    for (roster, first) in own.into_iter().chain(WORKSHOP_PACKS.iter().map(|p| (p.guns, p.first_index))) {
        if let Some(k) = roster.iter().position(|g| g.game == gun.game && g.gun == gun.gun) {
            return first + k as i32;
        }
    }
    FIRST_INDEX + position as i32
}

/// The guest gun numbered `index` in the statstable (the first in `all()`
/// with that number). A table built once: `stats_index` is a search of the
/// rosters, and Create-a-Class asks for every listed gun's game each time it
/// lists them (a search per gun of a search per gun made the gun picker
/// crawl).
fn gun_for_index(index: i32) -> Option<&'static GuestGun> {
    static TABLE: std::sync::OnceLock<std::collections::HashMap<i32, &'static GuestGun>> = std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| {
            let mut m = std::collections::HashMap::new();
            for (k, g) in all().enumerate() {
                m.entry(stats_index(k, g)).or_insert(g);
            }
            m
        })
        .get(&index)
        .copied()
}

impl GuestGun {
    /// The statstable reference (`bo1_ak74u`; MW3's `iw5_acr` is `mw3_acr`).
    #[must_use]
    pub fn reference(&self) -> String {
        let gun = if self.game == "iw5" { self.gun.strip_prefix("iw5_").unwrap_or(self.gun) } else { self.gun };
        format!("{}{}", prefix(self.game), gun)
    }

    fn key(&self) -> String {
        // (The mod's own keys name its guns in its text; ours are apart.)
        if self.game == MOD_GAME {
            return format!("WEAPON_T6MOD_{}", self.gun.to_ascii_uppercase());
        }
        format!("WEAPON_{}", self.reference().to_ascii_uppercase())
    }

    /// Its description's text key: ours, apart from any game's (the mod's
    /// own text signs its guns).
    fn desc_key(&self) -> String {
        format!("MPUI_BO2MP_DESC_{}", self.reference().to_ascii_uppercase())
    }

    /// What it is, as Black Ops II's Create-a-Class says it: its kind and
    /// how it plays, then the game it comes from.
    fn description(&self) -> String {
        let reference = self.reference();
        let line = GUN_LINES.iter().find(|(r, _)| *r == reference).map(|(_, l)| *l);
        let kind = line.unwrap_or(match self.group {
            AR => "Assault rifle. Effective at medium to long range.",
            SMG => "Submachine gun. High rate of fire, deadly up close.",
            LMG => "Light machine gun. Large magazine, steady at long range.",
            SNIPER => "Sniper rifle. Lethal at long range.",
            CQB => "Shotgun. Devastating at close range.",
            PISTOL => "Handgun. Quick to draw as a sidearm.",
            LAUNCHER => "Launcher. Fires explosive rounds.",
            GRENADE => "Equipment.",
            _ => "Special weapon.",
        });
        match self.game {
            MOD_GAME => kind.to_owned(),
            _ => format!("{kind} From {}.", self.game_title()),
        }
    }

    fn game_title(&self) -> &'static str {
        match self.game {
            "t5" => "Call of Duty: Black Ops",
            "iw4" => "Modern Warfare 2",
            "iw5" => "Modern Warfare 3",
            "iw6" => "Call of Duty: Ghosts",
            "t4" => "Call of Duty: World at War",
            "iw3" => "Call of Duty 4",
            "s1" => "Call of Duty: Advanced Warfare",
            "t7" => "Call of Duty: Black Ops III",
            "jup" if self.gun.starts_with("iw9_") => "Call of Duty: Modern Warfare II (2022)",
            "jup" => "Call of Duty: Modern Warfare III (2023)",
            "h1" => "Horizon MW",
            "iw7" => "Call of Duty: Infinite Warfare",
            MOD_GAME => "Black Ops II",
            g if let Some(p) = workshop_pack(g) => p.title,
            _ => "another game",
        }
    }

    /// Create-a-Class pictures to try: the game's own menu picture, then
    /// its kill icon (`t5:menu_mp_weapons_ak74u|hud_icon_ak74u`).
    fn picture(&self) -> String {
        let gun = self.gun;
        match self.game {
            // (MW2's Create-a-Class pictures are `weapon_<model>`, named for
            // the model, not the weapon: `weapon_m4carbine` for the M4A1.)
            "iw4" => match mw2_picture(gun) {
                Some(p) => format!("iw4:weapon_{p}|weapon_{gun}|hud_icon_{gun}"),
                None => format!("iw4:weapon_{gun}|hud_icon_{gun}"),
            },
            // (MW3's are its statstable's, `weapon_acr`.)
            "iw5" => format!("iw5:{}", self.icon),
            // (World at War's are its 256x128 menu pictures, named for the
            // gun (`m1garand`, `kar98`), then its kill icon; CoD4's
            // `weapon_<gun>`.)
            "t4" => format!(
                "t4:{}|menu_mp_weapons_{gun}|{}|hud_{gun}",
                waw_picture(gun).unwrap_or(gun),
                self.icon
            ),
            "iw3" => format!("iw3:weapon_{}|{}", gun, self.icon),
            // (Ghosts' are its ui_mp's `weapon_<gun>_360`, then the HUD icon.)
            "iw6" if self.icon.is_empty() => format!("iw6:weapon_{gun}_360|hud_icon_{gun}"),
            "iw6" => format!("iw6:{}|weapon_{gun}_360|hud_icon_{gun}", self.icon),
            // (AW's are common_mp's `weapon_<gun>` (streamed), then the kill icon.)
            "s1" => format!("s1:weapon_{gun}|hud_icon_{gun}"),
            // (Black Ops III's are its weapon icons in core_common: the
            // pick-up icon, then the kill icon.)
            "t7" => format!("t7:t7_icon_weapon_{gun}_pu|t7_icon_weapon_{gun}_kf"),
            // (Modern Warfare III's are global_stream_mp's
            // `icon_weapon_jup_<gun>_mp`, then the HUD icon.)
            "jup" => {
                format!("jup:{}", crate::jup_names::icon_names(gun).join("|"))
            }
            // (Horizon MW's: the original game's picture of the gun, Modern
            // Warfare 2's then Call of Duty 4's (`hmw_original`), each name
            // with its game (`h1:iw4:weapon_cheytac|...`); then its own HUD
            // icon in its weapon zones.)
            "h1" => {
                let mut names: Vec<String> = Vec::new();
                let (mw2, cod4) = hmw_original(gun);
                for (roster, original) in [(MW2, mw2), (COD4, cod4)] {
                    let Some(g) = original.and_then(|o| roster.iter().find(|g| g.gun == o)) else {
                        continue;
                    };
                    let p = g.picture();
                    if let Some((game, rest)) = p.split_once(':') {
                        names.extend(rest.split('|').filter(|n| !n.is_empty()).map(|n| format!("{game}:{n}")));
                    }
                }
                // (The Boomhilda is Origins' Mauser: the mod's full-colour
                // picture of it, before Horizon MW's white kill icon.)
                if gun == "boomhilda" {
                    names.extend([format!("{MOD_GAME}:menu_mp_weapons_bh"), format!("{MOD_GAME}:menu_mp_weapons_mc96")]);
                }
                names.push(self.icon.to_owned());
                names.push(format!("hud_icon_{gun}"));
                format!("h1:{}", names.join("|"))
            }
            // (Infinite Warfare's are its guns' HUD icons in its
            // global_core_mp.)
            "iw7" => format!("iw7:{}|hud_icon_weapon_{gun}", self.icon),
            // (The mod's are in its archive, else Black Ops II's packs.)
            MOD_GAME => format!("{MOD_GAME}:{}|{}_big", self.icon, self.icon),
            // (A Workshop pack's: its own kill icons in its zone, where it
            // has them.)
            g if let Some(p) = workshop_pack(g) => p.picture(gun),
            _ => {
                let mut p = format!("{}:menu_mp_weapons_{gun}", self.game);
                if !self.icon.is_empty() {
                    p.push('|');
                    p.push_str(self.icon);
                }
                p
            }
        }
    }
}

// ---------------------------------------------------------------------------
// bo2mp: one of each gun. The same gun from several games (the AK-47 four
// times, MW2's SCAR-H beside Black Ops II's) is listed once: Black Ops
// II's own wins, else the first game in `PRIORITY` that is installed; the
// others are left out of Create-a-Class (a class that held one gets the
// one kept, `family_of_reference`).

/// Which game's copy of a shared gun is listed (Black Ops II's always).
/// A Horizon MW gun stands in its home game's place, ahead of that game's
/// own copy (`in_priority`): Modern Warfare Remastered's versions replace
/// Modern Warfare 2's, Modern Warfare 3's and Call of Duty 4's (the
/// owner's rule, 10-20: MWR stands for those games now); a gun MWR has no
/// version of keeps its own game's.
const PRIORITY: [&str; 17] = ["iw4", "iw5", "t5", "iw6", "iw3", "t4", "s1", "t7", "jup", "h1", "iw7", "t7cw", "t7vg", "t7mw", "t7cod2", "t7ol", "t7ol2"];

/// bo2mp: the game whose guns Create-a-Class lists beside the other games'
/// of the same name rather than once among them: Infinite Warfare's are its
/// own versions (its classic UMP45, M1 Garand, Intervention, SPAS-12 and
/// G18 are its own models), on its own tab. Black Ops II's own gun of a
/// name still leaves an Infinite Warfare copy out; the mod's copy of one
/// gives way to it, as to any game's.
const BESIDE: &str = "iw7";

/// bo2mp: the games whose every gun is listed on its tab, even where
/// Black Ops II or a game ahead in `PRIORITY` has the same gun (the
/// owner's rule, 10-26: "MWR is missing some of its weapons ... and Black
/// Ops one is too"). Black Ops' AK47, M16, FAMAS, AUG, FN FAL, Uzi,
/// Skorpion, M14, M60, SPAS-12, WA2000, L96A1, RPG, Crossbow and Ballistic
/// Knife, and Horizon MW's SCAR-H, FAL, Vector and RPG-7, were left out by
/// one-of-each-gun. Their guns still stand for later games' copies
/// (Horizon MW's still replace Modern Warfare 2's).
pub const COMPLETE: [&str; 2] = ["t5", "h1"];

/// The guns (positions in `all()`) a game's turn in `PRIORITY` lists:
/// the Horizon MW guns at home there, then the game's own.
fn in_priority(game: &'static str) -> Vec<usize> {
    let hmw = all().enumerate().filter(move |(_, g)| g.game == "h1" && hmw_home(g.gun) == game && available("h1"));
    let own = all().enumerate().filter(move |(_, g)| g.game == game && g.game != "h1" && gun_available(g));
    let home_h1 = all().enumerate().filter(move |(_, g)| game == "h1" && g.game == "h1" && hmw_home(g.gun) == "h1" && available("h1"));
    hmw.chain(own).chain(home_h1).map(|(k, _)| k).collect()
}

/// bo2mp: the Create-a-Class tab a Horizon MW gun is listed on: the game
/// it is a gun of (Modern Warfare 2's, else Call of Duty 4's, which Horizon
/// MW carries too: the AK-74u, M40A3, W1200 and M1911); only the guns new
/// to Modern Warfare Remastered (the MORS, the Boomhilda) keep its own tab.
#[must_use]
pub fn hmw_home(gun: &str) -> &'static str {
    let f = family("h1", gun);
    if gun == "m9" || MW2.iter().any(|g| family("iw4", g.gun) == f) {
        "iw4"
    } else if COD4.iter().any(|g| family("iw3", g.gun) == f) {
        "iw3"
    } else if MW3.iter().any(|g| family("iw5", g.gun) == f) {
        "iw5"
    } else {
        "h1"
    }
}

/// bo2mp: the guns Horizon MW remastered, by the original game's name for
/// them: (Horizon MW's gun, Modern Warfare 2's, Call of Duty 4's; "" none).
/// Horizon MW's own guns (the MORS, the Boomhilda) and its Black Ops II
/// M1911 have none.
pub const HMW_ORIGINALS: &[(&str, &str, &str)] = &[
    ("ak47", "ak47", "ak47"),
    ("fal", "fal", ""),
    ("famas", "famas", ""),
    ("fn2000", "fn2000", ""),
    ("m16", "m16", "m16"),
    ("m4", "m4", "m4"),
    ("masada", "masada", ""),
    ("scar", "scar", ""),
    ("tavor", "tavor", ""),
    ("ak74u", "", "ak74u"),
    ("kriss", "kriss", ""),
    ("mp5k", "mp5k", ""),
    ("p90", "p90", "p90"),
    ("ump45", "ump45", ""),
    ("uzi", "uzi", "uzi"),
    ("aug", "aug", ""),
    ("m240", "m240", ""),
    ("mg4", "mg4", ""),
    ("rpd", "rpd", "rpd"),
    ("sa80", "sa80", ""),
    ("barrett", "barrett", "barrett"),
    ("cheytac", "cheytac", ""),
    ("m21", "m21", ""),
    ("m40a3", "", "m40a3"),
    ("wa2000", "wa2000", ""),
    ("aa12", "aa12", ""),
    ("m1014", "m1014", "m1014"),
    ("spas12", "spas12", ""),
    ("striker", "striker", ""),
    ("winchester1200", "", "winchester1200"),
    ("ranger", "ranger", ""),
    ("model1887", "model1887", ""),
    ("pp2000", "pp2000", ""),
    ("glock", "glock", ""),
    ("beretta393", "beretta393", ""),
    ("tmp", "tmp", ""),
    ("usp", "usp", "usp"),
    ("coltanaconda", "coltanaconda", ""),
    ("m9", "beretta", "beretta"),
    ("deserteagle", "deserteagle", "deserteagle"),
    ("at4", "at4", ""),
    ("rpg", "rpg", "rpg"),
    ("m79", "m79", ""),
];

/// A Horizon MW gun's originals: (Modern Warfare 2's gun, Call of Duty
/// 4's), `HMW_ORIGINALS`.
#[must_use]
pub fn hmw_original(gun: &str) -> (Option<&'static str>, Option<&'static str>) {
    let some = |s: &'static str| (!s.is_empty()).then_some(s);
    HMW_ORIGINALS
        .iter()
        .find(|(h, ..)| *h == gun)
        .map_or((None, None), |&(_, mw2, cod4)| (some(mw2), some(cod4)))
}

/// The tab game of the guest gun numbered `index` (`game_for_index`, with
/// a Horizon MW gun under the game it is a gun of: `hmw_home`).
#[must_use]
pub fn tab_game_for_index(index: i32) -> Option<&'static str> {
    gun_for_index(index).map(|g| {
        if g.game == "h1" {
            hmw_home(g.gun)
        } else if g.game == "jup" && g.gun.starts_with("iw9_") {
            // (Modern Warfare II (2022)'s guns the MWIII install carries:
            // their own tab, never MWIII's.)
            "iw9"
        } else {
            g.game
        }
    })
}

/// Guns known by another name in another game (name without its game's
/// `iw5_` prefix -> the family). (game, name, family); game "" = any.
const FAMILIES: &[(&str, &str, &str)] = &[
    ("", "fnfal", "fal"),
    ("", "sa58", "fal"),
    ("", "masada", "acr"),
    ("", "tavor", "tar21"),
    ("", "fads", "fad"),
    ("", "mp5k", "mp5"),
    ("", "ump45", "ump"),
    ("", "kriss", "vector"),
    ("", "sa80", "l86"),
    ("", "m60e4", "m60"),
    ("", "hk416", "m27"),
    ("", "l96a1", "l96"),
    ("", "l115a3", "l96"),
    ("", "spas12", "spas"),
    ("", "model1887", "1887"),
    ("", "glock", "g18"),
    ("", "usp45", "usp"),
    ("", "coltanaconda", "44magnum"),
    ("", "magnum", "44magnum"),
    ("iw5", "m9", "pm9"),
    ("", "beretta", "m9"),
    ("", "m9a1", "m9"),
    ("", "deserteagle", "deagle"),
    ("", "colt45", "m1911"),
    ("t4", "colt", "m1911"),
    ("", "fnfiveseven", "fiveseven"),
    ("", "beretta393", "m93"),
    ("", "beretta93r", "m93"),
    ("", "crossbow_explosive", "crossbow"),
    ("", "mk14", "m14"),
    ("", "mp44", "stg44"),
    ("", "usrpg", "rpg"),
    // (AW's `lsat` is the Pytaek, not Ghosts' LSAT.)
    ("s1", "lsat", "pytaek"),
    // bo2mp: the mod's guns, by the real gun.
    ("t6mod", "barret", "barrett"),
    ("t6mod", "k98", "kar98k"),
    ("t6mod", "ptrs", "ptrs41"),
    ("t6mod", "psg", "psg1"),
    ("t6mod", "l96a", "l96"),
    ("t6mod", "stake", "ithaca"),
    ("t6mod", "m1887", "1887"),
    ("t6mod", "ebr", "m21"),
    ("t6mod", "svg", "svg100"),
    ("t7", "sniper_powerbolt", "svg100"),
    ("t7", "sniper_fastbolt", "locus"),
    ("t6mod", "lynx", "gm6"),
    ("t6mod", "fn_scar", "scar"),
    // (The M9 Bayonet is a knife, not Beretta's M9.)
    ("t6mod", "m9", "m9bayonet"),
];

/// A gun's family: the same gun from any game has the same one.
fn family(game: &str, name: &str) -> String {
    let name = name.strip_prefix("iw5_").unwrap_or(name).to_ascii_lowercase();
    FAMILIES
        .iter()
        .find(|(g, n, _)| (g.is_empty() || *g == game) && *n == name)
        .map_or(name, |(_, _, f)| (*f).to_owned())
}

/// The family of a statstable reference: a guest gun's (`mw2_scar`) or a
/// Black Ops II gun's (`sa58` -> `fal`).
#[must_use]
pub fn family_of_reference(reference: &str) -> String {
    for game in GAMES {
        let p = prefix(game);
        if let Some(name) = reference.strip_prefix(p) {
            // (A guest reference names its gun as its game does, MW3's
            // without `iw5_`.)
            return family(game, name);
        }
    }
    if MOD.iter().any(|g| g.gun == reference) {
        return family(MOD_GAME, reference);
    }
    family("t6", reference)
}

/// The game of the guest gun numbered `index` in the statstable (`iw4`).
#[must_use]
pub fn game_for_index(index: i32) -> Option<&'static str> {
    gun_for_index(index).map(|g| g.game)
}

/// The reference of the guest gun numbered `index` in the statstable,
/// listed or not (a class saved before a copy was left out).
#[must_use]
pub fn reference_for_index(index: i32) -> Option<String> {
    gun_for_index(index).map(GuestGun::reference)
}

/// The guest guns Create-a-Class lists given Black Ops II's own guns
/// (references): positions in `all()` of the available guns, each family
/// once.
fn listed(bo2: &std::collections::HashSet<String>) -> Vec<usize> {
    let mut taken: std::collections::HashSet<String> = bo2.clone();
    let mut beside: std::collections::HashSet<String> = bo2.clone();
    let mut keep = Vec::new();
    // bo2mp: the guns ported from the games come first; the mod's only
    // where none of them is the same gun (and none of `MOD_HIDDEN`).
    for game in PRIORITY.iter().copied().chain(std::iter::once(MOD_GAME)) {
        let turn: Vec<usize> =
            if game == MOD_GAME { all().enumerate().filter(|(_, g)| g.game == MOD_GAME && available(MOD_GAME)).map(|(k, _)| k).collect() } else { in_priority(game) };
        for k in turn {
            let Some(gun) = all().nth(k) else { continue };
            if mod_hidden(gun) || codol_hidden(gun.game, gun.gun) {
                continue;
            }
            let family = family(gun.game, gun.gun);
            // (A Workshop pack's guns are all listed on its tab, as a game
            // in `COMPLETE`: last in `PRIORITY`, they take no game's copy.)
            let kept = if COMPLETE.contains(&gun.game) || is_workshop_game(gun.game) {
                taken.insert(family);
                true
            } else if game == BESIDE {
                beside.insert(family.clone()) && {
                    taken.insert(family);
                    true
                }
            } else {
                taken.insert(family)
            };
            if kept {
                keep.push(k);
            }
        }
    }
    keep.sort_unstable();
    keep
}

/// bo2mp: the available guest guns of `games` Create-a-Class leaves out
/// given Black Ops II's own guns (`bo2`, references), each with why:
/// (reference, reason). Empty for a game in `COMPLETE`.
#[must_use]
pub fn left_out(bo2: &[String], games: &[&str]) -> Vec<(String, String)> {
    let bo2: std::collections::HashSet<String> = bo2.iter().map(|r| family("t6", r)).collect();
    let keep = listed(&bo2);
    let kept: Vec<&GuestGun> = keep.iter().filter_map(|&k| all().nth(k)).collect();
    all()
        .enumerate()
        .filter(|(k, g)| games.contains(&g.game) && gun_available(g) && !keep.contains(k))
        .map(|(_, g)| {
            let f = family(g.game, g.gun);
            let why = if mod_hidden(g) {
                "hidden (MOD_HIDDEN)".to_owned()
            } else if bo2.contains(&f) {
                format!("Black Ops II's own `{f}` stands for it")
            } else if let Some(o) = kept.iter().find(|o| family(o.game, o.gun) == f) {
                format!("{}'s copy `{}` stands for it", o.game, o.reference())
            } else {
                "not listed".to_owned()
            };
            (g.reference(), why)
        })
        .collect()
}

/// The text keys and words the guest rows use (key, text).
#[must_use]
pub fn localize() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for gun in all() {
        out.push((gun.key(), gun.name.to_owned()));
        out.push((gun.desc_key(), gun.description()));
        // (Its Pack-a-Punch upgrade in Zombies.)
        out.push((zm_upgrade_key(gun), zm_upgraded_name(gun)));
    }
    for (reference, _, name, _) in EXTRA_ATTACHMENTS {
        let key = format!("MPUI_BO2MP_{}", reference.to_ascii_uppercase());
        out.push((key.clone(), (*name).to_owned()));
        out.push((format!("{key}_DESC"), (*name).to_owned()));
    }
    out.push(("MPUI_MK".to_owned(), "Masterkey".to_owned()));
    out
}

/// Whether `extend_table` adds guest rows to this table (name any case).
#[must_use]
pub fn is_guest_table(name: &str) -> bool {
    name.eq_ignore_ascii_case("mp/statsTable.csv") || name.eq_ignore_ascii_case("mp/attachmentTable.csv")
}

/// mp/statsTable.csv: a row per available guest gun (none twice).
pub fn extend_stats_rows(rows: &mut Vec<Vec<String>>) -> bool {
    let width = rows.iter().map(Vec::len).max().unwrap_or(19).max(19);
    let mut changed = false;
    let mut sort: std::collections::HashMap<&str, i32> = std::collections::HashMap::new();
    // Black Ops II's own guns (its rows below the guest indices).
    let bo2: std::collections::HashSet<String> = rows
        .iter()
        .filter(|r| r.first().and_then(|c| c.parse::<i32>().ok()).is_some_and(|i| i < FIRST_INDEX))
        .filter(|r| r.get(2).is_some_and(|g| g.starts_with("weapon_")))
        .filter_map(|r| r.get(4))
        .filter(|r| !r.is_empty())
        .map(|r| family("t6", r))
        .collect();
    let keep = listed(&bo2);
    for (k, gun) in all().enumerate() {
        if !gun_available(gun) || !keep.contains(&k) {
            continue;
        }
        let reference = gun.reference();
        if rows.iter().any(|r| r.get(4).is_some_and(|c| *c == reference)) {
            continue;
        }
        let order = sort.entry(gun.group).or_insert(20);
        *order += 1;
        let mut row = vec![String::new(); width];
        row[0] = stats_index(k, gun).to_string();
        row[2] = gun.group.to_owned();
        row[3] = gun.key();
        row[4] = reference;
        if gun.group == LAUNCHER || (gun.game == MOD_GAME && matches!(gun.group, SPECIAL | GRENADE)) {
            row[5] = "1".to_owned();
        }
        row[6] = gun.picture();
        row[7] = gun.desc_key();
        row[8] = gun.attachments.to_owned();
        row[9] = order.to_string();
        // (Unlocked from the start, a point each in Pick 10.)
        row[10] = "0".to_owned();
        row[12] = "1".to_owned();
        row[13] = gun.slot.to_owned();
        rows.push(row);
        changed = true;
    }
    changed
}

// ---------------------------------------------------------------------------
// bo2mp: the guest guns in Zombies' Mystery Box.

/// Guns that do nothing against zombies: launchers that fire only once
/// locked on to a vehicle (game, gun).
const NO_ZOMBIES: &[(&str, &str)] = &[
    ("t5", "strela"),
    ("iw4", "stinger"),
    ("iw4", "javelin"),
    ("iw5", "stinger"),
    ("s1", "stingerm7"),
    ("t7", "launcher_lockonly"),
];

/// The family of a Black Ops II Zombies weapon's script name
/// (`fnfal_zm` -> `fal`, `ak74u_upgraded_zm` -> `ak74u`).
#[must_use]
pub fn family_of_zm_weapon(name: &str) -> String {
    let name = name.split('+').next().unwrap_or(name).to_ascii_lowercase();
    let name = name.strip_suffix("_mp").unwrap_or(&name);
    let name = name.strip_suffix("_zm").unwrap_or(name);
    let name = name.strip_suffix("_upgraded").unwrap_or(name);
    family("t6", name)
}

/// The script name a guest gun has in a match (`bo1_ak74u_mp`).
#[must_use]
pub fn script_name(gun: &GuestGun) -> String {
    format!("{}_mp", gun.reference())
}

/// The voice-over type Black Ops II Zombies' `add_zombie_weapon` gives a
/// gun of this group (what the player says when he picks it up).
#[must_use]
pub fn zm_vox(gun: &GuestGun) -> &'static str {
    match gun.group {
        SMG => "smg",
        AR => "assault",
        LMG => "mg",
        SNIPER => "sniper",
        CQB => "shotgun",
        PISTOL => "pistol",
        LAUNCHER => "launcher",
        _ => "",
    }
}

/// The guest guns the Mystery Box can give, given the families of the
/// map's own Zombies weapons (`bo2`): each family once and none the map
/// already has (its AK74u, not Black Ops' as well), the copy of the first
/// game in Create-a-Class's order whose gun `loaded` says is in the match;
/// no special weapons (ballistic knives, crossbows: Zombies' own scripts
/// handle those by name) and no lock-on-only launchers.
pub fn zm_box_guns(
    bo2: &std::collections::HashSet<String>,
    loaded: impl Fn(&GuestGun) -> bool,
) -> Vec<&'static GuestGun> {
    let mut taken = bo2.clone();
    let mut keep = Vec::new();
    // (The games' own copies first; the mod's only where none is the gun.)
    for game in PRIORITY.iter().copied().chain(std::iter::once(MOD_GAME)) {
        for gun in all().filter(|g| g.game == game && !mod_hidden(g)) {
            // (Nor the Workshop packs' guns: hundreds of them, for
            // Create-a-Class.)
            if gun.group == SPECIAL || gun.group == GRENADE || NO_ZOMBIES.contains(&(gun.game, gun.gun)) || is_workshop_game(gun.game) || !loaded(gun) {
                continue;
            }
            if taken.insert(family(gun.game, gun.gun)) {
                keep.push(gun);
            }
        }
    }
    keep
}

// ---------------------------------------------------------------------------
// bo2mp: Pack-a-Punch for the guest guns in Zombies.
//
// Black Ops II's `_zm_weapons` upgrades a gun into its `upgrade_name`
// weapon. A guest gun's upgrade is, in order:
//
// 1. its own game's upgraded weapon, when that game has a Pack-a-Punch
//    (World at War, Black Ops, Advanced Warfare's Exo Zombies, Infinite
//    Warfare's and Black Ops III's zombies) and the asset is among the
//    weapons loaded (`zm_upgrade_assets`: its names in that game's zones);
// 2. otherwise a Black Ops II-style upgrade made here: a copy of the gun
//    (`<gun>_upgraded_zm`, `bo1_ak74u_upgraded_zm_mp`) with the map's own
//    Pack-a-Punch proportions (damage, magazine, reserve), Black Ops II's
//    Pack-a-Punch camo and an upgraded name. Ghosts, Modern Warfare 2 and 3
//    and Call of Duty 4 have no Pack-a-Punch, so theirs are always made.

/// The camo option (as `bo2mp_camos` carries it) that draws Black Ops II's
/// Pack-a-Punch camo on another game's gun; past every camo number a class
/// carries (0-1023, `asset_core::weapon_camos::MAX_OPTION`), so no other
/// game's camo is taken for it.
pub const ZM_PAP_CAMO_OPTION: u32 = 1039;

/// Whether a game has a Pack-a-Punch of its own (its guns' upgraded
/// weapons are in its zombies zones). Black Ops II's (`t6`) are the map's.
#[must_use]
pub fn has_own_pack_a_punch(game: &str) -> bool {
    matches!(game, "t4" | "t5" | "t6" | "s1" | "iw7" | "t7" | MOD_GAME)
}

/// The names (as the catalog names its game's rows: lower case, no `_mp`,
/// AW's and Ghosts' without `iw5_`/`iw6_`) that `game`'s own upgraded
/// weapon of `gun` has in its zombies zones, most likely first. Empty when
/// the game has no Pack-a-Punch.
///
/// - Black Ops: `<gun>_upgraded_zm` (`ak74u_upgraded_zm`), the guns that
///   came with an attachment in its box named with it (`m16_gl_upgraded_zm`).
/// - World at War: the zombies copy's name with `_upgraded`
///   (`zombie_mp40_upgraded`, `ppsh_upgraded`, `ptrs41_zombie_upgraded`).
/// - Advanced Warfare (Exo Zombies): the zombies copy of the gun, upgraded
///   (`iw5_bal27zm_mp` and its upgrade, read as `bal27zm...`).
/// - Infinite Warfare (once its guns are guest guns): its zombies gun with
///   the Pack-a-Punch attachment (`iw7_ar57_zm+pap1`).
/// - Black Ops III (once its guns are guest guns): `<gun>_upgraded`
///   (`ar_standard_upgraded`).
#[must_use]
pub fn zm_upgrade_assets(game: &str, gun: &str) -> Vec<String> {
    let gun = gun.to_ascii_lowercase();
    match game {
        "t5" => {
            let own: &[&str] = match gun.as_str() {
                "m16" => &["m16_gl_upgraded_zm"],
                "aug" => &["aug_acog_mk_upgraded_zm", "aug_acog_upgraded_zm"],
                "g11" => &["g11_lps_upgraded_zm"],
                "cz75" => &["cz75_upgraded_zm", "cz75dw_upgraded_zm"],
                "crossbow_explosive" => &["crossbow_explosive_upgraded_zm"],
                _ => &[],
            };
            let mut out: Vec<String> = own.iter().map(|s| (*s).to_owned()).collect();
            out.push(format!("{gun}_upgraded_zm"));
            out.dedup();
            out
        }
        "t4" => {
            // (The zombies copies' names: `zombie_<gun>`, or the gun's own,
            // or `<gun>_zombie`; a few are named apart.)
            let zm = match gun.as_str() {
                "type100smg" => "type100_smg",
                "type99rifle" => "type99_rifle",
                "type99lmg" => "type99_lmg",
                "doublebarreledshotgun" => "doublebarrel",
                "357magnum" => "sw_357",
                other => other,
            };
            let mut out = vec![
                format!("zombie_{zm}_upgraded"),
                format!("{zm}_upgraded"),
                format!("{zm}_zombie_upgraded"),
            ];
            if gun == "kar98k" {
                out.push("kar98k_scoped_zombie_upgraded".to_owned());
            }
            if gun == "doublebarreledshotgun" {
                out.push("zombie_doublebarrel_sawed_upgraded".to_owned());
            }
            out
        }
        "s1" => vec![
            format!("{gun}zm_upgraded"),
            format!("{gun}zmup"),
            format!("{gun}_zm_upgraded"),
            format!("{gun}_upgraded_zm"),
        ],
        "iw7" => {
            let gun = gun.strip_prefix("iw7_").unwrap_or(&gun);
            vec![format!("{gun}_zm+pap1"), format!("iw7_{gun}_zm+pap1"), format!("{gun}_zm_pap1")]
        }
        "t7" => vec![format!("{gun}_upgraded"), format!("{gun}_upgraded_zm")],
        // (The mod's one upgraded gun: Origins' Mauser is Boomhilda; the
        // others are made Black Ops II-style.)
        MOD_GAME if gun == "mauser" => vec!["boomhilda_mp".to_owned(), "boomhilda".to_owned()],
        _ => Vec::new(),
    }
}

/// The name of the Black Ops II-style upgrade made of a gun's row
/// (`ak74u` -> `ak74u_upgraded_zm`; its guest name `bo1_ak74u_upgraded_zm_mp`).
#[must_use]
pub fn zm_upgrade_copy_name(row_name: &str) -> String {
    let n = row_name.to_ascii_lowercase();
    let n = n.strip_suffix("_mp").unwrap_or(&n);
    format!("{n}_upgraded_zm")
}

/// The text key of a guest gun's upgraded name (`WEAPON_BO1_AK74U_UPGRADED`).
#[must_use]
pub fn zm_upgrade_key(gun: &GuestGun) -> String {
    format!("{}_UPGRADED", gun.key())
}

/// Their own games' names for the upgraded guns (World at War's and Black
/// Ops' Pack-a-Punch), and Black Ops II-style ones for Ghosts' guns, which
/// never had one. (game, gun, name).
const UPGRADED_NAMES: &[(&str, &str, &str)] = &[
    // The mod (Origins' upgrade).
    (MOD_GAME, "mauser", "Boomhilda"),
    // Black Ops.
    ("t5", "ak74u", "AK74fu2"),
    ("t5", "commando", "Predator"),
    ("t5", "famas", "G16-GL35"),
    ("t5", "galil", "Lamentation"),
    ("t5", "fnfal", "WN"),
    ("t5", "m14", "Mnesia"),
    ("t5", "m16", "Skullcrusher"),
    ("t5", "aug", "AUG-50M3"),
    ("t5", "g11", "G115 Generator"),
    ("t5", "mp5k", "MP115 Kollider"),
    ("t5", "mpl", "MPL-LF"),
    ("t5", "pm63", "Tokyo & Rose"),
    ("t5", "spectre", "Phantom"),
    ("t5", "uzi", "Uncle Gal"),
    ("t5", "hs10", "Typhoid & Mary"),
    ("t5", "ithaca", "Raid"),
    ("t5", "spas", "SPAZ-24"),
    ("t5", "rottweil72", "Hades"),
    ("t5", "hk21", "H115 Oscillator"),
    ("t5", "rpk", "R115 Resonator"),
    ("t5", "dragunov", "D115 Disassembler"),
    ("t5", "l96a1", "L115 Isolator"),
    ("t5", "m1911", "Mustang & Sally"),
    ("t5", "python", "Cobra"),
    ("t5", "cz75", "Calamity"),
    ("t5", "m72_law", "M72 Anarchy"),
    ("t5", "china_lake", "China Beach"),
    // World at War.
    ("t4", "thompson", "Gibs-O-Matic"),
    ("t4", "mp40", "The Afterburner"),
    ("t4", "type100smg", "1001 Samurais"),
    ("t4", "ppsh", "The Reaper"),
    ("t4", "stg44", "Spatz-447 +"),
    ("t4", "m1carbine", "Widdershins RC-1"),
    ("t4", "m1garand", "M1000"),
    ("t4", "gewehr43", "G115 Compressor"),
    ("t4", "bar", "The Widow Maker"),
    ("t4", "fg42", "420 Impeller"),
    ("t4", "mg42", "Barracuda FU-A11"),
    ("t4", "30cal", "B115 Accelerator"),
    ("t4", "kar98k", "Armageddon"),
    ("t4", "ptrs41", "The Penetrator"),
    ("t4", "shotgun", "Gut Shot"),
    ("t4", "doublebarreledshotgun", "24 Bore Long Range"),
    // Ghosts (no Pack-a-Punch of its own: Black Ops II-style names).
    ("iw6", "ak12", "Kalashni-Kill 12"),
    ("iw6", "sc2010", "SC-2010 Sundowner"),
    ("iw6", "honeybadger", "Venom Badger"),
    ("iw6", "bren", "Bren-Gunner 115"),
    ("iw6", "msbs", "MSBS Mauler"),
    ("iw6", "arx160", "ARX-1600 Arkhangel"),
    ("iw6", "fads", "Fade to Black"),
    ("iw6", "r5rgp", "R5 Revenant"),
    ("iw6", "kriss", "Vector Vindicator"),
    ("iw6", "pp19", "Biz-Zone"),
    ("iw6", "vepr", "Vepr Viper"),
    ("iw6", "microtar", "MTAR-X Microwave"),
    ("iw6", "cbjms", "Cerebral Jackhammer"),
    ("iw6", "k7", "K7 Killcount"),
    ("iw6", "m27", "M27 Infinite Assault"),
    ("iw6", "ameli", "Ameli-orator"),
    ("iw6", "lsat", "LSAT Last Stand"),
    ("iw6", "kac", "Chain Reaction"),
    ("iw6", "vks", "VKS Vanquisher"),
    ("iw6", "gm6", "Lynxinator"),
    ("iw6", "usr", "USR Usurper"),
    ("iw6", "l115a3", "L115 Lacerator"),
    ("iw6", "imbel", "IA-115 Immolator"),
    ("iw6", "svu", "SVU Sanguine"),
    ("iw6", "g28", "MR-115 Mortician"),
    ("iw6", "mk14", "MK14 Mnemonic"),
    ("iw6", "mts255", "MTS-255 Meat Grinder"),
    ("iw6", "fp6", "FP6 Flesh Peeler"),
    ("iw6", "maul", "Maul-115"),
    ("iw6", "uts15", "Hound of Hell"),
    ("iw6", "p226", "P226 Punisher"),
    ("iw6", "m9a1", "M9A1 Mangler"),
    ("iw6", "mp443", "Grach Graverobber"),
    ("iw6", "magnum", ".44 Mega Magnum"),
    ("iw6", "pdw", "PDW Persuader"),
    ("iw6", "panzerfaust3", "Panzer Fiend"),
    ("iw6", "mk32", "MK32 Mayhem Maker"),
    ("iw6", "maaws", "Jaws of MAAWS"),
    ("iw6", "rgm", "Kastet Kaboom"),
];

/// A guest gun's upgraded name: its own game's where it had one (and for
/// Ghosts' guns, the Black Ops II-style ones above), else Black Ops II's
/// manner of naming its 115-charged guns (`M4A1-115`).
#[must_use]
pub fn zm_upgraded_name(gun: &GuestGun) -> String {
    UPGRADED_NAMES
        .iter()
        .find(|(game, g, _)| *game == gun.game && *g == gun.gun)
        .map_or_else(|| format!("{}-115", gun.name), |(_, _, n)| (*n).to_owned())
}

/// A guest gun's upgrade in this Zombies game: the gun's script name
/// (`bo1_ak74u_mp`), its upgrade's (`bo1_ak74u_upgraded_zm_mp`), whether
/// the upgrade was made here (Black Ops II-style) rather than its own
/// game's, and whether it wears Black Ops II's Pack-a-Punch camo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZmUpgrade {
    pub base: String,
    pub upgraded: String,
    pub made: bool,
    pub bo2_camo: bool,
}

/// Whether an upgraded weapon from its own game wears Black Ops II's
/// Pack-a-Punch camo: Black Ops' upgrades wore its own camo option
/// (its models are the plain gun's; Black Ops II's camo stands in for it);
/// World at War's wore none; the later games' upgrades carry their look in
/// their own models, but Black Ops III's (its Pack-a-Punch camo is a camo
/// asset over the plain gun's models, not read: Black Ops II's stands in).
#[must_use]
pub fn own_upgrade_wears_bo2_camo(game: &str) -> bool {
    matches!(game, "t5" | "t7")
}

static ZM_UPGRADES: std::sync::RwLock<Vec<ZmUpgrade>> = std::sync::RwLock::new(Vec::new());

/// The match load's guest upgrades (empty off a Zombies map).
pub fn set_zm_upgrades(list: Vec<ZmUpgrade>) {
    if let Ok(mut u) = ZM_UPGRADES.write() {
        *u = list;
    }
}

/// Every guest upgrade of this match.
#[must_use]
pub fn zm_upgrades() -> Vec<ZmUpgrade> {
    ZM_UPGRADES.read().map(|u| u.clone()).unwrap_or_default()
}

/// The upgrade of a guest gun (by its script name, `bo1_ak74u_mp`).
#[must_use]
pub fn zm_upgrade_of(base: &str) -> Option<ZmUpgrade> {
    let base = base.to_ascii_lowercase();
    ZM_UPGRADES.read().ok()?.iter().find(|u| u.base == base).cloned()
}

/// The guest upgrade a weapon is (by its script name), if it is one.
#[must_use]
pub fn zm_upgraded(name: &str) -> Option<ZmUpgrade> {
    let name = name.to_ascii_lowercase();
    ZM_UPGRADES.read().ok()?.iter().find(|u| u.upgraded == name).cloned()
}

/// mp/attachmentTable.csv: the guest attachments BO2's table lacks.
pub fn extend_attachment_rows(rows: &mut Vec<Vec<String>>) -> bool {
    if !GAMES.iter().any(|g| available(g)) {
        return false;
    }
    let width = rows.iter().map(Vec::len).max().unwrap_or(20).max(20);
    let all_refs: Vec<&str> = EXTRA_ATTACHMENTS.iter().map(|a| a.0).collect();
    let mut changed = false;
    for (k, (reference, slot, _, image)) in EXTRA_ATTACHMENTS.iter().enumerate() {
        if rows.iter().any(|r| r.get(2).is_some_and(|c| c == "attachment") && r.get(4).is_some_and(|c| c == reference)) {
            continue;
        }
        let key = format!("MPUI_BO2MP_{}", reference.to_ascii_uppercase());
        let mut row = vec![String::new(); width];
        row[0] = (100 + k).to_string();
        row[1] = (*slot).to_owned();
        row[2] = "attachment".to_owned();
        row[3] = key.clone();
        row[4] = (*reference).to_owned();
        row[6] = (*image).to_owned();
        row[7] = format!("{key}_DESC");
        row[8] = reference.to_ascii_uppercase();
        row[9] = (40 + k).to_string();
        row[11] = format!(
            "acog dualclip extclip gl grip ir reflex rf silencer vzoom mk {}",
            all_refs.iter().filter(|r| *r != reference).copied().collect::<Vec<_>>().join(" ")
        );
        row[12] = "1".to_owned();
        row[18] = "0".to_owned();
        rows.push(row);
        changed = true;
    }
    changed
}

/// Tests that switch games on and off (the switches are process-wide).
#[cfg(test)]
pub(crate) static TEST_GAMES: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    #[test]
    fn left_out_games_are_never_listed_and_keep_the_others_indices() {
        super::TEST_LEFT_OUT.with(|c| c.set(true));
        super::set_available("iw4", true);
        super::set_available("t5", true);
        assert!(!super::available("iw4") && !super::available("jup") && !super::available("iw3"));
        assert!(super::available("t5"));
        assert_eq!(super::game_bit("iw5"), 0);
        assert_ne!(super::game_bit("h1"), 0);
    }

    use super::*;

    #[test]
    fn a_running_match_lists_only_the_games_it_read() {
        end_match();
        assert!(read_by_match("t5") && read_by_match("t7"));
        begin_match(game_bit("t7"));
        assert!(read_by_match("t7"), "a game the match read stays");
        assert!(!read_by_match("t5"), "a game it did not read goes");
        end_match();
        assert!(read_by_match("t5"), "all are listed again after it");
    }

    #[test]
    fn horizon_mw_guns_show_their_original_games_pictures() {
        // Every listed original is a gun of that game's roster, and the
        // table covers each Horizon MW gun once.
        for &(h, mw2, cod4) in HMW_ORIGINALS {
            assert!(HMW.iter().any(|g| g.gun == h), "{h} is not a Horizon MW gun");
            assert!(mw2.is_empty() || MW2.iter().any(|g| g.gun == mw2), "MW2 has no {mw2}");
            assert!(cod4.is_empty() || COD4.iter().any(|g| g.gun == cod4), "CoD4 has no {cod4}");
            assert_eq!(HMW_ORIGINALS.iter().filter(|o| o.0 == h).count(), 1, "{h} twice");
        }
        let picture = |gun: &str| HMW.iter().find(|g| g.gun == gun).unwrap().picture();
        assert_eq!(hmw_original("cheytac"), (Some("cheytac"), None));
        assert_eq!(picture("cheytac"), "h1:iw4:weapon_cheytac|iw4:hud_icon_cheytac|hud_icon_cheytac|hud_icon_cheytac");
        // (MW2 names its pictures for the models.)
        assert!(picture("m9").starts_with("h1:iw4:weapon_m9beretta|"));
        // A gun MW2 lacks: Call of Duty 4's.
        assert_eq!(hmw_original("m40a3"), (None, Some("m40a3")));
        assert_eq!(picture("m40a3"), "h1:iw3:weapon_m40a3|iw3:hud_icon_m40a3|hud_icon_m40a3|hud_icon_m40a3");
        assert_eq!(hmw_original("ak74u"), (None, Some("ak74u")));
        assert_eq!(hmw_original("winchester1200"), (None, Some("winchester1200")));
        // MW2's first, then CoD4's, then Horizon MW's own.
        let ak = picture("ak47");
        let (a, b, c) = (ak.find("iw4:").unwrap(), ak.find("iw3:").unwrap(), ak.find("hud_icon_ak472").unwrap());
        assert!(a < b && b < c, "{ak}");
        // Horizon MW's own guns and its Black Ops II M1911 keep its icon.
        for own in ["mors", "boomhilda", "colt45"] {
            assert_eq!(hmw_original(own), (None, None));
            assert!(!picture(own).contains("iw4:") && !picture(own).contains("iw3:"), "{own}");
        }
        // The names come out as each game's own (`foreign_picture`).
        let (game, names) = crate::progression::foreign_picture(&picture("cheytac")).unwrap();
        assert_eq!(game, "h1");
        assert_eq!(names[0], "iw4:weapon_cheytac");
    }

    #[test]
    fn rows_only_for_available_games_and_once() {
        let _games = TEST_GAMES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for game in GAMES {
            set_available(game, false);
        }
        let mut rows = vec![vec!["0".to_owned(); 19]];
        assert!(!extend_stats_rows(&mut rows), "{:?}", rows.iter().skip(1).map(|r| r[4].clone()).collect::<Vec<_>>());
        set_available("t5", true);
        set_available("iw4", true);
        // (And the rosters numbered from their own first index.)
        for game in ["iw5", "t4", "iw3", "iw6"] {
            set_available(game, true);
        }
        assert!(extend_stats_rows(&mut rows));
        let refs: Vec<&String> = rows.iter().map(|r| &r[4]).collect();
        assert!(refs.iter().any(|r| *r == "mw2_m4"));
        assert!(refs.iter().any(|r| *r == "mw3_cm901"));
        assert_eq!(rows.iter().find(|r| r[4] == "mw3_cm901").map(|r| r[0].as_str()), Some("503"));
        assert_eq!(rows.iter().find(|r| r[4] == "waw_thompson").map(|r| r[0].as_str()), Some("605"));
        assert_eq!(rows.iter().find(|r| r[4] == "cod4_g3").map(|r| r[0].as_str()), Some("703"));
        assert_eq!(rows.iter().find(|r| r[4] == "gh_ak12").map(|r| r[0].as_str()), Some("800"));
        assert!(rows.iter().find(|r| r[4] == "gh_k7").is_some_and(|r| r[6].starts_with("iw6:weapon_k7_ui_360|hud_icon_k7_ui")));
        let mut indices: Vec<&String> = rows.iter().map(|r| &r[0]).collect();
        indices.sort();
        indices.dedup();
        assert_eq!(indices.len(), rows.len(), "indices unique");
        let ak = rows.iter().find(|r| r[4] == "bo1_ak74u").expect("ak74u row");
        assert_eq!(ak[2], "weapon_smg");
        assert_eq!(ak[13], "primary");
        assert!(ak[6].starts_with("t5:menu_mp_weapons_ak74u|"));
        assert!(ak[0].parse::<i32>().unwrap() >= FIRST_INDEX);
        let n = rows.len();
        assert!(!extend_stats_rows(&mut rows));
        assert_eq!(rows.len(), n);
        let mut atts = vec![vec![String::new(); 20]];
        assert!(extend_attachment_rows(&mut atts));
        assert!(atts.iter().any(|r| r[4] == "elbit" && r[2] == "attachment"));
        // Every guest gun's attachment is BO2's or one added here.
        let bo2 = [
            "acog", "reflex", "ir", "extclip", "dualclip", "gl", "mk", "grip", "rf", "silencer", "vzoom", "fmj", "steadyaim", "fastads", "stalker", "sf", "holo",
            "dualoptic", "tacknife", "extbarrel", "swayreduc", "dw", "is", "stackfire", "mms", "rangefinder",
        ];
        let mut missing = std::collections::BTreeSet::new();
        for gun in all().filter(|g| g.game != MOD_GAME) {
            for a in gun.attachments.split_whitespace() {
                if !(bo2.contains(&a) || atts.iter().any(|r| r[4] == a)) {
                    missing.insert(format!("{a} (e.g. {})", gun.gun));
                }
            }
        }
        assert!(missing.is_empty(), "attachments no table row has: {missing:?}");
    }

    /// Pack-a-Punch: the games with their own look for their upgraded
    /// weapons by their zombies names; the others are made here, named as
    /// Black Ops II names its upgraded guns, and every gun has a name.
    #[test]
    fn the_guest_upgrades_map_per_game() {
        assert_eq!(zm_upgrade_assets("t5", "ak74u"), vec!["ak74u_upgraded_zm"]);
        assert_eq!(zm_upgrade_assets("t5", "m16")[0], "m16_gl_upgraded_zm");
        assert!(zm_upgrade_assets("t4", "mp40").contains(&"zombie_mp40_upgraded".to_owned()));
        assert!(zm_upgrade_assets("t4", "ptrs41").contains(&"ptrs41_zombie_upgraded".to_owned()));
        assert!(zm_upgrade_assets("t4", "type100smg").contains(&"zombie_type100_smg_upgraded".to_owned()));
        assert!(!zm_upgrade_assets("s1", "bal27").is_empty());
        assert_eq!(zm_upgrade_assets("t7", "ar_standard")[0], "ar_standard_upgraded");
        assert_eq!(zm_upgrade_assets("iw7", "iw7_ar57")[0], "ar57_zm+pap1");
        for game in ["iw3", "iw4", "iw5", "iw6"] {
            assert!(!has_own_pack_a_punch(game) && zm_upgrade_assets(game, "ak47").is_empty(), "{game}");
        }
        assert_eq!(zm_upgrade_copy_name("ak74u"), "ak74u_upgraded_zm");
        assert_eq!(zm_upgrade_copy_name("iw5_acr_mp"), "iw5_acr_upgraded_zm");
        let gun = |game: &str, name: &str| all().find(|g| g.game == game && g.gun == name).unwrap();
        assert_eq!(zm_upgraded_name(gun("t5", "ak74u")), "AK74fu2");
        assert_eq!(zm_upgraded_name(gun("t4", "mp40")), "The Afterburner");
        assert_eq!(zm_upgraded_name(gun("iw4", "m4")), "M4A1-115");
        assert_eq!(zm_upgrade_key(gun("iw6", "ak12")), "WEAPON_GH_AK12_UPGRADED");
        // Every Ghosts gun has its own Black Ops II-style name.
        for g in GHOSTS {
            assert!(UPGRADED_NAMES.iter().any(|(game, n, _)| *game == "iw6" && *n == g.gun), "{}", g.gun);
        }
        // Each upgraded name is a text key.
        let keys = localize();
        assert!(keys.iter().any(|(k, t)| k == "WEAPON_BO1_AK74U_UPGRADED" && t == "AK74fu2"));
        assert!(own_upgrade_wears_bo2_camo("t5") && !own_upgrade_wears_bo2_camo("t4"));
        set_zm_upgrades(vec![ZmUpgrade {
            base: "gh_ak12_mp".into(),
            upgraded: "gh_ak12_upgraded_zm_mp".into(),
            made: true,
            bo2_camo: true,
        }]);
        assert!(zm_upgrade_of("GH_AK12_MP").is_some_and(|u| u.made));
        assert!(zm_upgraded("gh_ak12_upgraded_zm_mp").is_some_and(|u| u.bo2_camo));
        set_zm_upgrades(Vec::new());
        assert!(zm_upgrades().is_empty());
    }

    /// The Mystery Box: each family once, none the map's Zombies guns
    /// have, no special weapons or lock-on launchers, only loaded guns.
    #[test]
    fn the_zombies_box_takes_each_family_once() {
        let zm: std::collections::HashSet<String> =
            ["ak74u_zm", "fnfal_zm", "mp5k_upgraded_zm", "m1911_zm"].iter().map(|n| family_of_zm_weapon(n)).collect();
        assert!(zm.contains("fal") && zm.contains("mp5") && zm.contains("ak74u"));
        let guns = zm_box_guns(&zm, |g| g.game != "s1");
        let names: Vec<String> = guns.iter().map(|g| script_name(g)).collect();
        assert!(!names.iter().any(|n| n == "bo1_ak74u_mp" || n == "bo1_fnfal_mp" || n == "bo1_mp5k_mp"));
        assert!(!names.iter().any(|n| n.contains("stinger") || n.contains("javelin") || n.contains("knife_ballistic")));
        assert!(!names.iter().any(|n| n.starts_with("aw_")));
        assert!(names.iter().any(|n| n == "mw2_m4_mp"));
        assert!(names.iter().any(|n| n == "mw2_masada_mp") && !names.iter().any(|n| n == "mw3_acr_mp"));
        assert!(names.iter().any(|n| n == "mw3_cm901_mp"));
        let mut families: Vec<String> = guns.iter().map(|g| family(g.game, g.gun)).collect();
        let n = families.len();
        families.sort();
        families.dedup();
        assert_eq!(families.len(), n, "each family once");
        assert_eq!(zm_vox(guns.iter().find(|g| g.gun == "m4").unwrap()), "assault");
    }

    /// bo2mp: the mod's guns are listed whole while the mod is loaded, each
    /// leaving out the other games' copies of the same real gun (Black Ops
    /// II's own stay); its Zombies weapons are known as Custom Games only.
    #[test]
    fn each_gun_listed_once() {
        let _games = TEST_GAMES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for game in GAMES {
            set_available(game, true);
        }
        let bo2 = |r: &str| {
            let mut row = vec![String::new(); 19];
            row[0] = "10".to_owned();
            row[2] = "weapon_assault".to_owned();
            row[4] = r.to_owned();
            row
        };
        let mut rows = vec![bo2("scar"), bo2("sa58"), bo2("usrpg"), bo2("hk416")];
        extend_stats_rows(&mut rows);
        let refs: Vec<&str> = rows.iter().map(|r| r[4].as_str()).collect();
        for gone in ["mw2_scar", "mw3_scar", "mw2_fal", "mw2_rpg", "gh_m27", "mw3_m4", "cod4_m4", "cod4_ak47", "mw3_ak47"] {
            assert!(!refs.contains(&gone), "{gone} listed");
        }
        // (Horizon MW's versions stand for Modern Warfare 2's and Call of
        // Duty 4's: `horizon_mw_guns`.)
        for kept in ["h2_m4", "h2_ak47", "h2_deserteagle", "bo1_ak74u", "mw3_cm901", "gh_ak12"] {
            assert!(refs.contains(&kept), "{kept} missing");
        }
        for gone in ["mw2_m4", "mw2_ak47", "mw2_deserteagle"] {
            assert!(!refs.contains(&gone), "{gone} listed beside Horizon MW's");
        }
        // (Infinite Warfare's are listed beside the others:
        // `infinite_warfare_guns`.)
        let mut families: Vec<String> =
            rows.iter().filter(|r| !r[4].starts_with("iw7_") && !r[4].starts_with("bo1_") && !r[4].starts_with("h2_")).map(|r| family_of_reference(&r[4])).collect();
        let n = families.len();
        families.sort();
        families.dedup();
        assert_eq!(families.len(), n, "a family twice");
        assert_eq!(reference_for_index(500).as_deref(), Some("mw3_m4"));
        assert_eq!(family_of_reference("mw3_m4"), family_of_reference("mw2_m4"));
        assert_eq!(family_of_reference("sa58"), "fal");
    }

    /// bo2mp: Black Ops' and Horizon MW's guns are listed whole
    /// (`COMPLETE`), Black Ops II's own guns of a name notwithstanding.
    #[test]
    fn black_ops_and_horizon_mw_listed_whole() {
        let _games = TEST_GAMES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for game in GAMES {
            set_available(game, true);
        }
        let bo2 = |r: &str| {
            let mut row = vec![String::new(); 19];
            row[0] = "10".to_owned();
            row[2] = "weapon_assault".to_owned();
            row[4] = r.to_owned();
            row
        };
        let own = ["scar", "sa58", "usrpg", "crossbow", "knife_ballistic", "vector"];
        let mut rows: Vec<Vec<String>> = own.iter().map(|r| bo2(r)).collect();
        extend_stats_rows(&mut rows);
        let refs: Vec<&str> = rows.iter().map(|r| r[4].as_str()).collect();
        for g in BO1.iter().chain(HMW.iter()) {
            assert!(refs.contains(&g.reference().as_str()), "{} missing", g.reference());
        }
        let own: Vec<String> = own.iter().map(|s| (*s).to_owned()).collect();
        assert!(left_out(&own, &["t5", "h1"]).is_empty());
        // (Other games' copies still give way.)
        assert!(left_out(&own, &["iw4"]).iter().any(|(r, why)| r == "mw2_scar" && why.contains("Black Ops II")));
    }

    /// bo2mp: Horizon MW's guns are Modern Warfare Remastered's versions
    /// of Modern Warfare 2's, 3's and Call of Duty 4's: with it installed
    /// they replace those games' copies of a gun (`mw2_m4` gives way to
    /// `h2_m4`), each on its home game's tab (`hmw_home`); those games'
    /// guns MWR has no version of stay; Black Ops II's own gun of a name
    /// still leaves them out, and the mod's copy of one gives way to it.
    #[test]
    fn horizon_mw_guns() {
        let _games = TEST_GAMES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let bo2 = |r: &str| {
            let mut row = vec![String::new(); 19];
            row[0] = "10".to_owned();
            row[2] = "weapon_assault".to_owned();
            row[4] = r.to_owned();
            row
        };
        let listed = |games: &[&str]| {
            for game in GAMES.iter().chain(std::iter::once(&MOD_GAME)) {
                set_available(game, games.contains(game));
            }
            let mut rows = vec![bo2("scar")];
            extend_stats_rows(&mut rows);
            rows.into_iter().map(|r| (r[4].clone(), r[0].clone(), r[6].clone(), r[13].clone())).collect::<Vec<_>>()
        };
        let without = listed(&["iw4", "iw5", "iw3", MOD_GAME]);
        let with = listed(&["iw4", "iw5", "iw3", "h1", MOD_GAME]);
        let refs = |l: &[(String, String, String, String)]| l.iter().map(|r| r.0.clone()).collect::<Vec<_>>();
        let (a, b) = (refs(&without), refs(&with));
        for replaced in ["mw2_m4", "mw2_ak47", "mw2_cheytac", "mw2_deserteagle", "cod4_m40a3", "cod4_winchester1200"] {
            assert!(a.contains(&replaced.to_owned()), "{replaced} missing without Horizon MW");
            assert!(!b.contains(&replaced.to_owned()), "{replaced} listed beside Horizon MW's");
        }
        // (Their guns MWR has no version of stay.)
        for stays in ["mw2_stinger", "mw2_javelin", "cod4_g3", "cod4_remington700", "mw3_cm901", "mw3_ksg"] {
            assert!(b.contains(&stays.to_owned()), "{stays} missing");
        }
        for h2 in ["h2_m4", "h2_ak47", "h2_cheytac", "h2_deserteagle", "h2_ak74u", "h2_m40a3", "h2_boomhilda", "h2_at4"] {
            assert!(b.contains(&h2.to_owned()), "{h2} missing");
            assert!(!a.contains(&h2.to_owned()), "{h2} listed without Horizon MW");
        }
        // Home tabs.
        assert_eq!(hmw_home("m4"), "iw4");
        assert_eq!(hmw_home("m9"), "iw4");
        assert_eq!(hmw_home("m79"), "iw4");
        assert_eq!(hmw_home("ak74u"), "iw3");
        assert_eq!(hmw_home("m40a3"), "iw3");
        assert_eq!(hmw_home("winchester1200"), "iw3");
        assert_eq!(hmw_home("colt45"), "iw3");
        assert_eq!(hmw_home("mors"), "h1");
        assert_eq!(hmw_home("boomhilda"), "h1");
        // (Black Ops II's own SCAR-H leaves Horizon MW's out as it leaves
        // out Modern Warfare 2's.)
        // (Horizon MW is listed whole, `COMPLETE`: its SCAR-H stays.)
        assert!(b.contains(&"h2_scar".to_owned()) && !b.contains(&"mw2_scar".to_owned()));
        // The mod's copies of Horizon MW's guns give way to them (its
        // Boomhilda and MORS, which no other game here has).
        for gone in ["boomhilda", "mors"] {
            assert!(a.contains(&gone.to_owned()), "the mod's {gone} not listed without Horizon MW");
            assert!(!b.contains(&gone.to_owned()), "the mod's {gone} listed beside Horizon MW's");
        }
        // Once each among themselves, numbered from HMW_FIRST_INDEX, a
        // secondary where Modern Warfare 2's is.
        let h2: Vec<&(String, String, String, String)> = with.iter().filter(|r| r.0.starts_with("h2_")).collect();
        let mut fams: Vec<String> = h2.iter().map(|r| family_of_reference(&r.0)).collect();
        let n = fams.len();
        fams.sort();
        fams.dedup();
        assert_eq!(fams.len(), n, "a Horizon MW family twice");
        assert_eq!(n, HMW.len(), "every Horizon MW gun (`COMPLETE`)");
        let m4 = h2.iter().find(|r| r.0 == "h2_m4").unwrap();
        assert!(m4.1.parse::<i32>().unwrap() >= HMW_FIRST_INDEX);
        assert_eq!(
            m4.2,
            "h1:iw4:weapon_m4carbine|iw4:weapon_m4|iw4:hud_icon_m4|iw3:weapon_m4|iw3:hud_icon_m4carbine|hud_icon_m4_grunt|hud_icon_m4"
        );
        assert_eq!(h2.iter().find(|r| r.0 == "h2_ranger").unwrap().3, "secondary");
        assert_eq!(game_for_index(HMW_FIRST_INDEX), Some("h1"));
        assert_eq!(reference_for_index(HMW_FIRST_INDEX).as_deref(), Some("h2_ak47"));
        let at4 = HMW.iter().find(|g| g.gun == "at4").unwrap();
        assert_eq!(script_name(at4), "h2_at4_mp");
        assert!(is_guest_name("h2_m4_mp"));
        assert_eq!(family_of_reference("h2_m4"), family_of_reference("mw2_m4"));
        // Each has its own description and text.
        let text = localize();
        for g in HMW {
            let d = GUN_LINES.iter().find(|(r, _)| *r == g.reference());
            assert!(d.is_some(), "{} has no line", g.reference());
            assert!(text.iter().any(|(k, t)| *k == format!("MPUI_BO2MP_DESC_{}", g.reference().to_ascii_uppercase()) && t.ends_with("From Horizon MW.")));
        }
        for game in GAMES {
            set_available(game, false);
        }
        set_available(MOD_GAME, false);
    }

    /// bo2mp: Infinite Warfare's guns are its own: listed beside the other
    /// games' guns of the same name (its M4 beside Horizon MW's, its classic
    /// Intervention beside Modern Warfare 2's), the other games' rows
    /// exactly as they were; once each among themselves; Black Ops II's own
    /// gun of a name still leaves them out; the mod's copy gives way.
    #[test]
    fn infinite_warfare_guns() {
        let _games = TEST_GAMES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let bo2 = |r: &str| {
            let mut row = vec![String::new(); 19];
            row[0] = "10".to_owned();
            row[2] = "weapon_assault".to_owned();
            row[4] = r.to_owned();
            row
        };
        let listed = |games: &[&str], own: &[&str]| {
            for game in GAMES.iter().chain(std::iter::once(&MOD_GAME)) {
                set_available(game, games.contains(game));
            }
            let mut rows: Vec<Vec<String>> = own.iter().map(|r| bo2(r)).collect();
            extend_stats_rows(&mut rows);
            rows.into_iter().map(|r| (r[4].clone(), r[0].clone(), r[6].clone(), r[13].clone(), r[2].clone())).collect::<Vec<_>>()
        };
        let others = ["t5", "iw4", "iw5", "iw6", "iw3", "t4", "s1", "t7", "h1", MOD_GAME];
        let mut with_games = others.to_vec();
        with_games.push("iw7");
        let without = listed(&others, &["scar"]);
        let with = listed(&with_games, &["scar"]);
        type Row = (String, String, String, String, String);
        let refs = |l: &[Row]| l.iter().map(|r| r.0.clone()).collect::<Vec<_>>();
        let (a, b) = (refs(&without), refs(&with));
        // Every other game's row is the same with Infinite Warfare or not,
        // but the mod's copy of an Infinite Warfare gun (its KBS Longbow),
        // which gives way to it.
        assert!(a.contains(&"kbs".to_owned()) && !b.contains(&"kbs".to_owned()));
        let not_iw7 = |l: &[Row]| l.iter().filter(|r| !r.0.starts_with("iw7_") && r.0 != "kbs").cloned().collect::<Vec<_>>();
        assert_eq!(not_iw7(&without), not_iw7(&with));
        for both in ["h2_m4", "mw2_cheytac", "h2_cheytac", "h2_spas12", "h2_glock", "h2_ump45"] {
            assert!(a.contains(&both.to_owned()) == b.contains(&both.to_owned()), "{both} changed");
        }
        // All of them, once each, on their own rows.
        let iw7: Vec<&Row> = with.iter().filter(|r| r.0.starts_with("iw7_")).collect();
        assert_eq!(iw7.len(), IW7.len());
        assert!(!a.iter().any(|r| r.starts_with("iw7_")));
        let mut fams: Vec<String> = iw7.iter().map(|r| family_of_reference(&r.0)).collect();
        fams.sort();
        fams.dedup();
        assert_eq!(fams.len(), IW7.len(), "an Infinite Warfare family twice");
        let mut idx: Vec<i32> = iw7.iter().map(|r| r.1.parse().unwrap()).collect();
        idx.sort_unstable();
        assert_eq!(idx.first(), Some(&IW7_FIRST_INDEX));
        assert_eq!(idx.last(), Some(&(IW7_FIRST_INDEX + IW7.len() as i32 - 1)));
        assert_eq!(game_for_index(IW7_FIRST_INDEX), Some("iw7"));
        assert_eq!(tab_game_for_index(IW7_FIRST_INDEX), Some("iw7"));
        assert_eq!(reference_for_index(IW7_FIRST_INDEX).as_deref(), Some("iw7_m4"));
        let row = |r: &str| *iw7.iter().find(|x| x.0 == r).unwrap();
        assert_eq!(row("iw7_m4").2, "iw7:hud_icon_weapon_m4|hud_icon_weapon_m4");
        assert_eq!(row("iw7_ump45c").2, "iw7:hud_icon_weapon_ump|hud_icon_weapon_ump45c");
        assert_eq!((row("iw7_emc").3.as_str(), row("iw7_emc").4.as_str()), ("secondary", "weapon_pistol"));
        assert_eq!((row("iw7_lockon").3.as_str(), row("iw7_lockon").4.as_str()), ("secondary", "weapon_launcher"));
        assert_eq!((row("iw7_mod2187").3.as_str(), row("iw7_mod2187").4.as_str()), ("primary", "weapon_cqb"));
        // (Black Ops II's own gun of a name leaves Infinite Warfare's out.)
        let own = listed(&with_games, &["scar", "ripper", "m1c"]);
        assert!(!refs(&own).contains(&"iw7_ripper".to_owned()) && !refs(&own).contains(&"iw7_m1c".to_owned()));
        assert!(refs(&own).contains(&"iw7_m4".to_owned()));
        // Names, scripts and text.
        let m4 = IW7.iter().find(|g| g.gun == "m4").unwrap();
        assert_eq!((m4.name, script_name(m4).as_str()), ("NV4", "iw7_m4_mp"));
        assert!(is_guest_name("iw7_ump45c_mp"));
        let text = localize();
        for g in IW7 {
            assert!(GUN_LINES.iter().any(|(r, _)| *r == g.reference()), "{} has no line", g.reference());
            assert!(text.iter().any(|(k, t)| *k == format!("MPUI_BO2MP_DESC_{}", g.reference().to_ascii_uppercase())
                && t.ends_with("From Call of Duty: Infinite Warfare.")));
            assert!(text.iter().any(|(k, t)| *k == format!("WEAPON_{}", g.reference().to_ascii_uppercase()) && t == g.name));
            assert!(g.attachments.is_empty() && !g.icon.is_empty());
        }
        for game in GAMES {
            set_available(game, false);
        }
        set_available(MOD_GAME, false);
    }

    /// Modern Warfare III (2023)'s guns: their own rows and names, the
    /// pistol a secondary, a Black Ops II-style Pack-a-Punch name.
    #[test]
    fn modern_warfare_iii_guns() {
        assert_eq!(JUP.len(), crate::jup_names::JUP_PLAYED.len());
        let renetti = JUP.iter().find(|g| g.gun == "jp12_pi_mike93").unwrap();
        assert_eq!(renetti.name, "Renetti");
        assert_eq!(renetti.slot, S);
        assert_eq!(renetti.reference(), "jup_jp12_pi_mike93");
        assert_eq!(script_name(renetti), "jup_jp12_pi_mike93_mp");
        assert_eq!(reference_for_index(JUP_FIRST_INDEX).as_deref(), Some("jup_jp12_pi_mike93"));
        assert_eq!(game_for_index(JUP_FIRST_INDEX), Some("jup"));
        assert!(is_guest_name("jup_jp12_pi_mike93_mp"));
        assert!(!has_own_pack_a_punch("jup"));
        assert_eq!(zm_upgraded_name(renetti), "Renetti-115");
        // (Named but not played: not listed.)
        assert!(JUP.iter().all(|g| g.gun != "jp01_ar_golf36"));
    }

    /// Modern Warfare II (2022)'s guns (`iw9_`, carried by the MWIII
    /// install): on their own tab, none on MWIII's, each listed once.
    #[test]
    fn modern_warfare_ii_guns_on_their_own_tab_once_each() {
        let _games = TEST_GAMES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for game in GAMES {
            set_available(game, game == "jup");
        }
        let index_of = |k: usize| stats_index(k, all().nth(k).unwrap());
        let mut mwii = 0;
        for (k, g) in all().enumerate().filter(|(_, g)| g.game == "jup") {
            let tab = tab_game_for_index(index_of(k));
            if g.gun.starts_with("iw9_") {
                assert_eq!(tab, Some("iw9"), "{}", g.gun);
                mwii += 1;
            } else {
                assert_eq!(tab, Some("jup"), "{}", g.gun);
            }
        }
        assert_eq!(mwii, crate::jup_names::JUP_PLAYED.iter().filter(|g| g.starts_with("iw9_")).count());
        assert!(mwii >= 40, "{mwii}");
        let keep = listed(&std::collections::HashSet::new());
        let kept_mwii: Vec<&str> = keep.iter().filter_map(|&k| all().nth(k)).filter(|g| g.game == "jup" && g.gun.starts_with("iw9_")).map(|g| g.gun).collect();
        let mut once = kept_mwii.clone();
        once.sort_unstable();
        once.dedup();
        assert_eq!(once.len(), kept_mwii.len(), "an MWII gun twice");
        assert_eq!(kept_mwii.len(), mwii, "an MWII gun left out");
        for game in GAMES {
            set_available(game, false);
        }
    }
}

/// MW2's Create-a-Class picture (`weapon_<this>`) where it is not named for
/// the gun (its IWDs name the pictures for the models).
fn mw2_picture(gun: &str) -> Option<&'static str> {
    Some(match gun {
        "m4" => "m4carbine",
        "m16" => "m16a4",
        "aug" => "steyr",
        "barrett" => "barrett50cal",
        "m21" => "m14ebr",
        "beretta" => "m9beretta",
        "deserteagle" => "desert_eagle",
        "coltanaconda" => "colt_anaconda",
        "usp" => "usp_45",
        "tmp" => "mp9_v2",
        "ranger" => "sawed_off",
        "wa2000" => "wa2000",
        _ => return None,
    })
}

/// World at War's Create-a-Class picture where it is not named for the gun.
fn waw_picture(gun: &str) -> Option<&'static str> {
    Some(match gun {
        "gewehr43" => "g43",
        "doublebarreledshotgun" => "double_barreled_shotgun",
        "shotgun" => "trenchgun",
        "type99lmg" => "type99_lmg",
        "mg42" => "weapon_mg42",
        "type99rifle" => "type99_arisaka",
        "ptrs41" => "weapon_ptrs41",
        "kar98k" => "kar98",
        "mosinrifle" => "mosinnagant",
        "springfield" => "springfield_scoped",
        "type100smg" => "type100",
        "stg44" => "mp44",
        _ => return None,
    })
}
