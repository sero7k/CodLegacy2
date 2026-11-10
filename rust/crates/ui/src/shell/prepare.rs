//! bo2mp: PREPARE ALL MAPS' bar on the menus. While the run makes the maps'
//! fast-load files (at launch with PREPARE MAPS AT LAUNCH on, or from
//! Options > GAME FOLDERS > PREPARE ALL MAPS NOW) a panel above the notice
//! line shows "map N of M", the map, its step and counts, and a bar for the
//! whole run, and the time left for the checked games (from the times
//! measured on this PC: `map_bake::ahead::eta_left`). Esc stops it
//! (`shell_input`); the next run goes on from there.

use bevy::prelude::*;

use super::theme::*;
use super::view::{Ctx, Kids, abs};

/// What the bar shows now, `None` with no run going.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Bar {
    /// "Map 3 of 37: Black Ops 3 - Skyjacked", or "Starting...".
    pub title: String,
    /// The step and its counts ("pictures 412/1530 (first load)").
    pub step: String,
    /// The whole run, 0..=1.
    pub done: f32,
    /// The time left ("about 12 min left"), empty before any map is timed.
    pub left: String,
}

/// A stand-in bar for the menus' preview pictures (`shell_preview`).
static PREVIEW: std::sync::Mutex<Option<Bar>> = std::sync::Mutex::new(None);

pub(crate) fn set_preview(b: Option<Bar>) {
    if let Ok(mut p) = PREVIEW.lock() {
        *p = b;
    }
}

/// The time left as the bar shows it.
fn left_text(e: Option<assets::bake_all::Eta>) -> String {
    match e {
        Some(e) if e.untimed > 0 && e.secs > 0.0 => format!("{} left + {} maps not timed yet", super::options::eta_text(e.secs), e.untimed),
        Some(e) if e.untimed > 0 => format!("{} maps left, not timed yet", e.untimed),
        Some(e) => format!("{} left", super::options::eta_text(e.secs)),
        None => String::new(),
    }
}

/// The bar's contents from the run's place and its current job.
pub(crate) fn bar() -> Option<Bar> {
    if let Some(b) = PREVIEW.lock().ok().and_then(|p| p.clone()) {
        return Some(b);
    }
    if !assets::bake_all::running() {
        return None;
    }
    let Some(now) = assets::bake_all::now() else {
        return Some(Bar { title: "Starting...".to_owned(), step: String::new(), done: 0.0, left: String::new() });
    };
    let job = assets::bake_all::current_job().filter(|j| j.outcome().is_none() && j.label().starts_with(&format!("{} - ", now.map)));
    let (step, part) = match job {
        Some(j) => {
            let (done, total) = j.counts();
            let why = j.why();
            let why = if why.is_empty() { String::new() } else { format!(" ({why})") };
            // (The label is "<game> - <map> - <step>".)
            let name = j.label().strip_prefix(&format!("{} - ", now.map)).unwrap_or(j.label()).trim().to_owned();
            if total > 0 {
                (format!("{name} {done}/{total}{why}"), done as f32 / total as f32)
            } else {
                (format!("{name}{why}"), 0.0)
            }
        }
        None => ("checking...".to_owned(), 0.0),
    };
    let left = left_text(assets::bake_all::eta_left());
    Some(Bar { title: format!("Map {} of {}: {}", now.index, now.of, now.map), step, done: overall(now.index, now.of, part), left })
}

/// The whole run's share done: the maps before this one, and this one's part.
fn overall(index: usize, of: usize, part: f32) -> f32 {
    if of == 0 {
        return 0.0;
    }
    ((index.saturating_sub(1) as f32 + part.clamp(0.0, 1.0)) / of as f32).clamp(0.0, 1.0)
}

/// For the view's redraw key: changes when the bar would look different
/// (the counts in whole percent, the time left), or PREPARE MAPS' counts.
pub(crate) fn key() -> String {
    let rev = super::options::counts_rev();
    bar().map_or(format!("{rev}"), |b| format!("{rev}|{}|{}|{}|{}", b.title, b.step.split('/').next().unwrap_or(""), (b.done * 1000.0) as u32, b.left))
}

/// Draws the bar while a run is going.
pub(crate) fn draw(c: &mut Kids, cx: &Ctx) {
    let Some(b) = bar() else { return };
    c.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: cx.px(6.0),
            padding: UiRect::axes(cx.px(18.0), cx.px(10.0)),
            border: UiRect::all(cx.px(1.0)),
            ..abs(Some(14.0), None, Some(14.0), Some(124.0), cx)
        },
        BackgroundColor(cx.pal.panel),
        BorderColor::all(cx.pal.line),
    ))
    .with_children(|p| {
        p.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: cx.px(16.0), ..default() }).with_children(|top| {
            top.spawn((Text::new(format!("PREPARING MAPS   {}", b.title)), cx.bold(20.0), TextColor(WHITE)));
            top.spawn(Node { column_gap: cx.px(24.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                if !b.left.is_empty() {
                    r.spawn((Text::new(b.left.to_uppercase()), cx.bold(18.0), TextColor(WHITE)));
                }
                r.spawn((Text::new("ESC: STOP"), cx.bold(18.0), TextColor(GOLD)));
            });
        });
        p.spawn(Node { width: Val::Percent(100.0), height: cx.px(10.0), ..default() }).insert(BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12))).with_child((
            Node { width: Val::Percent(b.done * 100.0), height: Val::Percent(100.0), ..default() },
            BackgroundColor(ACCENT),
        ));
        let pct = (b.done * 100.0).floor() as u32;
        let line = if b.step.is_empty() { format!("{pct}% of all maps") } else { format!("{}   ({pct}% of all maps)", b.step) };
        p.spawn((Text::new(line), cx.regular(16.0), TextColor(DIM)));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_counts_the_whole_run() {
        assert_eq!(overall(1, 4, 0.0), 0.0);
        assert_eq!(overall(1, 4, 1.0), 0.25);
        assert_eq!(overall(3, 4, 0.5), 0.625);
        assert_eq!(overall(4, 4, 2.0), 1.0);
        assert_eq!(overall(1, 0, 0.5), 0.0);
        assert!(bar().is_none() && !key().contains('|'), "no bar with no run");
        use assets::bake_all::Eta;
        assert_eq!(left_text(None), "");
        assert_eq!(left_text(Some(Eta { secs: 600.0, untimed: 0 })), "about 10 min left");
        assert_eq!(left_text(Some(Eta { secs: 0.0, untimed: 3 })), "3 maps left, not timed yet");
        assert_eq!(left_text(Some(Eta { secs: 125.0, untimed: 2 })), "about 3 min left + 2 maps not timed yet");
    }
}
