//! bo2mp: guest guns' attachments as Create-a-Class offers them.
//!
//! An attachment of an Infinity Ward game (Ghosts, Advanced Warfare,
//! Modern Warfare Remastered) is named by its kind (`opticsreddot`,
//! `reflexsmg`, `silencer01`, `xmags`), its models hang on the gun's bone
//! named as their root (`tag_red_dot`, `tag_silencer`) and the gun names
//! the animations it plays with it (aiming down its sight). Here it gets
//! its Create-a-Class word, the Black Ops II attachment whose numbers it
//! takes and the Black Ops II sight it shows aimed (`asset_game::
//! GuestAttachment`). A sniper's own scope (`<gun>scope`) is the gun's
//! default; aimed, it is Black Ops II's sniper scope.

use asset_game::{GuestAttachment, SNIPER_SCOPE, WeaponSoundSlot};

/// A guest gun's class, for the Black Ops II attachment of its kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GunClass {
    Rifle,
    Smg,
    Lmg,
    Sniper,
    Shotgun,
    Pistol,
}

impl GunClass {
    /// From a statstable item group (`weapon_smg`).
    pub(crate) fn from_group(group: &str) -> Self {
        match group {
            "weapon_smg" => Self::Smg,
            "weapon_lmg" => Self::Lmg,
            "weapon_sniper" => Self::Sniper,
            "weapon_cqb" => Self::Shotgun,
            "weapon_pistol" => Self::Pistol,
            _ => Self::Rifle,
        }
    }
}

/// What an attachment is: its word, whether it is a sight, Black Ops II's
/// attachment of its kind and the Black Ops II weapon whose sight it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Kind {
    pub word: &'static str,
    pub sight: bool,
    bo2: Option<&'static str>,
    sight_from: Option<&'static str>,
}

const fn sight(word: &'static str, bo2: &'static str, from: &'static str) -> Kind {
    Kind {
        word,
        sight: true,
        bo2: Some(bo2),
        sight_from: Some(from),
    }
}

const fn other(word: &'static str, bo2: Option<&'static str>) -> Kind {
    Kind {
        word,
        sight: false,
        bo2,
        sight_from: None,
    }
}

/// The Black Ops II gun whose Dual Band a thermal sight is aimed through: a
/// sniper's is the sniper scope's, any other gun's the assault rifle's (its
/// small screen, not a sniper's full surround and zoom).
const fn ir_source(class: GunClass) -> &'static str {
    match class {
        GunClass::Sniper => "dsr50_mp+ir",
        _ => "an94_mp+ir",
    }
}

/// An Infinity Ward attachment's kind by its name (`opticsreddot`,
/// `reflexsmg`, `silencer03`); `None` for one not offered (under-barrel
/// weapons, perks, cosmetic and loot attachments, the gun's own scope).
pub(crate) fn iw_kind(name: &str, class: GunClass) -> Option<Kind> {
    let n = name.trim_start_matches(',').to_ascii_lowercase();
    let sniper = class == GunClass::Sniper;
    let k = if n.starts_with("opticsreddot") || n.starts_with("reflex") || n.starts_with("reddot") {
        sight(
            "reflex",
            if class == GunClass::Pistol {
                "reflex_pistol"
            } else {
                "reflex"
            },
            "an94_mp+reflex",
        )
    } else if n.starts_with("opticseotech") || n.starts_with("eotech") || n.starts_with("holo") {
        sight("eotech", "holo", "an94_mp+holo")
    } else if n.starts_with("opticsacog") || n.starts_with("acog") {
        if sniper {
            sight("acog", "acog_sniper", "dsr50_mp+acog")
        } else {
            sight("acog", "acog", "an94_mp+acog")
        }
    } else if n.starts_with("opticsthermal") || n.starts_with("thermal") {
        sight(
            "thermal",
            if class == GunClass::Lmg {
                "ir_lmg"
            } else {
                "ir"
            },
            ir_source(class),
        )
    } else if n.starts_with("variablereddot")
        || n.starts_with("hybrid")
        || n.starts_with("hamrhybrid")
    {
        sight("hybrid", "dualoptic", "an94_mp+dualoptic")
    } else if n.starts_with("opticstargetenhancer") || n.starts_with("tracker") {
        sight("tracker", "rangefinder", "an94_mp+rangefinder")
    } else if n.starts_with("vzscope") || (sniper && n.ends_with("scopevz")) {
        sight("vzscope", "vzoom", "dsr50_mp+vzoom")
    } else if n.starts_with("silencer") || n.starts_with("suppressor") {
        other(
            "silencer",
            Some(match class {
                GunClass::Smg | GunClass::Pistol => "silencer_smg",
                GunClass::Shotgun => "silencer_shotgun",
                _ => "silencer",
            }),
        )
    } else if n.starts_with("xmag") || n.starts_with("extclip") {
        other(
            "xmags",
            Some(match class {
                GunClass::Lmg => "extclip_lmg",
                GunClass::Sniper => "extclip_sniper",
                _ => "extclip",
            }),
        )
    } else if n.starts_with("foregrip") || (n.starts_with("grip") && !n.contains("hide")) {
        other("grip", Some("grip"))
    } else if n.starts_with("lasersight") {
        other(
            "steadyaim",
            Some(if sniper {
                "steadyaim_sniper"
            } else {
                "steadyaim"
            }),
        )
    } else if n.starts_with("quickdraw") {
        other(
            "fastads",
            Some(if class == GunClass::Smg {
                "fastads_smg"
            } else {
                "fastads"
            }),
        )
    } else if n == "stock" {
        other(
            "stalker",
            Some(if class == GunClass::Smg {
                "stalker_smg"
            } else {
                "stalker"
            }),
        )
    } else if n.starts_with("longrange") {
        other("extbarrel", Some("extbarrel"))
    } else if n.starts_with("barrelrange") {
        other("barrelrange", Some("extbarrel"))
    } else if n.starts_with("barrelbored") {
        other("barrelbored", Some("extbarrel"))
    } else if n.starts_with("dualmag") {
        other("dualclip", Some("dualclip"))
    } else if n == "fmj" {
        other("fmj", Some("fmj"))
    } else if n.starts_with("rof") {
        other("rof", Some("rf"))
    } else if n.starts_with("flashsuppress") {
        other("flashsuppress", None)
    } else {
        return None;
    };
    Some(k)
}

