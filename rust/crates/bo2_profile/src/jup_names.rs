//! bo2mp: Modern Warfare III (2023)'s gun names, by hand.
//!
//! The game's display names are localized strings it keeps obfuscated in
//! its zones, and those are not read; this table gives each gun the
//! public name it has in the game, by its internal name
//! (`jup_jp01_ar_golf36` is the Holger 556). Only guns whose public name is
//! known for certain are listed; the others are left out rather than
//! guessed. Each row: the internal name without its `jup_` prefix, the
//! public name, and Black Ops II's Create-a-Class group for it (battle
//! rifles as assault rifles, marksman rifles as sniper rifles, as Black
//! Ops II has no class of either), and its Create-a-Class slot.

/// (internal name without `jup_`, public name, BO2 item group, slot).
pub const JUP_NAMES: &[(&str, &str, &str, &str)] = &[
    // Assault rifles.
    ("jp01_ar_golf36", "Holger 556", "weapon_assault", "primary"),
    ("jp19_ar_acharlie", "MCW", "weapon_assault", "primary"),
    ("jp36_ar_anov94", "SVA 545", "weapon_assault", "primary"),
    ("jp44_ar_stango44", "STG44", "weapon_assault", "primary"),
    ("jp08_ar_qbravo97", "DG-56", "weapon_assault", "primary"),
    ("jp34_ar_balpha27", "BAL-27", "weapon_assault", "primary"),
    ("jp05_ar_foxtrot2000", "FR 5.56", "weapon_assault", "primary"),
    // Battle rifles.
    ("cp08_br_xmike5", "Sidewinder", "weapon_assault", "primary"),
    ("jp19_br_acharlie450", "MCW 6.8", "weapon_assault", "primary"),
    ("jp03_br_aromeo200", "BAS-B", "weapon_assault", "primary"),
    // Submachine guns.
    ("jp04_sm_umike", "Striker", "weapon_smg", "primary"),
    ("jp07_sm_uzulu", "WSP Swarm", "weapon_smg", "primary"),
    ("jp41_sm_kpappa50", "Static-HV", "weapon_smg", "primary"),
    ("jp25_sm_talpham", "RAM-9", "weapon_smg", "primary"),
    // Light machine guns.
    ("jp01_lm_mgolf36", "Holger 26", "weapon_lmg", "primary"),
    ("jp06_lm_pkilop", "Pulemyot 762", "weapon_lmg", "primary"),
    ("jp08_lm_qbravo95lsw", "DG-58 LSW", "weapon_lmg", "primary"),
    // Shotguns.
    ("jp09_sh_oromeo12", "Riveter", "weapon_cqb", "primary"),
    ("jp16_sh_recho870", "Lockwood 680", "weapon_cqb", "primary"),
    ("jp38_sh_spapa12", "Reclaimer 18", "weapon_cqb", "primary"),
    // Marksman rifles.
    ("jp37_dm_kappa98", "KAR98K", "weapon_sniper", "primary"),
    // Sniper rifles.
    ("jp10_sn_cdelta50", "KATT-AMR", "weapon_sniper", "primary"),
    ("jp17_sn_hsierra", "XRK Stalker", "weapon_sniper", "primary"),
    // Pistols.
    ("jp12_pi_mike93", "Renetti", "weapon_pistol", "secondary"),
    // Modern Warfare II's guns that Modern Warfare III carries (`iw9_`).
    ("iw9_ar_mike4", "M4", "weapon_assault", "primary"),
    ("iw9_ar_mike16", "M16", "weapon_assault", "primary"),
    ("iw9_ar_akilo", "Kastov 762", "weapon_assault", "primary"),
    ("iw9_ar_akilo105", "Kastov 545", "weapon_assault", "primary"),
    ("iw9_ar_akilo74", "Kastov-74u", "weapon_assault", "primary"),
    ("iw9_ar_kilo53", "Lachmann-556", "weapon_assault", "primary"),
    ("iw9_ar_golf3", "Lachmann-762", "weapon_assault", "primary"),
    ("iw9_ar_scharlie", "TAQ-56", "weapon_assault", "primary"),
    ("iw9_ar_mcbravo", "M13B", "weapon_assault", "primary"),
    ("iw9_ar_mcharlie", "M13C", "weapon_assault", "primary"),
    ("iw9_ar_falpha", "FR Avancer", "weapon_assault", "primary"),
    ("iw9_dm_soscar14", "SO-14", "weapon_assault", "primary"),
    ("iw9_dm_scromeo", "TAQ-M", "weapon_sniper", "primary"),
    ("iw9_dm_mike14", "EBR-14", "weapon_sniper", "primary"),
    ("iw9_dm_stango25", "Tempus Torrent", "weapon_sniper", "primary"),
    ("iw9_dm_pgolf1", "LM-S", "weapon_sniper", "primary"),
    ("iw9_dm_mike24", "SP-R 208", "weapon_sniper", "primary"),
    ("iw9_sm_mpapa5", "Lachmann Sub", "weapon_smg", "primary"),
    ("iw9_sm_mpapa7", "VEL 46", "weapon_smg", "primary"),
    ("iw9_sm_papa90", "PDSW 528", "weapon_smg", "primary"),
    ("iw9_sm_victor", "Fennec 45", "weapon_smg", "primary"),
    ("iw9_sm_beta", "Minibak", "weapon_smg", "primary"),
    ("iw9_sm_alpha57", "FSS Hurricane", "weapon_smg", "primary"),
    ("iw9_sm_mpapax", "BAS-P", "weapon_smg", "primary"),
    ("iw9_sm_acharlie45", "ISO 45", "weapon_smg", "primary"),
    ("iw9_sm_acharlie9", "ISO 9mm", "weapon_smg", "primary"),
    ("iw9_sm_aviktor", "Vaznev-9K", "weapon_smg", "primary"),
    ("iw9_sm_apapa", "MX9", "weapon_smg", "primary"),
    ("iw9_lm_rkilo", "RPK", "weapon_lmg", "primary"),
    ("iw9_lm_kilo21", "RAPP H", "weapon_lmg", "primary"),
    ("iw9_lm_ngolf7", "Sakin MG38", "weapon_lmg", "primary"),
    ("iw9_sh_vecho", "KV Broadside", "weapon_cqb", "primary"),
    ("iw9_sh_mike1014", "Expedite 12", "weapon_cqb", "primary"),
    ("iw9_sh_charlie725", "Lockwood 300", "weapon_cqb", "primary"),
    ("iw9_sh_tsierra12", "MX Guardian", "weapon_cqb", "primary"),
    ("iw9_sn_alpha50", "Victus XMR", "weapon_sniper", "primary"),
    ("iw9_sn_mromeo", "MCPR-300", "weapon_sniper", "primary"),
    ("iw9_sn_walpha", "Carrack .300", "weapon_sniper", "primary"),
    ("iw9_pi_golf17", "X12", "weapon_pistol", "secondary"),
    ("iw9_pi_decho", ".50 GS", "weapon_pistol", "secondary"),
    ("iw9_pi_swhiskey", "Basilisk", "weapon_pistol", "secondary"),
    ("iw9_pi_papa220", "P890", "weapon_pistol", "secondary"),
    ("iw9_pi_mike2011", "9mm Daemon", "weapon_pistol", "secondary"),
    ("iw9_pi_tango9", "FTAC Siege", "weapon_pistol", "secondary"),
];

