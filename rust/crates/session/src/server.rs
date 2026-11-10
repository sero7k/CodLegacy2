//! bo2mp: the dedicated server's map rotation (`CodLegacy serve`). Each match
//! plays its entry's map and mode with the server's bots; when it ends the
//! next entry starts, through the server's room on the master when it has
//! one (everyone in it loads from the offer), else straight here.
//!
//! `CODLEGACY_SERVER_ROTATION`: entries split by `,` or `;`, each `<zone> [mode]`
//! (`iw4:mp_highrise tricky_dm, t6:mp_nuketown_2020 tdm`); the mode defaults
//! to `CODLEGACY_GAMETYPE`, else `tdm`. `CODLEGACY_SERVER_BOTS=<friends>,<enemies>[,<difficulty 0-3>]`,
//! `CODLEGACY_SERVER_TRICKSHOT=1`. A Tricky mode plays its base mode with the
//! trickshot script on; Tricky Search and Destroy with two enemy bots only.
//!
//! With a room, the players vote the next map between matches: two of the
//! rotation's maps (never one map in two games' versions, nor the one just
//! played), `CODLEGACY_SERVER_VOTE_S` seconds (20; `0`: no vote, the rotation's
//! order). A rotation of one map counts down to it instead.

use std::sync::Mutex;

use bevy::prelude::*;
use frame::{HasWorld, ReturnedToMenu, RuntimeRole};

use crate::lifecycle::SessionSwapRequest;

/// The Tricky modes: (key, the mode they play).
const TRICKY: &[(&str, &str)] = &[("tricky_dm", "dm"), ("tricky_sd", "sd")];
const TRICKY_SD_BOTS: (u32, u32) = (0, 2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RotationEntry {
    pub zone: String,
    pub mode: String,
}

#[derive(Resource, Clone, Debug)]
pub struct ServerRotation {
    pub entries: Vec<RotationEntry>,
    pub at: usize,
    /// (friends, enemies, difficulty).
    pub bots: (u32, u32, u32),
    pub trickshot: bool,
    /// The pre-game map vote's length (0: none).
    pub vote_s: u16,
}

const VOTE_S: u16 = 20;

static CONFIGURED: Mutex<Option<ServerRotation>> = Mutex::new(None);

impl ServerRotation {
    /// The rotation `CodLegacy serve [zone]` plays: `zone` first (when given),
    /// then `CODLEGACY_SERVER_ROTATION`'s entries.
    pub fn from_env(zone: &str) -> Result<Self, String> {
        let default_mode = codlegacy_env::var("CODLEGACY_GAMETYPE")
            .ok()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| "tdm".to_owned());
        let mut entries = Vec::new();
        if !zone.trim().is_empty() {
            entries.push(RotationEntry {
                zone: zone.trim().to_ascii_lowercase(),
                mode: default_mode.clone(),
            });
        }
        if let Ok(list) = codlegacy_env::var("CODLEGACY_SERVER_ROTATION") {
            entries.extend(parse_rotation(&list, &default_mode));
        }
        if entries.is_empty() {
            return Err(
                "usage: CodLegacy serve <zone>, or set CODLEGACY_SERVER_ROTATION=\"<zone> [mode], ...\""
                    .to_owned(),
            );
        }
        let bots = codlegacy_env::var("CODLEGACY_SERVER_BOTS")
            .map_or(Ok((0, 0, 1)), |spec| parse_bots(&spec))?;
        let trickshot =
            codlegacy_env::var("CODLEGACY_SERVER_TRICKSHOT").is_ok_and(|v| v.trim() == "1");
        let vote_s = match codlegacy_env::var("CODLEGACY_SERVER_VOTE_S") {
            Ok(v) => v
                .trim()
                .parse::<u16>()
                .map_err(|_| format!("CODLEGACY_SERVER_VOTE_S=`{v}`: seconds (0: no vote)"))?,
            Err(_) => VOTE_S,
        };
        Ok(Self {
            entries,
            at: 0,
            bots,
            trickshot,
            vote_s,
        })
    }

    pub fn current(&self) -> &RotationEntry {
        &self.entries[self.at % self.entries.len()]
    }

    fn advance(&mut self) {
        self.at = (self.at + 1) % self.entries.len();
    }

    /// Two of the rotation's maps to vote between (one when it has one).
    fn vote_maps(&self, seed: u64) -> Vec<String> {
        let mut pool: Vec<(String, Option<String>)> = Vec::new();
        for e in &self.entries {
            if !pool.iter().any(|(z, _)| *z == e.zone) {
                pool.push((e.zone.clone(), None));
            }
        }
        net::master_protocol::mapvote::pick(&pool, Some(&self.current().zone), seed)
    }

    /// The next match plays `zone` (its first entry from here on).
    fn select(&mut self, zone: &str) {
        let n = self.entries.len();
        if let Some(k) = (1..=n).map(|k| (self.at + k) % n).find(|&k| self.entries[k].zone == zone) {
            self.at = k;
        }
    }

    /// The game type the room and the match run (a Tricky mode's base).
    pub fn gametype(&self) -> String {
        let mode = &self.current().mode;
        tricky_base(mode).map_or_else(|| mode.clone(), str::to_owned)
    }

    /// The current entry's match setup; sets the trickshot script for it.
    fn apply(&self) -> crate::Bo2mpMatchSetup {
        let tricky = tricky_base(&self.current().mode);
        sim::set_trickshot(self.trickshot || tricky.is_some());
        let (friends, enemies) = if tricky == Some("sd") {
            TRICKY_SD_BOTS
        } else {
            (self.bots.0, self.bots.1)
        };
        let entry = self.current();
        diag::info!(
            Launch,
            "server rotation {}/{}: {} {} (bots {friends}+{enemies}, trickshot {})",
            self.at + 1,
            self.entries.len(),
            entry.zone,
            entry.mode,
            self.trickshot || tricky.is_some()
        );
        crate::Bo2mpMatchSetup {
            map: crate::ui_name_for_zone(&entry.zone),
            gametype: self.gametype(),
            bot_friends: friends,
            bot_enemies: enemies,
            bot_difficulty: self.bots.2,
        }
    }
}

