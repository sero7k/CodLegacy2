//! bo2mp shell: OPTIONS as Ghosts lays it out, in the screen's own palette
//! (Multiplayer blue, or Zombies red when opened from the Zombies menu):
//!
//! - the main page: the title behind its accent bar, a dark category panel
//!   at the left (a category a tab of ours, `options::CATEGORIES`; the one he
//!   is on a solid band in the accent, Q / E or a click changes it), and its
//!   settings card at the right: a header with a cut corner, then ruled rows,
//!   the label at the left and the value in the right half (a value between
//!   its arrows, a slider bar with its number for the volumes, brightness,
//!   sensitivities and dead zones, nothing for a row that opens a page);
//! - a sub-page (VIDEO OPTIONS > ADVANCED VIDEO): the rows across the left of
//!   the screen, the label right-aligned in the first column and the value
//!   centred in the second, each a ruled strip ending in a cut corner;
//! - MOUSE & KEYBOARD > KEY BINDINGS (`keys`): one wide card, its ruled rows
//!   under section headings (MOVEMENT, COMBAT, INTERFACE), the action at the
//!   left and its keys at the right (PRESS A KEY in the accent while it
//!   waits for one), RESET TO DEFAULT last, the keys to use under it.
//!
//! Both scroll a window of rows that keeps the focus in view
//! (`options::list_window`: pointing at a row never scrolls it), with a
//! chevron at the top or foot while more rows are above or below and a
//! scroll bar at the right (the wheel and the bar scroll it,
//! `options_scroll`). The line about the row in focus goes under them.

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use super::anim;
use super::keys::{self, KEYS_PAGE};
use super::options::{self, ADVANCED_PAGE, CATEGORIES};
use super::theme::*;
use super::view::{Ctx, Kids, abs, header, lift};
use super::{Act, Hit, Row, Screen, Setting, Shell};

/// The category panel and the card (design units).
const PANEL_LEFT: f32 = 110.0;
const PANEL_W: f32 = 340.0;
const TOP: f32 = 196.0;
const ENTRY_H: f32 = 60.0;
const CARD_LEFT: f32 = 480.0;
const CARD_W: f32 = 760.0;
const HEAD_H: f32 = 54.0;
const ROW_H: f32 = 46.0;
/// Rows a card shows at once.
const CARD_ROWS: usize = 11;

/// A sub-page's strips.
const LIST_W: f32 = 780.0;
const LABEL_W: f32 = 400.0;
const STRIP_H: f32 = 50.0;
const PAGE_ROWS: usize = 11;

/// The OPTIONS screen (its page: a category, or a sub-page).
pub(crate) fn view(c: &mut Kids, cx: &mut Ctx, shell: &Shell, rows: &[Row]) {
    if shell.page == ADVANCED_PAGE {
        sub_page(c, cx, shell, rows, options::ADVANCED_TITLE);
    } else if shell.page == options::PREPARE_PAGE {
        sub_page(c, cx, shell, rows, options::PREPARE_TITLE);
    } else if shell.page == KEYS_PAGE {
        keys_page(c, cx, shell, rows);
    } else {
        main_page(c, cx, shell, rows);
    }
}

/// A triangle in the accent pointing up or down: more rows that way.
fn chevron(c: &mut Kids, cx: &Ctx, up: bool, size: f32) {
    let side = size * std::f32::consts::SQRT_2;
    let cy = if up { size } else { 0.0 };
    c.spawn(Node { width: cx.px(size * 2.0), height: cx.px(size), overflow: Overflow::clip(), flex_shrink: 0.0, ..default() }).with_child((
        Node { position_type: PositionType::Absolute, left: cx.px(size - side * 0.5), top: cx.px(cy - side * 0.5), width: cx.px(side), height: cx.px(side), ..default() },
        BackgroundColor(cx.pal.accent),
        UiTransform::from_rotation(Rot2::degrees(45.0)),
    ));
}

