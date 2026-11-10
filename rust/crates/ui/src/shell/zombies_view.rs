//! bo2mp shell: the Zombies screens, drawn with the Multiplayer screens'
//! parts (header, rows, tabs, map tiles, panels) in the Zombies palette
//! (`theme::palette`): the lobby, the map picker and the start picker.

use bevy::prelude::*;

use super::theme::*;
use super::bridge::PlayerCard;
use super::cards::{self, Kind};
use super::anim;
use super::ghosts;
use super::view::{Ctx, Kids, emblem, grid_more, grid_window, header, header_ex, lift, short_section, tabs};
use super::zm_record::{self, Record};
use super::zombies::{self, all, ZmKind, ZmState, chosen, game_line, pages, start_name};
use super::{Hit, Row, Screen, Shell};

pub(crate) fn view(c: &mut Kids, cx: &mut Ctx, shell: &Shell, rows: &[Row]) {
    let st = zombies::state();
    match shell.screen {
        Screen::Zombies => menu(c, cx, shell, rows),
        Screen::ZmRecord => record(c, cx),
        Screen::ZmMapPick => map_pick(c, cx, shell, &st),
        Screen::ZmStartPick => start_pick(c, cx, shell, rows),
        _ => lobby(c, cx, shell, rows, &st),
    }
}

/// His card on the Zombies screens: his name, calling card and emblem, with
/// his Zombies rank (BO2's symbol, its step and how far to the next) in
/// place of his Multiplayer level.
pub(crate) fn zm_card(c: &mut Kids, cx: &mut Ctx, card: &PlayerCard) {
    // As Infinite Warfare's Zombies menu: his line at the top right (rank,
    // name), his rank's progress under it.
    let r = zm_record::get();
    let (sym, step, frac) = zm_record::rank(&r);
    let icon = cx.pic(&[zm_record::icon(&r), format!("menu_zm_rank_{sym}")]);
    let mark = cards::mine(Kind::Emblem, &card.emblem);
    c.spawn(Node { width: cx.px(560.0), flex_direction: FlexDirection::Column, row_gap: cx.px(6.0), ..abs(None, Some(40.0), Some(90.0), None, cx) }).with_children(|p| {
        p.spawn((
            Node { height: cx.px(40.0), column_gap: cx.px(14.0), align_items: AlignItems::Center, padding: UiRect::horizontal(cx.px(12.0)), ..default() },
            BackgroundGradient::from(LinearGradient::to_right(vec![
                ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.85), Val::Percent(0.0)),
                ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.55), Val::Percent(100.0)),
            ])),
        ))
        .with_children(|l| {
            match icon {
                Some(h) => {
                    l.spawn((ImageNode::new(h), Node { width: cx.px(30.0), height: cx.px(30.0), ..default() }));
                }
                None => {
                    l.spawn((Text::new(sym.to_string()), cx.bold(24.0), TextColor(cx.pal.accent)));
                }
            }
            emblem(l, cx, &mark, 30.0);
            l.spawn((Text::new(card.name.clone()), cx.regular(24.0), TextColor(WHITE)));
        });
        p.spawn((Text::new(format!("{}  ·  {step} OF 5", zm_record::rank_title(sym))), cx.regular(19.0), TextColor(TEXT)));
        p.spawn((Node { width: cx.px(300.0), height: cx.px(4.0), ..default() }, BackgroundColor(KEYCAP)))
            .with_child((Node { width: Val::Percent(frac * 100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(cx.pal.accent)));
    });
}

/// The Zombies menu: the Ghosts-style frame (best round now lives in
/// COMBAT RECORD).
fn menu(c: &mut Kids, cx: &mut Ctx, shell: &Shell, rows: &[Row]) {
    super::view::ghosts_frame(c, cx, shell, rows, "SURVIVAL");
}

