//! bo2mp: Black Ops III's out-of-bounds screen (`CoD.OutOfBounds`, the LUI
//! widgets `ui/uieditor/widgets/mphudwidgets/outofbounds/*.lua` in
//! mp_patch), drawn while the server's client dvar `bo2mp_oob`
//! (`<ms left>,<ms spent>`, `sim::t6::oob`) is set. It also switches the
//! native renderer's BO3 filter (`frame::settings::BO3_OOB`).
//!
//! Layout from the widgets' constants (BO3's 1280x720 screen, from its
//! centre), read off the compiled LUI:
//! - the timer (`outofbounds_timerwidget`, FoundryGridnik-Bold): x -51.5..55.5,
//!   y -108..-60, alpha 0.9, the end time `HUDItems.outOfBoundsEndTime`; its
//!   red copy (`Timer00`, RGB 0.65, 0, 0) grows to twice its size and fades
//!   over 699 ms each second;
//! - the warning box (`outofbounds_warningboxwidget`): its frame
//!   `uie_t7_hud_outofbounds_centerbox` x -332.38..332.38, y -94.73..94.73,
//!   RGB 0.73, alpha 0.5; "WARNING" (`MPUI_OUT_OF_BOUNDS_WARNING`,
//!   FoundryGridnik-Medium, letter spacing 7.5, alpha 0.9) over its middle;
//! - the return box (`outofbounds_returnbox`), x -218.5..216.5, y
//!   23.5..53.5 of the warning box: a black box at alpha 0.7 and "RETURN TO
//!   COMBAT ZONE" (`MPUI_OUT_OF_BOUNDS_RETURN_TO_COMBAT_ZONE`, 24 px of its
//!   30, letter spacing 3.9, alpha 0.95);
//! - the vignette (`outofbounds_VignetteContainer`) fading to alpha 0.2 over
//!   3930 ms.
//!
//! APPROXIMATED: Black Ops II's fonts stand in for FoundryGridnik, letter
//! spacing by spaces, the centre box's and vignette's pictures by plain
//! fills; the scroll bars, arrows, side bars and dots are not drawn.
//! UNVERIFIED: the timer's text (`setupEndTimer`; here `0:0S`, whole seconds
//! up).

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use frame::ClientSet;
use net::{LocalPresentClient, PresentedSnapshot};

use crate::layers::{UiLayer, UiLayerVisibility};

/// The server's client dvar (`sim::t6::oob::OOB_DVAR`).
pub const OOB_DVAR: &str = "bo2mp_oob";

/// `MPUI_OUT_OF_BOUNDS_WARNING` (en_core_patch.ff).
pub const WARNING: &str = "WARNING";
/// `MPUI_OUT_OF_BOUNDS_RETURN_TO_COMBAT_ZONE` (en_core_patch.ff).
pub const RETURN: &str = "RETURN TO COMBAT ZONE";

#[derive(Component)]
struct OobScreen;

/// `<ms left>,<ms spent>` from the dvar.
pub fn parse(v: &str) -> Option<(i64, i64)> {
    let (l, s) = v.split_once(',')?;
    Some((l.trim().parse().ok()?, s.trim().parse().ok()?))
}

