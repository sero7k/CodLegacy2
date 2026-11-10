//! bo2mp: text chat on screen, as Black Ops II draws it: the lines received
//! (the server's `bo2mp_chat`) on the left a little below the middle of the
//! screen, newest at the bottom, at most five, each for twelve seconds
//! (`cg_chatHeight` 5, `cg_chatTime` 12000); and the line being typed
//! ("SAY: ..." / "SAY TEAM: ...") just under them. A line reads
//! "<name> (Playing-All): <text>": the name part in the sender's team colour
//! (white in modes without teams), the text light grey. Each new line is
//! also told as [`frame::ChatLineReceived`] (the speech side listens).
//!
//! Placed from the owner's footage of the real game at 1280x720 (units of
//! a 720-line screen, scaled with the window's height): the newest line's
//! glyphs span x 20 and y 358-371, the typing line's caps x 8 and y 385-395;
//! the font's caps are 0.7 of its size and Bevy puts the baseline one size
//! below a line's top.

use std::collections::VecDeque;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use frame::{ChatLineReceived, ChatTyping, ClientSet};
use net::{LocalPresentClient, PresentedSnapshot};

use crate::layers::{GameUiFont, UiLayer, UiLayerVisibility, game_text_font};

const LINES: usize = 5;
const LINE_LIFE: f32 = 12.0;
const FADE_OUT: f32 = 0.5;
/// The font's size on a 720-line screen.
const SIZE: f32 = 15.0;
/// The tops of the newest received line and of the typing line, and their
/// left edges, on a 720-line screen.
const NEWEST_TOP: f32 = 354.5;
const TYPING_TOP: f32 = 380.0;
const LINES_LEFT: f32 = 19.6;
const TYPING_LEFT: f32 = 7.5;
/// One received line above the next (Bevy's line height, 1.2 sizes).
const LINE_STEP: f32 = SIZE * 1.2;

const FRIENDLY: Color = Color::srgb(134.0 / 255.0, 154.0 / 255.0, 48.0 / 255.0);
const ENEMY: Color = Color::srgb(0.83, 0.33, 0.29);
const TEXT: Color = Color::srgb(0.78, 0.78, 0.78);
const TYPING: Color = Color::srgb(248.0 / 255.0, 248.0 / 255.0, 248.0 / 255.0);

/// One row of `bo2mp_chat`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChatRow {
    pub n: u32,
    pub sender: u32,
    pub team_only: bool,
    pub dead: bool,
    pub team_based: bool,
    pub team: String,
    pub name: String,
    pub text: String,
}

/// `bo2mp_chat`: rows by `\u{1e}`, fields by `\u{1f}` (see the server's
/// `natives_mp::chat`).
pub(crate) fn parse(raw: &str) -> Vec<ChatRow> {
    raw.split('\u{1e}')
        .filter_map(|row| {
            let f: Vec<&str> = row.splitn(8, '\u{1f}').collect();
            let [n, sender, team_only, dead, team_based, team, name, text] = f.as_slice() else {
                return None;
            };
            Some(ChatRow {
                n: n.trim().parse().ok()?,
                sender: sender.parse().ok()?,
                team_only: *team_only == "1",
                dead: *dead == "1",
                team_based: *team_based == "1",
                team: (*team).to_owned(),
                name: (*name).to_owned(),
                text: (*text).to_owned(),
            })
        })
        .collect()
}

/// "<name> (Playing-All): ", as the line shows the sender.
pub(crate) fn sender_label(row: &ChatRow) -> String {
    format!(
        "{} ({}-{}): ",
        row.name,
        if row.dead { "Dead" } else { "Playing" },
        if row.team_only { "Team" } else { "All" }
    )
}

/// The sender's colour: white without teams, else friendly or enemy.
pub(crate) fn sender_colour(row: &ChatRow, my_team: &str) -> Color {
    if !row.team_based {
        Color::WHITE
    } else if row.team == my_team {
        FRIENDLY
    } else {
        ENEMY
    }
}

