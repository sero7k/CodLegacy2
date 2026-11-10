//! bo2mp shell: the parts of the Ghosts-style front end (the look the owner
//! approved for the main menu), shared by the lobbies, the pickers,
//! Create-a-Class, the stats pages and the Zombies screens. Always in the
//! screen's own palette (`cx.pal`: Multiplayer blue, Zombies red, WARZONE
//! grey and orange), never Ghosts' yellow; no game art of Ghosts' own.
//!
//! - [`rows`]: ruled rows from the safe frame's left edge, the text pushed to
//!   their right end, each ending in a cut corner; the one in focus a band
//!   in the accent that widens in from the left; the line about it under.
//! - [`top_status`]: the small status line at the top right.
//! - [`vote_card`]: a map's picture, the mode and votes at its top right,
//!   its name on a strip under it with a cut corner.
//! - [`slot_list`] over [`grid`]: the right-hand list of places (players
//!   or games), empty ones a faint ruled band, over a faint grid with
//!   numbers floating in it.
//! - [`stats_page`]: PLAYER STATS / COMBAT RECORD: a soldier preview slot,
//!   the player's banner, a five-game chart and a ruled list of numbers.
//!
//! Motion comes from `super::anim` (rows and panels slide in from the
//! left, cards rise, the focus band grows, a working line pulses).

use bevy::prelude::*;

use super::anim;
use super::theme::*;
use super::view::{Ctx, Kids, LobbyPlayer, abs, banner, emblem_bare, lift, picture_box, pointer};
use super::{Act, Hit, Row};

/// Where titles and the line under the rows start (design units).
pub(crate) const INSET: f32 = 150.0;

/// bo2mp: the title-safe margin every front-end screen keeps clear of the
/// window's edges, in 1080p pixels (scaled with the window as the rest of
/// the UI): 80 at the sides, 50 at the top and foot (about 4-5%). The
/// shell's screens are drawn into a frame inset by it ([`safe_area`]);
/// only the backgrounds and the shades that must run off the screen
/// ([`bleed`], [`top_shade`], [`foot_shade`]) reach past it.
pub(crate) const SAFE_X: f32 = 80.0;
pub(crate) const SAFE_Y: f32 = 50.0;

/// The frame the shell's screens are drawn into, from the window's size in
/// pixels and its UI scale (`window_scale`: height / 1080).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SafeArea {
    /// The frame's offset from each edge, in pixels.
    pub inset: Vec2,
    /// Pixels per design unit inside the frame: the screens' 1080-high
    /// design fits the frame's height (so nothing laid out for 1080 falls
    /// off its foot).
    pub s: f32,
    /// The frame in design units (`h` is 1080 at any window size).
    pub w: f32,
    pub h: f32,
    /// The margin in design units: how far past the frame the window's
    /// edges are (for what [`bleed`]s).
    pub bleed: Vec2,
}

pub(crate) fn safe_area(area: Vec2, s: f32) -> SafeArea {
    let inset = Vec2::new(SAFE_X, SAFE_Y) * s;
    let inner = (area - inset * 2.0).max(Vec2::splat(1.0));
    let fit = inner.y / 1080.0;
    SafeArea { inset, s: fit, w: inner.x / fit, h: inner.y / fit, bleed: inset / fit }
}

/// A node over the whole window from inside the safe frame (a dimmer
/// behind a question).
pub(crate) fn bleed(cx: &Ctx) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: cx.px(-cx.bleed.x),
        right: cx.px(-cx.bleed.x),
        top: cx.px(-cx.bleed.y),
        bottom: cx.px(-cx.bleed.y),
        ..default()
    }
}

/// The dark shade across the top, `h` high under the frame's top, out to
/// the window's edges.
pub(crate) fn top_shade(cx: &Ctx, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: cx.px(-cx.bleed.x),
        right: cx.px(-cx.bleed.x),
        top: cx.px(-cx.bleed.y),
        height: cx.px(h + cx.bleed.y),
        ..default()
    }
}

/// A band along the foot, `h` high over the frame's foot, out to the
/// window's edges (its contents stay in the frame: pad by `cx.bleed`).
pub(crate) fn foot_shade(cx: &Ctx, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: cx.px(-cx.bleed.x),
        right: cx.px(-cx.bleed.x),
        bottom: cx.px(-cx.bleed.y),
        height: cx.px(h + cx.bleed.y),
        ..default()
    }
}

/// A cut corner at the right end of a band `h` high: its right edge
/// straight down half way, then cut back at 45 degrees to its foot.
pub(crate) fn bevel(c: &mut Kids, cx: &Ctx, h: f32, color: Color) {
    let d = h * std::f32::consts::SQRT_2;
    c.spawn(Node { position_type: PositionType::Absolute, right: Val::Px(0.0), top: Val::Px(0.0), width: cx.px(h * 0.5), height: cx.px(h), overflow: Overflow::clip(), ..default() }).with_child((
        Node { position_type: PositionType::Absolute, left: cx.px(-d * 0.5), top: cx.px(-d * 0.5), width: cx.px(d), height: cx.px(d), ..default() },
        BackgroundColor(color),
        UiTransform::from_rotation(Rot2::degrees(45.0)),
    ));
}

/// The dark a band fades into at its right end.
fn band_end(cx: &Ctx) -> Color {
    let p = cx.pal.panel.to_srgba();
    Color::srgba(p.red, p.green, p.blue, 0.66)
}

/// How [`rows`] are drawn.
#[derive(Clone, Copy)]
pub(crate) struct Look {
    pub width: f32,
    pub h: f32,
    pub px: f32,
    /// Where the line about the row in focus starts (`None`: no line).
    pub sub: Option<f32>,
}

impl Look {
    /// The lobbies' and pickers' rows.
    pub fn list(width: f32) -> Look {
        Look { width, h: 52.0, px: 28.0, sub: Some(INSET) }
    }

    /// Without the line under them.
    pub fn bare(self) -> Look {
        Look { sub: None, ..self }
    }
}

