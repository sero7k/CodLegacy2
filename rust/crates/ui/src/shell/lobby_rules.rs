//! bo2mp: the Custom Game lobby's bot rules (owner's, 2026-10-25).
//!
//! - The bots start at none (friendly and enemy) when he opens Custom Game.
//! - A mode without teams (free-for-all, the party modes) has no friendly
//!   bots: switching to one sets them to 0 (the enemy count is the
//!   free-for-all's bots); switching back to a team mode does not bring
//!   them back.
//! - 18 players at most: his lobby's real players (him included) plus the
//!   friendly and enemy bots. A player joining past it trims the bots
//!   (enemies first, then friendly); the bot rows stop at the cap, and the
//!   match's start clamps to it too.

/// Players in a match at most (bots included); `sv_maxclients` is 18.
pub(crate) const MAX_PLAYERS: u32 = 18;

/// The modes without teams (as the match HUDs know them), by key.
pub(crate) const NO_TEAMS: [&str; 5] = ["dm", "gun", "oic", "sas", "shrp"];

/// Whether `mode` (a mode key; a Tricky mode by the mode it plays) has teams.
pub(crate) fn has_teams(mode: &str) -> bool {
    let base = super::bridge::tricky_base(mode).or_else(|| super::bridge::jiggy_base(mode)).unwrap_or(mode);
    !NO_TEAMS.iter().any(|k| k.eq_ignore_ascii_case(base))
}

/// The real players in his Custom Game lobby: him, and his friends in it.
pub(crate) fn humans() -> u32 {
    let friends = crate::online_match::p2p_view().map_or(0, |v| v.friends.len());
    1 + friends as u32
}

/// Bots the lobby has room for with `humans` real players in it.
pub(crate) fn bot_room(humans: u32) -> u32 {
    MAX_PLAYERS.saturating_sub(humans.max(1))
}

/// The bot counts the rules allow: no friendly bots without teams, and
/// over the cap the enemies go first, then the friendly bots.
pub(crate) fn fit(friends: u32, enemies: u32, humans: u32, teams: bool) -> (u32, u32) {
    let mut f = if teams { friends } else { 0 };
    let mut e = enemies;
    let room = bot_room(humans);
    let over = (f + e).saturating_sub(room);
    let cut = over.min(e);
    e -= cut;
    f -= (over - cut).min(f);
    (f, e)
}

/// The most this bot row can hold, the other row's count as it is.
pub(crate) fn row_max(other: u32, humans: u32, teams: bool, friendly: bool) -> u32 {
    if friendly && !teams { 0 } else { bot_room(humans).saturating_sub(other) }
}

/// A bot row stepped by `by`: never under 0 nor over its live maximum.
/// `None` when it would have gone past (the row stays as it is).
pub(crate) fn step(v: u32, by: i32, max: u32) -> Option<u32> {
    let next = v as i64 + i64::from(by);
    if next < 0 || next > i64::from(max) {
        return None;
    }
    Some(next as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_with_and_without_teams() {
        assert!(has_teams("tdm") && has_teams("sd") && has_teams("dom") && has_teams("tricky_sd"));
        assert!(!has_teams("dm") && !has_teams("gun") && !has_teams("oic") && !has_teams("sas") && !has_teams("shrp") && !has_teams("tricky_dm") && !has_teams("jiggy_dm"));
    }

    #[test]
    fn room_for_bots_is_eighteen_less_the_players() {
        assert_eq!(bot_room(1), 17);
        assert_eq!(bot_room(2), 16);
        assert_eq!(bot_room(6), 12);
        assert_eq!(bot_room(18), 0);
        assert_eq!(bot_room(20), 0);
    }

    #[test]
    fn no_friendly_bots_without_teams() {
        assert_eq!(fit(5, 6, 1, false), (0, 6));
        // (Back to a team mode: they stay at 0.)
        let (f, e) = fit(5, 6, 1, false);
        assert_eq!(fit(f, e, 1, true), (0, 6));
    }

    #[test]
    fn a_joining_player_trims_enemies_then_friendly_bots() {
        // 1 + 8 + 9 = 18: full.
        assert_eq!(fit(8, 9, 1, true), (8, 9));
        // A second player: one enemy goes.
        assert_eq!(fit(8, 9, 2, true), (8, 8));
        // Six players: 12 bots; then with every enemy gone, friendly ones.
        assert_eq!(fit(8, 9, 6, true), (8, 4));
        assert_eq!(fit(10, 2, 8, true), (10, 0));
        assert_eq!(fit(12, 2, 8, true), (10, 0));
        assert_eq!(fit(3, 3, 18, true), (0, 0));
    }

    #[test]
    fn rows_stop_at_the_cap() {
        // Two players, 10 enemy bots: 6 friendly at most.
        assert_eq!(row_max(10, 2, true, true), 6);
        assert_eq!(step(6, 1, 6), None);
        assert_eq!(step(5, 1, 6), Some(6));
        assert_eq!(step(0, -1, 6), None);
        // Free-for-all: no friendly bots at all, 17 opponents alone.
        assert_eq!(row_max(4, 1, false, true), 0);
        assert_eq!(row_max(0, 1, false, false), 17);
    }

    fn catalog() -> super::super::bridge::Catalog {
        let mode = |k: &str| super::super::bridge::ModeEntry {
            key: k.to_owned(),
            title: k.to_uppercase(),
            desc: String::new(),
            rules: super::super::bridge::ModeRules::default(),
            party: false,
            picture: String::new(),
        };
        super::super::bridge::Catalog { modes: vec![mode("tdm"), mode("dm"), mode("dom")], ..Default::default() }
    }

    #[test]
    fn custom_game_starts_with_no_bots() {
        let s = super::super::Shell::default();
        assert_eq!((s.friends, s.enemies), (0, 0));
    }

    #[test]
    fn switching_to_free_for_all_drops_friendly_bots_for_good() {
        use super::super::{Setting, Shell};
        let c = catalog();
        let mut s = Shell { friends: 5, enemies: 6, ..Shell::default() };
        s.cycle(Setting::Mode, 1, &c); // tdm -> dm
        assert_eq!(s.mode, "dm");
        assert_eq!((s.friends, s.enemies), (0, 6));
        s.cycle(Setting::Friends, 1, &c);
        assert_eq!(s.friends, 0);
        s.cycle(Setting::Mode, 1, &c); // dm -> dom
        assert_eq!((s.friends, s.enemies), (0, 6));
    }

    #[test]
    fn bot_rows_stop_at_the_cap() {
        use super::super::Shell;
        let mut s = Shell { friends: 8, enemies: 9, ..Shell::default() };
        // Two real players: 16 bots, enemies trimmed first.
        assert!(s.fit_bots(2));
        assert_eq!((s.friends, s.enemies), (8, 8));
        s.step_bots(true, 1, 2);
        s.step_bots(false, 1, 2);
        assert_eq!((s.friends, s.enemies), (8, 8));
        s.step_bots(false, -1, 2);
        s.step_bots(true, 1, 2);
        assert_eq!((s.friends, s.enemies), (9, 7));
        // Six real players: 12.
        s.fit_bots(6);
        assert_eq!((s.friends, s.enemies), (9, 3));
        assert!(s.friends + s.enemies + 6 <= MAX_PLAYERS);
    }
}
