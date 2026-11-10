//! bo2mp: Ghosts' first-person quick melee on every gun.
//!
//! Every Ghosts gun's melee swipe (animation slot 9) is the same clip:
//! `viewmodel_knife_melee_slice` (a lunge, `viewmodel_knife_melee_stab`, has
//! the same arms and a `contact` note). The owner asked for that one clip to
//! be the first-person melee of every gun, Black Ops II's and every other
//! game's. Black Ops II's guns melee with the player's knife weapon, so that
//! weapon's melee slots take it; a guest gun melees with its own slots.
//!
//! What stays the gun's: the timings. The clip is re-timed to the gun's
//! `iMeleeTime` with the blow at its `iMeleeDelay` (the same numbers the
//! simulation uses for the hit), see [`retime`]. What is the clip's: the
//! poses, keyed by tag name: Ghosts' arms are named as Black Ops II's
//! (`j_shoulder_ri`, `j_wrist_le`, the finger chains), so the tracks bind
//! to Black Ops II's arm bones by name, and the gun hangs on `tag_weapon`
//! (a rig without that bone takes the gun's root from [`bone_for_track`]).
//!
//! Sounds: the clips' notetracks are kept (retimed with the clip) and the
//! guns' melee events play Ghosts' aliases (see [`MeleeSound`] and
//! [`NoteSound`]); `BO2MP_GHOSTS_MELEE_SOUNDS=0` keeps the animation with
//! Black Ops II's sounds. `BO2MP_GHOSTS_MELEE=0` restores each gun's own
//! melee animation (and sounds).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use asset_anim::XAnimCatalog;
use asset_anim::xanim_clip::{AnimClip, HitWarp};
use asset_core::{AssetEdge, XAnimSpace};
use weapon_iw4::WEAPON_ANIM_SLOTS;

use crate::{AssetNamespace, WeaponAnimSlot};

/// Ghosts' melee swipe (every gun's slot 9).
pub const QUICK_MELEE_CLIP: &str = "viewmodel_knife_melee_slice";
/// Ghosts' lunge stab (the melee-with-contact clip).
pub const LUNGE_MELEE_CLIP: &str = "viewmodel_knife_melee_stab";

/// The melee slots and which Ghosts clip each one plays: the plain melee
/// (full and empty magazine) the swipe, the charged one (Black Ops II's
/// lunge) the stab.
const SLOTS: [(WeaponAnimSlot, Swing); 4] = [
    (WeaponAnimSlot::Melee, Swing::Quick),
    (WeaponAnimSlot::MeleeEmpty, Swing::Quick),
    (WeaponAnimSlot::MeleeCharge, Swing::Lunge),
    (WeaponAnimSlot::MeleeChargeEmpty, Swing::Lunge),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swing {
    Quick,
    Lunge,
}

impl Swing {
    pub const fn clip_name(self) -> &'static str {
        match self {
            Self::Quick => QUICK_MELEE_CLIP,
            Self::Lunge => LUNGE_MELEE_CLIP,
        }
    }
}

/// Whether Ghosts' melee is played (default); `BO2MP_GHOSTS_MELEE=0` turns
/// it off.
pub fn enabled() -> bool {
    !std::env::var("BO2MP_GHOSTS_MELEE").is_ok_and(|v| v.trim() == "0")
}

/// Whether Ghosts' melee sounds are played with its animation (default);
/// `BO2MP_GHOSTS_MELEE_SOUNDS=0` keeps Black Ops II's. Off with the
/// animation (`BO2MP_GHOSTS_MELEE=0`).
pub fn sounds_enabled() -> bool {
    sounds_enabled_from(
        std::env::var("BO2MP_GHOSTS_MELEE").ok().as_deref(),
        std::env::var("BO2MP_GHOSTS_MELEE_SOUNDS").ok().as_deref(),
    )
}

fn sounds_enabled_from(melee: Option<&str>, sounds: Option<&str>) -> bool {
    let off = |v: Option<&str>| v.is_some_and(|v| v.trim() == "0");
    !off(melee) && !off(sounds)
}

/// The three melee events a gun's sounds come from: the swing when the
/// blow starts, a hit on a body, a blow that lands on a surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeleeSound {
    Swipe,
    Hit,
    Miss,
}