/// Ghosts' rows (the column they go in starts at the screen's left edge).
/// The hit targets are the plain rows' (`Hit::Row`, a selector's
/// `Hit::Step`); a gap goes before OPTIONS as on the main menu.
pub(crate) fn rows(c: &mut Kids, cx: &Ctx, rows: &[Row], focus: usize, look: Look) {
    let acc = cx.pal.accent;
    let tip = look.h * 0.5;
    let end = band_end(cx);
    for (i, r) in rows.iter().enumerate() {
        let on = i == focus;
        if r.act == Act::Options && i > 0 {
            c.spawn(Node { height: cx.px(22.0), flex_shrink: 0.0, ..default() });
        }
        c.spawn((
            Hit::Row(i),
            Button,
            Node { width: cx.px(look.width), height: cx.px(look.h), flex_shrink: 0.0, margin: UiRect::bottom(cx.px(4.0)), ..default() },
            anim::from_left(cx.s, 110.0, i as f32 * 0.025),
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    right: cx.px(tip),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::FlexEnd,
                    column_gap: cx.px(26.0),
                    padding: UiRect::right(cx.px(36.0)),
                    border: UiRect::top(cx.px(1.0)),
                    ..default()
                },
                BorderColor::all(if on { lift(acc, 0.35) } else { cx.pal.line }),
                fade_right(end.with_alpha(0.0), end),
            ))
            .with_children(|body| {
                if on {
                    let (g, w) = anim::grow();
                    body.spawn((
                        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(0.0), width: w, ..default() },
                        fade_right(acc.with_alpha(0.0), acc.with_alpha(0.85)),
                        g,
                    ));
                }
                body.spawn((Text::new(r.label.clone()), if on { cx.bold(look.px) } else { cx.regular(look.px) }, TextColor(if on { WHITE } else { TEXT.with_alpha(0.80) })));
                match &r.value {
                    Some(v) if !matches!(r.act, Act::Cycle(_)) => {
                        body.spawn((Text::new(v.clone()), cx.bold(look.px * 0.85), TextColor(if on { WHITE } else { DIM })));
                    }
                    Some(v) => {
                        body.spawn(Node { column_gap: cx.px(12.0), align_items: AlignItems::Center, ..default() }).with_children(|sel| {
                            sel.spawn((Hit::Step(i, -1), Button, Text::new("‹"), cx.bold(look.px), TextColor(if on { WHITE } else { FAINT })));
                            sel.spawn(Node { min_width: cx.px(150.0), justify_content: JustifyContent::Center, ..default() })
                                .with_child((Text::new(v.clone()), cx.bold(look.px * 0.85), TextColor(if on { WHITE } else { DIM })));
                            sel.spawn((Hit::Step(i, 1), Button, Text::new("›"), cx.bold(look.px), TextColor(if on { WHITE } else { FAINT })));
                        });
                    }
                    None => {}
                }
            });
            bevel(row, cx, look.h, if on { acc.with_alpha(0.85) } else { end });
        });
    }
    if let Some(inset) = look.sub
        && let Some(r) = rows.get(focus).filter(|r| !r.sub.is_empty())
    {
        c.spawn((
            Node { max_width: cx.px((look.width - inset).max(240.0)), margin: UiRect::new(cx.px(inset), Val::Px(0.0), cx.px(14.0), Val::Px(0.0)), column_gap: cx.px(12.0), align_items: AlignItems::Center, ..default() },
            anim::from_left(cx.s, 60.0, 0.06),
        ))
        .with_children(|d| {
            d.spawn((Node { width: cx.px(6.0), height: cx.px(20.0), flex_shrink: 0.0, ..default() }, BackgroundColor(cx.pal.accent)));
            d.spawn((Text::new(r.sub.clone()), cx.regular(24.0), TextColor(TEXT)));
        });
    }
}

/// The small line at the top right: what the screen is doing ("We need to
/// find 3 more players"), pulsing while it works; a second line under it
/// when there is one.
pub(crate) fn top_status(c: &mut Kids, cx: &Ctx, line: &str, pulse: bool, under: &str) {
    if line.is_empty() && under.is_empty() {
        return;
    }
    c.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::FlexEnd, row_gap: cx.px(2.0), max_width: cx.px(760.0), ..abs(None, Some(48.0), Some(72.0), None, cx) }).with_children(|t| {
        if !line.is_empty() {
            let mut e = t.spawn((Text::new(line.to_owned()), cx.regular(28.0), TextColor(TEXT), TextLayout::justify(bevy::text::Justify::Right)));
            if pulse {
                e.insert(anim::Pulse { base: TEXT });
            }
        }
        if !under.is_empty() {
            t.spawn((Text::new(under.to_owned()), cx.regular(21.0), TextColor(DIM), TextLayout::justify(bevy::text::Justify::Right)));
        }
    });
}

/// A map card (a vote, or the lobby's map).
pub(crate) struct Card<'a> {
    pub pictures: &'a [String],
    pub name: &'a str,
    /// The two lines at the picture's top right (the mode, the votes).
    pub top: (&'a str, &'a str),
    pub on: bool,
    pub hit: Option<Hit>,
    pub w: f32,
    pub pic_h: f32,
    /// After the screen opens, in seconds (the cards rise one by one).
    pub delay: f32,
}

/// A map card: its picture, the mode and votes at its top right, its name
/// on a strip under it ending in a cut corner. In focus: the strip in the
/// accent and an accent edge down the picture's left.
pub(crate) fn vote_card(c: &mut Kids, cx: &mut Ctx, k: Card) {
    let strip = 44.0;
    let tip = strip * 0.5;
    let acc = cx.pal.accent;
    let end = band_end(cx);
    let mut e = c.spawn((Node { width: cx.px(k.w), flex_direction: FlexDirection::Column, flex_shrink: 0.0, margin: UiRect::bottom(cx.px(10.0)), ..default() }, anim::rise(cx.s, 36.0, k.delay)));
    if let Some(hit) = k.hit {
        e.insert((hit, Button));
    }
    e.with_children(|card| {
        card.spawn((Node { width: cx.px(k.w - tip), height: cx.px(k.pic_h), overflow: Overflow::clip(), border: UiRect::left(cx.px(3.0)), ..default() }, BorderColor::all(if k.on { acc } else { Color::NONE })))
            .with_children(|p| {
                picture_box(p, cx, k.pictures, k.w - tip - 3.0, k.pic_h, ());
                p.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        right: Val::Px(0.0),
                        top: Val::Px(0.0),
                        bottom: Val::Px(0.0),
                        width: Val::Percent(55.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::FlexEnd,
                        padding: UiRect::axes(cx.px(14.0), cx.px(8.0)),
                        ..default()
                    },
                    fade_right(Color::srgba(0.0, 0.0, 0.0, 0.0), Color::srgba(0.0, 0.0, 0.0, 0.55)),
                ))
                .with_children(|t| {
                    let shadow = TextShadow { offset: Vec2::splat(2.0 * cx.s), color: Color::srgba(0.0, 0.0, 0.0, 0.85), ..default() };
                    t.spawn((Text::new(k.top.0.to_owned()), cx.regular(24.0), TextColor(TEXT), shadow));
                    t.spawn((Text::new(k.top.1.to_owned()), cx.bold(26.0), TextColor(WHITE), shadow));
                });
            });
        card.spawn(Node { width: cx.px(k.w), height: cx.px(strip), ..default() }).with_children(|s| {
            s.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    right: cx.px(tip),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::FlexEnd,
                    padding: UiRect::right(cx.px(30.0)),
                    border: UiRect::top(cx.px(1.0)),
                    ..default()
                },
                BorderColor::all(if k.on { lift(acc, 0.35) } else { cx.pal.line }),
                if k.on { fade_right(acc.with_alpha(0.25), acc.with_alpha(0.85)) } else { fade_right(Color::srgba(0.0, 0.0, 0.0, 0.70), end) },
            ))
            .with_child((Text::new(k.name.to_owned()), if k.on { cx.bold(28.0) } else { cx.regular(28.0) }, TextColor(if k.on { WHITE } else { TEXT })));
            bevel(s, cx, strip, if k.on { acc.with_alpha(0.85) } else { end });
        });
    });
}

