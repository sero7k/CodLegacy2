use std::collections::HashMap;

use asset_core::AssetNamespace;
use bevy::prelude::Resource;

pub(crate) fn voice_prefixes_for_zone(
    catalog: Option<&asset_game::MenuCatalog>,
    bank: Option<&crate::SoundBank>,
    identity: &frame::LaunchIdentity,
) -> (Option<String>, Option<String>) {
    let Some(catalog) = catalog else {
        return (None, None);
    };
    let Some(table) = catalog.string_table(gamemode_iw4::FACTION_TABLE) else {
        return (None, None);
    };
    let (allies_cs, axis_cs) = arena_faction_charsets(catalog, bank, identity);
    let allies = table.lookup_col(&allies_cs, gamemode_iw4::FACTION_VOICE_PREFIX_COL);
    let axis = table.lookup_col(&axis_cs, gamemode_iw4::FACTION_VOICE_PREFIX_COL);
    (
        (!allies.is_empty()).then(|| allies.to_owned()),
        (!axis.is_empty()).then(|| axis.to_owned()),
    )
}

fn arena_faction_charsets(
    catalog: &asset_game::MenuCatalog,
    bank: Option<&crate::SoundBank>,
    identity: &frame::LaunchIdentity,
) -> (String, String) {
    let from_ui = catalog.rawfile_text("mp/basemaps.arena").map(str::to_owned);
    let from_bank = bank.and_then(|b| b.0.rawfile_text("mp/basemaps.arena").map(str::to_owned));
    let arena_text = from_ui
        .or(from_bank)
        .or_else(|| asset_game::read_basemaps_arena(&identity.games_root));
    let row = arena_text
        .as_deref()
        .and_then(|text| asset_game::arena_charsets(text, &identity.zone));
    let (allies, axis) = row.map_or((None, None), |row| (row.allieschar, row.axischar));
    (
        allies
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| gamemode_iw4::DEFAULT_ALLIES_CHARSET.to_owned()),
        axis.filter(|s| !s.is_empty())
            .unwrap_or_else(|| gamemode_iw4::DEFAULT_AXIS_CHARSET.to_owned()),
    )
}

pub(crate) fn team_voice_aliases(
    bank: &asset_audio::SoundCatalog,
    allies: Option<&str>,
    axis: Option<&str>,
) -> Vec<String> {
    let prefixes: Vec<&str> = [allies, axis]
        .into_iter()
        .flatten()
        .chain(["generic_death_"])
        .collect();
    let mut aliases: Vec<String> = (0..bank.sounds.len())
        .filter(|&index| bank.namespace_of_alias(index) == AssetNamespace::Iw4)
        .filter_map(|index| bank.name_at(index))
        .filter(|name| prefixes.iter().any(|prefix| name.starts_with(prefix)))
        .map(str::to_owned)
        .collect();
    for prefix in [allies, axis].into_iter().flatten() {
        for line in TEAM_MUSIC {
            let alias = format!("{prefix}{line}");
            if !aliases.contains(&alias) {
                aliases.push(alias);
            }
        }
    }
    aliases
}

/// bo2mp: every `game["voice"][team]` prefix Black Ops II's faction scripts
/// set (`MP/Factions/faction_*_mp/maps/mp/teams/_teamset_*.gsc:41`, and
/// `_teamset_multiteam.gsc`): seals, pmc, cd, fbi, isa, pla.
const T6_ANNOUNCER_PREFIXES: [&str; 6] = [
    "vox_st6_", "vox_pmc_", "vox_cda_", "vox_fbi_", "vox_isa_", "vox_pla_",
];

/// bo2mp: the announcer lines of the match's factions. BO2 plays
/// `game["voice"][team] + game["dialog"][line]` with `playlocalsound`
/// (`_globallogic_audio.gsc:523-527`), e.g. `vox_st6_timesup`. The map's
/// zones load only this match's two factions' language zones, so every
/// T6 alias with one of the six prefixes is one of theirs. They are not in
/// the weapon or map sets, so without this they were first decoded when the
/// script played them and missed the start deadline (a late line is dropped).
pub(crate) fn t6_announcer_aliases(bank: &asset_audio::SoundCatalog) -> Vec<String> {
    (0..bank.sounds.len())
        .filter(|&index| bank.namespace_of_alias(index) == AssetNamespace::T6)
        .filter_map(|index| bank.name_at(index))
        .filter(|name| {
            let lower = name.to_ascii_lowercase();
            T6_ANNOUNCER_PREFIXES
                .iter()
                .any(|prefix| lower.starts_with(prefix))
        })
        .map(str::to_owned)
        .collect()
}

