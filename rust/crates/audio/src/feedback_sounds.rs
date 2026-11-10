//! bo2mp: the hit marker and kill sounds, found in whichever game's aliases
//! the loaded bank carries.
//!
//! Black Ops II's `_damagefeedback.gsc` plays `mpl_hit_alert` (a `t6:` alias
//! through `playlocalsound`). A bank composed for a map from another game
//! (IW, BO1, BO3, MWR) may lack the T6 alias; played as named, that sound
//! was dropped without a word. Each group below lists the same sound as
//! every game named it, best first.
//!
//! Names CONFIRMED from his own zones (2026-10-26, `t6_alias_list` /
//! `t5_alias_list` on his `bo2_game_files` and `bo1_game_files`):
//! - BO2 `zone/all/common_mp.ff`: `mpl_hit_alert`, `mpl_hit_alert_low` (the
//!   same clip, `alert_00`, id 58a1b392 in `sound/cmn_root.all.sabl`),
//!   `mpl_killconfirm_tags_pickup`, `mpl_killconfirm_tags_drop`;
//!   `patch_mp.ff`: `mpl_kc_killdeny`.
//! - BO1 `zone/Common/common_mp.ff`: `mpl_hit_alert` only (no low one, no
//!   Kill Confirmed).
//! - No score-popup / "+100" tick sound in either game: none of BO2's
//!   `common_mp`, `patch_mp`, `code_post_gfx_mp`, `ui_mp`, `patch_ui_mp` or
//!   BO1's `common_mp`, `code_post_gfx_mp` has one (the names tried before,
//!   `mpl_score_tick`, `uin_score_tick`, ..., were guesses that exist
//!   nowhere). The score popup is silent, as in those games.
//! - Not in his common zones (kept, unconfirmed): BO2's headshot crack
//!   `prj_bullet_impact_headshot(_2d)`, MW2/MW3's `MP_hit_alert`,
//!   `bullet_impact_headshot_2`, MW3's / Ghosts' `mp_killconfirm_tags_*`.

use asset_audio::SoundCatalog;
use asset_core::AssetNamespace::{self, Iw4, Iw5, Iw6, T5, T6};

/// The HUD's name for the score popup's sound (`ui::lui_hud` writes it as a
/// literal: the UI crate asks the audio crate by this name).
pub(crate) const SCORE_POPUP: &str = "bo2mp_score_popup";