/// A faint grid over the right of the screen, under the list of places.
/// (bo2mp: Ghosts' floating 10-digit match ids with diamonds are gone: the
/// owner read them as stray debug numbers over the lobby.)
pub(crate) fn grid(c: &mut Kids, cx: &Ctx, top: f32, w: f32, h: f32) {
    let cell = 54.0;
    c.spawn(Node { width: cx.px(w), height: cx.px(h), overflow: Overflow::clip(), ..abs(None, Some(top), Some(72.0), None, cx) }).with_children(|g| {
        let ink = WHITE.with_alpha(0.045);
        for k in 0..=(w / cell) as usize {
            g.spawn((Node { position_type: PositionType::Absolute, left: cx.px(k as f32 * cell), top: Val::Px(0.0), width: cx.px(1.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(ink)));
        }
        for k in 0..=(h / cell) as usize {
            g.spawn((Node { position_type: PositionType::Absolute, top: cx.px(k as f32 * cell), left: Val::Px(0.0), height: cx.px(1.0), width: Val::Percent(100.0), ..default() }, BackgroundColor(ink)));
        }
    });
}

/// The right-hand list of places: `title` over it (pulsing while it
/// works), a ruled band per place; his own first, his name in the accent;
/// an empty place a faint band.
#[allow(clippy::too_many_arguments)]
pub(crate) fn slot_list(c: &mut Kids, cx: &mut Ctx, title: &str, pulse: bool, players: &[LobbyPlayer], max: usize, top: f32, row_h: f32) {
    let w = 860.0;
    let acc = cx.pal.accent;
    c.spawn(Node { width: cx.px(w), flex_direction: FlexDirection::Column, ..abs(None, Some(top), Some(72.0), None, cx) }).with_children(|list| {
        list.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, padding: UiRect::horizontal(cx.px(48.0)), margin: UiRect::bottom(cx.px(6.0)), ..default() }).with_children(|t| {
            let mut e = t.spawn((Text::new(title.to_owned()), cx.regular(28.0), TextColor(WHITE)));
            if pulse {
                e.insert(anim::Pulse { base: WHITE });
            }
            t.spawn((Text::new(format!("{}/{max}", players.len().min(max))), cx.regular(24.0), TextColor(DIM)));
        });
        for i in 0..max.max(players.len()) {
            let p = players.get(i);
            list.spawn((
                Node { height: cx.px(row_h), flex_shrink: 0.0, column_gap: cx.px(14.0), align_items: AlignItems::Center, padding: UiRect::horizontal(cx.px(16.0)), margin: UiRect::bottom(cx.px(5.0)), border: UiRect::top(cx.px(1.0)), ..default() },
                BorderColor::all(WHITE.with_alpha(if p.is_some() { 0.22 } else { 0.08 })),
                fade_right(
                    if p.is_some_and(|p| p.me) { acc.with_alpha(0.22) } else { WHITE.with_alpha(if p.is_some() { 0.10 } else { 0.05 }) },
                    WHITE.with_alpha(0.025),
                ),
                anim::from_left(cx.s, 60.0, 0.05 + i as f32 * 0.012),
            ))
            .with_children(|r| {
                let Some(p) = p else { return };
                r.spawn((Node { width: cx.px(4.0), height: Val::Percent(80.0), ..default() }, BackgroundColor(p.team.unwrap_or(Color::NONE))));
                r.spawn(Node { width: cx.px(12.0), ..default() }).with_children(|m| {
                    if p.me {
                        pointer(m, cx);
                    }
                });
                r.spawn((Node { width: cx.px(40.0), justify_content: JustifyContent::FlexEnd, ..default() },))
                    .with_child((Text::new(p.level.map_or_else(String::new, |l| l.to_string())), cx.regular(row_h * 0.62), TextColor(WHITE)));
                let icon = p.level.and_then(|l| cx.pic(&[bo2_profile::progression::rank_picture(l, p.prestige)]));
                let side = row_h * 0.78;
                match icon {
                    Some(h) => {
                        r.spawn((ImageNode::new(h), Node { width: cx.px(side), height: cx.px(side), ..default() }));
                    }
                    None => {
                        r.spawn(Node { width: cx.px(side), ..default() });
                    }
                }
                r.spawn(Node { flex_grow: 1.0, column_gap: cx.px(12.0), align_items: AlignItems::Center, margin: UiRect::left(cx.px(90.0)), ..default() }).with_children(|n| {
                    n.spawn((Text::new(p.name.clone()), cx.regular(row_h * 0.62), TextColor(if p.me { lift(acc, 0.22) } else { WHITE })));
                    if p.host {
                        n.spawn((Text::new("HOST"), cx.bold(16.0), TextColor(acc)));
                    }
                });
                // His calling card, small, at the row's end.
                banner(r, cx, &p.card, 150.0, row_h - 8.0, None);
            });
        }
    });
}

/// One side of the Custom Game lobby's player list (`team_sections`).
pub(crate) struct Section {
    /// Its header: WAITING, or the team (YOUR TEAM, ENEMY TEAM).
    pub title: String,
    /// The team's colour: the header's edge and each row's mark.
    pub color: Color,
    /// The faction's emblem before the title (none: the title alone).
    pub emblem: Option<Handle<Image>>,
    /// Real players first, then bots (from `bots_from`).
    pub players: Vec<LobbyPlayer>,
    pub bots_from: usize,
    /// Open places shown under them, dimmed.
    pub open: usize,
    /// The places it shows in its count (`players / of`).
    pub of: usize,
}

/// A small tag at a player row's right end (HOST / YOU / BOT): `solid`
/// fills it with `color` (dark text), otherwise an outline in it.
fn pill(c: &mut Kids, cx: &Ctx, label: &str, color: Color, solid: bool, h: f32) {
    let p = cx.pal.panel.to_srgba();
    c.spawn((
        Node { height: cx.px(h), padding: UiRect::horizontal(cx.px(7.0)), align_items: AlignItems::Center, flex_shrink: 0.0, border: UiRect::all(cx.px(1.0)), ..default() },
        BorderColor::all(if solid { color } else { color.with_alpha(0.70) }),
        BackgroundColor(if solid { color } else { Color::srgba(p.red, p.green, p.blue, 0.55) }),
    ))
    .with_child((Text::new(label.to_owned()), cx.bold(h * 0.66), TextColor(if solid { Color::srgb(0.02, 0.03, 0.05) } else { color })));
}