/// The guns played as guest guns (their models, materials and handling are
/// built by the match, `assets::lane::jup_gun`, which has a row for each);
/// the other names wait for theirs.
pub const JUP_PLAYED: &[&str] = &[
    "jp12_pi_mike93",
    "jp36_ar_anov94",
    "jp19_ar_acharlie",
    "jp34_ar_balpha27",
    "jp05_ar_foxtrot2000",
    "jp08_ar_qbravo97",
    "jp44_ar_stango44",
    "jp19_br_acharlie450",
    "jp03_br_aromeo200",
    "jp37_dm_kappa98",
    "jp06_lm_pkilop",
    "jp08_lm_qbravo95lsw",
    "jp38_sh_spapa12",
    "jp41_sm_kpappa50",
    "jp25_sm_talpham",
    "jp10_sn_cdelta50",
    "jp17_sn_hsierra",
    "iw9_ar_mike4",
    "iw9_ar_mike16",
    "iw9_ar_akilo",
    "iw9_ar_akilo105",
    "iw9_ar_akilo74",
    "iw9_ar_kilo53",
    "iw9_ar_golf3",
    "iw9_ar_scharlie",
    "iw9_ar_mcbravo",
    "iw9_ar_mcharlie",
    "iw9_ar_falpha",
    "iw9_dm_soscar14",
    "iw9_dm_scromeo",
    "iw9_dm_mike14",
    "iw9_dm_stango25",
    "iw9_dm_pgolf1",
    "iw9_dm_mike24",
    "iw9_sm_mpapa5",
    "iw9_sm_mpapa7",
    "iw9_sm_papa90",
    "iw9_sm_victor",
    "iw9_sm_beta",
    "iw9_sm_alpha57",
    "iw9_sm_mpapax",
    "iw9_sm_acharlie45",
    "iw9_sm_acharlie9",
    "iw9_sm_aviktor",
    "iw9_sm_apapa",
    "iw9_lm_rkilo",
    "iw9_lm_kilo21",
    "iw9_lm_ngolf7",
    "iw9_sh_vecho",
    "iw9_sh_mike1014",
    "iw9_sh_charlie725",
    "iw9_sh_tsierra12",
    "iw9_sn_alpha50",
    "iw9_sn_mromeo",
    "iw9_sn_walpha",
    "iw9_pi_golf17",
    "iw9_pi_decho",
    "iw9_pi_swhiskey",
    "iw9_pi_papa220",
    "iw9_pi_mike2011",
    "iw9_pi_tango9",
];