pub(crate) type Candidate = (AssetNamespace, &'static str);

pub(crate) const HIT_GROUP: &[Candidate] = &[
    (T6, "mpl_hit_alert"),
    (T5, "mpl_hit_alert"),
    (Iw4, "MP_hit_alert"),
    (Iw5, "MP_hit_alert"),
    (T6, "mpl_hit_alert_low"),
];
pub(crate) const HIT_LOW_GROUP: &[Candidate] = &[
    (T6, "mpl_hit_alert_low"),
    (T6, "mpl_hit_alert"),
    (T5, "mpl_hit_alert"),
    (Iw4, "MP_hit_alert"),
    (Iw5, "MP_hit_alert"),
];
/// The headshot hit: the engine's crack, else a plain hit alert.
pub(crate) const HEADSHOT_GROUP: &[Candidate] = &[
    (T6, "prj_bullet_impact_headshot_2d"),
    (T6, "prj_bullet_impact_headshot"),
    (T5, "prj_bullet_impact_headshot"),
    (Iw4, "bullet_impact_headshot_2"),
    (Iw5, "bullet_impact_headshot_2"),
    (T6, "mpl_hit_alert"),
    (T5, "mpl_hit_alert"),
    (Iw4, "MP_hit_alert"),
];
/// Kill Confirmed: picking up a tag (Black Ops had no Kill Confirmed:
/// Modern Warfare 3's and Ghosts' versions stand in, then a hit alert).
pub(crate) const CONFIRM_GROUP: &[Candidate] = &[
    (T6, "mpl_killconfirm_tags_pickup"),
    (Iw5, "mp_killconfirm_tags_pickup"),
    (Iw6, "mp_killconfirm_tags_pickup"),
    (T6, "mpl_hit_alert"),
    (T5, "mpl_hit_alert"),
    (Iw4, "MP_hit_alert"),
];
/// Kill Confirmed: denying a tag (BO2's `patch_mp` `mpl_kc_killdeny`).
pub(crate) const DENY_GROUP: &[Candidate] = &[
    (T6, "mpl_kc_killdeny"),
    (Iw5, "mp_killconfirm_tags_deny"),
    (Iw6, "mp_killconfirm_tags_deny"),
    (T6, "mpl_hit_alert_low"),
    (T6, "mpl_hit_alert"),
    (T5, "mpl_hit_alert"),
    (Iw4, "MP_hit_alert"),
];
/// The score popup's sound: none (see the module notes); kept as a group so
/// a name found later only needs listing here.
pub(crate) const SCORE_GROUP: &[Candidate] = &[];

/// Every group, the order `group_of` searches them in (the hit alert's own
/// group first: a bare `mpl_hit_alert` is a hit, not a stand-in).
pub(crate) const GROUPS: &[&[Candidate]] =
    &[HIT_GROUP, HIT_LOW_GROUP, HEADSHOT_GROUP, CONFIRM_GROUP, DENY_GROUP, SCORE_GROUP];

/// The group an alias belongs to (compared without case), by the alias it
/// names first in a group.
fn group_of(ns: AssetNamespace, alias: &str) -> Option<&'static [Candidate]> {
    GROUPS
        .iter()
        .copied()
        .find(|g| g.iter().take(1).any(|(n, a)| *n == ns && a.eq_ignore_ascii_case(alias)))
        .or_else(|| {
            GROUPS
                .iter()
                .copied()
                .filter(|g| !std::ptr::eq(*g, SCORE_GROUP))
                .find(|g| g.iter().any(|(n, a)| *n == ns && a.eq_ignore_ascii_case(alias)))
        })
}

/// Whether an alias is one of the hit / kill sounds (any group, any game's
/// name for it): its starts are logged one by one.
pub(crate) fn is_feedback(ns: AssetNamespace, alias: &str) -> bool {
    group_of(ns, alias).is_some()
}

/// How much a Black Ops II hit / kill sound is lifted (about +15.6 dB).
/// His `common_mp` stores `mpl_hit_alert` at 9257/65535 (-17 dB) and
/// `mpl_hit_alert_low` at 3685/65535 (-25 dB): Black Ops II mixes them by
/// volume group and ducks the rest under them, which this engine does not,
/// so at those levels they vanished under the guns (above all Black Ops'
/// guns on its maps, played at Black Ops' own full levels).
pub(crate) const T6_FEEDBACK_LIFT: f32 = 6.0;

/// A hit / kill sound's volume as played: a Black Ops II alias lifted by
/// [`T6_FEEDBACK_LIFT`] (never past full), other games' as they are.
pub(crate) fn level(ns: AssetNamespace, volume: f32) -> f32 {
    if ns == T6 { (volume * T6_FEEDBACK_LIFT).min(1.0) } else { volume }
}

/// The first candidate of `group` that `has` accepts.
pub(crate) fn first_present(
    group: &[Candidate],
    has: impl Fn(AssetNamespace, &str) -> bool,
) -> Option<Candidate> {
    group.iter().copied().find(|(ns, alias)| has(*ns, alias))
}