/// A line holding a chevron at its right end (or nothing, keeping its
/// height, when there are no more rows that way).
fn chevron_line(c: &mut Kids, cx: &Ctx, up: bool, shown: bool, width: f32, right: f32) {
    c.spawn(Node { width: cx.px(width), height: cx.px(26.0), flex_shrink: 0.0, justify_content: JustifyContent::FlexEnd, align_items: AlignItems::Center, padding: UiRect::right(cx.px(right)), ..default() })
        .with_children(|l| {
            if shown {
                chevron(l, cx, up, 11.0);
            }
        });
}

/// The rows a list `max` rows high shows (`options::list_window`, from
/// where it was): kept for the wheel and the scroll bar.
fn shown_rows(n: usize, focus: usize, max: usize) -> (usize, usize) {
    use std::sync::atomic::Ordering::Relaxed;
    let (first, last) = options::list_window(super::OPT_FIRST.load(Relaxed), n, focus, max);
    super::OPT_FIRST.store(first, Relaxed);
    super::OPT_SHOWN.store(max, Relaxed);
    super::OPT_ROWS.store(n, Relaxed);
    (first, last)
}

/// A list's scroll bar's track (`options_scroll` drags it).
#[derive(Component)]
pub(crate) struct ScrollTrack;

/// A scroll bar beside a list, as tall as it: a faint track, the thumb
/// where the rows shown are among them all (nothing when they all fit).
/// Drag the thumb or click the track.
fn scroll_bar(c: &mut Kids, cx: &Ctx, first: usize, shown: usize, n: usize) {
    if n <= shown {
        return;
    }
    let pct = |k: usize| Val::Percent(k as f32 * 100.0 / n as f32);
    c.spawn((
        ScrollTrack,
        Button,
        RelativeCursorPosition::default(),
        Node { width: cx.px(12.0), flex_shrink: 0.0, margin: UiRect::left(cx.px(8.0)), ..default() },
        BackgroundColor(WHITE.with_alpha(0.08)),
    ))
    .with_child((
        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), top: pct(first), height: pct(shown), ..default() },
        BackgroundColor(cx.pal.accent.with_alpha(0.85)),
    ));
}

/// bo2mp: OPTIONS' list scrolls with the mouse wheel (a row a notch) and its
/// scroll bar (drag it, or click the track to jump there); the focus moves
/// onto a row it shows. Pointing at a row never scrolls it (F2: GAME FOLDERS
/// ran to its foot when he pointed at Modern Warfare 3, and had no bar).
pub(crate) fn options_scroll(
    mut shell: ResMut<Shell>,
    wheel: Res<AccumulatedMouseScroll>,
    mouse: Res<ButtonInput<MouseButton>>,
    tracks: Query<&RelativeCursorPosition, With<ScrollTrack>>,
    mut dragging: Local<bool>,
    mut spin: Local<f32>,
) {
    use std::sync::atomic::Ordering::Relaxed;
    let (shown, n) = (super::OPT_SHOWN.load(Relaxed), super::OPT_ROWS.load(Relaxed));
    let idle = shell.screen != Screen::Options
        || shell.ask.is_some()
        || !super::enabled()
        || crate::game_folders::is_open()
        || n <= shown
        || (shell.page == KEYS_PAGE && keys::snapshot().listening.is_some());
    if idle {
        *dragging = false;
        *spin = 0.0;
        return;
    }
    let top = n - shown;
    let was = super::OPT_FIRST.load(Relaxed).min(top);
    // The wheel (up scrolls up; a touchpad's pixels: 40 a row).
    *spin += match wheel.unit {
        MouseScrollUnit::Line => wheel.delta.y,
        MouseScrollUnit::Pixel => wheel.delta.y / 40.0,
    };
    let steps = spin.trunc();
    *spin -= steps;
    let mut first = (was as i64 - steps as i64).clamp(0, top as i64) as usize;
    // The bar: held from a press on its track, the thumb follows the mouse.
    if mouse.just_pressed(MouseButton::Left) && tracks.iter().any(|t| t.cursor_over()) {
        *dragging = true;
    }
    if !mouse.pressed(MouseButton::Left) {
        *dragging = false;
    }
    if *dragging && let Some(at) = tracks.iter().find_map(|t| t.normalized) {
        let y = (at.y + 0.5).clamp(0.0, 1.0);
        first = ((y * n as f32 - shown as f32 * 0.5).round().max(0.0) as usize).min(top);
    }
    if first != was {
        super::OPT_FIRST.store(first, Relaxed);
        let focus = shell.focus.clamp(first, first + shown - 1);
        if shell.focus != focus {
            shell.focus = focus;
        }
    }
}

