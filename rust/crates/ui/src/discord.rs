//! bo2mp: Discord Rich Presence (`discord_presence`): Discord shows
//! "Playing Call of Duty: Legacy" with what the player is doing: in the
//! menus, loading a map, or the mode and map of the match and how it
//! stands (team score lead, or place in a free-for-all), with the time
//! played and the time left.
//!
//! Discord takes the game's name and its pictures from a Discord
//! application: the owner makes one named "Call of Duty: Legacy" on the
//! Discord developer portal, uploads `assets/branding/legacy_discord.png`
//! under Rich Presence art assets as `legacy_logo`, and gives the game its
//! application ID: `BO2MP_DISCORD_APP_ID=<id>`, or the ID alone in
//! `discord_app_id.txt` beside the game, or [`APP_ID`] in this file. No ID:
//! nothing is sent. `BO2MP_DISCORD=0` turns it off.

use bevy::prelude::*;
use discord_presence::{Activity, Presence, unix_now};
use net::{LocalPresentClient, PresentedSnapshot};

/// The Discord application's ID built in (empty: read from the
/// environment or `discord_app_id.txt`).
pub const APP_ID: &str = "";
/// The large picture's asset key on the Discord application.
pub const LOGO_KEY: &str = "legacy_logo";
/// The game's title, the large picture's hover text.
pub const TITLE: &str = "Call of Duty: Legacy";

/// The application ID to use, if any.
fn app_id() -> Option<String> {
    if std::env::var("BO2MP_DISCORD").is_ok_and(|v| v.trim() == "0") {
        return None;
    }
    let from_file = || {
        let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(std::path::Path::to_path_buf));
        [Some(std::path::PathBuf::from("discord_app_id.txt")), exe_dir.map(|d| d.join("discord_app_id.txt"))]
            .into_iter()
            .flatten()
            .find_map(|p| std::fs::read_to_string(p).ok())
    };
    let id = std::env::var("BO2MP_DISCORD_APP_ID").ok().or_else(from_file).unwrap_or_else(|| APP_ID.to_owned());
    let id = id.trim().to_owned();
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

/// A map's name as people say it: `mp_nuketown_2020` -> `Nuketown 2020`,
/// `t6:mp_raid` -> `Raid`, `mp_warhawk_t7` -> `Warhawk`.
pub fn map_name(map: &str) -> String {
    let stem = map.rsplit(':').next().unwrap_or(map);
    let stem = stem.trim_start_matches("iw7_").trim_start_matches("mp_").trim_start_matches("zm_").trim_start_matches("cp_");
    // (A shared name's game suffix.)
    let stem = ["_t7", "_iw6", "_iw7", "_h1", "_s1", "_iw4", "_iw5", "_t5", "_t4", "_iw3", "_jup", "_bf1"].iter().find_map(|s| stem.strip_suffix(s)).unwrap_or(stem);
    stem.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| f.to_uppercase().collect::<String>() + c.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A game type's name from its token (BO2's `ui_gametype`).
pub fn mode_name(token: &str) -> String {
    match token.to_ascii_lowercase().as_str() {
        "tdm" | "war" => "Team Deathmatch",
        "dm" | "ffa" => "Free-for-All",
        "dom" => "Domination",
        "sd" => "Search and Destroy",
        "conf" => "Kill Confirmed",
        "koth" => "Hardpoint",
        "hq" => "Headquarters",
        "ctf" => "Capture the Flag",
        "dem" | "dd" => "Demolition",
        "oneflag" => "One Flag CTF",
        "gun" => "Gun Game",
        "shrp" => "Sharpshooter",
        "sas" => "Sticks and Stones",
        "oic" => "One in the Chamber",
        "sab" => "Sabotage",
        "br" | "resurgence" | "warzone" => "Resurgence",
        "zclassic" | "zstandard" | "zm" => "Zombies",
        "" => "Custom Game",
        other => return other.to_uppercase(),
    }
    .to_owned()
}

/// Where the map comes from, for the second line before a match.
fn origin(map: &str) -> Option<&'static str> {
    let game = session::bo2mp_setup::foreign_map(map).map(|(g, _)| g).or_else(|| map.split_once(':').map(|(g, _)| g.to_owned()))?;
    Some(match game.as_str() {
        "iw3" => "a Modern Warfare map",
        "t4" => "a World at War map",
        "iw4" => "a Modern Warfare 2 map",
        "iw5" => "a Modern Warfare 3 map",
        "t5" => "a Black Ops map",
        "iw6" => "a Ghosts map",
        "s1" => "an Advanced Warfare map",
        "h1" => "a Call of Duty 4 map",
        "iw7" => "an Infinite Warfare map",
        "t7" => "a Black Ops III map",
        "jup" => "a Modern Warfare III map",
        "bf1" => "a Battlefield 1 map",
        _ => return None,
    })
}

/// How a match stands for the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Team game: (ours, theirs).
    Team(i32, i32),
    /// Free-for-all: (place, players, score).
    Place(usize, usize, i32),
}

impl Standing {
    pub fn line(self) -> String {
        match self {
            Self::Team(us, them) if us > them => format!("Winning {us} - {them}"),
            Self::Team(us, them) if us < them => format!("Losing {us} - {them}"),
            Self::Team(us, them) => format!("Tied {us} - {them}"),
            Self::Place(place, players, score) => {
                let suffix = match (place % 10, place % 100) {
                    (_, 11..=13) => "th",
                    (1, _) => "st",
                    (2, _) => "nd",
                    (3, _) => "rd",
                    _ => "th",
                };
                format!("{place}{suffix} of {players} - {score} points")
            }
        }
    }
}