#[derive(Component)]
struct ChatRoot;

#[derive(Default)]
struct ChatState {
    /// The newest row seen; none before the first look (rows from before
    /// it are old).
    last: Option<u32>,
    lines: VecDeque<(f64, String, Color, String)>,
    shown: Option<(Vec<(u64, String)>, Option<(bool, String)>, u32)>,
}

fn alpha(born: f64, now: f64) -> f32 {
    ((LINE_LIFE - (now - born) as f32) / FADE_OUT).clamp(0.0, 1.0)
}

/// Strips `^n` colour codes from a typed name or text for the plain parts.
fn plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '^' && chars.peek().is_some_and(char::is_ascii_digit) {
            chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn chat(
    mut commands: Commands,
    presented: Option<Res<PresentedSnapshot>>,
    local: Option<Res<LocalPresentClient>>,
    typing: Option<Res<ChatTyping>>,
    font: Option<Res<GameUiFont>>,
    window: Query<&Window, With<PrimaryWindow>>,
    roots: Query<Entity, With<ChatRoot>>,
    time: Res<Time<Real>>,
    mut received: MessageWriter<ChatLineReceived>,
    mut st: Local<ChatState>,
) {
    let clear = |commands: &mut Commands| {
        for e in &roots {
            commands.entity(e).despawn();
        }
    };
    let me = local.as_deref().map(|l| l.0.0);
    let dvars = presented.as_deref().zip(local.as_deref()).and_then(|(p, l)| {
        let snap = p.snapshot()?;
        let d = snap.meta.script_dvars(l.0);
        // (No multiplayer match of Black Ops II's: none sent.)
        d.string("bo2mp_gametype")?;
        Some((
            d.string("bo2mp_chat").unwrap_or_default().to_owned(),
            d.string("bo2mp_team").unwrap_or_default().to_owned(),
        ))
    });
    let Some((raw, my_team)) = dvars else {
        clear(&mut commands);
        *st = ChatState::default();
        return;
    };
    let now = time.elapsed_secs_f64();
    let rows = parse(&raw);
    let newest = rows.iter().map(|r| r.n).max();
    match (st.last, newest) {
        (None, n) => st.last = Some(n.unwrap_or(0)),
        (Some(last), Some(n)) => {
            // (A new match counts from 1 again.)
            let last = if n < last { 0 } else { last };
            for row in rows.iter().filter(|r| r.n > last) {
                received.write(ChatLineReceived {
                    sender: plain(&row.name),
                    text: plain(&row.text),
                    team_only: row.team_only,
                    from_local_player: Some(row.sender) == me,
                });
                st.lines.push_back((now, sender_label(row), sender_colour(row, &my_team), row.text.clone()));
            }
            st.last = Some(n);
        }
        (Some(_), None) => {}
    }
    st.lines.retain(|(born, ..)| ((now - born) as f32) < LINE_LIFE);
    while st.lines.len() > LINES {
        st.lines.pop_front();
    }
    let typing_line = typing
        .as_deref()
        .filter(|t| t.open)
        .map(|t| (t.team, t.text.clone()));
    if st.lines.is_empty() && typing_line.is_none() {
        clear(&mut commands);
        st.shown = None;
        return;
    }
    let Some(font) = font else {
        return;
    };
    let height = window.single().map_or(1080.0, Window::height);
    let step = |born: f64| (alpha(born, now) * 20.0).round() as u64;
    let key = (
        st.lines
            .iter()
            .map(|(b, l, _, t)| (step(*b) ^ b.to_bits(), format!("{l}{t}")))
            .collect::<Vec<_>>(),
        typing_line.clone(),
        height as u32,
    );
    if st.shown.as_ref() == Some(&key) && !roots.is_empty() {
        return;
    }
    clear(&mut commands);
    let u = height / 720.0;
    let px = SIZE * u;
    let shadow = |a: f32| TextShadow {
        offset: Vec2::splat((1.0 * u).max(1.0)),
        color: Color::srgba(0.0, 0.0, 0.0, 0.8 * a),
        ..default()
    };
    commands
        .spawn((
            ChatRoot,
            UiLayer::Hud,
            UiLayerVisibility,
            GlobalZIndex(102),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
        ))
        .with_children(|root| {
            let count = st.lines.len();
            for (i, (born, label, colour, text)) in st.lines.iter().enumerate() {
                let a = alpha(*born, now);
                let up = (count - 1 - i) as f32 * LINE_STEP;
                root.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(LINES_LEFT * u),
                        top: Val::Px((NEWEST_TOP - up) * u),
                        ..default()
                    },
                    Text::new(""),
                    game_text_font(&font.0, px),
                    TextColor(colour.with_alpha(a)),
                    shadow(a),
                ))
                .with_children(|t| {
                    t.spawn((TextSpan::new(label.clone()), game_text_font(&font.0, px), TextColor(colour.with_alpha(a)), shadow(a)));
                    t.spawn((TextSpan::new(text.clone()), game_text_font(&font.0, px), TextColor(TEXT.with_alpha(a)), shadow(a)));
                });
            }
            if let Some((team, text)) = &typing_line {
                root.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(TYPING_LEFT * u),
                        top: Val::Px(TYPING_TOP * u),
                        ..default()
                    },
                    Text::new(format!("{}{text}", if *team { "SAY TEAM: " } else { "SAY: " })),
                    game_text_font(&font.0, px),
                    TextColor(TYPING),
                    shadow(1.0),
                ));
            }
        });
    st.shown = Some(key);
}

