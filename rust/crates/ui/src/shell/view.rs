//! bo2mp shell: the screens, drawn again whenever what they show changes
//! (the screen, the row in focus, the lobby's choices, a picture coming in,
//! the online numbers, the window's size).

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::bridge::{Catalog, PlayerCard};
use super::online::{OnlineInfo, Status};
use super::pics::{ShellPics, cover};
use super::theme::*;
use super::cards::{self, Kind};
use super::{Act, Hit, Row, Screen, Shell, Tab, bridge, pages, rows};

#[derive(Component)]
pub(crate) struct ViewRoot;

pub(crate) type Kids<'a> = ChildSpawnerCommands<'a>;

/// What the drawing needs besides the shell's state.
pub(crate) struct Ctx<'a> {
    pub s: f32,
    /// The screen in design units (1080 high).
    pub w: f32,
    pub h: f32,
    pub fonts: &'a ShellFonts,
    pub pics: &'a mut ShellPics,
    pub icons: Option<&'a assets::T6HudIcons>,
    pub images: &'a mut Assets<Image>,
    pub pad: bool,
    /// The screen's colours (blue, or the Zombies screens' red).
    pub pal: Palette,
    /// Calling cards and emblems are drawn still (a picker's page: only
    /// the one in focus and his card move).
    pub still: bool,
    /// bo2mp: how far past the screen's frame the window's edges are, in
    /// design units (the title-safe margin, `ghosts::safe_area`; zero where
    /// the frame is the whole window).
    pub bleed: Vec2,
}

impl Ctx<'_> {
    pub fn px(&self, v: f32) -> Val {
        Val::Px(v * self.s)
    }

    pub fn bold(&self, px: f32) -> TextFont {
        font(&self.fonts.bold, px * self.s)
    }

    pub fn regular(&self, px: f32) -> TextFont {
        font(&self.fonts.regular, px * self.s)
    }

    pub fn light(&self, px: f32) -> TextFont {
        font(&self.fonts.light, px * self.s)
    }

    pub fn black(&self, px: f32) -> TextFont {
        font(&self.fonts.black, px * self.s)
    }

    pub fn pic(&mut self, names: &[String]) -> Option<Handle<Image>> {
        self.pics.first(names, self.icons, self.images)
    }

    pub fn pic_size(&self, h: &Handle<Image>) -> Vec2 {
        self.images.get(h).map_or(Vec2::new(16.0, 9.0), |i| i.size_f32())
    }
}

pub(crate) fn abs(left: Option<f32>, top: Option<f32>, right: Option<f32>, bottom: Option<f32>, cx: &Ctx) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: left.map_or(Val::Auto, |v| cx.px(v)),
        top: top.map_or(Val::Auto, |v| cx.px(v)),
        right: right.map_or(Val::Auto, |v| cx.px(v)),
        bottom: bottom.map_or(Val::Auto, |v| cx.px(v)),
        ..default()
    }
}

/// The title at the top left, as Ghosts' menus: a dark shade across the
/// top with an accent rule under it, and the trail of screens
/// (`MULTIPLAYER / CUSTOM GAME`) behind an accent bar, sliding in.
pub(crate) fn header(c: &mut Kids, cx: &Ctx, trail: &[&str]) {
    header_ex(c, cx, trail, None, false);
}

/// [`header`] with a small picture after the title (`emblem`: the
/// playlist's or mode's mark, as Ghosts' lobby shows its XP mark) and a
/// back chevron before the accent bar (`back`: a hit target, as ESC).
pub(crate) fn header_ex(c: &mut Kids, cx: &Ctx, trail: &[&str], emblem: Option<Handle<Image>>, back: bool) {
    let acc = cx.pal.accent;
    let dark = Color::srgba(0.0, 0.01, 0.03, 0.80);
    c.spawn((super::ghosts::top_shade(cx, 150.0), BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::new(dark, Val::Percent(0.0)), ColorStop::new(dark.with_alpha(0.0), Val::Percent(100.0))]))));
    c.spawn((Node { width: Val::Percent(70.0), height: cx.px(3.0), ..abs(Some(0.0), Some(150.0), None, None, cx) }, fade_right(acc.with_alpha(0.65), acc.with_alpha(0.0))));
    c.spawn((Node { column_gap: cx.px(14.0), align_items: AlignItems::Center, ..abs(Some(if back { 104.0 } else { 150.0 }), Some(60.0), None, None, cx) }, super::anim::from_left(cx.s, 60.0, 0.0))).with_children(|t| {
        if back {
            t.spawn((Hit::Back, Button, Text::new("‹"), cx.bold(50.0), TextColor(WHITE), Node { margin: UiRect::bottom(cx.px(6.0)), ..default() }));
        }
        t.spawn((Node { width: cx.px(7.0), height: cx.px(40.0), ..default() }, BackgroundColor(acc)));
        t.spawn((Text::new(trail.join("  /  ")), cx.regular(46.0), TextColor(WHITE)));
        if let Some(h) = emblem {
            t.spawn((ImageNode::new(h), Node { width: cx.px(58.0), height: cx.px(58.0), margin: UiRect::left(cx.px(8.0)), ..default() }));
        }
    });
}

/// The first word of a shared screen's header (Barracks, Create-a-Class):
/// MULTIPLAYER, or WARZONE when opened from its menus.
pub(crate) fn section(shell: &Shell) -> &'static str {
    if shell.wz { super::wz_theme::HEADER } else { "MULTIPLAYER" }
}

/// A calling card: the picture covering a box, dark to its left so a name
/// over it reads (`name`: the name and its size).
pub(crate) fn banner(c: &mut Kids, cx: &mut Ctx, card: &str, w: f32, h: f32, name: Option<(&str, f32)>) {
    let pic = cx.pic(&[card.to_owned()]);
    let size = pic.as_ref().map(|p| cx.pic_size(p));
    c.spawn((Node { width: cx.px(w), height: cx.px(h), flex_shrink: 0.0, overflow: Overflow::clip(), ..default() }, BackgroundColor(cx.pal.picture_bg)))
        .with_children(|b| {
            match (pic, size) {
                (Some(p), Some(size)) => {
                    let s = cover(size, Vec2::new(w, h) * cx.s);
                    let book = flipbook(cx, card);
                    let mut e = b.spawn((
                        ImageNode::new(p),
                        Node {
                            position_type: PositionType::Absolute,
                            width: Val::Px(s.x),
                            height: Val::Px(s.y),
                            left: Val::Px((w * cx.s - s.x) * 0.5),
                            top: Val::Px((h * cx.s - s.y) * 0.5),
                            ..default()
                        },
                    ));
                    if let Some(book) = book {
                        e.insert(book);
                    }
                }
                // (Not in yet, or the game is not installed: a plain plate.)
                _ => {
                    b.spawn((
                        Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                        BackgroundGradient::from(LinearGradient::to_right(vec![
                            ColorStop::new(cx.pal.accent_a(0.10), Val::Percent(0.0)),
                            ColorStop::new(cx.pal.accent_a(0.32), Val::Percent(100.0)),
                        ])),
                    ));
                }
            }
            if let Some((text, px)) = name {
                b.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        top: Val::Px(0.0),
                        bottom: Val::Px(0.0),
                        width: Val::Percent(100.0),
                        padding: UiRect::horizontal(cx.px(px * 0.4)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundGradient::from(LinearGradient::to_right(vec![
                        ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.62), Val::Percent(0.0)),
                        ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.0), Val::Percent(85.0)),
                    ])),
                ))
                .with_child((Text::new(text), cx.bold(px), TextColor(WHITE)));
            }
        });
}

/// A flipbook's frames on screen (Modern Warfare III's animated emblems
/// and calling cards), turned by [`turn_flipbooks`].
#[derive(Component, Clone, Debug)]
pub(crate) struct Flipbook {
    pub frames: Vec<Handle<Image>>,
    /// The still picture (shown with ANIMATED CARDS off).
    pub poster: Handle<Image>,
}

/// Frames a second a flipbook turns at.
const FLIPBOOK_FPS: f32 = 12.0;

/// The flipbook of a calling card's or emblem's picture, once its frames
/// are in (its still picture shows till then): `None` for a still one, and
/// while they are being read (asking for them; the view is drawn again as
/// they come in).
pub(crate) fn flipbook(cx: &mut Ctx, name: &str) -> Option<Flipbook> {
    let n = cards::frames(name);
    if n < 2 || cx.still {
        return None;
    }
    let poster = cx.pic(&[name.to_owned()])?;
    let mut frames = Vec::with_capacity(n as usize);
    for i in 0..n {
        frames.push(cx.pic(&[cards::frame_name(name, i)])?);
    }
    Some(Flipbook { frames, poster })
}

/// Turns every flipbook on screen (each stands on its still picture with
/// ANIMATED CARDS off).
pub(crate) fn turn_flipbooks(time: Res<Time>, mut books: Query<(&mut ImageNode, &Flipbook)>) {
    let at = (time.elapsed_secs() * FLIPBOOK_FPS) as usize;
    let on = crate::hud_cards::animated();
    for (mut img, book) in &mut books {
        if !on {
            if img.image != book.poster {
                img.image = book.poster.clone();
            }
            continue;
        }
        if let Some(f) = book.frames.get(at % book.frames.len().max(1))
            && img.image != *f
        {
            img.image = f.clone();
        }
    }
}