impl MeleeSound {
    pub const ALL: [Self; 3] = [Self::Swipe, Self::Hit, Self::Miss];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Swipe => "swing",
            Self::Hit => "hit",
            Self::Miss => "miss",
        }
    }
}

/// Ghosts' aliases for a melee event, best first. The knife's swing is the
/// one every Ghosts gun's melee names (`melee_*_swing_plr`: the two
/// `h2h_knife_swing` clips); the player's own view hears the `_plr`
/// alias, the others the `_npc` one. Ghosts' knife weapon names no hit or
/// miss sound of its own (the fields are empty), so a body takes the
/// knife's flesh alias and a surface the knife's generic impact (the
/// aliases the knife's surface hits chain to).
pub fn event_aliases(sound: MeleeSound, own_view: bool) -> &'static [&'static str] {
    match (sound, own_view) {
        (MeleeSound::Swipe, true) => &["melee_knife_swing_plr", "melee_knife_swing_npc"],
        (MeleeSound::Swipe, false) => &["melee_knife_swing_npc", "melee_knife_swing_plr"],
        (MeleeSound::Hit, true) => &[
            "melee_knife_hit_body_plr",
            "melee_knife_hit_body",
            "melee_knife_hit_flesh",
        ],
        (MeleeSound::Hit, false) => &["melee_knife_hit_body", "melee_knife_hit_flesh"],
        (MeleeSound::Miss, _) => &["melee_knife_hit_other", "melee_knife_hit_default"],
    }
}

/// The first of a melee event's aliases `has` knows.
pub fn pick_event_alias(
    sound: MeleeSound,
    own_view: bool,
    has: impl Fn(&str) -> bool,
) -> Option<&'static str> {
    event_aliases(sound, own_view).iter().copied().find(|a| has(a))
}

/// Every note of the two Ghosts melee clips (`viewmodel_knife_melee_slice`
/// and `_stab`), as read from `common_core_mp`.
pub const MELEE_NOTES: [&str; 10] = [
    "ps_melee_knife_swing_plr",
    "melee_start_upper",
    "melee_knife_stab_upper_plr",
    "blade_hide",
    "contact",
    "melee_contact_upper",
    "melee_upper_push",
    "melee_upper_pullout",
    "blade_show",
    "start_ragdoll",
];

/// What a Ghosts melee note plays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoteSound {
    /// A note with no sound (the blade's visibility, the contact frame, the
    /// ragdoll).
    Silent,
    /// A sound the melee events play at the sim's own moment (the swing at
    /// the start, the blow's impact at the hit), so the note adds none.
    Covered,
    /// The alias it plays.
    Alias(String),
    /// A sound note whose alias the bank lacks.
    Unresolved,
}

/// Whether a note belongs to the Ghosts melee clips.
pub fn is_melee_note(note: &str) -> bool {
    MELEE_NOTES.iter().any(|n| n.eq_ignore_ascii_case(note))
}

/// What a note of the Ghosts melee clips plays: Ghosts' notes name their
/// alias (`ps_` marks a play-sound note), the first-person one carrying the
/// `_plr` suffix (`melee_start_upper` -> `melee_start_upper_plr`). `has`
/// tells whether Ghosts' sounds hold an alias.
pub fn note_sound(note: &str, has: impl Fn(&str) -> bool) -> NoteSound {
    let note = note.to_ascii_lowercase();
    match note.as_str() {
        "blade_hide" | "blade_show" | "contact" | "start_ragdoll" | "end" => {
            return NoteSound::Silent;
        }
        // The swipe's own swing and the stab's contact (the hit event).
        "ps_melee_knife_swing_plr" | "melee_contact_upper" => return NoteSound::Covered,
        _ => {}
    }
    let name = note.strip_prefix("ps_").unwrap_or(&note);
    let plr = format!("{name}_plr");
    [name, plr.as_str()]
        .into_iter()
        .find(|a| has(a))
        .map_or(NoteSound::Unresolved, |a| NoteSound::Alias(a.to_owned()))
}