/// The Custom Game lobby's players at the right, laid out as the owner's
/// reference: per side a header bar (the team, or WAITING) over its rows
/// (real players, then bots, then open places). Each row is our old
/// Infinite Warfare-style lobby line (`view::player_list` before the
/// Ghosts restyle) in the Ghosts look: the team's bar at its left, his rank
/// icon and level, the name left-aligned in the bright bold face, small
/// HOST / YOU / BOT pills and the calling card at its right; rows shaded in
/// turn, his own in the accent with the pointer outside it; open places a
/// dim OPEN SLOT with a + mark. Two sections in a team mode, one in a
/// free-for-all (and Zombies' SURVIVORS). The rows shrink so every section
/// fits above the foot.
pub(crate) fn team_sections(c: &mut Kids, cx: &mut Ctx, sections: &[Section], top: f32) {
    let w = 700.0;
    let (head_h, gap) = (44.0, 18.0);
    let n = sections.iter().map(|s| s.players.len() + s.open).sum::<usize>().max(1);
    let k_sec = sections.len() as f32;
    // Room down to the foot band (90) with a little air.
    let room = cx.h - top - 104.0 - k_sec * head_h - (k_sec - 1.0).max(0.0) * gap;
    let row_h = (room / n as f32).clamp(22.0, 36.0).floor();
    let acc = cx.pal.accent;
    let p = cx.pal.panel.to_srgba();
    let shade = |a: f32| Color::srgba(p.red, p.green, p.blue, a);
    c.spawn(Node { width: cx.px(w + 24.0), flex_direction: FlexDirection::Column, row_gap: cx.px(gap), ..abs(None, Some(top), Some(72.0), None, cx) }).with_children(|col| {
        let mut k = 0usize;
        for (si, s) in sections.iter().enumerate() {
            col.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|sec| {
                // The header bar.
                sec.spawn((
                    Node {
                        height: cx.px(head_h),
                        margin: UiRect::left(cx.px(24.0)),
                        padding: UiRect::horizontal(cx.px(16.0)),
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        border: UiRect { left: cx.px(4.0), bottom: cx.px(1.0), ..default() },
                        ..default()
                    },
                    // (A bar in the side's colour: blue his team, red the
                    // enemy's, the screen's accent with no teams.)
                    BorderColor { left: lift(s.color, 0.30), bottom: s.color, ..BorderColor::all(Color::NONE) },
                    fade_right(s.color.with_alpha(0.82), s.color.with_alpha(0.22)),
                    anim::slide(cx.s, Vec2::new(60.0, 0.0), 0.04 + si as f32 * 0.06),
                ))
                .with_children(|h| {
                    h.spawn(Node { column_gap: cx.px(12.0), align_items: AlignItems::Center, ..default() }).with_children(|t| {
                        if let Some(em) = s.emblem.clone() {
                            t.spawn((ImageNode::new(em), Node { width: cx.px(36.0), height: cx.px(36.0), flex_shrink: 0.0, ..default() }));
                        }
                        t.spawn((Text::new(s.title.clone()), cx.bold(26.0), TextColor(WHITE)));
                    });
                    h.spawn((Text::new(format!("{}/{}", s.players.len(), s.of)), cx.bold(22.0), TextColor(WHITE.with_alpha(0.85))));
                });
                for i in 0..s.players.len() + s.open {
                    let pl = s.players.get(i);
                    let me = pl.is_some_and(|p| p.me);
                    sec.spawn((Node { height: cx.px(row_h), flex_shrink: 0.0, align_items: AlignItems::Center, ..default() }, anim::slide(cx.s, Vec2::new(60.0, 0.0), 0.06 + si as f32 * 0.06 + k as f32 * 0.01)))
                        .with_children(|line| {
                            // The pointer, outside the row (his own).
                            line.spawn(Node { width: cx.px(24.0), justify_content: JustifyContent::Center, ..default() }).with_children(|m| {
                                if me {
                                    pointer(m, cx);
                                }
                            });
                            let bg = match pl {
                                _ if me => fade_right(acc.with_alpha(0.34), acc.with_alpha(0.06)),
                                Some(_) => fade_right(shade(if i % 2 == 0 { 0.74 } else { 0.58 }), shade(0.22)),
                                None => fade_right(shade(0.34), shade(0.10)),
                            };
                            line.spawn((
                                Node {
                                    flex_grow: 1.0,
                                    height: Val::Percent(100.0),
                                    align_items: AlignItems::Center,
                                    column_gap: cx.px(10.0),
                                    padding: UiRect { right: cx.px(4.0), ..default() },
                                    border: UiRect::bottom(cx.px(1.0)),
                                    ..default()
                                },
                                BorderColor::all(if me { acc.with_alpha(0.55) } else { WHITE.with_alpha(if pl.is_some() { 0.10 } else { 0.05 }) }),
                                bg,
                            ))
                            .with_children(|r| {
                                // The team's bar, full height.
                                r.spawn((Node { width: cx.px(4.0), height: Val::Percent(100.0), flex_shrink: 0.0, ..default() }, BackgroundColor(if pl.is_some() { s.color } else { s.color.with_alpha(0.22) })));
                                let Some(pl) = pl else {
                                    // An open place: a + box where his rank would be.
                                    r.spawn((
                                        Node { width: cx.px(row_h * 0.62), height: cx.px(row_h * 0.62), margin: UiRect::left(cx.px(6.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(cx.px(1.0)), flex_shrink: 0.0, ..default() },
                                        BorderColor::all(FAINT),
                                    ))
                                    .with_child((Text::new("+"), cx.bold(row_h * 0.56), TextColor(DIM)));
                                    r.spawn((Text::new("OPEN SLOT"), cx.bold(row_h * 0.48), TextColor(FAINT), Node { margin: UiRect::left(cx.px(56.0 - row_h * 0.62)), ..default() }));
                                    return;
                                };
                                // Rank icon and level (the old line's badge).
                                let side = row_h * 0.78;
                                r.spawn(Node { width: cx.px(side + 40.0), margin: UiRect::left(cx.px(4.0)), column_gap: cx.px(4.0), align_items: AlignItems::Center, flex_shrink: 0.0, ..default() }).with_children(|b| {
                                    match pl.level.and_then(|l| cx.pic(&[bo2_profile::progression::rank_picture(l, pl.prestige)])) {
                                        Some(h) => {
                                            b.spawn((ImageNode::new(h), Node { width: cx.px(side), height: cx.px(side), flex_shrink: 0.0, ..default() }));
                                        }
                                        None => {
                                            b.spawn(Node { width: cx.px(side), ..default() });
                                        }
                                    }
                                    b.spawn((Text::new(pl.level.map_or_else(String::new, |l| l.to_string())), cx.bold(row_h * 0.50), TextColor(if me { WHITE } else { TEXT })));
                                });
                                // The name, left-aligned.
                                r.spawn((Node { flex_grow: 1.0, overflow: Overflow::clip(), ..default() },)).with_child((
                                    Text::new(pl.name.clone()),
                                    cx.bold(row_h * 0.60),
                                    TextColor(if me { lift(acc, 0.30) } else { WHITE }),
                                    TextLayout::linebreak(bevy::text::LineBreak::NoWrap),
                                ));
                                // Tags.
                                let ph = (row_h * 0.56).max(14.0);
                                if pl.host {
                                    pill(r, cx, "HOST", acc, true, ph);
                                }
                                if me {
                                    pill(r, cx, "YOU", lift(acc, 0.30), false, ph);
                                }
                                if i >= s.bots_from {
                                    pill(r, cx, "BOT", DIM, false, ph);
                                }
                                // His calling card, small, at the row's end.
                                if !pl.card.is_empty() {
                                    banner(r, cx, &pl.card, (row_h - 6.0) * 4.0, row_h - 6.0, None);
                                } else {
                                    r.spawn(Node { width: cx.px((row_h - 6.0) * 4.0), flex_shrink: 0.0, ..default() });
                                }
                            });
                        });
                    k += 1;
                }
            });
        }
    });
}

/// The Custom Game screens' title, as the owner's reference: the trail of
/// screens small on a ruled strip (`MULTIPLAYER / CUSTOM GAME`), the
/// screen's name large and thin under it after an accent bar; the dark
/// shade and accent rule of [`super::view::header`] behind it.
pub(crate) fn title_block(c: &mut Kids, cx: &Ctx, trail: &str, title: &str) {
    let acc = cx.pal.accent;
    let p = cx.pal.panel.to_srgba();
    let dark = Color::srgba(0.0, 0.01, 0.03, 0.80);
    c.spawn((top_shade(cx, 170.0), BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::new(dark, Val::Percent(0.0)), ColorStop::new(dark.with_alpha(0.0), Val::Percent(100.0))]))));
    c.spawn((Node { width: Val::Percent(62.0), height: cx.px(2.0), ..abs(Some(0.0), Some(166.0), None, None, cx) }, fade_right(acc.with_alpha(0.65), acc.with_alpha(0.0))));
    c.spawn((Node { flex_direction: FlexDirection::Column, row_gap: cx.px(4.0), ..abs(Some(96.0), Some(40.0), None, None, cx) }, anim::from_left(cx.s, 60.0, 0.0))).with_children(|t| {
        t.spawn((
            Node { width: cx.px(520.0), height: cx.px(30.0), padding: UiRect::horizontal(cx.px(12.0)), align_items: AlignItems::Center, border: UiRect { left: cx.px(4.0), bottom: cx.px(1.0), ..default() }, ..default() },
            BorderColor { left: acc, bottom: acc.with_alpha(0.45), ..BorderColor::all(Color::NONE) },
            fade_right(Color::srgba(p.red, p.green, p.blue, 0.92), Color::srgba(p.red, p.green, p.blue, 0.0)),
        ))
        .with_child((Text::new(trail.to_owned()), cx.bold(19.0), TextColor(TEXT)));
        t.spawn(Node { column_gap: cx.px(16.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
            r.spawn((Node { width: cx.px(7.0), height: cx.px(56.0), ..default() }, BackgroundColor(acc)));
            r.spawn((Text::new(title.to_owned()), cx.light(76.0), TextColor(WHITE)));
        });
    });
}