/// An emblem in a square.
pub(crate) fn emblem(c: &mut Kids, cx: &mut Ctx, name: &str, size: f32) {
    let pic = cx.pic(&[name.to_owned()]);
    c.spawn((
        Node { width: cx.px(size), height: cx.px(size), flex_shrink: 0.0, justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
        BackgroundColor(cx.pal.panel_soft),
    ))
    .with_children(|b| match pic {
        Some(p) => {
            let sz = cx.pic_size(&p);
            let k = (size / sz.x.max(1.0)).min(size / sz.y.max(1.0));
            let book = flipbook(cx, name);
            let mut e = b.spawn((ImageNode::new(p), Node { width: cx.px(sz.x * k), height: cx.px(sz.y * k), ..default() }));
            if let Some(book) = book {
                e.insert(book);
            }
        }
        None => {
            b.spawn((
                Node { width: cx.px(size * 0.42), height: cx.px(size * 0.42), border: UiRect::all(cx.px(2.0)), ..default() },
                BorderColor::all(cx.pal.accent_a(0.45)),
                UiTransform { rotation: Rot2::degrees(45.0), ..UiTransform::IDENTITY },
            ));
        }
    });
}

/// His card at the top right, as Infinite Warfare's: a line of his numbers
/// (rank and level, prestige, kills, wins) over a thin rule, then his
/// calling card as a banner with his name and level on it, a band in the
/// accent with his progress, and his emblem standing over its right end.
fn player_card(c: &mut Kids, cx: &mut Ctx, card: &PlayerCard) {
    let icon = cx.pic(&[card.rank_picture.clone()]);
    let (title, mark) = (cards::mine(Kind::Card, &card.card), cards::mine(Kind::Emblem, &card.emblem));
    let w = 640.0;
    c.spawn(Node { width: cx.px(w + 120.0), flex_direction: FlexDirection::Column, ..abs(None, Some(26.0), Some(24.0), None, cx) }).with_children(|p| {
        // The numbers.
        p.spawn((
            Node { width: cx.px(w), height: cx.px(34.0), column_gap: cx.px(28.0), align_items: AlignItems::Center, padding: UiRect::horizontal(cx.px(10.0)), border: UiRect::bottom(cx.px(2.0)), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            BorderColor::all(WHITE.with_alpha(0.85)),
        ))
        .with_children(|n| {
            n.spawn(Node { column_gap: cx.px(6.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                if let Some(h) = icon.clone() {
                    r.spawn((ImageNode::new(h), Node { width: cx.px(24.0), height: cx.px(24.0), ..default() }));
                }
                r.spawn((Text::new(card.level.to_string()), cx.bold(22.0), TextColor(WHITE)));
            });
            if card.prestige > 0 {
                n.spawn((Text::new(format!("PRESTIGE {}", card.prestige)), cx.bold(20.0), TextColor(TEXT)));
            }
            for (k, v) in card.record.iter().take(3) {
                n.spawn(Node { column_gap: cx.px(6.0), align_items: AlignItems::Baseline, ..default() }).with_children(|r| {
                    r.spawn((Text::new(v.clone()), cx.bold(21.0), TextColor(WHITE)));
                    r.spawn((Text::new(k.clone()), cx.regular(16.0), TextColor(DIM)));
                });
            }
        });
        // The banner, the band, the emblem at its end. (bo2mp: the calling
        // card behind the whole box, the band over its foot, so most of the
        // card shows - a strip 62 high showed a third of it; the emblem
        // stands level with the box, its top and foot lined up.)
        p.spawn(Node { width: cx.px(w + 120.0), height: cx.px(112.0), column_gap: cx.px(8.0), ..default() }).with_children(|b| {
            b.spawn((Node { width: cx.px(w), height: cx.px(112.0), flex_direction: FlexDirection::Column, border: UiRect::all(cx.px(1.0)), overflow: Overflow::clip(), ..default() }, BorderColor::all(cx.pal.accent_a(0.65))))
                .with_children(|card_box| {
                    card_box.spawn(Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), ..default() }).with_children(|back| {
                        banner(back, cx, &title, w - 2.0, 110.0, None);
                    });
                    card_box.spawn(Node { width: Val::Percent(100.0), height: cx.px(62.0), ..default() }).with_children(|top| {
                        top.spawn(Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            right: cx.px(120.0),
                            top: Val::Px(0.0),
                            bottom: Val::Px(0.0),
                            justify_content: JustifyContent::FlexEnd,
                            align_items: AlignItems::Center,
                            column_gap: cx.px(28.0),
                            ..default()
                        })
                        .with_children(|t| {
                            t.spawn((Text::new(card.name.to_uppercase()), cx.bold(34.0), TextColor(WHITE), TextShadow { offset: Vec2::splat(2.0 * cx.s), color: Color::srgba(0.0, 0.0, 0.0, 0.8), ..default() }));
                            t.spawn((Text::new(card.level.to_string()), cx.bold(34.0), TextColor(WHITE), TextShadow { offset: Vec2::splat(2.0 * cx.s), color: Color::srgba(0.0, 0.0, 0.0, 0.8), ..default() }));
                        });
                    });
                    card_box
                        .spawn((
                            Node { width: Val::Percent(100.0), flex_grow: 1.0, flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center, row_gap: cx.px(5.0), padding: UiRect::horizontal(cx.px(10.0)), ..default() },
                            BackgroundGradient::from(LinearGradient::to_right(vec![
                                ColorStop::new(cx.pal.accent_a(0.88), Val::Percent(0.0)),
                                ColorStop::new(cx.pal.accent_a(0.55), Val::Percent(70.0)),
                                ColorStop::new(cx.pal.accent_a(0.35), Val::Percent(100.0)),
                            ])),
                        ))
                        .with_children(|band| {
                            let prestige = if card.prestige > 0 { format!("PRESTIGE {}  ·  ", card.prestige) } else { String::new() };
                            band.spawn((Text::new(format!("{prestige}LEVEL {}  ·  {} XP", card.level, card.xp)), cx.bold(18.0), TextColor(WHITE)));
                            band.spawn((Node { width: cx.px(w * 0.62), height: cx.px(4.0), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45))))
                                .with_child((Node { width: Val::Percent(card.xp_frac * 100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(WHITE)));
                        });
                });
            b.spawn(Node { width: cx.px(112.0), height: cx.px(112.0), flex_shrink: 0.0, ..default() }).with_children(|e| {
                emblem_bare(e, cx, &mark, 112.0);
            });
        });
    });
}

/// An emblem with no plate behind it (the player card's, standing over the
/// banner's end).
pub(crate) fn emblem_bare(c: &mut Kids, cx: &mut Ctx, name: &str, size: f32) {
    let pic = cx.pic(&[name.to_owned()]);
    c.spawn(Node { width: cx.px(size), height: cx.px(size), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }).with_children(|b| {
        if let Some(p) = pic {
            let sz = cx.pic_size(&p);
            let k = (size / sz.x.max(1.0)).min(size / sz.y.max(1.0));
            let book = flipbook(cx, name);
            let mut e = b.spawn((ImageNode::new(p), Node { width: cx.px(sz.x * k), height: cx.px(sz.y * k), ..default() }));
            if let Some(book) = book {
                e.insert(book);
            }
        }
    });
}

/// A key hint: the key (or the pad's button picture) and what it does.
pub(crate) fn hint(c: &mut Kids, cx: &mut Ctx, key: &str, pad: &str, what: &str) {
    let icon = if cx.pad && !pad.is_empty() { cx.pic(&[pad.to_owned()]) } else { None };
    c.spawn(Node { column_gap: cx.px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|h| {
        match icon {
            Some(img) => {
                h.spawn((ImageNode::new(img), Node { width: cx.px(30.0), height: cx.px(30.0), ..default() }));
            }
            // (Ghosts' keys: the key's name in the accent, then what it does.)
            None if !key.is_empty() => {
                h.spawn((Text::new(key), cx.bold(26.0), TextColor(lift(cx.pal.accent, 0.18))));
            }
            None => {}
        }
        if !what.is_empty() {
            h.spawn((Text::new(what), cx.regular(26.0), TextColor(TEXT.with_alpha(0.90))));
        }
    });
}

/// bo2mp: while the game prepares every game's guns in the background
/// (`assets::prefetch_match_common`), the foot says so and for how long.
/// The menus stay usable; it goes when the work is done.
fn preload_chip(r: &mut Kids, cx: &mut Ctx) {
    let Some(secs) = assets::prefetch_status() else { return };
    r.spawn(Node { column_gap: cx.px(10.0), align_items: AlignItems::Center, ..default() }).with_children(|h| {
        h.spawn((Node { width: cx.px(10.0), height: cx.px(10.0), ..default() }, BackgroundColor(GOLD)));
        h.spawn((Text::new(format!("PRELOADING  ·  {secs} S")), cx.regular(24.0), TextColor(FAINT)));
    });
}

fn footer(c: &mut Kids, cx: &mut Ctx, shell: &Shell, cat: &Catalog, online: &OnlineInfo) {
    // Ghosts' foot: the notice bar (a note is drawn into it), then a dark
    // band with the keys at the left, the server and the version at the
    // right.
    notice_bar(c, cx);
    c.spawn((
        Node {
            // (Out to the window's edges; the keys and the server's state
            // stay inside the safe frame.)
            padding: UiRect::new(cx.px(150.0 + cx.bleed.x), cx.px(150.0 + cx.bleed.x), Val::Px(0.0), cx.px(cx.bleed.y)),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            border: UiRect::top(cx.px(1.0)),
            ..super::ghosts::foot_shade(cx, 72.0)
        },
        BorderColor::all(cx.pal.line),
        BackgroundGradient::from(LinearGradient::to_bottom(vec![
            ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.72), Val::Percent(0.0)),
            ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.94), Val::Percent(100.0)),
        ])),
    ))
    .with_children(|f| {
        f.spawn((Node { column_gap: cx.px(28.0), align_items: AlignItems::Center, ..default() }, Hit::Back, Button))
            .with_children(|l| {
                if shell.screen != Screen::Main {
                    let back = match shell.screen {
                        // (bo2mp: BACK, as the owner's reference.)
                        _ if custom_lobby(shell) => "BACK",
                        Screen::Lobby => "LEAVE LOBBY",
                        Screen::WzLobby => super::wz_theme::LEAVE_LOBBY,
                        _ => "BACK",
                    };
                    hint(l, cx, "ESC", "xenonbutton_b", back);
                }
                hint(l, cx, "ENTER", "xenonbutton_a", "SELECT");
                match shell.screen {
                    Screen::Options if shell.page == super::options::FOLDERS_TAB => {
                        hint(l, cx, "Q / E", "xenonbutton_lb", "TABS");
                        if !cx.pad {
                            hint(l, cx, "F5", "", "SCAN FOR GAMES");
                        }
                    }
                    Screen::Lobby | Screen::WzLobby | Screen::Options | Screen::MatchFilter => hint(l, cx, "Q / E", "xenonbutton_lb", "TABS"),
                    Screen::MapPick => hint(l, cx, "Q / E", "xenonbutton_lb", "SECTIONS"),
                    Screen::ItemPick if super::cac::weapon_picker(shell) => {
                        hint(l, cx, "Q / E", "xenonbutton_lb", "GAMES");
                        hint(l, cx, "W / S", "xenonbutton_dpad_ud", "CATEGORIES");
                    }
                    Screen::ItemPick => hint(l, cx, "Q / E", "xenonbutton_lb", "CATEGORIES"),
                    Screen::CardPick | Screen::EmblemPick => {
                        hint(l, cx, "Q / E", "xenonbutton_lb", "PAGES");
                        if !cx.pad {
                            hint(l, cx, "R", "", if shell.typing_card.is_some() { "ENTER: KEEP  ·  ESC: CANCEL" } else { "RENAME" });
                        }
                    }
                    Screen::ZmMapPick if super::zombies::pages(super::zombies::state().bo1()).len() > 1 => hint(l, cx, "Q / E", "xenonbutton_lb", "SECTIONS"),
                    _ => {}
                }
                let stepped = super::rows(shell, &cat).get(shell.focus).is_some_and(|r| matches!(r.act, Act::Cycle(_)));
                if stepped || shell.screen == Screen::ClassEdit {
                    hint(l, cx, "A / D", "xenonbutton_dpad_rl", "CHANGE");
                }
            });
        // bo2mp: the Custom Game lobbies' foot, as the owner's reference: at
        // the right the team hint (team modes) and FRIENDS with how many of
        // his friends are in it.
        if custom_lobby(shell) {
            f.spawn(Node { column_gap: cx.px(40.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                preload_chip(r, cx);
                if shell.screen == Screen::Lobby && super::lobby_rules::has_teams(&shell.mode) {
                    // (Nothing changes teams yet: drawn faint, as a key
                    // that does nothing.)
                    r.spawn(Node { column_gap: cx.px(10.0), align_items: AlignItems::Center, ..default() }).with_children(|h| {
                        h.spawn((Text::new("‹"), cx.bold(30.0), TextColor(cx.pal.accent.with_alpha(0.45))));
                        h.spawn((Text::new("CHANGE TEAM"), cx.regular(24.0), TextColor(FAINT)));
                        h.spawn((Text::new("›"), cx.bold(30.0), TextColor(cx.pal.accent.with_alpha(0.45))));
                    });
                }
                let friends = if shell.screen == Screen::Lobby { crate::online_match::p2p_view().map_or(0, |v| v.friends.len()) } else { 0 };
                let p = cx.pal.panel.to_srgba();
                r.spawn((
                    Node { height: cx.px(42.0), min_width: cx.px(260.0), padding: UiRect::horizontal(cx.px(20.0)), column_gap: cx.px(14.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::left(cx.px(4.0)), ..default() },
                    BorderColor::all(cx.pal.accent),
                    fade_right(Color::srgba(p.red, p.green, p.blue, 0.95), Color::srgba(p.red, p.green, p.blue, 0.55)),
                ))
                .with_children(|b| {
                    b.spawn((Text::new("FRIENDS"), cx.bold(24.0), TextColor(WHITE)));
                    b.spawn((Text::new("›"), cx.bold(26.0), TextColor(DIM)));
                    b.spawn((Text::new(friends.to_string()), cx.bold(24.0), TextColor(GOLD)));
                });
            });
            return;
        }
        f.spawn(Node { column_gap: cx.px(28.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
            preload_chip(r, cx);
            if !cx.pad {
                hint(r, cx, "F10", "", "GAME FOLDERS");
            }
            let (dot, label) = match online.status {
                Status::Online => (GREEN, format!("ONLINE  ·  {} PLAYERS", fmt_count(online.players))),
                Status::Connecting => (GOLD, "CONNECTING".to_owned()),
                Status::Unreachable => (ENEMY, "SERVER UNREACHABLE".to_owned()),
                Status::Offline => (FAINT, "OFFLINE  ·  LOCAL PLAY".to_owned()),
            };
            r.spawn(Node { column_gap: cx.px(10.0), align_items: AlignItems::Center, ..default() }).with_children(|s| {
                s.spawn((
                    Node { width: cx.px(9.0), height: cx.px(9.0), border_radius: BorderRadius::all(Val::Percent(50.0)), ..default() },
                    BackgroundColor(dot),
                ));
                s.spawn((Text::new(label), cx.bold(18.0), TextColor(DIM)));
            });
            r.spawn((Text::new(format!("v{}", env!("CARGO_PKG_VERSION"))), cx.regular(16.0), TextColor(FAINT)));
        });
    });
}

/// bo2mp: a Custom Game lobby (Multiplayer's, or Zombies' CUSTOM GAME),
/// whose foot is the owner's reference's.
fn custom_lobby(shell: &Shell) -> bool {
    (shell.screen == Screen::Lobby && !shell.public) || (shell.screen == Screen::ZmLobby && shell.zm_kind == super::zombies::ZmKind::Custom)
}

/// The notice bar over the foot band (empty, or the note in it).
pub(super) fn notice_bar(c: &mut Kids, cx: &Ctx) {
    c.spawn((
        Node { height: cx.px(34.0), border: UiRect::vertical(cx.px(1.0)), ..abs(Some(14.0), None, Some(14.0), Some(80.0), cx) },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.42)),
        BorderColor::all(cx.pal.line),
    ));
}