/// COMBAT RECORD: Zombies' own PLAYER STATS, in the Zombies red - the
/// soldier preview's place at the left, his banner at the right (round
/// emblem, name and rank over his calling card, the rank's symbol and its
/// step as the big number), the rank's progress, the rounds his last five
/// games reached, and his Zombies numbers (no Multiplayer ratios or wins;
/// what is not recorded shows `—`).
fn record(c: &mut Kids, cx: &mut Ctx) {
    header_ex(c, cx, &["OUTBREAK", "COMBAT RECORD"], None, true);
    let r: Record = zm_record::get();
    let (sym, step, frac) = zm_record::rank(&r);
    super::ghosts::soldier_preview(c, cx, 260.0, 170.0, 620.0, 780.0);
    let none = || "—".to_owned();
    let kills_per_game = if r.games == 0 { none() } else { format!("{:.1}", f64::from(r.kills) / f64::from(r.games)) };
    let time = if r.seconds == 0 { none() } else { format!("{}h {:02}m", r.seconds / 3600, r.seconds / 60 % 60) };
    let favourite = r.favourite_map().map_or_else(none, |m| all().iter().find(|z| z.map == m).map_or_else(|| m.to_uppercase(), |z| z.title.to_owned()));
    let list: Vec<(String, String)> = vec![
        ("BEST ROUND".to_owned(), r.best_round.to_string()),
        ("GAMES PLAYED".to_owned(), r.games.to_string()),
        ("TOTAL KILLS".to_owned(), r.kills.to_string()),
        ("KILLS PER GAME".to_owned(), kills_per_game),
        ("REVIVES".to_owned(), r.revives.to_string()),
        ("DOWNS".to_owned(), r.downs.to_string()),
        ("HEADSHOTS".to_owned(), r.headshots.to_string()),
        ("TIME SURVIVED".to_owned(), time),
        ("FAVOURITE MAP".to_owned(), favourite),
        // (No weapon use is recorded yet.)
        ("PREFERRED WEAPON".to_owned(), none()),
    ];
    let note = if r.slump {
        "Your last game went badly: a few good games win the ground back."
    } else {
        "Rounds, kills, headshots and revives raise your rank; downs and leaving early lower it."
    };
    let cat = super::bridge::catalog();
    let (name, card, mark) = cat.as_ref().map_or_else(
        || (String::new(), String::new(), String::new()),
        |c| (c.player.name.clone(), cards::mine(Kind::Card, &c.player.card), cards::mine(Kind::Emblem, &c.player.emblem)),
    );
    let st = ghosts::Stats {
        name: &name,
        rank_title: zm_record::rank_title(sym).to_owned(),
        rank_icon: vec![zm_record::icon(&r), format!("menu_zm_rank_{sym}")],
        level: ((sym.saturating_sub(1)) * 5 + step).to_string(),
        card,
        emblem: mark,
        progress: (format!("RANK {sym}  ·  STEP {step} OF 5"), frac, note.to_owned()),
        chart_title: "ROUNDS REACHED (Last 5 Games)",
        chart: r.last_rounds.iter().map(|&n| n as f32).collect(),
        chart_whole: true,
        list,
    };
    ghosts::stats_panel(c, cx, &st, |p, cx| ghosts::stats_body(p, cx, &st));
}

fn abs(left: Option<f32>, top: Option<f32>, right: Option<f32>, bottom: Option<f32>, cx: &Ctx) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: left.map_or(Val::Auto, |v| cx.px(v)),
        top: top.map_or(Val::Auto, |v| cx.px(v)),
        right: right.map_or(Val::Auto, |v| cx.px(v)),
        bottom: bottom.map_or(Val::Auto, |v| cx.px(v)),
        ..default()
    }
}

