//! bo2mp online: Black Ops II's front end through the master server.
//!
//! With a server configured (`online.cfg` -> `CODLEGACY_MASTER_ADDR`), START in
//! the lobby does not load the map here: it opens a room on the server
//! (`MasterMenuAction::Host`), then starts its match (`StartMatch`), and
//! everyone in the room, him included, loads the map from the server's
//! offer (`session::follow_master_match`; a joiner's `Bo2mpMatchSetup`
//! comes from the offer there). A friend's lobby is joined from FRIENDS
//! (`MasterMenuAction::Join`); his front end closes when that match begins.
//!
//! When the server cannot host (unreachable, refused, or no room within
//! `HOST_WAIT_S`), the match starts on this PC as before.
//!
//! bo2mp: a public match's room stays together after it: everyone, its
//! host too, waits in the pre-game lobby while they vote the next map
//! (`shell::pregame`).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::prelude::*;
use net::{MasterBridgeState, MasterMenuAction};

/// How long START waits for the server's room before playing here.
const HOST_WAIT_S: f64 = 15.0;

/// A server is configured (set every frame).
static AVAILABLE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
struct Start {
    zone: String,
    mode: String,
    asked: Option<f64>,
}

static START: Mutex<Option<Start>> = Mutex::new(None);
static JOIN: Mutex<Option<MasterMenuAction>> = Mutex::new(None);
/// He left the room he joined (Esc on the waiting room).
static LEAVE: AtomicBool = AtomicBool::new(false);
/// The room he hosts plays public matches (its lobby is the pre-game one).
static PUBLIC_HOST: AtomicBool = AtomicBool::new(false);
/// The map vote's requests to send (open it, his vote).
static VOTES: Mutex<Vec<MasterMenuAction>> = Mutex::new(Vec::new());

/// The room he joined, while he waits for its host to start the match.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct RoomView {
    pub name: String,
    pub map: String,
    pub mode: String,
    pub players: Vec<String>,
    pub max_players: u8,
    /// He hosts it (a public room's host waits in it between matches).
    pub host: bool,
    pub vote: Option<net::RoomVote>,
}

static ROOM: Mutex<Option<RoomView>> = Mutex::new(None);

/// bo2mp: the peer-to-peer custom game he hosts (`net::DirectHost`), as
/// his lobby shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct P2pView {
    pub invite: Option<String>,
    pub port: u16,
    pub opened: Option<&'static str>,
    pub no_public: bool,
    /// His friends in the room (not him).
    pub friends: Vec<String>,
}

static P2P: Mutex<Option<P2pView>> = Mutex::new(None);
/// Why the last peer-to-peer join or game ended, once, for a note.
static P2P_NOTE: Mutex<Option<String>> = Mutex::new(None);
/// A peer-to-peer action to send (open, join).
static P2P_ACTION: Mutex<Option<MasterMenuAction>> = Mutex::new(None);

/// The peer-to-peer custom game he hosts, while it is open.
pub(crate) fn p2p_view() -> Option<P2pView> {
    P2P.lock().ok().and_then(|p| p.clone())
}

/// Open his custom lobby to friends by invite code.
pub(crate) fn p2p_host(map: String, mode: String) {
    if let Ok(mut a) = P2P_ACTION.lock() {
        *a = Some(MasterMenuAction::HostDirect { map, mode });
    }
}

/// Join a friend's peer-to-peer custom game by his code.
pub(crate) fn p2p_join(code: String) {
    if let Ok(mut a) = P2P_ACTION.lock() {
        *a = Some(MasterMenuAction::JoinCode { code });
    }
}

/// His lobby's map or mode changed: his room follows.
pub(crate) fn p2p_update(map: String, mode: String) {
    if let Ok(mut a) = P2P_ACTION.lock() {
        *a = Some(MasterMenuAction::UpdateLobby { map, mode });
    }
}

/// A message about the peer-to-peer game for him, once.
pub(crate) fn take_p2p_note() -> Option<String> {
    P2P_NOTE.lock().ok().and_then(|mut n| n.take())
}

fn p2p_note(text: String) {
    if let Ok(mut n) = P2P_NOTE.lock() {
        *n = Some(text);
    }
}
static ROOM_PREVIEW: Mutex<Option<RoomView>> = Mutex::new(None);

/// The room he waits in (the shell shows it instead of its menus).
pub(crate) fn room() -> Option<RoomView> {
    if let Some(p) = ROOM_PREVIEW.lock().ok().and_then(|p| p.clone()) {
        return Some(p);
    }
    ROOM.lock().ok().and_then(|r| r.clone())
}