/// A note in the notice bar: a ringed `!` and the words.
pub(super) fn notice(c: &mut Kids, cx: &Ctx, text: &str) {
    c.spawn((
        Node { height: cx.px(34.0), column_gap: cx.px(12.0), align_items: AlignItems::Center, padding: UiRect::left(cx.px(96.0)), border: UiRect::vertical(cx.px(1.0)), ..abs(Some(14.0), None, Some(14.0), Some(80.0), cx) },
        BackgroundColor(cx.pal.panel),
        BorderColor::all(cx.pal.line),
        super::anim::from_left(cx.s, 80.0, 0.0),
    ))
    .with_children(|n| {
        n.spawn((
            Node { width: cx.px(24.0), height: cx.px(24.0), border: UiRect::all(cx.px(2.0)), border_radius: BorderRadius::all(Val::Percent(50.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            BorderColor::all(GOLD),
        ))
        .with_child((Text::new("!"), cx.bold(16.0), TextColor(GOLD)));
        n.spawn((Text::new(text.to_owned()), cx.bold(22.0), TextColor(WHITE)));
    });
}

fn fmt_count(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A column of menu rows, as Infinite Warfare's: each a white bar at its
/// left and its name; the one in focus a band in the accent ending in a
/// point. A selector's value sits at the row's right between arrows. The
/// line about the row in focus goes under the column. `big`: taller rows.
pub(crate) fn menu_rows(c: &mut Kids, cx: &Ctx, rows: &[Row], focus: usize, width: f32, big: bool) {
    menu_rows_window(c, cx, rows, focus, width, big, 0, rows.len());
}

/// `menu_rows` for the rows `first..last` only (their numbers stay the
/// list's, so hits and focus keep meaning), with a chevron above or below
/// while more rows lie beyond.
#[allow(clippy::too_many_arguments)]
pub(crate) fn menu_rows_window(c: &mut Kids, cx: &Ctx, rows: &[Row], focus: usize, width: f32, big: bool, first: usize, last: usize) {
    let (h, px) = if big { (62.0, 34.0) } else { (52.0, 28.0) };
    let acc = cx.pal.accent;
    let more = first > 0 || last < rows.len();
    let chevron = |c: &mut Kids, glyph: &str, shown: bool| {
        c.spawn(Node { width: cx.px(width), height: cx.px(22.0), flex_shrink: 0.0, justify_content: JustifyContent::Center, ..default() })
            .with_child((Text::new(if shown { glyph } else { "" }), cx.bold(18.0), TextColor(DIM)));
    };
    if more {
        chevron(c, "\u{25B2}", first > 0);
    }
    for (i, r) in rows.iter().enumerate().take(last).skip(first) {
        let on = i == focus;
        c.spawn((
            Hit::Row(i),
            Button,
            Node {
                width: cx.px(width),
                min_height: cx.px(h),
                flex_shrink: 0.0,
                padding: UiRect::axes(cx.px(24.0), cx.px(2.0)),
                align_items: AlignItems::Center,
                justify_content: if r.value.is_some() { JustifyContent::SpaceBetween } else { JustifyContent::FlexEnd },
                border: UiRect::top(cx.px(1.0)),
                ..default()
            },
            BorderColor::all(if on { lift(acc, 0.35) } else { LINE }),
            if on { fade_right(acc.with_alpha(0.30), acc.with_alpha(0.80)) } else { fade_right(Color::srgba(0.02, 0.05, 0.10, 0.30), Color::srgba(0.02, 0.05, 0.10, 0.62)) },
        ))
        .with_children(|line| {
            line.spawn((Text::new(r.label.clone()), if on { cx.bold(px) } else { cx.regular(px) }, TextColor(if on { WHITE } else { TEXT.with_alpha(0.80) })));
            match &r.value {
                Some(v) if !matches!(r.act, Act::Cycle(_)) => {
                    line.spawn((Text::new(v.clone()), cx.bold(px * 0.9), TextColor(if on { WHITE } else { DIM })));
                }
                Some(v) => {
                    line.spawn(Node { column_gap: cx.px(12.0), align_items: AlignItems::Center, ..default() }).with_children(|sel| {
                        sel.spawn((Hit::Step(i, -1), Button, Text::new("‹"), cx.bold(px), TextColor(if on { WHITE } else { FAINT })));
                        sel.spawn(Node { min_width: cx.px(160.0), justify_content: JustifyContent::Center, ..default() })
                            .with_child((Text::new(v.clone()), cx.bold(px * 0.9), TextColor(if on { WHITE } else { DIM })));
                        sel.spawn((Hit::Step(i, 1), Button, Text::new("›"), cx.bold(px), TextColor(if on { WHITE } else { FAINT })));
                    });
                }
                None => {}
            }
        });
    }
    if more {
        chevron(c, "\u{25BC}", last < rows.len());
    }
    if let Some(r) = rows.get(focus).filter(|r| !r.sub.is_empty()) {
        c.spawn(Node { max_width: cx.px(width), margin: UiRect::top(cx.px(18.0)), column_gap: cx.px(12.0), align_items: AlignItems::Center, ..default() }).with_children(|d| {
            d.spawn((Node { width: cx.px(8.0), height: cx.px(20.0), flex_shrink: 0.0, ..default() }, BackgroundColor(acc)));
            d.spawn((Text::new(r.sub.clone()), cx.regular(25.0), TextColor(TEXT)));
        });
    }
}

/// OUTBREAK's rows, as Infinite Warfare's Zombies menu: each a dark box with
/// a thin rim; the one in focus a red band running out past its box.
#[allow(dead_code)]
fn outbreak_rows(c: &mut Kids, cx: &Ctx, rows: &[Row], focus: usize, width: f32) {
    let (h, px) = (36.0, 22.0);
    for (i, r) in rows.iter().enumerate() {
        let on = i == focus;
        let mut e = c.spawn((
            Hit::Row(i),
            Button,
            Node {
                width: cx.px(width),
                min_height: cx.px(h),
                padding: UiRect::axes(cx.px(16.0), cx.px(2.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                border: UiRect::all(cx.px(1.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(if on { Color::NONE } else { WHITE.with_alpha(0.28) }),
        ));
        if on {
            e.insert(BackgroundGradient::from(LinearGradient::to_right(vec![
                ColorStop::new(cx.pal.accent_a(0.95), Val::Percent(0.0)),
                ColorStop::new(cx.pal.accent_a(0.85), Val::Percent(75.0)),
                ColorStop::new(cx.pal.accent_a(0.0), Val::Percent(100.0)),
            ])));
        } else {
            e.insert(BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.62)));
        }
        e.with_children(|line| {
            line.spawn((Text::new(r.label.clone()), cx.regular(px), TextColor(if on { WHITE } else { TEXT.with_alpha(0.86) })));
            match &r.value {
                Some(v) if !matches!(r.act, Act::Cycle(_)) => {
                    line.spawn((Text::new(v.clone()), cx.bold(px * 0.9), TextColor(if on { WHITE } else { DIM })));
                }
                Some(v) => {
                    line.spawn(Node { column_gap: cx.px(12.0), align_items: AlignItems::Center, ..default() }).with_children(|sel| {
                        sel.spawn((Hit::Step(i, -1), Button, Text::new("‹"), cx.bold(px), TextColor(if on { WHITE } else { FAINT })));
                        sel.spawn(Node { min_width: cx.px(120.0), justify_content: JustifyContent::Center, ..default() })
                            .with_child((Text::new(v.clone()), cx.bold(px * 0.9), TextColor(if on { WHITE } else { DIM })));
                        sel.spawn((Hit::Step(i, 1), Button, Text::new("›"), cx.bold(px), TextColor(if on { WHITE } else { FAINT })));
                    });
                }
                None => {}
            }
        });
    }
    if let Some(r) = rows.get(focus).filter(|r| !r.sub.is_empty()) {
        c.spawn((Text::new(r.sub.clone()), cx.regular(21.0), TextColor(TEXT), Node { max_width: cx.px(width + 40.0), margin: UiRect::top(cx.px(6.0)), ..default() }));
    }
}

/// A colour a little lighter.
pub(crate) fn lift(c: Color, by: f32) -> Color {
    let l = c.to_srgba();
    Color::srgba((l.red + by).min(1.0), (l.green + by).min(1.0), (l.blue + by).min(1.0), l.alpha)
}

/// A picture cut to a box, covering it.
pub(crate) fn picture_box(c: &mut Kids, cx: &mut Ctx, names: &[String], w: f32, h: f32, extra: impl Bundle) {
    let pic = cx.pic(names);
    let size = pic.as_ref().map(|p| cx.pic_size(p));
    c.spawn((
        Node { width: cx.px(w), height: cx.px(h), overflow: Overflow::clip(), ..default() },
        BackgroundColor(cx.pal.picture_bg),
        extra,
    ))
    .with_children(|b| {
        if let (Some(p), Some(size)) = (pic, size) {
            let s = cover(size, Vec2::new(w, h) * cx.s);
            b.spawn((
                ImageNode::new(p),
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(s.x),
                    height: Val::Px(s.y),
                    left: Val::Px((w * cx.s - s.x) * 0.5),
                    top: Val::Px((h * cx.s - s.y) * 0.5),
                    ..default()
                },
            ));
        }
    });
}

/// A picture fitted inside a box, keeping its shape.
pub(crate) fn picture_contain(c: &mut Kids, cx: &mut Ctx, names: &[String], w: f32, h: f32) {
    let pic = cx.pic(names);
    c.spawn((Node { width: cx.px(w), height: cx.px(h), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },))
        .with_children(|b| {
            if let Some(p) = pic {
                let size = cx.pic_size(&p);
                let k = (w / size.x.max(1.0)).min(h / size.y.max(1.0));
                // (A weapon's menu picture is drawn mirrored, barrel to the
                // left, as Black Ops II's Create-a-Class shows it.)
                let weapon = names.first().is_some_and(|n| n.starts_with("menu_mp_weapons") || n.contains("_weapons_"));
                let mut img = ImageNode::new(p);
                img.flip_x = weapon;
                b.spawn((img, Node { width: cx.px(size.x * k), height: cx.px(size.y * k), ..default() }));
            }
        });
}

pub(crate) fn tabs(c: &mut Kids, cx: &mut Ctx, labels: &[String], on: usize, hit: fn(usize) -> Hit) {
    // (The row ends at 150, just over the header's accent rule, so the
    // rule runs under the tabs and never through their names.)
    tabs_at(c, cx, labels, on, hit, 96.0, 150.0 - TAB_H);
}

/// The height of a [`tabs`] row.
const TAB_H: f32 = 34.0;

/// [`tabs`] with their top left at (`left`, `top`): a band with a thin
/// rule under it, as Create-a-Class' game tabs: Q, `‹` when tabs are cut
/// off to the left, the tabs evenly spaced (the one in use on an accent
/// ground with the accent line under it), `›` when more lie to the right,
/// then E.
pub(crate) fn tabs_at(c: &mut Kids, cx: &mut Ctx, labels: &[String], on: usize, hit: fn(usize) -> Hit, left: f32, top: f32) {
    // More tabs than fit across the screen: the run around the one in
    // use, with `‹` / `›` for those cut off at either end.
    let width = |l: &str| l.chars().count() as f32 * 12.5 + 2.0 * 18.0 + 2.0;
    let room = cx.w - left - 132.0 - 2.0 * 60.0 - 2.0 * 30.0;
    let (first, last) = tab_window(labels, on, room, width);
    let pal = cx.pal;
    c.spawn((
        Node {
            height: cx.px(TAB_H),
            padding: UiRect::horizontal(cx.px(12.0)),
            column_gap: cx.px(2.0),
            align_items: AlignItems::Center,
            border: UiRect::bottom(cx.px(1.0)),
            ..abs(Some(left), Some(top), None, None, cx)
        },
        BorderColor::all(pal.line),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.22)),
    ))
    .with_children(|t| {
        let more = |t: &mut Kids, cx: &Ctx, glyph: &str, to: usize| {
            t.spawn((hit(to), Button, Node { height: Val::Percent(100.0), padding: UiRect::horizontal(cx.px(8.0)), align_items: AlignItems::Center, ..default() }))
                .with_child((Text::new(glyph.to_owned()), cx.bold(26.0), TextColor(DIM)));
        };
        t.spawn(Node { margin: UiRect::right(cx.px(10.0)), ..default() }).with_children(|h| hint(h, cx, "Q", "xenonbutton_lb", ""));
        if first > 0 {
            more(t, cx, "‹", first - 1);
        }
        for (i, l) in labels.iter().enumerate().take(last + 1).skip(first) {
            let sel = i == on;
            t.spawn((
                hit(i),
                Button,
                Node { height: Val::Percent(100.0), padding: UiRect::horizontal(cx.px(18.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                BackgroundColor(if sel { pal.accent_a(0.16) } else { Color::NONE }),
            ))
            .with_children(|b| {
                b.spawn((Text::new(l.clone()), cx.bold(21.0), TextColor(if sel { WHITE } else { DIM })));
                if sel {
                    b.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), bottom: Val::Px(0.0), height: cx.px(3.0), ..default() }, BackgroundColor(pal.accent)));
                }
            });
        }
        if last + 1 < labels.len() {
            more(t, cx, "›", last + 1);
        }
        t.spawn(Node { margin: UiRect::left(cx.px(10.0)), ..default() }).with_children(|h| hint(h, cx, "E", "xenonbutton_rb", ""));
    });
}

/// The tabs `first..=last` to draw: all of them when they fit in `room`,
/// else as many around `on` as do.
fn tab_window(labels: &[String], on: usize, room: f32, width: impl Fn(&str) -> f32) -> (usize, usize) {
    if labels.is_empty() {
        return (0, 0);
    }
    let on = on.min(labels.len() - 1);
    let (mut first, mut last) = (on, on);
    let mut used = width(&labels[on]);
    loop {
        let before = first.checked_sub(1).map(|i| width(&labels[i])).filter(|w| used + w <= room);
        let after = (last + 1 < labels.len()).then(|| width(&labels[last + 1])).filter(|w| used + w <= room);
        // (Grow the side with fewer tabs shown, so `on` stays near the
        // middle; the other side when that one is full.)
        let before_first = on - first <= last - on;
        match (before, after) {
            (Some(b), _) if before_first || after.is_none() => {
                first -= 1;
                used += b;
            }
            (_, Some(a)) => {
                last += 1;
                used += a;
            }
            _ => break,
        }
    }
    (first, last)
}

/// A small white point to the right (his own line in a list).
pub(crate) fn pointer(c: &mut Kids, cx: &Ctx) {
    c.spawn(Node { width: cx.px(10.0), height: cx.px(16.0), overflow: Overflow::clip(), ..default() }).with_child((
        Node { position_type: PositionType::Absolute, left: cx.px(-6.0), top: cx.px(2.3), width: cx.px(11.3), height: cx.px(11.3), ..default() },
        BackgroundColor(WHITE),
        UiTransform { rotation: Rot2::degrees(45.0), ..UiTransform::IDENTITY },
    ));
}

/// A player in a lobby's list.
pub(crate) struct LobbyPlayer {
    pub level: Option<i32>,
    pub prestige: i32,
    pub name: String,
    pub me: bool,
    pub host: bool,
    /// His team's colour (a bar at the row's left), if teams are known.
    pub team: Option<Color>,
    /// His calling card (drawn as the banner at the row's right).
    pub card: String,
}

/// The lobby's players, as Infinite Warfare's: a line each, his rank and
/// level, his name, and his calling card as a banner at the right; empty
/// places a faint line. `title`: the line over the list (players / max).
#[allow(dead_code)] // (bo2mp: the Ghosts-style lobbies use `ghosts::slot_list` / `vote_card`.)
pub(crate) fn player_list(c: &mut Kids, cx: &mut Ctx, players: &[LobbyPlayer], max: usize, title: &str, top: f32) {
    c.spawn(Node { width: cx.px(640.0), flex_direction: FlexDirection::Column, ..abs(None, Some(top), Some(90.0), None, cx) }).with_children(|list| {
        list.spawn(Node { justify_content: JustifyContent::SpaceBetween, margin: UiRect::bottom(cx.px(6.0)), ..default() }).with_children(|t| {
            t.spawn((Text::new(title.to_owned()), cx.regular(24.0), TextColor(WHITE)));
            t.spawn((Text::new(format!("{}/{max}", players.len().min(max))), cx.regular(24.0), TextColor(WHITE)));
        });
        for i in 0..max.max(players.len()) {
            let p = players.get(i);
            list.spawn((
                Node { height: cx.px(40.0), column_gap: cx.px(12.0), align_items: AlignItems::Center, border: UiRect::bottom(cx.px(1.0)), flex_shrink: 0.0, ..default() },
                BorderColor::all(WHITE.with_alpha(if p.is_some() { 0.30 } else { 0.10 })),
                BackgroundGradient::from(LinearGradient::to_right(vec![
                    ColorStop::new(Color::srgba(0.0, 0.0, 0.0, if p.is_some_and(|p| p.me) { 0.62 } else { 0.48 }), Val::Percent(0.0)),
                    ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.12), Val::Percent(100.0)),
                ])),
            ))
            .with_children(|r| {
                let Some(p) = p else {
                    r.spawn((Text::new("—"), cx.regular(20.0), TextColor(FAINT), Node { margin: UiRect::left(cx.px(30.0)), ..default() }));
                    return;
                };
                r.spawn((Node { width: cx.px(4.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(p.team.unwrap_or(Color::NONE))));
                r.spawn(Node { width: cx.px(14.0), ..default() }).with_children(|m| {
                    if p.me {
                        pointer(m, cx);
                    }
                });
                let icon = p.level.and_then(|l| cx.pic(&[bo2_profile::progression::rank_picture(l, p.prestige)]));
                match icon {
                    Some(h) => {
                        r.spawn((ImageNode::new(h), Node { width: cx.px(28.0), height: cx.px(28.0), ..default() }));
                    }
                    None => {
                        r.spawn(Node { width: cx.px(28.0), ..default() });
                    }
                }
                r.spawn((Node { width: cx.px(46.0), ..default() },)).with_child((Text::new(p.level.map_or_else(String::new, |l| l.to_string())), cx.regular(23.0), TextColor(WHITE)));
                r.spawn((Node { flex_grow: 1.0, column_gap: cx.px(10.0), align_items: AlignItems::Center, ..default() },)).with_children(|n| {
                    n.spawn((Text::new(p.name.clone()), cx.regular(23.0), TextColor(if p.me { GOLD } else { WHITE })));
                    if p.host {
                        n.spawn((Text::new("HOST"), cx.bold(16.0), TextColor(cx.pal.accent)));
                    }
                });
                banner(r, cx, &p.card, 176.0, 36.0, None);
            });
        }
    });
}

/// A map in the lobby's panel at the bottom left, as Infinite Warfare's vote
/// rows: its picture behind, a diamond mark, its name and mode, and a figure
/// at the right (votes, or the countdown).
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)] // (bo2mp: the Ghosts-style lobbies use `ghosts::slot_list` / `vote_card`.)
pub(crate) fn map_tile(c: &mut Kids, cx: &mut Ctx, pictures: &[String], name: &str, mode: &str, right: (&str, &str), on: bool, hit: Option<Hit>) {
    let (w, h) = (560.0, 86.0);
    let mut e = c.spawn((
        Node { width: cx.px(w), height: cx.px(h), border: UiRect::all(cx.px(if on { 2.0 } else { 1.0 })), flex_shrink: 0.0, ..default() },
        BorderColor::all(if on { WHITE } else { WHITE.with_alpha(0.25) }),
    ));
    if let Some(hit) = hit {
        e.insert((hit, Button));
    }
    e.with_children(|tile| {
        picture_box(tile, cx, pictures, w - if on { 4.0 } else { 2.0 }, h - if on { 4.0 } else { 2.0 }, ());
        tile.spawn((
            Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(0.0), right: Val::Px(0.0), align_items: AlignItems::Center, column_gap: cx.px(16.0), padding: UiRect::horizontal(cx.px(18.0)), ..default() },
            BackgroundGradient::from(LinearGradient::to_right(vec![
                ColorStop::new(if on { cx.pal.accent_a(0.70) } else { Color::srgba(0.0, 0.0, 0.0, 0.72) }, Val::Percent(0.0)),
                ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.35), Val::Percent(62.0)),
                ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.70), Val::Percent(100.0)),
            ])),
        ))
        .with_children(|bar| {
            bar.spawn((
                Node { width: cx.px(46.0), height: cx.px(46.0), border: UiRect::all(cx.px(3.0)), margin: UiRect::horizontal(cx.px(8.0)), ..default() },
                BorderColor::all(WHITE.with_alpha(0.9)),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
                UiTransform { rotation: Rot2::degrees(45.0), ..UiTransform::IDENTITY },
            ));
            bar.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, ..default() }).with_children(|t| {
                t.spawn((Text::new(name.to_owned()), cx.bold(28.0), TextColor(WHITE)));
                t.spawn((Text::new(mode.to_owned()), cx.bold(18.0), TextColor(TEXT.with_alpha(0.8))));
            });
            bar.spawn(Node { column_gap: cx.px(8.0), align_items: AlignItems::Baseline, ..default() }).with_children(|t| {
                t.spawn((Text::new(right.0.to_owned()), cx.bold(20.0), TextColor(WHITE)));
                t.spawn((Text::new(right.1.to_owned()), cx.bold(26.0), TextColor(GOLD)));
            });
        });
    });
}