#[derive(Default)]
struct DiscordState {
    presence: Option<Presence>,
    started: bool,
    /// What the player was last doing (for "elapsed" since it began).
    phase: String,
    since: u64,
    last_check: f32,
    last_status: String,
}

#[allow(clippy::too_many_arguments)]
fn update_presence(
    mut st: Local<DiscordState>,
    time: Res<Time>,
    front: Option<Res<crate::lui_frontend::Bo2mpFrontend>>,
    loading: Option<Res<assets::LoadingScreen>>,
    setup: Option<Res<session::Bo2mpMatchSetup>>,
    identity: Option<Res<frame::LaunchIdentity>>,
    presented: Option<Res<PresentedSnapshot>>,
    local: Option<Res<LocalPresentClient>>,
) {
    if !st.started {
        st.started = true;
        match app_id() {
            Some(id) => {
                diag::info!(Ui, "discord: rich presence for application {id}");
                st.presence = Some(Presence::start(&id));
            }
            None => diag::info!(Ui, "discord: no application ID (BO2MP_DISCORD_APP_ID or discord_app_id.txt); rich presence off"),
        }
    }
    let now = time.elapsed_secs();
    if st.presence.is_none() || now - st.last_check < 1.0 {
        return;
    }
    st.last_check = now;
    let map = setup.as_ref().map(|s| s.map.clone()).filter(|m| !m.is_empty()).or_else(|| identity.as_ref().map(|i| i.zone.clone())).unwrap_or_default();
    let mode = setup.as_ref().map_or_else(|| if crate::lui_hud::is_zombies_zone(&map) { "zm".to_owned() } else { String::new() }, |s| s.gametype.clone());
    let snap = presented.as_ref().and_then(|p| p.snapshot());
    let (phase, details, state, end) = if loading.is_some() {
        let name = map_name(&map);
        ("loading".to_owned(), if name.is_empty() { "Loading".to_owned() } else { format!("Loading {name}") }, mode_name(&mode), None)
    } else if let (Some(snap), Some(local), false) = (snap.as_ref(), local.as_ref(), front.is_some() && setup.is_none()) {
        let details = format!("{} on {}", mode_name(&mode), map_name(&map));
        let standing = snap.meta.for_client(local.0).map(|me| {
            if snap.meta.kind != gamemode_iw4::GameModeKind::FreeForAll && matches!(me.client_state_team, 1 | 2) {
                let s = snap.meta.objectives.scores;
                let (us, them) = if me.client_state_team == 1 { (s[1], s[2]) } else { (s[2], s[1]) };
                Standing::Team(us, them)
            } else {
                let mut scores: Vec<i32> = snap.meta.clients.iter().map(|(_, m)| m.score).collect();
                scores.sort_unstable_by(|a, b| b.cmp(a));
                let place = scores.iter().position(|&s| s <= me.score).map_or(scores.len(), |i| i + 1);
                Standing::Place(place, scores.len(), me.score)
            }
        });
        let now_ms = snap.tick.0.saturating_mul(sim::MATCH_TICK_MS) as i32;
        let left = (snap.meta.time_limit_ms > 0).then(|| snap.meta.objectives.time_left_ms(now_ms).max(0) as u64 / 1000).filter(|&s| s > 0);
        (format!("match:{map}:{mode}"), details, standing.map_or_else(String::new, Standing::line), left.map(|s| unix_now() + s))
    } else {
        let state = origin(&map).map(|o| format!("Next up: {}", map_name(&map)) + &format!(" ({o})")).filter(|_| setup.is_some()).unwrap_or_default();
        ("menus".to_owned(), "In the menus".to_owned(), state, None)
    };
    if phase != st.phase {
        st.phase = phase;
        st.since = unix_now();
    }
    let activity = Activity { details, state, large_image: LOGO_KEY.to_owned(), large_text: TITLE.to_owned(), start: Some(st.since), end, ..Default::default() };
    if let Some(p) = st.presence.as_ref() {
        p.set(activity);
        let status = p.status();
        if status != st.last_status {
            diag::info!(Ui, "discord: {status}");
            st.last_status = status;
        }
    }
}

pub(crate) fn register(app: &mut App) {
    app.add_systems(Update, update_presence);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_and_mode_names_read_as_people_say_them() {
        assert_eq!(map_name("mp_nuketown_2020"), "Nuketown 2020");
        assert_eq!(map_name("iw7:mp_riot"), "Riot");
        assert_eq!(map_name("mp_warhawk_t7"), "Warhawk");
        assert_eq!(mode_name("conf"), "Kill Confirmed");
        assert_eq!(mode_name("koth"), "Hardpoint");
    }

    #[test]
    fn the_standing_says_who_leads() {
        assert_eq!(Standing::Team(54, 32).line(), "Winning 54 - 32");
        assert_eq!(Standing::Team(10, 20).line(), "Losing 10 - 20");
        assert_eq!(Standing::Place(1, 8, 25).line(), "1st of 8 - 25 points");
        assert_eq!(Standing::Place(12, 18, 3).line(), "12th of 18 - 3 points");
    }
}