/// bo2mp: the start of every Black Ops II fight sound: bullet, bolt and blade
/// impacts, and the bots' footsteps and landings. Decoded on first play, the
/// first seconds of a 12-bot firefight queued dozens of clips at once and
/// left their first impacts and steps silent.
const T6_FIGHT_PREFIXES: &[&str] = &[
    "prj_bullet_impact_",
    "prj_bulletspray_",
    "prj_bolt_impact",
    "prj_blade_impact",
    "prj_bullet_debris",
    "fly_step_run_",
    "fly_step_sprint_",
    "fly_step_walk_",
    "fly_land_",
];

/// bo2mp: the Black Ops II fight sounds the bank has: every alias starting
/// with a `T6_FIGHT_PREFIXES` entry, and the players' pain, kill, threat and
/// attack voice lines (`vox_*_pain_*` and the like).
pub(crate) fn t6_fight_aliases(bank: &asset_audio::SoundCatalog) -> Vec<String> {
    (0..bank.sounds.len())
        .filter(|&index| bank.namespace_of_alias(index) == AssetNamespace::T6)
        .filter_map(|index| bank.name_at(index))
        .filter(|name| is_t6_fight_sound(&name.to_ascii_lowercase()))
        .map(str::to_owned)
        .collect()
}

fn is_t6_fight_sound(lower: &str) -> bool {
    T6_FIGHT_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
        || (lower.starts_with("vox_")
            && ["_pain_", "_kill_", "_threat_", "_attack_"]
                .iter()
                .any(|part| lower.contains(part)))
}

#[cfg(test)]
mod t6_fight_tests {
    use super::is_t6_fight_sound;

    #[test]
    fn impacts_steps_and_pain_are_fight_sounds() {
        for alias in [
            "prj_bullet_impact_flesh",
            "prj_bulletspray_large",
            "fly_step_run_concrete",
            "fly_land_dirt",
            "vox_plr_0_pain_small",
            "vox_ru_1_kill_confirm",
        ] {
            assert!(is_t6_fight_sound(alias), "{alias}");
        }
        for alias in ["mus_mp_frontend", "vox_mp_announcer_win", "wpn_ar_fire"] {
            assert!(!is_t6_fight_sound(alias), "{alias}");
        }
    }
}

/// Which announcer a Black Ops II match speaks with. Modern Warfare 3's
/// (2011) is the default; `BO2_ANNOUNCER=bo1` speaks with Black Ops',
/// `BO2_ANNOUNCER=bo2` keeps Black Ops II's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum T6Announcer {
    Mw3,
    Bo1,
    Bo2,
}

impl T6Announcer {
    pub(crate) fn from_env() -> Self {
        match std::env::var("BO2_ANNOUNCER") {
            Ok(value) if value.eq_ignore_ascii_case("bo2") => Self::Bo2,
            Ok(value) if value.eq_ignore_ascii_case("bo1") => Self::Bo1,
            _ => Self::Mw3,
        }
    }
}

/// bo2mp: BO2 faction voice -> the Modern Warfare 3 faction that speaks for
/// it (MW3's announcer aliases are `<faction>_1mc_<line>`): the Navy SEALs
/// and the ISA as Delta (US), the FBI as the SAS (UK); the mercenaries as
/// MW3's PMC, Cordis Die as the African militia, the PLA as Spetsnaz.
const T6_MW3_VOICES: [(&str, &str); 6] = [
    ("vox_st6_", "US_1mc_"),
    ("vox_fbi_", "UK_1mc_"),
    ("vox_isa_", "US_1mc_"),
    ("vox_pmc_", "PC_1mc_"),
    ("vox_cda_", "AF_1mc_"),
    ("vox_pla_", "RU_1mc_"),
];