/// The lobby's map, as Infinite Warfare's pre-match lobby shows it at the
/// bottom left: the map's name over a framed picture of it, the mode's mark
/// and name across the picture's foot; what the lobby is doing at the right
/// of the name (the countdown, the players it waits for).
#[allow(dead_code)] // (bo2mp: the Ghosts-style lobbies use `ghosts::slot_list` / `vote_card`.)
pub(crate) fn map_card(c: &mut Kids, cx: &mut Ctx, pictures: &[String], map: &str, mode: &str, status: &str, gold: bool) {
    let (w, h) = (470.0, 250.0);
    c.spawn(Node { width: cx.px(w), flex_direction: FlexDirection::Column, row_gap: cx.px(4.0), ..abs(Some(126.0), None, None, Some(96.0), cx) }).with_children(|col| {
        col.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|t| {
            t.spawn(Node { column_gap: cx.px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|n| {
                n.spawn((Node { width: cx.px(6.0), height: cx.px(6.0), ..default() }, BackgroundColor(WHITE)));
                n.spawn((Text::new(title_word(map)), cx.regular(22.0), TextColor(WHITE)));
            });
            t.spawn((Text::new(status.to_owned()), cx.regular(20.0), TextColor(if gold { GOLD } else { TEXT })));
        });
        col.spawn((Node { width: cx.px(w), height: cx.px(h), border: UiRect::all(cx.px(2.0)), ..default() }, BorderColor::all(WHITE.with_alpha(0.9)))).with_children(|f| {
            picture_box(f, cx, pictures, w - 4.0, h - 4.0, ());
            f.spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(0.0), right: Val::Px(0.0), bottom: Val::Px(0.0), height: cx.px(92.0), align_items: AlignItems::Center, column_gap: cx.px(18.0), padding: UiRect::horizontal(cx.px(22.0)), ..default() },
                BackgroundGradient::from(LinearGradient::to_bottom(vec![
                    ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.0), Val::Percent(0.0)),
                    ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.82), Val::Percent(100.0)),
                ])),
            ))
            .with_children(|bar| {
                bar.spawn((
                    Node { width: cx.px(44.0), height: cx.px(44.0), border: UiRect::all(cx.px(3.0)), margin: UiRect::horizontal(cx.px(6.0)), ..default() },
                    BorderColor::all(WHITE),
                    BackgroundColor(cx.pal.accent_a(0.55)),
                    UiTransform { rotation: Rot2::degrees(45.0), ..UiTransform::IDENTITY },
                ));
                bar.spawn((Text::new(mode.to_uppercase()), cx.bold(34.0), TextColor(WHITE), TextShadow { offset: Vec2::splat(2.0 * cx.s), color: Color::srgba(0.0, 0.0, 0.0, 0.8), ..default() }));
            });
        });
    });
}

