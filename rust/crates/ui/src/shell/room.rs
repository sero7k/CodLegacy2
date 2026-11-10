//! bo2mp shell: the waiting room. After he joins a lobby on the server
//! (FRIENDS or FIND A LOBBY) the shell shows its map, mode and players until
//! its host starts the match; Esc leaves it. Between a public room's
//! matches it is the pre-game lobby, with the next map's vote
//! (`super::pregame`).

use super::bridge::Catalog;
use super::view::{Ctx, Kids};
use super::{Row, Shell};
use crate::online_match::RoomView;

/// A map's name from its zone name (`t6:mp_nuketown_2020` -> `NUKETOWN 2020`).
fn map_title(map: &str) -> String {
    map.split(':').next_back().unwrap_or(map).trim_start_matches("mp_").replace('_', " ").to_uppercase()
}

pub(crate) fn rows(shell: &Shell, c: &Catalog) -> Vec<Row> {
    let voting = crate::online_match::room().and_then(|r| r.vote).is_some_and(|v| v.maps.len() > 1);
    super::matchmaking::lobby_rows(shell, c, voting)
}

/// The waiting room, as FIND A MATCH's matchmaking lobby: the mode, how many
/// places are left until the host starts, the players, the map.
pub(crate) fn view(c: &mut Kids, cx: &mut Ctx, shell: &Shell, cat: &Catalog, rows: &[Row]) {
    let room = crate::online_match::room().unwrap_or_else(RoomView::default);
    let mode = cat.mode(&room.mode).map_or_else(|| room.mode.to_uppercase(), |m| m.title.clone());
    // (A vote of one map: that map, counting down.)
    let next = room.vote.as_ref().filter(|v| v.maps.len() == 1).map(|v| v.maps[0].clone());
    let shown = next.as_deref().unwrap_or(&room.map);
    let stem = shown.split(':').next_back().unwrap_or(shown);
    let map = cat.map(stem).or_else(|| cat.maps.iter().find(|m| m.ui_name.starts_with(stem)));
    let pictures = map.map(|m| m.pictures.clone()).unwrap_or_default();
    let map_name = map.map_or_else(|| map_title(shown), |m| m.title.to_uppercase());
    let max = usize::from(room.max_players).max(room.players.len()).clamp(2, 18);
    let left = max.saturating_sub(room.players.len());
    let status = match &room.vote {
        Some(v) => {
            let s = v.seconds_left().ceil();
            if v.maps.len() > 1 { format!("VOTE FOR THE NEXT MAP: {s:.0}") } else { format!("NEXT MATCH IN {s:.0}") }
        }
        None if room.host => "PRE-GAME LOBBY".to_owned(),
        None if left == 0 => "LOBBY FULL: WAITING FOR THE HOST".to_owned(),
        None => format!("WAITING FOR {left} MORE PLAYER{}", if left == 1 { "" } else { "S" }),
    };
    let places: Vec<super::matchmaking::Place> = room
        .players
        .iter()
        .enumerate()
        .map(|(i, p)| super::matchmaking::Place {
            level: p.eq_ignore_ascii_case(&cat.player.name).then_some(cat.player.level),
            name: p.clone(),
            me: p.eq_ignore_ascii_case(&cat.player.name),
            host: i == 0,
        })
        .collect();
    let sub = if room.vote.as_ref().is_some_and(|v| v.maps.len() > 1) {
        format!("{} · the most votes win; a tie is a coin toss. Esc leaves.", room.name)
    } else if room.vote.is_some() || room.host {
        format!("{} · everyone plays the next match together. Esc leaves.", room.name)
    } else {
        format!("{} · the host starts the match. Esc leaves.", room.name)
    };
    // The status line at the top right, as the lobby's: WAITING until the
    // host starts, then the vote / the countdown.
    let top = if room.vote.is_some() { status.clone() } else { "WAITING".to_owned() };
    super::matchmaking::lobby_view(c, cx, shell, rows, &mode, (&status, &sub), (&pictures, &map_name, &mode), &places, max, Some(&top));
}

#[cfg(test)]
mod tests {
    #[test]
    fn map_titles() {
        assert_eq!(super::map_title("iw3:mp_crash"), "CRASH");
    }
}