/// The Custom Game's map card at the bottom left (`mode_card`).
pub(crate) struct ModeCard<'a> {
    pub pictures: &'a [String],
    /// In the label at the picture's top left corner.
    pub map: &'a str,
    /// Large at the picture's foot, after its icon.
    pub mode: &'a str,
    pub icon: Option<Handle<Image>>,
    /// Small at the picture's top right (what the lobby is doing).
    pub status: &'a str,
    pub pulse: bool,
    pub w: f32,
    pub pic_h: f32,
}

/// The selected map and mode, as the owner's reference lays them out: the
/// map's name in a label on the picture's top left corner, the mode's icon
/// and name over the picture's foot; an accent edge and rule in our look.
pub(crate) fn mode_card(c: &mut Kids, cx: &mut Ctx, k: ModeCard, left: f32, bottom: f32) {
    let acc = cx.pal.accent;
    let p = cx.pal.panel.to_srgba();
    c.spawn((Node { width: cx.px(k.w), flex_direction: FlexDirection::Column, ..abs(Some(left), None, None, Some(bottom), cx) }, anim::rise(cx.s, 36.0, 0.08))).with_children(|card| {
        card.spawn((
            Node { align_self: AlignSelf::FlexStart, height: cx.px(32.0), padding: UiRect::horizontal(cx.px(12.0)), column_gap: cx.px(10.0), align_items: AlignItems::Center, border: UiRect::top(cx.px(2.0)), ..default() },
            BorderColor::all(acc),
            BackgroundColor(Color::srgba(p.red, p.green, p.blue, 0.94)),
        ))
        .with_children(|l| {
            l.spawn((Node { width: cx.px(8.0), height: cx.px(8.0), ..default() }, BackgroundColor(acc)));
            l.spawn((Text::new(k.map.to_owned()), cx.bold(21.0), TextColor(WHITE)));
        });
        card.spawn((Node { width: cx.px(k.w), height: cx.px(k.pic_h), overflow: Overflow::clip(), border: UiRect::left(cx.px(3.0)), ..default() }, BorderColor::all(acc))).with_children(|pic| {
            picture_box(pic, cx, k.pictures, k.w - 3.0, k.pic_h, ());
            let shadow = TextShadow { offset: Vec2::splat(2.0 * cx.s), color: Color::srgba(0.0, 0.0, 0.0, 0.85), ..default() };
            if !k.status.is_empty() {
                pic.spawn((
                    Node { position_type: PositionType::Absolute, right: Val::Px(0.0), top: Val::Px(0.0), padding: UiRect::axes(cx.px(12.0), cx.px(6.0)), ..default() },
                    fade_right(Color::srgba(0.0, 0.0, 0.0, 0.0), Color::srgba(0.0, 0.0, 0.0, 0.60)),
                ))
                .with_children(|t| {
                    let mut e = t.spawn((Text::new(k.status.to_owned()), cx.bold(20.0), TextColor(TEXT), shadow));
                    if k.pulse {
                        e.insert(anim::Pulse { base: TEXT });
                    }
                });
            }
            pic.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    height: Val::Percent(55.0),
                    align_items: AlignItems::FlexEnd,
                    column_gap: cx.px(14.0),
                    padding: UiRect::new(cx.px(14.0), cx.px(14.0), Val::Px(0.0), cx.px(12.0)),
                    ..default()
                },
                BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.0), Val::Percent(0.0)), ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.78), Val::Percent(100.0))])),
            ))
            .with_children(|m| {
                if let Some(h) = k.icon.clone() {
                    m.spawn((ImageNode::new(h), Node { width: cx.px(72.0), height: cx.px(72.0), flex_shrink: 0.0, ..default() }));
                }
                m.spawn((Text::new(k.mode.to_owned()), cx.bold(34.0), TextColor(WHITE), shadow, Node { margin: UiRect::bottom(cx.px(6.0)), ..default() }));
            });
        });
        card.spawn((Node { width: cx.px(k.w), height: cx.px(5.0), ..default() }, fade_right(acc.with_alpha(0.9), acc.with_alpha(0.0))));
    });
}

/// A panel's dark ground with a thin accent rule along its top.
pub(crate) fn panel_ground(cx: &Ctx) -> (BackgroundGradient, BorderColor) {
    let p = cx.pal.panel.to_srgba();
    (
        BackgroundGradient::from(LinearGradient::to_bottom(vec![
            ColorStop::new(Color::srgba(p.red, p.green, p.blue, 0.86), Val::Percent(0.0)),
            ColorStop::new(Color::srgba(p.red, p.green, p.blue, 0.62), Val::Percent(100.0)),
        ])),
        BorderColor { top: cx.pal.accent_a(0.75), ..BorderColor::all(cx.pal.line) },
    )
}