/// `NUKETOWN 2025` -> `Nuketown 2025` (the card's small map name).
fn title_word(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The private lobby (Custom Game), laid out as the owner's reference in
/// our Ghosts-style look: `MULTIPLAYER / CUSTOM GAME` small over CUSTOM
/// GAME, the tabs (LOBBY / LOADOUTS / SETTINGS: our GAME SETUP, CREATE A
/// CLASS and MATCH SETTINGS) and their rows down the left with the line
/// about the one in focus under them, the map and mode as a card at the
/// bottom left, the players at the right (blue his team, red the enemy's;
/// one section without teams) and the lobby's foot (`footer`).
fn lobby(c: &mut Kids, cx: &mut Ctx, shell: &Shell, cat: &Catalog, rows: &[Row]) {
    let map = cat.map(&shell.map);
    let map_title = map.map_or_else(|| shell.map.to_uppercase(), |m| m.title.clone());
    let mode_title = cat.mode(&shell.mode).map_or_else(|| shell.mode.to_uppercase(), |m| m.title.clone());
    let pictures = map.map(|m| m.pictures.clone()).unwrap_or_default();
    let mark = cat.mode(&shell.mode).and_then(|m| cx.pic(&[m.picture.clone()]));
    super::ghosts::title_block(c, cx, "MULTIPLAYER  /  CUSTOM GAME", "CUSTOM GAME");
    let labels: Vec<String> = Tab::ALL.iter().map(|t| t.label().to_owned()).collect();
    let on = Tab::ALL.iter().position(|t| *t == shell.tab).unwrap_or(0);
    tabs_at(c, cx, &labels, on, Hit::Tab, 96.0, 186.0);
    let starting = bridge::starting_in();
    let when = starting.map_or_else(|| format!("{} BOTS  ·  READY", shell.friends + shell.enemies), |ms| format!("STARTING IN 0:{:02}", (ms / 1000.0).ceil() as i32));
    c.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(0.0), Some(242.0), None, None, cx) })
        .with_children(|col| super::ghosts::rows(col, cx, rows, shell.focus, super::ghosts::Look { width: 660.0, h: 46.0, px: 26.0, sub: Some(96.0) }));
    super::ghosts::mode_card(
        c,
        cx,
        super::ghosts::ModeCard { pictures: &pictures, map: &title_word(&map_title), mode: &mode_title, icon: mark, status: &when, pulse: starting.is_some(), w: 600.0, pic_h: 196.0 },
        96.0,
        128.0,
    );
    // bo2mp: the players as the owner's reference lays them out: a header
    // bar per side (the team, or WAITING) over ruled rows: real players,
    // then bots, then open places dimmed (18 in all, `lobby_rules`).
    let teams = super::lobby_rules::has_teams(&shell.mode);
    let me = &cat.player;
    let mut humans = vec![LobbyPlayer {
        level: Some(me.level),
        prestige: me.prestige,
        name: me.name.clone(),
        me: true,
        host: true,
        team: Some(cx.pal.accent),
        card: cards::mine(Kind::Card, &me.card),
    }];
    // bo2mp: his friends in his online lobby (`super::p2p`).
    for name in crate::online_match::p2p_view().into_iter().flat_map(|v| v.friends) {
        humans.push(LobbyPlayer {
            level: None,
            prestige: 0,
            card: cards::for_name(Kind::Card, &name),
            name,
            me: false,
            host: false,
            team: Some(cx.pal.accent),
        });
    }
    // bo2mp: each bot by the name and level it plays the match under.
    let bot = |n: u32, team: Color| {
        let name = sim::lobby_bots::lobby_bot_name(n);
        LobbyPlayer {
            level: Some(sim::lobby_bots::level_for(&name)),
            prestige: 0,
            card: cards::for_name(Kind::Card, &format!("Bot {n}")),
            name,
            me: false,
            host: false,
            team: Some(team),
        }
    };
    let max = super::lobby_rules::MAX_PLAYERS as usize;
    let real = humans.len();
    let waiting = if starting.is_some() { "STARTING" } else { "WAITING" };
    let sections = if teams {
        let acc = cx.pal.accent;
        let mut mine = humans;
        mine.extend((0..shell.friends).map(|i| bot(i + 1, acc)));
        let theirs: Vec<LobbyPlayer> = (0..shell.enemies).map(|i| bot(shell.friends + i + 1, ENEMY)).collect();
        let left = max.saturating_sub(mine.len() + theirs.len());
        let half = max / 2;
        let open_mine = half.saturating_sub(mine.len()).min(left);
        let open_theirs = half.saturating_sub(theirs.len()).min(left - open_mine);
        let of_mine = (mine.len() + open_mine).max(half);
        let of_theirs = (theirs.len() + open_theirs).max(half);
        // bo2mp: the factions the match shows for its teams, whatever the
        // map's game: Black Ops' and the Spetsnaz with their emblems, as the
        // score bar and scoreboard (`hud_match`, `hud_modern`); his side
        // blue, the enemy's red.
        let (ours, theirs_em) = (cx.pic(&["t5:faction_128_specops".to_owned()]), cx.pic(&["t5:faction_128_spetsnaz".to_owned()]));
        vec![
            super::ghosts::Section { title: format!("BLACK OPS  ·  {waiting}"), color: acc, emblem: ours, players: mine, bots_from: real, open: open_mine, of: of_mine },
            super::ghosts::Section { title: "SPETSNAZ".to_owned(), color: ENEMY, emblem: theirs_em, players: theirs, bots_from: 0, open: open_theirs, of: of_theirs },
        ]
    } else {
        let mut all = humans;
        all.extend((0..shell.enemies).map(|i| bot(i + 1, ENEMY)));
        let open = max.saturating_sub(all.len());
        vec![super::ghosts::Section { title: waiting.to_owned(), color: cx.pal.accent, emblem: None, players: all, bots_from: real, open, of: max }]
    };
    super::ghosts::grid(c, cx, 186.0, 860.0, 690.0);
    super::ghosts::team_sections(c, cx, &sections, 186.0);
}