/// The `bo2mp ghosts melee sounds:` log line: what each melee event and
/// each note of the clips plays, from Ghosts' aliases (or what is missing
/// and so keeps Black Ops II's).
pub fn sounds_report(has: impl Fn(&str) -> bool + Copy, weapons: usize) -> String {
    let mut parts = Vec::new();
    for sound in MeleeSound::ALL {
        let own = pick_event_alias(sound, true, has);
        let world = pick_event_alias(sound, false, has);
        parts.push(format!(
            "{} own={} others={}",
            sound.label(),
            own.unwrap_or("MISSING (Black Ops II's)"),
            world.unwrap_or("MISSING (Black Ops II's)")
        ));
    }
    let notes: Vec<String> = MELEE_NOTES
        .iter()
        .map(|note| {
            let what = match note_sound(note, has) {
                NoteSound::Silent => "silent".to_owned(),
                NoteSound::Covered => "played by the melee event".to_owned(),
                NoteSound::Alias(a) => a,
                NoteSound::Unresolved => "MISSING".to_owned(),
            };
            format!("{note} -> {what}")
        })
        .collect();
    format!(
        "bo2mp ghosts melee sounds: {}; notes: {}; on {weapons} weapons",
        parts.join(", "),
        notes.join(", ")
    )
}

/// Whether a clip is one of the two Ghosts melee clips.
pub fn is_ghosts_melee_clip(name: &str) -> bool {
    name.eq_ignore_ascii_case(QUICK_MELEE_CLIP) || name.eq_ignore_ascii_case(LUNGE_MELEE_CLIP)
}

/// Games whose view animations pose their own arm skeleton, retargeted onto
/// Black Ops II's by bind-pose differences (`fpv_retarget`): a clip keyed in
/// Black Ops II's arm frames would twist them, so they keep their own melee.
const fn keeps_own_melee(ns: AssetNamespace) -> bool {
    matches!(ns, AssetNamespace::T7 | AssetNamespace::Iw7)
}

/// A shield bash is not a swing of a hand-held blade: it keeps its clip.
fn is_shield_bash(clip_name: &str) -> bool {
    let n = clip_name.to_ascii_lowercase();
    n.contains("shield")
}

/// The edges of one weapon with its melee slots (the ones bound, but not a
/// shield's) pointed at Ghosts' clips; how many were changed.
pub fn rewrite_edges(
    edges: &mut [AssetEdge<XAnimSpace>; WEAPON_ANIM_SLOTS],
    xanims: &XAnimCatalog,
    quick: &AssetEdge<XAnimSpace>,
    lunge: &AssetEdge<XAnimSpace>,
) -> usize {
    let mut changed = 0;
    for (slot, swing) in SLOTS {
        let edge = &mut edges[slot.index()];
        let Some(current) = edge.bound_index() else {
            continue;
        };
        if xanims.name_at(current).is_some_and(is_shield_bash) {
            continue;
        }
        let replacement = match swing {
            Swing::Quick => quick,
            Swing::Lunge => lunge,
        };
        if replacement.is_bound() {
            *edge = *replacement;
            changed += 1;
        }
    }
    changed
}

/// Ghosts' two melee clips' edges in the catalog: (swipe, stab). A catalog
/// without Ghosts' animations (no Ghosts install) gives unbound edges and
/// every gun keeps its own.
pub fn catalog_edges(xanims: &XAnimCatalog) -> (AssetEdge<XAnimSpace>, AssetEdge<XAnimSpace>) {
    let find = |name: &str| match xanims.index_by_name(AssetNamespace::Iw6, name) {
        Some(index) => AssetEdge::bind_order(index, xanims.zone_of(index)),
        None => AssetEdge::Absent,
    };
    (find(QUICK_MELEE_CLIP), find(LUNGE_MELEE_CLIP))
}

/// Whether a weapon of this game takes Ghosts' melee.
pub fn applies_to(ns: AssetNamespace) -> bool {
    !keeps_own_melee(ns)
}