/// The soldier preview's place: a soft glow in the accent, a dim figure's
/// outline and SOLDIER PREVIEW faint at its foot. No soldier renders exist
/// yet: a render or picture goes in its `SoldierSlot` node later. It
/// slides over to the left as the stats panel comes in.
#[derive(Component)]
pub(crate) struct SoldierSlot;

pub(crate) fn soldier_preview(c: &mut Kids, cx: &Ctx, left: f32, top: f32, w: f32, h: f32) {
    let acc = cx.pal.accent;
    c.spawn((
        SoldierSlot,
        Node { width: cx.px(w), height: cx.px(h), justify_content: JustifyContent::Center, align_items: AlignItems::FlexEnd, ..abs(Some(left), Some(top), None, None, cx) },
        BackgroundGradient::from(RadialGradient {
            stops: vec![ColorStop::new(acc.with_alpha(0.20), Val::Percent(0.0)), ColorStop::new(acc.with_alpha(0.05), Val::Percent(45.0)), ColorStop::new(acc.with_alpha(0.0), Val::Percent(72.0))],
            ..default()
        }),
        anim::slide(cx.s, Vec2::new(220.0, 0.0), 0.0),
    ))
    .with_children(|s| {
        // The figure: head, shoulders and body, legs, as dim shapes.
        let ink = WHITE.with_alpha(0.05);
        let rim = WHITE.with_alpha(0.10);
        let part = |s: &mut Kids, l: f32, t: f32, pw: f32, ph: f32, r: f32| {
            s.spawn((
                Node { width: cx.px(pw), height: cx.px(ph), border: UiRect::all(cx.px(1.0)), border_radius: BorderRadius::all(cx.px(r)), ..abs(Some(l), Some(t), None, None, cx) },
                BackgroundColor(ink),
                BorderColor::all(rim),
            ));
        };
        let mid = w * 0.5;
        part(s, mid - 34.0, h * 0.10, 68.0, 80.0, 34.0);
        part(s, mid - 90.0, h * 0.10 + 92.0, 180.0, h * 0.36, 18.0);
        part(s, mid - 78.0, h * 0.10 + 100.0 + h * 0.36, 70.0, h * 0.36, 10.0);
        part(s, mid + 8.0, h * 0.10 + 100.0 + h * 0.36, 70.0, h * 0.36, 10.0);
        s.spawn((Text::new("SOLDIER PREVIEW"), cx.bold(20.0), TextColor(WHITE.with_alpha(0.22)), Node { margin: UiRect::bottom(cx.px(8.0)), ..default() }));
    });
}

/// What [`stats_page`] shows.
pub(crate) struct Stats<'a> {
    pub name: &'a str,
    /// Under the name (the rank's title).
    pub rank_title: String,
    pub rank_icon: Vec<String>,
    /// The big number by the rank icon.
    pub level: String,
    pub card: String,
    pub emblem: String,
    /// The lines and bar under the banner: (text, progress 0..1, a note).
    pub progress: (String, f32, String),
    pub chart_title: &'a str,
    /// The last five games' values (oldest first; fewer: empty columns).
    pub chart: Vec<f32>,
    /// The chart's values are whole numbers (rounds), labelled `12`, not
    /// `12.00`.
    pub chart_whole: bool,
    pub list: Vec<(String, String)>,
}

/// The stats panel at the right: the banner (round emblem, name and rank
/// over the calling card, the rank icon and the big level), the progress,
/// then `body` (the chart and the list, or what the row in focus shows).
pub(crate) fn stats_panel(c: &mut Kids, cx: &mut Ctx, st: &Stats, body: impl FnOnce(&mut Kids, &mut Ctx)) {
    let w = 640.0;
    let acc = cx.pal.accent;
    c.spawn((
        Node { width: cx.px(w), flex_direction: FlexDirection::Column, row_gap: cx.px(10.0), ..abs(None, Some(176.0), Some(90.0), None, cx) },
        anim::from_left(cx.s, -160.0, 0.05),
    ))
    .with_children(|p| {
        // The banner.
        p.spawn(Node { width: cx.px(w), height: cx.px(124.0), align_items: AlignItems::Center, ..default() }).with_children(|b| {
            b.spawn(Node { position_type: PositionType::Absolute, left: cx.px(70.0), top: cx.px(14.0), width: cx.px(w - 70.0), height: cx.px(96.0), overflow: Overflow::clip(), ..default() }).with_children(|back| {
                banner(back, cx, &st.card, w - 70.0, 96.0, None);
                back.spawn((
                    Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(0.0), ..default() },
                    fade_right(Color::srgba(0.0, 0.0, 0.0, 0.88), Color::srgba(0.0, 0.0, 0.0, 0.25)),
                ));
                back.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(80.0), Some(8.0), None, None, cx) }).with_children(|t| {
                    t.spawn((Text::new(st.name.to_owned()), cx.bold(28.0), TextColor(WHITE)));
                    t.spawn((Text::new(st.rank_title.clone()), cx.regular(22.0), TextColor(TEXT)));
                });
                back.spawn(Node { column_gap: cx.px(8.0), align_items: AlignItems::Center, ..abs(None, None, Some(18.0), Some(4.0), cx) }).with_children(|l| {
                    if let Some(h) = cx.pic(&st.rank_icon) {
                        l.spawn((ImageNode::new(h), Node { width: cx.px(48.0), height: cx.px(48.0), ..default() }));
                    }
                    l.spawn((Text::new(st.level.clone()), cx.bold(60.0), TextColor(WHITE), TextShadow { offset: Vec2::splat(2.0 * cx.s), color: Color::srgba(0.0, 0.0, 0.0, 0.8), ..default() }));
                });
            });
            // The round emblem over the banner's left end.
            b.spawn((
                Node { width: cx.px(124.0), height: cx.px(124.0), border: UiRect::all(cx.px(2.0)), border_radius: BorderRadius::all(Val::Percent(50.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                BorderColor::all(acc.with_alpha(0.8)),
                BackgroundGradient::from(RadialGradient {
                    stops: vec![ColorStop::new(acc.with_alpha(0.35), Val::Percent(0.0)), ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.92), Val::Percent(100.0))],
                    ..default()
                }),
            ))
            .with_children(|e| emblem_bare(e, cx, &st.emblem, 84.0));
        });
        // The progress.
        p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: cx.px(4.0), padding: UiRect::horizontal(cx.px(10.0)), ..default() }).with_children(|t| {
            t.spawn((Text::new(st.progress.0.clone()), cx.bold(20.0), TextColor(acc)));
            t.spawn((Node { width: Val::Percent(100.0), height: cx.px(5.0), ..default() }, BackgroundColor(KEYCAP)))
                .with_child((Node { width: Val::Percent(st.progress.1 * 100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(acc)));
            if !st.progress.2.is_empty() {
                t.spawn((Text::new(st.progress.2.clone()), cx.regular(18.0), TextColor(DIM)));
            }
        });
        body(p, cx);
    });
}