/// A grid that fits the screen: how many columns fit, and which tiles
/// (first..last) show so the one in focus is always in view. Input moves
/// by the columns drawn (`GRID_COLS`).
pub(crate) fn grid_window(cx: &Ctx, n: usize, focus: usize, tw: f32, th: f32, gap: f32, top: f32, bottom: f32) -> (usize, usize, usize) {
    let cols = (((cx.w - 144.0 + gap) / (tw + gap)).floor() as usize).clamp(2, super::COLUMNS);
    let rows = (((cx.h - top - bottom + gap) / (th + gap)).floor() as usize).max(1);
    super::GRID_COLS.store(cols, std::sync::atomic::Ordering::Relaxed);
    let focus_row = focus / cols;
    let total_rows = n.div_ceil(cols).max(1);
    let mut first_row = super::GRID_FIRST_ROW.load(std::sync::atomic::Ordering::Relaxed);
    if focus_row < first_row {
        first_row = focus_row;
    }
    if focus_row >= first_row + rows {
        first_row = focus_row + 1 - rows;
    }
    first_row = first_row.min(total_rows.saturating_sub(rows));
    super::GRID_FIRST_ROW.store(first_row, std::sync::atomic::Ordering::Relaxed);
    (cols, first_row * cols, ((first_row + rows) * cols).min(n))
}

/// "12-24 of 30", and arrows when more lie above or below.
pub(crate) fn grid_more(c: &mut Kids, cx: &Ctx, first: usize, last: usize, n: usize, left: f32, top: f32) {
    if first == 0 && last >= n {
        return;
    }
    // (The font has no arrow glyphs: plain words.)
    let up = if first > 0 { "MORE ABOVE  ·  " } else { "" };
    let down = if last < n { "  ·  MORE BELOW" } else { "" };
    c.spawn((
        Node { position_type: PositionType::Absolute, left: cx.px(left), top: cx.px(top), ..default() },
        Text::new(format!("{up}{}-{} OF {n}{down}", first + 1, last)),
        cx.bold(19.0),
        TextColor(DIM),
    ));
}

fn map_picker(c: &mut Kids, cx: &mut Ctx, shell: &Shell, cat: &Catalog) {
    let p = pages(cat);
    let page = shell.page.min(p.len().saturating_sub(1));
    let section = p.get(page).map_or(String::new(), |(t, _)| t.clone());
    header(c, cx, &["CUSTOM GAME", "SELECT MAP", section.as_str()]);
    // (Short names, so every game's tab fits on one line.)
    let labels: Vec<String> = p.iter().map(|(t, _)| short_section(t)).collect();
    tabs(c, cx, &labels, page, Hit::Page);
    let Some((_, maps)) = p.get(page) else { return };
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
            let m = &cat.maps[i];
            let on = k == shell.focus;
            let chosen = m.ui_name == shell.map;
            // (Ghosts' map cards: the picture, the name on a strip under it;
            // 10 under each card is its margin, so a card is `th` in all.)
            super::ghosts::vote_card(grid, cx, super::ghosts::Card {
                pictures: &m.pictures,
                name: &title_word(&m.title),
                // (A Black Ops III Steam Workshop map wears its badge.)
                top: (if crate::foreign_maps::is_workshop_origin(&m.origin) { "WORKSHOP" } else { "" }, if chosen { "SELECTED" } else { "" }),
                on,
                hit: Some(Hit::Row(k)),
                w: tw,
                pic_h: th - 44.0 - 10.0,
                delay: ((k - first) as f32 * 0.015).min(0.18),
            });
        }
    });
    if let Some(&i) = maps.get(shell.focus) {
        let m = &cat.maps[i];
        c.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                ..abs(None, None, Some(72.0), Some(124.0), cx)
            },
            super::anim::from_left(cx.s, -60.0, 0.05),
        ))
        .with_children(|t| {
            t.spawn((Text::new(m.title.clone()), cx.bold(64.0), TextColor(WHITE)));
            t.spawn((Text::new(m.origin.to_uppercase()), cx.bold(22.0), TextColor(cx.pal.accent)));
        });
    }
}

/// A map section's tab name (`MODERN WARFARE 3 2` -> `MW3 2`).
pub(crate) fn short_section(title: &str) -> String {
    const SHORT: [(&str, &str); 14] = [
        ("MODERN WARFARE REMASTERED", "MWR"),
        ("MODERN WARFARE II (2022)", "MWII"),
        ("BLACK OPS III", "BO3"),
        ("BLACK OPS II", "BO2"),
        ("BLACK OPS", "BO1"),
        ("MODERN WARFARE III (2023)", "MWIII"),
        ("MODERN WARFARE III", "MWIII"),
        ("INFINITE WARFARE", "IW"),
        ("ADVANCED WARFARE", "AW"),
        ("MODERN WARFARE 3", "MW3"),
        ("MODERN WARFARE 2", "MW2"),
        ("WORLD AT WAR", "WAW"),
        ("CALL OF DUTY 4", "COD4"),
        ("CUSTOM MAPS", "CUSTOM"),
    ];
    SHORT
        .iter()
        .find_map(|(long, short)| title.strip_prefix(long).map(|rest| format!("{short}{rest}")))
        .unwrap_or_else(|| title.to_owned())
}

fn mode_picker(c: &mut Kids, cx: &mut Ctx, shell: &Shell, cat: &Catalog, rows: &[Row]) {
    header(c, cx, &["CUSTOM GAME", "SELECT MODE"]);
    c.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(0.0), Some(176.0), None, None, cx) }).with_children(|col| {
        super::ghosts::rows(col, cx, rows, shell.focus, super::ghosts::Look { h: 46.0, px: 26.0, ..super::ghosts::Look::list(640.0) }.bare());
    });
    // The mode in focus, as Ghosts' FIND MATCH shows a playlist: its mark,
    // its name, what it is.
    if let Some(m) = cat.modes.get(shell.focus) {
        let mut names = vec![m.picture.clone()];
        names.extend(super::matchmaking::picture(&m.key));
        names.retain(|n| !n.is_empty());
        let mark = cx.pic(&names);
        let (ground, rim) = super::ghosts::panel_ground(cx);
        c.spawn((
            Node {
                width: cx.px(560.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::all(cx.px(28.0)),
                row_gap: cx.px(10.0),
                border: UiRect::top(cx.px(2.0)),
                ..abs(None, Some(196.0), Some(72.0), None, cx)
            },
            ground,
            rim,
            super::anim::from_left(cx.s, -80.0, 0.04),
        ))
        .with_children(|p| {
            if let Some(h) = mark {
                p.spawn((ImageNode::new(h), Node { width: cx.px(150.0), height: cx.px(150.0), ..default() }));
            }
            p.spawn((Text::new(m.title.clone()), cx.bold(44.0), TextColor(WHITE)));
            p.spawn((Text::new(m.desc.clone()), cx.regular(23.0), TextColor(TEXT), TextLayout::justify(bevy::text::Justify::Center)));
            if m.key == shell.mode {
                p.spawn((Text::new("SELECTED"), cx.bold(19.0), TextColor(cx.pal.accent)));
            }
        });
    }
}

/// Options, as Ghosts': the category panel and its settings card, or a
/// sub-page (`opt_view`).
pub(crate) fn options_view(c: &mut Kids, cx: &mut Ctx, shell: &Shell, rows: &[Row]) {
    super::opt_view::view(c, cx, shell, rows);
}

/// Barracks, as Ghosts' PLAYER STATS: the rows from the left edge, the
/// soldier preview's place in the middle, and at the right his banner (round
/// emblem, name, the big level over his calling card), his progress, then
/// the last five games' chart and his numbers (or, on CALLING CARD / EMBLEM,
/// his choice among its neighbours; on PRESTIGE, the prestige icons).
fn barracks_view(c: &mut Kids, cx: &mut Ctx, shell: &Shell, cat: &Catalog, rows: &[Row]) {
    header_ex(c, cx, &[section(shell), "PLAYER STATS"], None, true);
    c.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(0.0), Some(196.0), None, None, cx) })
        .with_children(|col| super::ghosts::rows(col, cx, rows, shell.focus, super::ghosts::Look { sub: Some(60.0), ..super::ghosts::Look::list(500.0) }));
    super::ghosts::soldier_preview(c, cx, 530.0, 170.0, 560.0, 780.0);
    let me = &cat.player;
    let rec = |k: &str| me.record.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
    let num = |k: &str| rec(k).and_then(|v| v.parse::<f64>().ok());
    let (wins, losses) = (num("WINS"), num("LOSSES"));
    let dash = || "—".to_owned();
    let mut list: Vec<(String, String)> = vec![
        ("K/D Ratio".to_owned(), rec("K/D RATIO").unwrap_or_else(dash)),
        ("Preferred Weapon".to_owned(), dash()),
        ("Current Streak".to_owned(), dash()),
        ("Wins".to_owned(), rec("WINS").unwrap_or_else(dash)),
        ("Games Played".to_owned(), match (wins, losses) {
            (Some(w), Some(l)) => format!("{}", (w + l) as i64),
            _ => dash(),
        }),
        ("W/L Ratio".to_owned(), match (wins, losses) {
            (Some(w), Some(l)) => format!("{:.2}", if l > 0.0 { w / l } else { w }),
            _ => dash(),
        }),
        ("Time Played".to_owned(), rec("TIME PLAYED").unwrap_or_else(dash)),
    ];
    // (The rest of his record, as before.)
    for (k, v) in &me.record {
        if !matches!(k.as_str(), "K/D RATIO" | "WINS" | "TIME PLAYED") {
            list.push((title_word(&k.to_lowercase()), v.clone()));
        }
    }
    let prestige = if me.prestige > 0 { format!("PRESTIGE {}  ·  ", me.prestige) } else { String::new() };
    let st = super::ghosts::Stats {
        name: &me.name,
        rank_title: if me.prestige > 0 { format!("Prestige {}", me.prestige) } else { "Soldier".to_owned() },
        rank_icon: vec![me.rank_picture.clone()],
        level: me.level.to_string(),
        card: cards::mine(Kind::Card, &me.card),
        emblem: cards::mine(Kind::Emblem, &me.emblem),
        progress: (
            format!("{prestige}LEVEL {} OF {}", me.level, bo2_profile::progression::max_level()),
            me.xp_frac,
            format!("{} XP  ·  NEXT LEVEL AT {}", me.xp, me.xp_next),
        ),
        chart_title: "Kill / Death Ratio (Last 5 Games)",
        chart: Vec::new(),
        chart_whole: false,
        list,
    };
    let focused = rows.get(shell.focus).map(|r| r.act);
    super::ghosts::stats_panel(c, cx, &st, |p, cx| match focused {
        Some(Act::Looks(kind)) => {
            p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: cx.px(14.0), margin: UiRect::top(cx.px(10.0)), ..default() })
                .with_children(|l| look_strip(l, cx, kind, if kind == Kind::Card { &me.card } else { &me.emblem }));
        }
        Some(Act::Prestige) => {
            p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: cx.px(10.0), margin: UiRect::top(cx.px(10.0)), ..default() })
                .with_children(|l| prestige_grid(l, cx, me.prestige));
        }
        _ => super::ghosts::stats_body(p, cx, &st),
    });
}

