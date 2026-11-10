//! bo2mp: what a scope sees besides its picture, while it is aimed.
//!
//! - Dual Band (`ir`) and the other games' thermal scopes: the world goes
//!   dark and every player in sight shows white hot (Cold Blooded stays
//!   cold); walls still hide him.
//! - Millimeter Scanner (`mms`): the sight's screen, centred on the aim,
//!   scans its own field of view and shows the enemies in it out to its
//!   range, through walls.
//!
//! Both are drawn on the scope root (the match world's camera), under the
//! scope picture and its surround, so a scope's frame stays on top. The
//! world's darkening is a veil over the frame, not Black Ops II's
//! per-material thermal pass, and a person is a soft figure the size of his
//! bounds, not his model.

use assets::PreparedWeapons;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::{Display, FocusPolicy, ZIndex};
use net::{CEntity, CEntityRuntime, LocalPresentClient, PresentedSnapshot, ViewweaponAim};
use weapon_iw4::get_viewmodel_weapon_index;

use crate::presentation_scale::{PresentationScale, ScaleClass};
use crate::ui_write::adopt_display;

/// What the aimed scope shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ScopeVision {
    Thermal,
    Scanner(asset_game::MmsScanner),
}

pub(crate) fn scope_vision_of(facts: &asset_game::WeaponBodyFacts) -> Option<ScopeVision> {
    if facts.thermal_scope {
        Some(ScopeVision::Thermal)
    } else {
        facts.mms.map(ScopeVision::Scanner)
    }
}

#[derive(Component, Clone, Copy)]
pub(crate) struct ThermalVeil;

#[derive(Component, Clone, Copy)]
pub(crate) struct ScannerScreen;

#[derive(Component, Clone, Copy)]
pub(crate) struct ScopeFigure(u8);

const FIGURES: u8 = 24;
/// A figure is this wide for its height.
const FIGURE_WIDTH: f32 = 0.42;
/// A standing player's bounds above his origin, past his eyes.
const HEAD_ROOM: f32 = 12.0;
/// The scanner's screen: its height on the frame (the sight's small screen,
/// or the scope's).
const SCREEN_HEIGHT: f32 = 0.36;
const SCREEN_HEIGHT_IN_SCOPE: f32 = 0.72;

const VEIL: [f32; 4] = [0.025, 0.03, 0.035, 0.4];
const HOT: [f32; 4] = [1.0, 1.0, 0.96, 0.95];
const SCREEN: [f32; 4] = [0.05, 0.11, 0.13, 0.94];
const SCAN_LINES: [f32; 4] = [0.4, 0.8, 0.9, 0.3];
const BLIP: [f32; 4] = [1.0, 0.9, 0.55, 0.95];

fn rgba(c: [f32; 4], alpha: f32) -> Color {
    Color::srgba(c[0], c[1], c[2], c[3] * alpha)
}

/// A figure's rect (left, top, width, height) from his feet, the top of
/// his bounds and a point a side step off the top, all on the frame.
pub(crate) fn figure_rect(feet: Vec2, top: Vec2, side: Vec2) -> Option<[f32; 4]> {
    if !(feet.is_finite() && top.is_finite() && side.is_finite()) {
        return None;
    }
    let height = (feet.y - top.y).abs().max(feet.distance(top) * 0.5);
    let width = (side.distance(top) * 2.0).max(height * FIGURE_WIDTH);
    if height < 1.0 {
        return None;
    }
    let cx = (feet.x + top.x) * 0.5;
    let y0 = feet.y.min(top.y);
    Some([cx - width * 0.5, y0, width, height])
}

/// Where a point in the scanner's own view space (x right, y up, z ahead)
/// lands on its screen, -1..1 across and up; None outside its field of view
/// (`fov_deg` across, `aspect` its width over its height).
pub(crate) fn scanner_point(view: Vec3, fov_deg: f32, aspect: f32) -> Option<Vec2> {
    if view.z <= 1.0 {
        return None;
    }
    let tan_x = (fov_deg.to_radians() * 0.5).tan().max(1e-4);
    let tan_y = tan_x / aspect.max(1e-3);
    let p = Vec2::new(view.x / view.z / tan_x, view.y / view.z / tan_y);
    (p.x.abs() <= 1.0 && p.y.abs() <= 1.0).then_some(p)
}

