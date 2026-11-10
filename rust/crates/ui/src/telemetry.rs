//! bo2zm: the Draw FPS overlay, in the style of Modern Warfare III's
//! telemetry: off, FPS only ("Simple"), or a full panel with frame time, a
//! rolling frame-time graph (green under 16.7 ms, yellow under 33, red above),
//! the average and 1% low frame rate, and the CPU time of the frame. It is a
//! separate overlay on its own layer: the match HUD is not touched.
//!
//! Not shown, because the game has no source for them: GPU time (no timestamp
//! queries), ping and packet loss (the net crate keeps no round-trip value).
//!
//! Cost: the graph is one small image rewritten each frame (no layout); the
//! text is refreshed four times a second.

use std::time::Instant;

use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use frame::ClientSet;

use crate::layers::{GameUiFont, UiLayer, UiLayerVisibility, game_text_font};

/// Frames kept for the average and the 1% low (about 4 s at 60 fps).
const RING: usize = 240;
/// Columns of the graph, one frame each (about 2 s at 60 fps).
const GRAPH_W: usize = 120;
const GRAPH_H: usize = 36;
/// Frame time at the top of the graph.
const GRAPH_TOP_MS: f32 = 40.0;
const GOOD_MS: f32 = 16.8;
const OK_MS: f32 = 33.4;
/// Text refresh interval (4 Hz).
const TEXT_EVERY: f32 = 0.25;

/// Rolling frame-time statistics over the last `RING` frames.
#[derive(Clone)]
pub(crate) struct FrameStats {
    ms: [f32; RING],
    len: usize,
    head: usize,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self {
            ms: [0.0; RING],
            len: 0,
            head: 0,
        }
    }
}

impl FrameStats {
    pub(crate) fn push(&mut self, ms: f32) {
        self.ms[self.head] = ms;
        self.head = (self.head + 1) % RING;
        self.len = (self.len + 1).min(RING);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.len
    }

    /// The `i`-th newest frame time (0 = newest).
    fn newest(&self, i: usize) -> Option<f32> {
        (i < self.len).then(|| self.ms[(self.head + RING - 1 - i) % RING])
    }

    pub(crate) fn avg_ms(&self) -> f32 {
        if self.len == 0 {
            return 0.0;
        }
        // The ring fills from slot 0 until full, so the first `len` slots are the samples.
        self.ms[..self.len].iter().sum::<f32>() / self.len as f32
    }

    pub(crate) fn avg_fps(&self) -> f32 {
        let ms = self.avg_ms();
        if ms > 0.0 { 1000.0 / ms } else { 0.0 }
    }

    /// The 1% low: the frame rate of the slowest 1% of frames (at least one).
    pub(crate) fn one_percent_low_fps(&self) -> f32 {
        if self.len == 0 {
            return 0.0;
        }
        let mut v: Vec<f32> = self.ms[..self.len].to_vec();
        v.sort_by(|a, b| b.total_cmp(a));
        let k = self.len.div_ceil(100).max(1);
        let worst = v[..k].iter().sum::<f32>() / k as f32;
        if worst > 0.0 { 1000.0 / worst } else { 0.0 }
    }
}

/// Colour tier of a frame time: 0 green, 1 yellow, 2 red.
fn tier(ms: f32) -> u8 {
    if ms < GOOD_MS {
        0
    } else if ms < OK_MS {
        1
    } else {
        2
    }
}

fn tier_rgb(t: u8) -> [u8; 3] {
    match t {
        0 => [120, 230, 130],
        1 => [245, 215, 90],
        _ => [245, 100, 90],
    }
}

fn tier_color(t: u8) -> Color {
    let [r, g, b] = tier_rgb(t);
    Color::srgb_u8(r, g, b)
}

