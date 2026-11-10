use crate::HostClassSlot;

#[derive(Debug, Clone, Copy)]
pub struct ClassPreset {
    pub name: &'static str,
    pub primary: &'static str,
    pub primary_attachments: &'static [&'static str],
    pub secondary: &'static str,
    pub secondary_attachments: &'static [&'static str],
    pub lethal: &'static str,
    pub tactical: &'static str,
    pub perks: [&'static str; 3],
    pub deathstreak: &'static str,
}

impl From<&ClassPreset> for HostClassSlot {
    fn from(preset: &ClassPreset) -> Self {
        let owned = |list: &[&str]| list.iter().map(|value| (*value).to_owned()).collect();
        Self {
            name: preset.name.to_owned(),
            primary: preset.primary.to_owned(),
            primary_attachments: owned(preset.primary_attachments),
            secondary: preset.secondary.to_owned(),
            secondary_attachments: owned(preset.secondary_attachments),
            lethal: preset.lethal.to_owned(),
            tactical: preset.tactical.to_owned(),
            perks: preset.perks.map(str::to_owned),
            deathstreak: preset.deathstreak.to_owned(),
        }
    }
}

static PREFER_BO2: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// bo2mp: Black Ops II is installed, so its own stock classes are the ones
/// the menus, the console and the match offer (the guest-game presets name
/// weapons a Black Ops II-only install does not have).
pub fn set_prefer_bo2_classes(on: bool) {
    PREFER_BO2.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn showcase_classes() -> &'static [ClassPreset] {
    if PREFER_BO2.load(std::sync::atomic::Ordering::Relaxed) {
        &BO2_STOCK
    } else {
        &SHOWCASE
    }
}

/// A Black Ops II stock custom class: a gun, a sidearm and the shared
/// equipment. Perks are left to the profile.
const fn bo2_custom(name: &'static str, primary: &'static str, secondary: &'static str) -> ClassPreset {
    ClassPreset {
        name,
        primary,
        primary_attachments: &[],
        secondary,
        secondary_attachments: &[],
        lethal: "t6:weapon/frag_grenade_mp",
        tactical: "t6:weapon/concussion_grenade_mp",
        perks: [""; 3],
        deathstreak: "",
    }
}

/// Black Ops II's ten stock classes: the five a reset custom class holds, then
/// the five presets. The custom five come first because the match is set up
/// from the first five, and the presets' attachments do not resolve yet.
const BO2_STOCK: [ClassPreset; 10] = [
    bo2_custom("class_custom_smg", "t6:weapon/mp7_mp", "t6:weapon/fiveseven_mp"),
    bo2_custom("class_custom_cqb", "t6:weapon/870mcs_mp", "t6:weapon/smaw_mp"),
    bo2_custom("class_custom_assault", "t6:weapon/tar21_mp", "t6:weapon/smaw_mp"),
    bo2_custom("class_custom_lmg", "t6:weapon/mk48_mp", "t6:weapon/smaw_mp"),
    bo2_custom("class_custom_sniper", "t6:weapon/svu_mp", "t6:weapon/fiveseven_mp"),
    ClassPreset {
        name: "class_smg",
        primary: "t6:weapon/evoskorpion_mp",
        primary_attachments: &["extclip", "grip"],
        secondary: "t6:weapon/beretta93r_mp",
        secondary_attachments: &["steadyaim"],
        lethal: "t6:weapon/satchel_charge_mp",
        tactical: "t6:weapon/willy_pete_mp",
        perks: [""; 3],
        deathstreak: "",
    },
    ClassPreset {
        name: "class_cqb",
        primary: "t6:weapon/srm1216_mp",
        primary_attachments: &["steadyaim"],
        secondary: "t6:weapon/crossbow_mp",
        secondary_attachments: &[],
        lethal: "t6:weapon/hatchet_mp",
        tactical: "t6:weapon/tactical_insertion_mp",
        perks: [""; 3],
        deathstreak: "",
    },
    ClassPreset {
        name: "class_assault",
        primary: "t6:weapon/hk416_mp",
        primary_attachments: &["reflex", "dualclip"],
        secondary: "t6:weapon/fhj18_mp",
        secondary_attachments: &[],
        lethal: "t6:weapon/frag_grenade_mp",
        tactical: "t6:weapon/proximity_grenade_mp",
        perks: [""; 3],
        deathstreak: "",
    },
    ClassPreset {
        name: "class_lmg",
        primary: "t6:weapon/lsat_mp",
        primary_attachments: &["rangefinder", "fastads", "fmj"],
        secondary: "",
        secondary_attachments: &[],
        lethal: "t6:weapon/sticky_grenade_mp",
        tactical: "t6:weapon/trophy_system_mp",
        perks: [""; 3],
        deathstreak: "",
    },
    ClassPreset {
        name: "class_sniper",
        primary: "t6:weapon/as50_mp",
        primary_attachments: &["swayreduc", "vzoom"],
        secondary: "t6:weapon/kard_mp",
        secondary_attachments: &["dw"],
        lethal: "t6:weapon/bouncingbetty_mp",
        tactical: "t6:weapon/sensor_grenade_mp",
        perks: [""; 3],
        deathstreak: "",
    },
];