/// The five-game chart: its title, five columns (a bar each, from the
/// foot), a value under each (0.00, or 0 when `whole`, when no game is in
/// yet).
pub(crate) fn chart(c: &mut Kids, cx: &Ctx, title: &str, values: &[f32], whole: bool) {
    let acc = cx.pal.accent;
    let top = values.iter().copied().fold(1.0_f32, f32::max);
    c.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: cx.px(4.0), margin: UiRect::top(cx.px(6.0)), ..default() }).with_children(|col| {
        col.spawn((Text::new(title.to_owned()), cx.regular(22.0), TextColor(TEXT)));
        col.spawn((
            Node { width: Val::Percent(94.0), height: cx.px(100.0), align_items: AlignItems::FlexEnd, border: UiRect::bottom(cx.px(2.0)), ..default() },
            BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::new(WHITE.with_alpha(0.03), Val::Percent(0.0)), ColorStop::new(WHITE.with_alpha(0.08), Val::Percent(100.0))])),
            BorderColor::all(WHITE.with_alpha(0.35)),
        ))
        .with_children(|b| {
            for k in 0..5 {
                b.spawn((
                    Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::FlexEnd, border: UiRect::left(cx.px(if k == 0 { 0.0 } else { 1.0 })), ..default() },
                    BorderColor::all(WHITE.with_alpha(0.10)),
                ))
                .with_children(|cell| {
                    if let Some(v) = values.get(k) {
                        cell.spawn((Node { width: Val::Percent(40.0), height: Val::Percent((v / top * 100.0).clamp(2.0, 100.0)), ..default() }, fade_right(acc.with_alpha(0.55), acc.with_alpha(0.85))));
                    }
                });
            }
        });
        col.spawn(Node { width: Val::Percent(94.0), ..default() }).with_children(|l| {
            for k in 0..5 {
                l.spawn(Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() }).with_children(|t| {
                    t.spawn((Node { width: cx.px(6.0), height: cx.px(6.0), margin: UiRect::top(cx.px(-4.0)), border_radius: BorderRadius::all(Val::Percent(50.0)), ..default() }, BackgroundColor(acc)));
                    t.spawn((Text::new({
                        let v = values.get(k).copied().unwrap_or(0.0);
                        if whole { format!("{v:.0}") } else { format!("{v:.2}") }
                    }), cx.regular(18.0), TextColor(DIM)));
                });
            }
        });
    });
}

/// The ruled list of numbers: the label at the left, the value in a dark
/// box at the right.
pub(crate) fn stat_list(c: &mut Kids, cx: &Ctx, list: &[(String, String)]) {
    c.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::horizontal(cx.px(14.0)), margin: UiRect::top(cx.px(6.0)), ..default() }).with_children(|col| {
        for (i, (k, v)) in list.iter().enumerate() {
            col.spawn((
                Node { height: cx.px(32.0), justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, border: UiRect::bottom(cx.px(1.0)), ..default() },
                BorderColor::all(cx.pal.line),
                anim::from_left(cx.s, -40.0, 0.08 + i as f32 * 0.015),
            ))
            .with_children(|r| {
                r.spawn((Text::new(k.clone()), cx.regular(22.0), TextColor(TEXT)));
                r.spawn((Node { width: cx.px(220.0), height: cx.px(26.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55))))
                    .with_child((Text::new(v.clone()), cx.bold(20.0), TextColor(WHITE)));
            });
        }
    });
}

/// PLAYER STATS / COMBAT RECORD's standard body: the chart and the list.
pub(crate) fn stats_body(c: &mut Kids, cx: &Ctx, st: &Stats) {
    chart(c, cx, st.chart_title, &st.chart, st.chart_whole);
    stat_list(c, cx, &st.list);
}

/// Ghosts' FIND GAME / FIND MATCH list: plain lines in a panel, the one in
/// focus a light band in the accent (widening in from the left); a value
/// at the line's right (a selector's between its arrows).
pub(crate) fn plain_list(c: &mut Kids, cx: &Ctx, rows: &[Row], focus: usize, width: f32, row_h: f32) {
    let acc = cx.pal.accent;
    for (i, r) in rows.iter().enumerate() {
        let on = i == focus;
        c.spawn((
            Hit::Row(i),
            Button,
            Node {
                width: cx.px(width),
                height: cx.px(row_h),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: if r.value.is_some() { JustifyContent::SpaceBetween } else { JustifyContent::Center },
                padding: UiRect::horizontal(cx.px(12.0)),
                margin: UiRect::bottom(cx.px(2.0)),
                ..default()
            },
            BackgroundColor(WHITE.with_alpha(if on { 0.0 } else { 0.03 })),
            anim::from_left(cx.s, 50.0, 0.03 + i as f32 * 0.012),
        ))
        .with_children(|line| {
            if on {
                let (g, w) = anim::grow();
                line.spawn((
                    Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(0.0), width: w, ..default() },
                    fade_right(lift(acc, 0.10).with_alpha(0.90), lift(acc, 0.10).with_alpha(0.55)),
                    g,
                ));
            }
            let px = row_h * 0.62;
            line.spawn((Text::new(r.label.clone()), if on { cx.bold(px) } else { cx.regular(px) }, TextColor(if on { WHITE } else { TEXT.with_alpha(0.85) })));
            match &r.value {
                Some(v) if !matches!(r.act, Act::Cycle(_)) => {
                    let lit = v == "ON";
                    line.spawn((Text::new(v.clone()), cx.bold(px * 0.8), TextColor(if on { WHITE } else if lit { lift(acc, 0.2) } else { FAINT })));
                }
                Some(v) => {
                    line.spawn(Node { column_gap: cx.px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|sel| {
                        sel.spawn((Hit::Step(i, -1), Button, Text::new("‹"), cx.bold(px), TextColor(if on { WHITE } else { FAINT })));
                        sel.spawn((Text::new(v.clone()), cx.bold(px * 0.8), TextColor(if on { WHITE } else { DIM })));
                        sel.spawn((Hit::Step(i, 1), Button, Text::new("›"), cx.bold(px), TextColor(if on { WHITE } else { FAINT })));
                    });
                }
                None => {}
            }
        });
    }
}

/// The FIND GAME panel's ground: dark, from the screen's left edge down to
/// the notice bar, sliding in from the left.
pub(crate) fn popup_ground(c: &mut Kids, cx: &Ctx, width: f32) {
    let p = cx.pal.panel.to_srgba();
    c.spawn((
        // (From the window's left and top edges, past the safe frame.)
        Node { width: cx.px(width + cx.bleed.x), border: UiRect::right(cx.px(1.0)), ..abs(Some(-cx.bleed.x), Some(-cx.bleed.y), None, Some(114.0), cx) },
        BackgroundGradient::from(LinearGradient::to_right(vec![
            ColorStop::new(Color::srgba(p.red, p.green, p.blue, 0.92), Val::Percent(0.0)),
            ColorStop::new(Color::srgba(p.red, p.green, p.blue, 0.80), Val::Percent(100.0)),
        ])),
        BorderColor::all(cx.pal.line),
        anim::from_left(cx.s, width * 0.3, 0.0),
    ));
}

