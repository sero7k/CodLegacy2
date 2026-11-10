//! bo2mp shell: FIND A MATCH. Public matches by playlist, as Black Ops II's
//! own filter does it: two tabs, CORE (the standard modes) and PARTY GAMES
//! (Gun Game, One in the Chamber, Sharpshooter, Sticks and Stones); each
//! mode a row to tick, its playlist picture, description and rules (from
//! the game's own `mp/gamesettings_<type>.cfg`) beside it; then SEARCH,
//! which lists the server's open lobbies of the modes ticked (FIND A
//! LOBBY's list, filtered). None found: host one of those modes, so the
//! next player searching finds yours.
//!
//! BRAINROT 24/7 (top of CORE): Shipment, Call of Duty 4's smallest map, and
//! nothing else, in whichever modes are ticked (Team Deathmatch when none).

use bevy::prelude::*;

use super::bridge::{Catalog, ModeEntry};
use super::theme::*;
use super::anim;
use super::ghosts;
use super::view::{Ctx, Kids, abs, header, header_ex, lift};
use super::{Act, Hit, Row, Shell};

/// The filter's tabs: the two playlists, then the server's lobbies.
pub(crate) const TABS: [&str; 3] = ["CORE", "PARTY GAMES", "ACTIVE LOBBIES"];

/// The tab listing the server's lobbies (FIND A LOBBY, as it was).
pub(crate) const ACTIVE_LOBBIES: usize = 2;

/// Black Ops II's party games (its PARTY GAMES playlist).
const PARTY: [&str; 4] = ["gun", "oic", "shrp", "sas"];

pub(crate) fn is_party(key: &str) -> bool {
    PARTY.contains(&key)
}

/// The modes on a tab, in the catalog's order: (index in the catalog, mode).
/// Every mode in mp/gametypestable.csv is on one: PARTY GAMES the ones its
/// playlist group calls party, CORE the rest.
pub(crate) fn modes(c: &Catalog, tab: usize) -> Vec<(usize, &ModeEntry)> {
    c.modes
        .iter()
        .enumerate()
        .filter(|(_, m)| super::bridge::tricky_base(&m.key).is_none() && (m.party || is_party(&m.key)) == (tab == 1))
        .collect()
}

/// Black Ops II's playlist picture for a mode (`playlist_tdm`, ...).
pub(crate) fn picture(key: &str) -> Vec<String> {
    let own = match key {
        "tdm" => "playlist_tdm",
        "dm" => "playlist_ffa",
        "dom" => "playlist_domination",
        "sd" => "playlist_search_destroy",
        "conf" => "playlist_kill_confirm",
        "hq" => "playlist_headquarters",
        "koth" => "playlist_koth",
        "ctf" => "playlist_ctf",
        "dem" => "playlist_demolition",
        "gun" => "playlist_gungame",
        "oic" => "playlist_one_chamber",
        "shrp" => "playlist_sharpshooter",
        "sas" => "playlist_sticks_and_stones",
        "oneflag" => "playlist_single_ctf",
        _ => "",
    };
    [own.to_owned(), format!("playlist_{key}"), if is_party(key) { "playlist_party" } else { "playlist_core" }.to_owned()]
        .into_iter()
        .filter(|n| !n.is_empty())
        .collect()
}

/// Shipment, as the catalog lists it (Call of Duty 4's `mp_shipment`).
pub(crate) fn shipment(c: &Catalog) -> Option<&super::bridge::MapEntry> {
    c.maps.iter().find(|m| is_shipment(&m.ui_name))
}

/// A map name (`iw3:mp_shipment`, `mp_shipment_iw3`) that is Shipment.
pub(crate) fn is_shipment(map: &str) -> bool {
    let stem = map.split(':').next_back().unwrap_or(map).to_ascii_lowercase();
    stem == "mp_shipment" || stem.starts_with("mp_shipment_") && !stem.starts_with("mp_shipment_long")
}

/// The modes ticked, as their titles.
fn ticked_titles(shell: &Shell, c: &Catalog) -> Vec<String> {
    shell.filter.iter().filter_map(|k| c.mode(k).map(|m| m.title.clone())).collect()
}