/// The scanner's screen on a frame `width` x `height`, centred at
/// `center`: (left, top, width, height).
pub(crate) fn scanner_screen(
    width: f32,
    height: f32,
    center: Vec2,
    mms: &asset_game::MmsScanner,
) -> [f32; 4] {
    let h = height
        * if mms.in_scope {
            SCREEN_HEIGHT_IN_SCOPE
        } else {
            SCREEN_HEIGHT
        };
    let w = (h * mms.aspect).min(width);
    let h = w / mms.aspect;
    [center.x - w * 0.5, center.y - h * 0.5, w, h]
}

/// A soft white figure: a head over a rounded body, its alpha falling off
/// to the edges.
pub(crate) fn figure_pixels(width: u32, height: u32) -> Vec<u8> {
    let mut out = vec![0u8; (width * height * 4) as usize];
    let (w, h) = (width as f32, height as f32);
    for y in 0..height {
        for x in 0..width {
            let u = (x as f32 + 0.5) / w * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / h;
            // Head: a circle in the top fifth; body: a capsule below it.
            let head = {
                let dy = (v - 0.1) / 0.1;
                let dx = u / 0.45;
                1.0 - (dx * dx + dy * dy).sqrt()
            };
            let body = {
                let dx = u.abs() / (0.95 - 0.35 * (v - 0.2).max(0.0));
                let dy = if v < 0.24 {
                    (0.24 - v) / 0.06
                } else if v > 0.98 {
                    (v - 0.98) / 0.02
                } else {
                    0.0
                };
                1.0 - (dx * dx + dy * dy).sqrt()
            };
            let a = (head.max(body) * 2.5).clamp(0.0, 1.0);
            let i = ((y * width + x) * 4) as usize;
            out[i..i + 4].copy_from_slice(&[255, 255, 255, (a * 255.0).round() as u8]);
        }
    }
    out
}

/// The scanner screen's lines: a faint line every fourth row.
pub(crate) fn scan_line_pixels(width: u32, height: u32) -> Vec<u8> {
    let mut out = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        let a = if y % 4 == 0 { 255 } else { 40 };
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            out[i..i + 4].copy_from_slice(&[255, 255, 255, a]);
        }
    }
    out
}

fn picture(width: u32, height: u32, pixels: Vec<u8>, images: &mut Assets<Image>) -> Handle<Image> {
    images.add(Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ))
}

fn hidden() -> (Node, FocusPolicy) {
    (
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            ..default()
        },
        FocusPolicy::Pass,
    )
}

pub(crate) fn spawn_scope_vision(root: &mut ChildSpawnerCommands) {
    let (node, focus) = hidden();
    root.spawn((
        ThermalVeil,
        node,
        focus,
        BackgroundColor(Color::NONE),
        ZIndex(0),
    ));
    let (node, focus) = hidden();
    root.spawn((
        ScannerScreen,
        node,
        focus,
        ImageNode::default(),
        BackgroundColor(Color::NONE),
        ZIndex(0),
    ));
    for i in 0..FIGURES {
        let (node, focus) = hidden();
        root.spawn((ScopeFigure(i), node, focus, ImageNode::default(), ZIndex(0)));
    }
}

#[derive(Default)]
pub(crate) struct ScopeVisionPictures {
    figure: Option<Handle<Image>>,
    lines: Option<Handle<Image>>,
}

type VeilQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Node, &'static mut BackgroundColor),
    (
        With<ThermalVeil>,
        Without<ScannerScreen>,
        Without<ScopeFigure>,
    ),
>;
type ScreenQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Node,
        &'static mut ImageNode,
        &'static mut BackgroundColor,
    ),
    (
        With<ScannerScreen>,
        Without<ThermalVeil>,
        Without<ScopeFigure>,
    ),
>;
type FigureQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static ScopeFigure,
        &'static mut Node,
        &'static mut ImageNode,
    ),
    (Without<ThermalVeil>, Without<ScannerScreen>),
>;

/// The match world's camera (the scope root draws on it).
type WorldLens<'w, 's> = Query<
    'w,
    's,
    (&'static Camera, &'static GlobalTransform),
    (
        With<Camera3d>,
        With<bevy::camera::CompositingSpace>,
        Without<frame::UiCamera>,
    ),