/// A Black Ops III attachment's kind by its unique's name for the gun
/// (`au_ar_standard_<this>`): Black Ops II's word where it has one.
pub(crate) fn t7_kind(name: &str, class: GunClass) -> Option<Kind> {
    let sniper = class == GunClass::Sniper;
    let k = match name {
        "reflex" => sight(
            "reflex",
            if class == GunClass::Pistol {
                "reflex_pistol"
            } else {
                "reflex"
            },
            "an94_mp+reflex",
        ),
        // (The ELO.)
        "reddot" => sight("elbit", "reflex", "an94_mp+reflex"),
        "holo" => sight("holo", "holo", "an94_mp+holo"),
        "acog" if sniper => sight("acog", "acog_sniper", "dsr50_mp+acog"),
        "acog" => sight("acog", "acog", "an94_mp+acog"),
        "dualoptic" => sight("dualoptic", "dualoptic", "an94_mp+dualoptic"),
        "ir" => sight(
            "ir",
            if class == GunClass::Lmg {
                "ir_lmg"
            } else {
                "ir"
            },
            ir_source(class),
        ),
        "suppressed" => other("silencer", Some(bo2_attachment("silencer", class))),
        "extclip" => other("extclip", Some(bo2_attachment("extclip", class))),
        "fastreload" => other("dualclip", Some("dualclip")),
        "extbarrel" => other("extbarrel", Some("extbarrel")),
        "fmj" => other("fmj", Some("fmj")),
        "grip" => other("grip", Some("grip")),
        "quickdraw" => other("fastads", Some(bo2_attachment("fastads", class))),
        "rf" => other("rf", Some("rf")),
        "stalker" => other("stalker", Some(bo2_attachment("stalker", class))),
        "steadyaim" => other("steadyaim", Some(bo2_attachment("steadyaim", class))),
        "swayreduc" if sniper => other("swayreduc", Some("swayreduc")),
        _ => return None,
    };
    Some(k)
}

/// Black Ops II's attachment of a Black Ops II word for a gun of `class`
/// (`extclip` on a light machine gun is `extclip_lmg`).
pub(crate) fn bo2_attachment(word: &str, class: GunClass) -> &str {
    use GunClass as C;
    match (word, class) {
        ("silencer", C::Smg | C::Pistol) => "silencer_smg",
        ("silencer", C::Shotgun) => "silencer_shotgun",
        ("extclip", C::Lmg) => "extclip_lmg",
        ("extclip", C::Sniper) => "extclip_sniper",
        ("fastads", C::Smg) => "fastads_smg",
        ("fastads", C::Lmg) => "fastads_lmg",
        ("stalker", C::Smg) => "stalker_smg",
        ("stalker", C::Lmg) => "stalker_lmg",
        ("steadyaim", C::Smg | C::Pistol) => "steadyaim_smg",
        ("steadyaim", C::Sniper) => "steadyaim_sniper",
        ("acog", C::Sniper) => "acog_sniper",
        ("ir", C::Lmg) => "ir_lmg",
        ("vzoom", C::Lmg) => "vzoom_lmg",
        ("reflex", C::Pistol) => "reflex_pistol",
        _ => word,
    }
}