pub(crate) fn rows(shell: &Shell, c: &Catalog) -> Vec<Row> {
    if shell.page == ACTIVE_LOBBIES {
        return super::lobbies::rows(shell, c);
    }
    let mut out: Vec<Row> = Vec::new();
    if shell.page == 0 {
        out.push(Row {
            label: "BRAINROT 24/7".to_owned(),
            sub: "Shipment. All day, every day.".to_owned(),
            value: Some(if shell.brainrot { "ON".to_owned() } else { "OFF".to_owned() }),
            act: Act::Brainrot,
        });
    }
    out.extend(modes(c, shell.page)
        .into_iter()
        .map(|(i, m)| Row {
            label: m.title.clone(),
            sub: m.desc.clone(),
            value: Some(if shell.filter.contains(&m.key) { "ON".to_owned() } else { "OFF".to_owned() }),
            act: Act::FilterMode(i),
        }));
    let picked = ticked_titles(shell, c);
    let picked: Vec<String> = if shell.brainrot {
        let modes = if picked.is_empty() { vec!["TEAM DEATHMATCH".to_owned()] } else { picked };
        modes.into_iter().map(|m| format!("{m} ON SHIPMENT")).collect()
    } else {
        picked
    };
    out.push(Row {
        label: "SEARCH".to_owned(),
        sub: if picked.is_empty() {
            "Every mode: tick the ones you want to play, or search them all.".to_owned()
        } else {
            format!("Open lobbies playing {}.", picked.join(", "))
        },
        value: None,
        act: Act::SearchMatches,
    });
    out
}

/// Enter on a mode: tick it, or untick it.
pub(crate) fn toggle(shell: &mut Shell, c: &Catalog, i: usize) {
    let Some(m) = c.modes.get(i) else { return };
    if let Some(at) = shell.filter.iter().position(|k| *k == m.key) {
        shell.filter.remove(at);
    } else {
        shell.filter.push(m.key.clone());
    }
}

/// Whether a lobby passes the filter: its mode is one ticked (nothing
/// ticked: every mode; BRAINROT 24/7 with none ticked: Team Deathmatch),
/// and with BRAINROT 24/7 on, its map is Shipment.
pub(crate) fn passes(shell: &Shell, map: &str, mode: &str) -> bool {
    let mode_ok = if shell.filter.is_empty() {
        !shell.brainrot || mode.eq_ignore_ascii_case("tdm")
    } else {
        shell.filter.iter().any(|k| k.eq_ignore_ascii_case(mode))
    };
    mode_ok && (!shell.brainrot || is_shipment(map))
}

/// The mode to host when the search finds nothing: the first ticked, or
/// the lobby's own.
pub(crate) fn host_mode(shell: &Shell) -> String {
    shell.filter.first().cloned().unwrap_or_else(|| if shell.brainrot { "tdm".to_owned() } else { shell.mode.clone() })
}

fn minutes(v: f32) -> String {
    if v <= 0.0 {
        "UNLIMITED".to_owned()
    } else if v.fract() == 0.0 {
        format!("{v:.0} MIN")
    } else {
        format!("{v:.1} MIN")
    }
}

fn count(v: f32) -> String {
    if v <= 0.0 { "UNLIMITED".to_owned() } else { format!("{v:.0}") }
}

/// A fact under the playlist in focus: its name small, the value under it.
fn fact(p: &mut Kids, cx: &Ctx, label: &str, value: String) {
    p.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() }).with_children(|f| {
        f.spawn((Text::new(label.to_owned()), cx.bold(16.0), TextColor(FAINT)));
        f.spawn((Text::new(value), cx.bold(22.0), TextColor(WHITE)));
    });
}

/// Whether the playlist in focus is ticked: SELECTED, or how to tick it.
fn selected(p: &mut Kids, cx: &Ctx, on: bool) {
    p.spawn((
        Node { padding: UiRect::axes(cx.px(12.0), cx.px(4.0)), border: UiRect::left(cx.px(3.0)), margin: UiRect::top(cx.px(6.0)), ..default() },
        BackgroundColor(if on { cx.pal.accent_soft.with_alpha(0.18) } else { Color::NONE }),
        BorderColor::all(if on { GOLD } else { cx.pal.line }),
    ))
    .with_child((Text::new(if on { "SELECTED" } else { "PRESS ENTER TO SELECT" }), cx.bold(18.0), TextColor(if on { WHITE } else { DIM })));
}