/// Every prestige icon, by the game it comes from (the ones he has reached
/// bright, his own framed).
fn prestige_grid(p: &mut Kids, cx: &mut Ctx, mine: i32) {
    p.spawn((Text::new("PRESTIGE"), cx.bold(22.0), TextColor(FAINT)));
    let parts: Vec<(&str, i32, i32)> = if bo2_profile::progression::combined() {
        vec![
            ("BLACK OPS", 1, bo2_profile::progression::BO1_PRESTIGES),
            ("MODERN WARFARE 2", 1 + bo2_profile::progression::BO1_PRESTIGES, bo2_profile::progression::MW2_PRESTIGES),
            ("MODERN WARFARE 3", 1 + bo2_profile::progression::BO1_PRESTIGES + bo2_profile::progression::MW2_PRESTIGES, bo2_profile::progression::MW3_PRESTIGES),
        ]
    } else {
        // (Black Ops II's own: prestiges 1 to 10, the Master after them.)
        vec![("BLACK OPS II", 1, bo2_profile::progression::BO2_MAX_PRESTIGE - 1)]
    };
    for (title, first, count) in parts {
        p.spawn((Text::new(format!("{title}  ·  PRESTIGE {first}-{}", first + count - 1)), cx.bold(18.0), TextColor(DIM)));
        p.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: cx.px(6.0), row_gap: cx.px(6.0), ..default() }).with_children(|g| {
            for n in first..first + count {
                let pic = cx.pic(&[bo2_profile::progression::rank_picture(1, n)]);
                let reached = mine >= n;
                g.spawn((
                    Node { width: cx.px(42.0), height: cx.px(42.0), border: UiRect::all(cx.px(if mine == n { 2.0 } else { 0.0 })), ..default() },
                    BorderColor::all(cx.pal.accent),
                    BackgroundColor(cx.pal.panel_soft),
                ))
                .with_children(|b| {
                    if let Some(h) = pic {
                        b.spawn((
                            ImageNode::new(h).with_color(Color::srgba(1.0, 1.0, 1.0, if reached { 1.0 } else { 0.28 })),
                            Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                        ));
                    }
                });
            }
        });
    }
    let master = bo2_profile::progression::max_prestige();
    p.spawn((Text::new(format!("MASTER PRESTIGE  ·  PRESTIGE {master}")), cx.bold(18.0), TextColor(GOLD)));
}

/// Barracks: his calling card or emblem among its neighbours in the list
/// (ENTER opens the picker).
fn look_strip(p: &mut Kids, cx: &mut Ctx, kind: Kind, stat: &str) {
    let list = cards::list(kind);
    let now = cards::mine(kind, stat);
    let (at, n) = cards::place_in(&list, &now);
    let what = if kind == Kind::Card { "CALLING CARD" } else { "EMBLEM" };
    p.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|t| {
        t.spawn((Text::new(format!("{what}  ·  {}", cards::title(&now).to_uppercase())), cx.bold(22.0), TextColor(FAINT)));
        t.spawn((Text::new(format!("{at} OF {n}")), cx.bold(22.0), TextColor(DIM)));
    });
    let i = at.saturating_sub(1) as i32;
    // (Sized for the stats panel at the right.)
    let (w, h) = if kind == Kind::Card { (190.0, 44.0) } else { (84.0, 84.0) };
    let side = if kind == Kind::Card { 1 } else { 2 };
    p.spawn(Node { column_gap: cx.px(12.0), align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default() }).with_children(|row| {
        for d in -side..=side {
            if list.is_empty() {
                break;
            }
            let name = &list[(i + d).rem_euclid(list.len() as i32) as usize];
            let on = d == 0;
            row.spawn((
                Node { padding: UiRect::all(cx.px(3.0)), border: UiRect::all(cx.px(2.0)), ..default() },
                BorderColor::all(if on { cx.pal.accent } else { Color::NONE }),
                BackgroundColor(if on { cx.pal.accent_a(0.18) } else { Color::NONE }),
            ))
            .with_children(|b| {
                let k = if on { 1.0 } else { 0.8 };
                if kind == Kind::Card {
                    banner(b, cx, name, w * k, h * k, None);
                } else {
                    emblem(b, cx, name, w * k);
                }
            });
        }
    });
    p.spawn((Text::new(format!("Press ENTER to choose your {} from every page.", what.to_lowercase())), cx.regular(19.0), TextColor(DIM)));
}


/// The CALL OF DUTY logo, drawn: the words in the black condensed weight,
/// a small OF stacked between them, filled with a brushed-silver sweep
/// (light at the top, a bright band under the middle, darker at the foot:
/// the words are drawn once per band, each band clipped to its strip) over
/// a soft dark shadow.
fn logo(c: &mut Kids, cx: &Ctx, left: f32, top: f32, px: f32) {
    // The logo's box: the words' line height, the caps sit in it.
    // (Wide enough for the whole of DUTY: the bands clip to it.)
    let (w, h) = (px * 4.8, px * 0.92);
    let words = |l: &mut Kids, color: Color, shadow: bool| {
        l.spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: cx.px(-px * 0.24),
            column_gap: cx.px(px * 0.035),
            align_items: AlignItems::FlexStart,
            ..default()
        })
        .with_children(|r| {
            let shade = |px: f32| TextShadow { offset: Vec2::new(0.0, px * 0.035 * cx.s), color: Color::srgba(0.0, 0.0, 0.0, if shadow { 0.85 } else { 0.0 }), ..default() };
            r.spawn((Text::new("CALL"), cx.black(px), TextColor(color), shade(px)));
            r.spawn((Text::new("OF"), cx.black(px * 0.40), TextColor(color), shade(px * 0.4), Node { margin: UiRect::top(cx.px(px * 0.17)), ..default() }));
            r.spawn((Text::new("DUTY"), cx.black(px), TextColor(color), shade(px)));
        });
    };
    c.spawn(Node { width: cx.px(w), height: cx.px(h), ..abs(Some(left), Some(top), None, None, cx) }).with_children(|l| {
        // The shadow under it all (the darkest copy, offset).
        words(l, Color::srgba(0.0, 0.0, 0.0, 0.0), true);
        // The silver: (where the band starts, 0..1 down the caps, its grey).
        const SWEEP: [(f32, f32); 6] = [(0.0, 0.97), (0.30, 0.86), (0.52, 0.70), (0.60, 0.92), (0.78, 0.66), (1.0, 0.50)];
        let bands = 22;
        for i in 0..bands {
            let t = (i as f32 + 0.5) / bands as f32;
            let k = SWEEP.windows(2).find(|p| t >= p[0].0 && t <= p[1].0).map_or(0.6, |p| {
                let f = (t - p[0].0) / (p[1].0 - p[0].0);
                p[0].1 + (p[1].1 - p[0].1) * f
            });
            let band = h / bands as f32;
            l.spawn(Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: cx.px(band * i as f32),
                width: Val::Percent(100.0),
                height: cx.px(band + 0.6),
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|b| {
                b.spawn(Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: cx.px(-band * i as f32), width: cx.px(w), height: cx.px(h), ..default() })
                    .with_children(|inner| words(inner, Color::srgb(k * 0.96, k * 0.975, k), false));
            });
        }
    });
}

/// The main menu, laid out as Ghosts' front end in the project's blue: a dark
/// band across the top with the title behind an accent bar, rows at the left
/// with their text pushed to the right edge and a thin rule between them (the
/// one in focus a blue band), the line about it under them, and a footer band
/// with the ESC hint.
/// The Ghosts-style frame shared by the main menu and the Zombies menu: the
/// bands, the title behind its accent bar, the rows, and the line under them.
pub(crate) fn ghosts_frame(c: &mut Kids, cx: &mut Ctx, shell: &Shell, rows: &[Row], title: &str) {
    let left = 150.0;
    let acc = cx.pal.accent;
    // The top and foot bands.
    c.spawn((super::ghosts::top_shade(cx, 150.0), BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::new(Color::srgba(0.0, 0.01, 0.03, 0.80), Val::Percent(0.0)), ColorStop::new(Color::srgba(0.0, 0.01, 0.03, 0.0), Val::Percent(100.0))]))));
    c.spawn((Node { width: Val::Percent(70.0), height: cx.px(3.0), ..abs(Some(0.0), Some(150.0), None, None, cx) }, fade_right(acc.with_alpha(0.65), acc.with_alpha(0.0))));
    c.spawn((super::ghosts::foot_shade(cx, 90.0), BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::new(Color::srgba(0.0, 0.01, 0.03, 0.0), Val::Percent(0.0)), ColorStop::new(Color::srgba(0.0, 0.01, 0.03, 0.82), Val::Percent(100.0))]))));
    // The title behind its accent bar.
    c.spawn((Node { column_gap: cx.px(14.0), align_items: AlignItems::Center, ..abs(Some(left), Some(60.0), None, None, cx) }, super::anim::from_left(cx.s, 60.0, 0.0))).with_children(|t| {
        t.spawn((Node { width: cx.px(7.0), height: cx.px(40.0), ..default() }, BackgroundColor(acc)));
        t.spawn((Text::new(title.to_owned()), cx.regular(54.0), TextColor(WHITE)));
    });
    // The rows: a 760-wide column from the safe frame's left edge (the
    // title-safe margin, `ghosts::SAFE_X`), text at the right.
    c.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(0.0), Some(200.0), None, None, cx) }).with_children(|col| {
        super::ghosts::rows(col, cx, rows, shell.focus, super::ghosts::Look { width: 760.0, h: 62.0, px: 34.0, sub: Some(left) });
    });
}