>;

fn hide_all(veil: &mut VeilQuery, screen: &mut ScreenQuery, figures: &mut FigureQuery) {
    for (mut node, mut bg) in veil.iter_mut() {
        adopt_display(&mut node, Display::None);
        *bg = BackgroundColor(Color::NONE);
    }
    for (mut node, _, _) in screen.iter_mut() {
        adopt_display(&mut node, Display::None);
    }
    for (_, mut node, _) in figures.iter_mut() {
        adopt_display(&mut node, Display::None);
    }
}

fn place(node: &mut Node, rect: [f32; 4]) {
    adopt_display(node, Display::Flex);
    node.left = Val::Px(rect[0].round());
    node.top = Val::Px(rect[1].round());
    node.width = Val::Px(rect[2].round().max(1.0));
    node.height = Val::Px(rect[3].round().max(1.0));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_scope_vision(
    surface: Res<crate::surface::Hud2dSurface>,
    presented: Res<PresentedSnapshot>,
    local: Res<LocalPresentClient>,
    weapons: Option<Res<PreparedWeapons>>,
    aim: Res<ViewweaponAim>,
    cameras: WorldLens,
    players: Query<(&CEntity, &CEntityRuntime)>,
    prediction: Option<Res<net::ClientPredictionState>>,
    mut images: ResMut<Assets<Image>>,
    mut pictures: Local<ScopeVisionPictures>,
    mut logged: Local<Option<(u32, bool)>>,
    (mut veil, mut screen, mut figures): (VeilQuery, ScreenQuery, FigureQuery),
) {
    let Some(ps) = presented.player(local.0) else {
        hide_all(&mut veil, &mut screen, &mut figures);
        return;
    };
    let weapon = get_viewmodel_weapon_index(ps);
    let vision = weapons
        .as_deref()
        .and_then(|w| w.0.facts_of(weapon))
        .and_then(|f| scope_vision_of(&f));
    let alpha = crate::sight::sight_alpha(ps.f_weapon_pos_frac);
    let seen = (weapon, vision.is_some() && alpha > 0.0);
    if *logged != Some(seen) {
        if codlegacy_env::var_os("CODLEGACY_SCOPELOG").is_some() {
            let name = weapons.as_deref().map_or("", |w| w.0.name_of(weapon));
            diag::info!(Ui, "scope: {name} vision {vision:?} aimed {}", seen.1);
        }
        *logged = Some(seen);
    }
    let (Some(vision), Some(snapshot), Some((camera, eye))) = (
        vision,
        presented.snapshot(),
        cameras.iter().find(|(c, _)| c.is_active),
    ) else {
        hide_all(&mut veil, &mut screen, &mut figures);
        return;
    };
    if !surface.is_ready() || alpha <= 0.0 || ps.pm_type >= playerstate_iw4::PM_TYPE_DEAD {
        hide_all(&mut veil, &mut screen, &mut figures);
        return;
    }
    let figure = pictures
        .figure
        .get_or_insert_with(|| picture(32, 64, figure_pixels(32, 64), &mut images))
        .clone();
    let view = PresentationScale::from_window(surface.width(), surface.height());
    let local_team = snapshot
        .meta
        .for_client(local.0)
        .map_or(0, |m| m.client_state_team);
    let eye_at = eye.translation();
    let (right, up, ahead) = (*eye.right(), *eye.up(), *eye.forward());
    let world = prediction
        .as_ref()
        .filter(|p| p.0.is_armed() && p.0.world().has_world_clip())
        .map(|p| p.0.world());
    // Everyone alive but him, with his bounds' height.
    let mut people: Vec<(Vec3, f32, bool, bool)> = Vec::new();
    for (identity, runtime) in &players {
        if !runtime.in_next_snap() || runtime.next_state.e_type != entity_iw4::ET_PLAYER {
            continue;
        }
        let Some(client) = identity.client() else {
            continue;
        };
        if client == local.0 {
            continue;
        }
        let Some(meta) = snapshot.meta.for_client(client) else {
            continue;
        };
        if meta.lifecycle != sim::ClientLifecycle::Alive {
            continue;
        }
        let Some((_, other)) = snapshot.players.iter().find(|(id, _)| *id == client) else {
            continue;
        };
        let enemy = local_team == 0 || meta.client_state_team != local_team;
        let cold = other.perks[0] & playerstate_iw4::PERK_COLDBLOODED != 0;
        let height = (other.view_height_current + HEAD_ROOM).max(16.0);
        people.push((Vec3::from_array(runtime.origin), height, enemy, cold));
    }
    let mut placed: Vec<([f32; 4], Color)> = Vec::new();
    match vision {
        ScopeVision::Thermal => {
            for (mut node, mut bg) in veil.iter_mut() {
                place(&mut node, [0.0, 0.0, view.width(), view.height()]);
                *bg = BackgroundColor(rgba(VEIL, alpha));
            }
            for (mut node, _, _) in screen.iter_mut() {
                adopt_display(&mut node, Display::None);
            }
            for &(origin, height, _, cold) in &people {
                if cold || (origin - eye_at).dot(ahead) <= 0.0 {
                    continue;
                }
                let top = origin + Vec3::Z * height;
                // Heat does not pass walls: his chest or head must be seen.
                if let Some(world) = world {
                    let seen = [origin + Vec3::Z * height * 0.6, top - Vec3::Z * 4.0]
                        .iter()
                        .any(|p| {
                            let hit = world.trace_world(
                                eye_at.to_array(),
                                p.to_array(),
                                [0.0; 3],
                                [0.0; 3],
                                hud_iw4::OVERHEAD_TRACE_MASK,
                            );
                            hit.fraction >= 1.0 && hit.startsolid == 0
                        });
                    if !seen {
                        continue;
                    }
                }
                let (Ok(feet), Ok(head), Ok(side)) = (
                    camera.world_to_viewport(eye, origin),
                    camera.world_to_viewport(eye, top),
                    camera.world_to_viewport(eye, top + right * 16.0),
                ) else {
                    continue;
                };
                if let Some(rect) = figure_rect(feet, head, side) {
                    placed.push((rect, rgba(HOT, alpha)));
                }
            }
        }
        ScopeVision::Scanner(mms) => {
            for (mut node, mut bg) in veil.iter_mut() {
                adopt_display(&mut node, Display::None);
                *bg = BackgroundColor(Color::NONE);
            }
            let factor = view.factor(ScaleClass::ProjectionBound);
            let center = Vec2::new(view.width() * 0.5, view.height() * 0.5)
                + if aim.live {
                    Vec2::new(aim.xhair_x, aim.xhair_y) * factor
                } else {
                    Vec2::ZERO
                };
            let panel = scanner_screen(view.width(), view.height(), center, &mms);
            let lines = pictures
                .lines
                .get_or_insert_with(|| picture(4, 64, scan_line_pixels(4, 64), &mut images))
                .clone();
            for (mut node, mut image, mut bg) in screen.iter_mut() {
                place(&mut node, panel);
                image.image = lines.clone();
                image.color = rgba(SCAN_LINES, alpha);
                *bg = BackgroundColor(rgba(SCREEN, alpha));
            }
            let tan_y = (mms.fov.to_radians() * 0.5).tan().max(1e-4) / mms.aspect.max(1e-3);
            for &(origin, height, enemy, _) in &people {
                if !enemy || origin.distance(eye_at) > mms.max_dist {
                    continue;
                }
                let to = |p: Vec3| {
                    let d = p - eye_at;
                    Vec3::new(d.dot(right), d.dot(up), d.dot(ahead))
                };
                let (feet, top) = (to(origin), to(origin + Vec3::Z * height));
                let mid = (feet + top) * 0.5;
                let Some(at) = scanner_point(mid, mms.fov, mms.aspect) else {
                    continue;
                };
                let h = (height / mid.z.max(1.0) / tan_y * panel[3] * 0.5).clamp(3.0, panel[3]);
                let w = (h * FIGURE_WIDTH).max(3.0);
                let cx = panel[0] + (at.x + 1.0) * 0.5 * panel[2];
                let cy = panel[1] + (1.0 - at.y) * 0.5 * panel[3];
                placed.push(([cx - w * 0.5, cy - h * 0.5, w, h], rgba(BLIP, alpha)));
            }
        }
    }
    for (slot, mut node, mut image) in figures.iter_mut() {
        let Some((rect, color)) = placed.get(usize::from(slot.0)) else {
            adopt_display(&mut node, Display::None);
            continue;
        };
        place(&mut node, *rect);
        image.image = figure.clone();
        image.color = *color;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thermal_or_scanner_weapon_has_its_vision() {
        let mut f = asset_game::WeaponBodyFacts::default();
        assert_eq!(scope_vision_of(&f), None);
        f.mms = Some(asset_game::MmsScanner::default());
        assert!(matches!(scope_vision_of(&f), Some(ScopeVision::Scanner(_))));
        f.thermal_scope = true;
        assert_eq!(scope_vision_of(&f), Some(ScopeVision::Thermal));
    }

    #[test]
    fn a_figure_stands_between_his_feet_and_head() {
        let r = figure_rect(
            Vec2::new(100.0, 300.0),
            Vec2::new(100.0, 200.0),
            Vec2::new(110.0, 200.0),
        )
        .expect("rect");
        assert_eq!(r[1], 200.0);
        assert_eq!(r[3], 100.0);
        assert!((r[2] - 42.0).abs() < 1e-3, "{r:?}");
        assert!((r[0] + r[2] * 0.5 - 100.0).abs() < 1e-3);
        // Too small to see, or off the frame's maths.
        assert!(figure_rect(Vec2::ZERO, Vec2::new(0.0, 0.2), Vec2::ZERO).is_none());
        assert!(figure_rect(Vec2::NAN, Vec2::ZERO, Vec2::ZERO).is_none());
    }

    #[test]
    fn the_scanner_sees_only_its_field_of_view() {
        // Straight ahead: the screen's middle.
        assert_eq!(
            scanner_point(Vec3::new(0.0, 0.0, 500.0), 40.0, 1.333),
            Some(Vec2::ZERO)
        );
        // At the edge of a 90 degree field: the screen's side.
        let p = scanner_point(Vec3::new(499.0, 0.0, 500.0), 90.0, 1.0).expect("in view");
        assert!((p.x - 0.998).abs() < 1e-3);
        // Behind, or outside the field.
        assert!(scanner_point(Vec3::new(0.0, 0.0, -10.0), 40.0, 1.0).is_none());
        assert!(scanner_point(Vec3::new(600.0, 0.0, 500.0), 90.0, 1.0).is_none());
        // Above: up on the screen.
        let p = scanner_point(Vec3::new(0.0, 100.0, 500.0), 60.0, 1.5).expect("in view");
        assert!(p.y > 0.0);
    }

    #[test]
    fn the_scanner_screen_keeps_its_shape_on_the_frame() {
        let mms = asset_game::MmsScanner::authored(40.0, 2.0, 1000.0, false);
        let r = scanner_screen(1920.0, 1080.0, Vec2::new(960.0, 540.0), &mms);
        assert!((r[2] / r[3] - 2.0).abs() < 1e-3);
        assert!((r[3] - 1080.0 * SCREEN_HEIGHT).abs() < 1e-3);
        assert!((r[0] + r[2] * 0.5 - 960.0).abs() < 1e-3);
        // A very wide screen on a narrow frame fits its width.
        let wide = asset_game::MmsScanner::authored(40.0, 10.0, 1000.0, true);
        let r = scanner_screen(640.0, 1080.0, Vec2::new(320.0, 540.0), &wide);
        assert!(r[2] <= 640.0 + 1e-3);
    }

    #[test]
    fn a_figure_is_solid_in_the_middle_and_clear_at_the_corners() {
        let px = figure_pixels(32, 64);
        let alpha = |x: u32, y: u32| px[((y * 32 + x) * 4 + 3) as usize];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(31, 0), 0);
        assert!(alpha(16, 32) > 200, "body");
        assert!(alpha(16, 6) > 200, "head");
        let lines = scan_line_pixels(4, 8);
        assert_eq!(lines[3], 255);
        assert_eq!(lines[4 * 4 + 3], 40);
    }

    #[test]
    fn a_bolt_action_falls_back_to_a_bolt_action_scope() {
        assert_eq!(crate::iris::fallback_scope_order(true)[0], "dsr50_mp");
        assert_eq!(crate::iris::fallback_scope_order(false)[0], "svu_mp");
    }
}
