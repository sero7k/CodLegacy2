//! bo2mp: the scripts' `iprintln` and `iprintlnbold` lines (BO2's game
//! messages), from the server's `bo2mp_prints`: plain lines at the bottom
//! left, above the kill feed, at most five, each for five seconds; bold
//! lines in the middle of the screen's upper third, at most three, three
//! seconds each (`con_gameMsgWindow0` / the bold window's line counts and
//! times). Colour codes (`^1`..`^9`) colour the runs and `[{+command}]`
//! shows the player's key, as the script HUD's text does.

use std::collections::VecDeque;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use frame::{ClientSet, HudInputView};
use net::{LocalPresentClient, PresentedSnapshot};

use crate::layers::{GameUiFont, UiLayer, UiLayerVisibility, game_text_font};

const LINES: usize = 5;
const LINE_LIFE: f32 = 5.0;
const BOLD_LINES: usize = 3;
const BOLD_LIFE: f32 = 3.0;
/// The last part of a line's life it fades out over.
const FADE_OUT: f32 = 0.5;

#[derive(Component)]
struct PrintRoot;

#[derive(Default)]
struct PrintState {
    /// The newest line seen; none before the first look (lines from before
    /// it are old).
    last: Option<u32>,
    lines: VecDeque<(f64, String)>,
    bold: VecDeque<(f64, String)>,
    shown: Option<(Vec<(u64, String)>, Vec<(u64, String)>, u32)>,
}

/// `bo2mp_prints`: "<n>\u{1f}<bold>\u{1f}<text>" rows by `\u{1e}`.
fn parse(prints: &str) -> Vec<(u32, bool, String)> {
    prints
        .split('\u{1e}')
        .filter_map(|row| {
            let mut f = row.splitn(3, '\u{1f}');
            let n = f.next()?.trim().parse().ok()?;
            let bold = f.next()? == "1";
            Some((n, bold, f.next()?.to_owned()))
        })
        .collect()
}

fn alpha(born: f64, life: f32, now: f64) -> f32 {
    ((life - (now - born) as f32) / FADE_OUT).clamp(0.0, 1.0)
}

#[allow(clippy::too_many_arguments)]
fn prints(
    mut commands: Commands,
    presented: Option<Res<PresentedSnapshot>>,
    local: Option<Res<LocalPresentClient>>,
    keys: Option<Res<HudInputView>>,
    font: Option<Res<GameUiFont>>,
    window: Query<&Window, With<PrimaryWindow>>,
    roots: Query<Entity, With<PrintRoot>>,
    time: Res<Time<Real>>,
    mut st: Local<PrintState>,
) {
    let clear = |commands: &mut Commands| {
        for e in &roots {
            commands.entity(e).despawn();
        }
    };
    let dvars = presented
        .as_deref()
        .zip(local.as_deref())
        .and_then(|(p, l)| {
            let snap = p.snapshot()?;
            let d = snap.meta.script_dvars(l.0);
            // (No multiplayer match of Black Ops II's: none sent.)
            d.string("bo2mp_gametype")?;
            Some((
                d.string("bo2mp_prints").unwrap_or_default().to_owned(),
                d.string("bo2zm_hud_hidden") == Some("1")
                    || d.string("bo2mp_killcam").is_some_and(|k| !k.is_empty()),
            ))
        });
    let Some((raw, hidden)) = dvars else {
        clear(&mut commands);
        *st = PrintState::default();
        return;
    };
    let now = time.elapsed_secs_f64();
    let rows = parse(&raw);
    let newest = rows.iter().map(|r| r.0).max();
    match (st.last, newest) {
        (None, n) => st.last = Some(n.unwrap_or(0)),
        (Some(last), Some(n)) => {
            // (A new match counts from 1 again.)
            let last = if n < last { 0 } else { last };
            for (_, bold, text) in rows.iter().filter(|r| r.0 > last) {
                if *bold {
                    st.bold.push_back((now, text.clone()));
                } else {
                    st.lines.push_back((now, text.clone()));
                }
            }
            st.last = Some(n);
        }
        (Some(_), None) => {}
    }
    st.lines
        .retain(|(born, _)| ((now - born) as f32) < LINE_LIFE);
    st.bold
        .retain(|(born, _)| ((now - born) as f32) < BOLD_LIFE);
    while st.lines.len() > LINES {
        st.lines.pop_front();
    }
    while st.bold.len() > BOLD_LINES {
        st.bold.pop_front();
    }
    if hidden || (st.lines.is_empty() && st.bold.is_empty()) {
        clear(&mut commands);
        st.shown = None;
        return;
    }
    let (Some(font), Some(keys)) = (font, keys) else {
        return;
    };
    let height = window.single().map_or(1080.0, Window::height);
    // Redrawn as the lines change or fade (in steps of a twentieth).
    let step = |born: f64, life: f32| (alpha(born, life, now) * 20.0).round() as u64;
    let key = (
        st.lines
            .iter()
            .map(|(b, t)| (step(*b, LINE_LIFE) ^ b.to_bits(), t.clone()))
            .collect::<Vec<_>>(),
        st.bold
            .iter()
            .map(|(b, t)| (step(*b, BOLD_LIFE) ^ b.to_bits(), t.clone()))
            .collect::<Vec<_>>(),
        height as u32,
    );
    if st.shown.as_ref() == Some(&key) && !roots.is_empty() {
        return;
    }
    clear(&mut commands);
    let k = (height / 1080.0).max(0.4);
    let shadow = |a: f32| TextShadow {
        offset: Vec2::splat((2.0 * k).max(1.0)),
        color: Color::srgba(0.0, 0.0, 0.0, 0.8 * a),
        ..default()
    };
    let line = |p: &mut ChildSpawnerCommands, text: &str, px: f32, a: f32| {
        let runs = crate::zm_hud::hint_runs(text, &keys);
        p.spawn((
            Text::new(""),
            game_text_font(&font.0, px),
            TextColor(Color::WHITE.with_alpha(a)),
            shadow(a),
        ))
        .with_children(|t| {
            for (run, colour) in runs {
                t.spawn((
                    TextSpan::new(run),
                    game_text_font(&font.0, px),
                    TextColor(colour.with_alpha(a)),
                    shadow(a),
                ));
            }
        });
    };
    commands
        .spawn((
            PrintRoot,
            UiLayer::Hud,
            UiLayerVisibility,
            GlobalZIndex(101),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn(Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(1.2),
                // (Above the kill feed's five rows.)
                bottom: Val::Px(height * 0.026 + 210.0 * k),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                row_gap: Val::Px(2.0 * k),
                ..default()
            })
            .with_children(|col| {
                for (born, text) in &st.lines {
                    line(col, text, 22.0 * k, alpha(*born, LINE_LIFE, now));
                }
            });
            root.spawn(Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(20.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(4.0 * k),
                ..default()
            })
            .with_children(|col| {
                for (born, text) in &st.bold {
                    line(col, text, 32.0 * k, alpha(*born, BOLD_LIFE, now));
                }
            });
        });
    st.shown = Some(key);
}

pub(crate) fn register_print_systems(app: &mut App) {
    app.add_systems(Update, prints.in_set(ClientSet::Ui));
}