/// What a hit/score alias plays as in this bank: itself when the bank has it,
/// else the same sound from another game. `None` when it is not a feedback
/// alias, or no game's version is in the bank.
pub(crate) fn resolve(bank: &SoundCatalog, ns: AssetNamespace, alias: &str) -> Option<Resolved> {
    let group = group_of(ns, alias)?;
    if bank.index_in(ns, alias).is_some() {
        return Some(Resolved::Same);
    }
    Some(match first_present(group, |n, a| bank.index_in(n, a).is_some()) {
        Some((n, a)) => Resolved::Fallback(n, a),
        None => Resolved::Missing,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Resolved {
    Same,
    Fallback(AssetNamespace, &'static str),
    Missing,
}

/// The score popup's sound: the first candidate in the bank.
pub(crate) fn score_popup(bank: &SoundCatalog) -> Option<Candidate> {
    first_present(SCORE_GROUP, |n, a| bank.index_in(n, a).is_some())
}

/// bo2mp: the kill banner and score popups are the modern HUD's (`ui`'s
/// `hud_modern::modern_hud_enabled`: Black Ops' HUD and the LUI HUD on, read
/// from the same switches), off in Resurgence. Only for the log line.
pub(crate) fn banner_on() -> bool {
    let bo1_hud = !std::env::var("BO2_HUD").is_ok_and(|v| v.trim().eq_ignore_ascii_case("bo2"));
    let lui = std::env::var("BO2ZM_LUI").map_or(true, |v| v != "0");
    bo1_hud && lui && !sim::resurgence_on()
}

fn picked(group: &[Candidate], has: &impl Fn(AssetNamespace, &str) -> bool) -> String {
    first_present(group, has).map_or_else(|| "none".to_owned(), |(n, a)| format!("{}:{a}", n.as_str()))
}

/// bo2mp: the once-per-map line saying what each feedback sound plays as:
/// `feedback: <map> namespace=<map game> sound=<bank game> hit=.. ..`.
pub(crate) fn summary_line(
    map: &str,
    map_game: Option<AssetNamespace>,
    bank_ns: AssetNamespace,
    banner: bool,
    has: impl Fn(AssetNamespace, &str) -> bool,
) -> String {
    format!(
        "feedback: {map} namespace={} sound={} hit={} hit_low={} headshot={} kill={} confirm={} deny={} banner={} (kill: the score popup, silent in BO2/BO1)",
        map_game.map_or("?", |g| g.as_str()),
        bank_ns.as_str(),
        picked(HIT_GROUP, &has),
        picked(HIT_LOW_GROUP, &has),
        picked(HEADSHOT_GROUP, &has),
        picked(SCORE_GROUP, &has),
        picked(CONFIRM_GROUP, &has),
        picked(DENY_GROUP, &has),
        if banner { "on" } else { "off" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_t6_hit_alert_falls_back_to_mw2s_when_the_bank_has_only_that() {
        let got = first_present(HIT_GROUP, |n, a| n == Iw4 && a == "MP_hit_alert");
        assert_eq!(got, Some((Iw4, "MP_hit_alert")));
    }

    #[test]
    fn the_own_game_alias_wins_when_present() {
        let got = first_present(HIT_GROUP, |_, _| true);
        assert_eq!(got, Some((T6, "mpl_hit_alert")));
    }

    #[test]
    fn nothing_in_the_bank_is_none() {
        assert_eq!(first_present(SCORE_GROUP, |_, _| false), None);
    }

    /// A Black Ops map's bank as his BO1 `common_mp` has it: one hit alert.
    fn bo1_bank(n: AssetNamespace, a: &str) -> bool {
        n == T5 && a == "mpl_hit_alert"
    }

    /// His BO2 `common_mp` + `patch_mp` names.
    fn bo2_bank(n: AssetNamespace, a: &str) -> bool {
        n == T6
            && matches!(
                a,
                "mpl_hit_alert" | "mpl_hit_alert_low" | "mpl_killconfirm_tags_pickup" | "mpl_killconfirm_tags_drop" | "mpl_kc_killdeny"
            )
    }

    #[test]
    fn a_bo1_map_plays_black_ops_hit_alerts() {
        assert_eq!(first_present(HIT_GROUP, bo1_bank), Some((T5, "mpl_hit_alert")));
        assert_eq!(first_present(HIT_LOW_GROUP, bo1_bank), Some((T5, "mpl_hit_alert")));
        assert_eq!(first_present(HEADSHOT_GROUP, bo1_bank), Some((T5, "mpl_hit_alert")));
        // (No Kill Confirmed in Black Ops: its hit alert is heard.)
        assert_eq!(first_present(CONFIRM_GROUP, bo1_bank), Some((T5, "mpl_hit_alert")));
        assert_eq!(first_present(DENY_GROUP, bo1_bank), Some((T5, "mpl_hit_alert")));
    }

    #[test]
    fn bo2s_own_names_win_with_bo2_installed() {
        assert_eq!(first_present(HIT_GROUP, bo2_bank), Some((T6, "mpl_hit_alert")));
        assert_eq!(first_present(HIT_LOW_GROUP, bo2_bank), Some((T6, "mpl_hit_alert_low")));
        assert_eq!(first_present(CONFIRM_GROUP, bo2_bank), Some((T6, "mpl_killconfirm_tags_pickup")));
        assert_eq!(first_present(DENY_GROUP, bo2_bank), Some((T6, "mpl_kc_killdeny")));
        assert_eq!(first_present(SCORE_GROUP, bo2_bank), None);
    }

    #[test]
    fn script_aliases_find_their_group_on_any_map() {
        // (By contents: a `const` slice's address differs between uses.)
        assert_eq!(group_of(T6, "mpl_hit_alert").unwrap(), HIT_GROUP);
        assert_eq!(group_of(T5, "mpl_hit_alert").unwrap(), HIT_GROUP);
        assert_eq!(group_of(T6, "mpl_killconfirm_tags_pickup").unwrap(), CONFIRM_GROUP);
        assert_eq!(group_of(T6, "mpl_kc_killdeny").unwrap(), DENY_GROUP);
        assert_eq!(group_of(T6, "prj_bullet_impact_headshot_2d").unwrap(), HEADSHOT_GROUP);
    }

    #[test]
    fn the_map_line_names_each_pick_or_none() {
        let line = summary_line("mp_nuked", Some(T5), T5, true, bo1_bank);
        assert!(line.starts_with(
            "feedback: mp_nuked namespace=t5 sound=t5 hit=t5:mpl_hit_alert hit_low=t5:mpl_hit_alert \
             headshot=t5:mpl_hit_alert kill=none confirm=t5:mpl_hit_alert deny=t5:mpl_hit_alert banner=on"
        ), "{line}");
        assert!(summary_line("mp_x", None, T6, false, |_, _| false).contains("deny=none banner=off"));
    }

    #[test]
    fn bo2_hit_sounds_are_lifted_over_the_guns() {
        // His common_mp's stored levels (9257 and 3685 of 65535).
        let (hit, low) = (9257.0 / 65535.0, 3685.0 / 65535.0);
        assert!(level(T6, hit) > 0.8 && level(T6, hit) <= 1.0, "{}", level(T6, hit));
        assert!(level(T6, low) < level(T6, hit));
        assert!(level(T6, low) > 0.3);
        assert_eq!(level(T6, 0.9), 1.0);
        // Black Ops' and Modern Warfare's own levels stay theirs.
        assert_eq!(level(T5, 0.5), 0.5);
        assert_eq!(level(Iw4, 0.7), 0.7);
    }

    #[test]
    fn every_hit_and_kill_sound_is_logged_and_nothing_else() {
        for (n, a) in [(T6, "mpl_hit_alert"), (T6, "mpl_hit_alert_low"), (T5, "mpl_hit_alert"), (T6, "mpl_killconfirm_tags_pickup"), (T6, "mpl_kc_killdeny"), (Iw4, "MP_hit_alert")] {
            assert!(is_feedback(n, a), "{a}");
        }
        assert!(!is_feedback(T6, "wpn_ak47_fire_plr"));
        assert!(!is_feedback(T5, "mpl_killconfirm_tags_pickup"));
    }

    #[test]
    fn aliases_find_their_group_without_case() {
        assert!(group_of(T6, "MPL_HIT_ALERT").is_some());
        assert!(group_of(Iw4, "mp_hit_alert").is_some());
        assert!(group_of(T6, "mpl_something_else").is_none());
    }
}