/// The slots Ghosts' clips go on, with the gun's (time, hit delay) for each.
pub fn slot_timings(
    melee_time_ms: i32,
    melee_delay_ms: i32,
    charge_time_ms: i32,
    charge_delay_ms: i32,
) -> [(WeaponAnimSlot, i32, i32); 4] {
    [
        (WeaponAnimSlot::Melee, melee_time_ms, melee_delay_ms),
        (WeaponAnimSlot::MeleeEmpty, melee_time_ms, melee_delay_ms),
        (WeaponAnimSlot::MeleeCharge, charge_time_ms, charge_delay_ms),
        (
            WeaponAnimSlot::MeleeChargeEmpty,
            charge_time_ms,
            charge_delay_ms,
        ),
    ]
}

/// The clip re-timed to a gun's melee: `time_ms` long (`iMeleeTime`), the
/// blow of the clip (its `contact` note or fastest swing) at `delay_ms`
/// (`iMeleeDelay`; none: the clip's own proportions). Same tracks as the
/// source. `None` for a gun with no melee time (it plays at the clip's own
/// speed). Cached per (clip, time, delay).
pub fn retime(clip: &Arc<AnimClip>, time_ms: i32, delay_ms: i32) -> Option<Arc<AnimClip>> {
    if time_ms <= 0 || clip.duration() <= 0.0 {
        return None;
    }
    static CACHE: Mutex<Option<HashMap<(String, i32, i32), Arc<AnimClip>>>> = Mutex::new(None);
    let key = (clip.name.to_ascii_lowercase(), time_ms, delay_ms);
    if let Ok(cache) = CACHE.lock()
        && let Some(hit) = cache.as_ref().and_then(|c| c.get(&key))
    {
        return Some(Arc::clone(hit));
    }
    let warp = HitWarp::new(
        clip.duration(),
        clip.hit_time(),
        time_ms as f32 / 1000.0,
        delay_ms as f32 / 1000.0,
    );
    let out = Arc::new(clip.retimed(&warp));
    if let Ok(mut cache) = CACHE.lock() {
        cache
            .get_or_insert_with(HashMap::new)
            .insert(key, Arc::clone(&out));
    }
    Some(out)
}