pub(crate) fn preview_room(room: Option<RoomView>) {
    if let Ok(mut p) = ROOM_PREVIEW.lock() {
        *p = room;
    }
}

/// Esc in the waiting room: leave it.
pub(crate) fn queue_leave() {
    LEAVE.store(true, Ordering::Relaxed);
}

pub(crate) fn online_available() -> bool {
    AVAILABLE.load(Ordering::Relaxed)
}

/// The host opens the room's map vote.
pub(crate) fn open_vote(maps: Vec<String>, seconds: u16) {
    if let Ok(mut v) = VOTES.lock() {
        v.push(MasterMenuAction::OpenVote { maps, seconds });
    }
}

/// His vote: the `k`th map.
pub(crate) fn cast_vote(k: usize) {
    if let (Ok(mut v), Ok(choice)) = (VOTES.lock(), u8::try_from(k)) {
        v.push(MasterMenuAction::CastVote { choice });
    }
}

/// START in the lobby with a server configured: this match through it.
pub(crate) fn queue_online_start(zone: String, mode: String) {
    PUBLIC_HOST.store(crate::shell::enabled() && crate::shell::bridge::public_start(), Ordering::Relaxed);
    if let Ok(mut s) = START.lock() {
        *s = Some(Start { zone, mode, asked: None });
    }
}

/// FRIENDS: join this lobby.
pub(crate) fn queue_join(advert: &net::master_protocol::Advert) {
    queue_join_room(advert.id, advert.map.clone(), advert.mode.clone());
}

/// FRIENDS / FIND A LOBBY: join this room.
pub(crate) fn queue_join_room(advert_id: net::master_protocol::AdvertId, map: String, mode: String) {
    if let Ok(mut j) = JOIN.lock() {
        *j = Some(MasterMenuAction::Join { advert_id, map, mode });
    }
}