/// FIND A MATCH, as Ghosts' FIND GAME / FIND MATCH panel: a dark panel from
/// the screen's left edge; in it the playlists (CORE, PARTY GAMES, ACTIVE
/// LOBBIES: Q / E or a click) as a plain list, the tab's modes (or lobbies)
/// as a second, and in the middle the one in focus (its mark, name, what it
/// is, its rules), the players in the playlists and online at its foot.
pub(crate) fn view(c: &mut Kids, cx: &mut Ctx, shell: &Shell, cat: &Catalog, rows: &[Row], online: &super::online::OnlineInfo) {
    let tab = shell.page.min(TABS.len() - 1);
    ghosts::popup_ground(c, cx, 1060.0);
    header(c, cx, &["FIND A MATCH", if tab == ACTIVE_LOBBIES { "ACTIVE LOBBIES" } else { "FIND MATCH" }]);
    // The playlists.
    c.spawn((Node { width: cx.px(230.0), flex_direction: FlexDirection::Column, row_gap: cx.px(2.0), ..abs(Some(40.0), Some(196.0), None, None, cx) }, anim::from_left(cx.s, 60.0, 0.0))).with_children(|l| {
        l.spawn((Text::new("PLAYLISTS"), cx.bold(18.0), TextColor(FAINT), Node { margin: UiRect::bottom(cx.px(6.0)), ..default() }));
        for (i, t) in TABS.iter().enumerate() {
            let on = i == tab;
            l.spawn((
                Hit::Page(i),
                Button,
                Node { height: cx.px(34.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                BackgroundColor(if on { lift(cx.pal.accent, 0.10).with_alpha(0.55) } else { WHITE.with_alpha(0.03) }),
            ))
            .with_child((Text::new(*t), if on { cx.bold(22.0) } else { cx.regular(22.0) }, TextColor(if on { WHITE } else { TEXT.with_alpha(0.85) })));
        }
    });
    // The tab's modes (how many are ticked over them: Black Ops II's
    // "Selected filter"), or the server's lobbies.
    let picked = shell.filter.len();
    let heading = if tab == ACTIVE_LOBBIES {
        "LOBBIES".to_owned()
    } else {
        match (picked, shell.brainrot) {
            (0, false) => "ALL MODES".to_owned(),
            (0, true) => "BRAINROT 24/7".to_owned(),
            (n, false) => format!("{n} SELECTED"),
            (n, true) => format!("BRAINROT 24/7  ·  {n} SELECTED"),
        }
    };
    c.spawn(Node { width: cx.px(380.0), flex_direction: FlexDirection::Column, ..abs(Some(300.0), Some(196.0), None, None, cx) }).with_children(|l| {
        l.spawn((Text::new(heading), cx.bold(18.0), TextColor(if tab == ACTIVE_LOBBIES { FAINT } else { GOLD }), Node { margin: UiRect::bottom(cx.px(6.0)), ..default() }));
        ghosts::plain_list(l, cx, rows, shell.focus, 380.0, if rows.len() > 18 { 28.0 } else { 34.0 });
    });
    // The one in focus.
    let focused = rows.get(shell.focus).map(|r| r.act);
    c.spawn((
        Node { width: cx.px(330.0), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: cx.px(8.0), ..abs(Some(710.0), Some(196.0), None, None, cx) },
        anim::from_left(cx.s, 40.0, 0.06),
    ))
    .with_children(|p| match focused {
        Some(Act::Lobby(_)) | Some(Act::Info) | Some(Act::HostMatch) if tab == ACTIVE_LOBBIES => super::lobbies::details(p, cx, shell, rows),
        Some(Act::Brainrot) => {
            let map = shipment(cat);
            if let Some(m) = map
                && let Some(pic) = cx.pic(&m.pictures)
            {
                let size = cx.pic_size(&pic);
                let w = 320.0;
                p.spawn((ImageNode::new(pic), Node { width: cx.px(w), height: cx.px(w * size.y / size.x.max(1.0)), ..default() }));
            }
            p.spawn((Text::new("BRAINROT 24/7"), cx.bold(36.0), TextColor(WHITE)));
            p.spawn((Text::new("Shipment. All day, every day. Team Deathmatch, or the modes you tick."), cx.regular(21.0), TextColor(TEXT), TextLayout::justify(bevy::text::Justify::Center)));
            fact(p, cx, "MAP", map.map_or_else(|| "SHIPMENT (CALL OF DUTY 4 NOT FOUND)".to_owned(), |m| m.title.to_uppercase()));
            fact(p, cx, "MODES", if shell.filter.is_empty() { "TEAM DEATHMATCH".to_owned() } else { format!("{} TICKED", shell.filter.len()) });
            selected(p, cx, shell.brainrot);
        }
        Some(Act::FilterMode(i)) if cat.modes.get(i).is_some() => {
            let m = &cat.modes[i];
            let mut names = vec![m.picture.clone()];
            names.extend(picture(&m.key));
            names.retain(|n| !n.is_empty());
            if let Some(pic) = cx.pic(&names) {
                p.spawn((ImageNode::new(pic), Node { width: cx.px(150.0), height: cx.px(150.0), flex_shrink: 0.0, ..default() }));
            }
            p.spawn((Text::new(m.title.clone()), cx.bold(36.0), TextColor(WHITE), TextLayout::justify(bevy::text::Justify::Center)));
            p.spawn((Text::new(m.desc.clone()), cx.regular(21.0), TextColor(TEXT), TextLayout::justify(bevy::text::Justify::Center)));
            let r = m.rules;
            if let Some(t) = r.time_limit {
                fact(p, cx, "MATCH TIME", minutes(t));
            }
            if let Some(v) = r.score_limit {
                fact(p, cx, "SCORE LIMIT", count(v));
            }
            if let Some(n) = r.round_limit.filter(|n| *n > 1.0) {
                fact(p, cx, "ROUNDS", count(n));
            }
            if let Some(n) = r.lives.filter(|n| *n > 0.0) {
                fact(p, cx, "LIVES", count(n));
            }
            selected(p, cx, shell.filter.contains(&m.key));
        }
        _ => {
            p.spawn((Text::new("SEARCH"), cx.bold(36.0), TextColor(WHITE)));
            let picked = ticked_titles(shell, cat);
            p.spawn((
                Text::new(if picked.is_empty() { "Every mode, Core and Party Games.".to_owned() } else { picked.join("  ·  ") }),
                cx.regular(22.0),
                TextColor(TEXT),
                TextLayout::justify(bevy::text::Justify::Center),
            ));
            p.spawn((
                Text::new("Finds open lobbies on the server playing these modes. None open: you host one, and the next player who searches finds yours."),
                cx.regular(20.0),
                TextColor(DIM),
                TextLayout::justify(bevy::text::Justify::Center),
            ));
        }
    });
    // The players: in the playlists (the lobbies on the server), online.
    let in_lists: u32 = super::lobbies::all().iter().map(|a| u32::from(a.players)).sum();
    let online_line = match online.status {
        super::online::Status::Online => online.players.to_string(),
        super::online::Status::Connecting => "CONNECTING".to_owned(),
        super::online::Status::Unreachable => "SERVER UNREACHABLE".to_owned(),
        super::online::Status::Offline => "OFFLINE".to_owned(),
    };
    c.spawn(Node { width: cx.px(330.0), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: cx.px(2.0), ..abs(Some(710.0), None, None, Some(140.0), cx) }).with_children(|t| {
        t.spawn((Text::new("Players in Playlists:"), cx.bold(20.0), TextColor(TEXT)));
        t.spawn((Text::new(in_lists.to_string()), cx.regular(20.0), TextColor(DIM), Node { margin: UiRect::bottom(cx.px(8.0)), ..default() }));
        t.spawn((Text::new("Players Online:"), cx.bold(20.0), TextColor(TEXT)));
        t.spawn((Text::new(online_line), cx.regular(20.0), TextColor(DIM)));
    });
}

// ---------------------------------------------------------------------------
// The search, as a matchmaking lobby (Call of Duty: Ghosts' online flow, in
// this shell's look): the playlist's name; at the top right what it is doing
// ("SEARCHING FOR GAMES", "2 POTENTIAL GAMES FOUND", "TRYING TO JOIN GAME
// 1/2", then in the room how many places are left); the players on the
// right filling its places; the menu and the playlist (then the map) card on
// the left. It joins the fullest open lobby that fits, the next one when a
// join does not take; none: it keeps looking (the list refreshes every 15
// seconds) and HOST A MATCH is there.

/// Seconds "SEARCHING FOR GAMES" shows before what was found, then before
/// the first join; how long a join may take before the next game is tried.
const LOOK_S: f64 = 2.0;
const FOUND_S: f64 = 1.5;
const JOIN_WAIT_S: f64 = 6.0;

/// The open lobbies that fit, fullest first.
pub(crate) fn candidates(shell: &Shell) -> Vec<net::MasterAdvert> {
    let mut list: Vec<net::MasterAdvert> = super::lobbies::all()
        .into_iter()
        .filter(|a| super::lobbies::joinable(a) && passes(shell, &a.map, &a.mode))
        .collect();
    list.sort_by_key(|a| std::cmp::Reverse(a.players));
    list
}

/// How long the search looks for a game before it starts its own on the
/// voted map (with the server: a room others searching can join; without
/// one: a match with bots here).
pub(crate) const OWN_MATCH_S: f64 = 25.0;

/// The map the lobby plays: his vote, else the first of the two.
pub(crate) fn voted_map(shell: &Shell) -> Option<String> {
    shell.vote.and_then(|k| shell.vote_maps.get(k)).or(shell.vote_maps.first()).cloned()
}

/// Two maps to vote between (`super::pregame::maps`: never one map in two
/// games' versions; Shipment alone for BRAINROT 24/7).
pub(crate) fn vote_maps(shell: &Shell, c: &Catalog, now: f64) -> Vec<String> {
    super::pregame::maps(shell, c, None, (now * 1000.0) as u64 ^ 0x9E37_79B9_7F4A_7C15)
}

/// The search's step this frame: join the next game when it is time; none
/// found by `OWN_MATCH_S`: start its own on the voted map.
pub(crate) fn tick(shell: &mut Shell, now: f64) {
    if shell.screen != super::Screen::Lobbies || !shell.searching || crate::online_match::room().is_some() {
        return;
    }
    let found = candidates(shell);
    let since = now - shell.search_started;
    if found.is_empty() {
        if since >= OWN_MATCH_S
            && super::bridge::starting_in().is_none()
            && let Some(map) = voted_map(shell)
        {
            diag::info!(Ui, "bo2mp shell: no game found in {OWN_MATCH_S:.0}s; starting our own on `{map}`");
            shell.map = map;
            shell.mode = host_mode(shell);
            shell.searching = false;
            shell.played = true;
            super::bridge::request(super::bridge::Request::Start(super::bridge::StartSpec {
                map: shell.map.clone(),
                gametype: shell.mode.clone(),
                bot_friends: shell.friends,
                bot_enemies: shell.enemies,
                bot_difficulty: shell.difficulty,
                public: true,
                br: None,
            }));
        }
        return;
    }
    if since < LOOK_S + FOUND_S {
        return;
    }
    let due = shell.join_at.is_none_or(|(_, at)| now - at > JOIN_WAIT_S);
    if due {
        let attempt = shell.join_at.map_or(0, |(k, _)| k + 1);
        if let Some(a) = found.get(attempt % found.len()) {
            crate::online_match::queue_join_room(a.id, a.map.clone(), a.mode.clone());
        }
        shell.join_at = Some((attempt, now));
    }
}

/// The lobby's countdown to its match, if one runs: (seconds left, how
/// long it was). A Custom Game's START MATCH countdown, the public lobby's
/// between matches, or, while searching with no game found, the time until
/// it starts its own.
pub(crate) fn countdown(shell: &Shell) -> Option<(f64, f64)> {
    if let Some(left) = super::pregame::countdown() {
        return Some(left);
    }
    if let Some(ms) = super::bridge::starting_in() {
        return Some((ms / 1000.0, (super::bridge::starting_total() / 1000.0).max(ms / 1000.0)));
    }
    let searching = shell.screen == super::Screen::Lobbies && shell.searching && crate::online_match::room().is_none();
    let since = shell.now - shell.search_started;
    (searching && candidates(shell).is_empty() && since >= LOOK_S).then(|| ((OWN_MATCH_S - since).max(0.0), OWN_MATCH_S - LOOK_S))
}

/// The countdown's tick: once a second through its last ten (Ghosts'
/// `match_countdown_tick` when Ghosts is installed: `uin_start_count_down`
/// is routed to it).
pub(crate) fn countdown_tick(shell: &Shell) -> Option<&'static str> {
    static LAST: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(-1);
    let left = countdown(shell).map_or(-1, |(s, _)| s.ceil() as i64);
    let before = LAST.swap(left, std::sync::atomic::Ordering::Relaxed);
    (left != before && (1..=10).contains(&left)).then_some("uin_start_count_down")
}

/// The search's status line, and a line under it.
pub(crate) fn search_status(shell: &Shell, now: f64) -> (String, String) {
    let found = candidates(shell);
    let since = now - shell.search_started;
    let dots = ".".repeat(1 + ((now * 2.0) as usize % 3));
    let left = (OWN_MATCH_S - since).ceil().max(0.0);
    if !crate::online_match::online_available() && !super::lobbies::previewing() {
        return (format!("OFFLINE  ·  MATCH WITH BOTS IN {left:.0}"), "No server set: fill in online.cfg next to run-bo2.cmd (docs\\ONLINE.md).".to_owned());
    }
    if found.is_empty() || since < LOOK_S {
        let sub = if since < LOOK_S { String::new() } else { format!("No open games in this playlist yet: your own starts in {left:.0} on the voted map.") };
        return (format!("SEARCHING FOR GAMES{dots}"), sub);
    }
    if since < LOOK_S + FOUND_S {
        let n = found.len();
        return (format!("{n} POTENTIAL GAME{} FOUND", if n == 1 { "" } else { "S" }), String::new());
    }
    let k = shell.join_at.map_or(0, |(k, _)| k % found.len()) + 1;
    (format!("TRYING TO JOIN GAME {k}/{}{dots}", found.len()), String::new())
}

/// The playlist's name: BRAINROT 24/7, the one mode ticked, or a count.
pub(crate) fn playlist_title(shell: &Shell, c: &Catalog) -> String {
    let picked = ticked_titles(shell, c);
    match (shell.brainrot, picked.len()) {
        (true, _) => "BRAINROT 24/7".to_owned(),
        (false, 0) => "ALL MODES".to_owned(),
        (false, 1) => picked[0].clone(),
        (false, n) => format!("{n} PLAYLISTS"),
    }
}

/// The left column's rows in the matchmaking lobby (Ghosts' lobby menu):
/// the menus he can open while he waits, the lobby's leaderboard, then
/// (searching) the two maps to vote between.
pub(crate) fn lobby_rows(shell: &Shell, c: &Catalog, searching: bool) -> Vec<Row> {
    let mut out = Vec::new();
    for (label, sub, act) in [
        ("CREATE A CLASS", "Build your loadouts while you wait.", Act::Classes),
        ("BARRACKS", "Your combat record, calling card, emblem and prestige.", Act::Barracks),
        ("LOBBY LEADERBOARD", "The players in this lobby, by rank.", Act::Leaderboard),
    ] {
        out.push(Row { label: label.to_owned(), sub: sub.to_owned(), value: None, act });
    }
    if searching {
        for (k, map) in shell.vote_maps.iter().enumerate() {
            let title = c.maps.iter().find(|m| m.ui_name == *map).map_or_else(|| map.to_uppercase(), |m| m.title.clone());
            out.push(Row { label: title, sub: "Vote for this map.".to_owned(), value: None, act: Act::Vote(k) });
        }
    }
    out
}

/// One place in the player list: (level, name, his own, the host).
pub(crate) struct Place {
    pub level: Option<i32>,
    pub name: String,
    pub me: bool,
    pub host: bool,
}

/// The matchmaking lobby, as Ghosts' lobby: the playlist (or mode) as the
/// title with its mark; at the top right how many players it still needs
/// (and the line about what it is doing); the menus as rows from the left
/// edge, the maps to vote between as cards under them (or the playlist's
/// / map's card); the places at the right, under what it is doing, over a
/// faint grid.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lobby_view(
    c: &mut Kids,
    cx: &mut Ctx,
    shell: &Shell,
    rows: &[Row],
    title: &str,
    status: (&str, &str),
    card: (&[String], &str, &str),
    places: &[Place],
    max: usize,
    top: Option<&str>,
) {
    let mark = super::bridge::catalog()
        .and_then(|c| c.modes.iter().find(|m| m.title == title || m.title == card.2).map(|m| m.picture.clone()))
        .and_then(|p| cx.pic(&[p]));
    header_ex(c, cx, &["PUBLIC MATCH", title], mark, false);
    let need = max.saturating_sub(places.len());
    let line = if need == 0 { "The lobby is full".to_owned() } else { format!("We need to find {need} more player{}", if need == 1 { "" } else { "s" }) };
    ghosts::top_status(c, cx, top.unwrap_or(&line), false, status.1);
    // The left: the menus, then the maps to vote between.
    let menu: Vec<Row> = rows.iter().filter(|r| !matches!(r.act, Act::Vote(_))).cloned().collect();
    let votes: Vec<(usize, &Row)> = rows.iter().enumerate().filter(|(_, r)| matches!(r.act, Act::Vote(_))).collect();
    c.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(0.0), Some(176.0), None, None, cx) }).with_children(|col| {
        ghosts::rows(col, cx, &menu, if shell.focus < menu.len() { shell.focus } else { usize::MAX }, ghosts::Look::list(760.0));
    });
    c.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(0.0), None, None, Some(124.0), cx) }).with_children(|col| {
        if votes.is_empty() {
            ghosts::vote_card(col, cx, ghosts::Card { pictures: card.0, name: card.1, top: (card.2, status.0), on: false, hit: None, w: 760.0, pic_h: 150.0, delay: 0.08 });
            return;
        }
        for (k, (i, r)) in votes.iter().enumerate() {
            let on = shell.focus == *i;
            let mine = shell.vote == Some(k);
            let pics: Vec<String> = cat_pictures(shell, k);
            let n = shell.vote_counts.get(k).map_or(usize::from(mine), |&n| usize::from(n));
            let votes = format!("{n} vote{}{}", if n == 1 { "" } else { "s" }, if mine { "  ·  YOURS" } else { "" });
            ghosts::vote_card(col, cx, ghosts::Card { pictures: &pics, name: &r.label, top: (card.2, &votes), on, hit: Some(Hit::Row(*i)), w: 760.0, pic_h: 120.0, delay: 0.08 + k as f32 * 0.05 });
        }
    });
    // The right: the places (by rank on the leaderboard), under what it is
    // doing (pulsing while it searches).
    let mut sorted: Vec<&Place> = places.iter().collect();
    if shell.leaderboard {
        sorted.sort_by_key(|p| std::cmp::Reverse(p.level.unwrap_or(0)));
    }
    let players: Vec<super::view::LobbyPlayer> = sorted
        .iter()
        .map(|p| super::view::LobbyPlayer {
            level: p.level,
            prestige: 0,
            name: p.name.clone(),
            me: p.me,
            host: p.host,
            team: None,
            card: if p.me {
                super::bridge::catalog().map_or_else(String::new, |c| super::cards::mine(super::cards::Kind::Card, &c.player.card))
            } else {
                super::cards::for_name(super::cards::Kind::Card, &p.name)
            },
        })
        .collect();
    let head = if shell.leaderboard { format!("LOBBY LEADERBOARD  ·  {}", status.0) } else { status.0.to_owned() };
    ghosts::grid(c, cx, 176.0, 860.0, 720.0);
    ghosts::slot_list(c, cx, &head, shell.searching, &players, max, 176.0, if max > 12 { 28.0 } else { 38.0 });
}