const SHOWCASE: [ClassPreset; 15] = [
    ClassPreset {
        name: "ballistic_knife",
        primary: "t5:weapon/spectre_mp",
        primary_attachments: &["rf"],
        secondary: "t5:weapon/knife_ballistic_mp",
        secondary_attachments: &[],
        lethal: "iw4:weapon/throwingknife_mp",
        tactical: "iw4:weapon/flash_grenade_mp",
        perks: [
            "specialty_marathon",
            "specialty_lightweight",
            "specialty_extendedmelee",
        ],
        deathstreak: "specialty_finalstand",
    },
    ClassPreset {
        name: "ksg_breacher",
        primary: "iw5:weapon/iw5_ksg_mp",
        primary_attachments: &["grip"],
        secondary: "iw4:weapon/pp2000_mp",
        secondary_attachments: &["akimbo"],
        lethal: "iw4:weapon/semtex_mp",
        tactical: "iw4:weapon/concussion_grenade_mp",
        perks: [
            "specialty_marathon",
            "specialty_coldblooded",
            "specialty_extendedmelee",
        ],
        deathstreak: "specialty_combathigh",
    },
    ClassPreset {
        name: "intervention",
        primary: "iw4:weapon/cheytac_mp",
        primary_attachments: &["fmj", "heartbeat"],
        secondary: "iw4:weapon/javelin_mp",
        secondary_attachments: &[],
        lethal: "iw4:weapon/claymore_mp",
        tactical: "iw4:weapon/smoke_grenade_mp",
        perks: [
            "specialty_bling",
            "specialty_coldblooded",
            "specialty_localjammer",
        ],
        deathstreak: "specialty_copycat",
    },
    ClassPreset {
        name: "akimbo_uzi",
        primary: "iw4:weapon/uzi_mp",
        primary_attachments: &["akimbo", "rof"],
        secondary: "iw4:weapon/model1887_mp",
        secondary_attachments: &["akimbo"],
        lethal: "iw4:weapon/frag_grenade_mp",
        tactical: "iw4:weapon/flash_grenade_mp",
        perks: [
            "specialty_bling",
            "specialty_lightweight",
            "specialty_extendedmelee",
        ],
        deathstreak: "specialty_combathigh",
    },
    ClassPreset {
        name: "shield_tacknife",
        primary: "iw4:weapon/riotshield_mp",
        primary_attachments: &[],
        secondary: "iw4:weapon/usp_mp",
        secondary_attachments: &["tactical", "fmj"],
        lethal: "iw4:weapon/throwingknife_mp",
        tactical: "iw4:weapon/concussion_grenade_mp",
        perks: [
            "specialty_bling",
            "specialty_lightweight",
            "specialty_extendedmelee",
        ],
        deathstreak: "specialty_finalstand",
    },
    ClassPreset {
        name: "crossbow",
        primary: "t5:weapon/galil_mp",
        primary_attachments: &["acog"],
        secondary: "t5:weapon/crossbow_explosive_mp",
        secondary_attachments: &[],
        lethal: "iw4:weapon/claymore_mp",
        tactical: "iw4:weapon/smoke_grenade_mp",
        perks: [
            "specialty_scavenger",
            "specialty_coldblooded",
            "specialty_heartbreaker",
        ],
        deathstreak: "specialty_copycat",
    },
    ClassPreset {
        name: "china_lake",
        primary: "t5:weapon/commando_mp",
        primary_attachments: &["silencer"],
        secondary: "t5:weapon/china_lake_mp",
        secondary_attachments: &[],
        lethal: "iw4:weapon/c4_mp",
        tactical: "iw4:weapon/concussion_grenade_mp",
        perks: [
            "specialty_scavenger",
            "specialty_explosivedamage",
            "specialty_detectexplosive",
        ],
        deathstreak: "specialty_grenadepulldeath",
    },
    ClassPreset {
        name: "cz75_auto",
        primary: "t5:weapon/ak74u_mp",
        primary_attachments: &["grip"],
        secondary: "t5:weapon/cz75_mp",
        secondary_attachments: &["auto"],
        lethal: "iw4:weapon/frag_grenade_mp",
        tactical: "iw4:weapon/flash_grenade_mp",
        perks: [
            "specialty_fastreload",
            "specialty_lightweight",
            "specialty_bulletaccuracy",
        ],
        deathstreak: "specialty_copycat",
    },
    ClassPreset {
        name: "strela_lmg",
        primary: "t5:weapon/stoner63_mp",
        primary_attachments: &["reflex"],
        secondary: "t5:weapon/strela_mp",
        secondary_attachments: &[],
        lethal: "iw4:weapon/semtex_mp",
        tactical: "iw4:weapon/smoke_grenade_mp",
        perks: [
            "specialty_scavenger",
            "specialty_hardline",
            "specialty_bulletaccuracy",
        ],
        deathstreak: "specialty_copycat",
    },
    ClassPreset {
        name: "xm25",
        primary: "iw5:weapon/iw5_type95_mp",
        primary_attachments: &["reflex"],
        secondary: "iw5:weapon/xm25_mp",
        secondary_attachments: &[],
        lethal: "iw4:weapon/frag_grenade_mp",
        tactical: "iw4:weapon/concussion_grenade_mp",
        perks: [
            "specialty_fastreload",
            "specialty_bulletdamage",
            "specialty_bulletaccuracy",
        ],
        deathstreak: "specialty_combathigh",
    },
    ClassPreset {
        name: "fmg_akimbo",
        primary: "iw5:weapon/iw5_mp7_mp",
        primary_attachments: &["silencer"],
        secondary: "iw5:weapon/iw5_fmg9_mp",
        secondary_attachments: &["akimbo"],
        lethal: "iw4:weapon/throwingknife_mp",
        tactical: "iw4:weapon/flash_grenade_mp",
        perks: [
            "specialty_marathon",
            "specialty_coldblooded",
            "specialty_heartbreaker",
        ],
        deathstreak: "specialty_finalstand",
    },
    ClassPreset {
        name: "hamr_magnum",
        primary: "iw5:weapon/iw5_pp90m1_mp",
        primary_attachments: &["hamrhybrid"],
        secondary: "iw5:weapon/iw5_44magnum_mp",
        secondary_attachments: &["tactical"],
        lethal: "iw4:weapon/frag_grenade_mp",
        tactical: "iw4:weapon/flash_grenade_mp",
        perks: [
            "specialty_fastreload",
            "specialty_lightweight",
            "specialty_extendedmelee",
        ],
        deathstreak: "specialty_combathigh",
    },
    ClassPreset {
        name: "bo_spas",
        primary: "t5:weapon/spas_mp",
        primary_attachments: &["silencer"],
        secondary: "iw4:weapon/glock_mp",
        secondary_attachments: &["akimbo"],
        lethal: "iw4:weapon/c4_mp",
        tactical: "iw4:weapon/flash_grenade_mp",
        perks: [
            "specialty_marathon",
            "specialty_lightweight",
            "specialty_extendedmelee",
        ],
        deathstreak: "specialty_grenadepulldeath",
    },
    ClassPreset {
        name: "mg4_bling",
        primary: "iw4:weapon/mg4_mp",
        primary_attachments: &["grip", "heartbeat"],
        secondary: "iw4:weapon/at4_mp",
        secondary_attachments: &[],
        lethal: "iw4:weapon/frag_grenade_mp",
        tactical: "iw4:weapon/smoke_grenade_mp",
        perks: [
            "specialty_bling",
            "specialty_bulletdamage",
            "specialty_detectexplosive",
        ],
        deathstreak: "specialty_copycat",
    },
    ClassPreset {
        name: "msr_python",
        primary: "iw5:weapon/iw5_msr_mp",
        primary_attachments: &["silencer03"],
        secondary: "t5:weapon/python_mp",
        secondary_attachments: &["speed"],
        lethal: "iw4:weapon/claymore_mp",
        tactical: "iw4:weapon/flash_grenade_mp",
        perks: [
            "specialty_marathon",
            "specialty_coldblooded",
            "specialty_heartbreaker",
        ],
        deathstreak: "specialty_finalstand",
    },
];

pub fn pick_showcase(seed: u64, count: usize) -> Vec<&'static ClassPreset> {
    let mut order: Vec<&'static ClassPreset> = showcase_classes().iter().collect();
    let mut state = seed;
    for i in (1..order.len()).rev() {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        order.swap(i, (z % (i as u64 + 1)) as usize);
    }
    order.truncate(count);
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bo2_stock_classes_use_only_t6_weapons() {
        assert_eq!(BO2_STOCK.len(), 10);
        for class in &BO2_STOCK {
            for key in [class.primary, class.secondary, class.lethal, class.tactical] {
                assert!(key.is_empty() || key.starts_with("t6:weapon/"), "{}: {key}", class.name);
            }
        }
    }

    #[test]
    fn the_bo2_table_is_chosen_only_when_asked_for() {
        set_prefer_bo2_classes(true);
        assert_eq!(showcase_classes()[0].name, "class_custom_smg");
        set_prefer_bo2_classes(false);
        assert_eq!(showcase_classes()[0].name, "ballistic_knife");
    }
}