/// Draw the graph (oldest column left) into RGBA bytes.
fn draw_graph(stats: &FrameStats, px: &mut [u8]) {
    px.fill(0);
    let mark =
        |ms: f32| GRAPH_H - ((ms / GRAPH_TOP_MS * GRAPH_H as f32) as usize).min(GRAPH_H - 1) - 1;
    for ms in [GOOD_MS, OK_MS] {
        let y = mark(ms);
        for x in 0..GRAPH_W {
            let o = (y * GRAPH_W + x) * 4;
            px[o..o + 4].copy_from_slice(&[255, 255, 255, 40]);
        }
    }
    for col in 0..GRAPH_W {
        let Some(ms) = stats.newest(GRAPH_W - 1 - col) else {
            continue;
        };
        let h = ((ms / GRAPH_TOP_MS * GRAPH_H as f32).ceil() as usize).clamp(1, GRAPH_H);
        let [r, g, b] = tier_rgb(tier(ms));
        for y in GRAPH_H - h..GRAPH_H {
            let o = (y * GRAPH_W + col) * 4;
            px[o..o + 4].copy_from_slice(&[r, g, b, 235]);
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct CpuClock {
    start: Option<Instant>,
    /// CPU time of the last finished frame, in ms.
    last_ms: f32,
}

pub(crate) fn stamp_cpu_start(mut clock: ResMut<CpuClock>) {
    clock.start = Some(Instant::now());
}

/// Runs at the end of the frame, before the frame cap sleeps.
pub(crate) fn record_cpu_end(mut clock: ResMut<CpuClock>) {
    if let Some(s) = clock.start {
        clock.last_ms = s.elapsed().as_secs_f32() * 1000.0;
    }
}

#[derive(Component)]
struct TelemetryRoot;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Line {
    Fps,
    Frame,
    Avg,
    Cpu,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Off,
    Simple,
    Full,
}

#[derive(Default)]
struct PanelState {
    stats: FrameStats,
    key: Option<(Mode, bool, u32)>,
    graph: Option<Handle<Image>>,
    text_at: f32,
}

#[allow(clippy::too_many_arguments)]
fn telemetry(
    mut commands: Commands,
    settings: Option<Res<frame::GameSettings>>,
    font: Option<Res<GameUiFont>>,
    window: Query<&Window, With<PrimaryWindow>>,
    time: Res<Time<Real>>,
    clock: Res<CpuClock>,
    roots: Query<Entity, With<TelemetryRoot>>,
    mut lines: Query<(&Line, &mut Text, &mut TextColor)>,
    images: Option<ResMut<Assets<Image>>>,
    mut st: Local<PanelState>,
) {
    let (mode, left) = match settings.as_deref() {
        Some(s) if s.draw_fps && s.fps_full => (Mode::Full, s.fps_left),
        Some(s) if s.draw_fps => (Mode::Simple, s.fps_left),
        _ => (Mode::Off, false),
    };
    if mode == Mode::Off {
        if st.key.take().is_some() {
            for e in &roots {
                commands.entity(e).despawn();
            }
        }
        return;
    }
    let (Some(font), Some(mut images)) = (font.as_deref(), images) else {
        return;
    };
    let dt = time.delta_secs();
    if dt > 0.0 {
        st.stats.push(dt * 1000.0);
    }
    let sh = window.single().map_or(1080.0, |w| w.height());
    let scale = (sh / 1080.0).max(0.4);
    let key = (mode, left, (scale * 100.0) as u32);
    let now = time.elapsed_secs();

    if st.key != Some(key) {
        for e in &roots {
            commands.entity(e).despawn();
        }
        st.key = Some(key);
        st.text_at = f32::MIN;
        let graph = st
            .graph
            .get_or_insert_with(|| {
                images.add(Image::new(
                    Extent3d {
                        width: GRAPH_W as u32,
                        height: GRAPH_H as u32,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    vec![0; GRAPH_W * GRAPH_H * 4],
                    TextureFormat::Rgba8UnormSrgb,
                    bevy::asset::RenderAssetUsages::default(),
                ))
            })
            .clone();
        let size = 15.0 * scale;
        let line = |c: &mut ChildSpawnerCommands, l: Line, big: bool| {
            c.spawn((
                l,
                Text::new(""),
                game_text_font(&font.0, if big { size * 1.5 } else { size }),
                TextColor(Color::WHITE),
            ));
        };
        let (h_edge, v_edge) = (Val::Px(10.0 * scale), Val::Px(8.0 * scale));
        let (l_edge, r_edge) = if left {
            (h_edge, Val::Auto)
        } else {
            (Val::Auto, h_edge)
        };
        commands
            .spawn((
                TelemetryRoot,
                UiLayer::Hud,
                UiLayerVisibility,
                ZIndex(50),
                Node {
                    position_type: PositionType::Absolute,
                    left: l_edge,
                    right: r_edge,
                    top: v_edge,
                    padding: UiRect::axes(Val::Px(8.0 * scale), Val::Px(5.0 * scale)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0 * scale),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.03, 0.04, 0.05, 0.62)),
            ))
            .with_children(|c| {
                line(c, Line::Fps, true);
                if mode == Mode::Full {
                    line(c, Line::Frame, false);
                    c.spawn((
                        ImageNode::new(graph),
                        Node {
                            width: Val::Px(GRAPH_W as f32 * 1.5 * scale),
                            height: Val::Px(GRAPH_H as f32 * 1.5 * scale),
                            ..default()
                        },
                    ));
                    line(c, Line::Avg, false);
                    line(c, Line::Cpu, false);
                }
            });
        return;
    }

    if mode == Mode::Full
        && let Some(h) = st.graph.clone()
        && let Some(mut img) = images.get_mut(&h)
        && let Some(data) = img.data.as_mut()
    {
        draw_graph(&st.stats, data);
    }

    if now - st.text_at < TEXT_EVERY {
        return;
    }
    st.text_at = now;
    let avg_ms = st.stats.avg_ms();
    let newest = st.stats.newest(0).unwrap_or(avg_ms);
    for (line, mut text, mut color) in &mut lines {
        let (s, c) = match line {
            Line::Fps => (
                format!("{:.0} FPS", st.stats.avg_fps()),
                tier_color(tier(avg_ms)),
            ),
            Line::Frame => (format!("Frame {newest:5.1} ms"), tier_color(tier(newest))),
            Line::Avg => (
                format!(
                    "Avg {:.0}   1% low {:.0}",
                    st.stats.avg_fps(),
                    st.stats.one_percent_low_fps()
                ),
                Color::srgb(0.85, 0.87, 0.9),
            ),
            Line::Cpu => (
                format!("CPU {:5.1} ms", clock.last_ms),
                Color::srgb(0.85, 0.87, 0.9),
            ),
        };
        if text.0 != s {
            text.0 = s;
        }
        color.0 = c;
    }
}

pub(crate) fn register_telemetry_systems(app: &mut App) {
    app.init_resource::<CpuClock>()
        .add_systems(First, stamp_cpu_start)
        .add_systems(Last, record_cpu_end)
        .add_systems(Update, telemetry.in_set(ClientSet::Ui));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(vals: &[f32]) -> FrameStats {
        let mut s = FrameStats::default();
        for v in vals {
            s.push(*v);
        }
        s
    }

    #[test]
    fn average_of_steady_frames() {
        let s = filled(&[10.0; 50]);
        assert!((s.avg_ms() - 10.0).abs() < 1e-4);
        assert!((s.avg_fps() - 100.0).abs() < 1e-2);
    }

    #[test]
    fn average_rolls_over_the_ring() {
        let mut s = filled(&[100.0; RING]);
        for _ in 0..RING {
            s.push(5.0);
        }
        assert_eq!(s.len(), RING);
        assert!((s.avg_ms() - 5.0).abs() < 1e-4);
        assert_eq!(s.newest(0), Some(5.0));
    }

    #[test]
    fn one_percent_low_sees_the_spikes() {
        // 200 frames: 197 at 10 ms and 3 at 50 ms. 1% of 200 = 2 frames,
        // both 50 ms, so the 1% low is 20 fps while the average stays high.
        let mut v = vec![10.0; 197];
        v.extend([50.0; 3]);
        let s = filled(&v);
        assert!((s.one_percent_low_fps() - 20.0).abs() < 1e-2);
        assert!(s.avg_fps() > 90.0);
    }

    #[test]
    fn one_percent_low_of_few_frames_is_the_worst_one() {
        let s = filled(&[10.0, 20.0, 40.0]);
        assert!((s.one_percent_low_fps() - 25.0).abs() < 1e-3);
    }

    #[test]
    fn empty_stats_are_zero() {
        let s = FrameStats::default();
        assert_eq!(s.avg_fps(), 0.0);
        assert_eq!(s.one_percent_low_fps(), 0.0);
        assert_eq!(s.newest(0), None);
    }

    #[test]
    fn tiers_follow_the_thresholds() {
        assert_eq!(tier(16.6), 0);
        assert_eq!(tier(20.0), 1);
        assert_eq!(tier(33.0), 1);
        assert_eq!(tier(50.0), 2);
    }

    #[test]
    fn graph_draws_newest_frame_at_the_right_edge() {
        let s = filled(&[10.0, 60.0]);
        let mut px = vec![0u8; GRAPH_W * GRAPH_H * 4];
        draw_graph(&s, &mut px);
        let at = |x: usize, y: usize| px[(y * GRAPH_W + x) * 4..(y * GRAPH_W + x) * 4 + 4].to_vec();
        // the 60 ms frame fills the whole last column, in red
        assert_eq!(at(GRAPH_W - 1, 0)[0], 245);
        assert_eq!(at(GRAPH_W - 1, 0)[1], 100);
        // the 10 ms frame is green and short
        assert_eq!(at(GRAPH_W - 2, GRAPH_H - 1)[1], 230);
        assert_eq!(at(GRAPH_W - 2, 0)[3], 0);
    }
}