fn tricky_base(mode: &str) -> Option<&'static str> {
    TRICKY
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(mode))
        .map(|(_, b)| *b)
}

fn parse_rotation(list: &str, default_mode: &str) -> Vec<RotationEntry> {
    list.split([',', ';', '\n'])
        .filter_map(|entry| {
            let mut words = entry.split_whitespace();
            let zone = words.next()?.to_ascii_lowercase();
            let mode = words
                .next()
                .map_or_else(|| default_mode.to_owned(), str::to_ascii_lowercase);
            Some(RotationEntry { zone, mode })
        })
        .collect()
}

fn parse_bots(spec: &str) -> Result<(u32, u32, u32), String> {
    let v: Vec<u32> = spec
        .split(',')
        .map(|s| {
            s.trim().parse::<u32>().map_err(|_| {
                format!("CODLEGACY_SERVER_BOTS=`{spec}`: <friends>,<enemies>[,<difficulty 0-3>]")
            })
        })
        .collect::<Result<_, _>>()?;
    match v.as_slice() {
        [f, e] => Ok((*f, *e, 1)),
        [f, e, d] if *d <= 3 => Ok((*f, *e, *d)),
        _ => Err(format!(
            "CODLEGACY_SERVER_BOTS=`{spec}`: <friends>,<enemies>[,<difficulty 0-3>]"
        )),
    }
}

/// `CodLegacy serve`: this process runs the rotation (read by `SessionPlugin`).
pub fn configure(rotation: ServerRotation) {
    if let Ok(mut c) = CONFIGURED.lock() {
        *c = Some(rotation);
    }
}

/// The configured rotation's first game type (the room's mode at launch).
pub fn first_gametype() -> Option<String> {
    CONFIGURED
        .lock()
        .ok()?
        .as_ref()
        .map(ServerRotation::gametype)
}

pub(crate) fn register(app: &mut App) {
    let Some(rotation) = CONFIGURED.lock().ok().and_then(|mut c| c.take()) else {
        return;
    };
    let setup = rotation.apply();
    app.insert_resource(setup)
        .insert_resource(rotation)
        .add_systems(
            Update,
            next_map
                .in_set(frame::SessionSwapApplied)
                .before(crate::lifecycle::follow_master_match),
        );
}