/// A Modern Warfare III gun's public name by its internal name, with or
/// without `jup_` and `_mp` (`jup_jp01_ar_golf36_mp` -> `Holger 556`).
#[must_use]
pub fn public_name(internal: &str) -> Option<&'static str> {
    let n = internal.to_ascii_lowercase();
    let n = n.strip_prefix("jup_").unwrap_or(&n);
    let n = n.strip_suffix("_mp").unwrap_or(n);
    JUP_NAMES.iter().find(|(code, ..)| *code == n).map(|(_, name, ..)| *name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_by_internal_name() {
        assert_eq!(public_name("jup_jp01_ar_golf36_mp"), Some("Holger 556"));
        assert_eq!(public_name("jp01_lm_mgolf36"), Some("Holger 26"));
        assert_eq!(public_name("jup_jp99_ar_unknown"), None);
    }

    #[test]
    fn each_gun_once() {
        let mut seen = std::collections::HashSet::new();
        for (code, name, ..) in JUP_NAMES {
            assert!(seen.insert(*code), "{code} twice");
            assert!(seen.insert(*name), "{name} twice");
        }
    }
}

/// A played gun's picture names in `global_stream_mp`, the full-colour
/// Create-a-Class pictures first (`icon_weapon_jup_<gun>_mp`; for the Modern
/// Warfare II guns it carries, `iw9_ar_mike4`, `icon_weapon_ar_mike4`, and
/// the same spelled the way the new guns' are, in case a remade picture
/// is named so), then the kill-feed icon (`hud_icon_weapon_...`), a white
/// silhouette that is only the last resort.
#[must_use]
pub fn icon_names(gun: &str) -> Vec<String> {
    let mut v = Vec::new();
    match gun.strip_prefix("iw9_") {
        Some(g) => {
            v.extend([format!("icon_weapon_{g}"), format!("icon_weapon_{g}_mp"), format!("icon_weapon_jup_{g}_mp"), format!("icon_weapon_iw9_{g}")]);
            v.extend([format!("hud_icon_weapon_{g}"), format!("hud_icon_weapon_{g}_mp")]);
        }
        None => {
            v.extend([format!("icon_weapon_jup_{gun}_mp"), format!("icon_weapon_jup_{gun}"), format!("icon_weapon_{gun}_mp"), format!("icon_weapon_{gun}")]);
            v.push(format!("hud_icon_weapon_jup_{gun}_mp"));
        }
    }
    v
}

/// Whether a picture name is a kill-feed icon (a white silhouette), not a
/// full-colour Create-a-Class picture.
#[must_use]
pub fn is_silhouette(name: &str) -> bool {
    name.starts_with("hud_icon_")
}