/// One band in [`rows`]' look with a hit of its own (a question's buttons,
/// a picker's categories, the pause menu's rows under a question): a ruled
/// top, the label at its right end, a cut corner; in focus the band in the
/// accent. `faint`: drawn as a row that does nothing (dim text, fainter
/// ground). The column it goes in starts at the screen's left edge.
#[allow(clippy::too_many_arguments)]
pub(crate) fn band(c: &mut Kids, cx: &Ctx, label: &str, on: bool, hit: Option<Hit>, look: Look, delay: f32, faint: bool) {
    let acc = cx.pal.accent;
    let tip = look.h * 0.5;
    let end = band_end(cx);
    let end = if faint { end.with_alpha(end.alpha() * 0.5) } else { end };
    let mut e = c.spawn((
        Node { width: cx.px(look.width), height: cx.px(look.h), flex_shrink: 0.0, margin: UiRect::bottom(cx.px(4.0)), ..default() },
        anim::from_left(cx.s, 110.0, delay),
    ));
    if let Some(hit) = hit {
        e.insert((hit, Button));
    }
    e.with_children(|row| {
        row.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                right: cx.px(tip),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexEnd,
                padding: UiRect::right(cx.px(36.0)),
                border: UiRect::top(cx.px(1.0)),
                ..default()
            },
            BorderColor::all(if on { lift(acc, 0.35) } else { cx.pal.line }),
            if on { fade_right(acc.with_alpha(0.0), acc.with_alpha(if faint { 0.45 } else { 0.85 })) } else { fade_right(end.with_alpha(0.0), end) },
        ))
        .with_child((
            Text::new(label.to_owned()),
            if on { cx.bold(look.px) } else { cx.regular(look.px) },
            TextColor(match (on, faint) {
                (true, false) => WHITE,
                (true, true) => TEXT,
                (false, false) => TEXT.with_alpha(0.80),
                (false, true) => FAINT,
            }),
        ));
        bevel(row, cx, look.h, if on { acc.with_alpha(if faint { 0.45 } else { 0.85 }) } else { end });
    });
}

/// A question over the screen, as Ghosts asks one: the screen dimmed, a
/// dark band right across it with an accent rule along its top, the title
/// behind an accent bar at the titles' inset, what it means under it, and
/// the buttons (`buttons`: a column from the screen's left edge, bands as
/// [`rows`] or [`band`]) under that. `z`: the shade's `GlobalZIndex` (the
/// band is one over it).
pub(crate) fn popup(c: &mut Kids, cx: &Ctx, title: &str, body: &str, z: i32, buttons: impl FnOnce(&mut Kids, &Ctx)) {
    let [r, g, b] = cx.pal.shade;
    let p = cx.pal.panel.to_srgba();
    let acc = cx.pal.accent;
    c.spawn((
        bleed(cx),
        BackgroundColor(Color::srgba(r, g, b, 0.72)),
        GlobalZIndex(z),
    ));
    let h = 380.0;
    c.spawn((
        // (The band runs out to the window's edges; its words and buttons
        // keep to the safe frame.)
        Node {
            min_height: cx.px(h),
            flex_direction: FlexDirection::Column,
            padding: UiRect::new(cx.px(INSET + cx.bleed.x), cx.px(INSET + cx.bleed.x), cx.px(34.0), cx.px(30.0)),
            row_gap: cx.px(10.0),
            border: UiRect::vertical(cx.px(1.0)).with_top(cx.px(2.0)),
            ..abs(Some(-cx.bleed.x), Some(((cx.h - h) * 0.5).max(0.0)), Some(-cx.bleed.x), None, cx)
        },
        fade_right(Color::srgba(p.red, p.green, p.blue, 0.98), Color::srgba(p.red, p.green, p.blue, 0.86)),
        BorderColor { top: acc.with_alpha(0.75), ..BorderColor::all(cx.pal.line) },
        GlobalZIndex(z + 1),
        anim::from_left(cx.s, 80.0, 0.0),
    ))
    .with_children(|band| {
        band.spawn(Node { column_gap: cx.px(14.0), align_items: AlignItems::Center, ..default() }).with_children(|t| {
            t.spawn((Node { width: cx.px(7.0), height: cx.px(40.0), ..default() }, BackgroundColor(acc)));
            t.spawn((Text::new(title.to_owned()), cx.regular(50.0), TextColor(WHITE)));
        });
        band.spawn((Text::new(body.to_owned()), cx.regular(24.0), TextColor(TEXT), Node { max_width: cx.px(1100.0), ..default() }));
        band.spawn(Node { flex_grow: 1.0, min_height: cx.px(18.0), ..default() });
        band.spawn(Node { flex_direction: FlexDirection::Column, margin: UiRect::left(cx.px(-INSET)), ..default() }).with_children(|col| buttons(col, cx));
    });
}

/// A panel at the right in the Ghosts look: the dark ground with its accent
/// rule (`panel_ground`), sliding in from the right. `top`, `right` and
/// `width` in design units.
pub(crate) fn side_panel(c: &mut Kids, cx: &Ctx, top: f32, right: f32, width: f32, body: impl FnOnce(&mut Kids)) {
    let (ground, rim) = panel_ground(cx);
    c.spawn((
        Node { width: cx.px(width), flex_direction: FlexDirection::Column, row_gap: cx.px(12.0), padding: UiRect::all(cx.px(28.0)), border: UiRect::top(cx.px(2.0)), ..abs(None, Some(top), Some(right), None, cx) },
        ground,
        rim,
        anim::from_left(cx.s, -80.0, 0.04),
    ))
    .with_children(body);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The safe frame at the usual window sizes: inset 80/50 at 1080p (in
    /// proportion at 720p and 1440p), the 1080-high design fitting its
    /// height, and at least the 1920-wide design across it (Create-a-Class
    /// lays out to x 1794), inside the window.
    #[test]
    fn safe_frame_fits() {
        for (wpx, hpx) in [(1280.0, 720.0), (1920.0, 1080.0), (2560.0, 1440.0)] {
            let area = Vec2::new(wpx, hpx);
            let a = safe_area(area, hpx / 1080.0);
            let k = hpx / 1080.0;
            assert!((a.inset.x - SAFE_X * k).abs() < 1e-3 && (a.inset.y - SAFE_Y * k).abs() < 1e-3);
            assert!((a.h - 1080.0).abs() < 1e-2, "{wpx}x{hpx}: h {}", a.h);
            assert!(a.w >= 1920.0, "{wpx}x{hpx}: w {}", a.w);
            // The frame in pixels, plus its margins, is the window.
            assert!((a.w * a.s + 2.0 * a.inset.x - wpx).abs() < 1e-2);
            assert!((a.h * a.s + 2.0 * a.inset.y - hpx).abs() < 1e-2);
            // The bleed reaches exactly the window's edges.
            assert!((a.bleed.x * a.s - a.inset.x).abs() < 1e-3 && (a.bleed.y * a.s - a.inset.y).abs() < 1e-3);
        }
        // The margin at 1080p, in pixels.
        let a = safe_area(Vec2::new(1920.0, 1080.0), 1.0);
        assert_eq!(a.inset, Vec2::new(80.0, 50.0));
    }
}
