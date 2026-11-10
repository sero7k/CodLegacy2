//! bo2mp shell: the public match's pre-game lobby. After a public match
//! everyone in its room, its host too, waits in the room together while
//! they vote the next map: two from the playlist (never one map in two
//! games' versions, nor the map just played), `VOTE_S` seconds, the most
//! votes win (a tie: a coin toss). BRAINROT 24/7 has no vote: its one map,
//! Shipment, counts down. The host opens the vote and starts the winner;
//! the master counts the votes, so everyone sees the same maps, tally and
//! result. Bots are not in the room and do not vote.

use super::bridge::{Catalog, Request, StartSpec};
use super::Shell;

/// How long the vote runs.
pub(crate) const VOTE_S: u16 = 20;

/// The playlist's maps to vote between, less the one just played.
pub(crate) fn maps(shell: &Shell, c: &Catalog, played: Option<&str>, seed: u64) -> Vec<String> {
    if shell.brainrot {
        return super::matchmaking::shipment(c).map_or_else(Vec::new, |m| vec![m.ui_name.clone()]);
    }
    let pool: Vec<(String, Option<String>)> =
        c.maps.iter().filter(|m| m.group != "CUSTOM MAPS").map(|m| (m.ui_name.clone(), Some(m.title.clone()))).collect();
    net::master_protocol::mapvote::pick(&pool, played, seed)
}

fn seed(now: f64) -> u64 {
    (now * 1000.0) as u64 ^ 0x9E37_79B9_7F4A_7C15
}

/// The room's vote this frame: its maps and tally shown; the host opens
/// it, and when it closes starts its winner.
pub(crate) fn tick(shell: &mut Shell, c: &Catalog, now: f64) {
    let Some(room) = crate::online_match::room() else {
        shell.pregame = Default::default();
        shell.vote_counts.clear();
        return;
    };
    if let Some(v) = &room.vote {
        if shell.vote_maps != v.maps {
            shell.vote_maps = v.maps.clone();
            shell.vote = None;
        }
        shell.vote_counts = v.counts.clone();
    } else {
        shell.vote_counts.clear();
    }
    if !room.host {
        return;
    }
    let (opened, started) = shell.pregame;
    match &room.vote {
        None if !opened => {
            let maps = maps(shell, c, Some(&room.map), seed(now));
            if maps.is_empty() {
                return;
            }
            diag::info!(Ui, "bo2mp shell: pre-game vote between {maps:?}");
            crate::online_match::open_vote(maps, VOTE_S);
            shell.pregame.0 = true;
        }
        Some(v) if !started && v.seconds_left() <= 0.0 && super::bridge::starting_in().is_none() => {
            let Some(won) = net::master_protocol::mapvote::winner(&v.maps, &v.counts, seed(now)) else { return };
            diag::info!(Ui, "bo2mp shell: the vote picked `{won}` ({:?})", v.counts);
            shell.pregame.1 = true;
            shell.map = won;
            shell.mode = room.mode.clone();
            shell.public = true;
            shell.played = true;
            super::bridge::request(Request::Start(StartSpec {
                map: shell.map.clone(),
                gametype: shell.mode.clone(),
                bot_friends: shell.friends,
                bot_enemies: shell.enemies,
                bot_difficulty: shell.difficulty,
                public: true,
                br: None,
            }));
        }
        _ => {}
    }
}

/// The room's vote: (seconds left, how long it runs).
pub(crate) fn countdown() -> Option<(f64, f64)> {
    let v = crate::online_match::room()?.vote?;
    Some((v.seconds_left(), f64::from(VOTE_S)))
}

/// After a public match with no room (no server, or it could not host):
/// the matchmaking lobby again, a new vote and its own match after it.
pub(crate) fn back_from_public(shell: &mut Shell, now: f64) {
    let played = shell.map.clone();
    shell.screen = super::Screen::Lobbies;
    shell.searching = true;
    shell.public = true;
    shell.search_started = now;
    shell.join_at = None;
    shell.vote = None;
    shell.focus = 0;
    shell.vote_maps = super::bridge::catalog().map_or_else(Vec::new, |c| maps(shell, &c, Some(&played), seed(now)));
}