/// BO2 lines MW3 says under another name (BO2 line -> MW3 line). Lines with
/// the same name in both (lead_taken, tied, timesup, winning, overtime...)
/// need no row; Black Ops' names, which BO2 kept, come back through
/// `T5_LINES` (MW's name -> Black Ops'), read the other way.
const T6_MW3_LINES: &[(&str, &str)] = &[
    ("dm_start", "freeforall"),
    ("conf_start", "kill_confirmed"),
    ("kc_start", "kill_confirmed"),
    ("dem_start", "demolition"),
    ("koth_start", "headquarters"),
    ("koth_located", "hq_located"),
    ("koth_online", "hq_online"),
    ("koth_offline", "hq_offline"),
    ("koth_secured", "hq_secured"),
    ("koth_captured", "hq_captured"),
    ("koth_lost", "hq_destroyed"),
    ("hq_located", "hq_located"),
    ("hq_online", "hq_online"),
    ("hq_offline", "hq_offline"),
    ("hq_secured", "hq_secured"),
    ("sd_bomb_taken", "acheive_bomb"),
    ("dest_sentry", "sentry_destroyed"),
    ("sudden_death", "suddendeath"),
    ("last_alive", "lastalive"),
    ("mission_failure", "mission_fail"),
    ("round_success", "encourage_win"),
    ("round_failure", "encourage_lost"),
    ("round_draw", "draw"),
    ("min_draw", "draw"),
];

/// BO2 scorestreak (the `kls_<streak>_*` lines) -> MW3's nearest (its
/// `achieve_` / `use_` / `enemy_` lines). A streak not here is tried under
/// its own name.
const T6_MW3_STREAKS: &[(&str, &str)] = &[
    ("radar", "uav"),
    ("counteruav", "jamuav"),
    ("satellite", "advanced_uav"),
    ("radardirection", "advanced_uav"),
    ("supplydrop", "carepackage"),
    ("airdrop", "carepackage"),
    ("remotemissile", "predator"),
    ("predator", "predator"),
    ("planemortar", "airstrike"),
    ("straferun", "strafe"),
    ("autoturret", "sentrygun"),
    ("turret", "sentrygun"),
    ("sentry", "sentrygun"),
    ("aitank", "assault_drone"),
    ("helicopter", "heli"),
    ("helicopter_comlink", "heli"),
    ("playerheli", "heli_sniper"),
    ("helicopter_player_gunner", "heli_sniper"),
    ("helicopter_guard", "lbguard"),
    ("littlebird", "littlebird"),
    ("remote_mortar", "ac130"),
    ("remotemortar", "ac130"),
    ("qrdrone", "recon_drone"),
    ("emp", "emp"),
    ("moab", "moab"),
];

/// The MW3 lines that may say a BO2 line (`line`: past `vox_<faction>_`),
/// most fitting first.
fn t6_mw3_lines(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(rest) = line.strip_prefix("kls_") {
        for (kind, mw3) in [("_ready", "achieve_"), ("_used", "use_"), ("_enemy", "enemy_")] {
            if let Some(streak) = rest.strip_suffix(kind) {
                let to = T6_MW3_STREAKS.iter().find(|(from, _)| *from == streak).map_or(streak, |&(_, to)| to);
                out.push(format!("{mw3}{to}"));
                // (MW3 shortens the advanced UAV on its own earning line.)
                if to == "advanced_uav" && mw3 == "achieve_" {
                    out.push("achieve_adv_uav".to_owned());
                }
            }
        }
        return out;
    }
    if let Some(&(_, to)) = T6_MW3_LINES.iter().find(|(from, _)| *from == line) {
        out.push(to.to_owned());
    }
    if let Some(&(mw, _)) = T5_LINES.iter().find(|(_, t5)| *t5 == line) {
        out.push(mw.to_owned());
    }
    out.push(line.to_owned());
    out
}