pub(crate) fn main_menu(c: &mut Kids, cx: &mut Ctx, shell: &Shell, rows: &[Row], online: &OnlineInfo) {
    let left = 150.0;
    if super::classic() {
        // The Classic layout: the logo, then the rows below it, left-aligned,
        // the one in focus white with an accent bar and the line about it.
        let acc = cx.pal.accent;
        c.spawn((super::ghosts::foot_shade(cx, 90.0), BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::new(Color::srgba(0.0, 0.01, 0.03, 0.0), Val::Percent(0.0)), ColorStop::new(Color::srgba(0.0, 0.01, 0.03, 0.82), Val::Percent(100.0))]))));
        logo(c, cx, left, 70.0, 120.0);
        c.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(left), Some(250.0), None, None, cx) }).with_children(|col| {
            for (i, r) in rows.iter().enumerate() {
                let on = i == shell.focus;
                col.spawn((Hit::Row(i), Button, Node { height: cx.px(54.0), column_gap: cx.px(14.0), align_items: AlignItems::Center, ..default() })).with_children(|b| {
                    b.spawn((Node { width: cx.px(6.0), height: cx.px(34.0), ..default() }, BackgroundColor(if on { acc } else { Color::NONE })));
                    b.spawn((Text::new(r.label.clone()), cx.regular(36.0), TextColor(if on { WHITE } else { DIM })));
                });
                if on {
                    col.spawn((Node { margin: UiRect::left(cx.px(20.0)), ..default() },)).with_child((Text::new(r.sub.clone()), cx.regular(19.0), TextColor(DIM)));
                }
            }
        });
    } else {
        ghosts_frame(c, cx, shell, rows, "MAIN MENU");
    }
    let acc = cx.pal.accent;
    // ESC at the foot (a hit target too), the server's state at the right.
    c.spawn((Hit::Back, Button, Node { column_gap: cx.px(10.0), align_items: AlignItems::Center, ..abs(Some(left), None, None, Some(30.0), cx) })).with_children(|b| {
        b.spawn((Text::new("ESC"), cx.bold(26.0), TextColor(acc)));
        b.spawn((Text::new("EXIT"), cx.regular(26.0), TextColor(WHITE)));
    });
    let (dot, label) = match online.status {
        Status::Online => (GREEN, format!("ONLINE  ·  {} PLAYERS", fmt_count(online.players))),
        Status::Connecting => (GOLD, "CONNECTING".to_owned()),
        Status::Unreachable => (ENEMY, "SERVER UNREACHABLE".to_owned()),
        Status::Offline => (FAINT, "OFFLINE  ·  LOCAL PLAY".to_owned()),
    };
    c.spawn(Node { column_gap: cx.px(24.0), align_items: AlignItems::Center, ..abs(None, None, Some(72.0), Some(22.0), cx) }).with_children(|r| {
        if !cx.pad {
            hint(r, cx, "F10", "", "GAME FOLDERS");
        }
        r.spawn(Node { column_gap: cx.px(10.0), align_items: AlignItems::Center, ..default() }).with_children(|s| {
            s.spawn((Node { width: cx.px(9.0), height: cx.px(9.0), border_radius: BorderRadius::all(Val::Percent(50.0)), ..default() }, BackgroundColor(dot)));
            s.spawn((Text::new(label), cx.bold(18.0), TextColor(DIM)));
        });
    });
}

/// Multiplayer: the title, Ghosts' rows from the left edge with the line
/// about the one in focus, his card and his party at the right.
fn multiplayer(c: &mut Kids, cx: &mut Ctx, shell: &Shell, cat: &Catalog, rows: &[Row]) {
    header(c, cx, &["MULTIPLAYER", "MATCH SELECT"]);
    c.spawn(Node { flex_direction: FlexDirection::Column, ..abs(Some(0.0), Some(196.0), None, None, cx) })
        .with_children(|col| super::ghosts::rows(col, cx, rows, shell.focus, super::ghosts::Look::list(660.0)));
    party(c, cx, cat);
}

/// His party under his card: a line each (an arrow at his own), rank and
/// level, then the name.
fn party(c: &mut Kids, cx: &mut Ctx, cat: &Catalog) {
    let me = &cat.player;
    let icon = cx.pic(&[me.rank_picture.clone()]);
    c.spawn(Node { width: cx.px(640.0), flex_direction: FlexDirection::Column, ..abs(None, Some(196.0), Some(90.0), None, cx) }).with_children(|list| {
        list.spawn((
            Node { height: cx.px(38.0), column_gap: cx.px(14.0), align_items: AlignItems::Center, border: UiRect::bottom(cx.px(1.0)), ..default() },
            BorderColor::all(WHITE.with_alpha(0.35)),
        ))
        .with_children(|r| {
            pointer(r, cx);
            if let Some(h) = icon {
                r.spawn((ImageNode::new(h), Node { width: cx.px(28.0), height: cx.px(28.0), ..default() }));
            }
            r.spawn((Node { width: cx.px(44.0), ..default() },)).with_child((Text::new(me.level.to_string()), cx.regular(24.0), TextColor(WHITE)));
            r.spawn((Text::new(me.name.clone()), cx.regular(24.0), TextColor(WHITE)));
        });
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn shell_view(
    mut commands: Commands,
    shell: Res<Shell>,
    fonts: Option<Res<ShellFonts>>,
    online: Option<Res<OnlineInfo>>,
    mut pics: ResMut<ShellPics>,
    icons: Option<Res<assets::T6HudIcons>>,
    mut images: ResMut<Assets<Image>>,
    roots: Query<Entity, With<ViewRoot>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    time: Res<Time>,
    mut drawn: Local<String>,
) {
    let up = bridge::covers_lui();
    let cat = bridge::catalog();
    let (Some(fonts), Some(cat), true) = (fonts, cat, up) else {
        for e in &roots {
            commands.entity(e).despawn();
        }
        drawn.clear();
        return;
    };
    let online = online.as_deref().cloned().unwrap_or_default();
    let (area, s) = super::window_scale(&windows);
    let pad = crate::pad_glyphs::pad_active();
    // (The lobby's countdown in tenths: its line drains smoothly.)
    let starting = super::matchmaking::countdown(&shell).map(|(left, _)| (left * 10.0).ceil() as i32);
    // (While pictures are still being read the view looks again twice a
    // second.)
    let tick = if pics.pending() { (time.elapsed_secs() * 2.0) as u64 } else { 0 };
    let book = super::classes::book().map_or(0, |b| b.rev);
    let key = format!(
        "{:?}|{:p}|{}|{online:?}|{area:?}|{pad}|{starting:?}|{tick}|{book}|{}|{:?}|{}|{:?}|{:?}|{}|{}",
        *shell,
        std::sync::Arc::as_ptr(&cat),
        pics.count(),
        super::options::snapshot().revision,
        super::zombies::state(),
        super::online::generation(),
        (crate::online_match::room(), crate::online_match::p2p_view()),
        assets::prefetch_status(),
        super::OPT_FIRST.load(std::sync::atomic::Ordering::Relaxed),
        super::prepare::key()
    );
    if *drawn == key && !roots.is_empty() {
        return;
    }
    *drawn = key;
    for e in &roots {
        commands.entity(e).despawn();
    }
    let rows = rows(&shell, &cat);
    // (The slides restart on a new screen or page, the focus band on a new
    // focus: not on every redraw.)
    super::anim::note(&format!("{:?}/{}/{}/{}", shell.screen, shell.page, shell.searching, shell.slot), shell.focus, time.elapsed_secs());
    // bo2mp: the screens are drawn into the title-safe frame (the
    // backgrounds stay full-bleed, in their own root).
    let safe = super::ghosts::safe_area(area, s);
    let mut cx = Ctx {
        s: safe.s,
        w: safe.w,
        h: safe.h,
        fonts: &fonts,
        pics: &mut pics,
        icons: icons.as_deref(),
        images: &mut images,
        pad,
        pal: palette(&shell),
        still: false,
        bleed: safe.bleed,
    };
    commands
        .spawn((
            ViewRoot,
            GlobalZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(safe.inset.x),
                right: Val::Px(safe.inset.x),
                top: Val::Px(safe.inset.y),
                bottom: Val::Px(safe.inset.y),
                ..default()
            },
        ))
        .with_children(|c| {
            match shell.screen {
                Screen::Main => main_menu(c, &mut cx, &shell, &rows, &online),
                Screen::Multiplayer => multiplayer(c, &mut cx, &shell, &cat, &rows),
                Screen::Lobby => lobby(c, &mut cx, &shell, &cat, &rows),
                Screen::MapPick => map_picker(c, &mut cx, &shell, &cat),
                Screen::ModePick => mode_picker(c, &mut cx, &shell, &cat, &rows),
                Screen::Options => options_view(c, &mut cx, &shell, &rows),
                Screen::Barracks => barracks_view(c, &mut cx, &shell, &cat, &rows),
                Screen::Friends => super::friends::view(c, &mut cx, &shell, &rows),
                Screen::Lobbies if shell.searching => super::matchmaking::search_view(c, &mut cx, &shell, &cat, &rows),
                Screen::Lobbies => super::lobbies::view(c, &mut cx, &shell, &rows),
                Screen::MatchFilter => super::matchmaking::view(c, &mut cx, &shell, &cat, &rows, &online),
                Screen::Room => super::room::view(c, &mut cx, &shell, &cat, &rows),
                Screen::CardPick | Screen::EmblemPick => super::looks::view(c, &mut cx, &shell, &cat, &rows),
                Screen::Classes | Screen::ClassEdit | Screen::ItemPick => {
                    if let Some(book) = super::classes::book() {
                        super::cac::view(c, &mut cx, &shell, &book, &rows);
                    }
                }
                Screen::Zombies | Screen::ZmMapPick | Screen::ZmStartPick | Screen::ZmLobby | Screen::ZmRecord => {
                    super::zombies_view::view(c, &mut cx, &shell, &rows);
                }
                Screen::WzMenu | Screen::WzLobby | Screen::WzModePick => super::wz_view::view(c, &mut cx, &shell, &cat, &rows),
            }
            // (bo2mp: no big MATCH STARTS IN block at the lower right: the
            // map card's STARTING IN line and the list's header carry it.)
            // (The main menu has its own foot, and no player card; the
            // calling card and emblem pickers show his card in their middle.)
            // (Nor the lobbies, FIND A MATCH and the stats pages: Ghosts'
            // status line is at their top right.)
            if shell.screen.is_zombies() && shell.screen != Screen::ZmRecord {
                super::zombies_view::zm_card(c, &mut cx, &cat.player);
            // (bo2mp: nor the map picker and Options, whose tab rows run
            // under the card.)
            } else if !shell.screen.is_zombies()
                && !matches!(
                    shell.screen,
                    Screen::Main | Screen::CardPick | Screen::EmblemPick | Screen::Classes | Screen::ClassEdit | Screen::ItemPick | Screen::MapPick | Screen::Options | Screen::Lobby | Screen::Lobbies | Screen::Room | Screen::MatchFilter | Screen::Barracks
                )
            {
                player_card(c, &mut cx, &cat.player);
            }
            if shell.screen != Screen::Main {
                footer(c, &mut cx, &shell, &cat, &online);
            }
            if let Some(ask) = shell.ask {
                question(c, &mut cx, ask, shell.ask_focus);
            }
            // (PREPARE ALL MAPS' bar, above the notice line, on every
            // screen while it runs.)
            super::prepare::draw(c, &cx);
            if let Some((note, _)) = &shell.note {
                notice(c, &cx, note);
            }
        });
}

/// A yes / no question over the screen, as Ghosts asks one
/// (`ghosts::popup`): the screen dimmed, a dark band across it with the
/// question and what it means, and YES / NO (or OK alone for a notice) as
/// bands in the rows' look.
pub(crate) fn question(c: &mut Kids, cx: &mut Ctx, ask: super::Ask, focus: usize) {
    let (title, body) = ask.text();
    super::ghosts::popup(c, cx, title, &body, 40, |col, cx| {
        for (i, &label) in ask.buttons().iter().enumerate() {
            super::ghosts::band(col, cx, label, i == focus, Some(Hit::Ask(i)), super::ghosts::Look { width: super::ghosts::INSET + 320.0, h: 50.0, px: 28.0, sub: None }, 0.04 + i as f32 * 0.025, false);
        }
    });
}