/// The timer's text: minutes and seconds, the seconds rounded up.
pub fn timer_text(left_ms: i64) -> String {
    let s = (left_ms.max(0) + 999) / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The vignette's alpha `spent_ms` in (0 to 0.2 over 3930 ms).
pub fn vignette_alpha(spent_ms: i64) -> f32 {
    0.2 * (spent_ms as f32 / 3930.0).clamp(0.0, 1.0)
}

/// The red timer copy's pulse within each second: (scale, alpha).
pub fn pulse(spent_ms: i64) -> (f32, f32) {
    let t = (spent_ms.rem_euclid(1000) as f32 / 699.0).min(1.0);
    (1.0 + t, 1.0 - t)
}

fn spaced(s: &str) -> String {
    s.chars().map(String::from).collect::<Vec<_>>().join(" ")
}

#[allow(clippy::too_many_arguments)]
fn oob_screen(
    mut commands: Commands,
    presented: Option<Res<PresentedSnapshot>>,
    local: Option<Res<LocalPresentClient>>,
    window: Query<&Window, With<PrimaryWindow>>,
    old: Query<Entity, With<OobScreen>>,
    bo2: Option<Res<crate::bo2_font::Bo2Fonts>>,
    mut last: Local<String>,
) {
    let value = match (presented.as_deref(), local.as_deref()) {
        (Some(p), Some(l)) => p.snapshot().and_then(|snap| snap.meta.script_dvars(l.0).string(OOB_DVAR).map(str::to_owned)),
        _ => None,
    }
    .unwrap_or_default();
    let state = parse(&value);
    frame::settings::BO3_OOB.store(state.is_some(), std::sync::atomic::Ordering::Relaxed);
    if *last == value && state.is_none() {
        return;
    }
    if value.is_empty() != last.is_empty() {
        diag::info!(Ui, "bo2mp out of bounds (BO3) screen: {}", if value.is_empty() { "down" } else { "up" });
    }
    value.clone_into(&mut last);
    for e in &old {
        commands.entity(e).despawn();
    }
    let Some((left, spent)) = state else { return };
    let (sw, sh) = window.single().map_or((1920.0, 1080.0), |w| (w.width(), w.height()));
    let k = (sh / 720.0).min(sw / 1280.0);
    let (cx, cy) = (sw * 0.5, sh * 0.5);
    // A box in BO3's units from the screen's centre.
    let node = |x0: f32, x1: f32, y0: f32, y1: f32| Node {
        position_type: PositionType::Absolute,
        left: Val::Px(cx + x0 * k),
        top: Val::Px(cy + y0 * k),
        width: Val::Px((x1 - x0) * k),
        height: Val::Px((y1 - y0) * k),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    let root = commands
        .spawn((
            OobScreen,
            UiLayer::Hud,
            UiLayerVisibility,
            Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Px(sw), height: Val::Px(sh), ..default() },
            ZIndex(-850),
        ))
        .id();
    let text = |c: &mut ChildSpawnerCommands, s: &str, px: f32, color: Color| {
        if let Some(b) = bo2.as_deref() {
            b.spawn_line(c, "Default", &[(s.to_owned(), color)], px, 0.0);
        } else {
            c.spawn((Text::new(s), TextFont { font_size: bevy::text::FontSize::Px(px), ..default() }, TextColor(color)));
        }
    };
    commands.entity(root).with_children(|p| {
        // The vignette (a plain fill for its picture).
        p.spawn((BackgroundColor(Color::srgba(0.0, 0.0, 0.0, vignette_alpha(spent))), node(-640.0, 640.0, -360.0, 360.0)));
        // The warning box's frame (the centre box picture's edge).
        let (bx, by) = (1.0, -2.5);
        p.spawn((
            Node { border: UiRect::all(Val::Px((2.0 * k).max(1.0))), ..node(bx - 332.38, bx + 332.38, by - 94.73, by + 94.73) },
            BorderColor::all(Color::srgba(0.73, 0.73, 0.73, 0.5)),
            BackgroundColor(Color::srgba(0.73, 0.73, 0.73, 0.06)),
        ));
        // WARNING over the box's middle.
        p.spawn(node(bx - 332.38, bx + 332.38, by - 58.5, by + 27.5)).with_children(|c| text(c, &spaced(WARNING), 64.0 * k, Color::srgba(1.0, 1.0, 1.0, 0.9)));
        // The return box under it.
        p.spawn((BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)), node(bx - 218.5, bx + 216.5, by + 23.5, by + 53.5)))
            .with_children(|c| text(c, RETURN, 24.0 * k, Color::srgba(1.0, 1.0, 1.0, 0.95)));
        // The timer, and its red copy pulsing each second.
        let t = timer_text(left);
        let (grow, fade) = pulse(spent);
        p.spawn(node(-51.5, 55.5, -108.0, -60.0)).with_children(|c| text(c, &t, 48.0 * k, Color::srgba(1.0, 1.0, 1.0, 0.9)));
        let (mx, my, hw, hh) = (2.0, -84.0, 53.5 * grow, 24.0 * grow);
        p.spawn(node(mx - hw, mx + hw, my - hh, my + hh)).with_children(|c| text(c, &t, 48.0 * k * grow, Color::srgba(0.65, 0.0, 0.0, fade)));
    });
}

pub(crate) fn register(app: &mut App) {
    app.add_systems(Update, oob_screen.in_set(ClientSet::Ui));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dvar_and_timer() {
        assert_eq!(parse("2400,600"), Some((2400, 600)));
        assert_eq!(parse(""), None);
        assert_eq!(timer_text(3000), "0:03");
        assert_eq!(timer_text(2100), "0:03");
        assert_eq!(timer_text(900), "0:01");
        assert_eq!(timer_text(0), "0:00");
    }

    #[test]
    fn vignette_and_pulse_follow_the_widgets() {
        assert_eq!(vignette_alpha(0), 0.0);
        assert!((vignette_alpha(3930) - 0.2).abs() < 1e-6);
        assert_eq!(pulse(0), (1.0, 1.0));
        assert_eq!(pulse(699), (2.0, 0.0));
        assert_eq!(pulse(1000), (1.0, 1.0));
    }
}