/// The lobby, as Ghosts' lobby in the Zombies red: the rows from the left
/// edge, the game card under them (its picture, the state and what START
/// plays at its top right, the map's name on its strip); the map's starts
/// as a ruled list at the right, then the Zombies game's state.
fn lobby(c: &mut Kids, cx: &mut Ctx, shell: &Shell, rows: &[Row], st: &ZmState) {
    if shell.zm_kind == ZmKind::Custom {
        return custom_lobby(c, cx, shell, rows, st);
    }
    header(c, cx, &["OUTBREAK", shell.zm_kind.title()]);
    let (m, z) = chosen(shell);
    let pictures = zombies::pictures(m, shell.zm_start);
    let round = zombies::ROUNDS[shell.zm_round.min(zombies::ROUNDS.len() - 1)];
    c.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(0.0), Some(176.0), None, None, cx) })
        .with_children(|col| ghosts::rows(col, cx, rows, shell.focus, ghosts::Look::list(700.0)));
    let status = match (&st.running, st.problem()) {
        (Some(_), _) => "IN OUTBREAK",
        (None, Some(_)) => "Zombies is not installed",
        (None, None) if shell.zm_kind == ZmKind::Public => "Public game",
        (None, None) if shell.zm_kind == ZmKind::Custom && shell.zm_bots > 0 => "Custom game, with bots",
        (None, None) => "Ready",
    };
    let line = match &st.running {
        Some(game) => format!("{game} is running. Quit it to come back here."),
        None if round > 1 => format!("{}, from round {round}", game_line(m, z)),
        None => game_line(m, z),
    };
    c.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(0.0), None, None, Some(124.0), cx) }).with_children(|col| {
        ghosts::vote_card(col, cx, ghosts::Card { pictures: &pictures, name: m.title, top: (&line, status), on: false, hit: None, w: 700.0, pic_h: 190.0, delay: 0.08 });
    });
    // The right column: the map and its starts, then the Zombies game.
    c.spawn((
        Node { width: cx.px(620.0), flex_direction: FlexDirection::Column, row_gap: cx.px(6.0), ..abs(None, Some(196.0), Some(72.0), None, cx) },
        anim::from_left(cx.s, -80.0, 0.05),
    ))
    .with_children(|p| {
        p.spawn((Text::new(m.title), cx.bold(44.0), TextColor(WHITE)));
        p.spawn((Text::new(m.place.to_uppercase()), cx.bold(19.0), TextColor(cx.pal.accent)));
        p.spawn((Text::new("STARTS AND MODES"), cx.bold(19.0), TextColor(FAINT), Node { margin: UiRect::top(cx.px(14.0)), ..default() }));
        for (i, s) in m.starts.iter().enumerate() {
            let on = i == shell.zm_start.min(m.starts.len() - 1);
            p.spawn((
                Node { height: cx.px(38.0), column_gap: cx.px(12.0), align_items: AlignItems::Center, padding: UiRect::horizontal(cx.px(14.0)), border: UiRect::top(cx.px(1.0)), ..default() },
                BorderColor::all(WHITE.with_alpha(if on { 0.22 } else { 0.08 })),
                fade_right(if on { cx.pal.accent_a(0.28) } else { WHITE.with_alpha(0.06) }, WHITE.with_alpha(0.02)),
            ))
            .with_children(|r| {
                r.spawn((Node { width: cx.px(4.0), height: cx.px(22.0), ..default() }, BackgroundColor(if on { cx.pal.accent } else { Color::NONE })));
                r.spawn((Node { flex_grow: 1.0, ..default() },)).with_child((Text::new(start_name(m, s)), cx.regular(23.0), TextColor(if on { lift(cx.pal.accent, 0.3) } else { WHITE })));
                r.spawn((Text::new(s.mode_title), cx.bold(18.0), TextColor(if on { WHITE } else { DIM })));
            });
        }
        p.spawn((Node { height: Val::Px(1.0), width: Val::Percent(100.0), margin: UiRect::vertical(cx.px(10.0)), ..default() }, BackgroundColor(cx.pal.line)));
        match st.problem() {
            Some(problem) => {
                p.spawn((Text::new("OUTBREAK IS NOT INSTALLED"), cx.bold(22.0), TextColor(cx.pal.accent)));
                p.spawn((Text::new(problem.to_owned()), cx.regular(20.0), TextColor(TEXT)));
            }
            None => {
                let pack = if st.bo1() { "ZOMBIES DECLASSIFIED" } else { "NOT SET UP" };
                for (k, v) in [("GAME", if st.running.is_some() { "RUNNING" } else { "READY" }), ("BLACK OPS MAPS", pack), ("CHEAT MENU", "NUM 1 IN A SOLO GAME")] {
                    p.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|r| {
                        r.spawn((Text::new(k), cx.bold(19.0), TextColor(FAINT)));
                        r.spawn((Text::new(v), cx.bold(19.0), TextColor(TEXT)));
                    });
                }
            }
        }
    });
}