/// bo2mp: every Black Ops II announcer alias MW3's voice can stand in for:
/// the bank's own BO2 lines, and each faction's alias of every line the
/// tables (or MW3's own names) cover, whether or not BO2's line is in the
/// bank (a foreign map's bank has none of them; see
/// `t6_announcer_candidates`).
pub(crate) fn t6_mw3_candidates(bank: &asset_audio::SoundCatalog) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut push = |alias: String| {
        if seen.insert(alias.clone()) {
            out.push(alias);
        }
    };
    for alias in t6_announcer_aliases(bank) {
        push(alias.to_ascii_lowercase());
    }
    let mut lines: Vec<String> = T6_MW3_LINES.iter().map(|(from, _)| (*from).to_owned()).collect();
    lines.extend(T5_LINES.iter().map(|(_, t5)| (*t5).to_owned()));
    for (streak, _) in T6_MW3_STREAKS {
        for kind in ["ready", "used", "enemy"] {
            lines.push(format!("kls_{streak}_{kind}"));
        }
    }
    // (Lines MW3 says under BO2's own names: every US line's name.)
    lines.extend(
        (0..bank.sounds.len())
            .filter(|&index| bank.namespace_of_alias(index) == AssetNamespace::Iw5)
            .filter_map(|index| bank.name_at(index))
            .filter_map(|name| name.strip_prefix("US_1mc_"))
            .map(str::to_ascii_lowercase),
    );
    for &(t6_voice, _) in &T6_MW3_VOICES {
        for line in &lines {
            push(format!("{t6_voice}{line}"));
        }
    }
    out
}

/// Routes Black Ops II announcer aliases (`vox_<faction>_<line>`) to Modern
/// Warfare 3's (IW5) lines in the faction's MW3 voice. Only a target the bank
/// carries is routed; a line MW3 never says is left to Black Ops' voice
/// (`t6_bo1_routes`), then BO2's own.
pub(crate) fn t6_mw3_routes(
    bank: &asset_audio::SoundCatalog,
    t6_aliases: &[String],
) -> HashMap<String, (AssetNamespace, String)> {
    t6_aliases
        .iter()
        .filter_map(|alias| {
            let lower = alias.to_ascii_lowercase();
            let &(t6_voice, mw3_voice) = T6_MW3_VOICES.iter().find(|(t6, _)| lower.starts_with(t6))?;
            let line = &lower[t6_voice.len()..];
            let routed = t6_mw3_lines(line)
                .into_iter()
                .map(|to| format!("{mw3_voice}{to}"))
                .find(|to| bank.index_in(AssetNamespace::Iw5, to).is_some())?;
            Some((lower, (AssetNamespace::Iw5, routed)))
        })
        .collect()
}

/// BO2 faction voice → the BO1 voice and MW3 voice that replace it. The
/// side BO2 puts a faction on picks the voice: the American and allied
/// factions speak as Black Ops (MW3: US), their opponents as Spetsnaz
/// (MW3: Russian).
const T6_VOICES: [(&str, &str, &str); 6] = [
    ("vox_st6_", "vox_ops_", "US_1mc_"),
    ("vox_fbi_", "vox_ops_", "US_1mc_"),
    ("vox_isa_", "vox_ops_", "US_1mc_"),
    ("vox_pmc_", "vox_rus_", "RU_1mc_"),
    ("vox_cda_", "vox_rus_", "RU_1mc_"),
    ("vox_pla_", "vox_rus_", "RU_1mc_"),
];