/// What the row's value shows: a slider bar and its number, or the value
/// alone; a selector's between its arrows (hit targets that step it).
fn value_cell(c: &mut Kids, cx: &Ctx, i: usize, r: &Row, tab: usize, on: bool, px: f32) {
    let ink = if on { WHITE } else { TEXT.with_alpha(0.80) };
    let Some(v) = &r.value else { return };
    let k = match r.act {
        Act::Cycle(Setting::Option(k)) => Some(k),
        _ => None,
    };
    let bar = k.and_then(|k| options::bar(tab, k, &options::snapshot()));
    // (PREPARE MAPS: a game's checkbox and its maps still to make and their time.)
    if let Some(checked) = k.and_then(|k| options::checkbox(tab, k, &options::snapshot())) {
        c.spawn(Node { column_gap: cx.px(14.0), align_items: AlignItems::Center, ..default() }).with_children(|cell| {
            cell.spawn((
                Hit::Step(i, 1),
                Button,
                Node { width: cx.px(24.0), height: cx.px(24.0), flex_shrink: 0.0, border: UiRect::all(cx.px(2.0)), padding: UiRect::all(cx.px(4.0)), ..default() },
                BorderColor::all(if on { WHITE } else { TEXT.with_alpha(0.80) }),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            ))
            .with_children(|b| {
                if checked {
                    b.spawn((Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(if on { WHITE } else { cx.pal.accent })));
                }
            });
            cell.spawn(Node { min_width: cx.px(250.0), ..default() })
                .with_child((Text::new(v.clone()), cx.regular(px * 0.78), TextColor(if checked { ink } else { DIM })));
        });
        return;
    }
    let arrow = |c: &mut Kids, by: i32, glyph: &str| {
        c.spawn((Hit::Step(i, by), Button, Text::new(glyph.to_owned()), cx.bold(px), TextColor(if on { WHITE } else { FAINT })));
    };
    c.spawn(Node { column_gap: cx.px(14.0), align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default() }).with_children(|cell| {
        if k.is_some() {
            arrow(cell, -1, "‹");
        }
        match bar {
            Some(f) => {
                cell.spawn((
                    Node { width: cx.px(200.0), height: cx.px(18.0), border: UiRect::all(cx.px(2.0)), padding: UiRect::all(cx.px(2.0)), ..default() },
                    BorderColor::all(WHITE.with_alpha(if on { 0.95 } else { 0.70 })),
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.70)),
                ))
                .with_child((Node { width: Val::Percent(f * 100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(if on { WHITE } else { TEXT.with_alpha(0.80) })));
                cell.spawn(Node { min_width: cx.px(64.0), justify_content: JustifyContent::FlexEnd, ..default() })
                    .with_child((Text::new(v.clone()), cx.bold(px * 0.82), TextColor(ink)));
            }
            None => {
                let faint = matches!(r.act, Act::Info);
                cell.spawn(Node { min_width: cx.px(180.0), justify_content: JustifyContent::Center, ..default() })
                    .with_child((Text::new(v.clone()), cx.regular(px), TextColor(if faint && !on { DIM } else { ink })));
            }
        }
        if k.is_some() {
            arrow(cell, 1, "›");
        }
    });
}

/// The line about the row in focus, behind a small accent bar.
fn sub_line(c: &mut Kids, cx: &Ctx, rows: &[Row], focus: usize, width: f32) {
    if let Some(r) = rows.get(focus).filter(|r| !r.sub.is_empty()) {
        c.spawn((Node { max_width: cx.px(width), column_gap: cx.px(12.0), align_items: AlignItems::FlexStart, ..default() }, anim::from_left(cx.s, 60.0, 0.06))).with_children(|d| {
            d.spawn((Node { width: cx.px(6.0), height: cx.px(20.0), flex_shrink: 0.0, margin: UiRect::top(cx.px(4.0)), ..default() }, BackgroundColor(cx.pal.accent)));
            d.spawn((Text::new(r.sub.clone()), cx.regular(23.0), TextColor(TEXT)));
        });
    }
}