#[allow(clippy::too_many_arguments)]
fn drive_online(
    mut commands: Commands,
    intent: Option<Res<net::MasterLaunchIntent>>,
    pending: Option<ResMut<net::PendingMasterMenuAction>>,
    bridge: Option<Res<net::MasterBridge>>,
    swap: Option<ResMut<session::SessionSwapRequest>>,
    front: Option<Res<crate::lui_frontend::Bo2mpFrontend>>,
    direct: Option<Res<net::DirectHost>>,
    time: Res<Time<Real>>,
) {
    AVAILABLE.store(intent.as_deref().is_some_and(net::MasterLaunchIntent::configured), Ordering::Relaxed);
    let now = time.elapsed_secs_f64();
    let state = bridge.as_deref().map(net::MasterBridge::state);
    let is_direct = intent.as_deref().is_some_and(net::MasterLaunchIntent::is_direct);
    let p2p = direct.as_deref().map(|d| {
        let info = d.info();
        let friends = match &state {
            Some(MasterBridgeState::Hosting { members, member_names, identity, .. }) => members
                .iter()
                .filter(|m| **m != identity.member_id)
                .map(|m| member_names.get(m).cloned().unwrap_or_else(|| "PLAYER".to_owned()))
                .collect(),
            _ => Vec::new(),
        };
        P2pView { invite: info.invite, port: info.port, opened: info.opened, no_public: info.no_public, friends }
    });
    if let Ok(mut v) = P2P.lock()
        && *v != p2p
    {
        *v = p2p;
    }
    // A joiner's front end closes once his match begins (the session loads
    // it from the server's offer).
    if front.is_some() && matches!(state, Some(MasterBridgeState::Joined { in_match: true, .. })) {
        diag::info!(Ui, "bo2mp online: the host started the match; front end closed");
        commands.remove_resource::<crate::lui_frontend::Bo2mpFrontend>();
    }
    let starting = START.lock().is_ok_and(|s| s.is_some());
    let waiting = match &state {
        Some(MasterBridgeState::Joined { name, map, mode, members, member_names, max_players, in_match: false, vote, .. }) => {
            Some(RoomView {
                name: name.clone(),
                map: map.clone(),
                mode: mode.clone(),
                players: members.iter().map(|m| member_names.get(m).cloned().unwrap_or_else(|| "PLAYER".to_owned())).collect(),
                max_players: *max_players,
                host: false,
                vote: vote.clone(),
            })
        }
        Some(MasterBridgeState::Hosting { name, map, mode, members, member_names, max_players, in_match: false, vote, .. })
            if PUBLIC_HOST.load(Ordering::Relaxed) && !starting && front.is_some() =>
        {
            Some(RoomView {
                name: name.clone(),
                map: map.clone(),
                mode: mode.clone(),
                players: members.iter().map(|m| member_names.get(m).cloned().unwrap_or_else(|| "PLAYER".to_owned())).collect(),
                max_players: *max_players,
                host: true,
                vote: vote.clone(),
            })
        }
        _ => None,
    };
    if let Ok(mut r) = ROOM.lock()
        && *r != waiting
    {
        *r = waiting;
    }
    let Some(mut pending) = pending else { return };
    // bo2mp: a peer-to-peer game that failed or ended: say why, and leave it.
    if is_direct && pending.0.is_none() {
        let ended = match &state {
            Some(MasterBridgeState::Failed { error, .. }) if error.operation == "connect" => Some(error.source.clone()),
            Some(MasterBridgeState::Failed { error, .. }) if error.operation == "control_rpc" => {
                Some(format!("The host's game turned you away: {}.", error.source))
            }
            Some(MasterBridgeState::Failed { .. }) => Some("Lost the connection to the host.".to_owned()),
            Some(MasterBridgeState::Closed { .. }) => Some("The host closed his game.".to_owned()),
            _ => None,
        };
        if let Some(why) = ended
            && START.lock().is_ok_and(|s| s.is_none())
        {
            diag::warn!(Ui, "bo2mp online: peer-to-peer game ended: {why}");
            p2p_note(why);
            pending.0 = Some(MasterMenuAction::LeaveLobby);
            return;
        }
    }
    if pending.0.is_none()
        && let Some(action) = P2P_ACTION.lock().ok().and_then(|mut a| a.take())
    {
        diag::info!(Ui, "bo2mp online: {action:?}");
        pending.0 = Some(action);
        return;
    }
    if LEAVE.swap(false, Ordering::Relaxed) {
        diag::info!(Ui, "bo2mp online: left the room");
        PUBLIC_HOST.store(false, Ordering::Relaxed);
        if let Ok(mut v) = VOTES.lock() {
            v.clear();
        }
        pending.0 = Some(MasterMenuAction::LeaveLobby);
        return;
    }
    if pending.0.is_none()
        && let Some(vote) = VOTES.lock().ok().and_then(|mut v| (!v.is_empty()).then(|| v.remove(0)))
    {
        diag::info!(Ui, "bo2mp online: {vote:?}");
        pending.0 = Some(vote);
        return;
    }
    if pending.0.is_none()
        && let Some(join) = JOIN.lock().ok().and_then(|mut j| j.take())
    {
        diag::info!(Ui, "bo2mp online: {join:?}");
        pending.0 = Some(join);
        return;
    }
    let Ok(mut slot) = START.lock() else { return };
    let Some(start) = slot.as_mut() else { return };
    let play_here = |reason: &str, start: &Start, swap: Option<ResMut<session::SessionSwapRequest>>| {
        diag::warn!(Ui, "bo2mp online: {reason}; `{}` starts on this PC", start.zone);
        if let Some(mut swap) = swap
            && let Err(e) = swap.request_zone(start.zone.clone())
        {
            diag::warn!(Ui, "bo2mp online: `{}` not started: {e}", start.zone);
        }
    };
    match state {
        Some(MasterBridgeState::Hosting { in_match: false, .. }) => {
            if pending.0.is_none() {
                diag::info!(Ui, "bo2mp online: room open; starting `{}` ({})", start.zone, start.mode);
                pending.0 = Some(MasterMenuAction::StartMatch { map: start.zone.clone(), mode: start.mode.clone() });
                *slot = None;
            }
        }
        Some(MasterBridgeState::Failed { .. } | MasterBridgeState::Closed { .. }) => {
            play_here("the server could not host", start, swap);
            pending.0 = Some(MasterMenuAction::LeaveLobby);
            *slot = None;
        }
        _ => match start.asked {
            None if bridge.is_none() && pending.0.is_none() => {
                diag::info!(Ui, "bo2mp online: opening a room for `{}` ({})", start.zone, start.mode);
                pending.0 = Some(MasterMenuAction::Host { map: start.zone.clone(), mode: start.mode.clone() });
                start.asked = Some(now);
            }
            Some(at) if now - at > HOST_WAIT_S => {
                play_here("no room from the server in time", start, swap);
                if bridge.is_some() {
                    pending.0 = Some(MasterMenuAction::LeaveLobby);
                }
                *slot = None;
            }
            _ => {}
        },
    }
}

pub(crate) fn register_online_match(app: &mut App) {
    app.add_systems(Update, drive_online.in_set(frame::ClientSet::Ui));
}
