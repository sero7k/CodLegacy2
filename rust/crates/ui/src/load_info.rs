//! bo2mp: a line along the bottom of the loading screen saying what the load
//! is doing and what has taken longest, so a slow map load shows why: the
//! total time so far, the stages running now (with their counts), and the
//! three slowest finished stages. When the load ends its slowest stages go
//! to the log as one line.

use bevy::prelude::*;
use frame::ClientSet;

use crate::layers::{GameUiFont, game_text_font};

#[derive(Component)]
struct InfoBar;

#[derive(Component)]
struct InfoBarRoot;

fn seconds(d: std::time::Duration) -> String {
    format!("{:.1}s", d.as_secs_f32())
}

/// The load that was on screen: its id, map, when it was asked for, and its
/// slowest stages.
struct LastLoad {
    id: u64,
    title: String,
    requested_at: std::time::Instant,
    line: String,
}

/// lane B (B4): one line per map change, for the overnight timing runs: the
/// map left, the map now playable, and the seconds from the change being
/// asked for to the loading screen going away.
fn map_to_map_line(left: Option<&str>, load: &LastLoad, changes: u32, now: std::time::Instant) -> String {
    let took = seconds(now.saturating_duration_since(load.requested_at));
    match left {
        Some(left) => format!(
            "map to map #{changes}: `{left}` -> `{}` playable in {took} (load #{})",
            load.title, load.id
        ),
        None => format!("first map: `{}` playable in {took} (load #{})", load.title, load.id),
    }
}

fn load_info_bar(
    mut commands: Commands,
    loading: Option<Res<assets::LoadingScreen>>,
    font: Option<Res<GameUiFont>>,
    roots: Query<Entity, With<InfoBarRoot>>,
    mut bars: Query<&mut Text, With<InfoBar>>,
    mut last: Local<Option<LastLoad>>,
    mut played: Local<(Option<String>, u32)>,
) {
    let Some(loading) = loading else {
        // The load is over: its slowest stages once, to the log.
        if let Some(load) = last.take() {
            diag::info!(Ui, "load #{} took: {}", load.id, load.line);
            let (left, changes) = &mut *played;
            if left.is_some() {
                *changes += 1;
            }
            diag::info!(Ui, "{}", map_to_map_line(left.as_deref(), &load, *changes, std::time::Instant::now()));
            *left = Some(load.title);
        }
        for e in &roots {
            commands.entity(e).despawn();
        }
        return;
    };
    let Some(font) = font else { return };
    let snap = loading.progress.snapshot();
    let total = snap.now.saturating_duration_since(snap.requested_at);
    let running: Vec<String> = snap
        .stages
        .iter()
        .filter(|s| s.running())
        .map(|s| {
            let count = s.count.map_or_else(String::new, |c| match c.total {
                Some(t) => format!(" {}/{}", c.completed, t),
                None => format!(" {}", c.completed),
            });
            format!("{}{count} ({})", s.key.label(), s.elapsed(snap.now).map_or_else(String::new, seconds))
        })
        .take(3)
        .collect();
    let mut done: Vec<(String, std::time::Duration)> = snap
        .stages
        .iter()
        .filter(|s| s.end.is_some())
        .filter_map(|s| Some((s.key.label(), s.elapsed(snap.now)?)))
        .collect();
    done.sort_by(|a, b| b.1.cmp(&a.1));
    let slowest: Vec<String> = done.iter().take(3).map(|(l, d)| format!("{l} {}", seconds(*d))).collect();
    let line = format!(
        "LOADING {}   |   NOW: {}   |   SLOWEST: {}",
        seconds(total),
        if running.is_empty() { "waiting for the world to appear".to_owned() } else { running.join(", ") },
        if slowest.is_empty() { "-".to_owned() } else { slowest.join(", ") }
    );
    *last = Some(LastLoad {
        id: snap.request_id,
        title: loading.title().to_owned(),
        requested_at: snap.requested_at,
        line: format!("{} in all; slowest {}", seconds(total), slowest.join(", ")),
    });
    if !roots.is_empty() {
        let Some(mut text) = bars.iter_mut().next() else { return };
        if text.0 != line {
            text.0 = line;
        }
        return;
    }
    commands
        .spawn((
            InfoBarRoot,
            GlobalZIndex(20_002),
            // (At the lower right of the shell loading screen, under the
            // progress bar's end.)
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(150.0),
                bottom: Val::Px(4.0),
                padding: UiRect::axes(Val::Px(0.0), Val::Px(2.0)),
                ..default()
            },
        ))
        .with_children(|c| {
            c.spawn((
                InfoBar,
                Text::new(line),
                game_text_font(&font.0, 14.0),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.55)),
            ));
        });
}

pub(crate) fn register_load_info(app: &mut App) {
    app.add_systems(Update, load_info_bar.in_set(ClientSet::Ui));
}

#[cfg(test)]
mod tests {
    use super::{LastLoad, map_to_map_line};
    use std::time::{Duration, Instant};

    #[test]
    fn map_to_map_line_names_both_maps_and_the_time() {
        let asked = Instant::now();
        let load = LastLoad { id: 7, title: "Takeoff".into(), requested_at: asked, line: String::new() };
        let now = asked + Duration::from_millis(12_340);
        assert_eq!(
            map_to_map_line(Some("Nuketown 2025"), &load, 1, now),
            "map to map #1: `Nuketown 2025` -> `Takeoff` playable in 12.3s (load #7)"
        );
        assert_eq!(map_to_map_line(None, &load, 0, now), "first map: `Takeoff` playable in 12.3s (load #7)");
    }
}