/// The main page: the category panel and the card of the one he is on.
fn main_page(c: &mut Kids, cx: &mut Ctx, shell: &Shell, rows: &[Row]) {
    header(c, cx, &["OPTIONS"]);
    let tab = options::tab_of(shell.page);
    let acc = cx.pal.accent;
    let p = cx.pal.panel.to_srgba();
    // The categories.
    c.spawn((
        Node { width: cx.px(PANEL_W), flex_direction: FlexDirection::Column, padding: UiRect::all(cx.px(8.0)), border: UiRect::all(cx.px(1.0)), ..abs(Some(PANEL_LEFT), Some(TOP), None, Some(130.0), cx) },
        BackgroundGradient::from(LinearGradient::to_bottom(vec![
            ColorStop::new(Color::srgba(p.red, p.green, p.blue, 0.90), Val::Percent(0.0)),
            ColorStop::new(Color::srgba(p.red, p.green, p.blue, 0.72), Val::Percent(100.0)),
        ])),
        BorderColor::all(cx.pal.line),
        anim::from_left(cx.s, 90.0, 0.0),
    ))
    .with_children(|list| {
        for (i, name) in CATEGORIES.iter().enumerate() {
            let on = i == tab;
            list.spawn((
                Hit::Page(i),
                Button,
                Node { height: cx.px(ENTRY_H), flex_shrink: 0.0, align_items: AlignItems::Center, padding: UiRect::left(cx.px(16.0)), margin: UiRect::bottom(cx.px(4.0)), border: UiRect::bottom(cx.px(1.0)), ..default() },
                BorderColor::all(if on { Color::NONE } else { cx.pal.line }),
                BackgroundColor(if on { acc } else { Color::NONE }),
            ))
            .with_child((Text::new((*name).to_owned()), if on { cx.bold(28.0) } else { cx.regular(28.0) }, TextColor(if on { WHITE } else { TEXT.with_alpha(0.85) })));
        }
    });
    // CONTROLLER OPTIONS: the pad and what each button does, at the right
    // (the card narrows for it).
    let room = cx.w - CARD_LEFT - 72.0;
    let width = if tab == 3 {
        super::pad_diagram::view(c, cx, &options::snapshot(), TOP);
        (cx.w - CARD_LEFT - 64.0 - 760.0 - 24.0).clamp(460.0, CARD_W)
    } else {
        room.clamp(460.0, CARD_W)
    };
    let (first, last) = shown_rows(rows.len(), shell.focus, CARD_ROWS);
    c.spawn((Node { width: cx.px(width), flex_direction: FlexDirection::Column, row_gap: cx.px(18.0), ..abs(Some(CARD_LEFT), Some(TOP), None, None, cx) }, anim::from_left(cx.s, 90.0, 0.04))).with_children(|col| {
        col.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|card| {
            card_head(card, cx, CATEGORIES[tab]);
            // The rows.
            card.spawn((
                Node { flex_direction: FlexDirection::Column, padding: UiRect::new(cx.px(6.0), cx.px(6.0), Val::Px(0.0), cx.px(6.0)), border: UiRect::new(cx.px(1.0), cx.px(1.0), Val::Px(0.0), cx.px(1.0)), ..default() },
                BackgroundColor(cx.pal.panel),
                BorderColor::all(cx.pal.line),
            ))
            .with_children(|body| {
                let more = rows.len() > CARD_ROWS;
                if more {
                    chevron_line(body, cx, true, first > 0, width - 14.0, 14.0);
                }
                body.spawn(Node::default()).with_children(|list| {
                    list.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, ..default() }).with_children(|list| {
                        for (i, r) in rows.iter().enumerate().take(last).skip(first) {
                            card_row(list, cx, i, r, tab, i == shell.focus);
                        }
                    });
                    scroll_bar(list, cx, first, CARD_ROWS, rows.len());
                });
                if more {
                    chevron_line(body, cx, false, last < rows.len(), width - 14.0, 14.0);
                }
            });
        });
        sub_line(col, cx, rows, shell.focus, width);
    });
}