/// Zombies' CUSTOM GAME, laid out as Multiplayer's Custom Game (the
/// owner's reference) in the Zombies red: `ZOMBIES / CUSTOM GAME` small
/// over CUSTOM GAME, the rows down the left with the line about the one in
/// focus, the map card at the bottom left (the map, the start and mode, the
/// round), and no teams: one SURVIVORS panel at the right, him, then his
/// bots, then open places up to Zombies' four players.
fn custom_lobby(c: &mut Kids, cx: &mut Ctx, shell: &Shell, rows: &[Row], st: &ZmState) {
    ghosts::title_block(c, cx, "ZOMBIES  /  CUSTOM GAME", "CUSTOM GAME");
    let (m, z) = chosen(shell);
    let pictures = zombies::pictures(m, shell.zm_start);
    let round = zombies::ROUNDS[shell.zm_round.min(zombies::ROUNDS.len() - 1)];
    c.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(0.0), Some(212.0), None, None, cx) })
        .with_children(|col| ghosts::rows(col, cx, rows, shell.focus, ghosts::Look { width: 660.0, h: 46.0, px: 26.0, sub: Some(96.0) }));
    let status = match (&st.running, st.problem()) {
        (Some(_), _) => "IN OUTBREAK".to_owned(),
        (None, Some(_)) => "NOT INSTALLED".to_owned(),
        (None, None) => format!("ROUND {round}  ·  READY"),
    };
    let s = m.starts.get(shell.zm_start.min(m.starts.len().saturating_sub(1)));
    let mode = s.map_or_else(|| z.mode_title.to_uppercase(), |s| format!("{}  ·  {}", start_name(m, s).to_uppercase(), s.mode_title.to_uppercase()));
    ghosts::mode_card(c, cx, ghosts::ModeCard { pictures: &pictures, map: m.title, mode: &mode, icon: None, status: &status, pulse: st.running.is_some(), w: 600.0, pic_h: 196.0 }, 96.0, 128.0);
    // The players: him, his bots (`zm_bots`, held to the four places), then
    // the places left open.
    let card = super::bridge::catalog().map(|c| c.player.clone());
    let me = super::view::LobbyPlayer {
        level: None,
        prestige: 0,
        name: card.as_ref().map_or_else(|| "You".to_owned(), |p| p.name.clone()),
        me: true,
        host: true,
        team: Some(cx.pal.accent),
        card: card.as_ref().map_or_else(String::new, |p| cards::mine(Kind::Card, &p.card)),
    };
    let max = zombies::MAX_PLAYERS as usize;
    let bots = shell.zm_bots.min(zombies::bot_cap(zombies::CUSTOM_HUMANS));
    let mut players = vec![me];
    players.extend((1..=bots).map(|n| super::view::LobbyPlayer {
        level: None,
        prestige: 0,
        name: format!("Bot {n}"),
        me: false,
        host: false,
        team: Some(cx.pal.accent),
        card: String::new(),
    }));
    let open = max.saturating_sub(players.len());
    let waiting = if st.running.is_some() { "IN GAME" } else { "WAITING" };
    let section = ghosts::Section { title: format!("SURVIVORS  ·  {waiting}"), color: cx.pal.accent, emblem: None, players, bots_from: zombies::CUSTOM_HUMANS as usize, open, of: max };
    ghosts::grid(c, cx, 186.0, 860.0, 420.0);
    ghosts::team_sections(c, cx, std::slice::from_ref(&section), 186.0);
    if let (None, Some(problem)) = (&st.running, st.problem()) {
        c.spawn((Node { width: cx.px(700.0), flex_direction: FlexDirection::Column, row_gap: cx.px(6.0), ..abs(None, Some(420.0), Some(72.0), None, cx) }, anim::from_left(cx.s, -60.0, 0.1))).with_children(|p| {
            p.spawn((Text::new("OUTBREAK IS NOT INSTALLED"), cx.bold(22.0), TextColor(cx.pal.accent)));
            p.spawn((Text::new(problem.to_owned()), cx.regular(20.0), TextColor(TEXT)));
        });
    }
}