/// After a match ends (back at the menu, nothing loaded), the next entry:
/// the room's next match when the server hosts one, else loaded here.
#[allow(clippy::too_many_arguments)]
fn next_map(
    mut commands: Commands,
    mut rotation: ResMut<ServerRotation>,
    mut ended: MessageReader<ReturnedToMenu>,
    role: Res<RuntimeRole>,
    has_world: Res<HasWorld>,
    busy: Option<Res<assets::MatchLoadBusy>>,
    request: Option<Res<assets::MatchLoadRequest>>,
    mut swap: ResMut<SessionSwapRequest>,
    bridge: Option<Res<net::MasterBridge>>,
    mut menu: Option<ResMut<net::PendingMasterMenuAction>>,
    mut due: Local<bool>,
    mut opened: Local<Option<(Vec<String>, std::time::Instant)>>,
) {
    if ended.read().last().is_some() && *role == RuntimeRole::Dedicated && !*due {
        *due = true;
        *opened = None;
    }
    if !*due || has_world.0 || busy.is_some_and(|b| b.0) || request.is_some() || swap.is_pending() {
        return;
    }
    let state = bridge.as_deref().map(net::MasterBridge::state);
    let hosting = matches!(state, Some(net::MasterBridgeState::Hosting { in_match: false, .. }));
    if !hosting || rotation.vote_s == 0 {
        if opened.is_none() {
            rotation.advance();
            commands.insert_resource(rotation.apply());
            *opened = Some((Vec::new(), std::time::Instant::now()));
        }
    } else if opened.as_ref().is_none_or(|(maps, _)| !maps.is_empty()) {
        let Some(pending) = menu.as_mut() else { return };
        if pending.0.is_some() {
            return;
        }
        let vote = state.as_ref().and_then(net::MasterBridgeState::vote);
        match (&*opened, vote) {
            (None, _) => {
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(1, |d| d.as_nanos() as u64);
                let maps = rotation.vote_maps(seed);
                diag::info!(Net, "server: map vote between {maps:?} ({}s)", rotation.vote_s);
                pending.0 = Some(net::MasterMenuAction::OpenVote {
                    maps: maps.clone(),
                    seconds: rotation.vote_s,
                });
                *opened = Some((maps, std::time::Instant::now()));
                return;
            }
            (Some((maps, at)), v) => {
                let (maps, counts) = match v {
                    Some(v) if v.seconds_left() > 0.0 => return,
                    Some(v) => (v.maps.clone(), v.counts.clone()),
                    // (No vote from the master after its time and more:
                    // the first map.)
                    None if at.elapsed().as_secs() > u64::from(rotation.vote_s) + 10 => (maps.clone(), Vec::new()),
                    None => return,
                };
                let seed = at.elapsed().as_nanos() as u64;
                if let Some(won) = net::master_protocol::mapvote::winner(&maps, &counts, seed) {
                    diag::info!(Net, "server: the vote picked `{won}` ({counts:?})");
                    rotation.select(&won);
                }
                commands.insert_resource(rotation.apply());
                *opened = Some((Vec::new(), std::time::Instant::now()));
            }
        }
    }
    let entry = rotation.current().clone();
    match state {
        Some(net::MasterBridgeState::Hosting {
            in_match: false, ..
        }) => {
            let Some(mut menu) = menu else { return };
            if menu.0.is_none() {
                diag::info!(
                    Net,
                    "server: next match `{}` ({}) in the room",
                    entry.zone,
                    rotation.gametype()
                );
                menu.0 = Some(net::MasterMenuAction::StartMatch {
                    map: entry.zone,
                    mode: rotation.gametype(),
                });
                *due = false;
            }
        }
        Some(
            net::MasterBridgeState::Hosting { .. } | net::MasterBridgeState::Connecting { .. },
        ) => {}
        _ => {
            match swap.request_zone(entry.zone.clone()) {
                Ok(id) => diag::info!(Sim, "server: next match `{}` (swap #{id})", entry.zone),
                Err(e) => diag::warn!(Sim, "server: next match `{}` refused: {e}", entry.zone),
            }
            *due = false;
        }
    }
}