/// A card's header: a cut corner at its top left, its title, the accent
/// rule under it.
fn card_head(card: &mut Kids, cx: &Ctx, title: &str) {
    let acc = cx.pal.accent;
    let p = cx.pal.panel.to_srgba();
    let head = Color::srgba(p.red * 0.6, p.green * 0.6, p.blue * 0.6, 0.94);
    let k = HEAD_H * 0.5;
    card.spawn(Node { height: cx.px(HEAD_H), ..default() }).with_children(|h| {
        h.spawn(Node { width: cx.px(k), flex_direction: FlexDirection::Column, flex_shrink: 0.0, ..default() }).with_children(|corner| {
            let side = k * std::f32::consts::SQRT_2;
            corner.spawn(Node { width: cx.px(k), height: cx.px(k), overflow: Overflow::clip(), ..default() }).with_child((
                Node { position_type: PositionType::Absolute, left: cx.px(k - side * 0.5), top: cx.px(k - side * 0.5), width: cx.px(side), height: cx.px(side), ..default() },
                BackgroundColor(head),
                UiTransform::from_rotation(Rot2::degrees(45.0)),
            ));
            corner.spawn((Node { width: cx.px(k), height: cx.px(HEAD_H - k), ..default() }, BackgroundColor(head)));
        });
        h.spawn((Node { flex_grow: 1.0, align_items: AlignItems::Center, padding: UiRect::left(cx.px(6.0)), ..default() }, BackgroundColor(head)))
            .with_child((Text::new(title.to_owned()), cx.bold(30.0), TextColor(WHITE)));
    });
    card.spawn((Node { height: cx.px(2.0), ..default() }, fade_right(acc.with_alpha(0.85), acc.with_alpha(0.15))));
}

/// A card's row: ruled, the label at the left, the value in the right half;
/// the one in focus a band in the accent widening in from the left.
fn card_row(c: &mut Kids, cx: &Ctx, i: usize, r: &Row, tab: usize, on: bool) {
    let acc = cx.pal.accent;
    c.spawn((
        Hit::Row(i),
        Button,
        Node { height: cx.px(ROW_H), flex_shrink: 0.0, align_items: AlignItems::Center, padding: UiRect::horizontal(cx.px(14.0)), border: UiRect::bottom(cx.px(1.0)), ..default() },
        BorderColor::all(cx.pal.line),
    ))
    .with_children(|row| {
        if on {
            let (g, w) = anim::grow();
            row.spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(0.0), width: w, ..default() },
                fade_right(lift(acc, 0.10).with_alpha(0.90), lift(acc, 0.10).with_alpha(0.40)),
                g,
            ));
        }
        row.spawn(Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), ..default() })
            .with_child((Text::new(r.label.clone()), if on { cx.bold(26.0) } else { cx.regular(26.0) }, TextColor(if on { WHITE } else { TEXT.with_alpha(0.85) })));
        row.spawn(Node { flex_grow: 1.15, flex_basis: Val::Px(0.0), justify_content: JustifyContent::Center, ..default() })
            .with_children(|v| value_cell(v, cx, i, r, tab, on, 26.0));
    });
}