/// Whether an attachment is the gun's own scope (`morsscope`,
/// `l115a3scope`): what the gun carries when no sight is picked.
pub(crate) fn is_own_scope(name: &str, gun: &str) -> bool {
    let n = name.trim_start_matches(',').to_ascii_lowercase();
    let gun = gun.to_ascii_lowercase();
    let gun = gun.rsplit('_').next().unwrap_or(&gun);
    n == format!("{gun}scope")
        || (n.ends_with("scope") && n.starts_with(gun) && !n.ends_with("vzscope"))
}

/// The sound slot an attachment's sound override fills, by its alias
/// (`wpn_ak12_sup_npc`, `wpn_ak12_sup_plr`).
pub(crate) fn fire_slot(alias: &str) -> Option<WeaponSoundSlot> {
    let a = alias.to_ascii_lowercase();
    // (Ghosts' left akimbo gun's own: `weap_p226_fire_aki`.)
    if a.ends_with("_aki") {
        Some(WeaponSoundSlot::FirePlayerAkimbo)
    } else if a.contains("plr") || a.contains("player") {
        Some(WeaponSoundSlot::FirePlayer)
    } else if a.contains("npc") {
        Some(WeaponSoundSlot::Fire)
    } else {
        None
    }
}

/// The sound slots a Ghosts attachment sound override fills, by the
/// entry's own sound type (1 fire, 2 player fire, 3 player akimbo fire,
/// 4 last shot for both views), as Ghosts picks them; see
/// `docs/exe_notes/IW6_VFX_AUDIO.md` (fire sounds).
pub(crate) fn iw6_override_slots(sound_type: u32) -> &'static [WeaponSoundSlot] {
    match sound_type {
        1 => &[WeaponSoundSlot::Fire],
        2 => &[WeaponSoundSlot::FirePlayer],
        3 => &[WeaponSoundSlot::FirePlayerAkimbo],
        4 => &[WeaponSoundSlot::FireLast, WeaponSoundSlot::FireLastPlayer],
        _ => &[],
    }
}

/// `BO2MP_IW6_FIRE_SOUND_TYPE=1`: a Ghosts gun's attachment fire sounds
/// take their slot from the override's sound type instead of the alias
/// name (`fire_slot`). Off by default: on Ghosts' own data the names agree
/// for fire and player fire; the type adds the last-shot overrides.
pub(crate) fn iw6_fire_sound_by_type() -> bool {
    super::bo3_look::game_on("BO2MP_IW6_FIRE_SOUND_TYPE", false)
}

/// The Black Ops II gun of a class whose attachments' sounds stand in for
/// a guest gun's that has none of its own.
fn bo2_gun(class: GunClass) -> &'static str {
    match class {
        GunClass::Rifle => "an94",
        GunClass::Smg => "mp7",
        GunClass::Lmg => "mk48",
        GunClass::Sniper => "dsr50",
        GunClass::Shotgun => "saiga12",
        GunClass::Pistol => "fiveseven",
    }
}

/// The attachment for the variants: `kind`'s word, Black Ops II numbers and
/// sight, with its own models (on their root bones), animations and fire
/// sounds (its last shot the same); a suppressor without sounds of its own
/// takes Black Ops II's of the gun's class.
pub(crate) fn guest_attachment(
    kind: Kind,
    class: GunClass,
    view_model: Option<String>,
    world_model: Option<String>,
    anims: Vec<(usize, String)>,
    sounds: Vec<(WeaponSoundSlot, String)>,
) -> GuestAttachment {
    let mut all = sounds.clone();
    for (slot, alias) in &sounds {
        let last = match slot {
            WeaponSoundSlot::Fire => WeaponSoundSlot::FireLast,
            WeaponSoundSlot::FirePlayer => WeaponSoundSlot::FireLastPlayer,
            _ => continue,
        };
        if !all.iter().any(|(s, _)| *s == last) {
            all.push((last, alias.clone()));
        }
    }
    GuestAttachment {
        word: kind.word.to_owned(),
        sight: kind.sight,
        view_models: view_model.into_iter().map(|m| (m, None)).collect(),
        world_models: world_model.into_iter().collect(),
        anims,
        sounds: all,
        bo2: kind.bo2.map(str::to_owned),
        sight_from: kind.sight_from.map(str::to_owned),
        sounds_from: (kind.word == "silencer").then(|| format!("{}_silencer", bo2_gun(class))),
        ..GuestAttachment::default()
    }
}