pub(crate) fn register_chat_systems(app: &mut App) {
    frame::register_chat(app);
    app.add_systems(Update, chat.in_set(ClientSet::Ui));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(team_based: bool, team: &str) -> ChatRow {
        ChatRow {
            n: 1,
            sender: 3,
            team_only: false,
            dead: false,
            team_based,
            team: team.into(),
            name: "[GFS]Sero7k".into(),
            text: "text chat example".into(),
        }
    }

    #[test]
    fn a_row_reads_back_as_sent() {
        let raw = "4\u{1f}3\u{1f}1\u{1f}0\u{1f}1\u{1f}allies\u{1f}Sero7k\u{1f}hello: there\u{1e}5\u{1f}2\u{1f}0\u{1f}1\u{1f}0\u{1f}free\u{1f}Bot\u{1f}gg";
        let rows = parse(raw);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].text, "hello: there");
        assert!(rows[0].team_only && rows[0].team_based && !rows[0].dead);
        assert!(rows[1].dead && !rows[1].team_based);
    }

    #[test]
    fn the_sender_reads_as_in_the_game() {
        assert_eq!(sender_label(&row(true, "allies")), "[GFS]Sero7k (Playing-All): ");
        let mut r = row(true, "allies");
        r.dead = true;
        r.team_only = true;
        assert_eq!(sender_label(&r), "[GFS]Sero7k (Dead-Team): ");
    }

    #[test]
    fn names_are_white_without_teams_and_by_side_with_them() {
        assert_eq!(sender_colour(&row(false, "free"), "free"), Color::WHITE);
        assert_eq!(sender_colour(&row(true, "allies"), "allies"), FRIENDLY);
        assert_eq!(sender_colour(&row(true, "axis"), "allies"), ENEMY);
    }

    #[test]
    fn a_line_fades_out_at_the_end_of_its_twelve_seconds() {
        assert_eq!(alpha(0.0, 1.0), 1.0);
        assert!(alpha(0.0, 11.75) > 0.0 && alpha(0.0, 11.75) < 1.0);
        assert_eq!(alpha(0.0, 12.0), 0.0);
    }

    #[test]
    fn colour_codes_are_left_out_of_what_is_told() {
        assert_eq!(plain("^1Red^7Name"), "RedName");
    }
}