/// A page of its own: the rows across the left of the screen.
fn sub_page(c: &mut Kids, cx: &mut Ctx, shell: &Shell, rows: &[Row], title: &str) {
    header(c, cx, &["OPTIONS", title]);
    let tab = options::tab_of(shell.page);
    let acc = cx.pal.accent;
    let tip = STRIP_H * 0.5;
    let p = cx.pal.panel.to_srgba();
    let end = Color::srgba(p.red, p.green, p.blue, 0.70);
    let (first, last) = shown_rows(rows.len(), shell.focus, PAGE_ROWS);
    c.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(0.0), Some(TOP - 10.0), None, None, cx) }).with_children(|col| {
        chevron_line(col, cx, true, first > 0, LIST_W, tip + 26.0);
        col.spawn(Node::default()).with_children(|list| {
            list.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|col| {
                for (i, r) in rows.iter().enumerate().take(last).skip(first) {
                    let on = i == shell.focus;
                    col.spawn((
                        Hit::Row(i),
                        Button,
                        Node { width: cx.px(LIST_W), height: cx.px(STRIP_H), flex_shrink: 0.0, margin: UiRect::bottom(cx.px(6.0)), ..default() },
                        anim::from_left(cx.s, 110.0, (i - first) as f32 * 0.02),
                    ))
                    .with_children(|strip| {
                        strip.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(0.0),
                                top: Val::Px(0.0),
                                bottom: Val::Px(0.0),
                                right: cx.px(tip),
                                align_items: AlignItems::Center,
                                border: UiRect::bottom(cx.px(1.0)),
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
                            body.spawn(Node { width: cx.px(LABEL_W), flex_shrink: 0.0, justify_content: JustifyContent::FlexEnd, ..default() })
                                .with_child((Text::new(r.label.clone()), if on { cx.bold(28.0) } else { cx.regular(28.0) }, TextColor(if on { WHITE } else { TEXT.with_alpha(0.85) })));
                            body.spawn(Node { flex_grow: 1.0, justify_content: JustifyContent::Center, padding: UiRect::left(cx.px(20.0)), ..default() })
                                .with_children(|v| value_cell(v, cx, i, r, tab, on, 28.0));
                        });
                        super::ghosts::bevel(strip, cx, STRIP_H, if on { acc.with_alpha(0.85) } else { end });
                    });
                }
            });
            scroll_bar(list, cx, first, PAGE_ROWS, rows.len());
        });
        chevron_line(col, cx, false, last < rows.len(), LIST_W, tip + 26.0);
        col.spawn(Node { margin: UiRect::left(cx.px(super::ghosts::INSET)), ..default() }).with_children(|s| sub_line(s, cx, rows, shell.focus, LIST_W - super::ghosts::INSET));
    });
}

/// KEY BINDINGS' card and the rows it shows at once.
const KEYS_W: f32 = 1000.0;
const KEY_ROW_H: f32 = 40.0;
const SECTION_H: f32 = 36.0;
const KEY_ROWS: usize = 13;

/// MOUSE & KEYBOARD > KEY BINDINGS: one wide card, the actions under their
/// sections, the action at the left and its keys at the right.
fn keys_page(c: &mut Kids, cx: &mut Ctx, shell: &Shell, rows: &[Row]) {
    header(c, cx, &["OPTIONS", keys::KEYS_TITLE]);
    let acc = cx.pal.accent;
    let width = (cx.w - PANEL_LEFT * 2.0).clamp(640.0, KEYS_W);
    let listening = rows.iter().position(|r| r.value.as_deref() == Some(keys::PRESS));
    let (first, last) = shown_rows(rows.len(), shell.focus, KEY_ROWS);
    c.spawn((Node { width: cx.px(width), flex_direction: FlexDirection::Column, row_gap: cx.px(18.0), ..abs(Some(PANEL_LEFT), Some(TOP - 20.0), None, None, cx) }, anim::from_left(cx.s, 90.0, 0.0))).with_children(|col| {
        col.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|card| {
            card_head(card, cx, keys::KEYS_TITLE);
            card.spawn((
                Node { flex_direction: FlexDirection::Column, padding: UiRect::new(cx.px(6.0), cx.px(6.0), Val::Px(0.0), cx.px(6.0)), border: UiRect::new(cx.px(1.0), cx.px(1.0), Val::Px(0.0), cx.px(1.0)), ..default() },
                BackgroundColor(cx.pal.panel),
                BorderColor::all(cx.pal.line),
            ))
            .with_children(|body| {
                let more = rows.len() > KEY_ROWS;
                if more {
                    chevron_line(body, cx, true, first > 0, width - 14.0, 14.0);
                }
                body.spawn(Node::default()).with_children(|list| {
                    list.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, ..default() }).with_children(|body| {
                        let mut section = None;
                        for (i, r) in rows.iter().enumerate().take(last).skip(first) {
                            let here = keys::section_of(i);
                            if here != section {
                                match here {
                                    Some(k) => section_heading(body, cx, keys::SECTIONS[k]),
                                    // (RESET TO DEFAULT: a gap and a rule above.)
                                    None => {
                                        body.spawn((Node { height: cx.px(2.0), margin: UiRect::vertical(cx.px(8.0)), ..default() }, fade_right(acc.with_alpha(0.55), acc.with_alpha(0.0))));
                                    }
                                }
                                section = here;
                            }
                            key_row(body, cx, i, r, i == shell.focus, listening == Some(i));
                        }
                    });
                    scroll_bar(list, cx, first, KEY_ROWS, rows.len());
                });
                if more {
                    chevron_line(body, cx, false, last < rows.len(), width - 14.0, 14.0);
                }
            });
        });
        // The keys to use (while it waits: how to cancel or clear).
        let hint = match listening.and_then(|i| rows.get(i)) {
            Some(r) => format!("PRESS A KEY FOR {}     ESC  CANCEL     DEL  CLEAR", r.label),
            None => "ENTER / CLICK  REBIND     DEL  CLEAR     ESC  BACK".to_owned(),
        };
        col.spawn(Node { column_gap: cx.px(12.0), align_items: AlignItems::Center, ..default() }).with_children(|h| {
            h.spawn((Node { width: cx.px(6.0), height: cx.px(20.0), flex_shrink: 0.0, ..default() }, BackgroundColor(acc)));
            h.spawn((Text::new(hint), cx.bold(21.0), TextColor(if listening.is_some() { WHITE } else { DIM })));
        });
        if listening.is_none() {
            sub_line(col, cx, rows, shell.focus, width);
        }
    });
}