/// What a BO2 line with no same-named BO1 line is said with instead.
#[derive(Clone, Copy)]
enum Stand {
    /// A BO1 team line, spoken in the faction's BO1 voice.
    Bo1(&'static str),
    /// A BO1 wager-match line (one announcer for everyone).
    Bo1Wager(&'static str),
    /// An MW3 team line, for what BO1 never said.
    Mw3(&'static str),
}

/// Lines whose names match BO1 play as themselves; a line in neither this
/// table nor BO1 keeps BO2's voice.
const T6_STAND_INS: &[(&str, Stand)] = &[
    ("koth_start", Stand::Bo1("hq_start")),
    ("koth_located", Stand::Bo1("hq_located")),
    ("koth_online", Stand::Bo1("hq_online")),
    ("koth_secured", Stand::Bo1("hq_secured")),
    ("koth_captured", Stand::Bo1("hq_capture")),
    ("koth_lost", Stand::Bo1("hq_capture")),
    ("sd_bomb_taken", Stand::Bo1("sd_bomb_taken_taken")),
    ("kls_planemortar_ready", Stand::Bo1("kls_airstrike_ready")),
    ("kls_planemortar_used", Stand::Bo1("kls_airstrike_used")),
    ("kls_planemortar_enemy", Stand::Bo1("kls_airstrike_enemy")),
    ("kls_straferun_ready", Stand::Bo1("kls_napalm_ready")),
    ("kls_straferun_used", Stand::Bo1("kls_napalm_used")),
    ("kls_straferun_enemy", Stand::Bo1("kls_napalm_enemy")),
    ("kls_mgl_ready", Stand::Bo1("kls_grim_ready")),
    ("kls_mgl_used", Stand::Bo1("kls_grim_used")),
    ("kls_mgl_enemy", Stand::Bo1("kls_grim_enemy")),
    ("kls_predator_ready", Stand::Bo1("kls_tv_ready")),
    ("kls_predator_used", Stand::Bo1("kls_tv_used")),
    ("kls_predator_enemy", Stand::Bo1("kls_tv_enemy")),
    ("kls_littlebird_ready", Stand::Bo1("kls_cobra_ready")),
    ("kls_littlebird_used", Stand::Bo1("kls_cobra_used")),
    ("kls_littlebird_enemy", Stand::Bo1("kls_cobra_enemy")),
    ("kls_playerheli_ready", Stand::Bo1("kls_huey_ready")),
    ("kls_playerheli_used", Stand::Bo1("kls_huey_used")),
    ("kls_playerheli_enemy", Stand::Bo1("kls_huey_enemy")),
    ("kls_microwave_ready", Stand::Bo1("kls_turret_ready")),
    ("kls_microwave_used", Stand::Bo1("kls_turret_used")),
    ("kls_microwave_enemy", Stand::Bo1("kls_turret_enemy")),
    ("gg_start", Stand::Bo1Wager("gg_start_00")),
    ("oic_start", Stand::Bo1Wager("start_gen")),
    ("oic_last", Stand::Bo1Wager("oic_finallife")),
    ("ss_start", Stand::Bo1Wager("ss_start")),
    ("sns_start", Stand::Bo1Wager("sstones_start")),
    ("sns_hum", Stand::Bo1Wager("sstones_bank_00")),
    ("kc_start", Stand::Mw3("kill_confirmed")),
    ("kls_emp_ready", Stand::Mw3("achieve_emp")),
    ("kls_emp_used", Stand::Mw3("use_emp")),
    ("kls_emp_enemy", Stand::Mw3("enemy_emp")),
    ("kls_aitank_ready", Stand::Mw3("achieve_assault_drone")),
    ("kls_aitank_used", Stand::Mw3("use_assault_drone")),
    ("kls_aitank_enemy", Stand::Mw3("enemy_assault_drone")),
];

const BO1_WAGER_VOICE: &str = "vox_wm_";

/// bo2mp: every Black Ops II announcer alias the BO1 voice can stand in for,
/// whether or not BO2's own line is in the bank. On a map from another game
/// the BO2-rules sound bank carries BO2's guns, UI and common sounds but not
/// the faction language banks a BO2 map loads beside its zone, so the
/// `vox_<faction>_*` lines the scripts play were never in it: deriving the
/// routes from the bank alone left none, and the announcer went silent.
/// Each BO1 team line (`vox_ops_*`, `vox_rus_*`) names the BO2 aliases of
/// the factions that speak with that voice; the stand-in table adds the
/// lines BO1 says under another name.
pub(crate) fn t6_announcer_candidates(bank: &asset_audio::SoundCatalog) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut push = |alias: String| {
        if seen.insert(alias.clone()) {
            out.push(alias);
        }
    };
    for alias in t6_announcer_aliases(bank) {
        push(alias.to_ascii_lowercase());
    }
    let bo1_lines: Vec<String> = (0..bank.sounds.len())
        .filter(|&index| bank.namespace_of_alias(index) == AssetNamespace::T5)
        .filter_map(|index| bank.name_at(index))
        .map(str::to_ascii_lowercase)
        .filter(|name| name.starts_with("vox_ops_") || name.starts_with("vox_rus_"))
        .collect();
    for &(t6_voice, bo1_voice, _) in &T6_VOICES {
        for name in &bo1_lines {
            if let Some(line) = name.strip_prefix(bo1_voice) {
                push(format!("{t6_voice}{line}"));
            }
        }
        for (from, _) in T6_STAND_INS {
            push(format!("{t6_voice}{from}"));
        }
    }
    out
}

/// Routes Black Ops II announcer aliases (`vox_<faction>_<line>`) to BO1's
/// lines (T5 namespace), or MW3's (IW5) where BO1 has none. Only a target the
/// bank carries is routed.
pub(crate) fn t6_bo1_routes(
    bank: &asset_audio::SoundCatalog,
    t6_aliases: &[String],
) -> HashMap<String, (AssetNamespace, String)> {
    t6_aliases
        .iter()
        .filter_map(|alias| {
            let lower = alias.to_ascii_lowercase();
            let &(t6_voice, bo1_voice, mw3_voice) =
                T6_VOICES.iter().find(|(t6, _, _)| lower.starts_with(t6))?;
            let line = &lower[t6_voice.len()..];
            let stand = T6_STAND_INS
                .iter()
                .find(|(from, _)| *from == line)
                .map(|&(_, stand)| stand);
            let (namespace, routed) = match stand {
                None => (AssetNamespace::T5, format!("{bo1_voice}{line}")),
                Some(Stand::Bo1(to)) => (AssetNamespace::T5, format!("{bo1_voice}{to}")),
                Some(Stand::Bo1Wager(to)) => {
                    (AssetNamespace::T5, format!("{BO1_WAGER_VOICE}{to}"))
                }
                Some(Stand::Mw3(to)) => (AssetNamespace::Iw5, format!("{mw3_voice}{to}")),
            };
            bank.index_in(namespace, &routed)?;
            Some((lower, (namespace, routed)))
        })
        .collect()
}

const TEAM_MUSIC: [&str; 5] = [
    "spawn_music",
    "victory_music",
    "defeat_music",
    "winning_music",
    "losing_music",
];

const IW4_LINE: &str = "1mc_";

const T5_LINES: &[(&str, &str)] = &[
    ("acheive_bomb", "sd_bomb_taken_taken"),
    ("bomb_defused", "sd_bomb_defused"),
    ("bomb_planted", "sd_bomb_planted"),
    ("bomb_taken", "sd_bomb_drop"),
    ("boost", "generic_boost"),
    ("capture_a", "dom_capture_a"),
    ("capture_b", "dom_capture_c"),
    ("capture_c", "dom_capture_b"),
    ("capturing_a", "dom_capturing_a"),
    ("capturing_b", "dom_capturing_b"),
    ("capturing_c", "dom_capturing_c"),
    ("captureflag", "ctf_start"),
    ("demolition", "demo_start"),
    ("dest_sentrygun", "dest_sentry"),
    ("domination", "dom_start"),
    ("freeforall", "ffa_start"),
    ("headquarters", "hq_start"),
    ("hq_captured", "hq_capture"),
    ("hq_destroyed", "hq_defend"),
    ("losing_a", "dom_losing_a"),
    ("losing_b", "dom_losing_b"),
    ("losing_c", "dom_losing_c"),
    ("lost_a", "dom_lost_a"),
    ("lost_b", "dom_lost_b"),
    ("lost_c", "dom_lost_c"),
    ("obj_defend", "defend_start"),
    ("obj_destroy", "destroy_start"),
    ("positions_lock", "dom_lock_wetake"),
    ("sabotage", "sab_start"),
    ("searchdestroy", "sd_start"),
    ("secure_a", "dom_secured_a"),
    ("secure_b", "dom_secured_b"),
    ("secure_c", "dom_secured_c"),
    ("securing_a", "dom_securing_a"),
    ("securing_b", "dom_securing_b"),
    ("securing_c", "dom_securing_c"),
    ("switching", "switchingsides"),
    ("take_positions", "dom_lock_theytake"),
    ("tm_death", "tdm_start"),
];

#[derive(Resource, Default)]
pub(crate) struct AnnouncerRoutes(HashMap<String, (AssetNamespace, String)>);

impl AnnouncerRoutes {
    pub(crate) fn route<'a>(&'a self, alias: &'a str) -> (AssetNamespace, &'a str) {
        self.0
            .get(alias)
            .map_or((AssetNamespace::Iw4, alias), |(ns, routed)| (*ns, routed))
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// The route for a script-played alias of `namespace`, if one is set.
    /// Black Ops II's routes are keyed by bare alias name.
    pub(crate) fn routed(
        &self,
        namespace: AssetNamespace,
        alias: &str,
    ) -> Option<(AssetNamespace, &str)> {
        if namespace != AssetNamespace::T6 {
            return None;
        }
        self.0
            .get(alias)
            .map(|(ns, routed)| (*ns, routed.as_str()))
    }

    pub(crate) fn extend(&mut self, routes: HashMap<String, (AssetNamespace, String)>) {
        self.0.extend(routes);
    }

    pub(crate) fn build(
        bank: &asset_audio::SoundCatalog,
        namespace: AssetNamespace,
        iw4: [Option<&str>; 2],
        native: Option<&asset_game::MapTeamSettings>,
        aliases: &[String],
    ) -> Self {
        let lines: &[(&str, &str)] = match namespace {
            AssetNamespace::Iw4 => return Self::default(),
            AssetNamespace::T5 => T5_LINES,
            AssetNamespace::Iw5 => &[],
            // bo2zm: Black Ops II announcer lines are not mapped yet.
            AssetNamespace::T6 | AssetNamespace::Iw6 | AssetNamespace::T4 | AssetNamespace::Iw3 | AssetNamespace::S1 | AssetNamespace::T7 | AssetNamespace::Jup | AssetNamespace::H1 | AssetNamespace::Iw7 => &[],
        };
        let Some(native) = native else {
            return Self::default();
        };
        let teams: Vec<(&str, Option<&str>, &asset_game::TeamMusic)> = iw4
            .into_iter()
            .zip([
                (native.allies_voice.as_deref(), &native.allies_music),
                (native.axis_voice.as_deref(), &native.axis_music),
            ])
            .filter_map(|(iw4, (voice, music))| Some((iw4?, voice, music)))
            .collect();
        let routes = aliases
            .iter()
            .filter_map(|alias| {
                teams.iter().find_map(|&(iw4, voice, music)| {
                    let line = alias.strip_prefix(iw4)?;
                    let routed = match line.strip_prefix(IW4_LINE) {
                        Some(line) => {
                            let line = lines
                                .iter()
                                .find(|(from, _)| *from == line)
                                .map_or(line, |(_, to)| to);
                            format!("{}{line}", voice?)
                        }
                        None => match line {
                            "spawn_music" => music.spawn.clone(),
                            "victory_music" => music.victory.clone(),
                            "defeat_music" => music.defeat.clone(),
                            "winning_music" => music.winning.clone(),
                            "losing_music" => music.losing.clone(),
                            _ => None,
                        }?,
                    };
                    bank.index_in(namespace, &routed)?;
                    Some((alias.clone(), (namespace, routed)))
                })
            })
            .collect();
        Self(routes)
    }
}

#[cfg(test)]
mod mw3_announcer_tests {
    use super::t6_mw3_lines;

    /// Black Ops II's common lines each find their Modern Warfare 3 line.
    #[test]
    fn bo2_lines_take_mw3_names() {
        let first = |l: &str| t6_mw3_lines(l).into_iter().next().unwrap_or_default();
        assert_eq!(first("tdm_start"), "tm_death");
        assert_eq!(first("ffa_start"), "freeforall");
        assert_eq!(first("dom_secured_a"), "secure_a");
        assert_eq!(first("sd_bomb_planted"), "bomb_planted");
        assert_eq!(first("switchingsides"), "switching");
        assert_eq!(first("koth_online"), "hq_online");
        assert_eq!(first("lead_taken"), "lead_taken");
        assert_eq!(first("kls_radar_ready"), "achieve_uav");
        assert_eq!(first("kls_counteruav_enemy"), "enemy_jamuav");
        assert_eq!(t6_mw3_lines("kls_satellite_ready"), ["achieve_advanced_uav", "achieve_adv_uav"]);
    }
}