/// Which bone of a rig a Ghosts melee track drives: the bone of the same
/// name; the gun's root track (`tag_weapon`) on a rig that has no such bone
/// takes the right hand's weapon tag, then the gun's own root, so the gun
/// moves rigidly with it and the arms follow their own bones. `find` gives a
/// rig's bone by name.
pub fn bone_for_track<T>(track: &str, find: impl Fn(&str) -> Option<T>) -> Option<T> {
    if let Some(bone) = find(track) {
        return Some(bone);
    }
    if track.eq_ignore_ascii_case("tag_weapon") {
        return ["tag_weapon_right", "j_gun"]
            .iter()
            .find_map(|alias| find(alias));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rig(names: &[&str]) -> impl Fn(&str) -> Option<u16> {
        let names: Vec<String> = names.iter().map(|s| (*s).to_owned()).collect();
        move |n: &str| names.iter().position(|b| b == n).map(|i| i as u16)
    }

    #[test]
    fn arm_tracks_bind_by_name_and_the_gun_root_falls_back() {
        let bo2 = rig(&[
            "tag_torso",
            "tag_weapon",
            "j_shoulder_ri",
            "j_wrist_le",
            "j_index_le_0",
        ]);
        for (track, want) in [
            ("tag_torso", Some(0)),
            ("tag_weapon", Some(1)),
            ("j_shoulder_ri", Some(2)),
            ("j_wrist_le", Some(3)),
            ("j_index_le_0", Some(4)),
            ("j_sleeveanim_back_01_le", None),
            ("tag_knife_attach", None),
        ] {
            assert_eq!(bone_for_track(track, &bo2), want, "{track}");
        }
        // A rig with no tag_weapon: the gun's root follows the hand's tag.
        let hand = rig(&["tag_torso", "tag_weapon_right", "j_gun"]);
        assert_eq!(bone_for_track("tag_weapon", &hand), Some(1));
        let root = rig(&["tag_torso", "j_gun"]);
        assert_eq!(bone_for_track("tag_weapon", &root), Some(1));
        // The hand tag itself is never borrowed for another track.
        assert_eq!(bone_for_track("tag_weapon_left", &root), None);
        assert_eq!(bone_for_track("tag_weapon", &rig(&["tag_torso"])), None);
    }

    fn has_all(a: &str) -> bool {
        [
            "melee_knife_swing_plr",
            "melee_knife_swing_npc",
            "melee_knife_hit_body",
            "melee_knife_hit_other",
            "melee_start_upper_plr",
            "melee_knife_stab_upper_plr",
            "melee_upper_push",
            "melee_upper_pullout",
        ]
        .contains(&a)
    }

    #[test]
    fn sound_switches() {
        assert!(sounds_enabled_from(None, None));
        assert!(!sounds_enabled_from(None, Some("0")));
        assert!(!sounds_enabled_from(Some("0"), None));
        assert!(sounds_enabled_from(Some("1"), Some("1")));
    }

    #[test]
    fn events_take_the_first_alias_ghosts_has() {
        // The own view's hit alias is missing here: the shared body one.
        assert_eq!(pick_event_alias(MeleeSound::Swipe, true, has_all), Some("melee_knife_swing_plr"));
        assert_eq!(pick_event_alias(MeleeSound::Swipe, false, has_all), Some("melee_knife_swing_npc"));
        assert_eq!(pick_event_alias(MeleeSound::Hit, true, has_all), Some("melee_knife_hit_body"));
        assert_eq!(pick_event_alias(MeleeSound::Miss, true, has_all), Some("melee_knife_hit_other"));
        // No Ghosts: nothing, so the caller keeps Black Ops II's.
        assert_eq!(pick_event_alias(MeleeSound::Swipe, true, |_| false), None);
    }

    #[test]
    fn notes_name_their_alias() {
        assert_eq!(note_sound("ps_melee_knife_swing_plr", has_all), NoteSound::Covered);
        assert_eq!(note_sound("melee_contact_upper", has_all), NoteSound::Covered);
        assert_eq!(note_sound("blade_hide", has_all), NoteSound::Silent);
        assert_eq!(note_sound("Contact", has_all), NoteSound::Silent);
        assert_eq!(
            note_sound("melee_start_upper", has_all),
            NoteSound::Alias("melee_start_upper_plr".into())
        );
        assert_eq!(
            note_sound("melee_knife_stab_upper_plr", has_all),
            NoteSound::Alias("melee_knife_stab_upper_plr".into())
        );
        assert_eq!(note_sound("melee_upper_push", has_all), NoteSound::Alias("melee_upper_push".into()));
        assert_eq!(note_sound("melee_upper_pullout", |_| false), NoteSound::Unresolved);
        assert!(MELEE_NOTES.iter().all(|n| is_melee_note(n)));
        assert!(!is_melee_note("weap_m4_clipin_plr"));
    }

    #[test]
    fn the_report_lists_what_is_found_and_missing() {
        let line = sounds_report(has_all, 120);
        assert!(
            line.starts_with("bo2mp ghosts melee sounds: swing own=melee_knife_swing_plr others=melee_knife_swing_npc"),
            "{line}"
        );
        assert!(line.contains("melee_start_upper -> melee_start_upper_plr"), "{line}");
        assert!(line.contains("on 120 weapons"), "{line}");
        assert!(sounds_report(|_| false, 0).contains("MISSING (Black Ops II's)"));
    }

    #[test]
    fn slots_and_clips() {
        assert_eq!(Swing::Quick.clip_name(), "viewmodel_knife_melee_slice");
        assert!(is_ghosts_melee_clip("ViewModel_Knife_Melee_Stab"));
        assert!(!is_ghosts_melee_clip("viewmodel_knife_melee_stab_miss"));
        assert!(is_shield_bash("viewmodel_riotshield_melee"));
        assert!(!is_shield_bash("viewmodel_knife_melee_slice"));
        assert!(
            applies_to(AssetNamespace::T6)
                && applies_to(AssetNamespace::Iw6)
                && applies_to(AssetNamespace::H1)
        );
        assert!(!applies_to(AssetNamespace::T7));
        let t = slot_timings(750, 150, 900, 300);
        assert_eq!(t[0], (WeaponAnimSlot::Melee, 750, 150));
        assert_eq!(t[3], (WeaponAnimSlot::MeleeChargeEmpty, 900, 300));
    }
}