/// The map picker: a page per game, the maps as tiles.
fn map_pick(c: &mut Kids, cx: &mut Ctx, shell: &Shell, st: &ZmState) {
    let p = pages(st.bo1());
    let page = shell.page.min(p.len() - 1);
    header(c, cx, &["OUTBREAK", "SELECT MAP", p[page].0]);
    if p.len() > 1 {
        let labels: Vec<String> = p.iter().map(|(t, _)| short_section(t)).collect();
        tabs(c, cx, &labels, page, Hit::Page);
    }
    let maps = &p[page].1;
    let (tw, th, gap) = (280.0, 158.0, 14.0);
    let (cols, first, last) = grid_window(cx, maps.len(), shell.focus, tw, th, gap, 196.0, 190.0);
    grid_more(c, cx, first, last, maps.len(), 72.0 + cols as f32 * (tw + gap) - 220.0, 160.0);
    c.spawn(Node {
        width: cx.px(cols as f32 * (tw + gap)),
        flex_wrap: FlexWrap::Wrap,
        column_gap: cx.px(gap),
        row_gap: cx.px(gap - 10.0),
        ..abs(Some(72.0), Some(196.0), None, None, cx)
    })
    .with_children(|grid| {
        for (k, &i) in maps.iter().enumerate().take(last).skip(first) {
            let m = &all()[i];
            let on = k == shell.focus;
            let chosen = m.map == shell.zm_map;
            let pictures = zombies::pictures(m, 0);
            ghosts::vote_card(grid, cx, ghosts::Card {
                pictures: &pictures,
                name: m.title,
                top: ("", if chosen { "SELECTED" } else { "" }),
                on,
                hit: Some(Hit::Row(k)),
                w: tw,
                pic_h: th - 44.0 - 10.0,
                delay: ((k - first) as f32 * 0.015).min(0.18),
            });
        }
    });
    if let Some(&i) = maps.get(shell.focus) {
        let m = &all()[i];
        let modes: Vec<&str> = m.starts.iter().map(|s| s.mode_title).fold(Vec::new(), |mut v, t| {
            if !v.contains(&t) {
                v.push(t);
            }
            v
        });
        c.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                ..abs(None, None, Some(72.0), Some(124.0), cx)
            },
            anim::from_left(cx.s, -60.0, 0.05),
        ))
        .with_children(|t| {
            t.spawn((Text::new(m.title), cx.bold(64.0), TextColor(WHITE)));
            t.spawn((Text::new(format!("{}  ·  {}", m.place.to_uppercase(), modes.join(" / "))), cx.bold(22.0), TextColor(cx.pal.accent)));
        });
    }
}

/// The start picker: the map's starts and modes as Ghosts' rows, and the
/// one in focus as a card at the right.
fn start_pick(c: &mut Kids, cx: &mut Ctx, shell: &Shell, rows: &[Row]) {
    let (m, _) = chosen(shell);
    header(c, cx, &["OUTBREAK", m.title, "SELECT START"]);
    c.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(0.0), Some(176.0), None, None, cx) }).with_children(|col| {
        ghosts::rows(col, cx, rows, shell.focus, ghosts::Look::list(640.0).bare());
    });
    let Some(s) = m.starts.get(shell.focus) else { return };
    let pictures = zombies::pictures(m, shell.focus);
    let picked = shell.focus == shell.zm_start.min(m.starts.len() - 1);
    c.spawn(Node { width: cx.px(640.0), flex_direction: FlexDirection::Column, row_gap: cx.px(8.0), ..abs(None, Some(196.0), Some(72.0), None, cx) }).with_children(|p| {
        ghosts::vote_card(p, cx, ghosts::Card { pictures: &pictures, name: &start_name(m, s), top: (s.mode_title, if picked { "SELECTED" } else { "" }), on: picked, hit: None, w: 640.0, pic_h: 300.0, delay: 0.04 });
        p.spawn((Node { flex_direction: FlexDirection::Column, row_gap: cx.px(8.0), padding: UiRect::horizontal(cx.px(8.0)), ..default() }, anim::from_left(cx.s, -60.0, 0.08)))
            .with_children(|t| {
                t.spawn((Text::new(start_name(m, s)), cx.bold(44.0), TextColor(WHITE)));
                t.spawn((Text::new(s.mode_title), cx.bold(22.0), TextColor(cx.pal.accent)));
                t.spawn((Text::new(s.desc), cx.regular(23.0), TextColor(TEXT)));
                if picked {
                    t.spawn((Text::new("SELECTED"), cx.bold(19.0), TextColor(cx.pal.accent)));
                }
            });
    });
}