/// The picture names of vote map `k` (from the catalog).
fn cat_pictures(shell: &Shell, k: usize) -> Vec<String> {
    let Some(map) = shell.vote_maps.get(k) else { return Vec::new() };
    super::bridge::catalog().and_then(|c| c.maps.iter().find(|m| m.ui_name == *map).map(|m| m.pictures.clone())).unwrap_or_default()
}

/// FIND A MATCH while it searches (the Lobbies screen in a search).
pub(crate) fn search_view(c: &mut Kids, cx: &mut Ctx, shell: &Shell, cat: &Catalog, rows: &[Row]) {
    let (status, sub) = search_status(shell, shell.now);
    let title = playlist_title(shell, cat);
    let mut pics: Vec<String> = Vec::new();
    if shell.brainrot
        && let Some(m) = shipment(cat)
    {
        pics.extend(m.pictures.iter().cloned());
    }
    if let Some(k) = shell.filter.first()
        && let Some(m) = cat.mode(k)
    {
        pics.push(m.picture.clone());
        pics.extend(picture(k));
    }
    pics.push("playlist_core".to_owned());
    pics.retain(|p| !p.is_empty());
    let modes = if shell.filter.is_empty() && shell.brainrot {
        "TEAM DEATHMATCH ON SHIPMENT".to_owned()
    } else if shell.filter.is_empty() {
        "EVERY MODE".to_owned()
    } else {
        ticked_titles(shell, cat).join("  ·  ")
    };
    let modes = if modes == title { "PUBLIC MATCH".to_owned() } else { modes };
    let me = Place { level: Some(cat.player.level), name: cat.player.name.clone(), me: true, host: false };
    lobby_view(c, cx, shell, rows, &title, (&status, &sub), (&pics, &title, &modes), &[me], 12, None);
}
