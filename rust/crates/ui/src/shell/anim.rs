//! bo2mp shell: the front end's motion, as Ghosts' menus move: panels and
//! rows slide in from the left over a fifth of a second (easing out), the
//! band of the row in focus widens in from the left when the focus moves,
//! map cards rise into place, and a line that is still working ("SEARCHING
//! FOR GAMES...") pulses.
//!
//! The shell's screens are drawn again whenever what they show changes (a
//! picture coming in, the online numbers), so the motion is not keyed to
//! when a node was spawned: [`note`] (called by `view::shell_view`) keeps
//! when the screen (or its page) was opened and when the focus last moved,
//! and a node redrawn half way through its slide starts where it was. Once
//! done, [`animate`] leaves a node alone (no change detection fires).

use std::sync::Mutex;

use bevy::prelude::*;

/// How long a slide or a band's growth takes, in seconds.
pub(crate) const DUR: f32 = 0.2;

/// Slides a node to its place from `from` (physical pixels, added to its
/// translation) between `start` and `start + dur` (`Time::elapsed_secs`),
/// easing out.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct SlideIn {
    pub from: Vec2,
    pub start: f32,
    pub dur: f32,
}

/// Widens a node from nothing to its full width (`Val::Percent(100)`) from
/// its left edge, easing out (the band of the row in focus).
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct Grow {
    pub start: f32,
    pub dur: f32,
}

/// Pulses a text's alpha (a line that is still working).
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct Pulse {
    pub base: Color,
}

struct Clock {
    now: f32,
    screen: String,
    screen_at: f32,
    focus: usize,
    focus_at: f32,
}

static CLOCK: Mutex<Clock> = Mutex::new(Clock { now: 0.0, screen: String::new(), screen_at: -10.0, focus: usize::MAX, focus_at: -10.0 });

fn clock<R>(f: impl FnOnce(&mut Clock) -> R) -> R {
    let mut g = CLOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&mut g)
}

/// Before a screen is drawn: which screen (and page) it is, the row in
/// focus, and the time. A new screen restarts the slides; a new focus
/// restarts the focus band.
pub(crate) fn note(screen: &str, focus: usize, now: f32) {
    clock(|c| {
        c.now = now;
        if c.screen != screen {
            c.screen = screen.to_owned();
            c.screen_at = now;
            c.focus = focus;
            // (On a new screen the band grows once its row is in.)
            c.focus_at = now + DUR * 0.5;
        } else if c.focus != focus {
            c.focus = focus;
            c.focus_at = now;
        }
    });
}

/// 0..1 eased out (cubic).
pub(crate) fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn progress(start: f32, dur: f32, now: f32) -> f32 {
    if dur <= 0.0 { 1.0 } else { ease_out((now - start) / dur) }
}

/// A slide from `from` (design units: `s` is the shell's scale), starting
/// `delay` seconds after the screen opened: the component, and the
/// transform to spawn with (where it is now, so a redraw does not jump).
pub(crate) fn slide(s: f32, from: Vec2, delay: f32) -> (SlideIn, UiTransform) {
    clock(|c| {
        let a = SlideIn { from: from * s, start: c.screen_at + delay, dur: DUR };
        let k = 1.0 - progress(a.start, a.dur, c.now);
        (a, UiTransform { translation: Val2::px(a.from.x * k, a.from.y * k), ..UiTransform::IDENTITY })
    })
}

/// Rows and panels: in from the left by `dx` design units.
pub(crate) fn from_left(s: f32, dx: f32, delay: f32) -> (SlideIn, UiTransform) {
    slide(s, Vec2::new(-dx, 0.0), delay)
}

/// Map cards: up from `dy` design units below.
pub(crate) fn rise(s: f32, dy: f32, delay: f32) -> (SlideIn, UiTransform) {
    slide(s, Vec2::new(0.0, dy), delay)
}

/// The focus band: the component, and the width to spawn with.
pub(crate) fn grow() -> (Grow, Val) {
    clock(|c| {
        let g = Grow { start: c.focus_at, dur: DUR };
        (g, Val::Percent(100.0 * progress(g.start, g.dur, c.now)))
    })
}

/// Moves every slide, band and pulse on screen.
pub(crate) fn animate(
    time: Res<Time>,
    mut slides: Query<(&mut UiTransform, &SlideIn)>,
    mut grows: Query<(&mut Node, &Grow)>,
    mut pulses: Query<(&mut TextColor, &Pulse)>,
) {
    let now = time.elapsed_secs();
    for (mut tf, a) in &mut slides {
        let k = 1.0 - progress(a.start, a.dur, now);
        let want = Val2::px(a.from.x * k, a.from.y * k);
        if tf.translation != want {
            tf.translation = want;
        }
    }
    for (mut node, g) in &mut grows {
        let want = Val::Percent(100.0 * progress(g.start, g.dur, now));
        if node.width != want {
            node.width = want;
        }
    }
    for (mut ink, p) in &mut pulses {
        let wave = 0.5 + 0.5 * (now * std::f32::consts::TAU / 1.4).cos();
        let a = p.base.alpha() * (0.40 + 0.60 * wave);
        ink.0 = p.base.with_alpha(a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eases_out_and_clamps() {
        assert_eq!(ease_out(-1.0), 0.0);
        assert_eq!(ease_out(2.0), 1.0);
        assert!(ease_out(0.5) > 0.5);
        assert_eq!(progress(1.0, 0.2, 5.0), 1.0);
        assert_eq!(progress(1.0, 0.0, 0.0), 1.0);
    }
}