/// A section's heading on KEY BINDINGS' card: its name in the accent over a
/// rule.
fn section_heading(c: &mut Kids, cx: &Ctx, name: &str) {
    c.spawn((
        Node { height: cx.px(SECTION_H), flex_shrink: 0.0, align_items: AlignItems::FlexEnd, padding: UiRect::new(cx.px(14.0), cx.px(14.0), Val::Px(0.0), cx.px(4.0)), border: UiRect::bottom(cx.px(2.0)), ..default() },
        BorderColor::all(cx.pal.accent.with_alpha(0.70)),
    ))
    .with_child((Text::new(name.to_owned()), cx.bold(21.0), TextColor(lift(cx.pal.accent, 0.35))));
}

/// An action's row: ruled, its name at the left, its keys at the right
/// (UNBOUND dim, PRESS A KEY in the accent); the one in focus a band in the
/// accent widening in from the left.
fn key_row(c: &mut Kids, cx: &Ctx, i: usize, r: &Row, on: bool, waiting: bool) {
    let acc = cx.pal.accent;
    c.spawn((
        Hit::Row(i),
        Button,
        Node { height: cx.px(KEY_ROW_H), flex_shrink: 0.0, align_items: AlignItems::Center, padding: UiRect::horizontal(cx.px(14.0)), border: UiRect::bottom(cx.px(1.0)), ..default() },
        BorderColor::all(cx.pal.line),
        BackgroundColor(if waiting { acc.with_alpha(0.22) } else { Color::NONE }),
    ))
    .with_children(|row| {
        if on {
            let (g, w) = anim::grow();
            row.spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(0.0), width: w, ..default() },
                fade_right(lift(acc, 0.10).with_alpha(0.90), lift(acc, 0.10).with_alpha(0.30)),
                g,
            ));
        }
        row.spawn(Node { flex_grow: 1.0, ..default() })
            .with_child((Text::new(r.label.clone()), if on { cx.bold(24.0) } else { cx.regular(24.0) }, TextColor(if on { WHITE } else { TEXT.with_alpha(0.85) })));
        if let Some(v) = &r.value {
            let unbound = v == keys::UNBOUND;
            let ink = if waiting {
                if on { WHITE } else { lift(acc, 0.35) }
            } else if unbound {
                if on { WHITE.with_alpha(0.75) } else { FAINT }
            } else if on {
                WHITE
            } else {
                TEXT.with_alpha(0.85)
            };
            row.spawn(Node { min_width: cx.px(260.0), justify_content: JustifyContent::FlexEnd, ..default() })
                .with_child((Text::new(v.clone()), if waiting || on { cx.bold(24.0) } else { cx.regular(24.0) }, TextColor(ink)));
        }
    });
}