/// A sniper's own scope: its models, Black Ops II's sniper scope aimed.
pub(crate) fn own_scope(
    view_model: Option<String>,
    world_model: Option<String>,
) -> GuestAttachment {
    GuestAttachment {
        word: String::new(),
        sight: true,
        view_models: view_model.into_iter().map(|m| (m, None)).collect(),
        world_models: world_model.into_iter().collect(),
        sight_from: Some(SNIPER_SCOPE.to_owned()),
        ..GuestAttachment::default()
    }
}

/// A sniper's scope when its lane found none of its own on the gun (its
/// game names it otherwise, or the scope is part of the gun's model):
/// aimed, Black Ops II's sniper scope all the same.
pub(crate) fn sniper_scope_or(
    scope: Option<GuestAttachment>,
    class: GunClass,
) -> Option<GuestAttachment> {
    scope.or_else(|| (class == GunClass::Sniper).then(|| own_scope(None, None)))
}

/// A sniper's own scope at its placements on the gun.
pub(crate) fn own_scope_placed(
    views: Vec<(String, asset_model::FpvMountOffset)>,
) -> GuestAttachment {
    GuestAttachment {
        view_models: views.into_iter().map(|(m, at)| (m, Some(at))).collect(),
        ..own_scope(None, None)
    }
}

/// The words a gun's variants carry, in its list's order (Create-a-Class's
/// list for the gun).
pub(crate) fn words(list: &[GuestAttachment]) -> String {
    let mut out: Vec<&str> = Vec::new();
    for a in list {
        if !a.word.is_empty() && !out.contains(&a.word.as_str()) {
            out.push(&a.word);
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ghosts_sound_override_fills_the_slot_of_its_type() {
        assert_eq!(iw6_override_slots(1), &[WeaponSoundSlot::Fire]);
        assert_eq!(iw6_override_slots(2), &[WeaponSoundSlot::FirePlayer]);
        // Akimbo (3) is the left gun's own; the last shot (4) is both views'.
        assert_eq!(iw6_override_slots(3), &[WeaponSoundSlot::FirePlayerAkimbo]);
        assert_eq!(
            iw6_override_slots(4),
            &[WeaponSoundSlot::FireLast, WeaponSoundSlot::FireLastPlayer]
        );
        // The name guess agrees with the types on Ghosts' own suppressor
        // aliases, the akimbo `_aki` ones included.
        assert_eq!(fire_slot("weap_p226sup_fire_npc"), Some(WeaponSoundSlot::Fire));
        assert_eq!(fire_slot("weap_p226sup_fire_plr"), Some(WeaponSoundSlot::FirePlayer));
        assert_eq!(fire_slot("weap_p226sup_fire_aki"), Some(WeaponSoundSlot::FirePlayerAkimbo));
    }

    #[test]
    fn a_sniper_without_a_scope_of_its_own_gets_black_ops_ii_s() {
        let given = sniper_scope_or(None, GunClass::Sniper).expect("scope");
        assert!(given.sight);
        assert_eq!(given.sight_from.as_deref(), Some(SNIPER_SCOPE));
        assert!(given.view_models.is_empty());
        // Its own scope stays; other classes get none.
        let own = own_scope(Some("gun_scope".into()), None);
        assert_eq!(
            sniper_scope_or(Some(own.clone()), GunClass::Sniper),
            Some(own)
        );
        assert_eq!(sniper_scope_or(None, GunClass::Rifle), None);
    }

    #[test]
    fn a_thermal_attachment_shows_black_ops_ii_s_dual_band() {
        for name in ["opticsthermal", "thermalsmg", "thermal_scope"] {
            let k = iw_kind(name, GunClass::Sniper).expect(name);
            assert!(k.sight);
            assert_eq!(k.sight_from, Some("dsr50_mp+ir"));
        }
        let rifle = iw_kind("opticsthermal", GunClass::Rifle).expect("rifle");
        assert_eq!(rifle.sight_from, Some("an94_mp+ir"));
    }
}
